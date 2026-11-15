//! The twins of tests/geodesy/test_units.cpp.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]

pub mod support;

use katana_core::ErrorCode;
use katana_geodesy::units::{
    self, abbreviation, convert_angle, convert_length, length_unit_from_metres_per_unit,
    metres_per_unit, parse_angle_unit, parse_length_unit, radians_per_unit,
};
use katana_geodesy::{AngleUnit, LengthUnit};
use katana_math::numerics::{DEG_TO_RAD, PI, RAD_TO_DEG, nearly_equal, tolerance};

use support::{PROPERTY_ITERATIONS, Random, assert_err, double_eq, near};

const ALL_LENGTH_UNITS: [LengthUnit; 15] = [
    LengthUnit::Millimetre,
    LengthUnit::Centimetre,
    LengthUnit::Metre,
    LengthUnit::Kilometre,
    LengthUnit::InternationalInch,
    LengthUnit::InternationalFoot,
    LengthUnit::InternationalYard,
    LengthUnit::InternationalChain,
    LengthUnit::InternationalLink,
    LengthUnit::InternationalMile,
    LengthUnit::NauticalMile,
    LengthUnit::UsSurveyFoot,
    LengthUnit::UsSurveyChain,
    LengthUnit::UsSurveyLink,
    LengthUnit::UsSurveyMile,
];

const ALL_ANGLE_UNITS: [AngleUnit; 5] = [
    AngleUnit::Radian,
    AngleUnit::Degree,
    AngleUnit::Gon,
    AngleUnit::ArcMinute,
    AngleUnit::ArcSecond,
];

// A conversion is (value * p) / q: two correctly rounded operations, each off
// by at most half a unit in the last place. A round trip is four. This bound
// is derived from IEEE-754, it is not a tolerance.
fn round_trip_bound(value: f64) -> f64 {
    4.0 * f64::EPSILON * value.abs()
}

fn pick<T: Copy, const N: usize>(random: &mut Random, values: &[T; N]) -> T {
    values[random.integer(0, N as i32 - 1) as usize]
}

// ---- defining constants (legal definitions, not computed) -------------------------

mod geodesy_units {
    use super::*;

    #[test]
    fn defining_constants_are_exact() {
        // International yard and pound agreement, 1959: 1 ft = 0.3048 m exactly.
        assert_eq!(metres_per_unit(LengthUnit::InternationalFoot), 0.3048);
        assert_eq!(metres_per_unit(LengthUnit::InternationalInch), 0.0254);
        assert_eq!(metres_per_unit(LengthUnit::InternationalYard), 0.9144);
        assert_eq!(metres_per_unit(LengthUnit::InternationalMile), 1609.344);
        assert_eq!(metres_per_unit(LengthUnit::InternationalChain), 20.1168);
        assert_eq!(metres_per_unit(LengthUnit::InternationalLink), 0.201168);
        assert_eq!(metres_per_unit(LengthUnit::NauticalMile), 1852.0);
        // Mendenhall order, 1893: 1 m = 39.37 in, hence 1 ftUS = 1200/3937 m exactly.
        assert_eq!(metres_per_unit(LengthUnit::UsSurveyFoot), 1200.0 / 3937.0);
        assert_eq!(metres_per_unit(LengthUnit::UsSurveyChain), 79200.0 / 3937.0);
        assert_eq!(metres_per_unit(LengthUnit::Metre), 1.0);
    }

