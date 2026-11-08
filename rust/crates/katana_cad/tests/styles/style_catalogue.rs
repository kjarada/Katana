// The style catalogue: what a linetype and a symbol picker offer (the lead's
// D2 and D3), what each library definition is, and the names a drawing uses
// that nothing defines (tests/cad/customisation/test_style_catalogue.cpp).
// Hand-built libraries only - the reference files are not in a worktree
// build, and the rules have to hold for any customisation.

use katana_cad::style_catalogue::{
    classify_definition, filter_choices, keep_current, linetype_choices, linetype_collisions,
    linetype_status, missing_names, symbol_choices, symbol_status,
};
use katana_cad::{
    CatalogueEntry, DefinitionKind, DefinitionSource, Document, MissingName, NameRole, NameStatus,
};
use katana_commands::{EntityAttributes, create_layer, create_line, create_linetype, create_style};
use katana_entity::{
    EnumName, Layer, LineStyle, Linetype, LinetypeElement, Stroke, StrokeOp, Style, StyleLibrary,
    StyleUnits, SurveyMap, SurveyRule, SurveySection, SurveySymbol,
};
use katana_geometry::Point2;

use crate::must;

// `symbol`: its customisation lists it as a symbol (LineStyle::symbol). Two of
// these definitions were once symbols because `source` held the word
// "symbol"; the flag says so now, and the sources only say where each came
// from.
fn definition(name: &str, at_vertices: bool, source: &str, group: &str, symbol: bool) -> LineStyle {
    LineStyle {
        name: String::from(name),
        at_vertices,
        source: String::from(source),
        group: String::from(group),
        symbol,
        units: StyleUnits::World,
        // One stroke: a definition with none is legal but not a realistic one.
        strokes: vec![Stroke {
            op: StrokeOp::Draw,
            point: Point2::new(1.0, 0.0),
            ..Stroke::default()
        }],
        ..LineStyle::default()
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

fn names(entries: &[CatalogueEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.name.clone()).collect()
}

fn find<'a>(entries: &'a [CatalogueEntry], name: &str) -> Option<&'a CatalogueEntry> {
    entries.iter().find(|entry| entry.name == name)
}

fn position(entries: &[CatalogueEntry], name: &str) -> usize {
    entries
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or(entries.len())
}

// The drawing and customisation every test here reads.
//
//   library   TEST Water Main     listed a linestyle, not vertex -> linestyle
//             TEST Bollard        listed a symbol, `mode vertex` -> symbol
//             TEST Manhole Cover  listed a symbol, NOT vertex    -> both (D3: the listing)
//             TREE Palm           other file, not vertex, named
//                                 by style Palm's symbol        -> both (D3: a style)
//             ELEC Pole           other file, not vertex, named
//                                 by a VertexSymbol rule        -> both (D3: the map)
//             fence               other file, not vertex; ALSO
//                                 a model linetype              -> a collision (D2)
//             circle              other file, `mode vertex`     -> hides the built-in
//   model     linetypes continuous, fence, dash
//             layer roads naming "MISSING Layer Linestyle"
//             styles Palm (ByLayer, symbol TREE Palm), Ghost (linetype
//             "NOPE Linestyle", symbol "NOPE Symbol Manhole"), Cross (symbol
//             cross, a built-in), Plain (linetype "0", the plain continuous line)
fn catalogue() -> Document {
    let mut document = Document::default();
    let mut library = StyleLibrary::default();
    for definition in [
        definition(
            "TEST Water Main",
            false,
            "linestyles_test.4d",
            "Services/WATR",
            false,
        ),
        definition("TEST Bollard", true, "symbols_test.4d", "Culture", true),
        definition(
            "TEST Manhole Cover",
            false,
            "SYMBOLS_Test.4d",
            "Services/SEWR",
            true,
        ),
        definition("TREE Palm", false, "extra.4d", "", false),
        definition("ELEC Pole", false, "extra.4d", "", false),
        definition("fence", false, "extra.4d", "", false),
        definition("circle", true, "extra.4d", "", false),
    ] {
        assert!(library.add(definition).is_ok());
    }
    document.set_style_library(library);

    let mut map = SurveyMap::default();
    let pole = SurveyRule {
        key: String::from("EP*"),
        section: SurveySection::VertexSymbol,
        symbol: Some(SurveySymbol {
            style: String::from("ELEC Pole"),
            ..SurveySymbol::default()
        }),
        ..SurveyRule::default()
    };
    assert!(map.add(pole).is_ok());
    document.set_survey_map(map);

    for name in ["fence", "dash"] {
        let linetype = Linetype {
            name: String::from(name),
            pattern: vec![
                LinetypeElement { length: 1.0 },
                LinetypeElement { length: -0.5 },
            ],
            ..Linetype::default()
        };
        must(&mut document, create_linetype(linetype));
    }
    let roads = Layer {
        name: String::from("roads"),
        linetype: String::from("MISSING Layer Linestyle"),
        ..Layer::default()
    };
    must(&mut document, create_layer(roads));
    must(
        &mut document,
        create_style(style("Palm", "ByLayer", "TREE Palm")),
    );
    must(
        &mut document,
        create_style(style("Ghost", "NOPE Linestyle", "NOPE Symbol Manhole")),
    );
    must(
        &mut document,
        create_style(style("Cross", "continuous", "cross")),
    );
    must(&mut document, create_style(style("Plain", "0", "")));
    document
}

