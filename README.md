# Katana

A high-performance, deterministic C++ CAD and surveying platform, built to the
plan in [PLAN.MD](PLAN.MD).

Katana is an engineering core first and an application second. Every geometric,
survey and numerical operation lives in a C++ library with no dependency on the
user interface, on storage, or (later) on the AI layer. The desktop application
and the command line are two thin front ends over the same engine.

## Status

The authoritative record is the roadmap table in [PLAN.MD](PLAN.MD) section 5;
this is a summary of it.

| Phase | Subject | State |
|---|---|---|
| 01 | Build system, tests, sanitizers, static analysis | done (the CI workflow was deleted in an unrelated commit and is being restored) |
| 02–08 | Maths, geometry, algorithms, entities, commands/undo, SQLite storage, the Qt 6 application | done |
| 09 | Professional 2D CAD | partial — editing, snapping, nested layers, resolved appearance, linetypes, dimension styles, hatching, point symbols and a styles/linetypes manager done; authoring splines and blocks outstanding |
| 10–13 | Survey data model, coordinate systems, survey calculations, least squares | done |
| 14 | Terrain engine | done |
| 15 | 3D rendering | partial — tiled multithreaded software renderer, orbit camera, tiled viewports, 3D and cross-section views; Vulkan backend outstanding |
| 16 | 3D CAD | reshaped — Open CASCADE rejected with reasons in the plan; civil solids on the TIN instead |
| 17 | Point clouds | partial — LAS/LAZ read and write, budgeted decimation, 2D display, COPC in the engine; out-of-core level of detail outstanding |
| 18 | Spatial indexing | partial — sparse hash grid behind snapping, picking, box selection and repaint (50×–310× measured); no KD-tree, BVH, octree or terrain quadtree |
| 19 | Performance architecture | partial — deterministic `TaskPool` drives the renderer; task graph and work stealing rejected with reasons |
| 20 | File interoperability | partial — vector (with heights), raster and point-cloud import and 12d Archive (.12da/.12daz) import/export in the GUI and CLI; vector export too; raster and point-cloud export only in the library; no DWG or IFC |
| 21 | Civil engineering | partial — sections, clothoids, alignments and profiles, corridor quantities and surface, parcels; grading without a GUI route; richer assemblies outstanding |
| 22 | Drawing and plotting | partial — Plot to PDF at ISO sizes and standard scales; layouts and title blocks outstanding |
| 23 | Application API | reshaped — expose the existing command interpreter rather than write a second one |
| 24–26 | Python AI layer, AI agent, production hardening | not started |
| 45 | Survey data exchange: Leica, Trimble, Topcon, LandXML, CSV | in progress — model, detection and the drawing bridge done; parsers, reduction and the Survey menu being merged |
| 46 | The audit of 2026-09-23 | [142 confirmed defects](docs/audit/2026-09-23-defects.md), being fixed area by area |

1353 tests pass in Debug and Release, including the architectural layering check, a headless plot to PDF, a headless build of the main window, a headless click on a layer's visibility box and a headless import of surveyed points drawn with their symbols.

## Building

Requires a C++26 compiler, CMake 3.30+, Qt 6, Eigen, PROJ, CGAL, SQLite,
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

The desktop application is themed dark to match its viewport, with icon
toolbars whose icons are drawn in code (crisp at any DPI, recoloured by the
theme). `katana <project> --screenshot out.png` renders the main window
headlessly, and `--plot out.pdf` plots the drawing, so both can be reviewed
without a display.

Other targets: `format` and `format-check` (clang-format), `run-benchmarks`,
`katana_make_icons` (regenerates `resources/katana.ico` and an icon contact
sheet), `bundle` (a self-contained `<build>/dist/Katana` that runs with no
MSYS2, Qt or GDAL installed) and `package` (that tree as `Katana-<version>-win64.zip`, plus
an installer when NSIS is present).

## Running

`katana` is the desktop application (`<build>/bin/katana.exe`); it takes an
optional project directory. `katana_cli` is the same engine without a GUI:

```bash
katana_cli                               # interactive
katana_cli drawing.kcs                   # run a script
katana_cli -c "RECT 0,0 30,20" -c "SAVE site.katana"
```

