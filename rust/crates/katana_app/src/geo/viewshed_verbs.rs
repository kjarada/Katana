//! The RASTER VIEWSHED and LOS verbs (docs/terrain.md, "Viewshed and line of
//! sight"), the port of src/katana_app/geo/viewshed_verbs.cpp: what can be
//! seen from observers over a surface or an elevation raster, kept as a tinted
//! derived reference raster with the visible area drawn and measured if asked;
//! and one sight line between two points - the same on katana_cli, katana_mcp
//! and the window's command line, where Terrain > Analysis > Viewshed and Line
//! of Sight builds these lines.
//!
//! ```text
//! RASTER VIEWSHED SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//!                 (OBSERVER x,y)... | OBSERVERS [<scope>] [height=1.7] [target=0]
//!                 [max=<m>] [curvature=<k>|none] [areas=<layer>] [NAME <n>] [PREVIEW]
//! LOS SURFACE <name> | RASTER <id|name> | FILE <path> OBSERVER x,y TARGET x,y
//!     [height=1.7] [target=0] [curvature=<k>|none] [step=<m>]
//!     [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]
//! ```
//!
//! GDAL computes a viewshed (`raster viewshed`, from one --position): each
//! observer is its own run, and several are unioned here, cell by cell on the
//! raster's grid - GDAL's cumulative mode refuses a position (the
//! investigators' finding), and a count of observers is not the union a person
//! asks for. `height=` is the eye above the ground, `target=` the height above
//! the ground a cell counts as seen at, `max=` the distance beyond which
//! nothing is computed, and `curvature=` GDAL's curvature and refraction
//! coefficient (0.85714 unless given; `none` is 0). The kept raster holds 1
//! where some observer sees the cell and 0 elsewhere; its display copy is
//! tinted, the unseen cells clear. `areas=<layer>` draws the visible area as
//! closed polylines (GDAL's `raster polygonize`), one undo step, and the reply
//! measures it: visible_cells= and area=.
//!
//! A sight line is native (katana_terrain's line_of_sight): no algorithm of
//! GDAL's answers one line with its clearance. The ground is a surface on its
//! own triangles, or a raster's file through GDAL's interpolation.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use katana_cad::scope_verbs::{match_scope, scope_record as cad_scope_record};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::layer_path::validate_layer_path;
use katana_entity::{EntityId, Geometry};
use katana_geometry::Point2;
use katana_gis::gdal_adapter::{describe_crs, same_crs};
use katana_gis::processing::{
    self as gp, ArgValue, DatasetValue, Feature, FeatureSet, FeatureTable, FieldValue, OutputTo,
    RasterGrid, RunOutputs, RunRequest, Scalar,
};
use katana_gis::raster_sampling::resampling_named;
use katana_gis::{GdalDataset, RasterInfo, Resampling};
use katana_interop::geo::drawing_dataset::{ResultOptions, result_command};
use katana_interop::import::import_raster;
use katana_interop::{RasterImportOptions, ReferenceId};
use katana_math::cpp;
use katana_terrain::line_of_sight::{DEFAULT_CURVATURE, SightOptions, line_of_sight};

use super::analysis_support::{
    bind_ground, open_ground, point_of, point_text, take_keyword_values,
};
use super::bindings::{ApplyRequest, Target, TargetKind, Tokens, apply_outputs, project_crs};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, trimmed_text};
use super::replies::{fixed3, parse_records, value, warning_record};
use super::terrain_verbs::{
    RasterChain, bind_terrain_source, raster_extent, raster_info_of, source_name,
};
use super::vector_support::{
    Applied, VerbWords, WordRules, answered, execute_step, field_index, gis_record, joined,
    not_run_record, number_option, output_record, progress_span, read_verb_words,
};
use crate::import_records::{path_from_text, path_text};

const VIEWSHED_USAGE: &str = "RASTER VIEWSHED SURFACE <name> [CELL <m>] | RASTER <id|name> | \
                              FILE <path> (OBSERVER x,y)... | OBSERVERS [<scope>] [height=1.7] \
                              [target=0] [max=<m>] [curvature=<k>|none] [areas=<layer>] \
                              [NAME <n>] [PREVIEW]";
