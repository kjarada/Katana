# Architecture

Cross-cutting decisions that every Katana module obeys, and the rules they
come from. Module documents live beside this one; `docs/index.md` says which
is which.

## Purpose

Katana is a deterministic C++ engineering platform for CAD and surveying. The
engine must be able to perform every important engineering operation on its own:
the user interface, the storage format and (later) the AI layer are all
consumers of it, never participants in it.

## The architectural rules

Seven rules shape the design. Code comments cite them by number ("Rule 4"),
so the numbers are fixed: a rule may be reworded here, never renumbered.

**Rule 1 - C++ owns the application.** All core functionality is C++. Python
is never a run-time dependency of the application: it is used at BUILD time
to embed the customisation (`tools/embed_customisation.py`; without Python
the table is empty and Katana draws plain lines) and by developer tools
(`tools/compare_benchmarks.py`, `tools/audit_register.py`,
`tools/check_docs.py`).

**Rule 2 - Nothing changes the model except a validated command.** No front
end - the window, `katana_cli`, a script, and the AI layer when it comes -
writes the model or the project database directly. Each issues a structured
command, which is validated, executed as one transaction and recorded for
undo: request, `commands::Command`, `validate`, `execute`, the change set,
then storage. `CommandInterpreter` builds the same `Command` objects the
window's tools do (`docs/cad.md`, "Command interpreter"; `docs/model.md`,
"Command system"). Direct SQL or memory manipulation from a front end is
the thing this rule forbids.

**Rule 3 - The renderer is not the source of truth.** The domain model is
authoritative; the plan view, the 3D view, the section view, the plot and
every preview are representations of it and hold no state the model does not
have. Made structural by the layering: `katana_render` sees only core, math
and geometry and draws a `render::DrawList`, with no idea a Document exists.

**Rule 4 - External libraries are isolated.** Third-party types never
contaminate the code base: each library is wrapped behind a Katana-owned
interface in one module (`geodesy::CoordinateTransformer` over PROJ,
`gis::` over GDAL, `terrain::TinSurface` over CGAL, `storage::ProjectStore`
over SQLite), and no public header under `include/` names a third-party
type. The `layering` test checks it ("Layering", below).

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
  Failure is reported one way (`core::Result` / `Status`), tolerance is one
  policy (`math::tolerance`), undo is one mechanism (`commands::Command`),
  layering is one list (`tools/check_layering.cmake`). A second way of doing
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
  with a benchmark committed under `benchmarks/` so the measurement can be
  repeated; put both numbers in the commit message and the module's document.
  This machine is shared, so a comparison alternates the builds
  (`tools/compare_benchmarks.py --alternate`) and runs an A/A control beside
  the A/B one, and reports ratios rather than single absolute numbers
  (`docs/testing.md`, "Benchmarks").
- **Judge a build by its exit code.** `-Werror` is on, so a warning is a
  failure. A build filtered for "error" lines once printed "built" over a
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
- **Absent is not zero.** A missing measurement is `std::optional`, never a
  placeholder number: a survey point's height is `std::optional<double>`,
  and a point with no coordinates is a `survey::UnpositionedPoint`, not a
  point at the origin.
- **Parallel work** (section 5.3). Work that divides into parts with disjoint
  file sets runs as parallel agents, each in its own worktree outside the
  checkout, each building with a capped job count (`-j 3`) and reporting
  evidence rather than a description; the merged result is built and tested
  whole before any of it is believed.
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
core → math → geometry → { terrain, render, entity } → commands → storage → cad → app → qt
              geodesy, survey              beside geometry (they see core and math)
              surveyio                     survey + geometry; seen only by app and qt
              archive12d                   terrain + entity; seen by interop, app, qt
              gis, pointcloud → interop    the GDAL/PDAL adapters (katana_io) and import/export
```

`cad` may see neither `interop` nor `surveyio` nor `archive12d`: the
application core builds without GDAL and PDAL (`-DKATANA_BUILD_IO=OFF`), and no
instrument format's or archive reader's own types can reach the drawing. The
exact lists are in the layering check and nowhere else.

A module may include headers from the layers it is listed as depending on in
[`tools/check_layering.cmake`](../tools/check_layering.cmake), and from nowhere
else. Two rules are checked there and run as the `layering` test:

1. **One-way dependencies.** `geometry` cannot include `survey`, `cad` or `qt`.
   The check reads every `#include "katana/<layer>/..."` and compares it against
   the declared allow-list for the file's own layer. Its pattern matches layer
   names of letters and underscores only, so an include of
   `katana/archive12d/...` is not checked at all: `cad` including the archive
   reader would pass today (audit BLD-03, BLD-11). No `cad` file does.
