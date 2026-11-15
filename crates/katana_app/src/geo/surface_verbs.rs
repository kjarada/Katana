//! The SURFACE verbs (docs/terrain.md "Surfaces on every front end"), the
//! port of src/katana_app/geo/surface_verbs.cpp but its helpers shared with
//! the other terrain verbs, which are terrain_verbs.rs's: the drawing's named
//! surfaces listed, described, removed, made from a raster, a point cloud or
//! the drawing, and written out as a DEM - the same on katana_cli, katana_mcp
//! and the window's command line, where the window's Surface From and Export
//! Surface as DEM items build these lines. The other SURFACE words - RENAME,
//! STYLE, SHOW, HIDE, COPY, the edits, the volumes' - are the interpreter's
//! (cad's terrain::surface_verbs); the verb table's SURFACE row takes only
//! these ([`takes_surface`]).
//!
//! ```text
//! SURFACE LIST [JSON]
//! SURFACE INFO <name>
//! SURFACE REMOVE <name>
//! SURFACE FROM RASTER <id|name> | FILE <path> [NAME <n>] [max=<points>] [AREA x0,y0,x1,y1]
//! SURFACE FROM CLOUD <id|name> [NAME <n>] [max=<points>] [classes=2,...]
//! SURFACE FROM <scope> [NAME <n>]
//! SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] [format=<driver>]
//!                [co=K=V]... [OVERWRITE]
//! ```
//!
//! FROM reads its source with the one source parser (bindings.rs), so the
//! drawing is taken by the one scope grammar: SURFACE FROM DRAWING WHERE DRAWN
//! is what the drawing shows, SURFACE FROM LAYERS ground ONLY one layer. As
//! the design had it, DRAWING may also come before a narrower scope - FROM
//! DRAWING LAYERS ground - where it names the source and the scope narrows it.
//! A CLOUD is read here: a point cloud is no dataset GDAL binds.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use katana_cad::SurfaceStyleSet;
use katana_cad::annotation::leader_edit::{ReadKind, leaders_reading};
use katana_cad::geo::surface_input::surface_input;
use katana_cad::scope_verbs::{
    ScopeSource, is_scope_word, match_scope, parse_scope_words, scope_record,
};
use katana_cad::terrain::surface_style::style_of;
use katana_cad::terrain::surface_verbs::surface_record;
use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_integer};
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Box2;
use katana_gis::gdal_adapter::describe_crs;
use katana_interop::reference_data::to_string;
use katana_interop::terrain_io::{
    SurfaceRasterOptions, export_surface_raster, suggested_cell_size, surface_points_of_classes,
};
use katana_interop::{PointCloudLayer, RasterOverlay};
use katana_math::cpp;
use katana_terrain::{
    CrossingBreaklinePolicy, DuplicatePointPolicy, NamedSurface, SurfaceStore, TinBuildOptions,
    TinInput, TinSurface,
};

use super::bindings::{Source, SourceKind, Tokens, input_record, parse_source, project_crs};
use super::geo_verbs::{Context, Prepared, make_apply, make_work};
use super::replies::{fixed3, value, warning_record, yes_no};
use super::terrain_verbs::{
    SURFACE_POINT_CAP, SurfaceBuild, answered_with, joined_records, keep_surface, key_value,
    positive_option, surface_from_raster_file, triangulate,
};
use crate::import_records::{path_from_text, path_text};

const USAGE: &str = "SURFACE LIST [JSON] | SURFACE INFO <name> | SURFACE REMOVE <name> | SURFACE \
                     FROM RASTER <id|name> | FILE <path> | CLOUD <id|name> | <scope> [NAME <n>] \
                     [max=<points>] [AREA x0,y0,x1,y1] [classes=2,...] | SURFACE EXPORT <name> \
                     <file> [cell=<m>] [type=Float32|Float64] [cog] [co=K=V]... [OVERWRITE] (and \
                     the drawing's: SURFACE RENAME|STYLE|SHOW|HIDE|COPY, HELP SURFACESTYLE)";

/// 1000 cells along the longer side: the C++'s default for a surface's
/// suggested cell (terrain_io.hpp, suggestedCellSize).
const SUGGESTED_CELLS: i32 = 1000;

fn refusal(why: impl Into<String>, word: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, why, word)
}

