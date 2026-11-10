# The gate's third refusal (tools/rust_cli_gate.py, "What it refuses"): the
# count of cli tests written differs from the count read. Two directories each
# define cli.twice, which ctest would run twice and one rewritten file cannot
# hold, so the rewrite has one cli test where two were read. Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
subdirs("a")
subdirs("b")
