#include "katana/geometry/spatial_index.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace katana::geometry {

namespace {

[[nodiscard]] bool usable(const Box2& box)
{
    return !box.empty() && std::isfinite(box.min.x) && std::isfinite(box.min.y) &&
           std::isfinite(box.max.x) && std::isfinite(box.max.y);
}

} // namespace

SpatialIndex::CellKey SpatialIndex::keyOf(std::int32_t x, std::int32_t y)
{
    // Two 32-bit cell coordinates packed into one 64-bit key. Cast through the
    // unsigned type first so a negative coordinate keeps its bit pattern rather
    // than sign-extending over the other half of the key.
    return (static_cast<CellKey>(static_cast<std::uint32_t>(x)) << 32) |
           static_cast<CellKey>(static_cast<std::uint32_t>(y));
}

std::int32_t SpatialIndex::cellOf(double coordinate) const
{
    const double scaled = std::floor(coordinate / cellSize_);
    // Clamped rather than wrapped: a coordinate far enough out to overflow an
    // int32 cell index is not real survey data, and folding it back would make
    // it collide with a cell that is.
    constexpr double kLow = -2.0e9;
    constexpr double kHigh = 2.0e9;
    return static_cast<std::int32_t>(std::clamp(scaled, kLow, kHigh));
}

bool SpatialIndex::cellRange(const Box2& box, std::int32_t& x0, std::int32_t& y0,
                             std::int32_t& x1, std::int32_t& y1) const
{
    x0 = cellOf(box.min.x);
    y0 = cellOf(box.min.y);
    x1 = cellOf(box.max.x);
    y1 = cellOf(box.max.y);
    const std::int64_t columns = static_cast<std::int64_t>(x1) - x0 + 1;
    const std::int64_t rows = static_cast<std::int64_t>(y1) - y0 + 1;
    return columns * rows <= kMaximumCellsPerBox;
}

void SpatialIndex::clear()
{
    // The buckets keep their capacity: a document that is cleared is usually
    // about to be refilled.
    for (auto& [key, ids] : cells_) {
        (void)key;
        ids.clear();
    }
    boxes_.clear();
    oversized_.clear();
    bounds_ = Box2{};
}

bool SpatialIndex::contains(SpatialId id) const { return boxes_.find(id) != boxes_.end(); }

void SpatialIndex::insertIntoCells(SpatialId id, const Box2& box)
{
    std::int32_t x0 = 0;
    std::int32_t y0 = 0;
    std::int32_t x1 = 0;
    std::int32_t y1 = 0;
    if (!cellRange(box, x0, y0, x1, y1)) {
        oversized_.push_back(id);
        return;
    }
    for (std::int32_t y = y0; y <= y1; ++y) {
        for (std::int32_t x = x0; x <= x1; ++x) {
            cells_[keyOf(x, y)].push_back(id);
        }
    }
}

void SpatialIndex::removeFromCells(SpatialId id, const Box2& box)
{
    std::int32_t x0 = 0;
    std::int32_t y0 = 0;
    std::int32_t x1 = 0;
    std::int32_t y1 = 0;
    if (!cellRange(box, x0, y0, x1, y1)) {
        const auto found = std::find(oversized_.begin(), oversized_.end(), id);
        if (found != oversized_.end()) {
            oversized_.erase(found);
        }
        return;
    }
    for (std::int32_t y = y0; y <= y1; ++y) {
        for (std::int32_t x = x0; x <= x1; ++x) {
            const auto cell = cells_.find(keyOf(x, y));
            if (cell == cells_.end()) {
                continue;
            }
            auto& ids = cell->second;
            const auto found = std::find(ids.begin(), ids.end(), id);
            if (found != ids.end()) {
                // Swap with the back: order inside a bucket does not matter
                // because every query sorts, and this avoids shifting the rest.
                *found = ids.back();
                ids.pop_back();
            }
        }
    }
}

void SpatialIndex::insert(SpatialId id, const Box2& box)
{
    const auto existing = boxes_.find(id);
    if (existing != boxes_.end()) {
        if (existing->second == box) {
            return; // an unchanged box costs nothing; moving an entity is common
        }
        removeFromCells(id, existing->second);
        boxes_.erase(existing);
    }
    if (!usable(box)) {
        // An empty or non-finite box is not findable by any spatial query, so
        // it is dropped rather than filed somewhere arbitrary where queries
        // would return it unpredictably. `bounds_` is not shrunk here: see
        // remove().
        return;
    }
    boxes_.emplace(id, box);
    insertIntoCells(id, box);
    bounds_.expand(box);
}

