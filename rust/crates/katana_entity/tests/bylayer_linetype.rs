//! The twin of tests/entity/customisation/test_bylayer_linetype.cpp: a Style
//! whose linetype is "ByLayer" inherits the layer's.
//!
//! Before, a Style always took Style::linetype, so a symbol-only or
//! colour-only style silently overrode its layer's linetype with
//! "continuous". "ByLayer" is already a reserved Linetype NAME (the DXF
//! keyword), so it can mean "inherit" without a storage change.
//!
//! The two TableValidation cases hand a record CP1252 bytes, which a String
//! cannot hold: the first keeps its UTF-8 half, and
//! TableValidation.AStyleWhoseLinetypeNameIsNotUtf8IsRefused has no twin.

pub mod support;

use katana_entity::display::{is_by_layer, resolve_display};
use katana_entity::{
    Entity, Geometry, Layer, Linetype, LinetypeElement, Model, PointGeometry, Style, validate,
};
use katana_geometry::Point2;
use support::double_eq;

fn layer_with_a_fence_linetype() -> Model {
    let mut model = Model::new();
    let fence = Linetype {
        name: String::from("fence"),
        pattern: vec![
            LinetypeElement { length: 1.0 },
            LinetypeElement { length: -0.5 },
        ],
        ..Linetype::default()
    };
    assert!(model.linetypes.add(fence).is_ok());
    let layer = Layer {
        name: String::from("survey"),
        linetype: String::from("fence"),
        ..Layer::default()
    };
    assert!(model.layers.add(layer).is_ok());
    model
}

fn point_on(layer: &str, style: &str) -> Entity {
    Entity {
        layer: String::from(layer),
        style: String::from(style),
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(0.0, 0.0),
        }),
        ..Entity::default()
    }
}

mod by_layer_linetype {
    use super::*;

    #[test]
    fn a_style_whose_linetype_is_by_layer_draws_with_its_layers_linetype() {
        let mut model = layer_with_a_fence_linetype();
        let tree = Style {
            name: String::from("Tree"),
            linetype: String::from("ByLayer"),
            symbol: String::from("tree"),
            line_weight: 0.7,
            ..Style::default()
        };
        assert!(model.styles.add(tree).is_ok());

        let display = resolve_display(&model, &point_on("survey", "Tree"));
        assert_eq!(
            display.linetype, "fence",
            "the layer's, not the word ByLayer"
        );
        // Everything else the style says still holds.
        assert_eq!(display.symbol, "tree");
        double_eq(display.line_weight, 0.7);
    }

    #[test]
    fn the_same_style_on_another_layer_follows_that_layer() {
        let mut model = layer_with_a_fence_linetype();
        let other = Layer {
            name: String::from("roads"), // continuous by default
            ..Layer::default()
        };
        assert!(model.layers.add(other).is_ok());
        let tree = Style {
            name: String::from("Tree"),
            linetype: String::from("ByLayer"),
            ..Style::default()
        };
        assert!(model.styles.add(tree).is_ok());

        assert_eq!(
            resolve_display(&model, &point_on("survey", "Tree")).linetype,
            "fence"
        );
        assert_eq!(
            resolve_display(&model, &point_on("roads", "Tree")).linetype,
            "continuous"
        );
        // A missing layer resolves to the defaults, as it always has.
        assert_eq!(
            resolve_display(&model, &point_on("gone", "Tree")).linetype,
            "continuous"
        );
    }

    #[test]
    fn a_style_naming_its_own_linetype_still_overrides_the_layer() {
        // The regression the change must not cause.
        let mut model = layer_with_a_fence_linetype();
        let kerb = Style {
            name: String::from("Kerb"),
            linetype: String::from("continuous"),
            ..Style::default()
        };
        assert!(model.styles.add(kerb).is_ok());
        assert_eq!(
            resolve_display(&model, &point_on("survey", "Kerb")).linetype,
            "continuous"
        );
    }

    #[test]
    fn the_word_is_read_in_any_case_because_no_linetype_can_be_called_that_in_any_case() {
        assert!(is_by_layer("ByLayer"));
        assert!(is_by_layer("BYLAYER"));
        assert!(is_by_layer("bylayer"));
        assert!(!is_by_layer("ByLayerish"));
        assert!(!is_by_layer(""));
        // The reservation it relies on.
        let named = Linetype {
            name: String::from("BYLAYER"),
            ..Linetype::default()
        };
        assert!(validate(&named).is_err());
    }

    #[test]
    fn validate_accepts_a_style_that_says_by_layer() {
        let style = Style {
            name: String::from("symbol only"),
            linetype: String::from("ByLayer"),
            ..Style::default()
        };
        assert!(validate(&style).is_ok());
    }
}

mod table_validation {
    use super::*;

    // The C++ first gives the description CP1252 bytes, which a String cannot
    // hold; the same word in UTF-8 is accepted.
    #[test]
    fn a_linetype_description_that_is_not_utf8_is_refused() {
        let linetype = Linetype {
            name: String::from("fence"),
            description: String::from("cl\u{e9}ture"),
            ..Linetype::default()
        };
        assert!(validate(&linetype).is_ok());
    }
}
