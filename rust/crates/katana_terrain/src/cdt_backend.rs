// The constraint insertion, the crossing construction and the perturbed in-circle test in
// this file are translations into Rust of CGAL 6.2's Constrained_triangulation_plus_2.h
// (Copyright (c) 2012 INRIA Sophia-Antipolis), Constrained_Delaunay_triangulation_2.h and
// Triangulation_2.h (Copyright (c) 1997-2010 INRIA Sophia-Antipolis), used under
// GPL-3.0-or-later, and of Intersections_2/Segment_2_Segment_2.h (Copyright (c) 2000 Utrecht
// University, ETH Zurich, INRIA Sophia-Antipolis and the other holders that file names),
// used under LGPL-3.0-or-later; THIRD_PARTY_NOTICES.md ("Code translated into the Rust
// port") lists them.
//! Internal interface to the constrained Delaunay triangulation kernel. Everything
//! that touches the triangulation library (spade) lives here; the rest of the
//! crate sees Katana types only.
//!
//! The C++ backend is CGAL's Constrained_triangulation_plus_2 over
//! Exact_predicates_inexact_constructions_kernel with Exact_predicates_tag
//! (CGAL 6.2). This port reproduces its answer, not its algorithm:
//!
//! * WHICH triangulation. CGAL decides "is this edge Delaunay?" with its
//!   symbolically perturbed in-circle test (Triangulation_2.h,
//!   side_of_oriented_circle with perturb = true): four cocircular points are
//!   resolved by the lexicographic order of the points, as if each were lifted
//!   by an infinitesimal of its own rank. That makes the constrained Delaunay
//!   triangulation of a given vertex set and constraint set UNIQUE - whatever
//!   order the points were inserted in, a regular grid gets the same diagonal in
//!   every cell. spade builds a constrained Delaunay triangulation that breaks
//!   such ties its own way; `normalize` then flips, with CGAL's perturbed
//!   predicate ported line for line, every unconstrained edge that is not
//!   Delaunay under it (Lawson's flip algorithm, which terminates at the unique
//!   triangulation because the perturbation is a lifting in general position).
//!   So CGAL's spatial sort, which only chooses the insertion order, is not
//!   ported: the result does not depend on it.
//! * WHERE constraints cross. `Planarizer` replays Constrained_triangulation_plus_2's
//!   insert_subconstraint: constraints in input order; along each, from its
//!   start, the first event decides - a vertex lying exactly on it splits it
//!   there, the first constrained edge it properly crosses is cut at a vertex
//!   computed by CGAL's own construction (Segment_2_Segment_2.h: endpoints in
//!   lexicographic order, s2s2_alpha with its fused multiply-adds, the
//!   barycentre), snapped as CGAL snaps it (a point within four ulps of an end
//!   of either segment is that end; Constrained_triangulation_2.h,
//!   intersection with Exact_predicates_tag), and the crossed constraint is
//!   split there and re-inserted piece by piece, as CGAL does. The "first"
//!   event along a segment is the same in every constrained triangulation of
//!   the vertices and the constrained edges inserted so far (constrained edges
//!   are edges of all of them), so the walk needs no triangulation.
//! * What is NOT reproduced: CGAL's check that a constructed crossing lies in
//!   one of the two triangles beside the edge it crosses (insert_intersection,
//!   intersection_not_in_the_two_triangles), with its recomputation in exact
//!   arithmetic and its snap to the nearer end of the crossed edge. It depends
//!   on the triangles of the moment and fires only when the rounded crossing
//!   falls outside them: a crossing within rounding of an end beyond the four
//!   ulps above, a sliver beside the crossed edge, or nearly parallel
//!   constraints. The callers bend a constraint through any point within
//!   tolerance::GEOMETRIC of it before asking (tin_patch, points_on_constraints)
//!   and check afterwards that every vertex a constraint runs through lies on
//!   it, as they do for CGAL.
//!
//! Orientation and in-circle signs are exact (the robust crate's adaptive
//! predicates, Shewchuk's), as CGAL's filtered predicates are.

use std::collections::{HashMap, HashSet};

use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::Point2;
use spade::{ConstrainedDelaunayTriangulation, Point2 as SpadePoint, Triangulation};

use crate::tin_surface::cell_coordinate;

pub(crate) const NO_INDEX: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CdtConstraint {
    /// indices into CdtInput::points
    pub from: u32,
    pub to: u32,
    /// non-zero bit mask, reported back per constrained edge
    pub tag: u8,
}

impl Default for CdtConstraint {
    fn default() -> Self {
        Self {
            from: 0,
            to: 0,
            tag: 1,
        }
    }
}

pub(crate) struct CdtInput<'a> {
    /// Pairwise distinct (exactly). At most 2^32 - 2 points.
    pub points: &'a [Point2],
    /// May cross, overlap, repeat and pass through points; from == to is ignored.
    pub constraints: &'a [CdtConstraint],
    pub export_constraint_vertices: bool,
}

