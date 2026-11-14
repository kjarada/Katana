//! The twin of tests/qt_widgets/drawing/test_plan_view_tracking.cpp: object snap tracking in the
//! plan view (docs/drawing.md, "Precision input"). With tracking on, an object snap the cursor
//! lands on is acquired, and a point on the horizontal or vertical through it is taken exactly -
//! driven with the view's mouse events. The paths themselves are katana_cad's (track_acquired).
//!
//! The view is 400 x 300 pixels centred on the model origin at 10 pixels a unit:
//! x = (px - 200) / 10, y = -(py - 150) / 10.

use katana_cad::snapping::{SnapMode, SnapModes};
use katana_entity::Geometry;
use katana_geometry::Point2;
use katana_window::viewport_widget::Tool;

use crate::support::Plan;

/// The tracking tests' view: the line (0,0)-(10,10), the snaps on, Endpoint alone.
fn fixture() -> Plan {
    let mut plan = Plan::at_origin();
    let _ = plan.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 10.0));
    plan.document.drafting_mut().snap_enabled = true;
    plan.document.drafting_mut().snap_modes = SnapMode::Endpoint as SnapModes;
    plan.paint();
    plan
}

/// The point the last Point tool click made.
fn last_point(plan: &Plan) -> Point2 {
    let made = plan.document.last_created_entities();
    assert_eq!(made.len(), 1);
    match &plan.entity(made[0]).expect("the point is there").geometry {
        Geometry::Point(point) => point.position,
        other => panic!("not a point: {other:?}"),
    }
}

/// gtest's EXPECT_DOUBLE_EQ: equal to within four units in the last place.
fn double_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= 4.0 * f64::EPSILON * a.abs().max(b.abs())
}

mod plan_view_tracking {
    use super::*;

    #[test]
    fn a_snapped_end_is_acquired_and_its_horizontal_takes_the_point() {
        let mut f = fixture();
        f.document.drafting_mut().object_tracking = true;
        f.start_tool("draw.point").expect("the tool starts");
        // Onto the line's end at (10, 10): acquired.
        f.move_to(300.0, 50.0);
        assert_eq!(f.view.tracking_points().len(), 1);
        assert_eq!(f.view.tracking_points()[0], Point2::new(10.0, 10.0));
        // (3, 10.2) is two pixels off the horizontal through (10, 10).
        f.move_to(230.0, 48.0);
        f.press(230.0, 48.0);
        let made = last_point(&f);
        assert!(double_eq(made.x, 3.0), "{}", made.x);
        assert!(
            double_eq(made.y, 10.0),
            "on the tracking path exactly: {}",
            made.y
        );
        // The acquired cross and the dotted path draw.
        f.paint();
    }

    #[test]
    fn with_tracking_off_nothing_is_acquired_and_the_point_is_where_clicked() {
        let mut f = fixture();
        f.start_tool("draw.point").expect("the tool starts");
        f.move_to(300.0, 50.0);
        assert!(f.view.tracking_points().is_empty());
        f.move_to(230.0, 48.0);
        f.press(230.0, 48.0);
        assert!((last_point(&f).y - 10.2).abs() <= 1e-9);
    }

    #[test]
    fn two_acquired_points_track_to_where_paths_cross() {
        let mut f = fixture();
        f.document.drafting_mut().object_tracking = true;
        f.start_tool("draw.point").expect("the tool starts");
        // (0, 0), then (10, 10).
        f.move_to(200.0, 150.0);
        f.move_to(300.0, 50.0);
        assert_eq!(f.view.tracking_points().len(), 2);
        // Near (10, 0): the vertical through (10, 10) crosses the horizontal through (0, 0).
        f.move_to(301.0, 151.0);
        f.press(301.0, 151.0);
        assert_eq!(last_point(&f), Point2::new(10.0, 0.0));
    }

    #[test]
    fn the_acquired_points_are_forgotten_when_no_tool_wants_a_point() {
        let mut f = fixture();
        f.document.drafting_mut().object_tracking = true;
        f.start_tool("draw.point").expect("the tool starts");
        f.move_to(300.0, 50.0);
        assert!(!f.view.tracking_points().is_empty());
        f.set_tool(Tool::Select);
        f.move_to(250.0, 100.0);
        assert!(f.view.tracking_points().is_empty());
    }
}
