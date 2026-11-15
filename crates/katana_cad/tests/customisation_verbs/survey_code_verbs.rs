//! CODE (survey_code_verbs) through CommandInterpreter::run, as the window's command line,
//! katana_cli and katana_mcp type it (tests/cad/customisation/test_survey_code_verbs.cpp). It was
//! katana_cli's own until 2026-09-26, and the window answered "unknown command"; in the
//! interpreter every front end has it.
//!
//! The map is the cli.* tests' own, built rule by rule, by index:
//!
//! ```text
//!   0  KT*  feature: layer KATANA TEST, colour red, linestyle "0"
//!   1  KX*  feature with no layer - a warning in the lint
//!   2  KT*  symbol: the built-in "cross", size 2
//! ```
//!
//! The texts are code_table's formatters', which the code table's own cases pin; what is pinned
//! here is that the verbs reach them, what they refuse, what a scope takes, and that CODE is one
//! undo step.
//!
//! A rule's section and fields are named here by the Katana customisation format's words
//! ("feature", "layer", "surface", "at vertices"), which the survey code tools show since the
//! survey code file's own (`map_data`, `model`, `tinable`, `mode vertex`) left the window. Each
//! expectation holding such a word was changed old to new from the table "The words a rule is
//! shown by" in docs/customisation.md, not from what a run printed.
//!
//! CODE LIST and CODE CHECK were a verb of their own, named after that file, and a reply of CODE
//! and CODE CENSUS now begins with what its scope took. Each expectation those two changed was
//! changed from docs/customisation.md, "The verbs" - the same replies under the new words, and
//! one line more, whose count is the number of entities the test itself drew.

use katana_cad::survey_code_verbs::code_check_reply;
use katana_cad::{
    CommandInterpreter, Document, LintIssue, LintKind, LintSeverity, ScopeView, ScopeViewFn,
};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{
    Color, ColourTable, EntityId, SurveyMap, SurveyRule, SurveySection, SurveySymbol,
};
use katana_geometry::{Box2, Point2};

fn test_codes() -> SurveyMap {
    let mut map = SurveyMap::default();
    let katana = SurveyRule {
        key: String::from("KT*"),
        model: String::from("KATANA TEST"),
        colour: String::from("red"),
        linestyle: String::from("0"),
        comment: String::from("[KT*] Katana test"),
        ..SurveyRule::default()
    };
    assert!(map.add(katana).is_ok());
    let nowhere = SurveyRule {
        key: String::from("KX*"),
        colour: String::from("red"),
        ..SurveyRule::default()
    };
    assert!(map.add(nowhere).is_ok());
    let cross = SurveyRule {
        key: String::from("KT*"),
        section: SurveySection::VertexSymbol,
        symbol: Some(SurveySymbol {
            style: String::from("cross"),
            colour: String::new(),
            size: 2.0,
            rotation: 0.0,
            offset: 0.0,
            raise: 0.0,
        }),
        ..SurveyRule::default()
    };
    assert!(map.add(cross).is_ok());
    map
}

// A document and an interpreter running lines on it; the three ids are the
// scope cases' (scoped), and 0 elsewhere.
struct Session {
    document: Document,
    interpreter: CommandInterpreter,
    on_zero: EntityId,
    point_on_a: EntityId,
    line_on_a: EntityId,
}

impl Session {
    fn new(loaded: bool) -> Self {
        let mut document = Document::default();
        if loaded {
            document.set_survey_map(test_codes());
        }
        Self {
            document,
            interpreter: CommandInterpreter::new(),
            on_zero: 0,
            point_on_a: 0,
            line_on_a: 0,
        }
    }

    #[track_caller]
    fn ok(&mut self, line: &str) -> String {
        self.interpreter
            .run(&mut self.document, line)
            .unwrap_or_else(|error| panic!("{line}\n  -> {}", error.describe()))
    }

    #[track_caller]
    fn refused(&mut self, line: &str) -> Error {
        match self.interpreter.run(&mut self.document, line) {
            Ok(reply) => panic!("{line} was accepted:\n{reply}"),
            Err(error) => error,
        }
    }

    // One coded point, as a survey import leaves it.
    #[track_caller]
    fn coded_point(&mut self) {
        self.ok("POINT 1,1");
        self.ok("SELECT ALL");
        self.ok("PROP SET code KT01");
    }