/// Canonical triangulation of the convex hull of the points.
///   * Vertex i < points.len() is input point i; vertex points.len() + j is
///     generated_points[j], a crossing of two constraints. Generated points are
///     sorted by (x, y).
///   * Triangles are counter-clockwise, start at their smallest vertex index and
///     are sorted lexicographically. Edge k joins vertex k and vertex (k + 1) % 3.
///   * neighbors[t][k] is the triangle across edge k, or NO_INDEX on the hull.
///   * edge_tags[t][k] is the OR of the tags of all constraints running along edge
///     k (0 = unconstrained).
///   * With export_constraint_vertices, constraint_vertices[constraint_vertex_offsets[c]
///     .. constraint_vertex_offsets[c + 1]] are the vertices along constraint c from
///     `from` to `to`: its end points plus every vertex that splits it (crossing
///     points and input points lying exactly on it). Empty for ignored constraints.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CdtOutput {
    pub generated_points: Vec<Point2>,
    pub triangles: Vec<[u32; 3]>,
    pub neighbors: Vec<[u32; 3]>,
    pub edge_tags: Vec<[u8; 3]>,
    pub constraint_vertex_offsets: Vec<u32>,
    pub constraint_vertices: Vec<u32>,
}

fn coord(point: Point2) -> robust::Coord<f64> {
    robust::Coord {
        x: point.x,
        y: point.y,
    }
}

/// The exact sign of the turn a -> b -> c: +1 when c lies to the left of the
/// directed line a -> b (a, b, c counter-clockwise), -1 to its right, 0 when
/// the three are collinear. The predicate the triangulation itself decides
/// with, so an edit that asks "which side of this edge?" gets the answer the
/// triangulation gives for the same doubles (docs/surface_editing.md, "Exact
/// where it decides").
pub(crate) fn orientation(a: Point2, b: Point2, c: Point2) -> i32 {
    let determinant = robust::orient2d(coord(a), coord(b), coord(c));
    if determinant > 0.0 {
        1
    } else if determinant < 0.0 {
        -1
    } else {
        0
    }
}

/// CGAL's compare_xy: x first, then y.
fn less_xy(p: Point2, q: Point2) -> bool {
    p.x < q.x || (p.x == q.x && p.y < q.y)
}

