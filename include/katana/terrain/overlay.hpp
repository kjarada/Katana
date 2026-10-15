#pragma once

// The exact overlay of two surfaces (docs/volumes.md, "One overlay").
//
// Every volume between two surfaces integrates their difference, and the
// difference is linear only where both surfaces are: inside one triangle of
// each. The overlay cuts the ground the two have in common into such pieces.
// A piece is the intersection of a triangle of each surface - both convex, so
// the intersection is a convex polygon of at most six corners - fanned into
// triangles from its first corner, which needs no corner that is not already
// on its boundary. Each piece is handed out with both surfaces' heights at its
// corners, read off the plane of the triangle it came from.
//
// It is what compareSurfaces integrates (volume.hpp), what the volume inside a
// polygon integrates after a third cut by the polygon's own triangles, and what
// the difference and envelope surfaces are made of. One implementation of the
// cut, so every one of them is exact in the same way: up to floating-point
// rounding, nothing sampled.
//
// THE ANSWER DOES NOT DEPEND ON THE THREAD COUNT. The base surface's triangles
// are cut into blocks of a FIXED kOverlayBlockTriangles, and every piece of a
// block is handed out in a fixed order - base triangle ascending, then the top
// triangles it meets in ascending index, then (inside a region) the region's
// triangles in ascending index, then the fan. A visitor that keeps one sum per
// block and combines them in block order therefore gets the same bits on one
// thread or sixteen (CLAUDE.md section 5), which is what compareSurfaces does.
//
// Both surfaces are only read, so an overlay may run on a background job.

#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::terrain {

// A plan area the overlay is cut to: a polygon less its holes, triangulated
// once (a constrained Delaunay triangulation of its own vertices, so its
// triangles tile it exactly and add no point of their own).
class OverlayRegion {
  public:
    OverlayRegion() = default; // no area

    // Either orientation. InvalidGeometry for a ring of fewer than three
    // distinct vertices, a ring that touches or crosses itself, or nothing
    // left once the holes are taken out, naming which.
    [[nodiscard]] static katana::core::Result<OverlayRegion>
    fromPolygon(const katana::geometry::Polyline2& ring,
                const std::vector<katana::geometry::Polyline2>& holes = {});

    // The polygon's triangles, counter-clockwise, tiling it.
    [[nodiscard]] const std::vector<katana::geometry::Triangle2>& triangles() const
    {
        return triangles_;
    }
    // Their plan area (compensated): the polygon's area less its holes'.
    [[nodiscard]] double area() const { return area_; }
    [[nodiscard]] const Box2& bounds() const { return bounds_; }
    [[nodiscard]] bool empty() const { return triangles_.empty(); }
    // Inside the ring and outside every hole; the boundary counts as inside.
    [[nodiscard]] bool contains(const Point2& position) const;
    // The polygon as a flat surface at height `level`: the same triangles,
    // every vertex at that height. What a volume to a level inside a polygon
    // compares the ground with.
    [[nodiscard]] katana::core::Result<TinSurface> atLevel(double level) const;

  private:
    katana::geometry::Polyline2 ring_;
    std::vector<katana::geometry::Polyline2> holes_;
    TinSurface plan_; // the triangulation, every vertex at 0
    std::vector<katana::geometry::Triangle2> triangles_;
    double area_ = 0.0;
    Box2 bounds_;
};

// One piece of the overlay.
struct OverlayPiece {
    std::array<Point2, 3> corners{}; // counter-clockwise
    double area = 0.0;               // its plan area, always > 0
    std::array<double, 3> zBase{};   // the base surface at each corner
    std::array<double, 3> zTop{};    // the top surface at each corner
    std::uint32_t baseTriangle = kNoTriangle;
    std::uint32_t topTriangle = kNoTriangle;
};

// Told every piece. `block` is the block the piece belongs to; the pieces of
// one block arrive on one thread in their fixed order, and different blocks may
// arrive at once on different threads - so a visitor writes only into its own
// slot for `block`.
class OverlayVisitor {
  public:
    virtual ~OverlayVisitor() = default;
    virtual void piece(std::size_t block, const OverlayPiece& piece) = 0;
};

// Base triangles to a block. Fixed: a partition that followed the thread count
// would change the order of the sums with it. 256 triangles is a few hundred
// microseconds of clipping, so dispatch is noise, and a 200 000-triangle
// surface still makes about 800 blocks to balance (docs/terrain.md,
// "compareSurfaces and contours on the TaskPool").
inline constexpr std::size_t kOverlayBlockTriangles = 256;

// The blocks visitOverlay will number, from 0: one per kOverlayBlockTriangles
// of `base`, the last one short.
[[nodiscard]] std::size_t overlayBlockCount(const TinSurface& base);

// Every piece of the overlay of `base` and `top` - inside `region` when one is
// given - handed to `visitor`. Surfaces that do not overlap (or do not reach
// the region) visit nothing and succeed. InvalidArgument for an empty surface.
// Runs on `pool` (null: TaskPool::shared()).
[[nodiscard]] katana::core::Status visitOverlay(const TinSurface& base, const TinSurface& top,
                                                OverlayVisitor& visitor,
                                                const OverlayRegion* region = nullptr,
                                                katana::core::TaskPool* pool = nullptr);

// ---- surfaces made of the overlay --------------------------------------------------------

struct OverlaySurfaceOptions {
    // Build through the constrained triangulation even when the pieces join
    // as they are. The pieces are joined directly when they can be - every
    // corner named by what it is (a vertex of one surface, or where an edge of
    // one crosses an edge of the other), so two pieces that share a corner
    // share its vertex - and the triangulation is what the build falls back
    // to when floating point leaves two pieces overlapping by a sliver.
    // Slower; the same surface up to rounding.
    bool retriangulate = false;
    katana::core::TaskPool* pool = nullptr;
};

struct OverlaySurface {
    TinSurface surface;
    std::size_t pieces = 0;       // overlay pieces it was made from
    bool retriangulated = false;  // made through the constrained triangulation
};

// The difference surface (an isopach): `top` minus `base`, over the ground the
// two have in common, exact - linear on every overlay piece, which is what a
// TIN is. Positive where the top is above the base (fill on a design over the
// ground), negative where it is below (cut). Integrated with volumeToDatum at
// 0, its above and below are compareSurfaces' fill and cut.
//
// InvalidArgument for an empty surface; InvalidGeometry when the two have no
// ground in common; what TinSurface::create or buildTin reports.
[[nodiscard]] katana::core::Result<OverlaySurface>
differenceSurface(const TinSurface& base, const TinSurface& top,
                  const OverlaySurfaceOptions& options = {});

enum class Envelope {
    Upper, // the higher of the two at every point
    Lower, // the lower
};

// The upper or lower envelope of two surfaces over the ground they have in
// common, exact: a piece on which the two cross is split along the line where
// they meet, so each part is one surface's plane. Fails as differenceSurface.
[[nodiscard]] katana::core::Result<OverlaySurface>
envelopeSurface(const TinSurface& first, const TinSurface& second, Envelope which,
                const OverlaySurfaceOptions& options = {});

} // namespace katana::terrain
