//! Triangulated irregular network surface.
//!
//! TinSurface is an immutable, self-contained value: plain Katana types in flat
//! arrays, no third-party objects. It can be copied, stored, serialised and handed
//! to a renderer. Surfaces are produced by build_tin() (tin_builder) or, for
//! triangulations that already exist (imported LandXML, stored projects), by
//! TinSurface::create().
//!
//! Conventions
//!   * Plan view is the XY plane; z is the elevation. Model units are metres.
//!   * Triangles are counter-clockwise in plan view. Edge k of a triangle joins
//!     its vertex k to its vertex (k + 1) % 3.
//!   * Indices are 32 bit: a surface holds fewer than 2^32 - 1 vertices and
//!     triangles (12 bytes per triangle instead of 24; see docs/terrain.md).
//!   * Coincidence decisions use tolerance::GEOMETRIC, direction decisions
//!     tolerance::ANGULAR. Nothing else.
//!   * All `&self` methods are safe to call concurrently.

use std::sync::Mutex;

use katana_core::task_pool::TaskPool;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::{Segment2, Triangle2, Triangle3};
use katana_math::numerics::{normalize_angle, tolerance};

use crate::summation::CompensatedSum;

pub use katana_geometry::{Box2, Point2, Point3};

/// Point-location index of a TinSurface: a uniform grid over its bounds, each
/// cell listing the triangles that reach into it (CSR layout, two flat arrays).
/// Implementation detail; not part of the API.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatorGrid {
    pub origin: Point2,
    pub cell_width: f64,
    pub cell_height: f64,
    pub columns: u32,
    pub rows: u32,
    /// columns * rows + 1 offsets
    pub cell_start: Vec<u32>,
    /// ascending within each cell
    pub cell_triangles: Vec<u32>,
}

impl Default for LocatorGrid {
    fn default() -> Self {
        Self {
            origin: Point2::default(),
            cell_width: 1.0,
            cell_height: 1.0,
            columns: 0,
            rows: 0,
            cell_start: Vec::new(),
            cell_triangles: Vec::new(),
        }
    }
}

pub const NO_TRIANGLE: u32 = u32::MAX;
pub const NO_VERTEX: u32 = u32::MAX;

/// Vertex indices of one triangle, counter-clockwise in plan view.
pub type TinTriangle = [u32; 3];
/// Triangle across edge k (vertex k -> vertex k + 1); NO_TRIANGLE on the surface edge.
pub type TinNeighbors = [u32; 3];

/// Where a plan position falls on the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceLocation {
    pub triangle: u32,
    /// Barycentric weights of the triangle's vertices; non-negative, sum to 1.
    pub weights: [f64; 3],
    /// False when the position was matched only through the GEOMETRIC tolerance
    /// (it lies on the rim of the surface, or on a sliver triangle); `weights`
    /// then describe the closest point of the triangle.
    pub interior: bool,
}

