//! The twin of tests/entity/test_entity.cpp: the entity model, its geometry
//! operations, the database, the tables, JSON and the heights properties.
//!
//! The C++ cases that hand the model bytes that are not UTF-8 keep their
//! UTF-8 half here: a Rust `String` cannot hold the other half, so the model
//! has nothing to refuse (the readers refuse such bytes where they decode
//! them). EntitySerialization.WritersReportFailureInsteadOfThrowing has no
//! twin at all: a writer handed a String has nothing to fail on.

pub mod support;

use std::sync::{Arc, Mutex};

use katana_core::error::ErrorCode;
use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, GEOMETRY_KIND_COUNT, entity_type_from_string,
    heights_of, is_valid_utf8, set_heights, to_string, type_of,
};
use katana_entity::entity_geometry::{bounding_box, distance_to, transformed};
use katana_entity::serialization::{
    entity_from_json, entity_to_json, geometry_from_json, geometry_to_json, properties_from_json,
};
use katana_entity::{
    Alignment, ChangeEvent, ChangeKind, Color, DimensionGeometry, Entity, EntityDatabase, EntityId,
    EntityType, Geometry, HatchPattern, LabelGeometry, Layer, LayerDatabase, LeaderGeometry,
    PointGeometry, PropertyDatabase, PropertyDefinition, PropertyMap, PropertyType, PropertyValue,
    Style, StyleDatabase, TextGeometry, validate,
};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Spline2,
    Vec2,
};
use katana_math::Mat3;
use katana_math::numerics::{HALF_PI, PI};
use katana_math::vec2::nearly_equal;
use support::{Random, double_eq, near};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn line_entity(x0: f64, y0: f64, x1: f64, y1: f64) -> Entity {
    Entity {
        geometry: Geometry::Line(Segment2::new(p(x0, y0), p(x1, y1))),
        ..Entity::default()
    }
}

fn one_of_each_geometry() -> Vec<Geometry> {
    vec![
        Geometry::Point(PointGeometry {
            position: p(1.5, -2.25),
        }),
        Geometry::Line(Segment2::new(p(0.0, 0.0), p(10.0, 5.0))),
        Geometry::Arc(Arc2::new(p(1.0, 2.0), 3.5, 0.25, -1.75)),
        Geometry::Polyline(Polyline2::new(
            vec![p(0.0, 0.0), p(4.0, 0.0), p(4.0, 3.0)],
            true,
        )),
        Geometry::Circle(Circle2::new(p(-1.0, -1.0), 2.0)),
        Geometry::Text(TextGeometry::new(p(5.0, 5.0), "BM \"A\" 102.45", 2.5, 0.5)),
        Geometry::Dimension(DimensionGeometry::new(
            p(0.0, 0.0),
            p(10.0, 0.0),
            3.0,
            "10.00 m",
        )),
        Geometry::Label(LabelGeometry {
            target: 7,
            style: String::from("Lot area"),
            anchor: p(3.0, 4.0),
            ..LabelGeometry::default()
        }),
        Geometry::Leader(LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(4.0, 3.0)],
            text: String::from("PIT 12"),
            ..LeaderGeometry::default()
        }),
        Geometry::CurvePolyline(CurvePolyline2 {
            vertices: vec![
                CurveVertex {
                    position: p(0.0, 0.0),
                    bulge: 0.5,
                    height: Some(10.25),
                },
                CurveVertex {
                    position: p(6.0, 0.0),
                    bulge: 0.0,
                    height: None,
                },
                CurveVertex {
                    position: p(6.0, 4.0),
                    bulge: -0.25,
                    height: Some(12.5),
                },
            ],
            closed: false,
        }),
        Geometry::Ellipse(Ellipse2 {
            center: p(2.0, 3.0),
            major_axis: Vec2::new(5.0, 1.0),
            ratio: 0.4,
            start_parameter: 0.5,
            sweep: 2.0,
        }),
        Geometry::Spline(
            Spline2::through_points(vec![p(0.0, 0.0), p(3.0, 4.0), p(7.0, 3.0), p(10.0, 8.0)], 3)
                .unwrap(),
        ),
    ]
}

/// The C++ nearlyEqual(Vec2, Vec2) with its default, tolerance::kRelative.
fn close(actual: Point2, expected: Point2) -> bool {
    nearly_equal(actual, expected, katana_math::numerics::tolerance::RELATIVE)
}

fn code(status: katana_core::Status) -> ErrorCode {
    status.unwrap_err().code
}

mod entity {
    use super::*;

