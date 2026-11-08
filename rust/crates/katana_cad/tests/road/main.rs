//! The cad layer's road model (wave G4), one module per C++ test file:
//!
//! ```text
//! corridor           tests/cad/test_corridor.cpp
//! road_template      tests/cad/road/test_road_template.cpp
//! road_section       tests/cad/road/test_road_section.cpp
//! corridor_objects   tests/cad/road/test_corridor_objects.cpp
//! ```
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

#[path = "../support/road_fixtures.rs"]
pub mod road_fixtures;

mod corridor;
mod corridor_objects;
mod road_section;
mod road_template;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
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