/// A point cloud of the reference data by id or name, as RASTER finds a
/// raster.
fn find_cloud<'r>(clouds: &'r [PointCloudLayer], word: &str) -> Option<&'r PointCloudLayer> {
    clouds
        .iter()
        .find(|cloud| format!("{}", cloud.id) == word)
        .or_else(|| {
            clouds
                .iter()
                .find(|cloud| equals_ignoring_case(cloud.name.as_bytes(), word.as_bytes()))
        })
}

fn find_raster<'r>(rasters: &'r [RasterOverlay], word: &str) -> Option<&'r RasterOverlay> {
    rasters
        .iter()
        .find(|raster| format!("{}", raster.id) == word)
        .or_else(|| {
            rasters
                .iter()
                .find(|raster| equals_ignoring_case(raster.name.as_bytes(), word.as_bytes()))
        })
}

fn box_text(area: &Box2) -> String {
    format!(
        "{},{},{},{}",
        fixed3(area.min.x),
        fixed3(area.min.y),
        fixed3(area.max.x),
        fixed3(area.max.y)
    )
}

/// The cell of a raster when it is square and unrotated; nothing otherwise.
fn square_cell(raster: &RasterOverlay) -> Option<f64> {
    let gt = &raster.geotransform;
    if !raster.has_geotransform || gt[2] != 0.0 || gt[4] != 0.0 || gt[1].abs() != gt[5].abs() {
        return None;
    }
    Some(gt[1].abs())
}

fn raster_record(raster: &RasterOverlay) -> String {
    format!(
        "raster id={} name={} kind={} width={} height={} cell={} file={}",
        raster.id,
        value(&raster.name),
        to_string(raster.role),
        raster.width,
        raster.height,
        square_cell(raster)
            .map(format_exact_real)
            .unwrap_or_default(),
        value(&path_text(&raster.source))
    )
}

/// The triangulation a drawing's surface always has.
fn tin_of(named: &NamedSurface) -> &Arc<TinSurface> {
    named
        .surface
        .as_ref()
        .expect("a drawing's surface has a triangulation")
}

fn surface_json(named: &NamedSurface, styles: &SurfaceStyleSet) -> Json {
    let surface = tin_of(named);
    let area = surface.bounds();
    Json::object_of([
        ("name", Json::from(&named.name)),
        ("triangles", Json::from(surface.triangle_count())),
        ("points", Json::from(surface.vertex_count())),
        (
            "bounds",
            Json::array_of(
                [area.min.x, area.min.y, area.max.x, area.max.y]
                    .into_iter()
                    .map(Json::from),
            ),
        ),
        ("zmin", Json::from(surface.min_elevation())),
        ("zmax", Json::from(surface.max_elevation())),
        ("plan_area", Json::from(surface.plan_area())),
        ("source", Json::from(&named.source)),
        // The style it is drawn in (the default's name when it has none of
        // its own), and whether the views draw it.
        ("style", Json::from(&style_of(styles, named).name)),
        ("visible", Json::from(named.visible)),
    ])
}

fn raster_json(raster: &RasterOverlay) -> Json {
    Json::object_of([
        ("id", Json::from(raster.id)),
        ("name", Json::from(&raster.name)),
        ("role", Json::from(to_string(raster.role))),
        ("width", Json::from(raster.width)),
        ("height", Json::from(raster.height)),
        ("cell", square_cell(raster).map_or(Json::Null, Json::from)),
        (
            "crs",
            if raster.projection_wkt.is_empty() {
                Json::Null
            } else {
                Json::from(describe_crs(&raster.projection_wkt))
            },
        ),
        (
            "nodata",
            raster.facts.no_data.map_or(Json::Null, Json::from),
        ),
        ("source", Json::from(path_text(&raster.source))),
        ("derived_from", Json::from(&raster.derivation)),
    ])
}

// ---- LIST, INFO, REMOVE ----------------------------------------------------------------------

