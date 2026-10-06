# Tests written alongside the customisation managers, globbed so that several
# authors can add a file at once without editing this list.
file(GLOB _archive12d_customisation_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp")

katana_add_test_suite(archive12d
    LIBS katana_archive12d
    SOURCES
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
        test_map_file.cpp
        test_customisation.cpp
        ${_archive12d_customisation_tests}
)
# The fixtures are small hand-written archives; the multi-megabyte exports the
# format was learned from are real project data and are not in the repository.
target_compile_definitions(katana_archive12d_tests PRIVATE
    KATANA_ARCHIVE12D_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data")
# The customisation the build compiles in (KATANA_CUSTOMISATION_DIR, set by
# src/katana_archive12d): the one the survey coding work was measured against
# (PLAN.MD 20.3). It is real third-party material under its own licence and is
# NOT part of this repository, so the tests that use it SKIP when it is not
# there rather than fail: the suite must stay green without it.
target_compile_definitions(katana_archive12d_tests PRIVATE
    KATANA_CUSTOMISATION_FILES="${KATANA_CUSTOMISATION_DIR}")
# The converter of the older customisation formats (src/katana_archive12d/legacy):
# customisation/test_convert.cpp calls its functions, so the suite links its
# library - which nothing that ships does.
if(TARGET katana_legacy_customisation)
    target_link_libraries(katana_archive12d_tests PRIVATE katana_legacy_customisation)
endif()
# What the converter is held to: the three small customisations in the Katana
# format, written by hand as the twins of the three fixtures under data/
# customisation. They are in a directory of no one module because the window,
# katana_cli and the widget tests load them too.
target_compile_definitions(katana_archive12d_tests PRIVATE
    KATANA_CUSTOMISATION_TEST_DATA="${PROJECT_SOURCE_DIR}/tests/data/customisation")
# The reference customisation in the legacy formats (the top-level
# CMakeLists.txt says what it is and why it is in no clone): the conversion
# test reads it, SKIPS when the folder is absent or holds no such file, and
# fails when it holds some that are not the four.
target_compile_definitions(katana_archive12d_tests PRIVATE
    KATANA_REFERENCE_CUSTOMISATION_FILES="${KATANA_REFERENCE_CUSTOMISATION_DIR}")
# The program itself: that its command line reaches the functions the suite
# above tests - every option of it, since the functions' tests would pass with
# an option wired to the wrong member or to none - that it writes the file it
# is asked for, and that a failure is exit code 1 with the reason. Everything
# it DOES is tested through the functions. One script, a case a test, each
# with a directory of its own so that they can run at once.
if(TARGET katana_customisation_convert)
    foreach(_case IN ITEMS
            "fixture:the_program_converts_a_fixture_to_its_hand_written_twin"
            "options:every_option_of_the_command_line_reaches_the_conversion"
            "usage:an_unknown_option_or_an_option_without_its_value_is_exit_code_1"
            "unicode:names_and_paths_outside_ascii_are_taken_as_they_were_typed")
        string(REGEX REPLACE ":.*$" "" _case_word "${_case}")
        string(REGEX REPLACE "^[^:]*:" "" _case_name "${_case}")
        add_test(NAME customisation_convert.${_case_name}
            COMMAND ${CMAKE_COMMAND}
                    "-DCASE=${_case_word}"
                    "-DPROGRAM=$<TARGET_FILE:katana_customisation_convert>"
                    "-DFIXTURE=${CMAKE_CURRENT_SOURCE_DIR}/data/customisation/test_symbols.4d"
                    "-DTWIN=${PROJECT_SOURCE_DIR}/tests/data/customisation/test_symbols.customisation.json"
                    "-DSCRATCH=${CMAKE_CURRENT_BINARY_DIR}/converted/${_case_word}"
                    -P "${CMAKE_CURRENT_SOURCE_DIR}/check_convert_program.cmake")
        set_tests_properties(customisation_convert.${_case_name}
            PROPERTIES ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:${KATANA_RUNTIME_BIN}")
    endforeach()
endif()
# The renames the drawing side knows (cad's customisation record) checked
# against the built-in customisation itself. Only where cad is configured too,
# which a module filter can leave out.
if(TARGET katana_cad)
    target_sources(katana_archive12d_tests PRIVATE test_builtin_renames.cpp)
    target_link_libraries(katana_archive12d_tests PRIVATE katana_cad)
endif()
