// What a PART of a customisation is written as
// (tests/cad/customisation/test_customisation_part.cpp): the one rule
// CUSTOMISE EXPORT with a kind word or ONLY, the Survey Code Manager's Export
// Codes and the Symbol Library's Export Selected all cut by.
//
// Nothing is read from a file and no verb or dialog is run: the session is
// the value built below, and every expectation is worked out by hand from it
// and from the rule as customisation_part states it.
//
//   name "Works", description "The works set.", notice "Works: all rights
//   reserved."; based on Works 0123456789abcdef; linework and automation said.
//
//   sources   Works   definitions, rules   no notice of its own (the
//                                          session's is its)
//             Client  rules                "Client codes, for this job only."
//             Marks   definitions          "Marks drawn by hand."
//             Tints   neither              "Tints: free to use."
//
//   colours   tint teal, tint rose, tint lime, tint plum
//
//   library   WORKS Peg    symbol, from Works   a pen "tint teal", a circle
//             WORKS Fence  linestyle, from Works   a move and a draw: no pen
//             MARK Cross   symbol, from Marks   a move and a draw: no pen
//             LOOSE Dot    symbol, made in a session (no source)
//                                               a pen "Tint_Rose", a dot
//
//   rules     #0 PG* feature   colour "tint teal"
//             #1 FN* feature   linestyle WORKS Fence, no colour
//             #2 MK* symbol    MARK Cross, its colour "TINT-LIME"
//             #3 TX* text      its colour "tint plum"
//
// A colour name is one name by its fold (entity's fold_colour_name: lower
// case, `_` and `-` as a blank), so "Tint_Rose" is tint rose and "TINT-LIME"
// tint lime; a table lists its entries in the order of the folded names.

use katana_cad::customisation_part::customisation_part;
use katana_core::ErrorCode;
use katana_entity::customisation::{customisation_from_json, customisation_to_json};
use katana_entity::{
    Color, Customisation, CustomisationAutomation, CustomisationBase, CustomisationSourceNote,
    CustomisationWriteOptions, LineStyle, LineworkCodes, Stroke, StrokeOp, SurveyRule,
    SurveySection, SurveySymbol, SurveyTextStyle,
};
use katana_geometry::Point2;

const OWN_NOTICE: &str = "Works: all rights reserved.";
const CLIENT_NOTICE: &str = "Client codes, for this job only.";
const MARKS_NOTICE: &str = "Marks drawn by hand.";
const TINTS_NOTICE: &str = "Tints: free to use.";

fn source(name: &str, definitions: bool, rules: bool, notice: &[&str]) -> CustomisationSourceNote {
    CustomisationSourceNote {
        name: String::from(name),
        definitions,
        rules,
        notice: lines(notice),
    }
}

fn lines(of: &[&str]) -> Vec<String> {
    of.iter().map(|&line| String::from(line)).collect()
}

// The C++ `move`, a keyword here.
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

fn pen(name: &str) -> Stroke {
    Stroke {
        op: StrokeOp::Pen,
        pen: String::from(name),
        ..Stroke::default()
    }
}

fn definition(name: &str, symbol: bool, from: &str, strokes: Vec<Stroke>) -> LineStyle {
    LineStyle {
        name: String::from(name),
        symbol,
        at_vertices: symbol,
        source: String::from(from),
        strokes,
        ..LineStyle::default()
    }
}

