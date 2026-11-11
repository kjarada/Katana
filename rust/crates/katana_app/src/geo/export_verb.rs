//! EXPORT (docs/interop.md, "IMPORT, EXPORT, INFO, REFS and COPC on every
//! front end", "Export options"), the port of
//! src/katana_app/geo/export_verb.cpp: the drawing - or what the shared scope
//! and filter take of it - to a file by its extension: a DXF natively, an
//! archive (.12da) with the drawing's surfaces, any other through GDAL - run as
//! the one executor runs every verb.
//!
//! ```text
//! EXPORT <file> [<scope>] [layername=<n> | split=layer] [append]
//!        [crs=project|native|<code>] [co=K=V]... [lco=K=V]...
//!        [text=points|skip] [curve=<m>] [properties=yes|no] [PREVIEW]
//! EXPORT <file.las|.laz> [CLOUD <id|name>] [PREVIEW]
//! ```
//!
//! A .las or .laz is a reference point cloud's, not the drawing's: the points
//! the session holds, written by PDAL - the sample, when the import was
//! budgeted, which the reply says (GIS > Export Point Cloud runs this line).
//!
//! The scope is the one grammar (cad's parse_scope_words, resolved by
//! match_scope): no scope words is the whole drawing, as EXPORT always was.
//! The reply says what the scope took. The options after it are GDAL's
//! formats' own; a .dxf or a .12da takes the scope alone.
//!
//! The drawing is copied when the line is prepared, so the work writes on a
//! worker while the window stays live; the file is written beside its place
//! and moved there by the apply (staged_files.rs), so a cancelled EXPORT
//! writes nothing - an append included, which adds to a copy of the file.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(x > 0.0) refuses NaN with the rest; kept as it is."
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use katana_archive12d::ExportSurface;
use katana_cad::annotation::leader_presets::leader_context;
use katana_cad::scope_verbs::{
    ScopeSource, is_scope_word, match_scope, scope_record as cad_scope_record,
};
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, lowered};
use katana_core::{Error, ErrorCode, Result};
use katana_dxf::is_dxf_path;
use katana_entity::{EntityId, LeaderContext, Model};
use katana_gis::GdalDataset;
use katana_gis::formats::{OptionList, check_options};
use katana_gis::gdal_adapter::{describe_crs, driver_assumes_wgs84};
use katana_gis::reproject::{crs_epsg_code, crs_to_wkt};
use katana_interop::archive12d::export_archive12d;
use katana_interop::export::{export_point_cloud, export_vector, point_cloud_export_formats};
use katana_interop::import::kind_for_path;
use katana_interop::{Archive12dExportOptions, SourceKind, VectorExportOptions};
use katana_terrain::NamedSurface;

use super::bindings::Tokens;
use super::geo_verbs::{
    Context, Prepared, make_apply, make_work, stop_requested, system_message, trimmed_text,
};
use super::import_verb::file_name_text;
use super::replies::{value, yes_no};
use super::staged_files::StagedFiles;
use super::vector_support::{WordRules, choice_option, number_option, read_verb_words};
use crate::dxf_verbs::{dxf_export_records, write_dxf_export, write_dxf_export_scoped};
use crate::import_records::{path_from_text, path_text, record_text, rest_of_line, warning_text};

const USAGE: &str = "usage: EXPORT <file> [<scope>] [layername=<n> | split=layer] [append] \
                     [crs=project|native|<code>] [co=K=V]... [lco=K=V]... [text=points|skip] \
                     [curve=<m>] [properties=yes|no] [PREVIEW] | EXPORT <file.las|.laz> \
                     [CLOUD <id|name>] [PREVIEW]";

/// The option keys, lower case. co= and lco= are read apart: each may be
/// given more than once. layername= and not layer=: an option key is never a
/// WHERE key, and WHERE's LAYER= would be taken for it (vector_support.rs).
const OPTIONS: [&str; 6] = ["layername", "split", "crs", "text", "curve", "properties"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Dxf,
    Archive,
    Vector,
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Dxf => "dxf",
        Kind::Archive => "archive",
        Kind::Vector => "vector",
    }
}

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

/// Whether word `i` begins the words after the path: an option, a flag or a
/// scope word. A path runs to the first of them, so one with a blank still
/// reads without quotes.
fn begins_options(tokens: &Tokens, i: usize) -> bool {
    let word = &tokens[i];
    if let Some(equals) = word.find('=')
        && equals > 0
    {
        let key = word[..equals].to_ascii_lowercase();
        if key == "co" || key == "lco" || OPTIONS.contains(&key.as_str()) {
            return true;
        }
    }
    if tokens.quoted[i] {
        return false;
    }
    tokens.is(i, "APPEND")
        || tokens.is(i, "PREVIEW")
        || tokens.is(i, "CLOUD")
        || is_scope_word(word)
}

