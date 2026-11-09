//! Volumes on the command line (docs/volumes.md;
//! include/katana/cad/terrain/volume_verbs.hpp,
//! src/katana_cad/terrain/volume_verbs.cpp and volume_polygons.cpp): the
//! verbs a person, a script, katana_cli and an agent through katana_mcp type
//! to measure earthworks between the drawing's surfaces, and the window's
//! Terrain > Volumes builds.
//!
//! ```text
//!   VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>]
//!          [csv=<file> [OVERWRITE]]
//!   VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=] [shrink=] [csv=]
//!   VOLUME STOCKPILE <surface> INSIDE <polygon> [base=boundary|lowest] [level=<z>] [EACH]
//!          [bulk=] [shrink=] [csv=]
//!   VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] [from=] [to=] [left=]
//!          [right=] [method=endarea|prismoidal|both] [curvature=on|off] [bulk=] [shrink=]
//!          [csv=]
//!   VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=] [shrink=] [csv=]
//!   SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]
//!   SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]
//!
//!   <polygon> := #id [#id ...] | x,y[,z] x,y[,z] x,y[,z] ... | FROM <scope> [WHERE k=v ...]
//!                (read as drawing_shapes.rs reads it; a ring tagged as a hole joins the
//!                polygon it lies in, its area is taken out of it, and the polygons
//!                record says holes=<how many joined>)
//! ```
//!
//! Words are case-insensitive; a surface or alignment name with a blank is
//! quoted. The key=value options are read wherever they stand - none of them
//! is a WHERE key - and each is refused by name where the method takes none.
//!
//! THE READING OF CUT AND FILL is compare_surfaces' throughout: the first
//! surface named is the existing one (the base), the second the design (the
//! top); fill is where the top is above the base, cut where it is below, and
//! net = fill - cut in every record (positive: material to bring in). A level
//! is the design for DATUM and the base for STOCKPILE, so what stands above a
//! level is cut, and a pile above its base is cut - what removes it. Factors
//! are applied in the reply, never to the geometry: bulk= multiplies the cut,
//! shrink= the fill, each as given, and the record adds cut.adjusted=,
//! fill.adjusted= and net.adjusted=.
//!
//! The VOLUME verbs change nothing (csv= writes a file, refusing one already
//! there unless OVERWRITE; with a scope that takes no polygon it writes
//! nothing and says so). SURFACE DIFFERENCE and ENVELOPE add a surface to the
//! drawing - ONE undo step - and with PREVIEW say what they would add; the
//! difference is drawn in the Cut Fill style, the envelope (real levels) in
//! the default.
//!
//! Each verb reads its words, finds the drawing's surfaces by name, hands the
//! geometry to the terrain layer's exact methods (katana_terrain's volume,
//! overlay and section_volume) and writes what they measured as key=value
//! records.

use std::sync::Arc;

use katana_core::error::make_error;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, lowered, parse_finite_double, read_reply_record,
};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::Box2;
use katana_geometry::alignment::solve_alignment;
use katana_math::CompensatedSum;
use katana_terrain::{
    AlignmentVolumeOptions, CurvatureShift, DatumVolume, Envelope, NamedSurface, OverlayRegion,
    OverlaySurface, OverlaySurfaceOptions, StockpileBase, StockpileOptions, SurfaceComparison,
    TinSurface, compare_surfaces, compare_surfaces_inside, difference_surface, envelope_surface,
    grid_volume, stockpile_volume, volume_to_datum, volume_to_datum_inside,
    volumes_along_alignment,
};

use crate::csv_table::{CsvTable, write_csv_table};
use crate::document::Document;
use crate::scope_verbs::{ScopeViewProvider, record_value, scope_record};
use crate::survey_tools::c_fixed;
use crate::terrain::drawing_shapes::{
    DrawingShape, DrawingShapes, NotAShape, POLYGON_KINDS, ShapeKind, ShapeOptions,
    read_shape_words, resolve_shapes,
};
use crate::terrain::surface_verbs::surface_record;
use crate::utilities::utility_data::trimmed_text;

// ---- words and numbers ----------------------------------------------------------------------

fn is(word: &str, expected: &str) -> bool {
    equals_ignoring_case(word.as_bytes(), expected.as_bytes())
}

fn lower(text: &str) -> String {
    // core's lowered changes ASCII letters alone, so UTF-8 stays UTF-8.
    String::from_utf8(lowered(text)).unwrap_or_default()
}

// A volume or an area as a reply gives it: to the millimetre (cubic or
// square), as the DEM tools' volumes are, and never "-0.000" - a quantity of
// nothing has no sign.
fn fixed(mut value: f64) -> String {
    if value.abs() < 0.0005 {
        value = 0.0;
    }
    c_fixed(value, 3)
}

// A number the person typed, said back as it reads back.
fn exact(value: f64) -> String {
    format_exact_real(value)
}

fn usage(form: &str) -> Error {
    Error::new(ErrorCode::ParseFailure, format!("usage: {form}"))
}

const SURFACES_USAGE: &str = "VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] \
                              [shrink=<f>] [csv=<file> [OVERWRITE]]";
const DATUM_USAGE: &str = "VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=<f>] \
                           [shrink=<f>] [csv=<file> [OVERWRITE]]";
const STOCKPILE_USAGE: &str = "VOLUME STOCKPILE <surface> INSIDE <polygon> \
                               [base=boundary|lowest] [level=<z>] [EACH] [bulk=<f>] \
                               [shrink=<f>] [csv=<file> [OVERWRITE]]";
const SECTIONS_USAGE: &str = "VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] \
                              [from=<s>] [to=<s>] [left=<m>] [right=<m>] \
                              [method=endarea|prismoidal|both] [curvature=on|off] [bulk=<f>] \
                              [shrink=<f>] [csv=<file> [OVERWRITE]]";
const GRID_USAGE: &str = "VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=<f>] \
                          [shrink=<f>] [csv=<file> [OVERWRITE]]";
const DIFFERENCE_USAGE: &str = "SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]";
const ENVELOPE_USAGE: &str = "SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]";

// ---- the options -----------------------------------------------------------------------------

// Every key any VOLUME form takes. None is a WHERE key (scope_verbs.rs), so
// one may stand after a scope's filter and still be the verb's.
const OPTION_KEYS: [&str; 13] = [
    "bulk",
    "shrink",
    "csv",
    "base",
    "level",
    "interval",
    "from",
    "to",
    "left",
    "right",
    "method",
    "cell",
    "curvature",
];

// The verb's own key=value words, in key order.
#[derive(Clone, Debug, Default)]
struct Options {
    values: std::collections::BTreeMap<String, String>,
}

impl Options {
    fn find(&self, key: &str) -> Option<&String> {
        self.values.get(key)
    }
}

// The verb's own key=value words, taken out of `words` wherever they stand;
// what is left is the verb's positional words. A key the form does not take
// is refused by name rather than ignored.
fn take_options(words: &mut Vec<String>, form: &str, allowed: &[&str]) -> Result<Options> {
    let mut options = Options::default();
    let mut rest = Vec::new();
    for word in std::mem::take(words) {
        if let Some(equals) = word.find('=').filter(|&equals| equals > 0) {
            let key = lower(&word[..equals]);
            if OPTION_KEYS.contains(&key.as_str()) {
                if !allowed.contains(&key.as_str()) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        format!("{form} takes no {key}="),
                        word,
                    ));
                }
                if options.values.contains_key(&key) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        format!("{key}= is given twice"),
                        word,
                    ));
                }
                let value = String::from(&word[equals + 1..]);
                options.values.insert(key, value);
                continue;
            }
        }
        rest.push(word);
    }
    *words = rest;
    Ok(options)
}

