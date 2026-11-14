//! The twins of tests/qt_widgets/customisation/test_code_manager.cpp: the survey code manager,
//! driven as a person drives it - through its controls, found by object name - over the committed
//! hand-written customisation in tests/data/customisation, loaded into the Document as a session
//! customisation. What is asserted is the Document and the buffer.
//!
//! The fixture, counted by hand from test_survey.customisation.json: 11 rules over 8 keys, in file
//! order
//!
//! ```text
//!   #0 WM* feature    TEST SERVICES, blue, Line, TEST Water Main
//!   #1 KB* feature    TEST ROADS, "sui test purple" (no colour table knows it)
//!   #2 AC* feature    TEST FURNITURE, white, Point, "0"
//!   #3 TR* feature    TEST VEGETATION, green, Point, "0"
//!   #4 1*  feature    TEST TEXT, orange
//!   #5 2*  feature    TEST TEXT, red
//!   #6 PX* feature    TEST MISC, yellow, Point, "0"
//!   #7 AC* symbol     TEST Survey Mark (at vertices), 1.5
//!   #8 TR* symbol     TEST Tree (not at vertices, listed as a symbol)
//!   #9 PX* symbol     TEST Missing Symbol (defined nowhere)
//!   #10 *  attributes text Source = Katana test fixture
//! ```
//!
//! What the C++ read off a tree item is read off the dialog's own rows (code_tree,
//! explained_fields) or the control's record; an item's icon is the row's `pictured`. A colour
//! field's items carry no swatch here, so the one assertion that an item has an icon is not made.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use katana_cad::code_edit::code_list_csv;
use katana_cad::code_table::{ColourLookup, DefinitionLookup, LintKind, NearMissKind, explain_code};
use katana_cad::colour_lookup::colour_lookup;
use katana_cad::customisation_merge::LoadMode;
use katana_cad::{CustomisationOrigin, Document};
use katana_commands::create_entities;
use katana_core::ErrorCode;
use katana_entity::colour_names::standard_colour_names;
use katana_entity::customisation::customisation_from_json;
use katana_entity::entity::to_string;
use katana_entity::linework_codes::LineworkCodes;
use katana_entity::{
    Color, ColourTable, ColourTableEntry, CustomisationSourceNote, Entity, EntityId, EntityType,
    Geometry, PointGeometry, PropertyValue, SurveyMap, SurveyRule, SurveySection, SurveySymbol,
    SurveyTextStyle,
};
use katana_geometry::Point2;
use katana_window::customisation::code_manager::{
    CODE_TABLE_HEADERS, CodeTreeRow, SurveyCodeManagerDialog,
};
use katana_window::customisation::codes_seams::{BeginCommit, CodesContext, Committed};
use katana_window::host::{Host, RecordingHost};

use crate::fixture_customisation::{
    customisation_fixture_file, customisation_from_text, install_customisation_fixtures,
    install_customisations, install_noticed_session, quoted_file,
};
use crate::support::{Bench, thumbnails};
use crate::window_support::{Scratch, interpreter_host};

type Manager = Bench<SurveyCodeManagerDialog>;

/// The three fixtures in this order, so the session is named after the first: "test_linestyles".
fn fixture_host() -> RecordingHost {
    let mut document = Document::new(None);
    install_customisation_fixtures(
        &mut document,
        &["test_linestyles", "test_survey", "test_symbols"],
    );
    interpreter_host(document)
}

fn context() -> CodesContext {
    CodesContext {
        thumbnails: Some(thumbnails()),
        ..CodesContext::default()
    }
}

fn manager_with(host: RecordingHost, context: CodesContext) -> Manager {
    Bench::new(host, |document| {
        SurveyCodeManagerDialog::new(document, context)
    })
}

fn manager(host: RecordingHost) -> Manager {
    manager_with(host, context())
}

fn add_point(host: &mut RecordingHost, x: f64, code: &str, number: &str) -> EntityId {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(x, 0.0),
        }),
        ..Entity::default()
    };
    entity.properties.insert(
        String::from("code"),
        PropertyValue::Text(String::from(code)),
    );
    entity.properties.insert(
        String::from("point"),
        PropertyValue::Text(String::from(number)),
    );
    host.document_mut()
        .execute(create_entities(vec![entity]))
        .expect("the point is made");
    host.document()
        .last_created_entities()
        .first()
        .copied()
        .unwrap_or(0)
}

/// The Code Table's key rows: the children of its group headings.
fn keys_shown(bench: &Manager) -> BTreeSet<String> {
    bench
        .dialog
        .code_tree()
        .iter()
        .flat_map(|heading| heading.children.iter())
        .map(|key| key.cells[0].clone())
        .collect()
}

fn keys(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

/// The group heading that says `value`.
fn heading(tree: &[CodeTreeRow], value: &str) -> Option<CodeTreeRow> {
    tree.iter().find(|row| row.cells[0] == value).cloned()
}

/// The Code Table's row for `key`, under whichever group heading it sits.
fn key_row(bench: &Manager, key: &str) -> Option<CodeTreeRow> {
    bench
        .dialog
        .code_tree()
        .iter()
        .flat_map(|heading| heading.children.iter())
        .find(|row| row.cells[0] == key)
        .cloned()
}

/// Selects rule 0 (WM* feature) and saves it with another layer, through the form's own fields
/// and buttons.
fn edit_water_main_layer(bench: &mut Manager, layer: &str) {
    bench.dialog.select_rule(&mut bench.host, 0);
    bench.fill("ruleModel", layer);
    assert!(bench.enabled("ruleUpdate"));
    bench.press("ruleUpdate");
}

fn read_bytes(path: &Path) -> String {
    String::from_utf8(fs::read(path).expect("the file is read")).expect("the file is UTF-8")
}

fn layer_of(bench: &Manager, id: EntityId) -> String {
    bench
        .document()
        .model()
        .entities
        .find(id)
        .map(|entity| entity.layer.clone())
        .unwrap_or_default()
}

fn polylines(document: &Document) -> Vec<Entity> {
    let mut lines = Vec::new();
    document.model().entities.for_each(|entity| {
        if entity.r#type() == EntityType::Polyline {
            lines.push(entity.clone());
        }
    });
    lines
}

fn purple() -> ColourTable {
    let mut colours = ColourTable::new();
    colours
        .add(
            "sui test purple",
            Color {
                r: 128,
                g: 0,
                b: 128,
                a: 255,
            },
        )
        .expect("the colour is added");
    colours
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

fn source(name: &str, definitions: bool, rules: bool, notice: &[&str]) -> CustomisationSourceNote {
    CustomisationSourceNote {
        name: String::from(name),
        definitions,
        rules,
        notice: notice.iter().map(|line| String::from(*line)).collect(),
    }
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| String::from(*line)).collect()
}

/// A one-rule map taken from elsewhere.
fn elsewhere() -> SurveyMap {
    let mut replacement = SurveyMap::new();
    replacement
        .add(SurveyRule {
            key: String::from("QQ*"),
            model: String::from("ELSEWHERE"),
            ..SurveyRule::default()
        })
        .expect("the rule is added");
    replacement
}

/// A customisation written for these tests, holding one of everything a file can: a rule for a
/// key the fixture lacks, a symbol, a colour and the linework codes.
const EXTRA_CUSTOMISATION: &str = r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "extra",
  "colours": {
    "sui test purple": "#800080"
  },
  "linework": {"start": "BEG"},
  "symbols": [
    {"name": "TEST Extra Mark", "strokes": [
      ["circle", 1]
    ]}
  ],
  "codes": [
    {"key": "ZZ*", "sets": "feature", "layer": "TEST MISC"}
  ]
}
"##;

/// The file a session of four customisations, each with its author's notice, exports its codes to
/// (fixture_customisation, install_noticed_session) - written out by hand from
/// docs/customisation.md, "Layout". The session's own name, description and notice, and after it
/// the notice of the one source the file leaves out; of its four sources the two that brought
/// rules, each with its notice, and the table of colours, because the file carries one of its
/// colours; the colour a rule names and not the other; and the rules. No definitions, nothing of
/// the linework codes, and not what the session is based on.
const NOTICED_CODES: &str = concat!(
    "{\n",
    "  \"format\": \"katana-customisation\",\n",
    "  \"version\": 1,\n",
    "  \"name\": \"base\",\n",
    "  \"description\": \"The base set.\",\n",
    "  \"notice\": [\n",
    "    \"Base: all rights reserved.\",\n",
    "    \"Marks drawn by hand.\"\n",
    "  ],\n",
    "  \"sources\": [\n",
    "    {\"name\": \"base\", \"rules\": true},\n",
    "    {\"name\": \"client\", \"rules\": true, ",
    "\"notice\": [\"Client codes, for this job only.\"]},\n",
    "    {\"name\": \"tints\", \"notice\": [\"Tints: free to use.\"]}\n",
    "  ],\n",
    "  \"colours\": {\n",
    "    \"tint teal\": \"#008080\"\n",
    "  },\n",
    "  \"codes\": [\n",
    "    {\"key\": \"PG*\", \"sets\": \"feature\", \"layer\": \"BASE PEGS\", ",
    "\"colour\": \"tint teal\", \"draw\": \"point\"},\n",
    "    {\"key\": \"FN*\", \"sets\": \"feature\", \"layer\": \"CLIENT FENCES\"}\n",
    "  ]\n",
    "}\n"
);

/// What a counting commit hook saw.
#[derive(Default)]
struct Seen {
    begun: Cell<i32>,
    finished: Cell<i32>,
    hand_back: Cell<bool>,
    map_at_begin: Cell<u64>,
    map_at_finish: Cell<u64>,
    kept_at_begin: Cell<bool>,
    kept_at_finish: Cell<bool>,
}

mod survey_code_manager {
    use super::*;

    #[test]
    fn is_a_non_modal_dialog_with_its_five_tabs_named() {
        let bench = manager(fixture_host());
        let target = bench
            .harness
            .target_record("surveyCodeManagerDialog")
            .expect("the dialog is a target");
        assert!(!target.modal);
        assert!(
            !bench.dialog.interactive(&bench.host),
            "the tests run headless, which is never interactive"
        );
        for tab in [
            "codeTableTab",
            "codesInDrawingTab",
            "codeIssuesTab",
            "applyCodesTab",
            "lineworkTab",
        ] {
            assert!(bench.has(tab), "{tab}");
        }
    }

