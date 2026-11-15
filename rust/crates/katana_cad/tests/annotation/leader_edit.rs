// The leader edits the LEADER verbs and the window's Leaders manager share
// (annotation/leader_edit.rs, docs/annotation.md "Smart leaders";
// tests/cad/annotation/test_leader_edit.cpp): each checked against the model
// and made ONE command, or refused in the words the command line replies with.

use katana_cad::Document;
use katana_cad::annotation::dimension_build::AnchoredPoint;
use katana_cad::annotation::leader_edit::{
    BalloonOrder, LeaderChange, LeaderNote, LeaderNoteKind, LeadersForOptions, align_leaders,
    apply_leader_change, attach_leader, change_leaders, check_leader_change, leader_place_on,
    leader_says, leader_target_values, leader_value_rows, leaders_for, new_leader,
    next_balloon_number, note_of, renumber_balloons, set_leader_target_property, tip_place_on,
};
use katana_commands::{EntityAttributes, create_entities, create_label_style, create_text_style};
use katana_core::ErrorCode;
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, DimensionGeometry, Entity, EntityId, Geometry,
    LabelKind, LabelStyle, LeaderContext, LeaderGeometry, PointGeometry, PropertyMap,
    PropertyValue, TextStyle,
};
use katana_geometry::{Arc2, Circle2, Point2, Polyline2, Segment2};
use katana_math::numerics::PI;

use crate::{near, ok, refused};

#[track_caller]
fn add(document: &mut Document, geometry: Geometry, properties: PropertyMap) -> EntityId {
    let entity = Entity {
        geometry,
        properties,
        ..Entity::default()
    };
    ok(document.execute(create_entities(vec![entity])));
    document.last_created_entities()[0]
}

fn properties(pairs: &[(&str, f64)]) -> PropertyMap {
    pairs
        .iter()
        .map(|&(key, value)| (String::from(key), PropertyValue::Real(value)))
        .collect()
}

fn point(x: f64, y: f64) -> Geometry {
    Geometry::Point(PointGeometry {
        position: Point2::new(x, y),
    })
}

#[track_caller]
fn leader_of(document: &Document, id: EntityId) -> &LeaderGeometry {
    match document
        .model()
        .entities
        .find(id)
        .map(|entity| &entity.geometry)
    {
        Some(Geometry::Leader(leader)) => leader,
        other => panic!("not a leader: {other:?}"),
    }
}

fn template_change(text: &str) -> LeaderChange {
    LeaderChange {
        note: Some(LeaderNote {
            kind: LeaderNoteKind::Template,
            text: String::from(text),
        }),
        ..LeaderChange::default()
    }
}

fn on(entity: EntityId) -> AnchorRef {
    AnchorRef {
        entity,
        ..AnchorRef::default()
    }
}

fn anchored(x: f64, y: f64, reference: AnchorRef) -> AnchoredPoint {
    AnchoredPoint {
        point: Point2::new(x, y),
        r#ref: reference,
    }
}

fn plain_leader(vertices: Vec<Point2>, text: &str) -> Geometry {
    Geometry::Leader(LeaderGeometry {
        vertices,
        text: String::from(text),
        ..LeaderGeometry::default()
    })
}

fn none() -> LeaderContext {
    LeaderContext::default()
}

mod leader_edit {
    use super::*;

