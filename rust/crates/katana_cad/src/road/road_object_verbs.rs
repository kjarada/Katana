//! ROADOBJECT: road objects on the text command line
//! (include/katana/cad/road/road_object_verbs.hpp,
//! src/katana_cad/road/road_object_verbs.cpp; docs/road_objects.md, "The
//! verbs") - the interpreter's, so the window's command line, its road object
//! dialogs, katana_cli and katana_mcp place them alike.
//!
//! ```text
//! ROADOBJECT KINDS [<kind>]
//! ROADOBJECT LIST [JSON | <scope> [WHERE k=v ...]]
//! ROADOBJECT INFO <id>
//! ROADOBJECT ADD <kind> [key=value ...] <placement> [key=value ...] [PREVIEW]
//!   <placement> is one of
//!     ALIGNMENT <name> station=<s> [to=<s>] [offset=<o>] [spacing=<m>]
//!     ALONG #<id> [#<id> ...]            one object along each line
//!     ALONG FROM <scope> [WHERE k=v ...] one along each line the scope takes
//!     AT x,y[,z] [x,y[,z] ...]           at the points (a linear kind runs
//!                                        through them, a culvert from the
//!                                        first to the second)
//!     AT FROM <scope> [WHERE k=v ...]    at the points the scope takes
//! ROADOBJECT SET <id> key=value ... [PREVIEW]
//! ROADOBJECT COPY <id> [station=<s>] [dx=<m>] [dy=<m>] [key=value ...] [PREVIEW]
//! ROADOBJECT DELETE <id> [<id> ...] | ALL | <scope> [WHERE k=v ...] [PREVIEW]
//! ROADOBJECT QUANTITIES [<id> ... | <scope> [WHERE k=v ...]]
//! ROADOBJECT PROBLEMS
//! ```
//!
//! <kind> is barrier, guidepost, sign, light, culvert, wall, fence or marking.
//! The keys are those every object takes - name, layer, colour, material,
//! facility, station, to, offset, spacing, heights=profile|surface|line,
//! surface, crossfall, dz - and the kind's own (KINDS <kind> lists them with
//! their units, ranges and defaults). spacing= is for the point kinds (guide
//! posts, signs, light poles) along an alignment or a line - a barrier's and a
//! fence's posts take postspacing= - and crossfall= for an object that stands
//! on the profile; each is refused where nothing would read it. <scope> is the
//! shared scope and filter grammar (scope_verbs.rs); after its WHERE every
//! word with '=' is a condition, so an object's own keys stand before ALONG or
//! AT there. LIST, QUANTITIES and DELETE act on ids, or on all of them, or on
//! the objects a <scope> takes (road_object_scope.rs says what each choice
//! means for a road object, which is a definition and not an entity): ids and
//! a scope are not mixed, a scope word is never read as an id, and LIST JSON
//! is the whole stored set and takes no scope. The scope's record leads the
//! reply (scope=layers layers=road/objects/sign sublayers=yes matched=1), and
//! a scope that takes nothing is reported, not refused: DELETE then says
//! count=0 and leaves no undo step.
//!
//! Words are case-insensitive. Every edit is ONE undo step and a refused one
//! changes nothing; an ADD of several lines is all or nothing. PREVIEW says
//! what would be done and does nothing. A reply is records, one per line, in
//! the key=value form the options take; a scope's record (scope_record) leads
//! the reply of a line that took one, and a scope that takes nothing is
//! reported, not refused.

use std::collections::BTreeMap;

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::text::{format_exact_real, lowered, parse_finite_double, parse_integer, uppered};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{INVALID_ENTITY_ID, heights_of};
use katana_entity::{Entity, EntityId, EnumName, Geometry};
use katana_geometry::Point2;
use katana_geometry::chording::chord_arc;
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;

use crate::bim::bim_object::{BimModel, BimObject, Quantity, placed_objects};
use crate::document::Document;
use crate::road::road_object_mesh::is_closed_mesh;
use crate::road::road_object_scope::{RoadObjectMatch, match_road_objects};
use crate::road::road_objects::{
    HeightSource, KindShape, ParameterType, PlacedVertex, PlacementContext, PlacementKind,
    RoadObject, RoadObjectKind, RoadObjectPlacement, RoadObjectSet, apply_key, build_road_object,
    check_built, culvert_ends, effective_parameters, height_source_of, height_word, kind_from_word,
    kind_source, kind_title, kind_word, layer_of, parameter_text, parameters_of, placement_context,
    placement_word, road_object_kinds, road_objects_to_json, shape_of, total_quantities,
};
use crate::scope_verbs::{
    ScopeViewProvider, ScopeWords, is_scope_word, match_scope, parse_scope_words, record_value,
    scope_record,
};

const USAGE: &str = "ROADOBJECT KINDS [kind] | LIST [JSON] [scope] | INFO id | ADD kind \
                     <placement> [key=value ...] [PREVIEW] | SET id key=value ... [PREVIEW] | \
                     COPY id [station=] [dx=] [dy=] [key=value ...] [PREVIEW] | DELETE id \
                     ...|ALL|scope [PREVIEW] | QUANTITIES [id ...|scope] | PROBLEMS   (HELP \
                     ROADOBJECT)";

