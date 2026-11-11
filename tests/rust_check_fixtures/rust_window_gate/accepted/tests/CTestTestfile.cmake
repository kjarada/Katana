# CMake generated Testfile for
# Source directory: /katana-source/tests
# Build directory: /katana-build/tests
#
# This file includes the relevant testing commands required for
# testing this directory and lists subdirectories to be tested as well.
add_test("qt_a_line_is_drawn_headless" "/opt/toolchain/bin/cmake" "-DAPP=/katana-build/bin/katana" "-DPROJECT=/katana-build/tests/no_such_project" "-DDRIVE=>LINE 0,0 10,0|>LIST" [[-DEXPECT=line created.*1 entities]] "-DOUTPUT=/katana-build/tests/line folder/line_headless.png" "-P" "/katana-source/tools/check_screenshot.cmake")
set_tests_properties("qt_a_line_is_drawn_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:/opt/toolchain/bin;QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:none;KATANA_CUSTOMISATION=unset:" FIXTURES_REQUIRED "line_folder" TIMEOUT "180" _BACKTRACE_TRIPLES "/katana-source/tests/CMakeLists.txt;10;add_test;/katana-source/tests/CMakeLists.txt;0;")
add_test("qt_line_folder_made" "/opt/toolchain/bin/cmake" "-E" "make_directory" "/katana-build/tests/line folder")
set_tests_properties("qt_line_folder_made" PROPERTIES  FIXTURES_SETUP "line_folder" _BACKTRACE_TRIPLES "/katana-source/tests/CMakeLists.txt;20;add_test;/katana-source/tests/CMakeLists.txt;0;")
add_test("qt_a_script_batch_saves_headless" "/katana-build/bin/katana" "--script" "/katana-source/tests/data/save.kcs")
set_tests_properties("qt_a_script_batch_saves_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen" FIXTURES_SETUP "batch_saved" PASS_REGULAR_EXPRESSION "lines=2 ran=2 failed=0" TIMEOUT "180" _BACKTRACE_TRIPLES "/katana-source/tests/CMakeLists.txt;30;add_test;/katana-source/tests/CMakeLists.txt;0;")
add_test("batch_project_is_there_after_the_batch_run" "/opt/toolchain/bin/cmake" "-E" "cat" "/katana-build/tests/batch.katana/project.db")
set_tests_properties("batch_project_is_there_after_the_batch_run" PROPERTIES  FIXTURES_REQUIRED "batch_saved" _BACKTRACE_TRIPLES "/katana-source/tests/CMakeLists.txt;40;add_test;/katana-source/tests/CMakeLists.txt;0;")
add_test("qt_toolbar_signs_through_env_headless" "/opt/toolchain/bin/cmake" "-E" "env" "QT_SCALE_FACTOR=1.25" "/katana-build/bin/katana" "--check-toolbars" "--screenshot" "/katana-build/tests/toolbar_signs_headless.png")
set_tests_properties("qt_toolbar_signs_through_env_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen" TIMEOUT "180" _BACKTRACE_TRIPLES "/katana-source/tests/CMakeLists.txt;50;add_test;/katana-source/tests/CMakeLists.txt;0;")
