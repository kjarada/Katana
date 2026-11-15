// Annotation drawn out as plain shapes for a file (annotation/export_annotation.rs,
// docs/annotation.md "Exchange"; tests/cad/annotation/test_export_annotation.cpp):
// what the DXF writer is handed for the labels, leaders and dimension kinds it
// cannot draw itself.

use katana_cad::annotation::drawing::{Drawing, estimated_measure};
use katana_cad::annotation::export_annotation::{draw_annotation_for_export, plain_shapes};
use katana_entity::{
    CalloutShape, DimensionGeometry, DimensionKind, Entity, EntityId, Geometry, LabelGeometry,
    LabelKind, LabelMarker, LabelStyle, LeaderContext, LeaderGeometry, Model, PointGeometry,
    PropertyValue,
};
use katana_geometry::{Circle2, Point2};

use crate::{double_eq, ok};

fn count(shapes: &[Geometry], kind: fn(&Geometry) -> bool) -> usize {
    shapes.iter().filter(|shape| kind(shape)).count()
}

fn is_text(geometry: &Geometry) -> bool {
    matches!(geometry, Geometry::Text(_))
}

fn is_polyline(geometry: &Geometry) -> bool {
    matches!(geometry, Geometry::Polyline(_))
}

fn is_segment(geometry: &Geometry) -> bool {
    matches!(geometry, Geometry::Line(_))
}

fn text_of(shapes: &[Geometry]) -> String {
    shapes
        .iter()
        .find_map(|shape| match shape {
            Geometry::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .expect("a text")
}

#[track_caller]
fn add(model: &mut Model, geometry: Geometry) -> EntityId {
    model
        .entities
        .add(Entity {
            geometry,
            ..Entity::default()
        })
        .expect("the entity is added")
}

fn point_style(displace: bool, marker: LabelMarker) -> LabelStyle {
    LabelStyle {
        name: String::from("pt"),
        kind: LabelKind::Point,
        text: String::from("{point}"),
        displace,
        marker,
        ..LabelStyle::default()
    }
}

fn numbered_point(model: &mut Model, at: Point2, number: &str) -> EntityId {
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    point.properties.insert(
        String::from("point"),
        PropertyValue::Text(String::from(number)),
    );
    model.entities.add(point).expect("the point is added")
}

fn label_on(model: &mut Model, target: EntityId, at: Point2) -> EntityId {
    add(
        model,
        Geometry::Label(LabelGeometry {
            target,
            style: String::from("pt"),
            anchor: at,
            ..LabelGeometry::default()
        }),
    )
}

mod export_annotation {
    use super::*;

    #[test]
    fn a_label_is_its_text_where_the_plan_view_puts_it_at_the_scale() {
        let mut model = Model::default();
        ok(model
            .label_styles
            .add(point_style(true, LabelMarker::Circle)));
        let target = numbered_point(&mut model, Point2::new(10.0, 10.0), "P7");
        let label = label_on(&mut model, target, Point2::new(10.0, 10.0));

        for scale in [200.0, 1000.0] {
            let drawn = draw_annotation_for_export(
                &model,
                scale,
                &estimated_measure(),
                &LeaderContext::default(),
            );
            let shapes = drawn.get(&label).expect("the label is drawn");
            assert_eq!(count(shapes, is_text), 1, "{scale}");
            let text = shapes
                .iter()
                .find_map(|shape| match shape {
                    Geometry::Text(text) => Some(text),
                    _ => None,
                })
                .expect("a text");
            assert_eq!(text.text, "P7");
            // 2.5 mm on paper at the scale.
            double_eq(text.height, 2.5 * scale / 1000.0);
            assert_eq!(
                count(shapes, is_polyline),
                1,
                "the marker, a closed outline"
            );
            assert!(
                !drawn.contains_key(&target),
                "the point is the writer's own"
            );
        }
    }

    #[test]
    fn a_label_with_no_room_is_an_entry_with_no_shapes() {
        let mut model = Model::default();
        ok(model
            .label_styles
            .add(point_style(false, LabelMarker::None)));
        let mut labels = Vec::new();
        for _ in 0..2 {
            let target = numbered_point(&mut model, Point2::new(0.0, 0.0), "P");
            labels.push(label_on(&mut model, target, Point2::new(0.0, 0.0)));
        }
        let drawn = draw_annotation_for_export(
            &model,
            1000.0,
            &estimated_measure(),
            &LeaderContext::default(),
        );
        assert!(!drawn[&labels[0]].is_empty());
        assert!(
            drawn[&labels[1]].is_empty(),
            "on top of the first, with nowhere else to go"
        );
    }

    #[test]
    fn dimension_kinds_and_leaders_are_drawn_out_and_aligned_ones_are_left_to_the_writer() {
        let mut model = Model::default();
        let round = add(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), 5.0)),
        );
        let radius = DimensionGeometry {
            kind: DimensionKind::Radius,
            vertex: Point2::new(0.0, 0.0),
            start: Point2::new(5.0, 0.0),
            offset: 3.0,
            ..DimensionGeometry::default()
        };
        let radial = add(&mut model, Geometry::Dimension(radius));
        let aligned = add(
            &mut model,
            Geometry::Dimension(DimensionGeometry::new(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                2.0,
                "",
            )),
        );
        let leader = LeaderGeometry {
            vertices: vec![Point2::new(0.0, 0.0), Point2::new(8.0, 8.0)],
            text: String::from("PIT 12\nIL 10.50"),
            callout: CalloutShape::Box,
            ..LeaderGeometry::default()
        };
        let note = add(&mut model, Geometry::Leader(leader));

        let drawn = draw_annotation_for_export(
            &model,
            500.0,
            &estimated_measure(),
            &LeaderContext::default(),
        );
        assert!(!drawn.contains_key(&round));
        assert!(!drawn.contains_key(&aligned));
        let dimension = drawn.get(&radial).expect("the radial dimension is drawn");
        assert_eq!(count(dimension, is_text), 1);
        assert!(text_of(dimension).starts_with('R'));
        assert!(
            count(dimension, is_segment) + count(dimension, is_polyline) >= 2,
            "line and arrow"
        );

        let callout = drawn.get(&note).expect("the leader is drawn");
        assert_eq!(count(callout, is_text), 2, "a text a line");
        // The line, the arrowhead's outline and the box.
        let closed = callout
            .iter()
            .filter(|shape| matches!(shape, Geometry::Polyline(polyline) if polyline.closed))
            .count();
        assert_eq!(closed, 2, "the arrowhead and the callout's box");
    }

    #[test]
    fn a_stroke_of_two_points_is_a_segment_and_one_with_no_length_is_nothing() {
        let drawing = Drawing {
            strokes: vec![
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                vec![Point2::new(2.0, 2.0), Point2::new(2.0, 2.0)],
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(1.0, 1.0),
                ],
            ],
            fills: vec![vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
            ]],
            ..Drawing::default()
        };
        let shapes = plain_shapes(&drawing);
        assert_eq!(count(&shapes, is_segment), 1);
        assert_eq!(count(&shapes, is_polyline), 2);
        assert!(matches!(&shapes[1], Geometry::Polyline(polyline) if !polyline.closed));
        assert!(
            matches!(&shapes[2], Geometry::Polyline(polyline) if polyline.closed),
            "a fill is its outline"
        );
    }
}
