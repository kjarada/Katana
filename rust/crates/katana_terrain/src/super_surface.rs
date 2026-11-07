//! Combining surfaces in rank order.
//!
//! A super tin of the archive format (.12da) is a list of tins that behave as
//! one surface: a base survey tin with design pads laid over it, so that where
//! a pad has a level the pad's level is the answer and everywhere else the
//! base's is. The archive carries only the list and its order; the surface
//! itself is something the reader has to build.
//!
//! RANK ORDER: a LATER member overrides an earlier one wherever it can give a
//! height. The format's manual does not state which end wins - it says only
//! that the tins are listed - so this follows the order the sample archive
//! spells out in its own words ("Rank order: base surface first, then the pads
//! that substitute it over their footprints"). It is one comparison to reverse
//! if that turns out to be wrong, and the test that pins it says as much.

use katana_core::Result;
use katana_math::numerics::tolerance;

use crate::tin_surface::{
    Box2, Point2, Point3, TinSurface, TinTriangle, std_clamp, std_max, std_max3, std_min3,
};

/// Roughly two triangles per cell: the query walks whole cells, so fewer cells
/// means longer lists to scan and more means a build that costs more than the
/// scan it replaces. The surface's own locator sets the same kind of target for
/// the same reason.
const TRIANGLES_PER_CELL: f64 = 2.0;

/// A triangle of the plan, counter-clockwise.
type PlanTriangle = [Point2; 3];

/// How far `p` lies to the inside of the directed edge from -> to of a
/// counter-clockwise triangle: positive on its left, in model units.
fn inward(from: Point2, to: Point2, p: Point2) -> f64 {
    let length = (to.x - from.x).hypot(to.y - from.y);
    if !(length > 0.0) {
        return 0.0;
    }
    ((to.x - from.x) * (p.y - from.y) - (to.y - from.y) * (p.x - from.x)) / length
}

/// Whether a line along an edge of `edge_of` has all of `other` on its outer
/// side or within `margin` of it: a line that parts the two triangles to within
/// the margin, which is all a separating axis has to be.
fn separates_by_an_edge(edge_of: &PlanTriangle, other: &PlanTriangle, margin: f64) -> bool {
    for k in 0..3 {
        let from = edge_of[k];
        let to = edge_of[(k + 1) % 3];
        if inward(from, to, other[0]) <= margin
            && inward(from, to, other[1]) <= margin
            && inward(from, to, other[2]) <= margin
        {
            return true;
        }
    }
    false
}

/// Do two counter-clockwise triangles share an AREA - more than a point or an
/// edge? Convex shapes have no common interior exactly when one edge's line
/// parts them (the separating axis theorem; for triangles the edge lines are
/// all the axes there are), and "parts them" is read with the coincidence
/// rule: a vertex of the one within `margin` of an edge line of the other is
/// on the line, not beyond it. So triangles that abut - along an edge, at a
/// corner, or by a sliver thinner than GEOMETRIC that rounding of the
/// coordinates left - touch; they do not overlap.
fn share_area(t: &PlanTriangle, u: &PlanTriangle, margin: f64) -> bool {
    !separates_by_an_edge(t, u, margin) && !separates_by_an_edge(u, t, margin)
}

#[derive(Clone, Copy)]
struct Extent {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

/// Triangles of one member bucketed by plan extent in a uniform grid, in the
/// CSR layout the surface locator uses: two flat arrays rather than a vector
/// per cell.
///
/// WHY. overlaps() asks "does this higher member share area with this
/// triangle?", and answered it by scanning every vertex - for every triangle
/// of every lower-ranked member, against every higher-ranked member. That is
/// O(triangles x members x vertices): two 200 000-triangle members make about
/// 2e10 tests, and a super-TIN import reaches it with real data.
///
/// Why triangles rather than the vertices it used to bucket: whether two
/// triangles share an area is decided by the pair, exactly as far as the
/// coincidence rule goes. A vertex of the higher member inside the lower
/// triangle, or a corner of it inside the higher surface, finds most overlaps
/// and none of those that merely touch - and misses a strip crossing a
/// triangle whose vertices lie ON its edges, which share area all the same
/// and left two sheets over one point.
struct TriangleGrid<'a> {
    surface: &'a TinSurface,
    origin: Point2,
    cell_width: f64,
    cell_height: f64,
    columns: u32,
    rows: u32,
    /// columns * rows + 1 offsets
    cell_start: Vec<u32>,
    /// ascending within each cell
    cell_triangles: Vec<u32>,
    usable: bool,
}