fn usage(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

// The C++ `uppered(text)` / `lowered(text)` as text: only ASCII letters change,
// so UTF-8 stays UTF-8.
fn upper(text: &str) -> String {
    String::from_utf8_lossy(&uppered(text)).into_owned()
}

fn lower(text: &str) -> String {
    String::from_utf8_lossy(&lowered(text)).into_owned()
}

// A real in a reply: to a micrometre, then exactly, so a length that is
// 54.99999999999999 by its arithmetic reads 55 and a typed 0.787 reads 0.787.
fn number(value: f64) -> String {
    let rounded = (value * 1e6).round() / 1e6;
    format_exact_real(if rounded == 0.0 { 0.0 } else { rounded })
}

fn quantity_keys(quantities: &[Quantity]) -> String {
    let mut out = String::new();
    for quantity in quantities {
        out += &format!(" {}={}", lower(&quantity.name), number(quantity.value));
    }
    out
}

fn placement_keys(object: &RoadObject) -> String {
    let placement = &object.placement;
    let mut out = format!(" placement={}", placement_word(placement.kind));
    if placement.kind == PlacementKind::Alignment {
        out += &format!(
            " alignment={} station={}",
            record_value(&placement.alignment),
            number(placement.station)
        );
        if let Some(to) = placement.to {
            out += &format!(" to={}", number(to));
        }
        out += &format!(" offset={}", number(placement.offset));
    } else {
        out += &format!(" points={}", placement.vertices.len());
        if placement.source != INVALID_ENTITY_ID {
            out += &format!(" source=#{}", placement.source);
        }
    }
    if let Some(spacing) = placement.spacing {
        out += &format!(" spacing={}", number(spacing));
    }
    let heights = height_source_of(placement);
    out += &format!(" heights={}", height_word(heights));
    if heights == HeightSource::Surface {
        out += &format!(" surface={}", record_value(&placement.surface));
    }
    if placement.crossfall != 0.0 {
        out += &format!(" crossfall={}", number(placement.crossfall));
    }
    if placement.dz != 0.0 {
        out += &format!(" dz={}", number(placement.dz));
    }
    out
}

fn object_record(word: &str, object: &RoadObject, built: &BimModel) -> String {
    let mut out = format!("{word} id={} kind={}", object.id, kind_word(object.kind));
    if !object.name.is_empty() {
        out += &format!(" name={}", record_value(&object.name));
    }
    out += &placement_keys(object);
    out += &format!(" layer={}", record_value(&layer_of(object)));
    if !object.facility.is_empty() {
        out += &format!(" facility={}", record_value(&object.facility));
    }
    out += &format!(" objects={}", built.objects.len());
    out += &quantity_keys(&total_quantities(&built.objects));
    if let Some(first) = built.problems.first() {
        out += &format!(" problem={}", record_value(&first.message));
    }
    out
}

// key=value, split at the first '='; None for a word without one.
fn key_value(word: &str) -> Option<(String, String)> {
    let equals = word.find('=')?;
    if equals == 0 {
        return None;
    }
    Some((
        String::from(&word[..equals]),
        String::from(&word[equals + 1..]),
    ))
}

// "x,y" or "x,y,z": a point typed on the line.
fn typed_point(word: &str) -> Option<PlacedVertex> {
    if word.contains('=') {
        return None;
    }
    let mut parts = Vec::new();
    for part in word.split(',') {
        parts.push(parse_finite_double(part)?);
    }
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    Some(PlacedVertex {
        position: Point2::new(parts[0], parts[1]),
        z: parts.get(2).copied(),
    })
}

// "#12": an entity id.
fn typed_id(word: &str) -> Option<EntityId> {
    if word.len() < 2 || !word.starts_with('#') {
        return None;
    }
    let value = parse_integer(&word.as_bytes()[1..])?;
    if value < 1 {
        return None;
    }
    Some(value as EntityId)
}

// The vertices of a line the drawing holds, with its heights: a line's two
// ends, a polyline's vertices (a closed one's first again at the end, so a
// fence goes all the way round), a curve polyline's or an arc's chords. None
// for anything that is not a line.
fn line_of(entity: &Entity) -> Option<Vec<PlacedVertex>> {
    let mut out: Vec<PlacedVertex> = Vec::new();
    match &entity.geometry {
        Geometry::Line(segment) => {
            let heights = heights_of(&entity.properties, 2);
            out.push(PlacedVertex {
                position: segment.start,
                z: heights[0],
            });
            out.push(PlacedVertex {
                position: segment.end,
                z: heights[1],
            });
        }
        Geometry::Polyline(polyline) => {
            let heights = heights_of(&entity.properties, polyline.vertices.len());
            for (&position, &z) in polyline.vertices.iter().zip(&heights) {
                out.push(PlacedVertex { position, z });
            }
            if polyline.closed && out.len() > 2 {
                out.push(out[0]);
            }
        }
        Geometry::CurvePolyline(curve) => {
            for step in curve.tessellate_with_heights(CURVE_CHORD_TOLERANCE) {
                out.push(PlacedVertex {
                    position: step.position,
                    z: step.height,
                });
            }
        }
        Geometry::Arc(arc) => {
            let chords = chord_arc(arc, CURVE_CHORD_TOLERANCE);
            // An arc keeps a height at each end - two in `elevations`, or the
            // one `elevation` both share - and one on a grade rises evenly by
            // its sweep, as section annotation reads it. The chords stand at
            // equal fractions of the sweep, so each takes that fraction of the
            // rise. An end with no height leaves every chord with none, and the
            // placement refuses it, naming surface=.
            let ends = heights_of(&entity.properties, 2);
            for (i, &chord) in chords.iter().enumerate() {
                let mut height = None;
                if let (Some(first), Some(last)) = (ends[0], ends[1]) {
                    let fraction = if chords.len() > 1 {
                        i as f64 / (chords.len() - 1) as f64
                    } else {
                        0.0
                    };
                    height = Some(first + (last - first) * fraction);
                }
                out.push(PlacedVertex {
                    position: chord,
                    z: height,
                });
            }
        }
        _ => return None,
    }
    if out.len() < 2 {
        return None;
    }
    Some(out)
}

fn point_of(entity: &Entity) -> Option<PlacedVertex> {
    let Geometry::Point(point) = &entity.geometry else {
        return None;
    };
    Some(PlacedVertex {
        position: point.position,
        z: heights_of(&entity.properties, 1)[0],
    })
}

fn type_name(entity: &Entity) -> String {
    lower(entity.r#type().name())
}

// The set as stored, refusing to work on objects that could not be read
// rather than overwrite them.
fn editable_set(document: &Document) -> Result<RoadObjectSet> {
    if let Err(error) = document.road_objects_status() {
        return Err(make_error(
            ErrorCode::CommandRejected,
            "the drawing's road objects cannot be read, and are kept as they are",
            error.describe(),
        ));
    }
    Ok(RoadObjectSet::clone(&document.road_objects()))
}

// Made, or refused naming what stopped it: what every edit checks before it
// commits, so a definition the model would only list as a problem is never
// stored (check_built, road_objects.rs).
fn check_builds(context: &PlacementContext<'_>, object: &RoadObject) -> Status {
    check_built(object, &build_road_object(context, object))
}

// LIST, QUANTITIES and DELETE take the objects by id, or by the shared scope
// and filter (road_object_scope.rs): when the word at `at` begins a scope it
// is read by the one parser from there, `at` is left at the first word after
// it, and the objects it takes are matched; None when it does not, so the
// words are ids. A scope word is never ignored, and never read as an id that
// does not exist (docs/road_objects.md, "Scope and filter").
fn read_scope(
    document: &Document,
    args: &[String],
    at: &mut usize,
    views: ScopeViewProvider<'_>,
) -> Result<Option<RoadObjectMatch>> {
    if *at >= args.len() || !is_scope_word(&args[*at]) {
        return Ok(None);
    }
    let words = parse_scope_words(args, at)?;
    Ok(Some(match_road_objects(document, &words, views)?))
}

// The words that ended a scope, which are the verb's own: `allowed` (PREVIEW
// for DELETE) and nothing else.
fn only_after_scope(verb: &str, args: &[String], at: usize, allowed: &str) -> Status {
    for word in args.iter().skip(at) {
        if allowed.is_empty() || upper(word) != allowed {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!(
                    "not a word ROADOBJECT {verb} takes after a scope{}",
                    if allowed.is_empty() {
                        String::new()
                    } else {
                        format!(": {allowed} may end the line")
                    }
                ),
                word.as_str(),
            ));
        }
    }
    Ok(())
}

