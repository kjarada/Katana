//! What the workspace's twins share: the workspace as the C++ tests build it - a main window whose
//! central widget is the workspace, with the chrome, and an interpreter as the one executor whose
//! view host is the workspace - drawn in the harness at the window's size.
//!
//! The C++ window is resized and shown and its events processed; here the workspace is drawn in a
//! rectangle of that size at the harness screen's top left, and `frame` is a run of frames. The
//! C++ fixture's interpreter, run straight (`w.interpreter.run`) or as the runner, is the
//! recording host's: its lines go through the workspace's line runner (WorkspaceLines::answer),
//! and the host keeps every line it was handed (the C++'s `ran`).

#![allow(
    dead_code,
    reason = "Each file of the target uses only some of the fixture's helpers."
)]

use std::cell::Cell;
use std::rc::Rc;

use egui::{Pos2, Rect, UiBuilder, vec2};
use katana_cad::Document;
use katana_cad::view_set::{ViewId, ViewState};
use katana_cad::viewport_layout::ViewKind;
use katana_commands::{EntityAttributes, create_layer, create_line};
use katana_entity::{EntityId, Layer};
use katana_geometry::Point2;
use katana_window::canvas::PointF;
use katana_window::dock_chrome::{ChromeIcons, DockTitleBar};
use katana_window::drive::Harness;
use katana_window::host::{Answer, Host, RecordingHost, VerbOutcome};
use katana_window::view_workspace::{Scenery, ViewWorkspace, dock_name};
use katana_window::viewport_widget::{KeyModifiers, MouseButton};

/// A layer of that name, everything else the defaults.
pub fn layer_named(name: &str) -> Layer {
    Layer {
        name: String::from(name),
        ..Layer::default()
    }
}

/// The window's arrangement: the workspace with its chrome, over a drawing whose executor runs
/// each line through the workspace on the host's interpreter - or refuses it, when asked.
pub struct Site {
    pub views: ViewWorkspace,
    pub host: RecordingHost,
    pub harness: Harness,
    pub width: f32,
    pub height: f32,
    pub refuse: Rc<Cell<bool>>,
}

impl Site {
    /// The workspace in an 800 x 600 window, chromed, shown.
    pub fn new() -> Self {
        Self::sized(800.0, 600.0)
    }

    pub fn sized(width: f32, height: f32) -> Self {
        Self::built(width, height, true)
    }

    /// A workspace with no chrome: its views keep a plain title (a workspace built on its own).
    pub fn plain(width: f32, height: f32) -> Self {
        Self::built(width, height, false)
    }

    fn built(width: f32, height: f32, chrome: bool) -> Self {
        let document = Document::new(None);
        let views = ViewWorkspace::new(&document);
        if chrome {
            views.set_chrome(ChromeIcons::none());
        }
        let refuse = Rc::new(Cell::new(false));
        let refused = Rc::clone(&refuse);
        let mut through = views.lines().answer();
        let host = RecordingHost::with_document(document).answering(Answer::Custom(Box::new(
            move |parts, line| {
                if refused.get() {
                    VerbOutcome::new(false, "", "refused by the test")
                } else {
                    through(parts, line)
                }
            },
        )));
        let mut site = Self {
            views,
            host,
            harness: Harness::new(),
            width,
            height,
            refuse,
        };
        site.frame();
        site
    }