    #[test]
    fn the_code_table_shows_every_key_of_the_fixture_under_its_group() {
        let bench = manager(fixture_host());
        // The 8 distinct keys of the fixture's 11 rules.
        assert_eq!(
            keys_shown(&bench),
            keys(&["*", "1*", "2*", "AC*", "KB*", "PX*", "TR*", "WM*"])
        );
        let tree = bench.dialog.code_tree();
        // Groups in name order; the bare `*` says no group. "TEST - TEXT" holds both text codes.
        let text = heading(&tree, "TEST - TEXT").expect("TEST - TEXT");
        assert_eq!(text.children.len(), 2);
        assert!(heading(&tree, "(no group)").is_some());
        // AC*'s row: its two rules beneath it, and what it resolves to - the feature rule's layer
        // and the vertex symbol with its size.
        let furniture = heading(&tree, "TEST - FURNITURE").expect("TEST - FURNITURE");
        assert_eq!(furniture.children.len(), 1);
        let chamber = &furniture.children[0];
        assert_eq!(chamber.children.len(), 2);
        assert_eq!(chamber.cells[2], "TEST FURNITURE");
        assert_eq!(chamber.cells[4], "Point");
        assert_eq!(chamber.cells[6], "TEST Survey Mark, 1.5");
        assert_eq!(chamber.children[0].cells[0], "#2 AC* (feature)");
        assert_eq!(chamber.children[1].cells[0], "#7 AC* (symbol)");
        // KB*'s colour is a name no table knows: said so, not swatched.
        let roads = heading(&tree, "TEST - ROADS").expect("TEST - ROADS");
        assert_eq!(roads.children[0].cells[3], "sui test purple (no RGB)");
    }

    #[test]
    fn the_search_and_the_section_chip_narrow_the_table() {
        let mut bench = manager(fixture_host());
        assert!(bench.has("codeTableFilter"));
        // "water" folds to the WM* comment "[WM*] Water main" and its linestyle.
        bench.fill("filterText", "water");
        assert_eq!(keys_shown(&bench), keys(&["WM*"]));
        bench.fill("filterText", "");
        // The Symbol chip: the three keys with a symbol rule.
        bench.press("filterSymbol");
        assert_eq!(keys_shown(&bench), keys(&["AC*", "PX*", "TR*"]));
        // A chip says its section's word with a capital, and a chip of several sections the word
        // of the first: `pipe` for the three pipe sections, `attributes` for the two attribute
        // ones. All seven are written here by hand from the table of words in
        // docs/customisation.md, since the dialog builds them from entity's table. The two first
        // labelled "Map" and "Tinable" are still found by the object names those labels gave
        // them.
        let chips: Vec<(String, String)> = [
            ("filterAll", "All"),
            ("filterMap", "Feature"),
            ("filterSymbol", "Symbol"),
            ("filterText", "Text"),
            ("filterPipe", "Pipe"),
            ("filterAttributes", "Attributes"),
            ("filterTinable", "Surface"),
        ]
        .iter()
        .map(|(name, label)| (String::from(*name), String::from(*label)))
        .collect();
        // The bar's own buttons are its chips: a chip this does not name would make an eighth.
        assert_eq!(bench.dialog.table_chips().len(), 7);
        assert_eq!(bench.dialog.table_chips(), chips);
        // Each chip but the one that shares the search field's name is drawn under its own.
        for (name, label) in &chips {
            if name != "filterText" {
                assert_eq!(bench.text(name), *label, "{name}");
            }
        }
    }

    #[test]
    fn the_form_offers_the_nine_sections_by_the_words_a_customisation_file_holds() {
        let bench = manager(fixture_host());
        // What "sets" takes in a Katana customisation, in the same order.
        assert_eq!(
            bench.items("ruleSection"),
            strings(&[
                "feature",
                "symbol",
                "text",
                "pipe",
                "vertexPipe",
                "segmentPipe",
                "attributes",
                "vertexAttributes",
                "surface"
            ])
        );
        // The eighth column of the table, between Symbol and Attributes.
        assert_eq!(CODE_TABLE_HEADERS[6], "Symbol");
        assert_eq!(CODE_TABLE_HEADERS[7], "Surface");
        assert_eq!(CODE_TABLE_HEADERS[8], "Attributes");
    }

    #[test]
    fn testing_a_code_shows_the_rule_that_set_its_layer() {
        let mut bench = manager(fixture_host());
        bench.fill("testCode", "WM01");
        // WM01 meets WM* (rule #0) and `*` (rule #10); its layer comes from #0.
        let fields = bench.dialog.explained_fields();
        let model = fields
            .iter()
            .find(|row| row[0] == "layer")
            .expect("the layer is explained");
        assert_eq!(model[1], "TEST SERVICES");
        assert_eq!(model[2], "#0 WM* (feature)");
        let heading = bench.text("explainHeading");
        assert!(heading.contains("matched"), "{heading}");
        assert!(heading.contains("WM*"));
        // The `*` rule's attribute, cited to it.
        let attribute = fields
            .iter()
            .find(|row| row[0] == "attribute (string)")
            .expect("the attribute is explained");
        assert_eq!(attribute[1], "text Source = Katana test fixture");
        assert_eq!(attribute[2], "#10 * (attributes)");
        // The table drawn is those rows.
        assert_eq!(bench.rows("explainFields"), fields.len());
    }

    #[test]
    fn a_definition_drawn_at_vertices_is_said_to_be_at_vertices_wherever_it_is_named() {
        let mut bench = manager(fixture_host());
        let said = |bench: &Manager, line: &str| {
            bench
                .text("explainDefinitions")
                .split('\n')
                .any(|each| each == line)
        };
        // As a SYMBOL. AC01's (#7) is TEST Survey Mark, which the library draws at vertices: CODE
        // EXPLAIN's own words for it. TR01's (#8) is TEST Tree, which it does not, so nothing is
        // added.
        bench.fill("testCode", "AC01");
        assert!(
            said(&bench, "Symbol TEST Survey Mark: defined, at vertices"),
            "{}",
            bench.text("explainDefinitions")
        );
        bench.fill("testCode", "TR01");
        assert!(
            said(&bench, "Symbol TEST Tree: defined"),
            "{}",
            bench.text("explainDefinitions")
        );

        // As a LINESTYLE, which it cannot be: a new rule gives the same name as its linestyle. The
        // table marks the name with what it is, and the explanation says that and what is drawn
        // instead - by the phrase the Style Manager's "Drawn as" has for the same state.
        let marks = SurveyRule {
            key: String::from("SM*"),
            model: String::from("TEST MARKS"),
            linestyle: String::from("TEST Survey Mark"),
            ..SurveyRule::default()
        };
        assert!(bench.dialog.add_rule(&mut bench.host, marks).is_ok());
        let row = key_row(&bench, "SM*").expect("SM* is listed");
        assert_eq!(row.cells[5], "TEST Survey Mark (an `at vertices` symbol)");
        bench.fill("testCode", "SM01");
        assert!(
            said(
                &bench,
                "Linestyle TEST Survey Mark: an `at vertices` symbol, not a linestyle: drawn as \
                 a plain line"
            ),
            "{}",
            bench.text("explainDefinitions")
        );
    }

    #[test]
    fn a_code_typed_in_the_wrong_case_is_unmatched_and_the_key_it_missed_is_named() {
        let mut bench = manager(fixture_host());
        bench.fill("testCode", "wm01");
        let heading = bench.text("explainHeading");
        let misses = bench.text("explainNearMisses");
        // Only `*` (attributes, no model) meets "wm01": fallback only (D5).
        assert!(heading.contains("fallback only"), "{heading}");
        assert!(
            misses.contains(&format!("WM* ({})", to_string(NearMissKind::Case))),
            "{misses}"
        );
    }

    #[test]
    fn editing_the_buffer_leaves_the_document_alone_until_apply_and_revert_restores_it() {
        let host = fixture_host();
        let before = host.document().survey_map().clone();
        let generation = host.document().survey_map_generation();
        let mut bench = manager(host);
        edit_water_main_layer(&mut bench, "TEST WATER");

        assert_eq!(bench.dialog.buffer().size(), 11);
        assert_eq!(bench.dialog.buffer().rules()[0].model, "TEST WATER");
        assert!(bench.dialog.dirty());
        assert!(
            *bench.document().survey_map() == before,
            "the Document's map moved before Apply"
        );
        assert_eq!(bench.document().survey_map_generation(), generation);
        assert!(bench.text("dirtyIndicator").contains("Unapplied"));

        bench.press("revertMap");
        assert!(*bench.dialog.buffer() == before);
        assert!(!bench.dialog.dirty());
        assert!(*bench.document().survey_map() == before);
    }

    #[test]
    fn apply_puts_the_buffer_on_the_document_where_explain_code_sees_it() {
        let mut bench = manager(fixture_host());
        edit_water_main_layer(&mut bench, "TEST WATER");
        bench.press("applyMap");
        assert!(!bench.dialog.dirty());
        assert!(bench.document().survey_map() == bench.dialog.buffer());

        let document = bench.document();
        let definitions = move |name: &str| document.definition_for(name);
        let definition_of: &DefinitionLookup<'_> = &definitions;
        let colours = colour_lookup(document);
        let colour_of: &ColourLookup<'_> = &colours;
        let explained = explain_code(
            document.survey_map(),
            "WM01",
            Some(definition_of),
            Some(colour_of),
        );
        let model = explained
            .fields
            .iter()
            .find(|field| field.field == "layer")
            .expect("the layer is explained");
        assert_eq!(model.value, "TEST WATER");
        assert_eq!(model.rule, 0);
    }

    #[test]
    fn add_duplicate_move_and_delete_edit_the_buffer_in_place() {
        let mut bench = manager(fixture_host());
        // Duplicate #1 (KB*): the copy lands at #2 and everything after moves on.
        bench.dialog.select_rule(&mut bench.host, 1);
        bench.press("ruleDuplicate");
        assert_eq!(bench.dialog.buffer().size(), 12);
        assert!(bench.dialog.buffer().rules()[2] == bench.dialog.buffer().rules()[1]);
        assert_eq!(bench.dialog.current_rule(), Some(2));
        // Up: the copy back to #1, the original now #2 - order is precedence.
        bench.press("ruleUp");
        assert_eq!(bench.dialog.current_rule(), Some(1));
        // Delete it: the fixture's 11 rules again, as loaded.
        bench.press("ruleDelete");
        assert!(bench.dialog.buffer() == bench.document().survey_map());
        assert!(!bench.dialog.dirty());

        // A new rule typed into the form, added at the end.
        bench.press("ruleNew");
        bench.fill("ruleKey", "FH*");
        bench.fill("ruleModel", "TEST SERVICES");
        bench.fill("ruleComment", "Fire hydrant");
        bench.press("ruleAdd");
        assert_eq!(bench.dialog.buffer().size(), 12);
        assert_eq!(bench.dialog.buffer().rules()[11].key, "FH*");
        assert_eq!(bench.dialog.buffer().rules()[11].model, "TEST SERVICES");
        assert_eq!(bench.dialog.buffer().rules()[11].comment, "Fire hydrant");
        assert_eq!(bench.document().survey_map().size(), 11);
    }

    #[test]
    fn an_invalid_layer_is_said_live_and_the_rule_is_refused_leaving_the_buffer_alone() {
        let mut bench = manager(fixture_host());
        bench.dialog.select_rule(&mut bench.host, 0);
        // An empty path segment: "A//B" is not a layer path.
        bench.fill("ruleModel", "A//B");
        let status = bench.text("ruleModelStatus");
        assert!(!status.contains("valid layer"), "{status}");
        bench.press("ruleUpdate");
        assert_eq!(bench.dialog.buffer().rules()[0].model, "TEST SERVICES");
        assert!(!bench.dialog.dirty());
    }

