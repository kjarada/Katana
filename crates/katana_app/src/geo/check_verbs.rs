//! GIS CHECK, GIS REPAIR and GIS COVERAGE (docs/geoprocessing.md, "V4"), the
//! port of src/katana_app/geo/check_verbs.cpp: invalid geometry in imported
//! data, and the gaps and overlaps of a subdivision or cadastral fabric -
//! through GDAL's `vector check-geometry`, `make-valid`, `check-coverage` and
//! `clean-coverage`, on the shared scope.
//!
//! ```text
//! GIS CHECK [<scope>] [markers=<layer>] [PREVIEW]
//! GIS REPAIR [<scope>] [method=linework|structure] [PREVIEW]
//! GIS COVERAGE CHECK [<scope>] [gap=<m>] [markers=<layer>] [PREVIEW]
//! GIS COVERAGE CLEAN [<scope>] [gap=<m>] [snap=<m>]
//!                    [merge=longest-border|max-area|min-area|min-index] REPLACE [PREVIEW]
//! ```
//!
//! What was measured, and so is relied on here:
//!
//! ```text
//! - check-geometry gives one feature per invalid input with its location
//!   (a multipoint: a bow-tie's crossing) and GDAL's reason, and carries
//!   katana_id when asked (--include-field);
//! - check-coverage refuses mixed geometry and carries no fields, but with
//!   --include-valid gives one feature per input, in order - the invalid
//!   edges, or nothing - which is how a problem is joined to its entity;
//! - clean-coverage gives every input back, in order and with its fields,
//!   and closes only ENCLOSED gaps: a sliver open to the outside stays;
//! - both rewrite a ring's start and direction even where nothing moved, so
//!   "changed" is judged by the ring, not by the vertex list.
//! ```
//!
//! A check changes nothing but its markers. A repair and a clean change
//! entities in place: the geometry is replaced (the id kept, so labels and
//! associations survive), a result of several parts keeps the id on its
//! largest and makes the rest, and the entities are compared with the copies
//! taken at prepare before anything is written.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use katana_cad::scope_verbs::ScopeWords;
use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::layer_path::validate_layer_path;
use katana_entity::{Entity, EntityId, Geometry, PointGeometry, PropertyMap, PropertyValue};
use katana_geometry::{Containment, Point2, Polyline2};
use katana_gis::processing::{FeatureSet, FieldValue};
use katana_gis::{GeoPoint, GeometryKind};
use katana_math::cpp;
use katana_math::tolerance::GEOMETRIC;

use super::bindings::{BoundDrawing, Tokens, scope_record, unchanged_since};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, trimmed_text};
use super::replies::{fixed3, value, warning_record};
use super::vector_support::{
    Applied, Reshaped, VerbWords, WordRules, answered, bind_scope, choice_option, copies_of,
    execute_step, feature_id, features_by_id, field_index, gis_record, joined, not_run_record,
    number_option, output_record, read_verb_words, replace_markers, reshape_command, run_vector,
    same_shape, split_records, table_named,
};

const CHECK_USAGE: &str = "GIS CHECK [<scope>] [markers=<layer>] [PREVIEW]";
const REPAIR_USAGE: &str = "GIS REPAIR [<scope>] [method=linework|structure] [PREVIEW]";
const COVERAGE_USAGE: &str = concat!(
    "GIS COVERAGE CHECK [<scope>] [gap=<m>] [markers=<layer>] [PREVIEW] | GIS COVERAGE CLEAN ",
    "[<scope>] [gap=<m>] [snap=<m>] [merge=longest-border|max-area|min-area|min-index] REPLACE ",
    "[PREVIEW]",
);

fn seconds_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64()
}

fn number(value: f64) -> String {
    format_exact_real(value)
}

/// A place, to the millimetre as length= is, without trailing zeros: GEOS's
/// arithmetic gives the middle of an edge from y = 0 to 40 as
/// 20.000000000000007, which is 20 to any survey. (+ 0.0 makes a -0 of a
/// rounded -0.0004 read 0.)
fn at(point: Point2) -> String {
    #[allow(
        clippy::identity_op,
        reason = "+ 0.0 is no identity: it turns a -0 into 0, as the C++ does"
    )]
    let millimetres = |value: f64| number((value * 1000.0).round() / 1000.0 + 0.0);
    format!("{},{}", millimetres(point.x), millimetres(point.y))
}

