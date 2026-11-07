//! The twin of tests/entity/customisation/test_table_usage.cpp: who uses every
//! style, linetype name, symbol name, hatch pattern and layer, in one pass -
//! the answer the delete guards, purge and the managers all read, so it has to
//! follow resolve_display's chain exactly, ByLayer included.

use katana_entity::display::resolve_display;
use katana_entity::table_usage::table_usage;
use katana_entity::{
    Entity, EntityId, Geometry, HatchLineFamily, HatchPattern, Layer, Linetype, LinetypeElement,
    Model, PointGeometry, Style, TableUsage, UsageOptions, Users,
};
use katana_geometry::Point2;

fn add_linetype(model: &mut Model, name: &str) {
    let linetype = Linetype {
        name: String::from(name),
        pattern: vec![
            LinetypeElement { length: 1.0 },
            LinetypeElement { length: -0.5 },
        ],
        ..Linetype::default()
    };
    assert!(model.linetypes.add(linetype).is_ok());
}

fn add_layer(model: &mut Model, name: &str, linetype: &str) {
    let layer = Layer {
        name: String::from(name),
        linetype: String::from(linetype),
        ..Layer::default()
    };
    assert!(model.layers.add(layer).is_ok());
}

fn add_style(model: &mut Model, name: &str, linetype: &str, symbol: &str, hatch: &str) {
    let style = Style {
        name: String::from(name),
        linetype: String::from(linetype),
        symbol: String::from(symbol),
        hatch_pattern: String::from(hatch),
        ..Style::default()
    };
    assert!(model.styles.add(style).is_ok());
}

fn add_entity(model: &mut Model, layer: &str, style: &str) -> EntityId {
    let entity = Entity {
        layer: String::from(layer),
        style: String::from(style),
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(0.0, 0.0),
        }),
        ..Entity::default()
    };
    model.entities.add(entity).unwrap()
}

/// The hand-built drawing every test below reads. Entity ids are assigned
/// 1, 2, 3 ... in the order added, so the comment on each line is its id.
///
///   linetypes  continuous (seeded), fence, dash, spare
///   hatches    none (seeded), grass
///   layers     0 (continuous), survey (fence), roads ("WATR Main", a library
///              name the model does not have), kerbs (dash)
///   styles     Kerb    linetype dash, hatch grass
///              Tree    linetype ByLayer, symbol tree
///              Bollard linetype and symbol "CULT Bollard"
///              Unused  linetype dash
fn drawing() -> Model {
    let mut model = Model::new();
    add_linetype(&mut model, "fence");
    add_linetype(&mut model, "dash");
    add_linetype(&mut model, "spare");
    let grass = HatchPattern {
        name: String::from("grass"),
        families: vec![HatchLineFamily {
            angle: 0.0,
            spacing: 1.0,
            offset: 0.0,
        }],
        ..HatchPattern::default()
    };
    assert!(model.hatch_patterns.add(grass).is_ok());
    add_layer(&mut model, "survey", "fence");
    add_layer(&mut model, "roads", "WATR Main");
    add_layer(&mut model, "kerbs", "dash");
    add_style(&mut model, "Kerb", "dash", "", "grass");
    add_style(&mut model, "Tree", "ByLayer", "tree", "");
    add_style(&mut model, "Bollard", "CULT Bollard", "CULT Bollard", "");
    add_style(&mut model, "Unused", "dash", "", "");

    add_entity(&mut model, "survey", ""); // 1: no style        -> fence (its layer's)
    add_entity(&mut model, "survey", "Tree"); // 2: ByLayer         -> fence
    add_entity(&mut model, "roads", "Tree"); // 3: ByLayer         -> WATR Main
    add_entity(&mut model, "survey", "Kerb"); // 4: its style's     -> dash, hatch grass
    add_entity(&mut model, "0", "Bollard"); // 5: its style's     -> CULT Bollard
    add_entity(&mut model, "gone", ""); // 6: missing layer   -> continuous
    add_entity(&mut model, "survey", "Ghost"); // 7: missing style   -> fence (its layer's)
    model
}

