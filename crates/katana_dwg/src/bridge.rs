//! The one module that sees acadrust (docs/dwg.md, "Where it sits"): a DWG's bytes to the DXF
//! text katana_dxf's reader reads, and the DXF text katana_dxf's writer writes to a DWG's bytes.
//! Every acadrust call this crate makes is here, written against acadrust 0.6.8 as published;
//! the file and line each relies on is named beside it.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

use acadrust::io::dxf::code_page::{
    dwg_code_page_index, encode_legacy_string, encoding_from_dwg_code_page,
};
use acadrust::objects::{ObjectType, associative_canonical_name};
use acadrust::xdata::{ExtendedData, XDataValue};
use acadrust::{
    CadDocument, Color, DwgReadOptions, DwgReader, DwgWriter, DxfReader, DxfReaderConfiguration,
    DxfVersion, DxfWriter, EntityType, ReadOutcome, ReadStats, Vector3,
};
use katana_core::{Error, ErrorCode, Result};
use katana_dxf::{APPLICATION_NAME, EXTENDED_COLOUR, EXTENDED_HEIGHTS, EXTENDED_PATH};

use crate::release::DwgRelease;

/// The diagnostic acadrust gives a record it kept whole without decoding it - not a loss
/// (io/dwg/dwg_reader.rs:1495 passes it by in the same way).
const PRESERVED: &str = "auxiliary-object-preserved";

/// How many of acadrust's diagnostics about the file as a whole (a section, the header) are
/// said one by one; the rest are counted. A damaged file can give hundreds (acadrust keeps up
/// to MAX_READ_DIAGNOSTICS, 1 000, io/read.rs:4), and a reply of hundreds of warnings is read
/// by nobody.
const FILE_DIAGNOSTICS_SAID: usize = 8;

/// `work`, with a panic inside acadrust caught and said as the failure it is (`code`). Its DWG
/// reader catches its own (io/dwg/dwg_reader.rs:1168-1179); its DXF reader and writer and its
/// DWG writer do not, and CADCraft's bridge guards its calls the same way.
fn guarded<T>(code: ErrorCode, doing: &str, work: impl FnOnce() -> T) -> Result<T> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|payload| {
        let said = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|text| String::from(*text)))
            .unwrap_or_default();
        Error::new(code, format!("acadrust failed {doing}: {said}"))
    })
}

// ---- DWG -> DXF -------------------------------------------------------------------------------

/// What `dwg_to_dxf` hands the DXF reader, and what it has to say about it first.
pub(crate) struct Decoded {
    /// UTF-8 DXF text (R2007's or later: see `dwg_to_dxf`).
    pub(crate) dxf: Vec<u8>,
    pub(crate) warnings: Vec<String>,
}

fn damaged(release: &str, why: &str) -> Error {
    Error::new(
        ErrorCode::ParseFailure,
        format!(
            "the {release} DWG file is damaged, and nothing of it could be read (acadrust: {why})"
        ),
    )
}

