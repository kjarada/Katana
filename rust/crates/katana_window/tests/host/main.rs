//! The host's machinery and the small panels (H, step 7 wave 1): the twins of tests/qt_widgets'
//! test_jobs, test_script_runner, test_import_placement (but VectorImportDialog, GA's),
//! test_property_panel, test_command_reference, test_plan_context_menu's SelectById and
//! SelectByIdDialog, and geo/test_geo_workbench and geo/test_gis_verbs_window - one module a C++
//! file, the suites as modules inside.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::float_cmp,
    reason = "Values the C++ compared exactly (a quarter stored as 250 thousandths, a clamp to 1) \
              are compared exactly."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod hosts;

mod command_reference;
mod geo_workbench;
mod gis_verbs_window;
mod import_placement;
mod jobs;
mod plan_context_menu;
mod property_panel;
mod script_runner;