// A flag word (EACH, OVERWRITE, PREVIEW) wherever it stands, taken out.
fn take_flag(words: &mut Vec<String>, flag: &str) -> bool {
    match words.iter().position(|word| is(word, flag)) {
        Some(found) => {
            words.remove(found);
            true
        }
        None => false,
    }
}

fn number_option(options: &Options, key: &str) -> Result<Option<f64>> {
    let Some(text) = options.find(key) else {
        return Ok(None);
    };
    match parse_finite_double(trimmed_text(text)) {
        Some(value) => Ok(Some(value)),
        None => Err(make_error(
            ErrorCode::ParseFailure,
            format!("{key}= is a number"),
            format!("{key}={text}"),
        )),
    }
}

fn positive_option(options: &Options, key: &str) -> Result<Option<f64>> {
    let value = number_option(options, key)?;
    if let Some(number) = value
        && !(number > 0.0)
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key}= must be more than 0"),
            format!(
                "{key}={}",
                options.find(key).map(String::as_str).unwrap_or_default()
            ),
        ));
    }
    Ok(value)
}

// ---- factors ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Factors {
    bulk: f64,
    shrink: f64,
    given: bool,
}

fn factors_of(options: &Options) -> Result<Factors> {
    let bulk = positive_option(options, "bulk")?;
    let shrink = positive_option(options, "shrink")?;
    Ok(Factors {
        given: bulk.is_some() || shrink.is_some(),
        bulk: bulk.unwrap_or(1.0),
        shrink: shrink.unwrap_or(1.0),
    })
}

// The factored quantities, applied here and never to the geometry: the cut
// times bulk=, the fill times shrink=, each as given.
fn factor_fields(factors: &Factors, cut: f64, fill: f64) -> String {
    if !factors.given {
        return String::new();
    }
    let cut_adjusted = cut * factors.bulk;
    let fill_adjusted = fill * factors.shrink;
    format!(
        " bulk={} shrink={} cut.adjusted={} fill.adjusted={} net.adjusted={}",
        exact(factors.bulk),
        exact(factors.shrink),
        fixed(cut_adjusted),
        fixed(fill_adjusted),
        fixed(fill_adjusted - cut_adjusted)
    )
}

// ---- surfaces --------------------------------------------------------------------------------

struct Named {
    name: String, // as the drawing has it
    surface: Arc<TinSurface>,
}

fn surface_named(document: &Document, name: &str) -> Result<Named> {
    match document.surfaces().find(name) {
        Some(NamedSurface {
            name: found,
            surface: Some(surface),
            ..
        }) => Ok(Named {
            name: found.clone(),
            surface: Arc::clone(surface),
        }),
        _ => Err(make_error(
            ErrorCode::NotFound,
            format!("the drawing has no surface named \"{name}\"; SURFACE LIST lists them"),
            name,
        )),
    }
}

fn box_text(extent: &Box2) -> String {
    format!(
        "{},{},{},{}",
        fixed(extent.min.x),
        fixed(extent.min.y),
        fixed(extent.max.x),
        fixed(extent.max.y)
    )
}

// "ground covers 0.000,0.000,100.000,100.000": what a refusal that two
// surfaces share no ground names of each, so a coordinate-system mistake
// shows.
fn coverage(surface: &Named) -> String {
    format!(
        "{} covers {}",
        surface.name,
        box_text(surface.surface.bounds())
    )
}

// A refusal that two surfaces share no ground, with what each covers; any
// other error as it was.
fn with_coverage(error: Error, first: &Named, second: &Named) -> Error {
    if error.code != ErrorCode::InvalidGeometry {
        return error;
    }
    make_error(
        error.code,
        format!(
            "{} ({}; {})",
            error.message,
            coverage(first),
            coverage(second)
        ),
        error.context,
    )
}

// ---- measuring inside polygons -------------------------------------------------------------

// What one method measured over one area.
#[derive(Clone, Debug, Default)]
struct Measure {
    cut: f64,
    fill: f64,
    area: f64, // the plan area measured
    cut_area: f64,
    fill_area: f64,
    cells: usize,
    fields: String, // the method's own, for this area's record
}

// How a method's records read.
#[derive(Clone, Debug)]
struct Reading {
    head: String,        // "volume method=exact base=ground top=design"
    level_reading: bool, // above= and below= as well (a level or a base)
    areas: bool,         // cut.area= and fill.area=
    cells: bool,         // cells=
    // The method counts each cell once across the polygons together (the
    // grid): polygons that overlap are not refused, only said, and a cell on
    // an edge they share is the first polygon's.
    counts_once: bool,
    nothing: String, // why a measure of no area is refused
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            head: String::new(),
            level_reading: false,
            areas: true,
            cells: false,
            counts_once: false,
            nothing: String::new(),
        }
    }
}

// One area: none (the whole of the surfaces) or a polygon and its region,
// with the regions of the polygons measured before it - which only a method
// that counts cells (the grid) has any use for.
type Measurer<'m> =
    dyn Fn(Option<&DrawingShape>, Option<&OverlayRegion>, &[OverlayRegion]) -> Result<Measure> + 'm;

fn measure_fields(reading: &Reading, measure: &Measure, factors: &Factors) -> String {
    let mut text = format!(
        " cut={} fill={} net={} area={}",
        fixed(measure.cut),
        fixed(measure.fill),
        fixed(measure.fill - measure.cut),
        fixed(measure.area)
    );
    if reading.level_reading {
        text += &format!(
            " above={} below={}",
            fixed(measure.cut),
            fixed(measure.fill)
        );
    }
    if reading.areas {
        text += &format!(
            " cut.area={} fill.area={}",
            fixed(measure.cut_area),
            fixed(measure.fill_area)
        );
    }
    if reading.cells {
        text += &format!(" cells={}", measure.cells);
    }
    text + &measure.fields + &factor_fields(factors, measure.cut, measure.fill)
}

// What a table says about how it was made, as columns that repeat on every
// row: the reply's own head (method=, the surfaces or the level, the cell) and
// the factors. A file handed on has no reply beside it, and an exact table
// and a grid table of the same polygons would otherwise differ only in their
// numbers.
#[derive(Clone, Debug, Default)]
struct CsvInputs {
    keys: Vec<String>,
    values: Vec<String>,
}

