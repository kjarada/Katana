//! The frame every GIS analysis and check dialog is built on (geo/gis_tool_dialog.hpp; GIS >
//! Analysis - GDAL, GIS > Check - GDAL; docs/geoprocessing.md "V1" to "V5"): Buffer, Dissolve,
//! Overlay, Boundary Around Features, Clip to Boundary, Query with SQL, Check Geometry, Repair
//! Geometry, Gaps and Overlaps - and Export Drawing.
//!
//! Such a dialog runs nothing of GDAL's itself. Its fields describe a GIS line - shown, exactly as
//! it will run, in <d>Command - and Run hands that line to the window's one executor
//! (`Host::run`), as if it had been typed: echoed in the command log, kept in the history, and one
//! undo step. So there is nothing the dialog does that an agent cannot do by typing the same line
//! into katana_cli or katana_mcp. Preview runs the same line with PREVIEW, which changes nothing
//! and says what the scope took.
//!
//! The window runs a GIS line as a background job (geo_workbench): typed interactively, the
//! executor answers `job id=<n> ... state=started` at once, and the dialog shows the job's reply
//! when it hears the job ended (`Host::await_job`, or `job_finished` from whoever listens to the
//! workbench). Headless (--dialog ... --press <d>Run) the job is waited for, and the reply is
//! there when Run returns.
//!
//! What the drawing's data is taken from is the shared "Apply to" and "Only those that match"
//! controls (ScopeFilterWidget, CLAUDE.md section 1.1), named <d>Scope, <d>ScopeSelection,
//! <d>ScopeView, <d>ScopeLayers, <d>ScopeDrawing, <d>View, <d>OnScreen, <d>Layers, <d>Sublayers,
//! <d>Type<Point|Line|...>, <d>FilterLayer, <d>FilterStyle, <d>FilterColour, <d>FilterProperty,
//! <d>FilterValue, <d>FilterText, <d>DrawnOnly.
//!
//! Every dialog has, besides its own fields (each dialog's module lists them):
//!
//! ```text
//! <d>Dialog    the dialog
//! <d>Command   the line Run will run (read-only)
//! <d>Preview   run it with PREVIEW: what the scope takes, nothing changed
//! <d>Run       run it
//! <d>Status    what happened last
//! <d>Reply     the reply records, or why the line was refused (read-only)
//! <d>Close     close
//! ```
//!
//! and a check's dialog (Check Geometry, Gaps and Overlaps) also <d>Problems: the problem records
//! of the last reply as a table - kind, entity, where, length, reason.
//!
//! THE RUST SHAPE. The C++ frame is a base class; here it is a value each dialog holds
//! (`GisToolFrame`) and a trait each dialog answers (`GisTool`): its line, its own fields, and
//! what it reads of a reply. What the C++ context carried besides - the executor, the drawing,
//! whether the session is headless, the jobs' listener - is the `Host` the dialog is drawn with;
//! the context keeps only what a test's bare dialog lacked (an executor, a drawing) and the
//! workspace's views.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{RichText, Ui};
use katana_app::geo::replies::{line_word, parse_records};
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};

use super::geo_workbench::{GeoMenus, started_job};
use super::terrain_dialog_parts::RunStatus;

use crate::customisation::scope_filter_widget::ViewsFn;
use crate::customisation::{
    DocumentWatcher, ScopeFilterView, ScopeFilterWidget, scope_filter_views,
};
use crate::drive::{Action, LineOptions, button, label_as, line_with, primary};
use crate::host::{Host, JobId, NO_JOB, VerbOutcome};
use crate::shell::{DialogFrame, dialog};

/// The workspace's open views, asked afresh each time a scope's View choice is filled; shared, as
/// a dialog with two scopes hands it to both.
pub type GisViews = Rc<dyn Fn() -> Vec<ScopeFilterView>>;

/// What a GIS dialog asks of whoever shows it besides the `Host` it is drawn with; the window's
/// is `GisDialogContext::of_window`, a test's its own.
#[derive(Clone, Default)]
pub struct GisDialogContext {
    /// Whether there is an executor to hand a line to (the C++'s `run`, unset in a bare
    /// context): without one Run says "nothing here can run a command".
    pub runs: bool,
    /// Whether the scope offers the host's drawing's layers and follows it as it changes (the
    /// C++'s `document`, which may be null in a test's).
    pub reads_drawing: bool,
    /// The workspace's open views, for the scope's View choice. May be unset.
    pub views: Option<GisViews>,
}

