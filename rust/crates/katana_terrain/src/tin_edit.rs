//! Editing a TIN surface, and surfaces made of others (docs/surface_editing.md).
//!
//! A TinSurface is immutable: every edit here is a pure function that returns
//! a NEW surface, or a refusal saying specifically why - the surface handed in
//! is never touched, so the caller decides whether the result replaces it (one
//! undo step) or is kept beside it.
//!
//! What every edit keeps, unless it is the edit's purpose to change it:
//!   * THE CONSTRAINED EDGES. A breakline or boundary edge stays an edge, flag
//!     and all; a vertex on one is never moved by smoothing nor deleted, and a
//!     new line crossing one meets it at the level the old edge has there.
//!   * THE FOOTPRINT. Only the triangles an edit has to change are rebuilt
//!     (re-triangulated by the one constrained Delaunay kernel, cdt_backend),
//!     inside the ring of edges around them, which is held; the rest of the
//!     surface is the same triangles, vertex for vertex. Holes stay holes.
//!   * EXACTNESS where it decides. Which side of an edge a point lies on, and
//!     whether two triangles make a convex quadrilateral, are decided with the
//!     triangulation's own exact predicate (cdt_backend::orientation), not with
//!     a tolerance. Where the kernel has to make a vertex - a new line crossing
//!     an old edge - the vertex's level is interpolated along the edge whose
//!     level holds there (docs/surface_editing.md, "Levels of new vertices").
//!
//! An edit that rebuilds triangles gives a canonical surface, as build_tin's
//! is: vertices of the old surface in their order (those left without a
//! triangle dropped), then new ones; every triangle rotated to start at its
//! smallest vertex and the triangles sorted. A level edit (RAISE, SMOOTH ...)
//! keeps the triangles as they were, in the order they were. An edit that
//! changes nothing returns the surface as it was, not a reordered copy of it.
//! The same edit of the same surface gives the same bits.

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::polygon::{Orientation, orientation as polygon_orientation};
use katana_geometry::{Containment, PolygonClassifier, Segment2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::cdt_backend::orientation;
use crate::point_merge::PointMerger;
use crate::super_surface::combine_surfaces;
use crate::tin_patch::{
    PatchOutput, PatchPoint, PatchRequest, PatchSegment, PlaceKind, TriangleGrid, VertexStars,
    describe_position, extend_surface, finish_surface, mark_insertion_cavity,
    mark_reached_by_point, place_of, rebuild_patch, report_of, too_large, unchanged,
    within_magnitude,
};
use crate::tin_surface::{
    Box2, NO_TRIANGLE, NO_VERTEX, Point2, Point3, TinSurface, TinTriangle, plan_of,
};

pub use crate::tin_thin::thin_surface;
pub use katana_geometry::Polyline2;

/// The largest coordinate or level, in metres, an edit accepts. Beyond a
/// projected grid or a reduced level by orders of magnitude (a UTM easting is
/// 1e7 at most), and well inside what areas and volumes (sums of products of
/// three such numbers) hold: a typing slip such as RAISE 1e308 is refused
/// where it is made, instead of making a surface whose plan area reads nan.
pub const MOST_MAGNITUDE: f64 = 1.0e9;

/// What an edit did: what a verb's reply says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TinEditReport {
    pub triangles_before: usize,
    pub triangles_after: usize,
    pub vertices_before: usize,
    pub vertices_after: usize,
    /// Vertices the edit made: the points added, a breakline's or a
    /// polygon's vertices, and the vertices made where a new line crossed an
    /// old edge (counted again in `crossings`).
    pub points_added: usize,
    /// Vertices the edit took out (deleted, thinned, or left without a
    /// triangle by a deletion, a boundary or a hole).
    pub points_removed: usize,
    /// Vertices of the surface a point given at their position gave a new
    /// level to (ADD POINT on an existing vertex, a 3D breakline through one,
    /// the seam of a paste).
    pub points_relevelled: usize,
    /// Vertices whose level a height edit or smoothing changed, or that
    /// move_point moved in plan (the vertex it makes is a new one, so that edit
    /// also counts one added and one removed).
    pub points_moved: usize,
    /// Vertices an edit left alone on purpose: on a breakline or the rim
    /// (smoothing, deleting points inside a polygon).
    pub points_held: usize,
    /// Vertices made where a new line crossed an edge of the surface.
    pub crossings: usize,
    /// delete_breaklines: the breakline edges made ordinary edges, and the
    /// constrained edges along the line it left alone on the rim (the
    /// surface's boundary, not a breakline).
    pub edges_cleared: usize,
    pub edges_held: usize,
    /// The largest change of level at any vertex (heights, smoothing), or the
    /// bound on how far a thinned surface lies from the original (thinning).
    pub largest_change: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TinEditResult {
    pub surface: TinSurface,
    pub report: TinEditReport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TinEdgeRef {
    pub triangle: u32,
    /// edge k joins the triangle's vertex k and k + 1
    pub edge: u32,
}

impl Default for TinEdgeRef {
    fn default() -> Self {
        Self {
            triangle: NO_TRIANGLE,
            edge: 0,
        }
    }
}

/// How delete_points treats a vertex on a breakline or a boundary edge, which
/// it never deletes (the constrained edge would go with it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeldPointRule {
    /// fail, naming the vertex: the one vertex a person pointed at
    #[default]
    Refuse,
    /// leave it, and count it (TinEditReport::points_held): a polygon's worth
    Skip,
}

/// One vertex of a breakline being added: where, and its level - none to take
/// the surface's there (draped).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BreaklineVertex {
    pub position: Point2,
    pub z: Option<f64>,
}

/// One line of an edit that adds several (BreaklineVertex's rules).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreaklineInput {
    pub vertices: Vec<BreaklineVertex>,
    pub closed: bool,
}

/// How cut_holes went about it: for a test and a benchmark to see that disjoint
/// footprints shared a patch, not for a front end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoleCutStats {
    /// Whole-surface rebuilds made (copies, finish_surface, TinSurface::create):
    /// one for every batch of footprints that could be cut together.
    pub patches: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeightOperation {
    /// z + value (a negative value lowers)
    #[default]
    Raise,
    /// value
    SetLevel,
    /// base + value * (z - base)
    Scale,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeightChange {
    pub operation: HeightOperation,
    pub value: f64,
    /// Scale only: the level that stays where it is. None: the lowest level
    /// of the vertices changed.
    pub base: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothOptions {
    pub passes: i32,
    /// How far each pass moves a vertex towards the mean level of its
    /// neighbours: 0 not at all, 1 all the way. In (0, 1].
    pub weight: f64,
}

impl Default for SmoothOptions {
    fn default() -> Self {
        Self {
            passes: 3,
            weight: 0.5,
        }
    }
}

fn require_surface(surface: &TinSurface, which: &str) -> Status {
    if surface.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{which} is empty"),
        ));
    }
    Ok(())
}

/// One flag per vertex: some triangle uses it.
fn referenced_vertices(surface: &TinSurface) -> Vec<u8> {
    let mut used = vec![0u8; surface.vertex_count()];
    for tri in surface.triangles() {
        for &v in tri {
            used[v as usize] = 1;
        }
    }
    used
}

/// One flag per vertex: an end of a constrained edge.
fn constrained_vertices(surface: &TinSurface) -> Vec<u8> {
    let mut flags = vec![0u8; surface.vertex_count()];
    for (t, tri) in surface.triangles().iter().enumerate() {
        for k in 0..3 {
            if surface.is_edge_constrained(t, k) {
                flags[tri[k] as usize] = 1;
                flags[tri[(k + 1) % 3] as usize] = 1;
            }
        }
    }
    flags
}

/// Whether two closed segments meet, decided exactly (the kernel's predicate).
fn segments_meet(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);
    if o1 * o2 < 0 && o3 * o4 < 0 {
        return true;
    }
    // A point on the line of the other segment meets it when it lies within it.
    let within = |p: Point2, s: Point2, e: Point2| {
        (cpp::min(s.x, e.x)..=cpp::max(s.x, e.x)).contains(&p.x)
            && (cpp::min(s.y, e.y)..=cpp::max(s.y, e.y)).contains(&p.y)
    };
    (o1 == 0 && within(c, a, b))
        || (o2 == 0 && within(d, a, b))
        || (o3 == 0 && within(a, c, d))
        || (o4 == 0 && within(b, c, d))
}

/// Whether two edges of the closed ring that share no corner meet: a bow-tie,
/// or a corner on another edge. Only asked of a ring with no signed area (a
/// figure of eight whose lobes cancel), to say why, so it is not worth
/// anything faster than every pair - and is not asked of a ring too long for
/// that to be a moment's work.
fn ring_touches_itself(ring: &[Point2]) -> bool {
    const MOST_CORNERS: usize = 2000;
    let n = ring.len();
    if n > MOST_CORNERS {
        return false;
    }
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j + 1 == n {
                continue; // joined by the closing edge
            }
            if segments_meet(ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]) {
                return true;
            }
        }
    }
    false
}