fn csv_inputs(reading: &Reading, factors: &Factors) -> CsvInputs {
    let mut inputs = CsvInputs::default();
    if let Some(head) = read_reply_record(&reading.head) {
        for (key, value) in head.fields {
            inputs.keys.push(key);
            inputs.values.push(value);
        }
    }
    if factors.given {
        inputs
            .keys
            .extend([String::from("bulk"), String::from("shrink")]);
        inputs
            .values
            .extend([exact(factors.bulk), exact(factors.shrink)]);
    }
    inputs
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

// The columns of a table of quantities, in the order of csv_row. The first
// eight, and the adjusted three, are where they always were; what the reply
// also says follows them, so a reader that counts columns still finds its
// own.
fn csv_header(reading: &Reading, factors: &Factors, inputs: &CsvInputs) -> Vec<String> {
    let mut header = strings(&[
        "record",
        "polygon",
        "entity",
        "polygon_area",
        "area",
        "cut",
        "fill",
        "net",
    ]);
    if factors.given {
        header.extend(strings(&["cut_adjusted", "fill_adjusted", "net_adjusted"]));
    }
    if reading.areas {
        header.extend(strings(&["cut_area", "fill_area"]));
    }
    if reading.level_reading {
        header.extend(strings(&["above", "below"]));
    }
    if reading.cells {
        header.push(String::from("cells"));
    }
    header.extend(inputs.keys.iter().cloned());
    header
}

#[allow(
    clippy::too_many_arguments,
    reason = "The C++ csvRow's parameters, one column group each."
)]
fn csv_row(
    kind: &str,
    index: String,
    entity: String,
    polygon_area: String,
    measure: &Measure,
    factors: &Factors,
    reading: &Reading,
    inputs: &CsvInputs,
) -> Vec<String> {
    let mut row = vec![
        String::from(kind),
        index,
        entity,
        polygon_area,
        fixed(measure.area),
        fixed(measure.cut),
        fixed(measure.fill),
        fixed(measure.fill - measure.cut),
    ];
    if factors.given {
        row.push(fixed(measure.cut * factors.bulk));
        row.push(fixed(measure.fill * factors.shrink));
        row.push(fixed(
            measure.fill * factors.shrink - measure.cut * factors.bulk,
        ));
    }
    if reading.areas {
        row.extend([fixed(measure.cut_area), fixed(measure.fill_area)]);
    }
    if reading.level_reading {
        row.extend([fixed(measure.cut), fixed(measure.fill)]);
    }
    if reading.cells {
        row.push(format!("{}", measure.cells));
    }
    row.extend(inputs.values.iter().cloned());
    row
}

fn polygon_name(polygon: &DrawingShape, index: usize) -> String {
    match polygon.entity {
        Some(entity) => format!("#{entity}"),
        None => format!("polygon {}", index + 1),
    }
}

// The plan area two regions share: the first made a 1 m slab over itself,
// cut by the second - the overlay already knows how.
fn shared_area(a: &OverlayRegion, b: &OverlayRegion) -> Result<f64> {
    if !a.bounds().intersects(*b.bounds()) {
        return Ok(0.0);
    }
    let floor = a.at_level(0.0);
    let roof = a.at_level(1.0);
    let (floor, roof) = match (floor, roof) {
        (Ok(floor), Ok(roof)) => (floor, roof),
        (Ok(_), Err(error)) | (Err(error), _) => return Err(error),
    };
    let slab = compare_surfaces_inside(&floor, &roof, b, None)?;
    Ok(slab.fill)
}

fn joined_lines(lines: &[String]) -> String {
    lines.join("\n")
}

// Measures the whole, or each polygon, and writes the records: the scope's,
// one per polygon with EACH, then the total. Polygons that overlap would be
// counted twice in a total, so without EACH they are refused; with EACH each
// is its own record and the total says it counts the overlap twice.
fn measure_areas(
    reading: &Reading,
    polygons: Option<&DrawingShapes>,
    each: bool,
    factors: &Factors,
    measure: &Measurer<'_>,
    options: &Options,
    overwrite: bool,
) -> Result<String> {
    let mut lines: Vec<String> = Vec::new();
    let inputs = csv_inputs(reading, factors);
    let mut table = CsvTable {
        header: csv_header(reading, factors, &inputs),
        rows: Vec::new(),
    };

    match polygons {
        None => {
            let whole = measure(None, None, &[])?;
            if !(whole.area > 0.0) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    reading.nothing.clone(),
                ));
            }
            lines.push(reading.head.clone() + &measure_fields(reading, &whole, factors));
            table.rows.push(csv_row(
                "total",
                String::new(),
                String::new(),
                String::new(),
                &whole,
                factors,
                reading,
                &inputs,
            ));
        }
        Some(polygons) => {
            if let Some(scope) = &polygons.scope {
                // What the boundary's scope took, as every verb that takes a
                // scope says it: a record of its own, its kind first.
                lines.push(format!("boundary {}", scope_record(scope)));
            }
            // skipped.open= is what bounds no area: an open outline, and a
            // closed one of fewer than three corners. holes= is the rings
            // tagged as holes that joined the polygon they lie in, so they
            // are no polygon of their own and not in used=.
            lines.push(format!(
                "polygons used={} holes={} skipped.open={} skipped.other={}",
                polygons.shapes.len(),
                polygons.holes_joined,
                polygons.skipped.open + polygons.skipped.too_few,
                polygons.skipped.no_outline
            ));
            if polygons.holes_alone != 0 {
                // As the feature-table conversion says it of the same rings.
                lines.push(format!(
                    "warning holes.alone={} message={}",
                    polygons.holes_alone,
                    record_value(&format!(
                        "{} rings tagged as holes lie in no polygon of the scope and are \
                         measured as polygons of their own",
                        polygons.holes_alone
                    ))
                ));
            }
            if polygons.shapes.is_empty() {
                // A scope that takes no closed outline is said, not refused.
                lines.push(format!(
                    "{} ran=no reason={}",
                    reading.head,
                    record_value("the scope took no closed polygon")
                ));
                if let Some(path) = options.find("csv") {
                    // Nothing ran, so nothing is written - the file a run
                    // before left there is as it was, and the reply says so
                    // rather than leave the person to find it still holds the
                    // old numbers.
                    lines.push(format!(
                        "csv file={} written=no reason={}",
                        record_value(path),
                        record_value("the scope took no closed polygon")
                    ));
                }
                return Ok(joined_lines(&lines));
            }
            let mut regions: Vec<OverlayRegion> = Vec::with_capacity(polygons.shapes.len());
            for (i, polygon) in polygons.shapes.iter().enumerate() {
                let region = OverlayRegion::from_polygon(&polygon.outline, &polygon.holes)
                    .map_err(|error| {
                        make_error(
                            error.code,
                            format!("{}: {}", polygon_name(polygon, i), error.message),
                            error.context,
                        )
                    })?;
                regions.push(region);
            }
            let mut overlaps: Vec<(usize, usize)> = Vec::new();
            for i in 0..regions.len() {
                for j in i + 1..regions.len() {
                    let shared = shared_area(&regions[i], &regions[j])?;
                    // Polygons that share an edge share a segment, of no area;
                    // what is counted twice is area, and a square millimetre
                    // of it is rounding, not ground.
                    if shared > 1e-6 {
                        if !each && !reading.counts_once {
                            return Err(Error::new(
                                ErrorCode::InvalidGeometry,
                                format!(
                                    "{} and {} overlap by {} m2, which one total would count \
                                     twice; give EACH for a record per polygon",
                                    polygon_name(&polygons.shapes[i], i),
                                    polygon_name(&polygons.shapes[j], j),
                                    fixed(shared)
                                ),
                            ));
                        }
                        overlaps.push((i, j));
                    }
                }
            }
            let mut cut = CompensatedSum::new();
            let mut fill = CompensatedSum::new();
            let mut area = CompensatedSum::new();
            let mut cut_area = CompensatedSum::new();
            let mut fill_area = CompensatedSum::new();
            let mut polygon_area = CompensatedSum::new();
            let mut cells = 0;
            // What the method says of its own about each polygon (a
            // stockpile's base level, whether the toe's own heights were
            // used): on the total when every polygon says the same, which one
            // polygon always does. Where they differ there is no one value to
            // give, and EACH gives each.
            let mut shared_fields: Option<String> = None;
            let mut fields_agree = true;
            for (i, (polygon, region)) in polygons.shapes.iter().zip(&regions).enumerate() {
                let measured =
                    measure(Some(polygon), Some(region), &regions[..i]).map_err(|error| {
                        make_error(
                            error.code,
                            format!("{}: {}", polygon_name(polygon, i), error.message),
                            error.context,
                        )
                    })?;
                cut.add(measured.cut);
                fill.add(measured.fill);
                area.add(measured.area);
                cut_area.add(measured.cut_area);
                fill_area.add(measured.fill_area);
                polygon_area.add(region.area());
                cells += measured.cells;
                if shared_fields.is_none() {
                    shared_fields = Some(measured.fields.clone());
                } else if shared_fields.as_deref() != Some(measured.fields.as_str()) {
                    fields_agree = false;
                }
                let entity = polygon
                    .entity
                    .map_or_else(String::new, |entity| format!("{entity}"));
                if each {
                    let mut record = format!("polygon index={}", i + 1);
                    if polygon.entity.is_some() {
                        record += &format!(" entity={entity}");
                    }
                    record += &format!(" polygon.area={}", fixed(region.area()));
                    if !polygon.holes.is_empty() {
                        record += &format!(" holes={}", polygon.holes.len());
                    }
                    record += &measure_fields(reading, &measured, factors);
                    if polygon.chorded {
                        record += " chorded=yes";
                    }
                    lines.push(record);
                    table.rows.push(csv_row(
                        "polygon",
                        format!("{}", i + 1),
                        entity,
                        fixed(region.area()),
                        &measured,
                        factors,
                        reading,
                        &inputs,
                    ));
                }
            }
            for &(i, j) in &overlaps {
                lines.push(format!(
                    "warning overlap={},{} message={}",
                    i + 1,
                    j + 1,
                    record_value(&format!(
                        "{} and {}{}",
                        polygon_name(&polygons.shapes[i], i),
                        polygon_name(&polygons.shapes[j], j),
                        if reading.counts_once {
                            " overlap: each cell is counted once, in the first, so area= is the \
                             union's and polygon.area= counts the overlap twice"
                        } else {
                            " overlap: the total counts the ground they share twice"
                        }
                    ))
                ));
            }
            let total = Measure {
                cut: cut.value(),
                fill: fill.value(),
                area: area.value(),
                cut_area: cut_area.value(),
                fill_area: fill_area.value(),
                cells,
                fields: if fields_agree {
                    shared_fields.unwrap_or_default()
                } else {
                    String::new()
                },
            };
            if !(total.area > 0.0) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    format!(
                        "nothing inside the polygons was measured: {}",
                        reading.nothing
                    ),
                ));
            }
            lines.push(format!(
                "{} polygons={} polygon.area={}{}",
                reading.head,
                regions.len(),
                fixed(polygon_area.value()),
                measure_fields(reading, &total, factors)
            ));
            table.rows.push(csv_row(
                "total",
                String::new(),
                String::new(),
                fixed(polygon_area.value()),
                &total,
                factors,
                reading,
                &inputs,
            ));
        }
    }

    if let Some(path) = options.find("csv") {
        lines.push(write_csv_table(&table, path, overwrite)?);
    }
    Ok(joined_lines(&lines))
}

