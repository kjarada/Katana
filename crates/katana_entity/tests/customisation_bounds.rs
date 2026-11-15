//! The twin of tests/entity/customisation/test_customisation_bounds.cpp: the
//! bounds a Katana customisation file is held to (CUSTOMISATION_MOST_*): the
//! reader refuses a file past them, naming the entry and the member as it
//! names every other refusal, and the writer refuses what would read back as
//! one of those.
//!
//! The expectations are written by hand from the bounds - a number's size at
//! most 1000000000, an arc's sweep at most 720 degrees, 100000 strokes to a
//! definition, 1000 bytes to a name or a text, 1000000 rules to a file - not
//! read off a run. Each bound is tried exactly AT it (accepted) and one past it
//! (refused).

pub mod customisation_support;

use katana_core::error::{Error, ErrorCode};
use katana_entity::customisation::{customisation_to_json, definition_from_json};
use katana_entity::{Customisation, CustomisationWriteOptions, LineStyle, Stroke, StrokeOp};
use katana_geometry::Point2;

use customisation_support::{customisation_with, read_customisation, refusal_of};

/// What every refusal of a number past the bound says after its member.
const LARGER: &str = "larger than 1000000000 in size, the most any number of a customisation holds";
const LONG: &str = "longer than 1000 bytes, the most a name or text of a customisation holds";
const SWEEPS: &str = "sweeps more than 720 degrees (two turns), the most an arc holds";

