// The drawing system's draw tools (tools/draw_professional.rs, and the
// Polyline tool's arc segments and the Point tool's heights in
// tools/draw_lines.rs; tests/cad/tools/test_draw_professional.cpp), each driven
// as a user would against geometry worked out by hand.

use katana_cad::drawing::construction::{CONSTRUCTION_REACH, is_construction_layer};
use katana_cad::selection::drawn_extent;
use katana_cad::tools::families::sketch_move_for_tests;
use katana_cad::{Document, NO_LAYER_OVERRIDES};
use katana_commands::{EntityAttributes, create_entities, create_line};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{Entity, Geometry, PointGeometry};
use katana_geometry::curves2d::CurveSegment;
use katana_geometry::{Arc2, Circle2, CurvePolyline2, Ellipse2, Polyline2, Segment2, Spline2};
use katana_math::numerics::HALF_PI;

use crate::tool_driver::ToolDriver;
use crate::{entities_of, near, near_point, point};

// The geometry of the document's only entity of one kind, picked out by
// `pick`; fails the test when there is not exactly one (the C++ only<T>).
#[track_caller]
fn only<T: Clone>(document: &Document, pick: impl Fn(&Geometry) -> Option<&T>) -> T {
    let entities = entities_of(document);
    let found: Vec<&T> = entities
        .iter()
        .filter_map(|entity| pick(&entity.geometry))
        .collect();
    assert_eq!(found.len(), 1);
    found[0].clone()
}

fn curve_polyline(geometry: &Geometry) -> Option<&CurvePolyline2> {
    match geometry {
        Geometry::CurvePolyline(shape) => Some(shape),
        _ => None,
    }
}

fn straight_polyline(geometry: &Geometry) -> Option<&Polyline2> {
    match geometry {
        Geometry::Polyline(shape) => Some(shape),
        _ => None,
    }
}

fn spline(geometry: &Geometry) -> Option<&Spline2> {
    match geometry {
        Geometry::Spline(shape) => Some(shape),
        _ => None,
    }
}

fn ellipse(geometry: &Geometry) -> Option<&Ellipse2> {
    match geometry {
        Geometry::Ellipse(shape) => Some(shape),
        _ => None,
    }
}

fn line(geometry: &Geometry) -> Option<&Segment2> {
    match geometry {
        Geometry::Line(shape) => Some(shape),
        _ => None,
    }
}

fn circle(geometry: &Geometry) -> Option<&Circle2> {
    match geometry {
        Geometry::Circle(shape) => Some(shape),
        _ => None,
    }
}

fn arc(geometry: &Geometry) -> Option<&Arc2> {
    match geometry {
        Geometry::Arc(shape) => Some(shape),
        _ => None,
    }
}

#[track_caller]
fn arc_of(segment: CurveSegment) -> Arc2 {
    match segment {
        CurveSegment::Arc(arc) => arc,
        CurveSegment::Segment(straight) => panic!("{straight:?} is not an arc"),
    }
}

// ---- Polyline with arc segments ---------------------------------------------------------

mod draw_polyline_arcs {
    use super::*;

    #[test]
    fn an_arc_segment_is_tangent_to_the_segment_before() {
        // East 10, then an arc to (15, 5): tangent to east at (10, 0), a
        // quarter circle of radius 5 about (10, 5).
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.click(10.0, 0.0);
        driver.r#type("A");
        assert_eq!(
            driver.prompt(),
            "Specify end point of arc or [Line/Second/Undo]"
        );
        driver.click(15.0, 5.0);
        driver.r#type("L");
        driver.click(15.0, 15.0);
        driver.enter();
        let shape = only(driver.document(), curve_polyline);
        assert_eq!(shape.vertices.len(), 4);
        let bend = arc_of(shape.segment(1));
        near_point(bend.center, point(10.0, 5.0), 1e-9);
        near(bend.radius, 5.0, 1e-9);
        near(bend.sweep, HALF_PI, 1e-9);
        assert!(!shape.is_arc(2), "back in Line mode");
        assert_eq!(driver.document().history().undo_name(), "CREATE_POLYLINE");
    }

