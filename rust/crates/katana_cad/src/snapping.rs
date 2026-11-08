//! Object snapping (PLAN.MD Phases 08/09; include/katana/cad/snapping.hpp):
//! endpoint, midpoint, centre, intersection, perpendicular, tangent, nearest
//! and grid.
//!
//! Resolution order: the closest candidate among the "exact" modes (endpoint,
//! midpoint, centre, intersection, perpendicular, tangent, and the drawing
//! system's node, quadrant and apparent intersection) wins; ties prefer the
//! order just listed. Only if none is inside the aperture does Nearest apply,
//! then Extension and Parallel (points on lines that are not drawn), and only
//! then Grid. This mirrors how drafters expect snaps to behave: an endpoint
//! always beats merely being somewhere on the line.

use std::ops::BitOr;

use katana_entity::anchor::resolve_anchor;
use katana_entity::curve_pieces::curve_pieces;
use katana_entity::{AnchorPoint, AnchorRef, Entity, EntityId, EnumName, Geometry, Model};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::editing::{bounding_box as curve_bounding_box, closest_point};
use katana_geometry::intersection::intersect;
use katana_geometry::{
    Arc2, Box2, Circle2, Curve2, CurveSegment, IntersectionKind, Line2, Point2, Segment2,
    SpatialIndex, Vec2,
};
use katana_math::numerics::{HALF_PI, tolerance};

use crate::layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
use crate::selection::is_drawn;
use crate::spatial_query::for_each_candidate;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SnapMode {
    #[default]
    None = 0,
    Endpoint = 1 << 0,
    Midpoint = 1 << 1,
    Center = 1 << 2,
    Intersection = 1 << 3,
    /// Needs SnapRequest::from.
    Perpendicular = 1 << 4,
    /// Needs SnapRequest::from.
    Tangent = 1 << 5,
    Nearest = 1 << 6,
    Grid = 1 << 7,
    // Appended 2026-09-25 with the drawing system (docs/drawing.md); the bits
    // above keep their values, since snap settings are saved by value.
    /// The ends of a circle's, arc's or ellipse's axes.
    Quadrant = 1 << 8,
    /// A point entity, a spline's fit points.
    Node = 1 << 9,
    /// Along a line or arc carried on past its end.
    Extension = 1 << 10,
    /// Needs SnapRequest::from: parallel to a hovered line.
    Parallel = 1 << 11,
    /// Where two lines WOULD cross if extended.
    ApparentIntersection = 1 << 12,
    // One-shot overrides, not running snaps: snap() ignores them and the
    // precision input (drawing/precision_input) runs them - From takes a base
    // point then an offset, MidBetween the middle of two picked points.
    From = 1 << 13,
    MidBetween = 1 << 14,
}

/// A set of SnapMode bits.
pub type SnapModes = u32;

impl BitOr for SnapMode {
    type Output = SnapModes;
    fn bitor(self, other: Self) -> SnapModes {
        self as SnapModes | other as SnapModes
    }
}

impl BitOr<SnapMode> for SnapModes {
    type Output = SnapModes;
    fn bitor(self, other: SnapMode) -> SnapModes {
        self | other as SnapModes
    }
}

pub const fn has_mode(modes: SnapModes, mode: SnapMode) -> bool {
    (modes & mode as SnapModes) != 0
}

pub const DEFAULT_SNAP_MODES: SnapModes = SnapMode::Endpoint as SnapModes
    | SnapMode::Midpoint as SnapModes
    | SnapMode::Center as SnapModes
    | SnapMode::Intersection as SnapModes;

/// Every running snap. (0xFF until the drawing system appended Quadrant to
/// ApparentIntersection; From and MidBetween are one-shot, so not in it.)
pub const ALL_SNAP_MODES: SnapModes = 0x1FFF;

