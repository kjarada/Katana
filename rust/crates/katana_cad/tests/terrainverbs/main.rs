//! The terrain and road-object verbs (wave H5), one module per C++ test file:
//!
//! ```text
//! drawing_shapes   tests/cad/terrain/test_drawing_shapes.cpp
//! ```
//!
//! The verb families this agent ported (surface_display_verbs,
//! surface_edit_verbs, road_object_scope, road_object_verbs, bim_view) are
//! driven by the interpreter in their C++ tests, which wave J twins.
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/circle_spline.rs"]
pub mod circle_spline;

mod drawing_shapes;

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

/// ASSERT_TRUE(result.ok()) << result.error().describe(), and the value.
#[track_caller]
pub fn must<T>(result: katana_core::Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// A step the test expects to succeed, with the error's description when it
/// does not.
#[track_caller]
pub fn ok(status: katana_core::Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}
