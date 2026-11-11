# The gate's second refusal (tools/rust_window_gate.py, "What it refuses"): a
# window test that names a program of the C++ build other than katana - here
# a check script given the window and katana_cli - which the gate does not
# swap, so the C++ program would run under the gate's verdict. Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
#   --output /katana-build/rust_window_gate
add_test("qt_a_line_is_drawn_headless" "/opt/toolchain/bin/cmake" "-DAPP=/katana-build/bin/katana" "-DPROJECT=/katana-build/no_such_project" "-DDRIVE=>LINE 0,0 10,0" "-DOUTPUT=/katana-build/line_headless.png" "-P" "/katana-source/tools/check_screenshot.cmake")
set_tests_properties("qt_a_line_is_drawn_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen")
add_test("qt_the_window_and_the_cli_headless" "/opt/toolchain/bin/cmake" "-DAPP=/katana-build/bin/katana" "-DCLI=/katana-build/bin/katana_cli" "-DPROJECT=/katana-build/no_such_project" "-DOUTPUT=/katana-build/both_headless.png" "-P" "/katana-source/tools/check_window_and_cli.cmake")
set_tests_properties("qt_the_window_and_the_cli_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen")
