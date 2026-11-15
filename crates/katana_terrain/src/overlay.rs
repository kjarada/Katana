//! The exact overlay of two surfaces (docs/volumes.md, "One overlay").
//!
//! Every volume between two surfaces integrates their difference, and the
//! difference is linear only where both surfaces are: inside one triangle of
//! each. The overlay cuts the ground the two have in common into such pieces.
//! A piece is the intersection of a triangle of each surface (both convex, so
//! the intersection is a convex polygon of at most six corners), fanned into
//! triangles from its first corner, which needs no corner that is not already
//! on its boundary. Each piece is handed out with both surfaces' heights at its
//! corners, read off the plane of the triangle it came from.
//!
//! It is what compare_surfaces integrates (volume), what the volume inside a
//! polygon integrates after a third cut by the polygon's own triangles, and what
//! the difference and envelope surfaces are made of. One implementation of the
//! cut, so every one of them is exact in the same way: up to floating-point
//! rounding, nothing sampled.
//!
//! THE ANSWER DOES NOT DEPEND ON THE THREAD COUNT. The base surface's triangles
//! are cut into blocks of a FIXED OVERLAY_BLOCK_TRIANGLES, and every piece of a
//! block is handed out in a fixed order: base triangle ascending, then the top
//! triangles it meets in ascending index, then (inside a region) the region's
//! triangles in ascending index, then the fan. A visitor that keeps one sum per
//! block and combines them in block order therefore gets the same bits on one
//! thread or sixteen (CLAUDE.md section 5), which is what compare_surfaces does.
//!
//! Both surfaces are only read, so an overlay may run on a background job.
//!
//! The C++ visitor is a class with a non-const `piece`, called from the pool's
//! threads for different blocks at once. Here it is a `Sync` trait whose
//! `piece` takes `&self`: a visitor keeps its per-block slots behind interior
//! mutability (a lock per slot, never contended, since one block's pieces
//! arrive on one thread).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Mutex;

use katana_core::error::make_error;
use katana_core::task_pool::TaskPool;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::{Containment, Polyline2, SpatialEntry, SpatialId, SpatialIndex, Triangle2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::summation::CompensatedSum;
use crate::tin_builder::{
    Breakline, CrossingBreaklinePolicy, DuplicatePointPolicy, TinBuildOptions, TinInput, build_tin,
};
use crate::tin_surface::{Box2, NO_TRIANGLE, NO_VERTEX, Point2, Point3, TinSurface, TinTriangle};

// ---- the region ------------------------------------------------------------------------

/// A plan area the overlay is cut to: a polygon less its holes, triangulated
/// once (a constrained Delaunay triangulation of its own vertices, so its
/// triangles tile it exactly and add no point of their own). The default is
/// no area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayRegion {
    ring: Polyline2,
    holes: Vec<Polyline2>,
    /// the triangulation, every vertex at 0
    plan: TinSurface,
    triangles: Vec<Triangle2>,
    area: f64,
    bounds: Box2,
}

impl OverlayRegion {
    /// Either orientation. InvalidGeometry for a ring of fewer than three
    /// distinct vertices, a ring that touches or crosses itself, or nothing
    /// left once the holes are taken out, naming which.
    pub fn from_polygon(ring: &Polyline2, holes: &[Polyline2]) -> Result<OverlayRegion> {
        // A ring drawn back to its start repeats its first vertex at its end:
        // a closing segment of no length, which the triangulator would refuse
        // as a degenerate ring. It is the same ring without it.
        let opened = |polygon: &Polyline2| -> Polyline2 {
            let mut polygon = polygon.clone();
            while polygon.vertices.len() > 1
                && polygon.vertices[0].distance_to(polygon.vertices[polygon.vertices.len() - 1])
                    <= tolerance::GEOMETRIC
            {
                polygon.vertices.pop();
            }
            polygon.closed = true;
            polygon
        };
        let mut region = OverlayRegion {
            ring: opened(ring),
            holes: holes.iter().map(opened).collect(),
            ..OverlayRegion::default()
        };

        // The polygon's own vertices are the only points, so the triangulation
        // adds none and its triangles tile the polygon exactly. Heights do not
        // matter here; 0 for all, so a point given twice merges without a
        // conflict.
        let mut input = TinInput::default();
        for vertex in &region.ring.vertices {
            input.points.push(Point3::new(vertex.x, vertex.y, 0.0));
        }
        for hole in &region.holes {
            for vertex in &hole.vertices {
                input.points.push(Point3::new(vertex.x, vertex.y, 0.0));
            }
        }
        input.boundary = region.ring.clone();
        input.holes = region.holes.clone();
        let built = build_tin(&input, &TinBuildOptions::default()).map_err(|error| {
            make_error(
                ErrorCode::InvalidGeometry,
                format!("the polygon cannot be used as an area: {}", error.message),
                error.context,
            )
        })?;
        region.plan = built.surface;
        let mut area = CompensatedSum::new();
        for t in 0..region.plan.triangle_count() {
            let triangle = region.plan.plan_triangle(t);
            if !(triangle.signed_area() > 0.0) {
                continue; // a sliver the exact triangulator kept; it covers nothing
            }
            region.triangles.push(triangle);
            area.add(triangle.signed_area());
            region.bounds.expand_box(triangle.bounding_box());
        }
        region.area = area.value();
        if region.triangles.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the polygon has no area",
            ));
        }
        Ok(region)
    }

    /// The polygon's triangles, counter-clockwise, tiling it.
    pub fn triangles(&self) -> &[Triangle2] {
        &self.triangles
    }

    /// Their plan area (compensated): the polygon's area less its holes'.
    pub fn area(&self) -> f64 {
        self.area
    }

    pub fn bounds(&self) -> &Box2 {
        &self.bounds
    }

    pub fn empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// Inside the ring and outside every hole; the boundary counts as inside.
    pub fn contains(&self, position: Point2) -> bool {
        if self.triangles.is_empty() || !self.ring.contains(position) {
            return false;
        }
        !self
            .holes
            .iter()
            .any(|hole| hole.classify(position) == Containment::Inside)
    }

    /// The polygon as a flat surface at height `level`: the same triangles,
    /// every vertex at that height. What a volume to a level inside a polygon
    /// compares the ground with.
    pub fn at_level(&self, level: f64) -> Result<TinSurface> {
        if !level.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the level must be finite",
            ));
        }
        let mut vertices = self.plan.vertices().clone();
        for vertex in &mut vertices {
            vertex.z = level;
        }
        TinSurface::create(
            vertices,
            self.plan.triangles().clone(),
            self.plan.constrained_edges().clone(),
        )
    }
}