    // What `draw` makes, carrying `code` under "code"; its id.
    #[track_caller]
    fn coded(&mut self, draw: &str, code: &str) -> EntityId {
        self.ok(draw);
        let made = self.document.last_created_entities();
        assert_eq!(made.len(), 1, "{draw}");
        let id = made.first().copied().unwrap_or(0);
        let select = format!("SELECT {id}");
        self.ok(&select);
        let set = format!("PROP SET code {code}");
        self.ok(&set);
        self.ok("SELECT NONE");
        id
    }

    fn layer_of(&self, id: EntityId) -> String {
        self.document.model().entities.find(id).map_or_else(
            || String::from("(no such entity)"),
            |entity| entity.layer.clone(),
        )
    }
}

// Three coded entities for the scope tests, by hand:
//   point (1,1)          on layer 0, code KT01
//   point (2,2)          on layer a, code KT02
//   line (10,10)-(15,15) on layer a, code KX01
// so the drawing holds 3, layer a holds 2, and 2 are points. KT* gives a
// layer (KATANA TEST); KX* is a rule and gives none.
#[track_caller]
fn scoped() -> Session {
    let mut session = Session::new(true);
    session.on_zero = session.coded("POINT 1,1", "KT01");
    session.ok("LAYER NEW a");
    session.ok("LAYER SET a");
    session.point_on_a = session.coded("POINT 2,2", "KT02");
    session.line_on_a = session.coded("LINE 10,10 15,15", "KX01");
    session.ok("LAYER SET 0");
    session
}

const CENSUS_OF_LAYER_A: &str = concat!(
    "2 entities carry a code in \"code\", 2 distinct codes\n",
    "  KT02: 1, prefix, matched, layer KATANA TEST\n",
    "  KX01: 1, prefix, matched"
);

mod survey_code_verbs {
    use super::*;

