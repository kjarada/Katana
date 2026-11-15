// The Inquiry tools (src/katana_cad/tools/inquiry.rs;
// tests/cad/tools/test_inquiry.cpp): Distance, Area, ID Point, Angle and
// List, driven as the plan view drives them. They change nothing, so each
// test checks the report - the command log's line - and that no command ran.
// The numbers are worked out by hand in the comments.
//
// The 3-4-5 triangle carries most of them, because its angles are known to
// the second: atan(3/4) = 36.869897645844 degrees; 0.869897645844 * 60 =
// 52.19385875' and 0.19385875 * 60 = 11.63" - so a 3 east, 4 north leg runs
// at 36°52'11.63", and its complement is 90° - 36°52'11.63" = 53°07'48.37".

use katana_cad::SurveyImportOptions;
use katana_cad::interactive_tool::ToolInput;
use katana_commands::{
    EntityAttributes, create_arc, create_circle, create_entities, create_line, create_polyline,
};
use katana_entity::entity::set_heights;
use katana_entity::{Entity, EntityId, Geometry, PointGeometry, PropertyValue};
use katana_geometry::{Arc2, Point2, Polyline2};
use katana_math::numerics::PI;

use crate::tool_driver::ToolDriver;
use crate::{CONTINUE, DONE, REJECTED, contains};

// The degree sign as the survey formatter writes it (UTF-8).
const DEG: &str = "\u{b0}";
// Square metres, likewise.
const SQUARE: &str = " m\u{b2}";

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

// A survey point as the survey import writes one: its number and its height.
fn add_survey_point(
    driver: &mut ToolDriver,
    at: Point2,
    name: &str,
    height: Option<f64>,
    code: &str,
) -> EntityId {
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    let options = SurveyImportOptions::default();
    point.properties.insert(
        options.point_number_property.clone(),
        PropertyValue::Text(String::from(name)),
    );
    if !code.is_empty() {
        point.properties.insert(
            options.code_property.clone(),
            PropertyValue::Text(String::from(code)),
        );
    }
    if height.is_some() {
        set_heights(&mut point.properties, &[height]);
    }
    driver.add(create_entities(vec![point]))
}

// ---- Distance ----------------------------------------------------------------------

mod inquiry_distance {
    use super::*;

    #[test]
    fn two_picks_report_the_inverse_of_a345_leg_in_survey_terms() {
        let mut driver = ToolDriver::new();
        driver.start("inquiry.distance");
        assert_eq!(driver.expects(), ToolInput::Point);
        assert_eq!(driver.click(1000.0, 2000.0).outcome, CONTINUE);
        let step = driver.click(1003.0, 2004.0);
        assert_eq!(step.outcome, DONE);
        assert!(step.command.is_none());
        let report = &step.message;
        // dE = 3, dN = 4, the distance sqrt(9 + 16) = 5.
        assert!(contains(report, "dE +3.000   dN +4.000"), "{report}");
        assert!(contains(report, "Horizontal distance 5.000"), "{report}");
        assert!(
            contains(report, &format!("Azimuth 36{DEG}52'11.63\"")),
            "{report}"
        );
        // Back: 36°52'11.63" + 180°.
        assert!(
            contains(report, &format!("back azimuth 216{DEG}52'11.63\"")),
            "{report}"
        );
        assert!(
            contains(report, &format!("Bearing N 36{DEG}52'11.63\" E")),
            "{report}"
        );
        // Two bare picks have no heights, and the report says so rather than
        // reporting a height difference of zero.
        assert!(contains(report, "No height difference"), "{report}");
        assert_eq!(driver.executed(), 0);
        // It starts again for the next measurement.
        assert!(!driver.finished());
    }

    #[test]
    fn picks_on_survey_points_report_their_numbers_and_height_difference() {
        let mut driver = ToolDriver::new();
        let a = add_survey_point(&mut driver, p(1000.0, 2000.0), "CP1", Some(10.0), "");
        let b = add_survey_point(&mut driver, p(1003.0, 2004.0), "CP2", Some(12.0), "");
        driver.start("inquiry.distance");
        let _ = driver.click(1000.0, 2000.0);
        // Typed as a report prints it, to three decimals: 0.0004 off the
        // point, inside the millimetre that makes it the point - its number
        // and height, not the bare coordinates.
        let step = driver.click(1003.0004, 2004.0);
        assert_eq!(step.outcome, DONE);
        let report = &step.message;
        assert!(contains(report, &format!("point {a} (CP1)")), "{report}");
        assert!(contains(report, &format!("point {b} (CP2)")), "{report}");
        // dZ = 12 - 10 = +2; slope sqrt(5^2 + 2^2) = sqrt(29) = 5.38516...;
        // grade 2 / 5 = 40 %.
        assert!(contains(report, "Height difference +2.000"), "{report}");
        assert!(contains(report, "slope distance 5.385"), "{report}");
        assert!(contains(report, "grade +40.000 %"), "{report}");
    }

