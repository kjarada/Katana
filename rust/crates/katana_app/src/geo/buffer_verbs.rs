//! GIS BUFFER and GIS DISSOLVE (docs/geoprocessing.md, "V1"), the port of
//! src/katana_app/geo/buffer_verbs.cpp: easements, setbacks, corridors and
//! clearance zones around drawn features, and areas merged by what their
//! properties say - through GDAL's `vector buffer`, `vector combine` and
//! `vector dissolve`, on the shared scope and filter.
//!
//! ```text
//! GIS BUFFER [<scope>] distance=<m>|distance=prop:<key> [side=both|left|right]
//!            [caps=round|flat|square] [joins=round|mitre|bevel] [dissolve[=k1,k2]]
//!            [TO LAYER <path>] [PREVIEW]
//! GIS DISSOLVE [<scope>] [by=k1,k2] [keep=identical] [TO LAYER <path>] [REPLACE] [PREVIEW]
//! ```
//!
//! What was measured, and so is relied on here:
//!
//! ```text
//! - `vector dissolve` alone unions only the parts WITHIN each feature; merging
//!   by a property takes `vector combine --group-by` first (and its output is
//!   a geometry collection, which the bridge reads as parts);
//! - `vector buffer` drops Z: a result is in plan;
//! - GEOS buffers a line or a point inwards to nothing (an empty polygon);
//! - GDAL's default of 8 segments a quadrant strays 0.096 m from a 5 m arc,
//!   so the quadrant's segments come from the curve tolerance by the sagitta
//!   rule every chording in Katana follows (geometry's chording);
//! - a round buffer of a point is a circle, so it is drawn as one - exactly -
//!   rather than as its chords.
//! ```

use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use katana_commands::{CommandPtr, Transaction, create_entities, create_layer, delete_entities};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::layer_path::validate_layer_path;
use katana_entity::{Entity, EntityId, Geometry, Layer, PropertyMap, PropertyValue};
use katana_geometry::chording::sagitta_chord_count;
use katana_geometry::{Circle2, Point2};
use katana_gis::GeometryKind;
use katana_gis::processing::{FeatureSet, FeatureTable, FieldValue};
use katana_interop::geo::ResultOptions;
use katana_interop::geo::drawing_dataset::{is_bookkeeping_field, result_command};
use katana_math::numerics::{HALF_PI, PI};

use super::bindings::{Tokens, scope_record, unchanged_since};
use super::geo_verbs::{Context, Prepared, Progress, make_apply, make_work, trimmed_text};
use super::replies::{fixed3, value, warning_record};
use super::vector_support::{
    Applied, Measures, VerbWords, WordRules, answered, bind_scope, choice_option, copies_of,
    execute_step, feature_id, field_index, gis_record, has_field, joined, layer_target, list_of,
    measure, merged_table, not_run_record, output_record, progress_span, property_of,
    read_verb_words, run_vector, table_named, without_empty,
};

const BUFFER_USAGE: &str = concat!(
    "GIS BUFFER [<scope>] distance=<m>|distance=prop:<key> [side=both|left|right] ",
    "[caps=round|flat|square] [joins=round|mitre|bevel] [dissolve[=k1,k2]] [TO LAYER <path>] ",
    "[PREVIEW]",
);
const DISSOLVE_USAGE: &str =
    "GIS DISSOLVE [<scope>] [by=k1,k2] [keep=identical] [TO LAYER <path>] [REPLACE] [PREVIEW]";

/// The chord tolerance a buffer's arcs are drawn to: drawing_dataset's own
/// default, the 1 mm sagitta EXPORT writes curves with.
const CURVE_TOLERANCE: f64 = 0.001;

fn seconds_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64()
}

/// A quadrant's segments for a buffer of `distance`: the fewest whose chords
/// stay within the curve tolerance of the true arc.
fn quadrant_segments(distance: f64) -> usize {
    sagitta_chord_count(distance.abs(), HALF_PI, CURVE_TOLERANCE)
}