impl EnumName for SnapMode {
    fn name(self) -> &'static str {
        match self {
            SnapMode::None => "None",
            SnapMode::Endpoint => "Endpoint",
            SnapMode::Midpoint => "Midpoint",
            SnapMode::Center => "Center",
            SnapMode::Intersection => "Intersection",
            SnapMode::Perpendicular => "Perpendicular",
            SnapMode::Tangent => "Tangent",
            SnapMode::Nearest => "Nearest",
            SnapMode::Grid => "Grid",
            SnapMode::Quadrant => "Quadrant",
            SnapMode::Node => "Node",
            SnapMode::Extension => "Extension",
            SnapMode::Parallel => "Parallel",
            SnapMode::ApparentIntersection => "Apparent Intersection",
            SnapMode::From => "From",
            SnapMode::MidBetween => "Midpoint Between Two Points",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapResult {
    pub point: Point2,
    pub mode: SnapMode,
    /// INVALID_ENTITY_ID (0) for Grid.
    pub entity: EntityId,
}

/// The C++ `std::function<bool(const SnapResult&)>`.
pub type SnapAccept<'a> = Box<dyn Fn(&SnapResult) -> bool + 'a>;

pub struct SnapRequest<'a> {
    pub cursor: Point2,
    /// Model units; candidates farther than this are ignored.
    pub aperture: f64,
    pub modes: SnapModes,
    /// Start of the segment being drawn; enables Perpendicular and Tangent.
    pub from: Option<Point2>,
    /// Model units; <= 0 disables Grid.
    pub grid_spacing: f64,
    /// Where Grid is anchored (a grid need not pass through the origin).
    pub grid_origin: Point2,
    /// The view the cursor is in; None for the document rule alone. A layer
    /// hidden in that view is not snappable there: a snap to a line nobody
    /// can see puts a point somewhere the user cannot explain.
    pub view: Option<&'a LayerOverrides>,
    /// Which candidates the asker can use; one it answers false for is passed
    /// over and the next best taken, in the usual order. The plan view asks
    /// the running tool (InteractiveTool::takes_snap): Insert Vertex takes
    /// only a snap ON the polyline it inserts into. None: every candidate.
    pub accept: Option<SnapAccept<'a>>,
}

impl Default for SnapRequest<'_> {
    fn default() -> Self {
        Self {
            cursor: Point2::default(),
            aperture: 0.0,
            modes: DEFAULT_SNAP_MODES,
            from: None,
            grid_spacing: 0.0,
            grid_origin: Point2::default(),
            view: None,
            accept: None,
        }
    }
}

impl SnapRequest<'_> {
    fn accepts(&self, candidate: &SnapResult) -> bool {
        self.accept.as_ref().is_none_or(|accept| accept(candidate))
    }
}

/// Lower rank wins ties between equally distant candidates.
fn rank_of(mode: SnapMode) -> i32 {
    match mode {
        SnapMode::Endpoint => 0,
        SnapMode::Intersection => 1,
        SnapMode::Midpoint => 2,
        SnapMode::Center => 3,
        SnapMode::Perpendicular => 4,
        SnapMode::Tangent => 5,
        // The drawing system's modes rank after the originals, and the ones
        // that are about lines NOT drawn (extensions, apparent crossings,
        // parallels) after the ones on drawn geometry.
        SnapMode::Node => 6,
        SnapMode::Quadrant => 7,
        SnapMode::ApparentIntersection => 8,
        SnapMode::Extension => 9,
        SnapMode::Parallel => 10,
        _ => 11,
    }
}

struct Collector<'r, 'a> {
    request: &'r SnapRequest<'a>,
    best: Option<SnapResult>,
    best_distance: f64,
}

impl<'r, 'a> Collector<'r, 'a> {
    fn new(request: &'r SnapRequest<'a>) -> Self {
        Self {
            request,
            best: None,
            best_distance: 0.0,
        }
    }

    /// Candidate that must itself lie within the aperture of the cursor.
    fn offer(&mut self, point: Point2, mode: SnapMode, entity: EntityId) {
        if has_mode(self.request.modes, mode) {
            self.consider(point, mode, entity, point.distance_to(self.request.cursor));
        }
    }

    /// A centre is offered while the cursor hovers the curve, not just the
    /// centre itself. Hovering the curve ranks as far away as the aperture
    /// allows, so any exact snap on that curve still wins.
    fn offer_center(&mut self, center: Point2, distance_to_curve: f64, entity: EntityId) {
        if !has_mode(self.request.modes, SnapMode::Center) {
            return;
        }
        let to_center = center.distance_to(self.request.cursor);
        if to_center <= self.request.aperture {
            self.consider(center, SnapMode::Center, entity, to_center);
        } else if distance_to_curve <= self.request.aperture {
            self.consider(center, SnapMode::Center, entity, self.request.aperture);
        }
    }