    #[test]
    fn an_unchanged_edit_is_no_command_and_a_change_is_one() {
        let mut document = Document::default();
        let pit = add(
            &mut document,
            point(0.0, 0.0),
            properties(&[("invert", 10.5)]),
        );
        let made = new_leader(
            document.model(),
            &[
                anchored(0.0, 0.0, on(pit)),
                anchored(5.0, 5.0, AnchorRef::default()),
            ],
            &template_change("IL {prop.invert:.2f}"),
            false,
            &none(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let leader = add(&mut document, Geometry::Leader(made), PropertyMap::new());
        assert_eq!(
            leader_says(document.model(), leader_of(&document, leader), &none()),
            "IL 10.50"
        );
        assert_eq!(
            note_of(leader_of(&document, leader)),
            LeaderNote {
                kind: LeaderNoteKind::Template,
                text: String::from("IL {prop.invert:.2f}"),
            }
        );

        let same = change_leaders(
            document.model(),
            vec![leader],
            &template_change("IL {prop.invert:.2f}"),
            &none(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(same.is_none(), "the same template again changes nothing");

        let look = LeaderChange {
            arrow: Some(ArrowHead::Open),
            paper_height: Some(3.5),
            ..LeaderChange::default()
        };
        let changed = change_leaders(document.model(), vec![leader, leader], &look, &none())
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("a command");
        let steps = document.history().undo_count();
        ok(document.execute(changed));
        assert_eq!(document.history().undo_count(), steps + 1);
        assert_eq!(leader_of(&document, leader).arrow, ArrowHead::Open);
        assert_eq!(leader_of(&document, leader).paper_height, 3.5);
        assert!(
            leader_of(&document, leader).fields,
            "the note is left as it was"
        );

        assert!(
            change_leaders(document.model(), vec![pit], &look, &none()).is_err(),
            "a point is no leader"
        );
        assert!(change_leaders(document.model(), vec![], &look, &none()).is_err());
    }

    #[test]
    fn what_cannot_be_made_is_refused_not_skipped() {
        let mut document = Document::default();
        let a = add(&mut document, point(0.0, 0.0), PropertyMap::new());
        let b = add(
            &mut document,
            point(10.0, 0.0),
            properties(&[("invert", 9.0)]),
        );
        let attributes = EntityAttributes::default();
        // No such value: every leader would be wrong.
        let mut options = LeadersForOptions {
            change: template_change("{invert}"),
            ..LeadersForOptions::default()
        };
        let error = refused(leaders_for(
            document.model(),
            vec![a, b],
            &options,
            1000.0,
            &attributes,
            &none(),
        ));
        assert!(
            error.context.contains("no value 'invert'"),
            "{}",
            error.describe()
        );

        // A note one target cannot say is that target skipped.
        options.change = template_change("IL {prop.invert}");
        let made = leaders_for(
            document.model(),
            vec![a, b],
            &options,
            1000.0,
            &attributes,
            &none(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(made.made, 1);
        assert_eq!(made.skipped, 1);
        assert!(
            made.first_skip.contains("no prop.invert"),
            "{}",
            made.first_skip
        );

        let missing_style = LeaderChange {
            text_style: Some(String::from("Nowhere")),
            ..LeaderChange::default()
        };
        assert_eq!(
            check_leader_change(document.model(), &missing_style)
                .expect_err("no such style")
                .code,
            ErrorCode::NotFound
        );

        // 0 mm is a size (the style's height, no landing); below it is none.
        let nothing = LeaderChange {
            paper_height: Some(0.0),
            landing: Some(0.0),
            ..LeaderChange::default()
        };
        assert!(check_leader_change(document.model(), &nothing).is_ok());
        for bad in [-0.5, f64::NAN, f64::INFINITY] {
            let landing = LeaderChange {
                landing: Some(bad),
                ..LeaderChange::default()
            };
            assert_eq!(
                check_leader_change(document.model(), &landing)
                    .expect_err("no landing")
                    .code,
                ErrorCode::InvalidArgument,
                "{bad}"
            );
            let arrow = LeaderChange {
                arrow_size: Some(bad),
                ..LeaderChange::default()
            };
            assert!(
                check_leader_change(document.model(), &arrow).is_err(),
                "{bad}"
            );
            let paper = LeaderChange {
                paper_height: Some(bad),
                ..LeaderChange::default()
            };
            assert!(
                check_leader_change(document.model(), &paper).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_place_for_a_leader_is_on_the_entity_or_there_is_none() {
        let mut document = Document::default();
        let at_point = add(&mut document, point(3.0, 4.0), PropertyMap::new());
        let line = add(
            &mut document,
            Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))),
            PropertyMap::new(),
        );
        let measured = DimensionGeometry {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
            ..DimensionGeometry::default()
        };
        let dimension = add(
            &mut document,
            Geometry::Dimension(measured),
            PropertyMap::new(),
        );
        let entity = |id: EntityId| document.model().entities.find(id).expect("the entity");
        let on_point = leader_place_on(entity(at_point), 0.25 * PI).expect("a place");
        assert_eq!(on_point.point, Point2::new(3.0, 4.0));
        assert_eq!(on_point.r#ref.entity, at_point);
        // A line's middle: half way along it.
        let on_line = leader_place_on(entity(line), 0.25 * PI).expect("a place");
        assert_eq!(on_line.point, Point2::new(5.0, 0.0));
        assert_eq!(on_line.r#ref.point, AnchorPoint::Along);
        assert_eq!(on_line.r#ref.parameter, 0.5);
        assert!(
            leader_place_on(entity(dimension), 0.25 * PI).is_none(),
            "a dimension is annotation, not something to point at"
        );

        // Halfway along an open polyline: (0,0)-(30,0)-(30,40) is 70 long, so
        // 35 along is 5 into segment 1 (40 long), a fraction 5/40 = 0.125:
        // (30,5).
        let pipe = add(
            &mut document,
            Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(30.0, 0.0),
                    Point2::new(30.0, 40.0),
                ],
                false,
            )),
            PropertyMap::new(),
        );
        let entity = |id: EntityId| document.model().entities.find(id).expect("the entity");
        let on_pipe = leader_place_on(entity(pipe), 0.0).expect("a place");
        assert_eq!(on_pipe.point, Point2::new(30.0, 5.0));
        assert_eq!(on_pipe.r#ref.index, 1);
        assert_eq!(on_pipe.r#ref.parameter, 0.125);
        // An arc's middle: a quarter circle of radius 10 from 0 to 90 degrees
        // is at 45 degrees, half way along it.
        let bend = add(
            &mut document,
            Geometry::Arc(Arc2::new(Point2::new(0.0, 0.0), 10.0, 0.0, 0.5 * PI)),
            PropertyMap::new(),
        );
        let on_bend = leader_place_on(document.model().entities.find(bend).expect("the arc"), 0.0)
            .expect("a place");
        assert_eq!(on_bend.r#ref.parameter, 0.5);
        near(on_bend.point.x, 10.0 * (0.25 * PI).cos(), 1e-12);
        near(on_bend.point.y, 10.0 * (0.25 * PI).sin(), 1e-12);
    }

    #[test]
    fn a_piece_within_the_geometric_tolerance_has_no_along_for_leader_for() {
        // (0,0)-(1,0)-(1+5e-8,0)-(2+5e-8,0): the half, 1 + 2.5e-8, falls in
        // the 5e-8 piece, below the geometric tolerance - no direction, so no
        // along, as for nearest_anchor. The tip goes to the next piece's start
        // instead, within 5e-8 of the half.
        let mut document = Document::default();
        let tiny = 5e-8;
        let line = add(
            &mut document,
            Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(1.0 + tiny, 0.0),
                    Point2::new(2.0 + tiny, 0.0),
                ],
                false,
            )),
            PropertyMap::new(),
        );
        let place = leader_place_on(document.model().entities.find(line).expect("the line"), 0.0)
            .expect("a place");
        assert_eq!(place.r#ref.index, 2);
        assert_eq!(place.r#ref.parameter, 0.0);
        // Nothing of any length - which the model will not hold, but a caller
        // may still hand over - is no place at all.
        let dot = Entity {
            id: 99,
            geometry: Geometry::Polyline(Polyline2::new(
                vec![Point2::new(1.0, 1.0), Point2::new(1.0, 1.0)],
                false,
            )),
            ..Entity::default()
        };
        assert!(leader_place_on(&dot, 0.0).is_none());
    }

    #[test]
    fn leaders_for_refuses_no_targets_and_a_length_angle_or_scale_that_is_no_size() {
        let mut document = Document::default();
        let a = add(&mut document, point(0.0, 0.0), PropertyMap::new());
        let options = LeadersForOptions::default();
        let code = |ids: Vec<EntityId>, with: &LeadersForOptions, scale: f64| {
            // None when it was made.
            leaders_for(
                document.model(),
                ids,
                with,
                scale,
                &EntityAttributes::default(),
                &none(),
            )
            .err()
            .map(|error| error.code)
        };
        assert_eq!(
            code(vec![], &options, 1000.0),
            Some(ErrorCode::InvalidArgument)
        );
        for bad in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            let length = LeadersForOptions {
                length: bad,
                ..options.clone()
            };
            assert_eq!(
                code(vec![a], &length, 1000.0),
                Some(ErrorCode::InvalidArgument),
                "{bad}"
            );
        }
        for bad in [f64::NAN, f64::INFINITY] {
            let angle = LeadersForOptions {
                angle: bad,
                ..options.clone()
            };
            assert_eq!(
                code(vec![a], &angle, 1000.0),
                Some(ErrorCode::InvalidArgument),
                "{bad}"
            );
        }
        assert_eq!(
            code(vec![a], &options, 0.0),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(vec![a], &options, -1.0),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(vec![a, 999999], &options, 1000.0),
            Some(ErrorCode::NotFound)
        );
        assert_eq!(code(vec![a], &options, 1000.0), None, "the premise: it can");
    }

    #[test]
    fn aligning_refuses_what_cannot_be_a_column_and_is_no_command_when_nothing_moves() {
        let mut document = Document::default();
        let pit = add(&mut document, point(0.0, 0.0), PropertyMap::new());
        let a = add(
            &mut document,
            plain_leader(vec![Point2::new(0.0, 0.0), Point2::new(20.0, 30.0)], "A"),
            PropertyMap::new(),
        );
        let b = add(
            &mut document,
            plain_leader(vec![Point2::new(5.0, 0.0), Point2::new(20.0, 10.0)], "B"),
            PropertyMap::new(),
        );
        let code =
            |document: &Document, ids: Vec<EntityId>, x: Option<f64>, spacing: Option<f64>| {
                align_leaders(document.model(), ids, x, spacing)
                    .err()
                    .map(|error| error.code)
            };
        assert_eq!(
            code(&document, vec![], None, None),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(&document, vec![pit], None, None),
            Some(ErrorCode::InvalidArgument)
        );
        for bad in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            assert_eq!(
                code(&document, vec![a, b], None, Some(bad)),
                Some(ErrorCode::InvalidArgument),
                "{bad}"
            );
        }
        for bad in [f64::NAN, f64::INFINITY] {
            assert_eq!(
                code(&document, vec![a, b], Some(bad), None),
                Some(ErrorCode::InvalidArgument),
                "{bad}"
            );
        }
        // Finite, yet the lowest of three notes would be stacked past any
        // number: 30 - 2 x DBL_MAX overflows to minus infinity.
        let c = add(
            &mut document,
            plain_leader(vec![Point2::new(9.0, 0.0), Point2::new(20.0, 20.0)], "C"),
            PropertyMap::new(),
        );
        assert_eq!(
            code(&document, vec![a, b, c], None, Some(f64::MAX)),
            Some(ErrorCode::InvalidArgument)
        );
        // Both already hang at x = 20 and x= keeps each one's height: no
        // command.
        let still = align_leaders(document.model(), vec![a, b], Some(20.0), None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(still.command.is_none());
        assert_eq!(still.count, 2);
    }

    #[test]
    fn a_balloon_at_the_largest_number_refuses_the_next_rather_than_wrapping() {
        let largest = i64::MAX;
        let mut document = Document::default();
        let a = add(&mut document, point(0.0, 0.0), PropertyMap::new());
        let b = add(&mut document, point(10.0, 0.0), PropertyMap::new());
        let points = [
            anchored(0.0, 0.0, on(a)),
            anchored(5.0, 5.0, AnchorRef::default()),
        ];
        let balloons = LeadersForOptions {
            balloon: true,
            ..LeadersForOptions::default()
        };
        let attributes = EntityAttributes::default();

        // One short of the largest: one more fits, two do not.
        add(
            &mut document,
            Geometry::Leader(LeaderGeometry {
                vertices: vec![Point2::new(50.0, 0.0), Point2::new(55.0, 5.0)],
                text: format!("{}", largest - 1),
                callout: CalloutShape::Circle,
                ..LeaderGeometry::default()
            }),
            PropertyMap::new(),
        );
        assert_eq!(next_balloon_number(document.model()), Ok(largest));
        let one = leaders_for(
            document.model(),
            vec![a],
            &balloons,
            1000.0,
            &attributes,
            &none(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            leaders_for(
                document.model(),
                vec![a, b],
                &balloons,
                1000.0,
                &attributes,
                &none()
            )
            .is_err()
        );
        ok(document.execute(one.command));
        let made = document.last_created_entities()[0];
        assert_eq!(leader_of(&document, made).text, format!("{largest}"));

        // At the largest there is no next: refused, not wrapped to a negative.
        assert!(next_balloon_number(document.model()).is_err());
        assert!(
            new_leader(
                document.model(),
                &points,
                &LeaderChange::default(),
                true,
                &none()
            )
            .is_err()
        );
        assert!(
            leaders_for(
                document.model(),
                vec![a],
                &balloons,
                1000.0,
                &attributes,
                &none()
            )
            .is_err()
        );
        // A balloon given its note is not numbered, so is made.
        let noted = LeaderChange {
            note: Some(LeaderNote {
                kind: LeaderNoteKind::Text,
                text: String::from("A"),
            }),
            ..LeaderChange::default()
        };
        assert!(new_leader(document.model(), &points, &noted, true, &none()).is_ok());
        // Numbered again from 1, there is a next once more.
        let renumbering = renumber_balloons(document.model(), 1, BalloonOrder::Id, None);
        ok(document.execute(renumbering.command.expect("a renumbering")));
        assert_eq!(next_balloon_number(document.model()), Ok(3));
    }

    #[test]
    fn a_tip_put_on_a_lot_goes_inside_it_when_it_is_inside() {
        let mut document = Document::default();
        // A 10 x 10 lot: inside, the tip points into it - its middle (5,5);
        // outside, it goes to the nearest place on it, (10,5) half way up the
        // east side (segment 1).
        let lot = add(
            &mut document,
            Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                    Point2::new(10.0, 10.0),
                    Point2::new(0.0, 10.0),
                ],
                true,
            )),
            PropertyMap::new(),
        );
        let ring = add(
            &mut document,
            Geometry::Circle(Circle2::new(Point2::new(20.0, 20.0), 4.0)),
            PropertyMap::new(),
        );
        let open = add(
            &mut document,
            Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                    Point2::new(10.0, 10.0),
                ],
                false,
            )),
            PropertyMap::new(),
        );
        let measured = DimensionGeometry {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
            ..DimensionGeometry::default()
        };
        let dimension = add(
            &mut document,
            Geometry::Dimension(measured),
            PropertyMap::new(),
        );
        let entity = |id: EntityId| document.model().entities.find(id).expect("the entity");

        let outline = entity(lot);
        let inside = tip_place_on(outline, Point2::new(3.0, 4.0)).expect("a place");
        assert_eq!(inside.r#ref.point, AnchorPoint::Inside);
        assert_eq!(inside.point, Point2::new(5.0, 5.0));
        let outside = tip_place_on(outline, Point2::new(14.0, 5.0)).expect("a place");
        assert_eq!(outside.r#ref.point, AnchorPoint::Along);
        assert_eq!(outside.r#ref.index, 1);
        assert_eq!(outside.point, Point2::new(10.0, 5.0));
        // A circle's inside is its centre; an open line has none.
        assert_eq!(
            tip_place_on(entity(ring), Point2::new(21.0, 21.0))
                .expect("a place")
                .point,
            Point2::new(20.0, 20.0)
        );
        assert_eq!(
            tip_place_on(entity(open), Point2::new(8.0, 2.0))
                .expect("a place")
                .r#ref
                .point,
            AnchorPoint::Along
        );
        // A dimension offers no place.
        assert!(tip_place_on(entity(dimension), Point2::new(5.0, 1.0)).is_none());
    }

    #[test]
    fn a_label_style_lends_its_look_unless_the_change_gives_one() {
        let mut document = Document::default();
        ok(document.execute(create_text_style(TextStyle {
            name: String::from("Notes"),
            ..TextStyle::default()
        })));
        ok(document.execute(create_label_style(LabelStyle {
            name: String::from("Invert"),
            kind: LabelKind::Point,
            text: String::from("{prop.invert}"),
            text_style: String::from("Notes"),
            paper_height: 3.0,
            ..LabelStyle::default()
        })));
        let pit = add(
            &mut document,
            point(0.0, 0.0),
            properties(&[("invert", 9.5)]),
        );
        let mut leader = LeaderGeometry {
            vertices: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 5.0)],
            tip_ref: on(pit),
            ..LeaderGeometry::default()
        };
        let mut change = LeaderChange {
            note: Some(LeaderNote {
                kind: LeaderNoteKind::LabelStyle,
                text: String::from("Invert"),
            }),
            ..LeaderChange::default()
        };
        ok(apply_leader_change(
            document.model(),
            &change,
            &mut leader,
            0,
            &none(),
        ));
        assert_eq!(leader.style, "Notes");
        assert_eq!(leader.paper_height, 3.0);

        let mut own = LeaderGeometry {
            vertices: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 5.0)],
            tip_ref: on(pit),
            ..LeaderGeometry::default()
        };
        change.text_style = Some(String::new());
        change.paper_height = Some(2.0);
        ok(apply_leader_change(
            document.model(),
            &change,
            &mut own,
            0,
            &none(),
        ));
        assert_eq!(own.style, "", "given: the default face");
        assert_eq!(own.paper_height, 2.0);
    }

    #[test]
    fn attaching_needs_an_entity_and_the_values_are_rows_in_order() {
        let mut document = Document::default();
        let line = add(
            &mut document,
            Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(40.0, 0.0))),
            properties(&[("size", 150.0)]),
        );
        let leader = add(
            &mut document,
            plain_leader(vec![Point2::new(9.0, 9.0), Point2::new(20.0, 20.0)], "X"),
            PropertyMap::new(),
        );
        assert!(
            attach_leader(
                document.model(),
                leader,
                &anchored(1.0, 1.0, AnchorRef::default()),
                &none()
            )
            .is_err(),
            "a plain point is nothing to attach to"
        );
        let place = anchored(
            10.0,
            0.0,
            AnchorRef {
                entity: line,
                point: AnchorPoint::Along,
                index: 0,
                parameter: 0.25,
            },
        );
        let attach = attach_leader(document.model(), leader, &place, &none())
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("a command");
        ok(document.execute(attach));
        assert_eq!(
            leader_of(&document, leader).vertices[0],
            Point2::new(10.0, 0.0)
        );