Both run the same command interpreter — `HELP` lists its verbs — and each adds a
few of its own: the command line adds `IMPORT`, `EXPORT`, `REFS`, `CODE` and
`CUSTOMISE`, the desktop application's command line adds `ZOOM`, `GRID` and
`SNAP` (and reaches the others through its menus). Points may be absolute
(`12.5,40`), relative (`@3,4`) or polar (`@5<30`).

## Architecture

Dependencies run one way only, lowest layer first. A layer may never include a
header from a layer above it, and public headers may never expose a third-party
type (PLAN.MD Rule 4). Both rules are enforced by
[tools/check_layering.cmake](tools/check_layering.cmake), which runs as the
`layering` test on every `ctest`.

```
core        Result<T>/Error, logging, text, XML, tasks   —
math        Vec/Mat/Transform, tolerances, unit ratios   core
geometry    2D/3D primitives, intersection, polygons,    math
            offset/trim/extend/fillet/chamfer
geodesy     CRS and transformations                      math     [PROJ]
survey      survey model, traverse, levelling, LSQ       math     [Eigen]
surveyio    Leica/Trimble/Topcon/LandXML/CSV parsers     survey, geometry
terrain     TIN, contours, volumes, tiles                geometry [CGAL]
render      camera, draw lists, software rasteriser      geometry
entity      entities, layers, styles, properties         geometry [nlohmann]
commands    commands, change sets, transactions, undo    entity
storage     SQLite persistence, migrations, recovery     entity, survey [SQLite]
archive12d  the 12d Archive format and customisation     terrain, entity
cad         document, selection, snapping, view,         commands, storage,
            command interpreter, scene, survey bridge    render, survey, geodesy
gis, pointcloud  raster/vector/point-cloud adapters      core     [GDAL, PDAL]
interop     import/export over the adapters and 12d      commands, archive12d, gis
app         katana_cli                                   cad, interop, surveyio
qt          desktop application                          app      [Qt 6]
```

`cad` may see neither `interop` nor `surveyio`, so the application core builds
without GDAL and PDAL and no instrument format's types can reach the drawing.
[tools/check_layering.cmake](tools/check_layering.cmake) holds the exact lists.

Third-party libraries in brackets are confined to that module's `.cpp` files
behind Katana's own interfaces.

Three rules shape most of the design:

* **The domain model is authoritative.** The viewport draws the model; it never
  holds state of its own that the model does not have.
* **Every modification is a command.** Commands validate before they touch
  anything, apply atomically, and undo from recorded before-images rather than
  by recomputing an inverse. This is also the interface the AI layer will get,
  which is why it is narrow and validated.
* **Tolerances are centralised.** Comparisons use a named, documented
  tolerance from [`numerics.hpp`](include/katana/math/numerics.hpp). This README
  used to say there were no ad-hoc epsilons; the audit of 2026-09-23 found a
  dozen local ones in the CAD engine (corridor, grading, section, the
  interpreter), which are being replaced.

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
[model](docs/model.md), [storage](docs/storage.md), [cad](docs/cad.md), [render](docs/render.md),
[terrain](docs/terrain.md), [interop](docs/interop.md), [survey](docs/survey.md),
[survey coding](docs/survey_coding.md) and [performance](docs/performance.md),
plus the [audit register](docs/audit/2026-09-23-defects.md) and its
[other findings](docs/audit/2026-09-23-findings.md).

## Importing and exporting data

The desktop application offers File > Import (Ctrl+I) and File > Export Vector,
and a Reference Data panel for imported imagery and point clouds. The headless
tool has the same verbs:

```bash
katana_cli -c "IMPORT parcels.shp" -c "LIST"
katana_cli site.kcs -c "EXPORT site.gpkg"
katana my-project.katana ortho.tif scan.las   # opens with data loaded
```

Vector data becomes ordinary entities, in one undoable command. Rasters and
point clouds become reference data: backdrop that is drawn but not drawn on, and
deliberately outside the entity model and undo. See [docs/interop.md](docs/interop.md)
for the formats, the lossy conversions and what is refused rather than guessed.