// An id among the words of a line that gave a scope, or a scope word among
// ids: one or the other.
fn ids_or_scope(verb: &str, word: &str) -> Error {
    make_error(
        ErrorCode::InvalidArgument,
        format!(
            "ROADOBJECT {verb} takes ids or one scope (DRAWING, SELECTION, VIEW, AREA or LAYERS, \
             then WHERE ...), not both"
        ),
        word,
    )
}

// One line more of a reply that is lines.
fn push_line(out: &mut String, text: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(text);
}

// ---- the subcommands ------------------------------------------------------------------------

fn shape_word(shape: KindShape) -> &'static str {
    match shape {
        KindShape::Linear => "linear",
        KindShape::Point => "point",
        KindShape::Crossing => "crossing",
    }
}

fn kinds(args: &[String]) -> Result<String> {
    let mut out = String::new();
    if args.len() > 2 {
        let kind = kind_from_word(&args[2])?;
        push_line(
            &mut out,
            &format!(
                "kind word={} title={} shape={} source={}",
                kind_word(kind),
                record_value(kind_title(kind)),
                shape_word(shape_of(kind)),
                record_value(kind_source(kind))
            ),
        );
        for spec in parameters_of(kind) {
            let mut record = format!("parameter key={}", spec.key);
            match spec.r#type {
                ParameterType::Number => {
                    record += " type=number";
                    if !spec.unit.is_empty() {
                        record += &format!(" unit={}", spec.unit);
                    }
                    record += &format!(
                        " default={} min={} max={}",
                        record_value(spec.default_text),
                        number(spec.minimum),
                        number(spec.maximum)
                    );
                    if spec.whole {
                        record += " whole=yes";
                    }
                }
                ParameterType::Choice => {
                    record += &format!(
                        " type=choice default={} choices={}",
                        record_value(spec.default_text),
                        spec.choices.join(",")
                    );
                }
                ParameterType::Text => record += " type=text",
            }
            record += &format!(" description={}", record_value(spec.description));
            push_line(&mut out, &record);
        }
        return Ok(out);
    }
    for &kind in road_object_kinds() {
        let keys: Vec<&str> = parameters_of(kind).iter().map(|spec| spec.key).collect();
        push_line(
            &mut out,
            &format!(
                "kind word={} title={} shape={} parameters={}",
                kind_word(kind),
                record_value(kind_title(kind)),
                shape_word(shape_of(kind)),
                keys.join(",")
            ),
        );
    }
    Ok(out)
}

