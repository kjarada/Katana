#pragma once

// Whether two TIN surfaces are the same surface: the same vertices and the
// same triangles, with the same constrained edges, wherever their vertices
// are numbered. An edit that makes new vertices in a different order - one
// patch for several footprints, or one patch each - makes the same surface,
// which TinSurface::operator== (index for index) would call different.
//
// Coordinates are compared in micrometres: a vertex two runs make by
// arithmetic that differs in the last bit is one vertex, and no two vertices
// of a test's surface are a micrometre apart.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/terrain/tin_surface.hpp"

namespace katana::test {

namespace surface_geometry_detail {

using Corner = std::array<std::int64_t, 3>;

inline Corner cornerOf(const katana::geometry::Point3& p)
{
    return {static_cast<std::int64_t>(std::llround(p.x * 1e6)),
            static_cast<std::int64_t>(std::llround(p.y * 1e6)),
            static_cast<std::int64_t>(std::llround(p.z * 1e6))};
}

struct Canonical {
    std::vector<Corner> vertices;
    // Three corners as x, y, z, then bit j set when the j-th edge from the
    // first corner is a breakline or boundary edge.
    std::vector<std::array<std::int64_t, 10>> triangles;
    bool operator==(const Canonical&) const = default;
};

inline Canonical canonical(const katana::terrain::TinSurface& surface)
{
    Canonical result;
    for (const katana::geometry::Point3& v : surface.vertices()) {
        result.vertices.push_back(cornerOf(v));
    }
    std::sort(result.vertices.begin(), result.vertices.end());
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        std::array<Corner, 3> corners;
        for (std::size_t k = 0; k < 3; ++k) {
            corners[k] = cornerOf(surface.vertices()[surface.triangles()[t][k]]);
        }
        const auto start = static_cast<std::size_t>(
            std::min_element(corners.begin(), corners.end()) - corners.begin());
        std::array<std::int64_t, 10> key{};
        for (std::size_t j = 0; j < 3; ++j) {
            const Corner& at = corners[(j + start) % 3];
            key[3 * j] = at[0];
            key[3 * j + 1] = at[1];
            key[3 * j + 2] = at[2];
            if (surface.isEdgeConstrained(t, (j + start) % 3)) {
                key[9] |= std::int64_t{1} << j;
            }
        }
        result.triangles.push_back(key);
    }
    std::sort(result.triangles.begin(), result.triangles.end());
    return result;
}

} // namespace surface_geometry_detail

// Success when `a` and `b` are the same surface; else what differs.
inline ::testing::AssertionResult sameSurfaceGeometry(const katana::terrain::TinSurface& a,
                                                      const katana::terrain::TinSurface& b)
{
    const auto ca = surface_geometry_detail::canonical(a);
    const auto cb = surface_geometry_detail::canonical(b);
    if (ca == cb) {
        return ::testing::AssertionSuccess();
    }
    return ::testing::AssertionFailure()
           << "vertices " << a.vertexCount() << " and " << b.vertexCount() << ", triangles "
           << a.triangleCount() << " and " << b.triangleCount() << ", "
           << (ca.vertices == cb.vertices ? "the same vertices" : "different vertices") << ", "
           << (ca.triangles == cb.triangles ? "the same triangles" : "different triangles");
}

} // namespace katana::test
