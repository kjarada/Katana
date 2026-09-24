#pragma once

// Volumes and surface comparison (PLAN.MD Phase 14: volume, cut/fill, surface
// comparison). Equations and error behaviour are in docs/terrain.md.
//
// Both computations integrate a function that is linear over every triangle, so
// each triangle is integrated in closed form; a triangle on which the function
// changes sign is split along its zero line and the two parts are accumulated
// separately. Nothing is sampled: the results are exact up to floating-point
// rounding (sums are compensated).

#include <cstddef>

#include "katana/core/error.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::terrain {

// Volume between a surface and the horizontal plane z = datum. Cubic metres and
// square metres for metre coordinates.
struct DatumVolume {
    double above = 0.0; // solid between datum and surface where the surface is higher (>= 0)
    double below = 0.0; // solid between surface and datum where the surface is lower (>= 0)
    double net = 0.0;   // above - below = integral of (z - datum) over the plan area
    double planAreaAbove = 0.0;
    double planAreaBelow = 0.0;
    // Earthwork reading for a level design at `datum`: cut = above, fill = below.
};

// Fails with InvalidArgument for a non-finite datum or an empty surface.
[[nodiscard]] katana::core::Result<DatumVolume> volumeToDatum(const TinSurface& surface,
                                                              double datum);

// Earthwork between an existing and a design surface over their common plan area.
struct SurfaceComparison {
    double cut = 0.0;  // where existing is above design: material to remove (>= 0)
    double fill = 0.0; // where design is above existing: material to place (>= 0)
    double net = 0.0;  // fill - cut = integral of (zDesign - zExisting)
    double planArea = 0.0; // common area that was compared
    double cutArea = 0.0;
    double fillArea = 0.0; // planArea - cutArea - fillArea is where the surfaces coincide
    std::size_t overlayTriangleCount = 0; // triangles of the overlay inside the common area
};

// Exact overlay: both triangulations are merged into one constrained
// triangulation (all vertices, all edges as constraints, crossing points
// inserted), so that each overlay triangle lies inside one triangle of each
// surface and zDesign - zExisting is linear over it. Surfaces that do not overlap
// give a result with planArea == 0. Fails with InvalidArgument for an empty
// surface and TriangulationFailure when the overlay cannot be built.
//
// Runs on `pool` (null: TaskPool::shared()). The existing surface is cut into
// blocks of a fixed number of triangles, each summed in order, and the block
// sums are combined in block order, so the result is the same bits at any
// thread count - TaskPool(0) included (Rule 7). Safe to call from a
// background job: both surfaces are only read.
[[nodiscard]] katana::core::Result<SurfaceComparison>
compareSurfaces(const TinSurface& existing, const TinSurface& design,
                katana::core::TaskPool* pool = nullptr);

} // namespace katana::terrain