/// One piece of the overlay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayPiece {
    /// counter-clockwise
    pub corners: [Point2; 3],
    /// its plan area, always > 0
    pub area: f64,
    /// the base surface at each corner
    pub z_base: [f64; 3],
    /// the top surface at each corner
    pub z_top: [f64; 3],
    pub base_triangle: u32,
    pub top_triangle: u32,
}

impl Default for OverlayPiece {
    fn default() -> Self {
        Self {
            corners: [Point2::default(); 3],
            area: 0.0,
            z_base: [0.0; 3],
            z_top: [0.0; 3],
            base_triangle: NO_TRIANGLE,
            top_triangle: NO_TRIANGLE,
        }
    }
}

/// Told every piece. `block` is the block the piece belongs to; the pieces of
/// one block arrive on one thread in their fixed order, and different blocks
/// may arrive at once on different threads, so a visitor writes only into its
/// own slot for `block`.
pub trait OverlayVisitor: Sync {
    fn piece(&self, block: usize, piece: &OverlayPiece);
}

/// Base triangles to a block. Fixed: a partition that followed the thread
/// count would change the order of the sums with it. 256 triangles is a few
/// hundred microseconds of clipping, so dispatch is noise, and a 200 000
/// triangle surface still makes about 800 blocks to balance (docs/terrain.md,
/// "compareSurfaces and contours on the TaskPool").
pub const OVERLAY_BLOCK_TRIANGLES: usize = 256;

/// The blocks visit_overlay will number, from 0: one per
/// OVERLAY_BLOCK_TRIANGLES of `base`, the last one short.
pub fn overlay_block_count(base: &TinSurface) -> usize {
    base.triangle_count().div_ceil(OVERLAY_BLOCK_TRIANGLES)
}

// ---- the cut ---------------------------------------------------------------------------

/// Two buffers of twelve: a triangle clipped by a triangle has at most six
/// corners, and three more clip edges (a region's triangle) add at most one
/// each.
const MAX_CORNERS: usize = 12;

// Which surface's triangulation a line of a piece lies on.
const BASE_FAMILY: u8 = 0;
const TOP_FAMILY: u8 = 1;
const REGION_FAMILY: u8 = 2;

/// An edge of one family's triangulation, by its two vertex indices, lower
/// first. Ordered by family, then i, then j, as the C++'s defaulted `<=>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Line {
    family: u8,
    i: u32,
    j: u32,
}

fn line_of(family: u8, a: u32, b: u32) -> Line {
    Line {
        family,
        i: a.min(b),
        j: a.max(b),
    }
}

/// A corner of the overlay named by WHAT it is, so two pieces cutting it from
/// different triangles name it alike: a vertex of one family (`crossing`
/// false; `a.family`, `a.i`), or the point where line `a` crosses line `b`
/// (a < b). The C++ hashes it with FNV-1a; a hash only spreads the keys and
/// the map's order is never read, so the derived hash serves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Key {
    crossing: bool,
    a: Line,
    b: Line,
}

fn vertex_key(family: u8, index: u32) -> Key {
    Key {
        crossing: false,
        a: Line {
            family,
            i: index,
            j: index,
        },
        b: Line::default(),
    }
}

/// A convex polygon being clipped: its corners and, when the corners are named
/// (the labelled cut), what each is and the line each edge (corner k to k + 1)
/// lies on. The unlabelled cut leaves `key` and `edge` unread.
#[derive(Clone, Copy, Debug, Default)]
struct Poly {
    p: [Point2; MAX_CORNERS],
    n: usize,
    key: [Key; MAX_CORNERS],
    edge: [Line; MAX_CORNERS],
}