    fn consider(&mut self, point: Point2, mode: SnapMode, entity: EntityId, distance: f64) {
        if distance > self.request.aperture {
            return;
        }
        let better = match &self.best {
            None => true,
            Some(best) => {
                distance < self.best_distance
                    || (distance == self.best_distance && rank_of(mode) < rank_of(best.mode))
            }
        };
        if !better {
            return;
        }
        // Asked only of a candidate that would win, so a costly accept (a
        // tool planning its edit there) runs a handful of times a move.
        let candidate = SnapResult {
            point,
            mode,
            entity,
        };
        if !self.request.accepts(&candidate) {
            return;
        }
        self.best = Some(candidate);
        self.best_distance = distance;
    }
}

/// Points where a line from `from` meets the circle at a right angle: the two
/// ends of the diameter through `from`.
fn perpendicular_feet(circle: &Circle2, from: Point2) -> Vec<Point2> {
    let direction = (from - circle.center).normalized();
    if direction == Vec2::default() {
        return Vec::new(); // from the centre every radius is perpendicular; no unique foot
    }
    vec![
        circle.center + direction * circle.radius,
        circle.center - direction * circle.radius,
    ]
}

/// Tangent points of the lines through `from`. None when `from` is inside.
fn tangent_points(circle: &Circle2, from: Point2) -> Vec<Point2> {
    let to_from = from - circle.center;
    let distance = to_from.length();
    if !(distance > circle.radius + tolerance::GEOMETRIC) {
        return Vec::new();
    }
    // The radius to a tangent point makes angle acos(r/d) with the
    // centre->from line.
    let angle = (circle.radius / distance).acos();
    let unit = to_from / distance;
    vec![
        circle.center + unit.rotated(angle) * circle.radius,
        circle.center + unit.rotated(-angle) * circle.radius,
    ]
}

fn on_arc(arc: &Arc2, point: Point2) -> bool {
    arc.contains_angle_with_tolerance(
        (point - arc.center).angle(),
        tolerance::GEOMETRIC / arc.radius,
    )
}

/// The snaps one entity offers (the C++ EntitySnaps visitor).
struct EntitySnaps<'c, 'r, 'a> {
    collector: &'c mut Collector<'r, 'a>,
    request: &'r SnapRequest<'a>,
    id: EntityId,
}

