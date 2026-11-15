// Dimension geometry (PLAN.MD Phase 09; tests/cad/test_dimension_draw.cpp).
//
// One builder feeds the 2D viewport, the 3D scene and the cull box, so these
// assertions are what keeps all three showing the same dimension. Everything
// is in MODEL units: the viewport previously drew a fixed 5-pixel tick and a
// 12-pixel label, which looks correct on screen and plots at whatever size the
// paper happens to give it.

use crate::{double_eq, near};
use katana_cad::DimensionDrawing;
use katana_cad::dimension_draw::{build_dimension, resolve_dimension_style};
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{ArrowHead, DimensionGeometry, DimensionStyle, Entity, Geometry, Layer, Model};
use katana_geometry::Point2;

/// A 10 m dimension along the x axis, offset 2 m to the left (+y).
fn horizontal(offset: f64) -> DimensionGeometry {
    DimensionGeometry::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), offset, "")
}

fn plain_style() -> DimensionStyle {
    DimensionStyle {
        text_height: 0.5,
        text_gap: 0.2,
        extension_offset: 0.1,
        extension_beyond: 0.3,
        arrow_size: 0.6,
        arrow_head: ArrowHead::Tick,
        decimals: 2,
        ..DimensionStyle::default()
    }
}

/// build_dimension at the default annotation scale, the C++'s default.
fn build(dimension: &DimensionGeometry, style: &DimensionStyle) -> DimensionDrawing {
    build_dimension(dimension, style, DEFAULT_ANNOTATION_SCALE)
}

mod dimension_draw {
    use super::*;

    #[test]
    fn the_dimension_line_sits_at_the_offset_and_spans_the_measurement() {
        let drawing = build(&horizontal(2.0), &plain_style());

        double_eq(drawing.dimension_line.start.x, 0.0);
        double_eq(drawing.dimension_line.start.y, 2.0);
        double_eq(drawing.dimension_line.end.x, 10.0);
        double_eq(drawing.dimension_line.end.y, 2.0);
        double_eq(drawing.dimension_line.length(), 10.0);
    }

    #[test]
    fn extension_lines_leave_a_gap_at_the_feature_and_overshoot_the_dimension_line() {
        // DIMEXO and DIMEXE. Without the gap the extension line touches the
        // thing being measured, which is wrong on a plan; without the
        // overshoot the dimension line has nothing to run into.
        let drawing = build(&horizontal(2.0), &plain_style());

        assert_eq!(drawing.extension_lines.len(), 2);
        // Starts 0.1 above the measured point, ends 0.3 past the dimension
        // line.
        double_eq(drawing.extension_lines[0].start.x, 0.0);
        double_eq(drawing.extension_lines[0].start.y, 0.1);
        double_eq(drawing.extension_lines[0].end.y, 2.3);
        double_eq(drawing.extension_lines[1].start.x, 10.0);
        double_eq(drawing.extension_lines[1].end.y, 2.3);
    }

    #[test]
    fn a_negative_offset_mirrors_the_whole_construction() {
        // Everything that means "away from the measured points" has to flip
        // together. Getting only some of it right turns the dimension inside
        // out: the line on one side and the extensions and text on the other.
        let drawing = build(&horizontal(-2.0), &plain_style());

        double_eq(drawing.dimension_line.start.y, -2.0);
        assert_eq!(drawing.extension_lines.len(), 2);
        // The gap is below now, and so is the overshoot.
        double_eq(drawing.extension_lines[0].start.y, -0.1);
        double_eq(drawing.extension_lines[0].end.y, -2.3);
        assert!(
            drawing.text_anchor.y < -2.0,
            "the text must be on the same side as the line"
        );
    }

    #[test]
    fn the_label_is_formatted_through_the_style() {
        let mut style = plain_style();
        style.decimals = 2;
        style.suffix = String::from(" m");
        assert_eq!(build(&horizontal(2.0), &style).text, "10.00 m");

        style.unit_scale = 1000.0;
        style.decimals = 0;
        style.suffix = String::from(" mm");
        assert_eq!(build(&horizontal(2.0), &style).text, "10000 mm");

        // An override wins verbatim, style and all.
        let mut overridden = horizontal(2.0);
        overridden.text_override = String::from("TYP");
        assert_eq!(build(&overridden, &style).text, "TYP");
    }

