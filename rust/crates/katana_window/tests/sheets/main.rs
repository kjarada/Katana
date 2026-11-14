//! SH1's cases (step 7 wave 2): the sheet painter and what plots - the twins of tests/qt_widgets'
//! test_sheet_painter and, under plotting/, test_sheet_sections, test_legend_painter,
//! test_legend_properties (its summary case), test_plan_grid_painter, test_plot_drawing_dialog,
//! test_plot_output (its PlotOutput suite; TiffWriter is C's), test_plot_style,
//! test_sheet_tables_painter and test_view_image_export (its Snapshot and ViewImageDialog suites)
//! - one module a C++ file, the suites as modules inside.
//!
//! Left for the wave that brings what they stand on: test_plan_grid_editor's three cases and
//! test_legend_properties' scope case (the sheet editor, SH2), and test_view_image_export's
//! ViewImage suite (the plan view's render to image, VP).
//!
//! A sheet is painted into a canvas Image at 4 px a millimetre and read back pixel by pixel, as the
//! C++ tests read their QImages. Ink is compared against a baseline painted in the same test,
//! never as an absolute number of pixels of text: fonts differ between machines (CLAUDE.md,
//! "Testing"). Each C++ test's own method is kept.

#![allow(
    clippy::float_cmp,
    reason = "Scales and sizes are compared exactly where the C++ compared them exactly."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept where a test rejects NaN with it."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod support;

mod legend_painter;
mod legend_properties;
mod plan_grid_painter;
mod plot_drawing_dialog;
mod plot_output;
mod plot_style;
mod sheet_painter;
mod sheet_sections;
mod sheet_tables_painter;
mod view_image_export;