    #[test]
    fn a_pick_near_a_survey_point_but_not_on_it_is_only_its_coordinates() {
        // 0.004 from the point: more than a millimetre, so a click that missed
        // the mark is not reported as the mark (its number, its height).
        let mut driver = ToolDriver::new();
        let _ = add_survey_point(&mut driver, p(1000.0, 2000.0), "CP1", Some(10.0), "");
        driver.start("inquiry.distance");
        let _ = driver.click(1000.004, 2000.0);
        let step = driver.click(1003.004, 2004.0);
        assert_eq!(step.outcome, DONE);
        assert!(!contains(&step.message, "CP1"), "{}", step.message);
        assert!(
            contains(&step.message, "Inverse  E 1000.004 N 2000.000"),
            "{}",
            step.message
        );
        assert!(
            contains(&step.message, "No height difference"),
            "{}",
            step.message
        );
    }

    #[test]
    fn the_same_point_twice_is_refused_and_undo_takes_back_the_first() {
        let mut driver = ToolDriver::new();
        driver.start("inquiry.distance");
        let _ = driver.click(5.0, 5.0);
        assert_eq!(driver.click(5.0, 5.0).outcome, REJECTED);
        assert_eq!(driver.undo().outcome, CONTINUE);
        assert_eq!(driver.prompt(), "Specify first point");
        assert_eq!(driver.undo().outcome, REJECTED);
    }
}

// ---- Area --------------------------------------------------------------------------

mod inquiry_area {
    use super::*;

    #[test]
    fn four_picked_corners_report_area_perimeter_and_hectares() {
        let mut driver = ToolDriver::new();
        driver.start("inquiry.area");
        let _ = driver.click(0.0, 0.0);
        let _ = driver.click(4.0, 0.0);
        assert_eq!(driver.enter().outcome, REJECTED); // two corners bound nothing
        let _ = driver.click(4.0, 3.0);
        let _ = driver.click(0.0, 3.0);
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        assert!(step.command.is_none());
        // 4 x 3 = 12 m^2 = 0.0012 ha; perimeter 4 + 3 + 4 + 3 = 14.
        assert!(
            contains(&step.message, &format!("area 12.000{SQUARE} (0.0012 ha)")),
            "{}",
            step.message
        );
        assert!(
            contains(&step.message, "perimeter 14.000 m"),
            "{}",
            step.message
        );
        assert!(
            contains(&step.message, "4 picked corners"),
            "{}",
            step.message
        );
    }

    #[test]
    fn object_measures_a_closed_circle_and_refuses_an_open_polyline() {
        let mut driver = ToolDriver::new();
        let circle = driver.add(create_circle(
            p(10.0, 10.0),
            2.0,
            EntityAttributes::default(),
        ));
        let open = driver.add(create_polyline(
            Polyline2::new(vec![p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0)], false),
            EntityAttributes::default(),
        ));
        driver.start("inquiry.area");
        assert_eq!(driver.r#type("O").outcome, CONTINUE);
        assert_eq!(driver.expects(), ToolInput::Entity);
        assert_eq!(driver.pick(open, 5.0, 2.0).outcome, REJECTED);
        let step = driver.pick(circle, 12.0, 10.0);
        assert_eq!(step.outcome, DONE);
        // pi * 2^2 = 12.566370... m^2 = 0.0012566 ha -> 0.0013; circumference
        // 2 * pi * 2 = 12.566370...
        assert!(
            contains(&step.message, &format!("area 12.566{SQUARE} (0.0013 ha)")),
            "{}",
            step.message
        );
        assert!(
            contains(&step.message, "perimeter 12.566 m"),
            "{}",
            step.message
        );
    }

