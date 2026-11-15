//! The survey-code manager's helpers below the window: the code list as CSV, the key a new rule
//! for a drawing's code starts from, and attribute lists as text
//! (tests/cad/customisation/test_code_edit.cpp). Every expected string is written out by hand
//! from the rules the test builds, with the working beside it - never captured from a run.
//!
//! The code list's last column is headed "surface", the Katana customisation format's word for
//! what a survey code file calls `tinable`: the three headers below were changed from the table
//! "The words a rule is shown by" in docs/customisation.md, not from what a run printed.

use katana_cad::code_edit::{
    code_list_csv, csv_field, format_attribute_lines, parse_attribute_lines, suggested_key,
};
use katana_entity::{
    SurveyAttribute, SurveyBreakline, SurveyMap, SurveyRule, SurveySection, SurveySymbol,
};

fn attribute(kind: &str, name: &str, value: &str) -> SurveyAttribute {
    SurveyAttribute {
        r#type: String::from(kind),
        name: String::from(name),
        value: String::from(value),
    }
}

#[track_caller]
fn add(map: &mut SurveyMap, rule: SurveyRule) {
    if let Err(error) = map.add(rule) {
        panic!("{}", error.describe());
    }
}

mod code_list_csv {
    use super::*;

    #[test]
    fn a_field_is_quoted_only_when_it_holds_a_comma_a_quote_or_a_line_break_and_its_quotes_are_doubled()
     {
        assert_eq!(csv_field("Water main"), "Water main");
        assert_eq!(csv_field(""), "");
        // Blanks are data: RFC 4180 does not quote for them, and nor does this.
        assert_eq!(csv_field(" padded "), " padded ");
        assert_eq!(csv_field("Kerb, barrier"), "\"Kerb, barrier\"");
        // 12" pipe -> the quote doubled, the whole enclosed: "12"" pipe"
        assert_eq!(csv_field("12\" pipe"), "\"12\"\" pipe\"");
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_field("cr\rhere"), "\"cr\rhere\"");
    }

    #[test]
    fn each_key_is_one_record_of_what_it_resolves_to_with_unset_fields_left_empty() {
        let mut map = SurveyMap::new();
        // WM*: a line code, whose comment holds a comma and a quote.
        add(
            &mut map,
            SurveyRule {
                key: String::from("WM*"),
                model: String::from("SURVEY SERVICES"),
                colour: String::from("blue"),
                breakline: Some(SurveyBreakline::Line),
                linestyle: String::from("WATR Main"),
                comment: String::from("Water main, 6\" cast iron"),
                group: String::from("SERVICES"),
                ..SurveyRule::default()
            },
        );
        // AC*: a point code, in two sections - feature and symbol.
        add(
            &mut map,
            SurveyRule {
                key: String::from("AC*"),
                model: String::from("FURNITURE"),
                colour: String::from("white"),
                breakline: Some(SurveyBreakline::Point),
                linestyle: String::from("0"),
                comment: String::from("Access chamber"),
                ..SurveyRule::default()
            },
        );
        add(
            &mut map,
            SurveyRule {
                key: String::from("AC*"),
                section: SurveySection::VertexSymbol,
                symbol: Some(SurveySymbol {
                    style: String::from("Survey Mark"),
                    size: 1.5,
                    ..SurveySymbol::default()
                }),
                ..SurveyRule::default()
            },
        );
        // TR*: a symbol whose size is 0, the map's "the definition's own size".
        add(
            &mut map,
            SurveyRule {
                key: String::from("TR*"),
                section: SurveySection::VertexSymbol,
                symbol: Some(SurveySymbol {
                    style: String::from("Tree"),
                    size: 0.0,
                    ..SurveySymbol::default()
                }),
                ..SurveyRule::default()
            },
        );
        // GS: surface only.
        add(
            &mut map,
            SurveyRule {
                key: String::from("GS"),
                section: SurveySection::Tinable,
                tinable: Some(true),
                ..SurveyRule::default()
            },
        );

        // Key order (byte order): "AC*" < "GS" < "TR*" < "WM*".
        //   AC*: comment, no group, layer, colour, point, "0", symbol, 1.5, no surface
        //   GS:  nothing but surface yes
        //   TR*: symbol "Tree", and its size 0 written as nothing
        //   WM*: the comment quoted, its quote doubled
        let expected = "code,description,group,layer,colour,line/point,linestyle,symbol,size,\
                        surface\r\nAC*,Access chamber,,FURNITURE,white,point,0,Survey \
                        Mark,1.5,\r\nGS,,,,,,,,,yes\r\nTR*,,,,,,,Tree,,\r\nWM*,\"Water main, \
                        6\"\" cast iron\",SERVICES,SURVEY SERVICES,blue,line,WATR Main,,,\r\n";
        assert_eq!(code_list_csv(&map), expected);
    }

    #[test]
    fn a_key_caught_by_a_less_specific_key_takes_what_that_key_says_and_it_does_not_say_itself() {
        let mut map = SurveyMap::new();
        add(
            &mut map,
            SurveyRule {
                key: String::from("W*"),
                model: String::from("WATER"),
                group: String::from("SERVICES"),
                ..SurveyRule::default()
            },
        );
        add(
            &mut map,
            SurveyRule {
                key: String::from("WM*"),
                comment: String::from("Water main"),
                ..SurveyRule::default()
            },
        );
        // W*: its own model and group. WM*: its own comment, and W*'s model and
        // group, because lookup("WM*") meets both keys - WM* first.
        assert_eq!(
            code_list_csv(&map),
            "code,description,group,layer,colour,line/point,linestyle,symbol,size,surface\r\nW*,,\
             SERVICES,WATER,,,,,,\r\nWM*,Water main,SERVICES,WATER,,,,,,\r\n"
        );
    }

    #[test]
    fn an_empty_map_is_the_header_alone() {
        assert_eq!(
            code_list_csv(&SurveyMap::new()),
            "code,description,group,layer,colour,line/point,linestyle,symbol,size,surface\r\n"
        );
    }
}

