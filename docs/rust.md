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
| How the work is run | Parallel agents, one per crate | Each agent ports one crate in a worktree of its own; one integration build and whole-suite run per step. |
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
| 5 | gis, pointcloud, interop | 744 | the cases twinned, the online tests too |
| 6 | app | 241 | the 375 cli tests pass against the Rust katana_cli; the replies and saved projects of both programs compared |
| 7 | window, gpu | 1,609 | the 285 headless window tests pass against the Rust window |
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
| 4 | in progress: planned in five waves (F to J); waves F to I done - the JSON home, the leaf libraries, the Document core, styles and scope, the plotting core, the scene and surface display, the road model, the customisation edits, every verb family, the tool framework and the annotation core, the command interpreter and the thirteen tool families (1,992 of 3,011 cases); wave J, the 1,018 cases that drive the interpreter or span two agents' work, remains |
| 5-9 | not started |

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
  fails, never skips. **The Rust side** is `katana_corpus`, a program of
  `katana_app` (the Rust has no CLI until step 6, and `rust_layering` refuses
  a crate that is no layer), which stands in for the part of cad's Document
  that OPEN, SAVE, IMPORT, EXPORT and SURVEY READ touch, with the C++'s
  options.
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
  fails when a data file has no row, so a new file is a decision. Left out,
  each with its reason in the table: the zipped archive and the GIS files
  (step 5), the legacy customisation formats (the owner's decision), the
  scripts, design files and survey settings that need cad (step 4).
- **Its first run found a real difference**: the archive import named an
  alignment "Imported from" the vendor's model in C++ and the archive's in
  Rust, which had followed the naming rule. Both programs now say "archive
  model", as they now say "archive" in the import's two warnings and the
  coverage table (a fix, so the C++ took it with its twin). Two files keep
  one key aside, `reference_layers`: their point clouds are the C++
  session's reference data, which is interop's (step 5).

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

Evidence, 2026-11-10, `tools/build_visible.sh wsl` after wave I: exit 0; the
Release C++ build, Clippy and the float-to-text guard clean; CTest 9,915
entries, 9,875 passed, 40 skipped, none failed; cargo test 5,472 passed, none
failed, none ignored (wave H's run gave 4,975, wave G's 4,433, wave F's
3,724), the round-trip gate among them over 73 files of the corpus.

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
| GDAL, including its algorithm framework | gdal-sys, and the C algorithm API ("The GDAL algorithm spike") | gis |
| PDAL (C++ only) | a C shim of the one adapter file | pointcloud |
| libcurl, the handle GDAL configures | the curl crate on the same libcurl | gis |
| OpenSSL (Ed25519 verification with the RFC 8032 checks) | ed25519-dalek and curve25519-dalek | update |
| zlib (raw inflate) | miniz_oxide | update |
| Qt | eframe, egui, egui_dock, wgpu, rfd, tiny-skia, krilla, cosmic-text | window |

A crate is added to `THIRD_PARTY_NOTICES.md` in the commit that first uses it,
and must be GPL-3.0-compatible.

Four rows above differ from the plan of 2026-11-05, each for a reason found
in porting:

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
  against the prescribed threshold, the solve. It is bit-identical to Eigen
  on systems of up to three equations; beyond that Eigen's SSE2 kernels sum
  in two lanes and the results differ in the last bit or two, inside every
  test's tolerance.
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
(`src/katana_io/geo/processing.cpp`, 102 allow-listed algorithms), which no
Rust binding wraps; it was the one part of the port with no known route.
GDAL 3.13 exposes the same framework in C (`gdalalgorithm_c.h`). The test
`rust/crates/katana_gis/tests/gdal_algorithm.rs` runs `gdal raster info` on
the geo suite's `tests/geo/data/plane.asc` through that API - the registry,
an algorithm from its path, the arguments parsed, the run, the output read
back - and checks the size it reports against the grid's own header (40
columns, 30 rows). It runs in every cargo stage, on both toolchains, which have
the same GDAL, 3.13.2. Phase 5 replaces its hand-written declarations with
the crate's binding.

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
    the vendor's name and which a C++ test asserts; three C++ test names
    holding that name (copied into their Rust twins by the parity rule); and
    the source an archive import gives a surface, the vendor's name before
    "archive <file>" (written by the C++ app and interop layers, and copied
    by the round-trip gate's program so the projects compare equal).
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
  number form from `format_exact_real`'s digits. Each wants one home when a second user
  appears or at the step's end.
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
- **The round-trip gate does not yet compare** SURVEY IMPORT (a survey job
  in a project needs cad, step 4), the zipped archive, GIS data and
  reference layers (step 5), or a Rust DXF of annotation cad draws (step 4).
- **A GPU hardware test aborted once in WSL** instead of skipping:
  `gpu.Devices/GpuRender.AShadedTriangleCoversTheSoftwarePathsPixelsInItsColours/Hardware`
  could not connect to `xvfb-run`'s display under the suite's parallel
  load; run alone it skips, as every run before. A test that loses its
  display should skip as it does without a device.
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
