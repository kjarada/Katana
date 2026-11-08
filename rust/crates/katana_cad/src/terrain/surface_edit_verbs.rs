//! Editing the drawing's surfaces on the command line
//! (include/katana/cad/terrain/surface_edit_verbs.hpp,
//! src/katana_cad/terrain/surface_edit_verbs.cpp; docs/surface_editing.md):
//! the verbs a person, a script, katana_cli, katana_mcp and the window's own
//! tools and dialogs run, so each edit has one code path.
//!
//! ```text
//! SURFACE EDIT <name> DELETE TRIANGLE x,y
//!                     DELETE TRIANGLES INSIDE|OUTSIDE <polygon>
//!                     DELETE LONG <length> | DELETE THIN <degrees>
//!                     DELETE POINT x,y [reach=<m>] | DELETE POINTS INSIDE <polygon>
//!                     FLIP x,y
//!                     ADD POINT x,y,z [x,y,z ...] | ADD POINTS <points>
//!                     BREAKLINE <line>
//!                     BOUNDARY <polygon> | HOLE <polygon>
//!                     RAISE <dz> | LOWER <dz> | SETLEVEL <z> | SCALE <factor> [base=<z>]
//!                         [INSIDE <polygon>]
//!                     SMOOTH [passes=3] [weight=0.5] [INSIDE <polygon>]
//!                     THIN <tolerance>
//! SURFACE MERGE <a> <b> [<c> ...] NAME <new>      (later surfaces win)
//! SURFACE PASTE <top> INTO <base> [NAME <new>]    (in place unless NAME)
//! SURFACE CLIP <name> <polygon> NAME <new>
//! SURFACE OFFSET <name> <dz> NAME <new>
//! ```
//!
//! <polygon>, <line> and <points> are the drawing's data, given three ways:
//!
//! ```text
//! #id [#id ...]                 entities by id;
//! x,y[,z] x,y[,z] ...           coordinates typed in (a polygon's three or
//!                               more corners, a line's vertices - a z is the
//!                               vertex's level, none drapes it on the surface
//!                               - or the x,y,z of points);
//! FROM <scope> [WHERE k=v ...]  what the shared scope takes (scope_verbs.rs),
//!                               the reply saying what it took (scope= matched=).
//! ```
//!
//! A polygon and a line are read as the volumes read their polygons, by the
//! one reader (drawing_shapes.rs, which says what each kind gives): a polygon
//! is a closed polyline or curve polyline (its arcs chorded), one drawn back
//! to its start, a circle or a full ellipse; a line a line, polyline or curve
//! polyline (closed ones are closed breaklines), its levels its
//! `elevation`/`elevations`. FROM with no scope word is the selection. Points
//! are what a surface is built from (geo::surface_input): points and the
//! vertices of levelled lines, a heightless one left out and counted.
//! BOUNDARY, CLIP and an INSIDE take one polygon; HOLE takes each it is given
//! in turn; DELETE ... INSIDE|OUTSIDE takes the union; BREAKLINE puts all its
//! lines in one patch. The verb's own options come before FROM, since
//! everything after FROM is the scope's. A scope that takes nothing is an
//! answer, not a refusal: the scope record says what it took (polygons=0
//! skipped=2), the surface record says changed=no ran=no reason="the scope
//! took no polygon", and there is no undo step (CLAUDE.md 1.1).
//!
//! Words are case-insensitive; a name with blanks is quoted. Every edit is ONE
//! undo step through Document::set_surfaces, named after it
//! (SURFACE_EDIT_FLIP, SURFACE_PASTE ...); an edit that changes nothing is no
//! step. PREVIEW, anywhere after the surface's name, works the edit out and
//! changes nothing - except where a VALUE stands: a surface or layer called
//! "preview" is a name after NAME, INTO, LAYERS or LAYER, and among a MERGE's
//! members. The reply is records, one a line:
//!
//! ```text
//! scope=drawing matched=3 polygons=2 skipped=1      (only for FROM)
//! surface name=ground operation=hole triangles_before=200 triangles_after=188
//!     vertices_before=121 vertices_after=125 points_added=4 points_removed=0
//!     relevelled=0 moved=0 held=0 crossings=4 changed=yes   [preview=yes]
//! ```
//!
//! largest_change= follows for RAISE, SETLEVEL, SCALE, SMOOTH and THIN (the
//! largest change of level; for THIN the bound on the distance from the
//! original). The terrain functions behind each are katana_terrain::tin_edit's.
//!
//! THE TOOLS' SURFACE. The surface the window's edit tools act on is one for
//! the program in the C++ (a process-wide name and observer list), and the
//! program sets and reads it on its one GUI thread; here it is one for the
//! thread (cad_plan.md, risk 4), so tests running on parallel threads never
//! see each other's.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

use katana_core::error::make_error;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, is_ascii_space, lowered, parse_finite_double,
    parse_integer, uppered,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::{Point2, Point3, Polyline2};
use katana_math::cpp;
use katana_terrain::tin_edit::{
    add_breaklines, add_points, change_heights, clip_to_boundary, cut_holes, delete_breaklines,
    delete_points, delete_triangles, edge_at, flip_edge, merge_surfaces, move_point,
    offset_surface, paste_surface, smooth_surface, thin_surface, triangle_at, triangles_by_polygon,
    triangles_longer_than, triangles_thinner_than, vertex_at, vertices_inside,
};
use katana_terrain::{
    BreaklineInput, BreaklineVertex, HeightChange, HeightOperation, HeldPointRule, NamedSurface,
    SmoothOptions, SurfaceStore, TinEditReport, TinEditResult, TinSurface,
};

use crate::document::Document;
use crate::geo::surface_input::surface_input;
use crate::scope_verbs::{ScopeViewProvider, record_value, scope_record};
use crate::terrain::drawing_shapes::{
    DrawingShape, LINE_KINDS, NotAShape, POLYGON_KINDS, ShapeKind, ShapeOptions, ShapeWords,
    TypedCorner, entities_named, parse_typed_corner, read_shape_words, resolve_shapes,
};

fn is(word: &str, keyword: &str) -> bool {
    equals_ignoring_case(word.as_bytes(), keyword.as_bytes())
}

// The C++ `uppered(text)` / `lowered(text)` as text: only ASCII letters change,
// so UTF-8 stays UTF-8.
fn upper(text: &str) -> String {
    String::from_utf8_lossy(&uppered(text)).into_owned()
}

fn lower(text: &str) -> String {
    String::from_utf8_lossy(&lowered(text)).into_owned()
}

