//! The CONTOUR verb (docs/terrain.md "Contours"), the port of
//! src/katana_app/geo/contour_verbs.cpp: contour lines of a surface or an
//! elevation raster drawn as polylines, the same on katana_cli, katana_mcp and
//! the window's command line, where Terrain > Analysis > Contours builds these
//! lines.
//!
//! ```text
//! CONTOUR SURFACE <name> | RASTER <id|name> | FILE <path> interval=<m> [major=5]
//!         [base=0] [layer=terrain/contours] [smooth=3|5] [<scope>] [PREVIEW]
//! ```
//!
//! One verb, two engines, one output: a SURFACE is traced exactly on its
//! triangles (katana_terrain's contours, the tracer the window's surfaces have
//! had and never showed); a RASTER or FILE is contoured by GDAL's `raster
//! contour` at its full resolution, always with --elevation-name (without it
//! GDAL writes no level) and --3d, and its lines turned into the tracer's
//! Contour. So which contour is major, its layer and its height are decided in
//! one place (interop's geo::contour_entities): polylines on <layer>/major and
//! <layer>/minor, each vertex at the level, in one undo step.
//!
//! A scope, last on the line, gives the closed shapes the contours are kept
//! inside: both engines' lines are cut at the boundary itself. A raster is
//! first cut by GDAL to the areas' box, a margin of cells wider so the cut
//! lines still reach the boundary, and so only the site is contoured. Open
//! shapes in the scope bound nothing: counted and said. smooth= runs GDAL's
//! gaussian `raster neighbors` over a raster first (3 or 5 cells, the sizes
//! GDAL has), for presentation, and the reply says smoothed=yes.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use katana_cad::scope_verbs::ScopeWords;
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_integer};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::layer_path::validate_layer_path;
use katana_geometry::{Box2, Point2};
use katana_gis::processing::DatasetValue;
use katana_interop::geo::contour_entities::{
    CONTOUR_ELEVATION_FIELD, clip_contours, contour_command, contours_from_features,
};
use katana_interop::geo::{ContourBoundary, ContourEntityOptions, ContourPlan};
use katana_math::cpp;
use katana_terrain::{Contour, MAX_CONTOUR_LEVELS, TinSurface, contours};

use super::bindings::{SourceKind, Tokens};
use super::geo_verbs::{
    Context, Prepared, Progress, make_apply, make_work, stop_requested, trimmed_text,
};
use super::replies::{value, warning_record, yes_no};
use super::terrain_verbs::{
    RasterChain, ScopeAreas, answered_with, bind_areas, bind_terrain_source, gdal_number,
    joined_records, number_option, positive_option, raster_extent, square_cell_of, value_range,
};
use super::vector_support::{WordRules, read_verb_words};

fn refusal(why: impl Into<String>, word: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, why, word)
}

#[derive(Clone, Debug, PartialEq)]
struct ContourWords {
    interval: Option<f64>,
    major_every: usize,
    base: f64,
    layer: String,
    smooth: i32,
    scope: Option<ScopeWords>,
    preview: bool,
}

impl Default for ContourWords {
    fn default() -> Self {
        Self {
            interval: None,
            major_every: 5,
            base: 0.0,
            layer: String::from("terrain/contours"),
            smooth: 0,
            scope: None,
            preview: false,
        }
    }
}

impl ContourWords {
    /// The interval, which contour_words never leaves out.
    fn interval(&self) -> f64 {
        self.interval
            .expect("CONTOUR's words always carry an interval")
    }
}

/// The words after the source, read by the shared reader (vector_support's
/// read_verb_words) as every GIS verb's are: an option may stand anywhere,
/// after the filter too, except a WHERE key after WHERE - `layer=` there is
/// the filter's LAYER=, which is what the dialog's "Only those that match"
/// writes.
fn contour_words(tokens: &Tokens, at: usize) -> Result<ContourWords> {
    let rules = WordRules {
        options: Vec::from(["interval", "major", "base", "layer", "smooth"].map(String::from)),
        flags: vec![String::from("PREVIEW")],
        target: false,
        usage: String::from(
            "CONTOUR takes interval=, major=, base=, layer=, smooth=, a scope and PREVIEW",
        ),
        ..WordRules::default()
    };
    let read = read_verb_words(tokens, at, tokens.size(), &rules)?;
    let mut words = ContourWords {
        preview: read.has("PREVIEW"),
        scope: read.scope_given.then(|| read.scope.clone()),
        ..ContourWords::default()
    };
    for (key, text) in &read.options {
        if key == "interval" {
            words.interval = Some(positive_option("interval", text)?);
        } else if key == "major" {
            match parse_integer(text) {
                Some(every) if every >= 0 => words.major_every = every as usize,
                _ => {
                    return Err(refusal(
                        "major is every how many contours is a major one, 0 for none",
                        text,
                    ));
                }
            }
        } else if key == "base" {
            words.base = number_option("base", text)?;
        } else if key == "layer" {
            if let Err(invalid) = validate_layer_path(text) {
                return Err(refusal(
                    format!("layer is a layer path: {}", invalid.message),
                    text,
                ));
            }
            words.layer = text.clone();
        } else if key == "smooth" {
            match parse_integer(text) {
                // GDAL's gaussian kernel has these two sizes.
                Some(size @ (0 | 3 | 5)) => words.smooth = size as i32,
                _ => {
                    return Err(refusal(
                        "smooth is 3 or 5 cells (GDAL's gaussian kernels), or 0 for none",
                        text,
                    ));
                }
            }
        }
    }
    if words.interval.is_none() {
        return Err(refusal(
            "CONTOUR needs interval=<m>, the height between contours",
            "",
        ));
    }
    Ok(words)
}