mod table_usage {
    use super::*;

    #[test]
    fn every_style_is_counted_by_the_entities_wearing_it_and_an_unused_one_is_present_with_none() {
        let model = drawing();
        let usage = table_usage(&model, UsageOptions::default());

        // Kerb: entity 4. Tree: 2 and 3. Bollard: 5. Unused: nobody.
        assert_eq!(TableUsage::of(&usage.styles, "Kerb").entities, 1);
        assert_eq!(TableUsage::of(&usage.styles, "Kerb").first_entity, 4);
        assert_eq!(TableUsage::of(&usage.styles, "Tree").entities, 2);
        assert_eq!(TableUsage::of(&usage.styles, "Tree").first_entity, 2);
        assert_eq!(TableUsage::of(&usage.styles, "Bollard").entities, 1);
        assert!(
            usage.styles.contains_key("Unused"),
            "a defined style is listed even when unused"
        );
        assert!(!TableUsage::of(&usage.styles, "Unused").used());
        // Entity 7 wears a style the table does not have: listed under its
        // name, because a manager has to be able to show it.
        assert_eq!(TableUsage::of(&usage.styles, "Ghost").entities, 1);
        assert!(!model.styles.contains("Ghost"));
    }

    #[test]
    fn an_entity_is_counted_against_the_linetype_it_is_drawn_with_by_layer_included() {
        let model = drawing();
        let usage = table_usage(&model, UsageOptions::default());

        // fence: named by layer survey; reached by 1 (no style), 2 (Tree is
        // ByLayer, on survey) and 7 (a missing style falls back to the layer).
        let fence: &Users = TableUsage::of(&usage.linetypes, "fence");
        assert_eq!(fence.layers, vec!["survey"]);
        assert!(fence.styles.is_empty());
        assert_eq!(fence.entities, 3);
        assert_eq!(fence.first_entity, 1);

        // WATR Main: not a model linetype - a library name - but named by
        // layer roads and reached by 3, which inherits it through ByLayer.
        let watr = TableUsage::of(&usage.linetypes, "WATR Main");
        assert_eq!(watr.layers, vec!["roads"]);
        assert_eq!(watr.entities, 1);
        assert_eq!(watr.first_entity, 3);

        // dash: named by layer kerbs and styles Kerb and Unused (ascending);
        // reached by 4 only.
        let dash = TableUsage::of(&usage.linetypes, "dash");
        assert_eq!(dash.layers, vec!["kerbs"]);
        assert_eq!(dash.styles, vec!["Kerb", "Unused"]);
        assert_eq!(dash.entities, 1);

        // continuous: named by layer 0; reached by 6 alone - 5 is on layer 0
        // but wears Bollard, whose own linetype wins.
        let continuous = TableUsage::of(&usage.linetypes, "continuous");
        assert_eq!(continuous.layers, vec!["0"]);
        assert_eq!(continuous.entities, 1);
        assert_eq!(continuous.first_entity, 6);

        assert!(!TableUsage::of(&usage.linetypes, "spare").used());
        assert!(
            !usage.linetypes.contains_key("ByLayer"),
            "ByLayer is not a linetype: its entities are counted under the layer's"
        );

        // Every entity reaches exactly one linetype: 3 + 1 + 1 + 1 + 1 = 7.
        let total: usize = usage.linetypes.values().map(|users| users.entities).sum();
        assert_eq!(total, model.entities.size());
    }

