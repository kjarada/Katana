// NEON point-splat projection, for 64-bit ARM. Compiled by
// katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_neon.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_neon_ entry, no includes but <arm_neon.h>, <cstddef>, <cstdint>
// and the declarations.
//
// Four points a step, as the AVX2 kernel takes them, in two registers of two
// doubles: widening a float to double is exact, and the projection is the
// scalar reference's subtract, multiply and add, never an FMLA.

#include <arm_neon.h>

#include <cstddef>
#include <cstdint>

#include "splat_kernels.hpp"

extern "C" void katana_neon_project_to_pixels(const double* p, const float* xs, const float* ys,
                                              std::size_t count, std::int32_t* px,
                                              std::int32_t* py)
{
    const float64x2_t halfWidth = vdupq_n_f64(p[0]);
    const float64x2_t halfHeight = vdupq_n_f64(p[1]);
    const float64x2_t centreX = vdupq_n_f64(p[2]);
    const float64x2_t centreY = vdupq_n_f64(p[3]);
    const float64x2_t scale = vdupq_n_f64(p[4]);
    const float64x2_t low = vdupq_n_f64(-1.0e6);
    const float64x2_t high = vdupq_n_f64(1.0e6);
    const int32x4_t rejected = vdupq_n_s32(INT32_MIN);
    // Two points: their screen coordinates and whether both lie inside.
    // FCMGT/FCMLT are ordered compares, false for a NaN, as !(a > b) makes it
    // outside in the scalar reference.
    const auto project = [&](float32x2_t x32, float32x2_t y32, int64x2_t& sxWhole,
                             int64x2_t& syWhole, uint64x2_t& inside) {
        const float64x2_t x = vcvt_f64_f32(x32);
        const float64x2_t y = vcvt_f64_f32(y32);
        const float64x2_t sx = vaddq_f64(halfWidth, vmulq_f64(vsubq_f64(x, centreX), scale));
        const float64x2_t sy = vsubq_f64(halfHeight, vmulq_f64(vsubq_f64(y, centreY), scale));
        inside = vandq_u64(vandq_u64(vcgtq_f64(sx, low), vcltq_f64(sx, high)),
                           vandq_u64(vcgtq_f64(sy, low), vcltq_f64(sy, high)));
        // FCVTZS truncates towards zero, as static_cast<std::int32_t> does;
        // inside (-1e6, 1e6) the 64-bit result is the 32-bit one exactly, and
        // outside it the lane is replaced below.
        sxWhole = vcvtq_s64_f64(sx);
        syWhole = vcvtq_s64_f64(sy);
    };
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const float32x4_t x4 = vld1q_f32(xs + i);
        const float32x4_t y4 = vld1q_f32(ys + i);
        int64x2_t sx0;
        int64x2_t sy0;
        uint64x2_t in0;
        int64x2_t sx1;
        int64x2_t sy1;
        uint64x2_t in1;
        project(vget_low_f32(x4), vget_low_f32(y4), sx0, sy0, in0);
        project(vget_high_f32(x4), vget_high_f32(y4), sx1, sy1, in1);
        // Narrowed to 32 bits: the low half of each 64-bit lane, of the
        // integers and of the all-ones or all-zeros masks alike.
        const uint32x4_t inside = vcombine_u32(vmovn_u64(in0), vmovn_u64(in1));
        const int32x4_t sx = vcombine_s32(vmovn_s64(sx0), vmovn_s64(sx1));
        const int32x4_t sy = vcombine_s32(vmovn_s64(sy0), vmovn_s64(sy1));
        vst1q_s32(px + i, vbslq_s32(inside, sx, rejected));
        vst1q_s32(py + i, vbslq_s32(inside, sy, rejected));
    }
    for (; i < count; ++i) {
        const double sx = p[0] + (static_cast<double>(xs[i]) - p[2]) * p[4];
        const double sy = p[1] - (static_cast<double>(ys[i]) - p[3]) * p[4];
        if (sx > -1.0e6 && sx < 1.0e6 && sy > -1.0e6 && sy < 1.0e6) {
            px[i] = static_cast<std::int32_t>(sx);
            py[i] = static_cast<std::int32_t>(sy);
        } else {
            px[i] = INT32_MIN;
            py[i] = INT32_MIN;
        }
    }
}
