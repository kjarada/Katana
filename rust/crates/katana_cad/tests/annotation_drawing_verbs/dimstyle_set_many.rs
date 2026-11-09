//! tests/cad/annotation/test_dimstyle_set_many.cpp: DIMSTYLE with several
//! fields on one line (annotation/dimension_style_verbs.rs, docs/annotation.md
//! "Dimension styles"): what the window's Dimension Styles manager sends when
//! Apply or Duplicate is pressed, so each is ONE undo step, and a line with one
//! bad pair changes nothing. Also the words a dialog puts on a line
//! (command_words.rs), which must read back as the value shown.

use katana_cad::CommandInterpreter;
use katana_cad::annotation::command_words::{annotation_text_word, command_word, record_value};
use katana_cad::annotation::dimension_style_verbs::{
    dimension_style_changes, dimension_style_field_value, dimension_style_fields,
    layers_using_dimension_style, set_dimension_style_field,
};
use katana_core::ErrorCode;
use katana_entity::{ArrowHead, DimensionStyle, Geometry};

use crate::{Session, double_eq};

#[track_caller]
fn style(s: &Session, name: &str) -> DimensionStyle {
    match s.document.model().dimension_styles.find(name) {
        Some(style) => style.clone(),
        None => panic!("no dimension style {name}"),
    }
}

// DimensionStyle{.name = name}: every field at its default.
fn named(name: &str) -> DimensionStyle {
    DimensionStyle {
        name: String::from(name),
        ..DimensionStyle::default()
    }
}

mod dimstyle_set_many {
    use super::*;

    #[test]
    fn several_pairs_on_one_line_are_one_undo_step() {
        let mut s = Session::default();
        s.run("DIMSTYLE NEW site");
        let before = s.steps();
        s.run("DIMSTYLE SET site TEXT 3.5 GAP 0.8 HEAD Tick PREFIX \"L= \" PAPER on DECIMALS 2");
        assert_eq!(s.steps(), before + 1, "six fields, one step");
        let site = style(&s, "site");
        double_eq(site.text_height, 3.5);
        double_eq(site.text_gap, 0.8);
        assert_eq!(site.arrow_head, ArrowHead::Tick);
        assert_eq!(site.prefix, "L= ");
        assert!(site.paper_sized);
        assert_eq!(site.decimals, 2);

        // One UNDO takes back every field the line set.
        s.run("UNDO");
        assert_eq!(style(&s, "site"), named("site"));
    }

