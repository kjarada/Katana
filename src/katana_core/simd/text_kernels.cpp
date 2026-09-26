// The scalar references of the text kernels and the dispatch between them and
// the AVX2 and NEON entries. Compiled for the baseline, like everything that is
// not a kernel file.

#include "text_kernels.hpp"

#include "katana/core/cpu_features.hpp"

namespace katana::core::kernels {

std::size_t narrowAsciiUtf16Scalar(const unsigned char* in, std::size_t units, bool littleEndian,
                                   char* out)
{
    std::size_t i = 0;
    for (; i < units; ++i) {
        const unsigned first = in[2 * i];
        const unsigned second = in[2 * i + 1];
        const unsigned unit = littleEndian ? (first | (second << 8)) : ((first << 8) | second);
        if (unit >= 0x80) {
            break;
        }
        out[i] = static_cast<char>(unit);
    }
    return i;
}

std::size_t narrowAsciiUtf16(const unsigned char* in, std::size_t units, bool littleEndian,
                             char* out)
{
#if defined(KATANA_HAVE_AVX2_KERNELS)
    if (activeSimdLevel() == SimdLevel::Avx2) {
        return katana_avx2_narrow_ascii_utf16(in, units, littleEndian ? 1 : 0, out);
    }
#elif defined(KATANA_HAVE_NEON_KERNELS)
    if (activeSimdLevel() == SimdLevel::Neon) {
        return katana_neon_narrow_ascii_utf16(in, units, littleEndian ? 1 : 0, out);
    }
#endif
    return narrowAsciiUtf16Scalar(in, units, littleEndian, out);
}

} // namespace katana::core::kernels
