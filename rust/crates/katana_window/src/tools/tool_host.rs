//! The host a plan view runs a catalogue tool in (katana_cad's interactive_tool): the port of
//! tools/tool_host.
//!
//! The tool is a state machine in cad that knows nothing of the window; the view knows nothing of
//! any one tool. This sits between them: it starts a tool from the catalogue with the document's
//! current attributes and the live selection, hands it what the user did - a point (already snapped
//! by the view), an entity picked, typed text, Enter, Esc, Undo - and when the tool says it is
//! done, executes its ONE command through the Document, so a whole chain of lines or a whole Trim
//! session is one undo step. It restarts the tool when the tool asks (Circle, Point), and reports
//! through hooks, never by opening anything, so a headless session and a test drive it the same
//! way.
//!
//! No egui here on purpose: everything below is testable by calling it, and the view's part is
//! only turning events into these calls and drawing `feedback`.
//!
//! The Rust shape. The C++ host keeps `Document&` for its life; here every call is lent the
//! document (`&mut Document` where the call may execute a command, `&Document` where it only asks
//! the tool), as a tool's every call is lent its context (interactive_tool.rs, "The Rust shape of
//! a tool"). What the C++ read from the view through stored functions when a tool asked - the two
//! apertures, the view's hidden layers - the view writes here before each call (`set_apertures`,
//! `set_view`): a call cannot outlast a zoom, so a value set before it is the value now. The
//! view's own pick (`setPick`) has no twin: it was `pickEntity` through the view's hidden layers,
//! which is what cad's `pick_under` does itself with the context's `view` when it is lent no pick.
//!
//! A hook is handed the host and the document, since a Rust closure cannot keep either: the
//! C++'s hooks captured both, and one may start another tool while the host is still dealing with
//! the step that raised it (the window chains one command into the next so).

use std::rc::Rc;

use katana_cad::drawing::grips::Grip;
use katana_cad::interactive_tool::{
    InteractiveTool, ToolContext, ToolInfo, ToolInput, ToolStart, ToolStep, ToolStepOutcome,
    route_snapped_point, route_typed_input_with_drafting, tool_catalog,
};
use katana_cad::snapping::SnapResult;
use katana_cad::tool_feedback::ToolFeedback;
use katana_cad::{Document, DocumentParts, LayerOverrides};
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, trimmed};
use katana_core::{ErrorCode, Status};
use katana_entity::EntityId;
use katana_geometry::Point2;

/// What the tool made of an input (the C++ `ToolHost::Outcome`).
pub type Outcome = ToolStepOutcome;

/// A hook told a sentence: the prompt, a message, a refusal, a tool's id.
pub type TextHook = Rc<dyn Fn(&mut ToolHost, &mut Document, &str)>;
/// The hook asked to carry out a transparent command; it answers whether it did.
pub type TransparentHook = Rc<dyn Fn(&mut ToolHost, &mut Document, &str) -> bool>;

/// A TextHook of a closure, its parameters' types given by this signature (a closure handed to
/// `Rc::new` has none to infer them from).
pub fn text_hook(hook: impl Fn(&mut ToolHost, &mut Document, &str) + 'static) -> TextHook {
    Rc::new(hook)
}

/// A TransparentHook of a closure, as `text_hook`.
pub fn transparent_hook(
    hook: impl Fn(&mut ToolHost, &mut Document, &str) -> bool + 'static,
) -> TransparentHook {
    Rc::new(hook)
}

/// One of the view's apertures as the tools' context carries it.
type StoredAperture = Option<Box<dyn Fn() -> f64>>;

/// The context a call is lent: the document now, the tolerance the tool was made with, and what
/// the view last said of its apertures and hidden layers.
fn lend<'a>(
    document: &'a Document,
    pick_tolerance: f64,
    pick_aperture: &'a StoredAperture,
    vertex_aperture: &'a StoredAperture,
    view: &'a Option<LayerOverrides>,
) -> ToolContext<'a> {
    ToolContext {
        document: Some(document),
        pick_tolerance,
        pick: None,
        pick_aperture: pick_aperture.as_deref(),
        vertex_aperture: vertex_aperture.as_deref(),
        view: view.as_ref(),
    }
}

