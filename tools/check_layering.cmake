# The layering table: which layer may depend on which (docs/architecture.md),
# and why where the reason is not obvious.
#
# It is DATA, read by tools/check_rust_layering.py (the `rust_layering` test),
# which holds the crates of crates/ to it: a crate's katana_* dependencies
# are its own layer's crates or the layers listed for it here, and no crate
# re-exports a third-party crate (`pub use`), which would put a third-party
# type in its public interface.
#
# It was also a CMake script that read the C++ sources' #include lines against
# the same table. The C++ is gone (2026-11-15) and with it that half; the file
# keeps its name because the crates' sources, their manifests and the
# documents cite it as where the rule is written. Run as a script it now
# checks nothing, and says so, so that a test still registering it fails
# rather than passing with nothing checked.
#
# Each layer is one crate, katana_<layer>, with two names that differ: the qt
# layer's crate is katana_window (the window is egui and wgpu, not Qt), and
# katana_updater, the helper program, is a crate of the update layer.

# Layer -> layers it may depend on (transitively closed by hand, lowest first).
set(KATANA_ALLOWED_core "")
set(KATANA_ALLOWED_math "core")
set(KATANA_ALLOWED_geometry "core;math")
set(KATANA_ALLOWED_geodesy "core;math")
set(KATANA_ALLOWED_survey "core;math")
# surveyio reads manufacturer field data - Leica, Trimble, Topcon, LandXML, a
# coordinate CSV - and produces katana survey values, nothing else.
#
# geodesy is deliberately ABSENT, and that is the module's whole discipline:
# surveyio records the coordinate system a file DECLARES and never transforms
# anything. A transformation needs both of its ends, and an importer knows only
# one of them, so transforming on import moves the survey silently.
set(KATANA_ALLOWED_surveyio "core;math;geometry;survey")
set(KATANA_ALLOWED_terrain "core;math;geometry")
set(KATANA_ALLOWED_entity "core;math;geometry")
set(KATANA_ALLOWED_commands "core;math;geometry;entity")
# The archive module: the .12da Archive, and the survey coding libraries beside it.
# Pure text handling plus the mapping onto entities, alignments and surfaces, so
# it sits beside commands and needs no GDAL.
set(KATANA_ALLOWED_archive12d "core;math;geometry;terrain;entity")
# dxf: the drawing exchange format, read and written natively. Text handling
# plus the mapping onto entities and their tables, beside archive12d and for
# the same reason: no GDAL. It sees commands for one thing, the import as the
# single undoable step both front ends execute, so that they cannot differ
# about it.
set(KATANA_ALLOWED_dxf "core;math;geometry;entity;commands")
# ifc: IFC 4.3 export, written natively beside dxf and for its reason: no
# third-party library. It sees survey for the AS 5488 grading of the services
# it writes (docs/ifc.md), terrain for the surfaces, and commands for one
# thing - the import as the single undoable step both front ends execute, as
# dxf does.
set(KATANA_ALLOWED_ifc "core;math;geometry;terrain;entity;commands;survey")
set(KATANA_ALLOWED_storage "core;math;geometry;entity;survey")
# gis reads and writes through GDAL; pointcloud reads through PDAL, which loads
# GDAL itself.
set(KATANA_ALLOWED_gis "core;pointcloud")
set(KATANA_ALLOWED_pointcloud "core")
# update: the auto-update engine (docs/update.md) and katana_updater, the helper
# program that swaps the install's files. Core only, so that the helper links
# nothing else - no GDAL, no window, no application layer - and stays small and
# able to run from a temporary folder while the install it replaces is being
# swapped. The network is injected, so it never sees libcurl either.
set(KATANA_ALLOWED_update "core")
# interop converts what gis and pointcloud read into the domain model, so it is
# the only layer allowed to see both the external readers and the entity model.
set(KATANA_ALLOWED_interop
    "core;math;geometry;terrain;entity;commands;archive12d;gis;pointcloud")
# render sits beside terrain: it consumes a DrawList of plain geometry and has
# no idea an Entity or a Document exists, which is Rule 3 made structural.
set(KATANA_ALLOWED_render "core;math;geometry")
# cad deliberately may NOT see interop: keeping GDAL and PDAL out of the core
# application layer keeps them out of everything a verb of the interpreter is
# built from. Reference data is owned by app and the window instead.
#
# cad deliberately may NOT see surveyio either, for the same shape of reason: the
# CAD core must not depend on manufacturer structures. app and the window own
# the parsers, and what reaches cad is katana survey values - points,
# observations, stations - so that adding a fifth instrument vendor cannot
# change anything cad is built against, and removing one cannot break it.
set(KATANA_ALLOWED_cad
    "core;math;geometry;geodesy;survey;terrain;render;entity;commands;storage")
set(KATANA_ALLOWED_app
    "core;math;geometry;geodesy;survey;surveyio;terrain;render;entity;commands;storage;cad;archive12d;dxf;ifc;gis;pointcloud;interop;update")
set(KATANA_ALLOWED_qt
    "core;math;geometry;geodesy;survey;surveyio;terrain;render;entity;commands;storage;cad;archive12d;dxf;ifc;gis;pointcloud;interop;update;app;gpu")
# The GPU renderer (katana_gpu, docs/gpu.md) is the software rasteriser's twin
# behind the same seam: it may see a DrawList, a Camera and the point cloud's
# plain types, and never a Document - so it gets render's rule, not the
# window's.
set(KATANA_ALLOWED_gpu "core;math;geometry;render;pointcloud")

if(CMAKE_SCRIPT_MODE_FILE STREQUAL CMAKE_CURRENT_LIST_FILE)
    message(FATAL_ERROR
        "tools/check_layering.cmake is the layering table and checks nothing when run: the C++ "
        "sources it once read are gone. The crates are held to it by tools/check_rust_layering.py.")
endif()