    #[test]
    fn are_refused_by_name_until_survey_codes_are_loaded() {
        let mut session = Session::new(false);
        session.coded_point();
        let steps = session.document.history().undo_count();
        for line in [
            "CODE",
            "code census",
            "CODE EXPLAIN KT01",
            "CODE LIST",
            "CODE CHECK",
            "CODE DRAWING PREVIEW",
        ] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidState, "{line}");
            // The remedy names the file that loads: "use CUSTOMISE <file> first"
            // once meant a survey code file, which is a kind of file that is gone.
            assert_eq!(
                error.message,
                "no survey codes are loaded; CUSTOMISE <file> loads a Katana customisation file",
                "{line}"
            );
        }
        assert_eq!(session.document.history().undo_count(), steps);
        assert_eq!(session.layer_of(1), "0");
    }

    #[test]
    fn code_applies_the_loaded_codes_as_one_undo_step() {
        let mut session = Session::new(true);
        session.coded_point();
        let steps = session.document.history().undo_count();
        let reply = session.ok("CODE");
        // What the scope took first: no scope word is the drawing, which holds
        // the one point.
        assert!(
            reply.starts_with(concat!(
                "scope=drawing matched=1\n",
                "1 entity carries a code in \"code\": 1 matched, 0 fallback-only"
            )),
            "{reply}"
        );
        assert!(
            reply.contains("Layers created: KATANA TEST\nStyles created: cross\n"),
            "{reply}"
        );
        assert!(
            reply.ends_with("Applied as one command. UNDO puts it all back."),
            "{reply}"
        );
        assert_eq!(session.layer_of(1), "KATANA TEST");
        assert_eq!(session.document.history().undo_count(), steps + 1);
        // No front end passed a colour: red is a standard name, #FF0000, and the
        // Document knows the standard names itself.
        let style = session
            .document
            .model()
            .styles
            .find("cross")
            .expect("the style cross");
        let colour = style.color.expect("the style has a colour");
        assert_eq!(colour.to_hex(), "#FF0000");

        // Again: everything already has what its rule gives.
        assert!(session.ok("CODE").ends_with("Nothing to change."));
        assert_eq!(session.document.history().undo_count(), steps + 1);

        session.ok("UNDO");
        assert_eq!(session.layer_of(1), "0");
        assert!(!session.document.model().layers.contains("KATANA TEST"));
        assert!(!session.document.model().styles.contains("cross"));
    }

    #[test]
    fn a_colour_name_nothing_knows_is_left_alone_rather_than_guessed_at() {
        // Two names: "red" is standard; "pen 025" is a plot pen, which nothing
        // resolves (docs/customisation.md, "Colour names"). The verb asks the
        // Document and nobody else - it took a lookup of a caller's own, asked
        // last, until 2026-10-07, and this test had a half that passed one.
        let mut map = SurveyMap::default();
        let mut feature = |key: &str, layer: &str, colour: &str| {
            let rule = SurveyRule {
                key: String::from(key),
                model: String::from(layer),
                colour: String::from(colour),
                linestyle: String::from("0"),
                ..SurveyRule::default()
            };
            assert!(map.add(rule).is_ok(), "{key}");
        };
        feature("RD*", "REDS", "red");
        feature("PN*", "PENS", "pen 025");

        let mut session = Session::new(false);
        session.document.set_survey_map(map);
        // The standard name resolves; the other is left alone.
        assert!(
            session
                .ok("CODE EXPLAIN RD1")
                .contains("colour \"red\": #FF0000")
        );
        assert!(
            session
                .ok("CODE EXPLAIN PN1")
                .contains("colour \"pen 025\": unknown name, so the entity keeps its own")
        );

        // And a coded entity whose colour nothing knows gets a style with none.
        session.coded("POINT 1,1", "PN1");
        session.ok("CODE");
        let mut found = false;
        session.document.model().styles.for_each(|style| {
            if style.description == "pen 025" {
                found = true;
                assert!(style.color.is_none(), "{}", style.name);
            }
        });
        assert!(found, "the style made for the code keeps the colour's name");
    }

    #[test]
    fn a_colour_the_customisation_names_itself_is_resolved_from_its_own_table() {
        // "site orange" is no standard name. Until the session's own table says
        // what it is, it is a name nothing knows; once the table says #112233 the
        // verb says so too - it asks the Document at each line, so it follows a
        // table installed since.
        let mut colours = ColourTable::default();
        assert!(
            colours
                .add(
                    "site orange",
                    Color::from_hex("#112233").unwrap_or_default()
                )
                .is_ok()
        );
        let mut map = SurveyMap::default();
        let rule = SurveyRule {
            key: String::from("SO*"),
            model: String::from("ORANGES"),
            colour: String::from("site orange"),
            ..SurveyRule::default()
        };
        assert!(map.add(rule).is_ok());

        let mut session = Session::new(false);
        session.document.set_survey_map(map);
        assert!(
            session
                .ok("CODE EXPLAIN SO1")
                .contains("colour \"site orange\": unknown name, so the entity keeps its own")
        );
        session.document.set_colour_table(colours);
        assert!(
            session
                .ok("CODE EXPLAIN SO1")
                .contains("colour \"site orange\": #112233")
        );
    }

    #[test]
    fn explain_census_and_list_answer_with_the_managers_words() {
        let mut session = Session::new(true);
        session.coded_point();
        // Rule #0 gives the layer and #2 the symbol; red is a standard colour.
        let explained = session.ok("code explain KT01");
        assert!(
            explained.starts_with("Code \"KT01\": prefix match, matched\n"),
            "{explained}"
        );
        assert!(explained.contains("  layer: KATANA TEST  <- rule #0 KT* (feature)\n"));
        assert!(explained.contains("  symbol: cross, size 2  <- rule #2 KT* (symbol)"));
        assert!(
            explained.ends_with("  colour \"red\": #FF0000"),
            "{explained}"
        );
        // A code of several words is the rest of the line.
        assert!(
            session
                .ok("CODE EXPLAIN KT 01")
                .starts_with("Code \"KT 01\"")
        );

        assert_eq!(
            session.ok("CODE CENSUS"),
            concat!(
                "scope=drawing matched=1\n",
                "1 entity carries a code in \"code\", 1 distinct code\n",
                "  KT01: 1, prefix, matched, layer KATANA TEST"
            )
        );
        // A word that is no scope word is the property, as it always was.
        assert_eq!(
            session.ok("CODE CENSUS elsewhere"),
            concat!(
                "scope=drawing matched=1\n",
                "0 entities carry a code in \"elsewhere\", 0 distinct codes"
            )
        );

        // One line a code. KT* is the only key of the two whose combined rule
        // holds "katana" (its layer); its two rules are a feature and a symbol
        // rule, and what they give is listed layer, linestyle, symbol, colour,
        // comment.
        let listed = session.ok("CODE LIST katana");
        assert_eq!(
            listed,
            concat!(
                "KT*  [prefix; 2 rules: feature, symbol]  layer KATANA TEST; linestyle 0; ",
                "symbol cross; colour red; \"[KT*] Katana test\"\n",
                "1 of 2 codes match \"katana\""
            )
        );
        let all = session.ok("code list");
        assert!(all.ends_with("2 of 2 codes"), "{all}");
        // Every reply is a line a front end ends itself: no trailing newline.
        for reply in [&explained, &listed] {
            assert!(!reply.ends_with('\n'));
        }
    }

    #[test]
    fn code_check_with_only_warnings_is_a_reply() {
        let mut session = Session::new(true);
        assert_eq!(
            session.ok("CODE CHECK"),
            concat!(
                "3 rules checked: 0 errors, 1 warning\n",
                "  by kind: 1 no layer\n",
                "  rule #1 KX* (feature): warning, no layer: a feature rule with no layer: its ",
                "codes stay on whatever layer they are on"
            )
        );
    }

    #[test]
    fn code_check_shows_a_field_its_section_does_not_use() {
        // A symbol rule that also names a layer: a lookup applies it, so it is no
        // error, but a load that replaces this key's symbol rules takes the layer
        // with them. One rule, one warning, naming the field by the word CODE
        // EXPLAIN names it by.
        let mut map = SurveyMap::default();
        let rule = SurveyRule {
            key: String::from("BL*"),
            section: SurveySection::VertexSymbol,
            symbol: Some(SurveySymbol {
                style: String::from("cross"),
                colour: String::new(),
                size: 1.0,
                rotation: 0.0,
                offset: 0.0,
                raise: 0.0,
            }),
            model: String::from("SURVEY DETAIL"),
            ..SurveyRule::default()
        };
        assert!(map.add(rule).is_ok());
        let mut session = Session::new(false);
        session.document.set_survey_map(map);
        assert_eq!(
            session.ok("CODE CHECK"),
            concat!(
                "1 rule checked: 0 errors, 1 warning\n",
                "  by kind: 1 field outside section\n",
                "  rule #0 BL* (symbol): warning, field outside section: holds layer, which a ",
                "symbol rule does not use: a load that replaces this key's symbol rules replaces ",
                "it too"
            )
        );
    }

    #[test]
    fn code_check_refuses_a_lint_with_an_error_and_carries_the_whole_lint() {
        // SurveyMap::add refuses the rules that lint as errors, so the refusal is
        // shown on issues made by hand - what lint_survey_rule reports for a key
        // with a blank before it.
        let blank = LintIssue {
            rule: 0,
            key: String::from(" WM*"),
            section: SurveySection::Map,
            severity: LintSeverity::Error,
            kind: LintKind::KeyWhitespace,
            message: String::from("key \" WM*\" has blanks around it"),
        };
        let colour = LintIssue {
            rule: 1,
            key: String::from("AC*"),
            section: SurveySection::Map,
            severity: LintSeverity::Warning,
            kind: LintKind::UnknownColour,
            message: String::from("colour \"sui\" is not a colour name"),
        };
        let refused =
            code_check_reply(&[blank, colour.clone()], 2).expect_err("a lint with an error");
        assert_eq!(refused.code, ErrorCode::InvalidState);
        assert_eq!(
            refused.message,
            concat!(
                "the loaded survey codes have 1 error: a rule cannot be applied as written\n",
                "2 rules checked: 1 error, 1 warning\n",
                "  by kind: 1 unknown colour, 1 key whitespace\n",
                "  rule #0  WM* (feature): error, key whitespace: key \" WM*\" has blanks around ",
                "it\n",
                "  rule #1 AC* (feature): warning, unknown colour: colour \"sui\" is not a colour ",
                "name"
            )
        );
        // Warnings alone, and nothing at all, are replies.
        assert!(code_check_reply(std::slice::from_ref(&colour), 2).is_ok());
        assert_eq!(
            code_check_reply(&[], 0).unwrap_or_default(),
            "0 rules checked: no problems found"
        );
    }

    #[test]
    fn a_subcommand_given_wrongly_is_a_usage_error() {
        let mut session = Session::new(true);
        for line in ["CODE CHECK everything", "CODE EXPLAIN"] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(
                error.message.starts_with("usage: "),
                "{line}: {}",
                error.message
            );
        }
        assert_eq!(
            session.refused("CODE CHECK everything").message,
            "usage: CODE CHECK"
        );
        // Malformed is malformed whatever is loaded.
        let mut empty = Session::new(false);
        assert_eq!(
            empty.refused("CODE CHECK everything").message,
            "usage: CODE CHECK"
        );
    }

    #[test]
    fn mapfile_is_no_longer_a_verb() {
        // Its two subcommands are CODE LIST and CODE CHECK. The word itself is
        // answered as any word that is no verb is, loaded or not.
        for loaded in [true, false] {
            let mut session = Session::new(loaded);
            for line in ["MAPFILE", "MAPFILE LIST", "MAPFILE CHECK", "mapfile check"] {
                let error = session.refused(line);
                assert_eq!(error.code, ErrorCode::ParseFailure, "{line}");
                assert_eq!(error.message, "unknown command; type HELP", "{line}");
            }
        }
    }

    #[test]
    fn the_help_names_them() {
        let help = CommandInterpreter::help_text();
        for verb in [
            "CODE [scope] [WHERE k=v ...] [PROPERTY name] [PREVIEW]",
            "CODE EXPLAIN code",
            "CODE CENSUS [scope] [WHERE k=v ...] [PROPERTY name]",
            "CODE LIST [filter]",
            "CODE CHECK",
        ] {
            assert!(help.contains(verb), "{verb}");
        }
        // Why its default is not the other verbs': said where the scope is said.
        assert!(
            help.contains("With no scope word it is the whole DRAWING, under a bare WHERE too")
        );
        assert!(!help.contains("MAPFILE"));
    }
}

