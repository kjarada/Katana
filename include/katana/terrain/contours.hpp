#pragma once

// Contour lines of a TinSurface (PLAN.MD Phase 14).
//
// Definition. The contour at level L is the boundary of the region { z >= L }
// inside the surface. A vertex counts as "above" when z >= L (a vertex exactly
// at the level is treated as infinitesimally above it), which settles every
// degenerate case without special handling:
//   * a level through a vertex gives one polyline through it - no duplicate or
//     zero-length pieces and no gaps;
//   * an edge lying in the level is traced once, as the rim of the higher side;
//   * a flat area (plateau) at the level is outlined where the ground falls away
//     from it and is not filled with lines; if ground rises from it on all sides
//     (a flat pit floor) or the whole surface is at the level there is no contour;
//   * a summit exactly at the level would give a single point; it is dropped.
//
// Contours are traced triangle to triangle through the surface adjacency, so the
// pieces join exactly (no endpoint matching). Open contours begin and end on the
// rim of the surface (outer edge or a hole); the others are closed rings.
// Higher ground is on the left of the direction of travel, hence a closed
// contour around a hill is counter-clockwise (positive signed area) and one
// around a depression clockwise.
//
// Order: levels ascending; within a level the open contours first, then the
// rings, each group by the index of the triangle where tracing starts.

#include <cstddef>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::terrain {

struct Contour {
    double elevation = 0.0;
    bool major = false;                // index contour (see contours())
    katana::geometry::Polyline2 line; // closed = ring
};

// Refuses requests for more levels than this (InvalidArgument): an interval far
// too small for the relief is a unit mistake, not a wish for millions of lines.
inline constexpr std::size_t kMaxContourLevels = 100000;

// Contours of one level. Fails with InvalidArgument for a non-finite elevation.
// A level outside the elevation range gives no contours.
[[nodiscard]] katana::core::Result<std::vector<Contour>> contourAt(const TinSurface& surface,
                                                                   double elevation);

// Contours at base + k * interval for every integer k with a level inside the
// elevation range. Levels with k divisible by `majorEvery` are flagged major
// (majorEvery == 0: none). Fails with InvalidArgument when interval is not
// finite and larger than tolerance::kGeometric, base is not finite, or more than
// kMaxContourLevels levels would be needed.
//
// Levels are traced in parallel on `pool` (null: TaskPool::shared()), each
// into its own list, and the lists are joined in level order: the result is
// identical at any thread count, TaskPool(0) included (Rule 7).
[[nodiscard]] katana::core::Result<std::vector<Contour>>
contours(const TinSurface& surface, double interval, double base = 0.0,
         std::size_t majorEvery = 5, katana::core::TaskPool* pool = nullptr);

} // namespace katana::terrain
