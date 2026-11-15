//! definition_edit: a definition's strokes as the text an editor shows and takes back, the name a
//! copy starts under, and the line that removes one
//! (tests/cad/customisation/test_definition_edit.cpp).
//!
//! Every definition expected here is built by hand, and every text is written out by the rules of
//! docs/customisation.md ("Layout"): one stroke a line, `["move", x, y]` with a blank after each
//! comma, a number as its shortest text, a comma ending every line but the last.

use katana_cad::definition_edit::{
    free_definition_name, read_stroke_text, remove_definition_line, stroke_text,
};
use katana_cad::{CommandInterpreter, Document, StrokeTextRead};
use katana_commands::create_linetype;
use katana_core::ErrorCode;
use katana_entity::{
    LineStyle, Linetype, LinetypeElement, Stroke, StrokeOp, StrokeText, StyleLibrary, StyleUnits,
};
use katana_geometry::Point2;

use crate::must;

fn move_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Move,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn draw(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Draw,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

// A paper linestyle of length 12 with no strokes: the members a text is read
// under.
fn kerb_members() -> LineStyle {
    LineStyle {
        name: String::from("TEST Kerb"),
        group: String::from("Test/Lines"),
        units: StyleUnits::Paper,
        length: 12.0,
        source: String::from("Site"),
        ..LineStyle::default()
    }
}

// The definition a text read as, failing the test with the refusal when it did not read.
#[track_caller]
fn definition_of(read: &StrokeTextRead) -> &LineStyle {
    match &read.definition {
        Some(definition) => definition,
        None => panic!("{}", read.problem),
    }
}

mod definition_edit {
    use super::*;

    #[test]
    fn the_strokes_of_a_definition_are_the_lines_a_file_holds_one_stroke_a_line() {
        let mut kerb = kerb_members();
        let letter = StrokeText {
            text: String::from("W"),
            height: 1.5,
            ..StrokeText::default()
        };
        kerb.texts = vec![letter];
        kerb.strokes = vec![
            move_to(0.0, 0.0),
            draw(8.0, 0.0),
            Stroke {
                op: StrokeOp::Circle,
                radius: 0.5,
                ..Stroke::default()
            },
            Stroke {
                op: StrokeOp::Text,
                text: 0,
                ..Stroke::default()
            },
        ];
        let text = stroke_text(&kerb).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            text,
            concat!(
                "[\"move\", 0, 0],\n",
                "[\"draw\", 8, 0],\n",
                "[\"circle\", 0.5],\n",
                "[\"text\", {\"text\": \"W\", \"height\": 1.5}]"
            )
        );

        // No strokes, no lines - not the members' own line.
        let none =
            stroke_text(&kerb_members()).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(none, "");

        // What the format cannot write has no text: a move that carries a radius.
        let mut stray = kerb_members();
        stray.strokes = vec![Stroke {
            op: StrokeOp::Move,
            point: Point2::new(0.0, 0.0),
            radius: 2.0,
            ..Stroke::default()
        }];
        let refused = stroke_text(&stray).expect_err("a move that carries a radius");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn text_reads_as_the_definition_its_members_and_its_lines_describe() {
        // The second line without the comma a file would end it with, a blank
        // line passed over, Windows line ends, and blanks round a line.
        let read = read_stroke_text(
            &kerb_members(),
            concat!(
                "[\"move\", 0, 0],\r\n",
                "  [\"draw\", 8, 0]  \r\n",
                "\r\n",
                "[\"dot\", 0.25],\r\n"
            ),
        );
        let mut expected = kerb_members();
        expected.strokes = vec![
            move_to(0.0, 0.0),
            draw(8.0, 0.0),
            Stroke {
                op: StrokeOp::Dot,
                radius: 0.25,
                ..Stroke::default()
            },
        ];
        assert_eq!(*definition_of(&read), expected);
        assert_eq!(read.line, 0);
        assert!(read.problem.is_empty());

        // The members' own strokes and texts are not looked at: the text is all
        // the strokes there are, and no text is no strokes.
        let mut with_old = kerb_members();
        with_old.strokes = vec![move_to(1.0, 1.0), draw(2.0, 2.0)];
        let none = read_stroke_text(&with_old, "\n  \n");
        assert_eq!(*definition_of(&none), kerb_members());

        // Which list holds it is the members' to say.
        let mut symbol = kerb_members();
        symbol.symbol = true;
        symbol.at_vertices = true;
        let as_symbol = read_stroke_text(&symbol, "[\"move\", 0, 0]");
        assert!(definition_of(&as_symbol).symbol);
        assert!(definition_of(&as_symbol).at_vertices);
    }

    #[test]
    fn a_text_of_a_stroke_is_kept_character_for_character() {
        // A no-break space (U+00A0) and a line separator (U+2028) are characters
        // of a text like any other; neither is a blank or a line end of the
        // lines themselves.
        let label = "A\u{a0}B\u{2028}C";
        let read = read_stroke_text(
            &kerb_members(),
            &format!("[\"move\", 0, 0],\n[\"text\", {{\"text\": \"{label}\", \"height\": 1}}]"),
        );
        let definition = definition_of(&read);
        assert_eq!(definition.texts.len(), 1);
        assert_eq!(definition.texts[0].text, label);
        // And written back, the same bytes.
        let text = stroke_text(definition).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(text.contains(label), "{text}");
    }

    #[test]
    fn a_stroke_the_reader_refuses_is_named_by_its_line_in_the_readers_own_words() {
        // A word that is no kind of stroke on the third line: the reader names
        // the stroke by its place counted from 0, and the line is put in front.
        let typo = read_stroke_text(
            &kerb_members(),
            concat!(
                "[\"move\", -1, 0],\n",
                "[\"draw\", 1, 0],\n",
                "[\"drow\", 0, -1],\n",
                "[\"draw\", 0, 1]"
            ),
        );
        assert!(typo.definition.is_none());
        assert_eq!(typo.line, 3);
        assert!(
            typo.problem.starts_with(
                "Line 3: definition \"TEST Kerb\" strokes[2]: \"drow\" is not a kind of stroke"
            ),
            "{}",
            typo.problem
        );
        assert_eq!(typo.error.code, ErrorCode::ParseFailure);

        // The line is the text's own, not the stroke's place: a blank line above
        // moves the one and not the other.
        let short = read_stroke_text(
            &kerb_members(),
            concat!("[\"move\", 0, 0],\n", "\n", "[\"arc\", 1, 90]"),
        );
        assert_eq!(short.line, 3);
        assert!(
            short.problem.starts_with(concat!(
                "Line 3: definition \"TEST Kerb\" strokes[1]: \"arc\" takes a radius, a start ",
                "angle and an end angle (3 after the word) and has 2"
            )),
            "{}",
            short.problem
        );

        // What entity's validate refuses of a stroke is named by its line too.
        let below = read_stroke_text(
            &kerb_members(),
            "[\"move\", 0, 0],\n[\"text\", {\"text\": \"W\", \"height\": -1}]",
        );
        assert_eq!(below.line, 2);
        assert!(
            below.problem.contains("linestyle text height is negative"),
            "{}",
            below.problem
        );
    }

    #[test]
    fn a_refusal_never_cites_a_place_in_text_nobody_typed() {
        // The lines are read under a first line of this function's making, so
        // the reader's "line 2, column 14" is of a text the person never saw.

        // A number too large to hold. The reader's words are `this stroke has a
        // number too large to hold`, then what was written, then its place - and
        // the place is what is left out.
        let large = read_stroke_text(&kerb_members(), "[\"move\", 1e999, 0]");
        assert_eq!(large.line, 1);
        assert_eq!(
            large.problem,
            concat!(
                "Line 1: definition \"TEST Kerb\" strokes[0]: this stroke has a ",
                "number too large to hold - 1e999"
            )
        );

        // A line missing its closing bracket is short of a "]" - not, as the
        // text it is read in would have it, met by a "}" nobody typed.
        let open = read_stroke_text(&kerb_members(), "[\"move\", 0, 0");
        assert_eq!(open.line, 1);
        assert!(
            open.problem.starts_with("Line 1 is not a stroke: "),
            "{}",
            open.problem
        );
        assert!(open.problem.contains("expected ']'"), "{}", open.problem);
        assert!(!open.problem.contains('}'), "{}", open.problem);

        // A word cut short - which every word of a stroke is while it is being
        // typed - is a string with no closing quote, and is said of what was
        // typed: no bracket of the text it is read in runs on into it.
        let cut = read_stroke_text(&kerb_members(), "[\"move\", 0, 0],\n[\"dr");
        assert_eq!(cut.line, 2);
        assert!(
            cut.problem.starts_with("Line 2 is not a stroke: "),
            "{}",
            cut.problem
        );
        assert!(cut.problem.contains("\"dr"), "{}", cut.problem);
        assert!(!cut.problem.contains(']'), "{}", cut.problem);
        assert!(!cut.problem.contains('}'), "{}", cut.problem);

        // Two strokes on one line are joined like any two, so a fault in the
        // second is said of it - not of there being something after the first.
        let two = read_stroke_text(&kerb_members(), "[\"move\", 0, 0], [\"draw\", 1 0]");
        assert_eq!(two.line, 1);
        assert!(
            two.problem.starts_with("Line 1 is not a stroke: "),
            "{}",
            two.problem
        );
        assert!(!two.problem.contains("end of input"), "{}", two.problem);
        let two_sound = read_stroke_text(&kerb_members(), "[\"move\", 0, 0], [\"draw\", 1, 0]");
        assert_eq!(definition_of(&two_sound).strokes.len(), 2);

        // Text that is not JSON, on the fourth line of the text.
        let comma = read_stroke_text(
            &kerb_members(),
            concat!(
                "[\"move\", -1, 0],\n",
                "\n",
                "[\"draw\", 1, 0],\n",
                "[\"draw\", 0 1]"
            ),
        );
        assert_eq!(comma.line, 4);
        assert!(
            comma.problem.starts_with("Line 4 is not a stroke: "),
            "{}",
            comma.problem
        );

        // A line that is nothing but a comma is not a stroke either, and is not
        // passed over as a blank one is.
        let lone = read_stroke_text(&kerb_members(), "[\"move\", 0, 0],\n,\n[\"draw\", 1, 0]");
        assert_eq!(lone.line, 2);

        // JSON has no word for a number that is not one.
        for not_a_number in ["inf", "nan", "NaN", "Infinity", "-inf"] {
            let read = read_stroke_text(
                &kerb_members(),
                &format!("[\"move\", 0, 0],\n[\"draw\", {not_a_number}, 0]"),
            );
            assert!(read.definition.is_none(), "{not_a_number}");
            assert_eq!(read.line, 2, "{not_a_number}");
            assert!(
                read.problem.starts_with("Line 2 is not a stroke: "),
                "{}",
                read.problem
            );
        }

        for each in [&large, &open, &cut, &two, &comma, &lone] {
            assert!(!each.problem.contains("column"), "{}", each.problem);
            assert!(!each.problem.contains(" at line "), "{}", each.problem);
            assert!(each.definition.is_none());
        }
    }

    #[test]
    fn text_that_is_more_than_strokes_and_members_that_are_no_definition_are_refused_of_no_line() {
        // A line that ends the list of strokes and goes on to give members: sound
        // JSON, and a definition at vertices under members that say it is not.
        let more = read_stroke_text(
            &kerb_members(),
            "[\"move\", 0, 0]], \"atVertices\": true, \"anchors\": [[0, 0], [1, 1]",
        );
        assert!(more.definition.is_none());
        assert_eq!(more.line, 0);
        assert!(
            more.problem
                .starts_with("The strokes hold more than strokes"),
            "{}",
            more.problem
        );
        assert_eq!(more.error.code, ErrorCode::InvalidArgument);

        // entity's validate's two rules for a definition's own numbers, in its
        // words; they are a member's, so no line is named.
        let mut negative = kerb_members();
        negative.length = -1.0;
        let length = read_stroke_text(&negative, "[\"move\", 0, 0]");
        assert!(length.definition.is_none());
        assert_eq!(length.line, 0);
        assert!(
            length
                .problem
                .contains("linestyle length must be finite and not negative"),
            "{}",
            length.problem
        );
        let mut flat = kerb_members();
        flat.factor = 0.0;
        let factor = read_stroke_text(&flat, "");
        assert!(factor.definition.is_none());
        assert!(
            factor
                .problem
                .contains("linestyle factor must be finite and greater than zero"),
            "{}",
            factor.problem
        );
    }

    #[test]
    fn a_copy_starts_under_the_first_name_neither_the_library_nor_the_drawing_holds() {
        let mut document = Document::default();
        let mut library = StyleLibrary::default();
        for name in ["TEST Valve", "TEST Valve 2"] {
            let definition = LineStyle {
                name: String::from(name),
                ..LineStyle::default()
            };
            assert!(library.add(definition).is_ok(), "{name}");
        }
        document.set_style_library(library);
        // The drawing's own linetype table holds the next: a library linestyle of
        // that name would be drawn in its place (decision D2).
        let dashes = Linetype {
            name: String::from("TEST Valve 3"),
            pattern: vec![
                LinetypeElement { length: 2.0 },
                LinetypeElement { length: -1.0 },
            ],
            ..Linetype::default()
        };
        must(document.execute(create_linetype(dashes)));

        assert_eq!(
            free_definition_name(&document, "TEST Valve"),
            "TEST Valve 4"
        );
        assert_eq!(
            free_definition_name(&document, "TEST Valve 2"),
            "TEST Valve 2 2"
        );
        // A name that is free is itself the answer.
        assert_eq!(
            free_definition_name(&document, "TEST Hydrant"),
            "TEST Hydrant"
        );
        assert_eq!(
            free_definition_name(&Document::default(), "TEST Valve"),
            "TEST Valve"
        );
    }

    #[test]
    fn the_remove_line_names_the_definition_in_quotes_and_reads_back_as_its_words() {
        let plain = remove_definition_line("TEST Valve", false)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(plain, "CUSTOMISE REMOVE \"TEST Valve\"");
        let forced = remove_definition_line("TEST Valve", true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(forced, "CUSTOMISE REMOVE \"TEST Valve\" FORCE");
        // A name with no blank is quoted all the same, as the line is documented.
        let one_word = remove_definition_line("Kerb", false)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(one_word, "CUSTOMISE REMOVE \"Kerb\"");

        // What the interpreter makes of the line is the three or four words meant.
        let words = CommandInterpreter::tokenize(&forced)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(words, ["CUSTOMISE", "REMOVE", "TEST Valve", "FORCE"]);
        // A name that only begins with one of the line's words is a name.
        let force_main = remove_definition_line("Force main", true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let main_words = CommandInterpreter::tokenize(&force_main)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(main_words, ["CUSTOMISE", "REMOVE", "Force main", "FORCE"]);
        assert!(remove_definition_line("CODES", false).is_ok());
    }

    #[test]
    fn a_name_no_line_can_name_is_refused_rather_than_written_as_something_else() {
        // A word of a command line has no escape for a double quote: the line
        // would run on another name, cut short at the quote.
        let quoted = remove_definition_line("TEST 6\" Pipe", false).expect_err("a double quote");
        assert_eq!(quoted.code, ErrorCode::InvalidArgument);
        assert_eq!(
            quoted.message,
            "a double quote cannot be written on a command line"
        );
        assert!(remove_definition_line("two\nlines", false).is_err());

        // The quotes do not survive the tokenizer, so a definition named as one
        // of the line's own words is read as that word: `CUSTOMISE REMOVE "CODE"
        // FORCE` would remove the survey code FORCE.
        for word in ["CODE", "code", "Code", "FORCE", "force"] {
            for force in [false, true] {
                let refused = remove_definition_line(word, force).expect_err(word);
                assert_eq!(refused.code, ErrorCode::InvalidArgument);
                assert!(
                    refused.message.contains("CUSTOMISE REMOVE"),
                    "{}",
                    refused.message
                );
            }
        }
        // No name is no definition.
        assert!(remove_definition_line("", false).is_err());
    }
}
