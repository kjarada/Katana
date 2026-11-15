//! UP's update cases (step 7): the twins of tests/qt_widgets/test_update_dialog.cpp - Help > Check
//! for Updates (katana_window::update_dialog), the decisions beside it, and the Updates page of
//! Settings (katana_window::customisation::settings_dialog) - one file a C++ suite, the suite a
//! module inside it.
//!
//! Neither dialog does any work: a button builds an UPDATE line and hands it to the runner it was
//! given, or calls one callback. So every case gives a RECORDING runner and callbacks that count,
//! and asserts on exactly what was handed over. Each dialog is drawn by the drive harness and
//! driven by the C++ object names; where the C++ read a widget a person could not reach - a button
//! its state hides - the dialog's own answer for it is read instead, and the support module says
//! where.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

pub mod support;

mod settings_updates;
mod update_decisions;
mod update_dialog;
mod update_words;
