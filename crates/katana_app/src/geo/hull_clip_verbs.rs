//! GIS HULL and GIS CLIP (docs/geoprocessing.md, "V3"), the port of
//! src/katana_app/geo/hull_clip_verbs.cpp: the boundary around what the scope
//! takes - a survey's extent, the edge of the data - and what the scope takes
//! clipped to the areas of a second scope or a file.
//!
//! ```text
//! GIS HULL [<scope>] [convex | concave=<0..1>] [holes] [TO LAYER <path>] [PREVIEW]
//! GIS CLIP [<scope>] BY (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
//!          [TO LAYER <path> | REPLACE] [PREVIEW]
//! ```
//!
//! The convex hull is Katana's own (geometry's convex_hull): nothing GDAL
//! adds. The concave one is GDAL's `vector concave-hull` of every point and
//! vertex in the scope as one multipoint - GEOS's ratio, 1 the convex hull and
//! towards 0 the tightest. The clip is `vector clip --like`, which clips to
//! the like dataset's geometries (measured: not merely their bounds), and
//! takes a file's layer and WHERE clause itself.
//!
//! A clip draws the pieces on a layer, or with REPLACE cuts the entities in
//! place: each keeps its id on its largest piece and the rest are made beside
//! it (vector_support's reshape, REPAIR's too); one wholly outside the
//! boundary is deleted; one wholly inside is left alone.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use katana_commands::{CommandPtr, Transaction, delete_entities};
use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_entity::layer_path::validate_layer_path;
use katana_geometry::Point2;
use katana_geometry::polygon::convex_hull;
use katana_gis::processing::{
    self as gp, ArgValue, DatasetPath, DatasetValue, Feature, FeatureSet, FeatureTable, OutputTo,
    RunRequest,
};
use katana_gis::{GeoPoint, GeometryKind, VectorGeometry};
use katana_interop::geo::ResultOptions;
use katana_interop::geo::drawing_dataset::result_command;

use super::bindings::{Tokens, project_crs, scope_record, unchanged_since};
use super::geo_verbs::{Context, Prepared, Progress, make_apply, make_work, trimmed_text};
use super::replies::{fixed3, value, warning_record};
use super::vector_support::{
    Applied, Reshaped, WordRules, answered, bind_scope, copies_of, execute_step, feature_id,
    features_by_id, gis_record, joined, layer_target, measure, merged_table, not_run_record,
    number_option, output_record, read_verb_words, reshape_command, run_vector, same_shape,
    split_records, table_named, without_empty,
};

const HULL_USAGE: &str =
    "GIS HULL [<scope>] [convex | concave=<0..1>] [holes] [TO LAYER <path>] [PREVIEW]";
const CLIP_USAGE: &str = concat!(
    "GIS CLIP [<scope>] BY (<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"]) ",
    "[TO LAYER <path> | REPLACE] [PREVIEW]",
);

fn seconds_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64()
}

fn check_layer(layer: &str) -> Status {
    if let Err(valid) = validate_layer_path(layer) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("TO LAYER needs a layer path: {}", valid.message),
            layer,
        ));
    }
    Ok(())
}

/// Every point and vertex of a feature set, once each.
fn vertices_of(set: &FeatureSet) -> Vec<GeoPoint> {
    let mut points: Vec<GeoPoint> = Vec::new();
    for table in &set.tables {
        for feature in &table.features {
            for part in &feature.parts {
                for ring in &part.parts {
                    points.extend_from_slice(ring);
                }
            }
        }
    }
    // The C++'s comparison, x then y by `<`: a -0 and a 0 are the same place,
    // as the repeats are found by `==`.
    points.sort_by(|a, b| {
        let order = if a.x != b.x {
            a.x.partial_cmp(&b.x)
        } else {
            a.y.partial_cmp(&b.y)
        };
        order.unwrap_or(Ordering::Equal)
    });
    points.dedup_by(|a, b| a.x == b.x && a.y == b.y);
    points
}