    #[test]
    fn the_issues_tab_lists_the_unknown_colour_and_the_undefined_symbol() {
        let mut bench = manager(fixture_host());
        // KB*'s "sui test purple" (#1) and PX*'s "TEST Missing Symbol" (#9) - and nothing else:
        // every other colour is standard, "0" is a plain line, and TR*'s TEST Tree is listed as a
        // symbol by its customisation (D3).
        assert_eq!(bench.rows("issuesTable"), 2);
        let colour = bench.row("issuesTable", 0);
        assert_eq!(colour[1], "#1");
        assert_eq!(colour[2], "KB*");
        assert_eq!(colour[4], to_string(LintKind::UnknownColour));
        let symbol = bench.row("issuesTable", 1);
        assert_eq!(symbol[1], "#9");
        assert_eq!(symbol[2], "PX*");
        assert_eq!(symbol[4], to_string(LintKind::UnresolvedSymbol));

        // A double-click takes the rule to the form on the Code Table.
        bench.dialog.activate_issue(1);
        bench.draw();
        assert_eq!(bench.dialog.current_rule(), Some(9));
        assert_eq!(bench.dialog.current_tab(), "codeTableTab");
        assert_eq!(bench.value("codeManagerTabs"), "Code Table");
    }

    #[test]
    fn apply_codes_preview_changes_nothing_and_execute_is_one_undo_step() {
        let mut host = fixture_host();
        let first = add_point(&mut host, 0.0, "WM01", "1");
        let second = add_point(&mut host, 5.0, "WM01", "2");
        let undo_before = host.document().history().undo_count();
        let revision = host.document().model_revision();
        let mut bench = manager(host);

        bench.press("applyPreview");
        assert_eq!(
            bench.document().history().undo_count(),
            undo_before,
            "Preview executed something"
        );
        assert_eq!(bench.document().model_revision(), revision);
        assert!(
            bench.lines().is_empty(),
            "Preview is a plan made here: it runs no line"
        );
        assert_eq!(layer_of(&bench, first), "0");
        let report = bench.value("applyReport");
        assert!(report.contains("WM01"), "{report}");
        // What the scope took comes first: the whole drawing's two entities.
        assert!(report.starts_with("2 matched."), "{report}");
        assert_eq!(bench.rows("applyRows"), 1);
        assert_eq!(bench.row("applyRows", 0)[0], "WM01");
        assert_eq!(bench.row("applyRows", 0)[1], "2");
        // ... and the line Execute would run is on show.
        assert_eq!(bench.value("applyCommand"), "CODE DRAWING");

        bench.press("applyExecute");
        // Execute is that line, through the executor the manager was handed.
        assert_eq!(bench.lines(), strings(&["CODE DRAWING"]));
        assert_eq!(bench.document().history().undo_count(), undo_before + 1);
        assert_eq!(layer_of(&bench, first), "TEST SERVICES");
        assert_eq!(layer_of(&bench, second), "TEST SERVICES");
        // The pane then holds the verb's own answer: what the scope took first.
        let report = bench.value("applyReport");
        assert!(report.starts_with("scope=drawing matched=2"), "{report}");
        assert!(bench.document_mut().undo().is_ok());
        assert_eq!(layer_of(&bench, first), "0");
        assert_eq!(layer_of(&bench, second), "0");
    }

    // "Execute builds the verb line": each control of the Apply Codes tab is a word of CODE (cad's
    // survey_code_verbs; the scope's words are the shared grammar's, cad's scope_verbs), and the
    // line is rebuilt as a control changes. What cannot be said on a line is not run some other
    // way.
    #[test]
    fn the_apply_codes_tab_builds_exactly_the_code_line_its_controls_say() {
        let mut host = fixture_host();
        let first = add_point(&mut host, 0.0, "WM01", "1");
        let second = add_point(&mut host, 5.0, "WM01", "2");
        let mut bench = manager(host);

        // The whole drawing to begin with, and the property found.
        assert!(bench.checked("applyScopeDrawing"));
        assert_eq!(bench.value("applyCommand"), "CODE DRAWING");

        bench.press("applyScopeSelection");
        assert_eq!(bench.value("applyCommand"), "CODE SELECTION");
        bench.fill("applyProperty", "feature_code");
        assert_eq!(
            bench.value("applyCommand"),
            "CODE SELECTION PROPERTY feature_code"
        );
        // A name with a blank is one quoted word, as the command line reads it.
        bench.fill("applyProperty", "Field Code");
        assert_eq!(
            bench.value("applyCommand"),
            "CODE SELECTION PROPERTY \"Field Code\""
        );
        bench.fill("applyProperty", "");

        // The filter is the scope's WHERE, before the verb's own words.
        bench.fill("applyFilterLayer", "survey/*");
        bench.fill("applyProperty", "code");
        assert_eq!(
            bench.value("applyCommand"),
            "CODE SELECTION WHERE LAYER=survey/* PROPERTY code"
        );
        bench.fill("applyFilterLayer", "");
        bench.fill("applyProperty", "");

        // The checked layers: the drawing has the one layer, "0".
        bench.press("applyScopeLayers");
        assert_eq!(bench.items("applyLayers"), strings(&["0"]));
        // None ticked: nothing a line can say, and Execute runs nothing.
        assert!(bench.value("applyCommand").is_empty());
        let placeholder = bench.text("applyCommand");
        assert!(
            placeholder.contains("tick at least one layer"),
            "{placeholder}"
        );
        bench.press("applyExecute");
        assert!(bench.lines().is_empty());
        assert_eq!(layer_of(&bench, first), "0");
        bench.dialog.apply_scope_mut().tick_layer("0", true);
        bench.draw();
        assert_eq!(bench.value("applyCommand"), "CODE LAYERS 0");
        bench.fill("applySublayers", "off");
        assert_eq!(bench.value("applyCommand"), "CODE LAYERS 0 ONLY");

        // A property name no line can carry is refused where it is typed.
        bench.fill("applyProperty", "co\"de");
        assert!(bench.value("applyCommand").is_empty());
        let placeholder = bench.text("applyCommand");
        assert!(placeholder.contains("double quote"), "{placeholder}");
        bench.press("applyExecute");
        assert!(bench.lines().is_empty());
        bench.fill("applyProperty", "");

        // The selection: only the point selected is coded, by exactly that line.
        bench.press("applyScopeSelection");
        bench.document_mut().selection_mut().set(vec![second]);
        bench.document().notify_selection_changed();
        bench.press("applyExecute");
        assert_eq!(bench.lines(), strings(&["CODE SELECTION"]));
        assert_eq!(layer_of(&bench, first), "0");
        assert_eq!(layer_of(&bench, second), "TEST SERVICES");
    }

    // A manager with no executor does not do the work some other way: Execute says it cannot run
    // the line, naming it, and the drawing is as it was.
    #[test]
    fn with_no_executor_execute_says_it_cannot_run_its_line_and_changes_nothing() {
        let mut host = fixture_host();
        add_point(&mut host, 0.0, "WM01", "1");
        add_point(&mut host, 5.0, "WM01", "2");
        let undo_before = host.document().history().undo_count();
        let revision = host.document().model_revision();
        let mut bench = manager_with(host, context().without_command_line());

        bench.press("applyExecute");
        assert!(bench.logged_containing("cannot run \"CODE DRAWING\""));
        bench.press("lineworkExecute");
        assert!(bench.logged_containing("cannot run \"LINEWORK DRAWING\""));
        bench.fill("lineworkStart", "BEG");
        bench.press("lineworkCodesUse");
        assert!(bench.logged_containing("cannot run \"CUSTOMISE SET linework.start=BEG\""));

        assert!(bench.lines().is_empty());
        assert_eq!(bench.document().history().undo_count(), undo_before);
        assert_eq!(bench.document().model_revision(), revision);
        assert_eq!(bench.document().model().entities.size(), 2);
        assert_eq!(bench.document().customisation_state().linework.start, "ST");
    }

    #[test]
    fn the_census_classes_the_drawings_codes_and_makes_a_rule_for_an_unmatched_one() {
        let mut host = fixture_host();
        add_point(&mut host, 0.0, "WM01", "1");
        add_point(&mut host, 5.0, "WM01", "2");
        let stray = add_point(&mut host, 9.0, "ZZ9", "3");
        let mut bench = manager(host);
        // In code order: WM01 on two points, matched by WM*; ZZ9 met only by the attribute-only
        // `*`: fallback only (D5).
        assert_eq!(bench.rows("censusTable"), 2);
        assert_eq!(bench.row("censusTable", 0)[0], "WM01");
        assert_eq!(bench.row("censusTable", 0)[1], "2");
        assert_eq!(bench.row("censusTable", 0)[2], "matched");
        assert_eq!(bench.row("censusTable", 1)[0], "ZZ9");
        assert_eq!(bench.row("censusTable", 1)[2], "fallback only");

        assert!(bench.dialog.select_census_code("ZZ9"));
        bench.press("censusSelect");
        assert_eq!(bench.document().selection().ids(), vec![stray]);

        bench.press("censusNewRule");
        assert_eq!(bench.value("ruleKey"), "ZZ*");
        assert!(
            bench.dialog.current_rule().is_none(),
            "nothing is added until Add"
        );
        assert_eq!(bench.dialog.buffer().size(), 11);
        bench.fill("ruleModel", "TEST MISC");
        bench.press("ruleAdd");
        // The census follows the buffer: ZZ9 is now matched by the new ZZ*.
        assert_eq!(bench.row("censusTable", 1)[2], "matched");
    }

