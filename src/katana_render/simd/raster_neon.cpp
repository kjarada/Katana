// NEON rasteriser kernels, for 64-bit ARM. Compiled by katana_add_simd_sources.
//
// A KERNEL FILE (cmake/KatanaSimd.cmake, docs/performance.md "SIMD: kernels
// chosen at run time"): raw pointers only, nothing visible to the linker but the
// katana_neon_ entries, no includes but <arm_neon.h>, <cstddef>, <cstdint> and
// the declarations, and no namespace-scope object with an initialiser.
//
// BIT FOR BIT, as raster_avx2.cpp is: each lane repeats its scalar reference in
// rasterizer.cpp operation for operation, in the same order, with the same
// comparisons. Three things differ from x86 and are handled here rather than
// left to chance:
//   - FMLA is a baseline instruction, so every product and sum is a separate
//     FMUL and FADD (and -ffp-contract=off keeps the compiler from fusing
//     them; the object check fails the build if it ever did).
//   - FMIN/FMAX/FMINNM/FMAXNM treat NaN and signed zero otherwise than
//     std::min, std::max and std::clamp, so those are a compare and a BSL
//     select that return what the std functions return.
//   - There is no masked load or store, so a block of four pixels that runs
//     past the end of its span is read and written one lane at a time.

#include <arm_neon.h>

#include <cstddef>
#include <cstdint>

#include "raster_kernels.hpp"

namespace {

inline float64x2_t mul(float64x2_t a, float64x2_t b) { return vmulq_f64(a, b); }
inline float64x2_t add(float64x2_t a, float64x2_t b) { return vaddq_f64(a, b); }
inline float64x2_t sub(float64x2_t a, float64x2_t b) { return vsubq_f64(a, b); }
inline float32x4_t mul(float32x4_t a, float32x4_t b) { return vmulq_f32(a, b); }
inline float32x4_t add(float32x4_t a, float32x4_t b) { return vaddq_f32(a, b); }
inline float32x4_t sub(float32x4_t a, float32x4_t b) { return vsubq_f32(a, b); }

// Bit `bit` in each lane whose plane distance is not >= 0 - which a NaN is
// not - exactly the reference's !(d >= 0). BIC is `bit and not inside`.
inline uint64x2_t outside(float64x2_t distance, std::uint64_t bit)
{
    return vbicq_u64(vdupq_n_u64(bit), vcgeq_f64(distance, vdupq_n_f64(0.0)));
}

// Two vertices' clip coordinates in double: Mat4::multiply with w = 1,
// (((m0 x + m1 y) + m2 z) + m3 * 1) for each row.
struct Clip2 {
    float64x2_t x, y, z, w;
};

inline Clip2 transform2(const float64x2_t (&m)[16], const float64x2x3_t& p)
{
    const float64x2_t one = vdupq_n_f64(1.0);
    const float64x2_t x = p.val[0];
    const float64x2_t y = p.val[1];
    const float64x2_t z = p.val[2];
    return Clip2{add(add(add(mul(m[0], x), mul(m[1], y)), mul(m[2], z)), mul(m[3], one)),
                 add(add(add(mul(m[4], x), mul(m[5], y)), mul(m[6], z)), mul(m[7], one)),
                 add(add(add(mul(m[8], x), mul(m[9], y)), mul(m[10], z)), mul(m[11], one)),
                 add(add(add(mul(m[12], x), mul(m[13], y)), mul(m[14], z)), mul(m[15], one))};
}

// The clip codes of two vertices, judged in double from their FLOAT
// coordinates as the reference's planeDistance is: w - z, then 2w +- x and
// 2w +- y.
inline uint64x2_t clipCodes(float32x2_t fx, float32x2_t fy, float32x2_t fz, float32x2_t fw)
{
    const float64x2_t dx = vcvt_f64_f32(fx);
    const float64x2_t dy = vcvt_f64_f32(fy);
    const float64x2_t dz = vcvt_f64_f32(fz);
    const float64x2_t dw = vcvt_f64_f32(fw);
    const float64x2_t band = mul(vdupq_n_f64(2.0), dw);
    return vorrq_u64(vorrq_u64(vorrq_u64(outside(sub(dw, dz), 1), outside(add(band, dx), 2)),
                               vorrq_u64(outside(sub(band, dx), 4), outside(add(band, dy), 8))),
                     outside(sub(band, dy), 16));
}

// std::min(v, hi) is (hi < v) ? hi : v; std::max(v, lo) is (v < lo) ? lo : v.
// A NaN v comes through both, as it does through the std functions.
inline float32x4_t minOf(float32x4_t v, float32x4_t hi) { return vbslq_f32(vcltq_f32(hi, v), hi, v); }
inline float32x4_t maxOf(float32x4_t v, float32x4_t lo) { return vbslq_f32(vcltq_f32(v, lo), lo, v); }

} // namespace

