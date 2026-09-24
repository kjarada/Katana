#pragma once

// The text kernels: a scalar reference for each, and the AVX2 entry point
// compiled from text_avx2.cpp. Declarations only.
//
// The AVX2 entries have C linkage and a katana_avx2_ prefix so that the
// object-file check (tools/check_simd_kernels.cmake) can tell them from
// anything else: in an AVX2 object they are the ONLY symbols allowed to be
// visible to the linker. This header is the one Katana header a kernel file
// may include, and it may include nothing but <cstddef>.

#include <cstddef>

extern "C" {

// Copies the leading run of UTF-16 code units below 0x80 - ASCII - from `in`
// (`units` code units; little-endian when `littleEndian` is non-zero) to `out`,
// one byte each, and returns how many it copied. `out` must have room for
// `units` bytes. Stops at the first unit that is not ASCII, without reading
// beyond `in + 2 * units`.
std::size_t katana_avx2_narrow_ascii_utf16(const unsigned char* in, std::size_t units,
                                           int littleEndian, char* out);

// The length of the leading run of bytes below 0x80 in `bytes[0, size)`.
std::size_t katana_avx2_ascii_prefix(const unsigned char* bytes, std::size_t size);

} // extern "C"

namespace katana::core::kernels {

// The references: what the AVX2 entries must equal, and what a processor
// without AVX2 runs.
std::size_t narrowAsciiUtf16Scalar(const unsigned char* in, std::size_t units, bool littleEndian,
                                   char* out);
std::size_t asciiPrefixScalar(const unsigned char* bytes, std::size_t size);

// Dispatch on core::activeSimdLevel().
std::size_t narrowAsciiUtf16(const unsigned char* in, std::size_t units, bool littleEndian,
                             char* out);
std::size_t asciiPrefix(const unsigned char* bytes, std::size_t size);

} // namespace katana::core::kernels
