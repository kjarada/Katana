//! The twin of tests/entity/annotation/test_leader_values.cpp: smart leaders
//! in the entity layer (docs/annotation.md, "Smart leaders") - the Along and
//! Inside anchors, the values a leader's note reads off the entity its tip is
//! on, the leader's template check, validation, and the version-3 geometry
//! blob and JSON that store them.
//!
//! Expected values are worked out by hand in the comments beside them.

pub mod support;

use katana_core::error::ErrorCode;
use katana_entity::anchor::{describe, inside_point, nearest_anchor, resolve_anchor};
use katana_entity::entity::{anchor_point_from_string, set_heights};
use katana_entity::geometry_blob::{
    BLOB_VERSION_ANNOTATION, BLOB_VERSION_SMART_LEADER, geometry_from_blob, geometry_to_blob,
};
use katana_entity::label_text::{format_value, template_fields};
use katana_entity::leader_values::{
    anchor_values, check_leader_says_something, check_leader_template, leader_note,
    leader_value_names, leader_value_quantity, leader_values, missing_values,
};
use katana_entity::serialization::{geometry_from_json, geometry_to_json};
use katana_entity::{
    AnchorPoint, AnchorRef, CalloutShape, DimensionGeometry, DimensionKind, Entity, EntityId,
    Geometry, LabelGeometry, LabelKind, LabelStyle, LabelValues, LeaderContext, LeaderGeometry,
    Model, PointGeometry, PropertyValue, TextGeometry, validate,
};
use katana_geometry::{Arc2, Circle2, Point2, Polyline2, Segment2, Vec2};
use katana_math::numerics::PI;
use support::{double_eq, near};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn entity_of(id: EntityId, geometry: Geometry) -> Entity {
    Entity {
        id,
        geometry,
        ..Entity::default()
    }
}

fn along(id: EntityId, parameter: f64, index: u32) -> AnchorRef {
    AnchorRef {
        entity: id,
        point: AnchorPoint::Along,
        index,
        parameter,
    }
}

fn at(id: EntityId, point: AnchorPoint) -> AnchorRef {
    AnchorRef {
        entity: id,
        point,
        ..AnchorRef::default()
    }
}

fn indexed(id: EntityId, point: AnchorPoint, index: u32) -> AnchorRef {
    AnchorRef {
        entity: id,
        point,
        index,
        parameter: 0.0,
    }
}

/// The value `name` in its own format, or "absent".
fn said(values: &LabelValues, name: &str) -> String {
    values
        .get(name)
        .map_or_else(|| String::from("absent"), format_value)
}

fn number(values: &LabelValues, name: &str) -> f64 {
    values[name].number
}

/// The values with no survey code and no context: the C++ defaults.
fn values_at(target: &Entity, reference: AnchorRef, tip: Point2) -> LabelValues {
    anchor_values(target, &reference, tip, &[], &LeaderContext::default())
}

/// A model holding `entities`, added in order (ids 1, 2, ...).
fn model_with(entities: Vec<Entity>) -> Model {
    let mut model = Model::new();
    for entity in entities {
        assert!(model.entities.add(entity).is_ok());
    }
    model
}

fn smart_leader(
    target: EntityId,
    mut reference: AnchorRef,
    tip: Point2,
    template_text: &str,
) -> LeaderGeometry {
    reference.entity = target;
    LeaderGeometry {
        vertices: vec![tip, tip + Vec2::new(5.0, 5.0)],
        tip_ref: reference,
        text: String::from(template_text),
        fields: true,
        ..LeaderGeometry::default()
    }
}

fn polyline(points: &[(f64, f64)], closed: bool) -> Geometry {
    Geometry::Polyline(Polyline2::new(
        points.iter().map(|&(x, y)| p(x, y)).collect(),
        closed,
    ))
}

// ---- anchors on and inside entities ------------------------------------------------------

mod smart_anchors {
    use super::*;

