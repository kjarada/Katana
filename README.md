# Katana

A high-performance, deterministic C++ CAD and surveying platform, built to the
plan in [PLAN.MD](PLAN.MD).

Katana is an engineering core first and an application second. Every geometric,
survey and numerical operation lives in a C++ library with no dependency on the
user interface, on storage, or (later) on the AI layer. The desktop application
and the command line are two thin front ends over the same engine.

## Status

| Phase | Subject | State |
|---|---|---|
| 01 | Build system, tests, sanitizers, static analysis | done |
| 02 | Mathematical foundation | done |
| 03 | Basic geometry | done |
| 04 | Geometry algorithms | done |
| 05 | Entity system | done |
| 06 | Command system, transactions, undo/redo | done |
| 07 | Project storage (SQLite) | done — schema 3 stores geometry as a binary blob; opening a 50k-entity project is 4.6× faster |
| 08 | Basic 2D CAD application (Qt 6) | done |
| 09 | Professional 2D CAD | partial — editing, snapping, nested layers, resolved appearance, linetypes and dimension styles done; splines, hatches and blocks outstanding |
| 10–13 | Survey data model, coordinate systems, survey calculations, least squares | done |
| 14 | Terrain engine | done |
| 15 | 3D rendering | partial — tiled multithreaded software renderer, orbit camera, tiled viewports, 3D and cross-section views; Vulkan backend outstanding |
| 16 | 3D CAD (Open CASCADE) | not started |
| 17 | Point clouds | partial — LAS/LAZ read/write, budgeted decimation, 2D display; no out-of-core LOD |
| 18 | Spatial indexing | partial — sparse hash grid behind snapping, picking, box selection and viewport repaint (50×–310× measured); no KD-tree, BVH, octree or terrain quadtree |
| 19 | Performance architecture | partial — deterministic `TaskPool` drives the renderer; no task graph, no SIMD |
| 20 | File interoperability | partial — raster, vector and point cloud import/export in the GUI and the CLI; no DWG/LandXML/IFC |
| 21 | Civil engineering | partial — profiles and cross sections with exact surface-break sampling; alignments, corridors, parcels, grading outstanding |
| 22–26 | Plotting, application API, Python AI layer, AI agent, hardening | not started |

776 tests pass in Debug and Release, including the architectural layering check.

## Building

Requires a C++23 compiler, CMake 3.24+, Qt 6, Eigen, PROJ, CGAL, SQLite,
nlohmann-json, GDAL and PDAL. On Windows these all come from MSYS2 UCRT64.

```bash
cmake --preset debug          # or release, relwithdebinfo, sanitize, tidy
cmake --build --preset debug
ctest --preset debug
```

Presets assume MSYS2 at `C:/msys64/ucrt64`; `linux-debug` and `linux-sanitize`
use system packages instead. Useful options:

| Option | Default | Effect |
|---|---|---|
| `KATANA_BUILD_TESTS` | ON | unit and integration tests |
| `KATANA_BUILD_BENCHMARKS` | ON | Google Benchmark executable |
| `KATANA_BUILD_QT_APP` | ON | Qt 6 desktop application |
| `KATANA_BUILD_IO` | ON | GDAL/PDAL interoperability module |
| `KATANA_WARNINGS_AS_ERRORS` | ON | `-Werror` |
| `KATANA_ENABLE_SANITIZERS` | OFF | ASan+UBSan (UBSan trap mode on MinGW) |
| `KATANA_ENABLE_CLANG_TIDY` | OFF | clang-tidy during compilation |
| `KATANA_ENABLE_CPPCHECK` | OFF | cppcheck during compilation |
| `KATANA_MODULE_FILTER` | *(empty)* | configure only the listed modules |

Other targets: `format` and `format-check` (clang-format), `run-benchmarks`.

## Running

`katana_qt_app` is the desktop application; it takes an optional project
directory. `katana_cli` is the same engine without a GUI:

```bash
katana_cli                               # interactive
katana_cli drawing.kcs                   # run a script
katana_cli -c "RECT 0,0 30,20" -c "SAVE site.katana"
```

Both accept the same command language — `HELP` lists it. Points may be absolute
(`12.5,40`), relative (`@3,4`) or polar (`@5<30`).

## Architecture

Dependencies run one way only, lowest layer first. A layer may never include a
header from a layer above it, and public headers may never expose a third-party
type (PLAN.MD Rule 4). Both rules are enforced by
[tools/check_layering.cmake](tools/check_layering.cmake), which runs as the
`layering` test on every `ctest`.

```
core        Result<T>/Error, structured logging          —
math        Vec/Mat/Quaternion/Transform, tolerances     core
geometry    2D/3D primitives, intersection, polygons,    math
            offset/trim/extend/fillet/chamfer
geodesy     CRS and transformations                      math    [PROJ]
survey      survey model, traverse, levelling, LSQ       math    [Eigen]
terrain     TIN, contours, volumes, tiles                geometry [CGAL]
entity      entities, layers, styles, properties         geometry [nlohmann]
commands    commands, change sets, transactions, undo    entity
storage     SQLite persistence, migrations, recovery     entity  [SQLite]
cad         document, selection, snapping, view,         commands, storage
            command interpreter
io          raster/vector/point-cloud adapters           core    [GDAL, PDAL]
qt          desktop application                          cad     [Qt 6]
app         katana_cli                                   cad
```

Third-party libraries in brackets are confined to that module's `.cpp` files
behind Katana's own interfaces.

Three rules shape most of the design:

* **The domain model is authoritative.** The viewport draws the model; it never
  holds state of its own that the model does not have.
* **Every modification is a command.** Commands validate before they touch
  anything, apply atomically, and undo from recorded before-images rather than
  by recomputing an inverse. This is also the interface the AI layer will get,
  which is why it is narrow and validated.
* **Tolerances are centralised.** Every comparison uses a named, documented
  tolerance from [`numerics.hpp`](include/katana/math/numerics.hpp). There are
  no ad-hoc epsilons.

## Testing

`ctest --preset debug` runs unit, integration, property and regression tests,
the end-to-end CLI tests, and the layering check. Expected values are derived
independently of the implementation — closed forms, exact synthetic
constructions, or hand calculations recorded in comments — because a test whose
expectation was taken from the code under test proves nothing.

## Documentation

`docs/` holds a document per major subsystem covering purpose, architecture,
API, numerical assumptions, threading, performance and failure modes:
[architecture](docs/architecture.md), [geometry](docs/geometry.md),
[model](docs/model.md), [cad](docs/cad.md), [interop](docs/interop.md).

## Importing and exporting data

The desktop application offers File > Import (Ctrl+I) and File > Export Vector,
and a Reference Data panel for imported imagery and point clouds. The headless
tool has the same verbs:

```bash
katana_cli -c "IMPORT parcels.shp" -c "LIST"
katana_cli site.kcs -c "EXPORT site.gpkg"
katana_qt_app my-project.katana ortho.tif scan.las   # opens with data loaded
```

Vector data becomes ordinary entities, in one undoable command. Rasters and
point clouds become reference data: backdrop that is drawn but not drawn on, and
deliberately outside the entity model and undo. See [docs/interop.md](docs/interop.md)
for the formats, the lossy conversions and what is refused rather than guessed.
