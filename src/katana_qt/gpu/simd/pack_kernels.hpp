#pragma once

// Entry points of the AVX2 packing of a draw list's vertices for the GPU,
// compiled from pack_avx2.cpp, and of its NEON twin, from pack_neon.cpp.
// Declarations only; see src/katana_core/simd/text_kernels.hpp for why they
// have C linkage and a katana_avx2_ or katana_neon_ prefix.
//
// It equals the scalar loop in gpu_scene.cpp (packDrawList) bit for bit: each
// offset is the scalar's double subtraction, rounded to float by CVTPD2PS,
// which rounds as static_cast<float> does - to nearest, under the default
// MXCSR - and nothing is fused.

#include <cstddef>
#include <cstdint>

extern "C" {

// For each i < count, with positions[3i .. 3i+2] the vertex's x, y and z:
//   the words of out[16i .. 16i+15] (a gpu::GpuVertex) are
//   float(x - origin[0]), float(y - origin[1]), float(z - origin[2]), colors[i].
void katana_avx2_pack_vertices(const double* positions, const std::uint32_t* colors,
                               std::size_t count, const double* origin, void* out);

// The same, rounding each double to float with FCVTN (to nearest, under the
// default FPCR).
void katana_neon_pack_vertices(const double* positions, const std::uint32_t* colors,
                               std::size_t count, const double* origin, void* out);

} // extern "C"
