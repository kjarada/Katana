//! The annotation dialogs' cases (AN, step 7 wave 1): the twins of tests/qt_widgets/annotation's
//! test_annotation_ui (AnnotationManagers but its scale-box case, which installs the annotation
//! workbench and is V3's), test_label_edit_dialog, test_leader_manager (but the case of the
//! Annotate menu, which installs the workbench), test_leader_manager_lines,
//! test_leader_manager_positions, test_leader_manager_presets and test_text_edit_dialog (but the
//! case of the Annotate menu) - one module a C++ file, the suites as modules inside.
//!
//! Each dialog is drawn in the headless harness (drive::Harness) with a recording host whose runner
//! is the interpreter on the host's drawing, as the C++ tests' runner was: the widgets the C++
//! found by `findChild` are filled, pressed and read by the same object names, and what the C++
//! called on the dialog (apply, addStyle, formChange ...) is called on the Rust dialog. A C++
//! `processEvents()` is a run of frames (Bench::draw).

#![allow(
    clippy::float_cmp,
    reason = "Values are compared exactly where the C++ compared them exactly."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::excessive_precision,
    reason = "Coordinates are written as the C++ tests wrote them."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

pub mod support;

mod annotation_ui;
mod label_edit_dialog;
mod leader_manager;
mod leader_manager_lines;
mod leader_manager_positions;
mod leader_manager_presets;
mod text_edit_dialog;
