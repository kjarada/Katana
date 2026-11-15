//! The desktop runner: the window a person uses, drawn by eframe (winit and egui-wgpu) - the
//! program's last branch in main.cpp, `application.exec()`, and what main() does after it.
//!
//! WHO NAMES EFRAME. Only the program, src/bin/katana/main.rs (the step-7 brief, "Never open a
//! window"; the plan's section 4.7, and the foundation's test that fails when another file names
//! the crate's paths - so not even this comment writes one). So the runner is what eframe's `App`
//! delegates to, and the program's few lines are the App and the call that runs it, given to
//! `run::run` as its `desktop` closure:
//!
//! ```text
//! struct Shown<'a, 'w>(&'a mut Desktop<'w, MainWindow>);
//! impl App for Shown<'_, '_> {                    // eframe's App, its ui method:
//!     fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) { self.0.ui(ui) }
//! }
//! |desktop| {
//!     let options = NativeOptions { viewport: desktop.viewport(), ..Default::default() };
//!     match run_native(TITLE, options, Box::new(|_| Ok(Box::new(Shown(desktop))))) {
//!         Ok(()) => 0,
//!         Err(error) => { /* said on stderr */ 1 }
//!     }
//! }
//! ```
//!
//! eframe's app creator may borrow (its `AppCreator<'app>`), so the window stays the program's and
//! is still there after the event loop has ended - for the update helper, which is started then
//! (`start_pending_update`), as main.cpp starts it after `exec()` returns.
//!
//! Nothing here runs in a test or a headless run: no test may make a winit window. It is written,
//! and driven by no test before step 8 (the plan's section 7, risk 9).

use egui::{IconData, Ui, ViewportBuilder};
use katana_app::abandon_apply;
use katana_app::console::Console;
use katana_update::launch_apply;

use super::harness::{HEADLESS_HEIGHT, HEADLESS_WIDTH};
use super::program::DrivenWindow;
use super::registry::install;

/// The application's name: eframe's app name and the window's first title, as
/// QApplication::setApplicationName named it.
pub const TITLE: &str = "Katana";

/// The window, as eframe's App draws it each frame.
pub struct Desktop<'a, W: ?Sized> {
    window: &'a mut W,
    installed: bool,
}

impl<'a, W: DrivenWindow + ?Sized> Desktop<'a, W> {
    pub fn new(window: &'a mut W) -> Self {
        Self {
            window,
            installed: false,
        }
    }

    pub fn window(&mut self) -> &mut W {
        self.window
    }

    /// The native window's first look (the plan's section 4.3): its title, the application's
    /// icon, and 1360 x 860 points - the window's own restored place, when it has one, it sets
    /// itself on its first frame (MainWindow::restoreSession).
    pub fn viewport(&self) -> ViewportBuilder {
        let icon = self.window.application_icon();
        let builder = ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([HEADLESS_WIDTH, HEADLESS_HEIGHT]);
        if icon.is_null() {
            return builder;
        }
        builder.with_icon(IconData {
            rgba: icon.to_straight_rgba8(),
            width: icon.width(),
            height: icon.height(),
        })
    }

    /// One frame of the window, inside eframe's `App::ui`. The registry is installed on eframe's
    /// context with the first, so the window's named controls are named to an assistive tool there
    /// too (each one's AccessKit author id).
    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.installed {
            install(ui.ctx());
            self.installed = true;
        }
        self.window.ui(ui);
    }
}

/// What main.cpp does once the event loop has ended: when the window was closed BY an update
/// (UPDATE APPLY CONFIRM prepared the helper, and the close went through), the helper that swaps
/// the files is started, detached, and waits for this process to go. One that could not be started
/// is said on stderr always, its copy listed for the next start's sweep, and said in a box as well
/// only for a window nobody drove from its command line (`step_kinds`, the switches of the steps
/// the run was given; update_dialog's updateFailureIsBoxed). The run's exit code: the event loop's
/// `code`, or 1 for an update asked for and not started when that was 0.
pub fn start_pending_update(
    window: &mut dyn DrivenWindow,
    step_kinds: &[String],
    code: i32,
    console: &mut dyn Console,
) -> i32 {
    let Some(request) = window.take_pending_update() else {
        return code;
    };
    let Err(error) = launch_apply(&request) else {
        return code;
    };
    // The window is gone and nothing will happen. On stderr ALWAYS: that is where a run started by
    // a script, a test or an agent is read, and a failure is never only a box.
    let why = error.describe();
    console.err(&format!("the update could not be started: {why}\n"));
    // The helper's copy UPDATE APPLY CONFIRM made for this request is nobody's now: listed for the
    // sweep the next start makes, as katana_cli lists the one it could not start.
    let _ = abandon_apply(&request);
    if window.update_failure_is_boxed(step_kinds) {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Katana Update")
            .set_description(format!(
                "The update could not be started, so nothing was changed.\n\n{why}\n\nIt is \
                 still downloaded: start Katana and choose Help > Check for Updates."
            ))
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    // An update that was asked for and not started is a failed run, as it is katana_cli's: a
    // script reads the exit code. A window's own exit code, when it had one, stays.
    if code != 0 { code } else { 1 }
}
