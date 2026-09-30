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
settings and registers every case with `katana_discover_tests`
(`cmake/KatanaTargetDefaults.cmake`): each case is a test that starts the
executable directly with `--gtest_filter`. The cases are listed at test time,
by running the executable, with a 60 s timeout, since the first run after a
runtime redeploy loads freshly copied DLLs slowly; the list is kept beside the
suite's `CTestTestfile.cmake` and made again only when the executable is
rebuilt. Why this and not `gtest_discover_tests` itself is in
`docs/building.md`, "Build and test speed". One executable per module keeps
link dependencies honest: the math tests cannot rely on SQLite, the geometry tests
cannot rely on Qt. `tests/<name>/` is configured only when module
`katana_<name>` is, so `KATANA_MODULE_FILTER` narrows the tests with the
build. Some folders are globbed (`tests/cad/customisation/`,
`tests/cad/tools/`, and the same two under `tests/qt_widgets/`) so that
authors working at the same time do not all edit one list.

`tests/geo/` is the one suite that belongs to no single module: the
geoprocessing bridge, its bindings and its executor are three
(`docs/geoprocessing.md`). It is configured whenever `katana_app` is built
with the interop module, links `katana_app`, and globs its `test_*.cpp`, as
the geoprocessing packages add theirs side by side. Its window checks are
`tests/geo/headless/*.cmake` and its `cli.*` cases
`src/katana_app/geo/cli/*.cmake`, each included from the file that would
otherwise list them.

### Interactive tools: ToolDriver

A drawing tool is a state machine in `katana_cad` (`docs/tools.md`), tested
with no Qt through `tests/cad/tools/tool_driver.hpp`. `ToolDriver` starts a
catalogue tool by id and feeds it what a user would - `click(x, y)`,
`pick(id, x, y)`, typed text, Enter, Undo - and executes the one command a
finished step returns on its own `Document`, exactly as the plan view does.
The test then asserts on the document against geometry worked out by hand.

### The widgets: qt_widgets

`katana_qt_widget_tests` links the view widgets, the workspace, the dock
chrome, the managers and the tool host - the application's own objects,
`katana_qt_ui` in `src/katana_qt/CMakeLists.txt`, compiled once and linked
into the application too - into one executable, and runs on Qt's offscreen
platform. `tests/qt_widgets/widget_harness.hpp` paints a widget,
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
report, `*` a menu item, `!` a button, `~` the pointer moved to a model
point and `+` a click there, anything else a fill), `-DEXPECT=`
checks what the run printed, `-DFORBID=` what it must never print,
`-DREFUSED=` that it was refused (exit 1, never a crash), and `-DCOMPARE=`
the files it wrote. A new check is an `add_test` in `tests/CMakeLists.txt`
beside the others, with `PATH` given the toolchain's runtime,
`QT_QPA_PLATFORM=offscreen` and a `TIMEOUT`, because a modal box in a
headless run is a hang.

### The same suite on Linux

The suite is written on Windows (MSYS2) and also run on Linux against
conda-forge's toolchain (`tools/setup_linux_toolchain.py`). What differs,
and how the tests hold on both:

- **Qt's offscreen plugin talks.** On Linux it prints "This plugin does not
  support raise()" and the like for each window call it stubs out.
  `tools/check_screenshot.cmake` takes those lines out before it matches
  `-DEXPECT=`, because they are Qt's and not the window's report
  (`docs/headless.md` lists which).
- **GDAL's drivers differ by build.** conda-forge's GDAL 3.13.2 has no PDF
  driver, so its catalogue has 120 leaf algorithms where MSYS2's has 121
  (`gdal driver pdf list-layers`); it has the CAD driver, which reads
  `.dwg`, and MSYS2's has not. A test that counts on a driver asks
  `gis::findFormat` whether it is there and expects what follows.
- **A test's paths are made native.** A dialog test that hands in a Windows
  path (`C:\out\view.png`) builds it with `QDir::toNativeSeparators`: on
  Linux a backslash is a legal file-name character, and
  `QDir::fromNativeSeparators` rightly leaves it alone.
