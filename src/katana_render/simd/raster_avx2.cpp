// AVX2 rasteriser kernels. Compiled with -mavx2 -mfma by katana_add_simd_sources.
//
// A KERNEL FILE (cmake/KatanaSimd.cmake, docs/performance.md "SIMD: kernels
// chosen at run time"): raw pointers only, nothing visible to the linker but the
// katana_avx2_ entries, no includes but intrinsics, <cstddef>, <cstdint> and the
// declarations, and no namespace-scope object with an initialiser.
//
// BIT FOR BIT. The rasteriser promises the same frame on every thread count
// (Rule 7), and the level must not be a second way to break that. So each lane
// repeats its scalar reference in rasterizer.cpp operation for operation, in
// the same order, with the same comparisons - down to which operand a MINPS
// returns - and -ffp-contract=off keeps the compiler from fusing a multiply and
// an add into one rounding; the object check fails the build if it ever did.

#include <immintrin.h>

#include <cstddef>
#include <cstdint>

#include "raster_kernels.hpp"

namespace {

inline __m256d mul(__m256d a, __m256d b) { return _mm256_mul_pd(a, b); }
inline __m256d add(__m256d a, __m256d b) { return _mm256_add_pd(a, b); }
inline __m256d sub(__m256d a, __m256d b) { return _mm256_sub_pd(a, b); }
inline __m256 mul(__m256 a, __m256 b) { return _mm256_mul_ps(a, b); }
inline __m256 add(__m256 a, __m256 b) { return _mm256_add_ps(a, b); }
inline __m256 sub(__m256 a, __m256 b) { return _mm256_sub_ps(a, b); }

// Bit p of the result set for each of the four vertices whose plane-p distance
// is not >= 0 - which a NaN is not - exactly the reference's !(d >= 0).
inline int outside(__m256d distance)
{
    return _mm256_movemask_pd(_mm256_cmp_pd(distance, _mm256_setzero_pd(), _CMP_NGE_UQ));
}

} // namespace

