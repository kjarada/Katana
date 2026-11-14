//! What the terrain folder's twins share: a panel or a dialog drawn in the harness with a recording
//! host whose runner is the interpreter on the host's drawing (`Bench`), the surfaces the C++
//! tests put in the drawing, and a workspace's views that keep the box they were asked to frame.

#![allow(dead_code, reason = "Each case uses only some of the bench's helpers.")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use egui::Ui;
use katana_cad::view_set::{ViewId, ViewSet};
use katana_geometry::{Box2, Point2, Point3};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};
use katana_window::drive::{ControlRecord, Harness};
use katana_window::host::{Answer, PickId, RecordingHost, ViewAccess};

/// What the harness draws with the recording host.
pub trait Drawn {
    fn draw(&mut self, ui: &mut Ui, host: &mut RecordingHost);
}

/// A panel or a shown dialog, its host and the harness that draws them.
pub struct Bench<D> {
    pub harness: Harness,
    pub dialog: D,
    pub host: RecordingHost,
}

impl<D: Drawn> Bench<D> {
    /// `dialog` drawn once in the target `target`.
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

    /// Frames until the window is idle (the C++'s processEvents).
    pub fn draw(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        self.harness
            .run(|ui| dialog.draw(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.draw();
    }

    pub fn tick(&mut self, name: &str, on: bool) {
        self.fill(name, if on { "on" } else { "off" });
    }

    pub fn press(&mut self, name: &str) {
        self.harness.press(name);
        self.draw();
    }

    pub fn control(&self, name: &str) -> ControlRecord {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    pub fn has(&self, name: &str) -> bool {
        self.harness.control(name).is_some()
    }

    pub fn text(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    pub fn value(&self, name: &str) -> String {
        self.control(name).value
    }

    pub fn items(&self, name: &str) -> Vec<String> {
        self.control(name).items
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    /// The last line the dialog handed the executor.
    pub fn last_line(&self) -> String {
        self.host.lines().last().cloned().unwrap_or_default()
    }
}

/// A host that runs every line it is handed through its interpreter on its drawing and hands back
/// what came of it: the C++ tests' runner.
pub fn interpreter_host() -> RecordingHost {
    RecordingHost::new().answering(Answer::Interpreter)
}

/// A flat square of side 10 at height `z`: two triangles.
pub fn square(z: f64) -> Arc<TinSurface> {
    let made = TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, z),
            Point3::new(10.0, 0.0, z),
            Point3::new(10.0, 10.0, z),
            Point3::new(0.0, 10.0, z),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{}", error.describe()));
    Arc::new(made)
}

pub fn named(name: &str, surface: Arc<TinSurface>, source: &str) -> NamedSurface {
    NamedSurface {
        name: String::from(name),
        surface: Some(surface),
        source: String::from(source),
        ..NamedSurface::default()
    }
}

/// Adds `surface` to the store, failing the test when it is refused.
pub fn keep(store: &mut SurfaceStore, surface: NamedSurface) {
    store
        .add(surface)
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

/// The drawing's surfaces replaced by `store`, as one step named `step`.
pub fn set_surfaces(host: &mut RecordingHost, store: SurfaceStore, step: &str) {
    host.document_mut()
        .set_surfaces(store, step)
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

/// A workspace's views that keep the box they were asked to frame (the C++ tests' zoomTo).
pub struct ZoomViews {
    pub zoomed: Rc<RefCell<Option<Box2>>>,
    views: Rc<RefCell<ViewSet>>,
}

impl ZoomViews {
    /// The views, and where the box they are asked to frame is kept.
    pub fn new() -> (Self, Rc<RefCell<Option<Box2>>>) {
        let zoomed = Rc::new(RefCell::new(None));
        (
            Self {
                zoomed: Rc::clone(&zoomed),
                views: Rc::new(RefCell::new(ViewSet::new())),
            },
            zoomed,
        )
    }
}

impl ViewAccess for ZoomViews {
    fn view_set(&self) -> Rc<RefCell<ViewSet>> {
        Rc::clone(&self.views)
    }
    fn zoom_to(&mut self, area: &Box2) {
        *self.zoomed.borrow_mut() = Some(*area);
    }
    fn zoom_extents_all(&mut self) {}
    fn zoom_to_selection(&mut self, _view: ViewId) {}
    fn active_plan_view(&self) -> Option<ViewId> {
        None
    }
    fn plan_views(&self) -> Vec<ViewId> {
        Vec::new()
    }
    fn refresh_all(&mut self) {}
    fn pick_point(&mut self) -> PickId {
        0
    }
    fn picked_point(&mut self, _pick: PickId) -> Option<Option<Point2>> {
        Some(None)
    }
}
