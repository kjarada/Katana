// Linetype dashing (tests/cad/test_dashing.cpp).
//
// The failure this file exists to prevent is the one that LOOKS right:
// dashing in screen space instead of model space. A fixed pixel pattern
// renders a perfectly convincing dashed line, and is exactly backwards - the
// dashes stay the same size as you zoom, so a 10 m fence and a 10 km boundary
// get identical dashes. ZoomDoesNotChangeWhereTheDashesFall and
// ALongerPathGetsProportionallyMoreDashes are the two that catch it.

use crate::near;
use katana_cad::DashOptions;
use katana_cad::dashing::{for_each_dash, qt_dash_pattern, should_dash};
use katana_entity::{Linetype, LinetypeElement};
use katana_geometry::Point2;

// 1 m dash, 0.5 m gap: a 1.5 m period.
fn dashed() -> Linetype {
    Linetype {
        name: String::from("dashed"),
        pattern: vec![
            LinetypeElement { length: 1.0 },
            LinetypeElement { length: -0.5 },
        ],
        ..Linetype::default()
    }
}

// Dash, gap, dot, gap - the DXF "dash dot" shape.
fn dash_dot() -> Linetype {
    Linetype {
        name: String::from("dashdot"),
        pattern: vec![
            LinetypeElement { length: 1.0 },
            LinetypeElement { length: -0.25 },
            LinetypeElement { length: 0.0 },
            LinetypeElement { length: -0.25 },
        ],
        ..Linetype::default()
    }
}

fn continuous() -> Linetype {
    Linetype {
        name: String::from("continuous"),
        ..Linetype::default()
    }
}

#[derive(Clone, Copy, Debug)]
struct Span {
    from: Point2,
    to: Point2,
}

impl Span {
    fn length(&self) -> f64 {
        self.from.distance_to(self.to)
    }
}

// Collects the spans of a horizontal run of `length` metres along the x axis,
// and whether they were laid.
fn dashes_along_x(length: f64, linetype: &Linetype, options: &DashOptions) -> (Vec<Span>, bool) {
    let points = [Point2::new(0.0, 0.0), Point2::new(length, 0.0)];
    let mut spans = Vec::new();
    let laid = for_each_dash(&points, false, linetype, options, |a, b| {
        spans.push(Span { from: a, to: b });
    });
    if !laid {
        spans.clear();
    }
    (spans, laid)
}

fn at_scale(view_scale: f64) -> DashOptions {
    DashOptions {
        view_scale,
        ..DashOptions::default()
    }
}

fn collect(
    points: &[Point2],
    closed: bool,
    linetype: &Linetype,
    options: &DashOptions,
) -> (Vec<Span>, bool) {
    let mut spans = Vec::new();
    let laid = for_each_dash(points, closed, linetype, options, |a, b| {
        spans.push(Span { from: a, to: b });
    });
    (spans, laid)
}

mod dashing {
    use super::*;

    #[test]
    fn a_continuous_linetype_is_never_dashed() {
        assert!(!should_dash(&continuous(), &at_scale(100.0)));
        let (spans, laid) = dashes_along_x(10.0, &continuous(), &at_scale(100.0));
        assert!(!laid, "the caller must be told to draw it solid");
        assert!(spans.is_empty());
    }

    #[test]
    fn dashes_are_laid_out_in_model_units() {
        // A 10 m run of a 1 m dash / 0.5 m gap pattern: dashes at 0-1,
        // 1.5-2.5, 3-4, 4.5-5.5, 6-7, 7.5-8.5, 9-10. Seven of them, every one
        // exactly 1 m.
        let (spans, _) = dashes_along_x(10.0, &dashed(), &at_scale(100.0));
        assert_eq!(spans.len(), 7);
        for (i, span) in spans.iter().enumerate() {
            near(span.from.x, i as f64 * 1.5, 1e-9);
            near(span.length(), 1.0, 1e-9);
        }
    }

    #[test]
    fn zoom_does_not_change_where_the_dashes_fall() {
        // THE test. Model-space dashing gives identical spans at every zoom; a
        // screen-space implementation would give the same NUMBER of dashes at
        // every zoom instead, which is the opposite.
        let (reference, _) = dashes_along_x(20.0, &dashed(), &at_scale(50.0));
        assert!(!reference.is_empty());

        for view_scale in [10.0, 50.0, 200.0, 1000.0] {
            let (spans, _) = dashes_along_x(20.0, &dashed(), &at_scale(view_scale));
            assert_eq!(spans.len(), reference.len(), "viewScale {view_scale}");
            for (span, expected) in spans.iter().zip(&reference) {
                near(span.from.x, expected.from.x, 1e-12);
                near(span.length(), expected.length(), 1e-12);
            }
        }
    }