    #[test]
    fn the_first_arc_heads_east_and_second_passes_through_a_point() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.r#type("Arc");
        // From (0,0) heading east to (4,4): a quarter turn left about (0,4).
        driver.click(4.0, 4.0);
        driver.r#type("S");
        driver.click(6.0, 6.0); // through
        driver.click(8.0, 4.0); // to
        driver.enter();
        let shape = only(driver.document(), curve_polyline);
        let first = arc_of(shape.segment(0));
        near_point(first.center, point(0.0, 4.0), 1e-9);
        let second = arc_of(shape.segment(1));
        // Through (6,6).
        near(
            second.center.distance_to(point(6.0, 6.0)),
            second.radius,
            1e-9,
        );
    }

    #[test]
    fn straight_only_is_still_a_polyline2_and_undo_takes_back_an_arc() {
        let mut driver = ToolDriver::new();
        driver.start("draw.polyline");
        driver.click(0.0, 0.0);
        driver.r#type("A");
        driver.click(4.0, 4.0);
        driver.undo();
        driver.r#type("L");
        driver.click(5.0, 0.0);
        driver.enter();
        // No arc left: the Polyline2 it always was.
        only(driver.document(), straight_polyline);
    }
}

// ---- Point with height -------------------------------------------------------------------

mod draw_point_heights {
    use super::*;

    #[test]
    fn a_typed_height_or_the_current_one_goes_in_the_elevation() {
        let mut driver = ToolDriver::new();
        driver.start("draw.point");
        driver.r#type("1,2,31.5");
        driver.r#type("H");
        driver.r#type("40");
        driver.click(5.0, 5.0);
        driver.click(6.0, 6.0); // after a restart, still at 40
        let entities = entities_of(driver.document());
        assert_eq!(entities.len(), 3);
        assert_eq!(heights_of(&entities[0].properties, 1)[0], Some(31.5));
        assert_eq!(heights_of(&entities[1].properties, 1)[0], Some(40.0));
        assert_eq!(heights_of(&entities[2].properties, 1)[0], Some(40.0));
        driver.r#type("H");
        driver.r#type("None"); // back to plan points for the next test
    }
}

// ---- 3D Polyline --------------------------------------------------------------------------

mod draw_polyline3d {
    use super::*;

    #[test]
    fn heights_come_from_typed_z_snapped_points_and_the_current_height() {
        let mut driver = ToolDriver::new();
        let mut mark = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: point(10.0, 0.0),
            }),
            ..Entity::default()
        };
        set_heights(&mut mark.properties, &[Some(12.5)]);
        driver.add(create_entities(vec![mark]));

        driver.start("draw.polyline3d");
        driver.r#type("0,0,10");
        driver.click(10.0, 0.0); // snapped onto the surveyed point: its height
        driver.click(10.0, 10.0); // nothing there and no current height: none
        driver.enter();
        // Straight: a Polyline2 with its heights in the properties.
        only(driver.document(), straight_polyline);
        let entities = entities_of(driver.document());
        let Some(string) = entities
            .iter()
            .find(|entity| matches!(entity.geometry, Geometry::Polyline(_)))
        else {
            panic!("a polyline");
        };
        let heights = heights_of(&string.properties, 3);
        assert_eq!(heights[0], Some(10.0));
        assert_eq!(heights[1], Some(12.5));
        assert!(heights[2].is_none(), "no height is not zero");
    }
}

// ---- Spline -------------------------------------------------------------------------------

mod draw_spline {
    use super::*;