/// A polygon as the edits take it: every coordinate finite and within
/// MOST_MAGNITUDE, repeated vertices (within GEOMETRIC) and a repeated
/// closing vertex dropped, at least three distinct vertices with an area,
/// closed, counter-clockwise.
fn clean_polygon(polygon: &Polyline2, what: &str) -> Result<Polyline2> {
    let mut ring = Polyline2::new(Vec::new(), true);
    for &vertex in &polygon.vertices {
        if !vertex.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{what} has a non-finite coordinate"),
            ));
        }
        if !within_magnitude(vertex.x) || !within_magnitude(vertex.y) {
            return Err(too_large(&describe_position(vertex)));
        }
        if ring
            .vertices
            .last()
            .is_none_or(|last| last.distance_to(vertex) > tolerance::GEOMETRIC)
        {
            ring.vertices.push(vertex);
        }
    }
    while ring.vertices.len() > 1
        && ring.vertices[0].distance_to(ring.vertices[ring.vertices.len() - 1])
            <= tolerance::GEOMETRIC
    {
        ring.vertices.pop();
    }
    if ring.vertices.len() < 3 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            format!("{what} needs at least 3 distinct vertices"),
        )
        .with_context(format!("distinct vertices={}", ring.vertices.len())));
    }
    let turn = polygon_orientation(&ring);
    if turn == Orientation::Degenerate {
        // A figure of eight whose lobes cancel has no signed area either, and
        // "no area" would send a person looking for a flat polygon.
        let message = if ring_touches_itself(&ring.vertices) {
            format!("{what} touches or crosses itself")
        } else {
            format!("{what} has no area")
        };
        return Err(Error::new(ErrorCode::InvalidGeometry, message));
    }
    if turn == Orientation::Clockwise {
        ring.vertices.reverse();
    }
    Ok(ring)
}

/// How many of a patch's new vertices the triangles use, and how many of the
/// surface's used vertices they no longer use.
struct Use {
    added: usize,
    dropped: usize,
}

fn use_of(output: &PatchOutput, before: &TinSurface) -> Use {
    let mut used = vec![0u8; output.vertices.len()];
    for tri in &output.triangles {
        for &v in tri {
            used[v as usize] = 1;
        }
    }
    let used_before = referenced_vertices(before);
    let mut use_ = Use {
        added: 0,
        dropped: 0,
    };
    for (v, &flag) in used.iter().enumerate() {
        if v < before.vertex_count() {
            if used_before[v] != 0 && flag == 0 {
                use_.dropped += 1;
            }
        } else if flag != 0 {
            use_.added += 1;
        }
    }
    use_
}

fn finish_patch(before: &TinSurface, output: PatchOutput) -> Result<TinEditResult> {
    let use_ = use_of(&output, before);
    let relevelled = output.relevelled;
    let crossings = output.crossings;
    let surface = finish_surface(output.vertices, output.triangles, output.masks)?;
    let mut report = report_of(before, &surface);
    report.points_added = use_.added;
    report.points_removed = use_.dropped;
    report.points_relevelled = relevelled;
    report.crossings = crossings;
    Ok(TinEditResult { surface, report })
}

/// The surface with new levels and nothing else changed: the same triangles,
/// so the trusted path assembles it without deriving the adjacency again.
fn with_levels(surface: &TinSurface, vertices: Vec<Point3>) -> Result<TinSurface> {
    TinSurface::assemble(
        vertices,
        surface.triangles().clone(),
        surface.neighbors().clone(),
        surface.constrained_edges().clone(),
    )
}

/// The counts of `report` made those of before and after.
fn with_counts(
    mut report: TinEditReport,
    before: &TinSurface,
    after: &TinSurface,
) -> TinEditReport {
    let counts = report_of(before, after);
    report.triangles_before = counts.triangles_before;
    report.triangles_after = counts.triangles_after;
    report.vertices_before = counts.vertices_before;
    report.vertices_after = counts.vertices_after;
    report
}

// ---- finders ------------------------------------------------------------------------------

/// The triangle under `position` (TinSurface::locate).
pub fn triangle_at(surface: &TinSurface, position: Point2) -> Option<u32> {
    surface.locate(position).map(|location| location.triangle)
}

/// The edge of the triangle under `position` that `position` lies nearest.
pub fn edge_at(surface: &TinSurface, position: Point2) -> Option<TinEdgeRef> {
    let triangle = triangle_at(surface, position)?;
    let plan = surface.plan_triangle(triangle as usize);
    let corners = [plan.a, plan.b, plan.c];
    let mut nearest = TinEdgeRef { triangle, edge: 0 };
    let mut best = f64::INFINITY;
    for k in 0..3u32 {
        let distance =
            Segment2::new(corners[k as usize], corners[(k as usize + 1) % 3]).distance_to(position);
        if distance < best {
            best = distance;
            nearest.edge = k;
        }
    }
    Some(nearest)
}

/// The corner of the triangle under `position` that lies nearest it; None
/// off the surface. With a `reach` (the snap aperture of a click) that corner
/// when it is within reach; else the vertex nearest `position` within reach,
/// wherever it is: a click a hair outside the rim, or in a thin triangle whose
/// corners are all farther than the vertex beside it, finds that vertex.
/// (Where a corner is within reach the answer is the one it always was.)
pub fn vertex_at(surface: &TinSurface, position: Point2, reach: Option<f64>) -> Option<u32> {
    let triangle = triangle_at(surface, position);
    let mut nearest = NO_VERTEX;
    let mut best = f64::INFINITY;
    if let Some(triangle) = triangle {
        for &v in &surface.triangles()[triangle as usize] {
            let distance = plan_of(surface.vertices()[v as usize]).distance_to(position);
            if distance < best {
                best = distance;
                nearest = v;
            }
        }
        if reach.is_none_or(|reach| best <= reach) {
            return Some(nearest);
        }
    } else if reach.is_none() {
        return None;
    }
    let reach = reach.unwrap_or(f64::INFINITY);
    // Off the surface, or in a triangle none of whose corners is within reach:
    // the aperture is a disc round the position, and a vertex in it need not
    // be a corner of the triangle under its centre. One click's worth of
    // scanning, only on this path (a corner of the triangle under the
    // position is the common answer and costs nothing).
    best = f64::INFINITY;
    nearest = NO_VERTEX;
    let used = referenced_vertices(surface);
    for (v, &flag) in used.iter().enumerate() {
        if flag == 0 {
            continue;
        }
        let distance = plan_of(surface.vertices()[v]).distance_to(position);
        if distance <= reach && distance < best {
            best = distance;
            nearest = v as u32;
        }
    }
    if nearest == NO_VERTEX {
        None
    } else {
        Some(nearest)
    }
}

/// Whether `vertex` is an end of a constrained (breakline or boundary) edge:
/// the rule delete_points refuses such a vertex by, for a front end to say so
/// before the click rather than after (the one test, not a second copy).
pub fn on_constrained_edge(surface: &TinSurface, vertex: u32) -> bool {
    for (t, tri) in surface.triangles().iter().enumerate() {
        for k in 0..3 {
            if surface.is_edge_constrained(t, k) && (tri[k] == vertex || tri[(k + 1) % 3] == vertex)
            {
                return true;
            }
        }
    }
    false
}

/// The triangles whose centroid lies inside `polygon` (on its edge counts as
/// inside), or outside it. `polygon` is taken as closed.
pub fn triangles_by_polygon(surface: &TinSurface, polygon: &Polyline2, inside: bool) -> Vec<u32> {
    let ring = PolygonClassifier::new(polygon);
    (0..surface.triangle_count() as u32)
        .filter(|&t| {
            let in_ =
                ring.classify(surface.plan_triangle(t as usize).centroid()) != Containment::Outside;
            in_ == inside
        })
        .collect()
}

/// The triangles with a side longer than `length` in plan.
pub fn triangles_longer_than(surface: &TinSurface, length: f64) -> Vec<u32> {
    (0..surface.triangle_count() as u32)
        .filter(|&t| {
            let plan = surface.plan_triangle(t as usize);
            let longest = cpp::max3(
                plan.a.distance_to(plan.b),
                plan.b.distance_to(plan.c),
                plan.c.distance_to(plan.a),
            );
            longest > length
        })
        .collect()
}

/// The triangles whose smallest angle in plan is below `radians`.
pub fn triangles_thinner_than(surface: &TinSurface, radians: f64) -> Vec<u32> {
    (0..surface.triangle_count() as u32)
        .filter(|&t| {
            let plan = surface.plan_triangle(t as usize);
            let corners = [plan.a, plan.b, plan.c];
            let mut smallest = f64::INFINITY;
            for k in 0..3 {
                let u = corners[(k + 1) % 3] - corners[k];
                let w = corners[(k + 2) % 3] - corners[k];
                // atan2(|u x w|, u . w) is the angle between them, well conditioned
                // near 0 and pi where acos of a cosine is not; 0 for a zero-length side.
                smallest = cpp::min(smallest, u.cross(w).abs().atan2(u.dot(w)));
            }
            smallest < radians
        })
        .collect()
}

/// The vertices that lie inside `polygon` or on its edge, ascending.
pub fn vertices_inside(surface: &TinSurface, polygon: &Polyline2) -> Vec<u32> {
    let ring = PolygonClassifier::new(polygon);
    let used = referenced_vertices(surface);
    (0..surface.vertex_count())
        .filter(|&v| {
            used[v] != 0 && ring.classify(plan_of(surface.vertices()[v])) != Containment::Outside
        })
        .map(|v| v as u32)
        .collect()
}

