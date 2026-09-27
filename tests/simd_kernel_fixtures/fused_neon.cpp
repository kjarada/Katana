// A kernel the object check must refuse: it fuses a multiply and an add
// (tests/CMakeLists.txt, simd_kernel_objects_catches_a_fused_multiply_add).
// The intrinsic, not a contracted expression, so that every optimisation
// level produces the instruction.

#include <arm_neon.h>

#include <cstddef>

extern "C" void katana_neon_fixture_fused(double* a, const double* b, const double* c,
                                          std::size_t count)
{
    for (std::size_t i = 0; i + 2 <= count; i += 2) {
        vst1q_f64(a + i, vfmaq_f64(vld1q_f64(c + i), vld1q_f64(a + i), vld1q_f64(b + i)));
    }
}
