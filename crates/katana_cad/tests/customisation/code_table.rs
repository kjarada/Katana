//! The survey code library questioned: why a code gets what it gets, the map as a table of codes,
//! the codes a drawing carries, and what is wrong with a map - and the one report text every front
//! end prints (tests/cad/customisation/test_code_table.cpp).
//!
//! A rule's section and fields are named here by the Katana customisation format's words
//! ("feature", "layer", "surface", "at vertices"), which the survey code tools show since the
//! survey code file's own (`map_data`, `model`, `tinable`, `mode vertex`) left the window. Each
//! expectation holding such a word was changed old to new from the table "The words a rule is
//! shown by" in docs/customisation.md, not from what a run printed.

use katana_cad::Document;
use katana_cad::code_table::{
    BuiltInSymbolTest, CodeExplanation, ColourLookup, DefinitionLookup, LintIssue, LintKind,
    LintSeverity, NearMissKind, code_census, code_table, code_table_row_matches, explain_code,
    format_code_census, format_code_explanation, format_coding_report, format_coverage,
    format_lint, lint_survey_map, lint_survey_rule,
};
use katana_cad::survey_coding::{
    CustomisationCoverage, SurveyCodingOptions, SurveyCodingReport, apply_survey_codes,
    customisation_coverage,
};
use katana_commands::{
    EntityAttributes, create_entities, create_point, create_style, set_entity_property,
};
use katana_entity::style_library::add_or_replace;
use katana_entity::tables::is_built_in_symbol_name;
use katana_entity::{
    Color, Entity, EntityId, EnumName, Geometry, LineStyle, PointGeometry, PropertyValue, Style,
    StyleLibrary, SurveyAttribute, SurveyBreakline, SurveyMap, SurveyMatchKind, SurveyPipe,
    SurveyRule, SurveySection, SurveySymbol, SurveyTextStyle,
};
use katana_geometry::Point2;

fn map_data(key: &str, model: &str, colour: &str, linestyle: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        model: String::from(model),
        colour: String::from(colour),
        linestyle: String::from(linestyle),
        ..SurveyRule::default()
    }
}

fn vertex_symbol(key: &str, style: &str, size: f64, colour: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        section: SurveySection::VertexSymbol,
        symbol: Some(SurveySymbol {
            style: String::from(style),
            colour: String::from(colour),
            size,
            rotation: 0.0,
            offset: 0.0,
            raise: 0.0,
        }),
        ..SurveyRule::default()
    }
}

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

// The shapes of the reference mapfile, cut down, by index:
//   0  AC*  feature: linestyle "0" - every symbol code says so
//   1  AC*  symbol: the bollard
//   2  1*   feature: a text code on linestyle "0", yellow
//   3  2*   feature: another, cyan
//   4  WM*  feature: a real linestyle, a colour Katana does not know
//   5  WM*  attributes: an attribute naming another ($)
//   6  PTBB feature: an exact key, a linestyle no library here defines
//   7  *    attributes: every code gets it
//   8-11 *  pipe: the reference files carry sixteen of these in each
//           pipe section, identical but for the DepthLocation they give
fn reference_shapes() -> SurveyMap {
    let mut map = SurveyMap::new();
    let mut bollard = map_data("AC*", "SURVEY DETAIL", "white", "0");
    bollard.group = String::from("SURVEY - CULT");
    bollard.comment = String::from("[AC*] Bollard");
    add(&mut map, bollard);
    add(&mut map, vertex_symbol("AC*", "TEST Bollard", 1.5, ""));
    add(&mut map, map_data("1*", "SURVEY LABELS", "yellow", "0"));
    add(&mut map, map_data("2*", "SURVEY LABELS", "cyan", "0"));
    let mut main = map_data(
        "WM*",
        "SURVEY PIPEWORK",
        "sui test water",
        "TEST Water Main",
    );
    main.breakline = Some(SurveyBreakline::Line);
    main.group = String::from("SURVEY - WATR");
    main.comment = String::from("[WM*] Main");
    add(&mut map, main);
    add(
        &mut map,
        SurveyRule {
            key: String::from("WM*"),
            section: SurveySection::StringAttribute,
            attributes: vec![attribute("text", "Diameter", "$PipeDiameter")],
            ..SurveyRule::default()
        },
    );
    add(
        &mut map,
        map_data("PTBB", "SURVEY DETAIL", "Green", "TEST Scrub Scattered"),
    );
    add(
        &mut map,
        SurveyRule {
            key: String::from("*"),
            section: SurveySection::StringAttribute,
            attributes: vec![attribute("integer", "Model Validation Zone", "1")],
            ..SurveyRule::default()
        },
    );
    for depth in ["Top of Pipe", "Obvert", "Cable", "Trace Wire"] {
        add(
            &mut map,
            SurveyRule {
                key: String::from("*"),
                section: SurveySection::Pipe,
                attributes: vec![attribute("text", "DepthLocation", depth)],
                pipe: Some(SurveyPipe {
                    justify: String::from("Obvert"),
                    shape: String::from("diameter"),
                    size1: String::from("$PipeDiameter"),
                    size2: String::new(),
                    active: true,
                }),
                ..SurveyRule::default()
            },
        );
    }
    map
}

// `symbol`: its customisation lists it as a symbol (LineStyle::symbol). That flag is what makes a
// definition a symbol now; these definitions once got there by `source` holding the word
// "symbol", and the sources are kept only as where each came from.
fn definition(name: &str, at_vertices: bool, source: &str, symbol: bool) -> LineStyle {
    LineStyle {
        name: String::from(name),
        at_vertices,
        source: String::from(source),
        symbol,
        ..LineStyle::default()
    }
}

