//! The cad layer's plotting upper half (wave H4), one module per C++ test
//! file:
//!
//! ```text
//! arrange           tests/cad/plotting/test_arrange.cpp
//! key_plan          tests/cad/plotting/test_key_plan.cpp
//! arrange_commands  tests/cad/plotting/test_arrange_commands.cpp
//! legend            tests/cad/plotting/test_legend.cpp
//! preflight         tests/cad/plotting/test_preflight.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The sheet verbs' own cases drive the
//! interpreter and are wave J's.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

use katana_geometry::Box2;

mod arrange;
mod arrange_commands;
mod key_plan;
mod legend;
mod preflight;

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

/// The C++ tests' `expectBox`: each corner within `tolerance` of the one
/// worked by hand.
#[track_caller]
pub fn expect_box(rect: &Box2, x0: f64, y0: f64, x1: f64, y1: f64, tolerance: f64) {
    near(rect.min.x, x0, tolerance);
    near(rect.min.y, y0, tolerance);
    near(rect.max.x, x1, tolerance);
    near(rect.max.y, y1, tolerance);
}

/// A result the case cannot go on without: its value, or the error described.
#[track_caller]
pub fn ok<T>(result: katana_core::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// The error a refused call gives.
#[track_caller]
pub fn refused<T: std::fmt::Debug>(result: katana_core::Result<T>) -> katana_core::Error {
    result.expect_err("the call was expected to be refused")
}
