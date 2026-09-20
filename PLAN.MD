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
| 09 | Professional 2D CAD | partial — nested layer tree delivered; splines, hatches, blocks, dimension styles, linetypes outstanding |
| 10 | Survey Data | **done** |
| 11 | Coordinate Systems | **done** |
| 12 | Survey Calculations | **done** |
| 13 | Least Squares | **done** |
| 14 | Terrain | **done** |
| 15 | 3D Rendering | partial — tiled multithreaded SOFTWARE renderer, camera, tiled viewports, 3D and section views; Vulkan backend outstanding |
| 16 | 3D CAD | not started |
| 17 | Point Clouds | partial — PDAL read/write, budgeted decimation, 2D display; no out-of-core LOD |
| 18 | Spatial Indexing | partial — sparse hash grid behind snapping, picking and box selection (50x-310x measured); no KD-tree, BVH, octree or terrain quadtree |
| 19 | Performance Architecture | partial — `katana::core::TaskPool` (jthread, deterministic parallel-for) drives the renderer; no task graph, no SIMD |
| 20 | File Interoperability | partial — raster, vector and point cloud import/export in GUI and CLI; no DWG/LandXML/IFC |
| 21 | Civil Engineering | partial — profiles and cross sections along an alignment, with exact surface-break sampling; alignments, corridors, parcels and grading outstanding |
| 22 | Drawing/Plotting | not started |
| 23 | C++ Application API | not started |
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

**OUTSTANDING: splines, hatches, blocks, dimension styles, linetypes.**

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

Integrate Open CASCADE.

Use it for:

```text
B-rep
solids
boolean operations
extrusion
revolve
sweep
NURBS
fillet
chamfer
```

Create an internal application-level CAD API around it.

Do not expose Open CASCADE types to unrelated application modules.

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

**OUTSTANDING: the out-of-core half of this phase.** There is no spatial
hierarchy, no level of detail, and no streaming: the decimated sample is held in
memory. A billion-point dataset is opened as a sample of it, not worked on in
full. Sections 32 and 33 govern what that will need.

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

**OUTSTANDING:** the rest of the structures the phase names, each for the
workload it suits - KD-tree for nearest-point queries over survey marks, BVH
for mesh intersection, octree for point clouds, quadtree for terrain tiles. The
terrain module already has its own uniform bucket grid inside `TinSurface`;
unifying the two is worth considering but has not been measured. View culling
in the 2D viewport and the scene builder still walks every entity.

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

**OUTSTANDING:** no task graph or dependency scheduling; no work stealing (a
single shared job at a time, nested calls run inline); TBB not evaluated; no
SIMD anywhere; terrain generation, triangulation, point-cloud processing,
imports and exports are all still single-threaded. Latency, throughput, memory
bandwidth and cache behaviour are not yet benchmarked as section 32 requires.

The next measured win is the one `docs/storage.md` names: replacing the JSON
geometry encoding with a versioned binary one.

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

**OUTSTANDING: DWG, LandXML, IFC, and DXF import** (DXF is export only, through
GDAL's driver). **No coordinate transformation on import**: a file's declared
CRS is reported but coordinates are never reprojected, so mixing coordinate
systems is the user's responsibility. Reference layers are not yet persisted
into the project - the pixels are re-read from the source path each session.

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

**OUTSTANDING:** everything below — alignments as first-class entities with
horizontal and vertical geometry, profiles as design objects, corridors,
parcels and grading.

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