fn list(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    let mut at = 2;
    let json = at < args.len() && upper(&args[at]) == "JSON";
    if json {
        at += 1;
    }
    let scoped = read_scope(document, args, &mut at, views)?;
    if at < args.len() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "usage: ROADOBJECT LIST [JSON] [<scope> [WHERE key=value ...]]",
            args[at].as_str(),
        ));
    }
    if json && scoped.is_some() {
        // The stored set is one document, and a scope's record cannot lead it
        // without making it something else.
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "ROADOBJECT LIST JSON is the whole stored set: LIST with a scope gives the objects \
             it takes as records, and the katana_road_objects tool gives them as data",
            args[2].as_str(),
        ));
    }
    document.road_objects_status()?;
    let objects = document.road_objects();
    if json {
        return road_objects_to_json(&objects);
    }
    let context = placement_context(document);
    let mut body = String::new();
    let mut problems = 0;
    let mut count = 0;
    for object in &objects.objects {
        if let Some(scoped) = &scoped
            && !scoped.ids.contains(&object.id)
        {
            continue;
        }
        let built = build_road_object(&context, object);
        problems += built.problems.len();
        count += 1;
        body += "\n";
        body += &object_record("object", object, &built);
    }
    let lead = match &scoped {
        Some(scoped) => format!("{}\n", scoped.record()),
        None => String::new(),
    };
    Ok(format!(
        "{lead}roadobjects count={count} problems={problems}{body}"
    ))
}

fn info(document: &Document, args: &[String]) -> Result<String> {
    if args.len() < 3 {
        return Err(usage("ROADOBJECT INFO id"));
    }
    // A set that cannot be read is empty to find(); say why, not that the
    // object is not there.
    document.road_objects_status()?;
    let objects = document.road_objects();
    let Some(object) = objects.find(&args[2]) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no road object has that id",
            args[2].as_str(),
        ));
    };
    let context = placement_context(document);
    let built = build_road_object(&context, object);
    let mut out = object_record("object", object, &built);
    if object.kind == RoadObjectKind::Culvert
        && let Ok(ends) = culvert_ends(&context, object)
    {
        out += &format!(
            " inlet={},{},{} outlet={},{},{}",
            number(ends.inlet.x),
            number(ends.inlet.y),
            number(ends.inlet.z),
            number(ends.outlet.x),
            number(ends.outlet.y),
            number(ends.outlet.z)
        );
    }
    let effective = effective_parameters(object);
    for spec in parameters_of(object.kind) {
        out += &format!("\nparameter key={}", spec.key);
        let value = match effective.get(spec.key) {
            None => record_value(if spec.default_text.is_empty() {
                "none"
            } else {
                spec.default_text
            }),
            Some(value) => record_value(&parameter_text(value)),
        };
        out += &format!(" value={value}");
        out += if object.parameters.contains_key(spec.key) {
            " given=yes"
        } else {
            " given=no"
        };
        if !spec.unit.is_empty() {
            out += &format!(" unit={}", spec.unit);
        }
    }
    for made in &built.objects {
        out += &format!(
            "\npart id={} class={} type={} material={} triangles={} closed={}",
            record_value(&made.id),
            made.ifc_class,
            made.predefined_type,
            record_value(&made.material),
            made.mesh.triangle_count(),
            if is_closed_mesh(&made.mesh) {
                "yes"
            } else {
                "no"
            }
        );
        out += &quantity_keys(&made.quantities);
    }
    for problem in &built.problems {
        out += &format!("\nproblem message={}", record_value(&problem.message));
    }
    Ok(out)
}

#[derive(Clone, Debug, Default)]
struct AddRequest {
    placement: Option<PlacementKind>,
    alignment: String,
    ids: Vec<EntityId>,
    scope: Option<ScopeWords>,
    points: Vec<PlacedVertex>,
    options: Vec<(String, String)>,
    preview: bool,
}

fn placement_once(request: &mut AddRequest, kind: PlacementKind) -> Status {
    if request.placement.is_some() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "give one placement: ALIGNMENT, ALONG or AT",
        ));
    }
    request.placement = Some(kind);
    Ok(())
}

fn read_add(args: &[String]) -> Result<AddRequest> {
    let mut request = AddRequest::default();
    let mut i = 3;
    while i < args.len() {
        let word = &args[i];
        let folded = upper(word);
        if folded == "PREVIEW" {
            request.preview = true;
            i += 1;
            continue;
        }
        if folded == "ALIGNMENT" {
            placement_once(&mut request, PlacementKind::Alignment)?;
            if i + 1 >= args.len() {
                return Err(usage(
                    "ROADOBJECT ADD kind ALIGNMENT name station=s [to=s] [offset=o]",
                ));
            }
            request.alignment.clone_from(&args[i + 1]);
            i += 2;
            continue;
        }
        if folded == "ALONG" || folded == "AT" {
            let along = folded == "ALONG";
            placement_once(
                &mut request,
                if along {
                    PlacementKind::Line
                } else {
                    PlacementKind::Points
                },
            )?;
            i += 1;
            if i < args.len() && upper(&args[i]) == "FROM" {
                i += 1;
                request.scope = Some(parse_scope_words(args, &mut i)?);
                continue;
            }
            let before = i;
            while i < args.len() {
                if along {
                    let Some(id) = typed_id(&args[i]) else {
                        break;
                    };
                    // One object along each line: a line named twice would be
                    // two objects on the same ground, doubling its quantities.
                    if request.ids.contains(&id) {
                        return Err(make_error(
                            ErrorCode::InvalidArgument,
                            "ALONG names that line twice; one object is made along each line",
                            args[i].as_str(),
                        ));
                    }
                    request.ids.push(id);
                } else {
                    let Some(point) = typed_point(&args[i]) else {
                        break;
                    };
                    request.points.push(point);
                }
                i += 1;
            }
            if i == before {
                return Err(usage(if along {
                    "ROADOBJECT ADD kind ALONG #id [#id ...] | ALONG FROM scope"
                } else {
                    "ROADOBJECT ADD kind AT x,y[,z] [x,y[,z] ...] | AT FROM scope"
                }));
            }
            continue;
        }
        if let Some(pair) = key_value(word) {
            request.options.push(pair);
            i += 1;
            continue;
        }
        return Err(make_error(
            ErrorCode::ParseFailure,
            "not a word ROADOBJECT ADD takes here: a placement (ALIGNMENT, ALONG, AT), \
             key=value or PREVIEW",
            word.as_str(),
        ));
    }
    if request.placement.is_none() {
        return Err(usage(
            "ROADOBJECT ADD kind ALIGNMENT name station=s ... | ALONG #id ... | ALONG FROM \
             scope | AT x,y ... | AT FROM scope",
        ));
    }
    Ok(request)
}

