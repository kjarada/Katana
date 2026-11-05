#[allow(
    unsafe_code,
    reason = "The oracle tests change the C locale; restore it under a mutex."
)]
mod c_locale {
    use std::ffi::{CStr, CString, c_char, c_int};
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());
    #[cfg(target_os = "macos")]
    const LC_CTYPE: c_int = 2;
    #[cfg(not(target_os = "macos"))]
    const LC_CTYPE: c_int = 0;

    unsafe extern "C" {
        fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
    }

    struct Restore(CString);

    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe { setlocale(LC_CTYPE, self.0.as_ptr()) };
        }
    }

    pub fn with_available_locales(mut check: impl FnMut(&str)) {
        let _lock = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let previous = unsafe { setlocale(LC_CTYPE, std::ptr::null()) };
        let saved = if previous.is_null() {
            CString::new("C").unwrap()
        } else {
            unsafe { CStr::from_ptr(previous) }.to_owned()
        };
        let _restore = Restore(saved);
        for name in ["en_US.ISO-8859-1", "de_DE.ISO8859-1", ".1252", "C"] {
            let locale = CString::new(name).unwrap();
            if !unsafe { setlocale(LC_CTYPE, locale.as_ptr()) }.is_null() {
                check(name);
            }
        }
    }
}

mod core_text {
    use katana_core::text::*;

    #[test]
    fn trim_removes_exactly_the_c_locale_blanks_from_both_ends() {
        assert_eq!(trimmed(b" \t\r\n\x0b\x0cword \t\r\n\x0b\x0c"), b"word");
        assert_eq!(trimmed(b"  two words  "), b"two words");
        assert_eq!(trimmed(b""), b"");
        assert_eq!(trimmed(b" \t "), b"");
        assert_eq!(trimmed(b"\0a\0"), b"\0a\0");
        assert_eq!(trimmed(b"\xc2\xa0x\xc2\xa0"), b"\xc2\xa0x\xc2\xa0");
    }

    #[test]
    fn trim_never_cuts_a_utf8_sequence_whatever_the_locale() {
        super::c_locale::with_available_locales(|name| {
            assert_eq!(trimmed(b"voil\xc3\xa0"), b"voil\xc3\xa0", "{name}");
            assert_eq!(lowered(b"VOIL\xc3\x80"), b"voil\xc3\x80", "{name}");
            assert_eq!(
                uppered(b"\xe0\xa4\x85 voil\xc3\xa0"),
                b"\xe0\xa4\x85 VOIL\xc3\xa0",
                "{name}"
            );
        });
    }

    #[test]
    fn case_folding_touches_only_ascii_letters() {
        assert_eq!(lowered(b"Survey CODE 12-b"), b"survey code 12-b");
        assert_eq!(uppered(b"Survey code 12-b"), b"SURVEY CODE 12-B");
        assert_eq!(uppered(b""), b"");
        assert_eq!(uppered(b"`{@["), b"`{@[");
        assert!(equals_ignoring_case(b"GSI16", b"gsi16"));
        assert!(!equals_ignoring_case(b"GSI16", b"GSI8"));
        assert!(!equals_ignoring_case(b"ab", b"abc"));
        assert!(!equals_ignoring_case(b"@", b"`"));
        assert!(is_ascii_space(b'\x0b'));
        assert!(!is_ascii_space(b'\0'));
    }

    #[test]
    fn an_integer_must_be_the_whole_token_with_at_most_one_sign() {
        assert_eq!(parse_integer("42"), Some(42));
        assert_eq!(parse_integer("+42"), Some(42));
        assert_eq!(parse_integer("-42"), Some(-42));
        assert_eq!(parse_integer("0007"), Some(7));
        assert_eq!(parse_integer("9223372036854775807"), Some(i64::MAX));
        for bad in [
            "",
            "+",
            "-",
            "+-1",
            "++1",
            "--1",
            "2.5",
            "1e3",
            "12a",
            " 1",
            "1 ",
            "9223372036854775808",
        ] {
            assert!(parse_integer(bad).is_none(), "'{bad}'");
        }
    }