impl<'a> TriangleGrid<'a> {
    fn new(surface: &'a TinSurface, build: bool) -> Self {
        let mut grid = Self {
            surface,
            origin: Point2::default(),
            cell_width: 1.0,
            cell_height: 1.0,
            columns: 1,
            rows: 1,
            cell_start: Vec::new(),
            cell_triangles: Vec::new(),
            usable: false,
        };
        if !build || surface.empty() {
            return grid;
        }
        let bounds: Box2 = *surface.bounds();
        if bounds.empty()
            || !bounds.min.x.is_finite()
            || !bounds.min.y.is_finite()
            || !bounds.max.x.is_finite()
            || !bounds.max.y.is_finite()
        {
            return grid; // unusable: nothing is reported as overlapping
        }
        grid.origin = bounds.min;
        let width = bounds.width();
        let height = bounds.height();
        let target_cells = std_max(1.0, surface.triangle_count() as f64 / TRIANGLES_PER_CELL);
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
        let mut cell_start: Vec<u32> = vec![0; cell_count + 1];
        for t in 0..surface.triangle_count() {
            let range = grid.cells_of(grid.extent_of(t));
            for row in range[2]..=range[3] {
                for column in range[0]..=range[1] {
                    cell_start[row as usize * grid.columns as usize + column as usize + 1] += 1;
                }
            }
        }
        for cell in 0..cell_count {
            cell_start[cell + 1] += cell_start[cell];
        }
        let mut cell_triangles: Vec<u32> = vec![0; cell_start[cell_count] as usize];
        let mut cursor: Vec<u32> = cell_start[..cell_count].to_vec();
        for t in 0..surface.triangle_count() {
            let range = grid.cells_of(grid.extent_of(t));
            for row in range[2]..=range[3] {
                for column in range[0]..=range[1] {
                    let slot = &mut cursor[row as usize * grid.columns as usize + column as usize];
                    cell_triangles[*slot as usize] = t as u32;
                    *slot += 1;
                }
            }
        }
        grid.cell_start = cell_start;
        grid.cell_triangles = cell_triangles;
        grid.usable = true;
        grid
    }