/// A problem's words: GDAL's reason, lower case, blanks as hyphens.
fn kind_of(reason: &str) -> String {
    let kind = trimmed_text(reason).to_ascii_lowercase().replace(' ', "-");
    if kind.is_empty() {
        String::from("invalid")
    } else {
        kind
    }
}

fn check_markers_layer(layer: Option<&str>) -> Status {
    let Some(layer) = layer else {
        return Ok(());
    };
    if let Err(valid) = validate_layer_path(layer) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("markers= needs a layer path: {}", valid.message),
            layer,
        ));
    }
    Ok(())
}

/// A positive length option; None when not given.
#[allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "the C++'s !(length > 0) refuses a NaN as well as a length at or below 0"
)]
fn length_option(words: &VerbWords, key: &str) -> Result<Option<f64>> {
    let length = number_option(words, key)?;
    if let Some(length) = length
        && !(length > 0.0)
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key}= is a length above 0"),
            format!(
                "{key}={}",
                words.option(key).map(String::as_str).unwrap_or_default()
            ),
        ));
    }
    Ok(length)
}

/// The drawing's features the verb reads, what its scope said, and the
/// records that say so.
struct Scoped {
    bound: BoundDrawing,
    records: Vec<String>,
}

fn scoped(context: &Context<'_>, scope: &ScopeWords) -> Result<Scoped> {
    let bound = bind_scope(context, scope)?;
    let mut records = vec![scope_record("input", &bound)];
    for warning in &bound.dataset.stats.warnings {
        records.push(warning_record(warning));
    }
    Ok(Scoped { bound, records })
}

/// A problem found: what, which entity, where, and GDAL's reason.
#[derive(Clone, Debug, Default, PartialEq)]
struct Problem {
    kind: String,
    entity: Option<EntityId>,
    at: Point2,
    reason: String,
    length: Option<f64>,
    /// A coverage problem's edge, drawn as a marker.
    edge: Vec<Point2>,
}

fn problem_record(problem: &Problem) -> String {
    let mut record = format!(
        "problem kind={} entity={} at={}",
        value(&problem.kind),
        problem.entity.map(|id| format!("{id}")).unwrap_or_default(),
        at(problem.at)
    );
    if let Some(length) = problem.length {
        record += &format!(" length={}", fixed3(length));
    }
    record + " reason=" + &value(&problem.reason)
}

/// The markers of `problems`, on `layer`, replacing the ones a previous run
/// of `kind` left there, as one step; the output record.
fn apply_markers(
    context: &mut Context<'_>,
    layer: &str,
    kind: &str,
    problems: &[Problem],
    command_name: &str,
) -> Result<String> {
    let mut markers = Vec::new();
    for problem in problems {
        let geometry = if problem.edge.len() >= 2 {
            Geometry::Polyline(Polyline2 {
                vertices: problem.edge.clone(),
                closed: false,
            })
        } else {
            Geometry::Point(PointGeometry {
                position: problem.at,
            })
        };
        let mut properties = PropertyMap::new();
        properties.insert(
            String::from("gis.problem"),
            PropertyValue::Text(problem.kind.clone()),
        );
        properties.insert(
            String::from("gis.reason"),
            PropertyValue::Text(problem.reason.clone()),
        );
        if let Some(entity) = problem.entity {
            properties.insert(
                String::from("gis.source"),
                PropertyValue::Integer(entity as i64),
            );
        }
        markers.push(Entity {
            geometry,
            properties,
            ..Entity::default()
        });
    }
    let mut removed = 0;
    let mut applied = Applied {
        created: markers.len(),
        ..Applied::default()
    };
    let command = replace_markers(
        context.document.model(),
        layer,
        kind,
        markers,
        command_name,
        &mut removed,
    );
    applied.deleted = removed;
    execute_step(context, command)?;
    let record = output_record(layer, &applied);
    Ok(record.replacen("arg=output", "arg=markers", 1))
}

