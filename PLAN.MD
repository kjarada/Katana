# AI Engineering Plan

## High-Performance C/C++ CAD & Survey Platform

## 1. Mission

Build a professional, high-performance CAD and surveying application from the ground up.

The system must prioritize:

1. Numerical correctness
2. Deterministic behavior
3. High performance
4. Modular architecture
5. Testability
6. Large-dataset support
7. Professional CAD workflows
8. Professional surveying workflows
9. Long-term extensibility
10. AI-assisted operation

The application must be implemented primarily in **C++**.

Use **C** only where low-level C implementations provide a clear benefit.

Use **Python exclusively for the AI layer and AI-related tooling**.

Do **not** use:

* Blender
* Rust
* Python for the core application
* Python for geometry
* Python for surveying calculations
* Python for rendering
* Python for the main UI

The C++ application must remain fully functional without the AI layer.

---

# 2. Core Architecture

Use this dependency hierarchy:

```text
AI Layer
    │
    ▼
C++ Application API
    │
    ▼
Command System
    │
    ▼
Domain Model
    │
    ├── CAD
    ├── Survey
    ├── Terrain
    ├── Civil
    └── GIS
    │
    ▼
Computational Core
    │
    ├── Geometry
    ├── Mathematics
    ├── Geodesy
    ├── Spatial Algorithms
    └── Numerical Computation
    │
    ▼
Infrastructure
    │
    ├── Storage
    ├── Memory
    ├── Concurrency
    └── File I/O
    │
    ▼
Rendering
    │
    ▼
Qt UI
```

The dependency direction must remain one-way.

Lower layers must never depend on higher-level application features.

For example:

```text
Geometry
```

must not depend on:

```text
Qt
Survey
AI
CAD UI
SQLite
```

---

# 3. Primary Technology Stack

## Core


* ++23 where compiler/toolchain support is sufficiently mature
* CMake
* Clang
* GCC
* MSVC

## GUI

Use:

* Qt 6
* C++

## Rendering

Use:

* Vulkan

Create a rendering abstraction so the domain model does not directly depend on Vulkan.

## Numerical Mathematics

Use:

* Eigen

## Computational Geometry

Use:

* CGAL where appropriate

## Solid CAD Geometry

Use:

* Open CASCADE Technology

## Coordinate Reference Systems

Use:

* PROJ

## GIS

Use:

* GDAL

## Point Clouds

Use:

* PDAL

## Database

Use:

* SQLite

## Compression

Consider:

* Zstandard
* LZ4
* zlib

## Testing

Use:

* GoogleTest
* Property-based testing where appropriate

## Profiling

Use:

* Tracy
* Native platform profilers
* GPU profiling tools

## AI

Use:

* Python
* LLM APIs
* Python AI orchestration

Python must communicate with the C++ application through a defined API or IPC mechanism.

---

# 4. Absolute Architectural Rules

The AI coding agent must follow these rules throughout development.

## Rule 1 — C++ owns the application

All core functionality must be implemented in C++.

Python cannot become a hidden dependency of the core application.

## Rule 2 — AI never directly modifies the database

The AI must issue structured commands.

Correct:

```text
User
 ↓
AI
 ↓
Structured Command
 ↓
Validation
 ↓
C++ Command Engine
 ↓
Transaction
 ↓
Database
```

Incorrect:

```text
User
 ↓
AI
 ↓
Direct SQL / arbitrary memory manipulation
```

## Rule 3 — The renderer is not the source of truth

The domain model is authoritative.

The viewport is a representation of the domain model.

## Rule 4 — External libraries must be isolated

Do not allow external library types to contaminate the entire codebase.

Create internal interfaces.

Example:

```cpp
class CoordinateTransformer;
class GeometryKernel;
class TerrainEngine;
class PointCloudEngine;
```

rather than exposing third-party implementation types everywhere.

## Rule 5 — Test before optimization

Every important subsystem must have automated tests before aggressive optimization.

## Rule 6 — Profile before optimizing

Do not optimize based solely on assumptions.

Measure:

* CPU
* memory
* cache behavior
* allocation
* GPU
* I/O
* latency
* throughput

## Rule 7 — Preserve deterministic computation

Given identical inputs and configuration, computational results should be reproducible within defined numerical tolerances.

---

# 5. Development Strategy

Development must proceed from simplest to most complex.

Do not implement advanced systems prematurely.

The required progression is:

| Phase | | Status |
|---|---|---|
| 01 | Build System | **done** |
| 02 | Mathematics | **done** |
| 03 | Basic Geometry | **done** |
| 04 | Geometry Algorithms | **done** |
| 05 | Entity System | **done** |
| 06 | Command System | **done** |
| 07 | Project Storage | **done** |
| 08 | Basic CAD | **done** |
| 09 | Professional 2D CAD | partial — nested layers, resolved appearance, linetypes, dimension styles and hatching delivered; authoring splines and blocks outstanding |
| 10 | Survey Data | **done** |
| 11 | Coordinate Systems | **done** |
| 12 | Survey Calculations | **done** |
| 13 | Least Squares | **done** |
| 14 | Terrain | **done** |
| 15 | 3D Rendering | partial — tiled multithreaded SOFTWARE renderer, camera, tiled viewports, 3D and section views; Vulkan backend outstanding |
| 16 | 3D CAD | reshaped — Open CASCADE rejected with reasons; civil solids on the TIN instead |
| 17 | Point Clouds | partial — PDAL read/write, budgeted decimation, 2D display; COPC conversion and resolution queries in the engine; import-time conversion and view-driven re-query outstanding |
| 18 | Spatial Indexing | partial — sparse hash grid behind snapping, picking and box selection (50x-310x measured); no KD-tree, BVH, octree or terrain quadtree |
| 19 | Performance Architecture | partial — `katana::core::TaskPool` drives the renderer; task graph and work stealing rejected with reasons; `compareSurfaces` 12.1 s -> 2.1 s by pairwise triangle clipping |
| 20 | File Interoperability | partial — raster, vector, point cloud and 12d Archive import/export in GUI and CLI; no DWG/LandXML/IFC |
| 21 | Civil Engineering | partial — sections; clothoid; alignments and profiles; corridor quantities and surface, cross-checked; parcels with bearings, area, deed wording and labels; richer assemblies and grading outstanding |
| 22 | Drawing/Plotting | partial — Plot to PDF through the screen's own drawing code at ISO sizes and standard scales, `Layer::lineWeight` finally honoured; layouts, title blocks, annotation scaling, printing outstanding |
| 23 | C++ Application API | reshaped — expose the existing interpreter, do not write a second one |
| 24 | Python AI | not started |
| 25 | AI Agent | not started |
| 26 | Production Hardening | not started |

The delivered phases below are recorded rather than specified: their
requirements have been met, so only what was built and where it lives is kept.
Section numbers are deliberately NOT renumbered — source comments cite them
(`PLAN.MD section 32`, `Rule 4`), and renumbering would silently invalidate
every such reference.

Do not skip phases without explicit architectural justification.

## 5.1 Standing process — the plan and the documentation are deliverables

`CLAUDE.md` at the repository root is the working contract for every
contributor, human or model, and is loaded automatically at the start of a
session. It is normative; this section states the parts that bind the plan
itself.

**Update this file as part of the change, not afterwards.** Whenever a phase or
sub-feature is completed, or anything changes what the software can do:

* Set the phase's status here using the vocabulary
  `**STATUS: DELIVERED.**` / `**STATUS: PARTIALLY DELIVERED.**`, and for a
  partial one write an explicit `OUTSTANDING:` paragraph. Never quietly narrow
  a phase to whatever was managed.
* Keep the roadmap table above in step.
* Update the document in `docs/` for the area, and `README.md`.
* Record the alternative that was rejected and why, wherever the decision is
  not obvious. `docs/storage.md` is the worked example: a measurement, a
  decision, and the reasons the other options lost.

**Do not renumber sections.** Source comments cite them (`PLAN.MD section 32`,
`Rule 4`); renumbering silently invalidates every such reference.

**If the plan turns out to be wrong** — it asks for something that proved to be
a bad idea, or the ground has moved — say so here, in place, with the reason.
Do not silently obey it and do not silently ignore it.

## 5.2 Standing process — commit at every milestone

A milestone is: a phase or sub-feature complete with its tests passing; a bug
fixed with a regression test that is proven to catch it; a refactor landed with
the suite green; or a measured performance change. At each one, commit — with
the plan and documentation already updated in the same change, and with the
evidence (test counts, before/after numbers, the external source consulted) in
the message.

Do not batch unrelated work into one commit, and do not commit a half-finished
feature to save progress.

## 5.2b Standing process — do not stop until the plan is finished

Work continuously through this plan until every phase is delivered. Finish a
milestone, commit it, and begin the next in the same session; do not stop at a
natural pause point and do not stop to ask whether to continue.

Stop only for a decision that is genuinely the user's (destructive,
outward-facing, or a fork where the options lead to materially different
software), for a hard block naming exactly what is missing, or because the plan
is complete. Running out of obvious next steps is not a stopping condition: the
table in this section lists what remains, and `CLAUDE.md` section 6 lists the
standing improvement work.

This does not license committing a half-finished feature to keep moving. Every
commit still lands green, with the plan and documentation updated in the same
change.

## 5.2a Standing process — hand over something runnable

Before stopping work, leave a Release build that runs and say how to run it,
what to look at, and what correct looks like. A session that ends with a green
test suite and no runnable application has delivered nothing the user can
check. `CLAUDE.md` section 5.1 is normative on this.

Say plainly which of the work is reachable from the GUI and which exists only
in the library with no way to invoke it yet - a feature with no route to it from
the application is finished work in the tests and unfinished work to the user.

## 5.3 Standing process — self-improvement

