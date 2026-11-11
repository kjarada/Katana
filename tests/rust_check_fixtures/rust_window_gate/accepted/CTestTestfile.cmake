# A build tree the gate must accept (tools/rust_window_gate.py, "Its own check"),
# in the shape CMake generates: its build directory is /katana-build, its
# programs are in /katana-build/bin. Read with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
#   --output /katana-build/rust_window_gate
# it has three window tests - a screenshot check given the window as -DAPP=,
# with a bracket argument; a script batch that runs it directly; one run
# through `cmake -E env` - a fixture provider that is not a window test (the
# folder the screenshot check writes into), a fixture user that is not a
# window test (a check of what the batch run saved), and, in another
# directory, tests that are not window tests - a cli test and a unit test,
# each naming another program, and a step whose text has the word katana in a
# sentence - which the gate leaves alone.
subdirs("src")
subdirs("tests")
