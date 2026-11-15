//! What a bare CUSTOMISE says (customisation_report): the numbers of the summary, and the verb's
//! reply made from one - the count lines, the records, the coverage. Counted by hand from what
//! each test loads (tests/cad/customisation/test_customisation_report.cpp).
//!
//! The four tests of an older prose text stood first there ("Loaded files, in load order: ...",
//! cad::customisationReport). They went with that text on 2026-10-07, when the last front end that
//! printed it took the verb's reply.

use katana_cad::Document;
use katana_cad::customisation_record::CustomisationSource;
use katana_cad::customisation_report::{
    CustomisationSummary, customisation_state_record, customisation_summary,
    customisation_summary_of, format_customisation_reply, undefined_rule_names,
};
use katana_cad::customisation_state::CustomisationOrigin;
use katana_entity::style_library::add_or_replace;
use katana_entity::{
    Color, ColourTable, CustomisationAutomation, EnumName, LineStyle, LineworkCodes, StyleLibrary,
    SurveyMap, SurveyRule, SurveySection, SurveySymbol,
};

fn definition(name: &str, group: &str, at_vertices: bool, source: &str) -> LineStyle {
    LineStyle {
        name: String::from(name),
        group: String::from(group),
        at_vertices,
        source: String::from(source),
        ..LineStyle::default()
    }
}

// Two definitions in two groups: a fence drawn along a line, from a file whose name does not say
// symbol (a linestyle only), and a pit drawn at vertices (a symbol only, decision D3). Two survey
// code rules over one code.
fn load(document: &mut Document) {
    let mut library = StyleLibrary::default();
    assert!(
        add_or_replace(
            &mut library,
            definition("TEST Fence", "FENCES", false, "lines.4d")
        )
        .is_ok()
    );
    assert!(
        add_or_replace(
            &mut library,
            definition("TEST Pit", "PITS", true, "pits.4d")
        )
        .is_ok()
    );
    document.set_style_library(library);
    let mut map = SurveyMap::new();
    let fence = SurveyRule {
        key: String::from("FE*"),
        model: String::from("FENCES"),
        linestyle: String::from("TEST Fence"),
        ..SurveyRule::default()
    };
    assert!(map.add(fence.clone()).is_ok());
    let again = SurveyRule {
        colour: String::from("red"),
        ..fence
    };
    assert!(map.add(again).is_ok());
    document.set_survey_map(map);
}

fn source(name: &str, definitions: bool, rules: bool) -> CustomisationSource {
    CustomisationSource {
        name: String::from(name),
        definitions,
        rules,
        notice: Vec::new(),
    }
}

// {name, brought definitions, brought rules}: two style libraries and a survey code file.
fn loaded() -> Vec<CustomisationSource> {
    vec![
        source("lines.4d", true, false),
        source("pits.4d", true, false),
        source("site codes.mapfile", false, true),
    ]
}

mod customisation_report {
    use super::*;

    #[test]
    fn the_summary_is_the_numbers_the_text_is_made_from() {
        let mut document = Document::default();
        load(&mut document);
        let summary =
            customisation_summary_of(&document, &loaded(), &[String::from("old symbols.4d")]);
        assert_eq!(summary.definitions, 2);
        assert_eq!(summary.groups, 2);
        assert_eq!(summary.symbols, 1);
        assert_eq!(summary.linestyles, 1);
        assert_eq!(summary.rules, 2);
        assert_eq!(summary.codes, 1);
        assert_eq!(summary.loaded, loaded());
        assert_eq!(summary.not_loaded, vec![String::from("old symbols.4d")]);
        assert_eq!(summary.coverage.styles, 0);
    }

    // ---- the verb's reply (format_customisation_reply) ---------------------------------
    //
    // What the interpreter's CUSTOMISE replies: the two count lines above, then
    // records. test_customisation_verbs drives it through the verb; here it is
    // given a summary written out by hand, which is the only way to show it a
    // name the open project is missing without a project on disk.