Every contributor is expected to leave the codebase easier to work on, not only
larger. On every session, look for and act on: a second way of doing something
that already has a first way; a comment that says what instead of why; a test
that would pass if the code were wrong; a silently ignored failure; a constant
with no stated source; anything that took three files to work out and is
written down in none of them.

A real defect found outside the current task is either fixed, if it is small
and adjacent, or recorded here under the phase it belongs to. Never leave it
only in a conversation: the conversation is lost, the repository is not.

---

# 6. Phase 01 — Build System

**STATUS: DELIVERED.**

CMake 3.24+ / Ninja / C++20, one target per module. `-Wall -Wextra -Wpedantic
-Wshadow -Werror`, and `-ffp-contract=off -fno-fast-math` for Rule 7.

* `CMakePresets.json` - debug, release, relwithdebinfo, sanitize, tidy, and the
  two Linux presets.
* `cmake/KatanaSanitizers.cmake` - ASan + UBSan on GCC/Clang and MSVC. MinGW
  ships no libsanitizer, so it falls back to UBSan trap mode; CI runs the real
  sanitizers on Linux.
* `cmake/KatanaStaticAnalysis.cmake` - clang-tidy and cppcheck as compiler
  launchers, so a finding fails the build.
* `cmake/KatanaThirdParty.cmake` - GoogleTest and Google Benchmark cached under
  `third_party/_cache`, so extra build directories configure offline.
* **Where things are built.** Everything a person runs lands in `<build>/bin`:
  `katana.exe` (the application - the target is `katana`, not the old
  `katana_qt_app`) and `katana_cli.exe`, with tests in `bin/tests` and
  benchmarks in `bin/benchmarks` so that `bin` itself holds only what ships.
  CMake's default had put the application at
  `build/release/src/katana_qt/katana_qt_app.exe`, a path that read as though
  the executable were in the sources.
* `cmake/KatanaPackaging.cmake` - `cmake --install`, a `bundle` target
  (`<build>/dist/Katana`) and CPack (`package`: a ZIP, plus an NSIS installer
  when `makensis` is installed). The installed tree is SELF-CONTAINED:
  `windeployqt` brings Qt and its plugins, a dependency scan over the
  executables AND the deployed plugins brings the other 184 DLLs, and
  `share/proj` and `share/gdal` carry the data files. Verified by running both
  bundled programs with `PATH` reduced to the Windows directories: the GUI
  executable plots a PDF headlessly and the CLI exports a DXF. It carries
  `qoffscreen.dll`, which windeployqt omits, because `--plot` is a headless
  feature and without that plugin Qt does not fail - it opens a modal box and
  waits for ever. Record in `docs/architecture.md`.

* **The executable is branded.** `katana.rc.in` embeds the application icon
  and a `VERSIONINFO` block (version from `project(... VERSION ...)`, written
  once). The icon is painted by the application's own icon code and written to
  `resources/katana.ico` by the `katana_make_icons` developer tool; it is
  committed rather than generated during the build, for the reason recorded in
  `src/katana_qt/make_icons.cpp`. Warning flags in `KatanaTargetDefaults` are
  now guarded to C++, because a target's options reach every language on it
  and `windres` rejects `-Wall`.

**OUTSTANDING:** the bundle is 388 MB, nearly all of it the dependency chain of
GDAL and PDAL as MSYS2 builds them (HDF5, netCDF, Poppler, ...); trimming it
means building GDAL with fewer drivers, which is a packaging project of its
own. There is no installer unless NSIS is installed, no code signing, and no
Linux or macOS bundle.
* `tools/check_layering.cmake` - machine-enforces section 2 and Rule 4: a layer
  may only include from its allow-list, and no public header may include a
  third-party header. Runs as the `layering` CTest case.
* `.github/workflows/ci.yml` - Debug and Release tests, sanitizers, static
  analysis, layering, and a Windows MSYS2 job.

Test discovery runs through a `TEST_LAUNCHER`, so the suite does not depend on
the ambient PATH.

---

# 7. Phase 02 — Mathematical Foundation

**STATUS: DELIVERED.** `include/katana/math`, header-only, 50 tests.

* `numerics.hpp` - the single tolerance policy (section 35). `kAbsolute` 1e-12,
  `kRelative` 1e-9, `kAngular` 1e-10 rad, `kGeometric` 1e-7 model units,
  `kCoordinate` 1e-4 m. Every constant carries its justification; 1 ulp at a UTM
  northing of 1e7 is about 1.9e-9 m, which is what sets the floor.
* `vec2/3/4`, `mat2/3/4` (row-major, acting on column vectors), `quaternion`
  (Hamilton, shortest-arc `slerp`), `transform` (TRS with an analytic inverse),
  `primitives` (Plane, Ray, AABB).

`operator==` is exact and `nearlyEqual` is separate: fuzzy equality is not
transitive and must never hide inside an operator. `nearlyEqual` guards
non-finite inputs explicitly, because `inf <= eps * inf` would otherwise make
infinity compare equal to any large number.

---

# 8. Phase 03 — Basic Geometry

**STATUS: DELIVERED.** `include/katana/geometry`, 2D and 3D primitives.

`Box2`, `Line2`, `Segment2`, `Circle2`, `Arc2` (start angle plus signed sweep,
so reversal is exact), `Polyline2`, `Rectangle2`, `Triangle2`; `Segment3`,
`Triangle3` with Moller-Trumbore ray intersection. `Containment` is a tri-state
(`Outside`, `OnBoundary`, `Inside`) rather than a bool.

Area, centroid and circumcircle are computed relative to a local origin, because
a naive shoelace sum at projected coordinate magnitudes loses about six
significant digits.

---

# 9. Phase 04 — Geometry Algorithms

**STATUS: DELIVERED.** 76 tests, benchmarked in `benchmarks/bench_geometry.cpp`.

* `intersection.hpp` - one `IntersectionResult` for every pair: line/line,
  line/segment, segment/segment, line/circle, segment/circle, circle/circle,
  line/arc, segment/arc, circle/arc, arc/arc, plus symmetric overloads. Near
  tangency uses the factored `sqrt((r-d)(r+d))`.
* `polygon.hpp` - orientation, isConvex, isSimple, convexHull (monotone chain),
  simplify (Douglas-Peucker), clip (Liang-Barsky), clipPolygon
  (Sutherland-Hodgman), triangulate (ear clipping, O(n^2), documented).
* `editing.hpp` - offset, trim, extend, fillet, chamfer over
  `Curve2 = variant<Segment2, Arc2, Circle2>`.

Measured figures are recorded in `docs/geometry.md`.

---

# 10. Phase 05 — Entity System

**STATUS: DELIVERED.** `include/katana/entity`, 22 tests.

* `entity.hpp` - `Geometry` variant ordered to match `EntityType`; `PropertyMap`
  of typed values.
* `entity_geometry` - validate, boundingBox, distanceTo, transformed. A
  non-similarity transform is rejected rather than silently distorting an arc.
* `entity_database` - ids are monotonic and **never reused**, so a stale
  reference can never resolve to a different entity. `ChangeEvent` observer.
* `tables` - layer, style and property databases; layer "0" cannot be deleted.
* `serialization` - JSON, with nlohmann confined to a single translation unit.

---


## 5.2 Resolved appearance (delivered)

`katana::entity::resolveDisplay` (`include/katana/entity/display.hpp`) is the
ONE answer to "what colour is this entity, how thick is it, and what linetype
does it use". The chain is the DXF ByLayer model: entity colour overrides a
named style's, which overrides the layer's; line weight and linetype come from
the style when one is named, otherwise from the layer.

It replaced three different answers, two of which were visibly wrong:

* the 3D scene builder read the entity's own colour or a fixed default and
  ignored the layer entirely - and ByLayer is the DEFAULT for a new entity, so
  everything on a coloured layer drew grey in 3D while the 2D view showed it
  correctly;
* the 2D viewport read the entity's or the layer's and ignored the style;
* nothing at all read `Style::color`, `Style::lineWeight` or `Style::linetype`.
  The Style table was validated on write and persisted faithfully, and was
  inert - a style could not change how anything looked.

**OUTSTANDING: no command creates a Style.** The table can only be populated by
loading a project that already contains one, so styles remain unreachable from
the application even now that they take effect. A `STYLE` verb and a styles
panel belong with the Phase 09 work below.

## 5.4 Dimension styles (delivered)

`DimensionStyle`, `ArrowHead` and `DimensionStyleDatabase` in
`include/katana/entity/tables.hpp`, with `formatMeasurement` in
`src/katana_entity/dimension_text.cpp`. Lengths are MODEL units, unlike
`Layer::lineWeight` which is paper millimetres: a dimension is part of the
drawing, so its text must keep its size relative to the geometry it annotates.

The formatter is the part that has to be exactly right, because the number ends
up on a drawing somebody signs:

* **Order is scale, then round, then fix the decimals** (DIMLFAC, DIMRND,
  DIMDEC). Rounding before scaling rounds in the wrong units: 1.24 m rounded to
  0.05 is 1.25 m = 1250 mm, but scaled to millimetres FIRST it is 1240 and the
  same rounding does nothing, giving 1240 mm. A centimetre of difference from
  the order alone.
* **Ties go away from zero**, through `std::llround`, which C17 7.12.9.6-7
  specifies to round halfway cases away from zero regardless of the current
  rounding mode. This is not pedantry: measured on this toolchain,
  `printf("%.0f", 2.5)` gives 2 and `llround(2.5)` gives 3, and
  `printf("%.2f", 0.125)` gives 0.12 where the correct surveying answer is 0.13.
  `printf`, `std::to_chars` with a fixed precision and `QString::number` all
  round ties to EVEN, so none of them may be used here. A test also changes the
  rounding mode and requires the answer not to move.
* **Locale independent**: `std::to_chars` on the integer parts and hand-emitted
  digits for the fraction, so no global locale can turn 1.5 into "1,5" and make
  the drawing read as a different number.