    #[test]
    fn along_is_a_fraction_of_what_it_names() {
        let line = entity_of(1, Geometry::Line(Segment2::new(p(0.0, 0.0), p(40.0, 0.0))));
        assert_eq!(
            resolve_anchor(&line, &along(1, 0.25, 0)),
            Some(p(10.0, 0.0))
        );
        assert_eq!(
            resolve_anchor(&line, &along(1, 1.5, 0)),
            Some(p(40.0, 0.0)),
            "clamped to the end"
        );

        // A quarter turn from east, radius 10: halfway is at 45 degrees.
        let arc = entity_of(
            2,
            Geometry::Arc(Arc2::new(p(0.0, 0.0), 10.0, 0.0, 0.5 * PI)),
        );
        let half = resolve_anchor(&arc, &along(2, 0.5, 0)).unwrap();
        near(half.x, 10.0 * (0.25 * PI).cos(), 1e-12);
        near(half.y, 10.0 * (0.25 * PI).sin(), 1e-12);

        // A quarter of a turn counter-clockwise from east is north.
        let circle = entity_of(3, Geometry::Circle(Circle2::new(p(0.0, 0.0), 5.0)));
        let north = resolve_anchor(&circle, &along(3, 0.25, 0)).unwrap();
        near(north.x, 0.0, 1e-12);
        near(north.y, 5.0, 1e-12);

        // Segment 1 of (0,0)-(30,0)-(30,40), halfway: (30, 20).
        let pipe = entity_of(4, polyline(&[(0.0, 0.0), (30.0, 0.0), (30.0, 40.0)], false));
        assert_eq!(
            resolve_anchor(&pipe, &along(4, 0.5, 1)),
            Some(p(30.0, 20.0))
        );
        assert!(
            resolve_anchor(&pipe, &along(4, 0.5, 2)).is_none(),
            "an open polyline of 3 has 2 segments"
        );
        assert!(
            resolve_anchor(
                &entity_of(
                    5,
                    Geometry::Point(PointGeometry {
                        position: p(1.0, 1.0)
                    })
                ),
                &along(5, 0.5, 0)
            )
            .is_none()
        );
    }