// INSIDE <polygon> [EACH] after the positional words at `at`, and nothing
// after them.
#[derive(Clone, Debug, Default)]
struct Inside {
    polygons: Option<DrawingShapes>,
    each: bool,
}

fn read_inside(
    document: &Document,
    words: &[String],
    mut at: usize,
    views: ScopeViewProvider<'_>,
    form: &str,
    required: bool,
) -> Result<Inside> {
    let mut inside = Inside::default();
    if at < words.len() && is(&words[at], "INSIDE") {
        at += 1;
        inside.polygons = Some(read_volume_polygons(document, words, &mut at, views)?);
    } else if required {
        return Err(usage(form));
    }
    if at < words.len() && is(&words[at], "EACH") {
        if inside.polygons.is_none() {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "EACH follows INSIDE <polygon>",
                words[at].as_str(),
            ));
        }
        inside.each = true;
        at += 1;
    }
    if at < words.len() {
        return Err(make_error(
            ErrorCode::ParseFailure,
            format!("not a word of this form; usage: {form}"),
            words[at].as_str(),
        ));
    }
    Ok(inside)
}

fn from_comparison(comparison: &SurfaceComparison) -> Measure {
    Measure {
        cut: comparison.cut,
        fill: comparison.fill,
        area: comparison.plan_area,
        cut_area: comparison.cut_area,
        fill_area: comparison.fill_area,
        ..Measure::default()
    }
}

fn from_datum(volume: &DatumVolume) -> Measure {
    // A level reading: what stands above is cut, what lies below is fill.
    Measure {
        cut: volume.above,
        fill: volume.below,
        area: volume.plan_area,
        cut_area: volume.plan_area_above,
        fill_area: volume.plan_area_below,
        ..Measure::default()
    }
}

// ---- VOLUME SURFACES ---------------------------------------------------------------------------

fn volume_surfaces(
    document: &Document,
    mut words: Vec<String>,
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let options = take_options(&mut words, "VOLUME SURFACES", &["bulk", "shrink", "csv"])?;
    let overwrite = take_flag(&mut words, "OVERWRITE");
    if words.len() < 2 {
        return Err(usage(SURFACES_USAGE));
    }
    let base = surface_named(document, &words[0])?;
    let top = surface_named(document, &words[1])?;
    let inside = read_inside(document, &words, 2, views, SURFACES_USAGE, false)?;
    let factors = factors_of(&options)?;
    let reading = Reading {
        head: format!(
            "volume method=exact base={} top={}",
            record_value(&base.name),
            record_value(&top.name)
        ),
        nothing: format!(
            "the surfaces have no ground in common there ({}; {})",
            coverage(&base),
            coverage(&top)
        ),
        ..Reading::default()
    };
    let existing: &TinSurface = &base.surface;
    let design: &TinSurface = &top.surface;
    let measure = |_: Option<&DrawingShape>,
                   region: Option<&OverlayRegion>,
                   _: &[OverlayRegion]|
     -> Result<Measure> {
        let comparison = match region {
            None => compare_surfaces(existing, design, None)?,
            Some(region) => compare_surfaces_inside(existing, design, region, None)?,
        };
        Ok(from_comparison(&comparison))
    };
    if inside.polygons.is_none() && !existing.bounds().intersects(*design.bounds()) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            format!(
                "{} covers {} and {} covers {}: they have no ground in common",
                base.name,
                box_text(existing.bounds()),
                top.name,
                box_text(design.bounds())
            ),
        ));
    }
    measure_areas(
        &reading,
        inside.polygons.as_ref(),
        inside.each,
        &factors,
        &measure,
        &options,
        overwrite,
    )
}

// ---- VOLUME DATUM -------------------------------------------------------------------------------