* A text override wins VERBATIM - no prefix, no suffix, no rounding, no unit
  scale - because DXF does not apply DIMPOST to overridden text and a surveyor
  who typed a value means that value.
* Trailing-zero suppression never produces a bare ".", an empty label or "-0".

Units are a scale factor plus a suffix rather than a unit enum, because the
entity layer may only see core, math and geometry (`tools/check_layering.cmake`)
so `katana/geodesy/units.hpp` is unreachable - and duplicating it would be a
second definition of a foot.

`katana::cad::buildDimension` turns a dimension and its style into plain
geometry - extension lines, the dimension line, arrow strokes or fills, and the
placed text - and BOTH renderers draw from it, so the two views cannot disagree
about what a dimension looks like. It also returns the drawn EXTENT, which the
broad phase now culls against: `entity::boundingBox` for a Dimension covers only
the measured points and the dimension line, so culling against it dropped
dimensions whose label was still on screen.

Everything is in model units. The viewport previously drew a fixed 5-pixel tick
and a 12-pixel label, which looks right on screen and plots at whatever size the
paper happens to give it, and formatted the number with `QString::number`, which
is locale dependent.

`Layer::dimensionStyle` names a style, resolved through the same ByLayer shape
as colour: the layer's named style, else the document default. A layer naming a
style that has gone still draws, in the default - leaving a dimension off a plan
because its styling is missing would remove information from it.

Schema 5 stores the styles and the layer column. A project from before it gets
the default, which is exactly how it already behaved.

Reachable from the application: `DIMSTYLE LIST | NEW name | SET name field value
| DELETE name` with fields TEXT, GAP, EXTOFF, EXTBEYOND, ARROW, HEAD, SCALE,
DECIMALS, ROUND, PREFIX, SUFFIX and TRIM, plus `LAYER DIMSTYLE layer style`.
LIST shows what a ten-unit dimension would READ as, because that is the question
anyone setting a style is actually asking.

**OUTSTANDING:** no GUI panel - styles are set from the command line only. The
3D view draws a dimension's lines and arrows but not its TEXT, because a glyph
outline needs a font the 3D path does not have. Arrow fills are outlined rather
than filled there, for the same reason `DrawList` fills only triangles. The text
width used for centring and for the extent assumes 0.6 of the height per
character, which matches the stroke font the 2D viewport draws with and would
need revisiting for a real font.

## 5.3 Linetypes (delivered)

`Linetype`, `LinetypeElement` and `LinetypeDatabase` in
`include/katana/entity/tables.hpp`, with the DXF sign convention (AutoCAD DXF
Reference, LTYPE group code 49): a positive length is a dash, a negative one a
gap, and exactly zero a dot. `patternLength()` is the sum of the ABSOLUTE
lengths (group 40), computed rather than stored so it cannot disagree with the
elements. Patterns must begin with a dash or a dot, end with a gap and strictly
alternate; each rule has its own error.

**Dash lengths are MODEL lengths.** A 0.5 m dash is half a metre of ground at
every zoom, so a 10 km boundary carries a thousand times the dashes of a 10 m
fence. The opposite - a fixed pattern in pixels - is the easy implementation and
renders a perfectly convincing dashed line while being exactly backwards; two
property tests exist to catch it.

`katana::cad::forEachDash` walks a path in model space and emits its pen-down
spans, carrying the pattern across vertices so it runs continuously along a
polyline rather than restarting at each corner (DXF `$PLINEGEN`). It is
all-or-nothing: where the span budget would be exceeded it emits NOTHING and
the caller draws the path solid, because a truncated dashed line is a shorter
line with nothing to say so.

Below about one device pixel per element the pattern is drawn solid instead -
under that, every dash and gap falls inside one pixel and antialiasing turns the
line a uniformly paler colour, so the user sees the wrong COLOUR rather than a
pattern. That is a display decision and is deliberately NOT in `math::tolerance`.

Both renderers use it. The 2D viewport converts to a `QPen` dash array, which is
in units of PEN WIDTH rather than pixels - at the 1.5 px pen the viewport draws
with, forgetting the division makes every pattern half again too long. Every 3D
path goes through one emitter INCLUDING a plain `Segment2`, because a segment
adding its line directly would have left lines solid while polylines, arcs and
circles dashed.

Schema 4 stores definitions, with elements in their own table so their order is
the database's to keep. An older project naming a pattern that has no definition
keeps the name and draws solid, exactly as it already did; no empty definition is
invented for it, because that would shadow a real one later.

Reachable from the application: `LINETYPE LIST`, `LINETYPE NEW name dash gap
[...]` (lengths in model units, + dash, - gap, 0 dot), `LINETYPE DELETE name`,
and `LAYER LTYPE layer linetype` to attach one. Deleting a pattern a layer or
style still names is refused, and the error names the layer.

**OUTSTANDING:** no built-in patterns are shipped. `acad.lin` is in imperial
drawing units and the ISO set in millimetres, while a survey drawing here is in
metres, so any table shipped would be a conversion of numbers not to hand or an
invention - and a test would then pin the invention as though it were a standard.
Patterns are defined by the user until an importer reads real ones. `$LTSCALE`
and a per-entity scale are parameterised throughout but pinned at 1, and there
is no GUI editor.

## 5.1 Nested layers (delivered)

Layer names are `/`-separated paths: `design/surface/tin1`, `asbuilt/road/kerb`.
The tree is DERIVED from the names, not stored - see
`include/katana/entity/layer_path.hpp` for the reasoning. In short: an entity
references its layer by full path, and that string is what the database, a DXF
export and every undo record carry. If the tree were primary, renaming `design`
would have to rewrite the node, every descendant and every entity, and any of
those failing would leave entities pointing at a layer that no longer exists.
Keeping the path as the identity means the tree is a view of the names, with one
source of truth - the one already persisted. `std::map` order over full paths is
already a pre-order walk, so a subtree is a range scan.

* Adding `design/surface/tin1` creates `design` and `design/surface` as real
  layers, each with its own colour, visibility and lock. Implicit nodes that
  exist only in the tree widget would give the user a row they can see and
  cannot switch off.
* **Visibility and lock inherit down the tree; colour and linetype do not.**
  ByLayer already resolves colour, and a second inheritance rule on top would
  make the resolved colour impossible to predict.
* Descendant tests compare whole segments, so `designs/x` is not under
  `design`. A plain string-prefix test would hide or lock the wrong layers.
* `remove` refuses a layer that still has children; `removeSubtree` takes the
  branch and returns it deepest-first so undo replays it parents-first.
  `renameSubtree` moves a branch, validates every target BEFORE moving anything
  so a collision cannot leave the tree half renamed, and returns the (old, new)
  mapping so the `RenameLayer` command can move the entities in the same undo
  step.
* `.` and `..` are refused as levels: layer names reach file names whenever
  something is exported per layer, and `..` there is a path traversal.
* Shown as a `QTreeWidget` in the left dock, with new/new-child/rename/delete,
  per-node visibility and lock ticks, a colour swatch, entity counts, the
  current layer in bold, and layers switched off by an ancestor greyed.

# 11. Phase 06 — Command System

**STATUS: DELIVERED.** 31 tests.

`ChangeSet` (add / modify / remove) validated before anything is touched, then
applied atomically and reported through change events. `CommandStack` carries
undo and redo stacks plus save-point tracking, and `Transaction` groups several
commands into one undo step. About 28 entity and layer commands.

Undo restores **recorded before-images**, never a computed inverse: a computed
inverse drifts in floating point and cannot represent a failed partial apply.

---

# 12. Phase 07 — Project Storage

**STATUS: DELIVERED.** 13 tests.

SQLite behind `sqlite_database.hpp` (RAII pimpl; `synchronous=FULL`,
`journal_mode=DELETE`, `foreign_keys=ON`; the online backup API for snapshots).
`project_store` carries the application id `0x4B544E41` ("KTNA"), a schema
version, an append-only migration table, backup rotation, and `recover()`, which
restores the newest sound backup and moves the damaged file aside rather than
deleting it.

---

# 13. Phase 08 — Basic CAD Application

**STATUS: DELIVERED.** 27 unit tests plus 4 end-to-end CTest cases.

`katana_cad` holds everything an interactive session needs that is not a widget,
so all of it is tested without a display:

* `Document` - model, command stack, project, selection, current layer.
* `selection` - `isDrawn` vs `isSelectable`; `pickEntity` against drawn geometry
  (clicking inside a circle selects nothing); `pickInBox` with real window and
  crossing tests, not bounding-box overlap.
* `view_transform` - `zoomAt` keeps the point under the cursor fixed; scale
  clamped to [1e-7, 1e7]; `gridSpacing` on a 1-2-5 sequence.
* `snapping` - 8 modes with a documented resolution order.

`katana_qt` is the desktop shell and `katana_app` the headless `katana_cli`;
both drive the same objects.

---

# 14. Phase 09 — Professional 2D CAD

**STATUS: PARTIALLY DELIVERED.**

Done: `CommandInterpreter` turns one line of text into one validated, undoable
command - about 40 verbs with the usual CAD aliases, absolute / relative /
polar points, and locale-independent parsing via `std::from_chars`. Layers,
styles and entity properties are editable in the GUI, each edit a command.

Linetypes, dimension styles and hatch patterns are delivered, each as a named
table resolved through the same ByLayer chain as colour. They share one
container, `entity::NamedTable` - see `docs/model.md` for what adding another
costs and, more importantly, for which steps around it fail SILENTLY.

