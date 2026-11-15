//! The twin of tests/entity/test_dimension_text.cpp: dimension label
//! formatting.
//!
//! Every expected string here is worked out by hand from the stated rules,
//! not captured from a run. This number ends up on a drawing somebody signs.
//!
//! DimensionText.TheTieDirectionSurvivesAChangeOfRoundingMode has no twin:
//! Rust offers no way to change the floating-point rounding mode (doing so
//! behind the compiler's back is undefined behaviour), and f64::round is
//! specified to round half away from zero regardless.

use katana_core::error::ErrorCode;
use katana_entity::dimension_text::{dimension_label, format_measurement};
use katana_entity::entity::to_string;
use katana_entity::tables::arrow_head_from_string;
use katana_entity::{ArrowHead, DimensionStyle, DimensionStyleDatabase, validate};

fn metres(decimals: i32) -> DimensionStyle {
    DimensionStyle {
        decimals,
        ..DimensionStyle::default()
    }
}

mod dimension_text {
    use super::*;

    #[test]
    fn fixes_the_number_of_decimals_and_pads_on_the_left() {
        let style = metres(3);
        assert_eq!(format_measurement(12.3456, &style), "12.346");
        assert_eq!(format_measurement(12.0, &style), "12.000");
        assert_eq!(
            format_measurement(0.05, &style),
            "0.050",
            "padded on the LEFT, not '0.5'"
        );
        assert_eq!(format_measurement(0.0, &style), "0.000");
        assert_eq!(format_measurement(0.0005, &style), "0.001");

        assert_eq!(format_measurement(12.3456, &metres(0)), "12");
        assert_eq!(format_measurement(12.6, &metres(0)), "13");
        assert_eq!(format_measurement(1.0 / 3.0, &metres(6)), "0.333333");
    }

    #[test]
    fn ties_go_away_from_zero_not_to_even() {
        // THE rule that rules out printf and the fixed formats, all of which
        // round ties to EVEN. A surveyor reading 2.5 at whole metres expects 3.
        assert_eq!(
            format_measurement(2.5, &metres(0)),
            "3",
            "round-half-even would give 2"
        );
        assert_eq!(format_measurement(3.5, &metres(0)), "4");
        assert_eq!(
            format_measurement(0.5, &metres(0)),
            "1",
            "round-half-even would give 0"
        );
        assert_eq!(format_measurement(1.5, &metres(0)), "2");

        // 0.125 and 0.375 are EXACTLY representable, so these really are ties.
        assert_eq!(
            format_measurement(0.125, &metres(2)),
            "0.13",
            "round-half-even would give 0.12"
        );
        assert_eq!(format_measurement(0.375, &metres(2)), "0.38");
    }

    #[test]
    fn scales_before_it_rounds() {
        // DIMLFAC then DIMRND, and the order changes the answer. 1.24 m scaled
        // to millimetres is 1240; rounding THAT to a multiple of 0.05 does
        // nothing, so the label is 1240.00 mm.
        let mut style = metres(2);
        style.unit_scale = 1000.0;
        style.round_to = 0.05;
        style.suffix = String::from(" mm");
        assert_eq!(format_measurement(1.24, &style), "1240.00 mm");

        // And with the scale at 1 the same rounding does bite.
        style.unit_scale = 1.0;
        style.suffix = String::from(" m");
        assert_eq!(format_measurement(1.24, &style), "1.25 m");
        assert_eq!(format_measurement(1.22, &style), "1.20 m");
        assert_eq!(format_measurement(1.26, &style), "1.25 m");
    }

    #[test]
    fn rounding_to_a_multiple_ties_away_from_zero_too() {
        let mut style = metres(3);
        style.round_to = 0.5;
        // 1.25 is exactly halfway between 1.0 and 1.5 in units of 0.5.
        assert_eq!(
            format_measurement(1.25, &style),
            "1.500",
            "away from zero, not to even"
        );
        assert_eq!(format_measurement(0.75, &style), "1.000");
        assert_eq!(format_measurement(1.74, &style), "1.500");
        assert_eq!(format_measurement(1.76, &style), "2.000");
    }

    #[test]
    fn carries_into_the_whole_part_rather_than_growing_a_third_digit() {
        // 0.9999 at two decimals rounds to 100 hundredths, which is a carry.
        assert_eq!(format_measurement(0.9999, &metres(2)), "1.00");
        assert_eq!(format_measurement(9.999, &metres(2)), "10.00");
        assert_eq!(format_measurement(99.9999, &metres(3)), "100.000");
        assert_eq!(format_measurement(0.99999, &metres(4)), "1.0000");
    }

