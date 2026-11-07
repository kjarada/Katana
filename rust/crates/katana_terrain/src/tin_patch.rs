//! Internal to the surface edits (tin_edit; docs/surface_editing.md, "One way
//! to change a triangulation"): how part of a surface is re-triangulated while
//! the rest stays exactly as it was.
//!
//! An edit names a REGION - the triangles it must rebuild - and what goes into
//! it: points, new breakline segments, the edges of a cut. The region's
//! vertices and the new points are triangulated by the one constrained
//! Delaunay kernel (cdt_backend) with the ring of edges around the region as
//! constraints, so the new triangles meet the untouched ones edge for edge; the
//! triangles inside the ring replace the region. Which triangles are inside is
//! decided topologically - a triangle on the left of a ring edge as the
//! region's own triangle ran it is inside, and inside spreads across every edge
//! that is not a ring edge - so no point-in-polygon test with a tolerance
//! decides it.
//!
//! If something the kernel must make reaches the ring (a vertex of the surface
//! lying exactly on a ring edge, a new line leaving the region), the ring edge
//! is split and the neighbour across it could no longer be met; the region
//! then grows by that neighbour and the attempt is made again, so the region
//! is never larger than the edit needs.

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::{Segment2, Triangle2};
use katana_math::numerics::{lerp, tolerance};

use crate::cdt_backend::{self, CdtConstraint, CdtInput, CdtOutput, NO_INDEX, orientation};
use crate::point_merge::PointMerger;
use crate::tin_edit::{MOST_MAGNITUDE, TinEditReport, TinEditResult};
use crate::tin_surface::{
    Box2, NO_TRIANGLE, NO_VERTEX, Point2, Point3, TinSurface, TinTriangle, plan_of, std_clamp,
    std_max, std_max3, std_min, std_min3,
};

/// Rounds of growth before the whole surface becomes the region. A round adds
/// the neighbours across every ring edge something split, so a run of
/// vertices lying exactly on one line across the ring (a degenerate stretch
/// of an imported surface) costs a round per vertex; 32 covers any such run
/// met in practice, and the fallback after it is always correct, only slower.
const GROWTH_ROUNDS: i32 = 32;

/// The triangle grid is sized like the surface's own locator (tin_surface,
/// TRIANGLES_PER_CELL): about two triangles a cell.
const GRID_TRIANGLES_PER_CELL: f64 = 2.0;

/// GEOMETRIC with a safety factor for rounding: sixty-four ulps of the
/// coordinates' magnitude cover the error of the cross products below (a few
/// ulps of |edge| * |offset| each), as super_surface's margin does.
fn rounding_margin(scale: f64) -> f64 {
    tolerance::GEOMETRIC + 64.0 * f64::EPSILON * scale
}

fn edge_key(from: u32, to: u32) -> u64 {
    (u64::from(from) << 32) | u64::from(to)
}

/// A set of directed edges, as sorted keys.
#[derive(Default)]
struct EdgeSet {
    keys: Vec<u64>,
}

impl EdgeSet {
    fn add(&mut self, from: u32, to: u32) {
        self.keys.push(edge_key(from, to));
    }
    fn seal(&mut self) {
        self.keys.sort_unstable();
        self.keys.dedup();
    }
    fn has(&self, from: u32, to: u32) -> bool {
        self.keys.binary_search(&edge_key(from, to)).is_ok()
    }
}

fn cell_index(value: f64, origin: f64, size: f64, count: u32) -> u32 {
    let cell = ((value - origin) / size).floor();
    if !(cell > 0.0) {
        return 0;
    }
    if cell >= f64::from(count) {
        count - 1
    } else {
        cell as u32
    }
}

/// Whether the segment p-q may reach the triangle a, b, c: false only when a
/// line separates them by more than `margin`. Inexact on purpose and
/// conservative (see TriangleGrid::mark_along).
fn segment_near_triangle(
    p: Point2,
    q: Point2,
    a: Point2,
    b: Point2,
    c: Point2,
    margin: f64,
) -> bool {
    let twice_area = (b - a).cross(c - a);
    let side = if twice_area >= 0.0 { 1.0 } else { -1.0 };
    let corners = [a, b, c];
    for k in 0..3 {
        let from = corners[k];
        let d = corners[(k + 1) % 3] - from;
        let reach = margin * (d.x.abs() + d.y.abs());
        if side * d.cross(p - from) < -reach && side * d.cross(q - from) < -reach {
            return false;
        }
    }
    let d = q - p;
    let reach = margin * (d.x.abs() + d.y.abs());
    if reach > 0.0 {
        let sa = d.cross(a - p);
        let sb = d.cross(b - p);
        let sc = d.cross(c - p);
        if (sa > reach && sb > reach && sc > reach) || (sa < -reach && sb < -reach && sc < -reach) {
            return false;
        }
    }
    true
}

/// Whether the open triangle a, b, c (counter-clockwise) and the closed
/// segment p-q share a point, exactly: they are disjoint when an edge line of
/// the triangle has the whole segment on its closed outer side, or the
/// segment's line has the whole triangle on one closed side (the separating
/// axes of two convex sets in the plane).
fn segment_meets_interior(a: Point2, b: Point2, c: Point2, p: Point2, q: Point2) -> bool {
    let corners = [a, b, c];
    for k in 0..3 {
        if orientation(corners[k], corners[(k + 1) % 3], p) <= 0
            && orientation(corners[k], corners[(k + 1) % 3], q) <= 0
        {
            return false;
        }
    }
    if p == q {
        return true; // strictly inside every edge: inside the triangle
    }
    let oa = orientation(p, q, a);
    let ob = orientation(p, q, b);
    let oc = orientation(p, q, c);
    !((oa >= 0 && ob >= 0 && oc >= 0) || (oa <= 0 && ob <= 0 && oc <= 0))
}

/// Whether `p` lies strictly inside the circumcircle of the counter-clockwise
/// triangle a, b, c. Inexact (it only chooses which triangles an inserted
/// point re-triangulates; the kernel decides the triangles), evaluated on
/// differences from p so that large coordinates keep their precision.
fn in_circumcircle(a: Point2, b: Point2, c: Point2, p: Point2) -> bool {
    let adx = a.x - p.x;
    let ady = a.y - p.y;
    let bdx = b.x - p.x;
    let bdy = b.y - p.y;
    let cdx = c.x - p.x;
    let cdy = c.y - p.y;
    let ad = adx * adx + ady * ady;
    let bd = bdx * bdx + bdy * bdy;
    let cd = cdx * cdx + cdy * cdy;
    let determinant =
        adx * (bdy * cd - bd * cdy) - ady * (bdx * cd - bd * cdx) + ad * (bdx * cdy - bdy * cdx);
    determinant > 0.0
}

/// What a constraint of the patch is, for the level of a vertex made on it.
#[derive(Clone, Copy)]
struct ConstraintInfo {
    tag: u8,
    from: Point2,
    to: Point2,
    z_from: f64,
    z_to: f64,
    /// A ring edge: the region's triangle and edge it is, and whether the
    /// surface ends there.
    triangle: u32,
    edge: u32,
    rim: bool,
    draped: bool,
}

impl Default for ConstraintInfo {
    fn default() -> Self {
        Self {
            tag: 0,
            from: Point2::default(),
            to: Point2::default(),
            z_from: f64::NAN,
            z_to: f64::NAN,
            triangle: NO_TRIANGLE,
            edge: 0,
            rim: false,
            draped: false,
        }
    }
}

fn level_along(info: &ConstraintInfo, position: Point2) -> f64 {
    let t = std_clamp(
        Segment2::new(info.from, info.to).parameter_of(position),
        0.0,
        1.0,
    );
    lerp(info.z_from, info.z_to, t)
}

/// Which level holds at a vertex made where constraints cross: the lower the
/// rank the stronger (docs/surface_editing.md, "Levels of new vertices").
fn level_rank(info: &ConstraintInfo, cut_has_levels: bool) -> i32 {
    if (info.tag & TAG_CUT) != 0 {
        return if cut_has_levels { 0 } else { 4 };
    }
    if (info.tag & (TAG_KEPT | TAG_REGION)) != 0 {
        return 1;
    }
    if (info.tag & TAG_LINE) != 0 {
        return if info.draped { 4 } else { 2 };
    }
    3 // a shape edge
}

fn all_collinear(positions: &[Point2]) -> bool {
    (2..positions.len()).all(|i| orientation(positions[0], positions[1], positions[i]) == 0)
}

/// The constraints' bounding boxes, grown by `reach`, in a uniform grid: which
/// constraints may lie within `reach` of a point, without a pass over all of
/// them for each point. Only the constraints `eligible` marks are entered.
struct SegmentGrid {
    origin: Point2,
    cell_width: f64,
    cell_height: f64,
    columns: u32,
    rows: u32,
    cell_start: Vec<u32>,
    cell_constraints: Vec<u32>,
}