/// A point cloud's file: .las or .laz, what export_point_cloud writes
/// (interop's point_cloud_export_formats). A .copc.laz is not: COPC rewrites
/// a file whole, and a sample written as one would read as the survey.
fn cloud_format(file: &Path) -> Option<String> {
    let name = file_name_text(file).to_ascii_lowercase();
    if name.ends_with(".copc.laz") {
        return None;
    }
    point_cloud_export_formats()
        .into_iter()
        .find(|format| name.ends_with(&format!(".{}", format.extension)))
        .map(|format| format.extension)
}

/// EXPORT <file.las|.laz> [CLOUD <id|name>] [PREVIEW]: the reference cloud
/// the line names - or the one there is - as held, copied now for the worker
/// (the cloud is the reference data's, and could be removed while the job
/// runs), written beside its place and put there by the apply.
fn export_cloud(
    context: &Context<'_>,
    tokens: &Tokens,
    first: usize,
    file: &Path,
    format: &str,
) -> Result<Prepared> {
    let mut named = String::new();
    let mut preview = false;
    let mut i = first;
    while i < tokens.size() {
        if tokens.is(i, "CLOUD") && named.is_empty() && i + 1 < tokens.size() {
            i += 1;
            named = tokens[i].clone();
        } else if tokens.is(i, "PREVIEW") {
            preview = true;
        } else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a point cloud's export takes CLOUD <id|name> and PREVIEW; it writes the cloud as \
                 held, with no scope or format options",
                tokens[i].clone(),
            ));
        }
        i += 1;
    }
    let clouds = context.reference.point_clouds();
    let cloud = if named.is_empty() {
        if clouds.len() != 1 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                if clouds.is_empty() {
                    "there is no point cloud to export; IMPORT one first"
                } else {
                    "CLOUD <id|name> says which point cloud; REFS lists them"
                },
                path_text(file),
            ));
        }
        &clouds[0]
    } else {
        // By id first, then by name in any case, as SURFACE FROM CLOUD finds
        // one.
        let by_id = clouds.iter().find(|each| format!("{}", each.id) == named);
        let by_name = clouds
            .iter()
            .find(|each| equals_ignoring_case(each.name.as_bytes(), named.as_bytes()));
        match by_id.or(by_name) {
            Some(cloud) => cloud,
            None => {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "no point cloud has that id or name; REFS lists the reference layers",
                    named,
                ));
            }
        }
    };
    let sample = cloud.is_decimated();
    let record = format!(
        "export file={} kind=cloud format={format} cloud={} id={} points={} source_points={} \
         sample={}",
        value(&path_text(file)),
        value(&cloud.name),
        cloud.id,
        cloud.points.len(),
        cloud.source_point_count,
        yes_no(sample)
    );
    // Said, not asked: a headless run has nobody to ask, and the window asks
    // before it writes the line.
    let warning = if sample {
        format!(
            "\n{}",
            warning_text(
                "the cloud holds a sample of its file, and the sample is what is written; COPC \
                 <source> <destination.copc.laz> rewrites the whole file"
            )
        )
    } else {
        String::new()
    };
    let title = format!("EXPORT {}", file_name_text(file));
    if preview {
        return Ok(Prepared {
            title,
            reply: Some(format!("{record} preview=yes{warning}")),
            work: None,
        });
    }
    let held = cloud.clone();
    let reply = format!("exported{}{warning}", &record["export".len()..]);
    let file = file.to_path_buf();
    let work = make_work(move |stop, _progress| {
        if stop_requested(stop) {
            return Err(cancelled());
        }
        let staged = StagedFiles::beside(&file)?;
        if let Err(error) = export_point_cloud(&held, staged.write_to()) {
            return Err(staged.as_target(error));
        }
        if stop_requested(stop) {
            return Err(cancelled());
        }
        Ok(make_apply(move |_context| {
            staged.place()?;
            Ok(reply)
        }))
    });
    Ok(Prepared {
        title,
        reply: None,
        work: Some(work),
    })
}