    #[test]
    fn the_verbs_reply_is_the_counts_then_records_of_the_session_then_the_coverage() {
        let summary = CustomisationSummary {
            definitions: 2,
            groups: 2,
            symbols: 1,
            linestyles: 1,
            rules: 2,
            codes: 1,
            loaded: vec![
                source("Site Styles", true, false),
                source("Site codes", false, true),
            ],
            not_loaded: vec![String::from("old symbols"), String::from("plain")],
            name: String::from("Site Styles"),
            origin: CustomisationOrigin::Loaded,
            kept: false,
            colours: 3,
            automation: CustomisationAutomation {
                linework_on_survey_import: false,
                ..CustomisationAutomation::default()
            },
            // That control is off.
            linework: LineworkCodes {
                join: String::new(),
                ..LineworkCodes::default()
            },
            ..CustomisationSummary::default()
        };
        assert_eq!(
            format_customisation_reply(&summary),
            "2 linestyle and symbol definitions in 2 groups: 1 offered as symbols, 1 as \
             linestyles (one definition can be both)\n2 survey code rules over 1 distinct \
             code\ncustomisation name=\"Site Styles\" origin=loaded kept=no definitions=2 \
             codes=1 rules=2 colours=3\nsource name=\"Site Styles\" definitions=yes rules=no\n\
             source name=\"Site codes\" definitions=no rules=yes\nautomation auto.codes=on \
             auto.linework=off\nlinework linework.start=ST linework.end=END linework.close=CL \
             linework.arcstart=BC linework.arcend=EC linework.join=\"\" \
             linework.rectangle=RECT\nmissing name=\"old symbols\"\nmissing name=plain\nThis \
             drawing has no styles yet; import a drawing or survey that carries styles, or make \
             one in Format > Styles and Linetypes or with STYLE NEW, to see the customisation \
             take effect.\n"
        );
        // The one record a verb that changed the session ends its reply with.
        assert_eq!(
            customisation_state_record(&summary),
            "customisation name=\"Site Styles\" origin=loaded kept=no definitions=2 codes=1 \
             rules=2 colours=3"
        );
    }

    #[test]
    fn the_verbs_reply_of_nothing_keeps_what_the_project_is_missing_and_names_no_file_kind() {
        let summary = CustomisationSummary {
            not_loaded: vec![String::from("old symbols")],
            ..CustomisationSummary::default()
        };
        let reply = format_customisation_reply(&summary);
        assert_eq!(
            reply,
            "No customisation is loaded.\n  CUSTOMISE <file> [<file>...]  loads Katana \
             customisation files\ncustomisation name=\"\" origin=none kept=no definitions=0 \
             codes=0 rules=0 colours=0\nautomation auto.codes=on auto.linework=on\nlinework \
             linework.start=ST linework.end=END linework.close=CL linework.arcstart=BC \
             linework.arcend=EC linework.join=JPN linework.rectangle=RECT\nmissing name=\"old \
             symbols\"\n"
        );
        // The kinds of file another program wrote are not what loads any more.
        assert!(!reply.contains(".4d"));
        assert!(!reply.contains(".mapfile"));
    }

