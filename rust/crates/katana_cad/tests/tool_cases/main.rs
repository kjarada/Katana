//! The tools across families and the Katana Standard customisation (the port's
//! cad wave J, plan agent J6), one module per C++ test file:
//!
//! ```text
//! annotate_dimension_chain   tests/cad/tools/test_annotate_dimension_chain.cpp
//! annotate_label_tool        tests/cad/tools/test_annotate_label_tool.cpp
//! annotate_leader_options    tests/cad/tools/test_annotate_leader_options.cpp
//! annotate_text_options      tests/cad/tools/test_annotate_text_options.cpp
//! leader_preset_tools        tests/cad/tools/test_leader_preset_tools.cpp
//! vertex_tool_feedback       tests/cad/tools/test_vertex_tool_feedback.cpp
//! modify_vertex              tests/cad/tools/test_modify_vertex.cpp
//! select_and_match           tests/cad/tools/test_select_and_match.cpp
//! katana_standard            tests/cad/customisation/test_katana_standard.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The tools are reached through
//! `tool_catalog()` by their C++ ids and driven through H3's ToolDriver
//! (tests/support/tool_driver.rs) as the plan view drives them; a verb line is
//! run through a CommandInterpreter of its own, as the C++ tests' `run` makes
//! one per line.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/tool_driver.rs"]
pub mod tool_driver;

mod annotate_dimension_chain;
mod annotate_label_tool;
mod annotate_leader_options;
mod annotate_text_options;
mod katana_standard;
mod leader_preset_tools;
mod modify_vertex;
mod select_and_match;
mod vertex_tool_feedback;

use katana_cad::interactive_tool::ToolStepOutcome;
use katana_cad::{CommandInterpreter, Document};
use katana_core::Status;
use katana_entity::{AnchorPoint, AnchorRef};
use katana_geometry::Point2;

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

/// The C++ tests' `run`: `line` through an interpreter of its own, failing
/// the test when it is refused.
#[track_caller]
pub fn run(document: &mut Document, line: &str) {
    if let Err(error) = CommandInterpreter::new().run(document, line) {
        panic!("{line}\n  -> {}", error.describe());
    }
}

/// Whether `text` holds `part` (the C++ tests' `contains`).
pub fn contains(text: &str, part: &str) -> bool {
    text.contains(part)
}

/// Point2(x, y).
pub fn pt(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// The C++ `AnchorRef{entity, point, index}`: the parameter at its default.
pub fn anchor(entity: u64, point: AnchorPoint, index: u32) -> AnchorRef {
    AnchorRef {
        entity,
        point,
        index,
        parameter: 0.0,
    }
}
