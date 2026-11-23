//! SH2's cases (step 7 wave 3): the sheet editor - the twins of tests/qt_widgets'
//! test_sheet_editor and, under plotting/, test_sheet_editor_editing, test_plot_dialog,
//! test_sheet_arrange, test_sheet_checks, test_sheet_set_menu, test_sheet_view_options,
//! test_sheet_register_editor, test_section_editor, test_generate_dialog, test_plan_grid_editor and
//! test_legend_properties' scope case (its summary case is SH1's, in tests/sheets) - one module a
//! C++ file, the suites as modules inside.
//!
//! The C++ tests sent mouse and key events to the canvas widget (QApplication::sendEvent, which
//! calls its handlers at once); the twins call the editor's event methods. A control the C++ found
//! by its object name is driven by that name through the harness, the editor drawn in its target
//! `sheetEditor` and each dialog in its own. Where the C++ test changed the drawing directly, the
//! twin does the same on the host's drawing, and the editor follows it at its next look.
//!
//! `kept_canvas` is not a twin: it holds the canvas's kept picture, which the Rust editor has and
//! the C++ had no need of (a QWidget repaints only what is asked).

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

mod support;

mod generate_dialog;
mod kept_canvas;
mod legend_properties;
mod plan_grid_editor;
mod plot_dialog;
mod section_editor;
mod sheet_arrange;
mod sheet_checks;
mod sheet_editor;
mod sheet_editor_editing;
mod sheet_register_editor;
mod sheet_set_menu;
mod sheet_view_options;
