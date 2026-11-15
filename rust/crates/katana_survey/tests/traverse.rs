//! Twin of tests/survey/test_traverse.cpp.

mod survey_support;

mod survey_traverse {
    use crate::survey_support::{double_eq, error_code, near};
    use katana_core::ErrorCode;
    use katana_math::numerics::{DEG_TO_RAD, HALF_PI, PI, TWO_PI};
    use katana_survey::traverse::{compute_traverse, validate_traverse};
    use katana_survey::{
        Coordinate2, Traverse, TraverseAdjustment, TraverseClosingAngle, TraverseKind,
        TraverseOptions, TraverseSetup,
    };

    const ARC_SECOND: f64 = DEG_TO_RAD / 3600.0;

    fn c(northing: f64, easting: f64) -> Coordinate2 {
        Coordinate2 { northing, easting }
    }

    fn setup(id: &str, angle: f64, distance: f64) -> TraverseSetup {
        TraverseSetup {
            station_id: String::from(id),
            angle,
            distance,
        }
    }

    fn defaults() -> TraverseOptions {
        TraverseOptions::default()
    }

    // Square loop A -> B -> C -> D -> A, run counter-clockwise on the map so that
    // the clockwise "angles right" are the interior angles (90° each):
    //   A (N1000, E1000) -> B due east -> C due north -> D due west -> A due south.
    // The angles are exact; the distances carry the injected misclosure
    //   sum of departures = AB - CD = 99.97 - 100.03 = -0.06
    //   sum of latitudes  = BC - DA = 100.04 - 99.96 = +0.08
    // so the linear misclosure is the 3-4-5 triple 0.10 m over exactly 400.00 m.
    fn square_loop() -> Traverse {
        Traverse {
            name: String::from("square"),
            kind: TraverseKind::ClosedLoop,
            start: c(1000.0, 1000.0),
            // A -> B is due east
            start_azimuth: HALF_PI,
            setups: vec![
                setup("A", HALF_PI, 99.97),
                setup("B", HALF_PI, 100.04),
                setup("C", HALF_PI, 100.03),
                setup("D", HALF_PI, 99.96),
            ],
            ..Default::default()
        }
    }

    // Link traverse with exact geometry, A -> P1 -> P2 -> Z:
    //   leg      azimuth  distance        dN        dE
    //   A  - P1     60°   200            100       100*sqrt(3)
    //   P1 - P2    120°   150            -75        75*sqrt(3)
    //   P2 - Z      45°   100*sqrt(2)    100       100
    // Reference azimuths: A -> R1 = 210°, Z -> R2 = 100°. Clockwise angles from
    // the back direction to the forward direction (forward - back, mod 360°):
    //   at A : 60  - 210       = 210°
    //   at P1: 120 - (60+180)  = 240°
    //   at P2: 45  - (120+180) = 105°
    //   at Z : 100 - (45+180)  = 235°   (closing angle)
    fn link_start() -> Coordinate2 {
        c(5000.0, 2000.0)
    }

    fn link_p1() -> Coordinate2 {
        c(5100.0, 2000.0 + 100.0 * 3.0_f64.sqrt())
    }

    fn link_p2() -> Coordinate2 {
        c(5025.0, 2000.0 + 175.0 * 3.0_f64.sqrt())
    }

    fn link_end() -> Coordinate2 {
        c(5125.0, 2100.0 + 175.0 * 3.0_f64.sqrt())
    }

    fn link_traverse() -> Traverse {
        Traverse {
            name: String::from("link"),
            kind: TraverseKind::Link,
            start: link_start(),
            start_azimuth: 210.0 * DEG_TO_RAD,
            setups: vec![
                setup("A", 210.0 * DEG_TO_RAD, 200.0),
                setup("P1", 240.0 * DEG_TO_RAD, 150.0),
                setup("P2", 105.0 * DEG_TO_RAD, 100.0 * 2.0_f64.sqrt()),
            ],
            end_station_id: String::from("Z"),
            end: link_end(),
            closing_angle: Some(TraverseClosingAngle {
                angle: 235.0 * DEG_TO_RAD,
                reference_azimuth: 100.0 * DEG_TO_RAD,
            }),
        }
    }