    #[test]
    fn exact_conversions_of_round_values() {
        // 3937 US survey feet are exactly 1200 m, and 1200 m exactly 3937 ftUS:
        // the Mendenhall Order of 1893 defines 1 m = 39.37 in, i.e. 1 ftUS =
        // 1200/3937 m. Both directions evaluate as (value * p) / q over the
        // integers, so the 3937 cancels and the result is exact, not merely
        // close.
        assert_eq!(
            convert_length(3937.0, LengthUnit::UsSurveyFoot, LengthUnit::Metre),
            1200.0
        );
        assert_eq!(
            convert_length(1200.0, LengthUnit::Metre, LengthUnit::UsSurveyFoot),
            3937.0
        );
        assert_eq!(
            convert_length(2000000.0, LengthUnit::InternationalFoot, LengthUnit::Metre),
            609600.0
        );
        assert_eq!(
            convert_length(
                1.0,
                LengthUnit::InternationalChain,
                LengthUnit::InternationalFoot
            ),
            66.0
        );
        assert_eq!(
            convert_length(1.0, LengthUnit::UsSurveyChain, LengthUnit::UsSurveyFoot),
            66.0
        );
        assert_eq!(
            convert_length(
                100.0,
                LengthUnit::InternationalLink,
                LengthUnit::InternationalChain
            ),
            1.0
        );
        assert_eq!(
            convert_length(
                1.0,
                LengthUnit::InternationalMile,
                LengthUnit::InternationalFoot
            ),
            5280.0
        );
        assert_eq!(
            convert_length(1.0, LengthUnit::UsSurveyMile, LengthUnit::UsSurveyFoot),
            5280.0
        );
        assert_eq!(
            convert_length(2.5, LengthUnit::Kilometre, LengthUnit::Millimetre),
            2500000.0
        );
    }

    // The classic state-plane blunder. The two feet differ by exactly 2 parts
    // per million: ftUS / ft = (1200/3937) / 0.3048 = 1.000002000004. A
    // coordinate of 2 000 000 ft read in the wrong foot is wrong by 4 ft
    // (1.2 m).
    #[test]
    fn us_survey_foot_differs_from_international_foot_by_two_parts_per_million() {
        let ratio = metres_per_unit(LengthUnit::UsSurveyFoot)
            / metres_per_unit(LengthUnit::InternationalFoot);
        // Exact value 1 + 2e-6 + 4e-12 + ...; nearly_equal's 1e-9 resolves the 2e-6.
        assert!(nearly_equal(ratio, 1.000002000004, tolerance::RELATIVE));
        assert!(ratio > 1.0);

        let state_plane_feet = 2000000.0;
        let as_survey_feet = convert_length(
            state_plane_feet,
            LengthUnit::UsSurveyFoot,
            LengthUnit::Metre,
        );
        let as_international_feet = convert_length(
            state_plane_feet,
            LengthUnit::InternationalFoot,
            LengthUnit::Metre,
        );
        let blunder_metres = as_survey_feet - as_international_feet;
        let blunder_feet = convert_length(
            blunder_metres,
            LengthUnit::Metre,
            LengthUnit::InternationalFoot,
        );

        // By hand: 2e6 * (1200/3937 - 0.3048) m = 2e6 * 6.096012192e-7 m
        //        = 1.2192024384 m = 4.000008000 ft.
        near(blunder_metres, 1.2192024384, tolerance::GEOMETRIC);
        near(blunder_feet, 4.000008, tolerance::GEOMETRIC);
    }

