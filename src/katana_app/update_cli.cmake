# The UPDATE verbs in the real katana_cli (update_verbs.hpp), included from
# CMakeLists.txt beside this file where katana_cli and the test variables are.
#
# Every run here is katana_cli started from a BUILD TREE, and the expectations
# rest on that, as the verbs' contract states it: a copy that packaging did
# not write (no share/katana/install.json) is kind=development, its check is
# update=unsupported, and nothing is ever downloaded to it or applied to it. So
# no test here reaches any server, and none can: what a release does to an
# installed copy is the update system's own end-to-end tests', on a feed of
# their own on the loopback address.
#
# The environment of each run, beside the two customisation variables the
# program-test scan gives every `cli.` test (tests/CMakeLists.txt):
#   KATANA_UPDATE_STATE_DIR  an update state folder of the test's own, so that
#                            the settings of whoever runs the suite are neither
#                            read nor written;
#   KATANA_UPDATE_TOKEN      a token of the test's own, which no run sends
#                            anywhere. The variable is read first, so the
#                            credential store of whoever runs the suite is not
#                            read at all - and no test here runs UPDATE TOKEN
#                            CLEAR, which would empty it;
#   HTTPS_PROXY, ALL_PROXY   a closed port on this machine: a request that a
#                            defect made would be refused there, not sent to a
#                            server. A correct run makes none.
# "[\r\n]+" where a line ends: the console writes CRLF on Windows.
set(_update_cli_state "${CMAKE_CURRENT_BINARY_DIR}/cli_update_state")
set(_update_cli_token "ghp_cli_test_token_that_is_never_sent")
set(_update_cli_nowhere "http://127.0.0.1:9")
set(_update_cli_version "[0-9]+[.][0-9.]+")

# UPDATE STATUS: what this copy is. A build tree is a development copy with no
# target; nothing is kept in the fresh state folder, so every setting is its
# default; the token is the variable's, said as present and never shown.
add_test(NAME cli.update_status_in_a_build_tree_says_the_copy_is_a_development_one
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/status"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE STATUS")
set_tests_properties(cli.update_status_in_a_build_tree_says_the_copy_is_a_development_one PROPERTIES
    PASS_REGULAR_EXPRESSION
    "installed=${_update_cli_version} target=none kind=development staged=none skipped=none auto=on last_check=never token=present last=none"
    FAIL_REGULAR_EXPRESSION "error|${_update_cli_token}")

# UPDATE CHECK: unsupported, and said without asking anyone. A run that asked
# would answer available, current or - through the closed port - unreachable.
add_test(NAME cli.update_check_in_a_build_tree_is_unsupported_and_asks_no_server
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/check"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE CHECK" -c "update check")
set_tests_properties(cli.update_check_in_a_build_tree_is_unsupported_and_asks_no_server PROPERTIES
    PASS_REGULAR_EXPRESSION
    "update=unsupported installed=${_update_cli_version} reason=development_tree detail=[^\r\n]*[\r\n]+update=unsupported installed=${_update_cli_version} reason=development_tree"
    FAIL_REGULAR_EXPRESSION
    "error|update=available|update=current|update=unreachable|update=blocked|${_update_cli_token}")

# A line without its CONFIRM is refused, saying that nothing was done and what
# the line with the word does; the refusal stops the batch.
add_test(NAME cli.update_download_without_confirm_is_refused_saying_what_confirm_does
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/download"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE DOWNLOAD" -c "POINT 1,1")
set_tests_properties(cli.update_download_without_confirm_is_refused_saying_what_confirm_does
    PROPERTIES
    PASS_REGULAR_EXPRESSION
    "error: InvalidArgument: UPDATE DOWNLOAD needs the word CONFIRM, and nothing was downloaded: UPDATE DOWNLOAD CONFIRM asks the release server for the newest release and downloads the files"
    FAIL_REGULAR_EXPRESSION "point created|staged=")
# A pass expression makes ctest ignore the exit code, so the exit code of the
# same refusal is a test of its own: the run must fail.
add_test(NAME cli.update_download_without_confirm_fails_the_run
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/download"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE DOWNLOAD")
set_tests_properties(cli.update_download_without_confirm_fails_the_run PROPERTIES WILL_FAIL TRUE)
add_test(NAME cli.update_apply_without_confirm_is_refused_saying_what_confirm_does
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/apply"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE APPLY")
set_tests_properties(cli.update_apply_without_confirm_is_refused_saying_what_confirm_does
    PROPERTIES
    PASS_REGULAR_EXPRESSION
    "error: InvalidArgument: UPDATE APPLY needs the word CONFIRM, and nothing was changed: UPDATE APPLY CONFIRM ends this session and has the update helper replace the installed files"
    FAIL_REGULAR_EXPRESSION "ready=")

