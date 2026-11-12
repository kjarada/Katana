//! The steps: how a headless run acts on the window as a person would, one step after another -
//! the port of main.cpp's step loop (its `--dialog`, `--survey-dock`, `--panel`, `--fill`,
//! `--press`, `--command`, `--enter`, `--run-line`, `--report`, `--trigger`, `--export-options`,
//! `--wheel`, `--hover`, `--click` and `--script`, with openDialog, openSurveyDock, reportWidget,
//! turnWheel, runLine and runScriptFile), and of tools/check_screenshot.cmake's `-DDRIVE` sigils,
//! which a test writes a step with (docs/headless.md, "The steps"):
//!
//! ```text
//! @NAME     --dialog NAME          the dialog action NAME opens, made the target
//! #NAME     --survey-dock NAME     the dock action NAME shows, made the target
//! %NAME     --panel NAME           the window's dock, toolbar, menu or open popup NAME
//! F=TEXT    --fill F=TEXT          anything with no sigil: a field of the target filled
//! !NAME     --press NAME           a button of the target pressed
//! >TEXT     --command TEXT         TEXT typed on the command line; ">" alone is --enter
//! <TEXT     --run-line TEXT        TEXT through the one executor, as a dialog runs its line
//! ?NAME     --report NAME          what the widget, action, menu or view NAME shows
//! *NAME     --trigger NAME         the menu item NAME triggered
//! ^FILE     --export-options FILE  File > Export Drawing's dialog for FILE, made the target
//! ~X,Y      --hover X,Y            the pointer moved to model point X,Y in the plan view
//! ~NAME ..  --wheel "NAME X,Y N"   N notches of the wheel over widget NAME ("~" then a letter)
//! +X,Y      --click X,Y[,shift|,ctrl]  the pointer moved there and clicked
//! ```
//!
//! and `--script FILE`, a step with no sigil (check_screenshot.cmake's `-DSCRIPT`).
//!
//! HOW EACH IS DONE. A step that the C++ did through a widget it found by `findChild` is done
//! through the drive layer: a fill or a press is POSTED to the mailbox for the target and the
//! control's object name, and the control takes it as it is drawn (drive::controls); a report reads
//! the registry's records of the last frame; the wheel is an egui event at the control's rectangle.
//! A step that the C++ did through a call on the window is a call on `DrivenWindow`. After each
//! step the window is drawn until it is idle (drive::headless), where the C++ ran its event loop
//! once or twice: what a step set going has happened before the next.
//!
//! THE TARGET is what the steps made the place their fills and presses go and what `--screenshot`
//! grabs, by the name the registry draws it under; the C++ kept a guarded pointer to it, so a
//! target that closed itself left the window the target again ("NAME has closed: the window is
//! grabbed"). Here a target has closed when the last frame did not draw it.
//!
//! WHAT IS NOT THE C++'s. Settling is checked: a window that does not come idle after a step, a
//! name drawn twice in one target, and an act refused all end the run with exit 1 and their
//! sentence, where the C++'s event loop had no such failure. A step that opens a dialog, a dock, a
//! menu or the export dialog is followed by frames before the next step, since a target exists for
//! the mailbox only once a frame has drawn it; Qt showed a widget at once.

use egui::{Event, Modifiers, MouseWheelUnit, Pos2, TouchPhase, Vec2};
use katana_app::console::Console;
use katana_core::path_text::path_from_utf8;
use katana_core::text::{format_general, parse_finite_double, parse_integer};

use super::action::{Action, Menu, MenuItem};
use super::headless::Headless;
use super::mailbox::Posted;
use super::program::DrivenWindow;
use super::registry::{ControlKind, Snapshot, TargetKind, WINDOW, simplified};

/// QString::toDouble: the number, read the C locale's way, whitespace round it ignored; None for
/// text that is no finite number (Qt answers 0 with ok false).
pub fn qt_double(text: &str) -> Option<f64> {
    parse_finite_double(text.trim())
}

/// QString::toInt: a whole number that fits an int, whitespace round it ignored.
pub fn qt_int(text: &str) -> Option<i32> {
    parse_integer(text.trim()).and_then(|number| i32::try_from(number).ok())
}