/// A clip edge, directed so that the inside is on its left.
#[derive(Clone, Copy, Debug, Default)]
struct ClipEdge {
    from: Point2,
    to: Point2,
    line: Line,
}

/// Where the families' vertices are, to place a crossing from its two lines.
#[derive(Clone, Copy)]
struct Vertices<'a> {
    base: &'a TinSurface,
    top: &'a TinSurface,
}

impl Vertices<'_> {
    fn at(&self, family: u8, index: u32) -> Point2 {
        let surface = if family == BASE_FAMILY {
            self.base
        } else {
            self.top
        };
        let v = surface.vertices()[index as usize];
        Point2::new(v.x, v.y)
    }
}

/// The crossing of polygon edge `edge` with clip line `line`, named and placed
/// the same way whichever piece finds it. `interpolated` is the point the
/// clipper computed along the polygon's edge, kept when the lines are too
/// nearly parallel to place it any other way.
///
/// Two edges of one triangulation meet only at a shared vertex, which is then
/// the corner, exactly. Otherwise the lines are put in a fixed order and
/// intersected from their own end points (never from the corners of the
/// polygon being clipped, which differ from piece to piece), so the pieces on
/// either side of an edge compute the same bits for the same crossing.
fn name_crossing(
    edge: Line,
    line: Line,
    vertices: &Vertices<'_>,
    interpolated: Point2,
) -> (Key, Point2) {
    if edge.family == line.family {
        let mut shared = NO_VERTEX;
        if edge.i == line.i || edge.i == line.j {
            shared = edge.i;
        } else if edge.j == line.i || edge.j == line.j {
            shared = edge.j;
        }
        if shared != NO_VERTEX {
            return (
                vertex_key(edge.family, shared),
                vertices.at(edge.family, shared),
            );
        }
    }
    let (first, second) = if edge < line {
        (edge, line)
    } else {
        (line, edge)
    };
    let key = Key {
        crossing: true,
        a: first,
        b: second,
    };
    let a = vertices.at(first.family, first.i);
    let b = vertices.at(first.family, first.j);
    let c = vertices.at(second.family, second.i);
    let d = vertices.at(second.family, second.j);
    let rx = b.x - a.x;
    let ry = b.y - a.y;
    let sx = d.x - c.x;
    let sy = d.y - c.y;
    let denominator = rx * sy - ry * sx;
    let t = ((c.x - a.x) * sy - (c.y - a.y) * sx) / denominator;
    if denominator != 0.0 && t.is_finite() {
        (key, Point2::new(a.x + t * rx, a.y + t * ry))
    } else {
        (key, interpolated)
    }
}

/// One Sutherland-Hodgman step: `input` clipped to the left of `clip`, into
/// `out`.
///
/// The inside test is `side >= 0`, not `> 0`, and that is load-bearing. A
/// corner lying exactly on a clip edge (the normal case where two triangles
/// share an edge or a vertex) has a cross product of exactly zero in IEEE
/// arithmetic, because it reduces to `a - a` for a point that defines the
/// edge. Counting it inside keeps it bit for bit instead of recomputing it, so
/// two triangles sharing an edge clip to exactly that segment, whose fan has
/// signed area exactly 0.0 and is rejected, and a surface compared with
/// itself is exactly zero (docs/terrain.md, "The fix").
fn clip_step<const LABELS: bool>(
    input: &Poly,
    clip: &ClipEdge,
    vertices: &Vertices<'_>,
    out: &mut Poly,
) {
    let ex = clip.to.x - clip.from.x;
    let ey = clip.to.y - clip.from.y;
    let side = |v: Point2| ex * (v.y - clip.from.y) - ey * (v.x - clip.from.x);
    out.n = 0;
    for i in 0..input.n {
        if out.n >= MAX_CORNERS {
            break;
        }
        let current = input.p[i];
        let next = input.p[(i + 1) % input.n];
        let side_current = side(current);
        let side_next = side(next);
        if side_current >= 0.0 {
            out.p[out.n] = current;
            if LABELS {
                out.key[out.n] = input.key[i];
                // A corner on the clip line whose next corner is cut away runs
                // along the clip line to the next corner kept.
                out.edge[out.n] = if side_current == 0.0 && side_next < 0.0 {
                    clip.line
                } else {
                    input.edge[i]
                };
            }
            out.n += 1;
        }
        // A crossing only when the two strictly straddle the line: an end
        // exactly on it has been kept (or will be), and a point constructed
        // here would duplicate it.
        if out.n < MAX_CORNERS
            && ((side_current > 0.0 && side_next < 0.0) || (side_current < 0.0 && side_next > 0.0))
        {
            let t = side_current / (side_current - side_next);
            let interpolated = Point2::new(
                current.x + t * (next.x - current.x),
                current.y + t * (next.y - current.y),
            );
            if LABELS {
                let (key, point) = name_crossing(input.edge[i], clip.line, vertices, interpolated);
                out.key[out.n] = key;
                out.p[out.n] = point;
                // Leaving: the edge from here runs along the clip line.
                // Entering: along the polygon's edge to `next`.
                out.edge[out.n] = if side_current > 0.0 {
                    clip.line
                } else {
                    input.edge[i]
                };
            } else {
                out.p[out.n] = interpolated;
            }
            out.n += 1;
        }
    }
}