    #[test]
    fn the_text_is_sized_in_model_units_and_centred_on_the_dimension_line() {
        let mut style = plain_style();
        style.text_height = 0.5;
        let drawing = build(&horizontal(2.0), &style);

        // Model units, not pixels.
        double_eq(drawing.text_height, 0.5);
        // Sat DIMGAP above the dimension line.
        double_eq(drawing.text_anchor.y, 2.2);
        // Backed up half the text width from the middle, so it reads centred.
        // The advance is 0.6 of the height per character; "10.00" is five
        // characters, so the half width is 0.5 * 0.6 * 5 / 2 = 0.75.
        near(drawing.text_anchor.x, 5.0 - 0.75, 1e-9);
    }

    #[test]
    fn the_label_is_never_upside_down() {
        // A dimension measured right to left runs at 180 degrees; rotating the
        // text with it would put it on its head. It flips instead, which is
        // what every CAD package does and what a plan has to do to be
        // readable.
        let backwards =
            DimensionGeometry::new(Point2::new(10.0, 0.0), Point2::new(0.0, 0.0), 2.0, "");
        let drawing = build(&backwards, &plain_style());

        assert!(
            drawing.text_rotation.abs() <= 1.5707963267948966 + 1e-9,
            "rotation {} would read upside down",
            drawing.text_rotation
        );

        // Straight up is the boundary case: a vertical dimension reads bottom
        // to top, not top to bottom.
        let vertical =
            DimensionGeometry::new(Point2::new(0.0, 0.0), Point2::new(0.0, 10.0), 2.0, "");
        assert!(build(&vertical, &plain_style()).text_rotation.abs() <= 1.5707963267948966 + 1e-9);
    }

    #[test]
    fn each_arrow_head_produces_its_own_shape() {
        let mut style = plain_style();

        style.arrow_head = ArrowHead::None;
        let drawing = build(&horizontal(2.0), &style);
        assert!(drawing.arrow_strokes.is_empty());
        assert!(drawing.arrow_fills.is_empty());

        style.arrow_head = ArrowHead::Tick;
        let drawing = build(&horizontal(2.0), &style);
        assert_eq!(drawing.arrow_strokes.len(), 2, "one oblique stroke per end");
        assert!(drawing.arrow_fills.is_empty());

        style.arrow_head = ArrowHead::Open;
        let drawing = build(&horizontal(2.0), &style);
        assert_eq!(drawing.arrow_strokes.len(), 4, "two strokes per end");

        style.arrow_head = ArrowHead::ClosedFilled;
        let drawing = build(&horizontal(2.0), &style);
        assert!(drawing.arrow_strokes.is_empty());
        assert_eq!(drawing.arrow_fills.len(), 2);
        assert_eq!(drawing.arrow_fills[0].len(), 3, "a triangle");

        style.arrow_head = ArrowHead::Dot;
        let drawing = build(&horizontal(2.0), &style);
        assert_eq!(drawing.arrow_fills.len(), 2);
        assert!(
            drawing.arrow_fills[0].len() > 6,
            "a polygon approximating a circle"
        );
    }

    #[test]
    fn arrows_point_outward_from_the_dimension_line_ends() {
        let mut style = plain_style();
        style.arrow_head = ArrowHead::ClosedFilled;
        style.arrow_size = 0.6;
        let drawing = build(&horizontal(2.0), &style);

        assert_eq!(drawing.arrow_fills.len(), 2);
        // First arrow tips at the left end and its body extends to the RIGHT.
        double_eq(drawing.arrow_fills[0][0].x, 0.0);
        assert!(
            drawing.arrow_fills[0][1].x > 0.0,
            "the body must be inside the dimension"
        );
        // Second tips at the right end with its body to the left.
        double_eq(drawing.arrow_fills[1][0].x, 10.0);
        assert!(drawing.arrow_fills[1][1].x < 10.0);
    }