impl SegmentGrid {
    fn new(
        positions: &[Point2],
        constraints: &[CdtConstraint],
        eligible: &[u8],
        reach: f64,
    ) -> Self {
        let mut grid = Self {
            origin: Point2::default(),
            cell_width: 1.0,
            cell_height: 1.0,
            columns: 1,
            rows: 1,
            cell_start: vec![0, 0],
            cell_constraints: Vec::new(),
        };
        let mut bounds = Box2::default();
        let mut count = 0usize;
        for (c, constraint) in constraints.iter().enumerate() {
            if eligible[c] != 0 {
                bounds.expand(positions[constraint.from as usize]);
                bounds.expand(positions[constraint.to as usize]);
                count += 1;
            }
        }
        if count == 0 {
            return grid;
        }
        grid.origin = bounds.min;
        let width = bounds.width();
        let height = bounds.height();
        let target_cells = std_max(1.0, count as f64 / 2.0);
        if width > 0.0 && height > 0.0 {
            let cell = (width * height / target_cells).sqrt();
            let limit = 2.0 * target_cells; // an elongated extent must not explode one side
            grid.columns = std_clamp((width / cell).ceil(), 1.0, limit) as u32;
            grid.rows = std_clamp((height / cell).ceil(), 1.0, limit) as u32;
        }
        grid.cell_width = if width > 0.0 {
            width / f64::from(grid.columns)
        } else {
            1.0
        };
        grid.cell_height = if height > 0.0 {
            height / f64::from(grid.rows)
        } else {
            1.0
        };

        let cell_count = grid.columns as usize * grid.rows as usize;
        let range = |c: usize| {
            let a = positions[constraints[c].from as usize];
            let b = positions[constraints[c].to as usize];
            [
                cell_index(
                    std_min(a.x, b.x) - reach,
                    grid.origin.x,
                    grid.cell_width,
                    grid.columns,
                ),
                cell_index(
                    std_max(a.x, b.x) + reach,
                    grid.origin.x,
                    grid.cell_width,
                    grid.columns,
                ),
                cell_index(
                    std_min(a.y, b.y) - reach,
                    grid.origin.y,
                    grid.cell_height,
                    grid.rows,
                ),
                cell_index(
                    std_max(a.y, b.y) + reach,
                    grid.origin.y,
                    grid.cell_height,
                    grid.rows,
                ),
            ]
        };
        let mut counts: Vec<u32> = vec![0; cell_count + 1];
        for (c, &flag) in eligible.iter().enumerate() {
            if flag == 0 {
                continue;
            }
            let r = range(c);
            for row in r[2]..=r[3] {
                for column in r[0]..=r[1] {
                    counts[row as usize * grid.columns as usize + column as usize + 1] += 1;
                }
            }
        }
        for cell in 0..cell_count {
            counts[cell + 1] += counts[cell];
        }
        let mut cell_constraints: Vec<u32> = vec![0; counts[cell_count] as usize];
        let mut cursor: Vec<u32> = counts[..cell_count].to_vec();
        for (c, &flag) in eligible.iter().enumerate() {
            // ascending within a cell
            if flag == 0 {
                continue;
            }
            let r = range(c);
            for row in r[2]..=r[3] {
                for column in r[0]..=r[1] {
                    let slot = &mut cursor[row as usize * grid.columns as usize + column as usize];
                    cell_constraints[*slot as usize] = c as u32;
                    *slot += 1;
                }
            }
        }
        grid.cell_start = counts;
        grid.cell_constraints = cell_constraints;
        grid
    }

    /// Calls visit(c) for each constraint whose grown box reaches the cell of
    /// `p`: a superset of those within `reach` of it, in ascending order.
    fn near(&self, p: Point2, mut visit: impl FnMut(u32)) {
        if self.cell_constraints.is_empty() {
            return;
        }
        let cell = cell_index(p.y, self.origin.y, self.cell_height, self.rows) as usize
            * self.columns as usize
            + cell_index(p.x, self.origin.x, self.cell_width, self.columns) as usize;
        for i in self.cell_start[cell]..self.cell_start[cell + 1] {
            visit(self.cell_constraints[i as usize]);
        }
    }
}

#[derive(Clone, Copy)]
struct Hit {
    constraint: u32,
    along: f64,
    point: u32,
}

/// The points of `queries` (indices into `positions`) that lie within
/// tolerance::GEOMETRIC of a constraint they are not an end of, each at the
/// nearest such constraint. Coincidence is GEOMETRIC, the rule build_tin merges
/// by, and a point decimal arithmetic puts on a line is a few ulps off it: the
/// kernel decides only exactly, so a point 1e-15 beside a long edge does not
/// split it, and the new edge that ends there crosses it a few ulps from its
/// end instead - where CGAL's Exact_predicates_tag snaps the crossing to the
/// nearer END of the crossed edge, which can be tens of metres away. A point
/// the constraint is bent through (the kernel sees a->p and p->b) is exactly on
/// it, whatever the rounding.
///
/// Returned per constraint: the points to bend it through, in order along it.
fn points_on_constraints(
    positions: &[Point2],
    constraints: &[CdtConstraint],
    eligible: &[u8],
    queries: &[u32],
) -> Vec<Vec<u32>> {
    let mut through: Vec<Vec<u32>> = vec![Vec::new(); constraints.len()];
    if queries.is_empty() {
        return through;
    }
    let grid = SegmentGrid::new(positions, constraints, eligible, tolerance::GEOMETRIC);
    let mut hits: Vec<Hit> = Vec::new();
    for &q in queries {
        let p = positions[q as usize];
        let mut best: Option<Hit> = None;
        let mut best_distance = f64::INFINITY;
        grid.near(p, |c| {
            let k = constraints[c as usize];
            if k.from == q || k.to == q {
                return;
            }
            let a = positions[k.from as usize];
            let b = positions[k.to as usize];
            let d = b - a;
            let length_squared = d.length_squared();
            if !(length_squared > 0.0) {
                return;
            }
            let t = (p - a).dot(d) / length_squared;
            // Beyond an end the nearest point of the constraint is the end,
            // and a point within GEOMETRIC of an end was merged into it.
            if !(t > 0.0 && t < 1.0) {
                return;
            }
            let distance = p.distance_to(a + d * t);
            if distance <= tolerance::GEOMETRIC && distance < best_distance {
                best_distance = distance;
                best = Some(Hit {
                    constraint: c,
                    along: t,
                    point: q,
                });
            }
        });
        if let Some(hit) = best {
            hits.push(hit);
        }
    }
    hits.sort_by(|x, y| {
        x.constraint.cmp(&y.constraint).then_with(|| {
            if x.along != y.along {
                x.along
                    .partial_cmp(&y.along)
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                x.point.cmp(&y.point)
            }
        })
    });
    for hit in hits {
        through[hit.constraint as usize].push(hit.point);
    }
    through
}

/// The triangles around each vertex, ascending (two flat arrays).
pub(crate) struct VertexStars {
    start: Vec<u32>,
    triangles: Vec<u32>,
}

impl VertexStars {
    pub fn new(surface: &TinSurface) -> Self {
        let triangles = surface.triangles();
        let mut start: Vec<u32> = vec![0; surface.vertex_count() + 1];
        for tri in triangles {
            for &v in tri {
                start[v as usize + 1] += 1;
            }
        }
        for v in 0..surface.vertex_count() {
            start[v + 1] += start[v];
        }
        let mut stars: Vec<u32> = vec![0; start[surface.vertex_count()] as usize];
        let mut cursor: Vec<u32> = start[..surface.vertex_count()].to_vec();
        for (t, tri) in triangles.iter().enumerate() {
            for &v in tri {
                stars[cursor[v as usize] as usize] = t as u32;
                cursor[v as usize] += 1;
            }
        }
        Self {
            start,
            triangles: stars,
        }
    }

    pub fn of(&self, vertex: u32) -> &[u32] {
        let first = self.start[vertex as usize] as usize;
        let last = self.start[vertex as usize + 1] as usize;
        &self.triangles[first..last]
    }
}

/// Triangles by plan extent in a uniform grid, for "which triangles does this
/// segment reach?" without a pass over every triangle per segment.
pub(crate) struct TriangleGrid<'a> {
    surface: Option<&'a TinSurface>,
    origin: Point2,
    cell_width: f64,
    cell_height: f64,
    columns: u32,
    rows: u32,
    cell_start: Vec<u32>,
    cell_triangles: Vec<u32>,
}

impl<'a> TriangleGrid<'a> {
    pub fn new(surface: &'a TinSurface) -> Self {
        let mut grid = Self {
            surface: Some(surface),
            origin: Point2::default(),
            cell_width: 1.0,
            cell_height: 1.0,
            columns: 1,
            rows: 1,
            cell_start: vec![0, 0],
            cell_triangles: Vec::new(),
        };
        if surface.empty() {
            return grid;
        }
        let bounds = *surface.bounds();
        grid.origin = bounds.min;
        let width = bounds.width();
        let height = bounds.height();
        let target_cells = std_max(
            1.0,
            surface.triangle_count() as f64 / GRID_TRIANGLES_PER_CELL,
        );
        if width > 0.0 && height > 0.0 {
            let cell = (width * height / target_cells).sqrt();
            // An extremely elongated extent must not explode one dimension.
            let limit = 2.0 * target_cells;
            grid.columns = std_clamp((width / cell).ceil(), 1.0, limit) as u32;
            grid.rows = std_clamp((height / cell).ceil(), 1.0, limit) as u32;
        }
        grid.cell_width = if width > 0.0 {
            width / f64::from(grid.columns)
        } else {
            1.0
        };
        grid.cell_height = if height > 0.0 {
            height / f64::from(grid.rows)
        } else {
            1.0
        };

        let cell_count = grid.columns as usize * grid.rows as usize;
        let range = |t: usize| {
            let tri = surface.plan_triangle(t);
            let min_x = std_min3(tri.a.x, tri.b.x, tri.c.x);
            let max_x = std_max3(tri.a.x, tri.b.x, tri.c.x);
            let min_y = std_min3(tri.a.y, tri.b.y, tri.c.y);
            let max_y = std_max3(tri.a.y, tri.b.y, tri.c.y);
            [
                cell_index(min_x, grid.origin.x, grid.cell_width, grid.columns),
                cell_index(max_x, grid.origin.x, grid.cell_width, grid.columns),
                cell_index(min_y, grid.origin.y, grid.cell_height, grid.rows),
                cell_index(max_y, grid.origin.y, grid.cell_height, grid.rows),
            ]
        };
        let mut counts: Vec<u32> = vec![0; cell_count + 1];
        for t in 0..surface.triangle_count() {
            let r = range(t);
            for row in r[2]..=r[3] {
                for column in r[0]..=r[1] {
                    counts[row as usize * grid.columns as usize + column as usize + 1] += 1;
                }
            }
        }
        for cell in 0..cell_count {
            counts[cell + 1] += counts[cell];
        }
        let mut cell_triangles: Vec<u32> = vec![0; counts[cell_count] as usize];
        let mut cursor: Vec<u32> = counts[..cell_count].to_vec();
        for t in 0..surface.triangle_count() {
            let r = range(t);
            for row in r[2]..=r[3] {
                for column in r[0]..=r[1] {
                    let slot = &mut cursor[row as usize * grid.columns as usize + column as usize];
                    cell_triangles[*slot as usize] = t as u32;
                    *slot += 1;
                }
            }
        }
        grid.cell_start = counts;
        grid.cell_triangles = cell_triangles;
        grid
    }

