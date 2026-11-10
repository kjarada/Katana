# The gate's second refusal (tools/rust_cli_gate.py, "What it refuses"): a cli
# test that names a program of the C++ build other than katana_cli and
# katana_mcp - here the window - which the gate does not swap, so the C++
# program would run under the gate's verdict. Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
add_test("cli.a_line_is_drawn" "/katana-build/bin/katana_cli" "-c" "LINE 0,0 10,0")
set_tests_properties("cli.a_line_is_drawn" PROPERTIES  PASS_REGULAR_EXPRESSION "line created")
add_test("cli.the_window_is_run" "/opt/toolchain/bin/cmake" "-E" "env" "QT_QPA_PLATFORM=offscreen" "/katana-build/bin/katana" "--screenshot" "/katana-build/shot.png")