fn library() -> StyleLibrary {
    let mut library = StyleLibrary::default();
    for style in [
        definition("TEST Bollard", true, "symbols_test.4d", true),
        definition("TEST Water Main", false, "linestyles_test.4d", false),
        // Not `mode vertex`, but listed as a symbol: a symbol all the same
        // (decision D3).
        definition("TEST Manhole Cover", false, "symbols_test.4d", true),
        // Made in a session: it names no customisation, and nothing lists it
        // as a symbol.
        definition("MYST Thing", false, "", false),
    ] {
        assert!(add_or_replace(&mut library, style).is_ok());
    }
    library
}

// A colour table of the test's own; "sui test water" is unknown to it, as it is to Katana.
fn test_colour(name: &str) -> Option<Color> {
    match name {
        "white" => Some(Color::new(255, 255, 255, 255)),
        "yellow" => Some(Color::new(255, 255, 0, 255)),
        "cyan" => Some(Color::new(0, 255, 255, 255)),
        "Green" => Some(Color::new(0, 255, 0, 255)),
        "red" => Some(Color::new(255, 0, 0, 255)),
        _ => None,
    }
}

fn explain(map: &SurveyMap, code: &str) -> CodeExplanation {
    let defined = library();
    let lookup = |name: &str| defined.find(name);
    let definition_of: &DefinitionLookup<'_> = &lookup;
    let colour_of: &ColourLookup<'_> = &test_colour;
    explain_code(map, code, Some(definition_of), Some(colour_of))
}

fn add_coded_point(document: &mut Document, at: Point2, code: &str) -> EntityId {
    assert!(
        document
            .execute(create_point(at, EntityAttributes::default()))
            .is_ok()
    );
    let ids = document.model().entities.ids();
    let id = ids[ids.len() - 1];
    assert!(
        document
            .execute(set_entity_property(
                vec![id],
                "code",
                PropertyValue::from(code)
            ))
            .is_ok()
    );
    id
}

fn kinds(issues: &[LintIssue]) -> Vec<(usize, LintKind)> {
    issues
        .iter()
        .map(|issue| (issue.rule, issue.kind))
        .collect()
}

// The test's colour table and Katana's own shapes, as the C++ hands them over.
fn lint_with(map: &SurveyMap, library: &StyleLibrary) -> Vec<LintIssue> {
    let colour_of: &ColourLookup<'_> = &test_colour;
    let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
    lint_survey_map(map, library, Some(colour_of), Some(built_in))
}

// ---- explaining one code ------------------------------------------------------

mod explain_code {
    use super::*;

    #[test]
    fn each_field_names_the_rule_it_came_from_most_specific_first() {
        let map = reference_shapes();
        let why = explain(&map, "AC01");

        assert_eq!(why.kind, SurveyMatchKind::Prefix);
        assert!(why.matched);
        // AC* (0, 1), then the bare `*` rules in read order (7 to 11).
        assert_eq!(why.rules, vec![0, 1, 7, 8, 9, 10, 11]);
        assert_eq!(why.resolved, map.lookup("AC01").resolved);

        // In CodeFieldSource's field order: rule 0 gives five, the vertex_symbol
        // rule the symbol, and the first `*` pipe row the pipe.
        assert_eq!(why.fields.len(), 7);
        let expected: [(&str, &str, usize, SurveySection); 7] = [
            ("layer", "SURVEY DETAIL", 0, SurveySection::Map),
            ("colour", "white", 0, SurveySection::Map),
            ("linestyle", "0", 0, SurveySection::Map),
            ("group", "SURVEY - CULT", 0, SurveySection::Map),
            ("comment", "[AC*] Bollard", 0, SurveySection::Map),
            (
                "symbol",
                "TEST Bollard, size 1.5",
                1,
                SurveySection::VertexSymbol,
            ),
            (
                "pipe",
                "diameter $PipeDiameter, Obvert",
                8,
                SurveySection::Pipe,
            ),
        ];
        for (field, (name, value, rule, section)) in why.fields.iter().zip(expected) {
            assert_eq!(field.field, name);
            assert_eq!(field.value, value, "{}", field.field);
            assert_eq!(field.rule, rule, "{}", field.field);
            assert_eq!(field.section, section, "{}", field.field);
            assert_eq!(field.key, map.rules()[rule].key);
        }

        assert!(why.linestyle.plain, "\"0\" is the plain continuous line");
        assert!(!why.linestyle.defined);
        assert_eq!(why.symbol.name, "TEST Bollard");
        assert!(why.symbol.defined);
        assert!(why.symbol.vertex_mode);
        let rgb = why.colour.rgb.expect("white is in the table");
        assert_eq!(rgb.to_hex(), "#FFFFFF");

        // Accumulated in rule order: the zone from rule 7, DepthLocation from
        // the FIRST pipe row (8) - rows 9 to 11 name it again and are not taken.
        assert_eq!(why.attributes.len(), 2);
        assert_eq!(why.attributes[0].attribute.name, "Model Validation Zone");
        assert_eq!(why.attributes[0].rule, 7);
        assert_eq!(why.attributes[0].scope, "string");
        assert_eq!(why.attributes[1].attribute.value, "Top of Pipe");
        assert_eq!(why.attributes[1].rule, 8);
        assert!(!why.attributes[1].deferred);
        assert!(why.near_misses.is_empty());
    }

    #[test]
    fn a_linestyle_is_resolved_a_colour_unknown_by_name_and_a_dollar_value_is_deferred() {
        let map = reference_shapes();
        let why = explain(&map, "WM01");
        assert_eq!(why.rules, vec![4, 5, 7, 8, 9, 10, 11]);
        assert_eq!(why.linestyle.name, "TEST Water Main");
        assert!(!why.linestyle.plain);
        assert!(why.linestyle.defined);
        assert!(!why.linestyle.vertex_mode);
        assert_eq!(why.colour.name, "sui test water");
        assert!(
            why.colour.rgb.is_none(),
            "an unknown name, not a guessed colour"
        );
        assert_eq!(why.attributes.len(), 3);
        assert_eq!(why.attributes[0].attribute.name, "Diameter");
        assert!(
            why.attributes[0].deferred,
            "$PipeDiameter names another attribute"
        );
        assert_eq!(why.attributes[0].rule, 5);
        // model, colour, linestyle, group, comment, breakline from rule 4; pipe
        // from rule 8.
        assert_eq!(why.fields.len(), 7);
        assert_eq!(why.fields[5].field, "breakline");
        assert_eq!(why.fields[5].value, "Line");
    }

