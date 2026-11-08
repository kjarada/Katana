// Grips (drawing/grips.rs; tests/cad/drawing/test_grips.cpp): what each
// geometry kind offers and what dragging its grips does, headless - the plan
// view only draws them and forwards the drag.

use katana_cad::drawing::grips::{
    DEFAULT_GRIP_LIMIT, Grip, GripDrag, GripKind, apply_grip_drag, delete_hot_vertices, grip_at,
    grip_drag_command, grip_feedback, grip_preview, grips_of, grips_of_selection,
};
use katana_cad::layer_overrides::NO_LAYER_OVERRIDES;
use katana_cad::{Document, FeedbackRole, LayerOverrides};
use katana_commands::{
    EntityAttributes, create_entities, create_layer, create_line, create_polyline,
};
use katana_entity::entity::{heights_of, set_heights, to_string};
use katana_entity::{
    DimensionGeometry, Entity, EntityId, Geometry, Layer, PointGeometry, TextGeometry,
};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveSegment, Ellipse2, Point2, Polyline2, Segment2, Spline2,
    Vec2,
};
use katana_math::numerics::{HALF_PI, PI};

use crate::{near, ok};

fn entity_of(geometry: Geometry) -> Entity {
    Entity {
        id: 7,
        geometry,
        ..Entity::default()
    }
}

#[track_caller]
fn grip_of(entity: &Entity, kind: GripKind, index: usize) -> Grip {
    grips_of(entity)
        .into_iter()
        .find(|grip| grip.kind == kind && grip.index == index)
        .unwrap_or_else(|| panic!("no {} grip {index}", to_string(kind)))
}

#[track_caller]
fn dragged(entity: &Entity, kind: GripKind, index: usize, to: Point2, insert: bool) -> Entity {
    let drag = GripDrag {
        grabbed: grip_of(entity, kind, index),
        target: to,
        insert_vertex: insert,
        ..GripDrag::default()
    };
    apply_grip_drag(entity, &drag).unwrap_or_else(|error| panic!("{}", error.message))
}

#[track_caller]
fn expect_near(a: Point2, b: Point2) {
    near(a.x, b.x, 1e-9);
    near(a.y, b.y, 1e-9);
}

#[track_caller]
fn add(document: &mut Document, geometry: Geometry) -> EntityId {
    ok(document.execute(create_entities(vec![Entity {
        geometry,
        ..Entity::default()
    }])));
    document.last_created_entities()[0]
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn polyline(points: &[Point2], closed: bool) -> Geometry {
    Geometry::Polyline(Polyline2::new(points.to_vec(), closed))
}

#[track_caller]
fn polyline_of(entity: &Entity) -> &Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline,
        other => panic!("not a polyline: {other:?}"),
    }
}

#[track_caller]
fn curve_of(entity: &Entity) -> &CurvePolyline2 {
    match &entity.geometry {
        Geometry::CurvePolyline(polyline) => polyline,
        other => panic!("not a curve polyline: {other:?}"),
    }
}

#[track_caller]
fn position_of(geometry: &Geometry) -> Point2 {
    match geometry {
        Geometry::Point(PointGeometry { position }) => *position,
        other => panic!("not a point: {other:?}"),
    }
}

// ---- enumeration per kind --------------------------------------------------------------

mod grips {
    use super::*;