        let values = leader_target_values(document.model(), leader_of(&document, leader), &none())
            .expect("values");
        let rows = leader_value_rows(&values);
        assert!(!rows.is_empty());
        assert_eq!(rows[0].name, "id", "leader_value_names order");
        let last = &rows[rows.len() - 1];
        assert_eq!(last.name, "prop.size", "then the properties");
        assert_eq!(last.text, "150.000");

        let property = set_leader_target_property(
            document.model(),
            leader,
            "size",
            Some(&PropertyValue::Real(200.0)),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        ok(document.execute(property));
        assert_eq!(
            document
                .model()
                .entities
                .find(line)
                .expect("the line")
                .properties
                .get("size"),
            Some(&PropertyValue::Real(200.0))
        );
        assert!(
            set_leader_target_property(
                document.model(),
                leader,
                "",
                Some(&PropertyValue::Real(1.0))
            )
            .is_err()
        );
    }

    #[test]
    fn a_numbered_balloon_is_a_circle_so_its_number_is_counted() {
        let mut document = Document::default();
        let a = add(&mut document, point(0.0, 0.0), PropertyMap::new());
        let b = add(&mut document, point(10.0, 0.0), PropertyMap::new());
        let points = [
            anchored(0.0, 0.0, on(a)),
            anchored(5.0, 5.0, AnchorRef::default()),
        ];
        let mut boxed = LeaderChange {
            callout: Some(CalloutShape::Box),
            ..LeaderChange::default()
        };
        let error = new_leader(document.model(), &points, &boxed, true, &none())
            .expect_err("a numbered box");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("circle"));
        let options = LeadersForOptions {
            balloon: true,
            change: boxed.clone(),
            ..LeadersForOptions::default()
        };
        assert!(
            leaders_for(
                document.model(),
                vec![a, b],
                &options,
                1000.0,
                &EntityAttributes::default(),
                &none()
            )
            .is_err()
        );