/// See the module comment.
#[derive(Default)]
pub struct ToolHost {
    cursor: Option<Point2>,
    /// The last click the tool took and what it said (`feedback`); cleared by any other input, a
    /// new tool, and the cursor leaving it.
    held_at: Option<Point2>,
    held_message: String,
    /// What `apply` made of its step: taken (Continue, or Done with its command executed), and
    /// the message it reported.
    last_taken: bool,
    last_message: String,
    /// The prompt on_prompt last carried.
    reported_prompt: String,
    view: Option<LayerOverrides>,
    info: Option<&'static ToolInfo>,
    tool: Option<Box<dyn InteractiveTool>>,
    /// Bumped whenever the tool is made or dropped, so `apply` can tell that a hook replaced the
    /// tool it was dealing with.
    generation: u64,
    pick_tolerance: f64,
    /// The tolerance the running tool was made with: what its every call is lent.
    made_tolerance: f64,
    handles: Vec<Grip>,
    pick_aperture: StoredAperture,
    pick_aperture_value: Option<f64>,
    vertex_aperture: StoredAperture,

    // ---- hooks, all optional ---------------------------------------------------------------
    /// The prompt changed: after every input, a start, a restart, and "" when the tool ends.
    pub on_prompt: Option<TextHook>,
    /// What a tool reports that is not a refusal: "3 lines", "2 selected", a measurement.
    pub on_message: Option<TextHook>,
    /// An input the tool refused, a command the document refused, or input with no tool running -
    /// the sentence says which.
    pub on_rejected: Option<TextHook>,
    pub on_started: Option<TextHook>,
    /// The tool ended: finished without asking to restart, or cancelled.
    pub on_finished: Option<TextHook>,
    /// A transparent command typed while a tool runs (`is_transparent_command`): it is the VIEW's
    /// - ZOOM, PAN - and goes here instead of to the tool, which stays at the same step, as
    /// AutoCAD runs 'ZOOM inside LINE. Answers whether it was carried out. Unset, or answering
    /// false, the host says the command cannot run inside the tool, and the tool is not sent it
    /// either: "Z" is never a malformed point.
    pub on_transparent: Option<TransparentHook>,
}