    // The Codes in Drawing tab has the shared scope and filter controls (CLAUDE.md section 1.1)
    // and builds the line CODE CENSUS <scope> [WHERE ...] [PROPERTY p] from them, as the other two
    // tabs build theirs. The table is the census of exactly what the controls take.
    #[test]
    fn the_census_tab_counts_what_its_scope_takes_and_builds_the_code_census_line_it_says() {
        let mut host = fixture_host();
        add_point(&mut host, 0.0, "WM01", "1");
        add_point(&mut host, 5.0, "WM01", "2");
        let stray = add_point(&mut host, 9.0, "ZZ9", "3");
        let mut bench = manager(host);

        // The whole drawing to begin with: three entities, two distinct codes.
        assert!(bench.checked("censusScopeDrawing"));
        assert_eq!(bench.value("censusCommand"), "CODE CENSUS DRAWING");
        assert_eq!(bench.rows("censusTable"), 2);
        let summary = bench.text("censusSummary");
        assert!(
            summary.starts_with(
                "3 entities matched the scope; 3 carry a code under \"code\": 2 distinct codes"
            ),
            "{summary}"
        );

        // The selection, with the stray point in it: the table is of that point alone, and Select
        // Entities With Code acts among what the scope took.
        bench.press("censusScopeSelection");
        assert_eq!(bench.value("censusCommand"), "CODE CENSUS SELECTION");
        assert_eq!(bench.rows("censusTable"), 0, "nothing is selected yet");
        bench.document_mut().selection_mut().set(vec![stray]);
        bench.document().notify_selection_changed();
        bench.press("censusRefresh");
        assert_eq!(bench.rows("censusTable"), 1);
        assert_eq!(bench.row("censusTable", 0)[0], "ZZ9");
        assert_eq!(bench.row("censusTable", 0)[1], "1");
        let summary = bench.text("censusSummary");
        assert!(
            summary.starts_with(
                "1 entities matched the scope; 1 carry a code under \"code\": 1 distinct codes"
            ),
            "{summary}"
        );

        // The filter is the scope's WHERE, before the verb's own words; a filter that takes
        // nothing is a census of nothing, and said.
        bench.press("censusScopeDrawing");
        bench.fill("censusFilterLayer", "nothing/*");
        assert_eq!(
            bench.value("censusCommand"),
            "CODE CENSUS DRAWING WHERE LAYER=nothing/*"
        );
        assert_eq!(bench.rows("censusTable"), 0);
        let summary = bench.text("censusSummary");
        assert!(
            summary.starts_with("0 entities matched the scope"),
            "{summary}"
        );
        bench.fill("censusFilterLayer", "");
        assert_eq!(bench.rows("censusTable"), 2);

        // The property is the line's PROPERTY, a name with a blank one quoted word.
        bench.fill("censusProperty", "Field Code");
        assert_eq!(
            bench.value("censusCommand"),
            "CODE CENSUS DRAWING PROPERTY \"Field Code\""
        );
        bench.fill("censusProperty", "");

        // Run Line hands exactly that line to the executor.
        bench.host.take_lines();
        bench.press("censusRun");
        assert_eq!(bench.lines(), strings(&["CODE CENSUS DRAWING"]));

        // No layer ticked says nothing a line can: the line is empty, the table is empty with the
        // reason in the summary, and Run runs nothing.
        bench.press("censusScopeLayers");
        assert!(bench.value("censusCommand").is_empty());
        let placeholder = bench.text("censusCommand");
        assert!(
            placeholder.contains("tick at least one layer"),
            "{placeholder}"
        );
        assert_eq!(bench.rows("censusTable"), 0);
        let summary = bench.text("censusSummary");
        assert!(summary.contains("tick at least one layer"), "{summary}");
        bench.host.take_lines();
        bench.press("censusRun");
        assert!(bench.lines().is_empty());
    }

    #[test]
    fn the_census_tab_with_no_executor_says_it_cannot_run_its_line() {
        let mut host = fixture_host();
        add_point(&mut host, 0.0, "WM01", "1");
        let mut bench = manager_with(host, context().without_command_line());
        bench.press("censusRun");
        assert!(bench.logged_containing("cannot run \"CODE CENSUS DRAWING\""));
        // The table is still the census of the drawing: counting is not the line's.
        assert_eq!(bench.rows("censusTable"), 1);
    }

    #[test]
    fn linework_preview_changes_nothing_and_execute_is_one_undo_step() {
        let mut host = fixture_host();
        // WM* is a Line code: three points of string WM01 make one line.
        add_point(&mut host, 0.0, "WM01", "1");
        add_point(&mut host, 5.0, "WM01", "2");
        add_point(&mut host, 10.0, "WM01", "3");
        let undo_before = host.document().history().undo_count();
        let entities_before = host.document().model().entities.size();
        let revision = host.document().model_revision();
        let mut bench = manager(host);

        bench.press("lineworkPreview");
        assert_eq!(bench.document().history().undo_count(), undo_before);
        assert_eq!(bench.document().model().entities.size(), entities_before);
        assert_eq!(bench.document().model_revision(), revision);
        assert!(
            bench.lines().is_empty(),
            "Preview is a plan made here: it runs no line"
        );
        assert_eq!(bench.rows("lineworkStrings"), 1);
        assert_eq!(bench.row("lineworkStrings", 0)[0], "WM01");
        assert_eq!(bench.row("lineworkStrings", 0)[2], "3");
        assert_eq!(bench.row("lineworkStrings", 0)[5], "TEST SERVICES");
        // What the scope took, then the plan: the drawing's three points.
        assert_eq!(
            bench.text("lineworkSummary"),
            "3 matched. Preview only: 1 lines from 3 points; 0 points not placed; 0 notes. \
             Execute builds them as one undoable step."
        );
        assert_eq!(bench.value("lineworkCommand"), "LINEWORK DRAWING");

        bench.press("lineworkExecute");
        // Execute is that line, through the executor the manager was handed.
        assert_eq!(bench.lines(), strings(&["LINEWORK DRAWING"]));
        assert_eq!(bench.document().history().undo_count(), undo_before + 1);
        assert_eq!(
            bench.document().model().entities.size(),
            entities_before + 1
        );
        // Said from the verb's own first record: the three points, one line.
        assert_eq!(
            bench.text("lineworkSummary"),
            "Executed as one undoable step: 1 lines drawn. linework scope=drawing matched=3 \
             considered=3 lines=1 unplaced=0 notes=0"
        );

        // Run again, the line the drawing now holds is not drawn a second time, as LINEWORK typed
        // again does not draw it: the preview says so, and the run adds no entity and no undo
        // step. (The scope now takes four: the three points and the line.)
        bench.press("lineworkPreview");
        assert_eq!(bench.rows("lineworkStrings"), 0);
        assert_eq!(
            bench.text("lineworkSummary"),
            "4 matched. Nothing to build: 3 points looked at, 0 not placed. 1 lines are in the \
             drawing already and are not drawn again."
        );
        bench.press("lineworkExecute");
        assert_eq!(bench.lines().len(), 2);
        assert_eq!(bench.document().history().undo_count(), undo_before + 1);
        assert_eq!(
            bench.document().model().entities.size(),
            entities_before + 1
        );
        let summary = bench.text("lineworkSummary");
        assert!(
            summary.starts_with(
                "Executed: no line was drawn, so nothing was added to the undo history."
            ),
            "{summary}"
        );

        assert!(bench.document_mut().undo().is_ok());
        assert_eq!(bench.document().model().entities.size(), entities_before);
    }

    // Each control of the Linework tab is a word of LINEWORK (cad's linework_verbs), said only
    // when it is not what the verb does unasked. The chord length is in millimetres on the form
    // and in the drawing's units - metres - on the line: 350 mm is CHORD 0.35.
    #[test]
    fn the_linework_tab_builds_exactly_the_linework_line_its_controls_say() {
        let mut host = fixture_host();
        // WM* is a Line code. A half circle of radius 10 about the origin, begun at (10,0),
        // through (0,10), ended at (-10,0): at a chord length of 0.35 each quarter is three
        // chords of 30 degrees (tests/cad/customisation/test_linework.cpp works it), so the line
        // has 1 + 3 + 3 = 7 vertices, where the 5 mm the verb starts on gives 51. The points are
        // coded under "fcode", which is not among the names a code is looked for under.
        let shots = [
            (Point2::new(10.0, 0.0), "WM01 BC"),
            (Point2::new(0.0, 10.0), "WM01"),
            (Point2::new(-10.0, 0.0), "WM01 EC"),
        ];
        for (at, code) in shots {
            let mut entity = Entity {
                geometry: Geometry::Point(PointGeometry { position: at }),
                ..Entity::default()
            };
            entity.properties.insert(
                String::from("fcode"),
                PropertyValue::Text(String::from(code)),
            );
            assert!(
                host.document_mut()
                    .execute(create_entities(vec![entity]))
                    .is_ok()
            );
        }
        let mut bench = manager(host);

        assert!(bench.checked("lineworkScopeDrawing"));
        assert_eq!(
            bench.value("lineworkChordTolerance").parse::<f64>().ok(),
            Some(5.0),
            "the 5 mm the verb starts on"
        );
        assert_eq!(bench.value("lineworkCommand"), "LINEWORK DRAWING");

        // In the order observed: the points were made here and carry no number.
        bench.fill("lineworkOrder", "In the order observed");
        assert_eq!(
            bench.value("lineworkCommand"),
            "LINEWORK DRAWING ORDER entity"
        );
        bench.fill("lineworkProperty", "fcode");
        assert_eq!(
            bench.value("lineworkCommand"),
            "LINEWORK DRAWING PROPERTY fcode ORDER entity"
        );
        bench.fill("lineworkChordTolerance", "350");
        assert_eq!(
            bench.value("lineworkCommand"),
            "LINEWORK DRAWING PROPERTY fcode ORDER entity CHORD 0.35"
        );
        bench.press("lineworkScopeSelection");
        assert_eq!(
            bench.value("lineworkCommand"),
            "LINEWORK SELECTION PROPERTY fcode ORDER entity CHORD 0.35"
        );
        bench.press("lineworkScopeDrawing");

        // The preview is planned with those words - one curve, its three points - and Execute
        // runs exactly the line shown.
        bench.press("lineworkPreview");
        assert_eq!(bench.rows("lineworkStrings"), 1);
        assert_eq!(bench.row("lineworkStrings", 0)[0], "WM01");
        assert_eq!(bench.row("lineworkStrings", 0)[2], "3");
        assert_eq!(bench.row("lineworkStrings", 0)[4], "1", "one curve");
        assert!(bench.lines().is_empty());

        bench.press("lineworkExecute");
        assert_eq!(
            bench.lines(),
            strings(&["LINEWORK DRAWING PROPERTY fcode ORDER entity CHORD 0.35"])
        );
        let lines = polylines(bench.document());
        assert_eq!(lines.len(), 1);
        match &lines[0].geometry {
            Geometry::Polyline(polyline) => assert_eq!(polyline.vertices.len(), 7),
            other => panic!("the line is a polyline: {other:?}"),
        }

        // Back at the verb's own chord length and order, the words go again.
        assert!(bench.document_mut().undo().is_ok());
        bench.fill("lineworkChordTolerance", "5");
        bench.fill("lineworkOrder", "By point number");
        assert_eq!(
            bench.value("lineworkCommand"),
            "LINEWORK DRAWING PROPERTY fcode"
        );
    }

