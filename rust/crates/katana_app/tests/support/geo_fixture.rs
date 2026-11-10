//! What the geo tests share (tests/geo/vector_fixture.hpp and the fixtures of
//! tests/geo's files, which all have its shape): a drawing, its interpreter
//! and a scratch folder of the test's own, lent to the ONE executor as a
//! `Context` for each phase, as the window and the session lend theirs; the
//! data folders where the C++ reads them; and the lock of the cases that run
//! GDAL on a shared file.
//!
//! The C++ fixture holds a `geo::Context` of references for its life; the
//! Rust Context is a bundle of borrows made for each phase
//! (katana_app::geo::geo_verbs), so `GeoFixture::context` makes one when a
//! line runs and the drawing is the test's to read between lines. The
//! fixture has no reference data until step 6's wave-2 prelude adds it, with
//! the Context's member.
//!
//! One owner (G1); a test target includes it with
//! `#[path = "../support/geo_fixture.rs"] pub mod geo_fixture;`.

#![allow(
    dead_code,
    reason = "Each test target that includes the fixture uses a part of it."
)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use katana_app::geo::replies::{Record, parse_records};
use katana_app::geo::{Context, FrontEnd, Prepared, prepare, run_now};
use katana_cad::{CommandInterpreter, Document};
use katana_commands::{create_entities, create_layer};
use katana_core::Result;
use katana_entity::entity::{INVALID_ENTITY_ID, property_to_string};
use katana_entity::{Entity, EntityId, Geometry, Layer, PropertyMap};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_terrain::SurfaceStore;

// ---- the data, where the C++ reads it -----------------------------------------------------------

/// A path of the checkout, where the C++ tests read it.
pub fn checkout(relative: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")).join(relative)
}

/// KATANA_GEO_TEST_DATA, tests/geo/data, as the text a line names it by,
/// with forward slashes.
pub fn geo_data() -> String {
    generic(&checkout("tests/geo/data"))
}

/// KATANA_GIS_SAMPLES, samples/gis, with forward slashes.
pub fn gis_samples() -> String {
    generic(&checkout("samples/gis"))
}

