#pragma once

// Entry points of the AVX2 point kernels, compiled from points_avx2.cpp.
// Declarations only; see src/katana_core/simd/text_kernels.hpp for why they
// have C linkage and a katana_avx2_ prefix.
//
// Points are arrays of doubles in the layout of math::Vec2 (x, y) and
// math::Vec3 (x, y, z), which point_batch.cpp static_asserts. Each entry equals
// the scalar reference in point_batch.cpp bit for bit, EXCEPT that a bounds
// entry may return either zero for a coordinate whose extreme is zero; the
// caller settles that one case (see boundsOf).

#include <cstddef>

extern "C" {

// points[i] = M * points[i] for the affine 3D transform whose first three rows
// are m[0..11] (row-major), in place, with math::transformPoint's order of
// operations: ((m0 x + m1 y) + m2 z) + m3, never fused.
void katana_avx2_transform_points3(const double* m, double* points, std::size_t count);

// The 2D affine form: rows m[0..5] of a math::Mat3, ((m0 x + m1 y) + m2).
void katana_avx2_transform_points2(const double* m, double* points, std::size_t count);

// box = {min x, min y, max x, max y} over `count` points, with Box2::expand's
// comparisons, so a NaN coordinate is passed over as it is there. Empty input
// gives the empty box, +inf/-inf.
void katana_avx2_bounds2(const double* points, std::size_t count, double* box);

// box = {min x, min y, min z, max x, max y, max z}, as math::AABB::expand.
void katana_avx2_bounds3(const double* points, std::size_t count, double* box);

} // extern "C"