    #[test]
    fn a_real_must_be_the_whole_token_and_finite() {
        assert_eq!(parse_finite_double("0.5"), Some(0.5));
        assert_eq!(parse_finite_double("+0.5"), Some(0.5));
        assert_eq!(parse_finite_double("-1234.25"), Some(-1234.25));
        assert_eq!(parse_finite_double("1e3"), Some(1000.0));
        assert_eq!(parse_finite_double("6.25E-2"), Some(0.0625));
        assert_eq!(parse_finite_double("12"), Some(12.0));
        assert_eq!(parse_finite_double("7410850.123"), Some(7410850.123));
        for bad in [
            "", "+", "-", "+-1", "1,5", "0x10", "inf", "-inf", "nan", "infinity", "1e999", "1.5m",
            " 1", ".",
        ] {
            assert!(parse_finite_double(bad).is_none(), "'{bad}'");
        }
    }

    #[test]
    fn every_kind_of_line_ending_ends_a_line_and_interior_blank_lines_are_kept() {
        assert_eq!(
            split_lines(b"a\r\nb\rc\n\nd\n"),
            [b"a".as_slice(), b"b", b"c", b"", b"d"]
        );
        assert!(split_lines(b"").is_empty());
        assert_eq!(split_lines(b"only"), [b"only"]);
        assert_eq!(split_lines(b"a\n\rb"), [b"a".as_slice(), b"", b"b"]);
        assert_eq!(split_lines(b"\n"), [b""]);
    }

    #[test]
    fn exact_real_text_reads_back_as_the_same_double() {
        assert_eq!(format_exact_real(0.1), "0.1");
        assert_eq!(format_exact_real(502000.125), "502000.125");
        assert_eq!(format_exact_real(-3.0), "-3");
        for value in [
            0.1,
            1.0 / 3.0,
            7410850.123,
            -255440.0625,
            5e-324,
            1.7976931348623157e308,
        ] {
            let back = parse_finite_double(format_exact_real(value));
            assert!(back.is_some(), "{value}");
            assert_eq!(back.unwrap(), value);
        }
    }

    #[test]
    fn fixed_text_of_ordinary_values_is_the_three_decimals_a_record_always_had() {
        assert_eq!(format_fixed(0.0, 3), "0.000");
        assert_eq!(format_fixed(12.0, 3), "12.000");
        assert_eq!(format_fixed(1.25, 3), "1.250");
        assert_eq!(format_fixed(-2.25, 3), "-2.250");
        assert_eq!(format_fixed(0.017, 3), "0.017");
        assert_eq!(format_fixed(1471.24, 3), "1471.240");
        assert_eq!(format_fixed(123456.7895, 3), "123456.789");
        assert_eq!(format_fixed(0.0005, 3), "0.001");
        assert_eq!(format_fixed(-0.0005, 3), "-0.001");
        assert_eq!(format_fixed(0.0015, 3), "0.002");
        assert_eq!(format_fixed(2.0005, 3), "2.001");
        assert_eq!(format_fixed(1.25, 0), "1");
        assert_eq!(format_fixed(1.5, 2), "1.50");
    }

    #[test]
    fn a_value_that_rounds_to_nothing_has_no_sign() {
        assert_eq!(format_fixed(-0.0, 3), "0.000");
        assert_eq!(format_fixed(-0.0004, 3), "0.000");
        assert_eq!(format_fixed(-1e-300, 3), "0.000");
        assert_eq!(format_fixed(-5e-324, 3), "0.000");
        assert_eq!(format_fixed(-0.4, 0), "0");
        assert_eq!(format_fixed(-0.0006, 3), "-0.001");
        assert_eq!(format_fixed(-0.6, 0), "-1");
    }

    #[test]
    fn fixed_text_of_a_finite_value_never_fails_however_large_it_is() {
        assert_eq!(
            format_fixed(1e60, 3),
            "999999999999999949387135297074018866963645011013410073083904.000"
        );
        assert_eq!(
            format_fixed(1e61, 3),
            "9999999999999999493871352970740188669636450110134100730839040.000"
        );
        let minus = format_fixed(-1e300, 3);
        assert_eq!(minus.len(), 306);
        assert_eq!(minus.as_bytes()[0], b'-');
        assert_eq!(&minus[minus.len() - 4..], ".000");
        let minus_back = parse_finite_double(&minus[..minus.len() - 4]);
        assert!(minus_back.is_some());
        assert_eq!(
            minus_back.unwrap(),
            -1e300,
            "the digits are the double's, exactly"
        );
        let most = format_fixed(f64::MAX, 3);
        assert_eq!(most.len(), 313);
        assert_eq!(&most[..20], "17976931348623157081");
        assert_eq!(&most[most.len() - 10..], "858368.000");
        let most_back = parse_finite_double(&most[..most.len() - 4]);
        assert!(most_back.is_some());
        assert_eq!(most_back.unwrap(), f64::MAX);
        assert_eq!(format_fixed(1.5, 100).len(), 62);
        assert_eq!(format_fixed(1.25, -2), "1");
    }