fn compare_xy(p: Point2, q: Point2) -> std::cmp::Ordering {
    if less_xy(p, q) {
        std::cmp::Ordering::Less
    } else if less_xy(q, p) {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// Triangulation_2::side_of_oriented_circle(p0, p1, p2, p, perturb = true):
/// +1 when p lies inside the circle through the counter-clockwise p0, p1, p2,
/// -1 outside; on the circle, the symbolic perturbation decides.
fn side_of_oriented_circle(p0: Point2, p1: Point2, p2: Point2, p: Point2) -> i32 {
    let determinant = robust::incircle(coord(p0), coord(p1), coord(p2), coord(p));
    if determinant > 0.0 {
        return 1;
    }
    if determinant < 0.0 {
        return -1;
    }
    // A degenerate case: the symbolic perturbation. The points sorted
    // lexicographically; the leading monomial, then the second, of the
    // determinant decide (two iterations are enough when p0 p1 p2 is positive).
    let mut points = [(p0, 0u8), (p1, 1u8), (p2, 2u8), (p, 3u8)];
    points.sort_by(|x, y| compare_xy(x.0, y.0));
    for i in (1..=3).rev() {
        match points[i].1 {
            3 => return -1, // p0 p1 p2 are not collinear and positively oriented
            2 => {
                let o = orientation(p0, p1, p);
                if o != 0 {
                    return o;
                }
            }
            1 => {
                let o = orientation(p0, p, p2);
                if o != 0 {
                    return o;
                }
            }
            _ => {
                let o = orientation(p, p1, p2);
                if o != 0 {
                    return o;
                }
            }
        }
    }
    -1
}

/// Triangulation_2::collinear_between: q strictly between p and r, the three
/// being collinear.
fn collinear_between(p: Point2, q: Point2, r: Point2) -> bool {
    use std::cmp::Ordering::{Greater, Less};
    let (pq, qr) = if p.x == r.x {
        (p.y.partial_cmp(&q.y), q.y.partial_cmp(&r.y))
    } else {
        (p.x.partial_cmp(&q.x), q.x.partial_cmp(&r.x))
    };
    (pq == Some(Less) && qr == Some(Less)) || (pq == Some(Greater) && qr == Some(Greater))
}

/// boost::math::float_advance by `steps` ulps (negative: down).
fn float_advance(value: f64, steps: i32) -> f64 {
    let mut result = value;
    for _ in 0..steps.unsigned_abs() {
        result = if steps < 0 {
            result.next_down()
        } else {
            result.next_up()
        };
    }
    result
}

/// CGAL's s2s2_alpha for doubles (Segment_2_Segment_2.h), its fused
/// multiply-adds included: CGAL calls std::fma here.
#[allow(clippy::too_many_arguments, reason = "Mirrors CGAL's s2s2_alpha.")]
fn s2s2_alpha(x0: f64, y0: f64, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> f64 {
    let s1_dx = x0 - x1;
    let s1_dy = y0 - y1;
    let s2_dx = x3 - x2;
    let s2_dy = y3 - y2;
    let lx = x3 - x1;
    let ly = y3 - y1;
    let value = lx.mul_add(s2_dy, -ly * s2_dx) / s1_dx.mul_add(s2_dy, -s1_dy * s2_dx);
    if value.is_nan() {
        return 0.5;
    }
    if value < 0.0 {
        return 0.0;
    }
    if value > 1.0 {
        return 1.0;
    }
    value
}

/// The crossing of the segments pa-pb and pc-pd, which cross in their
/// interiors, as CGAL's Epick constructs it (Segment_2_Segment_2_pair::
/// intersection_type) and then snaps it (intersection with
/// Exact_predicates_tag and a floating-point number type).
fn intersection_point(pa: Point2, pb: Point2, pc: Point2, pd: Point2) -> Point2 {
    let (a1, a2) = if less_xy(pa, pb) { (pa, pb) } else { (pb, pa) };
    let (b1, b2) = if less_xy(pc, pd) { (pc, pd) } else { (pd, pc) };
    // The segment with the lexicographically smaller first end comes first,
    // each running from its smaller end (the s2s2_id table's eight cases).
    let pts = if less_xy(a1, b1) {
        [a1, a2, b1, b2]
    } else {
        [b1, b2, a1, a2]
    };
    let point = if pts[0].x == pts[1].x && pts[2].y == pts[3].y {
        Point2::new(pts[0].x, pts[2].y)
    } else if pts[0].y == pts[1].y && pts[2].x == pts[3].x {
        Point2::new(pts[2].x, pts[0].y)
    } else {
        let alpha = s2s2_alpha(
            pts[0].x, pts[0].y, pts[1].x, pts[1].y, pts[2].x, pts[2].y, pts[3].x, pts[3].y,
        );
        // barycenterC2: w1 * p1 + (1 - w1) * p2.
        let w2 = 1.0 - alpha;
        Point2::new(
            alpha * pts[0].x + w2 * pts[1].x,
            alpha * pts[0].y + w2 * pts[1].y,
        )
    };
    if point == pa || point == pb || point == pc || point == pd {
        return point;
    }
    // CGAL_CDT_2_INTERSECTION_SNAPPING_ULP_DISTANCE, 4 by default: the point's
    // box dilated by four ulps; an end inside it is the crossing (the last
    // of pa, pb, pc, pd that is wins).
    const SNAP_ULPS: i32 = 4;
    let x_low = float_advance(point.x, -SNAP_ULPS);
    let x_high = float_advance(point.x, SNAP_ULPS);
    let y_low = float_advance(point.y, -SNAP_ULPS);
    let y_high = float_advance(point.y, SNAP_ULPS);
    let overlaps = |q: Point2| !(x_high < q.x || q.x < x_low || y_high < q.y || q.y < y_low);
    let mut snapped = point;
    for end in [pa, pb, pc, pd] {
        if overlaps(end) {
            snapped = end;
        }
    }
    snapped
}

fn key(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

/// The first thing met walking a segment from its start (find_intersected_faces).
enum Event {
    /// The segment reaches its end.
    Clear,
    /// A vertex lies exactly on it.
    Vertex(u32),
    /// It properly crosses the constrained edge c -> d; the start lies to the
    /// right of c -> d (CGAL's vcc = f->vertex(cw(i)), vdd = f->vertex(ccw(i))
    /// of the face on the start's side).
    Crossing(u32, u32),
}

/// Vertices and constrained edges by position, for "what lies along this
/// segment?" without a pass over all of them.
struct EventGrid {
    origin: Point2,
    cell_width: f64,
    cell_height: f64,
    columns: u32,
    rows: u32,
    vertices: Vec<Vec<u32>>,
    edges: Vec<Vec<(u32, u32)>>,
}

impl EventGrid {
    fn new(points: &[Point2]) -> Self {
        let mut min = points[0];
        let mut max = points[0];
        for p in points {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        let width = max.x - min.x;
        let height = max.y - min.y;
        let target = (points.len() as f64).max(1.0);
        let mut columns = 1u32;
        let mut rows = 1u32;
        if width > 0.0 && height > 0.0 {
            let cell = (width * height / target).sqrt();
            let limit = 2.0 * target;
            columns = (width / cell).ceil().clamp(1.0, limit) as u32;
            rows = (height / cell).ceil().clamp(1.0, limit) as u32;
        } else if width > 0.0 {
            columns = target.ceil() as u32;
        } else if height > 0.0 {
            rows = target.ceil() as u32;
        }
        let cells = columns as usize * rows as usize;
        Self {
            origin: min,
            cell_width: if width > 0.0 {
                width / f64::from(columns)
            } else {
                1.0
            },
            cell_height: if height > 0.0 {
                height / f64::from(rows)
            } else {
                1.0
            },
            columns,
            rows,
            vertices: vec![Vec::new(); cells],
            edges: vec![Vec::new(); cells],
        }
    }

    fn cell_of(&self, p: Point2) -> usize {
        cell_coordinate(p.y, self.origin.y, self.cell_height, self.rows) as usize
            * self.columns as usize
            + cell_coordinate(p.x, self.origin.x, self.cell_width, self.columns) as usize
    }

    /// Every cell the segment a-b passes through, with a margin of a cell all
    /// round: conservative, so that the rounding of the cell arithmetic can
    /// never hide a vertex on the segment or an edge crossing it.
    fn cells_along(&self, a: Point2, b: Point2, mut visit: impl FnMut(usize)) {
        let row_of = |y: f64| cell_coordinate(y, self.origin.y, self.cell_height, self.rows);
        let column_of = |x: f64| cell_coordinate(x, self.origin.x, self.cell_width, self.columns);
        let row0 = row_of(a.y.min(b.y)).saturating_sub(1);
        let row1 = (row_of(a.y.max(b.y)) + 1).min(self.rows - 1);
        for row in row0..=row1 {
            let band_low = self.origin.y + (f64::from(row) - 1.0) * self.cell_height;
            let band_high = self.origin.y + (f64::from(row) + 2.0) * self.cell_height;
            let (x_low, x_high) = if b.y == a.y {
                (a.x.min(b.x), a.x.max(b.x))
            } else {
                let t0 = (band_low - a.y) / (b.y - a.y);
                let t1 = (band_high - a.y) / (b.y - a.y);
                let low = t0.min(t1).max(0.0);
                let high = t0.max(t1).min(1.0);
                if low > high {
                    continue;
                }
                let first = a.x + (b.x - a.x) * low;
                let second = a.x + (b.x - a.x) * high;
                (first.min(second), first.max(second))
            };
            let column0 = column_of(x_low).saturating_sub(1);
            let column1 = (column_of(x_high) + 1).min(self.columns - 1);
            for column in column0..=column1 {
                visit(row as usize * self.columns as usize + column as usize);
            }
        }
    }
}

/// Constrained_triangulation_plus_2's constraint insertion, replayed on the
/// geometry alone (see the module comment).
struct Planarizer {
    positions: Vec<Point2>,
    /// Per input constraint, its vertices from its `from` (empty: ignored).
    lists: Vec<Vec<u32>>,
    /// Per sub-constraint (unordered vertex pair), the constraints running along it.
    contexts: HashMap<(u32, u32), Vec<u32>>,
    /// The sub-constraints that are edges of the triangulation (CGAL's
    /// is_constrained), as opposed to those still waiting on the stack.
    realized: HashSet<(u32, u32)>,
    grid: EventGrid,
    /// A guard against a walk that makes no progress: CGAL would loop; this
    /// reports a kernel failure instead.
    steps_left: u64,
}

fn kernel_failed(context: &str) -> Error {
    Error::new(
        ErrorCode::TriangulationFailure,
        "triangulation kernel failed",
    )
    .with_context(context)
}

impl Planarizer {
    fn new(points: &[Point2], constraint_count: usize) -> Self {
        let grid = EventGrid::new(points);
        let mut planarizer = Self {
            positions: points.to_vec(),
            lists: vec![Vec::new(); constraint_count],
            contexts: HashMap::new(),
            realized: HashSet::new(),
            grid,
            steps_left: 64u64
                .saturating_mul(points.len() as u64 + 16)
                .saturating_mul(constraint_count as u64 + 16),
        };
        for (i, &p) in points.iter().enumerate() {
            let cell = planarizer.grid.cell_of(p);
            planarizer.grid.vertices[cell].push(i as u32);
        }
        planarizer
    }

    fn realize(&mut self, a: u32, b: u32) {
        if self.realized.insert(key(a, b)) {
            let (pa, pb) = (self.positions[a as usize], self.positions[b as usize]);
            let edge = key(a, b);
            let grid = &mut self.grid;
            let mut cells = Vec::new();
            grid.cells_along(pa, pb, |cell| cells.push(cell));
            for cell in cells {
                grid.edges[cell].push(edge);
            }
        }
    }

    fn unrealize(&mut self, a: u32, b: u32) {
        self.realized.remove(&key(a, b));
    }

    /// Polyline_constraint_hierarchy_2::split_constraint (add_Steiner): every
    /// constraint running along a-b runs through v.
    fn split(&mut self, a: u32, b: u32, v: u32) {
        let Some(ids) = self.contexts.remove(&key(a, b)) else {
            return; // the sub-constraint is already split
        };
        for &id in &ids {
            let list = &mut self.lists[id as usize];
            if let Some(at) = list
                .windows(2)
                .position(|pair| (pair[0] == a && pair[1] == b) || (pair[0] == b && pair[1] == a))
            {
                list.insert(at + 1, v);
            }
        }
        self.contexts
            .entry(key(v, b))
            .or_default()
            .extend(ids.iter().copied());
        self.contexts.entry(key(a, v)).or_default().extend(ids);
    }

    /// The vertex at `p`: the one already there, or a new one. A new vertex on
    /// a constrained edge splits it, as an insertion into a constrained edge
    /// does in CGAL.
    fn insert_point(&mut self, p: Point2) -> u32 {
        let cell = self.grid.cell_of(p);
        if let Some(&existing) = self.grid.vertices[cell]
            .iter()
            .find(|&&v| self.positions[v as usize] == p)
        {
            return existing;
        }
        let v = self.positions.len() as u32;
        self.positions.push(p);
        self.grid.vertices[cell].push(v);
        let mut candidates: Vec<(u32, u32)> = Vec::new();
        self.grid.cells_along(p, p, |cell| {
            candidates.extend(self.grid.edges[cell].iter().copied())
        });
        candidates.sort_unstable();
        candidates.dedup();
        for (e0, e1) in candidates {
            if !self.realized.contains(&(e0, e1)) {
                continue;
            }
            let (q0, q1) = (self.positions[e0 as usize], self.positions[e1 as usize]);
            if orientation(q0, q1, p) == 0 && collinear_between(q0, p, q1) {
                self.unrealize(e0, e1);
                self.split(e0, e1, v);
                self.realize(e0, v);
                self.realize(v, e1);
                break;
            }
        }
        v
    }

    /// The first vertex on, or constrained edge across, the open segment a-b,
    /// nearest a.
    fn first_event(&self, a: u32, b: u32) -> Event {
        let pa = self.positions[a as usize];
        let pb = self.positions[b as usize];
        let mut vertices: Vec<u32> = Vec::new();
        let mut edges: Vec<(u32, u32)> = Vec::new();
        self.grid.cells_along(pa, pb, |cell| {
            vertices.extend(self.grid.vertices[cell].iter().copied());
            edges.extend(self.grid.edges[cell].iter().copied());
        });
        vertices.sort_unstable();
        vertices.dedup();
        edges.sort_unstable();
        edges.dedup();

        let d = pb - pa;
        let length_squared = d.length_squared();
        // (parameter along a-b, 0 for a vertex and 1 for an edge, id) - the
        // smallest wins.
        let mut best: Option<(f64, u8, u32, Event)> = None;
        let mut consider = |t: f64, kind: u8, id: u32, event: Event| {
            let better = match &best {
                None => true,
                Some((bt, bk, bid, _)) => {
                    t < *bt || (t == *bt && (kind < *bk || (kind == *bk && id < *bid)))
                }
            };
            if better {
                best = Some((t, kind, id, event));
            }
        };
        for v in vertices {
            if v == a || v == b {
                continue;
            }
            let q = self.positions[v as usize];
            if orientation(pa, pb, q) == 0 && collinear_between(pa, q, pb) {
                consider((q - pa).dot(d) / length_squared, 0, v, Event::Vertex(v));
            }
        }
        for (index, (e0, e1)) in edges.into_iter().enumerate() {
            if e0 == a || e0 == b || e1 == a || e1 == b || !self.realized.contains(&(e0, e1)) {
                continue;
            }
            let q0 = self.positions[e0 as usize];
            let q1 = self.positions[e1 as usize];
            let s0 = orientation(pa, pb, q0);
            let s1 = orientation(pa, pb, q1);
            if s0 == 0 || s1 == 0 || s0 == s1 {
                continue;
            }
            let side_a = robust::orient2d(coord(q0), coord(q1), coord(pa));
            let side_b = robust::orient2d(coord(q0), coord(q1), coord(pb));
            if side_a == 0.0 || side_b == 0.0 || (side_a > 0.0) == (side_b > 0.0) {
                continue;
            }
            let t = side_a / (side_a - side_b);
            // a lies to the right of c -> d.
            let event = if side_a < 0.0 {
                Event::Crossing(e0, e1)
            } else {
                Event::Crossing(e1, e0)
            };
            consider(t, 1, index as u32, event);
        }
        match best {
            Some((_, _, _, event)) => event,
            None => Event::Clear,
        }
    }

    fn tick(&mut self) -> Status {
        if self.steps_left == 0 {
            return Err(kernel_failed("constraint insertion made no progress"));
        }
        self.steps_left -= 1;
        Ok(())
    }

    /// Constrained_triangulation_plus_2::intersect with Exact_predicates_tag:
    /// the constrained edge c-d crossed by the sub-constraint a-b.
    fn intersect(&mut self, c: u32, d: u32, a: u32, b: u32) -> Result<u32> {
        let pi = intersection_point(
            self.positions[a as usize],
            self.positions[b as usize],
            self.positions[c as usize],
            self.positions[d as usize],
        );
        self.unrealize(c, d); // remove_constrained_edge
        let vi = self.insert_point(pi);
        // vi == c or vi == d may happen even if the crossing was computed,
        // through the approximate construction.
        if vi != c && vi != d {
            self.split(c, d, vi);
            self.insert_subconstraint(c, vi)?;
            self.insert_subconstraint(vi, d)?;
        } else {
            self.insert_subconstraint(c, d)?;
        }
        Ok(vi)
    }

    /// Constrained_triangulation_plus_2::insert_subconstraint.
    fn insert_subconstraint(&mut self, start: u32, end: u32) -> Status {
        let mut stack = vec![(start, end)];
        while let Some((a, b)) = stack.pop() {
            self.tick()?;
            match self.first_event(a, b) {
                Event::Clear => self.realize(a, b),
                Event::Vertex(v) => {
                    self.split(a, b, v);
                    self.realize(a, v);
                    stack.push((v, b));
                }
                Event::Crossing(c, d) => {
                    let vi = self.intersect(c, d, a, b)?;
                    if vi != a && vi != b {
                        self.split(a, b, vi);
                        stack.push((a, vi));
                        stack.push((vi, b));
                    } else {
                        stack.push((a, b));
                    }
                }
            }
        }
        Ok(())
    }

    fn insert_constraint(&mut self, id: usize, from: u32, to: u32) -> Status {
        self.lists[id] = vec![from, to];
        self.contexts
            .entry(key(from, to))
            .or_default()
            .push(id as u32);
        self.insert_subconstraint(from, to)
    }
}

/// Bits of a 16-bit value spread to the even bits of a 32-bit one.
fn spread_bits(value: u32) -> u32 {
    let mut v = value & 0xFFFF;
    v = (v | (v << 8)) & 0x00FF_00FF;
    v = (v | (v << 4)) & 0x0F0F_0F0F;
    v = (v | (v << 2)) & 0x3333_3333;
    v = (v | (v << 1)) & 0x5555_5555;
    v
}

/// An insertion order that keeps spade's point location short (Z order).
/// Only speed depends on it: the result is normalised afterwards.
fn spatial_order(points: &[Point2]) -> Vec<usize> {
    let mut min = points[0];
    let mut max = points[0];
    for p in points {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    let extent = (max.x - min.x).max(max.y - min.y);
    let scale = if extent > 0.0 { 65535.0 / extent } else { 0.0 };
    let mut keyed: Vec<(u32, usize)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let qx = ((p.x - min.x) * scale).clamp(0.0, 65535.0) as u32;
            let qy = ((p.y - min.y) * scale).clamp(0.0, 65535.0) as u32;
            (spread_bits(qx) | (spread_bits(qy) << 1), i)
        })
        .collect();
    keyed.sort_unstable();
    keyed.into_iter().map(|(_, i)| i).collect()
}

/// Flips every unconstrained edge that is not Delaunay under CGAL's perturbed
/// in-circle test, until none is left: the unique constrained Delaunay
/// triangulation CGAL builds.
fn normalize(
    positions: &[Point2],
    triangles: &mut [[u32; 3]],
    neighbors: &mut [[u32; 3]],
    constrained: &mut [[bool; 3]],
) {
    let mut stack: Vec<(u32, u8)> = Vec::with_capacity(3 * triangles.len());
    for t in 0..triangles.len() {
        for k in 0..3u8 {
            let n = neighbors[t][k as usize];
            if n != NO_INDEX && (t as u32) < n && !constrained[t][k as usize] {
                stack.push((t as u32, k));
            }
        }
    }
    while let Some((t, k)) = stack.pop() {
        let (t, k) = (t as usize, k as usize);
        let n = neighbors[t][k];
        if n == NO_INDEX || constrained[t][k] {
            continue;
        }
        let n = n as usize;
        let a = triangles[t][k];
        let b = triangles[t][(k + 1) % 3];
        let c = triangles[t][(k + 2) % 3];
        let Some(j) = (0..3).find(|&j| triangles[n][j] == b && triangles[n][(j + 1) % 3] == a)
        else {
            continue;
        };
        // is_flipable(f, i): the circle of the neighbour against f's vertex
        // opposite the edge.
        let [n0, n1, n2] = triangles[n];
        if side_of_oriented_circle(
            positions[n0 as usize],
            positions[n1 as usize],
            positions[n2 as usize],
            positions[c as usize],
        ) <= 0
        {
            continue;
        }
        let d = triangles[n][(j + 2) % 3];
        let n_bc = neighbors[t][(k + 1) % 3];
        let c_bc = constrained[t][(k + 1) % 3];
        let n_ca = neighbors[t][(k + 2) % 3];
        let c_ca = constrained[t][(k + 2) % 3];
        let n_ad = neighbors[n][(j + 1) % 3];
        let c_ad = constrained[n][(j + 1) % 3];
        let n_db = neighbors[n][(j + 2) % 3];
        let c_db = constrained[n][(j + 2) % 3];
        triangles[t] = [a, d, c];
        neighbors[t] = [n_ad, n as u32, n_ca];
        constrained[t] = [c_ad, false, c_ca];
        triangles[n] = [d, b, c];
        neighbors[n] = [n_db, n_bc, t as u32];
        constrained[n] = [c_db, c_bc, false];
        if n_bc != NO_INDEX
            && let Some(entry) = neighbors[n_bc as usize]
                .iter_mut()
                .find(|entry| **entry == t as u32)
        {
            *entry = n as u32;
        }
        if n_ad != NO_INDEX
            && let Some(entry) = neighbors[n_ad as usize]
                .iter_mut()
                .find(|entry| **entry == n as u32)
        {
            *entry = t as u32;
        }
        stack.push((t as u32, 0));
        stack.push((t as u32, 2));
        stack.push((n as u32, 0));
        stack.push((n as u32, 1));
    }
}

/// Fails with TriangulationFailure when there are fewer than 3 points, all points
/// are collinear, or the kernel reports an error; with InvalidArgument for
/// out-of-range constraint indices or duplicate points.
pub(crate) fn triangulate(input: &CdtInput) -> Result<CdtOutput> {
    if input.points.len() < 3 {
        return Err(Error::new(
            ErrorCode::TriangulationFailure,
            "a triangulation needs at least 3 distinct positions",
        )
        .with_context(format!("distinct positions={}", input.points.len())));
    }
    if input.points.len() >= (NO_INDEX - 1) as usize {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "too many points for 32-bit indices",
        )
        .with_context(format!("points={}", input.points.len())));
    }
    for (c, constraint) in input.constraints.iter().enumerate() {
        if constraint.from as usize >= input.points.len()
            || constraint.to as usize >= input.points.len()
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "constraint names a point out of range",
            )
            .with_context(format!("constraint={c}")));
        }
        if constraint.tag == 0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "constraint tag must be non-zero",
            )
            .with_context(format!("constraint={c}")));
        }
    }
    triangulate_unchecked(input)
}