mod catalogue {
    use super::*;

    #[test]
    fn a_definition_is_a_symbol_for_any_of_the_four_reasons_and_a_linestyle_when_not_at_vertices() {
        let document = catalogue();
        let bollard = classify_definition(&document, "TEST Bollard");
        assert!(bollard.symbol);
        assert!(
            !bollard.linestyle,
            "`mode vertex` is never a line pattern (D8)"
        );
        assert!(bollard.at_vertices);
        assert!(bollard.listed_as_symbol);

        // Not `mode vertex`, but its customisation lists it as a symbol. (It was
        // once the NAME of its source that said so, "symbol" in any case; the
        // definition says it itself now, so the expectation stands and the cause
        // is new.)
        let manhole = classify_definition(&document, "TEST Manhole Cover");
        assert_eq!(
            manhole,
            DefinitionKind {
                linestyle: true,
                symbol: true,
                at_vertices: false,
                named_by_survey_rule: false,
                named_by_style: false,
                listed_as_symbol: true,
            }
        );

        let palm = classify_definition(&document, "TREE Palm");
        assert!(palm.symbol && palm.named_by_style && palm.linestyle);
        assert!(!palm.listed_as_symbol);

        let pole = classify_definition(&document, "ELEC Pole");
        assert!(pole.symbol && pole.named_by_survey_rule);
        assert!(!pole.named_by_style);

        let watr = classify_definition(&document, "TEST Water Main");
        assert!(watr.linestyle);
        assert!(!watr.symbol);

        // A model linetype or a built-in shape is not a library definition.
        assert!(!classify_definition(&document, "dash").known());
        assert!(!classify_definition(&document, "cross").known());
    }

    #[test]
    fn a_definition_is_listed_as_a_symbol_by_its_customisation_and_not_by_the_name_of_its_source() {
        // The fixture above cannot tell the two apart: its symbols also come
        // from sources named "symbols". Here the two disagree, both ways. One
        // customisation holds linestyles and symbols under ONE name, so a
        // source's name says nothing: "Roadside Symbols" would otherwise make
        // every definition in it a symbol, and "NSW" none.
        let mut document = catalogue();
        let mut library = StyleLibrary::default();
        assert!(
            library
                .add(definition(
                    "KERB Line",
                    false,
                    "Roadside Symbols",
                    "",
                    false
                ))
                .is_ok()
        );
        assert!(
            library
                .add(definition("SIGN Post", false, "NSW", "", true))
                .is_ok()
        );
        document.set_style_library(library);

        let kerb = classify_definition(&document, "KERB Line");
        assert!(!kerb.listed_as_symbol);
        assert!(!kerb.symbol, "its source's name is not what makes a symbol");
        assert!(kerb.linestyle);

        let post = classify_definition(&document, "SIGN Post");
        assert!(post.listed_as_symbol);
        assert!(post.symbol);
        assert!(post.linestyle, "not `mode vertex`, so a linestyle as well");

        // And so in the picker: the post, not the kerb.
        let choices = symbol_choices(&document);
        assert!(find(&choices, "SIGN Post").is_some());
        assert!(find(&choices, "KERB Line").is_none());
    }

