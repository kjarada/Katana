//! The cad layer's leaf libraries (wave F2), one module per C++ test file:
//!
//! ```text
//! dashing               tests/cad/test_dashing.cpp
//! style_drawing         tests/cad/test_style_drawing.cpp
//! symbols               tests/cad/test_symbols.cpp
//! hatching              tests/cad/test_hatching.cpp
//! grading               tests/cad/test_grading.cpp
//! plot                  tests/cad/test_plot.cpp
//! viewport_layout       tests/cad/test_viewport_layout.cpp
//! text_layout           tests/cad/annotation/test_text_layout.cpp
//! customisation_part    tests/cad/customisation/test_customisation_part.cpp
//! road_object_mesh      tests/cad/road/test_road_object_mesh.cpp
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

mod customisation_part;
mod dashing;
mod grading;
mod hatching;
mod plot;
mod road_object_mesh;
mod style_drawing;
mod symbols;
mod text_layout;
mod viewport_layout;

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