    // The control codes are the customisation's - the Document's - and Use These Codes is the
    // line CUSTOMISE SET linework.*: it says the codes that are changing and no others, in the
    // order the controls stand (start, end, close, arcstart, arcend, join, rectangle). The
    // fixture's customisation says nothing of them, so they start as the usual spellings: ST, END,
    // CL ...
    #[test]
    fn the_linework_codes_are_the_customisations_and_use_these_codes_runs_the_set_line() {
        let mut bench = manager(fixture_host());
        let held = |bench: &Manager| -> LineworkCodes {
            bench.document().customisation_state().linework.clone()
        };
        assert_eq!(bench.value("lineworkStart"), "ST");
        assert_eq!(bench.value("lineworkEnd"), "END");
        assert_eq!(bench.value("lineworkClose"), "CL");

        // Nothing typed: nothing to set, and no line is run for it.
        bench.press("lineworkCodesUse");
        assert!(bench.lines().is_empty());
        let status = bench.text("lineworkCodesStatus");
        assert!(status.contains("already"), "{status}");

        bench.fill("lineworkStart", "BEG");
        assert_eq!(held(&bench).start, "ST", "typed is not set");
        bench.press("lineworkCodesUse");
        assert_eq!(
            bench.lines(),
            strings(&["CUSTOMISE SET linework.start=BEG"])
        );
        assert_eq!(held(&bench).start, "BEG");
        assert_eq!(held(&bench).end, "END");

        // Two at once, one of them emptied: an empty spelling switches the control off, and is
        // written as the key with nothing after its '='.
        bench.fill("lineworkEnd", "FIN");
        bench.fill("lineworkClose", "");
        bench.press("lineworkCodesUse");
        assert_eq!(bench.lines().len(), 2);
        assert_eq!(
            bench.lines()[1],
            "CUSTOMISE SET linework.end=FIN linework.close="
        );
        assert_eq!(held(&bench).start, "BEG");
        assert_eq!(held(&bench).end, "FIN");
        assert_eq!(held(&bench).close, "");

        // "FIN" twice: a token meaning two things means neither. The button is off, and nothing is
        // set.
        bench.fill("lineworkStart", "FIN");
        assert!(!bench.enabled("lineworkCodesUse"));
        assert_eq!(bench.lines().len(), 2);
        assert_eq!(held(&bench).start, "BEG");

        // What is being typed survives a change of something else - a click in the drawing - and
        // is replaced when the customisation's codes change: typed on the command line here, as
        // any line may be.
        bench.fill("lineworkStart", "HALF");
        bench.document_mut().selection_mut().clear();
        bench.document().notify_selection_changed();
        bench.draw();
        assert_eq!(bench.value("lineworkStart"), "HALF");
        let parts = &mut bench.host.parts;
        assert!(
            parts
                .interpreter
                .run(
                    &mut parts.document,
                    "CUSTOMISE SET linework.start=GO linework.close=SHUT"
                )
                .is_ok()
        );
        bench.draw();
        assert_eq!(bench.value("lineworkStart"), "GO");
        assert_eq!(bench.value("lineworkEnd"), "FIN");
        assert_eq!(bench.value("lineworkClose"), "SHUT");

        // A manager opened afterwards shows them from the start.
        let later = manager(bench.host);
        assert_eq!(later.value("lineworkStart"), "GO");
        assert_eq!(later.value("lineworkClose"), "SHUT");
    }

    #[test]
    fn apply_calls_the_commit_hook_before_the_map_is_set_and_what_it_hands_back_after() {
        // The commit hook is how a session whose customisation is the kept one stays kept across
        // an editor's own commit. Its maker has to be asked BEFORE the commit - Apply's
        // set_survey_map leaves the session "not kept", so only before it can "was it kept?" be
        // answered - and what it hands back is called AFTER, which is where it keeps again. The
        // manager knows neither; it calls, in that order, as the definition editor's Save does. A
        // counting hook that notes at each call the survey map's generation, and whether the
        // session says it is kept, shows which side of the commit the call came on.
        let mut host = fixture_host();
        // The session as a start leaves one: kept.
        let session = host.document().customisation();
        assert!(
            host.document_mut()
                .install_customisation(session, CustomisationOrigin::Kept, true)
                .is_ok()
        );
        assert!(host.document().customisation_state().kept);
        let seen = Rc::new(Seen {
            hand_back: Cell::new(true),
            kept_at_finish: Cell::new(true),
            ..Seen::default()
        });
        let begin: BeginCommit = {
            let seen = Rc::clone(&seen);
            Rc::new(move |host: &mut dyn Host| -> Option<Committed> {
                seen.begun.set(seen.begun.get() + 1);
                seen.map_at_begin
                    .set(host.document().survey_map_generation());
                seen.kept_at_begin
                    .set(host.document().customisation_state().kept);
                if !seen.hand_back.get() {
                    return None;
                }
                let seen = Rc::clone(&seen);
                Some(Box::new(move |host: &mut dyn Host| {
                    seen.finished.set(seen.finished.get() + 1);
                    seen.map_at_finish
                        .set(host.document().survey_map_generation());
                    seen.kept_at_finish
                        .set(host.document().customisation_state().kept);
                }))
            })
        };
        let mut bench = manager_with(
            host,
            CodesContext {
                begin_commit: Some(begin),
                ..context()
            },
        );

        // Opening the manager and editing its buffer commit nothing.
        edit_water_main_layer(&mut bench, "TEST WATER");
        assert_eq!(seen.begun.get(), 0);

        let before = bench.document().survey_map_generation();
        assert!(bench.dialog.apply(&mut bench.host).is_ok());
        assert_eq!(seen.begun.get(), 1);
        assert_eq!(seen.finished.get(), 1);
        assert_eq!(
            seen.map_at_begin.get(),
            before,
            "asked before the map was set"
        );
        assert!(
            seen.kept_at_begin.get(),
            "and so while the session still said it was kept"
        );
        assert_eq!(
            seen.map_at_finish.get(),
            before + 1,
            "answered once it had been"
        );
        assert!(
            !seen.kept_at_finish.get(),
            "which is what the answer is there to put right"
        );
        assert_eq!(bench.document().survey_map().rules()[0].model, "TEST WATER");

        // A maker that hands nothing back - the session was not kept - is asked all the same, and
        // that is all.
        seen.hand_back.set(false);
        edit_water_main_layer(&mut bench, "TEST MAINS");
        assert!(bench.dialog.apply(&mut bench.host).is_ok());
        assert_eq!(seen.begun.get(), 2);
        assert_eq!(seen.finished.get(), 1);
        assert_eq!(bench.document().survey_map().rules()[0].model, "TEST MAINS");

        // The Apply button is that call.
        seen.hand_back.set(true);
        edit_water_main_layer(&mut bench, "TEST PIPES");
        assert!(bench.enabled("applyMap"));
        bench.press("applyMap");
        assert_eq!(seen.begun.get(), 3);
        assert_eq!(seen.finished.get(), 2);
        assert_eq!(bench.document().survey_map().rules()[0].model, "TEST PIPES");
    }

    #[test]
    fn exported_codes_are_a_katana_customisation_of_the_rules_alone_that_reads_back_as_the_buffer()
    {
        let scratch = Scratch::new("codes-exported");
        let mut bench = manager(fixture_host());
        edit_water_main_layer(&mut bench, "TEST WATER");
        // The export is the line CUSTOMISE EXPORT, which writes the drawing's rules: the edit is
        // applied first.
        assert!(bench.dialog.apply(&mut bench.host).is_ok());
        let path = scratch.path("exported.customisation.json");
        assert!(bench.dialog.export_codes(&mut bench.host, &path).is_ok());
        assert_eq!(
            bench.lines().last().cloned(),
            Some(format!("CUSTOMISE EXPORT {} CODES", quoted_file(&path)))
        );

        let bytes = read_bytes(&path);
        // How a Katana customisation file begins and ends (docs/customisation.md, "Layout", rules
        // 1 to 4): `{`, then each member of the file on a line of its own, two blanks in - format,
        // version, name, then the lists - and `}` with a line feed. The name is the session's,
        // which is the first fixture's. Of the three customisations that session was loaded from,
        // the one that brought rules is this file's source - one line, its name and the kind it
        // brought (rules 6 and 11) - and `codes` follows.
        let head = concat!(
            "{\n",
            "  \"format\": \"katana-customisation\",\n",
            "  \"version\": 1,\n",
            "  \"name\": \"test_linestyles\",\n",
            "  \"sources\": [\n",
            "    {\"name\": \"test_survey\", \"rules\": true}\n",
            "  ],\n",
            "  \"codes\": [\n"
        );
        assert!(bytes.starts_with(head), "{bytes}");
        assert!(bytes.ends_with("\n  ]\n}\n"), "{bytes}");
        // A rule is one line, four blanks in, its members in the format's order (rule 6): the
        // fixture file's own first line with the edited layer.
        let water_main = concat!(
            "    {\"key\": \"WM*\", \"sets\": \"feature\", \"layer\": \"TEST WATER\", ",
            "\"colour\": \"blue\", \"draw\": \"line\", \"linestyle\": \"TEST Water Main\", ",
            "\"weight\": \"0\", \"group\": \"TEST - SERVICES\", ",
            "\"comment\": \"[WM*] Water main\"},\n"
        );
        assert!(bytes.contains(water_main), "{bytes}");

        let read = customisation_from_json(bytes.as_bytes()).expect("the file reads");
        assert!(read.map == *bench.dialog.buffer());
        assert_eq!(read.map.rules()[0].model, "TEST WATER");
        // The rules alone: none of the session's seven definitions, and nothing said of its
        // settings - a file of codes for a colleague must not reset their control codes. No
        // colours either: the session defines none.
        assert!(read.library.empty());
        assert!(read.colours.empty());
        assert!(read.linework.is_none());
        assert!(read.automation.is_none());
        // The source that brought rules, said to have brought those alone; the two that brought
        // the session's definitions are no source of this file.
        assert_eq!(read.sources, vec![source("test_survey", false, true, &[])]);
        assert!(read.notice.is_empty());
    }

