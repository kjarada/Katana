// The setting-out table and the profile report (alignment_report.rs), and the
// ALIGN verbs that print them and edit PIs (tests/cad/test_alignment_report.cpp).
//
// The numbers are worked by hand from the circle, not taken from a run:
//   * the corner (0,0) (100,0) (100,100) rounded with R = 50 turns 90 degrees,
//     so its tangent length is R tan 45 = 50 - the TC is at chainage 50,
//     (50,0) - and its arc is 50 pi / 2 = 78.540 long, so the CT is at
//     128.540, (100,50), and the end at 178.540;
//   * 25 m into that arc the chord angle is 25 / 50 = 0.5 rad about the
//     centre (50,50): the point is (50 + 50 sin 0.5, 50 - 50 cos 0.5) =
//     (73.971, 6.121), heading 0.5 rad left of east, azimuth 61.352 degrees;
//   * a 30 degree corner with R = 300 and 90 m spirals keeps an arc of
//     R (delta - 2 theta_s) with theta_s = Ls / 2R = 0.15 rad: 67.080 m;
//   * the textbook sag, -3% into +2% over a 100 m curve, has K = 100 / 5 = 20.

use katana_cad::SettingOutStation;
use katana_cad::alignment_report::{
    curve_k, format_profile_report, format_setting_out, setting_out_csv, setting_out_stations,
};
use katana_core::ErrorCode;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentPi, HorizontalAlignment, Point2, ProfileElement, ProfileElementKind, ProfilePvi,
    SolvedAlignment, VerticalAlignment,
};
use katana_math::numerics::{DEG_TO_RAD, PI};

use crate::{Session, err, near, ok};

fn solved(pis: Vec<AlignmentPi>) -> SolvedAlignment {
    let definition = HorizontalAlignment {
        pis,
        ..HorizontalAlignment::default()
    };
    ok(solve_alignment(&definition))
}

// A PI with no curve, and one rounded with a radius and spirals.
fn corner(x: f64, y: f64) -> AlignmentPi {
    AlignmentPi {
        point: Point2::new(x, y),
        ..AlignmentPi::default()
    }
}

fn curve(x: f64, y: f64, radius: f64, spiral_in: f64, spiral_out: f64) -> AlignmentPi {
    AlignmentPi {
        point: Point2::new(x, y),
        radius,
        spiral_in,
        spiral_out,
    }
}

// (0,0) (100,0) (100,100), the corner rounded with R = 50: a left turn.
fn rounded_corner() -> SolvedAlignment {
    solved(vec![
        corner(0.0, 0.0),
        curve(100.0, 0.0, 50.0, 0.0, 0.0),
        corner(100.0, 100.0),
    ])
}

fn keys_of(rows: &[SettingOutStation]) -> Vec<String> {
    rows.iter()
        .filter(|row| !row.key.is_empty())
        .map(|row| row.key.clone())
        .collect()
}

// The text up to the first `separator`, or all of it: substr(0, find(...)).
fn up_to<'a>(text: &'a str, separator: &str) -> &'a str {
    text.split(separator).next().unwrap_or("")
}

// The PIs of the alignment `name`, failing the test when there is none.
fn pis(session: &Session, name: &str) -> Vec<AlignmentPi> {
    match session.document.model().alignments.find(name) {
        Some(alignment) => alignment.horizontal.pis.clone(),
        None => panic!("no alignment {name}"),
    }
}

mod setting_out {
    use super::*;

    #[test]
    fn every_interval_station_and_every_key_station_is_a_row_and_a_key_on_the_interval_is_one_row()
    {
        let rows = ok(setting_out_stations(&rounded_corner(), 25.0));
        // 0 25 50 75 100 125 150 175 on the interval, the CT at 128.540 and the
        // end at 178.540 off it; the TC at 50 is on it and is ONE row.
        let expected = [
            0.0,
            25.0,
            50.0,
            75.0,
            100.0,
            125.0,
            50.0 + 25.0 * PI,
            150.0,
            175.0,
            100.0 + 25.0 * PI,
        ];
        assert_eq!(rows.len(), expected.len());
        for (row, &station) in rows.iter().zip(&expected) {
            near(row.station, station, 1e-9);
        }
        assert_eq!(rows[0].key, "start");
        assert_eq!(rows[2].key, "TC");
        assert_eq!(rows[6].key, "CT");
        assert_eq!(rows[9].key, "end");
        assert!(rows[1].key.is_empty());
    }

