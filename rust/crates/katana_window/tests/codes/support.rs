//! What the survey code tools' twins share: a dialog drawn in the harness with a recording host
//! over a drawing (`Bench`), and the readers of what a frame drew - by the C++ object names, as
//! `findChild` read the widgets.

#![allow(dead_code, reason = "Each case uses only some of the bench's readers.")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use katana_cad::Document;
use katana_window::customisation::definition_thumbnails::DefinitionThumbnails;
use katana_window::customisation::style_painter::CanvasRasterBackend;
use katana_window::customisation::{
    DefinitionEditorDialog, SurveyCodeManagerDialog, SymbolLibraryDialog,
};
use katana_window::drive::{ControlRecord, Harness};
use katana_window::host::{Host, RecordingHost};

/// A dialog the harness draws: its object name and its frame.
pub trait Dialog {
    const NAME: &'static str;
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool;
}

impl Dialog for SurveyCodeManagerDialog {
    const NAME: &'static str = "surveyCodeManagerDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

impl Dialog for DefinitionEditorDialog {
    const NAME: &'static str = "definitionEditorDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

impl Dialog for SymbolLibraryDialog {
    const NAME: &'static str = "symbolLibraryDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

/// A picture cache of a test's own, as a fixture's DefinitionThumbnails was.
pub fn thumbnails() -> Rc<RefCell<DefinitionThumbnails>> {
    Rc::new(RefCell::new(DefinitionThumbnails::new(Arc::new(
        CanvasRasterBackend,
    ))))
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
        self.try_draw().expect("the dialog is drawn");
    }

    pub fn try_draw(&mut self) -> Result<(), String> {
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
        if let Err(refusal) = self.try_draw() {
            panic!("{name}={text}: {refusal}");
        }
    }

    /// `--fill NAME=TEXT` to a control of the target `target` inside the dialog (a filter bar).
    pub fn fill_in(&mut self, target: &str, name: &str, text: &str) {
        self.harness.target(target);
        self.harness.fill(name, text);
        let drawn = self.try_draw();
        self.harness.target(D::NAME);
        if let Err(refusal) = drawn {
            panic!("{target}: {name}={text}: {refusal}");
        }
    }

    /// `--press NAME`, and the frames after it.
    pub fn press(&mut self, name: &str) {
        if let Err(refusal) = self.try_press(name) {
            panic!("{name}: {refusal}");
        }
    }

    /// `--press NAME`, the refusal handed back: a press of a button that is hidden or disabled.
    pub fn try_press(&mut self, name: &str) -> Result<(), String> {
        self.harness.press(name);
        self.try_draw()
    }

    /// The control `name` as the last frame drew it.
    pub fn control(&self, name: &str) -> ControlRecord {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("{name} is drawn"))
    }

    /// Whether the last frame drew a control `name`.
    pub fn has(&self, name: &str) -> bool {
        self.harness.control(name).is_some()
    }

    /// What a label says, a button is labelled, a field's placeholder.
    pub fn text(&self, name: &str) -> String {
        self.control(name).text
    }

    /// What a field, a choice or a number box holds as it shows it.
    pub fn value(&self, name: &str) -> String {
        self.control(name).value
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    pub fn visible(&self, name: &str) -> bool {
        self.control(name).visible
    }

    pub fn checked(&self, name: &str) -> bool {
        self.control(name).checked == Some(true)
    }

    /// A choice's items, a list's rows.
    pub fn items(&self, name: &str) -> Vec<String> {
        self.control(name).items
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

    /// The row of table `name` whose cell in `column` is `value`.
    pub fn row_with(&self, name: &str, column: usize, value: &str) -> Option<Vec<String>> {
        (0..self.rows(name))
            .map(|row| self.row(name, row))
            .find(|cells| cells.get(column).is_some_and(|cell| cell == value))
    }

    pub fn document(&self) -> &Document {
        self.host.document()
    }

    pub fn document_mut(&mut self) -> &mut Document {
        self.host.document_mut()
    }

    /// The lines the dialog handed to the host.
    pub fn lines(&self) -> Vec<String> {
        self.host.lines().to_vec()
    }

    /// Whether anything logged holds `part`.
    pub fn logged_containing(&self, part: &str) -> bool {
        self.host
            .logged()
            .iter()
            .any(|(message, _)| message.contains(part))
    }

    /// Whether exactly `message` was logged.
    pub fn logged_exactly(&self, message: &str) -> bool {
        self.host.logged().iter().any(|(said, _)| said == message)
    }

    /// The drawing the dialog was made with goes, and another takes its place in the host: what
    /// the C++ tests did by destroying the Document first.
    pub fn close_drawing(&mut self) {
        self.host.parts.document = Document::new(None);
        self.draw();
    }
}