    // ---- closed loop -----------------------------------------------------------

    #[test]
    fn closed_loop_misclosure_and_precision() {
        let result = match compute_traverse(&square_loop(), &defaults()) {
            Ok(result) => result,
            Err(error) => panic!("{}", error.describe()),
        };

        assert_eq!(result.kind, TraverseKind::ClosedLoop);
        assert_eq!(result.angle_count, 4);
        assert!(result.angular_misclosure.is_some());
        // 4 x 90° = (n-2) x 180°
        near(result.angular_misclosure.unwrap(), 0.0, 1e-15);
        near(result.angle_correction, 0.0, 1e-15);
        near(result.total_length, 400.0, 1e-12);

        assert!(result.linear_misclosure.is_some());
        let misclosure = result.linear_misclosure.unwrap();
        near(misclosure.latitude, 0.08, 1e-12);
        near(misclosure.departure, -0.06, 1e-12);
        near(misclosure.length, 0.10, 1e-12);
        near(misclosure.relative_precision, 0.10 / 400.0, 1e-15);
        assert!(misclosure.precision_denominator.is_some());
        // 1 : 4000
        near(misclosure.precision_denominator.unwrap(), 4000.0, 1e-6);

        assert_eq!(result.legs.len(), 4);
        let azimuths = [90.0, 0.0, 270.0, 180.0];
        let latitudes = [0.0, 100.04, 0.0, -99.96];
        let departures = [99.97, 0.0, -100.03, 0.0];
        let to = ["B", "C", "D", "A"];
        for (k, leg) in result.legs.iter().enumerate() {
            near(leg.azimuth, azimuths[k] * DEG_TO_RAD, 1e-14);
            near(leg.latitude, latitudes[k], 1e-12);
            near(leg.departure, departures[k], 1e-12);
            assert_eq!(leg.to_id, to[k]);
        }
    }

    #[test]
    fn compass_rule_hand_verified() {
        let result = compute_traverse(&square_loop(), &defaults()).unwrap();

        // Compass (Bowditch) rule: correction = -misclosure * leg length / total
        // length, with misclosure (+0.08, -0.06) and total 400:
        //   leg   length   lat corr = -0.08 L/400   dep corr = +0.06 L/400
        //   AB     99.97        -0.019994               +0.0149955
        //   BC    100.04        -0.020008               +0.0150060
        //   CD    100.03        -0.020006               +0.0150045
        //   DA     99.96        -0.019992               +0.0149940
        //   sum                 -0.080000               +0.0600000   (closes exactly)
        let latitude_corrections = [-0.019994, -0.020008, -0.020006, -0.019992];
        let departure_corrections = [0.0149955, 0.0150060, 0.0150045, 0.0149940];
        let mut sum_latitude = 0.0;
        let mut sum_departure = 0.0;
        for (k, leg) in result.legs.iter().enumerate() {
            near(leg.latitude_correction, latitude_corrections[k], 1e-12);
            near(leg.departure_correction, departure_corrections[k], 1e-12);
            sum_latitude += leg.latitude_correction;
            sum_departure += leg.departure_correction;
        }
        near(sum_latitude, -0.08, 1e-12);
        near(sum_departure, 0.06, 1e-12);

        // Adjusted coordinates, accumulated by hand from A (1000, 1000):
        //   B: N = 1000 + 0      - 0.019994 =  999.980006   E = 1000 + 99.97 + 0.0149955 = 1099.9849955
        //   C: N = B  + 100.04   - 0.020008 = 1099.999998   E = B + 0       + 0.0150060 = 1100.0000015
        //   D: N = C  + 0        - 0.020006 = 1099.979992   E = C - 100.03  + 0.0150045 =  999.9850060
        //   A: N = D  - 99.96    - 0.019992 = 1000.000000   E = D + 0       + 0.0149940 = 1000.0000000
        // the start is not repeated
        assert_eq!(result.stations.len(), 4);
        let adjusted = [
            c(1000.0, 1000.0),
            c(999.980006, 1099.9849955),
            c(1099.999998, 1100.0000015),
            c(1099.979992, 999.985006),
        ];
        let unadjusted = [
            c(1000.0, 1000.0),
            c(1000.0, 1099.97),
            c(1100.04, 1099.97),
            c(1100.04, 999.94),
        ];
        let ids = ["A", "B", "C", "D"];
        for (s, station) in result.stations.iter().enumerate() {
            assert_eq!(station.id, ids[s]);
            near(station.adjusted.northing, adjusted[s].northing, 1e-9);
            near(station.adjusted.easting, adjusted[s].easting, 1e-9);
            near(station.unadjusted.northing, unadjusted[s].northing, 1e-9);
            near(station.unadjusted.easting, unadjusted[s].easting, 1e-9);
        }
        // The last adjusted leg returns to A.
        let last = &result.legs[3];
        near(
            result.stations[3].adjusted.northing + last.latitude + last.latitude_correction,
            1000.0,
            1e-9,
        );
        near(
            result.stations[3].adjusted.easting + last.departure + last.departure_correction,
            1000.0,
            1e-9,
        );
    }

