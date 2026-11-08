// Applying a survey code to a drawing (PLAN.MD 20.3, slice 4;
// tests/cad/test_survey_coding.cpp).

use std::sync::Arc;

use katana_cad::style_catalogue::missing_names;
use katana_cad::survey_coding::{apply_survey_codes, customisation_coverage, is_plain_linestyle};
use katana_cad::{
    Document, NameStatus, SurveyCodingOptions, SurveyCodingReport, SurveyStyleOutcome,
};
use katana_commands::{
    EntityAttributes, create_entities, create_layer, create_line, create_point, create_style,
    rename_style, set_entity_layer, set_entity_property, update_style,
};
use katana_entity::tables::{CONTINUOUS_LINETYPE, DEFAULT_LAYER_NAME};
use katana_entity::{
    Color, Entity, EntityId, Geometry, Layer, LineStyle, PointGeometry, PropertyValue, Stroke,
    StrokeOp, Style, StyleLibrary, SurveyAttribute, SurveyBreakline, SurveyMap, SurveyMatchKind,
    SurveyRule, SurveySection, SurveySymbol,
};
use katana_geometry::Point2;

fn symbol(style: &str, colour: &str, size: f64) -> SurveySymbol {
    SurveySymbol {
        style: String::from(style),
        colour: String::from(colour),
        size,
        rotation: 0.0,
        offset: 0.0,
        raise: 0.0,
    }
}

fn attribute(kind: &str, name: &str, value: &str) -> SurveyAttribute {
    SurveyAttribute {
        r#type: String::from(kind),
        name: String::from(name),
        value: String::from(value),
    }
}

// The rules the reference mapfile gives these codes, cut down.
fn water_and_bollards() -> SurveyMap {
    let mut map = SurveyMap::default();
    let main = SurveyRule {
        key: String::from("WM*"),
        model: String::from("SURVEY SERVICES"),
        colour: String::from("sui water potable"),
        linestyle: String::from("WATR Main"),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    };
    assert!(map.add(main).is_ok());

    let bollard = SurveyRule {
        key: String::from("AC*"),
        model: String::from("SURVEY DETAIL"),
        section: SurveySection::VertexSymbol,
        symbol: Some(symbol("CULT Bollard", "white", 1.5)),
        ..SurveyRule::default()
    };
    assert!(map.add(bollard).is_ok());

    let everything = SurveyRule {
        key: String::from("*"),
        section: SurveySection::StringAttribute,
        attributes: vec![
            attribute("text", "DepthLocation", "Top of Pipe"),
            attribute("text", "Diameter", "$PipeDiameter"),
        ],
        ..SurveyRule::default()
    };
    assert!(map.add(everything).is_ok());
    map
}

fn coded(map: SurveyMap) -> Document {
    let mut document = Document::default();
    document.set_survey_map(map);
    document
}

fn add_coded_point(document: &mut Document, x: f64, y: f64, code: &str) -> EntityId {
    assert!(
        document
            .execute(create_point(Point2::new(x, y), EntityAttributes::default()))
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

fn last_id(document: &Document) -> EntityId {
    let ids = document.model().entities.ids();
    ids[ids.len() - 1]
}

fn first_id(document: &Document) -> EntityId {
    document.model().entities.ids()[0]
}

fn entity(document: &Document, id: EntityId) -> &Entity {
    document.model().entities.find(id).expect("the entity")
}

fn text_property(entity: &Entity, key: &str) -> String {
    match entity.properties.get(key) {
        Some(PropertyValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

fn plan(document: &Document, options: &SurveyCodingOptions) -> SurveyCodingReport {
    let mut report = SurveyCodingReport::default();
    if let Err(error) = apply_survey_codes(document, options, Some(&mut report)) {
        panic!("{}", error.describe());
    }
    report
}

// The shapes the reference mapfile gives these codes, cut down: every symbol
// code ALSO has a map_data rule saying linestyle "0"; the text codes 1* and
// 2* both say linestyle "0" in different colours; and one real linestyle is
// used in two colours.
fn real_shapes() -> SurveyMap {
    let mut map = SurveyMap::default();
    let mut map_data = |key: &str, model: &str, colour: &str, linestyle: &str| {
        let rule = SurveyRule {
            key: String::from(key),
            model: String::from(model),
            colour: String::from(colour),
            linestyle: String::from(linestyle),
            ..SurveyRule::default()
        };
        assert!(map.add(rule).is_ok());
    };
    map_data("AC*", "SURVEY DETAIL", "white", "0");
    map_data("SV*", "SURVEY DETAIL", "red", "0");
    map_data("1*", "SURVEY TEXT", "yellow", "0");
    map_data("2*", "SURVEY TEXT", "cyan", "0");
    map_data(
        "TS*",
        "SURVEY DETAIL",
        "Green",
        "TOPO Timber or Scrub Scattered",
    );
    map_data(
        "TD*",
        "SURVEY DETAIL",
        "Dark Green",
        "TOPO Timber or Scrub Scattered",
    );
    map_data("WM*", "SURVEY SERVICES", "sui water potable", "WATR Main");
    for (key, style, size) in [
        ("AC*", "CULT Bollard", 1.5),
        ("SV*", "SEWR Manhole Cover", 1.0),
    ] {
        let rule = SurveyRule {
            key: String::from(key),
            section: SurveySection::VertexSymbol,
            symbol: Some(symbol(style, "", size)),
            ..SurveyRule::default()
        };
        assert!(map.add(rule).is_ok());
    }
    map
}

// A colour table of the test's own, so that what is expected does not hang on
// the one in the archive layer, which cad may not see anyway.
fn test_colour(name: &str) -> Option<Color> {
    match name {
        "white" => Some(Color::new(255, 255, 255, 255)),
        "red" => Some(Color::new(255, 0, 0, 255)),
        "yellow" => Some(Color::new(255, 255, 0, 255)),
        "cyan" => Some(Color::new(0, 255, 255, 255)),
        "Green" => Some(Color::new(0, 255, 0, 255)),
        "Dark Green" => Some(Color::new(0, 100, 0, 255)),
        _ => None, // "sui water potable" is unknown here, as it is to Katana
    }
}

fn with_colours() -> SurveyCodingOptions {
    SurveyCodingOptions {
        colour_of: Some(Arc::new(test_colour)),
        ..SurveyCodingOptions::default()
    }
}

fn style_of(document: &Document, id: EntityId) -> Style {
    let entity = document
        .model()
        .entities
        .find(id)
        .unwrap_or_else(|| panic!("no entity {id}"));
    document
        .model()
        .styles
        .find(&entity.style)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "entity {id} has style \"{}\", which does not exist",
                entity.style
            )
        })
}

fn apply(document: &mut Document, options: &SurveyCodingOptions) -> SurveyCodingReport {
    let mut report = SurveyCodingReport::default();
    let command = match apply_survey_codes(document, options, Some(&mut report)) {
        Ok(command) => command,
        Err(error) => panic!("{}", error.describe()),
    };
    if let Some(command) = command {
        assert!(document.execute(command).is_ok());
    }
    report
}

fn add_style(document: &mut Document, style: Style) {
    assert!(document.execute(create_style(style)).is_ok());
}

fn imported_point(document: &mut Document, style: &str, name: &str) -> EntityId {
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(5.0, 5.0),
        }),
        style: String::from(style),
        ..Entity::default()
    };
    point
        .metadata
        .insert(String::from("12d.name"), PropertyValue::from(name));
    assert!(document.execute(create_entities(vec![point])).is_ok());
    last_id(document)
}

