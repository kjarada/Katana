//! What the sheet editor's twins share: an editor shown over a drawing, as the C++ tests' Shown
//! made one - its host holding the drawing, the harness that draws its frames - and the mouse, the
//! keys and the pixels the C++ tests sent and read.
//!
//! The C++ tests sent their events to the canvas widget itself (QApplication::sendEvent), which
//! calls its handlers directly; the twins call the editor's event methods the same way, and draw
//! frames only where a test reads or drives a named control.

#![allow(
    dead_code,
    reason = "Each file of the target uses only some of these helpers."
)]

use std::rc::Rc;

use katana_cad::Document;
use katana_cad::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind};
use katana_geometry::{Box2, Point2};
use katana_window::canvas::{Color, Image, PointF, RectF};
use katana_window::drive::{ControlRecord, Harness};
use katana_window::host::{Answer, RecordingHost, VerbOutcome};
use katana_window::plan_painter::plan_source_of;
use katana_window::sheet_editor::{SheetCanvas, SheetEditor, SheetKey, SourceProvider};
use katana_window::sheet_painter::SheetSource;
use katana_window::viewport_widget::{KeyModifiers, MouseButton};

pub const NONE: KeyModifiers = KeyModifiers::NONE;
pub const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
pub const CTRL: KeyModifiers = KeyModifiers::CTRL;
pub const ALT: KeyModifiers = KeyModifiers {
    alt: true,
    ..KeyModifiers::NONE
};

pub fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

/// EXPECT_NEAR.
pub fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
pub fn double_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return false;
    }
    a.to_bits().abs_diff(b.to_bits()) <= 4
}

/// The C++ tests' sourceOf: the drawing's plan and its revision.
pub fn source_of() -> SourceProvider {
    Rc::new(|document: &Document| SheetSource {
        plan: plan_source_of(document),
        revision: document.model_revision(),
        ..SheetSource::default()
    })
}

/// The same with the document itself, which a section's and a legend's painting read.
pub fn source_with_document() -> SourceProvider {
    Rc::new(|document: &Document| SheetSource {
        plan: plan_source_of(document),
        document: Some(document),
        revision: document.model_revision(),
        ..SheetSource::default()
    })
}

pub fn viewport(id: &str, kind: ViewportKind, rect: Box2) -> Viewport {
    Viewport {
        id: String::from(id),
        kind,
        rect,
        ..Viewport::default()
    }
}

pub fn sheet_of(id: &str, name: &str, viewports: Vec<Viewport>) -> Sheet {
    Sheet {
        id: String::from(id),
        name: String::from(name),
        viewports,
        ..Sheet::default()
    }
}

/// An editor shown over a drawing at the C++ tests' 1400 x 900, its page fitted and painted.
pub struct Shown {
    pub host: RecordingHost,
    pub editor: SheetEditor,
    pub harness: Harness,
}

impl Shown {
    pub fn new(document: Document) -> Self {
        Self::with_source(document, source_of())
    }

    /// The drawing with `set` as its sheets, as one step, then shown.
    pub fn with_set(mut document: Document, set: &SheetSet) -> Self {
        if !set.sheets.is_empty() {
            document
                .set_sheet_set(set, Document::SET_SHEETS)
                .expect("the sheets are set");
        }
        Self::new(document)
    }

    pub fn with_source(document: Document, source: SourceProvider) -> Self {
        let mut shown = Self::bare(document, source);
        shown.editor.canvas_mut().fit_page();
        shown.paint();
        shown
    }

    /// Shown, laid out, but its page not fitted nor painted by the test (the C++ Shown of the
    /// tests that only processed events).
    pub fn bare(document: Document, source: SourceProvider) -> Self {
        let mut shown = Self {
            host: RecordingHost::with_document(document).answering(Answer::Interpreter),
            editor: SheetEditor::new(source),
            harness: Harness::with_screen(1400.0, 900.0),
        };
        shown.harness.target("sheetEditor");
        shown.editor.show(&shown.host);
        shown.draw();
        shown
    }