fn number(value: f64) -> String {
    format_exact_real(value)
}

// ---- typed coordinates -----------------------------------------------------------------

fn describe(at: Point2) -> String {
    format!("{},{}", number(at.x), number(at.y))
}

fn require_point(words: &[String], at: &mut usize, what: &str) -> Result<TypedCorner> {
    if *at >= words.len() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!("{what} needs a point x,y"),
        ));
    }
    let Some(point) = parse_typed_corner(&words[*at]) else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            format!("{what} needs a point x,y, not this word"),
            words[*at].as_str(),
        ));
    };
    *at += 1;
    Ok(point)
}

fn require_number(words: &[String], at: &mut usize, what: &str) -> Result<f64> {
    if *at >= words.len() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!("{what} needs a number"),
        ));
    }
    let Some(value) = parse_finite_double(&words[*at]) else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            format!("{what} needs a number, not this word"),
            words[*at].as_str(),
        ));
    };
    *at += 1;
    Ok(value)
}

// key=value, the key in any case. None when `word` is not one.
fn option(word: &str) -> Option<(String, String)> {
    let equals = word.find('=')?;
    if equals == 0 {
        return None;
    }
    Some((lower(&word[..equals]), String::from(&word[equals + 1..])))
}

// ---- the drawing's data: <polygon>, <line>, <points> ---------------------------------------
//
// Read by the one reader of the drawing's shapes (drawing_shapes.rs), which the
// volumes read their polygons with too, so an entity is the same polygon to
// both; the replies and refusals here are this family's own words.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataKind {
    Polygon,
    Line,
    Points,
}

fn name_of(kind: DataKind) -> &'static str {
    match kind {
        DataKind::Polygon => "a polygon",
        DataKind::Line => "a line",
        DataKind::Points => "points",
    }
}

fn read_geometry_words(words: &[String], at: &mut usize, kind: DataKind) -> Result<ShapeWords> {
    let expected = format!(
        "{} is #id, coordinates x,y[,z] ... or FROM <scope> [WHERE ...]",
        name_of(kind)
    );
    if *at >= words.len() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!("expected {expected}"),
        ));
    }
    let first = *at;
    let read = read_shape_words(words, at)?;
    if let Some(bad) = &read.bad_id {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "an entity id is # and a number",
            bad.as_str(),
        ));
    }
    if read.empty() {
        return Err(make_error(
            ErrorCode::ParseFailure,
            format!("expected {expected}"),
            words[first].as_str(),
        ));
    }
    Ok(read)
}

#[derive(Clone, Debug, Default)]
struct ResolvedGeometry {
    shapes: Vec<DrawingShape>, // a <polygon>'s or a <line>'s
    points: Vec<Point3>,       // <points>'
    record: String,            // what a FROM scope took; empty otherwise
}

// <points>: typed points, each with its level, or the points and levelled
// vertices of the entities named - what a surface is built from
// (geo::surface_input), not a shape.
fn resolve_points(
    document: &Document,
    words: &ShapeWords,
    views: ScopeViewProvider<'_>,
) -> Result<ResolvedGeometry> {
    let mut resolved = ResolvedGeometry::default();
    if words.scope.is_none() && words.ids.is_empty() {
        for corner in &words.corners {
            let Some(z) = corner.z else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a point added to a surface needs its level: x,y,z",
                    describe(corner.at),
                ));
            };
            resolved
                .points
                .push(Point3::new(corner.at.x, corner.at.y, z));
        }
        return Ok(resolved);
    }
    let named = entities_named(document, words, views)?;
    if let Some(missing) = named.missing {
        return Err(make_error(
            ErrorCode::NotFound,
            "no entity of that id in the drawing",
            format!("#{missing}"),
        ));
    }
    let input = surface_input(document.model(), &named.ids);
    resolved.points.clone_from(&input.input.points);
    for line in &input.input.breaklines {
        resolved.points.extend_from_slice(&line.vertices);
    }
    let skipped: usize = input.skipped.values().sum();
    if let Some(scope) = &named.scope {
        resolved.record = format!(
            "{} points={} skipped={skipped} heightless_vertices={}",
            scope_record(scope),
            resolved.points.len(),
            input.heightless_vertices
        );
    }
    // A scope that takes nothing is an answer, not a refusal (CLAUDE.md 1.1):
    // the record says what it took, and the verb says it ran nothing. Entities
    // NAMED that have no level to give are the person's mistake, and refused.
    if resolved.points.is_empty() && named.scope.is_none() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "nothing taken has a level to add as a point",
            format!("entities={}", named.ids.len()),
        ));
    }
    Ok(resolved)
}

fn resolve_geometry(
    document: &Document,
    words: &ShapeWords,
    kind: DataKind,
    views: ScopeViewProvider<'_>,
) -> Result<ResolvedGeometry> {
    if kind == DataKind::Points {
        return resolve_points(document, words, views);
    }
    let polygon = kind == DataKind::Polygon;
    let taken = resolve_shapes(
        document,
        words,
        if polygon {
            ShapeKind::Polygon
        } else {
            ShapeKind::Line
        },
        views,
        &ShapeOptions::default(),
    )?;
    if let Some(refused) = &taken.refused {
        let Some(entity) = refused.entity else {
            // The count is the distinct corners left once a polygon's closing
            // repeat of its first corner is taken off (the reader's rule, said
            // by the reader), with what was typed beside it: "needs three
            // corners [corners=3]" contradicted itself.
            let counted = format!("corners={} typed={}", refused.corners, words.corners.len());
            return Err(if polygon {
                make_error(
                    ErrorCode::InvalidArgument,
                    "a polygon typed in needs at least three distinct corners",
                    counted,
                )
            } else {
                make_error(
                    ErrorCode::InvalidArgument,
                    "a line typed in needs at least two distinct vertices",
                    counted,
                )
            });
        };
        let id = format!("#{entity}");
        if refused.why == NotAShape::NoEntity {
            return Err(make_error(
                ErrorCode::NotFound,
                "no entity of that id in the drawing",
                id,
            ));
        }
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the entity is not {}",
                if polygon { POLYGON_KINDS } else { LINE_KINDS }
            ),
            id,
        ));
    }
    let mut resolved = ResolvedGeometry {
        shapes: taken.shapes,
        ..ResolvedGeometry::default()
    };
    if let Some(scope) = &taken.scope {
        resolved.record = format!(
            "{}{}{} skipped={}",
            scope_record(scope),
            if polygon { " polygons=" } else { " lines=" },
            resolved.shapes.len(),
            taken.skipped.total()
        );
    }
    // No shape from a FROM scope is an answer: the record says what it took
    // and skipped, and the edit does nothing. (Typed corners and #ids that are
    // no shape were refused above.)
    Ok(resolved)
}