/// A path's generic_string: the text, '/' its separator.
pub fn generic(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

static SHARED_DATA: Mutex<()> = Mutex::new(());

/// The lock of the cases that run GDAL on a file of tests/geo/data or
/// samples/gis: libtest runs a target's cases on threads of one process,
/// where ctest ran each in its own, and GDAL's .aux.xml sidecars of a shared
/// file must never race. One lock per target that includes this file; cargo
/// runs the targets one after another.
pub fn shared_data() -> MutexGuard<'static, ()> {
    SHARED_DATA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- a folder of the test's own -----------------------------------------------------------------

/// The C++'s TempDir(name): `<temp>/<prefix><name>`, emptied when made and
/// removed with what it holds when dropped - here named for the process too,
/// since libtest runs a target's cases on threads of one process.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str, name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{prefix}{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A file in it, as the generic_string a line names it by.
    pub fn file(&self, name: &str) -> String {
        generic(&self.path.join(name))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ---- the executor's fixture ---------------------------------------------------------------------

/// The drawing the geo verbs' tests act on, with the executor's own context
/// lent for each phase.
pub struct GeoFixture {
    pub scratch: TempDir,
    pub document: Document,
    pub interpreter: CommandInterpreter,
    /// No hooks, as in a session: nothing framed, nothing asked.
    pub front_end: FrontEnd,
}

impl GeoFixture {
    /// A fixture whose scratch folder is `<temp>/<prefix><name>`: the C++
    /// fixtures name theirs after the test ("katana-geo-verbs-" + its name).
    pub fn new(prefix: &str, name: &str) -> Self {
        Self {
            scratch: TempDir::new(prefix, name),
            document: Document::new(None),
            interpreter: CommandInterpreter::new(),
            front_end: FrontEnd::default(),
        }
    }

    /// The executor's context for one phase.
    pub fn context(&mut self) -> Context<'_> {
        Context::new(
            &mut self.document,
            &self.interpreter,
            self.scratch.path(),
            &self.front_end,
        )
    }

    /// The line through the one executor, all three phases.
    pub fn run(&mut self, line: &str) -> Result<String> {
        run_now(&mut self.context(), line, None, None)
    }

    /// The first phase alone.
    pub fn prepare(&mut self, line: &str) -> Result<Prepared> {
        prepare(&mut self.context(), line)
    }

    /// `line` must run; its reply (the C++'s EXPECT, here an assertion).
    #[track_caller]
    pub fn ok(&mut self, line: &str) -> String {
        match self.run(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line}\n{}", error.describe()),
        }
    }

    /// A line the interpreter must take: the C++'s `type(line)`.
    #[track_caller]
    pub fn type_line(&mut self, line: &str) {
        if let Err(error) = self.interpreter.run(&mut self.document, line) {
            panic!("{line}: {}", error.describe());
        }
    }

    /// The drawing's surfaces, which the verbs change.
    pub fn surfaces(&self) -> &SurfaceStore {
        self.document.surfaces()
    }

    // ---- vector_fixture.hpp ----

    /// One entity drawn as one undo step; its id.
    #[track_caller]
    pub fn add(&mut self, geometry: Geometry, layer: &str, properties: PropertyMap) -> EntityId {
        let entity = Entity {
            geometry,
            layer: String::from(layer),
            properties,
            ..Entity::default()
        };
        if !self.document.model().layers.contains(layer) {
            let made = Layer {
                name: String::from(layer),
                ..Layer::default()
            };
            assert!(self.document.execute(create_layer(made)).is_ok());
        }
        assert!(self.document.execute(create_entities(vec![entity])).is_ok());
        self.document
            .last_created_entities()
            .first()
            .copied()
            .unwrap_or(INVALID_ENTITY_ID)
    }

    /// A closed polyline through `corners`.
    #[track_caller]
    pub fn area(&mut self, corners: &[Point2], layer: &str, properties: PropertyMap) -> EntityId {
        let ring = Polyline2 {
            vertices: corners.to_vec(),
            closed: true,
        };
        self.add(Geometry::Polyline(ring), layer, properties)
    }

    /// The x0,y0 - x1,y1 rectangle as a closed polyline, anticlockwise.
    #[track_caller]
    pub fn rect(
        &mut self,
        (x0, y0): (f64, f64),
        (x1, y1): (f64, f64),
        layer: &str,
        properties: PropertyMap,
    ) -> EntityId {
        self.area(
            &[
                Point2::new(x0, y0),
                Point2::new(x1, y0),
                Point2::new(x1, y1),
                Point2::new(x0, y1),
            ],
            layer,
            properties,
        )
    }

    #[track_caller]
    pub fn line(
        &mut self,
        (x0, y0): (f64, f64),
        (x1, y1): (f64, f64),
        layer: &str,
        properties: PropertyMap,
    ) -> EntityId {
        let segment = Segment2::new(Point2::new(x0, y0), Point2::new(x1, y1));
        self.add(Geometry::Line(segment), layer, properties)
    }

    /// Every entity on `layer`, in id order.
    pub fn on(&self, layer: &str) -> Vec<Entity> {
        let mut found = Vec::new();
        self.document.model().entities.for_each(|entity| {
            if entity.layer == layer {
                found.push(entity.clone());
            }
        });
        found
    }

    /// The area the closed polylines and circles on `layer` enclose, holes
    /// (tagged gis.ring=hole) taken away: vector_fixture.hpp's areaOn.
    pub fn area_on(&self, layer: &str) -> f64 {
        let mut total = 0.0;
        for entity in self.on(layer) {
            let one = match &entity.geometry {
                Geometry::Polyline(ring) => ring.area(),
                Geometry::Circle(circle) => circle.area(),
                _ => 0.0,
            };
            let hole = entity
                .properties
                .get("gis.ring")
                .is_some_and(|tag| property_to_string(tag) == "hole");
            total += if hole { -one } else { one };
        }
        total
    }

    pub fn entity_count(&self) -> usize {
        self.document.model().entities.size()
    }

    /// The first record of `kind` in a reply.
    pub fn record(reply: &str, kind: &str) -> Option<Record> {
        parse_records(reply)
            .into_iter()
            .find(|record| record.kind == kind)
    }
}

// ---- the tests' asserts -------------------------------------------------------------------------

/// ASSERT_TRUE(result.ok()) << result.error().describe(), and the value.
#[track_caller]
pub fn must<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// ASSERT_FALSE(result.ok()), and the error.
#[track_caller]
pub fn refused<T>(result: Result<T>, what: &str) -> katana_core::Error {
    match result {
        Ok(_) => panic!("{what} should have been refused"),
        Err(error) => error,
    }
}

/// EXPECT_NE(text.find(part), npos) << text.
#[track_caller]
pub fn holds(text: &str, part: &str) {
    assert!(text.contains(part), "{part:?} is not in:\n{text}");
}

/// EXPECT_EQ(text.find(part), npos) << text.
#[track_caller]
pub fn lacks(text: &str, part: &str) {
    assert!(!text.contains(part), "{part:?} is in:\n{text}");
}

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// The words as the owned strings a path or a tail is.
pub fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// The bytes of a file, to show a refused line left it as it was; empty for
/// none.
pub fn bytes_of(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}