        // Given a note, a balloon is not numbered, and may be any shape.
        boxed.note = Some(LeaderNote {
            kind: LeaderNoteKind::Text,
            text: String::from("A"),
        });
        let noted = new_leader(document.model(), &points, &boxed, true, &none())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(noted.callout, CalloutShape::Box);
        // A circle numbered, and the next one numbered on from it.
        let first = new_leader(
            document.model(),
            &points,
            &LeaderChange::default(),
            true,
            &none(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(first.text, "1");
        add(&mut document, Geometry::Leader(first), PropertyMap::new());
        assert_eq!(next_balloon_number(document.model()), Ok(2));
    }

    #[test]
    fn attaching_a_tip_where_it_is_is_no_command() {
        let mut document = Document::default();
        let line = add(
            &mut document,
            Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(40.0, 0.0))),
            PropertyMap::new(),
        );
        let leader = add(
            &mut document,
            plain_leader(vec![Point2::new(9.0, 9.0), Point2::new(20.0, 20.0)], "X"),
            PropertyMap::new(),
        );
        let place = anchored(
            10.0,
            0.0,
            AnchorRef {
                entity: line,
                point: AnchorPoint::Along,
                index: 0,
                parameter: 0.25,
            },
        );
        let attach = attach_leader(document.model(), leader, &place, &none())
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("a command");
        ok(document.execute(attach));
        let again = attach_leader(document.model(), leader, &place, &none())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(again.is_none(), "on that place already");
    }
}
