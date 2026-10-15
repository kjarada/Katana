#pragma once

// Volumes and surface comparison (docs/terrain.md; docs/volumes.md for the
// methods, their sources and the verbs that offer them).
//
// The exact methods integrate a function that is linear over every triangle,
// so each triangle is integrated in closed form; a triangle on which the
// function changes sign is split along its zero line and the two parts are
// accumulated separately. Nothing is sampled: the results are exact up to
// floating-point rounding (sums are compensated). Two surfaces are compared
// on their overlay (overlay.hpp), whose every piece lies inside one triangle
// of each, so their difference is linear on it.
//
// The grid method (gridVolume) is the one sampled method, kept as an
// independent check of the exact ones and because it is what a grid of
// cells - a DEM, a borrow pit - is measured by.

#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/overlay.hpp"
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
    // The plan area measured: the surface's, or the part of it inside a
    // region. planAreaAbove + planAreaBelow, less where the surface is
    // exactly at the datum.
    double planArea = 0.0;
};

// Fails with InvalidArgument for a non-finite datum or an empty surface.
[[nodiscard]] katana::core::Result<DatumVolume> volumeToDatum(const TinSurface& surface,
                                                              double datum);

// The same over the part of the surface inside `region`: the surface compared
// with the region made flat at the datum (OverlayRegion::atLevel), so the
// region's edges cut the surface's triangles exactly. A region partly off the
// surface measures the part on it, which planArea says.
[[nodiscard]] katana::core::Result<DatumVolume>
volumeToDatum(const TinSurface& surface, double datum, const OverlayRegion& region,
              katana::core::TaskPool* pool = nullptr);

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

// Exact, on the overlay of the two (overlay.hpp): each triangle of the
// existing surface is clipped against the design triangles it meets, and
// each piece - inside one triangle of each, so zDesign - zExisting is linear
// over it - is integrated in closed form. Surfaces that do not overlap give a
// result with planArea == 0. Fails with InvalidArgument for an empty surface.
//
// Runs on `pool` (null: TaskPool::shared()). The overlay's pieces arrive in
// blocks of a fixed number of existing triangles; each block is summed in its
// fixed order and the block sums are combined in block order, so the result
// is the same bits at any thread count - TaskPool(0) included (CLAUDE.md
// section 5). Safe to call from a background job: both surfaces are only read.
[[nodiscard]] katana::core::Result<SurfaceComparison>
compareSurfaces(const TinSurface& existing, const TinSurface& design,
                katana::core::TaskPool* pool = nullptr);

// The same over the part of the common area inside `region`: the overlay cut
// again by the region's triangles, so the polygon's edges are followed
// exactly, not by whole triangles.
[[nodiscard]] katana::core::Result<SurfaceComparison>
compareSurfaces(const TinSurface& existing, const TinSurface& design, const OverlayRegion& region,
                katana::core::TaskPool* pool = nullptr);

// ---- stockpiles and pits ------------------------------------------------------------------

// What a stockpile (or a pit) is measured against: a base made from its
// boundary.
enum class StockpileBase {
    // The boundary's heights, triangulated inside it: the toe of the pile as
    // it stands. The boundary's own heights when every vertex has one,
    // otherwise the surface's at each vertex.
    Boundary,
    // A level plane at the lowest point of the surface along the boundary -
    // its vertices and every place an edge of the surface crosses it.
    Lowest,
    // A level plane at a given height.
    Level,
};

struct StockpileOptions {
    StockpileBase base = StockpileBase::Boundary;
    double level = 0.0; // StockpileBase::Level
    // StockpileBase::Boundary: the boundary's own height at each of its
    // vertices, when it has one (a 3D string surveyed along the toe). Used
    // only when every vertex has one; otherwise ignored for the surface's.
    std::vector<std::optional<double>> boundaryHeights{};
};

struct StockpileVolume {
    // above: the pile, the surface above its base (what is cut to remove
    // it); below: a pit, the surface below its base (what is filled).
    DatumVolume volume{};
    StockpileBase base = StockpileBase::Boundary;
    // Lowest and Level: the height of the base.
    std::optional<double> level{};
    // Boundary: whether the heights were the boundary's own.
    bool boundaryHeights = false;
    // The boundary's plan area; volume.planArea is the part of it on the
    // surface.
    double boundaryArea = 0.0;
};

// Fails with InvalidArgument for an empty surface or a non-finite level;
// InvalidGeometry for a boundary that is no polygon (OverlayRegion::fromPolygon),
// a Boundary base whose boundary has a vertex off the surface and no height of
// its own (naming the vertex: absent is not zero), and a Lowest base whose
// boundary never crosses the surface.
[[nodiscard]] katana::core::Result<StockpileVolume>
stockpileVolume(const TinSurface& surface, const katana::geometry::Polyline2& boundary,
                const StockpileOptions& options, katana::core::TaskPool* pool = nullptr);

// ---- the grid method -----------------------------------------------------------------------

// A cross-check of the exact methods, and the way a grid of cells is
// measured: square cells of side `cell`, on lines at whole multiples of the
// cell from the origin (so grids of one cell size line up, as RASTER GRID's
// do), each counted when its centre is on both surfaces - and inside the
// region, when one is given - as a prism of the cell's area and the
// difference at its centre (the midpoint rule). Converges on the exact
// answer as the cell shrinks; on a surface whose ridges run through cells it
// is out by a known amount (docs/volumes.md, "The grid method").
//
// A centre on a polygon's edge is inside it (OverlayRegion::contains), so a
// cell centred on an edge two polygons share belongs to both: lots measured
// one at a time and summed would count that line of cells twice (+432 m3 of
// cut on a site of four quadrants). Measuring several polygons, each is given
// the polygons before it as `claimed`, and a cell any of them holds is left
// to the first that did: the cells counted, summed over the polygons, are the
// cells of their union, each once.
struct GridVolume {
    double cut = 0.0;  // existing above design, as SurfaceComparison
    double fill = 0.0;
    double net = 0.0;  // fill - cut
    double planArea = 0.0; // cells counted x cell area
    std::size_t cells = 0;
    double cell = 0.0;
};

// The grid of cells is capped, as a surface's raster export caps its grid
// (interop/terrain_io.hpp, maxCells): more cells than this is a unit mistake,
// and the refusal names the cell that would fit.
inline constexpr std::uint64_t kMaxGridVolumeCells = 25'000'000;

// InvalidArgument for an empty surface, a cell that is not finite and
// positive, or a grid of more than kMaxGridVolumeCells. Surfaces with no
// ground in common count no cells. The same bits at any thread count: rows of
// cells are summed in fixed blocks combined in order.
//
// `claimed`: regions whose cells are already counted (the polygons measured
// before this one). A cell whose centre any of them holds, the boundary
// included, is not counted again. Empty: none.
[[nodiscard]] katana::core::Result<GridVolume>
gridVolume(const TinSurface& existing, const TinSurface& design, double cell,
           const OverlayRegion* region = nullptr, katana::core::TaskPool* pool = nullptr,
           std::span<const OverlayRegion> claimed = {});

} // namespace katana::terrain