// ---- GIS HULL --------------------------------------------------------------------------------

/// The hull of `points`: Katana's convex hull, or GDAL's concave one of them
/// all as one multipoint.
fn hull_of(
    points: &[GeoPoint],
    concave: Option<f64>,
    holes: bool,
    crs: &str,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
    warnings: &mut Vec<String>,
) -> Result<FeatureTable> {
    let mut table = FeatureTable {
        name: String::from("hull"),
        kind: GeometryKind::Polygon,
        crs_wkt: String::from(crs),
        ..FeatureTable::default()
    };
    let Some(ratio) = concave else {
        let plan: Vec<Point2> = points
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect();
        let ring = convex_hull(plan);
        if ring.len() >= 3 {
            let polygon = VectorGeometry {
                kind: GeometryKind::Polygon,
                parts: vec![
                    ring.iter()
                        .map(|corner| GeoPoint {
                            x: corner.x,
                            y: corner.y,
                            z: 0.0,
                        })
                        .collect(),
                ],
                ..VectorGeometry::default()
            };
            table.features.push(Feature {
                parts: vec![polygon],
                values: Vec::new(),
            });
        }
        return Ok(table);
    };
    // All the points as ONE multipoint: the hull of them together, not one
    // per entity.
    let all = Feature {
        parts: points
            .iter()
            .map(|point| VectorGeometry {
                kind: GeometryKind::Point,
                parts: vec![vec![GeoPoint {
                    x: point.x,
                    y: point.y,
                    z: 0.0,
                }]],
                ..VectorGeometry::default()
            })
            .collect(),
        values: Vec::new(),
    };
    let input = FeatureSet {
        tables: vec![FeatureTable {
            name: String::from("points"),
            kind: GeometryKind::Point,
            crs_wkt: String::from(crs),
            features: vec![all],
            ..FeatureTable::default()
        }],
    };
    let mut gdal_words = vec![format!("--ratio={}", format_exact_real(ratio))];
    if holes {
        gdal_words.push(String::from("--allow-holes"));
    }
    let out = run_vector(
        &["vector", "concave-hull"].map(String::from),
        &input,
        &gdal_words,
        stop,
        progress,
        warnings,
        &[],
    )?;
    for made in &out.tables {
        for feature in &made.features {
            let copy = Feature {
                parts: feature
                    .parts
                    .iter()
                    .filter(|part| part.kind == GeometryKind::Polygon)
                    .cloned()
                    .collect(),
                values: Vec::new(),
            };
            if !copy.parts.is_empty() {
                table.features.push(copy);
            }
        }
    }
    Ok(table)
}

