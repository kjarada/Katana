//! RASTER GRID: surveyed points to a DEM (docs/terrain.md, "Gridding points to
//! a DEM"; docs/geoprocessing.md, "T6"), the port of
//! src/katana_app/geo/grid_verbs.cpp.
//!
//! ```text
//! RASTER GRID [<scope>] [method=linear|invdist|invdistnn|nearest|average|...]
//!             [cell=<m> | size=<columns>x<rows>] [z=geometry|<property>]
//!             [extent=scope|x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>]
//!             [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>] | TO SURFACE <name>]
//!             [OVERWRITE] [PREVIEW]
//! ```
//!
//! The points - and the vertices of lines and areas - the scope takes are
//! gridded by GDAL's `vector grid <method>`. The methods are the catalogue's,
//! read at run time, so a method GDAL adds is offered with no change here.
//!
//! Heights. With z=geometry (the default) a vertex's height is the drawing's
//! (entity's heights_of; a curve polyline's are its vertices' own): an entity
//! without a height at every vertex is left out and counted
//! (skipped.heightless), because GDAL reads a 2D point as z = 0 - a surface
//! pulled down to the datum with no error anywhere. With z=<property> GDAL
//! reads the property (--zfield), and an entity without a number there is left
//! out and counted (skipped.no_z): GDAL reads a word in the field as 0
//! (measured: "x" on a point at the centre of four at 100 m made the centre 0,
//! where it skips a field left null and gives 100).
//!
//! The grid. GDAL takes a resolution only with an extent, and rounds the
//! extent to a whole number of cells by STRETCHING the cells (measured: 10 m
//! at a 3 m cell came back as 3 cells of 3.333 m). So the extent defaults to
//! the bounds of the vertices used, grown outwards to whole cells from the
//! origin - every datum inside the grid, every cell the size asked for, and
//! grids of one cell size line up - and a given extent keeps its lower-left
//! corner and grows up and right to whole cells. size= takes the extent as it
//! is. Cells no datum reaches hold interop's GRID_NO_DATA, the surfaces'
//! no-data value, declared on the band: GDAL's default of 0 would be read as
//! ground at the datum.
//!
//! Its power=, radius= and cell= are read by dem_support's positive_option
//! ("<key>= is a positive number"), the one the C++ calls resolve to, and its
//! records are joined by dem_support's join_records.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use katana_cad::scope_verbs::{ScopeWords, is_scope_word, parse_scope_words};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::{Box2, Point2};
use katana_gis::GeometryKind;
use katana_gis::processing::{
    self as gp, AlgorithmSpec, ArgValue, DatasetValue, Feature, FeatureSet, FieldValue, OutputTo,
    RunRequest, catalogue, describe, validate,
};
use katana_interop::geo::drawing_dataset::ResultOptions;
use katana_interop::geo::{DrawingDataset, DrawingDatasetOptions, GRID_NO_DATA};
use katana_interop::terrain_io::suggested_cell_size;
use katana_math::cpp;
use katana_math::tolerance::GEOMETRIC;

use super::bindings::{
    ApplyRequest, Source, Target, TargetKind, Tokens, apply_outputs, bind_drawing, derived_folder,
    input_record, parse_target, project_crs, scope_record,
};
use super::dem_support::{
    box_option, join_records, path_of_utf8, positive_option, split_options, utf8_of_path,
};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, trimmed_text};
use super::replies::{fixed3, joined, value, warning_record};

const GRID_OPTIONS: [&str; 7] = ["method", "cell", "size", "z", "extent", "power", "radius"];
/// The most cells a grid may have: the surfaces' own cap (surface_grid), a
/// 5000 x 5000 grid of doubles, 200 MB.
const MAX_GRID_CELLS: u64 = 25_000_000;
const DEFAULT_NAME: &str = "dem";
/// interop's suggested_cell_size's default: about 1000 cells along the
/// longer side (terrain_io.hpp).
const SUGGESTED_CELLS: i32 = 1000;

/// The methods `vector grid` has in this GDAL: its leaves, in its order.
fn grid_methods() -> Vec<String> {
    catalogue()
        .iter()
        .filter(|info| {
            !info.container
                && info.path.len() == 3
                && info.path[0] == "vector"
                && info.path[1] == "grid"
        })
        .map(|info| info.path[2].clone())
        .collect()
}

fn listed(words: &[String]) -> String {
    joined(words, ", ")
}

fn has_argument(spec: &AlgorithmSpec, name: &str) -> bool {
    spec.args.iter().any(|arg| arg.name == name)
}

