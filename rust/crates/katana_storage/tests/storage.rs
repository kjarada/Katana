//! The twin of tests/storage/test_storage.cpp: the SQLite wrapper, and the
//! project store's lifecycle, round trip, migrations, backups and recovery.

#![allow(
    clippy::excessive_precision,
    reason = "Preserve the decimal constants copied verbatim from the C++ oracle."
)]

pub mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use katana_core::{ErrorCode, Status};
use katana_entity::tables::{DEFAULT_LAYER_NAME, NO_HATCH, NO_SYMBOL};
use katana_entity::{
    Alignment, ArrowHead, ChangeEvent, Color, DimensionGeometry, DimensionStyle, Entity, Geometry,
    HatchLineFamily, HatchPattern, Layer, Linetype, LinetypeElement, Model, PointGeometry,
    PropertyDefinition, PropertyMap, PropertyType, PropertyValue, Style, TextGeometry,
};
use katana_geometry::{
    AlignmentPi, Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, HorizontalAlignment, Point2,
    Polyline2, ProfilePvi, Segment2, Spline2, Vec2, VerticalAlignment,
};
use katana_storage::{
    OpenMode, ProjectContents, ProjectMetadata, ProjectStore, Relationship, SqliteDatabase,
    SqliteTransaction, apply_to_model, apply_to_model_consuming, capture_model,
};
use support::{Scratch, double_eq, overwrite_with_garbage};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn open(path: &Path) -> SqliteDatabase {
    SqliteDatabase::open(path, OpenMode::ReadWriteCreate).unwrap()
}

fn captured(model: &Model) -> ProjectContents {
    capture_model(model, ProjectMetadata::default(), Vec::new())
}

fn create(project: &Path) -> ProjectStore {
    ProjectStore::create(project, &ProjectMetadata::default()).unwrap()
}

fn entity(geometry: Geometry) -> Entity {
    Entity {
        geometry,
        ..Entity::default()
    }
}

fn sample_model() -> Model {
    let mut model = Model::default();
    assert!(
        model
            .layers
            .add(Layer {
                name: String::from("Survey"),
                color: Color::new(255, 0, 0, 255),
                visible: true,
                locked: false,
                linetype: String::from("dashed"),
                line_weight: 0.5,
                ..Layer::default()
            })
            .is_ok()
    );
    assert!(
        model
            .layers
            .add(Layer {
                name: String::from("Locked"),
                color: Color::new(0, 0, 255, 128),
                visible: false,
                locked: true,
                ..Layer::default()
            })
            .is_ok()
    );
    assert!(
        model
            .styles
            .add(Style {
                name: String::from("Boundary"),
                color: Some(Color::new(0, 255, 0, 255)),
                line_weight: 0.7,
                linetype: String::from("continuous"),
                ..Style::default()
            })
            .is_ok()
    );
    assert!(
        model
            .styles
            .add(Style {
                name: String::from("ByLayer"),
                color: None,
                line_weight: 0.25,
                linetype: String::from("continuous"),
                ..Style::default()
            })
            .is_ok()
    );
    assert!(
        model
            .properties
            .define(PropertyDefinition {
                name: String::from("elevation"),
                r#type: PropertyType::Real,
                description: String::from("Ground level (m)"),
                default_value: Some(PropertyValue::Real(0.0)),
            })
            .is_ok()
    );
    assert!(
        model
            .properties
            .define(PropertyDefinition {
                name: String::from("code"),
                r#type: PropertyType::Text,
                description: String::new(),
                default_value: None,
            })
            .is_ok()
    );

    let line = Entity {
        geometry: Geometry::Line(Segment2::new(
            p(500000.123456789, 5000000.987654321),
            p(1.0 / 3.0, -2e-9),
        )),
        layer: String::from("Survey"),
        style: String::from("Boundary"),
        color: Some(Color::new(1, 2, 3, 255)),
        properties: PropertyMap::from([
            (String::from("elevation"), PropertyValue::Real(101.25)),
            (
                String::from("code"),
                PropertyValue::Text(String::from("IP \"A\"")),
            ),
            (String::from("n"), PropertyValue::Integer(7)),
        ]),
        metadata: PropertyMap::from([(
            String::from("source"),
            PropertyValue::Text(String::from("fieldbook.csv")),
        )]),
        ..Entity::default()
    };
    assert!(model.entities.add(line).is_ok());

    let hidden = Entity {
        geometry: Geometry::Circle(Circle2::new(p(3.0, 4.0), 5.5)),
        visible: false,
        ..Entity::default()
    };
    assert!(model.entities.add(hidden.clone()).is_ok());

    let geometries = [
        Geometry::Point(PointGeometry {
            position: p(9.0, 9.0),
        }),
        Geometry::Arc(Arc2::new(p(0.0, 0.0), 2.0, 0.5, -1.5)),
        Geometry::Polyline(Polyline2::new(
            vec![p(0.0, 0.0), p(4.0, 0.0), p(4.0, 3.0)],
            true,
        )),
        Geometry::Text(TextGeometry::new(p(1.0, 1.0), "Ünïcödé — BM1", 2.5, 0.25)),
        Geometry::Dimension(DimensionGeometry::new(
            p(0.0, 0.0),
            p(10.0, 0.0),
            2.0,
            "10.00",
        )),
        // The drawing system's kinds: a 3D string with an arc and a missing
        // height, an elliptical arc, a fit-point spline.
        Geometry::CurvePolyline(CurvePolyline2 {
            vertices: vec![
                CurveVertex {
                    position: p(0.0, 0.0),
                    bulge: 0.4,
                    height: Some(31.25),
                },
                CurveVertex {
                    position: p(8.0, 2.0),
                    bulge: 0.0,
                    height: None,
                },
                CurveVertex {
                    position: p(9.0, 7.0),
                    bulge: -0.2,
                    height: Some(33.5),
                },
            ],
            closed: false,
        }),
        Geometry::Ellipse(Ellipse2 {
            center: p(5.0, 5.0),
            major_axis: Vec2::new(4.0, 3.0),
            ratio: 0.5,
            start_parameter: 1.0,
            sweep: 3.0,
        }),
        Geometry::Spline(
            Spline2::through_points(vec![p(0.0, 0.0), p(2.0, 5.0), p(6.0, 4.0), p(9.0, 9.0)], 3)
                .unwrap(),
        ),
    ];
    for geometry in geometries {
        assert!(model.entities.add(entity(geometry)).is_ok());
    }
    // Retire an id so that next_entity_id is not simply "count + 1".
    let doomed = model.entities.add(hidden).unwrap();
    assert!(model.entities.remove(doomed).is_ok());
    model
}

// Undoes every migration above `version`, so a project can be made to look as
// though it were written by an older build.
//
// Migrations are append-only and each runs once, so a fixture that claims
// version N must actually LOOK like version N: leaving a later migration's
// table in place makes the replay fail with "table already exists", which is
// the migration machinery working correctly on a fixture that lied.
//
// This exists as one function because it was previously written out inside
// each test, and every new migration then broke every older fixture. Adding a
// migration now means adding one case here.
fn rewind_schema_to(database: &SqliteDatabase, version: i32) -> Status {
    // Newest first: a migration is undone before the one it was built on.
    if version < 13 {
        database.execute("DROP TABLE IF EXISTS surfaces;")?;
    }
    if version < 11 {
        database.execute(concat!(
            "DROP TABLE IF EXISTS label_rules;",
            "DROP TABLE IF EXISTS label_styles;",
            "DROP TABLE IF EXISTS text_styles;",
            "ALTER TABLE dimension_styles DROP COLUMN paper_sized;"
        ))?;
    }
    if version < 10 {
        database.execute(concat!(
            "DROP TABLE IF EXISTS survey_job_files;",
            "DROP TABLE IF EXISTS survey_jobs;"
        ))?;
    }
    if version < 9 {
        database.execute(concat!(
            "ALTER TABLE styles DROP COLUMN description;",
            "ALTER TABLE styles DROP COLUMN symbol;",
            "ALTER TABLE styles DROP COLUMN symbol_size;"
        ))?;
    }
    if version < 8 {
        database.execute("DROP TABLE IF EXISTS alignment_pvis;")?;
    }
    if version < 7 {
        database.execute(concat!(
            "DROP TABLE IF EXISTS alignment_pis;",
            "DROP TABLE IF EXISTS alignments;"
        ))?;
    }
    if version < 6 {
        database.execute(concat!(
            "DROP TABLE IF EXISTS hatch_families;",
            "DROP TABLE IF EXISTS hatch_patterns;",
            "ALTER TABLE layers DROP COLUMN hatch_pattern;",
            "ALTER TABLE styles DROP COLUMN hatch_pattern;"
        ))?;
    }
    if version < 5 {
        database.execute(concat!(
            "DROP TABLE IF EXISTS dimension_styles;",
            "ALTER TABLE layers DROP COLUMN dimension_style;"
        ))?;
    }
    if version < 4 {
        database.execute(concat!(
            "DROP TABLE IF EXISTS linetype_elements;",
            "DROP TABLE IF EXISTS linetypes;"
        ))?;
    }
    database.set_user_version(version)
}

