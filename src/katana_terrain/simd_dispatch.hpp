#pragma once

// Baseline-side helpers for calling the terrain kernels (simd/terrain_kernels.hpp).
// Included by ordinary source files only - never by a kernel file, which may
// include nothing of the project's but its declarations.

#include <array>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <type_traits>

#include "katana/core/cpu_features.hpp"
#include "katana/terrain/tin_surface.hpp"

#include "simd/terrain_kernels.hpp"

namespace katana::terrain::detail {

// The kernels read the surface's arrays as plain doubles and uint32s.
static_assert(std::is_standard_layout_v<Point3> && sizeof(Point3) == 3 * sizeof(double) &&
              offsetof(Point3, x) == 0 && offsetof(Point3, y) == sizeof(double) &&
              offsetof(Point3, z) == 2 * sizeof(double));
static_assert(sizeof(TinTriangle) == 3 * sizeof(std::uint32_t));

// The kernels gather with 32-bit offsets of three times an index, so a
// surface larger than this (715 million vertices or triangles, far beyond
// anything a TIN holds in memory) takes the scalar path.
inline constexpr std::size_t kMaxGatherCount =
    static_cast<std::size_t>(std::numeric_limits<std::int32_t>::max()) / 3;

[[nodiscard]] inline bool kernelsMayRead(const TinSurface& surface)
{
    return surface.vertexCount() <= kMaxGatherCount && surface.triangleCount() <= kMaxGatherCount;
}

[[nodiscard]] inline bool avx2Active()
{
    return katana::core::activeSimdLevel() == katana::core::SimdLevel::Avx2;
}

[[nodiscard]] inline const double* vertexData(const TinSurface& surface)
{
    return &surface.vertices().data()->x;
}

[[nodiscard]] inline const std::uint32_t* triangleData(const TinSurface& surface)
{
    return surface.triangles().data()->data();
}

} // namespace katana::terrain::detail