fn works() -> Customisation {
    let mut session = Customisation {
        name: String::from("Works"),
        description: String::from("The works set."),
        notice: lines(&[OWN_NOTICE]),
        sources: vec![
            source("Works", true, true, &[]),
            source("Client", false, true, &[CLIENT_NOTICE]),
            source("Marks", true, false, &[MARKS_NOTICE]),
            source("Tints", false, false, &[TINTS_NOTICE]),
        ],
        based_on: Some(CustomisationBase {
            name: String::from("Works"),
            digest: String::from("0123456789abcdef"),
        }),
        linework: Some(LineworkCodes::default()),
        automation: Some(CustomisationAutomation::default()),
        ..Customisation::default()
    };
    assert!(
        session
            .colours
            .add("tint teal", Color::new(0, 128, 128, 255))
            .is_ok()
    );
    assert!(
        session
            .colours
            .add("tint rose", Color::new(255, 0, 127, 255))
            .is_ok()
    );
    assert!(
        session
            .colours
            .add("tint lime", Color::new(0, 255, 0, 255))
            .is_ok()
    );
    assert!(
        session
            .colours
            .add("tint plum", Color::new(128, 0, 128, 255))
            .is_ok()
    );

    for each in [
        definition(
            "WORKS Peg",
            true,
            "Works",
            vec![
                pen("tint teal"),
                Stroke {
                    op: StrokeOp::Circle,
                    radius: 0.5,
                    ..Stroke::default()
                },
            ],
        ),
        definition(
            "WORKS Fence",
            false,
            "Works",
            vec![move_to(0.0, 0.0), draw(2.0, 0.0)],
        ),
        definition(
            "MARK Cross",
            true,
            "Marks",
            vec![move_to(-1.0, 0.0), draw(1.0, 0.0)],
        ),
        definition(
            "LOOSE Dot",
            true,
            "",
            vec![
                pen("Tint_Rose"),
                Stroke {
                    op: StrokeOp::Dot,
                    ..Stroke::default()
                },
            ],
        ),
    ] {
        let name = each.name.clone();
        assert!(session.library.add(each).is_ok(), "{name}");
    }

    let pegs = SurveyRule {
        key: String::from("PG*"),
        model: String::from("PEGS"),
        colour: String::from("tint teal"),
        ..SurveyRule::default()
    };
    let fences = SurveyRule {
        key: String::from("FN*"),
        model: String::from("FENCES"),
        linestyle: String::from("WORKS Fence"),
        ..SurveyRule::default()
    };
    let marks = SurveyRule {
        key: String::from("MK*"),
        section: SurveySection::VertexSymbol,
        symbol: Some(SurveySymbol {
            style: String::from("MARK Cross"),
            colour: String::from("TINT-LIME"),
            ..SurveySymbol::default()
        }),
        ..SurveyRule::default()
    };
    let texts = SurveyRule {
        key: String::from("TX*"),
        section: SurveySection::VertexTextStyle,
        text_style: Some(SurveyTextStyle {
            colour: String::from("tint plum"),
            ..SurveyTextStyle::default()
        }),
        ..SurveyRule::default()
    };
    for each in [pegs, fences, marks, texts] {
        let key = each.key.clone();
        assert!(session.map.add(each).is_ok(), "{key}");
    }
    session
}

// The names of a table's colours, in the order it lists them.
fn colour_names(of: &Customisation) -> Vec<String> {
    of.colours
        .entries()
        .into_iter()
        .map(|entry| entry.name)
        .collect()
}

fn codes_alone() -> CustomisationWriteOptions {
    CustomisationWriteOptions {
        linestyles: false,
        symbols: false,
        ..CustomisationWriteOptions::default()
    }
}

fn definitions_named(names: &[&str]) -> CustomisationWriteOptions {
    CustomisationWriteOptions {
        codes: false,
        only: lines(names),
        ..CustomisationWriteOptions::default()
    }
}

