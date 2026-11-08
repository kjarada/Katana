//! Grips: the handles a CAD user drags to reshape what is selected, with no
//! tool running (include/katana/cad/drawing/grips.hpp; docs/drawing.md,
//! "Grips"). The owner's request was vertex control, and the grip is its most
//! direct form: click a vertex of the selected string, drag it where it goes,
//! and that is one undo step.
//!
//! Everything a grip does is decided HERE, headless and per geometry kind -
//! which grips an entity offers (grips_of) and what dragging a set of them
//! does (apply_grip_drag, grip_drag_command) - so it is unit tested per kind
//! with no view anywhere. The plan view only draws the grips it is given,
//! finds the one under the cursor, and forwards the drag.
//!
//! A drag moves the grabbed grip to a TARGET point (already snapped, tracked
//! or typed by the view) and every other hot grip by the same displacement,
//! so several vertices picked with Shift move together. What "move" means is
//! the grip's kind:
//!
//! ```text
//! Vertex            the polyline's vertex moves; each arc keeps its bulge
//! SegmentMid        a straight segment moves bodily (both ends: a stretch),
//!                   an arc segment is reshaped through the target; with
//!                   insert_vertex (Ctrl) a new vertex is made there instead
//! End / Mid         a line's end moves, its middle moves it whole; an arc's
//!                   end or middle reshapes it through the other two points
//! Centre            a circle, arc or ellipse moves whole
//! Quadrant          a circle's radius becomes the distance to the target;
//!                   an ellipse's axis end sets that axis
//! Insertion         a point or a text moves
//! FitPoint          a spline's fit point moves and the spline is re-solved
//! ControlPoint      a spline's control point moves (a spline with no fit
//!                   points)
//! LeaderVertex      a leader's free tip, bend or note end moves
//! ```

use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{ChangeSet, ChangeSetCommand, CommandContext, CommandPtr};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::to_string;
use katana_entity::{Entity, EntityId, EnumName, Geometry, PointGeometry, validate};
use katana_geometry::polyline_vertices::{delete_vertices, insert_vertex, segment_to_arc};
use katana_geometry::{Arc2, Ellipse2, Point2, Segment2, Spline2, Vec2};
use katana_math::numerics::{HALF_PI, normalize_angle, tolerance};

use crate::annotation::leader_edit::{LeaderVertexMove, move_leader_vertices};
use crate::document::Document;
use crate::drawing::vertex_editing::{is_polyline_entity, read_polyline, write_polyline};
use crate::layer_overrides::LayerOverrides;
use crate::selection::is_selectable;
use crate::tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};

/// How many grips a selection offers at most (the C++ default limit of
/// gripsOfSelection): past it the view shows none, as CAD programs do past
/// their grip limit.
pub const DEFAULT_GRIP_LIMIT: usize = 20_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GripKind {
    #[default]
    Vertex,
    SegmentMid,
    End,
    Mid,
    Centre,
    Quadrant,
    Insertion,
    FitPoint,
    ControlPoint,
    LeaderVertex,
}

/// The C++ `toString(GripKind)`.
impl EnumName for GripKind {
    fn name(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::SegmentMid => "segment middle",
            Self::End => "end",
            Self::Mid => "middle",
            Self::Centre => "centre",
            Self::Quadrant => "quadrant",
            Self::Insertion => "insertion point",
            Self::FitPoint => "fit point",
            Self::ControlPoint => "control point",
            Self::LeaderVertex => "leader point",
        }
    }
}

/// The default is no entity's (INVALID_ENTITY_ID, 0), a vertex grip 0 at the origin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grip {
    pub entity: EntityId,
    pub kind: GripKind,
    /// Which one: a vertex, a segment, an end (0 start, 1 end), a quadrant
    /// (0 east of the axis, counter-clockwise), a fit or control point.
    pub index: usize,
    pub position: Point2,
}

impl Grip {
    /// The same handle (entity, kind, index), whatever its position.
    pub fn same_handle(&self, other: &Grip) -> bool {
        self.entity == other.entity && self.kind == other.kind && self.index == other.index
    }
}

// ---- enumeration ----------------------------------------------------------------------

