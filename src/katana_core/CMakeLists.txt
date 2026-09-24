add_library(katana_core STATIC
    cpu_features.cpp
    log.cpp
    task_pool.cpp
    text.cpp
    text_encoding.cpp
    xml.cpp
    simd/text_kernels.cpp
)
katana_target_defaults(katana_core)
# The AVX2 text kernels: see cmake/KatanaSimd.cmake for why they are a file of
# their own and what they may contain.
katana_add_simd_sources(katana_core AVX2 simd/text_avx2.cpp)
