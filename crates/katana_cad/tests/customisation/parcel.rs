//! Parcels (tests/cad/test_parcel.cpp).
//!
//! The assertions here are definitions rather than worked results: due north is azimuth 0 and due
//! east is pi/2, a 100 by 50 rectangle has 5000 of area and 300 of perimeter, a 3-4-5 triangle
//! closes over 500. They are exactly what would catch the one easy mistake in this code, which is
//! swapping easting and northing on the way into the survey layer - a rectangle would still report
//! 5000, and every bearing would be wrong.

use katana_cad::parcel::{
    format_parcel_report, format_parcel_summary, legal_description, parcel_courses_csv,
    parcel_labels, parcel_report,
};
use katana_core::ErrorCode;
use katana_geometry::{Point2, Polyline2};
use katana_math::PI;

use crate::near;

fn closed(vertices: Vec<Point2>) -> Polyline2 {
    Polyline2::new(vertices, true)
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

// (0,0) -> (100,0) -> (100,50) -> (0,50): east, north, west, south, drawn counter-clockwise.
fn rectangle() -> Polyline2 {
    closed(vec![
        p(0.0, 0.0),
        p(100.0, 0.0),
        p(100.0, 50.0),
        p(0.0, 50.0),
    ])
}

mod parcel {
    use super::*;

    #[test]
    fn a_rectangle_reports_its_courses_area_and_centroid() {
        let report =
            parcel_report(&rectangle()).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(report.courses.len(), 4);
        let courses = &report.courses;

        near(courses[0].azimuth, PI / 2.0, 1e-12); // east
        near(courses[0].distance, 100.0, 1e-12);
        near(courses[1].azimuth, 0.0, 1e-12); // north
        near(courses[1].distance, 50.0, 1e-12);
        near(courses[2].azimuth, 3.0 * PI / 2.0, 1e-12); // west
        near(courses[3].azimuth, PI, 1e-12); // south
        assert_eq!(
            courses[3].to,
            p(0.0, 0.0),
            "the last course closes on the first vertex"
        );

        near(report.area, 5000.0, 1e-9);
        near(report.perimeter, 300.0, 1e-9);
        near(report.centroid.x, 50.0, 1e-9);
        near(report.centroid.y, 25.0, 1e-9);
        assert!(!report.clockwise);

        // Bearings as a surveyor writes them: the survey layer puts pi/2 in the
        // north-east quadrant as N 90 E, pi as S 0 E, 3 pi / 2 as S 90 W.
        assert!(
            courses[0].bearing.contains("N 90"),
            "{}",
            courses[0].bearing
        );
        assert!(courses[0].bearing.contains('E'));
        assert!(courses[1].bearing.contains("N 0"), "{}", courses[1].bearing);
        assert!(
            courses[2].bearing.contains("S 90"),
            "{}",
            courses[2].bearing
        );
        assert!(courses[2].bearing.contains('W'));
        assert!(courses[3].bearing.contains("S 0"), "{}", courses[3].bearing);
    }

    #[test]
    fn the_same_boundary_drawn_clockwise_is_the_same_parcel() {
        let report = parcel_report(&closed(vec![
            p(0.0, 0.0),
            p(0.0, 50.0),
            p(100.0, 50.0),
            p(100.0, 0.0),
        ]))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(report.clockwise);
        near(report.area, 5000.0, 1e-9); // area is never negative
        near(report.perimeter, 300.0, 1e-9);
        near(report.courses[0].azimuth, 0.0, 1e-12); // the first course now runs north
        near(report.centroid.x, 50.0, 1e-9);
    }

    #[test]
    fn a_closing_course_across_two_quadrants_reads_as_a_quadrant_bearing() {
        // (0,0) -> (300,0) -> (300,400): the closing course runs 300 west and
        // 400 south, azimuth atan2(-300, -400) + 2 pi = 216.8699 degrees, which
        // surveyors write S 36°52'12" W (36.8699 degrees is 36°52'11.6", to the
        // nearest second 12). The 3-4-5 triangle closes over exactly 500.
        let report = parcel_report(&closed(vec![p(0.0, 0.0), p(300.0, 0.0), p(300.0, 400.0)]))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(report.courses.len(), 3);
        let closing = &report.courses[2];
        near(
            closing.azimuth,
            (-300.0_f64).atan2(-400.0) + 2.0 * PI,
            1e-12,
        );
        near(closing.distance, 500.0, 1e-12);
        assert!(closing.bearing.contains("S 36"), "{}", closing.bearing);
        assert!(closing.bearing.contains("52'"), "{}", closing.bearing);
        assert!(closing.bearing.contains("12\""), "{}", closing.bearing);
        assert!(closing.bearing.contains('W'), "{}", closing.bearing);
        near(report.area, 60000.0, 1e-9);
        near(report.perimeter, 1200.0, 1e-9);
    }

    #[test]
    fn the_legal_description_lists_every_course_and_the_area() {
        let report = parcel_report(&rectangle()).expect("a rectangle is a parcel");
        let description = legal_description(&report, "Lot 1");
        assert!(description.contains("Lot 1"), "{description}");
        assert_eq!(description.matches("thence").count(), 4, "{description}");
        assert!(description.contains("N 90"), "{description}");
        assert!(description.contains("100.000"), "{description}");
        assert!(description.contains("5000.000"), "{description}");
        assert!(description.contains("0.5000 ha"), "{description}");
        assert!(description.contains("point of beginning"), "{description}");
    }

    #[test]
    fn labels_read_along_each_course_and_none_is_upside_down() {
        let report = parcel_report(&rectangle()).expect("a rectangle is a parcel");
        let labels =
            parcel_labels(&report, 2.0).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(labels.len(), 5, "four courses and the area");
        for (i, label) in labels.iter().take(4).enumerate() {
            assert_eq!(label.height, 2.0);
            // Readable: rotated to lie along the course but never past a quarter
            // turn either way, so the west and south courses are flipped.
            assert!(label.rotation > -PI / 2.0 - 1e-12, "course {i}");
            assert!(label.rotation <= PI / 2.0 + 1e-12, "course {i}");
            assert!(
                label.text.contains(&report.courses[i].bearing),
                "{}",
                label.text
            );
        }
        assert!(labels[0].text.contains("100.000"));
        near(labels[0].rotation, 0.0, 1e-12); // east reads left to right
        near(labels[1].rotation, PI / 2.0, 1e-12); // north reads upward
        near(labels[2].rotation, 0.0, 1e-12); // west is flipped to read left to right
        near(labels[3].rotation, PI / 2.0, 1e-12); // south is flipped to read upward
        // Course labels sit inside the boundary: the east course runs along
        // y = 0, so its label is above it.
        assert!(labels[0].position.y > 0.0);
        assert!(labels[0].position.y < 25.0);
        let area = &labels[labels.len() - 1];
        assert!(area.text.contains("5000.000"), "{}", area.text);
        near(area.position.y, 25.0, 2.0 + 1e-9); // at the centroid, give or take the text
        assert_eq!(
            parcel_labels(&report, 0.0).expect_err("no height").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn refuses_what_is_not_a_parcel_and_drops_repeated_corners() {
        let mut open = rectangle();
        open.closed = false;
        assert_eq!(
            parcel_report(&open).expect_err("open").code,
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            parcel_report(&closed(vec![p(0.0, 0.0), p(1.0, 1.0)]))
                .expect_err("two corners")
                .code,
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            parcel_report(&closed(vec![p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0)]))
                .expect_err("collinear: no area")
                .code,
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            parcel_report(&closed(vec![p(0.0, 0.0), p(f64::NAN, 0.0), p(1.0, 1.0)]))
                .expect_err("not finite")
                .code,
            ErrorCode::InvalidArgument
        );
        // A corner clicked twice is one corner, not a zero-length course.
        let report = parcel_report(&closed(vec![
            p(0.0, 0.0),
            p(100.0, 0.0),
            p(100.0, 0.0),
            p(100.0, 50.0),
            p(0.0, 50.0),
        ]))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(report.courses.len(), 4);
        near(report.area, 5000.0, 1e-9);
    }

    // ---- the report's text: what PARCEL id prints and the dialog shows ------------------

    #[test]
    fn the_report_is_a_row_per_course_then_the_summary() {
        let report = parcel_report(&rectangle()).expect("a rectangle is a parcel");
        // 5000 m2 is 0.5 ha; the perimeter 300; the centroid the middle.
        assert_eq!(
            format_parcel_summary(&report),
            "area 5000.000 m2 (0.500 ha), perimeter 300.000 m, centroid 50.000,25.000, drawn \
             counter-clockwise"
        );
        let text = format_parcel_report(&report);
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 6, "{text}"); // a header, four courses, the summary
        assert_eq!(
            lines[lines.len() - 1],
            format!("  {}", format_parcel_summary(&report))
        );
        assert!(lines[1].contains("   1 "), "{}", lines[1]);
        assert!(
            lines[1].contains(&report.courses[0].bearing),
            "{}",
            lines[1]
        );
        assert!(lines[1].contains("100.000"), "{}", lines[1]);
        assert!(
            lines[4].contains(&report.courses[3].bearing),
            "{}",
            lines[4]
        );
    }

    #[test]
    fn the_courses_csv_quotes_the_bearing_and_keeps_a_negative_coordinates_sign() {
        // The rectangle moved to straddle the origin: its first corner is at
        // (-10, -20).
        let report = parcel_report(&closed(vec![
            p(-10.0, -20.0),
            p(90.0, -20.0),
            p(90.0, 30.0),
            p(-10.0, 30.0),
        ]))
        .expect("a rectangle is a parcel");
        let csv = parcel_courses_csv(&report);
        assert_eq!(
            csv.split("\r\n").next(),
            Some(
                "Course,From easting,From northing,To easting,To northing,Azimuth (deg),Bearing,\
                 Distance"
            )
        );
        // Due east is azimuth 90, N 90 E; the bearing holds a double quote, so
        // the field is quoted and the quote doubled (RFC 4180).
        assert_eq!(report.courses[0].bearing, "N 90\u{b0}00'00\" E");
        let first = "1,-10.000,-20.000,90.000,-20.000,90.000000,\"N 90\u{b0}00'00\"\" \
                     E\",100.000\r\n";
        assert!(csv.contains(first), "{csv}");
        assert_eq!(csv.matches("\r\n").count(), 5); // the header and four courses
    }
}