fn add(document: &mut Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 3 {
        return Err(usage(
            "ROADOBJECT ADD kind <placement> [key=value ...] [PREVIEW]",
        ));
    }
    let kind = kind_from_word(&args[2])?;
    let request = read_add(args)?;
    // read_add refuses a line with no placement.
    let placement = request.placement.unwrap_or_default();
    let mut set = editable_set(document)?;
    let reply = {
        // What the placement takes, before any object is made of it.
        let mut lead = String::new();
        let mut lines: Vec<(EntityId, Vec<PlacedVertex>)> = Vec::new();
        let mut points = request.points.clone();
        let model = document.model();
        if let Some(scope) = &request.scope {
            let found = match_scope(document, scope, views)?;
            let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
            for &id in &found.matched {
                let Some(entity) = model.entities.find(id) else {
                    continue;
                };
                if placement == PlacementKind::Line {
                    if let Some(vertices) = line_of(entity) {
                        lines.push((id, vertices));
                        continue;
                    }
                } else if let Some(point) = point_of(entity) {
                    points.push(point);
                    continue;
                }
                *skipped.entry(type_name(entity)).or_insert(0) += 1;
            }
            let left: usize = skipped.values().sum();
            let which: Vec<&str> = skipped.keys().map(String::as_str).collect();
            let line = placement == PlacementKind::Line;
            lead = format!(
                "{} {}={} skipped={left}",
                scope_record(&found),
                if line { "lines" } else { "points" },
                if line { lines.len() } else { points.len() }
            );
            if left > 0 {
                lead += &format!(" skippedtypes={}", which.join(","));
            }
            if lines.is_empty() && points.is_empty() {
                // A scope that takes nothing to place on is reported, not
                // refused.
                return Ok(format!("{lead} added=0"));
            }
        } else {
            for &id in &request.ids {
                let Some(entity) = model.entities.find(id) else {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "no entity has that id",
                        format!("#{id}"),
                    ));
                };
                let Some(vertices) = line_of(entity) else {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        format!("#{id} is a {}, not a line", type_name(entity)),
                    ));
                };
                lines.push((id, vertices));
            }
        }

        // The objects: one along the alignment, one along each line, one at
        // the points.
        let start = |placement: PlacementKind| RoadObject {
            kind,
            placement: RoadObjectPlacement {
                kind: placement,
                ..RoadObjectPlacement::default()
            },
            ..RoadObject::default()
        };
        let mut made: Vec<RoadObject> = Vec::new();
        match placement {
            PlacementKind::Alignment => {
                let mut object = start(PlacementKind::Alignment);
                object.placement.alignment.clone_from(&request.alignment);
                made.push(object);
            }
            PlacementKind::Line => {
                for (id, vertices) in lines {
                    let mut object = start(PlacementKind::Line);
                    object.placement.vertices = vertices;
                    object.placement.source = id;
                    made.push(object);
                }
            }
            PlacementKind::Points => {
                let mut object = start(PlacementKind::Points);
                object.placement.vertices = points;
                made.push(object);
            }
        }
        let context = placement_context(document);
        let mut reply = lead;
        for object in &mut made {
            object.id = set.take_id(object.kind);
            for (key, value) in &request.options {
                apply_key(object, key, value)?;
            }
            check_builds(&context, object)?;
            let record = object_record(
                if request.preview { "preview" } else { "added" },
                object,
                &build_road_object(&context, object),
            );
            push_line(&mut reply, &record);
        }
        if request.preview {
            return Ok(reply);
        }
        set.objects.extend(made);
        reply
    };
    document.set_road_objects(&set, "ROADOBJECT ADD")?;
    Ok(reply)
}