    #[test]
    fn trailing_zero_suppression_never_leaves_a_bare_point_or_nothing() {
        let mut style = metres(3);
        style.suppress_trailing_zeros = true;

        assert_eq!(format_measurement(12.0, &style), "12", "not '12.'");
        assert_eq!(format_measurement(0.0, &style), "0", "not '' and not '.'");
        assert_eq!(format_measurement(12.5, &style), "12.5");
        assert_eq!(format_measurement(12.500, &style), "12.5");
        assert_eq!(format_measurement(0.050, &style), "0.05");
        assert_eq!(
            format_measurement(100.0, &style),
            "100",
            "the zeros BEFORE the point stay"
        );

        // Off by default, because a dimension usually wants a fixed number of
        // decimals so a column of them lines up.
        assert_eq!(format_measurement(12.0, &metres(3)), "12.000");
    }

    #[test]
    fn negative_zero_never_appears_on_the_drawing() {
        let mut style = metres(3);
        assert_eq!(format_measurement(-0.0, &style), "0.000");

        style.suppress_trailing_zeros = true;
        assert_eq!(format_measurement(-0.0, &style), "0");
        assert_eq!(
            format_measurement(-0.0001, &style),
            "0",
            "rounds to zero, so no sign"
        );

        // A genuine negative still carries its sign.
        assert_eq!(format_measurement(-1.5, &style), "-1.5");
    }

    #[test]
    fn prefix_and_suffix_wrap_the_number() {
        let mut style = metres(2);
        style.prefix = String::from("R");
        style.suffix = String::from(" m");
        assert_eq!(format_measurement(2.5, &style), "R2.50 m");

        style.prefix.clear();
        style.unit_scale = 1000.0;
        style.decimals = 0;
        style.suffix = String::from(" mm");
        assert_eq!(format_measurement(2.5, &style), "2500 mm");
    }

    #[test]
    fn an_override_wins_verbatim() {
        // DXF does not apply DIMPOST to overridden text, and someone who typed
        // a value means that value.
        let mut style = metres(2);
        style.prefix = String::from("R");
        style.suffix = String::from(" m");
        style.unit_scale = 1000.0;
        style.round_to = 10.0;

        assert_eq!(dimension_label(2.5, "TYP", &style), "TYP");
        assert_eq!(dimension_label(2.5, "2.5 m EXACT", &style), "2.5 m EXACT");
        assert_eq!(
            dimension_label(2.5, "", &style),
            "R2500.00 m",
            "no override: the style applies"
        );
    }

    #[test]
    fn a_rounding_step_can_legitimately_erase_a_small_measurement() {
        // The chosen precision doing its job, pinned rather than left to be
        // discovered.
        let mut style = metres(3);
        style.round_to = 0.05;
        assert_eq!(
            format_measurement(0.0004, &style),
            "0.000",
            "a 0.4 mm gap rounded to 50 mm steps is zero"
        );
        assert_eq!(
            format_measurement(0.0004, &metres(0)),
            "0",
            "and so is a 0.4 mm gap shown to whole metres"
        );
    }

    #[test]
    fn large_and_extreme_values_do_not_produce_nonsense() {
        let style = metres(3);
        assert_eq!(format_measurement(1234567.891, &style), "1234567.891");
        // Past 2^53 a double has no fraction left, so the value is emitted
        // whole rather than with invented decimals.
        let huge = format_measurement(1.0e17, &style);
        assert!(!huge.contains('.'), "{huge}");
        assert_eq!(huge, "100000000000000000");

        // Non-finite cannot reach here from a validated entity, but printing
        // "inf" or "nan" as though it were a length would be worse.
        let mut marked = style.clone();
        marked.suffix = String::from(" m");
        assert_eq!(format_measurement(f64::INFINITY, &marked), "? m");
        assert_eq!(format_measurement(f64::NAN, &marked), "? m");
    }

    // A decimal comma would make a drawing read as a different number. The
    // C++ sets a German locale where one is installed; Rust's formatting has
    // no locale at all, so the values are asserted as they are.
    #[test]
    fn the_output_is_locale_independent() {
        assert_eq!(
            format_measurement(1.5, &metres(1)),
            "1.5",
            "a comma here would be a defect"
        );
        assert_eq!(
            format_measurement(1234.5, &metres(1)),
            "1234.5",
            "and no thousands separator either"
        );
    }
}

