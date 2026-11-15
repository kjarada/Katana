//! The cad layer's survey jobs (wave H1), one module per C++ test file:
//!
//! ```text
//! survey_points          tests/cad/customisation/test_survey_points.cpp
//! survey_finish          tests/cad/customisation/test_survey_finish.cpp
//! survey_job             tests/cad/test_survey_job.cpp
//! survey_job_finish      tests/cad/test_survey_job_finish.cpp
//! survey_job_throughput  tests/cad/test_survey_job_throughput.cpp
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod survey_finish;
mod survey_job;
mod survey_job_finish;
mod survey_job_throughput;
mod survey_points;

use std::collections::BTreeMap;
use std::path::PathBuf;

use katana_cad::Document;
use katana_commands::CommandPtr;
use katana_core::{Error, Result};
use katana_entity::{Entity, EntityId, Geometry, Layer, PropertyValue, Style};
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

/// The value of a result that must have succeeded, failing the test with the
/// refusal otherwise.
#[track_caller]
pub fn expect_ok<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// The refusal of a result that must have failed (its value need not be
/// Debug, as expect_err would want).
#[track_caller]
pub fn refused<T>(result: Result<T>) -> Error {
    match result {
        Ok(_) => panic!("expected a refusal"),
        Err(error) => error,
    }
}

/// Executes `command` on `document`, failing the test with the refusal.
#[track_caller]
pub fn must(document: &mut Document, command: CommandPtr) {
    if let Err(error) = document.execute(command) {
        panic!("{}", error.describe());
    }
}

pub fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// The text a property holds: "<absent>" without one, "<not text>" for
/// another kind of value.
pub fn text_property(entity: &Entity, key: &str) -> String {
    match entity.properties.get(key) {
        None => String::from("<absent>"),
        Some(PropertyValue::Text(text)) => text.clone(),
        Some(_) => String::from("<not text>"),
    }
}

/// The point entity whose "point" property reads `number` - the last such, in
/// entity order, as the C++'s visit leaves it.
pub fn point_numbered<'a>(document: &'a Document, number: &str) -> Option<&'a Entity> {
    let entities = &document.model().entities;
    let mut found = None;
    for id in entities.ids() {
        if let Some(entity) = entities.find(id)
            && matches!(entity.geometry, Geometry::Point(_))
            && text_property(entity, "point") == number
        {
            found = Some(entity);
        }
    }
    found
}

/// point_numbered, which the test needs to be there.
#[track_caller]
pub fn numbered<'a>(document: &'a Document, number: &str) -> &'a Entity {
    match point_numbered(document, number) {
        Some(entity) => entity,
        None => panic!("no point numbered {number}"),
    }
}

/// The lines in the drawing, in the order they were created.
pub fn lines_of(document: &Document) -> Vec<&Entity> {
    let entities = &document.model().entities;
    entities
        .ids()
        .into_iter()
        .filter_map(|id| entities.find(id))
        .filter(|entity| matches!(entity.geometry, Geometry::Polyline(_)))
        .collect()
}

pub fn point_count(document: &Document) -> usize {
    let mut count = 0;
    document.model().entities.for_each(|entity| {
        count += usize::from(matches!(entity.geometry, Geometry::Point(_)));
    });
    count
}

#[track_caller]
pub fn vertices_of(line: &Entity) -> Vec<Point2> {
    match &line.geometry {
        Geometry::Polyline(polyline) => polyline.vertices.clone(),
        _ => panic!("entity {} is not a line", line.id),
    }
}

/// Everything a drawing holds that an import can change, by value.
#[derive(Debug, PartialEq)]
pub struct Drawing {
    pub entities: BTreeMap<EntityId, Entity>,
    pub layers: Vec<Layer>,
    pub styles: Vec<Style>,
}

pub fn drawing_of(document: &Document) -> Drawing {
    Drawing {
        entities: entities_of(document),
        layers: document.model().layers.all(),
        styles: document.model().styles.all(),
    }
}

pub fn entities_of(document: &Document) -> BTreeMap<EntityId, Entity> {
    let mut entities = BTreeMap::new();
    document.model().entities.for_each(|entity| {
        entities.insert(entity.id, entity.clone());
    });
    entities
}

/// A scratch folder of the test's own, removed when it drops: unique per test
/// and per process, since Rust runs a binary's tests on parallel threads.
pub struct ScratchDirectory {
    pub path: PathBuf,
}

impl ScratchDirectory {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "katana-cad-surveyjobs-{name}-{}",
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
