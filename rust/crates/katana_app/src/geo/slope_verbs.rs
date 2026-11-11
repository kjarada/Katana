//! The RASTER SLOPE and RASTER ASPECT verbs (docs/terrain.md, "Slope and
//! aspect"), the port of src/katana_app/geo/slope_verbs.cpp: the slope or
//! aspect of a surface or an elevation raster kept as a derived reference
//! raster of its values, and the slope cut into classes drawn as areas - the
//! same on katana_cli, katana_mcp and the window's command line, where the
//! window's Terrain > Analysis > Slope and Aspect builds these lines.
//!
//! ```text
//! RASTER SLOPE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//!              [unit=percent|degree] [classes=<b1>,<b2>,...] [areas=terrain/slope]
//!              [min_area=<m2>] [NAME <n>] [<scope>] [PREVIEW]
//! RASTER ASPECT <source> [NAME <n>] [<scope>] [PREVIEW]
//! ```
//!
//! GDAL computes both (`raster slope`, `raster aspect`: Horn's 3 x 3 window,
//! the edges interpolated). The raster kept holds the values, at full
//! precision, so another verb can read it as RASTER <id> - zonal statistics of
//! slope, contours of it - and its display copy is coloured: by class when
//! there are classes, else by the slope ramp.
//!
//! classes= are the breaks between slope classes: classes=5,10,25 makes
//! [0,5), [5,10), [10,25) and [25, and up). GDAL's `raster reclassify` sorts
//! the cells into them, `raster sieve` merges regions under min_area into
//! their largest neighbour, and `raster polygonize` draws each region: a
//! closed polyline on <areas>/<class> with slope_class, slope_from, slope_to
//! (none on the last, open class) and slope_unit properties, in one undo step,
//! and a report of each class's area. The first class begins at 0, below which
//! no slope lies: a class bounded by -inf would take in the -9999 no-data GDAL
//! gives a slope it cannot compute.
//!
//! A scope, last on the line, keeps the analysis inside its closed shapes: the
//! raster is cut to their box (a cell wider, so the slope at the boundary is
//! computed from the ground beyond it), the slope then to the shapes - GDAL's
//! raster clip keeps every cell a shape touches - and the class areas, which
//! are whole cells, to the shapes exactly (vector clip), so each class's area
//! is its area inside them.
//!
//! Its `min_area=` is read by terrain_verbs' positive_option ("min_area is
//! greater than zero"), the one the C++ call resolves to: slope_verbs.cpp does
//! not see dem_support.hpp.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use katana_cad::scope_verbs::ScopeWords;
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::layer_path::validate_layer_path;
use katana_geometry::{Box2, Point2};
use katana_gis::processing::{
    self as gp, ArgValue, DatasetValue, Feature, FeatureSet, FeatureTable, FieldDef, FieldType,
    FieldValue, OutputTo, RunOutputs, RunRequest,
};
use katana_gis::{GeoPoint, GeometryKind, VectorGeometry};
use katana_interop::geo::colour_ramps::{built_in_ramp, spread};
use katana_interop::geo::drawing_dataset::{ResultOptions, result_command};
use katana_interop::import::import_raster;
use katana_interop::{RasterDisplayStyle, RasterImportOptions, ReferenceId};
use katana_math::cpp;

use super::analysis_support::take_keyword_values;
use super::bindings::{ApplyRequest, Target, TargetKind, Tokens, apply_outputs, project_crs};
use super::geo_verbs::{Context, Prepared, Progress, make_apply, make_work, trimmed_text};
use super::replies::{fixed3, parse_records, value, warning_record};
use super::terrain_verbs::{
    RasterChain, ScopeAreas, answered_with, areas_wkt, bind_areas, bind_terrain_source,
    gdal_number, joined_records, positive_option, raster_extent, source_name, square_cell_of,
};
use super::vector_support::{WordRules, read_verb_words};
use crate::import_records::path_text;

const SLOPE_USAGE: &str = "RASTER SLOPE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE \
                           <path> [unit=percent|degree] [classes=<b1>,<b2>,...] \
                           [areas=terrain/slope] [min_area=<m2>] [NAME <n>] [<scope>] [PREVIEW]";
