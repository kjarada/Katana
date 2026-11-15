// The Draw > Curves tools: Circle and Arc in each construction
// (tools/draw_curves.rs; tests/cad/tools/test_draw_curves.cpp). Each is driven
// as a user would drive it - clicks, typed values and options, Undo, Enter -
// and what it draws is compared with geometry worked out by hand, the working
// beside each figure. Inputs are chosen so that the arithmetic is exact in
// binary wherever the construction allows it; where it cannot be (a square
// root, pi) the comparison is to within a few ulps or 1e-12.

use katana_cad::interactive_tool::{ToolInput, ToolStepOutcome as Outcome, tool_catalog};
use katana_commands::{
    EntityAttributes, create_circle, create_layer, create_line, create_polyline,
};
use katana_entity::{Entity, EntityId, Geometry, Layer};
use katana_geometry::{Arc2, Circle2, Point2, Segment2};
use katana_math::numerics::{HALF_PI, PI};

use crate::tool_driver::ToolDriver;
use crate::{double_eq, near, point, polyline, segment};

fn drawn(driver: &ToolDriver) -> Vec<Entity> {
    crate::entities_of(driver.document())
}

// The one circle in the drawing, or None when there is not exactly one entity
// or it is not a circle.
fn the_circle(driver: &ToolDriver) -> Option<Circle2> {
    let entities = drawn(driver);
    if entities.len() != 1 {
        return None;
    }
    match entities[0].geometry {
        Geometry::Circle(circle) => Some(circle),
        _ => None,
    }
}

fn the_arc(driver: &ToolDriver) -> Option<Arc2> {
    let entities = drawn(driver);
    if entities.len() != 1 {
        return None;
    }
    match entities[0].geometry {
        Geometry::Arc(arc) => Some(arc),
        _ => None,
    }
}

#[track_caller]
fn circle_of(entity: &Entity) -> Circle2 {
    match entity.geometry {
        Geometry::Circle(circle) => circle,
        _ => panic!("{:?} is not a circle", entity.geometry),
    }
}

#[track_caller]
fn arc_in(shape: &Geometry) -> Arc2 {
    match shape {
        Geometry::Arc(arc) => *arc,
        _ => panic!("{shape:?} is not an arc"),
    }
}

#[track_caller]
fn circle_in(shape: &Geometry) -> Circle2 {
    match shape {
        Geometry::Circle(circle) => *circle,
        _ => panic!("{shape:?} is not a circle"),
    }
}

#[track_caller]
fn line_in(shape: &Geometry) -> Segment2 {
    match shape {
        Geometry::Line(line) => *line,
        _ => panic!("{shape:?} is not a line"),
    }
}

fn circle(x: f64, y: f64, radius: f64) -> Circle2 {
    Circle2::new(point(x, y), radius)
}

fn arc(x: f64, y: f64, radius: f64, start: f64, sweep: f64) -> Arc2 {
    Arc2::new(point(x, y), radius, start, sweep)
}

fn add_line(driver: &mut ToolDriver, a: Point2, b: Point2) -> EntityId {
    driver.add(create_line(a, b, EntityAttributes::default()))
}

fn kerb_layer(driver: &mut ToolDriver) {
    crate::ok(driver.document_mut().execute(create_layer(Layer {
        name: String::from("kerb"),
        ..Layer::default()
    })));
    crate::ok(driver.document_mut().set_current_layer("kerb"));
}

mod draw_curves {
    use super::*;

    // ---- the catalogue -------------------------------------------------------------

    #[test]
    fn every_construction_is_its_own_tool_in_the_draw_menus_curves_group() {
        let catalog = tool_catalog();
        let ids = [
            "draw.circle",
            "draw.circle.diameter",
            "draw.circle.2p",
            "draw.circle.3p",
            "draw.circle.ttr",
            "draw.arc",
            "draw.arc.sce",
            "draw.arc.cse",
            "draw.arc.ser",
        ];
        let mut previous_order = -1;
        for id in ids {
            let Some(info) = catalog.find(id) else {
                panic!("{id}");
            };
            assert_eq!(info.category, "Draw", "{id}");
            assert_eq!(info.group, "Curves", "{id}");
            assert!(!info.tip.is_empty(), "{id}");
            // Listed above in menu order: circles first, then arcs.
            assert!(info.order > previous_order, "{id}");
            previous_order = info.order;
        }
        assert_eq!(
            catalog
                .find("draw.circle.3p")
                .map(|info| info.name.as_str()),
            Some("Circle, 3 Points")
        );
    }

    #[test]
    fn circle_and_arc_are_started_by_their_auto_cad_verbs_in_any_case() {
        let catalog = tool_catalog();
        for verb in ["CIRCLE", "C", "circle", "c"] {
            let Some(tool) = catalog.find_by_alias(verb) else {
                panic!("{verb}");
            };
            assert_eq!(tool.id, "draw.circle", "{verb}");
        }
        for verb in ["ARC", "A", "arc", "a"] {
            let Some(tool) = catalog.find_by_alias(verb) else {
                panic!("{verb}");
            };
            assert_eq!(tool.id, "draw.arc", "{verb}");
        }
    }

    // ---- Circle: centre and radius -------------------------------------------------

    #[test]
    fn a_circle_from_a_centre_and_a_point_on_it_has_their_distance_for_its_radius() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        assert_eq!(driver.prompt(), "Specify centre point or [3P/2P/Ttr]");
        assert_eq!(driver.expects(), ToolInput::Point);
        assert_eq!(driver.click(10.0, 20.0).outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify radius or [Diameter/Undo]");
        assert_eq!(driver.click(13.0, 24.0).outcome, Outcome::Done);
        // (13, 24) - (10, 20) = (3, 4): a 3-4-5 triangle, radius 5 exactly.
        assert_eq!(the_circle(&driver), Some(circle(10.0, 20.0, 5.0)));
    }

