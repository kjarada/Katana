//! Survey > Subsurface Utilities (AS 5488): the UTILITY verb in the window
//! (docs/subsurface_utilities.md, docs/desktop.md). The port of survey/utility_workbench.
//!
//! Built as the Online Data workbench is: the main window only makes it, takes its menu rows, and
//! asks it to run any line that starts with UTILITY. The verb itself is the CommandInterpreter's,
//! shared with katana_cli; what this adds is what only a window has:
//!
//! * the menu section and its seven actions (utilityDraw, utilityReport, utilityVerify,
//!   utilityClearance, utilityCheck, utilityRegrade, utilityWriteSchedule), each opening the one
//!   dialog (utility_dialog) on its own tab;
//! * the drawing and the open views the dialog's scope and filter controls list (the host's);
//! * after a UTILITY DRAW or REGRADE that worked - typed, or run from the dialog - every plan view
//!   framed on the reply's bounds= box, read by the verb's own utility_verbs::draw_reply_bounds.
//!   Survey data in a real coordinate system usually lands far from what the view was showing,
//!   and left unframed a successful draw looked like one that did nothing.
//!
//! The dialog's Run goes back through the window's one executor (`Host::run`, MainWindow's
//! runVerbLine), so a line from the dialog is echoed, kept in the history and undone exactly as a
//! typed one; it reaches `run_line` like any other, which is where its reply is kept for the dialog
//! to show. A TYPED line reaches `run_line` only when no tool is waiting for typed text: then it is
//! the tool's - a label may read "Utility pit" - where the dialog's line never is.
//!
//! THE ONE RE-ENTRY. The dialog's Run happens while the workbench draws it, and the executor it
//! hands its line to comes back to this workbench's `run_line` for it. So what `run_line` keeps and
//! uses - the interpreter's door and the last reply - is a shared handle (`UtilityLines`) the
//! workbench, its dialog and the window's dispatcher each hold, rather than the workbench itself,
//! which is borrowed while it draws.
//!
//! The C++ hands the workbench the window's interpreter as a function (UtilityServices::interpret),
//! and so does this: `Host` lends the interpreter for reading only, and the window's
//! `interpreter_.run(line)` - with its history cursor reset - is the window's to supply.

use std::cell::RefCell;
use std::rc::Rc;

use egui::Ui;
use katana_cad::utilities::utility_verbs::draw_reply_bounds;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};

use super::utility_dialog::{
    UTILITY_TOOL_COUNT, UtilityDialogContext, UtilityExecute, UtilityTool, UtilityToolsDialog,
};
use crate::customisation::scope_filter_views;
use crate::customisation::scope_filter_widget::ViewsFn;
use crate::drive::action::{Action, Menu, ToolBar};
use crate::drive::registry::simplified;
use crate::host::Host;
use crate::shell::workbench::{DockSpec, Workbench};

/// The window's CommandInterpreter: runs one line on the drawing, keeps it in the history, and
/// returns its reply - the interpreter's own Result, so a refusal keeps its code.
pub type InterpretFn = Box<dyn FnMut(&mut dyn Host, &str) -> Result<String>>;

/// What the window hands the workbench (UtilityServices). The rest of the C++'s services - the
/// views, the log, whether the session is headless, the one executor, the actions - are the host's
/// and the menu rows'.
#[derive(Default)]
pub struct UtilityServices {
    /// Whether the dialog lists the host drawing's layers and alignments (the C++'s `document`,
    /// which a test may leave null).
    pub reads_drawing: bool,
    /// The window's interpreter; None answers every UTILITY line "there is no command interpreter
    /// here".
    pub interpret: Option<InterpretFn>,
}

/// One action of the menu section, in the order of the dialog's tabs. The letters are the ones the
/// Survey menu had left (--check-shortcuts): N and X for the two that act on what is drawn.
struct ToolAction {
    tool: UtilityTool,
    /// The icon's name in the window's icon set: the C++ Icon enumerator's.
    icon: &'static str,
    text: &'static str,
    tip: &'static str,
    name: &'static str,
}