    #[test]
    fn a_longer_path_gets_proportionally_more_dashes() {
        // The other half of the same property: a 10 km boundary has a
        // thousand times the dashes of a 10 m fence, because a dash is a
        // ground length.
        let (short_run, _) = dashes_along_x(15.0, &dashed(), &at_scale(100.0));
        let (long_run, _) = dashes_along_x(150.0, &dashed(), &at_scale(100.0));

        assert!(!short_run.is_empty());
        near(long_run.len() as f64 / short_run.len() as f64, 10.0, 0.15);
        // And every dash is still the same ground length in both.
        for span in &long_run {
            near(span.length(), 1.0, 1e-9);
        }
    }

    #[test]
    fn the_pattern_runs_continuously_across_vertices() {
        // DXF's $PLINEGEN default: the pattern carries on along the whole
        // polyline rather than restarting at each vertex. A restart would put
        // a dash at every corner and make the spacing depend on where the
        // vertices are.
        let bent = [
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 4.0),
        ];
        let (spans, laid) = collect(&bent, false, &dashed(), &at_scale(100.0));
        assert!(laid);

        // Total pen-down length must be the same fraction of the path length
        // as the dash is of the period (1.0 of 1.5), give or take the final
        // partial.
        let mut pen_down = 0.0;
        for span in &spans {
            pen_down += span.length();
        }
        let path_length = 6.0;
        near(pen_down / path_length, 1.0 / 1.5, 0.12);