    #[test]
    fn same_unit_conversion_is_bit_exact() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let value = random.real(-1e8, 1e8);
            for unit in ALL_LENGTH_UNITS {
                assert_eq!(convert_length(value, unit, unit), value);
            }
            for unit in ALL_ANGLE_UNITS {
                assert_eq!(convert_angle(value, unit, unit), value);
            }
        }
    }

    // ---- angles -------------------------------------------------------------------------

    #[test]
    fn angle_conversions_between_turn_fractions_are_exact() {
        assert_eq!(
            convert_angle(90.0, AngleUnit::Degree, AngleUnit::Gon),
            100.0
        );
        assert_eq!(
            convert_angle(400.0, AngleUnit::Gon, AngleUnit::Degree),
            360.0
        );
        assert_eq!(
            convert_angle(1.0, AngleUnit::Degree, AngleUnit::ArcSecond),
            3600.0
        );
        assert_eq!(
            convert_angle(1.0, AngleUnit::Degree, AngleUnit::ArcMinute),
            60.0
        );
        assert_eq!(
            convert_angle(1296000.0, AngleUnit::ArcSecond, AngleUnit::Gon),
            400.0
        );
        // NTF Lambert origin.
        assert_eq!(convert_angle(52.0, AngleUnit::Gon, AngleUnit::Degree), 46.8);
    }

    #[test]
    fn radian_conversions_match_the_central_constants_bit_for_bit() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let value = random.real(-720.0, 720.0);
            assert_eq!(
                convert_angle(value, AngleUnit::Degree, AngleUnit::Radian),
                value * DEG_TO_RAD
            );
            assert_eq!(
                convert_angle(value, AngleUnit::Radian, AngleUnit::Degree),
                value * RAD_TO_DEG
            );
        }
        assert_eq!(radians_per_unit(AngleUnit::Degree), DEG_TO_RAD);
        assert_eq!(radians_per_unit(AngleUnit::Radian), 1.0);
        // 180 * (pi/180) need not round to pi exactly: compare to 4 ulp (gtest).
        double_eq(
            convert_angle(180.0, AngleUnit::Degree, AngleUnit::Radian),
            PI,
        );
        double_eq(convert_angle(200.0, AngleUnit::Gon, AngleUnit::Radian), PI);
    }

    // ---- parsing ------------------------------------------------------------------------

    #[test]
    fn parses_names_symbols_and_proj_ids() {
        let expect_length = |text: &str, expected: LengthUnit| match parse_length_unit(text) {
            Ok(parsed) => assert_eq!(parsed, expected, "'{text}'"),
            Err(error) => panic!("'{text}': {}", error.describe()),
        };
        expect_length("m", LengthUnit::Metre);
        expect_length("metre", LengthUnit::Metre);
        expect_length("Meters", LengthUnit::Metre);
        expect_length("  km ", LengthUnit::Kilometre);
        expect_length("ft", LengthUnit::InternationalFoot);
        expect_length("foot", LengthUnit::InternationalFoot);
        expect_length("International Foot", LengthUnit::InternationalFoot);
        expect_length("ftUS", LengthUnit::UsSurveyFoot);
        expect_length("us-ft", LengthUnit::UsSurveyFoot); // PROJ unit id
        expect_length("US survey foot", LengthUnit::UsSurveyFoot);
        expect_length("Foot_US", LengthUnit::UsSurveyFoot); // ESRI WKT
        expect_length("ch", LengthUnit::InternationalChain);
        expect_length("us-ch", LengthUnit::UsSurveyChain);
        expect_length("link", LengthUnit::InternationalLink);
        expect_length("kmi", LengthUnit::NauticalMile);

        let expect_angle = |text: &str, expected: AngleUnit| match parse_angle_unit(text) {
            Ok(parsed) => assert_eq!(parsed, expected, "'{text}'"),
            Err(error) => panic!("'{text}': {}", error.describe()),
        };
        expect_angle("rad", AngleUnit::Radian);
        expect_angle("Degree", AngleUnit::Degree);
        expect_angle("gon", AngleUnit::Gon);
        expect_angle("grad", AngleUnit::Gon);
        expect_angle("arc-second", AngleUnit::ArcSecond);
        expect_angle("arcmin", AngleUnit::ArcMinute);
    }

    #[test]
    fn foot_never_means_us_survey_foot() {
        // "ft" is the international foot, as in EPSG and PROJ. Silently reading
        // it as the survey foot (or vice versa) is the 2 ppm blunder tested
        // above.
        assert_eq!(
            parse_length_unit("ft").unwrap(),
            LengthUnit::InternationalFoot
        );
        assert_eq!(
            parse_length_unit("feet").unwrap(),
            LengthUnit::InternationalFoot
        );
        assert_ne!(
            parse_length_unit("ftUS").unwrap(),
            parse_length_unit("ft").unwrap()
        );
    }

    #[test]
    fn unknown_unit_is_an_error_never_a_silent_factor_of_one() {
        for text in ["", "   ", "furlong", "metres per second", "f t x", "1"] {
            let length = assert_err(parse_length_unit(text));
            assert_eq!(length.code, ErrorCode::ParseFailure, "'{text}'");
            assert!(length.context.contains(text), "'{text}'");

            let angle = assert_err(parse_angle_unit(text));
            assert_eq!(angle.code, ErrorCode::ParseFailure, "'{text}'");
        }
        // A length name is not an angle and vice versa.
        assert!(parse_angle_unit("m").is_err());
        assert!(parse_length_unit("deg").is_err());
    }

    #[test]
    fn names_and_abbreviations_parse_back_to_the_same_unit() {
        for unit in ALL_LENGTH_UNITS {
            let from_name = parse_length_unit(units::to_string(unit));
            assert_eq!(from_name.ok(), Some(unit), "{}", units::to_string(unit));
            let from_symbol = parse_length_unit(abbreviation(unit));
            assert_eq!(from_symbol.ok(), Some(unit), "{}", abbreviation(unit));
        }
        for unit in ALL_ANGLE_UNITS {
            let from_name = parse_angle_unit(units::to_string(unit));
            assert_eq!(from_name.ok(), Some(unit), "{}", units::to_string(unit));
            let from_symbol = parse_angle_unit(abbreviation(unit));
            assert_eq!(from_symbol.ok(), Some(unit), "{}", abbreviation(unit));
        }
    }

    #[test]
    fn factor_lookup_separates_the_two_feet() {
        assert_eq!(
            length_unit_from_metres_per_unit(0.3048),
            Some(LengthUnit::InternationalFoot)
        );
        // The 15-digit value stored in the EPSG registry for the US survey foot.
        assert_eq!(
            length_unit_from_metres_per_unit(0.304800609601219),
            Some(LengthUnit::UsSurveyFoot)
        );
        assert_eq!(
            length_unit_from_metres_per_unit(1.0),
            Some(LengthUnit::Metre)
        );
        // Clarke's foot.
        assert_eq!(length_unit_from_metres_per_unit(0.3047972654), None);
        assert_eq!(length_unit_from_metres_per_unit(f64::NAN), None);
    }
}

