# Architecture

Cross-cutting decisions that every Katana module obeys, and the rules they
come from. Module documents live beside this one; `docs/index.md` says which
is which.

## Purpose

Katana is a deterministic engineering platform for CAD and surveying, written
in Rust (`docs/rust.md`; it was C++ until the owner had the C++ removed on
2026-11-15). The
engine must be able to perform every important engineering operation on its own:
the user interface, the storage format and (later) the AI layer are all
consumers of it, never participants in it.

## The architectural rules

Seven rules shape the design. Code comments cite them by number ("Rule 4"),
so the numbers are fixed: a rule may be reworded here, never renumbered.

**Rule 1 - The compiled language owns the application.** All core
functionality is Rust (it was C++, under the rule's first wording "C++ owns
the application"). Python is never a run-time dependency of the application,
and not a build dependency either: the built-in customisation is compiled in
with `include_bytes!`, so a build without Python is the same build. It is used by developer tools and checks
(`tools/audit_register.py`,
`tools/check_docs.py`, `tools/customisation_census.py`), which are not
registered where it is absent.

**Rule 2 - Nothing changes the model except a validated command.** No front
end - the window, `katana_cli`, a script, and the AI layer when it comes -
writes the model or the project database directly. Each issues a structured
command, which is validated, executed as one transaction and recorded for
undo: request, `katana_commands::Command`, `validate`, `execute`, the change set,
then storage. `CommandInterpreter` builds the same `Command` objects the
window's tools do (`docs/cad.md`, "Command interpreter"; `docs/model.md`,
"Command system"). Direct SQL or memory manipulation from a front end is
the thing this rule forbids.

**Rule 3 - The renderer is not the source of truth.** The domain model is
authoritative; the plan view, the 3D view, the section view, the plot and
every preview are representations of it and hold no state the model does not
have. Made structural by the layering: `katana_render` sees only core, math
and geometry and draws a `katana_render::DrawList`, with no idea a Document exists.

**Rule 4 - External libraries are isolated.** Third-party types never
contaminate the code base: each library is wrapped behind a Katana-owned
interface in one module (`katana_geodesy::coordinate_transformer::CoordinateTransformer` over PROJ,
`katana_gis` over GDAL, `katana_terrain::TinSurface` over the constrained
triangulation (spade, and the predicates translated from CGAL),
`katana_storage::ProjectStore` over SQLite), and no public item of a crate
names a third-party type ("Layering", below).

**Rule 5 - Test before optimising.** An important subsystem has automated
tests before it is optimised, and an optimisation that culls work (a spatial
filter, a tile bin) has a test proving the culled run finds exactly what the
exhaustive one did.

**Rule 6 - Profile before optimising.** No change is made for speed on an
assumption. Measure first - CPU, memory, cache behaviour, allocation, GPU,
I/O, latency, throughput - and claim a speed-up only with a committed
benchmark and its before and after figures ("Measure, then claim", below;
`docs/performance.md` holds the figures).

**Rule 7 - Computation is deterministic.** Given identical inputs and
configuration, results are identical ("Determinism", below). Parallel and
vectorised code produces the same bits as the serial scalar code unless a
document says otherwise and why.

## Working rules

How work is done here. Older comments cite some of these by the number of a
section in the removed contributor instructions; the number is given in
brackets so such a citation still resolves.

