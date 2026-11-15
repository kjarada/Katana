//! The draw and transform tools (the cad port's wave I2), one module per C++
//! test file:
//!
//! ```text
//! draw_lines          tests/cad/tools/test_draw_lines.cpp
//! draw_curves         tests/cad/tools/test_draw_curves.cpp
//! draw_professional   tests/cad/tools/test_draw_professional.cpp
//! modify_transform    tests/cad/tools/test_modify_transform.cpp
//! drafting            tests/cad/drawing/test_drafting.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. Every tool is driven through H3's
//! ToolDriver (tests/support/tool_driver.rs), as a user would drive it.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/tool_driver.rs"]
pub mod tool_driver;

mod drafting;
mod draw_curves;
mod draw_lines;
mod draw_professional;
mod modify_transform;

use katana_cad::Document;
use katana_cad::interactive_tool::{ToolStep, ToolStepOutcome};
use katana_core::Status;
use katana_entity::Entity;
use katana_geometry::{Point2, Polyline2, Segment2};

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_NEAR on both coordinates (the C++ tests' expectNear).
#[track_caller]
pub fn near_point(actual: Point2, expected: Point2, epsilon: f64) {
    assert!(
        (actual.x - expected.x).abs() <= epsilon && (actual.y - expected.y).abs() <= epsilon,
        "actual {actual:?} is not within {epsilon:?} of expected {expected:?}"
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

/// ASSERT_TRUE(status.ok()) << status.error().describe().
#[track_caller]
pub fn ok(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

/// A refusal says why (the C++ tests' expectRefused).
#[track_caller]
pub fn refused(step: &ToolStep) {
    assert_eq!(step.outcome, ToolStepOutcome::Rejected, "{step:?}");
    assert!(!step.message.is_empty(), "a refusal says why");
}

/// Every entity of the document, in id order - the order they were made in.
pub fn entities_of(document: &Document) -> Vec<Entity> {
    let mut out = Vec::new();
    document
        .model()
        .entities
        .for_each(|entity| out.push(entity.clone()));
    out
}

pub fn point(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

pub fn points(list: &[[f64; 2]]) -> Vec<Point2> {
    list.iter().map(|&[x, y]| Point2::new(x, y)).collect()
}

pub fn segment(a: [f64; 2], b: [f64; 2]) -> Segment2 {
    Segment2::new(Point2::new(a[0], a[1]), Point2::new(b[0], b[1]))
}

pub fn polyline(list: &[[f64; 2]], closed: bool) -> Polyline2 {
    Polyline2::new(points(list), closed)
}

/// Each vertex within `tolerance` of the one expected (the C++ verticesNear).
#[track_caller]
pub fn vertices_near(actual: &[Point2], expected: &[Point2], tolerance: f64) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{} vertices where {} were expected",
        actual.len(),
        expected.len()
    );
    for (i, (&got, &wanted)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (got.x - wanted.x).abs() <= tolerance && (got.y - wanted.y).abs() <= tolerance,
            "vertex {i}: {got:?} is not within {tolerance:?} of {wanted:?}"
        );
    }
}
