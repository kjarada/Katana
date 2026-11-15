//! The twin of tests/qt_widgets/drawing/test_feedback_painter.cpp: a tool's preview drawn by role
//! (drawing::feedback_painter) - what a click takes, adds and removes, and the vertices of the
//! polyline in play, each drawn where it belongs and in its own colour. Fonts differ from one
//! platform to the next, so no count of pixels is absolute: each role's ink near its mark is
//! compared with a baseline painted with nothing.

use katana_cad::tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};
use katana_entity::{Geometry, PointGeometry};
use katana_geometry::{Point2, Segment2};
use katana_window::canvas::{
    Canvas, Color, Elide, Image, IntRect, Pen, PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint,
};
use katana_window::drawing::feedback_painter::{
    FeedbackCounts, FeedbackFrame, overlay, paint_band, paint_feedback,
};
use katana_window::theme::palette;

use crate::raster::paint_on;

/// 400 x 300 px at 10 px a unit, model (0,0) at pixel (100,200), y up.
fn to_screen(p: Point2) -> PointF {
    PointF::new(100.0 + 10.0 * p.x, 200.0 - 10.0 * p.y)
}

struct Painted {
    image: Image,
    counts: FeedbackCounts,
}

/// The frame the C++ tests paint through: the segments these tests mark, in the canvas's pen, as
/// the plan painter draws a geometry.
fn frame<'a>() -> FeedbackFrame<'a> {
    FeedbackFrame {
        to_screen: Box::new(to_screen),
        draw_shape: Box::new(|canvas: &mut dyn Canvas, geometry: &Geometry| {
            if let Geometry::Line(segment) = geometry {
                canvas.draw_line(to_screen(segment.start), to_screen(segment.end));
            }
        }),
        visible: RectF::new(0.0, 0.0, 400.0, 300.0),
        cursor: PointF::new(100.0, 60.0),
        tracking_label: false,
        band_height: 0.0,
        beneath_glyphs: None,
    }
}

fn paint_over(feedback: &ToolFeedback, focus: &[Point2]) -> Painted {
    let mut image = Image::new(400, 300);
    image.fill(palette::viewport());
    let mut counts = FeedbackCounts::default();
    paint_on(&mut image, |canvas| {
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        let mut frame = frame();
        counts = paint_feedback(canvas, feedback, focus, &mut frame);
    });
    Painted { image, counts }
}

/// Whether `pixel` is `colour` laid over `ground` at some coverage from 40 % to full, within 24 a
/// channel: the ink of that colour, anti-aliased edges included, and never the ground or a colour
/// of another role.
fn ink_of(pixel: Color, colour: Color, ground: Color) -> bool {
    let channel = |c: Color| [f64::from(c.r), f64::from(c.g), f64::from(c.b)];
    let (p_raw, c_raw, g_raw) = (channel(pixel), channel(colour), channel(ground));
    let d = [
        c_raw[0] - g_raw[0],
        c_raw[1] - g_raw[1],
        c_raw[2] - g_raw[2],
    ];
    let p = [
        p_raw[0] - g_raw[0],
        p_raw[1] - g_raw[1],
        p_raw[2] - g_raw[2],
    ];
    let length = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let t = (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) / length;
    if !(0.4..=1.1).contains(&t) {
        return false;
    }
    (0..3).all(|i| (p[i] - t * d[i]).abs() <= 24.0)
}

fn ink_near(image: &Image, centre: PointF, radius: f64, colour: Color) -> i32 {
    let mut count = 0;
    let ground = palette::viewport();
    for y in (centre.y - radius) as i32..=(centre.y + radius) as i32 {
        for x in (centre.x - radius) as i32..=(centre.x + radius) as i32 {
            if x < 0
                || y < 0
                || x >= image.width() as i32
                || y >= image.height() as i32
                || (f64::from(x) - centre.x).hypot(f64::from(y) - centre.y) > radius
            {
                continue;
            }
            count += i32::from(ink_of(image.pixel(x as u32, y as u32), colour, ground));
        }
    }
    count
}

fn mark(role: FeedbackRole, geometry: Geometry, label: &str) -> FeedbackMark {
    FeedbackMark {
        role,
        geometry,
        label: String::from(label),
        refused: false,
    }
}

