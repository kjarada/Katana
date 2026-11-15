// definition_users: what names a library definition, and so what is left
// naming nothing when it is removed (definition_users.rs;
// tests/cad/customisation/test_definition_users.cpp). The customisation and
// the drawing below are written out by hand, and every expected list is read
// off them in the comment beside it.
//
// The rules, by index (a rule has no identity but its place):
//   #0  WM*  feature           linestyle "TEST Water Main"
//   #1  AC*  symbol            symbol    "TEST Valve"
//   #2  GT*  feature           linestyle "TEST Valve" AND symbol "TEST Valve"
//   #3  WM*  attributes        symbol    "TEST Water Main"   (a member a rule of that
//                                                             kind is not expected to
//                                                             carry: the model allows it)
//   #4  KB*  feature           linestyle "TEST Kerb"
//   #5  *    feature           nothing
// The styles:
//   Marks   symbol   "TEST Valve"
//   Mains   linetype "TEST Water Main"
//   Both    linetype "TEST Valve", symbol "TEST Valve"
//   Other   linetype "test valve"      (another name: names compare with case)
// The layers (besides the default "0"):
//   services  linetype "TEST Water Main"
//   kerbs     linetype "TEST Kerb"

use katana_cad::Document;
use katana_cad::customisation_record::{RenamedDefinition, source_name_hash};
use katana_cad::definition_users::{
    DefinitionRule, DefinitionUsers, definition_users, definition_users_in,
};
use katana_commands::{CommandPtr, create_layer, create_linetype, create_style};
use katana_entity::{
    Layer, LineStyle, Linetype, LinetypeElement, Style, StyleLibrary, SurveyMap, SurveyRule,
    SurveySection, SurveySymbol,
};

fn define(library: &mut StyleLibrary, name: &str) {
    let definition = LineStyle {
        name: String::from(name),
        ..LineStyle::default()
    };
    if let Err(error) = library.add(definition) {
        panic!("{}", error.describe());
    }
}

fn rule(key: &str, section: SurveySection, linestyle: &str, symbol: Option<&str>) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        section,
        linestyle: String::from(linestyle),
        symbol: symbol.map(|style| SurveySymbol {
            style: String::from(style),
            ..SurveySymbol::default()
        }),
        ..SurveyRule::default()
    }
}

