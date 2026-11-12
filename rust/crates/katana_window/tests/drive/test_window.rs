//! A small window for the step engine's and the program's own cases: a few actions and a menu, a
//! dialog with a field, a check box, a choice, a note, a locked field and button and a hidden one,
//! a dock with a status line, a plan view the wheel and a report reach, and the program's other
//! calls answered by what each case needs. What it was asked is kept in a log the case reads,
//! shared, since the program owns the window it makes.

#![allow(
    dead_code,
    reason = "Each case reads only the parts of the window it drives."
)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use egui::{Event, Pos2, Sense, Ui, Vec2};
use katana_app::console::Console;
use katana_cad::{Document, PlotSettings};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_update::ApplyRequest;
use katana_window::canvas::Image;
use katana_window::drive::{
    self, Action, Choice, ControlKind, ControlRecord, DrivenWindow, Menu, MenuSigns, SheetsPlot,
    TargetKind, TargetSpec, action_item, begin_target, end_target,
};
use katana_window::host::VerbOutcome;
use katana_window::shell::{DialogFrame, popup_menu};

/// What the window was asked, in order, for a case to read after the program has dropped it.
pub type Log = Rc<RefCell<Vec<String>>>;

/// The dialog `openTest` opens.
#[derive(Debug, Default)]
pub struct TestDialog {
    pub open: bool,
    pub field: String,
    pub locked: String,
    pub agree: bool,
    pub choice: Choice,
    pub runs: usize,
}

impl TestDialog {
    fn body(&mut self, ui: &mut Ui) {
        drive::line(ui, "testField", &mut self.field);
        drive::check(ui, "testAgree", "Agree", &mut self.agree);
        let items = vec![String::from("Red"), String::from("Blue")];
        drive::choice(ui, "testChoice", &items, &mut self.choice);
        drive::label(ui, "testNote", "a <b>note</b>  as   written");
        ui.add_enabled_ui(false, |ui| {
            drive::line(ui, "testLockedField", &mut self.locked);
            drive::button(ui, "testLocked", "Locked");
        });
        ui.scope(|ui| {
            ui.set_invisible();
            drive::button(ui, "testHidden", "Hidden");
        });
    }

    fn footer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if drive::button(ui, "testRun", "&Run").clicked() {
                self.runs += 1;
            }
            if drive::button(ui, "testClose", "Close").clicked() {
                self.open = false;
            }
        });
    }
}

/// The window (see the module comment).
pub struct TestWindow {
    pub log: Log,
    pub document: Document,
    pub headless: bool,
    pub shown: bool,
    pub frame: DialogFrame,
    pub dialog: TestDialog,
    pub dock_open: bool,
    pub menu_open: bool,
    pub grid: bool,
    /// Frames still to say a job runs.
    pub busy_frames: Cell<usize>,
    /// Asks for a repaint at every frame, and so never comes idle.
    pub never_idle: bool,
    /// Draws one name twice in the window.
    pub draw_twice: bool,
    /// The wheel's notches the plan view saw, in lines.
    pub wheel: Vec<f32>,
    /// What run_command and run_verb_line refuse: a line holding it.
    pub refusing: String,
}

impl TestWindow {
    pub fn new(log: &Log) -> Self {
        Self {
            log: Rc::clone(log),
            document: Document::default(),
            headless: false,
            shown: false,
            frame: DialogFrame::new("testDialog", "Test Dialog"),
            dialog: TestDialog::default(),
            dock_open: false,
            menu_open: false,
            grid: false,
            busy_frames: Cell::new(0),
            never_idle: false,
            draw_twice: false,
            wheel: Vec::new(),
            refusing: String::from("BAD"),
        }
    }

    fn said(&self, what: &str) {
        self.log.borrow_mut().push(String::from(what));
    }

    fn refused(&self, line: &str) -> bool {
        !self.refusing.is_empty() && line.contains(self.refusing.as_str())
    }

    fn open_dialog(&mut self) -> Option<String> {
        self.dialog.open = true;
        Some(String::from("testDialog"))
    }
}

/// A new log.
pub fn new_log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

