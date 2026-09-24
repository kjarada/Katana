#pragma once

// Entry point of the AVX2 point-splat projection, compiled from splat_avx2.cpp.
// Declarations only; see src/katana_core/simd/text_kernels.hpp for why it has
// C linkage and a katana_avx2_ prefix.
//
// It equals projectToPixelsScalar in point_splat.cpp bit for bit: each float
// coordinate is widened to double exactly, and the projection is the scalar
// reference's operations in its order, never fused.

#include <cstddef>
#include <cstdint>

extern "C" {

// For each i, with p = {half width, half height, centre x, centre y, scale}:
//   sx = p0 + (double(xs[i]) - p2) * p4,  sy = p1 - (double(ys[i]) - p3) * p4;
// px[i], py[i] = sx, sy truncated towards zero when both lie strictly inside
// (-1e6, 1e6), and both INT32_MIN otherwise (NaN included).
void katana_avx2_project_to_pixels(const double* p, const float* xs, const float* ys,
                                   std::size_t count, std::int32_t* px, std::int32_t* py);

} // extern "C"
