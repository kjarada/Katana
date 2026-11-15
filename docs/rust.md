# The Rust port

On 2026-11-05 the owner decided that Katana moves to Rust: the whole of it,
the window included. This document is the port's record - the decisions and
their reasons, the workspace and its two toolchains, the rules every port
follows, what is ported, the measurements and what is not done. It is updated
in the commit that changes any of them.

The C++ program stays the product while the port is made, and it is the
oracle the port is checked against. The rewrite runs beside it, bottom-up by
layer, and the programs are cut over one at a time: no C++ is deleted until
the Rust that replaces it has passed its gate and shipped.

## The owner's decisions (2026-11-05)

| Question | Decision | What follows |
|---|---|---|
| What the window becomes | Rebuilt in Rust with egui and wgpu | No C++ ever calls Rust, so nothing bridges them that way. Printing becomes a PDF printed by the operating system. |
| C++ while the port runs | Rust first for new work | C++ takes fixes only. A fix to a layer already ported lands in its Rust twin in the same commit (`rust_parity` refuses it otherwise); a new feature waits for its layer and is then written in Rust alone. |
| How the work is run | At most four agents at a time, one per crate or slice | Each agent ports one crate or slice in a worktree of its own, and at most four run at once; one integration build and whole-suite run per step. The owner first ruled "parallel agents per crate", and on 2026-11-12 ruled "run 1 agent at a time": step 7's wave 2 had six running at once, the account's limit was spent in minutes and all six stopped mid-file; on 2026-11-14 the owner raised it to two ("change it to 2"), and on 2026-11-15 to four ("spawn 4 agents max"). |
| Where Rust is built | In WSL only, until the port is a fully working version; then it is tested on Windows | Building, testing and the comparison with the C++ all run in WSL. Nothing is run on Windows meanwhile - and so nothing ships, since Windows is where Katana ships first and where the Windows-only code (the updater's swap, Credential Manager, wide paths) can only be tested. The owner first ruled "WSL every day, Windows at every gate", and narrowed it the same day: "i dont want to run anything on windows at this stage just build for wsl then once we have a fully working version we test on windows". |

Rejected for the window: keeping the Qt window in C++ over a Rust engine (not
a port of the whole), Slint (weak for docking and a large custom canvas), and
Qt Quick through cxx-qt (still Qt and a C++ toolchain). Rejected for the
build: the MSVC Rust installed on the owner's machine (it cannot link the
MSYS2 builds of GDAL, PDAL and PROJ the Windows build uses), rustup's GNU
target (its MinGW runtime is msvcrt, which must not be mixed with UCRT) and
cross-compiling Windows programs from WSL (the same mismatch).

## The order of the work

Bottom-up by layer, because a layer's tests can only run once what it stands
on exists; programs cut over one at a time, each behind evidence that it
answers as the C++ does.

| Step | Crates | C++ test cases | Its gate, in WSL |
|---|---|---|---|
| 0 | the workspace, the toolchain, the checks | - | the C++ Linux suite and the cargo stage green, the GDAL spike passes |
| 1 | core, math, geometry | 398 | the cases twinned, the number formats equal the C++'s on random doubles |
| 2 | geodesy, survey, terrain, render, entity, commands, update, updater | 2,094 | the cases twinned; the updater's end-to-end tests on Linux |
| 3 | storage, archive12d, dxf, ifc, surveyio | 1,124 | every file of the test data and samples read and written by both, each reading the other's |
| 4 | cad | 3,011 | the cases twinned; every verb and alias accepted |
| 5 | gis, pointcloud, interop | 370 (io 43, interop 174, geo 153) | the cases twinned, the online tests too |
| 6 | app, with the geo verbs of `src/katana_app/geo` | 616 (app 242, geo 374) | the 375 cli tests pass against the Rust katana_cli; the replies and saved projects of both programs compared |
| 7 | window, gpu | 1,609 | every headless window test the build registers passes against the Rust window (267 in WSL on 2026-11-11) |
| 8 | Windows | - | the whole of the above on Windows, natively; the updater applies, rolls back and recovers a real update there |
| 9 | - | - | a release on the Rust programs, one release cycle, then the C++ is removed |

Nothing ships before step 8: the owner's ruling keeps the port off Windows
until it works, and Windows is where Katana ships first. The case counts are
what `tools/check_rust_parity.py --list` printed on 2026-11-05 (9,236 in all);
the step's own run is the evidence, not this table.

## Where it stands