fn must(document: &mut Document, command: CommandPtr) {
    if let Err(error) = document.execute(command) {
        panic!("{}", error.describe());
    }
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

fn users_rule(
    index: usize,
    key: &str,
    section: SurveySection,
    linestyle: bool,
    symbol: bool,
) -> DefinitionRule {
    DefinitionRule {
        index,
        key: String::from(key),
        section,
        linestyle,
        symbol,
    }
}

fn style(name: &str, linetype: &str, symbol: &str) -> Style {
    Style {
        name: String::from(name),
        linetype: String::from(linetype),
        symbol: String::from(symbol),
        ..Style::default()
    }
}

fn layer(name: &str, linetype: &str) -> Layer {
    Layer {
        name: String::from(name),
        linetype: String::from(linetype),
        ..Layer::default()
    }
}

/// The customisation and the drawing of the file comment.
fn make_drawing(document: &mut Document) {
    let mut library = StyleLibrary::default();
    for name in ["TEST Water Main", "TEST Valve", "TEST Kerb", "TEST Unused"] {
        define(&mut library, name);
    }
    document.set_style_library(library);

    let mut map = SurveyMap::default();
    for each in [
        rule("WM*", SurveySection::Map, "TEST Water Main", None),
        rule("AC*", SurveySection::VertexSymbol, "", Some("TEST Valve")),
        rule("GT*", SurveySection::Map, "TEST Valve", Some("TEST Valve")),
        rule(
            "WM*",
            SurveySection::StringAttribute,
            "",
            Some("TEST Water Main"),
        ),
        rule("KB*", SurveySection::Map, "TEST Kerb", None),
        rule("*", SurveySection::Map, "", None),
    ] {
        if let Err(error) = map.add(each) {
            panic!("{}", error.describe());
        }
    }
    document.set_survey_map(map);

    for made in [
        style("Marks", "continuous", "TEST Valve"),
        style("Mains", "TEST Water Main", ""),
        style("Both", "TEST Valve", "TEST Valve"),
        style("Other", "test valve", ""),
    ] {
        must(document, create_style(made));
    }
    for made in [
        layer("services", "TEST Water Main"),
        layer("kerbs", "TEST Kerb"),
    ] {
        must(document, create_layer(made));
    }
}

mod definition_users {
    use super::*;

    #[test]
    fn the_rules_styles_and_layers_naming_a_definition_are_listed_each_by_how_it_names_it() {
        let mut document = Document::default();
        make_drawing(&mut document);

        // "TEST Valve": rule #1 draws it, rule #2 names it both ways; Both
        // names it as a linetype, and Both and Marks as a symbol (ascending: B
        // before M). No layer names it. "test valve" is another name.
        let valve: DefinitionUsers = definition_users(&document, "TEST Valve");
        assert_eq!(
            valve.rules,
            vec![
                users_rule(1, "AC*", SurveySection::VertexSymbol, false, true),
                users_rule(2, "GT*", SurveySection::Map, true, true),
            ]
        );
        assert_eq!(valve.linetype_styles, strings(&["Both"]));
        assert_eq!(valve.symbol_styles, strings(&["Both", "Marks"]));
        assert!(valve.layers.is_empty());
        assert!(!valve.empty());

        // "TEST Water Main": rule #0 as a linestyle, rule #3 - an attributes
        // rule carrying a symbol - as a symbol; the style Mains and the layer
        // services.
        let main = definition_users(&document, "TEST Water Main");
        assert_eq!(
            main.rules,
            vec![
                users_rule(0, "WM*", SurveySection::Map, true, false),
                users_rule(3, "WM*", SurveySection::StringAttribute, false, true),
            ]
        );
        assert_eq!(main.linetype_styles, strings(&["Mains"]));
        assert!(main.symbol_styles.is_empty());
        assert_eq!(main.layers, strings(&["services"]));
    }

    #[test]
    fn a_definition_nothing_names_has_no_users_and_neither_has_no_name() {
        let mut document = Document::default();
        make_drawing(&mut document);

        assert!(definition_users(&document, "TEST Unused").empty());
        assert!(
            definition_users(&document, "TEST Unused")
                .describe()
                .is_empty()
        );
        // A name no library defines is still only a name: nothing names this
        // one.
        assert!(definition_users(&document, "TEST Nowhere").empty());
        // No name is not a name, although rule #5 has an empty linestyle and
        // the style Mains an empty symbol.
        assert!(definition_users(&document, "").empty());
        // An empty drawing with no customisation.
        assert!(definition_users(&Document::default(), "TEST Valve").empty());
    }

    #[test]
    fn names_compare_with_regard_to_case() {
        let mut document = Document::default();
        make_drawing(&mut document);

        // Only the style Other spells it so, as its linetype.
        let lower = definition_users(&document, "test valve");
        assert!(lower.rules.is_empty());
        assert_eq!(lower.linetype_styles, strings(&["Other"]));
        assert!(lower.symbol_styles.is_empty());
        assert!(lower.layers.is_empty());
    }

    #[test]
    fn each_user_is_described_on_a_line_of_its_own_in_the_order_of_the_lists() {
        let mut document = Document::default();
        make_drawing(&mut document);

        assert_eq!(
            definition_users(&document, "TEST Valve").describe(),
            strings(&[
                "rule #1 AC* (symbol) draws it as its symbol",
                "rule #2 GT* (feature) names it as its linestyle and its symbol",
                "style \"Both\" names it as its linetype",
                "style \"Both\" draws it as its symbol",
                "style \"Marks\" draws it as its symbol",
            ])
        );
        assert_eq!(
            definition_users(&document, "TEST Kerb").describe(),
            strings(&[
                "rule #4 KB* (feature) names it as its linestyle",
                "layer \"kerbs\" names it as its linetype",
            ])
        );
    }

    #[test]
    fn an_earlier_name_of_a_renamed_definition_counts_until_the_library_defines_that_name_itself() {
        // "OLD Valve" is the name "TEST Valve" had before it was renamed. A
        // style and a rule still give the earlier name, and the resolver draws
        // them with "TEST Valve" (find_definition), so removing it would leave
        // them drawn as a stand-in: they use it.
        let renamed = [RenamedDefinition {
            former_name: source_name_hash("OLD Valve"),
            now: "TEST Valve",
        }];
        let mut document = Document::default();
        make_drawing(&mut document);
        let old = Style {
            name: String::from("Earlier"),
            symbol: String::from("OLD Valve"),
            ..Style::default()
        };
        must(&mut document, create_style(old));
        let mut map = document.survey_map().clone();
        assert!(
            map.add(rule(
                "OV*",
                SurveySection::VertexSymbol,
                "",
                Some("OLD Valve")
            ))
            .is_ok()
        );
        document.set_survey_map(map);

        let users = definition_users_in(&document, "TEST Valve", &renamed);
        // Rules #1 and #2 as before, and the new rule, #6; the style Earlier
        // sorts between Both and Marks.
        assert_eq!(users.rules.len(), 3);
        assert_eq!(
            users.rules.last(),
            Some(&users_rule(
                6,
                "OV*",
                SurveySection::VertexSymbol,
                false,
                true
            ))
        );
        assert_eq!(users.symbol_styles, strings(&["Both", "Earlier", "Marks"]));
        // Without the rename nothing ties the earlier name to it.
        assert_eq!(
            definition_users_in(&document, "TEST Valve", &[]).symbol_styles,
            strings(&["Both", "Marks"])
        );
        // The earlier name's own users are the two that spell it, whatever is
        // renamed.
        assert_eq!(
            definition_users_in(&document, "OLD Valve", &renamed).symbol_styles,
            strings(&["Earlier"])
        );

        // Once the library holds a definition of the earlier name itself, that
        // one is what is drawn, and "TEST Valve" is no longer used through it.
        let mut library = document.style_library().clone();
        define(&mut library, "OLD Valve");
        document.set_style_library(library);
        let after = definition_users_in(&document, "TEST Valve", &renamed);
        assert_eq!(after.rules.len(), 2);
        assert_eq!(after.symbol_styles, strings(&["Both", "Marks"]));
    }

    #[test]
    fn what_else_answers_to_the_name_is_said_beside_its_users_and_is_not_one_of_them() {
        let mut document = Document::default();
        make_drawing(&mut document);

        // Only the library holds "TEST Kerb"; rule #4 and the layer kerbs name
        // it as a linetype and nothing as a symbol.
        let kerb = definition_users(&document, "TEST Kerb");
        assert!(!kerb.drawing_linetype);
        assert!(!kerb.built_in_shape);
        assert!(kerb.named_as_linetype());
        assert!(!kerb.named_as_symbol());
        // "TEST Valve": rule #2 names it as a linestyle, rule #1 draws it.
        let valve = definition_users(&document, "TEST Valve");
        assert!(valve.named_as_linetype());
        assert!(valve.named_as_symbol());
        let unused = definition_users(&document, "TEST Unused");
        assert!(!unused.named_as_linetype());
        assert!(!unused.named_as_symbol());

        // The DRAWING holds a linetype "Fence" of its own and a layer names
        // it; no library definition has that name. The layer is drawn with the
        // drawing's dashes (decision D2) - not as a plain line, which is what
        // naming nothing would mean - and a definition made under the name
        // would take the layer from them.
        let fence = Linetype {
            name: String::from("Fence"),
            pattern: vec![
                LinetypeElement { length: 2.0 },
                LinetypeElement { length: -1.0 },
            ],
            ..Linetype::default()
        };
        must(&mut document, create_linetype(fence.clone()));
        must(&mut document, create_layer(layer("fences", "Fence")));
        let fence_users = definition_users(&document, "Fence");
        assert!(fence_users.drawing_linetype);
        assert!(!fence_users.built_in_shape);
        assert_eq!(fence_users.layers, strings(&["fences"]));
        // The fact is not a user: the layer alone is listed, and with no layer
        // there would be nothing to list.
        assert_eq!(
            fence_users.describe(),
            strings(&["layer \"fences\" names it as its linetype"])
        );
        let lone = Linetype {
            name: String::from("Lone"),
            pattern: fence.pattern,
            ..Linetype::default()
        };
        must(&mut document, create_linetype(lone));
        let lone_users = definition_users(&document, "Lone");
        assert!(lone_users.drawing_linetype);
        assert!(lone_users.empty());
        assert!(lone_users.describe().is_empty());

        // "circle" is one of the built-in shapes, and a style draws its points
        // with it: with no library definition of the name they are drawn as
        // that shape, not as a stand-in for a missing one.
        let rings = Style {
            name: String::from("Rings"),
            symbol: String::from("circle"),
            ..Style::default()
        };
        must(&mut document, create_style(rings));
        let circle = definition_users(&document, "circle");
        assert!(circle.built_in_shape);
        assert!(!circle.drawing_linetype);
        assert_eq!(circle.symbol_styles, strings(&["Rings"]));
        assert!(circle.named_as_symbol());
        assert!(!circle.named_as_linetype());
    }
}
