//! The cad layer's command interpreter and the Document it drives (wave I,
//! agent I1), one module per C++ test file:
//!
//! ```text
//! cad                tests/cad/test_cad.cpp
//! document_changes   tests/cad/test_document_changes.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The interpreter-driven files of the
//! verb families are wave J's.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/property.rs"]
mod property;

mod cad;
mod document_changes;

use std::path::PathBuf;

use katana_cad::{CommandInterpreter, Document};
use katana_commands::CommandPtr;
use katana_core::ErrorCode;
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::{Entity, EntityId};

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

/// Runs `command` and gives the id of the first entity it made (0 for none),
/// failing the test when the document refuses it.
#[track_caller]
pub fn must_create(document: &mut Document, command: CommandPtr) -> EntityId {
    let status = document.execute(command);
    assert!(status.is_ok(), "{status:?}");
    document
        .last_created_entities()
        .first()
        .copied()
        .unwrap_or(INVALID_ENTITY_ID)
}

/// A directory of a test's own under the temporary directory, named after the
/// test and the process, removed with its contents when dropped: Rust runs a
/// binary's tests on parallel threads, where gtest gave each its own process.
pub struct ScratchDirectory {
    pub path: PathBuf,
}

impl ScratchDirectory {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "katana-cad-interpreter-{name}-{}",
            std::process::id()
        ));
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

/// A document and an interpreter that runs lines on it (the C++ tests'
/// Session).
#[derive(Default)]
pub struct Session {
    pub document: Document,
    pub interpreter: CommandInterpreter,
}

impl Session {
    /// The reply to `line`, failing the test when it is refused.
    #[track_caller]
    pub fn ok(&mut self, line: &str) -> String {
        match self.interpreter.run(&mut self.document, line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line} -> {}", error.describe()),
        }
    }

    /// The code `line` is refused with, failing the test when it succeeds.
    #[track_caller]
    pub fn fails(&mut self, line: &str) -> ErrorCode {
        match self.interpreter.run(&mut self.document, line) {
            Ok(reply) => panic!("{line} unexpectedly succeeded: {reply}"),
            Err(error) => error.code,
        }
    }

    /// The entity of `id`, failing the test when there is none.
    #[track_caller]
    pub fn entity(&self, id: EntityId) -> Entity {
        match self.document.model().entities.find(id) {
            Some(entity) => entity.clone(),
            None => panic!("no entity with id {id}"),
        }
    }
}