    /// Every triangle the segment from `a` to `b` reaches, or comes within a
    /// rounding margin of, is marked in `marks` (one per triangle). The test
    /// is conservative: it may mark a triangle the segment only passes near,
    /// never miss one it crosses - a region a little too large costs a few
    /// triangles re-triangulated, one too small is caught and grown anyway.
    /// `newly_marked`, when given, receives the triangles THIS call marked (not
    /// those `marks` already held), so a caller that needs the list does not
    /// scan the surface's every triangle for it.
    pub fn mark_along(
        &self,
        a: Point2,
        b: Point2,
        marks: &mut [u8],
        mut newly_marked: Option<&mut Vec<u32>>,
    ) {
        let Some(surface) = self.surface else {
            return;
        };
        if surface.empty() {
            return;
        }
        let bounds = surface.bounds();
        let scale = [
            a.x.abs(),
            a.y.abs(),
            b.x.abs(),
            b.y.abs(),
            bounds.min.x.abs(),
            bounds.min.y.abs(),
            bounds.max.x.abs(),
            bounds.max.y.abs(),
        ]
        .into_iter()
        .fold(0.0, std_max);
        let margin = rounding_margin(scale);
        let min_x = std_min(a.x, b.x) - margin;
        let max_x = std_max(a.x, b.x) + margin;
        let min_y = std_min(a.y, b.y) - margin;
        let max_y = std_max(a.y, b.y) + margin;
        if max_x < bounds.min.x
            || min_x > bounds.max.x
            || max_y < bounds.min.y
            || min_y > bounds.max.y
        {
            return;
        }
        let column0 = cell_index(min_x, self.origin.x, self.cell_width, self.columns);
        let column1 = cell_index(max_x, self.origin.x, self.cell_width, self.columns);
        let row0 = cell_index(min_y, self.origin.y, self.cell_height, self.rows);
        let row1 = cell_index(max_y, self.origin.y, self.cell_height, self.rows);
        for row in row0..=row1 {
            for column in column0..=column1 {
                let cell = row as usize * self.columns as usize + column as usize;
                for i in self.cell_start[cell]..self.cell_start[cell + 1] {
                    let t = self.cell_triangles[i as usize];
                    if marks[t as usize] != 0 {
                        continue;
                    }
                    let tri = surface.plan_triangle(t as usize);
                    if segment_near_triangle(a, b, tri.a, tri.b, tri.c, margin) {
                        marks[t as usize] = 1;
                        if let Some(list) = newly_marked.as_deref_mut() {
                            list.push(t);
                        }
                    }
                }
            }
        }
    }
}

/// Where a plan position lies on a surface, decided exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlaceKind {
    #[default]
    Outside,
    Vertex,
    Edge,
    Inside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub kind: PlaceKind,
    /// Edge, Inside
    pub triangle: u32,
    /// Edge
    pub edge: u32,
    /// Vertex
    pub vertex: u32,
}

impl Default for Place {
    fn default() -> Self {
        Self {
            kind: PlaceKind::Outside,
            triangle: NO_TRIANGLE,
            edge: 0,
            vertex: NO_VERTEX,
        }
    }
}

/// Vertex when a vertex lies within tolerance::GEOMETRIC (the nearest; the
/// rule build_tin merges by); else Inside or Edge of a triangle by the exact
/// orientation of the position against its edges; else Edge of a rim edge
/// the position lies within GEOMETRIC beyond (TinSurface::locate counts that
/// as on the surface, and there is one definition of "on the surface"); else
/// Outside.
pub(crate) fn place_of(surface: &TinSurface, stars: &VertexStars, position: Point2) -> Place {
    let mut place = Place::default();
    let Some(location) = surface.locate(position) else {
        return place;
    };
    let mut candidates: Vec<u32> = Vec::new();
    for &v in &surface.triangles()[location.triangle as usize] {
        candidates.extend_from_slice(stars.of(v));
    }
    candidates.sort_unstable();
    candidates.dedup();

    let vertices = surface.vertices();
    let mut nearest = f64::INFINITY;
    let mut nearest_vertex = NO_VERTEX;
    for &t in &candidates {
        for &v in &surface.triangles()[t as usize] {
            let distance = plan_of(vertices[v as usize]).distance_to(position);
            if distance <= tolerance::GEOMETRIC
                && (distance < nearest || (distance == nearest && v < nearest_vertex))
            {
                nearest = distance;
                nearest_vertex = v;
            }
        }
    }
    if nearest_vertex != NO_VERTEX {
        place.kind = PlaceKind::Vertex;
        place.vertex = nearest_vertex;
        return place;
    }

    let corners_of = |t: u32| {
        let tri = surface.triangles()[t as usize];
        [
            plan_of(vertices[tri[0] as usize]),
            plan_of(vertices[tri[1] as usize]),
            plan_of(vertices[tri[2] as usize]),
        ]
    };
    for &t in &candidates {
        let tri = surface.triangles()[t as usize];
        let corners = corners_of(t);
        if orientation(corners[0], corners[1], corners[2]) <= 0 {
            continue; // a triangle flat in plan holds no position of its own
        }
        let mut sides = [0i32; 3];
        let mut outside = false;
        for k in 0..3 {
            sides[k] = orientation(corners[k], corners[(k + 1) % 3], position);
            outside = outside || sides[k] < 0;
        }
        if outside {
            continue;
        }
        let zeros = sides.iter().filter(|&&side| side == 0).count();
        if zeros == 0 {
            place.kind = PlaceKind::Inside;
            place.triangle = t;
            return place;
        }
        if zeros == 1 {
            place.kind = PlaceKind::Edge;
            place.triangle = t;
            place.edge = if sides[0] == 0 {
                0
            } else if sides[1] == 0 {
                1
            } else {
                2
            };
            return place;
        }
        // On two edge lines: exactly at a corner (which the distance test
        // above would have found; kept so the answer is never wrong).
        place.kind = PlaceKind::Vertex;
        place.vertex = tri[if sides[0] == 0 && sides[1] == 0 {
            1
        } else if sides[1] == 0 {
            2
        } else {
            0
        }];
        return place;
    }

    // Nothing holds it exactly, yet locate() found it: it lies within
    // GEOMETRIC of an edge, and beyond it by the exact orientation - so the
    // edge has no triangle on the other side to take it, a rim edge. A rim
    // point computed by an intersection or a snap lands there about as often
    // as on the right side. It is on the surface (locate and elevation_at say
    // so, and the rule is GEOMETRIC), and an edit that took it for off the
    // surface would refuse a breakline ending on a boundary just cut, or join
    // it to the rim with a sliver of no width that rises to its level.
    let mut nearest_rim = f64::INFINITY;
    for &t in &candidates {
        let corners = corners_of(t);
        if orientation(corners[0], corners[1], corners[2]) <= 0 {
            continue;
        }
        for k in 0..3u32 {
            if surface.neighbors()[t as usize][k as usize] != NO_TRIANGLE {
                continue;
            }
            let distance = Segment2::new(corners[k as usize], corners[(k as usize + 1) % 3])
                .distance_to(position);
            if distance <= tolerance::GEOMETRIC && distance < nearest_rim {
                nearest_rim = distance;
                place.kind = PlaceKind::Edge;
                place.triangle = t;
                place.edge = k;
            }
        }
    }
    place
}

/// The triangles an incremental Delaunay insertion of `position` (placed at
/// `place`, Inside or Edge) would rebuild, marked in `region`: the triangle it
/// lies in (both, on an edge), and every triangle reached from there across
/// an unconstrained edge whose circumcircle holds the position - the
/// Bowyer-Watson cavity, which stops at constrained edges as the kernel's own
/// insertion does.
pub(crate) fn mark_insertion_cavity(
    surface: &TinSurface,
    place: &Place,
    position: Point2,
    region: &mut [u8],
) {
    if place.kind != PlaceKind::Inside && place.kind != PlaceKind::Edge {
        return;
    }
    let mut cavity: Vec<u32> = vec![place.triangle];
    if place.kind == PlaceKind::Edge {
        let across = surface.neighbors()[place.triangle as usize][place.edge as usize];
        if across != NO_TRIANGLE {
            cavity.push(across);
        }
    }
    let mut i = 0;
    while i < cavity.len() {
        let t = cavity[i];
        for k in 0..3 {
            let next = surface.neighbors()[t as usize][k];
            if next == NO_TRIANGLE
                || surface.is_edge_constrained(t as usize, k)
                || cavity.contains(&next)
            {
                continue;
            }
            let corners = surface.plan_triangle(next as usize);
            if in_circumcircle(corners.a, corners.b, corners.c, position) {
                cavity.push(next);
            }
        }
        i += 1;
    }
    for t in cavity {
        region[t as usize] = 1;
    }
}

/// The triangles a point of an edit reaches, marked in `marks` (one per
/// triangle): the triangle it lies in, both when it lies on an edge, the whole
/// star of a vertex it lies at; none off the surface. The rule rebuild_patch
/// grows its region by, one function so a caller asking "what would this edit
/// rebuild?" (cut_holes, deciding which footprints can share a patch) cannot
/// come to a different answer. `newly_marked`: as TriangleGrid::mark_along.
pub(crate) fn mark_reached_by_point(
    surface: &TinSurface,
    stars: &VertexStars,
    position: Point2,
    marks: &mut [u8],
    mut newly_marked: Option<&mut Vec<u32>>,
) {
    let mut mark = |t: u32| {
        if marks[t as usize] == 0 {
            marks[t as usize] = 1;
            if let Some(list) = newly_marked.as_deref_mut() {
                list.push(t);
            }
        }
    };
    let place = place_of(surface, stars, position);
    match place.kind {
        PlaceKind::Inside => mark(place.triangle),
        PlaceKind::Edge => {
            mark(place.triangle);
            let across = surface.neighbors()[place.triangle as usize][place.edge as usize];
            if across != NO_TRIANGLE {
                mark(across);
            }
        }
        PlaceKind::Vertex => {
            for &t in stars.of(place.vertex) {
                mark(t);
            }
        }
        PlaceKind::Outside => {}
    }
}

