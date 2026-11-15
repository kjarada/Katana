//! D's cases (step 7 wave 1): the twins of tests/qt_widgets' test_theme (Theme's two cases about
//! the title bar's buttons) and tools/test_flyout_button (TitleBarButtons' case about every
//! button's icon) - one module a C++ file, the suites as modules inside - and D's own: the step
//! engine against a small test window, the program's runs, the soft renderer against hand-made
//! meshes, and the dock chrome over egui_dock. Nothing here opens a window: every frame is drawn
//! by the headless runner.

#![allow(
    clippy::float_cmp,
    reason = "Sizes and scales are compared exactly where they were worked by hand."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod test_window;

mod chrome;
mod flyout_button;
mod program;
mod soft_rendering;
mod step_engine;
mod theme;