    #[test]
    fn type_follows_geometry_alternative() {
        let geometries = one_of_each_geometry();
        let expected = [
            EntityType::Point,
            EntityType::Line,
            EntityType::Arc,
            EntityType::Polyline,
            EntityType::Circle,
            EntityType::Text,
            EntityType::Dimension,
            EntityType::Label,
            EntityType::Leader,
            EntityType::CurvePolyline,
            EntityType::Ellipse,
            EntityType::Spline,
        ];
        // Both pinned to the variant, so a kind added without extending this
        // corpus fails here rather than leaving a test named "one of each"
        // quietly covering fewer.
        assert_eq!(
            geometries.len(),
            GEOMETRY_KIND_COUNT,
            "one_of_each_geometry() is missing a geometry kind"
        );
        assert_eq!(expected.len(), geometries.len());
        for (geometry, &kind) in geometries.iter().zip(expected.iter()) {
            assert_eq!(type_of(geometry), kind);
            let parsed = entity_type_from_string(to_string(kind));
            assert_eq!(parsed.unwrap(), kind);
        }
        assert!(entity_type_from_string("Hyperbola").is_err());
    }
}

mod entity_geometry {
    use super::*;

    #[test]
    fn validate_rejects_degenerate_and_non_finite() {
        for geometry in one_of_each_geometry() {
            assert!(validate(&geometry).is_ok(), "{geometry:?}");
        }
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let invalid = [
            Geometry::Point(PointGeometry {
                position: p(nan, 0.0),
            }),
            Geometry::Line(Segment2::new(p(0.0, 0.0), p(inf, 0.0))),
            Geometry::Line(Segment2::new(p(1.0, 1.0), p(1.0, 1.0))), // zero length
            Geometry::Circle(Circle2::new(p(0.0, 0.0), 0.0)),        // zero radius
            Geometry::Circle(Circle2::new(p(0.0, 0.0), -1.0)),       // negative radius
            Geometry::Arc(Arc2::new(p(0.0, 0.0), 1.0, 0.0, 0.0)),    // zero sweep
            Geometry::Arc(Arc2::new(p(0.0, 0.0), 1.0, 0.0, 7.0)),    // > full turn
            Geometry::Polyline(Polyline2::new(vec![p(0.0, 0.0)], false)), // one vertex
            Geometry::Polyline(Polyline2::new(vec![p(0.0, 0.0), p(0.0, 0.0)], false)), // duplicates
            Geometry::Text(TextGeometry::new(p(0.0, 0.0), "", 2.5, 0.0)), // empty text
            Geometry::Text(TextGeometry::new(p(0.0, 0.0), "A", 0.0, 0.0)), // zero height
            Geometry::Dimension(DimensionGeometry::new(p(1.0, 1.0), p(1.0, 1.0), 1.0, "")),
        ];
        for geometry in &invalid {
            assert!(validate(geometry).is_err(), "{geometry:?}");
        }
        assert_eq!(
            code(validate(&Geometry::Circle(Circle2::new(p(0.0, 0.0), 0.0)))),
            ErrorCode::InvalidGeometry
        );
    }