mod survey_coding {
    use super::*;

    #[test]
    fn a_coded_point_gets_the_model_the_style_and_the_attributes_the_mapfile_gives_it() {
        let mut document = coded(water_and_bollards());
        let water = add_coded_point(&mut document, 0.0, 0.0, "WM01");
        let bollard = add_coded_point(&mut document, 10.0, 0.0, "AC07");

        let mut report = SurveyCodingReport::default();
        let command = apply_survey_codes(
            &document,
            &SurveyCodingOptions::default(),
            Some(&mut report),
        )
        .expect("planned")
        .expect("there is something to do");
        assert!(document.execute(command).is_ok());

        assert_eq!(report.coded, 2);
        assert_eq!(report.matched, 2);
        assert!(report.unmatched_codes.is_empty());
        assert_eq!(
            report.layers_created,
            vec!["SURVEY DETAIL", "SURVEY SERVICES"]
        );
        assert_eq!(report.styles_created, vec!["CULT Bollard", "WATR Main"]);

        let first = entity(&document, water);
        assert_eq!(
            first.layer, "SURVEY SERVICES",
            "a rule's model is Katana's layer"
        );
        assert_eq!(first.style, "WATR Main");
        // The `*` rule's attribute reaches both, because attributes accumulate.
        assert_eq!(text_property(first, "DepthLocation"), "Top of Pipe");
        // ... but the one whose value names another attribute is left, and
        // said so.
        assert!(!first.properties.contains_key("Diameter"));
        assert_eq!(report.deferred_attributes, 2, "one per coded entity");

        let second = entity(&document, bollard);
        assert_eq!(second.layer, "SURVEY DETAIL");
        assert_eq!(second.style, "CULT Bollard");
        let style = document
            .model()
            .styles
            .find("CULT Bollard")
            .expect("the style");
        assert_eq!(
            style.symbol, "CULT Bollard",
            "a symbol name resolved when it is drawn"
        );
        assert_eq!(style.symbol_size, 1.5);
        assert_eq!(
            document
                .model()
                .styles
                .find("WATR Main")
                .map(|style| style.linetype.as_str()),
            Some("WATR Main")
        );
    }

    #[test]
    fn it_is_one_undo_rather_than_one_per_entity() {
        let mut document = coded(water_and_bollards());
        for i in 0..20_i32 {
            add_coded_point(&mut document, f64::from(i), 0.0, &format!("WM{i}"));
        }
        let layer_before = entity(&document, first_id(&document)).layer.clone();

        let command = apply_survey_codes(&document, &SurveyCodingOptions::default(), None)
            .expect("planned")
            .expect("a command");
        assert!(document.execute(command).is_ok());
        assert_eq!(
            entity(&document, first_id(&document)).layer,
            "SURVEY SERVICES"
        );

        assert!(document.undo().is_ok());
        assert_eq!(
            entity(&document, first_id(&document)).layer,
            layer_before,
            "one undo puts all twenty back"
        );
        assert!(
            !document.model().layers.contains("SURVEY SERVICES"),
            "and takes the layer it created with them"
        );
    }

