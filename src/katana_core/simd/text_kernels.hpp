#pragma once

// The text kernels: a scalar reference for each, the AVX2 entry points
// compiled from text_avx2.cpp and the NEON ones from text_neon.cpp.
// Declarations only.
//
// The entries have C linkage and a katana_avx2_ or katana_neon_ prefix so that
// the object-file check (tools/check_simd_kernels.cmake) can tell them from
// anything else: in a kernel object they are the ONLY symbols allowed to be
// visible to the linker. This header is the one Katana header a kernel file
// may include, and it may include nothing but <cstddef>. Both sets are
// declared on every machine; only the set the build compiled is defined, and
// the dispatch names it only under KATANA_HAVE_AVX2_KERNELS or
// KATANA_HAVE_NEON_KERNELS.

#include <cstddef>

extern "C" {

// Copies the leading run of UTF-16 code units below 0x80 - ASCII - from `in`
// (`units` code units; little-endian when `littleEndian` is non-zero) to `out`,
// one byte each, and returns how many it copied. `out` must have room for
// `units` bytes. Stops at the first unit that is not ASCII, without reading
// beyond `in + 2 * units`.
std::size_t katana_avx2_narrow_ascii_utf16(const unsigned char* in, std::size_t units,
                                           int littleEndian, char* out);

// Whether `bytes[0, size)` is well-formed UTF-8: 1 if it is, 0 if not, and -1,
// having read nothing, when `size` is under 64 - its last block would begin
// before the text. Reads nothing outside `bytes[0, size)`.
int katana_avx2_valid_utf8(const unsigned char* bytes, std::size_t size);

// The same two, 16 bytes a register (text_neon.cpp), with the same contracts:
// the NEON validator also works in 64-byte steps and also returns -1 under 64.
std::size_t katana_neon_narrow_ascii_utf16(const unsigned char* in, std::size_t units,
                                           int littleEndian, char* out);
int katana_neon_valid_utf8(const unsigned char* bytes, std::size_t size);

} // extern "C"

namespace katana::core::kernels {

// The references: what the kernel entries must equal, and what a processor
// without kernels runs. (The validators' reference is the byte loop in
// text_encoding.cpp, kept there so that isValidUtf8 on a name costs no call.)
std::size_t narrowAsciiUtf16Scalar(const unsigned char* in, std::size_t units, bool littleEndian,
                                   char* out);

// Dispatch on core::activeSimdLevel().
std::size_t narrowAsciiUtf16(const unsigned char* in, std::size_t units, bool littleEndian,
                             char* out);

} // namespace katana::core::kernels