/// The grips an entity offers, in a fixed order (vertices first, then
/// segment middles, and so on), each with its position.
pub fn grips_of(entity: &Entity) -> Vec<Grip> {
    let mut out = Vec::new();
    let mut add = |kind: GripKind, index: usize, at: Point2| {
        out.push(Grip {
            entity: entity.id,
            kind,
            index,
            position: at,
        });
    };
    match &entity.geometry {
        Geometry::Point(shape) => add(GripKind::Insertion, 0, shape.position),
        Geometry::Line(shape) => {
            add(GripKind::End, 0, shape.start);
            add(GripKind::End, 1, shape.end);
            add(GripKind::Mid, 0, shape.midpoint());
        }
        Geometry::Arc(shape) => {
            add(GripKind::End, 0, shape.start_point());
            add(GripKind::End, 1, shape.end_point());
            add(GripKind::Mid, 0, shape.midpoint());
            add(GripKind::Centre, 0, shape.center);
        }
        Geometry::Circle(shape) => {
            add(GripKind::Centre, 0, shape.center);
            for k in 0..4 {
                add(
                    GripKind::Quadrant,
                    k,
                    shape.point_at_angle(HALF_PI * k as f64),
                );
            }
        }
        Geometry::Polyline(_) | Geometry::CurvePolyline(_) => {
            if let Some(polyline) = read_polyline(entity) {
                for (i, vertex) in polyline.vertices.iter().enumerate() {
                    add(GripKind::Vertex, i, vertex.position);
                }
                for i in 0..polyline.segment_count() {
                    add(GripKind::SegmentMid, i, polyline.segment(i).point_at(0.5));
                }
            }
        }
        Geometry::Text(shape) => add(GripKind::Insertion, 0, shape.position),
        Geometry::Ellipse(shape) => {
            add(GripKind::Centre, 0, shape.center);
            for k in 0..4 {
                let t = HALF_PI * k as f64;
                if shape.contains_parameter_with_tolerance(t, tolerance::ANGULAR) {
                    add(GripKind::Quadrant, k, shape.point_at_parameter(t));
                }
            }
            if !shape.is_full() {
                add(GripKind::End, 0, shape.start_point());
                add(GripKind::End, 1, shape.end_point());
            }
        }
        Geometry::Spline(shape) => {
            let points = if shape.has_fit_points() {
                (GripKind::FitPoint, &shape.fit_points)
            } else {
                (GripKind::ControlPoint, &shape.control_points)
            };
            for (i, &point) in points.1.iter().enumerate() {
                add(points.0, i, point);
            }
        }
        Geometry::Leader(shape) => {
            let first = usize::from(shape.tip_ref.associated());
            for (i, &vertex) in shape.vertices.iter().enumerate().skip(first) {
                add(GripKind::LeaderVertex, i, vertex);
            }
        }
        // Dimensions and labels are the annotation system's to give handles
        // to (docs/tools.md); they offer none here.
        Geometry::Dimension(_) | Geometry::Label(_) => {}
    }
    out
}

/// The grips of every entity in `ids` that `view` lets be edited (drawn
/// there, on an unlocked layer: is_selectable), capped at `limit` in all
/// (DEFAULT_GRIP_LIMIT is the C++ default): a selection of every entity of a
/// large survey would otherwise draw a million squares - past the cap the
/// view shows none, as CAD programs do past their grip limit. A selected
/// entity on a layer `view` hides offers no grip there: that view shows it as
/// a ghost at most (docs/desktop.md, "The selection in every view"), which is
/// never picked - a grip on it dragged a design line from the as-built view
/// that hid it. `view` has no default, as is_selectable's has none:
/// NO_LAYER_OVERRIDES for the document's rule alone.
pub fn grips_of_selection(
    document: &Document,
    ids: &[EntityId],
    view: &LayerOverrides,
    limit: usize,
) -> Vec<Grip> {
    let mut out = Vec::new();
    for &id in ids {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        if !is_selectable(document.model(), entity, view) {
            continue;
        }
        let grips = grips_of(entity);
        if out.len() + grips.len() > limit {
            return Vec::new();
        }
        out.extend(grips);
    }
    out
}

/// The grip within `aperture` of `at`, nearest first; None when none.
pub fn grip_at(grips: &[Grip], at: Point2, aperture: f64) -> Option<Grip> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for grip in grips {
        let d = grip.position.distance_to(at);
        if d <= aperture && d < best_distance {
            best_distance = d;
            best = Some(*grip);
        }
    }
    best
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GripDrag {
    /// The grip the drag started on.
    pub grabbed: Grip,
    /// Where it goes.
    pub target: Point2,
    /// Other hot grips, moved by the same displacement.
    pub also_hot: Vec<Grip>,
    /// Ctrl on a segment middle: insert, do not stretch.
    pub insert_vertex: bool,
}

// ---- editing ---------------------------------------------------------------------------

// Each of this entity's hot grips with the displacement the drag gives it: the
// grabbed one goes to the target, the rest move as far as it does.
struct Moves {
    grips: Vec<(Grip, Vec2)>,
}

