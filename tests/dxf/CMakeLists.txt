# The native DXF module (src/katana_dxf). The fixtures are small drawings
# written by hand, one R12 and one R2000; every value the tests expect of them
# is worked out in the test beside it.
katana_add_test_suite(dxf
    LIBS katana_dxf
    SOURCES
        test_codes.cpp
        test_reader.cpp
        test_writer.cpp
        test_robustness.cpp
)
target_compile_definitions(katana_dxf_tests PRIVATE
    KATANA_DXF_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data")
