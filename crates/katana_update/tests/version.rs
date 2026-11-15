//! Version text and its order (the twin of tests/update/test_version.cpp).
//!
//! Every expectation is worked out from the rule the module states and from the arithmetic of the
//! numbers, never read off the parser: "0.10" is above "0.9" because 10 > 9; a component of nine
//! digits is the largest that fits 32 bits because 999,999,999 is below 2^32 = 4,294,967,296 and
//! a tenth digit could reach 9,999,999,999, above it.

mod update_version {
    use katana_core::ErrorCode;
    use katana_update::version::{self, Version, detail, parse_version};
    use std::cmp::Ordering;

    fn parsed(text: &str) -> Version {
        match parse_version(text) {
            Ok(version) => version,
            Err(error) => panic!("{text:?}: {}", error.describe()),
        }
    }

    // ---- what is accepted ------------------------------------------------------------------

    #[test]
    fn two_components_are_major_and_minor_and_the_rest_are_zero() {
        let version = parsed("0.3");
        assert_eq!(version.components, [0, 3, 0, 0]);
        assert_eq!(version.written, 2);
    }

    #[test]
    fn three_and_four_components_keep_each_number_in_place() {
        let three = parsed("1.2.3");
        assert_eq!(three.components, [1, 2, 3, 0]);
        assert_eq!(three.written, 3);

        let four = parsed("1.2.3.4");
        assert_eq!(four.components, [1, 2, 3, 4]);
        assert_eq!(four.written, 4);
    }

    #[test]
    fn a_build_number_alone_does_not_make_a_trailing_zero_look_different() {
        // "1.2.0.0" has four components written, all of them 1, 2, 0, 0.
        let version = parsed("1.2.0.0");
        assert_eq!(version.components, [1, 2, 0, 0]);
        assert_eq!(version.written, 4);
    }

    #[test]
    fn the_largest_component_is_nine_nines_because_that_still_fits_thirty_two_bits() {
        let version = parsed("999999999.999999999.999999999.999999999");
        assert_eq!(
            version.components,
            [999_999_999, 999_999_999, 999_999_999, 999_999_999]
        );
        // 999,999,999 < 4,294,967,295 (u32::MAX): the digit count, not arithmetic, is what
        // guards the type.
    }

    #[test]
    fn leading_zeros_are_read_as_decimal_not_octal() {
        // "1.02" is 1.2 and "010" is ten; a reader that took a leading 0 for octal would say 8.
        assert_eq!(parsed("1.02").components, [1, 2, 0, 0]);
        assert_eq!(parsed("010.009").components, [10, 9, 0, 0]);
        assert_eq!(parsed("0.0").components, [0, 0, 0, 0]);
        assert_eq!(parsed("000000001.000000000").components, [1, 0, 0, 0]);
    }

    // ---- the order -------------------------------------------------------------------------

    #[test]
    fn components_are_compared_as_numbers_so_zero_ten_is_above_zero_nine() {
        // As text "0.10" < "0.9" ('1' < '9'); as numbers 10 > 9. The fixtures' two releases are
        // exactly this pair (0.9.0 and 0.10.0).
        assert!(parsed("0.10") > parsed("0.9"));
        assert!(parsed("0.10.0") > parsed("0.9.0"));
        assert!(parsed("10.0") > parsed("9.99.99.99"));
        assert!(parsed("2.0") < parsed("10.0"));
        assert!(parsed("1.10.0") > parsed("1.9.9.9"));
        assert!(parsed("0.0.10") > parsed("0.0.9"));
        assert!(parsed("0.0.0.10") > parsed("0.0.0.9"));
    }

    #[test]
    fn a_missing_trailing_component_is_zero_so_the_short_spelling_is_the_same_version() {
        assert_eq!(parsed("0.3"), parsed("0.3.0"));
        assert_eq!(parsed("0.3"), parsed("0.3.0.0"));
        assert_eq!(parsed("1.2.3"), parsed("1.2.3.0"));
        assert_eq!(parsed("1.2"), parsed("1.02"));
        assert_eq!(parsed("1.2").cmp(&parsed("1.2.0")), Ordering::Equal);
        // ... but a build number above zero is a later version.
        assert!(parsed("0.3") < parsed("0.3.0.1"));
    }