fn list(context: &Context<'_>, tokens: &Tokens, mut at: usize) -> Result<Prepared> {
    let mut json = false;
    while at < tokens.size() {
        if !tokens.is(at, "JSON") {
            return Err(refusal("SURFACE LIST takes JSON or nothing", &tokens[at]));
        }
        json = true;
        at += 1;
    }
    if json {
        // What katana_terrain_list hands an agent: the surfaces, and the
        // rasters a terrain verb can read as RASTER <id|name>. A String holds
        // no byte that is not UTF-8, so nlohmann's `replace` handler, which
        // the C++ names here, has nothing to replace.
        let surfaces: Vec<Json> = context
            .surfaces
            .all()
            .iter()
            .map(|named| surface_json(named, context.document.surface_styles()))
            .collect();
        let rasters: Vec<Json> = context
            .reference
            .rasters()
            .iter()
            .map(raster_json)
            .collect();
        return Ok(answered_with(
            "SURFACE LIST",
            Json::object_of([
                ("surfaces", Json::array_of(surfaces)),
                ("rasters", Json::array_of(rasters)),
            ])
            .dump(),
        ));
    }
    let mut records = Vec::new();
    for named in context.surfaces.all() {
        records.push(surface_record(named, context.document.surface_styles()));
    }
    for raster in context.reference.rasters() {
        records.push(raster_record(raster));
    }
    records.push(format!(
        "listed surfaces={} rasters={}",
        context.surfaces.all().len(),
        context.reference.rasters().len()
    ));
    Ok(answered_with("SURFACE LIST", joined_records(&records)))
}

fn info(context: &Context<'_>, tokens: &Tokens, at: usize) -> Result<Prepared> {
    if at + 1 != tokens.size() {
        return Err(refusal(
            "SURFACE INFO names one surface: SURFACE INFO <name>",
            "",
        ));
    }
    let Some(named) = context.surfaces.find(&tokens[at]) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no surface has that name; SURFACE LIST names them",
            tokens[at].clone(),
        ));
    };
    let surface = tin_of(named);
    Ok(answered_with(
        "SURFACE INFO",
        format!(
            "{}\narea plan={} surface={}",
            surface_record(named, context.document.surface_styles()),
            fixed3(surface.plan_area()),
            fixed3(surface.surface_area())
        ),
    ))
}

fn remove(context: &mut Context<'_>, tokens: &Tokens, at: usize) -> Result<Prepared> {
    if at + 1 != tokens.size() {
        return Err(refusal(
            "SURFACE REMOVE names one surface: SURFACE REMOVE <name>",
            "",
        ));
    }
    let Some(named) = context.surfaces.find(&tokens[at]) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no surface has that name; SURFACE LIST names them",
            tokens[at].clone(),
        ));
    };
    let name = named.name.clone();
    // The leaders whose notes read it by name will read nothing: their lines
    // are dropped, and one whose every line read it is drawn bare. Said, not
    // refused - the surface is the person's to remove, and UNDO puts it back
    // with its readers; the alternative, refusing while a leader reads it,
    // would make a surface that was meant to go stay (docs/annotation.md).
    let readers = leaders_reading(context.document.model(), ReadKind::Surface, &name);
    // Removed as the line runs - there is nothing to wait for - from the
    // phase's staging copy, which the executor commits as one undo step
    // ("SURFACE REMOVE"): UNDO puts the surface back.
    context.surfaces.remove(&name);
    let mut reply = format!(
        "removed surface={} surfaces={}",
        value(&name),
        context.surfaces.all().len()
    );
    if readers > 0 {
        reply.push('\n');
        reply += &warning_record(&format!(
            "leaders={readers} read surface {name} and will read nothing now"
        ));
    }
    Ok(answered_with("SURFACE REMOVE", reply))
}

// ---- FROM ---------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct FromWords {
    name: Option<String>,
    max_points: Option<usize>,
    area: Option<Box2>,
    classes: Vec<u8>,
    classes_given: bool,
    preview: bool,
}

fn class_list(text: &str) -> Result<Vec<u8>> {
    let mut classes = Vec::new();
    for item in text.split(',') {
        match parse_integer(item) {
            Some(number) if (0..=255).contains(&number) => classes.push(number as u8),
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "classes is a list of ASPRS class numbers from 0 to 255: classes=2,8",
                    item,
                ));
            }
        }
    }
    Ok(classes)
}

