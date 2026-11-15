//! The twin of tests/entity/customisation/test_linework_codes.cpp: the
//! spellings of the linework control codes, where a customisation keeps them
//! (linework_codes.rs). What the controls DO is tested with the linework
//! itself, in the cad suite.

use katana_core::error::ErrorCode;
use katana_entity::linework_codes::linework_code_members;
use katana_entity::{LineworkCodes, validate};

mod linework_code_spellings {
    use super::*;

    #[test]
    fn the_defaults_are_the_seven_common_field_spellings_and_are_sound() {
        let codes = LineworkCodes::default();
        assert_eq!(codes.start, "ST");
        assert_eq!(codes.end, "END");
        assert_eq!(codes.close, "CL");
        assert_eq!(codes.arc_start, "BC");
        assert_eq!(codes.arc_end, "EC");
        assert_eq!(codes.join, "JPN");
        assert_eq!(codes.rectangle, "RECT");
        assert!(validate(&codes).is_ok());
    }

    #[test]
    fn the_seven_controls_are_listed_by_name_each_with_its_own_spelling() {
        let members = linework_code_members();
        let names: Vec<&str> = members.iter().map(|member| member.name).collect();
        assert_eq!(
            names,
            [
                "start",
                "end",
                "close",
                "arcStart",
                "arcEnd",
                "join",
                "rectangle"
            ]
        );

        // Each name reaches the spelling of that name and no other: written
        // through the list, read through the struct.
        let mut codes = LineworkCodes::default();
        for (i, member) in members.iter().enumerate() {
            *member.spelling_mut(&mut codes) = format!("X{i}");
        }
        assert_eq!(codes.start, "X0");
        assert_eq!(codes.end, "X1");
        assert_eq!(codes.close, "X2");
        assert_eq!(codes.arc_start, "X3");
        assert_eq!(codes.arc_end, "X4");
        assert_eq!(codes.join, "X5");
        assert_eq!(codes.rectangle, "X6");
    }

    #[test]
    fn two_controls_spelled_alike_are_refused_naming_both() {
        let same = LineworkCodes {
            close: String::from("END"), // the end's spelling
            ..LineworkCodes::default()
        };
        let alike = validate(&same).expect_err("two controls are spelled alike");
        assert_eq!(alike.code, ErrorCode::InvalidArgument);
        assert_eq!(alike.context, "end and close are \"END\"");

        // Tokens are matched ignoring ASCII case, so the spellings are compared
        // so.
        let folded = LineworkCodes {
            arc_end: String::from("bc"),
            ..LineworkCodes::default()
        };
        let ignoring_case = validate(&folded).expect_err("BC and bc are one token");
        assert_eq!(ignoring_case.context, "arcStart and arcEnd are \"BC\"");
    }

    #[test]
    fn a_spelling_with_a_blank_in_it_is_refused_naming_its_control() {
        let blank = LineworkCodes {
            rectangle: String::from("RE CT"),
            ..LineworkCodes::default()
        };
        let error = validate(&blank).expect_err("a blank in a spelling");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.context, "rectangle=\"RE CT\"");

        let tab = LineworkCodes {
            start: String::from("S\tT"),
            ..LineworkCodes::default()
        };
        assert!(
            validate(&tab).is_err(),
            "a tab splits a code as a space does"
        );
    }

    #[test]
    fn an_empty_spelling_switches_a_control_off_and_two_off_are_not_a_clash() {
        let off = LineworkCodes {
            join: String::new(),
            rectangle: String::new(),
            ..LineworkCodes::default()
        };
        assert!(validate(&off).is_ok());

        let mut none = LineworkCodes::default();
        for member in linework_code_members() {
            member.spelling_mut(&mut none).clear();
        }
        assert!(
            validate(&none).is_ok(),
            "every control off is a sound table"
        );
    }

    #[test]
    fn two_tables_are_equal_when_every_spelling_is() {
        assert_eq!(LineworkCodes::default(), LineworkCodes::default());
        for member in linework_code_members() {
            let mut changed = LineworkCodes::default();
            member.spelling_mut(&mut changed).push('X');
            assert_ne!(changed, LineworkCodes::default(), "{}", member.name);
        }
    }
}