2. **No third-party types in public headers** (Rule 4). Eigen, CGAL, PROJ,
   SQLite, GDAL, PDAL, Qt, Vulkan and nlohmann may appear only in `src/`, never
   under `include/`. Each is wrapped by a Katana-owned interface -
   `Result<T>`-based, using Katana's own value types - so that replacing a
   library is a change to one module rather than to the whole code base.

A new module is registered in `tools/check_layering.cmake` in the same commit
that adds it. The practical consequence of the rules: `katana_math` and
`katana_geometry` have no dependencies beyond the standard library, and their
tests link nothing else.

## Error handling

There are no silent failures and no error codes that a caller can ignore by
accident. Every fallible operation returns
[`katana::core::Result<T>`](../include/katana/core/error.hpp) (or `Status`, its
void form), which holds either a value or an `Error` carrying:

* a typed `ErrorCode` — `InvalidGeometry`, `InvalidCRS`,
  `InvalidSurveyObservation`, `AdjustmentFailure`, `TriangulationFailure`,
  `DatabaseFailure`, `CommandRejected`, and so on;
* a human-readable message;
* a context string holding the offending input — the entity id, the file path,
  the layer name — so a failure can be diagnosed from a log alone.

`Result` is `[[nodiscard]]`. Exceptions are reserved for programmer error:
reading `value()` from a failed `Result` throws `BadResultAccess`, because that
is a bug in the caller, not a runtime condition. Third-party libraries that
throw (CGAL, nlohmann) or that use return codes (SQLite, PROJ) are converted to
`Result` at the module boundary; no third-party exception escapes a module.

A value that cannot be computed is refused or reported, never invented: a
section that cannot reach the ground is counted, not given an elevation; a
lossy export counts what it dropped. `Status` success is `return {};`.

## Numerical policy

Every tolerance in the code base is a named constant in
[`numerics.hpp`](../include/katana/math/numerics.hpp), documented with its unit
and rationale. Ad-hoc epsilons are forbidden — they are how CAD kernels acquire
inconsistent behaviour between subsystems.

| Constant | Unit | Meaning |
|---|---|---|
| `kAbsolute` = 1e-12 | dimensionless | a normalised quantity (a sine, a unit determinant) is zero |
| `kRelative` = 1e-9 | dimensionless | general-purpose scaled equality |
| `kAngular` = 1e-10 | radians | directions are parallel |
| `kGeometric` = 1e-7 | model units | features coincide: duplicate vertices, point-on-curve, tangency |
| `kCoordinate` = 1e-4 | model units | survey coordinates are the same ground mark |

The scale that sets these is the projected coordinate: at a UTM northing of
1e7 m, one ulp of a `double` is about 1.9e-9 m, so no linear tolerance below
about 1e-8 can carry meaning. `kGeometric` sits about 50 ulp above that, leaving
room for a few chained operations to round.

Two further consequences run through the code:

* **Comparisons are explicit.** `operator==` on `Vec2`/`Vec3` is exact; fuzzy
  equality is `nearlyEqual(a, b, tolerance)`. Approximate equality is not
  transitive and must never hide inside an operator.
* **Large coordinates are handled deliberately.** Area, centroid and
  triangulation routines work relative to a local origin. A naive shoelace sum
  over UTM-magnitude coordinates forms products near 2.5e12 and loses roughly
  six significant digits of a 100 m² area; evaluating relative to the first
  vertex keeps full precision. Tests assert this by computing a figure locally
  and again shifted by (500000, 5000000).

Text and units have one home each, low in the stack, so that every layer can
reach them: decoding bytes to UTF-8 is `core/text_encoding.hpp`; trimming,
case folding (`lowered`, `uppered`) and number parsing is `core/text.hpp`
(locale-independent - never `std::isspace`, `std::tolower`, `std::toupper`
or `strtod` on file text: Latin-1's toupper makes 0xE0, the lead byte of a
three-byte UTF-8 sequence, 0xC0); the exact length of a foot or a link is
`math/unit_ratio.hpp`, which `geodesy/units.hpp` is built from
(`docs/geodesy.md`). `katana_cad`'s five private `upper()` copies - four of
them `std::toupper` - are `core::uppered` now. Not done: the same sweep of
`std::toupper` in `katana_app` (the geo verbs, the session, the MCP server),
`katana_ifc` and the plot preflight, and of the `std::isspace` and
`std::isalnum` beside some of them; `katana_dxf`'s reader keeps an ASCII
`upper()` of its own.

## Determinism

Identical inputs and configuration must give identical results (Rule 7).
This is a correctness requirement for an engineering tool, not a convenience.

* Containers that feed output are ordered (`std::map`, sorted vectors). No
  result depends on `unordered_map` iteration order or on pointer addresses.
* Compilation uses `-ffp-contract=off -fno-fast-math`
  (`cmake/KatanaTargetDefaults.cmake`), so the compiler may not fuse or
  reassociate floating-point operations. Results match across optimisation
  levels, which is why every suite is run in Release as well as Debug.