    #[test]
    fn transit_rule_hand_verified() {
        let options = TraverseOptions {
            adjustment: TraverseAdjustment::Transit,
            ..Default::default()
        };
        let result = compute_traverse(&square_loop(), &options).unwrap();

        // Transit rule: latitude corrections in proportion to |latitude|,
        // departure corrections in proportion to |departure|.
        //   sum |lat| = 100.04 + 99.96 = 200   sum |dep| = 99.97 + 100.03 = 200
        //   BC lat corr = -0.08 * 100.04/200 = -0.040016   DA: -0.08 * 99.96/200 = -0.039984
        //   AB dep corr = +0.06 * 99.97/200  = +0.029991   CD: +0.06 * 100.03/200 = +0.030009
        // East-west legs get no latitude correction and vice versa.
        let latitude_corrections = [0.0, -0.040016, 0.0, -0.039984];
        let departure_corrections = [0.029991, 0.0, 0.030009, 0.0];
        for (k, leg) in result.legs.iter().enumerate() {
            near(leg.latitude_correction, latitude_corrections[k], 1e-12);
            near(leg.departure_correction, departure_corrections[k], 1e-12);
        }
    }

    #[test]
    fn no_adjustment_reports_misclosure_only() {
        let options = TraverseOptions {
            adjustment: TraverseAdjustment::None,
            ..Default::default()
        };
        let result = compute_traverse(&square_loop(), &options).unwrap();
        near(result.linear_misclosure.unwrap().length, 0.10, 1e-12);
        for leg in &result.legs {
            double_eq(leg.latitude_correction, 0.0);
            double_eq(leg.departure_correction, 0.0);
        }
        for station in &result.stations {
            assert_eq!(station.adjusted, station.unadjusted);
        }
    }