fn number(value: f64) -> String {
    format_exact_real(value)
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

/// The distance a feature's property gives, when it gives one.
fn distance_of(value: &FieldValue) -> Option<f64> {
    match value {
        FieldValue::Real(real) => real.is_finite().then_some(*real),
        FieldValue::Integer64(whole) => Some(*whole as f64),
        FieldValue::Text(text) => parse_finite_double(trimmed(text.as_bytes())),
        FieldValue::Null | FieldValue::Boolean(_) => None,
    }
}

/// A round buffer of a point, drawn as the circle it is.
#[derive(Clone, Debug, Default, PartialEq)]
struct CircleResult {
    centre: Point2,
    radius: f64,
    properties: PropertyMap,
}

#[derive(Clone, Debug, PartialEq)]
struct BufferPlan {
    distance: Option<f64>,
    /// distance=prop:<key>
    property: String,
    side: String,
    caps: String,
    joins: String,
    dissolve: bool,
    dissolve_by: Vec<String>,
    layer: String,
}

impl Default for BufferPlan {
    fn default() -> Self {
        Self {
            distance: None,
            property: String::new(),
            side: String::from("both"),
            caps: String::from("round"),
            joins: String::from("round"),
            dissolve: false,
            dissolve_by: Vec::new(),
            layer: String::from("gis/buffer"),
        }
    }
}

/// The C++'s `std::map<double, FeatureSet>`: the groups by their distance,
/// ascending. A distance is finite and never 0 (distance= refuses 0, and a
/// property's 0 is skipped), so `<` orders them as the map's key does.
type Groups = Vec<(f64, FeatureSet)>;

/// The group of `distance`, made empty where there is none (the map's
/// operator[]).
fn group_for(groups: &mut Groups, distance: f64) -> &mut FeatureSet {
    if let Some(at) = groups.iter().position(|(key, _)| *key == distance) {
        return &mut groups[at].1;
    }
    let at = groups
        .iter()
        .position(|(key, _)| distance < *key)
        .unwrap_or(groups.len());
    groups.insert(at, (distance, FeatureSet::default()));
    &mut groups[at].1
}

/// The words of combine and dissolve that merge `set` by `keys` - all of it
/// into one when there are none - as `vector combine` then `vector dissolve`
/// (dissolve alone unions only within a feature). One table named `name`.
fn dissolved(
    set: &FeatureSet,
    keys: &[String],
    keep_identical: bool,
    name: &str,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
    warnings: &mut Vec<String>,
) -> Result<FeatureSet> {
    let mut combine: Vec<String> = keys.iter().map(|key| format!("--group-by={key}")).collect();
    if keep_identical {
        combine.push(String::from("--add-extra-fields=always-identical"));
    }
    let combined = run_vector(
        &["vector", "combine"].map(String::from),
        set,
        &combine,
        stop,
        progress_span(progress, 0.0, 0.5).as_deref(),
        warnings,
        &[],
    )?;
    let merged = run_vector(
        &["vector", "dissolve"].map(String::from),
        &combined,
        &[],
        stop,
        progress_span(progress, 0.5, 1.0).as_deref(),
        warnings,
        &[],
    )?;
    let out = without_empty(merged);
    let tables: Vec<&FeatureTable> = out.tables.iter().collect();
    let mut one = FeatureSet::default();
    if !tables.is_empty() {
        one.tables.push(merged_table(&tables, String::from(name)));
    }
    Ok(one)
}

// ---- GIS BUFFER --------------------------------------------------------------------------------

fn read_buffer(words: &VerbWords) -> Result<BufferPlan> {
    let mut plan = BufferPlan::default();
    let Some(distance) = words.option("distance") else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("GIS BUFFER needs distance=<m> or distance=prop:<key>: {BUFFER_USAGE}"),
        ));
    };
    if distance.to_ascii_lowercase().starts_with("prop:") {
        // "prop:" is five ASCII bytes, so the rest begins on a character.
        plan.property = String::from(trimmed_text(&distance[5..]));
        if plan.property.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "distance=prop: names the property that holds each distance",
                format!("distance={distance}"),
            ));
        }
    } else {
        match parse_finite_double(distance) {
            Some(value) if value != 0.0 => plan.distance = Some(value),
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "distance= is a length other than 0 (negative shrinks an area), or prop:<key>",
                    format!("distance={distance}"),
                ));
            }
        }
    }
    let side = choice_option(words, "side", &["both", "left", "right"], "both");
    let caps = choice_option(words, "caps", &["round", "flat", "square"], "round");
    let joins = choice_option(
        words,
        "joins",
        &["round", "mitre", "miter", "bevel"],
        "round",
    );
    // Refused in the C++'s order: side, then caps, then joins.
    plan.side = side?;
    plan.caps = caps?;
    let joins = joins?;
    plan.joins = if joins == "miter" {
        String::from("mitre")
    } else {
        joins
    };
    let dissolve_by = words.option("dissolve");
    if let Some(keys) = dissolve_by
        && words.has("DISSOLVE")
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "DISSOLVE or dissolve=<keys>, not both",
            format!("dissolve={keys}"),
        ));
    }
    plan.dissolve = words.has("DISSOLVE") || dissolve_by.is_some();
    if let Some(keys) = dissolve_by {
        plan.dissolve_by = list_of(keys);
        if plan.dissolve_by.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "dissolve= names the properties to merge by; DISSOLVE alone merges everything",
                "dissolve=",
            ));
        }
    }
    plan.layer = layer_target(words, "GIS BUFFER", "gis/buffer")?;
    check_layer(&plan.layer)?;
    Ok(plan)
}

