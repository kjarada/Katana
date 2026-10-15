# The update engine's tests (src/katana_update, docs/update.md), the two executables
# the end-to-end tests drive, and the checks of the build's own promises about the
# update system: the pinned keys and the helper's imports.
#
# The suite is ONE target, katana_update_tests, so that a task adds a file and no list.
# Every source below is the real thing now: the placeholders the skeleton put here (a
# skipped case each) have all been replaced.

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
        # T9: the helper program's process scan, registry and command line, and the swap driven
        # through the real katana_updater_test (updater_stub.cpp stands in for Katana). Windows
        # only; the other platforms' seams are T18's.
        test_helper.cpp
)
# src/katana_update/credential_detail.hpp, the seams test_credential.cpp holds the stores to.
target_include_directories(katana_update_tests PRIVATE "${PROJECT_SOURCE_DIR}/src/katana_update")
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

# ---- what test_helper.cpp runs (T9) -----------------------------------------------------------------
#
# The shipped helper, the helper with the test key, and a stand-in for Katana that the helper starts
# for its health check and its relaunch (updater_stub.cpp). test_helper.cpp is part of
# katana_update_tests, so the three are built before it. The stub is static like the helper, so
# that it runs from an install with no toolchain on PATH. Never installed.
#
# The stub is a REAL program because a test must never start a file that is not one: Windows
# answers a text file named .exe with a message box somebody has to click. Where a release's own
# katana is such a file (the T3 fixtures'), the test helper is told to start the stub in its place
# (--stand-in, src/katana_updater/main.cpp). And the test helper has no way to the machine's own
# registry keys: its RunOnce entry goes under a key of the test's own, or nowhere.
if(WIN32 AND TARGET katana_updater_test)
    add_executable(katana_updater_stub updater_stub.cpp)
    katana_target_defaults(katana_updater_stub)
    target_link_options(katana_updater_stub PRIVATE ${_katana_updater_link_options})
    set_target_properties(katana_updater_stub PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")
    target_compile_definitions(katana_update_tests PRIVATE
        KATANA_UPDATER_EXE="$<TARGET_FILE:katana_updater>"
        KATANA_UPDATER_TEST_EXE="$<TARGET_FILE:katana_updater_test>"
        KATANA_UPDATER_STUB_EXE="$<TARGET_FILE:katana_updater_stub>")
    add_dependencies(katana_update_tests katana_updater katana_updater_test katana_updater_stub)
endif()

# ---- where a copy of Katana does not replace its own files (PX) -------------------------------------
#
# Linux, until its apply back end is written (task T18: src/katana_update/fsops_posix.cpp and
# spawn_posix.cpp), and macOS for good, where a file changed inside Katana.app breaks the seal of
# the bundle. UPDATE CHECK and UPDATE STATUS work there; a download, a preflight and the helper
# refuse FOR THE PLATFORM and write nothing. Two things hold that, on those systems only (Windows
# compiles neither: its seams are the real ones, with test_journal.cpp and test_helper.cpp):
#
#   test_platform.cpp              the engine and its seams, as cases of katana_update_tests
#   check_platform_refusal.cmake   the helper that ships and the helper with the test key, each
#                                  RUN with its swap line and with --recover: exit 13,
#                                  reason=platform, and the test's folder listed afterwards
#
# When T18 writes the Linux back end the two script tests stay for macOS (if(APPLE)), and
# test_platform.cpp says which of its cases do.
if(NOT WIN32)
    target_sources(katana_update_tests PRIVATE test_platform.cpp)
    foreach(_program IN ITEMS katana_updater katana_updater_test)
        if(TARGET ${_program})
            add_test(NAME ${_program}_refuses_for_the_platform_and_writes_nothing
                COMMAND ${CMAKE_COMMAND} "-DHELPER=$<TARGET_FILE:${_program}>"
                        "-DWORK=${CMAKE_CURRENT_BINARY_DIR}/platform_refusal/${_program}"
                        -P "${CMAKE_CURRENT_SOURCE_DIR}/check_platform_refusal.cmake")
            set_tests_properties(${_program}_refuses_for_the_platform_and_writes_nothing PROPERTIES
                PASS_REGULAR_EXPRESSION "refused a swap and a recovery for the platform, and wrote nothing"
                FAIL_REGULAR_EXPRESSION "CMake Error")
        endif()
    endforeach()
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
# A published RFC 8032 test key (anyone holds its private half) is refused in either slot, and
# with the release switch OFF as well as ON: no build has a reason to pin one. The last case is
# the third published key, which the test executables do not use.
katana_add_update_keys_test(refuses_a_published_test_key_as_the_active_key OFF published_test_key_as_active)
katana_phrase(_phrase "KATANA_UPDATE_KEY_ACTIVE is a published RFC 8032 test key")
set_tests_properties(update_keys_refuses_a_published_test_key_as_the_active_key PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")
katana_add_update_keys_test(refuses_a_published_test_key_as_the_spare_key ON published_test_key_as_spare)
katana_phrase(_phrase "KATANA_UPDATE_KEY_SPARE is a published RFC 8032 test key")
set_tests_properties(update_keys_refuses_a_published_test_key_as_the_spare_key PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")
katana_add_update_keys_test(refuses_the_third_published_test_key OFF published_test_key_three_as_spare)
set_tests_properties(update_keys_refuses_the_third_published_test_key PROPERTIES
    PASS_REGULAR_EXPRESSION "${_phrase}")

# The file states the policy its IN_LIST needs, so that run as a script it needs nothing of the caller:
# a CMake older than 4.0 (the release workflow's first job runs under 3.31.6) starts a script with no
# policy set and stops at IN_LIST. The runs above cannot tell, since CMake 4 sets that policy itself; the
# text can (tests/update/check_keys_policy.cmake), and each way of getting it wrong is shown refused.
set(_katana_keys_policy_check "${CMAKE_CURRENT_SOURCE_DIR}/check_keys_policy.cmake")
add_test(NAME update_keys_file_states_the_policy_its_in_list_needs
    COMMAND ${CMAKE_COMMAND} "-DKEYS=${_katana_keys_script}" -P "${_katana_keys_policy_check}")
set_tests_properties(update_keys_file_states_the_policy_its_in_list_needs PROPERTIES
    PASS_REGULAR_EXPRESSION "states CMP0057 before its first IN_LIST"
    FAIL_REGULAR_EXPRESSION "CMake Error")
function(katana_add_keys_policy_mutation mutation phrase)
    katana_phrase(_regex "${phrase}")
    add_test(NAME update_keys_policy_check_catches_${mutation}
        COMMAND ${CMAKE_COMMAND} "-DKEYS=${_katana_keys_script}" -DMUTATE=${mutation}
                -P "${_katana_keys_policy_check}")
    set_tests_properties(update_keys_policy_check_catches_${mutation} PROPERTIES
        PASS_REGULAR_EXPRESSION "${_regex}")
endfunction()
katana_add_keys_policy_mutation(policy_removed "as an instruction: run as a script under a CMake older than")
katana_add_keys_policy_mutation(policy_in_a_comment "as an instruction: run as a script under a CMake older than")
katana_add_keys_policy_mutation(policy_after_the_use "after its first IN_LIST: the policy has to be set before")
katana_add_keys_policy_mutation(policy_the_wrong_way "sets CMP0057 to OLD, which is the behaviour that stops at IN_LIST")

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
# The import check above speaks for what katana_updater CARRIES, and of the two libraries that
# is OpenSSL alone: the helper checks signatures and never inflates (a release is inflated
# when it is staged, by the program that downloads it), so the linker takes nothing of zlib
# into it and its check would pass on a link line that named zlib's DLL. This executable uses
# what the engine uses - Ed25519 through EVP, raw inflate - and zlib's crc32 as well (the
# engine's own CRC-32 is the journal's, and it calls none of zlib's), through the same third-party
# target and the same link options as the helper, so the import check on IT is a check of the
# whole link line (a static OpenSSL and a static zlib, nothing of the toolchain's DLLs), and
# running it is a check that the static libraries work. Never installed.
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
# because a skipped unittest exits 0 and would pass as quietly as one that ran. The tests
# need Python 3.10 or later (sys.stdlib_module_names holds the verifier to the standard library).
set(_katana_tool_tests_ready OFF)
if(Python3_Interpreter_FOUND AND Python3_VERSION VERSION_GREATER_EQUAL 3.10)
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
            TIMEOUT 1800)
    endforeach()
    # Not a tool's test: the RFC 8032 arithmetic that derived the literals of test_signature.cpp. Run so that
    # it keeps working (it exits non-zero when it no longer reproduces the RFC's own vectors).
    add_test(NAME update_tools_rfc8032_oracle
        COMMAND "${KATANA_PYTHON}" -I -B "${CMAKE_CURRENT_SOURCE_DIR}/tool_tests/rfc8032_oracle.py")
    set_tests_properties(update_tools_rfc8032_oracle PROPERTIES TIMEOUT 120)
endif()

# ---- the Windows installer is per user (cmake/NSIS.template.in) -----------------------------
#
# Decision D1 (docs/update.md): the installer asks for no administrator rights, installs under
# %LOCALAPPDATA%\Programs\Katana and writes HKCU only, so that the updater can replace files in
# it as the same user. tests/update/check_nsis_script.cmake holds the SCRIPT's text to that:
#
#   packaging_nsis_template_is_per_user     the template configured with sample values; on every
#                                           host, since it needs nothing but cmake
#   packaging_nsis_check_catches_*          the same check on that text changed in one way that
#                                           breaks one rule: a check that cannot see a violation
#                                           passes as quietly as one that finds none. The last
#                                           case is CPack's own stock template, which asks for
#                                           administrator rights and writes machine-wide
#   packaging_nsis_installer_builds_from_the_real_configuration
#                                           the build's real CPackConfig.cmake over a small tree,
#                                           makensis included: the script CPack generated is
#                                           checked and the installer is compiled (Windows with
#                                           NSIS, which is where `package` makes an installer)
#
# and tests/update/run_nsis_uninstaller.cmake RUNS the part of the uninstaller that the text
# cannot vouch for, the reader of the installed manifest and the filter in front of each delete:
#
#   packaging_nsis_uninstaller_removes_what_is_listed_and_nothing_else
#                                           the template's own four functions, compiled into a
#                                           throwaway installer that writes nothing but its
#                                           uninstaller, over three small trees with hostile
#                                           manifest entries; the tree that is left is compared
#                                           with the one worked out by hand (Windows with NSIS)
#   packaging_nsis_uninstaller_test_catches_*
#                                           the same with one rule of those functions broken: the
#                                           test must name the file the broken rule left or took.
#                                           Each requires the test above as a fixture: a broken
#                                           rule says something only where the unbroken functions
#                                           pass, and a machine that will not run the throwaway
#                                           programs then fails that one test after one wait, and
#                                           ctest reports these as not run
#
# Those throwaway programs write under their own folder of the build tree only: no registry, no
# Start menu, and a stand-in for LOCALAPPDATA. No test here runs Katana's installer, because that
# writes the user's registry and Start menu; an install, an update and an uninstall on a clean
# Windows machine (silent, and as a person runs them) are the acceptance step of
# docs/release.md, "The Windows installer".
set(_katana_nsis_template "${PROJECT_SOURCE_DIR}/cmake/NSIS.template.in")
set(_katana_nsis_check "${CMAKE_CURRENT_SOURCE_DIR}/check_nsis_script.cmake")
# Where the helper names its RunOnce value, which the uninstaller must remove under that name.
set(_katana_nsis_registry_header "${PROJECT_SOURCE_DIR}/include/katana/update/registry.hpp")
add_test(NAME packaging_nsis_template_is_per_user
    COMMAND ${CMAKE_COMMAND} "-DTEMPLATE=${_katana_nsis_template}"
            "-DREGISTRY_HEADER=${_katana_nsis_registry_header}" -P "${_katana_nsis_check}")
set_tests_properties(packaging_nsis_template_is_per_user PROPERTIES
    PASS_REGULAR_EXPRESSION "Installer script [(]the template with sample values[)]: per user"
    FAIL_REGULAR_EXPRESSION "CMake Error")
function(katana_add_nsis_mutation mutation phrase)
    katana_phrase(_regex "${phrase}")
    add_test(NAME packaging_nsis_check_catches_${mutation}
        COMMAND ${CMAKE_COMMAND} "-DTEMPLATE=${_katana_nsis_template}"
                "-DREGISTRY_HEADER=${_katana_nsis_registry_header}" -DMUTATE=${mutation}
                -P "${_katana_nsis_check}")
    set_tests_properties(packaging_nsis_check_catches_${mutation} PROPERTIES
        PASS_REGULAR_EXPRESSION "${_regex}")
endfunction()
katana_add_nsis_mutation(administrator "RequestExecutionLevel admin: the installer must run as the user")
katana_add_nsis_mutation(second_level "has 2 RequestExecutionLevel instructions")
katana_add_nsis_mutation(machine_registry "HKLM: the installer writes machine-wide")
katana_add_nsis_mutation(shell_context "SetShellVarContext all: the Start menu")
katana_add_nsis_mutation(program_files "[$]PROGRAMFILES: a folder only an administrator can write to")
katana_add_nsis_mutation(no_update_folder_removal "the uninstaller does not remove the updater's working folder")
katana_add_nsis_mutation(no_manifest_removal "does not remove the files the installed manifest lists")
katana_add_nsis_mutation(install_folder_removed_recursively "RMDir /r \"[$]INSTDIR\": recursive removal")
katana_add_nsis_mutation(install_folder_wildcard_delete "a wildcard delete in the install folder")
katana_add_nsis_mutation(silent_prompt "no /SD, so a silent install [(]/S[)] would wait for an answer")
katana_add_nsis_mutation(no_install_location "the entry in Settings > Apps: the script has no")
katana_add_nsis_mutation(no_folder_record "the install folder, for the updater: the script has no")
katana_add_nsis_mutation(ansi "a Unicode installer: the script has no")
katana_add_nsis_mutation(keys_stay_behind "does not remove its own entry in Settings > Apps")
# A running program's file must stop the install, not be skipped (which a silent run would do unseen).
katana_add_nsis_mutation(file_in_use_skipped "a file in use stops the install and is not skipped: the script has no")
# An instruction behind a comment that ends in a backslash is no instruction to NSIS.
katana_add_nsis_mutation(comment_takes_the_next_line "a line with a comment character in it ends in a backslash")
# Installed over a copy that an update changed: the updater's record and working folder go.
katana_add_nsis_mutation(stale_record_survives_a_reinstall
    "the installer does not clear the updater's record of the version of a copy it installs over")
katana_add_nsis_mutation(stale_journal_survives_a_reinstall
    "the installer does not clear the working folder of an update that was cut short")
# Outside the install folder: the updater's state folder and no other, less the person's choices.
katana_add_nsis_mutation(state_folder_is_another_folder "the updater's state folder, which is the only one")
katana_add_nsis_mutation(update_choices_removed "the person's choices about updating, which the uninstaller keeps")
katana_add_nsis_mutation(run_once_under_another_name
    "does not remove the helper's RunOnce value under the name the helper gives it")
# What NSIS does not compile is no instruction: a block comment, a condition the check cannot
# evaluate, one condition inside another. A condition of the template's own kind that is false
# is read as NSIS reads it, so the call inside it is not found and the rule that asks for the
# call is the one that speaks.
katana_add_nsis_mutation(block_comment_hides_an_instruction "/[*]: a block comment")
katana_add_nsis_mutation(unknown_condition_hides_an_instruction "!if 0: a condition this check cannot evaluate")
katana_add_nsis_mutation(false_condition_hides_an_instruction "does not remove the files the installed manifest lists")
katana_add_nsis_mutation(condition_inside_a_condition "one condition inside another")
# The installed manifest is read before it is deleted, or nothing an update added is removed.
katana_add_nsis_mutation(manifest_deleted_before_it_is_read
    "comes before Call un[.]DeleteManifestFiles: the list would be gone before it is read")
# The folder: none at all is refused, a silent run tries it too, a reinstall is offered the same one.
katana_add_nsis_mutation(no_folder_at_all_is_installed_into
    "no folder at all is refused, like one that cannot be written to: the script has no")
katana_add_nsis_mutation(silent_install_does_not_try_the_folder
    "the installer's section does not begin with Call CheckInstallFolder")
katana_add_nsis_mutation(installed_copy_is_not_offered_again
    "a copy that is already installed is offered as the folder again: the script has no")
katana_add_nsis_mutation(uninstaller_runs_without_a_folder
    "the uninstaller's section does not begin by stopping when its folder is not known")
# An entry in Settings > Apps whose uninstaller was deleted by hand must not stop the install.
katana_add_nsis_mutation(uninstaller_that_is_gone_stops_the_install
    "runs the uninstaller that the entry in Settings > Apps names without looking for it first")
# The Desktop shortcut (cmake/KatanaPackaging.cmake, CPACK_NSIS_CREATE_ICONS_EXTRA and _DELETE_): one, to the
# Start menu's program under its name and with its icon, starting in the install folder, made inside the
# Start menu page's block (so that "Do not create shortcuts" leaves it out) and removed by name, and the
# Desktop used for nothing else. The unbroken script passes in packaging_nsis_template_is_per_user above.
katana_add_nsis_mutation(desktop_shortcut_not_made "the installer makes no shortcut on the Desktop")
katana_add_nsis_mutation(desktop_shortcut_not_removed "the uninstaller does not remove the shortcut on the Desktop")
katana_add_nsis_mutation(desktop_shortcut_to_another_program
    "the Desktop shortcut goes to [$]INSTDIR.bin.katana_cli[.]exe, and must go to the program")
katana_add_nsis_mutation(desktop_shortcut_with_an_icon_of_its_own
    "a shortcut on the Desktop must name its file and the program and nothing after them")
katana_add_nsis_mutation(desktop_shortcut_ignores_do_not_create_shortcuts
    "the Desktop shortcut is not between the two macros of the Start menu page")
katana_add_nsis_mutation(desktop_shortcut_starts_in_another_folder
    "the Desktop shortcut does not start in the install folder")
katana_add_nsis_mutation(desktop_shortcut_removed_by_wildcard
    "the uninstaller takes from the Desktop what is not Katana's shortcut")
katana_add_nsis_mutation(desktop_shortcut_under_another_name
    "the Desktop shortcut and the Start menu one are not the same, Katana CAD[.]lnk")
katana_add_nsis_mutation(desktop_is_used_for_something_else "the script names [$]DESKTOP 3 times")
# The case no mutation can stand for: the template this one replaces, as CMake ships it. It is
# the installer Katana had (administrator rights, HKLM, Program Files); the check must refuse it
# on its own terms. Where CMake keeps it differs between versions, so it is only looked for.
set(_katana_cpack_stock_template "${CMAKE_ROOT}/Modules/Internal/CPack/NSIS.template.in")
if(EXISTS "${_katana_cpack_stock_template}")
    add_test(NAME packaging_nsis_check_catches_cpacks_own_template
        COMMAND ${CMAKE_COMMAND} "-DTEMPLATE=${_katana_cpack_stock_template}" -P "${_katana_nsis_check}")
    katana_phrase(_regex "RequestExecutionLevel admin: the installer must run as the user")
    set_tests_properties(packaging_nsis_check_catches_cpacks_own_template PROPERTIES
        PASS_REGULAR_EXPRESSION "${_regex}")
endif()
if(WIN32)
    # The search cmake/KatanaPackaging.cmake makes, made here too: that file is included after
    # the tests are added, so on a first configure its answer is not in the cache yet, and the
    # tests below would be missing from that configure without a word.
    find_program(KATANA_MAKENSIS NAMES makensis HINTS "${KATANA_RUNTIME_BIN}")
endif()
if(WIN32 AND KATANA_MAKENSIS)
    add_test(NAME packaging_nsis_installer_builds_from_the_real_configuration
        COMMAND ${CMAKE_COMMAND}
                "-DCPACK_COMMAND=${CMAKE_CPACK_COMMAND}"
                "-DCPACK_CONFIG=${CMAKE_BINARY_DIR}/CPackConfig.cmake"
                "-DWORK=${CMAKE_CURRENT_BINARY_DIR}/nsis_package"
                "-DCHECK=${_katana_nsis_check}"
                "-DREGISTRY_HEADER=${_katana_nsis_registry_header}"
                "-DEXPECT_VERSION=${PROJECT_VERSION}"
                -P "${CMAKE_CURRENT_SOURCE_DIR}/build_nsis_package.cmake")
    set_tests_properties(packaging_nsis_installer_builds_from_the_real_configuration PROPERTIES
        PASS_REGULAR_EXPRESSION "Installer built from the real configuration"
        FAIL_REGULAR_EXPRESSION "CMake Error"
        TIMEOUT 300)

    # The uninstaller's file removal, run. Each test has a folder of its own (they run side by
    # side) and names no folder outside it. NSIS takes the folder as the unquoted end of a
    # command line, which CMake cannot give it when the path has a space
    # (run_nsis_uninstaller.cmake says more), so a build tree with one has no such tests, and
    # says so here rather than failing them.
    set(_katana_nsis_uninstaller_work "${CMAKE_CURRENT_BINARY_DIR}/nsis_uninstaller")
    if(_katana_nsis_uninstaller_work MATCHES " ")
        message(STATUS "Installer tests: ${_katana_nsis_uninstaller_work} has a space in it; "
                       "the tests that run the uninstaller's file removal are not registered")
    else()
        set(_katana_nsis_uninstaller_run
            ${CMAKE_COMMAND} "-DTEMPLATE=${_katana_nsis_template}" "-DMAKENSIS=${KATANA_MAKENSIS}")
        add_test(NAME packaging_nsis_uninstaller_removes_what_is_listed_and_nothing_else
            COMMAND ${_katana_nsis_uninstaller_run} "-DWORK=${_katana_nsis_uninstaller_work}/as_it_is"
                    -P "${CMAKE_CURRENT_SOURCE_DIR}/run_nsis_uninstaller.cmake")
        # A fixture: the tests of the broken rules below run only where this one passes. The
        # script gives each program it starts a minute and stops at the first that does not
        # come back, so 300 s is never reached by waiting on a program that is held.
        set_tests_properties(packaging_nsis_uninstaller_removes_what_is_listed_and_nothing_else PROPERTIES
            PASS_REGULAR_EXPRESSION "Uninstaller file removal: the manifest's files and the updater's staged files went"
            FAIL_REGULAR_EXPRESSION "CMake Error"
            FIXTURES_SETUP packaging_nsis_uninstaller_runs
            TIMEOUT 300)
        # What each broken rule must be seen to do, named by the file it leaves or takes: a
        # listed file that stays (the rule that lets a release folder through is broken) or a
        # decoy that goes (the rule that keeps a hostile path out is).
        function(katana_add_nsis_uninstaller_mutation mutation phrase)
            katana_phrase(_regex "${phrase}")
            add_test(NAME packaging_nsis_uninstaller_test_catches_${mutation}
                COMMAND ${_katana_nsis_uninstaller_run} "-DWORK=${_katana_nsis_uninstaller_work}/${mutation}"
                        -DMUTATE=${mutation} -P "${CMAKE_CURRENT_SOURCE_DIR}/run_nsis_uninstaller.cmake")
            set_tests_properties(packaging_nsis_uninstaller_test_catches_${mutation} PROPERTIES
                PASS_REGULAR_EXPRESSION "${_regex}"
                FIXTURES_REQUIRED packaging_nsis_uninstaller_runs
                TIMEOUT 300)
        endfunction()
        katana_add_nsis_uninstaller_mutation(first_folder_length "left, should have gone: install/bin/katana[.]exe")
        katana_add_nsis_uninstaller_mutation(etc_not_accepted "left, should have gone: install/etc/ssl/certs/ca-bundle[.]crt")
        katana_add_nsis_uninstaller_mutation(slash_kept "left, should have gone: install/lib/libx[.]a")
        katana_add_nsis_uninstaller_mutation(no_dotdot_rule "gone, should have stayed: install/victim-dotdot[.]txt")
        katana_add_nsis_uninstaller_mutation(no_dot_rule "gone, should have stayed: install/bin/dot-target[.]dll")
        katana_add_nsis_uninstaller_mutation(no_empty_component_rule "gone, should have stayed: install/bin/double[.]dll")
        katana_add_nsis_uninstaller_mutation(wildcard_allowed "gone, should have stayed: install/bin/wild2[.]txt")
        katana_add_nsis_uninstaller_mutation(high_byte_allowed "gone, should have stayed: install/bin/hi")
        katana_add_nsis_uninstaller_mutation(state_json_removed
            "under LOCALAPPDATA: gone, should have stayed: Katana/update/state[.]json")
    endif()
endif()