    #[test]
    fn the_linetype_picker_offers_by_layer_first_then_every_linestyle_sorted_with_case_folded() {
        // Non-vertex library definitions: TEST Water Main, TEST Manhole Cover,
        // TREE Palm, ELEC Pole, fence. Model linetypes: continuous, dash, fence
        // (the collision, listed once as the library's). Folded order:
        // continuous, dash, elec pole, fence, test manhole cover, test water
        // main, tree palm ("e" < "r": TEST before TREE).
        let document = catalogue();
        let choices = linetype_choices(&document, true);
        assert_eq!(
            names(&choices),
            vec![
                "ByLayer",
                "continuous",
                "dash",
                "ELEC Pole",
                "fence",
                "TEST Manhole Cover",
                "TEST Water Main",
                "TREE Palm"
            ]
        );
        assert_eq!(choices[0].source, DefinitionSource::BuiltIn);
        assert_eq!(
            choices[0].users.styles,
            vec![String::from("Palm")],
            "the styles that inherit their layer's linetype"
        );

        let fence = find(&choices, "fence").expect("fence is offered");
        assert_eq!(
            fence.source,
            DefinitionSource::Library,
            "the library's definition draws (D2)"
        );
        assert!(fence.collision);
        let watr = find(&choices, "TEST Water Main").expect("TEST Water Main is offered");
        assert_eq!(watr.source_file, "linestyles_test.4d");
        assert_eq!(watr.group, "Services/WATR");
        assert_eq!(watr.units, Some(StyleUnits::World));
        assert!(!watr.collision);
        assert_eq!(
            find(&choices, "dash").map(|entry| entry.source),
            Some(DefinitionSource::ModelLinetype)
        );

        // A layer's picker has no ByLayer: a layer is what it inherits from.
        let for_layer = linetype_choices(&document, false);
        assert_eq!(for_layer.len(), choices.len() - 1);
        assert_eq!(for_layer[0].name, "continuous");
    }

    #[test]
    fn the_symbol_picker_offers_every_d3_symbol_and_the_built_ins_a_library_does_not_hide() {
        let document = catalogue();
        let choices = symbol_choices(&document);
        // Library symbols: TEST Bollard, TEST Manhole Cover, TREE Palm, ELEC
        // Pole, circle (5). Built-ins: 16, less "circle", which the library
        // defines and so draws: 15. Neither TEST Water Main nor fence.
        assert_eq!(choices.len(), 20);
        assert!(find(&choices, "TEST Water Main").is_none());
        assert!(find(&choices, "fence").is_none());
        assert!(
            find(&choices, "TEST Manhole Cover").is_some(),
            "a NON-vertex definition listed as a symbol is a symbol"
        );
        let palm = find(&choices, "TREE Palm").expect("TREE Palm is offered");
        assert_eq!(palm.users.styles, vec![String::from("Palm")]);
        assert_eq!(
            find(&choices, "circle").map(|entry| entry.source),
            Some(DefinitionSource::Library)
        );
        let cross = find(&choices, "cross").expect("cross is offered");
        assert_eq!(cross.source, DefinitionSource::BuiltIn);
        assert_eq!(cross.users.styles, vec![String::from("Cross")]);
        assert!(find(&choices, "TEST Bollard").is_some_and(|entry| entry.at_vertices));

        // Sorted with case folded: "arrow" (built-in) before "dot" before "ELEC
        // Pole" before "TEST Bollard" - an upper-case name among lower-case ones
        // is placed by its letters, so "ELEC Pole" falls between "dot" and
        // "TEST Bollard" ("d" < "e" < "t"), where byte order would put both
        // ahead of them.
        assert!(position(&choices, "arrow") < position(&choices, "dot"));
        assert!(position(&choices, "dot") < position(&choices, "ELEC Pole"));
        assert!(position(&choices, "ELEC Pole") < position(&choices, "TEST Bollard"));
    }