    #[test]
    fn each_kind_offers_its_handles() {
        assert_eq!(
            grips_of(&entity_of(Geometry::Point(PointGeometry {
                position: p(1.0, 2.0)
            })))
            .len(),
            1
        );
        assert_eq!(
            grips_of(&entity_of(Geometry::Line(Segment2::new(
                p(0.0, 0.0),
                p(4.0, 0.0)
            ))))
            .len(),
            3
        );
        assert_eq!(
            grips_of(&entity_of(Geometry::Arc(Arc2::new(
                p(0.0, 0.0),
                2.0,
                0.0,
                HALF_PI
            ))))
            .len(),
            4
        );
        assert_eq!(
            grips_of(&entity_of(Geometry::Circle(Circle2::new(p(0.0, 0.0), 2.0)))).len(),
            5
        );
        // A closed square: four vertices and four segment middles.
        let square = grips_of(&entity_of(polyline(
            &[p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0), p(0.0, 4.0)],
            true,
        )));
        assert_eq!(square.len(), 8);
        assert_eq!(square[0].kind, GripKind::Vertex);
        assert_eq!(square[4].kind, GripKind::SegmentMid);
        expect_near(square[7].position, p(0.0, 2.0)); // the closing segment's middle
        // A curve polyline's arc segment has its middle ON the arc.
        let mut bent = CurvePolyline2::from_points(&[p(0.0, 0.0), p(2.0, 0.0)], false);
        bent.vertices[0].bulge = 1.0;
        let arc_grips = grips_of(&entity_of(Geometry::CurvePolyline(bent)));
        assert_eq!(arc_grips.len(), 3);
        expect_near(arc_grips[2].position, p(1.0, -1.0));
        // A full ellipse: centre and four axis ends; an arc of it adds its
        // ends.
        let mut ellipse = Ellipse2 {
            major_axis: Vec2::new(4.0, 0.0),
            ratio: 0.5,
            ..Ellipse2::default()
        };
        assert_eq!(grips_of(&entity_of(Geometry::Ellipse(ellipse))).len(), 5);
        ellipse.sweep = HALF_PI;
        assert_eq!(
            grips_of(&entity_of(Geometry::Ellipse(ellipse))).len(),
            1 + 2 + 2
        );
        // A fit spline by its fit points.
        let spline = Spline2::through_points(vec![p(0.0, 0.0), p(2.0, 3.0), p(5.0, 0.0)], 3)
            .expect("a spline");
        let spline_grips = grips_of(&entity_of(Geometry::Spline(spline)));
        assert_eq!(spline_grips.len(), 3);
        assert_eq!(spline_grips[1].kind, GripKind::FitPoint);
        assert!(
            grips_of(&entity_of(Geometry::Dimension(DimensionGeometry::new(
                p(0.0, 0.0),
                p(1.0, 0.0),
                1.0,
                ""
            ))))
            .is_empty(),
            "annotation is the annotation system's to give handles to"
        );
    }

    #[test]
    fn grip_at_finds_the_nearest_within_the_aperture() {
        let grips = grips_of(&entity_of(Geometry::Line(Segment2::new(
            p(0.0, 0.0),
            p(4.0, 0.0),
        ))));
        let found = grip_at(&grips, p(3.9, 0.05), 0.2).expect("a grip");
        assert_eq!(found.kind, GripKind::End);
        assert_eq!(found.index, 1);
        assert!(grip_at(&grips, p(3.0, 1.0), 0.2).is_none());
    }
}

// ---- dragging per kind -------------------------------------------------------------------

mod grip_drags {
    use super::*;

    #[test]
    fn a_vertex_grip_moves_the_vertex_and_keeps_the_arcs() {
        let mut shape =
            CurvePolyline2::from_points(&[p(0.0, 0.0), p(4.0, 0.0), p(8.0, 0.0)], false);
        shape.vertices[0].bulge = 0.5;
        shape.vertices[1].height = Some(12.0);
        let out = dragged(
            &entity_of(Geometry::CurvePolyline(shape)),
            GripKind::Vertex,
            1,
            p(4.0, 3.0),
            false,
        );
        let moved = curve_of(&out);
        assert_eq!(moved.vertices[1].position, p(4.0, 3.0));
        assert_eq!(moved.vertices[0].bulge, 0.5);
        assert_eq!(moved.vertices[1].height, Some(12.0));
    }