impl Default for SurfaceLocation {
    fn default() -> Self {
        Self {
            triangle: NO_TRIANGLE,
            weights: [0.0; 3],
            interior: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlopeAspect {
    /// Rise over run (the tangent of slope_angle). Dimensionless, >= 0.
    pub slope: f64,
    /// Inclination of the triangle against the horizontal. Radians in [0, pi/2).
    pub slope_angle: f64,
    /// Azimuth of steepest descent: radians clockwise from north (+y) in
    /// [0, 2*pi). None when the triangle is flat (slope_angle <= ANGULAR).
    pub aspect: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct TinSurface {
    pub(crate) vertices: Vec<Point3>,
    pub(crate) triangles: Vec<TinTriangle>,
    pub(crate) neighbors: Vec<TinNeighbors>,
    pub(crate) constrained_edges: Vec<u8>,
    pub(crate) bounds: Box2,
    pub(crate) min_elevation: f64,
    pub(crate) max_elevation: f64,
    pub(crate) plan_area: f64,
    pub(crate) surface_area: f64,
    pub(crate) grid: LocatorGrid,
}

/// Exact comparison of the defining arrays (vertices, triangles, adjacency,
/// constraint flags). Used to assert determinism.
impl PartialEq for TinSurface {
    fn eq(&self, other: &Self) -> bool {
        self.vertices == other.vertices
            && self.triangles == other.triangles
            && self.neighbors == other.neighbors
            && self.constrained_edges == other.constrained_edges
    }
}

/// Reach of the locator grid. A position within GEOMETRIC of a triangle must
/// find that triangle in its cell, and the mapping from a coordinate to a cell
/// is itself rounded (by less than 1e-8 m for coordinates up to 1e8 m). Listing
/// every triangle in all cells within twice GEOMETRIC of it covers both; this is
/// GEOMETRIC applied with a safety factor, not a new tolerance.
const LOCATOR_MARGIN: f64 = 2.0 * tolerance::GEOMETRIC;

/// Average number of triangles per grid cell the grid is sized for. Fewer means
/// faster queries and more memory (each triangle is listed in every cell it
/// reaches into); 2 gives ~4 list entries per triangle and ~8 candidates per query.
const TRIANGLES_PER_CELL: f64 = 2.0;

/// Positions per parallel chunk of elevations_at(). elevation_at() measured
/// 1.09 us on the 200 000 triangle surface of BM_ElevationAt (Release, GCC 16.2,
/// 16 x 2496 MHz), so a chunk is a quarter of a millisecond of work - far
/// above what publishing a job to the pool costs, and still small enough
/// that a 4096-position section leaves every thread a share.
const ELEVATION_GRAIN: usize = 256;

/// std::min of two doubles: the first unless the second is smaller.
pub(crate) fn std_min(first: f64, second: f64) -> f64 {
    if second < first { second } else { first }
}

/// std::max of two doubles: the first unless it is smaller than the second.
pub(crate) fn std_max(first: f64, second: f64) -> f64 {
    if first < second { second } else { first }
}

/// std::min({a, b, c}).
pub(crate) fn std_min3(a: f64, b: f64, c: f64) -> f64 {
    std_min(std_min(a, b), c)
}

/// std::max({a, b, c}).
pub(crate) fn std_max3(a: f64, b: f64, c: f64) -> f64 {
    std_max(std_max(a, b), c)
}

/// std::clamp: `low` when value < low, `high` when high < value, else value.
pub(crate) fn std_clamp(value: f64, low: f64, high: f64) -> f64 {
    if value < low {
        low
    } else if high < value {
        high
    } else {
        value
    }
}

/// Monotonic in `value`, which is what makes the grid exact: a position inside a
/// triangle's bounding box always maps into the cell range of that box.
pub(crate) fn cell_coordinate(value: f64, origin: f64, cell_size: f64, count: u32) -> u32 {
    let cell = ((value - origin) / cell_size).floor();
    if !(cell > 0.0) {
        return 0;
    }
    if cell >= f64::from(count) {
        count - 1
    } else {
        cell as u32
    }
}

/// True when the cell rectangle grown by LOCATOR_MARGIN lies entirely outside one
/// edge line of the counter-clockwise triangle a, b, c: the triangle cannot reach
/// the cell. Conservative (never true for a cell the triangle touches). Long thin
/// triangles along a diagonal hull would otherwise be listed in every cell of
/// their bounding box.
fn cell_is_separated(a: Point2, b: Point2, c: Point2, cell_min: Point2, cell_max: Point2) -> bool {
    let corners = [a, b, c];
    for k in 0..3 {
        let p = corners[k];
        let d = corners[(k + 1) % 3] - p;
        // Rectangle corner furthest to the left (inner side) of the edge.
        let inner = Point2::new(
            if d.y >= 0.0 { cell_min.x } else { cell_max.x },
            if d.x >= 0.0 { cell_max.y } else { cell_min.y },
        );
        if d.cross(inner - p) < -LOCATOR_MARGIN * (d.x.abs() + d.y.abs()) {
            return true;
        }
    }
    false
}

/// Calls visit(cell_index) for every grid cell the triangle may reach into.
fn visit_cells(grid: &LocatorGrid, a: Point2, b: Point2, c: Point2, mut visit: impl FnMut(usize)) {
    let min_x = std_min3(a.x, b.x, c.x) - LOCATOR_MARGIN;
    let max_x = std_max3(a.x, b.x, c.x) + LOCATOR_MARGIN;
    let min_y = std_min3(a.y, b.y, c.y) - LOCATOR_MARGIN;
    let max_y = std_max3(a.y, b.y, c.y) + LOCATOR_MARGIN;
    let column0 = cell_coordinate(min_x, grid.origin.x, grid.cell_width, grid.columns);
    let column1 = cell_coordinate(max_x, grid.origin.x, grid.cell_width, grid.columns);
    let row0 = cell_coordinate(min_y, grid.origin.y, grid.cell_height, grid.rows);
    let row1 = cell_coordinate(max_y, grid.origin.y, grid.cell_height, grid.rows);

    // The separation test needs a reliable orientation; slivers keep the box.
    let prune = (column1 > column0 || row1 > row0) && (b - a).cross(c - a) > 0.0;
    for row in row0..=row1 {
        for column in column0..=column1 {
            if prune {
                let cell_min = Point2::new(
                    grid.origin.x + f64::from(column) * grid.cell_width,
                    grid.origin.y + f64::from(row) * grid.cell_height,
                );
                let cell_max =
                    Point2::new(cell_min.x + grid.cell_width, cell_min.y + grid.cell_height);
                if cell_is_separated(a, b, c, cell_min, cell_max) {
                    continue;
                }
            }
            visit(row as usize * grid.columns as usize + column as usize);
        }
    }
}

/// Height over the longest edge <= GEOMETRIC, the criterion of
/// Triangle2::is_degenerate(), from the doubled signed area and without roots.
/// Barycentric weights of such a triangle are dominated by rounding.
fn is_sliver(a: Point2, b: Point2, c: Point2, twice_area: f64) -> bool {
    let longest_squared = std_max3(
        (b - a).length_squared(),
        (c - b).length_squared(),
        (a - c).length_squared(),
    );
    !(twice_area > 0.0)
        || twice_area * twice_area <= tolerance::GEOMETRIC * tolerance::GEOMETRIC * longest_squared
}

#[derive(Clone, Copy)]
struct HalfEdge {
    /// smaller vertex index of the edge
    low: u32,
    /// larger vertex index
    high: u32,
    triangle: u32,
    edge: u8,
    /// the triangle runs the edge from low to high
    ascending: bool,
}

/// Values computed by index on a task pool, the same at any thread count:
/// each index's answer depends on nothing but that index, and the pieces are
/// put back in index order.
pub(crate) fn compute_by_index<T, F>(
    pool: &TaskPool,
    count: usize,
    grain: usize,
    initial: T,
    compute: F,
) -> Vec<T>
where
    T: Clone + Send,
    F: Fn(usize) -> T + Sync,
{
    let pieces: Mutex<Vec<(usize, Vec<T>)>> = Mutex::new(Vec::new());
    pool.parallel_ranges(0, count, grain, |low, high| {
        let values: Vec<T> = (low..high).map(&compute).collect();
        pieces
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((low, values));
    });
    let mut result = vec![initial; count];
    for (low, values) in pieces
        .into_inner()
        .unwrap_or_else(|error| error.into_inner())
    {
        let high = low + values.len();
        result[low..high].clone_from_slice(&values);
    }
    result
}

impl TinSurface {
    /// The empty surface.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a surface from an existing triangulation. `constrained_edges` is
    /// either empty or holds one bit mask per triangle (bit k set when edge k is a
    /// breakline/boundary edge). Adjacency is derived. Fails with
    ///   InvalidArgument  non-finite coordinates, indices out of range, a triangle
    ///                    naming a vertex twice, flags of the wrong size or
    ///                    disagreeing across a shared edge;
    ///   InvalidGeometry  a clockwise triangle, or an edge used by more than two
    ///                    triangles / twice in the same direction (non-manifold).
    /// Triangles that are degenerate in plan view (height <= GEOMETRIC) are
    /// accepted: exact triangulators legitimately produce them from nearly
    /// collinear points, and removing them would tear the adjacency.
    pub fn create(
        vertices: Vec<Point3>,
        triangles: Vec<TinTriangle>,
        constrained_edges: Vec<u8>,
    ) -> Result<TinSurface> {
        let mut constrained_edges = constrained_edges;
        if vertices.len() >= NO_VERTEX as usize || triangles.len() >= NO_TRIANGLE as usize {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "surface exceeds the 32-bit index range",
            ));
        }
        for (i, vertex) in vertices.iter().enumerate() {
            if !vertex.is_finite() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "vertex has a non-finite coordinate",
                )
                .with_context(format!("vertex={i}")));
            }
        }
        if !constrained_edges.is_empty() && constrained_edges.len() != triangles.len() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "constrainedEdges must be empty or hold one mask per triangle",
            )
            .with_context(format!(
                "masks={} triangles={}",
                constrained_edges.len(),
                triangles.len()
            )));
        }
        if constrained_edges.is_empty() {
            constrained_edges = vec![0; triangles.len()];
        }

        let mut half_edges: Vec<HalfEdge> = Vec::with_capacity(3 * triangles.len());
        for (t, tri) in triangles.iter().enumerate() {
            let context = format!("triangle={t}");
            for &v in tri {
                if v as usize >= vertices.len() {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "triangle names a vertex out of range",
                    )
                    .with_context(context));
                }
            }
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[2] == tri[0] {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "triangle names a vertex twice",
                )
                .with_context(context));
            }
            if constrained_edges[t] > 7 {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "constraint mask uses bits beyond 0..2",
                )
                .with_context(context));
            }
            let plan = Triangle2::new(
                plan_of(vertices[tri[0] as usize]),
                plan_of(vertices[tri[1] as usize]),
                plan_of(vertices[tri[2] as usize]),
            );
            if plan.signed_area() < 0.0 && !plan.is_degenerate() {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "triangle is clockwise in plan view; triangles must be counter-clockwise",
                )
                .with_context(context));
            }
            for k in 0..3u8 {
                let from = tri[k as usize];
                let to = tri[(k as usize + 1) % 3];
                half_edges.push(HalfEdge {
                    low: from.min(to),
                    high: from.max(to),
                    triangle: t as u32,
                    edge: k,
                    ascending: from < to,
                });
            }
        }

        // Adjacency: the two uses of an interior edge are neighbours in the sorted list.
        half_edges.sort_unstable_by_key(|half| (half.low, half.high, half.triangle));
        let mut neighbors = vec![[NO_TRIANGLE; 3]; triangles.len()];
        let mut i = 0;
        while i < half_edges.len() {
            let mut j = i + 1;
            while j < half_edges.len()
                && half_edges[j].low == half_edges[i].low
                && half_edges[j].high == half_edges[i].high
            {
                j += 1;
            }
            let first = half_edges[i];
            let context = format!("edge={}-{}", first.low, first.high);
            if j - i > 2 {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "edge is shared by more than two triangles",
                )
                .with_context(context));
            }
            if j - i == 2 {
                let second = half_edges[i + 1];
                if first.ascending == second.ascending {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "two triangles run along an edge in the same direction \
                         (overlapping or inconsistently oriented triangles)",
                    )
                    .with_context(context));
                }
                let first_flag = (constrained_edges[first.triangle as usize] >> first.edge) & 1;
                let second_flag = (constrained_edges[second.triangle as usize] >> second.edge) & 1;
                if first_flag != second_flag {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "constraint flags disagree across a shared edge",
                    )
                    .with_context(context));
                }
                neighbors[first.triangle as usize][first.edge as usize] = second.triangle;
                neighbors[second.triangle as usize][second.edge as usize] = first.triangle;
            }
            i = j;
        }

        let mut surface = TinSurface {
            vertices,
            triangles,
            neighbors,
            constrained_edges,
            ..TinSurface::default()
        };
        surface.finalize()?;
        Ok(surface)
    }

    /// Trusted construction of a surface from arrays the module produced itself
    /// (triangulation output): skips the validation and the adjacency derivation
    /// of create(), which would cost a sort over all half-edges.
    ///
    /// Preconditions (not checked): indices in range, triangles counter-clockwise
    /// by exact orientation, `neighbors` consistent with `triangles`,
    /// `constrained_edges` one mask per triangle and symmetric across shared edges.
    pub(crate) fn assemble(
        vertices: Vec<Point3>,
        triangles: Vec<TinTriangle>,
        neighbors: Vec<TinNeighbors>,
        constrained_edges: Vec<u8>,
    ) -> Result<TinSurface> {
        let mut surface = TinSurface {
            vertices,
            triangles,
            neighbors,
            constrained_edges,
            ..TinSurface::default()
        };
        surface.finalize()?;
        Ok(surface)
    }

    /// Derives bounds, areas, elevation range and the locator grid from the arrays.
    fn finalize(&mut self) -> Status {
        self.bounds = Box2::default();
        self.min_elevation = 0.0;
        self.max_elevation = 0.0;
        self.plan_area = 0.0;
        self.surface_area = 0.0;
        self.grid = LocatorGrid::default();
        if self.triangles.is_empty() {
            return Ok(());
        }

        let mut min_z = f64::INFINITY;
        let mut max_z = f64::NEG_INFINITY;
        let mut plan_sum = CompensatedSum::new();
        let mut surface_sum = CompensatedSum::new();
        for t in 0..self.triangles.len() {
            for &v in &self.triangles[t] {
                let p = self.vertices[v as usize];
                self.bounds.expand(Point2::new(p.x, p.y));
                min_z = std_min(min_z, p.z);
                max_z = std_max(max_z, p.z);
            }
            plan_sum.add(self.triangle_plan_area(t));
            surface_sum.add(self.triangle_surface_area(t));
        }
        self.min_elevation = min_z;
        self.max_elevation = max_z;
        self.plan_area = plan_sum.value();
        self.surface_area = surface_sum.value();

        // Grid of roughly square cells holding TRIANGLES_PER_CELL triangles on average.
        let width = self.bounds.width();
        let height = self.bounds.height();
        let target_cells = std_max(1.0, self.triangles.len() as f64 / TRIANGLES_PER_CELL);
        let mut grid = LocatorGrid {
            origin: self.bounds.min,
            columns: 1,
            rows: 1,
            ..LocatorGrid::default()
        };
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
        let mut counts: Vec<u64> = vec![0; cell_count + 1];
        for t in 0..self.triangles.len() {
            let [a, b, c] = self.plan_corners(t);
            visit_cells(&grid, a, b, c, |cell| counts[cell + 1] += 1);
        }
        for cell in 0..cell_count {
            counts[cell + 1] += counts[cell];
        }
        if counts[cell_count] >= u64::from(NO_TRIANGLE) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "surface is too large for the 32-bit locator index",
            )
            .with_context(format!("entries={}", counts[cell_count])));
        }
        grid.cell_start = counts.iter().map(|&count| count as u32).collect();
        grid.cell_triangles = vec![0; counts[cell_count] as usize];
        let mut cursor: Vec<u32> = grid.cell_start[..cell_count].to_vec();
        for t in 0..self.triangles.len() {
            let [a, b, c] = self.plan_corners(t);
            let mut cells = Vec::new();
            visit_cells(&grid, a, b, c, |cell| cells.push(cell));
            for cell in cells {
                grid.cell_triangles[cursor[cell] as usize] = t as u32;
                cursor[cell] += 1;
            }
        }
        self.grid = grid;
        Ok(())
    }

    fn plan_corners(&self, triangle: usize) -> [Point2; 3] {
        let tri = self.triangles[triangle];
        [
            plan_of(self.vertices[tri[0] as usize]),
            plan_of(self.vertices[tri[1] as usize]),
            plan_of(self.vertices[tri[2] as usize]),
        ]
    }

    // ---- data -------------------------------------------------------------------

    pub fn empty(&self) -> bool {
        self.triangles.is_empty()
    }
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn vertices(&self) -> &Vec<Point3> {
        &self.vertices
    }
    pub fn triangles(&self) -> &Vec<TinTriangle> {
        &self.triangles
    }
    pub fn neighbors(&self) -> &Vec<TinNeighbors> {
        &self.neighbors
    }
    /// One bit mask per triangle; bit k set when edge k is constrained.
    pub fn constrained_edges(&self) -> &Vec<u8> {
        &self.constrained_edges
    }
    pub fn is_edge_constrained(&self, triangle: usize, edge: usize) -> bool {
        edge < 3 && ((self.constrained_edges[triangle] >> edge) & 1) != 0
    }

    /// Plan extent and elevation range of the triangulated vertices. The box is
    /// empty and the elevations are 0 for the empty surface.
    pub fn bounds(&self) -> &Box2 {
        &self.bounds
    }
    pub fn min_elevation(&self) -> f64 {
        self.min_elevation
    }
    pub fn max_elevation(&self) -> f64 {
        self.max_elevation
    }

    /// Sum of the triangle areas projected to the XY plane / measured in 3D.
    pub fn plan_area(&self) -> f64 {
        self.plan_area
    }
    pub fn surface_area(&self) -> f64 {
        self.surface_area
    }

    // ---- per triangle (panic for a bad index, as the C++ throws) -----------------

    pub fn plan_triangle(&self, triangle: usize) -> Triangle2 {
        let tri = self.triangles[triangle];
        let a = self.vertices[tri[0] as usize];
        let b = self.vertices[tri[1] as usize];
        let c = self.vertices[tri[2] as usize];
        Triangle2::new(
            Point2::new(a.x, a.y),
            Point2::new(b.x, b.y),
            Point2::new(c.x, c.y),
        )
    }

    pub fn space_triangle(&self, triangle: usize) -> Triangle3 {
        let tri = self.triangles[triangle];
        Triangle3::new(
            self.vertices[tri[0] as usize],
            self.vertices[tri[1] as usize],
            self.vertices[tri[2] as usize],
        )
    }

    pub fn triangle_plan_area(&self, triangle: usize) -> f64 {
        // Slivers that are counter-clockwise by exact arithmetic can evaluate to a
        // rounding-sized negative area; they have no area.
        std_max(0.0, self.plan_triangle(triangle).signed_area())
    }

    pub fn triangle_surface_area(&self, triangle: usize) -> f64 {
        self.space_triangle(triangle).area()
    }

    /// None when the triangle is degenerate in plan view (its plane is
    /// vertical or undefined).
    pub fn triangle_slope_aspect(&self, triangle: usize) -> Option<SlopeAspect> {
        if self.plan_triangle(triangle).is_degenerate() {
            return None;
        }
        // Normal n = (b - a) x (c - a) points up (n.z = twice the plan area > 0). The
        // plane is z = const - (n.x * x + n.y * y) / n.z, so the gradient is
        // -(n.x, n.y) / n.z and the steepest descent runs along +(n.x, n.y).
        let normal = self.space_triangle(triangle).scaled_normal();
        let horizontal = normal.x.hypot(normal.y);
        let mut result = SlopeAspect {
            slope: horizontal / normal.z,
            slope_angle: horizontal.atan2(normal.z),
            aspect: None,
        };
        if result.slope_angle > tolerance::ANGULAR {
            // Azimuth: clockwise from +y, i.e. atan2(east, north).
            result.aspect = Some(normalize_angle(normal.x.atan2(normal.y)));
        }
        Some(result)
    }

    // ---- point queries -------------------------------------------------------------
    //
    // Backed by a uniform bucket grid built with the surface: expected O(1) per
    // query. A position is on the surface when it is inside a triangle or within
    // GEOMETRIC of one, so points exactly on edges, vertices and the rim work.
    // Where triangles share the position the lowest triangle index is reported.
    // None: outside the surface, inside a hole, or non-finite input.

    pub fn locate(&self, position: Point2) -> Option<SurfaceLocation> {
        if self.triangles.is_empty()
            || !position.is_finite()
            || !self
                .bounds
                .inflated(tolerance::GEOMETRIC)
                .contains(position)
        {
            return None;
        }
        let grid = &self.grid;
        let cell = cell_coordinate(position.y, grid.origin.y, grid.cell_height, grid.rows) as usize
            * grid.columns as usize
            + cell_coordinate(position.x, grid.origin.x, grid.cell_width, grid.columns) as usize;
        let first = grid.cell_start[cell] as usize;
        let last = grid.cell_start[cell + 1] as usize;

        // Pass 1: containment by the three edge functions, evaluated on coordinate
        // differences so that large projected coordinates keep their precision.
        for i in first..last {
            let t = grid.cell_triangles[i];
            let [a, b, c] = self.plan_corners(t as usize);
            let d0 = (b - a).cross(position - a); // weight of c, times twice the area
            let d1 = (c - b).cross(position - b); // weight of a
            let d2 = (a - c).cross(position - c); // weight of b
            if d0 < 0.0 || d1 < 0.0 || d2 < 0.0 {
                continue;
            }
            let twice_area = d0 + d1 + d2;
            if is_sliver(a, b, c, twice_area) {
                continue; // handled by distance below
            }
            return Some(SurfaceLocation {
                triangle: t,
                weights: [d1 / twice_area, d2 / twice_area, d0 / twice_area],
                interior: true,
            });
        }

        // Pass 2: the position is on no triangle by the sign tests. It may still lie
        // within GEOMETRIC of one (on the rim, in a rounding-sized crack between
        // two triangles, or on a sliver): take the closest edge, lowest triangle
        // index first, and interpolate along it.
        let mut best_distance = tolerance::GEOMETRIC;
        let mut best: Option<SurfaceLocation> = None;
        for i in first..last {
            let t = grid.cell_triangles[i];
            let corners = self.plan_corners(t as usize);
            for k in 0..3 {
                let edge = Segment2::new(corners[k], corners[(k + 1) % 3]);
                let parameter = edge.parameter_of(position);
                let distance = edge.point_at(parameter).distance_to(position);
                if distance < best_distance || (distance == best_distance && best.is_none()) {
                    best_distance = distance;
                    let mut location = SurfaceLocation {
                        triangle: t,
                        weights: [0.0; 3],
                        interior: false,
                    };
                    location.weights[k] = 1.0 - parameter;
                    location.weights[(k + 1) % 3] = parameter;
                    best = Some(location);
                }
            }
        }
        best
    }

    /// Barycentric (linear) interpolation of the vertex elevations.
    pub fn elevation_at(&self, position: Point2) -> Option<f64> {
        let location = self.locate(position)?;
        let tri = self.triangles[location.triangle as usize];
        Some(
            location.weights[0] * self.vertices[tri[0] as usize].z
                + location.weights[1] * self.vertices[tri[1] as usize].z
                + location.weights[2] * self.vertices[tri[2] as usize].z,
        )
    }

    pub fn elevation_at_xy(&self, x: f64, y: f64) -> Option<f64> {
        self.elevation_at(Point2::new(x, y))
    }

    /// Spot elevations: elevation_at() for every position, in order.
    pub fn elevations_at(&self, positions: &[Point2]) -> Vec<Option<f64>> {
        // Written by index, never appended: each position's answer depends on
        // nothing but that position, so the values returned are the same at
        // any thread count. locate() reads only the immutable surface, which
        // is this type's concurrency promise.
        if positions.len() <= ELEVATION_GRAIN {
            // nothing to share out; waking the pool would cost more
            return positions
                .iter()
                .map(|&position| self.elevation_at(position))
                .collect();
        }
        compute_by_index(
            TaskPool::shared(),
            positions.len(),
            ELEVATION_GRAIN,
            None,
            |i| self.elevation_at(positions[i]),
        )
    }

    /// Slope (rise over run) and aspect of the located triangle. aspect_at() is
    /// also None on flat ground. Both are discontinuous across edges; on a
    /// shared edge the lowest-index triangle decides.
    pub fn slope_at(&self, position: Point2) -> Option<f64> {
        let location = self.locate(position)?;
        self.triangle_slope_aspect(location.triangle as usize)
            .map(|slope_aspect| slope_aspect.slope)
    }

    pub fn aspect_at(&self, position: Point2) -> Option<f64> {
        let location = self.locate(position)?;
        self.triangle_slope_aspect(location.triangle as usize)
            .and_then(|slope_aspect| slope_aspect.aspect)
    }
}

/// The plan position of a vertex.
pub(crate) fn plan_of(point: Point3) -> Point2 {
    Point2::new(point.x, point.y)
}
