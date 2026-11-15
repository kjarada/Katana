//! The harness the window's tests drive a dialog, a panel or a workbench with: one egui context
//! with no window - a fixed screen, a fixed time step - the registry installed, and the acts a
//! test posts by the C++ object names (the plan's section 4.7):
//!
//! ```text
//! let mut harness = Harness::new();                        // 1360 x 860, a pixel a point
//! let mut host = RecordingHost::new();
//! harness.run(|ui| dialog.ui(ui, &mut host))?;             // frames until idle
//! harness.fill("vectorImportWhere", "kind=road").run(|ui| dialog.ui(ui, &mut host))?;
//! harness.press("vectorImportPreview").run(|ui| dialog.ui(ui, &mut host))?;
//! assert_eq!(harness.report("vectorImportMatchCount")?, "3 matched");
//! assert_eq!(host.lines(), ["IMPORT ..."]);
//! ```
//!
//! The acts are posted first and the frames run after, with the drawing closure lent for the run
//! only, so a test reads its dialog and host between two runs. It is the same registry and mailbox
//! the headless program drives (D's runner): one door.
//!
//! IDLE is what the C++'s two `processEvents()` after each step stood for: no repaint asked (an
//! animation, a scroll, a measuring pass), and no act still waiting for its control. A run that
//! does not come idle within its cap of frames fails, saying so.
//!
//! No winit and no eframe: nothing here opens a window (the step-7 brief, "Never open a window").

use std::time::Duration;

use egui::{Context, Event, Modifiers, PointerButton, Pos2, Rect, Ui, Vec2, ViewportId};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};

use super::action::{Action, Menu, MenuItem, ToolBar, ToolBarItem, action_item};
use super::mailbox::{ActId, ActOutcome, Posted};
use super::registry::{
    ControlRecord, Part, Snapshot, TargetKind, TargetRecord, TargetSpec, WINDOW, begin_target,
    end_target, install, outcome, post, snapshot, waiting,
};
use crate::host::Host;
use crate::shell::workbench::Workbench;

/// The headless window's size, in points: the C++'s headless window, 1360 x 860
/// (MainWindow::restoreSession's default for a headless run).
pub const HEADLESS_WIDTH: f32 = 1360.0;
pub const HEADLESS_HEIGHT: f32 = 860.0;

/// The time a frame takes on the harness's clock: a sixtieth of a second, egui's own guess.
const FRAME_SECONDS: f64 = 1.0 / 60.0;

/// The most frames a run waits for the window to come idle. An egui animation takes its
/// `animation_time` (a twelfth of a second, five frames); a dialog's first show takes a measuring
/// frame and a frame to place it; a hundred is far more than any of these.
const MOST_FRAMES: usize = 100;

