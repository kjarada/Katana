// Precision input (drawing/drafting.rs; tests/cad/drawing/test_drafting.cpp):
// the typed coordinate grammar with DMS angles and survey bearings, ortho,
// polar, the locks, object snap tracking, and how typed text reaches a tool.

use katana_cad::drawing::drafting::{
    constrain, direct_distance, format_bearing, format_dms, format_quadrant_bearing, from_base,
    mid_between, parse_degrees, parse_direction, parse_precise_point, track_acquired,
};
use katana_cad::interactive_tool::{ToolStep, ToolStepOutcome, route_typed_input_with_drafting};
use katana_cad::{AngleConvention, DraftingSettings, SnapMode, TrackingPoints};
use katana_core::ErrorCode;
use katana_entity::Geometry;
use katana_geometry::Point2;
use katana_math::numerics::DEG_TO_RAD;

use crate::tool_driver::ToolDriver;
use crate::{near, near_point, point};

#[track_caller]
fn degrees(text: &str) -> f64 {
    match parse_degrees(text) {
        Ok(value) => value,
        Err(error) => panic!("{text}: {}", error.describe()),
    }
}

// Typed text routed to the running tool with the drafting aids, as the
// command line routes it (the C++ routeTypedInput overload with settings).
fn route(
    driver: &mut ToolDriver,
    settings: &mut DraftingSettings,
    text: &str,
    cursor: Option<Point2>,
) -> ToolStep {
    driver.with_tool(|tool, cx| route_typed_input_with_drafting(tool, cx, text, settings, cursor))
}

#[track_caller]
fn direction(text: &str, convention: AngleConvention) -> f64 {
    match parse_direction(text, convention) {
        Ok(value) => value,
        Err(error) => panic!("{text}: {}", error.describe()),
    }
}

mod drafting {
    use super::*;

    #[test]
    fn degrees_minutes_and_seconds_parse() {
        near(degrees("45.5"), 45.5, 1e-15);
        near(
            degrees("45d30'15\""),
            45.0 + 30.0 / 60.0 + 15.0 / 3600.0,
            1e-12,
        );
        near(
            degrees("45\u{B0}30'15.5\""),
            45.0 + 30.0 / 60.0 + 15.5 / 3600.0,
            1e-12,
        );
        near(degrees("45d30'"), 45.5, 1e-12);
        near(degrees("45d"), 45.0, 1e-12);
        near(degrees("-12d30'"), -12.5, 1e-12);
        // A trailing number takes the next unit.
        near(
            degrees("45D30'15"),
            45.0 + 30.0 / 60.0 + 15.0 / 3600.0,
            1e-12,
        );
        assert!(parse_degrees("45d75'").is_err(), "75 minutes");
        assert!(parse_degrees("north").is_err());
        assert!(parse_degrees("").is_err());
    }

    #[test]
    fn directions_follow_the_convention_and_quadrant_bearings_are_always_bearings() {
        // 30 degrees counter-clockwise from east.
        near(
            direction("30", AngleConvention::Counterclockwise),
            30.0 * DEG_TO_RAD,
            1e-12,
        );
        // As a whole-circle bearing, 30 is 30 clockwise from north: 60 from
        // east.
        near(
            direction("30", AngleConvention::Bearing),
            60.0 * DEG_TO_RAD,
            1e-12,
        );
        // N45E is north-east whatever the convention.
        near(
            direction("N45d00'00\"E", AngleConvention::Counterclockwise),
            45.0 * DEG_TO_RAD,
            1e-12,
        );
        near(
            direction("S30W", AngleConvention::Counterclockwise),
            (90.0 - 210.0) * DEG_TO_RAD,
            1e-12,
        );
        near(
            direction("s 30d w", AngleConvention::Bearing),
            (90.0 - 210.0) * DEG_TO_RAD,
            1e-12,
        );
        near(
            direction("N10W", AngleConvention::Bearing),
            100.0 * DEG_TO_RAD,
            1e-12,
        );
        assert!(
            parse_direction("N95E", AngleConvention::Bearing).is_err(),
            "over 90 in a quadrant"
        );
    }

    #[test]
    fn bearings_format_as_degrees_minutes_and_seconds() {
        assert_eq!(format_dms(45.5, 0), "45\u{B0}30'00\"");
        assert_eq!(
            format_dms(12.0 + 30.0 / 60.0 + 15.25 / 3600.0, 2),
            "12\u{B0}30'15.25\""
        );
        // 59.9999" rounds to the next minute rather than printing 60".
        assert_eq!(
            format_dms(1.0 + 59.0 / 60.0 + 59.9999 / 3600.0, 0),
            "2\u{B0}00'00\""
        );
        // East is a bearing of 90; north-west 315.
        assert_eq!(format_bearing(0.0, 0), "90\u{B0}00'00\"");
        assert_eq!(format_bearing(135.0 * DEG_TO_RAD, 0), "315\u{B0}00'00\"");
        assert_eq!(
            format_quadrant_bearing(135.0 * DEG_TO_RAD, 0),
            "N45\u{B0}00'00\"W"
        );
        assert_eq!(
            format_quadrant_bearing(-60.0 * DEG_TO_RAD, 0),
            "S30\u{B0}00'00\"E"
        );
    }