// What the writer makes of the part with the SAME options, read back: the
// part is cut to be handed to it so.
fn written_and_read(part: &Customisation, options: &CustomisationWriteOptions) -> Customisation {
    let text =
        customisation_to_json(part, options).unwrap_or_else(|error| panic!("{}", error.describe()));
    customisation_from_json(text.as_bytes()).unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod customisation_part {
    use super::*;

    #[test]
    fn a_part_says_nothing_of_the_settings_and_is_no_edition_of_the_built_in() {
        let session = works();
        assert!(session.linework.is_some());
        assert!(session.automation.is_some());
        assert!(session.based_on.is_some());

        for options in [
            codes_alone(),
            definitions_named(&["MARK Cross"]),
            CustomisationWriteOptions::default(),
        ] {
            let part = customisation_part(session.clone(), &options);
            // Merged into a colleague's session a part must not reset their
            // control codes or switches, and it is not a copy of the built-in
            // to be told from another edition - whichever part it is, the
            // whole asked for AS a part included.
            assert!(part.linework.is_none());
            assert!(part.automation.is_none());
            assert!(part.based_on.is_none());
            // Whose it is travels with every part.
            assert_eq!(part.name, "Works");
            assert_eq!(part.description, "The works set.");
            assert!(!part.notice.is_empty());
            assert_eq!(part.notice[0], OWN_NOTICE);
            // The definitions and rules themselves are the writer's to choose
            // among, by the same options.
            assert_eq!(part.library.size(), 4);
            assert_eq!(part.map.size(), 4);
        }
    }

    #[test]
    fn the_codes_alone_list_the_sources_that_brought_rules_and_carry_the_colours_the_rules_name() {
        let session = works();
        let options = codes_alone();
        let part = customisation_part(session.clone(), &options);

        // Works and Client brought rules, and are said to have brought those
        // alone: no definition is written. Marks brought definitions, none of
        // which is written, so it is no source of this part - and its notice
        // is written with the part's own. Tints, a table of colours, is a
        // source because the part carries a colour.
        assert_eq!(
            part.sources,
            vec![
                source("Works", false, true, &[]),
                source("Client", false, true, &[CLIENT_NOTICE]),
                source("Tints", false, false, &[TINTS_NOTICE]),
            ]
        );
        assert_eq!(part.notice, lines(&[OWN_NOTICE, MARKS_NOTICE]));
        // The three places a rule names a colour: PG*'s own, MK*'s symbol's
        // and TX*'s text's, as the TABLE spells them and in its order. Not
        // tint rose, which only a pen names - and no pen is written.
        assert_eq!(
            colour_names(&part),
            lines(&["tint lime", "tint plum", "tint teal"])
        );

        let read = written_and_read(&part, &options);
        assert!(read.library.empty());
        assert!(read.map == session.map);
        assert_eq!(read.sources, part.sources);
        assert_eq!(read.notice, part.notice);
        assert!(read.linework.is_none());
    }

    #[test]
    fn named_definitions_list_the_sources_they_came_from_and_no_other() {
        let session = works();

        // The cross alone, which came from Marks and has no pen. Works
        // brought definitions too, but none of ITS is written: listed, it
        // would pass for loaded wherever the file went (a session's sources,
        // and a project's record of them, go by name). No colour is carried,
        // so the table of colours is no source either. Every notice left out
        // is written after the part's own, in the order of the sources.
        let cross = definitions_named(&["MARK Cross"]);
        let one = customisation_part(session.clone(), &cross);
        assert_eq!(
            one.sources,
            vec![source("Marks", true, false, &[MARKS_NOTICE])]
        );
        assert_eq!(
            one.notice,
            lines(&[OWN_NOTICE, CLIENT_NOTICE, TINTS_NOTICE])
        );
        assert!(one.colours.empty());
        let read = written_and_read(&one, &cross);
        assert_eq!(read.library.names(), lines(&["MARK Cross"]));
        assert!(read.map.empty());

        // With the peg, whose pen is tint teal: Works is a source now, of
        // definitions alone - its rules are not written - and Tints because a
        // colour is carried. That one colour and no other.
        let two = customisation_part(session, &definitions_named(&["MARK Cross", "WORKS Peg"]));
        assert_eq!(
            two.sources,
            vec![
                source("Works", true, false, &[]),
                source("Marks", true, false, &[MARKS_NOTICE]),
                source("Tints", false, false, &[TINTS_NOTICE]),
            ]
        );
        assert_eq!(two.notice, lines(&[OWN_NOTICE, CLIENT_NOTICE]));
        assert_eq!(colour_names(&two), lines(&["tint teal"]));
    }

    #[test]
    fn a_kind_of_definition_is_written_with_the_colours_its_pens_name_by_the_fold() {
        let session = works();

        // The symbols: the peg (a pen of tint teal), the cross, and the dot
        // made in a session, whose pen is spelled Tint_Rose - one name with
        // the table's tint rose. The dot has no source, so it adds none.
        let symbols = CustomisationWriteOptions {
            linestyles: false,
            codes: false,
            ..CustomisationWriteOptions::default()
        };
        let marks = customisation_part(session.clone(), &symbols);
        assert_eq!(
            marks.sources,
            vec![
                source("Works", true, false, &[]),
                source("Marks", true, false, &[MARKS_NOTICE]),
                source("Tints", false, false, &[TINTS_NOTICE]),
            ]
        );
        assert_eq!(marks.notice, lines(&[OWN_NOTICE, CLIENT_NOTICE]));
        assert_eq!(colour_names(&marks), lines(&["tint rose", "tint teal"]));

        // The linestyles: the fence, which has no pen. Nothing of Client,
        // Marks or Tints is written, and each notice is.
        let linestyles = CustomisationWriteOptions {
            symbols: false,
            codes: false,
            ..CustomisationWriteOptions::default()
        };
        let line_part = customisation_part(session, &linestyles);
        assert_eq!(line_part.sources, vec![source("Works", true, false, &[])]);
        assert_eq!(
            line_part.notice,
            lines(&[OWN_NOTICE, CLIENT_NOTICE, MARKS_NOTICE, TINTS_NOTICE])
        );
        assert!(line_part.colours.empty());
    }

    #[test]
    fn a_notice_is_written_once_however_many_sources_left_out_carry_it() {
        // Client's notice begins with the session's own line, and Marks
        // carries Client's: three sources left out of a part of linestyles,
        // five lines among them and the part, three of them distinct besides
        // the part's.
        let mut session = works();
        session.sources[1].notice = lines(&[OWN_NOTICE, CLIENT_NOTICE]);
        session.sources[2].notice = lines(&[CLIENT_NOTICE, MARKS_NOTICE]);
        let linestyles = CustomisationWriteOptions {
            symbols: false,
            codes: false,
            ..CustomisationWriteOptions::default()
        };

        let part = customisation_part(session, &linestyles);
        assert_eq!(
            part.notice,
            lines(&[OWN_NOTICE, CLIENT_NOTICE, MARKS_NOTICE, TINTS_NOTICE])
        );
    }

    #[test]
    fn every_kind_asked_for_as_a_part_holds_everything_but_the_settings() {
        // CUSTOMISE EXPORT <file> CODES LINESTYLES SYMBOLS: every definition
        // and rule, and so every source as it stands and every colour
        // something names - all four here - with no notice to carry over.
        let session = works();
        let part = customisation_part(session.clone(), &CustomisationWriteOptions::default());
        assert_eq!(part.sources, session.sources);
        assert_eq!(part.notice, lines(&[OWN_NOTICE]));
        assert_eq!(
            colour_names(&part),
            lines(&["tint lime", "tint plum", "tint rose", "tint teal"])
        );
        assert_eq!(part.library.names(), session.library.names());
        assert!(part.map == session.map);
    }

    #[test]
    fn what_the_writer_would_refuse_is_not_counted_as_written() {
        let session = works();

        // A linestyle asked for by name where only symbols are written: the
        // writer refuses the write, and it is not counted here either - so
        // the part lists no source for a definition it does not hold.
        let wrong_kind = CustomisationWriteOptions {
            linestyles: false,
            codes: false,
            only: lines(&["WORKS Fence"]),
            ..CustomisationWriteOptions::default()
        };
        let none = customisation_part(session.clone(), &wrong_kind);
        assert!(none.sources.is_empty());
        assert!(none.colours.empty());
        assert_eq!(
            none.notice,
            lines(&[OWN_NOTICE, CLIENT_NOTICE, MARKS_NOTICE, TINTS_NOTICE])
        );
        let refused = customisation_to_json(&none, &wrong_kind);
        assert_eq!(
            refused
                .expect_err("a linestyle where only symbols are written")
                .code,
            ErrorCode::InvalidArgument
        );

        // A name the library does not hold is no definition at all.
        let missing = customisation_part(session, &definitions_named(&["NOT There"]));
        assert!(missing.sources.is_empty());
        let not_found = customisation_to_json(&missing, &definitions_named(&["NOT There"]));
        assert_eq!(
            not_found
                .expect_err("a name the library does not hold")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn with_no_rules_to_write_no_source_is_said_to_have_brought_any() {
        // The codes asked for of a session whose rules are all gone - the
        // Survey Code Manager's buffer, emptied: nothing is written, so
        // nothing came from anywhere, no colour is named, and every notice is
        // carried.
        let mut session = works();
        session.map = Default::default();
        let part = customisation_part(session, &codes_alone());
        assert!(part.sources.is_empty());
        assert!(part.colours.empty());
        assert_eq!(
            part.notice,
            lines(&[OWN_NOTICE, CLIENT_NOTICE, MARKS_NOTICE, TINTS_NOTICE])
        );
    }

    #[test]
    fn a_customisation_with_nothing_in_it_is_a_part_with_nothing_in_it() {
        // Nothing to cut, nothing to carry: a Document nothing was loaded
        // into.
        assert!(
            customisation_part(Customisation::default(), &codes_alone())
                == Customisation::default()
        );
        assert!(
            customisation_part(Customisation::default(), &definitions_named(&["Anything"]))
                == Customisation::default()
        );

        // Rules set by hand, with no source to say where they came from: the
        // rules, the colour they name, and no source.
        let mut loose = Customisation {
            name: String::from("Loose"),
            ..Customisation::default()
        };
        assert!(
            loose
                .colours
                .add("tint teal", Color::new(0, 128, 128, 255))
                .is_ok()
        );
        assert!(
            loose
                .colours
                .add("tint rose", Color::new(255, 0, 127, 255))
                .is_ok()
        );
        let rule = SurveyRule {
            key: String::from("KT*"),
            colour: String::from("tint rose"),
            ..SurveyRule::default()
        };
        assert!(loose.map.add(rule).is_ok());
        let part = customisation_part(loose, &codes_alone());
        assert!(part.sources.is_empty());
        assert!(part.notice.is_empty());
        assert_eq!(colour_names(&part), lines(&["tint rose"]));
        assert_eq!(part.map.size(), 1);
    }
}