impl GisDialogContext {
    /// The window's: its executor, its drawing and its workspace's views, asked afresh each time
    /// the View choice is filled, as every workbench of the window does.
    pub fn of_window(host: &mut dyn Host) -> Self {
        let views = host.views().map(|views| {
            let set = views.view_set();
            let asked: GisViews = Rc::new(move || scope_filter_views(&set.borrow()));
            asked
        });
        Self {
            runs: true,
            reads_drawing: true,
            views,
        }
    }

    /// The views as one scope widget takes them.
    pub fn views_fn(&self) -> Option<ViewsFn> {
        self.views.as_ref().map(|views| {
            let views = Rc::clone(views);
            let asked: ViewsFn = Box::new(move || views());
            asked
        })
    }
}

/// The scope words a form carries: ScopeFilterWidget::verb_words, or why the controls say none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GisScopeWords {
    pub words: String,
    pub error: String,
}

impl GisScopeWords {
    /// What `scope`'s controls say now.
    pub fn of(scope: &ScopeFilterWidget) -> Self {
        match scope.verb_words() {
            Ok(words) => Self {
                words,
                error: String::new(),
            },
            Err(error) => Self {
                words: String::new(),
                error: error.message,
            },
        }
    }
}

fn invalid(message: String) -> Error {
    make_error(ErrorCode::InvalidArgument, message, String::new())
}

/// The value of `key` in the reply's first record of `kind`, or empty: read by the one record
/// reader (geo::replies::parse_records), which undoes a quoted value's escapes.
fn field(reply: &str, kind: &str, key: &str) -> String {
    for record in parse_records(reply) {
        if record.kind == kind {
            return String::from(record.get(key).unwrap_or(""));
        }
    }
    String::new()
}

/// A word as the command line reads it: double-quoted when it holds a blank. InvalidArgument
/// naming `field` for a double quote, which no line can carry.
pub fn gis_word(text: &str, field: &str) -> Result<String> {
    // The one quoting rule (geo::replies::line_word): a keyword as a name is quoted too.
    line_word(text).ok_or_else(|| {
        invalid(format!(
            "{field} holds a double quote or a line break, which a command line cannot carry"
        ))
    })
}

/// The scope words, or InvalidArgument saying why there are none.
pub fn gis_scope(scope: &GisScopeWords) -> Result<String> {
    if !scope.error.is_empty() {
        return Err(invalid(format!("the scope: {}", scope.error)));
    }
    if scope.words.trim().is_empty() {
        return Err(invalid(String::from(
            "choose what in the drawing to act on first",
        )));
    }
    Ok(String::from(scope.words.trim()))
}

/// " key=<number>" for a number given, nothing for a blank; InvalidArgument naming `field` for
/// one that does not read.
pub fn gis_number_option(key: &str, text: &str, field: &str) -> Result<String> {
    let number = text.trim();
    if number.is_empty() {
        return Ok(String::new());
    }
    if parse_finite_double(number).is_none() {
        return Err(invalid(format!("{field} must be a number, not '{number}'")));
    }
    Ok(format!(" {key}={number}"))
}

/// " TO LAYER <layer>" for a layer given, nothing for a blank (the verb's default applies).
pub fn gis_layer_target(layer: &str) -> Result<String> {
    let name = layer.trim();
    if name.is_empty() {
        return Ok(String::new());
    }
    let word = gis_word(name, "The layer")?;
    Ok(format!(" TO LAYER {word}"))
}

/// What a reply says, in a sentence for <d>Status: the frame's own reading, which a dialog may
/// say more than (`GisTool::summary`).
pub fn gis_summary(reply: &str) -> String {
    if reply.contains(" preview=yes") {
        return String::from("Preview: nothing changed. What the scope takes is below.");
    }
    if reply.contains(" ran=no") {
        return String::from("The scope took nothing: nothing ran, and nothing changed.");
    }
    let created = field(reply, "output", "created");
    let updated = field(reply, "output", "updated");
    let deleted = field(reply, "output", "deleted");
    if !created.is_empty() {
        let changed = created != "0" || updated != "0" || deleted != "0";
        if !changed {
            return String::from(
                "Done: nothing to draw, and nothing was added to the undo history.",
            );
        }
        // An in-place reply (target=in-place: REPLACE, REPAIR, COVERAGE CLEAN) names no layer;
        // the status said "... deleted on .".
        let layer = field(reply, "output", "layer");
        let place = if layer.is_empty() {
            String::from(", in place")
        } else {
            format!(" on {layer}")
        };
        return format!(
            "Done as one undo step: {created} created, {updated} changed, {deleted} deleted{place}."
        );
    }
    String::from("Done. The reply is below and in the command log.")
}

