//! Drives an interactive tool the way the plan view does
//! (tests/cad/tools/tool_driver.hpp), so a tool is tested by the inputs a user
//! would give it: start it from the catalogue, click, pick, type, press
//! Enter; and a finished step's command is executed on the document, exactly
//! as the view does. Assert on the document afterwards against geometry
//! worked out by hand.
//!
//! The driver is the tool's host (interactive_tool.rs, "The Rust shape of a
//! tool"): it makes the tool with a ToolStart, and lends every call a
//! ToolContext of its document, its view's hidden layers and the pick
//! tolerance the tool was made with, dropped before it executes anything.
//! The C++ `driver.tool().prompt()` is `driver.prompt()` here (likewise
//! expects, last_point, last_height), since a tool's every method takes the
//! context; `with_tool` lends the tool and its context for anything else.
//! The C++ ToolDriver's EXPECTs on an execution are assertions here: a
//! command the document refuses fails the test where it happens.
//!
//! Shared by the cad test targets that drive tools, each including it with
//! `#[path = "../support/tool_driver.rs"] pub mod tool_driver;`.

#![allow(
    dead_code,
    reason = "Each test target that includes the driver uses a part of it."
)]

use katana_cad::interactive_tool::{
    InteractiveTool, ToolContext, ToolInfo, ToolInput, ToolStart, ToolStep, ToolStepOutcome,
    route_snapped_point, route_typed_input, tool_catalog,
};
use katana_cad::snapping::{SnapAccept, snap};
use katana_cad::{Document, Grip, LayerOverrides, SnapRequest, SnapResult, ToolFeedback};
use katana_commands::CommandPtr;
use katana_entity::EntityId;
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_geometry::Point2;

// The context a call is lent: the driver's document and view, and the
// tolerance its tool was made with.
fn context_of<'a>(
    document: &'a Document,
    view: &'a LayerOverrides,
    pick_tolerance: f64,
) -> ToolContext<'a> {
    ToolContext {
        document: Some(document),
        pick_tolerance,
        view: Some(view),
        ..ToolContext::default()
    }
}

pub struct ToolDriver {
    document: Document,
    view: LayerOverrides,
    info: Option<&'static ToolInfo>,
    tool: Option<Box<dyn InteractiveTool>>,
    handles: Vec<Grip>,
    pick_tolerance: f64,
    messages: Vec<String>,
    executed: i32,
    finished: bool,
}

impl Default for ToolDriver {
    fn default() -> Self {
        Self {
            document: Document::default(),
            view: LayerOverrides::default(),
            info: None,
            tool: None,
            handles: Vec::new(),
            pick_tolerance: 0.01,
            messages: Vec::new(),
            executed: 0,
            finished: false,
        }
    }
}

