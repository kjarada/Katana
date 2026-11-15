// Lengthen and Reverse (src/katana_cad/tools/modify_length.rs;
// tests/cad/tools/test_modify_length.cpp), driven as the plan view drives
// them and checked against geometry worked out by hand. Straight results are
// exact in binary and compared exactly; an arc's end goes through cos and sin
// and is compared to 1e-12.

use katana_cad::interactive_tool::ToolInput;
use katana_commands::{
    EntityAttributes, create_arc, create_circle, create_entities, create_line, create_polyline,
};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::{Arc2, Point2, Polyline2, Segment2};
use katana_math::numerics::PI;

use crate::tool_driver::ToolDriver;
use crate::{CONTINUE, DONE, REJECTED, entity_of, near, ok};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn segment(ax: f64, ay: f64, bx: f64, by: f64) -> Segment2 {
    Segment2::new(p(ax, ay), p(bx, by))
}

#[track_caller]
fn segment_of(driver: &ToolDriver, id: EntityId) -> Segment2 {
    match entity_of(driver, id).geometry {
        Geometry::Line(line) => line,
        other => panic!("entity {id} is not of the kind expected: {other:?}"),
    }
}

#[track_caller]
fn polyline_of(driver: &ToolDriver, id: EntityId) -> Polyline2 {
    match entity_of(driver, id).geometry {
        Geometry::Polyline(polyline) => polyline,
        other => panic!("entity {id} is not of the kind expected: {other:?}"),
    }
}

#[track_caller]
fn arc_of(driver: &ToolDriver, id: EntityId) -> Arc2 {
    match entity_of(driver, id).geometry {
        Geometry::Arc(arc) => arc,
        other => panic!("entity {id} is not of the kind expected: {other:?}"),
    }
}

fn add_line(driver: &mut ToolDriver, a: Point2, b: Point2) -> EntityId {
    driver.add(create_line(a, b, EntityAttributes::default()))
}

fn add_arc(driver: &mut ToolDriver, arc: Arc2) -> EntityId {
    driver.add(create_arc(arc, EntityAttributes::default()))
}

fn add_polyline(driver: &mut ToolDriver, vertices: Vec<Point2>) -> EntityId {
    driver.add(create_polyline(
        Polyline2::new(vertices, false),
        EntityAttributes::default(),
    ))
}

// ---- Lengthen ----------------------------------------------------------------------

mod modify_lengthen {
    use super::*;

