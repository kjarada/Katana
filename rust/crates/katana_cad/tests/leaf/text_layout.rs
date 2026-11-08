// Text in its style at a scale (tests/cad/annotation/test_text_layout.cpp;
// docs/annotation.md "Text"): the paper-size arithmetic at several scales,
// justification with measured widths, the readable turn, masks, and a text
// written before styles laid out exactly as it always was drawn.

use std::sync::Arc;

use crate::{double_eq, near};
use katana_cad::annotation::drawing::{TextFace, TextMeasure};
use katana_cad::annotation::text_layout::{
    TextAppearance, is_paper_sized, layout_text, layout_text_entity, readable_rotation,
    resolve_text_style,
};
use katana_entity::text_block::{LINE_PITCH, character_count};
use katana_entity::{Model, TextGeometry, TextJustify, TextStyle};
use katana_geometry::{Box2, Point2};
use katana_math::numerics::{PI, normalize_angle};

// A measure of exactly 0.5 per character, so widths are round numbers.
fn half_per_character() -> TextMeasure {
    Arc::new(|line: &str, _face: &TextFace| 0.5 * character_count(line) as f64)
}

fn model_with_road_style() -> Model {
    let mut model = Model::default();
    let road = TextStyle {
        name: String::from("Road"),
        paper_height: 2.5,
        width_factor: 0.8,
        mask: true,
        mask_margin: 1.0,
        ..TextStyle::default()
    };
    assert!(model.text_styles.add(road).is_ok());
    model
}

fn text(x: f64, y: f64, text: &str, height: f64, rotation: f64) -> TextGeometry {
    TextGeometry {
        position: Point2::new(x, y),
        text: String::from(text),
        height,
        rotation,
        ..TextGeometry::default()
    }
}

fn bounds_of(points: &[Point2]) -> Box2 {
    let mut bounds = Box2::default();
    for &point in points {
        bounds.expand(point);
    }
    bounds
}

mod text_layout {
    use super::*;

    #[test]
    fn a_paper_height_is_the_same_on_paper_at_every_scale() {
        let model = model_with_road_style();
        let mut road = text(0.0, 0.0, "A", 99.0, 0.0);
        road.style = String::from("Road");
        // 2.5 mm at 1:200, 1:500, 1:2000 and 1:10000: 0.5, 1.25, 5 and 25 m,
        // which is 2.5 mm every time once the sheet divides by its scale.
        for scale in [200.0, 500.0, 2000.0, 10000.0] {
            let a = resolve_text_style(&model, &road, scale);
            double_eq(a.height, 2.5 * scale / 1000.0);
            double_eq(a.height / scale * 1000.0, 2.5);
        }
        // The text's own paper height beats its style's.
        road.paper_height = 5.0;
        double_eq(resolve_text_style(&model, &road, 200.0).height, 1.0);
    }

    #[test]
    fn a_text_from_before_styles_keeps_its_model_height_at_every_scale() {
        let model = Model::default();
        let old = text(3.0, 4.0, "OLD", 1.8, 0.4);
        assert!(!is_paper_sized(&model, &old));
        for scale in [200.0, 5000.0] {
            let a = resolve_text_style(&model, &old, scale);
            double_eq(a.height, 1.8);
            assert!(!a.readable, "a text in no style is never turned");
            assert!(!a.mask);
        }
        // Laid out, it is one run at its position: exactly the old drawing.
        let d = layout_text_entity(&model, &old, 500.0, &half_per_character());
        assert_eq!(d.texts.len(), 1);
        assert_eq!(d.texts[0].origin, Point2::new(3.0, 4.0));
        double_eq(d.texts[0].height, 1.8);
        double_eq(d.texts[0].rotation, 0.4);
    }

