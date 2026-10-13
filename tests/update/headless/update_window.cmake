# The update system in the real window (docs/desktop.md, "Help > Check for
# Updates"; docs/headless.md, "Updates in a headless run"), driven headlessly
# through tools/check_screenshot.cmake. Included from tests/CMakeLists.txt when
# the window is built.
#
# Every run here is a HEADLESS window started from a BUILD TREE, and the
# expectations rest on those two facts, as the UPDATE verbs' contract states
# them (src/katana_app/update_verbs.hpp):
#   - a copy that packaging did not write (no share/katana/install.json) is
#     kind=development, and is never updated;
#   - a headless window may CHECK and STATUS when it is asked, and its
#     DOWNLOAD and APPLY reply reason=headless.
# The dialog's sentence is the design's "Updates are not available for this
# copy of Katana: <reason>." with the dialog's words for the reason
# development_tree (src/katana_qt/update_dialog.cpp). Nothing here reaches any
# host: a development copy asks nobody, and no check is made.
#
# What is grabbed at the end of each run is a window with something in it -
# the main window, the Settings dialog, or Help > Keyboard Shortcuts' table,
# opened last where the update dialog would otherwise be the picture: that
# dialog in the state a build tree shows is one sentence on a plain ground,
# which is fewer bytes of PNG than the script takes for a window that painted.
#
# The environment: offscreen; no built-in and no kept customisation, which the
# program-test scan gives every *_headless test anyway (tests/CMakeLists.txt)
# and is said here so that it holds wherever this file is included from; an
# update state folder of the test's own; and no access token from whoever runs
# the suite.
set(_update_headless_environment
    "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:none;KATANA_CUSTOMISATION=unset:;KATANA_UPDATE_STATE_DIR=set:${CMAKE_CURRENT_BINARY_DIR}/update_headless_state;KATANA_UPDATE_TOKEN=unset:")

