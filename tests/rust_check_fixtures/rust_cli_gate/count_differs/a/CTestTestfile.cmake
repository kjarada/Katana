# CMake generated Testfile for
# Source directory: /katana-source/a
# Build directory: /katana-build/a
add_test("cli.twice" "/katana-build/bin/katana_cli" "-c" "LINE 0,0 10,0")
set_tests_properties("cli.twice" PROPERTIES  PASS_REGULAR_EXPRESSION "line created")