/// A headless egui context the tests drive (see the module comment).
pub struct Harness {
    ctx: Context,
    screen: Rect,
    pixels_per_point: f32,
    time: f64,
    most_frames: usize,
    target: String,
    said_as: String,
    posted: Vec<ActId>,
    events: Vec<Event>,
    frames: u64,
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness {
    /// The headless window's screen, 1360 x 860 points, a pixel a point.
    pub fn new() -> Self {
        Self::with_screen(HEADLESS_WIDTH, HEADLESS_HEIGHT)
    }

    /// A screen of `width` by `height` points: the ScreenFit twins' is Qt's offscreen screen,
    /// 800 x 600.
    pub fn with_screen(width: f32, height: f32) -> Self {
        let ctx = Context::default();
        install(&ctx);
        Self {
            ctx,
            screen: Rect::from_min_size(Pos2::ZERO, egui::vec2(width, height)),
            pixels_per_point: 1.0,
            time: 0.0,
            most_frames: MOST_FRAMES,
            target: String::from(WINDOW),
            said_as: String::from(WINDOW),
            posted: Vec::new(),
            events: Vec::new(),
            frames: 0,
        }
    }

    /// The same harness at `pixels_per_point` device pixels a point: 1.25 for the C++'s reruns
    /// under QT_SCALE_FACTOR=1.25.
    pub fn with_pixels_per_point(mut self, pixels_per_point: f32) -> Self {
        self.pixels_per_point = pixels_per_point;
        self
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    /// The screen, in points.
    pub fn screen(&self) -> Rect {
        self.screen
    }

    pub fn pixels_per_point(&self) -> f32 {
        self.pixels_per_point
    }

    /// How many frames have run.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The target the acts after this go to, named as it is drawn (a dialog's object name).
    pub fn target(&mut self, name: &str) -> &mut Self {
        self.target = String::from(name);
        self.said_as = String::from(name);
        self
    }

    /// The target the acts after this go to, named as it is drawn and as the step named it
    /// (`--dialog surveyInverse` opened `surveyInverseDialog`).
    pub fn target_said_as(&mut self, name: &str, said_as: &str) -> &mut Self {
        self.target = String::from(name);
        self.said_as = String::from(said_as);
        self
    }

    fn post(&mut self, posted: Posted) -> &mut Self {
        let posted = posted.said_as(&self.said_as);
        if let Some(id) = post(&self.ctx, posted) {
            self.posted.push(id);
        }
        self
    }

    /// `--fill NAME=TEXT` to the target, for the next run.
    pub fn fill(&mut self, name: &str, text: &str) -> &mut Self {
        let posted = Posted::fill(&self.target, name, text);
        self.post(posted)
    }

    /// `--press NAME` to the target, for the next run.
    pub fn press(&mut self, name: &str) -> &mut Self {
        let posted = Posted::press(&self.target, name);
        self.post(posted)
    }

    /// The target's control `name` brought into view, for the next run.
    pub fn scroll_to(&mut self, name: &str) -> &mut Self {
        let posted = Posted::scroll_to(&self.target, name);
        self.post(posted)
    }

    /// The pointer moved to `at` (points), for the next frame.
    pub fn pointer_move(&mut self, at: Pos2) -> &mut Self {
        self.events.push(Event::PointerMoved(at));
        self
    }

    /// A mouse button pressed or released at `at`, with `modifiers` held, for the next frame.
    pub fn pointer_button(
        &mut self,
        at: Pos2,
        button: PointerButton,
        pressed: bool,
        modifiers: Modifiers,
    ) -> &mut Self {
        self.events.push(Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers,
        });
        self
    }

    /// The wheel turned at `at` by `delta` points, for the next frame.
    pub fn wheel(&mut self, at: Pos2, delta: Vec2) -> &mut Self {
        self.events.push(Event::PointerMoved(at));
        self.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        self
    }

    /// A key pressed and released with `modifiers` held, for the next frame.
    pub fn key(&mut self, key: egui::Key, modifiers: Modifiers) -> &mut Self {
        for pressed in [true, false] {
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
        self
    }

    /// Text typed, for the next frame.
    pub fn type_text(&mut self, text: &str) -> &mut Self {
        self.events.push(Event::Text(String::from(text)));
        self
    }

    /// One frame of `ui`, with the input given since the last.
    pub fn frame(&mut self, mut ui: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(self.screen),
            time: Some(self.time),
            predicted_dt: FRAME_SECONDS as f32,
            events: std::mem::take(&mut self.events),
            ..egui::RawInput::default()
        };
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.pixels_per_point);
        let output = self.ctx.run_ui(input, &mut ui);
        self.time += FRAME_SECONDS;
        self.frames += 1;
        output
    }

    /// Frames of `ui` until the window is idle: every act posted since the last run decided and
    /// none refused, no name drawn twice in a target, and no repaint asked. The first refusal - in
    /// the C++'s words - or the first name drawn twice fails the run.
    pub fn run(&mut self, mut ui: impl FnMut(&mut Ui)) -> Result<(), String> {
        for _ in 0..self.most_frames {
            // A frame that takes an act drew the controls before the one that took it with the
            // state from before the act (a chip group's other chips, a form's earlier fields),
            // where Qt shows an act's result everywhere at once. So a run never ends on such a
            // frame: one more frame draws what the act left, and the snapshot reads that.
            let had_acts = !self.posted.is_empty();
            let output = self.frame(&mut ui);
            if let Some(problem) = self.problem() {
                return Err(problem);
            }
            let repaint = output
                .viewport_output
                .get(&ViewportId::ROOT)
                .is_some_and(|viewport| viewport.repaint_delay == Duration::ZERO);
            if !had_acts && !repaint && self.posted.is_empty() && waiting(&self.ctx) == 0 {
                return Ok(());
            }
        }
        Err(format!(
            "the window did not settle in {} frames",
            self.most_frames
        ))
    }

    /// `run`, for a first show with nothing posted.
    pub fn show(&mut self, ui: impl FnMut(&mut Ui)) -> Result<(), String> {
        self.run(ui)
    }

    /// The first refusal of an act posted, or the first name drawn twice; the acts decided are
    /// forgotten.
    fn problem(&mut self) -> Option<String> {
        let mut refused = None;
        let mut still = Vec::new();
        for id in std::mem::take(&mut self.posted) {
            match outcome(&self.ctx, id) {
                ActOutcome::Waiting => still.push(id),
                ActOutcome::Done => {}
                ActOutcome::Refused(sentence) => {
                    refused.get_or_insert(sentence);
                }
            }
        }
        self.posted = still;
        refused.or_else(|| self.snapshot().duplicates.into_iter().next())
    }

    /// The last frame's records.
    pub fn snapshot(&self) -> Snapshot {
        snapshot(&self.ctx)
    }

    /// The control `name` of the target, else of any target (main.cpp's report looks in the
    /// target, then anywhere in the window).
    pub fn control(&self, name: &str) -> Option<ControlRecord> {
        let frame = self.snapshot();
        frame
            .control(&self.target, name)
            .or_else(|| frame.control_anywhere(name))
            .cloned()
    }