- **One way of doing a thing** (section 1, and section 6's "collapse it").
  Failure is reported one way (`katana_core::Result` / `Status`), tolerance is one
  policy (`katana_math::numerics::tolerance`), undo is one mechanism (`katana_commands::Command`),
  layering is one table (`tools/check_layering.cmake`, which the crates'
  manifests are held to). A second way of doing
  something that already has a first way is a defect, not a shortcut: collapse
  it into the first.
- **Derived expectations** (section 3). Never change an expected value to
  match what the program produced. When a test fails, establish which is
  wrong: the implementation (fix it), the expectation (fix it and justify it
  from a source outside this program - a standard, a published constant, an
  independent calculation, a hand-worked example in a comment), or the
  tolerance (widen it only with the error analysis that justifies the new
  number). Never delete or disable a failing test. `docs/testing.md` has the
  rest of the testing rules.
- **Measure, then claim** (section 4). Measure in Release, before and after,
  with a benchmark committed beside the code so the measurement can be
  repeated, and put both numbers in the commit message and the module's
  document. The Rust has no benchmarks yet; the figures in
  `docs/performance.md` are the C++ build's. This machine is shared, so a
  comparison alternates the builds and runs an A/A control beside the A/B
  one, and reports ratios rather than single absolute numbers
  (`docs/testing.md`, "Benchmarks").
- **Judge a build by its exit code.** Clippy runs with `-D warnings`
  (`-Werror` in the C++ build), so a warning is a failure. A build filtered for "error" lines once printed "built" over a
  missing binary, because a link failure prints no line that a source-path
  filter matches.
- **Comments say why, and every number has a source** (section 6). A constant
  names where it comes from - a standard, a measurement, a stated policy. A
  comment that says what the code does is replaced by the reason or deleted.
  Functions are named, not line numbers. British spelling in prose, American
  in identifiers where the surrounding code already uses it (`color`).
- **A change updates its document in the same commit.** The document of the
  module a change touches is part of the change; `docs/index.md` says which
  document that is, and `tools/check_docs.py` (the `docs` test) checks that
  what the documents cite exists.
- **Absent is not zero.** A missing measurement is an `Option`, never a
  placeholder number: a survey point's height is an `Option<f64>`,
  and a point with no coordinates is a `katana_survey::data_model::UnpositionedPoint`, not a
  point at the origin.
- **Parallel work** (section 5.3). Work that divides into parts with disjoint
  file sets runs as parallel agents, each in its own worktree outside the
  checkout, at most the number the owner allows at once, reporting evidence
  rather than a description; the merged result is built and tested whole,
  once, before any of it is believed (`KATANA_BUILD_JOBS` caps cargo's jobs
  when several worktrees build at once, `docs/building.md`).
- **Report honestly.** A failure is shown with its output; skipped scope is
  named with its reason.

Commit messages carry what changed and why, the evidence, and what is still
open:

```
<area>: <what changed, imperative>

<why it was needed; the alternative rejected, if the choice was not obvious>

Evidence: <test counts, before/after measurements, external source consulted>
Outstanding: <what this deliberately does not do>
```

## Performance targets

The engineering targets every interactive path is judged against. They are
targets, not assumptions: each claim of meeting one needs a benchmark.

| Operation | Target |
|---|---|
| Viewport interaction | 60+ frames per second |
| Simple selection (pick, snap, box) | under 16 ms |
| Simple geometry command | under 100 ms |
| Undo or redo | under 50 ms |
| Opening a small project | under 2 s |
| 10 million survey points | interactive navigation |
| 100 million point cloud | streamed display |
| 1 billion points | out-of-core, tiled, levels of detail |

Memory layout (structure of arrays, pools, arenas, memory mapping) is changed
only after profiling shows it matters, and a change reports allocations,
peak and resident memory. `docs/performance.md` has what has been measured.

## Layering

Dependencies run one way, lowest first:

```
core → math → geometry → { terrain, render, entity } → commands → storage → cad → app → window
              geodesy, survey              beside geometry (they see core and math)
              surveyio                     survey + geometry; seen only by app and window
              archive12d                   terrain + entity; seen by interop, app, window (the .12da archive
                                           only: the older customisation formats' readers were C++ alone and
                                           went with it)
              gis, pointcloud → interop    the GDAL/PDAL adapters and import/export
              update                       core alone; seen only by app and window (the auto-update engine, and
                                           the helper program crates/katana_updater/src, built on it)
              gpu                          render's rule (never a Document); seen by the window
```

`cad` may see neither `interop` nor `surveyio` nor `archive12d`: the
application core builds without GDAL and PDAL, and no instrument format's or
archive reader's own types can reach the drawing. The exact lists are in the
layering table and nowhere else; the crates' `[dependencies]` follow it.

`update` (`crates/katana_update/src/`; `docs/update.md`)
sits beside `math`, not above anything: it depends on `core` and nothing
else, and only `app` and the window depend on it. The reason is the fourth
program. `katana_updater` (`crates/katana_updater/src`) replaces the files of an
install while Katana is not running, from a copy of itself in a temporary
folder, so it must be small and static and link no GDAL, no GUI toolkit and
nothing of the drawing - and it is built from this layer and `katana_core` alone.
Three consequences follow from the same rule:

- **The network is injected.** The engine takes a function that fetches
  (`katana_update::Fetch`) as a parameter; `katana_app` makes one from the project's
  one HTTP stack (`crates/katana_app/src/update_transport.rs`), and a test hands in
  a fake. The layer never sees libcurl.
- **The machine is behind interfaces.** Every change to the disk, every
  process started or looked for, the registry and the credential store are
  interfaces of the layer (`FsOps`, `Launcher`, `ProcessOps`, `RegistryOps`,
  `CredentialStore`), with one real implementation per platform in the
  crate, so that the swap and its recovery are driven in process by tests.
  One question about the machine has no interface: whether the process
  was started as administrator (`katana_update::process_is_elevated`). It is asked
  in two places and handed on from each, so that a test gives the answer
  and never the machine: when a program's update context is made, as a
  member of that context (`UpdateContext::elevated`); and by
  `katana_update::recover`, the first act of every program, which hands it to the
  recovery its tests drive (`katana_update::recover::recover_with`).
- **Its two libraries stay inside the crate** like every other (Rule 4): the
  signature is checked with `ed25519-dalek` and an update is inflated with
  `miniz_oxide` (OpenSSL's libcrypto and zlib in the C++ build), both pure
  Rust and compiled in, so that the helper imports only the operating
  system's DLLs.

The verbs that drive it, `UPDATE ...`, are the session's
(`crates/katana_app/src/update_verbs.rs`), not the interpreter's: `cad` cannot see
`update`, and an update acts on the program, not on a drawing.

A crate may use the crates it lists as `[dependencies]` in its `Cargo.toml`,
and nothing else: the layering holds by construction, since a crate cannot
name what it does not depend on. Two rules follow:

1. **One-way dependencies.** `geometry` cannot use `survey`, `cad` or the
   window, because its `Cargo.toml` does not list them, and cargo refuses a
   cycle. Which layer may depend on which is one table,
   [`tools/check_layering.cmake`](../tools/check_layering.cmake) (its
   `KATANA_ALLOWED_<layer>` lines), and `tools/check_rust_layering.py` holds
   every crate's manifest to it, with the fixtures of
   `tests/rust_check_fixtures/layering/` that break each rule. In the C++ build
   the same table was read against every `#include` (the `layering` test).
2. **No third-party types in a public interface** (Rule 4). PROJ, SQLite,
   GDAL, PDAL, wgpu, egui and the other third-party crates appear only
   inside the crate that wraps each - GDAL and PDAL through FFI modules and
   the three C++ shims under `crates/katana_gis/native/` and
   `crates/katana_pointcloud/native/`. Each is wrapped by a Katana-owned
   interface - `Result<T>`-based, using Katana's own value types - so that
   replacing a library is a change to one crate rather than to the whole code
   base.

A new layer is a new crate under `crates/` and a row of the layering
table, in the same commit.

**Unsafe code** is denied in the workspace's lints (`Cargo.toml`,
`unsafe_code = "deny"`) and allowed, module by module and each with its
reason, only where a module must be unsafe: the FFI adapters to GDAL, PDAL,
PROJ, SQLite and libcurl, the operating-system calls of the update layer,
the entries into the SIMD kernels (the kernels themselves are safe
functions since 2026-11-20, `docs/performance.md`, "Safe kernels"), and the
one module that hands the task pool's tasks
disjoint parts of one buffer (`docs/rust.md`, "The workspace"). On
2026-11-16 the owner decided that it is to go everywhere it can: safe crates
in place of the bindings of our own, the hand-written SIMD kernels removed
in favour of their scalar twins (superseded on 2026-11-20: the kernels
stay, made safe, for their speed), and `#![forbid(unsafe_code)]` in every
crate but the two shim boundaries of GDAL's algorithm framework and PDAL.
That work has not started (`docs/rust.md`, "Not done").

The check runs in every suite as the test runner's `rust_layering` (with
Python found), beside its `rust_layering_catches_*` cases on those fixtures,
and by hand as `python tools/check_rust_layering.py`. The practical
consequence of the rules: `katana_math` and `katana_geometry` have no
dependencies beyond the standard library, and their tests link nothing else.

