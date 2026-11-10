# CMake generated Testfile for
# Source directory: /katana-source/b
# Build directory: /katana-build/b
add_test("cli.twice" "/katana-build/bin/katana_cli" "-c" "LIST")
set_tests_properties("cli.twice" PROPERTIES  FAIL_REGULAR_EXPRESSION "error")