    #[test]
    fn bounding_box_and_pick_distance() {
        let circle = Geometry::Circle(Circle2::new(p(0.0, 0.0), 2.0));
        assert_eq!(bounding_box(&circle).min, p(-2.0, -2.0));
        double_eq(distance_to(&circle, p(5.0, 0.0)), 3.0);
        double_eq(distance_to(&circle, p(0.0, 0.0)), 2.0); // to the curve, not the disc

        let dimension =
            Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 3.0, ""));
        double_eq(distance_to(&dimension, p(5.0, 3.0)), 0.0); // on the dimension line
        assert!(bounding_box(&dimension).contains(p(0.0, 0.0)));
        assert!(bounding_box(&dimension).contains(p(10.0, 3.0)));

        // Reads upwards.
        let text = Geometry::Text(TextGeometry::new(p(0.0, 0.0), "ABCD", 10.0, HALF_PI));
        let bounds = bounding_box(&text);
        near(bounds.max.y, 24.0, 1e-9); // 4 glyphs * 0.6 * 10 along +y
        near(bounds.min.x, -10.0, 1e-9); // height extends to the left of the baseline
    }

    #[test]
    fn similarity_transforms_every_geometry_kind() {
        let shift = Mat3::translation(Vec2::new(100.0, 200.0));
        for geometry in one_of_each_geometry() {
            let moved = transformed(&geometry, &shift);
            let moved = moved.unwrap_or_else(|error| panic!("{}", error.describe()));
            let before = bounding_box(&geometry);
            let after = bounding_box(&moved);
            assert!(nearly_equal(
                after.min,
                before.min + Vec2::new(100.0, 200.0),
                1e-12
            ));
            assert!(nearly_equal(
                after.max,
                before.max + Vec2::new(100.0, 200.0),
                1e-12
            ));
        }
    }

    #[test]
    fn rotation_scale_and_mirror_of_an_arc() {
        // (2,0) -> (0,2), counter-clockwise.
        let arc = Geometry::Arc(Arc2::new(p(0.0, 0.0), 2.0, 0.0, HALF_PI));

        let rotated = transformed(&arc, &Mat3::rotation(HALF_PI)).unwrap();
        let Geometry::Arc(r) = rotated else {
            panic!("not an arc")
        };
        assert!(close(r.start_point(), p(0.0, 2.0)));
        assert!(close(r.end_point(), p(-2.0, 0.0)));
        double_eq(r.sweep, HALF_PI);

        let scaled = transformed(&arc, &Mat3::scaling(3.0, 3.0)).unwrap();
        let Geometry::Arc(s) = scaled else {
            panic!("not an arc")
        };
        double_eq(s.radius, 6.0);

        // Mirror about the y axis: the arc must still join the mirrored
        // endpoints through the mirrored midpoint, which requires reversing the
        // sweep.
        let mirrored =
            transformed(&arc, &Mat3::reflection(p(0.0, 0.0), Vec2::new(0.0, 1.0))).unwrap();
        let Geometry::Arc(m) = mirrored else {
            panic!("not an arc")
        };
        assert!(close(m.start_point(), p(-2.0, 0.0)));
        assert!(close(m.end_point(), p(0.0, 2.0)));
        assert!(close(m.midpoint(), p(-(2.0_f64.sqrt()), 2.0_f64.sqrt())));
        double_eq(m.sweep, -HALF_PI);
    }

    #[test]
    fn mirror_flips_dimension_side() {
        // Line at y = +3.
        let dimension =
            Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 3.0, ""));
        let mirrored = transformed(
            &dimension,
            &Mat3::reflection(p(0.0, 0.0), Vec2::new(1.0, 0.0)),
        )
        .unwrap();
        let Geometry::Dimension(flipped) = &mirrored else {
            panic!("not a dimension")
        };
        double_eq(flipped.offset, -3.0);
        double_eq(distance_to(&mirrored, p(5.0, -3.0)), 0.0); // now at y = -3
    }

    #[test]
    fn rejects_non_similarity_transforms() {
        let circle = Geometry::Circle(Circle2::new(p(0.0, 0.0), 2.0));
        let stretched = transformed(&circle, &Mat3::scaling(2.0, 1.0));
        assert_eq!(stretched.unwrap_err().code, ErrorCode::InvalidArgument);
        // Shear.
        assert!(
            transformed(
                &circle,
                &Mat3::new(1.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
            )
            .is_err()
        );
        assert!(transformed(&circle, &Mat3::scaling(0.0, 0.0)).is_err()); // collapse
        assert!(transformed(&circle, &Mat3::scaling(1e-9, 1e-9)).is_err()); // below tolerance
    }

    #[test]
    fn transform_round_trip_property() {
        let mut random = Random::default();
        for _ in 0..200 {
            let pivot_x = random.real(-100.0, 100.0);
            let pivot_y = random.real(-100.0, 100.0);
            let pivot = p(pivot_x, pivot_y);
            let angle = random.real(-PI, PI);
            let forward = Mat3::rotation_about(pivot, angle);
            let backward = Mat3::rotation_about(pivot, -angle);
            for geometry in one_of_each_geometry() {
                let there = transformed(&geometry, &forward).unwrap();
                let back = transformed(&there, &backward).unwrap();
                let expected = bounding_box(&geometry);
                let actual = bounding_box(&back);
                assert!(nearly_equal(actual.min, expected.min, 1e-9));
                assert!(nearly_equal(actual.max, expected.max, 1e-9));
            }
        }
    }
}

mod entity_color {
    use super::*;

    #[test]
    fn hex_round_trip_and_errors() {
        let opaque = Color::from_hex("#1a2B3c").unwrap();
        assert_eq!(opaque, Color::new(0x1A, 0x2B, 0x3C, 0xFF));
        assert_eq!(opaque.to_hex(), "#1A2B3C");

        let translucent = Color::from_hex("#FF000080").unwrap();
        assert_eq!(translucent.a, 0x80);
        assert_eq!(translucent.to_hex(), "#FF000080");

        for bad in [
            "",
            "#",
            "123456",
            "#12345",
            "#GGGGGG",
            "#1234567",
            "#123456789",
        ] {
            assert!(Color::from_hex(bad).is_err(), "{bad}");
        }
    }
}