// ---- the style table ----------------------------------------------------------------

mod dimension_style_table {
    use super::*;

    fn refuses(mut bad: DimensionStyle, expect_in_message: &str) {
        bad.name = String::from("site");
        let error = validate(&bad).expect_err(expect_in_message);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.describe().contains(expect_in_message),
            "{}",
            error.describe()
        );
    }

    // The C++ also refuses a suffix of CP1252 bytes, which a String cannot
    // hold.
    #[test]
    fn rejects_each_bad_field_with_its_own_reason() {
        let style = DimensionStyle {
            name: String::from("site"),
            ..DimensionStyle::default()
        };
        assert!(validate(&style).is_ok());

        refuses(
            DimensionStyle {
                text_height: 0.0,
                ..style.clone()
            },
            "text height",
        );
        refuses(
            DimensionStyle {
                arrow_size: -1.0,
                ..style.clone()
            },
            "arrow size",
        );
        refuses(
            DimensionStyle {
                unit_scale: 0.0,
                ..style.clone()
            },
            "unit scale",
        );
        refuses(
            DimensionStyle {
                text_gap: -0.1,
                ..style.clone()
            },
            "text gap",
        );
        refuses(
            DimensionStyle {
                decimals: 13,
                ..style.clone()
            },
            "decimals",
        );
        refuses(
            DimensionStyle {
                round_to: -1.0,
                ..style.clone()
            },
            "rounding step",
        );
        // No new epsilon: rounding finer than the geometric tolerance is
        // rounding to noise, and a tiny step overflows value/step before it
        // rounds.
        refuses(
            DimensionStyle {
                round_to: 1.0e-12,
                ..style
            },
            "geometric tolerance",
        );
    }

    #[test]
    fn always_holds_standard_and_will_not_give_it_up() {
        let mut styles = DimensionStyleDatabase::new();
        assert_eq!(styles.size(), 1);
        assert!(styles.find("Standard").is_some());

        assert!(styles.remove("Standard").is_err());
        assert!(styles.contains("Standard"));

        // Standard can be RE-STYLED, which is how a job sets its own defaults.
        let mut restyled = styles.find("Standard").unwrap().clone();
        restyled.decimals = 2;
        restyled.suffix = String::from(" m");
        assert!(styles.update(&restyled).is_ok());
        assert_eq!(styles.find("Standard").unwrap().decimals, 2);

        // And an invalid update is refused without damaging the stored one.
        let mut broken = restyled.clone();
        broken.text_height = -1.0;
        assert!(styles.update(&broken).is_err());
        assert_eq!(
            styles.find("Standard").unwrap().text_height,
            restyled.text_height
        );
    }

    #[test]
    fn add_update_and_remove_behave_like_the_other_named_tables() {
        let mut styles = DimensionStyleDatabase::new();

        let site = DimensionStyle {
            name: String::from("site"),
            decimals: 2,
            ..DimensionStyle::default()
        };
        assert!(styles.add(site.clone()).is_ok());
        assert_eq!(styles.add(site).unwrap_err().code, ErrorCode::AlreadyExists);
        let missing = DimensionStyle {
            name: String::from("missing"),
            ..DimensionStyle::default()
        };
        assert_eq!(
            styles.update(&missing).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            styles.remove("missing").unwrap_err().code,
            ErrorCode::NotFound
        );

        // Ascending by name, like every other table, so iteration is
        // deterministic.
        assert_eq!(styles.names(), vec!["Standard", "site"]);

        let removed = styles.remove("site").unwrap();
        assert_eq!(removed.decimals, 2);
        assert_eq!(styles.size(), 1);

        styles.reset();
        assert_eq!(styles.size(), 1);
        assert!(styles.contains("Standard"));
    }

    #[test]
    fn arrow_head_names_round_trip() {
        for head in [
            ArrowHead::None,
            ArrowHead::Tick,
            ArrowHead::ClosedFilled,
            ArrowHead::Open,
            ArrowHead::Dot,
        ] {
            let name = to_string(head);
            assert_ne!(name, "Unknown");
            assert_eq!(arrow_head_from_string(name).unwrap(), head, "{name}");
        }
        // Case insensitive, because an exchange format spells these in
        // capitals.
        assert!(arrow_head_from_string("CLOSEDFILLED").is_ok());
        assert!(arrow_head_from_string("Diamond").is_err());
    }
}
