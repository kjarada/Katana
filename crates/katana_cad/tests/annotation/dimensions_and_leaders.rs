// Dimensions of every kind and leaders (annotation/dimension_build.rs,
// dimension_draw.rs, annotation/leader_draw.rs; docs/annotation.md
// "Dimensions" and "Leaders and callouts"; tests/cad/annotation/
// test_dimensions_and_leaders.cpp): what each kind measures and draws,
// paper-sized styles at several scales, chains, and callouts.

use std::sync::Arc;

use katana_cad::annotation::dimension_build::{
    AnchoredPoint, angular_dimension, baseline_dimensions, continued_dimensions, linear_dimension,
    ordinate_dimension,
};
use katana_cad::annotation::drawing::{TextFace, TextMeasure, estimated_measure};
use katana_cad::annotation::leader_draw::build_leader;
use katana_cad::dimension_draw::build_dimension;
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::text_block::character_count;
use katana_entity::{
    CalloutShape, DimensionGeometry, DimensionKind, DimensionStyle, LeaderContext, LeaderGeometry,
    Model,
};
use katana_geometry::{Box2, Point2};
use katana_math::numerics::PI;

use crate::{double_eq, near};

fn free(x: f64, y: f64) -> AnchoredPoint {
    AnchoredPoint {
        point: Point2::new(x, y),
        ..AnchoredPoint::default()
    }
}

fn leader(vertices: Vec<Point2>, text: &str) -> LeaderGeometry {
    LeaderGeometry {
        vertices,
        text: String::from(text),
        ..LeaderGeometry::default()
    }
}

mod dimension_build {
    use super::*;

    #[test]
    fn linear_reads_horizontal_or_vertical_from_where_it_is_dragged() {
        // Points (0,0) and (10,4). Dragged above them: horizontal, 10.
        let above = linear_dimension(
            &free(0.0, 0.0),
            &free(10.0, 4.0),
            Point2::new(5.0, 9.0),
            None,
        )
        .expect("a dimension");
        double_eq(above.angle, 0.0);
        double_eq(above.measurement(), 10.0);
        // The line through the dragged point, from start.
        double_eq(above.offset, 9.0);
        // Beside them: vertical, 4.
        let beside = linear_dimension(
            &free(0.0, 0.0),
            &free(10.0, 4.0),
            Point2::new(14.0, 2.0),
            None,
        )
        .expect("a dimension");
        near(beside.measurement(), 4.0, 1e-12);
        // At an angle of 30 degrees: (10, 4) . (cos 30, sin 30) = 8.660 + 2 = 10.660.
        let rotated = linear_dimension(
            &free(0.0, 0.0),
            &free(10.0, 4.0),
            Point2::new(0.0, 10.0),
            Some(30.0 * PI / 180.0),
        )
        .expect("a dimension");
        near(
            rotated.measurement(),
            10.0 * 3.0_f64.sqrt() / 2.0 + 2.0,
            1e-12,
        );
        assert!(
            linear_dimension(
                &free(0.0, 0.0),
                &free(0.0, 5.0),
                Point2::new(3.0, 9.0),
                Some(0.0)
            )
            .is_err(),
            "nothing to measure horizontally"
        );
    }

    #[test]
    fn angular_measures_the_side_the_arc_is_dragged_to() {
        let inside = angular_dimension(
            &free(0.0, 0.0),
            &free(10.0, 0.0),
            &free(0.0, 10.0),
            Some(Point2::new(3.0, 3.0)),
        )
        .expect("a dimension");
        near(inside.measurement(), 0.5 * PI, 1e-12);
        near(inside.offset, 18.0_f64.sqrt(), 1e-12);
        let outside = angular_dimension(
            &free(0.0, 0.0),
            &free(10.0, 0.0),
            &free(0.0, 10.0),
            Some(Point2::new(-3.0, -3.0)),
        )
        .expect("a dimension");
        // The reflex angle.
        near(outside.measurement(), 1.5 * PI, 1e-12);
    }

    #[test]
    fn ordinate_takes_its_axis_from_the_leader() {
        // Leader straight up from the feature: the X ordinate.
        let x = ordinate_dimension(
            &free(100.0, 100.0),
            &free(130.0, 110.0),
            Point2::new(130.0, 140.0),
            None,
        )
        .expect("a dimension");
        assert_eq!(x.kind, DimensionKind::OrdinateX);
        double_eq(x.measurement(), 30.0);
        let y = ordinate_dimension(
            &free(100.0, 100.0),
            &free(130.0, 110.0),
            Point2::new(160.0, 110.0),
            None,
        )
        .expect("a dimension");
        assert_eq!(y.kind, DimensionKind::OrdinateY);
        double_eq(y.measurement(), 10.0);
    }