Hatching: `geometry::hatchLines` clips a family of parallel lines to a closed
boundary by the even-odd rule, so a concave notch is left unfilled rather than
bridged. Spacing and angle are model units and the family is anchored to the
world origin, so two adjacent parcels hatched alike line up across their shared
edge. `cad::hatchDrawing` decides when a pattern has become finer than the
screen can show and draws it as a solid tone instead - the same rule
`cad::shouldDash` applies to linetypes, and what keeps a 0.2 m pattern over a
site plan from costing thousands of clipped segments at zoom-out.

**OUTSTANDING: splines, blocks.** Neither is lost on import - the OGR DXF
driver tessellates a SPLINE to a LINESTRING and expands an INSERT inline, and a
HATCH arrives as a POLYGON, verified on a file written for the purpose. What is
missing is AUTHORING them, and the two differ in cost:

* a spline needs an eighth `entity::Geometry` alternative, which is the
  expensive kind of change - `docs/model.md` lists the places it must be
  threaded through and separates the ones the compiler catches from the ones
  that do not;
* a block needs a definition table and an instance entity, so that editing the
  definition updates every instance. Expanded-on-import geometry, which is what
  arrives today, cannot do that.

Also outstanding, found while adding the hatch table and recorded rather than
fixed: **the create / update / delete command classes are now written out three
times** (linetypes, dimension styles, hatch patterns) with the same shape and
differing only in which table they touch and what guards a delete. Collapsing
them needs a template over the table plus a per-table delete guard; it is not
the small adjacent fix CLAUDE.md section 6 allows to be done in passing, so it
is recorded here instead.

---

# 15. Phase 10 — Survey Data Model

**STATUS: DELIVERED.** `src/katana_survey/{data_model,network}.cpp`.

Stations, observations and networks as domain types, independent of any file
format or solver.

---

# 16. Phase 11 — Coordinate Systems

**STATUS: DELIVERED.** `src/katana_geodesy`, 66 tests.

PROJ confined behind `proj_internal.hpp`: `CoordinateReferenceSystem`,
`CoordinateTransformer` (reporting the operation actually selected, its accuracy
and whether it is a ballpark), `geodesic`, `grid_factors`, `units`,
`similarity_transform2d`. No PROJ type appears in a public header.

A conversion between CRSs that share a datum reports accuracy 0 rather than
"unknown", so exact is distinguishable from a ballpark operation.

---

# 17. Phase 12 — Survey Calculations

**STATUS: DELIVERED.** `angles`, `cogo`, `traverse`, `leveling`.

Bearings and azimuths, DMS parsing and formatting, inverse and forward COGO,
traverse closure and adjustment, differential levelling.

---

# 18. Phase 13 — Least-Squares Adjustment

**STATUS: DELIVERED.** `least_squares.cpp`, `lsq_core.hpp`,
`network_adjustment.cpp`, `error_propagation.cpp`, `statistics.cpp`.

A genuine normal-equation solve with weights, residuals, variance factor and
error ellipses - Eigen confined to the implementation. `statistics.cpp` computes
the chi-square CDF through the regularised lower incomplete gamma, using an
exact half-integer ln-Gamma so the 2 and 4 degree-of-freedom cases are correct
to the last place.

114 survey tests. An earlier placeholder that returned hard-coded values was
deleted; that incident is why every numeric expectation in this project must be
justified from an external definition rather than from program output.

---

# 19. Phase 14 — Terrain Engine

**STATUS: DELIVERED.** `src/katana_terrain`, 72 tests.

`tin_builder` (constrained Delaunay via CGAL, behind `cdt_backend.hpp`),
`tin_surface`, `contours`, `volume`, `tiled_terrain`, `point_merge`. Breaklines,
boundaries and holes; duplicate-point policies; bitwise-deterministic rebuilds.

---

# 20. Phase 15 — 3D Rendering

**STATUS: PARTIALLY DELIVERED.**

A complete renderer exists and is in the application; it runs on the CPU rather
than on Vulkan. The abstraction the plan asks for is in place, so the Vulkan
backend is a second implementation rather than a rewrite.

Delivered, in `katana_render` (allowed to see only core, math and geometry — it
cannot include an Entity or a Document, which is Rule 3 made structural):

* `Camera` — orbit/pan/dolly, perspective and orthographic, the ten standard
  CAD views, framing by bounding SPHERE so orbiting after a zoom-extents cannot
  push a corner off screen, pick rays that are the exact inverse of projection.
  Z-up world, screen +y down, NDC depth 0 at the near plane (the Vulkan range,
  chosen because that is the eventual backend and because a float depth buffer
  resolves far more of [0,1] than of [-1,1]).
* `DrawList` — structure-of-arrays positions/colours with indexed triangles,
  lines and points. Lighting is BAKED by the scene builder, so the rasteriser
  interpolates and nothing else.
* `Rasterizer` — three stages in the shape a tile-based GPU uses: transform
  every vertex once in parallel; clip against the near plane, project, widen
  lines into screen-space quads with square caps, and bin each primitive into
  every 64x64 tile it touches; then rasterise one task per tile, each tile
  owning its pixels exclusively so there is no lock and no false sharing.
  Perspective-correct colour, z-buffered, depth bias for wireframe over
  surfaces.
* **Reproducible (Rule 7).** The binning chunk count is a function of the
  primitive count alone, never of the core count, so a tile visits its
  primitives in the same order on every machine. Asserted by rendering the same
  scene with 0, 1, 3 and 7 workers and comparing the buffers byte for byte.
* `katana::cad::SceneBuilder` — Document and TIN surfaces to a DrawList:
  elevation and slope ramps, flat shading, breakline edges drawn heavier,
  vertical exaggeration about a datum, sagitta-based arc chording.
* `katana::cad::ViewportLayout` and the Qt `ViewportContainer` — tiled
  viewports (1, 2, 3 or 4 cells), each with its own camera and kind (Plan, 3D,
  Section, Elevation). The tiling arithmetic is headless and unit tested: every
  point belongs to exactly one cell, and pixel rectangles tile any window size
  with no seam and no overlap.

**OUTSTANDING: the GPU.** No Vulkan device, no GPU buffers, no GPU picking, no
instancing, no frustum culling beyond the per-tile bin, no LOD, no streaming.
The software renderer is interactive on the scenes it is given today but will
not reach section 32's 10M-point target; it exists so that the clipping, the
fill rule and the depth rules are pinned down by pixel-exact tests BEFORE a
driver has to reproduce them. Everything above the renderer talks to `DrawList`
and `Camera`, so the Vulkan backend does not change application code.

---

# 21. Phase 16 — 3D CAD

**STATUS: THE PLAN IS WRONG HERE. RESHAPED, NOT STARTED.**

This phase said "integrate Open CASCADE". Section 5 of this plan licenses
saying so when the plan asks for something that turns out to be a bad idea.
This is one. The verdict below was checked against the machine, not argued
from taste.

**Why not Open CASCADE:**

* **It is not a dependency, it is a second application.** Not installed here
  (`pacman -Q mingw-w64-ucrt-x86_64-opencascade` -> not found). `pacman -Si`:
  36.84 MiB to download, **261.70 MiB installed**, and it depends on **VTK,
  Tcl, Tk, FFmpeg, OpenVR, FreeImage and TBB**. Katana's entire `build/release`
  is a fraction of that. Rule 4 keeps third-party types out of public headers,
  and this would satisfy Rule 4 while still making the shipped product an
  Open CASCADE distribution with a CAD program attached.
* **It brings a second tolerance system.** OCCT carries per-shape tolerances
  that propagate through boolean operations. Section 35 and `math::tolerance`
  are the single tolerance policy. Two is a defect (CLAUDE.md section 1).
* **It brings a second parallelism system.** OCCT parallel booleans are TBB.
  `katana::core::TaskPool` exists to make parallelism *deterministic* - Rule 7.
  TBB's work stealing gives no such guarantee, so an OCCT boolean could return
  a different shape on a different machine.
* **`TopoDS_Shape` cannot live in the entity model.** It is a handle into a
  shared `TShape` graph, not a copyable value. `entity::Geometry` is a
  `std::variant` of plain values that the before-image undo copies wholesale
  and `geometryToBlob` serialises field by field. A B-rep would need its own
  ownership, its own undo and its own serialisation - see `docs/model.md`.
* **The civil work does not need it.** `katana::geometry::volume` already
  computes cut and fill between surfaces *exactly*: "Nothing is sampled: the
  results are exact up to floating-point rounding (sums are compensated)"
  (`volume.hpp:9`). A B-rep boolean would replace an exact answer with a
  tolerance-dependent one. That is a regression dressed as a feature.

**What this phase should be instead: 3D solids for civil, built on the TIN.**
The precedent is already set - `TinSurface` appears nowhere in
`entity/entity.hpp` (`grep -c` -> 0); a surface is a referenced dataset the
model names, not an eighth geometry kind. A solid follows the same shape: a
`TriangleMesh` dataset, produced by extruding a closed plan polygon between two
surfaces or two elevations, with volume from the existing exact code.

NURBS, fillet, chamfer, revolve and sweep are **removed from this plan**. They
are mechanical-CAD features. Nothing in sections 1-20 asks for them, and no
survey or civil workflow described anywhere in this document uses one. If a
real requirement for machined solids ever arrives, it arrives with a user, and
Open CASCADE can be reconsidered then - behind an optional `KATANA_BUILD_BREP`
module, the way GDAL and PDAL are already optional.

---

# 22. Phase 17 — Point Cloud Engine

**STATUS: PARTIALLY DELIVERED.**

PDAL behind `katana::pointcloud` (`src/katana_io/point_cloud_engine.cpp`); no
PDAL type appears in a public header. LAS and LAZ read and write, with crop,
classification filtering and decimation applied INSIDE the pipeline so the
points are discarded before they are ever materialised.

`readHeader()` reads only the header, so the importer sizes its decimation from
the real point count before committing to a read: opening a 400-million-point
file costs what opening a small one costs. `decimationForBudget()` rounds up, so
the result is at or under the stated budget rather than just over it.

