# The end-to-end tests of the update system, and the test of the fake feed they fetch from (T12).
# Included by tests/update/CMakeLists.txt (it globs e2e*.cmake) once katana_update_e2e,
# katana_updater_test and katana_updater_stub exist.
#
#   update_e2e_program_...   katana_update_e2e by itself: its usage, and that it cannot leave the machine
#   update_tools_fake_feed   tools/update_fake_feed.py asked with urllib (tool_tests/test_fake_feed.py)
#   update_e2e_driver_...    the driver's own readers, held to answers worked out by hand
#   update_e2e_<sentence>    one case of tests/update/update_e2e.py each: the real producer makes and
#                            signs two releases, the fake feed serves them on 127.0.0.1, and
#                            katana_update_e2e and katana_updater_test are the engine and the helper
#   update_e2e_every_case_of_the_driver_is_registered, and
#   update_every_end_to_end_test_has_the_runtime_first_on_its_path_and_a_timeout
#                            the two checks of this file itself
#
# They reach the loopback address and nothing else (CLAUDE.md, section 4: a test may start its own
# server on 127.0.0.1, port 0, with proxies disabled), so they run without KATANA_ONLINE_TESTS. The
# proxy variables are taken away and NO_PROXY names the loopback: Python's urllib reads them, and a
# request to 127.0.0.1 must never be handed to a proxy. Every folder a case writes is under the
# system's temporary folder and is removed when the case ends.
#
# Python is a build-time convenience, never a requirement: without it (3.10 or later, as the release
# tools' tests need) nothing here is registered - not registered as skipped, since a skipped test
# passes as quietly as one that ran. The end-to-end cases also need openssl 3 for the producer's
# signature, like the release tools' tests.

# katana_update_e2e speaks to the feed over plain sockets (e2e_main.cpp). The update library needs
# ws2_32 too, but only as a private need of its static libcrypto: the program says what it uses.
if(WIN32)
    target_link_libraries(katana_update_e2e PRIVATE ws2_32)
endif()

# katana_update_e2e is linked like the test suites and not like the helper: it is not static, and
# loads the C++ runtime (libstdc++, libgcc) from the toolchain's folder. So every test that starts
# it - by itself, below, or through the driver, which hands its own environment on to the programs
# it runs - has that folder first on PATH, whatever shell ctest was started from. In a Git Bash
# whose PATH puts another MinGW first the program would load the wrong runtime and die before main
# (0xc0000139), and with none on PATH it would not start at all (tests/CMakeLists.txt says the same
# of the application's own checks). The helper and the stand-in for Katana need nothing: they are
# linked static (CMakeLists.txt, beside this file).
set(_katana_e2e_runtime "PATH=path_list_prepend:${KATANA_RUNTIME_BIN}")

# A test added below without that passes only where the caller's own PATH happens to be right, and
# one without a TIMEOUT holds the run for ctest's default 1,500 seconds when a program waits. So a
# test names every `update_e2e_...` test that lacks either. The scan is deferred to the end of the
# directory, after the last set_tests_properties - and past every `return()` of this file, which
# leaves early where there is no Python or no openssl.
function(katana_name_update_e2e_tests_without_their_environment)
    get_property(_tests DIRECTORY PROPERTY TESTS)
    set(_bare "")
    set(_seen 0)
    foreach(_test IN LISTS _tests)
        if(NOT _test MATCHES "^update_e2e_")
            continue()
        endif()
        math(EXPR _seen "${_seen} + 1")
        get_test_property(${_test} ENVIRONMENT_MODIFICATION _environment)
        get_test_property(${_test} TIMEOUT _timeout)
        if(NOT _environment MATCHES "(^|;)PATH=path_list_prepend:" OR NOT _timeout)
            list(APPEND _bare ${_test})
        endif()
    endforeach()
    if(_bare)
        string(JOIN ", " _named ${_bare})
    else()
        set(_named "none")
    endif()
    add_test(NAME update_every_end_to_end_test_has_the_runtime_first_on_its_path_and_a_timeout
        COMMAND ${CMAKE_COMMAND} -E echo
            "Of ${_seen} end-to-end tests, those without the runtime on PATH or without a timeout: ${_named}.")
    set_tests_properties(update_every_end_to_end_test_has_the_runtime_first_on_its_path_and_a_timeout
        PROPERTIES PASS_REGULAR_EXPRESSION "Of [1-9][0-9]* end-to-end tests, .*without a timeout: none[.]")
