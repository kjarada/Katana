// What a hatch pattern is drawn as, and which pattern an entity resolves to
// (tests/cad/test_hatching.cpp).

use katana_cad::hatching::{hatch_drawing, hatch_segments, resolve_hatch_pattern};
use katana_cad::{HatchDrawing, HatchOptions};
use katana_entity::{Entity, Geometry, HatchLineFamily, HatchPattern, Layer, Model, Style};
use katana_geometry::{Point2, Polyline2};
use katana_math::numerics::PI;

fn line_family(name: &str, spacing: f64) -> HatchPattern {
    HatchPattern {
        name: String::from(name),
        families: vec![HatchLineFamily {
            angle: 0.0,
            spacing,
            offset: 0.0,
        }],
        ..HatchPattern::default()
    }
}

fn square(size: f64) -> Polyline2 {
    Polyline2::new(
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(size, 0.0),
            Point2::new(size, size),
            Point2::new(0.0, size),
        ],
        true,
    )
}

fn at_view_scale(view_scale: f64) -> HatchOptions {
    HatchOptions {
        view_scale,
        ..HatchOptions::default()
    }
}

fn hatched_layer(name: &str, pattern: &str) -> Layer {
    Layer {
        name: String::from(name),
        hatch_pattern: String::from(pattern),
        ..Layer::default()
    }
}

fn square_on(layer: &str) -> Entity {
    Entity {
        geometry: Geometry::Polyline(square(10.0)),
        layer: String::from(layer),
        ..Entity::default()
    }
}

mod hatching {
    use super::*;

    #[test]
    fn a_family_too_fine_to_resolve_is_drawn_solid_rather_than_as_thousands_of_lines() {
        // 0.2 m spacing at 5 pixels per metre is one pixel between lines: the
        // gaps cannot be seen, so the honest drawing is a solid tone and it
        // costs one polygon instead of thousands of clipped segments.
        let fine = line_family("fine", 0.2);
        let zoomed_out = at_view_scale(5.0); // 0.2 * 5 = 1 pixel, under the 3 px threshold
        assert_eq!(hatch_drawing(&fine, &zoomed_out), HatchDrawing::Solid);

        let zoomed_in = at_view_scale(40.0); // 0.2 * 40 = 8 pixels
        assert_eq!(hatch_drawing(&fine, &zoomed_in), HatchDrawing::Lines);
    }

    #[test]
    fn a_crosshatch_does_not_become_half_solid_and_half_lines() {
        // Two families at different spacings. If the decision were taken per
        // family there would be a zoom at which the coarse one still drew as
        // lines while the fine one had become solid, and the pattern would
        // look like neither.
        let crosshatch = HatchPattern {
            name: String::from("mixed"),
            families: vec![
                HatchLineFamily {
                    angle: 0.0,
                    spacing: 2.0,
                    offset: 0.0,
                },
                HatchLineFamily {
                    angle: PI / 2.0,
                    spacing: 0.1,
                    offset: 0.0,
                },
            ],
            ..HatchPattern::default()
        };

        let options = at_view_scale(5.0); // coarse: 10 px (fine enough for lines); fine: 0.5 px
        assert_eq!(hatch_drawing(&crosshatch, &options), HatchDrawing::Solid);
    }

    #[test]
    fn the_built_in_none_pattern_draws_nothing_and_a_solid_one_is_always_solid() {
        let none = HatchPattern {
            name: String::from("none"),
            ..HatchPattern::default()
        };
        let mut options = at_view_scale(100.0);
        assert_eq!(hatch_drawing(&none, &options), HatchDrawing::None);

        let solid = HatchPattern {
            name: String::from("filled"),
            solid: true,
            ..HatchPattern::default()
        };
        assert_eq!(hatch_drawing(&solid, &options), HatchDrawing::Solid);
        // A solid fill is a solid fill at every zoom - there is no spacing
        // for the threshold to act on.
        options.view_scale = 0.001;
        assert_eq!(hatch_drawing(&solid, &options), HatchDrawing::Solid);
    }

    #[test]
    fn a_view_that_has_not_been_scaled_yet_draws_solid_rather_than_asking_for_nonsense() {
        // A zero or negative scale means the widget has not been laid out.
        // Working out a pixel spacing from it would be meaningless, so the
        // cheap branch is taken rather than generating a family for a view
        // nobody is looking at.
        let pattern = line_family("hatch", 1.0);
        let unset = at_view_scale(0.0);
        assert_eq!(hatch_drawing(&pattern, &unset), HatchDrawing::Solid);
    }