    #[test]
    fn a_code_only_the_star_rule_answers_is_reported_as_fallback_only_rather_than_as_matched() {
        // Audit CAD-05. Every code meets the `*` attribute rule, so "some rule
        // matched" is true of a typo too. ZZ99 and ZZ98 get only the `*`
        // attributes - no model, no linestyle, no symbol - and must be named,
        // not counted with WM01 as codes the mapfile has a rule for.
        let mut document = coded(water_and_bollards());
        add_coded_point(&mut document, 0.0, 0.0, "WM01");
        add_coded_point(&mut document, 1.0, 0.0, "ZZ99");
        add_coded_point(&mut document, 2.0, 0.0, "ZZ98");

        let report = plan(&document, &SurveyCodingOptions::default());
        assert_eq!(report.coded, 3);
        assert_eq!(report.matched, 1, "only WM01 has a rule of its own");
        assert_eq!(report.fallback_only, 2);
        assert_eq!(report.fallback_only_codes, vec!["ZZ98", "ZZ99"]);
        assert!(
            report.unmatched_codes.is_empty(),
            "a rule did match them: the bare `*`"
        );
        assert_eq!(report.layers_created, vec!["SURVEY SERVICES"]);
    }

    #[test]
    fn a_code_no_rule_at_all_matches_is_listed_as_unmatched() {
        let mut map = SurveyMap::default();
        let main = SurveyRule {
            key: String::from("WM*"),
            model: String::from("SURVEY SERVICES"),
            ..SurveyRule::default()
        };
        assert!(map.add(main).is_ok());
        let mut document = coded(map);
        add_coded_point(&mut document, 0.0, 0.0, "WM01");
        add_coded_point(&mut document, 1.0, 0.0, "ZZ99");

        let report = plan(&document, &SurveyCodingOptions::default());
        assert_eq!(report.coded, 2);
        assert_eq!(report.matched, 1);
        assert_eq!(report.fallback_only, 0);
        assert_eq!(report.unmatched_codes, vec!["ZZ99"]);
    }

    #[test]
    fn an_entity_with_no_code_or_a_number_for_one_is_left_alone() {
        let mut document = coded(water_and_bollards());
        assert!(
            document
                .execute(create_point(
                    Point2::new(0.0, 0.0),
                    EntityAttributes::default()
                ))
                .is_ok()
        );
        let plain = last_id(&document);
        // A number in the code property is a measurement someone named badly.
        assert!(
            document
                .execute(set_entity_property(
                    vec![plain],
                    "code",
                    PropertyValue::from(1.5)
                ))
                .is_ok()
        );
        let mut report = SurveyCodingReport::default();
        let command = apply_survey_codes(
            &document,
            &SurveyCodingOptions::default(),
            Some(&mut report),
        )
        .expect("planned");
        assert_eq!(report.coded, 0);
        assert!(command.is_none(), "nothing to do is not an error");
    }

    #[test]
    fn a_name_the_library_does_not_define_is_still_recorded_and_is_reported() {
        // A customisation need not be self-contained. The name is what the rules
        // say this is, so it is kept and draws plainly until a library defines
        // it.
        let mut document = coded(water_and_bollards());
        add_coded_point(&mut document, 0.0, 0.0, "WM01");

        let report = apply(&mut document, &SurveyCodingOptions::default());
        assert_eq!(report.missing_definitions, vec!["WATR Main"]);
        assert_eq!(
            document
                .model()
                .styles
                .find("WATR Main")
                .map(|style| style.linetype.as_str()),
            Some("WATR Main")
        );

        // With the library loaded there is nothing missing.
        let mut library = StyleLibrary::default();
        let main = LineStyle {
            name: String::from("WATR Main"),
            strokes: vec![Stroke {
                op: StrokeOp::Move,
                point: Point2::new(0.0, 0.0),
                ..Stroke::default()
            }],
            ..LineStyle::default()
        };
        assert!(library.add(main).is_ok());
        let mut second = coded(water_and_bollards());
        second.set_style_library(library);
        add_coded_point(&mut second, 0.0, 0.0, "WM01");
        let clean = plan(&second, &SurveyCodingOptions::default());
        assert!(clean.missing_definitions.is_empty());
    }

    #[test]
    fn the_colour_callback_decides_the_colour_and_without_one_none_is_guessed() {
        let mut document = coded(water_and_bollards());
        add_coded_point(&mut document, 0.0, 0.0, "WM01");

        let mut options = SurveyCodingOptions::default();
        apply(&mut document, &options);
        let style = document
            .model()
            .styles
            .find("WATR Main")
            .expect("the style");
        assert!(
            style.color.is_none(),
            "no resolver, so the colour is left alone"
        );
        assert_eq!(
            style.description, "sui water potable",
            "the colour name is kept whatever Katana makes of it"
        );

        let mut second = coded(water_and_bollards());
        add_coded_point(&mut second, 0.0, 0.0, "WM01");
        options.colour_of = Some(Arc::new(|name: &str| {
            if name == "sui water potable" {
                Some(Color::new(0, 128, 255, 255))
            } else {
                None
            }
        }));
        apply(&mut second, &options);
        let resolved = second.model().styles.find("WATR Main").expect("the style");
        assert_eq!(resolved.color.map(|colour| colour.b), Some(255));
    }

    #[test]
    fn only_the_entities_asked_for_are_touched() {
        let mut document = coded(water_and_bollards());
        let first = add_coded_point(&mut document, 0.0, 0.0, "WM01");
        add_coded_point(&mut document, 1.0, 0.0, "WM02");

        let options = SurveyCodingOptions {
            ids: vec![first],
            ..SurveyCodingOptions::default()
        };
        let report = apply(&mut document, &options);
        assert_eq!(report.coded, 1);
        assert_eq!(entity(&document, first).layer, "SURVEY SERVICES");
        assert_eq!(
            entity(&document, last_id(&document)).layer,
            DEFAULT_LAYER_NAME
        );
    }