/// What a step does: its switch in main.cpp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepKind {
    Dialog,
    SurveyDock,
    Panel,
    Fill,
    Press,
    /// `--command`, and `--enter` (an empty one).
    Command,
    RunLine,
    Report,
    Trigger,
    ExportOptions,
    Wheel,
    Hover,
    Click,
    Script,
}

impl StepKind {
    /// The switch main.cpp records the step under, and prints in its sentences.
    pub const fn switch(self) -> &'static str {
        match self {
            Self::Dialog => "--dialog",
            Self::SurveyDock => "--survey-dock",
            Self::Panel => "--panel",
            Self::Fill => "--fill",
            Self::Press => "--press",
            Self::Command => "--command",
            Self::RunLine => "--run-line",
            Self::Report => "--report",
            Self::Trigger => "--trigger",
            Self::ExportOptions => "--export-options",
            Self::Wheel => "--wheel",
            Self::Hover => "--hover",
            Self::Click => "--click",
            Self::Script => "--script",
        }
    }

    /// The step a switch that takes a value is (main.cpp's argument loop): `--survey-dialog` is
    /// `--dialog`'s first name. None for any other switch; `--enter`, which takes none, is
    /// `Step::enter`.
    pub fn from_switch(switch: &str) -> Option<Self> {
        Some(match switch {
            "--dialog" | "--survey-dialog" => Self::Dialog,
            "--survey-dock" => Self::SurveyDock,
            "--panel" => Self::Panel,
            "--fill" => Self::Fill,
            "--press" => Self::Press,
            "--command" => Self::Command,
            "--run-line" => Self::RunLine,
            "--report" => Self::Report,
            "--trigger" => Self::Trigger,
            "--export-options" => Self::ExportOptions,
            "--wheel" => Self::Wheel,
            "--hover" => Self::Hover,
            "--click" => Self::Click,
            "--script" => Self::Script,
            _ => return None,
        })
    }
}

/// One step: what it does and the text it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub text: String,
}

impl Step {
    pub fn new(kind: StepKind, text: &str) -> Self {
        Self {
            kind,
            text: String::from(text),
        }
    }

    /// `--enter`: Enter on an empty command line, an empty `--command`.
    pub fn enter() -> Self {
        Self::new(StepKind::Command, "")
    }

    /// The step a `-DDRIVE` step is, by its sigil (check_screenshot.cmake): the first character,
    /// and the rest its text; anything with no sigil is a fill of the whole step.
    pub fn from_drive(step: &str) -> Self {
        let mut characters = step.chars();
        let sigil = characters.next();
        let rest = characters.as_str();
        let kind = match sigil {
            Some('@') => StepKind::Dialog,
            Some('#') => StepKind::SurveyDock,
            Some('%') => StepKind::Panel,
            Some('>') => StepKind::Command,
            Some('<') => StepKind::RunLine,
            Some('?') => StepKind::Report,
            Some('*') => StepKind::Trigger,
            Some('!') => StepKind::Press,
            Some('^') => StepKind::ExportOptions,
            // A widget name first is the wheel over it; a model point first is the pointer.
            Some('~') if rest.starts_with(|c: char| c.is_ascii_alphabetic()) => StepKind::Wheel,
            Some('~') => StepKind::Hover,
            Some('+') => StepKind::Click,
            _ => return Self::new(StepKind::Fill, step),
        };
        Self::new(kind, rest)
    }

    /// The switch main.cpp records it under.
    pub fn switch(&self) -> &'static str {
        self.kind.switch()
    }
}

/// A `-DDRIVE` list: its steps, '|' between them (check_screenshot.cmake).
pub fn drive_steps(drive: &str) -> Vec<Step> {
    drive.split('|').map(Step::from_drive).collect()
}

