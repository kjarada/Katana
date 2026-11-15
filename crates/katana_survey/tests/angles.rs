//! Twin of tests/survey/test_angles.cpp.

mod survey_support;

mod survey_angles {
    use crate::survey_support::{PROPERTY_ITERATIONS, Random, double_eq, near};
    use katana_core::ErrorCode;
    use katana_math::numerics::{DEG_TO_RAD, HALF_PI, PI, TWO_PI};
    use katana_survey::angles::{
        self, ARC_SECOND_TO_RAD, BearingMeridian, BearingSide, DmsAngle, QuadrantBearing,
        arc_seconds_to_radians, azimuth_to_bearing, bearing_to_azimuth, degrees_to_radians,
        dms_to_radians, format_bearing, gon_to_radians, normalize_azimuth, parse_bearing,
        parse_dms, radians_to_arc_seconds, radians_to_degrees, radians_to_dms, radians_to_gon,
    };

    // Expected values are formed in decimal degrees, a different route from the
    // implementation's integer arc-second arithmetic.
    const DMS_453015: f64 = (45.0 + 30.0 / 60.0 + 15.0 / 3600.0) * DEG_TO_RAD;

    // ---- units ---------------------------------------------------------------

    #[test]
    fn unit_conversions() {
        double_eq(degrees_to_radians(180.0), PI);
        double_eq(radians_to_degrees(HALF_PI), 90.0);
        // 400 gon in a full turn
        double_eq(gon_to_radians(200.0), PI);
        double_eq(gon_to_radians(100.0), HALF_PI);
        double_eq(radians_to_gon(TWO_PI), 400.0);
        // 1 degree = 3600 arc-seconds.
        near(arc_seconds_to_radians(3600.0), DEG_TO_RAD, 1e-17);
        near(radians_to_arc_seconds(DEG_TO_RAD), 3600.0, 1e-9);
    }

    #[test]
    fn normalize_azimuth_wraps_into_full_circle() {
        double_eq(normalize_azimuth(0.0), 0.0);
        double_eq(normalize_azimuth(TWO_PI), 0.0);
        near(normalize_azimuth(-HALF_PI), 1.5 * PI, 1e-15);
        near(normalize_azimuth(5.0 * PI), PI, 1e-14);
        // never returns 2*pi itself
        assert!(normalize_azimuth(-1e-20) < TWO_PI);
    }

    // ---- DMS parsing ---------------------------------------------------------

    #[test]
    fn parse_dms_accepts_common_notations() {
        for text in [
            "45\u{B0}30'15\"",
            "45 30 15",
            "45-30-15",
            "45:30:15",
            "45d30m15s",
            "45D30M15S",
            "  45\u{B0} 30' 15\"  ",
            "+45 30 15",
            "45\u{B0}30'15''",
            "45\u{BA}30\u{2032}15\u{2033}",
        ] {
            let parsed = parse_dms(text);
            assert!(parsed.is_ok(), "{text}");
            near(parsed.unwrap(), DMS_453015, 1e-15);
        }
    }

    #[test]
    fn parse_dms_fraction_on_last_field_only() {
        near(parse_dms("45.5").unwrap(), 45.5 * DEG_TO_RAD, 1e-15);
        near(parse_dms("45.5\u{B0}").unwrap(), 45.5 * DEG_TO_RAD, 1e-15);
        // 30.25' = 30'15"
        near(parse_dms("45\u{B0}30.25'").unwrap(), DMS_453015, 1e-15);
        near(
            parse_dms("45 30 15.5").unwrap(),
            (45.0 + 30.0 / 60.0 + 15.5 / 3600.0) * DEG_TO_RAD,
            1e-15,
        );

        assert!(parse_dms("45.5 30").is_err());
        assert!(parse_dms("45 30.5 10").is_err());
    }

    #[test]
    fn parse_dms_sign_applies_to_whole_angle() {
        // -0°30' is -0.5°, which a signed degrees field could not express.
        near(
            parse_dms("-0\u{B0}30'00\"").unwrap(),
            -0.5 * DEG_TO_RAD,
            1e-16,
        );
        near(
            parse_dms("-12 05 07").unwrap(),
            -(12.0 + 5.0 / 60.0 + 7.0 / 3600.0) * DEG_TO_RAD,
            1e-15,
        );
    }

    #[test]
    fn parse_dms_rejects_malformed_text() {
        for text in [
            "",
            "   ",
            "abc",
            "45\u{B0}60'00\"",
            "45\u{B0}30'60\"",
            "45 30 15 10",
            "45\u{B0}30'15\"x",
            "1e5",
            "nan",
            "inf",
            "--45",
            "45'",
            "12 \" 30",
        ] {
            let parsed = parse_dms(text);
            assert!(parsed.is_err(), "'{text}'");
            assert_eq!(parsed.unwrap_err().code, ErrorCode::ParseFailure, "{text}");
        }
    }