// A project exactly as Katana schema version 1 wrote it (no relationships
// table), named `name` with `next` as its next entity id.
fn write_version1_project(project: &Path, name: &str, next: &str) {
    std::fs::create_dir_all(project).unwrap();
    let v1 = open(&project.join("project.db"));
    v1.set_application_id(0x4B544E41).unwrap();
    v1.execute(&format!(
        r#"
            CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE layers (name TEXT PRIMARY KEY, color TEXT NOT NULL, visible INTEGER NOT NULL,
                locked INTEGER NOT NULL, linetype TEXT NOT NULL, line_weight REAL NOT NULL) WITHOUT ROWID;
            CREATE TABLE styles (name TEXT PRIMARY KEY, color TEXT, line_weight REAL NOT NULL,
                linetype TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE property_definitions (name TEXT PRIMARY KEY, type TEXT NOT NULL,
                description TEXT NOT NULL, default_value TEXT) WITHOUT ROWID;
            CREATE TABLE entities (id INTEGER PRIMARY KEY, type TEXT NOT NULL,
                layer TEXT NOT NULL REFERENCES layers(name), style TEXT NOT NULL, color TEXT,
                visible INTEGER NOT NULL, geometry TEXT NOT NULL, properties TEXT NOT NULL,
                metadata TEXT NOT NULL);
            INSERT INTO metadata VALUES ('name', '{name}'), ('next_entity_id', '{next}');
            INSERT INTO layers VALUES ('0', '#FFFFFF', 1, 0, 'continuous', 0.25);
            INSERT INTO entities VALUES (5, 'Circle', '0', '', NULL, 1,
                '{{"type":"Circle","center":[1.5,2.5],"radius":4.0}}', '{{}}', '{{}}');
        "#
    ))
    .unwrap();
    v1.set_user_version(1).unwrap();
}

fn main_road() -> Alignment {
    let d = (-1.0_f64).acos() / 6.0; // 30 degrees
    Alignment {
        name: String::from("road"),
        description: String::from("Main road"),
        horizontal: HorizontalAlignment {
            pis: vec![
                AlignmentPi {
                    point: p(0.0, 0.0),
                    ..AlignmentPi::default()
                },
                AlignmentPi {
                    point: p(400.0, 0.0),
                    radius: 300.0,
                    spiral_in: 90.0,
                    spiral_out: 90.0,
                },
                AlignmentPi {
                    point: p(400.0 + 300.0 * d.cos(), 300.0 * d.sin()),
                    ..AlignmentPi::default()
                },
            ],
            start_station: 1000.0,
        },
        vertical: None,
    }
}

fn main_road_with_profile() -> Alignment {
    let mut road = main_road();
    // Stations lie within the road's 1000 to ~1247 chainage: a crest of 60 m
    // at 1120.
    road.vertical = Some(VerticalAlignment {
        pvis: vec![
            ProfilePvi {
                station: 1000.0,
                elevation: 50.0,
                curve_length: 0.0,
            },
            ProfilePvi {
                station: 1120.0,
                elevation: 53.0,
                curve_length: 60.0,
            },
            ProfilePvi {
                station: 1240.0,
                elevation: 51.0,
                curve_length: 0.0,
            },
        ],
    });
    road
}

fn brick() -> HatchPattern {
    HatchPattern {
        name: String::from("brick"),
        description: String::from("Running bond"),
        solid: false,
        // Two families with different angles, spacings and offsets, so that a
        // writer which dropped a column, or a reader which mixed two of them
        // up, could not pass by coincidence.
        families: vec![
            HatchLineFamily {
                angle: 0.0,
                spacing: 0.25,
                offset: 0.0,
            },
            HatchLineFamily {
                angle: 1.25,
                spacing: 0.5,
                offset: 0.125,
            },
        ],
    }
}

fn linetype(name: &str, description: &str, lengths: &[f64]) -> Linetype {
    Linetype {
        name: String::from(name),
        description: String::from(description),
        pattern: lengths
            .iter()
            .map(|&length| LinetypeElement { length })
            .collect(),
    }
}

// The metadata rows as the file holds them, read past the store.
fn metadata_rows(database: &Path) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let db = open(database);
    let mut select = db.prepare("SELECT key, value FROM metadata").unwrap();
    let mut rows = BTreeMap::new();
    while select.step().unwrap() {
        let key = select.column_text(0);
        let value = select.column_text(1);
        rows.entry(key).or_insert(value);
    }
    rows
}

fn entities_of(model: &Model) -> Vec<Entity> {
    let mut entities = Vec::new();
    model.entities.for_each(|e| entities.push(e.clone()));
    entities
}

// ---- SQLite wrapper ------------------------------------------------------------

mod sqlite_wrapper {
    use super::*;

    #[test]
    fn statements_bind_step_and_report_errors() {
        let _scratch = Scratch::new("SqliteWrapper", "StatementsBindStepAndReportErrors");
        let database = SqliteDatabase::open_in_memory().unwrap();
        database
            .execute("CREATE TABLE t (id INTEGER PRIMARY KEY, x REAL, s TEXT)")
            .unwrap();
        assert!(database.table_exists("t").unwrap());
        assert!(!database.table_exists("missing").unwrap());

        let mut insert = database
            .prepare("INSERT INTO t (id, x, s) VALUES (?1, ?2, ?3)")
            .unwrap();
        insert.bind(1, 7_i64).unwrap();
        insert.bind(2, 0.1 + 0.2).unwrap();
        insert.bind(3, "text with 'quotes'").unwrap();
        insert.run().unwrap();

        let mut select = database.prepare("SELECT id, x, s FROM t").unwrap();
        assert!(select.step().unwrap());
        assert_eq!(select.column_int64(0), 7);
        // REAL columns hold doubles exactly.
        assert_eq!(select.column_double(1), 0.1 + 0.2);
        assert_eq!(select.column_text(2), b"text with 'quotes'");
        assert!(!select.step().unwrap());

        let bad = database
            .execute("THIS IS NOT SQL")
            .expect_err("not SQL was executed");
        assert_eq!(bad.code, ErrorCode::DatabaseFailure);
        // It carries SQLite's own message.
        assert!(!bad.context.is_empty());
        assert!(database.prepare("SELECT * FROM nowhere").is_err());

        // Primary key violation surfaces as an error, not a silent no-op.
        insert.bind(1, 7_i64).unwrap();
        insert.bind(2, 1.0).unwrap();
        insert.bind(3, "dup").unwrap();
        assert!(insert.run().is_err());
    }

    #[test]
    fn transaction_rolls_back_unless_committed() {
        let _scratch = Scratch::new("SqliteWrapper", "TransactionRollsBackUnlessCommitted");
        let database = SqliteDatabase::open_in_memory().unwrap();
        database.execute("CREATE TABLE t (id INTEGER)").unwrap();
        let count = || {
            let mut statement = database.prepare("SELECT COUNT(*) FROM t").unwrap();
            assert!(statement.step().unwrap());
            statement.column_int64(0)
        };
        {
            let _transaction = SqliteTransaction::begin(&database).unwrap();
            database.execute("INSERT INTO t VALUES (1)").unwrap();
        } // dropped without commit
        assert_eq!(count(), 0);
        {
            let mut transaction = SqliteTransaction::begin(&database).unwrap();
            database.execute("INSERT INTO t VALUES (1)").unwrap();
            transaction.commit().unwrap();
            assert!(transaction.commit().is_err()); // already finished
        }
        assert_eq!(count(), 1);
    }
}

// ---- lifecycle -------------------------------------------------------------------

mod project_store_lifecycle {
    use super::*;

    #[test]
    fn create_builds_the_project_layout() {
        let scratch = Scratch::new("ProjectStoreLifecycle", "CreateBuildsTheProjectLayout");
        let project = scratch.project_dir();
        let metadata = ProjectMetadata {
            name: String::from("Hilltop Subdivision"),
            ..ProjectMetadata::default()
        };
        let store = ProjectStore::create(&project, &metadata).unwrap();

        for name in [
            "project.db",
            "backups",
            "terrain",
            "pointcloud",
            "assets",
            "cache",
        ] {
            assert!(project.join(name).exists(), "{name}");
        }
        assert!(ProjectStore::is_project_directory(&project));
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );

