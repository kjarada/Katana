//! The survey dialogs' cases (SV, step 7 wave 1): the twins of tests/qt_widgets/survey's
//! test_parcel_dialog, test_reduction_options_widget, test_survey_import_wizard and
//! test_survey_jobs_dialog - one module a C++ file, the suites as modules inside.
//!
//! Each dialog is driven as the C++ tests drove its widgets, by the C++ object names: a fill or a
//! press posted to the dialog's target and the frames run until the window is idle
//! (drive::Harness), one act a run, as each C++ statement was one change; what a control shows is
//! read from the frame's records (Harness::report, Harness::control). The runner the C++ tests
//! handed a dialog is a RecordingHost: answering by its interpreter on its drawing where the C++
//! ran the real interpreter (window_support's interpreter_host).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::float_cmp,
    reason = "Values are compared exactly where the C++ compared them with EXPECT_EQ."
)]

#[path = "../support/fixture_customisation.rs"]
pub mod fixture_customisation;
#[path = "../support/window_support.rs"]
pub mod window_support;

mod parcel_dialog;
mod reduction_options_widget;
mod survey_import_wizard;
mod survey_jobs_dialog;

use std::path::{Path, PathBuf};

use katana_core::path_text::path_to_utf8;
use katana_window::RecordingHost;

use crate::window_support::checkout;

/// EXPECT_DOUBLE_EQ: within 4 units in the last place, as gtest compares.
pub fn double_eq(actual: f64, expected: f64) -> bool {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4
}

/// tests/surveyio/data/<relative>: the C++'s KATANA_SURVEYIO_DATA.
pub fn surveyio_data(relative: &str) -> PathBuf {
    checkout()
        .join("tests")
        .join("surveyio")
        .join("data")
        .join(relative)
}

/// tests/qt_widgets/survey/data/<name>: the C++'s KATANA_SURVEY_UI_DATA.
pub fn survey_ui_data(name: &str) -> PathBuf {
    checkout()
        .join("tests")
        .join("qt_widgets")
        .join("survey")
        .join("data")
        .join(name)
}

/// A path as a field takes it.
pub fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

/// Whether a line the host logged holds `pattern`.
pub fn logged(host: &RecordingHost, pattern: &str) -> bool {
    host.logged().iter().any(|(text, _)| text.contains(pattern))
}

/// The cells of a row as a control records it (its cells joined by " | ").
pub fn cells(row: &str) -> Vec<String> {
    row.split(" | ").map(String::from).collect()
}