    #[test]
    fn a_straight_polyline_stays_a_polyline_with_its_heights() {
        let mut e = entity_of(polyline(&[p(0.0, 0.0), p(4.0, 0.0), p(8.0, 0.0)], false));
        set_heights(&mut e.properties, &[Some(1.0), Some(2.0), Some(3.0)]);
        let out = dragged(&e, GripKind::Vertex, 2, p(8.0, 5.0), false);
        assert_eq!(polyline_of(&out).vertices[2], p(8.0, 5.0));
        assert_eq!(heights_of(&out.properties, 3)[2], Some(3.0));
    }

    #[test]
    fn a_segment_middle_stretches_or_inserts_a_vertex() {
        let e = entity_of(polyline(&[p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0)], false));
        // Drag: the whole first segment moves up by 1 (both its vertices).
        let stretched = dragged(&e, GripKind::SegmentMid, 0, p(2.0, 1.0), false);
        let s = polyline_of(&stretched);
        assert_eq!(s.vertices[0], p(0.0, 1.0));
        assert_eq!(s.vertices[1], p(4.0, 1.0));
        assert_eq!(s.vertices[2], p(4.0, 4.0));
        // Ctrl-drag: a new vertex where the middle was dragged to.
        let inserted = dragged(&e, GripKind::SegmentMid, 0, p(2.0, -1.0), true);
        let i = polyline_of(&inserted);
        assert_eq!(i.vertices.len(), 4);
        assert_eq!(i.vertices[1], p(2.0, -1.0));
    }

    #[test]
    fn grip_feedback_of_a_ctrl_insert_shows_the_new_vertex() {
        // Segment 0's middle of (0,0) (10,0) (20,0), Ctrl-dragged to (5,-3):
        // the grip was at (5,0), and the vertex it makes is (5,-3), the new
        // vertex 1.
        let mut document = Document::default();
        ok(document.execute(create_polyline(
            Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)], false),
            EntityAttributes::default(),
        )));
        let id = document.last_created_entities()[0];
        let grabbed = grip_of(
            document.model().entities.find(id).expect("the polyline"),
            GripKind::SegmentMid,
            0,
        );
        let mut drag = GripDrag {
            grabbed,
            target: p(5.0, -3.0),
            insert_vertex: true,
            ..GripDrag::default()
        };
        let feedback = grip_feedback(&document, &drag);
        assert_eq!(feedback.shapes.len(), 1);
        let Geometry::Polyline(ghost) = &feedback.shapes[0] else {
            panic!("a polyline ghost");
        };
        assert_eq!(
            ghost.vertices,
            vec![p(0.0, 0.0), p(5.0, -3.0), p(10.0, 0.0), p(20.0, 0.0)]
        );
        assert_eq!(feedback.marks.len(), 2);
        assert_eq!(feedback.marks[0].role, FeedbackRole::Target);
        assert_eq!(position_of(&feedback.marks[0].geometry), p(5.0, 0.0));
        assert_eq!(feedback.marks[1].role, FeedbackRole::Added);
        assert_eq!(position_of(&feedback.marks[1].geometry), p(5.0, -3.0));

        // A plain drag of the same grip stretches the segment: no vertex
        // added.
        drag.insert_vertex = false;
        let stretch = grip_feedback(&document, &drag);
        assert_eq!(stretch.marks.len(), 1);
        assert_eq!(stretch.marks[0].role, FeedbackRole::Target);
    }

    #[test]
    fn an_arc_segments_middle_reshapes_the_arc_through_the_target() {
        let mut shape = CurvePolyline2::from_points(&[p(0.0, 0.0), p(2.0, 0.0)], false);
        shape.vertices[0].bulge = 1.0; // through (1,-1)
        let e = entity_of(Geometry::CurvePolyline(shape));
        let out = dragged(&e, GripKind::SegmentMid, 0, p(1.0, -0.5), false);
        let moved = curve_of(&out);
        assert_eq!(moved.vertices[0].position, p(0.0, 0.0));
        assert_eq!(moved.vertices[1].position, p(2.0, 0.0));
        let CurveSegment::Arc(arc) = moved.segment(0) else {
            panic!("an arc segment");
        };
        expect_near(arc.midpoint(), p(1.0, -0.5));
        // Dragged back onto the chord it is refused, not made into nonsense.
        let flat = GripDrag {
            grabbed: grip_of(&e, GripKind::SegmentMid, 0),
            target: p(1.0, 0.0),
            ..GripDrag::default()
        };
        assert!(apply_grip_drag(&e, &flat).is_err());
    }

    #[test]
    fn several_hot_grips_move_together() {
        let e = entity_of(polyline(
            &[p(0.0, 0.0), p(4.0, 0.0), p(8.0, 0.0), p(12.0, 0.0)],
            false,
        ));
        let drag = GripDrag {
            grabbed: grip_of(&e, GripKind::Vertex, 1),
            also_hot: vec![
                grip_of(&e, GripKind::Vertex, 2),
                grip_of(&e, GripKind::SegmentMid, 1),
            ],
            target: p(4.0, 2.0),
            ..GripDrag::default()
        };
        let out = apply_grip_drag(&e, &drag).expect("a drag");
        let moved = polyline_of(&out);
        // Each vertex moves once, however many hot grips name it.
        assert_eq!(moved.vertices[1], p(4.0, 2.0));
        assert_eq!(moved.vertices[2], p(8.0, 2.0));
        assert_eq!(moved.vertices[3], p(12.0, 0.0));
    }

    #[test]
    fn lines_arcs_and_circles() {
        let line = entity_of(Geometry::Line(Segment2::new(p(0.0, 0.0), p(4.0, 0.0))));
        let Geometry::Line(end) = dragged(&line, GripKind::End, 1, p(4.0, 3.0), false).geometry
        else {
            panic!("a line");
        };
        assert_eq!(end.end, p(4.0, 3.0));
        let Geometry::Line(whole) = dragged(&line, GripKind::Mid, 0, p(2.0, 1.0), false).geometry
        else {
            panic!("a line");
        };
        assert_eq!(whole.start, p(0.0, 1.0));
        assert_eq!(whole.end, p(4.0, 1.0));

        let arc = entity_of(Geometry::Arc(Arc2::new(p(0.0, 0.0), 2.0, 0.0, PI)));
        let Geometry::Arc(moved) = dragged(&arc, GripKind::Centre, 0, p(1.0, 1.0), false).geometry
        else {
            panic!("an arc");
        };
        expect_near(moved.center, p(1.0, 1.0));
        near(moved.radius, 2.0, 1e-12);
        // Its middle dragged up to (0, 3): the arc through (2,0), (0,3),
        // (-2,0).
        let Geometry::Arc(reshaped) = dragged(&arc, GripKind::Mid, 0, p(0.0, 3.0), false).geometry
        else {
            panic!("an arc");
        };
        expect_near(reshaped.midpoint(), p(0.0, 3.0));
        expect_near(reshaped.start_point(), p(2.0, 0.0));

        let circle = entity_of(Geometry::Circle(Circle2::new(p(0.0, 0.0), 2.0)));
        let Geometry::Circle(bigger) =
            dragged(&circle, GripKind::Quadrant, 1, p(0.0, 5.0), false).geometry
        else {
            panic!("a circle");
        };
        near(bigger.radius, 5.0, 1e-12);
        let collapse = GripDrag {
            grabbed: grip_of(&circle, GripKind::Quadrant, 0),
            target: p(0.0, 0.0),
            ..GripDrag::default()
        };
        assert!(
            apply_grip_drag(&circle, &collapse).is_err(),
            "a circle of no radius"
        );
    }

    #[test]
    fn ellipses_and_splines() {
        let ellipse = Ellipse2 {
            major_axis: Vec2::new(4.0, 0.0),
            ratio: 0.5,
            ..Ellipse2::default()
        };
        let e = entity_of(Geometry::Ellipse(ellipse));
        let ellipse_of = |out: Entity| match out.geometry {
            Geometry::Ellipse(ellipse) => ellipse,
            other => panic!("not an ellipse: {other:?}"),
        };
        let longer = ellipse_of(dragged(&e, GripKind::Quadrant, 0, p(8.0, 0.0), false));
        near(longer.major_radius(), 8.0, 1e-12);
        near(longer.minor_radius(), 2.0, 1e-12);
        let fatter = ellipse_of(dragged(&e, GripKind::Quadrant, 1, p(0.0, 3.0), false));
        near(fatter.minor_radius(), 3.0, 1e-12);
        // Past the major axis on a full ellipse, the axes swap.
        let swapped = ellipse_of(dragged(&e, GripKind::Quadrant, 1, p(0.0, 6.0), false));
        near(swapped.major_radius(), 6.0, 1e-12);
        near(swapped.minor_radius(), 4.0, 1e-12);

        let spline = Spline2::through_points(vec![p(0.0, 0.0), p(2.0, 3.0), p(5.0, 0.0)], 3)
            .expect("a spline");
        let Geometry::Spline(moved) = dragged(
            &entity_of(Geometry::Spline(spline)),
            GripKind::FitPoint,
            1,
            p(2.0, 5.0),
            false,
        )
        .geometry
        else {
            panic!("a spline");
        };
        assert_eq!(moved.fit_points[1], p(2.0, 5.0));
        assert!(
            moved.distance_to(p(2.0, 5.0)) < 1e-3,
            "re-solved through the moved point"
        );
    }

    #[test]
    fn text_and_point_move_by_their_insertion_grip() {
        let text = entity_of(Geometry::Text(TextGeometry::new(
            p(1.0, 1.0),
            "A",
            2.5,
            0.0,
        )));
        let Geometry::Text(moved) =
            dragged(&text, GripKind::Insertion, 0, p(5.0, 6.0), false).geometry
        else {
            panic!("a text");
        };
        assert_eq!(moved.position, p(5.0, 6.0));
    }
}