extern "C" void katana_neon_transform_vertices(const double* mvp, const double* positions,
                                               std::size_t count, const std::uint32_t* colors,
                                               std::size_t colorCount, float width, float height,
                                               float* clip, std::uint8_t* codes, float* screen)
{
    float64x2_t m[16];
    for (int k = 0; k < 16; ++k) {
        m[k] = vdupq_n_f64(mvp[k]);
    }
    const float32x4_t onef = vdupq_n_f32(1.0f);
    const float32x4_t halff = vdupq_n_f32(0.5f);
    const float32x4_t widthf = vdupq_n_f32(width);
    const float32x4_t heightf = vdupq_n_f32(height);

    // Four vertices a step, as the AVX2 kernel takes them; the caller hands
    // the remainder to the reference.
    for (std::size_t i = 0; i + 4 <= count; i += 4) {
        // LD3 splits two vertices' x, y, z into a register each.
        const Clip2 a = transform2(m, vld3q_f64(positions + 3 * i));
        const Clip2 b = transform2(m, vld3q_f64(positions + 3 * i + 6));

        // To float, rounding to nearest (the FPCR default) as static_cast<float>
        // does. FCVTN narrows two doubles; the pairs are then joined.
        const float32x2_t ax = vcvt_f32_f64(a.x);
        const float32x2_t ay = vcvt_f32_f64(a.y);
        const float32x2_t az = vcvt_f32_f64(a.z);
        const float32x2_t aw = vcvt_f32_f64(a.w);
        const float32x2_t bx = vcvt_f32_f64(b.x);
        const float32x2_t by = vcvt_f32_f64(b.y);
        const float32x2_t bz = vcvt_f32_f64(b.z);
        const float32x2_t bw = vcvt_f32_f64(b.w);

        const uint64x2_t codesA = clipCodes(ax, ay, az, aw);
        const uint64x2_t codesB = clipCodes(bx, by, bz, bw);
        codes[i] = static_cast<std::uint8_t>(vgetq_lane_u64(codesA, 0));
        codes[i + 1] = static_cast<std::uint8_t>(vgetq_lane_u64(codesA, 1));
        codes[i + 2] = static_cast<std::uint8_t>(vgetq_lane_u64(codesB, 0));
        codes[i + 3] = static_cast<std::uint8_t>(vgetq_lane_u64(codesB, 1));

        const float32x4_t fx = vcombine_f32(ax, bx);
        const float32x4_t fy = vcombine_f32(ay, by);
        const float32x4_t fz = vcombine_f32(az, bz);
        const float32x4_t fw = vcombine_f32(aw, bw);

        // The projection, in float: 1/w, then ((x/w) * 0.5 + 0.5) * width,
        // (0.5 - (y/w) * 0.5) * height and z/w, each product as the reference
        // groups it. FDIV is correctly rounded, as the scalar divide is.
        const float32x4_t invW = vdivq_f32(onef, fw);
        const float32x4_t sx = mul(add(mul(mul(fx, invW), halff), halff), widthf);
        const float32x4_t sy = mul(sub(halff, mul(mul(fy, invW), halff)), heightf);
        const float32x4_t sz = mul(fz, invW);

        // Out at five words a vertex: ST4 writes four interleaved, so the
        // four coordinates go through a small buffer and out with the
        // colour word, which is a 32-bit word of the caller's struct and is
        // copied as the integer it is.
        // (Named first: clang's vst4q_f32 is a macro, and a braced list
        // inside its call would be split at the commas.)
        const float32x4x4_t clipColumns = {{fx, fy, fz, fw}};
        const float32x4x4_t screenColumns = {{sx, sy, sz, invW}};
        float clipRows[16];
        float screenRows[16];
        vst4q_f32(clipRows, clipColumns);
        vst4q_f32(screenRows, screenColumns);
        for (std::size_t k = 0; k < 4; ++k) {
            const std::size_t vertex = i + k;
            const std::uint32_t word = vertex < colorCount ? colors[vertex] : 0xFFFFFFFFu;
            float* clipOut = clip + 5 * vertex;
            float* screenOut = screen + 5 * vertex;
            vst1q_f32(clipOut, vld1q_f32(clipRows + 4 * k));
            vst1q_f32(screenOut, vld1q_f32(screenRows + 4 * k));
            __builtin_memcpy(clipOut + 4, &word, sizeof word);
            __builtin_memcpy(screenOut + 4, &word, sizeof word);
        }
    }
}