Displayed in the 2D viewport, coloured by elevation, intensity, ASPRS
classification or the file's own RGB. Points are splatted into an image buffer
rather than drawn one QPainter call at a time, which is what keeps two million
points interactive; the per-point colours are cached and only the projection is
redone per frame.

**OUTSTANDING: the out-of-core half of this phase - but NOT as written.**
There is no level of detail and no streaming: the decimated sample is held in
memory, so a billion-point dataset is opened as a sample of it rather than
worked on in full. Sections 32 and 33 govern what that will need.

**The plan is wrong about how.** It asks Katana to build a spatial hierarchy.
It should not build one. **COPC** (Cloud Optimised Point Cloud) is a LAZ file
whose chunks are already arranged as an octree, and the PDAL already linked
into `katana_io` reads and writes it natively - verified on this machine:
`pdal --drivers` lists **`readers.copc`** and **`writers.copc`**. `readers.copc`
takes a `resolution` argument and returns only the octree levels coarse enough
to matter, so level of detail becomes a *query parameter* rather than a data
structure Katana owns, maintains and has to keep correct under edits.

**Engine half DELIVERED.** `PointCloudReadOptions::resolution`,
`PointCloudEngine::convertToCopc` and `isCopc`. A resolution asked of a file
that is not COPC is refused rather than ignored, because `readers.las` would
silently return the whole file; the destination must be named `.copc.laz`
because the extension is what makes later reads infer `readers.copc`. Tested
on a 160 000-point cloud converted inside the test, asserting the properties
of level of detail rather than one PDAL version's counts. Record in
`docs/interop.md`.

**OUTSTANDING:** nothing ASKS the query yet. Converting on import (build
once), and a viewport that re-queries at a resolution derived from the
current `worldPerPixel` - the same quantity the renderer already computes -
are the remaining work; until then the display is still one decimated sample. Building a
bespoke octree would duplicate, in Katana, a structure that is already in the
file format, already in the linked library, and already an OGC standard.

---

# 23. Phase 18 — Spatial Indexing

**STATUS: PARTIALLY DELIVERED — the 2D broad phase.**

`katana::geometry::SpatialIndex` (`include/katana/geometry/spatial_index.hpp`):
a sparse spatial hash grid over 2D bounding boxes with an overflow list for
boxes too large to bucket, giving O(1) insert, remove and update. It is a BROAD
phase: a query returns every id whose box overlaps, and the caller still runs
the exact geometric test, so swapping the scan for the index cannot change an
answer.

**Why a hash grid rather than the R-tree the phase names.** An R-tree handles
pathological size distributions better and is the textbook answer, but its
insertion, splitting and rebalancing are substantially more code, and every one
of those paths has to stay correct under the constant incremental edits a CAD
document makes. The grid is O(1) on all three, which is what keeps it honest
while the user is drawing. `oversizedCount()` is exposed so that the case where
an R-tree would win can be SEEN rather than guessed at; that is the recorded
upgrade trigger. A KD-tree or BVH was rejected for the same reason in reverse:
both are excellent for a static set and both want rebuilding after every edit.

**The subtlety worth knowing about.** The indexed box is NOT always the
geometric bounding box. Snapping an arc offers its CENTRE, which lies outside
the arc's own bounding box - far outside, for a shallow arc. Indexing plain
bounding boxes would have made the broad phase reject such an arc and centre
snap on arcs would have silently stopped working the moment the index was
enabled. The indexed box is therefore `detail::queryExtents`: the full circle
for an arc, the bounding box otherwise. Picking still tests the true bounding
box, so clicking an arc's centre does not select it. Both behaviours are
asserted.

`Document` owns an index and keeps it in step from the per-entity changes every
command reports, so a single click on a 250 000-entity drawing does not pay for
a rebuild; the whole index is rebuilt only when the model is replaced (new
document, project open), which is also what chooses the cell size from the data.

**Measured in Release** (16 x 2496 MHz), 4-vertex strings over a 1 km square:

| Entities | Snap scan | Snap indexed | Pick scan | Pick indexed | Box scan | Box indexed |
|---|---|---|---|---|---|---|
| 10 000 | 201 us | 2.3 us | 154 us | 0.97 us | 153 us | 0.042 us |
| 100 000 | 4404 us | 88 us | 3910 us | 12.6 us | 4442 us | 0.040 us |
| 250 000 | 11 277 us | 498 us | - | - | - | - |
| 500 000 | 24 252 us | 1886 us | - | - | - | - |

Box selection is now constant in drawing size. Snapping still grows with
DENSITY rather than with count, because intersection snapping is quadratic in
the candidates inside the aperture and a denser drawing puts more of them
there; that is inherent to the mode, not an index failure. The headline is that
a half-million-entity drawing went from 24 ms per mouse move - over the whole
16 ms frame budget before anything is drawn - to 1.9 ms.

Rebuilding costs 7.8 ms at 100 000 entities and 44.6 ms at 500 000, paid once
on open (about 5% of the 891 ms load).

The 2D viewport's repaint goes through the same narrowing, which matters more
than the click paths because it runs on every pan and zoom rather than on an
event: at 500 000 entities zoomed to 1% of the extent, collecting what to draw
fell from 22.3 ms to 0.090 ms (248x).

**A measured non-obvious result**, recorded because it is the kind of thing an
index is assumed not to do: asking the index for EVERYTHING is slower than
scanning. A query covering the whole drawing gathers every id, sorts them and
looks each one up again, against one ordered walk - 8.1 ms against 4.0 ms at
100 000 entities. A zoom-extents repaint is exactly that case. `forEachCandidate`
therefore picks: index below ~35% of the indexed area, scan above it, the
threshold taken from the measured crossover. Tests compare the two paths on
both sides of it.

**OUTSTANDING:** the rest of the structures the phase names, each for the
workload it suits - KD-tree for nearest-point queries over survey marks, BVH
for mesh intersection, octree for point clouds, quadtree for terrain tiles. The
terrain module already has its own uniform bucket grid inside `TinSurface`;
unifying the two is worth considering but has not been measured. The 3D scene
builder still walks every entity, which matters less because it rebuilds only
when the document changes rather than per frame.

---

# 24. Phase 19 — Performance Architecture

**STATUS: PARTIALLY DELIVERED.**

`katana::core::TaskPool` (`include/katana/core/task_pool.hpp`): a `std::jthread`
pool created once and kept warm, offering `parallelFor` and `parallelRanges`
over an index space. It drives the renderer's three stages.

The contract is that parallelism must never change a result: `body` is called
exactly once per index, concurrent calls touch disjoint state, and the visit
order is unspecified and must not matter. Tests assert identical output at 0, 1,
3 and 8 workers. A nested submission runs inline rather than waiting, so it
cannot deadlock; an exception is captured and rethrown on the submitting thread
rather than crossing a thread boundary unhandled.

Also delivered here, because both were measured rather than assumed:

* **Snapping is no longer quadratic in polyline length.** Measured in Release at
  3000 vertices: 247 ms per mouse move before, 0.196 ms after a per-segment cull
  against the cursor reach — 1260x, and now linear. An equivalence test asserts
  the cull changes no result. See `docs/cad.md`.
* **The project store is not the bottleneck it was assumed to be.** Opening a
  50 000-entity project costs 891 ms, of which 602 ms (68%) is JSON decoding of
  geometry and only 289 ms is SQLite and model rebuilding. The decision record,
  including why DuckDB is the wrong engine for this table and where it would
  earn its place, is `docs/storage.md`.

**OUTSTANDING - and the list as written is in the wrong order.** It names a
task graph, work stealing, TBB and SIMD. Rule 6 says profile before optimising,
and none of those four is where the time goes. What is measured:

`benchmarks/bench_terrain.cpp` has since been written and run - terrain was
the largest area of the codebase with no benchmark, which is why this list had
been written from the shape of the API instead. Full table in `docs/terrain.md`.
Release, 16 x 2496 MHz:

* **`compareSurfaces` was 12 150 ms, now 2 141 ms (5.7x). DELIVERED.**
  Comparing a 200 000-vertex surface with a 50 000-vertex one took 7.2x the
  worst `buildTin` measured, for the single number an earthworks job exists to
  produce. **This was the bottleneck, and nothing in the original list above
  would have touched it.** Instrumentation put 82% of it in one constrained
  triangulation of the merged overlay: 250 000 points, 749 928 constraints.
  That triangulation was unnecessary. Two triangles are convex, so their
  intersection is a convex polygon needing no new vertices, and the set of
  those intersections is the same partition the global overlay was building.
  Pairs come from `geometry::SpatialIndex`, the broad phase the codebase
  already had. The answer is unchanged - no expected value was touched, and the
  exactness `volume.hpp` promises is preserved rather than traded for a grid.
  Full record, including why the clipper's inside test must be `>= 0`, in
  `docs/terrain.md`.
* **`buildTin` is 1681 ms at 400 000 points**, 360 ms at 100 000. Second
  largest, and the one every user reaches: it runs on import and on every
  surface rebuild. Cost per point rises from 3.49 to 4.20 microseconds across
  that range, which is the n log n of incremental Delaunay insertion, not
  anything quadratic.
* A task graph solves a problem Katana does not have. There is no fan-in of
  dependent stages anywhere; the renderer's three stages are a straight line.
* Work stealing would change visit order, which is exactly what `TaskPool`'s
  contract forbids, and incremental Delaunay insertion is sequential by
  construction. It is not a missing feature; it is a rejected one, and this
  plan should have said so.
* SIMD is not where the 12 seconds is.
* `elevationAt` (499 ns) and `contours` (~45 ms fixed, 1.97x for 25x the bands)
  are already the right shape and need no work.

An earlier draft of this section claimed 657.7 ms for the 400 000-point build.
That number was never measured on this machine and is wrong by 2.6x; it is
recorded here as struck rather than silently replaced, because CLAUDE.md
section 3 forbids trusting a figure without a source and this plan was briefly
guilty of exactly that.