impl EntitySnaps<'_, '_, '_> {
    fn segment(&mut self, s: &Segment2, with_endpoints: bool) {
        if with_endpoints {
            self.collector.offer(s.start, SnapMode::Endpoint, self.id);
            self.collector.offer(s.end, SnapMode::Endpoint, self.id);
        }
        self.collector
            .offer(s.midpoint(), SnapMode::Midpoint, self.id);
        if let Some(from) = self.request.from
            && !s.is_degenerate()
        {
            let line = Line2 {
                origin: s.start,
                direction: s.delta(),
            };
            let foot = line.closest_point(from);
            if s.distance_to(foot) <= tolerance::GEOMETRIC {
                self.collector.offer(foot, SnapMode::Perpendicular, self.id);
            }
        }
    }

    fn quadrants(&mut self, circle: &Circle2, within: Option<&Arc2>) {
        for k in 0..4 {
            let p = circle.point_at_angle(HALF_PI * f64::from(k));
            if within.is_none_or(|arc| on_arc(arc, p)) {
                self.collector.offer(p, SnapMode::Quadrant, self.id);
            }
        }
    }

    fn circle(&mut self, g: &Circle2) {
        self.collector
            .offer_center(g.center, g.distance_to(self.request.cursor), self.id);
        self.quadrants(g, None);
        if let Some(from) = self.request.from {
            for p in perpendicular_feet(g, from) {
                self.collector.offer(p, SnapMode::Perpendicular, self.id);
            }
            for p in tangent_points(g, from) {
                self.collector.offer(p, SnapMode::Tangent, self.id);
            }
        }
    }

    fn arc(&mut self, g: &Arc2, with_endpoints: bool) {
        if with_endpoints {
            self.collector
                .offer(g.start_point(), SnapMode::Endpoint, self.id);
            self.collector
                .offer(g.end_point(), SnapMode::Endpoint, self.id);
        }
        self.collector
            .offer(g.midpoint(), SnapMode::Midpoint, self.id);
        self.collector
            .offer_center(g.center, g.distance_to(self.request.cursor), self.id);
        self.quadrants(&g.circle(), Some(g));
        if let Some(from) = self.request.from {
            for p in perpendicular_feet(&g.circle(), from) {
                if on_arc(g, p) {
                    self.collector.offer(p, SnapMode::Perpendicular, self.id);
                }
            }
            for p in tangent_points(&g.circle(), from) {
                if on_arc(g, p) {
                    self.collector.offer(p, SnapMode::Tangent, self.id);
                }
            }
        }
    }

    fn geometry(&mut self, geometry: &Geometry) {
        match geometry {
            Geometry::Point(g) => {
                self.collector
                    .offer(g.position, SnapMode::Endpoint, self.id);
                self.collector.offer(g.position, SnapMode::Node, self.id);
            }
            Geometry::Line(g) => self.segment(g, true),
            Geometry::Polyline(g) => {
                for &vertex in &g.vertices {
                    self.collector.offer(vertex, SnapMode::Endpoint, self.id);
                }
                for i in 0..g.segment_count() {
                    self.segment(&g.segment(i), false);
                }
            }
            Geometry::Circle(g) => self.circle(g),
            Geometry::Arc(g) => self.arc(g, true),
            // The insertion point.
            Geometry::Text(g) => self
                .collector
                .offer(g.position, SnapMode::Endpoint, self.id),
            Geometry::Dimension(g) => {
                self.collector.offer(g.start, SnapMode::Endpoint, self.id);
                self.collector.offer(g.end, SnapMode::Endpoint, self.id);
                if g.uses_vertex() {
                    self.collector.offer(g.vertex, SnapMode::Endpoint, self.id);
                }
            }
            // A label offers nothing: its text is not geometry anyone draws to.
            Geometry::Label(_) => {}
            // A leader's tip and each bend, as a polyline's vertices.
            Geometry::Leader(g) => {
                for &vertex in &g.vertices {
                    self.collector.offer(vertex, SnapMode::Endpoint, self.id);
                }
            }
            // Each vertex an endpoint; a straight segment as a Polyline2's, an
            // arc segment as an Arc2 (its midpoint, its centre, perpendicular
            // and tangent feet on it).
            Geometry::CurvePolyline(g) => {
                for vertex in &g.vertices {
                    self.collector
                        .offer(vertex.position, SnapMode::Endpoint, self.id);
                }
                for i in 0..g.segment_count() {
                    match g.segment(i) {
                        CurveSegment::Segment(line) => self.segment(&line, false),
                        CurveSegment::Arc(arc) => self.arc(&arc, false),
                    }
                }
            }
            // The centre (while the curve is hovered, as a circle's), the ends
            // of an arc, and the ends of the axes that lie on it (the Quadrant
            // snap).
            Geometry::Ellipse(g) => {
                self.collector
                    .offer_center(g.center, g.distance_to(self.request.cursor), self.id);
                if !g.is_full() {
                    self.collector
                        .offer(g.start_point(), SnapMode::Endpoint, self.id);
                    self.collector
                        .offer(g.end_point(), SnapMode::Endpoint, self.id);
                    self.collector.offer(
                        g.point_at_parameter(g.start_parameter + 0.5 * g.sweep),
                        SnapMode::Midpoint,
                        self.id,
                    );
                }
                for p in g.quadrants() {
                    self.collector.offer(p, SnapMode::Quadrant, self.id);
                }
            }
            // The ends, and the fit points a spline was drawn through (Node:
            // they are points the user placed, as a point entity is).
            Geometry::Spline(g) => {
                if g.check_structure().is_err() {
                    return;
                }
                self.collector
                    .offer(g.start_point(), SnapMode::Endpoint, self.id);
                self.collector
                    .offer(g.end_point(), SnapMode::Endpoint, self.id);
                for &p in &g.fit_points {
                    self.collector.offer(p, SnapMode::Node, self.id);
                }
            }
        }
    }
}