// The one polygon an edit takes.
fn one_polygon(geometry: &ResolvedGeometry, what: &str) -> Result<Polyline2> {
    if geometry.shapes.len() != 1 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{what} takes one polygon"),
            format!("polygons={}", geometry.shapes.len()),
        ));
    }
    Ok(geometry.shapes[0].outline.clone())
}

// ---- the edits --------------------------------------------------------------------------

// What a run of edits did, summed: the first's "before", the last's "after".
#[derive(Clone, Debug, Default)]
struct Outcome {
    surface: TinSurface,
    report: TinEditReport,
    levels: bool, // the edit changes levels: report largest_change
    edges: bool,  // the edit clears breakline edges: report edges_cleared, edges_held
}

fn accumulate(outcome: &mut Outcome, step: TinEditResult, first: bool) {
    let r = step.report;
    if first {
        outcome.report = r;
    } else {
        outcome.report.triangles_after = r.triangles_after;
        outcome.report.vertices_after = r.vertices_after;
        outcome.report.points_added += r.points_added;
        outcome.report.points_removed += r.points_removed;
        outcome.report.points_relevelled += r.points_relevelled;
        outcome.report.points_moved += r.points_moved;
        outcome.report.points_held += r.points_held;
        outcome.report.crossings += r.crossings;
        outcome.report.largest_change = cpp::max(outcome.report.largest_change, r.largest_change);
    }
    outcome.surface = step.surface;
}

fn one(step: Result<TinEditResult>) -> Result<Outcome> {
    let mut outcome = Outcome::default();
    accumulate(&mut outcome, step?, true);
    Ok(outcome)
}

fn nothing(surface: &TinSurface) -> Outcome {
    let mut outcome = Outcome {
        surface: surface.clone(),
        ..Outcome::default()
    };
    outcome.report.triangles_before = surface.triangle_count();
    outcome.report.triangles_after = surface.triangle_count();
    outcome.report.vertices_before = surface.vertex_count();
    outcome.report.vertices_after = surface.vertex_count();
    outcome
}

fn surface_record(
    name: &str,
    operation: &str,
    outcome: &Outcome,
    changed: bool,
    preview: bool,
    not_run: &str,
) -> String {
    let r = &outcome.report;
    let mut record = format!(
        "surface name={} operation={operation} triangles_before={} triangles_after={} \
         vertices_before={} vertices_after={} points_added={} points_removed={} relevelled={} \
         moved={} held={} crossings={}",
        record_value(name),
        r.triangles_before,
        r.triangles_after,
        r.vertices_before,
        r.vertices_after,
        r.points_added,
        r.points_removed,
        r.points_relevelled,
        r.points_moved,
        r.points_held,
        r.crossings
    );
    if outcome.edges {
        record += &format!(
            " edges_cleared={} edges_held={}",
            r.edges_cleared, r.edges_held
        );
    }
    if outcome.levels {
        record += &format!(" largest_change={}", number(r.largest_change));
    }
    record += if changed {
        " changed=yes"
    } else {
        " changed=no"
    };
    if !not_run.is_empty() {
        record += &format!(" ran=no reason={}", record_value(not_run));
    }
    if preview {
        record += " preview=yes";
    }
    record
}

// The surface of that name, with its TIN; NotFound naming the surfaces there
// are.
fn find_surface<'a>(
    document: &'a Document,
    name: &str,
) -> Result<(&'a NamedSurface, &'a TinSurface)> {
    if let Some(named) = document.surfaces().find(name)
        && let Some(surface) = named.surface.as_deref()
    {
        return Ok((named, surface));
    }
    let mut known = String::new();
    for surface in document.surfaces().all() {
        if !known.is_empty() {
            known += ", ";
        }
        known += &surface.name;
    }
    Err(make_error(
        ErrorCode::NotFound,
        "no surface of that name in the drawing",
        if known.is_empty() {
            format!("{name} (the drawing has none)")
        } else {
            format!("{name} (surfaces: {known})")
        },
    ))
}

// Reads key=value options while the words are options; refuses one not in
// `keys`.
fn read_options(words: &[String], at: &mut usize, keys: &[&str]) -> Result<Vec<(String, String)>> {
    let mut found = Vec::new();
    while *at < words.len() {
        let Some(pair) = option(&words[*at]) else {
            break;
        };
        if !keys.contains(&pair.0.as_str()) {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "an option this operation does not take",
                words[*at].as_str(),
            ));
        }
        found.push(pair);
        *at += 1;
    }
    Ok(found)
}

fn option_number(key: &str, value: &str) -> Result<f64> {
    parse_finite_double(value).ok_or_else(|| {
        make_error(
            ErrorCode::ParseFailure,
            format!("{key}= needs a number"),
            format!("{key}={value}"),
        )
    })
}

// reach=<m> after a point, when it is there: the snap aperture of a click,
// which DELETE POINT and MOVE POINT share. `reach` is left as it was when none
// is given.
fn read_reach(words: &[String], at: &mut usize, reach: &mut Option<f64>) -> Status {
    for (key, value) in read_options(words, at, &["reach"])? {
        let parsed = option_number(&key, &value)?;
        if !(parsed >= 0.0) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "reach= cannot be negative",
                value,
            ));
        }
        *reach = Some(parsed);
    }
    Ok(())
}

// The vertex a point names, for DELETE POINT and MOVE POINT: the corner of the
// triangle under it nearest it, or with a reach the nearest vertex within it.
fn vertex_named(surface: &TinSurface, point: Point2, reach: Option<f64>) -> Result<u32> {
    if let Some(vertex) = vertex_at(surface, point, reach) {
        return Ok(vertex);
    }
    // Off the surface is said when the point is, whatever the reach: a reach
    // that found nothing near it on the surface is another thing.
    let on_surface = triangle_at(surface, point).is_some();
    Err(make_error(
        ErrorCode::InvalidArgument,
        if reach.is_none() {
            "the point is off the surface"
        } else if on_surface {
            "no vertex of the surface within reach= of the point"
        } else {
            "the point is off the surface, and no vertex is within reach="
        },
        describe(point),
    ))
}

// INSIDE <polygon>, when it comes next. `took_nothing`: a scope that took no
// polygon - which is NOT "no INSIDE", the whole surface, but nothing to act
// on.
#[derive(Clone, Debug, Default)]
struct InsideRead {
    polygon: Option<Polyline2>,
    took_nothing: bool,
}