    /// The C++'s window.resize: the workspace drawn at its new size.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        self.frame();
    }

    /// processEvents: frames until the window is idle.
    pub fn frame(&mut self) {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(self.width, self.height));
        let views = &self.views;
        let host = &mut self.host;
        self.harness
            .run(|ui| {
                ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    views.ui(ui, host, Scenery::default());
                });
            })
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// A press of `button` on view `id`'s bar, as a click does, and the frames after it.
    pub fn press(&mut self, id: ViewId, button: &str) {
        self.harness.target(&dock_name(id)).press(button);
        self.frame();
    }

    pub fn document(&self) -> &Document {
        self.host.document()
    }

    pub fn document_mut(&mut self) -> &mut Document {
        self.host.document_mut()
    }

    /// Every line the executor was handed, in order.
    pub fn ran(&self) -> Vec<String> {
        self.host.lines().to_vec()
    }

    /// A line run straight through the workspace's line runner, as the C++ fixtures' interpreter
    /// runs it: what it answered, or why it refused. Not one of `ran`.
    pub fn interpret(&mut self, line: &str) -> Result<String, String> {
        let lines = self.views.lines();
        let parts = &mut self.host.parts;
        let mut cx = katana_window::viewport_widget::ViewParts {
            document: &mut parts.document,
            reference: Some(&parts.reference),
            meshes: &[],
        };
        lines
            .run_line(&mut parts.interpreter, &mut cx, &[], line)
            .map_err(|error| error.describe())
    }

    /// The C++ fixtures' run: the line through the interpreter, which must take it, and the
    /// events processed; its reply.
    pub fn run(&mut self, line: &str) -> String {
        let reply = self
            .interpret(line)
            .unwrap_or_else(|problem| panic!("{line}: {problem}"));
        self.frame();
        reply
    }

    /// A line through the window's executor, as a dialog hands one over.
    pub fn execute(&mut self, line: &str) -> VerbOutcome {
        self.host.run(line)
    }

    pub fn active(&self) -> ViewId {
        self.views.active_id()
    }

    pub fn state(&self, id: ViewId) -> ViewState {
        self.views
            .state(id)
            .unwrap_or_else(|| panic!("view {id} is open"))
    }

    pub fn with_state(&self, id: ViewId, f: impl FnOnce(&mut ViewState)) {
        self.views
            .with_state(id, f)
            .unwrap_or_else(|| panic!("view {id} is open"));
    }

    pub fn open(&mut self, kind: ViewKind) -> ViewId {
        let id = self.views.open_view(self.host.document_mut(), kind, true);
        self.frame();
        id
    }

    pub fn bar(&self, id: ViewId) -> DockTitleBar {
        self.views
            .title_bar(id)
            .unwrap_or_else(|| panic!("view {id} has a bar"))
    }

    /// Whether `button` is on view `id`'s bar as last drawn.
    pub fn shown(&self, id: ViewId, button: &str) -> bool {
        self.harness
            .snapshot()
            .control(&dock_name(id), button)
            .is_some_and(|control| control.visible)
    }

    /// A button's tooltip as its bar holds it: the label over the sentence.
    pub fn tip(&self, id: ViewId, button: &str) -> String {
        let mut bar = self.bar(id);
        bar.tool_mut(button)
            .map(|tool| format!("{}\n{}", tool.label, tool.tip))
            .unwrap_or_default()
    }

    /// The width of view `id`'s dock, as last laid out.
    pub fn dock_width(&self, id: ViewId) -> f32 {
        self.views
            .dock_rect(id)
            .unwrap_or_else(|| panic!("view {id} is docked"))
            .width()
    }

    // ---- a plan view, as the C++ tests send its widget events --------------------------------

    /// A plan view pinned to `width` x `height` pixels, showing (x, y) at `scale` as though the
    /// person had put it there: framed, and kept.
    pub fn place(&mut self, id: ViewId, width: i32, height: i32, x: f64, y: f64, scale: f64) {
        self.views.set_fixed_size(id, Some((width, height)));
        self.frame();
        self.with_state(id, |view| {
            view.plan.center = Point2::new(x, y);
            view.plan.scale = scale;
            view.plan_framed = true;
        });
        self.views.with_plan_view(id, |plan, _| plan.hold_view());
        self.frame();
    }

    /// The widget's grab: plan view `id` painted, what it raised handled with the drawing lent.
    pub fn paint(&mut self, id: ViewId) {
        let painted = self
            .views
            .with_plan_view_lent(self.host.document_mut(), id, |plan, state, cx| {
                let _ = plan.paint(cx, state);
            });
        assert!(painted.is_some(), "view {id} is a plan view");
    }

    /// One wheel notch (or `notches`) at `at` in plan view `id`.
    pub fn wheel(&mut self, id: ViewId, at: PointF, notches: i32) {
        self.views
            .with_plan_view_lent(self.host.document_mut(), id, |plan, state, cx| {
                plan.wheel(cx, state, at, 120.0 * f64::from(notches));
            });
        self.frame();
    }

    /// A middle-button drag in plan view `id` from `from` by `by` pixels, in four moves.
    pub fn pan(&mut self, id: ViewId, from: PointF, by: PointF) {
        self.views
            .with_plan_view_lent(self.host.document_mut(), id, |plan, state, cx| {
                plan.mouse_press(cx, state, from, MouseButton::Middle, KeyModifiers::NONE);
                for step in 1..=4 {
                    let at = PointF::new(
                        from.x + by.x * f64::from(step) / 4.0,
                        from.y + by.y * f64::from(step) / 4.0,
                    );
                    plan.mouse_move(cx, state, at, KeyModifiers::NONE);
                }
                let end = PointF::new(from.x + by.x, from.y + by.y);
                plan.mouse_release(cx, state, end, MouseButton::Middle, KeyModifiers::NONE);
            });
        self.frame();
    }

    pub fn drawing_paints(&self, id: ViewId) -> usize {
        self.views
            .with_plan_view(id, |plan, _| plan.drawing_paint_count())
            .unwrap_or_else(|| panic!("view {id} is a plan view"))
    }

    // ---- the drawing -------------------------------------------------------------------------

    pub fn add_layers(&mut self, names: &[&str]) {
        for name in names {
            let made = self.document_mut().execute(create_layer(layer_named(name)));
            assert!(made.is_ok(), "{name}");
        }
    }

    /// A line on `layer` (the current one for None): its id.
    pub fn line(&mut self, from: Point2, to: Point2, layer: Option<&str>) -> EntityId {
        let mut attributes: EntityAttributes = self.document().current_attributes();
        if let Some(layer) = layer {
            attributes.layer = String::from(layer);
        }
        let made = self.document_mut().execute(create_line(from, to, attributes));
        assert!(made.is_ok());
        *self
            .document()
            .model()
            .entities
            .ids()
            .last()
            .expect("a line was made")
    }
}

impl Default for Site {
    fn default() -> Self {
        Self::new()
    }
}

/// Bit for bit: the follower is given the leader's numbers, not a recomputation of them.
pub fn expect_same_view(follower: &ViewState, leader: &ViewState) {
    assert_eq!(follower.plan.center.x, leader.plan.center.x);
    assert_eq!(follower.plan.center.y, leader.plan.center.y);
    assert_eq!(follower.plan.scale, leader.plan.scale);
}

pub fn assert_near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not within {tolerance} of {expected}"
    );
}