* Parallel work partitions its OUTPUT - one screen tile, one vertex, one
  triangle bin per index - so the bytes are those of the serial run whatever
  the thread count, and tests prove it by running with `TaskPool(1)`
  ("Threading", below).
* Property tests use a fixed-seed generator
  ([`tests/support/property.hpp`](../tests/support/property.hpp)), so a failure
  reproduces exactly.
* Entity ids increase monotonically and are never reused, so a recorded id
  cannot come to mean a different entity after an undo.

## Logging

[`katana::core::Logger`](../include/katana/core/log.hpp) emits structured
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
widget belong to one thread, the GUI thread (or `katana_cli`'s main thread),
and each header says so. What runs in parallel does so inside one call,
through one primitive:
[`katana::core::TaskPool`](../include/katana/core/task_pool.hpp).

* **What it is.** A `std::jthread` pool, created once and kept warm
  (`TaskPool::shared()`, never destroyed), with `hardware_concurrency() - 1`
  workers; the calling thread is the last worker. It offers one operation,
  "run this index range across the cores": `parallelRanges(begin, end, grain,
  body)` and its grain-1 form `parallelFor`. There are no futures and no task
  graphs, because nothing needs them.
* **The contract.** `body` is called exactly once for every index; calls that
  run at once touch disjoint state; the ORDER in which indices are visited is
  unspecified and must not matter. A caller meets it by partitioning its
  output, which is what makes the result bit-identical to the serial run
  (Rule 7).
* **Nesting cannot deadlock.** A `parallelRanges` issued while the pool is
  busy runs serially on the calling thread. Throughput degrades; correctness
  does not.
* **Exceptions stop the job.** One escaping `body` is caught, the remaining
  chunks are skipped, and the first is rethrown to the caller after the
  workers have stopped.
* **Grain is measured, not guessed.** A chunk must cost well above what
  publishing a job to the pool costs; `TinSurface::elevationsAt` records its
  grain of 256 positions with the measurement it came from, and runs inline
  below one grain.

Users today: the software rasteriser (`Rasterizer::render`, which transforms
vertices, builds screen primitives, bins them and rasterises tiles in
parallel; a caller may pass its own pool in `RenderOptions::pool`),
`TiledTerrain::buildAll` (one tile per index) and `TinSurface::elevationsAt`.
Other types that are safe to use from several threads say so in their
headers: `Logger`, and the geodesy value types (`docs/geodesy.md`,
"Threading model"). A PROJ-backed object (`CoordinateTransformer`,
`GridFactorCalculator`) and a `GdalDataset` are one per thread.

## Testing

`docs/testing.md`: the suites, how a test is registered, the headless driver
of the window, and the rules that make a test evidence.

## Build

`docs/building.md`: presets, options, targets, where binaries land, running
`katana` and `katana_cli`, and the packaging decisions - "Where things are
built", "Bundling", "The build tree runs on its own too" and "Why DLLs beside
the program, and not static linking" moved there from this document.

## Failure modes

| Condition | Behaviour |
|---|---|
| Invalid geometry submitted | rejected by `validate()` before entering the model |
| Command fails mid-way | change set rolls back; model unchanged; command not added to history |
| Transaction step fails | earlier steps are undone in reverse order |
| Locked layer edited | `CommandRejected`, nothing modified |
| Project database corrupt | detected on open by integrity check; `recover()` restores the newest sound backup and keeps the damaged file |
| Project from a newer Katana | refused with `Unsupported` rather than guessed at |
| Third-party library throws | converted to a `Result` at the module boundary |

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
| Phase 19 (§24) | performance architecture, the task pool, the language standard | `docs/performance.md`; this document, "Threading" |
| Phase 20 (§25); 20.1, 20.2, 20.3 | file interoperability; where imported data lands, the archive programme, the survey coding programme | `docs/interop.md`, `docs/survey_coding.md` |
| Phase 21 (§26) | alignments, profiles, corridors, parcels, grading | `docs/geometry.md`, `docs/cad.md` |
| Phase 22 (§27) | drawing and plotting | `docs/cad.md`, "Plotting to PDF" |
| Phase 23 (§28) | application API: the command interpreter | `docs/cad.md`, "Command interpreter" |
| Phases 24-26 (§29-30); §31 | Python AI layer, AI agent, production hardening; the AI safety model (a destructive command needs confirmation) | not started, and Rules 1 and 2 bind them; `Command::isDestructive` is in `docs/model.md`, "Command system" |
| §45; 45.1 to 45.5 | the survey module and instrument formats | `docs/survey.md` |
| §46 | the audit register | `docs/audit/2026-09-23-defects.md` |
| §47 | dockable views, per-view layers | `docs/desktop.md`, "The workspace" |