const LOS_USAGE: &str = "LOS SURFACE <name> | RASTER <id|name> | FILE <path> OBSERVER x,y TARGET \
                         x,y [height=1.7] [target=0] [curvature=<k>|none] [step=<m>] \
                         [method=bilinear|nearest|cubic|cubicspline] [PREVIEW]";

/// An eye height: a person standing, the height a sight line is usually asked
/// from; any other is height=.
const EYE_HEIGHT: f64 = 1.7;
/// Each observer is a whole GDAL run over the raster: a hundred is already
/// minutes on a large DEM, and more is a scope that took the wrong layer.
const MAX_OBSERVERS: usize = 100;
/// What GDAL writes for a visible cell (its --visible-value default, set
/// explicitly so a changed default cannot change the union).
const VISIBLE_VALUE: f64 = 255.0;

fn refusal(why: impl Into<String>, word: impl Into<String>) -> Error {
    make_error(ErrorCode::InvalidArgument, why, word)
}

/// An algorithm's path, as the bridge takes it.
fn algorithm(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// Keys and flags, as a verb's rules name them.
fn listed(words: &[&str]) -> Vec<String> {
    algorithm(words)
}

/// The options both verbs share.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SightWords {
    height: f64,
    target: f64,
    curvature: f64,
}

fn sight_words(words: &VerbWords) -> Result<SightWords> {
    let height = number_option(words, "height")?.unwrap_or(EYE_HEIGHT);
    let target = number_option(words, "target")?.unwrap_or(0.0);
    if height < 0.0 || target < 0.0 {
        return Err(refusal(
            "height= and target= are heights above the ground, 0 or more",
            "",
        ));
    }
    let mut curvature = DEFAULT_CURVATURE;
    if let Some(text) = words.option("curvature") {
        if text.eq_ignore_ascii_case("none") {
            curvature = 0.0;
        } else {
            match parse_finite_double(text) {
                Some(number) if !(number < 0.0) => curvature = number,
                _ => {
                    return Err(refusal(
                        "curvature is a coefficient of 0 or more, or none",
                        format!("curvature={text}"),
                    ));
                }
            }
        }
    }
    Ok(SightWords {
        height,
        target,
        curvature,
    })
}

/// A station of a sight line: a place computed along it, so to the millimetre
/// like the distances beside it (fixed3), where a point the user gave is
/// echoed exactly (point_text). At full precision a station read
/// 149.43965517241378,119.50431034482759.
fn station_text(point: &Point2) -> String {
    format!("{},{}", fixed3(point.x), fixed3(point.y))
}

#[derive(Clone, Copy, Debug)]
struct Observer {
    at: Point2,
    entity: Option<EntityId>,
}

/// One observer's run, read back: its grid and which cells it sees.
#[derive(Clone, Debug, Default)]
struct Seen {
    geotransform: [f64; 6],
    width: i32,
    height: i32,
    /// Row major, 1 seen.
    visible: Vec<u8>,
    /// Seen.
    cells: usize,
}

fn read_seen(outputs: &RunOutputs) -> Result<Seen> {
    let mut seen = Seen::default();
    let mut values: Vec<f64> = Vec::new();
    if let Some(raster) = &outputs.raster {
        seen.geotransform = raster.info.geotransform;
        seen.width = raster.info.width;
        seen.height = raster.info.height;
        if let Some(band) = raster.bands.first() {
            values = band.clone();
        }
    } else if let Some(file) = &outputs.file {
        let opened = GdalDataset::open(&path_from_text(file))?;
        let info = opened.raster_info()?;
        seen.geotransform = info.geotransform;
        seen.width = info.width;
        seen.height = info.height;
        values = opened.read_band(1)?;
    } else {
        return Err(Error::new(
            ErrorCode::CommandRejected,
            "raster viewshed wrote no raster",
        ));
    }
    seen.visible = values
        .iter()
        .map(|&cell| u8::from(cell == VISIBLE_VALUE))
        .collect();
    seen.cells = seen.visible.iter().map(|&cell| usize::from(cell)).sum();
    Ok(seen)
}