/// Curves of an entity that can take part in intersection and nearest snaps.
///
/// A polyline's SEGMENTS are culled individually against the cursor's reach.
/// The entity-level filter admits the whole polyline when any part of it is
/// near the cursor, so a 3000-vertex surveyed string contributed 3000 curves to
/// a loop that intersects every pair - 4.5 million intersections, measured at
/// 247 ms per mouse move in Release, against section 32's 16 ms interaction
/// budget.
///
/// The cull is exact, not an approximation. Every candidate is accepted only if
/// it lies within `aperture` of the cursor (Collector::consider, and the
/// Nearest pass below), and every candidate lies ON the curve that produced it.
/// So an accepted point is inside `reach`, and the bounding box of the curve
/// carrying it therefore meets `reach` too: a segment whose box misses `reach`
/// cannot contribute a point that would have been accepted.
fn append_curves(entity: &Entity, reach: Box2, curves: &mut Vec<(EntityId, Curve2)>) {
    match &entity.geometry {
        Geometry::Line(segment) => curves.push((entity.id, Curve2::Segment(*segment))),
        Geometry::Arc(arc) => curves.push((entity.id, Curve2::Arc(*arc))),
        Geometry::Circle(circle) => curves.push((entity.id, Curve2::Circle(*circle))),
        Geometry::Polyline(polyline) => {
            for i in 0..polyline.segment_count() {
                let piece = polyline.segment(i);
                if piece.bounding_box().intersects(reach) {
                    curves.push((entity.id, Curve2::Segment(piece)));
                }
            }
        }
        // Every other linework kind through the one list of pieces (entity's
        // curve_pieces.rs): a curve polyline's segments and arcs, an
        // ellipse's and a spline's chords - culled the same way.
        _ => {
            for piece in curve_pieces(&entity.geometry, CURVE_CHORD_TOLERANCE) {
                if curve_bounding_box(&piece).intersects(reach) {
                    curves.push((entity.id, piece));
                }
            }
        }
    }
}

// ---- the snaps to lines that are not drawn --------------------------------------------
//
// Extension, Apparent Intersection and Parallel find points AWAY from the
// geometry that produces them - on a line carried on past its end, where two
// lines would cross, on the parallel through the last point - so they look at
// the curves of a wider neighbourhood than the aperture (TRACKING_REACH
// apertures), and accept only candidates within the aperture of the cursor
// like every other snap.
const TRACKING_REACH: f64 = 25.0;

#[allow(
    clippy::manual_range_contains,
    reason = "The C++'s `t < 0 || t > 1` offers nothing for a NaN, where `!contains` would."
)]
fn offer_extensions(
    collector: &mut Collector<'_, '_>,
    request: &SnapRequest<'_>,
    curves: &[(EntityId, Curve2)],
) {
    for (id, curve) in curves {
        match curve {
            Curve2::Segment(line) => {
                if line.is_degenerate() {
                    continue;
                }
                let carried = Line2 {
                    origin: line.start,
                    direction: line.delta(),
                };
                let t = carried.parameter_of(request.cursor);
                if t < 0.0 || t > 1.0 {
                    collector.offer(carried.point_at(t), SnapMode::Extension, *id);
                }
            }
            Curve2::Arc(arc) => {
                let to_cursor = request.cursor - arc.center;
                if to_cursor.length() > 0.0 && !on_arc(arc, arc.center + to_cursor) {
                    collector.offer(
                        arc.center + to_cursor.normalized() * arc.radius,
                        SnapMode::Extension,
                        *id,
                    );
                }
            }
            Curve2::Circle(_) => {}
        }
    }
}

fn offer_apparent_intersections(collector: &mut Collector<'_, '_>, curves: &[(EntityId, Curve2)]) {
    for (i, (first_id, first)) in curves.iter().enumerate() {
        let Curve2::Segment(a) = first else {
            continue;
        };
        if a.is_degenerate() {
            continue;
        }
        for (second_id, second) in &curves[i + 1..] {
            let Curve2::Segment(b) = second else {
                continue;
            };
            if b.is_degenerate() || first_id == second_id {
                continue;
            }
            let da = a.delta();
            let db = b.delta();
            let denominator = da.cross(db);
            if denominator.abs() <= tolerance::ANGULAR * da.length() * db.length() {
                continue;
            }
            let t = (b.start - a.start).cross(db) / denominator;
            let u = (b.start - a.start).cross(da) / denominator;
            // A real crossing is Intersection's; this is only the ones that
            // need at least one of the lines carried on.
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                continue;
            }
            collector.offer(a.point_at(t), SnapMode::ApparentIntersection, *first_id);
        }
    }
}