/// `subject` clipped by the three edges of a counter-clockwise triangle, into
/// `out`; fewer than three corners left means they meet in a segment at most.
/// The three steps alternate between the two buffers and end in `out`:
/// subject -> out -> scratch -> out.
fn clip_by_triangle<const LABELS: bool>(
    subject: &Poly,
    window: &[ClipEdge; 3],
    vertices: &Vertices<'_>,
    scratch: &mut Poly,
    out: &mut Poly,
) {
    clip_step::<LABELS>(subject, &window[0], vertices, out);
    if out.n < 3 {
        out.n = 0;
        return;
    }
    clip_step::<LABELS>(out, &window[1], vertices, scratch);
    if scratch.n < 3 {
        out.n = 0;
        return;
    }
    clip_step::<LABELS>(scratch, &window[2], vertices, out);
    if out.n < 3 {
        out.n = 0;
    }
}

/// The triangle's corners counter-clockwise, with their vertex indices. The
/// clipper needs a known winding to know which side of an edge is inside, and
/// a stored triangle's winding is whatever made it.
#[derive(Clone, Copy, Debug)]
struct OrientedTriangle {
    corners: [Point2; 3],
    vertices: [u32; 3],
}

fn oriented(surface: &TinSurface, triangle: usize) -> OrientedTriangle {
    let tri = surface.triangles()[triangle];
    let mut out = OrientedTriangle {
        corners: tri.map(|v| {
            let p = surface.vertices()[v as usize];
            Point2::new(p.x, p.y)
        }),
        vertices: tri,
    };
    if Triangle2::new(out.corners[0], out.corners[1], out.corners[2]).signed_area() < 0.0 {
        out.corners.swap(1, 2);
        out.vertices.swap(1, 2);
    }
    out
}

fn window_of(triangle: &OrientedTriangle, family: u8) -> [ClipEdge; 3] {
    std::array::from_fn(|k| {
        let next = (k + 1) % 3;
        ClipEdge {
            from: triangle.corners[k],
            to: triangle.corners[next],
            line: line_of(family, triangle.vertices[k], triangle.vertices[next]),
        }
    })
}

fn window_of_region(triangle: &Triangle2) -> [ClipEdge; 3] {
    let corners = [triangle.a, triangle.b, triangle.c];
    // A region's edges never name a crossing (the region is cut only on the
    // unlabelled path), so their line is left blank.
    std::array::from_fn(|k| ClipEdge {
        from: corners[k],
        to: corners[(k + 1) % 3],
        line: Line {
            family: REGION_FAMILY,
            i: 0,
            j: 0,
        },
    })
}

/// The plane of `triangle` at three positions inside it. The weights are exact
/// at the triangle's own vertices (1, 0, 0), which is what makes the
/// comparison of identical surfaces exactly zero. The plan triangle and its
/// doubled area are the caller's loop invariants (Triangle2::barycentric).
fn plane_elevations(
    surface: &TinSurface,
    triangle: u32,
    plan: &Triangle2,
    twice_area: f64,
    positions: [Point2; 3],
) -> [f64; 3] {
    let tri = surface.triangles()[triangle as usize];
    let vertices = surface.vertices();
    positions.map(|position| {
        let weights = plan.barycentric_with_area(position, twice_area);
        weights[0] * vertices[tri[0] as usize].z
            + weights[1] * vertices[tri[1] as usize].z
            + weights[2] * vertices[tri[2] as usize].z
    })
}

fn box_of(points: &[Point2; MAX_CORNERS], count: usize) -> Box2 {
    let mut bounds = Box2::default();
    for &point in &points[..count] {
        bounds.expand(point);
    }
    bounds
}

/// The pool a call runs on: the one given, or the shared one.
fn workers_of(pool: Option<&TaskPool>) -> &TaskPool {
    match pool {
        Some(pool) => pool,
        None => TaskPool::shared(),
    }
}