    /// The target record `name` from the last frame.
    pub fn target_record(&self, name: &str) -> Option<TargetRecord> {
        self.snapshot().target(name).cloned()
    }

    /// What `--report NAME` says of control `name` (ControlRecord::report), or main.cpp's
    /// refusal for a name that is none of what it reports.
    pub fn report(&self, name: &str) -> Result<String, String> {
        self.control(name)
            .and_then(|control| control.report())
            .ok_or_else(|| {
                format!(
                    "--report: there is no label, field, text, list, button, view, action or menu {name}"
                )
            })
    }
}

/// The stand-in for the bare QMainWindow the C++ tests install a workbench into: the workbench's
/// menus as a menu bar, its toolbars, its docks each a Dock target, and its own drawing, with its
/// actions triggered by name as `QAction::trigger` triggers them.
pub struct WorkbenchHost<W: Workbench> {
    pub workbench: W,
}

impl<W: Workbench> WorkbenchHost<W> {
    pub fn new(workbench: W) -> Self {
        Self { workbench }
    }

    pub fn menus(&self) -> Vec<Menu> {
        self.workbench.menus()
    }

    pub fn tool_bars(&self) -> Vec<ToolBar> {
        self.workbench.tool_bars()
    }

    /// The action `name` among the workbench's menus and toolbars.
    pub fn action(&self, name: &str) -> Option<Action> {
        let menus = self.menus();
        let in_menus = menus
            .iter()
            .flat_map(Menu::actions)
            .find(|action| action.name == name)
            .cloned();
        in_menus.or_else(|| {
            self.tool_bars()
                .iter()
                .flat_map(ToolBar::actions)
                .find(|action| action.name == name)
                .cloned()
        })
    }

    /// The action `name` triggered, as MainWindow::triggerAction does: NotFound for a name the
    /// workbench has no action of, InvalidState for a disabled one.
    pub fn trigger(&mut self, host: &mut dyn Host, name: &str) -> Status {
        let Some(action) = self.action(name) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no menu item or tool has that name",
                name,
            ));
        };
        if !action.enabled {
            return Err(make_error(
                ErrorCode::InvalidState,
                "that menu item is disabled",
                name,
            ));
        }
        if !self.workbench.trigger(host, name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "the workbench did not take that action",
                name,
            ));
        }
        Ok(())
    }

    /// Runs `line` if it is the workbench's.
    pub fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        self.workbench.run_line(host, line)
    }

    /// One frame of the window the workbench is installed in.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut chosen: Option<String> = None;
        let menus = self.menus();
        egui::Panel::top("workbenchHostMenus").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for menu in &menus {
                    menu_ui(ui, menu, &mut chosen);
                }
            });
        });
        let tool_bars = self.tool_bars();
        if !tool_bars.is_empty() {
            egui::Panel::top("workbenchHostToolBars").show(ui, |ui| {
                for tool_bar in &tool_bars {
                    let spec =
                        TargetSpec::new(&tool_bar.name, TargetKind::ToolBar, &tool_bar.title);
                    let shown = ui.horizontal(|ui| {
                        begin_target(ui, spec);
                        for item in &tool_bar.items {
                            match item {
                                ToolBarItem::Action(action) => {
                                    if action_item(ui, action).clicked() {
                                        chosen = Some(action.name.clone());
                                    }
                                }
                                ToolBarItem::Separator => {
                                    ui.separator();
                                }
                                ToolBarItem::Widget(_) => {}
                            }
                        }
                    });
                    end_target(ui, shown.response.rect);
                }
            });
        }
        let docks = self.workbench.docks();
        if !docks.is_empty() {
            egui::Panel::left("workbenchHostDocks").show(ui, |ui| {
                for dock in &docks {
                    let spec = TargetSpec::new(&dock.name, TargetKind::Dock, &dock.title);
                    let shown = ui.vertical(|ui| {
                        begin_target(ui, spec);
                        super::registry::begin_part(ui, Part::None);
                        ui.strong(&dock.title);
                        self.workbench.dock_ui(ui, host, &dock.name);
                    });
                    end_target(ui, shown.response.rect);
                }
            });
        }
        self.workbench.ui(ui, host);
        if let Some(name) = chosen {
            let _ = self.trigger(host, &name);
        }
    }
}

/// A menu drawn as a menu button with its items: a click on an action is the action chosen.
fn menu_ui(ui: &mut Ui, menu: &Menu, chosen: &mut Option<String>) {
    ui.menu_button(menu.title.replace('&', ""), |ui| {
        for item in &menu.items {
            match item {
                MenuItem::Action(action) => {
                    if action_item(ui, action).clicked() {
                        *chosen = Some(action.name.clone());
                    }
                }
                MenuItem::Section(title) => {
                    ui.weak(title.as_str());
                }
                MenuItem::Separator => {
                    ui.separator();
                }
                MenuItem::Menu(menu) => menu_ui(ui, menu, chosen),
            }
        }
    });
}
