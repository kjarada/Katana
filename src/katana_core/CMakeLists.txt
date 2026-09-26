add_library(katana_core STATIC
    cpu_features.cpp
    library_data.cpp
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
# std::thread, for TaskPool and for every module above that starts one. PUBLIC
# so each of them links it: before glibc 2.34 pthread_create is in a library
# of its own, and the Linux build targets glibc 2.28 (docs/release.md), where
# a program that does not name it fails to link.
find_package(Threads REQUIRED)
target_link_libraries(katana_core PUBLIC Threads::Threads)