    #[test]
    fn every_family_of_a_pattern_contributes_its_own_lines() {
        // A crosshatch is two families, and the segments of both must come
        // back - a renderer that saw only the first would draw half a pattern
        // and nothing would report it.
        let vertical_family = HatchLineFamily {
            angle: PI / 2.0,
            spacing: 2.0,
            offset: 0.0,
        };
        let crosshatch = HatchPattern {
            name: String::from("cross"),
            families: vec![
                HatchLineFamily {
                    angle: 0.0,
                    spacing: 2.0,
                    offset: 0.0,
                },
                vertical_family,
            ],
            ..HatchPattern::default()
        };

        let vertical_only = HatchPattern {
            name: String::from("vertical"),
            families: vec![crosshatch.families[1]],
            ..HatchPattern::default()
        };

        let horizontal = hatch_segments(&square(10.0), &line_family("horizontal", 2.0));
        let vertical = hatch_segments(&square(10.0), &vertical_only);
        let both = hatch_segments(&square(10.0), &crosshatch);

        // Not "twice the one-family count": the two families do not produce
        // the same number of lines even on a square. hatch_lines counts an
        // edge lying exactly on a line under a half-open rule, so one
        // bounding line of the square is excluded per family - and which one
        // depends on the direction of that family's normal, giving 5
        // horizontal lines and 6 vertical. The property that does hold is
        // that the result is the families concatenated.
        assert!(!horizontal.is_empty());
        assert!(!vertical.is_empty());
        assert_ne!(horizontal.len(), vertical.len()); // the asymmetry above, pinned
        assert_eq!(both.len(), horizontal.len() + vertical.len());
    }

    #[test]
    fn a_boundary_that_cannot_be_hatched_yields_no_segments_rather_than_failing() {
        // A renderer asks this once per entity per frame and has nothing
        // useful to do with an error. The model has already refused an
        // invalid pattern.
        let mut open = square(10.0);
        open.closed = false;
        assert!(hatch_segments(&open, &line_family("hatch", 1.0)).is_empty());

        let solid = HatchPattern {
            name: String::from("filled"),
            solid: true,
            ..HatchPattern::default()
        };
        assert!(
            hatch_segments(&square(10.0), &solid).is_empty(),
            "a solid fill has no lines"
        );
    }

    // ---- resolution through the ByLayer chain -------------------------------

    #[test]
    fn an_entity_takes_its_hatch_from_its_layer_and_a_style_overrides_it() {
        let mut model = Model::default();
        assert!(model.hatch_patterns.add(line_family("rock", 1.0)).is_ok());
        assert!(model.hatch_patterns.add(line_family("grass", 2.0)).is_ok());
        assert!(model.layers.add(hatched_layer("ground", "rock")).is_ok());

        let mut entity = square_on("ground");
        let resolved = resolve_hatch_pattern(&model, &entity).expect("hatched by its layer");
        assert_eq!(resolved.name, "rock");

        let style = Style {
            name: String::from("lawn"),
            hatch_pattern: String::from("grass"),
            ..Style::default()
        };
        assert!(model.styles.add(style).is_ok());
        entity.style = String::from("lawn");
        let resolved = resolve_hatch_pattern(&model, &entity).expect("hatched by its style");
        assert_eq!(resolved.name, "grass");
    }

    #[test]
    fn a_style_that_says_nothing_about_hatching_leaves_the_layer_pattern_alone() {
        // Style::hatch_pattern is empty by default, which means ByLayer. If an
        // empty style field overrode the layer, naming any style at all would
        // silently un-hatch the entity.
        let mut model = Model::default();
        assert!(model.hatch_patterns.add(line_family("rock", 1.0)).is_ok());
        assert!(model.layers.add(hatched_layer("ground", "rock")).is_ok());
        let style = Style {
            name: String::from("thin"), // no hatch pattern named
            ..Style::default()
        };
        assert!(model.styles.add(style).is_ok());

        let mut entity = square_on("ground");
        entity.style = String::from("thin");
        let resolved = resolve_hatch_pattern(&model, &entity).expect("hatched by its layer");
        assert_eq!(resolved.name, "rock");
    }

    #[test]
    fn an_entity_on_an_unhatched_or_missing_layer_resolves_to_nothing() {
        let model = Model::default();
        let mut entity = square_on("0"); // the default layer, which is not hatched
        assert!(resolve_hatch_pattern(&model, &entity).is_none());

        // A layer that has been removed. The model permits the dangling
        // reference and a renderer must not crash on it.
        entity.layer = String::from("deleted");
        assert!(resolve_hatch_pattern(&model, &entity).is_none());
    }

    #[test]
    fn a_pattern_that_has_been_deleted_reads_as_unhatched_rather_than_crashing() {
        let mut model = Model::default();
        // "rock" is never added to the table.
        assert!(model.layers.add(hatched_layer("ground", "rock")).is_ok());

        let entity = square_on("ground");
        assert!(resolve_hatch_pattern(&model, &entity).is_none());
    }
}
