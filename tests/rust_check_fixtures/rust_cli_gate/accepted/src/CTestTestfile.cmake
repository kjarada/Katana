# CMake generated Testfile for
# Source directory: /katana-source/src
# Build directory: /katana-build/src
#
# This file includes the relevant testing commands required for
# testing this directory and lists subdirectories to be tested as well.
add_test("cli.a_line_is_drawn_and_saved" "/opt/toolchain/bin/cmake" "-E" "env" "PATH=/opt/toolchain/bin;/usr/bin" "/katana-build/bin/katana_cli" "-c" "LINE 0,0 10,0" "-c" "SAVE \"/katana-build/src/cli-smoke.katana\"")
set_tests_properties("cli.a_line_is_drawn_and_saved" PROPERTIES  ENVIRONMENT_MODIFICATION "KATANA_BUILTIN_CUSTOMISATION=set:none;KATANA_CUSTOMISATION=unset:" FIXTURES_SETUP "smoke" PASS_REGULAR_EXPRESSION [=[line created
.*saved "[^"]*"]=] _BACKTRACE_TRIPLES "/katana-source/src/CMakeLists.txt;10;add_test;/katana-source/src/CMakeLists.txt;0;")
add_test("cli.a_script_runs_both_programs" "/opt/toolchain/bin/cmake" "-DCLI=/katana-build/bin/katana_cli" "-DMCP=/katana-build/bin/katana_mcp" "-DWORK=/katana-build/src/cli_text" "-DSTEP=save" "-P" "/katana-source/src/text_cli_check.cmake")
set_tests_properties("cli.a_script_runs_both_programs" PROPERTIES  FAIL_REGULAR_EXPRESSION "CMake Error" WORKING_DIRECTORY "/katana-build/src" _BACKTRACE_TRIPLES "/katana-source/src/CMakeLists.txt;20;add_test;/katana-source/src/CMakeLists.txt;0;")
add_test("cli.cleanup" "/opt/toolchain/bin/cmake" "-E" "rm" "-rf" "/katana-build/src/cli-smoke.katana")
set_tests_properties("cli.cleanup" PROPERTIES  FIXTURES_CLEANUP "smoke" _BACKTRACE_TRIPLES "/katana-source/src/CMakeLists.txt;30;add_test;/katana-source/src/CMakeLists.txt;0;")
add_test("katana_core_tests" "/katana-build/bin/katana_core_tests")
set_tests_properties("katana_core_tests" PROPERTIES  _BACKTRACE_TRIPLES "/katana-source/src/CMakeLists.txt;40;add_test;/katana-source/src/CMakeLists.txt;0;")
