// Vertex editing of polyline entities (drawing/vertex_editing.rs;
// tests/cad/drawing/test_vertex_editing.cpp): which kind an edited polyline is
// stored as, where its heights go, and that each edit is one undo step.

use katana_cad::Document;
use katana_cad::drawing::vertex_editing::{
    SURVEY_POINT_TOLERANCE, edit_polyline, edit_polylines, read_polyline,
    vertices_on_survey_points_in, write_polyline,
};
use katana_commands::create_entities;
use katana_entity::entity::{ELEVATIONS_PROPERTY, heights_of, set_heights};
use katana_entity::{Entity, EntityId, Geometry, PointGeometry, PropertyValue};
use katana_geometry::polyline_vertices::{delete_vertex, insert_vertex, weed};
use katana_geometry::{CurvePolyline2, Point2, Polyline2};

use crate::ok;

#[track_caller]
fn add(document: &mut Document, entity: Entity) -> EntityId {
    ok(document.execute(create_entities(vec![entity])));
    document.last_created_entities()[0]
}

fn polyline_entity(points: &[Point2], heights: &[Option<f64>]) -> Entity {
    let mut entity = Entity {
        geometry: Geometry::Polyline(Polyline2::new(points.to_vec(), false)),
        ..Entity::default()
    };
    if !heights.is_empty() {
        set_heights(&mut entity.properties, heights);
    }
    entity
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

#[track_caller]
fn vertex_count(document: &Document, id: EntityId) -> usize {
    match &document
        .model()
        .entities
        .find(id)
        .expect("the entity")
        .geometry
    {
        Geometry::Polyline(polyline) => polyline.vertices.len(),
        other => panic!("not a polyline: {other:?}"),
    }
}

mod vertex_editing {
    use super::*;

    #[test]
    fn a_straight_polyline_reads_with_its_property_heights() {
        let e = polyline_entity(&[p(0.0, 0.0), p(5.0, 0.0)], &[Some(10.0), None]);
        let read = read_polyline(&e).expect("a polyline");
        assert_eq!(read.vertices[0].height, Some(10.0));
        assert!(read.vertices[1].height.is_none());
        assert!(
            read_polyline(&Entity::default()).is_none(),
            "a point is no polyline"
        );
    }

    #[test]
    fn an_edit_is_stored_as_the_simplest_kind() {
        let e = polyline_entity(
            &[p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0)],
            &[Some(1.0), Some(2.0), Some(3.0)],
        );
        let mut polyline = read_polyline(&e).expect("a polyline");
        // An arc makes it a curve polyline, heights in the geometry and the
        // properties cleared.
        polyline.vertices[0].bulge = 0.4;
        let curved = write_polyline(e.clone(), &polyline).expect("a curve polyline");
        let Geometry::CurvePolyline(stored) = &curved.geometry else {
            panic!("a curve polyline");
        };
        assert_eq!(stored.vertices[2].height, Some(3.0));
        assert!(!curved.properties.contains_key(ELEVATIONS_PROPERTY));
        // Straightened again, it goes back to a Polyline2 with its heights in
        // the properties, as every other 3D string is held.
        polyline.vertices[0].bulge = 0.0;
        let straight = write_polyline(curved.clone(), &polyline).expect("a polyline");
        assert!(matches!(straight.geometry, Geometry::Polyline(_)));
        assert_eq!(heights_of(&straight.properties, 3)[1], Some(2.0));
    }

    #[test]
    fn each_edit_is_one_undo_step() {
        let mut document = Document::default();
        let id = add(
            &mut document,
            polyline_entity(
                &[p(0.0, 0.0), p(5.0, 0.0), p(10.0, 0.0)],
                &[Some(0.0), Some(5.0), Some(10.0)],
            ),
        );
        let before = document.history().undo_count();
        ok(document.execute(edit_polyline(
            id,
            "VERTEX_INSERT",
            |polyline: &CurvePolyline2| insert_vertex(polyline, 0, p(2.0, 0.0), None),
        )));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(vertex_count(&document, id), 4);
        let edited = document.model().entities.find(id).expect("the polyline");
        assert_eq!(heights_of(&edited.properties, 4)[1], Some(2.0));
        assert_eq!(document.history().undo_name(), "VERTEX_INSERT");
        ok(document.undo());
        assert_eq!(vertex_count(&document, id), 3);
    }

    #[test]
    fn a_failed_edit_leaves_the_drawing_and_history_alone() {
        let mut document = Document::default();
        let id = add(
            &mut document,
            polyline_entity(&[p(0.0, 0.0), p(5.0, 0.0)], &[]),
        );
        let before = document.history().undo_count();
        let status = document.execute(edit_polyline(
            id,
            "VERTEX_DELETE",
            |polyline: &CurvePolyline2| delete_vertex(polyline, 0),
        ));
        assert!(status.is_err());
        assert_eq!(document.history().undo_count(), before);
        let point = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(1.0, 1.0),
            }),
            ..Entity::default()
        };
        let point_id = add(&mut document, point);
        let not_polyline = document
            .execute(edit_polyline(
                point_id,
                "WEED",
                |polyline: &CurvePolyline2| weed(polyline, 0.1, &[]),
            ))
            .expect_err("a point is refused");
        assert!(
            not_polyline.message.contains("not a polyline"),
            "{}",
            not_polyline.describe()
        );
    }

    #[test]
    fn several_polylines_in_one_step() {
        let mut document = Document::default();
        let a = add(
            &mut document,
            polyline_entity(&[p(0.0, 0.0), p(1.0, 0.001), p(2.0, 0.0)], &[]),
        );
        let b = add(
            &mut document,
            polyline_entity(&[p(0.0, 5.0), p(1.0, 5.001), p(2.0, 5.0)], &[]),
        );
        let before = document.history().undo_count();
        ok(document.execute(edit_polylines(
            vec![a, b],
            "WEED",
            |polyline: &CurvePolyline2| weed(polyline, 0.01, &[]),
        )));
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(vertex_count(&document, a), 2);
        assert_eq!(vertex_count(&document, b), 2);
    }

    #[test]
    fn survey_points_on_vertices_are_found() {
        let mut document = Document::default();
        let mut mark = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(5.0, 0.0),
            }),
            ..Entity::default()
        };
        mark.properties.insert(
            String::from("point"),
            PropertyValue::Text(String::from("101")),
        );
        add(&mut document, mark);
        // No point number.
        let cad = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(10.0, 0.0),
            }),
            ..Entity::default()
        };
        add(&mut document, cad);
        let polyline =
            CurvePolyline2::from_points(&[p(0.0, 0.0), p(5.0, 0.0), p(10.0, 0.0)], false);
        assert_eq!(
            vertices_on_survey_points_in(&document, &polyline, SURVEY_POINT_TOLERANCE),
            vec![false, true, false]
        );
    }
}