const ACTIONS: [ToolAction; UTILITY_TOOL_COUNT] = [
    ToolAction {
        tool: UtilityTool::Draw,
        icon: "FormatStyles",
        text: "Draw &Utility Schedule...",
        tip: "Grade a utility schedule by AS 5488 quality level and add it to the drawing: a layer \
              for each type and level, a linetype for each level, a point at each located vertex - \
              one undo step",
        name: "utilityDraw",
    },
    ToolAction {
        tool: UtilityTool::Report,
        icon: "SurveyPointReport",
        text: "Utility Investigation Rep&ort...",
        tip: "Grade a utility schedule by AS 5488 quality level: each service's length at each \
              level, depth of cover, and what is claimed better than its evidence supports",
        name: "utilityReport",
    },
    ToolAction {
        tool: UtilityTool::Verify,
        icon: "SurveyInverse",
        text: "Verify Detections A&gainst Exposures...",
        tip: "Compare the QL-B detections with the QL-A exposures that check them, in plan and \
              level",
        name: "utilityVerify",
    },
    ToolAction {
        tool: UtilityTool::Clearance,
        icon: "Section",
        text: "Clearance of Proposed &Works...",
        tip: "Clearance of proposed works from each service, widened by its quality level's \
              tolerance",
        name: "utilityClearance",
    },
    ToolAction {
        tool: UtilityTool::Check,
        icon: "DatasetInfo",
        text: "Check Against a Delivery Sc&hema...",
        tip: "Check a utility schedule against a client's delivery schema: mandatory attributes \
              and their value lists",
        name: "utilityCheck",
    },
    ToolAction {
        tool: UtilityTool::Regrade,
        icon: "GlobalModify",
        text: "Regrade Draw&n Utilities...",
        tip: "Grade the drawn utility lines in a scope again from their points as they are now - \
              moved, levels or methods edited - and draw their runs again: one undo step",
        name: "utilityRegrade",
    },
    ToolAction {
        tool: UtilityTool::Schedule,
        icon: "SurveyExport",
        text: "E&xport Drawn Utilities as a Schedule...",
        tip: "Write the drawn utility lines in a scope as a schedule (.csv) that reads back \
              exactly: a drawing edited in CAD made a deliverable",
        name: "utilityWriteSchedule",
    },
];

/// The dialog every action opens, by object name: how --dialog finds it, since seven actions share
/// one dialog.
const DIALOG: &str = "utilityDialog";

/// What `run_line` keeps between a line and the dialog that asked for it.
struct LinesState {
    interpret: Option<InterpretFn>,
    /// The reply of the last line `run_line` ran, for `execute`.
    last_reply: Option<Result<String>>,
}

/// The UTILITY lines' runner, shared by the workbench, its dialog and the window's dispatcher (see
/// "THE ONE RE-ENTRY" above). Cloning it shares it.
#[derive(Clone)]
pub struct UtilityLines {
    state: Rc<RefCell<LinesState>>,
}

impl UtilityLines {
    fn new(interpret: Option<InterpretFn>) -> Self {
        Self {
            state: Rc::new(RefCell::new(LinesState {
                interpret,
                last_reply: None,
            })),
        }
    }

    /// True when `line` is a UTILITY line, which it then runs through the interpreter, replying to
    /// the log and, after a DRAW or a REGRADE that worked, framing what was drawn in every plan
    /// view; false leaves the line to whoever asked.
    pub fn run_line(&self, host: &mut dyn Host, line: &str) -> bool {
        let words = simplified(line);
        let mut said = words.split(' ');
        let verb = said.next().unwrap_or("");
        if !verb.eq_ignore_ascii_case("UTILITY") {
            return false;
        }
        // Taken out for the call, so that nothing the interpreter does can find the runner
        // borrowed.
        let taken = self.state.borrow_mut().interpret.take();
        let reply = match taken {
            Some(mut interpret) => {
                let reply = interpret(&mut *host, line);
                self.state.borrow_mut().interpret = Some(interpret);
                reply
            }
            None => Err(Error::new(
                ErrorCode::InvalidState,
                "there is no command interpreter here",
            )),
        };
        match &reply {
            Err(error) => host.log(&error.describe(), true),
            Ok(text) => {
                host.log(text.trim_end_matches('\n'), false);
                // A DRAW adds the services and a REGRADE draws them again: either way the views are
                // shown what the reply's bounds= box holds.
                let action = said.next().unwrap_or("");
                let draws =
                    action.eq_ignore_ascii_case("DRAW") || action.eq_ignore_ascii_case("REGRADE");
                if draws
                    && let Some(area) = draw_reply_bounds(text)
                    && let Some(views) = host.views()
                {
                    views.zoom_to(&area);
                }
            }
        }
        self.state.borrow_mut().last_reply = Some(reply);
        true
    }

