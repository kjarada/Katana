#pragma once

// Internal interface to the constrained Delaunay triangulation kernel (PLAN.MD
// Rule 4). Everything that touches CGAL lives in cdt_backend.cpp; this header and
// the rest of the module see Katana types only.

#include <array>
#include <cstdint>
#include <span>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::terrain::detail {

inline constexpr std::uint32_t kNoIndex = 0xFFFFFFFFu;

struct CdtConstraint {
    std::uint32_t from = 0; // indices into CdtInput::points
    std::uint32_t to = 0;
    std::uint8_t tag = 1; // non-zero bit mask, reported back per constrained edge
};

struct CdtInput {
    // Pairwise distinct (exactly). At most 2^32 - 2 points.
    std::span<const katana::geometry::Point2> points;
    // May cross, overlap, repeat and pass through points; from == to is ignored.
    std::span<const CdtConstraint> constraints;
    bool exportConstraintVertices = false;
};

// Canonical triangulation of the convex hull of the points.
//   * Vertex i < points.size() is input point i; vertex points.size() + j is
//     generatedPoints[j], a crossing of two constraints. Generated points are
//     sorted by (x, y).
//   * Triangles are counter-clockwise, start at their smallest vertex index and
//     are sorted lexicographically. Edge k joins vertex k and vertex (k + 1) % 3.
//   * neighbors[t][k] is the triangle across edge k, or kNoIndex on the hull.
//   * edgeTags[t][k] is the OR of the tags of all constraints running along edge
//     k (0 = unconstrained).
//   * With exportConstraintVertices, constraintVertices[constraintVertexOffsets[c]
//     .. constraintVertexOffsets[c + 1]) are the vertices along constraint c from
//     `from` to `to`: its end points plus every vertex that splits it (crossing
//     points and input points lying exactly on it). Empty for ignored constraints.
struct CdtOutput {
    std::vector<katana::geometry::Point2> generatedPoints;
    std::vector<std::array<std::uint32_t, 3>> triangles;
    std::vector<std::array<std::uint32_t, 3>> neighbors;
    std::vector<std::array<std::uint8_t, 3>> edgeTags;
    std::vector<std::uint32_t> constraintVertexOffsets;
    std::vector<std::uint32_t> constraintVertices;
};

// Fails with TriangulationFailure when there are fewer than 3 points, all points
// are collinear, or the kernel reports an error (kernel exceptions never escape);
// with InvalidArgument for out-of-range constraint indices or duplicate points.
[[nodiscard]] katana::core::Result<CdtOutput> triangulate(const CdtInput& input);

} // namespace katana::terrain::detail