    #[test]
    fn baseline_and_continued_chains() {
        let base = linear_dimension(
            &free(0.0, 0.0),
            &free(10.0, 0.0),
            Point2::new(5.0, 5.0),
            Some(0.0),
        )
        .expect("a dimension");
        let baseline =
            baseline_dimensions(&base, &[free(20.0, 0.0), free(35.0, 0.0)], 3.0).expect("a chain");
        assert_eq!(baseline.len(), 2);
        assert_eq!(
            baseline[0].start,
            Point2::new(0.0, 0.0),
            "every one from the base's first point"
        );
        double_eq(baseline[0].measurement(), 20.0);
        double_eq(baseline[0].offset, 8.0);
        // Each a spacing further out.
        double_eq(baseline[1].offset, 11.0);

        let continued =
            continued_dimensions(&base, &[free(20.0, 2.0), free(35.0, -1.0)]).expect("a chain");
        assert_eq!(continued.len(), 2);
        assert_eq!(
            continued[0].start,
            Point2::new(10.0, 0.0),
            "from where the last one ended"
        );
        double_eq(continued[0].measurement(), 10.0);
        double_eq(continued[1].measurement(), 15.0);
        // Every dimension line on the base's: y = 5.
        for d in &continued {
            let drawing = build_dimension(d, &DimensionStyle::default(), DEFAULT_ANNOTATION_SCALE);
            near(drawing.dimension_line.start.y, 5.0, 1e-12);
        }
    }
}

mod dimension_draw {
    use super::*;

    #[test]
    fn every_kind_draws_and_shows_its_value() {
        let style = DimensionStyle {
            decimals: 2,
            ..DimensionStyle::default()
        };
        let angular = DimensionGeometry {
            kind: DimensionKind::Angular,
            vertex: Point2::new(0.0, 0.0),
            start: Point2::new(10.0, 0.0),
            end: Point2::new(0.0, 10.0),
            offset: 5.0,
            ..DimensionGeometry::default()
        };
        let a = build_dimension(&angular, &style, DEFAULT_ANNOTATION_SCALE);
        assert!(!a.has_dimension_line);
        assert_eq!(a.curves.len(), 1);
        // The arc's radius.
        near(
            a.curves[0][0].distance_to(Point2::new(0.0, 0.0)),
            5.0,
            1e-12,
        );
        assert_eq!(a.text, "90\u{b0}00'00\"");

        let mut radius = DimensionGeometry {
            kind: DimensionKind::Radius,
            vertex: Point2::new(0.0, 0.0),
            start: Point2::new(3.0, 4.0),
            ..DimensionGeometry::default()
        };
        assert_eq!(
            build_dimension(&radius, &style, DEFAULT_ANNOTATION_SCALE).text,
            "R5.00"
        );
        radius.kind = DimensionKind::Diameter;
        assert_eq!(
            build_dimension(&radius, &style, DEFAULT_ANNOTATION_SCALE).text,
            "\u{d8}10.00"
        );
        radius.text_override = String::from("SEE NOTE");
        assert_eq!(
            build_dimension(&radius, &style, DEFAULT_ANNOTATION_SCALE).text,
            "SEE NOTE",
            "an override is verbatim"
        );

        let ordinate = DimensionGeometry {
            kind: DimensionKind::OrdinateX,
            vertex: Point2::new(0.0, 0.0),
            start: Point2::new(-7.5, 3.0),
            end: Point2::new(-7.5, 10.0),
            ..DimensionGeometry::default()
        };
        assert_eq!(
            build_dimension(&ordinate, &style, DEFAULT_ANNOTATION_SCALE).text,
            "-7.50"
        );
    }

    #[test]
    fn a_paper_sized_style_is_the_same_on_paper_at_every_scale() {
        let paper = DimensionStyle {
            paper_sized: true,
            text_height: 2.5,
            arrow_size: 3.0,
            ..DimensionStyle::default()
        };
        let dimension =
            DimensionGeometry::new(Point2::new(0.0, 0.0), Point2::new(1000.0, 0.0), 50.0, "");
        for scale in [200.0, 1000.0, 2500.0] {
            let drawing = build_dimension(&dimension, &paper, scale);
            double_eq(drawing.text_height, 2.5 * scale / 1000.0);
            // The closed arrow is arrow_size long: its tip to its back.
            assert!(!drawing.arrow_fills.is_empty(), "1:{scale}");
            let head = &drawing.arrow_fills[0];
            let length = head[0].distance_to((head[1] + head[2]) * 0.5);
            near(length, 3.0 * scale / 1000.0, 1e-9);
        }
        // A model-unit style draws alike at every scale.
        let model = DimensionStyle::default();
        double_eq(
            build_dimension(&dimension, &model, 200.0).text_height,
            build_dimension(&dimension, &model, 5000.0).text_height,
        );
    }

    #[test]
    fn the_aligned_drawing_is_what_it_was_before_the_kinds() {
        // The construction pinned by test_dimension_draw: a dimension from
        // (0,0) to (10,0) offset 3 has its line at y = 3 and text above it.
        let d = build_dimension(
            &DimensionGeometry::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), 3.0, ""),
            &DimensionStyle::default(),
            DEFAULT_ANNOTATION_SCALE,
        );
        assert!(d.has_dimension_line);
        assert!(d.curves.is_empty());
        assert_eq!(d.dimension_line.start, Point2::new(0.0, 3.0));
        assert_eq!(d.dimension_line.end, Point2::new(10.0, 3.0));
    }
}