/// The problems table's columns, and the record keys they show.
const PROBLEM_HEADERS: [&str; 5] = ["Kind", "Entity", "At", "Length", "Reason"];
const PROBLEM_KEYS: [&str; 5] = ["kind", "entity", "at", "length", "reason"];

/// The width a scope's controls are given beside a dialog's fields: two fifths of a 980 px
/// dialog (the C++ layout's stretch of 2 to 3).
const SCOPE_WIDTH: f32 = 380.0;

/// What every GIS dialog holds (see the module comment): its frame, its scope controls, the line
/// shown, the status, the reply and the job its last line started.
pub struct GisToolFrame {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: Option<DocumentWatcher>,
    name: String,
    frame: DialogFrame,
    visible: bool,
    context: GisDialogContext,
    scope: Option<ScopeFilterWidget>,
    width: f32,
    command: String,
    ready: bool,
    /// What happened last, or why there is no line yet.
    status: RunStatus,
    reply: String,
    /// The <d>Problems rows; None for a dialog that shows none.
    problems: Option<Vec<Vec<String>>>,
    problem: Option<usize>,
    pending: JobId,
    /// Where the end of a job this dialog's line started is told (`Host::await_job`'s done), for
    /// the next frame to take.
    heard: Rc<RefCell<Vec<(JobId, VerbOutcome)>>>,
}

