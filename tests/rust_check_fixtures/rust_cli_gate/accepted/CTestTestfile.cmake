# A build tree the gate must accept (tools/rust_cli_gate.py, "Its own check"),
# in the shape CMake generates: its build directory is /katana-build, its
# programs are in /katana-build/bin. Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
# it has four cli tests - a program run through `cmake -E env`, a script given
# both programs, a step that runs none, a project the window made opened with
# a bracket argument - one fixture provider that is not a cli test, and a test
# that is not a cli test naming another program, which the gate leaves alone.
subdirs("src")
subdirs("tests")