/// The DXF of the DWG `bytes`, a file of `release` (its name, "R2018", for the messages).
///
/// acadrust reads it in failsafe mode (`DwgReadOptions::failsafe`, io/dwg/dwg_reader.rs:330),
/// which gives back what it could decode and says what it could not; a file whose header or
/// section map cannot be read comes back with `ReadStats::record_stream_read` false
/// (dwg_reader.rs:1280-1307, 1736-1747), and that is the one failure: nothing of it can be read.
/// CADCraft's bridge draws the line at the same place.
///
/// The DXF is written as R2007 at the oldest (`DxfWriter::with_version`,
/// io/dxf/writer/mod.rs:148), because from R2007 acadrust writes a DXF's strings as UTF-8
/// whatever the drawing's code page (writer/mod.rs:201-206), and UTF-8 is what katana_dxf's
/// reader reads as it is; an older DXF
/// would be in the drawing's code page, which that reader reads as Windows-1252. acadrust has
/// already decoded the DWG's strings by its code page.
pub(crate) fn dwg_to_dxf(bytes: &[u8], release: &str) -> Result<Decoded> {
    // io/dwg/dwg_reader.rs:1042 (from_stream_with_options) and 1146 (read_with_stats).
    let mut reader =
        DwgReader::from_stream_with_options(Cursor::new(bytes), DwgReadOptions::failsafe());
    let ReadOutcome {
        mut document,
        stats,
    } = reader
        .read_with_stats()
        .map_err(|error| damaged(release, &format!("{error}")))?;
    if !stats.record_stream_read {
        let why = stats
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.trim_start_matches("Failsafe: "))
            .unwrap_or("its section map could not be read");
        return Err(damaged(release, why));
    }
    let mut warnings = read_warnings(&stats);
    let left_out = leave_out_what_is_not_a_number(&mut document);
    if left_out != 0 {
        warnings.push(format!(
            "{left_out} entities with a coordinate, size or angle that is not a number were left \
             out: the file is damaged there"
        ));
    }
    leave_out_undecoded_constraint_groups(&mut document);
    // The DXF below is UTF-8 (see above), so the page it would declare means nothing to the
    // reader - which warns of a code page that is not Windows-1252 (katana_dxf's reader.rs, its
    // last warnings) - and the declaration is the one that reader takes as no news.
    document.header.code_page = String::from("ANSI_1252");
    let version = if document.version < DxfVersion::AC1021 {
        DxfVersion::AC1021
    } else {
        document.version
    };
    // io/dxf/writer/mod.rs:124 (new), 148 (with_version), 252 (write_to_vec).
    let dxf = guarded(ErrorCode::FileImportFailure, "reading the DWG", || {
        DxfWriter::new(&document).with_version(version).write_to_vec()
    })?
    .map_err(|error| {
        Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "the {release} DWG was read, and acadrust could not write it out for the \
                 reader: {error}"
            ),
        )
    })?;
    Ok(Decoded { dxf, warnings })
}

/// acadrust's account of the read (io/read.rs:100-118) as warnings: the records it could not
/// decode, counted by kind as the DXF's tally counts entities, then what it could not read of
/// the file as a whole, in its own words.
fn read_warnings(stats: &ReadStats) -> Vec<String> {
    let mut warnings = Vec::new();
    if stats.skipped_source_records != 0 {
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for diagnostic in &stats.diagnostics {
            if diagnostic.record_handle.is_some() && diagnostic.code != PRESERVED {
                let kind = diagnostic.record_type.as_deref().unwrap_or("unnamed");
                *kinds.entry(kind).or_insert(0) += 1;
            }
        }
        let by_kind: Vec<String> = kinds
            .iter()
            .map(|(kind, count)| format!("{kind} {count}"))
            .collect();
        warnings.push(format!(
            "{} of the {} records of the DWG could not be decoded and were left out{}",
            stats.skipped_source_records,
            stats.source_records,
            if by_kind.is_empty() {
                String::new()
            } else {
                format!(" ({})", by_kind.join(", "))
            }
        ));
    }
    let of_the_file: Vec<&str> = stats
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.record_handle.is_none() && diagnostic.code != PRESERVED)
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    for message in of_the_file.iter().take(FILE_DIAGNOSTICS_SAID) {
        warnings.push(format!("acadrust: {message}"));
    }
    if of_the_file.len() > FILE_DIAGNOSTICS_SAID {
        warnings.push(format!(
            "and {} more of acadrust's notes on the file",
            of_the_file.len() - FILE_DIAGNOSTICS_SAID
        ));
    }
    warnings
}