const ASPECT_USAGE: &str = "RASTER ASPECT SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE \
                            <path> [NAME <n>] [<scope>] [PREVIEW]";

/// What polygonize names the field holding each region's class.
const CLASS_FIELD: &str = "slope_class";

fn refusal(why: impl Into<String>, word: impl Into<String>) -> Error {
    make_error(ErrorCode::InvalidArgument, why, word)
}

/// An algorithm's path, as the bridge takes it.
fn algorithm(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

#[derive(Clone, Debug)]
struct SlopeWords {
    aspect: bool,
    unit: String,
    breaks: Vec<f64>,
    areas: String,
    areas_given: bool,
    min_area: Option<f64>,
    name: Option<String>,
    scope: Option<ScopeWords>,
    preview: bool,
}

impl Default for SlopeWords {
    fn default() -> Self {
        Self {
            aspect: false,
            unit: String::from("percent"),
            breaks: Vec::new(),
            areas: String::from("terrain/slope"),
            areas_given: false,
            min_area: None,
            name: None,
            scope: None,
            preview: false,
        }
    }
}

fn break_list(text: &str, unit: &str) -> Result<Vec<f64>> {
    let mut breaks: Vec<f64> = Vec::new();
    let mut start = 0;
    while start <= text.len() {
        let comma = text[start..].find(',').map(|at| start + at);
        let item = &text[start..comma.unwrap_or(text.len())];
        let taken = parse_finite_double(item).filter(|&number| {
            number > 0.0
                && breaks.last().is_none_or(|&last| number > last)
                && (unit != "degree" || number < 90.0)
        });
        let Some(number) = taken else {
            return Err(refusal(
                "classes are the slopes between classes, ascending and above 0 (under 90 in \
                 degrees): classes=5,10,25",
                text,
            ));
        };
        breaks.push(number);
        let Some(comma) = comma else {
            break;
        };
        start = comma + 1;
    }
    Ok(breaks)
}

/// The words after the source, read by the shared reader as every GIS verb's
/// are (vector_support's read_verb_words), NAME <n> taken out first.
fn slope_words(tokens: &Tokens, at: usize, aspect: bool) -> Result<SlopeWords> {
    let mut words = SlopeWords {
        aspect,
        ..SlopeWords::default()
    };
    let mut classes: Option<String> = None;
    let split = take_keyword_values(tokens, at, &["NAME"])?;
    for (_, name) in &split.taken {
        if words.name.is_some() {
            return Err(refusal("NAME is given twice", name.clone()));
        }
        if name.is_empty() {
            return Err(refusal("NAME needs the reference raster's name", ""));
        }
        words.name = Some(name.clone());
    }
    let options: &[&str] = if aspect {
        &[]
    } else {
        &["unit", "classes", "areas", "min_area"]
    };
    let rules = WordRules {
        options: options.iter().map(|key| String::from(*key)).collect(),
        flags: vec![String::from("PREVIEW")],
        target: false,
        usage: String::from(if aspect {
            "RASTER ASPECT takes NAME, a scope and PREVIEW"
        } else {
            "RASTER SLOPE takes unit=, classes=, areas=, min_area=, NAME, a scope and PREVIEW"
        }),
        ..WordRules::default()
    };
    let read = read_verb_words(&split.rest, 0, split.rest.size(), &rules)?;
    words.preview = read.has("PREVIEW");
    if read.scope_given {
        words.scope = Some(read.scope.clone());
    }
    for (key, text) in &read.options {
        match key.as_str() {
            "unit" => {
                let unit = text.to_ascii_lowercase();
                if unit != "percent" && unit != "degree" {
                    return Err(refusal("unit is percent or degree", text.clone()));
                }
                words.unit = unit;
            }
            "classes" => classes = Some(text.clone()),
            "areas" => {
                if let Err(valid) = validate_layer_path(text) {
                    return Err(refusal(
                        format!("areas is a layer path: {}", valid.message),
                        text.clone(),
                    ));
                }
                words.areas = text.clone();
                words.areas_given = true;
            }
            "min_area" => {
                words.min_area = Some(positive_option("min_area", text)?);
            }
            _ => {}
        }
    }
    if let Some(classes) = classes {
        // Read after the line, so unit= may come after classes=.
        words.breaks = break_list(&classes, &words.unit)?;
    }
    if words.breaks.is_empty() && (words.min_area.is_some() || words.areas_given) {
        return Err(refusal(
            "areas= and min_area= are the classes' areas: give classes= too",
            "",
        ));
    }
    Ok(words)
}

/// One class of slope: [from, to), the last open above.
#[derive(Clone, Debug, Default)]
struct SlopeClass {
    /// 1-based, what reclassify writes.
    number: i32,
    from: f64,
    to: Option<f64>,
    /// "5-10", "25+": its layer under areas=.
    name: String,
    r: u8,
    g: u8,
    b: u8,
}

fn classes_of(breaks: &[f64]) -> Vec<SlopeClass> {
    let ramp = built_in_ramp("slope").expect("the ramp table has the slope ramp");
    let mut classes = Vec::new();
    let count = breaks.len() + 1;
    for i in 0..count {
        let from = if i == 0 { 0.0 } else { breaks[i - 1] };
        let to = breaks.get(i).copied();
        let name = format!(
            "{}{}",
            gdal_number(from),
            match to {
                Some(to) => format!("-{}", gdal_number(to)),
                None => String::from("+"),
            }
        );
        // The slope ramp (the table's, the one a surface style's slope ramp
        // is) read at the class's place among them: green for the gentlest,
        // red for the steepest.
        let position = if count > 1 {
            i as f64 / (count - 1) as f64
        } else {
            0.5
        };
        let colours = spread(ramp, 0.0, 1.0);
        let after = colours
            .iter()
            .position(|entry| entry.value >= position)
            .expect("the ramp's last stop is at 1, which no position passes");
        let upper = &colours[after];
        let lower = if after == 0 {
            &colours[after]
        } else {
            &colours[after - 1]
        };
        let span = upper.value - lower.value;
        let t = if span > 0.0 {
            (position - lower.value) / span
        } else {
            0.0
        };
        // std::lround: half away from zero, as f64::round.
        let mix = |a: u8, c: u8| -> u8 {
            (f64::from(a) + t * (f64::from(c) - f64::from(a))).round() as u8
        };
        classes.push(SlopeClass {
            number: (i + 1) as i32,
            from,
            to,
            name,
            r: mix(lower.r, upper.r),
            g: mix(lower.g, upper.g),
            b: mix(lower.b, upper.b),
        });
    }
    classes
}

/// GDAL's reclassify mapping: [0,b1)=1; [b1,b2)=2; ...; [bk,inf]=k+1, and the
/// no-data kept no-data.
fn mapping_of(classes: &[SlopeClass]) -> String {
    let mut mapping = String::new();
    for each in classes {
        mapping += &format!(
            "[{},{}={}; ",
            gdal_number(each.from),
            match each.to {
                Some(to) => format!("{})", gdal_number(to)),
                None => String::from("inf]"),
            },
            each.number
        );
    }
    mapping + "NO_DATA=NO_DATA"
}

/// The plan area a ring encloses, by the shoelace rule, whether or not its
/// first vertex is repeated at its end.
fn ring_area(ring: &[GeoPoint]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    // Relative to the first vertex, so large coordinates keep their digits.
    let x0 = ring[0].x;
    let y0 = ring[0].y;
    let mut twice: f64 = 0.0;
    for (i, a) in ring.iter().enumerate() {
        let b = &ring[(i + 1) % ring.len()];
        twice += (a.x - x0) * (b.y - y0) - (b.x - x0) * (a.y - y0);
    }
    twice.abs() / 2.0
}

fn polygon_area(polygon: &VectorGeometry) -> f64 {
    let mut area = 0.0;
    for (r, ring) in polygon.parts.iter().enumerate() {
        area += (if r == 0 { 1.0 } else { -1.0 }) * ring_area(ring);
    }
    area
}

/// The regions polygonize drew, a table per class present, each feature
/// carrying its class's properties; and each class's area and count.
#[derive(Clone, Debug, Default)]
struct ClassAreas {
    set: FeatureSet,
    /// By class, index number - 1.
    area: Vec<f64>,
    /// Likewise.
    regions: Vec<usize>,
}

fn class_areas(drawn: &FeatureSet, classes: &[SlopeClass], unit: &str, crs: &str) -> ClassAreas {
    let mut result = ClassAreas {
        set: FeatureSet::default(),
        area: vec![0.0; classes.len()],
        regions: vec![0; classes.len()],
    };
    let mut tables: Vec<FeatureTable> = classes
        .iter()
        .map(|each| FeatureTable {
            name: each.name.clone(),
            kind: GeometryKind::Polygon,
            crs_wkt: String::from(crs),
            fields: vec![
                FieldDef {
                    name: String::from("slope_class"),
                    r#type: FieldType::Integer64,
                },
                FieldDef {
                    name: String::from("slope_from"),
                    r#type: FieldType::Real,
                },
                FieldDef {
                    name: String::from("slope_to"),
                    r#type: FieldType::Real,
                },
                FieldDef {
                    name: String::from("slope_unit"),
                    r#type: FieldType::String,
                },
            ],
            ..FeatureTable::default()
        })
        .collect();
    let known = 1..=classes.len() as i64;
    for table in &drawn.tables {
        let Some(index) = table.fields.iter().position(|def| def.name == CLASS_FIELD) else {
            continue;
        };
        for feature in &table.features {
            let Some(held) = feature.values.get(index) else {
                continue;
            };
            // std::llround of a real: half away from zero, as f64::round.
            let number = match held {
                FieldValue::Integer64(number) => *number,
                FieldValue::Real(real) => real.round() as i64,
                _ => 0,
            };
            if !known.contains(&number) {
                continue;
            }
            let slot = (number - 1) as usize;
            let each = &classes[slot];
            let made = Feature {
                parts: feature.parts.clone(),
                values: vec![
                    FieldValue::Integer64(i64::from(each.number)),
                    FieldValue::Real(each.from),
                    each.to.map_or(FieldValue::Null, FieldValue::Real),
                    FieldValue::Text(String::from(unit)),
                ],
            };
            for part in &feature.parts {
                result.area[slot] += polygon_area(part);
            }
            result.regions[slot] += 1;
            tables[slot].features.push(made);
        }
    }
    for table in tables {
        if !table.features.is_empty() {
            result.set.tables.push(table);
        }
    }
    result
}

/// What the work hands the apply.
#[derive(Clone, Debug, Default)]
struct Analysed {
    /// The slope or aspect, full precision.
    values: PathBuf,
    /// Its display copy, coloured.
    picture: PathBuf,
    records: Vec<String>,
    areas: Option<ClassAreas>,
    warnings: Vec<String>,
}

/// Aspect is a direction: a grey ramp would draw north (0) and north (360) as
/// black and white. Eight compass colours round the circle, north at both
/// ends: hues from ColorBrewer's schemes, a constant of their own and not a
/// row of the ramp table (docs/surface_display.md, "Not done").
const COMPASS: [[u8; 3]; 9] = [
    [215, 25, 28],
    [253, 174, 97],
    [255, 255, 191],
    [166, 217, 106],
    [26, 150, 65],
    [43, 131, 186],
    [94, 60, 153],
    [194, 75, 160],
    [215, 25, 28],
];

#[allow(
    clippy::too_many_arguments,
    reason = "The C++'s analyse() takes these eight, each the job's own copy."
)]
fn analyse(
    input: DatasetValue,
    words: &SlopeWords,
    classes: &[SlopeClass],
    areas: &ScopeAreas,
    crs: &str,
    scratch: &Path,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
) -> Result<Analysed> {
    let mut chain = RasterChain::new(input, scratch);
    let mut result = Analysed::default();
    let facts = chain.info()?;
    let gt = facts.geotransform;
    let cell = cpp::max(gt[1].hypot(gt[4]), gt[2].hypot(gt[5]));
    if words.scope.is_some() {
        // A cell wider than the areas, within the raster: Horn's window at
        // the boundary then reads real ground, not an edge rule.
        let wanted = areas.r#box.inflated(cell);
        let extent = raster_extent(&facts);
        let cut = Box2::new(
            Point2::new(
                cpp::max(wanted.min.x, extent.min.x),
                cpp::max(wanted.min.y, extent.min.y),
            ),
            Point2::new(
                cpp::min(wanted.max.x, extent.max.x),
                cpp::min(wanted.max.y, extent.max.y),
            ),
        );
        if cut.empty() || cut.width() < cell || cut.height() < cell {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the scope's closed shapes lie off the raster",
            ));
        }
        chain.step(
            &algorithm(&["raster", "clip"]),
            vec![format!(
                "--bbox={},{},{},{}",
                gdal_number(cut.min.x),
                gdal_number(cut.min.y),
                gdal_number(cut.max.x),
                gdal_number(cut.max.y)
            )],
            stop,
            progress,
            Vec::new(),
        )?;
    }
    if words.aspect {
        chain.step(
            &algorithm(&["raster", "aspect"]),
            Vec::new(),
            stop,
            progress,
            Vec::new(),
        )?;
    } else {
        chain.step(
            &algorithm(&["raster", "slope"]),
            vec![format!("--unit={}", words.unit)],
            stop,
            progress,
            Vec::new(),
        )?;
    }
    if words.scope.is_some() {
        chain.step(
            &algorithm(&["raster", "clip"]),
            vec![format!("--geometry={}", areas_wkt(&areas.areas))],
            stop,
            progress,
            Vec::new(),
        )?;
    }
    static MADE: AtomicU64 = AtomicU64::new(0);
    let stem = format!(
        "{}{}",
        if words.aspect {
            "katana-aspect-"
        } else {
            "katana-slope-"
        },
        MADE.fetch_add(1, Ordering::Relaxed) + 1
    );
    result.values = chain.keep(&scratch.join(format!("{stem}.tif")))?;
    let kept = chain.info()?;
    let side = square_cell_of(&kept);
    let cell_area = (kept.geotransform[1] * kept.geotransform[5]
        - kept.geotransform[2] * kept.geotransform[4])
        .abs();
    result.records.push(format!(
        "{} raster={}x{} cell={}",
        if words.aspect {
            String::from("aspect convention=azimuth flat=nodata")
        } else {
            format!("slope unit={} method=horn", words.unit)
        },
        kept.width,
        kept.height,
        side.map(format_exact_real).unwrap_or_default()
    ));

    let mut map = String::new();
    let mut colour_tokens: Vec<String> = Vec::new();
    if !words.aspect && !classes.is_empty() {
        chain.step(
            &algorithm(&["raster", "reclassify"]),
            vec![
                format!("--mapping={}", mapping_of(classes)),
                String::from("--ot=Int16"),
            ],
            stop,
            progress,
            Vec::new(),
        )?;
        if let Some(min_area) = words.min_area {
            // The least region kept, in whole cells: a region is sieved when
            // it covers fewer cells than min_area does.
            let cells = (min_area / cell_area).ceil();
            chain.step(
                &algorithm(&["raster", "sieve"]),
                vec![format!("--size-threshold={}", gdal_number(cells))],
                stop,
                progress,
                Vec::new(),
            )?;
            result.records.push(format!(
                "sieved min_area={} cells={}",
                gdal_number(min_area),
                gdal_number(cells)
            ));
        }
        let mut regions = chain.features(
            &algorithm(&["raster", "polygonize"]),
            vec![format!("--attribute-name={CLASS_FIELD}")],
            stop,
            progress,
        )?;
        if words.scope.is_some() && !regions.tables.is_empty() {
            // The regions are whole cells, and the raster's clip keeps every
            // cell a shape touches: cut to the shapes themselves, so a class's
            // area is its area inside them and the classes add up to them.
            let cut = RunRequest {
                path: algorithm(&["vector", "clip"]),
                values: vec![(
                    String::from("input"),
                    ArgValue::Dataset(DatasetValue::Features(regions)),
                )],
                tokens: vec![format!("--geometry={}", areas_wkt(&areas.areas))],
                output_to: OutputTo::Memory,
                ..RunRequest::default()
            };
            let inside = gp::run(&cut, stop, progress)?;
            regions = inside.features.unwrap_or_default();
            for warning in &inside.diagnostics {
                result.warnings.push(warning.message.clone());
            }
        }
        result.areas = Some(class_areas(&regions, classes, &words.unit, crs));
        for each in classes {
            map += &format!("{} {} {} {} 255\n", each.number, each.r, each.g, each.b);
        }
        colour_tokens.push(String::from("--color-selection=exact"));
    } else if !words.aspect {
        // No classes: the slope ramp over the slope's own range, which GDAL
        // reads itself from the percentages.
        let ramp = built_in_ramp("slope").expect("the ramp table has the slope ramp");
        for colour in &ramp.stops {
            map += &format!(
                "{}% {} {} {} 255\n",
                gdal_number(colour.position * 100.0),
                colour.r,
                colour.g,
                colour.b
            );
        }
    } else {
        for (i, colour) in COMPASS.iter().enumerate() {
            map += &format!(
                "{} {} {} {} 255\n",
                gdal_number(45.0 * i as f64),
                colour[0],
                colour[1],
                colour[2]
            );
        }
    }
    map += "nv 0 0 0 0\n";
    let written = chain.write("colours.txt", &map)?;
    let mut tokens = vec![
        format!("--color-map={}", path_text(&written)),
        String::from("--add-alpha"),
    ];
    tokens.extend(colour_tokens);
    chain.step(
        &algorithm(&["raster", "color-map"]),
        tokens,
        stop,
        progress,
        Vec::new(),
    )?;
    result.picture = chain.keep(&scratch.join(format!("{stem}-picture.tif")))?;
    for warning in &chain.warnings {
        result.warnings.push(warning.message.clone());
    }
    Ok(result)
}