fn vertex(x: f64, y: f64) -> Geometry {
    Geometry::Point(PointGeometry {
        position: Point2::new(x, y),
    })
}

fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
    Geometry::Line(Segment2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

fn sample() -> ToolFeedback {
    ToolFeedback {
        marks: vec![
            mark(FeedbackRole::Target, segment(0.0, 0.0, 10.0, 0.0), ""),
            mark(FeedbackRole::Added, vertex(5.0, 0.0), ""),
            mark(FeedbackRole::Removed, vertex(10.0, 0.0), ""),
        ],
        ..ToolFeedback::default()
    }
}

fn focus() -> Vec<Point2> {
    vec![
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(20.0, 0.0),
    ]
}

/// A colour as a red-green colour-blind eye sees it: deuteranopia at severity 1.0 in Machado,
/// Oliveira and Fernandes (2009), "A Physiologically-based Model for Simulation of Color Vision
/// Deficiency", IEEE TVCG 15(6), whose matrix applies to linear RGB (IEC 61966-2-1's sRGB
/// decoding).
fn deuteranope(colour: Color) -> [f64; 3] {
    let linear = |channel: u8| {
        let v = f64::from(channel) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = linear(colour.r);
    let g = linear(colour.g);
    let b = linear(colour.b);
    [
        0.367322 * r + 0.860646 * g - 0.227968 * b,
        0.280085 * r + 0.672501 * g + 0.047413 * b,
        -0.011820 * r + 0.042940 * g + 0.968881 * b,
    ]
}

/// The pixels within `radius` of `centre` that such an eye sees differently in `a` and `b`: some
/// channel more than 0.08 apart in linear light. The target's green (0x3cd070) and the refusal's
/// red (0xff6b6b) come out at (0.523, 0.445, 0.184) and (0.460, 0.386, 0.137) - 0.063 apart at
/// most, worked from the matrix - so two marks that differ only in those two hues count none (the
/// control below).
fn seen_apart(a: &Image, b: &Image, centre: PointF, radius: f64) -> i32 {
    let mut count = 0;
    for y in (centre.y - radius) as i32..=(centre.y + radius) as i32 {
        for x in (centre.x - radius) as i32..=(centre.x + radius) as i32 {
            if (f64::from(x) - centre.x).hypot(f64::from(y) - centre.y) > radius {
                continue;
            }
            let p = deuteranope(a.pixel(x as u32, y as u32));
            let q = deuteranope(b.pixel(x as u32, y as u32));
            if (0..3).any(|i| (p[i] - q[i]).abs() > 0.08) {
                count += 1;
            }
        }
    }
    count
}

/// WCAG 2.1's relative luminance of an sRGB colour, and the contrast ratio of two
/// (https://www.w3.org/TR/WCAG21/#dfn-relative-luminance).
fn luminance(colour: Color) -> f64 {
    let linear = |channel: u8| {
        let v = f64::from(channel) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(colour.r) + 0.7152 * linear(colour.g) + 0.0722 * linear(colour.b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let la = luminance(a);
    let lb = luminance(b);
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn text_ink(image: &Image, columns: std::ops::Range<u32>, rows: std::ops::Range<u32>) -> i32 {
    let mut text = 0;
    for y in rows {
        for x in columns.clone() {
            text += i32::from(ink_of(
                image.pixel(x, y),
                palette::text(),
                palette::viewport(),
            ));
        }
    }
    text
}

mod feedback_painter {
    use super::*;

    #[test]
    fn each_role_is_counted_as_drawn() {
        let painted = paint_over(&sample(), &focus());
        assert_eq!(painted.counts.target, 1);
        assert_eq!(painted.counts.added, 1);
        assert_eq!(painted.counts.removed, 1);
        assert_eq!(painted.counts.focus, 3);
        assert_eq!(painted.counts.shapes, 0);
        assert_eq!(painted.counts.markers, 0);
    }

    #[test]
    fn each_role_is_inked_in_its_colour_where_its_mark_is() {
        let marked = paint_over(&sample(), &focus());
        let baseline = paint_over(&ToolFeedback::default(), &[]);
        // The target segment at (2,0), away from the other marks.
        let on_target = to_screen(Point2::new(2.0, 0.0));
        assert!(
            ink_near(&marked.image, on_target, 8.0, overlay::target())
                > ink_near(&baseline.image, on_target, 8.0, overlay::target())
        );
        let added = to_screen(Point2::new(5.0, 0.0));
        assert!(
            ink_near(&marked.image, added, 8.0, overlay::preview())
                > ink_near(&baseline.image, added, 8.0, overlay::preview())
        );
        let removed = to_screen(Point2::new(10.0, 0.0));
        assert!(
            ink_near(&marked.image, removed, 8.0, overlay::removed())
                > ink_near(&baseline.image, removed, 8.0, overlay::removed())
        );
        // The polyline's vertex with no mark of its own shows the cold square.
        let focused = to_screen(Point2::new(20.0, 0.0));
        assert!(
            ink_near(&marked.image, focused, 8.0, overlay::grip_cold())
                > ink_near(&baseline.image, focused, 8.0, overlay::grip_cold())
        );
        // And a role's colour is not drawn where another role's mark is: no removed red at the
        // added vertex.
        assert_eq!(ink_near(&marked.image, added, 4.0, overlay::removed()), 0);
    }

    #[test]
    fn a_crowded_new_vertex_gives_way_to_the_vertex_that_goes() {
        // A fillet a few pixels across: its new vertex 3 px from the corner's X. Alone the new
        // vertex is drawn; over the X it is left out, so the X still reads - and, left out, it is
        // not counted: the counts are the headless record of what is on screen, and a record of
        // added=1 over a picture with no disc told a test something that was not there.
        let alone = ToolFeedback {
            marks: vec![mark(FeedbackRole::Added, vertex(10.3, 0.0), "")],
            ..ToolFeedback::default()
        };
        let mut crowded = alone.clone();
        crowded
            .marks
            .push(mark(FeedbackRole::Removed, vertex(10.0, 0.0), ""));
        let drawn = paint_over(&alone, &[]);
        let given = paint_over(&crowded, &[]);
        let place = to_screen(Point2::new(10.3, 0.0));
        assert!(ink_near(&drawn.image, place, 6.0, overlay::preview()) > 0);
        assert_eq!(ink_near(&given.image, place, 6.0, overlay::preview()), 0);
        assert!(ink_near(&given.image, place, 6.0, overlay::removed()) > 0);
        assert_eq!(drawn.counts.added, 1);
        assert_eq!(given.counts.added, 0);
    }

    #[test]
    fn enters_place_is_an_outlined_ring_not_the_new_vertexs_filled_disc() {
        // Where Enter would add: the Added disc's circle in the same cyan round a small dot, and
        // between the two the dark ground, where the disc is cyan all through
        // (EntersPlaceIsARingRoundADotWithNoLineThroughIt has the dot).
        let enter = ToolFeedback {
            marks: vec![mark(FeedbackRole::Enter, vertex(5.0, 0.0), "Enter")],
            ..ToolFeedback::default()
        };
        let added = ToolFeedback {
            marks: vec![mark(FeedbackRole::Added, vertex(5.0, 0.0), "")],
            ..ToolFeedback::default()
        };
        let ring = paint_over(&enter, &[]);
        let disc = paint_over(&added, &[]);
        let baseline = paint_over(&ToolFeedback::default(), &[]);
        // The ring, 6 px out: inked. A point 2.5 px out on each axis, 3.5 px from the middle -
        // past the 1.75 px dot, off the disc's "+": the disc's cyan, the ring's ground.
        let centre = to_screen(Point2::new(5.0, 0.0));
        assert!(
            ink_near(&ring.image, centre, 7.5, overlay::preview())
                > ink_near(&baseline.image, centre, 7.5, overlay::preview())
        );
        // ink_near names a pixel by its top-left corner, so the pixels whose centres lie within
        // 1 px of that point are those within 1 px of centre + (2, 2): from 2.24 px out to 5.0
        // px, inside the pen's inner edge at 5 px. Probed at centre + (2.5, 2.5), the pixel
        // [3,4] x [3,4] from the centre lies 45.3 % outside radius 5 (the area
        // 1 - (integral from 3 to 4 of sqrt(25 - x^2) dx - 3)), an inked edge pixel that an
        // exact rasteriser draws and Qt's curve offsets missed.
        let inside = centre + PointF::new(2.0, 2.0);
        assert!(ink_near(&disc.image, inside, 1.0, overlay::preview()) > 0);
        assert_eq!(ink_near(&ring.image, inside, 1.0, overlay::preview()), 0);
        assert_eq!(ring.counts.enter, 1);
        assert_eq!(
            ring.counts.added, 0,
            "Enter's place is not what a click adds"
        );
    }

    #[test]
    fn a_dense_strings_vertices_are_thinned_and_lie_under_the_target() {
        // 201 vertices 0.1 apart - 1 px at 10 px a unit - under a target segment along them. One
        // square a vertex was a solid blue band drawn over the target; now no two squares are
        // nearer than 10 px, the ends kept, and they are drawn first, so the target's green lies
        // on top.
        let dense: Vec<Point2> = (0..=200)
            .map(|i| Point2::new(0.1 * f64::from(i), 0.0))
            .collect();
        let feedback = ToolFeedback {
            marks: vec![mark(FeedbackRole::Target, segment(0.0, 0.0, 20.0, 0.0), "")],
            ..ToolFeedback::default()
        };
        let painted = paint_over(&feedback, &dense);
        // 200 px of string at one square per 10 px: 21 squares, give or take one where
        // 0.1 x 10 px rounds under 10.
        assert!(painted.counts.focus >= 20);
        assert!(painted.counts.focus <= 22);
        // Midway between squares and on them alike, the segment is green.
        for x in [3.05, 7.0, 11.05, 15.0] {
            assert!(
                ink_near(
                    &painted.image,
                    to_screen(Point2::new(x, 0.0)),
                    1.5,
                    overlay::target()
                ) > 0,
                "at x = {x}"
            );
        }
    }

    #[test]
    fn the_snap_marker_lies_beneath_the_new_vertex() {
        // The view's snap marker is drawn by beneath_glyphs: before the vertex glyphs, so a yellow
        // X at the new vertex no longer crosses its disc.
        let feedback = ToolFeedback {
            marks: vec![mark(FeedbackRole::Added, vertex(5.0, 0.0), "")],
            ..ToolFeedback::default()
        };
        let mut image = Image::new(400, 300);
        image.fill(palette::viewport());
        let centre = to_screen(Point2::new(5.0, 0.0));
        paint_on(&mut image, |canvas| {
            canvas.set_render_hint(RenderHint::Antialiasing, true);
            let mut frame = FeedbackFrame {
                draw_shape: Box::new(|_: &mut dyn Canvas, _: &Geometry| {}),
                beneath_glyphs: Some(Box::new(move |canvas: &mut dyn Canvas| {
                    // The Intersection marker: an X 6 px each way, 2 px wide.
                    canvas.set_pen(Pen::with(
                        overlay::snap(),
                        2.0,
                        PenStyle::Solid,
                        PenCap::Square,
                        PenJoin::Bevel,
                    ));
                    canvas.draw_line(
                        centre + PointF::new(-6.0, -6.0),
                        centre + PointF::new(6.0, 6.0),
                    );
                    canvas.draw_line(
                        centre + PointF::new(-6.0, 6.0),
                        centre + PointF::new(6.0, -6.0),
                    );
                })),
                ..frame()
            };
            let _ = paint_feedback(canvas, &feedback, &[], &mut frame);
        });
        // On the disc, off its "+": the disc's cyan, and no yellow over it.
        assert_eq!(
            ink_near(&image, centre + PointF::new(2.5, 2.5), 1.0, overlay::snap()),
            0
        );
        assert!(
            ink_near(
                &image,
                centre + PointF::new(2.5, 2.5),
                1.0,
                overlay::preview()
            ) > 0
        );
        // The marker's tips, beyond the disc, still show.
        assert!(ink_near(&image, centre + PointF::new(5.5, 5.5), 1.0, overlay::snap()) > 0);
    }

    #[test]
    fn a_refused_caption_is_in_the_refusals_colour() {
        let refused = ToolFeedback {
            caption: String::from("too close to vertex 0"),
            refused: true,
            ..ToolFeedback::default()
        };
        let taken = ToolFeedback {
            caption: String::from("too close to vertex 0"),
            ..ToolFeedback::default()
        };
        let red = paint_over(&refused, &[]);
        let plain = paint_over(&taken, &[]);
        // The caption's chip starts 16 px right of and below the cursor at (100,60): its red
        // stripe and the first words are round (140,84).
        let chip = PointF::new(140.0, 84.0);
        assert!(
            ink_near(&red.image, chip, 40.0, overlay::removed())
                > ink_near(&plain.image, chip, 40.0, overlay::removed())
        );
    }

    #[test]
    fn a_refused_picks_target_is_drawn_in_the_refusals_red_not_the_targets_green() {
        // Fillet over an end vertex: the marks still say what the pick took, but green there
        // would promise the click the caption says is turned down.
        let taken = ToolFeedback {
            marks: vec![
                mark(FeedbackRole::Target, vertex(5.0, 0.0), "0"),
                mark(FeedbackRole::Target, segment(10.0, 5.0, 20.0, 5.0), ""),
            ],
            ..ToolFeedback::default()
        };
        let refused = ToolFeedback {
            refused: true,
            ..taken.clone()
        };
        let green = paint_over(&taken, &[]);
        let red = paint_over(&refused, &[]);
        let baseline = paint_over(&ToolFeedback::default(), &[]);
        let at_vertex = to_screen(Point2::new(5.0, 0.0));
        let at_piece = to_screen(Point2::new(15.0, 5.0));
        for at in [at_vertex, at_piece] {
            assert!(
                ink_near(&green.image, at, 9.0, overlay::target())
                    > ink_near(&baseline.image, at, 9.0, overlay::target())
            );
            assert_eq!(ink_near(&green.image, at, 9.0, overlay::removed()), 0);
            assert!(
                ink_near(&red.image, at, 9.0, overlay::removed())
                    > ink_near(&baseline.image, at, 9.0, overlay::removed())
            );
            assert_eq!(ink_near(&red.image, at, 9.0, overlay::target()), 0);
        }
        // Still the target in the counts: the record says what the pick took.
        assert_eq!(red.counts.target, 2);
    }

    #[test]
    fn a_refused_mark_differs_in_shape_not_only_in_hue_to_a_red_green_colour_blind_eye() {
        // Fillet on an end vertex, Segment to Line on a straight segment: the refused pick was the
        // accepted one's ring and line in red, the same khaki to a deuteranope as the green. Now
        // the refusal's own shape - the ring struck through, on a vertex and at a piece's middle -
        // tells them apart without the colour.
        let taken = ToolFeedback {
            marks: vec![
                mark(FeedbackRole::Target, vertex(5.0, 0.0), ""),
                mark(FeedbackRole::Target, segment(10.0, 5.0, 20.0, 5.0), ""),
            ],
            ..ToolFeedback::default()
        };
        let refused = ToolFeedback {
            refused: true,
            ..taken.clone()
        };
        let accepted = paint_over(&taken, &[]);
        let turned_down = paint_over(&refused, &[]);
        assert!(
            seen_apart(
                &accepted.image,
                &turned_down.image,
                to_screen(Point2::new(5.0, 0.0)),
                9.0
            ) > 10
        );
        assert!(
            seen_apart(
                &accepted.image,
                &turned_down.image,
                to_screen(Point2::new(15.0, 5.0)),
                9.0
            ) > 10
        );

        // The control: one ring and line drawn in each colour, the same shape, are not told apart
        // by this measure.
        let drawn = |colour: Color| {
            let mut image = Image::new(400, 300);
            image.fill(palette::viewport());
            paint_on(&mut image, |canvas| {
                canvas.set_render_hint(RenderHint::Antialiasing, true);
                let ring = to_screen(Point2::new(5.0, 0.0));
                canvas.set_pen(Pen::with(
                    colour,
                    2.0,
                    PenStyle::Solid,
                    PenCap::Square,
                    PenJoin::Bevel,
                ));
                canvas.draw_ellipse(RectF::new(ring.x - 7.0, ring.y - 7.0, 14.0, 14.0));
                canvas.set_pen(Pen::with(
                    colour,
                    3.0,
                    PenStyle::Solid,
                    PenCap::Square,
                    PenJoin::Bevel,
                ));
                canvas.draw_line(
                    to_screen(Point2::new(10.0, 5.0)),
                    to_screen(Point2::new(20.0, 5.0)),
                );
            });
            image
        };
        let green = drawn(overlay::target());
        let red = drawn(overlay::removed());
        assert_eq!(
            seen_apart(&green, &red, to_screen(Point2::new(5.0, 0.0)), 9.0),
            0
        );
        assert_eq!(
            seen_apart(&green, &red, to_screen(Point2::new(15.0, 5.0)), 9.0),
            0
        );
    }

    #[test]
    fn only_the_mark_flagged_as_the_reason_is_drawn_refused() {
        // Insert beside a chosen vertex, too near another: the vertex it is too near is the
        // refusal's, struck in red; the chosen vertex stays the target green it was - the whole
        // preview in red said the choice itself was refused.
        let mut reason = mark(FeedbackRole::Target, vertex(15.0, 0.0), "0");
        reason.refused = true;
        let feedback = ToolFeedback {
            refused: true,
            marks: vec![mark(FeedbackRole::Target, vertex(5.0, 0.0), "2"), reason],
            ..ToolFeedback::default()
        };
        let painted = paint_over(&feedback, &[]);
        let baseline = paint_over(&ToolFeedback::default(), &[]);
        let chosen = to_screen(Point2::new(5.0, 0.0));
        let near = to_screen(Point2::new(15.0, 0.0));
        assert!(
            ink_near(&painted.image, chosen, 8.0, overlay::target())
                > ink_near(&baseline.image, chosen, 8.0, overlay::target())
        );
        assert_eq!(ink_near(&painted.image, chosen, 8.0, overlay::removed()), 0);
        assert!(
            ink_near(&painted.image, near, 8.0, overlay::removed())
                > ink_near(&baseline.image, near, 8.0, overlay::removed())
        );
        assert_eq!(ink_near(&painted.image, near, 8.0, overlay::target()), 0);
    }

    #[test]
    fn enters_place_is_a_ring_round_a_dot_with_no_line_through_it() {
        // Insert beside a chosen vertex draws Enter's place on the segment it splits. Hollow, the
        // ring had the green line across its middle - a circled minus beside the new vertex's
        // circled plus, "remove" and "add" for two places that both add. Its middle is now the
        // dark ground with a dot, and the line stops at the ring.
        let feedback = ToolFeedback {
            marks: vec![
                mark(FeedbackRole::Target, segment(0.0, 0.0, 10.0, 0.0), ""),
                mark(FeedbackRole::Enter, vertex(5.0, 0.0), ""),
            ],
            ..ToolFeedback::default()
        };
        let painted = paint_over(&feedback, &[]);
        let centre = to_screen(Point2::new(5.0, 0.0));
        // On the line, inside the ring's 6 px: no green.
        assert_eq!(
            ink_near(
                &painted.image,
                centre + PointF::new(3.5, 0.0),
                1.0,
                overlay::target()
            ),
            0
        );
        assert_eq!(
            ink_near(
                &painted.image,
                centre - PointF::new(3.5, 0.0),
                1.0,
                overlay::target()
            ),
            0
        );
        // The dot at its middle, in the preview cyan.
        assert!(ink_near(&painted.image, centre, 1.0, overlay::preview()) > 0);
        // Beyond the ring the line goes on.
        assert!(
            ink_near(
                &painted.image,
                centre + PointF::new(12.0, 0.0),
                1.0,
                overlay::target()
            ) > 0
        );
    }

    #[test]
    fn the_caption_keeps_off_the_marks() {
        // The caption's chip went 16 px right of and below the cursor whatever was there; beside
        // a chosen vertex it hid Enter's place. With a mark where it would go - the cursor at
        // (100,60), Enter's place at pixel (150,86), model (5,11.4) - it goes to another corner of
        // the cursor, and every bit of the mark's ink is still there.
        let feedback = ToolFeedback {
            marks: vec![mark(FeedbackRole::Enter, vertex(5.0, 11.4), "")],
            caption: String::from("vertex 2 \u{b7} 5.000"),
            ..ToolFeedback::default()
        };
        let bare = ToolFeedback {
            caption: String::new(),
            ..feedback.clone()
        };
        let with = paint_over(&feedback, &[]);
        let without = paint_over(&bare, &[]);
        let place = to_screen(Point2::new(5.0, 11.4));
        assert!(ink_near(&without.image, place, 7.5, overlay::preview()) > 0);
        assert_eq!(
            ink_near(&with.image, place, 7.5, overlay::preview()),
            ink_near(&without.image, place, 7.5, overlay::preview())
        );
        // The caption is drawn all the same: its text's ink, above the cursor.
        assert!(text_ink(&with.image, 100..400, 0..60) > 0);
    }

    #[test]
    fn the_band_cuts_where_it_is_told_and_says_what_it_drew() {
        // A prompt too long for the view: cut in the middle it keeps the tool's name and its end;
        // at the left, as for what is being typed, it keeps the end alone.
        let mut image = Image::new(300, 100);
        image.fill(palette::viewport());
        let prompt = "Insert Vertex: Press Enter for a vertex at the marked middle of segment 2, or \
                      click beside vertex 2 of polyline 2 where the new vertex goes, or anywhere \
                      else nearby";
        let mut middle = String::new();
        let mut left = String::new();
        paint_on(&mut image, |canvas| {
            middle = paint_band(canvas, IntRect::new(0, 0, 300, 100), prompt, Elide::Middle);
            left = paint_band(canvas, IntRect::new(0, 0, 300, 100), prompt, Elide::Left);
        });
        assert!(middle.starts_with("Insert Vertex:"), "{middle}");
        assert!(middle.ends_with("nearby"), "{middle}");
        assert!(middle.chars().count() < prompt.chars().count());
        assert!(!left.starts_with("Insert Vertex:"), "{left}");
        assert!(left.ends_with("nearby"));
    }

    #[test]
    fn a_run_of_vertices_that_go_is_thinned_and_never_crosses_out_a_kept_vertex() {
        // Straighten on a dense string: "keep" rings at (0,0) and (6,0), pixels (100,200) and
        // (160,200), and the 29 vertices between, 0.2 apart - 2 px - going. An X a vertex was one
        // red rope, and the X beside each end lay across its "keep" ring: a circled X, crossing
        // out a vertex that stays.
        let mut marks = vec![
            mark(FeedbackRole::Target, vertex(0.0, 0.0), "keep"),
            mark(FeedbackRole::Target, vertex(6.0, 0.0), "keep"),
        ];
        for i in 1..30 {
            marks.push(mark(
                FeedbackRole::Removed,
                vertex(0.2 * f64::from(i), 0.0),
                "",
            ));
        }
        let feedback = ToolFeedback {
            marks,
            ..ToolFeedback::default()
        };
        let painted = paint_over(&feedback, &[]);
        // No red within either ring's outer edge, 8 px.
        for kept in [Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)] {
            assert_eq!(
                ink_near(&painted.image, to_screen(kept), 8.0, overlay::removed()),
                0,
                "at {}",
                kept.x
            );
        }
        // The X's clear of the rings by an X's reach (8 + 7.6 px) lie from pixel 116 to 144,
        // 29 px; at one per 10 px, three - four at most, where the floating point puts two a hair
        // under 10 px apart and one is skipped.
        assert!(painted.counts.removed >= 2);
        assert!(painted.counts.removed <= 4);
        assert!(
            ink_near(
                &painted.image,
                to_screen(Point2::new(3.0, 0.0)),
                16.0,
                overlay::removed()
            ) > 0,
            "some of the run is still marked"
        );
    }

    #[test]
    fn a_chosen_vertex_is_drawn_over_enters_place_beside_it() {
        // Insert beside a chosen vertex on a short segment: Enter's place, the segment's middle,
        // 5 px from the chosen vertex. Enter's opaque ring was drawn after the vertex and covered
        // its ring and all of its filled square - the vertex the owner asked to see.
        let chosen = ToolFeedback {
            marks: vec![mark(FeedbackRole::Target, vertex(0.0, 0.0), "5")],
            ..ToolFeedback::default()
        };
        let mut both = chosen.clone();
        both.marks
            .push(mark(FeedbackRole::Enter, vertex(0.5, 0.0), "Enter"));
        let alone = paint_over(&chosen, &[]);
        let beside = paint_over(&both, &[]);
        let square = to_screen(Point2::new(0.0, 0.0));
        assert!(ink_near(&alone.image, square, 2.5, overlay::target()) > 0);
        assert_eq!(
            ink_near(&beside.image, square, 2.5, overlay::target()),
            ink_near(&alone.image, square, 2.5, overlay::target())
        );
        // Enter's place is still there: its ring's far side, 11 px from the vertex, past the
        // vertex's ring.
        assert!(
            ink_near(
                &beside.image,
                to_screen(Point2::new(0.5, 0.0)) + PointF::new(6.0, 0.0),
                1.5,
                overlay::preview()
            ) > 0
        );
    }

    #[test]
    fn a_refused_vertexs_number_reads_at_three_to_one_against_its_chip() {
        // A refused pick names its vertex in red beside the struck ring. At the regular weight
        // the "1"'s one-pixel stem fell across two columns at about half ink each - wherever the
        // label began - and its most inked pixel read at 2.9:1 against the chip (#9b4849 on
        // #14191d), under the 3:1 of WCAG 2.1's 1.4.11 that the overlay's colours are chosen to,
        // where the red itself is 6.4:1. Labels are DemiBold now: 4.9:1 on the C++'s machine's
        // font. The vertex at (5,0), pixel (150,200): the label's baseline starts at (159,191), its
        // chip one pixel left of that, the text one right.
        let mut reason = mark(FeedbackRole::Target, vertex(5.0, 0.0), "1");
        reason.refused = true;
        let feedback = ToolFeedback {
            marks: vec![reason],
            refused: true,
            ..ToolFeedback::default()
        };
        let painted = paint_over(&feedback, &[]);
        // The chip's own ground: its column left of the text.
        let chip = painted.image.pixel(159, 186);
        // The label's most inked pixel: the most red over green (the red 0xff6b6b is 148 more red
        // than green, the chip next to none), right of the struck ring's outer edge at 158.5.
        let redness = |c: Color| i32::from(c.r) - i32::from(c.g);
        let mut inked = chip;
        for y in 176..=196 {
            for x in 159..=180 {
                let pixel = painted.image.pixel(x, y);
                if redness(pixel) > redness(inked) {
                    inked = pixel;
                }
            }
        }
        assert!(redness(inked) > 40, "the label is there to measure");
        assert!(
            contrast(inked, chip) >= 3.0,
            "the stem at {} on {}",
            inked.name(),
            chip.name()
        );
    }

    #[test]
    fn the_caption_keeps_off_a_piece_along_its_length_not_only_its_middle() {
        // A target segment passing below and right of the cursor, its middle far off: pixel x 130
        // from the top of the view to the bottom, the middle at (130,150). Only a piece's middle
        // was kept off, so the caption went below and right of the cursor at (100,60) - from pixel
        // 116 rightwards, 76 to 96 down - over the very line the preview marks. A short caption,
        // so that one corner, below and left, is clear of the line. And the same line running a
        // million units past the view each way, at 10 px a unit: it is kept off where it is seen,
        // sampled there alone.
        for reach in [20.0, 1.0e6] {
            let feedback = ToolFeedback {
                marks: vec![mark(
                    FeedbackRole::Target,
                    segment(3.0, reach, 3.0, -reach),
                    "",
                )],
                caption: String::from("vertex 2"),
                ..ToolFeedback::default()
            };
            let bare = ToolFeedback {
                caption: String::new(),
                ..feedback.clone()
            };
            let with = paint_over(&feedback, &[]);
            let without = paint_over(&bare, &[]);
            let mut line_with = 0;
            let mut line_without = 0;
            for y in 70..=110 {
                let at = PointF::new(130.0, f64::from(y));
                line_with += i32::from(ink_near(&with.image, at, 1.5, overlay::target()) > 0);
                line_without += i32::from(ink_near(&without.image, at, 1.5, overlay::target()) > 0);
            }
            assert!(line_without > 30, "{reach}");
            assert_eq!(
                line_with, line_without,
                "{reach}: no part of the line under the caption"
            );
            // The caption is drawn all the same.
            assert!(text_ink(&with.image, 0..400, 0..300) > 0, "{reach}");
        }
    }
}