    #[test]
    fn through_the_clicked_points_or_on_them_as_control_points() {
        let mut driver = ToolDriver::new();
        driver.start("draw.spline");
        driver.click(0.0, 0.0);
        driver.click(3.0, 4.0);
        driver.click(7.0, 3.0);
        driver.enter();
        let fit = only(driver.document(), spline);
        assert_eq!(fit.fit_points.len(), 3);
        assert!(fit.distance_to(point(3.0, 4.0)) < 1e-3);

        let mut second = ToolDriver::new();
        second.start("draw.spline");
        second.r#type("C");
        second.r#type("D");
        second.r#type("2");
        second.click(0.0, 0.0);
        second.click(5.0, 5.0);
        second.click(10.0, 0.0);
        second.enter();
        let control = only(second.document(), spline);
        assert!(control.fit_points.is_empty());
        assert_eq!(control.degree, 2);
        near_point(control.point_at(0.5), point(5.0, 2.5), 1e-9);
    }
}

// ---- Ellipse ------------------------------------------------------------------------------

mod draw_ellipse {
    use super::*;

    #[test]
    fn axis_and_end_centre_and_arc() {
        let mut driver = ToolDriver::new();
        driver.start("draw.ellipse");
        driver.click(-10.0, 0.0);
        driver.click(10.0, 0.0);
        driver.click(0.0, 4.0); // 4 from the axis
        let whole = only(driver.document(), ellipse);
        near_point(whole.center, point(0.0, 0.0), 1e-9);
        near(whole.major_radius(), 10.0, 1e-12);
        near(whole.minor_radius(), 4.0, 1e-12);

        let mut centre = ToolDriver::new();
        centre.start("draw.ellipse.centre");
        centre.click(5.0, 5.0);
        centre.click(5.0, 8.0); // a 3 m half axis, north
        centre.r#type("6"); // the other: 6, so it becomes the major
        let tall = only(centre.document(), ellipse);
        near(tall.major_radius(), 6.0, 1e-12);
        near(tall.minor_radius(), 3.0, 1e-12);

        let mut part = ToolDriver::new();
        part.start("draw.ellipse.arc");
        part.click(-10.0, 0.0);
        part.click(10.0, 0.0);
        part.r#type("5");
        part.r#type("0");
        part.r#type("90");
        let quarter = only(part.document(), ellipse);
        assert!(!quarter.is_full());
        near(quarter.sweep, HALF_PI, 1e-12);
        near_point(quarter.end_point(), point(0.0, 5.0), 1e-9);
    }
}

// ---- Construction Line and Ray ---------------------------------------------------------------

mod draw_construction {
    use super::*;

    #[test]
    fn lines_and_rays_go_on_the_construction_layer_in_one_step() {
        let mut driver = ToolDriver::new();
        crate::ok(driver.document_mut().execute(create_line(
            point(0.0, 0.0),
            point(10.0, 10.0),
            EntityAttributes::default(),
        )));
        driver.start("draw.xline");
        driver.click(0.0, 0.0);
        driver.click(1.0, 0.0);
        driver.click(0.0, 1.0);
        let before = driver.document().history().undo_count();
        driver.enter();
        assert_eq!(driver.document().history().undo_count(), before + 1);
        let mut construction = 0;
        for entity in entities_of(driver.document()) {
            if is_construction_layer(&entity.layer) {
                construction += 1;
                let Geometry::Line(made) = entity.geometry else {
                    panic!("{:?} is not a line", entity.geometry);
                };
                near(made.length(), 2.0 * CONSTRUCTION_REACH, 1e-3);
            }
        }
        assert_eq!(construction, 2);
        assert!(
            driver
                .document()
                .model()
                .layers
                .find("construction")
                .is_some()
        );
        // Zoom Extents frames the drawing, not the construction lines.
        let extent = drawn_extent(driver.document().model(), &NO_LAYER_OVERRIDES);
        near(extent.max.x, 10.0, 1e-9);

        let mut ray = ToolDriver::new();
        ray.start("draw.ray");
        ray.click(0.0, 0.0);
        ray.click(0.0, 5.0);
        ray.enter();
        let made = only(ray.document(), line);
        assert_eq!(made.start, point(0.0, 0.0));
        near(made.end.y, CONSTRUCTION_REACH, 1e-6);
    }
}

// ---- Double Line, Sketch, Revision Cloud ---------------------------------------------------------

mod draw_double_line {
    use super::*;