Latency, throughput, memory bandwidth and cache behaviour are still not
benchmarked as section 32 requires.

The win `docs/storage.md` named has since landed: geometry is stored as a
versioned little-endian blob (schema 3), taking a 50 000-entity project open
from 891 ms to 195 ms (4.6x) and a save from 445 ms to 206 ms (2.2x). Doubles
are stored by bit pattern, so the round trip is exact. Projects written before
schema 3 keep their JSON and convert on the next save.

---

# 25. Phase 20 — File Interoperability

**STATUS: PARTIALLY DELIVERED.**

`katana_interop` converts external data to and from the domain model, and is the
only layer that may see both the external readers and the entity model - which
is what keeps GDAL and PDAL out of `katana_cad`, so the core still builds with
`-DKATANA_BUILD_IO=OFF`.

Working, in the desktop application (File > Import / Export Vector, and a
Reference Data panel) and in `katana_cli` (`IMPORT`, `EXPORT`, `REFS`):

| Direction | Formats |
|---|---|
| Vector in | Shapefile, GeoJSON, GeoPackage, KML, GML, DXF, MapInfo |
| Vector out | the same, driver chosen by extension |
| Raster in | GeoTIFF, ASCII Grid, IMG, VRT, PNG/JPEG and the rest of GDAL's readers |
| Point cloud | LAS, LAZ in and out |
| 12d Archive | `.12da`, `.12daz`, `.12dz` in and out - every element of the format |

Vector import produces plain `Entity` values which the caller wraps in
`createEntities`, so an import is one validated, atomic, undoable command like
any other edit - including the layers it needs, in a single `Transaction`.
Multi-geometries are flattened to one entity per part, polygon rings become
closed polylines, and a two-point LineString becomes a Line rather than a
polyline so it can still be filleted.

Raster and point cloud import produce REFERENCE DATA instead: backdrop that is
drawn, not drawn ON. It is deliberately outside the entity model and outside
undo - putting a 400-megapixel orthophoto through before-image undo for a
visibility toggle would be absurd - and it is held by the application layer, not
the Document.

Lossy conversions are stated rather than hidden. Arcs and circles have no exact
representation in these formats, so they are written as polylines at an explicit
`curveTolerance` (the sagitta, in model units). Text and dimension entities have
no counterpart at all: they are skipped, counted and reported, never silently
dropped. A Shapefile holds one geometry type per file, so a mixed selection is
refused BEFORE anything is written rather than failing halfway and leaving a
partial file. GeoJSON always declares WGS 84, so exporting projected coordinates
to it warns that a reader will take them for degrees.

**CORRECTED: this paragraph had DXF backwards.** It said "DXF is export only".
DXF IMPORT has worked all along, through the GDAL driver - a 51 000-feature
drawing was imported in testing. DXF EXPORT is what had never worked, for two
independent reasons found by running the bundled CLI rather than by any test:
GDAL could not find `header.dxf` (it has no usable compiled-in data path on
MSYS2, and only an MSYS2 login shell sets `GDAL_DATA`), and the exporter
treated "DXF does not support arbitrary fields" as fatal. Both are fixed -
GDAL's data is now located relative to the GDAL DLL, which is right in the
build tree and in a bundle alike - and DXF import now puts each entity on its
own CAD layer instead of all 51 000 on one layer called `entities`. Record in
`docs/interop.md`.

**The 12d Archive format is delivered** (`katana_archive12d`, a module of
its own beside `commands` so that it builds and sanitizes without GDAL;
`interop/archive12d.hpp` for the file and the zip container; File > Import /
Export and `IMPORT`/`EXPORT` in the CLI). Every element the 12d Model V15
manual defines is read - the current string types, the seven superseded ones,
tins in both forms, super tins, trimeshes, LAS clouds in all eleven point
formats - and `coverage.hpp` holds the list against the reader in a test, so
"all elements accounted for" is asserted rather than claimed. Strings become
entities with their heights, attributes and everything 12d knows about them
in metadata; super alignments become named Alignments where their geometry is
PI-definable and VERIFIED against 12d's own solved vertices; tins become
surfaces; clouds become reference layers; trimeshes are read and reported
(Katana has no mesh entity). Export writes entities, alignments and the
session's surfaces back, and what was imported and exported unchanged is the
string it was. Three conventions the manual gets wrong or leaves out - the
hand of a radius, the description of a trailing transition, the definition of
the cubic parabola - were measured against 12d Model's output and are
recorded in `src/katana_archive12d/plan_geometry.hpp`; the rest of the
reasoning is in `docs/interop.md`. An adversarial review found nineteen
defects, all fixed with regression tests; 185 hostile inputs crash nothing.
12d Model 15's own 58 MB exports read in a quarter of a second.

Two things this taught the application. Surface From Drawing now reads
per-vertex heights and leaves a null vertex out of the surface instead of
triangulating it at the datum. And a headless session (`--plot`,
`--screenshot`) no longer opens a question nobody can answer: the first
scripted 12da import hung on the "far from the current drawing" box.

**OUTSTANDING: DWG, LandXML and IFC**, and **text and dimensions are skipped on
DXF export** although DXF has both (the vector path models only points, lines
and polygons). **No coordinate transformation on import**: a file's declared
CRS is reported but coordinates are never reprojected, so mixing coordinate
systems is the user's responsibility. Reference layers are not yet persisted
into the project - the pixels are re-read from the source path each session.
For the 12d archive: trimeshes and vertex symbols are read and not drawn,
super tins are reported and not built, and the test-quality lens of the
review did not complete - the module's tests were written to CLAUDE.md
section 3 but have not been audited against it by a second reader.

---

## 20.1 Where imported data lands (delivered)

`katana::interop::advisePlacement` compares the incoming bounds with the
drawing's and says so when the two cannot usefully be seen together. This is the
survey case, not a corner case: a DXF in a projected CRS carries coordinates
like (255440, 7410850) and a drawing started from scratch sits near the origin,
so the merge succeeds and the original content becomes a dot smaller than a
pixel with nothing saying why.

The criterion is the SYMPTOM, not a distance: either part is far when showing
both would shrink it below 1% of the combined diagonal. A threshold in metres
would be wrong for a site plan and wrong again for a national grid - the same
20 km gap is far apart for two 100 m drawings (0.7% of the view) and not for two
50 km ones (54%).

`IMPORT <file> LOCAL` in the CLI shifts the data so its corner meets the
drawing's, using the `VectorImportOptions::originShift` that already existed and
that nothing had ever set. Verified on a 51 417-feature DXF at
(255440, 7410850): without LOCAL the warning fires and names the separation;
with it the data lands at 0,0 to 2272,1271 beside the existing plan.

The GUI asks BEFORE anything is added: Shift Alongside / Keep Survey
Coordinates / Cancel, with the file's extent shown. Shifting is the default,
because the drawing the user is already looking at is the one they want to keep
seeing.

**OUTSTANDING:** no CRS is read from the source and no reprojection is done, so
a shift is a translation, not a transformation - two files in different
projections still will not line up. Raster and point-cloud imports get no
equivalent check.

# 26. Phase 21 — Civil and Survey Engineering

**STATUS: PARTIALLY DELIVERED — profiles and cross sections only.**

`katana::cad::extractSection` (`include/katana/cad/section.hpp`) cuts any number
of named surfaces along a plan alignment and reports elevation against station,
plus the drawing entities that cross it. `crossSectionLine` builds the
perpendicular at a station (using the bisector at a vertex, so a station landing
exactly on a bend has a defined direction), and `sectionStations` produces the
stationing without accumulating rounding.

The part that makes it engineering rather than a chart: samples are placed at
every alignment vertex, at every interval, **and at every station where the
alignment crosses a TIN triangle edge**. A fixed interval alone cuts the top off
every ridge — measured on a test roof, a 30 m interval reports a peak of 8.0
where the true ridge is 10.0, a 20% error in the highest point on the profile.
Crossings are found by bisecting between consecutive samples whose located
triangle differs; the alternative, walking the triangulation by adjacency, is
asymptotically better but is derailed by holes and by an alignment that leaves
and re-enters the surface, both of which are constant in real survey data.

Shown by `SectionViewWidget` in any tiled viewport cell: axes on round values,
station and elevation readout under the cursor, gaps drawn as gaps where the
alignment leaves the surface, slope breaks marked, crossings ticked, vertical
exaggeration.

**OUTSTANDING:** everything below - alignments with horizontal and vertical
geometry, profiles as design objects, corridors, parcels and grading.

**Where an alignment lives, decided before it is built.** An alignment is a
**named table in `entity::Model`**, beside `layers`, `linetypes` and
`dimensionStyles` - it is **NOT** an eighth `entity::Geometry` alternative.
Three reasons, in order of how expensive getting it wrong would be:

1. `docs/model.md` lists the places a new geometry kind must be threaded
   through, and separates the ones the compiler catches from the ones that
   **fail silently**. `cad::section.cpp appendCrossings` is on the silent list.
   The alignment is the thing sections are cut *along*. Adding it as a geometry
   kind would therefore silently degrade the one part of this phase that is
   already delivered and tested.
2. An alignment is not a shape with a position; it is a *definition* that a
   shape is derived from. It is referenced by name by profiles, corridors and
   cross-section sets, exactly as a TIN surface is - and `TinSurface` is
   already a named dataset rather than a geometry kind (`grep -c TinSurface
   include/katana/entity/entity.hpp` -> 0). Following that precedent costs
   nothing; inventing a second pattern costs the next contributor.
3. Undo is a before-image copy of a whole entity. An alignment edited one PI at
   a time would copy the entire horizontal geometry per keystroke.