/// A customisation whose one symbol, S, has these strokes (written as the file
/// writes them, comma separated).
fn symbol_with(strokes: &str) -> String {
    format!(r#""symbols": [{{"name": "S", "atVertices": true, "strokes": [{strokes}]}}]"#)
}

/// A customisation whose one symbol, S, has these members besides its name.
fn symbol_members(members: &str) -> String {
    format!(r#""symbols": [{{"name": "S", {members}}}]"#)
}

fn repeated(piece: &str, times: usize) -> String {
    vec![piece; times].join(",")
}

/// A customisation holding one definition, which the writer is then asked for.
fn holding(mut definition: LineStyle) -> Customisation {
    let mut customisation = Customisation {
        name: String::from("W"),
        ..Customisation::default()
    };
    definition.source = String::from("W");
    if let Err(error) = customisation.library.add(definition) {
        panic!("{}", error.describe());
    }
    customisation
}

fn write_refusal(customisation: &Customisation) -> Error {
    match customisation_to_json(customisation, &CustomisationWriteOptions::default()) {
        Ok(_) => panic!("written without complaint"),
        Err(error) => error,
    }
}

fn named(name: &str) -> LineStyle {
    LineStyle {
        name: String::from(name),
        ..LineStyle::default()
    }
}

mod customisation_bounds {
    use super::*;

    // ---- an arc's sweep -----------------------------------------------------------------

    #[test]
    fn an_arc_may_sweep_two_turns_either_way_round_and_no_more() {
        // 720 exactly, forwards, backwards and about zero: two full turns each.
        for arc in [
            r#"["arc", 1, 0, 720]"#,
            r#"["arc", 1, 720, 0]"#,
            r#"["arc", 1, -360, 360]"#,
        ] {
            let read = read_customisation(customisation_with(&symbol_with(arc)));
            assert_eq!(read.library.size(), 1, "{arc}");
        }
        // A hair past it, in each direction, is refused naming the stroke.
        for arc in [
            r#"["arc", 1, 0, 720.5]"#,
            r#"["arc", 1, 720.5, 0]"#,
            r#"["arc", 1, -1e9, 1e9]"#,
        ] {
            let error = refusal_of(customisation_with(&symbol_with(arc)));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{arc}");
            assert_eq!(
                error.message,
                format!(r#"symbols[0] "S" strokes[0]: "arc" {SWEEPS}"#),
                "{arc}"
            );
            assert!(error.context.contains("arc"), "{}", error.context);
        }
    }

    // ---- a number's size ----------------------------------------------------------------

    #[test]
    fn a_strokes_number_may_be_a_billion_in_size_and_no_more() {
        for stroke in [
            r#"["move", 1000000000, -1000000000]"#,
            r#"["circle", 1000000000]"#,
            r#"["dot", -1000000000]"#,
        ] {
            assert_eq!(
                read_customisation(customisation_with(&symbol_with(stroke)))
                    .library
                    .size(),
                1,
                "{stroke}"
            );
        }
        for (stroke, word) in [
            (r#"["move", 1000000001, 0]"#, "move"),
            (r#"["draw", 0, -1.2e29]"#, "draw"),
            (r#"["circle", 1e10]"#, "circle"),
            (r#"["dot", -2e9]"#, "dot"),
            // The review's own: a sweep of 1e308 degrees is a number past the
            // bound before it is a sweep past it.
            (r#"["arc", 1, 0, 1e308]"#, "arc"),
            (r#"["arc", 1e12, 0, 90]"#, "arc"),
        ] {
            let error = refusal_of(customisation_with(&symbol_with(stroke)));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{stroke}");
            assert_eq!(
                error.message,
                format!(r#"symbols[0] "S" strokes[0]: "{word}" has a number {LARGER}"#),
                "{stroke}"
            );
        }
    }

    #[test]
    fn a_definitions_own_numbers_are_held_to_the_same_bound() {
        assert_eq!(
            read_customisation(customisation_with(&symbol_members(
                r#""length": 1000000000, "factor": 1000000000,
                                                    "origin": [1000000000, -1000000000],
                                                    "anchors": [[0, 0], [1000000000, 0]],
                                                    "strokes": []"#
            )))
            .library
            .size(),
            1
        );
        for (members, member) in [
            (r#""length": 1e10"#, "length"),
            (r#""factor": -1e10"#, "factor"),
            (r#""origin": [0, 1e10]"#, "origin"),
            (r#""anchors": [[0, 0], [-1e10, 0]]"#, "anchors"),
        ] {
            let error = refusal_of(customisation_with(&symbol_members(members)));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{members}");
            assert_eq!(
                error.message,
                format!(r#"symbols[0] "S": "{member}" is {LARGER}"#),
                "{members}"
            );
        }
    }

    #[test]
    fn a_text_strokes_numbers_and_a_rules_are_held_to_it_too() {
        let height = refusal_of(customisation_with(&symbol_with(
            r#"["text", {"text": "N", "height": 1e10}]"#,
        )));
        assert_eq!(height.code, ErrorCode::ParseFailure);
        assert_eq!(
            height.message,
            format!(r#"symbols[0] "S" strokes[0]: "height" is {LARGER}"#)
        );

        let extra = refusal_of(customisation_with(&symbol_with(
            r#"["text", {"text": "N", "extra": [0, 1e10, 0]}]"#,
        )));
        assert_eq!(extra.code, ErrorCode::ParseFailure);
        assert_eq!(
            extra.message,
            format!(r#"symbols[0] "S" strokes[0]: "extra" has a number {LARGER}"#)
        );

        // A rule's own: the size of its symbol.
        let size = refusal_of(customisation_with(
            r#""codes": [{"key": "A*", "sets": "symbol", "symbol": {"name": "X", "size": 1e10}}]"#,
        ));
        assert_eq!(size.code, ErrorCode::ParseFailure);
        assert!(
            size.message.starts_with(r#"codes[0] "A*""#),
            "{}",
            size.message
        );
        assert!(
            size.message.ends_with(&format!(r#""size" is {LARGER}"#)),
            "{}",
            size.message
        );
        // And at the bound it reads.
        assert_eq!(
            read_customisation(customisation_with(
                r#""codes": [{"key": "A*", "sets": "symbol",
                                                  "symbol": {"name": "X", "size": 1000000000}}]"#
            ))
            .map
            .size(),
            1
        );
    }

    // ---- strokes in a definition --------------------------------------------------------

    #[test]
    fn a_definition_may_hold_a_hundred_thousand_strokes_and_not_one_more() {
        let stroke = r#"["move", 0, 0]"#;
        let at_the_bound =
            read_customisation(customisation_with(&symbol_with(&repeated(stroke, 100_000))));
        assert_eq!(
            at_the_bound.library.find("S").expect("S").strokes.len(),
            100_000
        );

        let error = refusal_of(customisation_with(&symbol_with(&repeated(stroke, 100_001))));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        // The stroke that is one too many is the 100001st: counted from 0,
        // 100000.
        assert_eq!(
            error.message,
            r#"symbols[0] "S" strokes[100000]: a definition holds at most 100000 strokes, and this is one more"#
        );
    }

    #[test]
    fn a_definition_read_on_its_own_is_held_to_the_stroke_bound_too() {
        let stroke = r#"["move", 0, 0]"#;
        let text = format!(
            r#"{{"name": "D", "strokes": [{}]}}"#,
            repeated(stroke, 100_001)
        );
        let read =
            definition_from_json(text.as_bytes(), true, "").expect_err("one stroke too many");
        assert_eq!(
            read.message,
            r#"definition "D" strokes[100000]: a definition holds at most 100000 strokes, and this is one more"#
        );
        // And a number past the bound, as a file says it.
        let beyond =
            definition_from_json(br#"{"name": "D", "length": 5e9, "strokes": []}"#, false, "")
                .expect_err("a length past the bound");
        assert_eq!(
            beyond.message,
            format!(r#"definition "D": "length" is {LARGER}"#)
        );
    }

    // ---- names and texts ----------------------------------------------------------------

    #[test]
    fn a_name_or_a_text_may_be_a_thousand_bytes_and_no_more() {
        let fits = "A".repeat(1000);
        let past = "B".repeat(1001);
        // At the bound, each place a text can sit: a definition's name, a
        // rule's key and comment, a pen, a text stroke's words, a colour's name
        // and a notice's line.
        {
            let members = format!(
                r##"{}, "codes": [{{"key": "{fits}", "sets": "feature", "comment": "{fits}"}}], "colours": {{"{fits}": "#102030"}}, "notice": ["{fits}"]"##,
                symbol_with(&format!(r#"["pen", "{fits}"]"#))
            );
            assert_eq!(
                read_customisation(customisation_with(&members)).map.size(),
                1
            );
        }
        // One past it, each refused naming its entry (cut, as a name that long
        // must be to be of any use in a message) and its member.
        //
        // A name is shown cut at 240 bytes, with "..." inside the quotes.
        let b_shown = format!("{}...", "B".repeat(240));
        let cases = [
            (
                format!(r#""symbols": [{{"name": "{past}", "strokes": []}}]"#),
                format!(r#"symbols[0] "{b_shown}""#),
                format!(r#""name" is {LONG}"#),
            ),
            (
                format!(r#""codes": [{{"key": "{past}", "sets": "feature"}}]"#),
                format!(r#"codes[0] "{b_shown}""#),
                format!(r#""key" is {LONG}"#),
            ),
            (
                format!(r#""codes": [{{"key": "K", "sets": "feature", "comment": "{past}"}}]"#),
                String::from(r#"codes[0] "K""#),
                format!(r#""comment" is {LONG}"#),
            ),
            (
                symbol_with(&format!(r#"["pen", "{past}"]"#)),
                String::from(r#"symbols[0] "S" strokes[0]"#),
                format!(r#""pen" is {LONG}"#),
            ),
            (
                symbol_with(&format!(r#"["text", {{"text": "{past}"}}]"#)),
                String::from(r#"symbols[0] "S" strokes[0]"#),
                format!(r#""text" is {LONG}"#),
            ),
            (
                format!(r##""colours": {{"{past}": "#102030"}}"##),
                String::from("colours"),
                format!("a colour's name is {LONG}"),
            ),
            (
                format!(r#""notice": ["{past}"]"#),
                String::from("top level"),
                format!(r#""notice" holds a line that is {LONG}"#),
            ),
        ];
        for (members, entry_start, ends_with_text) in &cases {
            let error = refusal_of(customisation_with(members));
            let start: String = error.message.chars().take(300).collect();
            assert_eq!(error.code, ErrorCode::ParseFailure, "{entry_start}");
            assert!(
                error.message.starts_with(&format!("{entry_start}: ")),
                "{start}"
            );
            assert!(error.message.ends_with(ends_with_text.as_str()), "{start}");
            // Never the whole of the text: a refusal about a megabyte is not
            // one.
            assert!(error.message.len() < 600, "{entry_start}");
        }
    }

    #[test]
    fn a_files_own_name_is_held_to_it_too() {
        let name = "N".repeat(1001);
        let text =
            format!(r#"{{"format": "katana-customisation", "version": 1, "name": "{name}"}}"#);
        let error = refusal_of(text);
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert_eq!(error.message, format!(r#"top level: "name" is {LONG}"#));
    }

    // ---- rules in a file ----------------------------------------------------------------

    #[test]
    fn a_file_may_hold_a_million_rules_and_not_one_more() {
        // `{}` is no rule (it has no key), so each file is refused - the first
        // at its first entry, which is how it is known that the bound did not
        // fire at exactly a million, and the second by the bound, before any is
        // read.
        let rule = "{}";
        let at_the_bound = refusal_of(customisation_with(&format!(
            r#""codes": [{}]"#,
            repeated(rule, 1_000_000)
        )));
        assert_eq!(
            at_the_bound.message,
            r#"codes[0]: has no "key", which it must"#
        );

        let past = refusal_of(customisation_with(&format!(
            r#""codes": [{}]"#,
            repeated(rule, 1_000_001)
        )));
        assert_eq!(past.code, ErrorCode::ParseFailure);
        assert_eq!(
            past.message,
            r#"top level: "codes" holds more than 1000000 rules, the most a customisation holds"#
        );
    }

    // ---- the writer is held to the same, so that what it writes reads back --------------

    #[test]
    fn the_writer_refuses_a_number_past_the_bound() {
        let mut style = named("N");
        style.strokes = vec![Stroke {
            op: StrokeOp::Draw,
            point: Point2::new(2.0e9, 0.0),
            ..Stroke::default()
        }];
        let error = write_refusal(&holding(style.clone()));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            error.message,
            format!(r#"linestyles[0] "N" strokes[0]: "draw" is {LARGER}"#)
        );

        // And at the bound, a definition is written and reads back as itself.
        style.strokes = vec![Stroke {
            op: StrokeOp::Draw,
            point: Point2::new(1.0e9, -1.0e9),
            ..Stroke::default()
        }];
        let held = holding(style);
        let written =
            customisation_to_json(&held, &CustomisationWriteOptions::default()).expect("written");
        assert!(read_customisation(written) == held);
    }

    #[test]
    fn the_writer_refuses_an_arc_sweeping_more_than_two_turns() {
        let mut style = named("N");
        style.strokes = vec![Stroke {
            op: StrokeOp::Arc,
            radius: 1.0,
            start_angle: 0.0,
            end_angle: 721.0,
            ..Stroke::default()
        }];
        let error = write_refusal(&holding(style.clone()));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            error.message,
            format!(r#"linestyles[0] "N" strokes[0]: this arc {SWEEPS}"#)
        );

        style.strokes = vec![Stroke {
            op: StrokeOp::Arc,
            radius: 1.0,
            start_angle: 0.0,
            end_angle: 720.0,
            ..Stroke::default()
        }];
        let held = holding(style);
        let written =
            customisation_to_json(&held, &CustomisationWriteOptions::default()).expect("written");
        assert!(read_customisation(written) == held);
    }

    #[test]
    fn the_writer_refuses_too_many_strokes_and_a_text_over_a_thousand_bytes() {
        let move_stroke = Stroke {
            op: StrokeOp::Move,
            ..Stroke::default()
        };
        let mut style = named("N");
        style.strokes = vec![move_stroke.clone(); 100_001];
        let strokes = write_refusal(&holding(style.clone()));
        assert_eq!(strokes.code, ErrorCode::InvalidArgument);
        assert_eq!(
            strokes.message,
            r#"linestyles[0] "N": the definition holds 100001 strokes, and a definition holds at most 100000"#
        );
        style.strokes = vec![move_stroke; 100_000];
        let held = holding(style);
        assert!(customisation_to_json(&held, &CustomisationWriteOptions::default()).is_ok());

        let name = write_refusal(&holding(named(&"L".repeat(1001))));
        let start: String = name.message.chars().take(300).collect();
        assert_eq!(name.code, ErrorCode::InvalidArgument);
        assert!(
            name.message
                .starts_with(&format!(r#"linestyles[0] "{}"#, "L".repeat(240))),
            "{start}"
        );
        assert!(
            name.message.ends_with(&format!(r#""name" is {LONG}"#)),
            "{start}"
        );
        assert!(name.message.len() < 600);
    }
}