impl ToolDriver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts catalogue tool `id` on the current selection (select entities
    /// in document_mut() first for a Modify tool). Fails the test if there is
    /// no such tool.
    #[track_caller]
    pub fn start(&mut self, id: &str) {
        self.start_with(id, Vec::new());
    }

    /// The same with the grips hot when the tool starts (ToolStart::handles):
    /// given to the first make only, as the tool host gives them, so a restart
    /// after an edit sees none. (The C++ `start(id, handles)` overload.)
    #[track_caller]
    pub fn start_with(&mut self, id: &str, handles: Vec<Grip>) {
        let Some(info) = tool_catalog().find(id) else {
            panic!("no tool {id}");
        };
        self.info = Some(info);
        self.handles = handles;
        self.restart_tool();
    }

    /// The pick aperture a tool is made with (ToolContext::pick_tolerance),
    /// 0.01 unless set - before start(). The vertex tools reach a vertex
    /// within 1.5 times it and a segment within it, as the view's 12 and 8 px.
    pub fn set_pick_tolerance(&mut self, tolerance: f64) {
        self.pick_tolerance = tolerance;
    }

    /// The running tool and the context of a call to it, lent to `act`.
    #[track_caller]
    pub fn with_tool<R>(
        &mut self,
        act: impl FnOnce(&mut dyn InteractiveTool, &ToolContext<'_>) -> R,
    ) -> R {
        let cx = context_of(&self.document, &self.view, self.pick_tolerance);
        let Some(tool) = self.tool.as_deref_mut() else {
            panic!("no tool is running: start() one first");
        };
        act(tool, &cx)
    }

    pub fn click(&mut self, x: f64, y: f64) -> ToolStep {
        let step = self.with_tool(|tool, cx| tool.point(cx, Point2::new(x, y)));
        self.apply(step)
    }

    /// What the view would draw with the cursor at (x, y).
    #[track_caller]
    pub fn preview(&self, x: f64, y: f64) -> ToolFeedback {
        self.tool().preview(&self.context(), Point2::new(x, y))
    }

    /// A click the plan view would snap: to the nearest end, middle, centre
    /// or intersection (the default modes) within 0.5 of (x, y) that the
    /// tool's step takes (InteractiveTool::takes_snap, as the view asks it),
    /// handed on as the view hands it (route_snapped_point) - a point of an
    /// entity reaches the tool as that point. Nothing within 0.5 is a plain
    /// click.
    pub fn click_snapped(&mut self, x: f64, y: f64) -> ToolStep {
        let found = self.snap_at(x, y);
        let at = found.map_or(Point2::new(x, y), |found| found.point);
        let step = self.with_tool(|tool, cx| route_snapped_point(tool, cx, at, found.as_ref()));
        self.apply(step)
    }

    /// What the view would draw with the cursor at (x, y) snapped as
    /// click_snapped snaps it.
    pub fn preview_snapped(&self, x: f64, y: f64) -> ToolFeedback {
        let found = self.snap_at(x, y);
        let at = found.map_or(Point2::new(x, y), |found| found.point);
        self.tool().preview(&self.context(), at)
    }

    /// The snap the view would make at (x, y), the tool asked as the view
    /// asks it.
    #[track_caller]
    pub fn snap_at(&self, x: f64, y: f64) -> Option<SnapResult> {
        let cx = self.context();
        let tool = self.tool();
        let accept: SnapAccept<'_> =
            Box::new(move |found: &SnapResult| tool.takes_snap(&cx, found));
        let request = SnapRequest {
            cursor: Point2::new(x, y),
            aperture: 0.5,
            view: Some(&self.view),
            accept: Some(accept),
            ..SnapRequest::default()
        };
        snap(self.document.model(), &request, None)
    }

    /// Layers the view hides of its own (ToolContext::view) - before start().
    pub fn hide_in_view(&mut self, layer: &str) {
        self.view.hide(layer);
    }

    pub fn pick(&mut self, id: EntityId, x: f64, y: f64) -> ToolStep {
        let step = self.with_tool(|tool, cx| tool.entity(cx, id, Point2::new(x, y)));
        self.apply(step)
    }

    /// Typed text, through the same routing the command line uses (the C++
    /// `type`).
    pub fn r#type(&mut self, text: &str) -> ToolStep {
        let step = self.with_tool(|tool, cx| route_typed_input(tool, cx, text));
        self.apply(step)
    }

    pub fn enter(&mut self) -> ToolStep {
        let step = self.with_tool(|tool, cx| tool.enter(cx));
        self.apply(step)
    }

    pub fn undo(&mut self) -> ToolStep {
        let step = self.with_tool(|tool, cx| tool.undo(cx));
        self.apply(step)
    }

    /// Esc, as the tool host has it: what the tool's cancel() keeps is
    /// executed, and the tool ends whatever it answers.
    pub fn cancel(&mut self) -> ToolStep {
        let mut step = self.with_tool(|tool, cx| tool.cancel(cx));
        step.restart = false;
        let applied = self.apply(step);
        self.finished = true;
        applied
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    /// The running tool, for its methods with the context().
    #[track_caller]
    pub fn tool(&self) -> &dyn InteractiveTool {
        match self.tool.as_deref() {
            Some(tool) => tool,
            None => panic!("no tool is running: start() one first"),
        }
    }

    #[track_caller]
    pub fn tool_mut(&mut self) -> &mut dyn InteractiveTool {
        match self.tool.as_deref_mut() {
            Some(tool) => tool,
            None => panic!("no tool is running: start() one first"),
        }
    }

    /// The context a call to the tool is lent now.
    pub fn context(&self) -> ToolContext<'_> {
        context_of(&self.document, &self.view, self.pick_tolerance)
    }

    /// The running tool's prompt (the C++ `tool().prompt()`).
    #[track_caller]
    pub fn prompt(&self) -> String {
        self.tool().prompt(&self.context())
    }

    #[track_caller]
    pub fn expects(&self) -> ToolInput {
        self.tool().expects(&self.context())
    }

    #[track_caller]
    pub fn last_point(&self) -> Option<Point2> {
        self.tool().last_point(&self.context())
    }

    #[track_caller]
    pub fn last_height(&self) -> Option<f64> {
        self.tool().last_height(&self.context())
    }

    /// Messages the tool reported, in order.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Commands executed since start().
    pub fn executed(&self) -> i32 {
        self.executed
    }

    /// True once the tool finished and did not ask to restart.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Adds an entity by command and returns its id - for setting up a Modify
    /// tool's input.
    #[track_caller]
    pub fn add(&mut self, command: CommandPtr) -> EntityId {
        if let Err(error) = self.document.execute(command) {
            panic!("{}", error.describe());
        }
        let created = self.document.last_created_entities();
        assert_eq!(created.len(), 1, "one entity made");
        created.first().copied().unwrap_or(INVALID_ENTITY_ID)
    }

    fn restart_tool(&mut self) {
        let Some(info) = self.info else {
            return;
        };
        let start = ToolStart {
            attributes: self.document.current_attributes(),
            selection: self.document.selection().ids(),
            // Once: taken by this tool, so a restart after its edit has none.
            handles: std::mem::take(&mut self.handles),
        };
        let cx = context_of(&self.document, &self.view, self.pick_tolerance);
        let Some(make) = info.make.as_ref() else {
            panic!("tool {} has no factory", info.id);
        };
        self.tool = Some(make(&cx, &start));
        self.finished = false;
    }

    #[track_caller]
    fn apply(&mut self, mut step: ToolStep) -> ToolStep {
        if !step.message.is_empty() {
            self.messages.push(step.message.clone());
        }
        if step.outcome == ToolStepOutcome::Done {
            if let Some(command) = step.command.take() {
                if let Err(error) = self.document.execute(command) {
                    panic!("{}", error.describe());
                }
                self.executed += 1;
            }
            if let Some(selection) = &step.selection {
                // As the tool host leaves a selecting tool's answer.
                self.document.selection_mut().set(selection.clone());
            }
            if step.restart {
                self.restart_tool();
            } else {
                self.finished = true;
            }
        }
        step
    }
}
