//! The twin of tests/entity/test_display.cpp: resolved appearance.
//!
//! Before resolve_display there were three answers to "what colour is this
//! entity". The tests here pin the one chain, including the two cases that
//! were actually wrong on screen.

pub mod support;

use katana_entity::display::resolve_display;
use katana_entity::tables::{CONTINUOUS_LINETYPE, NO_SYMBOL};
use katana_entity::{Color, Entity, Geometry, Layer, Model, Style};
use katana_geometry::{Point2, Segment2};
use support::double_eq;

const RED: Color = Color::new(255, 0, 0, 255);
const GREEN: Color = Color::new(0, 255, 0, 255);
const BLUE: Color = Color::new(0, 0, 255, 255);

fn model_with_layer_and_style() -> Model {
    let mut model = Model::new();
    assert!(
        model
            .layers
            .add(Layer {
                name: String::from("kerb"),
                color: RED,
                line_weight: 0.5,
                linetype: String::from("dashed"),
                ..Layer::default()
            })
            .is_ok()
    );
    assert!(
        model
            .styles
            .add(Style {
                name: String::from("heavy"),
                color: Some(GREEN),
                line_weight: 1.2,
                linetype: String::from("centre"),
                ..Style::default()
            })
            .is_ok()
    );
    assert!(
        model
            .styles
            .add(Style {
                name: String::from("thick-only"),
                color: None, // ByLayer
                line_weight: 2.0,
                linetype: String::from("phantom"),
                ..Style::default()
            })
            .is_ok()
    );
    model
}

fn on_layer(layer: &str) -> Entity {
    Entity {
        layer: String::from(layer),
        geometry: Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0))),
        ..Entity::default()
    }
}

mod resolve_display {
    use super::*;

    #[test]
    fn an_entity_with_no_overrides_takes_everything_from_its_layer() {
        // THE case that was wrong in the 3D view: ByLayer is the default, so
        // this is the common path, not a corner one.
        let model = model_with_layer_and_style();
        let display = resolve_display(&model, &on_layer("kerb"));
        assert_eq!(display.color, RED);
        double_eq(display.line_weight, 0.5);
        assert_eq!(display.linetype, "dashed");
    }

    #[test]
    fn a_named_style_overrides_the_layer() {
        // THE case that was wrong in both views: the style was stored,
        // validated and persisted, and read by nothing.
        let model = model_with_layer_and_style();
        let mut entity = on_layer("kerb");
        entity.style = String::from("heavy");

        let display = resolve_display(&model, &entity);
        assert_eq!(display.color, GREEN);
        double_eq(display.line_weight, 1.2);
        assert_eq!(display.linetype, "centre");
    }

    #[test]
    fn a_style_without_a_colour_keeps_the_layers_colour_but_its_own_width() {
        // The asymmetry is in the Style type: `color: Option<Color>` is
        // "empty: ByLayer", while line_weight and linetype are plain values
        // that a named style always supplies.
        let model = model_with_layer_and_style();
        let mut entity = on_layer("kerb");
        entity.style = String::from("thick-only");

        let display = resolve_display(&model, &entity);
        assert_eq!(
            display.color, RED,
            "a colourless style must fall through to the layer"
        );
        double_eq(display.line_weight, 2.0);
        assert_eq!(display.linetype, "phantom");
    }

    #[test]
    fn the_entitys_own_colour_overrides_everything() {
        let model = model_with_layer_and_style();
        let mut entity = on_layer("kerb");
        entity.style = String::from("heavy");
        entity.color = Some(BLUE);

        let display = resolve_display(&model, &entity);
        assert_eq!(display.color, BLUE);
        // ...but only the colour. There is deliberately no per-entity line
        // weight or linetype, so those still come from the style.
        double_eq(display.line_weight, 1.2);
        assert_eq!(display.linetype, "centre");
    }

    #[test]
    fn a_missing_layer_or_style_falls_back_instead_of_dereferencing_null() {
        // An entity can reference a layer that has been removed; the model
        // permits it and a renderer must not crash on it.
        let model = model_with_layer_and_style();

        let orphan = resolve_display(&model, &on_layer("deleted"));
        assert_eq!(
            orphan.color,
            Color::default(),
            "the built-in default, not a dereference"
        );
        double_eq(orphan.line_weight, 0.25);
        assert_eq!(orphan.linetype, CONTINUOUS_LINETYPE);

        let mut with_missing_style = on_layer("kerb");
        with_missing_style.style = String::from("no-such-style");
        let display = resolve_display(&model, &with_missing_style);
        assert_eq!(
            display.color, RED,
            "a missing style must not lose the layer's values"
        );
        double_eq(display.line_weight, 0.5);

        // An entity's own colour still wins even with nothing else resolvable.
        let mut orphan_with_colour = on_layer("deleted");
        orphan_with_colour.color = Some(BLUE);
        assert_eq!(resolve_display(&model, &orphan_with_colour).color, BLUE);
    }

    #[test]
    fn the_default_layer_resolves_without_anything_being_configured() {
        // A fresh model with an entity on layer "0" is the very first thing the
        // application ever draws.
        let model = Model::new();
        let display = resolve_display(&model, &on_layer("0"));
        assert_eq!(display.color, Color::default());
        double_eq(display.line_weight, 0.25);
        assert_eq!(display.linetype, CONTINUOUS_LINETYPE);
    }

    #[test]
    fn resolution_is_pure_and_repeatable() {
        // Called once per entity per frame, so it must not depend on anything
        // but its arguments.
        let model = model_with_layer_and_style();
        let mut entity = on_layer("kerb");
        entity.style = String::from("heavy");

        let first = resolve_display(&model, &entity);
        for _ in 0..5 {
            assert_eq!(resolve_display(&model, &entity), first);
        }
    }

    #[test]
    fn the_point_symbol_comes_from_the_style_and_nowhere_else() {
        // A layer has no symbol - a layer holds lines and points alike - so a
        // point on a layer with no style draws the plain mark, and a point in a
        // style with a symbol draws that at the style's size.
        let mut model = model_with_layer_and_style();
        assert!(
            model
                .styles
                .add(Style {
                    name: String::from("manhole"),
                    symbol: String::from("manhole"),
                    symbol_size: 1.2,
                    ..Style::default()
                })
                .is_ok()
        );

        let plain = on_layer("kerb");
        assert_eq!(resolve_display(&model, &plain).symbol, NO_SYMBOL);
        double_eq(resolve_display(&model, &plain).symbol_size, 0.0);

        let mut marked = on_layer("kerb");
        marked.style = String::from("manhole");
        let display = resolve_display(&model, &marked);
        assert_eq!(display.symbol, "manhole");
        double_eq(display.symbol_size, 1.2);
        assert_eq!(
            display.color, RED,
            "a style that says nothing about colour leaves the layer's"
        );

        marked.style = String::from("heavy");
        assert_eq!(
            resolve_display(&model, &marked).symbol,
            NO_SYMBOL,
            "a line style draws a point with the plain mark"
        );
    }
}