/// Every piece, block by block, handed to `emit(block, piece, keys)`; the keys
/// are meaningful only when LABELS. The one loop both the visitor and the
/// surface builders run.
fn cut_overlay<E, const LABELS: bool>(
    base: &TinSurface,
    top: &TinSurface,
    region: Option<&OverlayRegion>,
    pool: Option<&TaskPool>,
    emit: &E,
) where
    E: Fn(usize, &OverlayPiece, &[Key; 3]) + Sync,
{
    if !base.bounds().intersects(*top.bounds()) {
        return;
    }
    let mut common = Box2::new(
        Point2::new(
            cpp::max(base.bounds().min.x, top.bounds().min.x),
            cpp::max(base.bounds().min.y, top.bounds().min.y),
        ),
        Point2::new(
            cpp::min(base.bounds().max.x, top.bounds().max.x),
            cpp::min(base.bounds().max.y, top.bounds().max.y),
        ),
    );
    if let Some(region) = region {
        if region.empty() || !common.intersects(*region.bounds()) {
            return;
        }
        common = Box2::new(
            Point2::new(
                cpp::max(common.min.x, region.bounds().min.x),
                cpp::max(common.min.y, region.bounds().min.y),
            ),
            Point2::new(
                cpp::min(common.max.x, region.bounds().max.x),
                cpp::min(common.max.y, region.bounds().max.y),
            ),
        );
    }
    let area = common.inflated(tolerance::GEOMETRIC);

    // Broad phase over the top triangles that reach the common area: only
    // these can contribute, and only they are worth indexing.
    let mut entries: Vec<SpatialEntry> = Vec::with_capacity(top.triangle_count());
    for t in 0..top.triangle_count() {
        let bounds = top.plan_triangle(t).bounding_box();
        if bounds.intersects(area) {
            entries.push(SpatialEntry {
                id: t as SpatialId,
                box_value: bounds,
            });
        }
    }
    if entries.is_empty() {
        return;
    }
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);
    let mut region_index = SpatialIndex::default();
    if let Some(region) = region {
        let region_entries: Vec<SpatialEntry> = region
            .triangles()
            .iter()
            .enumerate()
            .map(|(t, triangle)| SpatialEntry {
                id: t as SpatialId,
                box_value: triangle.bounding_box(),
            })
            .collect();
        region_index.rebuild(&region_entries);
    }

    let vertices = Vertices { base, top };
    let base_count = base.triangle_count();
    let block_count = overlay_block_count(base);

    let cut_blocks = |first_block: usize, end_block: usize| {
        // Per call, not shared: the queries and the clipper each need a
        // buffer, and a chunk of blocks runs on one thread.
        let mut candidates: Vec<SpatialId> = Vec::new();
        let mut region_candidates: Vec<SpatialId> = Vec::new();
        let mut subject = Poly::default();
        let mut scratch = Poly::default();
        let mut clipped = Poly::default();
        let mut inside = Poly::default();
        for block in first_block..end_block {
            let first = block * OVERLAY_BLOCK_TRIANGLES;
            let end = (first + OVERLAY_BLOCK_TRIANGLES).min(base_count);
            for tb in first..end {
                let plan_base = base.plan_triangle(tb);
                let box_base = plan_base.bounding_box();
                if !box_base.intersects(area) {
                    continue;
                }
                // A degenerate triangle has no plane to read a height off, so
                // every piece cut from it would be dropped one at a time.
                if plan_base.is_degenerate() {
                    continue;
                }
                let twice_area_base = plan_base.twice_signed_area();
                let oriented_base = oriented(base, tb);
                subject.n = 3;
                for k in 0..3 {
                    subject.p[k] = oriented_base.corners[k];
                    if LABELS {
                        subject.key[k] = vertex_key(BASE_FAMILY, oriented_base.vertices[k]);
                        subject.edge[k] = line_of(
                            BASE_FAMILY,
                            oriented_base.vertices[k],
                            oriented_base.vertices[(k + 1) % 3],
                        );
                    }
                }
                index.query_into(box_base, &mut candidates);
                for &id in &candidates {
                    let tt = id as u32;
                    let plan_top = top.plan_triangle(tt as usize);
                    if plan_top.is_degenerate() {
                        continue;
                    }
                    let twice_area_top = plan_top.twice_signed_area();
                    clip_by_triangle::<LABELS>(
                        &subject,
                        &window_of(&oriented(top, tt as usize), TOP_FAMILY),
                        &vertices,
                        &mut scratch,
                        &mut clipped,
                    );
                    if clipped.n < 3 {
                        continue;
                    }
                    // A convex polygon, so a fan from its first corner covers
                    // it exactly once with no corner not already on it.
                    let fan = |polygon: &Poly| {
                        for i in 1..polygon.n.saturating_sub(1) {
                            let corners = [polygon.p[0], polygon.p[i], polygon.p[i + 1]];
                            let piece_area =
                                Triangle2::new(corners[0], corners[1], corners[2]).signed_area();
                            if !(piece_area > 0.0) {
                                continue;
                            }
                            // Inside both triangles by construction, so both
                            // planes are read with weights in [0, 1] and
                            // neither surface is searched.
                            let piece = OverlayPiece {
                                corners,
                                area: piece_area,
                                z_base: plane_elevations(
                                    base,
                                    tb as u32,
                                    &plan_base,
                                    twice_area_base,
                                    corners,
                                ),
                                z_top: plane_elevations(
                                    top,
                                    tt,
                                    &plan_top,
                                    twice_area_top,
                                    corners,
                                ),
                                base_triangle: tb as u32,
                                top_triangle: tt,
                            };
                            let keys = if LABELS {
                                [polygon.key[0], polygon.key[i], polygon.key[i + 1]]
                            } else {
                                [Key::default(); 3]
                            };
                            emit(block, &piece, &keys);
                        }
                    };
                    let Some(region) = region else {
                        fan(&clipped);
                        continue;
                    };
                    region_index.query_into(box_of(&clipped.p, clipped.n), &mut region_candidates);
                    for &r in &region_candidates {
                        clip_by_triangle::<LABELS>(
                            &clipped,
                            &window_of_region(&region.triangles()[r as usize]),
                            &vertices,
                            &mut scratch,
                            &mut inside,
                        );
                        if inside.n >= 3 {
                            fan(&inside);
                        }
                    }
                }
            }
        }
    };
    // Everything the blocks read is immutable here, and each block's pieces
    // go only to that block's slot.
    workers_of(pool).parallel_ranges(0, block_count, 1, cut_blocks);
}