/// size=<columns>x<rows> (or a comma between them).
fn size_option(text: &str) -> Result<(i32, i32)> {
    let refuse = || {
        make_error(
            ErrorCode::InvalidArgument,
            "size= is columns x rows, two whole numbers from 1: size=400x300",
            format!("size={text}"),
        )
    };
    let Some(split) = text.find(['x', 'X', ',']) else {
        return Err(refuse());
    };
    let columns = parse_finite_double(&text[..split]);
    let rows = parse_finite_double(&text[split + 1..]);
    let refused = |columns: f64, rows: f64| {
        columns < 1.0
            || rows < 1.0
            || columns != columns.floor()
            || rows != rows.floor()
            || columns * rows > MAX_GRID_CELLS as f64
    };
    match (columns, rows) {
        (Some(columns), Some(rows)) if !refused(columns, rows) => Ok((columns as i32, rows as i32)),
        _ => Err(refuse()),
    }
}

/// The number of whole cells that cover `span`: a span within
/// katana_math::tolerance::GEOMETRIC of a whole number of cells is that
/// number, so the rounding of a span worked from projected coordinates adds no
/// cell.
fn whole_cells(span: f64, cell: f64) -> f64 {
    cpp::max(1.0, ((span - GEOMETRIC) / cell).ceil())
}

/// The cell line at or below `at`, a coordinate within
/// katana_math::tolerance::GEOMETRIC of a line being on it. The allowance is
/// in model units, not in cells: at an MGA northing a cell count near 6e7 has
/// an ulp of 7.5e-9, so the absolute 1e-9 cells this once allowed vanished in
/// the rounding and 6250000.3 / 0.1 = 62500002.99999999 put the grid a row low
/// (GEOMETRIC is ~50 ulp at the largest projected coordinates).
fn cell_line_below(at: f64, cell: f64) -> f64 {
    ((at + GEOMETRIC) / cell).floor() * cell
}

/// A feature's value of the field at `index`, as a number: a real or an
/// integer, or text that reads as one. Anything else - absent, a word - is no
/// height.
fn numeric_field(feature: &Feature, index: usize) -> bool {
    match feature.values.get(index) {
        Some(FieldValue::Real(real)) => real.is_finite(),
        Some(FieldValue::Integer64(_)) => true,
        Some(FieldValue::Text(text)) => parse_finite_double(trimmed(text.as_bytes())).is_some(),
        _ => false,
    }
}

/// Leaves out, and counts as no_z, the features with no number in the field
/// `key`, keeping the scope record's counts true to what GDAL is given.
fn keep_numeric(dataset: &mut DrawingDataset, key: &str) {
    let mut dropped = 0;
    for table in &mut dataset.set.tables {
        // The last field of that name, as the C++ finds it.
        let mut index = None;
        for (f, field) in table.fields.iter().enumerate() {
            if field.name == key {
                index = Some(f);
            }
        }
        let before = table.features.len();
        table
            .features
            .retain(|feature| index.is_some_and(|index| numeric_field(feature, index)));
        let removed = before - table.features.len();
        dropped += removed;
        let count = match table.kind {
            GeometryKind::Point => &mut dataset.stats.points,
            GeometryKind::Polygon => &mut dataset.stats.polygons,
            _ => &mut dataset.stats.lines,
        };
        *count = count.saturating_sub(removed);
    }
    dataset
        .set
        .tables
        .retain(|table| !table.features.is_empty());
    if dropped != 0 {
        dataset.stats.used = dataset.stats.used.saturating_sub(dropped);
        *dataset
            .stats
            .skipped
            .entry(String::from("no_z"))
            .or_insert(0) += dropped;
    }
}

/// The box round every vertex GDAL will read.
fn vertex_bounds(set: &FeatureSet) -> Box2 {
    let mut found = Box2::default();
    for table in &set.tables {
        for feature in &table.features {
            for geometry in &feature.parts {
                for part in &geometry.parts {
                    for point in part {
                        found.expand(Point2::new(point.x, point.y));
                    }
                }
            }
        }
    }
    found
}

fn real(number: f64) -> String {
    format_exact_real(number)
}