// Tags of the constraints of a patch, reported back per edge by the kernel.
/// an edge of the surface that is constrained
pub(crate) const TAG_KEPT: u8 = 1;
/// the ring around the region
pub(crate) const TAG_REGION: u8 = 2;
/// a new breakline segment
pub(crate) const TAG_LINE: u8 = 4;
/// an unconstrained edge held for its shape
pub(crate) const TAG_SHAPE: u8 = 8;
/// an edge of a cut (boundary, hole, seam)
pub(crate) const TAG_CUT: u8 = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PatchPoint {
    pub position: Point2,
    /// None: the surface's level there (draped).
    pub z: Option<f64>,
    /// The point must end up a vertex of the result: a breakline's vertex, a
    /// point added. A cut's corner off the surface need not.
    pub required: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PatchSegment {
    /// indices into PatchRequest::points
    pub from: u32,
    pub to: u32,
    /// A line segment with both ends draped follows the surface: its levels
    /// are the old edges' where it crosses them. Otherwise it is straight in
    /// 3D between its ends' levels.
    pub draped: bool,
}

#[derive(Default)]
pub(crate) struct PatchRequest<'a> {
    /// One flag per triangle: rebuilt. Grown as needed; the triangles around
    /// a removed vertex, and those the points and segments reach, are added.
    pub region: Vec<u8>,
    /// One flag per vertex, or empty: left out of the rebuilt region.
    pub removed: Vec<u8>,
    /// The region's unconstrained edges are held too, so the rebuilt region
    /// is the old triangles cut, not re-triangulated (a cut keeps the shape).
    pub keep_shape: bool,
    pub points: Vec<PatchPoint>,
    pub lines: Vec<PatchSegment>,
    /// Directed: what is inside the cut lies on their left.
    pub cuts: Vec<PatchSegment>,
    /// With cuts: whether the inside or the outside of the cut is kept.
    pub keep_inside_cut: bool,
    /// The cut's own points' levels hold along it, over every other edge (a
    /// paste's seam takes the top's levels); otherwise a cut is draped.
    pub cut_has_levels: bool,
    /// The cuts make ONE simple ring (a boundary or a hole): refused when it
    /// visits a vertex twice, which a corner shared by two lobes does without
    /// any edge crossing another. A paste's seam is the top's rim, which may
    /// touch itself, so it leaves this off.
    pub simple_cut: bool,
    /// An added point at an existing vertex moves it there too (a paste's
    /// seam lies on the top's rim, not on the base's vertex beside it).
    pub points_move_vertices: bool,
    /// With cuts: whether a position is inside the cut, for the triangles
    /// outside the region and for any piece of the region no cut edge
    /// reaches. Asked of a centroid, well away from any cut edge.
    pub inside_cut: Option<Box<dyn Fn(Point2) -> bool + 'a>>,
}

