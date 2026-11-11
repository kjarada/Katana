# CMake generated Testfile for
# Source directory: /katana-source/a
# Build directory: /katana-build/a
add_test("qt_twice_headless" "/opt/toolchain/bin/cmake" "-DAPP=/katana-build/bin/katana" "-DPROJECT=/katana-build/a/no_such_project" "-DOUTPUT=/katana-build/a/twice_headless.png" "-P" "/katana-source/tools/check_screenshot.cmake")
set_tests_properties("qt_twice_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen")
