//! The survey code tools' cases (CU2, step 7 wave 2): the twins of tests/qt_widgets/customisation's
//! test_code_manager (SurveyCodeManager, all 47), test_definition_editor (DefinitionEditor, but
//! the case of the preview panes' pictures, which waits for the style preview, and the five that
//! install the customisation workbench) and test_symbol_library (SymbolLibrary, but the three
//! that paint through the style preview, the legend painter and the sheet painter) - one module a
//! C++ file, the suite as a module inside.
//!
//! Each dialog is drawn in the headless harness (drive::Harness) with a recording host whose
//! runner is the interpreter on the host's drawing, as the C++ tests' runner was: the widgets the
//! C++ found by `findChild` are filled, pressed and read by the same object names, and what the
//! C++ called on the dialog (apply, importCodes, save ...) is called on the Rust dialog. A C++
//! `processEvents()` is a run of frames (Bench::draw). The customisation's context is the host and
//! the seams of customisation::codes_seams.

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

#[path = "../support/fixture_customisation.rs"]
pub mod fixture_customisation;

pub mod support;

mod code_manager;
mod definition_editor;
mod symbol_library;
