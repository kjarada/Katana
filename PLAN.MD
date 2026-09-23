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


* C++26 (the build variable `KATANA_CXX_STANDARD`; see "The language standard is a build variable" in section 24)
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
| 20 | File Interoperability | partial — raster, vector, point cloud and 12d Archive import/export in GUI and CLI; no DWG/LandXML/IFC. 20.2 (12d programme) delivered bar property definitions; 20.3 (survey coding) delivered bar the library browser |
| 21 | Civil Engineering | partial — sections; clothoid; alignments and profiles; corridor quantities and surface, cross-checked; parcels with bearings, area, deed wording and labels; grading engine without a GUI route; richer assemblies outstanding |
| 22 | Drawing/Plotting | partial — Plot to PDF through the screen's own drawing code at ISO sizes and standard scales, `Layer::lineWeight` finally honoured; layouts, title blocks, annotation scaling, printing outstanding |
| 23 | C++ Application API | reshaped — expose the existing interpreter, do not write a second one |
| 24 | Python AI | not started |
| 25 | AI Agent | not started |
| 26 | Production Hardening | not started — no section of its own yet; section 43 lists it last |
| 45 | Survey module and instrument interoperability | in progress — model, detection and the cad bridge delivered; parsers, reduction, Survey menu, wizard and Point Manager in progress |
| 46 | The audit of 2026-09-23 — defect register | open — 133 of 142 confirmed defects outstanding, 4 of them high |

The delivered phases below are recorded rather than specified: their
requirements have been met, so only what was built and where it lives is kept.
Section numbers are deliberately NOT renumbered — source comments cite them
(`PLAN.MD section 32`, `Rule 4`), and renumbering would silently invalidate
every such reference. A test count in a phase's record is the count when
that phase was delivered, not now: the suite's current total is in README.md,
and the counts here drifted from it for months before the audit of 2026-09-23
said so.

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

**The order of work is the one `CLAUDE.md` section 5.2 states, and it is
stated there only.** This paragraph used to say that 20.2 was the only work
until delivered; the user's later instruction put 20.3 before it, and section
45 (requested 2026-09-23) after 20.3 and before 20.2's remainder, so three
documents had come to give three orderings. As of 2026-09-23: 20.3's
remainder, then section 45, then 20.2's remainder, then the earliest
undelivered phase, with real defects found on the way fixed when small and
adjacent.

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

