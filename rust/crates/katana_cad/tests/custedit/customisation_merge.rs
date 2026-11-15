// Loading customisations ON TOP of the one a session has: merge_customisation
// in Merge and Replace mode (customisation_merge.rs;
// tests/cad/customisation/test_customisation_merge.cpp).
//
// The first twelve tests are the merge's own, moved here with it from the
// reader of the older formats, where the same rule was written over style
// libraries and survey code files. Their expectations are the ones those
// tests worked out by hand and are unchanged; what changed is how the inputs
// are made. The "current" customisation stands for the one a session starts
// with, and the three loaded ones are that suite's hand-written fixture - a
// linestyle library, a survey code file and a symbol library - as three
// customisations of the same names, built here in code:
//
//   test_linestyles  3 definitions, none listed as a symbol
//   test_survey      11 rules in 11 (section, key) groups over 8 keys:
//                    7 feature rules (WM* KB* AC* TR* 1* 2* PX*), 3 symbol
//                    rules (AC* TR* PX*), 1 string attribute rule (*)
//   test_symbols     4 definitions, all listed as symbols
//
// The strokes are cut down to one or two: the merge reads none of them.

use katana_cad::CustomisationSource;
use katana_cad::customisation_merge::{
    CustomisationLoad, CustomisationMerge, LoadMode, merge_customisation,
};
use katana_entity::customisation::{customisation_from_json, customisation_to_json};
use katana_entity::entity::to_string;
use katana_entity::{
    Color, Customisation, CustomisationAutomation, CustomisationBase, CustomisationWriteOptions,
    LineStyle, LineworkCodes, Stroke, StrokeOp, SurveyAttribute, SurveyMap, SurveyPipe, SurveyRule,
    SurveySection, SurveySymbol,
};
use katana_geometry::Point2;

fn move_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Move,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn draw_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Draw,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn circle(radius: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Circle,
        radius,
        ..Stroke::default()
    }
}

/// A definition of `customisation`, which is its source as a file's reader
/// stamps it; `symbol` is the array of the file it would sit in.
fn define(
    customisation: &mut Customisation,
    name: &str,
    symbol: bool,
    at_vertices: bool,
    strokes: Vec<Stroke>,
) {
    let definition = LineStyle {
        name: String::from(name),
        source: customisation.name.clone(),
        symbol,
        at_vertices,
        strokes,
        ..LineStyle::default()
    };
    if let Err(error) = customisation.library.add(definition) {
        panic!("{}", error.describe());
    }
}

fn rule(customisation: &mut Customisation, made: SurveyRule) {
    if let Err(error) = customisation.map.add(made) {
        panic!("{}", error.describe());
    }
}

fn feature(key: &str, model: &str, colour: &str, linestyle: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        section: SurveySection::Map,
        model: String::from(model),
        colour: String::from(colour),
        linestyle: String::from(linestyle),
        ..SurveyRule::default()
    }
}