extern "C" std::size_t katana_neon_shade_rows(const float* triangle, float invArea,
                                              const int* spans, int firstRow, int rowCount,
                                              int covered, int depthWrite, std::uint32_t* color,
                                              float* depth, std::size_t stride)
{
    const float* tx = triangle;
    const float* ty = triangle + 3;
    const float* tz = triangle + 6;
    const float* tw = triangle + 9;
    std::uint32_t words[3];
    __builtin_memcpy(words, triangle + 12, sizeof words);
    float bias;
    __builtin_memcpy(&bias, triangle + 15, sizeof bias);

    const float32x4_t x0 = vdupq_n_f32(tx[0]);
    const float32x4_t x1 = vdupq_n_f32(tx[1]);
    const float32x4_t x2 = vdupq_n_f32(tx[2]);
    const float32x4_t z0 = vdupq_n_f32(tz[0]);
    const float32x4_t z1 = vdupq_n_f32(tz[1]);
    const float32x4_t z2 = vdupq_n_f32(tz[2]);
    const float32x4_t iw0 = vdupq_n_f32(tw[0]);
    const float32x4_t iw1 = vdupq_n_f32(tw[1]);
    const float32x4_t iw2 = vdupq_n_f32(tw[2]);
    const float32x4_t biasV = vdupq_n_f32(bias);
    const float32x4_t inv = vdupq_n_f32(invArea);
    const float32x4_t zero = vdupq_n_f32(0.0f);
    const float32x4_t one = vdupq_n_f32(1.0f);
    const float32x4_t half = vdupq_n_f32(0.5f);
    const float32x4_t top = vdupq_n_f32(255.0f);
    const std::int32_t laneOffsets[4] = {0, 1, 2, 3};
    const int32x4_t lanes = vld1q_s32(laneOffsets);
    const uint32x4_t byteMask = vdupq_n_u32(0xFF);
    const uint32x4_t fallback = vdupq_n_u32(words[0]);

    // Each vertex's channels as floats, alpha first as the reference packs.
    float32x4_t channel[4][3];
    for (int k = 0; k < 4; ++k) {
        const int shift = 24 - 8 * k;
        for (int v = 0; v < 3; ++v) {
            channel[k][v] = vdupq_n_f32(static_cast<float>((words[v] >> shift) & 0xFFu));
        }
    }

    // Written lanes are all ones, -1 each: subtracted, they count up.
    uint32x4_t written = vdupq_n_u32(0);
    for (int r = 0; r < rowCount; ++r) {
        const int first = spans[2 * r];
        const int last = spans[2 * r + 1];
        if (first > last) {
            continue;
        }
        const int row = firstRow + r;
        const float py = static_cast<float>(row) + 0.5f;
        const float32x4_t dy0 = vdupq_n_f32(ty[0] - py);
        const float32x4_t dy1 = vdupq_n_f32(ty[1] - py);
        const float32x4_t dy2 = vdupq_n_f32(ty[2] - py);
        std::uint32_t* colorRow = color + static_cast<std::size_t>(row) * stride;
        float* depthRow = depth + static_cast<std::size_t>(row) * stride;
        const int32x4_t end = vdupq_n_s32(last + 1);

        for (int x = first; x <= last; x += 4) {
            const int32x4_t xs = vaddq_s32(vdupq_n_s32(x), lanes);
            uint32x4_t mask = vcgtq_s32(end, xs);
            // SCVTF rounds to nearest, as static_cast<float>(int) does.
            const float32x4_t px = add(vcvtq_f32_s32(xs), half);
            const float32x4_t dx0 = sub(x0, px);
            const float32x4_t dx1 = sub(x1, px);
            const float32x4_t dx2 = sub(x2, px);
            const float32x4_t w0 = mul(sub(mul(dx1, dy2), mul(dx2, dy1)), inv);
            const float32x4_t w1 = mul(sub(mul(dx2, dy0), mul(dx0, dy2)), inv);
            const float32x4_t w2 = sub(sub(one, w0), w1);
            if (covered == 0) {
                // Outside when any is < 0; NaN is not < 0, as in the reference.
                const uint32x4_t out = vorrq_u32(vorrq_u32(vcltq_f32(w0, zero), vcltq_f32(w1, zero)),
                                                 vcltq_f32(w2, zero));
                mask = vbicq_u32(mask, out);
                if (vmaxvq_u32(mask) == 0) {
                    continue;
                }
            }
            // The pixels of the block that belong to the span: all four, or
            // the first few of the span's last block, which must not be read
            // or written past `last`.
            const int inSpan = last - x + 1 < 4 ? last - x + 1 : 4;
            float* depthAt = depthRow + x;
            std::uint32_t* colorAt = colorRow + x;
            float32x4_t before;
            if (inSpan == 4) {
                before = vld1q_f32(depthAt);
            } else {
                float partial[4] = {0.0f, 0.0f, 0.0f, 0.0f};
                for (int k = 0; k < inSpan; ++k) {
                    partial[k] = depthAt[k];
                }
                before = vld1q_f32(partial);
            }
            const float32x4_t d =
                minOf(add(add(add(mul(w0, z0), mul(w1, z1)), mul(w2, z2)), biasV), one);
            const uint32x4_t pass = vandq_u32(mask, vcgtq_f32(d, before));
            if (vmaxvq_u32(pass) == 0) {
                continue;
            }
            const float32x4_t invW = add(add(mul(w0, iw0), mul(w1, iw1)), mul(w2, iw2));
            const uint32x4_t perspective = vcgtq_f32(invW, zero);
            const float32x4_t s = vdivq_f32(one, invW);
            uint32x4_t packed = vdupq_n_u32(0);
            for (int k = 0; k < 4; ++k) {
                const float32x4_t value =
                    mul(add(add(mul(mul(w0, channel[k][0]), iw0), mul(mul(w1, channel[k][1]), iw1)),
                            mul(mul(w2, channel[k][2]), iw2)),
                        s);
                // std::clamp(v, 0, 255), then +0.5 truncated. FCVTZS truncates
                // towards zero and gives 0 for a NaN, which the AVX2 kernel's
                // 0x80000000 & 0xFF also gives, and AArch64's scalar FCVTZU.
                const float32x4_t clamped = minOf(maxOf(value, zero), top);
                const uint32x4_t byte =
                    vandq_u32(vreinterpretq_u32_s32(vcvtq_s32_f32(add(clamped, half))), byteMask);
                packed = vorrq_u32(packed, vshlq_u32(byte, vdupq_n_s32(24 - 8 * k)));
            }
            const uint32x4_t shaded = vbslq_u32(perspective, packed, fallback);
            if (inSpan == 4) {
                // Lanes that did not pass get back what was there: all four
                // pixels are this span's, which only this call writes.
                vst1q_u32(colorAt, vbslq_u32(pass, shaded, vld1q_u32(colorAt)));
                if (depthWrite != 0) {
                    vst1q_f32(depthAt, vbslq_f32(pass, d, before));
                }
            } else {
                std::uint32_t passLanes[4];
                std::uint32_t shadedLanes[4];
                float depthLanes[4];
                vst1q_u32(passLanes, pass);
                vst1q_u32(shadedLanes, shaded);
                vst1q_f32(depthLanes, d);
                for (int k = 0; k < inSpan; ++k) {
                    if (passLanes[k] != 0) {
                        colorAt[k] = shadedLanes[k];
                        if (depthWrite != 0) {
                            depthAt[k] = depthLanes[k];
                        }
                    }
                }
            }
            written = vsubq_u32(written, pass);
        }
    }
    return vaddvq_u32(written);
}
