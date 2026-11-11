# CMake generated Testfile for
# Source directory: /katana-source/src
# Build directory: /katana-build/src
#
# This file includes the relevant testing commands required for
# testing this directory and lists subdirectories to be tested as well.
add_test("cli.a_line_is_drawn" "/katana-build/bin/katana_cli" "-c" "LINE 0,0 10,0")
set_tests_properties("cli.a_line_is_drawn" PROPERTIES  PASS_REGULAR_EXPRESSION "line created" _BACKTRACE_TRIPLES "/katana-source/src/CMakeLists.txt;10;add_test;/katana-source/src/CMakeLists.txt;0;")
add_test("katana_core_tests" "/katana-build/bin/katana_core_tests")
set_tests_properties("katana_core_tests" PROPERTIES  _BACKTRACE_TRIPLES "/katana-source/src/CMakeLists.txt;20;add_test;/katana-source/src/CMakeLists.txt;0;")
add_test("the_window_is_named_in_a_sentence" "/opt/toolchain/bin/cmake" "-E" "echo" "katana is the window; katana_cli its command line")
set_tests_properties("the_window_is_named_in_a_sentence" PROPERTIES  _BACKTRACE_TRIPLES "/katana-source/src/CMakeLists.txt;30;add_test;/katana-source/src/CMakeLists.txt;0;")