fn set_keys(document: &mut Document, args: &[String]) -> Result<String> {
    if args.len() < 4 {
        return Err(usage("ROADOBJECT SET id key=value ... [PREVIEW]"));
    }
    let mut set = editable_set(document)?;
    let Some(object) = set.find_mut(&args[2]) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no road object has that id",
            args[2].as_str(),
        ));
    };
    let mut preview = false;
    let mut changed = String::new();
    for word in &args[3..] {
        if upper(word) == "PREVIEW" {
            preview = true;
            continue;
        }
        let Some((key, value)) = key_value(word) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "ROADOBJECT SET takes key=value",
                word.as_str(),
            ));
        };
        apply_key(object, &key, &value)?;
        if !changed.is_empty() {
            changed.push(',');
        }
        changed += &lower(&key);
    }
    if changed.is_empty() {
        return Err(usage("ROADOBJECT SET id key=value ... [PREVIEW]"));
    }
    let reply = {
        let context = placement_context(document);
        check_builds(&context, object)?;
        format!(
            "{} changed={changed}",
            object_record(
                if preview { "preview" } else { "set" },
                object,
                &build_road_object(&context, object)
            )
        )
    };
    if !preview {
        document.set_road_objects(&set, "ROADOBJECT SET")?;
    }
    Ok(reply)
}

fn copy(document: &mut Document, args: &[String]) -> Result<String> {
    if args.len() < 3 {
        return Err(usage(
            "ROADOBJECT COPY id [station=s] [dx=m] [dy=m] [key=value ...] [PREVIEW]",
        ));
    }
    let mut set = editable_set(document)?;
    let Some(source) = set.find(&args[2]) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no road object has that id",
            args[2].as_str(),
        ));
    };
    let mut made = source.clone();
    let from = source.id.clone();
    made.id = set.take_id(made.kind);
    // A to= given is the copy's end whichever order the keys are typed in:
    // only an end inherited from the source moves with the station.
    let end_given = args[3..]
        .iter()
        .any(|word| key_value(word).is_some_and(|(key, _)| lower(&key) == "to"));
    let mut preview = false;
    for word in &args[3..] {
        if upper(word) == "PREVIEW" {
            preview = true;
            continue;
        }
        let Some((raw_key, value)) = key_value(word) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "ROADOBJECT COPY takes key=value",
                word.as_str(),
            ));
        };
        let key = lower(&raw_key);
        if key == "station" && made.placement.kind == PlacementKind::Alignment {
            // The copy keeps its length: its end moves with its start.
            let Some(station) = parse_finite_double(&value) else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "station= must be a number",
                    value,
                ));
            };
            let placement = &mut made.placement;
            if !end_given && let Some(to) = placement.to.as_mut() {
                *to += station - placement.station;
            }
            placement.station = station;
            continue;
        }
        if key == "dx" || key == "dy" {
            if made.placement.kind == PlacementKind::Alignment {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "dx= and dy= move a copy placed along a line or at points; along an \
                     alignment give station= and offset=",
                    from.as_str(),
                ));
            }
            let Some(shift) = parse_finite_double(&value) else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!("{key}= must be a number"),
                    value,
                ));
            };
            for vertex in &mut made.placement.vertices {
                if key == "dx" {
                    vertex.position.x += shift;
                } else {
                    vertex.position.y += shift;
                }
            }
            continue;
        }
        apply_key(&mut made, &raw_key, &value)?;
    }
    let reply = {
        let context = placement_context(document);
        check_builds(&context, &made)?;
        format!(
            "{} from={from}",
            object_record(
                if preview { "preview" } else { "copied" },
                &made,
                &build_road_object(&context, &made)
            )
        )
    };
    if !preview {
        set.objects.push(made);
        document.set_road_objects(&set, "ROADOBJECT COPY")?;
    }
    Ok(reply)
}

fn remove(
    document: &mut Document,
    args: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    if args.len() < 3 {
        return Err(usage(
            "ROADOBJECT DELETE id [id ...] | ALL | <scope> [WHERE key=value ...] [PREVIEW]",
        ));
    }
    let mut set = editable_set(document)?;
    let mut preview = false;
    let mut all = false;
    let mut ids: Vec<String> = Vec::new();
    // ALL on its own is every object, and keeps its plain reply; ALL followed
    // by WHERE is the shared scope's DRAWING, with the scope's record.
    let given: Vec<&String> = args[2..]
        .iter()
        .filter(|word| upper(word) != "PREVIEW")
        .collect();
    let bare_all = given.len() == 1 && upper(given[0]) == "ALL";
    let mut lead = String::new();
    let mut at = 2;
    let scoped = if bare_all {
        None
    } else {
        read_scope(document, args, &mut at, views)?
    };
    if let Some(scoped) = &scoped {
        only_after_scope("DELETE", args, at, "PREVIEW")?;
        preview = at < args.len();
        ids.clone_from(&scoped.ids);
        lead = format!("{}\n", scoped.record());
    } else {
        for word in &args[2..] {
            let folded = upper(word);
            if folded == "PREVIEW" {
                preview = true;
            } else if folded == "ALL" {
                all = true;
            } else {
                if is_scope_word(word) {
                    return Err(ids_or_scope("DELETE", word));
                }
                let Some(object) = set.find(word) else {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "no road object has that id",
                        word.as_str(),
                    ));
                };
                if !ids.contains(&object.id) {
                    ids.push(object.id.clone());
                }
            }
        }
    }
    if all {
        ids = set.objects.iter().map(|object| object.id.clone()).collect();
    }
    let reply = format!(
        "{lead}{} count={}{}",
        if preview { "preview delete" } else { "deleted" },
        ids.len(),
        if ids.is_empty() {
            String::new()
        } else {
            format!(" ids={}", ids.join(","))
        }
    );
    if preview || ids.is_empty() {
        return Ok(reply);
    }
    set.objects.retain(|object| !ids.contains(&object.id));
    document.set_road_objects(&set, "ROADOBJECT DELETE")?;
    Ok(reply)
}