fn check_surfaces(base: &TinSurface, top: &TinSurface) -> Status {
    if base.empty() || top.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "both surfaces must have triangles",
        ));
    }
    Ok(())
}

/// Every piece of the overlay of `base` and `top` (inside `region` when one is
/// given) handed to `visitor`. Surfaces that do not overlap (or do not reach
/// the region) visit nothing and succeed. InvalidArgument for an empty
/// surface. Runs on `pool` (None: TaskPool::shared()).
pub fn visit_overlay(
    base: &TinSurface,
    top: &TinSurface,
    visitor: &dyn OverlayVisitor,
    region: Option<&OverlayRegion>,
    pool: Option<&TaskPool>,
) -> Status {
    check_surfaces(base, top)?;
    let emit = |block: usize, piece: &OverlayPiece, _keys: &[Key; 3]| visitor.piece(block, piece);
    cut_overlay::<_, false>(base, top, region, pool, &emit);
    Ok(())
}

// ---- surfaces made of the overlay --------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct OverlaySurfaceOptions<'a> {
    /// Build through the constrained triangulation even when the pieces join
    /// as they are. The pieces are joined directly when they can be (every
    /// corner named by what it is, a vertex of one surface or where an edge of
    /// one crosses an edge of the other, so two pieces that share a corner
    /// share its vertex), and the triangulation is what the build falls back
    /// to when floating point leaves two pieces overlapping by a sliver.
    /// Slower; the same surface up to rounding.
    pub retriangulate: bool,
    pub pool: Option<&'a TaskPool>,
}