mod entity_database {
    use super::*;

    #[test]
    fn assigns_monotonic_ids_that_are_never_reused() {
        let mut database = EntityDatabase::new();
        let first = database.add(line_entity(0.0, 0.0, 1.0, 1.0)).unwrap();
        let second = database.add(line_entity(0.0, 0.0, 2.0, 2.0)).unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 2);

        assert!(database.remove(second).is_ok());
        let third = database.add(line_entity(0.0, 0.0, 3.0, 3.0)).unwrap();
        assert_eq!(third, 3); // 2 is retired for good

        database.clear();
        assert!(database.empty());
        let after_clear = database.add(line_entity(0.0, 0.0, 4.0, 4.0)).unwrap();
        assert_eq!(after_clear, 4);
    }

    #[test]
    fn rejects_invalid_entities() {
        let mut database = EntityDatabase::new();
        let degenerate = database.add(line_entity(1.0, 1.0, 1.0, 1.0));
        assert_eq!(degenerate.unwrap_err().code, ErrorCode::InvalidGeometry);

        let mut no_layer = line_entity(0.0, 0.0, 1.0, 1.0);
        no_layer.layer.clear();
        assert!(database.add(no_layer).is_err());
        assert!(database.empty());
        assert_eq!(database.next_id(), 1); // failed adds do not burn ids
    }

    #[test]
    fn insert_restores_a_removed_entity_under_its_original_id() {
        let mut database = EntityDatabase::new();
        let id = database.add(line_entity(0.0, 0.0, 5.0, 5.0)).unwrap();
        let removed = database.remove(id).unwrap();
        assert!(!database.contains(id));

        assert!(database.insert(removed.clone()).is_ok());
        assert_eq!(database.find(id), Some(&removed));

        assert_eq!(
            code(database.insert(removed.clone())),
            ErrorCode::AlreadyExists
        );
        // Id 0 is invalid.
        assert!(database.insert(line_entity(0.0, 0.0, 1.0, 1.0)).is_err());

        let mut loaded = line_entity(0.0, 0.0, 1.0, 1.0);
        loaded.id = 50;
        assert!(database.insert(loaded).is_ok());
        assert_eq!(database.next_id(), 51); // counter jumps past loaded ids
    }

    #[test]
    fn replace_and_remove_report_missing_entities() {
        let mut database = EntityDatabase::new();
        let mut ghost = line_entity(0.0, 0.0, 1.0, 1.0);
        ghost.id = 99;
        assert_eq!(code(database.replace(ghost)), ErrorCode::NotFound);
        assert_eq!(database.remove(99).unwrap_err().code, ErrorCode::NotFound);

        let id = database.add(line_entity(0.0, 0.0, 1.0, 1.0)).unwrap();
        let mut changed = database.find(id).unwrap().clone();
        changed.layer = String::from("Survey");
        changed.geometry = Geometry::Circle(Circle2::new(p(0.0, 0.0), 5.0));
        assert!(database.replace(changed.clone()).is_ok());
        assert_eq!(database.find(id).unwrap().r#type(), EntityType::Circle);

        changed.geometry = Geometry::Circle(Circle2::new(p(0.0, 0.0), -5.0));
        assert!(database.replace(changed).is_err());
        // Untouched by the failed replace.
        assert_eq!(database.find(id).unwrap().r#type(), EntityType::Circle);
        let Geometry::Circle(circle) = &database.find(id).unwrap().geometry else {
            panic!("not a circle")
        };
        double_eq(circle.radius, 5.0);
    }

    #[test]
    fn queries_are_ordered_and_layer_aware() {
        let mut database = EntityDatabase::new();
        let mut on_survey = line_entity(0.0, 0.0, 10.0, 0.0);
        on_survey.layer = String::from("Survey");
        let a = database.add(line_entity(0.0, 0.0, 1.0, 1.0)).unwrap();
        let b = database.add(on_survey).unwrap();
        let c = database.add(line_entity(-5.0, -5.0, 0.0, 0.0)).unwrap();

        assert_eq!(database.ids(), vec![a, b, c]);
        assert_eq!(database.ids_on_layer("Survey"), vec![b]);
        assert_eq!(database.count_on_layer("0"), 2);
        assert_eq!(database.count_on_layer("missing"), 0);

        let bounds = database.bounds();
        assert_eq!(bounds.min, p(-5.0, -5.0));
        assert_eq!(bounds.max, p(10.0, 1.0));
        assert!(EntityDatabase::new().bounds().empty());

        let mut visited: Vec<EntityId> = Vec::new();
        database.for_each(|entity| visited.push(entity.id));
        assert_eq!(visited, database.ids());
    }

    #[test]
    fn reports_every_mutation_to_the_observer() {
        let mut database = EntityDatabase::new();
        let events: Arc<Mutex<Vec<ChangeEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&events);
        database.set_observer(Some(Arc::new(move |event: &ChangeEvent| {
            recorded.lock().unwrap().push(*event);
        })));

        let id = database.add(line_entity(0.0, 0.0, 1.0, 1.0)).unwrap();
        let mut changed = database.find(id).unwrap().clone();
        changed.visible = false;
        assert!(database.replace(changed).is_ok());
        assert!(database.remove(id).is_ok());
        database.clear();
        assert!(database.remove(id).is_err()); // failures are silent to observers

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].kind, ChangeKind::EntityAdded);
        assert_eq!(events[1].kind, ChangeKind::EntityModified);
        assert_eq!(events[2].kind, ChangeKind::EntityRemoved);
        assert_eq!(events[3].kind, ChangeKind::Cleared);
        assert_eq!(events[0].id, id);
    }
}