fn quantities(
    document: &Document,
    args: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    document.road_objects_status()?;
    let set = document.road_objects();
    let mut at = 2;
    let scoped = read_scope(document, args, &mut at, views)?;
    if scoped.is_some() {
        only_after_scope("QUANTITIES", args, at, "")?;
    }
    let mut chosen: Vec<&RoadObject> = Vec::new();
    if let Some(scoped) = &scoped {
        for object in &set.objects {
            if scoped.ids.contains(&object.id) {
                chosen.push(object);
            }
        }
    } else {
        for word in &args[2..] {
            if is_scope_word(word) {
                return Err(ids_or_scope("QUANTITIES", word));
            }
            let Some(object) = set.find(word) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "no road object has that id",
                    word.as_str(),
                ));
            };
            // An id named twice is one object, as DELETE reads it: counting it
            // twice would double its length and its count in the totals.
            if !chosen.iter().any(|already| std::ptr::eq(*already, object)) {
                chosen.push(object);
            }
        }
        if chosen.is_empty() {
            chosen.extend(set.objects.iter());
        }
    }
    let context = placement_context(document);
    let mut out = String::new();
    if let Some(scoped) = &scoped {
        push_line(&mut out, &scoped.record());
    }
    // The totals by kind, in the order the kinds are listed.
    let mut by_kind: BTreeMap<RoadObjectKind, (usize, Vec<BimObject>)> = BTreeMap::new();
    for object in &chosen {
        let built = build_road_object(&context, object);
        for made in &built.objects {
            push_line(
                &mut out,
                &format!(
                    "quantity object={} part={} class={} type={}{}",
                    object.id,
                    record_value(&made.id),
                    made.ifc_class,
                    made.predefined_type,
                    quantity_keys(&made.quantities)
                ),
            );
        }
        for problem in &built.problems {
            push_line(
                &mut out,
                &format!(
                    "problem object={} message={}",
                    object.id,
                    record_value(&problem.message)
                ),
            );
        }
        let entry = by_kind.entry(object.kind).or_default();
        entry.0 += 1;
        entry.1.extend(built.objects);
    }
    for (kind, (count, objects)) in &by_kind {
        push_line(
            &mut out,
            &format!(
                "total kind={} objects={count}{}",
                kind_word(*kind),
                quantity_keys(&total_quantities(objects))
            ),
        );
    }
    if chosen.is_empty() {
        push_line(&mut out, "roadobjects count=0");
    }
    Ok(out)
}

fn problems(document: &Document) -> String {
    let model = placed_objects(document);
    if model.problems.is_empty() {
        return String::from("problems=0");
    }
    let mut out = String::new();
    for problem in &model.problems {
        push_line(
            &mut out,
            &format!(
                "problem source={} message={}",
                record_value(&problem.source),
                record_value(&problem.message)
            ),
        );
    }
    out
}

/// True for ROADOBJECT and ROADOBJECTS, in any case.
pub fn is_road_object_verb(verb: &str) -> bool {
    let folded = upper(verb);
    folded == "ROADOBJECT" || folded == "ROADOBJECTS"
}

/// Runs one ROADOBJECT line: `tokens` is the whole line split into words with
/// the quotes removed (the interpreter's tokenize), the verb first. `views`
/// answers the scope word VIEW (the window's; None headless, the C++ default).
pub fn run_road_object_verb(
    document: &mut Document,
    tokens: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    if tokens.len() < 2 {
        return Err(usage(USAGE));
    }
    let action = upper(&tokens[1]);
    match action.as_str() {
        "KINDS" => kinds(tokens),
        "LIST" => list(document, tokens, views),
        "INFO" => info(document, tokens),
        "ADD" => add(document, tokens, views),
        "SET" => set_keys(document, tokens),
        "COPY" => copy(document, tokens),
        "DELETE" => remove(document, tokens, views),
        "QUANTITIES" => quantities(document, tokens, views),
        "PROBLEMS" => Ok(problems(document)),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("not a ROADOBJECT word; {USAGE}"),
            tokens[1].as_str(),
        )),
    }
}