    pub fn document(&self) -> &Document {
        &self.host.parts.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.host.parts.document
    }

    /// The editor's look at the drawing after the test changed it (the C++ listener's call).
    pub fn observe(&mut self) {
        self.editor.observe(&self.host);
    }

    pub fn canvas(&mut self) -> &SheetCanvas {
        self.observe();
        self.editor.canvas()
    }

    pub fn canvas_mut(&mut self) -> &mut SheetCanvas {
        self.observe();
        self.editor.canvas_mut()
    }

    pub fn steps(&self) -> usize {
        self.document().history().undo_count()
    }

    pub fn undo(&mut self) {
        self.document_mut().undo().expect("an undo");
        self.observe();
    }

    /// The widget point of paper point (x, y) on the current sheet.
    pub fn at(&mut self, x: f64, y: f64) -> PointF {
        self.canvas().paper_to_widget(Point2::new(x, y))
    }

    pub fn sheet_set(&self) -> SheetSet {
        SheetSet::clone(&self.document().sheet_set())
    }

    pub fn by_id(&self, sheet: usize, id: &str) -> Viewport {
        self.document().sheet_set().sheets[sheet]
            .viewports
            .iter()
            .find(|viewport| viewport.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("no viewport {id}"))
    }

    pub fn ids_of(&self, sheet: usize) -> Vec<String> {
        self.document().sheet_set().sheets[sheet]
            .viewports
            .iter()
            .map(|viewport| viewport.id.clone())
            .collect()
    }

    pub fn selected_ids(&mut self) -> Vec<String> {
        self.canvas().selected_ids()
    }

    pub fn selected(&mut self) -> String {
        String::from(self.canvas().selected())
    }

    // ---- the mouse and the keys ------------------------------------------------------------

    pub fn press(&mut self, at: PointF, button: MouseButton, modifiers: KeyModifiers) {
        self.editor.mouse_press(&mut self.host, at, button, modifiers);
    }

    pub fn move_to(&mut self, at: PointF, modifiers: KeyModifiers) {
        self.editor.mouse_move(&mut self.host, at, modifiers);
    }

    pub fn release(&mut self, at: PointF, button: MouseButton) {
        self.editor.mouse_release(&mut self.host, at, button);
    }

    pub fn double_click(&mut self, at: PointF) {
        self.editor
            .mouse_double_click(&mut self.host, at, MouseButton::Left);
    }

    pub fn click(&mut self, at: PointF, modifiers: KeyModifiers) {
        self.press(at, MouseButton::Left, modifiers);
        self.release(at, MouseButton::Left);
    }

    /// Presses on `from` with `button`, moves in three steps to `to` and, unless told not to, lets
    /// go.
    pub fn drag_with(
        &mut self,
        from: PointF,
        to: PointF,
        modifiers: KeyModifiers,
        button: MouseButton,
        release: bool,
    ) {
        self.press(from, button, modifiers);
        for step in 1..=3 {
            let t = f64::from(step) / 3.0;
            self.move_to(from + (to - from) * t, modifiers);
        }
        if release {
            self.release(to, button);
        }
    }

    pub fn drag(&mut self, from: PointF, to: PointF, modifiers: KeyModifiers) {
        self.drag_with(from, to, modifiers, MouseButton::Left, true);
    }

    pub fn key(&mut self, key: SheetKey, modifiers: KeyModifiers) {
        self.editor.key_press(&mut self.host, key, modifiers);
    }

    pub fn key_release(&mut self, key: SheetKey) {
        self.editor.key_release(key);
    }

    // ---- the actions -----------------------------------------------------------------------

    pub fn action(&mut self, name: &str) -> katana_window::drive::Action {
        self.observe();
        self.editor
            .action(&self.host, name)
            .unwrap_or_else(|| panic!("no action {name}"))
    }