impl Moves {
    fn of(&self, kind: GripKind, index: usize) -> Vec2 {
        self.grips
            .iter()
            .find(|(grip, _)| grip.kind == kind && grip.index == index)
            .map_or(Vec2::default(), |(_, delta)| *delta)
    }

    fn has(&self, kind: GripKind, index: usize) -> bool {
        self.grips
            .iter()
            .any(|(grip, _)| grip.kind == kind && grip.index == index)
    }
}

fn moves_for(id: EntityId, drag: &GripDrag) -> Moves {
    let mut moves = Moves { grips: Vec::new() };
    let delta = drag.target - drag.grabbed.position;
    let mut push = |grip: &Grip| {
        if grip.entity != id {
            return;
        }
        if moves.grips.iter().any(|(item, _)| item.same_handle(grip)) {
            return;
        }
        moves.grips.push((*grip, delta));
    };
    push(&drag.grabbed);
    for grip in &drag.also_hot {
        push(grip);
    }
    moves
}

fn refused(why: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidArgument, why)
}

fn drag_polyline(entity: &Entity, drag: &GripDrag, moves: &Moves) -> Result<Entity> {
    let Some(mut polyline) = read_polyline(entity) else {
        return Err(refused("this kind of object has no grips to drag"));
    };
    if drag.insert_vertex
        && drag.grabbed.entity == entity.id
        && drag.grabbed.kind == GripKind::SegmentMid
    {
        let inserted = insert_vertex(&polyline, drag.grabbed.index, drag.target, None)?;
        return write_polyline(entity.clone(), &inserted);
    }
    // Every vertex moves once, however many hot grips name it (its own and
    // those of the two segments either side).
    let mut vertex_moves: BTreeMap<usize, Vec2> = BTreeMap::new();
    let mut arc_mids: Vec<(usize, Vec2)> = Vec::new();
    for &(grip, delta) in &moves.grips {
        if grip.kind == GripKind::Vertex && grip.index < polyline.vertices.len() {
            vertex_moves.insert(grip.index, delta);
        } else if grip.kind == GripKind::SegmentMid && grip.index < polyline.segment_count() {
            if polyline.is_arc(grip.index) {
                arc_mids.push((grip.index, delta));
            } else {
                vertex_moves.entry(grip.index).or_insert(delta);
                vertex_moves
                    .entry(polyline.segment_end(grip.index))
                    .or_insert(delta);
            }
        }
    }
    // The arcs' middles as they were, before any vertex moves.
    let through_points: Vec<(usize, Point2)> = arc_mids
        .iter()
        .map(|&(segment, delta)| (segment, polyline.segment(segment).point_at(0.5) + delta))
        .collect();
    for (&index, &delta) in &vertex_moves {
        polyline.vertices[index].position += delta;
    }
    for &(segment, through) in &through_points {
        let Ok(reshaped) = segment_to_arc(&polyline, segment, through) else {
            return Err(refused(
                "the arc cannot pass through that point: it is in line with the segment's ends",
            ));
        };
        polyline = reshaped;
    }
    if validate(&Geometry::CurvePolyline(polyline.clone())).is_err() {
        return Err(refused("the polyline would have no length"));
    }
    write_polyline(entity.clone(), &polyline)
}

fn drag_ellipse(mut ellipse: Ellipse2, moves: &Moves) -> Result<Geometry> {
    if moves.has(GripKind::Centre, 0) {
        ellipse.center += moves.of(GripKind::Centre, 0);
        return Ok(Geometry::Ellipse(ellipse));
    }
    let c = ellipse.center;
    for k in 0..4_usize {
        if !moves.has(GripKind::Quadrant, k) {
            continue;
        }
        let p = ellipse.point_at_parameter(HALF_PI * k as f64) + moves.of(GripKind::Quadrant, k);
        let major = ellipse.major_radius();
        let minor = ellipse.minor_radius();
        if k.is_multiple_of(2) {
            let axis = (p - c) * (if k == 0 { 1.0 } else { -1.0 });
            let length = axis.length();
            if !(length > tolerance::GEOMETRIC) {
                return Err(refused("an axis end cannot be dragged onto the centre"));
            }
            if minor <= length {
                ellipse.major_axis = axis;
                ellipse.ratio = minor / length;
            } else if ellipse.is_full() {
                ellipse.major_axis = axis.perpendicular().normalized() * minor;
                ellipse.ratio = length / minor;
            } else {
                return Err(refused(
                    "on an elliptical arc the major axis cannot be made shorter than the minor \
                     one",
                ));
            }
        } else {
            let unit_minor = ellipse.major_axis.perpendicular() / major;
            let length = (p - c).dot(unit_minor).abs();
            if !(length > tolerance::GEOMETRIC) {
                return Err(refused("the minor axis cannot be dragged to nothing"));
            }
            if length <= major {
                ellipse.ratio = length / major;
            } else if ellipse.is_full() {
                ellipse.major_axis = unit_minor * length;
                ellipse.ratio = major / length;
            } else {
                return Err(refused(
                    "on an elliptical arc the minor axis cannot be made longer than the major one",
                ));
            }
        }
    }
    if !ellipse.is_full() {
        let mut start = ellipse.start_parameter;
        let mut end = ellipse.end_parameter();
        if moves.has(GripKind::End, 0) {
            start = ellipse.parameter_towards(ellipse.start_point() + moves.of(GripKind::End, 0));
        }
        if moves.has(GripKind::End, 1) {
            end = ellipse.parameter_towards(ellipse.end_point() + moves.of(GripKind::End, 1));
        }
        let sweep = normalize_angle(end - start);
        if !(sweep > tolerance::ANGULAR) {
            return Err(refused("the elliptical arc's ends would meet"));
        }
        ellipse.start_parameter = start;
        ellipse.sweep = sweep;
    }
    Ok(Geometry::Ellipse(ellipse))
}