/// The runs unioned on one grid: every run's grid is a window of the input's
/// (GDAL clamps it to max=), so each lies at a whole number of cells from the
/// first; the union covers them all.
fn union_of(runs: &[Seen], crs: &str) -> RasterGrid {
    let base = runs[0].geotransform;
    let mut left = i64::MAX;
    let mut top = left;
    let mut right = i64::MIN;
    let mut bottom = right;
    let mut offsets: Vec<(i64, i64)> = Vec::new();
    for run in runs {
        // std::llround: half away from zero, as f64::round.
        let column = ((run.geotransform[0] - base[0]) / base[1]).round() as i64;
        let row = ((run.geotransform[3] - base[3]) / base[5]).round() as i64;
        offsets.push((column, row));
        left = left.min(column);
        top = top.min(row);
        right = right.max(column + i64::from(run.width));
        bottom = bottom.max(row + i64::from(run.height));
    }
    let mut geotransform = base;
    geotransform[0] = base[0] + left as f64 * base[1] + top as f64 * base[2];
    geotransform[3] = base[3] + left as f64 * base[4] + top as f64 * base[5];
    let info = RasterInfo {
        width: (right - left) as i32,
        height: (bottom - top) as i32,
        band_count: 1,
        has_geotransform: true,
        projection_wkt: String::from(crs),
        geotransform,
        ..RasterInfo::default()
    };
    let width = info.width as usize;
    let mut band = vec![0.0; width * info.height as usize];
    for (run, &(column, row)) in runs.iter().zip(&offsets) {
        let column0 = column - left;
        let row0 = row - top;
        for y in 0..run.height {
            for x in 0..run.width {
                if run.visible[y as usize * run.width as usize + x as usize] == 0 {
                    continue;
                }
                let at = (row0 + i64::from(y)) as usize * width + (column0 + i64::from(x)) as usize;
                band[at] = 1.0;
            }
        }
    }
    // Int16: a type every GDAL names alike, which polygonize reads as whole
    // numbers.
    RasterGrid {
        info,
        data_type: String::from("Int16"),
        bands: vec![band],
        no_data: vec![None],
    }
}

/// What the viewshed's work hands its apply.
#[derive(Clone, Debug, Default)]
struct Viewed {
    grid: RasterGrid,
    picture: PathBuf,
    areas: Option<FeatureSet>,
    observer_records: Vec<String>,
    visible_cells: usize,
    visible_area: f64,
    warnings: Vec<String>,
    seconds: f64,
}

