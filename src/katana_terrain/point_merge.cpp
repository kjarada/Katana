#include "point_merge.hpp"

#include <algorithm>
#include <cmath>

namespace katana::terrain::detail {

using katana::geometry::Point2;

namespace {

constexpr std::uint32_t kEmptySlot = 0xFFFFFFFFu;

// Cells are this many tolerances wide. A position then lies within reach of a
// neighbouring cell only rarely, so most add() calls probe one cell instead of
// nine. A performance parameter: it has no influence on the result.
constexpr double kCellWidthInTolerances = 1024.0;

// Cell coordinates are clamped to this magnitude so that the conversion to
// int64 and the +-1 neighbour step cannot overflow. Positions beyond it share
// cells, which costs speed but not correctness: the distance test decides.
constexpr double kMaxCellCoordinate = 4.0e18;

std::int64_t cellCoordinate(double scaled)
{
    return static_cast<std::int64_t>(
        std::clamp(std::floor(scaled), -kMaxCellCoordinate, kMaxCellCoordinate));
}

std::size_t tableSizeFor(std::size_t capacity)
{
    std::size_t size = 16;
    while (size < 2 * capacity) {
        size *= 2;
    }
    return size;
}

} // namespace

PointMerger::PointMerger(std::size_t capacity, double tolerance)
    : tolerance_(std::max(tolerance, 0.0)),
      cellSize_(kCellWidthInTolerances *
                std::max(tolerance_, katana::math::tolerance::kGeometric)),
      mask_(tableSizeFor(capacity) - 1), slots_(mask_ + 1, kEmptySlot)
{
    positions_.reserve(capacity);
}

std::size_t PointMerger::slotOf(const Cell& cell) const
{
    // splitmix64 finaliser over the combined cell coordinates.
    std::uint64_t h = static_cast<std::uint64_t>(cell.x) * 0x9E3779B97F4A7C15ULL +
                      static_cast<std::uint64_t>(cell.y);
    h ^= h >> 30;
    h *= 0xBF58476D1CE4E5B9ULL;
    h ^= h >> 27;
    h *= 0x94D049BB133111EBULL;
    h ^= h >> 31;
    return static_cast<std::size_t>(h) & mask_;
}

std::uint32_t PointMerger::findNear(const Cell& cell, const Point2& position) const
{
    std::uint32_t best = kEmptySlot;
    for (std::size_t slot = slotOf(cell); slots_[slot] != kEmptySlot; slot = (slot + 1) & mask_) {
        const std::uint32_t candidate = slots_[slot];
        const Point2& kept = positions_[candidate];
        // Cheap rejection before the hypot in distanceTo().
        if (std::abs(kept.x - position.x) > tolerance_ ||
            std::abs(kept.y - position.y) > tolerance_) {
            continue;
        }
        if (kept.distanceTo(position) <= tolerance_) {
            best = std::min(best, candidate);
        }
    }
    return best;
}

PointMerger::Entry PointMerger::add(const Point2& position)
{
    const double scaledX = position.x / cellSize_;
    const double scaledY = position.y / cellSize_;
    const Cell home{cellCoordinate(scaledX), cellCoordinate(scaledY)};

    std::uint32_t found = findNear(home, position);
    if (tolerance_ > 0.0) {
        // A neighbouring cell can hold a match only when the position is within
        // `tolerance` of the shared border; twice that is tested to absorb the
        // rounding of the scaled coordinates.
        const double reach = 2.0 * tolerance_ / cellSize_;
        const double fractionX = scaledX - std::floor(scaledX);
        const double fractionY = scaledY - std::floor(scaledY);
        const bool lowX = fractionX <= reach;
        const bool highX = 1.0 - fractionX <= reach;
        const bool lowY = fractionY <= reach;
        const bool highY = 1.0 - fractionY <= reach;
        for (int dy = -1; dy <= 1; ++dy) {
            if ((dy < 0 && !lowY) || (dy > 0 && !highY)) {
                continue;
            }
            for (int dx = -1; dx <= 1; ++dx) {
                if ((dx == 0 && dy == 0) || (dx < 0 && !lowX) || (dx > 0 && !highX)) {
                    continue;
                }
                found = std::min(found, findNear(Cell{home.x + dx, home.y + dy}, position));
            }
        }
    }
    if (found != kEmptySlot) {
        return Entry{found, false};
    }

    // Keep the load factor at or below one half even if the caller's capacity
    // estimate was too low.
    if (2 * (positions_.size() + 1) > slots_.size()) {
        mask_ = 2 * slots_.size() - 1;
        slots_.assign(mask_ + 1, kEmptySlot);
        for (std::size_t i = 0; i < positions_.size(); ++i) {
            const Cell cell{cellCoordinate(positions_[i].x / cellSize_),
                            cellCoordinate(positions_[i].y / cellSize_)};
            std::size_t slot = slotOf(cell);
            while (slots_[slot] != kEmptySlot) {
                slot = (slot + 1) & mask_;
            }
            slots_[slot] = static_cast<std::uint32_t>(i);
        }
    }

    const auto index = static_cast<std::uint32_t>(positions_.size());
    positions_.push_back(position);
    std::size_t slot = slotOf(home);
    while (slots_[slot] != kEmptySlot) {
        slot = (slot + 1) & mask_;
    }
    slots_[slot] = index;
    return Entry{index, true};
}

} // namespace katana::terrain::detail