mod geodesy_units_property {
    use super::*;

    #[test]
    fn length_conversions_round_trip() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let value = random.real(-1e8, 1e8);
            let from = pick(&mut random, &ALL_LENGTH_UNITS);
            let to = pick(&mut random, &ALL_LENGTH_UNITS);
            let back = convert_length(convert_length(value, from, to), to, from);
            assert!(
                (back - value).abs() <= round_trip_bound(value),
                "{} -> {} value={value:?}",
                units::to_string(from),
                units::to_string(to)
            );
        }
    }

    #[test]
    fn length_conversions_are_transitive_through_metres() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let value = random.real(-1e7, 1e7);
            let from = pick(&mut random, &ALL_LENGTH_UNITS);
            let to = pick(&mut random, &ALL_LENGTH_UNITS);
            let direct = convert_length(value, from, to);
            let via_metres = convert_length(
                convert_length(value, from, LengthUnit::Metre),
                LengthUnit::Metre,
                to,
            );
            assert!((direct - via_metres).abs() <= round_trip_bound(direct));
        }
    }

    #[test]
    fn angle_conversions_round_trip() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let value = random.real(-1e6, 1e6);
            let from = pick(&mut random, &ALL_ANGLE_UNITS);
            let to = pick(&mut random, &ALL_ANGLE_UNITS);
            let back = convert_angle(convert_angle(value, from, to), to, from);
            assert!(
                (back - value).abs() <= round_trip_bound(value),
                "{} -> {} value={value:?}",
                units::to_string(from),
                units::to_string(to)
            );
        }
    }
}
