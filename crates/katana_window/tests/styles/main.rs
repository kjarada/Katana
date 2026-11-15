//! The styles agent's cases (CU1, step 7 wave 2): the twins of tests/qt_widgets/customisation's
//! test_shared_name_picker, test_shared_style_preview, test_drawing_summary_dialog,
//! test_global_modify_dialog, test_hatch_patterns_tab, test_layer_manager,
//! test_layer_manager_modeless and test_style_manager - one module a C++ file, the suites as
//! modules inside.
//!
//! Each dialog is drawn in the headless harness (drive::Harness) with a recording host: the
//! widgets the C++ found by `findChild` are filled, pressed and read by the same object names, and
//! what the C++ called on the dialog is called on the Rust dialog. A C++ `processEvents()` is a
//! run of frames (Bench::draw). The pickers and the preview are plain values and need no frame.

#![allow(
    clippy::float_cmp,
    reason = "Values are compared exactly where the C++ compared them exactly."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

pub mod support;

mod drawing_summary_dialog;
mod global_modify_dialog;
mod hatch_patterns_tab;
mod layer_manager;
mod layer_manager_modeless;
mod shared_name_picker;
mod shared_style_preview;
mod style_manager;