    #[test]
    fn a_points_linework_controls_are_left_out_of_the_code_it_is_looked_up_by() {
        // "PABB ST" is the string PABB with a start control, as a controller
        // writes it. The exact key PABB must code it: the whole field, blank
        // included, matches no key. A prefix key hid this ("KB1 ST" still begins
        // with KB), which is why the key here is exact.
        let mut map = SurveyMap::default();
        let bollard = SurveyRule {
            key: String::from("PABB"),
            model: String::from("SURVEY DETAIL"),
            ..SurveyRule::default()
        };
        assert!(map.add(bollard).is_ok());
        let mut document = coded(map);
        let started = add_coded_point(&mut document, 0.0, 0.0, "PABB ST");
        let lower_case = add_coded_point(&mut document, 1.0, 0.0, "PABB st JPN 12");
        let plain = add_coded_point(&mut document, 2.0, 0.0, "PABB");
        // A line is not a field point: its code is looked up as it is written.
        assert!(
            document
                .execute(create_line(
                    Point2::new(0.0, 5.0),
                    Point2::new(9.0, 5.0),
                    EntityAttributes::default()
                ))
                .is_ok()
        );
        let line = last_id(&document);
        assert!(
            document
                .execute(set_entity_property(
                    vec![line],
                    "code",
                    PropertyValue::from("PABB ST")
                ))
                .is_ok()
        );

        let report = apply(&mut document, &SurveyCodingOptions::default());
        // By hand: three points reduce to PABB, which the exact key matches; the
        // line keeps "PABB ST", which nothing matches. Four coded, three
        // matched, two rows in name order ("PABB" < "PABB ST").
        assert_eq!(report.coded, 4);
        assert_eq!(report.matched, 3);
        assert_eq!(report.unmatched_codes, vec!["PABB ST"]);
        assert_eq!(report.codes.len(), 2);
        assert_eq!(
            report.codes[0].code, "PABB",
            "reported under the code it was looked up by"
        );
        assert_eq!(report.codes[0].entities, 3);
        assert_eq!(report.codes[1].code, "PABB ST");
        for id in [started, lower_case, plain] {
            assert_eq!(entity(&document, id).layer, "SURVEY DETAIL", "{id}");
        }
        assert_eq!(entity(&document, line).layer, DEFAULT_LAYER_NAME);
    }
}

// ---- which style a code gets (decision D4) ----------------------------------

mod survey_coding_styles {
    use super::*;

    #[test]
    fn symbol_and_text_codes_on_linestyle_zero_each_keep_their_own_symbol_and_colour() {
        // They all say linestyle "0". Named after the linestyle, they became ONE
        // style "0" built from whichever point came first: one symbol and one
        // colour for all four.
        let mut document = coded(real_shapes());
        let bollard = add_coded_point(&mut document, 0.0, 0.0, "AC01");
        let manhole = add_coded_point(&mut document, 1.0, 0.0, "SV01");
        let yellow_text = add_coded_point(&mut document, 2.0, 0.0, "101");
        let cyan_text = add_coded_point(&mut document, 3.0, 0.0, "201");

        let report = apply(&mut document, &with_colours());

        assert!(
            !document.model().styles.contains("0"),
            "\"0\" is a plain line, not a name"
        );
        let first = style_of(&document, bollard);
        assert_eq!(first.symbol, "CULT Bollard");
        assert_eq!(first.symbol_size, 1.5);
        assert_eq!(first.linetype, CONTINUOUS_LINETYPE);
        assert_eq!(first.color, Some(Color::new(255, 255, 255, 255)));
        let second = style_of(&document, manhole);
        assert_eq!(second.symbol, "SEWR Manhole Cover");
        assert_eq!(second.color, Some(Color::new(255, 0, 0, 255)));
        assert_eq!(
            style_of(&document, yellow_text).color,
            Some(Color::new(255, 255, 0, 255))
        );
        assert_eq!(
            style_of(&document, cyan_text).color,
            Some(Color::new(0, 255, 255, 255))
        );
        assert!(style_of(&document, cyan_text).symbol.is_empty());

        // Named after the symbol where there is one, else "Plain". The two text
        // codes are both plain lines with no symbol, so both want "Plain".
        // Appearances are named in a fixed order - linestyle, symbol, size,
        // then colour, whose identity is its RGB as "#RRGGBB" - and cyan
        // "#00FFFF" sorts before yellow "#FFFF00", so cyan takes "Plain" and
        // yellow gets its colour name added.
        assert_eq!(
            report.styles_created,
            vec![
                "CULT Bollard",
                "Plain",
                "Plain (yellow)",
                "SEWR Manhole Cover"
            ]
        );
        assert_eq!(entity(&document, cyan_text).style, "Plain");
        assert_eq!(entity(&document, yellow_text).style, "Plain (yellow)");
    }

    #[test]
    fn a_linestyle_used_in_two_colours_gives_two_styles_named_the_same_whichever_point_comes_first()
    {
        // "TOPO Timber or Scrub Scattered" comes in Green and in Dark Green in
        // each reference mapfile. In the fixed order Dark Green "#006400" sorts
        // before Green "#00FF00", so Dark Green takes the bare name.
        let name = "TOPO Timber or Scrub Scattered";
        for green_first in [true, false] {
            let mut document = coded(real_shapes());
            let (green, dark) = if green_first {
                let green = add_coded_point(&mut document, 0.0, 0.0, "TS01");
                let dark = add_coded_point(&mut document, 1.0, 0.0, "TD01");
                (green, dark)
            } else {
                let dark = add_coded_point(&mut document, 1.0, 0.0, "TD01");
                let green = add_coded_point(&mut document, 0.0, 0.0, "TS01");
                (green, dark)
            };
            let report = apply(&mut document, &with_colours());

            let how = if green_first {
                "green first"
            } else {
                "dark green first"
            };
            assert_eq!(
                report.styles_created,
                vec![String::from(name), format!("{name} (Green)")],
                "{how}"
            );
            assert_eq!(entity(&document, dark).style, name);
            assert_eq!(entity(&document, green).style, format!("{name} (Green)"));
            assert_eq!(
                style_of(&document, green).color,
                Some(Color::new(0, 255, 0, 255))
            );
            assert_eq!(
                style_of(&document, dark).color,
                Some(Color::new(0, 100, 0, 255))
            );
            assert_eq!(style_of(&document, green).linetype, name);
        }
    }