    #[test]
    fn exported_codes_carry_the_sessions_notice_its_rule_sources_and_the_colours_the_rules_name()
    {
        let scratch = Scratch::new("codes-noticed");
        let mut document = Document::new(None);
        install_noticed_session(&mut document);
        // The session as the merge leaves it, before anything is asked of the manager: four
        // sources, and the first's notice the session's own.
        assert_eq!(document.customisation_state().name, "base");
        assert_eq!(
            document.customisation_state().notice,
            strings(&["Base: all rights reserved."])
        );
        assert_eq!(document.customisation_state().sources.len(), 4);
        assert_eq!(document.customisation_state().colours.size(), 2);
        assert!(document.customisation().based_on.is_some());
        let mut bench = manager_with(interpreter_host(document), CodesContext::default());
        assert_eq!(bench.dialog.buffer().size(), 2);

        let path = scratch.path("noticed.customisation.json");
        assert!(bench.dialog.export_codes(&mut bench.host, &path).is_ok());
        assert_eq!(read_bytes(&path), NOTICED_CODES);

        // The colleague the file is for loads it as a whole customisation is loaded
        // (merge_customisation, then install_customisation: what CUSTOMISE <file> does). They are
        // shown the notice - their session had no name, so it takes the file's with its
        // description and notice, the two lines the file carries - and each source keeps its own.
        // And their own spelling of Start, BEG, is as they left it: the file says nothing of the
        // control codes, which a merge takes from any file that does.
        {
            let mut colleague = Document::new(None);
            let theirs = LineworkCodes {
                start: String::from("BEG"),
                ..LineworkCodes::default()
            };
            assert!(colleague.set_linework_codes(theirs).is_ok());
            install_customisations(
                &mut colleague,
                &[customisation_from_text(&read_bytes(&path))],
            );
            let state = colleague.customisation_state();
            assert_eq!(state.name, "base");
            assert_eq!(state.description, "The base set.");
            assert_eq!(
                state.notice,
                strings(&["Base: all rights reserved.", "Marks drawn by hand."])
            );
            // Their session is not said to be made from the author's base.
            assert!(colleague.customisation().based_on.is_none());
            assert_eq!(
                state.sources,
                vec![
                    source("base", false, true, &[]),
                    source("client", false, true, &["Client codes, for this job only."]),
                    source("tints", false, false, &["Tints: free to use."]),
                ]
            );
            assert_eq!(colleague.survey_map().size(), 2);
            assert_eq!(state.colours.find("tint teal"), Some(rgb(0, 128, 128)));
            assert_eq!(state.linework.start, "BEG");
        }

        // With the one rule that names a colour gone from the buffer, the file carries no colour,
        // and so nothing of the table of colours: it is not a source of this file. Its notice is
        // written with the part's own, after the marks'.
        assert!(bench.dialog.remove_rule(&mut bench.host, 0).is_ok());
        assert!(bench.dialog.apply(&mut bench.host).is_ok());
        assert!(bench.dialog.export_codes(&mut bench.host, &path).is_ok());
        let read = customisation_from_json(read_bytes(&path).as_bytes()).expect("the file reads");
        assert!(read.colours.empty());
        assert_eq!(
            read.sources,
            vec![
                source("base", false, true, &[]),
                source("client", false, true, &["Client codes, for this job only."]),
            ]
        );
        assert_eq!(
            read.notice,
            strings(&[
                "Base: all rights reserved.",
                "Marks drawn by hand.",
                "Tints: free to use."
            ])
        );
        assert!(read.based_on.is_none());
        assert_eq!(read.description, "The base set.");
        assert!(read.map == *bench.dialog.buffer());
    }

    #[test]
    fn exported_codes_carry_the_colours_a_rules_symbol_and_text_name_too() {
        // Four colours of the session's own. A rule names a colour in three places - its own, its
        // symbol's and its text's - and one of the four is named in none of them.
        let scratch = Scratch::new("codes-coloured");
        let mut host = fixture_host();
        let mut colours = ColourTable::new();
        assert!(colours.add("sui test purple", rgb(128, 0, 128)).is_ok());
        assert!(colours.add("sui test teal", rgb(0, 128, 128)).is_ok());
        assert!(colours.add("sui test lime", rgb(0, 255, 0)).is_ok());
        assert!(colours.add("sui test unused", rgb(1, 2, 3)).is_ok());
        host.document_mut().set_colour_table(colours);
        let mut bench = manager(host);
        // KB* (#1) is coloured "sui test purple" already. A symbol rule whose symbol is teal,
        // spelled another way than the table spells it - one name, by the fold a colour name is
        // compared in - and a text rule whose text is lime.
        let marked = SurveyRule {
            key: String::from("MK*"),
            section: SurveySection::VertexSymbol,
            symbol: Some(SurveySymbol {
                style: String::from("TEST Valve"),
                colour: String::from("SUI_Test_Teal"),
                ..SurveySymbol::default()
            }),
            ..SurveyRule::default()
        };
        assert!(bench.dialog.add_rule(&mut bench.host, marked).is_ok());
        let lettered = SurveyRule {
            key: String::from("LT*"),
            section: SurveySection::VertexTextStyle,
            text_style: Some(SurveyTextStyle {
                colour: String::from("sui test lime"),
                ..SurveyTextStyle::default()
            }),
            ..SurveyRule::default()
        };
        assert!(bench.dialog.add_rule(&mut bench.host, lettered).is_ok());
        assert!(bench.dialog.apply(&mut bench.host).is_ok());

        let path = scratch.path("coloured.customisation.json");
        assert!(bench.dialog.export_codes(&mut bench.host, &path).is_ok());
        let read = customisation_from_json(read_bytes(&path).as_bytes()).expect("the file reads");
        // In the order of their folded names, as the table spells them; the standard names the
        // other rules give (blue, white, green ...) are no customisation's to define.
        let entry = |name: &str, colour: Color| ColourTableEntry {
            name: String::from(name),
            colour,
        };
        assert_eq!(
            read.colours.entries(),
            vec![
                entry("sui test lime", rgb(0, 255, 0)),
                entry("sui test purple", rgb(128, 0, 128)),
                entry("sui test teal", rgb(0, 128, 128)),
            ]
        );
        assert!(read.map == *bench.dialog.buffer());
    }

    #[test]
    fn exported_codes_keep_their_order_where_sections_interleave() {
        // The fixture's rules are grouped by section as it is written - seven feature rules, three
        // symbol rules, one of attributes - so a writer that gathered rules by section would give
        // that very order back. AC*'s symbol rule (#7) is moved up to #2, between two feature
        // rules: order is precedence, and the file must hold the rules as the buffer does.
        let scratch = Scratch::new("codes-interleaved");
        let mut bench = manager(fixture_host());
        assert!(bench.dialog.move_rule(&mut bench.host, 7, -5).is_ok());
        assert_eq!(bench.dialog.buffer().rules()[1].section, SurveySection::Map);
        assert_eq!(
            bench.dialog.buffer().rules()[2].section,
            SurveySection::VertexSymbol
        );
        assert_eq!(bench.dialog.buffer().rules()[2].key, "AC*");
        assert_eq!(bench.dialog.buffer().rules()[3].section, SurveySection::Map);
        assert!(bench.dialog.apply(&mut bench.host).is_ok());
        let path = scratch.path("interleaved.customisation.json");
        assert!(bench.dialog.export_codes(&mut bench.host, &path).is_ok());

        let bytes = read_bytes(&path);
        // Three lines of the fixture file itself, in the buffer's order now: KB*'s feature rule,
        // AC*'s symbol rule, AC*'s feature rule.
        let lines = concat!(
            "    {\"key\": \"KB*\", \"sets\": \"feature\", \"layer\": \"TEST ROADS\", ",
            "\"colour\": \"sui test purple\", \"draw\": \"line\", ",
            "\"linestyle\": \"TEST Dashed Kerb\", \"weight\": \"0\", \"group\": \"TEST - ROADS\", ",
            "\"comment\": \"[KB*] Kerb, in a colour no table knows\"},\n",
            "    {\"key\": \"AC*\", \"sets\": \"symbol\", \"comment\": \"[AC*] Access chamber\", ",
            "\"hide\": false, \"symbol\": {\"name\": \"TEST Survey Mark\", \"colour\": \"white\", ",
            "\"size\": 1.5}},\n",
            "    {\"key\": \"AC*\", \"sets\": \"feature\", \"layer\": \"TEST FURNITURE\", ",
            "\"colour\": \"white\", \"draw\": \"point\", \"linestyle\": \"0\", \"weight\": \"0\", ",
            "\"group\": \"TEST - FURNITURE\", \"comment\": \"[AC*] Access chamber\"},\n"
        );
        assert!(bytes.contains(lines), "{bytes}");
        let read = customisation_from_json(bytes.as_bytes()).expect("the file reads");
        assert!(read.map == *bench.dialog.buffer());
    }

    #[test]
    fn a_session_with_no_name_exports_its_codes_under_the_files_own_name() {
        // A drawing nothing was loaded into: its session has no name, and a customisation file
        // must have one. It is then the file's, without the format's `.customisation.json`.
        let scratch = Scratch::new("codes-no-name");
        let mut document = Document::new(None);
        let mut map = SurveyMap::new();
        assert!(
            map.add(SurveyRule {
                key: String::from("KT*"),
                model: String::from("TEST KERBS"),
                ..SurveyRule::default()
            })
            .is_ok()
        );
        document.set_survey_map(map.clone());
        assert_eq!(document.customisation_state().name, "");
        let mut bench = manager_with(interpreter_host(document), CodesContext::default());
        let path = scratch.path("site codes.customisation.json");
        assert!(bench.dialog.export_codes(&mut bench.host, &path).is_ok());
        // The session has no name, so the line gives the file's own.
        assert_eq!(
            bench.lines(),
            vec![format!(
                "CUSTOMISE EXPORT {} CODES NAME \"site codes\"",
                quoted_file(&path)
            )]
        );
        let read = customisation_from_json(read_bytes(&path).as_bytes()).expect("the file reads");
        assert_eq!(read.name, "site codes");
        assert!(read.map == map);
    }

    #[test]
    fn exporting_rules_that_are_not_on_the_drawing_is_refused_and_writes_nothing() {
        // The line CUSTOMISE EXPORT writes the drawing's rules, and no line can name the buffer:
        // with an edit not applied the export says to Apply or Revert, runs no line, and leaves a
        // file that was there as it was.
        let scratch = Scratch::new("codes-unapplied");
        let mut bench = manager(fixture_host());
        edit_water_main_layer(&mut bench, "TEST WATER");
        assert!(bench.dialog.dirty());
        let path = scratch.file("unapplied.customisation.json", "the file that was there");
        bench.host.take_lines();
        let error = bench
            .dialog
            .export_codes(&mut bench.host, &path)
            .expect_err("the export is refused");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(
            error.message.contains("Apply or Revert the edits first"),
            "{}",
            error.message
        );
        assert!(bench.lines().is_empty());
        assert_eq!(read_bytes(&path), "the file that was there");

        // Reverted, the buffer is the drawing's again and the export goes.
        bench.dialog.revert(&mut bench.host);
        assert!(!bench.dialog.dirty());
        assert!(bench.dialog.export_codes(&mut bench.host, &path).is_ok());
        assert_ne!(read_bytes(&path), "the file that was there");
    }