/// A vertex an edit holds: on the rim of the surface, or at an end of a
/// constrained (breakline or boundary) edge. One flag per vertex.
pub fn held_vertices(surface: &TinSurface) -> Vec<u8> {
    let mut held = constrained_vertices(surface);
    for (t, tri) in surface.triangles().iter().enumerate() {
        for k in 0..3 {
            if surface.neighbors()[t][k] == NO_TRIANGLE {
                held[tri[k] as usize] = 1;
                held[tri[(k + 1) % 3] as usize] = 1;
            }
        }
    }
    held
}

// ---- deleting -------------------------------------------------------------------------------

/// The surface without the given triangles (repeats ignored). Fails with
///   InvalidArgument  an index out of range, or none given;
///   InvalidState     every triangle would go.
pub fn delete_triangles(surface: &TinSurface, triangles: &[u32]) -> Result<TinEditResult> {
    if triangles.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no triangle to delete was given",
        ));
    }
    let mut drop = vec![0u8; surface.triangle_count()];
    for &t in triangles {
        if t as usize >= surface.triangle_count() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "triangle index out of range").with_context(
                    format!("triangle={t} triangles={}", surface.triangle_count()),
                ),
            );
        }
        drop[t as usize] = 1;
    }
    let mut output = PatchOutput {
        vertices: surface.vertices().clone(),
        ..PatchOutput::default()
    };
    for (t, &dropped) in drop.iter().enumerate() {
        if dropped == 0 {
            output.triangles.push(surface.triangles()[t]);
            output.masks.push(surface.constrained_edges()[t]);
        }
    }
    if output.triangles.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "every triangle of the surface would be deleted; nothing would be left",
        )
        .with_context(format!("triangles={}", surface.triangle_count())));
    }
    finish_patch(surface, output)
}

/// The surface without the given vertices: the triangles around them are
/// re-triangulated without them, inside the ring of edges around them. A
/// vertex on the rim may be deleted; the gap it leaves is filled only as far
/// as the old rim allows, so the footprint never grows. Fails with
///   InvalidArgument  an index out of range, or none given;
///   InvalidGeometry  a vertex on a constrained edge under HeldPointRule::Refuse;
///   InvalidState     every triangle would go;
///   TriangulationFailure  from the kernel.
pub fn delete_points(
    surface: &TinSurface,
    vertices: &[u32],
    held: HeldPointRule,
) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    if vertices.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no point to delete was given",
        ));
    }
    let constrained = constrained_vertices(surface);
    let used = referenced_vertices(surface);
    let mut request = PatchRequest::new();
    request.removed = vec![0; surface.vertex_count()];
    let mut held_count = 0usize;
    let mut removing = 0usize;
    for &v in vertices {
        if v as usize >= surface.vertex_count() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "vertex index out of range")
                    .with_context(format!("vertex={v} vertices={}", surface.vertex_count())),
            );
        }
        if constrained[v as usize] != 0 {
            if held == HeldPointRule::Refuse {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "the point is on a breakline or boundary edge, which is kept; delete the \
                     triangles around it instead",
                )
                .with_context(format!(
                    "vertex={v} at {}",
                    describe_position(plan_of(surface.vertices()[v as usize]))
                )));
            }
            held_count += 1;
            continue;
        }
        if used[v as usize] != 0 && request.removed[v as usize] == 0 {
            request.removed[v as usize] = 1;
            removing += 1;
        }
    }
    if removing == 0 {
        let mut result = unchanged(surface);
        result.report.points_held = held_count;
        return Ok(result);
    }
    let stars = VertexStars::new(surface);
    let output = rebuild_patch(surface, &stars, request, None)?;
    let mut result = finish_patch(surface, output)?;
    result.report.points_held = held_count;
    Ok(result)
}

// ---- changing the triangulation -----------------------------------------------------------

/// The vertex moved in plan to `target`, keeping its level: the triangles round
/// the old position and round the new one are rebuilt as ONE patch (the vertex
/// taken out and a point put in, by the one constrained Delaunay kernel, inside
/// the ring of edges round both), so the result is a valid surface whatever the
/// distance moved, and the rest of the surface is the same triangles. A target
/// within tolerance::GEOMETRIC of the vertex's own position is no move: the
/// surface as it was. Fails with
///   InvalidArgument  a vertex out of range or in no triangle, a target that is
///                    not finite or is beyond MOST_MAGNITUDE;
///   InvalidGeometry  the vertex is at an end of a constrained edge, or on the
///                    rim or the edge of a hole, the target is off the surface
///                    or within GEOMETRIC of another vertex;
///   TriangulationFailure  from the kernel.
/// The report counts the vertex as moved (one) and, since the vertex made is a
/// new one, as one added and one removed.
pub fn move_point(surface: &TinSurface, vertex: u32, target: Point2) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    if vertex as usize >= surface.vertex_count() {
        return Err(
            Error::new(ErrorCode::InvalidArgument, "vertex index out of range").with_context(
                format!("vertex={vertex} vertices={}", surface.vertex_count()),
            ),
        );
    }
    if !target.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the target has a non-finite coordinate",
        ));
    }
    if !within_magnitude(target.x) || !within_magnitude(target.y) {
        return Err(too_large(&describe_position(target)));
    }
    let stars = VertexStars::new(surface);
    let around = stars.of(vertex);
    if around.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the vertex belongs to no triangle",
        )
        .with_context(format!("vertex={vertex}")));
    }
    let from = plan_of(surface.vertices()[vertex as usize]);
    if from.distance_to(target) <= tolerance::GEOMETRIC {
        return Ok(unchanged(surface)); // a move of less than the coincidence tolerance
    }

    // What the vertex holds: an end of a constrained edge, or the rim.
    let mut on_breakline = false;
    let mut on_rim = false;
    for &t in around {
        let tri = surface.triangles()[t as usize];
        for k in 0..3 {
            if tri[k] != vertex && tri[(k + 1) % 3] != vertex {
                continue;
            }
            on_breakline = on_breakline || surface.is_edge_constrained(t as usize, k);
            on_rim = on_rim || surface.neighbors()[t as usize][k] == NO_TRIANGLE;
        }
    }
    let where_ = format!("vertex={vertex} at {}", describe_position(from));
    // The rim first: its edges are constrained when a boundary or a hole made
    // them, and "clear the breakline" would send a person after an edge
    // DELETE BREAKLINE never clears.
    if on_rim {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the point is on the rim of the surface or the edge of a hole: it is the footprint, \
             which a vertex moved would reshape; BOUNDARY and HOLE do that",
        )
        .with_context(where_));
    }
    if on_breakline {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the point is on a breakline or boundary edge, which is kept; clear the breakline \
             first (DELETE BREAKLINE) or delete the triangles around it instead",
        )
        .with_context(where_));
    }

    let place = place_of(surface, &stars, target);
    if place.kind == PlaceKind::Outside {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the target is off the surface: a point cannot be moved off it (ADD POINT extends \
             the surface)",
        )
        .with_context(format!("target {}", describe_position(target))));
    }
    if place.kind == PlaceKind::Vertex {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the target is closer than the coincidence tolerance to another vertex of the surface",
        )
        .with_context(format!(
            "vertex={} at {}",
            place.vertex,
            describe_position(plan_of(surface.vertices()[place.vertex as usize]))
        )));
    }

    // ONE patch: the star of the vertex (rebuild_patch takes it for a removed
    // vertex) and the cavity the new point makes, together.
    let mut request = PatchRequest::new();
    request.region = vec![0; surface.triangle_count()];
    request.removed = vec![0; surface.vertex_count()];
    request.removed[vertex as usize] = 1;
    mark_insertion_cavity(surface, &place, target, &mut request.region);
    request.points.push(PatchPoint {
        position: target,
        z: Some(surface.vertices()[vertex as usize].z),
        required: true,
    });
    let output = rebuild_patch(surface, &stars, request, None)?;
    let mut result = finish_patch(surface, output)?;
    result.report.points_moved = 1;
    Ok(result)
}