mod layer_database {
    use super::*;

    fn named(name: &str) -> Layer {
        Layer {
            name: String::from(name),
            ..Layer::default()
        }
    }

    #[test]
    fn default_layer_always_exists_and_cannot_be_removed() {
        let mut layers = LayerDatabase::new();
        assert!(layers.find("0").is_some());
        assert_eq!(
            layers.remove("0").unwrap_err().code,
            ErrorCode::InvalidArgument
        );

        assert!(
            layers
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
        assert_eq!(code(layers.add(named("Survey"))), ErrorCode::AlreadyExists);
        assert!(layers.add(named("")).is_err());
        assert_eq!(layers.names(), vec!["0", "Survey"]);

        let mut hidden = layers.find("Survey").unwrap().clone();
        hidden.visible = false;
        assert!(layers.update(&hidden).is_ok());
        assert!(!layers.find("Survey").unwrap().visible);
        assert_eq!(code(layers.update(&named("Missing"))), ErrorCode::NotFound);

        let removed = layers.remove("Survey").unwrap();
        assert_eq!(removed.linetype, "dashed"); // returned intact so undo can restore it
        assert_eq!(layers.size(), 1);

        assert!(layers.add(named("Temp")).is_ok());
        layers.reset();
        assert_eq!(layers.names(), vec!["0"]);
    }
}

mod style_database {
    use super::*;

    #[test]
    fn add_update_remove() {
        let mut styles = StyleDatabase::new();
        assert!(
            styles
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
            styles
                .add(Style {
                    name: String::from("Boundary"),
                    ..Style::default()
                })
                .is_err()
        );
        assert!(
            styles
                .add(Style {
                    name: String::from("Bad"),
                    color: None,
                    line_weight: -1.0,
                    linetype: String::from("continuous"),
                    ..Style::default()
                })
                .is_err()
        );
        let mut thinner = styles.find("Boundary").unwrap().clone();
        thinner.line_weight = 0.35;
        assert!(styles.update(&thinner).is_ok());
        double_eq(styles.find("Boundary").unwrap().line_weight, 0.35);
        assert!(styles.remove("Boundary").is_ok());
        assert_eq!(
            styles.remove("Boundary").unwrap_err().code,
            ErrorCode::NotFound
        );
    }
}

mod property_database {
    use super::*;

    #[test]
    fn defined_properties_are_typed() {
        let mut properties = PropertyDatabase::default();
        assert!(
            properties
                .define(PropertyDefinition {
                    name: String::from("elevation"),
                    r#type: PropertyType::Real,
                    description: String::from("Ground level (m)"),
                    default_value: Some(PropertyValue::Real(0.0)),
                })
                .is_ok()
        );
        assert!(
            properties
                .define(PropertyDefinition {
                    name: String::from("elevation"),
                    r#type: PropertyType::Real,
                    ..PropertyDefinition::default()
                })
                .is_err()
        );
        // A default of the wrong type.
        assert!(
            properties
                .define(PropertyDefinition {
                    name: String::from("code"),
                    r#type: PropertyType::Text,
                    description: String::new(),
                    default_value: Some(PropertyValue::Integer(1)),
                })
                .is_err()
        );

        assert!(
            properties
                .validate("elevation", &PropertyValue::Real(102.45))
                .is_ok()
        );
        assert!(
            properties
                .validate("elevation", &PropertyValue::from("high"))
                .is_err()
        );
        assert!(
            properties
                .validate("elevation", &PropertyValue::Real(f64::NAN))
                .is_err()
        );
        assert!(
            properties
                .validate("free-form", &PropertyValue::from("anything"))
                .is_ok()
        );

        assert!(properties.undefine("elevation").is_ok());
        assert!(
            properties
                .validate("elevation", &PropertyValue::from("now free"))
                .is_ok()
        );
    }
}

mod entity_serialization {
    use super::*;