# With its CONFIRM, in a build tree: nothing is downloaded to a development
# copy. Where this version installs updates (Windows) the reply is the check's
# own record and staged=none; elsewhere the line is refused for the platform.
add_test(NAME cli.update_download_confirm_in_a_build_tree_downloads_nothing
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/download_confirm"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE DOWNLOAD CONFIRM")
set_tests_properties(cli.update_download_confirm_in_a_build_tree_downloads_nothing PROPERTIES
    PASS_REGULAR_EXPRESSION
    "update=unsupported installed=${_update_cli_version} reason=development_tree[^\r\n]*[\r\n]+staged=none|error: Unsupported: UPDATE DOWNLOAD is not available on this system[^\r\n]*reason=platform"
    FAIL_REGULAR_EXPRESSION "progress |staged=[0-9]|${_update_cli_token}")

# UPDATE APPLY CONFIRM must be the last line of a run: the run ends with it,
# and only then is the helper started. With a line after it, it is refused
# BEFORE it prepares anything - the point before it is drawn, the one after it
# is not - and the run fails.
add_test(NAME cli.update_apply_confirm_that_is_not_the_last_line_is_refused
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/apply_not_last"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "POINT 1,1" -c "UPDATE APPLY CONFIRM" -c "POINT 2,2")
set_tests_properties(cli.update_apply_confirm_that_is_not_the_last_line_is_refused PROPERTIES
    PASS_REGULAR_EXPRESSION
    "point created[\r\n]+error: Unsupported: UPDATE APPLY CONFIRM must be the last line of a katana_cli run"
    FAIL_REGULAR_EXPRESSION "point created.*point created|ready=")
add_test(NAME cli.update_apply_confirm_that_is_not_the_last_line_fails_the_run
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/apply_not_last"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE APPLY CONFIRM" -c "LIST")
set_tests_properties(cli.update_apply_confirm_that_is_not_the_last_line_fails_the_run
    PROPERTIES WILL_FAIL TRUE)

# What "last" means: nothing is RUN after it. A script may end with blank
# lines and notes, and with a QUIT, which ends the run - what stands after a
# QUIT is never run. There the line reaches the verb, which refuses it for its
# own reason: a build tree is never updated (or, where this version installs
# no update at all, for the platform). Not for where the line stands.
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/cli_scripts/update_apply_last.kcs"
    "POINT 1,1\nUPDATE APPLY CONFIRM\n\n  # the update is the last thing this script does\nQUIT\nPOINT 2,2\n")
