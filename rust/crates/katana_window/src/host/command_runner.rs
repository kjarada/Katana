//! The window's ONE executor, as a dialog sees it (CLAUDE.md section 1; docs/desktop.md, "One
//! executor: the command runner"), and the rest of the window a dialog, a panel or a workbench
//! reaches: the port of command_runner.hpp, widened from a function to the `Host` the plan names
//! (section 4.1 and 4.2).
//!
//! A dialog that changes the drawing does not do the work itself: it builds the verb line a person
//! would type and hands it to `Host::run`, which the main window answers with its runVerbLine. The
//! line is echoed "> line" in the command log, run by the same dispatcher as a typed line - the
//! workbenches' verbs, the window's own, the interpreter's - and so kept in the history and undone
//! exactly as a typed one; what it logged comes back to the dialog. Unlike a typed line it is NEVER
//! a running tool's answer: a tool waiting for a point or a text would take any typed line for one,
//! and a dialog's line is never either. What was being typed on the command line is left as it
//! was.
//!
//! A test hands a dialog a host of its own (`RecordingHost`), which is what lets a dialog be tested
//! without a window.
//!
//! The Rust shape (the plan's section 4.2). The C++ hands each dialog a `std::function` runner and
//! pointers to what it reads; here the window's executor IS the host, lent to a dialog as
//! `&mut dyn Host` while the dialog draws, so the dialog reads the drawing and runs its line one
//! after the other with no shared ownership. Rejected: `Rc<RefCell<Document>>` everywhere (a
//! run-time borrow that can panic at every read), and dialogs queueing lines for the next frame
//! (the outcome would arrive a frame late, where every dialog reads it back at once).

use katana_app::geo::Context;
use katana_cad::{CommandInterpreter, Document};
use katana_interop::ReferenceData;

use super::clipboard::Clipboard;
use super::jobs::{JobId, Jobs};
use super::user_settings::UserSettings;
use super::view_access::ViewAccess;

/// What a line run through the executor did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerbOutcome {
    /// False when running the line logged an error it was refused for or failed on. A line carried
    /// out with problems it only reports - PLOTSHEETS with an image missing - is still ok.
    pub ok: bool,
    /// What the line logged that was not an error, a line each, in order: the reply a dialog
    /// shows.
    pub reply: String,
    /// What it logged as errors, a line each; empty when it logged none.
    pub error: String,
}

impl VerbOutcome {
    pub fn new(ok: bool, reply: &str, error: &str) -> Self {
        Self {
            ok,
            reply: String::from(reply),
            error: String::from(error),
        }
    }
}

/// One geoprocessing phase, lent the window's context for the call (`Host::geo`).
pub type GeoPhase<'a> = Box<dyn FnOnce(&mut Context<'_>) + 'a>;

/// Told how a job a line started ended (MainWindow::awaitJob's `done`), with the window.
pub type JobAwait = Box<dyn FnOnce(&VerbOutcome, &mut dyn Host)>;

/// The window as a dialog, a panel and a workbench see it: the main window's executor implements
/// it, and `RecordingHost` stands in for it in a test.
pub trait Host {
    /// MainWindow::runVerbLine, the one executor: `line` echoed in the command log and run by the
    /// dispatcher a typed line goes to - but never offered to a running tool - with what it logged
    /// returned. An empty line runs nothing and is not ok.
    fn run(&mut self, line: &str) -> VerbOutcome;

    /// The drawing, read. A dialog reads it here and changes it only through `run`.
    fn document(&self) -> &Document;
    /// The drawing, for a workbench's job apply (gis_online's import, its one command).
    fn document_mut(&mut self) -> &mut Document;
    /// The interpreter the executor runs lines with (its help, its tokenizer's rules).
    fn interpreter(&self) -> &CommandInterpreter;
    /// The window's reference rasters and point clouds.
    fn reference(&self) -> &ReferenceData;
    fn reference_mut(&mut self) -> &mut ReferenceData;

    /// One phase of a geoprocessing line (the plan's section 4.2): `phase` is called once, at
    /// once, with a `katana_app::geo::Context` over the window's document, interpreter, reference
    /// data, scratch folder and front end, lent for the call. `with_geo` is the form that answers
    /// a value.
    fn geo(&mut self, phase: GeoPhase<'_>);

    /// True in a session nobody watches (--screenshot, --plot, a script batch): no modal box, no
    /// file dialog, every question a refusal or a sentence (MainWindow::setHeadless).
    fn headless(&self) -> bool;
    /// The command log (MainWindow::logMessage): an error is also counted as the line's refusal
    /// and flashed in the status bar.
    fn log(&mut self, text: &str, is_error: bool);
    /// A warning box, or in a headless session a line in the log (MainWindow::warnUser).
    fn warn_user(&mut self, title: &str, text: &str);

    /// The workspace's views; None where there is no workspace (a test, a headless batch).
    fn views(&mut self) -> Option<&mut dyn ViewAccess>;

    /// The window's job runner.
    fn jobs(&mut self) -> &mut dyn Jobs;
    /// JobRunner::waitFor: runs the window until job `id` has ended and its finish was delivered.
    /// For a headless session, which has nobody to wait for a result and must not screenshot
    /// before it lands. An interactive caller never waits: that would be the freeze jobs exist to
    /// remove.
    fn wait_for(&mut self, id: JobId);
    /// JobRunner::waitForAll.
    fn wait_for_all(&mut self);
    /// MainWindow::awaitJob: when `started` - what `run` returned - says a geoprocessing job was
    /// started and it is still running, `done` is called with the job's outcome when it ends, and
    /// true answered. False otherwise: `started` is the answer. How a dialog hears what its line
    /// did.
    fn await_job(&mut self, started: &VerbOutcome, done: JobAwait) -> bool;

    /// The clipboard: in memory in a test and a headless run, the desktop's otherwise.
    fn clipboard(&mut self) -> &mut dyn Clipboard;
    /// The person's settings (the plan's section 4.11): in memory in a test and a headless run,
    /// which reads and writes no per-user place.
    fn settings(&mut self) -> &mut UserSettings;
}

/// A GeoPhase from a closure, its context's lifetimes left open as GeoPhase's are.
pub fn geo_phase<'a, F>(phase: F) -> GeoPhase<'a>
where
    F: FnOnce(&mut Context<'_>) + 'a,
{
    Box::new(phase)
}

/// `Host::geo` answering what `phase` answers.
pub fn with_geo<R>(host: &mut dyn Host, phase: impl FnOnce(&mut Context<'_>) -> R) -> R {
    let mut answer = None;
    host.geo(geo_phase(|context| {
        answer = Some(phase(context));
    }));
    answer.expect("a Host calls the geo phase it is lent, once, at once")
}