// ---- as commands: one undo step ------------------------------------------------------------

mod grip_commands {
    use super::*;

    #[test]
    fn a_grip_drag_is_one_undo_step_across_entities() {
        let mut document = Document::default();
        let a = add(
            &mut document,
            polyline(&[p(0.0, 0.0), p(4.0, 0.0), p(8.0, 0.0)], false),
        );
        let b = add(
            &mut document,
            Geometry::Line(Segment2::new(p(4.0, 0.0), p(4.0, 5.0))),
        );
        let before = document.history().undo_count();

        let grips_a = grips_of(document.model().entities.find(a).expect("a"));
        let grips_b = grips_of(document.model().entities.find(b).expect("b"));
        let drag = GripDrag {
            grabbed: grips_a[1],        // vertex 1 at (4,0)
            also_hot: vec![grips_b[0]], // the line's start, also at (4,0)
            target: p(4.0, -2.0),
            ..GripDrag::default()
        };
        ok(document.execute(grip_drag_command(drag)));
        assert_eq!(document.history().undo_count(), before + 1);
        let entity = |document: &Document, id: EntityId| {
            document
                .model()
                .entities
                .find(id)
                .expect("the entity")
                .clone()
        };
        assert_eq!(polyline_of(&entity(&document, a)).vertices[1], p(4.0, -2.0));
        let Geometry::Line(line) = entity(&document, b).geometry else {
            panic!("a line");
        };
        assert_eq!(line.start, p(4.0, -2.0));

        ok(document.undo());
        assert_eq!(polyline_of(&entity(&document, a)).vertices[1], p(4.0, 0.0));
        let Geometry::Line(line) = entity(&document, b).geometry else {
            panic!("a line");
        };
        assert_eq!(line.start, p(4.0, 0.0));
    }