## Error handling

There are no silent failures and no error codes that a caller can ignore by
accident. Every fallible operation returns
[`katana_core::error::Result<T>`](../crates/katana_core/src/error.rs) (or `Status`, its
unit form, `Result<()>`), which holds either a value or an `Error` carrying:

* a typed `ErrorCode` — `InvalidGeometry`, `InvalidCRS`,
  `InvalidSurveyObservation`, `AdjustmentFailure`, `TriangulationFailure`,
  `DatabaseFailure`, `CommandRejected`, and so on;
* a human-readable message;
* a context string holding the offending input — the entity id, the file path,
  the layer name — so a failure can be diagnosed from a log alone.

`Result` is `#[must_use]`, so an ignored failure is a warning and so a build
failure. A panic is reserved for programmer error, as an exception was in the
C++ (`BadResultAccess`). Third-party libraries that use return codes (SQLite,
PROJ, GDAL) or that may throw behind their C++ API (PDAL, through its shim)
are converted to `Result` at the crate boundary; no foreign error escapes a
crate.

A value that cannot be computed is refused or reported, never invented: a
section that cannot reach the ground is counted, not given an elevation; a
lossy export counts what it dropped. `Status` success is `Ok(())`.

## Numerical policy

Every tolerance in the code base is a named constant in
[`numerics.rs`](../crates/katana_math/src/numerics.rs), documented with its unit
and rationale. Ad-hoc epsilons are forbidden — they are how CAD kernels acquire
inconsistent behaviour between subsystems.

