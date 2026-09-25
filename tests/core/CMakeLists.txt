katana_add_test_suite(core
    LIBS katana_core
    SOURCES
        test_core.cpp
        test_cpu_features.cpp
        test_library_data.cpp
        test_simd_facade.cpp
        test_simd_text.cpp
        test_text.cpp
        test_text_encoding.cpp
        test_xml.cpp
)
