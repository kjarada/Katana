# Documentation map

Which document covers what, and so which one a change updates. A change to a
module updates that module's document in the same commit
(`docs/architecture.md`, "Working rules"); `tools/check_docs.py` (the `docs`
test) checks that every document here is listed and that what the checked
documents cite exists (`docs/testing.md`, "The docs test"). Every document is
a developer's record - decisions, measurements, failure modes, what is not
done. There is no user guide yet.

| Document | Covers | Update it when you change |
|---|---|---|
| [architecture.md](architecture.md) | the architectural rules and working rules, layering, error handling, numerics, determinism, logging, threading, the performance targets, what old section and phase numbers mean | a rule, a layer (`tools/check_layering.cmake`), `core` (errors, logging, `TaskPool`), `math/numerics.hpp` |
| [building.md](building.md) | toolchain, presets, `KATANA_*` options, targets, where binaries land, running `katana` and `katana_cli`, the customisation folder, bundling and the runtime deploy | `CMakeLists.txt`, `CMakePresets.json`, `cmake/`, a target or option, `src/katana_app/session.cpp`'s verbs |
| [testing.md](testing.md) | running the tests, each kind of test and how it is registered, the rules that make a test evidence, benchmark practice, the docs test | `tests/CMakeLists.txt`, `tests/support/`, `tools/check_*.cmake`, `tools/check_docs.py`, `tools/compare_benchmarks.py` |
| [geometry.md](geometry.md) | vectors, matrices, tolerances, 2D primitives and algorithms, spirals, chording, horizontal and vertical alignments | `katana_math`, `katana_geometry` |
| [model.md](model.md) | entities, layers and the named tables, commands and undo, project storage and its schema | `katana_entity`, `katana_commands`, `katana_storage` |
| [storage.md](storage.md) | why the project database is SQLite, the measurements, loading without copying | `katana_storage` (a storage engine or encoding decision) |
| [geodesy.md](geodesy.md) | coordinate reference systems, transformations, units, grid and ground factors, geodesics, site calibration | `katana_geodesy` |
| [terrain.md](terrain.md) | TIN surfaces, contours, volumes, tiled terrain | `katana_terrain` |
| [render.md](render.md) | camera, draw lists, the software rasteriser, the 3D and section views | `katana_render`, `render_view_widget.*`, `section_view_widget.*` |
| [performance.md](performance.md) | the measurements and what moved | anything measured (`benchmarks/`) |
| [interop.md](interop.md) | GIS, rasters, point clouds, the `.12da` archive, import and export | `katana_io`, `katana_interop`, `katana_archive12d` (the archive) |
| [geoprocessing.md](geoprocessing.md) | GDAL's algorithm framework on every front end: the bridge, the bindings of drawing data, surfaces and rasters, the one geoprocessing executor, the GDAL verb, its MCP tools, the window's jobs, and a section per geoprocessing package | `include/katana/gis/processing.hpp`, `src/katana_io/geo/`, `include/katana/interop/geo/`, `src/katana_interop/geo/`, `src/katana_app/geo/`, `src/katana_qt/geo/`, `tests/geo/` |
| [ifc.md](ifc.md) | IFC 4.3 export and import: which class each thing becomes (never a proxy), alignments as business logic and geometry, AS 5488 services and their delivery schema in IFC, georeferencing, GlobalIds, reconstructing alignments on import, validation against IfcOpenShell and buildingSMART's rules | `katana_ifc`, `include/katana/ifc/`, `src/katana_app/ifc_verbs.*`, `src/katana_qt/main_window_ifc.cpp`, `src/katana_qt/ifc_dialogs.*`, `tools/check_ifc.py`, `tools/ifc_product_classes.py` |
| [gis_online.md](gis_online.md) | GIS > Online Data: the provider catalogue, how each web service is asked, limits, the cache, licences and attribution, the dialog and the `ONLINE` verbs | `resources/online/`, `katana_interop`'s `online_*`, `gis/web_access.hpp`, `gis/reproject.hpp`, `src/katana_qt/gis_online*` |
| [survey.md](survey.md) | the survey model and calculations, instrument and field-file import, the Survey menu | `katana_survey`, `katana_surveyio`, `src/katana_qt/survey/` |
| [subsurface_utilities.md](subsurface_utilities.md) | AS 5488 quality levels, grading located services, depth of cover, clearance of proposed works, verifying detections by exposure, the utility schedule format, delivery schemas and the TfNSW Utility Schema, `UTILITY` and drawing the services by quality level | `include/katana/survey/subsurface/`, `src/katana_survey/subsurface_*.cpp`, `include/katana/cad/utilities/`, `src/katana_cad/utilities/`, `src/katana_qt/survey/utility_*`, `tools/utility_schema_domains.py` |
| [survey_coding.md](survey_coding.md) | style libraries, survey code files, coding a survey, the built-in customisation | `katana_archive12d` (customisation), `src/katana_cad/customisation/`, the Survey Code Manager |
| [cad.md](cad.md) | the engine both front ends drive: document, selection, snapping, the command interpreter, the spatial index, alignments, corridors, parcels, grading, plotting arithmetic, style and symbol resolution | `katana_cad` (outside `tools/`) |
| [desktop.md](desktop.md) | the window: theme, icons, toolbars, the workspace and docks, panels, the managers, the rules a dialog follows | `src/katana_qt` (outside `tools/` and `survey/`) |
| [annotation.md](annotation.md) | the annotation system: paper-sized text and the annotation scale, text styles, labels and their templates, the placer, auto-labelling rules, associativity, the dimension kinds, leaders and callouts, their verbs, managers, storage and DXF export | `include/katana/cad/annotation/`, `src/katana_cad/annotation/`, `entity/annotation.hpp`, `label_text.hpp`, `label_values.hpp`, `src/katana_qt/annotation/` |
| [drawing.md](drawing.md) | the drawing system: curve polylines, ellipses and splines, which kind a polyline is stored as, grips, the vertex tools and panel, precision input, the snaps, the drawing verbs | `include/katana/cad/drawing/`, `src/katana_cad/drawing/`, `src/katana_qt/drawing/`, `curves2d.hpp`, `polyline_vertices.hpp`, `entity/curve_pieces.hpp` |
| [tools.md](tools.md) | the interactive drawing tools, their catalogue and the tool host | `src/katana_cad/tools/`, `src/katana_qt/tools/`, `interactive_tool.hpp` |
| [headless.md](headless.md) | `katana_cli`, the window's headless switches and steps, `check_screenshot.cmake`'s variables | `src/katana_qt/main.cpp`, `tools/check_screenshot.cmake`, `tools/check_plot.cmake` |
| [mcp.md](mcp.md) | `katana_mcp`, the MCP server that lets Claude run projects in Katana: connecting a client, the tools, what the server adds to the command line | `src/katana_app/mcp_server.cpp`, `src/katana_app/mcp_main.cpp`, a verb in `src/katana_app/session.cpp` |
| [audit/2026-09-23-defects.md](audit/2026-09-23-defects.md) | the audit's defect register, one ID per defect | mark a defect FIXED in the commit that fixes it (`tools/audit_register.py`) |
| [audit/2026-09-23-findings.md](audit/2026-09-23-findings.md) | the audit's other findings | - |

Being written, listed so that the map is complete when they land:

| Document | Covers |
|---|---|
| [plan_view.md](plan_view.md) | the plan view |
| [plotting.md](plotting.md) | plotting: sheets, frames and title blocks (taking over `cad.md`'s "Plotting to PDF") |
| [gpu.md](gpu.md) | GPU drawing |
| [dxf.md](dxf.md) | DXF exchange |