impl DrivenWindow for TestWindow {
    fn ui(&mut self, ui: &mut Ui) {
        if self.never_idle {
            ui.ctx().request_repaint();
        }
        drive::label(ui, "statusLabel", "ready   now");
        if self.draw_twice {
            drive::label(ui, "twice", "one");
            drive::label(ui, "twice", "two");
        }
        // The plan view: 200 x 100 points, reported by the window, turned by the wheel.
        let (rect, response) = ui.allocate_exact_size(Vec2::new(200.0, 100.0), Sense::hover());
        drive::custom(
            ui,
            &response,
            ControlRecord::new("PlanView1", ControlKind::View),
        );
        if drive::live(ui) {
            let pointer = ui.input(|input| input.pointer.latest_pos());
            for event in ui.input(|input| input.events.clone()) {
                if let Event::MouseWheel { delta, .. } = event
                    && pointer.is_some_and(|at| rect.contains(at))
                {
                    self.wheel.push(delta.y);
                }
            }
        }
        if self.dock_open {
            let shown = ui.vertical(|ui| {
                begin_target(
                    ui,
                    TargetSpec::new("TestDock", TargetKind::Dock, "Test Dock"),
                );
                drive::label(ui, "status", "3 points, 3 shown");
            });
            end_target(ui, shown.response.rect);
        }
        if self.menu_open {
            let item = Action::new("testMenuItem", "&Item").tip("Does a thing");
            let mut chosen = false;
            popup_menu(ui.ctx(), "testMenu", Pos2::new(20.0, 20.0), |ui| {
                chosen = action_item(ui, &item).clicked();
            });
            if chosen {
                self.menu_open = false;
            }
        }
        if self.dialog.open {
            let ctx = ui.ctx().clone();
            self.frame.show(
                &ctx,
                &mut self.dialog,
                |ui, dialog| dialog.body(ui),
                |ui, dialog| dialog.footer(ui),
            );
        }
    }

    fn busy(&self) -> bool {
        let left = self.busy_frames.get();
        if left > 0 {
            self.busy_frames.set(left - 1);
            return true;
        }
        false
    }

    fn application_icon(&self) -> Image {
        Image::new(4, 4)
    }

    fn set_headless(&mut self, headless: bool) {
        self.headless = headless;
        self.said(if headless { "headless" } else { "watched" });
    }

    fn load_default_customisation(&mut self) {
        self.said("loadDefaultCustomisation");
    }

    fn restore_session(&mut self) {
        self.said("restoreSession");
    }

    fn show(&mut self) {
        self.shown = true;
        self.said("show");
    }

    fn is_visible(&self) -> bool {
        self.shown
    }

    fn start_updates(&mut self) {
        self.said("startUpdates");
    }

    fn show_start_screen_if_wanted(&mut self) {
        self.said("showStartScreenIfWanted");
    }

    fn open_project(&mut self, directory: &Path) {
        self.said(&format!("openProject {}", directory.display()));
    }

    fn import_path(&mut self, path: &Path) {
        self.said(&format!("importPath {}", path.display()));
    }

    fn run_command(&mut self, line: &str) -> bool {
        self.said(&format!("> {line}"));
        !self.refused(line)
    }

    fn run_verb_line(&mut self, line: &str) -> VerbOutcome {
        self.said(&format!("run {line}"));
        if self.refused(line) {
            VerbOutcome::new(false, "", &format!("refused {line}"))
        } else {
            VerbOutcome::new(true, &format!("done\n{line}"), "")
        }
    }

    fn script_command_line(&self, path: &str) -> String {
        format!("SCRIPT {path}")
    }

    fn actions(&self) -> Vec<Action> {
        vec![
            Action::new("openTest", "&Open Test").opens("testDialog"),
            Action::new("showDock", "Show &Dock").opens("TestDock"),
            Action::new("gridToggle", "&Grid").checkable(self.grid),
            Action::new("disabledAction", "Disabled").enabled(false),
            Action::new("plainAction", "Plain"),
        ]
    }

    fn menus(&self) -> Vec<Menu> {
        vec![
            Menu::new("testMenu", "&Test")
                .action(Action::new("testMenuItem", "&Item").tip("Does a thing"))
                .separator()
                .submenu(Menu::new("testSubMenu", "&More").action(Action::new("deepItem", "Deep"))),
        ]
    }

    fn trigger_action(&mut self, name: &str) -> Status {
        self.said(&format!("trigger {name}"));
        match name {
            "openTest" => {
                self.dialog.open = true;
                Ok(())
            }
            "showDock" => {
                self.dock_open = true;
                Ok(())
            }
            "gridToggle" => {
                self.grid = !self.grid;
                Ok(())
            }
            "plainAction" => Ok(()),
            "disabledAction" => Err(make_error(
                ErrorCode::InvalidState,
                "that menu item is disabled",
                name,
            )),
            _ => Err(make_error(
                ErrorCode::NotFound,
                "no menu item or tool has that name",
                name,
            )),
        }
    }