    #[test]
    fn an_export_that_cannot_be_written_is_refused_by_the_verb_and_leaves_nothing_beside() {
        // One writer, the verb's: it writes beside the file and renames, so a name that is a
        // folder is refused with the folder standing and nothing left. The refusal is the verb's
        // own words, with the path.
        let scratch = Scratch::new("codes-folder");
        let mut bench = manager(fixture_host());
        let folder = scratch.folder("a_folder_not_a_file");
        let error = bench
            .dialog
            .export_codes(&mut bench.host, &folder)
            .expect_err("a folder is no file to write");
        assert_eq!(error.code, ErrorCode::FileExportFailure);
        assert!(folder.is_dir());
        let beside = fs::read_dir(&scratch.root)
            .expect("the scratch folder is listed")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("a_folder_not_a_file")
            })
            .count();
        assert_eq!(beside, 1, "only the folder itself");
    }

    #[test]
    fn exporting_with_no_command_line_says_so_and_does_not_write_the_file_itself() {
        let scratch = Scratch::new("codes-no-runner");
        let mut bench = manager_with(fixture_host(), context().without_command_line());
        let path = scratch.path("no_runner.customisation.json");
        let error = bench
            .dialog
            .export_codes(&mut bench.host, &path)
            .expect_err("there is no command line");
        assert!(
            error.message.contains("without the window's command line"),
            "{}",
            error.message
        );
        assert!(!path.exists());
    }

    #[test]
    fn the_code_list_export_is_the_buffers_csv() {
        let scratch = Scratch::new("codes-csv");
        let mut bench = manager(fixture_host());
        let path = scratch.path("codes.csv");
        assert!(
            bench
                .dialog
                .export_code_list(&mut bench.host, &path)
                .is_ok()
        );
        let csv = read_bytes(&path);
        assert_eq!(csv, code_list_csv(bench.dialog.buffer()));
        // WM*'s record, worked from rules #0 and #10: the `*` rule sets none of these columns.
        assert!(
            csv.contains(
                "WM*,[WM*] Water main,TEST - SERVICES,TEST SERVICES,blue,line,TEST Water \
                 Main,,,\r\n"
            ),
            "{csv}"
        );
    }

    #[test]
    fn import_merges_a_customisations_rules_into_the_buffer_and_not_into_the_document() {
        let mut host = fixture_host();
        host.document_mut().set_survey_map(SurveyMap::new());
        let mut bench = manager(host);
        assert!(
            bench
                .dialog
                .import_codes(
                    &mut bench.host,
                    &customisation_fixture_file("test_survey"),
                    LoadMode::Merge
                )
                .is_ok()
        );
        assert_eq!(bench.dialog.buffer().size(), 11);
        assert!(bench.document().survey_map().empty());
        assert!(bench.dialog.dirty());
        // A key once for each section it has rules in: 7 feature keys, 3 symbol keys and the one
        // attributes key. The load is named by its customisation, "test_survey", and the word of
        // the mode.
        assert!(bench.logged_containing("test_survey (merge): 11 added, 0 replaced"));
    }

    #[test]
    fn import_takes_a_files_rules_and_leaves_its_definitions_colours_and_settings_alone() {
        let scratch = Scratch::new("codes-extra");
        let path = scratch.file("extra.customisation.json", EXTRA_CUSTOMISATION);
        let host = fixture_host();
        let library = host.document().library_generation();
        let mut bench = manager(host);

        // Merged: ZZ* is a key the fixture's 11 rules do not have, so it follows them.
        assert!(
            bench
                .dialog
                .import_codes(&mut bench.host, &path, LoadMode::Merge)
                .is_ok()
        );
        assert_eq!(bench.dialog.buffer().size(), 12);
        assert_eq!(bench.dialog.buffer().rules()[11].key, "ZZ*");
        assert_eq!(bench.dialog.buffer().rules()[11].model, "TEST MISC");
        assert!(bench.dialog.dirty());
        assert!(bench.logged_containing("extra (merge): 1 added, 0 replaced"));
        // Nothing else the file holds reached the drawing: not its symbol, its colour or its
        // spelling of Start, and not yet its rule either.
        assert_eq!(bench.document().survey_map().size(), 11);
        assert_eq!(bench.document().style_library().size(), 7);
        assert!(!bench.document().style_library().contains("TEST Extra Mark"));
        assert!(bench.document().customisation_state().colours.empty());
        assert_eq!(bench.document().customisation_state().linework.start, "ST");
        assert_eq!(bench.document().library_generation(), library);
        // ... and the log says what was left, each kind with its count.
        assert!(bench.logged_containing(
            "Not imported from extra.customisation.json, as this manager edits the survey code \
             rules: definitions (1), colours (1), the linework codes."
        ));

        // Replace, as the box beside the button asks for: the file's rules are the whole buffer.
        // ZZ* was there already (replaced), and the fixture's 8 keys are what went.
        bench.host.parts.logged.clear();
        assert!(
            bench
                .dialog
                .import_codes(&mut bench.host, &path, LoadMode::Replace)
                .is_ok()
        );
        assert_eq!(bench.dialog.buffer().size(), 1);
        assert_eq!(bench.dialog.buffer().rules()[0].key, "ZZ*");
        assert!(bench.logged_containing("extra (replace): 0 added, 1 replaced"));
        assert!(bench.logged_containing("8 keys removed by Replace."));
        assert_eq!(bench.document().survey_map().size(), 11);
    }

    #[test]
    fn import_replaces_a_keys_rules_in_a_section_where_they_stand_and_keeps_its_others() {
        // A file that gives rules to two keys the buffer HAS and to one it has not. A merge is by
        // key and section (cad's customisation_merge): the rules a load gives a key in a section
        // take the place of that key's rules in that section, going in ahead of the key's rules
        // it leaves standing, at the first of them; a key the buffer lacks follows. Rules merely
        // added to the end would leave 14, with the buffer's own KB* and AC* rules still ahead of
        // the file's and so still deciding.
        let scratch = Scratch::new("codes-kerbs");
        let path = scratch.file(
            "kerbs.customisation.json",
            r#"{
  "format": "katana-customisation",
  "version": 1,
  "name": "kerbs",
  "codes": [
    {"key": "KB*", "sets": "feature", "layer": "TEST KERBS", "draw": "line"},
    {"key": "AC*", "sets": "symbol", "symbol": {"name": "TEST Valve", "size": 2}},
    {"key": "ZZ*", "sets": "feature", "layer": "TEST MISC"}
  ]
}
"#,
        );
        let mut bench = manager(fixture_host());
        assert!(
            bench
                .dialog
                .import_codes(&mut bench.host, &path, LoadMode::Merge)
                .is_ok()
        );

        // The fixture's 11, less its KB* feature rule (#1) and its AC* symbol rule (#7), with the
        // file's three: 12.
        let expected = [
            ("WM*", SurveySection::Map),
            ("KB*", SurveySection::Map), // the file's, where the buffer's stood
            ("AC*", SurveySection::VertexSymbol), // the file's, ahead of AC*'s feature rule
            ("AC*", SurveySection::Map), // the buffer's own, left standing
            ("TR*", SurveySection::Map),
            ("1*", SurveySection::Map),
            ("2*", SurveySection::Map),
            ("PX*", SurveySection::Map),
            ("TR*", SurveySection::VertexSymbol),
            ("PX*", SurveySection::VertexSymbol),
            ("*", SurveySection::StringAttribute),
            ("ZZ*", SurveySection::Map), // a key the buffer did not have
        ];
        let rules = bench.dialog.buffer().rules();
        assert_eq!(rules.len(), expected.len());
        for (index, (key, section)) in expected.iter().enumerate() {
            assert_eq!(rules[index].key, *key, "#{index}");
            assert_eq!(rules[index].section, *section, "#{index}");
        }
        // Replaced whole, not field by field: the file's KB* rule gives no colour, and the
        // buffer's "sui test purple" has gone with its rule.
        assert_eq!(rules[1].model, "TEST KERBS");
        assert_eq!(rules[1].colour, "");
        assert_eq!(rules[1].linestyle, "");
        let valve = rules[2].symbol.clone().expect("the rule has a symbol");
        assert_eq!(valve.style, "TEST Valve");
        assert_eq!(valve.size, 2.0);
        assert_eq!(rules[3].model, "TEST FURNITURE");
        assert_eq!(rules[11].model, "TEST MISC");
        // So AC01 is now drawn with the valve, and a kerb goes to the new layer.
        assert_eq!(
            bench
                .dialog
                .buffer()
                .lookup("AC01")
                .resolved
                .symbol
                .unwrap_or_default()
                .style,
            "TEST Valve"
        );
        assert_eq!(
            bench.dialog.buffer().lookup("KB01").resolved.model,
            "TEST KERBS"
        );
        assert!(bench.logged_containing("kerbs (merge): 1 added, 2 replaced"));
        // For review: the drawing still has its own 11.
        assert!(bench.dialog.dirty());
        assert_eq!(bench.document().survey_map().size(), 11);
        assert_eq!(
            bench.document().survey_map().lookup("KB01").resolved.model,
            "TEST ROADS"
        );
    }

    #[test]
    fn import_refuses_what_is_not_a_katana_customisation_or_holds_no_rules_and_keeps_the_buffer()
    {
        let scratch = Scratch::new("codes-refused");
        let mut bench = manager(fixture_host());
        let before = bench.dialog.buffer().clone();

        // A file of another format - here a definition as the older style libraries wrote one -
        // is not JSON, and is told what it is not.
        let other = scratch.file(
            "older_library.txt",
            "worldstyle \"TEST Old\" {\n  move 0 0\n  draw 1 0\n}\n",
        );
        let refused = bench
            .dialog
            .import_codes(&mut bench.host, &other, LoadMode::Merge)
            .expect_err("another format is refused");
        assert_eq!(refused.code, ErrorCode::ParseFailure);
        assert!(
            refused.message.contains("not a Katana customisation file"),
            "{}",
            refused.describe()
        );

        // A customisation of symbols alone holds nothing for this manager.
        let no_rules = bench
            .dialog
            .import_codes(
                &mut bench.host,
                &customisation_fixture_file("test_symbols"),
                LoadMode::Merge,
            )
            .expect_err("a file with no rules is refused");
        assert_eq!(no_rules.code, ErrorCode::NotFound);
        assert_eq!(no_rules.message, "the file holds no survey code rules");

        // A file that is not there.
        let missing = scratch.path("no_such.customisation.json");
        let absent = bench
            .dialog
            .import_codes(&mut bench.host, &missing, LoadMode::Merge)
            .expect_err("a file that is not there is refused");
        assert_eq!(absent.code, ErrorCode::NotFound);

        assert!(*bench.dialog.buffer() == before);
        assert!(!bench.dialog.dirty());
    }

    #[test]
    fn a_colour_the_customisation_defines_is_offered_swatched_explained_and_no_longer_an_issue()
    {
        let mut bench = manager(fixture_host());
        // KB* (#1) is coloured "sui test purple", which is not a standard name. Until the
        // session's customisation defines it, the fields offer no colour and then the 27 standard
        // names, and it is an issue.
        let standard = standard_colour_names();
        assert_eq!(standard.len(), 27);
        assert_eq!(bench.items("ruleColour").len(), 1 + 27);
        assert_eq!(bench.rows("issuesTable"), 2);
        bench.dialog.select_rule(&mut bench.host, 1);
        bench.draw();
        assert_eq!(bench.value("ruleColour"), "sui test purple");

        // The customisation's own colours change while the manager is open: purple, 128 0 128.
        bench.document_mut().set_colour_table(purple());
        bench.draw();

        // OFFERED, in each of the three colour fields: the customisation's name after the empty
        // first entry and ahead of the standard ones.
        for name in ["ruleColour", "ruleSymbolColour", "ruleTextColour"] {
            let items = bench.items(name);
            assert_eq!(items.len(), 1 + 1 + 27, "{name}");
            assert_eq!(items[0], "", "{name}");
            assert_eq!(items[1], "sui test purple", "{name}");
            assert_eq!(Some(&items[2]), standard.first(), "{name}");
            assert_eq!(Some(&items[28]), standard.last(), "{name}");
        }
        // What the form showed is still what it shows, and the rule is untouched.
        assert_eq!(bench.value("ruleColour"), "sui test purple");
        assert!(!bench.dialog.dirty());

        // DRAWN: the Code Table swatches it where it said "(no RGB)" ...
        let kerb = key_row(&bench, "KB*").expect("KB* is listed");
        assert_eq!(kerb.cells[3], "sui test purple");
        assert!(kerb.pictured[3]);
        // ... the explanation gives its RGB ...
        bench.fill("testCode", "KB01");
        let definitions = bench.text("explainDefinitions");
        assert!(
            definitions
                .split('\n')
                .any(|line| line == "Colour sui test purple: #800080"),
            "{definitions}"
        );
        // ... and of the two issues only PX*'s undefined symbol (#9) is left.
        assert_eq!(bench.rows("issuesTable"), 1);
        assert_eq!(bench.row("issuesTable", 0)[1], "#9");

        // A manager opened afterwards lists it from the start.
        let later = manager(bench.host);
        assert_eq!(later.items("ruleColour")[1], "sui test purple");
    }

    // ---- the customisation's colours where the manager ACTS on them -----------------------------
    //
    // The test above reaches the fields, the table and the explanation. These reach the three
    // other places a colour name is resolved: the form's own issues, Apply Codes and Linework.
    // "sui test purple" is 128 0 128, a colour only the session's customisation gives that name -
    // the standard names do not know it - so each fails where the standard names alone are asked.

    #[test]
    fn the_forms_own_issues_know_a_colour_the_customisation_defines() {
        let mut bench = manager(fixture_host());
        // KB* (#1) in the form: its one issue is its colour, which nothing defines yet (of the
        // fixture's two issues the other is #9's symbol).
        bench.dialog.select_rule(&mut bench.host, 1);
        bench.draw();
        let issues = bench.text("ruleIssues");
        assert!(issues.contains("sui test purple"), "{issues}");
        assert_ne!(issues, "No issues with this rule.");

        bench.document_mut().set_colour_table(purple());
        bench.draw();
        assert_eq!(bench.dialog.current_rule(), Some(1));
        assert_eq!(bench.text("ruleIssues"), "No issues with this rule.");
    }

    #[test]
    fn apply_codes_colours_a_style_by_the_customisations_own_colour() {
        let mut host = fixture_host();
        host.document_mut().set_colour_table(purple());
        let kerb = add_point(&mut host, 0.0, "KB01", "1");
        let mut bench = manager(host);

        bench.press("applyExecute");

        // KB* gives the linestyle TEST Dashed Kerb and the colour "sui test purple". No style
        // draws that yet, so one is made, named after the linestyle (cad's survey_coding), in the
        // colour the name means HERE.
        let coded = bench
            .document()
            .model()
            .entities
            .find(kerb)
            .expect("the point is there");
        assert_eq!(coded.layer, "TEST ROADS");
        assert_eq!(coded.style, "TEST Dashed Kerb");
        let style = bench
            .document()
            .model()
            .styles
            .find("TEST Dashed Kerb")
            .expect("the style is made");
        assert_eq!(style.color, Some(rgb(128, 0, 128)));
    }

    #[test]
    fn linework_colours_its_line_by_the_customisations_own_colour() {
        let mut host = fixture_host();
        host.document_mut().set_colour_table(purple());
        // KB* is a Line code: three points of string KB01 make one line.
        add_point(&mut host, 0.0, "KB01", "1");
        add_point(&mut host, 5.0, "KB01", "2");
        add_point(&mut host, 10.0, "KB01", "3");
        let mut bench = manager(host);

        bench.press("lineworkExecute");

        // The one line Linework drew, styled as its code is: the style made for it carries the
        // colour the customisation gives the rule's name.
        let lines = polylines(bench.document());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].layer, "TEST ROADS");
        assert_eq!(lines[0].style, "TEST Dashed Kerb");
        let style = bench
            .document()
            .model()
            .styles
            .find("TEST Dashed Kerb")
            .expect("the style is made");
        assert_eq!(style.color, Some(rgb(128, 0, 128)));
    }

    #[test]
    fn closing_headless_with_unapplied_edits_asks_nothing_and_keeps_them() {
        let mut bench = manager(fixture_host());
        edit_water_main_layer(&mut bench, "TEST WATER");
        bench.press("closeManager");
        assert!(!bench.dialog.is_open());
        assert!(bench.dialog.dirty());
        assert_eq!(bench.dialog.buffer().rules()[0].model, "TEST WATER");
        assert!(bench.logged_containing("unapplied edits"));
    }

    #[test]
    fn a_map_loaded_elsewhere_is_taken_up_when_the_buffer_is_unedited() {
        let mut bench = manager(fixture_host());
        let replacement = elsewhere();
        bench.document_mut().set_survey_map(replacement.clone());
        bench.draw();
        assert!(*bench.dialog.buffer() == replacement);
        assert!(!bench.dialog.dirty());
    }

    #[test]
    fn edits_kept_over_a_map_changed_elsewhere_stay_unapplied_when_undone() {
        let mut bench = manager(fixture_host());
        let original = bench.dialog.buffer().clone();
        // An edit (12 rules), then the drawing takes a one-rule map from elsewhere, then the edit
        // is undone: the buffer is the fixture's 11 rules again. The drawing holds 1 rule, so the
        // buffer still differs from what the drawing has - Apply and Revert both still have work
        // to do, and the two tabs that run the drawing's map still say so.
        assert!(bench.dialog.duplicate_rule(&mut bench.host, 1).is_ok());
        let replacement = elsewhere();
        bench.document_mut().set_survey_map(replacement.clone());
        bench.draw();
        assert!(bench.logged_containing("changed elsewhere"));
        assert!(bench.dialog.remove_rule(&mut bench.host, 2).is_ok());
        assert!(*bench.dialog.buffer() == original);
        assert_eq!(bench.document().survey_map().size(), 1);
        bench.draw();

        assert!(bench.dialog.dirty());
        assert!(bench.enabled("applyMap"));
        assert!(bench.enabled("revertMap"));
        assert!(bench.text("dirtyIndicator").starts_with("Unapplied edits"));
        assert!(bench.visible("applyDirtyNote"));
        assert!(bench.visible("lineworkDirtyNote"));

        // Revert now takes the drawing's one rule, and nothing is left unapplied.
        bench.press("revertMap");
        assert!(*bench.dialog.buffer() == replacement);
        assert!(!bench.dialog.dirty());
    }

    #[test]
    fn the_dialog_outlives_its_document_and_then_refuses_to_apply() {
        let mut bench = manager_with(
            interpreter_host(Document::new(None)),
            CodesContext::default(),
        );
        bench.close_drawing();
        assert!(bench.dialog.apply(&mut bench.host).is_err());
        bench.fill("testCode", "WM01");
        bench.press("applyPreview");
        bench.press("lineworkPreview");
        bench.press("ruleNew");
        bench.draw();
    }

    #[test]
    fn once_its_document_has_gone_the_form_saves_each_rule_with_its_own_names() {
        let mut bench = manager_with(fixture_host(), CodesContext::default());
        let original = bench.dialog.buffer().clone();
        // While the drawing is open the pickers are loaded last with #7's symbol ("TEST Survey
        // Mark") and then #0's linestyle ("TEST Water Main"). Once it has gone they can no longer
        // be loaded (they read its library).
        bench.dialog.select_rule(&mut bench.host, 7);
        bench.dialog.select_rule(&mut bench.host, 0);
        bench.close_drawing();

        // Saved with no edit, a rule is itself: #4 (1* feature) keeps linestyle "0" and #8 (TR*
        // symbol) symbol "TEST Tree" - from the fixture's text - not the names the pickers were
        // last loaded with.
        for index in [4, 8] {
            bench.dialog.select_rule(&mut bench.host, index);
            bench.draw();
            assert!(bench.enabled("ruleUpdate"));
            bench.press("ruleUpdate");
            assert!(
                *bench.dialog.buffer() == original,
                "after saving #{index}"
            );
        }
        assert_eq!(
            bench.dialog.buffer().at(4).expect("rule 4").linestyle,
            "0"
        );
        assert_eq!(
            bench
                .dialog
                .buffer()
                .at(8)
                .expect("rule 8")
                .symbol
                .unwrap_or_default()
                .style,
            "TEST Tree"
        );
        assert!(!bench.dialog.dirty());

        // What a picker shows is what Save writes, and it cannot be changed: #8 was loaded last,
        // so the symbol picker shows "TEST Tree".
        assert!(!bench.enabled("ruleSymbol"));
        assert_eq!(bench.value("ruleSymbol"), "TEST Tree");

        // A new rule has no linestyle, whatever the picker was last loaded with.
        bench.press("ruleNew");
        let fresh = bench.dialog.form_rule().expect("the form reads");
        assert_eq!(fresh.linestyle, "");
    }

    #[test]
    fn a_typed_colour_name_is_kept_as_written_listed_in_another_case_or_not_at_all() {
        let mut bench = manager(fixture_host());
        bench.dialog.select_rule(&mut bench.host, 0);
        bench.draw();
        // "Blue" is listed only as "blue": Enter must not fold it into the listed name, because a
        // map keeps names as written.
        for typed in ["Blue", "sui water potable"] {
            bench.fill("ruleColour", typed);
            bench
                .harness
                .key(egui::Key::Enter, egui::Modifiers::NONE);
            bench.draw();
            let rule = bench.dialog.form_rule().expect("the form reads");
            assert_eq!(rule.colour, typed);
        }
    }

    #[test]
    fn enter_in_a_field_presses_no_button() {
        let mut bench = manager(fixture_host());
        // Every field a person types a name into and confirms with Enter: none may reach a
        // button - Import Codes would ask for a file, and Delete or Apply would edit. Each field
        // is clicked, so it has the keys, and Enter pressed in it; the Linework tab is brought to
        // the front for its field first.
        for name in ["testCode", "ruleModel", "ruleKey", "lineworkStart"] {
            if name == "lineworkStart" {
                bench.fill("codeManagerTabs", "Linework");
            }
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
        assert_eq!(bench.dialog.buffer().size(), 11);
    }

    #[test]
    fn double_clicking_an_explained_field_opens_the_rule_that_set_it() {
        let mut bench = manager(fixture_host());
        bench.fill("testCode", "AC01");
        // AC01's symbol comes from #7, AC* symbol.
        let fields = bench.dialog.explained_fields();
        let symbol = fields
            .iter()
            .position(|row| row[0] == "symbol")
            .expect("the symbol is explained");
        assert_eq!(fields[symbol][2], "#7 AC* (symbol)");
        bench.dialog.activate_explained_field(symbol);
        // The jump waits for the click to return (it rebuilds this list).
        assert!(bench.dialog.current_rule().is_none());
        bench.draw();
        assert_eq!(bench.dialog.current_rule(), Some(7));
        assert_eq!(bench.value("ruleSymbolSize"), "1.5");
    }
}
