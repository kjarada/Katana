//! The foundation's cases (F0, step 7 wave 0): the twins of tests/qt_widgets' test_screen_fit,
//! customisation/test_scope_filter_widget, test_shared_document_watcher, test_shared_filter_bar
//! and test_shared_row_table_model - one module a C++ file, the suites as modules inside - and the
//! foundation's own: the named controls, a test implementation of each interface later agents
//! build on, and the check that no file but the program names eframe.
//!
//! ProjectFolder, which changes the working directory, is in tests/foundation_process.

#![allow(
    clippy::float_cmp,
    reason = "Sizes egui laid out are compared as the C++ compared Qt's, exactly where it did."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/fixture_customisation.rs"]
pub mod fixture_customisation;
#[path = "../support/window_support.rs"]
pub mod window_support;

mod interfaces;
mod named_controls;
mod scope_filter_widget;
mod screen_fit;
mod shared_document_watcher;
mod shared_filter_bar;
mod shared_row_table_model;
mod window_display;
