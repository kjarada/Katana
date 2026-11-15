//! The workspace's cases (WS, step 7 wave 3): the twins of tests/qt_widgets' test_view_chrome,
//! test_view_focus, test_view_layers_popup, test_view_links, test_view_scale and
//! test_view_selection, and the cases of other files that drive the workspace (test_tool_menus,
//! test_plan_context_menu, test_render_view, test_section_recut) - one module a C++ file or part,
//! the suites as modules inside.
//!
//! The workspace is built as the C++ tests build it: chromed, in a window of the size they give,
//! its lines run through its own line runner on the drawing's interpreter (support.rs).

#![allow(
    clippy::float_cmp,
    reason = "Positions are compared exactly where the C++ compared them exactly."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod support;

mod view_chrome;
mod view_focus;
mod view_layers_popup;
mod view_links;
mod view_scale;
mod view_selection;
mod workspace_tools;
mod workspace_views;