    #[test]
    fn renaming_a_coded_style_then_applying_codes_again_reuses_it_instead_of_making_a_second() {
        let mut document = coded(real_shapes());
        add_coded_point(&mut document, 0.0, 0.0, "WM01");
        apply(&mut document, &with_colours());
        assert!(document.model().styles.contains("WATR Main"));
        assert!(
            document
                .execute(rename_style("WATR Main", "Water main"))
                .is_ok()
        );
        let styles = document.model().styles.size();

        let later = add_coded_point(&mut document, 1.0, 0.0, "WM02");
        let report = apply(&mut document, &with_colours());

        assert!(report.styles_created.is_empty());
        assert_eq!(report.styles_reused, vec!["Water main"]);
        assert_eq!(
            document.model().styles.size(),
            styles,
            "no duplicate of the renamed style"
        );
        assert!(!document.model().styles.contains("WATR Main"));
        assert_eq!(entity(&document, later).style, "Water main");
    }

    #[test]
    fn an_imported_string_whose_code_is_only_in_its_metadata_is_coded_and_keeps_its_symbol() {
        // An archive import records a string's name - its code, in a coded
        // survey - as metadata "12d.name", and gives it a style named after its
        // linestyle: here a symbol style, beside the style "0" the import makes
        // for plain strings. Reading the code from the metadata WITHOUT the
        // appearance rule would move this point onto "0" and lose its symbol.
        let mut document = coded(real_shapes());
        add_style(
            &mut document,
            Style {
                name: String::from("0"),
                linetype: String::from("0"),
                description: String::from("library linestyle"),
                ..Style::default()
            },
        );
        add_style(
            &mut document,
            Style {
                name: String::from("CULT Bollard"),
                linetype: String::from("CULT Bollard"),
                symbol: String::from("CULT Bollard"),
                symbol_size: 1.5,
                description: String::from("library symbol"),
                ..Style::default()
            },
        );
        let id = imported_point(&mut document, "CULT Bollard", "AC01");

        let report = apply(&mut document, &with_colours());

        assert_eq!(report.property, "12d.name", "found in the metadata");
        assert_eq!(report.coded, 1);
        assert_eq!(report.matched, 1);
        let found = entity(&document, id);
        assert_eq!(found.layer, "SURVEY DETAIL");
        assert_ne!(found.style, "0");
        assert_eq!(
            style_of(&document, id).symbol,
            "CULT Bollard",
            "the symbol is kept"
        );
        // The imported style also runs the name as a linestyle and has no
        // colour, so it is not this appearance, and a new style is named for
        // the colour.
        assert_eq!(found.style, "CULT Bollard (white)");
    }

    #[test]
    fn a_style_is_reused_only_when_all_four_parts_of_the_appearance_agree() {
        // An existing plain style in the wrong colour is not reused, and its
        // name is not taken over either.
        let mut document = coded(real_shapes());
        add_style(
            &mut document,
            Style {
                name: String::from("Plain"),
                color: Some(Color::new(255, 0, 0, 255)),
                ..Style::default()
            },
        );
        add_style(
            &mut document,
            Style {
                name: String::from("Signs"),
                linetype: String::from("Continuous"), // plain, in any case
                color: Some(Color::new(255, 255, 0, 255)),
                ..Style::default()
            },
        );
        let yellow_text = add_coded_point(&mut document, 0.0, 0.0, "101");
        let cyan_text = add_coded_point(&mut document, 0.0, 0.0, "201");

        let report = apply(&mut document, &with_colours());
        assert_eq!(
            entity(&document, yellow_text).style,
            "Signs",
            "plain and yellow: reused under its own name"
        );
        assert_eq!(
            entity(&document, cyan_text).style,
            "Plain (cyan)",
            "\"Plain\" is red, so cyan cannot have that name"
        );
        assert_eq!(report.styles_reused, vec!["Signs"]);
        assert_eq!(report.styles_created, vec!["Plain (cyan)"]);
    }

    #[test]
    fn an_imported_style_with_no_colour_is_reused_for_a_code_whose_colour_name_has_no_rgb() {
        // The import-then-code path: an archive import gives a string a style
        // named after its linestyle, with no colour and a description that is
        // not a colour name. WM* says "sui water potable", which test_colour
        // (like Katana) has no RGB for, so the code draws as WATR Main with no
        // colour - exactly what the imported style draws. Matching the colour
        // NAME against the description made a second, identical "WATR Main (sui
        // water potable)" and moved the string onto it.
        let mut document = coded(real_shapes());
        add_style(
            &mut document,
            Style {
                name: String::from("WATR Main"),
                linetype: String::from("WATR Main"),
                description: String::from("library linestyle"),
                ..Style::default()
            },
        );
        let id = imported_point(&mut document, "WATR Main", "WM01");
        let styles = document.model().styles.size();

        let report = apply(&mut document, &with_colours());

        assert!(report.styles_created.is_empty());
        assert_eq!(report.styles_reused, vec!["WATR Main"]);
        assert_eq!(
            document.model().styles.size(),
            styles,
            "no duplicate of the imported style"
        );
        assert_eq!(entity(&document, id).style, "WATR Main");
        assert_eq!(
            entity(&document, id).layer,
            "SURVEY SERVICES",
            "still coded"
        );
        assert_eq!(report.codes.len(), 1);
        assert_eq!(report.codes[0].style_outcome, SurveyStyleOutcome::Reused);
    }

