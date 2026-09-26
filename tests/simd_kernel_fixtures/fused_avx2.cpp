// A kernel the object check must refuse: it fuses a multiply and an add
// (tests/CMakeLists.txt, simd_kernel_objects_catches_a_fused_multiply_add).
// The intrinsic, not a contracted expression, so that every optimisation
// level produces the instruction.

#include <immintrin.h>

#include <cstddef>

extern "C" void katana_avx2_fixture_fused(double* a, const double* b, const double* c,
                                          std::size_t count)
{
    for (std::size_t i = 0; i + 4 <= count; i += 4) {
        _mm256_storeu_pd(a + i, _mm256_fmadd_pd(_mm256_loadu_pd(a + i), _mm256_loadu_pd(b + i),
                                                _mm256_loadu_pd(c + i)));
    }
}