    #[test]
    fn a_picker_keeps_the_current_value_it_cannot_offer_marked_rather_than_dropping_it() {
        // The real QT-02 fix: whatever the list holds, the value being edited
        // is shown, so saving the form cannot silently replace it.
        let document = catalogue();
        let choices = linetype_choices(&document, true);
        let kept = keep_current(choices.clone(), "OLD Linestyle");
        assert_eq!(kept.len(), choices.len() + 1);
        assert_eq!(kept[0].name, "OLD Linestyle");
        assert!(kept[0].missing);
        assert_eq!(kept[0].source, DefinitionSource::Undefined);

        assert_eq!(
            keep_current(choices.clone(), "dash").len(),
            choices.len(),
            "already offered"
        );
        assert_eq!(
            keep_current(choices.clone(), "").len(),
            choices.len(),
            "nothing to keep"
        );
        // Names are case-sensitive: "DASH" is not "dash", and is kept as itself.
        assert_eq!(keep_current(choices, "DASH")[0].name, "DASH");
    }

    #[test]
    fn a_search_folds_case_over_name_group_and_source_file() {
        let document = catalogue();
        let symbols = symbol_choices(&document);
        // "manhole" (built-in) and "TEST Manhole Cover", by name.
        assert_eq!(
            names(&filter_choices(&symbols, "MANHOLE")),
            vec!["manhole", "TEST Manhole Cover"]
        );
        // Both symbol-file definitions, by their file.
        assert_eq!(
            names(&filter_choices(&symbols, "symbols_test")),
            vec!["TEST Bollard", "TEST Manhole Cover"]
        );
        // By group.
        assert_eq!(
            names(&filter_choices(
                &linetype_choices(&document, true),
                "services/w"
            )),
            vec!["TEST Water Main"]
        );
        assert_eq!(filter_choices(&symbols, "").len(), symbols.len());
    }

    #[test]
    fn missing_names_are_the_ones_nothing_defines_with_who_uses_them_and_what_is_drawn_instead() {
        let document = catalogue();
        let missing = missing_names(&document);
        // Linetypes, ascending: the layer's and Ghost's. Not Palm's ByLayer,
        // Plain's "0" (the plain continuous line), Cross's continuous, nor any
        // library name. Then the one symbol: Ghost's. Not "cross" - a built-in
        // draws as itself (audit CAD-17's symptom) - nor TREE Palm, which is
        // defined.
        assert_eq!(missing.len(), 3);

        assert_eq!(missing[0].name, "MISSING Layer Linestyle");
        assert_eq!(missing[0].role, NameRole::Linetype);
        assert_eq!(missing[0].users.layers, vec![String::from("roads")]);
        assert_eq!(missing[0].fallback, "continuous");

        assert_eq!(missing[1].name, "NOPE Linestyle");
        assert_eq!(missing[1].users.styles, vec![String::from("Ghost")]);

        assert_eq!(missing[2].name, "NOPE Symbol Manhole");
        assert_eq!(missing[2].role, NameRole::Symbol);
        assert_eq!(missing[2].users.styles, vec![String::from("Ghost")]);
        // built_in_symbol_for: "manhole" is in the name, so the manhole shape.
        assert_eq!(missing[2].fallback, "manhole");
        // Nothing defines any of the three: none is merely the wrong kind.
        for name in &missing {
            assert_eq!(name.status, NameStatus::Undefined, "{}", name.name);
        }
    }