fn offer_parallels(
    collector: &mut Collector<'_, '_>,
    request: &SnapRequest<'_>,
    curves: &[(EntityId, Curve2)],
) {
    let Some(from) = request.from else {
        return;
    };
    for (id, curve) in curves {
        let Curve2::Segment(line) = curve else {
            continue;
        };
        if line.is_degenerate() {
            continue;
        }
        let parallel = Line2 {
            origin: from,
            direction: line.delta(),
        };
        let foot = parallel.closest_point(request.cursor);
        if foot.distance_to(from) > tolerance::GEOMETRIC {
            collector.offer(foot, SnapMode::Parallel, *id);
        }
    }
}

/// Considers drawn entities only (visible, on visible layers; locked layers
/// are valid snap targets). None when nothing is within the aperture.
///
/// `index`, when supplied, narrows the search to the entities near the cursor
/// instead of scanning the model (PLAN.MD Phase 18). It must be in step with
/// `model`; the Document keeps one that is. The result is IDENTICAL either way
/// (the index is a broad phase and every exact test still runs), which is
/// asserted by a test that compares the two paths over random models.
pub fn snap(
    model: &Model,
    request: &SnapRequest<'_>,
    index: Option<&SpatialIndex>,
) -> Option<SnapResult> {
    let mut collector = Collector::new(request);
    let reach = Box2::new(request.cursor, request.cursor).inflated(request.aperture);
    let view = request.view.unwrap_or(&NO_LAYER_OVERRIDES);

    // By id: the visit hands each entity for the visit's own length, and the
    // offers below need them again (the C++ kept pointers).
    let mut nearby: Vec<EntityId> = Vec::new();
    let mut scratch = Vec::new();
    // The extent test is query_extents, which is deliberately wider than the
    // bounding box for an arc so that its centre stays snappable.
    for_each_candidate(model, index, reach, &mut scratch, |entity| {
        if is_drawn(model, entity, view) {
            nearby.push(entity.id);
        }
    });

    // Used by Intersection and Nearest.
    let mut curves: Vec<(EntityId, Curve2)> = Vec::new();
    for &id in &nearby {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        EntitySnaps {
            collector: &mut collector,
            request,
            id: entity.id,
        }
        .geometry(&entity.geometry);
        append_curves(entity, reach, &mut curves);
    }

    let tracking = has_mode(request.modes, SnapMode::Extension)
        || has_mode(request.modes, SnapMode::ApparentIntersection)
        || (has_mode(request.modes, SnapMode::Parallel) && request.from.is_some());
    // Extension and Parallel are "somewhere along a line" snaps, like
    // Nearest: an exact point on drawn geometry beats them however close they
    // are, so they are collected apart and asked after Nearest.
    let mut along = Collector::new(request);
    if tracking {
        let wide = reach.inflated(request.aperture * TRACKING_REACH);
        let mut far: Vec<(EntityId, Curve2)> = Vec::new();
        for_each_candidate(model, index, wide, &mut scratch, |entity| {
            if is_drawn(model, entity, view) {
                append_curves(entity, wide, &mut far);
            }
        });
        if has_mode(request.modes, SnapMode::Extension) {
            offer_extensions(&mut along, request, &far);
        }
        if has_mode(request.modes, SnapMode::ApparentIntersection) {
            offer_apparent_intersections(&mut collector, &far);
        }
        if has_mode(request.modes, SnapMode::Parallel) {
            offer_parallels(&mut along, request, &far);
        }
    }

    let pair_limit = if has_mode(request.modes, SnapMode::Intersection) {
        curves.len()
    } else {
        0
    };
    let pairs = &curves[..pair_limit];
    for (i, (first_id, first)) in pairs.iter().enumerate() {
        for (_, second) in &pairs[i + 1..] {
            let hit = intersect(first, second);
            if hit.kind != IntersectionKind::Points {
                continue;
            }
            for &point in &hit.points[..hit.count] {
                collector.offer(point, SnapMode::Intersection, *first_id);
            }
        }
    }
    if collector.best.is_some() {
        return collector.best;
    }

    if has_mode(request.modes, SnapMode::Nearest) {
        let mut nearest = None;
        let mut nearest_distance = request.aperture;
        for (id, curve) in &curves {
            let point = closest_point(curve, request.cursor);
            let distance = point.distance_to(request.cursor);
            let candidate = SnapResult {
                point,
                mode: SnapMode::Nearest,
                entity: *id,
            };
            if distance <= nearest_distance && request.accepts(&candidate) {
                nearest_distance = distance;
                nearest = Some(candidate);
            }
        }
        if nearest.is_some() {
            return nearest;
        }
    }
    if along.best.is_some() {
        return along.best;
    }

    if has_mode(request.modes, SnapMode::Grid) && request.grid_spacing > 0.0 {
        let spacing = request.grid_spacing;
        let d = request.cursor - request.grid_origin;
        let node = SnapResult {
            point: request.grid_origin
                + Vec2::new(
                    (d.x / spacing).round() * spacing,
                    (d.y / spacing).round() * spacing,
                ),
            mode: SnapMode::Grid,
            entity: katana_entity::entity::INVALID_ENTITY_ID,
        };
        if request.accepts(&node) {
            return Some(node);
        }
    }
    None
}