    #[test]
    fn picking_an_object_first_reports_its_length() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver, p(0.0, 0.0), p(3.0, 4.0));
        let arc = add_arc(&mut driver, Arc2::new(p(0.0, 0.0), 10.0, 0.0, PI / 2.0));
        driver.start("modify.lengthen");
        assert_eq!(driver.expects(), ToolInput::Entity);
        assert_eq!(driver.pick(line, 3.0, 4.0).message, "Current length 5.000");
        // 10 * pi / 2 = 15.70796.
        assert_eq!(
            driver.pick(arc, 10.0, 0.0).message,
            "Current length 15.708, included angle 90\u{b0}00'00.00\""
        );
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn a_delta_moves_the_end_nearer_the_pick_and_a_negative_one_shortens() {
        let mut driver = ToolDriver::new();
        let a = add_line(&mut driver, p(0.0, 0.0), p(10.0, 0.0));
        let b = add_line(&mut driver, p(0.0, 5.0), p(10.0, 5.0));
        driver.start("modify.lengthen");
        assert_eq!(driver.r#type("DE").outcome, CONTINUE);
        assert_eq!(driver.r#type("2").outcome, CONTINUE);
        // Nearer the end (10, 0): the end goes on 2, to (12, 0).
        assert_eq!(driver.pick(a, 9.0, 0.0).message, "Length 12.000");
        assert_eq!(driver.r#type("DE").outcome, CONTINUE);
        assert_eq!(driver.r#type("-2").outcome, CONTINUE);
        // Nearer the start (0, 5): 10 - 2 = 8 long from the fixed end
        // (10, 5), so the start comes in to (2, 5).
        let _ = driver.pick(b, 1.0, 5.0);
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        assert_eq!(step.message, "Lengthened 2 objects");
        assert_eq!(driver.executed(), 1);
        assert_eq!(segment_of(&driver, a), segment(0.0, 0.0, 12.0, 0.0));
        assert_eq!(segment_of(&driver, b), segment(2.0, 5.0, 10.0, 5.0));
        // One undo takes both back.
        ok(driver.document_mut().undo());
        assert_eq!(segment_of(&driver, a), segment(0.0, 0.0, 10.0, 0.0));
        assert_eq!(segment_of(&driver, b), segment(0.0, 5.0, 10.0, 5.0));
    }

    #[test]
    fn a_percentage_scales_the_length_from_the_far_end() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver, p(0.0, 0.0), p(10.0, 0.0));
        driver.start("modify.lengthen");
        let _ = driver.r#type("P");
        let _ = driver.r#type("50");
        let _ = driver.pick(line, 1.0, 0.0); // nearer the start; the end (10, 0) stays
        assert_eq!(driver.enter().outcome, DONE);
        // 50 % of 10 = 5, from (10, 0) back to (5, 0).
        assert_eq!(segment_of(&driver, line), segment(5.0, 0.0, 10.0, 0.0));
    }

    #[test]
    fn a_total_grows_a_polyline_along_its_end_segment_and_shrinks_it_along_its_path() {
        let mut driver = ToolDriver::new();
        // 10 east then 10 north: 20 long.
        let longer = add_polyline(&mut driver, vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]);
        let shorter = add_polyline(
            &mut driver,
            vec![p(0.0, 20.0), p(10.0, 20.0), p(10.0, 30.0)],
        );
        driver.start("modify.lengthen");
        let _ = driver.r#type("T");
        let _ = driver.r#type("25");
        // 25 - 20 = 5 more along the last leg, north: (10, 10) to (10, 15).
        let _ = driver.pick(longer, 10.0, 9.0);
        let _ = driver.r#type("T");
        let _ = driver.r#type("15");
        // 15 from the start: 10 east, then 5 of the northward leg - (10, 25).
        let _ = driver.pick(shorter, 10.0, 29.0);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(
            polyline_of(&driver, longer).vertices,
            vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 15.0)]
        );
        assert_eq!(
            polyline_of(&driver, shorter).vertices,
            vec![p(0.0, 20.0), p(10.0, 20.0), p(10.0, 25.0)]
        );
    }

    #[test]
    fn an_arc_keeps_its_centre_and_radius_and_changes_its_sweep() {
        let mut driver = ToolDriver::new();
        // A quarter circle of radius 10 from (10, 0) to (0, 10): 5 * pi long.
        let arc = add_arc(&mut driver, Arc2::new(p(0.0, 0.0), 10.0, 0.0, PI / 2.0));
        driver.start("modify.lengthen");
        let _ = driver.r#type("DE");
        // Another quarter: 5 * pi more makes a half circle ending at (-10, 0).
        let _ = driver.r#type("15.707963267948966");
        let _ = driver.pick(arc, 0.0, 10.0);
        assert_eq!(driver.enter().outcome, DONE);
        let half = arc_of(&driver, arc);
        assert_eq!(half.center, p(0.0, 0.0));
        assert_eq!(half.radius, 10.0);
        assert_eq!(half.start_angle, 0.0);
        near(half.sweep, PI, 1e-12);
        near(half.end_point().x, -10.0, 1e-12);
        near(half.end_point().y, 0.0, 1e-12);
    }

    #[test]
    fn dynamic_puts_the_end_where_the_cursor_is_along_the_line() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver, p(0.0, 0.0), p(10.0, 0.0));
        driver.start("modify.lengthen");
        let _ = driver.r#type("DY");
        let _ = driver.pick(line, 9.0, 0.0);
        assert_eq!(driver.expects(), ToolInput::Point);
        // (14, 3) projects onto the line's direction at 14.
        assert_eq!(driver.click(14.0, 3.0).outcome, CONTINUE);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(segment_of(&driver, line), segment(0.0, 0.0, 14.0, 0.0));
    }

    #[test]
    fn esc_keeps_the_changes_already_made() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver, p(0.0, 0.0), p(10.0, 0.0));
        driver.start("modify.lengthen");
        let _ = driver.r#type("DE");
        let _ = driver.r#type("5");
        let _ = driver.pick(line, 9.0, 0.0);
        let step = driver.cancel();
        assert_eq!(step.outcome, DONE);
        assert_eq!(driver.executed(), 1);
        assert_eq!(segment_of(&driver, line), segment(0.0, 0.0, 15.0, 0.0));
    }

    #[test]
    fn esc_before_any_change_keeps_nothing_and_applies_no_default() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver, p(0.0, 0.0), p(10.0, 0.0));
        driver.start("modify.lengthen");
        let _ = driver.r#type("T"); // at the total's prompt, whose Enter takes <1>
        let _ = driver.cancel();
        assert_eq!(driver.executed(), 0);
        assert_eq!(segment_of(&driver, line), segment(0.0, 0.0, 10.0, 0.0));
    }

    #[test]
    fn a_closed_outline_has_no_end_and_a_too_short_length_is_refused() {
        let mut driver = ToolDriver::new();
        let circle = driver.add(create_circle(p(0.0, 0.0), 5.0, EntityAttributes::default()));
        let line = add_line(&mut driver, p(0.0, 0.0), p(10.0, 0.0));
        driver.start("modify.lengthen");
        let _ = driver.r#type("DE");
        let _ = driver.r#type("-10");
        assert_eq!(
            driver.pick(circle, 5.0, 0.0).message,
            "a circle has no end to lengthen"
        );
        // 10 - 10 leaves nothing.
        assert_eq!(driver.pick(line, 9.0, 0.0).outcome, REJECTED);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn undo_takes_back_the_last_change_only() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver, p(0.0, 0.0), p(10.0, 0.0));
        driver.start("modify.lengthen");
        let _ = driver.r#type("DE");
        let _ = driver.r#type("1");
        let _ = driver.pick(line, 9.0, 0.0); // 11
        let _ = driver.pick(line, 9.0, 0.0); // 12
        assert_eq!(driver.undo().outcome, CONTINUE);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(segment_of(&driver, line), segment(0.0, 0.0, 11.0, 0.0));
    }
}

