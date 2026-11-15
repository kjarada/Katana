//! The annotation and drawing verbs driven through the interpreter, and the
//! leaders, presets and scope they act on (cad wave J, agent J4), one module
//! per C++ test file:
//!
//! ```text
//! annotation_verbs    tests/cad/annotation/test_annotation_verbs.cpp
//! dimstyle_set_many   tests/cad/annotation/test_dimstyle_set_many.cpp
//! leader_positions    tests/cad/annotation/test_leader_positions.cpp
//! leader_presets      tests/cad/annotation/test_leader_presets.cpp
//! leader_scope        tests/cad/annotation/test_leader_scope.cpp
//! smart_leaders       tests/cad/annotation/test_smart_leaders.cpp
//! drawing_verbs       tests/cad/drawing/test_drawing_verbs.cpp
//! new_kinds_in_main   tests/cad/drawing/test_new_kinds_in_main.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod annotation_verbs;
mod dimstyle_set_many;
mod drawing_verbs;
mod leader_positions;
mod leader_presets;
mod leader_scope;
mod new_kinds_in_main;
mod smart_leaders;

use katana_cad::{CommandInterpreter, Document};
use katana_commands::create_entities;
use katana_core::{ErrorCode, Result};
use katana_entity::{Entity, EntityId, Geometry, LeaderGeometry, PropertyValue};
use katana_geometry::Point2;

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

/// Point2(x, y).
pub fn pt(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// std::stoull: the whole number at the start of `text` (after any blanks),
/// failing the test when there is none, as stoull throws.
#[track_caller]
pub fn leading_number(text: &str) -> u64 {
    let trimmed = text.trim_start();
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    match trimmed[..digits].parse() {
        Ok(number) => number,
        Err(_) => panic!("no whole number at the start of {text:?}"),
    }
}

/// std::get<double> of a property: the real it holds, failing the test for
/// anything else.
#[track_caller]
pub fn real_of(value: Option<&PropertyValue>) -> f64 {
    match value {
        Some(PropertyValue::Real(real)) => *real,
        other => panic!("not a real: {other:?}"),
    }
}

/// A document and an interpreter that runs lines on it (the C++ tests'
/// Session). The interpreter does not hold the document (waveI_api.md, I1):
/// each line lends it.
#[derive(Default)]
pub struct Session {
    pub document: Document,
    pub interpreter: CommandInterpreter,
}

impl Session {
    /// What the interpreter answers `line`, refused or not.
    pub fn attempt(&mut self, line: &str) -> Result<String> {
        self.interpreter.run(&mut self.document, line)
    }

    /// The reply to `line`, failing the test when it is refused.
    #[track_caller]
    pub fn run(&mut self, line: &str) -> String {
        match self.attempt(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line}\n  -> {}", error.describe()),
        }
    }

    /// Whether `line` was refused, with no expectation either way.
    pub fn refused(&mut self, line: &str) -> bool {
        self.attempt(line).is_err()
    }

    /// The refusal's words, failing the test when `line` was carried out.
    #[track_caller]
    pub fn refusal(&mut self, line: &str) -> String {
        match self.attempt(line) {
            Ok(reply) => panic!("{line} was not refused: {reply}"),
            Err(error) => error.describe(),
        }
    }

    /// The refusal's code, failing the test when `line` was carried out.
    #[track_caller]
    pub fn fails(&mut self, line: &str) -> ErrorCode {
        match self.attempt(line) {
            Ok(reply) => panic!("{line} unexpectedly succeeded: {reply}"),
            Err(error) => error.code,
        }
    }

    /// The undo steps the history holds.
    pub fn steps(&self) -> usize {
        self.document.history().undo_count()
    }

    /// The first entity the latest step made.
    #[track_caller]
    pub fn last(&self) -> EntityId {
        match self.document.last_created_entities().first() {
            Some(&id) => id,
            None => panic!("the latest step made nothing"),
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

    /// The leader of `id`, failing the test for anything else.
    #[track_caller]
    pub fn leader(&self, id: EntityId) -> LeaderGeometry {
        let entity = self.entity(id);
        match entity.geometry {
            Geometry::Leader(leader) => leader,
            other => panic!("entity {id} is not a leader: {other:?}"),
        }
    }

    /// Makes `entity` as one step and gives its id.
    #[track_caller]
    pub fn add(&mut self, entity: Entity) -> EntityId {
        if let Err(error) = self.document.execute(create_entities(vec![entity])) {
            panic!("{}", error.describe());
        }
        self.last()
    }
}