fn read_inside(
    document: &Document,
    words: &[String],
    at: &mut usize,
    views: ScopeViewProvider<'_>,
    record: &mut String,
) -> Result<InsideRead> {
    if *at >= words.len() || !is(&words[*at], "INSIDE") {
        return Ok(InsideRead::default());
    }
    *at += 1;
    let geometry_words = read_geometry_words(words, at, DataKind::Polygon)?;
    let geometry = resolve_geometry(document, &geometry_words, DataKind::Polygon, views)?;
    record.clone_from(&geometry.record);
    if geometry.shapes.is_empty() {
        return Ok(InsideRead {
            polygon: None,
            took_nothing: true,
        });
    }
    let polygon = one_polygon(&geometry, "INSIDE")?;
    Ok(InsideRead {
        polygon: Some(polygon),
        took_nothing: false,
    })
}

fn read_geometry(
    document: &Document,
    words: &[String],
    at: &mut usize,
    kind: DataKind,
    views: ScopeViewProvider<'_>,
) -> Result<ResolvedGeometry> {
    let geometry_words = read_geometry_words(words, at, kind)?;
    resolve_geometry(document, &geometry_words, kind, views)
}

fn union_of(sets: &[Vec<u32>]) -> Vec<u32> {
    let all: BTreeSet<u32> = sets.iter().flatten().copied().collect();
    all.into_iter().collect()
}

#[derive(Clone, Debug, Default)]
struct EditOutcome {
    outcome: Outcome,
    operation: String, // as the reply and the step name say it
    record: String,    // what a scope took
    not_run: String,   // why nothing was done: the scope took nothing
}