// ---- GIS CHECK -------------------------------------------------------------------------------

fn prepare_check(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let rules = WordRules {
        options: vec![String::from("markers")],
        flags: vec![String::from("PREVIEW")],
        target: false,
        usage: String::from(CHECK_USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(tokens, 2, tokens.size(), &rules)?;
    let markers = words.option("markers").cloned();
    check_markers_layer(markers.as_deref())?;
    let scope = scoped(context, &words.scope)?;
    // Points are always valid: what can be wrong is a line or an area.
    let mut input = FeatureSet::default();
    let mut features = 0;
    for table in &scope.bound.dataset.set.tables {
        if table.kind != GeometryKind::Point {
            input.tables.push(table.clone());
            features += table.features.len();
        }
    }
    if input.tables.is_empty() && markers.is_none() {
        let mut reply = vec![not_run_record("check", "")];
        reply.extend(scope.records);
        reply.push(String::from("check features=0 problems=0 entities=0"));
        return Ok(answered("GIS CHECK", joined(&reply)));
    }
    if words.has("PREVIEW") {
        let mut reply = vec![String::from("gis op=check preview=yes")];
        reply.extend(scope.records);
        reply.push(format!("check features={features}"));
        reply.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS CHECK", joined(&reply)));
    }
    let marker_layer = markers;
    let records = scope.records;
    let command_name = String::from(trimmed_text(line));
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let mut problems: Vec<Problem> = Vec::new();
        if !input.tables.is_empty() {
            let found = run_vector(
                &["vector", "check-geometry"].map(String::from),
                &input,
                &[String::from("--include-field=katana_id")],
                stop,
                progress,
                &mut warnings,
                &[],
            )?;
            for table in &found.tables {
                let reason = field_index(table, "error");
                for feature in &table.features {
                    let reason = match reason.and_then(|index| feature.values.get(index)) {
                        Some(FieldValue::Text(text)) => text.clone(),
                        _ => String::new(),
                    };
                    let problem = Problem {
                        kind: kind_of(&reason),
                        entity: feature_id(table, feature, "katana_id"),
                        reason,
                        ..Problem::default()
                    };
                    // One problem at each place GDAL found one.
                    let mut placed = false;
                    for part in &feature.parts {
                        for ring in &part.parts {
                            for point in ring {
                                problems.push(Problem {
                                    at: Point2::new(point.x, point.y),
                                    ..problem.clone()
                                });
                                placed = true;
                            }
                        }
                    }
                    if !placed {
                        problems.push(problem);
                    }
                }
            }
        }
        let entities: BTreeSet<EntityId> = problems
            .iter()
            .filter_map(|problem| problem.entity)
            .collect();
        let summary = format!(
            "check features={features} problems={} entities={}",
            problems.len(),
            entities.len()
        );
        let seconds = seconds_since(start);
        Ok(make_apply(move |context| {
            let mut reply = vec![gis_record("check", seconds)];
            reply.extend(records);
            for problem in &problems {
                reply.push(problem_record(problem));
            }
            reply.push(summary);
            if let Some(layer) = &marker_layer {
                reply.push(apply_markers(
                    context,
                    layer,
                    "check",
                    &problems,
                    &command_name,
                )?);
            }
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS CHECK"),
        reply: None,
        work: Some(work),
    })
}

// ---- reshaping: GIS REPAIR and GIS COVERAGE CLEAN ----------------------------------------------

/// The entities whose features an algorithm gave back changed, with what
/// they became; `unchanged` counts those it gave back as they were.
fn changed_by(before: &FeatureSet, after: &FeatureSet, unchanged: &mut usize) -> Vec<Reshaped> {
    let was = features_by_id(before);
    let mut changed = Vec::new();
    for table in &after.tables {
        for feature in &table.features {
            let Some(id) = feature_id(table, feature, "katana_id") else {
                continue;
            };
            let Some(&found) = was.get(&id) else {
                continue;
            };
            if same_shape(found, feature) {
                *unchanged += 1;
                continue;
            }
            changed.push(Reshaped {
                id,
                parts: feature.parts.clone(),
                before: found.clone(),
            });
        }
    }
    changed
}

/// The ids of the features of `set`: the entities an in-place apply may
/// change, copied at prepare to be compared at apply.
fn ids_of(set: &FeatureSet) -> Vec<EntityId> {
    features_by_id(set).into_keys().collect()
}

/// The apply of a reshape: compared, then ONE command; the records.
fn apply_reshape(
    context: &mut Context<'_>,
    copies: &[Entity],
    changed: &[Reshaped],
    op: &str,
    command_name: &str,
    records: &mut Vec<String>,
) -> Result<usize> {
    // Changing what changed since would overwrite a concurrent edit.
    unchanged_since(context, copies)?;
    let mut reshape = reshape_command(context.document.model(), changed, op, command_name)?;
    let created = execute_step(context, reshape.command.take())?;
    records.extend(split_records(&reshape.splits, &created));
    let applied = Applied {
        created: reshape.created,
        updated: reshape.updated,
        skipped: reshape.left,
        ..Applied::default()
    };
    records.push(output_record("", &applied));
    for warning in &reshape.warnings {
        records.push(warning_record(warning));
    }
    Ok(reshape.splits.len())
}

// ---- GIS REPAIR -------------------------------------------------------------------------------

fn prepare_repair(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let rules = WordRules {
        options: vec![String::from("method")],
        flags: vec![String::from("PREVIEW")],
        target: false,
        usage: String::from(REPAIR_USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(tokens, 2, tokens.size(), &rules)?;
    let method = choice_option(&words, "method", &["linework", "structure"], "linework")?;
    let scope = scoped(context, &words.scope)?;
    let mut input = FeatureSet::default();
    for table in &scope.bound.dataset.set.tables {
        if table.kind != GeometryKind::Point {
            input.tables.push(table.clone());
        }
    }
    let ids = ids_of(&input);
    let settings = format!("repair method={method} features={}", ids.len());
    if input.tables.is_empty() {
        let mut reply = vec![not_run_record("repair", "")];
        reply.extend(scope.records);
        reply.push(format!("{settings} valid=0 repaired=0 split=0"));
        return Ok(answered("GIS REPAIR", joined(&reply)));
    }
    if words.has("PREVIEW") {
        let mut reply = vec![String::from("gis op=repair preview=yes")];
        reply.extend(scope.records);
        reply.push(settings);
        reply.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS REPAIR", joined(&reply)));
    }
    let copies = copies_of(context.document, &ids);
    let records = scope.records;
    let command_name = String::from(trimmed_text(line));
    let how = method;
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let repaired = run_vector(
            &["vector", "make-valid"].map(String::from),
            &input,
            &[format!("--method={how}")],
            stop,
            progress,
            &mut warnings,
            &[],
        )?;
        let mut unchanged = 0;
        let changed = changed_by(&input, &repaired, &mut unchanged);
        let seconds = seconds_since(start);
        Ok(make_apply(move |context| {
            let mut reply = vec![gis_record("repair", seconds)];
            reply.extend(records);
            let splits = apply_reshape(
                context,
                &copies,
                &changed,
                "vector make-valid",
                &command_name,
                &mut reply,
            )?;
            reply.push(format!(
                "{settings} valid={unchanged} repaired={} split={splits}",
                changed.len()
            ));
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS REPAIR"),
        reply: None,
        work: Some(work),
    })
}

// ---- GIS COVERAGE ------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct Ring {
    exterior: Polyline2,
    holes: Vec<Polyline2>,
}

fn ring_of(points: &[GeoPoint]) -> Polyline2 {
    Polyline2 {
        vertices: points
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect(),
        closed: true,
    }
}

/// Strictly inside an area: inside its exterior and none of its holes, on
/// neither boundary.
fn strictly_inside(area: &Ring, point: Point2) -> bool {
    if area.exterior.classify(point) != Containment::Inside {
        return false;
    }
    area.holes
        .iter()
        .all(|hole| hole.classify(point) == Containment::Outside)
}

/// 1e300: the C++'s "no boundary at all", larger than any distance a
/// drawing's coordinates give.
const FAR: f64 = 1e300;

fn boundary_distance(area: &Ring, point: Point2) -> f64 {
    let mut best = area.exterior.distance_to(point).unwrap_or(FAR);
    for hole in &area.holes {
        best = cpp::min(best, hole.distance_to(point).unwrap_or(FAR));
    }
    best
}

/// What an invalid edge of area `own` is, from where its segments' middles
/// lie against the other areas: inside one (an overlap); outside all, within
/// `gap` of one (a gap GDAL was asked for); or on another's boundary without
/// sharing its vertices (edges that do not match).
fn coverage_kind(areas: &[Ring], own: usize, edge: &[Point2], gap: Option<f64>) -> &'static str {
    let mut near_gap = false;
    for pair in edge.windows(2) {
        let middle = (pair[0] + pair[1]) * 0.5;
        let mut inside = false;
        let mut covered = false;
        let mut nearest = FAR;
        for (a, area) in areas.iter().enumerate() {
            if a == own {
                continue;
            }
            inside = inside || strictly_inside(area, middle);
            covered = covered || area.exterior.classify(middle) != Containment::Outside;
            nearest = cpp::min(nearest, boundary_distance(area, middle));
        }
        if inside {
            return "overlap";
        }
        near_gap = near_gap
            || gap.is_some_and(|gap| !covered && nearest > GEOMETRIC && nearest <= gap + GEOMETRIC);
    }
    if near_gap { "gap" } else { "mismatch" }
}

fn coverage_reason(kind: &str) -> String {
    String::from(match kind {
        "overlap" => "the area overlaps a neighbour along this edge",
        "gap" => "a gap narrower than gap= lies along this edge",
        _ => "the edge meets a neighbour's without sharing its vertices",
    })
}

fn length_of(edge: &[Point2]) -> f64 {
    let mut length = 0.0;
    for pair in edge.windows(2) {
        length += pair[0].distance_to(pair[1]);
    }
    length
}

/// The point halfway along an edge.
fn middle_of(edge: &[Point2]) -> Point2 {
    let half = length_of(edge) / 2.0;
    let mut walked = 0.0;
    for pair in edge.windows(2) {
        let step = pair[0].distance_to(pair[1]);
        if walked + step >= half && step > 0.0 {
            return pair[0] + (pair[1] - pair[0]) * ((half - walked) / step);
        }
        walked += step;
    }
    edge.first().copied().unwrap_or_default()
}

/// The coverage problems check-coverage found in `found`, against the areas
/// they were found in; a warning when its results cannot be joined to them.
fn coverage_problems(
    areas: &FeatureSet,
    found: &FeatureSet,
    gap: Option<f64>,
    warnings: &mut Vec<String>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Some(areas) = areas.tables.first() else {
        return problems;
    };
    let mut rings = Vec::new();
    for feature in &areas.features {
        let mut ring = Ring::default();
        if let Some(polygon) = feature.parts.first()
            && let Some(exterior) = polygon.parts.first()
        {
            ring.exterior = ring_of(exterior);
            for hole in &polygon.parts[1..] {
                ring.holes.push(ring_of(hole));
            }
        }
        rings.push(ring);
    }
    // --include-valid: one result per area, in order - how an edge is joined
    // to its entity, since check-coverage carries no field.
    let mut index = 0;
    for table in &found.tables {
        let in_order = table.features.len() == areas.features.len();
        if !in_order {
            warnings.push(format!(
                "check-coverage gave back {} results for {} areas; its problems name no entity",
                table.features.len(),
                areas.features.len()
            ));
        }
        for feature in &table.features {
            let own = if in_order {
                index
            } else {
                areas.features.len()
            };
            index += 1;
            for part in &feature.parts {
                for chain in &part.parts {
                    if chain.len() < 2 {
                        continue;
                    }
                    let edge: Vec<Point2> = chain
                        .iter()
                        .map(|point| Point2::new(point.x, point.y))
                        .collect();
                    let entity = if in_order {
                        feature_id(areas, &areas.features[own], "katana_id")
                    } else {
                        None
                    };
                    let kind = coverage_kind(&rings, own, &edge, gap);
                    problems.push(Problem {
                        kind: String::from(kind),
                        entity,
                        at: middle_of(&edge),
                        reason: coverage_reason(kind),
                        length: Some(length_of(&edge)),
                        edge,
                    });
                }
            }
        }
    }
    problems
}

fn prepare_coverage(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    if tokens.size() < 3 || !(tokens.is(2, "CHECK") || tokens.is(2, "CLEAN")) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("GIS COVERAGE takes CHECK or CLEAN: {COVERAGE_USAGE}"),
            if tokens.size() > 2 {
                tokens[2].clone()
            } else {
                String::new()
            },
        ));
    }
    let clean = tokens.is(2, "CLEAN");
    let rules = if clean {
        WordRules {
            options: Vec::from(["gap", "snap", "merge"].map(String::from)),
            flags: Vec::from(["REPLACE", "PREVIEW"].map(String::from)),
            target: false,
            usage: String::from(COVERAGE_USAGE),
            ..WordRules::default()
        }
    } else {
        WordRules {
            options: Vec::from(["gap", "markers"].map(String::from)),
            flags: vec![String::from("PREVIEW")],
            target: false,
            usage: String::from(COVERAGE_USAGE),
            ..WordRules::default()
        }
    };
    let words = read_verb_words(tokens, 3, tokens.size(), &rules)?;
    let gap = length_option(&words, "gap")?;
    let snap = length_option(&words, "snap")?;
    let merge = choice_option(
        &words,
        "merge",
        &["longest-border", "max-area", "min-area", "min-index"],
        "",
    )?;
    let markers = words.option("markers").cloned();
    check_markers_layer(markers.as_deref())?;
    if clean && !words.has("REPLACE") {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "GIS COVERAGE CLEAN moves boundaries: add REPLACE to change the areas in place. \
             Surveyed legal boundaries must not be adjusted silently; GIS COVERAGE CHECK reports \
             first",
        ));
    }
    let mut scope = scoped(context, &words.scope)?;
    // check-coverage refuses anything but polygons (measured): the scope's
    // points and lines are left out, and counted.
    let stats = &scope.bound.dataset.stats;
    let ignored = stats.points + stats.lines;
    if ignored != 0 {
        scope.records.push(warning_record(&format!(
            "{ignored} points and lines in the scope are not areas; a coverage is of areas only"
        )));
    }
    let mut input = FeatureSet::default();
    if let Some(areas) = table_named(&scope.bound.dataset.set, "polygons") {
        input.tables.push(areas.clone());
    }
    let area_count = input.tables.first().map_or(0, |areas| areas.features.len());
    let mut settings = format!(
        "coverage mode={} gap={}",
        if clean { "clean" } else { "check" },
        gap.map(number).unwrap_or_default()
    );
    if clean {
        settings += &format!(
            " snap={} merge={merge}",
            snap.map(number).unwrap_or_default()
        );
    }
    settings += &format!(" areas={area_count} ignored={ignored}");
    if input.tables.is_empty() && markers.is_none() {
        let mut reply = vec![not_run_record("coverage", "")];
        reply.extend(scope.records);
        reply.push(settings);
        return Ok(answered("GIS COVERAGE", joined(&reply)));
    }
    if words.has("PREVIEW") {
        let mut reply = vec![String::from("gis op=coverage preview=yes")];
        reply.extend(scope.records);
        reply.push(settings);
        reply.push(String::from("preview valid=yes changed=no"));
        return Ok(answered("GIS COVERAGE", joined(&reply)));
    }

    let records = scope.records;
    let command_name = String::from(trimmed_text(line));
    let gap_width = gap;
    if clean {
        let mut gdal_words = Vec::new();
        if let Some(width) = gap_width {
            gdal_words.push(format!("--maximum-gap-width={}", number(width)));
        }
        if let Some(snap) = snap {
            gdal_words.push(format!("--snapping-distance={}", number(snap)));
        }
        if !merge.is_empty() {
            gdal_words.push(format!("--merge-strategy={merge}"));
        }
        let copies = copies_of(context.document, &ids_of(&input));
        let work = make_work(move |stop, progress| {
            let start = Instant::now();
            let mut warnings: Vec<String> = Vec::new();
            let cleaned = run_vector(
                &["vector", "clean-coverage"].map(String::from),
                &input,
                &gdal_words,
                stop,
                progress,
                &mut warnings,
                &[],
            )?;
            let mut unchanged = 0;
            let changed = changed_by(&input, &cleaned, &mut unchanged);
            let seconds = seconds_since(start);
            Ok(make_apply(move |context| {
                let mut reply = vec![gis_record("coverage", seconds)];
                reply.extend(records);
                apply_reshape(
                    context,
                    &copies,
                    &changed,
                    "vector clean-coverage",
                    &command_name,
                    &mut reply,
                )?;
                reply.push(format!("{settings} changed={}", changed.len()));
                if !changed.is_empty() {
                    reply.push(warning_record(&format!(
                        "{} boundaries moved: check them against the survey before relying on \
                         them; one UNDO puts them back",
                        changed.len()
                    )));
                }
                for warning in &warnings {
                    reply.push(warning_record(warning));
                }
                Ok(joined(&reply))
            }))
        });
        return Ok(Prepared {
            title: String::from("GIS COVERAGE"),
            reply: None,
            work: Some(work),
        });
    }

    let mut gdal_words = vec![String::from("--include-valid")];
    if let Some(width) = gap_width {
        gdal_words.push(format!("--maximum-gap-width={}", number(width)));
    }
    let marker_layer = markers;
    let work = make_work(move |stop, progress| {
        let start = Instant::now();
        let mut warnings: Vec<String> = Vec::new();
        let mut problems: Vec<Problem> = Vec::new();
        if !input.tables.is_empty() {
            let found = run_vector(
                &["vector", "check-coverage"].map(String::from),
                &input,
                &gdal_words,
                stop,
                progress,
                &mut warnings,
                &[],
            )?;
            problems = coverage_problems(&input, &found, gap_width, &mut warnings);
        }
        let mut entities: BTreeSet<EntityId> = BTreeSet::new();
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for problem in &problems {
            if let Some(entity) = problem.entity {
                entities.insert(entity);
            }
            *kinds.entry(problem.kind.clone()).or_insert(0) += 1;
        }
        let count = |kind: &str| kinds.get(kind).copied().unwrap_or(0);
        let summary = format!(
            "{settings} problems={} entities={} overlaps={} gaps={} mismatches={}",
            problems.len(),
            entities.len(),
            count("overlap"),
            count("gap"),
            count("mismatch")
        );
        let seconds = seconds_since(start);
        Ok(make_apply(move |context| {
            let mut reply = vec![gis_record("coverage", seconds)];
            reply.extend(records);
            for problem in &problems {
                reply.push(problem_record(problem));
            }
            reply.push(summary);
            if let Some(layer) = &marker_layer {
                reply.push(apply_markers(
                    context,
                    layer,
                    "coverage",
                    &problems,
                    &command_name,
                )?);
            }
            for warning in &warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("GIS COVERAGE"),
        reply: None,
        work: Some(work),
    })
}

/// GIS CHECK, its row in the verb table's V4 block (verb_table.rs).
pub fn prepare_gis_check(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_check(context, tokens, line)
}

/// GIS REPAIR, its row in the verb table's V4 block.
pub fn prepare_gis_repair(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_repair(context, tokens, line)
}

/// GIS COVERAGE CHECK and CLEAN, their row in the verb table's V4 block.
pub fn prepare_gis_coverage(
    context: &mut Context<'_>,
    tokens: &Tokens,
    line: &str,
) -> Result<Prepared> {
    prepare_coverage(context, tokens, line)
}