/// What the work writes from: copies, never the Document.
struct Snapshot {
    model: Model,
    annotation_scale: f64,
    /// What a leader's note reads in a DXF: the drawing's surfaces and
    /// alignments, held as copies (cad's leader_presets).
    leaders: LeaderContext,
    /// A DXF of a scope: the entities it took. `model` stays whole until the
    /// worker has drawn the annotation from it (write_dxf_export_scoped),
    /// because a smart leader's note is read off the entity its tip is on,
    /// and the scope may have left that entity out. None: the whole drawing.
    /// The worker cuts `model` to it in place (no second copy of a big
    /// drawing), and runs once.
    only: Option<BTreeSet<EntityId>>,
    surfaces: Vec<NamedSurface>,
    /// The project's coordinate system: it goes into the file, and is what a
    /// KML or GPX is converted to longitude and latitude from (docs/interop.md,
    /// "Fidelity").
    projection_wkt: String,
    /// The GDAL formats' options, with the entities the scope took.
    vector: VectorExportOptions,
    /// Records said after the exported one: what the scope took, a
    /// conversion.
    extra: Vec<String>,
}

fn write(kind: Kind, snapshot: &mut Snapshot, to: &Path, named: &Path) -> Result<String> {
    let mut reply = match kind {
        Kind::Dxf => {
            let written = match &snapshot.only {
                Some(only) => write_dxf_export_scoped(
                    &mut snapshot.model,
                    snapshot.annotation_scale,
                    to,
                    &snapshot.leaders,
                    only,
                )?,
                None => write_dxf_export(
                    &snapshot.model,
                    snapshot.annotation_scale,
                    to,
                    &snapshot.leaders,
                )?,
            };
            dxf_export_records(&written, named)
        }
        Kind::Archive => {
            // An archive carries what the other formats cannot: the drawing's
            // surfaces go with it.
            let surfaces: Vec<ExportSurface<'_>> = snapshot
                .surfaces
                .iter()
                .map(|surface| ExportSurface {
                    name: surface.name.clone(),
                    surface: surface.surface.as_deref(),
                })
                .collect();
            let written = export_archive12d(
                &snapshot.model,
                &surfaces,
                to,
                &Archive12dExportOptions::default(),
                &[],
            )?;
            let mut reply = format!(
                "exported file={} kind=archive driver=12da entities={} skipped={} alignments={} \
                 surfaces={} bytes={}",
                record_text(&path_text(named)),
                written.entities_written,
                written.entities_skipped,
                written.alignments_written,
                written.surfaces_written,
                written.bytes_written
            );
            for warning in &written.warnings {
                reply.push('\n');
                reply += &warning_text(warning);
            }
            reply
        }
        Kind::Vector => {
            let options = VectorExportOptions {
                projection_wkt: snapshot.projection_wkt.clone(),
                ..snapshot.vector.clone()
            };
            let written = export_vector(&snapshot.model, to, &options)?;
            let layers = written.layers.join(",");
            // What the file says it is in, which is not always the project's:
            // a KML is in longitude and latitude, a LOCAL import in none.
            let crs = if written.projection_wkt.is_empty() {
                String::new()
            } else {
                describe_crs(&written.projection_wkt)
            };
            let mut reply = format!(
                "exported file={} kind=vector driver={} features={} skipped={} layers={} crs={}",
                record_text(&path_text(named)),
                record_text(&written.driver),
                written.features_written,
                written.entities_skipped,
                record_text(&layers),
                record_text(&crs)
            );
            if options.text_as_points {
                reply += &format!(" texts={}", written.texts_written);
            }
            if options.append {
                reply += " append=yes";
            }
            for warning in &written.warnings {
                reply.push('\n');
                reply += &warning_text(warning);
            }
            reply
        }
    };
    // The exported record first, as it always was: a reader of the first
    // record keeps finding it there.
    let mut extra = String::new();
    for record in &snapshot.extra {
        extra.push('\n');
        extra += record;
    }
    let end = reply.find('\n').unwrap_or(reply.len());
    reply.insert_str(end, &extra);
    Ok(reply)
}