/// The contours of the report: `contours method=tin|grid cell= levels=
/// count= major= minor= layer= smoothed=`.
fn contours_record(
    method: &str,
    cell: Option<f64>,
    plan: &ContourPlan,
    layer: &str,
    smoothed: bool,
) -> String {
    format!(
        "contours method={method} cell={} levels={} count={} major={} minor={} layer={} \
         smoothed={}",
        cell.map(format_exact_real).unwrap_or_default(),
        plan.levels,
        plan.created,
        plan.major,
        plan.minor,
        value(layer),
        yes_no(smoothed)
    )
}

/// How many levels base + k * interval lie in [low, high].
fn levels_between(low: f64, high: f64, interval: f64, base: f64) -> f64 {
    ((high - base) / interval).floor() - ((low - base) / interval).ceil() + 1.0
}

/// A raster's values read on a stride of about a million cells: what the
/// level count is judged by before GDAL is asked for millions of lines. A
/// sample's range is within the whole's, so a count over it that is already
/// too many is certainly too many.
const LEVEL_COUNT_SAMPLES: usize = 1_000_000;

fn too_many_levels(levels: f64) -> Error {
    make_error(
        ErrorCode::InvalidArgument,
        "contour interval is too small for the relief of the raster",
        format!(
            "levels={} limit={MAX_CONTOUR_LEVELS}",
            format_exact_real(levels)
        ),
    )
}

/// What the work hands the apply: the contours, and how they were made.
#[derive(Clone, Debug, Default, PartialEq)]
struct Traced {
    contours: Vec<Contour>,
    method: String,
    cell: Option<f64>,
    warnings: Vec<String>,
}

fn trace_surface(
    surface: &TinSurface,
    words: &ContourWords,
    areas: &[ContourBoundary],
    stop: Option<&AtomicBool>,
) -> Result<Traced> {
    // On the shared task pool, as the C++'s default pool is.
    let traced = contours(
        surface,
        words.interval(),
        words.base,
        words.major_every,
        None,
    )?;
    if stop_requested(stop) {
        return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
    }
    Ok(Traced {
        contours: if words.scope.is_some() {
            clip_contours(&traced, areas)
        } else {
            traced
        },
        method: String::from("tin"),
        ..Traced::default()
    })
}