    #[test]
    fn renaming_a_coded_style_and_rewriting_its_description_then_applying_again_makes_no_duplicate()
    {
        // Coding writes the colour name into the description because it has no
        // RGB for it. Someone renames the style AND describes it in their own
        // words; it still draws as the code says, so it is still the code's
        // style.
        let mut document = coded(real_shapes());
        let first = add_coded_point(&mut document, 0.0, 0.0, "WM01");
        apply(&mut document, &with_colours());
        assert!(document.model().styles.contains("WATR Main"));
        assert!(document.execute(rename_style("WATR Main", "Water")).is_ok());
        let mut edited = document
            .model()
            .styles
            .find("Water")
            .cloned()
            .expect("the renamed style");
        edited.description = String::from("potable water main");
        assert!(document.execute(update_style(edited)).is_ok());
        let styles = document.model().styles.size();

        let later = add_coded_point(&mut document, 1.0, 0.0, "WM02");
        let report = apply(&mut document, &with_colours());

        assert!(report.styles_created.is_empty());
        assert_eq!(report.styles_reused, vec!["Water"]);
        assert_eq!(
            document.model().styles.size(),
            styles,
            "no second WATR Main"
        );
        assert!(!document.model().styles.contains("WATR Main"));
        assert_eq!(entity(&document, first).style, "Water", "not moved off it");
        assert_eq!(entity(&document, later).style, "Water");
    }

    #[test]
    fn a_style_made_for_one_unknown_colour_is_not_taken_by_another_whatever_the_order_of_coding() {
        // WM* and WR* draw alike in Katana - WATR Main with no RGB for either
        // colour - but they are different colour names, and a fresh drawing
        // gives each its own style. By hand: appearances are named in a fixed
        // order, and "sui water potable" sorts before "sui water recycled"
        // ('p' < 'r'), so potable takes the bare "WATR Main" and recycled,
        // finding it taken, gets " (sui water recycled)". Coding WM01 first and
        // WR01 in a later pass must end the same way, not put WR01 on potable's
        // style.
        let with_recycled = || {
            let mut map = real_shapes();
            let recycled = SurveyRule {
                key: String::from("WR*"),
                model: String::from("SURVEY SERVICES"),
                colour: String::from("sui water recycled"),
                linestyle: String::from("WATR Main"),
                ..SurveyRule::default()
            };
            assert!(map.add(recycled).is_ok());
            map
        };
        for in_two_passes in [false, true] {
            let mut document = coded(with_recycled());
            let potable = add_coded_point(&mut document, 0.0, 0.0, "WM01");
            if in_two_passes {
                apply(&mut document, &with_colours());
            }
            let recycled = add_coded_point(&mut document, 1.0, 0.0, "WR01");
            apply(&mut document, &with_colours());

            let how = if in_two_passes {
                "in two passes"
            } else {
                "in one"
            };
            assert_eq!(entity(&document, potable).style, "WATR Main", "{how}");
            assert_eq!(
                entity(&document, recycled).style,
                "WATR Main (sui water recycled)",
                "{how}"
            );
            assert_eq!(
                style_of(&document, recycled).description,
                "sui water recycled",
                "{how}"
            );
        }
    }

    #[test]
    fn of_two_styles_that_draw_a_code_alike_the_one_described_by_its_colour_name_is_reused() {
        // "A water" is first in name order and draws WM* just as well, but
        // "Water main" carries WM*'s colour name - it is the style an earlier
        // pass made for this code - so applying codes again keeps it there.
        let mut document = coded(real_shapes());
        for (name, description) in [
            ("A water", "library linestyle"),
            ("Water main", "sui water potable"),
        ] {
            add_style(
                &mut document,
                Style {
                    name: String::from(name),
                    linetype: String::from("WATR Main"),
                    description: String::from(description),
                    ..Style::default()
                },
            );
        }
        let id = add_coded_point(&mut document, 0.0, 0.0, "WM01");
        let report = apply(&mut document, &with_colours());
        assert_eq!(entity(&document, id).style, "Water main");
        assert_eq!(report.styles_reused, vec!["Water main"]);
    }
}

// ---- what the report says -------------------------------------------------

mod survey_coding_report {
    use super::*;

    #[test]
    fn without_creating_layers_the_style_and_attributes_are_still_applied() {
        // Audit CAD-18: a missing layer used to abandon the whole entity.
        let mut document = coded(water_and_bollards());
        let id = add_coded_point(&mut document, 0.0, 0.0, "WM01");

        let options = SurveyCodingOptions {
            create_layers: false,
            ..SurveyCodingOptions::default()
        };
        let report = apply(&mut document, &options);

        let found = entity(&document, id);
        assert_eq!(found.layer, DEFAULT_LAYER_NAME, "no layer, so it stays");
        assert_eq!(found.style, "WATR Main");
        assert_eq!(text_property(found, "DepthLocation"), "Top of Pipe");
        assert_eq!(report.skipped_no_layer, 1);
        assert!(report.layers_created.is_empty());
        assert_eq!(report.changed, 1, "its style and a property changed");
        assert_eq!(report.codes.len(), 1);
        assert_eq!(report.codes[0].layer_kept, 1, "the row says so too");
    }

