//! The geoprocessing menu items (geo/menu_table.cpp; geo_workbench, docs/desktop.md): one block
//! per package of the geoprocessing plan, each adding its actions through `GeoMenus` and nowhere
//! else, so packages built side by side never edit the same lines of the main window.
//!
//! ```text
//! Where each goes (docs/geoprocessing.md, "Window"):
//!   GIS > Processing          gdalToolbox (X1), gisFormats (I2)
//!   GIS > Analysis            gisBuffer, gisDissolve (V1), gisOverlay (V2),
//!                             gisHull, gisClip (V3), gisSql (V5)
//!   GIS > Check               gisCheck, gisRepair, gisCoverage (V4)
//!   Terrain > Analysis        terrainContours (T1), terrainShading (T2),
//!                             terrainSlope (T3), terrainZonal, terrainDrape
//!                             (T4), terrainViewshed (T5)
//!   Terrain > DEM             terrainGrid (T6), terrainDemTools (T7)
//! ```
//!
//! Each item opens its dialog, which builds the line and runs it through the window's one
//! executor (`Host::run`).
//!
//! THE RUST SHAPE. The C++ connected each QAction to a lambda that made the dialog as a child of
//! the window, found again by its object name. Here an item is a row (`Action`), and what it opens
//! is kept by `GeoMenuDialogs`, which the window asks to `trigger` an item by its name and to draw
//! its dialogs each frame: one of each dialog per window, made the first time and kept, as the
//! C++'s children were.

use egui::Ui;

use crate::drive::Action;
use crate::geo::buffer_dialog::GisBufferDialog;
use crate::geo::clip_dialog::GisClipDialog;
use crate::geo::contours_dialog::ContoursDialog;
use crate::geo::coverage_dialog::GisCoverageDialog;
use crate::geo::dem_tools_dialog::DemToolsDialog;
use crate::geo::dissolve_dialog::GisDissolveDialog;
use crate::geo::drape_dialog::DrapeDialog;
use crate::geo::formats_dialog::{KeptFormatsDialog, add_formats_item};
use crate::geo::gdal_toolbox_dialog::GdalToolboxDialog;
use crate::geo::geo_workbench::GeoMenus;
use crate::geo::geometry_check_dialog::{GisCheckDialog, GisRepairDialog};
use crate::geo::gis_tool_dialog::{GisDialogContext, GisTool, GisToolDialogs, add_gis_tool_action};
use crate::geo::grid_dem_dialog::GridDemDialog;
use crate::geo::hull_dialog::GisHullDialog;
use crate::geo::overlay_dialog::GisOverlayDialog;
use crate::geo::slope_analysis_dialog::SlopeAnalysisDialog;
use crate::geo::sql_dialog::GisSqlDialog;
use crate::geo::terrain_shading_dialog::TerrainShadingDialog;
use crate::geo::viewshed_dialog::ViewshedDialog;
use crate::geo::zonal_stats_dialog::ZonalStatsDialog;
use crate::host::{Host, JobId, VerbOutcome};

/// Terrain > A&nalysis: A is Cut Section Along Alignment's in the Terrain menu. Its six items'
/// letters (C, H, S, A, D, V) are each one item's;
/// qt_every_shortcut_and_menu_letter_reaches_one_thing_headless refuses a letter shared in one
/// menu, and GeoMenus.EveryItemTheLanesAddHasALetterOfItsOwn an item with none.
const ANALYSIS: &str = "A&nalysis";
const ANALYSIS_MENU: &str = "terrainAnalysisMenu";
const ANALYSIS_TIP: &str = "Contours, shading, slope, statistics by area, drape and viewshed of a \
                            surface or an elevation raster";
/// D&EM: D is Surface From Drawing's and M Alignment Manager's in the Terrain menu.
const DEM: &str = "D&EM";
const DEM_MENU: &str = "terrainDemMenu";
const DEM_TIP: &str = "Grid survey points to a DEM, and mosaic, clip, fill, footprint, reproject \
                       or difference DEMs";

/// The terrain dialogs, by the action that opens each.
const CONTOURS: &str = "terrainContours";
const SHADING: &str = "terrainShading";
const SLOPE: &str = "terrainSlope";
const ZONAL: &str = "terrainZonal";
const DRAPE: &str = "terrainDrape";
const VIEWSHED: &str = "terrainViewshed";
const GRID: &str = "terrainGrid";
const DEM_TOOLS: &str = "terrainDemTools";
const TOOLBOX: &str = "gdalToolbox";
const FORMATS: &str = "gisFormats";

