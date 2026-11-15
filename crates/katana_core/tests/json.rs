//! katana_core::json, nlohmann::json 3.12's twin. These are new tests, not twins of C++ cases:
//! the C++ never tested the library it used. Every expected value is worked out by hand from
//! nlohmann's source (the header named at each module) or its documentation, never taken from a
//! run of this port. A position is chars_read_total from lexer.hpp's get() and unget(): each byte
//! read counts one, the byte read only to see that a number has ended is given back, and the
//! end of the text counts one when it is read.

use katana_core::json::{
    DomBuilder, Json, JsonError, JsonErrorKind, ObjectMap, OrderedJson, Sax, dump_float, sax_parse,
    write_string,
};

fn parse(text: &str) -> Json {
    Json::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn refused(text: &[u8]) -> JsonError {
    match Json::parse(text) {
        Ok(_) => panic!("accepted: {}", String::from_utf8_lossy(text)),
        Err(error) => error,
    }
}

// lexer.hpp's scan_number: unsigned until a minus sign, a fraction or an exponent is read, and
// a whole number that strtoull or strtoll cannot hold (ERANGE) read again as a double.
mod json_numbers {
    use super::*;

    #[test]
    fn a_whole_number_without_a_sign_is_unsigned() {
        assert_eq!(parse("0").as_unsigned(), Some(0));
        assert_eq!(parse("7").as_unsigned(), Some(7));
        assert_eq!(
            parse("18446744073709551615").as_unsigned(),
            Some(u64::MAX),
            "2^64 - 1, the largest strtoull holds"
        );
    }

    #[test]
    fn a_whole_number_with_a_minus_sign_is_signed_and_minus_zero_is_the_integer_zero() {
        assert_eq!(parse("-1").as_integer(), Some(-1));
        // "-0" has no fraction and no exponent: strtoll reads it as 0, which is no double -0.0.
        let minus_zero = parse("-0");
        assert_eq!(minus_zero.as_integer(), Some(0));
        assert!(!minus_zero.is_number_float());
        assert_eq!(minus_zero.dump(), "0");
        assert_eq!(
            parse("-9223372036854775808").as_integer(),
            Some(i64::MIN),
            "-2^63, the smallest strtoll holds"
        );
    }

    #[test]
    fn a_fraction_an_exponent_or_a_whole_number_past_64_bits_is_a_double() {
        assert_eq!(parse("1.0").as_float(), Some(1.0));
        assert_eq!(parse("1e2").as_float(), Some(100.0));
        assert_eq!(parse("1E2").as_float(), Some(100.0));
        assert_eq!(
            parse("-0.0").as_float().map(f64::to_bits),
            Some((-0.0_f64).to_bits())
        );
        // 2^64 is one past the largest unsigned, and a double exactly.
        assert_eq!(
            parse("18446744073709551616").as_float(),
            Some(2.0_f64.powi(64))
        );
        // -2^63 - 1 is past strtoll; the nearest double is -2^63.
        assert_eq!(
            parse("-9223372036854775809").as_float(),
            Some(-(2.0_f64.powi(63)))
        );
        // Below the smallest double: strtod gives 0 and nlohmann does not look at errno.
        assert_eq!(parse("1e-400").as_float(), Some(0.0));
    }

    #[test]
    fn a_value_made_in_code_takes_the_kind_of_its_type() {
        // basic_json's constructors: a signed integer type gives number_integer, an unsigned one
        // number_unsigned, a floating type number_float.
        assert_eq!(Json::from(1_i32).as_integer(), Some(1));
        assert_eq!(Json::from(1_i64).as_integer(), Some(1));
        assert_eq!(Json::from(1_u32).as_unsigned(), Some(1));
        assert_eq!(Json::from(1_u64).as_unsigned(), Some(1));
        assert_eq!(Json::from(1.0_f64).as_float(), Some(1.0));
        assert_eq!(Json::from(0.5_f32).as_float(), Some(0.5));
        assert!(Json::from(f64::NAN).is_number_float());
    }

    #[test]
    fn numbers_of_two_kinds_compare_as_numbers() {
        // JSON_IMPLEMENT_OPERATOR: through the double where one side is a float, through the
        // signed integer where the kinds are the two integers.
        assert_eq!(Json::from(1_i64), Json::from(1.0));
        assert_eq!(Json::from(1_u64), Json::from(1.0));
        assert_eq!(Json::from(1_u64), Json::from(1_i64));
        // static_cast<std::int64_t>(2^64 - 1) is -1.
        assert_eq!(Json::from(u64::MAX), Json::from(-1_i64));
        assert_ne!(Json::from(1_i64), Json::from(1.5));
        // A NaN is unequal to everything, itself included (compares_unordered).
        assert_ne!(Json::from(f64::NAN), Json::from(f64::NAN));
        assert_ne!(Json::from(f64::NAN), Json::from(1_i64));
        // Different kinds that are not two numbers are unequal.
        assert_ne!(Json::from(1_i64), Json::from(true));
        assert_ne!(Json::from("1"), Json::from(1_i64));
        assert_eq!(parse("[1, 2.0]"), parse("[1.0, 2]"));
    }
}

// The two objects: json's std::map, ordered_json's ordered_map (ordered_map.hpp), and what each
// does with a key given twice in the parser (json_sax.hpp: key() takes operator[], the value is
// assigned when read), through operator[] (emplace of a null, then the assignment) and in an
// initializer list (basic_json's constructor emplaces each pair).
mod json_objects {
    use super::*;

    #[test]
    fn a_json_object_is_sorted_by_key_and_an_ordered_one_keeps_its_order() {
        let text = r#"{"b":1,"a":2,"c":{"z":1,"y":2}}"#;
        assert_eq!(parse(text).dump(), r#"{"a":2,"b":1,"c":{"y":2,"z":1}}"#);
        assert_eq!(OrderedJson::parse(text).unwrap().dump(), text);
    }

    #[test]
    fn a_key_given_twice_in_a_text_keeps_its_last_value_and_its_first_place() {
        let text = r#"{"b":1,"a":2,"b":3}"#;
        assert_eq!(parse(text).dump(), r#"{"a":2,"b":3}"#);
        assert_eq!(OrderedJson::parse(text).unwrap().dump(), r#"{"b":3,"a":2}"#);
        // The value replaced is a container as readily as a number.
        assert_eq!(parse(r#"{"a":[1,2],"a":{}}"#).dump(), r#"{"a":{}}"#);
    }

    #[test]
    fn assigning_through_a_key_replaces_the_value_in_its_place() {
        let mut ordered = OrderedJson::Null;
        ordered["b"] = OrderedJson::from(1);
        ordered["a"] = OrderedJson::from(2);
        ordered["b"] = OrderedJson::from(3);
        assert_eq!(ordered.dump(), r#"{"b":3,"a":2}"#);

        let mut sorted = Json::Null;
        sorted["b"] = Json::from(1);
        sorted["a"] = Json::from(2);
        assert_eq!(sorted.dump(), r#"{"a":2,"b":1}"#);
        // operator[] on a missing key puts a null in before anything is assigned.
        let _ = &mut sorted["c"];
        assert_eq!(sorted.dump(), r#"{"a":2,"b":1,"c":null}"#);
    }

    #[test]
    fn an_object_made_from_a_list_of_members_keeps_the_first_of_a_key_given_twice() {
        let made = Json::object_of([("a", Json::from(1)), ("a", Json::from(2))]);
        assert_eq!(made.dump(), r#"{"a":1}"#);
        let ordered = OrderedJson::object_of([
            ("b", OrderedJson::from(1)),
            ("a", OrderedJson::from(2)),
            ("b", OrderedJson::from(3)),
        ]);
        assert_eq!(ordered.dump(), r#"{"b":1,"a":2}"#);
    }

    #[test]
    fn erasing_a_member_keeps_the_order_of_the_rest() {
        let mut ordered = OrderedJson::parse(r#"{"c":1,"a":2,"b":3}"#).unwrap();
        assert_eq!(ordered.erase("a"), Ok(1));
        assert_eq!(ordered.erase("z"), Ok(0));
        assert_eq!(ordered.dump(), r#"{"c":1,"b":3}"#);
    }

    #[test]
    fn objects_compare_member_by_member_in_their_order() {
        // std::map == compares the sorted pairs; ordered_map is a std::vector, compared in order.
        assert_eq!(parse(r#"{"a":1,"b":2}"#), parse(r#"{"b":2,"a":1}"#));
        assert_ne!(
            OrderedJson::parse(r#"{"a":1,"b":2}"#).unwrap(),
            OrderedJson::parse(r#"{"b":2,"a":1}"#).unwrap()
        );
    }

    #[test]
    fn the_members_are_walked_in_the_objects_order() {
        let sorted = parse(r#"{"b":1,"a":2}"#);
        let keys: Vec<&str> = sorted
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(keys, ["a", "b"]);
        let ordered = OrderedJson::parse(r#"{"b":1,"a":2}"#).unwrap();
        let keys: Vec<&str> = ordered
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(keys, ["b", "a"]);
    }
}

// serializer.hpp's dump and dump_escaped, and to_chars.hpp for the doubles.
mod json_dump {
    use super::*;

    // Worked out by hand from to_chars.hpp's format_buffer and append_exponent (fixed for a
    // decimal exponent in (-4, 15], at least one fractional digit, two exponent digits) for
    // values whose shortest digits are plain.
    #[test]
    fn doubles_are_written_as_nlohmann_writes_them() {
        assert_eq!(dump_float(0.0), "0.0");
        assert_eq!(dump_float(-0.0), "-0.0");
        assert_eq!(dump_float(1.0), "1.0");
        assert_eq!(dump_float(7.0), "7.0");
        assert_eq!(dump_float(1.5), "1.5");
        assert_eq!(dump_float(-2.25), "-2.25");
        assert_eq!(dump_float(0.0001), "0.0001");
        assert_eq!(dump_float(1e-5), "1e-05");
        assert_eq!(dump_float(1e14), "100000000000000.0");
        assert_eq!(dump_float(1e15), "1e+15");
        assert_eq!(dump_float(1e20), "1e+20");
        assert_eq!(dump_float(1.5e20), "1.5e+20");
        assert_eq!(dump_float(123.456), "123.456");
        assert_eq!(dump_float(f64::NAN), "null");
        assert_eq!(dump_float(f64::INFINITY), "null");
        assert_eq!(dump_float(f64::NEG_INFINITY), "null");
    }

    // The digits 12345 at every place format_buffer distinguishes: n = 5 + decimal exponent.
    #[test]
    fn the_notation_follows_the_place_of_the_decimal_point() {
        assert_eq!(dump_float(1.2345e-05), "1.2345e-05"); // n = -4, not above min_exp
        assert_eq!(dump_float(1.2345e-04), "0.00012345"); // n = -3
        assert_eq!(dump_float(1.2345e-01), "0.12345"); // n = 0
        assert_eq!(dump_float(1.2345), "1.2345"); // n = 1
        assert_eq!(dump_float(1.2345e+04), "12345.0"); // n = k = 5
        assert_eq!(dump_float(1.2345e+14), "123450000000000.0"); // n = 15 = max_exp
        assert_eq!(dump_float(1.2345e+15), "1.2345e+15"); // n = 16
        // Three exponent digits. The doubles nearest 1e100 and 1e-100 are 0.16 and 0.31 of half
        // an ulp from them, so the one digit is the shortest text of each.
        assert_eq!(dump_float(1e100), "1e+100");
        assert_eq!(dump_float(-1e-100), "-1e-100");
    }

    // The shortest text that reads back to the same double, not the double's exact digits:
    // 0.1 is 0.1000000000000000055511151231257827... The extremes, each the shortest round-trip
    // text of its double: the smallest denormal (any one digit from 3 to 7 lies inside its
    // rounding interval, 2.47e-324 to 7.41e-324, and Grisu2's rounding step walks down from 7 to
    // the nearest, 5), the smallest normal and the largest double.
    #[test]
    fn a_double_is_written_with_the_fewest_digits_that_read_back_to_it() {
        assert_eq!(dump_float(0.1), "0.1");
        assert_eq!(dump_float(0.3), "0.3");
        assert_eq!(dump_float(f64::from_bits(1)), "5e-324");
        assert_eq!(dump_float(f64::MIN_POSITIVE), "2.2250738585072014e-308");
        assert_eq!(dump_float(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(dump_float(-f64::MAX), "-1.7976931348623157e+308");
    }

    #[test]
    fn objects_are_written_with_their_keys_sorted() {
        let mut map = Json::object();
        map["b"] = Json::from(1_u64);
        map["a"] = Json::from(2.0);
        map["c"] = Json::from("x\"\n\u{1}");
        assert_eq!(map.dump(), "{\"a\":2.0,\"b\":1,\"c\":\"x\\\"\\n\\u0001\"}");
    }

    #[test]
    fn integers_are_written_in_decimal_and_doubles_always_look_like_doubles() {
        assert_eq!(Json::from(i64::MIN).dump(), "-9223372036854775808");
        assert_eq!(Json::from(u64::MAX).dump(), "18446744073709551615");
        assert_eq!(Json::from(0_u64).dump(), "0");
        assert_eq!(Json::from(2.0).dump(), "2.0");
        assert_eq!(Json::from(f64::NAN).dump(), "null");
        assert_eq!(
            parse("[1,1.0,1e0,true,null]").dump(),
            "[1,1.0,1.0,true,null]"
        );
    }

    #[test]
    fn strings_are_escaped_as_dump_escaped_escapes_them() {
        // The seven two-character escapes, \u00xx in lower-case hex for the other control
        // characters, and the solidus, DEL and every non-ASCII character as they are.
        let text = "\"\\/\u{08}\u{0C}\n\r\t\u{01}\u{1F}\u{7F}\u{E9}\u{1F600}";
        assert_eq!(
            Json::from(text).dump(),
            "\"\\\"\\\\/\\b\\f\\n\\r\\t\\u0001\\u001f\u{7F}\u{E9}\u{1F600}\""
        );
        // ensure_ascii: from DEL up as \uxxxx, a surrogate pair beyond U+FFFF -
        // 0xD7C0 + (0x1F600 >> 10) = 0xD83D and 0xDC00 + (0x1F600 & 0x3FF) = 0xDE00.
        assert_eq!(
            Json::from("\u{7F}\u{E9}\u{1F600}").dump_with(-1, ' ', true),
            "\"\\u007f\\u00e9\\ud83d\\ude00\""
        );
        let mut quoted = String::new();
        write_string(&mut quoted, "a\"b", false);
        assert_eq!(quoted, "\"a\\\"b\"");
    }

    #[test]
    fn an_indent_puts_each_member_on_a_line_of_its_own() {
        let value = parse(r#"{"b":{},"a":[1,[]]}"#);
        assert_eq!(
            value.dump_with(2, ' ', false),
            "{\n  \"a\": [\n    1,\n    []\n  ],\n  \"b\": {}\n}"
        );
        // 0 is pretty-printed with no indent; below 0 is compact.
        assert_eq!(
            value.dump_with(0, ' ', false),
            "{\n\"a\": [\n1,\n[]\n],\n\"b\": {}\n}"
        );
        assert_eq!(value.dump_with(-1, ' ', false), r#"{"a":[1,[]],"b":{}}"#);
        assert_eq!(parse("[1]").dump_with(1, '\t', false), "[\n\t1\n]");
        assert_eq!(parse("5").dump_with(4, ' ', false), "5");
    }

    #[test]
    fn an_indent_wider_than_the_indent_string_is_padded_with_spaces() {
        // The serializer's indent_string holds 512 indent characters and is doubled with spaces
        // when a level needs more: 600 is 512 of the character and 88 spaces.
        let expected = format!("[\n{}{}1\n]", "x".repeat(512), " ".repeat(88));
        assert_eq!(parse("[1]").dump_with(600, 'x', false), expected);
    }
}

// parser.hpp's exception_message and lexer.hpp's error messages, with parse_error::create's
// "parse error at line L, column C: " (position_t: the line from 1, the column the characters
// read on it).
mod json_parse_errors {
    use super::*;

    #[test]
    fn an_empty_text_is_said_to_be_empty() {
        let error = refused(b"");
        assert_eq!(error.kind, JsonErrorKind::ParseError);
        assert_eq!(error.id, 101);
        assert_eq!(error.byte, 1);
        assert_eq!(
            error.what,
            "[json.exception.parse_error.101] parse error at line 1, column 1: attempting to parse an empty input; check that your input string or stream contains the expected JSON"
        );
        // A NUL is the end of the text (the end of a C string), so "\0" is empty too.
        assert_eq!(refused(b"\0").what, error.what);
    }

    #[test]
    fn an_unexpected_token_names_what_was_expected_and_where() {
        // '[' '1' ',' '2' and the end of the text: the fifth character read.
        let error = refused(b"[1,2");
        assert_eq!(error.byte, 5);
        assert_eq!(
            error.what,
            "[json.exception.parse_error.101] parse error at line 1, column 5: syntax error while parsing array - unexpected end of input; expected ']'"
        );
        // The '1' is the sixth byte: its end is seen at the '}' (7), which is given back.
        assert_eq!(
            refused(br#"{"a" 1}"#).what,
            "[json.exception.parse_error.101] parse error at line 1, column 6: syntax error while parsing object separator - unexpected number literal; expected ':'"
        );
        assert_eq!(
            refused(b"[1,]").what,
            "[json.exception.parse_error.101] parse error at line 1, column 4: syntax error while parsing value - unexpected ']'; expected '[', '{', or a literal"
        );
        assert_eq!(
            refused(b"{1:2}").what,
            "[json.exception.parse_error.101] parse error at line 1, column 2: syntax error while parsing object key - unexpected number literal; expected string literal"
        );
    }

    #[test]
    fn last_read_is_the_text_since_the_last_string_or_number_began() {
        // Only scan_string and scan_number reset the token text; a literal and the structural
        // characters add to it. After the object, "1" then '}' then 'x' have been read.
        assert_eq!(
            refused(br#"{"a":1}x"#).what,
            "[json.exception.parse_error.101] parse error at line 1, column 8: syntax error while parsing value - invalid literal; last read: '1}x'; expected end of input"
        );
        assert_eq!(
            refused(b"not json").what,
            "[json.exception.parse_error.101] parse error at line 1, column 2: syntax error while parsing value - invalid literal; last read: 'no'"
        );
        // A control character in last read is shown as <U+XXXX>.
        assert_eq!(
            refused(b"\"a\x01\"").what,
            "[json.exception.parse_error.101] parse error at line 1, column 3: syntax error while parsing value - invalid string: control character U+0001 (SOH) must be escaped to \\u0001; last read: '\"a<U+0001>'"
        );
    }

    #[test]
    fn the_string_errors_are_nlohmanns() {
        let message = |text: &[u8]| {
            let what = refused(text).what;
            what[what.find(": ").unwrap() + 2..].to_owned()
        };
        assert_eq!(
            message(br#""\x""#),
            "syntax error while parsing value - invalid string: forbidden character after backslash; last read: '\"\\x'"
        );
        assert_eq!(
            message(br#""\u12""#),
            "syntax error while parsing value - invalid string: '\\u' must be followed by 4 hex digits; last read: '\"\\u12\"'"
        );
        assert_eq!(
            message(br#""\ud800""#),
            "syntax error while parsing value - invalid string: surrogate U+D800..U+DBFF must be followed by U+DC00..U+DFFF; last read: '\"\\ud800\"'"
        );
        assert_eq!(
            message(br#""\udc00""#),
            "syntax error while parsing value - invalid string: surrogate U+DC00..U+DFFF must follow U+D800..U+DBFF; last read: '\"\\udc00'"
        );
        assert_eq!(
            message(b"\"abc"),
            "syntax error while parsing value - invalid string: missing closing quote; last read: '\"abc'"
        );
        assert!(message(b"\"\xFF\"").contains("invalid string: ill-formed UTF-8 byte"));
        // C0 80, an overlong NUL, and ED A0 80, a surrogate written as UTF-8.
        assert!(message(b"\"\xC0\x80\"").contains("ill-formed UTF-8 byte"));
        assert!(message(b"\"\xED\xA0\x80\"").contains("ill-formed UTF-8 byte"));
    }

    #[test]
    fn the_number_errors_are_nlohmanns() {
        let message = |text: &[u8]| {
            let what = refused(text).what;
            what[what.find(": ").unwrap() + 2..].to_owned()
        };
        assert_eq!(
            message(b"-"),
            "syntax error while parsing value - invalid number; expected digit after '-'; last read: '-'"
        );
        assert_eq!(
            message(b"1."),
            "syntax error while parsing value - invalid number; expected digit after '.'; last read: '1.'"
        );
        assert_eq!(
            message(b"1e"),
            "syntax error while parsing value - invalid number; expected '+', '-', or digit after exponent; last read: '1e'"
        );
        assert_eq!(
            message(b"1e+"),
            "syntax error while parsing value - invalid number; expected digit after exponent sign; last read: '1e+'"
        );
        // A leading zero ends the number: "01" is the number 0 and then a number 1.
        assert_eq!(
            message(b"01"),
            "syntax error while parsing value - unexpected number literal; expected end of input"
        );
    }

    #[test]
    fn a_number_past_the_largest_double_is_out_of_range_406() {
        let error = refused(b"[1e400]");
        assert_eq!(error.kind, JsonErrorKind::OutOfRange);
        assert_eq!(error.id, 406);
        assert_eq!(error.byte, 0, "out_of_range has no byte");
        assert_eq!(
            error.what,
            "[json.exception.out_of_range.406] number overflow parsing '1e400'"
        );
        assert_eq!(
            refused(b"-1e400").what,
            "[json.exception.out_of_range.406] number overflow parsing '-1e400'"
        );
    }

    #[test]
    fn a_byte_order_mark_is_stepped_over_and_a_cut_one_refused() {
        assert_eq!(parse("\u{FEFF}[1]").dump(), "[1]");
        let error = refused(b"\xEF\xBB{}");
        assert_eq!(error.byte, 3);
        assert!(
            error.what.starts_with(
                "[json.exception.parse_error.101] parse error at line 1, column 3: syntax error while parsing value - invalid BOM; must be 0xEF 0xBB 0xBF if given; last read: '"
            ),
            "{}",
            error.what
        );
    }

    #[test]
    fn a_nul_byte_outside_a_string_ends_the_text() {
        assert_eq!(Json::parse(b"{}\0junk").unwrap().dump(), "{}");
        assert!(Json::parse(b"{\0}").is_err());
        assert!(Json::parse(b"\"a\0\"").is_err());
    }

    #[test]
    fn the_escapes_are_read() {
        assert_eq!(
            parse(r#""\"\\\/\b\f\n\r\t\u00e9\ud83d\ude00\u0000""#).as_str(),
            Some("\"\\/\u{08}\u{0C}\n\r\t\u{E9}\u{1F600}\0")
        );
    }
}

// json.hpp's accessors and from_json.hpp's conversions, with their exceptions' what() texts.
mod json_accessors {
    use super::*;

    #[test]
    fn at_finds_a_member_or_an_element_and_says_why_not() {
        let value = parse(r#"{"a":[10,20]}"#);
        assert_eq!(
            value.at("a").unwrap().at_index(1).unwrap().as_unsigned(),
            Some(20)
        );
        assert_eq!(
            value.at("b").unwrap_err().what,
            "[json.exception.out_of_range.403] key 'b' not found"
        );
        assert_eq!(
            value.at("a").unwrap().at_index(2).unwrap_err().what,
            "[json.exception.out_of_range.401] array index 2 is out of range"
        );
        assert_eq!(
            Json::from(1).at("a").unwrap_err().what,
            "[json.exception.type_error.304] cannot use at() with number"
        );
        assert_eq!(
            value.at_index(0).unwrap_err().what,
            "[json.exception.type_error.304] cannot use at() with object"
        );
    }

    #[test]
    fn get_converts_as_from_json_does() {
        assert_eq!(parse("2").get::<f64>(), Ok(2.0));
        assert_eq!(parse("2.75").get::<i32>(), Ok(2), "static_cast truncates");
        assert_eq!(parse("-1").get::<u32>(), Ok(u32::MAX), "modular");
        assert_eq!(
            parse("3000000000").get::<i32>(),
            Ok(-1_294_967_296),
            "3e9 - 2^32"
        );
        assert_eq!(parse("-1").get::<u64>(), Ok(u64::MAX));
        assert_eq!(parse("\"x\"").get::<String>(), Ok(String::from("x")));
        assert_eq!(parse("[1,2.5]").get::<Vec<f64>>(), Ok(vec![1.0, 2.5]));
        // int and std::uint32_t take a boolean (the generic from_json); double, std::int64_t and
        // std::uint64_t, the json's own number types, do not (get_arithmetic_value).
        assert_eq!(parse("true").get::<i32>(), Ok(1));
        assert_eq!(parse("false").get::<u32>(), Ok(0));
        assert_eq!(
            parse("true").get::<f64>().unwrap_err().what,
            "[json.exception.type_error.302] type must be number, but is boolean"
        );
        assert!(parse("true").get::<i64>().is_err());
        assert!(parse("true").get::<u64>().is_err());
    }

    #[test]
    fn get_of_the_wrong_type_is_type_error_302() {
        assert_eq!(
            parse("\"x\"").get::<f64>().unwrap_err().what,
            "[json.exception.type_error.302] type must be number, but is string"
        );
        assert_eq!(
            Json::Null.get::<bool>().unwrap_err().what,
            "[json.exception.type_error.302] type must be boolean, but is null"
        );
        assert_eq!(
            parse("1").get::<String>().unwrap_err().what,
            "[json.exception.type_error.302] type must be string, but is number"
        );
        assert_eq!(
            parse("{}").get::<Vec<f64>>().unwrap_err().what,
            "[json.exception.type_error.302] type must be array, but is object"
        );
        assert_eq!(
            parse(r#"[1,"a"]"#).get::<Vec<f64>>().unwrap_err().what,
            "[json.exception.type_error.302] type must be number, but is string"
        );
    }

    #[test]
    fn value_reads_a_member_or_gives_the_default() {
        let value = parse(r#"{"a":1,"s":"t"}"#);
        assert_eq!(value.value("a", 0_i32), Ok(1));
        assert_eq!(value.value("b", 7_i32), Ok(7));
        assert_eq!(value.value("s", String::new()), Ok(String::from("t")));
        assert_eq!(
            value.value("a", String::new()).unwrap_err().what,
            "[json.exception.type_error.302] type must be string, but is number"
        );
        assert_eq!(
            parse("[]").value("a", 1_i32).unwrap_err().what,
            "[json.exception.type_error.306] cannot use value() with array"
        );
    }

    #[test]
    fn size_empty_contains_and_find_follow_nlohmann() {
        assert_eq!(Json::Null.size(), 0);
        assert!(Json::Null.empty());
        assert_eq!(Json::from(5).size(), 1);
        assert!(!Json::from(5).empty());
        assert_eq!(parse("[]").size(), 0);
        assert!(parse("{}").empty());
        let value = parse(r#"{"a":null}"#);
        assert_eq!(value.size(), 1);
        assert!(value.contains("a"));
        assert!(!value.contains("b"));
        assert!(value.find("a").is_some_and(Json::is_null));
        assert!(!Json::from(5).contains("a"));
        assert!(Json::from(5).find("a").is_none());
    }

    #[test]
    fn iterating_visits_elements_values_or_the_value_itself() {
        let object = parse(r#"{"b":2,"a":1}"#);
        let values: Vec<u64> = object.iter().filter_map(Json::as_unsigned).collect();
        assert_eq!(values, [1, 2], "the values in key order");
        assert_eq!(Json::Null.iter().count(), 0);
        let primitive = Json::from("x");
        let visited: Vec<&Json> = primitive.iter().collect();
        assert_eq!(visited, [&primitive]);

        let array = parse("[5,6]");
        let items: Vec<(String, u64)> = array
            .items()
            .into_iter()
            .map(|(key, value)| (key.into_owned(), value.as_unsigned().unwrap()))
            .collect();
        assert_eq!(items, [(String::from("0"), 5), (String::from("1"), 6)]);
        let keys: Vec<String> = object
            .items()
            .into_iter()
            .map(|(key, _)| key.into_owned())
            .collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(primitive.items().len(), 1);
        assert_eq!(primitive.items()[0].0, "");
    }

    #[test]
    fn the_changes_refuse_a_value_of_the_wrong_type() {
        let mut list = Json::Null;
        list.push_back(Json::from(1)).unwrap();
        assert_eq!(list.dump(), "[1]", "null becomes an array");
        assert_eq!(
            Json::object().push_back(Json::from(1)).unwrap_err().what,
            "[json.exception.type_error.308] cannot use push_back() with object"
        );
        assert_eq!(
            parse("[]").erase("a").unwrap_err().what,
            "[json.exception.type_error.307] cannot use erase() with array"
        );
        let mut grown = Json::Null;
        grown[2] = Json::from(true);
        assert_eq!(
            grown.dump(),
            "[null,null,true]",
            "filled with nulls up to the index"
        );
        assert_eq!(
            JsonError::other_error(501, "a point must be an array of two numbers").what,
            "[json.exception.other_error.501] a point must be an array of two numbers"
        );
    }

    #[test]
    #[should_panic(
        expected = "[json.exception.type_error.305] cannot use operator[] with a string argument with number"
    )]
    fn indexing_a_number_by_a_key_panics_with_type_error_305() {
        let mut number = Json::from(1);
        number["a"] = Json::Null;
    }
}

// The events (parser.hpp's sax_parse_internal) and nlohmann's iterative parser and destructor.
mod json_events {
    use super::*;

    /// Records the events as text, the doubles apart, and the first error.
    #[derive(Default)]
    struct Events {
        seen: Vec<String>,
        doubles: Vec<f64>,
        error: Option<(usize, String, u32, String)>,
    }

    impl Sax for Events {
        fn null(&mut self) -> bool {
            self.seen.push(String::from("null"));
            true
        }
        fn boolean(&mut self, value: bool) -> bool {
            self.seen.push(format!("bool {value}"));
            true
        }
        fn number_integer(&mut self, value: i64) -> bool {
            self.seen.push(format!("integer {value}"));
            true
        }
        fn number_unsigned(&mut self, value: u64) -> bool {
            self.seen.push(format!("unsigned {value}"));
            true
        }
        fn number_float(&mut self, value: f64, written: &str) -> bool {
            self.seen.push(format!("float {written}"));
            self.doubles.push(value);
            true
        }
        fn string(&mut self, value: &mut String) -> bool {
            self.seen.push(format!("string {value}"));
            true
        }
        fn start_object(&mut self) -> bool {
            self.seen.push(String::from("{"));
            true
        }
        fn key(&mut self, value: &mut String) -> bool {
            self.seen.push(format!("key {value}"));
            true
        }
        fn end_object(&mut self) -> bool {
            self.seen.push(String::from("}"));
            true
        }
        fn start_array(&mut self) -> bool {
            self.seen.push(String::from("["));
            true
        }
        fn end_array(&mut self) -> bool {
            self.seen.push(String::from("]"));
            true
        }
        fn parse_error(&mut self, position: usize, token: &str, error: &JsonError) -> bool {
            self.error = Some((position, String::from(token), error.id, error.what.clone()));
            false
        }
    }

    fn events(text: &str) -> Events {
        let mut events = Events::default();
        sax_parse(text.as_bytes(), &mut events);
        events
    }

    // The events, the classification of numbers and the escapes, worked out from lexer.hpp's
    // scan_number and scan_string: "-0" is the integer 0, a fraction or an exponent makes a
    // double, an integer past 64 bits falls back to a double.
    #[test]
    fn the_events_are_nlohmanns() {
        let read = events(
            "{\"a\": [1, -0, -2, 1.5, 1e2, 18446744073709551616, true, null], \"b\\u00e9\": \"x\\ny\"}",
        );
        assert_eq!(read.error, None);
        assert_eq!(
            read.seen,
            [
                "{",
                "key a",
                "[",
                "unsigned 1",
                "integer 0",
                "integer -2",
                "float 1.5",
                "float 1e2",
                "float 18446744073709551616",
                "bool true",
                "null",
                "]",
                "key b\u{e9}",
                "string x\ny",
                "}",
            ]
        );
        // 2^64, one past the largest unsigned, is a double exactly.
        assert_eq!(read.doubles, [1.5, 100.0, 2.0_f64.powi(64)]);
    }

    // By hand from position_t and unget(): "[0,\n" is four bytes, and the number's last
    // character is the ninth byte read.
    #[test]
    fn a_number_past_the_largest_double_stops_the_parse_where_it_ends() {
        let read = events("[0,\n1e400]");
        let (position, token, id, what) = read.error.expect("an overflow");
        assert_eq!(position, 9);
        assert_eq!(token, "1e400");
        assert_eq!(id, 406);
        assert_eq!(
            what,
            "[json.exception.out_of_range.406] number overflow parsing '1e400'"
        );
    }

    // By hand: after "1" the lexer reads the line break and the blanks of line 4, and stops at
    // the closing quote of "name", column 8.
    #[test]
    fn a_syntax_error_says_its_line_and_column_as_nlohmann_does() {
        let read = events("{\n  \"format\": \"x\",\n  \"version\": 1\n  \"name\": \"T\"\n}\n");
        let (_, _, id, what) = read.error.expect("a syntax error");
        assert_eq!(id, 101);
        assert_eq!(
            what,
            "[json.exception.parse_error.101] parse error at line 4, column 8: syntax error while parsing object - unexpected string literal; expected '}'"
        );
        let empty = events("");
        assert_eq!(
            empty.error.expect("an empty input").3,
            "[json.exception.parse_error.101] parse error at line 1, column 1: attempting to parse an empty input; check that your input string or stream contains the expected JSON"
        );
        let literal = events("not json");
        assert_eq!(
            literal.error.expect("an invalid literal").3,
            "[json.exception.parse_error.101] parse error at line 1, column 2: syntax error while parsing value - invalid literal; last read: 'no'"
        );
    }

    #[test]
    fn a_text_nested_a_hundred_thousand_deep_is_read() {
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        let read = events(&deep);
        assert_eq!(read.error, None);
        assert_eq!(read.seen.len(), 200_000);
    }

    // json_value::destroy releases the children from a list, so neither reading nor dropping
    // the tree spends a level of the stack on a level of the document (a test thread has 2 MiB).
    #[test]
    fn a_tree_nested_a_hundred_thousand_deep_is_built_and_released() {
        let arrays = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        let tree = Json::parse(&arrays).unwrap();
        assert!(tree.is_array());
        assert_eq!(tree.size(), 1);
        drop(tree);
        let objects = format!("{}1{}", "{\"k\":".repeat(100_000), "}".repeat(100_000));
        let tree = OrderedJson::parse(&objects).unwrap();
        assert!(tree.find("k").is_some_and(OrderedJson::is_object));
        drop(tree);
        // Unclosed: refused, and what was open released.
        assert!(Json::parse("[".repeat(100_000)).is_err());
    }

    /// Hands every event on to a tree builder and counts the levels, as the update layer's watch
    /// does.
    struct Deepest {
        tree: DomBuilder<katana_core::json::SortedMap>,
        depth: usize,
        deepest: usize,
    }

    impl Sax for Deepest {
        fn null(&mut self) -> bool {
            self.tree.null()
        }
        fn boolean(&mut self, value: bool) -> bool {
            self.tree.boolean(value)
        }
        fn number_integer(&mut self, value: i64) -> bool {
            self.tree.number_integer(value)
        }
        fn number_unsigned(&mut self, value: u64) -> bool {
            self.tree.number_unsigned(value)
        }
        fn number_float(&mut self, value: f64, written: &str) -> bool {
            self.tree.number_float(value, written)
        }
        fn string(&mut self, value: &mut String) -> bool {
            self.tree.string(value)
        }
        fn start_object(&mut self) -> bool {
            self.depth += 1;
            self.deepest = self.deepest.max(self.depth);
            self.tree.start_object()
        }
        fn key(&mut self, value: &mut String) -> bool {
            self.tree.key(value)
        }
        fn end_object(&mut self) -> bool {
            self.depth -= 1;
            self.tree.end_object()
        }
        fn start_array(&mut self) -> bool {
            self.depth += 1;
            self.deepest = self.deepest.max(self.depth);
            self.tree.start_array()
        }
        fn end_array(&mut self) -> bool {
            self.depth -= 1;
            self.tree.end_array()
        }
        fn parse_error(&mut self, position: usize, token: &str, error: &JsonError) -> bool {
            self.tree.parse_error(position, token, error)
        }
    }

    #[test]
    fn a_tree_builder_wrapped_in_another_handler_builds_the_same_tree() {
        let text = r#"{"b":[1,{"c":null}],"a":"x","b":2.5}"#;
        let mut watch = Deepest {
            tree: DomBuilder::new(),
            depth: 0,
            deepest: 0,
        };
        assert!(sax_parse(text.as_bytes(), &mut watch));
        assert_eq!(watch.deepest, 3);
        let tree = watch.tree.into_value();
        assert_eq!(tree.dump(), parse(text).dump());
        assert_eq!(tree.dump(), r#"{"a":"x","b":2.5}"#);

        let mut failed = Deepest {
            tree: DomBuilder::new(),
            depth: 0,
            deepest: 0,
        };
        assert!(!sax_parse(b"[1,", &mut failed));
        let error = failed.tree.into_error().expect("the parse error is kept");
        assert_eq!(error.kind, JsonErrorKind::ParseError);
        assert_eq!(error.byte, 4);
    }
}