    #[test]
    fn angular_misclosure_is_balanced_equally_and_azimuths_wrap() {
        // Observed interior angles sum to 360°00'20": misclosure +20", -5" per angle.
        //   observed  A 90°00'20"   B 89°59'50"   C 90°00'05"   D 90°00'05"
        //   balanced  A 90°00'15"   B 89°59'45"   C 90°00'00"   D 90°00'00"
        // Azimuths from the balanced angles (previous + 180° + angle):
        //   AB = 90° (given)
        //   BC = 90° + 180° + 89°59'45"      = 359°59'45"   (just below the wrap)
        //   CD = 359°59'45" + 180° + 90°     = 269°59'45"
        //   DA = 269°59'45" + 180° + 90°     = 179°59'45"
        //   check: 179°59'45" + 180° + 90°00'15" = 450° = 90°.
        let mut traverse = square_loop();
        traverse.setups[0].angle = HALF_PI + 20.0 * ARC_SECOND;
        traverse.setups[1].angle = HALF_PI - 10.0 * ARC_SECOND;
        traverse.setups[2].angle = HALF_PI + 5.0 * ARC_SECOND;
        traverse.setups[3].angle = HALF_PI + 5.0 * ARC_SECOND;

        let result = compute_traverse(&traverse, &defaults()).unwrap();
        assert!(result.angular_misclosure.is_some());
        near(result.angular_misclosure.unwrap(), 20.0 * ARC_SECOND, 1e-14);
        near(result.angle_correction, -5.0 * ARC_SECOND, 1e-14);

        near(result.legs[0].azimuth, HALF_PI, 1e-14);
        near(result.legs[1].azimuth, TWO_PI - 15.0 * ARC_SECOND, 1e-13);
        near(result.legs[2].azimuth, 1.5 * PI - 15.0 * ARC_SECOND, 1e-13);
        near(result.legs[3].azimuth, PI - 15.0 * ARC_SECOND, 1e-13);
        for leg in &result.legs {
            assert!(leg.azimuth >= 0.0);
            assert!(leg.azimuth < TWO_PI);
        }

        // Without balancing the raw angles are carried: BC = 90° + 180° + 89°59'50".
        let raw = TraverseOptions {
            balance_angles: false,
            ..Default::default()
        };
        let unbalanced = compute_traverse(&traverse, &raw).unwrap();
        near(
            unbalanced.angular_misclosure.unwrap(),
            20.0 * ARC_SECOND,
            1e-14,
        );
        double_eq(unbalanced.angle_correction, 0.0);
        near(
            unbalanced.legs[1].azimuth,
            TWO_PI - 10.0 * ARC_SECOND,
            1e-13,
        );
    }

    #[test]
    fn clockwise_loop_uses_exterior_angles() {
        // The same square run the other way (A -> D -> C -> B): the angles right
        // are now the exterior angles, 270° each, summing to (n + 2) x 180° = 1080°.
        let traverse = Traverse {
            kind: TraverseKind::ClosedLoop,
            start: c(1000.0, 1000.0),
            // A -> D is due north
            start_azimuth: 0.0,
            setups: vec![
                setup("A", 1.5 * PI, 100.0),
                setup("D", 1.5 * PI, 100.0),
                setup("C", 1.5 * PI, 100.0),
                setup("B", 1.5 * PI, 100.0),
            ],
            ..Default::default()
        };
        let result = compute_traverse(&traverse, &defaults()).unwrap();
        near(result.angular_misclosure.unwrap(), 0.0, 1e-14);
        near(result.linear_misclosure.unwrap().length, 0.0, 1e-12);
        // D north of A, C north-east, B east.
        near(result.stations[1].adjusted.northing, 1100.0, 1e-10);
        near(result.stations[1].adjusted.easting, 1000.0, 1e-10);
        near(result.stations[2].adjusted.easting, 1100.0, 1e-10);
        near(result.stations[3].adjusted.northing, 1000.0, 1e-10);
        near(result.stations[3].adjusted.easting, 1100.0, 1e-10);
    }

    #[test]
    fn consistent_loop_closes_to_rounding_noise() {
        // A -> B north, then west, south, east: a consistent 100 m square. It
        // closes exactly on paper; in doubles cos(pi/2) is 6e-17 rather than 0,
        // so the misclosure is rounding noise (~1e-14 m), far below any survey
        // meaning.
        let traverse = Traverse {
            kind: TraverseKind::ClosedLoop,
            start: c(0.0, 0.0),
            start_azimuth: 0.0,
            setups: vec![
                setup("A", HALF_PI, 100.0),
                setup("B", HALF_PI, 100.0),
                setup("C", HALF_PI, 100.0),
                setup("D", HALF_PI, 100.0),
            ],
            ..Default::default()
        };
        let result = compute_traverse(&traverse, &defaults()).unwrap();
        let misclosure = result.linear_misclosure.unwrap();
        assert!(misclosure.length < 1e-12);
        assert!(misclosure.relative_precision < 1e-14);
    }