| Constant | Unit | Meaning |
|---|---|---|
| `ABSOLUTE` = 1e-12 | dimensionless | a normalised quantity (a sine, a unit determinant) is zero |
| `RELATIVE` = 1e-9 | dimensionless | general-purpose scaled equality |
| `ANGULAR` = 1e-10 | radians | directions are parallel |
| `GEOMETRIC` = 1e-7 | model units | features coincide: duplicate vertices, point-on-curve, tangency |
| `COORDINATE` = 1e-4 | model units | survey coordinates are the same ground mark |

The scale that sets these is the projected coordinate: at a UTM northing of
1e7 m, one ulp of a `double` is about 1.9e-9 m, so no linear tolerance below
about 1e-8 can carry meaning. `GEOMETRIC` sits about 50 ulp above that, leaving
room for a few chained operations to round.

Two further consequences run through the code:

* **Comparisons are explicit.** `==` on `Vec2`/`Vec3` is exact; fuzzy
  equality is `nearly_equal(a, b, tolerance)`. Approximate equality is not
  transitive and must never hide inside an operator.
* **Large coordinates are handled deliberately.** Area, centroid and
  triangulation routines work relative to a local origin. A naive shoelace sum
  over UTM-magnitude coordinates forms products near 2.5e12 and loses roughly
  six significant digits of a 100 m² area; evaluating relative to the first
  vertex keeps full precision. Tests assert this by computing a figure locally
  and again shifted by (500000, 5000000).

Text and units have one home each, low in the stack, so that every layer can
reach them: decoding bytes to UTF-8 is `crates/katana_core/src/text_encoding.rs`; trimming,
case folding (`lowered`, `uppered`) and number parsing is `crates/katana_core/src/text.rs`
(locale-independent: the C++ build's rule was never `std::isspace`,
`std::tolower`, `std::toupper` or `strtod` on file text, since Latin-1's
toupper makes 0xE0, the lead byte of a three-byte UTF-8 sequence, 0xC0; Rust's
text functions take no locale, and the port keeps the one home); the exact
length of a foot or a link is `crates/katana_math/src/unit_ratio.rs`,
which `crates/katana_geodesy/src/units.rs` is built from
(`docs/geodesy.md`). The C++ `katana_cad`'s private case-folding copies were
swept into `katana_core::text::lowered`, `uppered`, `ascii_lower` and
`is_ascii_space` before the port; the same sweep of `katana_app` and
`katana_ifc` was not made, and their ports kept the private folding they had
(`katana_dxf`'s reader keeps an ASCII `upper()` of its own).

## Determinism

Identical inputs and configuration must give identical results (Rule 7).
This is a correctness requirement for an engineering tool, not a convenience.

* Containers that feed output are ordered (`BTreeMap`, sorted vectors). No
  result depends on `HashMap` iteration order or on addresses.
* The compiler may not fuse or reassociate floating-point operations: Rust
  never contracts `a * b + c` into a fused multiply-add unless the code asks
  (`mul_add`), and has no fast-math mode (the C++ build had to say
  `-ffp-contract=off -fno-fast-math`). Results match across optimisation
  levels.
* Parallel work partitions its OUTPUT - one screen tile, one vertex, one
  triangle bin per index - so the bytes are those of the serial run whatever
  the thread count, and tests prove it by running with `TaskPool(1)`
  ("Threading", below).
* Property tests use a fixed-seed generator
  ([`crates/katana_cad/tests/support/property.rs`](../crates/katana_cad/tests/support/property.rs)), so a failure
  reproduces exactly.
* Entity ids increase monotonically and are never reused, so a recorded id
  cannot come to mean a different entity after an undo.

## Logging

[`katana_core::Logger`](../crates/katana_core/src/log.rs) emits structured
records: timestamp, level, category, message and key/value fields, rendered as
`2026-09-19T10:15:30.123Z INFO [command] executed name=CREATE_LINE entities=1`.

A logger is an ordinary object passed to the subsystems that need one; there is
no global logger. `log()` is safe to call concurrently and serialises sink
invocation. Values containing whitespace or `=` are quoted, so records stay
parseable. Commands log their name, change count and execution time; the
convention is to log identifiers and counts rather than coordinate payloads, so
logs do not accumulate project data.

## Threading

The document, its model, the command stack, the project store and every
control belong to one thread, the window's (or `katana_cli`'s main thread);
the types are not `Sync` where that matters, so the compiler says so. What runs in parallel does so inside one call,
through one primitive:
[`katana_core::TaskPool`](../crates/katana_core/src/task_pool.rs).