fn run_edit(
    document: &Document,
    surface: &TinSurface,
    words: &[String],
    at: &mut usize,
    views: ScopeViewProvider<'_>,
) -> Result<EditOutcome> {
    if *at >= words.len() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "SURFACE EDIT <name> needs an operation: DELETE, MOVE, FLIP, ADD, BREAKLINE, \
             BOUNDARY, HOLE, RAISE, LOWER, SETLEVEL, SCALE, SMOOTH or THIN",
        ));
    }
    let mut edit = EditOutcome::default();
    let op = upper(&words[*at]);
    *at += 1;

    if op == "DELETE" {
        if *at >= words.len() {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "DELETE needs TRIANGLE, TRIANGLES, LONG, THIN, POINT, POINTS or BREAKLINE",
            ));
        }
        let what = upper(&words[*at]);
        *at += 1;
        if what == "TRIANGLE" {
            edit.operation = String::from("delete_triangle");
            let point = require_point(words, at, "DELETE TRIANGLE")?;
            let Some(triangle) = triangle_at(surface, point.at) else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "no triangle of the surface there",
                    describe(point.at),
                ));
            };
            edit.outcome = one(delete_triangles(surface, &[triangle]))?;
        } else if what == "TRIANGLES" {
            if *at >= words.len() || !(is(&words[*at], "INSIDE") || is(&words[*at], "OUTSIDE")) {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    "DELETE TRIANGLES needs INSIDE or OUTSIDE",
                ));
            }
            let inside = is(&words[*at], "INSIDE");
            *at += 1;
            edit.operation = String::from(if inside {
                "delete_triangles_inside"
            } else {
                "delete_triangles_outside"
            });
            let geometry = read_geometry(document, words, at, DataKind::Polygon, views)?;
            edit.record.clone_from(&geometry.record);
            if geometry.shapes.is_empty() {
                // OUTSIDE of no polygon is not the whole surface: nothing was
                // named.
                edit.outcome = nothing(surface);
                edit.not_run = String::from("the scope took no polygon");
                return Ok(edit);
            }
            let sets: Vec<Vec<u32>> = geometry
                .shapes
                .iter()
                .map(|shape| triangles_by_polygon(surface, &shape.outline, true))
                .collect();
            let mut chosen = union_of(&sets);
            if !inside {
                let mut outside = Vec::new();
                for t in 0..surface.triangle_count() {
                    let t = t as u32;
                    if chosen.binary_search(&t).is_err() {
                        outside.push(t);
                    }
                }
                chosen = outside;
            }
            if chosen.is_empty() {
                edit.outcome = nothing(surface);
            } else {
                edit.outcome = one(delete_triangles(surface, &chosen))?;
            }
        } else if what == "LONG" || what == "THIN" {
            let is_long = what == "LONG";
            edit.operation = String::from(if is_long {
                "delete_long"
            } else {
                "delete_thin"
            });
            let value = require_number(
                words,
                at,
                if is_long {
                    "DELETE LONG"
                } else {
                    "DELETE THIN"
                },
            )?;
            if !(value > 0.0) || (!is_long && value >= 180.0) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    if is_long {
                        "the length must be more than 0"
                    } else {
                        "the angle must be more than 0 and less than 180 degrees"
                    },
                    number(value),
                ));
            }
            let chosen = if is_long {
                triangles_longer_than(surface, value)
            } else {
                triangles_thinner_than(surface, value * std::f64::consts::PI / 180.0)
            };
            if chosen.is_empty() {
                edit.outcome = nothing(surface);
            } else {
                edit.outcome = one(delete_triangles(surface, &chosen))?;
            }
        } else if what == "POINT" {
            edit.operation = String::from("delete_point");
            let point = require_point(words, at, "DELETE POINT")?;
            let mut reach = None;
            read_reach(words, at, &mut reach)?;
            let vertex = vertex_named(surface, point.at, reach)?;
            edit.outcome = one(delete_points(surface, &[vertex], HeldPointRule::Refuse))?;
        } else if what == "POINTS" {
            if *at >= words.len() || !is(&words[*at], "INSIDE") {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    "DELETE POINTS needs INSIDE <polygon>",
                ));
            }
            *at += 1;
            edit.operation = String::from("delete_points_inside");
            let geometry = read_geometry(document, words, at, DataKind::Polygon, views)?;
            edit.record.clone_from(&geometry.record);
            if geometry.shapes.is_empty() {
                edit.outcome = nothing(surface);
                edit.not_run = String::from("the scope took no polygon");
                return Ok(edit);
            }
            let sets: Vec<Vec<u32>> = geometry
                .shapes
                .iter()
                .map(|shape| vertices_inside(surface, &shape.outline))
                .collect();
            let chosen = union_of(&sets);
            if chosen.is_empty() {
                edit.outcome = nothing(surface);
            } else {
                edit.outcome = one(delete_points(surface, &chosen, HeldPointRule::Skip))?;
            }
        } else if what == "BREAKLINE" {
            // The breakline edges along the lines, made ordinary: the
            // surface's triangles and vertices as they were, the flags gone.
            edit.operation = String::from("delete_breakline");
            edit.outcome.edges = true;
            let geometry = read_geometry(document, words, at, DataKind::Line, views)?;
            edit.record.clone_from(&geometry.record);
            if geometry.shapes.is_empty() {
                edit.outcome = nothing(surface);
                edit.outcome.edges = true;
                edit.not_run = String::from("the scope took no line");
                return Ok(edit);
            }
            let lines: Vec<Polyline2> = geometry
                .shapes
                .iter()
                .map(|shape| shape.outline.clone())
                .collect();
            edit.outcome = one(delete_breaklines(surface, &lines))?;
            edit.outcome.edges = true;
        } else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "DELETE takes TRIANGLE, TRIANGLES, LONG, THIN, POINT, POINTS or BREAKLINE",
                words[*at - 1].as_str(),
            ));
        }
    } else if op == "MOVE" {
        // MOVE POINT x,y TO x2,y2 [reach=<m>]: the vertex under x,y, moved in
        // plan to x2,y2 with its level, as ONE patch (tin_edit::move_point).
        edit.operation = String::from("move_point");
        if *at >= words.len() || !is(&words[*at], "POINT") {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "MOVE needs POINT x,y TO x2,y2",
            ));
        }
        *at += 1;
        let from = require_point(words, at, "MOVE POINT")?;
        if *at >= words.len() || !is(&words[*at], "TO") {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "MOVE POINT x,y needs TO x2,y2",
            ));
        }
        *at += 1;
        let to = require_point(words, at, "MOVE POINT ... TO")?;
        if to.z.is_some() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "MOVE POINT keeps the vertex's level: give the target as x,y",
                words[*at - 1].as_str(),
            ));
        }
        let mut reach = None;
        read_reach(words, at, &mut reach)?;
        let vertex = vertex_named(surface, from.at, reach)?;
        edit.outcome = one(move_point(surface, vertex, to.at))?;
    } else if op == "FLIP" {
        edit.operation = String::from("flip");
        let point = require_point(words, at, "FLIP")?;
        let Some(edge) = edge_at(surface, point.at) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "no triangle of the surface there",
                describe(point.at),
            ));
        };
        edit.outcome = one(flip_edge(surface, edge.triangle, edge.edge))?;
    } else if op == "ADD" {
        if *at >= words.len() || !(is(&words[*at], "POINT") || is(&words[*at], "POINTS")) {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "ADD needs POINT x,y,z ... or POINTS <points>",
            ));
        }
        let typed = is(&words[*at], "POINT");
        *at += 1;
        edit.operation = String::from(if typed { "add_point" } else { "add_points" });
        let mut points: Vec<Point3> = Vec::new();
        if typed {
            while *at < words.len() {
                let Some(point) = parse_typed_corner(&words[*at]) else {
                    break;
                };
                let Some(z) = point.z else {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "a point added to a surface needs its level: x,y,z",
                        words[*at].as_str(),
                    ));
                };
                points.push(Point3::new(point.at.x, point.at.y, z));
                *at += 1;
            }
            if points.is_empty() {
                return Err(Error::new(ErrorCode::ParseFailure, "ADD POINT needs x,y,z"));
            }
        } else {
            let geometry = read_geometry(document, words, at, DataKind::Points, views)?;
            edit.record = geometry.record;
            points = geometry.points;
            if points.is_empty() {
                edit.outcome = nothing(surface);
                edit.not_run = String::from("the scope took no point with a level");
                return Ok(edit);
            }
        }
        edit.outcome = one(add_points(surface, &points))?;
    } else if op == "BREAKLINE" {
        edit.operation = String::from("breakline");
        let geometry = read_geometry(document, words, at, DataKind::Line, views)?;
        edit.record.clone_from(&geometry.record);
        if geometry.shapes.is_empty() {
            edit.outcome = nothing(surface);
            edit.not_run = String::from("the scope took no line");
            return Ok(edit);
        }
        // All the lines in ONE patch: lines of one edit that cross are judged
        // against each other (3D lines at different levels are refused
        // whatever order they are named in), and it is one rebuild, not one per
        // line.
        let lines: Vec<BreaklineInput> = geometry
            .shapes
            .iter()
            .map(|shape| BreaklineInput {
                vertices: shape
                    .outline
                    .vertices
                    .iter()
                    .zip(&shape.heights)
                    .map(|(&position, &z)| BreaklineVertex { position, z })
                    .collect(),
                closed: shape.outline.closed,
            })
            .collect();
        edit.outcome = one(add_breaklines(surface, &lines))?;
    } else if op == "BOUNDARY" || op == "HOLE" {
        let boundary = op == "BOUNDARY";
        edit.operation = String::from(if boundary { "boundary" } else { "hole" });
        let geometry = read_geometry(document, words, at, DataKind::Polygon, views)?;
        edit.record.clone_from(&geometry.record);
        if geometry.shapes.is_empty() {
            edit.outcome = nothing(surface);
            edit.not_run = String::from("the scope took no polygon");
            return Ok(edit);
        }
        if boundary {
            let polygon = one_polygon(&geometry, "BOUNDARY")?;
            edit.outcome = one(clip_to_boundary(surface, &polygon))?;
        } else {
            // Every footprint, in the order the scope gave them: the
            // footprints that can be cut together are cut in one patch, so a
            // scope of 100 buildings is one whole-surface rebuild, not 100.
            let footprints: Vec<Polyline2> = geometry
                .shapes
                .iter()
                .map(|shape| shape.outline.clone())
                .collect();
            edit.outcome = one(cut_holes(surface, &footprints, None))?;
        }
    } else if op == "RAISE" || op == "LOWER" || op == "SETLEVEL" || op == "SCALE" {
        edit.operation = lower(&op);
        let value = require_number(words, at, &op)?;
        let mut change = HeightChange {
            value: if op == "LOWER" { -value } else { value },
            operation: if op == "SETLEVEL" {
                HeightOperation::SetLevel
            } else if op == "SCALE" {
                HeightOperation::Scale
            } else {
                HeightOperation::Raise
            },
            base: None,
        };
        let keys: &[&str] = if op == "SCALE" { &["base"] } else { &[] };
        for (key, text) in read_options(words, at, keys)? {
            change.base = Some(option_number(&key, &text)?);
        }
        let inside = read_inside(document, words, at, views, &mut edit.record)?;
        if inside.took_nothing {
            edit.outcome = nothing(surface);
            edit.outcome.levels = true;
            edit.not_run = String::from("the scope took no polygon");
            return Ok(edit);
        }
        edit.outcome = one(change_heights(surface, &change, inside.polygon.as_ref()))?;
        edit.outcome.levels = true;
    } else if op == "SMOOTH" {
        edit.operation = String::from("smooth");
        let mut smooth = SmoothOptions::default();
        for (key, text) in read_options(words, at, &["passes", "weight"])? {
            if key == "passes" {
                let passes = parse_integer(&text);
                let Some(passes) = passes.filter(|passes| (1..=1000).contains(passes)) else {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "passes= is a whole number from 1 to 1000",
                        text,
                    ));
                };
                smooth.passes = passes as i32;
            } else {
                smooth.weight = option_number(&key, &text)?;
            }
        }
        let inside = read_inside(document, words, at, views, &mut edit.record)?;
        if inside.took_nothing {
            edit.outcome = nothing(surface);
            edit.outcome.levels = true;
            edit.not_run = String::from("the scope took no polygon");
            return Ok(edit);
        }
        edit.outcome = one(smooth_surface(surface, &smooth, inside.polygon.as_ref()))?;
        edit.outcome.levels = true;
    } else if op == "THIN" {
        edit.operation = String::from("thin");
        let tolerance = require_number(words, at, "THIN")?;
        edit.outcome = one(thin_surface(surface, tolerance, None))?;
        edit.outcome.levels = true;
    } else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "SURFACE EDIT takes DELETE, MOVE, FLIP, ADD, BREAKLINE, BOUNDARY, HOLE, RAISE, \
             LOWER, SETLEVEL, SCALE, SMOOTH or THIN",
            words[*at - 1].as_str(),
        ));
    }
    Ok(edit)
}

