//! The twin of tests/storage/test_annotation_storage.cpp: the annotation
//! tables and kinds in a project file (schema 11, docs/annotation.md) - saved,
//! saved again (the DELETE list), loaded back exactly, and a project from
//! before schema 11 opening with none.

pub mod support;

use std::path::{Path, PathBuf};

use katana_entity::annotation::DEFAULT_TEXT_STYLE_NAME;
use katana_entity::tables::DEFAULT_DIMENSION_STYLE_NAME;
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, Color, DimensionGeometry, DimensionKind,
    Entity, Geometry, LabelGeometry, LabelKind, LabelPlacement, LabelRule, LabelStyle,
    LeaderGeometry, Model, TextGeometry, TextJustify, TextStyle,
};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_storage::{
    OpenMode, ProjectMetadata, ProjectStore, SqliteDatabase, apply_to_model, capture_model,
};
use support::Scratch;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn scratch(case: &str) -> (Scratch, PathBuf) {
    let scratch = Scratch::new("AnnotationStorage", case);
    let project = scratch.project_dir();
    (scratch, project)
}

fn add(model: &mut Model, geometry: Geometry) {
    let entity = Entity {
        geometry,
        ..Entity::default()
    };
    assert!(model.entities.add(entity).is_ok());
}

fn anchor(entity: u64, point: AnchorPoint, index: u32, parameter: f64) -> AnchorRef {
    AnchorRef {
        entity,
        point,
        index,
        parameter,
    }
}

fn annotated_model() -> Model {
    let mut model = Model::default();
    let road = TextStyle {
        name: String::from("Road"),
        font_family: String::from("DejaVu Sans"),
        paper_height: 3.5,
        width_factor: 0.85,
        oblique: 0.25,
        bold: true,
        color: Some(Color::new(200, 10, 20, 255)),
        mask: true,
        mask_margin: 0.75,
        readable: false,
        line_spacing: 1.25,
        ..TextStyle::default()
    };
    assert!(model.text_styles.add(road).is_ok());
    // The built-in one, changed: stored, and read back over the seed.
    let mut standard = model
        .text_styles
        .find(DEFAULT_TEXT_STYLE_NAME)
        .expect("the built-in text style")
        .clone();
    standard.paper_height = 1.8;
    assert!(model.text_styles.update(&standard).is_ok());

    let bearing = LabelStyle {
        name: String::from("Bearing Distance"),
        kind: LabelKind::Segment,
        text: String::from("{bearing:dms} {distance:.3f}"),
        text_style: String::from("Road"),
        placement: LabelPlacement::Below,
        offset: 0.8,
        priority: 3,
        color: Some(Color::new(0, 128, 0, 255)),
        minimum_length: 4.0,
        ..LabelStyle::default()
    };
    assert!(model.label_styles.add(bearing).is_ok());
    let chainage = LabelStyle {
        name: String::from("Chainage"),
        kind: LabelKind::Chainage,
        text: String::from("{chainage:ch}"),
        interval: 50.0,
        tick_interval: 0.0,
        ..LabelStyle::default()
    };
    assert!(model.label_styles.add(chainage).is_ok());

    let rule = LabelRule {
        name: String::from("boundaries"),
        label_style: String::from("Bearing Distance"),
        layer: String::from("cadastre*"),
        code: String::from("BDY*"),
        entity_type: String::from("Polyline"),
        label_layer: String::from("labels/bearings"),
        enabled: false,
    };
    assert!(model.label_rules.add(rule).is_ok());

    let mut paper = model
        .dimension_styles
        .find(DEFAULT_DIMENSION_STYLE_NAME)
        .expect("the built-in dimension style")
        .clone();
    paper.paper_sized = true;
    assert!(model.dimension_styles.update(&paper).is_ok());

    add(
        &mut model,
        Geometry::Polyline(Polyline2::new(
            vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)],
            true,
        )),
    );
    let text = TextGeometry {
        style: String::from("Road"),
        paper_height: 5.0,
        justify: TextJustify::MiddleCentre,
        ..TextGeometry::new(p(1.0, 1.0), "LOT 1\nDP 1234", 3.5, 0.0)
    };
    add(&mut model, Geometry::Text(text));
    add(
        &mut model,
        Geometry::Label(LabelGeometry {
            target: 1,
            part: 1,
            style: String::from("Bearing Distance"),
            anchor: p(10.0, 5.0),
            rule: String::from("boundaries"),
            ..LabelGeometry::default()
        }),
    );
    add(
        &mut model,
        Geometry::Leader(LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(5.0, 5.0)],
            text: String::from("7"),
            callout: CalloutShape::Circle,
            tip_ref: anchor(1, AnchorPoint::Vertex, 0, 0.0),
            ..LeaderGeometry::default()
        }),
    );
    let dimension = DimensionGeometry {
        kind: DimensionKind::Linear,
        start: p(0.0, 0.0),
        end: p(10.0, 10.0),
        offset: -3.0,
        start_ref: anchor(1, AnchorPoint::Vertex, 0, 0.0),
        end_ref: anchor(1, AnchorPoint::Vertex, 2, 0.0),
        ..DimensionGeometry::default()
    };
    add(&mut model, Geometry::Dimension(dimension));
    model
}

fn entities_of(model: &Model) -> Vec<Entity> {
    let mut entities = Vec::new();
    model.entities.for_each(|e| entities.push(e.clone()));
    entities
}

