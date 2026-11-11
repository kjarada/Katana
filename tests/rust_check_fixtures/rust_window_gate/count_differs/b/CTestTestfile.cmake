# CMake generated Testfile for
# Source directory: /katana-source/b
# Build directory: /katana-build/b
add_test("qt_twice_headless" "/katana-build/bin/katana" "--script" "/katana-source/b/twice.kcs")
set_tests_properties("qt_twice_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen" WILL_FAIL "TRUE")
