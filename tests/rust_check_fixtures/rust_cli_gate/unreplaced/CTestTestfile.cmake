# The gate's first refusal (tools/rust_cli_gate.py, "What it refuses"): a cli
# test that runs katana_cli where the gate replaces nothing - here found on
# PATH by its bare name, so the swap of /katana-build/bin/katana_cli never
# reaches it and the test would run whatever katana_cli PATH finds. Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
add_test("cli.a_line_is_drawn" "/katana-build/bin/katana_cli" "-c" "LINE 0,0 10,0")
set_tests_properties("cli.a_line_is_drawn" PROPERTIES  PASS_REGULAR_EXPRESSION "line created")
add_test("cli.a_program_found_on_the_path" "/opt/toolchain/bin/cmake" "-E" "env" "katana_cli" "-c" "LIST")
set_tests_properties("cli.a_program_found_on_the_path" PROPERTIES  FAIL_REGULAR_EXPRESSION "error")
