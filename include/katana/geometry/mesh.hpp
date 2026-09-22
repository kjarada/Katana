#pragma once

// A mesh of triangles in space (PLAN.MD 20.2, slice 4).
//
// What a 12d `primitive_3d` carries, and what an IFC solid or an OBJ would:
// a list of points and a list of triangles naming three of them each. It is
// NOT a TinSurface - a surface is a function of x and y, single-valued, and
// exists to be sampled for a height; a mesh may be closed, may overhang and
// may have several sheets above one point, so it can only be drawn and
// measured, never sampled. Keeping them separate types is what stops a pipe
// or a wall being asked for "the level at this station".
//
// Indices are into `vertices`, zero-based (12d's file format is one-based;
// the reader has already subtracted).

#include <array>
#include <cstdint>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/primitives3d.hpp"
#include "katana/math/primitives.hpp"

namespace katana::geometry {

struct TriangleMesh {
    std::vector<Point3> vertices;
    std::vector<std::array<std::uint32_t, 3>> faces;

    [[nodiscard]] std::size_t triangleCount() const { return faces.size(); }
    [[nodiscard]] bool empty() const { return faces.empty(); }

    // nullopt for an index past the end or a face naming a vertex that does
    // not exist, so a caller that iterates cannot read out of bounds even if
    // the mesh was never validated.
    [[nodiscard]] std::optional<Triangle3> triangle(std::size_t index) const;

    [[nodiscard]] katana::math::AABB bounds() const;

    // Total area of the triangles. For a closed mesh this is its surface
    // area; for an open one it is the area of the sheet.
    [[nodiscard]] double area() const;

    // The footprint in plan: the convex HULL of the vertices, not the
    // silhouette. Stated because they differ - a horseshoe-shaped mesh's
    // hull covers ground the mesh does not - and the hull is what can be
    // computed exactly and cheaply for a mesh of any topology.
    //
    // Degenerate footprints come back as they are rather than as nothing: a
    // vertical wall gives the two ends of a segment, because seen from above
    // a wall IS a line and drawing that line is the truth about where it
    // stands. A caller therefore handles 0, 1, 2 or more points.
    [[nodiscard]] std::vector<Point2> planHull() const;
};

// Fails naming the first face that indexes a vertex that does not exist, or
// the first non-finite vertex. A mesh that fails this may still be drawn
// through triangle(), which refuses such a face one at a time.
[[nodiscard]] katana::core::Status validate(const TriangleMesh& mesh);

} // namespace katana::geometry
