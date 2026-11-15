# Testing

What the tests are, how one is added and registered, how the window is tested
without a display, and the rules that make a test evidence rather than
decoration. How to build is `docs/building.md`.

Katana is Rust (`docs/rust.md`). Its tests are of two kinds, run by two
runners: cargo's tests of each crate, and the process-level tests - the
programs run as a person or an agent runs them - which a CMake project that
compiles nothing registers and CTest runs against the programs cargo built
(the test runner, `CMakeLists.txt`; `docs/building.md`). The C++ suite these
were ported from (GoogleTest, one executable per module) went with the C++
tree on 2026-11-15; every one of its cases has a Rust twin or a recorded
exemption (`docs/rust.md`, "Tests are the specification").

## Running

Until the port is built on Windows every run is in WSL, through
`tools/build_visible.sh`, in a console window the owner watches (the
contributor instructions, "Show the build"):

```sh
bash tools/build_visible.sh wsl                          # clippy, build, cargo test, then the runner's whole suite
bash tools/build_visible.sh wsl cargo                    # the Rust workspace alone: clippy, build, test
bash tools/build_visible.sh wsl cargo -p katana_cad      # one crate's tests
bash tools/build_visible.sh wsl test -R '^qt_'           # the runner's suite only: the window's headless checks
bash tools/build_visible.sh wsl test -R '^cli\.'         # the command line's
```

Inside WSL, by hand, the same is `cargo test --release` in `rust/` and `ctest`
in the runner's build folder (`~/katana-build/<checkout>/tests`).

Run the WHOLE suite before a commit, not only the crate touched: the `docs`
test is part of it, and a change in one crate breaks another's tests more
often than its own.

## What there is