extern "C" void katana_avx2_transform_vertices(const double* mvp, const double* positions,
                                               std::size_t count, const std::uint32_t* colors,
                                               std::size_t colorCount, float width, float height,
                                               float* clip, std::uint8_t* codes, float* screen)
{
    __m256d m[16];
    for (int k = 0; k < 16; ++k) {
        m[k] = _mm256_set1_pd(mvp[k]);
    }
    const __m256d one = _mm256_set1_pd(1.0);
    const __m256d two = _mm256_set1_pd(2.0);
    const __m128 onef = _mm_set1_ps(1.0f);
    const __m128 halff = _mm_set1_ps(0.5f);
    const __m128 widthf = _mm_set1_ps(width);
    const __m128 heightf = _mm_set1_ps(height);

    // Four vertices a step; the caller hands the remainder to the reference.
    for (std::size_t i = 0; i + 4 <= count; i += 4) {
        const double* p = positions + 3 * i;
        const __m256d v0 = _mm256_loadu_pd(p);     // x0 y0 | z0 x1
        const __m256d v1 = _mm256_loadu_pd(p + 4); // y1 z1 | x2 y2
        const __m256d v2 = _mm256_loadu_pd(p + 8); // z2 x3 | y3 z3
        const __m256d a = _mm256_permute2f128_pd(v0, v1, 0x30); // x0 y0 | x2 y2
        const __m256d b = _mm256_permute2f128_pd(v0, v2, 0x21); // z0 x1 | z2 x3
        const __m256d c = _mm256_permute2f128_pd(v1, v2, 0x30); // y1 z1 | y3 z3
        const __m256d x = _mm256_blend_pd(a, b, 0b1010);        // x0 x1 | x2 x3
        const __m256d y = _mm256_shuffle_pd(a, c, 0b0101);      // y0 y1 | y2 y3
        const __m256d z = _mm256_blend_pd(b, c, 0b1010);        // z0 z1 | z2 z3

        // Mat4::multiply with w = 1: (((m0 x + m1 y) + m2 z) + m3 * 1).
        const __m256d cx = add(add(add(mul(m[0], x), mul(m[1], y)), mul(m[2], z)), mul(m[3], one));
        const __m256d cy = add(add(add(mul(m[4], x), mul(m[5], y)), mul(m[6], z)), mul(m[7], one));
        const __m256d cz =
            add(add(add(mul(m[8], x), mul(m[9], y)), mul(m[10], z)), mul(m[11], one));
        const __m256d cw =
            add(add(add(mul(m[12], x), mul(m[13], y)), mul(m[14], z)), mul(m[15], one));

        // To float, rounding to nearest as static_cast<float> does.
        __m128 fx = _mm256_cvtpd_ps(cx);
        __m128 fy = _mm256_cvtpd_ps(cy);
        __m128 fz = _mm256_cvtpd_ps(cz);
        __m128 fw = _mm256_cvtpd_ps(cw);

        // The clip codes, judged in double from the FLOAT coordinates, as the
        // reference's planeDistance is: w - z, then 2w +- x and 2w +- y.
        const __m256d dx = _mm256_cvtps_pd(fx);
        const __m256d dy = _mm256_cvtps_pd(fy);
        const __m256d dz = _mm256_cvtps_pd(fz);
        const __m256d dw = _mm256_cvtps_pd(fw);
        const __m256d band = mul(two, dw);
        const int o0 = outside(sub(dw, dz));
        const int o1 = outside(add(band, dx));
        const int o2 = outside(sub(band, dx));
        const int o3 = outside(add(band, dy));
        const int o4 = outside(sub(band, dy));
        for (int k = 0; k < 4; ++k) {
            codes[i + static_cast<std::size_t>(k)] = static_cast<std::uint8_t>(
                ((o0 >> k) & 1) | (((o1 >> k) & 1) << 1) | (((o2 >> k) & 1) << 2) |
                (((o3 >> k) & 1) << 3) | (((o4 >> k) & 1) << 4));
        }

        // The projection, in float: 1/w, then ((x/w) * 0.5 + 0.5) * width,
        // (0.5 - (y/w) * 0.5) * height and z/w, each product as the reference
        // groups it.
        __m128 invW = _mm_div_ps(onef, fw);
        __m128 sx = _mm_mul_ps(_mm_add_ps(_mm_mul_ps(_mm_mul_ps(fx, invW), halff), halff), widthf);
        __m128 sy = _mm_mul_ps(_mm_sub_ps(halff, _mm_mul_ps(_mm_mul_ps(fy, invW), halff)), heightf);
        __m128 sz = _mm_mul_ps(fz, invW);

        // Back to one vertex a register, and out at five words a vertex.
        _MM_TRANSPOSE4_PS(fx, fy, fz, fw);
        _MM_TRANSPOSE4_PS(sx, sy, sz, invW);
        const __m128 clipRows[4] = {fx, fy, fz, fw};
        const __m128 screenRows[4] = {sx, sy, sz, invW};
        for (std::size_t k = 0; k < 4; ++k) {
            const std::size_t vertex = i + k;
            const std::uint32_t word = vertex < colorCount ? colors[vertex] : 0xFFFFFFFFu;
            float* clipOut = clip + 5 * vertex;
            float* screenOut = screen + 5 * vertex;
            _mm_storeu_ps(clipOut, clipRows[k]);
            _mm_storeu_ps(screenOut, screenRows[k]);
            // The fifth word of each is the colour, a 32-bit word of the
            // caller's struct; stored as the integer it is.
            _mm_storeu_si32(clipOut + 4, _mm_cvtsi32_si128(static_cast<int>(word)));
            _mm_storeu_si32(screenOut + 4, _mm_cvtsi32_si128(static_cast<int>(word)));
        }
    }
}