    // ---- DMS formatting ------------------------------------------------------

    #[test]
    fn format_dms() {
        assert_eq!(
            angles::format_dms(DMS_453015, 2).unwrap(),
            "45\u{B0}30'15.00\""
        );
        assert_eq!(
            angles::format_dms(DMS_453015, 0).unwrap(),
            "45\u{B0}30'15\""
        );
        assert_eq!(angles::format_dms(0.0, 1).unwrap(), "0\u{B0}00'00.0\"");
        assert_eq!(
            angles::format_dms(-0.5 * DEG_TO_RAD, 2).unwrap(),
            "-0\u{B0}30'00.00\""
        );
        assert_eq!(angles::format_dms(TWO_PI, 0).unwrap(), "360\u{B0}00'00\"");
    }

    #[test]
    fn format_dms_carries_rounded_seconds_into_minutes_and_degrees() {
        // 10°59'59.9996": rounds up to 11° at 2 and 3 decimals, stays at 4.
        let angle = (10.0 + 59.0 / 60.0 + 59.9996 / 3600.0) * DEG_TO_RAD;
        assert_eq!(angles::format_dms(angle, 2).unwrap(), "11\u{B0}00'00.00\"");
        assert_eq!(angles::format_dms(angle, 3).unwrap(), "11\u{B0}00'00.000\"");
        assert_eq!(
            angles::format_dms(angle, 4).unwrap(),
            "10\u{B0}59'59.9996\""
        );

        let dms = radians_to_dms(angle, 2).unwrap();
        assert_eq!(dms.degrees, 11);
        assert_eq!(dms.minutes, 0);
        double_eq(dms.seconds, 0.0);
    }

    #[test]
    fn format_dms_never_prints_negative_zero() {
        assert_eq!(angles::format_dms(-1e-12, 2).unwrap(), "0\u{B0}00'00.00\"");
    }