fn prepare_hull(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let rules = WordRules {
        options: vec![String::from("concave")],
        flags: Vec::from(["CONVEX", "HOLES", "PREVIEW"].map(String::from)),
        target: true,
        usage: String::from(HULL_USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(tokens, 2, tokens.size(), &rules)?;
    let ratio = number_option(&words, "concave")?;
    if ratio.is_some() && words.has("CONVEX") {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "convex or concave=<ratio>, not both",
        ));
    }
    if let Some(ratio) = ratio
        && !(0.0..=1.0).contains(&ratio)
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "concave= is a ratio from 0 (the tightest) to 1 (the convex hull)",
            format!(
                "concave={}",
                words
                    .option("concave")
                    .map(String::as_str)
                    .unwrap_or_default()
            ),
        ));
    }
    if words.has("HOLES") && ratio.is_none() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a convex hull has no holes: HOLES goes with concave=<ratio>",
            "HOLES",
        ));
    }
    let layer = layer_target(&words, "GIS HULL", "gis/hull")?;
    check_layer(&layer)?;
    let bound = bind_scope(context, &words.scope)?;
    let mut records = vec![scope_record("input", &bound)];
    for warning in &bound.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    let points = vertices_of(&bound.dataset.set);
    let concave = ratio;
    let holes = words.has("HOLES");
    let settings = format!(
        "hull kind={} ratio={} holes={} points={}",
        if concave.is_some() {
            "concave"
        } else {
            "convex"
        },
        concave.map(format_exact_real).unwrap_or_default(),
        if holes { "yes" } else { "no" },
        points.len()
    );
    if points.is_empty() {
        records.insert(0, not_run_record("hull", ""));
        records.push(format!("{settings} area=0.000"));
        return Ok(answered("GIS HULL", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=hull preview=yes"));
        records.push(settings);
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS HULL", joined(&records)));
    }
    let target = layer;
    let crs = project_crs(context);
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let table = hull_of(&points, concave, holes, &crs, stop, progress, &mut warnings)?;
        if table.features.is_empty() {
            warnings.push(String::from(
                "the points in the scope are fewer than three or all in a line: they bound no area",
            ));
        }
        let hull = FeatureSet {
            tables: vec![table],
        };
        let measures = measure(&hull);
        let summary = format!("{settings} area={}", fixed3(measures.area));
        let seconds = seconds_since(start);
        let operation = if concave.is_some() {
            "vector concave-hull"
        } else {
            "convex hull"
        };
        Ok(make_apply(move |context| {
            let options = ResultOptions {
                target_layer: target.clone(),
                operation: String::from(operation),
                command_name,
                ..ResultOptions::default()
            };
            let mut made = result_command(context.document.model(), &hull, &options)?;
            let applied = Applied {
                created: made.created,
                skipped: made.skipped,
                ..Applied::default()
            };
            execute_step(context, made.command.take())?;
            let mut reply = vec![gis_record("hull", seconds)];
            reply.extend(records);
            reply.push(output_record(&target, &applied));
            reply.push(summary);
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS HULL"),
        reply: None,
        work: Some(work),
    })
}

// ---- GIS CLIP --------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct Boundary {
    /// The second scope's areas.
    drawn: Option<FeatureSet>,
    /// FILE: its path, layer and where= clause.
    path: String,
    layer: String,
    r#where: String,
}

/// The pieces of each clipped feature, gathered by the entity it came from:
/// vector clip gives a piece a feature of its own.
fn pieces_by_id(clipped: &FeatureSet) -> BTreeMap<EntityId, Feature> {
    let mut pieces: BTreeMap<EntityId, Feature> = BTreeMap::new();
    for table in &clipped.tables {
        for feature in &table.features {
            if let Some(id) = feature_id(table, feature, "katana_id") {
                pieces
                    .entry(id)
                    .or_default()
                    .parts
                    .extend(feature.parts.iter().cloned());
            }
        }
    }
    pieces
}

fn prepare_clip(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let by = (2..tokens.size())
        .find(|&i| tokens.is(i, "BY"))
        .unwrap_or(tokens.size());
    if by == tokens.size() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("GIS CLIP needs BY and the boundary: {CLIP_USAGE}"),
        ));
    }
    let rules = WordRules {
        options: vec![String::from("where")],
        flags: Vec::from(["REPLACE", "PREVIEW"].map(String::from)),
        target: true,
        usage: String::from(CLIP_USAGE),
        ..WordRules::default()
    };
    let subject_words = read_verb_words(tokens, 2, by, &rules)?;
    let mut from = by + 1;
    let mut boundary = Boundary::default();
    if tokens.is(from, "FILE") {
        if from + 1 >= tokens.size() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "FILE needs a path",
                "FILE",
            ));
        }
        boundary.path = tokens[from + 1].clone();
        from += 2;
        if tokens.is(from, "LAYER") {
            if from + 1 >= tokens.size() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "LAYER needs the file's layer name",
                    "LAYER",
                ));
            }
            boundary.layer = tokens[from + 1].clone();
            from += 2;
        }
    }
    let by_words = read_verb_words(tokens, from, tokens.size(), &rules)?;
    if !boundary.path.is_empty() && by_words.scope_given {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "BY takes a scope or FILE <path>, not both",
        ));
    }
    if boundary.path.is_empty() && !by_words.scope_given {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("BY needs the boundary: a scope or FILE <path>: {CLIP_USAGE}"),
        ));
    }
    let mut words = subject_words;
    for (key, text) in &by_words.options {
        if words.options.insert(key.clone(), text.clone()).is_some() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{key}= is given twice"),
                key.clone(),
            ));
        }
    }
    for flag in &by_words.flags {
        if !words.flags.insert(flag.clone()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{flag} is given twice"),
                flag.clone(),
            ));
        }
    }
    if let Some(target) = &by_words.target {
        if words.target.is_some() {
            return Err(Error::new(ErrorCode::InvalidArgument, "one TO per line"));
        }
        words.target = Some(target.clone());
    }
    if let Some(filter) = words.option("where") {
        if boundary.path.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "where= filters a FILE boundary; a drawn one takes WHERE conditions",
                format!("where={filter}"),
            ));
        }
        boundary.r#where = filter.clone();
    }
    let replace = words.has("REPLACE");
    if replace && words.target.is_some() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "REPLACE cuts the entities in place; TO LAYER draws the pieces beside them - one or \
             the other",
        ));
    }
    let layer = layer_target(&words, "GIS CLIP", "gis/clip")?;
    check_layer(&layer)?;

    let subject = bind_scope(context, &words.scope)?;
    let mut records = vec![scope_record("input", &subject)];
    for warning in &subject.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    let mut took_nothing = subject.dataset.set.tables.is_empty();
    if boundary.path.is_empty() {
        let drawn = bind_scope(context, &by_words.scope)?;
        records.push(scope_record("like", &drawn));
        let mut areas = FeatureSet::default();
        if let Some(polygons) = table_named(&drawn.dataset.set, "polygons") {
            areas.tables.push(polygons.clone());
        }
        took_nothing = took_nothing || areas.tables.is_empty();
        boundary.drawn = Some(areas);
    } else {
        let mut record = format!("input arg=like source=file file={}", value(&boundary.path));
        if !boundary.layer.is_empty() {
            record += &format!(" layer={}", value(&boundary.layer));
        }
        if !boundary.r#where.is_empty() {
            record += &format!(" where={}", value(&boundary.r#where));
        }
        records.push(record);
    }
    let features: usize = subject
        .dataset
        .set
        .tables
        .iter()
        .map(|table| table.features.len())
        .sum();
    let settings = format!(
        "clip mode={} features={features}",
        if replace { "replace" } else { "layer" }
    );
    if took_nothing {
        records.insert(0, not_run_record("clip", ""));
        records.push(settings);
        return Ok(answered("GIS CLIP", joined(&records)));
    }
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=clip preview=yes"));
        records.push(settings);
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS CLIP", joined(&records)));
    }

    // An area wholly outside goes with the holes it took in, so they are
    // compared at apply too: deleting a hole moved while the job ran would
    // lose the move.
    let holes = if replace {
        subject.dataset.holes.clone()
    } else {
        BTreeMap::new()
    };
    let input = subject.dataset.set;
    let mut ids = Vec::new();
    for id in features_by_id(&input).into_keys() {
        ids.push(id);
        if let Some(found) = holes.get(&id) {
            ids.extend(found.iter().copied());
        }
    }
    let copies = if replace {
        copies_of(context.document, &ids)
    } else {
        Vec::new()
    };
    let target = layer;
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let mut request = RunRequest {
            path: ["vector", "clip"].map(String::from).to_vec(),
            ..RunRequest::default()
        };
        request.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Features(input.clone())),
        ));
        if let Some(drawn) = &boundary.drawn {
            request.values.push((
                String::from("like"),
                ArgValue::Dataset(DatasetValue::Features(drawn.clone())),
            ));
        } else {
            let file = DatasetPath {
                path: boundary.path.clone(),
                ..DatasetPath::default()
            };
            request.values.push((
                String::from("like"),
                ArgValue::Dataset(DatasetValue::Path(file)),
            ));
            if !boundary.layer.is_empty() {
                request
                    .tokens
                    .push(format!("--like-layer={}", boundary.layer));
            }
            if !boundary.r#where.is_empty() {
                request
                    .tokens
                    .push(format!("--like-where={}", boundary.r#where));
            }
        }
        request.output_to = OutputTo::Memory;
        let outputs = gp::run(&request, stop, progress)?;
        for diagnostic in &outputs.diagnostics {
            if !diagnostic.failure {
                warnings.push(diagnostic.message.clone());
            }
        }
        let clipped = without_empty(outputs.features.unwrap_or_default());
        let measures = measure(&clipped);
        // What became of each entity: whole, cut, or gone.
        let before = features_by_id(&input);
        let pieces = pieces_by_id(&clipped);
        let mut whole = 0;
        let mut cut: Vec<Reshaped> = Vec::new();
        let mut gone: Vec<EntityId> = Vec::new();
        let mut outside = 0;
        for (&id, &was) in &before {
            match pieces.get(&id) {
                None => {
                    outside += 1;
                    gone.push(id);
                    if let Some(joined_holes) = holes.get(&id) {
                        gone.extend(joined_holes.iter().copied());
                    }
                }
                Some(found) if same_shape(was, found) => whole += 1,
                Some(found) => cut.push(Reshaped {
                    id,
                    parts: found.parts.clone(),
                    before: was.clone(),
                }),
            }
        }
        let summary = format!(
            "{settings} whole={whole} cut={} outside={outside} area={} length={}",
            cut.len(),
            fixed3(measures.area),
            fixed3(measures.length)
        );
        let seconds = seconds_since(start);
        Ok(make_apply(move |context| {
            let mut reply = vec![gis_record("clip", seconds)];
            reply.extend(records);
            let mut applied = Applied::default();
            if replace {
                // Cutting what changed since would overwrite a concurrent
                // edit.
                unchanged_since(context, &copies)?;
                let mut reshape =
                    reshape_command(context.document.model(), &cut, "vector clip", &command_name)?;
                let mut step = Transaction::new(command_name.clone());
                if let Some(command) = reshape.command.take() {
                    step.add(command);
                }
                applied.deleted = gone.len();
                if !gone.is_empty() {
                    step.add(delete_entities(gone));
                }
                let command: Option<CommandPtr> = if step.size() == 0 {
                    None
                } else {
                    Some(Box::new(step))
                };
                let created = execute_step(context, command)?;
                reply.extend(split_records(&reshape.splits, &created));
                applied.created = reshape.created;
                applied.updated = reshape.updated;
                applied.skipped = reshape.left;
                reply.push(output_record("", &applied));
                for warning in &reshape.warnings {
                    reply.push(warning_record(warning));
                }
            } else {
                let options = ResultOptions {
                    target_layer: target.clone(),
                    operation: String::from("vector clip"),
                    command_name: command_name.clone(),
                    ..ResultOptions::default()
                };
                let tables: Vec<&FeatureTable> = clipped.tables.iter().collect();
                let mut one = FeatureSet::default();
                if !tables.is_empty() {
                    one.tables.push(merged_table(&tables, String::from("clip")));
                }
                let mut made = result_command(context.document.model(), &one, &options)?;
                applied.created = made.created;
                applied.skipped = made.skipped;
                execute_step(context, made.command.take())?;
                reply.push(output_record(&target, &applied));
            }
            reply.push(summary);
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS CLIP"),
        reply: None,
        work: Some(work),
    })
}

/// GIS HULL, its row in the verb table's V3 block (verb_table.rs).
pub fn prepare_gis_hull(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_hull(context, tokens, line)
}

/// GIS CLIP, its row in the verb table's V3 block.
pub fn prepare_gis_clip(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_clip(context, tokens, line)
}
