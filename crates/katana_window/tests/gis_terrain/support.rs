//! What the geoprocessing and terrain dialogs' twins share: a dialog drawn in the harness with a
//! host (`Bench`), the hosts the C++ tests' runners stand for, a workspace's views that answer a
//! point pick at once, and the window's side as the main window hands it over - a host whose
//! executor is the geoprocessing workbench (`GeoHost`).

#![allow(dead_code, reason = "Each case uses only some of the bench's helpers.")]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use egui::Ui;
use katana_cad::view_set::{ViewId, ViewSet};
use katana_cad::{CommandInterpreter, Document};
use katana_geometry::{Box2, Point2, Point3};
use katana_interop::{RasterOverlay, ReferenceData, ReferenceId};
use katana_terrain::{NamedSurface, SurfaceStore, TinBuildOptions, TinInput, TinSurface, build_tin};
use katana_window::drive::{ControlRecord, Harness};
use katana_window::geo::geo_workbench::GeoWorkbench;
use katana_window::host::{
    Answer, Clipboard, GeoPhase, Host, JobAwait, JobId, Jobs, PickId, RecordingHost, UserSettings,
    VerbOutcome, ViewAccess,
};

/// A dialog the harness draws with a host of kind `H`.
pub trait Drawn<H> {
    fn draw(&mut self, ui: &mut Ui, host: &mut H);
}

/// A shown dialog, its host and the harness that draws them, its acts addressed to the dialog's
/// target.
pub struct Bench<D, H = RecordingHost> {
    pub harness: Harness,
    pub dialog: D,
    pub host: H,
}

