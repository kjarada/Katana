#pragma once

// Merging of coincident plan positions.
//
// PointMerger hands out one index per distinct position: a position within
// `tolerance` (Euclidean, inclusive) of an already kept position gets that
// position's index, otherwise it is kept and gets the next index. When several
// kept positions qualify the earliest wins, so the result depends only on the
// order of the add() calls. Coincidence is not transitive; "compare against the
// kept positions" is the rule that makes the outcome well defined.
//
// Implementation: open-addressing hash grid sized once for the expected number
// of positions (8 bytes per position, no per-position allocation). Expected O(1)
// per add().

#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/geometry/primitives2d.hpp"

namespace katana::terrain::detail {

class PointMerger {
  public:
    struct Entry {
        std::uint32_t index = 0;
        bool inserted = false; // false: merged into an earlier position
    };

    // `capacity` is an upper bound on the number of add() calls. `tolerance` is
    // in model units; 0 merges exactly equal positions only.
    PointMerger(std::size_t capacity, double tolerance);

    Entry add(const katana::geometry::Point2& position);

    [[nodiscard]] const std::vector<katana::geometry::Point2>& positions() const
    {
        return positions_;
    }

  private:
    struct Cell {
        std::int64_t x = 0;
        std::int64_t y = 0;
    };

    [[nodiscard]] std::size_t slotOf(const Cell& cell) const;
    // Earliest kept position within tolerance found in the probe sequence of `cell`.
    [[nodiscard]] std::uint32_t findNear(const Cell& cell,
                                         const katana::geometry::Point2& position) const;

    double tolerance_ = 0.0;
    double cellSize_ = 1.0;
    std::size_t mask_ = 0;
    std::vector<std::uint32_t> slots_;
    std::vector<katana::geometry::Point2> positions_;
};

} // namespace katana::terrain::detail
