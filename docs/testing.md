# Testing

What the tests are, how one is added and registered, how the window is tested
without a display, and the rules that make a test evidence rather than
decoration. How to configure and build is `docs/building.md`.

## Running

```sh
ctest --preset debug                       # everything, in build/debug
ctest --test-dir build/wt -j 8             # a hand-made build tree
ctest --test-dir build/wt -R '^qt_'        # the window: widget tests and headless checks
ctest --test-dir build/wt -R 'Snapping'    # one suite's cases, by name
ctest --test-dir build/wt -N               # list every test without running it
./build/wt/bin/tests/katana_cad_tests.exe --gtest_filter='Snap*'
```

Run the WHOLE suite before a commit, not only the module touched: the
`layering` and `docs` tests are part of it, and a change in one module breaks
another's tests more often than its own. Run it in Release as well as Debug
before handing back: results must match across optimisation levels
(`docs/architecture.md`, "Determinism"), and a Debug-only green says nothing
about the configuration that ships.

## What there is

| Kind | Registered by | Named | Where |
|---|---|---|---|
| unit, integration, property and regression cases (GoogleTest) | `katana_add_test_suite` | `<Suite>.<Case>` | `tests/<module>/`, one executable per module |
| widget tests, offscreen | `tests/qt_widgets/CMakeLists.txt` | `qt_widgets.<Suite>.<Case>` | `tests/qt_widgets/` |
| the whole window, headless | `add_test` in `tests/CMakeLists.txt` | `qt_<what it shows>_headless` | `tools/check_screenshot.cmake`, `tools/check_plot.cmake` |
| end-to-end command line | `add_test` in `src/katana_app/CMakeLists.txt` | `cli.<what it shows>` | `katana_cli` run as a process |
| the layering rules | `tests/CMakeLists.txt` | `layering` | `tools/check_layering.cmake` |
| the documents' references | `tests/CMakeLists.txt` | `docs` | `tools/check_docs.py` ("The docs test", below) |

`ctest -N` gives the current count of each; this document does not, because a
number here would be stale by the next commit.

### A module's suite

```cmake
katana_add_test_suite(geodesy
    LIBS katana_geodesy
    SOURCES
        test_units.cpp
        test_coordinate_reference_system.cpp
        test_coordinate_transformer.cpp
)
```

`katana_add_test_suite(<module> LIBS <targets...> SOURCES <files...>)`
(`tests/CMakeLists.txt`) builds `katana_<module>_tests` into
`<build>/bin/tests`, links GoogleTest's main, applies the project's compiler
settings and registers every case with `gtest_discover_tests` (at test time,
with a 60 s discovery timeout, since the first run after a runtime redeploy
loads freshly copied DLLs slowly). One executable per module keeps link
dependencies honest: the math tests cannot rely on SQLite, the geometry tests
cannot rely on Qt. `tests/<name>/` is configured only when module
`katana_<name>` is, so `KATANA_MODULE_FILTER` narrows the tests with the
build. Some folders are globbed (`tests/cad/customisation/`,
`tests/cad/tools/`, and the same two under `tests/qt_widgets/`) so that
authors working at the same time do not all edit one list.

### Interactive tools: ToolDriver

A drawing tool is a state machine in `katana_cad` (`docs/tools.md`), tested
with no Qt through `tests/cad/tools/tool_driver.hpp`. `ToolDriver` starts a
catalogue tool by id and feeds it what a user would - `click(x, y)`,
`pick(id, x, y)`, typed text, Enter, Undo - and executes the one command a
finished step returns on its own `Document`, exactly as the plan view does.
The test then asserts on the document against geometry worked out by hand.

### The widgets: qt_widgets

`katana_qt_widget_tests` compiles the view widgets, the workspace, the dock
chrome, the managers and the tool host from `src/katana_qt` into one
executable - the application has no library of its own - and runs on Qt's
offscreen platform. `tests/qt_widgets/widget_harness.hpp` paints a widget,
runs the event loop twice (an activation queued by the first pass is
delivered by the second), makes a window the active one and builds any of the
three views; the managers' tests find their fields and buttons by object name
and assert on the `Document`. What cannot be tested here is anything that
needs `MainWindow`, which is not compiled into this target: that is what the
headless checks are for.

### The whole window: qt_*_headless

