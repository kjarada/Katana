// The associative update (annotation/associative.rs, docs/annotation.md
// "Associativity"; tests/cad/annotation/test_associative.cpp): dimensions,
// leaders and labels follow the geometry they refer to through ordinary
// edits, in the SAME undo step, and one undo puts back the geometry and
// everything that followed it.

use katana_cad::Document;
use katana_cad::annotation::associative::associative_changes;
use katana_cad::annotation::dimension_build::{
    AnchoredPoint, aligned_dimension, angular_between_lines, linear_dimension, radial_dimension,
};
use katana_commands::{
    create_entities, create_label_style, create_layer, delete_entities, move_entities,
    set_entity_geometry, update_layer,
};
use katana_entity::{
    AnchorPoint, AnchorRef, DimensionGeometry, Entity, EntityId, Geometry, LabelGeometry,
    LabelKind, LabelStyle, Layer, LeaderGeometry, Model, PointGeometry,
};
use katana_geometry::{Circle2, Point2, Segment2, Vec2};
use katana_math::numerics::PI;

use crate::{double_eq, near};

#[track_caller]
fn add(document: &mut Document, geometry: Geometry) -> EntityId {
    let entity = Entity {
        geometry,
        ..Entity::default()
    };
    assert!(document.execute(create_entities(vec![entity])).is_ok());
    document.last_created_entities()[0]
}

#[track_caller]
fn geometry_of(document: &Document, id: EntityId) -> &Geometry {
    &document
        .model()
        .entities
        .find(id)
        .expect("the entity")
        .geometry
}

#[track_caller]
fn dimension_of(document: &Document, id: EntityId) -> &DimensionGeometry {
    match geometry_of(document, id) {
        Geometry::Dimension(dimension) => dimension,
        other => panic!("not a dimension: {other:?}"),
    }
}

#[track_caller]
fn line_of(document: &Document, id: EntityId) -> Segment2 {
    match geometry_of(document, id) {
        Geometry::Line(line) => *line,
        other => panic!("not a line: {other:?}"),
    }
}

fn line(start: Point2, end: Point2) -> Geometry {
    Geometry::Line(Segment2::new(start, end))
}

fn point(position: Point2) -> Geometry {
    Geometry::Point(PointGeometry { position })
}

fn anchor(entity: EntityId, point: AnchorPoint) -> AnchorRef {
    AnchorRef {
        entity,
        point,
        ..AnchorRef::default()
    }
}

fn anchored(point: Point2, reference: AnchorRef) -> AnchoredPoint {
    AnchoredPoint {
        point,
        r#ref: reference,
    }
}

