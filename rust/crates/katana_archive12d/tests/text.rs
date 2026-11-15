//! The twin of tests/archive12d/test_text.cpp: text placed as the archive's
//! program draws it (PLAN.MD 20.2, slice 7) - justification, the text's own
//! colour and style, an offset in model units, and a raise that is a level and
//! not a plan displacement.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use std::f64::consts::PI;

use katana_archive12d::domain::standard_colour;
use katana_entity::TextGeometry;
use katana_geometry::Point2;

use support::{double_eq, import, meta, near, prop_real, shape};

// "ABCD" at 2 m: the width estimate is 4 characters x 2 m x 0.6 = 4.8 m.
const WIDTH: f64 = 4.0 * 2.0 * 0.6;

fn text_at(justify: &str) -> String {
    format!(
        "string text {{ name t x 100 y 200 z 5 text \"ABCD\"\n  angle 0 worldsize 2 justify \"{justify}\" }}"
    )
}

mod text_annotation {
    use super::*;

    #[test]
    fn justification_moves_the_anchor_to_where_the_baseline_starts() {
        // A Katana text's position IS the left end of its baseline, so the
        // archive's anchor has to be moved by the justification it was placed
        // with. Every annotation in the sample archives is bottom-left, which
        // is already that, and the other eight must not be silently treated
        // as it.
        let left = import(&text_at("bottom-left"));
        assert_eq!(left.entities.len(), 1);
        assert_eq!(
            shape::<TextGeometry>(&left.entities[0].geometry).position,
            Point2::new(100.0, 200.0),
            "bottom-left is already what a Katana text position means"
        );

        let centre = import(&text_at("bottom-centre"));
        let middle = shape::<TextGeometry>(&centre.entities[0].geometry);
        near(middle.position.x, 100.0 - WIDTH / 2.0, 1e-12);
        near(middle.position.y, 200.0, 1e-12);

        let right = import(&text_at("bottom-right"));
        near(
            shape::<TextGeometry>(&right.entities[0].geometry)
                .position
                .x,
            100.0 - WIDTH,
            1e-12,
        );

        // Vertically: a top-anchored text hangs below its anchor by its
        // height.
        let top = import(&text_at("top-left"));
        near(
            shape::<TextGeometry>(&top.entities[0].geometry).position.y,
            200.0 - 2.0,
            1e-12,
        );
        let mid = import(&text_at("middle-left"));
        near(
            shape::<TextGeometry>(&mid.entities[0].geometry).position.y,
            200.0 - 1.0,
            1e-12,
        );
    }

    #[test]
    fn justification_is_measured_along_the_text_not_along_the_axes() {
        // Rotated a quarter turn, "along the text" is north and "across" is
        // west.
        let domain = import(
            "string text { name t x 100 y 200 z 5 text \"ABCD\"\n  angle 90 worldsize 2 justify \"top-right\" }",
        );
        let text = shape::<TextGeometry>(&domain.entities[0].geometry);
        near(text.rotation, PI / 2.0, 1e-12);
        // along = -width (right), across = -height (top), turned 90 degrees:
        // x gets +height, y gets -width.
        near(text.position.x, 100.0 + 2.0, 1e-12);
        near(text.position.y, 200.0 - WIDTH, 1e-12);
    }

    #[test]
    fn the_texts_own_colour_and_style_are_its_own_and_not_the_strings() {
        let domain = import(
            "string text { name t colour red x 0 y 0 z 0 text \"RL\"\n  \
             angle 0 worldsize 1 justify \"bottom-left\" text_colour cyan textstyle \"Arial\" }",
        );
        assert_eq!(domain.entities.len(), 1);
        let entity = &domain.entities[0];
        assert_eq!(
            entity.color,
            standard_colour("cyan"),
            "the annotation's colour, not red"
        );
        assert_eq!(entity.style, "Arial");
        assert_eq!(
            meta(entity, "12d.colour"),
            Some("red"),
            "the string's own colour is still kept"
        );
        assert!(
            domain
                .styles_needed
                .iter()
                .any(|style| style.name == "Arial"),
            "a textstyle is a style like any other"
        );
    }