    #[test]
    fn enter_at_the_first_corner_measures_the_selection_once() {
        let mut driver = ToolDriver::new();
        let lot = driver.add(create_polyline(
            Polyline2::new(
                vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0), p(0.0, 5.0)],
                true,
            ),
            EntityAttributes::default(),
        ));
        driver.document_mut().selection_mut().set(vec![lot]);
        driver.start("inquiry.area");
        assert!(contains(&driver.prompt(), "<Selection>"));
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        // 10 x 5 = 50 m^2 = 0.0050 ha; perimeter 30.
        assert!(
            contains(&step.message, &format!("area 50.000{SQUARE} (0.0050 ha)")),
            "{}",
            step.message
        );
        assert!(
            contains(&step.message, "perimeter 30.000 m"),
            "{}",
            step.message
        );
        assert!(driver.finished()); // not measured again by a restart
    }
}

// ---- ID Point ----------------------------------------------------------------------

mod inquiry_id {
    use super::*;

    #[test]
    fn a_bare_pick_reports_easting_and_northing() {
        let mut driver = ToolDriver::new();
        driver.start("inquiry.id");
        let step = driver.click(3.0, 4.0);
        assert_eq!(step.outcome, DONE);
        assert_eq!(step.message, "E 3.000 N 4.000");
        assert!(!driver.finished());
    }

    #[test]
    fn a_pick_on_a_survey_point_reports_its_number_height_and_code() {
        let mut driver = ToolDriver::new();
        let id = add_survey_point(&mut driver, p(100.0, 200.0), "CP1", Some(10.5), "TB");
        driver.start("inquiry.id");
        let step = driver.click(100.0005, 200.0); // within the millimetre
        assert_eq!(step.outcome, DONE);
        assert_eq!(
            step.message,
            format!("point {id} (CP1)  E 100.000 N 200.000  Z 10.500  code TB")
        );
        // Ten centimetres off it is somewhere else.
        assert_eq!(driver.click(100.1, 200.0).message, "E 100.100 N 200.000");
    }
}

// ---- Angle -------------------------------------------------------------------------

mod inquiry_angle {
    use super::*;

    #[test]
    fn the_angle_from_east_to345_is_reported_both_ways_with_bearings() {
        let mut driver = ToolDriver::new();
        driver.start("inquiry.angle");
        let _ = driver.click(0.0, 0.0);
        assert_eq!(driver.click(0.0, 0.0).outcome, REJECTED); // an arm needs length
        let _ = driver.click(10.0, 0.0); // due east: azimuth 90°
        let step = driver.click(3.0, 4.0); // azimuth 36°52'11.63"
        assert_eq!(step.outcome, DONE);
        let report = &step.message;
        // Turned clockwise from 90° to 36°52'11.63": 36°52'11.63" - 90° +
        // 360° = 306°52'11.63"; the other way 53°07'48.37", the included
        // angle.
        assert!(
            contains(report, &format!("Included angle 53{DEG}07'48.37\"")),
            "{report}"
        );
        assert!(
            contains(report, &format!("to the second 306{DEG}52'11.63\"")),
            "{report}"
        );
        assert!(
            contains(report, &format!("the other way 53{DEG}07'48.37\"")),
            "{report}"
        );
        assert!(
            contains(report, &format!("First arm  azimuth 90{DEG}00'00.00\"")),
            "{report}"
        );
        assert!(
            contains(report, &format!("bearing N 36{DEG}52'11.63\" E")),
            "{report}"
        );
    }

    #[test]
    fn north_then_east_is_a_right_angle_turned_clockwise() {
        let mut driver = ToolDriver::new();
        driver.start("inquiry.angle");
        let _ = driver.click(5.0, 5.0);
        let _ = driver.click(5.0, 15.0); // north: azimuth 0
        let step = driver.click(15.0, 5.0); // east: azimuth 90°
        assert_eq!(step.outcome, DONE);
        assert!(
            contains(&step.message, &format!("Included angle 90{DEG}00'00.00\"")),
            "{}",
            step.message
        );
        assert!(
            contains(&step.message, &format!("to the second 90{DEG}00'00.00\"")),
            "{}",
            step.message
        );
        assert!(
            contains(&step.message, &format!("the other way 270{DEG}00'00.00\"")),
            "{}",
            step.message
        );
    }
}

// ---- List --------------------------------------------------------------------------

mod inquiry_list {
    use super::*;

