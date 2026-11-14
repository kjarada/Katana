//! GA's cases (step 7 wave 2): the twins of tests/qt_widgets' geo/test_buffer_dialog,
//! geo/test_check_dialogs, geo/test_dataset_info_dialog, geo/test_export_dialog,
//! geo/test_formats_dialog, geo/test_hull_and_clip_dialogs, geo/test_import_dialogs,
//! geo/test_overlay_dialog, geo/test_sql_dialog and test_gis_online - one module a C++ file, the
//! suites as modules inside.
//!
//! Each dialog is drawn by the drive harness and driven by the C++ object names, as a person or
//! the headless driver drives it. The window's side is a recording host where the C++ test's
//! runner recorded and answered, and a host that runs each line by the geoprocessing workbench on
//! its own drawing (`support::WindowHost`) where the C++ test's `Window` did. Nothing here
//! reaches a network: the online dialog's context is the test's own.
//!
//! What the C++ did to a widget that is not a control a driver fills - a list's check box, a
//! tree's item - is done by the dialog's own method that the C++ call stood for. Each such place
//! says so.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::float_cmp,
    reason = "Values are compared exactly where the C++ compared them exactly."
)]

#![allow(
    clippy::field_reassign_with_default,
    reason = "A form is filled a field at a time, as the C++ tests fill theirs."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

pub mod support;

mod buffer_dialog;
mod check_dialogs;
mod dataset_info_dialog;
mod export_dialog;
mod formats_dialog;
mod gis_online;
mod hull_and_clip_dialogs;
mod import_dialogs;
mod overlay_dialog;
mod sql_dialog;