/// The named point of the entity `snap` found that sits where it snapped - a
/// line's or an arc's start, end or middle, an arc's or a circle's centre, a
/// polyline's vertex or the middle of one of its segments, a point's or a
/// text's position, a leader's tip or last vertex - as the reference that
/// names it (entity's anchor.rs), so annotation made from the snapped point
/// can follow the entity as one made from a typed #id.end point does.
/// Endpoint, Midpoint and Center snaps only: an Intersection, Perpendicular,
/// Tangent or Nearest point is not a point one entity keeps as it is edited,
/// and Grid is no entity's. None too when the entity is gone, or none of its
/// named points is within the geometric tolerance of the snapped point.
pub fn snap_anchor(model: &Model, snap: &SnapResult) -> Option<AnchorRef> {
    if snap.mode != SnapMode::Endpoint
        && snap.mode != SnapMode::Midpoint
        && snap.mode != SnapMode::Center
    {
        return None;
    }
    let entity = model.entities.find(snap.entity)?;
    // The points each kind names, in the order a tie is settled: a polyline's
    // corners by their vertex, not as its start or end.
    let id = entity.id;
    let name = |point: AnchorPoint, index: u32| AnchorRef {
        entity: id,
        point,
        index,
        parameter: 0.0,
    };
    let mut names: Vec<AnchorRef> = Vec::new();
    match &entity.geometry {
        Geometry::Line(_) => {
            names.push(name(AnchorPoint::Start, 0));
            names.push(name(AnchorPoint::End, 0));
            names.push(name(AnchorPoint::Mid, 0));
        }
        Geometry::Arc(_) => {
            names.push(name(AnchorPoint::Start, 0));
            names.push(name(AnchorPoint::End, 0));
            names.push(name(AnchorPoint::Mid, 0));
            names.push(name(AnchorPoint::Centre, 0));
        }
        Geometry::Circle(_) => names.push(name(AnchorPoint::Centre, 0)),
        Geometry::Polyline(polyline) => {
            for i in 0..polyline.vertices.len() {
                names.push(name(AnchorPoint::Vertex, i as u32));
            }
            for i in 0..polyline.segment_count() {
                names.push(name(AnchorPoint::SegmentMid, i as u32));
            }
        }
        // As a polyline: its segment middles are ON its arcs (resolve_anchor).
        Geometry::CurvePolyline(curve) => {
            for i in 0..curve.vertices.len() {
                names.push(name(AnchorPoint::Vertex, i as u32));
            }
            for i in 0..curve.segment_count() {
                names.push(name(AnchorPoint::SegmentMid, i as u32));
            }
        }
        Geometry::Ellipse(_) => {
            names.push(name(AnchorPoint::Centre, 0));
            names.push(name(AnchorPoint::Start, 0));
            names.push(name(AnchorPoint::End, 0));
        }
        Geometry::Spline(_) | Geometry::Leader(_) => {
            names.push(name(AnchorPoint::Start, 0));
            names.push(name(AnchorPoint::End, 0));
        }
        _ => names.push(name(AnchorPoint::Position, 0)),
    }
    names.into_iter().find(|reference| {
        resolve_anchor(entity, reference)
            .is_some_and(|at| at.distance_to(snap.point) <= tolerance::GEOMETRIC)
    })
}