fn finite(point: &Vector3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

/// Whether an entity of a kind the DXF reader reads holds a number that is not one. acadrust's
/// DXF writer writes such a number as 0.0 (io/dxf/writer/text_writer.rs:94-101), where the DXF
/// reader would have refused it, and the entity would come in at the origin; a DWG holds raw
/// doubles, so a damaged one can hold anything. The fields are acadrust's (entities/point.rs,
/// line.rs, circle.rs, arc.rs, ellipse.rs, lwpolyline.rs, polyline.rs, polyline3d.rs, text.rs,
/// mtext.rs, insert.rs).
fn holds_what_is_not_a_number(entity: &EntityType) -> bool {
    let fine = match entity {
        EntityType::Point(point) => finite(&point.location) && finite(&point.normal),
        EntityType::Line(line) => finite(&line.start) && finite(&line.end) && finite(&line.normal),
        EntityType::Circle(circle) => {
            finite(&circle.center) && circle.radius.is_finite() && finite(&circle.normal)
        }
        EntityType::Arc(arc) => {
            finite(&arc.center)
                && arc.radius.is_finite()
                && arc.start_angle.is_finite()
                && arc.end_angle.is_finite()
                && finite(&arc.normal)
        }
        EntityType::Ellipse(ellipse) => {
            finite(&ellipse.center)
                && finite(&ellipse.major_axis)
                && ellipse.minor_axis_ratio.is_finite()
                && ellipse.start_parameter.is_finite()
                && ellipse.end_parameter.is_finite()
                && finite(&ellipse.normal)
        }
        EntityType::LwPolyline(polyline) => {
            polyline.elevation.is_finite()
                && finite(&polyline.normal)
                && polyline.vertices.iter().all(|vertex| {
                    vertex.location.x.is_finite()
                        && vertex.location.y.is_finite()
                        && vertex.bulge.is_finite()
                })
        }
        EntityType::Polyline2D(polyline) => {
            polyline.elevation.is_finite()
                && finite(&polyline.normal)
                && polyline
                    .vertices
                    .iter()
                    .all(|vertex| finite(&vertex.location) && vertex.bulge.is_finite())
        }
        EntityType::Polyline3D(polyline) => polyline
            .vertices
            .iter()
            .all(|vertex| finite(&vertex.position)),
        EntityType::Polyline(polyline) => polyline
            .vertices
            .iter()
            .all(|vertex| finite(&vertex.location)),
        EntityType::Text(text) => {
            finite(&text.insertion_point)
                && text.alignment_point.as_ref().is_none_or(finite)
                && text.height.is_finite()
                && text.rotation.is_finite()
                && finite(&text.normal)
        }
        EntityType::MText(text) => {
            finite(&text.insertion_point)
                && text.height.is_finite()
                && text.rotation.is_finite()
                && finite(&text.normal)
        }
        EntityType::Insert(insert) => {
            finite(&insert.insert_point)
                && insert.x_scale().is_finite()
                && insert.y_scale().is_finite()
                && insert.z_scale().is_finite()
                && insert.rotation.is_finite()
                && insert.column_spacing.is_finite()
                && insert.row_spacing.is_finite()
                && finite(&insert.normal)
        }
        // A kind the DXF reader does not read is counted in its tally whatever it holds.
        _ => true,
    };
    !fine
}

/// Takes out every entity `holds_what_is_not_a_number` finds (`CadDocument::remove_entity`,
/// document.rs:3808), block definitions' included; how many.
fn leave_out_what_is_not_a_number(document: &mut CadDocument) -> usize {
    let doomed: Vec<_> = document
        .entities()
        .filter(|entity| holds_what_is_not_a_number(entity))
        .map(|entity| entity.common().handle)
        .collect();
    for handle in &doomed {
        let _ = document.remove_entity(*handle);
    }
    doomed.len()
}

/// acadrust's DXF writer refuses a document that holds a constraint group it could not decode
/// (io/dxf/writer/mod.rs:181-195). One draws nothing, and the DXF reader reads no objects, so
/// such a group is left out of the document the DXF is written from rather than lose the whole
/// drawing to it.
fn leave_out_undecoded_constraint_groups(document: &mut CadDocument) {
    document.objects.retain(|_, object| {
        !matches!(
            object,
            ObjectType::Unknown { type_name, raw_dxf_codes: None, .. }
                if associative_canonical_name(type_name.as_str()) == "ASSOC2DCONSTRAINTGROUP"
        )
    });
}

// ---- DXF -> DWG -------------------------------------------------------------------------------

/// acadrust's version for a release (types/mod.rs, `DxfVersion`), which decides the DWG its
/// writer writes (io/dwg/dwg_writer.rs:45).
fn dxf_version(release: DwgRelease) -> DxfVersion {
    match release {
        DwgRelease::R2000 => DxfVersion::AC1015,
        DwgRelease::R2004 => DxfVersion::AC1018,
        DwgRelease::R2007 => DxfVersion::AC1021,
        DwgRelease::R2010 => DxfVersion::AC1024,
        DwgRelease::R2013 => DxfVersion::AC1027,
        DwgRelease::R2018 => DxfVersion::AC1032,
    }
}

/// katana_dxf's writer writes ASCII, every other character as \U+XXXX, under $DWGCODEPAGE
/// ANSI_1252 (katana_dxf's writer.rs, its header). acadrust's DXF reader turns those escapes
/// into the characters only when the file names no code page: with one, it keeps an escape of
/// a character that page holds as the seven characters of the escape (io/dxf/code_page.rs:
/// 536-546, called from io/dxf/reader/text_reader.rs:83-92), and the DWG would hold "\U+00E9"
/// where the drawing has "é". So the declaration is left out of the text acadrust reads; its
/// document then keeps its own default, ANSI_1252 (document.rs:1152), for a DWG before R2007.
fn without_code_page(dxf: &str) -> String {
    const DECLARATION: &str = "  9\n$DWGCODEPAGE\n  3\nANSI_1252\n";
    dxf.replacen(DECLARATION, "", 1)
}

/// The colour katana_dxf's writer put beside an entity or a layer whose colour no index is
/// exactly (the KATANA extended data's `colour` and its "#RRGGBB"), read as katana_dxf's reader
/// reads it: after `path` comes one value, after `elevations` a list in braces.
fn katana_colour(xdata: &ExtendedData) -> Option<Color> {
    let record = xdata.get_record(APPLICATION_NAME)?;
    let mut word: Option<&str> = None;
    for value in &record.values {
        match value {
            XDataValue::ControlString(brace) if brace == "}" => word = None,
            XDataValue::String(text) => match word {
                Some(EXTENDED_COLOUR) => {
                    let colour = katana_entity::Color::from_hex(text).ok()?;
                    return Some(Color::from_rgb(colour.r, colour.g, colour.b));
                }
                Some(EXTENDED_PATH) => word = None,
                // A piece of the list of heights.
                Some(_) => {}
                None if text == EXTENDED_PATH
                    || text == EXTENDED_COLOUR
                    || text == EXTENDED_HEIGHTS =>
                {
                    word = Some(text.as_str());
                }
                None => {}
            },
            _ => {}
        }
    }
    None
}

/// From R2004 a DWG holds a true colour (`DwgVersion::supports_true_color`,
/// io/dwg/dwg_version.rs:66-69): the colour katana_dxf's writer could give R2000's DXF only as
/// the nearest index, with itself beside it, becomes the colour of the entity or layer, so that
/// every program draws it as Katana does. The extended data stays, and Katana reads the same
/// colour from either.
fn exact_colours(document: &mut CadDocument) {
    // document.rs:4188 (entities_mut); entities/mod.rs:846-853 (common, common_mut).
    for entity in document.entities_mut() {
        if let Some(colour) = katana_colour(&entity.common().extended_data) {
            entity.common_mut().color = colour;
        }
    }
    // A layer's extended data is the document's, by the layer's handle (document.rs:1372),
    // where acadrust's DXF reader puts it (io/dxf/reader/section_reader.rs:9722-9730, 21800-21830)
    // and its DWG writer takes it from (io/dwg/dwg_stream_writers/object_writer/common.rs:830-836).
    for layer in document.layers.iter_mut() {
        if let Some(colour) = document
            .object_xdata
            .get(&layer.handle)
            .and_then(katana_colour)
        {
            layer.color = colour;
        }
    }
}

/// Before R2007 a DWG holds an extended data string behind a length of one byte: acadrust
/// writes the length as a u8 (io/dwg/eed_codec.rs:63-67), so a string of more than 255 bytes
/// would corrupt the record rather than be refused. katana_dxf's writer cuts a list of heights
/// into pieces of 250 characters; a layer's path is one string, and may be longer (a path is
/// up to 512 bytes, `katana_entity::layer_path::MAXIMUM_LAYER_NAME_LENGTH`). Each string is
/// measured as the writer will encode it, in the drawing's code page (as
/// object_writer/common.rs:1016-1021 chooses it).
fn fits_extended_data(document: &CadDocument, release: DwgRelease) -> Result<()> {
    let encoding = encoding_from_dwg_code_page(dwg_code_page_index(&document.header.code_page));
    let longest = document
        .entities()
        .map(|entity| &entity.common().extended_data)
        .chain(document.object_xdata.values())
        .flat_map(|data| data.records())
        .flat_map(|record| &record.values)
        .filter_map(|value| match value {
            XDataValue::String(text) | XDataValue::LayerName(text) => {
                Some(encode_legacy_string(text, encoding).len())
            }
            _ => None,
        })
        .max()
        .unwrap_or(0);
    if longest > 255 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "an extended data string of {longest} bytes - a layer's path, written beside its \
                 name - is longer than the 255 bytes an {} DWG holds; write R2007 or later \
                 (release=R2018)",
                release.name()
            ),
        ));
    }
    Ok(())
}

