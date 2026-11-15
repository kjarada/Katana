//! The cad layer's top-level verbs and the tools under them, driven as the
//! window, katana_cli and katana_mcp drive them (wave J, agent J5), one module
//! per C++ test file:
//!
//! ```text
//! alignment_report   tests/cad/test_alignment_report.cpp
//! code_words         tests/cad/customisation/test_code_words.cpp
//! project_crs        tests/cad/test_project_crs.cpp
//! property_outline   tests/cad/test_property_outline.cpp
//! scope_verbs        tests/cad/test_scope_verbs.cpp
//! survey_tools       tests/cad/test_survey_tools.cpp
//! view_verbs         tests/cad/test_view_verbs.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The project's coordinate system and the
//! coordinate converter reach PROJ, which finds its proj.db as it does for the
//! geodesy twins.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod alignment_report;
mod code_words;
mod project_crs;
mod property_outline;
mod scope_verbs;
mod survey_tools;
mod view_verbs;

use katana_cad::{CommandInterpreter, Document};
use katana_commands::CommandPtr;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_entity::entity::INVALID_ENTITY_ID;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// ASSERT_TRUE(result.ok()) and the value, or a panic naming the error.
#[track_caller]
pub fn ok<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// ASSERT_FALSE(result.ok()) and the error, or a panic: the call succeeded.
#[track_caller]
pub fn err<T>(result: Result<T>) -> Error {
    match result {
        Ok(_) => panic!("expected a refusal, and the call succeeded"),
        Err(error) => error,
    }
}

/// ASSERT_TRUE(status.ok()), naming the error when it is not.
#[track_caller]
pub fn done(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

/// Executes `command`, failing the test when the document refuses it.
#[track_caller]
pub fn must(document: &mut Document, command: CommandPtr) {
    done(document.execute(command));
}

/// Executes `command` and gives the id of the first entity it made (0 for
/// none), failing the test when the document refuses it: the C++ fixtures'
/// create().
#[track_caller]
pub fn create(document: &mut Document, command: CommandPtr) -> EntityId {
    must(document, command);
    document
        .last_created_entities()
        .first()
        .copied()
        .unwrap_or(INVALID_ENTITY_ID)
}

/// Owned strings, for the fields that hold a Vec<String>.
pub fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

/// The lines of a reply, as core::splitLines gives them.
pub fn lines_of(text: &str) -> Vec<String> {
    katana_core::text::split_lines(text.as_bytes())
        .into_iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect()
}

/// A document and an interpreter that runs lines on it (the C++ tests'
/// Session, and their `Document document; CommandInterpreter
/// interpreter(document);`).
#[derive(Default)]
pub struct Session {
    pub document: Document,
    pub interpreter: CommandInterpreter,
}

impl Session {
    /// What the interpreter answers `line` with.
    pub fn run(&mut self, line: &str) -> Result<String> {
        self.interpreter.run(&mut self.document, line)
    }

    /// The reply to `line`, failing the test when it is refused.
    #[track_caller]
    pub fn ok(&mut self, line: &str) -> String {
        match self.run(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line} -> {}", error.describe()),
        }
    }

    /// The refusal of `line`, failing the test when it succeeds.
    #[track_caller]
    pub fn refused(&mut self, line: &str) -> Error {
        match self.run(line) {
            Ok(reply) => panic!("{line} unexpectedly succeeded: {reply}"),
            Err(error) => error,
        }
    }

    /// The code `line` is refused with, failing the test when it succeeds.
    #[track_caller]
    pub fn fails(&mut self, line: &str) -> ErrorCode {
        self.refused(line).code
    }
}
