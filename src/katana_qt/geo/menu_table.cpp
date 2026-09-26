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
    // ---- V3: GIS > Analysis - GDAL > Hull, Clip ----
    // ---- V4: GIS > Check - GDAL > Check, Repair, Coverage ----
    // ---- V5: GIS > Analysis - GDAL > Query with SQL ----
    // ---- T1: Terrain > Analysis > Contours ----
    // ---- T2: Terrain > Analysis > Terrain Shading ----
    // ---- T3: Terrain > Analysis > Slope ----
    // ---- T4: Terrain > Analysis > Zonal Statistics, Drape ----
    // ---- T5: Terrain > Analysis > Viewshed ----
    // ---- T6: Terrain > DEM > Grid from Points ----
    // ---- T7: Terrain > DEM > DEM Tools ----
}

} // namespace katana::qt