fn symbol_of(key: &str, style: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        section: SurveySection::VertexSymbol,
        symbol: Some(SurveySymbol {
            style: String::from(style),
            ..SurveySymbol::default()
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

fn pipe(justify: &str) -> Option<SurveyPipe> {
    Some(SurveyPipe {
        justify: String::from(justify),
        ..SurveyPipe::default()
    })
}

/// A rule of one of the four sections that give attributes: the two pipe
/// sections also say how the pipe is justified.
fn attributes_of(
    section: SurveySection,
    key: &str,
    attributes: Vec<SurveyAttribute>,
    justify: &str,
) -> SurveyRule {
    let mut made = SurveyRule {
        key: String::from(key),
        section,
        ..SurveyRule::default()
    };
    match section {
        SurveySection::Pipe => {
            made.attributes = attributes;
            made.pipe = pipe(justify);
        }
        SurveySection::StringAttribute => made.attributes = attributes,
        SurveySection::VertexPipe => {
            made.vertex_attributes = attributes;
            made.vertex_pipe = pipe(justify);
        }
        SurveySection::VertexAttribute => made.vertex_attributes = attributes,
        _ => panic!("not a section that gives attributes"),
    }
    made
}

fn named(name: &str) -> Customisation {
    Customisation {
        name: String::from(name),
        ..Customisation::default()
    }
}

/// What is loaded before the load: one definition the fixture also defines
/// ("TEST Survey Mark", with a different stroke) and one it does not; a WM*
/// feature rule the fixture also gives, a WM* symbol rule it does not, and a
/// ZZ* rule it does not.
fn current() -> Customisation {
    let mut now = named("builtin");
    define(
        &mut now,
        "TEST Survey Mark",
        true,
        true,
        vec![move_to(0.0, 0.0), circle(9.0)],
    );
    define(
        &mut now,
        "Kept Style",
        false,
        false,
        vec![move_to(0.0, 0.0), draw_to(1.0, 0.0)],
    );
    rule(&mut now, feature("WM*", "OLD SERVICES", "red", ""));
    rule(&mut now, feature("ZZ*", "KEPT", "", ""));
    rule(&mut now, symbol_of("WM*", "Kept Style"));
    now
}

fn fixture_linestyles() -> Customisation {
    let mut lines = named("test_linestyles");
    define(
        &mut lines,
        "TEST Dashed Kerb",
        false,
        false,
        vec![move_to(0.0, 0.0), draw_to(1.5, 0.0)],
    );
    define(
        &mut lines,
        "TEST Water Main",
        false,
        false,
        vec![move_to(0.0, 0.0), draw_to(4.5, 0.0)],
    );
    define(
        &mut lines,
        "TEST Gate",
        false,
        false,
        vec![move_to(0.0, 0.0), draw_to(4.0, 0.0)],
    );
    lines
}

fn fixture_survey() -> Customisation {
    let mut survey = named("test_survey");
    rule(
        &mut survey,
        feature("WM*", "TEST SERVICES", "blue", "TEST Water Main"),
    );
    rule(
        &mut survey,
        feature("KB*", "TEST ROADS", "sui test purple", "TEST Dashed Kerb"),
    );
    rule(&mut survey, feature("AC*", "TEST FURNITURE", "white", "0"));
    rule(&mut survey, feature("TR*", "TEST VEGETATION", "green", "0"));
    rule(&mut survey, feature("1*", "TEST TEXT", "orange", "0"));
    rule(&mut survey, feature("2*", "TEST TEXT", "red", "0"));
    rule(&mut survey, feature("PX*", "TEST MISC", "yellow", "0"));
    rule(&mut survey, symbol_of("AC*", "TEST Survey Mark"));
    rule(&mut survey, symbol_of("TR*", "TEST Tree"));
    rule(&mut survey, symbol_of("PX*", "TEST Missing Symbol"));
    rule(
        &mut survey,
        attributes_of(
            SurveySection::StringAttribute,
            "*",
            vec![attribute("text", "Source", "Katana test fixture")],
            "",
        ),
    );
    survey
}

fn fixture_symbols() -> Customisation {
    let mut symbols = named("test_symbols");
    define(
        &mut symbols,
        "TEST Survey Mark",
        true,
        true,
        vec![move_to(0.0, 0.0), circle(0.3)],
    );
    define(
        &mut symbols,
        "TEST Tree",
        true,
        false,
        vec![move_to(0.0, 0.0), circle(1.0)],
    );
    define(
        &mut symbols,
        "TEST Valve",
        true,
        true,
        vec![move_to(-0.4, -0.4), draw_to(0.4, -0.4)],
    );
    define(
        &mut symbols,
        "TEST U Turn",
        true,
        true,
        vec![move_to(0.0, 0.0), draw_to(1.0, 0.0)],
    );
    symbols
}

/// The whole fixture, in the order its files loaded in: by name.
fn fixture() -> Vec<Customisation> {
    vec![fixture_linestyles(), fixture_survey(), fixture_symbols()]
}

fn load_named<'a>(merge: &'a CustomisationMerge, name: &str) -> Option<&'a CustomisationLoad> {
    merge.loads.iter().find(|load| load.name == name)
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

/// "type name=value" each, so that a failure prints something readable.
fn shown(attributes: &[SurveyAttribute]) -> Vec<String> {
    attributes
        .iter()
        .map(|attribute| {
            format!(
                "{} {}={}",
                attribute.r#type, attribute.name, attribute.value
            )
        })
        .collect()
}

/// Each rule's section, in map order.
fn sections(map: &SurveyMap) -> Vec<SurveySection> {
    map.rules().iter().map(|one| one.section).collect()
}

fn source(name: &str, definitions: bool, rules: bool, notice: &[&str]) -> CustomisationSource {
    CustomisationSource {
        name: String::from(name),
        definitions,
        rules,
        notice: strings(notice),
    }
}

/// Two pairs of sections fill ONE field of a code: the pipe and the string
/// attribute sections both give the string's attributes, the vertex pipe and
/// the vertex attribute sections both give each vertex's. Where two rules of
/// one key name the same attribute the EARLIER rule's value is the one a code
/// gets (SurveyMap::lookup), whichever sections they are in.
fn pipe_session() -> Customisation {
    let mut now = named("builtin");
    rule(
        &mut now,
        attributes_of(
            SurveySection::Pipe,
            "*",
            vec![
                attribute("text", "DepthLocation", "Top of Pipe"),
                attribute("text", "Material", "PVC"),
            ],
            "Obvert",
        ),
    );
    rule(
        &mut now,
        attributes_of(
            SurveySection::VertexPipe,
            "SW*",
            vec![attribute("text", "Pit", "Grated")],
            "Invert",
        ),
    );
    now
}

mod merge_customisation {
    use super::*;

    // ---- the twelve tests the merge came with ---------------------------------------

    #[test]
    fn merging_keeps_every_current_definition_the_load_does_not_replace() {
        let now = current();
        let loaded = fixture();
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);

        // 2 current + 7 loaded, less the 1 name both hold ("TEST Survey Mark") = 8.
        assert!(merged.definitions_loaded);
        assert_eq!(merged.merged.library.size(), 8);
        let kept = merged
            .merged
            .library
            .find("Kept Style")
            .expect("Kept Style");
        assert_eq!(kept.source, "builtin");
        // The loaded definition won, and says where it came from.
        let mark = merged
            .merged
            .library
            .find("TEST Survey Mark")
            .expect("TEST Survey Mark");
        assert_eq!(mark.source, "test_symbols");
        assert!(
            merged.removed_definitions.is_empty(),
            "merging removes nothing"
        );
        assert!(merged.problems.is_empty());
        assert!(merged.ok());
    }

    #[test]
    fn each_loaded_customisation_reports_the_names_it_added_and_the_names_it_replaced() {
        let now = current();
        let loaded = fixture();
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);

        // One entry per loaded customisation, in load order.
        assert_eq!(merged.loads.len(), 3);
        assert_eq!(merged.loads[0].name, "test_linestyles");
        assert_eq!(merged.loads[1].name, "test_survey");
        assert_eq!(merged.loads[2].name, "test_symbols");

        let lines = load_named(&merged, "test_linestyles").expect("test_linestyles");
        assert!(lines.definitions);
        assert!(!lines.rules);
        assert_eq!(
            sorted(lines.added_definitions.clone()),
            strings(&["TEST Dashed Kerb", "TEST Gate", "TEST Water Main"])
        );
        assert!(lines.replaced_definitions.is_empty());

        let symbols = load_named(&merged, "test_symbols").expect("test_symbols");
        assert_eq!(
            sorted(symbols.added_definitions.clone()),
            strings(&["TEST Tree", "TEST U Turn", "TEST Valve"])
        );
        assert_eq!(symbols.replaced_definitions, strings(&["TEST Survey Mark"]));

        // The 11 rules are 11 (section, key) groups: 7 feature, 3 symbol, 1
        // string attribute. Only the WM* feature group was already there;
        // AC*, TR* and PX* are listed twice because the customisation gives
        // each rules in two sections.
        let survey = load_named(&merged, "test_survey").expect("test_survey");
        assert!(!survey.definitions);
        assert!(survey.rules);
        assert_eq!(survey.replaced_keys, strings(&["WM*"]));
        assert_eq!(
            sorted(survey.added_keys.clone()),
            strings(&[
                "*", "1*", "2*", "AC*", "AC*", "KB*", "PX*", "PX*", "TR*", "TR*"
            ])
        );
    }

    #[test]
    fn a_loaded_key_replaces_that_keys_rules_in_one_section_and_no_other() {
        let now = current();
        let loaded = fixture();
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);

        // 3 current rules, less the 1 WM* feature rule the load replaces,
        // plus the 11 loaded = 13.
        assert!(merged.rules_loaded);
        let map = &merged.merged.map;
        assert_eq!(map.size(), 13);

        // WM1 now goes where the fixture sends it, and keeps the symbol the
        // current map gave WM* - the fixture has no WM* symbol rule.
        let wm = map.lookup("WM1");
        assert_eq!(wm.resolved.model, "TEST SERVICES");
        assert_eq!(wm.resolved.colour, "blue");
        let symbol = wm.resolved.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.style, "Kept Style");
        // A key the load does not mention is untouched.
        assert_eq!(map.lookup("ZZ1").resolved.model, "KEPT");

        // The loaded WM* rule stands where the key's first current rule stood
        // - here the one it replaces - ahead of the WM* symbol rule it leaves;
        // the rest keep their order, and keys that are new follow in the
        // order they were loaded.
        let rules = map.rules();
        assert_eq!(rules[0].key, "WM*");
        assert_eq!(rules[0].model, "TEST SERVICES");
        assert_eq!(rules[1].key, "ZZ*");
        assert_eq!(rules[2].key, "WM*");
        assert_eq!(rules[2].section, SurveySection::VertexSymbol);
        assert_eq!(rules[3].key, "KB*");
        assert_eq!(rules[12].key, "*");
        assert_eq!(rules[12].section, SurveySection::StringAttribute);
    }

    #[test]
    fn rules_of_one_key_in_a_loaded_customisation_all_replace_and_are_reported_once() {
        let now = current();
        // Two WM* feature rules in one customisation: the earlier wins a field
        // both set (SurveyMap::add), so both must come in, in their order, and
        // the old WM* feature rule must go - left in front it would outrank
        // them both.
        let mut mine = named("mine");
        rule(&mut mine, feature("WM*", "MINE", "", ""));
        rule(&mut mine, feature("WM*", "SHADOWED", "green", ""));
        let loaded = vec![mine];
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);

        // 3 current - 1 WM* feature rule + 2 = 4.
        assert_eq!(merged.merged.map.size(), 4);
        let wm = merged.merged.map.lookup("WM1");
        assert_eq!(wm.resolved.model, "MINE");
        assert_eq!(
            wm.resolved.colour, "green",
            "the second rule supplies what the first leaves out"
        );
        assert_eq!(merged.loads.len(), 1);
        assert_eq!(merged.loads[0].replaced_keys, strings(&["WM*"]));
        assert!(merged.loads[0].added_keys.is_empty());
    }

    #[test]
    fn a_symbol_library_alone_leaves_the_survey_map_as_it_was() {
        // Loading a personal symbol library once installed its EMPTY map
        // over the survey codes.
        let now = current();
        let loaded = vec![fixture_symbols()];
        for mode in [LoadMode::Merge, LoadMode::Replace] {
            let merged = merge_customisation(&now, &loaded, mode);
            assert!(!merged.rules_loaded, "{}", to_string(mode));
            assert_eq!(
                merged.merged.map.rules(),
                now.map.rules(),
                "{}",
                to_string(mode)
            );
            assert!(merged.removed_keys.is_empty(), "{}", to_string(mode));
        }
    }

    #[test]
    fn survey_codes_alone_leave_the_library_as_it_was() {
        let now = current();
        let loaded = vec![fixture_survey()];
        for mode in [LoadMode::Merge, LoadMode::Replace] {
            let merged = merge_customisation(&now, &loaded, mode);
            assert!(!merged.definitions_loaded, "{}", to_string(mode));
            assert_eq!(
                merged.merged.library.all(),
                now.library.all(),
                "{}",
                to_string(mode)
            );
            assert!(merged.removed_definitions.is_empty(), "{}", to_string(mode));
        }
    }

    #[test]
    fn a_load_that_brought_nothing_changes_nothing() {
        let now = current();
        let merged = merge_customisation(&now, &[], LoadMode::Replace);
        assert!(!merged.definitions_loaded);
        assert!(!merged.rules_loaded);
        assert_eq!(merged.merged.library.all(), now.library.all());
        assert_eq!(merged.merged.map.rules(), now.map.rules());
        assert!(merged.loads.is_empty());
        assert!(
            merged.merged == now,
            "not its name, its sources or anything else"
        );

        // A customisation that is named and holds nothing is still a load,
        // listed with nothing against it, and replaces neither kind: it
        // brought neither.
        let empty = vec![named("empty")];
        let nothing = merge_customisation(&now, &empty, LoadMode::Replace);
        assert_eq!(nothing.loads.len(), 1);
        assert_eq!(nothing.loads[0].name, "empty");
        assert!(!nothing.loads[0].definitions);
        assert!(!nothing.loads[0].rules);
        assert_eq!(nothing.merged.library.all(), now.library.all());
        assert_eq!(nothing.merged.map.rules(), now.map.rules());
        assert!(nothing.removed_definitions.is_empty());
        assert!(nothing.removed_keys.is_empty());
    }

    #[test]
    fn replacing_installs_only_what_was_loaded_and_lists_what_went_with_it() {
        let now = current();
        let loaded = fixture();
        let replaced = merge_customisation(&now, &loaded, LoadMode::Replace);

        // The 7 loaded definitions and nothing else, each as it was loaded,
        // in name order; and the 11 loaded rules in the order they were
        // loaded.
        let mut definitions = loaded[0].library.all();
        definitions.extend(loaded[2].library.all());
        definitions.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(definitions.len(), 7);
        assert_eq!(replaced.merged.library.all(), definitions);
        assert_eq!(replaced.merged.map.rules(), loaded[1].map.rules());
        // "Kept Style" is not in the fixture; "TEST Survey Mark" is. WM* is
        // still a key of the loaded map (its feature rule), so only ZZ* went -
        // even though the WM* symbol rule went with the rest of the old map.
        assert_eq!(replaced.removed_definitions, strings(&["Kept Style"]));
        assert_eq!(replaced.removed_keys, strings(&["ZZ*"]));
        // What each customisation added or replaced is reported the same way
        // as a merge.
        let from_symbols = load_named(&replaced, "test_symbols").expect("test_symbols");
        assert_eq!(
            from_symbols.replaced_definitions,
            strings(&["TEST Survey Mark"])
        );
    }

    #[test]
    fn a_loaded_customisation_that_changes_nothing_is_still_listed() {
        // Loading a library twice over itself: the second copy replaces every
        // name, and a customisation whose definitions all lost to a later one
        // is still named, with nothing against it, so "it was loaded" is on
        // the report.
        let mut a = named("a");
        define(&mut a, "X", false, false, vec![move_to(0.0, 0.0)]);
        let mut b = named("b");
        define(
            &mut b,
            "X",
            false,
            false,
            vec![move_to(0.0, 0.0), draw_to(1.0, 0.0)],
        );
        let loaded = vec![a, b];
        let merged = merge_customisation(&Customisation::default(), &loaded, LoadMode::Merge);
        assert_eq!(merged.loads.len(), 2);
        assert_eq!(merged.loads[0].name, "a");
        assert!(merged.loads[0].added_definitions.is_empty());
        assert_eq!(merged.loads[1].name, "b");
        assert_eq!(merged.loads[1].added_definitions, strings(&["X"]));
        let x = merged.merged.library.find("X").expect("X");
        assert_eq!(x.strokes.len(), 2, "the later customisation's definition");
    }

    #[test]
    fn a_loaded_attribute_wins_over_the_same_attribute_from_the_other_section_that_sets_it() {
        // The load gives `*` a DepthLocation in the string attribute section
        // and SW* a Pit in the vertex attribute section; the current map gives
        // both in the sibling sections, the pipe and the vertex pipe. Neither
        // (section, key) group is in the current map, so both are ADDED - and
        // what the load brings must still be what a code gets.
        let now = pipe_session();
        let mut mine = named("mine");
        rule(
            &mut mine,
            attributes_of(
                SurveySection::StringAttribute,
                "*",
                vec![attribute("text", "DepthLocation", "Invert")],
                "",
            ),
        );
        rule(
            &mut mine,
            attributes_of(
                SurveySection::VertexAttribute,
                "SW*",
                vec![attribute("text", "Pit", "Solid")],
                "",
            ),
        );
        let loaded = vec![mine];
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);

        assert_eq!(merged.loads.len(), 1);
        assert_eq!(merged.loads[0].added_keys, strings(&["*", "SW*"]));
        assert!(merged.loads[0].replaced_keys.is_empty());
        // 2 current rules, none replaced, + 2 loaded = 4.
        let map = &merged.merged.map;
        assert_eq!(map.size(), 4);

        let sw = map.lookup("SW1");
        // SW1 meets SW* and then `*`. The string's attributes all come from
        // `*`: DepthLocation from the LOADED string attribute rule, Material -
        // which the load says nothing about - still from the current pipe
        // rule, in that order.
        assert_eq!(
            shown(&sw.resolved.attributes),
            strings(&["text DepthLocation=Invert", "text Material=PVC"])
        );
        // Each vertex's Pit comes from SW*: the loaded vertex attribute rule,
        // not the current vertex pipe one.
        assert_eq!(
            shown(&sw.resolved.vertex_attributes),
            strings(&["text Pit=Solid"])
        );
        // And the current rules still give what only they say.
        assert_eq!(sw.resolved.pipe.as_ref().expect("a pipe").justify, "Obvert");
        assert_eq!(
            sw.resolved
                .vertex_pipe
                .as_ref()
                .expect("a vertex pipe")
                .justify,
            "Invert"
        );

        // Worked out by hand: at the current map's first `*` rule the loaded
        // `*` rule goes in first, then the current one it leaves standing; the
        // same at SW*.
        assert_eq!(
            sections(map),
            vec![
                SurveySection::StringAttribute,
                SurveySection::Pipe,
                SurveySection::VertexAttribute,
                SurveySection::VertexPipe,
            ]
        );
    }

    #[test]
    fn a_loaded_group_replacing_one_in_place_still_goes_ahead_of_its_keys_other_sections() {
        // The current map has `*` in the pipe section AND, after it, in the
        // string attribute section. The load replaces the string attribute
        // group; put back where that group stood, it would stand behind the
        // pipe rule and its DepthLocation would lose to "Top of Pipe".
        let mut now = named("builtin");
        rule(
            &mut now,
            attributes_of(
                SurveySection::Pipe,
                "*",
                vec![attribute("text", "DepthLocation", "Top of Pipe")],
                "Obvert",
            ),
        );
        rule(
            &mut now,
            attributes_of(
                SurveySection::StringAttribute,
                "*",
                vec![attribute("text", "DepthLocation", "Centre")],
                "",
            ),
        );
        let mut mine = named("mine");
        rule(
            &mut mine,
            attributes_of(
                SurveySection::StringAttribute,
                "*",
                vec![attribute("text", "DepthLocation", "Invert")],
                "",
            ),
        );
        let loaded = vec![mine];
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);

        assert_eq!(merged.loads.len(), 1);
        assert_eq!(merged.loads[0].replaced_keys, strings(&["*"]));
        // 2 current - 1 replaced + 1 loaded = 2: the loaded rule, then the
        // pipe.
        let map = &merged.merged.map;
        assert_eq!(map.size(), 2);
        assert_eq!(
            sections(map),
            vec![SurveySection::StringAttribute, SurveySection::Pipe]
        );
        assert_eq!(
            shown(&map.lookup("ANY").resolved.attributes),
            strings(&["text DepthLocation=Invert"])
        );
    }

    #[test]
    fn what_a_merged_load_brought_still_wins_after_the_session_is_written_and_read_back() {
        // An edited customisation persists only by being written, so a merge
        // is only as good as what is written of it. The merged map holds `*`
        // in the string attribute section (loaded) ahead of `*` in the pipe
        // section (current), and the file must keep the loaded DepthLocation
        // winning.
        let now = pipe_session();
        let mut mine = named("mine");
        rule(
            &mut mine,
            attributes_of(
                SurveySection::StringAttribute,
                "*",
                vec![attribute("text", "DepthLocation", "Invert")],
                "",
            ),
        );
        let loaded = vec![mine];
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);
        let written =
            match customisation_to_json(&merged.merged, &CustomisationWriteOptions::default()) {
                Ok(written) => written,
                Err(error) => panic!("{}", error.describe()),
            };
        let again = match customisation_from_json(written.as_bytes()) {
            Ok(again) => again,
            Err(error) => panic!("{}", error.describe()),
        };

        // By hand, as in ALoadedAttributeWinsOverTheSameAttributeFromTheOther-
        // SectionThatSetsIt: DepthLocation from the loaded rule, Material from
        // the current pipe rule.
        assert_eq!(
            shown(&again.map.lookup("SW1").resolved.attributes),
            strings(&["text DepthLocation=Invert", "text Material=PVC"])
        );
        // And the whole of it: what was merged is what was read.
        assert!(again == merged.merged);
    }

    // ---- what one customisation holds beyond definitions and rules ------------------

    #[test]
    fn a_loaded_colour_takes_the_place_of_the_sessions_of_that_name_however_it_is_spelled() {
        // Colour names compare by their fold: "SUI_Gas" is "sui gas". By hand:
        // the session has two names; the load gives one of them again, spelled
        // another way and meaning another colour, and one that is new. Three
        // names after it, the respelled one the loaded colour - in BOTH modes:
        // a colour is no kind a Replace takes the place of.
        let mut now = named("builtin");
        assert!(now.colours.add("sui gas", Color::new(1, 2, 3, 255)).is_ok());
        assert!(
            now.colours
                .add("site kerb", Color::new(9, 9, 9, 255))
                .is_ok()
        );
        let mut mine = named("mine");
        assert!(
            mine.colours
                .add("SUI_Gas", Color::new(10, 11, 12, 255))
                .is_ok()
        );
        assert!(
            mine.colours
                .add("new one", Color::new(20, 21, 22, 255))
                .is_ok()
        );
        let loaded = vec![mine];

        for mode in [LoadMode::Merge, LoadMode::Replace] {
            let merged = merge_customisation(&now, &loaded, mode);
            assert!(merged.ok(), "{}", to_string(mode));
            let colours = &merged.merged.colours;
            assert_eq!(colours.size(), 3);
            assert_eq!(colours.find("sui gas"), Some(Color::new(10, 11, 12, 255)));
            assert_eq!(colours.find("site kerb"), Some(Color::new(9, 9, 9, 255)));
            assert_eq!(colours.find("new one"), Some(Color::new(20, 21, 22, 255)));
            // Written back as the loaded customisation spelled it.
            let names: Vec<String> = colours
                .entries()
                .into_iter()
                .map(|entry| entry.name)
                .collect();
            assert_eq!(names, strings(&["new one", "site kerb", "SUI_Gas"]));
            assert_eq!(merged.loads.len(), 1);
            assert_eq!(merged.loads[0].replaced_colours, strings(&["SUI_Gas"]));
            assert_eq!(merged.loads[0].added_colours, strings(&["new one"]));
            // Colours alone are neither kind.
            assert!(!merged.definitions_loaded);
            assert!(!merged.rules_loaded);
        }
    }

    #[test]
    fn linework_codes_and_automation_are_taken_only_from_a_customisation_that_says_them() {
        // A file of symbols for a colleague must not reset their control
        // codes.
        let mut now = named("builtin");
        let own = LineworkCodes {
            start: String::from("S"),
            ..LineworkCodes::default()
        };
        now.linework = Some(own.clone());
        now.automation = Some(CustomisationAutomation {
            codes_on_survey_import: true,
            linework_on_survey_import: false,
        });

        let silent = vec![fixture_symbols()];
        let kept = merge_customisation(&now, &silent, LoadMode::Replace);
        assert_eq!(kept.merged.linework.as_ref(), Some(&own));
        assert_eq!(kept.merged.automation, now.automation);
        assert!(!kept.loads[0].linework);
        assert!(!kept.loads[0].automation);

        // One that says them is taken at its word, the later of two winning.
        let mut first = named("first");
        first.linework = Some(LineworkCodes {
            start: String::from("BEGIN"),
            ..LineworkCodes::default()
        });
        let mut second = named("second");
        second.linework = Some(LineworkCodes {
            start: String::from("GO"),
            ..LineworkCodes::default()
        });
        second.automation = Some(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        let saying = vec![first, second];
        let taken = merge_customisation(&now, &saying, LoadMode::Merge);
        assert_eq!(
            taken.merged.linework.as_ref().expect("linework").start,
            "GO"
        );
        assert_eq!(
            taken.merged.automation,
            Some(CustomisationAutomation {
                codes_on_survey_import: false,
                linework_on_survey_import: false,
            })
        );
        assert!(taken.loads[0].linework);
        assert!(!taken.loads[0].automation);
        assert!(taken.loads[1].linework);
        assert!(taken.loads[1].automation);

        // A session that says neither, loaded with one that says neither,
        // still says neither: absent is not the defaults.
        let neither = merge_customisation(&named("bare"), &silent, LoadMode::Merge);
        assert!(neither.merged.linework.is_none());
        assert!(neither.merged.automation.is_none());
    }

    #[test]
    fn each_loaded_customisation_is_recorded_as_a_source_with_what_it_brought() {
        // By hand. The session lists no sources, so it is its own one source,
        // bringing both kinds; the three loads follow in load order, each with
        // what it brought, and the notice the survey codes came with.
        let now = current();
        let mut loaded = fixture();
        loaded[1].notice = strings(&["Hand-written for tests."]);
        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);
        assert_eq!(
            merged.merged.sources,
            vec![
                source("builtin", true, true, &[]),
                source("test_linestyles", true, false, &[]),
                source("test_survey", false, true, &["Hand-written for tests."]),
                source("test_symbols", true, false, &[]),
            ]
        );
        // A load is added to the session, which keeps its name.
        assert_eq!(merged.merged.name, "builtin");

        // A Replace by survey codes alone takes the rules the session brought
        // and leaves it its definitions; it is still the session it was.
        let codes = vec![loaded[1].clone()];
        let recoded = merge_customisation(&now, &codes, LoadMode::Replace);
        assert_eq!(
            recoded.merged.sources,
            vec![
                source("builtin", true, false, &[]),
                source("test_survey", false, true, &["Hand-written for tests."]),
            ]
        );
        assert_eq!(recoded.merged.name, "builtin");

        // A Replace that brought BOTH kinds leaves nothing of the session's
        // definitions or rules for its name to be about: the first loaded
        // customisation's name, description and notice are the session's now,
        // and only the loads are sources. Its notice is said once, as the
        // session's.
        loaded[0].description = String::from("Lines for tests");
        loaded[0].notice = strings(&["Every stroke drawn for this file."]);
        let replaced = merge_customisation(&now, &loaded, LoadMode::Replace);
        assert_eq!(replaced.merged.name, "test_linestyles");
        assert_eq!(replaced.merged.description, "Lines for tests");
        assert_eq!(
            replaced.merged.notice,
            strings(&["Every stroke drawn for this file."])
        );
        assert_eq!(
            replaced.merged.sources,
            vec![
                source("test_linestyles", true, false, &[]),
                source("test_survey", false, true, &["Hand-written for tests."]),
                source("test_symbols", true, false, &[]),
            ]
        );
    }

    #[test]
    fn a_loaded_customisation_that_lists_its_sources_keeps_its_own_notice() {
        // What a session writes of itself (Document::customisation, pinned by
        // CustomisationState.TheSessionAsOneCustomisationInstallsBackAsTheSame-
        // Session): its notice at the top level and its sources listed, itself
        // among them WITHOUT a notice. Every kept or exported customisation
        // has that shape, and its notice was dropped when it was merged into
        // another session.
        let now = current();
        let mut site = named("Site");
        define(
            &mut site,
            "SITE Peg",
            true,
            true,
            vec![move_to(0.0, 0.0), circle(1.0)],
        );
        rule(&mut site, feature("PG*", "PEGS", "", ""));
        site.notice = strings(&["For this site only."]);
        site.sources = vec![source("Site", true, true, &[])];

        // By hand: the session keeps its name and has no notice of its own;
        // "Site" follows it as a source, carrying the notice it came with.
        let one = vec![site.clone()];
        let merged = merge_customisation(&now, &one, LoadMode::Merge);
        assert!(merged.ok());
        assert_eq!(merged.merged.name, "builtin");
        assert!(merged.merged.notice.is_empty());
        assert_eq!(
            merged.merged.sources,
            vec![
                source("builtin", true, true, &[]),
                source("Site", true, true, &["For this site only."]),
            ]
        );

        // The second of a Replace that brought both kinds. The first becomes
        // the session: its notice is the session's, said once, and its entry
        // has none. The second is a source, and keeps its own.
        let mut first = named("First");
        define(
            &mut first,
            "FIRST Line",
            false,
            false,
            vec![move_to(0.0, 0.0), draw_to(1.0, 0.0)],
        );
        rule(&mut first, feature("FL*", "FIRST", "", ""));
        first.notice = strings(&["The first one's."]);
        first.sources = vec![source("First", true, true, &[])];
        let two = vec![first, site.clone()];
        let replaced = merge_customisation(&now, &two, LoadMode::Replace);
        assert!(replaced.ok());
        assert_eq!(replaced.merged.name, "First");
        assert_eq!(replaced.merged.notice, strings(&["The first one's."]));
        assert_eq!(
            replaced.merged.sources,
            vec![
                source("First", true, true, &[]),
                source("Site", true, true, &["For this site only."]),
            ]
        );

        // An entry of its name that already says something keeps that too,
        // and a line said in both places is said once.
        let mut twice = site.clone();
        twice.sources = vec![source(
            "Site",
            true,
            true,
            &["An earlier line.", "For this site only."],
        )];
        let said = vec![twice];
        assert_eq!(
            merge_customisation(&now, &said, LoadMode::Merge)
                .merged
                .sources,
            vec![
                source("builtin", true, true, &[]),
                source(
                    "Site",
                    true,
                    true,
                    &["For this site only.", "An earlier line."]
                ),
            ]
        );

        // Written under another name than any source it lists: nothing says
        // which of them the notice came with, so every one of them carries it
        // rather than none - ahead of what it said already.
        let mut renamed = site;
        renamed.name = String::from("Site for Others");
        renamed.sources = vec![
            source("NSW", true, true, &[]),
            source("roads", true, false, &["Roads notice."]),
        ];
        let other = vec![renamed];
        assert_eq!(
            merge_customisation(&now, &other, LoadMode::Merge)
                .merged
                .sources,
            vec![
                source("builtin", true, true, &[]),
                source("NSW", true, true, &["For this site only."]),
                source(
                    "roads",
                    true,
                    false,
                    &["For this site only.", "Roads notice."]
                ),
            ]
        );
    }

    #[test]
    fn a_session_with_no_name_yet_takes_the_first_loaded_customisations_name() {
        // Nothing installed: the first thing loaded is what the session is.
        let mut first = fixture_symbols();
        first.description = String::from("Marks");
        first.based_on = Some(CustomisationBase {
            name: String::from("Marks"),
            digest: String::from("0123456789abcdef"),
        });
        let loaded = vec![first.clone(), fixture_survey()];
        let merged = merge_customisation(&Customisation::default(), &loaded, LoadMode::Merge);
        assert!(merged.ok());
        assert_eq!(merged.merged.name, "test_symbols");
        assert_eq!(merged.merged.description, "Marks");
        assert_eq!(merged.merged.based_on, first.based_on);
        assert_eq!(
            merged.merged.sources,
            vec![
                source("test_symbols", true, false, &[]),
                source("test_survey", false, true, &[]),
            ]
        );
        assert_eq!(merged.merged.library.size(), 4);
        assert_eq!(merged.merged.map.size(), 11);
    }

    #[test]
    fn a_load_with_a_problem_installs_nothing_and_says_every_problem() {
        // Two faulty customisations in one load, built in code as no file
        // could be read: one whose name a project could not record, one whose
        // control codes spell two controls alike. Both are said, each naming
        // its customisation, and NOTHING of the load is in what would be
        // installed - not even the sound customisation loaded beside them.
        let now = current();
        let mut bad_name = fixture_symbols();
        bad_name.name = String::from("roads/2026");
        let mut bad_codes = named("codes");
        bad_codes.linework = Some(LineworkCodes {
            start: String::from("X"),
            end: String::from("x"),
            ..LineworkCodes::default()
        });
        let loaded = vec![fixture_linestyles(), bad_name, bad_codes];

        for mode in [LoadMode::Merge, LoadMode::Replace] {
            let merged = merge_customisation(&now, &loaded, mode);
            assert!(!merged.ok());
            assert_eq!(merged.problems.len(), 2, "{}", to_string(mode));
            // The one with no usable name is known by its place in the load.
            assert!(
                merged.problems[0].starts_with("customisation 2 of this load: its name: "),
                "{}",
                merged.problems[0]
            );
            assert!(
                merged.problems[1].starts_with("\"codes\": its linework codes: "),
                "{}",
                merged.problems[1]
            );
            assert!(merged.merged == now, "all or nothing");
            // The report still says what each would have done.
            assert_eq!(merged.loads.len(), 3);
            assert_eq!(merged.loads[0].added_definitions.len(), 3);
        }
    }

    #[test]
    fn a_definitions_source_and_what_a_load_is_based_on_are_judged_as_its_name_is() {
        // Built in code, as no file could be read. A definition whose source
        // is a path reaches the project's record as a name, and the store
        // refuses the record: every SAVE of the session then fails. A basedOn
        // whose digest is not one is refused by the kept file's writer: every
        // KEEP fails. Both are problems of the load that brought them, said
        // before anything is installed.
        let now = current();
        let mut roads = named("roads");
        let kerb = LineStyle {
            name: String::from("ROAD Kerb"),
            source: String::from("lib/roads"),
            strokes: vec![move_to(0.0, 0.0), draw_to(1.0, 0.0)],
            ..LineStyle::default()
        };
        assert!(roads.library.add(kerb).is_ok());
        roads.based_on = Some(CustomisationBase {
            name: String::from("NSW"),
            digest: String::from("0123"),
        });
        let loaded = vec![roads];

        let merged = merge_customisation(&now, &loaded, LoadMode::Merge);
        assert!(!merged.ok());
        assert_eq!(merged.problems.len(), 2);
        let of_source = "\"roads\": the source of its definition \"ROAD Kerb\": ";
        assert!(
            merged.problems[0].starts_with(of_source),
            "{}",
            merged.problems[0]
        );
        assert!(
            merged.problems[1].starts_with("\"roads\": its basedOn: "),
            "{}",
            merged.problems[1]
        );
        assert!(merged.merged == now, "all or nothing");
    }
}