- **The GPU cases need a Vulkan driver.** With Mesa's lavapipe
  (`mesa-vulkan-drivers`) the "OnTheDesktop" cases draw under Xvfb; on a
  machine with no Vulkan driver at all they skip, saying why.
- **A file dialog is answered with the name box unfocused.** The dialog
  tests share `chooseFile` (`tests/qt_widgets/widget_harness.hpp`), which
  takes the focus off the name box before `selectFile`; without that, a
  late poll left the box empty and the dialog waited for ever.

Each of these is explained, with the evidence, in "The eighteen tests that
failed in the Linux cloud container" below.

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

A test command that goes through `cmake -E env "PATH=...;..."` writes the
PATH in the `add_test` itself. Kept in a variable, the semicolons in it split
the list, and the process that runs is not the one meant: the first
`cli.gdal_*` cases failed exactly so, with "no such file or directory".

A semicolon in a `PASS_REGULAR_EXPRESSION` splits it the same way, into a
list of expressions, and the test passes when ANY of them matches - one
fragment of what was meant. Write it `\\;` in the quoted string: the property
then holds `\;`, which ctest's list expansion keeps as one literal `;`. Found
on 2026-09-30, when the three GSI cases whose expressions pin the parser
version passed against a reader built to say "parser=1.1": each quotes a
warning or note with a `;` in it, and a fragment after one matched on its own
(`cli.survey_read_gsi_refuses_a_negative_vertical_reading_at_its_record` had
two). They are escaped now, and fail on 1.1. Not fixed: six other cases in
the same file still carry a bare `;` - `cli.mapfile_list`,
`cli.code_applies_and_reports`, `cli.list_refuses_words_it_does_not_take`,
`cli.utility_draw_refuses_a_line_it_cannot_grade`,
`cli.utility_view_is_refused_headless_naming_area` and
`cli.utility_draw_of_geometry_with_no_method_is_refused_by_the_vertex` - so
each passes on part of its expression. Escaping them is for their areas,
since the whole expression may not match what those verbs print today.

A file written for another program is also handed to an implementation that
is not Katana's: `cli.ifc_the_scenario_export_is_valid_to_ifcopenshell` runs
`tools/check_ifc.py` over the IFC export. Such a test is registered only
where the Python that configured the build can import the judge - an
optional check of the output, never a build requirement (`docs/ifc.md`,
"Validation").

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

## The eighteen tests that failed in the Linux cloud container

On 2026-09-26 sixteen tests failed, and two more failed now and then, in the
Linux container (GCC 16, Qt 6.11.2 from conda-forge, offscreen) while they
passed on the owner's Windows machine. On 2026-09-27 each was traced to its
cause and fixed; a full `QT_QPA_PLATFORM=offscreen ctest --preset
linux-release -j 2 --timeout 300` in the container then passes with no
failures. What was wrong with each, and why it was fixed where it was:

- **Four headless reports** (`qt_the_help_menu_finds_a_verb_and_lists_every_key_headless`,
  `qt_drawing_summary_and_status_json_describe_the_drawing_headless`,
  `qt_plot_and_view_image_dialogs_run_their_verbs_headless`,
  `qt_every_view_menu_item_is_named_and_its_values_are_typed_headless`).
  Their `-DEXPECT=` wants two report lines next to each other. Linux's
  offscreen plugin prints "This plugin does not support propagateSizeHints()"
  between them whenever a window's minimum size changes. The program was not
  at fault. `tools/check_screenshot.cmake` now drops Qt's window-management
  "does not support" lines before it matches (`docs/headless.md`). Putting `.*`
  between the paired lines was rejected: it would stop those checks proving
  that two reports are adjacent. Without the filter all four fail again.