    #[test]
    fn exact_closure_has_no_precision_denominator() {
        // Due north is the one direction whose sine and cosine are exact (0 and
        // 1), so this single leg closes with a misclosure of exactly zero:
        // "1 : infinity" is reported as an absent denominator, not as a division
        // by zero.
        let traverse = Traverse {
            kind: TraverseKind::Link,
            start: c(1000.0, 2000.0),
            start_azimuth: 0.0,
            setups: vec![setup("A", 0.0, 100.0)],
            end_station_id: String::from("Z"),
            end: c(1100.0, 2000.0),
            ..Default::default()
        };
        let result = compute_traverse(&traverse, &defaults()).unwrap();
        assert!(result.linear_misclosure.is_some());
        let misclosure = result.linear_misclosure.unwrap();
        double_eq(misclosure.length, 0.0);
        double_eq(misclosure.relative_precision, 0.0);
        assert!(misclosure.precision_denominator.is_none());
        double_eq(result.legs[0].latitude_correction, 0.0);
    }

    // ---- link traverse ---------------------------------------------------------

    #[test]
    fn link_traverse_with_exact_observations_closes() {
        let result = match compute_traverse(&link_traverse(), &defaults()) {
            Ok(result) => result,
            Err(error) => panic!("{}", error.describe()),
        };

        // three setups plus the closing angle
        assert_eq!(result.angle_count, 4);
        assert!(result.angular_misclosure.is_some());
        near(result.angular_misclosure.unwrap(), 0.0, 1e-14);
        assert!(result.linear_misclosure.is_some());
        near(result.linear_misclosure.unwrap().length, 0.0, 1e-11);

        near(result.legs[0].azimuth, 60.0 * DEG_TO_RAD, 1e-14);
        near(result.legs[1].azimuth, 120.0 * DEG_TO_RAD, 1e-14);
        near(result.legs[2].azimuth, 45.0 * DEG_TO_RAD, 1e-14);

        assert_eq!(result.stations.len(), 4);
        let expected = [link_start(), link_p1(), link_p2(), link_end()];
        let ids = ["A", "P1", "P2", "Z"];
        for (s, station) in result.stations.iter().enumerate() {
            assert_eq!(station.id, ids[s]);
            near(station.adjusted.northing, expected[s].northing, 1e-10);
            near(station.adjusted.easting, expected[s].easting, 1e-10);
        }
        // control is stated exactly
        assert_eq!(result.stations[3].adjusted, link_end());
    }

    #[test]
    fn link_traverse_balances_injected_angular_misclosure() {
        // +6" on each of the four angles: misclosure +24", correction -6" each,
        // which restores the exact angles and therefore the exact coordinates.
        let mut traverse = link_traverse();
        for setup in &mut traverse.setups {
            setup.angle += 6.0 * ARC_SECOND;
        }
        if let Some(closing_angle) = traverse.closing_angle.as_mut() {
            closing_angle.angle += 6.0 * ARC_SECOND;
        }

        let result = compute_traverse(&traverse, &defaults()).unwrap();
        near(result.angular_misclosure.unwrap(), 24.0 * ARC_SECOND, 1e-13);
        near(result.angle_correction, -6.0 * ARC_SECOND, 1e-13);
        near(result.linear_misclosure.unwrap().length, 0.0, 1e-9);
        near(
            result.stations[1].unadjusted.northing,
            link_p1().northing,
            1e-9,
        );
        near(
            result.stations[2].unadjusted.easting,
            link_p2().easting,
            1e-9,
        );

        // Unbalanced, the 6", 12", 18" azimuth errors swing the far end sideways
        // by roughly d * error: visible at the millimetre level over these legs.
        let raw = TraverseOptions {
            balance_angles: false,
            adjustment: TraverseAdjustment::None,
        };
        let unbalanced = compute_traverse(&traverse, &raw).unwrap();
        let length = unbalanced.linear_misclosure.unwrap().length;
        assert!(length > 0.005);
        assert!(length < 0.05);
    }