fn trace_raster(
    input: DatasetValue,
    words: &ContourWords,
    areas: &[ContourBoundary],
    area_box: &Box2,
    scratch: &Path,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
) -> Result<Traced> {
    let mut chain = RasterChain::new(input, scratch);
    let facts = chain.info()?;
    let mut result = Traced {
        method: String::from("grid"),
        cell: square_cell_of(&facts),
        ..Traced::default()
    };
    if words.scope.is_some() {
        // Cut to the areas' box, a margin wider: a contour is traced up to
        // the last cell centre, and the smoothing kernel reads its half-width
        // beyond, so the lines still reach the boundary where they are cut.
        // Within the raster: past its edge GDAL would fill the box with 0
        // where the raster has no no-data value, and contour the cliff.
        let gt = &facts.geotransform;
        let cell = cpp::max(gt[1].hypot(gt[4]), gt[2].hypot(gt[5]));
        let wanted = area_box.inflated(cell * (1.0 + f64::from(words.smooth / 2)));
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
            return Ok(result); // the areas lie off the raster: no contour there
        }
        chain.step(
            &[String::from("raster"), String::from("clip")],
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
    if words.smooth != 0 {
        chain.step(
            &[String::from("raster"), String::from("neighbors")],
            vec![
                String::from("--kernel=gaussian"),
                format!("--size={}", words.smooth),
            ],
            stop,
            progress,
            Vec::new(),
        )?;
    }
    let interval = words.interval();
    if let Some((low, high)) = value_range(chain.current(), LEVEL_COUNT_SAMPLES)? {
        let levels = levels_between(low, high, interval, words.base);
        if levels > MAX_CONTOUR_LEVELS as f64 {
            return Err(too_many_levels(levels));
        }
    }
    let mut tokens = vec![
        format!("--interval={}", gdal_number(interval)),
        format!("--elevation-name={CONTOUR_ELEVATION_FIELD}"),
        String::from("--3d"),
    ];
    if words.base != 0.0 {
        tokens.push(format!("--offset={}", gdal_number(words.base)));
    }
    let features = chain.features(
        &[String::from("raster"), String::from("contour")],
        tokens,
        stop,
        progress,
    )?;
    let traced = contours_from_features(
        &features,
        CONTOUR_ELEVATION_FIELD,
        interval,
        words.base,
        words.major_every,
    )?;
    result.contours = if words.scope.is_some() {
        clip_contours(&traced, areas)
    } else {
        traced
    };
    for warning in &chain.warnings {
        result.warnings.push(warning.message.clone());
    }
    Ok(result)
}

/// A CONTOUR line (verb_table.rs, the T1 block).
pub fn prepare_contour(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let mut at = 1;
    let source = bind_terrain_source(context, tokens, &mut at, "CONTOUR")?;
    let words = contour_words(tokens, at)?;
    let tin = source.source.kind == SourceKind::Surface;
    let mut records = Vec::new();
    if tin {
        // Traced on the triangles: no cell is read, whatever CELL said.
        if source.cell_given {
            return Err(refusal(
                "a surface is contoured exactly on its triangles; CELL is for a surface sampled \
                 into a grid",
                &source.source.surface,
            ));
        }
        if words.smooth != 0 {
            return Err(refusal(
                "smooth= is a raster's; a surface is contoured exactly on its triangles",
                "",
            ));
        }
        records.push(format!(
            "input arg=input source=surface name={}",
            value(&source.source.surface)
        ));
    } else {
        records.push(source.record.clone());
    }

    let mut areas = ScopeAreas::default();
    if let Some(scope) = &words.scope {
        areas = bind_areas(context, scope, "boundary")?;
        records.extend_from_slice(&areas.records);
    }
    let nothing_inside = words.scope.is_some() && areas.areas.is_empty();
    if words.preview || nothing_inside {
        // A scope with no closed shape keeps no contour: said, not refused.
        records.push(contours_record(
            if tin { "tin" } else { "grid" },
            None,
            &ContourPlan::default(),
            &words.layer,
            words.smooth != 0,
        ));
        records.push(if words.preview {
            String::from("preview valid=yes changed=no")
        } else {
            format!(
                "output arg=output kind=vector target=layer layer={} created=0",
                value(&words.layer)
            )
        });
        return Ok(answered_with("CONTOUR", joined_records(&records)));
    }

    let layer = words.layer.clone();
    let command = String::from(trimmed_text(line));
    let surface = source.surface.clone();
    let dataset = source.dataset.clone();
    let scratch = context.scratch.to_path_buf();
    let options = words;
    let work = make_work(move |stop, progress| {
        let traced = match &surface {
            Some(surface) => trace_surface(surface, &options, &areas.areas, stop)?,
            None => {
                let input = dataset()?;
                trace_raster(
                    input,
                    &options,
                    &areas.areas,
                    &areas.r#box,
                    &scratch,
                    stop,
                    progress,
                )?
            }
        };
        let smoothed = options.smooth != 0;
        Ok(make_apply(move |context| {
            let entity_options = ContourEntityOptions {
                layer: layer.clone(),
                command_name: command,
            };
            let mut plan =
                contour_command(context.document.model(), &traced.contours, &entity_options)?;
            let mut created = Vec::new();
            if let Some(step) = plan.command.take() {
                context.document.execute(step)?;
                created = context.document.last_created_entities();
            }
            let mut reply = records;
            reply.push(contours_record(
                &traced.method,
                traced.cell,
                &plan,
                &layer,
                smoothed,
            ));
            reply.push(format!(
                "output arg=output kind=vector target=layer layer={} created={}",
                value(&layer),
                plan.created
            ));
            for warning in &traced.warnings {
                reply.push(warning_record(warning));
            }
            if let Some(frame) = &context.front_end.frame
                && !created.is_empty()
            {
                let mut framed = Box2::default();
                for &id in &created {
                    if let Some(entity) = context.document.model().entities.find(id) {
                        framed.expand_box(bounding_box(&entity.geometry));
                    }
                }
                frame(&framed);
            }
            Ok(joined_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("Contours"),
        reply: None,
        work: Some(work),
    })
}