impl ToolHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts catalogue tool `id`. A tool already running is stopped first as Esc stops it
    /// (`cancel`), so switching tools half-way keeps whatever Esc would keep. NotFound for an id
    /// the catalogue does not have, and then nothing changes: the running tool, if any, runs on.
    pub fn start(&mut self, document: &mut Document, id: &str) -> Status {
        let Some(info) = tool_catalog().find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                format!("there is no tool '{id}'"),
                "",
            ));
        };
        self.cancel(document);
        self.held_at = None;
        self.info = Some(info);
        self.make(document);
        if let Some(hook) = self.on_started.clone() {
            hook(self, document, &info.id);
        }
        self.report_prompt(document);
        Ok(())
    }

    pub fn active(&self) -> bool {
        self.tool.is_some()
    }

    /// The running tool's catalogue entry; None when none is running.
    pub fn info(&self) -> Option<&'static ToolInfo> {
        self.info
    }

    /// The running tool's id, or "" when none is running.
    pub fn active_id(&self) -> String {
        self.info.map(|info| info.id.clone()).unwrap_or_default()
    }

    /// What the running tool wants next; None when none is running.
    pub fn expects(&self, document: &Document) -> Option<ToolInput> {
        let cx = lend(
            document,
            self.made_tolerance,
            &self.pick_aperture,
            &self.vertex_aperture,
            &self.view,
        );
        self.tool.as_ref().map(|tool| tool.expects(&cx))
    }

    /// The running tool's prompt, "" when none is running.
    pub fn prompt(&self, document: &Document) -> String {
        let cx = lend(
            document,
            self.made_tolerance,
            &self.pick_aperture,
            &self.vertex_aperture,
            &self.view,
        );
        self.tool
            .as_ref()
            .map(|tool| tool.prompt(&cx))
            .unwrap_or_default()
    }

    /// The rubber band for the cursor at `cursor`; empty when none is running. A click the tool
    /// took is never answered in red: until the cursor leaves the place of that click (by the pick
    /// aperture, `set_cursor`), a refusal there is shown without it - the marks the tool flags as
    /// its reason gone, the others kept - and captioned with what the click did ("vertex 2 added
    /// ..."). The tool, restarted or at its next step, is right that a second click there would
    /// be refused - a vertex is there now - but red straight after every success read as a
    /// failure.
    pub fn feedback(&self, document: &Document, cursor: Point2) -> ToolFeedback {
        let Some(tool) = self.tool.as_ref() else {
            return ToolFeedback::default();
        };
        let cx = lend(
            document,
            self.made_tolerance,
            &self.pick_aperture,
            &self.vertex_aperture,
            &self.view,
        );
        let shown = tool.preview(&cx, cursor);
        if shown.refused
            && let Some(held_at) = self.held_at
            && cursor.distance_to(held_at) <= self.hold_reach()
        {
            // What the tool shows there less its refusal: the marks it flags as the refusal's
            // reason go (the vertex Insert has just made, too near for another), the rest stay
            // as drawn - Straighten's first "keep", which the click has just taken - and the
            // caption is what the click did.
            let mut held = shown;
            held.refused = false;
            held.marks.retain(|mark| !mark.refused);
            held.caption.clone_from(&self.held_message);
            return held;
        }
        shown
    }

    /// Whether the running tool takes `snap` (InteractiveTool::takes_snap); with none running,
    /// every snap is taken - a grip's drag snaps as it always did.
    pub fn takes_snap(&self, document: &Document, snap: &SnapResult) -> bool {
        let cx = lend(
            document,
            self.made_tolerance,
            &self.pick_aperture,
            &self.vertex_aperture,
            &self.view,
        );
        self.tool
            .as_ref()
            .is_none_or(|tool| tool.takes_snap(&cx, snap))
    }

    /// What relative input (@dx,dy) and the Perpendicular and Tangent snaps measure from; None
    /// before the tool's first point.
    pub fn last_point(&self, document: &Document) -> Option<Point2> {
        let cx = lend(
            document,
            self.made_tolerance,
            &self.pick_aperture,
            &self.vertex_aperture,
            &self.view,
        );
        self.tool.as_ref().and_then(|tool| tool.last_point(&cx))
    }

    /// Changes whenever a tool is made, remade for a restart, or dropped: what a caller's own
    /// state about "the tool running now" is checked against.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The view's pick aperture in model units, given to a tool when it starts or restarts
    /// (ToolContext::pick_tolerance). The view sets it from its zoom.
    pub fn set_pick_tolerance(&mut self, tolerance: f64) {
        self.pick_tolerance = tolerance;
    }

    /// The grips hot in the view when the next tool starts (ToolStart::handles). Taken by the
    /// next tool made and by no other: a restart after an edit gets none, since the edit may have
    /// renumbered the vertices they name.
    pub fn set_handles(&mut self, handles: Vec<Grip>) {
        self.handles = handles;
    }

    /// The view's two apertures now, in model units (ToolContext::pick_aperture and
    /// vertex_aperture): the view writes them before each call, so a zoom inside a tool is
    /// followed. None, as in a test with no view: the tolerance stands for them.
    pub fn set_apertures(&mut self, pick: Option<f64>, vertex: Option<f64>) {
        self.pick_aperture_value = pick;
        self.pick_aperture = pick.map(|value| Box::new(move || value) as Box<dyn Fn() -> f64>);
        self.vertex_aperture = vertex.map(|value| Box::new(move || value) as Box<dyn Fn() -> f64>);
    }

    /// The view's own hidden layers (ToolContext::view): a tool finding what to act on itself
    /// passes them as the pick does. They change with no notification, so the view writes them
    /// before each call; kept as they are when they have not changed.
    pub fn set_view(&mut self, view: Option<&LayerOverrides>) {
        if self.view.as_ref() != view {
            self.view = view.cloned();
        }
    }

    // ---- what the user did -------------------------------------------------------------------
    // Each answers what the tool made of it. With no tool running the answer is Rejected and
    // on_rejected says so: an input with nowhere to go is reported, not dropped.

    /// A click at `at`; `snap` is what the view's snapping found there, if anything. A snap to an
    /// entity's end, middle, centre or vertex reaches the tool as that point of that entity (cad's
    /// route_snapped_point), so the annotation it makes follows the entity.
    pub fn point(
        &mut self,
        document: &mut Document,
        at: Point2,
        snap: Option<&SnapResult>,
    ) -> Outcome {
        let Some(tool) = self.tool.as_deref_mut() else {
            return self.idle(document);
        };
        let step = {
            let cx = lend(
                document,
                self.made_tolerance,
                &self.pick_aperture,
                &self.vertex_aperture,
                &self.view,
            );
            route_snapped_point(tool, &cx, at, snap)
        };
        self.click(document, at, step)
    }

    pub fn entity(&mut self, document: &mut Document, id: EntityId, at: Point2) -> Outcome {
        let Some(tool) = self.tool.as_deref_mut() else {
            return self.idle(document);
        };
        let step = {
            let cx = lend(
                document,
                self.made_tolerance,
                &self.pick_aperture,
                &self.vertex_aperture,
                &self.view,
            );
            tool.entity(&cx, id, at)
        };
        self.click(document, at, step)
    }

    /// Typed text, through route_typed_input_with_drafting: "10,20", "@5,0" and "@5<90" are
    /// points (the relative forms from `last_point`), anything else is a value or an option
    /// keyword.
    pub fn typed(&mut self, document: &mut Document, text: &str) -> Outcome {
        self.held_at = None;
        if self.tool.is_none() {
            return self.idle(document);
        }
        let wants_value = self.expects(document) == Some(ToolInput::Value);
        if is_transparent_command(text, wants_value) {
            // The view's, not the tool's: the tool stays at its step and its prompt is shown
            // again, as AutoCAD resumes LINE after 'ZOOM.
            let command = String::from_utf8_lossy(trimmed(text.as_bytes())).into_owned();
            if let Some(hook) = self.on_transparent.clone()
                && hook(self, document, &command)
            {
                self.report_prompt(document);
                return Outcome::Continue;
            }
            let name = self.info.map(|info| info.name.clone()).unwrap_or_default();
            self.report_rejected(
                document,
                &format!("{command} cannot run inside {name}; press Esc to end the tool first."),
            );
            return Outcome::Rejected;
        }
        // The drafting settings are the host's copy for the call, written back after it
        // (interactive_tool.rs, "The drafting aids' settings").
        let mut drafting = *document.drafting();
        let cursor = self.cursor;
        let step = {
            let Some(tool) = self.tool.as_deref_mut() else {
                return Outcome::Rejected;
            };
            let cx = lend(
                document,
                self.made_tolerance,
                &self.pick_aperture,
                &self.vertex_aperture,
                &self.view,
            );
            route_typed_input_with_drafting(tool, &cx, text, &mut drafting, cursor)
        };
        *document.drafting_mut() = drafting;
        self.apply(document, step)
    }

    /// Where the cursor is, for direct distance entry: a number typed at a point prompt is that
    /// far from the last point towards it. Leaving the place of the last click the tool took ends
    /// `feedback`'s hold on it.
    pub fn set_cursor(&mut self, at: Point2) {
        self.cursor = Some(at);
        if let Some(held_at) = self.held_at
            && at.distance_to(held_at) > self.hold_reach()
        {
            self.held_at = None;
        }
    }

    /// Enter, Space or a right-click.
    pub fn enter(&mut self, document: &mut Document) -> Outcome {
        self.held_at = None;
        let Some(tool) = self.tool.as_deref_mut() else {
            return self.idle(document);
        };
        let step = {
            let cx = lend(
                document,
                self.made_tolerance,
                &self.pick_aperture,
                &self.vertex_aperture,
                &self.view,
            );
            tool.enter(&cx)
        };
        self.apply(document, step)
    }

    /// Steps back one input inside the tool (the U inside LINE); never the document's undo.
    pub fn undo(&mut self, document: &mut Document) -> Outcome {
        self.held_at = None;
        let Some(tool) = self.tool.as_deref_mut() else {
            return self.idle(document);
        };
        let step = {
            let cx = lend(
                document,
                self.made_tolerance,
                &self.pick_aperture,
                &self.vertex_aperture,
                &self.view,
            );
            tool.undo(&cx)
        };
        self.apply(document, step)
    }

    /// Whether the running tool takes the Delete key from the window's Erase
    /// (InteractiveTool::takes_delete): the vertex tools, whose polyline Erase would take from
    /// under them. False when none is running.
    pub fn takes_delete(&self, document: &Document) -> bool {
        let cx = lend(
            document,
            self.made_tolerance,
            &self.pick_aperture,
            &self.vertex_aperture,
            &self.view,
        );
        self.tool
            .as_ref()
            .is_some_and(|tool| tool.takes_delete(&cx))
    }

    /// Delete pressed while `takes_delete`: the tool's answer, applied as any input's is (Delete
    /// Vertex deletes its chosen vertices; the others say the key does nothing there).
    pub fn delete_key(&mut self, document: &mut Document) -> Outcome {
        self.held_at = None;
        let Some(tool) = self.tool.as_deref_mut() else {
            return self.idle(document);
        };
        let step = {
            let cx = lend(
                document,
                self.made_tolerance,
                &self.pick_aperture,
                &self.vertex_aperture,
                &self.view,
            );
            tool.delete_key(&cx)
        };
        self.apply(document, step)
    }

    /// Esc: ends the tool, keeping what the tool's own cancel() says it has already placed - a
    /// chain of lines, the parts a Trim has cut, the copies Copy has put down - as AutoCAD keeps
    /// the segments of a LINE, and executing it as one command. Everything else goes with the
    /// tool. No-op when idle.
    pub fn cancel(&mut self, document: &mut Document) {
        let Some(tool) = self.tool.as_deref_mut() else {
            return;
        };
        // The tool says what of its work stays (InteractiveTool::cancel). A Continue or a
        // Rejected keeps nothing; the tool ends either way.
        let mut step = {
            let cx = lend(
                document,
                self.made_tolerance,
                &self.pick_aperture,
                &self.vertex_aperture,
                &self.view,
            );
            tool.cancel(&cx)
        };
        if step.outcome == Outcome::Done && (step.command.is_some() || step.selection.is_some()) {
            step.restart = false;
            // Ends the tool, as any finished step does.
            self.apply(document, step);
            return;
        }
        self.end(document);
    }

    /// Ends the running tool WITHOUT committing anything (on_finished, as for Esc): for when the
    /// document under it is being replaced, and its picks and ids belong to a drawing that is
    /// going. Ended rather than restarted, because a restart reads the current layer and the
    /// selection at that moment, and a caller replacing the document may call this before the
    /// replacement (the main window's new document does). No-op when idle.
    pub fn abandon(&mut self, document: &mut Document) {
        if self.tool.is_some() {
            self.end(document);
        }
    }

    // ---- the workings ------------------------------------------------------------------------

    /// (Re)creates the tool from the catalogue entry with a start read NOW: the current layer and
    /// style (D9) and the selection as it is.
    fn make(&mut self, document: &Document) {
        let Some(info) = self.info else {
            return;
        };
        let start = ToolStart {
            attributes: document.current_attributes(),
            selection: document.selection().ids(),
            // Once: taken by this tool, so a restart after its edit has none.
            handles: std::mem::take(&mut self.handles),
        };
        self.made_tolerance = self.pick_tolerance;
        let cx = lend(
            document,
            self.made_tolerance,
            &self.pick_aperture,
            &self.vertex_aperture,
            &self.view,
        );
        // The catalogue refuses an entry with no factory (ToolCatalog::add), so one found there
        // has one; with none there is nothing to run.
        self.tool = info.make.as_ref().map(|make| make(&cx, &start));
        self.generation += 1;
    }

    fn apply(&mut self, document: &mut Document, mut step: ToolStep) -> Outcome {
        let outcome = step.outcome;
        self.last_taken = outcome == Outcome::Continue;
        self.last_message.clone_from(&step.message);
        // The tool this step came from. A hook raised below may stop it or start another; what
        // follows a hook applies only while it is still this one. Counted, not compared by
        // address: a tool started in a hook can be allocated where the one it replaced was.
        let from = self.generation;
        match outcome {
            Outcome::Continue => {
                self.report_message(document, &step.message);
                if self.generation == from {
                    self.report_prompt(document);
                }
            }
            Outcome::Rejected => {
                // A refusal leaves the prompt as it was, but one that drops what the tool held -
                // a pick made stale by another edit - asks afresh, and the command line's
                // placeholder must say so. Before the refusal, so the status bar ends on the
                // refusal, as it does for every other.
                if self.prompt(document) != self.reported_prompt {
                    self.report_prompt(document);
                }
                self.report_rejected(document, &step.message);
            }
            Outcome::Done => {
                let mut executed = true;
                if let Some(command) = step.command.take()
                    && let Err(error) = document.execute(command)
                {
                    executed = false;
                    self.report_rejected(document, &error.describe());
                }
                self.last_taken = executed;
                if executed {
                    self.report_message(document, &step.message);
                    if let Some(selection) = step.selection.take() {
                        // What a selecting tool found, left selected: pruned, so an id the
                        // step's command removed is not held.
                        {
                            let DocumentParts {
                                model,
                                selection: held,
                                ..
                            } = document.parts_mut();
                            held.set(selection);
                            let _ = held.prune(&model.entities);
                        }
                        document.notify_selection_changed();
                    }
                }
                if self.generation == from {
                    if step.restart {
                        self.make(document);
                        self.report_prompt(document);
                    } else {
                        self.end(document);
                    }
                }
            }
        }
        outcome
    }

    /// A click at `at` that the tool answered with `step`: applied, and when taken, held so that
    /// its place is not answered in red.
    fn click(&mut self, document: &mut Document, at: Point2, step: ToolStep) -> Outcome {
        self.held_at = None;
        let outcome = self.apply(document, step);
        if self.last_taken && self.tool.is_some() {
            self.held_at = Some(at);
            self.held_message.clone_from(&self.last_message);
        }
        outcome
    }

    /// How far the cursor must go from a held click to end the hold: the view's pick aperture
    /// now, else the tolerance a tool is made with.
    fn hold_reach(&self) -> f64 {
        self.pick_aperture_value.unwrap_or(self.pick_tolerance)
    }

    /// Drops the tool and raises on_finished and on_prompt.
    fn end(&mut self, document: &mut Document) {
        let id = self.active_id();
        // Cleared before the hooks run, so a hook that starts another tool finds this one gone
        // rather than cancelling it a second time.
        self.tool = None;
        self.info = None;
        self.held_at = None;
        self.generation += 1;
        if let Some(hook) = self.on_finished.clone() {
            hook(self, document, &id);
        }
        self.reported_prompt.clear();
        if let Some(hook) = self.on_prompt.clone() {
            hook(self, document, "");
        }
    }

    /// Raises on_prompt with the running tool's prompt, and keeps it: a refusal says the prompt
    /// again only when it changed (`apply`).
    fn report_prompt(&mut self, document: &mut Document) {
        self.reported_prompt = self.prompt(document);
        if !self.reported_prompt.is_empty()
            && let Some(hook) = self.on_prompt.clone()
        {
            let prompt = self.reported_prompt.clone();
            hook(self, document, &prompt);
        }
    }

    fn idle(&mut self, document: &mut Document) -> Outcome {
        self.report_rejected(document, "No tool is running.");
        Outcome::Rejected
    }

    /// A hook is raised only with something to say.
    fn report_message(&mut self, document: &mut Document, text: &str) {
        if !text.is_empty()
            && let Some(hook) = self.on_message.clone()
        {
            hook(self, document, text);
        }
    }

    fn report_rejected(&mut self, document: &mut Document, text: &str) {
        if !text.is_empty()
            && let Some(hook) = self.on_rejected.clone()
        {
            hook(self, document, text);
        }
    }
}

/// True for text typed while a tool runs that is a command for the view, not an answer for the
/// tool: ZOOM, Z and PAN in any case, with or without AutoCAD's leading apostrophe ('ZOOM), and
/// 'P (a bare P is Rotate's and Scale's [Points] option), with any arguments after the verb
/// ("ZOOM E"). When the tool `wants_value` (ToolInput::Value: a text's string, a layer name, a
/// count) a bare word is its answer - a text may well read "Z" - so there only the apostrophe
/// forms are the view's.
pub fn is_transparent_command(text: &str, wants_value: bool) -> bool {
    let mut word = trimmed(text.as_bytes());
    let apostrophe = word.first() == Some(&b'\'');
    if apostrophe {
        word = &word[1..];
    } else if wants_value {
        return false;
    }
    let end = word
        .iter()
        .position(|&byte| byte == b' ')
        .unwrap_or(word.len());
    let word = &word[..end];
    let is = |verb: &str| equals_ignoring_case(word, verb.as_bytes());
    // A bare P is Rotate's and Scale's [Points] option, so only 'P - the apostrophe AutoCAD marks
    // a transparent command with - is PAN. No tool offers Z.
    is("ZOOM") || is("Z") || is("PAN") || (apostrophe && is("P"))
}