mod leader_draw {
    use super::*;

    #[test]
    fn the_landing_runs_away_from_the_line_and_the_note_sits_beyond_it() {
        let model = Model::default();
        let measure: TextMeasure =
            Arc::new(|line: &str, _face: &TextFace| 0.5 * character_count(line) as f64);
        let context = LeaderContext::default();
        // Tip at the origin, bend at (10, 10): heading right. At 1:1000 the
        // landing is 2.5 m, the note 2.5 m tall and half its height beyond.
        let mut geometry = leader(vec![Point2::new(0.0, 0.0), Point2::new(10.0, 10.0)], "AB");
        let right = build_leader(&model, &geometry, 1000.0, &measure, &context);
        assert!(!right.strokes.is_empty());
        assert_eq!(
            right.strokes[0].last().copied(),
            Some(Point2::new(12.5, 10.0)),
            "the landing's end"
        );
        assert_eq!(right.texts.len(), 1);
        near(right.texts[0].origin.x, 12.5 + 1.25, 1e-12);
        assert_eq!(right.fills.len(), 1, "the closed arrow");
        assert_eq!(
            right.fills[0][0],
            Point2::new(0.0, 0.0),
            "its tip at the leader's tip"
        );

        // Heading left: the landing and the note go left, the note
        // right-justified.
        geometry.vertices = vec![Point2::new(0.0, 0.0), Point2::new(-10.0, 10.0)];
        let left = build_leader(&model, &geometry, 1000.0, &measure, &context);
        assert_eq!(
            left.strokes[0].last().copied(),
            Some(Point2::new(-12.5, 10.0))
        );
        assert!(left.texts[0].origin.x < -12.5);
    }

    #[test]
    fn callouts_frame_their_note() {
        let model = Model::default();
        let context = LeaderContext::default();
        let mut geometry = LeaderGeometry {
            callout: CalloutShape::Circle,
            ..leader(vec![Point2::new(0.0, 0.0), Point2::new(10.0, 10.0)], "12")
        };
        let balloon = build_leader(&model, &geometry, 1000.0, &estimated_measure(), &context);
        assert_eq!(balloon.outlines.len(), 1);
        // The circle clears the note: every text box corner inside it.
        let mut circle = Box2::default();
        for &p in &balloon.outlines[0] {
            circle.expand(p);
        }
        let centre = circle.center();
        let radius = 0.5 * circle.width();
        for corner in &balloon.text_boxes[0] {
            assert!(corner.distance_to(centre) < radius);
        }
        // The landing ends on the circle.
        near(centre.x - radius, 12.5, 0.05);

        geometry.callout = CalloutShape::Box;
        let boxed = build_leader(&model, &geometry, 1000.0, &estimated_measure(), &context);
        assert_eq!(boxed.outlines.len(), 1);
        assert_eq!(boxed.masks.len(), 1, "the frame is filled behind the note");
    }

    #[test]
    fn an_explicit_landing_direction_does_not_guess_the_note_side_from_the_last_leg() {
        let model = Model::default();
        let context = LeaderContext::default();
        let mut geometry = LeaderGeometry {
            paper_height: 2.0,
            landing: 4.0,
            ..leader(vec![Point2::new(0.0, 0.0), Point2::new(10.0, 10.0)], "AB")
        };
        geometry.landing_angle = Some(PI);
        let left = build_leader(&model, &geometry, 500.0, &estimated_measure(), &context);
        assert!(!left.strokes.is_empty());
        let end = left.strokes[0][left.strokes[0].len() - 1];
        near(end.x, 8.0, 1e-12);
        near(end.y, 10.0, 1e-12);
        assert_eq!(left.texts.len(), 1);
        assert!(left.texts[0].origin.x < 8.0);
        geometry.landing_angle = Some(PI / 2.0);
        let up = build_leader(&model, &geometry, 500.0, &estimated_measure(), &context);
        assert!(!up.strokes.is_empty());
        let end = up.strokes[0][up.strokes[0].len() - 1];
        near(end.x, 10.0, 1e-12);
        near(end.y, 12.0, 1e-12);
        assert_eq!(up.texts.len(), 1);
        near(up.texts[0].rotation, PI / 2.0, 1e-12);
    }

    #[test]
    fn arrow_sizes_are_paper_millimetres() {
        let model = Model::default();
        let geometry = LeaderGeometry {
            vertices: vec![Point2::new(0.0, 0.0), Point2::new(100.0, 0.0)],
            arrow_size: 2.0,
            landing: 0.0,
            ..LeaderGeometry::default()
        };
        for scale in [500.0, 2000.0] {
            let d = build_leader(
                &model,
                &geometry,
                scale,
                &estimated_measure(),
                &LeaderContext::default(),
            );
            assert_eq!(d.fills.len(), 1);
            let head = &d.fills[0];
            near(
                head[0].distance_to((head[1] + head[2]) * 0.5),
                2.0 * scale / 1000.0,
                1e-9,
            );
        }
    }
}
