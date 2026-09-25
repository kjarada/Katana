// AVX2 packing of a draw list's vertices for the GPU. Compiled with
// -mavx2 -mfma by katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_avx2.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_avx2_ entry, no includes but intrinsics, <cstddef>, <cstdint> and
// the declarations.
//
// Four vertices a step. Their twelve doubles are three registers,
//   A = x0 y0 z0 x1,  B = y1 z1 x2 y2,  C = z2 x3 y3 z3,
// so the origin is laid out in the same pattern and subtracted lane by lane -
// each lane the scalar loop's own subtraction - then each register is rounded
// to four floats (CVTPD2PS, round to nearest as static_cast<float>). Each
// vertex's x, y, z are then brought to lanes 0-2 by one byte rotation and its
// colour blended into lane 3: the 16-byte gpu::GpuVertex, stored whole.

#include <immintrin.h>

#include <cstddef>
#include <cstdint>

#include "pack_kernels.hpp"

extern "C" void katana_avx2_pack_vertices(const double* positions, const std::uint32_t* colors,
                                          std::size_t count, const double* origin, void* out)
{
    auto* bytes = static_cast<unsigned char*>(out);
    const __m256d originA = _mm256_setr_pd(origin[0], origin[1], origin[2], origin[0]);
    const __m256d originB = _mm256_setr_pd(origin[1], origin[2], origin[0], origin[1]);
    const __m256d originC = _mm256_setr_pd(origin[2], origin[0], origin[1], origin[2]);
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const double* p = positions + 3 * i;
        const __m128i a = _mm_castps_si128(
            _mm256_cvtpd_ps(_mm256_sub_pd(_mm256_loadu_pd(p), originA))); // x0 y0 z0 x1
        const __m128i b = _mm_castps_si128(
            _mm256_cvtpd_ps(_mm256_sub_pd(_mm256_loadu_pd(p + 4), originB))); // y1 z1 x2 y2
        const __m128i c = _mm_castps_si128(
            _mm256_cvtpd_ps(_mm256_sub_pd(_mm256_loadu_pd(p + 8), originC))); // z2 x3 y3 z3
        const __m128i ink = _mm_loadu_si128(reinterpret_cast<const __m128i*>(colors + i));

        const __m128i v1 = _mm_alignr_epi8(b, a, 12); // x1 y1 z1 .
        const __m128i v2 = _mm_alignr_epi8(c, b, 8);  // x2 y2 z2 .
        const __m128i v3 = _mm_srli_si128(c, 4);      // x3 y3 z3 .
        auto* at = reinterpret_cast<__m128i*>(bytes + 16 * i);
        _mm_storeu_si128(at + 0,
                         _mm_blend_epi32(a, _mm_shuffle_epi32(ink, _MM_SHUFFLE(0, 0, 0, 0)), 0x8));
        _mm_storeu_si128(at + 1,
                         _mm_blend_epi32(v1, _mm_shuffle_epi32(ink, _MM_SHUFFLE(1, 1, 1, 1)), 0x8));
        _mm_storeu_si128(at + 2,
                         _mm_blend_epi32(v2, _mm_shuffle_epi32(ink, _MM_SHUFFLE(2, 2, 2, 2)), 0x8));
        _mm_storeu_si128(at + 3,
                         _mm_blend_epi32(v3, _mm_shuffle_epi32(ink, _MM_SHUFFLE(3, 3, 3, 3)), 0x8));
    }
    // The last one to three, one at a time, reading no double past the last
    // vertex's z: its x and y as a pair, its z alone.
    for (; i < count; ++i) {
        const double* p = positions + 3 * i;
        const __m128d xy = _mm_sub_pd(_mm_loadu_pd(p), _mm_loadu_pd(origin));
        const __m128d z = _mm_sub_sd(_mm_load_sd(p + 2), _mm_load_sd(origin + 2));
        const __m128 xyz = _mm_movelh_ps(_mm_cvtpd_ps(xy), _mm_cvtpd_ps(z)); // x y z 0
        const __m128i vertex =
            _mm_insert_epi32(_mm_castps_si128(xyz), static_cast<int>(colors[i]), 3);
        _mm_storeu_si128(reinterpret_cast<__m128i*>(bytes + 16 * i), vertex);
    }
}
