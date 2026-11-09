//! The customisation verbs and the families they meet, driven through the command interpreter
//! (wave J, agent J3), one module per C++ test file:
//!
//! ```text
//! customisation_verbs   tests/cad/customisation/test_customisation_verbs.cpp
//! definition_edit       tests/cad/customisation/test_definition_edit.cpp
//! document_status       tests/cad/customisation/test_document_status.cpp
//! global_modify         tests/cad/customisation/test_global_modify.cpp
//! hatch_set_verb        tests/cad/customisation/test_hatch_set_verb.cpp
//! linework_verbs        tests/cad/customisation/test_linework_verbs.cpp
//! survey_code_verbs     tests/cad/customisation/test_survey_code_verbs.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as tools/check_rust_parity.py
//! expects. The one case that changes the working folder is in the target
//! tests/customisation_verbs_env instead, which holds one lock around it: the working folder is
//! the process's, and this binary runs its tests on parallel threads.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/customisation_support.rs"]
mod customisation_support;
mod verbs_support;

mod customisation_verbs;
mod definition_edit;
mod document_status;
mod global_modify;
mod hatch_set_verb;
mod linework_verbs;
mod survey_code_verbs;

use katana_core::Status;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// ASSERT_TRUE(status.ok()), saying why when it is not.
#[track_caller]
pub fn must(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}