    #[test]
    fn the_order_is_strictly_ascending_along_a_hand_written_list() {
        // Each is below the next by the first component that differs, read left to right.
        let ascending = [
            "0.1",
            "0.9",
            "0.9.1",
            "0.10",
            "0.10.0.1",
            "0.11",
            "1.0",
            "1.0.0.1",
            "1.0.1",
            "1.1",
            "2.0",
            "10.0",
            "10.0.1",
            "999999999.0",
        ];
        for (lower, &low) in ascending.iter().enumerate() {
            for &high in &ascending[lower + 1..] {
                assert!(parsed(low) < parsed(high), "{low} < {high}");
                assert!(parsed(high) > parsed(low), "{high} > {low}");
                assert_ne!(parsed(low), parsed(high));
            }
        }
    }

    #[test]
    fn equality_ignores_how_many_components_were_written() {
        let two = parsed("5.6");
        let four = parsed("5.6.0.0");
        assert_ne!(two.written, four.written);
        let equal = two == four;
        let different = two != four;
        assert!(equal);
        assert!(!different);
    }

    // ---- the text back -----------------------------------------------------------------------

    #[test]
    fn to_string_gives_back_as_many_components_as_were_written() {
        assert_eq!(version::to_string(&parsed("0.3")), "0.3");
        assert_eq!(version::to_string(&parsed("0.3.0")), "0.3.0");
        assert_eq!(version::to_string(&parsed("0.3.0.0")), "0.3.0.0");
        assert_eq!(version::to_string(&parsed("10.20.30.40")), "10.20.30.40");
        assert_eq!(version::to_string(&parsed("999999999.0")), "999999999.0");
    }

    #[test]
    fn to_string_writes_the_number_not_the_leading_zeros() {
        // 1.02 is 1.2 (above), and the text that comes back says so.
        assert_eq!(version::to_string(&parsed("1.02")), "1.2");
        assert_eq!(version::to_string(&parsed("000.0010.7")), "0.10.7");
    }

    #[test]
    fn to_string_of_a_hand_built_version_stays_within_two_to_four_components() {
        // Out of range on purpose: values parse_version never makes.
        let mut version = Version {
            components: [1, 2, 3, 4],
            written: 0,
        };
        assert_eq!(version::to_string(&version), "1.2");
        version.written = 1;
        assert_eq!(version::to_string(&version), "1.2");
        version.written = 9;
        assert_eq!(version::to_string(&version), "1.2.3.4");
        // The default Version is 0.0 and says so.
        assert_eq!(version::to_string(&Version::default()), "0.0");
    }

    #[test]
    fn what_to_string_writes_parses_back_to_the_same_version() {
        for text in ["0.0", "0.3", "1.2.3", "1.2.3.4", "999999999.999999999"] {
            let version = parsed(text);
            assert_eq!(version::to_string(&version), text);
            assert_eq!(
                parsed(&version::to_string(&version)).written,
                version.written
            );
        }
    }

    // ---- what is refused -----------------------------------------------------------------------