endfunction()
cmake_language(DEFER CALL katana_name_update_e2e_tests_without_their_environment)

# ---- the program's own rule, with no feed and no Python ------------------------------------------
#
# katana_update_e2e is what every case below fetches with, so that it cannot leave the machine is a
# promise worth a test of its own (e2e_main.cpp, "THE WIRE REACHES 127.0.0.1 AND NOTHING ELSE"): a
# feed that is not `http://127.0.0.1` is refused on the command line, a name is never looked up
# (localhost included), and GitHub's own addresses are not connected to unless --connect says which
# loopback port stands in for the host. None of these opens a socket. The install they name is the
# T3 fixtures' (read, never written); the state folder is never made.
add_test(NAME update_e2e_program_prints_its_usage_when_given_no_command COMMAND katana_update_e2e)
set_tests_properties(update_e2e_program_prints_its_usage_when_given_no_command PROPERTIES
    PASS_REGULAR_EXPRESSION "usage: katana_update_e2e check[|]plan[|]stage"
    ENVIRONMENT_MODIFICATION "${_katana_e2e_runtime}"
    TIMEOUT 300)
set(_katana_e2e_fixture_install --root "${CMAKE_CURRENT_SOURCE_DIR}/fixtures/install/v1"
    --state "${CMAKE_CURRENT_BINARY_DIR}/e2e-state-that-is-never-made")
foreach(_katana_e2e_refused IN ITEMS
        "an_https_feed|https://github.com/kjarada/Katana/releases/latest/download"
        "a_feed_named_localhost_because_no_name_is_looked_up|http://localhost:8080"
        "a_plain_http_feed_on_another_host|http://example.com/feed")
    string(REPLACE "|" ";" _katana_e2e_refused "${_katana_e2e_refused}")
    list(GET _katana_e2e_refused 0 _katana_e2e_what)
    list(GET _katana_e2e_refused 1 _katana_e2e_address)
    add_test(NAME update_e2e_program_refuses_${_katana_e2e_what}
        COMMAND katana_update_e2e check ${_katana_e2e_fixture_install} --feed "${_katana_e2e_address}")
    set_tests_properties(update_e2e_program_refuses_${_katana_e2e_what} PROPERTIES
        PASS_REGULAR_EXPRESSION "--feed is an http address on the loopback"
        ENVIRONMENT_MODIFICATION "${_katana_e2e_runtime}"
        TIMEOUT 300)
endforeach()
add_test(NAME update_e2e_program_connects_to_no_github_address_it_was_given_no_route_for
    COMMAND katana_update_e2e check ${_katana_e2e_fixture_install} --github kjarada/Katana)
set_tests_properties(update_e2e_program_connects_to_no_github_address_it_was_given_no_route_for PROPERTIES
    PASS_REGULAR_EXPRESSION "detail=the test client reaches the loopback address only"
    ENVIRONMENT_MODIFICATION "${_katana_e2e_runtime}"
    TIMEOUT 300)

if(NOT Python3_Interpreter_FOUND OR Python3_VERSION VERSION_LESS 3.10)
    message(STATUS "Update end-to-end tests: no Python 3.10 or later; the fake feed's test and the "
                   "end-to-end tests are not registered")
    return()
endif()

set(_katana_e2e_no_proxy
    "NO_PROXY=set:127.0.0.1;no_proxy=set:127.0.0.1;HTTP_PROXY=unset:;HTTPS_PROXY=unset:;ALL_PROXY=unset:;http_proxy=unset:;https_proxy=unset:;all_proxy=unset:")