/// The verb, its row in the verb table's I0 block (verb_table.rs).
pub fn prepare_export(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    // The path, then the words after it; with nothing after it the line reads
    // as it always did (rest_of_line), a path with blanks included.
    if tokens.size() < 2 {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let mut first = 2;
    let mut path = if tokens.quoted[1] {
        tokens[1].clone()
    } else {
        while first < tokens.size() && !begins_options(tokens, first) {
            first += 1;
        }
        tokens.words[1..first].join(" ")
    };
    if first >= tokens.size() {
        path = rest_of_line(line);
    }
    if trimmed_text(&path).is_empty() || begins_options(tokens, 1) {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let file: PathBuf = path_from_text(&path);
    if let Some(format) = cloud_format(&file) {
        return export_cloud(context, tokens, first, &file, &format);
    }
    let kind = if is_dxf_path(&file) {
        Kind::Dxf
    } else if kind_for_path(&file) == SourceKind::Archive12d {
        Kind::Archive
    } else {
        Kind::Vector
    };

    // co= and lco= may be given more than once: taken out before the rest.
    let mut rest = Tokens::default();
    let mut creation: Vec<String> = Vec::new();
    let mut layer_creation: Vec<String> = Vec::new();
    for (word, &quoted) in tokens.words[first..].iter().zip(&tokens.quoted[first..]) {
        let folded = lowered(&word.as_bytes()[..word.len().min(4)]);
        if folded.starts_with(b"co=") {
            creation.push(String::from(&word[3..]));
            continue;
        }
        if folded.starts_with(b"lco=") {
            layer_creation.push(String::from(&word[4..]));
            continue;
        }
        rest.words.push(word.clone());
        rest.quoted.push(quoted);
    }
    let rules = WordRules {
        options: OPTIONS.map(String::from).to_vec(),
        flags: ["APPEND", "PREVIEW"].map(String::from).to_vec(),
        usage: String::from(USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(&rest, 0, rest.size(), &rules)?;
    let preview = words.has("PREVIEW");

    // A DXF and an archive take the scope alone: GDAL's options are for
    // GDAL's formats. Refused by name rather than ignored.
    if kind != Kind::Vector {
        let given = if let Some(key) = words.options.keys().next() {
            format!("{key}=")
        } else if !creation.is_empty() {
            String::from("co=")
        } else if !layer_creation.is_empty() {
            String::from("lco=")
        } else if words.has("APPEND") {
            String::from("append")
        } else {
            String::new()
        };
        if !given.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "{given} is an option of a GDAL format; a {} export takes a scope and PREVIEW",
                    kind_word(kind)
                ),
                path_text(&file),
            ));
        }
    }

    let mut vector = VectorExportOptions::default();
    let mut extra: Vec<String> = Vec::new();
    let project = context.document.metadata().coordinate_system.clone();
    let mut driver = String::new();
    if kind == Kind::Vector {
        driver = GdalDataset::vector_driver_for_path(&file)?;
        if let Some(name) = words.option("layername") {
            let name = trimmed_text(name);
            if name.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "layername= names the file's layer",
                ));
            }
            vector.layer_name = String::from(name);
        }
        if let Some(split) = words.option("split") {
            if !split.eq_ignore_ascii_case("layer") {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "split=layer writes one file layer per drawing layer",
                    format!("split={split}"),
                ));
            }
            if words.option("layername").is_some() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "split=layer names each file layer after its drawing layer: give it without \
                     layername=",
                ));
            }
            vector.split_by_layer = true;
        }
        vector.append = words.has("APPEND");
        if vector.append
            && let Some(option) = creation.first()
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "co= are the options of a new file, and append adds to one that is there: give \
                 lco= for the new layer's",
                format!("co={option}"),
            ));
        }
        check_options(&driver, OptionList::Creation, &creation)?;
        check_options(&driver, OptionList::LayerCreation, &layer_creation)?;
        vector.creation_options = creation;
        vector.layer_creation_options = layer_creation;
        let text = choice_option(&words, "text", &["points", "skip"], "skip")?;
        vector.text_as_points = text == "points";
        let properties = choice_option(&words, "properties", &["yes", "no"], "yes")?;
        vector.properties_as_attributes = properties == "yes";
        if let Some(curve) = number_option(&words, "curve")? {
            if !(curve > 0.0) {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "curve= is the largest distance a chord may stray from its arc, above 0",
                ));
            }
            vector.curve_tolerance = curve;
        }

        // crs=: the project's by default; native keeps it where the format
        // allows; a code moves the coordinates on the way out. A GeoJSON by
        // default is written as RFC 7946 (section 4) says it is - longitude
        // and latitude on WGS 84 - converted from the project's, where GDAL's
        // writer would write projected coordinates in a file most readers
        // take for degrees.
        let asked = words
            .option("crs")
            .map(|crs| String::from(trimmed_text(crs)))
            .unwrap_or_default();
        let folded = asked.to_ascii_lowercase();
        // native: the project's, unconverted.
        if folded != "native" {
            if !asked.is_empty() && folded != "project" {
                crs_to_wkt(&asked)?;
                if project.is_empty() {
                    return Err(Error::new(
                        ErrorCode::InvalidCrs,
                        format!(
                            "crs={asked} moves the drawing's coordinates, and the project has no \
                             coordinate system to move them from: CRS SET <code> first"
                        ),
                    ));
                }
                vector.target_crs = asked;
            } else if driver_assumes_wgs84(&driver)
                && !project.is_empty()
                && crs_epsg_code(&project) != Some(4326)
            {
                vector.target_crs = String::from("EPSG:4326");
                extra.push(warning_text(
                    "GeoJSON is longitude and latitude on WGS 84 (RFC 7946): the coordinates were \
                     converted from the project's; crs=native keeps the project's",
                ));
            }
        }
    }

    // The drawing as it is now, copied for the worker; the copy's entity
    // observer is the Document's, and a copy must never report to it.
    let mut model = context.document.model().clone();
    model.entities.set_observer(None);
    let mut snapshot = Snapshot {
        model,
        annotation_scale: context.document.annotation_scale(),
        leaders: leader_context(context.document),
        only: None,
        surfaces: Vec::new(),
        projection_wkt: project,
        vector: VectorExportOptions::default(),
        extra: Vec::new(),
    };

    // What the scope took; no scope is the whole drawing, as EXPORT was.
    let title = format!("EXPORT {}", file_name_text(&file));
    let mut whole_drawing = true;
    let mut taking = snapshot.model.entities.size();
    if words.scope_given {
        let found = match_scope(
            context.document,
            &words.scope,
            context.interpreter.scope_context(),
        )?;
        let record = format!("scope {}", cad_scope_record(&found));
        taking = found.matched.len();
        if found.matched.is_empty() {
            // Taking nothing is an answer, not a failure: said, nothing
            // written.
            return Ok(Prepared {
                title,
                reply: Some(format!(
                    "{record}\nexport file={} kind={} ran=no reason={}",
                    value(&path_text(&file)),
                    kind_word(kind),
                    value("the scope took nothing")
                )),
                work: None,
            });
        }
        extra.insert(0, record);
        whole_drawing =
            found.resolved.words.source == ScopeSource::Drawing && found.resolved.filter.empty();
        if kind == Kind::Vector {
            vector.entities = found.matched;
        } else if !whole_drawing {
            // The DXF and archive writers take a model: the copy keeps what
            // the scope took and nothing else. The archive's now; a DXF's
            // after its annotation is drawn from the whole (Snapshot::only).
            let kept: BTreeSet<EntityId> = found.matched.iter().copied().collect();
            if kind == Kind::Dxf {
                snapshot.only = Some(kept);
            } else {
                for id in snapshot.model.entities.ids() {
                    if !kept.contains(&id) {
                        // Each id is the copy's own, just listed, so the
                        // removal cannot miss; the C++ discards its status.
                        let _ = snapshot.model.entities.remove(id);
                    }
                }
            }
        }
    }
    // An archive carries the drawing's surfaces with the whole drawing; a
    // part of it is only that part, as the window's export always did.
    if kind == Kind::Archive && whole_drawing {
        snapshot.surfaces = context.surfaces.all().clone();
    }
    snapshot.vector = vector;
    snapshot.extra = extra;

    if preview {
        // Nothing written: what would be, and what the scope took.
        let mut reply = format!(
            "export file={} kind={}{} preview=yes entities={taking}",
            value(&path_text(&file)),
            kind_word(kind),
            if driver.is_empty() {
                String::new()
            } else {
                format!(" driver={}", value(&driver))
            }
        );
        for record in &snapshot.extra {
            reply.push('\n');
            reply += record;
        }
        return Ok(Prepared {
            title,
            reply: Some(reply),
            work: None,
        });
    }
    let work = make_work(move |stop, _progress| {
        let mut snapshot = snapshot;
        if stop_requested(stop) {
            return Err(cancelled());
        }
        let staged = StagedFiles::beside(&file)?;
        // append adds to a copy of the file, which the apply puts in place of
        // it: a cancelled append leaves the file as it was.
        if kind == Kind::Vector
            && snapshot.vector.append
            && file.exists()
            && let Err(error) = std::fs::copy(&file, staged.write_to())
        {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                format!(
                    "could not copy the file to add to it: {}",
                    system_message(&error)
                ),
                path_text(&file),
            ));
        }
        let records = match write(kind, &mut snapshot, staged.write_to(), &file) {
            Ok(records) => records,
            Err(error) => return Err(staged.as_target(error)),
        };
        if stop_requested(stop) {
            return Err(cancelled());
        }
        Ok(make_apply(move |_context| {
            staged.place()?;
            Ok(records)
        }))
    });
    Ok(Prepared {
        title,
        reply: None,
        work: Some(work),
    })
}