        let contents = store.load().unwrap();
        assert_eq!(contents.metadata.name, "Hilltop Subdivision");
        assert!(!contents.metadata.created_utc.is_empty());
        assert!(!contents.metadata.modified_utc.is_empty());
        assert!(contents.entities.is_empty());
        assert_eq!(contents.layers.len(), 1);
        assert_eq!(contents.layers[0].name, "0");
    }

    #[test]
    fn create_refuses_to_overwrite_and_open_requires_a_project() {
        let scratch = Scratch::new(
            "ProjectStoreLifecycle",
            "CreateRefusesToOverwriteAndOpenRequiresAProject",
        );
        let project = scratch.project_dir();
        assert!(ProjectStore::create(&project, &ProjectMetadata::default()).is_ok());
        assert_eq!(
            ProjectStore::create(&project, &ProjectMetadata::default())
                .expect_err("a second project was created over the first")
                .code,
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            ProjectStore::open(&scratch.root.join("nothing-here"))
                .expect_err("an empty folder opened")
                .code,
            ErrorCode::NotFound
        );
        assert!(!ProjectStore::is_project_directory(&scratch.root));
    }

    #[test]
    fn rejects_foreign_and_garbage_databases() {
        let scratch = Scratch::new("ProjectStoreLifecycle", "RejectsForeignAndGarbageDatabases");
        let project = scratch.project_dir();
        std::fs::create_dir_all(project.as_path()).unwrap();
        {
            let foreign = open(&project.join("project.db"));
            foreign.execute("CREATE TABLE unrelated (x)").unwrap();
        }
        let not_katana = ProjectStore::open(&project).expect_err("a foreign database opened");
        assert_eq!(not_katana.code, ErrorCode::DatabaseFailure);

        overwrite_with_garbage(&project.join("project.db"));
        assert_eq!(
            ProjectStore::open(&project)
                .expect_err("garbage opened")
                .code,
            ErrorCode::DatabaseFailure
        );
    }
}

// ---- round trip --------------------------------------------------------------------

mod project_store_round_trip {
    use super::*;