    #[test]
    fn the_verbs_reply_never_says_no_customisation_is_loaded_of_one_that_is() {
        // A customisation that brought neither kind: a name and a table of two
        // colours, loaded. "No customisation is loaded." above the record that
        // names it contradicted the next line. What is true is that it holds
        // nothing that draws - and there is still no coverage to report of that.
        let mut summary = CustomisationSummary {
            name: String::from("Site colours"),
            origin: CustomisationOrigin::Loaded,
            colours: 2,
            loaded: vec![source("Site colours", false, false)],
            ..CustomisationSummary::default()
        };
        let reply = format_customisation_reply(&summary);
        assert_eq!(
            reply,
            "No linestyle or symbol definitions and no survey code rules are loaded.\n  \
             CUSTOMISE <file> [<file>...]  loads Katana customisation files\ncustomisation \
             name=\"Site colours\" origin=loaded kept=no definitions=0 codes=0 rules=0 \
             colours=2\nsource name=\"Site colours\" definitions=no rules=no\nautomation \
             auto.codes=on auto.linework=on\nlinework linework.start=ST linework.end=END \
             linework.close=CL linework.arcstart=BC linework.arcend=EC linework.join=JPN \
             linework.rectangle=RECT\n"
        );
        assert!(!reply.contains("No customisation is loaded"));
        // Every origin but "none" is a customisation that is there.
        for origin in [
            CustomisationOrigin::BuiltIn,
            CustomisationOrigin::Kept,
            CustomisationOrigin::Edited,
        ] {
            summary.origin = origin;
            assert!(
                format_customisation_reply(&summary).starts_with(
                    "No linestyle or symbol definitions and no survey code rules are loaded.\n"
                ),
                "{}",
                origin.name()
            );
        }
    }

    #[test]
    fn the_summary_of_a_session_reads_its_state_from_the_document() {
        // The raw setters, as an editor calls them: an edited session with no
        // name and no source, the switches and codes as they were set.
        let mut document = Document::default();
        load(&mut document);
        document.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: true,
        });
        let codes = LineworkCodes {
            start: String::from("S"),
            ..LineworkCodes::default()
        };
        assert!(document.set_linework_codes(codes).is_ok());
        let mut colours = ColourTable::default();
        assert!(colours.add("site orange", Color::new(0, 0, 0, 255)).is_ok());
        document.set_colour_table(colours);

        let summary = customisation_summary(&document);
        assert_eq!(summary.definitions, 2);
        assert_eq!(summary.rules, 2);
        assert_eq!(summary.codes, 1);
        assert_eq!(summary.name, "");
        assert_eq!(summary.origin, CustomisationOrigin::Edited);
        assert!(!summary.kept);
        assert_eq!(summary.colours, 1);
        assert!(!summary.automation.codes_on_survey_import);
        assert!(summary.automation.linework_on_survey_import);
        assert_eq!(summary.linework.start, "S");
        assert!(summary.loaded.is_empty());
        assert!(summary.not_loaded.is_empty());
    }

    #[test]
    fn the_names_the_rules_ask_for_that_nothing_defines_are_each_listed_once() {
        // load() gives a rule naming "TEST Fence", which the library defines.
        // Added here: a linestyle and a symbol nothing defines (the linestyle
        // named by two rules), a plain line, and a symbol Katana draws itself -
        // neither of which needs a definition.
        let mut document = Document::default();
        load(&mut document);
        let mut map = document.survey_map().clone();
        let mut feature = |key: &str, linestyle: &str| {
            let rule = SurveyRule {
                key: String::from(key),
                model: String::from("SOMEWHERE"),
                linestyle: String::from(linestyle),
                ..SurveyRule::default()
            };
            assert!(map.add(rule).is_ok());
        };
        feature("A1*", "ZZ Missing");
        feature("A2*", "ZZ Missing");
        feature("A3*", "0");
        let mut symbol = |key: &str, name: &str| {
            let rule = SurveyRule {
                key: String::from(key),
                section: SurveySection::VertexSymbol,
                symbol: Some(SurveySymbol {
                    style: String::from(name),
                    colour: String::new(),
                    size: 1.0,
                    rotation: 0.0,
                    offset: 0.0,
                    raise: 0.0,
                }),
                ..SurveyRule::default()
            };
            assert!(map.add(rule).is_ok());
        };
        symbol("B1*", "AA Missing");
        symbol("B2*", "cross");
        document.set_survey_map(map);
        assert_eq!(
            undefined_rule_names(&document),
            vec![String::from("AA Missing"), String::from("ZZ Missing")]
        );

        let empty = Document::default();
        assert!(undefined_rule_names(&empty).is_empty());
    }
}