impl GisToolFrame {
    /// `name` is the prefix of every object name ("gisBuffer"); `scoped` says whether the dialog
    /// reads the drawing through the scope controls.
    pub fn new(
        name: &str,
        title: &str,
        context: GisDialogContext,
        host: &dyn Host,
        scoped: bool,
    ) -> Self {
        let scope = scoped.then(|| {
            let mut scope = ScopeFilterWidget::new(name);
            scope.views = context.views_fn();
            scope
        });
        // The layers follow the drawing: a result adds its layer, an undo takes it away.
        let watcher = context
            .reads_drawing
            .then(|| DocumentWatcher::new(host.document()));
        Self {
            watcher,
            name: String::from(name),
            // Non-modal: kept open beside the drawing its lines change.
            frame: DialogFrame::new(&format!("{name}Dialog"), title),
            visible: false,
            context,
            scope,
            width: if scoped { 950.0 } else { 690.0 },
            command: String::new(),
            ready: false,
            status: RunStatus::default(),
            reply: String::new(),
            problems: None,
            problem: None,
            pending: NO_JOB,
            heard: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// The prefix of every object name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The dialog's own object name: the prefix and "Dialog".
    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    pub fn title(&self) -> &str {
        self.frame.title()
    }

    pub fn context(&self) -> &GisDialogContext {
        &self.context
    }

    /// The body's width, for a dialog with more to show than the frame's own (QDialog::resize).
    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }

    /// The scope controls; None for a dialog that takes no scope.
    pub fn scope_controls(&self) -> Option<&ScopeFilterWidget> {
        self.scope.as_ref()
    }

    pub fn scope_controls_mut(&mut self) -> Option<&mut ScopeFilterWidget> {
        self.scope.as_mut()
    }

    /// The scope words as the controls say them; nothing for a dialog with no scope.
    pub fn scope_words(&self) -> GisScopeWords {
        self.scope
            .as_ref()
            .map(GisScopeWords::of)
            .unwrap_or_default()
    }

    /// Whether the dialog reads a drawing that is still there.
    pub fn document_alive(&self) -> bool {
        self.context.reads_drawing
            && self
                .watcher
                .as_ref()
                .is_some_and(DocumentWatcher::document_alive)
    }

    /// Refills `scope`'s layers and views from the drawing, or its views alone when the dialog
    /// reads none: what the frame does for its own scope, and a dialog for its second.
    pub fn reload_scope(&self, scope: &mut ScopeFilterWidget, host: &dyn Host) {
        if self.document_alive() {
            scope.reload(host.document().model());
        } else {
            scope.reload_views();
        }
    }

    /// Refills the scope's layers and views, keeping what is chosen.
    pub fn reload(&mut self, host: &dyn Host) {
        if let Some(mut scope) = self.scope.take() {
            self.reload_scope(&mut scope, host);
            self.scope = Some(scope);
        }
    }

    /// Adds <d>Problems below the fields: a check's dialog.
    pub fn add_problems_table(&mut self) {
        self.problems = Some(Vec::new());
    }

    /// The <d>Problems rows; None for a dialog that shows none.
    pub fn problems(&self) -> Option<&[Vec<String>]> {
        self.problems.as_deref()
    }

    pub fn reply_text(&self) -> &str {
        &self.reply
    }

    /// The last thing <d>Status said; "" when it says only why there is no line.
    pub fn status_text(&self) -> &str {
        self.status.said()
    }

    pub fn status_is_error(&self) -> bool {
        self.status.is_error()
    }

    /// The line <d>Command shows; empty when the fields make none.
    pub fn command_text(&self) -> &str {
        &self.command
    }

    /// What <d>Status says; an error is shown as one.
    pub fn set_status(&mut self, text: &str, is_error: bool) {
        self.status.set(text, is_error);
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// QDialog::isModal: never.
    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    /// Shows the line the fields make now, or - in the status - why there is none (RunStatus).
    fn refresh_command(&mut self, made: &Result<String>) {
        let why = match made {
            Ok(line) => {
                self.command.clone_from(line);
                self.ready = true;
                String::new()
            }
            Err(error) => {
                self.command.clear();
                self.ready = false;
                format!("Nothing to run yet: {}", error.message)
            }
        };
        self.status.read_line(why);
    }
}

/// One GIS dialog: what it answers the frame (see the module comment), and what the frame does
/// for it.
pub trait GisTool {
    fn frame(&self) -> &GisToolFrame;
    fn frame_mut(&mut self) -> &mut GisToolFrame;

    /// The line Run would run, or why there is none.
    fn command(&self) -> Result<String>;

    /// Draws the dialog's own fields, beside the scope.
    fn fields_ui(&mut self, ui: &mut Ui, host: &mut dyn Host);

    /// What a reply says, in a sentence for <d>Status; a dialog may say more.
    fn summary(&self, reply: &str) -> String {
        gis_summary(reply)
    }

    /// Told each reply shown - for a dialog that reads more of it than the frame does (Query
    /// with SQL's grid). An empty reply for a failure.
    fn replied(&mut self, reply: &str) {
        let _ = reply;
    }

    /// Refills the scope's layers and views, keeping what is chosen.
    fn reload(&mut self, host: &dyn Host) {
        self.frame_mut().reload(host);
    }

    /// What Run does: the line handed to the executor; its reply - at once headless, when its
    /// job ends interactively - into <d>Reply. A line that cannot be written runs nothing, and
    /// <d>Status says why.
    fn run(&mut self, host: &mut dyn Host) {
        match self.command() {
            Ok(line) => run_line(self, host, &line),
            Err(error) => self.frame_mut().set_status(&error.message, true),
        }
    }

    /// Run with PREVIEW: nothing changes.
    fn preview(&mut self, host: &mut dyn Host) {
        match self.command() {
            Ok(line) => run_line(self, host, &format!("{line} PREVIEW")),
            Err(error) => self.frame_mut().set_status(&error.message, true),
        }
    }

    /// The word that a job ended; the dialog shows it when it is the job its last line started.
    fn job_finished(&mut self, id: JobId, outcome: &VerbOutcome) {
        if id == NO_JOB || id != self.frame().pending {
            return;
        }
        self.frame_mut().pending = NO_JOB;
        show_outcome(self, outcome);
    }

    /// Shows the dialog, with the layers and views there are now, not when it was made (its
    /// showEvent).
    fn show(&mut self, host: &dyn Host) {
        if !self.frame().visible {
            self.frame_mut().visible = true;
            self.reload(host);
        }
    }

    fn hide(&mut self) {
        self.frame_mut().visible = false;
    }

    fn is_visible(&self) -> bool {
        self.frame().visible
    }

    fn reply_text(&self) -> String {
        self.frame().reply.clone()
    }

    fn status_text(&self) -> String {
        String::from(self.frame().status.said())
    }

    /// One frame of the dialog, when it is shown.
    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        tool_ui(self, ui, host);
    }
}

fn run_line<T: GisTool + ?Sized>(tool: &mut T, host: &mut dyn Host, line: &str) {
    if !tool.frame().context.runs {
        tool.frame_mut()
            .set_status("nothing here can run a command", true);
        return;
    }
    tool.frame_mut().pending = NO_JOB;
    let outcome = host.run(line);
    // Interactively the line became a job: its reply comes when it ends.
    let started = if outcome.ok {
        started_job(&outcome.reply)
    } else {
        None
    };
    if let Some(started) = started {
        let heard = Rc::clone(&tool.frame().heard);
        // A host that hears no job's end leaves the dialog to whoever tells it (`job_finished`).
        let _ = host.await_job(
            &outcome,
            Box::new(move |done: &VerbOutcome, _host: &mut dyn Host| {
                heard.borrow_mut().push((started, done.clone()));
            }),
        );
        let frame = tool.frame_mut();
        frame.pending = started;
        frame.reply = String::from(outcome.reply.trim());
        frame.set_status(
            "Running as a background job: the reply appears here when it ends. Cancel is in \
             the status bar.",
            false,
        );
        return;
    }
    show_outcome(tool, &outcome);
}

fn show_outcome<T: GisTool + ?Sized>(tool: &mut T, outcome: &VerbOutcome) {
    let mut text = if outcome.ok {
        outcome.reply.clone()
    } else {
        outcome.error.clone()
    };
    if !outcome.ok && !outcome.reply.is_empty() {
        text = format!("{}\n{}", outcome.reply, outcome.error);
    }
    while text.ends_with('\n') {
        text.pop();
    }
    let frame = tool.frame_mut();
    frame.reply = text;
    if let Some(problems) = frame.problems.as_mut() {
        problems.clear();
        frame.problem = None;
        for record in parse_records(&outcome.reply) {
            if record.kind != "problem" {
                continue;
            }
            problems.push(
                PROBLEM_KEYS
                    .iter()
                    .map(|key| String::from(record.get(key).unwrap_or("")))
                    .collect(),
            );
        }
    }
    if !outcome.ok {
        let first = String::from(outcome.error.split('\n').next().unwrap_or(""));
        tool.frame_mut().set_status(&first, true);
        tool.replied("");
        return;
    }
    let said = tool.summary(&outcome.reply);
    tool.frame_mut().set_status(&said, false);
    tool.replied(&outcome.reply);
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h, T: ?Sized> {
    tool: &'a mut T,
    host: &'a mut (dyn Host + 'h),
    /// Whether the footer's Preview and Run were pressed.
    pressed: (bool, bool),
}

fn tool_ui<T: GisTool + ?Sized>(tool: &mut T, ui: &mut Ui, host: &mut dyn Host) {
    if !tool.frame().visible {
        return;
    }
    let changed = match &mut tool.frame_mut().watcher {
        Some(watcher) => watcher.deliver(host.document()),
        None => None,
    };
    if changed.is_some_and(|changes| changes.model) {
        tool.reload(&*host);
    }
    let heard: Vec<(JobId, VerbOutcome)> = std::mem::take(&mut *tool.frame().heard.borrow_mut());
    for (id, outcome) in heard {
        tool.job_finished(id, &outcome);
    }
    let made = tool.command();
    tool.frame_mut().refresh_command(&made);

    let ctx = ui.ctx().clone();
    let mut frame = tool.frame().frame.clone();
    let (preview, run) = {
        let mut lent = Lent {
            tool: &mut *tool,
            host: &mut *host,
            pressed: (false, false),
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| body(&mut *lent.tool, ui, &mut *lent.host),
            |ui, lent| lent.pressed = footer(&mut *lent.tool, ui),
        );
        if shown.closed {
            lent.tool.frame_mut().visible = false;
        }
        lent.pressed
    };
    tool.frame_mut().frame = frame;
    if preview {
        tool.preview(host);
    }
    if run {
        tool.run(host);
    }
}

/// The scope beside the fields, then the line and the reply.
fn body<T: GisTool + ?Sized>(tool: &mut T, ui: &mut Ui, host: &mut dyn Host) {
    ui.set_max_width(tool.frame().width);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0 * dialog::GROUP_GAP;
        if let Some(scope) = tool.frame_mut().scope.as_mut() {
            ui.vertical(|ui| {
                ui.set_max_width(SCOPE_WIDTH);
                scope.ui(ui);
            });
        }
        // The fields' column is a form of its own (dialog::form_panel): its rows share a label
        // column measured in it, and a second scope in it (Clip's boundary, Overlay's other
        // layer) fills the column as its fields do. Taken for a column of a row, it kept the
        // scope's own 400 and ended 350 short of the fields under it (Clip and Overlay).
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.x = dialog::GROUP_GAP;
            ui.spacing_mut().item_spacing.y = dialog::FORM_SPACING.y;
            dialog::form_panel(ui, |ui| tool.fields_ui(ui, host));
            let frame = tool.frame_mut();
            if let Some(problems) = frame.problems.as_ref() {
                let name = format!("{}Problems", frame.name);
                ui.add_space(dialog::GROUP_GAP);
                let shown = dialog::table_or_empty(
                    ui,
                    &name,
                    &PROBLEM_HEADERS,
                    problems,
                    frame.problem,
                    "The problems a run finds are listed here, one a row.",
                );
                if shown.chosen.is_some() {
                    frame.problem = shown.chosen;
                }
            }
        });
    });

    ui.add_space(dialog::GROUP_GAP);
    let frame = tool.frame_mut();
    let prefix = frame.name.clone();
    dialog::labelled(ui, "Command", |ui| {
        line_with(
            ui,
            &format!("{prefix}Command"),
            &mut frame.command,
            LineOptions {
                read_only: true,
                width: Some(f32::INFINITY),
                ..LineOptions::default()
            },
        )
        .on_hover_text(
            "The line Run hands to the command line - type it there, or give it to katana_cli \
             or katana_mcp, and it does the same",
        );
    });
}

