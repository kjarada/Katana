//! The survey code tools' cases (CU2, step 7 wave 2): the twins of tests/qt_widgets/customisation's
//! test_code_manager (SurveyCodeManager, all 47), test_definition_editor (DefinitionEditor) and
//! test_symbol_library (SymbolLibrary) - one module a C++ file, the suite as a module inside.
//! Wave 4's stragglers added the cases that waited for the style preview, the legend and sheet
//! painters and the customisation workbench, but TheWindowIsNotClosedOverAFormThatIsNotSaved,
//! which is the main window's.
//!
//! Each dialog is drawn in the headless harness (drive::Harness) with a recording host whose
//! runner is the interpreter on the host's drawing, as the C++ tests' runner was: the widgets the
//! C++ found by `findChild` are filled, pressed and read by the same object names, and what the
//! C++ called on the dialog (apply, importCodes, save ...) is called on the Rust dialog. A C++
//! `processEvents()` is a run of frames (Bench::draw). The customisation's context is the host and
//! customisation::customisation_context.

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