    #[test]
    fn format_dms_rejects_bad_arguments() {
        assert_eq!(
            angles::format_dms(f64::NAN, 2).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            angles::format_dms(f64::INFINITY, 2).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            angles::format_dms(1.0, -1).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            angles::format_dms(1.0, 10).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        // not resolvable
        assert_eq!(
            angles::format_dms(1e12, 9).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn dms_struct_round_trip() {
        let dms = DmsAngle {
            negative: true,
            degrees: 12,
            minutes: 5,
            seconds: 7.25,
        };
        let radians = dms_to_radians(dms).unwrap();
        near(
            radians,
            -(12.0 + 5.0 / 60.0 + 7.25 / 3600.0) * DEG_TO_RAD,
            1e-15,
        );
        assert_eq!(radians_to_dms(radians, 2).unwrap(), dms);

        assert!(
            dms_to_radians(DmsAngle {
                negative: false,
                degrees: 1,
                minutes: 60,
                seconds: 0.0
            })
            .is_err()
        );
        assert!(
            dms_to_radians(DmsAngle {
                negative: false,
                degrees: 1,
                minutes: 0,
                seconds: 60.0
            })
            .is_err()
        );
        assert!(
            dms_to_radians(DmsAngle {
                negative: false,
                degrees: 1,
                minutes: 0,
                seconds: -1.0
            })
            .is_err()
        );
    }

    #[test]
    fn property_format_then_parse_round_trips() {
        let mut random = Random::default();
        // Formatting with 6 decimals rounds to 1e-6", so the round trip is exact
        // to half of that: 0.5e-6" = 2.4e-12 rad.
        let tolerance = 0.5e-6 * ARC_SECOND_TO_RAD * 1.0001;
        for _ in 0..PROPERTY_ITERATIONS {
            let angle = random.real(-TWO_PI, TWO_PI);
            let text = angles::format_dms(angle, 6).unwrap();
            let parsed = parse_dms(&text);
            assert!(parsed.is_ok(), "{text}");
            near(parsed.unwrap(), angle, tolerance);
        }
    }

    // ---- bearings ------------------------------------------------------------

    #[test]
    fn azimuth_to_bearing_by_quadrant() {
        // NE: bearing = azimuth; SE: 180 - az; SW: az - 180; NW: 360 - az.
        let ne = azimuth_to_bearing(30.0 * DEG_TO_RAD).unwrap();
        assert_eq!(ne.meridian, BearingMeridian::North);
        assert_eq!(ne.side, BearingSide::East);
        near(ne.angle, 30.0 * DEG_TO_RAD, 1e-15);

        let se = azimuth_to_bearing(135.0 * DEG_TO_RAD).unwrap();
        assert_eq!(se.meridian, BearingMeridian::South);
        assert_eq!(se.side, BearingSide::East);
        near(se.angle, 45.0 * DEG_TO_RAD, 1e-15);

        let sw = azimuth_to_bearing(192.5 * DEG_TO_RAD).unwrap();
        assert_eq!(sw.meridian, BearingMeridian::South);
        assert_eq!(sw.side, BearingSide::West);
        near(sw.angle, 12.5 * DEG_TO_RAD, 1e-15);

        let nw = azimuth_to_bearing(315.0 * DEG_TO_RAD).unwrap();
        assert_eq!(nw.meridian, BearingMeridian::North);
        assert_eq!(nw.side, BearingSide::West);
        near(nw.angle, 45.0 * DEG_TO_RAD, 1e-15);

        assert!(azimuth_to_bearing(f64::NAN).is_err());
    }

    #[test]
    fn bearing_on_the_axes() {
        assert_eq!(format_bearing(0.0, 0).unwrap(), "N 0\u{B0}00'00\" E");
        assert_eq!(format_bearing(HALF_PI, 0).unwrap(), "N 90\u{B0}00'00\" E");
        assert_eq!(format_bearing(PI, 0).unwrap(), "S 0\u{B0}00'00\" E");
        assert_eq!(format_bearing(1.5 * PI, 0).unwrap(), "S 90\u{B0}00'00\" W");
        // N 0 W is due north: azimuth 0, not 2*pi.
        double_eq(
            bearing_to_azimuth(QuadrantBearing {
                meridian: BearingMeridian::North,
                angle: 0.0,
                side: BearingSide::West,
            })
            .unwrap(),
            0.0,
        );
    }

    #[test]
    fn format_and_parse_bearing() {
        assert_eq!(
            format_bearing(DMS_453015, 0).unwrap(),
            "N 45\u{B0}30'15\" E"
        );
        assert_eq!(
            format_bearing(135.0 * DEG_TO_RAD, 0).unwrap(),
            "S 45\u{B0}00'00\" E"
        );
        assert_eq!(
            format_bearing(225.0 * DEG_TO_RAD, 0).unwrap(),
            "S 45\u{B0}00'00\" W"
        );
        assert_eq!(
            format_bearing(315.0 * DEG_TO_RAD, 1).unwrap(),
            "N 45\u{B0}00'00.0\" W"
        );

        near(
            parse_bearing("N 45\u{B0}30'15\" E").unwrap(),
            DMS_453015,
            1e-15,
        );
        near(
            parse_bearing("S 12\u{B0}30'00\" W").unwrap(),
            192.5 * DEG_TO_RAD,
            1e-15,
        );
        near(
            parse_bearing("s45-00-00e").unwrap(),
            135.0 * DEG_TO_RAD,
            1e-15,
        );
        near(
            parse_bearing("N 10.5 W").unwrap(),
            349.5 * DEG_TO_RAD,
            1e-15,
        );
        // exactly 90 is allowed
        near(parse_bearing("N 90 E").unwrap(), HALF_PI, 1e-15);
    }

    #[test]
    fn parse_bearing_rejects_malformed_text() {
        for text in [
            "",
            "N",
            "NE",
            "N 91 E",
            "X 10 E",
            "N 10 Q",
            "N -10 E",
            "N E",
            "45 30 15",
            "N 45 60 00 E",
        ] {
            let parsed = parse_bearing(text);
            assert!(parsed.is_err(), "'{text}'");
            assert_eq!(parsed.unwrap_err().code, ErrorCode::ParseFailure, "{text}");
        }
        assert!(
            bearing_to_azimuth(QuadrantBearing {
                meridian: BearingMeridian::North,
                angle: 2.0,
                side: BearingSide::East,
            })
            .is_err()
        );
        assert!(
            bearing_to_azimuth(QuadrantBearing {
                meridian: BearingMeridian::North,
                angle: -0.1,
                side: BearingSide::East,
            })
            .is_err()
        );
    }

    #[test]
    fn property_bearing_round_trip() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let azimuth = random.real(0.0, TWO_PI);
            let bearing = azimuth_to_bearing(azimuth).unwrap();
            assert!(bearing.angle >= 0.0);
            assert!(bearing.angle <= HALF_PI);
            let back = bearing_to_azimuth(bearing).unwrap();
            // One subtraction from pi or 2*pi each way: a few ulp of 2*pi.
            near(back, azimuth, 4e-15);
        }
    }
}
