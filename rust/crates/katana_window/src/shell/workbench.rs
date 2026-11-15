//! A workbench: the window's way of being extended by a family of commands - survey, utilities,
//! IFC, roads, the online data, the customisation managers, updates - each delivering its menu and
//! toolbar rows, its docks, the verbs it runs and what it draws, so the main window (K) composes
//! them in the C++'s order and no agent edits a shared list (the plan's section 6, hotspot 4).
//!
//! The C++ workbenches are handed the QMainWindow and fill its menus and toolbars as they are
//! built, keep their dialogs as children of it, and are asked `runLine` by the dispatcher before
//! the window's own verbs (MainWindow::runWorkbenchLine). Here the window asks: the rows to draw,
//! the line to run, the action to trigger, and each frame what to draw - and lends itself as the
//! host for each call, so a workbench holds no window.

use egui::Ui;

use crate::drive::action::{Menu, ToolBar};
use crate::host::Host;

/// A dock a workbench adds: its object name (a target's name, "surveyPointManagerDock") and the
/// title its tab shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockSpec {
    pub name: String,
    pub title: String,
}

impl DockSpec {
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: String::from(name),
            title: String::from(title),
        }
    }
}

/// One family of commands in the window (see the module comment).
pub trait Workbench {
    /// What it adds to the window's menus: each menu named as the window's menu it fills
    /// ("surveyMenu"), with its items in their order; the window merges them by name.
    fn menus(&self) -> Vec<Menu>;
    /// Its toolbars.
    fn tool_bars(&self) -> Vec<ToolBar>;
    /// The docks it shows, drawn by `dock_ui` in the window's dock area.
    fn docks(&self) -> Vec<DockSpec>;
    /// Runs `line` if it is one of this workbench's verbs (its C++ runLine): true when it took
    /// the line, which it has logged and run - or started as a job - through `host`.
    fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool;
    /// Triggers its action `name` as a click does; false when it has no action of that name.
    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool;
    /// Draws its open dialogs for this frame, each in its frame (shell::dialog).
    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host);
    /// Draws the content of its dock `dock`.
    fn dock_ui(&mut self, ui: &mut Ui, host: &mut dyn Host, dock: &str) {
        let _ = (ui, host, dock);
    }
}