fn prepare_slope_or_aspect(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
    aspect: bool,
) -> Result<Prepared> {
    let mut at = 2;
    let verb = if aspect {
        "RASTER ASPECT"
    } else {
        "RASTER SLOPE"
    };
    let source = bind_terrain_source(context, tokens, &mut at, verb)?;
    let words = slope_words(tokens, at, aspect)?;
    // No breaks, no classes: the slope alone.
    let classes = if words.breaks.is_empty() {
        Vec::new()
    } else {
        classes_of(&words.breaks)
    };
    let mut records = vec![source.record.clone()];
    let mut areas = ScopeAreas::default();
    if let Some(scope) = &words.scope {
        areas = bind_areas(context, scope, "area")?;
        records.extend(areas.records.iter().cloned());
    }
    let name = match &words.name {
        Some(name) => name.clone(),
        None => format!(
            "{}{}",
            source_name(context, &source.source),
            if aspect { "-aspect" } else { "-slope" }
        ),
    };
    let nothing_inside = words.scope.is_some() && areas.areas.is_empty();
    if words.preview || nothing_inside {
        // A scope with no closed shape has nothing to analyse: said, not
        // refused.
        records.push(format!(
            "{} name={} made=no",
            if aspect { "aspect" } else { "slope" },
            value(&name)
        ));
        if words.preview {
            records.push(String::from("preview valid=yes changed=no"));
        }
        return Ok(answered_with(verb, joined_records(&records)));
    }
    if !words.breaks.is_empty() {
        for each in &classes {
            let layer = format!("{}/{}", words.areas, each.name);
            if let Err(valid) = validate_layer_path(&layer) {
                return Err(refusal(
                    format!("a class's layer is not a layer path: {}", valid.message),
                    layer,
                ));
            }
        }
    }

    let options = words;
    let dataset = std::sync::Arc::clone(&source.dataset);
    let scratch = context.scratch.to_path_buf();
    let command = String::from(trimmed_text(line));
    let crs = project_crs(context);
    let work = make_work(move |stop, progress| {
        let input = dataset()?;
        let analysed = analyse(
            input, &options, &classes, &areas, &crs, &scratch, stop, progress,
        )?;
        Ok(make_apply(move |ctx| {
            let mut reply = records;
            reply.extend(analysed.records.iter().cloned());
            // The values, as a derived reference raster another verb can
            // read.
            let request = ApplyRequest {
                target: Target {
                    kind: TargetKind::Reference,
                    name: name.clone(),
                    ..Target::default()
                },
                default_name: name.clone(),
                result: ResultOptions {
                    command_name: command.clone(),
                    ..ResultOptions::default()
                },
                ..ApplyRequest::default()
            };
            let outputs = RunOutputs {
                file: Some(path_text(&analysed.values)),
                ..RunOutputs::default()
            };
            let applied = match apply_outputs(ctx, &request, outputs) {
                Ok(applied) => applied,
                Err(error) => {
                    let _ = std::fs::remove_file(&analysed.picture);
                    return Err(error);
                }
            };
            let found = parse_records(&applied);
            reply.push(applied);
            // Drawn as its coloured picture, whose grid is the values' own.
            let mut picture = import_raster(&analysed.picture, &RasterImportOptions::default());
            let _ = std::fs::remove_file(&analysed.picture);
            for record in &found {
                let id = if record.kind == "output" {
                    record.get("id")
                } else {
                    None
                };
                let number = id.and_then(parse_integer);
                let Some(raster) =
                    number.and_then(|number| ctx.reference.find_raster_mut(number as ReferenceId))
                else {
                    continue;
                };
                if let Ok(picture) = &mut picture {
                    raster.rgba = std::mem::take(&mut picture.rgba);
                    raster.width = picture.width;
                    raster.height = picture.height;
                    raster.geotransform = picture.geotransform;
                }
                if !options.aspect {
                    raster.display_style = RasterDisplayStyle::Slope;
                }
            }
            if let Err(error) = &picture {
                reply.push(warning_record(&format!(
                    "the coloured picture could not be read, so the raster is drawn grey: {}",
                    error.message
                )));
            }
            if let Some(changed) = &ctx.front_end.changed {
                changed();
            }
            if let Some(drawn) = &analysed.areas {
                // The class areas: one command, one undo step.
                let result = ResultOptions {
                    target_layer: if drawn.set.tables.len() == 1 {
                        format!("{}/{}", options.areas, drawn.set.tables[0].name)
                    } else {
                        options.areas.clone()
                    },
                    operation: String::from("RASTER SLOPE"),
                    command_name: command.clone(),
                    ..ResultOptions::default()
                };
                let mut plan = result_command(ctx.document.model(), &drawn.set, &result)?;
                if let Some(step) = plan.command.take() {
                    ctx.document.execute(step)?;
                }
                reply.push(format!(
                    "output arg=areas kind=vector target=layer layer={} created={}",
                    value(&options.areas),
                    plan.created
                ));
                for each in &classes {
                    let slot = (each.number - 1) as usize;
                    reply.push(format!(
                        "class name={} from={} to={} unit={} area={} polygons={}",
                        value(&each.name),
                        gdal_number(each.from),
                        each.to.map(gdal_number).unwrap_or_default(),
                        options.unit,
                        fixed3(drawn.area[slot]),
                        drawn.regions[slot]
                    ));
                }
                for each in &classes {
                    reply.push(format!(
                        "legend value={} r={} g={} b={}",
                        gdal_number(each.from),
                        each.r,
                        each.g,
                        each.b
                    ));
                }
            }
            for warning in &analysed.warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from(if aspect { "Aspect" } else { "Slope Analysis" }),
        reply: None,
        work: Some(work),
    })
}

pub fn prepare_slope(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    prepare_slope_or_aspect(context, tokens, line, false)
}

pub fn prepare_aspect(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    prepare_slope_or_aspect(context, tokens, line, true)
}

/// RASTER SLOPE's usage on one line (terrain_verbs.hpp's slopeUsage). Nothing
/// calls it, in the C++ either: the verb table carries the usage HELP shows.
#[allow(
    dead_code,
    reason = "Ported as the C++ declares it; the verb table holds its own usage text, so nothing calls this one."
)]
pub fn slope_usage() -> String {
    String::from(SLOPE_USAGE)
}

/// RASTER ASPECT's usage on one line (terrain_verbs.hpp's aspectUsage).
#[allow(
    dead_code,
    reason = "Ported as the C++ declares it; the verb table holds its own usage text, so nothing calls this one."
)]
pub fn aspect_usage() -> String {
    String::from(ASPECT_USAGE)
}