- **Three Windows paths** (`qt_widgets.ScriptRunner.TheLineItWritesIsTheLineItReads`,
  `qt_widgets.PlotDrawing.TheLineItWritesIsTheLineItReads`,
  `qt_widgets.Snapshot.TheLineItWritesIsTheLineItReads`).
  The tests handed the line writer `C:\...` as a platform's file dialog would
  on Windows, and expected `/` back. On Linux `QDir::fromNativeSeparators`
  rightly leaves `\` alone, because POSIX allows it in a file name
  (POSIX.1-2017, 3.170). So the code was right and the input was not a native
  path there. The tests now build their input with `QDir::toNativeSeparators`.
  On Windows that is exactly the old backslashed path, so nothing they assert
  there is weaker. On POSIX there is no separator to convert, and that part of
  the check can only be made on Windows.
- **Three file dialogs: one always, two intermittently**
  (`qt_widgets.SheetViewOptions.AnImageViewsPictureIsChosenAndCopiedInOneStep`,
  `qt_widgets.SheetSetMenu.TheSetIsSavedAndLoadedBackAskingFirst`,
  `qt_widgets.SheetSetMenu.AnotherSetsSheetsAreAppendedInOneStep`).
  - What was wrong. The note written on 2026-09-26 guessed a native dialog.
    That was wrong: a backtrace shows Qt's own widget `QFileDialog` in
    `exec()`, with its file-system thread running. The cause is in how the
    tests answered it. `QFileDialog::selectFile` writes the name into the
    dialog's file-name box only while that box does not have the keyboard
    focus (`qfiledialog.cpp`). The box takes the focus when the dialog is
    activated, which on the offscreen platform happens in the first pass of
    the dialog's own event loop.
  - What followed. A 10 ms poll that ran before the activation chose the file.
    One that ran after it left the box empty, and Open with an empty box
    keeps the dialog up. The image test's poll always came after, and it had
    stopped polling, so it waited for ever. The Sheet Set tests' poll came
    before or after depending on load. Save then wrote the default name, and
    Load waited.
  - The fix. The one answer they now share, `chooseFile` in
    `tests/qt_widgets/widget_harness.hpp`, takes the focus off the box before
    `selectFile`. The image test's helper also cancels a dialog that is still
    up rather than leaving it, so a choice that does not take fails the test
    instead of hanging it.
  - The regression test. `qt_widgets.FileDialogAnswer.TheFileIsChosenWhenTheNameBoxAlreadyHasTheFocus`
    activates the dialog first, which is the late poll's case, every time.
    Without the fix it fails (result 0, no file chosen). With the fix, the
    three dialog tests and the rest of `SheetSetMenu` and `SheetViewOptions`
    passed 20 repeats each, 4 at a time, with 4 more CPU-bound processes
    running.
  - Making the product force non-native dialogs was not needed and was not
    done. The widget dialog is what the offscreen platform gives.
- **Eight GPU desktop cases** (`gpu.GpuSceneView.OnTheDesktop...` x5,
  `qt_widgets_gpu.RenderViewGpu.OnTheDesktop...` x3).
  - What was wrong. "Failed to create Vulkan instance: -9" is
    `VK_ERROR_INCOMPATIBLE_DRIVER`: the Vulkan loader found no driver, because
    the container had no `mesa-vulkan-drivers`. Both the conda-forge loader in
    the toolchain and the system one search `/usr/share/vulkan/icd.d`. With
    the package installed, all eight draw on lavapipe under Xvfb and pass.
    Pointing `VK_DRIVER_FILES` at nothing brings the -9 back.
  - The fix. A machine with no Vulkan driver at all is a fact about the
    machine, so these cases now skip there with that reason, as the device
    cases already skip without a device (`whyNoVulkanDriver`,
    `tests/gpu/gpu_test_support.cpp`, compiled into the widget tests too). A
    driver that is there and fails still fails them.
  - Found on the way. The three `qt_widgets_gpu.*` tests and
    `gpu_offscreen.*` are plain `add_test`s, and a gtest skip exits 0, so
    ctest reported a skipped case as passed. They now carry a
    `SKIP_REGULAR_EXPRESSION`.
  - For the release workflow. A Linux runner that is to exercise these
    installs `xvfb` and `mesa-vulkan-drivers` before configuring. Without them
    the cases skip, saying why, rather than fail.

What still skips in that run, and why, all for reasons outside the program:

- the tests that read the gitignored reference customisation, and the headless
  quit check that is disabled without it;
- the two `OnlineLive.*` cases (`KATANA_ONLINE_TESTS=1`);
- the `gpu.*/Hardware` device cases, since the container has no GPU;
- `GdalAdapter.GdalIsPointedAtTheCertificatesBesideLibcurl`, where the
  environment already names the certificates;
- the offscreen registrations of the desktop-only cases:
  `gpu.GpuSceneView.UnderTheOffscreenPlatformReportsThatItCannotRenderSoTheHostFallsBack`
  under xcb, which `gpu_offscreen.*` runs, and `qt_widgets.RenderViewGpu.OnTheDesktop...`,
  which `qt_widgets_gpu.*` runs.

## Three things the Windows run found in the Linux fixes (2026-09-28)

The Linux-container fixes above and the GDAL work were merged and then built
and run on Windows for the first time, on a Hyper-V guest and on the owner's
machine. Two of the tests added above were correct only on Linux, and one
suite could not be listed:

- **`qt_widgets.FileDialogAnswer.TheFileIsChosenWhenTheNameBoxAlreadyHasTheFocus`**
  compared `QFileDialog::selectedFiles()` with the scratch path as
  `std::filesystem` gives it. Qt returns `/` on every platform (`QDir`: "Qt
  uses '/' as a universal directory separator"), and on Windows the scratch
  path has `\`, so the same file failed to compare. It now compares against
  `QDir::fromNativeSeparators` of the path. What the dialog chose is unchanged.
- **`LibraryData.*` and `simd_*.core`** made their scratch prefixes under one
  fixed name. ctest runs the core suite and its `simd_*` reruns at once, so
  one process removed a folder while another held a file in it open, and the
  `filesystem_error` ended that run. The name now carries the process id.
  Evidence: `ctest -R "^LibraryData\.|^simd_.*\.core$" --parallel 8
  --repeat until-fail:10` failed in its first round before the change and
  passed all ten after it.
- **`katana_qt_widget_tests` was listed with the 5 s default discovery
  timeout**, not the 60 s every other suite has (`cmake/KatanaTargetDefaults.cmake`).
  On its first launch after a build, on a machine whose virus scanner checks
  new files, listing it took 10.7 s, so ctest ran nothing. It now has 60 s.

Four `qt_widgets` cases also died on the guest with "OpenBLAS error: Memory
allocation still failed": 12 test processes at once, each starting OpenBLAS's
thread pool, on a guest with about 5 GB free. All four passed when run again
one at a time there, and they passed on the owner's machine, so the program is
not at fault and nothing was changed for them.

## A sidecar left in samples/gis (2026-09-29, not fixed)

A whole-suite run at `--parallel 4` left `terrain.asc.aux.xml` beside
`samples/gis/terrain.asc`: GDAL's statistics sidecar, which
`InfoVerb.InfoOfTheSampleTerrainDescribesItsRasterAndBand` and
`qt_widgets.DatasetInfoDialog.ItRunsTheInfoLinesAndShowsTheRasterItsBandsAndGdalsJson`
assert is absent (no min or max without statistics being asked for). Both
passed in that run and failed when run again while the file was there. The
headless check `tests/geo/headless/info.cmake` asks for the same file's
statistics, and the sidecar guard in `src/katana_io/geo/processing.cpp`
removes what a run made only after the run ends, so a case reading the file
at the same moment can see it; which test left it behind was not found. The
file is git-ignored, so nothing shows it but the failure. Delete it before
running the suite again.

## Fixed scratch folders shared by parallel runs (2026-09-29, not fixed)

A whole-suite run at `--parallel 6`, while two other worktrees on the same
machine ran theirs, failed
`PipelineVerb.APipelineEndingInWriteKeepsItsRasterAsAReference`: "could not
keep the derived raster: No such file or directory" for `shade.tif` in
`katana-pipeline-verb` under the system temp folder. It passed run again
alone, with nothing changed. Its fixture empties one fixed folder,
`temp_directory_path() / "katana-pipeline-verb"`, before and after every
case, and ctest runs each case in a process of its own: two cases of the
fixture at once, or the same case in another checkout's run, empty the
folder under each other. 29 test files take a fixed `katana-...` folder
under the system temp in the same way. A folder of each process's own (its
id in the name) would end it; until then, a failure of such a case during
another run is to be run again alone before it is believed.

## The clang builds' seven failures (Windows ARM64, macOS)

The first Release run with the whole suite in every job (run 36356917571,
2026-09-27) failed seven tests on each clang/libc++ build that GCC passes.
Most were older than the merge: those jobs had run only part of the suite.
They were reproduced on Windows x86-64 with MSYS2's CLANG64 environment
(clang 22, libc++), which fails the same five that are not macOS-only, and
fixed on 2026-09-29:

- **`std::to_string` of a double** (four tests, and `GenerateRotate`).
  C++26 makes it the shortest text that reads back, as `std::format("{}")`
  (P2587); GCC 16's library does, libc++ still prints `%f`. So the JobXML
  reader wrote "measured 1.650000 m", a refused circle said "[-1.000000]",
  and `GenerateRotate.TheFittedDrawingIsTurnedToFillTheSheetInOneStep` typed
  its strip's corners to the micrometre and got a turn 2.6e-9 rad off. All
  86 such calls (65 in the program, 21 in tests) now use
  `katana::core::formatExactReal`, the shortest round-trip text: by the
  standard the same `to_chars` call C++26's `to_string` makes, so GCC's
  output is unchanged. `tools/check_float_to_string.py` finds any new one; a
  text search cannot, because it cannot tell a double from an int. One test
  that compared against `std::to_string(-33.865)` - the same call as the code
  under test - now compares against the literal "-33.865". The tool exits 1
  when any file could not be checked, as well as on a hit: on 2026-09-30,
  run without MSYS2's bin on PATH, g++ could not load its DLLs, every one of
  320 files was "not checked" with no reason given, and the tool still exited
  0 - a pass that checked nothing. It now names the reason ("the compiler
  exited 1 and printed nothing") and fails. It fails too, saying "nothing to
  check", when the compile database gives it no file at all - one that is
  empty, or lists no source that mentions `to_string`, where the tree has
  some 320 - which passed as "0 of 0 files checked"; and a source the
  database names but the checkout lacks is "not checked" rather than a
  Python traceback. Nothing showed that a check could fire, though: a
  database whose "compiler" was `true` checked every file, found nothing and
  exited 0. So before the files, a canary - one `std::to_string(1.0)` - is
  compiled with the first file's own command and must be reported at its
  line; when it is not, the tool says "the check cannot fire" and exits 1,
  and when it is, the summary says so ("the canary to_string(1.0) was
  reported").
- **`LibraryData.MissingDataBesideALibraryIsNotFound`.** libc++'s
  `weakly_canonical` makes an empty path the working directory, which is
  absolute; libstdc++ leaves it empty. `dataBesideLibraryFile` relied on the
  latter to refuse an empty name, and on libc++ looked beside the working
  directory. It now refuses an empty name first. The old check passed or
  failed by where the test stood, so
  `LibraryData.AnEmptyNameIsRefusedWhereverTheWorkingDirectoryIs` now stands
  inside a prefix that does hold `share/gdal`: on clang it failed without the
  fix and passes with it.
- **The keyboard shortcuts on macOS** (`KeyboardShortcutsDialog.EveryKeyIsListedAndAClashIsMarked`,
  `qt_the_help_menu_finds_a_verb_and_lists_every_key_headless`). The table
  shows a key as the platform writes it, "⌘L" on macOS, and the search read
  only that, so "ctrl+l" found nothing there - a fault in the dialog, not
  only in the tests. The search now also matches the key as typed
  ("Ctrl+L"), and the tests expect each platform's own text. The macOS half
  is checked only by the macOS job: on Windows and Linux the two texts are
  the same.

The ARM64-only suspicion, fused multiply-adds, was ruled out: the build
compiles with `-ffp-contract=off`, and `GenerateRotate` failed on x86-64
clang too.