mod suggested_key {
    use super::*;

    #[test]
    fn a_code_ending_in_a_string_number_gets_a_prefix_key_on_what_comes_before_it() {
        assert_eq!(suggested_key("WM01"), "WM*");
        assert_eq!(suggested_key("KB12"), "KB*");
        // The blank between the code and its number belongs to neither.
        assert_eq!(suggested_key("KB 12"), "KB*");
        // Only the TRAILING number is the string number.
        assert_eq!(suggested_key("P2X7"), "P2X*");
    }

    #[test]
    fn a_code_with_no_number_or_nothing_but_a_number_stays_exact_and_is_trimmed() {
        assert_eq!(suggested_key("PABB"), "PABB");
        assert_eq!(suggested_key("  PABB "), "PABB");
        // "*" alone would catch every code.
        assert_eq!(suggested_key("105"), "105");
        assert_eq!(suggested_key("   "), "");
    }
}

mod attribute_lines {
    use super::*;

    #[test]
    fn attributes_become_one_line_each_and_read_back_as_themselves() {
        let attributes = vec![
            attribute("text", "Source", "Field survey"),
            attribute("text", "Diameter", "$PipeDiameter"),
            attribute("integer", "Pit Number", "12"),
        ];
        let text = format_attribute_lines(&attributes);
        assert_eq!(
            text,
            "text Source = Field survey\ntext Diameter = $PipeDiameter\ninteger Pit Number = 12\n"
        );
        let back =
            parse_attribute_lines(&text).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(back, attributes);
    }

    #[test]
    fn blank_lines_are_skipped_and_the_blanks_around_the_equals_sign_are_not_data() {
        let parsed = parse_attribute_lines("\n  text   Source=Survey = 2 \r\n\n")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        // The FIRST '=' ends the name; the value keeps its own '='.
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], attribute("text", "Source", "Survey = 2"));
        let none = parse_attribute_lines("").expect("nothing is no attribute");
        assert!(none.is_empty());
    }

    #[test]
    fn a_line_with_no_equals_no_name_or_an_unknown_type_is_refused_naming_the_line() {
        let no_equals = parse_attribute_lines("text A = 1\ntext B 2\n")
            .expect_err("the second line has no '='");
        assert!(
            no_equals.describe().contains("attribute line 2"),
            "{}",
            no_equals.describe()
        );
        assert!(parse_attribute_lines("text = 1").is_err());
        assert!(parse_attribute_lines("real Height = 1.5").is_err());
        assert!(parse_attribute_lines("Source = Survey").is_err());
    }
}
