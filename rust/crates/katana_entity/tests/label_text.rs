//! The twin of tests/entity/annotation/test_label_text.cpp: the label
//! template language and its number formats (docs/annotation.md,
//! "Templates").
//!
//! Every expected string is worked out by hand from the stated rules: a
//! bearing of atan2(3, 4) is 36.8698976...deg = 36 52' 11.63", a 10 m by
//! 12.5 m lot is 125 m2 = 0.0125 ha. A label is a number somebody signs.

use katana_entity::LabelKind;
use katana_entity::label_text::{
    LabelQuantity, LabelValue, LabelValues, check_label_template, format_chainage, format_dm,
    format_dms, format_fixed, format_label, format_quadrant_bearing, label_value_names,
};
use katana_math::numerics::DEG_TO_RAD;

fn dms(degrees: f64, minutes: f64, seconds: f64) -> f64 {
    (degrees + minutes / 60.0 + seconds / 3600.0) * DEG_TO_RAD
}

fn segment_values() -> LabelValues {
    let mut values = LabelValues::new();
    values.insert(
        String::from("bearing"),
        LabelValue::of(LabelQuantity::Bearing, 3.0_f64.atan2(4.0)),
    );
    values.insert(
        String::from("distance"),
        LabelValue::of(LabelQuantity::Length, 5.0),
    );
    values.insert(String::from("code"), LabelValue::of_text("bdy"));
    values
}

mod label_text {
    use super::*;

    #[test]
    fn bearing_and_distance_in_the_survey_form() {
        // Bearing of the 3-4-5 line from north: atan(3/4) = 36.869897646 deg
        // = 36 deg 52' 11.6315" -> whole seconds 12.
        assert_eq!(
            format_label("{bearing:dms} {distance:.3f}", &segment_values()),
            "36\u{b0}52'12\" 5.000"
        );
        assert_eq!(
            format_label("{bearing:dms.2}", &segment_values()),
            "36\u{b0}52'11.63\""
        );
        assert_eq!(
            format_label("{bearing:dm}", &segment_values()),
            "36\u{b0}52'"
        );
        // With no format step a bearing is whole-second DMS and a length 3
        // decimals.
        assert_eq!(
            format_label("{bearing} {distance}", &segment_values()),
            "36\u{b0}52'12\" 5.000"
        );
        assert_eq!(
            format_label("{bearing:qb}", &segment_values()),
            "N 36\u{b0}52'12\" E"
        );
        assert_eq!(
            format_label("{bearing:deg:.4f}", &segment_values()),
            "36.8699"
        );
    }

    #[test]
    fn unit_steps_convert_before_the_number_format() {
        let mut values = LabelValues::new();
        values.insert(
            String::from("area"),
            LabelValue::of(LabelQuantity::Area, 125.0),
        );
        values.insert(
            String::from("distance"),
            LabelValue::of(LabelQuantity::Length, 1.2345),
        );
        assert_eq!(format_label("{area:m2:.1f} m2", &values), "125.0 m2");
        assert_eq!(format_label("{area:ha:.4f} ha", &values), "0.0125 ha");
        // 125 / 4046.8564224 = 0.030888...
        assert_eq!(format_label("{area:ac:.3f}", &values), "0.031");
        assert_eq!(
            format_label("{distance:mm:.0f} mm", &values),
            "1235 mm",
            "1234.5 rounds half away from zero"
        );
        assert_eq!(format_label("{distance:ft:.2f}", &values), "4.05"); // 1.2345 / 0.3048 = 4.0502
        assert_eq!(
            format_label("{area}", &values),
            "125.0",
            "an area's own format is 1 decimal"
        );
    }

    #[test]
    fn a_line_with_an_absent_value_is_dropped_not_printed_as_zero() {
        let mut values = LabelValues::new();
        values.insert(String::from("point"), LabelValue::of_text("1001"));
        // No "z": the point has no level.
        assert_eq!(format_label("{point}\nRL {z:.3f}", &values), "1001");
        assert_eq!(
            format_label("RL {z:.3f}", &values),
            "",
            "every line dropped: no label at all"
        );
        values.insert(
            String::from("z"),
            LabelValue::of(LabelQuantity::Number, 31.2495),
        );
        assert_eq!(
            format_label("{point}\nRL {z:.3f}", &values),
            "1001\nRL 31.250"
        );
    }