// ---- the scope -------------------------------------------------------------------------------

mod survey_code_scope {
    use super::*;

    #[test]
    fn no_scope_word_is_the_whole_drawing_whatever_is_selected() {
        let mut session = scoped();
        let select = format!("SELECT {}", session.line_on_a);
        session.ok(&select);
        assert_eq!(
            session.ok("CODE CENSUS"),
            concat!(
                "scope=drawing matched=3\n",
                "3 entities carry a code in \"code\", 3 distinct codes\n",
                "  KT01: 1, prefix, matched, layer KATANA TEST\n",
                "  KT02: 1, prefix, matched, layer KATANA TEST\n",
                "  KX01: 1, prefix, matched"
            )
        );
        // Under a bare WHERE too: the two points of the drawing, not the line
        // that is selected. (MODIFY ... WHERE with no scope word is the selection.)
        assert_eq!(
            session.ok("CODE CENSUS WHERE TYPE=point"),
            concat!(
                "scope=drawing where=\"TYPE=point\" matched=2\n",
                "2 entities carry a code in \"code\", 2 distinct codes\n",
                "  KT01: 1, prefix, matched, layer KATANA TEST\n",
                "  KT02: 1, prefix, matched, layer KATANA TEST"
            )
        );
        // ALL and DRAWING say the same in so many words.
        assert!(
            session
                .ok("CODE CENSUS ALL")
                .starts_with("scope=drawing matched=3\n")
        );
        assert!(
            session
                .ok("CODE CENSUS DRAWING")
                .starts_with("scope=drawing matched=3\n")
        );
        // The selection is a scope when it is asked for.
        assert_eq!(
            session.ok("CODE CENSUS SELECTION"),
            concat!(
                "scope=selection matched=1\n",
                "1 entity carries a code in \"code\", 1 distinct code\n",
                "  KX01: 1, prefix, matched"
            )
        );
    }