/// The words after the source, any order: NAME <n>, max=, classes=, AREA and
/// PREVIEW. AREA is read by the one scope parser, so it is written as a
/// scope's AREA is; for a raster it is a window, not a scope.
fn from_words(tokens: &Tokens, mut at: usize) -> Result<FromWords> {
    let mut words = FromWords::default();
    while at < tokens.size() {
        if tokens.is(at, "NAME") {
            if at + 1 >= tokens.size() || tokens[at + 1].is_empty() {
                return Err(refusal("NAME needs the surface's name", ""));
            }
            // Refused now, by the store's own rule, and not after the surface
            // has been built: a name that holds a quote a line cannot carry
            // would otherwise fail at the end of the work.
            SurfaceStore::check_name(tokens[at + 1].as_bytes())?;
            words.name = Some(tokens[at + 1].clone());
            at += 2;
        } else if tokens.is(at, "PREVIEW") {
            words.preview = true;
            at += 1;
        } else if tokens.is(at, "AREA") {
            let scope = parse_scope_words(&tokens.words, &mut at)?;
            if scope.source != ScopeSource::Area || !scope.filter.empty() {
                return Err(refusal("a raster is cut by AREA x0,y0,x1,y1 alone", ""));
            }
            words.area = Some(scope.area);
        } else if let Some((key, text)) = key_value(tokens, at) {
            if key == "max" {
                match parse_integer(&text) {
                    Some(number) if number >= 3 => words.max_points = Some(number as usize),
                    _ => {
                        return Err(refusal("max is how many points at most, 3 or more", &text));
                    }
                }
            } else if key == "classes" {
                words.classes = class_list(&text)?;
                words.classes_given = true;
            } else {
                return Err(refusal(
                    "SURFACE FROM takes NAME, max=, classes=, AREA and PREVIEW",
                    &tokens[at],
                ));
            }
            at += 1;
        } else {
            return Err(refusal(
                "SURFACE FROM takes NAME, max=, classes=, AREA and PREVIEW",
                &tokens[at],
            ));
        }
    }
    Ok(words)
}

/// What a surface made at apply is called: the name asked for, which must be
/// free, or the source's own made unique.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Naming {
    name: String,
    unique: bool,
}

fn naming_for(context: &Context<'_>, words: &FromWords, fallback: String) -> Result<Naming> {
    if let Some(name) = &words.name {
        if context.surfaces.find(name).is_some() {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "a surface already has that name; SURFACE REMOVE it first, or name this one \
                 otherwise",
                name.clone(),
            ));
        }
        return Ok(Naming {
            name: name.clone(),
            unique: false,
        });
    }
    Ok(Naming {
        name: if fallback.is_empty() {
            String::from("surface")
        } else {
            fallback
        },
        unique: true,
    })
}

/// The job that triangulates what `make` reads and the apply that keeps it.
/// (The C++ hands `make` the job's stop, which none of its makers reads.)
fn build_job(
    title: &str,
    records: Vec<String>,
    naming: Naming,
    source: String,
    make: impl FnOnce() -> Result<SurfaceBuild> + Send + 'static,
) -> Prepared {
    let work = make_work(move |_stop, _progress| {
        let built = make()?;
        Ok(make_apply(move |context| {
            let reply = keep_surface(context, naming.name, source, naming.unique, built)?;
            let mut all = records;
            all.push(reply);
            Ok(joined_records(&all))
        }))
    });
    Prepared {
        title: String::from(title),
        reply: None,
        work: Some(work),
    }
}

/// A path's file name, or its stem, as the C++'s `.string()` gives it.
fn name_of(path: &Path, stem: bool) -> String {
    let part = if stem {
        path.file_stem()
    } else {
        path.file_name()
    };
    part.map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn from_raster(context: &Context<'_>, source: &Source, words: &FromWords) -> Result<Prepared> {
    if words.classes_given {
        return Err(refusal(
            "classes= is a point cloud's; a raster has no classes",
            "",
        ));
    }
    let (path, name): (PathBuf, String) = if source.kind == SourceKind::Raster {
        let Some(raster) = find_raster(context.reference.rasters(), &source.raster) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no reference raster has that id or name; SURFACE LIST lists them",
                source.raster.clone(),
            ));
        };
        if raster.source.as_os_str().is_empty() {
            // A raster fetched from a web service and held only as pixels:
            // its heights were never kept.
            return Err(make_error(
                ErrorCode::Unsupported,
                "the reference raster has no file to read its true values from",
                source.raster.clone(),
            ));
        }
        (raster.source.clone(), raster.name.clone())
    } else {
        let path = path_from_text(&source.path);
        let name = name_of(&path, true);
        (path, name)
    };
    let naming = naming_for(context, words, name)?;
    let mut records = vec![input_record("input", source)];
    if let Some(area) = &words.area {
        records.push(format!("window area={}", box_text(area)));
    }
    if words.preview {
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered_with("SURFACE FROM", joined_records(&records)));
    }
    let cap = words.max_points.unwrap_or(SURFACE_POINT_CAP);
    let area = words.area;
    let source_text = format!("raster {}", name_of(&path, false));
    // Read on the job's thread, from the file: a path is all the job takes,
    // so the reference raster may go away meanwhile.
    Ok(build_job(
        "Surface From Raster",
        records,
        naming,
        source_text,
        move || surface_from_raster_file(&path, cap, area),
    ))
}