/// The features of `set` by the distance each is buffered by: one group for a
/// distance given, one per value of the property otherwise. `skipped` counts
/// the features whose property gives no distance other than 0.
fn by_distance(set: &FeatureSet, plan: &BufferPlan, skipped: &mut usize) -> Groups {
    let mut groups = Groups::new();
    if let Some(distance) = plan.distance {
        groups.push((distance, set.clone()));
        return groups;
    }
    for table in &set.tables {
        let field = field_index(table, &plan.property);
        for feature in &table.features {
            let distance = field
                .and_then(|at| feature.values.get(at))
                .and_then(distance_of);
            let Some(distance) = distance.filter(|distance| *distance != 0.0) else {
                *skipped += 1;
                continue;
            };
            let group = group_for(&mut groups, distance);
            // The last table of the name, as the C++ finds it.
            let into = match group
                .tables
                .iter()
                .rposition(|existing| existing.name == table.name)
            {
                Some(at) => at,
                None => {
                    group.tables.push(FeatureTable {
                        name: table.name.clone(),
                        kind: table.kind,
                        has_z: table.has_z,
                        crs_wkt: table.crs_wkt.clone(),
                        fields: table.fields.clone(),
                        features: Vec::new(),
                    });
                    group.tables.len() - 1
                }
            };
            group.tables[into].features.push(feature.clone());
        }
    }
    groups
}

fn buffer_record(
    plan: &BufferPlan,
    groups: usize,
    result: &Measures,
    circles: usize,
    circle_area: f64,
    empty: usize,
    skipped: usize,
) -> String {
    let mut record = format!(
        "buffer distance={}",
        match plan.distance {
            Some(distance) => number(distance),
            None => format!("prop:{}", value(&plan.property)),
        }
    );
    record += &format!(
        " side={} caps={} joins={}",
        plan.side, plan.caps, plan.joins
    );
    if let Some(distance) = plan.distance {
        record += &format!(" quadrant_segments={}", quadrant_segments(distance));
    }
    let mut dissolve = String::from("no");
    if plan.dissolve {
        dissolve = String::from("all");
        if !plan.dissolve_by.is_empty() {
            dissolve = super::replies::joined(&plan.dissolve_by, ",");
        }
    }
    record += &format!(
        " dissolve={} groups={groups} features={} circles={circles} area={} empty={empty}",
        value(&dissolve),
        result.features + circles,
        fixed3(result.area + circle_area)
    );
    if plan.distance.is_none() {
        record += &format!(" skipped.no_distance={skipped}");
    }
    record
}