    #[test]
    fn justification_uses_the_measured_widths() {
        let a = TextAppearance {
            height: 2.0,
            ..TextAppearance::default()
        };
        // "ABCD" is 4 x 0.5 x 2 = 4 wide; "AB" is 2. Centred at (10, 10):
        // the block is 4 wide, so the first line starts at 8 and the second,
        // centred in it, at 9. Two lines at height 2: the block is 2 + 10/3
        // tall, its middle at 10.
        let d = layout_text(
            "ABCD\nAB",
            Point2::new(10.0, 10.0),
            0.0,
            TextJustify::MiddleCentre,
            &a,
            &half_per_character(),
        );
        assert_eq!(d.texts.len(), 2);
        let block = 2.0 + LINE_PITCH * 2.0;
        double_eq(d.texts[0].origin.x, 8.0);
        double_eq(d.texts[1].origin.x, 9.0);
        near(d.texts[0].origin.y, 10.0 + 0.5 * block - 2.0, 1e-12);
        near(d.texts[1].origin.y, 10.0 - 0.5 * block, 1e-12);
        // Right-justified: every line ends at the point.
        let r = layout_text(
            "ABCD\nAB",
            Point2::new(10.0, 10.0),
            0.0,
            TextJustify::TopRight,
            &a,
            &half_per_character(),
        );
        double_eq(r.texts[0].origin.x, 6.0);
        double_eq(r.texts[1].origin.x, 8.0);
        double_eq(r.texts[0].origin.y, 8.0); // the top of the first line at the point
    }

    #[test]
    fn a_readable_text_is_turned_and_covers_the_same_place() {
        let mut a = TextAppearance {
            height: 1.0,
            readable: true,
            ..TextAppearance::default()
        };
        // Pointing west (180 degrees) reads upside down: turned to 0, and its
        // bottom-left anchor becomes top-right, so the block sits where it
        // did.
        let turned = layout_text(
            "AB",
            Point2::new(0.0, 0.0),
            PI,
            TextJustify::BottomLeft,
            &a,
            &half_per_character(),
        );
        assert_eq!(turned.texts.len(), 1);
        near(normalize_angle(turned.texts[0].rotation), 0.0, 1e-12);
        a.readable = false;
        let as_is = layout_text(
            "AB",
            Point2::new(0.0, 0.0),
            PI,
            TextJustify::BottomLeft,
            &a,
            &half_per_character(),
        );
        // Same block, drawn the other way up: compare the boxes' extents.
        let one = bounds_of(&turned.text_boxes[0]);
        let two = bounds_of(&as_is.text_boxes[0]);
        near(one.min.x, two.min.x, 1e-9);
        near(one.max.x, two.max.x, 1e-9);
        // The descender allowance is on the other side once turned, so the
        // heights may differ by it; the text body covers the same ground.
        near(one.max.y - one.min.y, two.max.y - two.min.y, 1e-9);

        // Straight up (90 degrees) reads from the right of the sheet: left
        // alone.
        assert!(!readable_rotation(0.5 * PI, TextJustify::BottomLeft).turned);
        assert!(readable_rotation(1.5 * PI, TextJustify::BottomLeft).turned);
        assert_eq!(
            readable_rotation(PI, TextJustify::MiddleLeft).justify,
            TextJustify::MiddleRight
        );
    }

    #[test]
    fn a_mask_reaches_past_the_text_by_its_paper_margin() {
        let model = model_with_road_style();
        let mut road = text(0.0, 0.0, "AAAA", 1.0, 0.0);
        road.style = String::from("Road");
        // At 1:1000 the text is 2.5 tall and 4 x 0.5 x 2.5 x 0.8 = 4 wide;
        // the mask is 1 mm = 1 unit beyond it all round.
        let d = layout_text_entity(&model, &road, 1000.0, &half_per_character());
        assert_eq!(d.masks.len(), 1);
        let mask = bounds_of(&d.masks[0]);
        double_eq(mask.min.x, -1.0);
        double_eq(mask.max.x, 5.0);
        double_eq(mask.max.y, 3.5);
        double_eq(d.texts[0].width_factor, 0.8);
    }
}