/// main.cpp's runLine: `text` through the window's one executor, as a dialog runs a line, and what
/// the dialog gets printed back - the ok, then each line of the reply and of the errors. Whether
/// it was ok.
pub fn run_line(window: &mut dyn DrivenWindow, text: &str, console: &mut dyn Console) -> bool {
    let outcome = window.run_verb_line(text);
    console.err(&format!(
        "--run-line {text}: ok={}\n",
        if outcome.ok { "yes" } else { "no" }
    ));
    for (label, lines) in [("reply", &outcome.reply), ("error", &outcome.error)] {
        if lines.is_empty() {
            continue;
        }
        for line in lines.split('\n') {
            console.err(&format!("  {label}: {line}\n"));
        }
    }
    outcome.ok
}

/// main.cpp's runScriptFile: the file run by the window's SCRIPT verb through the one executor,
/// stopping at the first line refused. Whether every line ran.
pub fn run_script_file(
    window: &mut dyn DrivenWindow,
    path: &str,
    console: &mut dyn Console,
) -> bool {
    let line = window.script_command_line(path);
    if window.run_verb_line(&line).ok {
        return true;
    }
    console.err(&format!(
        "--script {path} stopped at a line that was refused\n"
    ));
    false
}

/// The menu `name` among `menus` and their submenus (a QMenu found by `findChild`).
pub fn find_menu<'a>(menus: &'a [Menu], name: &str) -> Option<&'a Menu> {
    menus.iter().find_map(|menu| menu_named(menu, name))
}

/// `menu` itself when it is called `name`, else its submenu of that name, depth first.
fn menu_named<'a>(menu: &'a Menu, name: &str) -> Option<&'a Menu> {
    if menu.name == name {
        return Some(menu);
    }
    menu.items.iter().find_map(|item| match item {
        MenuItem::Menu(submenu) => menu_named(submenu, name),
        MenuItem::Action(_) | MenuItem::Section(_) | MenuItem::Separator => None,
    })
}

/// The step engine: the target the steps made, and the docks they showed (see the module
/// comment).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepEngine {
    /// The target, by the name the registry draws it under; None until a step makes one.
    target: Option<String>,
    /// The target as the step named it (main.cpp's targetName): `--dialog surveyInverse` opened
    /// `surveyInverseDialog`, and the refusals of a press there name `surveyInverse`.
    said_as: String,
    /// The docks `--survey-dock` showed, by their object names, whose status lines end the run.
    docks: Vec<String>,
}

