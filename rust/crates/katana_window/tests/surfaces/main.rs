//! TR's cases (step 7 wave 2): the twins of tests/qt_widgets/terrain's test_surfaces_panel - one
//! module a C++ file, the suites as modules inside.
//!
//! The panel is drawn in the headless harness (drive::Harness) with a recording host whose runner
//! is the interpreter on the host's drawing, as the C++ tests' runner was. What the C++ did to a
//! table item through Qt's own class - its check state, its text, a row's combo - is done through
//! the panel's own setter, which is what the item's change called; each such place says so.
//!
//! Not twinned here, each waiting for a later wave's module:
//!
//! * SurfacesPanelWidget.ItsDockIsReachedFromViewPanelsAndTheTerrainMenu and
//!   SurfacesPanelWidget.ViewPanelsSurfacesBringsThePanelToTheFrontOfItsTabGroup install the
//!   panel's dock in a main window with its menus and tab groups: the main window's (K).
//! * test_surface_edit_dialogs, test_surface_style_manager and test_volumes_dialog: their modules
//!   are not ported yet (see the slice's report).

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

mod surfaces_panel;

use katana_window::host::RecordingHost;
use katana_window::terrain::surfaces_panel::SurfacesPanel;

impl support::Drawn for SurfacesPanel {
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut RecordingHost) {
        self.ui(ui, host);
    }
}