add_test(NAME cli.update_apply_confirm_followed_only_by_notes_and_a_quit_is_the_last_line
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/apply_last"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> "${CMAKE_CURRENT_BINARY_DIR}/cli_scripts/update_apply_last.kcs")
set_tests_properties(cli.update_apply_confirm_followed_only_by_notes_and_a_quit_is_the_last_line
    PROPERTIES
    PASS_REGULAR_EXPRESSION
    "point created[\r\n]+error: Unsupported: [^\r\n]*(reason=development_tree|reason=platform)"
    FAIL_REGULAR_EXPRESSION "must be the last line|point created.*point created|ready=")

# HELP UPDATE prints the grammar, every line of it, and --help carries the
# same block between the session's other verbs and the window's.
add_test(NAME cli.update_help_prints_the_grammar
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/help"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "HELP UPDATE" -c "help update")
set_tests_properties(cli.update_help_prints_the_grammar PROPERTIES
    PASS_REGULAR_EXPRESSION
    "Update    UPDATE CHECK .*UPDATE STATUS .*UPDATE DOWNLOAD CONFIRM .*UPDATE APPLY CONFIRM .*reason=elevated.*UPDATE SKIP <version> [|] UPDATE SKIP NONE .*UPDATE BLOCKED CLEAR .*UPDATE SET AUTO ON[|]OFF .*UPDATE TOKEN STATUS [|] UPDATE TOKEN CLEAR .*katana_check_update.*Update    UPDATE CHECK "
    FAIL_REGULAR_EXPRESSION "error")
add_test(NAME cli.update_verbs_are_in_the_help
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/help"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> --help)
set_tests_properties(cli.update_verbs_are_in_the_help PROPERTIES
    PASS_REGULAR_EXPRESSION
    "usage: katana_cli .*Update    UPDATE CHECK .*UPDATE APPLY CONFIRM .*UPDATE BLOCKED CLEAR .*UPDATE TOKEN STATUS [|] UPDATE TOKEN CLEAR .*Window    PLOT, PLOTSHEETS")

# A word that is no action is refused with the usage, and stops the batch.
# ("\\;": a bare semicolon would end the expression and begin a second one.)
add_test(NAME cli.update_refuses_a_word_that_is_no_action
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/usage"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE NOW" -c "POINT 1,1")
set_tests_properties(cli.update_refuses_a_word_that_is_no_action PROPERTIES
    PASS_REGULAR_EXPRESSION
    "error: InvalidArgument: 'NOW' is not an UPDATE action\\; usage: UPDATE CHECK [|] UPDATE STATUS [|] UPDATE DOWNLOAD CONFIRM [|] UPDATE APPLY CONFIRM [|] UPDATE SKIP <version>[|]NONE [|] UPDATE BLOCKED CLEAR [|] UPDATE SET AUTO ON[|]OFF [|] UPDATE TOKEN STATUS[|]CLEAR"
    FAIL_REGULAR_EXPRESSION "point created")

# No line takes a token, because lines are logged: one typed after TOKEN is
# refused and is not said back. TOKEN STATUS says present or absent - here
# present, the variable's - and never the token.
add_test(NAME cli.update_token_typed_on_a_line_is_refused_and_not_repeated
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/token"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE TOKEN SET ghp_typed_on_a_line")
set_tests_properties(cli.update_token_typed_on_a_line_is_refused_and_not_repeated PROPERTIES
    PASS_REGULAR_EXPRESSION
    "error: InvalidArgument: UPDATE TOKEN takes STATUS or CLEAR and no other word[.] A token is never typed on a line"
    FAIL_REGULAR_EXPRESSION "ghp_typed_on_a_line|${_update_cli_token}")
add_test(NAME cli.update_token_status_says_present_and_never_the_token
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_state}/token"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE TOKEN STATUS" -c "UPDATE ${_update_cli_token}")
set_tests_properties(cli.update_token_status_says_present_and_never_the_token PROPERTIES
    PASS_REGULAR_EXPRESSION
    "token=present[\r\n]+error: InvalidArgument: '[*][*][*]' is not an UPDATE action"
    FAIL_REGULAR_EXPRESSION "${_update_cli_token}")

# UPDATE SKIP and UPDATE SET AUTO are the person's settings, kept in the state
# folder between runs - by a development copy too: they are the user's, not
# the copy's. Three tests in order, held together as fixtures, on a folder
# that is emptied first: one run sets both and reads them back, the next run
# reads what the first kept, takes both back and reads again.
set(_update_cli_kept "${_update_cli_state}/kept")
add_test(NAME cli.update_settings_folder_is_emptied
    COMMAND ${CMAKE_COMMAND} -E rm -rf "${_update_cli_kept}")
add_test(NAME cli.update_skip_and_set_auto_are_kept
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_kept}"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE SET AUTO OFF" -c "UPDATE SKIP 9.9.9"
            -c "UPDATE STATUS")
add_test(NAME cli.update_settings_are_read_back_by_the_next_run_and_taken_back
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            "KATANA_UPDATE_STATE_DIR=${_update_cli_kept}"
            "KATANA_UPDATE_TOKEN=${_update_cli_token}"
            "HTTPS_PROXY=${_update_cli_nowhere}" "https_proxy=${_update_cli_nowhere}"
            "ALL_PROXY=${_update_cli_nowhere}"
            $<TARGET_FILE:katana_cli> -c "UPDATE STATUS" -c "UPDATE SKIP NONE"
            -c "UPDATE SET AUTO ON" -c "UPDATE STATUS")
set_tests_properties(cli.update_settings_folder_is_emptied PROPERTIES
    FIXTURES_SETUP update_cli_settings_emptied)
set_tests_properties(cli.update_skip_and_set_auto_are_kept PROPERTIES
    FIXTURES_REQUIRED update_cli_settings_emptied
    FIXTURES_SETUP update_cli_settings_kept
    PASS_REGULAR_EXPRESSION
    "auto=off[\r\n]+skipped=9[.]9[.]9[\r\n]+installed=${_update_cli_version} target=none kind=development staged=none skipped=9[.]9[.]9 auto=off last_check=never token=present last=none"
    FAIL_REGULAR_EXPRESSION "error|reason=")
set_tests_properties(cli.update_settings_are_read_back_by_the_next_run_and_taken_back PROPERTIES
    FIXTURES_REQUIRED update_cli_settings_kept
    PASS_REGULAR_EXPRESSION
    "staged=none skipped=9[.]9[.]9 auto=off last_check=never [^\r\n]*[\r\n]+skipped=none[\r\n]+auto=on[\r\n]+installed=${_update_cli_version} target=none kind=development staged=none skipped=none auto=on last_check=never token=present last=none"
    FAIL_REGULAR_EXPRESSION "error|reason=")