    #[test]
    fn layers_and_area_take_what_they_say() {
        let mut session = scoped();
        assert_eq!(
            session.ok("CODE CENSUS LAYERS a"),
            String::from("scope=layers layers=a sublayers=yes matched=2\n") + CENSUS_OF_LAYER_A
        );
        // LAYER is LAYERS, as the shared grammar has it - when a layer follows.
        assert_eq!(
            session.ok("CODE CENSUS LAYER a ONLY"),
            String::from("scope=layers layers=a sublayers=no matched=2\n") + CENSUS_OF_LAYER_A
        );
        // A window holding the first point alone: (1,1) is inside 0,0 - 1.5,1.5,
        // (2,2) and the line from (10,10) are not.
        assert_eq!(
            session.ok("CODE CENSUS AREA 0,0,1.5,1.5"),
            concat!(
                "scope=area area=0,0,1.5,1.5 matched=1\n",
                "1 entity carries a code in \"code\", 1 distinct code\n",
                "  KT01: 1, prefix, matched, layer KATANA TEST"
            )
        );
        // The scope and its filter together, and the property named after them.
        assert_eq!(
            session.ok("CODE CENSUS LAYERS a WHERE TYPE=point PROPERTY code"),
            concat!(
                "scope=layers layers=a sublayers=yes where=\"TYPE=point\" matched=1\n",
                "1 entity carries a code in \"code\", 1 distinct code\n",
                "  KT02: 1, prefix, matched, layer KATANA TEST"
            )
        );
    }

