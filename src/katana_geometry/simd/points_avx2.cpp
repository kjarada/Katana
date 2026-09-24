// AVX2 point kernels. Compiled with -mavx2 -mfma by katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_avx2.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_avx2_ entries, no includes but intrinsics, <cstddef> and the
// declarations.
//
// Every lane does the operations of the scalar reference in the scalar
// reference's order - multiply by the matrix entry, add left to right - so each
// result rounds exactly as math::transformPoint's does. -ffp-contract=off keeps
// the compiler from fusing a multiply and an add, which would round once
// instead of twice; the object check fails the build if it ever did.

#include <immintrin.h>

#include <cstddef>

#include "points_kernels.hpp"

namespace {

inline __m256d mul(__m256d a, __m256d b) { return _mm256_mul_pd(a, b); }
inline __m256d add(__m256d a, __m256d b) { return _mm256_add_pd(a, b); }

// Box2::expand and AABB::expand keep the running extreme unless the new value
// is strictly beyond it: std::min(acc, v) is (v < acc) ? v : acc. MINPD and
// MAXPD return their SECOND operand unless the first is strictly less
// (greater), so with v first they are that expression exactly - a NaN in v
// keeps acc, and of two zeros the running one stays.
inline __m256d keepMin(__m256d v, __m256d acc) { return _mm256_min_pd(v, acc); }
inline __m256d keepMax(__m256d v, __m256d acc) { return _mm256_max_pd(v, acc); }
inline double keepMin(double v, double acc) { return v < acc ? v : acc; }
inline double keepMax(double v, double acc) { return v > acc ? v : acc; }

} // namespace

extern "C" void katana_avx2_transform_points3(const double* m, double* points, std::size_t count)
{
    const __m256d m0 = _mm256_set1_pd(m[0]);
    const __m256d m1 = _mm256_set1_pd(m[1]);
    const __m256d m2 = _mm256_set1_pd(m[2]);
    const __m256d m3 = _mm256_set1_pd(m[3]);
    const __m256d m4 = _mm256_set1_pd(m[4]);
    const __m256d m5 = _mm256_set1_pd(m[5]);
    const __m256d m6 = _mm256_set1_pd(m[6]);
    const __m256d m7 = _mm256_set1_pd(m[7]);
    const __m256d m8 = _mm256_set1_pd(m[8]);
    const __m256d m9 = _mm256_set1_pd(m[9]);
    const __m256d m10 = _mm256_set1_pd(m[10]);
    const __m256d m11 = _mm256_set1_pd(m[11]);
    std::size_t i = 0;
    // Four points are twelve doubles, three registers. The comments give the
    // lanes by 128-bit half, which is the unit the permutes move.
    for (; i + 4 <= count; i += 4) {
        double* p = points + 3 * i;
        const __m256d v0 = _mm256_loadu_pd(p);     // x0 y0 | z0 x1
        const __m256d v1 = _mm256_loadu_pd(p + 4); // y1 z1 | x2 y2
        const __m256d v2 = _mm256_loadu_pd(p + 8); // z2 x3 | y3 z3
        const __m256d a = _mm256_permute2f128_pd(v0, v1, 0x30); // x0 y0 | x2 y2
        const __m256d b = _mm256_permute2f128_pd(v0, v2, 0x21); // z0 x1 | z2 x3
        const __m256d c = _mm256_permute2f128_pd(v1, v2, 0x30); // y1 z1 | y3 z3
        const __m256d x = _mm256_blend_pd(a, b, 0b1010);        // x0 x1 | x2 x3
        const __m256d y = _mm256_shuffle_pd(a, c, 0b0101);      // y0 y1 | y2 y3
        const __m256d z = _mm256_blend_pd(b, c, 0b1010);        // z0 z1 | z2 z3

        const __m256d tx = add(add(add(mul(m0, x), mul(m1, y)), mul(m2, z)), m3);
        const __m256d ty = add(add(add(mul(m4, x), mul(m5, y)), mul(m6, z)), m7);
        const __m256d tz = add(add(add(mul(m8, x), mul(m9, y)), mul(m10, z)), m11);

        const __m256d ra = _mm256_shuffle_pd(tx, ty, 0b0000); // x0 y0 | x2 y2
        const __m256d rb = _mm256_blend_pd(tz, tx, 0b1010);   // z0 x1 | z2 x3
        const __m256d rc = _mm256_shuffle_pd(ty, tz, 0b1111); // y1 z1 | y3 z3
        _mm256_storeu_pd(p, _mm256_permute2f128_pd(ra, rb, 0x20));     // x0 y0 | z0 x1
        _mm256_storeu_pd(p + 4, _mm256_permute2f128_pd(rc, ra, 0x30)); // y1 z1 | x2 y2
        _mm256_storeu_pd(p + 8, _mm256_permute2f128_pd(rb, rc, 0x31)); // z2 x3 | y3 z3
    }
    for (; i < count; ++i) {
        double* p = points + 3 * i;
        const double x = p[0];
        const double y = p[1];
        const double z = p[2];
        p[0] = m[0] * x + m[1] * y + m[2] * z + m[3];
        p[1] = m[4] * x + m[5] * y + m[6] * z + m[7];
        p[2] = m[8] * x + m[9] * y + m[10] * z + m[11];
    }
}

