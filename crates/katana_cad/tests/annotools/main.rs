//! The vertex and annotate tools (wave I4), one module per C++ test file:
//!
//! ```text
//! annotate            tests/cad/tools/test_annotate.cpp
//! smart_leader_tool   tests/cad/tools/test_smart_leader_tool.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The tools are driven through H3's
//! ToolDriver (tests/support/tool_driver.rs) as the plan view drives them.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/tool_driver.rs"]
pub mod tool_driver;

mod annotate;
mod smart_leader_tool;

use katana_cad::annotation::drawing::{Drawing, estimated_measure};
use katana_cad::annotation::leader_draw::build_leader;
use katana_core::Status;
use katana_entity::{
    DimensionGeometry, Entity, Geometry, LeaderContext, LeaderGeometry, TextGeometry,
};

use crate::tool_driver::ToolDriver;

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

/// ASSERT_TRUE(status.ok()) << status.error().describe().
#[track_caller]
pub fn ok(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

/// Whether `text` holds `part`, with the text in the failure message.
#[track_caller]
pub fn assert_contains(text: &str, part: &str) {
    assert!(text.contains(part), "{text:?} does not contain {part:?}");
}

/// Every entity in the drawing, in id order - which is creation order.
pub fn drawing(driver: &ToolDriver) -> Vec<Entity> {
    let mut out = Vec::new();
    driver
        .document()
        .model()
        .entities
        .for_each(|entity| out.push(entity.clone()));
    out
}

/// The texts in the drawing, in creation order (the C++ shapesOf<TextGeometry>).
pub fn texts(driver: &ToolDriver) -> Vec<TextGeometry> {
    drawing(driver)
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

/// The dimensions in the drawing, in creation order.
pub fn dimensions(driver: &ToolDriver) -> Vec<DimensionGeometry> {
    drawing(driver)
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Dimension(dimension) => Some(dimension),
            _ => None,
        })
        .collect()
}

/// The leaders in the drawing, in creation order.
pub fn leaders(driver: &ToolDriver) -> Vec<LeaderGeometry> {
    drawing(driver)
        .into_iter()
        .filter_map(|entity| match entity.geometry {
            Geometry::Leader(leader) => Some(leader),
            _ => None,
        })
        .collect()
}

/// What `leader` draws at 1 : 1000 in `driver`'s drawing, with the estimated
/// measure: 0.6 of the height a character.
pub fn drawn(driver: &ToolDriver, leader: &LeaderGeometry) -> Drawing {
    build_leader(
        driver.document().model(),
        leader,
        1000.0,
        &estimated_measure(),
        &LeaderContext::default(),
    )
}