    #[test]
    fn two_text_codes_on_linestyle_zero_differ_only_in_colour() {
        let map = reference_shapes();
        let yellow = explain(&map, "101");
        let cyan = explain(&map, "201");
        assert!(yellow.matched);
        assert!(yellow.linestyle.plain);
        assert!(cyan.linestyle.plain);
        assert!(yellow.symbol.name.is_empty());
        assert_eq!(yellow.resolved.model, cyan.resolved.model);
        let (Some(yellow_rgb), Some(cyan_rgb)) = (yellow.colour.rgb, cyan.colour.rgb) else {
            panic!("both colours are in the table");
        };
        assert_eq!(yellow_rgb.to_hex(), "#FFFF00");
        assert_eq!(cyan_rgb.to_hex(), "#00FFFF");
    }

    #[test]
    fn an_exact_key_is_exact_and_a_code_only_the_star_answers_is_fallback_only() {
        let map = reference_shapes();
        let exact = explain(&map, "PTBB");
        assert_eq!(exact.kind, SurveyMatchKind::Exact);
        assert!(exact.matched);
        assert!(!exact.linestyle.defined, "no library here defines it");

        // D5: the `*` rules give a pipe and attributes, but no model, linestyle
        // or symbol, so this is not a match.
        let typo = explain(&map, "ZZ99");
        assert_eq!(typo.kind, SurveyMatchKind::FallbackOnly);
        assert!(!typo.matched);
        assert_eq!(typo.rules, vec![7, 8, 9, 10, 11]);

        let nothing = explain(&SurveyMap::new(), "ZZ99");
        assert_eq!(nothing.kind, SurveyMatchKind::None);
        assert!(!nothing.matched);
        assert!(nothing.rules.is_empty());
        assert!(nothing.fields.is_empty());
        assert!(nothing.attributes.is_empty());
    }

    #[test]
    fn a_key_missed_only_by_case_or_by_blanks_is_named_as_a_near_miss() {
        let map = reference_shapes();
        let lower = explain(&map, "wm01");
        assert_eq!(
            lower.kind,
            SurveyMatchKind::FallbackOnly,
            "matching is byte for byte"
        );
        assert_eq!(lower.near_misses.len(), 1);
        assert_eq!(lower.near_misses[0].key, "WM*");
        assert_eq!(lower.near_misses[0].kind, NearMissKind::Case);

        let padded = explain(&map, " WM01");
        assert_eq!(padded.near_misses.len(), 1);
        assert_eq!(padded.near_misses[0].key, "WM*");
        assert_eq!(padded.near_misses[0].kind, NearMissKind::Whitespace);

        let exact = explain(&map, "Ptbb");
        assert_eq!(exact.near_misses.len(), 1);
        assert_eq!(exact.near_misses[0].key, "PTBB");
        assert_eq!(exact.near_misses[0].kind, NearMissKind::Case);
    }

    #[test]
    fn is_printed_as_one_text_naming_each_rule() {
        // Worked by hand from reference_shapes(): only the `*` rules catch
        // "wm01"; the pipe comes from rule 8, the zone from 7, DepthLocation
        // from 8.
        let expected = "Code \"wm01\": only the bare * rule answers it: fallback-only, not \
                        matched\n  pipe: diameter $PipeDiameter, Obvert  <- rule #8 * (pipe)\n  \
                        string attribute Model Validation Zone = 1  <- rule #7 * (attributes)\n  \
                        string attribute DepthLocation = Top of Pipe  <- rule #8 * (pipe)\n  near \
                        miss: key \"WM*\" differs only in letter case\n";
        assert_eq!(
            format_code_explanation(&explain(&reference_shapes(), "wm01")),
            expected
        );
    }

    #[test]
    fn a_later_rule_saying_something_else_is_shown_as_overruled() {
        // The two built-in mapfiles disagree on `hide` for 190 keys; a shorter
        // prefix can disagree with a longer one. A later rule saying the SAME is
        // not shown: nothing was decided against it.
        let mut map = SurveyMap::new();
        let mut shown = vertex_symbol("AC*", "TEST Bollard", 1.5, "");
        shown.hide = Some(false);
        let mut hidden = shown.clone();
        hidden.hide = Some(true);
        add(&mut map, shown); // 0
        add(&mut map, hidden); // 1
        add(&mut map, map_data("A*", "SURVEY DETAIL", "red", "")); // 2
        add(&mut map, map_data("AC*", "SURVEY DETAIL", "white", "")); // 3

        // By hand: AC* (0, 1, 3) before A* (2). layer: 3, and 2 agrees. colour:
        // 3 white, 2 red. hide: 0 no, 1 yes. symbol: 0, and 1 agrees.
        assert_eq!(
            format_code_explanation(&explain(&map, "AC01")),
            "Code \"AC01\": prefix match, matched\n  layer: SURVEY DETAIL  <- rule #3 AC* \
             (feature)\n  colour: white  <- rule #3 AC* (feature)\n    overruled: red  <- rule \
             #2 A* (feature)\n  hide: no  <- rule #0 AC* (symbol)\n    overruled: yes  <- rule #1 \
             AC* (symbol)\n  symbol: TEST Bollard, size 1.5  <- rule #0 AC* (symbol)\n  symbol \
             \"TEST Bollard\": defined, at vertices\n  colour \"white\": #FFFFFF\n"
        );
    }
}

// ---- the map as a table of codes ------------------------------------------------