    #[test]
    fn a_point_on_the_arc_is_where_the_circle_says_with_its_azimuth_and_curvature() {
        let rows = ok(setting_out_stations(&rounded_corner(), 25.0));
        let on_arc = &rows[3]; // chainage 75
        near(on_arc.point.x, 50.0 + 50.0 * 0.5_f64.sin(), 1e-6);
        near(on_arc.point.y, 50.0 - 50.0 * 0.5_f64.cos(), 1e-6);
        near(on_arc.direction, 0.5, 1e-9);
        near(on_arc.azimuth, PI / 2.0 - 0.5, 1e-9);
        near(on_arc.curvature, 1.0 / 50.0, 1e-12); // left
        // The start runs due east, the end due north: azimuths 90 and 0.
        let first = &rows[0];
        let last = &rows[rows.len() - 1];
        near(first.azimuth, PI / 2.0, 1e-12);
        near(last.azimuth, 0.0, 1e-9);
        near(last.point.x, 100.0, 1e-9);
        near(last.point.y, 100.0, 1e-9);
        assert_eq!(first.curvature, 0.0);
    }

    #[test]
    fn a_spiralled_curve_is_keyed_tssccsst_with_the_lengths_between_them() {
        let alignment = solved(vec![
            corner(0.0, 0.0),
            curve(400.0, 0.0, 300.0, 90.0, 90.0),
            corner(659.808, 150.0),
        ]);
        let rows = ok(setting_out_stations(&alignment, 1000.0));
        // An interval longer than the road leaves only the key stations.
        assert_eq!(keys_of(&rows), ["start", "TS", "SC", "CS", "ST", "end"]);
        assert_eq!(rows.len(), 6);
        near(rows[2].station - rows[1].station, 90.0, 1e-6);
        // 659.808,150 is 30 degrees to 3 decimals, so the arc is too.
        near(
            rows[3].station - rows[2].station,
            300.0 * (30.0 * DEG_TO_RAD - 0.3),
            1e-3,
        );
        near(rows[4].station - rows[3].station, 90.0, 1e-6);
    }

    #[test]
    fn a_corner_with_no_curve_is_keyed_pi() {
        let rows = ok(setting_out_stations(
            &solved(vec![
                corner(0.0, 0.0),
                corner(100.0, 0.0),
                corner(100.0, 100.0),
            ]),
            150.0,
        ));
        assert_eq!(keys_of(&rows), ["start", "PI", "end"]);
        // 0 and 150 on the interval, the PI at 100 and the end at 200.
        assert_eq!(rows.len(), 4);
        near(rows[1].station, 100.0, 1e-9);
        near(rows[2].station, 150.0, 1e-9);
    }

