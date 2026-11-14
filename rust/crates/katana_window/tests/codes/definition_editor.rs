//! The twins of tests/qt_widgets/customisation/test_definition_editor.cpp: the definition editor -
//! one linestyle or symbol definition of the session's customisation made, changed, copied and
//! deleted, driven by object name as a person - or the headless driver - drives it.
//!
//! Nothing is read from a file. The customisation the tests edit is written out here, and every
//! definition a test expects in the library afterwards is built by hand beside the assertion:
//!
//! ```text
//!   library   "TEST Valve"  a symbol, at vertices, paper, group Test/Marks, from "Site": a 2 x 2
//!                           cross through the origin and a circle of radius 0.5 round it
//!             "TEST Kerb"   a linestyle, paper, length 12, group Test/Lines, from "Site": one
//!                           dash of 8
//!             "TEST Spare"  a symbol, at vertices, made in a session: a dot
//!   rules     #0  WM*  feature  linestyle "TEST Kerb"
//!             #1  AC*  symbol   symbol    "TEST Valve"
//!   styles    Marks    symbol "TEST Valve"
//! ```
//!
//! So "TEST Valve" is used by rule #1 and the style Marks, "TEST Kerb" by rule #0, and "TEST
//! Spare" by nothing.
//!
//! The editor does not remove a definition: it builds the line CUSTOMISE REMOVE and hands it to
//! the host. Most tests here give it a runner of their own that does what the verb does - refuse a
//! definition something uses until FORCE - so that what is asserted is the editor's: the lines it
//! built, and the runner's own words shown as they came. ONE test gives it the verb itself, the
//! interpreter over the same Document.
//!
//! Left for the modules they wait for: ThePreviewDrawsWhatIsTypedBeforeAnyOfItIsSaved (the style
//! preview's pictures, CU1's style_preview), and the five that install the customisation
//! workbench (TheSymbolLibrarysFourButtonsOpenTheOneEditorTheWorkbenchKeeps,
//! ASymbolMadeHereIsListedByTheSymbolLibraryThoughItIsNotAtVertices,
//! TheLinetypesTabOpensTheSameEditorOnALibraryLinestyle,
//! TheEditorGoesWithTheWorkbenchBeforeTheDocument, TheWindowIsNotClosedOverAFormThatIsNotSaved).

use std::cell::Cell;
use std::rc::Rc;

use katana_cad::definition_users::definition_users;
use katana_cad::Document;
use katana_commands::{create_layer, create_linetype, create_style};
use katana_entity::customisation::definition_to_json;
use katana_entity::{
    Layer, LineStyle, Linetype, LinetypeElement, Stroke, StrokeOp, StrokeText, Style,
    StyleLibrary, StyleUnits, SurveyMap, SurveyRule, SurveySection, SurveySymbol,
};
use katana_geometry::Point2;
use katana_window::customisation::codes_seams::{
    BeginCommit, CodesContext, Committed, DefinitionEdit,
};
use katana_window::customisation::definition_editor::DefinitionEditorDialog;
use katana_window::drive::Harness;
use katana_window::host::{Answer, Host, HostParts, RecordingHost, VerbOutcome};

use crate::support::Bench;
use crate::window_support::interpreter_host;

type Editor = Bench<DefinitionEditorDialog>;

fn mv(x: f64, y: f64) -> Stroke {
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

fn round(op: StrokeOp, radius: f64) -> Stroke {
    Stroke {
        op,
        radius,
        ..Stroke::default()
    }
}

fn text_stroke(text: usize) -> Stroke {
    Stroke {
        op: StrokeOp::Text,
        text,
        ..Stroke::default()
    }
}

fn valve() -> LineStyle {
    LineStyle {
        name: String::from("TEST Valve"),
        group: String::from("Test/Marks"),
        units: StyleUnits::Paper,
        at_vertices: true,
        symbol: true,
        source: String::from("Site"),
        strokes: vec![
            mv(-1.0, 0.0),
            draw(1.0, 0.0),
            mv(0.0, -1.0),
            draw(0.0, 1.0),
            mv(0.0, 0.0),
            round(StrokeOp::Circle, 0.5),
        ],
        ..LineStyle::default()
    }
}

fn kerb() -> LineStyle {
    LineStyle {
        name: String::from("TEST Kerb"),
        group: String::from("Test/Lines"),
        units: StyleUnits::Paper,
        length: 12.0,
        source: String::from("Site"),
        strokes: vec![mv(0.0, 0.0), draw(8.0, 0.0)],
        ..LineStyle::default()
    }
}

fn spare() -> LineStyle {
    LineStyle {
        name: String::from("TEST Spare"),
        at_vertices: true,
        symbol: true,
        strokes: vec![mv(0.0, 0.0), round(StrokeOp::Dot, 0.0)],
        ..LineStyle::default()
    }
}

/// CUSTOMISE REMOVE as its design gives it, for the two lines the editor builds: a definition
/// something names is refused until FORCE, and one removed leaves the library through
/// Document::set_style_library.
fn remove_as_the_verb_would(document: &mut Document, line: &str) -> VerbOutcome {
    let parsed = line
        .strip_prefix("CUSTOMISE REMOVE \"")
        .and_then(|rest| rest.split_once('"'))
        .and_then(|(name, tail)| match tail {
            "" if !name.is_empty() => Some((name, false)),
            " FORCE" if !name.is_empty() => Some((name, true)),
            _ => None,
        });
    let Some((name, force)) = parsed else {
        return VerbOutcome::new(false, "", &format!("not a line this runner knows: {line}"));
    };
    if !force && !definition_users(document, name).empty() {
        return VerbOutcome::new(false, "", "the definition is used");
    }
    let mut library = document.style_library().clone();
    if library.remove(name).is_err() {
        return VerbOutcome::new(false, "", "no such definition");
    }
    document.set_style_library(library);
    VerbOutcome::new(true, "", "")
}

/// The customisation of the file comment, in a drawing.
fn fixture_document() -> Document {
    let mut document = Document::new(None);
    let mut library = StyleLibrary::new();
    for each in [valve(), kerb(), spare()] {
        library.add(each).expect("the definition is added");
    }
    document.set_style_library(library);

    let mut map = SurveyMap::new();
    map.add(SurveyRule {
        key: String::from("WM*"),
        linestyle: String::from("TEST Kerb"),
        ..SurveyRule::default()
    })
    .expect("the rule is added");
    map.add(SurveyRule {
        key: String::from("AC*"),
        section: SurveySection::VertexSymbol,
        symbol: Some(SurveySymbol {
            style: String::from("TEST Valve"),
            ..SurveySymbol::default()
        }),
        ..SurveyRule::default()
    })
    .expect("the rule is added");
    document.set_survey_map(map);

    document
        .execute(create_style(Style {
            name: String::from("Marks"),
            symbol: String::from("TEST Valve"),
            ..Style::default()
        }))
        .expect("the style is made");
    document
}

/// A host whose runner does what the verb does (remove_as_the_verb_would).
fn fixture_host() -> RecordingHost {
    RecordingHost::with_document(fixture_document()).answering(Answer::Custom(Box::new(
        |parts: &mut HostParts, line: &str| remove_as_the_verb_would(&mut parts.document, line),
    )))
}

fn editor_with(host: RecordingHost, context: CodesContext) -> Editor {
    Bench::new(host, |document| {
        DefinitionEditorDialog::new(document, context)
    })
}

fn editor(host: RecordingHost) -> Editor {
    editor_with(host, CodesContext::default())
}

fn in_library(bench: &Editor, name: &str) -> LineStyle {
    bench
        .document()
        .style_library()
        .find(name)
        .cloned()
        .unwrap_or_else(|| panic!("{name} is in the library"))
}

fn has_definition(bench: &Editor, name: &str) -> bool {
    bench.document().style_library().contains(name)
}

/// The library with `definition` added, installed.
fn add_to_library(document: &mut Document, definition: &LineStyle) {
    let mut library = document.style_library().clone();
    library
        .add(definition.clone())
        .expect("the definition is added");
    document.set_style_library(library);
}

fn edit(bench: &mut Editor, name: &str) -> bool {
    let opened = bench.dialog.edit_definition(&mut bench.host, name);
    bench.draw();
    opened
}

fn new_definition(bench: &mut Editor, symbol: bool) -> bool {
    let opened = bench.dialog.new_definition(&mut bench.host, symbol);
    bench.draw();
    opened
}

fn duplicate(bench: &mut Editor, name: &str) -> bool {
    let opened = bench.dialog.duplicate_definition(&mut bench.host, name);
    bench.draw();
    opened
}

fn strokes(bench: &mut Editor, text: &str) {
    bench.fill("definitionStrokes", text);
}

fn issues(bench: &Editor) -> String {
    String::from(bench.dialog.issues())
}

/// A button the page is offering: shown and pressable ITSELF.
fn offered(bench: &Editor, name: &str) -> bool {
    let button = bench.control(name);
    button.visible && button.enabled
}

fn withdrawn(bench: &Editor, name: &str) -> bool {
    let button = bench.control(name);
    !button.visible && !button.enabled
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| String::from(*line)).collect()
}

const REMOVE_VALVE: &str = "CUSTOMISE REMOVE \"TEST Valve\"";
const REMOVE_VALVE_FORCE: &str = "CUSTOMISE REMOVE \"TEST Valve\" FORCE";