/// Every step-th point, the step the smallest that brings them under the cap;
/// rounded up, so the result is at the cap or under it, never just over.
fn thinning_for(available: usize, cap: usize) -> usize {
    if available <= cap {
        1
    } else {
        available.div_ceil(cap)
    }
}

fn from_cloud(context: &Context<'_>, word: &str, words: &FromWords) -> Result<Prepared> {
    if words.area.is_some() {
        return Err(refusal(
            "AREA cuts a raster; a point cloud is imported for the area it needs",
            "",
        ));
    }
    let Some(cloud) = find_cloud(context.reference.point_clouds(), word) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no point cloud has that id or name; REFS lists the reference layers",
            word,
        ));
    };
    let naming = naming_for(context, words, cloud.name.clone())?;
    // Copied here, on the calling thread, because the cloud belongs to the
    // reference data and could be removed while the job runs.
    let points = surface_points_of_classes(cloud, &words.classes);
    let mut classes = String::new();
    for each in &words.classes {
        if !classes.is_empty() {
            classes.push(',');
        }
        classes += &format!("{each}");
    }
    let classes = if words.classes_given {
        classes
    } else if points.ground_only {
        String::from("2")
    } else {
        String::from("all")
    };
    let mut records = vec![format!(
        "input arg=input source=cloud name={} id={} points={} ground={} excluded={} classes={classes}",
        value(&cloud.name),
        cloud.id,
        points.points.len(),
        yes_no(points.ground_only),
        points.excluded
    )];
    if !words.classes_given && !points.ground_only {
        // Said, because a canopy surface presented as ground is the silent
        // failure audit QT-10 found.
        records.push(warning_record(
            "the cloud has no return classified as ground (ASPRS class 2), so every return is \
             triangulated: vegetation and buildings are part of the surface",
        ));
    }
    let cap = words.max_points.unwrap_or(SURFACE_POINT_CAP);
    let step = thinning_for(points.points.len(), cap);
    if step > 1 {
        records.push(format!(
            "thinned from={} step={step} points={}",
            points.points.len(),
            points.points.len().div_ceil(step)
        ));
    }
    if points.points.len().div_ceil(step) < 3 {
        // What the job would refuse after it started, said before it does -
        // and by PREVIEW, which must not call valid a line whose run fails.
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "fewer than three points with a height to triangulate: {}",
                joined_records(&records)
            ),
        ));
    }
    if words.preview {
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered_with("SURFACE FROM", joined_records(&records)));
    }
    let source = format!("point cloud {}", cloud.name);
    let points = points.points;
    Ok(build_job(
        "Surface From Point Cloud",
        records,
        naming,
        source,
        move || {
            let mut kept = Vec::with_capacity(points.len() / step + 1);
            kept.extend(points.iter().step_by(step).copied());
            let input = TinInput {
                points: kept,
                ..TinInput::default()
            };
            // Scanned points land on one ground mark again and again, their
            // heights millimetres apart: the mean is taken and the merges
            // counted.
            let options = TinBuildOptions {
                duplicate_points: DuplicatePointPolicy::Average,
                ..TinBuildOptions::default()
            };
            triangulate(input, &options)
        },
    ))
}

