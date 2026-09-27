// NEON point kernels, for 64-bit ARM. Compiled by katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_neon.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_neon_ entries, no includes but <arm_neon.h>, <cstddef> and the
// declarations.
//
// Every lane does the operations of the scalar reference in its order -
// multiply by the matrix entry, add left to right - with FMUL and FADD, never
// FMLA, so each result rounds exactly as math::transformPoint's does.
// -ffp-contract=off keeps the compiler from fusing them on its own; the object
// check fails the build if it ever did.
//
// LD2/LD3 split interleaved points into one register per coordinate, and
// ST2/ST3 put them back, so no lane is ever shuffled by hand.

#include <arm_neon.h>

#include <cstddef>

#include "points_kernels.hpp"

namespace {

inline float64x2_t mul(float64x2_t a, float64x2_t b) { return vmulq_f64(a, b); }
inline float64x2_t add(float64x2_t a, float64x2_t b) { return vaddq_f64(a, b); }

// Box2::expand and AABB::expand keep the running extreme unless the new value
// is strictly beyond it: (v < acc) ? v : acc. A compare and a select, because
// FMIN and FMINNM answer otherwise - FMIN returns a NaN from either operand,
// FMINNM drops a NaN from either, and both order -0 below +0 - where the
// reference keeps acc for a NaN in v and the running one of two zeros.
inline float64x2_t keepMin(float64x2_t v, float64x2_t acc)
{
    return vbslq_f64(vcltq_f64(v, acc), v, acc);
}
inline float64x2_t keepMax(float64x2_t v, float64x2_t acc)
{
    return vbslq_f64(vcgtq_f64(v, acc), v, acc);
}
inline double keepMin(double v, double acc) { return v < acc ? v : acc; }
inline double keepMax(double v, double acc) { return v > acc ? v : acc; }

} // namespace

