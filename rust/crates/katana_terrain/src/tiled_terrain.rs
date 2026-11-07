//! Tiled terrain for large point sets. First version; see docs/terrain.md for
//! the limits.
//!
//! The plan extent of the points is cut into a regular grid of square tiles.
//! Every tile owns a TinSurface triangulated from the points inside the tile
//! grown by `buffer_width` on all sides. A query is answered by the tile that
//! contains the position.
//!
//! Agreement with a monolithic TIN. A Delaunay triangle is decided by the points
//! inside its circumcircle. When the circumcircle of the tile triangle that
//! answers a query holds no part of the data extent outside the buffered tile,
//! the tile saw every point that could have changed that triangle, so it is a
//! triangle of the monolithic Delaunay TIN as well and the interpolated elevation
//! is the same (up to rounding). Such answers are flagged `certified`. With a
//! buffer of a few point spacings this covers the inside of the data; the long
//! thin triangles along the convex hull of the data have huge circumcircles and
//! stay uncertified: there the tile answer is a valid interpolation of the nearby
//! points, but may differ from a monolithic TIN and from the neighbouring tile.
//!
//! Tiles are built on request. compute_tile() takes `&self` and touches no
//! shared state, so different tiles can be built at the same time: build_all()
//! does exactly that through the task pool. Nothing else here starts a thread,
//! and build_all()'s answer - the surfaces and the Status - does not depend on
//! the thread count.
//!
//! Not supported yet: breaklines, boundaries and holes (create() fails with
//! Unsupported rather than building a surface that ignores them), and
//! out-of-core storage (all points stay in memory).

use katana_core::task_pool::TaskPool;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::tolerance;