fn from_drawing(context: &Context<'_>, source: &Source, words: &FromWords) -> Result<Prepared> {
    if words.area.is_some() || words.max_points.is_some() || words.classes_given {
        return Err(refusal(
            "the drawing's surface is narrowed by its scope and filter; max=, classes= and a \
             window are a raster's and a cloud's",
            "",
        ));
    }
    let found = match_scope(
        context.document,
        &source.scope,
        context.interpreter.scope_context(),
    )?;
    // Read here, on the calling thread: the document is single-threaded, and
    // from here the job owns what it triangulates.
    let input = surface_input(context.document.model(), &found.matched);
    let mut record = format!(
        "scope arg=input {} used={} points={} breaklines={} vertices.heightless={}",
        scope_record(&found),
        input.used,
        input.input.points.len(),
        input.input.breaklines.len(),
        input.heightless_vertices
    );
    for (reason, count) in &input.skipped {
        record += &format!(" skipped.{reason}={count}");
    }
    let mut records = vec![String::from("input arg=input source=drawing"), record];
    if input.heightless_vertices != 0 {
        records.push(warning_record(&format!(
            "{} vertices have no height ('elevation' or 'elevations' property) and were left out",
            input.heightless_vertices
        )));
    }
    let naming = naming_for(context, words, String::from("drawing"))?;
    if input.input.points.len() < 3 {
        // Refused here, with what the scope took, rather than after a job:
        // there is nothing to triangulate. Before PREVIEW answers, which says
        // a line is valid only if running it would not be refused for this.
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "fewer than three points with a height to triangulate: {}",
                joined_records(&records)
            ),
        ));
    }
    if words.preview {
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered_with("SURFACE FROM", joined_records(&records)));
    }
    let source_text = format!("drawing ({} entities)", input.used);
    let surface_points = input.input;
    Ok(build_job(
        "Surface From Drawing",
        records,
        naming,
        source_text,
        move || {
            let options = TinBuildOptions {
                duplicate_points: DuplicatePointPolicy::Average,
                crossing_breaklines: CrossingBreaklinePolicy::Average,
                ..TinBuildOptions::default()
            };
            triangulate(surface_points, &options)
        },
    ))
}

fn from(context: &Context<'_>, tokens: &Tokens, mut at: usize) -> Result<Prepared> {
    if at >= tokens.size() {
        return Err(refusal(
            "SURFACE FROM needs a source: RASTER <id|name>, FILE <path>, CLOUD <id|name> or a \
             scope of the drawing (DRAWING, SELECTION, LAYERS ...)",
            "",
        ));
    }
    if tokens.is(at, "CLOUD") {
        if at + 1 >= tokens.size() {
            return Err(refusal("CLOUD needs the point cloud's id or name", ""));
        }
        let words = from_words(tokens, at + 2)?;
        return from_cloud(context, &tokens[at + 1], &words);
    }
    // DRAWING before a narrower scope names the source; the scope narrows it.
    if tokens.is(at, "DRAWING")
        && at + 1 < tokens.size()
        && !tokens.quoted[at + 1]
        && is_scope_word(&tokens[at + 1])
        && !tokens.is(at + 1, "WHERE")
    {
        at += 1;
    }
    if tokens.is(at, "SURFACE") {
        return Err(refusal(
            "a surface is made from a raster, a point cloud or the drawing",
            "",
        ));
    }
    let source = parse_source(tokens, &mut at)?;
    let words = from_words(tokens, at)?;
    if source.kind == SourceKind::Drawing {
        return from_drawing(context, &source, &words);
    }
    from_raster(context, &source, &words)
}

// ---- EXPORT -------------------------------------------------------------------------------------