impl<H, D: Drawn<H>> Bench<D, H> {
    /// `dialog`, already shown, drawn once in the target `target`.
    pub fn new(target: &str, dialog: D, host: H) -> Self {
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

    /// Frames until the window is idle (the C++'s processEvents). An act refused - a press of a
    /// disabled button - fails the test with the C++'s sentence.
    pub fn draw(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        self.harness
            .run(|ui| dialog.draw(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// A field typed into (setText), a choice made by its text (setCurrentText), a number box set.
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.draw();
    }

    /// A check box ticked or cleared (setChecked).
    pub fn tick(&mut self, name: &str, on: bool) {
        self.fill(name, if on { "on" } else { "off" });
    }

    /// A click.
    pub fn press(&mut self, name: &str) {
        self.harness.press(name);
        self.draw();
    }

    /// The control `name` as the last frame drew it (findChild).
    pub fn control(&self, name: &str) -> ControlRecord {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    /// Whether the last frame drew a control `name` at all.
    pub fn has(&self, name: &str) -> bool {
        self.harness.control(name).is_some()
    }

    /// What a label, a field or a text box shows (text(), toPlainText()).
    pub fn text(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    /// What a choice or a number box shows (currentText(), value()).
    pub fn value(&self, name: &str) -> String {
        self.control(name).value
    }

    /// A field's placeholder (placeholderText()).
    pub fn placeholder(&self, name: &str) -> String {
        self.control(name).text
    }

    /// The items of a choice or the rows of a list.
    pub fn items(&self, name: &str) -> Vec<String> {
        self.control(name).items
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    pub fn visible(&self, name: &str) -> bool {
        self.control(name).visible
    }
}

/// A host that keeps the lines and answers each as the C++ tests' Recorder does: ok, "ran <line>".
pub fn ran_host() -> RecordingHost {
    RecordingHost::new().answering(Answer::Custom(Box::new(|_parts, line| {
        VerbOutcome::new(true, &format!("ran {line}"), "")
    })))
}

/// A host that keeps the lines and answers each `reply`, ok: the C++ tests' StubRunner.
pub fn stub_host(reply: &str) -> RecordingHost {
    RecordingHost::new().answering(Answer::Fixed(VerbOutcome::new(true, reply, "")))
}

/// The reference raster the C++ tests add: named "terrain", from terrain.asc. Its id.
pub fn add_raster(host: &mut RecordingHost, name: &str, source: &str) -> ReferenceId {
    let raster = RasterOverlay {
        name: String::from(name),
        source: PathBuf::from(source),
        ..RasterOverlay::default()
    };
    host.parts.reference.add(raster)
}

/// The surface `name` kept in the host's drawing.
pub fn add_surface(host: &mut RecordingHost, name: &str, surface: TinSurface) {
    let mut store: SurfaceStore = host.document().surfaces().clone();
    store
        .add(NamedSurface {
            name: String::from(name),
            surface: Some(Arc::new(surface)),
            source: String::from("test"),
            ..NamedSurface::default()
        })
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    host.document_mut()
        .set_surfaces(store, "TEST_SURFACES")
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

/// The surface built over `points` (x, y, z), as the C++ tests' buildTin builds it.
pub fn tin_of(points: &[(f64, f64, f64)]) -> TinSurface {
    let input = TinInput {
        points: points
            .iter()
            .map(|&(x, y, z)| Point3::new(x, y, z))
            .collect(),
        ..TinInput::default()
    };
    build_tin(&input, &TinBuildOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
        .surface
}

/// A workspace's views that answer a point pick at once with `point`, as a click in a plan view
/// would: the C++ tests' pickPoint.
pub struct PickingViews {
    pub point: Point2,
    views: Rc<RefCell<ViewSet>>,
    asked: PickId,
}

impl PickingViews {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            point: Point2::new(x, y),
            views: Rc::new(RefCell::new(ViewSet::new())),
            asked: 0,
        }
    }
}

impl ViewAccess for PickingViews {
    fn view_set(&self) -> Rc<RefCell<ViewSet>> {
        Rc::clone(&self.views)
    }
    fn zoom_to(&mut self, _area: &Box2) {}
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
        self.asked += 1;
        self.asked
    }
    fn picked_point(&mut self, _pick: PickId) -> Option<Option<Point2>> {
        Some(Some(self.point))
    }
}

/// The window's side, as the main window hands it over (the C++ tests' Window): a host whose
/// executor is the geoprocessing workbench - what runVerbLine does for a geoprocessing line - with
/// a scratch folder of its own. Headless, so a line waits for its job and what it logged is the
/// reply.
pub struct GeoHost {
    pub inner: RecordingHost,
    pub workbench: GeoWorkbench,
    scratch: PathBuf,
}

impl GeoHost {
    /// `name` names the scratch folder, emptied before and after: a raster left there by an
    /// earlier run would take the name the test expects.
    pub fn new(name: &str) -> Self {
        let scratch =
            std::env::temp_dir().join(format!("katana-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let mut inner = RecordingHost::new();
        inner.parts.scratch.clone_from(&scratch);
        Self {
            inner,
            workbench: GeoWorkbench::new(),
            scratch,
        }
    }
}

impl Drop for GeoHost {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

impl Host for GeoHost {
    fn run(&mut self, line: &str) -> VerbOutcome {
        self.inner.parts.logged.clear();
        let taken = self.workbench.run_line(&mut self.inner, line);
        let reply = self
            .inner
            .parts
            .logged
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let ok = taken && !reply.starts_with("error:");
        VerbOutcome {
            ok,
            error: if ok { String::new() } else { reply.clone() },
            reply,
        }
    }
    fn document(&self) -> &Document {
        Host::document(&self.inner)
    }
    fn document_mut(&mut self) -> &mut Document {
        Host::document_mut(&mut self.inner)
    }
    fn interpreter(&self) -> &CommandInterpreter {
        self.inner.interpreter()
    }
    fn reference(&self) -> &ReferenceData {
        self.inner.reference()
    }
    fn reference_mut(&mut self) -> &mut ReferenceData {
        self.inner.reference_mut()
    }
    fn geo(&mut self, phase: GeoPhase<'_>) {
        self.inner.geo(phase);
    }
    fn headless(&self) -> bool {
        true
    }
    fn log(&mut self, text: &str, is_error: bool) {
        self.inner.log(text, is_error);
    }
    fn warn_user(&mut self, title: &str, text: &str) {
        self.inner.warn_user(title, text);
    }
    fn views(&mut self) -> Option<&mut dyn ViewAccess> {
        self.inner.views()
    }
    fn jobs(&mut self) -> &mut dyn Jobs {
        self.inner.jobs()
    }
    fn wait_for(&mut self, id: JobId) {
        self.inner.wait_for(id);
    }
    fn wait_for_all(&mut self) {
        self.inner.wait_for_all();
    }
    fn await_job(&mut self, started: &VerbOutcome, done: JobAwait) -> bool {
        self.inner.await_job(started, done)
    }
    fn clipboard(&mut self) -> &mut dyn Clipboard {
        self.inner.clipboard()
    }
    fn settings(&mut self) -> &mut UserSettings {
        self.inner.settings()
    }
}