/// A round buffer of each point of `table` - one at least as far out as it
/// is - as a circle; `empty` counts the points buffered inwards, which are
/// nothing.
fn circles_of(
    table: &FeatureTable,
    distance: f64,
    circles: &mut Vec<CircleResult>,
    empty: &mut usize,
) {
    for feature in &table.features {
        let first = feature
            .parts
            .first()
            .and_then(|part| part.parts.first())
            .and_then(|ring| ring.first());
        let Some(at) = first else {
            *empty += 1;
            continue;
        };
        if distance < 0.0 {
            *empty += 1; // a point buffered inwards is nothing
            continue;
        }
        let mut properties = PropertyMap::new();
        for (field, value) in table.fields.iter().zip(&feature.values) {
            if is_bookkeeping_field(&field.name) {
                continue;
            }
            if let Some(property) = property_of(value) {
                properties.insert(field.name.clone(), property);
            }
        }
        properties.insert(
            String::from("gis.op"),
            PropertyValue::Text(String::from("vector buffer")),
        );
        if let Some(id) = feature_id(table, feature, "katana_id") {
            properties.insert(
                String::from("gis.source"),
                PropertyValue::Integer(id as i64),
            );
        }
        circles.push(CircleResult {
            centre: Point2::new(at.x, at.y),
            radius: distance,
            properties,
        });
    }
}