/// The edge swapped for the other diagonal of the quadrilateral its two
/// triangles make. Fails with
///   InvalidArgument  a triangle or edge out of range;
///   InvalidGeometry  the edge is on the rim (no triangle across it), the
///                    edge is constrained (a breakline or boundary), or the
///                    quadrilateral is not strictly convex, which would fold
///                    the surface over itself.
pub fn flip_edge(surface: &TinSurface, triangle: u32, edge: u32) -> Result<TinEditResult> {
    if triangle as usize >= surface.triangle_count() || edge > 2 {
        return Err(
            Error::new(ErrorCode::InvalidArgument, "no such edge of the surface")
                .with_context(format!("triangle={triangle} edge={edge}")),
        );
    }
    let t = triangle as usize;
    let k = edge as usize;
    let tri = surface.triangles()[t];
    let a = tri[k];
    let b = tri[(k + 1) % 3];
    let c = tri[(k + 2) % 3];
    let v = surface.vertices();
    let at = Segment2::new(plan_of(v[a as usize]), plan_of(v[b as usize])).point_at(0.5);
    let across = surface.neighbors()[t][k];
    if across == NO_TRIANGLE {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the edge is on the rim of the surface: there is no triangle across it to flip with",
        )
        .with_context(format!("edge at {}", describe_position(at))));
    }
    if surface.is_edge_constrained(t, k) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the edge is a breakline or boundary edge, which is kept; it cannot be flipped",
        )
        .with_context(format!("edge at {}", describe_position(at))));
    }
    let other = surface.triangles()[across as usize];
    let Some(j) = (0..3).find(|&j| other[j] == b && other[(j + 1) % 3] == a) else {
        return Err(Error::new(
            ErrorCode::Internal,
            "the triangle across the edge does not share it",
        ));
    };
    let d = other[(j + 2) % 3];
    let p = |vertex: u32| plan_of(v[vertex as usize]);
    // The quadrilateral a, d, b, c is strictly convex exactly when both
    // triangles of the other diagonal turn the right way.
    if orientation(p(a), p(d), p(c)) <= 0 || orientation(p(d), p(b), p(c)) <= 0 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the two triangles do not make a convex quadrilateral; flipping the edge would fold \
             the surface over itself",
        )
        .with_context(format!("edge at {}", describe_position(at))));
    }
    let bit = |mask: u8, k: usize| (mask >> k) & 1;
    let mask_t = surface.constrained_edges()[t];
    let mask_o = surface.constrained_edges()[across as usize];
    let mut output = PatchOutput {
        vertices: v.clone(),
        triangles: surface.triangles().clone(),
        masks: surface.constrained_edges().clone(),
        ..PatchOutput::default()
    };
    // (a, d, c): a->d is the other triangle's edge j+1, d->c the new
    // diagonal, c->a this triangle's edge k+2.
    output.triangles[t] = [a, d, c];
    output.masks[t] = bit(mask_o, (j + 1) % 3) | (bit(mask_t, (k + 2) % 3) << 2);
    // (d, b, c): d->b the other's edge j+2, b->c this one's edge k+1.
    output.triangles[across as usize] = [d, b, c];
    output.masks[across as usize] = bit(mask_o, (j + 2) % 3) | (bit(mask_t, (k + 1) % 3) << 1);
    finish_patch(surface, output)
}

fn two_levels(position: Point2, first: f64, second: f64) -> Error {
    Error::new(
        ErrorCode::InvalidGeometry,
        "two points at one position with different levels",
    )
    .with_context(format!(
        "at {}: {} and {}",
        describe_position(position),
        format_exact_real(first),
        format_exact_real(second)
    ))
}

/// The surface with the points added as vertices. A point on the surface is
/// inserted where it lies and the triangles whose circumcircle it falls in
/// are re-triangulated around it (as an incremental Delaunay insertion would,
/// stopping at constrained edges); a point on a breakline splits it and keeps
/// its own level, as build_tin does. A point at an existing vertex (within
/// tolerance::GEOMETRIC) gives that vertex its level. A point off the
/// surface - outside it, or in a hole - extends it by the triangles that join
/// it to the rim edges it sees (docs/surface_editing.md, "A point off the
/// surface"). Fails with
///   InvalidArgument  no points, or a non-finite coordinate;
///   InvalidGeometry  two points at one position with levels that differ by
///                    more than tolerance::COORDINATE;
///   TriangulationFailure  from the kernel.
pub fn add_points(surface: &TinSurface, points: &[Point3]) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    if points.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no point to add was given",
        ));
    }
    for point in points {
        if !point.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a point has a non-finite coordinate",
            )
            .with_context(format!("at {}", describe_position(plan_of(*point)))));
        }
        if !within_magnitude(point.x) || !within_magnitude(point.y) || !within_magnitude(point.z) {
            return Err(too_large(&describe_position(plan_of(*point))));
        }
    }

    // Where each point lies, on the surface as it is.
    let stars = VertexStars::new(surface);
    let mut levels: Vec<Point3> = surface.vertices().clone();
    let mut given: Vec<Option<f64>> = vec![None; surface.vertex_count()];
    let mut request = PatchRequest::new();
    request.region = vec![0; surface.triangle_count()];
    let mut outside: Vec<Point3> = Vec::new();
    for point in points {
        let position = plan_of(*point);
        let place = place_of(surface, &stars, position);
        match place.kind {
            PlaceKind::Vertex => {
                // A point at a vertex gives it its level: no triangle changes.
                let level = &mut given[place.vertex as usize];
                if let Some(earlier) = *level
                    && (earlier - point.z).abs() > tolerance::COORDINATE
                {
                    return Err(two_levels(position, earlier, point.z));
                }
                *level = Some(point.z);
                levels[place.vertex as usize].z = point.z;
            }
            PlaceKind::Inside | PlaceKind::Edge => {
                mark_insertion_cavity(surface, &place, position, &mut request.region);
                request.points.push(PatchPoint {
                    position,
                    z: Some(point.z),
                    required: true,
                });
            }
            PlaceKind::Outside => outside.push(*point),
        }
    }

    let relevelled = levels
        .iter()
        .zip(surface.vertices().iter())
        .filter(|(after, before)| after.z != before.z)
        .count();
    let mut current = surface.clone();
    if relevelled > 0 {
        current = with_levels(surface, levels)?;
    }
    let mut report = TinEditReport {
        points_relevelled: relevelled,
        ..TinEditReport::default()
    };
    if !request.points.is_empty() {
        // The relevelled surface has the same triangles, so the stars and the
        // cavities found on the original hold for it.
        let output = rebuild_patch(&current, &stars, request, None)?;
        let step = finish_patch(&current, output)?;
        report.points_added += step.report.points_added;
        report.points_relevelled += step.report.points_relevelled;
        report.crossings += step.report.crossings;
        current = step.surface;
    }
    if !outside.is_empty() {
        let output = extend_surface(&current, &outside)?;
        let step = finish_patch(&current, output)?;
        report.points_added += step.report.points_added;
        report.points_relevelled += step.report.points_relevelled;
        current = step.surface;
    }
    Ok(TinEditResult {
        report: with_counts(report, surface, &current),
        surface: current,
    })
}

/// The line made constrained edges of the surface. A vertex with a level is a
/// 3D point the surface is pulled to (an existing vertex there takes it); one
/// without takes the surface's level. A stretch between two vertices that both
/// lack a level follows the surface: a vertex is made wherever it crosses an
/// edge, at the edge's level, so the surface keeps its shape and only gains
/// the edges. A stretch with a level at either end is straight in 3D. Where
/// the line crosses an existing breakline it meets it at that breakline's
/// level. A vertex of the surface lying exactly on a 3D stretch takes the
/// stretch's level, so the line is straight end to end (one on an existing
/// breakline keeps its level, as where they cross). A vertex of the line that
/// is within tolerance::GEOMETRIC of an existing edge is on it: the edge is
/// split there. A position the line visits twice has the level either visit
/// gives it. Fails with
///   InvalidArgument  fewer than two vertices, a non-finite coordinate or level,
///                    one beyond MOST_MAGNITUDE;
///   InvalidGeometry  a vertex off the surface, the line leaving the surface
///                    (across the rim or a hole), the line crossing itself at
///                    two levels, two levels at one position, an edge passing
///                    so close to the end of another that the crossing cannot
///                    be placed on it, or no length at all;
///   TriangulationFailure  from the kernel.
pub fn add_breakline(
    surface: &TinSurface,
    vertices: &[BreaklineVertex],
    closed: bool,
) -> Result<TinEditResult> {
    let line = BreaklineInput {
        vertices: vertices.to_vec(),
        closed,
    };
    add_breaklines(surface, std::slice::from_ref(&line))
}