    #[test]
    fn model_survives_save_close_open_load_exactly() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "ModelSurvivesSaveCloseOpenLoadExactly",
        );
        let project = scratch.project_dir();
        let original = sample_model();
        // Every field set to something distinctive. Asserting one or two of
        // them would pass just as happily if the others were dropped, swapped
        // or written into each other's columns.
        let metadata = ProjectMetadata {
            name: String::from("Round trip"),
            description: String::from(
                "A description with punctuation: commas, \"quotes\" and a / slash.",
            ),
            linear_unit: String::from("US survey foot"),
            coordinate_system: String::from("EPSG:32630"),
            created_utc: String::from("2019-03-04T05:06:07Z"),
            // save() must replace this.
            modified_utc: String::from("1970-01-01T00:00:00Z"),
            application_version: String::from("katana-test/9.9.9"),
            ..ProjectMetadata::default()
        };
        let relationships = vec![Relationship {
            from: 7,
            to: 1,
            kind: String::from("measures"),
            data: String::from("{\"side\":\"left\"}"),
        }];
        {
            let mut store = ProjectStore::create(&project, &metadata).unwrap();
            store
                .save(&capture_model(
                    &original,
                    metadata.clone(),
                    relationships.clone(),
                ))
                .unwrap();
        } // connection closed

        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened.load().unwrap();

        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        assert_eq!(loaded.layers.all(), original.layers.all());
        assert_eq!(loaded.styles.all(), original.styles.all());
        assert_eq!(loaded.properties.all(), original.properties.all());
        assert_eq!(loaded.entities.ids(), original.entities.ids());
        original.entities.for_each(|entity| {
            let back = loaded.entities.find(entity.id).expect("an entity was lost");
            assert_eq!(back, entity, "entity {}", entity.id); // bit exact
        });
        // The retired id stays retired after a reload.
        assert_eq!(loaded.entities.next_id(), original.entities.next_id());
        assert_eq!(contents.relationships, relationships);

        // Six of the seven metadata fields survive unchanged...
        assert_eq!(contents.metadata.name, metadata.name);
        assert_eq!(contents.metadata.description, metadata.description);
        assert_eq!(contents.metadata.linear_unit, metadata.linear_unit);
        assert_eq!(
            contents.metadata.coordinate_system,
            metadata.coordinate_system
        );
        assert_eq!(contents.metadata.created_utc, metadata.created_utc);
        assert_eq!(
            contents.metadata.application_version,
            metadata.application_version
        );

        // ...and modified_utc is the one save() owns: it must have replaced the
        // sentinel with a real timestamp rather than passing it through.
        assert_ne!(
            contents.metadata.modified_utc, metadata.modified_utc,
            "save() must stamp the modification time, not preserve it"
        );
        assert!(!contents.metadata.modified_utc.is_empty());
        // The writer's form is "%04d-%02d-%02dT%02d:%02d:%02d.%03dZ", so the
        // stamp is 24 characters: YYYY-MM-DDTHH:MM:SS.mmmZ. Checked by shape
        // rather than by length alone, so a reordering would also be caught.
        let text = &contents.metadata.modified_utc;
        let stamp = text.as_bytes();
        assert_eq!(stamp.len(), 24, "{text}");
        assert_eq!(stamp[4], b'-');
        assert_eq!(stamp[7], b'-');
        assert_eq!(stamp[10], b'T');
        assert_eq!(stamp[13], b':');
        assert_eq!(stamp[16], b':');
        assert_eq!(stamp[19], b'.');
        assert_eq!(stamp[23], b'Z');
        for digit in [
            0_usize, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22,
        ] {
            assert!(stamp[digit].is_ascii_digit(), "position {digit} of {text}");
        }

        // The whole struct, once the one field save() owns is accounted for.
        // This is what catches a field added later and never persisted.
        let mut expected = metadata.clone();
        expected.modified_utc = contents.metadata.modified_utc.clone();
        assert_eq!(contents.metadata, expected);
    }

    #[test]
    fn save_is_atomic_when_the_database_rejects_a_row() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "SaveIsAtomicWhenTheDatabaseRejectsARow",
        );
        let project = scratch.project_dir();
        let mut store = create(&project);
        let model = sample_model();
        store.save(&captured(&model)).unwrap();

        // Two layers with the same name violate the primary key half way
        // through the save, after the old rows were deleted inside the
        // transaction.
        let mut broken = captured(&model);
        broken.entities.clear();
        broken.layers.push(Layer {
            name: String::from("Survey"),
            ..Layer::default()
        });
        let status = store
            .save(&broken)
            .expect_err("a duplicate layer was saved");
        assert_eq!(status.code, ErrorCode::DatabaseFailure);

        let contents = store.load().unwrap();
        // The previous save intact.
        assert_eq!(contents.entities.len(), model.entities.size());
    }

    #[test]
    fn invalid_contents_are_never_written_or_applied() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "InvalidContentsAreNeverWrittenOrApplied",
        );
        let project = scratch.project_dir();
        let mut store = create(&project);
        let model = sample_model();

        let mut unknown_layer = captured(&model);
        unknown_layer.entities[0].layer = String::from("Nowhere");
        assert_eq!(
            store
                .save(&unknown_layer)
                .expect_err("an unknown layer was saved")
                .code,
            ErrorCode::InvalidArgument
        );

        let mut duplicate_id = captured(&model);
        duplicate_id.entities[1].id = duplicate_id.entities[0].id;
        assert!(store.save(&duplicate_id).is_err());

        let mut stale_counter = captured(&model);
        stale_counter.next_entity_id = 1;
        assert!(store.save(&stale_counter).is_err());

        let dangling_relationship = capture_model(
            &model,
            ProjectMetadata::default(),
            vec![Relationship {
                from: 1,
                to: 9999,
                kind: String::from("x"),
                data: String::new(),
            }],
        );
        assert!(store.save(&dangling_relationship).is_err());

        let mut target = sample_model();
        let before = target.entities.ids();
        assert!(apply_to_model(&unknown_layer, &mut target).is_err());
        // Rejected before anything was touched.
        assert_eq!(target.entities.ids(), before);
    }

    // ---- regressions: four ways the store used to lose data ----------------------------

    // apply_to_model() used to reset the caller's model before populating it,
    // and reset it again if any insertion failed - so a project that passed
    // validate_contents but tripped a table rule destroyed the drawing that was
    // already open. docs/cad.md documents the opposite, and Document::open
    // relies on it: "a failed open leaves the current drawing exactly as it
    // was".
    #[test]
    fn a_failed_load_leaves_the_callers_model_untouched() {
        let _scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AFailedLoadLeavesTheCallersModelUntouched",
        );
        let mut existing = sample_model();
        let ids_before = existing.entities.ids();
        let layers_before = existing.layers.all().len();
        assert!(!ids_before.is_empty());

        // Contents that pass validate_contents but that the tables reject: a
        // style named twice cannot be added twice.
        let contents = ProjectContents {
            layers: vec![Layer {
                name: String::from(DEFAULT_LAYER_NAME),
                ..Layer::default()
            }],
            styles: vec![
                Style {
                    name: String::from("Duplicate"),
                    ..Style::default()
                },
                Style {
                    name: String::from("Duplicate"),
                    ..Style::default()
                },
            ],
            next_entity_id: 1,
            ..ProjectContents::default()
        };

        let status = apply_to_model(&contents, &mut existing);
        assert!(status.is_err(), "a duplicate style should be refused");

        // The drawing that was open must still be there, in full.
        assert_eq!(existing.entities.ids(), ids_before);
        assert_eq!(existing.layers.all().len(), layers_before);
    }

    // The observer belongs to the model's OWNER, not to the contents being
    // loaded. Committing the staged contents with a plain assignment would
    // carry the staged model's empty observer across and silently stop every
    // change notification the Document depends on.
    #[test]
    fn loading_keeps_the_callers_entity_observer() {
        let _scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "LoadingKeepsTheCallersEntityObserver",
        );
        let mut model = Model::default();
        let events = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&events);
        model
            .entities
            .set_observer(Some(Arc::new(move |_: &ChangeEvent| {
                counter.fetch_add(1, Ordering::SeqCst);
            })));

        let status = apply_to_model(&captured(&sample_model()), &mut model);
        assert!(status.is_ok(), "{status:?}");
        assert!(!model.entities.empty());

        let after_load = events.load(Ordering::SeqCst);
        assert!(
            after_load > 0,
            "the wholesale replacement should report a Cleared event"
        );

        // And the observer must still be live afterwards.
        let point = entity(Geometry::Point(PointGeometry {
            position: p(1.0, 2.0),
        }));
        assert!(model.entities.add(point).is_ok());
        assert!(
            events.load(Ordering::SeqCst) > after_load,
            "the caller's observer was replaced by the loaded model's"
        );
    }

    #[test]
    fn linetype_definitions_survive_save_and_reload() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "LinetypeDefinitionsSurviveSaveAndReload",
        );
        let project = scratch.project_dir();
        let dashed = linetype("dashed", "Dashed  __  __  __", &[1.0, -0.5]);
        // Includes a dot (exactly zero), which is the element a naive "store
        // the sign" encoding loses.
        let dash_dot = linetype("dashdot", "Dash dot  __ . __ .", &[2.0, -0.25, 0.0, -0.25]);

        let mut model = Model::default();
        assert!(model.linetypes.add(dashed.clone()).is_ok());
        assert!(model.linetypes.add(dash_dot.clone()).is_ok());

        let fence = Layer {
            name: String::from("fence"),
            linetype: String::from("dashed"),
            ..Layer::default()
        };
        assert!(model.layers.add(fence).is_ok());

        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }

        let reopened = ProjectStore::open(&project).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let contents = reopened.load().unwrap();

        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        // Element ORDER is the thing a per-row table can get wrong, and
        // reversing it turns a dash-dot into a dot-dash while every length
        // still matches.
        let back = loaded.linetypes.find("dashdot").expect("dashdot was lost");
        assert_eq!(back.description, dash_dot.description);
        assert_eq!(back.pattern.len(), 4);
        for (element, original) in back.pattern.iter().zip(&dash_dot.pattern) {
            double_eq(element.length, original.length);
        }
        assert!(
            back.pattern[2].is_dot(),
            "a zero-length dot must survive as a dot"
        );

        assert_eq!(loaded.linetypes.find("dashed"), Some(&dashed));
        assert!(
            loaded.linetypes.contains("continuous"),
            "the built-in must still be there"
        );
        assert_eq!(loaded.layers.find("fence").unwrap().linetype, "dashed");
    }

    #[test]
    fn dimension_styles_and_the_layers_that_name_them_survive() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "DimensionStylesAndTheLayersThatNameThemSurvive",
        );
        let project = scratch.project_dir();
        let site = DimensionStyle {
            name: String::from("site"),
            text_height: 1.8,
            text_gap: 0.4,
            extension_offset: 0.2,
            extension_beyond: 0.9,
            arrow_size: 1.5,
            arrow_head: ArrowHead::Dot,
            unit_scale: 1000.0,
            prefix: String::from("~"),
            suffix: String::from(" mm"),
            decimals: 1,
            round_to: 0.5,
            suppress_trailing_zeros: true,
            ..DimensionStyle::default()
        };

        let mut model = Model::default();
        assert!(model.dimension_styles.add(site.clone()).is_ok());

        let dims = Layer {
            name: String::from("dims"),
            dimension_style: String::from("site"),
            ..Layer::default()
        };
        assert!(model.layers.add(dims).is_ok());

        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }

        let reopened = ProjectStore::open(&project).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let contents = reopened.load().unwrap();

        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        // The whole struct, so a field added later and never persisted fails
        // here rather than silently reverting to its default on every save.
        assert_eq!(loaded.dimension_styles.find("site"), Some(&site));

        assert_eq!(loaded.layers.find("dims").unwrap().dimension_style, "site");
        assert!(
            loaded.dimension_styles.contains("Standard"),
            "the built-in must still be there"
        );
    }

    #[test]
    fn a_hatch_pattern_survives_save_and_reopen_with_every_family() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AHatchPatternSurvivesSaveAndReopenWithEveryFamily",
        );
        let project = scratch.project_dir();
        let mut model = sample_model();

        let brick = brick();
        assert!(model.hatch_patterns.add(brick.clone()).is_ok());

        let solid = HatchPattern {
            name: String::from("concrete"),
            solid: true,
            ..HatchPattern::default()
        };
        assert!(model.hatch_patterns.add(solid).is_ok());

        let hatched = Layer {
            name: String::from("paving"),
            hatch_pattern: String::from("brick"),
            ..Layer::default()
        };
        assert!(model.layers.add(hatched).is_ok());

        let style = Style {
            name: String::from("slab"),
            hatch_pattern: String::from("concrete"),
            ..Style::default()
        };
        assert!(model.styles.add(style).is_ok());

        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
            // Saved twice: save clears and rewrites every table it owns, so a
            // second save is what catches a missing DELETE and its primary-key
            // conflict. That is exactly how the linetype and dimension style
            // tables each failed when they were added.
            store.save(&captured(&model)).unwrap();
        }

        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        assert_eq!(
            loaded.hatch_patterns.find("brick"),
            Some(&brick),
            "a field was lost or reordered on the way through SQLite"
        );

        let reloaded_solid = loaded
            .hatch_patterns
            .find("concrete")
            .expect("concrete was lost");
        assert!(reloaded_solid.solid);
        assert!(reloaded_solid.families.is_empty());

        assert_eq!(
            loaded
                .layers
                .find("paving")
                .expect("paving was lost")
                .hatch_pattern,
            "brick"
        );
        assert_eq!(
            loaded
                .styles
                .find("slab")
                .expect("slab was lost")
                .hatch_pattern,
            "concrete"
        );
    }

    // ---- alignments (schema 7) ---------------------------------------------------------

    #[test]
    fn an_alignment_survives_save_and_reopen_with_every_pi() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AnAlignmentSurvivesSaveAndReopenWithEveryPI",
        );
        let project = scratch.project_dir();
        let mut model = sample_model();
        let road = main_road();
        assert!(model.alignments.add(road.clone()).is_ok());
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
            // Twice: save clears and rewrites every table it owns, and a
            // missing DELETE only shows as a primary-key conflict on the
            // second save.
            store.save(&captured(&model)).unwrap();
        }
        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        let reloaded = loaded
            .alignments
            .find("road")
            .expect("dropped somewhere between capture and adopt_contents");
        // Exact equality: SQLite REAL is a double, so every coordinate,
        // radius, spiral length and the start station must come back bit for
        // bit.
        assert_eq!(reloaded, &road);
    }

    #[test]
    fn a_file_holding_an_alignment_that_cannot_be_built_is_refused_naming_the_pi() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AFileHoldingAnAlignmentThatCannotBeBuiltIsRefusedNamingThePI",
        );
        let project = scratch.project_dir();
        // Nothing in the writer can produce this - add() refuses it - so it
        // can only arrive by a hand edit, another tool, or a future bug.
        // Whichever it is, opening the file must say so rather than draw the
        // alignment wrong (PLAN.MD section 36).
        let mut model = sample_model();
        assert!(model.alignments.add(main_road()).is_ok());
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let database = open(&project.join("project.db"));
            // 500 m spirals on R = 300 each use 0.833 rad; the corner has
            // 0.524.
            database
                .execute(concat!(
                    "UPDATE alignment_pis SET spiral_in = 500, spiral_out = 500",
                    " WHERE alignment = 'road' AND position = 1"
                ))
                .unwrap();
        }
        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened
            .load()
            .expect("reading rows is fine; building them is what must fail");
        let mut loaded = Model::default();
        let applied = apply_to_model(&contents, &mut loaded)
            .expect_err("an alignment that cannot be built was applied");
        assert_eq!(applied.code, ErrorCode::InvalidGeometry);
        assert!(
            applied.describe().contains("PI 1"),
            "{}",
            applied.describe()
        );
        assert_eq!(
            loaded.alignments.size(),
            0,
            "a refused load must leave the model untouched"
        );
    }

    // ---- design profiles (schema 8) ----------------------------------------------------

    #[test]
    fn a_profile_on_an_alignment_survives_save_and_reopen() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AProfileOnAnAlignmentSurvivesSaveAndReopen",
        );
        let project = scratch.project_dir();
        let mut model = sample_model();
        let road = main_road_with_profile();
        assert!(model.alignments.add(road.clone()).is_ok());
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
            store.save(&captured(&model)).unwrap(); // the second-save check
        }
        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        let reloaded = loaded.alignments.find("road").expect("road was lost");
        assert!(reloaded.vertical.is_some());
        assert_eq!(reloaded, &road);
    }

    #[test]
    fn an_alignment_without_a_profile_comes_back_without_one() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AnAlignmentWithoutAProfileComesBackWithoutOne",
        );
        let project = scratch.project_dir();
        // The reader creates the profile on its first row. An alignment with
        // no rows must NOT come back with an empty profile, which would then
        // fail to solve and refuse the whole load.
        let mut model = sample_model();
        assert!(model.alignments.add(main_road()).is_ok());
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        assert!(
            loaded
                .alignments
                .find("road")
                .expect("road was lost")
                .vertical
                .is_none()
        );
    }

    // ---- the consuming function is the same function -----------------------------------

    // apply_to_model has a twin that consumes the contents, which is what
    // Document::open uses so that opening a drawing does not duplicate every
    // entity on its way into the model. They must be the SAME function, so this
    // builds a project with something in EVERY table apply_to_model fills -
    // including the four whose built-in names take the update() path that
    // cannot move - applies one copy of it each way, and compares the two
    // models value for value.
    //
    // A weaker test would compare only the entity ids: this compares whole
    // values, because the failure a move introduces is a field left behind,
    // not a record.
    #[test]
    fn consuming_the_contents_builds_exactly_the_model_copying_them_does() {
        let _scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "ConsumingTheContentsBuildsExactlyTheModelCopyingThemDoes",
        );
        let mut rich = sample_model();

        let dashed = linetype("dashed", "Dashed  __  __  __", &[0.5, -0.25, 0.0, -0.25]);
        assert!(rich.linetypes.add(dashed).is_ok());

        let metric = DimensionStyle {
            name: String::from("metric"),
            suffix: String::from(" m"),
            decimals: 2,
            ..DimensionStyle::default()
        };
        assert!(rich.dimension_styles.add(metric).is_ok());

        assert!(rich.hatch_patterns.add(brick()).is_ok());

        assert!(rich.alignments.add(main_road_with_profile()).is_ok());

        // capture_model carries the built-in names too - layer "0",
        // "continuous", "none" and the default dimension style - so the
        // update() path, the one that cannot move, is exercised as well.
        let contents = captured(&rich);
        assert!(!contents.entities.is_empty());
        assert!(!contents.alignments.is_empty());
        assert!(!contents.property_definitions.is_empty());
        let duplicate = contents.clone(); // the identical input, kept whole

        let mut copied = Model::default();
        assert!(apply_to_model(&duplicate, &mut copied).is_ok());
        let mut moved = Model::default();
        assert!(apply_to_model_consuming(contents, &mut moved).is_ok());

        // Every table apply_to_model fills. Each of these types derives
        // PartialEq, so this compares all of their fields, not their names.
        assert_eq!(copied.layers.all(), moved.layers.all());
        assert_eq!(copied.styles.all(), moved.styles.all());
        assert_eq!(copied.linetypes.all(), moved.linetypes.all());
        assert_eq!(copied.dimension_styles.all(), moved.dimension_styles.all());
        assert_eq!(copied.hatch_patterns.all(), moved.hatch_patterns.all());
        assert_eq!(copied.alignments.all(), moved.alignments.all());
        assert_eq!(copied.properties.all(), moved.properties.all());

        let ids = copied.entities.ids();
        assert_eq!(ids, moved.entities.ids());
        assert!(!ids.is_empty());
        for &id in &ids {
            let left = copied
                .entities
                .find(id)
                .expect("entity missing on the left");
            let right = moved
                .entities
                .find(id)
                .expect("entity missing on the right");
            assert_eq!(left, right, "entity {id} differs between the two functions");
        }
        // The retired id in sample_model() makes this more than "count + 1".
        assert_eq!(copied.entities.next_id(), moved.entities.next_id());
    }

    // The consuming function leaves the caller's model untouched when it
    // fails, exactly as the copying one does - Document::open relies on that
    // and only then installs the metadata it took out beforehand.
    #[test]
    fn a_failed_consuming_load_also_leaves_the_callers_model_untouched() {
        let _scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AFailedConsumingLoadAlsoLeavesTheCallersModelUntouched",
        );
        let mut existing = sample_model();
        let ids_before = existing.entities.ids();
        let layers_before = existing.layers.all();
        assert!(!ids_before.is_empty());

        let contents = ProjectContents {
            layers: vec![Layer {
                name: String::from(DEFAULT_LAYER_NAME),
                ..Layer::default()
            }],
            styles: vec![
                Style {
                    name: String::from("Duplicate"),
                    ..Style::default()
                },
                Style {
                    name: String::from("Duplicate"),
                    ..Style::default()
                },
            ],
            next_entity_id: 1,
            ..ProjectContents::default()
        };

        let status = apply_to_model_consuming(contents, &mut existing);
        assert!(status.is_err(), "a duplicate style should be refused");

        assert_eq!(existing.entities.ids(), ids_before);
        assert_eq!(existing.layers.all(), layers_before);
    }

    // ---- metadata ----------------------------------------------------------------------

    #[test]
    fn a_metadata_key_a_newer_build_wrote_survives_open_save_save() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AMetadataKeyANewerBuildWroteSurvivesOpenSaveSave",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        let metadata = ProjectMetadata {
            name: String::from("Forward"),
            ..ProjectMetadata::default()
        };
        {
            let mut store = ProjectStore::create(&project, &metadata).unwrap();
            store
                .save(&capture_model(&model, metadata.clone(), Vec::new()))
                .unwrap();
        }
        // What a newer Katana would have written beside the keys this one
        // knows.
        {
            let db = open(&project.join("project.db"));
            db.execute("INSERT INTO metadata VALUES ('symbol_rotation_mode', 'north')")
                .unwrap();
        }

        let newer = BTreeMap::from([(String::from("symbol_rotation_mode"), String::from("north"))]);
        let mut store = ProjectStore::open(&project).unwrap();
        let opened = store.load().unwrap();
        assert_eq!(
            opened.metadata.unknown_keys, newer,
            "read, and kept to be written back"
        );
        // Saved twice, as a session does: the second save is the one a table
        // that is not emptied first would fail on.
        store
            .save(&capture_model(&model, opened.metadata.clone(), Vec::new()))
            .unwrap();
        store
            .save(&capture_model(&model, opened.metadata.clone(), Vec::new()))
            .unwrap();
        // And saved by a caller that never looked at the metadata it loaded:
        // the key is still not this build's to delete.
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();

        let again = store.load().unwrap();
        assert_eq!(again.metadata.unknown_keys, newer);
        assert_eq!(again.metadata.name, "Forward");
        // 10 keys this build writes (reference_layers the tenth) + the 1 it
        // does not know = 11 rows, each once.
        assert_eq!(metadata_rows(&project.join("project.db")).len(), 11);
    }

    // The regression: a sheet set emptied (the drawing stops writing `sheets`)
    // came back from the previous save when the project was opened again,
    // because a save only REPLACEd rows into the table. A key of the drawing's
    // own that it no longer writes is deleted; a newer build's is still kept.
    #[test]
    fn a_key_of_the_drawings_own_that_it_stops_writing_is_gone_after_the_next_save() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AKeyOfTheDrawingsOwnThatItStopsWritingIsGoneAfterTheNextSave",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        let mut metadata = ProjectMetadata {
            name: String::from("Removed"),
            unknown_keys: BTreeMap::from([
                (
                    String::from("sheets"),
                    String::from("{\"version\":1,\"sheets\":[]}"),
                ),
                (
                    String::from("road_objects"),
                    String::from("{\"version\":1}"),
                ),
                (String::from("symbol_rotation_mode"), String::from("north")),
            ]),
            ..ProjectMetadata::default()
        };
        let mut store = ProjectStore::create(&project, &metadata).unwrap();
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();

        metadata.unknown_keys.remove("sheets");
        metadata.unknown_keys.remove("road_objects");
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();

        let again = store.load().unwrap();
        assert_eq!(
            again.metadata.unknown_keys,
            BTreeMap::from([(String::from("symbol_rotation_mode"), String::from("north"))]),
            "sheets and road_objects were removed; the newer build's key was not"
        );
        // The 10 keys this build writes + the newer build's 1.
        assert_eq!(metadata_rows(&project.join("project.db")).len(), 11);
    }

    #[test]
    fn the_reference_layers_a_project_records_round_trip_as_they_were() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "TheReferenceLayersAProjectRecordsRoundTripAsTheyWere",
        );
        let project = scratch.project_dir();
        // Opaque to storage: the records come back exactly, in their order.
        let model = sample_model();
        let mut metadata = ProjectMetadata {
            reference_layers: vec![
                String::from(r#"{"version":1,"kind":"raster","name":"ortho ÿ"}"#),
                String::from(r#"{"version":1,"kind":"pointcloud","name":"scan"}"#),
            ],
            ..ProjectMetadata::default()
        };
        let mut store = ProjectStore::create(&project, &metadata).unwrap();
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();
        let contents = store.load().unwrap();
        assert_eq!(
            contents.metadata.reference_layers,
            metadata.reference_layers
        );
        assert!(
            contents.metadata.unknown_keys.is_empty(),
            "reference_layers is this build's key"
        );

        metadata.reference_layers.clear();
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();
        assert!(
            store.load().unwrap().metadata.reference_layers.is_empty(),
            "none, not one empty record"
        );

        // A record that would not read back as itself is refused.
        for record in ["", "two\nrecords"] {
            let refused = ProjectMetadata {
                reference_layers: vec![String::from(record)],
                ..ProjectMetadata::default()
            };
            let status = store
                .save(&capture_model(&model, refused, Vec::new()))
                .expect_err(record);
            assert_eq!(status.code, ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn a_project_from_before_reference_layers_were_recorded_opens_with_none() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "AProjectFromBeforeReferenceLayersWereRecordedOpensWithNone",
        );
        let project = scratch.project_dir();
        // Written before the key existed: no reference_layers row at all. It
        // opens as it was, at the same schema - the records needed no
        // migration - and its next save writes the key.
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let db = open(&project.join("project.db"));
            db.execute("DELETE FROM metadata WHERE key = 'reference_layers'")
                .unwrap();
        }
        let store = ProjectStore::open(&project).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        assert!(
            store.list_backups().is_empty(),
            "nothing to migrate, so nothing backed up"
        );
        let contents = store.load().unwrap();
        assert!(contents.metadata.reference_layers.is_empty());
        assert!(contents.metadata.unknown_keys.is_empty());
    }

    #[test]
    fn the_customisation_a_project_was_drawn_with_round_trips() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "TheCustomisationAProjectWasDrawnWithRoundTrips",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        // A name may hold anything a file name can - a semicolon, spaces,
        // non-ASCII - and the order is the load order, not name order.
        let mut metadata = ProjectMetadata {
            customisation: vec![
                String::from("user_symbols_ÿ.4d"),
                String::from("linestyles; v2.4d"),
                String::from("survey codes.mapfile"),
            ],
            ..ProjectMetadata::default()
        };
        let mut store = ProjectStore::create(&project, &metadata).unwrap();
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();
        let contents = store.load().unwrap();
        assert_eq!(contents.metadata.customisation, metadata.customisation);
        assert!(
            contents.metadata.unknown_keys.is_empty(),
            "customisation is a key this build knows"
        );

        metadata.customisation.clear();
        store
            .save(&capture_model(&model, metadata.clone(), Vec::new()))
            .unwrap();
        assert!(
            store.load().unwrap().metadata.customisation.is_empty(),
            "an empty list, not one empty name"
        );
    }

    #[test]
    fn metadata_that_would_not_read_back_as_it_was_is_refused() {
        let scratch = Scratch::new(
            "ProjectStoreRoundTrip",
            "MetadataThatWouldNotReadBackAsItWasIsRefused",
        );
        let project = scratch.project_dir();
        let mut store = create(&project);
        let model = sample_model();
        let mut refused = |metadata: ProjectMetadata| {
            store
                .save(&capture_model(&model, metadata, Vec::new()))
                .is_err_and(|error| error.code == ErrorCode::InvalidArgument)
        };
        for name in [
            "",
            "C:/Customisation/user.4d",
            "folder\\user.4d",
            "two\nnames.4d",
        ] {
            let metadata = ProjectMetadata {
                customisation: vec![String::from(name)],
                ..ProjectMetadata::default()
            };
            assert!(refused(metadata), "\"{name}\"");
        }
        let shadowing = ProjectMetadata {
            unknown_keys: BTreeMap::from([(String::from("name"), String::from("Not the name"))]),
            ..ProjectMetadata::default()
        };
        assert!(
            refused(shadowing),
            "an unknown key may not overwrite a field"
        );
    }
}

