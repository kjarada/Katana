# UPDATE BLOCKED CLEAR in the real katana_cli (update_verbs.hpp), included from
# CMakeLists.txt beside this file as update_cli.cmake is, and before it: the
# variables here are this file's own.
#
# Every run is katana_cli started from a BUILD TREE, a development copy: its
# UPDATE CHECK asks nobody and its APPLY is refused, so that a release which was
# blocked is offered and applied again is not something these runs can show.
# That is tests/app/test_update_verbs.cpp's, on a scratch install and a fake
# feed (UpdateVerbs.BlockedClearEmptiesTheListAndTheNextCheckOffersTheReleaseAgain,
# ...AfterBlockedClearApplyConfirmReturnsTheRequestForTheReleaseThatWasPutBack).
# What IS the program's own to show: that the line is taken by the real
# command line, prints its record, empties the list in the state file it was
# pointed at and leaves the person's other settings in it - which a development
# copy does too, for they are the user's and not the copy's.
#
# The environment of each run is update_cli.cmake's: the two customisation
# variables come from the program-test scan (tests/CMakeLists.txt gives them to
# every `cli.` test), and
#   KATANA_UPDATE_STATE_DIR  a folder in the BUILD tree, so that the per-user
#                            update folder of whoever runs the suite is neither
#                            read nor written;
#   KATANA_UPDATE_TOKEN      a token of the test's own, read before any
#                            credential store and sent nowhere;
#   HTTPS_PROXY, ALL_PROXY   a closed port on this machine. No run here makes a
#                            request.
set(_update_blocked_state "${CMAKE_CURRENT_BINARY_DIR}/cli_update_state/blocked")
set(_update_blocked_token "ghp_cli_test_token_that_is_never_sent")
set(_update_blocked_nowhere "http://127.0.0.1:9")
set(_update_blocked_version "[0-9]+[.][0-9.]+")

# The state file before and after, written and read by a script and not by
# Katana: what the first writes is the shape update/state.hpp documents, by
# hand - two releases blocked (the sequences of the fixtures' two), a version
# skipped, the automatic check off, and a last check at 1792368000, which is
# 2026-10-19T00:00:00Z - and what the second prints is read with CMake's own
# JSON reader.
set(_update_blocked_script "${CMAKE_CURRENT_BINARY_DIR}/cli_scripts/update_blocked_state.cmake")
file(WRITE "${_update_blocked_script}" [==[
# Written by src/katana_app/update_blocked.cmake when the tree is configured.
#   cmake -DSTATE=<folder> -DSTEP=write|read -P update_blocked_state.cmake
if(NOT DEFINED STATE OR NOT DEFINED STEP)
    message(FATAL_ERROR "STATE and STEP must be given")
endif()
if(STEP STREQUAL "write")
    file(REMOVE_RECURSE "${STATE}")
    file(MAKE_DIRECTORY "${STATE}")
    file(WRITE "${STATE}/state.json"
        "{\"schema\":1,\"last_check\":1792368000,\"auto\":false,\"skipped\":\"9.9.9\",\"blocked\":[1790000000,1792312800]}\n")
elseif(STEP STREQUAL "read")
    file(READ "${STATE}/state.json" _state)
    string(JSON _schema GET "${_state}" schema)
    string(JSON _blocked LENGTH "${_state}" blocked)
    string(JSON _skipped GET "${_state}" skipped)
    string(JSON _auto GET "${_state}" auto)
    string(JSON _last GET "${_state}" last_check)
    message("state schema=${_schema} blocked=${_blocked} skipped=${_skipped} auto=${_auto} last_check=${_last}")
else()
    message(FATAL_ERROR "STEP is write or read, not '${STEP}'")
endif()
]==])

# Three tests in order, held together as fixtures: the state is written; one
# run reads the settings, clears the list and reads them again; the file is
# read back.
add_test(NAME cli.update_blocked_state_is_written_with_two_releases_blocked
    COMMAND ${CMAKE_COMMAND} "-DSTATE=${_update_blocked_state}/kept" -DSTEP=write
            -P "${_update_blocked_script}")