/// The reply, once there is one (records one to a line, fixed pitch and not wrapped), and under
/// it the design's one footer row (shell::dialog::button_row): Preview - it reads the drawing and
/// changes nothing - and the status at the left, Close and Run - the dialog's act, in the accent -
/// at the right. Answers whether Preview and Run were pressed.
fn footer<T: GisTool + ?Sized>(tool: &mut T, ui: &mut Ui) -> (bool, bool) {
    let frame = tool.frame_mut();
    let prefix = frame.name.clone();
    dialog::reply(
        ui,
        &format!("{prefix}Reply"),
        &mut frame.reply,
        dialog::REPLY_ROWS,
    );
    let (status, is_error) = frame.status.shown();
    let ready = frame.ready;
    // A refusal's words, never markup; red only for a refusal.
    let text = RichText::new(status.as_str()).color(dialog::status_colour(is_error));
    let (preview, (close, run)) = dialog::button_row(
        ui,
        |ui| {
            let preview = ui
                .add_enabled_ui(ready, |ui| {
                    button(ui, &format!("{prefix}Preview"), "Preview")
                        .on_hover_text(
                            "Run the line with PREVIEW: what the scope takes, and nothing changed",
                        )
                        .clicked()
                })
                .inner;
            label_as(
                ui,
                &format!("{prefix}Status"),
                &status,
                egui::Label::new(text).wrap(),
            );
            preview
        },
        |ui| {
            let close = button(ui, &format!("{prefix}Close"), "Close").clicked();
            let run = ui
                .add_enabled_ui(ready, |ui| {
                    primary(ui, &format!("{prefix}Run"), "Run").clicked()
                })
                .inner;
            (close, run)
        },
    );
    if close {
        frame.visible = false;
    }
    (preview, run)
}