    #[test]
    fn typed_points_absolute_relative_and_polar() {
        let last = Some(point(10.0, 10.0));
        let defaults = DraftingSettings::default();
        let typed = |text: &str, settings: &DraftingSettings| match parse_precise_point(
            text, last, settings,
        ) {
            Ok(typed) => typed,
            Err(error) => panic!("{text}: {}", error.describe()),
        };
        near_point(typed("3,4", &defaults).point, point(3.0, 4.0), 1e-9);
        let with_height = typed("3,4,101.25", &defaults);
        assert_eq!(with_height.z, Some(101.25));
        near_point(typed("@3,4", &defaults).point, point(13.0, 14.0), 1e-9);
        near_point(typed("@10<90", &defaults).point, point(10.0, 20.0), 1e-9);
        // A DMS angle after the <, counter-clockwise from east by default.
        let half = 0.5_f64.sqrt();
        near_point(
            typed("@10<45d", &defaults).point,
            point(10.0 + 10.0 * half, 10.0 + 10.0 * half),
            1e-9,
        );
        // A quadrant bearing: S45E is south-east.
        near_point(
            typed("@10<S45d00'00\"E", &defaults).point,
            point(10.0 + 10.0 * half, 10.0 - 10.0 * half),
            1e-9,
        );
        // With bearings as the convention, <0 is north.
        let bearings = DraftingSettings {
            angles: AngleConvention::Bearing,
            ..DraftingSettings::default()
        };
        near_point(typed("@25<0", &bearings).point, point(10.0, 35.0), 1e-9);
        near_point(
            typed("@25<90d00'00\"", &bearings).point,
            point(35.0, 10.0),
            1e-9,
        );
        match parse_precise_point("@1,1", None, &defaults) {
            Ok(typed) => panic!("{typed:?} measured from nothing"),
            Err(error) => assert_eq!(error.code, ErrorCode::InvalidState),
        }
        assert!(
            parse_precise_point("10<45", last, &defaults).is_err(),
            "polar is relative"
        );
        assert!(parse_precise_point("1,2,3,4", last, &defaults).is_err());
    }

    #[test]
    fn ortho_polar_and_the_locks() {
        let mut settings = DraftingSettings::default();
        let base = Some(point(0.0, 0.0));
        // Nothing on: the cursor.
        near_point(
            constrain(base, point(3.0, 1.0), &settings, 0.5).point,
            point(3.0, 1.0),
            1e-9,
        );
        settings.ortho = true;
        near_point(
            constrain(base, point(3.0, 1.0), &settings, 0.5).point,
            point(3.0, 0.0),
            1e-9,
        );
        near_point(
            constrain(base, point(1.0, -3.0), &settings, 0.5).point,
            point(0.0, -3.0),
            1e-9,
        );
        settings.ortho = false;
        settings.polar = true;
        settings.polar_increment = 45.0 * DEG_TO_RAD;
        // Near the 45 degree ray: onto it.
        let polar = constrain(base, point(10.0, 10.2), &settings, 0.5);
        near_point(polar.point, point(10.1, 10.1), 1e-9);
        assert!(polar.label.starts_with("Polar 45"), "{}", polar.label);
        // Far from any ray: left alone.
        near_point(
            constrain(base, point(10.0, 4.0), &settings, 0.5).point,
            point(10.0, 4.0),
            1e-9,
        );
        settings.polar = false;
        settings.angle_lock = Some(90.0 * DEG_TO_RAD);
        near_point(
            constrain(base, point(2.0, 7.0), &settings, 0.5).point,
            point(0.0, 7.0),
            1e-9,
        );
        settings.length_lock = Some(5.0);
        near_point(
            constrain(base, point(2.0, 7.0), &settings, 0.5).point,
            point(0.0, 5.0),
            1e-9,
        );
        settings.angle_lock = None;
        near_point(
            constrain(base, point(3.0, 4.0), &settings, 0.5).point,
            point(3.0, 4.0),
            1e-9,
        );
        near_point(
            constrain(base, point(6.0, 8.0), &settings, 0.5).point,
            point(3.0, 4.0),
            1e-9,
        );
    }

    #[test]
    fn object_snap_tracking_finds_paths_and_their_crossings() {
        let acquired = [point(0.0, 0.0), point(10.0, 5.0)];
        // On the horizontal through (0,0), far from it.
        let Some(path) = track_acquired(&acquired, point(20.0, 0.1), 0.5) else {
            panic!("a path");
        };
        near_point(path.point, point(20.0, 0.0), 1e-9);
        // Near where the vertical through (0,0) meets the horizontal through
        // (10,5): the crossing wins.
        let Some(crossing) = track_acquired(&acquired, point(0.2, 5.3), 0.5) else {
            panic!("a crossing");
        };
        near_point(crossing.point, point(0.0, 5.0), 1e-9);
        assert_eq!(crossing.label, "Tracking crossing");
        assert!(track_acquired(&acquired, point(5.0, 2.5), 0.5).is_none());
    }

