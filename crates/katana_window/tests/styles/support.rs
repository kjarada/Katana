//! What the styles twins share: a dialog or a tab drawn in the harness with a recording host over
//! a drawing (`Bench`), the hosts the C++ tests' runners stand for, and the readers of what a
//! frame drew - by the C++ object names, as `findChild` read the widgets.

#![allow(dead_code, reason = "Each case uses only some of the bench's readers.")]

use katana_cad::Document;
use katana_commands::CommandPtr;
use katana_entity::EntityId;
use katana_window::customisation::{DrawingSummaryDialog, GlobalModifyDialog, HatchPatternsTab};
use katana_window::drive::{ControlRecord, Harness, WINDOW};
use katana_window::host::{Answer, Host, RecordingHost, VerbOutcome};
use katana_window::layer_manager::{LAYER_MANAGER, LayerManagerDialog};
use katana_window::style_manager::{STYLE_MANAGER, StyleManagerDialog};

/// What the harness draws: the target its controls are in, and its frame.
pub trait Drawn {
    const TARGET: &'static str;
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn Host);
}

impl Drawn for DrawingSummaryDialog {
    const TARGET: &'static str = "drawingSummaryDialog";
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let ctx = ui.ctx().clone();
        self.ui(&ctx, host);
    }
}

impl Drawn for GlobalModifyDialog {
    const TARGET: &'static str = "globalModifyDialog";
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let ctx = ui.ctx().clone();
        self.ui(&ctx, host);
    }
}

impl Drawn for LayerManagerDialog {
    const TARGET: &'static str = LAYER_MANAGER;
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let ctx = ui.ctx().clone();
        self.ui(&ctx, host);
    }
}

impl Drawn for StyleManagerDialog {
    const TARGET: &'static str = STYLE_MANAGER;
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let ctx = ui.ctx().clone();
        self.ui(&ctx, host);
    }
}

/// The tab on its own, as the C++ tests show it as a window of its own: drawn in the window.
impl Drawn for HatchPatternsTab {
    const TARGET: &'static str = WINDOW;
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        self.ui(ui, host);
    }
}

/// A host that runs every line it is handed through its interpreter on its drawing and hands back
/// what came of it: the C++ fixtures' runner.
pub fn interpreter_host() -> RecordingHost {
    RecordingHost::new().answering(Answer::Interpreter)
}

/// A host answering every line with `outcome`.
pub fn fixed_host(outcome: VerbOutcome) -> RecordingHost {
    RecordingHost::new().answering(Answer::Fixed(outcome))
}

/// `line` run by the host's own interpreter on its drawing, not through the dialog (the C++
/// fixtures' `interpreter.run`). A refusal fails the test.
pub fn run(host: &mut RecordingHost, line: &str) {
    let parts = &mut host.parts;
    if let Err(error) = parts.interpreter.run(&mut parts.document, line) {
        panic!("{line} -> {}", error.describe());
    }
}

/// `command` executed on the host's drawing (the C++ fixtures' `must`).
pub fn must(host: &mut RecordingHost, command: CommandPtr) {
    if let Err(error) = host.document_mut().execute(command) {
        panic!("{}", error.describe());
    }
}

/// `command` executed, and the first entity it made.
pub fn create(host: &mut RecordingHost, command: CommandPtr) -> EntityId {
    must(host, command);
    host.document().last_created_entities()[0]
}

/// The undo steps the drawing has.
pub fn steps(host: &RecordingHost) -> usize {
    host.document().history().undo_count()
}

/// A dialog or a tab drawn in the headless harness with its host.
pub struct Bench<D: Drawn> {
    pub harness: Harness,
    pub host: RecordingHost,
    pub dialog: D,
}

impl<D: Drawn> Bench<D> {
    /// `make` given the host's drawing, then shown; the acts after go to what was made.
    pub fn new(host: RecordingHost, make: impl FnOnce(&Document) -> D) -> Self {
        let dialog = make(host.document());
        Self::showing(host, dialog)
    }

    /// `dialog`, made over whatever drawing the test chose, shown with `host`.
    pub fn showing(host: RecordingHost, dialog: D) -> Self {
        let mut bench = Self {
            harness: Harness::new(),
            host,
            dialog,
        };
        bench.harness.target(D::TARGET);
        bench.draw();
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
        harness.run(|ui| dialog.draw(ui, host))
    }

    /// `--fill NAME=TEXT`, and the frames after it.
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        if let Err(refusal) = self.frames() {
            panic!("{name}={text}: {refusal}");
        }
    }

    /// `--press NAME`, and the frames after it: the C++ tests' click, which asserts the button is
    /// enabled - a disabled one refuses the press, and fails here.
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

    /// Whether a control `name` was drawn at all.
    pub fn has(&self, name: &str) -> bool {
        self.harness.control(name).is_some()
    }

    /// A label's or a button's own words, as written.
    pub fn text(&self, name: &str) -> String {
        self.control(name).text
    }

    /// What a field, a choice or a number box holds as it shows it.
    pub fn value(&self, name: &str) -> String {
        self.control(name).value
    }

    /// What a number box holds.
    pub fn number(&self, name: &str) -> f64 {
        let shown = self.value(name);
        shown
            .parse()
            .unwrap_or_else(|_| panic!("{name} shows {shown}"))
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    /// QWidget::isHidden: its owner hid it.
    pub fn hidden(&self, name: &str) -> bool {
        !self.control(name).visible
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

    /// How many rows the list or table `name` has, or items a choice.
    pub fn rows(&self, name: &str) -> usize {
        self.control(name).items.len()
    }

    /// The rows of a list, or the items of a choice, as they read.
    pub fn items(&self, name: &str) -> Vec<String> {
        self.control(name).items
    }

    pub fn steps(&self) -> usize {
        steps(&self.host)
    }

    pub fn document(&self) -> &Document {
        self.host.document()
    }

    /// The lines the dialog handed to the host.
    pub fn lines(&self) -> &[String] {
        self.host.lines()
    }

    /// The last line handed to the host; empty before any.
    pub fn last_line(&self) -> String {
        self.host.lines().last().cloned().unwrap_or_default()
    }
}