// NAME <new>, where it is required or allowed; empty when absent.
fn read_name(words: &[String], at: &mut usize, required: bool, verb: &str) -> Result<String> {
    if *at < words.len() && is(&words[*at], "NAME") {
        if *at + 1 >= words.len() || words[*at + 1].is_empty() {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "NAME needs the new surface's name",
            ));
        }
        *at += 2;
        return Ok(words[*at - 1].clone());
    }
    if required {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!("{verb} needs NAME <new>: the surface it makes"),
        ));
    }
    Ok(String::new())
}

fn require_end(words: &[String], at: usize) -> Status {
    if at < words.len() {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "a word the line does not take here",
            words[at].as_str(),
        ));
    }
    Ok(())
}

fn step_name_of(operation: &str) -> String {
    format!("SURFACE_EDIT_{}", upper(operation))
}

// The scope record, and a line break after it, when a scope took something.
fn led_by(record: &str) -> String {
    if record.is_empty() {
        String::new()
    } else {
        format!("{record}\n")
    }
}

/// True for SURFACE EDIT, MERGE, PASTE, CLIP and OFFSET, in any case: the
/// whole line, its first two words. The other SURFACE words are other
/// modules' (the surfaces module's RENAME, STYLE ...; the volumes' DIFFERENCE
/// and ENVELOPE; the geoprocessing executor's LIST, FROM ...).
pub fn is_surface_edit_verb(tokens: &[String]) -> bool {
    if tokens.len() < 2 || !is(&tokens[0], "SURFACE") {
        return false;
    }
    ["EDIT", "MERGE", "PASTE", "CLIP", "OFFSET"]
        .iter()
        .any(|word| is(&tokens[1], word))
}

/// What a surface edit line would do, worked out and not done: the reply, and
/// the drawing's surfaces after it - none when it changes nothing or the line
/// says PREVIEW.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceEditPlan {
    pub reply: String,
    pub surfaces: Option<SurfaceStore>,
    pub step_name: String,
}

