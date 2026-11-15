//! The geo suite's cases of the terrain layer's own code (step 5, agent K2),
//! one module per C++ test file:
//!
//! ```text
//! surface_store      tests/geo/test_surface_store.cpp (SurfaceStore)
//! viewshed_and_los   tests/geo/test_viewshed_and_los.cpp (the LineOfSight suite)
//! ```
//!
//! Twins of code step 2 ported: the C++ tests sit in tests/geo because the
//! geo verbs are what use the store and the sight line, and their twins sit
//! here, beside the code they test (the step-5 plan, section 8, choice 8).
//! The rest of test_viewshed_and_los.cpp drives RASTER VIEWSHED and LOS
//! through the app layer (step 6), or binds GDAL's arguments (wave L).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod surface_store;
mod viewshed_and_los;

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
