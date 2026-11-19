//! The program `katana`: the desktop window, and its headless runs (a screenshot, a plot, the
//! sheets' JSON, a script batch, the update's health check) - the port of src/katana_qt/main.cpp,
//! whose switches and steps are the program driver's (katana_window::drive::run), run here over
//! the main window (katana_window::MainWindow).
//!
//! This is the one file that names eframe (the step-7 brief, "Never open a window"; the
//! foundation's test fails when another does): the desktop runner (drive::desktop) is what
//! eframe's App delegates to, and the few lines below are that App and the call that runs it.
//!
//! The window's 3D views take the software rasteriser on the desktop too, for now: the window is
//! made before eframe's wgpu device exists (drive::run makes it as the switches are read), and
//! handing it that device afterwards is left (docs/rust.md, "Not done").
//!
//! On Windows it is a GUI program, so starting it opens no console window beside the main one
//! (the owner, 2026-11-19: "when i open katana i get an additional cmd"); started from a console,
//! it borrows that one for its headless reports (`katana_app::console::attach_parent_console`).
//! Rejected: FreeConsole at start-up, which still flashes the console open first.

#![cfg_attr(windows, windows_subsystem = "windows")]

use katana_app::console::{Terminal, attach_parent_console};
use katana_window::MainWindow;
use katana_window::drive::desktop::{Desktop, TITLE};
use katana_window::drive::run::{program_arguments, run};

/// The window as eframe draws it each frame.
struct Shown<'a, 'w>(&'a mut Desktop<'w, MainWindow>);

impl eframe::App for Shown<'_, '_> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn main() {
    attach_parent_console();
    let arguments = program_arguments();
    let mut console = Terminal;
    let code = run(
        &arguments,
        &mut console,
        MainWindow::new,
        |desktop: &mut Desktop<'_, MainWindow>| {
            let options = eframe::NativeOptions {
                viewport: desktop.viewport(),
                ..Default::default()
            };
            match eframe::run_native(
                TITLE,
                options,
                Box::new(|_| Ok(Box::new(Shown(desktop)))),
            ) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("the window could not be opened: {error}");
                    1
                }
            }
        },
    );
    std::process::exit(code);
}