/// Every ROADOBJECT word and key, for HELP ROADOBJECT.
pub fn road_object_verb_help() -> String {
    let mut text = String::from(
        "Road objects: furniture and structures placed along an alignment or a line
(docs/road_objects.md). Every edit is one undo step; PREVIEW changes nothing.

ROADOBJECT KINDS [kind]          the kinds, or one kind's keys with units, ranges, defaults
ROADOBJECT LIST [JSON]           a record per object, or the stored JSON
ROADOBJECT LIST <scope> [WHERE k=v ...]     the objects a scope takes (below)
ROADOBJECT INFO id               its keys, the 3D objects made of it, its problems
ROADOBJECT ADD kind [key=value ...] <placement> [key=value ...] [PREVIEW]
  placement: ALIGNMENT name station=s [to=s] [offset=o] [spacing=m]
             ALONG #id [#id ...] | ALONG FROM <scope> [WHERE k=v ...]   one along each line
             AT x,y[,z] ... | AT FROM <scope> [WHERE k=v ...]            at the points
  (after WHERE every key=value is a condition: give an object's keys before ALONG or AT)
ROADOBJECT SET id key=value ... [PREVIEW]
ROADOBJECT COPY id [station=s] [dx=m] [dy=m] [key=value ...] [PREVIEW]
ROADOBJECT DELETE id [id ...] | ALL | <scope> [WHERE k=v ...] [PREVIEW]
ROADOBJECT QUANTITIES [id ... | <scope> [WHERE k=v ...]]   per 3D object, then totals by kind
ROADOBJECT PROBLEMS              what could not be made, and why
Every object: name= layer= colour=#RRGGBB|name material= facility= (the road it is part of)
  station= to= offset= (left positive) spacing= (guide posts, signs, lights) heights=profile|surface|line
  surface=name crossfall=% (from the centreline out, on the profile) dz= (raised from the ground)
  (empty: offset= crossfall= dz= go back to 0; to= spacing= heights= surface= and the kind's keys clear)
<scope> is SELECTION | DRAWING | VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY], as every
  verb on drawing data takes it; its record (scope=... matched=N) leads the reply, and a scope that
  takes nothing is reported. Of a road object: LAYERS and WHERE LAYER= read its layer; AREA and VIEW
  its plan extent (an object that is not made has none: unmade=N); SELECTION the objects placed
  ALONG a selected line. WHERE PROP=key:pattern reads the keys SET writes (kind, id, type, offset ...),
  COLOUR= the colour it is drawn in, DRAWN a layer that is shown; TYPE=, STYLE= and TEXT= are refused.
Kinds:",
    );
    for &kind in road_object_kinds() {
        let mut keys = String::new();
        for spec in parameters_of(kind) {
            keys += &format!(" {}=", spec.key);
        }
        text += &format!("\n  {:<10}{keys}", kind_word(kind));
    }
    text
}

// The report of the drawing's road objects, or of the ones `found` took.
fn report_of(document: &Document, found: Option<&RoadObjectMatch>) -> String {
    let mut report = Json::object_of([
        ("format", Json::from("katana-road-objects-report")),
        ("version", Json::from(1_i32)),
    ]);
    if let Some(found) = found {
        report["scope"] = Json::object_of([
            ("record", Json::from(found.record())),
            ("matched", Json::from(found.ids.len())),
            ("unmade", Json::from(found.unmade)),
        ]);
    }
    let mut objects: Vec<Json> = Vec::new();
    let mut problems: Vec<Json> = Vec::new();
    if let Err(error) = document.road_objects_status() {
        problems.push(Json::object_of([
            ("source", Json::from("road objects")),
            ("message", Json::from(error.describe())),
        ]));
    }
    let context = placement_context(document);
    let set = document.road_objects();
    // json::parse(text, nullptr, false): a text that does not parse is the
    // discarded value, which is no object - and so no list.
    let definitions = road_objects_to_json(&set)
        .ok()
        .and_then(|text| Json::parse(text).ok())
        .unwrap_or_default();
    let empty = Json::array();
    let list = if definitions.is_object() && definitions.contains("objects") {
        &definitions["objects"]
    } else {
        &empty
    };
    for (index, object) in set.objects.iter().enumerate() {
        let mut entry = if index < list.size() {
            list[index].clone()
        } else {
            Json::object_of([("id", Json::from(object.id.as_str()))])
        };
        if let Some(found) = found
            && !found.ids.contains(&object.id)
        {
            continue;
        }
        let built = build_road_object(&context, object);
        let mut made: Vec<Json> = Vec::new();
        for part in &built.objects {
            let mut quantities = Json::object();
            for quantity in &part.quantities {
                quantities[quantity.name.as_str()] = Json::from(quantity.value);
            }
            made.push(Json::object_of([
                ("id", Json::from(part.id.as_str())),
                ("name", Json::from(part.name.as_str())),
                ("ifc_class", Json::from(part.ifc_class.as_str())),
                ("predefined_type", Json::from(part.predefined_type.as_str())),
                ("layer", Json::from(part.layer.as_str())),
                ("material", Json::from(part.material.as_str())),
                ("triangles", Json::from(part.mesh.triangle_count())),
                ("closed", Json::from(is_closed_mesh(&part.mesh))),
                ("quantities", quantities),
            ]));
        }
        entry["made"] = Json::from(made);
        let mut own: Vec<Json> = Vec::new();
        for problem in &built.problems {
            own.push(Json::from(problem.message.as_str()));
            problems.push(Json::object_of([
                ("source", Json::from(problem.source.as_str())),
                ("message", Json::from(problem.message.as_str())),
            ]));
        }
        entry["problems"] = Json::from(own);
        objects.push(entry);
    }
    report["objects"] = Json::from(objects);
    report["problems"] = Json::from(problems);
    report.dump_with(2, ' ', false)
}

/// The drawing's road objects for an agent, as JSON: each definition as the
/// drawing keeps it, with the 3D objects made from it - id, IFC class and
/// predefined type, layer, triangles, quantities - and the problems. What
/// katana_mcp's katana_road_objects tool returns (a tool, not a resource:
/// docs/mcp.md).
pub fn road_objects_report(document: &Document) -> String {
    report_of(document, None)
}

/// The same for the objects a scope and filter take, with a "scope" member:
/// `record` (the scope's record, as a reply leads with it), `matched`, and
/// `unmade` (objects the scope would take but for not being made). The
/// problems are those of the objects it took. Fails as match_road_objects
/// does. (The C++ overload `roadObjectsReport(document, words, views)`.)
pub fn road_objects_report_for(
    document: &Document,
    words: &ScopeWords,
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let found = match_road_objects(document, words, views)?;
    Ok(report_of(document, Some(&found)))
}
