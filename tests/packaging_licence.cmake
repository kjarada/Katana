# The Windows installer shows the licence (docs/release.md, "The Windows installer";
# tools/check_installer_licence.cmake says what is held). Included from
# tests/CMakeLists.txt beside the other packaging tests.
#
# The first test runs the check on this build's own configure: the CPackConfig.cmake
# it wrote, the sources, the install rules. The others hand the check a configure that
# is wrong in one way each and read its refusal, so that a check which stopped looking
# would be seen to: the installer had no licence page for a release, and nothing noticed.
#
# Windows only, because the installer is. A build without NSIS passes the first test
# having checked the template only (the check says so).
if(NOT WIN32)
    return()
endif()

add_test(NAME packaging_installer_shows_the_licence
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR} -DKATANA_BINARY=${CMAKE_BINARY_DIR}
            -P ${PROJECT_SOURCE_DIR}/tools/check_installer_licence.cmake)

set(_licence_fixture_root "${CMAKE_CURRENT_BINARY_DIR}/installer_licence_fixtures")

# One fixture: a build directory (CPackConfig.cmake, cmake_install.cmake) and a source
# directory (the template, LICENSE, ADDITIONAL_TERMS.md), each right unless said wrong.
#   NO_LICENCE_FILE  CPackConfig.cmake does not set CPACK_RESOURCE_FILE_LICENSE
#   SHORT_DOUBLED_PAGE  it names a page that is a note with doubled line ends and no licence
#   NO_PAGE_IN_TEMPLATE  the template has no @CPACK_NSIS_LICENSE_PAGE@
#   NO_NOTICE_RULES  cmake_install.cmake installs none of the notices
function(katana_licence_fixture name)
    cmake_parse_arguments(ARG "NO_LICENCE_FILE;SHORT_DOUBLED_PAGE;NO_PAGE_IN_TEMPLATE;NO_NOTICE_RULES" "" "" ${ARGN})
    set(_dir "${_licence_fixture_root}/${name}")
    file(REMOVE_RECURSE "${_dir}")
    file(MAKE_DIRECTORY "${_dir}/cmake")
    file(COPY "${PROJECT_SOURCE_DIR}/LICENSE" "${PROJECT_SOURCE_DIR}/ADDITIONAL_TERMS.md"
         DESTINATION "${_dir}")
    file(READ "${PROJECT_SOURCE_DIR}/cmake/NSIS.template.in" _template)
    if(ARG_NO_PAGE_IN_TEMPLATE)
        string(REPLACE "@CPACK_NSIS_LICENSE_PAGE@" "" _template "${_template}")
    endif()
    file(WRITE "${_dir}/cmake/NSIS.template.in" "${_template}")

    set(_config "set(CPACK_GENERATOR \"ZIP;NSIS\")\n")
    if(ARG_SHORT_DOUBLED_PAGE)
        # CR LF written through a text-mode write is CR CR LF on Windows: the doubled line ends
        # this once made of the real page (a blank line after every line).
        file(WRITE "${_dir}/page.txt"
            "Katana, copyright (c) 2026 Jarada, https://github.com/kjarada/Katana\r\nonly a note\r\n")
    endif()
    if(NOT ARG_NO_LICENCE_FILE)
        string(APPEND _config "set(CPACK_RESOURCE_FILE_LICENSE \"${_dir}/page.txt\")\n")
        if(NOT ARG_SHORT_DOUBLED_PAGE)
            # A page that is complete, made the way cmake/KatanaPackaging.cmake makes it.
            file(READ "${PROJECT_SOURCE_DIR}/LICENSE" _gpl)
            file(READ "${PROJECT_SOURCE_DIR}/ADDITIONAL_TERMS.md" _terms)
            set(_page "Katana, copyright (c) 2026 Jarada, https://github.com/kjarada/Katana\n\nYou do not have to accept these terms.\n\n${_gpl}\n\nAdditional terms\n\n${_terms}")
            # LF only: file(WRITE) is a text-mode write and ends each line CRLF on Windows, as
            # cmake/KatanaPackaging.cmake relies on to make the real page.
            string(REPLACE "\r\n" "\n" _page "${_page}")
            file(WRITE "${_dir}/page.txt" "${_page}")
        endif()
    endif()
    file(WRITE "${_dir}/CPackConfig.cmake" "${_config}")

    set(_install "# A top-level install script, as CMake writes it.\n")
    if(NOT ARG_NO_NOTICE_RULES)
        foreach(_notice ADDITIONAL_TERMS.md LICENSING.md THIRD_PARTY_NOTICES.md)
            string(APPEND _install "file(INSTALL DESTINATION \"\${CMAKE_INSTALL_PREFIX}/share/katana/licences\" TYPE FILE FILES \"${PROJECT_SOURCE_DIR}/${_notice}\")\n")
        endforeach()
    endif()
    file(WRITE "${_dir}/cmake_install.cmake" "${_install}")
endfunction()

# What the check must say for each, and that it is the fixture's one fault that it names.
function(katana_licence_refusal test fixture refusal)
    add_test(NAME packaging_installer_licence_check_${test}
        COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${_licence_fixture_root}/${fixture}
                -DKATANA_BINARY=${_licence_fixture_root}/${fixture}
                -P ${PROJECT_SOURCE_DIR}/tools/check_installer_licence.cmake)
    set_tests_properties(packaging_installer_licence_check_${test} PROPERTIES
        PASS_REGULAR_EXPRESSION "${refusal}")
endfunction()

# The fixtures that are right, so that a refusal below is the one fault and not a check that
# refuses everything: this one has no fault at all and must pass.
katana_licence_fixture(complete)
add_test(NAME packaging_installer_licence_check_passes_a_complete_configure
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${_licence_fixture_root}/complete
            -DKATANA_BINARY=${_licence_fixture_root}/complete
            -P ${PROJECT_SOURCE_DIR}/tools/check_installer_licence.cmake)
set_tests_properties(packaging_installer_licence_check_passes_a_complete_configure PROPERTIES
    PASS_REGULAR_EXPRESSION "the page holds the licence and the additional terms"
    FAIL_REGULAR_EXPRESSION "is not as it must be")

katana_licence_fixture(no_licence_file NO_LICENCE_FILE)
katana_licence_refusal(refuses_a_cpack_that_names_no_licence_file no_licence_file
    "does not set CPACK_RESOURCE_FILE_LICENSE")

katana_licence_fixture(a_short_doubled_page SHORT_DOUBLED_PAGE)
katana_licence_refusal(refuses_a_page_with_doubled_line_ends_and_no_licence a_short_doubled_page
    "does not end every line in CRLF alone.*does not hold all of LICENSE")

katana_licence_fixture(no_page_in_the_template NO_PAGE_IN_TEMPLATE)
katana_licence_refusal(refuses_a_template_that_shows_no_licence_page no_page_in_the_template
    "must show the licence page")

katana_licence_fixture(no_notice_rules NO_NOTICE_RULES)
katana_licence_refusal(refuses_notices_no_install_rule_names no_notice_rules
    "no install rule puts ADDITIONAL_TERMS.md in the package")
