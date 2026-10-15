#pragma once

// Internal to the surface edits (tin_edit.hpp; docs/surface_editing.md, "One
// way to change a triangulation"): how part of a surface is re-triangulated
// while the rest stays exactly as it was.
//
// An edit names a REGION - the triangles it must rebuild - and what goes into
// it: points, new breakline segments, the edges of a cut. The region's
// vertices and the new points are triangulated by the one constrained
// Delaunay kernel (cdt_backend.hpp) with the ring of edges around the region
// as constraints, so the new triangles meet the untouched ones edge for
// edge; the triangles inside the ring replace the region. Which triangles
// are inside is decided topologically - a triangle on the left of a ring
// edge as the region's own triangle ran it is inside, and inside spreads
// across every edge that is not a ring edge - so no point-in-polygon test
// with a tolerance decides it.
//
// If something the kernel must make reaches the ring (a vertex of the surface
// lying exactly on a ring edge, a new line leaving the region), the ring edge
// is split and the neighbour across it could no longer be met; the region
// then grows by that neighbour and the attempt is made again, so the region
// is never larger than the edit needs.

#include <cstddef>
#include <cstdint>
#include <functional>
#include <optional>
#include <span>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_edit.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::terrain::detail {

// The triangles around each vertex, ascending (two flat arrays).
class VertexStars {
  public:
    explicit VertexStars(const TinSurface& surface);
    [[nodiscard]] std::span<const std::uint32_t> of(std::uint32_t vertex) const;

  private:
    std::vector<std::uint32_t> start_;
    std::vector<std::uint32_t> triangles_;
};

// Triangles by plan extent in a uniform grid, for "which triangles does this
// segment reach?" without a pass over every triangle per segment.
class TriangleGrid {
  public:
    explicit TriangleGrid(const TinSurface& surface);
    // Every triangle the segment from `a` to `b` reaches, or comes within a
    // rounding margin of, is marked in `marks` (one per triangle). The test
    // is conservative: it may mark a triangle the segment only passes near,
    // never miss one it crosses - a region a little too large costs a few
    // triangles re-triangulated, one too small is caught and grown anyway.
    void markAlong(const Point2& a, const Point2& b, std::vector<std::uint8_t>& marks) const;

  private:
    const TinSurface* surface_ = nullptr;
    Point2 origin_;
    double cellWidth_ = 1.0;
    double cellHeight_ = 1.0;
    std::uint32_t columns_ = 1;
    std::uint32_t rows_ = 1;
    std::vector<std::uint32_t> cellStart_;
    std::vector<std::uint32_t> cellTriangles_;
};

// Where a plan position lies on a surface, decided exactly.
struct Place {
    enum class Kind { Outside, Vertex, Edge, Inside };
    Kind kind = Kind::Outside;
    std::uint32_t triangle = kNoTriangle; // Edge, Inside
    std::uint32_t edge = 0;               // Edge
    std::uint32_t vertex = kNoVertex;     // Vertex
};
// Vertex when a vertex lies within tolerance::kGeometric (the nearest; the
// rule buildTin merges by); else Inside or Edge of a triangle by the exact
// orientation of the position against its edges; else Edge of a rim edge
// the position lies within kGeometric beyond (TinSurface::locate counts that
// as on the surface, and there is one definition of "on the surface"); else
// Outside.
[[nodiscard]] Place placeOf(const TinSurface& surface, const VertexStars& stars,
                            const Point2& position);

// The triangles an incremental Delaunay insertion of `position` (placed at
// `place`, Inside or Edge) would rebuild, marked in `region`: the triangle it
// lies in (both, on an edge), and every triangle reached from there across
// an unconstrained edge whose circumcircle holds the position - the
// Bowyer-Watson cavity, which stops at constrained edges as the kernel's own
// insertion does.
void markInsertionCavity(const TinSurface& surface, const Place& place, const Point2& position,
                         std::vector<std::uint8_t>& region);

// Tags of the constraints of a patch, reported back per edge by the kernel.
inline constexpr std::uint8_t kTagKept = 1;   // an edge of the surface that is constrained
inline constexpr std::uint8_t kTagRegion = 2; // the ring around the region
inline constexpr std::uint8_t kTagLine = 4;   // a new breakline segment
inline constexpr std::uint8_t kTagShape = 8;  // an unconstrained edge held for its shape
inline constexpr std::uint8_t kTagCut = 16;   // an edge of a cut (boundary, hole, seam)

struct PatchPoint {
    Point2 position;
    // None: the surface's level there (draped).
    std::optional<double> z;
    // The point must end up a vertex of the result: a breakline's vertex, a
    // point added. A cut's corner off the surface need not.
    bool required = false;
};