/// What the geoprocessing items open: one of each dialog per window, made the first time its item
/// is triggered and kept (see the module comment).
#[derive(Default)]
pub struct GeoMenuDialogs {
    gis: GisToolDialogs,
    formats: KeptFormatsDialog,
    toolbox: Option<GdalToolboxDialog>,
    contours: Option<ContoursDialog>,
    shading: Option<TerrainShadingDialog>,
    slope: Option<SlopeAnalysisDialog>,
    zonal: Option<ZonalStatsDialog>,
    drape: Option<DrapeDialog>,
    viewshed: Option<ViewshedDialog>,
    grid: Option<GridDemDialog>,
    dem_tools: Option<DemToolsDialog>,
}

impl GeoMenuDialogs {
    pub fn new() -> Self {
        Self::default()
    }

    /// The GIS tool dialogs (buffer, dissolve, ...).
    pub fn gis(&mut self) -> &mut GisToolDialogs {
        &mut self.gis
    }

    /// What item `name` does when it is triggered: its dialog made the first time, and shown.
    /// False when `name` is none of the geoprocessing items.
    pub fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        // A dialog made the first time over the window, then shown with its lists read again.
        fn shown<D>(
            slot: &mut Option<D>,
            host: &mut dyn Host,
            make: impl FnOnce(&mut dyn Host) -> D,
            show: impl FnOnce(&mut D, &dyn Host),
        ) {
            if slot.is_none() {
                *slot = Some(make(&mut *host));
            }
            if let Some(dialog) = slot.as_mut() {
                show(dialog, &*host);
            }
        }
        match name {
            TOOLBOX => shown(&mut self.toolbox, host, GdalToolboxDialog::new, GdalToolboxDialog::show),
            FORMATS => self.formats.trigger(host),
            CONTOURS => shown(&mut self.contours, host, ContoursDialog::new, ContoursDialog::show),
            SHADING => shown(
                &mut self.shading,
                host,
                |host| TerrainShadingDialog::new(&*host),
                TerrainShadingDialog::show,
            ),
            SLOPE => shown(&mut self.slope, host, SlopeAnalysisDialog::new, SlopeAnalysisDialog::show),
            ZONAL => shown(&mut self.zonal, host, ZonalStatsDialog::new, ZonalStatsDialog::show),
            DRAPE => shown(&mut self.drape, host, DrapeDialog::new, DrapeDialog::show),
            VIEWSHED => shown(&mut self.viewshed, host, ViewshedDialog::new, ViewshedDialog::show),
            GRID => shown(&mut self.grid, host, GridDemDialog::new, GridDemDialog::show),
            DEM_TOOLS => shown(&mut self.dem_tools, host, DemToolsDialog::new, DemToolsDialog::show),
            _ => return self.gis.trigger(host, name),
        }
        true
    }

    /// The geo workbench's word that a job ended, for the GIS dialogs that wait on theirs.
    pub fn job_finished(&mut self, id: JobId, outcome: &VerbOutcome) {
        self.gis.job_finished(id, outcome);
    }

    /// One frame of every dialog made.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.gis.ui(ui, host);
        self.formats.ui(ui, host);
        if let Some(dialog) = &mut self.toolbox {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.contours {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.shading {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.slope {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.zonal {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.drape {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.viewshed {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.grid {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.dem_tools {
            dialog.ui(ui, host);
        }
    }
}

/// One of a package's terrain items: its object name, icon, text, tip and the dialog it opens.
fn terrain_item(name: &str, icon: &'static str, text: &str, tip: &str, dialog: &str) -> Action {
    Action::new(name, text).icon(icon).tip(tip).opens(dialog)
}

/// A GIS dialog over the window, boxed as GisToolDialogs keeps it.
fn boxed<D: GisTool + 'static>(dialog: D) -> Box<dyn GisTool> {
    Box::new(dialog)
}

/// Every package's menu items, from one table with a block per package (buildGeoMenus).
pub fn build_geo_menus(menus: &mut GeoMenus, dialogs: &mut GeoMenuDialogs) {
    // ---- F0: GDAL algorithm bridge: no item; the typed GDAL line (X1 is its menu item) ----
    // ---- X1: GIS > Processing > GDAL Toolbox ----
    // X: every other letter of "GDAL Toolbox" is another GIS item's once the lanes merge (T Import
    // Vector Data, D Export Surface as DEM, O COPC; and the vector lane's G, A, L and B), and
    // qt_every_shortcut_and_menu_letter_reaches_one_thing_headless refuses a letter shared in one
    // menu.
    menus.add_to_gis(
        "Processing",
        Action::new(TOOLBOX, "GDAL Toolbo&x...")
            .icon("Processing")
            .tip(
                "Every one of GDAL's algorithms, with forms made from its own arguments, run on \
                 the drawing, reference rasters, surfaces or files (the GDAL verb)",
            )
            .opens("gdalToolboxDialog"),
    );
    // ---- X2: the toolbox's pipeline tab (no item of its own) ----
    // ---- I2: GIS > Processing > Formats ----
    add_formats_item(menus);
    // ---- V1: GIS > Analysis > Buffer, Dissolve ----
    let gis = &mut dialogs.gis;
    add_gis_tool_action(
        menus,
        gis,
        "Analysis",
        "GisBuffer",
        "&Buffer...",
        "Areas around what the scope takes: easements, setbacks (negative), corridors, clearance \
         zones - GIS BUFFER",
        "gisBuffer",
        |context: GisDialogContext, host: &dyn Host| boxed(GisBufferDialog::new(context, host)),
    );
    add_gis_tool_action(
        menus,
        gis,
        "Analysis",
        "GisDissolve",
        "Di&ssolve...",
        "Merge areas whose properties agree - superlots by owner, one clearance zone - GIS \
         DISSOLVE",
        "gisDissolve",
        |context: GisDialogContext, host: &dyn Host| boxed(GisDissolveDialog::new(context, host)),
    );
    // ---- V2: GIS > Analysis > Overlay ----
    add_gis_tool_action(
        menus,
        gis,
        "Analysis",
        "GisOverlay",
        "Ov&erlay...",
        "Intersection, difference, union and the rest between two scopes, or a scope and a file - \
         easement area per lot, pipe length per lot - GIS OVERLAY",
        "gisOverlay",
        |context: GisDialogContext, host: &dyn Host| boxed(GisOverlayDialog::new(context, host)),
    );
    // ---- V3: GIS > Analysis > Hull, Clip ----
    add_gis_tool_action(
        menus,
        gis,
        "Analysis",
        "GisHull",
        "Boundary Around Feat&ures...",
        "The boundary around every point and vertex the scope takes - convex, or concave to \
         follow them in - GIS HULL",
        "gisHull",
        |context: GisDialogContext, host: &dyn Host| boxed(GisHullDialog::new(context, host)),
    );
    add_gis_tool_action(
        menus,
        gis,
        "Analysis",
        "GisClip",
        "C&lip to Boundary...",
        "Cut what the scope takes to the areas of a second scope or a file - the pieces on a \
         layer, or in place - GIS CLIP",
        "gisClip",
        |context: GisDialogContext, host: &dyn Host| boxed(GisClipDialog::new(context, host)),
    );
    // ---- V4: GIS > Check > Check, Repair, Coverage ----
    add_gis_tool_action(
        menus,
        gis,
        "Check",
        "GisCheck",
        "Check &Geometry...",
        "Invalid lines and areas - self-intersections and the like - each where it is, optionally \
         marked on a layer - GIS CHECK",
        "gisCheck",
        |context: GisDialogContext, host: &dyn Host| boxed(GisCheckDialog::new(context, host)),
    );
    add_gis_tool_action(
        menus,
        gis,
        "Check",
        "GisRepair",
        "Repair Geo&metry...",
        "Make invalid areas valid in place, ids kept; a bow-tie becomes its two triangles - GIS \
         REPAIR",
        "gisRepair",
        |context: GisDialogContext, host: &dyn Host| boxed(GisRepairDialog::new(context, host)),
    );
    add_gis_tool_action(
        menus,
        gis,
        "Check",
        "GisCoverage",
        "Gaps &and Overlaps...",
        "Overlaps, enclosed gaps and edges that do not match between areas - and cleaning them, \
         only when asked - GIS COVERAGE",
        "gisCoverage",
        |context: GisDialogContext, host: &dyn Host| boxed(GisCoverageDialog::new(context, host)),
    );
    // ---- V5: GIS > Analysis > Query with SQL ----
    add_gis_tool_action(
        menus,
        gis,
        "Analysis",
        "GisSql",
        "Query with S&QL...",
        "Ask the drawing a question in SQL - the easement area per owner - as rows, a selection or \
         new entities - GIS SQL",
        "gisSql",
        |context: GisDialogContext, host: &dyn Host| boxed(GisSqlDialog::new(context, host)),
    );
    // ---- T1: Terrain > Analysis > Contours ----
    menus.add_to_terrain(
        ANALYSIS,
        ANALYSIS_MENU,
        "TerrainContours",
        ANALYSIS_TIP,
        terrain_item(
            CONTOURS,
            "TerrainContours",
            "&Contours...",
            "Contour lines of a surface or an elevation raster, drawn on layers (CONTOUR)",
            "contoursDialog",
        ),
    );
    // ---- T2: Terrain > Analysis > Terrain Shading ----
    menus.add_to_terrain(
        ANALYSIS,
        ANALYSIS_MENU,
        "TerrainContours",
        ANALYSIS_TIP,
        terrain_item(
            SHADING,
            "TerrainShading",
            "Terrain S&hading...",
            "Hillshade, colour relief or slope shading of a surface or an elevation raster, kept \
             as a reference raster with its legend (RASTER SHADE)",
            "terrainShadingDialog",
        ),
    );
    // ---- T3: Terrain > Analysis > Slope ----
    menus.add_to_terrain(
        ANALYSIS,
        ANALYSIS_MENU,
        "TerrainContours",
        ANALYSIS_TIP,
        terrain_item(
            SLOPE,
            "TerrainSlope",
            "&Slope and Aspect...",
            "The slope or aspect of a surface or an elevation raster, and slope classes drawn as \
             areas (RASTER SLOPE, RASTER ASPECT)",
            "slopeAnalysisDialog",
        ),
    );
    // ---- T4: Terrain > Analysis > Statistics by Area, Drape and Sample Heights ----
    menus.add_to_terrain(
        ANALYSIS,
        ANALYSIS_MENU,
        "TerrainContours",
        ANALYSIS_TIP,
        terrain_item(
            ZONAL,
            "TerrainZonal",
            "Statistics by &Area...",
            "The mean, range, count and more of a surface or an elevation raster inside each \
             closed shape, written on the shapes as properties (RASTER ZONAL)",
            "zonalStatsDialog",
        ),
    );
    menus.add_to_terrain(
        ANALYSIS,
        ANALYSIS_MENU,
        "TerrainContours",
        ANALYSIS_TIP,
        terrain_item(
            DRAPE,
            "TerrainDrape",
            "&Drape and Sample Heights...",
            "Give points and line vertices the height of a surface or an elevation raster, or \
             read the height at points (DRAPE, RASTER SAMPLE)",
            "drapeDialog",
        ),
    );
    // ---- T5: Terrain > Analysis > Viewshed and Line of Sight ----
    menus.add_to_terrain(
        ANALYSIS,
        ANALYSIS_MENU,
        "TerrainContours",
        ANALYSIS_TIP,
        terrain_item(
            VIEWSHED,
            "TerrainViewshed",
            "&Viewshed and Line of Sight...",
            "What can be seen from observers over a surface or an elevation raster, and whether \
             one point can be seen from another (RASTER VIEWSHED, LOS)",
            "viewshedDialog",
        ),
    );
    // ---- T6: Terrain > DEM > Grid from Points ----
    menus.add_to_terrain(
        DEM,
        DEM_MENU,
        "GridDem",
        DEM_TIP,
        terrain_item(
            GRID,
            "GridDem",
            "&Grid Points to DEM...",
            "Grid the points in a scope - their heights, or a property - into a DEM kept as a \
             reference raster (RASTER GRID)",
            "gridDemDialog",
        ),
    );
    // ---- T7: Terrain > DEM > DEM Tools ----
    menus.add_to_terrain(
        DEM,
        DEM_MENU,
        "GridDem",
        DEM_TIP,
        terrain_item(
            DEM_TOOLS,
            "DemTools",
            "DEM &Tools...",
            "Mosaic, clip, fill, reproject, trace and difference DEMs, with cut and fill volumes \
             (RASTER MOSAIC, CLIP, FILL, REPROJECT, FOOTPRINT, DIFFERENCE)",
            "demToolsDialog",
        ),
    );
}