fn export_dem(context: &Context<'_>, tokens: &Tokens, mut at: usize) -> Result<Prepared> {
    const TAKES: &str =
        "SURFACE EXPORT takes cell=, type=, format=, co=, COG, OVERWRITE and PREVIEW";
    if at + 2 > tokens.size() {
        return Err(refusal(
            "SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] [co=K=V]... \
             [OVERWRITE]",
            "",
        ));
    }
    let Some(named) = context.surfaces.find(&tokens[at]) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no surface has that name; SURFACE LIST names them",
            tokens[at].clone(),
        ));
    };
    let path = path_from_text(&tokens[at + 1]);
    let surface = Arc::clone(tin_of(named));
    let mut options = SurfaceRasterOptions {
        cell_size: suggested_cell_size(surface.bounds(), SUGGESTED_CELLS),
        projection_wkt: project_crs(context),
        ..SurfaceRasterOptions::default()
    };
    let mut preview = false;
    at += 2;
    while at < tokens.size() {
        if tokens.is(at, "OVERWRITE") {
            options.overwrite = true;
        } else if tokens.is(at, "COG") {
            options.cog = true;
        } else if tokens.is(at, "PREVIEW") {
            preview = true;
        } else if let Some((key, text)) = key_value(tokens, at) {
            if key == "cell" {
                options.cell_size = positive_option("cell", &text)?;
            } else if key == "type" {
                let lowered = text.to_ascii_lowercase();
                if lowered != "float32" && lowered != "float64" {
                    return Err(refusal("type is Float32 or Float64", &text));
                }
                options.data_type = String::from(if lowered == "float32" {
                    "Float32"
                } else {
                    "Float64"
                });
            } else if key == "format" {
                options.driver = text;
            } else if key == "co" {
                if !text.contains('=') {
                    return Err(refusal("co= takes a creation option: co=KEY=VALUE", &text));
                }
                options.creation_options.push(text);
            } else {
                return Err(refusal(TAKES, &tokens[at]));
            }
        } else {
            return Err(refusal(TAKES, &tokens[at]));
        }
        at += 1;
    }
    // Refused now rather than after a job: a file in the way is the line's
    // to allow, and every front end asks the same way (TO FILE's rule).
    if !options.overwrite && path.exists() {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "the file exists; add OVERWRITE to replace it",
            path_text(&path),
        ));
    }
    let bounds = surface.bounds();
    let columns = cpp::max(1.0, (bounds.width() / options.cell_size).ceil());
    let rows = cpp::max(1.0, (bounds.height() / options.cell_size).ceil());
    let grid = format!("{}x{}", format_exact_real(columns), format_exact_real(rows));
    if preview {
        return Ok(answered_with(
            "SURFACE EXPORT",
            format!(
                "export surface={} file={} raster={grid} cell={} type={}\npreview valid=yes \
                 changed=no",
                value(&named.name),
                value(&path_text(&path)),
                format_exact_real(options.cell_size),
                options.data_type
            ),
        ));
    }
    let name = named.name.clone();
    let work = make_work(move |stop, _progress| {
        let written = export_surface_raster(&surface, &path, &options, stop)?;
        let crs = if options.projection_wkt.is_empty() {
            String::from("none")
        } else {
            describe_crs(&options.projection_wkt)
        };
        let record = format!(
            "exported file={} driver={} surface={} raster={}x{} cell={} type={} cells={} \
             nodata={} crs={}",
            value(&path_text(&path)),
            written.driver,
            value(&name),
            written.columns,
            written.rows,
            format_exact_real(options.cell_size),
            written.data_type,
            written.cells_with_data,
            format_exact_real(written.no_data_value),
            value(&crs)
        );
        Ok(make_apply(move |_context| Ok(record)))
    });
    Ok(Prepared {
        title: String::from("Export Surface as DEM"),
        reply: None,
        work: Some(work),
    })
}

/// A SURFACE line the table's T0 row takes ([`takes_surface`]).
pub fn prepare_surface(
    context: &mut Context<'_>,
    tokens: &Tokens,
    _line: &str,
) -> Result<Prepared> {
    let at = 1;
    if at >= tokens.size() {
        return Err(refusal(format!("usage: {USAGE}"), ""));
    }
    if tokens.is(at, "LIST") {
        return list(context, tokens, at + 1);
    }
    if tokens.is(at, "INFO") {
        return info(context, tokens, at + 1);
    }
    if tokens.is(at, "REMOVE") {
        return remove(context, tokens, at + 1);
    }
    if tokens.is(at, "FROM") {
        return from(context, tokens, at + 1);
    }
    if tokens.is(at, "EXPORT") {
        return export_dem(context, tokens, at + 1);
    }
    Err(refusal(
        format!("SURFACE takes LIST, INFO, REMOVE, FROM or EXPORT: {USAGE}"),
        &tokens[at],
    ))
}

/// Only those five words (and SURFACE alone, for its usage): every other
/// SURFACE line - RENAME, STYLE, SHOW, HIDE, COPY, the edits, the volumes' -
/// is the interpreter's (cad's terrain::surface_verbs, docs/terrain.md).
pub fn takes_surface(tokens: &Tokens) -> bool {
    if tokens.size() < 2 {
        return true; // SURFACE alone: its usage, which names the other words too
    }
    ["LIST", "INFO", "REMOVE", "FROM", "EXPORT"]
        .iter()
        .any(|word| tokens.is(1, word))
}
