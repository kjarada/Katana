# CMake generated Testfile for
# Source directory: /katana-source/tests
# Build directory: /katana-build/tests
#
# This file includes the relevant testing commands required for
# testing this directory and lists subdirectories to be tested as well.
add_test("qt_a_project_is_saved_by_the_window_headless" "/opt/toolchain/bin/cmake" "-DAPP=/katana-build/bin/katana" "-DDRIVE=>LINE 0,0 10,0|>SAVE \"/katana-build/tests/window.katana\"" "-P" "/katana-source/tools/check_screenshot.cmake")
set_tests_properties("qt_a_project_is_saved_by_the_window_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen" FIXTURES_SETUP "window_project" _BACKTRACE_TRIPLES "/katana-source/tests/CMakeLists.txt;10;add_test;/katana-source/tests/CMakeLists.txt;0;")
add_test("cli.the_windows_project_is_opened" "/katana-build/bin/katana_cli" "-c" [[OPEN "/katana-build/tests/window.katana"]] "-c" "LIST")
set_tests_properties("cli.the_windows_project_is_opened" PROPERTIES  ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:/opt/toolchain/bin;KATANA_BUILTIN_CUSTOMISATION=set:none;KATANA_CUSTOMISATION=unset:" FAIL_REGULAR_EXPRESSION "error" FIXTURES_REQUIRED "window_project" _BACKTRACE_TRIPLES "/katana-source/tests/CMakeLists.txt;20;add_test;/katana-source/tests/CMakeLists.txt;0;")