mod code_table {
    use super::*;

    #[test]
    fn has_one_row_per_distinct_key_in_key_order_with_its_rules_and_sections() {
        let map = reference_shapes();
        let rows = code_table(&map);
        // By byte: '*' 42 < '1' 49 < '2' 50 < 'A' 65 < 'P' 80 < 'W' 87.
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0].key, "*");
        assert_eq!(rows[0].kind, SurveyMatchKind::FallbackOnly);
        assert_eq!(rows[0].rules, vec![7, 8, 9, 10, 11]);
        assert_eq!(
            rows[0].sections,
            vec![SurveySection::Pipe, SurveySection::StringAttribute],
            "distinct, in section order rather than read order"
        );
        assert_eq!(rows[1].key, "1*");
        assert_eq!(rows[2].key, "2*");

        let bollard = &rows[3];
        assert_eq!(bollard.key, "AC*");
        assert_eq!(bollard.kind, SurveyMatchKind::Prefix);
        assert_eq!(bollard.rules, vec![0, 1]);
        assert_eq!(
            bollard.sections,
            vec![SurveySection::Map, SurveySection::VertexSymbol]
        );
        // Combined as a code the key catches: its own rules, then `*`.
        assert_eq!(bollard.combined.model, "SURVEY DETAIL");
        let symbol = bollard.combined.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.style, "TEST Bollard");
        assert!(
            bollard.combined.pipe.is_some(),
            "the `*` pipe row reaches it too"
        );

        assert_eq!(rows[4].key, "PTBB");
        assert_eq!(rows[4].kind, SurveyMatchKind::Exact);
        assert_eq!(rows[5].key, "WM*");
        assert_eq!(rows[5].rules, vec![4, 5]);
    }

    #[test]
    fn a_filter_folds_case_and_searches_key_comment_group_model_colour_linestyle_and_symbol() {
        let rows = code_table(&reference_shapes());
        let keys_matching = |filter: &str| -> Vec<String> {
            rows.iter()
                .filter(|row| code_table_row_matches(row, filter))
                .map(|row| row.key.clone())
                .collect()
        };
        assert_eq!(keys_matching("bollard"), vec!["AC*"]);
        assert_eq!(keys_matching("survey labels"), vec!["1*", "2*"]);
        // Every row but `*`, which has no model, group or comment of its own.
        assert_eq!(
            keys_matching("SURVEY"),
            vec!["1*", "2*", "AC*", "PTBB", "WM*"]
        );
        assert_eq!(keys_matching("ptbb"), vec!["PTBB"]);
        assert_eq!(keys_matching("cyan"), vec!["2*"]);
        assert_eq!(keys_matching("test water main"), vec!["WM*"]);
        assert_eq!(keys_matching("").len(), 6);
    }
}

// ---- the codes a drawing carries ------------------------------------------------

mod code_census {
    use super::*;

    #[test]
    fn counts_each_distinct_code_and_classes_it_against_the_map() {
        let mut document = Document::default();
        document.set_survey_map(reference_shapes());
        add_coded_point(&mut document, Point2::new(0.0, 0.0), "AC01");
        add_coded_point(&mut document, Point2::new(1.0, 0.0), "ZZ99");
        add_coded_point(&mut document, Point2::new(2.0, 0.0), "AC01");
        add_coded_point(&mut document, Point2::new(3.0, 0.0), "101");
        assert!(
            document
                .execute(create_point(
                    Point2::new(4.0, 0.0),
                    EntityAttributes::default()
                ))
                .is_ok()
        ); // no code

        let census = code_census(&document, "");
        assert_eq!(census.property, "code");
        assert_eq!(census.coded, 4);
        assert_eq!(census.codes.len(), 3);
        // "101" < "AC01" < "ZZ99".
        assert_eq!(census.codes[0].code, "101");
        assert_eq!(census.codes[0].entities, 1);
        assert_eq!(census.codes[0].model, "SURVEY LABELS");
        assert_eq!(census.codes[1].code, "AC01");
        assert_eq!(census.codes[1].entities, 2);
        assert_eq!(census.codes[1].kind, SurveyMatchKind::Prefix);
        assert!(census.codes[1].matched);
        assert_eq!(census.codes[2].code, "ZZ99");
        assert_eq!(census.codes[2].kind, SurveyMatchKind::FallbackOnly);
        assert!(!census.codes[2].matched);

        let expected = "4 entities carry a code in \"code\", 3 distinct codes\n  101: 1, prefix, \
                        matched, layer SURVEY LABELS\n  AC01: 2, prefix, matched, layer SURVEY \
                        DETAIL\n  ZZ99: 1, fallback only, not matched\n";
        assert_eq!(format_code_census(&census), expected);
    }

    #[test]
    fn finds_codes_an_import_left_in_metadata() {
        let mut document = Document::default();
        document.set_survey_map(reference_shapes());
        let mut point = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(5.0, 5.0),
            }),
            ..Entity::default()
        };
        point
            .metadata
            .insert(String::from("12d.name"), PropertyValue::from("WM01"));
        assert!(document.execute(create_entities(vec![point])).is_ok());
        let census = code_census(&document, "");
        assert_eq!(census.property, "12d.name");
        assert_eq!(census.codes.len(), 1);
        assert_eq!(census.codes[0].code, "WM01");
        assert_eq!(census.codes[0].model, "SURVEY PIPEWORK");
    }
}

// ---- what is wrong with a map -----------------------------------------------------

mod lint_survey_map {
    use super::*;