    #[test]
    fn delete_on_hot_vertex_grips_removes_them_in_one_step() {
        let mut document = Document::default();
        let id = add(
            &mut document,
            polyline(
                &[
                    p(0.0, 0.0),
                    p(2.0, 1.0),
                    p(4.0, 0.0),
                    p(6.0, 1.0),
                    p(8.0, 0.0),
                ],
                false,
            ),
        );
        let grips = grips_of(document.model().entities.find(id).expect("the polyline"));
        let before = document.history().undo_count();
        ok(document.execute(delete_hot_vertices(vec![grips[1], grips[3]])));
        assert_eq!(document.history().undo_count(), before + 1);
        let vertices = |document: &Document| {
            polyline_of(document.model().entities.find(id).expect("the polyline"))
                .vertices
                .len()
        };
        assert_eq!(vertices(&document), 3);
        // Segment middles are not vertices: nothing to delete is refused.
        assert!(
            document
                .execute(delete_hot_vertices(vec![grips[6]]))
                .is_err()
        );
        ok(document.undo());
        assert_eq!(vertices(&document), 5);
    }

    #[test]
    fn the_selections_grips_skip_locked_layers_and_honour_the_limit() {
        let mut document = Document::default();
        let id = add(
            &mut document,
            Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 0.0))),
        );
        assert_eq!(
            grips_of_selection(&document, &[id], &NO_LAYER_OVERRIDES, DEFAULT_GRIP_LIMIT).len(),
            3
        );
        assert!(
            grips_of_selection(&document, &[id], &NO_LAYER_OVERRIDES, 2).is_empty(),
            "past the limit, none"
        );
        let drag = GripDrag {
            grabbed: grips_of(document.model().entities.find(id).expect("the line"))[1],
            target: p(1.0, 1.0),
            also_hot: Vec::new(),
            insert_vertex: false,
        };
        let preview = grip_preview(&document, &drag);
        assert_eq!(preview.len(), 1);
        let Geometry::Line(line) = &preview[0] else {
            panic!("a line");
        };
        assert_eq!(line.end, p(1.0, 1.0));
    }

    #[test]
    fn a_view_offers_no_grip_on_what_it_hides_and_the_others_keep_theirs() {
        // A design line and an as-built line, both selected; a view hiding the
        // design layer - the as-built view - offers the as-built line's three
        // grips (two ends and the middle) and none of the design line's, which
        // it shows as a ghost at most. The document's rule alone offers all
        // six.
        let mut document = Document::default();
        for name in ["design", "asbuilt"] {
            ok(document.execute(create_layer(Layer {
                name: String::from(name),
                ..Layer::default()
            })));
        }
        let on = |layer: &str| EntityAttributes {
            layer: String::from(layer),
            ..EntityAttributes::default()
        };
        ok(document.execute(create_line(p(10.0, 10.0), p(40.0, 30.0), on("design"))));
        let design = document.last_created_entities()[0];
        ok(document.execute(create_line(p(10.0, 30.0), p(40.0, 10.0), on("asbuilt"))));
        let as_built = document.last_created_entities()[0];

        let mut as_built_view = LayerOverrides::default();
        assert!(as_built_view.hide("design"));
        let offered = grips_of_selection(
            &document,
            &[design, as_built],
            &as_built_view,
            DEFAULT_GRIP_LIMIT,
        );
        assert_eq!(offered.len(), 3);
        for grip in &offered {
            assert_eq!(grip.entity, as_built);
        }
        // Where the two cross, (25, 20), both lines have their middles: in the
        // as-built view the grip there is the as-built line's.
        let at_crossing = grip_at(&offered, p(25.0, 20.0), 0.5).expect("a grip");
        assert_eq!(at_crossing.entity, as_built);
        // The design line alone selected, the as-built view offers nothing.
        assert!(
            grips_of_selection(&document, &[design], &as_built_view, DEFAULT_GRIP_LIMIT).is_empty()
        );
        assert_eq!(
            grips_of_selection(
                &document,
                &[design, as_built],
                &NO_LAYER_OVERRIDES,
                DEFAULT_GRIP_LIMIT
            )
            .len(),
            6
        );
    }
}