    #[test]
    fn one_bad_pair_refuses_the_whole_line_and_changes_nothing() {
        let mut s = Session::default();
        s.run("DIMSTYLE NEW site");
        let before = s.steps();
        assert_eq!(
            s.fails("DIMSTYLE SET site TEXT 3.5 HEAD Diamond"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            s.fails("DIMSTYLE SET site TEXT 3.5 NOSUCH 1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            s.fails("DIMSTYLE SET site TEXT 3.5 GAP"),
            ErrorCode::InvalidArgument,
            "a field with no value"
        );
        assert_eq!(
            s.fails("DIMSTYLE SET site TEXT 3.5 ARROW 0"),
            ErrorCode::InvalidArgument,
            "every pair reads, but the style it makes is invalid"
        );
        assert_eq!(
            s.fails("DIMSTYLE SET site TEXT"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(s.fails("DIMSTYLE SET site"), ErrorCode::InvalidArgument);
        assert_eq!(
            style(&s, "site"),
            named("site"),
            "the TEXT before each was kept"
        );
        assert_eq!(s.steps(), before);
    }

    #[test]
    fn switches_and_places_are_read_strictly() {
        // "PAPER of" once turned paper sizing off without a word, and
        // "DECIMALS 2.7" gave two places.
        let mut s = Session::default();
        s.run("DIMSTYLE NEW site PAPER on");
        assert_eq!(
            s.fails("DIMSTYLE SET site PAPER of"),
            ErrorCode::ParseFailure
        );
        assert!(style(&s, "site").paper_sized);
        assert_eq!(
            s.fails("DIMSTYLE SET site DECIMALS 2.7"),
            ErrorCode::ParseFailure
        );
        s.run("DIMSTYLE SET site TRIM yes PAPER no");
        assert!(style(&s, "site").suppress_trailing_zeros);
        assert!(!style(&s, "site").paper_sized);
    }

    #[test]
    fn new_with_fields_makes_the_style_in_one_step() {
        let mut s = Session::default();
        let before = s.steps();
        s.run("DIMSTYLE NEW copy TEXT 5 SUFFIX mm SCALE 1000");
        assert_eq!(s.steps(), before + 1);
        double_eq(style(&s, "copy").text_height, 5.0);
        assert_eq!(style(&s, "copy").suffix, "mm");
        s.run("UNDO");
        assert!(!s.document.model().dimension_styles.contains("copy"));
        assert_eq!(
            s.fails("DIMSTYLE NEW bad TEXT 5 HEAD Diamond"),
            ErrorCode::ParseFailure
        );
        assert!(!s.document.model().dimension_styles.contains("bad"));
    }

    #[test]
    fn info_is_one_record_of_every_field() {
        // The defaults are DimensionStyle's (tables.rs: DIMTXT 2.5, DIMGAP and
        // DIMEXO 0.625, DIMEXE 1.25, DIMASZ 2.5, three places), and ten units
        // at three places read "10.000".
        let mut s = Session::default();
        assert_eq!(
            s.run("DIMSTYLE INFO Standard"),
            concat!(
                "name=Standard text=2.5 gap=0.625 extoff=0.625 extbeyond=1.25 arrow=2.5 ",
                "head=ClosedFilled scale=1 decimals=3 round=0 prefix=\"\" suffix=\"\" trim=off ",
                "paper=off layers=0 reads=10.000"
            )
        );
        s.run("DIMSTYLE NEW site SCALE 1000 DECIMALS 1 SUFFIX mm");
        s.run("LAYER NEW dims");
        s.run("LAYER DIMSTYLE dims site");
        let info = s.run("DIMSTYLE INFO site");
        assert!(info.contains(" layers=1 "), "{info}");
        assert!(info.contains(" reads=10000.0mm"), "{info}");
        assert_eq!(s.fails("DIMSTYLE INFO missing"), ErrorCode::NotFound);
        assert_eq!(
            layers_using_dimension_style(s.document.model(), "site"),
            ["dims"]
        );
    }

    #[test]
    fn the_changes_between_two_styles_read_back_as_the_second() {
        let mut s = Session::default();
        s.run("DIMSTYLE NEW site");
        let from = style(&s, "site");
        let mut to = from.clone();
        to.text_height = 1.8;
        to.text_gap = 0.1 + 0.2; // not a short decimal: it must come back exactly
        to.arrow_head = ArrowHead::Open;
        to.decimals = 1;
        to.round_to = 0.05;
        to.prefix = String::from("R ");
        to.suffix = String::from("m");
        to.paper_sized = true;

        let changes = match dimension_style_changes(&from, &to) {
            Ok(changes) => changes,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(
            !changes.contains("EXTOFF"),
            "an unchanged field is not written"
        );
        assert!(changes.contains("PREFIX \"R \""), "{changes}");
        s.run(&format!("DIMSTYLE SET site {changes}"));
        assert_eq!(style(&s, "site"), to);

        // Back again, the suffix emptied: an empty word is "".
        let back = match dimension_style_changes(&to, &from) {
            Ok(back) => back,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(back.contains("SUFFIX \"\""), "{back}");
        s.run(&format!("DIMSTYLE SET site {back}"));
        assert_eq!(style(&s, "site"), from);

        assert_eq!(
            dimension_style_changes(&from, &from).ok().as_deref(),
            Some(""),
            "nothing to set"
        );
        let mut quoted = from.clone();
        quoted.suffix = String::from("\"");
        assert!(
            dimension_style_changes(&from, &quoted).is_err(),
            "a double quote cannot be written on a line, so it is refused, not cut short"
        );
    }

    #[test]
    fn every_field_has_a_value_that_sets_it_back() {
        // Each field written by dimension_style_field_value and read by
        // set_dimension_style_field gives the same style: the record and the
        // line speak one language.
        let mut style = DimensionStyle {
            name: String::from("x"),
            text_height: 3.25,
            arrow_head: ArrowHead::Dot,
            suppress_trailing_zeros: true,
            prefix: String::from("~"),
            ..DimensionStyle::default()
        };
        let mut copy = named("x");
        for &field in dimension_style_fields() {
            let value = dimension_style_field_value(&style, field);
            if let Err(error) = set_dimension_style_field(&mut copy, field, &value) {
                panic!("{field}: {}", error.describe());
            }
        }
        assert_eq!(copy, style);
        assert_eq!(
            dimension_style_fields().len(),
            13,
            "TEXT GAP EXTOFF EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND PREFIX SUFFIX TRIM PAPER"
        );
        assert_eq!(
            set_dimension_style_field(&mut style, "colour", "red")
                .expect_err("colour is no dimension style field")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_help_gives_the_many_pair_set() {
        let help = CommandInterpreter::help_text();
        assert!(help.contains("DIMSTYLE SET name field value [field value ...]"));
        assert!(help.contains("INFO name"));
    }
}

mod command_words {
    use super::*;

    #[test]
    fn a_value_is_quoted_only_when_it_must_be_and_refused_when_it_cannot_be() {
        assert_eq!(command_word("Notes").ok().as_deref(), Some("Notes"));
        assert_eq!(
            command_word("Road names").ok().as_deref(),
            Some("\"Road names\"")
        );
        assert_eq!(command_word("").ok().as_deref(), Some("\"\""));
        assert_eq!(command_word("a\tb").ok().as_deref(), Some("\"a\tb\""));
        assert_eq!(
            command_word("5\" pipe").expect_err("a double quote").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            command_word("two\nlines").expect_err("a line break").code,
            ErrorCode::InvalidArgument
        );

        // What the tokenizer reads back is the value.
        for value in ["Notes", "Road names", "", "L= "] {
            let Ok(word) = command_word(value) else {
                panic!("{value:?} has no word");
            };
            let Ok(words) = CommandInterpreter::tokenize(&format!("X {word}")) else {
                panic!("X {word} does not split");
            };
            assert_eq!(words.len(), 2, "{value}");
            assert_eq!(words[1], value);
        }
    }

    #[test]
    fn an_annotation_text_writes_its_line_breaks_as_backslash_n() {
        assert_eq!(
            annotation_text_word("PIT 12\nIL 10.50").ok().as_deref(),
            Some("\"PIT 12\\nIL 10.50\"")
        );
        assert_eq!(
            annotation_text_word("A\r\nB").ok().as_deref(),
            Some("A\\nB"),
            "a Windows line end is one break"
        );
        assert_eq!(
            annotation_text_word("C:\\new")
                .expect_err("a backslash before an n")
                .code,
            ErrorCode::InvalidArgument,
            "the verb would read the backslash and n as a break"
        );
        assert_eq!(
            annotation_text_word("say \"hi\"")
                .expect_err("a double quote")
                .code,
            ErrorCode::InvalidArgument
        );

        // Through the verb: the text it makes has the breaks.
        let mut s = Session::default();
        let Ok(word) = annotation_text_word("first line\nsecond") else {
            panic!("the text has no word");
        };
        s.run(&format!("TEXT 0,0 2.5 {word} justify=TL"));
        let entity = s.entity(1);
        let Geometry::Text(text) = entity.geometry else {
            panic!("entity 1 is not a text");
        };
        assert_eq!(text.text, "first line\nsecond");
        assert_eq!(record_value("a b"), "\"a b\"");
        assert_eq!(record_value("x=1"), "\"x=1\"");
        assert_eq!(record_value("plain"), "plain");
    }
}