// ---- Reverse -----------------------------------------------------------------------

mod modify_reverse {
    use super::*;

    #[test]
    fn lines_arcs_and_polylines_turn_round_with_their_heights() {
        let mut driver = ToolDriver::new();
        let mut string = Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)],
                false,
            )),
            ..Entity::default()
        };
        set_heights(&mut string.properties, &[Some(1.0), Some(2.0), Some(3.0)]);
        let polyline = driver.add(create_entities(vec![string]));
        let line = add_line(&mut driver, p(0.0, 0.0), p(3.0, 4.0));
        let arc = add_arc(&mut driver, Arc2::new(p(0.0, 0.0), 10.0, 0.0, PI / 2.0));
        let circle = driver.add(create_circle(p(0.0, 0.0), 1.0, EntityAttributes::default()));
        driver
            .document_mut()
            .selection_mut()
            .set(vec![polyline, line, arc, circle]);
        driver.start("modify.reverse");
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        assert_eq!(
            step.message,
            "Reversed 3 objects; 1 skipped: circle has no direction"
        );
        assert_eq!(
            polyline_of(&driver, polyline).vertices,
            vec![p(10.0, 5.0), p(10.0, 0.0), p(0.0, 0.0)]
        );
        let turned = entity_of(&driver, polyline);
        let heights = heights_of(&turned.properties, 3);
        assert_eq!(heights, vec![Some(3.0), Some(2.0), Some(1.0)]);
        assert_eq!(segment_of(&driver, line), segment(3.0, 4.0, 0.0, 0.0));
        // The same quarter circle, drawn from (0, 10) back to (10, 0).
        let back = arc_of(&driver, arc);
        assert_eq!(back.start_angle, PI / 2.0);
        assert_eq!(back.sweep, -PI / 2.0);
        assert_eq!(driver.executed(), 1);
    }

    #[test]
    fn nothing_with_a_direction_is_refused_and_says_why() {
        let mut driver = ToolDriver::new();
        let circle = driver.add(create_circle(p(0.0, 0.0), 1.0, EntityAttributes::default()));
        driver.document_mut().selection_mut().set(vec![circle]);
        driver.start("modify.reverse");
        let step = driver.enter();
        assert_eq!(step.outcome, REJECTED);
        assert_eq!(
            step.message,
            "nothing selected can be reversed; 1 skipped: circle has no direction"
        );
    }
}