fn volume_datum(
    document: &Document,
    mut words: Vec<String>,
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let options = take_options(&mut words, "VOLUME DATUM", &["bulk", "shrink", "csv"])?;
    let overwrite = take_flag(&mut words, "OVERWRITE");
    if words.len() < 2 {
        return Err(usage(DATUM_USAGE));
    }
    let surface = surface_named(document, &words[0])?;
    let Some(level) = parse_finite_double(trimmed_text(&words[1])) else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the level is a number",
            words[1].as_str(),
        ));
    };
    let inside = read_inside(document, &words, 2, views, DATUM_USAGE, false)?;
    let factors = factors_of(&options)?;
    let reading = Reading {
        head: format!(
            "volume method=exact surface={} level={}",
            record_value(&surface.name),
            exact(level)
        ),
        level_reading: true,
        nothing: format!("the polygons are off the surface ({})", coverage(&surface)),
        ..Reading::default()
    };
    let ground: &TinSurface = &surface.surface;
    let measure = |_: Option<&DrawingShape>,
                   region: Option<&OverlayRegion>,
                   _: &[OverlayRegion]|
     -> Result<Measure> {
        let volume = match region {
            None => volume_to_datum(ground, level)?,
            Some(region) => volume_to_datum_inside(ground, level, region, None)?,
        };
        Ok(from_datum(&volume))
    };
    measure_areas(
        &reading,
        inside.polygons.as_ref(),
        inside.each,
        &factors,
        &measure,
        &options,
        overwrite,
    )
}

// ---- VOLUME STOCKPILE ---------------------------------------------------------------------------

fn volume_stockpile(
    document: &Document,
    mut words: Vec<String>,
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let options = take_options(
        &mut words,
        "VOLUME STOCKPILE",
        &["base", "level", "csv", "bulk", "shrink"],
    )?;
    let overwrite = take_flag(&mut words, "OVERWRITE");
    if words.is_empty() {
        return Err(usage(STOCKPILE_USAGE));
    }
    let surface = surface_named(document, &words[0])?;
    let inside = read_inside(document, &words, 1, views, STOCKPILE_USAGE, true)?;
    let factors = factors_of(&options)?;
    let level = number_option(&options, "level")?;
    let mut stockpile = StockpileOptions::default();
    let mut base_word = String::from("boundary");
    if let Some(base) = options.find("base") {
        base_word = lower(base);
        if base_word != "boundary" && base_word != "lowest" {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "base= is boundary or lowest; a level is level=<z>",
                format!("base={base}"),
            ));
        }
        if level.is_some() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "give base= or level=, not both: level= is a base of its own",
            ));
        }
    }
    if let Some(level) = level {
        stockpile.base = StockpileBase::Level;
        stockpile.level = level;
        base_word = String::from("level");
    } else if base_word == "lowest" {
        stockpile.base = StockpileBase::Lowest;
    }
    let mut head = format!(
        "volume method=exact surface={} base={base_word}",
        record_value(&surface.name)
    );
    if let Some(level) = level {
        head += &format!(" level={}", exact(level));
    }
    let reading = Reading {
        head,
        level_reading: true,
        nothing: format!("the polygons are off the surface ({})", coverage(&surface)),
        ..Reading::default()
    };
    let ground: &TinSurface = &surface.surface;
    let measure = |polygon: Option<&DrawingShape>,
                   _: Option<&OverlayRegion>,
                   _: &[OverlayRegion]|
     -> Result<Measure> {
        // A stockpile is always measured inside its polygons (INSIDE is
        // required), so there is always one.
        let polygon = polygon.expect("a stockpile is measured inside a polygon");
        let these = StockpileOptions {
            boundary_heights: polygon.heights.clone(),
            holes: polygon.holes.clone(),
            ..stockpile.clone()
        };
        let volume = stockpile_volume(ground, &polygon.outline, &these, None)?;
        let mut measured = from_datum(&volume.volume);
        if volume.base == StockpileBase::Boundary {
            measured.fields = format!(
                " heights={}",
                if volume.boundary_heights {
                    "own"
                } else {
                    "surface"
                }
            );
        } else if volume.base == StockpileBase::Lowest
            && let Some(level) = volume.level
        {
            measured.fields = format!(" lowest={}", fixed(level));
        }
        Ok(measured)
    };
    measure_areas(
        &reading,
        inside.polygons.as_ref(),
        inside.each,
        &factors,
        &measure,
        &options,
        overwrite,
    )
}

// ---- VOLUME GRID --------------------------------------------------------------------------------

fn volume_grid(
    document: &Document,
    mut words: Vec<String>,
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let options = take_options(
        &mut words,
        "VOLUME GRID",
        &["cell", "bulk", "shrink", "csv"],
    )?;
    let overwrite = take_flag(&mut words, "OVERWRITE");
    if words.len() < 2 {
        return Err(usage(GRID_USAGE));
    }
    let base = surface_named(document, &words[0])?;
    let top = surface_named(document, &words[1])?;
    let Some(side) = positive_option(&options, "cell")? else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("VOLUME GRID needs the cell: cell=<m>; usage: {GRID_USAGE}"),
        ));
    };
    let inside = read_inside(document, &words, 2, views, GRID_USAGE, false)?;
    if inside.each {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "VOLUME GRID counts each cell once across all the polygons, so it has no EACH; \
             VOLUME SURFACES, DATUM and STOCKPILE take it",
        ));
    }
    let factors = factors_of(&options)?;
    let reading = Reading {
        head: format!(
            "volume method=grid base={} top={} cell={}",
            record_value(&base.name),
            record_value(&top.name),
            exact(side)
        ),
        areas: false,
        cells: true,
        counts_once: true,
        nothing: format!(
            "no cell's centre is on both surfaces ({}; {})",
            coverage(&base),
            coverage(&top)
        ),
        ..Reading::default()
    };
    let existing: &TinSurface = &base.surface;
    let design: &TinSurface = &top.surface;
    let measure = |_: Option<&DrawingShape>,
                   region: Option<&OverlayRegion>,
                   earlier: &[OverlayRegion]|
     -> Result<Measure> {
        // A cell on an edge two polygons share is the first polygon's, so the
        // polygons' cells, summed, are those of their union.
        let grid = grid_volume(existing, design, side, region, None, earlier)?;
        Ok(Measure {
            cut: grid.cut,
            fill: grid.fill,
            area: grid.plan_area,
            cells: grid.cells,
            ..Measure::default()
        })
    };
    measure_areas(
        &reading,
        inside.polygons.as_ref(),
        false,
        &factors,
        &measure,
        &options,
        overwrite,
    )
}

// ---- VOLUME SECTIONS ----------------------------------------------------------------------------

