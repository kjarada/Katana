# The update engine's tests (src/katana_update, docs/update.md), the two executables
# the end-to-end tests drive, and the checks of the build's own promises about the
# update system: the pinned keys and the helper's imports.
#
# The suite is ONE target, katana_update_tests, so that each task adds a file and no
# list: every source below exists as a placeholder (a skipped case) or as the real thing
# the task replaces it with.

katana_add_test_suite(update
    LIBS katana_update
    SOURCES
        # What the skeleton promises (T1): the version reaches the library, the test keys,
        # the seams a fake can implement.
        test_skeleton.cpp
        # The pure functions of the contract the skeleton finished itself (src/katana_update/
        # contract.cpp, feed.cpp's addresses, arguments.cpp), and the constants of the headers.
        # No later task replaces this file.
        test_contract.cpp
        # The test trust root (RFC 8032 vectors 1 and 2). In three executables, never installed.
        test_key.cpp
        # T4: versions, the manifest, the path rules.
        test_version.cpp
        test_manifest.cpp
        test_paths.cpp
        # T5: Ed25519, the trust gate.
        test_signature.cpp
        test_trust.cpp
        # T6: the planner, inflate, the range merger, the records.
        test_plan.cpp
        test_inflate.cpp
        test_ranges.cpp
        test_state.cpp
        # T7: the journal, the swap, recovery, the three crash loops.
        test_journal.cpp
        test_apply.cpp
        test_recover.cpp
        test_fault_loops.cpp
        # T13: the credential stores.
        test_credential.cpp
        # T8: the feed, check, stage and preflight.
        test_feed.cpp
        test_check.cpp
        test_stage.cpp
)
target_compile_definitions(katana_update_tests PRIVATE
    # The version the project declares, to hold the library's KATANA_VERSION to.
    KATANA_EXPECTED_VERSION="${PROJECT_VERSION}"
    # The release trees, manifests and signed fixtures of tools/update_make_manifest.py (T3).
    KATANA_UPDATE_FIXTURES="${CMAKE_CURRENT_SOURCE_DIR}/fixtures")