* **What it is.** A pool of `std::thread`s, created once and kept warm
  (`TaskPool::shared()`, never destroyed), with one worker fewer than the
  machine's hardware threads; the calling thread is the last worker. It
  offers one operation, "run this index range across the cores":
  `parallel_ranges(begin, end, grain, body)` and its grain-1 form
  `parallel_for`. There are no futures and no task
  graphs, because nothing needs them.
* **The contract.** `body` is called exactly once for every index; calls that
  run at once touch disjoint state; the ORDER in which indices are visited is
  unspecified and must not matter. A caller meets it by partitioning its
  output, which is what makes the result bit-identical to the serial run
  (Rule 7).
* **Nesting cannot deadlock.** A `parallel_ranges` issued while the pool is
  busy runs serially on the calling thread. Throughput degrades; correctness
  does not.
* **A panic stops the job.** One escaping `body` is caught, the remaining
  chunks are skipped, and the first is resumed on the caller after the
  workers have stopped (an exception in the C++ build).
* **Grain is measured, not guessed.** A chunk must cost well above what
  publishing a job to the pool costs; `TinSurface::elevations_at` records its
  grain of 256 positions with the measurement it came from, and runs inline
  below one grain.

Users today: the software rasteriser (`Rasterizer::render`, which transforms
vertices, builds screen primitives, bins them and rasterises tiles in
parallel; a caller may pass its own pool in `RenderOptions::pool`),
`TiledTerrain::build_all` (one tile per index) and `TinSurface::elevations_at`.
Other types that are safe to use from several threads say so in their
documentation: `Logger`, and the geodesy value types (`docs/geodesy.md`,
"Threading model"). A PROJ-backed object (`CoordinateTransformer`,
`GridFactorCalculator`) and a `GdalDataset` are one per thread.

## Testing

`docs/testing.md`: the suites, how a test is registered, the headless driver
of the window, and the rules that make a test evidence.

## Build

`docs/building.md`: building with cargo, where the programs land, running
`katana` and `katana_cli`, the CMake test runner, and the packaging
decisions.

## Failure modes

| Condition | Behaviour |
|---|---|
| Invalid geometry submitted | rejected by `validate()` before entering the model |
| Command fails mid-way | change set rolls back; model unchanged; command not added to history |
| Transaction step fails | earlier steps are undone in reverse order |
| Locked layer edited | `CommandRejected`, nothing modified |
| Project database corrupt | detected on open by integrity check; `recover()` restores the newest sound backup and keeps the damaged file |
| Project from a newer Katana | refused with `Unsupported` rather than guessed at |
| Third-party library fails | converted to a `Result` at the crate boundary |
| An update of the program interrupted by a crash or a power cut | `katana_update::recover`, the first thing `main()` does in every program, has the old version's files put back before anything else is read (`docs/update.md`, "Recovery") |
| An updated program that does not start | the update helper puts every file back, starts the old version again, and that release is neither offered nor applied again until the person asks for it (`UPDATE BLOCKED CLEAR`; `docs/update.md`, "The swap") |
| The update helper itself dies while the new version is proving that it starts | the tree is a whole new version that nothing has confirmed, and no program's recovery touches it. It is settled from what is on the disk, under the update lock, by `katana_updater --recover` at the next logon, by a window of that version at its first idle turn, or by the next update: confirmed on the new version's marker; put back only by `--recover`, and only when the files are still that release's and no program of the install is running; otherwise left for a start that can decide (`docs/update.md`, "An update whose helper died during the health check") |
| A program of the install is started while an update is being prepared | the helper lists the running programs a second time, after its plan and before the first file moves, and gives way: nothing is touched and the download is kept (`docs/update.md`, "The swap", step 4) |
| Katana was started as administrator and asked to install an update | refused before the helper is copied (`reason=elevated`): the helper would run with those rights from a folder ordinary programs can write. Started the ordinary way it installs the release, which stays downloaded (`docs/update.md`, "The three kinds") |
| Katana was started as administrator and finds an update that a crash interrupted | its recovery runs the install's own `bin/katana_updater` and never the helper copy in the per-user folder, for the same reason; where the install's own is not there, nothing is run and stderr names the two ways out (`docs/update.md`, "The three kinds") |
| A scanner or a backup has an update's journal open when it is looked for | what could not be read is never taken for "no update here": the logon's `--recover` leaves it with its entry registered again, a window asks again, and a new update is refused until the journal reads (`docs/update.md`, "An update whose helper died during the health check") |