/// `entity` as the drag leaves it, for the grips of `drag` that belong to it
/// (others are ignored). Fails with a sentence when the result would not be
/// valid geometry (a circle of no radius, a polyline folded onto itself).
pub fn apply_grip_drag(entity: &Entity, drag: &GripDrag) -> Result<Entity> {
    let moves = moves_for(entity.id, drag);
    if moves.grips.is_empty() {
        return Ok(entity.clone());
    }
    if is_polyline_entity(entity) {
        return drag_polyline(entity, drag, &moves);
    }
    let geometry = match &entity.geometry {
        Geometry::Point(shape) => Geometry::Point(PointGeometry {
            position: shape.position + moves.of(GripKind::Insertion, 0),
        }),
        Geometry::Line(shape) => {
            let whole = moves.of(GripKind::Mid, 0);
            Geometry::Line(Segment2::new(
                shape.start + moves.of(GripKind::End, 0) + whole,
                shape.end + moves.of(GripKind::End, 1) + whole,
            ))
        }
        Geometry::Arc(shape) => {
            if moves.has(GripKind::Centre, 0) {
                let mut moved = *shape;
                moved.center += moves.of(GripKind::Centre, 0);
                Geometry::Arc(moved)
            } else {
                let Some(rebuilt) = Arc2::through_points(
                    shape.start_point() + moves.of(GripKind::End, 0),
                    shape.midpoint() + moves.of(GripKind::Mid, 0),
                    shape.end_point() + moves.of(GripKind::End, 1),
                ) else {
                    return Err(refused("the arc's ends and middle would be in line"));
                };
                Geometry::Arc(rebuilt)
            }
        }
        Geometry::Circle(shape) => {
            let mut moved = *shape;
            moved.center += moves.of(GripKind::Centre, 0);
            for k in 0..4_usize {
                if moves.has(GripKind::Quadrant, k) {
                    let p =
                        shape.point_at_angle(HALF_PI * k as f64) + moves.of(GripKind::Quadrant, k);
                    moved.radius = p.distance_to(moved.center);
                }
            }
            Geometry::Circle(moved)
        }
        Geometry::Text(shape) => {
            let mut moved = shape.clone();
            moved.position += moves.of(GripKind::Insertion, 0);
            Geometry::Text(moved)
        }
        Geometry::Leader(shape) => {
            let mut points = Vec::new();
            for &(grip, delta) in &moves.grips {
                if grip.kind != GripKind::LeaderVertex || grip.index >= shape.vertices.len() {
                    return Err(refused("this is not a leader point grip"));
                }
                points.push(LeaderVertexMove {
                    index: grip.index,
                    position: shape.vertices[grip.index] + delta,
                });
            }
            Geometry::Leader(move_leader_vertices(shape, &points)?)
        }
        Geometry::Ellipse(shape) => drag_ellipse(*shape, &moves)?,
        Geometry::Spline(shape) => {
            let fit = shape.has_fit_points();
            let mut points = if fit {
                shape.fit_points.clone()
            } else {
                shape.control_points.clone()
            };
            let kind = if fit {
                GripKind::FitPoint
            } else {
                GripKind::ControlPoint
            };
            for (i, point) in points.iter_mut().enumerate() {
                *point += moves.of(kind, i);
            }
            if fit {
                Geometry::Spline(Spline2::through_points(points, shape.degree)?)
            } else {
                let mut moved = shape.clone();
                moved.control_points = points;
                Geometry::Spline(moved)
            }
        }
        _ => return Err(refused("this kind of object has no grips to drag")),
    };
    if let Err(error) = validate(&geometry) {
        return Err(refused(format!(
            "the drag would leave {} {} degenerate: {}",
            to_string(entity.r#type()),
            entity.id,
            error.message
        )));
    }
    let mut out = entity.clone();
    out.geometry = geometry;
    Ok(out)
}

fn touched(drag: &GripDrag) -> Vec<EntityId> {
    let mut ids = BTreeSet::new();
    ids.insert(drag.grabbed.entity);
    for grip in &drag.also_hot {
        ids.insert(grip.entity);
    }
    ids.into_iter().collect()
}

/// The whole drag as ONE command (GRIP_EDIT) over every entity it touches.
pub fn grip_drag_command(drag: GripDrag) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        "GRIP_EDIT",
        move |context: &CommandContext<'_>| {
            let model = &*context.model;
            let mut changes = ChangeSet::default();
            for id in touched(&drag) {
                let Some(entity) = model.entities.find(id) else {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        format!("there is no entity {id}"),
                    ));
                };
                changes.modify.push(apply_grip_drag(entity, &drag)?);
            }
            Ok(changes)
        },
        false,
    ))
}

