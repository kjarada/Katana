//! The tool bars' cases (I, step 7 wave 1): the twins of tests/qt_widgets' test_theme (its Icons
//! suite; Theme's cases are C's and D's), tools/test_flyout_button (ToolFamilyButtons,
//! SplitButtons, ToolBarSigns and ToolBarOverflow; TitleBarButtons is D's and WS's) and
//! tools/test_tool_menus (the cases of fillToolMenus and its words; those that build the view
//! workspace are WS's) - one module a C++ file, the suites as modules inside.
//!
//! The C++ reran ToolFamilyButtons, SplitButtons, ToolBarSigns and ToolBarOverflow at 125 %
//! (qt_toolbar_signs_at_125_percent.*, QT_SCALE_FACTOR=1.25): each twin runs its body at a ratio
//! of 1.0 and again at 1.25 (`RATIOS`), as the plan's section 3.3 says.
//!
//! Painted twins paint through the canvas with test_canvas's raster, a stand-in for the window's
//! raster canvas, which is written beside this target in the same wave.

#![allow(
    clippy::float_cmp,
    reason = "Device-pixel ratios are compared exactly where the C++ compared them."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod test_canvas;

mod flyout_button;
mod theme;
mod tool_menus;

/// The screen's ratios the C++ ran these suites at: its own run, and the 125 % rerun.
pub const RATIOS: [f64; 2] = [1.0, 1.25];
