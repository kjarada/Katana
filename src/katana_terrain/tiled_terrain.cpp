#include "katana/terrain/tiled_terrain.hpp"

#include <algorithm>
#include <cmath>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "katana/core/task_pool.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// Tile column/row of a coordinate. Monotonic, and shared by the bucketing of the
// points and the routing of queries, so a query always reaches the tile that
// owns the points around it.
std::size_t tileCoordinate(double value, double origin, double tileSize, std::size_t count)
{
    const double tile = std::floor((value - origin) / tileSize);
    if (!(tile > 0.0)) {
        return 0;
    }
    return tile >= static_cast<double>(count) ? count - 1 : static_cast<std::size_t>(tile);
}

} // namespace

Result<TiledTerrain> TiledTerrain::create(TinInput input, const TiledTerrainOptions& options)
{
    if (!(options.tileSize > 0.0) || !std::isfinite(options.tileSize)) {
        return makeError(ErrorCode::InvalidArgument, "tileSize must be positive and finite",
                         "tileSize=" + std::to_string(options.tileSize));
    }
    if (!(options.bufferWidth >= 0.0) || !std::isfinite(options.bufferWidth)) {
        return makeError(ErrorCode::InvalidArgument, "bufferWidth must be finite and not negative",
                         "bufferWidth=" + std::to_string(options.bufferWidth));
    }
    if (!input.breaklines.empty() || !input.boundary.vertices.empty() || !input.holes.empty()) {
        return makeError(ErrorCode::Unsupported,
                         "tiled terrain does not support breaklines, boundaries or holes yet; "
                         "a constraint crossing a tile border would have to be split "
                         "consistently in both tiles");
    }
    if (input.points.size() < 3) {
        return makeError(ErrorCode::TriangulationFailure,
                         "a terrain needs at least 3 points",
                         "points=" + std::to_string(input.points.size()));
    }
    if (input.points.size() >= kNoVertex) {
        return makeError(ErrorCode::InvalidArgument, "too many points for 32-bit indices",
                         "points=" + std::to_string(input.points.size()));
    }

    TiledTerrain terrain;
    terrain.options_ = options;
    terrain.points_ = std::move(input.points);
    for (std::size_t i = 0; i < terrain.points_.size(); ++i) {
        const Point3& p = terrain.points_[i];
        if (!p.isFinite()) {
            return makeError(ErrorCode::InvalidArgument, "point has a non-finite coordinate",
                             "point=" + std::to_string(i));
        }
        terrain.bounds_.expand(Point2(p.x, p.y));
    }

    const double columns = std::max(1.0, std::ceil(terrain.bounds_.width() / options.tileSize));
    const double rows = std::max(1.0, std::ceil(terrain.bounds_.height() / options.tileSize));
    if (columns * rows > static_cast<double>(kMaxTileCount)) {
        return makeError(ErrorCode::InvalidArgument,
                         "tileSize is too small for the extent of the points",
                         "tiles=" + std::to_string(columns * rows) +
                             " limit=" + std::to_string(kMaxTileCount));
    }
    terrain.columns_ = static_cast<std::size_t>(columns);
    terrain.rows_ = static_cast<std::size_t>(rows);
    terrain.tiles_.resize(terrain.tileCount());

    // Counting sort of the point indices by tile; ascending within each tile.
    const auto tileOf = [&terrain](const Point3& p) {
        return *terrain.tileAt(Point2(p.x, p.y));
    };
    terrain.tilePointStart_.assign(terrain.tileCount() + 1, 0);
    for (const Point3& p : terrain.points_) {
        ++terrain.tilePointStart_[tileOf(p) + 1];
    }
    for (std::size_t tile = 0; tile < terrain.tileCount(); ++tile) {
        terrain.tilePointStart_[tile + 1] += terrain.tilePointStart_[tile];
    }
    terrain.tilePoints_.resize(terrain.points_.size());
    std::vector<std::uint32_t> cursor(terrain.tilePointStart_.begin(),
                                      terrain.tilePointStart_.end() - 1);
    for (std::size_t i = 0; i < terrain.points_.size(); ++i) {
        terrain.tilePoints_[cursor[tileOf(terrain.points_[i])]++] = static_cast<std::uint32_t>(i);
    }
    return terrain;
}

Box2 TiledTerrain::tileBounds(std::size_t tile) const
{
    if (tile >= tileCount()) {
        throw std::out_of_range("TiledTerrain: tile index out of range");
    }
    const auto column = static_cast<double>(tile % columns_);
    const auto row = static_cast<double>(tile / columns_);
    const Point2 lower(bounds_.min.x + column * options_.tileSize,
                       bounds_.min.y + row * options_.tileSize);
    return Box2(lower, Point2(lower.x + options_.tileSize, lower.y + options_.tileSize));
}

Box2 TiledTerrain::bufferedTileBounds(std::size_t tile) const
{
    return tileBounds(tile).inflated(options_.bufferWidth);
}

std::optional<std::size_t> TiledTerrain::tileAt(const Point2& position) const
{
    if (!position.isFinite() || !bounds_.contains(position)) {
        return std::nullopt;
    }
    return tileCoordinate(position.y, bounds_.min.y, options_.tileSize, rows_) * columns_ +
           tileCoordinate(position.x, bounds_.min.x, options_.tileSize, columns_);
}