/// Several lines made constrained edges in ONE patch, so lines of the same
/// edit that cross are checked against each other as the two halves of one
/// line are: 3D lines crossing at different levels are refused (the answer
/// does not depend on the order they are named), and a draped line meets a 3D
/// one at its level. Refusals as add_breakline's, for any of the lines.
pub fn add_breaklines(surface: &TinSurface, lines: &[BreaklineInput]) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    if lines.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no breakline was given",
        ));
    }
    let stars = VertexStars::new(surface);
    let mut request = PatchRequest::new();
    request.region = vec![0; surface.triangle_count()];
    // Which line a refusal is about, when there are several.
    let where_ = |line: usize, vertex: usize| {
        let prefix = if lines.len() > 1 {
            format!("line={line} ")
        } else {
            String::new()
        };
        format!("{prefix}vertex={vertex}")
    };
    let which_line = |line: usize| {
        if lines.len() > 1 {
            format!("line={line}")
        } else {
            String::new()
        }
    };
    for (l, input) in lines.iter().enumerate() {
        // The levels given to a position this line visits more than once are
        // one: each visit takes the level either gives it, and the segments
        // are judged draped or not by the levels they then have (a line that
        // comes back to a point and gives its level on the second visit is
        // straight 3D to it from the first, as the same line written the
        // other way round is).
        let mut vertices: Vec<BreaklineVertex> = input.vertices.clone();
        if vertices.len() < 2 {
            let line_part = if lines.len() > 1 {
                format!(" line={l}")
            } else {
                String::new()
            };
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a breakline needs at least two vertices",
            )
            .with_context(format!("vertices={}{line_part}", vertices.len())));
        }
        let mut merger = PointMerger::new(vertices.len(), tolerance::GEOMETRIC);
        let mut position: Vec<u32> = vec![0; vertices.len()];
        let mut level_at: Vec<Option<f64>> = Vec::new();
        for (i, vertex) in vertices.iter().enumerate() {
            if !vertex.position.is_finite() || vertex.z.is_some_and(|z| !z.is_finite()) {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a breakline vertex has a non-finite coordinate",
                )
                .with_context(where_(l, i)));
            }
            if !within_magnitude(vertex.position.x)
                || !within_magnitude(vertex.position.y)
                || vertex.z.is_some_and(|z| !within_magnitude(z))
            {
                return Err(too_large(&describe_position(vertex.position)));
            }
            let entry = merger.add(vertex.position);
            position[i] = entry.index;
            if entry.inserted {
                level_at.push(None);
            }
            if let Some(z) = vertex.z {
                let level = &mut level_at[entry.index as usize];
                if let Some(earlier) = *level
                    && (earlier - z).abs() > tolerance::COORDINATE
                {
                    return Err(two_levels(vertex.position, earlier, z));
                }
                if level.is_none() {
                    *level = Some(z);
                }
            }
        }
        for (vertex, &at) in vertices.iter_mut().zip(position.iter()) {
            if vertex.z.is_none() {
                vertex.z = level_at[at as usize];
            }
        }

        let base = request.points.len() as u32;
        let mut has_length = false;
        for (i, vertex) in vertices.iter().enumerate() {
            let place = place_of(surface, &stars, vertex.position);
            if place.kind == PlaceKind::Outside {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "a breakline vertex lies off the surface",
                )
                .with_context(format!(
                    "{} at {}",
                    where_(l, i),
                    describe_position(vertex.position)
                )));
            }
            // A draped vertex goes in without a level: the patch gives it the
            // surface's, and one at an existing vertex is that vertex, untouched.
            request.points.push(PatchPoint {
                position: vertex.position,
                z: vertex.z,
                required: true,
            });
            has_length = has_length
                || vertex.position.distance_to(vertices[0].position) > tolerance::GEOMETRIC;
        }
        if !has_length {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the breakline has no length: its vertices coincide",
            )
            .with_context(which_line(l)));
        }
        let segments = if input.closed && vertices.len() > 2 {
            vertices.len()
        } else {
            vertices.len() - 1
        };
        for s in 0..segments {
            let from = s;
            let to = (s + 1) % vertices.len();
            let draped = vertices[from].z.is_none() && vertices[to].z.is_none();
            request.lines.push(PatchSegment {
                from: base + from as u32,
                to: base + to as u32,
                draped,
            });
            request.keep_shape = request.keep_shape || draped;
        }
    }
    let output = rebuild_patch(surface, &stars, request, None)?;
    finish_patch(surface, output)
}

#[derive(Clone, Copy)]
struct Piece {
    segment: Segment2,
    bounds: Box2,
}

/// The constrained edges that lie along any of `lines` made ordinary edges, so
/// a breakline the surface was given (or a seam) is no longer one: the triangles
/// and the vertices are exactly as they were, only the flags go, on both sides
/// of each edge. An edge lies along a line when both its ends are within
/// tolerance::GEOMETRIC of ONE segment of it (a line's closing segment
/// included when it is closed), so a line over part of a breakline clears the
/// edges of that part and no other, and a vertex of the line need not be a vertex
/// of the surface. The levels of the line are not asked. An edge on the rim is
/// never cleared - a constrained rim edge is the surface's boundary, which
/// BOUNDARY and HOLE make - and is counted in `edges_held`. A vertex whose every
/// constrained edge was cleared can then be deleted (delete_points). No edge
/// along the lines is no change: the surface as it was. Fails with
///   InvalidArgument  no line, a line of fewer than two vertices, a coordinate
///                    that is not finite or is beyond MOST_MAGNITUDE;
///   InvalidGeometry  lines whose vertices all coincide.
pub fn delete_breaklines(surface: &TinSurface, lines: &[Polyline2]) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    if lines.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, "no line was given"));
    }
    // The segments of the lines, each with its box grown by the tolerance.
    let mut pieces: Vec<Piece> = Vec::new();
    for (l, line) in lines.iter().enumerate() {
        let which = if lines.len() > 1 {
            format!(" line={l}")
        } else {
            String::new()
        };
        if line.vertices.len() < 2 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a line needs at least two vertices",
            )
            .with_context(format!("vertices={}{which}", line.vertices.len())));
        }
        for &vertex in &line.vertices {
            if !vertex.is_finite() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a line has a non-finite coordinate",
                )
                .with_context(which));
            }
            if !within_magnitude(vertex.x) || !within_magnitude(vertex.y) {
                return Err(too_large(&describe_position(vertex)));
            }
        }
        let count = if line.closed && line.vertices.len() > 2 {
            line.vertices.len()
        } else {
            line.vertices.len() - 1
        };
        for i in 0..count {
            let segment = Segment2::new(
                line.vertices[i],
                line.vertices[(i + 1) % line.vertices.len()],
            );
            if segment.length() <= tolerance::GEOMETRIC {
                continue; // a repeated vertex: no length, and along no edge
            }
            pieces.push(Piece {
                segment,
                bounds: segment.bounding_box().inflated(tolerance::GEOMETRIC),
            });
        }
    }
    if pieces.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the line has no length: its vertices coincide",
        ));
    }
    let lies = |a: Point2, b: Point2| {
        pieces.iter().any(|piece| {
            piece.bounds.contains(a)
                && piece.bounds.contains(b)
                && piece.segment.distance_to(a) <= tolerance::GEOMETRIC
                && piece.segment.distance_to(b) <= tolerance::GEOMETRIC
        })
    };

    let vertices = surface.vertices();
    let triangles = surface.triangles();
    let mut masks = surface.constrained_edges().clone();
    let mut cleared = 0usize;
    let mut held = 0usize;
    for t in 0..surface.triangle_count() {
        for k in 0..3 {
            if !surface.is_edge_constrained(t, k) {
                continue;
            }
            let across = surface.neighbors()[t][k];
            // Each edge once: a rim edge is only ever seen from its one triangle,
            // an interior one from the lower of its two.
            if across != NO_TRIANGLE && (across as usize) < t {
                continue;
            }
            let a = triangles[t][k];
            let b = triangles[t][(k + 1) % 3];
            if !lies(plan_of(vertices[a as usize]), plan_of(vertices[b as usize])) {
                continue;
            }
            if across == NO_TRIANGLE {
                held += 1;
                continue;
            }
            let other = triangles[across as usize];
            let Some(j) = (0..3).find(|&j| other[j] == b && other[(j + 1) % 3] == a) else {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "the triangle across the edge does not share it",
                ));
            };
            masks[t] &= !(1u8 << k);
            masks[across as usize] &= !(1u8 << j);
            cleared += 1;
        }
    }
    if cleared == 0 {
        let mut result = unchanged(surface);
        result.report.edges_held = held;
        return Ok(result);
    }
    let cleaned = TinSurface::assemble(
        vertices.clone(),
        triangles.clone(),
        surface.neighbors().clone(),
        masks,
    )?;
    let mut report = report_of(surface, &cleaned);
    report.edges_cleared = cleared;
    report.edges_held = held;
    Ok(TinEditResult {
        surface: cleaned,
        report,
    })
}

/// A polygon as a cut takes it: cleaned (clean_polygon), with its box and its
/// point test made once.
struct CutRing {
    ring: Polyline2,
    bounds: Box2,
    shape: PolygonClassifier,
}

fn make_cut_ring(polygon: &Polyline2, what: &str) -> Result<CutRing> {
    let ring = clean_polygon(polygon, what)?;
    Ok(CutRing {
        bounds: ring.bounding_box(),
        shape: PolygonClassifier::new(&ring),
        ring,
    })
}

/// The surface cut along every ring in ONE patch: what lies inside any of them
/// is dropped (keep_inside false: a hole), or what lies inside none is (a
/// boundary, which is only ever asked with one ring). The rings must not
/// overlap, touch or nest - two of them that did would be judged by the edges
/// of each alone, and the piece between them taken for outside both
/// (cut_holes decides which footprints may share a patch). `stars` and `grid`
/// are the surface's, when the caller has them already.
fn cut_rings(
    surface: &TinSurface,
    rings: &[CutRing],
    keep_inside: bool,
    stars: Option<&VertexStars>,
    grid: Option<&TriangleGrid>,
) -> Result<TinEditResult> {
    let own_stars;
    let stars = match stars {
        Some(stars) => stars,
        None => {
            own_stars = VertexStars::new(surface);
            &own_stars
        }
    };
    let mut request = PatchRequest::new();
    request.region = vec![0; surface.triangle_count()];
    request.keep_shape = true;
    request.keep_inside_cut = keep_inside;
    request.simple_cut = true;
    for cut in rings {
        let first = request.points.len() as u32;
        let count = cut.ring.vertices.len() as u32;
        for i in 0..count {
            request.points.push(PatchPoint {
                position: cut.ring.vertices[i as usize],
                z: None,
                required: false,
            });
            request.cuts.push(PatchSegment {
                from: first + i,
                to: first + (i + 1) % count,
                draped: false,
            });
        }
    }
    // Inside any ring: the rings are apart, so the union is what each one says.
    request.inside_cut = Some(Box::new(move |p: Point2| {
        rings
            .iter()
            .any(|cut| cut.shape.classify(p) != Containment::Outside)
    }));
    let output = rebuild_patch(surface, stars, request, grid)?;
    if output.triangles.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            if keep_inside {
                "nothing of the surface lies inside the boundary"
            } else {
                "the hole covers the whole surface; nothing would be left"
            },
        ));
    }
    // Nothing rebuilt and nothing dropped - the polygon is round the whole
    // surface, or beside it: the surface as it was, not a reordered copy.
    if output.triangles == *surface.triangles() {
        return Ok(unchanged(surface));
    }
    finish_patch(surface, output)
}