#[track_caller]
fn must(status: katana_core::Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

mod associative {
    use super::*;

    #[test]
    fn a_dimension_follows_the_line_it_measures_in_one_undo_step() {
        let mut document = Document::default();
        let measured = add(
            &mut document,
            line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
        );
        let dimension = aligned_dimension(
            &anchored(Point2::new(0.0, 0.0), anchor(measured, AnchorPoint::Start)),
            &anchored(Point2::new(10.0, 0.0), anchor(measured, AnchorPoint::End)),
            Point2::new(5.0, 3.0),
        )
        .expect("a dimension");
        let dim = add(&mut document, Geometry::Dimension(dimension));
        let steps = document.history().undo_count();

        // Stretch the line by replacing its geometry, as a grip edit does.
        must(document.execute(set_entity_geometry(
            measured,
            line(Point2::new(0.0, 0.0), Point2::new(25.0, 0.0)),
        )));
        assert_eq!(
            document.history().undo_count(),
            steps + 1,
            "the follow-up is not a step of its own"
        );
        let followed = dimension_of(&document, dim);
        assert_eq!(followed.end, Point2::new(25.0, 0.0));
        double_eq(followed.measurement(), 25.0);

        // One undo: the line AND the dimension.
        must(document.undo());
        assert_eq!(line_of(&document, measured).end, Point2::new(10.0, 0.0));
        assert_eq!(dimension_of(&document, dim).end, Point2::new(10.0, 0.0));
        must(document.redo());
        assert_eq!(dimension_of(&document, dim).end, Point2::new(25.0, 0.0));
    }

    #[test]
    fn moving_the_measured_entity_moves_the_dimension() {
        let mut document = Document::default();
        let a = add(&mut document, point(Point2::new(0.0, 0.0)));
        let b = add(&mut document, point(Point2::new(0.0, 10.0)));
        let dimension = linear_dimension(
            &anchored(Point2::new(0.0, 0.0), anchor(a, AnchorPoint::Position)),
            &anchored(Point2::new(0.0, 10.0), anchor(b, AnchorPoint::Position)),
            Point2::new(-3.0, 5.0),
            None,
        )
        .expect("a dimension");
        near(dimension.angle, 0.5 * PI, 1e-15);
        let dim = add(&mut document, Geometry::Dimension(dimension));
        must(document.execute(move_entities(vec![b], Vec2::new(0.0, 5.0))));
        double_eq(dimension_of(&document, dim).measurement(), 15.0);
    }

    #[test]
    fn a_radius_follows_its_circle_and_keeps_its_direction() {
        let mut document = Document::default();
        let circle = add(
            &mut document,
            Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), 5.0)),
        );
        let dimension = radial_dimension(document.model(), circle, Point2::new(10.0, 10.0), false)
            .expect("a dimension");
        let dim = add(&mut document, Geometry::Dimension(dimension));
        must(document.execute(set_entity_geometry(
            circle,
            Geometry::Circle(Circle2::new(Point2::new(100.0, 0.0), 8.0)),
        )));
        let followed = dimension_of(&document, dim);
        assert_eq!(followed.vertex, Point2::new(100.0, 0.0));
        near(followed.measurement(), 8.0, 1e-12);
        // Still towards the upper right, at 45 degrees.
        near(followed.start.x - 100.0, followed.start.y, 1e-12);
    }

    #[test]
    fn an_angle_between_lines_follows_their_intersection() {
        let mut document = Document::default();
        let first = add(
            &mut document,
            line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
        );
        let second = add(
            &mut document,
            line(Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)),
        );
        let dimension =
            angular_between_lines(document.model(), first, second, Point2::new(3.0, 3.0))
                .expect("a dimension");
        near(dimension.measurement(), 0.5 * PI, 1e-12);
        let dim = add(&mut document, Geometry::Dimension(dimension));
        must(document.execute(move_entities(vec![first, second], Vec2::new(7.0, -2.0))));
        assert_eq!(dimension_of(&document, dim).vertex, Point2::new(7.0, -2.0));
    }

    #[test]
    fn erasing_the_measured_line_leaves_the_dimension_unassociated() {
        let mut document = Document::default();
        let measured = add(
            &mut document,
            line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
        );
        let dimension = aligned_dimension(
            &anchored(Point2::new(0.0, 0.0), anchor(measured, AnchorPoint::Start)),
            &anchored(Point2::new(10.0, 0.0), anchor(measured, AnchorPoint::End)),
            Point2::new(5.0, 3.0),
        )
        .expect("a dimension");
        let dim = add(&mut document, Geometry::Dimension(dimension));
        must(document.execute(delete_entities(vec![measured])));
        let left = dimension_of(&document, dim);
        assert!(!left.start_ref.associated());
        assert!(!left.end_ref.associated());
        assert_eq!(left.end, Point2::new(10.0, 0.0), "where it last was");
        must(document.undo());
        assert!(
            dimension_of(&document, dim).end_ref.associated(),
            "the undo gives the reference back with the line"
        );
    }

    #[test]
    fn a_leader_tip_follows_its_point() {
        let mut document = Document::default();
        let pit = add(&mut document, point(Point2::new(5.0, 5.0)));
        let leader = add(
            &mut document,
            Geometry::Leader(LeaderGeometry {
                vertices: vec![Point2::new(5.0, 5.0), Point2::new(10.0, 10.0)],
                text: String::from("PIT"),
                tip_ref: anchor(pit, AnchorPoint::Position),
                ..LeaderGeometry::default()
            }),
        );
        must(document.execute(move_entities(vec![pit], Vec2::new(1.0, 0.0))));
        let Geometry::Leader(followed) = geometry_of(&document, leader) else {
            panic!("not a leader");
        };
        assert_eq!(followed.vertices[0], Point2::new(6.0, 5.0));
        assert_eq!(
            followed.vertices[followed.vertices.len() - 1],
            Point2::new(10.0, 10.0),
            "only the tip follows"
        );
    }

    #[test]
    fn a_label_follows_its_target_and_goes_with_it() {
        let mut document = Document::default();
        let style = LabelStyle {
            name: String::from("pt"),
            kind: LabelKind::Point,
            text: String::from("{x:.1f}"),
            ..LabelStyle::default()
        };
        must(document.execute(create_label_style(style)));
        let target = add(&mut document, point(Point2::new(1.0, 1.0)));
        let label = add(
            &mut document,
            Geometry::Label(LabelGeometry {
                target,
                style: String::from("pt"),
                anchor: Point2::new(1.0, 1.0),
                position: Some(Point2::new(3.0, 3.0)),
                ..LabelGeometry::default()
            }),
        );
        must(document.execute(move_entities(vec![target], Vec2::new(10.0, 0.0))));
        let Geometry::Label(moved) = geometry_of(&document, label) else {
            panic!("not a label");
        };
        assert_eq!(moved.anchor, Point2::new(11.0, 1.0));
        assert_eq!(
            moved.position,
            Some(Point2::new(13.0, 3.0)),
            "a dragged label keeps its place beside it"
        );

        let steps = document.history().undo_count();
        must(document.execute(delete_entities(vec![target])));
        assert!(
            !document.model().entities.contains(label),
            "a label goes with its target"
        );
        assert_eq!(document.history().undo_count(), steps + 1);
        must(document.undo());
        assert!(
            document.model().entities.contains(label),
            "and comes back with it"
        );
        assert!(document.model().entities.contains(target));
    }

    #[test]
    fn an_annotation_on_a_locked_layer_is_left_alone() {
        let mut document = Document::default();
        let mut locked = Layer {
            name: String::from("dims"),
            ..Layer::default()
        };
        must(document.execute(create_layer(locked.clone())));
        let measured = add(
            &mut document,
            line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
        );
        let dimension = aligned_dimension(
            &anchored(Point2::new(0.0, 0.0), anchor(measured, AnchorPoint::Start)),
            &anchored(Point2::new(10.0, 0.0), anchor(measured, AnchorPoint::End)),
            Point2::new(5.0, 3.0),
        )
        .expect("a dimension");
        let entity = Entity {
            geometry: Geometry::Dimension(dimension),
            layer: String::from("dims"),
            ..Entity::default()
        };
        must(document.execute(create_entities(vec![entity])));
        let dim = document.last_created_entities()[0];
        locked.locked = true;
        must(document.execute(update_layer(locked)));
        assert!(
            document
                .execute(set_entity_geometry(
                    measured,
                    line(Point2::new(0.0, 0.0), Point2::new(20.0, 0.0)),
                ))
                .is_ok(),
            "the edit is not refused because a dimension that follows it is locked"
        );
        assert_eq!(dimension_of(&document, dim).end, Point2::new(10.0, 0.0));
    }

    #[test]
    fn nothing_to_follow_is_no_change() {
        let mut model = Model::default();
        assert!(associative_changes(&model).empty());
        let plain = Entity {
            id: 1,
            geometry: Geometry::Dimension(DimensionGeometry::new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                1.0,
                "",
            )),
            ..Entity::default()
        };
        assert!(model.entities.insert(plain).is_ok());
        assert!(
            associative_changes(&model).empty(),
            "an unassociated dimension stays"
        );
    }
}