    #[test]
    fn a_number_typed_at_the_radius_prompt_is_the_radius() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.r#type("10,20");
        assert_eq!(driver.r#type("12.5").outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(10.0, 20.0, 12.5)));
    }

    #[test]
    fn relative_and_polar_points_at_the_radius_prompt_measure_from_the_centre() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.r#type("10,20");
        // @3,4 from the centre is (13, 24): radius 5, as the clicked case.
        assert_eq!(driver.r#type("@3,4").outcome, Outcome::Done);
        let entities = drawn(&driver);
        assert_eq!(entities.len(), 1);
        assert_eq!(circle_of(&entities[0]), circle(10.0, 20.0, 5.0));

        crate::ok(driver.document_mut().undo());
        driver.r#type("10,20");
        // 5 at 90 degrees is straight up. cos(90 deg) is ~6e-17, not 0, so the
        // offset is (3e-16, 5) - but 3e-16 is under half an ulp of 10
        // (1.8e-15), so the point rounds to (10, 25) and the radius is 5.
        assert_eq!(driver.r#type("@5<90").outcome, Outcome::Done);
        let Some(made) = the_circle(&driver) else {
            panic!("one circle");
        };
        assert_eq!(made.center, point(10.0, 20.0));
        double_eq(made.radius, 5.0);
    }

    #[test]
    fn the_diameter_option_takes_a_typed_or_picked_diameter() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.click(10.0, 20.0);
        assert_eq!(driver.r#type("d").outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify diameter or [Radius/Undo]");
        assert_eq!(driver.r#type("10").outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(10.0, 20.0, 5.0))); // diameter 10 / 2

        crate::ok(driver.document_mut().undo());
        driver.click(10.0, 20.0);
        driver.r#type("DIAMETER");
        // A picked point is the far end of the diameter from the centre: 10
        // away along x, so the radius is 5.
        assert_eq!(driver.click(20.0, 20.0).outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(10.0, 20.0, 5.0)));
    }

    #[test]
    fn the_diameter_variant_asks_for_a_diameter_and_can_switch_to_a_radius() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle.diameter");
        // Not the general CIRCLE: no other constructions are offered.
        assert_eq!(driver.prompt(), "Specify centre point");
        driver.click(0.0, 0.0);
        assert_eq!(driver.prompt(), "Specify diameter or [Radius/Undo]");
        assert_eq!(driver.r#type("8").outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(0.0, 0.0, 4.0)));

        crate::ok(driver.document_mut().undo());
        driver.click(0.0, 0.0);
        driver.r#type("R");
        assert_eq!(driver.r#type("8").outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(0.0, 0.0, 8.0)));
    }

    #[test]
    fn circle_starts_again_after_each_circle_until_the_user_stops_it() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.click(0.0, 0.0);
        driver.r#type("1");
        assert!(!driver.finished());
        assert_eq!(driver.prompt(), "Specify centre point or [3P/2P/Ttr]");
        driver.click(10.0, 0.0);
        driver.r#type("2");
        assert_eq!(driver.executed(), 2);
        assert_eq!(drawn(&driver).len(), 2);
    }

    #[test]
    fn circle_refuses_a_non_positive_or_malformed_radius_and_keeps_its_centre() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.click(10.0, 20.0);
        let prompt = driver.prompt();
        for bad in ["0", "-3", "abc", "1e999"] {
            let step = driver.r#type(bad);
            assert_eq!(step.outcome, Outcome::Rejected, "{bad}");
            assert!(!step.message.is_empty(), "{bad}");
            assert_eq!(driver.prompt(), prompt, "{bad}");
        }
        // A point on the centre is a zero radius.
        assert_eq!(driver.click(10.0, 20.0).outcome, Outcome::Rejected);
        assert!(drawn(&driver).is_empty());
        // Still at the radius prompt with the centre kept.
        assert_eq!(driver.r#type("5").outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(10.0, 20.0, 5.0)));
    }

    #[test]
    fn undo_inside_circle_steps_back_over_the_centre_and_over_an_option() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        // Nothing to step back over yet.
        assert_eq!(driver.undo().outcome, Outcome::Rejected);
        driver.click(1.0, 1.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify centre point or [3P/2P/Ttr]");
        driver.click(10.0, 20.0);
        driver.r#type("D");
        // U typed is the same as the Undo button, and takes back the option
        // only.
        assert_eq!(driver.r#type("u").outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify radius or [Diameter/Undo]");
        assert_eq!(driver.r#type("5").outcome, Outcome::Done);
        // A radius of 5, not a diameter.
        assert_eq!(the_circle(&driver), Some(circle(10.0, 20.0, 5.0)));
    }

    #[test]
    fn enter_part_way_through_a_circle_ends_the_tool_with_nothing_drawn() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.click(10.0, 20.0);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert!(driver.finished());
        assert_eq!(driver.executed(), 0);
        assert!(drawn(&driver).is_empty());
    }

    #[test]
    fn one_undo_of_the_document_removes_the_circle() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.click(10.0, 20.0);
        driver.r#type("5");
        assert_eq!(drawn(&driver).len(), 1);
        crate::ok(driver.document_mut().undo());
        assert!(drawn(&driver).is_empty());
    }

    #[test]
    fn a_circle_is_drawn_with_the_documents_current_layer() {
        let mut driver = ToolDriver::new();
        kerb_layer(&mut driver);
        driver.start("draw.circle");
        driver.click(0.0, 0.0);
        driver.r#type("2");
        let entities = drawn(&driver);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].layer, "kerb");
    }

    #[test]
    fn the_circle_preview_is_the_circle_through_the_cursor_with_its_radius() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        assert!(driver.preview(13.0, 24.0).shapes.is_empty());
        driver.click(10.0, 20.0);
        let feedback = driver.preview(13.0, 24.0);
        assert_eq!(feedback.shapes.len(), 2);
        assert_eq!(circle_in(&feedback.shapes[0]), circle(10.0, 20.0, 5.0));
        assert_eq!(
            line_in(&feedback.shapes[1]),
            segment([10.0, 20.0], [13.0, 24.0])
        );
        assert_eq!(feedback.markers, [point(10.0, 20.0)]);
    }

    #[test]
    fn the_diameter_preview_is_the_circle_whose_diameter_reaches_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle.diameter");
        driver.click(10.0, 20.0);
        // The cursor is 10 from the centre along x. At the diameter prompt
        // that distance is the diameter, so the rubber band is the circle of
        // radius 5 - the one a click there draws
        // (the_diameter_option_takes_a_typed_or_picked_diameter) - and not
        // the circle of radius 10 through the cursor.
        let feedback = driver.preview(20.0, 20.0);
        assert_eq!(feedback.shapes.len(), 2);
        assert_eq!(circle_in(&feedback.shapes[0]), circle(10.0, 20.0, 5.0));
        assert_eq!(
            line_in(&feedback.shapes[1]),
            segment([10.0, 20.0], [20.0, 20.0])
        );
        assert_eq!(feedback.markers, [point(10.0, 20.0)]);
    }

    // ---- Circle: two points --------------------------------------------------------

    #[test]
    fn a_two_point_circle_has_its_diameter_between_the_points() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle.2p");
        assert_eq!(
            driver.prompt(),
            "Specify first end of the circle's diameter"
        );
        driver.click(2.0, 4.0);
        assert_eq!(
            driver.prompt(),
            "Specify second end of the circle's diameter or [Undo]"
        );
        assert_eq!(driver.click(8.0, 12.0).outcome, Outcome::Done);
        // Centre midway, (5, 8); the ends are (6, 8) apart, 10, so the radius
        // is 5.
        assert_eq!(the_circle(&driver), Some(circle(5.0, 8.0, 5.0)));
    }

    #[test]
    fn the_general_circle_takes2_p_and_polar_input_from_the_first_end() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        assert_eq!(driver.r#type("2p").outcome, Outcome::Continue);
        driver.r#type("2,4");
        // 10 due east of (2, 4) is (12, 4) exactly (cos 0 and sin 0 are
        // exact): centre (7, 4), radius 5.
        assert_eq!(driver.r#type("@10<0").outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(7.0, 4.0, 5.0)));
    }

    #[test]
    fn a_two_point_circle_refuses_coincident_ends() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle.2p");
        driver.click(2.0, 4.0);
        let step = driver.click(2.0, 4.0);
        assert_eq!(step.outcome, Outcome::Rejected);
        assert!(!step.message.is_empty());
        assert!(drawn(&driver).is_empty());
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify first end of the circle's diameter"
        );
    }

    #[test]
    fn the_two_point_preview_is_the_circle_on_the_first_end_and_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle.2p");
        driver.click(2.0, 4.0);
        let feedback = driver.preview(8.0, 12.0);
        assert!(!feedback.shapes.is_empty());
        assert_eq!(circle_in(&feedback.shapes[0]), circle(5.0, 8.0, 5.0));
    }

    // ---- Circle: three points ------------------------------------------------------

    #[test]
    fn a_three_point_circle_is_the_circumcircle_of_the_points() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle.3p");
        assert_eq!(driver.prompt(), "Specify first point on the circle");
        driver.click(10.0, 0.0);
        driver.click(0.0, 10.0);
        assert_eq!(
            driver.prompt(),
            "Specify third point on the circle or [Undo]"
        );
        assert_eq!(driver.click(-10.0, 0.0).outcome, Outcome::Done);
        // All three are 10 from the origin, which is therefore the centre.
        assert_eq!(the_circle(&driver), Some(circle(0.0, 0.0, 10.0)));
    }

    #[test]
    fn a_three_point_circle_refuses_points_in_a_line_and_coincident_points() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.r#type("3P");
        driver.click(0.0, 0.0);
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Rejected); // on the first
        driver.click(5.0, 5.0);
        let in_line = driver.click(10.0, 10.0);
        assert_eq!(in_line.outcome, Outcome::Rejected);
        assert!(in_line.message.contains("line"));
        assert_eq!(driver.click(5.0, 5.0).outcome, Outcome::Rejected); // on the second
        assert!(drawn(&driver).is_empty());
        // The tool kept both points. Through (0,0), (5,5) and (10,0): the
        // centre is on x = 5, the bisector of the first and last, and (5,y) is
        // as far from (0,0) as from (5,5) when 25 + y^2 = (5 - y)^2, so y = 0
        // and r = 5.
        assert_eq!(driver.click(10.0, 0.0).outcome, Outcome::Done);
        assert_eq!(the_circle(&driver), Some(circle(5.0, 0.0, 5.0)));
    }

    #[test]
    fn the_three_point_preview_is_the_circle_through_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.circle.3p");
        driver.click(10.0, 0.0);
        assert_eq!(
            line_in(&driver.preview(0.0, 10.0).shapes[0]),
            segment([10.0, 0.0], [0.0, 10.0])
        );
        driver.click(0.0, 10.0);
        let feedback = driver.preview(-10.0, 0.0);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(circle_in(&feedback.shapes[0]), circle(0.0, 0.0, 10.0));
        assert_eq!(feedback.markers.len(), 2);
    }

    // ---- Circle: tangent, tangent, radius ------------------------------------------

    #[test]
    fn a_tangent_tangent_radius_circle_sits_in_the_corner_of_two_perpendicular_lines() {
        let mut driver = ToolDriver::new();
        let x = add_line(&mut driver, point(0.0, 0.0), point(20.0, 0.0));
        let y = add_line(&mut driver, point(0.0, 0.0), point(0.0, 20.0));
        driver.start("draw.circle.ttr");
        assert_eq!(driver.prompt(), "Select the first line to touch");
        assert_eq!(driver.expects(), ToolInput::Entity);
        assert_eq!(driver.pick(x, 10.0, 0.0).outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Select the second line to touch or [Undo]");
        assert_eq!(driver.pick(y, 0.0, 10.0).outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Specify radius of circle or [Undo]");
        assert_eq!(driver.expects(), ToolInput::Value);
        assert_eq!(driver.r#type("5").outcome, Outcome::Done);
        // Touching y = 0 and x = 0 with radius 5, in the quadrant both picks
        // are in: the centre is 5 from each axis, (5, 5).
        let entities = drawn(&driver);
        assert_eq!(entities.len(), 3);
        assert_eq!(circle_of(&entities[2]), circle(5.0, 5.0, 5.0));
    }

    #[test]
    fn of_the_four_tangent_circles_the_one_taken_is_in_the_corner_whose_arms_were_picked() {
        // Two lines crossing at the origin make four corners, and a circle of
        // radius 5 fits in each, centred at (+-5, +-5). The picks say which:
        // the circle is between the half of each line on the side it was
        // picked.
        let cases = [
            (point(10.0, 0.0), point(0.0, 10.0), point(5.0, 5.0)),
            (point(-10.0, 0.0), point(0.0, 10.0), point(-5.0, 5.0)),
            (point(-10.0, 0.0), point(0.0, -10.0), point(-5.0, -5.0)),
            (point(10.0, 0.0), point(0.0, -10.0), point(5.0, -5.0)),
        ];
        for (on_x, on_y, centre) in cases {
            let mut driver = ToolDriver::new();
            let x = add_line(&mut driver, point(-20.0, 0.0), point(20.0, 0.0));
            let y = add_line(&mut driver, point(0.0, -20.0), point(0.0, 20.0));
            driver.start("draw.circle.ttr");
            driver.pick(x, on_x.x, on_x.y);
            driver.pick(y, on_y.x, on_y.y);
            assert_eq!(driver.r#type("5").outcome, Outcome::Done);
            let entities = drawn(&driver);
            assert_eq!(entities.len(), 3);
            assert_eq!(
                circle_of(&entities[2]),
                Circle2::new(centre, 5.0),
                "{},{}",
                on_x.x,
                on_y.y
            );
        }
    }

    #[test]
    fn a_tangent_tangent_radius_circle_fits_an_oblique_corner() {
        let mut driver = ToolDriver::new();
        let x = add_line(&mut driver, point(0.0, 0.0), point(20.0, 0.0));
        let slope = add_line(&mut driver, point(0.0, 0.0), point(6.0, 8.0));
        driver.start("draw.circle");
        assert_eq!(driver.r#type("ttr").outcome, Outcome::Continue);
        driver.pick(x, 10.0, 0.0);
        driver.pick(slope, 3.0, 4.0);
        assert_eq!(driver.r#type("4").outcome, Outcome::Done);
        // Arms u1 = (1, 0) and u2 = (0.6, 0.8); sin of the angle between them
        // is 0.8. The centre c = (u1 + u2) r / sin = (1.6, 0.8) * 4 / 0.8 =
        // (8, 4). Check: 4 above y = 0, and |0.6*4 - 0.8*8| = 4 from the
        // sloping line.
        let entities = drawn(&driver);
        assert_eq!(entities.len(), 3);
        let made = circle_of(&entities[2]);
        near(made.center.x, 8.0, 1e-12);
        near(made.center.y, 4.0, 1e-12);
        assert_eq!(made.radius, 4.0);
    }

    #[test]
    fn a_tangent_tangent_radius_circle_touches_two_segments_of_one_polyline() {
        let mut driver = ToolDriver::new();
        let id = driver.add(create_polyline(
            polyline(&[[0.0, 0.0], [20.0, 0.0], [20.0, 20.0]], false),
            EntityAttributes::default(),
        ));
        driver.start("draw.circle.ttr");
        assert_eq!(driver.pick(id, 10.0, 0.0).outcome, Outcome::Continue);
        assert_eq!(driver.pick(id, 20.0, 10.0).outcome, Outcome::Continue);
        assert_eq!(driver.r#type("5").outcome, Outcome::Done);
        // The corner is (20, 0), its arms running west and north: the centre
        // is 5 west and 5 north of it, (15, 5).
        let entities = drawn(&driver);
        assert_eq!(entities.len(), 2);
        assert_eq!(circle_of(&entities[1]), circle(15.0, 5.0, 5.0));
    }

    #[test]
    fn a_polylines_zero_length_segment_is_passed_over_for_the_segment_beside_it() {
        // A repeated vertex makes a segment of no length at (0, 0), and a pick
        // at (-1, 0) is 1 from it and 1 from the real segment after it: a tie
        // the zero-length one would win by coming first, and then be refused
        // as having no direction. The real segment, y = 0, is the line meant.
        let mut driver = ToolDriver::new();
        let id = driver.add(create_polyline(
            polyline(&[[0.0, 0.0], [0.0, 0.0], [20.0, 0.0]], false),
            EntityAttributes::default(),
        ));
        let y = add_line(&mut driver, point(0.0, 0.0), point(0.0, 20.0));
        driver.start("draw.circle.ttr");
        assert_eq!(driver.pick(id, -1.0, 0.0).outcome, Outcome::Continue);
        assert_eq!(driver.pick(y, 0.0, 10.0).outcome, Outcome::Continue);
        assert_eq!(driver.r#type("5").outcome, Outcome::Done);
        // The arms run west (towards the pick at x = -1) and north: centre
        // (-5, 5).
        let entities = drawn(&driver);
        assert_eq!(
            circle_of(&entities[entities.len() - 1]),
            circle(-5.0, 5.0, 5.0)
        );
    }

    #[test]
    fn tangent_tangent_radius_refuses_what_has_no_corner_to_fit_in() {
        let mut driver = ToolDriver::new();
        let bottom = add_line(&mut driver, point(0.0, 0.0), point(20.0, 0.0));
        let top = add_line(&mut driver, point(0.0, 10.0), point(20.0, 10.0));
        let side = add_line(&mut driver, point(0.0, -5.0), point(0.0, 20.0));
        let ring = driver.add(create_circle(
            point(50.0, 50.0),
            3.0,
            EntityAttributes::default(),
        ));
        driver.start("draw.circle.ttr");

        assert_eq!(driver.pick(ring, 53.0, 50.0).outcome, Outcome::Rejected); // not a line
        assert_eq!(driver.pick(999, 0.0, 0.0).outcome, Outcome::Rejected); // not in the drawing
        assert_eq!(driver.click(5.0, 5.0).outcome, Outcome::Rejected); // a point, not a line
        assert_eq!(driver.r#type("5").outcome, Outcome::Rejected); // not yet a radius
        assert_eq!(driver.prompt(), "Select the first line to touch");

        assert_eq!(driver.pick(bottom, 10.0, 0.0).outcome, Outcome::Continue);
        let parallel = driver.pick(top, 10.0, 10.0);
        assert_eq!(parallel.outcome, Outcome::Rejected);
        assert!(parallel.message.contains("parallel"));
        assert_eq!(driver.pick(bottom, 15.0, 0.0).outcome, Outcome::Rejected); // the same line
        // The side crosses the bottom at the origin; picked there, it does not
        // say which corner is meant.
        assert_eq!(driver.pick(side, 0.0, 0.0).outcome, Outcome::Rejected);
        assert_eq!(driver.prompt(), "Select the second line to touch or [Undo]");

        assert_eq!(driver.pick(side, 0.0, 10.0).outcome, Outcome::Continue);
        for bad in ["0", "-1", "abc", "3,4"] {
            assert_eq!(driver.r#type(bad).outcome, Outcome::Rejected, "{bad}");
        }
        assert_eq!(driver.click(5.0, 5.0).outcome, Outcome::Rejected);
        assert_eq!(drawn(&driver).len(), 4);
        assert_eq!(driver.r#type("2").outcome, Outcome::Done);
        let entities = drawn(&driver);
        assert_eq!(
            circle_of(&entities[entities.len() - 1]),
            circle(2.0, 2.0, 2.0)
        );
    }

    #[test]
    fn undo_inside_tangent_tangent_radius_forgets_the_last_line() {
        let mut driver = ToolDriver::new();
        let x = add_line(&mut driver, point(0.0, 0.0), point(20.0, 0.0));
        let y = add_line(&mut driver, point(0.0, 0.0), point(0.0, 20.0));
        driver.start("draw.circle.ttr");
        driver.pick(x, 10.0, 0.0);
        driver.pick(y, 0.0, 10.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Select the second line to touch or [Undo]");
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(driver.prompt(), "Select the first line to touch");
        // Picked the other way round, the same corner and the same circle.
        driver.pick(y, 0.0, 10.0);
        driver.pick(x, 10.0, 0.0);
        driver.r#type("5");
        let entities = drawn(&driver);
        assert_eq!(
            circle_of(&entities[entities.len() - 1]),
            circle(5.0, 5.0, 5.0)
        );
        // One undo of the document takes the circle and leaves the lines.
        crate::ok(driver.document_mut().undo());
        assert_eq!(drawn(&driver).len(), 2);
    }

    #[test]
    fn the_tangent_tangent_radius_preview_marks_the_picks_and_the_corner() {
        let mut driver = ToolDriver::new();
        let x = add_line(&mut driver, point(0.0, 0.0), point(20.0, 0.0));
        let y = add_line(&mut driver, point(0.0, 0.0), point(0.0, 20.0));
        driver.start("draw.circle.ttr");
        driver.pick(x, 10.0, 0.5); // a pick beside the line marks the line
        driver.pick(y, 0.0, 10.0);
        let feedback = driver.preview(30.0, 30.0);
        assert_eq!(
            feedback.markers,
            [point(10.0, 0.0), point(0.0, 10.0), point(0.0, 0.0)]
        );
    }

    // ---- Arc: three points ---------------------------------------------------------

    #[test]
    fn a_three_point_arc_runs_counter_clockwise_through_its_points() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        assert_eq!(driver.prompt(), "Specify start point of arc or [Centre]");
        driver.click(10.0, 0.0);
        assert_eq!(
            driver.prompt(),
            "Specify second point of arc or [Centre/End/Undo]"
        );
        driver.click(0.0, 10.0);
        assert_eq!(driver.prompt(), "Specify end point of arc or [Undo]");
        assert_eq!(driver.click(-10.0, 0.0).outcome, Outcome::Done);
        // The upper half of the circle of radius 10 about the origin: from 0
        // counter-clockwise through pi/2 to pi, a sweep of pi.
        assert_eq!(the_arc(&driver), Some(arc(0.0, 0.0, 10.0, 0.0, PI)));
    }

    #[test]
    fn a_three_point_arc_picked_clockwise_is_stored_counter_clockwise_from_its_other_end() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        driver.click(-10.0, 0.0);
        driver.click(0.0, 10.0);
        driver.click(10.0, 0.0);
        // The same upper half circle, picked clockwise from (-10, 0): stored
        // from (10, 0) at angle 0, counter-clockwise through pi.
        assert_eq!(the_arc(&driver), Some(arc(0.0, 0.0, 10.0, 0.0, PI)));
    }

    #[test]
    fn a_three_point_arc_takes_relative_points_from_the_point_before() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        driver.r#type("10,0");
        driver.r#type("@-10,10"); // (0, 10)
        assert_eq!(driver.r#type("@-10,-10").outcome, Outcome::Done); // (-10, 0)
        assert_eq!(the_arc(&driver), Some(arc(0.0, 0.0, 10.0, 0.0, PI)));
    }

    #[test]
    fn a_three_point_arc_refuses_points_in_a_line_and_coincident_points() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        driver.click(0.0, 0.0);
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Rejected);
        driver.click(5.0, 5.0);
        let in_line = driver.click(10.0, 10.0);
        assert_eq!(in_line.outcome, Outcome::Rejected);
        assert!(in_line.message.contains("line"));
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Rejected);
        assert_eq!(driver.r#type("12").outcome, Outcome::Rejected); // a number is not a point
        assert!(drawn(&driver).is_empty());
        assert_eq!(driver.prompt(), "Specify end point of arc or [Undo]");
    }

    #[test]
    fn undo_inside_arc_steps_back_one_point_and_enter_ends_it() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        driver.click(10.0, 0.0);
        driver.click(3.0, 3.0);
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        driver.click(0.0, 10.0);
        driver.click(-10.0, 0.0);
        assert_eq!(the_arc(&driver), Some(arc(0.0, 0.0, 10.0, 0.0, PI)));
        // It started again; Enter now ends it.
        assert!(!driver.finished());
        driver.click(1.0, 1.0);
        assert_eq!(driver.enter().outcome, Outcome::Done);
        assert!(driver.finished());
        assert_eq!(drawn(&driver).len(), 1);
    }

    #[test]
    fn one_undo_of_the_document_removes_the_arc() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        driver.click(10.0, 0.0);
        driver.click(0.0, 10.0);
        driver.click(-10.0, 0.0);
        assert_eq!(drawn(&driver).len(), 1);
        crate::ok(driver.document_mut().undo());
        assert!(drawn(&driver).is_empty());
    }

    #[test]
    fn the_three_point_arc_preview_is_the_arc_through_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        driver.click(10.0, 0.0);
        assert_eq!(
            line_in(&driver.preview(0.0, 10.0).shapes[0]),
            segment([10.0, 0.0], [0.0, 10.0])
        );
        driver.click(0.0, 10.0);
        let feedback = driver.preview(-10.0, 0.0);
        assert_eq!(feedback.shapes.len(), 1);
        assert_eq!(arc_in(&feedback.shapes[0]), arc(0.0, 0.0, 10.0, 0.0, PI));
        assert_eq!(feedback.markers.len(), 2);
    }

    // ---- Arc: start, centre, end / centre, start, end ------------------------------

    #[test]
    fn a_start_centre_end_arc_runs_counter_clockwise_to_the_ends_direction() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.sce");
        assert_eq!(driver.prompt(), "Specify start point of arc");
        driver.click(10.0, 0.0);
        assert_eq!(driver.prompt(), "Specify centre point of arc or [Undo]");
        driver.click(0.0, 0.0);
        assert_eq!(
            driver.prompt(),
            "Specify end point of arc, counter-clockwise from the start or [Angle/Undo]"
        );
        // The end only aims: (0, 5) is inside the circle, due north of the
        // centre, so the arc ends at (0, 10). Radius 10 from the start; 0 to
        // pi/2.
        assert_eq!(driver.click(0.0, 5.0).outcome, Outcome::Done);
        assert_eq!(the_arc(&driver), Some(arc(0.0, 0.0, 10.0, 0.0, HALF_PI)));
    }

    #[test]
    fn a_start_centre_end_arc_goes_the_long_way_when_the_end_is_clockwise_of_the_start() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.sce");
        driver.click(10.0, 0.0);
        driver.click(0.0, 0.0);
        driver.click(0.0, -10.0);
        // Due south is -pi/2; counter-clockwise from 0 that is three quarters
        // of a turn, 3 pi / 2 - never the quarter turn clockwise.
        let Some(made) = the_arc(&driver) else {
            panic!("one arc");
        };
        assert_eq!(made.start_angle, 0.0);
        double_eq(made.sweep, 3.0 * HALF_PI);
    }

    #[test]
    fn the_angle_option_turns_the_arc_through_a_typed_angle_either_way() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.sce");
        driver.click(10.0, 0.0);
        driver.click(0.0, 0.0);
        assert_eq!(driver.r#type("a").outcome, Outcome::Continue);
        assert_eq!(driver.expects(), ToolInput::Value);
        assert_eq!(
            driver.prompt(),
            "Specify included angle in degrees (negative for clockwise) or [Undo]"
        );
        assert_eq!(driver.r#type("90").outcome, Outcome::Done);
        let Some(made) = the_arc(&driver) else {
            panic!("one arc");
        };
        assert_eq!(made.start_angle, 0.0);
        double_eq(made.sweep, HALF_PI);

        crate::ok(driver.document_mut().undo());
        driver.click(10.0, 0.0);
        driver.click(0.0, 0.0);
        driver.r#type("ANGLE");
        // A quarter turn clockwise from (10, 0) ends at (0, -10); stored
        // counter-clockwise from there: start 3 pi / 2, sweep pi / 2.
        assert_eq!(driver.r#type("-90").outcome, Outcome::Done);
        let Some(made) = the_arc(&driver) else {
            panic!("one arc");
        };
        double_eq(made.start_angle, 3.0 * HALF_PI);
        double_eq(made.sweep, HALF_PI);
    }

    #[test]
    fn a_centre_start_end_arc_takes_the_centre_first() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.cse");
        assert_eq!(driver.prompt(), "Specify centre point of arc");
        driver.click(0.0, 0.0);
        assert_eq!(driver.prompt(), "Specify start point of arc or [Undo]");
        driver.click(0.0, 10.0);
        // Start due north (pi/2), end aimed due west (pi): a quarter turn.
        assert_eq!(driver.click(-3.0, 0.0).outcome, Outcome::Done);
        assert_eq!(
            the_arc(&driver),
            Some(arc(0.0, 0.0, 10.0, HALF_PI, HALF_PI))
        );
    }

    #[test]
    fn the_general_arc_reaches_each_construction_by_its_option() {
        // C first: centre, start, end.
        {
            let mut driver = ToolDriver::new();
            driver.start("draw.arc");
            assert_eq!(driver.r#type("C").outcome, Outcome::Continue);
            assert_eq!(driver.prompt(), "Specify centre point of arc or [Undo]");
            driver.click(0.0, 0.0);
            driver.click(0.0, 10.0);
            driver.click(-3.0, 0.0);
            assert_eq!(
                the_arc(&driver),
                Some(arc(0.0, 0.0, 10.0, HALF_PI, HALF_PI))
            );
        }
        // C after the start: start, centre, end.
        {
            let mut driver = ToolDriver::new();
            driver.start("draw.arc");
            driver.click(10.0, 0.0);
            assert_eq!(driver.r#type("centre").outcome, Outcome::Continue);
            assert_eq!(driver.prompt(), "Specify centre point of arc or [Undo]");
            driver.click(0.0, 0.0);
            driver.click(0.0, 5.0);
            assert_eq!(the_arc(&driver), Some(arc(0.0, 0.0, 10.0, 0.0, HALF_PI)));
        }
        // E after the start: start, end, radius (the radius case is worked in
        // a_start_end_radius_arc_takes_the_minor_arc_for_a_positive_radius).
        {
            let mut driver = ToolDriver::new();
            driver.start("draw.arc");
            driver.click(0.0, 0.0);
            assert_eq!(driver.r#type("e").outcome, Outcome::Continue);
            assert_eq!(driver.prompt(), "Specify end point of arc or [Undo]");
            driver.click(8.0, 0.0);
            assert_eq!(driver.r#type("5").outcome, Outcome::Done);
            let Some(made) = the_arc(&driver) else {
                panic!("one arc");
            };
            assert_eq!(made.center, point(4.0, 3.0));
            assert_eq!(made.radius, 5.0);
        }
    }

    #[test]
    fn undo_inside_the_general_arc_takes_back_an_option() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc");
        driver.click(10.0, 0.0);
        driver.r#type("C");
        assert_eq!(driver.undo().outcome, Outcome::Continue);
        assert_eq!(
            driver.prompt(),
            "Specify second point of arc or [Centre/End/Undo]"
        );
        driver.click(0.0, 10.0);
        driver.click(-10.0, 0.0);
        assert_eq!(the_arc(&driver), Some(arc(0.0, 0.0, 10.0, 0.0, PI)));
    }

    #[test]
    fn a_centred_arc_refuses_a_zero_radius_an_end_with_no_direction_and_an_empty_sweep() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.sce");
        driver.click(10.0, 0.0);
        assert_eq!(driver.click(10.0, 0.0).outcome, Outcome::Rejected); // centre on the start
        driver.click(0.0, 0.0);
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Rejected); // end on the centre
        let along = driver.click(20.0, 0.0); // the start's own direction
        assert_eq!(along.outcome, Outcome::Rejected);
        assert!(along.message.contains("empty"));
        assert_eq!(driver.r#type("xyz").outcome, Outcome::Rejected);
        driver.r#type("A");
        for bad in ["0", "360", "-400", "abc"] {
            assert_eq!(driver.r#type(bad).outcome, Outcome::Rejected, "{bad}");
        }
        assert_eq!(driver.click(0.0, 10.0).outcome, Outcome::Rejected); // an angle is typed
        assert!(drawn(&driver).is_empty());

        let mut centre_first = ToolDriver::new();
        centre_first.start("draw.arc.cse");
        centre_first.click(0.0, 0.0);
        // The start on the centre.
        assert_eq!(centre_first.click(0.0, 0.0).outcome, Outcome::Rejected);
        assert!(drawn(&centre_first).is_empty());
    }

    #[test]
    fn the_centred_arc_preview_is_the_arc_to_the_cursors_direction() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.sce");
        driver.click(10.0, 0.0);
        driver.click(0.0, 0.0);
        let feedback = driver.preview(0.0, 5.0);
        assert_eq!(feedback.shapes.len(), 2);
        assert_eq!(
            arc_in(&feedback.shapes[0]),
            arc(0.0, 0.0, 10.0, 0.0, HALF_PI)
        );
        assert_eq!(
            line_in(&feedback.shapes[1]),
            segment([0.0, 0.0], [0.0, 5.0])
        );
    }

    // ---- Arc: start, end, radius ---------------------------------------------------

    #[test]
    fn a_start_end_radius_arc_takes_the_minor_arc_for_a_positive_radius() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.ser");
        driver.click(0.0, 0.0);
        driver.click(8.0, 0.0);
        assert_eq!(
            driver.prompt(),
            "Specify radius of arc (negative for the major arc) or pick its centre or [Undo]"
        );
        assert_eq!(driver.r#type("5").outcome, Outcome::Done);
        // Half the chord is 4, so the centre is sqrt(25 - 16) = 3 off its
        // middle (4, 0) - on the LEFT of start->end for a positive radius:
        // (4, 3). Counter-clockwise from (0,0), at atan2(-3,-4) = pi +
        // atan(3/4), to (8,0), at atan2(-3,4): a sweep of 2 atan(4/3) = 106.26
        // degrees, the minor arc, which dips to (4, 3 - 5) = (4, -2) at its
        // middle.
        let Some(made) = the_arc(&driver) else {
            panic!("one arc");
        };
        assert_eq!(made.center, point(4.0, 3.0));
        assert_eq!(made.radius, 5.0);
        near(made.start_angle, PI + 0.75_f64.atan(), 1e-12);
        near(made.sweep, 2.0 * (4.0_f64 / 3.0).atan(), 1e-12);
        near(made.midpoint().x, 4.0, 1e-12);
        near(made.midpoint().y, -2.0, 1e-12);
        near(made.end_point().x, 8.0, 1e-12);
        near(made.end_point().y, 0.0, 1e-12);
    }

    #[test]
    fn a_start_end_radius_arc_takes_the_major_arc_for_a_negative_radius() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.ser");
        driver.click(0.0, 0.0);
        driver.click(8.0, 0.0);
        assert_eq!(driver.r#type("-5").outcome, Outcome::Done);
        // The centre on the RIGHT, (4, -3). Counter-clockwise from (0,0), at
        // atan2(3,-4), round through south to (8,0): 2 pi - 2 atan(4/3) =
        // 253.74 degrees, whose middle is the bottom of the circle, (4, -3 -
        // 5).
        let Some(made) = the_arc(&driver) else {
            panic!("one arc");
        };
        assert_eq!(made.center, point(4.0, -3.0));
        assert_eq!(made.radius, 5.0);
        near(made.sweep, 2.0 * PI - 2.0 * (4.0_f64 / 3.0).atan(), 1e-12);
        near(made.midpoint().x, 4.0, 1e-12);
        near(made.midpoint().y, -8.0, 1e-12);
    }

    #[test]
    fn a_start_end_radius_arc_of_half_the_chord_is_a_semicircle() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.ser");
        driver.click(0.0, 0.0);
        driver.click(8.0, 0.0);
        driver.r#type("4");
        // Centre at the middle of the chord, from pi counter-clockwise to 0
        // (2 pi): the lower half, since counter-clockwise from the west goes
        // south.
        assert_eq!(the_arc(&driver), Some(arc(4.0, 0.0, 4.0, PI, PI)));
    }

    #[test]
    fn a_picked_centre_for_a_start_end_radius_arc_is_moved_onto_the_ends_bisector() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.ser");
        driver.click(0.0, 0.0);
        driver.click(8.0, 0.0);
        // Every centre of an arc through both ends is on x = 4; (7, 3) goes to
        // (4, 3), the centre a radius of +5 gives.
        assert_eq!(driver.click(7.0, 3.0).outcome, Outcome::Done);
        let Some(made) = the_arc(&driver) else {
            panic!("one arc");
        };
        assert_eq!(made.center, point(4.0, 3.0));
        assert_eq!(made.radius, 5.0);
        near(made.sweep, 2.0 * (4.0_f64 / 3.0).atan(), 1e-12);
    }

    #[test]
    fn a_picked_centre_on_the_right_of_the_chord_gives_the_major_arc_as_a_negative_radius_does() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.ser");
        driver.click(0.0, 0.0);
        driver.click(8.0, 0.0);
        // (1, -3) is on the RIGHT of start->end. Its foot on the bisector x = 4
        // is (4, -3): the centre a radius of -5 gives, so the arc is the major
        // one. The working is that of
        // a_start_end_radius_arc_takes_the_major_arc_for_a_negative_radius:
        // radius |(4,-3)| = 5; counter-clockwise from (0,0), at atan2(3,-4),
        // round through south to (8,0), a sweep of 2 pi - 2 atan(4/3) = 253.74
        // degrees whose middle is the bottom of the circle, (4, -3 - 5) =
        // (4, -8).
        let feedback = driver.preview(1.0, -3.0);
        assert_eq!(feedback.shapes.len(), 1);
        let bent = arc_in(&feedback.shapes[0]);
        assert_eq!(bent.center, point(4.0, -3.0));
        near(bent.sweep, 2.0 * PI - 2.0 * (4.0_f64 / 3.0).atan(), 1e-12);

        assert_eq!(driver.click(1.0, -3.0).outcome, Outcome::Done);
        let Some(made) = the_arc(&driver) else {
            panic!("one arc");
        };
        assert_eq!(made.center, point(4.0, -3.0));
        assert_eq!(made.radius, 5.0);
        near(made.sweep, 2.0 * PI - 2.0 * (4.0_f64 / 3.0).atan(), 1e-12);
        near(made.midpoint().x, 4.0, 1e-12);
        near(made.midpoint().y, -8.0, 1e-12);
    }

    #[test]
    fn a_start_end_radius_arc_refuses_coincident_ends_and_a_too_small_radius() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.ser");
        driver.click(0.0, 0.0);
        assert_eq!(driver.click(0.0, 0.0).outcome, Outcome::Rejected);
        driver.click(8.0, 0.0);
        let small = driver.r#type("3");
        assert_eq!(small.outcome, Outcome::Rejected);
        // Says what the limit is: half the distance between the ends.
        assert!(small.message.contains('4'));
        for bad in ["0", "-3", "abc"] {
            assert_eq!(driver.r#type(bad).outcome, Outcome::Rejected, "{bad}");
        }
        assert!(drawn(&driver).is_empty());
        assert_eq!(driver.r#type("5").outcome, Outcome::Done);
    }

    #[test]
    fn the_start_end_radius_preview_bends_the_arc_about_the_cursors_centre() {
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.ser");
        driver.click(0.0, 0.0);
        driver.click(8.0, 0.0);
        let feedback = driver.preview(7.0, 3.0);
        assert_eq!(feedback.shapes.len(), 1);
        let bent = arc_in(&feedback.shapes[0]);
        assert_eq!(bent.center, point(4.0, 3.0));
        assert_eq!(bent.radius, 5.0);
        assert_eq!(feedback.markers.len(), 3);
        assert_eq!(feedback.markers[2], point(4.0, 3.0));
    }

    #[test]
    fn an_arc_is_drawn_with_the_documents_current_layer_and_the_tool_starts_again() {
        let mut driver = ToolDriver::new();
        kerb_layer(&mut driver);
        driver.start("draw.arc.cse");
        driver.click(0.0, 0.0);
        driver.click(0.0, 10.0);
        driver.click(-3.0, 0.0);
        let entities = drawn(&driver);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].layer, "kerb");
        assert!(!driver.finished());
        assert_eq!(driver.prompt(), "Specify centre point of arc");
    }
}