    /// What the dialog's Run does: `line` through the window's one executor, and the reply
    /// `run_line` had for it - the interpreter's own, so a refusal keeps its code and its report.
    /// InvalidState when the line never reached `run_line`.
    pub fn execute(&self, host: &mut dyn Host, line: &str) -> Result<String> {
        self.state.borrow_mut().last_reply = None;
        host.run(line);
        // The reply run_line kept, not the runner's outcome: the interpreter's Result keeps a
        // refusal's code apart from its message, which the dialog's status shows alone.
        let kept = self.state.borrow().last_reply.clone();
        kept.unwrap_or_else(|| {
            Err(make_error(
                ErrorCode::InvalidState,
                "the command line did not run this as a UTILITY line",
                line,
            ))
        })
    }
}

/// The utilities workbench (see the module comment).
pub struct UtilityWorkbench {
    lines: UtilityLines,
    reads_drawing: bool,
    /// The dialog, made the first time it is asked for.
    dialog: Option<UtilityToolsDialog>,
}

impl UtilityWorkbench {
    pub fn new(services: UtilityServices) -> Self {
        Self {
            lines: UtilityLines::new(services.interpret),
            reads_drawing: services.reads_drawing,
            dialog: None,
        }
    }

    /// The UTILITY lines' runner, for the window's dispatcher to hold beside the workbench.
    pub fn lines(&self) -> UtilityLines {
        self.lines.clone()
    }

    /// The action of `tool`'s menu item.
    pub fn action(&self, tool: UtilityTool) -> Action {
        tool_action(&ACTIONS[tool.index()])
    }

    /// `UtilityLines::execute`.
    pub fn execute(&self, host: &mut dyn Host, line: &str) -> Result<String> {
        self.lines.execute(host, line)
    }

    /// The dialog, made the first time it is asked for: its Run through `execute`, its View scope
    /// offering the host's open views.
    pub fn dialog(&mut self, host: &mut dyn Host) -> &mut UtilityToolsDialog {
        let dialog = match self.dialog.take() {
            Some(dialog) => dialog,
            None => {
                let lines = self.lines.clone();
                let execute: UtilityExecute =
                    Box::new(move |host: &mut dyn Host, line: &str| lines.execute(host, line));
                let views = host.views().map(|access| -> ViewsFn {
                    let open = access.view_set();
                    Box::new(move || scope_filter_views(&open.borrow()))
                });
                let context = UtilityDialogContext {
                    execute: Some(execute),
                    reads_drawing: self.reads_drawing,
                    views,
                };
                UtilityToolsDialog::new(context, &*host)
            }
        };
        self.dialog.insert(dialog)
    }

    /// The dialog if it has been made.
    pub fn made_dialog(&mut self) -> Option<&mut UtilityToolsDialog> {
        self.dialog.as_mut()
    }

    /// Shows the dialog on `tool`'s tab.
    pub fn open(&mut self, host: &mut dyn Host, tool: UtilityTool) {
        let shown = self.dialog(host);
        shown.show_tool(tool);
        // The views open now, when the dialog is already showing too.
        shown.reload(&*host);
        shown.show(&*host);
    }
}

fn tool_action(entry: &ToolAction) -> Action {
    Action::new(entry.name, entry.text)
        .icon(entry.icon)
        .tip(entry.tip)
        .opens(DIALOG)
}

impl Workbench for UtilityWorkbench {
    /// The "Subsurface Utilities (AS 5488)" section and its seven actions, added to the Survey menu
    /// after what is already there. A section, as the Survey menu's others are: a style that draws
    /// its title says what the group is for.
    fn menus(&self) -> Vec<Menu> {
        let mut menu = Menu::new("surveyMenu", "&Survey").section("Subsurface Utilities (AS 5488)");
        for entry in &ACTIONS {
            menu = menu.action(tool_action(entry));
        }
        vec![menu]
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        Vec::new()
    }

    fn docks(&self) -> Vec<DockSpec> {
        Vec::new()
    }

    fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        self.lines.run_line(host, line)
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let Some(tool) = ACTIONS
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.tool)
        else {
            return false;
        };
        self.open(host, tool);
        true
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if let Some(dialog) = &mut self.dialog {
            dialog.ui(ui, host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ACTIONS;
    use crate::survey::utility_dialog::UTILITY_TOOLS;

    // The rows are in the order of the tabs, so a row's place is its tool's.
    #[test]
    fn each_action_row_is_at_its_tools_place() {
        for (at, tool) in UTILITY_TOOLS.iter().enumerate() {
            assert_eq!(ACTIONS[at].tool, *tool);
        }
    }
}