    #[test]
    fn inside_is_where_an_area_label_goes() {
        let lot = entity_of(
            1,
            polyline(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], true),
        );
        assert_eq!(
            resolve_anchor(&lot, &at(1, AnchorPoint::Inside)),
            Some(p(5.0, 5.0))
        );
        // An L of two 2-wide legs: its centroid, (29/9, 29/9) by the two
        // rectangles' moments, is in the notch, and the point given must still
        // be inside.
        let ell = Polyline2::new(
            vec![
                p(0.0, 0.0),
                p(10.0, 0.0),
                p(10.0, 2.0),
                p(2.0, 2.0),
                p(2.0, 10.0),
                p(0.0, 10.0),
            ],
            true,
        );
        assert!(
            !ell.contains(ell.centroid().unwrap()),
            "the premise: the centroid is in the notch"
        );
        // The horizontal y = 29/9 crosses the L only at x = 0 and x = 2: one run
        // inside, whose middle is x = 1.
        let inside = inside_point(&ell);
        assert!(ell.contains(inside));
        assert_eq!(inside.x, 1.0);
        near(inside.y, 29.0 / 9.0, 1e-12);
        assert_eq!(
            resolve_anchor(
                &entity_of(6, Geometry::Polyline(ell)),
                &at(6, AnchorPoint::Inside)
            ),
            Some(inside)
        );
        // Two vertices closed are no figure; three in a row have no area, and
        // their inside point is still one on them, the same each time.
        assert!(
            resolve_anchor(
                &entity_of(7, polyline(&[(0.0, 0.0), (10.0, 0.0)], true)),
                &at(7, AnchorPoint::Inside)
            )
            .is_none()
        );
        let flat = Polyline2::new(vec![p(0.0, 0.0), p(5.0, 0.0), p(10.0, 0.0)], true);
        let on_flat = inside_point(&flat);
        assert_eq!(on_flat.y, 0.0);
        assert!(on_flat.x >= 0.0);
        assert!(on_flat.x <= 10.0);
        assert_eq!(inside_point(&flat), on_flat);
        assert_eq!(
            resolve_anchor(
                &entity_of(2, Geometry::Circle(Circle2::new(p(3.0, 4.0), 2.0))),
                &at(2, AnchorPoint::Inside)
            ),
            Some(p(3.0, 4.0))
        );
        let open = entity_of(3, polyline(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false));
        assert!(
            resolve_anchor(&open, &at(3, AnchorPoint::Inside)).is_none(),
            "an open line has no inside"
        );
    }

    #[test]
    fn the_nearest_place_is_named_so_it_follows() {
        let line = nearest_anchor(
            &entity_of(7, Geometry::Line(Segment2::new(p(0.0, 0.0), p(40.0, 0.0)))),
            p(10.0, 3.0),
        )
        .unwrap();
        assert_eq!(line.entity, 7);
        assert_eq!(line.point, AnchorPoint::Along);
        double_eq(line.parameter, 0.25);

        // (1,1) is nearer segment 0 of the pipe, (29,20) nearer segment 1 at
        // 20/40.
        let pipe = entity_of(4, polyline(&[(0.0, 0.0), (30.0, 0.0), (30.0, 40.0)], false));
        assert_eq!(nearest_anchor(&pipe, p(1.0, 1.0)).unwrap().index, 0);
        let on_second = nearest_anchor(&pipe, p(29.0, 20.0)).unwrap();
        assert_eq!(on_second.index, 1);
        double_eq(on_second.parameter, 0.5);
        // Exactly at the shared vertex both segments are as near: the first
        // wins.
        let corner = nearest_anchor(&pipe, p(30.0, 0.0)).unwrap();
        assert_eq!(corner.index, 0);
        double_eq(corner.parameter, 1.0);
        // A repeated vertex has no "along"; the segment after it is used.
        let repeated = entity_of(5, polyline(&[(0.0, 0.0), (0.0, 0.0), (10.0, 0.0)], false));
        assert_eq!(nearest_anchor(&repeated, p(0.0, 0.0)).unwrap().index, 1);

        // West of a circle's centre is half a turn; the turn is [0, 1).
        let circle = entity_of(3, Geometry::Circle(Circle2::new(p(0.0, 0.0), 5.0)));
        double_eq(
            nearest_anchor(&circle, p(-9.0, 0.0)).unwrap().parameter,
            0.5,
        );
        let east = nearest_anchor(&circle, p(9.0, -1e-300)).unwrap();
        assert!(east.parameter < 1.0);

        // At a circle's centre every direction is as near: atan2(+0, +0) is +0
        // (IEEE 754, C Annex F), the turn's start.
        let centred = entity_of(3, Geometry::Circle(Circle2::new(p(2.0, 3.0), 5.0)));
        assert_eq!(
            nearest_anchor(&centred, p(2.0, 3.0)).unwrap().parameter,
            0.0
        );

        let point = entity_of(
            8,
            Geometry::Point(PointGeometry {
                position: p(1.0, 1.0),
            }),
        );
        assert_eq!(
            nearest_anchor(&point, p(9.0, 9.0)).unwrap().point,
            AnchorPoint::Position
        );
        // A polyline with no piece of any length, one of one vertex, a label
        // and a leader offer no place.
        assert!(
            nearest_anchor(
                &entity_of(10, polyline(&[(1.0, 1.0), (1.0, 1.0), (1.0, 1.0)], true)),
                p(0.0, 0.0)
            )
            .is_none()
        );
        assert!(
            nearest_anchor(&entity_of(11, polyline(&[(1.0, 1.0)], false)), p(0.0, 0.0)).is_none()
        );
        assert!(
            nearest_anchor(
                &entity_of(12, Geometry::Label(LabelGeometry::default())),
                p(0.0, 0.0)
            )
            .is_none()
        );
        assert!(
            nearest_anchor(
                &entity_of(
                    13,
                    Geometry::Leader(LeaderGeometry {
                        vertices: vec![p(0.0, 0.0), p(5.0, 5.0)],
                        ..LeaderGeometry::default()
                    })
                ),
                p(0.0, 0.0)
            )
            .is_none()
        );
        assert!(
            nearest_anchor(
                &entity_of(
                    9,
                    Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 2.0, ""))
                ),
                p(1.0, 1.0)
            )
            .is_none()
        );
        assert_eq!(describe(&along(1, 0.25, 3)), "along 3 0.25");
        assert_eq!(describe(&at(1, AnchorPoint::Inside)), "inside");
        assert_eq!(
            anchor_point_from_string("Along").unwrap(),
            AnchorPoint::Along
        );
        assert_eq!(
            anchor_point_from_string("inside").unwrap(),
            AnchorPoint::Inside
        );
    }

    #[test]
    fn a_fraction_that_is_no_number_reads_as_the_start() {
        // A stored reference is never refused for its fraction: one that is
        // not a number, or infinite either way, is 0, the start.
        let line = entity_of(1, Geometry::Line(Segment2::new(p(2.0, 3.0), p(42.0, 3.0))));
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                resolve_anchor(&line, &along(1, bad, 0)),
                Some(p(2.0, 3.0)),
                "{bad}"
            );
        }
        assert_eq!(
            resolve_anchor(&line, &along(1, 7.0, 0)),
            Some(p(42.0, 3.0)),
            "a finite one is clamped"
        );
    }

    #[test]
    fn places_far_from_the_origin_are_the_same_as_near_it() {
        // At a national grid's magnitudes every coordinate here is an integer,
        // exact in a double, and so are the answers.
        let line = entity_of(
            1,
            Geometry::Line(Segment2::new(
                p(334000.0, 6250000.0),
                p(334040.0, 6250000.0),
            )),
        );
        assert_eq!(
            resolve_anchor(&line, &along(1, 0.25, 0)),
            Some(p(334010.0, 6250000.0))
        );
        assert_eq!(
            nearest_anchor(&line, p(334010.0, 6250003.0))
                .unwrap()
                .parameter,
            0.25
        );
        let lot = entity_of(
            2,
            polyline(
                &[
                    (334000.0, 6250000.0),
                    (334010.0, 6250000.0),
                    (334010.0, 6250010.0),
                    (334000.0, 6250010.0),
                ],
                true,
            ),
        );
        assert_eq!(
            resolve_anchor(&lot, &at(2, AnchorPoint::Inside)),
            Some(p(334005.0, 6250005.0))
        );
        let values = values_at(&line, along(1, 0.25, 0), p(334010.0, 6250000.0));
        assert_eq!(said(&values, "chainage"), "10.000");
    }

    // The C++ also gives the tip an AnchorPoint of 9, which the enumeration
    // cannot hold (the blob reader refuses such a byte: SmartLeaders).
    #[test]
    fn only_along_has_a_fraction_and_it_is_from_zero_to_one() {
        let mut leader = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(5.0, 5.0)],
            ..LeaderGeometry::default()
        };
        leader.tip_ref = along(1, 0.5, 0);
        assert!(validate(&Geometry::Leader(leader.clone())).is_ok());
        leader.tip_ref = along(1, 1.0000001, 0);
        assert!(validate(&Geometry::Leader(leader.clone())).is_err());
        leader.tip_ref = along(1, f64::NAN, 0);
        assert!(validate(&Geometry::Leader(leader.clone())).is_err());
        leader.tip_ref = AnchorRef {
            entity: 1,
            point: AnchorPoint::End,
            index: 0,
            parameter: 0.5,
        };
        assert!(
            validate(&Geometry::Leader(leader)).is_err(),
            "an end has no fraction"
        );

        let mut dimension = DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 2.0, "");
        dimension.start_ref = along(1, 2.0, 0);
        assert!(
            validate(&Geometry::Dimension(dimension)).is_err(),
            "a dimension's anchors obey the same rule"
        );
    }
}