A headless check runs the real `katana` under `QT_QPA_PLATFORM=offscreen`
through `tools/check_screenshot.cmake`, which copies the project, runs the
window with switches built from its `-D` variables and checks the exit code,
the log and the PNG. The switches and the variables are `docs/headless.md`:
`-DDRIVE=` drives dialogs, docks and the command line step by step with a
sigil per step (`@` a dialog, `#` a dock, `%` a panel, `>` a command, `?` a
report, `*` a menu item, `!` a button, anything else a fill), `-DEXPECT=`
checks what the run printed, `-DFORBID=` what it must never print,
`-DREFUSED=` that it was refused (exit 1, never a crash), and `-DCOMPARE=`
the files it wrote. A new check is an `add_test` in `tests/CMakeLists.txt`
beside the others, with `PATH` given the toolchain's runtime,
`QT_QPA_PLATFORM=offscreen` and a `TIMEOUT`, because a modal box in a
headless run is a hang.

Choose the lighter tool: a widget test when the widget can be built alone, a
headless check when the behaviour needs the window, its menus or the
interplay of several parts. A headless check proves construction and what
the window SAYS; whether it LOOKS right is for a person, so look at the PNG
the check leaves in the build tree.

### The command line: cli.*

`src/katana_app/CMakeLists.txt` runs `katana_cli` as a process: draw, save,
reopen in a SECOND process and query, so a round trip through the project
file is tested as a user would make it; `WILL_FAIL` cases show bad input
fails the process; fixtures (`FIXTURES_SETUP`, `FIXTURES_REQUIRED`) order the
steps and clean up. The survey-code cases write their own small code file
rather than depend on the git-ignored customisation.

## Data

**Copy before opening.** A test never changes data checked into the
repository. Opening a project can back it up or migrate it in place, so
`check_screenshot.cmake` and `check_plot.cmake` copy the project they are
given into the build tree first, and a test that needs a project of its own
writes it there. The same holds for a person: open a copy of a sample.