fn volume_sections(document: &Document, mut words: Vec<String>) -> Result<String> {
    let options = take_options(
        &mut words,
        "VOLUME SECTIONS",
        &[
            "interval",
            "from",
            "to",
            "left",
            "right",
            "method",
            "curvature",
            "bulk",
            "shrink",
            "csv",
        ],
    )?;
    let overwrite = take_flag(&mut words, "OVERWRITE");
    if words.len() != 4 || !is(&words[2], "ALIGNMENT") {
        return Err(usage(SECTIONS_USAGE));
    }
    let base = surface_named(document, &words[0])?;
    let top = surface_named(document, &words[1])?;
    let Some(alignment) = document.model().alignments.find(&words[3]) else {
        return Err(make_error(
            ErrorCode::NotFound,
            format!(
                "the drawing has no alignment named \"{}\"; ALIGN LIST lists them",
                words[3]
            ),
            words[3].as_str(),
        ));
    };
    let solved = solve_alignment(&alignment.horizontal)?;
    let factors = factors_of(&options)?;

    let interval = positive_option(&options, "interval")?.unwrap_or(10.0);
    let from = number_option(&options, "from")?;
    let to = number_option(&options, "to")?;
    let left = number_option(&options, "left")?;
    let right = number_option(&options, "right")?;
    let mut end_area = true;
    let mut prismoidal = false;
    if let Some(method) = options.find("method") {
        match lower(method).as_str() {
            "prismoidal" => {
                end_area = false;
                prismoidal = true;
            }
            "both" => prismoidal = true,
            "endarea" => {}
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "method= is endarea, prismoidal or both",
                    format!("method={method}"),
                ));
            }
        }
    }
    let mut curvature = AlignmentVolumeOptions::default().curvature;
    if let Some(word) = options.find("curvature") {
        let setting = lower(trimmed_text(word));
        if setting == "off" {
            curvature = false;
        } else if setting != "on" {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "curvature= is on or off",
                format!("curvature={word}"),
            ));
        }
    }
    let sections = AlignmentVolumeOptions {
        interval,
        from,
        to,
        left,
        right,
        prismoidal,
        curvature,
    };
    let method_word = if end_area {
        if prismoidal { "both" } else { "endarea" }
    } else {
        "prismoidal"
    };
    let volumes =
        match volumes_along_alignment(&base.surface, &top.surface, &solved, &sections, None) {
            Ok(volumes) => volumes,
            Err(error) if error.code == ErrorCode::InvalidGeometry => {
                // Nothing to measure: which alignment, and what each surface
                // covers, as the other forms name theirs.
                let named = with_coverage(error, &base, &top);
                return Err(make_error(
                    named.code,
                    format!("alignment {}: {}", alignment.name, named.message),
                    named.context,
                ));
            }
            Err(error) => return Err(error),
        };

    // The mass haul ordinate: cut taken as positive and fill as negative,
    // summed from the start - the mass diagram's convention - with the
    // factors applied.
    let mut lines: Vec<String> = Vec::new();
    let mut table = CsvTable {
        header: strings(&[
            "station",
            "key",
            "cut_area",
            "fill_area",
            "width",
            "cut",
            "fill",
            "cut_total",
            "fill_total",
            "mass",
        ]),
        rows: Vec::new(),
    };
    if end_area && prismoidal {
        table.header.extend(strings(&[
            "cut_prismoidal",
            "fill_prismoidal",
            "cut_total_prismoidal",
            "fill_total_prismoidal",
            "mass_prismoidal",
        ]));
    }
    // The columns above are the section table. The ones that follow say how
    // it was made, on every row: with method=prismoidal the plain cut, fill
    // and mass columns hold the prismoidal volumes, which only this column
    // tells from an end-area file, and the factors behind a factored mass
    // column are here too.
    let quantity_columns = table.header.len();
    table.header.extend(strings(&[
        "method",
        "curvature",
        "base",
        "top",
        "alignment",
        "interval",
        "left",
        "right",
    ]));
    if factors.given {
        table.header.extend(strings(&["bulk", "shrink"]));
    }
    let mut cut_total = CompensatedSum::new();
    let mut fill_total = CompensatedSum::new();
    let mut prism_cut_total = CompensatedSum::new();
    let mut prism_fill_total = CompensatedSum::new();
    let mut empty = 0;
    for (i, section) in volumes.sections.iter().enumerate() {
        if !(section.width > 0.0) {
            empty += 1;
        }
        let mut record = format!("section station={}", fixed(section.station));
        if section.key {
            record += " key=yes";
        }
        record += &format!(
            " cut.area={} fill.area={} width={}",
            fixed(section.cut_area),
            fixed(section.fill_area),
            fixed(section.width)
        );
        let mut row = vec![
            fixed(section.station),
            String::from(if section.key { "yes" } else { "no" }),
            fixed(section.cut_area),
            fixed(section.fill_area),
            fixed(section.width),
        ];
        if i > 0 {
            let panel = &volumes.panels[i - 1];
            let cut = if end_area {
                panel.end_area_cut
            } else {
                panel.prismoidal_cut
            };
            let fill = if end_area {
                panel.end_area_fill
            } else {
                panel.prismoidal_fill
            };
            cut_total.add(cut);
            fill_total.add(fill);
            let mass = cut_total.value() * factors.bulk - fill_total.value() * factors.shrink;
            record += &format!(
                " cut={} fill={} cut.total={} fill.total={} mass={}",
                fixed(cut),
                fixed(fill),
                fixed(cut_total.value()),
                fixed(fill_total.value()),
                fixed(mass)
            );
            row.extend([
                fixed(cut),
                fixed(fill),
                fixed(cut_total.value()),
                fixed(fill_total.value()),
                fixed(mass),
            ]);
            if prismoidal && let Some(middle) = &panel.middle {
                record += &format!(
                    " middle.cut.area={} middle.fill.area={}",
                    fixed(middle.cut_area),
                    fixed(middle.fill_area)
                );
            }
            if panel.curvature != 0.0 {
                // On a curve: its radius at the middle of the panel and which
                // way it turns, and what the correction did to this panel's
                // volumes (the method of cut= and fill=).
                let shift: &CurvatureShift = if end_area {
                    &panel.end_area_shift
                } else {
                    &panel.prismoidal_shift
                };
                record += &format!(
                    " radius={} turn={} curvature.cut={} curvature.fill={}",
                    fixed(1.0 / panel.curvature.abs()),
                    if panel.curvature > 0.0 {
                        "left"
                    } else {
                        "right"
                    },
                    fixed(shift.cut),
                    fixed(shift.fill)
                );
            }
            if end_area && prismoidal {
                prism_cut_total.add(panel.prismoidal_cut);
                prism_fill_total.add(panel.prismoidal_fill);
                let prism_mass = prism_cut_total.value() * factors.bulk
                    - prism_fill_total.value() * factors.shrink;
                record += &format!(
                    " cut.prismoidal={} fill.prismoidal={} mass.prismoidal={}",
                    fixed(panel.prismoidal_cut),
                    fixed(panel.prismoidal_fill),
                    fixed(prism_mass)
                );
                row.extend([
                    fixed(panel.prismoidal_cut),
                    fixed(panel.prismoidal_fill),
                    fixed(prism_cut_total.value()),
                    fixed(prism_fill_total.value()),
                    fixed(prism_mass),
                ]);
            }
        } else {
            row.resize(quantity_columns, String::new());
        }
        row.extend([
            String::from(method_word),
            String::from(if sections.curvature { "on" } else { "off" }),
            base.name.clone(),
            top.name.clone(),
            alignment.name.clone(),
            exact(sections.interval),
            fixed(volumes.left),
            fixed(volumes.right),
        ]);
        if factors.given {
            row.extend([exact(factors.bulk), exact(factors.shrink)]);
        }
        lines.push(record);
        table.rows.push(row);
    }

    let range = format!(
        " base={} top={} alignment={} from={} to={} interval={} left={} right={} curvature={} \
         sections={} sections.empty={empty}",
        record_value(&base.name),
        record_value(&top.name),
        record_value(&alignment.name),
        fixed(volumes.from_station),
        fixed(volumes.to_station),
        exact(sections.interval),
        fixed(volumes.left),
        fixed(volumes.right),
        if sections.curvature { "on" } else { "off" },
        volumes.sections.len()
    );
    // With the correction on, how much it moved the total (negative: took
    // away), so a figure that differs from the plain end-area one says by
    // how much.
    let totals = |method: &str, cut: f64, fill: f64, shift: &CurvatureShift| {
        let mut record = format!(
            "volume method={method}{range} cut={} fill={} net={}{}",
            fixed(cut),
            fixed(fill),
            fixed(fill - cut),
            factor_fields(&factors, cut, fill)
        );
        if sections.curvature {
            record += &format!(
                " curvature.cut={} curvature.fill={}",
                fixed(shift.cut),
                fixed(shift.fill)
            );
        }
        record
    };
    if end_area {
        lines.push(totals(
            "endarea",
            volumes.end_area_cut,
            volumes.end_area_fill,
            &volumes.end_area_shift,
        ));
    }
    if prismoidal {
        let mut record = totals(
            "prismoidal",
            volumes.prismoidal_cut,
            volumes.prismoidal_fill,
            &volumes.prismoidal_shift,
        );
        // The prismoidal correction: what the average end area overstates
        // (or understates) by.
        record += &format!(
            " correction.cut={} correction.fill={}",
            fixed(volumes.end_area_cut - volumes.prismoidal_cut),
            fixed(volumes.end_area_fill - volumes.prismoidal_fill)
        );
        lines.push(record);
    }
    if let Some(path) = options.find("csv") {
        lines.push(write_csv_table(&table, path, overwrite)?);
    }
    Ok(joined_lines(&lines))
}

