//! UP's settings cases (step 7): the twins of tests/qt_widgets/customisation/
//! test_settings_dialog.cpp - File > Settings (katana_window::customisation::settings_dialog) and
//! the one list of the linework controls' words (customisation::linework_labels). The C++ file
//! holds one suite, SettingsDialog, split here into two files, each with the suite a module inside
//! it: the page and its lines, and the built-in library with the verb itself and the kept file.
//! The Updates page's own cases are the SettingsUpdates suite of the update target (tests/update).
//!
//! The dialog does none of the work, so most cases give it a RECORDING runner - the host's - that
//! notes each line and, unless a case says what a line does, changes nothing and answers that it
//! was carried out. Where what follows a line depends on what the verb did to the Document, the
//! case's runner does that one thing to the host's drawing, by the Document's own API. The cases
//! that give it the verb itself run the host's own interpreter over the same drawing, as the
//! window's executor runs a line.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

#[path = "../support/fixture_customisation.rs"]
pub mod fixture_customisation;

pub mod support;

mod settings_dialog;
mod settings_dialog_library;
