// NEON packing of a draw list's vertices for the GPU, for 64-bit ARM.
// Compiled by katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_neon.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_neon_ entry, no includes but <arm_neon.h>, <cstddef>, <cstdint>
// and the declarations.
//
// Four vertices a step, as pack_avx2.cpp takes them. Each LD3 splits two
// vertices' six doubles into x, y and z registers; each lane is the
// scalar loop's own subtraction of the origin, rounded to float by FCVTN, which
// rounds to nearest (the FPCR default) as static_cast<float> does. ST4 then
// interleaves x, y, z and the colour words - stored as the bits they are,
// never converted - into four whole gpu::GpuVertex records.

#include <arm_neon.h>

#include <cstddef>
#include <cstdint>

#include "pack_kernels.hpp"

extern "C" void katana_neon_pack_vertices(const double* positions, const std::uint32_t* colors,
                                          std::size_t count, const double* origin, void* out)
{
    auto* words = static_cast<float*>(out);
    const float64x2_t ox = vdupq_n_f64(origin[0]);
    const float64x2_t oy = vdupq_n_f64(origin[1]);
    const float64x2_t oz = vdupq_n_f64(origin[2]);
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        const float64x2x3_t a = vld3q_f64(positions + 3 * i);     // vertices i, i + 1
        const float64x2x3_t b = vld3q_f64(positions + 3 * i + 6); // vertices i + 2, i + 3
        float32x4x4_t vertex;
        vertex.val[0] = vcombine_f32(vcvt_f32_f64(vsubq_f64(a.val[0], ox)),
                                     vcvt_f32_f64(vsubq_f64(b.val[0], ox)));
        vertex.val[1] = vcombine_f32(vcvt_f32_f64(vsubq_f64(a.val[1], oy)),
                                     vcvt_f32_f64(vsubq_f64(b.val[1], oy)));
        vertex.val[2] = vcombine_f32(vcvt_f32_f64(vsubq_f64(a.val[2], oz)),
                                     vcvt_f32_f64(vsubq_f64(b.val[2], oz)));
        vertex.val[3] = vreinterpretq_f32_u32(vld1q_u32(colors + i));
        vst4q_f32(words + 4 * i, vertex);
    }
    // The last one to three, one at a time.
    for (; i < count; ++i) {
        const double* p = positions + 3 * i;
        const float xyz[3] = {static_cast<float>(p[0] - origin[0]),
                              static_cast<float>(p[1] - origin[1]),
                              static_cast<float>(p[2] - origin[2])};
        __builtin_memcpy(words + 4 * i, xyz, sizeof xyz);
        __builtin_memcpy(words + 4 * i + 3, colors + i, sizeof(std::uint32_t));
    }
}