fn prepare_buffer(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let rules = WordRules {
        options: Vec::from(["distance", "side", "caps", "joins", "dissolve"].map(String::from)),
        flags: Vec::from(["DISSOLVE", "PREVIEW"].map(String::from)),
        target: true,
        usage: String::from(BUFFER_USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(tokens, 2, tokens.size(), &rules)?;
    let plan = read_buffer(&words)?;
    let bound = bind_scope(context, &words.scope)?;
    let mut records = vec![scope_record("input", &bound)];
    for warning in &bound.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    let set = &bound.dataset.set;
    if set.tables.is_empty() {
        records.insert(0, not_run_record("buffer", ""));
        return Ok(answered("GIS BUFFER", joined(&records)));
    }
    if !plan.property.is_empty() && !has_field(set, &plan.property) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no entity in the scope has the property distance=prop: names",
            plan.property.clone(),
        ));
    }
    for key in &plan.dissolve_by {
        if !has_field(set, key) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "no entity in the scope has the property dissolve= names",
                key.clone(),
            ));
        }
    }
    if words.has("PREVIEW") {
        let mut skipped = 0;
        let groups = by_distance(set, &plan, &mut skipped);
        records.insert(0, String::from("gis op=buffer preview=yes"));
        records.push(format!(
            "buffer groups={}{}",
            groups.len(),
            if plan.distance.is_some() {
                String::new()
            } else {
                format!(" skipped.no_distance={skipped}")
            }
        ));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS BUFFER", joined(&records)));
    }

    let input = bound.dataset.set;
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let mut skipped = 0;
        let mut groups = by_distance(&input, &plan, &mut skipped);
        // A round buffer of a point is a circle: drawn as one, exactly, when
        // nothing is to be merged with it.
        let mut circles: Vec<CircleResult> = Vec::new();
        let mut empty = 0;
        let points_as_circles = plan.caps == "round" && !plan.dissolve;
        let mut buffered: Vec<FeatureTable> = Vec::new();
        let count = groups.len();
        for (done, (distance, group)) in groups.iter_mut().enumerate() {
            let distance = *distance;
            if points_as_circles {
                let mut kept = Vec::new();
                for table in std::mem::take(&mut group.tables) {
                    if table.kind == GeometryKind::Point {
                        circles_of(&table, distance, &mut circles, &mut empty);
                    } else {
                        kept.push(table);
                    }
                }
                group.tables = kept;
            }
            if !group.tables.is_empty() {
                let gdal_words = [
                    format!("--distance={}", number(distance)),
                    format!("--endcap-style={}", plan.caps),
                    format!("--join-style={}", plan.joins),
                    format!("--side={}", plan.side),
                    format!("--quadrant-segments={}", quadrant_segments(distance)),
                ];
                let from = done as f64 / count as f64;
                let to = (done + 1) as f64 / count as f64;
                let out = run_vector(
                    &["vector", "buffer"].map(String::from),
                    group,
                    &gdal_words,
                    stop,
                    progress_span(
                        progress,
                        if plan.dissolve { from * 0.5 } else { from },
                        if plan.dissolve { to * 0.5 } else { to },
                    )
                    .as_deref(),
                    &mut warnings,
                    &[],
                )?;
                buffered.extend(out.tables);
            }
        }
        let tables: Vec<&FeatureTable> = buffered.iter().collect();
        let mut features = FeatureSet::default();
        if !tables.is_empty() {
            features
                .tables
                .push(merged_table(&tables, String::from("buffer")));
        }
        empty += measure(&features).empty;
        features = without_empty(features);
        if plan.dissolve && !features.tables.is_empty() {
            features = dissolved(
                &features,
                &plan.dissolve_by,
                false,
                "buffer",
                stop,
                progress_span(progress, 0.5, 1.0).as_deref(),
                &mut warnings,
            )?;
        }
        let measures = measure(&features);
        let mut circle_area = 0.0;
        for circle in &circles {
            circle_area += PI * circle.radius * circle.radius;
        }
        let summary = buffer_record(
            &plan,
            count,
            &measures,
            circles.len(),
            circle_area,
            empty,
            skipped,
        );
        let seconds = seconds_since(start);
        Ok(make_apply(move |context| {
            let options = ResultOptions {
                target_layer: plan.layer.clone(),
                operation: String::from("vector buffer"),
                command_name: command_name.clone(),
                ..ResultOptions::default()
            };
            let mut made = result_command(context.document.model(), &features, &options)?;
            let mut step = Transaction::new(command_name);
            let layer_made = made.layers.contains(&plan.layer);
            if let Some(command) = made.command.take() {
                step.add(command);
            }
            if !circles.is_empty() {
                if !layer_made && !context.document.model().layers.contains(&plan.layer) {
                    step.add(create_layer(Layer {
                        name: plan.layer.clone(),
                        ..Layer::default()
                    }));
                }
                let entities: Vec<Entity> = circles
                    .iter()
                    .map(|circle| Entity {
                        geometry: Geometry::Circle(Circle2::new(circle.centre, circle.radius)),
                        layer: plan.layer.clone(),
                        properties: circle.properties.clone(),
                        ..Entity::default()
                    })
                    .collect();
                step.add(create_entities(entities));
            }
            let applied = Applied {
                created: made.created + circles.len(),
                skipped: made.skipped,
                ..Applied::default()
            };
            let command: Option<CommandPtr> = if step.size() == 0 {
                None
            } else {
                Some(Box::new(step))
            };
            execute_step(context, command)?;
            let mut reply = vec![gis_record("buffer", seconds)];
            reply.extend(records);
            reply.push(output_record(&plan.layer, &applied));
            reply.push(summary);
            for warning in &made.warnings {
                reply.push(warning_record(warning));
            }
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS BUFFER"),
        reply: None,
        work: Some(work),
    })
}

// ---- GIS DISSOLVE ------------------------------------------------------------------------------

