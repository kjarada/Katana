//! What the GIS twins share: a dialog drawn in the harness with its host (`Bench`), the readers of
//! what a frame drew - by the C++ object names, as `findChild` read the widgets - and the C++
//! tests' `Window`: a host that runs each line a dialog hands it by the geoprocessing workbench on
//! its own drawing, and answers what the line logged, the way the window's executor does.

#![allow(dead_code, reason = "Each case uses only some of the bench's readers.")]

use egui::Ui;
use katana_app::geo::Context;
use katana_cad::{CommandInterpreter, Document};
use katana_interop::ReferenceData;
use katana_window::dataset_info_dialog::DatasetInfoDialog;
use katana_window::drive::{ControlRecord, Harness};
use katana_window::geo::buffer_dialog::GisBufferDialog;
use katana_window::geo::clip_dialog::GisClipDialog;
use katana_window::geo::coverage_dialog::GisCoverageDialog;
use katana_window::geo::dissolve_dialog::GisDissolveDialog;
use katana_window::geo::formats_dialog::{FormatsDialog, KeptFormatsDialog};
use katana_window::geo::geo_workbench::GeoWorkbench;
use katana_window::geo::geometry_check_dialog::{GisCheckDialog, GisRepairDialog};
use katana_window::geo::gis_tool_dialog::{GisDialogContext, GisTool};
use katana_window::geo::hull_dialog::GisHullDialog;
use katana_window::geo::overlay_dialog::GisOverlayDialog;
use katana_window::geo::sql_dialog::GisSqlDialog;
use katana_window::gis_export_dialog::VectorExportDialog;
use katana_window::gis_import_dialogs::{
    PointCloudImportDialog, RasterImportDialog, VectorImportDialog,
};
use katana_window::host::{
    Answer, Clipboard, GeoPhase, Host, HostParts, JobAwait, JobId, Jobs, RecordingHost,
    UserSettings, VerbOutcome, ViewAccess,
};

/// A dialog the harness draws with a host of kind `H`.
pub trait Drawn<H> {
    fn draw(&mut self, ui: &mut Ui, host: &mut H);
}

macro_rules! drawn_gis_tools {
    ($($dialog:ty),* $(,)?) => {
        $(
            impl<H: Host> Drawn<H> for $dialog {
                fn draw(&mut self, ui: &mut Ui, host: &mut H) {
                    GisTool::ui(self, ui, host);
                }
            }
        )*
    };
}

drawn_gis_tools!(
    GisBufferDialog,
    GisDissolveDialog,
    GisHullDialog,
    GisClipDialog,
    GisOverlayDialog,
    GisCheckDialog,
    GisRepairDialog,
    GisCoverageDialog,
    GisSqlDialog,
    VectorExportDialog,
);

impl<H: Host> Drawn<H> for FormatsDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut H) {
        self.ui(ui, host);
    }
}

impl<H: Host> Drawn<H> for KeptFormatsDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut H) {
        self.ui(ui, host);
    }
}

impl<H: Host> Drawn<H> for DatasetInfoDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut H) {
        self.ui(ui, host);
    }
}

impl<H: Host> Drawn<H> for VectorImportDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut H) {
        let _ = self.ui(ui, host);
    }
}

impl<H: Host> Drawn<H> for RasterImportDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut H) {
        let _ = self.ui(ui, host);
    }
}

impl<H: Host> Drawn<H> for PointCloudImportDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut H) {
        let _ = self.ui(ui, host);
    }
}

/// A dialog, its host and the harness that draws them, its acts addressed to the dialog's
/// target.
pub struct Bench<D, H> {
    pub harness: Harness,
    pub dialog: D,
    pub host: H,
}

impl<D: Drawn<H>, H> Bench<D, H> {
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

    /// Frames until the window is idle. An act refused - a press of a disabled button, which the
    /// C++'s click would have done nothing for - fails the test with the C++'s sentence.
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

    /// Whether the last frame drew a control `name` (the C++ tests' findChild is not null).
    pub fn has(&self, name: &str) -> bool {
        self.harness.control(name).is_some()
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

    /// What a choice or a number box holds (currentText, value).
    pub fn value(&self, name: &str) -> String {
        self.control(name).value
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    pub fn visible(&self, name: &str) -> bool {
        self.control(name).visible
    }
}

/// A context with an executor and no drawing: the C++ tests' FakeRunner's.
pub fn runner_context() -> GisDialogContext {
    GisDialogContext {
        runs: true,
        ..GisDialogContext::default()
    }
}

/// A context with an executor and the host's drawing: the C++ tests' Window's.
pub fn window_context() -> GisDialogContext {
    GisDialogContext {
        runs: true,
        reads_drawing: true,
        views: None,
    }
}

/// A recording host answering every line with `outcome`, as the C++ tests' FakeRunner did.
pub fn fixed(outcome: VerbOutcome) -> RecordingHost {
    RecordingHost::new().answering(Answer::Fixed(outcome))
}

/// An ok outcome replying `reply`.
pub fn ok(reply: &str) -> VerbOutcome {
    VerbOutcome::new(true, reply, "")
}

/// The window's side, as the main window hands it to the geoprocessing workbench, with the
/// runner that captures what a line logged the way the window's executor does: the C++ tests'
/// `Window`. Headless, so a line waits for its job and its reply is there when `run` returns.
pub struct WindowHost {
    pub inner: RecordingHost,
    workbench: GeoWorkbench,
}

impl WindowHost {
    /// `scratch` names the folder the workbench's derived files go to.
    pub fn new(scratch: &str) -> Self {
        let mut inner = RecordingHost::new();
        inner.parts.scratch = std::env::temp_dir().join(scratch);
        Self {
            inner,
            workbench: GeoWorkbench::new(),
        }
    }

    /// A setup line run straight on the drawing by the window's interpreter (the C++ fixture's
    /// `interpreter.run`).
    pub fn type_line(&mut self, line: &str) {
        let parts = &mut self.inner.parts;
        parts
            .interpreter
            .run(&mut parts.document, line)
            .unwrap_or_else(|error| panic!("{line}: {}", error.describe()));
    }
}

impl Host for WindowHost {
    fn run(&mut self, line: &str) -> VerbOutcome {
        let before = self.inner.parts.logged.len();
        if !self.workbench.run_line(&mut self.inner, line) {
            return VerbOutcome::new(false, "", "not a geoprocessing line");
        }
        let said = self.inner.parts.logged.split_off(before);
        let mut logged = Vec::new();
        let mut errors = Vec::new();
        for (text, is_error) in said {
            if is_error {
                errors.push(text);
            } else {
                logged.push(text);
            }
        }
        VerbOutcome::new(errors.is_empty(), &logged.join("\n"), &errors.join("\n"))
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
        self.inner.headless()
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

/// A host that runs each line now by the geoprocessing executor on its own drawing and answers
/// its reply or its refusal: what a headless window's executor gives back once a job has been
/// waited for (the C++ dataset tests' Executor).
pub fn executor_host(scratch: &str) -> RecordingHost {
    let mut host = RecordingHost::new().answering(Answer::Custom(Box::new(
        |parts: &mut HostParts, line: &str| {
        let mut context = Context::new(
            &mut parts.document,
            &parts.interpreter,
            &mut parts.reference,
            &parts.scratch,
            &parts.front_end,
        );
        match katana_app::geo::run_now(&mut context, line, None, None) {
            Ok(reply) => VerbOutcome::new(true, &reply, ""),
            Err(error) => VerbOutcome::new(false, "", &format!("error: {}", error.describe())),
        }
        },
    )));
    host.parts.scratch = std::env::temp_dir().join(scratch);
    host
}