fn cut_by(surface: &TinSurface, polygon: &Polyline2, keep_inside: bool) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    let ring = make_cut_ring(
        polygon,
        if keep_inside {
            "the boundary"
        } else {
            "the hole"
        },
    )?;
    cut_rings(
        surface,
        std::slice::from_ref(&ring),
        keep_inside,
        None,
        None,
    )
}

/// What a run of cuts did, summed: the first's "before", the last's "after",
/// the counts of all (the verb's report of a run of edits, one place).
fn add_report(total: &mut TinEditReport, step: &TinEditReport, first: bool) {
    if first {
        *total = *step;
        return;
    }
    total.triangles_after = step.triangles_after;
    total.vertices_after = step.vertices_after;
    total.points_added += step.points_added;
    total.points_removed += step.points_removed;
    total.points_relevelled += step.points_relevelled;
    total.points_moved += step.points_moved;
    total.points_held += step.points_held;
    total.crossings += step.crossings;
    total.largest_change = cpp::max(total.largest_change, step.largest_change);
}

/// Whether two segments come within GEOMETRIC of each other: they meet, or an
/// end of one is that close to the other (the nearest points of two segments
/// that do not meet include an end of one of them).
fn segments_close(s: &Segment2, t: &Segment2) -> bool {
    segments_meet(s.start, s.end, t.start, t.end)
        || t.distance_to(s.start) <= tolerance::GEOMETRIC
        || t.distance_to(s.end) <= tolerance::GEOMETRIC
        || s.distance_to(t.start) <= tolerance::GEOMETRIC
        || s.distance_to(t.end) <= tolerance::GEOMETRIC
}

/// Whether the insides of two cut rings are apart: the outlines nowhere within
/// GEOMETRIC of one another and neither ring inside the other. Boxes first;
/// then only the edges that reach the other ring's box are laid against each
/// other.
fn rings_apart(a: &CutRing, b: &CutRing) -> bool {
    let box_a = a.bounds.inflated(tolerance::GEOMETRIC);
    let box_b = b.bounds.inflated(tolerance::GEOMETRIC);
    if !box_a.intersects(box_b) {
        return true;
    }
    let edges_reaching = |ring: &Polyline2, bounds: Box2| {
        let n = ring.vertices.len();
        let mut edges: Vec<(Segment2, Box2)> = Vec::new();
        for i in 0..n {
            let segment = Segment2::new(ring.vertices[i], ring.vertices[(i + 1) % n]);
            let grown = segment.bounding_box().inflated(tolerance::GEOMETRIC);
            if grown.intersects(bounds) {
                edges.push((segment, grown));
            }
        }
        edges
    };
    let mine = edges_reaching(&a.ring, box_b);
    let theirs = edges_reaching(&b.ring, box_a);
    for (s, s_box) in &mine {
        for (t, t_box) in &theirs {
            if s_box.intersects(*t_box) && segments_close(s, t) {
                return false;
            }
        }
    }
    // The outlines are apart, so a ring is wholly inside the other or wholly
    // outside it, and any one of its vertices says which.
    b.shape.classify(a.ring.vertices[0]) == Containment::Outside
        && a.shape.classify(b.ring.vertices[0]) == Containment::Outside
}

/// Cutting the rings one at a time, each on the surface the one before left:
/// what cut_hole in a loop gives, and what cut_holes falls back to.
fn cut_rings_in_turn(
    surface: &TinSurface,
    rings: &[CutRing],
    patches: &mut usize,
) -> Result<TinEditResult> {
    let mut total = TinEditResult {
        surface: surface.clone(),
        report: TinEditReport::default(),
    };
    let mut first = true;
    for cut in rings {
        *patches += 1;
        let step = cut_rings(&total.surface, std::slice::from_ref(cut), false, None, None)?;
        add_report(&mut total.report, &step.report, first);
        first = false;
        total.surface = step.surface;
    }
    Ok(total)
}

/// The part of the surface inside `boundary` (an outer clip). The polygon's
/// edges become the new rim, exactly: triangles they cross are cut along them,
/// the new vertices lying on the old surface, so the surface inside keeps its
/// shape; the new rim edges are constrained, as build_tin's boundary edges are.
/// Either orientation. Fails with
///   InvalidArgument  a non-finite coordinate;
///   InvalidGeometry  fewer than 3 distinct vertices, no area, or a polygon
///                    that touches or crosses itself (a corner it visits
///                    twice, or one on another edge, included);
///   InvalidState     nothing of the surface would be left;
///   TriangulationFailure  from the kernel.
/// A polygon that changes nothing (a boundary around the whole surface, a
/// hole beside it) returns the surface as it was.
pub fn clip_to_boundary(surface: &TinSurface, boundary: &Polyline2) -> Result<TinEditResult> {
    cut_by(surface, boundary, true)
}

/// The part of the surface outside `hole`; as clip_to_boundary otherwise.
pub fn cut_hole(surface: &TinSurface, hole: &Polyline2) -> Result<TinEditResult> {
    cut_by(surface, hole, false)
}

/// The triangles a ring's outline reaches: the rule the patch grows its
/// region by (mark_reached_by_point, mark_along), so a footprint that shares
/// none with the batch has a region of its own.
fn reach_of(
    surface: &TinSurface,
    stars: &VertexStars,
    grid: &TriangleGrid,
    cut: &CutRing,
    scratch: &mut [u8],
) -> Vec<u32> {
    let mut reached: Vec<u32> = Vec::new();
    let corners = &cut.ring.vertices;
    for &corner in corners {
        mark_reached_by_point(surface, stars, corner, scratch, Some(&mut reached));
    }
    for i in 0..corners.len() {
        grid.mark_along(
            corners[i],
            corners[(i + 1) % corners.len()],
            scratch,
            Some(&mut reached),
        );
    }
    for &t in &reached {
        scratch[t as usize] = 0;
    }
    reached
}