impl fmt::Debug for OverlaySurfaceOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OverlaySurfaceOptions")
            .field("retriangulate", &self.retriangulate)
            .field("pool_concurrency", &self.pool.map(TaskPool::concurrency))
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlaySurface {
    pub surface: TinSurface,
    /// overlay pieces it was made from
    pub pieces: usize,
    /// made through the constrained triangulation
    pub retriangulated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Envelope {
    /// the higher of the two at every point
    Upper,
    /// the lower
    Lower,
}

#[derive(Clone, Copy, Debug)]
struct NamedPiece {
    piece: OverlayPiece,
    keys: [Key; 3],
}

/// The overlay's pieces joined into one mesh: a vertex per distinct corner, in
/// the order the pieces first name it (block, then piece order: the same at
/// any thread count), with both surfaces' heights there as that first piece
/// read them.
#[derive(Debug, Default)]
struct OverlayMesh {
    positions: Vec<Point2>,
    z_base: Vec<f64>,
    z_top: Vec<f64>,
    triangles: Vec<TinTriangle>,
    pieces: usize,
}

fn join_pieces(base: &TinSurface, top: &TinSurface, pool: Option<&TaskPool>) -> OverlayMesh {
    let blocks: Vec<Mutex<Vec<NamedPiece>>> = (0..overlay_block_count(base))
        .map(|_| Mutex::new(Vec::new()))
        .collect();
    let emit = |block: usize, piece: &OverlayPiece, keys: &[Key; 3]| {
        blocks[block]
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(NamedPiece {
                piece: *piece,
                keys: *keys,
            });
    };
    cut_overlay::<_, true>(base, top, None, pool, &emit);

    let mut mesh = OverlayMesh::default();
    // Only looked up, never iterated: its order is not observable.
    let mut by_key: HashMap<Key, u32> = HashMap::new();
    for block in blocks {
        let block = block
            .into_inner()
            .unwrap_or_else(|error| error.into_inner());
        for named in &block {
            let mut triangle: TinTriangle = [0; 3];
            for (c, slot) in triangle.iter_mut().enumerate() {
                let next = mesh.positions.len() as u32;
                let found = *by_key.entry(named.keys[c]).or_insert_with(|| {
                    mesh.positions.push(named.piece.corners[c]);
                    mesh.z_base.push(named.piece.z_base[c]);
                    mesh.z_top.push(named.piece.z_top[c]);
                    next
                });
                *slot = found;
            }
            mesh.triangles.push(triangle);
            mesh.pieces += 1;
        }
    }

    // Two names for one place (a vertex of the base where the top has one
    // too, which is common where a design was built on the ground's own
    // points) are one vertex: the first.
    let positions = &mesh.positions;
    let mut order: Vec<u32> = (0..positions.len() as u32).collect();
    order.sort_by(|&a, &b| {
        let pa = positions[a as usize];
        let pb = positions[b as usize];
        if pa.x != pb.x {
            return if pa.x < pb.x {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        if pa.y != pb.y {
            return if pa.y < pb.y {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.cmp(&b)
    });
    let mut same: Vec<u32> = vec![0; positions.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        let first = positions[order[i] as usize];
        while j < order.len()
            && positions[order[j] as usize].x == first.x
            && positions[order[j] as usize].y == first.y
        {
            same[order[j] as usize] = order[i]; // the lowest index of the run, by the sort
            j += 1;
        }
        i = j;
    }
    for triangle in &mut mesh.triangles {
        for v in triangle.iter_mut() {
            *v = same[*v as usize];
        }
    }
    mesh
}

/// The vertices `triangles` use, renumbered densely in order of first use, and
/// the triangles that still have three distinct corners and an area.
#[derive(Debug, Default)]
struct CompactMesh {
    vertices: Vec<Point3>,
    triangles: Vec<TinTriangle>,
}

fn compact(vertices: &[Point3], triangles: &[TinTriangle]) -> CompactMesh {
    let mut mesh = CompactMesh::default();
    let mut renumber: Vec<u32> = vec![NO_VERTEX; vertices.len()];
    for triangle in triangles {
        if triangle[0] == triangle[1] || triangle[1] == triangle[2] || triangle[0] == triangle[2] {
            continue;
        }
        let plan = |c: usize| {
            let v = vertices[triangle[c] as usize];
            Point2::new(v.x, v.y)
        };
        if !(Triangle2::new(plan(0), plan(1), plan(2)).twice_signed_area() > 0.0) {
            continue;
        }
        let mut out: TinTriangle = [0; 3];
        for c in 0..3 {
            let to = &mut renumber[triangle[c] as usize];
            if *to == NO_VERTEX {
                *to = mesh.vertices.len() as u32;
                mesh.vertices.push(vertices[triangle[c] as usize]);
            }
            out[c] = *to;
        }
        mesh.triangles.push(out);
    }
    mesh
}

fn no_ground_in_common() -> Error {
    Error::new(
        ErrorCode::InvalidGeometry,
        "the surfaces have no ground in common",
    )
}

fn finish_surface(
    positions: &[Point2],
    heights: &[f64],
    triangles: &[TinTriangle],
    pieces: usize,
    retriangulate: bool,
) -> Result<OverlaySurface> {
    let all: Vec<Point3> = positions
        .iter()
        .zip(heights)
        .map(|(position, &height)| Point3::new(position.x, position.y, height))
        .collect();
    let joined = compact(&all, triangles);
    let vertices = &joined.vertices;
    let kept = &joined.triangles;
    if kept.is_empty() {
        return Err(no_ground_in_common());
    }

    let mut result = OverlaySurface {
        pieces,
        ..OverlaySurface::default()
    };
    if !retriangulate
        && let Ok(direct) = TinSurface::create(vertices.clone(), kept.clone(), Vec::new())
    {
        result.surface = direct;
        return Ok(result);
    }

    // The fallback: the same vertices and every edge of every piece given to
    // the constrained triangulation, whose exact predicates settle a sliver
    // that floating point left two pieces sharing. Every edge is a breakline,
    // so every triangle it makes lies inside one piece, where the height is
    // linear; it triangulates the hull, so the triangles outside every piece
    // are left out again.
    let mut input = TinInput {
        points: vertices.clone(),
        ..TinInput::default()
    };
    let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for triangle in kept {
        for k in 0..3 {
            let a = triangle[k];
            let b = triangle[(k + 1) % 3];
            edges.insert((a.min(b), a.max(b)));
        }
    }
    for &(from, to) in &edges {
        input.breaklines.push(Breakline {
            vertices: vec![vertices[from as usize], vertices[to as usize]],
            closed: false,
        });
    }
    let options = TinBuildOptions {
        duplicate_points: DuplicatePointPolicy::Average,
        crossing_breaklines: CrossingBreaklinePolicy::Average,
        ..TinBuildOptions::default()
    };
    let built = build_tin(&input, &options)?;
    let hull = &built.surface;
    let plan_of = |piece: &TinTriangle| {
        let corner = |c: usize| {
            let v = vertices[piece[c] as usize];
            Point2::new(v.x, v.y)
        };
        Triangle2::new(corner(0), corner(1), corner(2))
    };
    let entries: Vec<SpatialEntry> = kept
        .iter()
        .enumerate()
        .map(|(t, piece)| SpatialEntry {
            id: t as SpatialId,
            box_value: plan_of(piece).bounding_box(),
        })
        .collect();
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);
    let mut candidates: Vec<SpatialId> = Vec::new();
    let mut inside: Vec<TinTriangle> = Vec::new();
    for t in 0..hull.triangle_count() {
        let centroid = hull.plan_triangle(t).centroid();
        index.query_into(Box2::new(centroid, centroid), &mut candidates);
        let is_covered = candidates
            .iter()
            .any(|&id| plan_of(&kept[id as usize]).contains(centroid));
        if is_covered {
            inside.push(hull.triangles()[t]);
        }
    }
    let covered = compact(hull.vertices(), &inside);
    if covered.triangles.is_empty() {
        return Err(no_ground_in_common());
    }
    result.surface = TinSurface::create(covered.vertices, covered.triangles, Vec::new())?;
    result.retriangulated = true;
    Ok(result)
}

/// The difference surface (an isopach): `top` minus `base`, over the ground the
/// two have in common, exact: linear on every overlay piece, which is what a
/// TIN is. Positive where the top is above the base (fill on a design over the
/// ground), negative where it is below (cut). Integrated with volume_to_datum
/// at 0, its above and below are compare_surfaces' fill and cut.
///
/// InvalidArgument for an empty surface; InvalidGeometry when the two have no
/// ground in common; what TinSurface::create or build_tin reports.
pub fn difference_surface(
    base: &TinSurface,
    top: &TinSurface,
    options: &OverlaySurfaceOptions<'_>,
) -> Result<OverlaySurface> {
    check_surfaces(base, top)?;
    let mesh = join_pieces(base, top, options.pool);
    if mesh.triangles.is_empty() {
        return Err(no_ground_in_common());
    }
    let heights: Vec<f64> = mesh
        .z_top
        .iter()
        .zip(&mesh.z_base)
        .map(|(&z_top, &z_base)| z_top - z_base)
        .collect();
    finish_surface(
        &mesh.positions,
        &heights,
        &mesh.triangles,
        mesh.pieces,
        options.retriangulate,
    )
}

/// The envelope's mesh while the pieces on which the two cross are split.
struct Meetings<'a> {
    positions: Vec<Point2>,
    heights: Vec<f64>,
    difference: Vec<f64>,
    z_top: &'a [f64],
    /// one vertex per edge, by its two ends, lower first
    meetings: BTreeMap<(u32, u32), u32>,
}

impl Meetings<'_> {
    /// Where the two meet on an edge whose ends lie on opposite sides: one
    /// vertex per edge, placed from the edge's ends in a fixed order (the lower
    /// index first), so both pieces that share the edge share the point.
    fn meet(&mut self, u: u32, v: u32) -> u32 {
        if self.difference[u as usize] == 0.0 {
            return u;
        }
        if self.difference[v as usize] == 0.0 {
            return v;
        }
        let low = u.min(v);
        let high = u.max(v);
        if let Some(&found) = self.meetings.get(&(low, high)) {
            return found;
        }
        let (l, h) = (low as usize, high as usize);
        let t = self.difference[l] / (self.difference[l] - self.difference[h]);
        let a = self.positions[l];
        let b = self.positions[h];
        self.positions
            .push(Point2::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y)));
        // Both surfaces have this height there; the second's is read.
        self.heights
            .push(self.z_top[l] + t * (self.z_top[h] - self.z_top[l]));
        self.difference.push(0.0);
        let index = (self.positions.len() - 1) as u32;
        self.meetings.insert((low, high), index);
        index
    }
}

