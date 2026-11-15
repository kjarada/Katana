//! UT's cases (step 7 wave 1): the twins of tests/qt_widgets' test_ifc_dialogs,
//! test_project_crs_dialog and survey/test_utility_dialog - one module a C++ file, the suites as
//! modules inside. Each dialog is drawn by the drive harness and driven by the C++ object names, as
//! a person or the headless driver drives it; the window's side is a recording host, or a host of
//! the test's own where the C++ test's runner did more than record.
//!
//! What the C++ did to a widget a person could not reach - a control on a tab not in front, a
//! field disabled - is done here as a person would: the tab brought forward first, or the dialog's
//! own setter that the C++ call stood for. Each such place says so.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod ifc_dialogs;
mod project_crs_dialog;
mod utility_dialog;

use egui::Ui;
use katana_window::drive::{ControlRecord, Harness};
use katana_window::host::{Host, RecordingHost};
use katana_window::ifc_dialogs::{IfcExportDialog, IfcImportDialog};
use katana_window::project_crs_dialog::ProjectCrsDialog;
use katana_window::survey::utility_dialog::UtilityToolsDialog;

/// A dialog the harness draws with a host.
pub trait Drawn {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host);
}

impl Drawn for UtilityToolsDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Drawn for IfcExportDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Drawn for IfcImportDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Drawn for ProjectCrsDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

/// A shown dialog, its host and the harness that draws them, its acts addressed to the dialog's
/// target.
pub struct Bench<D: Drawn> {
    pub harness: Harness,
    pub dialog: D,
    pub host: RecordingHost,
}

impl<D: Drawn> Bench<D> {
    /// `dialog`, already shown, drawn once in the target `target`.
    pub fn new(target: &str, dialog: D, host: RecordingHost) -> Self {
        let mut harness = Harness::new();
        harness.target(target);
        let mut bench = Self {
            harness,
            dialog,
            host,
        };
        bench.draw();
        bench
    }

    /// Frames until the window is idle. An act refused - a press of a disabled button, which the
    /// C++'s click helper asserted against - fails the test with the C++'s sentence.
    pub fn draw(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        self.harness
            .run(|ui| dialog.draw(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// `--fill NAME=TEXT`: a field typed into, a choice made by its text, a box "on" or "off".
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.draw();
    }

    /// A check box ticked or cleared (QCheckBox::setChecked).
    pub fn tick(&mut self, name: &str, on: bool) {
        self.fill(name, if on { "on" } else { "off" });
    }

    /// `--press NAME`: a click.
    pub fn press(&mut self, name: &str) {
        self.harness.press(name);
        self.draw();
    }

    /// The control `name` as the last frame drew it.
    pub fn control(&self, name: &str) -> ControlRecord {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    /// What a label, a field or a text box shows (the C++ tests' text()).
    pub fn text(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }
}