Fixtures live beside their suite (`tests/archive12d/data/`,
`tests/surveyio/`); `samples/` holds a small project (`samples/site_plan`), the
script that draws it and a few GIS files. The owner's large real archives are
kept outside the repository and used for measuring and looking, never
committed and never needed by a test. A test that reads the git-ignored
customisation must pass without it (`docs/building.md`, "The customisation
folder").

**No test needs the network.** The online import's tests answer every request
from fixtures (`tests/interop/data/online/`) through
`OnlineEnvironment::transport`, or read local files as `file://` URLs; the
few that reach real services are named `OnlineLive.*` and skip unless
`KATANA_ONLINE_TESTS=1` is set (`docs/gis_online.md`, "Tests").

## What makes a test evidence

**Derived expectations** (`docs/architecture.md`, "Working rules"). The
expected value is worked out independently of the implementation - a closed
form, a textbook identity, a standard's worked example, an exact synthetic
construction, or a hand calculation written in a comment beside the assertion
- never copied from the program's output. A value taken from the code under
test proves only that the code agrees with itself; this project has seen an
early least-squares "solver" that hardcoded its own test's expected answer and
passed. When a test fails, decide which is wrong on the merits: fix the code,
or fix the expectation and justify it from a source outside the program, or
widen a tolerance with the error analysis that justifies the new number.
Never delete or disable a failing test to make a suite green.

The other standing rules:

- **Test names are sentences that state the property**, not the method:
  `StationsOffTheSurfaceReportNoElevationRatherThanZero`,
  `qt_a_tool_refused_for_want_of_a_plan_view_is_left_unchecked_headless`.
- **Prefer inputs whose answers are exact in binary**, so an assertion can be
  exact; where it cannot, the tolerance is a named constant or is derived in a
  comment.
- **Edge cases are named, not assumed**: parallel and coincident lines,
  zero-length segments, tangent circles, nearly parallel geometry, duplicate
  points, degenerate polygons, empty containers, non-finite input, and
  UTM-magnitude coordinates (compute a figure locally and again shifted by
  (500000, 5000000)).
- **New public behaviour needs** the happy path, every documented failure,
  and the degenerate input (empty, zero, coincident, non-finite, enormous).
- **A regression test must be shown to catch its bug**: remove the fix, watch
  the test fail, restore the fix. A test that passes either way is not a
  regression test. Say in the commit that it was done.
- **A property test must reach the cases it claims to cover**: count the
  interesting hits and assert the count is not trivial. It uses the
  fixed-seed generator in `tests/support/property.hpp` (`katana::test::Random`,
  `kPropertyIterations`), so every run explores the same inputs and a failure
  reproduces exactly.
- **Parallel and culled paths are tested against the plain one.** A
  `TaskPool`-parallel result is compared with a `TaskPool(1)` run and must be
  bit-identical; a spatial cull or tile bin is compared with the exhaustive
  scan and must find exactly the same things, including after edits, undo
  and redo (`BothSidesOfTheScanCrossoverGiveTheSameAnswer`).
- **A run that survives by luck proves nothing.** When a headless check
  guards a crash that happened only sometimes, the proof of the mechanism
  belongs in a unit test that fails deterministically without the fix
  (`qt_import_12da_then_toggle_headless` and the listener tests in
  `tests/cad/test_cad.cpp`).

## Benchmarks

Speed is measured, never asserted (Rules 5 and 6, "Measure, then claim").
Benchmarks are Google Benchmark cases in `benchmarks/bench_*.cpp`, globbed
into one `katana_benchmarks` executable, which lands in
`<build>/bin/benchmarks/`. Measure only in a Release tree:

```sh
cmake -S . -B build/wtr -G Ninja -DCMAKE_BUILD_TYPE=Release -DKATANA_BUILD_BENCHMARKS=ON
cmake --build build/wtr -j 3 --target katana_benchmarks
./build/wtr/bin/benchmarks/katana_benchmarks.exe --benchmark_filter='BM_Snap' --benchmark_min_time=0.3s
```

This machine is shared with other builds and often on battery, so one run of
one binary against one run of another measures the machine as much as the
code: single runs of the same binary have differed by up to 2x
(`docs/performance.md`). The practice is:

1. **Keep both builds.** Copy the baseline's `katana_benchmarks.exe` aside
   before building the change.
2. **Alternate them.**
   `python tools/compare_benchmarks.py --alternate ROUNDS FILTER NAME=EXE [NAME=EXE ...]`
   runs every binary in each round, reversing the order every round, three
   repetitions each, and prints the minimum and the median of every
   benchmark's samples in milliseconds of wall time.
3. **Run an A/A control beside the A/B.** Give the SAME binary twice under two
   names (`base=a.exe again=a.exe new=b.exe`). The spread between `base` and
   `again` is the noise on this machine today; an A/B difference inside it is
   no result.
4. **Report ratios and counts** - "new/base 0.62 on the median, A/A
   1.00-1.04, 18 samples each" - not a single absolute time, and name the
   machine and its state.
5. **Commit the benchmark** with the change, so the measurement can be
   repeated, and put the before and after figures in the commit message and
   the module's document.

The two-file form, `compare_benchmarks.py baseline.json candidate.json
[threshold%]`, compares two saved `--benchmark_format=json` runs by the median
of their per-benchmark ratios. It reads CPU time, which under-counts work done
by `TaskPool` threads, so it overstates a parallel path's change (audit
BLD-12); prefer `--alternate`, which reads wall time, and give a parallel
benchmark `UseRealTime()`.

## The docs test

`tools/check_docs.py` runs as the `docs` test (registered only when Python 3
is found) and fails when a document in `docs/` cites something that does not
exist:

- a repository path (`src/...`, `include/...`, `tests/...`, `tools/...`,
  `docs/...`, `cmake/...`, `benchmarks/...`, `resources/...`, `samples/...`,
  and the root build files), in backticks or as a link; paths under a
  git-ignored folder are not checked, since a clean checkout lacks them;
- a backticked qualified name (`Document::setCurrentStyle`,
  `cad::toolCatalog`): its last two names must both appear in one source or
  header file;
- a test name: a `qt_..._headless` or `cli.` test must be registered, and a
  backticked `CamelCase` name that looks like a test case must be a `TEST`
  in `tests/`;
- a citation of a removed document (the root plan, the root contributor
  instructions, the root readme);

and every document in `docs/` must be listed in `docs/index.md`, and
`docs/headless.md` must name every switch `src/katana_qt/main.cpp` parses and
every `-D` variable `tools/check_screenshot.cmake` reads - a new switch is
documented in the commit that adds it. Documents
still being brought up to this standard are named in the script's
`NOT_YET_CHECKED` list, which shrinks as each is fixed.

Run it alone with `python tools/check_docs.py` from the checkout root; it
prints each broken reference with its file and line.