# ---- the two executables the end-to-end tests drive (tests/update/update_e2e.py) -----------
#
# Neither is installed (tools/check_install_rules.cmake refuses them by name), and each
# exists because the production programs cannot be given another trust root: the engine and
# the helper take their keys as a PARAMETER, and only these pass a test key.
#
#   katana_update_e2e   the engine from a command line: check, plan, stage, preflight, launch
#                       (e2e_main.cpp, T12)
#   katana_updater_test the helper's own main.cpp, the same source, compiled with
#                       KATANA_UPDATER_TEST_BUILD: the test key instead of the pinned ones, and
#                       --die-at N (a FaultFs). The same link options, so what the tests run
#                       is what ships, less the keys.
add_executable(katana_update_e2e e2e_main.cpp test_key.cpp)
katana_target_defaults(katana_update_e2e)
target_link_libraries(katana_update_e2e PRIVATE katana_update katana_core)
set_target_properties(katana_update_e2e PROPERTIES RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")

get_property(_katana_updater_main GLOBAL PROPERTY KATANA_UPDATER_MAIN)
get_property(_katana_updater_link_options GLOBAL PROPERTY KATANA_UPDATER_LINK_OPTIONS)
if(_katana_updater_main)
    add_executable(katana_updater_test "${_katana_updater_main}" test_key.cpp)
    katana_target_defaults(katana_updater_test)
    target_compile_definitions(katana_updater_test PRIVATE KATANA_UPDATER_TEST_BUILD=1)
    target_link_libraries(katana_updater_test PRIVATE katana_update katana_core)
    target_link_options(katana_updater_test PRIVATE ${_katana_updater_link_options})
    set_target_properties(katana_updater_test PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")
endif()

# ---- the pinned keys (cmake/KatanaUpdateKeys.cmake) ----------------------------------
#
# The module is the configure's own check run as a script, so the refusals are proved on
# fixtures: keys that are not the ones in the tree. A check that cannot see a violation
# passes as quietly as one that finds none.
#
# CMake wraps a message that has no leading blank at 80 columns, at any space, so a
# phrase a test looks for can be broken across two lines. The same phrase, with a run of
# spaces and line breaks wherever it has one space.
function(katana_phrase out text)
    string(REPLACE " " "[ \r\n]+" _phrase "${text}")
    set(${out} "${_phrase}" PARENT_SCOPE)
endfunction()
set(_katana_keys_script "${PROJECT_SOURCE_DIR}/cmake/KatanaUpdateKeys.cmake")
function(katana_add_update_keys_test name require fixture)
    set(_command ${CMAKE_COMMAND} -DKATANA_REQUIRE_UPDATE_KEYS=${require})
    if(fixture)
        list(APPEND _command "-DKATANA_UPDATE_KEYS_FIXTURE=${CMAKE_CURRENT_SOURCE_DIR}/key_fixtures/${fixture}.cmake")
    endif()
    add_test(NAME update_keys_${name} COMMAND ${_command} -P "${_katana_keys_script}")
endfunction()
# The file in the tree is well formed, whatever it holds (OFF: placeholders are allowed here).
katana_add_update_keys_test(file_in_the_tree_is_well_formed OFF "")
set_tests_properties(update_keys_file_in_the_tree_is_well_formed PROPERTIES
    PASS_REGULAR_EXPRESSION "Update keys: (PLACEHOLDERS|pinned)"
    FAIL_REGULAR_EXPRESSION "CMake Error")
# OFF, the default, lets the placeholders through and says loudly that they are there.
katana_add_update_keys_test(placeholders_are_allowed_when_not_required OFF placeholders)
set_tests_properties(update_keys_placeholders_are_allowed_when_not_required PROPERTIES
    PASS_REGULAR_EXPRESSION "Update keys: PLACEHOLDERS - this build can neither verify nor receive an update"
    FAIL_REGULAR_EXPRESSION "CMake Error")
# ON refuses them: this is what stops a release from shipping with keys nobody holds.
katana_add_update_keys_test(required_refuses_the_placeholders ON placeholders)
katana_phrase(_phrase "KATANA_REQUIRE_UPDATE_KEYS is ON, but cmake/KatanaUpdateKeys.cmake still holds the PLACEHOLDER update keys")
set_tests_properties(update_keys_required_refuses_the_placeholders PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")
# ... and ON accepts two real-looking keys: the refusal is about the placeholders, not about ON.
katana_add_update_keys_test(required_accepts_real_looking_keys ON real_looking)
set_tests_properties(update_keys_required_accepts_real_looking_keys PROPERTIES
    PASS_REGULAR_EXPRESSION "Update keys: pinned, active 97fcb0a317920f5d, spare 393a4825f86253ba"
    FAIL_REGULAR_EXPRESSION "CMake Error")
katana_add_update_keys_test(refuses_a_key_one_digit_short OFF malformed)
katana_phrase(_phrase "KATANA_UPDATE_KEY_ACTIVE must be 64 lower-case hexadecimal digits")
set_tests_properties(update_keys_refuses_a_key_one_digit_short PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")
katana_add_update_keys_test(refuses_a_spare_that_is_the_active_key OFF one_key_twice)
katana_phrase(_phrase "the spare key must be a different key from the active key")
set_tests_properties(update_keys_refuses_a_spare_that_is_the_active_key PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")
katana_add_update_keys_test(refuses_a_flag_that_says_real_over_placeholder_bytes ON
    flag_says_real_over_placeholder_bytes)
katana_phrase(_phrase "says the keys are real, but they spell PLACEHOLDER")
set_tests_properties(update_keys_refuses_a_flag_that_says_real_over_placeholder_bytes PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")
katana_add_update_keys_test(refuses_a_flag_that_says_placeholder_over_real_keys OFF
    flag_says_placeholder_over_real_keys)
katana_phrase(_phrase "says the keys are placeholders, but they do not spell PLACEHOLDER")
set_tests_properties(update_keys_refuses_a_flag_that_says_placeholder_over_real_keys PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")

# ---- the helper's imports (tests/update/check_updater_imports.cmake) -----------------------
#
# The check, on a listing of a static program and on one that imports the GCC runtime,
# which it must name; and on the two real programs where objdump can read them. PE only.
add_test(NAME updater_import_check_accepts_a_static_listing
    COMMAND ${CMAKE_COMMAND}
            "-DLISTING=${CMAKE_CURRENT_SOURCE_DIR}/import_fixtures/static.txt"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/check_updater_imports.cmake")
set_tests_properties(updater_import_check_accepts_a_static_listing PROPERTIES
    PASS_REGULAR_EXPRESSION "The helper imports 9 DLLs, all of them the operating system's"
    FAIL_REGULAR_EXPRESSION "CMake Error")
add_test(NAME updater_import_check_names_the_gcc_runtime
    COMMAND ${CMAKE_COMMAND}
            "-DLISTING=${CMAKE_CURRENT_SOURCE_DIR}/import_fixtures/gcc_runtime.txt"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/check_updater_imports.cmake")
katana_phrase(_phrase "not the operating system's: libgcc_s_seh-1[.]dll, libstdc[+][+]-6[.]dll, libwinpthread-1[.]dll")
set_tests_properties(updater_import_check_names_the_gcc_runtime PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")
if(WIN32 AND CMAKE_OBJDUMP)
    foreach(_program IN ITEMS katana_updater katana_updater_test)
        if(TARGET ${_program})
            add_test(NAME ${_program}_imports_only_os_dlls
                COMMAND ${CMAKE_COMMAND} "-DOBJDUMP=${CMAKE_OBJDUMP}" "-DPROGRAM=$<TARGET_FILE:${_program}>"
                        -P "${CMAKE_CURRENT_SOURCE_DIR}/check_updater_imports.cmake")
            set_tests_properties(${_program}_imports_only_os_dlls PROPERTIES
                PASS_REGULAR_EXPRESSION "all of them the operating system's"
                FAIL_REGULAR_EXPRESSION "CMake Error")
        endif()
    endforeach()
endif()

# ---- the link, proved (tests/update/link_probe.cpp) --------------------------------------
#
# While katana_updater is a stub it uses neither OpenSSL nor zlib, so the import check above
# passes on ANY link line. This executable uses what the engine will - Ed25519 through EVP,
# raw inflate, crc32 - through the same third-party target and the same link options as the
# helper, so the import check on IT is a check of the link line (a static OpenSSL and zlib,
# nothing of the toolchain's DLLs), and running it is a check that the static libraries work.
# Never installed.
add_executable(katana_update_link_probe link_probe.cpp)
katana_target_defaults(katana_update_link_probe)
target_link_libraries(katana_update_link_probe PRIVATE katana_update_third_party)
if(_katana_updater_link_options)
    target_link_options(katana_update_link_probe PRIVATE ${_katana_updater_link_options})
endif()
set_target_properties(katana_update_link_probe PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")
add_test(NAME update_link_probe_verifies_ed25519_inflates_and_checksums
    COMMAND katana_update_link_probe)
if(WIN32 AND CMAKE_OBJDUMP)
    add_test(NAME katana_update_link_probe_imports_only_os_dlls
        COMMAND ${CMAKE_COMMAND} "-DOBJDUMP=${CMAKE_OBJDUMP}"
                "-DPROGRAM=$<TARGET_FILE:katana_update_link_probe>"
                -P "${CMAKE_CURRENT_SOURCE_DIR}/check_updater_imports.cmake")
    set_tests_properties(katana_update_link_probe_imports_only_os_dlls PROPERTIES
        PASS_REGULAR_EXPRESSION "all of them the operating system's"
        FAIL_REGULAR_EXPRESSION "CMake Error")
endif()

# ---- no shipped program carries a test key (tests/update/check_no_test_keys.cmake) ---------
#
# trust.hpp promises that the trust root is a parameter and only the test executables pass a
# test key. This looks at the bytes: the four programs that ship must contain none of the
# RFC 8032 keys of test_key.hpp, and the two test executables that DO link them are the
# controls, which fail the check unless it sees the keys - so "absent" is not a scan that
# cannot see. The key list is read from test_key.hpp, so there is one.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/test_key.hpp" _katana_test_key_header)
string(REGEX MATCHALL "\"[0-9a-f]+\"" _katana_quoted_hex "${_katana_test_key_header}")
set(_katana_test_keys "")
foreach(_quoted IN LISTS _katana_quoted_hex)
    string(REGEX REPLACE "\"" "" _hex "${_quoted}")
    string(LENGTH "${_hex}" _length)
    if(_length EQUAL 64)
        list(APPEND _katana_test_keys "${_hex}")
    endif()
endforeach()
list(LENGTH _katana_test_keys _katana_test_key_count)
if(NOT _katana_test_key_count EQUAL 4)
    message(FATAL_ERROR "tests/update/test_key.hpp should hold four 64-digit keys (two seeds, "
        "two public keys); found ${_katana_test_key_count}. check_no_test_keys.cmake reads them from there.")
endif()
list(JOIN _katana_test_keys "," _katana_test_keys_argument)
# The four programs that ship, as a file a path a line, so that the test's command names none
# of them: a test whose command names katana, katana_cli or katana_mcp is judged to RUN it, and
# this one only reads bytes (tools/check_program_tests.cmake explains).
set(_katana_shipped_programs "")
foreach(_program IN ITEMS katana katana_cli katana_mcp katana_updater)
    if(TARGET ${_program})
        string(APPEND _katana_shipped_programs "$<TARGET_FILE:${_program}>\n")
    endif()
endforeach()
file(GENERATE OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/shipped_programs_$<CONFIG>.txt"
    CONTENT "${_katana_shipped_programs}")
add_test(NAME update_test_keys_are_in_no_shipped_program
    COMMAND ${CMAKE_COMMAND}
            "-DPROGRAM_LIST=${CMAKE_CURRENT_BINARY_DIR}/shipped_programs_$<CONFIG>.txt"
            "-DKEYS=${_katana_test_keys_argument}" -DEXPECT=absent
            -P "${CMAKE_CURRENT_SOURCE_DIR}/check_no_test_keys.cmake")
set_tests_properties(update_test_keys_are_in_no_shipped_program PROPERTIES
    PASS_REGULAR_EXPRESSION "No test key in" FAIL_REGULAR_EXPRESSION "CMake Error")
# And the check on a list that DOES hold a program with the keys: it must refuse, naming it.
if(TARGET katana_updater_test)
    file(GENERATE OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/a_program_with_test_keys_$<CONFIG>.txt"
        CONTENT "$<TARGET_FILE:katana_updater_test>\n")
    add_test(NAME update_test_keys_check_refuses_a_list_holding_a_program_that_has_them
        COMMAND ${CMAKE_COMMAND}
                "-DPROGRAM_LIST=${CMAKE_CURRENT_BINARY_DIR}/a_program_with_test_keys_$<CONFIG>.txt"
                "-DKEYS=${_katana_test_keys_argument}" -DEXPECT=absent
                -P "${CMAKE_CURRENT_SOURCE_DIR}/check_no_test_keys.cmake")
    katana_phrase(_phrase "A shipped program contains a test key")
    set_tests_properties(update_test_keys_check_refuses_a_list_holding_a_program_that_has_them
        PROPERTIES PASS_REGULAR_EXPRESSION "${_phrase}")
endif()
foreach(_control IN ITEMS katana_update_e2e katana_updater_test)
    if(TARGET ${_control})
        add_test(NAME update_test_keys_control_is_found_in_${_control}
            COMMAND ${CMAKE_COMMAND} "-DPROGRAM=$<TARGET_FILE:${_control}>"
                    "-DKEYS=${_katana_test_keys_argument}" -DEXPECT=present
                    -P "${CMAKE_CURRENT_SOURCE_DIR}/check_no_test_keys.cmake")
        set_tests_properties(update_test_keys_control_is_found_in_${_control} PROPERTIES
            PASS_REGULAR_EXPRESSION "Control: [0-9]+ of 8 forms found" FAIL_REGULAR_EXPRESSION "CMake Error")
    endif()
endforeach()

# ---- the end-to-end tests, and the loopback fake feed (T12) --------------------------------
#
# update_e2e.py drives tools/update_fake_feed.py (stdlib http.server, port 0, proxies off)
# and the two executables above; CLAUDE.md, section 4, says a test may start its own server
# on 127.0.0.1. Registered by e2e.cmake, beside this file, so that task adds them without
# editing this one (it needs Python, which is optional: Python3_Interpreter_FOUND).
file(GLOB _katana_update_e2e_tests CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/e2e*.cmake")
foreach(_katana_update_e2e_test IN LISTS _katana_update_e2e_tests)
    include("${_katana_update_e2e_test}")
endforeach()

# ---- the release tools, tested (tests/update/tool_tests, T3) -------------------------------
#
# tools/update_make_manifest.py, update_keytool.py, update_verify_release.py,
# update_release_sign.py and update_make_fixtures.py are the producer, the owner's key step,
# the independent verifier, the offline signer and the fixture generator. Their tests are
# stdlib unittest (no pytest), each file one ctest so that --parallel runs them side by side,
# and they judge the tools with oracles outside them: Python's zipfile, hashlib, the RFC 8032
# vectors and the openssl command line (3.0 or later: pkeyutl -rawin).
#
# Python and openssl are build-time conveniences, never requirements (Rule 1): without either
# there are no such tests rather than failing ones. They are not registered as skipped,
# because a skipped unittest exits 0 and would pass as quietly as one that ran.
set(_katana_tool_tests_ready OFF)
if(Python3_Interpreter_FOUND)
    find_program(KATANA_OPENSSL_EXECUTABLE openssl)
    if(KATANA_OPENSSL_EXECUTABLE)
        execute_process(COMMAND "${KATANA_OPENSSL_EXECUTABLE}" version
            OUTPUT_VARIABLE _katana_openssl_version RESULT_VARIABLE _katana_openssl_status
            OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
        set(_katana_openssl_major 0)
        if(_katana_openssl_status EQUAL 0 AND _katana_openssl_version MATCHES "^OpenSSL ([0-9]+)[.]")
            set(_katana_openssl_major ${CMAKE_MATCH_1})
        endif()
        if(_katana_openssl_major GREATER_EQUAL 3)
            set(_katana_tool_tests_ready ON)
        else()
            message(STATUS "Update tool tests: ${KATANA_OPENSSL_EXECUTABLE} is '${_katana_openssl_version}', "
                           "not OpenSSL 3 or later; the release tools' tests are not registered")
        endif()
    else()
        message(STATUS "Update tool tests: no openssl on PATH; the release tools' tests are not registered")
    endif()
endif()
if(_katana_tool_tests_ready)
    foreach(_katana_tool_test IN ITEMS make_manifest keytool verify_release release_sign fixtures)
        add_test(NAME update_tools_${_katana_tool_test}
            COMMAND "${KATANA_PYTHON}" -B "${CMAKE_CURRENT_SOURCE_DIR}/tool_tests/test_${_katana_tool_test}.py")
        set_tests_properties(update_tools_${_katana_tool_test} PROPERTIES
            ENVIRONMENT_MODIFICATION
                "KATANA_OPENSSL=set:${KATANA_OPENSSL_EXECUTABLE};KATANA_CMAKE=set:${CMAKE_COMMAND}"
            TIMEOUT 600)
    endforeach()
endif()