    #[test]
    fn the_extent_covers_what_is_drawn_not_just_what_is_measured() {
        // THE reason the extent is computed here. entity's bounding_box for a
        // Dimension covers only the measured points and the dimension line, so
        // a viewport culling against it drops a dimension whose label is still
        // on screen.
        let drawing = build(&horizontal(2.0), &plain_style());

        assert!(!drawing.extent.empty());
        // Above the text, which sits at 2.2 and is 0.5 tall.
        assert!(drawing.extent.max.y >= 2.7 - 1e-9);
        // Below the measured points.
        assert!(drawing.extent.min.y <= 1e-9);
        // And wider than the measurement, because the text is centred and the
        // arrows have width.
        assert!(drawing.extent.min.x <= 1e-9);
        assert!(drawing.extent.max.x >= 10.0 - 1e-9);

        // Strictly bigger than what the entity layer reports.
        let plain = bounding_box(&Geometry::Dimension(horizontal(2.0)));
        assert!(
            drawing.extent.max.y > plain.max.y,
            "the drawn extent must exceed the measured one, or culling is wrong"
        );
    }

    #[test]
    fn a_degenerate_dimension_draws_nothing_rather_than_guessing() {
        let style = plain_style();

        // Coincident points: no direction to offset along.
        let point = DimensionGeometry::new(Point2::new(5.0, 5.0), Point2::new(5.0, 5.0), 2.0, "");
        assert!(build(&point, &style).empty());

        // A non-finite offset would put the dimension line at infinity.
        let broken =
            DimensionGeometry::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), f64::NAN, "");
        assert!(build(&broken, &style).empty());

        // A zero offset is legitimate: the dimension line runs through the
        // points.
        assert!(!build(&horizontal(0.0), &style).empty());
    }

    // ---- resolution -------------------------------------------------------------------

    #[test]
    fn a_style_resolves_through_the_layer_then_the_document_default() {
        let mut model = Model::default();

        let site = DimensionStyle {
            name: String::from("site"),
            decimals: 1,
            ..DimensionStyle::default()
        };
        assert!(model.dimension_styles.add(site).is_ok());

        let styled = Layer {
            name: String::from("dims"),
            dimension_style: String::from("site"),
            ..Layer::default()
        };
        assert!(model.layers.add(styled).is_ok());

        let on_styled_layer = Entity {
            layer: String::from("dims"),
            geometry: Geometry::Dimension(horizontal(2.0)),
            ..Entity::default()
        };
        assert_eq!(
            resolve_dimension_style(&model, &on_styled_layer).decimals,
            1
        );

        // A layer naming no style falls through to Standard.
        let on_plain_layer = Entity {
            layer: String::from("0"),
            geometry: Geometry::Dimension(horizontal(2.0)),
            ..Entity::default()
        };
        assert_eq!(
            resolve_dimension_style(&model, &on_plain_layer).name,
            "Standard"
        );
    }

    #[test]
    fn a_missing_layer_or_style_still_draws_in_the_default() {
        // A dimension on a deleted layer, or one naming a style that has gone,
        // must still appear: leaving it off the drawing because its styling is
        // missing would remove information from a plan.
        let mut model = Model::default();

        let dangling = Layer {
            name: String::from("dims"),
            dimension_style: String::from("no-such-style"),
            ..Layer::default()
        };
        assert!(model.layers.add(dangling).is_ok());

        let on_dangling = Entity {
            layer: String::from("dims"),
            geometry: Geometry::Dimension(horizontal(2.0)),
            ..Entity::default()
        };
        assert_eq!(
            resolve_dimension_style(&model, &on_dangling).name,
            "Standard"
        );

        let orphan = Entity {
            layer: String::from("deleted"),
            geometry: Geometry::Dimension(horizontal(2.0)),
            ..Entity::default()
        };
        assert_eq!(resolve_dimension_style(&model, &orphan).name, "Standard");

        // And the resolved style actually produces a drawing.
        assert!(!build(&horizontal(2.0), resolve_dimension_style(&model, &orphan)).empty());
    }
}