| Kind | Registered by | Named | Where |
|---|---|---|---|
| unit, integration, property and regression cases | `#[test]` | `<module>::<case>`, the twin of the C++ `<Suite>.<Case>` in snake case | `rust/crates/<crate>/tests/` (and a crate's own `#[cfg(test)]` modules) |
| the window's own tests, with no display | `#[test]`, `rust/crates/katana_window/tests/` | `<module>::<case>` | the headless runner (`rust/crates/katana_window/src/drive/`) |
| the whole window, headless | `add_test` in `tests/CMakeLists.txt` (and `tests/geo/headless/`, `tests/update/headless/`, `tests/interop/`) | `qt_<what it shows>_headless` | `tools/check_screenshot.cmake`, `tools/check_plot.cmake` |
| end-to-end command line | `add_test` in `tests/cli/CMakeLists.txt` (and `tests/cli/geo/*.cmake` and the files beside it) | `cli.<what it shows>` | `katana_cli` run as a process |
| what the programs install | `tests/CMakeLists.txt` | `packaging_check_on_fixture_<what>`, `packaging_check_on_deploy_script_<what>` | `tools/check_install_rules.cmake` ("What the programs hold", below) |
| the documents' references | `tests/CMakeLists.txt` | `docs` | `tools/check_docs.py` ("The docs test", below) |
| Katana Standard is what its data makes | `tests/CMakeLists.txt` | `katana_standard_current`, `katana_standard_<check>` | `tools/katana_standard/` (`docs/katana_standard.md`) |
| the update system end to end, on a feed of the tests' own | `tests/update/e2e.cmake` | `update_e2e_<what it shows>` | `tests/update/update_e2e.py` driving `tools/update_fake_feed.py`, `katana_update_e2e` and `katana_updater_test` ("The update system's tests", below) |
| the release tools, and the build's promises about updating | `tests/update/CMakeLists.txt` | `update_tools_<tool>`, `update_keys_<what it shows>`, `katana_updater_imports_only_os_dlls`, `update_test_keys_are_in_no_shipped_program` | Python `unittest` files in `tests/update/tool_tests/`; CMake scripts in `tests/update/` |

`cargo test -- --list` and `ctest -N` give the current count of each; this
document does not, because a number here would be stale by the next commit.

The C++ suite also registered the layering check (`layering`, and since the
port `rust_layering` and `rust_parity`), the link-closure checks of the legacy
customisation readers and the SIMD kernel checks. The layering check survives
as `tools/check_rust_layering.py` over the table `tools/check_layering.cmake`,
not registered (`docs/architecture.md`, "Layering"); the parity check had
nothing left to compare once the C++ cases were gone; the readers were never
ported; and `#[target_feature]` leaves the kernel checks nothing to find
(`docs/performance.md`).

### A crate's tests

A crate's integration tests are `rust/crates/<crate>/tests/<file>.rs` or a
folder with a `main.rs` that names its modules (`mod plan_view;`), each module
holding the cases of one C++ suite: the twin of `Suite.CaseName` is
`fn case_name` inside `mod suite`, or at the top of `suite.rs` (the rule
`tools/check_docs.py` resolves a cited `<suite>::<case_name>` by). Shared helpers
are a `support` module beside them (`rust/crates/katana_cad/tests/support/`,
`rust/crates/katana_window/tests/support/window_support.rs`). A crate's tests
can use only what the crate and its `[dev-dependencies]` give them, so the
math tests cannot rely on SQLite and the geometry tests cannot rely on the
window - the property the C++ got from one executable per module. A folder of
cases whose environment differs (`customisation_env`, `customisation_verbs_env`:
cases that set the built-in's variable) is a test binary of its own: cargo runs one binary's cases on threads of one
process, where the variable is the process's, so each such case holds a lock
for its whole length and puts the variable back before it lets go.

The geoprocessing tests are split across the crates they test - the bridge
and its policy in `katana_gis`, the bindings in `katana_interop`, the
executor and the verbs in `katana_app` (`docs/geoprocessing.md`). Their
window checks are `tests/geo/headless/*.cmake` and their `cli.*` cases
`tests/cli/geo/*.cmake`, each included from the file that would otherwise
list them.

### Interactive tools: ToolDriver

A drawing tool is a state machine in `katana_cad` (`docs/tools.md`), tested
with no window through `rust/crates/katana_cad/tests/support/tool_driver.rs`. `ToolDriver` starts a
catalogue tool by id and feeds it what a user would - `click(x, y)`,
`pick(id, x, y)`, typed text, Enter, Undo - and executes the one command a
finished step returns on its own `Document`, exactly as the plan view does.
The test then asserts on the document against geometry worked out by hand.

### The window's tests

`rust/crates/katana_window/tests/` builds the views, the workspace, the dock
chrome, the managers, the dialogs and the main window with no display: the
headless runner draws egui frames on the CPU (`docs/headless.md`), and
`rust/crates/katana_window/tests/support/window_support.rs` draws a frame,
draws until the window is idle, and finds a control by its name - the object
name the C++ widget had - to fill or press it, as a person would; the tests
assert on the `Document`. The C++ widget suite (`qt_widgets.*`) could not
build `MainWindow`; the Rust's can, so a case that needed the whole window no
longer needs a process of its own.

### The whole window: qt_*_headless

A headless check runs the real `katana` (the cargo-built program, which the
runner names `KATANA_PROGRAM_katana`) through `tools/check_screenshot.cmake`,
which copies the project, runs the window with switches built from its `-D`
variables and checks the exit code, the log and the PNG. The switches and the
variables are `docs/headless.md`: `-DDRIVE=` drives dialogs, docks and the
command line step by step with a sigil per step (`@` a dialog, `#` a dock,
`%` a panel, `>` a command, `?` a report, `*` a menu item, `!` a button, `~`
the pointer moved to a model point and `+` a click there, anything else a
fill), `-DEXPECT=` checks what the run printed, `-DFORBID=` what it must never
print, `-DREFUSED=` that it was refused (exit 1, never a crash), and
`-DCOMPARE=` the files it wrote. A new check is an `add_test` in
`tests/CMakeLists.txt` beside the others, with a `TIMEOUT`, because a run that
waits on something is a hang. The names are still `qt_`: the tests are the
C++ window's, run against the Rust one, and a test's name is how it is cited.

### Windows and Linux

The suite was written on Windows (MSYS2) and also run on Linux against
conda-forge's toolchain (`tools/setup_linux_toolchain.py`), which is where
the Rust is built and tested today (WSL). What differs, and how the tests
hold on both:

- **GDAL's drivers differ by build.** conda-forge's GDAL 3.13.2 has no PDF
  driver, so its catalogue has 120 leaf algorithms where MSYS2's has 121
  (`gdal driver pdf list-layers`); it has the CAD driver, which reads
  `.dwg`, and MSYS2's has not. A test that counts on a driver asks
  `katana_gis::formats::find_format` whether it is there and expects what follows.
- **A test's paths are made native.** A dialog test that hands in a Windows
  path (`C:\out\view.png`) builds it with the platform's separators: on
  Linux a backslash is a legal file-name character, and it is rightly left
  alone.
- **The GPU cases need a Vulkan driver.** With Mesa's lavapipe
  (`mesa-vulkan-drivers`) the device cases draw; on a machine with no Vulkan
  driver at all they skip, saying why (`docs/gpu.md`, "Testing").

The C++ suite's further Linux differences - Qt's offscreen plugin's messages,
which `tools/check_screenshot.cmake` still drops, and the file dialog
answered with its name box unfocused - are explained, with the evidence, in
"The eighteen tests that failed in the Linux cloud container" below.

Choose the lighter tool: a crate's test when the behaviour can be reached
from its crate, a headless check when the behaviour needs the program, its
menus or the interplay of several parts. A headless check proves construction
and what the window SAYS; whether it LOOKS right is for a person, so look at
the PNG the check leaves in the runner's build folder.

### The command line: cli.*


`tests/cli/CMakeLists.txt` runs `katana_cli` as a process: draw, save,
reopen in a SECOND process and query, so a round trip through the project
file is tested as a user would make it; `WILL_FAIL` cases show bad input
fails the process; fixtures (`FIXTURES_SETUP`, `FIXTURES_REQUIRED`) order the
steps and clean up. A bare `WILL_FAIL` passes on ANY failure - an earlier line
of the script refused, or `katana_cli` crashing (`cmake -E env` turns a crash
in its child into an ordinary exit 1) - so where a refusal is the property, the
case names it with `PASS_REGULAR_EXPRESSION` (the earlier line's success and
the refusal's own words), never beside `WILL_FAIL`, which inverts the
pattern's result
(`cli.a_surface_leader_on_no_surface_fails_the_script`). The survey-code cases write their own small Katana
customisation file rather than depend on the compiled-in one. The cases that
pin what a session STARTS with say so on their own command line, through
`cmake -E env`: `KATANA_BUILTIN_CUSTOMISATION` naming a committed fixture of
`tests/data/customisation` as the built-in of the run (or `none`), and
`KATANA_CUSTOMISATION` naming the kept file (`docs/customisation.md`, "What
katana_cli and katana_mcp start with"). A case that names a built-in and no
kept file unsets `KATANA_CUSTOMISATION` (`--unset=`), so that one set in the
caller's shell cannot start the session instead.

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
two). They are escaped now, and fail on 1.1. Two of the survey-code cases
were escaped on 2026-10-06, when their verbs were reworded (`cli.code_list`,
named after the verb `MAPFILE LIST` until then, and
`cli.code_applies_and_reports`): each whole expression matched what its verb
prints. Not fixed: four other cases in
the same file still carry a bare `;` -
`cli.list_refuses_words_it_does_not_take`,
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
`tests/surveyio/`), except the three small customisations of
`tests/data/customisation/`, which more than one suite loads
(`docs/customisation.md`, "The fixtures, and what the converter is held to"),
and the one of `tests/data/field_codes/`, which the survey import's tests load;
`samples/` holds a small project (`samples/site_plan`), the
script that draws it and a few GIS files. The owner's large real archives are
kept outside the repository and used for measuring and looking, never
committed and never needed by a test. A test that reads the git-ignored
reference customisation, or the compiled-in one, must pass without it
(`docs/building.md`, "The built-in customisation and the reference folder").

**The suites that read third-party data** are these: `built_in_census`, the
NSW half of `built_in_renames` and the NSW cases of `built_in_library`
(`rust/crates/katana_cad/tests/customisation/`), which read the NSW customisation
compiled in as an alternate built-in, found by name. (The C++ build's legacy
readers' and writers' reference tests, which read the git-ignored reference
folder, were never ported and went with it.) Each skips, saying why, where
its data is absent - a build has a built-in unless its path names no file,
the file being tracked - and none is ever changed to match what
the program gave: the census's figures are counted by
`tools/customisation_census.py` and `tools/reference_census.py`, which use none
of the program's code, and no tracked test or document holds a name from the
data, only counts (the converted file, being the data, holds them). A build
with neither was built and run on 2026-10-07, when the file was not tracked
(`-DKATANA_BUILTIN_CUSTOMISATION` naming a file that does not exist, when that
file was the only built-in; today the equivalent is
`-DKATANA_ALTERNATE_CUSTOMISATIONS=` with an empty list, which leaves Katana
Standard alone; the cad and app suites): it configures, builds and passes.

**What the programs hold** is proved on fixtures, since a check that cannot
see a violation passes as quietly as one that finds none (`tests/CMakeLists.txt`).
The C++ build proved it twice: a configure-time walk of the four programs'
link closure (the legacy customisation readers must be in none of them), and
the install rules. The readers were never ported, so the first has nothing to
find and went with the C++; the second stays:

- the install rules. `tools/check_install_rules.cmake` reads the install
  scripts CMake wrote (the top one and those it includes) and fails on any
  program but `katana`, `katana_cli`, `katana_mcp` and the update helper
  `katana_updater` (the two executables the update tests build with a test key
  are refused by name), and on a build that installs no program at all. It
  also fails if `share/katana/install.json`, the file that makes an install a
  release, is written by anything but CPack's pre-build script, and if the one
  `foreach(_program ...)` line of `cmake/KatanaDeployUnix.cmake.in` that lists
  the programs scanned for libraries leaves out an installed program: a Windows
  build never runs that script, so nothing else would notice.
  `packaging_check_on_fixture_*` run the check on install scripts written at
  configure time (the programs, with and without the helper; the converter and
  the test executables installed from a directory below, which must be named;
  none, which must say there is nothing to check; and the install-record
  cases), and `packaging_check_on_deploy_script_*` on deploy scripts written
  there (one that scans all four, one that leaves the helper out, one whose list
  is split over two lines, and a missing one).

**No test needs the network.** The online import's tests answer every request
from fixtures (`tests/interop/data/online/`) through
`OnlineEnvironment::transport`, or read local files as `file://` URLs; the
few that reach real services are named `online_live` and skip unless
`KATANA_ONLINE_TESTS=1` is set (`docs/gis_online.md`, "Tests").

**The loopback rule.** A test that reaches any host other than the loopback
address runs only with `KATANA_ONLINE_TESTS=1`. A test MAY start its own
server on `127.0.0.1`, on port 0 - the system picks a free one - with proxies
disabled, and such a test counts as local: it runs in every suite run (the
owner's ruling of 2026-10-08, decision D4 of `docs/update.md`). The update
system's end-to-end tests are the ones that do ("The update system's tests",
below). What keeps them on the machine is not good behaviour but
construction: the server binds `127.0.0.1` and has no option to bind anything
else, and the client the tests fetch with can connect to no other address.

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
  `cad_section::stations_off_the_surface_report_no_elevation_rather_than_zero`,
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
  fixed-seed generator in `rust/crates/katana_cad/tests/support/property.rs` (`property::Random`,
  `PROPERTY_ITERATIONS`), so every run explores the same inputs and a failure
  reproduces exactly.
- **Parallel and culled paths are tested against the plain one.** A
  `TaskPool`-parallel result is compared with a `TaskPool(1)` run and must be
  bit-identical; a spatial cull or tile bin is compared with the exhaustive
  scan and must find exactly the same things, including after edits, undo
  and redo (`indexed_queries::both_sides_of_the_scan_crossover_give_the_same_answer`).
- **A run that survives by luck proves nothing.** When a headless check
  guards a crash that happened only sometimes, the proof of the mechanism
  belongs in a unit test that fails deterministically without the fix
  (`qt_import_12da_then_toggle_headless` and the listener tests in
  `rust/crates/katana_cad/tests/interpreter/cad.rs`).

## The update system's tests

An update replaces the programs of an install, so a mistake in it is the one
kind that a later release cannot repair. Its tests are built so that the
real code runs against things it cannot tell from the real ones - a feed, a
signed release, a file system that fails - and none of them touches a real
server, a real key, an installed copy or the machine's registry
(`docs/update.md` is the system; `tests/update/CMakeLists.txt` registers
what follows).

**Where the expectations come from.** Never from the program: the Ed25519
answers are RFC 8032's published vectors and an independent derivation of
them (`tests/update/tool_tests/rfc8032_oracle.py`); what a release's zip
holds is asked of Python's `zipfile` and `hashlib`; a signature is made by
the `openssl` command line; the tree an update must leave is built from the
specification of the two releases, in a folder of its own, and compared as a
whole.

**The engine, in process** (`katana_update_tests`, `rust/crates/katana_update/tests/*.rs`).
Everything the engine does to the machine goes through an interface -
`FsOps`, `ProcessOps`, `Launcher`, `RegistryOps`, `CredentialStore`, `Fetch` -
and the tests hand it fakes: a transport that records what each hop was
sent, a launcher that counts what it was asked to start, a registry that is
a map. The trust root is a parameter too, so these tests verify releases
signed with the published test key, which no shipped program trusts
(`rust/crates/katana_update/tests/test_key/mod.rs`).

**The fault loops** (`rust/crates/katana_update/tests/fault_loops.rs`). `FaultFs`
(`rust/crates/katana_update/src/fault_fs.rs`) wraps a file system, counts its
changing operations and can make the k-th one not happen, as if the process
had died just before it. Three loops rest on it, each run over a 40-file
release and over the fixture releases the producer made:

1. the swap is crashed before EVERY one of its file operations, recovery
   runs on what the crash left, and the whole tree must hash to the old
   version's or the new one's, never a mix - the old one unless the crash
   came after the journal said the swap was complete
   (`fault_loops::crashing_before_every_operation_of_a_forty_file_update_leaves_the_old_tree_or_the_new_never_a_mix`,
   `fault_loops::the_same_loop_over_the_fixture_releases_made_by_the_producer`);
2. the crash INSIDE the recovery: the swap is interrupted, the recovery is
   crashed before every one of its operations, and the recovery after that
   must still end at the old tree. The loop names six interrupted states -
   before the commit is recorded, before its first rename, half way through
   the renames, after the last, before the swap is recorded as complete, and
   inside the swap's own rollback - and the two runs do NOT take the same
   ones. The fixture run takes all six
   (`fault_loops::the_recovery_loop_over_the_fixture_releases`); the 40-file run takes the
   half-way state alone, the largest recovery there is
   (`fault_loops::crashing_inside_recovery_at_every_operation_still_ends_at_the_old_tree`). So the
   other five states are crashed into only on the small update; the test's
   reason is that what a recovery does does not depend on how many files
   there are;
3. an error 32, a locked file, at every rename of the commit: the update
   must end rolled back, with the tree exactly the old one
   (`fault_loops::an_error_thirty_two_at_every_rename_of_a_forty_file_update_ends_rolled_back_to_exactly_the_old_tree`,
   `fault_loops::the_same_loop_over_the_fixture_releases`).

A loop that crashed nowhere would pass, so each shows that it reached what
it claims: a crash at every k, at the operation numbered k, and crashes
that left the journal in each state a swap passes through. The loops are
deterministic: the fault is the k-th operation, not a moment in time.

**An update whose helper died during the health check**
(`rust/crates/katana_update/tests/recover.rs`, the suites `SettleUnproved`, `SettleLoops`,
`ConfirmStarted` and `RecoverSuperseded`; `docs/update.md` has the rule under
that name). The state is never written by hand: the fixture update is applied
on a `FaultFs` that dies before the operation a clean run's log names as the
`HEALTHY` append, which leaves what a helper that lost power leaves. The
expectations are the specification's, worked out by hand:

- WHICH TREE an install must end as: the new version's, built from the
  release in a folder of its own, or the old one's, the snapshot taken before
  the update - never what the code left behind;
- "nothing written" is zero changes counted by a `FaultFs` under the code,
  and the install (its `.update` included) and the per-user folder compared
  entry by entry, byte by byte;
- the ORDER of the registry against the disk is read off a registry double
  that notes, at each call, how many changes to the disk had been made by
  then (`settle_unproved::the_logon_net_is_put_back_first_and_the_version_is_rewritten_before_the_record_is_written`);
- the `RunOnce` command is written out as the swap's step 4 gives it.

What they hold: with its marker there the update is settled healthy, the
per-user folder only read, and a second run changes nothing
(`settle_unproved::an_update_whose_marker_is_there_is_settled_healthy_and_the_per_user_folder_is_only_read`);
with the marker gone and nobody running it is put back, and nothing per user
says so (`settle_unproved::an_update_with_no_marker_and_nobody_running_is_put_back_and_nothing_per_user_says_so`);
a program found by the first scan, or only by the second, a scan that fails,
or one file of the new version overwritten each leave the journal, the tree
and the folder as they were, with the logon entry registered
(`settle_unproved::nothing_is_put_back_under_a_running_program_or_over_files_that_are_no_longer_that_release`);
a held lock is a live helper, and then not even the registry is written
(`AHeldLockIsALiveHelperAndNothingIsWrittenNotEvenTheRunOnceEntry`); a marker
missing from another per-user folder, old files that are not all kept, a
lock that cannot be opened and a folder that cannot be read through decide
nothing. Nor does a journal that is there and cannot be read, which a file
system double gives by refusing that one read as a scanner holding the file
does: the settlement leaves the update with the logon entry registered,
changes nothing and asks no scan
(`AJournalThatCannotBeReadIsAnUpdateThatMayBeThereItIsLeftAndTheLogonEntryIsPutBack`,
and for an `R/.update` that will not be listed
`settle_unproved::an_update_folder_that_cannot_be_listed_at_the_first_look_is_not_an_install_with_nothing_to_settle`);
a journal held for the first look alone is read under the lock
(`AJournalHeldOnlyAtTheFirstLookIsReadUnderTheLockAndTheUpdateIsSettled`);
the window is told to ask again and writes no marker
(`AJournalThatCannotBeReadJustNowIsAskedAboutAgainAndNoMarkerIsWrittenForIt`);
and the next swap is refused with the install and the download as they
were (`recover_superseded::an_earlier_update_whose_journal_cannot_be_read_stops_the_next_update_as_an_unsettled_one_does`).
The window's half is `katana_update::recover::confirm_started_with`
(`confirm_started::the_window_of_the_new_version_settles_the_update_healthy_and_clears_what_the_dead_helper_left`,
`AHeldLockIsALiveHelperTheWindowAsksOnceAndChangesNothing`), and the next
swap's is step 1 of the swap itself
(`recover_superseded::an_earlier_update_whose_marker_is_there_is_settled_first_whatever_becomes_of_the_next_one`,
`recover_superseded::an_earlier_update_that_was_never_said_to_start_stops_the_next_one_before_it_touches_anything`).
That a program's own recovery leaves such an update exactly as it is, with
no lock and nothing started, is `recover_with::an_update_that_was_never_said_to_start_is_left_exactly_as_it_is_with_no_lock_and_no_helper`.

Three more loops, of the same kind as the fault loops, die before EVERY
change to the disk of one settlement and then do what the machine does next
- a recovery and a settlement on a file system that works - and require the
whole tree to be one version's: of a healthy settlement always the new one
(`settle_loops::dying_before_every_change_of_a_healthy_settlement_always_ends_at_the_new_tree`), of a
putting back always the old one
(`settle_loops::dying_before_every_change_of_a_putting_back_always_ends_at_the_old_tree`), and of a
window's confirming the old one while its marker is not yet written and the
new one from then on
(`settle_loops::dying_before_every_change_of_a_windows_confirming_ends_at_the_old_tree_before_its_marker_and_the_new_one_after`).
Each requires a death at every k, at operation k.

The same through the REAL helper (`rust/crates/katana_updater/tests/helper.rs`):
`katana_updater_test` is killed with `--die-at` before its `HEALTHY` append,
after a real program stood in for the new version and wrote the marker; then
`--recover` is run as a logon runs it, the `RunOnce` entry consumed first.
It exits 0 with `detail=confirmed:`, the tree the new version's, the
per-user folder unchanged and the entry gone
(`helper_swap::an_update_whose_helper_died_after_the_new_version_said_it_starts_is_confirmed_by_recover_which_only_reads_the_per_user_folder`);
with the marker deleted it puts the old version back and writes no
`last.json` and no `state.json`
(`helper_swap::an_update_whose_new_version_never_said_it_starts_is_put_back_by_recover_when_nothing_of_the_install_is_running`);
with a stub running from the install it changes nothing of the update - the
tree, the journal and the per-user folder are as they were - writes its one
line in `R/.update/recovery.log`, the update's id after `left=`, and the
entry is the command again; with the stub closed the next run puts the old
version back
(`helper_swap::recover_puts_nothing_back_under_a_running_program_and_puts_the_logon_entry_back_for_the_next_try`);
and killed before its own first change it had put the entry back already
(`ARecoverThatDiesWhileItSettlesHadPutTheLogonEntryBackFirstAndTheNextOneFinishes`).
What was there before holds unchanged, which is the point of leaving
recovery itself alone: `recover::every_interrupted_state_is_put_back_and_every_finished_one_is_left_alone`,
the three fault loops, and
`helper_swap::recovery_started_the_way_a_headless_program_starts_it_writes_the_recovery_log_and_no_per_user_state`.

**The second scan** has a process seam whose answers are a list, one for
each time it is asked, so that the second can differ from the first
(`rust/crates/katana_update/tests/apply.rs`, suite `ApplyWait`): nobody and then
`katana_cli` is `blocked` with the install untouched, the download kept and
no `RunOnce` entry ever registered, and the Retry then installs from that
same stage
(`AProgramStartedWhileThePlanWasMadeBlocksTheUpdateAndTheInstallAndTheDownloadAreAsBefore`);
the PLAN group is the last change to the disk before the second question
and `COMMITTING` the first after it
(`apply_wait::the_second_look_is_the_last_thing_before_the_journal_says_committing`); and a scan
that gives no answer the second time is a refusal, not "nobody"
(`AScanThatFailsAtTheSecondLookIsARefusalTooAndNothingIsTouched`).

**The fake feed** (`tools/update_fake_feed.py`). A release host for the
tests, of the standard library alone: it serves one folder - a manifest, its
signature, the update zip - on `127.0.0.1`, port 0, and on a second port
that stands for the storage host a release redirects to. It answers `Range`
as a release host does (206 with `Content-Range`, 501 for several ranges in
one request) and misbehaves on request: a status (`--status 429:GLOB`), a
sign-in page in the place of a file (`--html`), a redirect to the second
port with a signed-looking query (`--redirect`), `Range` ignored
(`--ignore-range`), a connection that drops (`--truncate`), and GitHub's
answers for a private repository, token and all (`--github`, `--token`).
Every request on either port is a record in its log - the method, the path,
the headers as they arrived, what was served - which is how a test proves
what went over the wire: which bytes of the zip were asked for, and that
the second hop carried no `Authorization`. `update_tools_fake_feed`
(`tests/update/tool_tests/test_fake_feed.py`) asks it with Python's own
client.

**The loopback, and nothing else.** The fake feed binds the loopback
address and has no option to bind another; `katana_update_e2e`, the client
the end-to-end tests fetch with, connects to `127.0.0.1` and a port and to
nothing else, looks no name up, and refuses a feed address that is not one
(`update_e2e_program_refuses_an_https_feed` and its neighbours). The tests'
environment unsets every proxy variable and sets `NO_PROXY=127.0.0.1`. So
these tests run without `KATANA_ONLINE_TESTS` ("The loopback rule", above).

**Three programs the tests build and nothing ships**
(`tools/check_install_rules.cmake` refuses the first two by name):

- `katana_update_e2e` (`rust/crates/katana_updater/src/bin/katana_update_e2e.rs`): the engine from a
  command line - check, plan, stage, preflight, launch, recover - with the
  test key as its trust;
- `katana_updater_test`: the helper's OWN `main.rs`, compiled with the test
  key in the place of the pinned ones and four options that exist only
  there - `--die-at N` (a `FaultFs`: the N-th change is not made and the
  process ends), `--registry-base K` (its registry values re-rooted under a
  key of the test's), `--timings` (so that a test does not wait a minute)
  and `--stand-in P`. Without a registry base it uses a registry that has
  nothing, so a test that forgets the option leaves no `RunOnce` entry on
  the machine;
- `katana_updater_stub` (`rust/crates/katana_updater/src/bin/katana_updater_stub.rs`): a real program
  that stands in for Katana when the helper starts the new version. A test
  never starts a file that is not a program: Windows answers that with a
  message box somebody has to click.

**The end-to-end tests** (`tests/update/update_e2e.py`, one case a ctest).
Nothing in them is a fake of the code under test: the real producer makes
and signs two releases, the fake feed serves them, `katana_update_e2e` is
the engine and `katana_updater_test` the helper of every install. Fifteen
cases: the update from the first release to the second (the tree is exactly
the second's, and the feed served only the bytes that changed); a member
that is not the signed file; a file held open, which rolls back; the helper
killed before every change it makes, then recovery (in four shards, to
spread it over the cores); a downgrade, a replayed release, a manifest with
one bit changed, a signature of another manifest, another key, another
target; a program of the install still running; a new version that exits
with 1, which is rolled back and not offered again; the token, which goes to
the API and to no address a redirect names; a feed that cannot be read; a
download that fails and then succeeds. Every folder a case writes is
temporary and removed at its end, and the install's has a blank and a letter
outside ASCII in its name, as a folder under a person's profile has. Five of
the cases start no program of the install and
so run on Linux and macOS too; the others are Windows only, where the swap
is written. `update_e2e_every_case_of_the_driver_is_registered` fails when
the driver has a case that nothing registered. They need Python 3.10 and
`openssl` 3; without them they are not registered - not registered as
skipped, since a skipped test passes as quietly as one that ran.

**The build's promises**, each proved on a fixture as well, since a check
that cannot see a violation passes as quietly as one that finds none:

- the pinned keys: `update_keys_*` run `cmake/KatanaUpdateKeys.cmake` as a
  script over keys that are not the tree's - placeholders are let through
  loudly, refused when `KATANA_REQUIRE_UPDATE_KEYS` is ON, and a published
  test key is refused in either slot;
- the helper's imports: `katana_updater_imports_only_os_dlls` reads the
  import table of the helper (Windows), and
  `updater_import_check_names_the_gcc_runtime` runs the same check on the
  listing of a program that is not static, which it must name. The C++
  helper carried libcrypto and zlib, and a link probe held the whole link
  line; the Rust helper links neither, its signature and inflate crates
  being pure Rust (`docs/architecture.md`), so the import table is the
  whole of it;
- no test key in what ships: `update_test_keys_are_in_no_shipped_program`
  looks for the test keys in the bytes of the four programs, and the two
  test executables that DO link them are its controls;
- the release tools: `update_tools_make_manifest`, `update_tools_keytool`,
  `update_tools_verify_release`, `update_tools_release_sign` and
  `update_tools_fixtures`, Python `unittest` files judged by `zipfile`,
  `hashlib`, the RFC's vectors and `openssl`;
- the installer: `packaging_nsis_template_is_per_user` holds the installer
  script's text to its rules, and each `packaging_nsis_check_catches_*`
  breaks one rule in that text (`docs/release.md`, "The Windows
  installer");
- the install record: the `packaging_install_record_*` tests run the
  script CPack runs, on a staged folder of their own that is emptied
  first, and hold the `install.json` it leaves to the whole text written
  out in the test - this build's, one of a release (the version, then the
  sequence) and one of no release (the version alone). The rule a
  sequence is held to when a build is configured is put to the least and
  the greatest a manifest may carry and to what is refused - nought, a
  sign, a fraction, an exponent, a leading zero, a number past the
  greatest - by `packaging_takes_a_release_sequence_and_refuses_what_no_manifest_could_carry`
  (`cmake/KatanaPackaging.cmake`, "install.json").

**The surfaces.** The verbs run for real over a fake transport, a scratch
install and the signed fixtures (`rust/crates/katana_app/tests/session/update_verbs.rs`, suite
`UpdateVerbs`); `katana_cli` and the window are started from a build tree,
which is a development copy and asks no server
(`tests/cli/update_cli.cmake` and `tests/cli/update_blocked.cmake`,
the `cli.update_*` cases; `tests/update/headless/update_window.cmake`); the
dialog, its sentences and the window's decisions are widget tests
(`rust/crates/katana_window/tests/update/update_dialog.rs`), and the Command Reference's
Updates section is held to the verbs' own help
(`rust/crates/katana_window/tests/host/command_reference.rs`). Each run of a program is
given an update state folder of the tests' own (`KATANA_UPDATE_STATE_DIR`),
and a token of the tests' own or none, so that nothing of whoever runs the
suite is read or written.

Three things about a machine that a test must not take from the machine it
runs on:

- **How the suite was started.** `UPDATE APPLY CONFIRM` is refused in a
  Katana that was started as administrator, and whether the suite's own
  process is one is not for a test to assume. The verbs' tests SET the
  context's member (`UpdateContext::elevated`:
  `update_verbs::apply_is_refused_in_a_katana_started_as_administrator_before_any_helper_is_copied`),
  and the one decision of `katana_update::process_is_elevated` - which kind of token
  is a yes - is a function of its own to which every kind is put
  (`process_is_elevated::only_the_elevated_half_of_a_split_token_is_a_yes`). What holds of every run is
  held of this one: the answer is steady, and never "yes" for a token
  Windows itself calls not elevated
  (`process_is_elevated::the_answer_is_steady_and_is_never_yes_for_a_token_windows_calls_not_elevated`). A
  "yes" from the system is seen only by a suite that holds the elevated
  half of a split token. Recovery is told in the same way: `katana_update::recover`
  hands the answer to `katana_update::recover::recover_with`, and the test hands it both
  answers - started as administrator, the install's own helper is run and
  never the copy in the per-user folder, and with only the copy there
  nothing is run
  (`AProgramStartedAsAdministratorRunsTheInstallsOwnHelperAndNeverTheCopyInThePerUserFolder`).
- **Which version the suite was built as.** A copy whose `install.json`
  says its target and no more is stood for by the program that reads it:
  its installed version is the higher of the helper's record and the
  program's own, so what `katana_update::install::read_install` answers for it depends on
  the build (`docs/update.md`, "The installed version"). The rule is
  therefore shown through `katana_update::install::read_install_as`, the same reading with
  the program's version handed in, for a program below, at and above a
  record (`install_records::of_a_record_and_a_program_the_higher_version_is_the_installed_one_in_both_orders`,
  `AStaleRecordUnderNewerProgramsLetsNoReleaseBetweenTheTwoBackIn`), and so
  is the reading of a package that names its own build, which no
  program's version moves
  (`install_records::of_the_helpers_record_and_the_packages_own_build_the_higher_is_the_installed_one`);
  the cases that read with this build's own version place their record by
  `KATANA_EXPECTED_VERSION`, which the build hands the suite from
  `project(Katana VERSION ...)`.

  The verbs' test that reads an install's records through this program
  asserts no premise about the project's version any more. Its install
  names the package's own build, 0.8.0, under the helper's record of the
  fixtures' 0.9.0, so the program is not asked and the answer is 0.9.0
  whatever the suite was built as
  (`update_verbs::the_context_of_a_program_in_an_install_is_that_installs_own_records`). The reading
  that does follow the build is the test beside it, whose record is 0.1.0,
  a version the project left behind
  (`update_verbs::the_context_of_an_install_whose_record_is_below_this_program_says_this_programs_version`).

  The swap's tests are the other place the build's version reaches, since
  the helper reads the install it updates. Every install their harness
  writes therefore names the version of the package it came from
  (`INSTALL_JSON` in `rust/crates/katana_update/tests/apply_harness/mod.rs`: 0.1.0, at or below
  every release a test installs), and a scenario that copies a fixture
  tree, whose own record says the target alone, gives it that record. So
  what is installed there is what the helper's record says, whichever
  build reads it - shown where it makes a difference, on an install of
  0.1.0, which is below this build
  (`apply::an_install_the_harness_writes_is_read_as_its_records_say_and_not_as_this_build`). The
  releases a test writes out for itself are 900.1.0 installed and 900.2.0
  staged (`build_small_scenario` in `rust/crates/katana_update/tests/apply.rs`,
  `build_tiny_loop` in `rust/crates/katana_update/tests/recover.rs`) - versions no build
  of Katana has, as is the version of a release that is given none
  (`apply::the_releases_written_out_here_are_above_the_version_of_any_build`); the fixtures'
  pair and the forty-file pair are 0.9.0 and 0.10.0. Under a record that
  said the target alone each of those updates would rest on the project's
  version: an install whose record is below the program reads as the
  program's version, and the staged release is then the newer one only
  while the project has not passed it.

  The end-to-end tests stand the same way. Their driver gives both of its
  packages that record (`INSTALL_RECORD` in `tests/update/update_e2e.py`:
  0.1.0, as `INSTALL_JSON`), so the installs that `katana_update_e2e` and
  `katana_updater_test` read - programs built as the project's version -
  are what their records say, whichever build reads them.
- **What the person kept.** `UPDATE BLOCKED CLEAR` is run by `katana_cli`
  against a state file a CMake script wrote by hand - two releases blocked,
  a version skipped, the switch off, a time of the last check - and the
  file is read back with CMake's own JSON reader, not by Katana
  (`cli.update_blocked_state_is_written_with_two_releases_blocked`,
  `cli.update_blocked_clear_says_none_and_keeps_the_other_settings`,
  `cli.update_blocked_clear_left_the_state_file_with_an_empty_list`). That
  a blocked release is then offered and applied again is the verbs' own
  test, on a scratch install and the fake transport
  (`update_verbs::blocked_clear_empties_the_list_and_the_next_check_offers_the_release_again`,
  `update_verbs::after_blocked_clear_apply_confirm_returns_the_request_for_the_release_that_was_put_back`);
  and the button in Settings > Updates is pressed in the real window, where
  the command log shows that one line and the status the verbs' refusal
  for a headless run
  (`qt_settings_offers_again_a_version_that_was_put_back_by_its_line_headless`).

**Where nothing is installed.** On Linux and macOS the swap is not written,
and what is held there is the refusal: `rust/crates/katana_update/tests/platform.rs`, and
`katana_updater_refuses_for_the_platform_and_writes_nothing`, which runs the
helper with its swap line and with `--recover` and lists its folder
afterwards.

What no test covers:

- **The transport the programs ship with, against a server.** Its rules -
  where the token goes, which redirects are followed, what an answer to a
  range must be - are tested to the letter, over a recorded wire
  (`rust/crates/katana_update/tests/feed.rs`), and end to end through
  `katana_update_e2e`. But that program moves its requests over plain
  sockets of its own, because it must not be able to leave the machine. The
  half the shipped programs use instead - `katana_app::update_transport::make_update_fetch`
  (`rust/crates/katana_app/src/update_transport.rs`) over libcurl, `katana_gis::web_access::http_exchange`
  - is run against a server only by the two `OnlineLive` cases of
  `rust/crates/katana_interop/tests/online_requests/online_fetch.rs`, which need `KATANA_ONLINE_TESTS=1`
  and a real host, and which ask for no byte range; no test in the ordinary
  run puts it in front of the fake feed. A release is the first place it
  meets a `Range` answer.
- **A shipped program updating an installed copy.** The shipped programs
  cannot be given a test key, which is the point of them; so the few lines
  that only they have - the window's close-and-apply, `katana_cli` starting
  the helper - are exercised by a release and by no test.
- **The window's call that confirms an update whose helper died.** Whether a
  window may confirm is a function with a test
  (`AStartConfirmsAnUpdateOnlyInAWindowOfTheVersionThatWasInstalled`), and
  what confirming does is the engine's, tested with its seams
  (`ConfirmStarted`). The call between them - `UpdateWorkbench::start`'s
  first idle turn, its fresh read of the install record and its twelve
  askings while a helper holds the lock - is reached by neither a build
  tree nor a headless run, and so by an installed copy alone. The same is
  true of `main()` saying that the helper could not be started: which runs
  are shown a message box is tested
  (`update_decisions::an_update_that_could_not_be_started_is_boxed_only_in_a_window_nobody_drove`), that
  `main()` asks it and says the rest on stderr is not.
- **GitHub itself** (`docs/update.md`, "To verify on first release").

## Benchmarks

Speed is measured, never asserted (Rules 5 and 6, "Measure, then claim").
The C++ build's benchmarks - Google Benchmark cases in one
`katana_benchmarks` program, compared with a script that alternated the
builds - went with the C++ tree on 2026-11-15, and the Rust has none yet
(`docs/performance.md`). The practice they followed stands for whatever
replaces them, because this machine is shared with other builds and often on
battery, so one run of one binary against one run of another measures the
machine as much as the code: single runs of the same binary have differed by
up to 2x.

1. **Keep both builds**, and measure only in Release.
2. **Alternate them**: every binary in each round, reversing the order every
   round, several repetitions each; report the minimum and the median of
   every benchmark's samples in wall time.
3. **Run an A/A control beside the A/B.** Give the SAME binary twice under two
   names. The spread between the two is the noise on this machine today; an
   A/B difference inside it is no result.
4. **Report ratios and counts** - "new/base 0.62 on the median, A/A
   1.00-1.04, 18 samples each" - not a single absolute time, and name the
   machine and its state.
5. **Commit the benchmark** with the change, so the measurement can be
   repeated, and put the before and after figures in the commit message and
   the module's document.

CPU time under-counts work done by `TaskPool` threads, so it overstates a
parallel path's change (audit BLD-12): read wall time.

## The docs test

`tools/check_docs.py` runs as the `docs` test (registered only when Python 3
is found) and fails when a document in `docs/` cites something that does not
exist. The forms it accepts are listed, with how each is checked, at the top
of the script; in short:

- a repository path (`rust/crates/...`, `tests/...`, `tools/...`, `docs/...`,
  `cmake/...`, `resources/...`, `samples/...`, and the root files), in
  backticks, in a link or in fenced code; paths under a git-ignored folder
  are not checked, since a clean checkout lacks them. A path into the removed
  C++ tree (src/, include/, the benchmarks, a tests' `.cpp`) is reported
  as such;
- a Rust path rooted at a crate (`katana_cad::global_modify::match_entities`),
  followed through the crate's module files to a declaration;
- a shorter qualified name (`Document::set_current_style`,
  `global_modify::match_entities`): its last two names both occur in the code
  of one `.rs` file - outside comments and strings, so a C++ name a port's
  comment mentions does not keep a stale citation alive;
- a test: a Rust twin `<suite>::<case_name>` must be a `#[test]` function whose
  innermost module is `<suite>`; a `qt_..._headless` or `cli.` test, or any
  long snake_case name, must be registered by the CMake test runner; a long
  CamelCase name must occur in the code; a gtest name (`Suite.CaseName`) is
  reported, with the twin's form to cite instead;
- a citation of a removed document (the root plan, the root contributor
  instructions, the root readme);

and every document in `docs/` must be listed in `docs/index.md`, and
`docs/headless.md` must name every switch the window's driver parses
(`rust/crates/katana_window/src/drive/run.rs` and `steps.rs`) and every `-D`
variable `tools/check_screenshot.cmake` reads - a new switch is documented in
the commit that adds it. Documents
still being brought up to this standard are named in the script's
`NOT_YET_CHECKED` list, which shrinks as each is fixed.

One check goes further than "what is cited exists". The worked example in
`docs/customisation.md` is a file a person may copy to start their own, so
it must be exactly what the writer writes;
`customisation_format::the_worked_example_of_the_document_is_written_as_it_is_printed` proves that of the copy
it holds in a raw string, and the `docs` test fails when the document's block
is not that copy, character for character. *Rejected: having the entity test
read the document.* That suite is given no path into the source tree, and a
unit test that passes or fails by where the checkout sits is the wrong place
for a check on a document.

Run it alone with `python tools/check_docs.py` from the checkout root; it
prints each broken reference with its file and line.

## The eighteen tests that failed in the Linux cloud container

This section and the four after it are the C++ suite's record (GoogleTest,
Qt's offscreen platform): what failed, why, and what the fix taught. The
lessons that outlive the C++ are kept in the rules above.

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
- **Three Windows paths** (`script_runner::the_line_it_writes_is_the_line_it_reads`,
  `plot_drawing::the_line_it_writes_is_the_line_it_reads`,
  `snapshot::the_line_it_writes_is_the_line_it_reads`).
  The tests handed the line writer `C:\...` as a platform's file dialog would
  on Windows, and expected `/` back. On Linux QDir::fromNativeSeparators
  rightly leaves `\` alone, because POSIX allows it in a file name
  (POSIX.1-2017, 3.170). So the code was right and the input was not a native
  path there. The tests now build their input with QDir::toNativeSeparators.
  On Windows that is exactly the old backslashed path, so nothing they assert
  there is weaker. On POSIX there is no separator to convert, and that part of
  the check can only be made on Windows.
- **Three file dialogs: one always, two intermittently**
  (`sheet_view_options::an_image_views_picture_is_chosen_and_copied_in_one_step`,
  `sheet_set_menu::the_set_is_saved_and_loaded_back_asking_first`,
  `sheet_set_menu::another_sets_sheets_are_appended_in_one_step`).
  - What was wrong. The note written on 2026-09-26 guessed a native dialog.
    That was wrong: a backtrace shows Qt's own widget `QFileDialog` in
    `exec()`, with its file-system thread running. The cause is in how the
    tests answered it. QFileDialog::selectFile writes the name into the
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
  - The fix. The one answer they now share, `choose_file` in
    `rust/crates/katana_window/tests/support/window_support.rs`, takes the focus off the box before
    `selectFile`. The image test's helper also cancels a dialog that is still
    up rather than leaving it, so a choice that does not take fails the test
    instead of hanging it.
  - The regression test. qt_widgets.FileDialogAnswer.TheFileIsChosenWhenTheNameBoxAlreadyHasTheFocus
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
    `rust/crates/katana_gpu/tests/support/gpu_support.rs`, compiled into the widget tests too). A
    driver that is there and fails still fails them.
  - Found on the way. The three `qt_widgets_gpu.*` tests and
    `gpu_offscreen.*` are plain `add_test`s, and a gtest skip exits 0, so
    ctest reported a skipped case as passed. They now carry a
    `SKIP_REGULAR_EXPRESSION`.
  - For the release workflow. A Linux runner that is to exercise these
    installs `xvfb` and `mesa-vulkan-drivers` before configuring. Without them
    the cases skip, saying why, rather than fail.

What still skips in that run, and why, all for reasons outside the program:

- the tests that read the gitignored reference customisation (the headless
  quit check was disabled without it too, until it was given a committed
  fixture as its built-in: `docs/headless.md`, "The customisation a run
  starts with");
- the two `online_live` cases (`KATANA_ONLINE_TESTS=1`);
- the `gpu.*/Hardware` device cases, since the container has no GPU;
- `gdal_adapter::gdal_is_pointed_at_the_certificates_beside_libcurl`, where the
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

- **qt_widgets.FileDialogAnswer.TheFileIsChosenWhenTheNameBoxAlreadyHasTheFocus**
  compared QFileDialog::selectedFiles() with the scratch path as
  `std::filesystem` gives it. Qt returns `/` on every platform (`QDir`: "Qt
  uses '/' as a universal directory separator"), and on Windows the scratch
  path has `\`, so the same file failed to compare. It now compares against
  QDir::fromNativeSeparators of the path. What the dialog chose is unchanged.
- **`library_data` and `simd_*.core`** made their scratch prefixes under one
  fixed name. ctest runs the core suite and its `simd_*` reruns at once, so
  one process removed a folder while another held a file in it open, and the
  `filesystem_error` ended that run. The name now carries the process id.
  Evidence: `ctest -R "^LibraryData\.|^simd_.*\.core$" --parallel 8
  --repeat until-fail:10` failed in its first round before the change and
  passed all ten after it.
- **`katana_qt_widget_tests` was listed with the 5 s default discovery
  timeout**, not the 60 s every other suite has (the C++ build's KatanaTargetDefaults.cmake).
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
`info_verb::info_of_the_sample_terrain_describes_its_raster_and_band` and
`dataset_info_dialog::it_runs_the_info_lines_and_shows_the_raster_its_bands_and_gdals_json`
assert is absent (no min or max without statistics being asked for). Both
passed in that run and failed when run again while the file was there. The
headless check `tests/geo/headless/info.cmake` asks for the same file's
statistics, and the sidecar guard in `rust/crates/katana_gis/src/processing/bridge.rs`
removes what a run made only after the run ends, so a case reading the file
at the same moment can see it; which test left it behind was not found. The
file is git-ignored, so nothing shows it but the failure. Delete it before
running the suite again.

## Fixed scratch folders shared by parallel runs (2026-09-29, not fixed)

A whole-suite run at `--parallel 6`, while two other worktrees on the same
machine ran theirs, failed
`pipeline_verb::a_pipeline_ending_in_write_keeps_its_raster_as_a_reference`: "could not
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
  and `generate_rotate::the_fitted_drawing_is_turned_to_fill_the_sheet_in_one_step` typed
  its strip's corners to the micrometre and got a turn 2.6e-9 rad off. All
  86 such calls (65 in the program, 21 in tests) now use
  `katana_core::text::format_exact_real`, the shortest round-trip text: by the
  standard the same `to_chars` call C++26's `to_string` makes, so GCC's
  output is unchanged. The C++ build's check_float_to_string.py found any new one (retired with it: the Rust guard is `rust/clippy.toml`'s disallowed methods); a
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
- **`library_data::missing_data_beside_a_library_is_not_found`.** libc++'s
  `weakly_canonical` makes an empty path the working directory, which is
  absolute; libstdc++ leaves it empty. `data_beside_library_file` relied on the
  latter to refuse an empty name, and on libc++ looked beside the working
  directory. It now refuses an empty name first. The old check passed or
  failed by where the test stood, so
  `library_data::an_empty_name_is_refused_wherever_the_working_directory_is` now stands
  inside a prefix that does hold `share/gdal`: on clang it failed without the
  fix and passes with it.
- **The keyboard shortcuts on macOS** (`keyboard_shortcuts_dialog::every_key_is_listed_and_a_clash_is_marked`,
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
