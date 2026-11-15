//! The cad layer's code table, customisation host and report, and parcels (wave H2), one module
//! per C++ test file:
//!
//! ```text
//! code_table             tests/cad/customisation/test_code_table.cpp
//! code_edit              tests/cad/customisation/test_code_edit.cpp
//! colour_lookup          tests/cad/customisation/test_colour_lookup.cpp
//! builtin_census         tests/cad/customisation/test_builtin_census.cpp
//! builtin_library        tests/cad/customisation/test_builtin_library.cpp
//! builtin_renames        tests/cad/customisation/test_builtin_renames.cpp
//! customisation_host     tests/cad/customisation/test_customisation_host.cpp
//! customisation_report   tests/cad/customisation/test_customisation_report.cpp
//! parcel                 tests/cad/test_parcel.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as tools/check_rust_parity.py
//! expects. The cases that set KATANA_BUILTIN_CUSTOMISATION, or change the working folder, are in
//! the target tests/customisation_env instead, which holds one lock around each: the environment
//! and the working folder are the process's, and this binary runs its tests on parallel threads.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/customisation_support.rs"]
mod customisation_support;

mod builtin_census;
mod builtin_library;
mod builtin_renames;
mod code_edit;
mod code_table;
mod colour_lookup;
mod customisation_host;
mod customisation_report;
mod parcel;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}