CMake 3.30+ / Ninja / C++26, one target per module. (This line said 3.24 and
C++20. The project has been C++26 since 6095a19, and `cxx_std_26` is a compile
feature CMake first knows in 3.30; TEST_LAUNCHER honoured by
gtest_discover_tests needs 3.29 and preset schema 6 needs 3.25.) `-Wall -Wextra -Wpedantic
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
  analysis, layering, and a Windows MSYS2 job. **CORRECTED (2026-09-23): this
  file was DELETED** in 759c115, a Qt bug fix whose message does not mention it,
  and no CI had run since; `.github/lsan.supp` was left orphaned and later
  commits still justified designs by "the sanitizer job". Being restored.

Test discovery runs through a `TEST_LAUNCHER`, so the suite does not depend on
the ambient PATH.

---

# 7. Phase 02 — Mathematical Foundation

**STATUS: DELIVERED.** `include/katana/math`, header-only, 50 tests at delivery.

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

**STATUS: DELIVERED.** 76 tests at delivery, benchmarked in `benchmarks/bench_geometry.cpp`.

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

**STATUS: DELIVERED.** `include/katana/entity`, 22 tests at delivery.

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

**Styles are created and edited** (this paragraph used to say no command
created one): `commands::createStyle / updateStyle / deleteStyle / renameStyle`
in `table_commands.cpp`, the `STYLE` verb, and Edit > Styles and Linetypes...,
all delivered with 20.2 slices 1 and 3.

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
and a per-entity scale are parameterised throughout but pinned at 1. (The GUI
editor this used to say was missing is Edit > Styles and Linetypes..., 20.2
slice 3.)

## 5.1 Nested layers (delivered)

Layer names are `/`-separated paths: `design/surface/tin1`, `asbuilt/road/kerb`.
The tree is DERIVED from the names, not stored - see
`include/katana/entity/layer_path.hpp` for the reasoning. In short: an entity
references its layer by full path, and that string is what the database, a DXF
export and every undo record carry. If the tree were primary, renaming `design`
would have to rewrite the node, every descendant and every entity, and any of
those failing would leave entities pointing at a layer that no longer exists.
Keeping the path as the identity means the tree is a view of the names, with one
source of truth - the one already persisted. `std::map` order over full paths
puts every parent before its children and makes a layer's descendants one
contiguous range (`name/` to `name0`), so a subtree is a range scan.
CORRECTED 2026-09-23: this said the order was a pre-order walk, and
`hasChildren` believed it - it tested only the key after the layer. But space,
`-` and `.` sort below `/`, so "design 2" comes between "design" and
"design/surface"; with both present "design" had no children and `remove`
orphaned the branch. Found while fixing the inheritance below; regression
tested.

* Adding `design/surface/tin1` creates `design` and `design/surface` as real
  layers, each with its own colour, visibility and lock. Implicit nodes that
  exist only in the tree widget would give the user a row they can see and
  cannot switch off.
* **Visibility and lock inherit down the tree; colour and linetype do not.**
  ByLayer already resolves colour, and a second inheritance rule on top would
  make the resolved colour impossible to predict. CORRECTED 2026-09-23 (audit
  MOD-01/02/03, REN-03/04): until then only the layer panel honoured the
  inheritance. The viewport, picking, box selection, snapping, the plot, the 3D
  scene, sections and Surface From Drawing looked at the entity's own layer, and
  the commands' lock check at its own lock, so a line on
  `design/surface/tin1` stayed drawn under a hidden `design` and SELECT ALL,
  ERASE erased it under a locked one. Now `LayerDatabase::resolve` answers
  shown/locked with inheritance in one lookup, from state the table keeps
  current on every change (only the changed subtree is recomputed), and
  `cad::isDrawn` / `isSelectable` and `requireUnlockedLayer` are built on it;
  a refusal names the ancestor holding the lock.
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

**STATUS: DELIVERED.** 31 tests at delivery.

`ChangeSet` (add / modify / remove) validated before anything is touched, then
applied atomically and reported through change events. `CommandStack` carries
undo and redo stacks plus save-point tracking, and `Transaction` groups several
commands into one undo step. About 28 entity and layer commands.

Undo restores **recorded before-images**, never a computed inverse: a computed
inverse drifts in floating point and cannot represent a failed partial apply.
CORRECTED 2026-09-23: not quite never - creating and renaming a layer are
undone by computed inverses, which leave behind the ancestor layers the command
created implicitly, and so does a Transaction's rollback (audit MOD-07, open).

---

# 12. Phase 07 — Project Storage

**STATUS: DELIVERED.** 13 tests at delivery.

SQLite behind `sqlite_database.hpp` (RAII pimpl; `synchronous=FULL`,
`journal_mode=DELETE`, `foreign_keys=ON`; the online backup API for snapshots).
`project_store` carries the application id `0x4B544E41` ("KTNA"), a schema
version, an append-only migration table, backup rotation, and `recover()`, which
restores the newest sound backup and moves the damaged file aside rather than
deleting it.

---

# 13. Phase 08 — Basic CAD Application

**STATUS: DELIVERED.** 27 unit tests plus 4 end-to-end CTest cases at delivery.

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

**FIXED since it was recorded here:** the create / update / delete command
classes that were written out three times are one template set over a
`TablePolicy<T>` in `table_commands.cpp`, done in 20.2 slice 1 and used for
linetypes, dimension styles, hatch patterns, alignments and styles.

---

# 15. Phase 10 — Survey Data Model

**STATUS: DELIVERED.** `src/katana_survey/{data_model,network}.cpp`.

Stations, observations and networks as domain types, independent of any file
format or solver.

---

# 16. Phase 11 — Coordinate Systems

**STATUS: DELIVERED.** `src/katana_geodesy`, 66 tests at delivery.

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

A genuine weighted least-squares solve with residuals, variance factor and
error ellipses - Eigen confined to the implementation. The normal equations
are deliberately NOT formed: the solve is a column-equilibrated,
column-pivoted Householder QR, with the cofactor matrix from triangular
solves, which gives the same estimate without squaring the condition number
(this line used to say "normal-equation solve", describing the method the code
rejects). `statistics.cpp` computes
the chi-square CDF through the regularised lower incomplete gamma, using an
exact half-integer ln-Gamma so the 2 and 4 degree-of-freedom cases are correct
to the last place.

114 survey tests. An earlier placeholder that returned hard-coded values was
deleted; that incident is why every numeric expectation in this project must be
justified from an external definition rather than from program output.

**CORRECTED (2026-09-23): the network adjustments had NO tests.**
`adjustHorizontalNetwork`, `adjustLevelNetwork`, `adjustTraverse`,
`buildTraverseNetwork` and both `applyAdjustment` overloads - about 900 lines
of `network_adjustment.cpp` - were referenced by no file under `tests/`, and
never had been (`git log` finds no deleted test). The 114 tests above cover the
dense least-squares core, the traverse computation, levelling runs and the
statistics, not the network solve built on them. `tests/survey/
test_network_adjustment.cpp` now exists with a hand-worked level network.
OUTSTANDING: the horizontal and traverse adjustments still need tests against
published worked examples (Ghilani, *Adjustment Computations*, is the standard
source), with the error ellipses and the global test among what they assert.

---

# 19. Phase 14 — Terrain Engine

**STATUS: DELIVERED.** `src/katana_terrain` (72 tests when this was written;
see the suite for the count now).

`tin_builder` (constrained Delaunay via CGAL, behind `cdt_backend.hpp`),
`tin_surface`, `contours`, `volume`, `tiled_terrain`, `point_merge`, and
`super_surface` (the combined surface of 20.2 slice 8). Breaklines, boundaries
and holes; duplicate-point policies; bitwise-deterministic rebuilds.

OUTSTANDING (stated in `tiled_terrain.hpp` and missing from this record until
the audit of 2026-09-23): the TILED terrain refuses breaklines, boundaries and
holes - `create()` fails with Unsupported rather than building a surface that
ignores them - and keeps every point in memory; out-of-core tiles are not
built.

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
  chosen because that is the eventual backend). CORRECTED 2026-09-23: the
  second reason given here - "a float depth buffer resolves far more of [0,1]
  than of [-1,1]" - holds only with REVERSED Z (near at 1). With near at 0,
  as built, the far scene lies just below 1.0 at a fixed 2^-24 spacing, and a
  framed 500 m site gets depth steps of 0.8-3.75 m (audit REN-02, open).
* `DrawList` — structure-of-arrays positions/colours with indexed triangles,
  lines and points. Lighting is BAKED by the scene builder, so the rasteriser
  interpolates and nothing else.
* `Rasterizer` — three stages in the shape a tile-based GPU uses: transform
  every vertex once in parallel and classify it against the clip planes; clip,
  project, widen lines into screen-space quads with square caps, and bin each
  primitive into every 64x64 tile it touches; then rasterise one task per
  tile, each tile owning its pixels exclusively so there is no lock and no
  false sharing. Perspective-correct colour, z-buffered, depth bias for
  wireframe over surfaces.
* **Clipping - CORRECTED 2026-09-23.** Clipping is against the true near plane
  (clip z >= 0, i.e. w >= near) and a guard band of twice the viewport, with
  the cut computed in double (Sutherland-Hodgman for triangles, Liang-Barsky
  for lines), and every float-to-int conversion is clamped first. It used to
  clip at w > 1e-6, which kept the divide finite and nothing else: the cut
  vertex projected beyond INT_MAX, the conversion in the binning was undefined
  (INT_MIN on x86-64), and a triangle or line crossing the eye plane was
  dropped whole, visible part included, in any view taller than about 213 px -
  the ground under a perspective eye went missing. Found by the audit (REN-01); the
  regression tests at 400x400 and 1920x1080 fail on the old code. The reasons
  for the band's width and for double are recorded at `kGuardBand` in
  `rasterizer.cpp`; the cost, none measurable, is in `docs/performance.md`.
  No sanitizer could have caught it: GCC's `-fsanitize=undefined` leaves out
  `float-cast-overflow`, which `cmake/KatanaSanitizers.cmake` now names - and
  a MinGW trap-mode UBSan build of the old code dies on it with an illegal
  instruction in the new tests.
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
driver has to reproduce them. The 3D and Elevation cells talk to `DrawList`
and `Camera` only, so a Vulkan backend does not change them. (This said
"everything above the renderer": the plan viewport and the section view paint
with QPainter and never touch a DrawList, so a GPU plan view is its own work.)

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
on open (about 5% of the 891 ms load as it was then; opening has since become
several times faster - section 24 - so the share is larger now).

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

## The optimisation pass of 2026-09-23 — DELIVERED

Five hot paths, analysed and changed at once by parallel agents on disjoint
directories (`CLAUDE.md` §5.3), then re-measured centrally. Full tables, method
and rejected findings in `docs/performance.md`; the reasoning lives beside the
code it governs in `docs/terrain.md`, `docs/geometry.md`, `docs/storage.md` and
`docs/interop.md`. Release, interleaved A/B on an idle machine:

* **`combineSurfaces` was 30 984 ms, now 248 ms (125x).** A 12d super tin of a
  diagonal corridor over a 200 000-point base took **thirty-one seconds**. Every
  triangle of a lower member scanned every vertex of every higher member; the
  vertices now go into a bucket grid built once per member. The output is
  identical - 402 719 triangles either way - and a test asserts that equality
  rather than inferring it from the timing.
* **`TiledTerrain::buildAll` 184 ms to 34 ms (5.3x)** and **`elevationsAt` 17.9
  ms to 2.5 ms (5.9x)**, both over `TaskPool`, both asserted bit-identical to
  the serial path. `buildAll` now builds every tile even when one fails and
  returns the lowest-index failure; that is a behaviour change and it is stated
  in the header.
* **`isSimple` 3.19 ms to 0.80 ms at 512 vertices (3.9x)**, from a bounding-box
  reject whose margin is *derived* from `intersect()`'s own acceptance rule
  rather than chosen. **`Rectangle2::closestPoint` 2.8x**, from a stack array
  instead of a heap vector of four corners.
* **Opening a 50 000-entity project: 184 ms to 170 ms (-7.6%)**, and creating
  27 000 entities **2.0x** faster on 61% fewer allocations. Three separate
  copies of every entity were removed - `ChangeSetCommand` built its change set
  twice, kept an image of every created entity that `remove()` already returns,
  and `applyToModel` duplicated the whole project on its way into the model.
* **Reading a 62.8 MB 12da archive: ~1.96 s to ~0.42 s (4.7x)**, by not building
  strings to throw away. Proved byte-identical on 21 archives including nine
  malformed ones. **This is the one figure here that was not re-measured
  centrally** - the archives are synthetic and not in the repository - and it is
  recorded as the implementing agent's measurement.

**A benchmark that lied, recorded because it is the useful part.** The first
`BM_CombineSurfaces` used an axis-aligned band and showed *no difference at
all*. `overlaps()` rejects by bounding box, then returns on the first covered
sample point, and only then scans vertices - so an axis-aligned band never
reaches the code that was changed. A diagonal band has a bounding box covering
the whole site while covering only a strip, which is what a road corridor or a
watercourse actually looks like. Both shapes are kept. The agents' own report
claimed 5.9x for this work from a harness with the same flaw; the real figure
is twenty times larger.

**OUTSTANDING.** Five of these optimisations had **no benchmark in the
repository** when they were made - they were measured in scratch harnesses that
are now gone, which `CLAUDE.md` §4 exists to prevent. They have benchmarks now
(`BM_CombineSurfaces`, `BM_ElevationsAt`, `BM_TiledTerrainBuildAll`,
`BM_IsSimple`, `BM_ClipPolygon`, `BM_RectangleClosestPoint`). Still missing: a
benchmark for a 12da read, and one for scene building and the per-frame draw
path, which is why the viewport work of 2026-09-22 quotes no figure.
`Importer::tallyFor` (`domain_import.cpp`) is a linear scan per element, the
third instance of that shape, worth about 1% of an import and left alone.

## The language standard is a build variable

The project builds as **C++26** (`KATANA_CXX_STANDARD`, default 26; GCC 16.2
accepts `-std=c++26`, `__cplusplus 202400L`, and the whole suite passes on it).
A C++23 build is one cache variable away, and exists so that a claim about the
standard can be **measured** rather than argued about:

```sh
cmake -S . -B build/rel23 -G Ninja -DCMAKE_BUILD_TYPE=Release -DKATANA_CXX_STANDARD=23
```

**No figure is claimed for C++23 versus C++26.** Two attempts to produce one
were unsound - the first was Windows clock quantisation reported as a result,
the second compared two sequential runs eight minutes apart on a laptop and
would have attributed thermal drift to the language. The A/A control that would
settle it was not run. `docs/performance.md` records both attempts so that the
next person starts from the control rather than from the A/B.


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
| 12d Archive | `.12da`, `.12daz` in and out - every element of the format (`.12dz` was withdrawn in 20.2 slice 1) |

Vector import produces plain `Entity` values which the caller wraps in
`createEntities`, so an import is one validated, atomic, undoable command like
any other edit - including the layers it needs, in a single `Transaction`.
Multi-geometries are flattened to one entity per part, polygon rings become
closed polylines, and a two-point LineString becomes a Line rather than a
polyline so it can still be filleted. A file's Z becomes the entity's heights
(the `elevation` / `elevations` properties) and goes back out as Z; a 2D
feature has no height, not a height of 0. CORRECTED 2026-09-23 (audit IO-01):
import dropped Z and export wrote Z = 0 until then; what a format cannot carry
is reported, and the rules are in `docs/interop.md`, "Conversion".

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
surfaces; clouds become reference layers; trimeshes become session meshes
(20.2 slice 4). Export writes entities, alignments and the
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
For the 12d archive (this list was stale until 2026-09-23: meshes are
imported and drawn, super tins are built, point symbols are drawn - 20.2
slices 2, 4 and 8): a per-vertex symbol list that differs along a line is kept
and not drawn, and the module's tests were audited by a second reader on
2026-09-23 - its findings are in the register of section 46 (A12-nn).

---

## 20.1 Where imported data lands (delivered)

`katana::interop::advisePlacement` compares the incoming bounds with the
drawing's and says so when the two cannot usefully be seen together. This is the
survey case, not a corner case: a DXF in a projected CRS carries coordinates
like (255440, 7410850) and a drawing started from scratch sits near the origin,
so the merge succeeds and the original content becomes a dot smaller than a
pixel with nothing saying why.

The criterion is the SYMPTOM, not a distance: a part is lost when showing both
would shrink it below 1% of the combined diagonal. A threshold in metres would
be wrong for a site plan and wrong again for a national grid - the same 20 km
gap is far apart for two 100 m drawings (0.7% of the view) and not for two
50 km ones (54%). CORRECTED 2026-09-23 (audit IO-02): the loss must be the
placement's doing, so a part counts only if it would plainly be visible beside
the other with the gap closed (over 2% of the other's diagonal), and a part with
no extent never counts. The old rule reported correctly placed data that was
merely small - one control point, a 10 m detail inside a 2 km site - and the
default button moved it to the drawing's corner. A far-off single point still
reports the drawing it would hide.

`IMPORT <file> LOCAL` in the CLI shifts the data so its corner lands at the
ORIGIN (this said "meets the drawing's"; for rasters and point clouds the verb
is broken outright, audit QT-13/QT-14, open), using the
`VectorImportOptions::originShift` that already existed and
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

## 20.2 The 12d Model programme — TAKES PRECEDENCE OVER EVERY OTHER PHASE

**Instruction (2026-09-22, from the user): nothing in any other phase of this
plan is to be started or resumed until every slice below is DELIVERED.** The
grading command, the plotting phases, the point-cloud work and the rest wait.
`CLAUDE.md` section 5.2 carries the same instruction. The one exception is a
defect that blocks a slice here.

12d Model is what Katana's users exchange work with. Section 25 delivered the
archive format - every element read, written and mapped. This programme
finishes the JOB: everything a 12d archive carries has a home in Katana, can
be seen, managed and edited, and goes back out as it came in. The measure is
the fourteen real archives in the user's `kjarada.github.io/examples/test-files`
(4 to 58 MB, written by 12d Model 15), which carry among them 270 distinct
linestyle names, 109 colour names, 7 text styles, 28 479 vertex symbols,
1 719 trimeshes, seven kinds of attribute block and every string type.

There are only two container forms: `.12da` and `.12daz`. There is no
`.12dz`; the extension was accepted in error and is withdrawn in slice 1.

### Slices, in order

1. **Names become tables.** A 12d linestyle name becomes a Katana `Style`
   (created by the import, continuous unless known) and the entity carries it
   as its style - not as metadata. A 12d text style becomes a Katana style
   too. Colours: the standard names as now; every other name (`sui
   electricity`, `shade 48`, `pen 025a`) is kept as the style's description so
   that nothing is anonymous. `.12dz` withdrawn. Export writes the names back
   from the tables.
2. **Symbols.** A `Symbol` is a named marker - a shape from a fixed set
   (circle, square, triangle, cross, diamond, tick, tree, pole, manhole ...)
   at a size, in a colour or ByLayer - held in the `Style` table so that "the
   style of a point" and "the style of a line" are one thing, as they are in
   12d. A point entity draws its style's symbol in plan; `symbol_value` and
   `symbol_data` on import set it; export writes them back. A **Symbols
   manager** lists, creates, edits and previews them.
3. **Line styles manager.** One dialog for the `Style` and `Linetype` tables
   together: name, linetype (with pattern editing and a preview), line
   weight, colour, hatch, symbol; new, edit, rename, delete with the
   in-use guard; assign to the selection or to a layer.
4. **Trimeshes.** A `SceneMesh` - name, vertices, triangular faces, colour,
   per-face colours where the file has them - drawn in the 3D view like a
   surface and as its outline in plan, held with the surfaces in the session
   (persistence of session data is a separate, later question for surfaces
   and meshes alike). Import creates one per `primitive_3d`; export writes
   `primitive_3d` back with its infos and flags.
5. **Attribute manager.** A dialog on the selection showing every property
   as the tree 12d had it (`Group/Sub/Name`, `vertex/3/Name`), typed, with
   add, edit, remove and rename, each an undoable command; property
   definitions (the schema) managed in the same place. Text, integer and real
   attributes as themselves; groups as branches.
6. **Layer manager.** A dialog with the whole layer table - visible, locked,
   colour, linetype, line weight, dimension style, hatch, entity count - with
   new, new child, rename, delete, move, and assign-to-selection; the dock
   stays as the quick view.
7. **Text as 12d draws it.** Justification, offset, raise and slant applied
   on import so the text sits where 12d put it; text style as a Katana style;
   width factor and the rest kept and written back.
8. **Super tins built, and referenced clouds found.** A super tin becomes a
   surface in which each member overrides the members before it within its
   footprint; a `ref_data` cloud is looked for beside the archive and imported
   when found.
9. **The round trip, on the real archives.** Import each of the fourteen
   archives, export it, import the export: every element kind present in the
   same numbers, every string with the same vertices, heights, style, colour,
   attributes and symbols, every mesh with the same faces. A test that reads
   the small fixtures does this in ctest; a script does it on the large ones
   and its results are recorded in `docs/interop.md`.

Each slice: tests first, engine, application, docs, commit. The status line
below is updated as each lands.

**Slice 1 DELIVERED (2026-09-22).** A 12d linestyle is a Katana `Style`
the entity carries in its own `style` field; the importer lists the styles
it needs (`stylesNeeded`) and the CLI and the desktop application create the
missing ones in the import's transaction, beside the layers. `Test 4 without
tin.12daz` (27 177 entities) arrives with its 98 linestyles as styles. The
`Style` table gained `description`, `symbol` and `symbolSize` (schema 9,
defaults reproducing every older project), a free `validate`, and its
create/update/delete commands - by collapsing the four hand-written copies
of the table command trio into one template with a policy per table
(`table_commands.cpp`), which was the "second way of doing something" that
CLAUDE.md section 6 says to remove when met. `STYLE LIST | SYMBOLS | NEW |
SET | DELETE | APPLY` in the interpreter. `.12dz` withdrawn. Colour names
Katana has no RGB for stay on the entity as `12d.colour` rather than on the
style, because a 12d colour belongs to the string, not to its linestyle.

**Slice 2 PARTIALLY DELIVERED (2026-09-22) - the engine and the drawing;
the manager dialog is folded into slice 3.** `cad::symbolStrokes` is the
one definition of the sixteen shapes (`include/katana/cad/symbols.hpp`);
`resolveDisplay` carries the style's symbol and size; the plan viewport and
the PDF plot draw a point's symbol from its style (`ViewportWidget::
drawSymbol`, size = width as 12d's is, 0 = the plain mark). A 12d
`symbol_value`/`symbol_data` on a point makes the point's style the
symbol's linestyle, with the shape read from the name
(`archive12d::symbolForLinestyle`: manhole, pole, tree, target, flag, star,
diamond, dot, cross, else circle) and the size from the block; the symbol's
colour is the point's; `12d.string_style` keeps the string's own linestyle;
only non-default rotation/offset/raise, a non-standard colour and a size
unlike the style's become `12d.symbol.*`. A line's per-vertex blocks are
kept as one list per key and written back as `symbol_value` or
`symbol_data`. `Test 4 without tin` arrives with 113 of its 211 styles as
symbols; the fixture `tests/archive12d/data/symbols.12da` is drawn by
`qt_import_symbols_headless`. The rest of the work is in `docs/cad.md`
(Point symbols) and `docs/interop.md` (Symbols are on the style).
**Decision:** the "Symbols manager" this slice asked for and the "Line
styles manager" of slice 3 are ONE dialog, because a symbol is a field of a
Style and a second dialog on the same table would be the second way of doing
something; it is delivered under slice 3. OUTSTANDING: that dialog (slice
3); a symbol on the vertices of a line is kept, counted and not drawn; a
point symbol's rotation is kept and not drawn.

**Slice 3 DELIVERED (2026-09-22).** One dialog for both tables -
`src/katana_qt/style_manager.cpp`, Edit > Styles and Linetypes... - listing
every style (linetype, weight, colour, hatch, symbol and its size,
description) and every linetype (pattern, period, description), with a form
below and a preview that draws the sample through the SAME
`cad::qtDashPattern` and `cad::symbolStrokes` the viewport and the plot use,
so a preview that looks right is evidence and not a second opinion. New,
rename, delete, save changes, apply to the selection, and assign a linetype
to a layer; everything through the ordinary commands, so a change made here
is on the same undo stack as one typed on the command line.

The new engine piece is `renameStyle` / `renameLinetype`
(`table_commands.cpp`): a rename is a move in a name-keyed table, so it is
remove + add + repoint every holder - entities for a style, layers and
styles for a linetype - as ONE undo step. A protected item ("continuous")
may not be renamed away for the same reason it may not be deleted. The
rename ENDS by asking the delete guard whether anything still names the old
item, so the two readings of the same references police each other rather
than drifting. `STYLE RENAME` and `LINETYPE RENAME` expose it without the
GUI, and `STYLE NEW/DELETE/APPLY/RENAME` now refuse trailing arguments
instead of silently dropping them - `STYLE NEW TOPO Natural Surface Point`
used to make a style called "TOPO".

The table is read-only and edited through the form: a table that edited in
place would run a command from inside its own `itemChanged` signal, which is
exactly the shape of the layer-panel crash in `docs/cad.md`.
`qt_style_manager_headless` builds and paints the dialog over a 12d import.
OUTSTANDING: the dialog's buttons are driven by hand - the commands beneath
them are covered by tests, the dialog itself only by "it builds and paints".

**Slice 4 DELIVERED (2026-09-22).** A 12d `primitive_3d` becomes a
`geometry::TriangleMesh` - vertices and faces, deliberately NOT a
`TinSurface`, because a surface is single-valued in x and y and a mesh may
be closed or overhang, and keeping them separate types is what stops a pipe
being asked for "the level at this station". It is held in the session
beside the surfaces (`MainWindow::addMesh`), drawn in the 3D view like a
surface, and in plan as its FOOTPRINT - the convex hull of its vertices,
which is exact and cheap for any topology, dashed and lightly filled under
the drawing. Per-face colours survive both ways: the one-based
`face_flags` into `face_infos` on the way in, one info per distinct colour
on the way out. `Test 4`'s Windsor Road archive brings 1 453 meshes of
90 656 triangles in 0.11 s; `test trimishes complex.12da` brings 266 of
126 536.

Two decisions recorded where they apply. A mesh defaults to `Shaded` and
not `ShadedWithEdges`: it has no neighbour table, so every edge of every
face is drawn and each line costs two triangles - measured in Release,
886 504 triangles at 24.9 ms with edges against 127 288 at 18.0 ms without
(`include/katana/cad/scene.hpp`). And the plan view draws the footprint
rather than the triangles, because 1 453 meshes of 90 656 triangles would
bury the drawing they are context for.

OUTSTANDING: a mesh is session data, so it is not saved with the project
(the same open question as surfaces - PLAN.MD Phase 07); vertex and edge
infos, the edge list and `blend` are read and not modelled; the Reference
Data panel does not list meshes, so there is no way to hide or delete one
from the GUI yet.

**Slice 5 PARTIALLY DELIVERED (2026-09-22).** Edit > Attributes... (Ctrl+1)
shows the selection's properties as the TREE 12d had them - the importer
flattens `group { name "Asset" ... }` to "Asset/Dimensions/Size" and this
puts the branches back - with each value's type, and Add, Save Value,
Rename and Remove as ordinary commands on the one undo stack. It acts on
the whole selection: a value the selected entities do not share, or a
property only some of them carry, shows as `<varies>` rather than as one of
them, because a manager that showed the first entity's value and wrote it to
all of them would quietly overwrite the rest.

The engine gained `renameEntityProperty` (keeps the value and its type,
refuses a name the entity already has - the two values differ and keeping
one would lose the other) and `entity::toString`/`typeName`, ONE definition
of what a property value says, which the panel, the command line and the
dialog now share. `PROP` became verb-first like every other table verb -
`PROP LIST | SET key value [text|integer|real|boolean] | DELETE key |
RENAME old new` - because the old bare `PROP key value` could not be
extended without becoming ambiguous (a property may be named DELETE), and
because a 12d feature code of "2" must be able to stay text.

OUTSTANDING: property DEFINITIONS - the schema, what 12d calls attribute
definitions - are not managed here or anywhere; the dialog edits the
attributes an entity has rather than declaring what attributes a kind of
entity should have. That is what remains of this slice.

**Slice 6 DELIVERED (2026-09-22).** Edit > Layers... (Ctrl+L) is the whole
table - name, on, locked, colour, linetype, weight, hatch and how many
entities are on each - with a form below for the definition (including the
dimension style, which the dock has no room for), and New, New Child, Save
Changes, Rename or Move, Delete, Put Selection Here and Make Current. The
dock stays as the quick view, as the slice asked.

"Move" needed no new command: a layer name is a PATH, so moving a layer
under another parent IS renaming it, and `renameLayer` already takes the
subtree and the entities on it as one undo step. Make Current is not a
command at all - the current layer is session state like the selection, and
putting it on the undo stack would make Ctrl+Z undo where the next line
will be drawn.

**Slice 7 DELIVERED (2026-09-22).** One `applyAnnotation` in the importer
serves all three places text arrives - `string text`, vertex text and
segment text - and applies what 12d says about it: justification (the anchor
is moved to where the baseline starts, since a Katana text position IS the
left end of the baseline), the text's own colour ("no_colour" meaning the
string's), its textstyle as a Katana style, an `offset` where the size is
`worldsize` and so in model units, and a `raise` added to the elevation
because a raise is a level and not a plan displacement. Slant, width factor,
justification and a paper-millimetre offset are kept as `12d.text.*`.

**The plan asked for offset, raise and slant to be "applied"; two of those
are recorded here as wrong.** Slant and width factor have no field in a
Katana `TextGeometry`, and inventing them in the renderer would be a worse
lie than leaving the text upright - they are kept and written back instead.
The manual does not give the DIRECTION of a text offset; it is taken as
perpendicular to the text, to the left, and that assumption is stated where
it is made. Measured in the sample archives: every annotation is
"bottom-left", `raise`, `slant` and `x_factor` are always 0, 0 and 1, and
`offset` is 0 except 12 (with `papersize 10`) and 0.2 (with `worldsize`) -
so justification and colour are what actually change a real file, and the
width estimate behind justification never fires on one.

**Slice 8 DELIVERED (2026-09-22).** A super tin is built into a surface
(`terrain::combineSurfaces`, a new `super_surface.hpp`) in which a LATER
member overrides an earlier one wherever it covers it; the members stay as
surfaces of their own, because a tin and a super tin are separate objects in
12d. `test super tin.12da` now arrives as four surfaces - NATURAL SURFACE,
PAD A, PAD B and COMBINED SURFACE of 34 triangles, the base's 32 minus the
14 the pads took, plus the pads' 16 (this said 18, which does not add up). A `ref_data` point cloud is looked for
BESIDE the archive by file name and read when it is there; when it is not,
the warning now says so rather than implying it was never tried.

Three decisions recorded where they are made. A triangle of a lower member
is dropped WHOLE if a higher one covers any part of it, not merely its
middle: two sheets over one point would make `elevationAt` answer with
whichever triangle it met first, which is the base as often as the pad, and
a ragged seam that gives NO answer just outside a pad is honest where a
wrong level is not. The rank order (later wins) is not in the manual, which
lists the tins and says nothing; it follows the sample archive's own comment
("Rank order: base surface first, then the pads that substitute it over
their footprints") and is one comparison to reverse, with the test that pins
it saying so. And only the file NAME of a `ref_data` reference is used -
following "..\..\scans\site.las" out of the directory the archive sits in
would let a file choose what gets read.

OUTSTANDING: the seam is exact to within one triangle of the lower member,
not exactly clipped; clipping each triangle against the higher surface's
boundary and re-triangulating the offcuts is the exact answer and is
recorded in `super_surface.hpp` as the alternative rejected.

**Slice 9 PARTIALLY DELIVERED (2026-09-22).**
`tests/interop/test_round_trip.cpp` takes six fixtures through import,
export, import, export, import. The first export NORMALISES (an arc is
written as a two-vertex super string, a drainage string as its line plus its
pits, a full_tin as a tin), so the property asserted is that every reading
after the first is identical - the same entities, styles with their symbols,
layers, meshes with their faces and face colours, tallies and per-string
summaries - and that the first pass loses nothing.
`katana_12da_probe --roundtrip` does the same on the large archives and
`docs/interop.md` records the run over all fourteen: thirteen are stable.

It found two defects, which is what it was for. **Fixed:** exporting a
drawing imported from a 12da wrote every alignment twice - as the alignment
AND as the centreline polyline the import made to draw it with - and each
round trip read the duplicate back and added another. The exporter now skips
a centreline whose alignment it is writing, and the alignment carries the
layer, colour and style that polyline wore (before this they were hard-coded
to model "Alignments", colour red, style "1", so an alignment now keeps its
appearance for the first time).

**The surface defect this slice found, and its cause.** Four surfaces of
`plot_PW_example_data.12da` - `Design/2/LOTS`, `Design/2/ROADS`,
`Design/2/ROADS DETAIL` and `Super/FS` - built on the first import and were
REFUSED on the second with "two triangles run along an edge in the same
direction". The hypothesis recorded here was a duplicated triangle pair. It
was wrong. The cause was a FLAT triangle, and there were two independent
faults behind it:

1. 12d's own tins carry triangles with no plan area. Counted directly from
   the file's hexadecimal floats (outside Katana, by
   `tools/sliver_census.py`): seven triangles across the eight surfaces
   have a doubled area below 6e-14 m^2 over coordinates of 6.2e6 m - smaller
   than the rounding error of computing that area, so which way round they
   were listed is noise. Katana measured the sign anyway and handed
   `TinSurface` an edge direction chosen by the last bits of a subtraction.
   **Fixed**: the import nulls a triangle that is degenerate in plan, exactly
   as it already nulls one with a null height. The census predicted the four
   failures and the four survivors with no exceptions.
2. The writer put tin points out at eight decimal places, moving each by up
   to 5e-9 - enough to flatten a sliver that was not quite flat. 12d Model's
   own default is `output_tin_hex_floats true` for this reason, and there was
   already a test in the repository showing eight places loses the value.
   **Fixed**: `WriteOptions::hexFloatTins` now defaults to true.

Either fix alone makes the file stable (both were measured that way); both
are kept, because they are answers to different questions - what a degenerate
triangle means, and whose bits those are to throw away.

**STATUS: 20.2 is delivered but for the property DEFINITIONS left over from
slice 5, which is recorded and does not block the application.**

---

## 20.3 The survey coding programme — TAKES PRECEDENCE OVER 20.2's REMAINDER AND EVERY OTHER PHASE

**The user's instruction of 2026-09-22, and the reason this section exists:**

> look at the files i put here `C:\GitHubProjects\Katana\docs\12d Refrence Files`,
> they are all related, as survey code WM is a water main and gets a specific
> linestyle in linestyles file, as well as some codes get a symbole, i want you
> to implement all this, mapfile, symples and linestyles, this is more
> important than other steps on the plan, so dont stop till it finishes and
> fully implmented

**Nothing else is started or resumed until every slice here is DELIVERED**,
including the property definitions left over from 20.2 slice 5. The only
exception is a defect that blocks this work.

### What the three files are

They are a real production customisation from a road authority, and they are
how a survey becomes a drawing. The files are third-party material under
their own licence and are NOT part of this repository; everything that uses
them skips when they are absent.

**CORRECTED (2026-09-23): until this date they WERE in the repository.** Commit
b77386e said it took them out and that `.gitignore` kept them out; neither was
true - all four were still tracked at HEAD and `.gitignore` had no rule for
them. They are now untracked with a rule added. They remain in the history
from commit 4e070d8 onwards, which has been pushed to `origin`; removing them
from the history needs a history rewrite and a force-push, which is the
owner's decision and has not been done.

| File | What it is |
|---|---|
| a detail mapfile (`.mapfile`) | XML (UTF-16LE). 728 rules keyed by survey code: what MODEL, colour, linestyle, weight and breakline a code becomes, which symbol it gets, what attributes it carries. |
| a linestyle library (`.4d`) | 322 linestyle definitions - the strokes a line is drawn with. ASCII. |
| a symbol library (`.4d`) | 474 symbol definitions, same grammar. UTF-16LE. |
| a second mapfile (`.4d`) | 900 more rules, sharing its extension with the libraries. |

A surveyor codes a point `WM01`. The mapfile's `WM*` rule puts it in model
`SURVEY SERVICES`, colour `sui water potable`, breakline `Line`, linestyle
`WATR Main`; the linestyle library says what `WATR Main` is drawn with. A
code like `AC*` additionally names a symbol (`CULT Bollard`) from the symbol
library. That chain is what this section builds.

### The grammar, as measured rather than assumed

Both `.4d` files are ONE grammar - which the 12d manual confirms: "There can
be the same symbol (**defined as a linestyle**) for every vertex"
(`10-super-string.md` 510). A symbol is a linestyle whose `mode` is `vertex`.

```
<kind> "NAME" {                 kind = paperstyle | worldstyle | twoptstyle
  group "Survey/WATR"           a folder path for the library tree
  mode vertex                   drawn at each vertex rather than along the line
  length 2.5                    the period of the pattern along the line
  factor 5                      a scale applied to every coordinate
  xorigin 0   yorigin 0         worldstyle/paperstyle origin
  xorigin1/yorigin1             twoptstyle: the first anchor
  xorigin2/yorigin2             twoptstyle: the second anchor
  stretch_mode 2  cycle_mode 2  twoptstyle: how it fills the span
  colour "pen 035"              the pen for the strokes that follow;
                                "view_colour" means the entity's own
  move X Y                      pen up
  draw X Y                      pen down
  arc RADIUS START END          degrees, centred on the current point
  circle RADIUS                 centred on the current point
  dot 0
  text "T" ANGLE HEIGHT "justify" "font" WIDTHFACTOR SLANT ? ?
}
```

Counted over the two files: paperstyle 238, worldstyle 510, twoptstyle 48;
`move` 17045, `draw` 17317, `circle` 312, `arc` 104, `dot` 179, `text` 514.
`paperstyle` sizes are plot millimetres and so scale with the plot;
`worldstyle` sizes are model units and stay on the ground.

The mapfile's `<map_data>` is a list of `<item>`, each keyed by a survey code
which is either exact (209 of the 465 distinct keys) or a prefix wildcard
(`WM*`, 256 of them) - there is not one mid-string wildcard in the file. An
item carries exactly ONE aspect, and 213 keys carry more than one item, so a
code's treatment is the UNION of the aspects that match it:

| Aspect | Fields | Count |
|---|---|---|
| geometry | `model` `colour` `breakline` `linestyle` `weight` `group` | 457 |
| symbol | `symbol_data{style colour size rotation offset raise}` `hide` | 200 |
| attributes | `attributes` / `vertex_attributes` / `segment_attributes`, with `justify` `shape` `size1` `size2` `active` | 48 |
| map_attributes | `map_attributes{integer\|text{name value}}` | 13 |
| textstyle | `textstyle_data{textstyle colour type size justify_x justify_y}` | 7 |

### The slices

1. **The style library reader. DELIVERED.** `entity::LineStyle` and
   `entity::StyleLibrary` (`include/katana/entity/style_library.hpp`) are the
   definitions - one type for linestyles and symbols alike, because in 12d a
   symbol IS a linestyle and `mode vertex` is the only difference.
   `archive12d::readStyleLibrary` (`include/katana/archive12d/style_library.hpp`)
   reads a `.4d` into one, keeping every command and naming every command it
   does not know. `readStyleLibraryInto` loads a customisation made of several
   files, later winning. See `docs/survey_coding.md` for the grammar, where
   each piece lives and why, and the measurements.

   Evidence: 12 tests. The real reference customisation reads with
   NO warnings - 796 definitions, 792 after the four defined twice, 157 of
   them symbols, in 71 groups - and every count is checked against a script
   that uses none of Katana's code.

   The types live in `entity`, not in `cad` as this plan first said, because
   `commands` has to see them to apply a survey code and `commands` cannot see
   `cad`. A library is deliberately NOT part of a Model: it is a site-wide
   customisation a project NAMES rather than copies, which is how 12d works
   and avoids writing 35,000 strokes into every project file.
2. **Strokes on demand. DELIVERED for the geometry.**
   `cad::styleDrawing` (`include/katana/cad/style_drawing.hpp`) turns a
   definition into polylines and texts in model coordinates - a symbol at each
   vertex, a two-point style across the span, or a pattern repeated along the
   line, with paper and world scaling. 14 tests, every expected value worked
   out by hand. See docs/survey_coding.md for the three decisions in it.

   OUTSTANDING: the sixteen built-in symbols are still a closed set that
   `Style::symbol` is validated against, so a style naming a library
   definition is refused by the model. That is slice 5, where it belongs.
3. **The mapfile reader. DELIVERED.** `entity::SurveyMap` and `SurveyRule`
   (`include/katana/entity/survey_map.hpp`) are the table;
   `archive12d::readMapFile` (`include/katana/archive12d/map_file.hpp`) reads
   it, on a small strict XML reader (`katana::core::readXml`,
   `include/katana/core/xml.hpp`) rather than a linked one, so this module
   keeps its no-third-party-dependency property and stays under the sanitizer
   job. `readMapFileInto` loads several mapfiles as one customisation.

   The plan above said the aspects are told apart by which fields a rule
   carries. THAT IS WRONG, and the test against the real file is what caught
   it: a mapfile is written in up to ten SECTIONS and the section is the
   meaning - `<map_attributes>` is attributes on the string inside
   `string_attribute_data` and attributes on each vertex inside
   `vertex_attribute_data`. Reading only `<map_data>` takes 457 of the 725
   rules and loses every symbol. See docs/survey_coding.md.

   Evidence: 15 tests. The reference detail mapfile reads as 725
   rules over 465 keys with no warnings; `names.4d` - a second mapfile,
   despite the extension a linestyle library also uses - reads as 899 rules
   with one warning, which is correct.
4. **Applying a code. DELIVERED.** `cad::applySurveyCodes`
   (`include/katana/cad/survey_coding.hpp`) turns the mapfile's answer into
   the layer, the style and the properties an entity should have, as ONE
   undoable transaction - twenty thousand coded points are one undo, not
   twenty thousand. `CODE [<property>]` in `katana_cli`. It reports what it
   did AND every code the mapfile had no rule for, because a code silently
   left alone looks exactly like a code that was handled. 7 tests; see
   docs/survey_coding.md for the six decisions in it.

   On a menu since b15eaa8: File > Apply Survey Codes (this slice said it was
   CLI-only, and so did the status below, until the audit of 2026-09-23 found
   the menu item).
5. **The model and the project. PARTIALLY DELIVERED.** `Style::symbol` is no
   longer a closed set: it is a name resolved when it is drawn, exactly as
   `Style::linetype` already was. `cad::Document` carries the loaded
   `StyleLibrary` and `SurveyMap` - not in the Model, because a customisation
   is named by a project rather than copied into it - and `definitionFor` is
   the one place that resolves a name.

   The model accepts any name because a project may be opened before its
   library is loaded; the COMMAND still refuses one nothing defines, so a
   typo is caught where a person typed it. `STYLE SET s symbol CULT Bollard`
   also now takes the rest of the line, since a library name has spaces in it.

   OUTSTANDING: a project does not yet remember which library and mapfile it
   was drawn with, so they must be loaded again each session.
6. **Loading and drawing. PARTIALLY DELIVERED.**
   `archive12d::readCustomisation` loads any mix of libraries and mapfiles,
   deciding what each file is BY LOOKING INSIDE IT - `.4d` is the extension of
   both formats in the reference customisation, so the name cannot be
   trusted. `File > Load 12d Customisation...` in the application, `CUSTOMISE
   <file>...` in `katana_cli`, and `--customise <file>...` headlessly. They
   share `readCustomisation`, but not one report: the CLI writes its own, and
   `--customise` is ignored outside the `--screenshot` path (audit QT-04, open)
   - this used to say all three take the same path and write the same report.

   The viewport draws them: a point whose style names a definition gets that
   definition's strokes, a line whose linetype names one has it laid along
   the line, and a `colour` command inside a definition changes the pen.

   OUTSTANDING: a library BROWSER - the groups tree with a preview of each
   definition - is not built; the style manager still lists only the
   document's own styles.
7. **The CLI and the documentation. DELIVERED** for what exists so far:
   `CUSTOMISE` with its report, and `docs/survey_coding.md`.

### Two defects a production archive found, both now FIXED

Found with a real 15 MB archive and fixed with its numbers in hand.

**1. A `breakline point` string of several vertices was drawn as a polyline.**
That flag means the vertices ARE points. Measured in that archive: every
string states it - 12,280 `line`, 13,379 `point` - and 53 of the point ones
have several vertices, including a 61-vertex string of drill holes in a
`SURVEY INFRASTRUCTURE V2` model. Each vertex is now its own point entity,
and the import says how many it made. The trap, recorded in
docs/survey_coding.md: the CURRENT breakline type defaults to `point`, so 21
hand-written fixtures that meant lines became points; they now say
`breakline line` once at file level.

**2. A string's symbol was taken onto its style only when the string had ONE
vertex**, so that drill-hole string drew no symbols where 12d draws 61. A
string with one symbol block now takes it whatever its length, and the
exporter's symbol block moved out of its `PointGeometry` branch so a line is
written back with it.

**3. And a diagnostic, because neither was visible as a fault.** Loading a
customisation now reports how many of the drawing's styles it answers for -
`cad::customisationCoverage` - since "no linestyles showing" looks the same
whether nothing is loaded, the library does not define what the drawing
names, or the drawing's styles are 12d's plain lines.

### The customisation is compiled in (the user's instruction of 2026-09-22)

> i dont want it be loded at everytime softwares opens i want it as solid part
> of the software not as a customization

Done: `tools/embed_customisation.py` turns the customisation files into a
generated source at build time and `archive12d::builtinCustomisation` parses
them once, so the linestyles, symbols and survey codes are part of the binary
and nothing is found, loaded or configured. Verified by moving the files away
entirely and running: 792 definitions and 1,624 rules, still there.

The bytes are embedded rather than generated as C++ structures - one reader
for a built-in and a loaded customisation, and 21 ms in Release once per
session against a multi-megabyte translation unit. The generated source is
never committed, since the customisation is third-party material under its own
licence; a checkout without it builds an empty table and draws plain lines.

**STATUS: DELIVERED, bar the one thing below that is still true.** The
chain the user asked for works end to end: a mapfile, a linestyle library and
a symbol library load together; a survey code resolves through the mapfile to
a model, a colour, a linestyle and a symbol; the entity gets them; and the
viewport draws the definition's strokes.

OUTSTANDING, not blocking:
- a library BROWSER - the groups tree with a preview of each definition - is
  not built, so the 792 definitions can be used but not looked through (in
  progress 2026-09-23).
- a mapfile symbol's `rotation`, `offset` (across the string) and `raise` are
  parsed into `entity::SurveySymbol` and then dropped: `applySurveyCodes`
  carries only the style and size, `Style` has no rotation, and the viewport
  draws every symbol at 0. Found by the audit of 2026-09-23 (the docs said the
  rotation was applied). Not blocking because the reference customisation
  sets all three to nothing: counted over both of its mapfiles (decoded from
  UTF-16), 401 `<symbol_data>` rules, and every `rotation`, `offset` and
  `raise` in them is zero or empty - so no drawing made from it is affected.

Struck from this list, with the reason: "a project does not remember which
customisation it was drawn with" stopped being true of the ordinary case when
the customisation was compiled in (above) - there is nothing to remember. A
customisation loaded ON TOP of the built-in one is still per-session, and that
is recorded as a limitation rather than as outstanding work, since nothing
asks for it. "Applying a code is a CLI verb" was never true after b15eaa8:
File > Apply Survey Codes exists.

---

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

**Ninth slice PARTIALLY DELIVERED: grading, the engine.** `cad::gradeToSurface`
takes a feature line - a closed pad at its design heights, or an open line
graded on one side - and carries a batter of the given cut or fill slope
outward from every vertex and every interval along every edge until it meets
the ground, by the corridor's march-and-bisect, then triangulates the feature
line, the daylight line and every batter as breaklines into the grading
surface and measures the earthwork against the ground with the exact overlay.
At a vertex the batter runs along the bisector and is FLATTER by the cosine
of half the corner, so the daylight corner is where the two batter planes
meet - the mitre a machine builds - not a chamfer 29% short. Tested against
hand-worked frustums (a 10 m pad 2 m up at 1 in 2: an 18 m daylight square
and 402.667 m^3, exact) and a sloping ground where each side daylights at its
own distance. **OUTSTANDING:** no way to reach it from the application yet
(Terrain > Grade Pad... is the intended command: select a closed polyline,
give elevation and slopes, get the surface and the daylight polyline);
rounded corners; benches; grading to a fixed width or elevation rather than
to a surface. Set aside on 2026-09-22 for the 12d programme in section 25.2,
which takes precedence over everything else in this plan until it is done.

**OUTSTANDING in this phase:** richer assemblies (kerbs, verges, benches,
subgrades) as a table of their own; the prismoidal correction, deliberately
not applied; grading in the application (above). `deleteAlignment` still has
no in-use guard because nothing references an alignment by name.

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

# 45. Survey module and instrument interoperability

**STATUS: IN PROGRESS.** Requested by the user on 2026-09-23, and taking
precedence over the remainder of section 20.2 in `CLAUDE.md` section 5.2's
ordering until it is delivered.

A Survey menu, a Point Manager, an import wizard, and a Survey Data Exchange
subsystem that reads field data from Leica, Trimble and Topcon instruments and
data collectors into the drawing.

## 45.1 What already existed, and what this programme is therefore NOT

The brief asked for fifteen phases. Four of them were already built, and this is
recorded here so that nobody builds them a second time:

* **The survey calculation engine exists.** `katana::survey` has `cogo.hpp`
  (inverse, forward, polygon area and perimeter, slope-to-horizontal reduction,
  trigonometric height difference), `traverse.hpp` (computation, misclosure and
  adjustment), `angles.hpp`, `leveling.hpp`, `least_squares.hpp`,
  `network_adjustment.hpp`, `error_propagation.hpp` and `statistics.hpp`. The
  brief's "Phase 3 - survey calculation engine" is a wiring job, not a build.
* **A normalized survey model exists** in `survey/data_model.hpp`: `SurveyPoint`
  and four observation kinds, with the conventions stated at the top of the file
  (metres, **radians**, azimuths clockwise in [0, 2*pi), ordered containers so no
  result depends on hashing). What it lacked was provenance, stations, a project
  aggregate, and the calculated-versus-observed distinction. Those are 45.2.
* **Coordinate reference systems exist.** `katana::geodesy` has
  `coordinate_reference_system.hpp`, `coordinate_transformer.hpp`, `units.hpp`,
  `ellipsoid.hpp`, `geodesic.hpp` and `grid_factors.hpp`, with PROJ kept behind
  `src/` (Rule 4). The brief's "Phase 11" is wiring, and the wizard's job is to
  refuse to transform rather than to implement a transformation.
* **A text decoder, text helpers and exact unit ratios exist** in the layers
  below surveyio - `core/text_encoding.hpp`, `core/text.hpp`,
  `math/unit_ratio.hpp` - and a parser uses them rather than its own (45.3b).
* **A strict XML reader exists.** It was private to `archive12d`; LandXML does
  not get a second one, so it was promoted to `core` as `katana::core::readXml`
  (`include/katana/core/xml.hpp`). It refuses a doctype, an undefined entity and
  nesting past `kXmlMaxDepth`, and those three refusals are security properties
  rather than simplifications: a doctype is the entity-expansion and
  external-entity entry point, and the reader recurses, so past the cap the
  answer must be an error and not a blown stack.

## 45.2 Architecture, decided before any of it was written

**A new `surveyio` layer**, registered in `tools/check_layering.cmake` as
allowed `core;math;geometry;survey`. It parses manufacturer formats into
`katana::survey` values and nothing else.

* **`cad` may NOT see `surveyio`**, exactly as it may not see `interop` or
  `archive12d`. This is the structural answer to the brief's section 7 - "do not
  make the CAD application internally dependent on Leica, Trimble or Topcon
  structures". `app`/`qt` own the parsers; `cad` only ever sees
  `katana::survey`, which it is already allowed to see. A Leica type cannot
  reach the drawing even by accident, because the layering test fails the build.
* **`geodesy` is deliberately absent from `surveyio`'s allowed list.** A parser
  records the coordinate system a file *declares* - a name, an EPSG code, or
  explicitly "unknown" - and transforms nothing. Transformation is an act the
  user authorises in the wizard with both systems named. Making it structurally
  impossible in the parser is stronger than remembering not to do it.
* **Only the file NAME of a source is stored, never a path the file supplied.**
  The same rule the 12da reader follows for `ref_data`: a file must not be able
  to choose what gets read.

## 45.3 Slices

| # | Slice | Status |
|---|---|---|
| 1 | The XML reader promoted from `archive12d` to `core`, with the tests it never had | **DELIVERED** |
| 2 | Model: provenance, `SurveyStation`, `SurveyProject`, calculated-vs-observed | **DELIVERED** |
| 3 | `surveyio` scaffolding: format descriptors, a self-registering format registry, and detection that cannot silently guess | **DELIVERED** |
| 4 | Generic ASCII/CSV import and export, with column mapping and saved templates | pending |
| 5 | LandXML import and export | pending |
| 6 | Leica: GSI-8 and GSI-16, and ASCII/CSV | pending |
| 7 | Trimble: JobXML, CSV/ASCII, LandXML | pending |
| 8 | Topcon: documented observation formats, CSV/ASCII | pending |
| 8b | The `cad` bridge: a `SurveyProject` becomes ONE undoable command | **DELIVERED** |
| 9 | The Survey menu and its commands | pending |
| 10 | The import wizard, six steps, with validation and a warning/error report | pending |
| 11 | Point Manager with source column and filtering | pending |
| 12 | Instrument profiles, and import history undoable as one transaction | pending |
| 13 | Reports: point, observation, import, traverse | pending |
| 14 | The compatibility matrix, stating format AND version actually supported | pending |

## 45.3b The bridge, and a gap it exposed

`cad/survey_import.hpp` turns a `survey::SurveyProject` into one
`commands::Transaction` - the layers it needs, then its points - so a single
undo removes an import of ten thousand points. It calls
`survey::validateProject()` rather than re-checking anything, so a parser and
the bridge cannot disagree about what a valid project is, and it returns
`nullptr` with no error for an empty project so that "nothing to do" and
"something went wrong" stay distinguishable. It lives in `cad` because `cad` may
not see `surveyio`; a field code is sanitised before it becomes a layer name,
because a layer name is a PATH and a code containing `/` would otherwise build a
subtree nobody asked for.

**GAP, found while building it - FIXED (2026-09-23).**
`survey::SurveyPoint::elevation` was a plain `double` with no "unset" state, so a
file with no height column yielded 0.0 and the bridge wrote it as a real height
at the datum; a surface built from such a drawing was flattened to zero under
every one of those points, with nothing reported. It is now
`std::optional<double>`. The bridge writes no elevation property for a point
without one (the surface builder already leaves such a point out), counts them
in `SurveyImportReport::pointsWithoutElevation` and warns; the level adjustment
refuses a held or weighted benchmark with no published height, naming it,
rather than levelling the run onto a datum of zero. The optional was chosen
over a companion flag because a flag and a value can disagree and a bare double
can be read without looking at the flag. Proven: with the refusal removed,
`SurveyLevelNetwork.AHeldBenchmarkWithNoHeightIsRefusedRatherThanTakenAsZero`
fails. The first round of parsers had each worked round the gap with a private
metadata key (`elevation-absent`, `landxml.elevationAbsent`, "none in source");
those go when the parsers are finished against the fixed model.

**A second gap of the same shape, FIXED the same day: a point with no
coordinates at all.** A raw observation file names its targets without
positioning them, and `SurveyPoint` has no such state. The unfinished Leica
parser created such a point AT THE ORIGIN with a metadata marker and the Topcon
one at placeholder ordinates - either would be drawn as a real mark at (0, 0) by
any consumer that forgot the marker. `survey::UnpositionedPoint` and
`SurveyProject::unpositionedPoints` now carry them: same id space as `points`,
referable by observations, stations and features, never drawn by the bridge,
which counts them in `pointsWithoutPosition` and warns - including when that
leaves nothing to import. Reduction of raw observations to coordinates (the
brief's "survey calculation engine" wiring) is what will turn them into
`CoordinateSource::Calculated` points, and is not yet built.

**Three shared pieces the parsers had each written for themselves, collapsed
(2026-09-23).** Found by reading the unfinished parsers side by side: a second
text decoder, a third private trim/number parser, and the foot and link ratios
repeated as doubles. The decoder moved from `archive12d` to
`katana::core::decodeText` (with `decodeTextAs` for an encoding a person
states), the helpers became `core/text.hpp`, and the exact length ratios moved
to `math/unit_ratio.hpp` with `geodesy/units.cpp` built on them and
`survey::metresPer(LinearUnit)` mapping a declared unit onto them. The 12d
reader's and the XML reader's own helpers now delegate to core, and in doing so
stopped consulting the C locale. Record in `docs/survey.md`.

## 45.4 The proprietary-format rule, as it will be applied

The brief forbids guessing undocumented binary structures and forbids claiming
support because of a file extension. This programme applies that as follows, and
the UI carries it:

* A format is implemented only when its structure is publicly documented or can
  be validated against a specification. Every format the UI offers names its
  parser variant - "Leica GSI-16" - not its manufacturer.
* A `FormatDescriptor` states what a format can carry (points, observations,
  stations), whether import and export are supported, and the parser version.
  The UI shows that record. There is no "all Leica files supported" anywhere.
* Detection returns a RANKED list with an explicit uncertain outcome that a
  caller cannot ignore. An unknown file is never handed to an incompatible
  parser.
* Test fixtures are constructed FROM specifications. No proprietary sample data.

## 45.5 Numbers that must not be guessed

Two failure modes in this area silently corrupt survey data rather than failing,
and both are called out here because a test that does not target them will pass
over the bug:

* **GSI encodes the decimal position in the word, not as a decimal point.** A
  parser that reads the data block as a plain integer produces coordinates wrong
  by orders of magnitude, and they look plausible. Every GSI test asserts a
  value worked out by hand from the specification.
* **Coordinate ORDER differs between families.** LandXML `CgPoint` content and
  Trimble exports are northing-first; much else is easting-first. Where the
  order is genuinely ambiguous in a file, the importer asks. It never guesses,
  because a transposed survey is a survey that looks fine and is wrong.

---

# 46. The audit of 2026-09-23 — the defect register

**STATUS: OPEN — 133 of 142 confirmed defects outstanding.**

A read-only audit of every subsystem: ten auditors, one per area, each told to
verify the code rather than trust this plan, and every defect they reported put
to a second reader told to refute it. The 142 that survived are recorded in
full — what goes wrong, a failure scenario, a suggested fix — in
`docs/audit/2026-09-23-defects.md`, each with an ID. The audit's other findings
(tests that would pass over a bug, duplication, numbers with no source, stale
documentation, unmeasured performance concerns, knowledge to write next to the
code) are in `docs/audit/2026-09-23-findings.md`; they were not challenged, so
each is a lead to check. The false claims it found in this plan were corrected
in f90a1db where no pending fix changes them.

A fix cites the ID (`fixes audit QT-07`) in its commit message and, in the same
commit, runs `python tools/audit_register.py <date> QT-07`, which marks the
entry FIXED in the register and recounts this table from the register, so the
two cannot disagree. (A commit cannot name its own hash, so the entry says
FIXED and a date, and `git log --grep QT-07` finds the commit.)

| Area | high | medium | low | open |
|---|---|---|---|---|
| MOD — entity model, commands, storage, selection | 0 | 0 | 12 | 12 |
| QT — the desktop application | 2 | 10 | 15 | 27 |
| IO — raster, vector and point-cloud interoperability | 1 | 6 | 9 | 16 |
| A12 — the 12d archive and customisation | 1 | 4 | 11 | 16 |
| CAD — the CAD engine | 0 | 5 | 13 | 18 |
| REN — rendering and scene building | 0 | 2 | 5 | 7 |
| GEO — core, maths and geometry | 0 | 2 | 10 | 12 |
| TER — terrain | 0 | 1 | 4 | 5 |
| SUR — survey, geodesy and survey import | 0 | 1 | 4 | 5 |
| NET — least-squares adjustment | 0 | 1 | 2 | 3 |
| BLD — build, tests and tooling | 0 | 3 | 9 | 12 |
| **total** | **4** | **35** | **94** | **133** |

The high-severity ones, taken first:

* ~~**MOD-01**~~ FIXED 2026-09-23 — hiding or locking a parent layer does not hide, protect or stop
  editing the entities on its children (`selection.cpp`, `change_set.cpp`).
* **QT-01** — a 12d archive holding only tins or trimeshes cannot be imported:
  the empty import transaction is rejected and the surfaces are thrown away.
* **QT-02** — Styles and Linetypes' Save Changes rewrites the linetype and
  symbol of every 12d-imported style.
* ~~**IO-01**~~ FIXED 2026-09-23 — vector import discards Z and vector export writes Z = 0.
* ~~**IO-02**~~ FIXED 2026-09-23 — placement advice calls small data inside a large drawing "far
  apart", and its default button then moves it.
* **IO-03** — a budgeted point-cloud read materialises the whole file.
* **A12-01** — a split breakline-point string exports every point with the
  first vertex's attributes and no point id.
* REN-01 (eye-plane clipping) — FIXED in ea6cd24.

Where this sits in the order of work: CLAUDE.md 5.2 item 2 says a real defect
is fixed when it is small and adjacent and recorded otherwise - this register
is that record. CLAUDE.md 5.3 lets a wave of defect fixes run BESIDE the work
the order puts first when their file sets are disjoint, which is how they are
being taken; where the two compete for the same files, the order in 5.2 decides.