use crate::tin_builder::{TinBuildOptions, TinInput, build_tin};
use crate::tin_surface::{Box2, NO_VERTEX, Point2, Point3, TinSurface, compute_by_index, std_max};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TiledTerrainOptions {
    /// metres, > 0
    pub tile_size: f64,
    /// metres, >= 0; use several times the point spacing
    pub buffer_width: f64,
    /// applied to every tile
    pub build: TinBuildOptions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TileSample {
    /// None: the position is off the tile's surface
    pub elevation: Option<f64>,
    /// True when the answer provably equals that of a monolithic Delaunay TIN of
    /// all points (see the module comment). An off-surface answer is certified
    /// only outside the data extent.
    pub certified: bool,
}

#[derive(Clone, Debug, Default)]
struct TileSlot {
    built: bool,
    surface: TinSurface,
}

#[derive(Clone, Debug, Default)]
pub struct TiledTerrain {
    points: Vec<Point3>,
    options: TiledTerrainOptions,
    bounds: Box2,
    columns: usize,
    rows: usize,
    /// Points bucketed by tile (CSR): indices into points, ascending per tile.
    tile_point_start: Vec<u32>,
    tile_points: Vec<u32>,
    tiles: Vec<TileSlot>,
}

/// Tile column/row of a coordinate. Monotonic, and shared by the bucketing of the
/// points and the routing of queries, so a query always reaches the tile that
/// owns the points around it.
fn tile_coordinate(value: f64, origin: f64, tile_size: f64, count: usize) -> usize {
    let tile = ((value - origin) / tile_size).floor();
    if !(tile > 0.0) {
        return 0;
    }
    if tile >= count as f64 {
        count - 1
    } else {
        tile as usize
    }
}

impl TiledTerrain {
    /// More tiles than this is a unit mistake in tile_size (InvalidArgument).
    pub const MAX_TILE_COUNT: usize = 65536;

    /// Validates the input and sorts the points into tiles; builds no tile. Fails with
    ///   InvalidArgument       non-finite coordinate, tile_size <= 0, buffer_width < 0,
    ///                         more than MAX_TILE_COUNT tiles;
    ///   Unsupported           breaklines, boundary or holes present;
    ///   TriangulationFailure  fewer than 3 points.
    pub fn create(input: TinInput, options: &TiledTerrainOptions) -> Result<TiledTerrain> {
        if !(options.tile_size > 0.0) || !options.tile_size.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "tileSize must be positive and finite",
            )
            .with_context(format!("tileSize={}", format_exact_real(options.tile_size))));
        }
        if !(options.buffer_width >= 0.0) || !options.buffer_width.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "bufferWidth must be finite and not negative",
            )
            .with_context(format!(
                "bufferWidth={}",
                format_exact_real(options.buffer_width)
            )));
        }
        if !input.breaklines.is_empty()
            || !input.boundary.vertices.is_empty()
            || !input.holes.is_empty()
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "tiled terrain does not support breaklines, boundaries or holes yet; \
                 a constraint crossing a tile border would have to be split \
                 consistently in both tiles",
            ));
        }
        if input.points.len() < 3 {
            return Err(Error::new(
                ErrorCode::TriangulationFailure,
                "a terrain needs at least 3 points",
            )
            .with_context(format!("points={}", input.points.len())));
        }
        if input.points.len() >= NO_VERTEX as usize {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "too many points for 32-bit indices",
            )
            .with_context(format!("points={}", input.points.len())));
        }

        let mut terrain = TiledTerrain {
            options: *options,
            points: input.points,
            ..TiledTerrain::default()
        };
        for (i, p) in terrain.points.iter().enumerate() {
            if !p.is_finite() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "point has a non-finite coordinate",
                )
                .with_context(format!("point={i}")));
            }
            terrain.bounds.expand(Point2::new(p.x, p.y));
        }

        let columns = std_max(1.0, (terrain.bounds.width() / options.tile_size).ceil());
        let rows = std_max(1.0, (terrain.bounds.height() / options.tile_size).ceil());
        if columns * rows > Self::MAX_TILE_COUNT as f64 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "tileSize is too small for the extent of the points",
            )
            .with_context(format!(
                "tiles={} limit={}",
                format_exact_real(columns * rows),
                Self::MAX_TILE_COUNT
            )));
        }
        terrain.columns = columns as usize;
        terrain.rows = rows as usize;
        terrain.tiles = vec![TileSlot::default(); terrain.tile_count()];

        // Counting sort of the point indices by tile; ascending within each tile.
        let tiles_of: Vec<usize> = terrain
            .points
            .iter()
            .map(|p| terrain.tile_at(Point2::new(p.x, p.y)).unwrap_or(0))
            .collect();
        let mut start: Vec<u32> = vec![0; terrain.tile_count() + 1];
        for &tile in &tiles_of {
            start[tile + 1] += 1;
        }
        for tile in 0..terrain.tile_count() {
            start[tile + 1] += start[tile];
        }
        let mut tile_points: Vec<u32> = vec![0; terrain.points.len()];
        let mut cursor: Vec<u32> = start[..terrain.tile_count()].to_vec();
        for (i, &tile) in tiles_of.iter().enumerate() {
            tile_points[cursor[tile] as usize] = i as u32;
            cursor[tile] += 1;
        }
        terrain.tile_point_start = start;
        terrain.tile_points = tile_points;
        Ok(terrain)
    }

    pub fn columns(&self) -> usize {
        self.columns
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn tile_count(&self) -> usize {
        self.columns * self.rows
    }
    pub fn point_count(&self) -> usize {
        self.points.len()
    }
    pub fn bounds(&self) -> &Box2 {
        &self.bounds
    }

    /// Tile index = row * columns() + column. Panics for a bad index, as the
    /// C++ throws std::out_of_range. Without the buffer.
    pub fn tile_bounds(&self, tile: usize) -> Box2 {
        assert!(
            tile < self.tile_count(),
            "TiledTerrain: tile index out of range"
        );
        let column = (tile % self.columns) as f64;
        let row = (tile / self.columns) as f64;
        let lower = Point2::new(
            self.bounds.min.x + column * self.options.tile_size,
            self.bounds.min.y + row * self.options.tile_size,
        );
        Box2::new(
            lower,
            Point2::new(
                lower.x + self.options.tile_size,
                lower.y + self.options.tile_size,
            ),
        )
    }

    /// With the buffer.
    pub fn buffered_tile_bounds(&self, tile: usize) -> Box2 {
        self.tile_bounds(tile).inflated(self.options.buffer_width)
    }

    /// Tile that answers queries at `position`; None outside bounds().
    pub fn tile_at(&self, position: Point2) -> Option<usize> {
        if !position.is_finite() || !self.bounds.contains(position) {
            return None;
        }
        Some(
            tile_coordinate(
                position.y,
                self.bounds.min.y,
                self.options.tile_size,
                self.rows,
            ) * self.columns
                + tile_coordinate(
                    position.x,
                    self.bounds.min.x,
                    self.options.tile_size,
                    self.columns,
                ),
        )
    }

    /// Triangulates one tile without storing it. A tile whose buffered points
    /// cannot be triangulated (fewer than 3, or collinear) yields the empty
    /// surface; every other failure of build_tin() is passed on.
    pub fn compute_tile(&self, tile: usize) -> Result<TinSurface> {
        let buffered = self.buffered_tile_bounds(tile); // panics for a bad index

        // Candidate buckets: every tile the buffered box reaches into. The box test
        // on each point then decides, so the selection does not depend on how the
        // buckets are laid out.
        let size = self.options.tile_size;
        let column0 = tile_coordinate(buffered.min.x, self.bounds.min.x, size, self.columns);
        let column1 = tile_coordinate(buffered.max.x, self.bounds.min.x, size, self.columns);
        let row0 = tile_coordinate(buffered.min.y, self.bounds.min.y, size, self.rows);
        let row1 = tile_coordinate(buffered.max.y, self.bounds.min.y, size, self.rows);

        let mut selected: Vec<u32> = Vec::new();
        for row in row0..=row1 {
            for column in column0..=column1 {
                let bucket = row * self.columns + column;
                let first = self.tile_point_start[bucket] as usize;
                let last = self.tile_point_start[bucket + 1] as usize;
                for &index in &self.tile_points[first..last] {
                    let p = self.points[index as usize];
                    if buffered.contains(Point2::new(p.x, p.y)) {
                        selected.push(index);
                    }
                }
            }
        }
        // Input order, so that the tile's TIN is a function of the points alone.
        selected.sort_unstable();

        let input = TinInput {
            points: selected
                .iter()
                .map(|&index| self.points[index as usize])
                .collect(),
            ..TinInput::default()
        };
        match build_tin(&input, &self.options.build) {
            Ok(built) => Ok(built.surface),
            // too few or collinear points: no surface here
            Err(error) if error.code == ErrorCode::TriangulationFailure => Ok(TinSurface::new()),
            Err(error) => Err(error),
        }
    }

    /// No-op when built.
    pub fn build_tile(&mut self, tile: usize) -> Status {
        if tile >= self.tile_count() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "tile index out of range")
                    .with_context(format!("tile={tile}")),
            );
        }
        if self.tiles[tile].built {
            return Ok(());
        }
        let surface = self.compute_tile(tile)?;
        self.tiles[tile] = TileSlot {
            built: true,
            surface,
        };
        Ok(())
    }

    pub fn build_all(&mut self) -> Status {
        let count = self.tile_count();
        // Tiles are independent by construction: compute_tile() reads only the
        // points and the options, and each result lands in its own slot. One
        // tile per chunk because a tile is a whole triangulation.
        //
        // Each tile's Status is kept in its own slot and the LOWEST-index failure
        // is the one reported, so the error does not depend on which thread lost.
        // The tiles after a failure are built too, which is a fact about
        // recovery rather than about the answer - the Status returned is the
        // same, and a caller that cares asks is_built().
        let built: Vec<Option<Result<TinSurface>>> = {
            let terrain = &*self;
            compute_by_index(TaskPool::shared(), count, 1, None, |tile| {
                if terrain.tiles[tile].built {
                    None
                } else {
                    Some(terrain.compute_tile(tile))
                }
            })
        };
        let mut first_failure: Option<Error> = None;
        for (tile, outcome) in built.into_iter().enumerate() {
            match outcome {
                None => {}
                Some(Ok(surface)) => {
                    self.tiles[tile] = TileSlot {
                        built: true,
                        surface,
                    };
                }
                Some(Err(error)) => {
                    // The first failure in tile order is the one reported.
                    first_failure.get_or_insert(error);
                }
            }
        }
        match first_failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Builds the tile that answers queries at `position`, if there is one.
    pub fn ensure_tile_at(&mut self, position: Point2) -> Status {
        match self.tile_at(position) {
            Some(tile) => self.build_tile(tile),
            None => Ok(()),
        }
    }

    /// Frees the tile's surface; it can be rebuilt.
    pub fn evict_tile(&mut self, tile: usize) {
        if tile < self.tile_count() {
            self.tiles[tile] = TileSlot::default();
        }
    }

    pub fn is_built(&self, tile: usize) -> bool {
        tile < self.tile_count() && self.tiles[tile].built
    }

    /// None when the tile is not built.
    pub fn tile_surface(&self, tile: usize) -> Option<&TinSurface> {
        if self.is_built(tile) {
            Some(&self.tiles[tile].surface)
        } else {
            None
        }
    }

    /// Fails with InvalidState when the responsible tile has not been built
    /// (queries never build tiles: they take `&self` and are thread-safe).
    pub fn sample_at(&self, position: Point2) -> Result<TileSample> {
        let Some(tile) = self.tile_at(position) else {
            // outside the data extent: certainly no surface
            return Ok(TileSample {
                elevation: None,
                certified: true,
            });
        };
        if !self.tiles[tile].built {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the tile answering this position has not been built",
            )
            .with_context(format!("tile={tile}")));
        }
        let surface = &self.tiles[tile].surface;
        let Some(location) = surface.locate(position) else {
            return Ok(TileSample {
                elevation: None,
                certified: false,
            });
        };
        let tri = surface.triangles()[location.triangle as usize];
        let vertices = surface.vertices();
        let mut sample = TileSample {
            elevation: Some(
                location.weights[0] * vertices[tri[0] as usize].z
                    + location.weights[1] * vertices[tri[1] as usize].z
                    + location.weights[2] * vertices[tri[2] as usize].z,
            ),
            certified: false,
        };

        // Certificate: no part of the data extent outside the buffered tile lies
        // inside the triangle's circumcircle (see the module comment). The circle
        // is grown by the geometric tolerance to cover the rounding of its
        // construction.
        if let Some(circle) = surface
            .plan_triangle(location.triangle as usize)
            .circumcircle()
        {
            let buffered = self.buffered_tile_bounds(tile);
            let radius = circle.radius * (1.0 + tolerance::RELATIVE) + tolerance::GEOMETRIC;
            let c = circle.center;
            let bounds = &self.bounds;
            sample.certified = (c.x - radius >= buffered.min.x || buffered.min.x <= bounds.min.x)
                && (c.x + radius <= buffered.max.x || buffered.max.x >= bounds.max.x)
                && (c.y - radius >= buffered.min.y || buffered.min.y <= bounds.min.y)
                && (c.y + radius <= buffered.max.y || buffered.max.y >= bounds.max.y);
        }
        Ok(sample)
    }

    /// sample_at().elevation: the tile's answer whether certified or not.
    pub fn elevation_at(&self, position: Point2) -> Result<Option<f64>> {
        Ok(self.sample_at(position)?.elevation)
    }
}
