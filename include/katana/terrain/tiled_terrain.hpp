#pragma once

// Tiled terrain for large point sets (PLAN.MD Phase 14: "Support tiled terrain
// for large datasets"). First version; see docs/terrain.md for the limits.
//
// The plan extent of the points is cut into a regular grid of square tiles.
// Every tile owns a TinSurface triangulated from the points inside the tile
// grown by `bufferWidth` on all sides. A query is answered by the tile that
// contains the position.
//
// Agreement with a monolithic TIN. A Delaunay triangle is decided by the points
// inside its circumcircle. When the circumcircle of the tile triangle that
// answers a query holds no part of the data extent outside the buffered tile,
// the tile saw every point that could have changed that triangle, so it is a
// triangle of the monolithic Delaunay TIN as well and the interpolated elevation
// is the same (up to rounding, and up to the free choice of diagonal among
// exactly cocircular points). Such answers are flagged `certified`. With a
// buffer of a few point spacings this covers the inside of the data; the long
// thin triangles along the convex hull of the data have huge circumcircles and
// stay uncertified: there the tile answer is a valid interpolation of the nearby
// points, but may differ from a monolithic TIN and from the neighbouring tile.
//
// Tiles are built on request. computeTile() is const and touches no shared
// state, so different tiles can be built at the same time: buildAll() does
// exactly that through core::TaskPool (Phase 19). Nothing else here starts a
// thread, and buildAll()'s answer - the surfaces and the Status - does not
// depend on the thread count.
//
// Not supported yet: breaklines, boundaries and holes (create() fails with
// Unsupported rather than building a surface that ignores them), and
// out-of-core storage (all points stay in memory).

#include <cstddef>
#include <cstdint>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::terrain {

struct TiledTerrainOptions {
    double tileSize = 0.0;    // metres, > 0
    double bufferWidth = 0.0; // metres, >= 0; use several times the point spacing
    TinBuildOptions build;    // applied to every tile
};

struct TileSample {
    std::optional<double> elevation; // nullopt: the position is off the tile's surface
    // True when the answer provably equals that of a monolithic Delaunay TIN of
    // all points (see the header comment). An off-surface answer is certified
    // only outside the data extent.
    bool certified = false;
};

class TiledTerrain {
  public:
    // More tiles than this is a unit mistake in tileSize (InvalidArgument).
    static constexpr std::size_t kMaxTileCount = 65536;

    // Validates the input and sorts the points into tiles; builds no tile. Fails with
    //   InvalidArgument       non-finite coordinate, tileSize <= 0, bufferWidth < 0,
    //                         more than kMaxTileCount tiles;
    //   Unsupported           breaklines, boundary or holes present;
    //   TriangulationFailure  fewer than 3 points.
    [[nodiscard]] static katana::core::Result<TiledTerrain>
    create(TinInput input, const TiledTerrainOptions& options);

    [[nodiscard]] std::size_t columns() const { return columns_; }
    [[nodiscard]] std::size_t rows() const { return rows_; }
    [[nodiscard]] std::size_t tileCount() const { return columns_ * rows_; }
    [[nodiscard]] std::size_t pointCount() const { return points_.size(); }
    [[nodiscard]] const Box2& bounds() const { return bounds_; }

    // Tile index = row * columns() + column. Throws std::out_of_range for a bad index.
    [[nodiscard]] Box2 tileBounds(std::size_t tile) const;         // without the buffer
    [[nodiscard]] Box2 bufferedTileBounds(std::size_t tile) const; // with the buffer
    // Tile that answers queries at `position`; nullopt outside bounds().
    [[nodiscard]] std::optional<std::size_t> tileAt(const Point2& position) const;

    // Triangulates one tile without storing it. A tile whose buffered points
    // cannot be triangulated (fewer than 3, or collinear) yields the empty
    // surface; every other failure of buildTin() is passed on.
    [[nodiscard]] katana::core::Result<TinSurface> computeTile(std::size_t tile) const;

    [[nodiscard]] katana::core::Status buildTile(std::size_t tile); // no-op when built
    [[nodiscard]] katana::core::Status buildAll();
    // Builds the tile that answers queries at `position`, if there is one.
    [[nodiscard]] katana::core::Status ensureTileAt(const Point2& position);
    void evictTile(std::size_t tile); // frees the tile's surface; it can be rebuilt
    [[nodiscard]] bool isBuilt(std::size_t tile) const;
    // Observer; nullptr when the tile is not built. Invalidated by evictTile().
    [[nodiscard]] const TinSurface* tileSurface(std::size_t tile) const;

    // Fails with InvalidState when the responsible tile has not been built
    // (queries never build tiles: they are const and thread-safe).
    [[nodiscard]] katana::core::Result<TileSample> sampleAt(const Point2& position) const;
    // sampleAt().elevation: the tile's answer whether certified or not.
    [[nodiscard]] katana::core::Result<std::optional<double>>
    elevationAt(const Point2& position) const;

  private:
    struct TileSlot {
        bool built = false;
        TinSurface surface;
    };

    TiledTerrain() = default;

    std::vector<Point3> points_;
    TiledTerrainOptions options_;
    Box2 bounds_;
    std::size_t columns_ = 0;
    std::size_t rows_ = 0;
    // Points bucketed by tile (CSR): indices into points_, ascending per tile.
    std::vector<std::uint32_t> tilePointStart_;
    std::vector<std::uint32_t> tilePoints_;
    std::vector<TileSlot> tiles_;
};

} // namespace katana::terrain
