katana_add_test_suite(archive12d
    LIBS katana_archive12d
    SOURCES
        test_text_encoding.cpp
        test_reader.cpp
        test_elements.cpp
        test_writer.cpp
        test_coverage.cpp
        test_domain_import.cpp
        test_domain_alignment.cpp
        test_domain_export.cpp
        test_review_findings.cpp
        test_symbols.cpp
        test_meshes.cpp
        test_text.cpp
        test_style_library.cpp
)
# The fixtures are small hand-written archives; the multi-megabyte exports the
# format was learned from are real project data and are not in the repository.
target_compile_definitions(katana_archive12d_tests PRIVATE
    KATANA_ARCHIVE12D_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data")
# The Transport for NSW customisation the survey coding work was built against
# (PLAN.MD 20.3). It is a real customisation carrying its author's licence
# notice, so the tests that use it SKIP when it is not there rather than fail:
# the suite must stay green in a checkout that does not include it.
target_compile_definitions(katana_archive12d_tests PRIVATE
    KATANA_12D_REFERENCE_FILES="${CMAKE_SOURCE_DIR}/docs/12d Refrence Files")