// ---- migration -----------------------------------------------------------------------

mod project_store_migration {
    use super::*;

    #[test]
    fn upgrades_a_version1_project_and_backs_it_up_first() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "UpgradesAVersion1ProjectAndBacksItUpFirst",
        );
        let project = scratch.project_dir();
        write_version1_project(&project, "Legacy site", "6");

        let store = ProjectStore::open(&project).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        // Taken before the schema was touched.
        assert_eq!(store.list_backups().len(), 1);

        let contents = store.load().unwrap();
        assert_eq!(contents.metadata.name, "Legacy site");
        assert_eq!(contents.entities.len(), 1);
        assert_eq!(contents.entities[0].id, 5);
        assert_eq!(
            contents.entities[0].geometry,
            Geometry::Circle(Circle2::new(p(1.5, 2.5), 4.0))
        );
        assert_eq!(contents.next_entity_id, 6);

        // The pre-migration backup is still a version 1 database.
        let backup = SqliteDatabase::open(&store.list_backups()[0], OpenMode::ReadOnly).unwrap();
        assert_eq!(backup.user_version().unwrap(), 1);
        assert!(!backup.table_exists("relationships").unwrap());
    }

    #[test]
    fn refuses_projects_from_a_newer_katana() {
        let scratch = Scratch::new("ProjectStoreMigration", "RefusesProjectsFromANewerKatana");
        let project = scratch.project_dir();
        assert!(ProjectStore::create(&project, &ProjectMetadata::default()).is_ok());
        {
            let raw = open(&project.join("project.db"));
            raw.set_user_version(ProjectStore::CURRENT_SCHEMA_VERSION + 1)
                .unwrap();
        }
        let opened = ProjectStore::open(&project).expect_err("a newer project opened");
        assert_eq!(opened.code, ErrorCode::Unsupported);
    }

    // Schema 12 (docs/model.md): the drawing system's geometry kinds - curve
    // polyline 9, ellipse 10, spline 11 - with no table change of their own.

    #[test]
    fn a_schema_eleven_project_opens_as_it_was_and_is_raised_to_the_current_schema() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "ASchemaElevenProjectOpensAsItWasAndIsRaisedToTheCurrentSchema",
        );
        let project = scratch.project_dir();
        let mut model = Model::default();
        let line = entity(Geometry::Line(Segment2::new(p(1.0, 2.0), p(3.0, 4.0))));
        let id = model.entities.add(line.clone()).unwrap();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            // Schema 11 as the build before 12 left it: 12 changed no table,
            // so the version is the only difference - and 13's surfaces table
            // goes too, which a schema-11 file never had.
            let raw = open(&project.join("project.db"));
            rewind_schema_to(&raw, 11).unwrap();
        }
        let store = ProjectStore::open(&project).unwrap();
        // Raised through 12 and 13, every migration after the file's.
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let contents = store.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        assert_eq!(
            loaded
                .entities
                .find(id)
                .expect("the line was lost")
                .geometry,
            line.geometry
        );
        // Migrated only with a way back, as every migration is.
        assert!(
            std::fs::read_dir(project.join("backups"))
                .unwrap()
                .next()
                .is_some()
        );
    }

    #[test]
    fn a_project_holding_the_drawing_kinds_is_written_as_schema_twelve() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "AProjectHoldingTheDrawingKindsIsWrittenAsSchemaTwelve",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        let mut curve = false;
        let mut ellipse = false;
        let mut spline = false;
        model.entities.for_each(|entity| {
            curve = curve || matches!(entity.geometry, Geometry::CurvePolyline(_));
            ellipse = ellipse || matches!(entity.geometry, Geometry::Ellipse(_));
            spline = spline || matches!(entity.geometry, Geometry::Spline(_));
        });
        assert!(
            curve && ellipse && spline,
            "the sample no longer holds the three kinds"
        );
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            // Read off the file, not through the store, which could agree with
            // itself about a wrong number.
            let raw = open(&project.join("project.db"));
            let version = raw.user_version().unwrap();
            // 12 is the first schema with the kinds, which a schema-11 build
            // refuses; a file is written at the current one, 13 since the
            // surfaces (docs/model.md).
            assert!(version >= 12);
            assert_eq!(version, 13);
        }
        let store = ProjectStore::open(&project).unwrap();
        let contents = store.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        assert_eq!(entities_of(&loaded), entities_of(&model));
    }

    #[test]
    fn a_build_that_cannot_read_the_project_says_it_is_newer_and_which_schemas() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "ABuildThatCannotReadTheProjectSaysItIsNewerAndWhichSchemas",
        );
        let project = scratch.project_dir();
        // What a schema-11 build does with a schema-12 project, by the same
        // check one schema up: refused before any row is read, as version
        // skew, naming both numbers - not "unknown geometry kind in blob" per
        // entity. One up from 13, the current schema since the surfaces
        // (docs/model.md).
        assert!(ProjectStore::create(&project, &ProjectMetadata::default()).is_ok());
        {
            let raw = open(&project.join("project.db"));
            raw.set_user_version(14).unwrap();
        }
        let opened = ProjectStore::open(&project).expect_err("a newer project opened");
        assert_eq!(opened.code, ErrorCode::Unsupported);
        assert!(
            opened.message.contains("newer version of Katana"),
            "{}",
            opened.describe()
        );
        assert_eq!(opened.context, "schema=14 supported=13");
    }

    #[test]
    fn saving_a_migrated_project_converts_its_geometry_to_blobs() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "SavingAMigratedProjectConvertsItsGeometryToBlobs",
        );
        let project = scratch.project_dir();
        // A schema-1 project keeps its JSON until it is next saved; after that
        // the blob is the record. Both readings must give the same geometry,
        // or the act of saving would silently change a drawing.
        write_version1_project(&project, "Converted", "9");

        let expected = Geometry::Circle(Circle2::new(p(1.5, 2.5), 4.0));

        // Open (migrates the schema, still reads JSON) and save (writes blobs).
        {
            let mut store = ProjectStore::open(&project).unwrap();
            let contents = store.load().unwrap();
            assert_eq!(contents.entities.len(), 1);
            assert_eq!(contents.entities[0].geometry, expected);
            store.save(&contents).unwrap();
        }

        // The blob is now populated and the legacy column emptied.
        {
            let database =
                SqliteDatabase::open(&project.join("project.db"), OpenMode::ReadOnly).unwrap();
            let mut row = database
                .prepare("SELECT geometry, geometry_blob FROM entities WHERE id = 5")
                .unwrap();
            assert!(row.step().unwrap());
            assert!(
                row.column_text(0).is_empty(),
                "the JSON column is no longer written"
            );
            assert!(!row.column_is_null(1), "the blob must have been written");
            assert!(row.column_blob(1).len() >= 2);
        }

        // And it still reads back as the same circle.
        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened.load().unwrap();
        assert_eq!(contents.entities.len(), 1);
        assert_eq!(contents.entities[0].geometry, expected);
        assert_eq!(contents.metadata.name, "Converted");
    }

    #[test]
    fn where_both_encodings_are_present_the_blob_wins() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "WhereBothEncodingsArePresentTheBlobWins",
        );
        let project = scratch.project_dir();
        // A half-migrated row - JSON from before, blob from a later save -
        // must read the blob, which is the newer of the two. The JSON here is
        // deliberately a DIFFERENT circle, so a reader taking the wrong column
        // fails loudly instead of happening to agree.
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let database = open(&project.join("project.db"));
            database
                .execute(concat!(
                    "UPDATE entities SET geometry = ",
                    "'{\"type\":\"Circle\",\"center\":[999.0,999.0],\"radius\":1.0}'"
                ))
                .unwrap();
        }

        let reopened = ProjectStore::open(&project).unwrap();
        let contents = reopened.load().unwrap();

        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        model.entities.for_each(|entity| {
            let back = loaded.entities.find(entity.id).expect("an entity was lost");
            assert_eq!(
                back.geometry, entity.geometry,
                "entity {} was read from the stale JSON column",
                entity.id
            );
        });
    }

    #[test]
    fn a_project_from_before_linetypes_opens_with_its_layer_patterns_intact() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "AProjectFromBeforeLinetypesOpensWithItsLayerPatternsIntact",
        );
        let project = scratch.project_dir();
        // Layer::linetype has always been a free string with no table behind
        // it, so an older project can name a pattern that has no definition.
        // Opening it must keep the name - it is what the layer says - and
        // resolve to solid, which is exactly how it already drew.
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let database = open(&project.join("project.db"));
            database
                .execute("UPDATE layers SET linetype = 'hidden'")
                .unwrap();
            rewind_schema_to(&database, 3).unwrap();
        }

        let reopened = ProjectStore::open(&project).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );

        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        assert_eq!(
            loaded.layers.find("0").unwrap().linetype,
            "hidden",
            "the name the layer carries must be kept, not rewritten to continuous"
        );
        assert!(
            !loaded.linetypes.contains("hidden"),
            "no empty definition may be invented, or it would shadow a real one later"
        );
        assert!(loaded.linetypes.contains("continuous"));
    }

    #[test]
    fn a_project_from_before_dimension_styles_gets_the_default_and_keeps_working() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "AProjectFromBeforeDimensionStylesGetsTheDefaultAndKeepsWorking",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let database = open(&project.join("project.db"));
            rewind_schema_to(&database, 4).unwrap();
        }

        let reopened = ProjectStore::open(&project).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );

        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        // Every layer names no style, which means the document default - the
        // same behaviour the project already had, rather than an invented
        // style.
        for layer in loaded.layers.all() {
            assert!(layer.dimension_style.is_empty(), "{}", layer.name);
        }
        assert!(loaded.dimension_styles.contains("Standard"));
        assert_eq!(loaded.dimension_styles.size(), 1);
    }

    #[test]
    fn a_styles_description_and_symbol_survive_saving_twice_and_an_old_project_has_none() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "AStylesDescriptionAndSymbolSurviveSavingTwiceAndAnOldProjectHasNone",
        );
        let project = scratch.project_dir();
        let mut model = sample_model();
        let marked = Style {
            name: String::from("TOPO Natural Surface Point"),
            description: String::from("colour: shade 48"),
            symbol: String::from("cross"),
            symbol_size: 1.5,
            ..Style::default()
        };
        assert!(model.styles.add(marked.clone()).is_ok());
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
            store
                .save(&captured(&model))
                .expect("the second save is the one that finds a missing DELETE");
        }
        {
            let reopened = ProjectStore::open(&project).unwrap();
            let contents = reopened.load().unwrap();
            let mut loaded = Model::default();
            assert!(apply_to_model(&contents, &mut loaded).is_ok());
            assert_eq!(
                loaded.styles.find(&marked.name),
                Some(&marked),
                "a field was lost or reordered on the way through SQLite"
            );
        }

        // A project from before schema 9: every style comes back with no
        // description, no symbol and the default mark size - as it was.
        {
            let database = open(&project.join("project.db"));
            rewind_schema_to(&database, 8).unwrap();
        }
        let old = ProjectStore::open(&project).unwrap();
        assert_eq!(
            old.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let contents = old.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        let plain = loaded
            .styles
            .find(&marked.name)
            .expect("the style was lost");
        assert!(plain.description.is_empty());
        assert_eq!(plain.symbol, NO_SYMBOL);
        assert_eq!(plain.symbol_size, 0.0);
        assert_eq!(
            plain.line_weight, marked.line_weight,
            "the older columns are untouched"
        );
    }

    #[test]
    fn a_project_from_before_hatch_patterns_opens_unhatched() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "AProjectFromBeforeHatchPatternsOpensUnhatched",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let database = open(&project.join("project.db"));
            rewind_schema_to(&database, 5).unwrap();
        }

        let reopened = ProjectStore::open(&project).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );

        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());

        // The point of the migration's default: a drawing made before hatching
        // existed must look exactly as it did, so every layer comes back
        // naming the built-in that draws no fill, and no other pattern is
        // invented.
        for layer in loaded.layers.all() {
            assert_eq!(layer.hatch_pattern, NO_HATCH, "{}", layer.name);
        }
        for style in loaded.styles.all() {
            assert!(style.hatch_pattern.is_empty(), "{}", style.name); // ByLayer
        }
        assert!(loaded.hatch_patterns.contains(NO_HATCH));
        assert_eq!(loaded.hatch_patterns.size(), 1);
    }

    #[test]
    fn a_project_from_before_alignments_opens_with_none() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "AProjectFromBeforeAlignmentsOpensWithNone",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let database = open(&project.join("project.db"));
            rewind_schema_to(&database, 6).unwrap();
        }
        let reopened = ProjectStore::open(&project).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        assert_eq!(
            loaded.alignments.size(),
            0,
            "nothing should be invented for an old file"
        );
    }

    #[test]
    fn a_project_from_before_profiles_keeps_its_alignments_unprofiled() {
        let scratch = Scratch::new(
            "ProjectStoreMigration",
            "AProjectFromBeforeProfilesKeepsItsAlignmentsUnprofiled",
        );
        let project = scratch.project_dir();
        let mut model = sample_model();
        assert!(model.alignments.add(main_road_with_profile()).is_ok());
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }
        {
            let database = open(&project.join("project.db"));
            rewind_schema_to(&database, 7).unwrap(); // drops the profile table
        }
        let reopened = ProjectStore::open(&project).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let contents = reopened.load().unwrap();
        let mut loaded = Model::default();
        assert!(apply_to_model(&contents, &mut loaded).is_ok());
        let road = loaded
            .alignments
            .find("road")
            .expect("the alignment itself predates the migration and must survive");
        assert!(road.vertical.is_none());
    }
}