    #[test]
    fn finds_the_unknown_colour_the_missing_linestyle_and_the_star_pipe_rows_that_never_contribute()
    {
        let issues = lint_with(&reference_shapes(), &library());
        // By hand, rule by rule: 0-3 are clean (linestyle "0" is plain, the
        // bollard is defined). 4: "sui test water" is not in the colour table.
        // 5 adds an attribute rule 4 does not give. 6: no library defines it.
        // 7 is the first `*`; 8 is the first to give a pipe. 9, 10, 11 give a
        // pipe and a DepthLocation, both of which 8 already gives, so a lookup
        // never takes anything from them.
        let expected = vec![
            (4, LintKind::UnknownColour),
            (6, LintKind::UnresolvedLinestyle),
            (9, LintKind::ShadowedRule),
            (10, LintKind::ShadowedRule),
            (11, LintKind::ShadowedRule),
        ];
        assert_eq!(kinds(&issues), expected);
        for issue in &issues {
            assert_eq!(issue.severity, LintSeverity::Warning);
        }
        assert_eq!(issues[2].key, "*");
        assert_eq!(issues[2].section, SurveySection::Pipe);
    }

    #[test]
    fn judges_symbols_and_linestyles_by_what_the_definition_is() {
        let mut map = SurveyMap::new();
        let no_model = SurveyRule {
            key: String::from("XX*"),
            colour: String::from("red"),
            ..SurveyRule::default()
        };
        add(&mut map, no_model.clone()); // 0
        add(
            &mut map,
            map_data("BL*", "SURVEY DETAIL", "", "TEST Bollard"),
        ); // 1
        add(&mut map, vertex_symbol("WV*", "TEST Water Main", 1.0, "")); // 2
        add(
            &mut map,
            vertex_symbol("SV*", "TEST Manhole Cover", 1.0, ""),
        ); // 3
        add(&mut map, vertex_symbol("CR*", "cross", 1.0, "")); // 4
        add(
            &mut map,
            vertex_symbol("NS*", "NOPE Symbol", 1.0, "not a colour"),
        ); // 5
        add(&mut map, map_data("CL*", "SURVEY DETAIL", "", "Continuous")); // 6
        add(&mut map, map_data("ON*", "SURVEY DETAIL", "", "1")); // 7
        add(&mut map, no_model); // 8
        add(&mut map, vertex_symbol("MY*", "MYST Thing", 1.0, "")); // 9

        // 0 and 8 put their code nowhere; 8 is also rule 0 again. 1 draws a
        // vertex definition along a line. 2 uses a definition listed as a
        // linestyle as a symbol. 3 is listed as a symbol and is taken as one. 4
        // is a shape Katana draws. 5 names nothing and its symbol colour is
        // unknown (UnresolvedSymbol sorts first). 6 and 7 are plain lines.
        //
        // 9 CHANGED, and not to match a run. "MYST Thing" names no source, and
        // this test once pinned that such a definition "is given the benefit of
        // the doubt" and passes as a symbol: the sign of a symbol was then the
        // name of the file a definition came from, and one from nowhere known
        // had no name to judge. Every definition now says which it is
        // (LineStyle::symbol), so a definition of unknown kind no longer
        // exists: one not listed as a symbol is a linestyle, and rule 9 uses a
        // linestyle as a symbol exactly as rule 2 does.
        let expected = vec![
            (0, LintKind::NoModel),
            (1, LintKind::LinestyleIsVertex),
            (2, LintKind::SymbolNotSymbolCapable),
            (5, LintKind::UnresolvedSymbol),
            (5, LintKind::UnknownColour),
            (8, LintKind::NoModel),
            (8, LintKind::DuplicateRule),
            (9, LintKind::SymbolNotSymbolCapable),
        ];
        let issues = lint_with(&map, &library());
        assert_eq!(kinds(&issues), expected);
        // It says where the definition is listed so, when the definition says
        // where it is from; one made in a session names no customisation.
        assert_eq!(
            issues[2].message,
            "symbol \"TEST Water Main\" is a linestyle from \"linestyles_test.4d\", not a symbol"
        );
        assert_eq!(
            issues[7].message,
            "symbol \"MYST Thing\" is a linestyle, not a symbol"
        );

        // With no built-in test, "cross" is unresolved too; with no colour table
        // no colour is judged.
        let bare = lint_survey_map(&map, &library(), None, None);
        let without_helpers = vec![
            (0, LintKind::NoModel),
            (1, LintKind::LinestyleIsVertex),
            (2, LintKind::SymbolNotSymbolCapable),
            (4, LintKind::UnresolvedSymbol),
            (5, LintKind::UnresolvedSymbol),
            (8, LintKind::NoModel),
            (8, LintKind::DuplicateRule),
            (9, LintKind::SymbolNotSymbolCapable),
        ];
        assert_eq!(kinds(&bare), without_helpers);
    }

    // The definition alone decides, not where it came from: the same name and
    // the same source, listed as a symbol or not.
    #[test]
    fn a_definition_is_a_symbol_when_its_customisation_lists_it_as_one_whatever_its_source_is_called()
     {
        let mut map = SurveyMap::new();
        add(&mut map, map_data("XX*", "SURVEY DETAIL", "", "")); // 0
        add(&mut map, vertex_symbol("XX*", "TEST Thing", 1.0, "")); // 1
        let warned_of = |at_vertices: bool, source: &str, symbol: bool| {
            let mut one = StyleLibrary::default();
            assert!(
                add_or_replace(
                    &mut one,
                    definition("TEST Thing", at_vertices, source, symbol)
                )
                .is_ok()
            );
            kinds(&lint_with(&map, &one))
        };
        let not_a_symbol = vec![(1, LintKind::SymbolNotSymbolCapable)];
        let clean: Vec<(usize, LintKind)> = Vec::new();
        // A source whose name holds "symbol" was once enough. It is a name.
        assert_eq!(warned_of(false, "My Symbols", false), not_a_symbol);
        assert_eq!(warned_of(false, "symbols_test.4d", false), not_a_symbol);
        // Listed as a symbol under a name that says nothing of the kind.
        assert_eq!(warned_of(false, "NSW", true), clean);
        assert_eq!(warned_of(false, "", true), clean);
        // `mode vertex` is a symbol whatever it is listed as.
        assert_eq!(warned_of(true, "linestyles_test.4d", false), clean);
    }