/// The plan of one line. `views` answers the scope word VIEW (the window's;
/// None headless, the C++ default).
pub fn plan_surface_edit(
    document: &Document,
    tokens: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<SurfaceEditPlan> {
    if !is_surface_edit_verb(tokens) {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "not a surface edit line",
        ));
    }
    let verb = upper(&tokens[1]);
    // PREVIEW anywhere after the verb's own two words and the surface's name
    // is the preview, wherever a person typed it - except where the grammar
    // wants a VALUE: a surface called Preview (the first surface of every verb
    // is at 2, a paste's base after INTO, a merge's members before NAME), the
    // new surface's NAME, a layer after LAYERS. The words arrive with their
    // quotes already off, so quoting the word cannot protect it; the position
    // does. (The interpreter's MODIFY does the same for a layer called
    // "preview" or "set".)
    const FIRST_FREE: usize = 3;
    let mut words: Vec<String> = Vec::new();
    let mut preview = false;
    let mut past_name = false; // a MERGE's members are the words before NAME
    for (i, token) in tokens.iter().enumerate() {
        let mut is_value = false;
        if i > 0 {
            let before = &tokens[i - 1];
            is_value = is(before, "NAME")
                || is(before, "INTO")
                || is(before, "LAYERS")
                || is(before, "LAYER");
        }
        if verb == "MERGE" && !past_name && document.surfaces().find(token).is_some() {
            is_value = true; // a surface of that name is named, not a flag
        }
        if is(token, "NAME") {
            past_name = true;
        }
        if i >= FIRST_FREE && !is_value && is(token, "PREVIEW") {
            preview = true;
        } else {
            words.push(token.clone());
        }
    }

    let mut store = document.surfaces().clone();
    let mut plan = SurfaceEditPlan::default();
    let mut at: usize = 2;
    if verb == "EDIT" {
        if words.len() < 3 {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "SURFACE EDIT needs a surface's name",
            ));
        }
        let (named, surface) = find_surface(document, &words[2])?;
        let name = named.name.clone();
        at = 3;
        let edit = run_edit(document, surface, &words, &mut at, views)?;
        require_end(&words, at)?;
        let changed = edit.outcome.surface != *surface;
        plan.reply = led_by(&edit.record)
            + &surface_record(
                &name,
                &edit.operation,
                &edit.outcome,
                changed,
                preview,
                &edit.not_run,
            );
        plan.step_name = step_name_of(&edit.operation);
        if changed && !preview {
            store.replace(&name, Some(Arc::new(edit.outcome.surface)))?;
            plan.surfaces = Some(store);
        }
        return Ok(plan);
    }

    let mut outcome = Outcome::default();
    let target: String; // the surface made or changed
    let source: String; // what it was made from, for SURFACE LIST
    let mut record = String::new(); // what a scope took
    let mut replace_in_place = false;
    let operation = lower(&verb);
    if verb == "MERGE" {
        let mut members: Vec<Option<&TinSurface>> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        while at < words.len() && !is(&words[at], "NAME") {
            let (named, surface) = find_surface(document, &words[at])?;
            members.push(Some(surface));
            names.push(named.name.clone());
            at += 1;
        }
        target = read_name(&words, &mut at, true, "SURFACE MERGE")?;
        let merged = merge_surfaces(&members)?;
        accumulate(&mut outcome, merged, true);
        source = format!("merge of {}", names.join(", "));
    } else if verb == "PASTE" {
        if words.len() < 5 || !is(&words[3], "INTO") {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "SURFACE PASTE <top> INTO <base> [NAME <new>]",
            ));
        }
        let (top, top_surface) = find_surface(document, &words[2])?;
        let (base, base_surface) = find_surface(document, &words[4])?;
        at = 5;
        let name = read_name(&words, &mut at, false, "SURFACE PASTE")?;
        let pasted = paste_surface(base_surface, top_surface)?;
        accumulate(&mut outcome, pasted, true);
        replace_in_place = name.is_empty();
        target = if replace_in_place {
            base.name.clone()
        } else {
            name
        };
        source = format!("paste of {} into {}", top.name, base.name);
    } else {
        // CLIP or OFFSET: is_surface_edit_verb took no other word.
        let clip = verb == "CLIP";
        if words.len() < 4 {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                if clip {
                    "SURFACE CLIP <name> <polygon> NAME <new>"
                } else {
                    "SURFACE OFFSET <name> <dz> NAME <new>"
                },
            ));
        }
        let (named, surface) = find_surface(document, &words[2])?;
        at = 3;
        let made: Result<TinEditResult> = if clip {
            let geometry = read_geometry(document, &words, &mut at, DataKind::Polygon, views)?;
            record.clone_from(&geometry.record);
            if geometry.shapes.is_empty() {
                // A scope that takes no polygon makes no surface, and says so.
                let name = read_name(&words, &mut at, true, "SURFACE CLIP")?;
                require_end(&words, at)?;
                plan.reply = format!(
                    "{}surface name={} operation=clip changed=no ran=no reason={}{}",
                    led_by(&record),
                    record_value(&name),
                    record_value("the scope took no polygon"),
                    if preview { " preview=yes" } else { "" }
                );
                plan.step_name = String::from("SURFACE_CLIP");
                return Ok(plan);
            }
            let polygon = one_polygon(&geometry, "SURFACE CLIP")?;
            source = format!("clip of {}", named.name);
            clip_to_boundary(surface, &polygon)
        } else {
            let dz = require_number(&words, &mut at, "SURFACE OFFSET")?;
            source = format!("offset of {} by {}", named.name, number(dz));
            outcome.levels = true;
            offset_surface(surface, dz)
        };
        // The name is read, and refused, before what the edit made is.
        let name = read_name(
            &words,
            &mut at,
            true,
            if clip {
                "SURFACE CLIP"
            } else {
                "SURFACE OFFSET"
            },
        )?;
        accumulate(&mut outcome, made?, true);
        target = name;
    }
    require_end(&words, at)?;
    if !replace_in_place && store.find(&target).is_some() {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "a surface of that name is already in the drawing",
            target,
        ));
    }
    plan.reply =
        led_by(&record) + &surface_record(&target, &operation, &outcome, true, preview, "");
    plan.step_name = format!("SURFACE_{verb}");
    if preview {
        return Ok(plan);
    }
    let shared = Arc::new(outcome.surface);
    if replace_in_place {
        store.replace(&target, Some(shared))?;
    } else {
        store.add(NamedSurface {
            name: target,
            surface: Some(shared),
            source,
            ..NamedSurface::default()
        })?;
    }
    plan.surfaces = Some(store);
    Ok(plan)
}

/// Runs one line: `tokens` is the whole line split into words with the quotes
/// removed (the interpreter's tokenize). The plan, committed as one undo step.
/// Fails, changing nothing, with
///
/// ```text
/// ParseFailure     a word the grammar does not take, naming it;
/// NotFound         a surface or an entity (#id) the drawing has not;
/// AlreadyExists    a NAME already in use;
/// InvalidArgument  an entity of the wrong kind, typed corners that are no
///                  polygon or line, a number out of range;
/// ```
///
/// and the terrain edit's own refusal (tin_edit), with what was asked.
pub fn run_surface_edit_verb(
    document: &mut Document,
    tokens: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let plan = plan_surface_edit(document, tokens, views)?;
    if let Some(surfaces) = plan.surfaces {
        document.set_surfaces(surfaces, plan.step_name)?;
    }
    Ok(plan.reply)
}

/// Two lines for the interpreter's own HELP.
pub fn surface_edit_help_summary() -> String {
    String::from(
        "Surfaces  SURFACE EDIT name DELETE TRIANGLE|POINT x,y | DELETE TRIANGLES INSIDE|OUTSIDE poly
          | DELETE LONG m | DELETE THIN deg | DELETE POINTS INSIDE poly | DELETE BREAKLINE line
          | MOVE POINT x,y TO x,y | FLIP x,y | ADD POINT
          x,y,z ... | ADD POINTS pts | BREAKLINE line | BOUNDARY poly | HOLE poly | RAISE|LOWER
          dz | SETLEVEL z | SCALE f [base=z] [INSIDE poly] | SMOOTH [passes=] [weight=] [INSIDE
          poly] | THIN tol    SURFACE MERGE a b ... NAME new | PASTE top INTO base [NAME new]
          | CLIP name poly NAME new | OFFSET name dz NAME new   one undo step each, PREVIEW;
          poly/line/pts: #id ... | x,y[,z] ... | FROM scope [WHERE ...] (HELP SURFACE EDIT)
",
    )
}

