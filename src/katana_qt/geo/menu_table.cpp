// The geoprocessing menu items (geo_workbench.hpp, docs/desktop.md): one
// block per package of the geoprocessing plan, each adding its actions
// through GeoMenus and nowhere else, so packages built side by side never
// edit the same lines of main_window.cpp.
//
// Where each goes (docs/geoprocessing.md, "Window"):
//   GIS > Processing - GDAL   gdalToolbox (X1), gisFormats (I2)
//   GIS > Analysis - GDAL     gisBuffer, gisDissolve (V1), gisOverlay (V2),
//                             gisHull, gisClip (V3), gisSql (V5)
//   GIS > Check - GDAL        gisCheck, gisRepair, gisCoverage (V4)
//   Terrain > Analysis        terrainContours (T1), terrainShading (T2),
//                             terrainSlope (T3), terrainZonal, terrainDrape
//                             (T4), terrainViewshed (T5)
//   Terrain > DEM             terrainGrid (T6), terrainDemTools (T7)
// Each item opens its dialog, which builds the line and runs it through the
// window's one executor (GeoServices::run).

#include "geo/geo_workbench.hpp"

#include <utility>

// ---- V1: GIS > Analysis - GDAL > Buffer, Dissolve ----
#include "geo/buffer_dialog.hpp"
#include "geo/dissolve_dialog.hpp"
#include "geo/gis_tool_dialog.hpp"
// ---- V2: GIS > Analysis - GDAL > Overlay ----
#include "geo/overlay_dialog.hpp"
// ---- V3: GIS > Analysis - GDAL > Hull, Clip ----
#include "geo/clip_dialog.hpp"
#include "geo/hull_dialog.hpp"
// ---- V4: GIS > Check - GDAL > Check, Repair, Coverage ----
#include "geo/coverage_dialog.hpp"
#include "geo/geometry_check_dialog.hpp"
// ---- V5: GIS > Analysis - GDAL > Query with SQL ----
#include "geo/sql_dialog.hpp"

namespace katana::qt {

void buildGeoMenus(GeoMenus& menus, GeoWorkbench& workbench)
{
    (void)menus;
    (void)workbench;
    // ---- F0: GDAL algorithm bridge: no item; the typed GDAL line (X1 is its menu item) ----
    // ---- X1: GIS > Processing - GDAL > GDAL Toolbox ----
    // ---- X2: the toolbox's pipeline tab (no item of its own) ----
    // ---- I2: GIS > Processing - GDAL > Formats ----
    // ---- V1: GIS > Analysis - GDAL > Buffer, Dissolve ----
    addGisToolAction(menus, workbench, "Analysis - GDAL", Icon::Processing, "&Buffer...",
                     "Areas around what the scope takes: easements, setbacks (negative), "
                     "corridors, clearance zones - GIS BUFFER",
                     "gisBuffer", [](GisDialogContext context, QWidget* parent) {
                         return new GisBufferDialog(std::move(context), parent);
                     });
    addGisToolAction(menus, workbench, "Analysis - GDAL", Icon::Processing, "Di&ssolve...",
                     "Merge areas whose properties agree - superlots by owner, one clearance "
                     "zone - GIS DISSOLVE",
                     "gisDissolve", [](GisDialogContext context, QWidget* parent) {
                         return new GisDissolveDialog(std::move(context), parent);
                     });
    // ---- V2: GIS > Analysis - GDAL > Overlay ----
    addGisToolAction(menus, workbench, "Analysis - GDAL", Icon::Processing, "Ov&erlay...",
                     "Intersection, difference, union and the rest between two scopes, or a "
                     "scope and a file - easement area per lot, pipe length per lot - GIS OVERLAY",
                     "gisOverlay", [](GisDialogContext context, QWidget* parent) {
                         return new GisOverlayDialog(std::move(context), parent);
                     });
    // ---- V3: GIS > Analysis - GDAL > Hull, Clip ----
    addGisToolAction(menus, workbench, "Analysis - GDAL", Icon::Processing,
                     "Boundary Around Feat&ures...",
                     "The boundary around every point and vertex the scope takes - convex, or "
                     "concave to follow them in - GIS HULL",
                     "gisHull", [](GisDialogContext context, QWidget* parent) {
                         return new GisHullDialog(std::move(context), parent);
                     });
    addGisToolAction(menus, workbench, "Analysis - GDAL", Icon::Processing, "C&lip to Boundary...",
                     "Cut what the scope takes to the areas of a second scope or a file - the "
                     "pieces on a layer, or in place - GIS CLIP",
                     "gisClip", [](GisDialogContext context, QWidget* parent) {
                         return new GisClipDialog(std::move(context), parent);
                     });
    // ---- V4: GIS > Check - GDAL > Check, Repair, Coverage ----
    addGisToolAction(menus, workbench, "Check - GDAL", Icon::Processing, "Check &Geometry...",
                     "Invalid lines and areas - self-intersections and the like - each where it "
                     "is, optionally marked on a layer - GIS CHECK",
                     "gisCheck", [](GisDialogContext context, QWidget* parent) {
                         return new GisCheckDialog(std::move(context), parent);
                     });
    addGisToolAction(menus, workbench, "Check - GDAL", Icon::Processing, "Repair Geo&metry...",
                     "Make invalid areas valid in place, ids kept; a bow-tie becomes its two "
                     "triangles - GIS REPAIR",
                     "gisRepair", [](GisDialogContext context, QWidget* parent) {
                         return new GisRepairDialog(std::move(context), parent);
                     });
    addGisToolAction(menus, workbench, "Check - GDAL", Icon::Processing, "Gaps &and Overlaps...",
                     "Overlaps, enclosed gaps and edges that do not match between areas - and "
                     "cleaning them, only when asked - GIS COVERAGE",
                     "gisCoverage", [](GisDialogContext context, QWidget* parent) {
                         return new GisCoverageDialog(std::move(context), parent);
                     });
    // ---- V5: GIS > Analysis - GDAL > Query with SQL ----
    addGisToolAction(menus, workbench, "Analysis - GDAL", Icon::Processing, "Query with S&QL...",
                     "Ask the drawing a question in SQL - the easement area per owner - as rows, "
                     "a selection or new entities - GIS SQL",
                     "gisSql", [](GisDialogContext context, QWidget* parent) {
                         return new GisSqlDialog(std::move(context), parent);
                     });
    // ---- T1: Terrain > Analysis > Contours ----
    // ---- T2: Terrain > Analysis > Terrain Shading ----
    // ---- T3: Terrain > Analysis > Slope ----
    // ---- T4: Terrain > Analysis > Zonal Statistics, Drape ----
    // ---- T5: Terrain > Analysis > Viewshed ----
    // ---- T6: Terrain > DEM > Grid from Points ----
    // ---- T7: Terrain > DEM > DEM Tools ----
}

} // namespace katana::qt
