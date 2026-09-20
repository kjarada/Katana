#pragma once

// TIN construction (PLAN.MD Phase 14):
//
//   Survey Points -> Breaklines -> Boundaries -> Triangulation -> TIN -> Surface
//
// buildTin() computes a 2D constrained Delaunay triangulation of the plan
// positions with exact predicates (CGAL, isolated inside src/katana_terrain) and
// returns a TinSurface made of Katana types only. See docs/terrain.md for the
// algorithms and numerical assumptions.
//
// Input rules
//   * Points and breakline vertices carry elevations. Breakline segments become
//     constrained edges: the surface interpolates linearly along them.
//   * Plan positions closer than tolerance::kGeometric are one vertex. What
//     happens to their elevations is decided by DuplicatePointPolicy; every
//     merge is counted in the report.
//   * A survey point lying exactly on a breakline segment splits the segment and
//     keeps its own elevation.
//   * Breaklines that cross in plan view create a vertex at the crossing. Its
//     elevation is interpolated along each breakline; what happens when the
//     breaklines disagree is decided by CrossingBreaklinePolicy.
//   * The boundary is a hard clip: its edges are constrained edges and only
//     triangles inside it are kept. Holes remove the triangles inside them.
//     Boundary and hole vertices are plan positions. One that coincides with a
//     survey point or breakline vertex is that vertex; any other takes the
//     elevation of the unclipped surface at its position, and must therefore lie
//     on it (inside the convex hull of the data).
//   * Rings must be simple polygons (a ring that touches or crosses itself is
//     rejected); different rings may overlap or cross each other.
//
// Output is canonical and deterministic: vertices in order of first appearance
// (points, breakline vertices, boundary vertices, hole vertices, then crossing
// vertices sorted by x, y), every triangle rotated to start at its smallest
// vertex index, triangles sorted lexicographically. Identical input gives
// bitwise-identical output.

#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::terrain {

// A 3D polyline whose segments the triangulation must honour.
struct Breakline {
    std::vector<Point3> vertices;
    bool closed = false; // adds the segment from the last vertex back to the first
};

struct TinInput {
    std::vector<Point3> points;
    std::vector<Breakline> breaklines;
    // Outer boundary; no vertices = none (the convex hull bounds the surface).
    // Always treated as a closed ring, either orientation.
    katana::geometry::Polyline2 boundary;
    // Voids. Always treated as closed rings, either orientation.
    std::vector<katana::geometry::Polyline2> holes;
};

// Elevations of vertices that share a plan position (within kGeometric).
enum class DuplicatePointPolicy {
    // Merge when the elevations agree within `elevationTolerance` (the first is
    // kept); fail with InvalidGeometry otherwise. A TIN cannot represent two
    // elevations at one position, so this never alters data silently.
    ErrorOnConflict,
    KeepFirst, // the first occurrence in input order wins
    Average,   // arithmetic mean of all occurrences
};

// Elevation of the vertex created where two breaklines cross in plan view.
enum class CrossingBreaklinePolicy {
    // Use the mean when the breaklines agree within `elevationTolerance`; fail
    // with InvalidGeometry otherwise.
    ErrorOnConflict,
    Average, // always use the mean of the crossing breaklines
};

struct TinBuildOptions {
    DuplicatePointPolicy duplicatePoints = DuplicatePointPolicy::ErrorOnConflict;
    CrossingBreaklinePolicy crossingBreaklines = CrossingBreaklinePolicy::ErrorOnConflict;
    // Metres. Elevations at one plan position that differ by more than this are
    // in conflict. Default: survey coordinates closer than kCoordinate are the
    // same ground mark.
    double elevationTolerance = katana::math::tolerance::kCoordinate;
};

struct TinBuildReport {
    // Input points merged into an earlier vertex.
    std::size_t duplicatePointCount = 0;
    // Breakline vertices that coincide with an earlier vertex (normal when
    // breaklines are drawn through survey points).
    std::size_t sharedBreaklineVertexCount = 0;
    // Merges whose elevations differed by more than elevationTolerance and were
    // resolved by the policy (always 0 with ErrorOnConflict).
    std::size_t elevationConflictCount = 0;
    // Vertices created where breaklines cross, and how many of those had
    // breakline elevations differing by more than elevationTolerance.
    std::size_t breaklineCrossingCount = 0;
    std::size_t crossingConflictCount = 0;
    // Triangles removed by the boundary and the holes; vertices left without a
    // triangle by that removal (they are not part of the surface).
    std::size_t clippedTriangleCount = 0;
    std::size_t droppedVertexCount = 0;
    // For every input point: its vertex in the surface, or kNoVertex when the
    // point was clipped away.
    std::vector<std::uint32_t> pointVertex;
};

struct TinBuildResult {
    TinSurface surface;
    TinBuildReport report;
};

// Fails with
//   InvalidArgument       non-finite coordinate, invalid option value, too many
//                         vertices for 32-bit indices;
//   InvalidGeometry       breakline with fewer than 2 vertices, degenerate or
//                         self-intersecting ring, ring vertex off the data,
//                         elevation conflict under an ErrorOnConflict policy;
//   TriangulationFailure  fewer than 3 distinct positions, all positions
//                         collinear, nothing left after clipping, or a failure
//                         reported by the triangulation kernel.
[[nodiscard]] katana::core::Result<TinBuildResult> buildTin(const TinInput& input,
                                                            const TinBuildOptions& options = {});

} // namespace katana::terrain