    #[test]
    fn no_colour_means_the_strings_colour_rather_than_none() {
        // The format's program writes "no_colour" on the annotation of text
        // that takes the string's colour; treating it as a colour name would
        // leave the text ByLayer and lose the string's.
        let domain = import(
            "string text { name t colour green x 0 y 0 z 0 text \"RL\"\n  \
             angle 0 worldsize 1 justify \"bottom-left\" text_colour \"no_colour\" }",
        );
        assert_eq!(domain.entities.len(), 1);
        assert_eq!(domain.entities[0].color, standard_colour("green"));
        assert_eq!(meta(&domain.entities[0], "12d.text.colour"), None);
    }

    #[test]
    fn an_offset_in_model_units_moves_the_text_and_one_in_paper_millimetres_is_kept() {
        // An offset is in the same units as the size. worldsize is model
        // units, so it can be applied; papersize is millimetres on a plot,
        // which has no model-unit meaning without a plot scale - the rule
        // text_height follows.
        let world = import(
            "string text { name t x 10 y 20 z 0 text \"A\"\n  \
             angle 0 worldsize 1 offset 3 justify \"bottom-left\" }",
        );
        let moved = shape::<TextGeometry>(&world.entities[0].geometry);
        near(moved.position.x, 10.0, 1e-12);
        near(moved.position.y, 23.0, 1e-12); // perpendicular to the text, to its left
        assert_eq!(
            meta(&world.entities[0], "12d.text.offset"),
            Some("3"),
            "and kept, to write back"
        );

        let paper = import(
            "string text { name t x 10 y 20 z 0 text \"A\"\n  \
             angle 0 papersize 10 offset 12 justify \"bottom-left\" }",
        );
        let still = shape::<TextGeometry>(&paper.entities[0].geometry);
        near(still.position.x, 10.0, 1e-12);
        near(still.position.y, 20.0, 1e-12); // 12 mm on paper is not 12 m on the ground
        assert_eq!(meta(&paper.entities[0], "12d.text.offset"), Some("12"));
        assert_eq!(meta(&paper.entities[0], "12d.text.papersize"), Some("10"));
    }

    #[test]
    fn a_raise_is_a_level_and_not_a_plan_displacement() {
        let domain = import(
            "string text { name t x 10 y 20 z 5 text \"A\"\n  \
             angle 0 worldsize 1 raise 2.5 justify \"bottom-left\" }",
        );
        assert_eq!(domain.entities.len(), 1);
        let text = shape::<TextGeometry>(&domain.entities[0].geometry);
        assert_eq!(
            text.position,
            Point2::new(10.0, 20.0),
            "a raise moves nothing in plan"
        );
        double_eq(prop_real(&domain.entities[0].properties, "elevation"), 7.5);
        assert_eq!(meta(&domain.entities[0], "12d.text.raise"), Some("2.5"));
    }

    #[test]
    fn what_katanas_text_has_no_field_for_is_kept_rather_than_dropped() {
        let domain = import(
            "string text { name t x 0 y 0 z 0 text \"A\"\n  \
             angle 0 worldsize 1 slant 15 x_factor 0.8 justify \"middle-centre\" }",
        );
        assert_eq!(domain.entities.len(), 1);
        let entity = &domain.entities[0];
        assert_eq!(meta(entity, "12d.text.slant"), Some("15"));
        assert_eq!(meta(entity, "12d.text.x_factor"), Some("0.8"));
        assert_eq!(
            meta(entity, "12d.text.justify"),
            Some("middle-centre"),
            "kept, because the position it produced cannot be turned back into it"
        );
    }

    #[test]
    fn vertex_and_segment_text_are_annotated_the_same_way_as_a_standalone_text() {
        // One annotation applies to every vertex, which is how the format's
        // program writes a string whose vertices are all labelled the same
        // way.
        let domain = import(
            r#"string super { name s data_3d { 0 0 0  10 0 0 }
  vertex_text_data { "A" "B" }
  vertex_annotate_value { angle 0 worldsize 2 offset 1 justify "bottom-left" colour cyan } }"#,
        );
        // The string, then its two vertex texts.
        assert_eq!(domain.entities.len(), 3);
        let first = shape::<TextGeometry>(&domain.entities[1].geometry);
        near(first.position.y, 1.0, 1e-12); // offset applied to vertex text too
        assert_eq!(domain.entities[1].color, standard_colour("cyan"));
        assert_eq!(domain.entities[2].color, standard_colour("cyan"));
    }
}