/// A run's folder, removed however the work ends.
struct Cleanup {
    folder: PathBuf,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

// ---- RASTER VIEWSHED ----------------------------------------------------------------------------

pub fn prepare_raster_viewshed(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    let mut at = 2;
    let source = bind_terrain_source(context, tokens, &mut at, "RASTER VIEWSHED")?;
    let split = take_keyword_values(tokens, at, &["OBSERVER", "NAME"])?;
    let rules = WordRules {
        options: listed(&["height", "target", "max", "curvature", "areas"]),
        flags: listed(&["OBSERVERS", "PREVIEW"]),
        target: false,
        usage: String::from(VIEWSHED_USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(&split.rest, 0, split.rest.size(), &rules)?;
    let sight = sight_words(&words)?;
    let max_distance = number_option(&words, "max")?;
    if let Some(max) = max_distance
        && !(max > 0.0)
    {
        return Err(refusal(
            "max is a distance above 0",
            format!("max={}", format_exact_real(max)),
        ));
    }
    let mut areas = String::new();
    if let Some(layer) = words.option("areas") {
        if let Err(valid) = validate_layer_path(layer) {
            return Err(refusal(
                format!("areas is a layer path: {}", valid.message),
                layer.clone(),
            ));
        }
        areas = layer.clone();
    }
    let mut name: Option<String> = None;
    let mut observers: Vec<Observer> = Vec::new();
    for (keyword, text) in &split.taken {
        if keyword == "NAME" {
            if name.is_some() || text.is_empty() {
                return Err(refusal(
                    "NAME is the reference raster's name, once",
                    text.clone(),
                ));
            }
            name = Some(text.clone());
            continue;
        }
        let Some(point) = point_of(text) else {
            return Err(refusal(
                "OBSERVER is a point: OBSERVER 120.5,40",
                text.clone(),
            ));
        };
        observers.push(Observer {
            at: point,
            entity: None,
        });
    }
    let from_scope = words.has("OBSERVERS");
    if from_scope && !observers.is_empty() {
        return Err(refusal(
            "observers are OBSERVER x,y or OBSERVERS <scope>, not both",
            "",
        ));
    }
    if !from_scope && words.scope_given {
        return Err(refusal(
            "a scope names observers only after OBSERVERS: OBSERVERS LAYERS survey",
            "",
        ));
    }
    if !from_scope && observers.is_empty() {
        return Err(refusal(
            format!("RASTER VIEWSHED needs an observer: {VIEWSHED_USAGE}"),
            "",
        ));
    }
    let mut records = vec![source.record.clone()];
    if from_scope {
        let found = match_scope(
            context.document,
            &words.scope,
            context.interpreter.scope_context(),
        )?;
        let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
        for &id in &found.matched {
            let Some(entity) = context.document.model().entities.find(id) else {
                continue;
            };
            if let Geometry::Point(point) = &entity.geometry {
                observers.push(Observer {
                    at: point.position,
                    entity: Some(id),
                });
            } else {
                let kind = katana_entity::entity::to_string(entity.r#type()).to_ascii_lowercase();
                *skipped.entry(kind).or_insert(0) += 1;
            }
        }
        let mut record = format!(
            "scope arg=observers {} points={}",
            cad_scope_record(&found),
            observers.len()
        );
        for (kind, count) in &skipped {
            record += &format!(" skipped.{kind}={count}");
        }
        records.push(record);
    }
    if observers.len() > MAX_OBSERVERS {
        return Err(refusal(
            format!("at most {MAX_OBSERVERS} observers: each is a whole run over the raster"),
            "",
        ));
    }
    let settings = format!(
        "viewshed observers={} height={} target={} max={} curvature={}",
        observers.len(),
        format_exact_real(sight.height),
        format_exact_real(sight.target),
        max_distance.map(format_exact_real).unwrap_or_default(),
        format_exact_real(sight.curvature)
    );
    let raster_name =
        name.unwrap_or_else(|| format!("{}-viewshed", source_name(context, &source.source)));
    if observers.is_empty() {
        records.insert(0, not_run_record("viewshed", "observers=0"));
        records.push(settings);
        return Ok(answered("RASTER VIEWSHED", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=viewshed preview=yes"));
        records.push(format!("{settings} name={}", value(&raster_name)));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("RASTER VIEWSHED", joined(&records)));
    }

    let dataset = Arc::clone(&source.dataset);
    let scratch = context.scratch.to_path_buf();
    let crs = project_crs(context);
    let options = sight;
    let reach = max_distance;
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let input = dataset()?;
        let facts = raster_info_of(&input)?;
        // The observers are the drawing's points, handed to GDAL as the
        // raster's coordinates: in another system they stand somewhere else,
        // and the result was stamped with the project's system whatever the
        // raster's was (review finding). Refused when both are known and
        // differ, as the design has the curated raster tools do.
        if same_crs(&facts.projection_wkt, &crs) == Some(false) {
            return Err(Error::new(
                ErrorCode::InvalidCrs,
                format!(
                    "the raster is in {} and the drawing in {}: the observers would stand \
                     somewhere else; RASTER REPROJECT the raster into the project's first",
                    describe_crs(&facts.projection_wkt),
                    describe_crs(&crs)
                ),
            ));
        }
        let extent = raster_extent(&facts);
        for observer in &observers {
            if !((extent.min.x..=extent.max.x).contains(&observer.at.x)
                && (extent.min.y..=extent.max.y).contains(&observer.at.y))
            {
                return Err(refusal(
                    "the observer is off the raster",
                    point_text(&observer.at),
                ));
            }
        }
        static MADE: AtomicU64 = AtomicU64::new(0);
        let run = format!("{}", MADE.fetch_add(1, Ordering::Relaxed) + 1);
        let folder = scratch.join(format!("katana-viewshed-{run}"));
        let _ = std::fs::create_dir_all(&folder);
        // The runs' files go with the folder, however the work ends.
        let _cleanup = Cleanup {
            folder: folder.clone(),
        };

        let mut viewed = Viewed::default();
        let mut runs: Vec<Seen> = Vec::new();
        let share = 0.8 / observers.len() as f64;
        for (i, observer) in observers.iter().enumerate() {
            // Its own copy of the input: never a dataset shared between runs.
            // The eye height rides as the position's third value, X,Y,H: GDAL
            // 3.13 ignores --height beside a two-value position and looks
            // from its default 2 m instead (a 1 m wall 20 m off then shadows
            // to 20 x 2 / 1 = 40 m whatever height= said; measured).
            let mut values = vec![
                (String::from("input"), ArgValue::Dataset(input.clone())),
                (
                    String::from("position"),
                    ArgValue::Scalar(Scalar::RealList(vec![
                        observer.at.x,
                        observer.at.y,
                        options.height,
                    ])),
                ),
                (
                    String::from("target-height"),
                    ArgValue::Scalar(Scalar::Real(options.target)),
                ),
                (
                    String::from("curvature-coefficient"),
                    ArgValue::Scalar(Scalar::Real(options.curvature)),
                ),
                (
                    String::from("visible-value"),
                    ArgValue::Scalar(Scalar::Real(VISIBLE_VALUE)),
                ),
            ];
            if let Some(reach) = reach {
                values.push((
                    String::from("max-distance"),
                    ArgValue::Scalar(Scalar::Real(reach)),
                ));
            }
            let request = RunRequest {
                path: algorithm(&["raster", "viewshed"]),
                values,
                output_to: OutputTo::Memory,
                max_memory_cells: 0,
                spill_directory: path_text(&folder),
                ..RunRequest::default()
            };
            let span = progress_span(progress, share * i as f64, share * (i + 1) as f64);
            let outputs = gp::run(&request, stop, span.as_deref())?;
            for warning in &outputs.diagnostics {
                viewed.warnings.push(warning.message.clone());
            }
            let seen = read_seen(&outputs)?;
            viewed.observer_records.push(format!(
                "observer at={}{} visible_cells={}",
                point_text(&observer.at),
                observer
                    .entity
                    .map(|entity| format!(" entity={entity}"))
                    .unwrap_or_default(),
                seen.cells
            ));
            runs.push(seen);
        }
        // The raster's own system: the one GDAL computed in, the project's
        // only for a raster that declares none.
        viewed.grid = union_of(
            &runs,
            if facts.projection_wkt.is_empty() {
                &crs
            } else {
                &facts.projection_wkt
            },
        );
        let gt = viewed.grid.info.geotransform;
        let cell_area = (gt[1] * gt[5] - gt[2] * gt[4]).abs();
        viewed.visible_cells = viewed.grid.bands[0]
            .iter()
            .filter(|&&cell| cell == 1.0)
            .count();
        viewed.visible_area = viewed.visible_cells as f64 * cell_area;

        let mut chain = RasterChain::new(DatasetValue::Raster(viewed.grid.clone()), &scratch);
        if !areas.is_empty() && viewed.visible_cells != 0 {
            let span = progress_span(progress, 0.8, 0.9);
            let regions = chain.features(
                &algorithm(&["raster", "polygonize"]),
                vec![String::from("--attribute-name=visible")],
                stop,
                span.as_deref(),
            )?;
            // The seen regions only, with no field of their own: the layer
            // says what they are.
            let mut seen = FeatureSet::default();
            for table in &regions.tables {
                let field = field_index(table, "visible");
                let mut kept = FeatureTable {
                    name: String::from("visible"),
                    kind: table.kind,
                    crs_wkt: table.crs_wkt.clone(),
                    ..FeatureTable::default()
                };
                for feature in &table.features {
                    let held = field.and_then(|field| feature.values.get(field));
                    let is_seen = matches!(held, Some(FieldValue::Integer64(1)))
                        || matches!(held, Some(FieldValue::Real(real)) if *real == 1.0);
                    if is_seen {
                        kept.features.push(Feature {
                            parts: feature.parts.clone(),
                            ..Feature::default()
                        });
                    }
                }
                if !kept.features.is_empty() {
                    seen.tables.push(kept);
                }
            }
            viewed.areas = Some(seen);
        }
        // The display copy: seen cells tinted, the rest clear. No "nv" entry:
        // the union grid has no no-data value, and GDAL warns on every run
        // that it ignores one.
        let colours = chain.write("colours.txt", "1 255 140 0 150\n0 0 0 0 0\n")?;
        let span = progress_span(progress, 0.9, 1.0);
        chain.step(
            &algorithm(&["raster", "color-map"]),
            vec![
                format!("--color-map={}", path_text(&colours)),
                String::from("--add-alpha"),
                String::from("--color-selection=exact"),
            ],
            stop,
            span.as_deref(),
            Vec::new(),
        )?;
        viewed.picture = chain.keep(&scratch.join(format!("katana-viewshed-picture-{run}.tif")))?;
        for warning in &chain.warnings {
            viewed.warnings.push(warning.message.clone());
        }
        viewed.seconds = start.elapsed().as_secs_f64();
        Ok(make_apply(move |ctx| {
            let mut reply = vec![gis_record("viewshed", viewed.seconds)];
            reply.extend(records.iter().cloned());
            reply.extend(viewed.observer_records.iter().cloned());
            let request = ApplyRequest {
                target: Target {
                    kind: TargetKind::Reference,
                    name: raster_name.clone(),
                    ..Target::default()
                },
                default_name: raster_name.clone(),
                result: ResultOptions {
                    command_name: command_name.clone(),
                    ..ResultOptions::default()
                },
                ..ApplyRequest::default()
            };
            let outputs = RunOutputs {
                raster: Some(viewed.grid.clone()),
                ..RunOutputs::default()
            };
            let applied = match apply_outputs(ctx, &request, outputs) {
                Ok(applied) => applied,
                Err(error) => {
                    let _ = std::fs::remove_file(&viewed.picture);
                    return Err(error);
                }
            };
            let found = parse_records(&applied);
            reply.push(applied);
            // Drawn as its tinted picture, whose grid is the values' own.
            let mut picture = import_raster(&viewed.picture, &RasterImportOptions::default());
            let _ = std::fs::remove_file(&viewed.picture);
            for record in &found {
                let id = if record.kind == "output" {
                    record.get("id")
                } else {
                    None
                };
                let number = id.and_then(parse_integer);
                let raster =
                    number.and_then(|number| ctx.reference.find_raster_mut(number as ReferenceId));
                if let Some(raster) = raster
                    && let Ok(picture) = &mut picture
                {
                    raster.rgba = std::mem::take(&mut picture.rgba);
                    raster.width = picture.width;
                    raster.height = picture.height;
                    raster.geotransform = picture.geotransform;
                }
            }
            if let Err(error) = &picture {
                reply.push(warning_record(&format!(
                    "the tinted picture could not be read, so the viewshed is drawn grey: {}",
                    error.message
                )));
            }
            if let Some(changed) = &ctx.front_end.changed {
                changed();
            }
            if let Some(set) = &viewed.areas {
                let result = ResultOptions {
                    target_layer: areas.clone(),
                    operation: String::from("raster viewshed"),
                    command_name: command_name.clone(),
                    ..ResultOptions::default()
                };
                let mut plan = result_command(ctx.document.model(), set, &result)?;
                let drawn = Applied {
                    created: plan.created,
                    skipped: plan.skipped,
                    ..Applied::default()
                };
                execute_step(ctx, plan.command.take())?;
                let record = output_record(&areas, &drawn);
                reply.push(format!(
                    "output arg=areas{}",
                    &record["output arg=output".len()..]
                ));
                for warning in &plan.warnings {
                    reply.push(warning_record(warning));
                }
            }
            reply.push(format!(
                "{settings} visible_cells={} area={}",
                viewed.visible_cells,
                fixed3(viewed.visible_area)
            ));
            for warning in &viewed.warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("Viewshed"),
        reply: None,
        work: Some(work),
    })
}

// ---- LOS ----------------------------------------------------------------------------------------

pub fn prepare_line_of_sight(
    context: &mut Context<'_>,
    tokens: &Tokens,
    _line: &str,
) -> Result<Prepared> {
    let mut at = 1;
    let ground = bind_ground(context, tokens, &mut at, "LOS")?;
    let split = take_keyword_values(tokens, at, &["OBSERVER", "TARGET"])?;
    let rules = WordRules {
        options: listed(&["height", "target", "curvature", "step", "method"]),
        flags: listed(&["PREVIEW"]),
        target: false,
        usage: String::from(LOS_USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(&split.rest, 0, split.rest.size(), &rules)?;
    if words.scope_given {
        return Err(refusal(format!("LOS reads no scope: {LOS_USAGE}"), ""));
    }
    let sight = sight_words(&words)?;
    let mut observer: Option<Point2> = None;
    let mut target: Option<Point2> = None;
    for (keyword, text) in &split.taken {
        let Some(point) = point_of(text) else {
            return Err(refusal(
                format!("{keyword} is a point: {keyword} 120.5,40"),
                text.clone(),
            ));
        };
        let slot = if keyword == "OBSERVER" {
            &mut observer
        } else {
            &mut target
        };
        if slot.is_some() {
            return Err(refusal(format!("{keyword} is given twice"), text.clone()));
        }
        *slot = Some(point);
    }
    let (Some(observer), Some(target)) = (observer, target) else {
        return Err(refusal(
            format!("LOS needs an OBSERVER and a TARGET: {LOS_USAGE}"),
            "",
        ));
    };
    let mut method = Resampling::Bilinear;
    if let Some(named) = words.option("method") {
        if ground.surface.is_some() {
            return Err(refusal(
                "a surface is read on its own triangles; method= is for a raster",
                format!("method={named}"),
            ));
        }
        method = resampling_named(named)?;
    }
    let step = number_option(&words, "step")?;
    if let Some(step) = step
        && !(step > 0.0)
    {
        return Err(refusal(
            "step is a distance above 0",
            format!("step={}", format_exact_real(step)),
        ));
    }
    let length = (target.x - observer.x).hypot(target.y - observer.y);
    let mut records = vec![ground.record.clone()];
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=los preview=yes"));
        records.push(format!(
            "sight observer={} target={} distance={}",
            point_text(&observer),
            point_text(&target),
            fixed3(length)
        ));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("LOS", joined(&records)));
    }

    let from = ground;
    let options = sight;
    let given = step;
    let eye = observer;
    let aim = target;
    let work = make_work(move |stop, _progress| {
        let start = Instant::now();
        let open = open_ground(&from, method)?;
        // Half a raster cell resolves the raster; a surface, read exactly,
        // every 10 cm - finer than survey triangles - or a millionth of the
        // line when that is longer.
        let walk = SightOptions {
            observer_height: options.height,
            target_height: options.target,
            curvature: options.curvature,
            step: given
                .unwrap_or_else(|| open.spacing.unwrap_or_else(|| cpp::max(0.1, length / 1e6))),
        };
        let line = line_of_sight(&*open.at, eye, aim, &walk, stop)?;
        let mut record = format!(
            "sight visible={} observer={} target={} distance={} observer_z={} target_z={}",
            if line.visible { "yes" } else { "no" },
            point_text(&eye),
            point_text(&aim),
            fixed3(line.distance),
            fixed3(line.observer_z),
            fixed3(line.target_z)
        );
        if let (Some(clearance), Some(clearance_at), Some(clearance_distance)) =
            (line.clearance, line.clearance_at, line.clearance_distance)
        {
            record += &format!(
                " clearance={} clearance_at={} clearance_distance={}",
                fixed3(clearance),
                station_text(&clearance_at),
                fixed3(clearance_distance)
            );
        }
        if let (Some(blocked_at), Some(blocked_distance), Some(blocked_ground)) =
            (line.blocked_at, line.blocked_distance, line.blocked_ground)
        {
            record += &format!(
                " blocked_at={} blocked_distance={} blocked_ground={}",
                station_text(&blocked_at),
                fixed3(blocked_distance),
                fixed3(blocked_ground)
            );
        }
        record += &format!(
            " stations={} unknown={} step={} curvature={}",
            line.stations,
            line.unknown,
            format_exact_real(walk.step),
            format_exact_real(walk.curvature)
        );
        let unknown = line.unknown;
        let seconds = start.elapsed().as_secs_f64();
        Ok(make_apply(move |_ctx| {
            let mut reply = vec![gis_record("los", seconds)];
            reply.extend(records);
            reply.push(record);
            if unknown != 0 {
                reply.push(warning_record(&format!(
                    "{unknown} stations of the sight line are off the ground, and nothing there \
                     can hide the target"
                )));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("Line of Sight"),
        reply: None,
        work: Some(work),
    })
}