    #[test]
    fn fixed_text_of_a_non_finite_value_is_what_to_chars_writes() {
        assert_eq!(format_fixed(f64::NAN, 3), "nan");
        assert_eq!(format_fixed(f64::INFINITY, 3), "inf");
        assert_eq!(format_fixed(f64::NEG_INFINITY, 3), "-inf");
    }
}

mod reply_record {
    use katana_core::text::{read_reply_record, reply_quoted};

    #[test]
    fn a_quoted_value_reads_back_as_the_text_it_was() {
        let awkward = "layer Survey/Kerb = \"a\" \\ b\nnext";
        let line = format!("ifc object from={} count=2", reply_quoted(awkward));
        assert!(!line.contains('\n'));
        let record = read_reply_record(&line).unwrap();
        assert_eq!(record.words, ["ifc", "object"]);
        assert_eq!(record.fields.len(), 2);
        assert_eq!(record.value("from"), Some(awkward));
        assert_eq!(record.value("count"), Some("2"));
        assert_eq!(record.value("absent"), None);
    }

    #[test]
    fn words_come_first_and_an_empty_value_is_kept() {
        let record = read_reply_record("  line id=W1 system= why=\"\"  ").unwrap();
        assert_eq!(record.words, ["line"]);
        assert_eq!(record.value("id"), Some("W1"));
        assert_eq!(record.value("system"), Some(""));
        assert_eq!(record.value("why"), Some(""));
    }

    #[test]
    fn a_line_that_is_not_a_record_is_refused() {
        assert!(read_reply_record("").is_none());
        assert!(read_reply_record("   ").is_none());
        assert!(read_reply_record("ifc why=\"never closed").is_none());
        assert!(read_reply_record("ifc count=1 stray").is_none());
        assert!(read_reply_record("ifc =1").is_none());
    }
}

mod numeric_contract {
    use katana_core::text::{
        format_exact_real, format_general, parse_finite_double, unescape_typed,
    };

    #[test]
    fn underflow_is_not_a_successful_zero() {
        for token in ["1e-999", "-1e-999", "2e-324", "0.0001e-999"] {
            assert!(parse_finite_double(token).is_none(), "{token}");
        }
        assert_eq!(parse_finite_double("0e-999"), Some(0.0));
        assert_eq!(parse_finite_double("5e-324"), Some(5e-324));
        assert!(parse_finite_double("-0e-999").unwrap().is_sign_negative());
    }

    #[test]
    fn exact_exponents_and_negative_zero_keep_the_cpp_spelling() {
        assert_eq!(format_exact_real(1e20), "1e+20");
        assert_eq!(format_exact_real(1e-7), "1e-07");
        assert_eq!(format_exact_real(-0.0), "-0");
        assert_eq!(format_exact_real(f64::NAN), "nan");
        assert_eq!(format_exact_real(f64::INFINITY), "inf");
        assert_eq!(format_exact_real(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn shortest_decimal_ties_choose_the_even_digit_as_cpp_does() {
        let value = f64::from_bits(0x3e60_0000_0000_0000);
        assert_eq!(format_exact_real(value), "2.9802322387695312e-08");
        assert_eq!(format_exact_real(-value), "-2.9802322387695312e-08");
    }

    #[test]
    fn twelve_significant_digits_use_general_notation() {
        assert_eq!(format_general(1.23456789012345, 12), "1.23456789012");
        assert_eq!(format_general(1e12, 12), "1e+12");
        assert_eq!(format_general(1e-4, 12), "0.0001");
        assert_eq!(format_general(1e-5, 12), "1e-05");
        assert_eq!(format_general(-0.0, 12), "-0");
    }

    #[test]
    fn typed_text_unescapes_only_newlines_and_backslashes() {
        assert_eq!(unescape_typed(r"a\nb\\c\q"), "a\nb\\c\\q");
        assert_eq!(unescape_typed("tail\\"), "tail\\");
        assert_eq!(unescape_typed(r"C:\\new"), r"C:\new");
    }
}
