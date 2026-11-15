//! The edit tools (the port's cad wave I, plan agent I3), driven as the plan
//! view drives them through H3's ToolDriver, one module per C++ test file:
//!
//! ```text
//! modify_edit     tests/cad/tools/test_modify_edit.cpp
//! draw_divide     tests/cad/tools/test_draw_divide.cpp
//! inquiry         tests/cad/tools/test_inquiry.cpp
//! modify_length   tests/cad/tools/test_modify_length.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/tool_driver.rs"]
pub mod tool_driver;

mod draw_divide;
mod inquiry;
mod modify_edit;
mod modify_length;

use katana_cad::interactive_tool::ToolStepOutcome;
use katana_core::Status;
use katana_entity::{Entity, EntityId};

use tool_driver::ToolDriver;

pub const CONTINUE: ToolStepOutcome = ToolStepOutcome::Continue;
pub const DONE: ToolStepOutcome = ToolStepOutcome::Done;
pub const REJECTED: ToolStepOutcome = ToolStepOutcome::Rejected;

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

/// The entity `id` as the document has it now; fails the test when it is
/// not in the drawing.
#[track_caller]
pub fn entity_of(driver: &ToolDriver, id: EntityId) -> Entity {
    match driver.document().model().entities.find(id) {
        Some(entity) => entity.clone(),
        None => panic!("entity {id} is not in the drawing"),
    }
}

/// The only entity the last command created.
#[track_caller]
pub fn created(driver: &ToolDriver) -> EntityId {
    let ids = driver.document().last_created_entities();
    assert_eq!(ids.len(), 1, "one entity made");
    ids[0]
}

/// Whether `text` holds `part` (the C++ tests' `contains`).
pub fn contains(text: &str, part: &str) -> bool {
    text.contains(part)
}
