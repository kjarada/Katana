// AVX2 text kernels. Compiled with -mavx2 -mfma by katana_add_simd_sources.
//
// A KERNEL FILE: raw pointers in, raw pointers out, and nothing with external
// linkage but the katana_avx2_ entries. It includes only intrinsics, <cstddef>,
// <cstdint> and its declarations, because any inline function from a shared
// header compiled here would come out as AVX2 code that the linker may hand to
// a baseline caller (docs/performance.md, "The inline-copy hazard").
// tools/check_simd_kernels.cmake enforces both rules, on the source and on the
// object file.

#include <immintrin.h>

#include <cstddef>
#include <cstdint>

#include "text_kernels.hpp"

namespace {

// 32 code units, 64 bytes: two 256-bit loads that pack into one 256-bit store.
constexpr std::size_t kUnitsPerStep = 32;

inline __m256i load(const unsigned char* at)
{
    return _mm256_loadu_si256(reinterpret_cast<const __m256i*>(at));
}

} // namespace

extern "C" std::size_t katana_avx2_narrow_ascii_utf16(const unsigned char* in, std::size_t units,
                                                      int littleEndian, char* out)
{
    // A unit is ASCII when its value is below 0x80. Read as 16-bit
    // little-endian lanes, a little-endian unit is the lane itself, so any bit
    // of 0xFF80 set means "not ASCII"; a big-endian unit has its bytes swapped
    // in the lane, so the bits to test are 0x80FF.
    const __m256i notAscii = _mm256_set1_epi16(
        static_cast<short>(littleEndian != 0 ? 0xFF80 : 0x80FF));
    std::size_t i = 0;
    for (; i + kUnitsPerStep <= units; i += kUnitsPerStep) {
        __m256i low = load(in + 2 * i);
        __m256i high = load(in + 2 * i + 32);
        if (_mm256_testz_si256(_mm256_or_si256(low, high), notAscii) == 0) {
            break; // this block holds a non-ASCII unit: found below, one at a time
        }
        if (littleEndian == 0) {
            low = _mm256_srli_epi16(low, 8);
            high = _mm256_srli_epi16(high, 8);
        }
        // Every lane is now 0-0x7F, so the saturating pack is an exact
        // narrowing. It packs within 128-bit halves - [low 0-7, high 0-7 |
        // low 8-15, high 8-15] - and the permute (0xD8: quarters 0, 2, 1, 3)
        // puts the 32 bytes back in order.
        const __m256i packed = _mm256_permute4x64_epi64(_mm256_packus_epi16(low, high), 0xD8);
        _mm256_storeu_si256(reinterpret_cast<__m256i*>(out + i), packed);
    }
    // The tail, or the block that held a non-ASCII unit up to that unit.
    for (; i < units; ++i) {
        const unsigned first = in[2 * i];
        const unsigned second = in[2 * i + 1];
        const unsigned unit = littleEndian != 0 ? (first | (second << 8)) : ((first << 8) | second);
        if (unit >= 0x80) {
            break;
        }
        out[i] = static_cast<char>(unit);
    }
    return i;
}

extern "C" std::size_t katana_avx2_ascii_prefix(const unsigned char* bytes, std::size_t size)
{
    std::size_t i = 0;
    // 64 bytes a step: the OR of two loads has its top bit set in some byte
    // exactly when one of the 64 bytes is 0x80 or above.
    for (; i + 64 <= size; i += 64) {
        const __m256i either = _mm256_or_si256(load(bytes + i), load(bytes + i + 32));
        if (_mm256_movemask_epi8(either) != 0) {
            break;
        }
    }
    for (; i + 32 <= size; i += 32) {
        if (_mm256_movemask_epi8(load(bytes + i)) != 0) {
            break;
        }
    }
    while (i < size && bytes[i] < 0x80) {
        ++i;
    }
    return i;
}
