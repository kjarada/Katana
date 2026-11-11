# The gate's first refusal (tools/rust_window_gate.py, "What it refuses"): a
# window test that runs katana where the gate replaces nothing - here given to
# the check script as -DAPP=katana, found on PATH, so the swap of
# /katana-build/bin/katana never reaches it and the test would run whatever
# katana PATH finds. Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
#   --output /katana-build/rust_window_gate
add_test("qt_a_line_is_drawn_headless" "/opt/toolchain/bin/cmake" "-DAPP=/katana-build/bin/katana" "-DPROJECT=/katana-build/no_such_project" "-DDRIVE=>LINE 0,0 10,0" "-DOUTPUT=/katana-build/line_headless.png" "-P" "/katana-source/tools/check_screenshot.cmake")
set_tests_properties("qt_a_line_is_drawn_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen")
add_test("qt_a_window_found_on_the_path_headless" "/opt/toolchain/bin/cmake" "-DAPP=katana" "-DPROJECT=/katana-build/no_such_project" "-DOUTPUT=/katana-build/path_headless.png" "-P" "/katana-source/tools/check_screenshot.cmake")
set_tests_properties("qt_a_window_found_on_the_path_headless" PROPERTIES  ENVIRONMENT_MODIFICATION "QT_QPA_PLATFORM=set:offscreen")