## Section and phase numbers in older comments

Comments written before 2026-09-24 cite the original development plan, which
is no longer in the tree, by section ("§32", "section 36", "20.3") or by phase
("Phase 18"). A plan section numbered 6 or more is phase (section - 5):
section 23 is Phase 18. This table resolves each number to its subject and
the document that covers it now.

| Cited as | Subject | Documented in |
|---|---|---|
| Rule 1 to Rule 7 | the architectural rules | this document, "The architectural rules" |
| §32 | performance targets | this document, "Performance targets"; `docs/performance.md` |
| §33 | memory architecture | this document, "Performance targets" |
| §34 | testing strategy | `docs/testing.md` |
| §35 | numerical correctness | this document, "Numerical policy" |
| §36 | error handling, no silent failures | this document, "Error handling" |
| §37 | logging | this document, "Logging" |
| Phase 01 (§6) | build system | `docs/building.md` |
| Phases 02-04 (§7-9) | mathematics, geometry and its algorithms | `docs/geometry.md` |
| Phase 05 (§10); 5.1 to 5.4 | entity system; 5.1 nested layers, 5.2 resolved appearance, 5.3 linetypes, 5.4 dimension styles | `docs/model.md` |
| Phases 06-07 (§11-12) | commands; project storage | `docs/model.md`, `docs/storage.md` |
| Phases 08-09 (§13-14) | the CAD application; professional 2D CAD | `docs/cad.md`, `docs/desktop.md`, `docs/tools.md` |
| Phases 10, 12, 13 (§15, 17, 18) | survey model, survey calculations, least squares | `docs/survey.md` |
| Phase 11 (§16) | coordinate systems and units | `docs/geodesy.md` |
| Phase 14 (§19) | terrain | `docs/terrain.md` |
| Phases 15-16 (§20-21) | 3D rendering, 3D CAD | `docs/render.md` |
| Phase 17 (§22) | point clouds | `docs/interop.md` |
| Phase 18 (§23) | spatial indexing | `docs/cad.md`, "Spatial indexing" |
| Phase 19 (§24) | performance architecture, the task pool, the language standard (the C++'s, with no Rust counterpart) | `docs/performance.md`; this document, "Threading" |
| Phase 20 (§25); 20.1, 20.2, 20.3 | file interoperability; where imported data lands, the archive programme, the survey coding programme | `docs/interop.md`, `docs/survey_coding.md` |
| Phase 21 (§26) | alignments, profiles, corridors, parcels, grading | `docs/geometry.md`, `docs/cad.md` |
| Phase 22 (§27) | drawing and plotting | `docs/plotting.md`; `docs/cad.md`, "Plotting to PDF" |
| Phase 23 (§28) | application API: the command interpreter | `docs/cad.md`, "Command interpreter" |
| Phases 24-26 (§29-30); §31 | Python AI layer, AI agent, production hardening; the AI safety model (a destructive command needs confirmation) | not started, and Rules 1 and 2 bind them; `Command::is_destructive` is in `docs/model.md`, "Command system" |
| §45; 45.1 to 45.5 | the survey module and instrument formats | `docs/survey.md` |
| §46 | the audit register | `docs/audit/2026-09-23-defects.md` |
| §47 | dockable views, per-view layers | `docs/desktop.md`, "The workspace" |