extern "C" void katana_avx2_transform_points2(const double* m, double* points, std::size_t count)
{
    // Two points a register, x0 y0 | x1 y1, and the layout never changes: the
    // even lanes compute x' and the odd lanes y', each from its point's x
    // (duplicated across the pair) and y.
    const __m256d byX = _mm256_setr_pd(m[0], m[3], m[0], m[3]);
    const __m256d byY = _mm256_setr_pd(m[1], m[4], m[1], m[4]);
    const __m256d offset = _mm256_setr_pd(m[2], m[5], m[2], m[5]);
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        double* p = points + 2 * i;
        const __m256d v0 = _mm256_loadu_pd(p);
        const __m256d v1 = _mm256_loadu_pd(p + 4);
        const __m256d r0 = add(add(mul(byX, _mm256_movedup_pd(v0)), mul(byY, _mm256_permute_pd(v0, 0b1111))), offset);
        const __m256d r1 = add(add(mul(byX, _mm256_movedup_pd(v1)), mul(byY, _mm256_permute_pd(v1, 0b1111))), offset);
        _mm256_storeu_pd(p, r0);
        _mm256_storeu_pd(p + 4, r1);
    }
    for (; i < count; ++i) {
        double* p = points + 2 * i;
        const double x = p[0];
        const double y = p[1];
        p[0] = m[0] * x + m[1] * y + m[2];
        p[1] = m[3] * x + m[4] * y + m[5];
    }
}

extern "C" void katana_avx2_bounds2(const double* points, std::size_t count, double* box)
{
    const double inf = __builtin_inf();
    // Two accumulators of each kind, so consecutive MINPDs do not wait on each
    // other. Lanes 0 and 2 hold x, lanes 1 and 3 hold y.
    __m256d lo0 = _mm256_set1_pd(inf);
    __m256d lo1 = lo0;
    __m256d hi0 = _mm256_set1_pd(-inf);
    __m256d hi1 = hi0;
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const __m256d v0 = _mm256_loadu_pd(points + 2 * i);
        const __m256d v1 = _mm256_loadu_pd(points + 2 * i + 4);
        lo0 = keepMin(v0, lo0);
        hi0 = keepMax(v0, hi0);
        lo1 = keepMin(v1, lo1);
        hi1 = keepMax(v1, hi1);
    }
    double lo[8];
    double hi[8];
    _mm256_storeu_pd(lo, lo0);
    _mm256_storeu_pd(lo + 4, lo1);
    _mm256_storeu_pd(hi, hi0);
    _mm256_storeu_pd(hi + 4, hi1);
    double minX = inf;
    double minY = inf;
    double maxX = -inf;
    double maxY = -inf;
    for (int lane = 0; lane < 8; lane += 2) {
        minX = keepMin(lo[lane], minX);
        minY = keepMin(lo[lane + 1], minY);
        maxX = keepMax(hi[lane], maxX);
        maxY = keepMax(hi[lane + 1], maxY);
    }
    for (; i < count; ++i) {
        minX = keepMin(points[2 * i], minX);
        minY = keepMin(points[2 * i + 1], minY);
        maxX = keepMax(points[2 * i], maxX);
        maxY = keepMax(points[2 * i + 1], maxY);
    }
    box[0] = minX;
    box[1] = minY;
    box[2] = maxX;
    box[3] = maxY;
}

extern "C" void katana_avx2_bounds3(const double* points, std::size_t count, double* box)
{
    const double inf = __builtin_inf();
    // Twelve doubles - four points - a step, in three registers whose lanes
    // repeat the coordinates as x y z x | y z x y | z x y z, so each lane of
    // each accumulator always sees the same coordinate and no shuffle is
    // needed until the end.
    __m256d lo0 = _mm256_set1_pd(inf);
    __m256d lo1 = lo0;
    __m256d lo2 = lo0;
    __m256d hi0 = _mm256_set1_pd(-inf);
    __m256d hi1 = hi0;
    __m256d hi2 = hi0;
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const double* p = points + 3 * i;
        const __m256d v0 = _mm256_loadu_pd(p);
        const __m256d v1 = _mm256_loadu_pd(p + 4);
        const __m256d v2 = _mm256_loadu_pd(p + 8);
        lo0 = keepMin(v0, lo0);
        lo1 = keepMin(v1, lo1);
        lo2 = keepMin(v2, lo2);
        hi0 = keepMax(v0, hi0);
        hi1 = keepMax(v1, hi1);
        hi2 = keepMax(v2, hi2);
    }
    double lo[12];
    double hi[12];
    _mm256_storeu_pd(lo, lo0);
    _mm256_storeu_pd(lo + 4, lo1);
    _mm256_storeu_pd(lo + 8, lo2);
    _mm256_storeu_pd(hi, hi0);
    _mm256_storeu_pd(hi + 4, hi1);
    _mm256_storeu_pd(hi + 8, hi2);
    // Stored side by side, the twelve lanes are x y z repeated four times.
    double minimum[3] = {inf, inf, inf};
    double maximum[3] = {-inf, -inf, -inf};
    for (int lane = 0; lane < 12; ++lane) {
        minimum[lane % 3] = keepMin(lo[lane], minimum[lane % 3]);
        maximum[lane % 3] = keepMax(hi[lane], maximum[lane % 3]);
    }
    for (; i < count; ++i) {
        for (int axis = 0; axis < 3; ++axis) {
            minimum[axis] = keepMin(points[3 * i + static_cast<std::size_t>(axis)], minimum[axis]);
            maximum[axis] = keepMax(points[3 * i + static_cast<std::size_t>(axis)], maximum[axis]);
        }
    }
    for (int axis = 0; axis < 3; ++axis) {
        box[axis] = minimum[axis];
        box[3 + axis] = maximum[axis];
    }
}
