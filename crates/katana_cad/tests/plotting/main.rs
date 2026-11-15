//! The cad layer's plotting core (wave G2), one module per C++ test file:
//!
//! ```text
//! frame               tests/cad/plotting/test_frame.cpp
//! sheet_fields        tests/cad/plotting/test_sheet_fields.cpp
//! layout              tests/cad/plotting/test_layout.cpp
//! page_setup          tests/cad/plotting/test_page_setup.cpp
//! section_fit         tests/cad/plotting/test_section_fit.cpp
//! generators          tests/cad/plotting/test_generators.cpp
//! sheet_storage       tests/cad/plotting/test_sheet_storage.cpp
//! tables              tests/cad/plotting/test_tables.cpp
//! viewport_edits      tests/cad/plotting/test_viewport_edits.cpp
//! plan_grid           tests/cad/plotting/test_plan_grid.cpp
//! section_annotation  tests/cad/plotting/test_section_annotation.cpp
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

use katana_geometry::{Box2, Point2};

mod frame;
mod generators;
mod layout;
mod page_setup;
mod plan_grid;
mod section_annotation;
mod section_fit;
mod sheet_fields;
mod sheet_storage;
mod tables;
mod viewport_edits;

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

/// The C++ tests' `expectBox`: each corner within 1e-9 of the one worked by
/// hand.
#[track_caller]
pub fn expect_box(rect: &Box2, x0: f64, y0: f64, x1: f64, y1: f64) {
    near(rect.min.x, x0, 1e-9);
    near(rect.min.y, y0, 1e-9);
    near(rect.max.x, x1, 1e-9);
    near(rect.max.y, y1, 1e-9);
}

/// The C++ tests' `expectPoint`.
#[track_caller]
pub fn expect_point(point: Point2, x: f64, y: f64, tolerance: f64) {
    near(point.x, x, tolerance);
    near(point.y, y, tolerance);
}

/// The C++ tests' `box(x0, y0, x1, y1)`.
pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}