    #[test]
    fn braces_are_escaped_by_doubling() {
        assert_eq!(format_label("{{{code}}}", &segment_values()), "{bdy}");
        assert_eq!(format_label("{code:upper}", &segment_values()), "BDY");
    }

    #[test]
    fn templates_are_checked_for_the_kind_they_label() {
        assert!(check_label_template("{bearing:dms} {distance:.3f}", LabelKind::Segment).is_ok());
        assert!(
            check_label_template("{area:m2:.1f} m2\n{area:ha:.4f} ha", LabelKind::Area).is_ok()
        );
        assert!(check_label_template("RL {z:.3f}", LabelKind::Point).is_ok());
        assert!(check_label_template("{chainage:ch}", LabelKind::Chainage).is_ok());
        assert!(check_label_template("{prop.owner:upper}", LabelKind::Area).is_ok());

        assert!(
            check_label_template("{area}", LabelKind::Segment).is_err(),
            "a segment has no area"
        );
        assert!(
            check_label_template("{distance:dms}", LabelKind::Segment).is_err(),
            "dms applies to an angle, not a length"
        );
        assert!(
            check_label_template("{bearing:.3f}", LabelKind::Segment).is_err(),
            "an angle needs deg, rad or gon before a number format"
        );
        assert!(
            check_label_template("{distance:.3f:mm}", LabelKind::Segment).is_err(),
            "the number format is the last step"
        );
        assert!(check_label_template("{distance", LabelKind::Segment).is_err());
        assert!(check_label_template("distance}", LabelKind::Segment).is_err());
        assert!(check_label_template("{}", LabelKind::Point).is_err());
    }

    #[test]
    fn dms_rounds_once_and_carries() {
        // 59.9996" at whole seconds is a carry into the minutes, never 60".
        assert_eq!(format_dms(dms(10.0, 59.0, 59.9996), 0), "11\u{b0}00'00\"");
        assert_eq!(
            format_dms(dms(0.0, 0.0, 0.5), 0),
            "0\u{b0}00'01\"",
            "half away from zero"
        );
        assert_eq!(
            format_dms(dms(359.0, 59.0, 59.96), 1),
            "360\u{b0}00'00.0\"",
            "an angle is not wrapped; a bearing is (below)"
        );
        assert_eq!(format_dms(-dms(12.0, 30.0, 0.0), 0), "-12\u{b0}30'00\"");
        assert_eq!(format_dm(dms(12.0, 30.0, 30.0)), "12\u{b0}31'");
    }

    #[test]
    fn quadrant_bearings_in_each_quadrant() {
        assert_eq!(
            format_quadrant_bearing(dms(45.0, 0.0, 0.0), 0),
            "N 45\u{b0}00'00\" E"
        );
        assert_eq!(
            format_quadrant_bearing(dms(135.0, 0.0, 0.0), 0),
            "S 45\u{b0}00'00\" E"
        );
        assert_eq!(
            format_quadrant_bearing(dms(200.0, 30.0, 0.0), 0),
            "S 20\u{b0}30'00\" W"
        );
        assert_eq!(
            format_quadrant_bearing(dms(315.0, 0.0, 0.0), 0),
            "N 45\u{b0}00'00\" W"
        );
    }

    #[test]
    fn chainage_splits_kilometres_after_rounding() {
        assert_eq!(format_chainage(1234.5, 3), "1+234.500");
        assert_eq!(format_chainage(20.0, 3), "0+020.000");
        assert_eq!(
            format_chainage(999.9996, 3),
            "1+000.000",
            "rounded first, then split"
        );
        assert_eq!(format_chainage(-12.0, 3), "-0+012.000");
        assert_eq!(format_chainage(12345.0, 1), "12+345.0");
    }

    #[test]
    fn fixed_never_prints_minus_zero() {
        assert_eq!(format_fixed(-0.0004, 3), "0.000");
        assert_eq!(format_fixed(-0.0006, 3), "-0.001");
        assert_eq!(format_fixed(2.5, 0), "3", "ties away from zero");
    }

    #[test]
    fn the_kinds_list_their_values() {
        let segment = label_value_names(LabelKind::Segment);
        assert!(segment.contains(&"bearing"));
        let area = label_value_names(LabelKind::Area);
        assert!(area.contains(&"area"));
        assert!(!area.contains(&"bearing"));
    }
}
