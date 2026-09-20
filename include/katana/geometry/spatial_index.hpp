#pragma once

// Broad-phase spatial index over 2D bounding boxes (PLAN.MD Phase 18).
//
// WHY THIS EXISTS, measured rather than assumed (Rule 6). Snapping, picking and
// box selection each walked every entity in the model. In Release, on 4-vertex
// strings scattered over a 1 km square:
//
//     100 000 entities    4.3 ms per mouse move   (27% of the 16 ms budget)
//     250 000 entities   11.6 ms                  (73%)
//     500 000 entities   23.9 ms                  (over budget, before drawing)
//
// A quarter-million-entity as-built is ordinary, so the linear scan runs out
// before the drawings do.
//
// WHAT THIS IS: a sparse spatial hash grid with an overflow list for boxes too
// large to bucket usefully. Chosen over the alternatives for these reasons:
//
//   * R-tree - better on pathological size distributions and the textbook
//     answer for polygon queries, but insertion, splitting and rebalancing are
//     substantially more code to get right, and every one of those paths has
//     to stay correct under the constant incremental edits a CAD document
//     makes. Recorded as the upgrade path: if `oversizedCount()` or the mean
//     bucket occupancy grows in practice, measure and reconsider.
//   * Uniform dense grid - O(1) but allocates for the whole extent, so one
//     entity a kilometre from the rest costs a million empty cells. The hash
//     makes occupancy sparse, which is what real drawings are.
//   * KD-tree / BVH - excellent for a STATIC set, but both want rebuilding
//     after edits, and this index is mutated on every command.
//
// The grid gives O(1) insert, remove and update, which is what keeps it honest
// while the user is drawing.
//
// DETERMINISM (Rule 7). Query results are sorted ascending by id before being
// returned, so they never depend on hash iteration order. Callers rely on this:
// `pickEntity` resolves ties by preferring the higher id, and that rule is only
// meaningful if the candidate order is fixed.
//
// This is a BROAD phase. A query returns every id whose BOX overlaps, which is
// a superset of the true answer; the caller still does the exact geometric
// test. That is the division of labour that keeps this file free of any
// knowledge of what the boxes contain.

#include <cstddef>
#include <cstdint>
#include <span>
#include <unordered_map>
#include <utility>
#include <vector>

#include "katana/geometry/primitives2d.hpp"

namespace katana::geometry {

// Opaque payload. Entities pass their EntityId; nothing here interprets it.
using SpatialId = std::uint64_t;

struct SpatialEntry {
    SpatialId id = 0;
    Box2 box;
};

class SpatialIndex {
  public:
    SpatialIndex() = default;

    // Discards everything, keeping the allocated buckets for reuse.
    void clear();

    // Inserts or replaces `id`. An empty or non-finite box removes it instead:
    // an entity with no extent cannot be found by a spatial query, and keeping
    // it under some arbitrary cell would make queries return it at random.
    void insert(SpatialId id, const Box2& box);
    // True when the id was present.
    bool remove(SpatialId id);
    [[nodiscard]] bool contains(SpatialId id) const;

    // Rebuilds from scratch, choosing the cell size from the data. Far cheaper
    // than the same entries inserted one at a time, and it is what a load or a
    // large import should use.
    void rebuild(std::span<const SpatialEntry> entries);

    [[nodiscard]] std::size_t size() const { return boxes_.size(); }
    [[nodiscard]] bool empty() const { return boxes_.empty(); }
    [[nodiscard]] double cellSize() const { return cellSize_; }
    // Entries too large for the grid, scanned by every query. A large fraction
    // here means the cell size is wrong for the data, or that an R-tree is
    // warranted; exposed so that can be seen rather than guessed.
    [[nodiscard]] std::size_t oversizedCount() const { return oversized_.size(); }
    [[nodiscard]] std::size_t bucketCount() const { return cells_.size(); }
    // Union of every indexed box.
    [[nodiscard]] Box2 bounds() const { return bounds_; }

    // ---- queries ----------------------------------------------------------
    //
    // `out` is cleared first, then filled with ids ascending. Taking the vector
    // by reference lets a caller in a mouse-move path reuse one buffer instead
    // of allocating per frame (PLAN.MD section 33).

    void query(const Box2& box, std::vector<SpatialId>& out) const;
    void query(const Point2& point, double radius, std::vector<SpatialId>& out) const;

    // Convenience for tests and cold paths.
    [[nodiscard]] std::vector<SpatialId> query(const Box2& box) const;

  private:
    // Grid coordinates of a cell. Packed into one 64-bit key so the hash map
    // needs no custom hasher for a pair.
    using CellKey = std::uint64_t;

    [[nodiscard]] static CellKey keyOf(std::int32_t x, std::int32_t y);
    [[nodiscard]] std::int32_t cellOf(double coordinate) const;
    // Cells the box covers, clamped so a huge box cannot ask for billions.
    // Returns false when the box should go to the oversized list instead.
    [[nodiscard]] bool cellRange(const Box2& box, std::int32_t& x0, std::int32_t& y0,
                                 std::int32_t& x1, std::int32_t& y1) const;

    void insertIntoCells(SpatialId id, const Box2& box);
    void removeFromCells(SpatialId id, const Box2& box);
    void chooseCellSize(std::span<const SpatialEntry> entries);

    // Beyond this many cells a box is treated as oversized and kept in a list
    // that every query scans. 32 x 32 is generous: a box covering more cells
    // than that is being indexed by a grid that does not suit it, and listing
    // it costs less than writing it into a thousand buckets.
    static constexpr std::int64_t kMaximumCellsPerBox = 1024;

    double cellSize_ = 1.0;
    std::unordered_map<CellKey, std::vector<SpatialId>> cells_;
    std::unordered_map<SpatialId, Box2> boxes_; // every indexed id, for removal
    std::vector<SpatialId> oversized_;
    Box2 bounds_;
};

} // namespace katana::geometry