    #[test]
    fn both_sides_at_half_the_width() {
        let mut driver = ToolDriver::new();
        driver.start("draw.dline");
        driver.r#type("W");
        driver.r#type("2");
        driver.click(0.0, 0.0);
        driver.click(10.0, 0.0);
        driver.enter();
        let entities = entities_of(driver.document());
        assert_eq!(entities.len(), 2);
        let mut ys: Vec<f64> = entities
            .iter()
            .map(|entity| match &entity.geometry {
                Geometry::Polyline(side) => side.vertices[0].y,
                other => panic!("{other:?} is not a polyline"),
            })
            .collect();
        ys.sort_by(f64::total_cmp);
        near(ys[0], -1.0, 1e-12);
        near(ys[1], 1.0, 1e-12);
    }
}

mod draw_sketch {
    use super::*;

    #[test]
    fn a_stroke_is_recorded_between_clicks_and_weeded() {
        let mut driver = ToolDriver::new();
        driver.start("draw.sketch");
        driver.r#type("T");
        driver.r#type("0.01");
        driver.click(0.0, 0.0);
        for i in 1..10 {
            sketch_move_for_tests(
                driver.tool_mut(),
                point(f64::from(i), 0.001 * f64::from(i % 2)),
            );
        }
        driver.click(10.0, 0.0);
        driver.enter();
        let stroke = only(driver.document(), straight_polyline);
        assert_eq!(
            stroke.vertices.len(),
            2,
            "the wobble is within the tolerance"
        );
    }
}

mod draw_revision_cloud {
    use super::*;

    #[test]
    fn a_rectangle_cloud_bulges_outward() {
        let mut driver = ToolDriver::new();
        driver.start("draw.revcloud");
        driver.r#type("A");
        driver.r#type("2");
        driver.r#type("R");
        driver.click(0.0, 0.0);
        driver.click(10.0, 4.0);
        let cloud = only(driver.document(), curve_polyline);
        assert!(cloud.closed);
        assert_eq!(cloud.vertices.len(), 14); // 5 + 2 + 5 + 2 arcs of 2 m
        assert!(cloud.area() > 40.0, "outward, so more than the rectangle");
        assert!(cloud.bounding_box().min.y < 0.0);
    }
}

// ---- the constructions the Circle and Arc families lacked -------------------------------------

mod draw_circle_tangents {
    use super::*;

    #[test]
    fn the_circle_touching_three_lines_nearest_the_picks() {
        // The 3-4-5 triangle (0,0) (4,0) (0,3): its incircle has radius
        // (3 + 4 - 5) / 2 = 1 about (1, 1).
        let mut driver = ToolDriver::new();
        let a = driver.add(create_line(
            point(0.0, 0.0),
            point(4.0, 0.0),
            EntityAttributes::default(),
        ));
        let b = driver.add(create_line(
            point(0.0, 0.0),
            point(0.0, 3.0),
            EntityAttributes::default(),
        ));
        let c = driver.add(create_line(
            point(4.0, 0.0),
            point(0.0, 3.0),
            EntityAttributes::default(),
        ));
        driver.start("draw.circle.ttt");
        driver.pick(a, 1.0, 0.0);
        driver.pick(b, 0.0, 1.0);
        driver.pick(c, 1.6, 1.8);
        let touching = only(driver.document(), circle);
        near(touching.radius, 1.0, 1e-9);
        near_point(touching.center, point(1.0, 1.0), 1e-9);
    }
}

mod draw_arc_direction {
    use super::*;

    #[test]
    fn an_arc_leaving_its_start_in_the_given_direction() {
        // From (0,0) to (4,4) leaving east: the quarter circle about (0,4).
        let mut driver = ToolDriver::new();
        driver.start("draw.arc.sed");
        driver.click(0.0, 0.0);
        driver.click(4.0, 4.0);
        driver.r#type("0");
        let made = only(driver.document(), arc);
        near_point(made.center, point(0.0, 4.0), 1e-9);
        near(made.radius, 4.0, 1e-9);
    }
}