pub fn prepare_raster_grid(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    let words = split_options(tokens, 2, &GRID_OPTIONS, "RASTER GRID")?;
    let option = |key: &str| -> Option<String> { words.options.get(key).cloned() };

    // The scope, NAME, TO and the flags.
    let rest = &words.rest;
    let mut scope: Option<ScopeWords> = None;
    let mut target: Option<Target> = None;
    let mut name = String::new();
    let mut preview = false;
    let mut overwrite = false;
    let mut at = 0;
    while at < rest.size() {
        if rest.is(at, "NAME") {
            if at + 1 >= rest.size() || !name.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "NAME takes the reference raster's name, once",
                    rest[at].clone(),
                ));
            }
            name = rest[at + 1].clone();
            at += 2;
        } else if rest.is(at, "TO") {
            if target.is_some() {
                return Err(Error::new(ErrorCode::InvalidArgument, "one TO per line"));
            }
            at += 1;
            target = Some(parse_target(rest, &mut at)?);
        } else if rest.is(at, "PREVIEW") {
            preview = true;
            at += 1;
        } else if rest.is(at, "OVERWRITE") {
            overwrite = true;
            at += 1;
        } else if scope.is_none() && !rest.quoted[at] && is_scope_word(&rest[at]) {
            scope = Some(parse_scope_words(&rest.words, &mut at)?);
        } else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "RASTER GRID takes a scope, method=, cell= or size=, z=, extent=, power=, \
                 radius=, NAME, TO, OVERWRITE and PREVIEW",
                rest[at].clone(),
            ));
        }
    }

    // The method, from the catalogue.
    let method = option("method")
        .unwrap_or_else(|| String::from("linear"))
        .to_ascii_lowercase();
    let methods = grid_methods();
    if !methods.contains(&method) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "method= is one of GDAL's gridding methods: {}",
                listed(&methods)
            ),
            format!("method={method}"),
        ));
    }
    let path = vec![String::from("vector"), String::from("grid"), method.clone()];
    let spec = describe(&path)?;
    for key in ["power", "radius"] {
        if let Some(given) = option(key)
            && !has_argument(&spec, key)
        {
            let mut taking = Vec::new();
            for other in &methods {
                let described =
                    describe(&[String::from("vector"), String::from("grid"), other.clone()]);
                if let Ok(described) = described
                    && has_argument(&described, key)
                {
                    taking.push(other.clone());
                }
            }
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "{key}= is for the methods {}, not {method}",
                    listed(&taking)
                ),
                format!("{key}={given}"),
            ));
        }
    }
    let power = option("power")
        .map(|text| positive_option("power", &text))
        .transpose()?;
    let radius = option("radius")
        .map(|text| positive_option("radius", &text))
        .transpose()?;

    // The grid's size: a cell, or columns and rows - never both.
    if option("cell").is_some() && option("size").is_some() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "cell= and size= both say how big the grid is; give one",
        ));
    }
    let mut cell = option("cell")
        .map(|text| positive_option("cell", &text))
        .transpose()?;
    let size = option("size").map(|text| size_option(&text)).transpose()?;
    let mut given_extent: Option<[f64; 4]> = None;
    if let Some(text) = option("extent")
        && !text.eq_ignore_ascii_case("scope")
    {
        given_extent = Some(box_option("extent", &text)?);
    }
    let z_text = option("z").unwrap_or_else(|| String::from("geometry"));
    let z_from_geometry = z_text.eq_ignore_ascii_case("geometry");

    // Where the result goes: a reference raster unless TO says otherwise.
    let to = target.unwrap_or_default();
    if matches!(
        to.kind,
        TargetKind::Layer | TargetKind::Selection | TargetKind::Report
    ) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "RASTER GRID makes a raster: TO REFERENCE [<name>] keeps it as a reference raster, \
             TO FILE <path> writes it, TO SURFACE <name> keeps it as a surface",
        ));
    }
    if !name.is_empty() && !to.name.is_empty() && to.kind == TargetKind::Reference {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "NAME and TO REFERENCE both name the raster; give one",
        ));
    }
    if to.kind == TargetKind::File && path_of_utf8(&to.name).exists() && !overwrite {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "the file exists; add OVERWRITE to replace it",
            to.name.clone(),
        ));
    }

    // The drawing's points: what the scope takes, heights required unless
    // they come from a property.
    let options = DrawingDatasetOptions {
        crs_wkt: project_crs(context),
        require_heights: z_from_geometry,
        ..DrawingDatasetOptions::default()
    };
    let mut bound = bind_drawing(context, &scope.unwrap_or_default(), &options)?;
    if !z_from_geometry {
        keep_numeric(&mut bound.dataset, &z_text);
    }
    let drawing = Source::default();
    let mut records = vec![
        input_record("input", &drawing),
        scope_record("input", &bound),
    ];
    for warning in &bound.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    let z_record = if z_from_geometry {
        String::from("geometry")
    } else {
        value(&z_text)
    };
    let head = format!(
        "grid method={method} algorithm={} z={z_record}",
        value(&gp::path_text(&path))
    );
    let set = &bound.dataset.set;
    if set.tables.is_empty() {
        // Nothing to grid is said, as a scope that takes nothing is, and
        // nothing runs.
        records.insert(0, format!("{head} ran=no"));
        return Ok(Prepared {
            title: String::from("RASTER GRID"),
            reply: Some(join_records(&records)),
            work: None,
        });
    }

    // The grid: extent and cell or size.
    let data = vertex_bounds(set);
    let (mut x0, mut y0, mut x1, mut y1) = match given_extent {
        Some(extent) => (extent[0], extent[1], extent[2], extent[3]),
        None => (data.min.x, data.min.y, data.max.x, data.max.y),
    };
    if cell.is_none() && size.is_none() {
        cell = Some(suggested_cell_size(
            &Box2::new(Point2::new(x0, y0), Point2::new(x1, y1)),
            SUGGESTED_CELLS,
        ));
    }
    let columns;
    let rows;
    let xres;
    let yres;
    if let Some(cell) = cell {
        if given_extent.is_none() {
            // Outwards to whole cells from the origin, so grids of one cell
            // size line up and a datum on the edge is inside.
            x0 = cell_line_below(x0, cell);
            y0 = cell_line_below(y0, cell);
        }
        let across = whole_cells(x1 - x0, cell);
        let down = whole_cells(y1 - y0, cell);
        if across * down > MAX_GRID_CELLS as f64 {
            let fits = ((x1 - x0) * (y1 - y0) / MAX_GRID_CELLS as f64).sqrt();
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "a {} cell makes {} x {} cells, more than {MAX_GRID_CELLS}; a cell of {} or \
                     more fits",
                    real(cell),
                    real(across),
                    real(down),
                    fixed3(fits)
                ),
                format!("cell={}", real(cell)),
            ));
        }
        columns = across as i32;
        rows = down as i32;
        x1 = x0 + f64::from(columns) * cell;
        y1 = y0 + f64::from(rows) * cell;
        xres = cell;
        yres = cell;
    } else {
        if !(x1 > x0) || !(y1 > y0) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the points span no area for size= to divide; give extent=",
            ));
        }
        let (across, down) = size.expect("a grid given no cell= was given size=");
        columns = across;
        rows = down;
        xres = (x1 - x0) / f64::from(columns);
        yres = (y1 - y0) / f64::from(rows);
    }
    let extent = format!("{},{},{},{}", real(x0), real(y0), real(x1), real(y1));

    let mut request_tokens = vec![format!("--extent={extent}")];
    if let Some(cell) = cell {
        request_tokens.push(format!("--resolution={},{}", real(cell), real(cell)));
    } else {
        request_tokens.push(format!("--size={columns},{rows}"));
    }
    request_tokens.push(format!("--nodata={}", real(GRID_NO_DATA)));
    // Every table a scope made: GDAL grids one layer unless told which.
    for table in &set.tables {
        request_tokens.push(format!("--input-layer={}", table.name));
    }
    if !z_from_geometry {
        request_tokens.push(format!("--zfield={z_text}"));
    }
    if let Some(power) = power {
        request_tokens.push(format!("--power={}", real(power)));
    }
    if let Some(radius) = radius {
        request_tokens.push(format!("--radius={}", real(radius)));
    }
    let mut request = RunRequest {
        path: path.clone(),
        tokens: request_tokens,
        values: vec![(
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Features(set.clone())),
        )],
        overwrite,
        ..RunRequest::default()
    };
    if to.kind == TargetKind::File {
        request.output_to = OutputTo::File;
        request.output_path = to.name.clone();
        request.output_format = to.format.clone();
    } else {
        // Written once, where a derived raster is kept (the GDAL verb's rule).
        request.output_to = OutputTo::Memory;
        request.max_memory_cells = 0;
        request.spill_directory = utf8_of_path(&derived_folder(context));
    }

    let cell_text = if xres == yres {
        real(xres)
    } else {
        format!("{},{}", real(xres), real(yres))
    };
    let grid = format!("{head} cell={cell_text} extent={extent} size={columns}x{rows}");
    if preview {
        validate(&request)?;
        records.insert(0, format!("{grid} preview=yes"));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(Prepared {
            title: String::from("RASTER GRID"),
            reply: Some(join_records(&records)),
            work: None,
        });
    }

    let mut apply = ApplyRequest {
        target: to.clone(),
        default_name: String::from(DEFAULT_NAME),
        result: ResultOptions {
            operation: gp::path_text(&path),
            command_name: String::from(trimmed_text(line)),
            ..ResultOptions::default()
        },
        ..ApplyRequest::default()
    };
    if to.kind == TargetKind::Reference || to.kind == TargetKind::Default {
        apply.target.kind = TargetKind::Reference;
        apply.target.name = if to.name.is_empty() {
            name
        } else {
            to.name.clone()
        };
    }

    let work = make_work(move |stop, progress| {
        let outputs = gp::run(&request, stop, progress)?;
        Ok(make_apply(move |ctx| {
            let seconds = outputs.seconds;
            let applied = apply_outputs(ctx, &apply, outputs)?;
            let mut reply = vec![format!("{grid} seconds={}", fixed3(seconds))];
            reply.extend(records);
            reply.push(applied);
            Ok(join_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("RASTER GRID"),
        reply: None,
        work: Some(work),
    })
}