    #[test]
    fn every_geometry_kind_round_trips_exactly() {
        for geometry in one_of_each_geometry() {
            let json = geometry_to_json(&geometry).unwrap();
            let parsed = geometry_from_json(&json)
                .unwrap_or_else(|error| panic!("{json} -> {}", error.describe()));
            assert_eq!(parsed, geometry, "{json}");
        }
    }

    #[test]
    fn doubles_survive_bit_for_bit() {
        // Values with no short decimal form, at survey coordinate magnitudes.
        let mut random = Random::default();
        for _ in 0..500 {
            let x0 = random.real(4e5, 6e5);
            let y0 = random.real(4e6, 6e6);
            let x1 = random.real(-1e-6, 1e-6);
            let segment = Segment2::new(p(x0, y0), p(x1, 1.0 / 3.0));
            let written = geometry_to_json(&Geometry::Line(segment)).unwrap();
            let parsed = geometry_from_json(&written).unwrap();
            assert_eq!(parsed, Geometry::Line(segment)); // exact ==
        }
    }

    #[test]
    fn full_entity_round_trip_keeps_property_types() {
        let mut properties = PropertyMap::new();
        properties.insert(String::from("code"), PropertyValue::from("IP"));
        properties.insert(String::from("elevation"), PropertyValue::Real(102.5));
        // A real that happens to be integral.
        properties.insert(String::from("whole"), PropertyValue::Real(7.0));
        properties.insert(String::from("order"), PropertyValue::Integer(2));
        properties.insert(String::from("verified"), PropertyValue::Boolean(true));
        let mut metadata = PropertyMap::new();
        metadata.insert(
            String::from("source"),
            PropertyValue::from("field-book-7.csv"),
        );
        let entity = Entity {
            id: 42,
            geometry: Geometry::Circle(Circle2::new(p(3.0, 4.0), 5.0)),
            layer: String::from("Control"),
            style: String::from("Monument"),
            color: Some(Color::new(10, 20, 30, 255)),
            visible: false,
            properties,
            metadata,
        };

        let written = entity_to_json(&entity).unwrap();
        let parsed =
            entity_from_json(&written).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed, entity);
        assert!(matches!(
            parsed.properties.get("whole"),
            Some(PropertyValue::Real(_))
        ));
        assert!(matches!(
            parsed.properties.get("order"),
            Some(PropertyValue::Integer(_))
        ));
    }

