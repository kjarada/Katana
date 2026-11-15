//! The drawing bindings' cases (step 5, wave L, agent L1), one module per C++
//! test file:
//!
//! ```text
//! drawing_dataset  tests/geo/test_drawing_dataset.cpp
//! drawing_curves   tests/geo/test_drawing_curves.cpp   the five drawingDataset
//!                                                      and three surfaceInput cases
//! contour_verb     tests/geo/test_contour_verb.cpp     ContourEntities
//! ```
//!
//! The other suites of those files drive the verbs through the app layer
//! (step 6), or bind GDAL's arguments (katana_gis's processing target). Each
//! gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. Nothing here opens a file: every case
//! builds its drawing or its features in memory.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod contour_verb;
mod drawing_curves;
mod drawing_dataset;

use katana_cad::Document;
use katana_commands::create_entities;
use katana_entity::{Entity, EntityId};
use katana_gis::GeoPoint;

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

/// The entities into a drawing, one step, and their ids in order.
#[track_caller]
pub fn draw(document: &mut Document, entities: Vec<Entity>) -> Vec<EntityId> {
    let status = document.execute(create_entities(entities));
    assert!(status.is_ok(), "{status:?}");
    document.last_created_entities()
}

pub fn point(x: f64, y: f64, z: f64) -> GeoPoint {
    GeoPoint { x, y, z }
}