/// A line edit named `name` with a placeholder and a tip, beside its label in the form's label
/// column: a dialog's field.
pub fn gis_field(
    ui: &mut Ui,
    name: &str,
    label: &str,
    value: &mut String,
    placeholder: &str,
    tip: &str,
) -> egui::Response {
    dialog::labelled(ui, label, |ui| {
        line_with(
            ui,
            name,
            value,
            LineOptions {
                placeholder,
                ..LineOptions::default()
            },
        )
        .on_hover_text(tip)
    })
}

/// A check box under a dialog's fields, lined up with them (in the form's second column).
pub fn gis_check(ui: &mut Ui, name: &str, text: &str, on: &mut bool) -> egui::Response {
    dialog::labelled(ui, "", |ui| crate::drive::check(ui, name, text, on))
}

/// A file's field and a Browse button beside it, in one labelled row: the field leaves the
/// button its room. `names` are the field's and the button's. Whether Browse was clicked.
pub fn gis_file_field(
    ui: &mut Ui,
    names: (&str, &str),
    label: &str,
    value: &mut String,
    placeholder: &str,
    tip: &str,
) -> bool {
    let (name, browse) = names;
    dialog::labelled(ui, label, |ui| {
        let width = dialog::beside_a_button(ui, "Browse...");
        line_with(
            ui,
            name,
            value,
            LineOptions {
                placeholder,
                width: Some(width),
                ..LineOptions::default()
            },
        )
        .on_hover_text(tip);
        button(ui, browse, "Browse...").clicked()
    })
}