**First slice DELIVERED: `katana::geometry::Spiral2`**
(`include/katana/geometry/spiral2.hpp`), the clothoid, in the geometry layer
beside `Arc2`. Kept in its general form - curvature from k0 to k1 over L -
because a real alignment also needs the transition between two curves and
the reverse spiral, and all three are one object. Evaluated by composite
Gauss-Legendre rather than the textbook power series, because the series is
exact only for the from-a-straight case and the general one has no closed
form; on half-radian panels the quadrature is exact to below 1e-20 relative.
Tested against the published Fresnel integrals (Abramowitz & Stegun 7.7) to
1e-14, the hand-derived series on an R = 300 m / L = 90 m transition, and an
independent adaptive-Simpson integration for the cases that have no series;
a right-hand spiral is the EXACT reflection of the left-hand one, asserted as
equality. Decision record in `docs/geometry.md`.

**Second slice DELIVERED: `katana::geometry::HorizontalAlignment` and
`solveAlignment`** (`include/katana/geometry/alignment.hpp`). An alignment is
defined by its PIs - each with a radius and spiral lengths - and the tangents,
spirals and arcs are DERIVED, so continuity holds by construction and a
definition that cannot be built is refused naming the PI. The tangent length
for unequal spirals, which the design texts do not give, was derived by
placing the shifted circle and checked by closing the chain independently in
Python (2e-14 m off the forward tangent); it reduces to the published
`T = k + (R + p) tan(D / 2)` for equal spirals and to `R tan(D / 2)` for
none. `SolvedAlignment` answers point, direction, curvature and left-offset
at a station, lists its key stations (TS, SC, CS, ST), and chords itself to
a `Polyline2` through the one sagitta rule - which is what lets a section be
cut along it with `extractSection` untouched, exactly as intended above.
Decision record in `docs/geometry.md`.

**Third slice DELIVERED: alignments in the application.** `entity::Alignment`
is a named table (the third on `NamedTable`) holding the PI definition only;
`add()` solves it, so an alignment that cannot be built is refused naming its
PI - including one arriving from a file, proved by a test that hand-edits a
saved row. Schema 7 stores `alignments` and `alignment_pis`. The `ALIGN` verb
defines, edits and prints a setting-out table that always carries the key
stations; the viewport draws every alignment as an amber overlay with a tick
and chainage label at each TS, SC, CS and ST; and Terrain > Cut Section Along
Alignment... cuts a section down one by name through the same routine the
selection path uses. `samples/site_plan` carries one. Records in
`docs/geometry.md` and `docs/cad.md`.

**Fourth slice DELIVERED: `katana::geometry::VerticalAlignment` and
`solveProfile`** (`include/katana/geometry/profile.hpp`), the design grade
line. Defined by PVIs - station, elevation, curve length - with symmetric
parabolic vertical curves, the tangents and curves derived, continuity at
every joint holding by the identity `z_PVT = z_PVI + g2 L / 2` rather than by
adjustment, and high and low points in closed form. Tested against the two
standard textbook curves, worked by hand and confirmed by direct evaluation
and a fine scan: sag low point 1020 @ 101.2, crest high point 550 @ 148.0.
Asymmetric curves are deliberately not provided. Record in
`docs/geometry.md`.

**Fifth slice DELIVERED: profiles in the application.** `Alignment::vertical`
holds the design profile as an `optional`, validated by solving so an
unbuildable one is refused naming its PVI - from the command line or from a
file. Schema 8 stores `alignment_pvis`; an alignment with no rows comes back
without a profile, proved by a test, because an empty one would fail to
solve and refuse the whole load. `ALIGN DESIGN` defines a profile in one
command (a profile cannot be grown from a single PVI, which the model
refuses - recorded in `docs/cad.md`), `PVI` appends, `PROFILE` prints it with
high and low points. `cad::appendDesignProfile` adds the design to a section
as a series sampled at every ground station plus the profile's key stations,
and Cut Section Along Alignment... passes it through, so the section view
shows design against ground with no change to the view. `samples/site_plan`
carries one: a sag whose low point the command reports at 76.226 @ 50.723.

**Sixth slice DELIVERED: corridor quantities.** `cad::corridorQuantities`
sets a template - half width, crossfall, cut and fill batters - across the
alignment at the design elevation, carries its edges to daylight by a march
and bisection against the TIN, and sums cut and fill by average end area with
the compensated sum, always sectioning the profile's key stations. A section
that cannot reach the ground contributes nothing and is counted, and the
report says the totals are not the whole job. Terrain > Corridor
Quantities... takes the assembly from a dialog and shows the schedule. Tested
against hand-worked sections checked independently in Python: 10, 11.68 and
10.75 m^2, and area-times-length exactly along a straight. Record in
`docs/cad.md`.

**Seventh slice DELIVERED: the corridor as a surface.** `cad::corridorSurface`
triangulates the template strings as breaklines with the daylight lines as
the boundary; Terrain > Corridor Surface... adds it to the scene, visible in
3D and in sections. It makes an independent check of the quantities
possible, and the tests make it: `compareSurfaces` of the corridor against
the ground (an exact overlay) and the end-area total (a different method
entirely) agree on 2000 m^3 to 1e-6 on a straight. Both commands share one
dialog. Record in `docs/cad.md`.

**Eighth slice DELIVERED: parcels.** `cad::parcelReport` gives the courses of
any closed polyline as quadrant bearings to the second and distances to the
millimetre, with area, perimeter and centroid; `legalDescription` writes the
deed wording; `parcelLabels` places text along each course, flipped so none
reads upside down, on the inside of the boundary, created in one undo step
by `PARCEL id LABEL`. Nothing is stored - a parcel is its boundary, computed
on demand. Built entirely on `katana::survey` (azimuths, bearings, DMS,
inverse, area, centroid); the one conversion from (x, y) to (northing,
easting) is in one function and the tests check bearings, not only areas,
because that is what an axis swap would break. Record in `docs/cad.md`.

**OUTSTANDING in this phase:** richer assemblies (kerbs, verges, benches,
subgrades) as a table of their own; the prismoidal correction, deliberately
not applied; grading (feature lines, pads, slopes, daylight to a surface,
grading surfaces). `deleteAlignment` still has no in-use guard because
nothing references an alignment by name.

## Alignments

```text
tangent
circular curve
spiral
stationing
offsets
```

## Profiles

```text
existing ground
design profile
vertical curves
station/elevation
```

## Corridors

```text
alignment
profile
assembly
surface
cross sections
```

## Parcels

```text
boundaries
bearings
distances
areas
labels
legal descriptions
```

## Grading

```text
feature lines
pads
slopes
daylight
grading surfaces
```

---

# 27. Phase 22 — Drawing and Plotting

**STATUS: PARTIALLY DELIVERED. Reshaped - this is the phase that makes three
already-built things honest, and the first of them now is.**

Delivered: File > Plot to PDF... The plot paints the drawing with the SAME
drawing code the screen uses, through a sheet transform in place of the
screen's (`cad/plot.hpp`: ISO 216 paper sizes, a scale ladder, the sheet as
a `ViewTransform`, a fit rule that picks the first standard scale the drawing
fits at), with every line as wide as its layer's `lineWeight` says in
millimetres of paper - which that field has claimed to mean since Phase 09
and has never meant until now. Entities, hatches (with their model-unit
patterns) and alignments are plotted; rasters and point clouds are not, by
decision. The arithmetic is tested against ISO 216 and 25.4 mm to the inch,
and the PDF itself by the `qt_plot_headless` test, which runs the
application's `--plot` switch offscreen on a copy of the sample project and
checks the file that comes out - the dialog and the switch share one code
path, so the test exercises what the menu does. Record in `docs/cad.md`.

**OUTSTANDING:** layouts and paper space (a plot is the current view on one
sheet); viewports on a sheet; title blocks; annotation scaling (text and
dimension sizes are still model units, so a 2.5 m label is 2.5 mm at 1:1000
and 0.25 mm at 1:10 000); plot styles beyond the line weight (colour-to-pen
mapping, screening); printing to a device (needs Qt PrintSupport, not yet
linked); rasters and point clouds on the sheet.

Three capabilities exist, are validated, are persisted, and are read by nothing
that draws. Each is a silent lie to the user, who sets a value and sees no
effect:

* **`Layer::lineWeight`** is documented as *millimetres on paper*. There is no
  paper. `grep lineWeight src/katana_qt/` finds nothing: the viewport draws
  every line at a hardcoded width. "Millimetres on paper" is meaningless until
  this phase defines paper.
* **`DimensionStyle::textHeight`** is in model units, which is correct, but
  annotation scaling - the thing that makes one dimension readable at 1:200 and
  at 1:2000 - has nowhere to live without a sheet scale.
* **Dimension styles themselves** (delivered above) are named "plot styles"
  in this phase's own list, which double-counts work already done.

So this phase is not "add plotting". It is: **define paper, and connect the
three settings that already claim to describe it.** A `lineWeight` that changes
nothing is worse than an absent one, because it reads as a working feature.

`dimension styles` is struck from the list below - delivered in Phase 09.

Separate the engineering model from the drawing model.

Architecture:

```text
Engineering Model
       ↓
Drawing Model
       ↓
Sheet
       ↓
Viewport
       ↓
Annotations
       ↓
Plot
```

Implement:

```text
layouts
paper space
viewports
title blocks
annotation scaling
dimension styles
plot styles
PDF output
printing
```

---

# 28. Phase 23 — C++ Application API

**STATUS: NOT STARTED. Reshaped, and roughly half the list is already built.**

The JSON command shape below duplicates `katana::cad::CommandInterpreter`,
which already parses verbs, validates arguments, returns `Result<T>` and routes
every mutation through the `Command` undo stack. A second command parser beside
it is precisely the "second way of doing something that already has a first
way" that CLAUDE.md section 6 says to collapse.