extern "C" std::size_t katana_avx2_shade_rows(const float* triangle, float invArea,
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

    const __m256 x0 = _mm256_set1_ps(tx[0]);
    const __m256 x1 = _mm256_set1_ps(tx[1]);
    const __m256 x2 = _mm256_set1_ps(tx[2]);
    const __m256 z0 = _mm256_set1_ps(tz[0]);
    const __m256 z1 = _mm256_set1_ps(tz[1]);
    const __m256 z2 = _mm256_set1_ps(tz[2]);
    const __m256 iw0 = _mm256_set1_ps(tw[0]);
    const __m256 iw1 = _mm256_set1_ps(tw[1]);
    const __m256 iw2 = _mm256_set1_ps(tw[2]);
    const __m256 biasV = _mm256_set1_ps(bias);
    const __m256 inv = _mm256_set1_ps(invArea);
    const __m256 zero = _mm256_setzero_ps();
    const __m256 one = _mm256_set1_ps(1.0f);
    const __m256 half = _mm256_set1_ps(0.5f);
    const __m256 top = _mm256_set1_ps(255.0f);
    const __m256i lanes = _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7);
    const __m256i byteMask = _mm256_set1_epi32(0xFF);
    const __m256i fallback = _mm256_set1_epi32(static_cast<int>(words[0]));

    // Each vertex's channels as floats, alpha first as the reference packs.
    __m256 channel[4][3];
    for (int k = 0; k < 4; ++k) {
        const int shift = 24 - 8 * k;
        for (int v = 0; v < 3; ++v) {
            channel[k][v] = _mm256_set1_ps(static_cast<float>((words[v] >> shift) & 0xFFu));
        }
    }

    // Written lanes are counted as -1 each, summed once at the end.
    __m256i written = _mm256_setzero_si256();
    for (int r = 0; r < rowCount; ++r) {
        const int first = spans[2 * r];
        const int last = spans[2 * r + 1];
        if (first > last) {
            continue;
        }
        const int row = firstRow + r;
        const float py = static_cast<float>(row) + 0.5f;
        const __m256 dy0 = _mm256_set1_ps(ty[0] - py);
        const __m256 dy1 = _mm256_set1_ps(ty[1] - py);
        const __m256 dy2 = _mm256_set1_ps(ty[2] - py);
        std::uint32_t* colorRow = color + static_cast<std::size_t>(row) * stride;
        float* depthRow = depth + static_cast<std::size_t>(row) * stride;
        const __m256i end = _mm256_set1_epi32(last + 1);

        for (int x = first; x <= last; x += 8) {
            const __m256i xs = _mm256_add_epi32(_mm256_set1_epi32(x), lanes);
            __m256 mask = _mm256_castsi256_ps(_mm256_cmpgt_epi32(end, xs));
            const __m256 px = add(_mm256_cvtepi32_ps(xs), half);
            const __m256 dx0 = sub(x0, px);
            const __m256 dx1 = sub(x1, px);
            const __m256 dx2 = sub(x2, px);
            const __m256 w0 = mul(sub(mul(dx1, dy2), mul(dx2, dy1)), inv);
            const __m256 w1 = mul(sub(mul(dx2, dy0), mul(dx0, dy2)), inv);
            const __m256 w2 = sub(sub(one, w0), w1);
            if (covered == 0) {
                // Outside when any is < 0; NaN is not < 0, as in the reference.
                const __m256 out = _mm256_or_ps(
                    _mm256_or_ps(_mm256_cmp_ps(w0, zero, _CMP_LT_OQ),
                                 _mm256_cmp_ps(w1, zero, _CMP_LT_OQ)),
                    _mm256_cmp_ps(w2, zero, _CMP_LT_OQ));
                mask = _mm256_andnot_ps(out, mask);
                if (_mm256_movemask_ps(mask) == 0) {
                    continue;
                }
            }
            float* depthAt = depthRow + x;
            const __m256i maskBits = _mm256_castps_si256(mask);
            const __m256 before = _mm256_maskload_ps(depthAt, maskBits);
            // std::min(d, 1.0f) is (1 < d) ? 1 : d, which is MINPS(1, d).
            const __m256 d = _mm256_min_ps(
                one, add(add(add(mul(w0, z0), mul(w1, z1)), mul(w2, z2)), biasV));
            const __m256 pass = _mm256_and_ps(mask, _mm256_cmp_ps(d, before, _CMP_GT_OQ));
            if (_mm256_movemask_ps(pass) == 0) {
                continue;
            }
            const __m256 invW = add(add(mul(w0, iw0), mul(w1, iw1)), mul(w2, iw2));
            const __m256 perspective = _mm256_cmp_ps(invW, zero, _CMP_GT_OQ);
            const __m256 s = _mm256_div_ps(one, invW);
            __m256i packed = _mm256_setzero_si256();
            for (int k = 0; k < 4; ++k) {
                const __m256 value =
                    mul(add(add(mul(mul(w0, channel[k][0]), iw0), mul(mul(w1, channel[k][1]), iw1)),
                            mul(mul(w2, channel[k][2]), iw2)),
                        s);
                // std::clamp(v, 0, 255) is min(max(v, 0), 255): MAXPS(0, v)
                // then MINPS(255, .), which return what those return for NaN.
                const __m256 clamped = _mm256_min_ps(top, _mm256_max_ps(zero, value));
                const __m256i byte =
                    _mm256_and_si256(_mm256_cvttps_epi32(add(clamped, half)), byteMask);
                packed = _mm256_or_si256(packed, _mm256_slli_epi32(byte, 24 - 8 * k));
            }
            const __m256i shaded =
                _mm256_castps_si256(_mm256_blendv_ps(_mm256_castsi256_ps(fallback),
                                                     _mm256_castsi256_ps(packed), perspective));
            const __m256i passBits = _mm256_castps_si256(pass);
            _mm256_maskstore_epi32(reinterpret_cast<int*>(colorRow + x), passBits, shaded);
            if (depthWrite != 0) {
                _mm256_maskstore_ps(depthAt, passBits, d);
            }
            written = _mm256_add_epi32(written, passBits);
        }
    }
    // -1 per written lane: negate the horizontal sum.
    const __m128i sum4 =
        _mm_add_epi32(_mm256_castsi256_si128(written), _mm256_extracti128_si256(written, 1));
    const __m128i sum2 = _mm_add_epi32(sum4, _mm_shuffle_epi32(sum4, 0x4E));
    const __m128i sum1 = _mm_add_epi32(sum2, _mm_shuffle_epi32(sum2, 0xB1));
    return static_cast<std::size_t>(-_mm_cvtsi128_si32(sum1));
}
