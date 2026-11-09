// The Draw > Vertices tools (src/tools/modify_vertex.rs), each driven as a
// user would - pick the polyline, click or type - against results worked out
// by hand. The arithmetic itself is tested in katana_geometry's
// polyline_vertices tests; these test that each tool collects the right
// arguments and ends in one undoable command
// (tests/cad/tools/test_modify_vertex.cpp).

use katana_cad::drawing::vertex_editing::read_polyline;
use katana_commands::create_entities;
use katana_entity::entity::set_heights;
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::{CurvePolyline2, Point2, Polyline2};

use crate::tool_driver::ToolDriver;

#[track_caller]
fn add_polyline_of(
    driver: &mut ToolDriver,
    points: &[Point2],
    closed: bool,
    heights: &[Option<f64>],
) -> EntityId {
    let mut entity = Entity {
        geometry: Geometry::Polyline(Polyline2::new(points.to_vec(), closed)),
        ..Entity::default()
    };
    if !heights.is_empty() {
        set_heights(&mut entity.properties, heights);
    }
    driver.add(create_entities(vec![entity]))
}

// An open polyline with no heights (the C++ defaults).
#[track_caller]
fn add_polyline(driver: &mut ToolDriver, points: &[Point2]) -> EntityId {
    add_polyline_of(driver, points, false, &[])
}

// A curve polyline as an entity of its own.
#[track_caller]
fn add_curve(driver: &mut ToolDriver, shape: CurvePolyline2) -> EntityId {
    let entity = Entity {
        geometry: Geometry::CurvePolyline(shape),
        ..Entity::default()
    };
    driver.add(create_entities(vec![entity]))
}

#[track_caller]
fn shape_of(driver: &ToolDriver, id: EntityId) -> CurvePolyline2 {
    let Some(entity) = driver.document().model().entities.find(id) else {
        panic!("entity {id} is not in the drawing");
    };
    match read_polyline(entity) {
        Some(shape) => shape,
        None => panic!("entity {id} is not a polyline"),
    }
}

fn select_only(driver: &mut ToolDriver, ids: Vec<EntityId>) {
    driver.document_mut().selection_mut().set(ids);
}

// The geometry of `id` as the drawing has it.
#[track_caller]
fn geometry_of(driver: &ToolDriver, id: EntityId) -> Geometry {
    match driver.document().model().entities.find(id) {
        Some(entity) => entity.geometry.clone(),
        None => panic!("entity {id} is not in the drawing"),
    }
}

// The curve polyline `id` is, failing the test when it is another kind.
#[track_caller]
fn curve_of(driver: &ToolDriver, id: EntityId) -> CurvePolyline2 {
    match geometry_of(driver, id) {
        Geometry::CurvePolyline(shape) => shape,
        other => panic!("not a curve polyline: {other:?}"),
    }
}

mod vertex_tools {
    use katana_cad::drawing::vertex_editing::read_polyline;
    use katana_cad::interactive_tool::{ToolInput, tool_catalog, tool_catalog_problems};
    use katana_commands::{create_entities, create_line};
    use katana_entity::entity::heights_of;
    use katana_entity::{Entity, EntityType, Geometry, PointGeometry, PropertyValue};
    use katana_geometry::{CurvePolyline2, CurveSegment, Vec2};
    use katana_math::numerics::PI;

