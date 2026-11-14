//! GB's cases (step 7 wave 2): the twins of tests/qt_widgets/geo's test_contours_dialog,
//! test_dem_tools_dialog, test_gdal_toolbox, test_grid_dem_dialog, test_pipeline_builder,
//! test_slope_analysis_dialog, test_surface_dialogs, test_terrain_shading_dialog,
//! test_viewshed_dialog and test_zonal_and_drape_dialogs - one module a C++ file, the suites as
//! modules inside.
//!
//! Each dialog is drawn in the headless harness (drive::Harness) with a recording host whose
//! runner answers as the C++ test's did: the widgets the C++ found by `findChild` are filled,
//! pressed and read by the same object names. What the C++ did to a widget through its own class
//! where the harness has no such act - a list item's check state, a combo's item data - is done
//! through the dialog's own setter, and each such place says so. A press of a disabled button is
//! refused by the harness, where Qt's click() did nothing: such a press is not made, and the case
//! asserts the button is disabled instead.

#![allow(
    clippy::float_cmp,
    reason = "Values are compared exactly where the C++ compared them exactly."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

pub mod support;

mod contours_dialog;
mod dem_tools_dialog;
mod gdal_toolbox;
mod grid_dem_dialog;
mod pipeline_builder;
mod slope_analysis_dialog;
mod surface_dialogs;
mod terrain_shading_dialog;
mod viewshed_dialog;
mod zonal_and_drape_dialogs;

use katana_window::geo::contours_dialog::ContoursDialog;
use katana_window::geo::dem_tools_dialog::DemToolsDialog;
use katana_window::geo::drape_dialog::DrapeDialog;
use katana_window::geo::gdal_toolbox_dialog::GdalToolboxDialog;
use katana_window::geo::grid_dem_dialog::GridDemDialog;
use katana_window::geo::pipeline_builder::PipelineBuilder;
use katana_window::geo::slope_analysis_dialog::SlopeAnalysisDialog;
use katana_window::geo::terrain_shading_dialog::TerrainShadingDialog;
use katana_window::geo::viewshed_dialog::ViewshedDialog;
use katana_window::geo::zonal_stats_dialog::ZonalStatsDialog;
use katana_window::host::Host;
use katana_window::surface_raster_dialog::{SurfaceFromDialog, SurfaceRasterDialog};

macro_rules! drawn {
    ($($dialog:ty),* $(,)?) => {
        $(impl<H: Host> support::Drawn<H> for $dialog {
            fn draw(&mut self, ui: &mut egui::Ui, host: &mut H) {
                self.ui(ui, host);
            }
        })*
    };
}

drawn!(
    ContoursDialog,
    DemToolsDialog,
    DrapeDialog,
    GdalToolboxDialog,
    GridDemDialog,
    PipelineBuilder,
    SlopeAnalysisDialog,
    SurfaceFromDialog,
    SurfaceRasterDialog,
    TerrainShadingDialog,
    ViewshedDialog,
    ZonalStatsDialog,
);