    #[test]
    fn code_on_a_scope_changes_only_what_the_scope_took() {
        let mut session = scoped();
        let steps = session.document.history().undo_count();
        let reply = session.ok("CODE LAYERS a");
        assert!(
            reply.starts_with(concat!(
                "scope=layers layers=a sublayers=yes matched=2\n",
                "2 entities carry a code in \"code\": 2 matched, 0 ",
                "fallback-only"
            )),
            "{reply}"
        );
        assert!(
            reply.ends_with("Applied as one command. UNDO puts it all back."),
            "{reply}"
        );
        assert_eq!(session.document.history().undo_count(), steps + 1);
        // KT02 on layer a went to its rule's layer; KX01's rule gives none, so
        // the line stays; KT01 on layer 0 was outside the scope and is untouched.
        assert_eq!(session.layer_of(session.point_on_a), "KATANA TEST");
        assert_eq!(session.layer_of(session.line_on_a), "a");
        assert_eq!(session.layer_of(session.on_zero), "0");

        session.ok("UNDO");
        assert_eq!(session.layer_of(session.point_on_a), "a");
    }

    #[test]
    fn preview_reports_and_changes_nothing() {
        let mut session = scoped();
        let steps = session.document.history().undo_count();
        // PREVIEW alone is a preview of the whole drawing - never a property
        // called PREVIEW, which would have coded the drawing for real.
        for line in [
            "CODE PREVIEW",
            "code preview",
            "CODE AREA 0,0,1.5,1.5 PREVIEW",
            "CODE PREVIEW AREA 0,0,1.5,1.5",
            "CODE PROPERTY code PREVIEW",
        ] {
            let reply = session.ok(line);
            assert!(reply.starts_with("scope="), "{line}: {reply}");
            assert!(
                reply.contains("Layers created: KATANA TEST\n"),
                "{line}: {reply}"
            );
            assert!(
                reply.ends_with("Preview: nothing was changed."),
                "{line}: {reply}"
            );
            assert_eq!(session.document.history().undo_count(), steps, "{line}");
            assert_eq!(session.layer_of(session.on_zero), "0", "{line}");
            assert!(
                !session.document.model().layers.contains("KATANA TEST"),
                "{line}"
            );
        }
        // By hand, for the window holding the first point alone.
        assert_eq!(
            session.ok("CODE AREA 0,0,1.5,1.5 PREVIEW"),
            concat!(
                "scope=area area=0,0,1.5,1.5 matched=1\n",
                "1 entity carries a code in \"code\": 1 matched, 0 fallback-only (only the bare * ",
                "rule answers), 0 with no rule\n",
                "1 entity changed\n",
                "Layers created: KATANA TEST\n",
                "Styles created: cross\n",
                "By code:\n",
                "  KT01: 1 entity, prefix, matched; layer 0 -> KATANA TEST; style cross (created); ",
                "1 changed\n",
                "Preview: nothing was changed."
            )
        );
    }

    #[test]
    fn a_census_takes_preview_and_is_the_same_census() {
        // CODE and CODE CENSUS are one grammar, PREVIEW included. A census
        // changes nothing, so the word asks nothing more of it: wherever it
        // stands the reply is the census of the same scope, never a usage error
        // and never a property called PREVIEW.
        let mut session = scoped();
        let drawing = concat!(
            "scope=drawing matched=3\n",
            "3 entities carry a code in \"code\", 3 distinct codes\n",
            "  KT01: 1, prefix, matched, layer KATANA TEST\n",
            "  KT02: 1, prefix, matched, layer KATANA TEST\n",
            "  KX01: 1, prefix, matched"
        );
        for line in [
            "CODE CENSUS DRAWING PREVIEW",
            "CODE CENSUS PREVIEW",
            "code census preview drawing",
            "CODE CENSUS PREVIEW PROPERTY code",
        ] {
            assert_eq!(session.ok(line), drawing, "{line}");
        }
        assert_eq!(
            session.ok("CODE CENSUS LAYERS a PREVIEW"),
            String::from("scope=layers layers=a sublayers=yes matched=2\n") + CENSUS_OF_LAYER_A
        );
        // A property that IS called as the word is named with PROPERTY, as for
        // CODE.
        assert_eq!(
            session.ok("CODE CENSUS PROPERTY preview"),
            "scope=drawing matched=3\n0 entities carry a code in \"preview\", 0 distinct codes"
        );
    }