    #[test]
    fn a_symbol_rule_that_also_names_a_layer_is_warned_of_once_naming_each_stray_field() {
        // The hand-written rule the warning is for: a symbol rule given a layer
        // and a colour as well. A lookup applies both, so it is no error - but a
        // load that replaces this key's symbol rules takes the layer with them.
        let mut map = SurveyMap::new();
        let mut rule = vertex_symbol("BL*", "TEST Bollard", 1.5, "");
        rule.model = String::from("SURVEY DETAIL");
        rule.colour = String::from("white");
        add(&mut map, rule);
        add(
            &mut map,
            map_data("WM*", "SURVEY PIPEWORK", "white", "TEST Water Main"),
        );

        let issues = lint_with(&map, &library());
        assert_eq!(kinds(&issues), vec![(0, LintKind::FieldOutsideSection)]);
        assert_eq!(issues[0].severity, LintSeverity::Warning);
        // Both fields in one issue, in the order a rule's fields are listed, and
        // "them" for two.
        assert!(
            issues[0]
                .message
                .starts_with("holds layer, colour, which a "),
            "{}",
            issues[0].message
        );
        assert!(
            issues[0].message.contains(" replaces them too"),
            "{}",
            issues[0].message
        );

        // Counted with the rest, under its own name.
        let text = format_lint(&issues, map.size());
        assert!(
            text.contains("2 rules checked: 0 errors, 1 warning\n"),
            "{text}"
        );
        assert!(
            text.contains("  by kind: 1 field outside section\n"),
            "{text}"
        );
    }

    #[test]
    fn is_printed_as_one_text_with_the_counts() {
        let mut map = SurveyMap::new();
        add(
            &mut map,
            map_data(
                "WM*",
                "SURVEY PIPEWORK",
                "sui test water",
                "TEST Water Main",
            ),
        );
        add(&mut map, map_data("AC*", "SURVEY DETAIL", "white", "0"));
        let issues = lint_with(&map, &library());
        assert_eq!(
            format_lint(&issues, map.size()),
            "2 rules checked: 0 errors, 1 warning\n  by kind: 1 unknown colour\n  rule #0 WM* \
             (feature): warning, unknown colour: colour \"sui test water\" is not a colour name \
             Katana knows, so the entity keeps its own\n"
        );
        assert_eq!(format_lint(&[], 3), "3 rules checked: no problems found\n");
    }
}

// ---- a field its section does not use ---------------------------------------------

fn outside_section(rule: &SurveyRule) -> Vec<LintIssue> {
    let colour_of: &ColourLookup<'_> = &test_colour;
    let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
    lint_survey_rule(rule, 0, &library(), Some(colour_of), Some(built_in))
        .into_iter()
        .filter(|issue| issue.kind == LintKind::FieldOutsideSection)
        .collect()
}

// One field of a rule: the word the warning names it by, the sections that use it, and how to set
// it. The table is what a survey code file's sections each hold, written here from that format's
// own layout and not read from the lint: where a code goes (model, colour, linestyle, weight,
// group, breakline) in the feature section; its symbol and whether to hide it in the symbol
// section; its text style in the text section; each pipe in its own pipe section; the string's
// attributes in the pipe and the string-attribute sections, each vertex's in the vertex-pipe and
// the vertex-attribute sections, each segment's in the segment-pipe section; the surface flag in
// its own.
struct Field {
    word: &'static str,
    used_by: Vec<SurveySection>,
    set: fn(&mut SurveyRule),
}

fn fields() -> Vec<Field> {
    use SurveySection as S;
    // "layer" and "surface" are SurveyRule::model and ::tinable, by the words
    // every reply names them by (docs/customisation.md, "The words a rule is
    // shown by"); the warning once said the survey code file's own two.
    vec![
        Field {
            word: "layer",
            used_by: vec![S::Map],
            set: |r| r.model = String::from("SURVEY DETAIL"),
        },
        Field {
            word: "colour",
            used_by: vec![S::Map],
            set: |r| r.colour = String::from("white"),
        },
        Field {
            word: "linestyle",
            used_by: vec![S::Map],
            set: |r| r.linestyle = String::from("0"),
        },
        Field {
            word: "weight",
            used_by: vec![S::Map],
            set: |r| r.weight = String::from("0"),
        },
        Field {
            word: "group",
            used_by: vec![S::Map],
            set: |r| r.group = String::from("SURVEY"),
        },
        Field {
            word: "breakline",
            used_by: vec![S::Map],
            set: |r| r.breakline = Some(SurveyBreakline::Line),
        },
        Field {
            word: "surface",
            used_by: vec![S::Tinable],
            set: |r| r.tinable = Some(true),
        },
        Field {
            word: "hide",
            used_by: vec![S::VertexSymbol],
            set: |r| r.hide = Some(false),
        },
        Field {
            word: "symbol",
            used_by: vec![S::VertexSymbol],
            set: |r| {
                r.symbol = Some(SurveySymbol {
                    style: String::from("cross"),
                    ..SurveySymbol::default()
                });
            },
        },
        Field {
            word: "text style",
            used_by: vec![S::VertexTextStyle],
            set: |r| r.text_style = Some(SurveyTextStyle::default()),
        },
        Field {
            word: "pipe",
            used_by: vec![S::Pipe],
            set: |r| r.pipe = Some(SurveyPipe::default()),
        },
        Field {
            word: "vertex pipe",
            used_by: vec![S::VertexPipe],
            set: |r| r.vertex_pipe = Some(SurveyPipe::default()),
        },
        Field {
            word: "segment pipe",
            used_by: vec![S::SegmentPipe],
            set: |r| r.segment_pipe = Some(SurveyPipe::default()),
        },
        Field {
            word: "string attributes",
            used_by: vec![S::Pipe, S::StringAttribute],
            set: |r| r.attributes = vec![attribute("text", "Source", "field")],
        },
        Field {
            word: "vertex attributes",
            used_by: vec![S::VertexPipe, S::VertexAttribute],
            set: |r| r.vertex_attributes = vec![attribute("text", "Pit", "Grated")],
        },
        Field {
            word: "segment attributes",
            used_by: vec![S::SegmentPipe],
            set: |r| r.segment_attributes = vec![attribute("integer", "Count", "2")],
        },
    ]
}