    #[test]
    fn each_names_status_is_what_the_viewport_draws_for_it() {
        let document = catalogue();
        // Linetypes. The plain names, in any case, name nothing (D4).
        for plain in ["", "ByLayer", "CONTINUOUS", "0", "1"] {
            assert_eq!(
                linetype_status(&document, plain, ""),
                NameStatus::Plain,
                "{plain}"
            );
        }
        assert_eq!(
            linetype_status(&document, "TEST Water Main", ""),
            NameStatus::Library
        );
        assert_eq!(
            linetype_status(&document, "dash", ""),
            NameStatus::Katana,
            "a model linetype"
        );
        assert_eq!(
            linetype_status(&document, "fence", ""),
            NameStatus::Library,
            "a collision: the library's linestyle is what draws (D2)"
        );
        // `mode vertex` only: resolve_linetype gives Solid, and the linetype
        // picker (linetype_choices, which the NamePicker lists) does not offer
        // it.
        assert_eq!(
            linetype_status(&document, "TEST Bollard", ""),
            NameStatus::NotALinestyle
        );
        assert!(find(&linetype_choices(&document, true), "TEST Bollard").is_none());
        assert_eq!(
            linetype_status(&document, "NOPE", ""),
            NameStatus::Undefined
        );
        // A style's own symbol as its linetype draws plain (D8), whatever kind
        // of definition the name is, and even when nothing defines it.
        assert_eq!(
            linetype_status(&document, "TEST Bollard", "TEST Bollard"),
            NameStatus::OwnSymbol
        );
        assert_eq!(
            linetype_status(&document, "TEST Manhole Cover", "TEST Manhole Cover"),
            NameStatus::OwnSymbol
        );
        assert_eq!(
            linetype_status(&document, "NOPE", "NOPE"),
            NameStatus::OwnSymbol
        );
        // ... but only its OWN symbol.
        assert_eq!(
            linetype_status(&document, "TEST Bollard", "TREE Palm"),
            NameStatus::NotALinestyle
        );

        // Symbols: any library definition, `mode vertex` or not (D3); else a
        // built-in shape; "circle" is defined, and the library's wins.
        assert_eq!(symbol_status(&document, ""), NameStatus::Plain);
        assert_eq!(
            symbol_status(&document, "TEST Bollard"),
            NameStatus::Library
        );
        assert_eq!(
            symbol_status(&document, "TEST Water Main"),
            NameStatus::Library
        );
        assert_eq!(symbol_status(&document, "circle"), NameStatus::Library);
        assert_eq!(symbol_status(&document, "cross"), NameStatus::Katana);
        assert_eq!(
            symbol_status(&document, "NOPE Symbol"),
            NameStatus::Undefined
        );
        assert_eq!(NameStatus::NotALinestyle.name(), "not a linestyle");
    }

    #[test]
    fn a_linetype_naming_only_a_vertex_symbol_is_missing_as_not_a_linestyle() {
        // TEST Bollard is `mode vertex`: a symbol, never a line pattern. A layer
        // and a style that give it as their LINETYPE draw solid, and the
        // NamePicker marks it "(not defined)", so missing_names says so too -
        // with why. "circle" is the same, and a built-in shape besides: as a
        // linetype that makes it no less solid.
        let mut document = catalogue();
        let posts = Layer {
            name: String::from("posts"),
            linetype: String::from("TEST Bollard"),
            ..Layer::default()
        };
        must(&mut document, create_layer(posts));
        must(
            &mut document,
            create_style(style("Bollard Line", "TEST Bollard", "")),
        );
        must(
            &mut document,
            create_style(style("Ring Line", "circle", "")),
        );

        let missing = missing_names(&document);
        // Linetypes ascending: MISSING Layer Linestyle, NOPE Linestyle, TEST
        // Bollard, circle ("M" < "N" < "T" < "c", by byte). Then the symbol.
        assert_eq!(missing.len(), 5);
        assert_eq!(missing[2].name, "TEST Bollard");
        assert_eq!(missing[2].role, NameRole::Linetype);
        assert_eq!(missing[2].status, NameStatus::NotALinestyle);
        assert_eq!(missing[2].users.layers, vec![String::from("posts")]);
        assert_eq!(missing[2].users.styles, vec![String::from("Bollard Line")]);
        assert_eq!(
            missing[2].fallback, "continuous",
            "what the viewport draws: solid"
        );
        assert_eq!(missing[3].name, "circle");
        assert_eq!(missing[3].status, NameStatus::NotALinestyle);
        assert_eq!(missing[3].users.styles, vec![String::from("Ring Line")]);
        assert_eq!(missing[4].name, "NOPE Symbol Manhole");
        assert_eq!(missing[4].role, NameRole::Symbol);
    }