    use super::{
        add_curve, add_polyline, add_polyline_of, curve_of, geometry_of, select_only, shape_of,
    };
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, double_eq, near, ok, pt};

    #[test]
    fn the_family_is_in_the_catalogue_under_draw_vertices() {
        let catalog = tool_catalog();
        assert!(tool_catalog_problems().is_empty());
        let mut count = 0;
        for info in catalog.all() {
            if info.group == "Vertices" {
                assert_eq!(info.category, "Draw");
                assert!(info.name.starts_with("Vertices, "), "{}", info.name);
                count += 1;
            }
        }
        assert_eq!(count, 18);
        assert!(catalog.find_by_alias("WEED").is_some());
        assert!(catalog.find_by_alias("vertexz").is_some());
    }

    #[test]
    fn insert_vertex_adds_a_vertex_on_the_nearest_segment() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.insert");
        // One click near the segment: the vertex goes ON it, where the click
        // projects (a pick of the polyline came first before 2026-09-30).
        driver.click(4.0, 0.1);
        assert_eq!(driver.executed(), 1);
        assert_eq!(shape_of(&driver, id).vertices.len(), 4);
        assert_eq!(shape_of(&driver, id).vertices[1].position, pt(4.0, 0.0));
        assert!(!driver.finished(), "it restarts for the next vertex");
        assert_eq!(driver.document().history().undo_name(), "VERTEX_INSERT");
    }

    #[test]
    fn delete_vertex_removes_the_one_nearest_the_pick() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 3.0), pt(10.0, 0.0)]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.delete");
        driver.pick(id, 5.2, 2.9);
        assert_eq!(shape_of(&driver, id).vertices.len(), 2);
        // A second delete would leave one vertex: refused, and said.
        let refused = driver.pick(id, 0.0, 0.0);
        assert_eq!(refused.outcome, REJECTED);
        assert_eq!(shape_of(&driver, id).vertices.len(), 2);
    }

    #[test]
    fn move_vertex_takes_a_relative_point_from_the_vertex() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.move");
        driver.pick(id, 5.0, 0.2);
        driver.r#type("@0,3");
        assert_eq!(shape_of(&driver, id).vertices[1].position, pt(5.0, 3.0));
    }

    #[test]
    fn edit_vertices_selects_the_polyline_for_the_panel() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.0)]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.edit");
        driver.pick(id, 2.0, 0.0);
        assert_eq!(driver.executed(), 0, "no edit, only a selection");
        assert_eq!(driver.document().selection().ids(), vec![id]);
        assert!(driver.finished());
    }

    #[test]
    fn straighten_removes_the_vertices_between_two_picks() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(
            &mut driver,
            &[
                pt(0.0, 0.0),
                pt(1.0, 1.0),
                pt(2.0, -1.0),
                pt(3.0, 1.0),
                pt(4.0, 0.0),
            ],
        );
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.straighten");
        driver.pick(id, 0.0, 0.0);
        driver.click(4.0, 0.0);
        assert_eq!(shape_of(&driver, id).vertices.len(), 2);
        assert_eq!(
            driver.messages().last().map(String::as_str),
            Some("3 vertices removed")
        );
    }

    #[test]
    fn weed_takes_a_tolerance_and_can_keep_survey_points() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.01), pt(10.0, 0.0)]);
        let mut mark = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: pt(5.0, 0.01),
            }),
            ..Entity::default()
        };
        mark.properties
            .insert(String::from("point"), PropertyValue::from("7"));
        driver.add(create_entities(vec![mark]));

        select_only(&mut driver, vec![id]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.weed");
        driver.r#type("0.1");
        driver.enter(); // <Yes>: keep the survey point's vertex
        assert_eq!(shape_of(&driver, id).vertices.len(), 3);

        select_only(&mut driver, vec![id]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.weed");
        driver.enter(); // the remembered 0.1
        driver.r#type("No");
        assert_eq!(shape_of(&driver, id).vertices.len(), 2);
        assert_eq!(driver.document().history().undo_name(), "WEED");
    }

    #[test]
    fn densify_selects_then_takes_the_interval_and_chord_tolerance() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0)]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.densify");
        assert_eq!(driver.expects(), ToolInput::Selection);
        select_only(&mut driver, vec![id]);
        driver.enter();
        driver.r#type("2.5");
        driver.r#type("0");
        assert_eq!(shape_of(&driver, id).vertices.len(), 5);
    }

    #[test]
    fn close_or_open_toggles_each_polyline() {
        let mut driver = ToolDriver::new();
        let open = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(5.0, 5.0)]);
        let shut = add_polyline_of(
            &mut driver,
            &[pt(10.0, 0.0), pt(15.0, 0.0), pt(15.0, 5.0)],
            true,
            &[],
        );
        select_only(&mut driver, vec![open, shut]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.close");
        // The selection answered the only question; Enter applies it.
        assert_eq!(driver.executed(), 0);
        driver.enter();
        assert_eq!(driver.executed(), 1);
        assert!(shape_of(&driver, open).closed);
        assert!(!shape_of(&driver, shut).closed);
    }

    #[test]
    fn change_start_vertex_renumbers_a_closed_polyline() {
        let mut driver = ToolDriver::new();
        let id = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)],
            true,
            &[],
        );
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.start");
        driver.pick(id, 10.0, 10.0);
        assert_eq!(shape_of(&driver, id).vertices[0].position, pt(10.0, 10.0));
    }

    #[test]
    fn heights_are_set_interpolated_and_graded() {
        let mut driver = ToolDriver::new();
        let id = add_polyline_of(
            &mut driver,
            &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)],
            false,
            &[Some(100.0), None, Some(110.0)],
        );
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.height");
        driver.pick(id, 0.0, 0.0);
        driver.r#type("101");
        assert_eq!(shape_of(&driver, id).vertices[0].height, Some(101.0));
        driver.pick(id, 10.0, 0.0);
        driver.r#type("None");
        assert!(shape_of(&driver, id).vertices[2].height.is_none());

        let mut second = ToolDriver::new();
        let other = add_polyline_of(
            &mut second,
            &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)],
            false,
            &[Some(100.0), None, Some(110.0)],
        );
        select_only(&mut second, vec![other]);
        second.set_pick_tolerance(0.5);
        second.start("draw.vertex.interpolate");
        second.enter();
        let Some(interpolated) = shape_of(&second, other).vertices[1].height else {
            panic!("vertex 1 has no height");
        };
        near(interpolated, 105.0, 1e-12);

        let mut third = ToolDriver::new();
        let graded = add_polyline_of(
            &mut third,
            &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)],
            false,
            &[Some(100.0), Some(50.0), Some(110.0)],
        );
        third.set_pick_tolerance(0.5);
        third.start("draw.vertex.grade");
        third.pick(graded, 0.0, 0.0);
        third.click(10.0, 0.0);
        let Some(height) = shape_of(&third, graded).vertices[1].height else {
            panic!("vertex 1 has no height");
        };
        near(height, 105.0, 1e-12);
    }

    #[test]
    fn segments_become_arcs_and_lines_again() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(2.0, 0.0), pt(2.0, 5.0)]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.arc");
        driver.pick(id, 1.0, 0.0);
        driver.click(1.0, -1.0);
        assert!(
            matches!(geometry_of(&driver, id), Geometry::CurvePolyline(_)),
            "a polyline with an arc is a curve polyline"
        );
        near(shape_of(&driver, id).vertices[0].bulge, 1.0, 1e-12);

        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.line");
        driver.pick(id, 1.0, -1.0);
        assert!(
            matches!(geometry_of(&driver, id), Geometry::Polyline(_)),
            "straight again, it is a Polyline2 again"
        );
    }

    #[test]
    fn fillet_and_chamfer_one_corner() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.fillet");
        driver.pick(id, 10.0, 0.0);
        driver.r#type("2");
        assert_eq!(shape_of(&driver, id).vertices.len(), 4);
        assert_eq!(shape_of(&driver, id).vertices[1].position, pt(8.0, 0.0));

        let mut second = ToolDriver::new();
        let other = add_polyline(&mut second, &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]);
        second.set_pick_tolerance(0.5);
        second.start("draw.vertex.chamfer");
        second.pick(other, 10.0, 0.0);
        second.r#type("2");
        second.r#type("3");
        assert_eq!(shape_of(&second, other).vertices[2].position, pt(10.0, 3.0));
        // A radius that does not fit is refused and the tool asks again.
        let mut third = ToolDriver::new();
        let small = add_polyline(&mut third, &[pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0)]);
        third.set_pick_tolerance(0.5);
        third.start("draw.vertex.fillet");
        third.pick(small, 1.0, 0.0);
        assert_eq!(third.r#type("50").outcome, REJECTED);
        assert_eq!(third.expects(), ToolInput::Value);
    }

    #[test]
    fn merge_and_snap_to_grid() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(
            &mut driver,
            &[pt(0.0, 0.0), pt(0.0005, 0.0), pt(5.2, 0.3), pt(9.9, 0.1)],
        );
        select_only(&mut driver, vec![id]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.merge");
        driver.enter(); // 0.001
        assert_eq!(shape_of(&driver, id).vertices.len(), 3);
        select_only(&mut driver, vec![id]);
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.grid");
        driver.r#type("1");
        assert_eq!(shape_of(&driver, id).vertices[1].position, pt(5.0, 0.0));
        assert_eq!(shape_of(&driver, id).vertices[2].position, pt(10.0, 0.0));
    }

    #[test]
    fn each_edit_is_one_undo_step() {
        let mut driver = ToolDriver::new();
        let id = add_polyline(&mut driver, &[pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0)]);
        let before = driver.document().history().undo_count();
        driver.set_pick_tolerance(0.5);
        driver.start("draw.vertex.delete");
        driver.pick(id, 5.0, 0.0);
        assert_eq!(driver.document().history().undo_count(), before + 1);
        ok(driver.document_mut().undo());
        assert_eq!(shape_of(&driver, id).vertices.len(), 3);
    }

    #[test]
    fn exploding_a_curve_polyline_gives_lines_and_arcs() {
        let mut driver = ToolDriver::new();
        let mut shape =
            CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(2.0, 0.0), pt(2.0, 5.0)], false);
        shape.vertices[0].bulge = 1.0;
        shape.vertices[0].height = Some(10.0);
        shape.vertices[1].height = Some(11.0);
        let id = add_curve(&mut driver, shape);
        select_only(&mut driver, vec![id]);
        driver.start("modify.explode");
        driver.enter();
        let mut arcs = 0;
        let mut lines = 0;
        driver
            .document()
            .model()
            .entities
            .for_each(|entity: &Entity| match &entity.geometry {
                Geometry::Arc(arc) => {
                    arcs += 1;
                    near(arc.radius, 1.0, 1e-12);
                    let heights = heights_of(&entity.properties, 2);
                    assert_eq!(heights[0], Some(10.0));
                    assert_eq!(heights[1], Some(11.0));
                }
                Geometry::Line(_) => lines += 1,
                _ => {}
            });
        assert_eq!(arcs, 1);
        assert_eq!(lines, 1);
    }

    #[test]
    fn offsetting_a_curve_polyline_makes_concentric_arcs() {
        // A straight 10 m then a quarter circle of radius 5 turning left:
        // offset 1 to the left (inside the turn) gives radius 4.
        let mut driver = ToolDriver::new();
        let mut shape =
            CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(15.0, 5.0)], false);
        // A quarter turn, counter-clockwise.
        shape.vertices[1].bulge = (1.0_f64.atan() / 2.0).tan();
        let id = add_curve(&mut driver, shape);
        driver.start("modify.offset");
        driver.r#type("1");
        driver.pick(id, 5.0, 0.0);
        driver.click(5.0, 3.0);
        driver.enter();
        let made = driver.document().last_created_entities();
        assert_eq!(made.len(), 1);
        let moved = curve_of(&driver, made[0]);
        assert_eq!(moved.vertices.len(), 3);
        near(moved.vertices[0].position.y, 1.0, 1e-9);
        let CurveSegment::Arc(arc) = moved.segment(1) else {
            panic!("segment 1 is not an arc");
        };
        near(arc.radius, 4.0, 1e-9);
        near(arc.center.distance_to(pt(10.0, 5.0)), 0.0, 1e-9);
    }

    #[test]
    fn breaking_a_curve_polyline_keeps_its_arcs_circle() {
        // A straight 10 m then a quarter circle of radius 5 about (10,5):
        // broken from (5,0) to the arc's middle, the piece kept past the gap is
        // the arc's second half, still on its circle, and the first piece a
        // straight polyline (stored as one) with the heights it had.
        let mut driver = ToolDriver::new();
        let mut shape =
            CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(15.0, 5.0)], false);
        shape.vertices[1].bulge = (PI / 8.0).tan();
        shape.vertices[0].height = Some(100.0);
        shape.vertices[1].height = Some(110.0);
        shape.vertices[2].height = Some(120.0);
        let id = add_curve(&mut driver, shape);
        let middle = pt(10.0, 5.0) + Vec2::new(5.0, 0.0).rotated(-PI / 4.0);
        driver.start("modify.break");
        assert_eq!(driver.pick(id, 5.0, 0.0).outcome, CONTINUE);
        assert_eq!(driver.click(middle.x, middle.y).outcome, DONE);
        let Some(first) = driver.document().model().entities.find(id) else {
            panic!("the first piece is gone");
        };
        assert_eq!(first.r#type(), EntityType::Polyline);
        let Some(first_shape) = read_polyline(first) else {
            panic!("the first piece is not a polyline");
        };
        let Some(last) = first_shape.vertices.last() else {
            panic!("the first piece has no vertex");
        };
        assert_eq!(last.position, pt(5.0, 0.0));
        let Some(last_height) = last.height else {
            panic!("the first piece's end has no height");
        };
        double_eq(last_height, 105.0);
        let made = driver.document().last_created_entities();
        assert_eq!(made.len(), 1);
        let second = curve_of(&driver, made[0]);
        assert_eq!(second.vertices.len(), 2);
        let CurveSegment::Arc(arc) = second.segment(0) else {
            panic!("segment 0 is not an arc");
        };
        near(arc.radius, 5.0, 1e-9);
        near(arc.center.distance_to(pt(10.0, 5.0)), 0.0, 1e-9);
        near(arc.sweep.abs(), PI / 4.0, 1e-9);
        let Some(start_height) = second.vertices[0].height else {
            panic!("the second piece's start has no height");
        };
        near(start_height, 115.0, 1e-9); // halfway along the arc
        ok(driver.document_mut().undo());
        assert_eq!(driver.document().model().entities.size(), 1);
    }

    #[test]
    fn trimming_a_curve_polyline_at_a_line_crossing_its_arc() {
        // The same path, cut by the line x = 10 + 5 sin 45 through the arc;
        // the pick on the arc's far end trims that end away.
        let mut driver = ToolDriver::new();
        let mut shape =
            CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(15.0, 5.0)], false);
        shape.vertices[1].bulge = (PI / 8.0).tan();
        let id = add_curve(&mut driver, shape);
        let x = 10.0 + 5.0 * (PI / 4.0).sin();
        let attributes = driver.document().current_attributes();
        let cutter = driver.add(create_line(pt(x, -10.0), pt(x, 10.0), attributes));
        driver.start("modify.trim");
        driver.pick(cutter, x, 0.0);
        driver.enter();
        assert_eq!(driver.pick(id, 14.9, 4.0).outcome, CONTINUE);
        assert_eq!(driver.enter().outcome, DONE);
        let kept = curve_of(&driver, id);
        assert_eq!(kept.vertices.len(), 3);
        let Some(last) = kept.vertices.last() else {
            panic!("no vertex");
        };
        near(last.position.x, x, 1e-9);
        let CurveSegment::Arc(arc) = kept.segment(1) else {
            panic!("segment 1 is not an arc");
        };
        near(arc.radius, 5.0, 1e-9);
        near(kept.length(), 10.0 + 5.0 * PI / 4.0, 1e-9);
    }

    #[test]
    fn a_curve_polyline_is_a_cutting_edge_for_trim() {
        let mut driver = ToolDriver::new();
        let mut shape = CurvePolyline2::from_points(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
        shape.vertices[0].bulge = 1.0; // a semicircle below the chord, centre (5,0), radius 5
        let edge = add_curve(&mut driver, shape);
        let attributes = driver.document().current_attributes();
        let target = driver.add(create_line(pt(5.0, 0.0), pt(5.0, -10.0), attributes));
        driver.start("modify.trim");
        driver.pick(edge, 5.0, -5.0);
        driver.enter();
        driver.pick(target, 5.0, -8.0);
        assert_eq!(driver.enter().outcome, DONE);
        let Geometry::Line(line) = geometry_of(&driver, target) else {
            panic!("the target is not a line");
        };
        near(line.end.y, -5.0, 1e-9); // cut where it crosses the arc
    }
}
