#pragma once

// Polygon algorithms (PLAN.MD Phase 04). A polygon is a closed Polyline2.
//
// Not yet provided: boolean set operations (union / intersection / difference
// of arbitrary polygons) and polygons with holes. Clipping is available against
// convex clip regions only.

#include <array>
#include <cstddef>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::geometry {

enum class Orientation { Clockwise, CounterClockwise, Degenerate };

// Degenerate when open, fewer than 3 vertices, or the area is negligible.
[[nodiscard]] Orientation orientation(const Polyline2& polygon);

// True when every turn has the same sign (collinear vertices allowed).
[[nodiscard]] bool isConvex(const Polyline2& polygon);

// True when no two edges meet other than consecutive edges at their shared
// vertex. O(n^2); intended for validation, not for bulk data.
[[nodiscard]] bool isSimple(const Polyline2& polygon);

// Andrew's monotone chain. Counter-clockwise, without collinear points and
// without repeating the first point. Fewer than 3 distinct points are returned
// as they are (deduplicated).
[[nodiscard]] std::vector<Point2> convexHull(std::vector<Point2> points);

// Douglas-Peucker: drops vertices closer than `tolerance` to the simplified
// chain. Endpoints of open polylines are always kept. tolerance <= 0 returns
// the input unchanged.
[[nodiscard]] Polyline2 simplify(const Polyline2& polyline, double tolerance);

// Liang-Barsky. nullopt when the segment lies outside the box. A segment that
// only touches the box boundary yields a zero-length segment.
[[nodiscard]] std::optional<Segment2> clip(const Segment2& segment, const Box2& box);

// Sutherland-Hodgman clipping of `subject` against the convex polygon `clip`
// (either orientation). The result may have no vertices. Fails with
// InvalidGeometry when `clip` is not a convex polygon or `subject` is not closed.
[[nodiscard]] katana::core::Result<Polyline2> clipPolygon(const Polyline2& subject,
                                                          const Polyline2& clip);

using TriangleIndices = std::array<std::size_t, 3>;

// Ear-clipping triangulation of a simple polygon without holes. Indices refer
// to `polygon.vertices`; triangles are counter-clockwise. Vertices collinear
// with their neighbours are not referenced by any triangle. O(n^2).
// Fails with TriangulationFailure for non-simple or degenerate input.
[[nodiscard]] katana::core::Result<std::vector<TriangleIndices>>
triangulate(const Polyline2& polygon);

} // namespace katana::geometry