What is genuinely missing is not a command language but a **stable, typed,
header-only-facing surface** that a caller outside `src/` can link against -
which is a new `katana_api` module at the top of the layering graph, not a new
interpreter. `query entities`, `query terrain`, `run spatial queries`,
`create surfaces` and `export data` in the list below are all already reachable
in-process; they need exposing, not implementing.

The JSON example is retained below as the *wire format for a future
out-of-process caller*, not as the in-process API.

Before implementing AI, expose a stable C++ application API.

The API should support:

```text
query entities
create entities
modify entities
delete entities
execute commands
query survey data
query terrain
run calculations
run spatial queries
create surfaces
export data
```

The API must use structured commands and typed data.

Example:

```json
{
  "command": "CREATE_LINE",
  "start": [100.0, 200.0],
  "end": [300.0, 500.0]
}
```

The C++ application must validate this command.

---

# 29. Phase 24 — Python AI Layer

Python is introduced only at this stage.

Python must not implement:

```text
geometry
survey calculations
rendering
database logic
terrain algorithms
CAD algorithms
```

Python is responsible for:

```text
LLM integration
prompt construction
intent recognition
agent orchestration
AI tool selection
conversation
AI context management
```

Architecture:

```text
User
 ↓
Python AI
 ↓
Structured command
 ↓
C++ API
 ↓
Validation
 ↓
Execution
 ↓
Result
 ↓
Python AI
 ↓
User
```

---

# 30. Phase 25 — AI Agent

Implement progressively.

## Level 1 — Simple commands

Example:

```text
"Create a line between points 100 and 120."
```

AI produces:

```json
{
  "command": "CREATE_LINE",
  "start_point": 100,
  "end_point": 120
}
```

## Level 2 — Queries

Example:

```text
"Find all survey points below elevation 100."
```

## Level 3 — Multi-step workflows

Example:

```text
"Create a surface from today's survey points and generate 1-foot contours."
```

AI produces a command sequence:

```text
IMPORT_POINTS
BUILD_TIN
CREATE_CONTOURS
DISPLAY
```

## Level 4 — Engineering workflows

Example:

```text
"Calculate the cut and fill between the existing and proposed surfaces."
```

AI orchestrates the appropriate C++ commands.

---

# 31. AI Safety Model

The AI must never receive unrestricted access to the C++ process.

Use a controlled command interface.

Every AI command must pass through:

```text
Schema validation
 ↓
Permission validation
 ↓
Domain validation
 ↓
Numerical validation
 ↓
Transaction
 ↓
Execution
```

For destructive operations:

```text
AI
 ↓
Proposed command
 ↓
User confirmation
 ↓
C++ execution
```

Example:

```text
DELETE 1,842 ENTITIES
```

must not execute automatically unless the application policy explicitly permits it.

---

# 32. Performance Targets

Establish benchmarks progressively.

Initial targets:

```text
Viewport interaction:
60+ FPS

Simple selection:
< 16 ms

Simple geometry command:
< 100 ms

Undo/redo:
< 50 ms

Small project opening:
< 2 seconds
```

For large datasets:

```text
10M+ survey points:
interactive navigation

100M+ point cloud:
streamed visualization

1B+ points:
out-of-core / tiled / LOD architecture
```

These are engineering targets, not assumptions.

Benchmark on real datasets.

---

# 33. Memory Architecture

Optimize memory layout only after profiling.

Consider:

```text
SoA
AoS
memory pools
arena allocators
object pools
memory mapping
compressed storage
```

For massive point data, consider structures such as:

```text
X X X X X X
Y Y Y Y Y Y
Z Z Z Z Z Z
```

where appropriate instead of object-heavy layouts.

Avoid unnecessary heap allocations.

Measure:

```text
allocations/second
peak memory
resident memory
cache misses
memory bandwidth
```

---

# 34. Testing Strategy

Every subsystem requires tests.

## Unit tests

Test individual algorithms.

## Integration tests

Test subsystem interactions.

## Regression tests

Protect known survey/CAD results.

## Property tests

Test mathematical invariants.

Examples:

```text
distance(A,A) == 0

area(reversed polygon) == area(polygon)

transform(inverse_transform(x)) ≈ x

length(reverse(polyline)) == length(polyline)
```

## Performance tests

Track benchmark regressions.

## Real-world datasets

Maintain a test corpus containing:

```text
small surveys
large surveys
terrain
point clouds
CAD drawings
complex parcels
large civil projects
```

---

# 35. Numerical Correctness

Numerical correctness is a first-class requirement.

Never use arbitrary tolerances.

Centralize:

```text
absolute tolerance
relative tolerance
angular tolerance
geometric tolerance
coordinate tolerance
```

Document each tolerance.

Survey calculations must use appropriate numerical precision.

Do not sacrifice engineering correctness for visual performance.

---

# 36. Error Handling

Do not use silent failures.

Every subsystem should have structured error handling.

Examples:

```text
InvalidGeometry
InvalidCRS
InvalidSurveyObservation
AdjustmentFailure
TriangulationFailure
FileImportFailure
DatabaseFailure
RenderingFailure
```

Errors should contain enough information for diagnostics.

---

# 37. Logging

Implement structured logging.

Levels:

```text
TRACE
DEBUG
INFO
WARNING
ERROR
CRITICAL
```

Log:

```text
command execution
imports
exports
database operations
survey calculations
performance events
errors
```

Avoid logging sensitive project data unnecessarily.

---

# 38. Documentation

Every major subsystem must document:

```text
purpose
architecture
API
data model
numerical assumptions
threading model
ownership
performance characteristics
failure modes
```

For mathematical algorithms, document the underlying equations and assumptions.

---

# 39. Code Quality Requirements

The AI coding agent must:

* Keep functions focused
* Avoid unnecessary abstractions
* Avoid premature optimization
* Avoid global mutable state
* Use RAII
* Prefer value semantics where appropriate
* Clearly define ownership
* Minimize raw pointers
* Use smart pointers where ownership requires them
* Avoid unnecessary inheritance
* Prefer composition
* Keep dependencies directional
* Keep public APIs small

Do not generate large speculative frameworks before they are needed.

---

# 40. AI Coding Workflow

For every implementation task, follow this sequence:

```text
1. Inspect existing architecture
2. Identify affected modules
3. Define API
4. Implement smallest correct version
5. Add unit tests
6. Add integration tests if necessary
7. Compile
8. Run tests
9. Run sanitizers
10. Benchmark if performance-sensitive
11. Review dependencies
12. Document the implementation
```

Never blindly rewrite large sections of the project.

---

# 41. Definition of Done

A feature is not complete merely because it compiles.

A feature is complete when:

```text
Implementation
+
Unit tests
+
Integration tests where necessary
+
Error handling
+
Documentation
+
Performance benchmark where relevant
+
No sanitizer errors
+
No known memory leaks
+
Code review
```

---

# 42. Final Project Architecture

The intended final system is:

```text
┌─────────────────────────────────────────────┐
│              PYTHON AI LAYER                │
│                                             │
│ LLM │ Agents │ Intent │ Context │ Planning  │
└──────────────────────┬──────────────────────┘
                       │
                 C++ Application API
                       │
┌──────────────────────▼──────────────────────┐
│              COMMAND ENGINE                 │
│                                             │
│ Commands │ Transactions │ Undo │ Redo       │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│               DOMAIN MODEL                  │
│                                             │
│ CAD │ Survey │ Terrain │ Civil │ GIS       │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│             COMPUTATIONAL CORE              │
│                                             │
│ Geometry │ Geodesy │ Numerical │ Spatial    │
└──────────┬───────────┬───────────┬──────────┘
           │           │           │
        Eigen        CGAL       PROJ
           │           │           │
      Open CASCADE   GDAL       PDAL
           │
┌──────────▼──────────────────────────────────┐
│          PERFORMANCE INFRASTRUCTURE         │
│                                             │
│ Threads │ SIMD │ Memory │ Cache │ GPU       │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│               VULKAN ENGINE                 │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│                  QT UI                      │
└─────────────────────────────────────────────┘
```

---

# 43. Required Development Order

The AI coding agent must maintain this order unless a documented architectural reason requires deviation:

```text
01. Build system
02. Testing infrastructure
03. Math
04. Basic geometry
05. Geometry algorithms
06. Entity system
07. Command system
08. Persistence
09. Basic 2D CAD
10. Professional drafting
11. Survey model
12. CRS/geodesy
13. Survey calculations
14. Least-squares adjustment
15. Terrain
16. 3D renderer
17. 3D CAD
18. Point clouds
19. Spatial indexing
20. Parallel computation
21. Performance optimization
22. File interoperability
23. Civil engineering
24. Drawing/plotting
25. C++ application API
26. Python AI
27. AI agent
28. Production hardening
```

# 44. Core Principle

The final product should **not** be an AI wrapped around a CAD program.

It should be:

```text
A high-performance deterministic C++ engineering platform
                    +
             A controlled AI interface
```

The C++ engine must be capable of performing every important engineering operation independently.

AI should provide:

```text
natural-language interaction
automation
workflow generation
data querying
command composition
documentation assistance
```

The AI must never replace the deterministic geometry, surveying, numerical, or engineering systems.

The long-term objective is therefore:

```text
                 AI
                  │
                  ▼
        ┌───────────────────┐
        │ C++ Command Layer │
        └─────────┬─────────┘
                  │
                  ▼
        ┌───────────────────┐
        │ Engineering Core  │
        └─────────┬─────────┘
                  │
        ┌─────────┼──────────┐
        ▼         ▼          ▼
       CAD      Survey     Terrain
        │         │          │
        └─────────┼──────────┘
                  ▼
             High-Speed
             C++ Engine
                  │
                  ▼
              GPU / UI
```

**Build the deterministic C++ engine first. Add intelligence only after the engine exposes reliable, structured commands.**