fn triangulate_unchecked(input: &CdtInput) -> Result<CdtOutput> {
    let points = input.points;
    let point_count = points.len();

    // Pairwise distinct. (CGAL names the pair in its insertion order, a
    // Hilbert sort this port does not reproduce; here the lower index first.)
    let mut sorted: Vec<usize> = (0..point_count).collect();
    sorted.sort_unstable_by(|&x, &y| compare_xy(points[x], points[y]).then(x.cmp(&y)));
    for pair in sorted.windows(2) {
        if points[pair[0]] == points[pair[1]] {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "triangulation points must be pairwise distinct",
            )
            .with_context(format!("points {} and {}", pair[0], pair[1])));
        }
    }
    let collinear = (2..point_count).all(|i| orientation(points[0], points[1], points[i]) == 0);
    if collinear {
        return Err(Error::new(
            ErrorCode::TriangulationFailure,
            "all positions are collinear; no triangle can be formed",
        )
        .with_context(format!("distinct positions={point_count}")));
    }

    // Constraints, in input order.
    let mut planarizer = Planarizer::new(points, input.constraints.len());
    for (c, constraint) in input.constraints.iter().enumerate() {
        if constraint.from == constraint.to {
            continue;
        }
        planarizer.insert_constraint(c, constraint.from, constraint.to)?;
    }
    let positions = std::mem::take(&mut planarizer.positions);
    if positions.len() >= NO_INDEX as usize {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "triangulation exceeds the 32-bit index range",
        )
        .with_context(format!("vertices={}", positions.len())));
    }

    // The triangulation of every vertex with every constrained edge.
    let mut cdt: ConstrainedDelaunayTriangulation<SpadePoint<f64>> =
        ConstrainedDelaunayTriangulation::new();
    let mut handles = vec![None; positions.len()];
    let mut mine_of_spade: Vec<u32> = Vec::new();
    for v in spatial_order(&positions) {
        let p = positions[v];
        let handle = cdt
            .insert(SpadePoint::new(p.x, p.y))
            .map_err(|_| kernel_failed("a position the kernel cannot hold"))?;
        let index = handle.index();
        if index >= mine_of_spade.len() {
            mine_of_spade.resize(index + 1, NO_INDEX);
        }
        if mine_of_spade[index] != NO_INDEX {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "triangulation points must be pairwise distinct",
            )
            .with_context(format!("points {} and {v}", mine_of_spade[index])));
        }
        mine_of_spade[index] = v as u32;
        handles[v] = Some(handle);
    }
    let mut constrained_pairs: Vec<(u32, u32)> = planarizer.realized.iter().copied().collect();
    constrained_pairs.sort_unstable();
    for &(a, b) in &constrained_pairs {
        let (Some(from), Some(to)) = (handles[a as usize], handles[b as usize]) else {
            return Err(kernel_failed("a constraint names no vertex"));
        };
        if !cdt.can_add_constraint(from, to) {
            return Err(kernel_failed("constrained edges cross"));
        }
        let _ = cdt.add_constraint(from, to);
    }

    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for face in cdt.inner_faces() {
        let corners = face.vertices();
        let mut tri = [
            mine_of_spade[corners[0].index()],
            mine_of_spade[corners[1].index()],
            mine_of_spade[corners[2].index()],
        ];
        if orientation(
            positions[tri[0] as usize],
            positions[tri[1] as usize],
            positions[tri[2] as usize],
        ) < 0
        {
            tri.swap(1, 2);
        }
        triangles.push(tri);
    }
    if triangles.len() >= NO_INDEX as usize {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "triangulation exceeds the 32-bit index range",
        )
        .with_context(format!("vertices={}", positions.len())));
    }

    // Adjacency and constraint flags of the arrays.
    let mut half_edges: Vec<(u32, u32, u32, u8)> = Vec::with_capacity(3 * triangles.len());
    for (t, tri) in triangles.iter().enumerate() {
        for k in 0..3u8 {
            let a = tri[k as usize];
            let b = tri[(k as usize + 1) % 3];
            half_edges.push((a.min(b), a.max(b), t as u32, k));
        }
    }
    half_edges.sort_unstable();
    let mut neighbors = vec![[NO_INDEX; 3]; triangles.len()];
    for pair in half_edges.windows(2) {
        let (x, y) = (pair[0], pair[1]);
        if x.0 == y.0 && x.1 == y.1 {
            neighbors[x.2 as usize][x.3 as usize] = y.2;
            neighbors[y.2 as usize][y.3 as usize] = x.2;
        }
    }
    let mut constrained = vec![[false; 3]; triangles.len()];
    for (t, tri) in triangles.iter().enumerate() {
        for k in 0..3 {
            constrained[t][k] = planarizer.realized.contains(&key(tri[k], tri[(k + 1) % 3]));
        }
    }
    normalize(&positions, &mut triangles, &mut neighbors, &mut constrained);

    let mut output = CdtOutput::default();

    // Vertices created at constraint crossings, numbered in (x, y) order.
    let mut generated: Vec<u32> = (point_count as u32..positions.len() as u32).collect();
    generated.sort_by(|&x, &y| compare_xy(positions[x as usize], positions[y as usize]));
    let mut renumber: Vec<u32> = (0..positions.len() as u32).collect();
    for (rank, &g) in generated.iter().enumerate() {
        renumber[g as usize] = (point_count + rank) as u32;
        output.generated_points.push(positions[g as usize]);
    }

    // Triangles in canonical rotation, then sorted.
    struct FaceRecord {
        vertices: [u32; 3],
        face: usize,
        /// canonical vertex j is the array's vertex (j + rotation) % 3
        rotation: usize,
    }
    let mut faces: Vec<FaceRecord> = triangles
        .iter()
        .enumerate()
        .map(|(face, tri)| {
            let v = [
                renumber[tri[0] as usize],
                renumber[tri[1] as usize],
                renumber[tri[2] as usize],
            ];
            let mut smallest = 0;
            if v[1] < v[smallest] {
                smallest = 1;
            }
            if v[2] < v[smallest] {
                smallest = 2;
            }
            FaceRecord {
                vertices: [v[smallest], v[(smallest + 1) % 3], v[(smallest + 2) % 3]],
                face,
                rotation: smallest,
            }
        })
        .collect();
    faces.sort_unstable_by_key(|record| record.vertices);
    let mut index_of_face = vec![0u32; triangles.len()];
    for (t, record) in faces.iter().enumerate() {
        index_of_face[record.face] = t as u32;
    }
    for record in &faces {
        output.triangles.push(record.vertices);
        let mut across = [NO_INDEX; 3];
        let mut tags = [0u8; 3];
        for k in 0..3 {
            let edge = (k + record.rotation) % 3;
            let neighbor = neighbors[record.face][edge];
            across[k] = if neighbor == NO_INDEX {
                NO_INDEX
            } else {
                index_of_face[neighbor as usize]
            };
            if constrained[record.face][edge] {
                let tri = triangles[record.face];
                if let Some(ids) = planarizer
                    .contexts
                    .get(&key(tri[edge], tri[(edge + 1) % 3]))
                {
                    tags[k] = ids
                        .iter()
                        .fold(0u8, |all, &c| all | input.constraints[c as usize].tag);
                }
            }
        }
        output.neighbors.push(across);
        output.edge_tags.push(tags);
    }

    if input.export_constraint_vertices {
        output
            .constraint_vertex_offsets
            .reserve(input.constraints.len() + 1);
        output.constraint_vertex_offsets.push(0);
        for (c, constraint) in input.constraints.iter().enumerate() {
            let list = &planarizer.lists[c];
            if !list.is_empty() {
                let first = output.constraint_vertices.len();
                output
                    .constraint_vertices
                    .extend(list.iter().map(|&v| renumber[v as usize]));
                // Documented direction is from -> to, whatever the kernel stores.
                if output.constraint_vertices[first] != constraint.from {
                    output.constraint_vertices[first..].reverse();
                }
            }
            output
                .constraint_vertex_offsets
                .push(output.constraint_vertices.len() as u32);
        }
    }
    Ok(output)
}