    #[test]
    fn changed_counts_each_altered_entity_once_whatever_was_altered() {
        let mut document = coded(water_and_bollards());
        let services = Layer {
            name: String::from("SURVEY SERVICES"),
            ..Layer::default()
        };
        assert!(document.execute(create_layer(services)).is_ok());
        let on_layer = add_coded_point(&mut document, 0.0, 0.0, "WM01");
        assert!(
            document
                .execute(set_entity_layer(vec![on_layer], "SURVEY SERVICES"))
                .is_ok()
        );
        add_coded_point(&mut document, 1.0, 0.0, "WM02");

        let report = apply(&mut document, &SurveyCodingOptions::default());
        // Both change: the first only its style and a property (it is already
        // on the layer), the second its layer as well. Counting layer moves
        // alone gave 1.
        assert_eq!(report.changed, 2);

        // Everything is now as the map says, so a second pass has nothing to do.
        let mut again = SurveyCodingReport::default();
        let second =
            apply_survey_codes(&document, &SurveyCodingOptions::default(), Some(&mut again))
                .expect("planned");
        assert!(second.is_none());
        assert_eq!(again.changed, 0);
        assert_eq!(
            again.matched, 2,
            "still matched; only nothing is left to change"
        );
    }

    #[test]
    fn without_creating_styles_an_appearance_no_style_has_is_counted_and_the_style_kept() {
        let mut document = coded(water_and_bollards());
        let id = add_coded_point(&mut document, 0.0, 0.0, "WM01");
        let options = SurveyCodingOptions {
            create_styles: false,
            ..SurveyCodingOptions::default()
        };
        let report = apply(&mut document, &options);
        assert_eq!(entity(&document, id).style, "");
        assert_eq!(entity(&document, id).layer, "SURVEY SERVICES");
        assert_eq!(report.skipped_no_style, 1);
        assert!(report.styles_created.is_empty());
    }

    #[test]
    fn a_missing_definition_is_reported_even_when_the_code_reuses_a_style() {
        let mut document = coded(water_and_bollards());
        add_coded_point(&mut document, 0.0, 0.0, "WM01");
        apply(&mut document, &SurveyCodingOptions::default());
        add_coded_point(&mut document, 1.0, 0.0, "WM02");
        let report = apply(&mut document, &SurveyCodingOptions::default());
        assert_eq!(report.styles_reused, vec!["WATR Main"]);
        assert_eq!(
            report.missing_definitions,
            vec!["WATR Main"],
            "the library still does not define it"
        );
    }

    #[test]
    fn each_distinct_code_has_a_row_saying_what_happened_to_its_entities() {
        let mut document = coded(water_and_bollards());
        add_coded_point(&mut document, 0.0, 0.0, "WM01");
        add_coded_point(&mut document, 1.0, 0.0, "ZZ99");
        add_coded_point(&mut document, 2.0, 0.0, "WM01");
        add_coded_point(&mut document, 3.0, 0.0, "AC07");

        let report = apply(&mut document, &SurveyCodingOptions::default());
        assert_eq!(report.codes.len(), 3);
        let bollard = &report.codes[0];
        let water = &report.codes[1];
        let typo = &report.codes[2];
        assert_eq!(bollard.code, "AC07");
        assert_eq!(water.code, "WM01");
        assert_eq!(typo.code, "ZZ99");

        assert_eq!(water.entities, 2);
        assert_eq!(water.kind, SurveyMatchKind::Prefix);
        assert!(water.matched);
        assert_eq!(water.layers_from, vec![DEFAULT_LAYER_NAME]);
        assert_eq!(water.layer, "SURVEY SERVICES");
        assert_eq!(water.style, "WATR Main");
        assert_eq!(water.style_outcome, SurveyStyleOutcome::Created);
        assert_eq!(water.attributes_set, vec!["DepthLocation"]);
        assert_eq!(water.attributes_deferred, vec!["Diameter"]);
        assert_eq!(water.changed, 2);

        assert_eq!(typo.kind, SurveyMatchKind::FallbackOnly);
        assert!(!typo.matched);
        assert_eq!(typo.layer, "");
        assert_eq!(typo.style_outcome, SurveyStyleOutcome::None);
        assert_eq!(
            typo.attributes_set,
            vec!["DepthLocation"],
            "the `*` rule does say every code gets it"
        );
        assert_eq!(bollard.style, "CULT Bollard");
    }

    #[test]
    fn every_entity_gets_what_a_lookup_of_its_own_code_gives_when_codes_repeat() {
        // Decisions are made once per distinct code; this checks each entity
        // against a lookup of its own code alone, which is the per-entity path.
        let map = real_shapes();
        let mut document = coded(map.clone());
        let codes = [
            "WM01", "AC01", "TS01", "WM01", "101", "AC01", "TD02", "WM03", "101", "SV09", "TS01",
            "201",
        ];
        let mut ids = Vec::new();
        for (i, code) in codes.iter().enumerate() {
            ids.push(add_coded_point(&mut document, i as f64, 0.0, code));
        }
        let report = apply(&mut document, &with_colours());

        for (code, &id) in codes.iter().zip(&ids) {
            let found = map.lookup(code);
            let style = style_of(&document, id);
            assert_eq!(entity(&document, id).layer, found.resolved.model, "{code}");
            let plain = is_plain_linestyle(&found.resolved.linestyle);
            let expected_linetype = if plain {
                String::from(CONTINUOUS_LINETYPE)
            } else {
                found.resolved.linestyle.clone()
            };
            assert_eq!(style.linetype, expected_linetype, "{code}");
            let expected_symbol = found
                .resolved
                .symbol
                .as_ref()
                .map(|symbol| symbol.style.clone())
                .unwrap_or_default();
            assert_eq!(style.symbol, expected_symbol, "{code}");
            assert_eq!(style.color, test_colour(&found.resolved.colour), "{code}");
        }
        let rows_total: usize = report.codes.iter().map(|row| row.entities).sum();
        assert_eq!(rows_total, codes.len());
        // WM01 AC01 TS01 101 TD02 WM03 SV09 201 are distinct; WM01, AC01, 101
        // and TS01 repeat: twelve entities, eight distinct codes.
        assert_eq!(report.codes.len(), 8);
    }
}