// ---- what a leader can say --------------------------------------------------------------

mod leader_values {
    use super::*;

    #[test]
    fn a_tip_on_a_repeated_vertex_has_no_bearing_or_grade_but_keeps_its_chainage() {
        // (0,0) -> (30,0) -> (30,0) -> (30,40) at RL 100, 103, 103, 111. Vertex
        // 1 starts segment 2, which has no length: no direction, so no bearing
        // and no grade (absent is not zero); distance 0, dz 0, chainage 30, RL
        // 103; the whole line 30 + 0 + 40 = 70.
        let mut pipe = entity_of(
            4,
            polyline(&[(0.0, 0.0), (30.0, 0.0), (30.0, 0.0), (30.0, 40.0)], false),
        );
        set_heights(
            &mut pipe.properties,
            &[Some(100.0), Some(103.0), Some(103.0), Some(111.0)],
        );
        let values = values_at(&pipe, indexed(4, AnchorPoint::Vertex, 1), p(30.0, 0.0));
        assert_eq!(said(&values, "segment"), "2");
        assert_eq!(said(&values, "bearing"), "absent");
        assert_eq!(said(&values, "grade"), "absent");
        assert_eq!(said(&values, "distance"), "0.000");
        assert_eq!(said(&values, "dz"), "0.000");
        assert_eq!(said(&values, "chainage"), "30.000");
        assert_eq!(said(&values, "z"), "103.000");
        assert_eq!(said(&values, "length"), "70.000");

        // A fraction that is not a number is its segment's start: segment 3
        // from (30,0), due north, chainage 30 + 0 = 30.
        let nan = values_at(&pipe, along(4, f64::NAN, 2), p(30.0, 0.0));
        assert_eq!(said(&nan, "segment"), "3");
        assert_eq!(said(&nan, "chainage"), "30.000");
        assert_eq!(said(&nan, "x"), "30.000");
        assert_eq!(said(&nan, "bearing"), "0\u{b0}00'00\"");
    }

    #[test]
    fn the_template_is_checked_against_the_leaders_values() {
        assert!(
            check_leader_template("IL {prop.invert:.3f}\n{length:.2f} m {bearing:dms}").is_ok()
        );
        assert!(check_leader_template("{chainage:ch} {area:ha:.4f} {z:.3f} {type:upper}").is_ok());
        let unknown = check_leader_template("{invert}").unwrap_err();
        assert!(
            unknown.context.contains("a leader has no value 'invert'"),
            "{}",
            unknown.describe()
        );
        assert!(
            check_leader_template("{layer:.2f}").is_err(),
            "a number format on text"
        );
        assert!(
            check_leader_template("{bearing:.2f}").is_err(),
            "an angle needs deg first"
        );
        assert!(
            check_leader_template("{prop.x:dms}").is_err(),
            "dms applies to no property"
        );
        assert!(
            check_leader_template("{length").is_err(),
            "braces must balance"
        );
        // Every name listed has a quantity, and nothing else does.
        for name in leader_value_names() {
            assert!(leader_value_quantity(name).is_some(), "{name}");
        }
        assert!(leader_value_quantity("prop.x").is_none());
        assert_eq!(template_fields("{a} {b:.1f}\n{a}{{c}}"), vec!["a", "b"]);
    }