Result<TinSurface> TiledTerrain::computeTile(std::size_t tile) const
{
    const Box2 buffered = bufferedTileBounds(tile); // throws for a bad index

    // Candidate buckets: every tile the buffered box reaches into. The box test
    // on each point then decides, so the selection does not depend on how the
    // buckets are laid out.
    const std::size_t column0 =
        tileCoordinate(buffered.min.x, bounds_.min.x, options_.tileSize, columns_);
    const std::size_t column1 =
        tileCoordinate(buffered.max.x, bounds_.min.x, options_.tileSize, columns_);
    const std::size_t row0 = tileCoordinate(buffered.min.y, bounds_.min.y, options_.tileSize, rows_);
    const std::size_t row1 = tileCoordinate(buffered.max.y, bounds_.min.y, options_.tileSize, rows_);

    std::vector<std::uint32_t> selected;
    for (std::size_t row = row0; row <= row1; ++row) {
        for (std::size_t column = column0; column <= column1; ++column) {
            const std::size_t bucket = row * columns_ + column;
            for (std::uint32_t i = tilePointStart_[bucket]; i < tilePointStart_[bucket + 1]; ++i) {
                const Point3& p = points_[tilePoints_[i]];
                if (buffered.contains(Point2(p.x, p.y))) {
                    selected.push_back(tilePoints_[i]);
                }
            }
        }
    }
    // Input order, so that the tile's TIN is a function of the points alone.
    std::sort(selected.begin(), selected.end());

    TinInput input;
    input.points.reserve(selected.size());
    for (const std::uint32_t index : selected) {
        input.points.push_back(points_[index]);
    }
    auto built = buildTin(input, options_.build);
    if (!built) {
        if (built.error().code == ErrorCode::TriangulationFailure) {
            return TinSurface{}; // too few or collinear points: no surface here
        }
        return built.error();
    }
    return std::move(built->surface);
}

Status TiledTerrain::buildTile(std::size_t tile)
{
    if (tile >= tileCount()) {
        return makeError(ErrorCode::InvalidArgument, "tile index out of range",
                         "tile=" + std::to_string(tile));
    }
    if (tiles_[tile].built) {
        return {};
    }
    auto surface = computeTile(tile);
    if (!surface) {
        return surface.error();
    }
    tiles_[tile].surface = std::move(*surface);
    tiles_[tile].built = true;
    return {};
}

Status TiledTerrain::buildAll()
{
    const std::size_t count = tileCount();
    // Tiles are independent by construction: computeTile() is const and reads
    // only the points and the options, and buildTile() writes nothing but
    // tiles_[tile], whose slot already exists - so no two chunks touch the same
    // bytes. One tile per chunk because a tile is a whole triangulation.
    //
    // Each tile's Status is kept in its own slot and the LOWEST-index failure
    // is the one reported, so the error does not depend on which thread lost
    // (Rule 7). The serial version stopped at the first failure and left the
    // tiles after it unbuilt; this one builds them, which is a fact about
    // recovery rather than about the answer - the Status returned is the same,
    // and a caller that cares asks isBuilt().
    std::vector<Status> failures(count);
    katana::core::TaskPool::shared().parallelRanges(0, count, 1, [&](std::size_t lo,
                                                                    std::size_t hi) {
        for (std::size_t tile = lo; tile < hi; ++tile) {
            failures[tile] = buildTile(tile);
        }
    });
    for (std::size_t tile = 0; tile < count; ++tile) {
        if (!failures[tile]) {
            return failures[tile];
        }
    }
    return {};
}

Status TiledTerrain::ensureTileAt(const Point2& position)
{
    const auto tile = tileAt(position);
    return tile ? buildTile(*tile) : Status{};
}

void TiledTerrain::evictTile(std::size_t tile)
{
    if (tile < tileCount()) {
        tiles_[tile] = TileSlot{};
    }
}

bool TiledTerrain::isBuilt(std::size_t tile) const
{
    return tile < tileCount() && tiles_[tile].built;
}

const TinSurface* TiledTerrain::tileSurface(std::size_t tile) const
{
    return isBuilt(tile) ? &tiles_[tile].surface : nullptr;
}

Result<TileSample> TiledTerrain::sampleAt(const Point2& position) const
{
    const auto tile = tileAt(position);
    if (!tile) {
        return TileSample{std::nullopt, true}; // outside the data extent: certainly no surface
    }
    if (!tiles_[*tile].built) {
        return makeError(ErrorCode::InvalidState,
                         "the tile answering this position has not been built",
                         "tile=" + std::to_string(*tile));
    }
    const TinSurface& surface = tiles_[*tile].surface;
    const auto location = surface.locate(position);
    if (!location) {
        return TileSample{std::nullopt, false};
    }
    const TinTriangle& tri = surface.triangles()[location->triangle];
    TileSample sample;
    sample.elevation = location->weights[0] * surface.vertices()[tri[0]].z +
                       location->weights[1] * surface.vertices()[tri[1]].z +
                       location->weights[2] * surface.vertices()[tri[2]].z;

    // Certificate: no part of the data extent outside the buffered tile lies
    // inside the triangle's circumcircle (see the header). The circle is grown
    // by the geometric tolerance to cover the rounding of its construction.
    if (const auto circle = surface.planTriangle(location->triangle).circumcircle()) {
        const Box2 buffered = bufferedTileBounds(*tile);
        const double radius = circle->radius * (1.0 + tol::kRelative) + tol::kGeometric;
        const Point2& c = circle->center;
        sample.certified = (c.x - radius >= buffered.min.x || buffered.min.x <= bounds_.min.x) &&
                           (c.x + radius <= buffered.max.x || buffered.max.x >= bounds_.max.x) &&
                           (c.y - radius >= buffered.min.y || buffered.min.y <= bounds_.min.y) &&
                           (c.y + radius <= buffered.max.y || buffered.max.y >= bounds_.max.y);
    }
    return sample;
}

Result<std::optional<double>> TiledTerrain::elevationAt(const Point2& position) const
{
    auto sample = sampleAt(position);
    if (!sample) {
        return sample.error();
    }
    return sample->elevation;
}

} // namespace katana::terrain