    /// True when `accept` says yes to any triangle in the cells the box
    /// touches (ascending within a cell). A superset of the box's own contents,
    /// never a subset, so an `accept` that only ever says yes inside the box
    /// gets the same answer it would from every triangle in the surface.
    fn any_in(&self, extent: Extent, accept: impl Fn(u32) -> bool) -> bool {
        let range = self.cells_of(extent);
        for row in range[2]..=range[3] {
            let row_base = row as usize * self.columns as usize;
            for column in range[0]..=range[1] {
                let cell = row_base + column as usize;
                for i in self.cell_start[cell]..self.cell_start[cell + 1] {
                    if accept(self.cell_triangles[i as usize]) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn extent_of(&self, t: usize) -> Extent {
        let tri: TinTriangle = self.surface.triangles()[t];
        let a = self.surface.vertices()[tri[0] as usize];
        let b = self.surface.vertices()[tri[1] as usize];
        let c = self.surface.vertices()[tri[2] as usize];
        Extent {
            min_x: std_min3(a.x, b.x, c.x),
            min_y: std_min3(a.y, b.y, c.y),
            max_x: std_max3(a.x, b.x, c.x),
            max_y: std_max3(a.y, b.y, c.y),
        }
    }

    fn cells_of(&self, e: Extent) -> [u32; 4] {
        [
            Self::coordinate(e.min_x, self.origin.x, self.cell_width, self.columns),
            Self::coordinate(e.max_x, self.origin.x, self.cell_width, self.columns),
            Self::coordinate(e.min_y, self.origin.y, self.cell_height, self.rows),
            Self::coordinate(e.max_y, self.origin.y, self.cell_height, self.rows),
        ]
    }

    fn coordinate(value: f64, origin: f64, cell_size: f64, count: u32) -> u32 {
        let cell = ((value - origin) / cell_size).floor();
        if !(cell > 0.0) {
            return 0; // also catches NaN, which belongs in the first cell rather than nowhere
        }
        if cell >= f64::from(count) {
            count - 1
        } else {
            cell as u32
        }
    }
}

/// Does `higher` cover any AREA of the triangle abc? Some triangle of it shares
/// an area with abc (share_area): touching along an edge or at a corner does not
/// cover. Exact to the coincidence rule, not sampled, so a strip that crosses
/// the triangle between its corners and its middle is found as surely as a pad
/// over its middle.
fn overlaps(higher: &TinSurface, grid: &TriangleGrid, a: Point3, b: Point3, c: Point3) -> bool {
    let bounds = higher.bounds();
    let min_x = std_min3(a.x, b.x, c.x);
    let max_x = std_max3(a.x, b.x, c.x);
    let min_y = std_min3(a.y, b.y, c.y);
    let max_y = std_max3(a.y, b.y, c.y);
    if !grid.usable
        || bounds.empty()
        || max_x < bounds.min.x
        || min_x > bounds.max.x
        || max_y < bounds.min.y
        || min_y > bounds.max.y
    {
        return false;
    }
    let lower: PlanTriangle = [
        Point2::new(a.x, a.y),
        Point2::new(b.x, b.y),
        Point2::new(c.x, c.y),
    ];
    let twice_area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if !(twice_area > 0.0) {
        return false; // a flat triangle covers nothing, and nothing covers it
    }

    // The inward distances are two products differenced, divided by a length:
    // their absolute error is a few dozen ulps of the coordinates, which
    // GEOMETRIC plus sixty-four ulps of them covers with room to spare (the
    // rounding margin tin_patch and the cull used before it derive).
    let scale = std_max(
        std_max(std_max(min_x.abs(), max_x.abs()), min_y.abs()),
        max_y.abs(),
    );
    let margin = tolerance::GEOMETRIC + 64.0 * f64::EPSILON * scale;
    let vertices = higher.vertices();
    let triangles = higher.triangles();
    let accepts = |t: u32| {
        let tri = triangles[t as usize];
        let p = vertices[tri[0] as usize];
        let q = vertices[tri[1] as usize];
        let r = vertices[tri[2] as usize];
        // Boxes that only touch share no area: the cheap rejection first.
        if std_min3(p.x, q.x, r.x) >= max_x - margin
            || std_max3(p.x, q.x, r.x) <= min_x + margin
            || std_min3(p.y, q.y, r.y) >= max_y - margin
            || std_max3(p.y, q.y, r.y) <= min_y + margin
        {
            return false;
        }
        if !((q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x) > 0.0) {
            return false; // flat, or turned over: it holds no area of its own
        }
        share_area(
            &lower,
            &[
                Point2::new(p.x, p.y),
                Point2::new(q.x, q.y),
                Point2::new(r.x, r.y),
            ],
            margin,
        )
    };
    grid.any_in(
        Extent {
            min_x: min_x - margin,
            min_y: min_y - margin,
            max_x: max_x + margin,
            max_y: max_y + margin,
        },
        accepts,
    )
}

/// One surface from several, later members winning. None members are
/// ignored; an empty list, or one with nothing in it that has a triangle,
/// gives an empty surface rather than an error, because a super tin whose
/// members the archive did not carry is a fact about the file and not a
/// failure of the reader.
///
/// A triangle of a lower-ranked member is kept or dropped WHOLE, by whether
/// a higher-ranked member covers any AREA of it. The seam is therefore ragged
/// to within one triangle of the pad's edge, which is stated rather than
/// hidden: clipping each triangle against the higher surface's boundary and
/// re-triangulating the offcuts would be exact, and would also mean re-running
/// a constrained triangulation for every overlap - paid on every import of
/// every archive, to move a seam by less than the triangle size of the data
/// that drew it. A higher member that only abuts a triangle - shares an edge
/// or a corner, or overlaps it by less than tolerance::GEOMETRIC - covers
/// none of it, and the triangle is kept: two tiles of one survey merge into
/// both, not into whichever was named last.
pub fn combine_surfaces(members_in_rank_order: &[Option<&TinSurface>]) -> Result<TinSurface> {
    let members: Vec<&TinSurface> = members_in_rank_order
        .iter()
        .flatten()
        .copied()
        .filter(|member| !member.empty())
        .collect();

    // One grid per member, built before the triangle loop rather than inside
    // it: every lower-ranked triangle queries the same higher-ranked member.
    // Rank 0 is never a HIGHER member of anything, so its grid would never
    // be queried - and it is usually the survey tin, the biggest of them.
    let grids: Vec<TriangleGrid> = members
        .iter()
        .enumerate()
        .map(|(rank, member)| TriangleGrid::new(member, rank != 0))
        .collect();

    let mut vertices: Vec<Point3> = Vec::new();
    let mut triangles: Vec<TinTriangle> = Vec::new();
    let mut constrained: Vec<u8> = Vec::new();
    for (rank, member) in members.iter().enumerate() {
        let base = vertices.len() as u32;
        vertices.extend_from_slice(member.vertices());

        for t in 0..member.triangle_count() {
            let triangle = member.triangles()[t];
            let a = member.vertices()[triangle[0] as usize];
            let b = member.vertices()[triangle[1] as usize];
            let c = member.vertices()[triangle[2] as usize];
            // Dropped if ANY higher-ranked member covers any AREA of this
            // ground - not merely if it covers the middle of it. Two sheets
            // over one point would make elevation_at() answer with whichever
            // triangle it met first, which is the base as often as the pad; a
            // triangle removed a little too eagerly leaves no answer there,
            // and no answer is what Katana already says off a surface. But
            // a member that only ABUTS - shares an edge or a corner, or a
            // sliver thinner than GEOMETRIC - covers none of it, and the
            // triangle stays: two tiles of one survey merge into both.
            let overridden = (rank + 1..members.len())
                .any(|higher| overlaps(members[higher], &grids[higher], a, b, c));
            if overridden {
                continue;
            }
            triangles.push([base + triangle[0], base + triangle[1], base + triangle[2]]);
            // One bit per edge, rebuilt as the member had it so that a
            // breakline stays a breakline in the combined surface.
            let mut mask = 0u8;
            for e in 0..3 {
                if member.is_edge_constrained(t, e) {
                    mask |= 1 << e;
                }
            }
            constrained.push(mask);
        }
    }

    // Vertices of a member whose triangles were all overridden are left in
    // the list: TinSurface::create accepts them, and compacting would cost a
    // pass over every vertex of every surface to save memory that the
    // triangles already dwarf.
    TinSurface::create(vertices, triangles, constrained)
}