    #[test]
    fn a_line_is_listed_with_its_ends_length_and_bearing() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            p(0.0, 0.0),
            p(3.0, 4.0),
            EntityAttributes::default(),
        ));
        driver.document_mut().selection_mut().set(vec![line]);
        driver.start("inquiry.list");
        assert_eq!(driver.expects(), ToolInput::Selection);
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        let report = &step.message;
        assert!(contains(report, "1 entity listed"), "{report}");
        assert!(
            contains(
                report,
                &format!("{line}  Line  layer 0  style ByLayer  colour ByLayer")
            ),
            "{report}"
        );
        assert!(
            contains(report, "from E 0.000 N 0.000  to E 3.000 N 4.000"),
            "{report}"
        );
        assert!(
            contains(
                report,
                &format!("length 5.000 m   bearing N 36{DEG}52'11.63\" E")
            ),
            "{report}"
        );
        assert!(driver.finished());
    }

    #[test]
    fn a_polyline_arc_and_picked_point_are_listed_in_survey_terms() {
        let mut driver = ToolDriver::new();
        let lot = driver.add(create_polyline(
            Polyline2::new(
                vec![p(0.0, 0.0), p(4.0, 0.0), p(4.0, 3.0), p(0.0, 3.0)],
                true,
            ),
            EntityAttributes::default(),
        ));
        let arc = driver.add(create_arc(
            Arc2::new(p(0.0, 0.0), 10.0, 0.0, PI / 2.0),
            EntityAttributes::default(),
        ));
        let point = add_survey_point(&mut driver, p(7.0, 8.0), "P9", Some(1.25), "FC");
        driver.start("inquiry.list");
        assert_eq!(driver.pick(lot, 0.0, 0.0).outcome, CONTINUE);
        assert_eq!(driver.pick(lot, 0.0, 0.0).outcome, REJECTED); // picked already
        let _ = driver.pick(arc, 10.0, 0.0);
        let _ = driver.pick(point, 7.0, 8.0);
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        let report = &step.message;
        assert!(contains(report, "3 entities listed"), "{report}");
        // The rectangle: 4 + 3 + 4 + 3 = 14 round, 4 x 3 = 12 inside.
        assert!(
            contains(
                report,
                &format!("closed, 4 vertices, length 14.000 m   area 12.000{SQUARE} (0.0012 ha)")
            ),
            "{report}"
        );
        assert!(contains(report, "3  E 4.000 N 3.000"), "{report}");
        // The quarter arc of radius 10: 10 * pi / 2 = 15.70796 long, its
        // chord 10 * sqrt(2) = 14.14214, from (10, 0) round to (0, 10).
        assert!(
            contains(
                report,
                "from E 10.000 N 0.000  to E 0.000 N 10.000  anticlockwise"
            ),
            "{report}"
        );
        assert!(
            contains(
                report,
                &format!("length 15.708 m   delta 90{DEG}00'00.00\"   chord 14.142 m")
            ),
            "{report}"
        );
        // The point, with its height beside its coordinates and its
        // properties.
        assert!(contains(report, "E 7.000 N 8.000  Z 1.250"), "{report}");
        assert!(contains(report, "code = FC"), "{report}");
        assert!(contains(report, "point = P9"), "{report}");
    }

    #[test]
    fn enter_with_nothing_selected_is_refused_with_what_to_do() {
        let mut driver = ToolDriver::new();
        driver.start("inquiry.list");
        let step = driver.enter();
        assert_eq!(step.outcome, REJECTED);
        assert!(
            contains(&step.message, "nothing is selected"),
            "{}",
            step.message
        );
    }
}

mod inquiry_tools {
    use super::*;

    #[test]
    fn none_of_them_changes_the_drawing() {
        let mut driver = ToolDriver::new();
        let _ = driver.add(create_line(
            p(0.0, 0.0),
            p(3.0, 4.0),
            EntityAttributes::default(),
        ));
        let revision = driver.document().model_revision();
        driver.start("inquiry.distance");
        let _ = driver.click(0.0, 0.0);
        let _ = driver.click(3.0, 4.0);
        driver.start("inquiry.id");
        let _ = driver.click(0.0, 0.0);
        driver.start("inquiry.angle");
        let _ = driver.click(0.0, 0.0);
        let _ = driver.click(1.0, 0.0);
        let _ = driver.click(0.0, 1.0);
        assert_eq!(driver.executed(), 0);
        assert_eq!(driver.document().model_revision(), revision);
    }
}