/// What `dxf_to_dwg` wrote, and what it has to say about it.
pub(crate) struct Encoded {
    pub(crate) bytes: Vec<u8>,
    pub(crate) warnings: Vec<String>,
}

/// The DWG of `dxf`, katana_dxf's writer's text, as `release`.
///
/// acadrust reads the DXF in failsafe mode (`DxfReaderConfiguration::failsafe`,
/// io/dxf/reader.rs:30-51): the DXF is Katana's own, so a record it cannot read is a defect in
/// one of the two - and the drawing is still written, the records left out said in a warning,
/// rather than the whole export lost to one record. Its document is then given the release
/// (`CadDocument::version`, document.rs:1244, which its DWG writer writes - the same step
/// CADCraft's bridge takes, and acadrust's own tests, e.g. tests/unit_mode_single_source.rs:52).
pub(crate) fn dxf_to_dwg(dxf: &str, release: DwgRelease) -> Result<Encoded> {
    let text = without_code_page(dxf);
    let configuration = DxfReaderConfiguration {
        failsafe: true,
        ..DxfReaderConfiguration::default()
    };
    // io/dxf/reader.rs:64 (from_reader), 151 (with_configuration), 185 (read_with_stats).
    let ReadOutcome {
        mut document,
        stats,
    } = guarded(
        ErrorCode::FileExportFailure,
        "reading the DXF Katana wrote",
        || {
            DxfReader::from_reader(Cursor::new(text.into_bytes()))
                .map(|reader| reader.with_configuration(configuration))
                .and_then(DxfReader::read_with_stats)
        },
    )?
    .map_err(|error| {
        Error::new(
            ErrorCode::FileExportFailure,
            format!("acadrust could not read the DXF Katana wrote for the DWG: {error}"),
        )
    })?;
    if !stats.record_stream_read {
        let why = stats
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.as_str())
            .unwrap_or("nothing of it");
        return Err(Error::new(
            ErrorCode::FileExportFailure,
            format!("acadrust could not read the DXF Katana wrote for the DWG: {why}"),
        ));
    }
    let mut warnings = Vec::new();
    if stats.skipped_source_records != 0 {
        let first = stats
            .diagnostics
            .first()
            .map(|diagnostic| format!(" (the first: {})", diagnostic.message))
            .unwrap_or_default();
        warnings.push(format!(
            "acadrust could not read {} of the records of the DXF Katana wrote, and they are not \
             in the DWG{first}",
            stats.skipped_source_records
        ));
    }
    document.version = dxf_version(release);
    if release.has_true_colour() {
        exact_colours(&mut document);
    }
    if !release.is_unicode() {
        fits_extended_data(&document, release)?;
    }
    // io/dwg/dwg_writer.rs:172 (write_to_vec).
    let bytes = guarded(ErrorCode::FileExportFailure, "writing the DWG", || {
        DwgWriter::write_to_vec(&document)
    })?
    .map_err(|error| {
        Error::new(
            ErrorCode::FileExportFailure,
            format!("acadrust could not write the {} DWG: {error}", release.name()),
        )
    })?;
    Ok(Encoded { bytes, warnings })
}