    #[test]
    fn an_interval_that_is_not_positive_or_gives_too_many_stations_is_refused() {
        let alignment = rounded_corner();
        for interval in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let refusal = err(setting_out_stations(&alignment, interval));
            assert_eq!(refusal.code, ErrorCode::InvalidArgument, "{interval}");
        }
        // 100 + 25 pi = 178.5398 m every 1 mm is the stations 0 to 178.539 m:
        // 178 540 of them, over the limit.
        let too_many = err(setting_out_stations(&alignment, 0.001));
        assert_eq!(too_many.code, ErrorCode::InvalidArgument);
        assert!(
            too_many.context.contains("178540 stations"),
            "{}",
            too_many.describe()
        );
    }

    #[test]
    fn the_records_give_every_field_as_key_equals_value() {
        let rows = ok(setting_out_stations(&rounded_corner(), 25.0));
        let text = format_setting_out(&rows);
        assert_eq!(
            up_to(&text, "\n"),
            "station=0.000 x=0.000 y=0.000 direction=0.000 azimuth=90\u{b0}00'00\" \
             radius=straight key=start"
        );
        assert!(
            text.contains(
                "station=75.000 x=73.971 y=6.121 direction=28.648 \
                 azimuth=61\u{b0}21'08\" radius=50.000 turn=left\n"
            ),
            "{text}"
        );
        // A station with no key has no key field: absent, not blank.
        assert!(
            text.contains(
                "station=25.000 x=25.000 y=0.000 direction=0.000 \
                 azimuth=90\u{b0}00'00\" radius=straight\n"
            ),
            "{text}"
        );
        assert_eq!(text.matches('\n').count() + 1, 10);
    }

    #[test]
    fn a_right_hand_curve_says_so_and_its_csv_radius_is_negative() {
        let rows = ok(setting_out_stations(
            &solved(vec![
                corner(0.0, 0.0),
                curve(100.0, 0.0, 50.0, 0.0, 0.0),
                corner(100.0, -100.0),
            ]),
            25.0,
        ));
        assert!(format_setting_out(&rows).contains("radius=50.000 turn=right"));
        let csv = setting_out_csv(&rows);
        assert_eq!(
            up_to(&csv, "\r\n"),
            "Chainage,Easting,Northing,Azimuth (deg),Azimuth (DMS),Radius,Key"
        );
        // The DMS field holds a double quote, so it is quoted and the quote doubled.
        assert!(
            csv.contains("0.000,0.000,0.000,90.000000,\"90\u{b0}00'00\"\"\",,start\r\n"),
            "{csv}"
        );
        assert!(csv.contains(",-50.000,"), "{csv}");
        // A header and one line per row, each ended CRLF.
        assert_eq!(csv.matches("\r\n").count(), rows.len() + 1);
    }
}

mod profile_report {
    use super::*;

    #[test]
    fn a_vertical_curves_k_is_its_length_per_percent_of_grade_change() {
        let sag = VerticalAlignment {
            pvis: vec![
                ProfilePvi {
                    station: 0.0,
                    elevation: 16.0,
                    curve_length: 0.0,
                },
                ProfilePvi {
                    station: 100.0,
                    elevation: 13.0,
                    curve_length: 100.0,
                },
                ProfilePvi {
                    station: 300.0,
                    elevation: 17.0,
                    curve_length: 0.0,
                },
            ],
        };
        let profile = ok(solve_profile(&sag));
        let mut curves = 0;
        for element in profile.elements() {
            let k = curve_k(element);
            if element.kind == ProfileElementKind::Curve {
                curves += 1;
                match k {
                    Some(k) => near(k, 20.0, 1e-9),
                    None => panic!("a curve has a K"),
                }
            } else {
                assert!(k.is_none());
            }
        }
        assert_eq!(curves, 1);
        let report = format_profile_report(&sag, &profile);
        assert!(report.contains("K=20.000"), "{report}");
        assert!(
            report.contains("low point  at 110.000 @ 13.600"),
            "{report}"
        );
    }

    #[test]
    fn a_curve_with_no_change_of_grade_has_no_k() {
        let flat = ProfileElement {
            kind: ProfileElementKind::Curve,
            length: 50.0,
            start_grade: 0.02,
            end_grade: 0.02,
            ..ProfileElement::default()
        };
        assert!(curve_k(&flat).is_none());
    }
}

mod align_verb {
    use super::*;

