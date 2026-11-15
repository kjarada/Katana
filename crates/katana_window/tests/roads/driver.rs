//! What the road dialogs' twins share: a dialog drawn by the harness with its host, driven by the
//! C++ object names as the C++ tests' `fill`, `choose`, `click` and `label` drove them, and the
//! C++ tests' executor (the interpreter on the same drawing).

#![allow(
    dead_code,
    reason = "Each twin module uses only some of the driver's helpers."
)]

use egui::Ui;
use katana_cad::Document;
use katana_window::alignment_manager::AlignmentManagerDialog;
use katana_window::bim::corridor_dialog::CorridorDialog;
use katana_window::bim::corridor_quantities_dialog::CorridorQuantitiesDialog;
use katana_window::bim::road_object_dialog::RoadObjectDialog;
use katana_window::bim::road_object_manager::RoadObjectManagerDialog;
use katana_window::bim::road_template_manager::RoadTemplateManagerDialog;
use katana_window::drive::{ControlKind, ControlRecord, Harness, WorkbenchHost};
use katana_window::host::{Host, RecordingHost};
use katana_window::shell::Workbench;

/// Something the harness draws with a host: a dialog, or the stand-in window of a workbench.
pub trait Shown {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host);
}

impl Shown for AlignmentManagerDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Shown for CorridorDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Shown for CorridorQuantitiesDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Shown for RoadObjectDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Shown for RoadObjectManagerDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl Shown for RoadTemplateManagerDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

impl<W: Workbench> Shown for WorkbenchHost<W> {
    fn draw(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

/// Runs `line` on the host's interpreter and drawing directly, as the C++ tests'
/// `executor.ok(line)` and `site.run(line)` did: not handed to the dialog's executor, so not one
/// of its lines.
#[track_caller]
pub fn ok(host: &mut RecordingHost, line: &str) {
    let parts = &mut host.parts;
    if let Err(error) = parts.interpreter.run(&mut parts.document, line) {
        panic!("{line}: {}", error.describe());
    }
}

/// A dialog, its host and the harness drawing them, its acts addressed to the dialog's target.
pub struct Driven<D: Shown> {
    pub harness: Harness,
    pub host: RecordingHost,
    pub dialog: D,
    pub target: &'static str,
}

impl<D: Shown> Driven<D> {
    /// The dialog shown on the headless window's screen, its first frames run.
    #[track_caller]
    pub fn new(host: RecordingHost, dialog: D, target: &'static str) -> Self {
        Self::on(Harness::new(), host, dialog, target)
    }

    /// The same on `harness`'s screen.
    #[track_caller]
    pub fn on(harness: Harness, host: RecordingHost, dialog: D, target: &'static str) -> Self {
        let mut driven = Self {
            harness,
            host,
            dialog,
            target,
        };
        driven.settle();
        driven
    }

    /// Frames until the window is idle: the C++'s processEvents, the watcher's delivery with it.
    #[track_caller]
    pub fn settle(&mut self) {
        if let Err(problem) = self.try_settle() {
            panic!("{problem}");
        }
    }

    pub fn try_settle(&mut self) -> Result<(), String> {
        let Self {
            harness,
            host,
            dialog,
            ..
        } = self;
        harness.run(|ui| dialog.draw(ui, host))
    }

    /// `fill` / `choose` / setChecked on a check box ("on" or "off"): the field `name` given
    /// `text`, as a person types or chooses it.
    #[track_caller]
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.target(self.target).fill(name, text);
        self.settle();
    }

    /// `click`: the button `name` pressed, refused as the C++'s helper refused a disabled one.
    #[track_caller]
    pub fn press(&mut self, name: &str) {
        self.harness.target(self.target).press(name);
        self.settle();
    }

    /// The control `name` as the last frame drew it.
    #[track_caller]
    pub fn control(&self, name: &str) -> ControlRecord {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    /// What the control `name` says: a label's text, a field's or a text box's text, a choice's
    /// current text.
    #[track_caller]
    pub fn text(&self, name: &str) -> String {
        let control = self.control(name);
        match control.kind {
            ControlKind::Label => control.text,
            _ => control.value,
        }
    }

    /// How many rows the list or table `name` holds.
    #[track_caller]
    pub fn rows(&self, name: &str) -> usize {
        self.control(name).items.len()
    }

    #[track_caller]
    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    /// Every line handed to the dialog's executor, in order.
    pub fn lines(&self) -> &[String] {
        self.host.lines()
    }

    /// The last line handed to the executor; empty before any.
    pub fn last(&self) -> String {
        self.host.lines().last().cloned().unwrap_or_default()
    }

    /// `executor.ok(line)`: the line run on the drawing directly, not through the dialog.
    #[track_caller]
    pub fn ok(&mut self, line: &str) {
        ok(&mut self.host, line);
    }

    pub fn document(&self) -> &Document {
        self.host.document()
    }

    pub fn document_mut(&mut self) -> &mut Document {
        self.host.document_mut()
    }
}
