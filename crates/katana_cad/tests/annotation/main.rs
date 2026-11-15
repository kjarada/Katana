//! The annotation core, the tool framework and the grips (wave H3), one
//! module per C++ test file:
//!
//! ```text
//! dimensions_and_leaders   tests/cad/annotation/test_dimensions_and_leaders.cpp
//! export_annotation        tests/cad/annotation/test_export_annotation.cpp
//! label_layout             tests/cad/annotation/test_label_layout.cpp
//! leader_edit              tests/cad/annotation/test_leader_edit.cpp
//! auto_label               tests/cad/annotation/test_auto_label.cpp
//! tool_framework           tests/cad/tools/test_tool_framework.cpp
//! tool_feedback            tests/cad/tools/test_tool_feedback.cpp
//! grips                    tests/cad/drawing/test_grips.cpp
//! vertex_editing           tests/cad/drawing/test_vertex_editing.cpp
//! layer_overrides          tests/cad/test_layer_overrides.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The shared ToolDriver
//! (tests/support/tool_driver.rs, this agent's) is compiled here so the
//! tool families' targets find it built.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/tool_driver.rs"]
pub mod tool_driver;

mod auto_label;
mod dimensions_and_leaders;
mod export_annotation;
mod grips;
mod label_layout;
mod layer_overrides;
mod leader_edit;
mod tool_feedback;
mod tool_framework;
mod vertex_editing;

use katana_core::{Error, Result, Status};

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

/// The refusal of a result whose value has no Debug (a command): what
/// `expect_err` would give.
#[track_caller]
pub fn refused<T>(result: Result<T>) -> Error {
    match result {
        Ok(_) => panic!("expected a refusal"),
        Err(error) => error,
    }
}

/// std::mt19937 with std::uniform_real_distribution<double>: GCC's exact
/// sequence (generate_canonical draws two 32-bit words, the first the low
/// half), as katana_geometry's test support has it.
pub struct Mt19937 {
    state: [u32; 624],
    index: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Self {
        let mut state = [0; 624];
        state[0] = seed;
        for index in 1..624 {
            state[index] = 1812433253_u32
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 30))
                .wrapping_add(index as u32);
        }
        Self { state, index: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index == 624 {
            for index in 0..624 {
                let value =
                    (self.state[index] & 0x80000000) | (self.state[(index + 1) % 624] & 0x7fffffff);
                self.state[index] = self.state[(index + 397) % 624]
                    ^ (value >> 1)
                    ^ if value & 1 != 0 { 0x9908b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c5680;
        value ^= (value << 15) & 0xefc60000;
        value ^ (value >> 18)
    }

    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        loop {
            let bits = u64::from(self.next_u32()) | (u64::from(self.next_u32()) << 32);
            let fraction = bits as f64 / 18446744073709551616.0;
            if fraction < 1.0 {
                return fraction * (high - low) + low;
            }
        }
    }
}