add_test(NAME update_tools_fake_feed
    COMMAND "${KATANA_PYTHON}" -B "${CMAKE_CURRENT_SOURCE_DIR}/tool_tests/test_fake_feed.py")
set_tests_properties(update_tools_fake_feed PROPERTIES
    ENVIRONMENT_MODIFICATION "${_katana_e2e_no_proxy}"
    TIMEOUT 1800)

# The driver's own readers, held to answers worked out by hand (update_e2e.py --self-test): the path
# a line of the stub's log names, whichever way this Python and the system spell it; a path as a
# program is given it; installed.json compared as the record it holds; the port nobody listens on.
# A case cannot tell a wrong program from a driver that reads a right one wrongly, so this can. It
# starts no program and needs no openssl.
set(_katana_e2e_driver "${CMAKE_CURRENT_SOURCE_DIR}/update_e2e.py")
add_test(NAME update_e2e_driver_reads_a_path_and_a_record_however_they_are_spelled
    COMMAND "${KATANA_PYTHON}" -B "${_katana_e2e_driver}" --self-test)
set_tests_properties(update_e2e_driver_reads_a_path_and_a_record_however_they_are_spelled PROPERTIES
    ENVIRONMENT_MODIFICATION "${_katana_e2e_runtime};${_katana_e2e_no_proxy}"
    TIMEOUT 300)