// ---- SURFACE DIFFERENCE and ENVELOPE -------------------------------------------------------------

// NAME <new> and PREVIEW wherever they stand, taken out of `words`.
#[derive(Clone, Debug, Default)]
struct MakeWords {
    name: Option<String>,
    preview: bool,
}

fn take_make_words(words: &mut Vec<String>, form: &str) -> Result<MakeWords> {
    let mut made = MakeWords {
        preview: take_flag(words, "PREVIEW"),
        ..MakeWords::default()
    };
    let mut i = 0;
    while i < words.len() {
        if is(&words[i], "NAME") {
            if i + 1 >= words.len() || made.name.is_some() {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!("NAME takes the new surface's name, once; usage: {form}"),
                    words[i].as_str(),
                ));
            }
            made.name = Some(words[i + 1].clone());
            words.drain(i..i + 2);
        } else {
            i += 1;
        }
    }
    if made
        .name
        .as_deref()
        .is_some_and(|name| trimmed_text(name).is_empty())
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a surface's name cannot be empty",
        ));
    }
    Ok(made)
}

// What a new surface is made of and called: the surface, where it came from,
// the name suggested when NAME gave none, its first record, its undo step and
// its style.
struct MadeSurface {
    suggested: String,
    source: String,
    surface: OverlaySurface,
    first_record: String,
    step_name: &'static str,
    style: String,
}

// The new surface added to the drawing's - ONE undo step named after the
// verb - or, with PREVIEW, only described. The reply's surface line is
// surface_record, the one record every reply that names a surface writes (so
// it has visible=, and names the style the surface is drawn in, which is the
// drawing's default when `style` is empty or no longer there). `style` is the
// caller's: a difference is a cut/fill map and wants "Cut Fill", an envelope
// holds real levels and must not have it.
fn add_made_surface(
    document: &mut Document,
    made: &MakeWords,
    surface: MadeSurface,
) -> Result<String> {
    let mut store = document.surfaces().clone();
    let name = match &made.name {
        Some(name) => name.clone(),
        None => store.unique_name(surface.suggested.as_bytes()),
    };
    if made.name.is_some() && store.find(&name).is_some() {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            format!("the drawing already has a surface named \"{name}\""),
            name,
        ));
    }
    let named = NamedSurface {
        name,
        surface: Some(Arc::new(surface.surface.surface)),
        source: surface.source,
        style: surface.style,
        visible: true,
    };
    let reply = format!(
        "{}\n{}",
        surface.first_record,
        surface_record(&named, document.surface_styles())
    );
    if made.preview {
        return Ok(reply + " preview=yes");
    }
    store.add(named)?;
    document.set_surfaces(store, surface.step_name)?;
    Ok(reply)
}

fn surface_difference(document: &mut Document, mut words: Vec<String>) -> Result<String> {
    let made = take_make_words(&mut words, DIFFERENCE_USAGE)?;
    if words.len() != 2 {
        return Err(usage(DIFFERENCE_USAGE));
    }
    let base = surface_named(document, &words[0])?;
    let top = surface_named(document, &words[1])?;
    let difference = difference_surface(
        &base.surface,
        &top.surface,
        &OverlaySurfaceOptions::default(),
    )
    .map_err(|error| with_coverage(error, &base, &top))?;
    let record = format!(
        "difference base={} top={} pieces={} retriangulated={}",
        record_value(&base.name),
        record_value(&top.name),
        difference.pieces,
        if difference.retriangulated {
            "yes"
        } else {
            "no"
        }
    );
    // The cut/fill style when the drawing still has it; otherwise its
    // default.
    let style = document
        .surface_styles()
        .find("Cut Fill")
        .map(|cut_fill| cut_fill.name.clone())
        .unwrap_or_default();
    add_made_surface(
        document,
        &made,
        MadeSurface {
            suggested: format!("{}-{}", top.name, base.name),
            source: format!("difference {} minus {}", top.name, base.name),
            surface: difference,
            first_record: record,
            step_name: "SURFACE DIFFERENCE",
            style,
        },
    )
}

fn surface_envelope(document: &mut Document, mut words: Vec<String>) -> Result<String> {
    let made = take_make_words(&mut words, ENVELOPE_USAGE)?;
    if words.len() != 3 {
        return Err(usage(ENVELOPE_USAGE));
    }
    let which = if is(&words[2], "MIN") {
        Envelope::Lower
    } else if is(&words[2], "MAX") {
        Envelope::Upper
    } else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the envelope is MAX or MIN",
            words[2].as_str(),
        ));
    };
    let first = surface_named(document, &words[0])?;
    let second = surface_named(document, &words[1])?;
    let envelope = envelope_surface(
        &first.surface,
        &second.surface,
        which,
        &OverlaySurfaceOptions::default(),
    )
    .map_err(|error| with_coverage(error, &first, &second))?;
    let side = if which == Envelope::Upper {
        "max"
    } else {
        "min"
    };
    let record = format!(
        "envelope first={} second={} side={side} pieces={} retriangulated={}",
        record_value(&first.name),
        record_value(&second.name),
        envelope.pieces,
        if envelope.retriangulated { "yes" } else { "no" }
    );
    add_made_surface(
        document,
        &made,
        MadeSurface {
            suggested: format!("{}-{}-{side}", first.name, second.name),
            source: format!("envelope {side} of {} and {}", first.name, second.name),
            surface: envelope,
            first_record: record,
            step_name: "SURFACE ENVELOPE",
            // Real levels, not a difference about 0: the default style.
            style: String::new(),
        },
    )
}

// ---- the polygons (volume_polygons.cpp) -------------------------------------------------------
//
// The boundaries the volume verbs measure inside: the drawing's polygons as
// the one reader reads them (drawing_shapes.rs), refused in these verbs' own
// words.

fn id_text(id: EntityId) -> String {
    format!("#{id}")
}