    #[test]
    fn a_point_says_its_level_only_when_it_has_one() {
        let mut pit = entity_of(
            4,
            Geometry::Point(PointGeometry {
                position: p(100.0, 200.0),
            }),
        );
        pit.layer = String::from("PITS");
        pit.properties
            .insert(String::from("elevation"), PropertyValue::Real(12.25));
        pit.properties
            .insert(String::from("invert"), PropertyValue::Real(10.5));
        pit.properties
            .insert(String::from("pitcode"), PropertyValue::from("SMH"));
        let codes = [String::from("pitcode")];
        let values = anchor_values(
            &pit,
            &at(4, AnchorPoint::Position),
            p(100.0, 200.0),
            &codes,
            &LeaderContext::default(),
        );
        assert_eq!(said(&values, "type"), "Point");
        assert_eq!(said(&values, "id"), "4");
        assert_eq!(said(&values, "layer"), "PITS");
        assert_eq!(said(&values, "code"), "SMH");
        assert_eq!(said(&values, "x"), "100.000");
        assert_eq!(said(&values, "northing"), "200.000");
        assert_eq!(said(&values, "rl"), "12.250");
        assert_eq!(said(&values, "prop.invert"), "10.500");
        assert_eq!(said(&values, "bearing"), "absent");

        pit.properties.remove("elevation");
        assert_eq!(
            said(
                &values_at(&pit, at(4, AnchorPoint::Position), p(100.0, 200.0)),
                "z"
            ),
            "absent",
            "absent is not zero"
        );
    }

    #[test]
    fn a_line_says_its_bearing_and_the_level_and_chainage_at_the_tip() {
        // East 40 m, from RL 10 to RL 20: grade 10 / 40 = 25 %, and a quarter
        // of the way along is chainage 10 at RL 10 + 0.25 x 10 = 12.5.
        let mut line = entity_of(1, Geometry::Line(Segment2::new(p(0.0, 0.0), p(40.0, 0.0))));
        set_heights(&mut line.properties, &[Some(10.0), Some(20.0)]);
        let values = values_at(&line, along(1, 0.25, 0), p(10.0, 0.0));
        assert_eq!(said(&values, "bearing"), "90\u{b0}00'00\"");
        assert_eq!(said(&values, "distance"), "40.000");
        assert_eq!(said(&values, "length"), "40.000");
        assert_eq!(said(&values, "chainage"), "10.000");
        assert_eq!(said(&values, "z"), "12.500");
        assert_eq!(said(&values, "dz"), "10.000");
        assert_eq!(said(&values, "grade"), "25.000");
        assert_eq!(said(&values, "segment"), "1");
        assert_eq!(said(&values, "x"), "10.000");

        // Only the start has a level: the tip at the start has it, the middle
        // not.
        let mut half = entity_of(2, Geometry::Line(Segment2::new(p(0.0, 0.0), p(40.0, 0.0))));
        set_heights(&mut half.properties, &[Some(10.0), None]);
        assert_eq!(
            said(
                &values_at(&half, at(2, AnchorPoint::Start), p(0.0, 0.0)),
                "z"
            ),
            "10.000"
        );
        let middle = values_at(&half, at(2, AnchorPoint::Mid), p(20.0, 0.0));
        assert_eq!(said(&middle, "z"), "absent");
        assert_eq!(said(&middle, "grade"), "absent");
        assert_eq!(said(&middle, "chainage"), "20.000");
    }

    #[test]
    fn an_arc_says_its_curve_data_and_how_far_along_the_tip_is() {
        // A quarter circle of radius 10 from east to north: length 5 pi, chord
        // 10 root 2, tangent 10 tan 45 = 10, the chord from (10,0) to (0,10)
        // bearing 315; halfway along is chainage 2.5 pi.
        let arc = entity_of(
            3,
            Geometry::Arc(Arc2::new(p(0.0, 0.0), 10.0, 0.0, 0.5 * PI)),
        );
        let values = values_at(&arc, along(3, 0.5, 0), p(7.0, 7.0));
        double_eq(number(&values, "radius"), 10.0);
        double_eq(number(&values, "diameter"), 20.0);
        near(number(&values, "length"), 5.0 * PI, 1e-12);
        near(number(&values, "chainage"), 2.5 * PI, 1e-12);
        near(number(&values, "chord"), 10.0 * 2.0_f64.sqrt(), 1e-12);
        near(number(&values, "tangent"), 10.0, 1e-12);
        assert_eq!(said(&values, "delta"), "90\u{b0}00'00\"");
        assert_eq!(said(&values, "bearing"), "315\u{b0}00'00\"");
        // At the centre the tip is on no part of the curve: no chainage.
        assert_eq!(
            said(
                &values_at(&arc, at(3, AnchorPoint::Centre), p(0.0, 0.0)),
                "chainage"
            ),
            "absent"
        );
    }