mod definition_editor {
    use super::*;

    #[test]
    fn it_is_one_non_modal_dialog_with_a_name_for_every_control() {
        let bench = editor(fixture_host());
        let target = bench
            .harness
            .target_record("definitionEditorDialog")
            .expect("the dialog is a target");
        assert!(!target.modal);
        for name in [
            "definitionHeading",
            "definitionName",
            "definitionGroup",
            "definitionKind",
            "definitionUnits",
            "definitionUnitsNote",
            "definitionAtVertices",
            "definitionLength",
            "definitionFactor",
            "definitionOriginX",
            "definitionOriginY",
            "definitionAnchor1X",
            "definitionAnchor1Y",
            "definitionAnchor2X",
            "definitionAnchor2Y",
            "definitionStretchMode",
            "definitionCycleMode",
            "definitionSource",
            "definitionStrokes",
            "definitionStrokesHelp",
            "definitionPreview",
            "definitionLinePreview",
            "definitionPreviewNote",
            "definitionPlotScale",
            "definitionIssues",
            "definitionDeletePanel",
            "definitionDeleteAnyway",
            "definitionDeleteCancel",
            "definitionNew",
            "definitionDuplicate",
            "definitionDelete",
            "definitionRevert",
            "definitionSave",
            "definitionClose",
        ] {
            assert!(bench.has(name), "{name}");
        }
    }

