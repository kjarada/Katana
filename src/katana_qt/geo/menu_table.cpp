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

#include "geo/dem_tools_dialog.hpp"
#include "geo/formats_dialog.hpp"
#include "geo/gdal_toolbox_dialog.hpp"
#include "geo/geo_workbench.hpp"
#include "geo/grid_dem_dialog.hpp"

#include <QAction>
#include <QDialog>
#include <QMainWindow>
#include <QWidget>

#include <utility>

// ---- T1: Terrain > Analysis > Contours ----
#include "geo/contours_dialog.hpp"
// ---- T2: Terrain > Analysis > Terrain Shading ----
#include "geo/terrain_shading_dialog.hpp"
// ---- T3: Terrain > Analysis > Slope ----
#include "geo/slope_analysis_dialog.hpp"
// ---- T4: Terrain > Analysis > Statistics by Area, Drape and Sample Heights ----
#include "geo/drape_dialog.hpp"
#include "geo/zonal_stats_dialog.hpp"
// ---- T5: Terrain > Analysis > Viewshed and Line of Sight ----
#include "geo/viewshed_dialog.hpp"

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
    // Terrain > A&nalysis: A is Cut Section Along Alignment's in the Terrain
    // menu. Its six items' letters (C, H, S, A, D, V) are each one item's;
    // qt_every_shortcut_and_menu_letter_reaches_one_thing_headless refuses a
    // letter shared in one menu, and GeoMenus.EveryItemTheLanesAddHasALetterOfItsOwn
    // an item with none.
    const QString kAnalysis = "A&nalysis";
    (void)menus;
    (void)workbench;
    // ---- F0: GDAL algorithm bridge: no item; the typed GDAL line (X1 is its menu item) ----
    // ---- X1: GIS > Processing - GDAL > GDAL Toolbox ----
    {
        // X: every other letter of "GDAL Toolbox" is another GIS item's once
        // the lanes merge (T Import Vector Data, D Export Surface as DEM, O
        // COPC; and the vector lane's G, A, L and B), and
        // qt_every_shortcut_and_menu_letter_reaches_one_thing_headless
        // refuses a letter shared in one menu.
        QAction* toolbox = workbench.services().makeAction(
            Icon::Processing, "GDAL Toolbo&x...",
            "Every one of GDAL's algorithms, with forms made from its own arguments, run on the "
            "drawing, reference rasters, surfaces or files (the GDAL verb)",
            {}, "gdalToolbox");
        toolbox->setData(QString("gdalToolboxDialog"));
        menus.addToGis("Processing - GDAL", toolbox);
        QObject::connect(toolbox, &QAction::triggered, toolbox, [&workbench, toolbox] {
            if (auto* window = qobject_cast<QWidget*>(toolbox->parent())) {
                showGdalToolboxDialog(workbench, *window);
            }
        });
    }
    // ---- X2: the toolbox's pipeline tab (no item of its own) ----
    // ---- I2: GIS > Processing - GDAL > Formats ----
    addFormatsItem(menus, workbench);
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
    {
        QAction* contours = workbench.services().makeAction(
            Icon::Processing, "&Contours...",
            "Contour lines of a surface or an elevation raster, drawn on layers (CONTOUR)", {},
            "terrainContours");
        contours->setData("contoursDialog");
        QObject::connect(contours, &QAction::triggered, &workbench.window(), [&workbench] {
            (void)showTerrainDialog(workbench, "contoursDialog",
                                    [](TerrainDialogContext context, QWidget* parent) -> QDialog* {
                                        return new ContoursDialog(std::move(context), parent);
                                    });
        });
        menus.addToTerrain(kAnalysis, "terrainAnalysisMenu", contours);
    }
    // ---- T2: Terrain > Analysis > Terrain Shading ----
    {
        QAction* shading = workbench.services().makeAction(
            Icon::Processing, "Terrain S&hading...",
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
        menus.addToTerrain(kAnalysis, "terrainAnalysisMenu", shading);
    }
    // ---- T3: Terrain > Analysis > Slope ----
    {
        QAction* slope = workbench.services().makeAction(
            Icon::Processing, "&Slope and Aspect...",
            "The slope or aspect of a surface or an elevation raster, and slope classes drawn as "
            "areas (RASTER SLOPE, RASTER ASPECT)",
            {}, "terrainSlope");
        slope->setData("slopeAnalysisDialog");
        QObject::connect(slope, &QAction::triggered, &workbench.window(), [&workbench] {
            (void)showTerrainDialog(workbench, "slopeAnalysisDialog",
                                    [](TerrainDialogContext context, QWidget* parent) -> QDialog* {
                                        return new SlopeAnalysisDialog(std::move(context), parent);
                                    });
        });
        menus.addToTerrain(kAnalysis, "terrainAnalysisMenu", slope);
    }
    // ---- T4: Terrain > Analysis > Statistics by Area, Drape and Sample Heights ----
    {
        QAction* zonal = workbench.services().makeAction(
            Icon::Processing, "Statistics by &Area...",
            "The mean, range, count and more of a surface or an elevation raster inside each "
            "closed shape, written on the shapes as properties (RASTER ZONAL)",
            {}, "terrainZonal");
        zonal->setData("zonalStatsDialog");
        QObject::connect(zonal, &QAction::triggered, &workbench.window(), [&workbench] {
            (void)showTerrainDialog(workbench, "zonalStatsDialog",
                                    [](TerrainDialogContext context, QWidget* parent) -> QDialog* {
                                        return new ZonalStatsDialog(std::move(context), parent);
                                    });
        });
        menus.addToTerrain(kAnalysis, "terrainAnalysisMenu", zonal);
        QAction* drape = workbench.services().makeAction(
            Icon::Processing, "&Drape and Sample Heights...",
            "Give points and line vertices the height of a surface or an elevation raster, or "
            "read the height at points (DRAPE, RASTER SAMPLE)",
            {}, "terrainDrape");
        drape->setData("drapeDialog");
        QObject::connect(drape, &QAction::triggered, &workbench.window(), [&workbench] {
            (void)showTerrainDialog(workbench, "drapeDialog",
                                    [](TerrainDialogContext context, QWidget* parent) -> QDialog* {
                                        return new DrapeDialog(std::move(context), parent);
                                    });
        });
        menus.addToTerrain(kAnalysis, "terrainAnalysisMenu", drape);
    }
    // ---- T5: Terrain > Analysis > Viewshed and Line of Sight ----
    {
        QAction* viewshed = workbench.services().makeAction(
            Icon::Processing, "&Viewshed and Line of Sight...",
            "What can be seen from observers over a surface or an elevation raster, and whether "
            "one point can be seen from another (RASTER VIEWSHED, LOS)",
            {}, "terrainViewshed");
        viewshed->setData("viewshedDialog");
        QObject::connect(viewshed, &QAction::triggered, &workbench.window(), [&workbench] {
            (void)showTerrainDialog(workbench, "viewshedDialog",
                                    [](TerrainDialogContext context, QWidget* parent) -> QDialog* {
                                        return new ViewshedDialog(std::move(context), parent);
                                    });
        });
        menus.addToTerrain(kAnalysis, "terrainAnalysisMenu", viewshed);
    }
    // ---- T6: Terrain > DEM > Grid from Points ----
    {
        QAction* grid = workbench.services().makeAction(
            Icon::Processing, "&Grid Points to DEM...",
            "Grid the points in a scope - their heights, or a property - into a DEM kept as a "
            "reference raster (RASTER GRID)",
            {}, "terrainGrid");
        // The dialog it opens, by object name: how --dialog finds it.
        grid->setData(QString("gridDemDialog"));
        // D&EM: D is Surface From Drawing's and M Alignment Manager's in the
        // Terrain menu, and a letter shared in one menu is refused by
        // qt_every_shortcut_and_menu_letter_reaches_one_thing_headless.
        menus.addToTerrain("D&EM", "terrainDemMenu", grid);
        QObject::connect(grid, &QAction::triggered, grid, [&workbench, grid] {
            if (auto* window = qobject_cast<QWidget*>(grid->parent())) {
                showGridDemDialog(workbench, *window);
            }
        });
    }
    // ---- T7: Terrain > DEM > DEM Tools ----
    {
        QAction* tools = workbench.services().makeAction(
            Icon::Processing, "DEM &Tools...",
            "Mosaic, clip, fill, reproject, trace and difference DEMs, with cut and fill volumes "
            "(RASTER MOSAIC, CLIP, FILL, REPROJECT, FOOTPRINT, DIFFERENCE)",
            {}, "terrainDemTools");
        tools->setData(QString("demToolsDialog"));
        menus.addToTerrain("D&EM", "terrainDemMenu", tools);
        QObject::connect(tools, &QAction::triggered, tools, [&workbench, tools] {
            if (auto* window = qobject_cast<QWidget*>(tools->parent())) {
                showDemToolsDialog(workbench, *window);
            }
        });
    }
}

} // namespace katana::qt