struct PatchSegment {
    std::uint32_t from = 0; // indices into PatchRequest::points
    std::uint32_t to = 0;
    // A line segment with both ends draped follows the surface: its levels
    // are the old edges' where it crosses them. Otherwise it is straight in
    // 3D between its ends' levels.
    bool draped = false;
};

struct PatchRequest {
    // One flag per triangle: rebuilt. Grown as needed; the triangles around
    // a removed vertex, and those the points and segments reach, are added.
    std::vector<std::uint8_t> region;
    // One flag per vertex, or empty: left out of the rebuilt region.
    std::vector<std::uint8_t> removed;
    // The region's unconstrained edges are held too, so the rebuilt region
    // is the old triangles cut, not re-triangulated (a cut keeps the shape).
    bool keepShape = false;
    std::vector<PatchPoint> points;
    std::vector<PatchSegment> lines;
    // Directed: what is inside the cut lies on their left.
    std::vector<PatchSegment> cuts;
    // With cuts: whether the inside or the outside of the cut is kept.
    bool keepInsideCut = true;
    // The cut's own points' levels hold along it, over every other edge (a
    // paste's seam takes the top's levels); otherwise a cut is draped.
    bool cutHasLevels = false;
    // The cuts make ONE simple ring (a boundary or a hole): refused when it
    // visits a vertex twice, which a corner shared by two lobes does without
    // any edge crossing another. A paste's seam is the top's rim, which may
    // touch itself, so it leaves this off.
    bool simpleCut = false;
    // An added point at an existing vertex moves it there too (a paste's
    // seam lies on the top's rim, not on the base's vertex beside it).
    bool pointsMoveVertices = false;
    // With cuts: whether a position is inside the cut, for the triangles
    // outside the region and for any piece of the region no cut edge
    // reaches. Asked of a centroid, well away from any cut edge.
    std::function<bool(const Point2&)> insideCut;
};

struct PatchOutput {
    // The surface's vertices (levels and positions updated), then the new
    // ones. Vertices left without a triangle are still here: finishSurface
    // drops them.
    std::vector<Point3> vertices;
    std::vector<TinTriangle> triangles;
    std::vector<std::uint8_t> masks;
    // Per request point, its vertex.
    std::vector<std::uint32_t> pointVertex;
    // Per cut segment, the vertices along it from `from` to `to` (empty for
    // a segment of no length).
    std::vector<std::vector<std::uint32_t>> cutVertices;
    std::size_t relevelled = 0;
    std::size_t crossings = 0;
    std::size_t newVertices = 0;
};

[[nodiscard]] katana::core::Result<PatchOutput>
rebuildPatch(const TinSurface& surface, const VertexStars& stars, PatchRequest request);

// The triangles that join points off the surface to its rim: the surface's
// rim vertices and `points` triangulated with the rim edges as constraints,
// keeping the triangles outside the surface that have one of the points as
// a corner. The points must all be off the surface (placeOf Outside).
[[nodiscard]] katana::core::Result<PatchOutput> extendSurface(const TinSurface& surface,
                                                              std::span<const Point3> points);

// The arrays made a surface: flags made to agree across every shared edge
// (a new constrained edge along an old one marks both sides), vertices no
// triangle uses dropped, triangles rotated and sorted, and the result
// validated by TinSurface::create. InvalidState when no triangle is left;
// TriangulationFailure for a vertex left without a level.
[[nodiscard]] katana::core::Result<TinSurface>
finishSurface(std::vector<Point3> vertices, std::vector<TinTriangle> triangles,
              std::vector<std::uint8_t> masks);

// The report every edit fills the same way from the surfaces before and
// after.
[[nodiscard]] TinEditReport reportOf(const TinSurface& before, const TinSurface& after);

// The surface as it was with an empty report: what an edit that changes
// nothing returns, not a reordered copy of it (finishSurface sorts, and a
// surface imported from an archive is not in that order, so an edit that
// removed nothing would otherwise look like a change and record an undo step).
[[nodiscard]] TinEditResult unchanged(const TinSurface& surface);

// A plan position as a message gives it: "(x, y)" with every digit.
[[nodiscard]] std::string describePosition(const Point2& position);

// Whether a coordinate or level is within kMostMagnitude (tin_edit.hpp), and
// the refusal for one that is not, naming `where`.
[[nodiscard]] bool withinMagnitude(double value);
[[nodiscard]] katana::core::Error tooLarge(const std::string& where);

} // namespace katana::terrain::detail