impl PatchRequest<'_> {
    /// A request with the C++'s defaults (the inside of a cut kept).
    pub fn new() -> Self {
        Self {
            keep_inside_cut: true,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PatchOutput {
    /// The surface's vertices (levels and positions updated), then the new
    /// ones. Vertices left without a triangle are still here: finish_surface
    /// drops them.
    pub vertices: Vec<Point3>,
    pub triangles: Vec<TinTriangle>,
    pub masks: Vec<u8>,
    /// Per request point, its vertex.
    pub point_vertex: Vec<u32>,
    /// Per cut segment, the vertices along it from `from` to `to` (empty for
    /// a segment of no length).
    pub cut_vertices: Vec<Vec<u32>>,
    pub relevelled: usize,
    pub crossings: usize,
    pub new_vertices: usize,
}

struct Attempt {
    output: Option<PatchOutput>,
    grow: Vec<u32>,
}

fn add_constraint(
    constraints: &mut Vec<CdtConstraint>,
    infos: &mut Vec<ConstraintInfo>,
    positions: &[Point2],
    sub_level: &[f64],
    (from, to, tag): (u32, u32, u8),
    mut info: ConstraintInfo,
) {
    constraints.push(CdtConstraint { from, to, tag });
    info.tag = tag;
    info.from = positions[from as usize];
    info.to = positions[to as usize];
    if info.z_from.is_nan() {
        info.z_from = sub_level[from as usize];
        info.z_to = sub_level[to as usize];
    }
    infos.push(info);
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

fn try_patch(surface: &TinSurface, request: &PatchRequest, region: &[u8]) -> Result<Attempt> {
    let source_vertices = surface.vertices();
    let triangles = surface.triangles();
    let neighbors = surface.neighbors();
    let masks = surface.constrained_edges();
    let vertex_count = source_vertices.len();
    let removed = |v: u32| !request.removed.is_empty() && request.removed[v as usize] != 0;
    let in_region = |t: u32| t != NO_TRIANGLE && region[t as usize] != 0;

    let region_triangles: Vec<u32> = (0..triangles.len() as u32)
        .filter(|&t| region[t as usize] != 0)
        .collect();

    // ---- the points of the sub-problem: the region's vertices, then the request's
    let mut vertices: Vec<Point3> = source_vertices.clone();
    let mut merger = PointMerger::new(
        3 * region_triangles.len() + request.points.len() + 1,
        tolerance::GEOMETRIC,
    );
    let mut sub_of_vertex: Vec<u32> = vec![NO_INDEX; vertex_count];
    let mut vertex_of_sub: Vec<u32> = Vec::new();
    let mut point_of_sub: Vec<u32> = Vec::new();
    for &t in &region_triangles {
        for &v in &triangles[t as usize] {
            if removed(v) || sub_of_vertex[v as usize] != NO_INDEX {
                continue;
            }
            let entry = merger.add(plan_of(source_vertices[v as usize]));
            if !entry.inserted {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "the surface has two vertices closer than the coincidence tolerance; an \
                     edit there cannot tell them apart",
                )
                .with_context(format!(
                    "vertices {} and {v} at {}",
                    vertex_of_sub[entry.index as usize],
                    describe_position(plan_of(source_vertices[v as usize]))
                )));
            }
            sub_of_vertex[v as usize] = entry.index;
            vertex_of_sub.push(v);
            point_of_sub.push(NO_INDEX);
        }
    }
    let surface_sub_count = vertex_of_sub.len();

    let mut relevelled = 0usize;
    let mut point_sub: Vec<u32> = vec![NO_INDEX; request.points.len()];
    // The level a request point gave a surface vertex, to refuse two that disagree.
    let mut given_level: Vec<Option<f64>> = vec![None; surface_sub_count];
    let mut relevelled_sub: Vec<u8> = vec![0; surface_sub_count];
    for (i, point) in request.points.iter().enumerate() {
        let entry = merger.add(point.position);
        point_sub[i] = entry.index;
        if entry.inserted {
            vertex_of_sub.push(NO_VERTEX);
            point_of_sub.push(i as u32);
            continue;
        }
        let conflict = |other: f64| {
            point
                .z
                .is_some_and(|z| (z - other).abs() > tolerance::COORDINATE)
        };
        let index = entry.index as usize;
        if index < surface_sub_count {
            let v = vertex_of_sub[index] as usize;
            if let Some(z) = point.z {
                if let Some(given) = given_level[index]
                    && conflict(given)
                {
                    return Err(two_levels(point.position, given, z));
                }
                given_level[index] = Some(z);
                if vertices[v].z != z {
                    vertices[v].z = z;
                    if relevelled_sub[index] == 0 {
                        relevelled_sub[index] = 1;
                        relevelled += 1;
                    }
                }
            }
            if request.points_move_vertices {
                vertices[v].x = point.position.x;
                vertices[v].y = point.position.y;
            }
            continue;
        }
        let earlier = request.points[point_of_sub[index] as usize];
        if let Some(earlier_z) = earlier.z
            && conflict(earlier_z)
        {
            return Err(two_levels(
                point.position,
                earlier_z,
                point.z.unwrap_or(f64::NAN),
            ));
        }
        // The level any visit gives the position is the position's, as it is
        // at a surface vertex above: a first visit with none must not drop
        // the level a later one carries.
        if earlier.z.is_none() && point.z.is_some() {
            point_of_sub[index] = i as u32;
        }
    }
    let positions: &Vec<Point2> = merger.positions();

    // Every sub-problem point's level: a surface vertex's own (as relevelled),
    // a point's given one, else the surface's under it (none off the surface).
    let mut sub_level: Vec<f64> = vec![f64::NAN; positions.len()];
    for s in 0..positions.len() {
        if vertex_of_sub[s] != NO_VERTEX {
            sub_level[s] = vertices[vertex_of_sub[s] as usize].z;
        } else {
            let point = request.points[point_of_sub[s] as usize];
            if let Some(z) = point.z {
                sub_level[s] = z;
            } else if let Some(z) = surface.elevation_at(point.position) {
                sub_level[s] = z;
            }
        }
    }

    // ---- constraints ----------------------------------------------------------
    let mut constraints: Vec<CdtConstraint> = Vec::new();
    let mut infos: Vec<ConstraintInfo> = Vec::new();
    let mut open_ring: Vec<(Point2, Point2)> = Vec::new(); // ring edges at a removed vertex
    for &t in &region_triangles {
        for k in 0..3u32 {
            let a = triangles[t as usize][k as usize];
            let b = triangles[t as usize][(k as usize + 1) % 3];
            let across = neighbors[t as usize][k as usize];
            let constrained = ((masks[t as usize] >> k) & 1) != 0;
            if !in_region(across) {
                if removed(a) || removed(b) {
                    if across != NO_TRIANGLE {
                        return Err(Error::new(
                            ErrorCode::Internal,
                            "a removed vertex's triangles are not all rebuilt",
                        )
                        .with_context(format!("triangle={t}")));
                    }
                    open_ring.push((
                        plan_of(source_vertices[a as usize]),
                        plan_of(source_vertices[b as usize]),
                    ));
                    continue;
                }
                let info = ConstraintInfo {
                    triangle: t,
                    edge: k,
                    rim: across == NO_TRIANGLE,
                    ..ConstraintInfo::default()
                };
                add_constraint(
                    &mut constraints,
                    &mut infos,
                    positions,
                    &sub_level,
                    (
                        sub_of_vertex[a as usize],
                        sub_of_vertex[b as usize],
                        TAG_REGION | if constrained { TAG_KEPT } else { 0 },
                    ),
                    info,
                );
            } else if t < across && !removed(a) && !removed(b) {
                if constrained {
                    add_constraint(
                        &mut constraints,
                        &mut infos,
                        positions,
                        &sub_level,
                        (
                            sub_of_vertex[a as usize],
                            sub_of_vertex[b as usize],
                            TAG_KEPT,
                        ),
                        ConstraintInfo::default(),
                    );
                } else if request.keep_shape {
                    add_constraint(
                        &mut constraints,
                        &mut infos,
                        positions,
                        &sub_level,
                        (
                            sub_of_vertex[a as usize],
                            sub_of_vertex[b as usize],
                            TAG_SHAPE,
                        ),
                        ConstraintInfo::default(),
                    );
                }
            }
        }
    }
    let line_begin = constraints.len();
    for segment in &request.lines {
        let from = point_sub[segment.from as usize];
        let to = point_sub[segment.to as usize];
        if from != to {
            let info = ConstraintInfo {
                draped: segment.draped,
                ..ConstraintInfo::default()
            };
            add_constraint(
                &mut constraints,
                &mut infos,
                positions,
                &sub_level,
                (from, to, TAG_LINE),
                info,
            );
        }
    }
    let line_end = constraints.len();
    let mut cut_constraint: Vec<Option<usize>> = vec![None; request.cuts.len()];
    for (s, cut) in request.cuts.iter().enumerate() {
        let from = point_sub[cut.from as usize];
        let to = point_sub[cut.to as usize];
        if from != to {
            cut_constraint[s] = Some(constraints.len());
            add_constraint(
                &mut constraints,
                &mut infos,
                positions,
                &sub_level,
                (from, to, TAG_CUT),
                ConstraintInfo::default(),
            );
        }
    }

    // ---- the triangulation ------------------------------------------------------
    // A new point within GEOMETRIC of an edge it is not an end of is on it:
    // the edge is bent through the point, so the kernel is given the pieces
    // (a to p, p to b) and the junction is exact, whatever the last bit of the
    // point's coordinates rounded to (points_on_constraints). The edges that
    // hold a shape or a level stay what they were to within GEOMETRIC. An
    // unconstrained ring edge is left alone: a point beside it is in a region
    // triangle or the neighbour (placed, so the neighbour is in the region),
    // and a sliver there changes nothing; splitting it would only grow the
    // region by a neighbour the edit does not need.
    let can_triangulate =
        !region_triangles.is_empty() && positions.len() >= 3 && !all_collinear(positions);
    let mut through: Vec<Vec<u32>> = vec![Vec::new(); constraints.len()];
    if can_triangulate {
        let eligible: Vec<u8> = infos
            .iter()
            .map(|info| u8::from(info.tag != TAG_REGION || info.rim))
            .collect();
        let queries: Vec<u32> = (surface_sub_count as u32..positions.len() as u32).collect();
        through = points_on_constraints(positions, &constraints, &eligible, &queries);
    }
    // What the kernel is given: each constraint as the run of pieces between
    // the points it was bent through. kernel_start[c] is constraint c's first.
    let mut kernel_constraints: Vec<CdtConstraint> = Vec::new();
    let mut kernel_start: Vec<usize> = vec![0; constraints.len() + 1];
    for (c, constraint) in constraints.iter().enumerate() {
        kernel_start[c] = kernel_constraints.len();
        let mut previous = constraint.from;
        for &via in &through[c] {
            kernel_constraints.push(CdtConstraint {
                from: previous,
                to: via,
                tag: constraint.tag,
            });
            previous = via;
        }
        kernel_constraints.push(CdtConstraint {
            from: previous,
            to: constraint.to,
            tag: constraint.tag,
        });
    }
    kernel_start[constraints.len()] = kernel_constraints.len();

    let mut cdt = CdtOutput::default();
    let mut triangulated = false;
    if can_triangulate {
        cdt = cdt_backend::triangulate(&CdtInput {
            points: positions,
            constraints: &kernel_constraints,
            export_constraint_vertices: true,
        })?;
        triangulated = true;
    }
    let input_count = positions.len();
    let generated_count = cdt.generated_points.len();
    let position_of = |sub: u32| {
        if (sub as usize) < input_count {
            positions[sub as usize]
        } else {
            cdt.generated_points[sub as usize - input_count]
        }
    };
    // The vertices along constraint c, from its `from` to its `to`; without a
    // triangulation, its two ends.
    let along = |c: usize| -> Vec<u32> {
        if !triangulated {
            return vec![constraints[c].from, constraints[c].to];
        }
        let mut list: Vec<u32> = Vec::new();
        for k in kernel_start[c]..kernel_start[c + 1] {
            let first = cdt.constraint_vertex_offsets[k] as usize;
            let last = cdt.constraint_vertex_offsets[k + 1] as usize;
            // A piece starts where the one before it ended.
            let skip = usize::from(k > kernel_start[c] && first != last);
            list.extend_from_slice(&cdt.constraint_vertices[first + skip..last]);
        }
        list
    };

    // Which constraints each made vertex lies on, and which input points lie
    // on a constrained edge of the surface.
    let mut on_constraints: Vec<Vec<u32>> = vec![Vec::new(); generated_count];
    let mut cut_appearances: Vec<u32> = vec![0; input_count + generated_count];
    let mut on_kept_edge: Vec<u8> = vec![0; input_count];
    if triangulated {
        for (c, info) in infos.iter().enumerate() {
            for x in along(c) {
                if x as usize >= input_count {
                    on_constraints[x as usize - input_count].push(c as u32);
                } else if (info.tag & TAG_KEPT) != 0 {
                    on_kept_edge[x as usize] = 1;
                }
                if (info.tag & TAG_CUT) != 0 {
                    cut_appearances[x as usize] += 1;
                }
            }
        }
    }

    // ---- a split ring edge grows the region; a line across the rim is refused
    let mut grow: Vec<u32> = Vec::new();
    if triangulated {
        for (c, info) in infos.iter().enumerate() {
            if (info.tag & TAG_REGION) == 0 {
                continue;
            }
            let list = along(c);
            if list.len() <= 2 {
                continue;
            }
            if !info.rim {
                grow.push(neighbors[info.triangle as usize][info.edge as usize]);
                continue;
            }
            for &x in &list[1..list.len() - 1] {
                if (x as usize) < input_count {
                    continue;
                }
                for &other in &on_constraints[x as usize - input_count] {
                    if (infos[other as usize].tag & TAG_LINE) != 0 {
                        return Err(Error::new(
                            ErrorCode::InvalidGeometry,
                            "the breakline leaves the surface: it crosses the rim",
                        )
                        .with_context(format!("at {}", describe_position(position_of(x)))));
                    }
                }
            }
        }
    }
    if !grow.is_empty() {
        return Ok(Attempt { output: None, grow });
    }
    if triangulated {
        for &c in cut_constraint.iter().flatten() {
            let list = along(c);
            for &x in list.iter().take(list.len().saturating_sub(1)).skip(1) {
                if cut_appearances[x as usize] > 1 {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "the polygon touches or crosses itself",
                    )
                    .with_context(format!("at {}", describe_position(position_of(x)))));
                }
            }
        }
    }
    // A ring that visits a vertex twice - a corner shared by two lobes, a
    // corner on another edge - is not a polygon with an inside. The check
    // above sees only vertices interior to an edge, and a corner visited
    // twice is the end of four edges; build_tin's require_simple_ring walks
    // the ring the same way.
    if request.simple_cut && triangulated {
        let mut visited: Vec<u8> = vec![0; input_count + generated_count];
        for &c in cut_constraint.iter().flatten() {
            let list = along(c);
            for &x in list.iter().take(list.len().saturating_sub(1)) {
                if visited[x as usize] != 0 {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "the polygon touches or crosses itself",
                    )
                    .with_context(format!("at {}", describe_position(position_of(x)))));
                }
                visited[x as usize] = 1;
            }
        }
    }
    // The kernel's promise is that each constraint runs through the vertices it
    // reports. Where a crossing falls within rounding of the end of the edge
    // it crosses, CGAL's Exact_predicates_tag moves it to that END, which may
    // be a long way off: the constraint is then routed through a vertex that
    // is not on it. Points within GEOMETRIC of an edge were bent onto it
    // above; what is left is a vertex of the surface that close to a new edge,
    // and an edit that would build on such a result is refused rather than
    // trusted.
    if triangulated {
        for (c, info) in infos.iter().enumerate() {
            let list = along(c);
            if list.len() <= 2 {
                continue;
            }
            let segment = Segment2::new(info.from, info.to);
            for &x in &list[1..list.len() - 1] {
                let at = position_of(x);
                let scale = [
                    at.x.abs(),
                    at.y.abs(),
                    segment.start.x.abs(),
                    segment.start.y.abs(),
                    segment.end.x.abs(),
                    segment.end.y.abs(),
                ]
                .into_iter()
                .fold(0.0, std_max);
                if segment.distance_to(at) > rounding_margin(scale) {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "an edge of the edit passes so close to the end of another that the \
                         kernel could not place the crossing on it: move the end onto the edge \
                         or clear of it",
                    )
                    .with_context(format!("near {}", describe_position(at))));
                }
            }
        }
    }

    // ---- which new triangles are inside the region ---------------------------
    let new_count = cdt.triangles.len();
    let mut state: Vec<u8> = vec![0; new_count]; // 0 not known, 1 inside, 2 outside
    if triangulated {
        let mut ring = EdgeSet::default();
        for (c, info) in infos.iter().enumerate() {
            if (info.tag & TAG_REGION) != 0 {
                for pair in along(c).windows(2) {
                    ring.add(pair[0], pair[1]);
                }
            }
        }
        ring.seal();
        let mut queue: Vec<u32> = Vec::new();
        for (t, slot) in state.iter_mut().enumerate() {
            let tri = cdt.triangles[t];
            let mut inside = false;
            let mut outside = false;
            for k in 0..3 {
                if (cdt.edge_tags[t][k] & TAG_REGION) != 0 {
                    if ring.has(tri[k], tri[(k + 1) % 3]) {
                        inside = true;
                    } else {
                        outside = true;
                    }
                }
            }
            for &(p, q) in &open_ring {
                if !outside
                    && segment_meets_interior(
                        position_of(tri[0]),
                        position_of(tri[1]),
                        position_of(tri[2]),
                        p,
                        q,
                    )
                {
                    outside = true;
                }
            }
            if outside {
                *slot = 2;
            } else if inside {
                *slot = 1;
                queue.push(t as u32);
            }
        }
        while let Some(t) = queue.pop() {
            for k in 0..3 {
                let next = cdt.neighbors[t as usize][k];
                if (cdt.edge_tags[t as usize][k] & TAG_REGION) == 0
                    && next != NO_INDEX
                    && state[next as usize] == 0
                {
                    state[next as usize] = 1;
                    queue.push(next);
                }
            }
        }
    }
    let plan_of_new = |t: usize| {
        let tri = cdt.triangles[t];
        Triangle2::new(
            position_of(tri[0]),
            position_of(tri[1]),
            position_of(tri[2]),
        )
    };
    let inside_cut = |p: Point2| request.inside_cut.as_ref().is_some_and(|ask| ask(p));

    // ---- which of them the cut keeps --------------------------------------------
    let mut keep: Vec<u8> = state.iter().map(|&s| u8::from(s == 1)).collect();
    if !request.cuts.is_empty() && triangulated {
        let mut cut_edges = EdgeSet::default();
        for &c in cut_constraint.iter().flatten() {
            for pair in along(c).windows(2) {
                cut_edges.add(pair[0], pair[1]);
            }
        }
        cut_edges.seal();
        let mut side: Vec<u8> = vec![0; new_count]; // 0 not known, 1 inside the cut, 2 outside
        let mut queue: Vec<u32> = Vec::new();
        for t in 0..new_count {
            if state[t] != 1 {
                continue;
            }
            let tri = cdt.triangles[t];
            let mut inside = false;
            let mut outside = false;
            for k in 0..3 {
                if (cdt.edge_tags[t][k] & TAG_CUT) != 0 {
                    if cut_edges.has(tri[k], tri[(k + 1) % 3]) {
                        inside = true;
                    } else {
                        outside = true;
                    }
                }
            }
            if inside != outside {
                side[t] = if inside { 1 } else { 2 };
                queue.push(t as u32);
            }
        }
        while let Some(t) = queue.pop() {
            for k in 0..3 {
                let next = cdt.neighbors[t as usize][k];
                if (cdt.edge_tags[t as usize][k] & (TAG_CUT | TAG_REGION)) == 0
                    && next != NO_INDEX
                    && state[next as usize] == 1
                    && side[next as usize] == 0
                {
                    side[next as usize] = side[t as usize];
                    queue.push(next);
                }
            }
        }
        // A piece no cut edge reaches is inside or outside as a whole: asked at
        // the centroid of its largest triangle, the point of it farthest from
        // any edge the piece allows.
        for seed in 0..new_count {
            if state[seed] != 1 || side[seed] != 0 {
                continue;
            }
            let mut piece: Vec<u32> = vec![seed as u32];
            let mut stack: Vec<u32> = vec![seed as u32];
            side[seed] = 3; // being gathered
            while let Some(t) = stack.pop() {
                for k in 0..3 {
                    let next = cdt.neighbors[t as usize][k];
                    if (cdt.edge_tags[t as usize][k] & (TAG_CUT | TAG_REGION)) == 0
                        && next != NO_INDEX
                        && state[next as usize] == 1
                        && side[next as usize] == 0
                    {
                        side[next as usize] = 3;
                        piece.push(next);
                        stack.push(next);
                    }
                }
            }
            let mut largest = piece[0];
            let mut largest_area = -1.0;
            for &t in &piece {
                let area = plan_of_new(t as usize).signed_area();
                if area > largest_area {
                    largest_area = area;
                    largest = t;
                }
            }
            let answer = if inside_cut(plan_of_new(largest as usize).centroid()) {
                1
            } else {
                2
            };
            for &t in &piece {
                side[t as usize] = answer;
            }
        }
        for t in 0..new_count {
            keep[t] = u8::from(state[t] == 1 && ((side[t] == 1) == request.keep_inside_cut));
        }
    }

    // ---- levels of the vertices the kernel made ------------------------------
    let mut generated_level: Vec<f64> = vec![f64::NAN; generated_count];
    for g in 0..generated_count {
        let position = cdt.generated_points[g];
        let mut best_rank = i32::MAX;
        let mut best: Option<u32> = None;
        for &c in &on_constraints[g] {
            let rank = level_rank(&infos[c as usize], request.cut_has_levels);
            if rank < best_rank {
                best_rank = rank;
                best = Some(c);
            }
        }
        let Some(best) = best.filter(|_| best_rank != 4) else {
            if let Some(z) = surface.elevation_at(position) {
                generated_level[g] = z;
            }
            continue;
        };
        let level = level_along(&infos[best as usize], position);
        if best_rank == 2 {
            // Two new 3D lines crossing - one line crossing itself, or two of
            // the lines of one edit - must agree there.
            for &c in &on_constraints[g] {
                let info = &infos[c as usize];
                if level_rank(info, request.cut_has_levels) == 2
                    && (level_along(info, position) - level).abs() > tolerance::COORDINATE
                {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "breaklines cross at two different levels",
                    )
                    .with_context(format!(
                        "at {}: {} and {}",
                        describe_position(position),
                        format_exact_real(level),
                        format_exact_real(level_along(info, position))
                    )));
                }
            }
        }
        generated_level[g] = level;
    }

    // A vertex that was already there and lies exactly on a new 3D line takes
    // the line's level: "straight in 3D" holds end to end, not only between
    // the vertices the kernel happened to make, and the same line moved off
    // the vertex by a hair must not give a different surface. Not a vertex on
    // a breakline the surface already had (that keeps its level and the line
    // bends there, as where they cross) and not one a point of the edit gave a
    // level (a line through such a point at another level is two levels at one
    // place, as two lines crossing are). Where several new lines pass through
    // one vertex they must agree.
    let mut on_lines = 0usize;
    let mut line_level: Vec<Option<f64>> = vec![None; input_count];
    if triangulated {
        for (c, info) in infos.iter().enumerate().take(line_end).skip(line_begin) {
            if info.draped {
                continue;
            }
            let list = along(c);
            for &s in list.iter().take(list.len().saturating_sub(1)).skip(1) {
                if s as usize >= input_count || on_kept_edge[s as usize] != 0 {
                    continue;
                }
                let level = level_along(info, positions[s as usize]);
                let own: Option<f64> = if vertex_of_sub[s as usize] != NO_VERTEX {
                    if (s as usize) < surface_sub_count {
                        given_level[s as usize]
                    } else {
                        None
                    }
                } else {
                    request.points[point_of_sub[s as usize] as usize].z
                };
                let other = if own.is_some() {
                    own
                } else {
                    line_level[s as usize]
                };
                if let Some(other) = other
                    && (other - level).abs() > tolerance::COORDINATE
                {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "breaklines cross at two different levels",
                    )
                    .with_context(format!(
                        "at {}: {} and {}",
                        describe_position(positions[s as usize]),
                        format_exact_real(other),
                        format_exact_real(level)
                    )));
                }
                if own.is_none() {
                    line_level[s as usize] = Some(level);
                }
            }
        }
        for s in 0..input_count {
            let Some(level) = line_level[s] else {
                continue;
            };
            if vertex_of_sub[s] != NO_VERTEX {
                let z = &mut vertices[vertex_of_sub[s] as usize].z;
                if *z != level {
                    *z = level;
                    on_lines += 1;
                }
            } else {
                sub_level[s] = level;
            }
        }
    }

    // ---- the result -----------------------------------------------------------
    let mut output = PatchOutput {
        relevelled: relevelled + on_lines,
        ..PatchOutput::default()
    };
    let mut result_of: Vec<u32> = vec![NO_VERTEX; input_count + generated_count];
    for s in 0..input_count {
        if vertex_of_sub[s] != NO_VERTEX {
            result_of[s] = vertex_of_sub[s];
        } else {
            result_of[s] = vertices.len() as u32;
            vertices.push(Point3::new(positions[s].x, positions[s].y, sub_level[s]));
            output.new_vertices += 1;
        }
    }
    for g in 0..generated_count {
        result_of[input_count + g] = vertices.len() as u32;
        vertices.push(Point3::new(
            cdt.generated_points[g].x,
            cdt.generated_points[g].y,
            generated_level[g],
        ));
    }
    // A paste's seam: a surface vertex lying on the top's rim takes its level.
    if request.cut_has_levels && triangulated {
        for &c in cut_constraint.iter().flatten() {
            let list = along(c);
            for &x in list.iter().take(list.len().saturating_sub(1)).skip(1) {
                if (x as usize) < input_count && vertex_of_sub[x as usize] != NO_VERTEX {
                    let v = vertex_of_sub[x as usize] as usize;
                    let level = level_along(&infos[c], positions[x as usize]);
                    if vertices[v].z != level {
                        vertices[v].z = level;
                        output.relevelled += 1;
                    }
                }
            }
        }
    }

    // The triangles outside the region stay as they were, those a cut drops
    // apart: not crossed by the cut, each is wholly on one side of it, and so
    // is every piece of them that shares edges (the cut lies only in region
    // triangles; TriangleGrid::mark_along is conservative). So the question is
    // asked once for a piece, at the centroid of its largest triangle as for
    // the pieces of the region above, and not once for each triangle: a
    // polygon of n corners costs n per question, and the triangles are many.
    let mut kept_outside: Vec<u8> = Vec::new();
    if !request.cuts.is_empty() {
        kept_outside = vec![0; triangles.len()];
        let mut seen: Vec<u8> = vec![0; triangles.len()];
        let mut piece: Vec<u32> = Vec::new();
        let mut stack: Vec<u32> = Vec::new();
        for seed in 0..triangles.len() as u32 {
            if region[seed as usize] != 0 || seen[seed as usize] != 0 {
                continue;
            }
            piece.clear();
            piece.push(seed);
            stack.clear();
            stack.push(seed);
            seen[seed as usize] = 1;
            while let Some(t) = stack.pop() {
                for &next in &neighbors[t as usize] {
                    if next != NO_TRIANGLE && region[next as usize] == 0 && seen[next as usize] == 0
                    {
                        seen[next as usize] = 1;
                        piece.push(next);
                        stack.push(next);
                    }
                }
            }
            let mut largest = seed;
            let mut largest_area = -1.0;
            for &t in &piece {
                let area = surface.plan_triangle(t as usize).signed_area();
                if area > largest_area {
                    largest_area = area;
                    largest = t;
                }
            }
            let inside = inside_cut(surface.plan_triangle(largest as usize).centroid());
            if inside == request.keep_inside_cut {
                for &t in &piece {
                    kept_outside[t as usize] = 1;
                }
            }
        }
    }
    for t in 0..triangles.len() {
        if region[t] != 0 || (!request.cuts.is_empty() && kept_outside[t] == 0) {
            continue;
        }
        output.triangles.push(triangles[t]);
        output.masks.push(masks[t]);
    }
    let mut used_by_new: Vec<u8> = vec![0; vertices.len()];
    let mut new_edges = EdgeSet::default();
    for (t, &kept) in keep.iter().enumerate() {
        if kept == 0 {
            continue;
        }
        let tri = cdt.triangles[t];
        let mapped: TinTriangle = [
            result_of[tri[0] as usize],
            result_of[tri[1] as usize],
            result_of[tri[2] as usize],
        ];
        let mut mask = 0u8;
        for k in 0..3 {
            if (cdt.edge_tags[t][k] & (TAG_KEPT | TAG_LINE | TAG_CUT)) != 0 {
                mask |= 1 << k;
            }
            used_by_new[mapped[k] as usize] = 1;
            let u = tri[k];
            let w = tri[(k + 1) % 3];
            new_edges.add(u.min(w), u.max(w));
        }
        output.triangles.push(mapped);
        output.masks.push(mask);
    }
    new_edges.seal();
    for g in 0..generated_count {
        if used_by_new[result_of[input_count + g] as usize] != 0 {
            output.crossings += 1;
        }
    }

    // ---- what had to land, did -------------------------------------------------
    for (i, point) in request.points.iter().enumerate() {
        let s = point_sub[i] as usize;
        if point.required
            && vertex_of_sub[s] == NO_VERTEX
            && used_by_new[result_of[s] as usize] == 0
        {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the point could not be joined to the surface: it lies off it",
            )
            .with_context(format!("at {}", describe_position(point.position))));
        }
    }
    if triangulated {
        for c in line_begin..line_end {
            for pair in along(c).windows(2) {
                if !new_edges.has(pair[0].min(pair[1]), pair[0].max(pair[1])) {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "the breakline leaves the surface",
                    )
                    .with_context(format!("near {}", describe_position(position_of(pair[0])))));
                }
            }
        }
    }

    output.point_vertex = point_sub.iter().map(|&s| result_of[s as usize]).collect();
    output.cut_vertices = cut_constraint
        .iter()
        .map(|cut| match cut {
            Some(c) => along(*c).iter().map(|&x| result_of[x as usize]).collect(),
            None => Vec::new(),
        })
        .collect();
    output.vertices = vertices;
    Ok(Attempt {
        output: Some(output),
        grow: Vec::new(),
    })
}

