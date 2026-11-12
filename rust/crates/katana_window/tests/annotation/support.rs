//! What the annotation twins share: a dialog drawn in the harness with a recording host over a
//! drawing (`Bench`), the C++ tests' `Drawing::run` (a setup line run by the host's own
//! interpreter, not handed through the dialog), and the readers of what a frame drew - by the C++
//! object names, as `findChild` read the widgets.

#![allow(dead_code, reason = "Each case uses only some of the bench's readers.")]

use katana_cad::Document;
use katana_entity::{EntityId, Geometry, LeaderGeometry};
use katana_window::annotation::{
    LabelEditDialog, LabelLayoutReportDialog, LabelStyleManagerDialog, LeaderManagerDialog,
    TextEditDialog, TextStyleManagerDialog,
};
use katana_window::drive::{ControlRecord, Harness};
use katana_window::host::{Answer, Host, RecordingHost, VerbOutcome};

/// A dialog the harness draws: its object name and its frame.
pub trait Dialog {
    const NAME: &'static str;
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool;
}

impl Dialog for TextStyleManagerDialog {
    const NAME: &'static str = "textStyleManagerDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

impl Dialog for LabelStyleManagerDialog {
    const NAME: &'static str = "labelStyleManagerDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

impl Dialog for LabelEditDialog {
    const NAME: &'static str = "labelEditDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

impl Dialog for LabelLayoutReportDialog {
    const NAME: &'static str = "labelLayoutDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

impl Dialog for LeaderManagerDialog {
    const NAME: &'static str = "leaderManagerDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

impl Dialog for TextEditDialog {
    const NAME: &'static str = "textEditDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

/// A host that runs every line it is handed through its interpreter on its drawing and hands back
/// what came of it: the C++ tests' Runner and Drawing::context.
pub fn interpreter_host() -> RecordingHost {
    RecordingHost::new().answering(Answer::Interpreter)
}

/// A host that records every line and answers it ok without running it: the C++ tests' Drawing
/// with `execute = false`, to see the dialog change nothing itself.
pub fn recording_only_host() -> RecordingHost {
    RecordingHost::new().answering(Answer::Fixed(VerbOutcome::new(true, "", "")))
}

/// `line` run by the host's own interpreter on its drawing, not through the dialog (the C++
/// tests' `run` and `Drawing::run`): the first id it made, or 0. A refusal fails the test.
pub fn run(host: &mut RecordingHost, line: &str) -> EntityId {
    let parts = &mut host.parts;
    if let Err(error) = parts.interpreter.run(&mut parts.document, line) {
        panic!("{line} -> {}", error.describe());
    }
    parts
        .document
        .last_created_entities()
        .first()
        .copied()
        .unwrap_or(0)
}

/// The undo steps the drawing has.
pub fn steps(host: &RecordingHost) -> usize {
    host.document().history().undo_count()
}

/// Leader `id` of the drawing.
pub fn leader(host: &RecordingHost, id: EntityId) -> LeaderGeometry {
    match host
        .document()
        .model()
        .entities
        .find(id)
        .map(|entity| &entity.geometry)
    {
        Some(Geometry::Leader(leader)) => leader.clone(),
        other => panic!("{id} is no leader: {other:?}"),
    }
}

/// A dialog drawn in the headless harness with its host.
pub struct Bench<D: Dialog> {
    pub harness: Harness,
    pub host: RecordingHost,
    pub dialog: D,
}

impl<D: Dialog> Bench<D> {
    /// `make` given the host's drawing, then shown; the acts after go to the dialog.
    pub fn new(host: RecordingHost, make: impl FnOnce(&Document) -> D) -> Self {
        let dialog = make(host.document());
        let mut bench = Self {
            harness: Harness::new(),
            host,
            dialog,
        };
        bench.draw();
        bench.harness.target(D::NAME);
        bench
    }

    /// Frames until the window is idle: the C++'s processEvents, and a refusal fails the test.
    pub fn draw(&mut self) {
        self.frames().expect("the dialog is drawn");
    }

    fn frames(&mut self) -> Result<(), String> {
        let Self {
            harness,
            host,
            dialog,
        } = self;
        harness.run(|ui| {
            let ctx = ui.ctx().clone();
            dialog.draw(&ctx, host);
        })
    }

    /// `--fill NAME=TEXT`, and the frames after it.
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        if let Err(refusal) = self.frames() {
            panic!("{name}={text}: {refusal}");
        }
    }

    /// `--press NAME`, and the frames after it.
    pub fn press(&mut self, name: &str) {
        self.harness.press(name);
        if let Err(refusal) = self.frames() {
            panic!("{name}: {refusal}");
        }
    }

    /// The control `name` as the last frame drew it.
    pub fn control(&self, name: &str) -> ControlRecord {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("{name} is drawn"))
    }

    /// What `--report NAME` says: a label's text, a field's, a button's.
    pub fn report(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|refusal| panic!("{refusal}"))
    }

    /// What a field, a choice or a number box holds as it shows it.
    pub fn value(&self, name: &str) -> String {
        self.control(name).value
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    pub fn checked(&self, name: &str) -> bool {
        self.control(name).checked == Some(true)
    }

    /// The cells of row `row` of the list or table `name`.
    pub fn row(&self, name: &str, row: usize) -> Vec<String> {
        let control = self.control(name);
        let line = control
            .items
            .get(row)
            .unwrap_or_else(|| panic!("{name} has a row {row}"));
        line.split(" | ").map(String::from).collect()
    }

    /// How many rows the list or table `name` has.
    pub fn rows(&self, name: &str) -> usize {
        self.control(name).items.len()
    }

    /// The rows of a values table, "name=text" (the C++'s valueRows).
    pub fn value_rows(&self, name: &str) -> Vec<String> {
        (0..self.rows(name))
            .map(|row| self.row(name, row).join("="))
            .collect()
    }

    /// The C++ tests' `Drawing::run`.
    pub fn run(&mut self, line: &str) -> EntityId {
        run(&mut self.host, line)
    }

    pub fn steps(&self) -> usize {
        steps(&self.host)
    }

    pub fn document(&self) -> &Document {
        self.host.document()
    }

    pub fn document_mut(&mut self) -> &mut Document {
        self.host.document_mut()
    }

    /// The lines the dialog handed to the host.
    pub fn lines(&self) -> &[String] {
        self.host.lines()
    }
}