    #[test]
    fn link_traverse_without_closing_angle_has_only_a_linear_check() {
        let mut traverse = link_traverse();
        traverse.closing_angle = None;
        // known end 3 cm further east than the traverse arrives
        traverse.end.easting += 0.03;
        let result = compute_traverse(&traverse, &defaults()).unwrap();
        assert!(result.angular_misclosure.is_none());
        assert_eq!(result.angle_count, 0);
        double_eq(result.angle_correction, 0.0);
        // misclosure = computed - known
        let misclosure = result.linear_misclosure.unwrap();
        near(misclosure.departure, -0.03, 1e-10);
        near(misclosure.latitude, 0.0, 1e-10);
        // Compass rule spreads +0.03 of easting by length: 200 : 150 : 141.42.
        let total = 350.0 + 100.0 * 2.0_f64.sqrt();
        near(
            result.legs[0].departure_correction,
            0.03 * 200.0 / total,
            1e-12,
        );
        near(
            result.stations[1].adjusted.easting,
            link_p1().easting + 0.03 * 200.0 / total,
            1e-10,
        );
    }

    // ---- open traverse ---------------------------------------------------------

    #[test]
    fn open_traverse_has_no_closure() {
        let mut traverse = link_traverse();
        traverse.kind = TraverseKind::Open;
        traverse.closing_angle = None;
        let result = compute_traverse(&traverse, &defaults()).unwrap();
        assert!(result.angular_misclosure.is_none());
        assert!(result.linear_misclosure.is_none());
        assert_eq!(result.stations.len(), 4);
        near(
            result.stations[3].adjusted.northing,
            link_end().northing,
            1e-10,
        );
        near(
            result.stations[3].adjusted.easting,
            link_end().easting,
            1e-10,
        );
        assert_eq!(result.stations[3].adjusted, result.stations[3].unadjusted);
    }

    #[test]
    fn single_leg_open_traverse() {
        // Backsight due north (azimuth 0), 90° to the right, 50 m: due east.
        let traverse = Traverse {
            kind: TraverseKind::Open,
            start: c(100.0, 200.0),
            start_azimuth: 0.0,
            setups: vec![setup("A", HALF_PI, 50.0)],
            end_station_id: String::from("P"),
            ..Default::default()
        };
        let result = compute_traverse(&traverse, &defaults()).unwrap();
        assert_eq!(result.legs.len(), 1);
        assert_eq!(result.stations.len(), 2);
        assert_eq!(result.stations[1].id, "P");
        near(result.stations[1].adjusted.northing, 100.0, 1e-13);
        near(result.stations[1].adjusted.easting, 250.0, 1e-13);
        double_eq(result.total_length, 50.0);
    }

    #[test]
    fn utm_sized_start_keeps_millimetres() {
        let mut traverse = link_traverse();
        let d_n = 5000000.0;
        let d_e = 500000.0;
        traverse.start = c(link_start().northing + d_n, link_start().easting + d_e);
        traverse.end = c(link_end().northing + d_n, link_end().easting + d_e);
        let result = compute_traverse(&traverse, &defaults()).unwrap();
        // Latitudes and departures are summed before they meet the large start
        // coordinates, so the closure is not polluted by 1e-9 m rounding per leg.
        assert!(result.linear_misclosure.unwrap().length < 5e-9);
        near(
            result.stations[2].adjusted.northing,
            link_p2().northing + d_n,
            5e-9,
        );
        near(
            result.stations[2].adjusted.easting,
            link_p2().easting + d_e,
            5e-9,
        );
    }