pub(crate) fn within_magnitude(value: f64) -> bool {
    value.abs() <= MOST_MAGNITUDE
}

pub(crate) fn too_large(where_: &str) -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        format!(
            "a coordinate or level beyond {} m is refused: areas and volumes of the surface \
             would overflow",
            format_exact_real(MOST_MAGNITUDE)
        ),
    )
    .with_context(format!("at {where_}"))
}

/// A plan position as a message gives it: "(x, y)" with every digit.
pub(crate) fn describe_position(position: Point2) -> String {
    format!(
        "({}, {})",
        format_exact_real(position.x),
        format_exact_real(position.y)
    )
}

/// `grid`, when given, is the TriangleGrid of `surface` the caller already
/// built (cut_holes asks the same grid which footprints can share a patch);
/// otherwise one is built here when a line or a cut needs it.
pub(crate) fn rebuild_patch(
    surface: &TinSurface,
    stars: &VertexStars,
    request: PatchRequest,
    grid: Option<&TriangleGrid>,
) -> Result<PatchOutput> {
    let mut request = request;
    let triangle_count = surface.triangle_count();
    if request.region.len() != triangle_count {
        request.region.resize(triangle_count, 0);
    }
    for point in &request.points {
        if !point.position.is_finite() || point.z.is_some_and(|z| !z.is_finite()) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a point has a non-finite coordinate",
            )
            .with_context(format!("at {}", describe_position(point.position))));
        }
        if !within_magnitude(point.position.x)
            || !within_magnitude(point.position.y)
            || point.z.is_some_and(|z| !within_magnitude(z))
        {
            return Err(too_large(&describe_position(point.position)));
        }
    }
    for segment in request.lines.iter().chain(request.cuts.iter()) {
        if segment.from as usize >= request.points.len()
            || segment.to as usize >= request.points.len()
        {
            return Err(Error::new(
                ErrorCode::Internal,
                "a patch segment names no point",
            ));
        }
    }

    // The triangles everything named reaches: around a removed vertex, under
    // a point, along a segment.
    for v in 0..request.removed.len() {
        if request.removed[v] != 0 {
            for &t in stars.of(v as u32) {
                request.region[t as usize] = 1;
            }
        }
    }
    for point in &request.points {
        mark_reached_by_point(surface, stars, point.position, &mut request.region, None);
    }
    if !request.lines.is_empty() || !request.cuts.is_empty() {
        let own;
        let grid = match grid {
            Some(grid) => grid,
            None => {
                own = TriangleGrid::new(surface);
                &own
            }
        };
        for segment in request.lines.iter().chain(request.cuts.iter()) {
            grid.mark_along(
                request.points[segment.from as usize].position,
                request.points[segment.to as usize].position,
                &mut request.region,
                None,
            );
        }
    }

    for round in 0..=GROWTH_ROUNDS + 1 {
        if round == GROWTH_ROUNDS + 1 {
            request.region.fill(1);
        }
        let attempt = try_patch(surface, &request, &request.region)?;
        if let Some(output) = attempt.output {
            return Ok(output);
        }
        for t in attempt.grow {
            request.region[t as usize] = 1;
        }
    }
    Err(Error::new(
        ErrorCode::TriangulationFailure,
        "the edit could not be confined to the surface: a ring edge was split with every \
         triangle rebuilt",
    ))
}