        // A span crossing the corner is split into two, so no span may cut
        // the corner: every one must lie on the path.
        for span in &spans {
            let on_first = span.from.y.abs() < 1e-9 && span.to.y.abs() < 1e-9;
            let on_second = (span.from.x - 2.0).abs() < 1e-9 && (span.to.x - 2.0).abs() < 1e-9;
            assert!(on_first || on_second, "a span cut the corner: {span:?}");
        }
    }

    #[test]
    fn a_closed_path_includes_its_closing_segment() {
        let square = [
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 10.0),
            Point2::new(0.0, 10.0),
        ];
        let (open, open_laid) = collect(&square, false, &dashed(), &at_scale(100.0));
        let (closed, closed_laid) = collect(&square, true, &dashed(), &at_scale(100.0));
        assert!(open_laid);
        assert!(closed_laid);
        assert!(
            closed.len() > open.len(),
            "the fourth side must be dashed too"
        );
    }

    #[test]
    fn a_pattern_too_fine_to_resolve_is_drawn_solid_rather_than_as_a_paler_line() {
        // Below about a pixel per element, every dash and gap falls inside one
        // pixel and antialiasing turns the line a uniformly paler colour. The
        // user sees the wrong COLOUR, not a dash pattern, so the honest answer
        // is a solid line.
        assert!(
            should_dash(&dashed(), &at_scale(10.0)), // 0.5 m gap = 5 px
            "zoomed in, the pattern is resolvable"
        );
        assert!(
            !should_dash(&dashed(), &at_scale(1.0)), // 0.5 m gap = 0.5 px
            "zoomed out, it is not"
        );

        let (spans, laid) = dashes_along_x(1000.0, &dashed(), &at_scale(0.5));
        assert!(!laid);
        assert!(spans.is_empty());
    }

    #[test]
    fn an_unreasonable_span_count_falls_back_to_solid_instead_of_truncating() {
        // A truncating implementation silently shortens the line: a 900 m
        // fence drawn as 300 m, with no error anywhere. All-or-nothing
        // instead.
        let mut options = at_scale(1000.0);
        options.maximum_spans = 50;

        let (spans, laid) = dashes_along_x(10000.0, &dashed(), &options);
        assert!(
            !laid,
            "the caller must be told to draw the whole path solid"
        );
        assert!(spans.is_empty(), "nothing may have been emitted");

        // Raising the budget makes the same path dash.
        options.maximum_spans = 100000;
        let (spans, laid) = dashes_along_x(10000.0, &dashed(), &options);
        assert!(spans.len() > 1000);
        assert!(laid);
    }

    #[test]
    fn a_dot_is_emitted_as_a_zero_length_span() {
        let (spans, _) = dashes_along_x(10.0, &dash_dot(), &at_scale(100.0));
        assert!(!spans.is_empty());

        let mut dots = 0;
        let mut dashes = 0;
        for span in &spans {
            if span.length() < 1e-12 {
                dots += 1;
            } else {
                dashes += 1;
                near(span.length(), 1.0, 1e-9);
            }
        }
        assert!(dots > 3, "the dots in the pattern must actually be emitted");
        assert!(dashes > 3);
    }

    #[test]
    fn degenerate_input_produces_nothing_rather_than_spinning() {
        let options = at_scale(100.0);
        let mut spans: Vec<Span> = Vec::new();
        let mut collect = |a: Point2, b: Point2| spans.push(Span { from: a, to: b });

        assert!(!for_each_dash(
            &[],
            false,
            &dashed(),
            &options,
            &mut collect
        ));
        assert!(!for_each_dash(
            &[Point2::new(0.0, 0.0)],
            false,
            &dashed(),
            &options,
            &mut collect
        ));
        // Two coincident points: a path with no length.
        assert!(!for_each_dash(
            &[Point2::new(1.0, 1.0), Point2::new(1.0, 1.0)],
            false,
            &dashed(),
            &options,
            &mut collect
        ));
        // A pattern that is all dots has no length either; it must not loop.
        let dots = Linetype {
            name: String::from("dots"),
            pattern: vec![
                LinetypeElement { length: 0.0 },
                LinetypeElement { length: 0.0 },
            ],
            ..Linetype::default()
        };
        assert!(!for_each_dash(
            &[Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            false,
            &dots,
            &options,
            &mut collect
        ));
        assert!(spans.is_empty());

        // A nonsensical view scale is refused rather than dividing by it.
        assert!(!should_dash(&dashed(), &at_scale(0.0)));
        assert!(!should_dash(&dashed(), &at_scale(-5.0)));
        assert!(!should_dash(&dashed(), &at_scale(f64::NAN)));
    }

    // ---- the painter's conversion -------------------------------------------

    #[test]
    fn qt_dash_units_are_pen_widths_not_pixels() {
        // The pen multiplies the array by its width when it draws, so a 2 px
        // pen doubles every entry. Forgetting the division renders perfectly
        // and makes every pattern as many times too long as the pen is wide -
        // and the viewport draws at 1.5 and 2 px, so 1.5x and 2x wrong.
        let options = at_scale(100.0); // 1 m dash = 100 px

        let at_one_pixel = qt_dash_pattern(&dashed(), &options, 1.0);
        assert_eq!(at_one_pixel.len(), 2);
        near(at_one_pixel[0], 100.0, 1e-9); // 1 m at 100 px/m with a 1 px pen
        near(at_one_pixel[1], 50.0, 1e-9);

        let at_two_pixels = qt_dash_pattern(&dashed(), &options, 2.0);
        assert_eq!(at_two_pixels.len(), 2);
        near(at_two_pixels[0], 50.0, 1e-9); // a 2 px pen must HALVE every entry
        near(at_two_pixels[1], 25.0, 1e-9);

        // A cosmetic pen (width 0) is drawn one pixel wide, so it divides by
        // 1.
        let cosmetic = qt_dash_pattern(&dashed(), &options, 0.0);
        assert_eq!(cosmetic.len(), 2);
        near(cosmetic[0], 100.0, 1e-9);
    }

    #[test]
    fn the_qt_pattern_scales_with_zoom_because_pixels_do() {
        // The mirror of ZoomDoesNotChangeWhereTheDashesFall: the MODEL layout
        // is fixed, so the PIXEL pattern must grow with the view scale. A
        // constant array at every zoom would be the screen-space bug.
        let near_view = qt_dash_pattern(&dashed(), &at_scale(200.0), 1.0);
        let far_view = qt_dash_pattern(&dashed(), &at_scale(100.0), 1.0);
        assert_eq!(near_view.len(), 2);
        assert_eq!(far_view.len(), 2);
        near(near_view[0] / far_view[0], 2.0, 1e-9); // twice the zoom, twice the pixels per dash
    }

    #[test]
    fn the_qt_pattern_is_empty_when_the_line_should_be_solid() {
        assert!(qt_dash_pattern(&continuous(), &at_scale(100.0), 1.0).is_empty());
        assert!(
            qt_dash_pattern(&dashed(), &at_scale(0.5), 1.0).is_empty(),
            "too fine to resolve"
        );
    }

    #[test]
    fn the_qt_pattern_always_has_an_even_length() {
        // The pen alternates on/off from on, so an odd count inverts the
        // meaning of every entry when the array repeats. validate() makes a
        // well-formed pattern even, but a project written elsewhere is not
        // ours to trust.
        let odd = Linetype {
            name: String::from("odd"),
            pattern: vec![
                LinetypeElement { length: 1.0 },
                LinetypeElement { length: -0.5 },
                LinetypeElement { length: 1.0 },
            ],
            ..Linetype::default()
        };
        let qt = qt_dash_pattern(&odd, &at_scale(100.0), 1.0);
        assert!(!qt.is_empty());
        assert!(qt.len().is_multiple_of(2));
        for &value in &qt {
            assert!(value > 0.0, "the pen rejects a zero-length entry");
        }
    }

    #[test]
    fn a_dot_becomes_a_visible_mark_not_a_zero() {
        // A zero-length on-span draws nothing under a flat cap, so a dot has
        // to become the thinnest mark the pen will actually render.
        let qt = qt_dash_pattern(&dash_dot(), &at_scale(100.0), 2.0);
        assert_eq!(qt.len(), 4);
        for &value in &qt {
            assert!(value > 0.0);
        }
        assert!(qt[2] < qt[0], "the dot must be far shorter than the dash");
    }
}
