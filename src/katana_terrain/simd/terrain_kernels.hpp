#pragma once

// Entry points of the AVX2 terrain kernels, compiled from terrain_avx2.cpp.
// Declarations only; see src/katana_core/simd/text_kernels.hpp for why they
// have C linkage and a katana_avx2_ prefix.
//
// A surface is passed as its raw arrays: `vertices` is TinSurface::vertices()
// as doubles (x, y, z per vertex, the layout of math::Vec3), and `triangles` is
// TinSurface::triangles() as uint32 (three vertex indices per triangle). The
// kernels gather through those indices with 32-bit offsets, so the caller
// passes a surface only when three times its vertex and triangle counts fit in
// an int32 (see kMaxGatherCount in the callers). Each entry equals the scalar
// reference named against it bit for bit.

#include <cstddef>
#include <cstdint>

extern "C" {

// For every triangle t in [begin, end): the contour levels that cross it, as
// contours() in contours.cpp finds them. With low and high the smallest and
// largest of its three elevations, chosen as std::min / std::max over an
// initializer list choose them, and level(k) = base + k * interval: when
// low < high, ranges[2t] = the first k with level(k) > low and
// ranges[2t + 1] = (the first k with level(k) > high) - 1, starting from
// floor((z - base) / interval) and settled as firstLevelAbove settles it.
// Triangles with low < high false are left untouched. The caller guarantees
// |(z - base) / interval| stays within the 4e15 contours() checks.
void katana_avx2_contour_level_ranges(const double* vertices, const std::uint32_t* triangles,
                                      std::size_t begin, std::size_t end, double base,
                                      double interval, std::int64_t* ranges);

// Pass 1 of TinSurface::locate over candidates[start, count): the index of the
// first candidate triangle a, b, c whose three edge functions at (px, py),
//   d0 = (b - a).cross(p - a), d1 = (c - b).cross(p - b), d2 = (a - c).cross(p - c),
// are none of them < 0, or `count` when there is none. For the candidate
// found, d[0..2] receive d0, d1, d2.
std::size_t katana_avx2_first_enclosing(const double* vertices, const std::uint32_t* triangles,
                                        const std::uint32_t* candidates, std::size_t start,
                                        std::size_t count, double px, double py, double* d);

} // extern "C"