#[derive(Clone, Copy)]
struct RimEdge {
    from: u32,
    to: u32,
    constrained: bool,
}

/// The triangles that join points off the surface to its rim: the surface's
/// rim vertices and `points` triangulated with the rim edges as constraints,
/// keeping the triangles outside the surface that have one of the points as
/// a corner. The points must all be off the surface (place_of Outside).
pub(crate) fn extend_surface(surface: &TinSurface, points: &[Point3]) -> Result<PatchOutput> {
    let triangles = surface.triangles();
    let neighbors = surface.neighbors();
    let masks = surface.constrained_edges();
    let source_vertices = surface.vertices();

    let mut rim: Vec<RimEdge> = Vec::new();
    let mut on_rim: Vec<u8> = vec![0; surface.vertex_count()];
    for t in 0..triangles.len() {
        for k in 0..3 {
            if neighbors[t][k] == NO_TRIANGLE {
                let a = triangles[t][k];
                let b = triangles[t][(k + 1) % 3];
                rim.push(RimEdge {
                    from: a,
                    to: b,
                    constrained: ((masks[t] >> k) & 1) != 0,
                });
                on_rim[a as usize] = 1;
                on_rim[b as usize] = 1;
            }
        }
    }

    let mut vertices: Vec<Point3> = source_vertices.clone();
    let mut merger = PointMerger::new(
        surface.vertex_count() + points.len() + 1,
        tolerance::GEOMETRIC,
    );
    let mut sub_of_vertex: Vec<u32> = vec![NO_INDEX; surface.vertex_count()];
    let mut vertex_of_sub: Vec<u32> = Vec::new();
    for v in 0..surface.vertex_count() {
        if on_rim[v] == 0 {
            continue;
        }
        let entry = merger.add(plan_of(source_vertices[v]));
        if !entry.inserted {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the surface's rim has two vertices closer than the coincidence tolerance; a \
                 point cannot be joined to it",
            )
            .with_context(format!(
                "at {}",
                describe_position(plan_of(source_vertices[v]))
            )));
        }
        sub_of_vertex[v] = entry.index;
        vertex_of_sub.push(v as u32);
    }
    let rim_count = vertex_of_sub.len();
    let mut point_sub: Vec<u32> = vec![0; points.len()];
    let mut point_level: Vec<f64> = Vec::new();
    // The level a point of this call gave a rim vertex, to refuse two that disagree.
    let mut rim_given: Vec<Option<f64>> = vec![None; rim_count];
    let mut output = PatchOutput::default();
    for (i, point) in points.iter().enumerate() {
        let position = Point2::new(point.x, point.y);
        let entry = merger.add(position);
        point_sub[i] = entry.index;
        if entry.inserted {
            vertex_of_sub.push(NO_VERTEX);
            point_level.push(point.z);
            continue;
        }
        let index = entry.index as usize;
        if index < rim_count {
            if let Some(given) = rim_given[index]
                && (given - point.z).abs() > tolerance::COORDINATE
            {
                return Err(two_levels(position, given, point.z));
            }
            rim_given[index] = Some(point.z);
            let vertex = &mut vertices[vertex_of_sub[index] as usize];
            if vertex.z != point.z {
                vertex.z = point.z;
                output.relevelled += 1;
            }
            continue;
        }
        let earlier = point_level[index - rim_count];
        if (earlier - point.z).abs() > tolerance::COORDINATE {
            return Err(two_levels(position, earlier, point.z));
        }
    }
    let positions: &Vec<Point2> = merger.positions();

    let constraints: Vec<CdtConstraint> = rim
        .iter()
        .map(|edge| CdtConstraint {
            from: sub_of_vertex[edge.from as usize],
            to: sub_of_vertex[edge.to as usize],
            tag: TAG_REGION | if edge.constrained { TAG_KEPT } else { 0 },
        })
        .collect();
    let cdt = cdt_backend::triangulate(&CdtInput {
        points: positions,
        constraints: &constraints,
        export_constraint_vertices: true,
    })?;
    if let Some(&first) = cdt.generated_points.first() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the surface's rim crosses itself; a point cannot be joined to it",
        )
        .with_context(format!("at {}", describe_position(first))));
    }
    let mut ring = EdgeSet::default();
    for (c, constraint) in constraints.iter().enumerate() {
        let begin = cdt.constraint_vertex_offsets[c];
        let end = cdt.constraint_vertex_offsets[c + 1];
        if end - begin != 2 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the surface's rim runs through one of its own vertices; a point cannot be \
                 joined to it",
            )
            .with_context(format!(
                "near {}",
                describe_position(positions[constraint.from as usize])
            )));
        }
        ring.add(constraint.from, constraint.to);
    }
    ring.seal();

    let new_count = cdt.triangles.len();
    let mut inside: Vec<u8> = vec![0; new_count];
    let mut queue: Vec<u32> = Vec::new();
    for (t, flag) in inside.iter_mut().enumerate() {
        let tri = cdt.triangles[t];
        for k in 0..3 {
            if (cdt.edge_tags[t][k] & TAG_REGION) != 0 && ring.has(tri[k], tri[(k + 1) % 3]) {
                *flag = 1;
            }
        }
        if *flag != 0 {
            queue.push(t as u32);
        }
    }
    while let Some(t) = queue.pop() {
        for k in 0..3 {
            let next = cdt.neighbors[t as usize][k];
            if (cdt.edge_tags[t as usize][k] & TAG_REGION) == 0
                && next != NO_INDEX
                && inside[next as usize] == 0
            {
                inside[next as usize] = 1;
                queue.push(next);
            }
        }
    }

    let mut result_of: Vec<u32> = vec![NO_VERTEX; positions.len()];
    for s in 0..positions.len() {
        if vertex_of_sub[s] != NO_VERTEX {
            result_of[s] = vertex_of_sub[s];
        } else {
            result_of[s] = vertices.len() as u32;
            vertices.push(Point3::new(
                positions[s].x,
                positions[s].y,
                point_level[s - rim_count],
            ));
            output.new_vertices += 1;
        }
    }
    output.triangles = triangles.clone();
    output.masks = masks.clone();

    // What is kept: the triangles outside the old footprint that have a new
    // point as a corner - and the pockets those leave closed in. The kernel
    // triangulates the convex hull, so outside the footprint there are also
    // triangles of three rim vertices: the ears of a hole a point in it did
    // not reach, the inside of a notch the new triangles bridge. Left out,
    // a point in a square hole filled only a fan of it and a point beyond a
    // notch walled off a hole that was not there. A piece of those that
    // reaches the hull is open to the outside and is not filled (a concavity
    // the point does not cover is left, as it always was); one that does not
    // reach it, and borders a kept triangle, is shut in by the new triangles
    // and is filled; one that borders none is a hole no point is in, and stays.
    let rim_only = |t: usize| {
        let tri = cdt.triangles[t];
        (tri[0] as usize) < rim_count
            && (tri[1] as usize) < rim_count
            && (tri[2] as usize) < rim_count
    };
    let mut kept: Vec<u8> = (0..new_count)
        .map(|t| u8::from(inside[t] == 0 && !rim_only(t)))
        .collect();
    {
        let mut seen: Vec<u8> = vec![0; new_count];
        let mut piece: Vec<u32> = Vec::new();
        let mut stack: Vec<u32> = Vec::new();
        for seed in 0..new_count {
            if inside[seed] != 0 || kept[seed] != 0 || seen[seed] != 0 {
                continue;
            }
            piece.clear();
            piece.push(seed as u32);
            stack.clear();
            stack.push(seed as u32);
            seen[seed] = 1;
            let mut open = false;
            let mut shut_in = false;
            while let Some(t) = stack.pop() {
                for k in 0..3 {
                    let next = cdt.neighbors[t as usize][k];
                    if next == NO_INDEX {
                        open = true;
                    } else if (cdt.edge_tags[t as usize][k] & TAG_REGION) != 0 {
                        continue; // the old rim: the footprint's side of it is not a pocket
                    } else if kept[next as usize] != 0 {
                        shut_in = true;
                    } else if seen[next as usize] == 0 {
                        seen[next as usize] = 1;
                        piece.push(next);
                        stack.push(next);
                    }
                }
            }
            if !open && shut_in {
                for &t in &piece {
                    kept[t as usize] = 1;
                }
            }
        }
    }

    let mut joined: Vec<u8> = vec![0; positions.len()];
    for (t, &is_kept) in kept.iter().enumerate() {
        let tri = cdt.triangles[t];
        if is_kept == 0 {
            continue;
        }
        let mut mask = 0u8;
        for k in 0..3 {
            if (cdt.edge_tags[t][k] & TAG_KEPT) != 0 {
                mask |= 1 << k;
            }
            joined[tri[k] as usize] = 1;
        }
        output.triangles.push([
            result_of[tri[0] as usize],
            result_of[tri[1] as usize],
            result_of[tri[2] as usize],
        ]);
        output.masks.push(mask);
    }
    for (i, point) in points.iter().enumerate() {
        if point_sub[i] as usize >= rim_count && joined[point_sub[i] as usize] == 0 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the point could not be joined to the surface: it lies in a crack of the rim",
            )
            .with_context(format!(
                "at {}",
                describe_position(Point2::new(point.x, point.y))
            )));
        }
    }
    output.point_vertex = point_sub.iter().map(|&s| result_of[s as usize]).collect();
    output.vertices = vertices;
    Ok(output)
}