    #[test]
    fn the_counts_agree_with_resolve_display_for_every_entity() {
        // The property the one-pass count exists to keep: an entity is counted
        // against the linetype and hatch resolve_display draws it with.
        let model = drawing();
        let usage = table_usage(&model, UsageOptions { entity_ids: true });
        model.entities.for_each(|entity| {
            let display = resolve_display(&model, entity);
            let linetype_ids = &TableUsage::of(&usage.linetypes, &display.linetype).entity_ids;
            assert!(
                linetype_ids.contains(&entity.id),
                "entity {} draws with {}",
                entity.id,
                display.linetype
            );
            let hatch_ids =
                &TableUsage::of(&usage.hatch_patterns, &display.hatch_pattern).entity_ids;
            assert!(
                hatch_ids.contains(&entity.id),
                "entity {} hatches with {}",
                entity.id,
                display.hatch_pattern
            );
        });
    }

    #[test]
    fn symbols_hatches_and_layers_are_counted_in_the_same_pass() {
        let model = drawing();
        let usage = table_usage(&model, UsageOptions::default());

        // tree: named by Tree, worn by 2 and 3.
        let tree = TableUsage::of(&usage.symbols, "tree");
        assert_eq!(tree.styles, vec!["Tree"]);
        assert_eq!(tree.entities, 2);
        assert_eq!(TableUsage::of(&usage.symbols, "CULT Bollard").entities, 1);
        assert!(
            !usage.symbols.contains_key(""),
            "no symbol is not a symbol name"
        );

        // grass: Kerb's, so entity 4. none: every layer's default, and the
        // other six entities.
        assert_eq!(
            TableUsage::of(&usage.hatch_patterns, "grass").styles,
            vec!["Kerb"]
        );
        assert_eq!(TableUsage::of(&usage.hatch_patterns, "grass").entities, 1);
        let none = TableUsage::of(&usage.hatch_patterns, "none");
        assert_eq!(none.layers, vec!["0", "kerbs", "roads", "survey"]);
        assert_eq!(none.entities, 6);

        // The layer manager's count: entities ON each layer, not its subtree.
        // survey holds 1, 2, 4 and 7; kerbs none; "gone" is not a layer but 6
        // is on it.
        assert_eq!(TableUsage::of(&usage.layers, "survey").entities, 4);
        assert_eq!(TableUsage::of(&usage.layers, "roads").entities, 1);
        assert_eq!(TableUsage::of(&usage.layers, "0").entities, 1);
        assert!(usage.layers.contains_key("kerbs"));
        assert_eq!(TableUsage::of(&usage.layers, "kerbs").entities, 0);
        assert_eq!(TableUsage::of(&usage.layers, "gone").entities, 1);
    }

    #[test]
    fn ids_are_collected_only_when_asked_for_and_then_ascending() {
        let model = drawing();
        assert!(
            TableUsage::of(
                &table_usage(&model, UsageOptions::default()).linetypes,
                "fence"
            )
            .entity_ids
            .is_empty(),
            "a count is all a column needs"
        );
        let usage = table_usage(&model, UsageOptions { entity_ids: true });
        assert_eq!(
            TableUsage::of(&usage.linetypes, "fence").entity_ids,
            vec![1, 2, 7]
        );
        assert_eq!(TableUsage::of(&usage.styles, "Tree").entity_ids, vec![2, 3]);
    }

    #[test]
    fn a_refusal_reads_as_a_count_and_the_first_holder() {
        let model = drawing();
        let usage = table_usage(&model, UsageOptions::default());
        // A layer is named first because it is what a person fixes first.
        assert_eq!(
            TableUsage::of(&usage.linetypes, "fence").describe(),
            "used by 1 layer and 3 entities, e.g. layer=survey"
        );
        assert_eq!(
            TableUsage::of(&usage.linetypes, "dash").describe(),
            "used by 1 layer, 2 styles and 1 entity, e.g. layer=kerbs"
        );
        assert_eq!(
            TableUsage::of(&usage.styles, "Tree").describe(),
            "used by 2 entities, e.g. id=2"
        );
        assert_eq!(TableUsage::of(&usage.styles, "Unused").describe(), "");
        assert_eq!(
            TableUsage::of(&usage.styles, "never heard of").describe(),
            "",
            "an unknown name is used by nobody, not an error"
        );
    }
}
