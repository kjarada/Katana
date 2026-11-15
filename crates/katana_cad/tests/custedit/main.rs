//! The cad layer's customisation edits, survey tools and drawing pieces (wave
//! G5), one module per C++ test file:
//!
//! ```text
//! customisation_merge   tests/cad/customisation/test_customisation_merge.cpp
//! customisation_state   tests/cad/customisation/test_customisation_state.cpp
//! definition_users      tests/cad/customisation/test_definition_users.cpp
//! purge                 tests/cad/customisation/test_purge.cpp
//! associative           tests/cad/annotation/test_associative.cpp
//! vertex_table          tests/cad/drawing/test_vertex_table.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod associative;
mod customisation_merge;
mod customisation_state;
mod definition_users;
mod purge;
mod vertex_table;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four units in the last place, as gtest compares.
#[track_caller]
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}