# STATUS does not say what is blocked (the grammar: CHECK does, for the release
# the feed offers), so its record is the same before and after - which is the
# point: the line took nothing but the list. The exit code is a test of its
# own below, since a pass expression makes ctest ignore it.
add_test(NAME cli.update_blocked_clear_says_none_and_keeps_the_other_settings
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_blocked_state}/kept"
            "KATANA_UPDATE_TOKEN=${_update_blocked_token}"
            "HTTPS_PROXY=${_update_blocked_nowhere}" "https_proxy=${_update_blocked_nowhere}"
            "ALL_PROXY=${_update_blocked_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE STATUS" -c "UPDATE BLOCKED CLEAR"
            -c "update blocked clear" -c "UPDATE STATUS")
add_test(NAME cli.update_blocked_clear_left_the_state_file_with_an_empty_list
    COMMAND ${CMAKE_COMMAND} "-DSTATE=${_update_blocked_state}/kept" -DSTEP=read
            -P "${_update_blocked_script}")
set_tests_properties(cli.update_blocked_state_is_written_with_two_releases_blocked PROPERTIES
    FIXTURES_SETUP update_cli_blocked_written)
set_tests_properties(cli.update_blocked_clear_says_none_and_keeps_the_other_settings PROPERTIES
    FIXTURES_REQUIRED update_cli_blocked_written
    FIXTURES_SETUP update_cli_blocked_cleared
    PASS_REGULAR_EXPRESSION
    "installed=${_update_blocked_version} target=none kind=development staged=none skipped=9[.]9[.]9 auto=off last_check=2026-10-19T00:00:00Z token=present last=none[\r\n]+blocked=none[\r\n]+blocked=none[\r\n]+installed=${_update_blocked_version} target=none kind=development staged=none skipped=9[.]9[.]9 auto=off last_check=2026-10-19T00:00:00Z token=present last=none"
    FAIL_REGULAR_EXPRESSION "error|reason=|${_update_blocked_token}")
# JSON's false is OFF to CMake's reader.
set_tests_properties(cli.update_blocked_clear_left_the_state_file_with_an_empty_list PROPERTIES
    FIXTURES_REQUIRED update_cli_blocked_cleared
    PASS_REGULAR_EXPRESSION
    "state schema=1 blocked=0 skipped=9[.]9[.]9 auto=OFF last_check=1792368000")

# UPDATE BLOCKED with no word after it, or another one, is refused saying the
# one line there is, and the refusal stops the batch.
add_test(NAME cli.update_blocked_without_clear_is_refused_saying_the_one_line_there_is
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_blocked_state}/usage"
            "KATANA_UPDATE_TOKEN=${_update_blocked_token}"
            "HTTPS_PROXY=${_update_blocked_nowhere}" "https_proxy=${_update_blocked_nowhere}"
            "ALL_PROXY=${_update_blocked_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE BLOCKED" -c "POINT 1,1")
set_tests_properties(cli.update_blocked_without_clear_is_refused_saying_the_one_line_there_is
    PROPERTIES
    PASS_REGULAR_EXPRESSION
    "error: InvalidArgument: UPDATE BLOCKED takes CLEAR and no other word: UPDATE BLOCKED CLEAR is the one line there is"
    FAIL_REGULAR_EXPRESSION "point created|blocked=")
add_test(NAME cli.update_blocked_with_another_word_fails_the_run
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_blocked_state}/usage"
            "KATANA_UPDATE_TOKEN=${_update_blocked_token}"
            "HTTPS_PROXY=${_update_blocked_nowhere}" "https_proxy=${_update_blocked_nowhere}"
            "ALL_PROXY=${_update_blocked_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE BLOCKED NONE")
set_tests_properties(cli.update_blocked_with_another_word_fails_the_run PROPERTIES WILL_FAIL TRUE)
# ... and the line itself, with nothing wrong, ends the run with success.
add_test(NAME cli.update_blocked_clear_ends_the_run_with_success
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_blocked_state}/success"
            "KATANA_UPDATE_TOKEN=${_update_blocked_token}"
            "HTTPS_PROXY=${_update_blocked_nowhere}" "https_proxy=${_update_blocked_nowhere}"
            "ALL_PROXY=${_update_blocked_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE BLOCKED CLEAR")
