# The gate's third refusal (tools/rust_window_gate.py, "What it refuses"): the
# count of window tests written differs from the count read. Two directories
# each define qt_twice_headless, which ctest would run twice and one rewritten
# file cannot hold, so the rewrite has one window test where two were read.
# Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
#   --output /katana-build/rust_window_gate
subdirs("a")
subdirs("b")