# --dialog takes the dialog's own name, updateDialog: it opens, non-modal, and
# says why a build tree cannot update. Nothing was typed and nothing is asked
# of any server: the copy is known to be a development one from its own files.
add_test(NAME qt_the_update_dialog_opens_by_its_name_and_says_a_build_tree_cannot_update_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=@updateDialog|?updateStateText|?updateDetailText|@helpKeyboardShortcuts"
        "-DEXPECT=--dialog updateDialog opened updateDialog \"Check for Updates\"[\r\n].*updateStateText: Updates are not available for this copy of Katana: it is a development build, not an installed release\\.[\r\n]+updateDetailText: [\r\n]"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/update_dialog_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# In that state the dialog offers nothing to press: Check now is hidden, and a
# press of a hidden button fails the run (docs/headless.md, the steps).
add_test(NAME qt_a_copy_that_cannot_update_is_offered_no_check_in_the_update_dialog_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=@updateDialog|?updateStateText|!updateCheckNow"
        "-DREFUSED=updateStateText: Updates are not available for this copy of Katana: .*--press: updateDialog's button updateCheckNow is hidden"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/update_dialog_no_check_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# Help > Check for Updates is a menu item like every other: an icon, a status
# tip, a letter of its own in the Help menu (U), and it opens the same dialog.
# --check-shortcuts and --check-menus fail the run when it does not; the Help
# menu is then listed with the tip each item shows.
add_test(NAME qt_help_has_check_for_updates_with_an_icon_a_tip_and_a_letter_of_its_own_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DCHECK_SHORTCUTS=ON"
        "-DCHECK_MENUS=ON"
        "-DDRIVE=@helpCheckForUpdates|?helpMenu|?helpCheckForUpdates|@helpKeyboardShortcuts"
        "-DEXPECT=shortcuts: [0-9]+ key sequences, each reaching one thing.*menus: [0-9]+ items, each with an icon and a status tip.*--dialog helpCheckForUpdates opened updateDialog \"Check for Updates\".*helpMenu: Help: Command Reference \\[[^]]+\\] . Sheets and Plotting Commands \\[[^]]+\\] . Keyboard Shortcuts\\.\\.\\. \\[[^]]+\\] . Check for Updates\\.\\.\\. \\[Ask whether a newer Katana has been released[^]]+\\] . About Katana \\[[^]]+\\][\r\n]+helpCheckForUpdates: Check for Updates\\.\\.\\."
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/update_help_item_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# UPDATE STATUS typed on the window's command line: the verbs' reply, which in
# a build tree says the copy is a development one.
add_test(NAME qt_a_typed_update_status_says_a_build_tree_is_a_development_copy_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>UPDATE STATUS"
        "-DEXPECT=kind=development"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/update_status_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# A headless window never downloads and never applies: each line is answered
# with the reason, and the run goes on to the next (it is a reply, not a
# refusal of the line).
add_test(NAME qt_a_typed_update_download_and_apply_say_a_headless_run_does_neither_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=>UPDATE DOWNLOAD CONFIRM|>UPDATE APPLY CONFIRM"
        "-DEXPECT=reason=headless.*reason=headless"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/update_download_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# Settings has its second page: turned to by its name in the page list, it
# says a headless run keeps no update settings, and what a check sends - the
# shape of the user agent, since a headless run asks the verbs for nothing it
# was not told to. The box is off: nobody says the automatic check is on.
add_test(NAME qt_settings_has_an_updates_page_that_says_what_a_check_sends_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=@fileSettings|settingsPages=Updates|?settingsUpdateNote|?settingsUpdateSends|?settingsUpdateAuto|?settingsUpdateTokenState"
        "-DEXPECT=--dialog fileSettings opened settingsDialog \"Settings\"[\r\n]+settingsUpdateNote: A headless run keeps no update settings and checks only when it is told to \\(UPDATE CHECK\\)\\.[\r\n]+settingsUpdateSends: A check asks the release server for the newest version\\. It sends only \"Katana/<version> \\(<target>\\)\" as its user agent - nothing about you, your files or your drawings - and, as with every website, the release server sees this computer's network address\\.[\r\n]+settingsUpdateAuto: Check for updates automatically, unchecked[\r\n]+settingsUpdateTokenState: This session keeps no access token. the environment variable KATANA_UPDATE_TOKEN gives a run one\\."
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/update_settings_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# And it writes no credential store: a headless run is the same on every
# machine and leaves nothing behind on any, so Save is disabled there - and a
# press of a disabled button fails the run.
add_test(NAME qt_a_headless_run_saves_no_access_token_headless
    COMMAND ${CMAKE_COMMAND}
        "-DAPP=$<TARGET_FILE:katana>"
        "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
        "-DDRIVE=@fileSettings|settingsPages=Updates|!settingsUpdateTokenSave"
        "-DREFUSED=--dialog fileSettings opened settingsDialog.*--press: fileSettings's button settingsUpdateTokenSave is disabled"
        "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/update_settings_no_token_headless.png"
        -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

# --update-health TX: what the update helper runs to learn whether the version
# it has just put in place starts (include/katana/update/apply.hpp, step 7:
# "it writes the marker after its first idle turn and exits"). Three tests in
# order, held together as fixtures, on a state folder of their own:
#   - the folder is emptied, so a marker left by an earlier run cannot pass;
#   - the REAL window is built - never shown - writes healthy-<TX> and exits 0.
#     The helper waits for that exit and would stop a copy still running at the
#     end of its wait, so "exits" is the property, not "started";
#   - the marker is there. "ok" is what the engine's own tests write in one;
#     only its presence is read.
# The id is the shape the helper makes (update::isValidTransactionId: 1 to 64
# of 0-9 and a-f).
set(_update_health_state "${CMAKE_CURRENT_BINARY_DIR}/update_health_state")
add_test(NAME update_health_state_folder_is_emptied_before_the_health_run
    COMMAND ${CMAKE_COMMAND} -E rm -rf "${_update_health_state}")
add_test(NAME qt_the_health_run_writes_its_marker_and_exits_headless
    COMMAND $<TARGET_FILE:katana> --update-health 0a1b2c3d)
add_test(NAME update_health_marker_is_there_after_the_health_run
    COMMAND ${CMAKE_COMMAND} -E cat "${_update_health_state}/healthy-0a1b2c3d")
set_tests_properties(update_health_state_folder_is_emptied_before_the_health_run
    PROPERTIES FIXTURES_SETUP update_health_state_emptied)
set_tests_properties(qt_the_health_run_writes_its_marker_and_exits_headless
    PROPERTIES FIXTURES_REQUIRED update_health_state_emptied
               FIXTURES_SETUP update_health_run_made
               ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:none;KATANA_CUSTOMISATION=unset:;KATANA_UPDATE_STATE_DIR=set:${_update_health_state}"
               TIMEOUT 180)
set_tests_properties(update_health_marker_is_there_after_the_health_run
    PROPERTIES FIXTURES_REQUIRED update_health_run_made
               PASS_REGULAR_EXPRESSION "^ok")

# What comes with the switch names a file in the state folder, so only an id is
# taken: anything else - here, a path that climbs out of the folder - ends the
# run before a window is built, saying so, and writes nothing.
add_test(NAME qt_a_health_run_given_no_transaction_id_is_refused_headless
    COMMAND $<TARGET_FILE:katana> --update-health ../healthy)
set_tests_properties(qt_a_health_run_given_no_transaction_id_is_refused_headless
    PROPERTIES PASS_REGULAR_EXPRESSION
        "--update-health needs the transaction id the update helper gives it"
               ENVIRONMENT_MODIFICATION
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:none;KATANA_CUSTOMISATION=unset:;KATANA_UPDATE_STATE_DIR=set:${_update_health_state}_refused"
               TIMEOUT 180)

set_tests_properties(
    qt_the_update_dialog_opens_by_its_name_and_says_a_build_tree_cannot_update_headless
    qt_a_copy_that_cannot_update_is_offered_no_check_in_the_update_dialog_headless
    qt_help_has_check_for_updates_with_an_icon_a_tip_and_a_letter_of_its_own_headless
    qt_a_typed_update_status_says_a_build_tree_is_a_development_copy_headless
    qt_a_typed_update_download_and_apply_say_a_headless_run_does_neither_headless
    qt_settings_has_an_updates_page_that_says_what_a_check_sends_headless
    qt_a_headless_run_saves_no_access_token_headless
    PROPERTIES ENVIRONMENT_MODIFICATION "${_update_headless_environment}"
    TIMEOUT 180)
