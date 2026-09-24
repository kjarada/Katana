// AVX2 point-splat projection. Compiled with -mavx2 -mfma by
// katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_avx2.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_avx2_ entry, no includes but intrinsics, <cstddef>, <cstdint> and
// the declarations.
//
// Four points a step, in double: the display cache stores float offsets from a
// double origin, and widening a float to double is exact, so doing the
// arithmetic in double keeps the only difference from the old all-double splat
// in how the offsets were stored. Eight float lanes were measured no faster in
// the first probe (docs/performance.md): the scatter that follows is what the
// frame waits on.

#include <immintrin.h>

#include <cstddef>
#include <cstdint>

#include "splat_kernels.hpp"

extern "C" void katana_avx2_project_to_pixels(const double* p, const float* xs, const float* ys,
                                              std::size_t count, std::int32_t* px,
                                              std::int32_t* py)
{
    const __m256d halfWidth = _mm256_set1_pd(p[0]);
    const __m256d halfHeight = _mm256_set1_pd(p[1]);
    const __m256d centreX = _mm256_set1_pd(p[2]);
    const __m256d centreY = _mm256_set1_pd(p[3]);
    const __m256d scale = _mm256_set1_pd(p[4]);
    const __m256d low = _mm256_set1_pd(-1.0e6);
    const __m256d high = _mm256_set1_pd(1.0e6);
    const __m128i rejected = _mm_set1_epi32(INT32_MIN);
    // The low 32 bits of each 64-bit compare lane, gathered into one 128-bit
    // mask: an all-ones lane stays all ones.
    const __m256i lowHalves = _mm256_setr_epi32(0, 2, 4, 6, 0, 2, 4, 6);
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const __m256d x = _mm256_cvtps_pd(_mm_loadu_ps(xs + i));
        const __m256d y = _mm256_cvtps_pd(_mm_loadu_ps(ys + i));
        const __m256d sx = _mm256_add_pd(halfWidth, _mm256_mul_pd(_mm256_sub_pd(x, centreX), scale));
        const __m256d sy = _mm256_sub_pd(halfHeight, _mm256_mul_pd(_mm256_sub_pd(y, centreY), scale));
        // Ordered, non-signalling compares: a NaN is outside, as !(a > b)
        // makes it in the scalar reference.
        const __m256d inside = _mm256_and_pd(
            _mm256_and_pd(_mm256_cmp_pd(sx, low, _CMP_GT_OQ), _mm256_cmp_pd(sx, high, _CMP_LT_OQ)),
            _mm256_and_pd(_mm256_cmp_pd(sy, low, _CMP_GT_OQ), _mm256_cmp_pd(sy, high, _CMP_LT_OQ)));
        const __m128i mask = _mm256_castsi256_si128(
            _mm256_permutevar8x32_epi32(_mm256_castpd_si256(inside), lowHalves));
        // CVTTPD2DQ truncates towards zero, as static_cast<int> does.
        _mm_storeu_si128(reinterpret_cast<__m128i*>(px + i),
                         _mm_blendv_epi8(rejected, _mm256_cvttpd_epi32(sx), mask));
        _mm_storeu_si128(reinterpret_cast<__m128i*>(py + i),
                         _mm_blendv_epi8(rejected, _mm256_cvttpd_epi32(sy), mask));
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
