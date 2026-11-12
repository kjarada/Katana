//! The plan view (VP, step 7 wave 2): the twins of tests/qt_widgets' test_plan_view,
//! drawing/test_plan_view_grips, drawing/test_plan_view_tracking, drawing/test_vertex_panel,
//! tools/test_plan_view_tools, tools/test_tool_host, tools/test_vertex_tool_hover, the PlanView
//! cases of test_plan_painter and the PlanContextMenu and PlanViewShortcutMenu cases of
//! test_plan_context_menu - one module a C++ file, the suites as modules inside.
//!
//! The view is driven alone, as the C++ tests drive a ViewportWidget with no window: its events
//! are its own methods at a pixel of the view (support.rs), and what it paints is read from its
//! picture. `view_ui` is not a twin: it draws the view through egui in the harness, to hold that
//! the egui adaptor sends the same events to the same pixels.

#![allow(
    clippy::float_cmp,
    reason = "Coordinates and scales are compared as the C++ compared them, exactly where it did."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod support;

mod plan_context_menu;
mod plan_painter;
mod plan_view;
mod plan_view_grips;
mod plan_view_tools;
mod plan_view_tracking;
mod tool_host;
mod vertex_panel;
mod vertex_tool_hover;
mod view_ui;