/// cut_hole of each polygon in turn, each on the surface the one before left:
/// the surface, and a report that is the first step's "before", the last's
/// "after" and the counts of all summed (largest_change the greatest). The
/// same surface as that loop gives - the same triangles, the same vertices,
/// the same constrained edges, to within the numbering of the vertices the
/// edit made - but footprints that can be cut together are cut in ONE patch,
/// so 100 buildings cost one whole-surface rebuild, not 100.
///
/// Footprints share a patch when each is separate from every other in the
/// batch - further than tolerance::GEOMETRIC from it everywhere, neither
/// inside the other - and no triangle of the surface is reached by two of
/// their edges. Anything else (overlapping, touching, nested, identical
/// footprints, two in one triangle) is cut in a patch of its own, in the
/// order the polygons are given, as it always was. A refusal is the loop's
/// refusal: when a batch fails it is cut again one polygon at a time and the
/// first refusal is the answer. Fails with
///   InvalidArgument  no polygon, or a coordinate that is not finite or is
///                    beyond MOST_MAGNITUDE;
///   InvalidGeometry  a polygon with fewer than 3 distinct vertices, no area,
///                    or that touches or crosses itself;
///   InvalidState     everything of the surface would be cut away;
///   TriangulationFailure  from the kernel.
/// A polygon that changes nothing leaves the surface as it was, as cut_hole's.
pub fn cut_holes(
    surface: &TinSurface,
    holes: &[Polyline2],
    stats: Option<&mut HoleCutStats>,
) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    if holes.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, "no hole was given"));
    }
    if holes.len() == 1 {
        // Nothing to share a patch with: cut_hole, without the bookkeeping.
        let only = cut_by(surface, &holes[0], false)?;
        if let Some(stats) = stats {
            stats.patches = 1;
        }
        return Ok(only);
    }

    let mut total_surface = surface.clone();
    let mut total_report = TinEditReport::default();
    let mut any_step = false;
    let mut patches = 0usize;
    // A footprint that did not fit the batch before: it starts the next one,
    // its reach found again on the surface that batch left.
    let mut carried: Option<CutRing> = None;
    let mut next = 0usize;
    loop {
        // The batch being gathered, and what is made once for the surface it
        // starts from: where the triangles are (stars and grid, which also
        // serve the patch itself), and which of them the batch's outlines reach.
        let mut refusal: Option<Error> = None;
        let outcome: Option<Result<TinEditResult>> = {
            let stars = VertexStars::new(&total_surface);
            let grid = TriangleGrid::new(&total_surface);
            let mut taken = vec![0u8; total_surface.triangle_count()];
            let mut scratch = vec![0u8; total_surface.triangle_count()];
            let mut batch: Vec<CutRing> = Vec::new();
            if let Some(cut) = carried.take() {
                for t in reach_of(&total_surface, &stars, &grid, &cut, &mut scratch) {
                    taken[t as usize] = 1;
                }
                batch.push(cut);
            }
            while next < holes.len() {
                let cut = match make_cut_ring(&holes[next], "the hole") {
                    Ok(cut) => cut,
                    Err(error) => {
                        // The footprints before this one are cut first, as a
                        // loop would have: their refusal comes before this one's.
                        refusal = Some(error);
                        next += 1;
                        break;
                    }
                };
                next += 1;
                let reached = reach_of(&total_surface, &stars, &grid, &cut, &mut scratch);
                let fits = !reached.iter().any(|&t| taken[t as usize] != 0)
                    && batch.iter().all(|member| rings_apart(member, &cut));
                if !batch.is_empty() && !fits {
                    carried = Some(cut);
                    break;
                }
                for t in reached {
                    taken[t as usize] = 1;
                }
                batch.push(cut);
            }
            if batch.is_empty() {
                None
            } else {
                patches += 1;
                let mut result =
                    cut_rings(&total_surface, &batch, false, Some(&stars), Some(&grid));
                if result.is_err() && batch.len() > 1 {
                    // The refusal of a loop is the answer, not the batch's: cut the
                    // footprints one at a time to meet the first one that fails.
                    result = cut_rings_in_turn(&total_surface, &batch, &mut patches);
                }
                Some(result)
            }
        };
        if let Some(result) = outcome {
            let result = result?;
            add_report(&mut total_report, &result.report, !any_step);
            any_step = true;
            total_surface = result.surface;
        }
        if let Some(error) = refusal {
            return Err(error);
        }
        if carried.is_none() && next >= holes.len() {
            break;
        }
    }
    if let Some(stats) = stats {
        stats.patches = patches;
    }
    Ok(TinEditResult {
        surface: total_surface,
        report: total_report,
    })
}

// ---- changing levels ------------------------------------------------------------------------

/// Every vertex's level changed, or only those inside `inside` (on its edge
/// counts). The triangles are not touched, so a triangle with corners on
/// both sides of the polygon slopes between them; a breakline along the
/// polygon first (add_breakline) makes the change start exactly at it. Fails
/// with InvalidArgument for a non-finite value or base, a level beyond
/// MOST_MAGNITUDE, and for a polygon with no vertex of the surface inside it.
pub fn change_heights(
    surface: &TinSurface,
    change: &HeightChange,
    inside: Option<&Polyline2>,
) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    if !change.value.is_finite() || change.base.is_some_and(|base| !base.is_finite()) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a level change must be a finite number",
        ));
    }
    if !within_magnitude(change.value) || change.base.is_some_and(|base| !within_magnitude(base)) {
        return Err(too_large(&format!(
            "the level change {}",
            format_exact_real(change.value)
        )));
    }
    let ring = match inside {
        Some(polygon) => Some(PolygonClassifier::new(&clean_polygon(
            polygon,
            "the polygon",
        )?)),
        None => None,
    };
    let used = referenced_vertices(surface);
    let affected: Vec<u32> = (0..surface.vertex_count())
        .filter(|&v| {
            used[v] != 0
                && ring.as_ref().is_none_or(|ring| {
                    ring.classify(plan_of(surface.vertices()[v])) != Containment::Outside
                })
        })
        .map(|v| v as u32)
        .collect();
    if affected.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no vertex of the surface lies inside the polygon",
        ));
    }
    let mut base = 0.0;
    if change.operation == HeightOperation::Scale {
        base = change.base.unwrap_or_else(|| {
            affected.iter().fold(f64::INFINITY, |lowest, &v| {
                cpp::min(lowest, surface.vertices()[v as usize].z)
            })
        });
    }
    let mut vertices = surface.vertices().clone();
    let mut report = TinEditReport::default();
    for &v in &affected {
        let z = vertices[v as usize].z;
        let next = match change.operation {
            HeightOperation::Raise => z + change.value,
            HeightOperation::SetLevel => change.value,
            HeightOperation::Scale => base + change.value * (z - base),
        };
        if !next.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the change makes a level non-finite",
            )
            .with_context(format!("vertex={v}")));
        }
        if !within_magnitude(next) {
            return Err(too_large(&format!(
                "vertex={v} would be at level {}",
                format_exact_real(next)
            )));
        }
        if next != z {
            report.points_moved += 1;
            report.largest_change = cpp::max(report.largest_change, (next - z).abs());
        }
        vertices[v as usize].z = next;
    }
    let changed = with_levels(surface, vertices)?;
    Ok(TinEditResult {
        report: with_counts(report, surface, &changed),
        surface: changed,
    })
}

/// Laplacian smoothing of the levels: each pass moves every free vertex
/// `weight` of the way to the mean level of the vertices it shares an edge
/// with, all at once (one pass reads only the levels before it, so the
/// result does not depend on the order of the vertices). Vertices on the rim
/// and on constrained edges are held. Plan positions never move. Fails with
/// InvalidArgument for passes outside 1..1000, a weight outside (0, 1], and a
/// polygon with no free vertex inside it.
pub fn smooth_surface(
    surface: &TinSurface,
    options: &SmoothOptions,
    inside: Option<&Polyline2>,
) -> Result<TinEditResult> {
    require_surface(surface, "the surface")?;
    // A thousand passes of the mildest weight move a vertex further than any
    // smoothing a surveyor asks for; more is a typing slip, not a request.
    const MOST_PASSES: i32 = 1000;
    if !(1..=MOST_PASSES).contains(&options.passes) {
        return Err(
            Error::new(ErrorCode::InvalidArgument, "passes must be from 1 to 1000")
                .with_context(format!("passes={}", options.passes)),
        );
    }
    if !(options.weight > 0.0 && options.weight <= 1.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "weight must be above 0 and at most 1",
        )
        .with_context(format!("weight={}", format_exact_real(options.weight))));
    }
    let ring = match inside {
        Some(polygon) => Some(PolygonClassifier::new(&clean_polygon(
            polygon,
            "the polygon",
        )?)),
        None => None,
    };

    // Each vertex's neighbours along an edge, once each (two flat arrays).
    let count = surface.vertex_count();
    let mut pairs: Vec<(u32, u32)> = Vec::with_capacity(6 * surface.triangle_count());
    for tri in surface.triangles() {
        for k in 0..3 {
            pairs.push((tri[k], tri[(k + 1) % 3]));
            pairs.push((tri[(k + 1) % 3], tri[k]));
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    let mut start: Vec<u32> = vec![0; count + 1];
    for &(first, _) in &pairs {
        start[first as usize + 1] += 1;
    }
    for v in 0..count {
        start[v + 1] += start[v];
    }

    let held = held_vertices(surface);
    let used = referenced_vertices(surface);
    let mut free = vec![0u8; count];
    let mut report = TinEditReport::default();
    let mut free_count = 0usize;
    for v in 0..count {
        if used[v] == 0
            || ring.as_ref().is_some_and(|ring| {
                ring.classify(plan_of(surface.vertices()[v])) == Containment::Outside
            })
        {
            continue;
        }
        if held[v] != 0 {
            report.points_held += 1;
            continue;
        }
        free[v] = 1;
        free_count += 1;
    }
    if free_count == 0 && ring.is_some() {
        // The same words change_heights uses for a polygon with no vertex in
        // it: "every one is held" is a claim about vertices that exist.
        if report.points_held == 0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "no vertex of the surface lies inside the polygon",
            ));
        }
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no vertex inside the polygon is free to move: every one is on the rim or a \
             breakline",
        )
        .with_context(format!("held={}", report.points_held)));
    }

    let mut z: Vec<f64> = surface.vertices().iter().map(|vertex| vertex.z).collect();
    let mut next = z.clone();
    for _ in 0..options.passes {
        for v in 0..count {
            if free[v] == 0 || start[v + 1] == start[v] {
                continue;
            }
            let mut sum = 0.0;
            for &(_, neighbour) in &pairs[start[v] as usize..start[v + 1] as usize] {
                sum += z[neighbour as usize];
            }
            let mean = sum / f64::from(start[v + 1] - start[v]);
            next[v] = z[v] + options.weight * (mean - z[v]);
        }
        z.clone_from(&next);
    }
    let mut vertices = surface.vertices().clone();
    for (vertex, &level) in vertices.iter_mut().zip(z.iter()) {
        if level != vertex.z {
            report.points_moved += 1;
            report.largest_change = cpp::max(report.largest_change, (level - vertex.z).abs());
            vertex.z = level;
        }
    }
    let smoothed = with_levels(surface, vertices)?;
    Ok(TinEditResult {
        report: with_counts(report, surface, &smoothed),
        surface: smoothed,
    })
}

