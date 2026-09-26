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

#include <QAction>
#include <QDialog>
#include <QMainWindow>

#include <utility>

// ---- T1: Terrain > Analysis > Contours ----
#include "geo/contours_dialog.hpp"
// ---- T2: Terrain > Analysis > Terrain Shading ----
#include "geo/terrain_shading_dialog.hpp"

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
    // ---- V2: GIS > Analysis - GDAL > Overlay ----
    // ---- V3: GIS > Analysis - GDAL > Hull, Clip ----
    // ---- V4: GIS > Check - GDAL > Check, Repair, Coverage ----
    // ---- V5: GIS > Analysis - GDAL > Query with SQL ----
    // ---- T1: Terrain > Analysis > Contours ----
    {
        QAction* contours = workbench.services().makeAction(
            Icon::Processing, "Contours...",
            "Contour lines of a surface or an elevation raster, drawn on layers (CONTOUR)", {},
            "terrainContours");
        contours->setData("contoursDialog");
        QObject::connect(contours, &QAction::triggered, &workbench.window(), [&workbench] {
            (void)showTerrainDialog(workbench, "contoursDialog",
                                    [](TerrainDialogContext context, QWidget* parent) -> QDialog* {
                                        return new ContoursDialog(std::move(context), parent);
                                    });
        });
        menus.addToTerrain("Analysis", "terrainAnalysisMenu", contours);
    }
    // ---- T2: Terrain > Analysis > Terrain Shading ----
    {
        QAction* shading = workbench.services().makeAction(
            Icon::Processing, "Terrain Shading...",
            "Hillshade, colour relief or slope shading of a surface or an elevation raster, kept "
            "as a reference raster with its legend (RASTER SHADE)",
            {}, "terrainShading");
        shading->setData("terrainShadingDialog");
        QObject::connect(shading, &QAction::triggered, &workbench.window(), [&workbench] {
            (void)showTerrainDialog(workbench, "terrainShadingDialog",
                                    [](TerrainDialogContext context, QWidget* parent) -> QDialog* {
                                        return new TerrainShadingDialog(std::move(context), parent);
                                    });
        });
        menus.addToTerrain("Analysis", "terrainAnalysisMenu", shading);
    }
    // ---- T3: Terrain > Analysis > Slope ----
    // ---- T4: Terrain > Analysis > Zonal Statistics, Drape ----
    // ---- T5: Terrain > Analysis > Viewshed ----
    // ---- T6: Terrain > DEM > Grid from Points ----
    // ---- T7: Terrain > DEM > DEM Tools ----
}

} // namespace katana::qt
