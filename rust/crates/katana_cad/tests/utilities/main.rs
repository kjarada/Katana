//! The cad layer's views, utilities and style rows (wave H6), one module per
//! C++ test file:
//!
//! ```text
//! utility_drawing      tests/cad/utilities/test_utility_drawing.cpp
//! style_manager_rows   tests/cad/customisation/test_style_manager_rows.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The interpreter-driven files of the
//! same modules (test_view_verbs, test_property_outline, test_utility_data,
//! test_utility_geometry, test_utility_verbs) are wave J's.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod style_manager_rows;
mod utility_drawing;

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

/// Owned strings, for comparing with a Vec<String>.
pub fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}