/// The upper or lower envelope of two surfaces over the ground they have in
/// common, exact: a piece on which the two cross is split along the line where
/// they meet, so each part is one surface's plane. Fails as
/// difference_surface.
pub fn envelope_surface(
    first: &TinSurface,
    second: &TinSurface,
    which: Envelope,
    options: &OverlaySurfaceOptions<'_>,
) -> Result<OverlaySurface> {
    check_surfaces(first, second)?;
    let mesh = join_pieces(first, second, options.pool);
    if mesh.triangles.is_empty() {
        return Err(no_ground_in_common());
    }
    let mut heights: Vec<f64> = Vec::with_capacity(mesh.positions.len());
    let mut difference: Vec<f64> = Vec::with_capacity(mesh.positions.len());
    for (&z_base, &z_top) in mesh.z_base.iter().zip(&mesh.z_top) {
        difference.push(z_top - z_base);
        heights.push(match which {
            Envelope::Upper => cpp::max(z_base, z_top),
            Envelope::Lower => cpp::min(z_base, z_top),
        });
    }
    let mut state = Meetings {
        positions: mesh.positions.clone(),
        heights,
        difference,
        z_top: &mesh.z_top,
        meetings: BTreeMap::new(),
    };

    let mut triangles: Vec<TinTriangle> = Vec::with_capacity(mesh.triangles.len());
    for &triangle in &mesh.triangles {
        let d = triangle.map(|v| state.difference[v as usize]);
        let positives = d.iter().filter(|&&value| value > 0.0).count();
        let negatives = d.iter().filter(|&&value| value < 0.0).count();
        if positives == 0 || negatives == 0 {
            triangles.push(triangle);
            continue;
        }
        // The two cross inside: the lone vertex is the only positive or the
        // only negative one. Rotating it to the front keeps the winding.
        let lone_positive = positives == 1;
        let mut p = 0;
        for (i, &value) in d.iter().enumerate() {
            let lone = if lone_positive {
                value > 0.0
            } else {
                value < 0.0
            };
            if lone {
                p = i;
            }
        }
        let vp = triangle[p];
        let vq = triangle[(p + 1) % 3];
        let vr = triangle[(p + 2) % 3];
        let mq = state.meet(vp, vq);
        let mr = state.meet(vp, vr);
        // The tip on the lone vertex's side, and the rest split in two; a part
        // that collapses (a corner exactly where the two meet) names a vertex
        // twice and is dropped when the surface is finished.
        triangles.push([vp, mq, mr]);
        triangles.push([mq, vq, vr]);
        triangles.push([mq, vr, mr]);
    }
    finish_surface(
        &state.positions,
        &state.heights,
        &triangles,
        mesh.pieces,
        options.retriangulate,
    )
}