| Step | State |
|---|---|
| 0 | done: the workspace of 23 empty crates, the WSL toolchain, `rust_layering`, `rust_parity`, the visible build's cargo stage and WSL mode, the GDAL spike |
| 1 | done in WSL: core, math and geometry, all 398 C++ cases twinned, plus core's 11 task-pool cases from the render suite; the Release build, Clippy, numeric oracle and whole suite pass |
| 2 | done in WSL: geodesy, render, entity, commands, terrain, survey and update, all seven test directories held as ported (2,108 cases, render's 11 task-pool cases among them, twinned in core), and the updater program with its test build, its stand-in for Katana and the end-to-end engine; the end-to-end tests' portable cases pass against the Rust programs on Linux. Its gate's Windows parts - a real update applied, rolled back and recovered there, and the first Rust program shipped - wait for step 8 by the owner's ruling |
| 3 | in progress, begun beside step 2 because it stands only on step 2's finished layers: storage, dxf and ifc whole and held as ported; surveyio whole; storage, dxf, ifc and surveyio held as ported; archive12d but for the legacy customisation readers (209 of 312; the other 103 wait on the owner's decision); the round-trip gate written and passing in WSL ("Step 3's round-trip gate", below) |
| 4 | done in WSL: cad whole, in five waves (F to J) - all 3,011 cases twinned but three exempt (text that is not UTF-8, which a Rust String cannot hold), `tests/cad` held as ported; the 46 aliases of the C++'s table accepted, compared entry by entry; the gate's run below |
| 5 | done in WSL but its online half: in three waves (K, L, M) - the GDAL core (the adapter, formats, reprojection, zip, raster sampling), the point-cloud engine, the online catalogue and the colour ramps, the geoprocessing bridge and the drawing bindings, web access and the online requests and discovery, and interop's core (reference data, import, export, the archive, dataset descriptions, terrain I/O, raster products, table reprojection) with online fetch and its verbs; tests/io and tests/interop held as ported; 374 of the geo suite's cases drive the app layer and moved to step 6. The cases that reach real hosts (`KATANA_ONLINE_TESTS=1`) have not been run on the port |
| 6 | done in WSL: the session, katana_cli and katana_mcp, the update, survey, IFC and DXF verbs, the MCP server, and the geo executor with every family of its verbs, in a foundation and two waves; all 616 cases twinned (app 242, geo 374; 27 of the update verbs' cases are Windows-only, written and not compiled until step 8), `tests/app` and `tests/geo` held as ported; the cli gate: all 376 cli tests pass against the Rust katana_cli and katana_mcp with replies identical to the C++'s, and it is part of the whole WSL run; the round-trip gate runs the real katana_cli. Its Windows half waits for step 8 |
| 7 | in progress: planned in a foundation and four waves of agents; the third-party crates declared, fetched and pinned at releases two weeks old; wave 0 done - the window's foundation (named controls, the drive harness, the canvas, the host, the dialog frame), katana_gpu whole on wgpu (its device cases on lavapipe in WSL) and the window gate's tools (79 cases, 15 exempt by the plan); wave 1 done - the raster and PDF canvases and their text, the drive programs and the docks' chrome, the jobs and command log, the plan painters, the icons and tool bars, and the annotation, roads, utilities and survey dialogs (440 cases); wave 2 done - the plan view and its tool host, the sheet painter and plot output, the style, layer and Global Modify managers, the survey code manager and symbol library, the GIS vector and terrain dialogs, and the surface dialogs (about 590 cases); wave 3 done - the update and settings dialogs, the 3D, elevation and section views and the GPU scene view, the annotation and road workbenches, the sheet editor with its plot, arrange, checks and Sheet Set menu, the view workspace and its Layers popup, the start screen and the customisation workbench (about 450 cases); wave 4 done - the main window, its dispatcher, menus, toolbars, panels and checks, the IFC, surface display and GIS menu parts, and the `katana` program, with the last 32 window cases (every window case twinned or exempt: tests/qt_widgets and tests/gpu held to parity); the window gate 266 of 267 after its fix-up round, 265 with the C++'s streams (the two left are Qt's own pixel metrics, under Not done) |
| 8-9 | not started |

### Step 1: the foundation crates

The port keeps the C++ header boundaries as Rust modules, with the public
types re-exported at each crate's root. Native `Result` and `Option` express
failure and absence; matrices retain their row-major storage and column-vector
composition order. The three-argument vector length uses libstdc++'s scaled
calculation, not chained two-argument hypot calls, because the rounding order
is observable. The existing AVX2 and NEON text, point and splat kernels have
Rust implementations with runtime dispatch and the scalar comparison tests.

`rust/parity.toml` now holds `core`, `math` and `geometry` to their 126, 55 and
217 cases respectively. The 11 task-pool cases remain owned by the C++ render
test directory but have their Rust twins in core, where the pool lives. No
whole case is exempted. Test values and tolerances come from the C++ tests;
additional tests exercise Rust ownership and panic cleanup. In particular,
workers retain surplus panic payloads until completion: dropping a payload
whose destructor also panics must not kill a worker and strand a waiting job.
Restoring worker-side disposal made the regression hit its ten-second
deadlock watchdog; restoring the repair made it pass and reuse the workers.
The property-test generators follow GCC 16's integer sample assembly and
redraw a fraction rounded to one, rather than clamping it. A forced
maximum-integer sample proves that branch is reached and rejects the old
clamp.

The number-format gate is `rust/crates/katana_core/tests/number_oracle.rs`,
against the test-only C++ executable built from
`tests/core/number_format_oracle.cpp`. Its 28,942 bit patterns cover both
signs of every binary exponent, boundary mantissas, half-integer rounding,
signed zero, infinities, NaNs and 16,384 deterministic random samples. The
random sequence is the xorshift sequence and seed already used by the C++
point-batch tests. Each input compares exact text, twelve significant digits,
and six fixed precisions including both sides of the precision clamp: 231,536
byte-for-byte comparisons. This is an oracle comparison, not a Rust
formatter's output saved as an expectation.

The gate found a shortest-decimal half tie: binary bits `3e60000000000000`
were written as `2.9802322387695313e-08` by Rust and
`2.9802322387695312e-08` by C++. The exact formatter now detects equal-distance
decimal candidates using integer factors of two and five, chooses the even
final digit, and checks that the adjusted text still round-trips. Fixed
integers retain every exact digit; using ordinary Rust display for every
value was rejected because it does not have this wire-format contract.

The visible build sets `KATANA_NUMBER_FORMAT_ORACLE` to the helper in the
C++ build's `bin/tests` directory. A missing helper fails the Rust test;
`wsl cargo` therefore needs a preceding full WSL build. It never silently
skips the cross-language gate. `rust/clippy.toml` and the workspace lint
reject direct `to_string` calls.
Because that method is generic, the guard deliberately rejects integer and
string calls too; integers use explicit formatting and strings use owned
copies. The cargo stage proves the rule on
`tests/rust_check_fixtures/float_to_string`, requiring the named diagnostic
and a nonzero exit. Formatting macros still require the same review as C++
streams; this guard targets direct `to_string`, as its C++ counterpart does.

Geometry explicitly allows Clippy's negated-partial-comparison lint so that
the C++ conditions continue to reject NaNs; replacing `!(value > 0)` with
`value <= 0` would change behaviour. Oracle decimal literals and test-suite
module names likewise retain their spelling with narrow, reasoned lint
exceptions. Fresh child processes test scalar, detected-native and invalid
SIMD environment requests. A kernel-parity run requires a real kernel:
scalar-against-scalar is not credited as native coverage. Unlike gtest's
runtime skip, the Rust parity gate fails on a scalar-only test host; the
library itself still supports scalar execution.

This is library work, not a program cut-over. The window, CLI and MCP still
run the C++ implementation; no Rust drawing operation is exposed yet.

Final integration evidence on 2026-11-05: Release C++ build and Rust Clippy,
build and tests in Ubuntu-24.04; the complete visible test phase exited 0.
CTest had 9,914 entries: 9,874 passed, 40 skipped, none failed. Its skips
cover unavailable reference files, hardware/display and online cases, and
permission checks unavailable to the root user. The cargo-stage aggregate
was 430 passed, zero failed and zero ignored, including child-process test
summaries. All 231,536 number-format comparisons passed. No Windows run or
program cut-over is claimed by this evidence.

### Step 2: the domain kernels

Step 2 is run in waves, split by what each module and each C++ test file
includes: an agent may build only on what is already integrated, because
under the owner's no-build rule it cannot compile its code, and code written
against another agent's unbuilt API is a guess. Each wave is merged, built
once, fixed and committed before the next begins.

| Wave | What | Cases |
|---|---|---|
| A | geodesy; render; the entity model (all but the customisation libraries); terrain's TIN core (all but the volumes); survey's core and its subsurface tools (all but the reduction); update's version, ranges, inflate, paths, credential, file operations, fault injection, registry and process seams | 902 |
| B | entity's customisation libraries; commands; terrain's overlay and volumes; survey's reduction; update's install record, feed, manifest, signature and trust | 628 |
| C | update's journal, state, plan and check; and step 3's storage, dxf, ifc, archive12d (without the legacy readers) and surveyio's core and delimited formats | 736 |
| D | update's stage, apply, recover and contract and the swap's test harness, with the tests that need nothing more; surveyio's ten instrument formats; one home in katana_math for the C++ library's semantics | 532 |
| E | update's tests of the swap, recovery, journal and fault loops on the harness, the helper's tests, and the updater program | 315 |

Wave E compiled with three Clippy findings and passed its first test run:
3,437 passed, none failed (229 new: 212 of the 315 twins - the other 103
are Windows-only, the helper's 53 among them - and 17 end-to-end and
program checks). Its decisions:

- **The helper's test build is a second program, not a cargo feature.**
  Cargo merges a feature into every binary of a build, so a feature would put
  the test key and the test options into the helper that ships.
- **All four programs - the helper, its test build, the stand-in for Katana
  and the end-to-end engine - are binaries of `katana_updater`**: cargo
  hands an integration test the programs of its own package only, and the
  end-to-end tests need the three together.
- **The end-to-end tests run the same driver** (`tests/update/update_e2e.py`,
  unchanged) with e2e.cmake's arguments, environment and timeouts, from a
  Rust test that is generated with the list of cases it hands the driver's
  registration check, so the two cannot drift.

Wave A's seven agents wrote 73,540 lines no one had compiled: 44,357 of
library and 29,099 of tests. The
first build of the merged tree type-checked all six libraries; what failed
was 18 Clippy findings, then 27 `.err().expect()` calls, two names a test
module shadowed and an untyped float in the tests. The first test run
passed: 1,302 passed, none failed (430 from step 1, and 872 new - the 902
twins less the 33 Windows-only credential cases, plus three extra tests).
Two of Clippy's suggestions were not taken because they would change
behaviour: eager construction of a tile's 4,096-byte mask for every point,
and an auto-deref that would borrow through `&mut` where the C++'s copy of
the reference is meant.

**CGAL to spade: decided for spade.** The plan's rule was spade if the
terrain tests pass unchanged, else a C++ shim of the CGAL backend. They pass
unchanged: all 185 TIN-core cases, among them the 89 TIN-edit cases. What
made it possible: CGAL's constrained Delaunay triangulation flips with a
symbolically perturbed in-circle test, so its result for a given set of
vertices and constraints is unique and independent of insertion order - the
spatial sort before insertion only makes it faster. The Rust
(`katana_terrain`'s `cdt_backend.rs`) splits crossing constraints as
`Constrained_triangulation_plus_2::insert_subconstraint` does, with CGAL's
own construction of the crossing point and its four-ulp snap; triangulates
with spade; and flips with CGAL's perturbed predicate, ported line for line,
to CGAL's unique triangulation. Not reproduced: CGAL's check that a crossing
it computed lies in a triangle beside the crossed edge, with its exact
recomputation when it does not - it can only matter for crossings within
rounding of an end. The overlay and volume cases of wave B test the kernel
further.

**Text in the model is validated UTF-8.** The entity layer holds `String`,
by the rule "bytes up to the decoder": a C++ case that stores non-UTF-8 text
in the model and refuses it later keeps its UTF-8 half as its twin, and the
readers of stored bytes - the geometry blob - still refuse such bytes. Two
cases whose only subject is that refusal are exempt (`rust/parity.toml`), as
are a moved-from transformer and a change of the floating-point rounding
mode, which Rust cannot express.

Wave B's five agents built on wave A's compiled code and compiled almost
clean: eight Clippy findings in the libraries, two in the tests and one
untyped float, then the first test run passed - 1,933 passed, none failed
(631 new: 628 twins less one Windows-only case, plus four unit tests of the
customisation parser). Its decisions:

- **The undo stack does not hold the model.** `CommandStack::new(&mut model,
  ..)`, then `execute`, `undo` and `redo` each take `&mut Model`. The C++
  stack keeps a reference to a model that must outlive it, which a Rust
  document holding both the model and its stack cannot express. A save point
  is a step's serial number, not a command's address.
- **Three JSON readers, by what each C++ reader depends on.** The entity
  serialization reads through serde_json with nlohmann's answers; the
  customisation reader is a line-for-line port of nlohmann 3.12's lexer and
  event parser (`katana_entity`'s `json/sax.rs`), because the C++ reads
  customisations as events - a member given twice, strokes read as they come,
  error 406 with its line and column, 100,000 levels deep; the update layer
  reads feeds, manifests and records through serde_json with nlohmann's
  answers on duplicate keys, `-0`, a byte-order mark and number range
  (`katana_update`'s `json.rs`), because it may depend on core alone. One
  shared reader would need a crate below both with a JSON value type of its
  own, since no third-party type may cross a crate's interface. The one
  shared piece with no third-party type - nlohmann's number text, the port
  of its Grisu2 - moves into `katana_core::text` when a second crate writes
  a real number as nlohmann does; the update layer's records hold none (their
  integers, strings and flags give serde_json's two-space text, with the keys
  sorted as `nlohmann::json` holds them). Superseded before cad began: one
  home, `katana_core::json` (below, "Step 4").
- **The update layer's build configuration** - the pinned keys and the feed's
  repository and address - is read by its `build.rs` from the same CMake
  files, with the same refusals, so both programs of one checkout carry the
  same keys.
- **Signatures**: OpenSSL's Ed25519 and the C++'s RFC 8032 checks become
  ed25519-dalek's `verify_strict` with each check made explicitly before it -
  the canonical y by bytes (dalek's decompression would reduce it), the
  point on the curve, the sign of x = 0, the neutral element, the prime-order
  subgroup and S < L - each with the C++'s refusal.

Wave C began step 3 beside step 2: its layers stand only on step 2's
finished ones, so waiting for the update layer would only have idled the
agents. Its first compile found 18 Clippy findings; the tests passed first
time - 2,660 passed, none failed (727 new: 736 twins less ten Windows-only
cases, plus one test of the record text). Its decisions:

- The update layer's records are written by serde_json's two-space printer
  with the keys sorted, which gives nlohmann's `dump(2)` text for integers,
  strings, booleans, arrays and objects (the escapes were compared in both
  sources; a test asserts the bytes). Superseded by `katana_core::json`'s
  own `dump(2)`; the same test asserts the same bytes.
- surveyio's formats registered themselves at static initialisation in C++;
  Rust has no life before main, so the program's registry is one explicit
  list in `format.rs`, built once, to which each format adds one line.
- The C++'s default name for an imported point cloud with none named the
  vendor, against the naming rule; both programs now say "archive cloud" (a
  fix, so the C++ takes it with its twin).

Wave D compiled with four Clippy findings and passed its first test run -
3,208 passed, none failed (548 new: 532 twins less seven Windows-only
cases, plus 23 tests of katana_math's new modules). Its decisions:

- **The C++ standard library's semantics have one home**: `katana_math::cpp`
  (`min`, `max`, `min3`, `max3`, `clamp`, `remainder`) and
  `katana_math::libm` (the C library's `asinh`, `erf`, `erfc`, `lgamma`,
  whose digits Rust's own functions do not reproduce), replacing private
  copies in ten crates. `clamp` is libstdc++'s body, `min(max(v, lo), hi)`,
  not the standard's Returns clause: they differ only when the bounds cross,
  and the C++ the port is held to is GCC's build. `lgamma` holds a lock
  around the C call, since it writes the global `signgam`. Kept apart:
  render's generic `min`/`max`/`clamp`, used on integers too.
- **Where the C++ catches `std::exception`** in the swap, the Rust catches a
  panic and lets the fault injector's simulated crash through, as the C++
  does (`apply::catch_unexpected`).
- **A test that subclasses a C++ file system** to break one operation becomes
  a forwarding file system that takes one closure per operation.

Wave E's integration evidence, 2026-11-08, `tools/build_visible.sh wsl` on
the merged tree: exit 0; Clippy and the float-to-text guard clean; CTest
9,914 entries, 9,874 passed, 40 skipped, none failed, with `rust_parity`
holding fourteen directories as ported; cargo test 3,437 passed, none
failed, none ignored. The Rust end-to-end cases took as long as the C++
ones (about 1.3 s each), so they ran rather than skipped.

Wave D's integration evidence, 2026-11-08, `tools/build_visible.sh wsl` on
the merged tree: exit 0; Clippy and the float-to-text guard clean; CTest
9,914 entries, 9,874 passed, 40 skipped, none failed, with `rust_parity`
holding thirteen directories as ported; cargo test 3,208 passed, none
failed, none ignored.

Wave C's integration evidence, 2026-11-07, `tools/build_visible.sh wsl` on
the merged tree, the C++ rebuilt for its one change: exit 0; Clippy and the
float-to-text guard clean; CTest 9,914 entries, 9,874 passed, 40 skipped,
none failed, with `rust_parity` holding twelve directories as ported; cargo
test 2,660 passed, none failed, none ignored.

Wave B's integration evidence, 2026-11-07, `tools/build_visible.sh wsl` on
the merged tree: exit 0; the Release C++ build, Clippy and the float-to-text
guard clean; CTest 9,914 entries, 9,874 passed, 40 skipped, none failed, with
`rust_parity` holding nine directories as ported; cargo test 1,933 passed,
none failed, none ignored.

Wave A's integration evidence, 2026-11-07, `tools/build_visible.sh wsl` on
the merged tree: exit 0; the Release C++ build, Clippy with `-D warnings`
and the float-to-text guard clean; CTest 9,914 entries, 9,874 passed, 40
skipped as before, none failed, `rust_parity` holding geodesy and render as
ported; cargo test 1,302 passed, none failed, none ignored. No Windows run is
claimed: the update layer's Windows code is written and not compiled.

### Step 3's round-trip gate

The gate is a Rust test, `rust/crates/katana_app/tests/round_trip.rs`, that
runs both programs on every file of the test data and the samples of the
formats step 3 ported, and compares what they make:

- **The C++ side** is `katana_cli`, its path passed by `tools/build_visible.sh`
  as `KATANA_CPP_CLI`, as the number oracle's is; without it the test
  fails, never skips. **The Rust side** is the Rust `katana_cli` (since
  step 6; until then `katana_corpus`, a stand-in program of `katana_app`
  for the part of cad's Document that OPEN, SAVE, IMPORT, EXPORT and
  SURVEY READ touch). Both get the same lines.
- **Both programs start alike**: the working directory and `TMPDIR` are
  the gate's scratch, and neither customisation variable is set, so each
  starts with the built-in customisation it was built with. Starting with
  none would hide what a SAVE records of the customisation, an archive's
  colours and SURVEY IMPORT's coding; a run of a note alone, before any
  row, stops the test with one sentence if the two start differently.
- **Each run pair is compared on how it ended and on what it said**: the
  last error line, then both streams word for word, each program's own
  paths written as `<project>`, `<opened>` and `<file>`.
- **Each file is read by both, written by both, and each program reads
  what the other wrote**: a project opened, migrated, saved and saved as;
  a DXF or .12da imported and saved; each writer's DXF, IFC and .12da
  compared byte for byte (the IFC header's time set aside) and re-imported
  by the other program; a survey file's SURVEY READ reply compared exactly.
- **Projects are compared by `tools/project_dump.py`**, which reads them with
  Python's sqlite3, independent of both programs: every table in key order,
  blobs as hex, the stamps that differ between two saves of the same content
  normalised (created and modified times, the application version, the
  survey jobs' import and report times, the backups' names), and the stored
  SQL's line ends normalised (a C++ built in WSL from this Windows checkout
  stores CRLF).
- **The corpus is a table**, one row per file and format, and a second test
  fails when a data file has no row, so a new file is a decision. Since
  step 6 it compares 104 of its 116 rows: projects, imports (the zipped
  archive, the GIS samples and the online-data fixtures among them), every
  survey file read and imported, scripts, lines of verbs and IFC exports.
  Left out, each with its reason in the table: the services' capabilities
  documents (GDAL's drivers would reach the hosts they name), a build
  template, and the legacy customisation formats (the owner's decision).
- **Its first run found a real difference**: the archive import named an
  alignment "Imported from" the vendor's model in C++ and the archive's in
  Rust, which had followed the naming rule. Both programs now say "archive
  model", as they now say "archive" in the import's two warnings and the
  coverage table (a fix, so the C++ took it with its twin). Until step 6
  two files kept one key aside, `reference_layers`, which the Rust session
  now records as the C++'s does.

### Step 4: the cad layer

cad (3,011 C++ cases) is planned in five waves by its include graph, so that
no agent writes against another's unbuilt code: F, the Document and the
leaf libraries; G, styles, scope, plotting, the scene and the road model; H,
every verb family but volumes, the tool framework and the annotation core;
I, the interpreter with the tool families; J, the 1,018 cases that drive the
interpreter or span two agents' work. The 45 of cad's 148 test files that
construct the interpreter can only be twinned in J. The crate's layout
rules (one module tree in `lib.rs`, a module per header path, `document.rs`
owned by one agent, one test target per agent) keep the agents of a wave
off each other's files.

Before cad: **one JSON home.** The plan had cad take a third copy of the
JSON adapter; instead `katana_core::json` translates nlohmann 3.12, and
entity and update moved onto it, so serde and serde_json left the workspace.
Following nlohmann ended the readers' last differences from the C++: `-0`
reads as the integer 0, `1e400` is nlohmann's out-of-range error, nesting
deeper than 128 is read, a NUL outside a string ends the text, and a parse
error names nlohmann's byte. No test had asserted the old answers. The
first cad wave also ported the leaf libraries (dashing, style drawing,
symbols, hatching, plot, view transform, viewport layout and the others)
and gave the civil date and UTC timestamp a home in `katana_core`.

**The Document's shape**, which every later cad agent builds on
(`katana_cad`'s `document.rs` records it at its top):

- The Document owns its model and its undo stack and hands the model to the
  stack on each call, as wave B's stack requires.
- What a command may change that is not the model - the project's metadata
  and survey jobs, the surfaces and their styles, the session's
  customisation, and the parsed caches of sheets, road templates, corridors,
  road objects and the BIM model - is one state struct. Between steps it
  lives in the Document, so its accessors return plain references as the
  C++'s do; for one execute, undo or redo it is lent to a shared slot that
  the document's own commands capture, so a command run through another
  document's stack finds the slot empty and fails (the C++'s would change
  the document that made it). Rejected: the plan's permanent shared state
  with snapshots returned from the accessors - the C++'s readers hold
  references, and a survey job holds its field file whole, which every
  change would copy; and changes queued for after the step, since a survey
  job command reads the job list while it runs.
- The C++'s single stack listener becomes the work the Document does after
  the stack returns (the index, tables, surfaces, road model, selection,
  current layer and style, revision), then its listeners are told.
- Every member of document.hpp is declared once: members the C++ implements
  in other files (sheets, road templates and corridors, road objects, the
  BIM cache, the coordinate system, survey jobs, the view's paper scale) are
  `impl Document` blocks in their own modules, over crate-private fields and
  primitives the Document provides, so no later agent edits document.rs.

**Wave G** ported the five families that stand on the Document alone:
styles, survey coding and the scope (global modify, the scope words, the
view set and its links); the plotting core (frame, sheet set, layout, page
setup, generators, sheet storage, plan grid, tables, section annotation);
the scene with its SIMD kernels and the surface display (colouring, plan,
section cut); the road model (templates, sections, corridors, road objects,
the BIM model); and the customisation edits, survey tools and drawing
pieces. Its 709 cases compiled after five Clippy findings and one test
that caught a mistaken fix, and passed on their first run. Decisions:

- **The view provider of the scope words is a borrow**,
  `Option<&dyn Fn(Option<u32>) -> Result<ScopeView>>`, `None` being headless,
  where `VIEW` is refused in favour of `AREA`. Rejected: an
  `Arc<dyn Fn + Send + Sync>` as the C++'s `std::function` would map, because
  the interpreter calls it only during one verb, and a borrow lets the
  window lend a closure over its own view set without making that set
  shareable or thread-safe.
- **The view set keeps its views boxed**, against Clippy's `vec_box`:
  `take()` hands a closing widget the very object the set held, as the
  C++'s `unique_ptr` does, and a C++ case asserts that identity; held by
  value, the view moved and the case failed.
- **A command a step plans for its own document refuses another's link**:
  linework's styling step, which the C++ guards by comparing model
  addresses, checks that its document's state is lent to it, and otherwise
  fails with "linework was planned for another document" - the same refusal
  from outside. The state link does not nest, so a step that styles lines
  must not be run from inside another step's use of it.
- **The sheet set's storage case holds its non-finite half**:
  `SheetStorage.TextThatIsNotUtf8OrANumberThatIsNotFiniteCannotBeStored`'s
  twin asserts the non-finite number's refusal, by step 2's rule for text
  that is not UTF-8, as wave F's four surface-style cases do.

**Wave H** ported the verb families and the tool framework that stand on
waves F and G: survey jobs (survey points, the survey finish, the linework
verbs, the job commands); the code table, the customisation host with its
built-ins, the customisation, survey code and road verbs, the alignment
report and parcels; the annotation core (leaders, label layout, automatic
labels, annotation for export, leader presets), the tool framework, grips,
vertex editing and the surface verbs; the plotting layer above the core (key
plan, arrangement, legend, preflight) and the sheet verbs; the terrain and
road-object verbs with the drawing's shapes and the BIM view; and the view
verbs, the property outline, the utilities and the style manager's rows. Its
542 cases compiled after fifteen Clippy findings, none of whose fixes changes
behaviour, and passed on their first run. The verbs whose cases drive the
interpreter are ported whole, so that wave J writes tests only. Decisions:

- **A tool keeps no reference; its host lends it a context for each call.**
  The C++ tools hold `const Document*` across steps while the host executes
  each step's command between inputs, which Rust's borrows cannot express. A
  Rust tool is `'static`, owned as `Box<dyn InteractiveTool>`, made once from
  a `ToolStart` (attributes, selection, handles) whose parts it copies; every
  call, `prompt` and `expects` included, takes a `ToolContext` - a `Copy`
  bundle of the document, the pick tolerance, the view's pick and apertures
  and its layer overrides - that the host builds for the call and drops before
  executing the step's command. Typed input with drafting takes the host's
  own copy of the drafting settings, written back after. `as_any_mut`
  replaces the C++'s `dynamic_cast`. Rejected: one struct of the C++'s fields
  rebuilt for each call (it copies the selection on every mouse move and says
  nothing of which fields a later call may trust); a shared, interior-mutable
  Document (shareable for one front end's sake, every execute behind a
  run-time borrow); and a router holding the Document mutably while it lends
  the tool an immutable one.
- **The tool families are one list that starts empty.** The catalogue is
  built from a constant list of family adders, and each of wave I's tool
  agents adds its own line, in the C++'s order.
- **A verb's front-end hooks take the document at each call.** The sheet
  verbs' context closures take `&Document` first, since the verb holds the
  document mutably and a closure cannot keep it as the C++'s lambdas do. The
  view verbs' host takes `&self` and keeps its view set in a `RefCell`,
  because the scope's view provider reads that same set while ZOOM VIEW runs.
- **A survey job's command plans each step inside the document's state link
  and runs it after.** The link does not nest, so nothing is planned while it
  is held twice. The reduction's context is a job-owned copy, because
  survey's `ReductionContext` holds functions that are not `Send` and a
  command must be; the colour lookup the finish hands to linework is a
  snapshot of the colour table at the call, where the C++ asks the document
  each time.
- **The built-in customisations are embedded by a build script**, which reads
  the default and the alternates from `src/katana_cad/CMakeLists.txt` as the
  update layer's reads its own; the cases that set the customisation's
  environment variables are a test binary of their own behind one lock,
  since `set_var` is unsafe in edition 2024 and races parallel tests.

**Wave I** ported the command interpreter with the annotation, drawing and
volume verbs, and the thirteen tool families: lines, curves and the
professional drawing tools, the transforms, the edits (trim, extend, offset,
fillet, chamfer, break, join, explode), divide and measure, inquiry,
lengthen, match properties, selection, the vertex tools, every annotation
tool and the terrain edits. Its 497 twinned cases compiled after five Clippy
findings and passed on their first run; wave H's tool-framework cases,
written against an empty family list, pass against the full catalogue.
Decisions:

- **The interpreter does not hold the Document**: it is
  `run(&mut self, &mut Document, line)`. Holding the Document as the C++
  does would lock it away for the interpreter's life, so that the window
  could not paint it nor a test read it between lines; an
  `Rc<RefCell<Document>>` makes every reader a run-time borrow that can
  panic. The interpreter keeps its own history and last point and the front
  end's hooks - the sheet verbs' context, the scope's view provider, the view
  host and the customisation host - all owned, so it is single-threaded, as
  the C++ window is, and a front end that replaces a hook sets it again,
  where the C++ asked its providers at each line.
- **A tool's remembered settings are per thread**: the edit, lengthen, match
  properties, select similar and terrain edit choices, the vertex defaults,
  and the drawing heights, double-line width and cloud arc length, which the
  C++ keeps per catalogue or per process. The test harness runs each case on
  a thread of its own, so each starts at the initial values, as each gtest
  case did in its own process; two catalogues on one thread share them.
- **A tool base class with virtual steps becomes a tool generic over a trait
  of steps** (the transforms' selection tool), lent the chosen entities at
  each call, as every tool is lent its context.
- **A tool's text prompt takes text**, so the C++'s refusals of input that is
  not UTF-8 (Text, a dimension's text, a leader's note, a balloon's number, a
  label's text) cannot be reached.
  `AnnotateAlignedDimension.TextThatIsNotUtf8IsRefusedAtTheTextPrompt`, whose
  only subject is that refusal, has no twin, and is an exemption to make when
  cad is held as ported.

**Wave J** twinned the 1,018 cases that drive the interpreter or span two
agents' work - six agents writing tests only, against code no case had run
before. 1,016 passed after one fix to the program: CUSTOMISE KEEP, and the
customisation host's start, asked whether the kept file was there with
`Path::try_exists`, which errs for a path that runs through a file where
libstdc++'s `exists` answers false, so a kept file whose folder could not be
made was taken to be there and refused as unreadable (InvalidState) where
the C++ refuses the write (FileExportFailure). `katana_core::path_text::path_exists`
now answers as libstdc++ does, the one home for that rule in cad. The other
two cases are exempt (`rust/parity.toml`), as are
`AnnotateAlignedDimension.TextThatIsNotUtf8IsRefusedAtTheTextPrompt` from
wave I: each one's only subject is a byte that is not UTF-8 in text a
`String` holds - a tool's typed text, a leader report's names, the status's
layer name. Where such a case has a UTF-8 half, the half is twinned.

The gate's second half, every verb and alias accepted: the interpreter's
alias table was compared with the C++'s entry by entry (46 entries,
identical and in the same order), and its dispatch arms follow the C++'s
order; the interpreter's cases (`test_cad` and the families' verb files)
pass.

Evidence, 2026-11-10, `tools/build_visible.sh wsl` after wave J: exit 0; the
Release C++ build, Clippy and the float-to-text guard clean; CTest 9,915
entries, 9,875 passed, 40 skipped, none failed - `rust_parity` among them
with `tests/cad` held as ported, 3,011 of 3,011; cargo test 6,488 passed,
none failed, none ignored (wave I's run gave 5,472, H's 4,975, G's 4,433,
F's 3,724), the round-trip gate among them over 73 files of the corpus.
The parity tool: 6,538 of the C++ suite's 9,236 cases twinned or exempt.

### Step 5: gis, pointcloud and interop

A planning pass split step 5 by dependency into three waves: K, the GDAL
core and the point clouds; L, the geoprocessing bridge and web access; M,
interop's core and online fetch. It also split the geo suite by case:
374 of its 527 cases drive the app layer - `src/katana_app/geo`, its
executor, verbs, records and MCP tools - and are twinned with that layer in
step 6, so step 5 twins tests/io (43), tests/interop (174) and the 153 geo
cases of the bridge, the bindings and what they stand on. Rejected: leaving
all of tests/geo to step 6, which would leave the bridge with no twin for a
step, and porting the app's geo verbs in step 5, which are the app's layer.

**Wave K** ported the GDAL core - the adapter with its registry and error
handling, formats, reprojection, zip containers and raster sampling - over
167 declarations of GDAL's C API, and the point-cloud engine over its PDAL
shim, the online catalogue and the colour ramps, with 27 twins of the
terrain and cad code the geo suite reaches. Its 93 cases passed on their
first run. Decisions:

- **A GDAL dataset owns its handle**: it moves between threads but is not
  shared, closing it is its drop, and nothing ever destroys GDAL's driver
  manager (a case pins it). An error collector is a guard that pops GDAL's
  handler when dropped, its handler catching every panic.
- **Text from GDAL and PDAL is decoded lossily** at the boundary, as
  PROJ's is; names handed to GDAL go as their own bytes.
- **PDAL is called without a lock**: it guards its own registries, each
  call owns its stages, and the C++ runs it from job threads unlocked. The
  shim's C ABI returns a code and, on a throw, the exception's message.
- **PROJ's search path has one home**, pointcloud's, which gis may see;
  GDAL's registry calls it where the C++ does, so one once-flag points both
  at PROJ's data.

Evidence, 2026-11-10, `tools/build_visible.sh wsl` after wave K: exit 0;
the Release C++ build, Clippy and the float-to-text guard clean, and the
C checks of every GDAL declaration compiled; CTest 9,915 entries, 9,875
passed, 40 skipped, none failed; cargo test 6,581 passed, none failed, none
ignored (6,488 before).

**Wave L** ported the geoprocessing bridge - GDAL's algorithm framework
through its C API, the policy that classes each algorithm, staging, the run
and its outputs - with the drawing bindings (a drawing as a dataset,
results back as entities, contours), and web access through GDAL's HTTP and
the libcurl GDAL loads, with the online request builders and service
discovery. Its 143 cases passed on their first build and run; the GDAL
spike test is retired. Decisions:

- **The algorithm framework through C, with a shim of thirteen calls.**
  Disassembly of libgdal 3.13.2 showed that the C API's sub-algorithm and
  argument lookups ask for suggestions where the C++ bridge does not, so
  an unknown name would raise a failure the C++ never meets; the shim makes
  those lookups as the C++ does, and gives the calls the C API lacks
  (aliases, hidden algorithms, suggestions, completion, validation,
  bounds, datasets by name). Each catches every exception and reports it
  as a GDAL error.
- **GDAL's HTTP option setter is C++ only** (its symbol is mangled), so a
  one-call shim gives it a C name. Rejected: declaring the mangled name,
  which no compiler would check, and through which a C++ exception could
  unwind into Rust.
- **A run's stop and progress** are an `Option<&AtomicBool>` and a progress
  closure that is `Sync`, because GDAL calls progress from its own threads;
  a panic in it stops GDAL's run and is raised again when the run returns.
- **libcurl's variadic calls** are declared as variadics and reached only
  through typed wrappers, each option constant carrying the C type libcurl
  reads.
- **A text fetcher is a borrow** used during the call, and parsed XML is
  plain owned data in the terms of GDAL's parser, which the C++ uses rather
  than core's XML reader.

Evidence, 2026-11-10, `tools/build_visible.sh wsl` after wave L: exit 0;
the Release C++ build, Clippy and the float-to-text guard clean, the C
checks of the algorithm and HTTP declarations compiled; CTest 9,915
entries, 9,874 passed, 40 skipped, and the docs check failed on the retired
spike test, which this section no longer cites (rerun on the final docs: it
passed); cargo test 6,723 passed, none failed, none ignored (6,581 before,
less the retired spike).

**Wave M** ported interop's core - reference data, the import of vector,
raster and point-cloud files with the placement advice, export, the
archive's import and export, dataset descriptions, terrain I/O, the
derived raster products and table reprojection - and online fetch with
its verbs: 132 cases, which passed once a derive the compiler refused was
dropped (a struct holding a `Vec2` cannot be `Eq`). Decisions:

- **Reference data is a plain value**, cloned and compared, changed by no
  command, as the C++'s is: the session owns it beside its drawing and
  lends it to the geo executor with the drawing for each line (step 6).
- **The online transport is a shared, thread-safe closure in an owned
  environment** built for each import, so a fetch moves to a job thread
  with everything it needs; its progress is a borrowed `Sync` closure,
  because GDAL's warp reports from its own threads, and its stop an
  `Option<&AtomicBool>`, as in wave L.
- **Four shown texts that named a vendor say "archive" in both programs**,
  as the naming rule asks: the archive kind's name, the refusal to describe
  an archive, a reference layer's refusal of an archive without a point
  cloud, and the archive's refusal of a file not named `.12da` or
  `.12daz`. None is stored, read back, or asserted by a test or a cli
  script but the one C++ fixture changed with it, so each C++ line
  changed in the same commit as its twin.
- **The cases that reach real hosts** run only with
  `KATANA_ONLINE_TESTS=1`, as every such case in Katana does, which
  `tools/build_visible.sh` now passes into WSL. They have not been run on
  the port: one run, on 2026-11-10, ended during its configure, before any
  test. They run at the owner's word, and step 5's gate waits on them.

Evidence, 2026-11-11, `tools/build_visible.sh wsl` after wave M, merged onto
step 6's wave 1: exit 0; the Release C++ build, Clippy and the
float-to-text guard clean; CTest 9,919 entries, 9,879 passed,
40 skipped, none failed; cargo test 7,016 passed, none failed,
none ignored; `rust/parity.toml` holds io and interop as ported.

### Step 6: the app layer

Step 6 ports the headless session that katana_cli and katana_mcp share,
with the geo verbs of `src/katana_app/geo` - 374 geo cases whose verbs are
the app's (step 5) - for 616 cases in all. It began beside step 5's last
wave, so its wave 1 stands only on built code: where the C++ calls interop's
reference data, which that wave was writing, the geo frame calls one module
of marked seams that wave 2 fills. Its foundation, built first because three
agents needed it, is the console and the import records. Decisions:

- **A session is lent a console for each call.** The C++ writes a line's
  reply and refusals to `std::cout` and `std::cerr`, and katana_mcp swaps
  the buffers around each line; Rust has no buffer to swap and runs a
  binary's tests on parallel threads, so `Session::run` writes through a
  `Console` its caller lends it - the process's streams in katana_cli, a
  capture in katana_mcp and the tests. Rejected: a session owning a boxed
  console, `run` returning its text (a download's progress would print
  late), a process-wide capture (it races parallel tests).
- **The session owns its drawing and its interpreter** and takes a line as
  bytes. A panic out of a verb is reported as `error: Internal: ...` and the
  line fails, as the C++ catches every exception, so katana_cli's unsaved
  drawing survives. Off Windows a line that is not UTF-8 is refused at the
  session's door, since the interpreter reads text; on Windows it is read
  as Windows-1252, as the C++ does.
- **The geo executor's context is a bundle of borrows built for each
  phase** - the drawing, the interpreter, the scratch folder, the front
  end's hooks - and its work and apply are `Send`, so the window can run
  the work on a job thread (step 7). Rejected: a long-lived context in the
  session, which would be self-referential, and a shared drawing behind a
  run-time lock.
- **The update rig's feed is a trait, not a closure**, because the context
  it gives borrows the rig's feed, store and launcher; the running
  program's credential stores are made once per thread, where the C++'s are
  process-wide statics, because they are not `Sync`.
- **The cli gate reruns the C++ build's own cli tests against the Rust
  programs** (`tools/rust_cli_gate.py`, the `wsl gate` phase): it reads the
  build tree's CTest files, puts the Rust katana_cli and katana_mcp in place
  of the C++ ones, reruns the C++ ones beside them, and compares the two
  runs' replies, normalising only the programs' paths, a run's seconds and
  the per-process scratch folder, and comparing the two streams apart
  (the C++'s output is buffered on ctest's pipe and its errors are not).
  Rejected: a CMake variable at some 330 call sites, a Rust copy of the
  cli tests, CTest's test launcher, and pass and fail alone, which two
  programs can share with different words.

Evidence, 2026-11-11, `tools/build_visible.sh wsl` after wave 1: exit 0; the
Release C++ build, Clippy and the float-to-text guard clean; CTest 9,919
entries (the gate's four checks new), 9,879 passed, 40 skipped, none
failed; cargo test 6,884 passed, none failed, none ignored. The first
`wsl gate`: 376 cli tests read and rewritten, 270 passed (248 of the 354
that run a program, and the 22 fixture steps); 257 replies the same as
the C++'s. The failures are wave 2's verbs and katana_mcp, but for the
HELP texts, whose geo families wave 2 adds, and one survey reply whose
two numbers differ in the last digit ("Not done").

**Wave 2** ported the MCP server and katana_mcp, and the geo verbs' every
family - the vector verbs; the reference layers, SURFACE, REFS, INFO and
CONTOUR; IMPORT, EXPORT, DRAPE, RASTER SAMPLE and ZONAL; the raster
analysis verbs - in six agents, three of them after wave M, and moved the
round-trip gate onto the real katana_cli. A prelude, built first, gave the
geo context and the session interop's reference data. The agents' code
compiled at the first build but for one temporary borrowed past its
statement; Clippy had six findings. Decisions:

- **The MCP server is attached to nothing.** `Server::handle` marks the
  session it is lent as katana_mcp's at the head of every message, so
  there is no attach call to forget. A tool is a row of a plain function
  over the session and its arguments; a JSON error out of a tool is its
  refusal, `error: <what>`, as the C++ says it, and a panic is the tool's
  error inside a call and JSON-RPC's -32603 inside a method.
- **The case that nothing leaks onto the real streams runs itself as a
  child** of its own test program, with a variable set on the child
  alone, and reads the child's streams between two markers: a test
  thread cannot own the process's streams.
- **The geo frame's seams are gone.** Wave 1's eight stand-ins for
  interop's reference data are the C++'s code again at each site, and no
  signature changed; the session records the reference layers on SAVE,
  lets them go on NEW and OPEN, and restores them on an OPEN of a
  project that records them.
- **The round-trip gate drives the real katana_cli** ("Step 3's
  round-trip gate"), and `katana_corpus`, which stood in for it, is
  deleted.
- **Two differences the gates found were fixed at their home, not at the
  gate.** Core's generic path form copied a path's bytes, where
  libstdc++'s rebuilds the path from its elements and so writes a run of
  separators as one (`/vsizip//root/...` typed, `/vsizip/root/...`
  replied). The survey solver added its sums in order, where the C++
  build's Eigen 3.4.0 adds them in SSE2's two-lane patterns: it now adds
  them in Eigen's order, read from the installed headers ("Third-party
  libraries"), and both resections in the gates give the C++'s digits.
- **The cli gate is part of the whole WSL run**, after both suites, and a
  reply that differs fails the run as a failed test does.

Evidence, 2026-11-11, `tools/build_visible.sh wsl` after wave 2: exit 0; the
Release C++ build, Clippy and the float-to-text guard clean; CTest
9,919 entries, 9,879 passed, 40 skipped, none failed; cargo
test 7,456 passed, none failed, none ignored, the round-trip gate's
104 compared files among them; the cli gate: 376 cli tests, none
failed, every reply the same as the C++'s. `rust/parity.toml` holds app and
geo as ported.

### Step 7: the window

Step 7 ports the desktop window, `src/katana_qt` (121,563 lines of C++ and
516 of GLSL), and its GPU layer to `katana_window` and `katana_gpu`, in egui
and wgpu: 1,609 cases (tests/qt_widgets 1,548, tests/gpu 61). A planning
pass split it into a foundation and four waves, because one chain cannot be
cut by interfaces: the plan painter, the plan view, the workspace that holds
the views and the main window that holds the workspace. Its gate is every
headless window test the build registers, 267 in WSL: the 285 of this
file's first table could not be reproduced (the commit that wrote it had
268). Decisions so far:

- **egui 0.36.2, not 0.37**: egui_dock, the docks the window needs, has no
  release for 0.37 yet. The move later is one commit for the whole window.
- **Every crate is at least two weeks old when it is locked.** Cargo picks
  each transitive crate's newest compatible release, which can be a day
  old; the prelude dated every new lock entry from crates.io and pinned the
  24 younger than 2026-10-28 back to their newest older release, all at
  once (the wasm-bindgen family pins itself to exact versions, so one at a
  time cannot move it). Every cargo of `tools/build_visible.sh` is
  `--locked`, so no build takes a newer crate on its own. Rejected:
  `cargo vendor` into git, some 420 crates' sources in the repository.
- **No test opens a window.** WSLg sets `DISPLAY` in every WSL process, so
  a real window would open on the owner's screen: egui runs headless, fed
  by the drive harness, and only the `katana` program names eframe (a
  foundation case fails when another file does).

**Wave 0** built what every later agent builds on, in three agents:

- **Every control is driven by its C++ object name.** The foundation's
  controls register each control under its name in a target (a dialog, a
  dock, a panel, the window), and a test or the headless runner posts an
  act - fill, press, scroll to - to that name in a mailbox. The first
  control of that name drawn in a frame takes it, applies it before it
  draws, and reports the change after; an act no control took is refused
  in the C++'s sentence for a missing widget, or kept while its target is
  still being sized, or refused because the target has closed. A name drawn
  twice in one target is refused: a driver could not tell the two apart.
  The main window is the target with no name, as its C++ object has none.
- **A run of the harness never ends on the frame that took an act.** Such
  a frame drew the controls before the taker with the state from before
  the act, where Qt shows a click's result everywhere at once; one more
  frame follows (found by FilterBar's exclusive chips at the first run).
- **A dialog declares its footer** and is fitted to the screen every frame,
  where the C++ read a Qt dialog's laid-out widgets to find the footer.
  The eleven ScreenFit cases about reading and wrapping Qt's layouts are
  exempt, as the plan foresaw.
- **The painters draw through one `Canvas` trait**, made from a census of
  the C++ painters' QPainter calls (in its module's header), so the screen,
  PNG, TIFF and PDF are one painter's backends.
- **The GPU layer's shaders are WGSL**, written from the C++'s GLSL by
  hand and validated by naga. wgpu has no geometry stage, so lines and
  points are instanced only; there are no serialized shaders and one
  compiler, so four GPU cases are exempt. Each device case runs on a
  Hardware device, then a Software one, skipping only when the device is
  not there: in WSL the Hardware cases skip and every Software case runs on
  lavapipe, with no X server needed. The default backend on Windows is
  Direct3D 12, where the C++ used Direct3D 11.
- **The window gate** (`tools/rust_window_gate.py`, the `wsl wgate`
  phase) runs every headless window test against the Rust `katana` beside
  a rerun of the C++ window. It compares both programs' streams after
  listed normalisations, and their exit codes, since a test that passes on
  any failure would not tell an exit 1 from an exit 2. It keeps both
  screenshots side by side for a person to look at and never compares their
  pixels. It shares one CTest reader (`tools/ctest_tree.py`) with the cli
  gate, whose output did not change. It joins the whole run when the Rust
  `katana` exists, at the last wave.
- **The headless window has the C++'s geometry.** Tests pin pointer
  coordinates and toolbar device pixels, so the Rust window's headless runs
  must lay out as the C++'s do. The probe (`tools/window_geometry.py`) ran
  the C++ window headless on nine variants and measured it from its own
  screenshots: at scale 1 the window is 1360 x 860, the Layers dock
  45,159 300 x 295, Reference Data 45,459 300 x 328, Properties 1015,159
  300 x 628, the command line 350,637 660 x 150, and the views 350,159
  660 x 473 under a 26-pixel title strip, two views splitting it 327 | 328.
  At 125 % the window is 1700 x 1075 and the docks' sizes scale; their
  places there are still to measure, as the probe could not locate them.

Evidence, 2026-11-11, `tools/build_visible.sh wsl` after wave 0: exit 0; the
Release C++ build, Clippy (`--locked`) and the float-to-text guard clean;
CTest 9,923 entries (the window gate's four checks new), 9,883 passed, 40
skipped, none failed; cargo test 7,565 passed, none failed, none ignored
(6,884 before step 7); the cli gate 376 of 376 identical. The GPU cases,
run once with their output shown, skipped only on the Hardware device.

**Wave 1** ported, in nine agents, what stands on the foundation alone:
the drive programs and the docks' chrome, the raster and PDF canvases and
the text both draw, the host's jobs and command log, the plan painters, the
icons and tool bars, and the annotation, roads, utilities and survey
dialogs. Decisions:

- **The raster canvas covers an antialiased shape by the exact area of each
  pixel inside it** (`canvas::coverage`), as Qt's raster engine does, and
  tiny-skia only blends. tiny-skia's own antialiasing samples a pixel on a
  4 x 4 grid, so every edge lands on a quarter of a pixel: a pen 1.3 px
  wide inked as 1.25, and the twin comparing a mesh footprint's outline at
  two resolutions read 3.33 where Qt's exact coverage reads
  2 x 1.3 / 0.65 = 4.0. Rejected: rasterising at four times and averaging
  (edges to a sixteenth of a pixel, still not the area), and widening the
  twin's band, which is the C++ test's. A stroke is outlined by tiny-skia's
  stroker and the outline covered exactly; one a pixel wide or thinner
  stays tiny-skia's hairline. The coverage is worked out 64 rows at a time,
  so a fill across a plotted A1 sheet holds 5 MB of it, not 560.
- **Each medium has one canvas.** The icons' rasteriser, the painters'
  raster backend and the plot's PDF page writer are the raster and PDF
  canvases (`icons::CanvasRasteriser`, `style_painter::CanvasRasterBackend`,
  `plan_painter::CanvasPdfWriter`), and the survey report's PDF is written
  through the PDF canvas, where it had an interim writer of its own.
- **A PDF's text is real text**: glyph runs from the same shaping as the
  raster, embedded as subset fonts that map back to Unicode, so a plot's
  text can be searched and copied. Rejected: krilla's own text drawing (one
  font, no fallback, its own advances) and glyph outlines (not text).
- **A font is found by the families asked for, in order**, then a fixed
  pitch face for a fixed pitch font, then the window's embedded sans, so a
  missing font falls back to the same face on every machine.
- **Arial and Helvetica stand in as Liberation Sans where the machine lacks
  them** (the owner's choice, 2026-11-15). Liberation Sans 2.1.5 (SIL OFL 1.1)
  is embedded in four styles (`rust/crates/katana_window/fonts/`), and its
  every glyph is as wide as Arial's. Found by wave 2's integration: the
  C++ asks for Arial, and where Arial is missing Qt takes fontconfig's
  substitute (DejaVu Sans in WSL), about as wide; the Rust took egui's thinner
  UI face, so text set for Arial wrapped into fewer, tighter lines and
  `NotesAndALegendDrawTheirText` found two lines of notes where it asks for
  three. Where Arial is installed (Windows) it is still Arial. Rejected: the
  machine's own substitute, as Qt does (text would differ from machine to
  machine, which the rule above avoids), and keeping egui's face (text
  measured narrower than the C++'s everywhere Arial is missing). The four
  faces add 1.6 MB to the window program.
- **A job runs on a thread of its own**, its result handed back down a
  channel at the start of a frame, and frames keep coming every 10 ms while
  one runs. Dropping the job runner stops and joins every job.
- **The geo verbs' effects on the window are queued** and applied after each
  phase of a frame: egui cannot ask a question in the middle of applying a
  result. Headless, the far-apart question is answered Keep, as the C++
  answers it; in the window the line is cancelled, the question shown, and
  the line run again on the answer.
- **A logged line is masked in one place**, `command_log` (the online data
  keys, the update tokens), which the online data and update dialogs call.
- **An icon is painted once** for each name, device size, mode and state,
  kept as a texture and drawn on whole device pixels. An icon is named by
  the C++ enumerator (`Import`), a catalogue tool by its id (`draw.circle`).
- **A dialog follows the document at its next look**: what a Qt listener
  did at once is done at the next frame or method call, and a document
  watcher delivers at the end of each frame's drawing.
- **A page or tab that is not in front is drawn but not shown**, so a
  driver can fill a control on it, as it can on a hidden Qt page; a control
  the C++ hid is drawn invisible, and pressing it is refused as hidden.
- **A test reads a PDF written readable**: `PdfDocument::uncompressed`
  writes page contents as they are and dictionaries spaced, so an image
  reads `/Subtype /Image` as in QPdfWriter's files (krilla writes
  `/Subtype/Image` unless asked).
- **A C++ test's probe was half a pixel out, fixed in both languages.** In
  `EntersPlaceIsAnOutlinedRingNotTheNewVertexsFilledDisc`, `inkNear` names a
  pixel by its top-left corner, so the probe at 2.5 px out on each axis
  took a pixel 45.3 % covered by the ring's pen: the area of the pixel
  [3,4] x [3,4] outside radius 5 is 1 - (integral from 3 to 4 of
  sqrt(25 - x^2) dx - 3) = 0.45257, over the 0.4456 at which `inkOf` counts
  ink. Qt passed only because its curve offsets put the edge 0.073 px
  further out. The probe at 2 px out is the one the comment means.

Wave 1 twinned 440 cases, and the plan painter's agent left its 20
preview cases (the name picker and the style preview) to wave 2's styles
agent, as the plan allowed. The first run of its tests failed 8 cases;
each was fixed in the code, none by changing an expected value, and the
one C++ test changed is the ring probe above, by its probe point.

Evidence, 2026-11-12, `tools/build_visible.sh wsl` after wave 1: exit 0; the
Release C++ build, Clippy (`--locked`) and the float-to-text guard clean;
CTest 9,923 entries, 9,883 passed, 40 skipped, none failed; cargo test
8,094 passed, none failed, none ignored (7,565 after wave 0); the cli gate
376 of 376 identical.

**Wave 2** ported, in eight agents run at most two at a time (the owner's
rule from 2026-11-12, when six at once spent the account's limit and all
stopped mid-file), what stands on wave 1: the plan view (`viewport_widget`,
its context menu, grips, the Vertices panel and the tool host); the sheet
painter, legends, grids, tables and plot output; the name picker, style
preview, Global Modify, hatch patterns and the style and layer managers; the
survey code manager, definition editor and symbol library; the GIS vector
dialogs, import, export and online data; the terrain analysis dialogs, GDAL
toolbox and pipeline builder; and the Surfaces panel, surface editing,
surface styles and volumes dialogs. Decisions:

- **The plan view owns no window state.** It is one struct drawn into the
  rect it is given, with events in (`mouse_press`, `wheel`, `key_press` ...)
  and `take_events()` out, and its hooks set as flags; the workspace (wave 3)
  implements `ViewAccess` over its views. A double click is read from raw
  presses (0.4 s, 5 px), as Qt delivers it at the second press, not egui's
  at the release; a wheel line is one notch.
- **A dialog asks for the next frame when it works out a change after
  drawing.** The code manager, definition editor and symbol library react
  to their events after the frame, so what an event changed (a scope ticked
  by a setter, Revert enabled) is drawn a frame later; each now asks egui for
  that frame, as wave 1's leader manager does. Found at integration, where
  the harness settled before the frame that showed it.
- **Arial and Helvetica stand in as Liberation Sans** where the machine lacks
  them (above, under wave 1's font rule).
- **A name the C++ gives two controls is split**, as the registry refuses a
  name drawn twice: the vector import's file layer list is
  `vectorImportFileLayers` (the C++ names it and the scope's list
  `vectorImportLayers` alike), and the DEM tools' Reproject cell is
  `demReprojectTargetCell` (the C++'s `demReprojectCell` is also the tab's
  picker's surface cell). No test or script used either name. The code
  manager's `filterText`, both the search field and the Text chip in the
  C++, is one control taking a fill as the search and a press as the chip.
- **A C++ test's press of a disabled button is made only when the button is
  enabled**, where the C++ pressed it as tidy-up and Qt's `click()` did
  nothing; the harness refuses such a press. Twins that asserted on the
  press itself assert the button disabled instead.
- **Printing writes the sheets as a PDF**, a page a sheet, and hands it to
  the operating system's print queue (`hand_to_print_queue` in
  `plotting/plot_output.rs`), which returns Unsupported for now ("Not done").

Wave 2 was written by agents that did not build, in eight worktrees, and
merged into `rust-port`. Its first compile found one error (a missing `mut`)
and 22 Clippy findings, and its first run 11 failures from five causes, each
fixed in the code and none by changing an expected value: the duplicate DEM
name (7 cases), the late frame (2), the font (1), and a property panel test
that egui's delay arithmetic made fail under load (1). egui takes a
predicted frame off a delayed repaint, so a filter pause with less than a
frame left asked for an immediate one, and the harness, whose frames take no
real time, spun until the real clock caught up and ran out of frames; the
panel now asks a frame later, so the repaint is never early.

Evidence, 2026-11-15, `tools/build_visible.sh wsl` after wave 2: in the
commit that merged it; the window's twins 1,124 of 1,609 (`tests/qt_widgets`
1,075 of 1,548, `tests/gpu` 49 of 61).

**Wave 3** ported, in four agents run at most two at a time, what stands on
wave 2: the update and settings dialogs; the 3D and elevation views, the GPU
scene view and the section view, with the annotation and road workbenches;
the sheet editor, its plot dialog, arrange, checks, Sheet Set menu and
generate; and the view workspace, the views' Layers popup, the start screen
and the customisation workbench, which joined the code manager to the real
name picker and style preview in place of wave 2's seams. Decisions:

- **The workspace lays its docks out itself**, with egui_dock's `DockState`
  and the split arithmetic of its tree, not `DockArea`'s tab strips, and
  lays a view out as it opens. Qt queued that layout, so a script's
  `VIEWS OPEN plan` then `ZOOM IN` framed the view at a width the layout
  then changed, and the C++ workspace does the queued layout before it
  zooms; here the case cannot arise, and the twin of
  `ViewLinks.AZoomStraightAfterAViewOpensIsFramedAtTheWidthTheViewIsSeenAt`
  asserts that no view's width changed after it opened, where the C++
  asserts that its sweep reached one that did. A 3D view that has framed
  nothing yet takes its size from that layout too (`take_layout_size`), so
  its first zoom frames the scene at the size it is seen at; found at
  integration, where it framed at its default 640 by 480.
- **A ZOOM line run while the drawing is lent** goes to a probe interpreter
  over a scratch copy of the views, which records the zoom requests; the
  workspace applies them and replays the line. Zoom to Selection is queued
  as the line `ZOOM SELECTION view=<id>`.
- **The sheet editor edits as the C++ does**: its canvas and panels call
  cad's undoable functions on the Document, and named lines run as lines; it
  never waits on a modal question, and a plot runs inside the frame under
  the progress dialog's Cancel. Its unnamed C++ controls are given `sheet*`
  names; a wheel notch is 50 points.
- **A number box has Qt's special value text**
  (`SpinRange::with_special_text`): the plan grid's spacing reads "Auto" at
  0, and its record carries what `QAbstractSpinBox::text()` would. A fill of
  "Auto" is refused, as `--fill` reads a number.
- **A hidden button is pressed through its dialog** (the update dialog's
  `click`), as egui draws a hidden control disabled and the harness refuses
  a press of one.

Wave 3 was written by agents that did not build, in four worktrees, and
merged into `rust-port`. Its first compile found 6 errors (an
`Option<&mut dyn PlanViewHost>` that could not be lent twice in a loop, and
a missing `mut`) and 29 Clippy findings, and its first run 5 failures, each
fixed in the code or in the twin and none by changing an expected value:
the grid's "Auto" (above); the 3D view framed at its default size (above);
the sweep (above); a twin that asked `--report` for a combo box, which it
does not report, where the C++ reads the box's current item; and a twin
whose painter had no raster back end, so no symbol was blitted from a
sprite, where the C++ painter always reaches Qt's. Six sheet editor twins
wrote their pictures into the crate when `KATANA_SHEET_PNG` was set empty;
they now write only to a folder named.

Evidence, 2026-11-15, `tools/build_visible.sh wsl` after wave 3: in the commit that merged it, the C++ suite 9,923 of 9,923 with this record in place (9,922 before it, the one failure the docs check finding wave 2's note on the code manager's seams after the seams had gone), cargo test 9,144 passed and 0 failed, the cli gate 379 of 379; the window's twins 1,563 of 1,609 (`tests/qt_widgets` 1,506 of 1,548, `tests/gpu` 57 of 61).

**Wave 4** ported the main window and the `katana` program in one agent
(K), beside one (S) that twinned the 18 window cases the earlier waves
left. K's window is two halves, as the plan's borrow split has it: the
executor (the drawing, the interpreter, the reference data, the
workbenches the dispatcher runs, the log and the jobs; it is the `Host`)
and the UI state (menus, toolbars, panels, the views' workspace). The
dispatcher keeps the C++'s order; `rust/crates/katana_window/src/bin/katana/main.rs` is the only file
that names eframe and hands the window to wave 1's program driver. Decisions:

- **The window's settings keep the C++'s QSettings names** (`window/layout`,
  `appearance/textSize`, `start/showAtStart`, `scripts/recent`,
  `files/projectFolder` and the rest); the dock layout is the window's own
  JSON, version 2, and a version 1 layout (an older build's, with Surface
  Edit and BIM on the second toolbar row) is carried over.
- **The panels are laid out to the C++'s headless geometry** (the probe's
  numbers, above), not by egui_dock: Minimise and Close hide a panel, which
  cannot float or be resized yet.
- **A question the C++ waited on is asked without waiting**: a text or
  choice in a small dialog named `inputDialog`, Yes/No/Cancel in the
  system's box (rfd); a headless run is never asked.
- **A dialog's minimum height** (`DialogFrame::set_minimum_height`,
  QWidget::setMinimumHeight) pads its body, so the screen-fit twins make a
  dialog taller than an 800 x 600 screen as the C++ does, where K's first
  twins shrank the screen and the frame's rule for a footer over half the
  room then put the buttons in the scroll.
- **The window's own target is opened once a frame.** The drive registry
  opens it at every frame's start; the window beginning it to give it its
  title takes that record. The gate's first run failed 244 of 267 tests on
  "the target  is drawn twice in one frame"; a foundation test now holds it.
- **`tests/qt_widgets` and `tests/gpu` are held to parity**, every case
  twinned or exempt: 1,548 and 61. Four exemptions were added: the three
  GeneralTexts cases, which read the Qt file dialogs a manager opens (the
  Rust opens the system's own, which no test may open), and
  FileDialogAnswer's case, which tests the C++ test helper that answers a
  QFileDialog.

Wave 4 compiled with one type error and 15 Clippy findings, and its first
run failed 5 twins: four screen-fit twins (the minimum height, above, and a
scroll to a record of a region rather than a control) and the kind
switcher's arrow at 125 %, measured from a crop that took the pixels the
button shares with the bar (now only its own, as QWidget::grab).

Evidence, 2026-11-15, `tools/build_visible.sh wsl` after wave 4: the C++
suite 9,923 of 9,923, the cli gate 379 of 379, cargo test 9,168 passed and
3 failed - the two intermittent twins under Not done and one screen-fit
twin since fixed (8 of 8 after); the parity check every window case. The
window gate (`wsl wgate`), first sound run: 267 tests, 236 passed, 198 with
the C++'s streams, 69 differing; the fix-up round is grouping the 31
failures and 69 differences by area.

The fix-up round's main-window share (composition and logging) found:

- **The window had no log.** The C++'s MainWindow gives its Document a
  logger at Warning with a standard-error sink, so a command the stack
  refuses is a `WARNING [command] rejected` line in every headless run; the
  Rust window now does the same. Its timestamp is the moment of the run, so
  the gate needs a normalisation for it, as it needs one for a frame's
  milliseconds (`FrameStatsLabel: Plan N drawn X ms (kept, Y ms)`), whose
  "kept" part says only whether the toolkit painted the view once more.
- **A submenu's status tip is on its row** (`drive::Menu::tip`), where the
  frozen row had none and `--report` of a menu showed a submenu as its bare
  title. `signs` stays the one source; the rows take its tips once
  composed. Rejected: a second table of tips beside `signs`.
- **Every saved layout was refused**: the reader took the version with
  `as_integer`, and a positive number is unsigned to the JSON reader, as to
  nlohmann. Restoring an older build's layout now works as decided above.
- **File > Quit has the platform's key** (QKeySequence::Quit): none in Qt's
  Windows keyboard scheme, which is also the offscreen platform's, so none
  on Windows or in a headless run; Ctrl+Q elsewhere. The Properties tree's
  Copy (Ctrl+C) is listed in Keyboard Shortcuts as no menu's, after the
  menus' keys, as findChildren met it.
- **What a view raised is delivered after every call**, not only at the
  frame's end: the C++'s hooks ran at once, and a `--report` of the command
  log after a typed point missed the DIST tool's answer.
- **A tool its title bar hides is still the bar's button, hidden**: a press
  of it is refused as hidden, as main.cpp refuses it.
- **Not done**: the toolbars' natural widths along the top
  (`--check-toolbar-fit`) are 2 to 22 px off the C++'s, which takes Qt's
  toolbar metrics (handle, item spacing, the widgets' size hints) to match;
  and at 125 % the split buttons' arrow is 10 device pixels wide where
  Fusion's, drawn into a cached pixmap and scaled, is 12.
- **Undo and Redo drop their history lists down** (`editUndoMenu`,
  `editRedoMenu`), made from the history whenever they are asked for; the
  buttons were plain text buttons without them.

The fix-up round ran four agents (the owner raised the limit to four
mid-round): the main window and the driver (FX1), the dialogs and managers
(FX2), the window's composition and logging (FX3, above), and the plan
view's tools, the 3D view, annotation and online data (FX4). FX1's and
FX2's findings are under Not done, "Found in the window gate's fix-up
round". FX4 found:

- **A click's answer is logged before the pointer step's record**: the
  views' events waited for the next frame, where the C++'s signals logged
  them during the click; eight tool tests differed only in that order.
- **A 3D view framed before its first paint takes its size first**
  (`zoom_extents_on` sizes the camera), where it fitted the aspect as
  square: the window frames a view a surface import opened before any
  paint. Worked by hand from the camera's framing: 435.4 at 328 by 447,
  321.4 square, the C++'s 435.32.
- **The Properties panel follows the drawing**, as the C++ window's
  document listener refreshed it; nothing showed the selection before.

The integration found three more:

- **A control drawn hidden in a title bar takes no room in it.** FX3 drew
  the bar's hidden tools in a `scope_builder`, which advances the bar's
  cursor past its child: a zero-sized child at the bar's corner put the
  dock's contents under the corner, and a 3D view took 598 points where
  the bar leaves 574. They are drawn in a `new_child`, which allocates
  nothing (the twin of `ViewLinks.AThreeDViewNotYetPaintedFramesTheSceneBeforeItZooms`
  found it).
- **The sheet editor is drawn as a window of its own** while it is shown,
  over the screen in an area of its own: it was drawn in the room the main
  window's layout left, below the screen, and the two tests that grab its
  toolbar wrote nothing. A grab of a target that took no room now says so,
  with the rectangle it recorded.
- **The gate normalises two times** (its normalisation (c)): a log line's
  UTC time, and the status bar's frame time with its "(kept, ...)",
  which differ between two runs of the C++ itself.

Evidence, 2026-11-15, after the fix-up round: cargo test 9,172 passed and
1 failed (the maximise twin under Not done; 9,173 of 9,173 in the run
before); the window gate 267 tests, 266 passed, 265 with the C++'s
streams - left, the two pixel metrics under Not done.

## The workspace

`rust/Cargo.toml` is one Cargo workspace, with one crate per layer of
`tools/check_layering.cmake` in `rust/crates/`, named `katana_<layer>`. Two
names differ: the qt layer's crate is `katana_window` (the window is not Qt
in the port), and the helper program `katana_updater` is a crate of the update
layer, as `src/katana_updater` is.

A crate's `katana_*` dependencies are exactly the layers the table allows it,
all of them, declared now. A crate cannot name a crate it does not depend on,
so the layering rule holds by construction; and with every allowed dependency
declared up front, an agent porting one crate never edits another's
dependency list. Unused path dependencies cost nothing.
`tools/check_rust_layering.py` (the `rust_layering` test) keeps the crates and
the table in step while both exist: every layer has a crate, no crate depends
on a layer the table does not allow it, and no crate re-exports a third-party
crate (`pub use`), which would put a third-party type in its public
interface - the table's second rule. Dev-dependencies are not checked, as
`tests/` is not in the C++ check.

Edition 2024; `rust-version` 1.98, the older of the two compilers. `unsafe`
is denied in the workspace's lints and allowed only where a module must be
unsafe - an FFI adapter, an OS call, a SIMD kernel, or the one module that
hands the task pool's tasks disjoint parts of one buffer
(`katana_render`'s `disjoint.rs`: tiles of a framebuffer have no safe slice
form, and every step rests on the pool's contract that each index runs once
and no range overlaps another) - each saying why. Clippy
runs with `-D warnings`, as `-Werror` does for the C++.

## The toolchains

### WSL, where the port is built

The distribution is **Ubuntu-24.04**, the release job's Linux runner. WSL
3.0.3 on the owner's machine had no distribution until 2026-11-05; it now has
two: `Ubuntu-24.04`, installed for the port, and `Ubuntu` (26.04.1, the
default), which the port does not use. The distribution has no Linux user:
it was provisioned as root, non-interactively, and the builds run as root. A
user can be added at any time and becomes the default; the toolchain prefix
under `/opt` is readable by every user.

Set up, as root (`wsl -d Ubuntu-24.04 -u root`):

```sh
apt-get install -y --no-install-recommends python3-venv libegl1 libgl1 libopengl0 \
    libfontconfig1 xvfb mesa-vulkan-drivers libxkbcommon-x11-0 libxcb-cursor0 \
    libxcb-icccm4 libxcb-keysyms1 libxcb-shape0 libxcb-xinerama0 libxcb-randr0 \
    libxcb-render-util0 libxcb-image0          # what the Linux release job installs
python3 -m venv /opt/katana-venv              # Ubuntu 24.04's Python is externally managed
cd /mnt/d/<checkout> && /opt/katana-venv/bin/python tools/setup_linux_toolchain.py
cd /opt/katana-toolchain && mkdir -p bin-tools && for t in cmake ctest cpack ninja \
    cargo rustc rustdoc cargo-clippy clippy-driver rustfmt cargo-fmt; do
    ln -sf ../bin/$t bin-tools/$t; done
```

The compiler comes from the toolchain's own conda-forge snapshot: `rust` is
one of the specs of `tools/setup_linux_toolchain.py`, so WSL, the Linux and
macOS release jobs and a cloud session - whose egress reaches conda-forge
and not the Rust project's own servers - build with the same Rust, as they
build with the same GCC. On 2026-11-05 the snapshot gave rustc 1.98.1 with
cargo, clippy and rustfmt, beside GCC 16.2.0 and GDAL 3.13.2: 291 packages,
5.9 GB in `/opt/katana-toolchain`. Only the build tools are on `PATH`, linked
into `bin-tools/`: the prefix's `bin/` also holds a python3 that would shadow
the system's (the release job does the same for CMake's tools).

The sources stay in the worktree on `D:`, where git and the IDE work. Git is
never run inside WSL: a worktree made by Windows git names its repository by
a `D:/` path that Linux cannot open, and nothing in the build calls git. The
builds are on WSL's own disk, in `~/katana-build/<worktree>/` - the C++ in
`linux-release/`, cargo's target in `cargo/` - because a build on the
Windows drive, through `/mnt/d`, is slow.

Fixture tests that need POSIX modes must recreate their input in a native
temporary directory too. The first full step-1 run passed the Rust stage
but failed `update_tools_make_manifest`: Git's non-executable fixture files
appeared executable on the Windows mount. Its existing temporary-tree helper
now supplies the same bytes with native modes; the manifest expectations and
producer are unchanged (`docs/update.md`, "The pieces").

cargo links with the prefix's GCC and its sysroot (glibc 2.28), as CMake
builds with them, and writes the prefix's `lib/` into each program's run
path, so a test finds GDAL without `LD_LIBRARY_PATH`. `tools/build_visible.sh`
sets both; a crate that links a C library names the prefix as a search path
from `KATANA_TOOLCHAIN` (as `rust/crates/katana_gis/build.rs` does).

The build in WSL has a `PATH` of its own: the prefix's build tools, then the
system's directories, and nothing of Windows. WSL appends the Windows `PATH`
to Linux's, and CMake's `find_package` searches the prefixes of `PATH`; the
first configure there, on 2026-11-05, took MSYS2's Windows CURL package
through `/mnt/c/msys64/ucrt64` and failed on its missing Brotli. A configure
that has failed so keeps the Windows path in its cache (`CURL_DIR`), so the
cure includes deleting that build directory, as was done.

### Windows, once the port works

MSYS2's `mingw-w64-ucrt-x86_64-rust`, **1.99.0-1**, built against LLVM 22.
The newest build of the same version, 1.99.0-2, was made against LLVM 23 and
does not start on the owner's toolchain, whose `llvm-libs` is 22 - and
`llvm-libs` is also what MSYS2's Arrow, which GDAL links, was built against,
so moving it alone would leave GDAL unable to load Arrow. A partial upgrade of
MSYS2 was rejected; the next whole `pacman -Syu` moves Rust and LLVM together.
The package came from an MSYS2 mirror (`repo.msys2.org` reset the
connection); its signature, by an MSYS2 development key, was checked against
pacman's keyring with gpg before it was installed with `pacman -U`.

It targets `x86_64-pc-windows-gnu` against UCRT: a program it builds imports
only `api-ms-win-crt-*` and system DLLs, never msvcrt. Its cargo is the first
on `PATH` in a build window, because `/c/msys64/ucrt64/bin` is.

It was installed, and the empty workspace's cargo stage with the GDAL spike
ran green in it once (2026-11-05), before the owner's ruling kept the port
off Windows; nothing has been run on Windows since, and nothing will be until
the port is a fully working version (step 8). The Windows ARM64 release job
will take `mingw-w64-clang-aarch64-rust` from its CLANGARM64 environment, as
it takes clang ("Not done", below).

## Building and testing

Every build goes through `tools/build_visible.sh`, in a window the owner
watches, and the Rust is part of each run. Until step 8 every run is a `wsl`
one:

```sh
bash tools/build_visible.sh wsl        # linux-release, its suite under xvfb-run, clippy, cargo build and test
bash tools/build_visible.sh wsl cargo  # the Rust workspace alone
bash tools/build_visible.sh wsl test -R Docs   # ctest's arguments, as on Windows
```

The same phases without `wsl` are the Windows run, for step 8.

After the C++ build come clippy and the workspace's build with warnings as
errors; after ctest, `cargo test`, whose counts the log sums into one line
(`== cargo test: N passed, M failed, K ignored`). A narrow run - `build` with
targets, `test` with ctest's arguments - leaves the Rust out, as it leaves the
rest of the C++ out. A WSL run's window, log and status file are on the
Windows side, in `build/wsl/`, and are waited on and read as any other.

## Tests are the specification

Every C++ test case of a layer is ported with the layer, and its expected
values are copied, not recomputed: a ported test that fails means the port is
wrong, until shown otherwise from a source outside both programs (the
cardinal rule of testing). The twin of the gtest case `Suite.CaseName` is a
Rust test function `case_name` in a module `suite` - the names in snake case,
the module usually a `mod` block in a file under the crate's `tests/`. Test
data stays where the C++ reads it, under `tests/`, and is read from there.

`tools/check_rust_parity.py` (the `rust_parity` test) holds this. A test
directory listed as ported in `rust/parity.toml` must have a twin for every
case, a case added later by a fix in C++ included, unless the file exempts the
case with its reason (a misuse that the types now refuse to compile, a reader
the owner chose not to port); an exemption naming a case that does not exist
fails too. `--list` prints each directory's count. The Rust is read rather
than built, stepping over comments, string, raw string and character literals,
so that a brace inside a test's text cannot move a test into another module.

Both checks are shown to see what they claim, on fixtures in
`tests/rust_check_fixtures/` that break each rule: a crate depending on a
layer it may not, one re-exporting `rusqlite`, one that is no layer, a layer
with no crate, a case with no twin, a stale exemption. One fixture must pass:
`rust_parity_skips_literals`, whose braces are all inside literals and
comments. It was checked on 2026-11-05 to fail with each of the four kinds of
literal not stepped over.

## Porting rules

- **Behaviour is ported as it is.** A port that also fixes or redesigns loses
  its oracle. A defect found while porting goes under "Not done" here and is
  fixed after the gate, in both programs, or in Rust alone after the cut-over.
- **Errors.** One `Error` in `katana_core` with the C++'s code, message and
  context and the same `describe()` text; `Result<T>` is
  `std::result::Result<T, Error>` and a status is `Result<()>`. The C++'s
  `BadResultAccess` has no custom twin. Its success and structured-failure
  cases retain their ordinary assertions; misuse is governed by Rust's
  native result API rather than a second result wrapper or a case exemption.
- **Text and numbers.** Bytes up to the decoder, validated UTF-8 after it.
  Each of the reply's number formats must give the C++'s text exactly:
  `core::formatExactReal` (the shortest round-trip text, as `std::to_chars`
  writes it - `1e+20`, exact digits for large integers), `core::formatFixed`
  and the twelve significant digits of LIST and INFO. `parseFiniteDouble`
  refuses an underflow such as `1e-999`, where Rust's parse returns zero.
- **Numerics.** Rust never fuses a multiply and an add, as
  `-ffp-contract=off` keeps the C++ from doing; three-argument `hypot` is
  ported as libstdc++'s (scaled by the largest component); output never
  depends on a hash map's order; `math::tolerance` stays the one source of
  tolerances.
- **Parallelism.** The task pool is ported with its contract - nested calls
  run inline, the first panic is rethrown, the result is the serial run's -
  and with the tests that prove it with one thread. rayon was rejected: its
  nesting and panics behave otherwise than the contract the tests hold.
- **SIMD.** The 23 kernel entries are ported with their layers, bit-identical
  to their scalar references, with `KATANA_SIMD` kept. `#[target_feature]`
  confines the AVX2 code to the kernel itself, so the object check the C++
  needs (`tools/check_simd_kernels.cmake`) has no twin.
- **Names.** The naming rule holds in the port: comments, identifiers and
  messages say "the archive format", "survey code file", "style library";
  the kept tokens keep their spelling (`.12da`, the `katana_archive12d`
  module, the persisted `12d.*` keys).

## Third-party libraries

Each stays confined to one crate, as each is to one module in C++, and is the
version the C++ build links, from the same toolchain.

| C++ | Rust | Crate |
|---|---|---|
| nlohmann_json | `katana_core::json`: a translation of nlohmann 3.12 itself - its lexer and parser, serializer, Grisu2 number text and value rules - with no JSON crate | core, used by entity, cad, app, update, interop |
| SQLite | hand-written declarations of the 28 functions used, on the same libsqlite3, every pragma kept | storage |
| Eigen (rank-revealing QR, Cholesky) | nalgebra for the dense storage and the Cholesky factor; the column-pivoted QR is the port's own, Eigen 3.4's algorithm step for step | survey |
| PROJ | hand-written declarations of the 57 functions used, on the same libproj | geodesy |
| CGAL (one 385-line file) | spade, if the terrain tests pass unchanged; otherwise that file stays as a C++ shim | terrain |
| GDAL, including its algorithm framework | hand-written declarations of the C functions used, each checked by the C compiler against GDAL's headers; a C++ shim of thirteen calls where the C algorithm API has gaps or differs | gis, pointcloud |
| PDAL (C++ only) | a thin C++ shim of its primitives behind a C ABI, the adapter's logic in Rust | pointcloud |
| libcurl, the handle GDAL configures | hand-written declarations on the libcurl GDAL loads; a one-call shim for GDAL's C++-only option setter | gis |
| OpenSSL (Ed25519 verification with the RFC 8032 checks) | ed25519-dalek and curve25519-dalek | update |
| zlib (raw inflate) | miniz_oxide | update |
| Qt | egui, eframe and egui_extras 0.36.2, egui_dock 0.21.1, wgpu 30.0.1, tiny-skia 0.12.0, cosmic-text 0.19.0, krilla 0.8.2, png 0.18.1, image 0.25.10, arboard 3.7.0, rfd 0.17.2, pollster 1.0.1 | window, gpu |

A crate is added to `THIRD_PARTY_NOTICES.md` in the commit that first uses it,
and must be GPL-3.0-compatible.

Seven rows above differ from the plan of 2026-11-05, each for a reason found
in porting:

- **GDAL, PDAL and libcurl** (step 5) are reached as PROJ and SQLite are:
  declarations of exactly what the C++ calls, on the libraries the build
  links. GDAL's are checked by a C file compiled with the crate
  (`rust/crates/katana_gis/native/ffi_check.c`): a `_Generic` test of each
  declared function's type, written in the header's own typedefs, and a
  static assert of every size, offset and constant copied, so that a
  declaration GDAL's headers contradict fails the build. PDAL has no C
  API, so a thin C++ shim
  (`rust/crates/katana_pointcloud/native/pdal_shim.cpp`) puts its
  primitives behind one, catching every exception, and every decision of
  the C++ engine is Rust. The `cc` crate compiles both from the crates'
  build scripts, with the toolchain's own compilers, so the C++ ABI is that
  of the PDAL and GDAL linked. Rejected: gdal-sys, a binding of all of GDAL
  made for a release of its own, whose bindgen route needs libclang, which
  the toolchain lacks; the gdal crate, which puts its own dataset and error
  semantics between the port and GDAL, as rusqlite would have; the curl
  crate, which builds a libcurl of its own when it finds none on
  pkg-config's path, so a handle of one copy would be handed to a GDAL
  linked against the other; and a CMake-built shim, which would make
  cargo depend on a C++ build directory.

- **SQLite** is reached through the C++'s own wrapper (`sqlite_database.cpp`)
  ported as it is over declarations of the functions it calls. Rejected:
  rusqlite, which would put its own statement cache, transaction and error
  semantics between the store and SQLite, where the port must keep every
  pragma, statement and error path the C++ has.

- **PROJ** is reached as GDAL is in the spike, through declarations of the
  functions the C++ calls and nothing else, with every `#[repr(C)]` layout
  checked by size and offset against PROJ 9.8's headers. Rejected: the
  proj-sys crate, a binding of the whole API made for a PROJ release of its
  own, where 57 declarations checked against the headers of the PROJ the
  build links pin exactly what is used.
- **Eigen's QR**: nalgebra's `ColPivQR` pivots on the column holding the
  largest entry, not the largest norm, and has no rank or threshold, so a
  rank-deficient network would be found so elsewhere than Eigen finds it.
  The port implements `ColPivHouseholderQR` as Eigen 3.4 does it (`lsq_core.rs`):
  norms and the first-largest pivot, the LAWN 176 norm downdate, the rank
  against the prescribed threshold, the solve. Its sums are added in the
  order the C++ build's Eigen 3.4.0 adds them (SSE2, no fused
  multiply-add): a vector's reduction in two packet accumulators, the
  Householder products in the row-major matrix-vector kernel's two lanes
  or as a dot product for one column, the leverage reduced a pair of rows
  at a time. The order depends on the lengths alone, never on where the
  data lies, so the results are the C++'s bit for bit; until step 6 they
  were sequential and differed in the last bit or two beyond three
  equations. Eigen's blocked Q (48 reflectors or more) and its
  triangular solves' register panels are still applied one at a time.
- **nlohmann/json is translated, not replaced.** serde_json was tried
  first: it writes numbers with ryu (`1e20`, `1`) where nlohmann writes
  `1e+20` and `1.0`, keeps one object order for the whole build once
  Cargo unifies `preserve_order`, and reads `-0`, `1e400`, deep nesting and
  duplicate keys otherwise than nlohmann. Each crate grew an adapter of its
  own to give nlohmann's answers, three in all by step 3, and a shared one
  could not expose serde_json's types, as no third-party type may cross a
  crate's interface. So
  `katana_core::json` translates nlohmann itself, beside core's XML reader:
  its value with both object orders (`Json` sorted as `nlohmann::json`,
  `OrderedJson` as `ordered_json`), its lexer and event parser, its
  serializer and Grisu2, its accessors and their error texts. The
  translation keeps nlohmann's copyright and MIT notice
  (THIRD_PARTY_NOTICES.md).

## The GDAL algorithm spike

The C++ runs GDAL's algorithm framework through its C++ API
(`src/katana_io/geo/processing.cpp`), which no Rust binding wraps; it was
the one part of the port with no known route. GDAL 3.13 exposes the same
framework in C (`gdalalgorithm_c.h`). A test of the gis crate ran `gdal
raster info` on the geo suite's `tests/geo/data/plane.asc` through that
API - the registry, an algorithm from its path, the arguments parsed, the
run, the output read back - and checked the size it reported against the
grid's own header (40 columns, 30 rows), in every cargo stage from step 0.
Step 5 took that route ("Third-party libraries"), and the spike retired in
wave L, when the bridge's twins came to hold it; the policy table they
pin classifies every algorithm GDAL lists.

## Measurements

| What | Where | Result |
|---|---|---|
| The toolchain prefix, solved and installed | WSL, 2026-11-05 | 291 packages, 5.9 GB |
| The empty workspace: clippy, build, test | WSL, 2026-11-05 | 11 s for the whole cargo stage; 14 s with the GDAL spike |
| The same with the GDAL spike, its first build | Windows, 2026-11-05, before the ruling | 147 s |

## Not done

- **No release job builds the Rust yet.** Nothing in `rust/` ships before
  step 8, and a change to `.github/workflows/release.yml` can only be proven
  by a release run; the cargo steps are added then, when that run is part of
  the gate anyway. The ARM64 job's Rust (CLANGARM64) and the macOS job's (the
  conda-forge snapshot) come with them.
- **Nothing of the port has been tested on Windows** beyond the one cargo
  stage of the empty workspace: the owner's ruling (above). Step 8 is where
  the Windows-only code - the updater's swap, Credential Manager, wide paths -
  is first proven.
- **ARM NEON execution waits for an ARM host.** The current WSL gate runs
  x86-64 and proves AVX2/scalar equivalence; NEON source has not been compiled
  or run in this gate.
- **Oracle edge cases are retained, not repaired during the port.** Static
  review found XML accepting trailing content and surrogate/NUL character
  references, logger fields permitting raw newlines, task-pool range
  arithmetic wrapping near the largest index, and spatial cell-count
  multiplication wrapping. Degenerate closed paths can retain too few
  vertices; extreme splat radii overflow C++ signed arithmetic. Ratio
  reduction has undefined zero/zero and minimum-integer inputs in C++,
  where the Rust port panics. These need separately justified fixes after
  the gate, in both implementations. The alignment prose also contradicts
  the implementation's acceptance of negative radius magnitudes.
- **The WSL distribution has no user of the owner's.** The builds run as root
  there; nothing depends on it.
- **Defects found in porting step 2, ported as they are** (fix after the gate,
  in both programs):
  - geodesy: `proj_info()` and `proj_context_create()` run unguarded in
    `projRuntimeInfo` and `ProjContext::create`'s fallback, though PROJ's
    default context is filled lazily and copied without its lock (the Rust
    serialises both behind one mutex, since its tests share a process);
    `CoordinateReferenceSystem::operator==` is not transitive.
  - render: the clipper's fixed 3+5-vertex buffers assume each plane adds at
    most one vertex, which a near-degenerate polygon rounded to float may not
    keep (an out-of-bounds write in C++, a panic in Rust; not shown to
    happen); `DrawLine::depthBias`'s comment says NDC units where the
    rasteriser applies pixel footprints of view distance.
  - entity: `rewriteLayerPrefix` with an empty `from` drops the name's first
    byte; `labelPiecesFor` and `labelAnchor` on an open polyline with no
    vertices wrap `size() - 1` and divide by zero, and `resolveAnchor` calls
    `front()` on an empty leader (undefined in C++; unreachable for a
    validated entity); `formatDms`/`formatDm` and nlohmann's integer reads
    convert an out-of-range double unchecked; `insidePoint` sorts crossings
    that may hold a NaN.
  - survey: `toString(SurveyTimestamp)` pads a "0" before negative seconds
    under 10; `observationKindName` calls a global GNSS position with
    neither part set "geodetic"; `millimetres` in the quality levels calls
    `std::llround` on a NaN uncertainty (glibc gives the minimum integer, which
    the Rust reproduces; the Windows runtime may not).
  - update: `removeTree` resets the depth for the entry it deferred, so a
    deep tree planted under the journal's name gets another 64 levels;
    `writeAll` loops for ever on a `WriteFile` that succeeds having written
    nothing; `spawn_win`'s `run` leaks the attribute list when
    `UpdateProcThreadAttribute` fails; `makeDirectories` on Windows counts an
    entry that vanished after `ERROR_ALREADY_EXISTS` as made by another
    process; `absoluteNormal` may strip a trailing dot or space before
    `endsInDotOrSpace` sees it (suspected, to be checked on Windows).
  - commands: a hatch pattern's update is still refused as MOD-09 found it -
    `validate()` answers CommandRejected where the table's own check answers
    InvalidArgument (linetypes were fixed to ask the table; hatch patterns
    were not); `ChangeSetCommand::execute` dereferences a lookup unchecked
    (validation keeps it unreachable).
  - terrain: `gridVolume` casts its column and row counts to an integer
    unguarded, undefined for an infinite count; `SegmentProfiler::along`
    sorts with ties in no specified order.
  - survey reduction: the report's `fixed()` copies a 64-byte buffer whole
    for a value too long for it, and `dms()` casts unguarded (both
    unreachable for survey values); `seedGnss`, `convertGnssVectors` and
    `findTraverse` dereference an optional that can be empty (undefined in
    C++; the Rust skips the observation or ends the search); a face pair with
    no distance takes its target height from the left face's zenith only; a
    later observation of a pointing overwrites the shot's face.
  - customisation: nlohmann's lexer ends the input at a NUL, so `{...}\0junk`
    is read; `Members::finish` counts the members found, so a name asked for
    twice could hide an unknown member (nothing asks twice).
- **Where the Rust necessarily differs from the C++** (step 2):
  - Text the model holds is validated UTF-8 (`String`), by the rule "bytes
    up to the decoder": the C++ cases that store non-UTF-8 text in a model
    and refuse it later have their UTF-8 halves twinned, and the readers of
    stored bytes (the geometry blob) still refuse such bytes.
  - Non-UTF-8 bytes in a string PROJ returns become U+FFFD; the C++ keeps
    the bytes.
  - The raw inflate is miniz_oxide's: a stream that is not deflate is
    refused with "inflate error: <reason>" where zlib gave "zlib error <n>:
    <msg>", and zlib's two internal failures have no counterpart.
  - `std::format` pads report columns by display width, Rust by characters:
    the subsurface reports differ for an id holding wide or combining
    characters.
  - Windows-only code of the update layer (the Credential Manager, the long
    path forms, the Windows file operations and process launch) is written,
    not compiled, until step 8; so is every NEON kernel.
  - A record's I/O failure in the update layer gives Rust's error text; OpenSSL's initialisation and allocation failures have
    no counterpart.
- **Decisions waiting on the owner** (step 3):
  - archive12d's legacy customisation readers and writers
    (`src/katana_archive12d/legacy/`, the retired style library and survey
    code file formats, and `katana_customisation_convert`): ported, or
    exempt? Their 103 cases are neither twinned nor exempt, so
    `tests/archive12d` is not yet held as ported.
  - A project whose text is not UTF-8: the C++ loads the bytes; the Rust
    refuses to load it, naming the table and column. Decoding such text as
    Windows-1252, as cad already repairs surface names, would keep such
    projects openable. To be settled before storage is cut over.
  - The naming rule in the archive layer: the fallback layer name, which is
    the vendor's name and which a C++ test asserts; four C++ test names
    holding that name (copied into their Rust twins by the parity rule; the
    fourth, a zip-container case found in step 5, spells it out); and
    the source an archive import gives a surface, the vendor's name before
    "archive <file>" (written by the C++ app and interop layers, and by
    the Rust IMPORT as they write it, since it is saved with the project).
- **Decisions waiting on the owner** (step 7), none needed before step 8:
  - The window's fonts: egui's embedded UI fonts (Ubuntu-Light, Hack, Noto
    Emoji) are data under the SIL Open Font Licence 1.1 and the Ubuntu Font
    Licence 1.0. Does the rule that a crate be GPL-3.0-compatible apply to
    embedded font data, and which font should the window carry? The work
    goes on with egui's defaults, recorded in THIRD_PARTY_NOTICES.md.
  - A person's Qt settings at the cut-over: the Rust window cannot read
    `QSettings` (the registry on Windows), so the online data keys, recent
    scripts, start-up and toolbar choices, the project folder and the
    layout are lost unless migrated at step 8 or 9.
  - Views floating onto another screen: egui_dock floats a view inside
    the main window, where the C++'s can leave it.
  - A `rust_notices` check that every crate of the lock is listed in
    THIRD_PARTY_NOTICES.md with its version.
- **Step 3's round-trip gate must normalise line ends in stored SQL.** The
  WSL build reads this Windows checkout, where `core.autocrlf` gives the
  C++ sources CRLF, so the C++ built there stores CRLF in the CREATE
  statements of `sqlite_master`; the Rust stores LF, as the committed source
  does. (Whether the Windows-built program stores CRLF too is to be
  checked.)
- **What is still written more than once in the port**: the civil date
  and UTC timestamp, which now have a home in `katana_core::civil_date` but
  whose private copies remain (core's log, storage, ifc, surveyio's RINEX,
  update's manifest, the updater); a Gregorian
  calendar check (surveyio's SDR reader, update's manifest, survey's
  delivery schema), `std::to_chars` into a fixed 64-byte buffer (the GSI and
  RINEX readers, as the C++ has it twice), the hexadecimal float text
  (archive12d alone so far), and surveyio's test helpers (three support
  modules with their own `double_eq`, `near` and Mersenne Twister). cad
  added more: printf's `%.Nf` with the sign of a rounded zero (the plotting
  core, the survey tools, import placement and survey's utility report each
  have a copy), the system error text (copied from storage), and the
  `from_chars` prefix readers; and wave H's colour-table snapshot (the
  survey finish's, the survey code verbs' and a test's), which belongs to
  the colour lookup's module; wave I's tool agents each wrote a tool's call to the trait's own default
  answer, a factory from a closure and a text trim (the drawing tools', the
  edit support's and the annotation tools' copies, beside the tool
  framework's private trim), and the edit support re-derives the fixed
  number form from `format_exact_real`'s digits; and the update layer reads
  a path through a file as not there inline, three times, where
  `katana_core::path_text::path_exists` now holds the rule; and the generic form of
  a path (`generic_u8string`), private in cad's customisation verbs,
  geodesy and gis, now has one home in
  `katana_core::path_text::generic_path_utf8`, which step 6 uses. Each wants one
  home when a second user appears or at the step's end.
- **Defects found in step 3's porting, ported as they are:**
  - storage: a stray file in `backups/` whose number overflows `std::stol`
    ends the program; a backup taken in the same millisecond after pruning
    can reuse a freed name and be pruned at once.
  - dxf: `Sink::blockPath` is never set; MTEXT's `\M` can cut a UTF-8
    character, and the rest of that line is then refused; `writeLeader`
    reads `v[size-2]` of a drawn leader that may have fewer than two
    vertices; `std::abs(INT_MIN)` on a colour; text widths count bytes, not
    characters.
  - archive12d: an invisible vertex short-circuits the storing of its
    segment's visibility; a leading empty point id is lost from the joined
    list; the "points from point strings" notice is dropped past twenty
    problems; `indices[i] - 1` and an unguarded cast are undefined for
    extreme values.
  - surveyio: `addPointMetadata` gives a point first named through metadata
    source record 0; `siblingsInFolder` drops the tail of a file that grows
    while it is read.
  - update: plan.cpp's "is a file, not a folder" also names a special file;
    a cleanup line acted on and listed twice is acted on twice;
    `sweepOlderStages` converts folder names without the guard `nameAsText`
    gives elsewhere, so on Windows a name that is not Unicode throws out of
    `stage()` uncaught.
  - surveyio's instrument readers: the RW5 and GTS-7 unit-change warning
    compares with the first units record, not the previous one; the RW5
    face-mismatch warning says "face left-face"; the SDR lost-RED warning
    quotes ids raw where the lost-MC one cleans them; compact RINEX writes an
    epoch's clock offset half-done when it is too large for its field; the
    JobXML number parse accepts "+-5", and its timestamps negative hours.
- **Where the instrument readers' Rust differs**: a JobXML document that is
  not UTF-8 is refused (the C++ puts its bytes into the model); GSI text
  that is not UTF-8 is decoded as Windows-1252 where it reaches the model,
  so two ids whose bytes differ but decode alike are refused as duplicates;
  fields cut at a byte count may show U+FFFD where the C++ keeps a broken
  character.
- **Defects found in porting cad (step 4, waves F to I), ported as they
  are:**
  - the leaf libraries: `layLinestyle`'s repeat counter, a double, loops for
    ever past 2^53 repeats; grading converts `floor(length / interval)` to a
    size unguarded (a tiny interval is undefined in C++; the Rust saturates,
    then runs out of memory); `dimensionStyleChanges` reports any refusal of
    a command word as "holds a double quote"; a comment in the hatch angle's
    text says a near-zero angle prints 0, where only an exact -0 does; a flat
    definition's text reach counts bytes, not characters.
  - the Document: an aligned dimension's label width counts bytes;
    `open()` and `newDocument()` never refresh the road model's last-seen
    keys, so the next change can report a road-model change that did not
    happen; a surface's decode error text is lossy.
  - styles and scope: `appendArc`'s chord count casts a double to an integer
    unguarded (undefined in C++ for a huge or non-finite count; the Rust
    saturates); the view set's id counter wraps, so ids repeat after 2^32
    views.
  - plotting: `denominatorText`'s `llround`, the grid's tile count and the
    strip's sample count are undefined in C++ for extreme input (the Rust
    saturates); `gridSheets` has no cap on its tiles, so a huge area at a
    fine scale allocates without limit; the generators dereference
    `pointAtStation` unchecked; the grid-label and label-priority sorts meet
    a NaN in no specified order (the Rust's comparison can panic there).
  - the scene and surface display: a colouring built by hand with stops out
    of order indexes past its end (the Rust panics); NaN stops are sorted
    with `<`; `fadeEdges` with a short edge base reads past it; the legend's
    value text gives "?" past its 64-character buffer and "-" before a
    negative value rounded to zero; `Heights::at` is never called.
  - the road model: a template's parameters are matched ignoring case but
    stored with the file's spelling and then looked up exactly, so a
    "Height" key in a file is silently ignored; an error text builds a point
    from its parts in the wrong order; a placement's alignment, profile and
    surface and a culvert's ground levels are dereferenced unchecked
    (unreachable for a validated model).
  - customisation edits and survey tools: the area tool skips a closed curve
    polyline of fewer than three vertices as "an open polyline has no area";
    the line-ends tool refuses a curve polyline; `unsignedZero` strips the
    sign of -inf and -nan; the coordinate conversion's `isGeographic()`
    argument is never read; the inverse report dereferences an inconsistent
    hand-built result (the Rust prints its "no height difference" line).
  - survey jobs: none found beyond the job list's unchecked index in
    `RemoveSurveyJobCommand::execute` (the Rust returns NotFound and undoes,
    on a path that cannot be reached); a re-adjustment takes the last job of
    an id, as the C++'s loop does.
  - the customisation family and road verbs: ROADTEMPLATE RENAME and
    CORRIDOR RENAME build their reply from a reference into the cached set
    after the step replaced it (dangling in C++; the Rust's shared pointer
    gives the old name); the coding report's `coded - matched - fallbackOnly`
    is unsigned and can wrap; CORRIDOR REGION and SUPERELEVATION sort with an
    unstable sort (the Rust's is stable, so entries at one station keep their
    order); `legalDescription` reads the first course of an empty report.
  - the annotation core: the label layout's grid casts a float to an integer
    cell unguarded (the Rust saturates); `surfaceRecord` dereferences a
    surface's triangulation unchecked; both branches of `lineCandidates`'
    `aligned ? ... : ...` pick one justification; `SelectionStep::all`
    ignores the view (noted in the C++).
  - the sheet verbs: their `number()` strips one `+` and reads the rest, so
    "+-1" is -1, where `core::parseFiniteDouble` refuses it - two number
    readers that disagree; `drawnOutline`'s arc step count is undefined for a
    NaN or huge sweep; a NaN line weight or symbol size breaks the legend's
    map order (never observed: only "most entities, then first met" is
    asked); preflight's image check joins the viewport's text as a narrow
    path on Windows, where the verbs use UTF-8.
  - the utilities: a schedule, schema or design file that opens and then fails
    to read (a folder, on Linux) is read as empty text - a silent failure;
    `sampleAlignment` casts its piece count unguarded; views a host may have
    closed, a vertex's point geometry, the first of no points and a
    regrade's named points are dereferenced unchecked (the Rust refuses or
    skips).
  - the interpreter: ALIGN PI and ALIGN PVI build their reply's count as an
    argument beside the step that moves the changed alignment away, in an
    order C++ leaves open, so the count can be read after the move ("0
    PVIs", or one PI too many) - the class of the OPEN count the C++ already
    fixed; the Rust counts before the step. Its number reader, like the
    sheet verbs', reads "+-1" as -1; `check_index` wraps for an empty
    polyline.
  - the tools: the transforms' All option and Stretch's window take entities
    on layers the view hides, where the context now carries the view;
    Revision Cloud and Circle Tangent-Tangent-Tangent clear what was picked
    before a refusal, against the rule that a refusal leaves a tool as it
    was; the 3D polyline accepts Close at two vertices and then refuses it,
    though its prompt offers Close only from three.
- **Where cad's Rust differs** (step 4):
  - A document's command run through another document's stack fails with
    InvalidState; the C++ changes the document that made it. A road-objects
    step outside its document undoes its layers and fails, a path the C++
    does not have.
  - Report tables pad by characters (the survey tools, as the subsurface
    reports, above); a project folder whose name is not UTF-8 shows U+FFFD
    in STATUS; on Windows, a file's identity is its canonical path and a sheet
    file's stem is UTF-8, not the ANSI code page; `Path::exists` answers
    false where `std::filesystem::exists` throws.
  - Of the SIMD kernels, NEON keeps only libstdc++'s `hypot` formula, and,
    like every NEON kernel, is written, not yet compiled (above).
  - The survey point report pads by characters; an option text's property
    name that percent-decodes to bytes that are not UTF-8 reads with U+FFFD;
    UTILITY reads a file's bytes as UTF-8, else Windows-1252, where the C++
    passes them on; a sheet set file that is not UTF-8 gives the JSON
    reader's own parse error rather than a lossy read.
  - The surface edit target and its observers are per thread; observers are
    shared closures, where the C++ copies each `std::function` (they differ
    only for a lambda that changes its own captures).
  - A tool given no document refuses ("There is no drawing to edit.", "there
    is no drawing to dimension") or previews nothing, where the C++
    dereferences a null document (the edit, inquiry, match properties,
    terrain edit, dimension and leader tools); no view reaches it.
- **Defects found in porting step 5, ported as they are:**
  - the GDAL core: `writeTables` asks a geometry `Is3D()` after
    `SetGeometryDirectly` has deleted it, when the layer has no geometry
    field (the Rust asks before handing it over); `readImage`'s
    `longest + maxPixels - 1` overflows for a budget near `INT_MAX`;
    `warpToGeoTiff` sets progress on options it never checked.
  - the point clouds and the catalogue: `parseCatalogue` lets nlohmann's
    type_error out for a member of the wrong JSON type, where its header
    promises ParseFailure, and the window's read of a user's catalogue
    catches nothing (the Rust panics with the same text); the engine asks
    PDAL for a driver outside its guard in `isCopc`, `convertToCopc` and
    `write`; `decimationForBudget` wraps within the budget of 2^64;
    `readColourMap` sorts with no strict weak order when a value is NaN; on
    Windows the engine hands PDAL a path in the ANSI code page (suspected).
  - the bridge and web access: `arcgisExtent` indexes a missing `ymin`,
    `xmax` or `ymax` (undefined; the Rust reads the extent as unreadable);
    the exchange's body callback subtracts sizes that could wrap; the HTTP
    option setter leaves GDAL's error handler pushed when it throws; the
    readers of query pages, items, STAC and CKAN answers let nlohmann's
    type_error out for a member of the wrong type, so a CKAN answer with a
    null licence ends the C++ program (the Rust panics with the same text).
- **Defects found in porting step 5's wave M, ported as they are:**
  `parseReferenceRecord` lets nlohmann's type error out for a member of
  the wrong type (the Rust panics with the same text, which the session
  reports as an internal error); `fetchVectors` replaces `{layer}` by
  searching from the start each time, so a layer name that holds
  `{layer}` loops for ever.
- **One way, not yet:** `lexically_normal` and `lexically_relative` have
  private copies in interop's reference data, in cad and in update; they
  want one home in core's path text.
- **Defects found in porting step 6, ported as they are:** `session.cpp`'s
  `isQuit` and `upperVerb` are two copies of one loop; the update
  progress's step multiplication can wrap; `describeIfc` fails silently on
  a parse it cannot reach; the survey verbs' refusal of a line break in a
  SET item names no item, unlike every other; `importIfc`'s note keeps
  "-0.000" where core's fixed form drops the sign - a third fixed-number
  form; the scoped DXF export drops the status of a removal; the geo
  verbs keep two `positiveOption`s that word their refusals differently and
  four joins of records, two of which keep empty records; `describeJson`
  catches only nlohmann's parse error; `runNow` on a prepared line with
  neither a reply nor work throws.
- **Defect found in porting step 7's wave 0:** the C++ GPU tests' `makeGpu`
  returns nothing for every failure, so a pipeline that does not build is
  skipped as if the device were missing; the Rust skips only when the
  device is not there, and fails on anything else.
- **Where step 7's Rust differs so far:** a failed GPU pipeline or shader
  says wgpu's or naga's words ("the WGSL module did not compile: ..."),
  and wgpu's own steps add sentences of their own (the read-back buffer,
  the frame timer); a GPU target takes 1 or 4 samples; a path's arc is
  split into pieces of at most 90 degrees, where Qt splits it otherwise,
  so a curve's control points can differ slightly; a number grouped for
  the locale puts "," every three digits unless the locale is C or POSIX.
- **Defects found in porting step 7's wave 1, ported as they are:**
  - The plan painter's raster drawing wraps a raster's pixels without
    checking their length, so a short buffer is read past its end; the Rust
    skips such a raster.
  - The text and label style managers send back a value their spin box has
    rounded to two decimals, so Apply rewrites a stored 2.3456 as 2.35. The
    leader manager does not.
  - The disk icon passes `true` as its accent where the outline's
    closedness may have been meant, so Save's label is in the accent.
  - The attribute manager, the survey tool dialogs, the survey import
    wizard's Import and the survey jobs' Apply and Remove change the
    document directly, not through a verb line, so those edits are not
    reachable as commands.
- **Changed from the C++ in step 7's wave 1:** the survey import wizard's
  Browse and Save, its Export, Survey Jobs and the point report opened a
  file dialog even headless, where nothing can answer it; the Rust refuses
  with a sentence instead.
- **Second ways left by step 7's wave 1:** the operating system's error
  sentence has a third copy in the script runner beside the app layer's; the
  survey dialogs keep the C++'s readers and reduction helpers beside the
  app layer's SURVEY verbs; the drive harness's headless runner and the
  foundation's harness both run frames until the window is idle; and the
  roads dialogs keep the C++'s two refusals of a missing command line.
- **The foundation's gaps found by wave 1**, to settle before wave 2: the
  host lends its interpreter only to read, where running a line needs it
  to write; a workbench's dialog that runs a line reaches back into the
  workbench drawing it; a menu item draws no icon cell, and a submenu has
  no icon or status tip; an action has no tooltip of its own; a text field
  has no placeholder; a path cannot be transformed; a colour has no lighter
  or darker; a rectangle has no intersection test; font metrics have no
  cap height; an image keeps no dots per metre; the clipboard carries only
  text and images; a radio button has no mnemonic; and an act taken in a
  pass egui throws away is spent where the icons' controls carry it over.
  Two roads twins were shortened until wave 1 joined: the road object icons
  are checked by name, not by their ink, and the command reference's road
  object verbs by their help, not by its sections.
- **Where step 7's wave 1 differs from the C++:** a curve's stroke is
  offset by tiny-skia's stroker, which follows a circle more closely than
  Qt's, so an edge pixel can differ by a shade; the viewport clipboard
  travels as text, a copy being any text that reads as a sheet set; a
  section title is not bold and a tooltip is plain text, since the window's
  face has no bold; a TIFF drops a translucent raster's alpha (a plotted
  sheet is opaque); a disabled control is faded, not recoloured; a combo
  box's per-item tooltips are folded into its own, and a grid's cell
  colours are not drawn; the survey report's PDF has no title or creator,
  which the PDF canvas does not write yet; a number in a roads message is
  written as `%g` to six digits and a darker colour is worked per channel,
  either of which can differ by one; the survey dialog's Enter does not run
  its first verb; a step of the drive that does not settle, or a name drawn
  twice, fails the run with exit 1, where the C++ had no such failure;
  choosing the project's coordinate system no longer blocks; the leader
  manager's size boxes show 0 where the C++ said "the text style's" or
  "none"; and a dock is dragged by its tab, not its title bar.
- **Defects found in porting step 6's wave 2, ported as they are:**
  - katana_mcp: `Server::handle` says it never throws, but a non-string
    `jsonrpc` member (read outside its try) and a number past the largest
    double (an out-of-range error its catch misses) end the C++ program;
    the Rust panics out of `handle` and exits 101 where the C++ aborts.
  - The vector verbs: COVERAGE CHECK counts features across every table
    GDAL returns, so a second table would read past the end (GDAL returns
    one); OVERLAY drops the warnings of a FILE overlay's read; COVERAGE
    CLEAN works out how many features were unchanged and never says it.
  - The reference layers: three raster finders that answer an id or a
    name differently (bindings', SURFACE's, and the source name's single
    pass, where a raster named "2" before raster 2 is found first);
    `referenceNameTaken` written again in dem_support.
  - The raster verbs: five unchecked dereferences that cannot be reached
    (the Rust checks the invariant); grid's numeric field takes the last of
    its name; viewshed rewrites `arg=output` to `arg=areas` in a record's
    text.
  - EXPORT ending in a bare CLOUD gives the general sentence rather than a
    missing value; RASTER SAMPLE asks for a stop only between scope
    entities.
  - Dead code: `surfaceUsage`, `contourUsage`, `shadeUsage`, `slopeUsage`
    and `aspectUsage`, whose texts the verb table holds again.
- **Second ways in the geo verbs, kept as the C++ has them:** each file's
  own `secondsSince`, `number`, `utf8Of` and `pathOf`; three copies of
  the layer refusal; slope's ring area beside vector_support's; dem's
  `answered` beside two others; the cell area written inline twice beside
  `cellArea`; grid's target refusals beside dem's; three spellings of a
  file name and four of a cancelled error in the Rust. They want one home
  each, in both programs.
- **Where step 6's Rust differs**: off Windows a line that is not UTF-8 is
  refused, where the C++ hands its bytes on and can still reach a file
  named in Latin-1; the update verbs' credential stores are per thread;
  the no-GDAL arms of the C++ (DXF-only IMPORT and EXPORT, the no-network
  update fetch) are not ported, the Rust app always having GDAL; the geo
  scratch folder is `std::env::temp_dir()`'s. Where the C++ leaves the
  order of two refusable arguments unspecified, the Rust reads them left
  to right, so with both wrong it may name the other one; HULL's sort is
  stable where `std::sort` is not, which changes nothing the hull reads;
  MOSAIC's temporary name takes the system clock's nanoseconds, and a
  rounding to an integer saturates past range where the C++ is undefined.
- **One run of the round-trip gate, under the suite's load, found one
  project's two backups the other way round** in the Rust (the schema-13
  one first, the schema-9 one 0.75 s after the save). Six runs of both
  programs alone, and the gate's later runs, gave the C++'s order; the
  storage writes its backups on one thread through SQLite's own backup.
  Not explained; to be watched.
- **Where step 5's Rust differs**: text from GDAL and PDAL is decoded
  lossily; a point with a NaN coordinate is empty where the C++
  constructor may make it not; a temporary name's suffix comes from the
  hasher's random state, not `std::random_device`; an I/O error's text has
  no "(os error N)"; a ring or curve GDAL refuses is deleted, where the
  C++ leaks it. A text argument holding a NUL is cut there by GDAL's C
  API, where the C++ passes the whole string; a spill file goes in
  `std::env::temp_dir()`; a byte cut through a character in a licence,
  title or date reads as U+FFFD; a float-to-integer cast past range
  saturates where the C++ is undefined. Online fetch's text is decoded
  lossily where the C++ keeps the bytes; a size in millimetres is written
  with three decimals, where the C++'s `to_chars` falls back to the exact
  form for a number past 64 characters; the cache's pruning walks its
  folder by a simpler route, which its porter judged to remove the same
  files.
- **Before step 8, for the updater:** Rust's standard library imports
  `bcryptprimitives.dll` on Windows (its random numbers), which the import
  checks' allowed lists (`tests/update/check_updater_imports.cmake`) must
  take, with that reason; the control case that expects a test program to
  import a toolchain DLL may need another program; whether the test key's
  text survives optimisation in the Rust test programs, which the
  test-key check relies on, is unverified; `tools/check_install_rules.cmake`
  must refuse the Rust test programs by name once a Rust program is
  packaged; the Windows tests' console probes declare two kernel32
  functions by hand where a windows-sys feature would do.
- **The two programs reading each other's plan, journal and records** is
  not tested directly yet: both pass the same tests on the same formats,
  and the records are written to nlohmann's text, but no case has the C++
  engine stage what the Rust helper swaps, or the reverse. The first is
  part of step 8's real update on Windows.
- **The round-trip gate does not scan `tests/data`** (the customisations,
  a survey file and a terrain grid shared by several suites): its table
  is checked against each suite's own data folder, as it was first
  written.
- **A GPU hardware test that loses its display aborts** instead of
  skipping, as it skips without a device. The cause of the aborts seen in
  WSL was the cases' `xvfb-run -a` launchers racing for one display, now
  kept apart (docs/gpu.md); a display lost for another reason would still
  abort the case.
- **nlohmann widens its indent string once**, so an indent over 1,024
  characters reads past it in C++ (undefined); `katana_core::json` widens it
  until it fits, the same text wherever the C++ is defined.
- **A non-default `-DKATANA_UPDATE_REPO` or `-DKATANA_UPDATE_FEED` does not
  reach the Rust yet.** Its `build.rs` reads the defaults from
  `src/katana_update/CMakeLists.txt` and would take environment variables of
  the same names, but nothing sets them; the cargo stage of
  `tools/build_visible.sh` and the release jobs must pass them when a build
  that is not the default is made.
- **A non-default built-in customisation does not reach the Rust yet.**
  cad's `build.rs` reads the default and the alternates from
  `src/katana_cad/CMakeLists.txt`; the alternates could come from a
  `KATANA_ALTERNATE_CUSTOMISATIONS` that the cargo stage and the release jobs
  pass, but the default cannot come in under its CMake name, which is also
  the run-time setting that chooses a built-in. It wants a build-time
  variable of another name, decided when a build that is not the default is
  made.
- **Defects found in porting step 7's wave 2, ported as they are:** the
  vertex panel's Insert After computes `segmentCount() - 1` unsigned (a
  polyline with no segments wraps; the Rust refuses the row); the plan
  context menu's Choose Colour starts on the wrong colour for a translucent
  colour (`#RRGGBBAA` written, `#AARRGGBB` read); a double click during a
  point pick falls through to the view; `paintViewport`'s message slice
  wraps when a problem has no ": " (unreachable today); `paintMarks` fills
  a stored sheet outline with `paperFill` where the live key plan uses
  `paperShade`; `plotSheetsToPdf` writes straight to the file, so a failed
  plot leaves a partial PDF; the layer manager, style manager and Global
  Modify's Apply run commands on the Document directly, not as verb lines
  (against the three-surfaces rule; the tests pin their undo steps);
  "Select users" writes the selection directly; a driver's fill of a name
  picker can choose a section header; the vector import and the DEM tools
  each give two controls one name (above); the vector export's Browse has
  no headless guard; `OnlineDataWorkbench::areaOf` does not check for a
  missing view; the export filter names the archive format by its vendor
  (the naming rule); the code manager's Linework status and Use These Codes
  are set only as a field is typed in; `ArgumentForm::setValue` takes "2.0"
  as a whole number and sets 0; the pipeline builder's Add with "write"
  selected leaves the step form hidden; the binding list's Remove reports a
  change with no row current; the volumes dialog measures a surface against
  itself when only one is left, and keeps the last results under a refused
  line.
- **Where step 7's wave 2 differs from the C++:** the style and layer
  managers' tables are text rows (no preview pictures, swatches, amber
  undefined names, bold current row or header sort), as are the census and
  apply tables' marks and the Formats table's sort; a number box shows no
  special text or prefix (`SpinRange` has neither), and the command lines
  are not monospace (`LineOptions` has no font); a PDF's title and creator
  are not written (the PDF canvas cannot yet); the plan view's measure of
  wrapped text and the sheet's cap height are private seams over the
  canvas; online data's "today" is UTC where Qt's was local.
- **Printing is not handed to the operating system yet**:
  `hand_to_print_queue` returns Unsupported after the PDF is written (step
  8, with Windows).
- **Defects found in porting step 7's wave 3, ported as they are:** the
  sheet canvas fits the sheet again on every resize, losing a zoom made by
  hand; adding an image viewport opens a file dialog even headless; the
  interactive plot's busy flag is not reset when the plot throws.
- **Where step 7's wave 3 differs from the C++:** a GPU view's frame is read
  back to the CPU every frame (the window has no `egui_wgpu` yet); the
  dialog frame has no hide and show of its own; a control's record has no
  tooltip, and a list has no row activation, so the sheet list's rename by
  double click never fires; there are no printers to list; the
  `QT_SCALE_FACTOR=1.25` reruns have no twin; `ErrorCode` has no Cancelled,
  so a cancel is `InvalidState`; the customisation workbench named the style
  manager `styleManager` where the C++ renamed it `styleManagerDialog`
  (since put right, below); the plan painter's road model switch is
  process-wide, and the twins that set it hold a mutex; the sheet editor's source provider is given only the
  Document (reference data, meshes, surfaces and the logo must reach it
  another way, in wave 4).
- **Found in the window gate's fix-up round (dialogs and managers):**
  - *A C++ defect, worked round:* Zoom To names two controls `zoomToView`,
    its view chooser and its scope widget's view list. Qt's `findChild`
    visits a widget's own children before their children, so a driver
    reached the chooser, a direct child of the dialog, and never the list.
    The Rust names the list `zoomToScopeViewList`
    (`ScopeFilterWidget::name_view_list`), so each name is drawn once and
    the chooser keeps the C++'s. Rejected: letting the first of two
    controls drawn take the act, which the foundation refuses so that a
    driver never has to guess (and the list, drawn first, would have taken
    it).
  - *A control hidden AND disabled is drawn `withdrawn`*
    (`drive::controls::withdrawn`), and a press or a fill of it is refused
    as disabled, as `--press` and `--fill` ask `isEnabled` before
    `isHidden` (the definition editor's Delete Anyway before Delete was
    refused). An invisible egui `Ui` is always disabled, so only the tag
    tells the two apart. Rejected: asking disabled first for every
    invisible control, which would refuse every merely hidden button as
    disabled.
  - The Alignment Manager draws its pages not in front offstage, as the
    other tabbed dialogs do, so the setting-out interval is filled from the
    first tab; the Reference Data panel gained its opacity and colour
    choices (`referenceOpacity`, `referenceColour`), which run REFS OPACITY
    and REFS COLOR; the code manager's `explainHeading` is reported as the
    plain text a person reads, as `--report` reads a rich QLabel; Generate
    Sheets, Page Setup and Plot are modal, as `QDialog::open` and `exec`
    make them.
- **Found in the window gate's fix-up round (the main window and the
  driver):**
  - *A view a verb opens* is reported as the C++ reported it. `VIEWS OPEN`'s
    record came from a view whose widget the Rust makes only when the
    drawing is next lent, so it said `scale=1` over a 1 x 1 view. The C++'s
    widget is made at once: its constructor sets the initial zoom (10 px a
    metre), and the dock's `show()` after `splitDockWidget` gives it the
    size Qt's split made before `resizeDocks` evened the halves - half the
    active view's side, 330 of 660 points, where the next layout leaves it
    328 (`area=-16.5,-22.35,16.5,22.35`, then `-16.4..16.4`). The workspace
    now starts an unbuilt plan view's state there. Rejected: building the
    widget inside the verb, which would need the drawing the verbs hold;
    and giving it its laid-out size at once, which is not what the C++
    printed. A split along the height halves the dock with its bar, a
    guess no gate test shows.
  - *A target that closed* is said to have closed ("NAME has closed: the
    window is grabbed", "--fill ...: NAME has closed") only when the dialog
    went with it: the C++'s target is a `QPointer`, null only for a widget
    deleted (`Qt::WA_DeleteOnClose`: the sheet editor's Page Setup,
    Generate Sheets, Hidden Layers and Suggest Paper dialogs, the view
    Layers popup and the paper scale dialog), where a dialog the window
    keeps is hidden and stays the target. `DialogFrame::deletes_on_close`
    and `TargetSpec::deletes_on_close` carry the attribute, and the step
    engine remembers it from the last frame that drew the target. Not
    done: a fill or a press of a kept dialog after it closed is still
    refused here, where the C++ acted on the hidden dialog; and such a
    dialog cannot be grabbed, so the window is.
  - *The sheet editor's actions reach the window's steps:* the C++ editor is
    a child of the window, so `findChild<QAction*>` found `sheetGenerate`,
    `sheetCopySetJson` and `sheetPageSetup` from the window once File >
    Sheets had made it; `MainWindow::trigger` and `actions` fall back to the
    editor's, after the window's own. Its toolbar is a target of its own
    (`sheetToolBar`) for `--panel`.
  - *The managers' names:* the window names Format > Layers' copy
    `formatLayersDialog` and the customisation workbench names its style
    manager `styleManagerDialog`, as the C++ did
    (`LayerManagerDialog::set_object_name`,
    `StyleManagerDialog::set_object_name`); a manager made on its own keeps
    `layerManager` and `styleManager`, which its twins drive it by.
- **The corpus round trip failed once under the whole suite's load**
  (`every_file_of_the_corpus_is_read_and_written_alike_by_the_cpp_and_the_rust`,
  wave 4's run) and passed on its own: the schema-9 project's backups
  differed, the Rust side's backup of the migrated file named 6 s before its
  backup of the schema-9 file, so the dump's name order put them the other
  way. Both programs name a backup by the time it is made; the cause is not
  found yet.
- **The window gate's last two differences are Qt's own pixel metrics**:
  at 125 % the 3D view's kind switcher's arrow inks rows 11 to 16 where
  the C++ test expects 11 to 17 (its comment: Fusion draws the arrow into
  a cached pixmap and scales it, which spreads it into one more row; the
  Rust draws the shape at the device's scale, whose exact edges, 11.875 to
  16.875, cover 11 to 16), and Undo's and Redo's arrows 40..49 where
  Fusion's are 39..50; and the toolbars' natural widths along the top are
  2 to 22 points off the C++'s. Matching them is drawing as Qt draws
  (its pixmap scaling, its toolbar handle, spacing and size hints), not a
  defect of either program; until then the 125 % test fails and the
  toolbar-fit test passes with other numbers.
- **One workspace twin fails now and then**:
  `view_chrome::a_maximised_view_fills_the_workspace_and_restore_gives_every_view_back_its_place`
  failed in 2 of 6 runs at wave 3's integration, and in 1 of about 4 since ("view 2 is docked": the
  3D view had no place in the dock) and passed on its own, with the other
  workspace twins and in the whole run. The harness's clock is simulated,
  the tests run headless with no GPU, and the dock code keeps no state
  beyond the test; the cause is not found yet.