mod customisation_coverage {
    use super::*;

    #[test]
    fn a_built_in_symbol_is_never_listed_as_unresolved() {
        // Audit CAD-17: "cross" draws through Katana's own shapes.
        let mut document = Document::default();
        add_style(
            &mut document,
            Style {
                name: String::from("s"),
                symbol: String::from("cross"),
                ..Style::default()
            },
        );
        let coverage = customisation_coverage(&document);
        assert_eq!(coverage.styles, 1);
        assert!(coverage.unresolved.is_empty());
        assert_eq!(coverage.named, 0);
        assert_eq!(coverage.built_in, 1);

        // A name nothing draws is still reported.
        add_style(
            &mut document,
            Style {
                name: String::from("t"),
                symbol: String::from("CULT Bollard"),
                ..Style::default()
            },
        );
        assert_eq!(
            customisation_coverage(&document).unresolved,
            vec!["CULT Bollard"]
        );
    }

    #[test]
    fn a_style_that_takes_its_linetype_from_its_layer_names_no_definition() {
        // "ByLayer" as a style's linetype means "whatever the layer draws with"
        // (decision D2), not a library linestyle called ByLayer: reporting it as
        // in no loaded library sent people looking for a definition that is not
        // meant to exist.
        let mut document = Document::default();
        add_style(
            &mut document,
            Style {
                name: String::from("s"),
                linetype: String::from("ByLayer"),
                ..Style::default()
            },
        );
        let coverage = customisation_coverage(&document);
        assert_eq!(coverage.styles, 1);
        assert!(coverage.unresolved.is_empty());
        assert_eq!(coverage.named, 0, "it names nothing to resolve");
    }

    #[test]
    fn its_missing_names_are_the_styles_names_missing_names_reports_each_under_its_reason() {
        // One rule, not two (audit CAD-17): customisation_coverage and
        // missing_names read linetype_status and symbol_status. Its two lists
        // are that rule's two reasons, kept apart because CUSTOMISE and the
        // window print `unresolved` as "in no loaded library".
        //
        //   library  CULT Bollard  `mode vertex`: a symbol, not a linestyle
        //            SEWR Pit      not `mode vertex`: a linestyle, and a symbol
        //   styles   a  linetype CULT Bollard            -> drawn solid: not a
        //               linestyle, though the library defines it
        //            b  linetype = symbol = CULT Bollard -> the importer's own
        //               pattern: a plain line under a defined symbol (D8)
        //            c  linetype = symbol = SEWR Pit     -> the same
        //            d  symbol cross                     -> a built-in shape
        //            e  linetype ByLayer                 -> names nothing
        //            f  linetype WATR Main               -> defined nowhere
        //
        // By hand: 6 styles; named a, b, c, f (4); resolved b, c (2); built in
        // d (1); not_linestyles {"CULT Bollard"}, which a alone names as a
        // linetype; unresolved {"WATR Main"}, from f.
        let mut library = StyleLibrary::default();
        for (name, at_vertices) in [("CULT Bollard", true), ("SEWR Pit", false)] {
            let definition = LineStyle {
                name: String::from(name),
                at_vertices,
                strokes: vec![Stroke {
                    op: StrokeOp::Draw,
                    point: Point2::new(1.0, 0.0),
                    ..Stroke::default()
                }],
                ..LineStyle::default()
            };
            assert!(library.add(definition).is_ok());
        }
        let mut document = Document::default();
        document.set_style_library(library);
        for (name, linetype, symbol) in [
            ("a", "CULT Bollard", ""),
            ("b", "CULT Bollard", "CULT Bollard"),
            ("c", "SEWR Pit", "SEWR Pit"),
            ("d", "continuous", "cross"),
            ("e", "ByLayer", ""),
            ("f", "WATR Main", ""),
        ] {
            add_style(
                &mut document,
                Style {
                    name: String::from(name),
                    linetype: String::from(linetype),
                    symbol: String::from(symbol),
                    ..Style::default()
                },
            );
        }

        let coverage = customisation_coverage(&document);
        assert_eq!(coverage.styles, 6);
        assert_eq!(coverage.named, 4);
        assert_eq!(coverage.resolved, 2);
        assert_eq!(coverage.built_in, 1);
        assert_eq!(coverage.unresolved, vec!["WATR Main"]);
        assert_eq!(coverage.not_linestyles, vec!["CULT Bollard"]);

        // And missing_names says the same of the styles, and why.
        let mut undefined = Vec::new();
        let mut not_linestyles = Vec::new();
        for missing in missing_names(&document) {
            if missing.users.styles.is_empty() {
                continue;
            }
            if missing.name == "CULT Bollard" {
                assert_eq!(
                    missing.users.styles,
                    vec![String::from("a")],
                    "b draws its own symbol's name plain"
                );
            }
            if missing.status == NameStatus::NotALinestyle {
                not_linestyles.push(missing.name);
            } else {
                undefined.push(missing.name);
            }
        }
        assert_eq!(undefined, coverage.unresolved);
        assert_eq!(not_linestyles, coverage.not_linestyles);
    }
}