    /// QAction::trigger: a disabled action does nothing, as Qt's does.
    pub fn trigger(&mut self, name: &str) {
        let _ = self.editor.trigger(&mut self.host, name);
    }

    // ---- painting and frames ---------------------------------------------------------------

    /// The canvas painted (the C++'s test::paint).
    pub fn paint(&mut self) {
        let _ = self.editor.paint_canvas(&self.host);
    }

    /// The canvas as it shows (QWidget::grab).
    pub fn grab(&mut self) -> Image {
        self.editor.paint_canvas(&self.host)
    }

    /// Frames of the editor until the window is idle; an act refused fails the test.
    pub fn draw(&mut self) {
        let editor = &mut self.editor;
        let host = &mut self.host;
        self.harness
            .run(|ui| editor.ui(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// A field filled, a choice made, a box set, a check box "on" or "off", in target `target`.
    pub fn fill_in(&mut self, target: &str, name: &str, text: &str) {
        self.harness.target(target);
        self.harness.fill(name, text);
        self.draw();
        self.harness.target("sheetEditor");
    }

    pub fn press_in(&mut self, target: &str, name: &str) {
        self.harness.target(target);
        self.harness.press(name);
        self.draw();
        self.harness.target("sheetEditor");
    }

    pub fn fill(&mut self, name: &str, text: &str) {
        self.fill_in("sheetEditor", name, text);
    }

    pub fn press_button(&mut self, name: &str) {
        self.press_in("sheetEditor", name);
    }

    /// The control `name` as the last frame drew it.
    pub fn control(&self, name: &str) -> Option<ControlRecord> {
        self.harness.control(name)
    }

    pub fn has_control(&self, name: &str) -> bool {
        self.control(name).is_some()
    }

    /// What the window's log was told: the messages, the C++'s onMessage.
    pub fn messages(&self) -> Vec<String> {
        self.host.logged().iter().map(|(text, _)| text.clone()).collect()
    }

    /// How many of them were errors.
    pub fn errors(&self) -> usize {
        self.host.logged().iter().filter(|(_, error)| *error).count()
    }

    pub fn said(&self, text: &str) -> bool {
        self.host.logged().iter().any(|(said, _)| said == text)
    }
}

/// A host that hands every line to the window's executor and answers `outcome`: the C++ tests'
/// setCommandRunner with a lambda that recorded the line.
pub fn runner_answering(shown: &mut Shown, outcome: VerbOutcome) {
    shown.host.answer = Answer::Fixed(outcome);
    shown.editor.set_command_runner(true);
}

/// Pixels of `image` inside widget rectangle `rect` whose every channel is below `darkest`: ink.
pub fn dark_in(image: &Image, rect: RectF, darkest: u8) -> usize {
    let left = rect.left().floor().max(0.0) as u32;
    let top = rect.top().floor().max(0.0) as u32;
    let right = (rect.right().ceil() as i64 - 1).min(i64::from(image.width()) - 1);
    let bottom = (rect.bottom().ceil() as i64 - 1).min(i64::from(image.height()) - 1);
    let mut count = 0;
    if right < 0 || bottom < 0 {
        return 0;
    }
    for y in top..=bottom as u32 {
        for x in left..=right as u32 {
            let c = image.pixel(x, y);
            if c.r < darkest && c.g < darkest && c.b < darkest {
                count += 1;
            }
        }
    }
    count
}

/// The widget rectangle of paper `paper` on `canvas`, shrunk by `inset` pixels all round so that
/// an outline drawn on its edge is left out.
pub fn inside(canvas: &SheetCanvas, paper: &Box2, inset: f64) -> RectF {
    katana_window::plotting::sheet_canvas_editing::widget_rect_inside(canvas, paper, inset)
}

pub fn white() -> Color {
    Color::WHITE
}