bool SpatialIndex::remove(SpatialId id)
{
    const auto found = boxes_.find(id);
    if (found == boxes_.end()) {
        return false;
    }
    removeFromCells(id, found->second);
    boxes_.erase(found);
    // bounds_ deliberately not recomputed. It is only ever used to frame a
    // view, so an over-large box costs a slightly wider zoom-extents and
    // nothing else, whereas recomputing it would make every delete O(n).
    // rebuild() tightens it.
    return true;
}

void SpatialIndex::chooseCellSize(std::span<const SpatialEntry> entries)
{
    // Cell size is the MEAN box extent, not the extent of the whole data set.
    // Sizing from the extent would give one cell per drawing when the entities
    // are small and many; sizing from the entities means an average query
    // touches a handful of buckets whatever the drawing covers.
    double total = 0.0;
    std::size_t counted = 0;
    for (const SpatialEntry& entry : entries) {
        if (!usable(entry.box)) {
            continue;
        }
        total += entry.box.width() + entry.box.height();
        ++counted;
    }
    if (counted == 0) {
        cellSize_ = 1.0;
        return;
    }
    const double mean = total / (2.0 * static_cast<double>(counted));
    // A degenerate set (every entity a point) gives a mean of zero, which would
    // divide by zero in cellOf. Falling back to 1 model unit keeps the grid
    // usable; such data is uniform anyway, so the exact size hardly matters.
    cellSize_ = mean > 1.0e-9 ? mean * 2.0 : 1.0;
}

void SpatialIndex::rebuild(std::span<const SpatialEntry> entries)
{
    cells_.clear();
    boxes_.clear();
    oversized_.clear();
    bounds_ = Box2{};

    chooseCellSize(entries);
    boxes_.reserve(entries.size());
    cells_.reserve(entries.size());

    for (const SpatialEntry& entry : entries) {
        if (!usable(entry.box)) {
            continue;
        }
        boxes_.emplace(entry.id, entry.box);
        insertIntoCells(entry.id, entry.box);
        bounds_.expand(entry.box);
    }
}

void SpatialIndex::query(const Box2& box, std::vector<SpatialId>& out) const
{
    out.clear();
    if (!usable(box) || boxes_.empty()) {
        return;
    }

    const std::int32_t x0 = cellOf(box.min.x);
    const std::int32_t y0 = cellOf(box.min.y);
    const std::int32_t x1 = cellOf(box.max.x);
    const std::int32_t y1 = cellOf(box.max.y);

    const std::int64_t columns = static_cast<std::int64_t>(x1) - x0 + 1;
    const std::int64_t rows = static_cast<std::int64_t>(y1) - y0 + 1;

    if (columns * rows > kMaximumCellsPerBox) {
        // The query covers more of the grid than it is worth walking - a
        // zoom-extents selection, typically. Every indexed box is cheaper.
        out.reserve(boxes_.size());
        for (const auto& [id, stored] : boxes_) {
            if (stored.intersects(box)) {
                out.push_back(id);
            }
        }
    } else {
        for (std::int32_t y = y0; y <= y1; ++y) {
            for (std::int32_t x = x0; x <= x1; ++x) {
                const auto cell = cells_.find(keyOf(x, y));
                if (cell == cells_.end()) {
                    continue;
                }
                for (const SpatialId id : cell->second) {
                    const auto stored = boxes_.find(id);
                    if (stored != boxes_.end() && stored->second.intersects(box)) {
                        out.push_back(id);
                    }
                }
            }
        }
        // A box spanning several cells is listed in each of them.
        for (const SpatialId id : oversized_) {
            const auto stored = boxes_.find(id);
            if (stored != boxes_.end() && stored->second.intersects(box)) {
                out.push_back(id);
            }
        }
    }

    // Sorted and deduplicated, so the result never depends on hash iteration
    // order and a caller's tie-break rule (pickEntity prefers the higher id)
    // means something.
    std::sort(out.begin(), out.end());
    out.erase(std::unique(out.begin(), out.end()), out.end());
}

void SpatialIndex::query(const Point2& point, double radius, std::vector<SpatialId>& out) const
{
    if (!std::isfinite(radius) || radius < 0.0) {
        out.clear();
        return;
    }
    query(Box2(Point2(point.x - radius, point.y - radius),
               Point2(point.x + radius, point.y + radius)),
          out);
}

std::vector<SpatialId> SpatialIndex::query(const Box2& box) const
{
    std::vector<SpatialId> out;
    query(box, out);
    return out;
}

} // namespace katana::geometry