    #[test]
    fn malformed_or_invalid_input_is_rejected() {
        let failure = |json: &str| geometry_from_json(json).unwrap_err().code;
        assert_eq!(failure("not json"), ErrorCode::ParseFailure);
        assert_eq!(failure("{}"), ErrorCode::ParseFailure); // no type
        assert_eq!(failure(r#"{"type":"Spline"}"#), ErrorCode::ParseFailure);
        // Missing radius.
        assert_eq!(
            failure(r#"{"type":"Circle","center":[0,0]}"#),
            ErrorCode::ParseFailure
        );
        // A point with three numbers.
        assert_eq!(
            failure(r#"{"type":"Circle","center":[0,0,0],"radius":1}"#),
            ErrorCode::ParseFailure
        );
        // A value of the wrong type.
        assert_eq!(
            failure(r#"{"type":"Circle","center":[0,0],"radius":"big"}"#),
            ErrorCode::ParseFailure
        );
        // Well formed but invalid.
        assert_eq!(
            failure(r#"{"type":"Circle","center":[0,0],"radius":-1}"#),
            ErrorCode::InvalidGeometry
        );
        assert!(properties_from_json(r#"{"nested":{"a":1}}"#).is_err());
        assert!(properties_from_json("[1,2]").is_err());
        assert!(
            entity_from_json(
                r#"{"id":1,"color":"red","geometry":{"type":"Point","position":[0,0]}}"#
            )
            .is_err()
        );
    }
}

// Regression. The C++ JSON writer throws on text that is not valid UTF-8, and
// it is reached from a save path that returns Status and promises not to
// throw. Bad bytes are refused on the way in; here a String cannot carry them.
mod entity_utf8 {
    use super::*;

    #[test]
    fn well_formed_sequences_are_accepted() {
        assert!(is_valid_utf8(b""));
        assert!(is_valid_utf8(b"plain ASCII"));
        assert!(is_valid_utf8(b"caf\xc3\xa9")); // U+00E9, two bytes
        assert!(is_valid_utf8(b"\xe2\x88\x86")); // U+2206, three bytes
        assert!(is_valid_utf8(b"\xf0\x9f\x93\x90")); // U+1F4D0, four bytes
        assert!(is_valid_utf8(b"a\0b")); // an embedded NUL is valid UTF-8
    }

    #[test]
    fn malformed_sequences_are_rejected() {
        assert!(!is_valid_utf8(b"caf\xe9")); // CP1252 e-acute: a lone lead byte
        assert!(!is_valid_utf8(b"\x80")); // continuation byte with no lead
        assert!(!is_valid_utf8(b"\xc3")); // truncated two-byte sequence
        assert!(!is_valid_utf8(b"\xe2\x88")); // truncated three-byte sequence
        assert!(!is_valid_utf8(b"\xc3\x28")); // lead followed by a non-continuation
        assert!(!is_valid_utf8(b"\xff\xfe")); // never valid lead bytes
        // Overlong encodings: the standard requires the shortest form, and
        // accepting these is a classic way to smuggle characters past a filter.
        assert!(!is_valid_utf8(b"\xc0\xaf")); // overlong '/'
        assert!(!is_valid_utf8(b"\xe0\x80\xaf")); // overlong again
        assert!(!is_valid_utf8(b"\xf0\x80\x80\xaf"));
        // Surrogate halves are not characters and must not appear in UTF-8.
        assert!(!is_valid_utf8(b"\xed\xa0\x80")); // U+D800
        assert!(!is_valid_utf8(b"\xed\xbf\xbf")); // U+DFFF
        // Beyond U+10FFFF.
        assert!(!is_valid_utf8(b"\xf4\x90\x80\x80"));
        assert!(!is_valid_utf8(b"\xf5\x80\x80\x80"));
    }

    // The C++ first hands validate a text of CP1252 bytes, which a String
    // cannot hold; the same word properly encoded is accepted.
    #[test]
    fn invalid_text_is_refused_by_geometry_validation() {
        let text = TextGeometry {
            position: p(0.0, 0.0),
            height: 2.0,
            text: String::from("caf\u{e9}"), // the same word, properly encoded
            ..TextGeometry::default()
        };
        assert!(validate(&Geometry::Text(text)).is_ok());
    }

    // The C++ also refuses a value and a name of CP1252 bytes, which a String
    // cannot hold.
    #[test]
    fn invalid_property_strings_are_refused() {
        let properties = PropertyDatabase::default();
        assert!(
            properties
                .validate("note", &PropertyValue::from("caf\u{e9}"))
                .is_ok()
        );
    }

    // Audit MOD-12: a description is saved as JSON like the name beside it.
    // The C++ first gives each a CP1252 description, which a String cannot
    // hold; the UTF-8 halves are kept.
    #[test]
    fn descriptions_are_refused_as_names_are_when_they_are_not_utf8() {
        let hatch = HatchPattern {
            name: String::from("grass"),
            solid: true,
            description: String::from("caf\u{e9}"),
            ..HatchPattern::default()
        };
        assert!(validate(&hatch).is_ok());

        let mut properties = PropertyDatabase::default();
        let note = PropertyDefinition {
            name: String::from("note"),
            description: String::from("caf\u{e9}"),
            ..PropertyDefinition::default()
        };
        assert!(properties.define(note).is_ok());

        // An alignment with no PIs is refused anyway, and not for its
        // description.
        let road = Alignment {
            name: String::from("road"),
            description: String::from("caf\u{e9}"),
            ..Alignment::default()
        };
        let refused = validate(&road);
        assert!(refused.is_err());
        assert_ne!(
            refused.unwrap_err().message,
            "alignment description is not valid UTF-8"
        );
    }
}

mod entity_heights {
    use super::*;

    #[test]
    fn one_height_shared_by_every_vertex_is_written_once_as_elevation() {
        let mut properties = PropertyMap::new();
        set_heights(&mut properties, &[Some(32.5), Some(32.5), Some(32.5)]);
        assert_eq!(properties.len(), 1);
        assert_eq!(
            properties.get(ELEVATION_PROPERTY),
            Some(&PropertyValue::Real(32.5))
        );
        // Read back for any vertex count: a single height is every vertex's.
        let heights = heights_of(&properties, 3);
        assert_eq!(heights.len(), 3);
        for z in heights {
            assert_eq!(z, Some(32.5));
        }
    }

    #[test]
    fn a_string_surveyed_at_some_vertices_keeps_its_gaps_as_null() {
        // 0.1 is not exact in binary; the written text must still read back as
        // the same double, which is what format_exact_real promises.
        let mut properties = PropertyMap::new();
        set_heights(&mut properties, &[Some(10.25), None, Some(0.1)]);
        assert_eq!(
            properties.get(ELEVATIONS_PROPERTY),
            Some(&PropertyValue::from("10.25 null 0.1"))
        );
        assert!(!properties.contains_key(ELEVATION_PROPERTY));

        let heights = heights_of(&properties, 3);
        assert_eq!(heights.len(), 3);
        assert_eq!(heights[0], Some(10.25));
        assert!(heights[1].is_none(), "a gap is no height, not zero");
        assert_eq!(heights[2], Some(0.1));
    }

    #[test]
    fn no_height_anywhere_writes_nothing_and_clears_a_stale_one() {
        let mut properties = PropertyMap::new();
        properties.insert(String::from(ELEVATION_PROPERTY), PropertyValue::Real(5.0));
        properties.insert(
            String::from(ELEVATIONS_PROPERTY),
            PropertyValue::from("1 2"),
        );
        properties.insert(String::from("note"), PropertyValue::from("kept"));
        set_heights(&mut properties, &[None, None]);
        assert!(!properties.contains_key(ELEVATION_PROPERTY));
        assert!(!properties.contains_key(ELEVATIONS_PROPERTY));
        assert!(
            properties.contains_key("note"),
            "only the two height properties are touched"
        );

        let heights = heights_of(&properties, 2);
        assert_eq!(heights.len(), 2);
        assert!(heights[0].is_none());
        assert!(heights[1].is_none());
    }

    #[test]
    fn rewriting_replaces_the_other_form_rather_than_leaving_it_to_win() {
        // A list written first and a uniform height written after: the reader
        // prefers a list of the right length, so a stale one left behind would
        // silently override the new height.
        let mut properties = PropertyMap::new();
        set_heights(&mut properties, &[Some(1.0), Some(2.0)]);
        set_heights(&mut properties, &[Some(7.0), Some(7.0)]);
        assert!(!properties.contains_key(ELEVATIONS_PROPERTY));
        assert_eq!(heights_of(&properties, 2)[1], Some(7.0));
    }

    #[test]
    fn a_list_of_the_wrong_length_is_not_stretched_to_fit() {
        // Three heights for a string that now has four vertices: which vertex
        // lost or gained one is unknowable, so the list is passed over and the
        // single elevation, if any, stands in.
        let mut properties = PropertyMap::new();
        properties.insert(
            String::from(ELEVATIONS_PROPERTY),
            PropertyValue::from("1 2 3"),
        );
        for z in heights_of(&properties, 4) {
            assert!(z.is_none());
        }
        properties.insert(String::from(ELEVATION_PROPERTY), PropertyValue::Integer(12));
        for z in heights_of(&properties, 4) {
            assert_eq!(z, Some(12.0), "an integer elevation is a height too");
        }
    }

    #[test]
    fn non_finite_heights_are_no_height() {
        let mut properties = PropertyMap::new();
        set_heights(&mut properties, &[Some(f64::NAN), Some(f64::INFINITY)]);
        assert!(properties.is_empty());

        set_heights(&mut properties, &[Some(4.0), Some(f64::NAN)]);
        assert_eq!(
            properties.get(ELEVATIONS_PROPERTY),
            Some(&PropertyValue::from("4 null"))
        );

        let mut written = PropertyMap::new();
        written.insert(
            String::from(ELEVATIONS_PROPERTY),
            PropertyValue::from("4 nan inf 5"),
        );
        let heights = heights_of(&written, 4);
        assert_eq!(heights[0], Some(4.0));
        assert!(heights[1].is_none());
        assert!(heights[2].is_none());
        assert_eq!(heights[3], Some(5.0));
    }

    #[test]
    fn an_empty_vertex_list_writes_and_reads_nothing() {
        let mut properties = PropertyMap::new();
        set_heights(&mut properties, &[]);
        assert!(properties.is_empty());
        assert!(heights_of(&properties, 0).is_empty());
    }
}