impl StepEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// The steps start on target `name`, named as it is drawn: the dialog `--dataset-info` or
    /// `--import-options` made.
    pub fn start_on(&mut self, name: &str) {
        self.target = Some(String::from(name));
        self.said_as = String::from(name);
    }

    /// The target the steps made, as the registry names it; None for the window.
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// The target as the steps named it.
    pub fn said_as(&self) -> &str {
        &self.said_as
    }

    /// The docks `--survey-dock` showed.
    pub fn docks(&self) -> &[String] {
        &self.docks
    }

    /// The target if `frame` drew it: None when there is none, or it has closed.
    pub fn live_target<'a>(&'a self, frame: &Snapshot) -> Option<&'a str> {
        self.target
            .as_deref()
            .filter(|name| frame.target(name).is_some())
    }

    /// Frames until the window is idle; false, said, when it is not.
    fn settle(
        window: &mut dyn DrivenWindow,
        headless: &mut Headless,
        console: &mut dyn Console,
    ) -> bool {
        match headless.settle(window) {
            Ok(()) => true,
            Err(problem) => {
                console.err(&format!("{problem}\n"));
                false
            }
        }
    }

    /// Takes `step` in its turn; false, said on `console`, when the run must end with exit 1.
    pub fn run(
        &mut self,
        step: &Step,
        window: &mut dyn DrivenWindow,
        headless: &mut Headless,
        console: &mut dyn Console,
    ) -> bool {
        let text = step.text.as_str();
        match step.kind {
            StepKind::Trigger => {
                // --action, in its turn among the steps: a menu command that acts on what the
                // steps before it made.
                if let Err(error) = window.trigger_action(text) {
                    console.err(&format!("--trigger {text} failed: {}\n", error.describe()));
                    return false;
                }
                Self::settle(window, headless, console)
            }
            StepKind::Command => {
                window.run_command(text);
                Self::settle(window, headless, console)
            }
            StepKind::RunLine => {
                run_line(window, text, console);
                Self::settle(window, headless, console)
            }
            StepKind::Script => {
                if !run_script_file(window, text, console) {
                    return false;
                }
                Self::settle(window, headless, console)
            }
            StepKind::Hover | StepKind::Click => {
                // The pointer, where a headless run has no mouse: moved to a model point in the
                // plan view, and clicked, as real events; what the view then showed is printed.
                match window.pointer_at(text, step.kind == StepKind::Click) {
                    Ok(record) => console.err(&format!("{record}\n")),
                    Err(error) => {
                        console.err(&format!("{} {text}: {}\n", step.switch(), error.describe()));
                        return false;
                    }
                }
                Self::settle(window, headless, console)
            }
            StepKind::Panel => self.panel(text, window, headless, console),
            StepKind::Dialog => self.dialog(text, window, headless, console),
            StepKind::SurveyDock => self.survey_dock(text, window, headless, console),
            StepKind::ExportOptions => {
                let opened = window.open_export_options(&path_from_utf8(text));
                console.err(&format!(
                    "--export-options {text}: opened vectorExportDialog\n"
                ));
                let Some(name) = opened else {
                    return false;
                };
                self.target = Some(name.clone());
                self.said_as = name;
                Self::settle(window, headless, console)
            }
            StepKind::Report => {
                // The window's own widgets and actions before any dialog: what a command line
                // step changed there.
                let frame = headless.snapshot();
                match self.report_widget(&frame, window, text) {
                    Some(report) => {
                        console.err(&format!("{text}: {}\n", simplified(&report)));
                        true
                    }
                    None => {
                        console.err(&format!(
                            "--report: there is no label, field, text, list, button, view, action or menu {text}\n"
                        ));
                        false
                    }
                }
            }
            StepKind::Wheel => self.turn_wheel(text, window, headless, console),
            StepKind::Fill | StepKind::Press => self.act(step, window, headless, console),
        }
    }

    /// `--panel NAME`: a menu of the window opened under its title, as a click on the menu bar
    /// opens it; else the window's dock or toolbar, or a window of its own a step opened and that
    /// is shown (a view bar's Layers popup, a dialog another dialog's button opened).
    fn panel(
        &mut self,
        text: &str,
        window: &mut dyn DrivenWindow,
        headless: &mut Headless,
        console: &mut dyn Console,
    ) -> bool {
        if window.open_menu(text) {
            if !Self::settle(window, headless, console) {
                return false;
            }
        } else {
            let frame = headless.snapshot();
            let taken = frame.target(text).is_some_and(|target| {
                matches!(
                    target.kind,
                    TargetKind::Dock
                        | TargetKind::ToolBar
                        | TargetKind::Popup
                        | TargetKind::Dialog
                        | TargetKind::Menu
                )
            });
            if !taken {
                console.err(&format!(
                    "--panel: the window has no dock, toolbar, menu or open popup {text}\n"
                ));
                return false;
            }
        }
        self.target = Some(String::from(text));
        self.said_as = String::from(text);
        true
    }

    /// main.cpp's openDialog: the dialog action NAME opens, triggered as a click does, found by the
    /// object name the action carries as its data, else the action's own plus "Dialog". NAME may
    /// also be the dialog's own name when no action is called that: the first action whose data
    /// names it is the one triggered. What opened is said.
    fn dialog(
        &mut self,
        asked: &str,
        window: &mut dyn DrivenWindow,
        headless: &mut Headless,
        console: &mut dyn Console,
    ) -> bool {
        let actions = window.actions();
        let mut name = String::from(asked);
        if !actions.iter().any(|action| action.name == asked)
            && let Some(action) = actions
                .iter()
                .find(|action| !action.name.is_empty() && action.dialog.as_deref() == Some(asked))
        {
            name.clone_from(&action.name);
        }
        if let Err(error) = window.trigger_action(&name) {
            console.err(&format!("--dialog {asked} failed: {}\n", error.describe()));
            return false;
        }
        if !Self::settle(window, headless, console) {
            return false;
        }
        let named = window
            .actions()
            .into_iter()
            .find(|action| action.name == name)
            .and_then(|action| action.dialog)
            .filter(|dialog| !dialog.is_empty())
            .unwrap_or_else(|| format!("{name}Dialog"));
        let frame = headless.snapshot();
        let Some(dialog) = frame
            .target(&named)
            .filter(|target| target.kind == TargetKind::Dialog)
        else {
            console.err(&format!(
                "--dialog: {asked} opened no dialog named {named}\n"
            ));
            return false;
        };
        // By the name that was asked for, as before there were two ways to ask.
        console.err(&format!(
            "--dialog {asked} opened {named} \"{}\"{}\n",
            dialog.title,
            if dialog.modal { " (modal)" } else { "" }
        ));
        self.target = Some(named);
        self.said_as = String::from(asked);
        true
    }

    /// main.cpp's openSurveyDock: the action that shows a survey dock triggered (unless it is
    /// already ticked) and the dock it names in its data found.
    fn survey_dock(
        &mut self,
        name: &str,
        window: &mut dyn DrivenWindow,
        headless: &mut Headless,
        console: &mut dyn Console,
    ) -> bool {
        let action: Option<Action> = window
            .actions()
            .into_iter()
            .find(|action| action.name == name);
        let Some((dock, checked)) = action.and_then(|action| {
            let checked = action.checked;
            action
                .dialog
                .filter(|dock| !dock.is_empty())
                .map(|dock| (dock, checked))
        }) else {
            console.err(&format!(
                "--survey-dock: no action {name} that names a dock\n"
            ));
            return false;
        };
        if checked != Some(true) {
            // QAction::trigger: a disabled action does nothing, and says nothing.
            let _ = window.trigger_action(name);
        }
        if !Self::settle(window, headless, console) {
            return false;
        }
        let frame = headless.snapshot();
        if !frame
            .target(&dock)
            .is_some_and(|target| target.kind == TargetKind::Dock)
        {
            console.err(&format!(
                "--survey-dock: {name} showed no dock named {dock}\n"
            ));
            return false;
        }
        self.docks.push(dock.clone());
        self.target = Some(dock);
        self.said_as = String::from(name);
        true
    }

    /// main.cpp's reportWidget: what NAME shows - a control of the target, else one of the window's
    /// menus, else one of its actions, else a control anywhere in the window; a view (PlanView2,
    /// RenderView2) is reported by the window, painted afresh. None for none of these.
    pub fn report_widget(
        &self,
        frame: &Snapshot,
        window: &mut dyn DrivenWindow,
        name: &str,
    ) -> Option<String> {
        let target = self.live_target(frame).unwrap_or(WINDOW);
        let in_target = if target == WINDOW {
            frame.control_anywhere(name)
        } else {
            frame.control(target, name)
        };
        if let Some(control) = in_target {
            return if control.kind == ControlKind::View {
                window.report_view(name)
            } else {
                control.report()
            };
        }
        if let Some(menu) = find_menu(&window.menus(), name) {
            return Some(menu.report());
        }
        if let Some(action) = window
            .actions()
            .into_iter()
            .find(|action| action.name == name)
        {
            return Some(action.report());
        }
        // Last, any of the window's own widgets: the status bar's FrameStatsLabel after a dialog
        // framed the views.
        match frame.control_anywhere(name) {
            Some(control) if control.kind != ControlKind::View => control.report(),
            _ => window.report_view(name),
        }
    }

    /// main.cpp's turnWheel: "NAME X,Y N", N notches of the wheel - positive turned away from the
    /// person, which zooms in - over the control NAME at X,Y in its points, NAME found as a report
    /// finds one. Each notch is an egui wheel event of one line, as egui-winit makes a wheel's
    /// notch, at that point, with the window drawn until idle after each.
    fn turn_wheel(
        &mut self,
        step: &str,
        window: &mut dyn DrivenWindow,
        headless: &mut Headless,
        console: &mut dyn Console,
    ) -> bool {
        let words: Vec<&str> = step.split(' ').filter(|word| !word.is_empty()).collect();
        let at: Vec<&str> = if words.len() == 3 {
            words[1].split(',').collect()
        } else {
            Vec::new()
        };
        let x = if at.len() == 2 {
            qt_double(at[0])
        } else {
            None
        };
        let y = if at.len() == 2 {
            qt_double(at[1])
        } else {
            None
        };
        let notches = if words.len() == 3 {
            qt_int(words[2])
        } else {
            None
        };
        let (Some(x), Some(y), Some(notches)) = (x, y, notches.filter(|&notches| notches != 0))
        else {
            console.err(&format!(
                "--wheel: usage: --wheel \"NAME X,Y NOTCHES\", not \"{step}\"\n"
            ));
            return false;
        };
        let name = words[0];
        let frame = headless.snapshot();
        let target = self.live_target(&frame).unwrap_or(WINDOW);
        let found = if target == WINDOW {
            frame.control_anywhere(name)
        } else {
            frame
                .control(target, name)
                .or_else(|| frame.control_anywhere(name))
        };
        let Some(control) = found else {
            console.err(&format!("--wheel: there is no widget {name}\n"));
            return false;
        };
        let rect = control.rect;
        let inside =
            x >= 0.0 && y >= 0.0 && x <= f64::from(rect.width()) && y <= f64::from(rect.height());
        if !inside {
            console.err(&format!(
                "--wheel: {},{} is outside {name}, {} x {}\n",
                format_general(x, 6),
                format_general(y, 6),
                rect.width().round() as i32,
                rect.height().round() as i32
            ));
            return false;
        }
        let point = Pos2::new(rect.min.x + x as f32, rect.min.y + y as f32);
        let towards = if notches > 0 { 1.0 } else { -1.0 };
        for _ in 0..notches.unsigned_abs() {
            headless.push_event(Event::PointerMoved(point));
            headless.push_event(Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: Vec2::new(0.0, towards),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            });
            if !Self::settle(window, headless, console) {
                return false;
            }
        }
        console.err(&format!(
            "--wheel {name}: {notches} notches at {},{}\n",
            at[0], at[1]
        ));
        true
    }

    /// A fill or a press of the target's control: posted to the mailbox and taken as the control
    /// is drawn, or refused in the C++'s sentence.
    fn act(
        &mut self,
        step: &Step,
        window: &mut dyn DrivenWindow,
        headless: &mut Headless,
        console: &mut dyn Console,
    ) -> bool {
        let text = step.text.as_str();
        let frame = headless.snapshot();
        let Some(target) = self.live_target(&frame).map(String::from) else {
            if self.said_as.is_empty() {
                console.err(&format!(
                    "{} {text} comes before any --dialog\n",
                    step.switch()
                ));
            } else {
                console.err(&format!(
                    "{} {text}: {} has closed\n",
                    step.switch(),
                    self.said_as
                ));
            }
            return false;
        };
        let posted = if step.kind == StepKind::Fill {
            let Some(equals) = text.find('=').filter(|&at| at > 0) else {
                console.err(&format!("--fill needs FIELD=TEXT, not {text}\n"));
                return false;
            };
            Posted::fill(&target, &text[..equals], &text[equals + 1..])
        } else {
            Posted::press(&target, text)
        };
        match headless.act(window, posted.said_as(&self.said_as)) {
            Ok(()) => true,
            Err(sentence) => {
                console.err(&format!("{sentence}\n"));
                false
            }
        }
    }

    /// The end of the steps (main.cpp after its loop): each dock `--survey-dock` showed says its
    /// status line, and a target that has closed is said; the name of what `--screenshot` grabs -
    /// the target, or the window.
    pub fn finish(&self, frame: &Snapshot, console: &mut dyn Console) -> String {
        for dock in &self.docks {
            let status = frame
                .control(dock, "status")
                .filter(|control| control.kind == ControlKind::Label)
                .map_or_else(
                    || String::from("(no status)"),
                    |control| control.text.clone(),
                );
            console.err(&format!("{dock}: {status}\n"));
        }
        // Steps that were all --command leave no target: the window is what they changed. So is
        // it when the target closed itself.
        match self.live_target(frame) {
            Some(target) => String::from(target),
            None => {
                if !self.said_as.is_empty() {
                    console.err(&format!(
                        "{} has closed: the window is grabbed\n",
                        self.said_as
                    ));
                }
                String::from(WINDOW)
            }
        }
    }
}