    #[test]
    fn from_and_mid_between() {
        let defaults = DraftingSettings::default();
        let from = |offset: &str| match from_base(point(10.0, 10.0), offset, &defaults) {
            Ok(at) => at,
            Err(error) => panic!("{offset}: {}", error.describe()),
        };
        near_point(from("@3,4"), point(13.0, 14.0), 1e-9);
        near_point(from("5<90"), point(10.0, 15.0), 1e-9); // the @ is implied
        near_point(
            mid_between(point(0.0, 0.0), point(4.0, 6.0)),
            point(2.0, 3.0),
            1e-9,
        );
        near_point(
            direct_distance(point(0.0, 0.0), point(3.0, 4.0), 10.0, &defaults),
            point(6.0, 8.0),
            1e-9,
        );
    }

    #[test]
    fn tracking_points_keep_the_seven_most_recent_and_move_a_repeat_to_the_front() {
        let mut tracking = TrackingPoints::default();
        for i in 0..9 {
            tracking.acquire(point(f64::from(i), 0.0), 1e-9);
        }
        assert_eq!(tracking.points().len(), TrackingPoints::MOST);
        assert_eq!(tracking.points().first(), Some(&point(8.0, 0.0)));
        assert_eq!(tracking.points().last(), Some(&point(2.0, 0.0)));
        tracking.acquire(point(5.0, 0.0), 1e-9);
        assert_eq!(tracking.points().first(), Some(&point(5.0, 0.0)));
        assert_eq!(
            tracking.points().len(),
            TrackingPoints::MOST,
            "moved, not added again"
        );
        assert!(TrackingPoints::tracks(SnapMode::Endpoint));
        assert!(TrackingPoints::tracks(SnapMode::Quadrant));
        assert!(!TrackingPoints::tracks(SnapMode::Nearest));
        assert!(!TrackingPoints::tracks(SnapMode::Grid));
        tracking.clear();
        assert!(tracking.points().is_empty());
    }
}

mod drafting_typed_input {
    use super::*;

    #[test]
    fn a_distance_typed_at_a_line_prompt_goes_along_the_cursor() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        let mut settings = DraftingSettings::default();
        // Direct distance entry: 5 towards a cursor at (30, 40).
        let step = route(&mut driver, &mut settings, "5", Some(point(30.0, 40.0)));
        assert_eq!(step.outcome, ToolStepOutcome::Continue);
        let Some(last) = driver.last_point() else {
            panic!("a last point");
        };
        near_point(last, point(3.0, 4.0), 1e-9);
    }

    #[test]
    fn angles_and_lengths_lock_and_clear() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        let mut settings = DraftingSettings::default();
        assert_eq!(
            route(&mut driver, &mut settings, "<N45E", None).outcome,
            ToolStepOutcome::Continue
        );
        let Some(lock) = settings.angle_lock else {
            panic!("an angle lock");
        };
        near(lock, 45.0 * DEG_TO_RAD, 1e-12);
        // A distance now goes along the lock whatever the cursor does.
        route(&mut driver, &mut settings, "10", Some(point(-5.0, 0.0)));
        let half = 0.5_f64.sqrt();
        let Some(last) = driver.last_point() else {
            panic!("a last point");
        };
        near_point(last, point(10.0 * half, 10.0 * half), 1e-9);
        route(&mut driver, &mut settings, "=7", None);
        assert_eq!(settings.length_lock, Some(7.0));
        route(&mut driver, &mut settings, "<", None);
        route(&mut driver, &mut settings, "=", None);
        assert!(settings.angle_lock.is_none());
        assert!(settings.length_lock.is_none());
        assert_eq!(
            route(&mut driver, &mut settings, "=abc", None).outcome,
            ToolStepOutcome::Rejected
        );
    }

    #[test]
    fn a_radius_is_still_the_tools_value() {
        // A number the tool takes is its value, not a distance along the
        // cursor.
        let mut driver = ToolDriver::new();
        driver.start("draw.circle");
        driver.click(5.0, 5.0);
        let mut settings = DraftingSettings::default();
        let step = route(&mut driver, &mut settings, "2.5", Some(point(100.0, 5.0)));
        assert_eq!(step.outcome, ToolStepOutcome::Done);
        let Some(command) = step.command else {
            panic!("a command");
        };
        crate::ok(driver.document_mut().execute(command));
        let entities = crate::entities_of(driver.document());
        assert_eq!(entities.len(), 1);
        let mut found = false;
        for entity in &entities {
            if let Geometry::Circle(made) = entity.geometry {
                near(made.radius, 2.5, 1e-12);
                found = true;
            }
        }
        assert!(found);
    }
}