    #[test]
    fn a_circle_says_its_size_and_area() {
        let values = values_at(
            &entity_of(3, Geometry::Circle(Circle2::new(p(0.0, 0.0), 5.0))),
            along(3, 0.25, 0),
            p(0.0, 5.0),
        );
        double_eq(number(&values, "radius"), 5.0);
        double_eq(number(&values, "diameter"), 10.0);
        near(number(&values, "area"), 25.0 * PI, 1e-12);
        near(number(&values, "perimeter"), 10.0 * PI, 1e-12);
        near(number(&values, "length"), 10.0 * PI, 1e-12);
        near(number(&values, "y"), 5.0, 1e-12); // the tip, a quarter turn round
        assert_eq!(
            said(&values, "chainage"),
            "absent",
            "a circle has no start to count from"
        );
    }

    #[test]
    fn a_polyline_says_the_segment_the_tip_is_on_and_its_whole_length() {
        // (0,0) -> (30,0) -> (30,40) at RL 100, 103, 111. Halfway up segment 2
        // (index 1): bearing 0 (north), 40 long, dz 8, grade 8 / 40 = 20 %,
        // chainage 30 + 20 = 50, RL 103 + 4 = 107; the whole line 70 long.
        let mut pipe = entity_of(4, polyline(&[(0.0, 0.0), (30.0, 0.0), (30.0, 40.0)], false));
        set_heights(
            &mut pipe.properties,
            &[Some(100.0), Some(103.0), Some(111.0)],
        );
        let values = values_at(&pipe, along(4, 0.5, 1), p(30.0, 20.0));
        assert_eq!(said(&values, "segment"), "2");
        assert_eq!(said(&values, "bearing"), "0\u{b0}00'00\"");
        assert_eq!(said(&values, "distance"), "40.000");
        assert_eq!(said(&values, "dz"), "8.000");
        assert_eq!(said(&values, "grade"), "20.000");
        assert_eq!(said(&values, "chainage"), "50.000");
        assert_eq!(said(&values, "z"), "107.000");
        assert_eq!(
            said(&values, "length"),
            "70.000",
            "the whole line's, not the segment's"
        );
        assert_eq!(said(&values, "vertices"), "3");
        assert_eq!(said(&values, "area"), "absent", "an open line has no area");

        // The end of an open line is the end of its last segment.
        let end = values_at(&pipe, at(4, AnchorPoint::End), p(30.0, 40.0));
        assert_eq!(said(&end, "chainage"), "70.000");
        assert_eq!(said(&end, "z"), "111.000");
        assert_eq!(said(&end, "segment"), "2");

        // A vertex since removed: the place nearest the tip stands for it.
        let stale = values_at(&pipe, indexed(4, AnchorPoint::Vertex, 9), p(31.0, 10.0));
        assert_eq!(said(&stale, "chainage"), "40.000");
        assert_eq!(said(&stale, "x"), "30.000");
    }

    #[test]
    fn inside_a_lot_says_its_area_and_a_level_only_when_it_is_flat() {
        let mut lot = entity_of(
            5,
            polyline(&[(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)], true),
        );
        lot.properties
            .insert(String::from("lot"), PropertyValue::Integer(12));
        let values = values_at(&lot, at(5, AnchorPoint::Inside), p(0.0, 0.0));
        assert_eq!(said(&values, "area"), "200.0");
        assert_eq!(said(&values, "perimeter"), "60.000");
        assert_eq!(said(&values, "x"), "10.000");
        assert_eq!(said(&values, "y"), "5.000");
        assert_eq!(said(&values, "prop.lot"), "12");
        assert_eq!(
            said(&values, "bearing"),
            "absent",
            "inside is on no segment"
        );
        assert_eq!(said(&values, "z"), "absent");
        lot.properties
            .insert(String::from("elevation"), PropertyValue::Real(31.5));
        assert_eq!(
            said(
                &values_at(&lot, at(5, AnchorPoint::Inside), p(0.0, 0.0)),
                "rl"
            ),
            "31.500"
        );
    }