extern "C" void katana_neon_transform_points3(const double* m, double* points, std::size_t count)
{
    const float64x2_t m0 = vdupq_n_f64(m[0]);
    const float64x2_t m1 = vdupq_n_f64(m[1]);
    const float64x2_t m2 = vdupq_n_f64(m[2]);
    const float64x2_t m3 = vdupq_n_f64(m[3]);
    const float64x2_t m4 = vdupq_n_f64(m[4]);
    const float64x2_t m5 = vdupq_n_f64(m[5]);
    const float64x2_t m6 = vdupq_n_f64(m[6]);
    const float64x2_t m7 = vdupq_n_f64(m[7]);
    const float64x2_t m8 = vdupq_n_f64(m[8]);
    const float64x2_t m9 = vdupq_n_f64(m[9]);
    const float64x2_t m10 = vdupq_n_f64(m[10]);
    const float64x2_t m11 = vdupq_n_f64(m[11]);
    const auto transform = [&](double* p) {
        const float64x2x3_t v = vld3q_f64(p); // x0 x1, y0 y1, z0 z1
        float64x2x3_t r;
        r.val[0] = add(add(add(mul(m0, v.val[0]), mul(m1, v.val[1])), mul(m2, v.val[2])), m3);
        r.val[1] = add(add(add(mul(m4, v.val[0]), mul(m5, v.val[1])), mul(m6, v.val[2])), m7);
        r.val[2] = add(add(add(mul(m8, v.val[0]), mul(m9, v.val[1])), mul(m10, v.val[2])), m11);
        vst3q_f64(p, r);
    };
    std::size_t i = 0;
    // Four points a step, as the AVX2 kernel takes them, so the tests' tails
    // (lengths past each multiple of four) are this kernel's tails too.
    for (; i + 4 <= count; i += 4) {
        transform(points + 3 * i);
        transform(points + 3 * i + 6);
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

extern "C" void katana_neon_transform_points2(const double* m, double* points, std::size_t count)
{
    const float64x2_t m0 = vdupq_n_f64(m[0]);
    const float64x2_t m1 = vdupq_n_f64(m[1]);
    const float64x2_t m2 = vdupq_n_f64(m[2]);
    const float64x2_t m3 = vdupq_n_f64(m[3]);
    const float64x2_t m4 = vdupq_n_f64(m[4]);
    const float64x2_t m5 = vdupq_n_f64(m[5]);
    const auto transform = [&](double* p) {
        const float64x2x2_t v = vld2q_f64(p); // x0 x1, y0 y1
        float64x2x2_t r;
        r.val[0] = add(add(mul(m0, v.val[0]), mul(m1, v.val[1])), m2);
        r.val[1] = add(add(mul(m3, v.val[0]), mul(m4, v.val[1])), m5);
        vst2q_f64(p, r);
    };
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        transform(points + 2 * i);
        transform(points + 2 * i + 4);
    }
    for (; i < count; ++i) {
        double* p = points + 2 * i;
        const double x = p[0];
        const double y = p[1];
        p[0] = m[0] * x + m[1] * y + m[2];
        p[1] = m[3] * x + m[4] * y + m[5];
    }
}

extern "C" void katana_neon_bounds2(const double* points, std::size_t count, double* box)
{
    const double inf = __builtin_inf();
    // One register per coordinate, two points a register; two of each, so
    // consecutive selects do not wait on each other.
    float64x2_t loX0 = vdupq_n_f64(inf);
    float64x2_t loY0 = loX0;
    float64x2_t loX1 = loX0;
    float64x2_t loY1 = loX0;
    float64x2_t hiX0 = vdupq_n_f64(-inf);
    float64x2_t hiY0 = hiX0;
    float64x2_t hiX1 = hiX0;
    float64x2_t hiY1 = hiX0;
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const float64x2x2_t v0 = vld2q_f64(points + 2 * i);
        const float64x2x2_t v1 = vld2q_f64(points + 2 * i + 4);
        loX0 = keepMin(v0.val[0], loX0);
        loY0 = keepMin(v0.val[1], loY0);
        hiX0 = keepMax(v0.val[0], hiX0);
        hiY0 = keepMax(v0.val[1], hiY0);
        loX1 = keepMin(v1.val[0], loX1);
        loY1 = keepMin(v1.val[1], loY1);
        hiX1 = keepMax(v1.val[0], hiX1);
        hiY1 = keepMax(v1.val[1], hiY1);
    }
    double lo[8];
    double hi[8];
    vst1q_f64(lo, loX0);
    vst1q_f64(lo + 2, loX1);
    vst1q_f64(lo + 4, loY0);
    vst1q_f64(lo + 6, loY1);
    vst1q_f64(hi, hiX0);
    vst1q_f64(hi + 2, hiX1);
    vst1q_f64(hi + 4, hiY0);
    vst1q_f64(hi + 6, hiY1);
    double minX = inf;
    double minY = inf;
    double maxX = -inf;
    double maxY = -inf;
    for (int lane = 0; lane < 4; ++lane) {
        minX = keepMin(lo[lane], minX);
        minY = keepMin(lo[4 + lane], minY);
        maxX = keepMax(hi[lane], maxX);
        maxY = keepMax(hi[4 + lane], maxY);
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

extern "C" void katana_neon_bounds3(const double* points, std::size_t count, double* box)
{
    const double inf = __builtin_inf();
    // Four points a step, as two de-interleaved pairs: lo[axis] and hi[axis]
    // each see only their own coordinate, so no shuffle is needed until the
    // end.
    float64x2_t lo[3] = {vdupq_n_f64(inf), vdupq_n_f64(inf), vdupq_n_f64(inf)};
    float64x2_t hi[3] = {vdupq_n_f64(-inf), vdupq_n_f64(-inf), vdupq_n_f64(-inf)};
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const float64x2x3_t v0 = vld3q_f64(points + 3 * i);
        const float64x2x3_t v1 = vld3q_f64(points + 3 * i + 6);
        for (int axis = 0; axis < 3; ++axis) {
            lo[axis] = keepMin(v1.val[axis], keepMin(v0.val[axis], lo[axis]));
            hi[axis] = keepMax(v1.val[axis], keepMax(v0.val[axis], hi[axis]));
        }
    }
    double minimum[3] = {inf, inf, inf};
    double maximum[3] = {-inf, -inf, -inf};
    for (int axis = 0; axis < 3; ++axis) {
        double l[2];
        double h[2];
        vst1q_f64(l, lo[axis]);
        vst1q_f64(h, hi[axis]);
        for (int lane = 0; lane < 2; ++lane) {
            minimum[axis] = keepMin(l[lane], minimum[axis]);
            maximum[axis] = keepMax(h[lane], maximum[axis]);
        }
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
