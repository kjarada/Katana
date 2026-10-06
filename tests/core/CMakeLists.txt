katana_add_test_suite(core
    LIBS katana_core
    SOURCES
        test_core.cpp
        test_cpu_features.cpp
        test_library_data.cpp
        test_path_text.cpp
        test_simd_facade.cpp
        test_simd_text.cpp
        test_text.cpp
        test_text_encoding.cpp
        test_xml.cpp
)
# Which kernel set the build compiled, so test_cpu_features can hold a NEON
# build to detecting Neon: on AArch64 there is no probe that could say no.
if(KATANA_SIMD_KERNEL_SET STREQUAL "neon")
    target_compile_definitions(katana_core_tests PRIVATE KATANA_TEST_SIMD_KERNEL_SET_NEON=1)
endif()