/// The arrays made a surface: flags made to agree across every shared edge
/// (a new constrained edge along an old one marks both sides), vertices no
/// triangle uses dropped, triangles rotated and sorted, and the result
/// validated by TinSurface::create. InvalidState when no triangle is left;
/// TriangulationFailure for a vertex left without a level.
pub(crate) fn finish_surface(
    vertices: Vec<Point3>,
    triangles: Vec<TinTriangle>,
    masks: Vec<u8>,
) -> Result<TinSurface> {
    let mut vertices = vertices;
    let mut masks = masks;
    if triangles.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "nothing of the surface would be left",
        ));
    }
    // A constrained edge is constrained on both its sides.
    let mut half_edges: Vec<(u32, u32, u32, u8)> = Vec::with_capacity(3 * triangles.len());
    for (t, tri) in triangles.iter().enumerate() {
        for k in 0..3u8 {
            let a = tri[k as usize];
            let b = tri[(k as usize + 1) % 3];
            half_edges.push((a.min(b), a.max(b), t as u32, k));
        }
    }
    half_edges.sort_unstable_by_key(|half| (half.0, half.1, half.2));
    for pair in half_edges.windows(2) {
        let (x, y) = (pair[0], pair[1]);
        if x.0 != y.0 || x.1 != y.1 {
            continue;
        }
        let flagged =
            ((masks[x.2 as usize] >> x.3) & 1) != 0 || ((masks[y.2 as usize] >> y.3) & 1) != 0;
        if flagged {
            masks[x.2 as usize] |= 1 << x.3;
            masks[y.2 as usize] |= 1 << y.3;
        }
    }

    // Vertices no triangle uses are dropped, keeping the order of the rest.
    let mut renumber: Vec<u32> = vec![NO_VERTEX; vertices.len()];
    for tri in &triangles {
        for &v in tri {
            renumber[v as usize] = 0;
        }
    }
    let mut kept: u32 = 0;
    for v in 0..vertices.len() {
        if renumber[v] == NO_VERTEX {
            continue;
        }
        if !vertices[v].is_finite() {
            return Err(Error::new(
                ErrorCode::TriangulationFailure,
                "a vertex of the edited surface has no level: it lies off the surface",
            )
            .with_context(format!("at {}", describe_position(plan_of(vertices[v])))));
        }
        renumber[v] = kept;
        vertices[kept as usize] = vertices[v]; // monotone: never overwrites what is still to be read
        kept += 1;
    }
    vertices.truncate(kept as usize);

    // Canonical: each triangle starts at its smallest vertex, the triangles sorted.
    let mut entries: Vec<(TinTriangle, u8)> = triangles
        .iter()
        .zip(masks.iter())
        .map(|(tri, &mask)| {
            let mapped = [
                renumber[tri[0] as usize],
                renumber[tri[1] as usize],
                renumber[tri[2] as usize],
            ];
            let mut smallest = 0;
            if mapped[1] < mapped[smallest] {
                smallest = 1;
            }
            if mapped[2] < mapped[smallest] {
                smallest = 2;
            }
            let mut triangle = [0u32; 3];
            let mut entry_mask = 0u8;
            for j in 0..3 {
                triangle[j] = mapped[(j + smallest) % 3];
                if ((mask >> ((j + smallest) % 3)) & 1) != 0 {
                    entry_mask |= 1 << j;
                }
            }
            (triangle, entry_mask)
        })
        .collect();
    entries.sort_by_key(|entry| entry.0);
    let (triangles, masks): (Vec<TinTriangle>, Vec<u8>) = entries.into_iter().unzip();
    TinSurface::create(vertices, triangles, masks)
}

/// The report every edit fills the same way from the surfaces before and
/// after.
pub(crate) fn report_of(before: &TinSurface, after: &TinSurface) -> TinEditReport {
    TinEditReport {
        triangles_before: before.triangle_count(),
        triangles_after: after.triangle_count(),
        vertices_before: before.vertex_count(),
        vertices_after: after.vertex_count(),
        ..TinEditReport::default()
    }
}

/// The surface as it was with an empty report: what an edit that changes
/// nothing returns, not a reordered copy of it (finish_surface sorts, and a
/// surface imported from an archive is not in that order, so an edit that
/// removed nothing would otherwise look like a change and record an undo step).
pub(crate) fn unchanged(surface: &TinSurface) -> TinEditResult {
    TinEditResult {
        surface: surface.clone(),
        report: report_of(surface, surface),
    }
}