    // ---- validation ------------------------------------------------------------

    #[test]
    fn rejects_malformed_traverses() {
        let expect_invalid = |traverse: &Traverse, why: &str| {
            let result = compute_traverse(traverse, &defaults());
            assert!(result.is_err(), "{why}");
            assert_eq!(error_code(result), ErrorCode::InvalidArgument, "{why}");
            assert!(validate_traverse(traverse).is_err(), "{why}");
        };

        let empty = Traverse::default();
        expect_invalid(&empty, "no setups");

        let mut short_loop = square_loop();
        short_loop.setups.truncate(2);
        expect_invalid(&short_loop, "loop with two setups");

        let mut repeated = square_loop();
        repeated.setups[2].station_id = String::from("A");
        expect_invalid(&repeated, "repeated station id");

        let mut unnamed = square_loop();
        unnamed.setups[1].station_id.clear();
        expect_invalid(&unnamed, "empty station id");

        for distance in [0.0, -10.0, f64::NAN] {
            let mut bad = square_loop();
            bad.setups[1].distance = distance;
            expect_invalid(&bad, "bad distance");
        }

        let mut bad_angle = square_loop();
        bad_angle.setups[0].angle = f64::NAN;
        expect_invalid(&bad_angle, "non-finite angle");

        let mut bad_start = square_loop();
        bad_start.start.northing = f64::NAN;
        expect_invalid(&bad_start, "non-finite start");

        let mut no_end = link_traverse();
        no_end.end_station_id.clear();
        expect_invalid(&no_end, "missing end station id");

        let mut end_repeats = link_traverse();
        end_repeats.end_station_id = String::from("P1");
        expect_invalid(&end_repeats, "end id repeats an interior station");

        let mut bad_end = link_traverse();
        bad_end.end.easting = f64::NAN;
        expect_invalid(&bad_end, "non-finite end");

        let mut bad_closing = link_traverse();
        if let Some(closing_angle) = bad_closing.closing_angle.as_mut() {
            closing_angle.reference_azimuth = f64::NAN;
        }
        expect_invalid(&bad_closing, "non-finite closing azimuth");

        assert!(validate_traverse(&square_loop()).is_ok());
        assert!(validate_traverse(&link_traverse()).is_ok());
    }

    #[test]
    fn link_may_close_on_its_own_start_but_only_consistently() {
        // A loop oriented on an external reference mark: a link whose end is its
        // start.
        let mut traverse = Traverse {
            kind: TraverseKind::Link,
            start: c(1000.0, 1000.0),
            end: c(1000.0, 1000.0),
            end_station_id: String::from("A"),
            // reference mark due south of A
            start_azimuth: PI,
            // A -> B east (from south, 270° clockwise reaches east), then the square.
            setups: vec![
                setup("A", 1.5 * PI, 100.0),
                setup("B", HALF_PI, 100.0),
                setup("C", HALF_PI, 100.0),
                setup("D", HALF_PI, 100.0),
            ],
            // At A again: back direction D (due north, azimuth 0) to the mark (180°).
            closing_angle: Some(TraverseClosingAngle {
                angle: PI,
                reference_azimuth: PI,
            }),
            ..Default::default()
        };
        let result = match compute_traverse(&traverse, &defaults()) {
            Ok(result) => result,
            Err(error) => panic!("{}", error.describe()),
        };
        near(result.angular_misclosure.unwrap(), 0.0, 1e-14);
        near(result.linear_misclosure.unwrap().length, 0.0, 1e-11);
        assert_eq!(result.stations[result.stations.len() - 1].id, "A");

        // same id, different coordinates: contradictory
        traverse.end.easting += 1.0;
        assert!(compute_traverse(&traverse, &defaults()).is_err());
    }
}