// ---- surfaces made of others ----------------------------------------------------------------

/// The vertex of the result a top vertex is: its seam vertex, or a copy of it
/// made on first use.
fn mapped_vertex(
    v: u32,
    result_of: &mut [u32],
    vertices: &mut Vec<Point3>,
    top_vertices: &[Point3],
) -> u32 {
    if result_of[v as usize] == NO_VERTEX {
        result_of[v as usize] = vertices.len() as u32;
        vertices.push(top_vertices[v as usize]);
    }
    result_of[v as usize]
}

/// The surface `top` laid into `base`: inside the top's footprint the top is
/// the surface, everywhere else the base, joined along the top's rim by a
/// clean seam - the base cut exactly along the rim, every seam vertex on the
/// top's rim at the top's level, the seam edges constrained. The base's
/// triangles the rim crosses are cut, not dropped whole (compare
/// combine_surfaces). Where the top reaches past the base it is kept as it is.
/// Fails with InvalidArgument for an empty top or base, and with
/// TriangulationFailure from the kernel.
pub fn paste_surface(base: &TinSurface, top: &TinSurface) -> Result<TinEditResult> {
    require_surface(base, "the base surface")?;
    require_surface(top, "the top surface")?;
    let top_vertices = top.vertices();
    let top_triangles = top.triangles();

    // The top's rim, each edge directed with the top on its left: the cut.
    let mut point_of_vertex: Vec<u32> = vec![NO_VERTEX; top.vertex_count()];
    let mut cut_of_edge: Vec<u32> = vec![NO_VERTEX; 3 * top.triangle_count()];
    let mut request = PatchRequest::new();
    request.region = vec![0; base.triangle_count()];
    for t in 0..top.triangle_count() {
        for k in 0..3 {
            if top.neighbors()[t][k] != NO_TRIANGLE {
                continue;
            }
            let mut ends = [top_triangles[t][k], top_triangles[t][(k + 1) % 3]];
            for end in &mut ends {
                if point_of_vertex[*end as usize] == NO_VERTEX {
                    point_of_vertex[*end as usize] = request.points.len() as u32;
                    request.points.push(PatchPoint {
                        position: plan_of(top_vertices[*end as usize]),
                        z: Some(top_vertices[*end as usize].z),
                        required: false,
                    });
                }
                *end = point_of_vertex[*end as usize];
            }
            cut_of_edge[3 * t + k] = request.cuts.len() as u32;
            request.cuts.push(PatchSegment {
                from: ends[0],
                to: ends[1],
                draped: false,
            });
        }
    }
    request.keep_shape = true;
    request.keep_inside_cut = false;
    request.cut_has_levels = true;
    request.points_move_vertices = true;
    request.inside_cut = Some(Box::new(|p: Point2| top.locate(p).is_some()));

    let stars = VertexStars::new(base);
    let mut output = rebuild_patch(base, &stars, request, None)?;

    // The top's vertices: its rim's are the seam's, the rest are its own.
    let mut result_of: Vec<u32> = vec![NO_VERTEX; top.vertex_count()];
    for (v, &point) in point_of_vertex.iter().enumerate() {
        if point != NO_VERTEX {
            result_of[v] = output.point_vertex[point as usize];
        }
    }
    // The patch takes two points within GEOMETRIC for one, so two vertices of
    // the top's rim that close to each other - or to one vertex of the base -
    // come out as one vertex, and a top triangle between them names it twice:
    // TinSurface::create refused that as "triangle names a vertex twice",
    // naming the triangle and not the cause. Said here, with the vertices. (Two
    // rim vertices that merge but share no triangle - a rim that touches itself
    // at a duplicated vertex - are one vertex of a valid surface, and pass.)
    for tri in top_triangles {
        for k in 0..3 {
            let a = tri[k];
            let b = tri[(k + 1) % 3];
            if a != b
                && result_of[a as usize] != NO_VERTEX
                && result_of[a as usize] == result_of[b as usize]
            {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    format!(
                        "two vertices of the top surface's rim land on one vertex of the result: \
                         they are closer to each other, or to one vertex of the base, than the \
                         coincidence tolerance ({} m), and the top's triangle between them would \
                         have no area; delete one of them from the top first",
                        format_exact_real(tolerance::GEOMETRIC)
                    ),
                )
                .with_context(format!(
                    "top vertices {a} and {b} at {} and {}",
                    describe_position(plan_of(top_vertices[a as usize])),
                    describe_position(plan_of(top_vertices[b as usize]))
                )));
            }
        }
    }
    for t in 0..top.triangle_count() {
        let tri: TinTriangle = top_triangles[t];
        let mask = top.constrained_edges()[t];
        // The vertices along each edge: a rim edge split where the seam met
        // the base's edges, the others as they are.
        let mut sides: [Vec<u32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut split = 0usize;
        let mut split_edge = 0usize;
        for k in 0..3 {
            let cut = cut_of_edge[3 * t + k];
            if cut != NO_VERTEX && output.cut_vertices[cut as usize].len() > 2 {
                sides[k] = output.cut_vertices[cut as usize].clone();
                split += 1;
                split_edge = k;
            } else {
                let first =
                    mapped_vertex(tri[k], &mut result_of, &mut output.vertices, top_vertices);
                let second = mapped_vertex(
                    tri[(k + 1) % 3],
                    &mut result_of,
                    &mut output.vertices,
                    top_vertices,
                );
                sides[k] = vec![first, second];
            }
        }
        let flag = |k: usize| (mask >> k) & 1;
        if split == 0 {
            output
                .triangles
                .push([sides[0][0], sides[1][0], sides[2][0]]);
            output.masks.push(mask);
        } else if split == 1 {
            // A fan from the corner opposite the split edge.
            let run = &sides[split_edge];
            let opposite = mapped_vertex(
                tri[(split_edge + 2) % 3],
                &mut result_of,
                &mut output.vertices,
                top_vertices,
            );
            for i in 0..run.len() - 1 {
                let mut m = flag(split_edge);
                if i + 2 == run.len() {
                    m |= flag((split_edge + 1) % 3) << 1;
                }
                if i == 0 {
                    m |= flag((split_edge + 2) % 3) << 2;
                }
                output.triangles.push([run[i], run[i + 1], opposite]);
                output.masks.push(m);
            }
        } else {
            // Two or three sides split: a fan from the centroid, which lies on
            // the top's own plane there, so the top keeps its shape.
            let a = top_vertices[tri[0] as usize];
            let b = top_vertices[tri[1] as usize];
            let c = top_vertices[tri[2] as usize];
            let centre = output.vertices.len() as u32;
            output.vertices.push(Point3::new(
                (a.x + b.x + c.x) / 3.0,
                (a.y + b.y + c.y) / 3.0,
                (a.z + b.z + c.z) / 3.0,
            ));
            for (k, side) in sides.iter().enumerate() {
                for pair in side.windows(2) {
                    output.triangles.push([pair[0], pair[1], centre]);
                    output.masks.push(flag(k));
                }
            }
        }
    }
    // The seam is constrained on the base's side (its cut edges); finish_surface
    // carries the flag across to the top's. A centroid is a vertex the fans
    // use, so finish_patch counts it as added with the others: counting it
    // again here made vertices_after - vertices_before + points_removed differ
    // from points_added by one for each fan.
    finish_patch(base, output)
}

/// A copy raised by `dz` (change_heights with Raise over the whole surface).
pub fn offset_surface(surface: &TinSurface, dz: f64) -> Result<TinEditResult> {
    change_heights(
        surface,
        &HeightChange {
            operation: HeightOperation::Raise,
            value: dz,
            base: None,
        },
        None,
    )
}

/// combine_surfaces (super_surface) - later members win, a lower member's
/// triangle dropped whole where a later one covers any of it - with a report.
/// Fails with InvalidArgument for fewer than two surfaces, or an empty one,
/// and InvalidState when nothing is left.
pub fn merge_surfaces(in_rank_order: &[Option<&TinSurface>]) -> Result<TinEditResult> {
    if in_rank_order.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a merge needs at least two surfaces",
        )
        .with_context(format!("surfaces={}", in_rank_order.len())));
    }
    let mut triangles_before = 0usize;
    let mut vertices_before = 0usize;
    for (i, member) in in_rank_order.iter().enumerate() {
        let Some(member) = member.filter(|member| !member.empty()) else {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "a surface to merge is empty")
                    .with_context(format!("surface={}", i + 1)),
            );
        };
        triangles_before += member.triangle_count();
        vertices_before += member.vertex_count();
    }
    let combined = combine_surfaces(in_rank_order)?;
    if combined.empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "nothing is left of the surfaces merged",
        ));
    }
    // combine_surfaces keeps the vertices of dropped triangles; a merged
    // surface kept in the drawing has no use for them.
    let finished = finish_surface(
        combined.vertices().clone(),
        combined.triangles().clone(),
        combined.constrained_edges().clone(),
    )?;
    Ok(TinEditResult {
        report: TinEditReport {
            triangles_before,
            vertices_before,
            triangles_after: finished.triangle_count(),
            vertices_after: finished.vertex_count(),
            ..TinEditReport::default()
        },
        surface: finished,
    })
}