fn sections() -> [SurveySection; 9] {
    use SurveySection as S;
    [
        S::Map,
        S::VertexSymbol,
        S::VertexTextStyle,
        S::Pipe,
        S::VertexPipe,
        S::SegmentPipe,
        S::StringAttribute,
        S::VertexAttribute,
        S::Tinable,
    ]
}

mod lint_survey_rule {
    use super::*;

    #[test]
    fn every_field_is_warned_of_in_every_section_that_does_not_use_it_and_in_no_other() {
        // 16 fields in 9 sections: 144 rules, each holding one field. 18 of the
        // pairs belong - 6 feature fields, the surface flag, 2 symbol fields, the
        // text style, 3 pipes, and the string's, each vertex's and each
        // segment's attributes with 2, 2 and 1 homes: 6 + 1 + 2 + 1 + 3 + 2 + 2 +
        // 1 = 18 - so 126 warn.
        let mut belong = 0usize;
        let mut warned = 0usize;
        for field in fields() {
            for section in sections() {
                let mut rule = SurveyRule {
                    key: String::from("AB*"),
                    section,
                    ..SurveyRule::default()
                };
                (field.set)(&mut rule);
                let uses = field.used_by.contains(&section);
                let issues = outside_section(&rule);
                let place = format!("{} in {}", field.word, section.name());
                if uses {
                    belong += 1;
                    assert!(issues.is_empty(), "{place}");
                    continue;
                }
                warned += 1;
                assert_eq!(issues.len(), 1, "{place}");
                assert_eq!(issues[0].severity, LintSeverity::Warning, "{place}");
                assert_eq!(issues[0].key, "AB*");
                assert_eq!(issues[0].section, section);
                // It names the field, first, in the words a person knows it by.
                assert!(
                    issues[0]
                        .message
                        .starts_with(&format!("holds {},", field.word)),
                    "{place}: {}",
                    issues[0].message
                );
            }
        }
        assert_eq!(belong, 18);
        assert_eq!(warned, 16 * 9 - 18);
    }

    #[test]
    fn a_rule_holding_only_what_its_section_uses_and_a_comment_is_not_warned_of() {
        // Everything a section uses, together, and a comment, which any may hold.
        for section in sections() {
            let mut rule = SurveyRule {
                key: String::from("AB*"),
                section,
                comment: String::from("a note about AB"),
                ..SurveyRule::default()
            };
            for field in fields() {
                if field.used_by.contains(&section) {
                    (field.set)(&mut rule);
                }
            }
            assert!(outside_section(&rule).is_empty(), "{}", section.name());
        }
    }

    #[test]
    fn catches_a_key_with_blanks_and_a_model_that_cannot_be_a_layer_before_the_map_refuses_them() {
        let rule = map_data(" WM*", "SURVEY//SERVICES", "", "");
        let mut map = SurveyMap::new();
        assert!(
            map.add(rule.clone()).is_err(),
            "a map never holds such a rule"
        );

        let colour_of: &ColourLookup<'_> = &test_colour;
        let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
        let issues = lint_survey_rule(&rule, 42, &library(), Some(colour_of), Some(built_in));
        assert_eq!(
            kinds(&issues),
            vec![
                (42, LintKind::InvalidLayerPath),
                (42, LintKind::KeyWhitespace)
            ]
        );
        for issue in &issues {
            assert_eq!(issue.severity, LintSeverity::Error);
            assert_eq!(issue.key, " WM*");
        }
    }
}

// ---- the coding report, as text ---------------------------------------------------

mod format_coding_report {
    use super::*;

    #[test]
    fn says_what_happened_overall_and_code_by_code() {
        let mut map = SurveyMap::new();
        add(
            &mut map,
            map_data("WM*", "SURVEY PIPEWORK", "", "TEST Water Main"),
        );
        let mut document = Document::default();
        document.set_survey_map(map);
        add_coded_point(&mut document, Point2::new(0.0, 0.0), "WM01");
        add_coded_point(&mut document, Point2::new(1.0, 0.0), "ZZ99");
        add_coded_point(&mut document, Point2::new(2.0, 0.0), "WM01");

        let mut report = SurveyCodingReport::default();
        let command = apply_survey_codes(
            &document,
            &SurveyCodingOptions::default(),
            Some(&mut report),
        );
        assert!(command.is_ok());
        // By hand: three coded, the two WM01 matched by WM*, ZZ99 by nothing (no
        // `*` here). Both WM01 points move from layer "0" and get the new style;
        // no library is loaded, so TEST Water Main is missing.
        assert_eq!(
            format_coding_report(&report),
            "3 entities carry a code in \"code\": 2 matched, 0 fallback-only (only the bare * \
             rule answers), 1 with no rule\n2 entities changed\nLayers created: SURVEY \
             PIPEWORK\nStyles created: TEST Water Main\nCodes with no rule: ZZ99\nNamed but in \
             no loaded library: TEST Water Main\nBy code:\n  WM01: 2 entities, prefix, matched; \
             layer 0 -> SURVEY PIPEWORK; style TEST Water Main (created); 2 changed\n  ZZ99: 1 \
             entity, none, not matched; 0 changed\n"
        );
    }

