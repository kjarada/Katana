//! The cad layer's styles, survey coding and scope (wave G1), one module per
//! C++ test file:
//!
//! ```text
//! style_resolver                  tests/cad/customisation/test_style_resolver.cpp
//! style_catalogue                 tests/cad/customisation/test_style_catalogue.cpp
//! style_catalogue_current_style   tests/cad/customisation/test_style_catalogue_current_style.cpp
//! symbol_assign                   tests/cad/customisation/test_symbol_assign.cpp
//! linework                        tests/cad/customisation/test_linework.cpp
//! linework_code_names             tests/cad/customisation/test_linework_code_names.cpp
//! linework_features               tests/cad/customisation/test_linework_features.cpp
//! survey_import                   tests/cad/test_survey_import.cpp
//! survey_coding                   tests/cad/test_survey_coding.cpp
//! view_set                        tests/cad/test_view_set.cpp
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

mod linework;
mod linework_code_names;
mod linework_features;
mod style_catalogue;
mod style_catalogue_current_style;
mod style_resolver;
mod survey_coding;
mod survey_import;
mod symbol_assign;
mod view_set;

use std::path::PathBuf;

use katana_cad::Document;
use katana_commands::CommandPtr;
use katana_entity::{LineStyle, StyleLibrary};

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

/// Executes `command` on `document`, failing the test with the refusal.
#[track_caller]
pub fn must(document: &mut Document, command: CommandPtr) {
    if let Err(error) = document.execute(command) {
        panic!("{}", error.describe());
    }
}

/// A library holding `definitions`, each added (and failing the test when one
/// cannot be).
#[track_caller]
pub fn library_of(definitions: Vec<LineStyle>) -> StyleLibrary {
    let mut library = StyleLibrary::default();
    for definition in definitions {
        assert!(library.add(definition).is_ok());
    }
    library
}

/// A scratch folder of the test's own, removed when it drops: unique per test
/// and per process, since Rust runs a binary's tests on parallel threads.
pub struct ScratchDirectory {
    pub path: PathBuf,
}

impl ScratchDirectory {
    pub fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("katana-cad-styles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