    #[test]
    fn a_text_and_a_dimension_say_what_they_are() {
        let text = values_at(
            &entity_of(
                6,
                Geometry::Text(TextGeometry::new(p(0.0, 0.0), "EX. KERB", 2.5, 0.0)),
            ),
            at(6, AnchorPoint::Position),
            p(0.0, 0.0),
        );
        assert_eq!(said(&text, "text"), "EX. KERB");
        let aligned = values_at(
            &entity_of(
                7,
                Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(12.0, 5.0), 2.0, "")),
            ),
            at(7, AnchorPoint::Position),
            p(0.0, 0.0),
        );
        assert_eq!(said(&aligned, "measurement"), "13.000");
        assert_eq!(said(&aligned, "angle"), "absent");
        let angular = DimensionGeometry {
            kind: DimensionKind::Angular,
            start: p(10.0, 0.0),
            end: p(0.0, 10.0),
            vertex: p(0.0, 0.0),
            ..DimensionGeometry::default()
        };
        let angle = values_at(
            &entity_of(8, Geometry::Dimension(angular)),
            at(8, AnchorPoint::Position),
            p(0.0, 0.0),
        );
        assert_eq!(said(&angle, "angle"), "90\u{b0}00'00\"");
    }

    #[test]
    fn the_note_is_read_off_the_target_and_a_line_with_nothing_to_say_is_dropped() {
        let mut pit = entity_of(
            0,
            Geometry::Point(PointGeometry {
                position: p(10.0, 10.0),
            }),
        );
        pit.properties
            .insert(String::from("invert"), PropertyValue::Real(10.25));
        let mut model = model_with(vec![pit]);
        let id = model.entities.ids()[0];
        let none = LeaderContext::default();

        let leader = smart_leader(
            id,
            AnchorRef::default(),
            p(10.0, 10.0),
            "PIT {id}\nIL {prop.invert:.2f}\nRL {rl:.3f}",
        );
        assert_eq!(
            leader_note(&model, &leader, &[], &none),
            format!("PIT {id}\nIL 10.25"),
            "no RL: that line is dropped"
        );
        assert_eq!(
            missing_values(
                &leader.text,
                &leader_values(&model, &leader, &[], &none).unwrap()
            ),
            vec!["rl"]
        );

        // A plain leader's text is its text, braces and all.
        let mut plain = leader.clone();
        plain.fields = false;
        assert_eq!(leader_note(&model, &plain, &[], &none), plain.text);

        // A label style's template, read live.
        let style = LabelStyle {
            name: String::from("Invert"),
            kind: LabelKind::Point,
            text: String::from("INV {prop.invert:.3f}"),
            ..LabelStyle::default()
        };
        assert!(model.label_styles.add(style).is_ok());
        let mut styled = leader.clone();
        styled.fields = false;
        styled.text.clear();
        styled.label_style = String::from("Invert");
        assert!(validate(&Geometry::Leader(styled.clone())).is_ok());
        assert_eq!(leader_note(&model, &styled, &[], &none), "INV 10.250");
        styled.label_style = String::from("Gone");
        assert_eq!(
            leader_note(&model, &styled, &[], &none),
            "",
            "a missing style says nothing"
        );
        assert!(check_leader_says_something(&model, &styled, &[], &none).is_err());
    }

    #[test]
    fn a_leader_that_would_say_nothing_is_refused_naming_what_is_missing() {
        let model = model_with(vec![entity_of(
            0,
            Geometry::Point(PointGeometry {
                position: p(0.0, 0.0),
            }),
        )]);
        let id = model.entities.ids()[0];
        let none = LeaderContext::default();
        let leader = smart_leader(id, AnchorRef::default(), p(0.0, 0.0), "IL {prop.invrt}");
        let refused = check_leader_says_something(&model, &leader, &[], &none).unwrap_err();
        assert!(
            refused.message.contains("no prop.invrt"),
            "{}",
            refused.describe()
        );
        assert!(refused.context.contains("type=Point"));

        let mut nowhere = leader.clone();
        nowhere.tip_ref = AnchorRef::default();
        assert!(check_leader_says_something(&model, &nowhere, &[], &none).is_err());
        let mut gone = leader.clone();
        gone.tip_ref.entity = 999;
        assert_eq!(
            check_leader_says_something(&model, &gone, &[], &none)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let plain = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(1.0, 1.0)],
            text: String::from("{nothing}"),
            ..LeaderGeometry::default()
        };
        assert!(
            check_leader_says_something(&model, &plain, &[], &none).is_ok(),
            "a plain note is what it is"
        );
    }
}

// ---- validation and storage ---------------------------------------------------------------

mod smart_leaders {
    use super::*;

    #[test]
    fn validation_keeps_one_template_and_a_tip_to_read_from() {
        let leader = smart_leader(3, AnchorRef::default(), p(0.0, 0.0), "{length}");
        assert!(validate(&Geometry::Leader(leader.clone())).is_ok());

        let mut no_tip = leader.clone();
        no_tip.tip_ref = AnchorRef::default();
        assert!(
            validate(&Geometry::Leader(no_tip)).is_err(),
            "a smart leader reads the entity its tip is on"
        );

        let mut both = leader.clone();
        both.label_style = String::from("Invert");
        assert!(
            validate(&Geometry::Leader(both)).is_err(),
            "its own template or its style's, not both"
        );

        let mut styled_with_text = leader.clone();
        styled_with_text.fields = false;
        styled_with_text.label_style = String::from("Invert");
        assert!(
            validate(&Geometry::Leader(styled_with_text)).is_err(),
            "a styled leader has no text of its own"
        );

        let mut bad_template = leader.clone();
        bad_template.text = String::from("{invert}");
        let refused = validate(&Geometry::Leader(bad_template)).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);