/// Every word, for HELP SURFACE EDIT.
pub fn surface_edit_verb_help() -> String {
    String::from(
        r#"Surface edits (docs/surface_editing.md). Words are case-insensitive; quote a
name with blanks. Every edit is ONE undo step; PREVIEW (anywhere after the name)
works it out and changes nothing; an edit that changes nothing is no step.

SURFACE EDIT <name> DELETE TRIANGLE x,y            the triangle under x,y
SURFACE EDIT <name> DELETE TRIANGLES INSIDE|OUTSIDE <polygon>
                                                   by their centroids; several polygons: their union
SURFACE EDIT <name> DELETE LONG <length>           triangles with a side longer than length
SURFACE EDIT <name> DELETE THIN <degrees>          triangles with an angle below degrees
SURFACE EDIT <name> DELETE POINT x,y [reach=<m>]   the vertex of the triangle under x,y nearest
                                                   it (within reach); not one on a breakline
SURFACE EDIT <name> DELETE POINTS INSIDE <polygon> breakline and boundary vertices are held
SURFACE EDIT <name> DELETE BREAKLINE <line>        the breakline edges along the line become
                                                   ordinary edges; the triangles are as they were
SURFACE EDIT <name> MOVE POINT x,y TO x2,y2 [reach=<m>]
                                                   the vertex under x,y to x2,y2, keeping its level;
                                                   not one on a breakline or the rim, not off the surface
SURFACE EDIT <name> FLIP x,y                       the edge nearest x,y of the triangle under it
SURFACE EDIT <name> ADD POINT x,y,z [x,y,z ...]    off the surface, a point extends it
SURFACE EDIT <name> ADD POINTS <points>            points and levelled vertices of the entities
SURFACE EDIT <name> BREAKLINE <line>               3D where a vertex has a level, draped where not
SURFACE EDIT <name> BOUNDARY <polygon>             keep what is inside, cut exactly
SURFACE EDIT <name> HOLE <polygon>                 remove what is inside, cut exactly
SURFACE EDIT <name> RAISE|LOWER <dz> [INSIDE <polygon>]
SURFACE EDIT <name> SETLEVEL <z> [INSIDE <polygon>]
SURFACE EDIT <name> SCALE <factor> [base=<z>] [INSIDE <polygon>]
                                                   about base, else the lowest level changed
SURFACE EDIT <name> SMOOTH [passes=3] [weight=0.5] [INSIDE <polygon>]
                                                   rim and breakline vertices held
SURFACE EDIT <name> THIN <tolerance>               remove vertices that move the surface by no
                                                   more than tolerance anywhere
SURFACE MERGE <a> <b> [...] NAME <new>             later surfaces win where they reach
SURFACE PASTE <top> INTO <base> [NAME <new>]       the top in the base with a clean seam; in
                                                   place unless NAME
SURFACE CLIP <name> <polygon> NAME <new>           a copy of the part inside the polygon
SURFACE OFFSET <name> <dz> NAME <new>              a copy raised by dz

<polygon>, <line>, <points>:  #id [#id ...]  |  x,y[,z] x,y[,z] ...  |  FROM <scope> [WHERE k=v ...]
  (the scope words of HELP: SELECTION | DRAWING | VIEW [id] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY];
  options come before FROM). A scope that takes nothing is reported, not refused:
  changed=no ran=no reason="the scope took no polygon", and no undo step.

Reply: [scope=... matched=N polygons=|lines=|points= skipped=]
       surface name= operation= triangles_before= triangles_after= vertices_before=
       vertices_after= points_added= points_removed= relevelled= moved= held= crossings=
       [largest_change=] changed=yes|no [preview=yes]
"#,
    )
}

// ---- the tools' surface -------------------------------------------------------------------
//
// The Surface Edit toolbar's choice, or a tool's Surface option: one for the
// program, as the edit tools' EditDefaults are, and not the drawing's - which
// surface a person is editing is not part of the drawing. The program uses it
// on its one GUI thread, so here it is one for the thread (cad_plan.md, risk
// 4): parallel tests each have their own.

type Observer = Rc<dyn Fn()>;

thread_local! {
    // One for the program, as the edit tools' EditDefaults are (docs/tools.md).
    static TARGET_NAME: RefCell<String> = const { RefCell::new(String::new()) };
    static OBSERVERS: RefCell<Vec<(usize, Observer)>> = const { RefCell::new(Vec::new()) };
    static NEXT_OBSERVER: Cell<usize> = const { Cell::new(0) };
}

/// The surface the tools are pointed at by name; empty: none chosen. A copy:
/// the C++ hands out a reference to the one name.
pub fn surface_edit_target() -> String {
    TARGET_NAME.with(|name| name.borrow().clone())
}

/// Points the tools at `name`, and tells every observer.
pub fn set_surface_edit_target(name: String) {
    TARGET_NAME.with(|target| *target.borrow_mut() = name);
    // A copy: an observer may add or remove observers.
    let notify: Vec<Observer> = OBSERVERS.with(|all| {
        all.borrow()
            .iter()
            .map(|(_, observer)| Rc::clone(observer))
            .collect()
    });
    for observer in notify {
        observer();
    }
}

/// A function called (on the thread that sets it) whenever the target is set,
/// so the Surface Edit toolbar's list follows a tool's own Surface option
/// instead of keeping the old name until the drawing's surfaces next change.
/// The handle removes it again.
pub fn add_surface_edit_target_observer(observer: impl Fn() + 'static) -> usize {
    let handle = NEXT_OBSERVER.with(|next| {
        next.set(next.get() + 1);
        next.get()
    });
    let observer: Observer = Rc::new(observer);
    OBSERVERS.with(|all| all.borrow_mut().push((handle, observer)));
    handle
}

pub fn remove_surface_edit_target_observer(handle: usize) {
    OBSERVERS.with(|all| all.borrow_mut().retain(|(each, _)| *each != handle));
}

/// The surface a tool edits in `document`: the chosen one when the drawing
/// has it, else the first visible surface, else the first. NotFound when the
/// drawing has no surface at all.
pub fn active_edit_surface(document: &Document) -> Result<String> {
    let store = document.surfaces();
    let target = surface_edit_target();
    if !target.is_empty()
        && let Some(named) = store.find(&target)
    {
        return Ok(named.name.clone());
    }
    if let Some(surface) = store.all().iter().find(|surface| surface.visible) {
        return Ok(surface.name.clone());
    }
    if let Some(first) = store.all().first() {
        return Ok(first.name.clone());
    }
    Err(Error::new(
        ErrorCode::NotFound,
        "the drawing has no surface to edit: make one first (Terrain > Surface From Drawing, \
         or SURFACE FROM)",
    ))
}

/// `name` as a line writes it: quoted when it has a blank or is empty.
pub fn surface_name_word(name: &str) -> String {
    let blank = name.is_empty() || name.bytes().any(is_ascii_space);
    if blank {
        format!("\"{name}\"")
    } else {
        String::from(name)
    }
}