    #[test]
    fn stations_prints_the_setting_out_functions_table() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 100,0 100,100");
        session.ok("ALIGN SET road 1 50");
        let rows = ok(setting_out_stations(&rounded_corner(), 25.0));
        assert_eq!(
            session.ok("ALIGN STATIONS road 25"),
            format_setting_out(&rows)
        );
        assert_eq!(
            session.fails("ALIGN STATIONS road 0.0001"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn pis_alone_prints_every_pi_exactly_as_a_record() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 100.125,0.1 100,100");
        session.ok("ALIGN SET road 1 50 10 12.5");
        assert_eq!(
            session.ok("ALIGN PIS road"),
            "pi index=0 x=0 y=0 radius=0 spiral_in=0 spiral_out=0\n\
             pi index=1 x=100.125 y=0.1 radius=50 spiral_in=10 spiral_out=12.5\n\
             pi index=2 x=100 y=100 radius=0 spiral_in=0 spiral_out=0"
        );
        assert_eq!(session.fails("ALIGN PIS nosuch"), ErrorCode::NotFound);
    }

    #[test]
    fn pis_replaces_every_pi_as_one_undo_step_and_keeps_the_rest() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 100,0 100,100");
        session.ok("ALIGN START road 1000");
        session.ok("ALIGN DESIGN road 1000,16 1150,17");
        let before = session.document.history().undo_count();
        assert_eq!(
            session.ok("ALIGN PIS road 0,0 100,0,50 100,200,0,0,0 250,200"),
            "alignment road now has 4 PIs"
        );
        assert_eq!(session.document.history().undo_count(), before + 1);
        let road = pis(&session, "road");
        assert_eq!(road.len(), 4);
        assert_eq!(road[1].point, Point2::new(100.0, 0.0));
        assert_eq!(road[1].radius, 50.0);
        assert_eq!(road[2].point, Point2::new(100.0, 200.0));
        assert_eq!(road[3].point, Point2::new(250.0, 200.0));
        // The chainage origin and the design profile are not the PIs'.
        match session.document.model().alignments.find("road") {
            Some(road) => {
                assert_eq!(road.horizontal.start_station, 1000.0);
                assert!(road.vertical.is_some());
            }
            None => panic!("no alignment road"),
        }

        session.ok("UNDO");
        let road = pis(&session, "road");
        assert_eq!(road.len(), 3);
        assert_eq!(road[2].point, Point2::new(100.0, 100.0));
    }

    // Setting what is already there is not an edit: the Alignment Manager's
    // Apply on a grid nobody edited writes the stored PIs and PVIs back
    // exactly, and each press was once an undo step and a drawing to save.
    #[test]
    fn pis_and_design_that_change_nothing_are_no_undo_step() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 100,0 100,100");
        session.ok("ALIGN SET road 1 50");
        session.ok("ALIGN DESIGN road 0,16 150,17");
        let before = session.document.history().undo_count();
        assert_eq!(
            session.ok("ALIGN PIS road 0,0 100,0,50 100,100"),
            "alignment road unchanged"
        );
        assert_eq!(
            session.ok("ALIGN DESIGN road 0,16 150,17"),
            "alignment road unchanged"
        );
        assert_eq!(session.document.history().undo_count(), before);
        // A change is still one step each.
        assert_eq!(
            session.ok("ALIGN PIS road 0,0 100,0,60 100,100"),
            "alignment road now has 3 PIs"
        );
        assert_eq!(
            session.ok("ALIGN DESIGN road 0,16 150,18"),
            "alignment road designed with 2 PVIs"
        );
        assert_eq!(session.document.history().undo_count(), before + 2);
    }

    #[test]
    fn pis_refuses_what_cannot_be_an_alignment_and_keeps_the_last_good_one() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 100,0 100,100");
        // One PI.
        assert_eq!(
            session.fails("ALIGN PIS road 0,0"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("ALIGN PIS road 0,0 5"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("ALIGN PIS road 0,0 1,2,3,4,5,6"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("ALIGN PIS road 0,0 1,x"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("ALIGN PIS road 0,0 1,1,"),
            ErrorCode::InvalidArgument
        );
        // Coincident PIs: the solver refuses them, naming them, and the model
        // keeps the definition it had.
        assert_eq!(
            session.fails("ALIGN PIS road 0,0 0,0 5,5"),
            ErrorCode::InvalidGeometry
        );
        let road = pis(&session, "road");
        assert_eq!(road.len(), 3);
        assert_eq!(road[1].point, Point2::new(100.0, 0.0));
    }
}