    #[test]
    fn a_symbol_typed_as_strokes_is_saved_into_the_library_as_the_definition_written_out_by_hand()
    {
        let mut bench = editor(fixture_host());
        assert!(new_definition(&mut bench, true));
        assert_eq!(bench.text("definitionHeading"), "New symbol");
        // Nothing to save yet: a definition is named.
        assert!(!bench.enabled("definitionSave"));
        assert!(issues(&bench).starts_with("Type a name"));

        // A blank at either end of a new name is not part of it.
        bench.fill("definitionName", " TEST Post ");
        bench.fill("definitionGroup", "Test/Marks");
        bench.fill("definitionUnits", "paper");
        bench.fill("definitionFactor", "2");
        bench.fill("definitionOriginX", "0.5");
        bench.fill("definitionOriginY", "-0.25");
        // One of each kind of stroke, as a file holds them; the second line is without the comma
        // that would end it there, and a blank line is passed over.
        strokes(
            &mut bench,
            concat!(
                "[\"move\", 0, 0],\n",
                "[\"draw\", 0, 2.5]\n",
                "\n",
                "[\"arc\", 1, 0, 180],\n",
                "[\"pen\", \"red\"],\n",
                "[\"circle\", 0.25],\n",
                "[\"dot\", 0],\n",
                "[\"text\", {\"text\": \"P\", \"height\": 1.5, ",
                "\"justify\": \"middle-centre\"}]"
            ),
        );
        assert!(bench.enabled("definitionSave"), "{}", issues(&bench));
        let generation = bench.document().library_generation();
        bench.press("definitionSave");

        let expected = LineStyle {
            name: String::from("TEST Post"),
            group: String::from("Test/Marks"),
            units: StyleUnits::Paper,
            factor: 2.0,
            origin: Point2::new(0.5, -0.25),
            // New from the symbol library's side: listed as a symbol.
            symbol: true,
            // Made in this session, of no customisation.
            source: String::new(),
            strokes: vec![
                mv(0.0, 0.0),
                draw(0.0, 2.5),
                Stroke {
                    op: StrokeOp::Arc,
                    radius: 1.0,
                    start_angle: 0.0,
                    end_angle: 180.0,
                    ..Stroke::default()
                },
                Stroke {
                    op: StrokeOp::Pen,
                    pen: String::from("red"),
                    ..Stroke::default()
                },
                round(StrokeOp::Circle, 0.25),
                round(StrokeOp::Dot, 0.0),
                text_stroke(0),
            ],
            texts: vec![StrokeText {
                text: String::from("P"),
                height: 1.5,
                justify: String::from("middle-centre"),
                ..StrokeText::default()
            }],
            ..LineStyle::default()
        };

        assert_eq!(bench.document().style_library().size(), 4);
        assert!(in_library(&bench, "TEST Post") == expected);
        // One commit, and nothing else in the library touched by it.
        assert_eq!(bench.document().library_generation(), generation + 1);
        assert!(in_library(&bench, "TEST Valve") == valve());
        assert!(in_library(&bench, "TEST Kerb") == kerb());
        assert!(
            bench.logged_exactly("Symbol \"TEST Post\" added to the library: 7 strokes, 1 text.")
        );

        // It exists now: its name is fixed, and there is nothing left to save.
        assert_eq!(bench.dialog.edited_name(), "TEST Post");
        assert_eq!(bench.value("definitionName"), "TEST Post");
        assert!(!bench.enabled("definitionName"));
        assert!(!bench.dialog.dirty());
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.text("definitionHeading"), "Symbol \"TEST Post\"");
        assert_eq!(bench.text("definitionSource"), "Made in this session");
    }

    #[test]
    fn an_existing_definition_is_shown_as_the_format_writes_it_and_its_length_is_changed() {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));

        assert_eq!(bench.value("definitionName"), "TEST Kerb");
        assert!(
            !bench.enabled("definitionName"),
            "a name is fixed once it exists"
        );
        assert_eq!(bench.value("definitionGroup"), "Test/Lines");
        assert_eq!(bench.value("definitionKind"), "Linestyle");
        assert_eq!(bench.value("definitionUnits"), "paper");
        assert!(!bench.checked("definitionAtVertices"));
        assert_eq!(bench.value("definitionLength"), "12");
        assert_eq!(bench.value("definitionFactor"), "1");
        assert_eq!(bench.text("definitionSource"), "Site");
        assert_eq!(
            bench.value("definitionStrokes"),
            "[\"move\", 0, 0],\n[\"draw\", 8, 0]"
        );
        // What names it is said while the form reads: rule #0 is the only one.
        assert_eq!(
            issues(&bench),
            "Named by:\n  rule #0 WM* (feature) names it as its linestyle"
        );
        assert_eq!(bench.value("definitionIssues"), issues(&bench));

        // Opened and not touched: nothing to save or to go back to.
        assert!(!bench.dialog.dirty());
        assert!(!bench.enabled("definitionSave"));
        assert!(!bench.enabled("definitionRevert"));

        bench.fill("definitionLength", "15");
        assert!(bench.dialog.dirty());
        assert_eq!(
            bench.text("definitionHeading"),
            "Linestyle \"TEST Kerb\" - edited, not saved"
        );
        assert!(bench.enabled("definitionSave"));
        assert!(
            in_library(&bench, "TEST Kerb") == kerb(),
            "nothing reaches the library before Save"
        );
        bench.press("definitionSave");

        let expected = LineStyle {
            length: 15.0,
            ..kerb()
        };
        assert!(in_library(&bench, "TEST Kerb") == expected);
        assert_eq!(
            bench.document().style_library().size(),
            3,
            "replaced, not added beside"
        );
        assert!(bench.logged_exactly("Linestyle \"TEST Kerb\" saved: 2 strokes, 0 texts."));
        assert!(!bench.dialog.dirty());

        // Revert goes back to what the form was opened with.
        bench.fill("definitionLength", "99");
        bench.press("definitionRevert");
        assert_eq!(bench.value("definitionLength"), "15");
        assert!(!bench.dialog.dirty());
        assert!(in_library(&bench, "TEST Kerb") == expected);
    }

    #[test]
    fn an_untouched_form_describes_exactly_the_definition_it_was_opened_on() {
        // Numbers no decimal field would hold - 0.1 + 0.2, a third - a negative radius, anchors
        // and modes on a definition that is not two-point, and a text with every member: a Save
        // of an untouched form, or of one where only the group was changed, may alter none of
        // them.
        let awkward = LineStyle {
            name: String::from("TEST Awkward"),
            group: String::from("Test/Odd"),
            units: StyleUnits::World,
            length: 0.1 + 0.2,
            factor: 1.0 / 3.0,
            origin: Point2::new(1e-7, -2.5e8), // within the format's bound of 1e9 in size
            anchor1: Point2::new(0.25, 0.5),
            anchor2: Point2::new(-4.0, 8.0),
            stretch_mode: 2,
            cycle_mode: 1,
            source: String::from("Site"),
            texts: vec![StrokeText {
                text: String::from("W \"1\""),
                angle: 30.0,
                height: 1.5,
                justify: String::from("top-left"),
                font: String::from("Arial"),
                width_factor: 0.8,
                unnamed: [0.0, -0.3, 0.035],
            }],
            strokes: vec![
                mv(0.1, 0.2),
                Stroke {
                    op: StrokeOp::Arc,
                    radius: -1.5,
                    start_angle: 10.0,
                    end_angle: 350.0,
                    ..Stroke::default()
                },
                text_stroke(0),
            ],
            ..LineStyle::default()
        };
        let mut host = fixture_host();
        add_to_library(host.document_mut(), &awkward);

        let mut bench = editor(host);
        assert!(edit(&mut bench, "TEST Awkward"));
        let read = bench.dialog.definition().expect("the form reads");
        assert!(read == awkward);
        // Not two-point: its anchors and modes are shown and cannot be typed into.
        assert!(!bench.enabled("definitionAnchor1X"));
        assert!(!bench.enabled("definitionCycleMode"));
        assert_eq!(bench.value("definitionAnchor2Y"), "8");
        assert_eq!(bench.value("definitionStretchMode"), "2");

        bench.fill("definitionGroup", "Test/Other");
        bench.press("definitionSave");
        let expected = LineStyle {
            group: String::from("Test/Other"),
            ..awkward
        };
        assert!(in_library(&bench, "TEST Awkward") == expected);
    }

    #[test]
    fn a_stroke_line_with_a_typo_keeps_save_disabled_and_names_the_line() {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Valve"));

        // A word that is no kind of stroke, on the third line. The reader's own refusal is shown,
        // which names the stroke by its place counted from 0.
        strokes(
            &mut bench,
            "[\"move\", -1, 0],\n[\"draw\", 1, 0],\n[\"drow\", 0, -1],\n[\"draw\", 0, 1]",
        );
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.dialog.problem_line(), 3);
        assert!(
            issues(&bench).starts_with(
                "Line 3: definition \"TEST Valve\" strokes[2]: \"drow\" is not a kind of stroke"
            ),
            "{}",
            issues(&bench)
        );
        // The line is washed in the box, and the message is in a refusal's colour.
        assert_eq!(bench.dialog.washed_lines(), vec![3]);
        assert!(bench.dialog.says_an_error());
        assert!(!bench.dialog.save(&mut bench.host));
        assert!(bench.dialog.definition().is_err());
        assert!(in_library(&bench, "TEST Valve") == valve());

        // Text that is not JSON at all - a comma missing between two numbers - on the fourth line
        // of the box, a blank line above it: the line is the box's, not the stroke's place.
        strokes(
            &mut bench,
            "[\"move\", -1, 0],\n\n[\"draw\", 1, 0],\n[\"draw\", 0 1]",
        );
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.dialog.problem_line(), 4);
        assert!(
            issues(&bench).starts_with("Line 4 is not a stroke: "),
            "{}",
            issues(&bench)
        );
        assert_eq!(bench.dialog.washed_lines(), vec![4]);
        // The reader's own place - a line and column of the text the lines are read in, which
        // nobody typed - is not passed on.
        assert!(!issues(&bench).contains("column"), "{}", issues(&bench));

        // A stroke with a value short: an arc takes three.
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"arc\", 1, 90]");
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.dialog.problem_line(), 2);
        assert!(
            issues(&bench).starts_with(
                "Line 2: definition \"TEST Valve\" strokes[1]: \"arc\" takes a radius, a start \
                 angle and an end angle (3 after the word) and has 2"
            ),
            "{}",
            issues(&bench)
        );

        // What entity's validate refuses of a stroke - a text below the line - is named by its
        // line as well.
        strokes(
            &mut bench,
            "[\"move\", 0, 0],\n[\"text\", {\"text\": \"W\", \"height\": -1}]",
        );
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.dialog.problem_line(), 2);
        assert!(
            issues(&bench).contains("linestyle text height is negative"),
            "{}",
            issues(&bench)
        );

        // A line that is nothing but a comma is not a stroke either.
        strokes(&mut bench, "[\"move\", 0, 0],\n,\n[\"draw\", 1, 0]");
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.dialog.problem_line(), 2);

        // A number no double holds, and a line short of its closing bracket: each said of the
        // line typed, with nothing of the text it is read in - no "line 2, column 14", and no "}"
        // where it is a "]" that is missing.
        strokes(&mut bench, "[\"move\", 1e999, 0]");
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(
            issues(&bench),
            "Line 1: definition \"TEST Valve\" strokes[0]: this stroke has a number too large to \
             hold - 1e999"
        );
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"draw\", 1, 0");
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.dialog.problem_line(), 2);
        assert!(
            issues(&bench).starts_with("Line 2 is not a stroke: "),
            "{}",
            issues(&bench)
        );
        assert!(issues(&bench).contains("expected ']'"), "{}", issues(&bench));
        assert!(!issues(&bench).contains('}'), "{}", issues(&bench));

        // Put right, it saves; the wash and the colour go.
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"draw\", 1, 0]");
        assert_eq!(bench.dialog.problem_line(), 0);
        assert!(bench.dialog.washed_lines().is_empty());
        assert!(!bench.dialog.says_an_error());
        assert!(bench.enabled("definitionSave"), "{}", issues(&bench));
        bench.press("definitionSave");
        let expected = LineStyle {
            strokes: vec![mv(0.0, 0.0), draw(1.0, 0.0)],
            ..valve()
        };
        assert!(in_library(&bench, "TEST Valve") == expected);
    }

    #[test]
    fn the_strokes_box_holds_strokes_and_nothing_else_of_the_definition() {
        // A line that ends the list of strokes and goes on to give members: as JSON it reads, and
        // would save a definition at vertices, with anchors, under a form whose box is unticked
        // and whose anchors read 0.
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));
        strokes(
            &mut bench,
            "[\"move\", 0, 0]], \"atVertices\": true, \"anchors\": [[0, 0], [1, 1]",
        );
        assert!(!bench.enabled("definitionSave"));
        assert!(
            issues(&bench).starts_with("The strokes hold more than strokes"),
            "{}",
            issues(&bench)
        );
        assert!(!bench.dialog.save(&mut bench.host));
        assert!(in_library(&bench, "TEST Kerb") == kerb());
    }

    #[test]
    fn a_number_that_does_not_read_or_that_a_definition_may_not_have_keeps_save_disabled() {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));

        bench.fill("definitionLength", "twelve");
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(issues(&bench), "Length: \"twelve\" is not a number.");
        // A decimal comma is not a decimal point.
        bench.fill("definitionLength", "1,5");
        assert!(!bench.enabled("definitionSave"));
        // Nor is anything that is not finite a length: the words for infinity and for no number,
        // and a number past the largest a double holds.
        for not_finite in ["inf", "-inf", "nan", "NaN", "Infinity", "1e999"] {
            bench.fill("definitionLength", not_finite);
            assert!(!bench.enabled("definitionSave"), "{not_finite}");
            assert_eq!(
                issues(&bench),
                format!("Length: \"{not_finite}\" is not a number.")
            );
            assert!(!bench.dialog.save(&mut bench.host), "{not_finite}");
        }
        assert!(bench.dialog.says_an_error());

        // entity's validate's two rules for a definition's own numbers, in its words.
        bench.fill("definitionLength", "-1");
        assert!(!bench.enabled("definitionSave"));
        assert!(
            issues(&bench).contains("linestyle length must be finite and not negative"),
            "{}",
            issues(&bench)
        );
        bench.fill("definitionLength", "12");
        bench.fill("definitionFactor", "0");
        assert!(!bench.enabled("definitionSave"));
        assert!(
            issues(&bench).contains("linestyle factor must be finite and greater than zero"),
            "{}",
            issues(&bench)
        );
        assert!(!bench.dialog.save(&mut bench.host));
        assert!(in_library(&bench, "TEST Kerb") == kerb());

        // An empty number is the value a file's missing member has: no length said, a factor of
        // 1.
        bench.fill("definitionLength", "");
        bench.fill("definitionFactor", "");
        assert!(bench.enabled("definitionSave"), "{}", issues(&bench));
        bench.press("definitionSave");
        let expected = LineStyle {
            length: 0.0,
            factor: 1.0,
            ..kerb()
        };
        assert!(in_library(&bench, "TEST Kerb") == expected);
    }

    #[test]
    fn a_name_the_library_holds_cannot_be_made_a_second_time() {
        let mut bench = editor(fixture_host());
        assert!(new_definition(&mut bench, true));
        bench.fill("definitionName", "TEST Valve");
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"draw\", 3, 0]");

        // It reads as a definition, and is still not saved over the one there is.
        assert!(bench.dialog.definition().is_ok());
        assert!(!bench.enabled("definitionSave"));
        assert!(
            issues(&bench).starts_with("\"TEST Valve\" is already in the library."),
            "{}",
            issues(&bench)
        );
        assert!(!bench.dialog.save(&mut bench.host));
        assert!(in_library(&bench, "TEST Valve") == valve());
        assert_eq!(bench.document().style_library().size(), 3);

        // Under a name of its own it is; and that name is then taken too.
        bench.fill("definitionName", "TEST Valve B");
        assert!(bench.enabled("definitionSave"), "{}", issues(&bench));
        bench.press("definitionSave");
        assert_eq!(bench.document().style_library().size(), 4);
        assert!(new_definition(&mut bench, true));
        bench.fill("definitionName", "TEST Valve B");
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(bench.document().style_library().size(), 4);
    }

    #[test]
    fn a_name_rules_already_give_is_said_while_it_is_being_made() {
        // Rule #1 could as well name a symbol nothing defines yet; making it is how that is put
        // right, and the form says who is waiting for it.
        let mut host = fixture_host();
        let mut map = host.document().survey_map().clone();
        map.add(SurveyRule {
            key: String::from("PX*"),
            section: SurveySection::VertexSymbol,
            symbol: Some(SurveySymbol {
                style: String::from("TEST Pit"),
                ..SurveySymbol::default()
            }),
            ..SurveyRule::default()
        })
        .expect("the rule is added");
        host.document_mut().set_survey_map(map);

        let mut bench = editor(host);
        assert!(new_definition(&mut bench, true));
        bench.fill("definitionName", "TEST Pit");
        assert!(
            issues(&bench).contains("rule #2 PX* (symbol) draws it as its symbol"),
            "{}",
            issues(&bench)
        );
        assert!(bench.enabled("definitionSave"));
    }

    #[test]
    fn duplicate_makes_a_second_definition_equal_but_for_its_name() {
        let mut bench = editor(fixture_host());
        assert!(duplicate(&mut bench, "TEST Valve"));

        // The first free name, which can still be changed: the copy does not exist until it is
        // saved.
        assert_eq!(bench.value("definitionName"), "TEST Valve 2");
        assert!(bench.enabled("definitionName"));
        assert_eq!(
            bench.text("definitionHeading"),
            "New symbol, a copy of \"TEST Valve\""
        );
        assert_eq!(bench.dialog.edited_name(), "");
        assert_eq!(bench.document().style_library().size(), 3);
        assert!(bench.enabled("definitionSave"));
        bench.press("definitionSave");

        let mut expected = valve();
        expected.name = String::from("TEST Valve 2");
        assert!(in_library(&bench, "TEST Valve 2") == expected);
        assert!(
            in_library(&bench, "TEST Valve") == valve(),
            "the original is as it was"
        );
        assert_eq!(bench.document().style_library().size(), 4);

        // A second copy takes the next free name; one given a name of its own keeps everything
        // else.
        assert!(duplicate(&mut bench, "TEST Valve"));
        assert_eq!(bench.value("definitionName"), "TEST Valve 3");
        bench.fill("definitionName", "TEST Hydrant");
        bench.press("definitionSave");
        expected.name = String::from("TEST Hydrant");
        assert!(in_library(&bench, "TEST Hydrant") == expected);

        // The editor's own Duplicate copies the definition it is on.
        bench.press("definitionDuplicate");
        assert_eq!(bench.value("definitionName"), "TEST Hydrant 2");
    }

    #[test]
    fn delete_of_a_definition_a_rule_names_is_refused_listing_the_rule_and_delete_anyway_removes_it()
     {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Valve"));
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert!(withdrawn(&bench, "definitionDeleteCancel"));
        assert!(!bench.dialog.delete_anyway_offered(bench.host.document()));

        bench.press("definitionDelete");
        // The plain line, as it would be typed; refused, since rule #1 and the style Marks name
        // the definition. What the line answered comes first, word for word; then what names it;
        // then what Delete Anyway would do - a point is all that draws it, so a stand-in mark is
        // all that follows.
        assert_eq!(bench.lines(), strings(&[REMOVE_VALVE]));
        assert!(in_library(&bench, "TEST Valve") == valve());
        assert_eq!(
            issues(&bench),
            "\"TEST Valve\" was not deleted: the definition is used\n\
             It is named by:\n  rule #1 AC* (symbol) draws it as its symbol\n  style \"Marks\" \
             draws it as its symbol\n\
             Delete Anyway runs the line again with FORCE. A point naming it is then drawn as a \
             stand-in mark."
        );
        assert!(bench.dialog.says_an_error());
        assert!(offered(&bench, "definitionDeleteAnyway"));
        assert!(offered(&bench, "definitionDeleteCancel"));
        assert!(bench.dialog.delete_anyway_offered(bench.host.document()));

        // Keep It takes the offer away, and nothing more is run.
        bench.press("definitionDeleteCancel");
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert_eq!(bench.lines().len(), 1);
        assert_eq!(
            issues(&bench),
            "Named by:\n  rule #1 AC* (symbol) draws it as its symbol\n  style \"Marks\" draws it \
             as its symbol"
        );

        bench.press("definitionDelete");
        assert!(offered(&bench, "definitionDeleteAnyway"));
        bench.press("definitionDeleteAnyway");
        assert_eq!(
            bench.lines(),
            strings(&[REMOVE_VALVE, REMOVE_VALVE, REMOVE_VALVE_FORCE])
        );
        assert!(!has_definition(&bench, "TEST Valve"));
        assert_eq!(bench.document().style_library().size(), 2);
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert!(
            issues(&bench).starts_with(
                "\"TEST Valve\" was deleted from the library. It is still shown here: Save puts \
                 it back."
            ),
            "{}",
            issues(&bench)
        );
        assert_eq!(
            bench.text("definitionHeading"),
            "Symbol \"TEST Valve\" - not in the library"
        );
        assert!(!bench.enabled("definitionDelete"));

        // The library has no undo; the form is the way back.
        assert!(bench.enabled("definitionSave"));
        bench.press("definitionSave");
        assert!(in_library(&bench, "TEST Valve") == valve());
        assert!(
            bench
                .logged_exactly("Symbol \"TEST Valve\" added to the library: 6 strokes, 0 texts.")
        );
    }

    #[test]
    fn the_verb_itself_refuses_a_definition_in_use_and_delete_anyway_removes_it() {
        // The editor's line run by CUSTOMISE REMOVE itself, with no stand-in between them. By
        // hand from the head of this file: "TEST Valve" is named by rule #1 (AC*, a symbol rule)
        // and by the style Marks, and "TEST Spare" by nothing.
        //
        // One nothing names goes by the plain line: the verb answers with its record of what it
        // removed, and the editor says the definition is gone.
        let mut spare_editor = editor(interpreter_host(fixture_document()));
        assert!(edit(&mut spare_editor, "TEST Spare"));
        spare_editor.press("definitionDelete");
        assert_eq!(
            spare_editor.lines(),
            strings(&["CUSTOMISE REMOVE \"TEST Spare\""])
        );
        assert!(!has_definition(&spare_editor, "TEST Spare"));
        assert!(
            issues(&spare_editor).starts_with("\"TEST Spare\" was deleted from the library."),
            "{}",
            issues(&spare_editor)
        );
        assert!(withdrawn(&spare_editor, "definitionDeleteAnyway"));
        let mut host = spare_editor.host;
        host.take_lines();

        let mut bench = editor(host);
        assert!(edit(&mut bench, "TEST Valve"));
        bench.press("definitionDelete");
        // Refused. The verb's own words come first, as the window logs a refused line, and they
        // cite who names the definition - the verb asks the function the editor lists with
        // (cad's definition_users), and words each user its own way. They are said ONCE: the
        // editor does not put its own list of the same two under them. Then what Delete Anyway
        // would do: a point is all that draws the valve, so a stand-in mark is all that follows.
        assert_eq!(bench.lines(), strings(&[REMOVE_VALVE]));
        let refusal = "\"TEST Valve\" was not deleted: InvalidState: CUSTOMISE REMOVE: 1 of the 1 \
                       definition named is in use, so nothing was removed; FORCE removes what is \
                       used too, and what names it then draws plain\n  \"TEST Valve\": rule #1 AC* \
                       (symbol) names it\n  \"TEST Valve\": the drawing's style \"Marks\" names \
                       it\n";
        let offer = "Delete Anyway runs the line again with FORCE. A point naming it is then \
                     drawn as a stand-in mark.";
        assert_eq!(issues(&bench), format!("{refusal}{offer}"));
        assert!(!issues(&bench).contains("It is named by:"));

        // Something else comes to name the valve while that refusal is on the page: a second
        // style. The verb's words above are of the drawing as it was and do not cite it, so the
        // editor's own list - of all three, as they stand now - goes under them: FORCE is never
        // offered over a user nothing on the page names.
        assert!(
            bench
                .document_mut()
                .execute(create_style(Style {
                    name: String::from("More"),
                    symbol: String::from("TEST Valve"),
                    ..Style::default()
                }))
                .is_ok()
        );
        bench.draw();
        assert!(offered(&bench, "definitionDeleteAnyway"));
        assert_eq!(
            issues(&bench),
            format!(
                "{refusal}It is named by:\n  rule #1 AC* (symbol) draws it as its symbol\n  style \
                 \"Marks\" draws it as its symbol\n  style \"More\" draws it as its symbol\n{offer}"
            )
        );
        assert!(bench.document_mut().undo().is_ok());
        bench.draw();
        assert_eq!(
            issues(&bench),
            format!("{refusal}{offer}"),
            "and goes again with what it listed"
        );
        assert!(bench.dialog.says_an_error());
        assert!(in_library(&bench, "TEST Valve") == valve());
        assert!(offered(&bench, "definitionDeleteAnyway"));

        bench.press("definitionDeleteAnyway");
        assert_eq!(bench.lines(), strings(&[REMOVE_VALVE, REMOVE_VALVE_FORCE]));
        assert!(!has_definition(&bench, "TEST Valve"));
        // The kerb is all that is left of the three, and the two rules still stand: the verb
        // removes a definition and nothing that names one.
        assert_eq!(
            bench.document().style_library().names(),
            strings(&["TEST Kerb"])
        );
        assert_eq!(bench.document().survey_map().size(), 2);
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert!(
            issues(&bench).starts_with(
                "\"TEST Valve\" was deleted from the library. It is still shown here: Save puts \
                 it back."
            ),
            "{}",
            issues(&bench)
        );
    }

    #[test]
    fn delete_anyway_is_not_carried_out_unless_its_refusal_is_on_the_page() {
        // FORCE deletes what something uses, and the library has no undo. It is offered under a
        // refusal that lists what; asked for with none showing - a script pressing the button, a
        // caller - nothing is run.
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Valve"));

        // The button ITSELF is hidden and disabled, not only the row it is in: the headless driver
        // asks the button, and a press of it is refused.
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert!(bench.try_press("definitionDeleteAnyway").is_err());
        assert!(bench.lines().is_empty());
        // Nor does the call behind it run a line.
        assert!(!bench.dialog.remove(&mut bench.host, true));
        bench.draw();
        assert!(bench.lines().is_empty());
        assert!(in_library(&bench, "TEST Valve") == valve());
        assert!(
            issues(&bench).starts_with("Delete Anyway is offered once Delete has been refused"),
            "{}",
            issues(&bench)
        );
        assert!(
            withdrawn(&bench, "definitionDeleteAnyway"),
            "saying so offers nothing"
        );

        // An edit of the form withdraws an offer that was up: the refusal was of the page as it
        // then stood.
        bench.press("definitionDelete");
        assert!(offered(&bench, "definitionDeleteAnyway"));
        bench.fill("definitionGroup", "Test/Other");
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert!(!bench.dialog.remove(&mut bench.host, true));
        assert_eq!(bench.lines(), strings(&[REMOVE_VALVE]));
        assert!(in_library(&bench, "TEST Valve") == valve());
    }

    #[test]
    fn a_line_that_fails_for_another_reason_is_shown_as_it_failed_and_a_failed_force_offers_nothing()
     {
        // The line fails, and not because the definition is used - here as the window fails it
        // while the verb is not there: REMOVE read as a file's name. The editor shows what it was
        // told, and does not say "is used".
        let reply = "NotFound: cannot open this file [REMOVE]";
        let host = RecordingHost::with_document(fixture_document())
            .answering(Answer::Fixed(VerbOutcome::new(false, "", reply)));
        let mut bench = editor(host);
        assert!(edit(&mut bench, "TEST Valve"));

        // Something names it, so FORCE is offered - under the line's own words.
        bench.press("definitionDelete");
        assert_eq!(
            issues(&bench),
            "\"TEST Valve\" was not deleted: NotFound: cannot open this file [REMOVE]\n\
             It is named by:\n  rule #1 AC* (symbol) draws it as its symbol\n  style \"Marks\" \
             draws it as its symbol\n\
             Delete Anyway runs the line again with FORCE. A point naming it is then drawn as a \
             stand-in mark."
        );
        assert!(!issues(&bench).contains("is used"));
        assert!(offered(&bench, "definitionDeleteAnyway"));

        // The FORCE line fails as well: said, and nothing further is offered.
        bench.press("definitionDeleteAnyway");
        assert_eq!(bench.lines(), strings(&[REMOVE_VALVE, REMOVE_VALVE_FORCE]));
        assert_eq!(
            issues(&bench),
            "\"TEST Valve\" was not deleted: NotFound: cannot open this file [REMOVE]\n\
             Named by:\n  rule #1 AC* (symbol) draws it as its symbol\n  style \"Marks\" draws it \
             as its symbol"
        );
        assert!(bench.dialog.says_an_error());
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert!(withdrawn(&bench, "definitionDeleteCancel"));
        assert!(!bench.dialog.delete_anyway_offered(bench.host.document()));
        assert!(in_library(&bench, "TEST Valve") == valve());

        // Of a definition nothing names there is no FORCE to offer at all.
        assert!(edit(&mut bench, "TEST Spare"));
        bench.press("definitionDelete");
        assert_eq!(
            issues(&bench),
            "\"TEST Spare\" was not deleted: NotFound: cannot open this file [REMOVE]\n\
             No survey code, style or layer names it."
        );
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));
        assert!(in_library(&bench, "TEST Spare") == spare());

        // A line refused with nothing said at all is still said to be refused.
        bench.host.answer = Answer::Fixed(VerbOutcome::default());
        let mut mute =
            DefinitionEditorDialog::new(bench.host.document(), CodesContext::default());
        assert!(mute.edit_definition(&mut bench.host, "TEST Spare"));
        assert!(!mute.remove(&mut bench.host, false));
        assert!(
            mute.issues()
                .starts_with("\"TEST Spare\" was not deleted: the command was refused."),
            "{}",
            mute.issues()
        );
    }

    #[test]
    fn a_refused_delete_lists_what_names_the_definition_whatever_the_form_reads_as() {
        // Delete acts on the library's definition, so it can be pressed over a form that does not
        // read; the offer of FORCE then still has its list above it, of the definition the form
        // is ON.
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Valve"));
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"drow\", 1, 0]");
        assert!(bench.dialog.definition().is_err());

        bench.press("definitionDelete");
        assert!(offered(&bench, "definitionDeleteAnyway"));
        let said = issues(&bench);
        assert!(
            said.starts_with(
                "\"TEST Valve\" was not deleted: the definition is used\n\
                 It is named by:\n  rule #1 AC* (symbol) draws it as its symbol\n  style \
                 \"Marks\" draws it as its symbol\n\
                 Delete Anyway runs the line again with FORCE. A point naming it is then drawn as \
                 a stand-in mark.\n\
                 Line 2: definition \"TEST Valve\" strokes[1]: \"drow\" is not a kind of stroke"
            ),
            "{said}"
        );
        assert_eq!(bench.dialog.washed_lines(), vec![2]);
    }

    #[test]
    fn delete_of_a_form_that_is_not_in_the_library_runs_nothing() {
        let mut bench = editor(fixture_host());
        // A new definition: there is nothing of it to delete yet.
        assert!(new_definition(&mut bench, true));
        bench.fill("definitionName", "TEST Post");
        assert!(!bench.enabled("definitionDelete"));
        assert!(!bench.dialog.remove(&mut bench.host, false));
        assert!(bench.lines().is_empty());
        assert!(
            issues(&bench)
                .starts_with("Delete: the form holds no definition that is in the library."),
            "{}",
            issues(&bench)
        );
        assert!(bench.dialog.says_an_error());
        assert_eq!(bench.document().style_library().size(), 3);
    }

    #[test]
    fn a_definition_nothing_names_is_deleted_by_the_one_line() {
        let mut bench = editor(fixture_host());
        // What the symbol library's Delete asks: shown, then deleted.
        assert!(bench.dialog.delete_definition(&mut bench.host, "TEST Spare"));
        bench.draw();
        assert_eq!(
            bench.lines(),
            strings(&["CUSTOMISE REMOVE \"TEST Spare\""])
        );
        assert!(!has_definition(&bench, "TEST Spare"));
        assert_eq!(bench.dialog.edited_name(), "TEST Spare");
        assert!(withdrawn(&bench, "definitionDeleteAnyway"));

        // A name of one word is quoted on the line all the same, as the line is documented: it
        // reads the same either way.
        let lone = LineStyle {
            name: String::from("Lone"),
            ..spare()
        };
        add_to_library(bench.document_mut(), &lone);
        bench.host.take_lines();
        let mut other =
            DefinitionEditorDialog::new(bench.host.document(), CodesContext::default());
        assert!(other.delete_definition(&mut bench.host, "Lone"));
        assert_eq!(bench.lines(), strings(&["CUSTOMISE REMOVE \"Lone\""]));
        assert!(!has_definition(&bench, "Lone"));

        // A refusal that is not about users - here, no command line at all - is said, and offers
        // no Delete Anyway: FORCE would not answer it.
        let mut bare = editor_with(
            fixture_host(),
            CodesContext::default().without_command_line(),
        );
        assert!(!bare.dialog.delete_definition(&mut bare.host, "TEST Spare"));
        bare.draw();
        assert!(
            issues(&bare).starts_with("Nothing here can run CUSTOMISE REMOVE"),
            "{}",
            issues(&bare)
        );
        assert!(in_library(&bare, "TEST Spare") == spare());
        assert!(withdrawn(&bare, "definitionDeleteAnyway"));
    }

    #[test]
    fn a_name_the_command_line_cannot_carry_is_not_put_on_a_line() {
        // A word of a command line has no escape for a double quote: the line would run on
        // another name, cut short at the quote. And its quotes do not survive the tokenizer, so a
        // definition named CODE is the verb's own word: `CUSTOMISE REMOVE "CODE" FORCE` would
        // remove the survey code FORCE. Such names come out of a file; they are opened and
        // changed, and Delete says why it cannot name them.
        let quoted = LineStyle {
            name: String::from("TEST 6\" Pipe"),
            ..spare()
        };
        let keyword = LineStyle {
            name: String::from("CODE"),
            ..spare()
        };
        let mut host = fixture_host();
        add_to_library(host.document_mut(), &quoted);
        add_to_library(host.document_mut(), &keyword);

        let mut bench = editor(host);
        assert!(edit(&mut bench, &quoted.name));
        assert!(!bench.dialog.remove(&mut bench.host, false));
        bench.draw();
        assert!(bench.lines().is_empty());
        assert_eq!(
            issues(&bench).split('\n').next(),
            Some(
                "Delete: no command line can name \"TEST 6\" Pipe\" - a double quote cannot be \
                 written on a command line."
            )
        );
        assert!(in_library(&bench, &quoted.name) == quoted);
        // It is edited like any other: its name is its own.
        bench.fill("definitionGroup", "Test/Pipes");
        assert!(bench.enabled("definitionSave"), "{}", issues(&bench));
        bench.press("definitionSave");
        let regrouped = LineStyle {
            group: String::from("Test/Pipes"),
            ..quoted.clone()
        };
        assert!(in_library(&bench, &quoted.name) == regrouped);

        assert!(edit(&mut bench, "CODE"));
        assert!(!bench.dialog.remove(&mut bench.host, false));
        assert!(bench.lines().is_empty());
        assert_eq!(
            issues(&bench).split('\n').next(),
            Some(
                "Delete: no command line can name \"CODE\" - the name is a word of the CUSTOMISE \
                 REMOVE line itself (CODE), which a command line cannot tell from the definition."
            )
        );
        assert!(in_library(&bench, "CODE") == keyword);
    }

    #[test]
    fn a_definition_is_not_made_under_a_name_no_line_could_remove_it_by() {
        // Saved, it could never be deleted again - not here, and not by the line an agent types.
        let mut bench = editor(fixture_host());
        assert!(new_definition(&mut bench, true));
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"draw\", 0, 2]");

        bench.fill("definitionName", "6\" Valve");
        assert!(!bench.enabled("definitionSave"));
        assert_eq!(
            issues(&bench),
            "A new definition cannot be named \"6\" Valve\": a double quote cannot be written on \
             a command line."
        );
        assert!(!bench.dialog.save(&mut bench.host));
        assert!(bench.dialog.definition().is_err());
        for word in ["Force", "code"] {
            bench.fill("definitionName", word);
            assert!(!bench.enabled("definitionSave"), "{word}");
            assert!(
                issues(&bench).starts_with(&format!(
                    "A new definition cannot be named \"{word}\": the name is a word of the \
                     CUSTOMISE REMOVE line itself"
                )),
                "{}",
                issues(&bench)
            );
            assert!(!bench.dialog.save(&mut bench.host));
        }
        assert_eq!(bench.document().style_library().size(), 3);

        // A name that only begins with such a word is a name like any other.
        bench.fill("definitionName", "Force main");
        assert!(bench.enabled("definitionSave"), "{}", issues(&bench));
        bench.press("definitionSave");
        assert_eq!(bench.document().style_library().size(), 4);
        assert!(has_definition(&bench, "Force main"));
    }

    #[test]
    fn the_commit_hook_is_called_before_a_save_and_what_it_hands_back_after_it() {
        // The hook is how a session that was kept stays kept: its maker runs CUSTOMISE KEEP in
        // what it hands back, and hands nothing back when the session was not kept. The editor
        // knows neither; it calls, and counts on the order.
        #[derive(Default)]
        struct Seen {
            kept: Cell<bool>,
            begun: Cell<i32>,
            finished: Cell<i32>,
            generation_at_begin: Cell<u64>,
            generation_at_finish: Cell<u64>,
        }
        let seen = Rc::new(Seen {
            kept: Cell::new(true),
            ..Seen::default()
        });
        let begin: BeginCommit = {
            let seen = Rc::clone(&seen);
            Rc::new(move |host: &mut dyn Host| -> Option<Committed> {
                seen.begun.set(seen.begun.get() + 1);
                seen.generation_at_begin
                    .set(host.document().library_generation());
                if !seen.kept.get() {
                    return None;
                }
                let seen = Rc::clone(&seen);
                Some(Box::new(move |host: &mut dyn Host| {
                    seen.finished.set(seen.finished.get() + 1);
                    seen.generation_at_finish
                        .set(host.document().library_generation());
                    host.run("CUSTOMISE KEEP");
                }))
            })
        };
        // KEEP is not this runner's to carry out; it is only recorded.
        let host = RecordingHost::with_document(fixture_document()).answering(Answer::Custom(
            Box::new(|parts: &mut HostParts, line: &str| {
                if line == "CUSTOMISE KEEP" {
                    VerbOutcome::new(true, "", "")
                } else {
                    remove_as_the_verb_would(&mut parts.document, line)
                }
            }),
        ));
        let mut bench = editor_with(
            host,
            CodesContext {
                begin_commit: Some(begin),
                ..CodesContext::default()
            },
        );
        assert!(edit(&mut bench, "TEST Kerb"));

        // A Save that is refused commits nothing, and the hook is not troubled.
        bench.fill("definitionLength", "-1");
        assert!(!bench.dialog.save(&mut bench.host));
        assert_eq!(seen.begun.get(), 0);

        let before = bench.document().library_generation();
        bench.fill("definitionLength", "20");
        bench.press("definitionSave");
        assert_eq!(seen.begun.get(), 1);
        assert_eq!(seen.finished.get(), 1);
        assert_eq!(
            seen.generation_at_begin.get(),
            before,
            "asked before the library was touched"
        );
        assert_eq!(
            seen.generation_at_finish.get(),
            before + 1,
            "answered once it had been"
        );
        assert_eq!(bench.lines(), strings(&["CUSTOMISE KEEP"]));

        // Not kept: asked all the same, and nothing is run.
        seen.kept.set(false);
        bench.fill("definitionLength", "21");
        bench.press("definitionSave");
        assert_eq!(seen.begun.get(), 2);
        assert_eq!(seen.finished.get(), 1);
        assert_eq!(bench.lines().len(), 1);
        assert_eq!(in_library(&bench, "TEST Kerb").length, 21.0);

        // A Delete is the editor's commit too: after the line that removed the definition, and
        // not after the one that was refused.
        seen.kept.set(true);
        bench.host.take_lines();
        assert!(edit(&mut bench, "TEST Valve"));
        bench.press("definitionDelete");
        assert_eq!(seen.begun.get(), 3);
        assert_eq!(seen.finished.get(), 1, "refused: nothing was committed");
        bench.press("definitionDeleteAnyway");
        assert_eq!(seen.begun.get(), 4);
        assert_eq!(seen.finished.get(), 2);
        assert_eq!(
            bench.lines(),
            strings(&[REMOVE_VALVE, REMOVE_VALVE_FORCE, "CUSTOMISE KEEP"])
        );
    }

    #[test]
    fn the_units_are_chosen_by_the_words_a_file_holds() {
        // docs/customisation.md, "The members": `units` is `world`, `paper` or `twoPoint`, and
        // `world` is what a file that says nothing means. The box offers those three words, and
        // what it gives a definition is what the format's own writer then writes.
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));
        assert_eq!(
            bench.items("definitionUnits"),
            strings(&["world", "paper", "twoPoint"])
        );

        let written = |bench: &Editor| -> String {
            let read = bench.dialog.definition().expect("the form reads");
            definition_to_json(&read, "").expect("the definition is written")
        };
        bench.fill("definitionUnits", "world");
        assert!(!written(&bench).contains("\"units\""), "{}", written(&bench));
        assert!(!bench.enabled("definitionAnchor1X"));
        bench.fill("definitionUnits", "paper");
        assert!(
            written(&bench).contains("\"units\": \"paper\""),
            "{}",
            written(&bench)
        );
        assert!(!bench.enabled("definitionStretchMode"));
        // Two-point: the anchors and the two modes can be typed.
        bench.fill("definitionUnits", "twoPoint");
        assert!(
            written(&bench).contains("\"units\": \"twoPoint\""),
            "{}",
            written(&bench)
        );
        for name in [
            "definitionAnchor1X",
            "definitionAnchor1Y",
            "definitionAnchor2X",
            "definitionAnchor2Y",
            "definitionStretchMode",
            "definitionCycleMode",
        ] {
            assert!(bench.enabled(name), "{name}");
        }
        bench.fill("definitionAnchor2X", "8");
        bench.fill("definitionStretchMode", "1");
        bench.press("definitionSave");
        let mut expected = LineStyle {
            units: StyleUnits::TwoPoint,
            anchor2: Point2::new(8.0, 0.0),
            stretch_mode: 1,
            ..kerb()
        };
        assert!(in_library(&bench, "TEST Kerb") == expected);

        // The kind is which of a file's two lists holds it.
        bench.fill("definitionKind", "Symbol");
        bench.press("definitionSave");
        expected.symbol = true;
        assert!(in_library(&bench, "TEST Kerb") == expected);
    }

    #[test]
    fn another_definition_is_not_opened_over_edits_that_are_not_saved() {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));
        bench.fill("definitionLength", "15");

        for what in [
            DefinitionEdit::NewSymbol,
            DefinitionEdit::NewLinestyle,
            DefinitionEdit::Edit,
            DefinitionEdit::Duplicate,
            DefinitionEdit::Delete,
        ] {
            assert!(!bench.dialog.request(&mut bench.host, what, "TEST Valve"));
        }
        bench.draw();
        assert_eq!(bench.dialog.edited_name(), "TEST Kerb");
        assert_eq!(bench.value("definitionLength"), "15");
        assert!(bench.lines().is_empty(), "and nothing was deleted");
        assert!(bench.logged_exactly(
            "Edit \"TEST Valve\": the definition editor holds edits to \"TEST Kerb\" that are not \
             saved. Save or Revert them first."
        ));
        // The last of the five asked was the Delete, and the form says so.
        assert!(
            issues(&bench).starts_with("Delete \"TEST Valve\": "),
            "{}",
            issues(&bench)
        );

        // Asked for again, the definition it is on is simply still there.
        assert!(edit(&mut bench, "TEST Kerb"));
        assert_eq!(bench.value("definitionLength"), "15");

        bench.press("definitionRevert");
        assert!(edit(&mut bench, "TEST Valve"));
        assert_eq!(bench.dialog.edited_name(), "TEST Valve");
        // A name the library does not define opens nothing.
        assert!(!edit(&mut bench, "TEST Nowhere"));
        assert!(!duplicate(&mut bench, "TEST Nowhere"));
        assert_eq!(bench.dialog.edited_name(), "TEST Valve");
    }

    #[test]
    fn a_definition_changed_elsewhere_is_taken_up_unedited_and_said_over_edits() {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));
        let elsewhere = |bench: &mut Editor, length: f64| {
            let mut library = bench.document().style_library().clone();
            let changed = LineStyle { length, ..kerb() };
            library.update(&changed).expect("the kerb is changed");
            bench.document_mut().set_style_library(library);
            bench.draw();
        };

        // Nothing typed: the form shows what the library has now.
        elsewhere(&mut bench, 20.0);
        assert_eq!(bench.value("definitionLength"), "20");
        assert!(!bench.dialog.dirty());

        // Something typed: it is kept, the change is said, and Revert shows the library's
        // definition as it is now - not as it was when the form opened.
        bench.fill("definitionGroup", "Test/Mine");
        elsewhere(&mut bench, 30.0);
        assert_eq!(bench.value("definitionGroup"), "Test/Mine");
        assert_eq!(bench.value("definitionLength"), "20");
        assert!(bench.dialog.dirty());
        assert!(
            issues(&bench)
                .starts_with("\"TEST Kerb\" was changed elsewhere since it was opened here."),
            "{}",
            issues(&bench)
        );
        bench.press("definitionRevert");
        assert_eq!(bench.value("definitionGroup"), "Test/Lines");
        assert_eq!(bench.value("definitionLength"), "30");
        assert!(!bench.dialog.dirty());

        // Removed elsewhere: said, the form kept, and Save puts it back.
        bench.fill("definitionLength", "31");
        let mut without = bench.document().style_library().clone();
        without.remove("TEST Kerb").expect("the kerb is removed");
        bench.document_mut().set_style_library(without);
        bench.draw();
        assert!(
            issues(&bench).starts_with("\"TEST Kerb\" was removed from the library elsewhere."),
            "{}",
            issues(&bench)
        );
        assert_eq!(bench.value("definitionLength"), "31");
        assert!(bench.enabled("definitionSave"));
        bench.press("definitionSave");
        let expected = LineStyle {
            length: 31.0,
            ..kerb()
        };
        assert!(in_library(&bench, "TEST Kerb") == expected);
    }

    #[test]
    fn no_button_is_a_default_and_enter_in_a_field_presses_nothing() {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));
        bench.fill("definitionLength", "15");
        // With an edit waiting, Enter in a field would Save it - or, the first button being New,
        // try to throw it away - were any button a default. Each field is clicked, so it has the
        // keys, and Enter pressed in it.
        for name in [
            "definitionGroup",
            "definitionLength",
            "definitionFactor",
            "definitionOriginX",
        ] {
            let at = bench.control(name).rect.center();
            for pressed in [true, false] {
                bench.harness.pointer_button(
                    at,
                    egui::PointerButton::Primary,
                    pressed,
                    egui::Modifiers::NONE,
                );
            }
            bench.draw();
            bench
                .harness
                .key(egui::Key::Enter, egui::Modifiers::NONE);
            bench.draw();
        }
        assert!(
            bench.host.logged().is_empty(),
            "{:?}",
            bench.host.logged().first()
        );
        assert!(bench.dialog.is_open());
        assert!(bench.dialog.dirty());
        assert!(in_library(&bench, "TEST Kerb") == kerb());
    }

    #[test]
    fn closing_it_keeps_what_was_typed_and_says_so() {
        let mut bench = editor(fixture_host());
        assert!(edit(&mut bench, "TEST Kerb"));
        bench.fill("definitionLength", "15");
        bench.press("definitionClose");
        assert!(!bench.dialog.is_open());
        assert!(bench.dialog.dirty());
        // Shown again, what was typed is there.
        bench.dialog.show();
        bench.draw();
        assert_eq!(bench.value("definitionLength"), "15");
        assert!(bench.logged_exactly(
            "The definition editor was closed with edits that are not saved; they are kept in it."
        ));
    }

    #[test]
    fn the_dialog_outlives_its_document_and_does_nothing_after_it() {
        let mut bench = editor_with(
            fixture_host(),
            CodesContext::default().without_command_line(),
        );
        assert!(edit(&mut bench, "TEST Kerb"));
        bench.fill("definitionLength", "15");

        bench.close_drawing();

        assert!(!bench.dialog.save(&mut bench.host));
        assert!(!bench.dialog.remove(&mut bench.host, false));
        assert!(!bench.dialog.remove(&mut bench.host, true));
        assert!(!bench.dialog.new_definition(&mut bench.host, true));
        assert!(!bench.dialog.edit_definition(&mut bench.host, "TEST Valve"));
        assert!(
            !bench
                .dialog
                .duplicate_definition(&mut bench.host, "TEST Kerb")
        );
        bench.dialog.revert(&mut bench.host);
        // Typing still only reads the form.
        assert!(
            bench
                .dialog
                .type_into(&mut bench.host, "definitionGroup", "Test/After")
        );
        assert!(
            bench
                .dialog
                .type_into(&mut bench.host, "definitionStrokes", "[\"move\", 0, 0]")
        );
        bench.draw();
        assert!(
            issues(&bench).starts_with("The drawing this editor worked on has closed"),
            "{}",
            issues(&bench)
        );
        for name in [
            "definitionSave",
            "definitionRevert",
            "definitionNew",
            "definitionDuplicate",
            "definitionDelete",
            "definitionStrokes",
            "definitionName",
        ] {
            assert!(!bench.enabled(name), "{name}");
        }
        assert!(bench.enabled("definitionClose"));
    }

    // ---- what the strokes box can and cannot hold -----------------------------------------------

    #[test]
    fn a_text_holding_a_no_break_space_opens_unedited_and_is_saved_character_for_character() {
        // A no-break space (U+00A0) and a line separator (U+2028) in a stroke's text are
        // characters of it. A text box asked for its "plain text" hands the first back as a blank
        // and the second as a line break; the form would then differ from what it was opened
        // with before anything was typed - "edited, not saved" for ever, every other definition
        // refused - and Save would write the blank.
        let words = "A\u{a0}B\u{2028}C";
        let label = LineStyle {
            name: String::from("TEST Label"),
            texts: vec![StrokeText {
                text: String::from(words),
                height: 1.0,
                ..StrokeText::default()
            }],
            strokes: vec![mv(0.0, 0.0), text_stroke(0)],
            ..spare()
        };
        let mut host = fixture_host();
        add_to_library(host.document_mut(), &label);

        let mut bench = editor(host);
        assert!(edit(&mut bench, "TEST Label"), "{}", issues(&bench));
        assert!(!bench.dialog.dirty());
        assert_eq!(bench.text("definitionHeading"), "Symbol \"TEST Label\"");
        assert!(!bench.enabled("definitionSave"));
        assert!(!bench.enabled("definitionRevert"));
        let read = bench.dialog.definition().expect("the form reads");
        assert!(read == label);

        // Nothing is unsaved, so another definition opens, and this one again.
        assert!(edit(&mut bench, "TEST Kerb"));
        assert!(edit(&mut bench, "TEST Label"));
        assert!(!bench.dialog.dirty());

        // A change elsewhere in the form saves the text as it was.
        bench.fill("definitionGroup", "Test/Labels");
        bench.press("definitionSave");
        let expected = LineStyle {
            group: String::from("Test/Labels"),
            ..label
        };
        assert!(in_library(&bench, "TEST Label") == expected);
        assert_eq!(in_library(&bench, "TEST Label").texts[0].text, words);

        // Typed into a new definition, the two characters are kept the same way.
        assert!(new_definition(&mut bench, true));
        bench.fill("definitionName", "TEST Typed");
        strokes(
            &mut bench,
            "[\"move\", 0, 0],\n[\"text\", {\"text\": \"A\u{a0}B\u{2028}C\", \"height\": 1}]",
        );
        assert!(bench.enabled("definitionSave"), "{}", issues(&bench));
        bench.press("definitionSave");
        assert_eq!(in_library(&bench, "TEST Typed").texts[0].text, words);
        assert!(!bench.dialog.dirty());
    }

    #[test]
    fn a_definition_the_form_cannot_show_is_not_opened_or_copied_and_is_said_when_it_arrives() {
        // Two ways a library can hold what the form cannot show: a stroke the format cannot write
        // - a move that carries a radius - and a text holding a paragraph separator (U+2029), at
        // which the strokes box breaks a line, so the stroke could not be kept on one. Neither is
        // opened as something it is not.
        let stray_strokes = vec![Stroke {
            op: StrokeOp::Move,
            point: Point2::new(0.0, 0.0),
            radius: 2.0,
            ..Stroke::default()
        }];
        let stray = LineStyle {
            name: String::from("TEST Stray"),
            strokes: stray_strokes.clone(),
            ..spare()
        };
        let broken = LineStyle {
            name: String::from("TEST Broken"),
            texts: vec![StrokeText {
                text: String::from("A\u{2029}B"),
                height: 1.0,
                ..StrokeText::default()
            }],
            strokes: vec![mv(0.0, 0.0), text_stroke(0)],
            ..spare()
        };
        let mut host = fixture_host();
        add_to_library(host.document_mut(), &stray);
        add_to_library(host.document_mut(), &broken);

        let mut bench = editor(host);
        assert!(edit(&mut bench, "TEST Kerb"));

        assert!(!edit(&mut bench, "TEST Stray"));
        assert!(
            issues(&bench).starts_with(
                "\"TEST Stray\" cannot be edited here, because the customisation format cannot \
                 write it: "
            ),
            "{}",
            issues(&bench)
        );
        assert!(bench.dialog.says_an_error());
        assert!(!duplicate(&mut bench, "TEST Stray"));
        assert!(
            issues(&bench).starts_with(
                "\"TEST Stray\" cannot be copied here, because the customisation format cannot \
                 write it: "
            ),
            "{}",
            issues(&bench)
        );
        // Delete is asked through Edit, and so is refused the same way: nothing ran.
        assert!(!bench.dialog.delete_definition(&mut bench.host, "TEST Stray"));
        assert!(bench.lines().is_empty());

        assert!(!edit(&mut bench, "TEST Broken"));
        assert_eq!(
            issues(&bench).split('\n').next(),
            Some(
                "\"TEST Broken\" cannot be edited here, because one of its texts holds a \
                 paragraph separator, which the strokes box breaks a line at: its strokes could \
                 not be shown one a line"
            )
        );
        assert!(!duplicate(&mut bench, "TEST Broken"));

        // Each refusal is in the log as an error, and the form is still on what it was on.
        assert!(bench.host.logged().len() >= 4);
        for (message, is_error) in bench.host.logged() {
            assert!(is_error, "{message}");
        }
        assert_eq!(bench.dialog.edited_name(), "TEST Kerb");
        assert_eq!(
            bench.value("definitionStrokes"),
            "[\"move\", 0, 0],\n[\"draw\", 8, 0]"
        );

        // The definition a form is on changed elsewhere into such a thing: said, and what the
        // form shows is kept. (A second editor, opened on it and told nothing yet: the first
        // still shows its last refusal first, as it does until its form is next edited.)
        let mut second =
            DefinitionEditorDialog::new(bench.host.document(), CodesContext::default());
        assert!(second.edit_definition(&mut bench.host, "TEST Kerb"));
        let kerb_stray = LineStyle {
            strokes: stray_strokes,
            ..kerb()
        };
        let mut changed = bench.document().style_library().clone();
        changed.update(&kerb_stray).expect("the kerb is changed");
        bench.document_mut().set_style_library(changed);
        bench.draw();
        // The second editor's own frames, in a window of its own.
        let mut other = Harness::new();
        {
            let host = &mut bench.host;
            other
                .run(|ui| {
                    let ctx = ui.ctx().clone();
                    second.ui(&ctx, host);
                })
                .expect("the second editor is drawn");
        }
        assert!(
            second.issues().starts_with(
                "\"TEST Kerb\" was changed elsewhere into something that cannot be shown here, \
                 because the customisation format cannot write it: "
            ),
            "{}",
            second.issues()
        );
        assert!(second.issues().contains("What is typed here is kept."));
        assert_eq!(
            other
                .control("definitionStrokes")
                .map(|control| control.value),
            Some(String::from("[\"move\", 0, 0],\n[\"draw\", 8, 0]"))
        );
        // The first editor is told the same, under what it last said.
        assert!(
            issues(&bench).contains(
                "\"TEST Kerb\" was changed elsewhere into something that cannot be shown here"
            ),
            "{}",
            issues(&bench)
        );
        assert_eq!(
            bench.value("definitionStrokes"),
            "[\"move\", 0, 0],\n[\"draw\", 8, 0]"
        );
        assert!(in_library(&bench, "TEST Kerb") == kerb_stray);
    }

    // ---- a name the drawing itself also answers to ----------------------------------------------

    #[test]
    fn a_name_the_drawing_also_answers_to_is_said_as_that_and_not_as_naming_nothing() {
        // The drawing has a linetype "Fence" of its own, and a layer names it: the layer's lines
        // are dashed by that linetype (decision D2, cad's style_resolver), not drawn plain. A
        // library linestyle made under the same name is drawn in its place once saved. That is
        // allowed - a name in both is reported, never an error - and is said before Save.
        let linetype = |name: &str| Linetype {
            name: String::from(name),
            pattern: vec![
                LinetypeElement { length: 2.0 },
                LinetypeElement { length: -1.0 },
            ],
            ..Linetype::default()
        };
        let mut host = fixture_host();
        assert!(
            host.document_mut()
                .execute(create_linetype(linetype("Fence")))
                .is_ok()
        );
        assert!(
            host.document_mut()
                .execute(create_layer(Layer {
                    name: String::from("fences"),
                    linetype: String::from("Fence"),
                    ..Layer::default()
                }))
                .is_ok()
        );

        let mut bench = editor(host);
        assert!(new_definition(&mut bench, false));
        bench.fill("definitionName", "Fence");
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"draw\", 2, 0]");
        assert_eq!(
            issues(&bench),
            "Named already:\n  layer \"fences\" names it as its linetype\n\
             The drawing has a linetype \"Fence\" of its own, which draws the lines that name it \
             now. Saved, this definition is drawn in its place, and Styles and Linetypes lists \
             the name under Diagnostics as one in both."
        );
        assert!(bench.enabled("definitionSave"));
        assert!(!bench.dialog.says_an_error());
        // At vertices it is not drawn along a line at all, and takes nothing over.
        bench.fill("definitionAtVertices", "on");
        assert!(
            issues(&bench).ends_with(
                "The drawing has a linetype \"Fence\" of its own, which goes on drawing the lines \
                 that name it: a definition at vertices is not drawn along a line."
            ),
            "{}",
            issues(&bench)
        );
        bench.press("definitionRevert");

        // The same of a built-in shape: a style draws its points as "circle".
        assert!(
            bench
                .document_mut()
                .execute(create_style(Style {
                    name: String::from("Rings"),
                    symbol: String::from("circle"),
                    ..Style::default()
                }))
                .is_ok()
        );
        assert!(new_definition(&mut bench, true));
        bench.fill("definitionName", "circle");
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"circle\", 1]");
        assert_eq!(
            issues(&bench),
            "Named already:\n  style \"Rings\" draws it as its symbol\n\
             \"circle\" is a built-in shape, which draws the points that name it now. Saved, this \
             definition is drawn in its place."
        );
        bench.press("definitionRevert");

        // A name nothing else answers to: until it is saved, what names it IS drawn plain or as a
        // stand-in, and that is what is said.
        let mut map = bench.document().survey_map().clone();
        map.add(SurveyRule {
            key: String::from("PX*"),
            linestyle: String::from("TEST Pipe"),
            ..SurveyRule::default()
        })
        .expect("the rule is added");
        bench.document_mut().set_survey_map(map);
        assert!(new_definition(&mut bench, false));
        bench.fill("definitionName", "TEST Pipe");
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"draw\", 2, 0]");
        assert_eq!(
            issues(&bench),
            "Named already:\n  rule #2 PX* (feature) names it as its linestyle\n\
             Until it is saved a line naming it is drawn plain."
        );
        bench.press("definitionRevert");

        // A copy does not begin under a name the drawing's linetypes hold either.
        assert!(
            bench
                .document_mut()
                .execute(create_linetype(linetype("TEST Kerb 2")))
                .is_ok()
        );
        assert!(duplicate(&mut bench, "TEST Kerb"));
        assert_eq!(bench.value("definitionName"), "TEST Kerb 3");
        bench.press("definitionRevert");

        // And a library linestyle the drawing also holds: deleted, what names it is handed to the
        // drawing's linetype, not left plain.
        assert!(
            bench
                .document_mut()
                .execute(create_linetype(linetype("TEST Kerb")))
                .is_ok()
        );
        assert!(edit(&mut bench, "TEST Kerb"));
        bench.press("definitionDelete");
        assert_eq!(
            issues(&bench),
            "\"TEST Kerb\" was not deleted: the definition is used\n\
             It is named by:\n  rule #0 WM* (feature) names it as its linestyle\n\
             Delete Anyway runs the line again with FORCE. A line naming it is then drawn with \
             the drawing's own linetype of that name."
        );
    }

    #[test]
    fn a_definition_with_no_strokes_is_saved_and_said_to_draw_nothing() {
        // The format holds a definition with no strokes, so the editor saves one; but it draws
        // nothing, which is rarely what was meant, and is said.
        let mut bench = editor(fixture_host());
        assert!(new_definition(&mut bench, false));
        bench.fill("definitionName", "TEST Gate");
        assert_eq!(
            issues(&bench),
            "No strokes yet: a definition is drawn by its strokes, and this one has none."
        );
        assert!(!bench.dialog.says_an_error());
        assert!(bench.enabled("definitionSave"));
        bench.press("definitionSave");
        let expected = LineStyle {
            name: String::from("TEST Gate"),
            ..LineStyle::default()
        };
        assert!(in_library(&bench, "TEST Gate") == expected);
        // Still said of it once it is in the library.
        assert!(
            issues(&bench).contains("No strokes yet"),
            "{}",
            issues(&bench)
        );
        strokes(&mut bench, "[\"move\", 0, 0],\n[\"draw\", 2, 0]");
        assert!(
            !issues(&bench).contains("No strokes yet"),
            "{}",
            issues(&bench)
        );
    }
}