/// Delete on hot vertex grips: removes those vertices, as ONE command
/// (VERTEX_DELETE). Refused when no hot grip is a vertex, or a polyline would
/// be left with too few.
pub fn delete_hot_vertices(hot: Vec<Grip>) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        "VERTEX_DELETE",
        move |context: &CommandContext<'_>| {
            let model = &*context.model;
            let mut by_entity: BTreeMap<EntityId, Vec<usize>> = BTreeMap::new();
            for grip in &hot {
                if grip.kind == GripKind::Vertex {
                    by_entity.entry(grip.entity).or_default().push(grip.index);
                }
            }
            if by_entity.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "no vertex grip is hot; click a polyline's vertex grip first",
                ));
            }
            let mut changes = ChangeSet::default();
            for (&id, indices) in &by_entity {
                let Some(entity) = model.entities.find(id) else {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        format!("there is no entity {id}"),
                    ));
                };
                let Some(polyline) = read_polyline(entity) else {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        format!("entity {id} is not a polyline"),
                    ));
                };
                let remaining = delete_vertices(&polyline, indices.clone())?;
                changes
                    .modify
                    .push(write_polyline(entity.clone(), &remaining)?);
            }
            Ok(changes)
        },
        false,
    ))
}

/// What the drag would draw: the edited geometry of every touched entity,
/// for the view's rubber band. Entities the drag cannot edit are left out.
pub fn grip_preview(document: &Document, drag: &GripDrag) -> Vec<Geometry> {
    let mut out = Vec::new();
    for id in touched(drag) {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        if let Ok(edited) = apply_grip_drag(entity, drag) {
            out.push(edited.geometry);
        }
    }
    out
}

/// The drag as a tool's preview is (tool_feedback.rs), so a grip and a tool
/// speak one language: grip_preview's geometry as the ghost, the grabbed grip
/// at its old place as the Target, and for a Ctrl-insert the new vertex as
/// Added - read from the edited polyline, so on an arc it is on the arc.
pub fn grip_feedback(document: &Document, drag: &GripDrag) -> ToolFeedback {
    let mut feedback = ToolFeedback {
        shapes: grip_preview(document, drag),
        ..ToolFeedback::default()
    };
    feedback.marks.push(FeedbackMark {
        role: FeedbackRole::Target,
        geometry: Geometry::Point(PointGeometry {
            position: drag.grabbed.position,
        }),
        ..FeedbackMark::default()
    });
    if drag.insert_vertex && drag.grabbed.kind == GripKind::SegmentMid {
        // The vertex a Ctrl-drag adds is the one after the segment's start in
        // the edited polyline (drag_polyline's insert_vertex).
        if let Some(entity) = document.model().entities.find(drag.grabbed.entity)
            && let Ok(edited) = apply_grip_drag(entity, drag)
            && let Some(polyline) = read_polyline(&edited)
            && drag.grabbed.index + 1 < polyline.vertices.len()
        {
            feedback.marks.push(FeedbackMark {
                role: FeedbackRole::Added,
                geometry: Geometry::Point(PointGeometry {
                    position: polyline.vertices[drag.grabbed.index + 1].position,
                }),
                ..FeedbackMark::default()
            });
        }
    }
    feedback
}
