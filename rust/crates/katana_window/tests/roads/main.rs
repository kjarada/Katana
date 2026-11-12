//! Roads and alignments (BM, step 7 wave 1): the twins of tests/qt_widgets' bim/test_road_dialogs
//! (the RoadTemplateManager, CorridorDialog and CorridorQuantities suites; RoadWorkbench is V3's),
//! bim/test_road_object_dialogs and test_alignment_manager - one module a C++ file, the suites as
//! modules inside - driven by the C++ object names through drive::Harness.
//!
//! The C++ tests' executor ran each line through the real CommandInterpreter on the same drawing;
//! here that is window_support's `interpreter_host`, its `lines` the C++'s `executor.lines`.

#![allow(
    clippy::float_cmp,
    reason = "Values the C++ compared exactly are compared exactly, as the C++ did."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod alignment_manager;
mod driver;
mod road_dialogs;
mod road_object_dialogs;