# openssl 3 or later (pkeyutl -rawin), looked for here as well as further down in CMakeLists.txt:
# this file is included before that search, and find_program keeps the one answer for both.
find_program(KATANA_OPENSSL_EXECUTABLE openssl)
set(_katana_e2e_openssl_major 0)
if(KATANA_OPENSSL_EXECUTABLE)
    execute_process(COMMAND "${KATANA_OPENSSL_EXECUTABLE}" version
        OUTPUT_VARIABLE _katana_e2e_openssl_version RESULT_VARIABLE _katana_e2e_openssl_status
        OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
    if(_katana_e2e_openssl_status EQUAL 0 AND _katana_e2e_openssl_version MATCHES "^OpenSSL ([0-9]+)[.]")
        set(_katana_e2e_openssl_major ${CMAKE_MATCH_1})
    endif()
endif()
if(_katana_e2e_openssl_major LESS 3)
    message(STATUS "Update end-to-end tests: no openssl 3 or later on PATH to sign the test releases "
                   "with; the end-to-end tests are not registered")
    return()
endif()

# Where the swap is written (Windows) every case runs, with the helper and the stand-in for Katana.
# Elsewhere the engine still checks a feed, so the cases that ask a feed and start no program of the
# install run there, with the engine alone (update_e2e.py --list marks them `portable`).
set(_katana_e2e_programs --engine "$<TARGET_FILE:katana_update_e2e>")
set(_katana_e2e_swaps OFF)
if(WIN32 AND TARGET katana_updater_test AND TARGET katana_updater_stub)
    set(_katana_e2e_swaps ON)
    list(APPEND _katana_e2e_programs
        --helper "$<TARGET_FILE:katana_updater_test>" --stub "$<TARGET_FILE:katana_updater_stub>")
endif()

set(_katana_e2e_cases "")

# katana_add_update_e2e_test(<sentence> <case> [PORTABLE] [SHARDS n])
# One ctest for a case of the driver; SHARDS n makes n of them, each taking every n-th crash point,
# so that the longest case is spread over the machine's cores.
function(katana_add_update_e2e_test sentence case)
    cmake_parse_arguments(PARSE_ARGV 2 _e2e "PORTABLE" "SHARDS" "")
    if(NOT _katana_e2e_swaps AND NOT _e2e_PORTABLE)
        return()
    endif()
    set(_names "")
    if(_e2e_SHARDS)
        foreach(_shard RANGE 1 ${_e2e_SHARDS})
            set(_name update_e2e_${sentence}_${_shard}_of_${_e2e_SHARDS})
            add_test(NAME ${_name}
                COMMAND "${KATANA_PYTHON}" -B "${_katana_e2e_driver}" --case ${case}
                        --shard ${_shard}/${_e2e_SHARDS} ${_katana_e2e_programs})
            list(APPEND _names ${_name})
        endforeach()
    else()
        add_test(NAME update_e2e_${sentence}
            COMMAND "${KATANA_PYTHON}" -B "${_katana_e2e_driver}" --case ${case} ${_katana_e2e_programs})
        list(APPEND _names update_e2e_${sentence})
    endif()
    # The toolchain's folder for katana_update_e2e, which the driver starts (see the top). The
    # timeout is generous: a case starts a dozen programs, and the whole suite runs beside it.
    set_tests_properties(${_names} PROPERTIES
        ENVIRONMENT_MODIFICATION
            "${_katana_e2e_runtime};KATANA_OPENSSL=set:${KATANA_OPENSSL_EXECUTABLE};${_katana_e2e_no_proxy}"
        TIMEOUT 1800)
    set(_katana_e2e_cases ${_katana_e2e_cases} ${case} PARENT_SCOPE)
endfunction()

katana_add_update_e2e_test(v1_to_v2_installs_exactly_v2_and_the_feed_serves_only_what_changed v1_to_v2)
katana_add_update_e2e_test(a_member_that_is_not_the_signed_file_is_never_staged_and_the_download_resumes
    bad_member_hash)
katana_add_update_e2e_test(a_file_held_open_rolls_the_install_back_to_v1 locked_file)
katana_add_update_e2e_test(a_helper_killed_before_any_change_leaves_v1_or_v2_after_recovery
    crash_at_every_step SHARDS 4)
katana_add_update_e2e_test(a_downgrade_is_not_offered_and_the_helper_refuses_one downgrade)
katana_add_update_e2e_test(a_replayed_old_release_is_not_offered_and_the_helper_refuses_it replayed_release)
katana_add_update_e2e_test(a_manifest_with_one_bit_changed_is_not_believed tampered_byte PORTABLE)
katana_add_update_e2e_test(a_signature_of_another_manifest_is_not_believed swapped_signature PORTABLE)
katana_add_update_e2e_test(a_release_signed_by_another_key_is_not_believed other_key PORTABLE)
katana_add_update_e2e_test(a_release_of_another_target_is_not_offered wrong_target PORTABLE)
katana_add_update_e2e_test(a_running_program_of_the_install_blocks_the_swap_and_nothing_is_touched
    program_running)
katana_add_update_e2e_test(a_new_version_that_exits_with_1_is_rolled_back_and_not_offered_again
    new_version_exits_1)
katana_add_update_e2e_test(the_token_goes_to_the_api_and_to_no_address_a_redirect_names token_first_hop_only)
katana_add_update_e2e_test(a_feed_that_cannot_be_read_is_unreachable_with_its_reason feed_unreachable PORTABLE)
katana_add_update_e2e_test(a_download_that_fails_keeps_nothing_unverified_and_then_succeeds download_fails)

# A case the driver has and nothing here registers would never run, and nothing would say so. The
# driver compares its own list with what was registered above (on Windows all of its cases, elsewhere
# those it calls portable).
set(_katana_e2e_expected --registered ${_katana_e2e_cases})
if(NOT _katana_e2e_swaps)
    list(APPEND _katana_e2e_expected --portable-only)
endif()
add_test(NAME update_e2e_every_case_of_the_driver_is_registered
    COMMAND "${KATANA_PYTHON}" -B "${_katana_e2e_driver}" ${_katana_e2e_expected})
set_tests_properties(update_e2e_every_case_of_the_driver_is_registered PROPERTIES
    ENVIRONMENT_MODIFICATION "${_katana_e2e_runtime}"
    TIMEOUT 300)