fn save_twice(project: &Path, model: &Model) {
    let mut store = ProjectStore::create(project, &ProjectMetadata::default()).unwrap();
    store
        .save(&capture_model(
            model,
            ProjectMetadata::default(),
            Vec::new(),
        ))
        .unwrap();
    // Twice: a table missing from save's DELETE list fails here, on a
    // primary-key conflict, and not on the first save.
    store
        .save(&capture_model(
            model,
            ProjectMetadata::default(),
            Vec::new(),
        ))
        .unwrap();
}

fn loaded_model(store: &ProjectStore) -> Model {
    let contents = store.load().unwrap();
    let mut loaded = Model::default();
    assert!(apply_to_model(&contents, &mut loaded).is_ok());
    loaded
}

mod annotation_storage {
    use super::*;

    #[test]
    fn every_table_and_kind_survives_saving_twice_and_reopening() {
        let (_scratch, project) = scratch("EveryTableAndKindSurvivesSavingTwiceAndReopening");
        let model = annotated_model();
        save_twice(&project, &model);
        let store = ProjectStore::open(&project).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let loaded = loaded_model(&store);

        assert_eq!(loaded.text_styles.all(), model.text_styles.all());
        assert_eq!(loaded.label_styles.all(), model.label_styles.all());
        assert_eq!(loaded.label_rules.all(), model.label_rules.all());
        assert_eq!(loaded.dimension_styles.all(), model.dimension_styles.all());
        assert_eq!(entities_of(&loaded), entities_of(&model));
    }

    #[test]
    fn a_project_from_before_schema11_opens_with_none_and_model_dimensions() {
        let (_scratch, project) =
            scratch("AProjectFromBeforeSchema11OpensWithNoneAndModelDimensions");
        let mut model = Model::default();
        add(
            &mut model,
            Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 1.0))),
        );
        {
            let mut store = ProjectStore::create(&project, &ProjectMetadata::default()).unwrap();
            store
                .save(&capture_model(
                    &model,
                    ProjectMetadata::default(),
                    Vec::new(),
                ))
                .unwrap();
        }
        {
            // Back to schema 10 as a build before this one left it.
            let database =
                SqliteDatabase::open(&project.join("project.db"), OpenMode::ReadWriteCreate)
                    .unwrap();
            database
                .execute(concat!(
                    "DROP TABLE label_rules; DROP TABLE label_styles;",
                    "DROP TABLE text_styles;",
                    "ALTER TABLE dimension_styles DROP COLUMN paper_sized;",
                    "DROP TABLE surfaces;"
                ))
                .unwrap();
            database.set_user_version(10).unwrap();
        }
        let store = ProjectStore::open(&project).unwrap();
        // Raised through every migration after 10, the surfaces' 13 the last.
        assert_eq!(
            store.schema_version().unwrap(),
            ProjectStore::CURRENT_SCHEMA_VERSION
        );
        let loaded = loaded_model(&store);
        assert_eq!(loaded.text_styles.size(), 1, "only the built-in Standard");
        assert_eq!(loaded.label_styles.size(), 0);
        assert_eq!(loaded.label_rules.size(), 0);
        assert!(
            !loaded
                .dimension_styles
                .find(DEFAULT_DIMENSION_STYLE_NAME)
                .expect("the built-in dimension style")
                .paper_sized,
            "the column's default is what every style was: model units"
        );
    }

    // Smart leaders (docs/annotation.md, "Smart leaders"): a leader whose note
    // is its own template or a label style's, tips along and inside what they
    // are on, and a dimension along a line - written in geometry blob version
    // 3 - come back exactly, beside a plain leader that is still version 2.
    #[test]
    fn smart_leaders_and_anchors_along_survive_saving_twice_and_reopening() {
        let (_scratch, project) =
            scratch("SmartLeadersAndAnchorsAlongSurviveSavingTwiceAndReopening");
        let mut model = annotated_model();
        let invert = LabelStyle {
            name: String::from("Invert"),
            kind: LabelKind::Point,
            text: String::from("IL {prop.invert:.3f}"),
            ..LabelStyle::default()
        };
        assert!(model.label_styles.add(invert).is_ok());
        let own = LeaderGeometry {
            vertices: vec![p(5.0, 0.0), p(8.0, 4.0)],
            text: String::from("{bearing:dms}\nCH {chainage:.2f}"),
            tip_ref: anchor(1, AnchorPoint::Along, 0, 0.5),
            fields: true,
            ..LeaderGeometry::default()
        };
        add(&mut model, Geometry::Leader(own));
        let styled = LeaderGeometry {
            vertices: vec![p(7.0, 3.0), p(12.0, 8.0)],
            arrow: ArrowHead::Dot,
            tip_ref: anchor(1, AnchorPoint::Inside, 0, 0.0),
            label_style: String::from("Invert"),
            ..LeaderGeometry::default()
        };
        add(&mut model, Geometry::Leader(styled));
        let along = DimensionGeometry {
            end_ref: anchor(1, AnchorPoint::Along, 0, 0.5),
            ..DimensionGeometry::new(p(0.0, 0.0), p(5.0, 0.0), 2.0, "")
        };
        add(&mut model, Geometry::Dimension(along));

        save_twice(&project, &model);
        let store = ProjectStore::open(&project).unwrap();
        let loaded = loaded_model(&store);
        assert_eq!(entities_of(&loaded), entities_of(&model));
    }
}