// ---- the menu items ----------------------------------------------------------------------------

/// Builds a GIS dialog over the window.
pub type GisDialogMaker = fn(GisDialogContext, &dyn Host) -> Box<dyn GisTool>;

/// The GIS dialogs the menu's actions open - one of each per window, made the first time and
/// kept (the C++ kept them as the window's children, found by object name) - and told when the
/// workbench's jobs end.
#[derive(Default)]
pub struct GisToolDialogs {
    /// Each action's name and what makes its dialog.
    makers: Vec<(String, GisDialogMaker)>,
    /// The dialogs made, by their action's name.
    kept: Vec<(String, Box<dyn GisTool>)>,
}

impl GisToolDialogs {
    pub fn new() -> Self {
        Self::default()
    }

    /// The dialog action `name` opens, when it has been made.
    pub fn dialog(&self, name: &str) -> Option<&dyn GisTool> {
        self.kept
            .iter()
            .find(|(kept, _)| kept == name)
            .map(|(_, dialog)| dialog.as_ref())
    }

    pub fn dialog_mut(&mut self, name: &str) -> Option<&mut (dyn GisTool + 'static)> {
        self.kept
            .iter_mut()
            .find(|(kept, _)| kept == name)
            .map(|(_, dialog)| dialog.as_mut())
    }

    /// How many dialogs have been made.
    pub fn made(&self) -> usize {
        self.kept.len()
    }

    /// What the action `name` does when it is triggered: its dialog made the first time, reloaded
    /// and shown. False when no action of that name was added.
    pub fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        if !self.kept.iter().any(|(kept, _)| kept == name) {
            let Some(make) = self
                .makers
                .iter()
                .find(|(action, _)| action == name)
                .map(|(_, make)| *make)
            else {
                return false;
            };
            let context = GisDialogContext::of_window(&mut *host);
            self.kept.push((String::from(name), make(context, &*host)));
        }
        if let Some(dialog) = self.dialog_mut(name) {
            dialog.reload(&*host);
            dialog.show(&*host);
        }
        true
    }

    /// The workbench's word that a job ended, handed to every dialog: each shows it when it is
    /// the job its last line started.
    pub fn job_finished(&mut self, id: JobId, outcome: &VerbOutcome) {
        for (_, dialog) in &mut self.kept {
            dialog.job_finished(id, outcome);
        }
    }

    /// One frame of every dialog shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        for (_, dialog) in &mut self.kept {
            dialog.ui(ui, host);
        }
    }
}

/// Makes the GIS menu action `name` in `section`, which opens the dialog `make` builds - one per
/// window, made the first time and kept (`GisToolDialogs::trigger`). The action names the dialog
/// it opens (<name>Dialog), which is how --dialog finds it.
#[allow(
    clippy::too_many_arguments,
    reason = "the C++ addGisToolAction's parameters, one for one"
)]
pub fn add_gis_tool_action(
    menus: &mut GeoMenus,
    dialogs: &mut GisToolDialogs,
    section: &str,
    icon: &'static str,
    text: &str,
    tip: &str,
    name: &str,
    make: GisDialogMaker,
) {
    let action = Action::new(name, text)
        .icon(icon)
        .tip(tip)
        .opens(&format!("{name}Dialog"));
    menus.add_to_gis(section, action);
    dialogs.makers.push((String::from(name), make));
}