// What an entity that is no polygon is called in a refusal.
fn kind_name(entity: Option<&Entity>, why: NotAShape) -> &'static str {
    if why == NotAShape::TooFew {
        return "an outline of fewer than three corners";
    }
    match entity.map(|entity| &entity.geometry) {
        Some(Geometry::Line(_)) => "a line",
        Some(Geometry::Arc(_)) => "an arc",
        Some(Geometry::Polyline(_) | Geometry::CurvePolyline(_)) => "an open polyline",
        Some(Geometry::Ellipse(_)) => "an elliptical arc", // a full ellipse is a polygon
        Some(Geometry::Spline(_)) => "an open spline", // one that returns to its start is a polygon
        _ => "no outline",
    }
}

/// Reads a <polygon> from `words` at `at` and leaves `at` after it: one or
/// more #ids, three or more typed corners x,y[,z], or FROM and a scope - the
/// drawing's polygons as the surface edits read them too (drawing_shapes.rs,
/// which says what each kind of entity gives), with these verbs' refusals. A
/// scope that takes no closed outline gives no polygon - said, not refused;
/// an #id that is no entity is NotFound, one that is no polygon
/// InvalidGeometry; fewer than three corners ParseFailure. A polygon's
/// heights are what STOCKPILE base=boundary uses, and `chorded` what EACH's
/// chorded=yes says.
pub fn read_volume_polygons(
    document: &Document,
    words: &[String],
    at: &mut usize,
    views: ScopeViewProvider<'_>,
) -> Result<DrawingShapes> {
    if *at >= words.len() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "a polygon is #id, three or more corners x,y, or FROM and a scope",
        ));
    }
    let read = read_shape_words(words, at)?;
    if let Some(bad) = &read.bad_id {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "an entity is #id, a whole number",
            bad.as_str(),
        ));
    }
    // A ring tagged as a hole joins the polygon it lies in: a lot with a hole
    // measures its area less the hole, not both as polygons of their own.
    let options = ShapeOptions {
        join_tagged_holes: true,
    };
    let polygons = resolve_shapes(document, &read, ShapeKind::Polygon, views, &options)?;
    if let Some(refused) = &polygons.refused {
        let Some(entity) = refused.entity else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "a typed polygon is three or more corners x,y or x,y,z",
                words.get(*at).cloned().unwrap_or_default(),
            ));
        };
        let id = id_text(entity);
        if refused.why == NotAShape::NoEntity {
            return Err(make_error(
                ErrorCode::NotFound,
                format!("the drawing has no entity {id}"),
                id,
            ));
        }
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            format!(
                "{id} is {}; a polygon is {POLYGON_KINDS}",
                kind_name(document.model().entities.find(entity), refused.why)
            ),
            id,
        ));
    }
    Ok(polygons)
}

// ---- the verbs --------------------------------------------------------------------------------

/// True for VOLUME, and for SURFACE DIFFERENCE and SURFACE ENVELOPE (`args`
/// are the words after the verb), in any case. The surfaces' own SURFACE
/// words are not claimed.
pub fn is_volume_verb(verb: &str, args: &[String]) -> bool {
    if is(verb, "VOLUME") {
        return true;
    }
    is(verb, "SURFACE")
        && args
            .first()
            .is_some_and(|word| is(word, "DIFFERENCE") || is(word, "ENVELOPE"))
}

/// Runs one of them: `tokens` is the whole line split into words with the
/// quotes removed (CommandInterpreter::tokenize), the verb first. `views`
/// answers a FROM VIEW scope; headless it is None and VIEW is refused, naming
/// AREA (the C++ default). Returns the reply records, or an error saying what
/// was refused.
pub fn run_volume_verb(
    document: &mut Document,
    tokens: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    if tokens.len() < 2 {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "VOLUME SURFACES | DATUM | STOCKPILE | SECTIONS | GRID ...; HELP VOLUME gives every \
             form",
        ));
    }
    let method = &tokens[1];
    let words: Vec<String> = tokens[2..].to_vec();
    if is(&tokens[0], "SURFACE") {
        if is(method, "DIFFERENCE") {
            return surface_difference(document, words);
        }
        return surface_envelope(document, words);
    }
    if is(method, "SURFACES") {
        return volume_surfaces(document, words, views);
    }
    if is(method, "DATUM") {
        return volume_datum(document, words, views);
    }
    if is(method, "STOCKPILE") {
        return volume_stockpile(document, words, views);
    }
    if is(method, "SECTIONS") {
        return volume_sections(document, words);
    }
    if is(method, "GRID") {
        return volume_grid(document, words, views);
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "VOLUME is SURFACES, DATUM, STOCKPILE, SECTIONS or GRID; HELP VOLUME gives every form",
        method.as_str(),
    ))
}

/// Every form and option, for HELP VOLUME.
pub fn volume_verb_help() -> String {
    String::from(VOLUME_HELP_TEXT)
}

const VOLUME_HELP_TEXT: &str = r#"Volumes between the drawing's surfaces (docs/volumes.md). Every VOLUME form changes
nothing; csv=<file> also writes the records as a table (OVERWRITE replaces a file).

VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>] [csv=<file>]
          exact, on the overlay of the two: fill where the top is above the base, cut where
          it is below, net = fill - cut; inside the polygon its edges cut the triangles
VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=] [shrink=] [csv=]
          against a level: what stands above it is cut (above=), what lies below is fill
VOLUME STOCKPILE <surface> INSIDE <polygon> [base=boundary|lowest] [level=<z>] [EACH] [bulk=]
          [shrink=] [csv=]
          a pile (above=, the cut that removes it) or a pit (below=) against a base made
          from its boundary: the boundary's heights (its own at every vertex, else the
          surface's), the lowest point of the surface along it, or level=<z>
VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] [from=] [to=] [left=] [right=]
          [method=endarea|prismoidal|both] [curvature=on|off] [bulk=] [shrink=] [csv=]
          cross sections square to the alignment at whole multiples of the interval and at
          its key stations, each area exact; average end area, or prismoidal with a middle
          section in each pair; one record per section with the running totals and the
          mass haul ordinate (cut positive); left= and right= default to across the common
          ground, which a curve or a corner refuses: sections that cross one another count
          ground twice, and so do two stretches that run side by side, so give a reach
          under the curve's radius or under half the distance between the stretches;
          on a curve the volume is corrected for the centroid of each section travelling
          a different distance than the centreline (Pappus; curvature=off gives the plain
          figure along the centreline, curvature.cut= and curvature.fill= say by how much
          it moved the total)
VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=] [shrink=] [csv=]
          the midpoint rule on cells at whole multiples of the cell: a cross-check; inside
          several polygons each cell is counted once, in the first that holds its centre
          (lots that share an edge, or overlap, are not counted twice, and it has no EACH)
SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]
          the top minus the base as a new surface (the cut/fill map), styled Cut Fill; one
          undo step
SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]
          the higher (MAX) or lower (MIN) of the two as a new surface, in the default
          style (it holds real levels, not a difference); one undo step

<polygon>  #id [#id ...] (closed polylines, curve polylines, circles and ellipses) |
           x,y[,z] x,y[,z] x,y[,z] ... |
           FROM SELECTION | DRAWING | VIEW [id] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY]
           [WHERE k=v ...]; several polygons give one total, refused if they overlap
           (GRID counts each cell once instead), and EACH a record per polygon as well
bulk=      multiplies the cut, shrink= the fill, in the reply only (cut.adjusted=,
           fill.adjusted=, net.adjusted=, and the mass ordinate)"#;