fn prepare_dissolve(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let rules = WordRules {
        options: Vec::from(["by", "keep"].map(String::from)),
        flags: Vec::from(["REPLACE", "PREVIEW"].map(String::from)),
        target: true,
        usage: String::from(DISSOLVE_USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(tokens, 2, tokens.size(), &rules)?;
    let keys = words
        .option("by")
        .map(String::as_str)
        .map(list_of)
        .unwrap_or_default();
    if words.option("by").is_some() && keys.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "by= names the properties to merge by; without it everything merges",
            "by=",
        ));
    }
    let keep = choice_option(&words, "keep", &["identical"], "")?;
    let keep_identical = keep == "identical";
    let layer = layer_target(&words, "GIS DISSOLVE", "gis/dissolve")?;
    check_layer(&layer)?;
    let replace = words.has("REPLACE");
    let bound = bind_scope(context, &words.scope)?;
    let mut records = vec![scope_record("input", &bound)];
    for warning in &bound.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    let areas = table_named(&bound.dataset.set, "polygons");
    let others = bound.dataset.stats.points + bound.dataset.stats.lines;
    if others != 0 {
        records.push(warning_record(&format!(
            "{others} points and lines in the scope are not areas; a dissolve merges areas and \
             leaves them as they are"
        )));
    }
    let Some(areas) = areas else {
        records.insert(0, not_run_record("dissolve", "areas=0"));
        return Ok(answered("GIS DISSOLVE", joined(&records)));
    };
    for key in &keys {
        if field_index(areas, key).is_none() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "no area in the scope has the property by= names",
                key.clone(),
            ));
        }
    }
    // REPLACE deletes what went in: every area, and every hole joined into
    // one - read from the areas table itself, so what is deleted is what the
    // one conversion made an area (a closed polyline or curve polyline, a
    // circle, a whole ellipse, a closed spline), never a second rule of it.
    let mut went_in: BTreeSet<EntityId> = BTreeSet::new();
    for feature in &areas.features {
        if let Some(id) = feature_id(areas, feature, "katana_id") {
            went_in.insert(id);
            if let Some(holes) = bound.dataset.holes.get(&id) {
                went_in.extend(holes.iter().copied());
            }
        }
    }
    let sources: Vec<EntityId> = bound
        .r#match
        .matched
        .iter()
        .copied()
        .filter(|id| went_in.contains(id))
        .collect();
    let by_text = super::replies::joined(&keys, ",");
    let settings = format!(
        "by={} keep={} areas={} replace={}",
        value(&by_text),
        if keep_identical { "identical" } else { "none" },
        areas.features.len(),
        if replace { "yes" } else { "no" }
    );
    if words.has("PREVIEW") {
        records.insert(0, String::from("gis op=dissolve preview=yes"));
        records.push(format!("dissolve {settings}"));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS DISSOLVE", joined(&records)));
    }

    let input = FeatureSet {
        tables: vec![areas.clone()],
    };
    let copies = if replace {
        copies_of(context.document, &sources)
    } else {
        Vec::new()
    };
    let command_name = String::from(trimmed_text(line));
    let target = layer;
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let merged = dissolved(
            &input,
            &keys,
            keep_identical,
            "dissolve",
            stop,
            progress,
            &mut warnings,
        )?;
        let measures = measure(&merged);
        let summary = format!(
            "dissolve {settings} groups={} area={}",
            measures.features,
            fixed3(measures.area)
        );
        let seconds = seconds_since(start);
        Ok(make_apply(move |context| {
            if replace {
                // Deleting what changed since would lose a concurrent edit.
                unchanged_since(context, &copies)?;
            }
            let options = ResultOptions {
                target_layer: target.clone(),
                operation: String::from("vector dissolve"),
                command_name: command_name.clone(),
                ..ResultOptions::default()
            };
            let mut made = result_command(context.document.model(), &merged, &options)?;
            let mut applied = Applied {
                created: made.created,
                skipped: made.skipped,
                ..Applied::default()
            };
            let mut step = Transaction::new(command_name);
            if let Some(command) = made.command.take() {
                step.add(command);
            }
            if replace && !sources.is_empty() {
                applied.deleted = sources.len();
                step.add(delete_entities(sources));
            }
            let command: Option<CommandPtr> = if step.size() == 0 {
                None
            } else {
                Some(Box::new(step))
            };
            execute_step(context, command)?;
            let mut reply = vec![gis_record("dissolve", seconds)];
            reply.extend(records);
            reply.push(output_record(&target, &applied));
            reply.push(summary);
            for warning in &made.warnings {
                reply.push(warning_record(warning));
            }
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS DISSOLVE"),
        reply: None,
        work: Some(work),
    })
}

/// GIS BUFFER, its row in the verb table's V1 block (verb_table.rs).
pub fn prepare_gis_buffer(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_buffer(context, tokens, line)
}

/// GIS DISSOLVE, its row in the verb table's V1 block.
pub fn prepare_gis_dissolve(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_dissolve(context, tokens, line)
}