// ---- backup & recovery --------------------------------------------------------------------

mod project_store_recovery {
    use super::*;

    #[test]
    fn backups_are_pruned_to_the_newest() {
        let scratch = Scratch::new("ProjectStoreRecovery", "BackupsArePrunedToTheNewest");
        let project = scratch.project_dir();
        let mut store = create(&project);

        // Every path in creation order, so the test knows which two SHOULD
        // survive rather than only checking that two did. Four backups taken
        // back to back land in the same millisecond, so this also exercises
        // the collision suffix and the ordering that has to parse it.
        let mut made: Vec<PathBuf> = Vec::new();
        for i in 0..4 {
            let path = store.backup(2).unwrap();
            assert!(path.exists(), "backup {i} was not written");
            made.push(path);
        }
        assert_eq!(
            made.iter().collect::<BTreeSet<_>>().len(),
            made.len(),
            "each backup must get its own file, even within one millisecond"
        );

        let backups = store.list_backups();
        assert_eq!(backups.len(), 2);
        // BOTH survivors are identified, newest first: asserting only the
        // first would pass with any arbitrary second file.
        assert_eq!(backups[0], made[3]);
        assert_eq!(backups[1], made[2]);

        // The pruned ones are gone from DISK, not merely absent from the
        // listing - a prune that only stopped listing them would leak a file
        // per save.
        assert!(
            !made[0].exists(),
            "the oldest backup should have been deleted"
        );
        assert!(!made[1].exists());
        assert!(made[2].exists());
        assert!(made[3].exists());

        // And what survived is usable. A prune that kept a truncated file
        // would satisfy every assertion above and still lose the project.
        for kept in &backups {
            let database = SqliteDatabase::open(kept, OpenMode::ReadOnly)
                .unwrap_or_else(|error| panic!("{}: {}", kept.display(), error.describe()));
            assert_eq!(
                database.user_version().unwrap(),
                ProjectStore::CURRENT_SCHEMA_VERSION
            );
        }
    }