    #[test]
    fn view_is_the_windows_view_where_a_front_end_answers_for_one() {
        // A view showing the window 0,0 - 1.5,1.5 and hiding nothing of its own,
        // as the window hands one to the interpreter: of the three coded
        // entities it shows the first point, (1,1), alone.
        let mut session = scoped();
        let views: Box<ScopeViewFn<'static>> = Box::new(|_: Option<u32>| -> Result<ScopeView> {
            Ok(ScopeView {
                id: 3,
                area: Some(Box2::new(Point2::new(0.0, 0.0), Point2::new(1.5, 1.5))),
                ..ScopeView::default()
            })
        });
        session.interpreter.set_scope_context(Some(views));
        assert_eq!(
            session.ok("CODE CENSUS VIEW"),
            concat!(
                "scope=view view=3 area=0,0,1.5,1.5 matched=1\n",
                "1 entity carries a code in \"code\", 1 distinct code\n",
                "  KT01: 1, prefix, matched, layer KATANA TEST"
            )
        );
        // CODE on it codes that point and leaves the two the view does not show.
        let reply = session.ok("CODE VIEW");
        assert!(
            reply.starts_with(concat!(
                "scope=view view=3 area=0,0,1.5,1.5 matched=1\n",
                "1 entity carries a code in \"code\": 1 matched"
            )),
            "{reply}"
        );
        assert_eq!(session.layer_of(session.on_zero), "KATANA TEST");
        assert_eq!(session.layer_of(session.point_on_a), "a");
        assert_eq!(session.layer_of(session.line_on_a), "a");
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_and_codes_nothing() {
        // Nothing is selected. An empty list of ids means EVERY entity to
        // apply_survey_codes, so a scope that took nothing must never be handed
        // on: it would code the whole drawing.
        let mut session = scoped();
        let steps = session.document.history().undo_count();
        assert_eq!(
            session.ok("CODE SELECTION"),
            concat!(
                "scope=selection matched=0\n",
                "0 entities carry a code in \"code\": 0 matched, 0 fallback-only (only the bare * ",
                "rule answers), 0 with no rule\n",
                "0 entities changed\n",
                "Nothing to change."
            )
        );
        assert_eq!(session.document.history().undo_count(), steps);
        assert_eq!(session.layer_of(session.on_zero), "0");
        assert_eq!(session.layer_of(session.point_on_a), "a");
        assert_eq!(
            session.ok("CODE CENSUS SELECTION PROPERTY feature_code"),
            concat!(
                "scope=selection matched=0\n",
                "0 entities carry a code in \"feature_code\", 0 distinct codes"
            )
        );
        // An empty window, and a filter nothing meets.
        assert!(
            session
                .ok("CODE AREA 100,100,101,101")
                .starts_with("scope=area area=100,100,101,101 matched=0\n")
        );
        assert!(
            session
                .ok("CODE WHERE TYPE=circle")
                .ends_with("Nothing to change.")
        );
        assert_eq!(session.document.history().undo_count(), steps);
    }

    #[test]
    fn a_word_that_cannot_begin_a_scope_is_still_the_property() {
        // "Layer" is an attribute many drawings from GIS data carry. LAYER needs
        // the layers after it, so alone it is the property it always was - and
        // AREA, which needs its window, and PROPERTY, which needs its name, are
        // read the same way.
        let mut session = scoped();
        let select = format!("SELECT {}", session.on_zero);
        session.ok(&select);
        session.ok("PROP SET Layer KT09");
        session.ok("SELECT NONE");
        assert_eq!(
            session.ok("CODE CENSUS Layer"),
            concat!(
                "scope=drawing matched=3\n",
                "1 entity carries a code in \"Layer\", 1 distinct code\n",
                "  KT09: 1, prefix, matched, layer KATANA TEST"
            )
        );
        assert_eq!(
            session.ok("CODE CENSUS AREA"),
            "scope=drawing matched=3\n0 entities carry a code in \"AREA\", 0 distinct codes"
        );
        assert_eq!(
            session.ok("CODE CENSUS PROPERTY"),
            "scope=drawing matched=3\n0 entities carry a code in \"PROPERTY\", 0 distinct codes"
        );
        // A name of two words is still the rest of the line.
        assert_eq!(
            session.ok("CODE CENSUS field code"),
            "scope=drawing matched=3\n0 entities carry a code in \"field code\", 0 distinct codes"
        );
        // CODE itself reads it so too: coded under "Layer", the one entity that
        // carries it goes to KT*'s layer.
        let reply = session.ok("CODE Layer");
        assert!(
            reply.starts_with(concat!(
                "scope=drawing matched=3\n",
                "1 entity carries a code in \"Layer\": 1 matched"
            )),
            "{reply}"
        );
        assert_eq!(session.layer_of(session.on_zero), "KATANA TEST");
        assert_eq!(
            session.layer_of(session.point_on_a),
            "a",
            "coded under \"code\", not \"Layer\""
        );

        // A property that IS called as a scope word that stands alone, or as a
        // subcommand, is named with PROPERTY.
        assert_eq!(
            session.ok("CODE CENSUS PROPERTY ALL"),
            "scope=drawing matched=3\n0 entities carry a code in \"ALL\", 0 distinct codes"
        );
        assert_eq!(
            session.ok("CODE CENSUS PROPERTY LIST"),
            "scope=drawing matched=3\n0 entities carry a code in \"LIST\", 0 distinct codes"
        );
        assert_eq!(
            session.ok("CODE CENSUS PROPERTY Layer LAYERS a"),
            concat!(
                "scope=layers layers=a sublayers=yes matched=2\n",
                "0 entities carry a code in \"Layer\", 0 distinct codes"
            )
        );
    }

    #[test]
    fn a_property_of_several_words_whose_first_is_a_scope_word_is_named_with_property() {
        // With a word after it LAYER begins the scope form, and nothing can tell
        // "Layer a", a property of two words, from the layer a: the line is the
        // layer. "Area m2" is then a window that is not four numbers. Each is
        // still a property when it is named as one, quoted to be one word.
        let mut session = scoped();
        assert_eq!(
            session.ok("CODE CENSUS Layer a"),
            String::from("scope=layers layers=a sublayers=yes matched=2\n") + CENSUS_OF_LAYER_A
        );
        let area = session.refused("CODE CENSUS Area m2");
        assert_eq!(area.code, ErrorCode::ParseFailure);
        assert_eq!(area.context, "m2");
        assert_eq!(
            session.ok("CODE CENSUS PROPERTY \"Layer a\""),
            "scope=drawing matched=3\n0 entities carry a code in \"Layer a\", 0 distinct codes"
        );
        assert_eq!(
            session.ok("CODE CENSUS PROPERTY \"Area m2\""),
            "scope=drawing matched=3\n0 entities carry a code in \"Area m2\", 0 distinct codes"
        );
        // A property of several words whose first is none of those words is the
        // rest of the line, quotes or none.
        assert_eq!(
            session.ok("CODE CENSUS Area_m2 of lot"),
            concat!(
                "scope=drawing matched=3\n",
                "0 entities carry a code in \"Area_m2 of lot\", 0 distinct codes"
            )
        );
    }

    #[test]
    fn what_the_grammar_refuses() {
        let mut session = scoped();
        let steps = session.document.history().undo_count();
        // A word that is neither the scope's nor the verb's own.
        for line in [
            "CODE DRAWING everything",
            "CODE LAYERS a b",
            "CODE PROPERTY a PROPERTY b",
            "CODE DRAWING PROPERTY",
            "CODE DRAWING PREVIEW WHERE TYPE=point",
        ] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert_eq!(
                error.message,
                "usage: CODE [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]",
                "{line}"
            );
        }
        // The census has the same grammar and its own usage line.
        for line in [
            "CODE CENSUS DRAWING everything",
            "CODE CENSUS PROPERTY a PROPERTY b",
            "CODE CENSUS DRAWING PROPERTY",
        ] {
            assert_eq!(
                session.refused(line).message,
                "usage: CODE CENSUS [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]",
                "{line}"
            );
        }
        // The shared parser's own refusals come through as they are.
        assert_eq!(
            session.refused("CODE DRAWING SELECTION").message,
            "give one scope: SELECTION, DRAWING, VIEW, AREA or LAYERS"
        );
        assert_eq!(
            session.refused("CODE WHERE TYPE=blob").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session.refused("CODE AREA 1,2,3").code,
            ErrorCode::ParseFailure
        );
        // A layer the drawing lacks, and the window's view where there is none.
        assert_eq!(
            session.refused("CODE LAYERS nowhere").code,
            ErrorCode::NotFound
        );
        let view = session.refused("CODE VIEW");
        assert_eq!(view.code, ErrorCode::InvalidState);
        assert!(
            view.message.contains("AREA x0,y0,x1,y1"),
            "{}",
            view.message
        );
        assert_eq!(session.document.history().undo_count(), steps);
        assert_eq!(session.layer_of(session.on_zero), "0");
    }
}