        let mut empty = leader;
        empty.text.clear();
        assert!(validate(&Geometry::Leader(empty)).is_err());
    }

    #[test]
    fn a_version_three_blob_holds_them_and_nothing_else_needs_one() {
        let mut smart = smart_leader(3, along(3, 0.375, 2), p(1.0, 2.0), "IL {prop.invert}");
        smart.callout = CalloutShape::Box;
        let mut styled = smart_leader(4, at(4, AnchorPoint::Inside), p(1.0, 2.0), "");
        styled.fields = false;
        styled.label_style = String::from("Lot");
        let plain_along = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0)],
            text: String::from("A"),
            tip_ref: along(9, 0.5, 0),
            ..LeaderGeometry::default()
        };
        let mut dimension = DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 2.0, "");
        dimension.end_ref = along(5, 1.0, 1);
        for geometry in [
            Geometry::Leader(smart),
            Geometry::Leader(styled),
            Geometry::Leader(plain_along),
            Geometry::Dimension(dimension),
        ] {
            let blob = geometry_to_blob(&geometry).unwrap();
            assert_eq!(blob[0], BLOB_VERSION_SMART_LEADER);
            let back =
                geometry_from_blob(&blob).unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(back, geometry);

            let json = geometry_to_json(&geometry).unwrap();
            let from_json = geometry_from_json(&json).unwrap_or_else(|_| panic!("{json}"));
            assert_eq!(from_json, geometry, "{json}");
        }

        // A plain leader with an old anchor is still version 2, byte for byte:
        // header 2, vertices 4 + 32, text 4 + 1, style 4, arrow and callout 2,
        // three sizes 24, the anchor 8 + 1 + 4.
        let plain = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0)],
            text: String::from("A"),
            tip_ref: indexed(3, AnchorPoint::Vertex, 7),
            ..LeaderGeometry::default()
        };
        let old = geometry_to_blob(&Geometry::Leader(plain.clone())).unwrap();
        assert_eq!(old[0], BLOB_VERSION_ANNOTATION);
        assert_eq!(old.len(), 2 + 36 + 5 + 4 + 2 + 24 + 13);
        let json = geometry_to_json(&Geometry::Leader(plain)).unwrap();
        assert!(!json.contains("fields"));
        assert!(!json.contains("parameter"));
    }

    #[test]
    fn a_negative_zero_fraction_keeps_its_bits() {
        let leader = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0)],
            tip_ref: AnchorRef {
                entity: 2,
                point: AnchorPoint::Start,
                index: 0,
                parameter: -0.0,
            },
            ..LeaderGeometry::default()
        };
        assert!(
            validate(&Geometry::Leader(leader.clone())).is_ok(),
            "-0 is 0"
        );
        let blob = geometry_to_blob(&Geometry::Leader(leader)).unwrap();
        assert_eq!(
            blob[0], BLOB_VERSION_SMART_LEADER,
            "version 2 cannot hold the sign"
        );
        let Geometry::Leader(back) = geometry_from_blob(&blob).unwrap() else {
            panic!("not a leader")
        };
        assert!(back.tip_ref.parameter.is_sign_negative());
    }

    #[test]
    fn a_malformed_blob_is_refused() {
        // A version-2 anchor naming a point version 2 did not have.
        let plain = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0)],
            text: String::from("A"),
            tip_ref: at(3, AnchorPoint::End),
            ..LeaderGeometry::default()
        };
        let mut v2 = geometry_to_blob(&Geometry::Leader(plain)).unwrap();
        assert_eq!(v2[0], BLOB_VERSION_ANNOTATION);
        let at_point = v2.len() - 5;
        v2[at_point] = AnchorPoint::Along as u8;
        assert!(geometry_from_blob(&v2).is_err());

        let smart = smart_leader(3, along(3, 0.5, 0), p(1.0, 2.0), "{length}");
        let v3 = geometry_to_blob(&Geometry::Leader(smart)).unwrap();
        let mut cut = v3.clone();
        cut.pop();
        assert!(geometry_from_blob(&cut).is_err());
        // The fields byte (before the empty label style's 4-byte length) is 0
        // or 1.
        let mut flag = v3.clone();
        let at_flag = flag.len() - 5;
        flag[at_flag] = 2;
        assert!(geometry_from_blob(&flag).is_err());
        let mut future = v3;
        future[0] = 4;
        assert!(
            geometry_from_blob(&future).is_err(),
            "a version from a newer build"
        );
    }
}