    #[test]
    fn the_keep_count_is_honoured_at_its_edges() {
        let scratch = Scratch::new("ProjectStoreRecovery", "TheKeepCountIsHonouredAtItsEdges");
        let project = scratch.project_dir();
        let mut store = create(&project);

        // keep larger than the number taken: nothing is pruned.
        for _ in 0..3 {
            assert!(store.backup(10).is_ok());
        }
        assert_eq!(store.list_backups().len(), 3);

        // keep = 1 collapses to the newest, which must be the one just made.
        let newest = store.backup(1).unwrap();
        let one = store.list_backups();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0], newest);
        assert!(newest.exists());

        // keep = 0 removes even the backup just taken. That is a strange thing
        // to ask for, but it must do what it says rather than quietly keep
        // one.
        let discarded = store.backup(0).unwrap();
        assert!(store.list_backups().is_empty());
        assert!(!discarded.exists());
    }

    #[test]
    fn restores_the_newest_sound_backup_and_keeps_the_damaged_file() {
        let scratch = Scratch::new(
            "ProjectStoreRecovery",
            "RestoresTheNewestSoundBackupAndKeepsTheDamagedFile",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
            // Older, sound.
            assert!(store.backup(ProjectStore::DEFAULT_BACKUPS_TO_KEEP).is_ok());
            let newer = store.backup(ProjectStore::DEFAULT_BACKUPS_TO_KEEP).unwrap();
            overwrite_with_garbage(&newer); // the newest backup is damaged as well
        }
        overwrite_with_garbage(&project.join("project.db"));
        assert!(ProjectStore::open(&project).is_err());

        let report = ProjectStore::recover(&project).unwrap();
        assert!(report.restored);
        assert!(!report.problems.is_empty());
        // Evidence is never deleted.
        assert!(report.damaged_file_kept_as.exists());

        let store = ProjectStore::open(&project).unwrap();
        let contents = store.load().unwrap();
        assert_eq!(contents.entities.len(), model.entities.size());
    }

    #[test]
    fn sound_projects_are_left_alone_and_missing_backups_are_reported() {
        let scratch = Scratch::new(
            "ProjectStoreRecovery",
            "SoundProjectsAreLeftAloneAndMissingBackupsAreReported",
        );
        let project = scratch.project_dir();
        assert!(ProjectStore::create(&project, &ProjectMetadata::default()).is_ok());
        let sound = ProjectStore::recover(&project).unwrap();
        assert!(!sound.restored);

        overwrite_with_garbage(&project.join("project.db"));
        let hopeless = ProjectStore::recover(&project).expect_err("a hopeless project recovered");
        assert_eq!(hopeless.code, ErrorCode::DatabaseFailure);
        // Untouched when nothing can replace it.
        assert!(project.join("project.db").exists());
    }

    // A database whose process was killed mid-commit has a hot rollback
    // journal beside it. SQLite replays that journal on the next WRITABLE
    // open, after which the file is sound. Inspecting read-only cannot replay
    // it and reports SQLITE_READONLY_ROLLBACK, which used to be taken for
    // corruption - so recover() threw away a perfectly recoverable database
    // for an older backup.
    #[test]
    fn a_crash_interrupted_database_is_not_mistaken_for_damage() {
        let scratch = Scratch::new(
            "ProjectStoreRecovery",
            "ACrashInterruptedDatabaseIsNotMistakenForDamage",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
        }

        // Simulate the crash: a leftover journal beside an otherwise intact
        // file. Its header is deliberately not a valid journal, which is
        // exactly what a torn write leaves behind; SQLite discards such a
        // journal on open.
        let journal = project.join("project.db-journal");
        std::fs::write(journal.as_path(), [0_u8; 512]).unwrap();
        assert!(journal.exists());

        // The store must open, and the drawing must still be all there.
        let reopened = ProjectStore::open(&project).unwrap_or_else(|error| {
            panic!(
                "a hot journal was taken for corruption: {}",
                error.describe()
            )
        });
        let loaded = reopened.load().unwrap();
        assert_eq!(loaded.entities.len(), model.entities.size());
    }

    // recover() must move the damaged file's rollback journal aside, or SQLite
    // replays it over the restored data on the next open. When project.db was
    // missing entirely, damaged_file_kept_as is empty and appending to it
    // produced the RELATIVE name "-journal": the journal was moved into the
    // working directory and left sitting beside the restored database.
    #[test]
    fn the_damaged_journal_is_moved_aside_and_never_into_the_working_directory() {
        let scratch = Scratch::new(
            "ProjectStoreRecovery",
            "TheDamagedJournalIsMovedAsideAndNeverIntoTheWorkingDirectory",
        );
        let project = scratch.project_dir();
        let model = sample_model();
        {
            let mut store = create(&project);
            store.save(&captured(&model)).unwrap();
            assert!(store.backup(ProjectStore::DEFAULT_BACKUPS_TO_KEEP).is_ok());
        }

        // project.db is gone, but its journal is not: the case that produced a
        // relative rename target.
        std::fs::remove_file(project.join("project.db")).unwrap();
        let journal = project.join("project.db-journal");
        std::fs::write(journal.as_path(), "stale journal").unwrap();

        let stray_in_cwd = std::env::current_dir().unwrap().join("-journal");
        let _ = std::fs::remove_file(stray_in_cwd.as_path());

        let report = ProjectStore::recover(&project).unwrap();
        assert!(report.restored);

        assert!(
            !journal.exists(),
            "the stale journal was left beside the restored database"
        );
        assert!(
            !stray_in_cwd.exists(),
            "the journal was renamed to a relative path and landed in the working directory"
        );
        let _ = std::fs::remove_file(stray_in_cwd.as_path());

        // And the restored database opens.
        assert!(ProjectStore::open(&project).is_ok());
    }

    // "Newest backup" used to be decided by filename, so anything else dropped
    // into backups/ could outrank the real snapshots - and the collision
    // suffix sorted BACKWARDS, because '.' (0x2E) is above '-' (0x2D), making
    // "...T120000.db" beat the later "...T120000-1.db".
    #[test]
    fn backup_order_ignores_stray_files_and_handles_the_collision_suffix() {
        let scratch = Scratch::new(
            "ProjectStoreRecovery",
            "BackupOrderIgnoresStrayFilesAndHandlesTheCollisionSuffix",
        );
        let project = scratch.project_dir();
        let mut store = create(&project);
        store.save(&captured(&sample_model())).unwrap();

        let first = store.backup(50).unwrap();

        let backup_dir = project.join("backups");
        // A name that beats every real snapshot on a plain descending filename
        // sort.
        let stray = backup_dir.join("zzz-not-ours.db");
        std::fs::copy(first.as_path(), stray.as_path()).unwrap();
        // A hand-made copy that is not even a database.
        std::fs::write(backup_dir.join("scratch.db"), "not a database").unwrap();

        let listed = store.list_backups();
        for path in &listed {
            assert_ne!(
                path.file_name(),
                stray.file_name(),
                "a file this store never wrote was offered as a backup"
            );
            assert_ne!(path.file_name().unwrap(), "scratch.db");
        }
        assert!(!listed.is_empty());

        // The collision suffix: same second, higher sequence, therefore newer.
        let stem = first.file_stem().unwrap().to_str().unwrap();
        let collision = backup_dir.join(format!("{stem}-1.db"));
        std::fs::copy(first.as_path(), collision.as_path()).unwrap();
        let reordered = store.list_backups();
        assert!(reordered.len() >= 2);
        assert_eq!(
            reordered[0].file_name(),
            collision.file_name(),
            "the later collision backup must rank above the one it collided with"
        );
    }
}