    #[test]
    fn an_entity_that_kept_its_layer_is_not_shown_moving_in_its_codes_row() {
        // create_layers off and no SURVEY PIPEWORK layer: the WM01 point stays on
        // "0" (audit CAD-18 keeps its style changing). Its row used to read
        // "layer 0 -> SURVEY PIPEWORK" all the same.
        let mut map = SurveyMap::new();
        add(
            &mut map,
            map_data("WM*", "SURVEY PIPEWORK", "", "TEST Water Main"),
        );
        let mut document = Document::default();
        document.set_survey_map(map);
        let id = add_coded_point(&mut document, Point2::new(0.0, 0.0), "WM01");

        let options = SurveyCodingOptions {
            create_layers: false,
            ..SurveyCodingOptions::default()
        };
        let mut report = SurveyCodingReport::default();
        let command = apply_survey_codes(&document, &options, Some(&mut report))
            .expect("the codes are planned")
            .expect("something changes");
        assert!(document.execute(command).is_ok());
        assert_eq!(
            document.model().entities.find(id).expect("the point").layer,
            "0",
            "the premise: it did not move"
        );
        // By hand: one coded, matched by WM*. It changes (its style), keeps
        // layer "0" because SURVEY PIPEWORK does not exist, and no layer is
        // created; the new style TEST Water Main is in no loaded library.
        assert_eq!(
            format_coding_report(&report),
            "1 entity carries a code in \"code\": 1 matched, 0 fallback-only (only the bare * \
             rule answers), 0 with no rule\n1 entity changed\nStyles created: TEST Water Main\n1 \
             entity kept their layer: it does not exist and layers are not being created\nNamed \
             but in no loaded library: TEST Water Main\nBy code:\n  WM01: 1 entity, prefix, \
             matched; layer 0 kept: SURVEY PIPEWORK does not exist and layers are not being \
             created; style TEST Water Main (created); 1 changed\n"
        );
    }
}

mod format_coverage {
    use super::*;

    #[test]
    fn names_built_in_shapes_apart_from_missing_names() {
        let coverage = CustomisationCoverage {
            styles: 3,
            named: 2,
            resolved: 1,
            built_in: 1,
            unresolved: vec![String::from("X")],
            ..CustomisationCoverage::default()
        };
        assert_eq!(
            format_coverage(&coverage),
            "1 of this drawing's 3 styles are drawn with a loaded definition (2 name one, 1 use \
             a shape Katana draws itself; the rest are plain lines)\n  1 name is in no loaded \
             library: \"X\"\n"
        );
    }

    #[test]
    fn a_drawing_with_no_styles_is_told_the_ways_to_get_one_without_being_sent_to_one_format() {
        // A drawing with no styles gives the customisation nothing to show. The
        // hint once sent everyone to one archive format; a survey import, a
        // drawing of any kind that carries styles, or a style made by hand all
        // show it as well.
        assert_eq!(
            format_coverage(&CustomisationCoverage::default()),
            "This drawing has no styles yet; import a drawing or survey that carries styles, or \
             make one in Format > Styles and Linetypes or with STYLE NEW, to see the \
             customisation take effect.\n"
        );
    }

    #[test]
    fn a_drawing_with_no_styles_is_not_sent_to_import_ones_when_the_customisation_has_definitions()
    {
        // With definitions loaded, "import a drawing or survey that carries
        // styles" points the wrong way: a survey import makes the styles itself,
        // and so does a rule that draws.
        let mut coverage = CustomisationCoverage {
            definitions_loaded: true,
            ..CustomisationCoverage::default()
        };
        assert_eq!(
            format_coverage(&coverage),
            "This drawing has no styles yet; the customisation's styles are made when survey \
             data is imported or when a rule draws.\n"
        );
        // Once the drawing has styles the hint is not said at all.
        coverage.styles = 1;
        assert!(!format_coverage(&coverage).contains("no styles yet"));
    }

    #[test]
    fn a_vertex_symbol_given_as_a_linetype_is_said_to_be_a_symbol_not_in_no_loaded_library() {
        // The reviewer's case: an archive's `style "TEST Bollard"` on a line,
        // where the loaded library defines TEST Bollard, but as a `mode vertex`
        // symbol. The line draws solid (D2), and the fix is to pick a linestyle -
        // so saying the name is "in no loaded library" sent people looking for a
        // library that is already loaded.
        //
        //   library  TEST Bollard  `mode vertex`  (the fixture's library())
        //            TEST Water Main     a linestyle
        //   styles   super  linetype TEST Bollard -> not a linestyle: drawn solid
        //            pipe   linetype PTBB Pipe    -> defined nowhere
        //            water  linetype TEST Water Main    -> the library draws it
        //
        // By hand: 3 styles; all three name a definition; water alone resolves
        // (1); none built in. One line each for the two reasons, in name order.
        let mut document = Document::default();
        document.set_style_library(library());
        for (name, linetype) in [
            ("super", "TEST Bollard"),
            ("pipe", "PTBB Pipe"),
            ("water", "TEST Water Main"),
        ] {
            let style = Style {
                name: String::from(name),
                linetype: String::from(linetype),
                ..Style::default()
            };
            assert!(document.execute(create_style(style)).is_ok(), "{name}");
        }
        assert_eq!(
            format_coverage(&customisation_coverage(&document)),
            "1 of this drawing's 3 styles are drawn with a loaded definition (3 name one; the \
             rest are plain lines)\n  1 name is in no loaded library: \"PTBB Pipe\"\n  1 name is \
             loaded as an `at vertices` symbol, not a linestyle, so a linetype naming it draws \
             solid: \"TEST Bollard\"\n"
        );
    }
}

mod customisation_coverage {
    use super::*;

    #[test]
    fn says_whether_definitions_are_loaded_before_any_style_is() {
        let mut document = Document::default();
        assert!(!customisation_coverage(&document).definitions_loaded);
        document.set_style_library(library());
        let coverage = customisation_coverage(&document);
        assert!(coverage.definitions_loaded);
        assert_eq!(coverage.styles, 0);
    }
}