    #[test]
    fn text_that_is_not_major_minor_patch_build_of_digits_is_a_parse_failure() {
        let refused = [
            "",                       // nothing
            ".",                      // two empty components
            "1",                      // a single component
            "1.",                     // an empty last component
            ".1",                     // an empty first component
            "1..2",                   // an empty middle component
            "1.2.",                   // a trailing dot
            "1.2.3.4.",               // a trailing dot after four
            "1.2.3.4.5",              // five components
            "1.2.3.4.5.6",            // six
            "a.b",                    // letters
            "v1.2",                   // a leading letter
            "V1.2",                   //
            "garbage1.2",             // leading garbage
            "1.2garbage",             // trailing garbage
            "1.2-beta",               // a pre-release suffix: there is no such syntax
            "1.2.3-rc1",              //
            "1.2+build5",             // build metadata
            "1.2.3.x",                // a non-digit in the last component
            "-1.2",                   // a sign
            "+1.2",                   //
            "1.-2",                   //
            "1.+2",                   //
            " 1.2",                   // white space, either end and inside
            "1.2 ",                   //
            "1. 2",                   //
            "1 .2",                   //
            "1.2\n",                  // a line end
            "1.2\r",                  //
            "\t1.2",                  //
            "1,2",                    // the wrong separator
            "1_2",                    //
            "1:2",                    //
            "1/2",                    //
            "0x1.2",                  // hexadecimal
            "1e3.2",                  // an exponent
            "1.2e1",                  //
            "1.5.",                   //
            "1.2.3.4a",               //
            "1.1234567890", // ten digits: 1,234,567,890 would fit, 9,999,999,999 would not
            "4294967296.0", // 2^32 exactly: ten digits
            "4294967295.0", // 2^32 - 1: still ten digits, refused by the same rule
            "99999999999999999999.1", // twenty digits: overflows even 64 bits
            "1.0000000000", // ten digits of zeros: the length is the rule
            "1.2.3.10000000000", //
            "\u{0661}.\u{0662}", // Arabic-Indic digits 1 and 2 (D9 A1, D9 A2 in UTF-8)
            "\u{FF11}.\u{FF12}", // fullwidth digits 1 and 2 (EF BC 91, EF BC 92 in UTF-8)
        ];
        for text in refused {
            let Err(error) = parse_version(text) else {
                panic!("accepted: {text:?}");
            };
            assert_eq!(error.code, ErrorCode::ParseFailure, "{text:?}");
            assert!(!error.message.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn a_nul_byte_anywhere_is_refused_and_never_ends_the_text() {
        // A C string would end at the NUL and read "1.2": the text is a str of its full length,
        // and the NUL is a character that is neither a digit nor a dot.
        assert!(parse_version("1.2\0").is_err());
        assert!(parse_version("1.2\x003").is_err());
        assert!(parse_version("\x001.2").is_err());
        assert!(parse_version("1.\x002").is_err());
        assert!(parse_version("\0").is_err());
    }

    #[test]
    fn a_refusal_quotes_the_text_bounded_and_on_one_line() {
        // The context is the text for text of a sensible length.
        let short_text = parse_version("1.2-beta").unwrap_err();
        assert_eq!(short_text.context, "1.2-beta");

        // A megabyte of digits is refused without being copied into the message.
        let huge = parse_version(&"7".repeat(1 << 20)).unwrap_err();
        assert!(huge.context.len() <= 64);
        assert!(huge.message.len() <= 200);

        // A NUL or a newline in the text does not reach the message as itself.
        let hostile = parse_version("1.\0\n2").unwrap_err();
        assert!(!hostile.context.contains('\0'));
        assert!(!hostile.context.contains('\n'));
        assert!(!hostile.describe().contains('\n'));
    }

    #[test]
    fn the_refusal_says_which_rule_it_broke_in_words_of_its_own() {
        // Different faults give different sentences, so a person reading a log can tell them
        // apart.
        let message = |text: &str| match parse_version(text) {
            Ok(_) => String::new(),
            Err(error) => error.message,
        };
        assert_ne!(message(""), message("1"));
        assert_ne!(message("1"), message("1..2"));
        assert_ne!(message("1..2"), message("1.2.3.4.5"));
        assert_ne!(message("1.2.3.4.5"), message("1.2a"));
        assert_ne!(message("1.2a"), message("1.12345678901"));
    }

    // ---- the program's own version --------------------------------------------------------------

    #[test]
    fn the_programs_own_version_parses_and_spells_the_text_it_was_built_with() {
        let own = version::program_version().unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(own, parsed(version::program_version_text()));
        // The project writes MAJOR.MINOR.PATCH without leading zeros (CMakeLists.txt), so the text
        // comes back exactly.
        assert_eq!(version::to_string(&own), version::program_version_text());
    }

    #[test]
    fn a_program_version_text_that_is_no_version_is_an_internal_error_not_a_parse_failure() {
        // A bad file is a ParseFailure (parse_version); the program's own text is not a file, so
        // a refusal of it is a broken build and says so.
        for text in [
            "",
            "1",
            "0.x",
            "0.3.0-beta",
            "v0.3.0",
            "0.3.0.1.2",
            " 0.3.0",
            "0..3",
            "1234567890.0",
        ] {
            let own = detail::program_version_of(text).expect_err(text);
            assert_eq!(own.code, ErrorCode::Internal, "{text:?}");
            assert!(parse_version(text).is_err(), "{text:?}");
            assert_eq!(
                parse_version(text).unwrap_err().code,
                ErrorCode::ParseFailure,
                "{text:?}"
            );
            assert_eq!(own.context, text, "the context is the text");
        }
        // A good text is the same as parse_version's.
        let good = detail::program_version_of("0.3.0").expect("0.3.0 is a version");
        assert_eq!(good, parsed("0.3"));
        // The context is bounded and printable, as every version error's is.
        let hostile = detail::program_version_of(&format!("0.3\n{}", "x".repeat(100_000)))
            .expect_err("not a version");
        assert_eq!(hostile.code, ErrorCode::Internal);
        assert!(hostile.context.len() <= 64);
        assert!(!hostile.context.contains('\n'));
    }
}