    fn select_all(&mut self) {
        self.said("selectAll");
    }

    fn select_only(&mut self, id: EntityId) {
        self.said(&format!("selectOnly {id}"));
    }

    fn toggle_layer_through_panel(&mut self, layer: &str) -> Status {
        if layer == "missing" {
            return Err(make_error(ErrorCode::NotFound, "no such layer", layer));
        }
        self.said(&format!("toggle {layer}"));
        Ok(())
    }

    fn open_style_manager(&mut self) -> String {
        self.said("styleManager");
        self.open_dialog().unwrap_or_default()
    }

    fn open_attribute_manager(&mut self) -> String {
        self.said("attributeManager");
        self.open_dialog().unwrap_or_default()
    }

    fn open_layer_manager(&mut self) -> String {
        self.said("layerManager");
        self.open_dialog().unwrap_or_default()
    }

    fn open_dataset_info(&mut self, path: &Path) -> Option<String> {
        self.said(&format!("datasetInfo {}", path.display()));
        if path.ends_with("none") {
            return None;
        }
        self.open_dialog()
    }

    fn open_import_options(&mut self, path: &Path) -> Option<String> {
        self.said(&format!("importOptions {}", path.display()));
        if path.ends_with("none") {
            return None;
        }
        self.open_dialog()
    }

    fn open_export_options(&mut self, path: &Path) -> Option<String> {
        self.said(&format!("exportOptions {}", path.display()));
        self.open_dialog()
    }

    fn open_menu(&mut self, name: &str) -> bool {
        if name == "testMenu" {
            self.menu_open = true;
            return true;
        }
        false
    }

    fn pointer_at(&mut self, spec: &str, click: bool) -> Result<String> {
        let parts: Vec<&str> = spec.split(',').collect();
        if parts.len() < 2
            || parts
                .iter()
                .take(2)
                .any(|part| part.parse::<f64>().is_err())
        {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "the pointer needs x,y[,shift|ctrl]",
                spec,
            ));
        }
        Ok(format!(
            "pointer: action={} x={} y={} tool=none",
            if click { "click" } else { "hover" },
            parts[0],
            parts[1]
        ))
    }

    fn report_view(&mut self, name: &str) -> Option<String> {
        (name == "PlanView1").then(|| String::from("drawn=3  ghosts=0 grips=0"))
    }

    fn shortcut_clashes(&self) -> (Vec<String>, usize) {
        (Vec::new(), 12)
    }

    fn menu_gaps(&self) -> (Vec<String>, usize) {
        (Vec::new(), 7)
    }

    fn menu_sign_clashes(&mut self) -> MenuSigns {
        MenuSigns {
            measured: vec![String::from(
                "toolbar button EditToolBar > editUndo: sign clear of its icon",
            )],
            clashes: Vec::new(),
            buttons: 1,
        }
    }

    fn check_tool_bar_fit(&mut self, sizes: &[(u32, u32)], err: &mut dyn Console) -> bool {
        for (width, height) in sizes {
            err.err(&format!("toolbar fit {width}x{height}: held\n"));
        }
        true
    }

    fn check_session_restore(&mut self, err: &mut dyn Console) -> bool {
        err.err("session restore: held\n");
        true
    }

    fn document(&self) -> &Document {
        &self.document
    }

    fn plot_drawing_to_pdf(&mut self, path: &Path, _settings: PlotSettings, fit: bool) -> Status {
        self.said(&format!("plot {} fit={fit}", path.display()));
        Ok(())
    }

    fn plot_sheets(&mut self, plot: &SheetsPlot) -> Result<Vec<PathBuf>> {
        self.said(&format!("plotSheets {}", plot.path.display()));
        Ok(vec![plot.path.join("sheet1.pdf")])
    }

    fn plot_format_known(format: &str) -> bool {
        ["pdf", "pdfs", "png", "tiff"].contains(&format)
    }

    fn take_pending_update(&mut self) -> Option<ApplyRequest> {
        None
    }

    fn update_failure_is_boxed(&self, _step_kinds: &[String]) -> bool {
        false
    }
}