    #[test]
    fn a_style_whose_linetype_is_its_own_symbol_is_the_importers_pattern_and_not_missing() {
        // What an archive import writes for a symbol string: style, linetype and
        // symbol all the symbol's name. resolve_line_pattern draws the line
        // plain and the symbol at every vertex (D8), so neither name is missing -
        // for a `mode vertex` symbol and for one that is not.
        let mut document = catalogue();
        must(
            &mut document,
            create_style(style("TEST Bollard", "TEST Bollard", "TEST Bollard")),
        );
        must(
            &mut document,
            create_style(style(
                "TEST Manhole Cover",
                "TEST Manhole Cover",
                "TEST Manhole Cover",
            )),
        );
        // An undefined symbol in the same pattern is missing once, as a SYMBOL:
        // its linetype still draws plain.
        must(
            &mut document,
            create_style(style("NOPE Post", "NOPE Post", "NOPE Post")),
        );

        let mut linetypes = Vec::new();
        let mut symbols = Vec::new();
        for name in missing_names(&document) {
            if name.role == NameRole::Linetype {
                linetypes.push(name.name);
            } else {
                symbols.push(name.name);
            }
        }
        assert_eq!(linetypes, vec!["MISSING Layer Linestyle", "NOPE Linestyle"]);
        assert_eq!(symbols, vec!["NOPE Post", "NOPE Symbol Manhole"]);
    }

    #[test]
    fn when_a_layer_also_names_a_styles_own_symbol_only_the_layers_lines_are_counted() {
        // Style "TEST Bollard" is the importer's pattern; layer "fences" gives
        // the same name as a plain linetype, which draws solid. Entities, in id
        // order: a on fences wearing the style, b on fences with no style, c on
        // layer 0 wearing the style. table_usage counts all three against the
        // linetype (each resolves to it); a and c draw it plain under the
        // symbol, so the missing name's lines are b alone.
        let mut document = catalogue();
        let fences = Layer {
            name: String::from("fences"),
            linetype: String::from("TEST Bollard"),
            ..Layer::default()
        };
        must(&mut document, create_layer(fences));
        must(
            &mut document,
            create_style(style("TEST Bollard", "TEST Bollard", "TEST Bollard")),
        );
        let attributes = |layer: &str, style: &str| EntityAttributes {
            layer: String::from(layer),
            style: String::from(style),
            color: None,
        };
        must(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                attributes("fences", "TEST Bollard"),
            ),
        );
        must(
            &mut document,
            create_line(
                Point2::new(0.0, 1.0),
                Point2::new(1.0, 1.0),
                attributes("fences", ""),
            ),
        );
        must(
            &mut document,
            create_line(
                Point2::new(0.0, 2.0),
                Point2::new(1.0, 2.0),
                attributes("0", "TEST Bollard"),
            ),
        );
        let ids = document.model().entities.ids();
        assert_eq!(ids.len(), 3);

        let missing = missing_names(&document);
        let bollard: &MissingName = missing
            .iter()
            .find(|name| name.name == "TEST Bollard")
            .expect("TEST Bollard is missing");
        assert_eq!(bollard.status, NameStatus::NotALinestyle);
        assert_eq!(bollard.users.layers, vec![String::from("fences")]);
        assert!(
            bollard.users.styles.is_empty(),
            "the style draws it plain (D8)"
        );
        assert_eq!(bollard.users.entities, 1);
        assert_eq!(bollard.users.first_entity, ids[1]);
    }

    #[test]
    fn a_name_that_is_both_a_model_linetype_and_a_library_linestyle_is_a_collision() {
        let mut document = catalogue();
        assert_eq!(linetype_collisions(&document), vec!["fence"]);
        // A vertex definition of the same name as a model linetype is not one:
        // a symbol is never drawn as a line pattern (D8), so the dashes stand.
        let mut library = document.style_library().clone();
        assert!(
            library
                .add(definition("dash", true, "extra.4d", "", false))
                .is_ok()
        );
        document.set_style_library(library);
        assert_eq!(linetype_collisions(&document), vec!["fence"]);
    }
}
