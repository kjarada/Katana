//! The one place a tool's preview (cad's ToolFeedback, tool_feedback.rs) is drawn over a plan
//! view, and the one home of the view's overlay colours: the port of drawing/feedback_painter.
//!
//! The owner asked on 2026-09-30 for a "visual clue where the vertex is going": a preview says by
//! ROLE what a click takes (Target), makes (Added), takes away (Removed) and what Enter acts on
//! (Enter), and each role has its own shape as well as its own colour, so it reads without the
//! colour - and so does a refusal, whose marks are a ring struck through, not only red
//! (FeedbackMark::refused).
//!
//! The colours are the furniture's - what a view draws over the drawing - not the chrome's, so
//! they are here and not in the theme, whose tokens are the window's (the views keep their drawing
//! colours). Each is at least 3:1 against the view's ground #1e2329, the WCAG 2.1 (1.4.11) floor
//! for graphics that carry meaning: the target green 7.9, the preview cyan 8.2, the removed red
//! 5.7, the cold grip blue 4.3.
//!
//! Nothing here is in the kept drawing image: the view paints this over it every frame, so a
//! mouse move costs the marks and never the drawing.

use std::f64::consts::SQRT_2;

use katana_cad::tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};
use katana_entity::Geometry;
use katana_geometry::{Arc2, CurveSegment, Point2, Segment2};

use crate::canvas::{
    Brush, Canvas, Color, Elide, Font, FontMetrics, FontSize, FontWeight, HAlign, IntRect, Pen,
    PenCap, PenJoin, PenStyle, PointF, RectF, SizeF, TextFlags, VAlign,
};
use crate::theme::palette;

/// The overlay's colours.
pub mod overlay {
    use crate::canvas::Color;
    use crate::theme::palette;

    /// A tool's ghost and base points, and the prompt's text: the preview cyan the plan view has
    /// drawn rubber bands in from the start.
    pub const fn preview() -> Color {
        Color::rgb(0x4c, 0xc9, 0xf0)
    }

    /// What a click here takes: the grips' hover green, which already meant that.
    pub const fn target() -> Color {
        Color::rgb(0x3c, 0xd0, 0x70)
    }

    /// What the step takes away: the chrome's error red (theme's error).
    pub const fn removed() -> Color {
        palette::error()
    }

    /// The grips (grip_controller): blue cold, green under the cursor, red hot, the colours a
    /// drafter already reads.
    pub const fn grip_cold() -> Color {
        Color::rgb(0x3f, 0x7f, 0xff)
    }

    pub const fn grip_hover() -> Color {
        target()
    }

    pub const fn grip_hot() -> Color {
        Color::rgb(0xf0, 0x40, 0x40)
    }

    /// The backing of the prompt band, a caption or a label: darker than the view, so text over a
    /// busy drawing stays legible.
    pub const fn ground() -> Color {
        Color::rgb(0x12, 0x16, 0x1a)
    }

    /// Laid under a removed piece before its red dashes: the view's own ground, most of the way
    /// opaque, so the dashes read over the orange selection.
    pub const fn dim() -> Color {
        palette::viewport().with_alpha(175)
    }

    /// A selection box: a window (left to right) in the preview cyan, a crossing (right to left)
    /// in green; both see-through.
    pub const fn window_box() -> Color {
        preview().with_alpha(40)
    }

    pub const fn crossing_box() -> Color {
        Color::rgba(0x7b, 0xd3, 0x89, 40)
    }

    /// The object snap's marker, its name and the tracking paths: the yellow the plan view has
    /// always drawn them in (it was a constant of the view's).
    pub const fn snap() -> Color {
        Color::rgb(0xf7, 0xd0, 0x3c)
    }
}

// ---- QColor's lighter and darker ----------------------------------------------------------------
//
// QColor::lighter and darker, which the overlay and the surface labels call. They go through Qt's
// HSV of 16-bit channels in single precision, so the digits are Qt's: the canvas's Color has no
// HSV of its own (reported; the integrator may move these beside it).

/// qRound for a float: half up for a positive value.
fn q_round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        ((value - ((value - 1.0) as i32) as f32 + 0.5) as i32) + (value - 1.0) as i32
    }
}

fn q_fuzzy_compare(p1: f32, p2: f32) -> bool {
    (p1 - p2).abs() * 100_000.0 <= p1.abs().min(p2.abs())
}

const USHRT_MAX: f32 = 65535.0;

/// Qt's HSV of a colour: (hue in hundredths of a degree, USHRT_MAX when undefined, saturation,
/// value), each 16-bit.
fn to_hsv(colour: Color) -> (i32, i32, i32) {
    let r = f32::from(u16::from(colour.r) * 0x101) / USHRT_MAX;
    let g = f32::from(u16::from(colour.g) * 0x101) / USHRT_MAX;
    let b = f32::from(u16::from(colour.b) * 0x101) / USHRT_MAX;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let value = q_round(max * USHRT_MAX);
    if delta.abs() <= 0.000_01 {
        // Achromatic: the hue is undefined.
        return (65535, 0, value);
    }
    let saturation = q_round((delta / max) * USHRT_MAX);
    let mut hue = if q_fuzzy_compare(r, max) {
        (g - b) / delta
    } else if q_fuzzy_compare(g, max) {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    hue *= 60.0;
    if hue < 0.0 {
        hue += 360.0;
    }
    (q_round(hue * 100.0), saturation, value)
}

/// qt_div_257: a 16-bit channel to 8 bits.
fn div_257(channel: i32) -> u8 {
    ((channel - (channel >> 8) + 0x80) >> 8).clamp(0, 255) as u8
}

/// Qt's RGB of an HSV colour, back to 8 bits a channel, `alpha` kept.
fn from_hsv(hue: i32, saturation: i32, value: i32, alpha: u8) -> Color {
    if saturation == 0 || hue == 65535 {
        let grey = div_257(value);
        return Color::rgba(grey, grey, grey, alpha);
    }
    let h = if hue == 36000 {
        0.0
    } else {
        hue as f32 / 6000.0
    };
    let s = saturation as f32 / USHRT_MAX;
    let v = value as f32 / USHRT_MAX;
    let i = h as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let (red, green, blue) = if i & 1 == 1 {
        let q = v * (1.0 - (s * f));
        match i {
            1 => (q, v, p),
            3 => (p, q, v),
            _ => (v, p, q),
        }
    } else {
        let t = v * (1.0 - (s * (1.0 - f)));
        match i {
            0 => (v, t, p),
            2 => (p, v, t),
            _ => (t, p, v),
        }
    };
    let channel = |x: f32| div_257(q_round(x * USHRT_MAX));
    Color::rgba(channel(red), channel(green), channel(blue), alpha)
}

/// QColor::lighter(factor): the value scaled by factor / 100, the saturation given up where the
/// value would pass its top.
pub fn lighter(colour: Color, factor: i32) -> Color {
    if factor <= 0 {
        return colour;
    }
    if factor < 100 {
        return darker(colour, 10000 / factor);
    }
    let (hue, mut saturation, value) = to_hsv(colour);
    let mut value = (i64::from(factor) * i64::from(value) / 100) as i32;
    if value > 65535 {
        saturation -= value - 65535;
        if saturation < 0 {
            saturation = 0;
        }
        value = 65535;
    }
    from_hsv(hue, saturation, value, colour.a)
}

/// QColor::darker(factor): the value scaled by 100 / factor.
pub fn darker(colour: Color, factor: i32) -> Color {
    if factor <= 0 {
        return colour;
    }
    if factor < 100 {
        return lighter(colour, 10000 / factor);
    }
    let (hue, saturation, value) = to_hsv(colour);
    let value = (i64::from(value) * 100 / i64::from(factor)) as i32;
    from_hsv(hue, saturation, value, colour.a)
}

// ---- what the painter draws -----------------------------------------------------------------------

/// The overlay's font at `pixels` (theme.cpp's overlayFont): the chrome's face, `pixels` high at
/// the Standard text size. The C++ scales it with the chosen View > Text Size; that choice is
/// held by C's theme, written beside this wave, so it is read here as Standard until the
/// integration joins them (one function).
pub fn overlay_font(pixels: i32) -> Font {
    Font::sized(FontSize::Pixels(pixels.max(1)))
}

/// How many of each the last paint_feedback drew, for the view's last preview counts and the
/// headless pointer record: what is on screen, so an Added vertex left out beside a Removed one (a
/// fillet a few pixels across), a Removed X thinned out or kept off a ring, and a focus square
/// thinned out are not counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeedbackCounts {
    pub shapes: usize,
    pub markers: usize,
    pub target: usize,
    pub added: usize,
    pub removed: usize,
    pub enter: usize,
    pub focus: usize,
}

/// Model to the view's pixels.
pub type ToScreen<'a> = Box<dyn Fn(Point2) -> PointF + 'a>;
/// A geometry drawn in the canvas's current pen, as the plan painter draws one.
pub type DrawShape<'a> = Box<dyn FnMut(&mut dyn Canvas, &Geometry) + 'a>;
/// What the view draws beneath the vertex glyphs.
pub type BeneathGlyphs<'a> = Box<dyn FnMut(&mut dyn Canvas) + 'a>;

/// Where a preview is drawn: the view's transform, the plan painter for a geometry in the
/// canvas's current pen (paint_plan_geometry), the view's rectangle, and the cursor the caption
/// sits beside. The closures are handed the canvas: in the C++ they captured the painter.
pub struct FeedbackFrame<'a> {
    pub to_screen: ToScreen<'a>,
    pub draw_shape: DrawShape<'a>,
    pub visible: RectF,
    pub cursor: PointF,
    /// A drafting aid's label is drawn below the cursor (the view's tracking label): the caption
    /// goes below it.
    pub tracking_label: bool,
    /// The prompt band along the bottom, which the caption keeps off.
    pub band_height: f64,
    /// Drawn after the pieces and before the vertex glyphs: the view's snap marker. It sits where
    /// the new vertex does - a segment's Midpoint, a crossing - and drawn over the disc, the
    /// triangle hid its "+" and the Intersection's X read as the Removed X.
    pub beneath_glyphs: Option<BeneathGlyphs<'a>>,
}

// Sizes in pixels, whatever the zoom: a mark is furniture, not drawing.
const RING_RADIUS: f64 = 7.0;
const TARGET_SQUARE: f64 = 6.0;
const ADDED_RADIUS: f64 = 6.0;
const PLUS_ARM: f64 = 3.0;
// The Removed X and its dark disc, smaller than a ring's 7 px, so an X never reads as a ring's;
// and no X is drawn over a Target vertex's ring at all (below): an X inside a ring is a circled
// X, "cancel", and beside the refusal's struck ring it said "no" of a pick that was taken.
const REMOVED_HALF: f64 = 4.5;
const REMOVED_BACKING: f64 = 5.5;
// How far an X's ink reaches from its middle: an arm's end on the diagonal, its round cap half
// the 2.5 px pen beyond.
const REMOVED_REACH: f64 = REMOVED_HALF * SQRT_2 + 1.25;
// The refusal's stroke runs corner to corner inside its ring, the ring's radius times cos 45
// degrees from the middle each way: the struck ring of "not here".
const STRUCK_HALF: f64 = RING_RADIUS * SQRT_2 / 2.0;
// What Enter acts on: the Added disc's circle round a dot, its middle the dark ground. Hollow,
// the segment through it read as a circled minus beside the new vertex's circled plus - "add"
// and "remove" for two places that add.
const ENTER_DOT: f64 = 1.75;
const FOCUS_SQUARE: f64 = 6.0;
// Focus squares - and the X's of a run of vertices that go - nearer each other than this are
// thinned: the square and its 1.5 px pen are 7.5 px across, the X's disc 11, so 10 px leaves a
// gap a squint still sees, where a dense string's squares were a solid band and its X's one red
// rope.
const FOCUS_SPACING: f64 = 10.0;
// Where a piece is sampled for what the caption keeps off: every few pixels along it, each
// sample the 3 px line and a pixel either side.
const PIECE_SAMPLE_SPACING: f64 = 6.0;
const PIECE_SAMPLE_HALF: f64 = 2.5;
const MARKER_HALF: f64 = 3.5;
// The caption's corner is this far right of and below the cursor, clear of the crosshair and the
// snap marker's name; 16 more below a tracking label (drawn at +12, +18 by the view).
const CAPTION_OFFSET: f64 = 16.0;
const CAPTION_BELOW_TRACKING: f64 = 32.0;
// Where a vertex mark's label and a piece's label sit from their point.
const VERTEX_LABEL_OFFSET: f64 = 9.0;
const PIECE_LABEL_OFFSET: f64 = 6.0;

fn is_vertex(mark: &FeedbackMark) -> bool {
    matches!(mark.geometry, Geometry::Point(_))
}

fn vertex_of(mark: &FeedbackMark, frame: &FeedbackFrame) -> PointF {
    match &mark.geometry {
        Geometry::Point(point) => (frame.to_screen)(point.position),
        _ => PointF::default(),
    }
}

fn hypot(a: PointF, b: PointF) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// QRectF::intersects: an overlap of positive area; a rectangle of no width or height meets
/// nothing.
fn rects_intersect(a: &RectF, b: &RectF) -> bool {
    let (mut l1, mut r1) = (a.x, a.x);
    if a.width < 0.0 {
        l1 += a.width;
    } else {
        r1 += a.width;
    }
    if l1 == r1 {
        return false;
    }
    let (mut l2, mut r2) = (b.x, b.x);
    if b.width < 0.0 {
        l2 += b.width;
    } else {
        r2 += b.width;
    }
    if l2 == r2 || l1 >= r2 || l2 >= r1 {
        return false;
    }
    let (mut t1, mut b1) = (a.y, a.y);
    if a.height < 0.0 {
        t1 += a.height;
    } else {
        b1 += a.height;
    }
    if t1 == b1 {
        return false;
    }
    let (mut t2, mut b2) = (b.y, b.y);
    if b.height < 0.0 {
        t2 += b.height;
    } else {
        b2 += b.height;
    }
    if t2 == b2 || t1 >= b2 || t2 >= b1 {
        return false;
    }
    true
}

fn ellipse_at(centre: PointF, rx: f64, ry: f64) -> RectF {
    RectF::new(centre.x - rx, centre.y - ry, 2.0 * rx, 2.0 * ry)
}

/// QPen(colour, width): solid, square caps, bevelled joins.
fn pen(colour: Color, width: f64) -> Pen {
    Pen::with(
        colour,
        width,
        PenStyle::Solid,
        PenCap::Square,
        PenJoin::Bevel,
    )
}

/// Where a piece's label and its refusal's glyph go: its middle, from the plan's own geometry - a
/// whole polyline's, the middle of its middle segment.
fn middle_of(geometry: &Geometry, frame: &FeedbackFrame) -> Option<PointF> {
    match geometry {
        Geometry::Line(segment) => Some((frame.to_screen)(segment.midpoint())),
        Geometry::Arc(arc) => Some((frame.to_screen)(arc.midpoint())),
        Geometry::Polyline(polyline) if polyline.segment_count() > 0 => Some((frame.to_screen)(
            polyline.segment(polyline.segment_count() / 2).midpoint(),
        )),
        Geometry::CurvePolyline(curve) if curve.segment_count() > 0 => Some((frame.to_screen)(
            curve.segment(curve.segment_count() / 2).point_at(0.5),
        )),
        _ => None,
    }
}

/// Whether `mark` is drawn as the refusal: flagged by the tool, or - in a refused preview that
/// flags none - a Target, the pick that is turned down (ToolFeedback::refused).
fn drawn_refused(mark: &FeedbackMark, feedback: &ToolFeedback, any_flagged: bool) -> bool {
    mark.refused || (feedback.refused && !any_flagged && mark.role == FeedbackRole::Target)
}

/// The refusal's glyph: a ring struck through, the sign for "not here", on a dark disc so it
/// reads over any line. Its SHAPE says refused, not only its red: a Target's ring round a filled
/// square, drawn red, was the same khaki ring as the accepted green one to a red-green
/// colour-blind eye (Machado, Oliveira and Fernandes 2009: the two a CIE76 difference of about 5).
fn paint_struck(canvas: &mut dyn Canvas, p: PointF) {
    canvas.set_pen(Pen::none());
    canvas.set_brush(Brush::solid(overlay::ground().with_alpha(190)));
    canvas.draw_ellipse(ellipse_at(p, RING_RADIUS, RING_RADIUS));
    canvas.set_brush(Brush::none());
    canvas.set_pen(pen(overlay::removed(), 2.0));
    canvas.draw_ellipse(ellipse_at(p, RING_RADIUS, RING_RADIUS));
    canvas.set_pen(Pen::with(
        overlay::removed(),
        2.0,
        PenStyle::Solid,
        PenCap::Flat,
        PenJoin::Bevel,
    ));
    canvas.draw_line(
        p + PointF::new(-STRUCK_HALF, -STRUCK_HALF),
        p + PointF::new(STRUCK_HALF, STRUCK_HALF),
    );
}

/// A mark's label: the overlay's 10 px, DemiBold. At the regular weight a digit's one-pixel stem
/// fell across two columns at about half ink each, wherever the label began (its origin on a
/// whole pixel changed nothing), and a refused vertex's red "1" read at 2.9:1 against its chip -
/// under the 3:1 the overlay's colours are chosen to - where the red itself is 6.4:1.
fn label_font() -> Font {
    let mut font = overlay_font(10);
    font.weight = FontWeight::DemiBold;
    font
}

/// QFontMetrics' whole-pixel measures of a font: the ascent, the height (the rounded ascent plus
/// the rounded descent) and a text's advance, each rounded as Qt rounds them.
fn whole_ascent(metrics: &dyn FontMetrics, font: &Font) -> f64 {
    metrics.ascent(font).round()
}

fn whole_height(metrics: &dyn FontMetrics, font: &Font) -> f64 {
    metrics.ascent(font).round() + metrics.descent(font).round()
}

fn whole_advance(metrics: &dyn FontMetrics, font: &Font, text: &str) -> f64 {
    metrics.horizontal_advance(font, text).round()
}

/// The box a label's chip covers with its baseline's left end at `corner`: the one sum
/// paint_label draws and the caption keeps off.
fn label_box(metrics: &dyn FontMetrics, corner: PointF, text: &str) -> RectF {
    let font = label_font();
    RectF::new(
        corner.x - 1.0,
        corner.y - whole_ascent(metrics, &font),
        whole_advance(metrics, &font, text) + 5.0,
        whole_height(metrics, &font),
    )
}

/// Text on a small dark chip, so a label reads over whatever it lands on.
fn paint_label(canvas: &mut dyn Canvas, corner: PointF, text: &str, colour: Color) {
    let chip = label_box(canvas.metrics(), corner, text);
    canvas.set_pen(Pen::none());
    canvas.set_brush(Brush::solid(overlay::ground().with_alpha(200)));
    canvas.draw_rounded_rect(chip, 2.0, 2.0);
    canvas.set_brush(Brush::none());
    canvas.set_font(label_font());
    canvas.set_pen(Pen::new(colour));
    canvas.draw_text(PointF::new(corner.x + 1.0, corner.y), text);
}

/// Where each mark's label goes, when it has one and a place.
fn label_corner(mark: &FeedbackMark, frame: &FeedbackFrame) -> Option<PointF> {
    if is_vertex(mark) {
        return Some(
            vertex_of(mark, frame) + PointF::new(VERTEX_LABEL_OFFSET, -VERTEX_LABEL_OFFSET),
        );
    }
    middle_of(&mark.geometry, frame)
        .map(|middle| middle + PointF::new(PIECE_LABEL_OFFSET, -PIECE_LABEL_OFFSET))
}

/// The part of the screen segment a-b inside `bounds`, as the parameters along it where it enters
/// and leaves (Liang and Barsky's clip); None when none of it is inside.
fn clipped_to(a: PointF, b: PointF, bounds: &RectF) -> Option<(f64, f64)> {
    let mut enter: f64 = 0.0;
    let mut leave: f64 = 1.0;
    // One edge: p the segment's rate outward, q how far inside a starts.
    let mut edge = |p: f64, q: f64| {
        if p == 0.0 {
            return q >= 0.0; // parallel to the edge: in or out throughout
        }
        let t = q / p;
        if p < 0.0 {
            enter = katana_math::cpp::max(enter, t);
        } else {
            leave = katana_math::cpp::min(leave, t);
        }
        enter <= leave
    };
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    if edge(-dx, a.x - bounds.left())
        && edge(dx, bounds.right() - a.x)
        && edge(-dy, a.y - bounds.top())
        && edge(dy, bounds.bottom() - a.y)
    {
        return Some((enter, leave));
    }
    None
}

/// Screen points along a piece - a segment, an arc, a whole polyline - every few pixels
/// (PIECE_SAMPLE_SPACING), for what the caption keeps off; only where the piece is inside
/// `bounds`, so a segment running far off the view at a close zoom costs its visible length, not
/// its whole.
fn sample_piece(geometry: &Geometry, frame: &FeedbackFrame, bounds: &RectF, out: &mut Vec<PointF>) {
    let line = |segment: &Segment2, out: &mut Vec<PointF>| {
        let a = (frame.to_screen)(segment.start);
        let b = (frame.to_screen)(segment.end);
        let Some((from, to)) = clipped_to(a, b, bounds) else {
            return;
        };
        let pixels = (b.x - a.x).hypot(b.y - a.y) * (to - from);
        let count = 1.max((pixels / PIECE_SAMPLE_SPACING).ceil() as i32);
        for i in 0..=count {
            let t = from + (to - from) * f64::from(i) / f64::from(count);
            out.push(a + (b - a) * t);
        }
    };
    let curve = |arc: &Arc2, out: &mut Vec<PointF>| {
        // Its length on screen through its middle, a little under an arc's; at most as many
        // samples as the view's edge-to-edge walk needs.
        let a = (frame.to_screen)(arc.point_at(0.0));
        let m = (frame.to_screen)(arc.point_at(0.5));
        let b = (frame.to_screen)(arc.point_at(1.0));
        let pixels = (m.x - a.x).hypot(m.y - a.y) + (b.x - m.x).hypot(b.y - m.y);
        let most = 4.0 * (bounds.width + bounds.height) / PIECE_SAMPLE_SPACING;
        let count = katana_math::cpp::clamp(
            (pixels / PIECE_SAMPLE_SPACING).ceil(),
            1.0,
            katana_math::cpp::max(1.0, most),
        ) as i32;
        for i in 0..=count {
            let p = (frame.to_screen)(arc.point_at(f64::from(i) / f64::from(count)));
            if bounds.contains(p) {
                out.push(p);
            }
        }
    };
    match geometry {
        Geometry::Line(segment) => line(segment, out),
        Geometry::Arc(arc) => curve(arc, out),
        Geometry::Polyline(polyline) => {
            for i in 0..polyline.segment_count() {
                line(&polyline.segment(i), out);
            }
        }
        Geometry::CurvePolyline(whole) => {
            for i in 0..whole.segment_count() {
                match whole.segment(i) {
                    CurveSegment::Arc(arc) => curve(&arc, out),
                    CurveSegment::Segment(segment) => line(&segment, out),
                }
            }
        }
        _ => {}
    }
}

/// What the caption keeps off: every vertex glyph a mark draws, every piece along its length -
/// its middle, where a refusal's ring goes, with room for the ring - and every label. Only a
/// piece's middle was kept off, and at the view's edge the caption was pushed onto the very
/// segment the preview marks.
fn mark_boxes(
    feedback: &ToolFeedback,
    frame: &FeedbackFrame,
    metrics: &dyn FontMetrics,
) -> Vec<RectF> {
    let r = RING_RADIUS + 1.0;
    let near = frame.visible.adjusted(-r, -r, r, r);
    let mut boxes = Vec::new();
    let mut along = Vec::new();
    for mark in &feedback.marks {
        let at = if is_vertex(mark) {
            Some(vertex_of(mark, frame))
        } else {
            middle_of(&mark.geometry, frame)
        };
        if let Some(at) = at {
            boxes.push(RectF::new(at.x - r, at.y - r, 2.0 * r, 2.0 * r));
        }
        if !is_vertex(mark) {
            along.clear();
            sample_piece(&mark.geometry, frame, &near, &mut along);
            for p in &along {
                if near.contains(*p) {
                    boxes.push(RectF::new(
                        p.x - PIECE_SAMPLE_HALF,
                        p.y - PIECE_SAMPLE_HALF,
                        2.0 * PIECE_SAMPLE_HALF,
                        2.0 * PIECE_SAMPLE_HALF,
                    ));
                }
            }
        }
        if !mark.label.is_empty()
            && let Some(corner) = label_corner(mark, frame)
        {
            boxes.push(label_box(metrics, corner, &mark.label));
        }
    }
    boxes
}

/// The caption's chip at the first of the cursor's four corners - below and right, above and
/// right, below and left, above and left - each moved inside the view and off the prompt band,
/// that covers no mark and not the cursor itself; where every one covers something, the one over
/// fewest. It sat below and right whatever was there, and hid the "Enter" place a user who chose
/// a vertex first needs to see.
fn caption_chip(size: SizeF, frame: &FeedbackFrame, keep_off: &[RectF]) -> RectF {
    let below = if frame.tracking_label {
        CAPTION_BELOW_TRACKING
    } else {
        CAPTION_OFFSET
    };
    let c = frame.cursor;
    let room = frame.visible.adjusted(0.0, 0.0, 0.0, -frame.band_height);
    // The crosshair's middle, which a chip pushed back into a small view could land on.
    let cursor = RectF::new(
        c.x - CAPTION_OFFSET / 2.0,
        c.y - CAPTION_OFFSET / 2.0,
        CAPTION_OFFSET,
        CAPTION_OFFSET,
    );
    let right = c.x + CAPTION_OFFSET;
    let left = c.x - CAPTION_OFFSET - size.width;
    let under = c.y + below;
    let over = c.y - CAPTION_OFFSET - size.height;
    let mut best = RectF::default();
    let mut best_covered = usize::MAX;
    for corner in [
        PointF::new(right, under),
        PointF::new(right, over),
        PointF::new(left, under),
        PointF::new(left, over),
    ] {
        let mut chip = RectF::new(corner.x, corner.y, size.width, size.height);
        chip.x = katana_math::cpp::clamp(
            chip.left(),
            room.left(),
            katana_math::cpp::max(room.left(), room.right() - size.width),
        );
        chip.y = katana_math::cpp::clamp(
            chip.top(),
            room.top(),
            katana_math::cpp::max(room.top(), room.bottom() - size.height),
        );
        let covered = keep_off
            .iter()
            .filter(|bounds| rects_intersect(&chip, bounds))
            .count()
            + usize::from(rects_intersect(&chip, &cursor));
        if covered < best_covered {
            best = chip;
            best_covered = covered;
        }
        if covered == 0 {
            break;
        }
    }
    best
}

fn paint_caption(canvas: &mut dyn Canvas, feedback: &ToolFeedback, frame: &FeedbackFrame) {
    let text = &feedback.caption;
    let font = overlay_font(11);
    let pad_x = 4.0;
    let pad_y = 2.0;
    let stripe = 2.0;
    let (width, height, keep_off) = {
        let metrics = canvas.metrics();
        (
            whole_advance(metrics, &font, text) + 2.0 * pad_x + stripe,
            whole_height(metrics, &font) + 2.0 * pad_y,
            mark_boxes(feedback, frame, metrics),
        )
    };
    let chip = caption_chip(SizeF::new(width, height), frame, &keep_off);
    canvas.set_pen(Pen::none());
    canvas.set_brush(Brush::solid(overlay::ground().with_alpha(225)));
    canvas.draw_rounded_rect(chip, 3.0, 3.0);
    canvas.set_brush(Brush::solid(if feedback.refused {
        overlay::removed()
    } else {
        overlay::preview()
    }));
    canvas.draw_rect(RectF::new(chip.left(), chip.top(), stripe, chip.height));
    canvas.set_brush(Brush::none());
    canvas.set_font(font);
    canvas.set_pen(Pen::new(if feedback.refused {
        overlay::removed()
    } else {
        palette::text()
    }));
    canvas.draw_text_in(
        chip.adjusted(stripe + pad_x, 0.0, -pad_x, 0.0),
        TextFlags::aligned(HAlign::Left, VAlign::VCenter),
        text,
    );
}

/// Draws `feedback` in the order that keeps what matters on top: `focus_vertices` (the polyline
/// in play, hollow squares, thinned so that no two are nearer than 10 px - a dense string was
/// buried under a band of them), removed pieces, target pieces, the ghost, the base points,
/// beneath_glyphs, what Enter acts on, target vertices, removed vertices (thinned as the squares
/// are, and never over a target's ring), the refused marks, added vertices, labels (DemiBold, so
/// a digit's stem is inked), and the caption at the first corner of the cursor where it covers
/// no mark, piece or label.
pub fn paint_feedback(
    canvas: &mut dyn Canvas,
    feedback: &ToolFeedback,
    focus_vertices: &[Point2],
    frame: &mut FeedbackFrame,
) -> FeedbackCounts {
    let mut counts = FeedbackCounts::default();
    canvas.save();
    canvas.set_brush(Brush::none());

    // The polyline in play first, under everything the tool marks: its vertices as the grips'
    // cold squares, hollow, vertex 0 numbered (below) so the numbering the prompts use can be
    // read. Drawn over the marks, a survey string's hundreds of squares were a band that buried
    // the target segment and the removed dashes; and a square is left out where it would crowd
    // the last one drawn, keeping each end, so a dense string shows where its vertices are
    // without becoming a solid bar.
    let reach = frame
        .visible
        .adjusted(-FOCUS_SQUARE, -FOCUS_SQUARE, FOCUS_SQUARE, FOCUS_SQUARE);
    // One draw_rects for all of them: a survey string has hundreds, and a call each cost more
    // than the rest of the preview.
    let mut squares = Vec::with_capacity(focus_vertices.len());
    let mut last_drawn: Option<PointF> = None;
    for (i, &vertex) in focus_vertices.iter().enumerate() {
        let p = (frame.to_screen)(vertex);
        let end = i == 0 || i + 1 == focus_vertices.len();
        if !reach.contains(p)
            || (!end && last_drawn.is_some_and(|last| hypot(p, last) < FOCUS_SPACING))
        {
            continue;
        }
        squares.push(RectF::new(
            p.x - FOCUS_SQUARE / 2.0,
            p.y - FOCUS_SQUARE / 2.0,
            FOCUS_SQUARE,
            FOCUS_SQUARE,
        ));
        last_drawn = Some(p);
    }
    canvas.set_pen(pen(overlay::grip_cold(), 1.5));
    canvas.draw_rects(&squares);
    counts.focus = squares.len();

    // Removed pieces: a dim stroke under red dashes, so the dashes read over the drawing's own
    // lines and the orange selection dashes alike.
    for mark in &feedback.marks {
        if mark.role != FeedbackRole::Removed || is_vertex(mark) {
            continue;
        }
        canvas.set_pen(Pen::with(
            overlay::dim(),
            5.0,
            PenStyle::Solid,
            PenCap::Round,
            PenJoin::Round,
        ));
        (frame.draw_shape)(&mut *canvas, &mark.geometry);
        // 5 px on, 4 off: a dash pattern is in pen widths.
        canvas.set_pen(Pen::with(
            overlay::removed(),
            1.6,
            PenStyle::Custom(vec![5.0 / 1.6, 4.0 / 1.6]),
            PenCap::Flat,
            PenJoin::Round,
        ));
        (frame.draw_shape)(&mut *canvas, &mark.geometry);
        counts.removed += 1;
    }
    // A mark that says why a click is refused keeps its place, which says what the pick took, but
    // in the refusal's red AND its shape - the ring struck through (paint_struck): green there
    // promised the click the caption says is turned down, and red alone is green to many eyes.
    let any_flagged = feedback.marks.iter().any(|mark| mark.refused);
    let refused_mark = |mark: &FeedbackMark| drawn_refused(mark, feedback, any_flagged);
    // Target pieces: a solid 3 px line along the geometry, arcs as arcs; a refused one red, with
    // the struck ring at its middle (below).
    for mark in &feedback.marks {
        if mark.role != FeedbackRole::Target || is_vertex(mark) {
            continue;
        }
        let stroke = if refused_mark(mark) {
            overlay::removed()
        } else {
            overlay::target()
        };
        canvas.set_pen(Pen::with(
            stroke.with_alpha(235),
            3.0,
            PenStyle::Solid,
            PenCap::Round,
            PenJoin::Round,
        ));
        (frame.draw_shape)(&mut *canvas, &mark.geometry);
    }
    // The ghost: the result as it will be, dashed, as every tool's always was.
    canvas.set_pen(Pen::with(
        overlay::preview(),
        1.0,
        PenStyle::Dash,
        PenCap::Square,
        PenJoin::Bevel,
    ));
    for shape in &feedback.shapes {
        (frame.draw_shape)(&mut *canvas, shape);
    }
    counts.shapes = feedback.shapes.len();
    // Base points: a small open square, the grip another CAD program draws at one.
    canvas.set_pen(pen(overlay::preview(), 1.5));
    for &marker in &feedback.markers {
        let p = (frame.to_screen)(marker);
        canvas.draw_rect(RectF::new(
            p.x - MARKER_HALF,
            p.y - MARKER_HALF,
            2.0 * MARKER_HALF,
            2.0 * MARKER_HALF,
        ));
    }
    counts.markers = feedback.markers.len();
    // What the view puts beneath the glyphs: its snap marker.
    if let Some(beneath) = frame.beneath_glyphs.as_mut() {
        canvas.save();
        beneath(&mut *canvas);
        canvas.restore();
    }
    // What Enter acts on (the Enter role): Insert's place at a segment's middle, where Enter adds
    // a vertex, and the vertex chosen before a tool while the pointer is on another - the one
    // Enter deletes, straightens to or makes the start, as the label beside it and the prompt
    // say. The Added disc's circle round a dot, its middle the dark ground, so the line through
    // it stops at the ring and no circled minus sits beside the new vertex's circled plus. Drawn
    // BEFORE the Target vertices: its opaque ground covered the ring and square of a chosen
    // vertex a few pixels off on a short segment - the vertex the owner asked to see.
    for mark in &feedback.marks {
        if mark.role != FeedbackRole::Enter || !is_vertex(mark) {
            continue;
        }
        counts.enter += 1;
        if refused_mark(mark) {
            continue; // struck, below
        }
        let p = vertex_of(mark, frame);
        let ring = ellipse_at(p, ADDED_RADIUS, ADDED_RADIUS);
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(overlay::ground())); // opaque: no line shows through
        canvas.draw_ellipse(ring);
        canvas.set_brush(Brush::none());
        canvas.set_pen(pen(overlay::ground(), 4.0));
        canvas.draw_ellipse(ring);
        canvas.set_pen(pen(overlay::preview(), 2.0));
        canvas.draw_ellipse(ring);
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(overlay::preview()));
        canvas.draw_ellipse(ellipse_at(p, ENTER_DOT, ENTER_DOT));
        canvas.set_brush(Brush::none());
    }
    // Target vertices: a ring round a filled square.
    let mut rings: Vec<PointF> = Vec::new();
    for mark in &feedback.marks {
        if mark.role != FeedbackRole::Target || !is_vertex(mark) {
            continue;
        }
        let p = vertex_of(mark, frame);
        rings.push(p); // a refused one is a ring too, struck (below)
        if refused_mark(mark) {
            continue;
        }
        canvas.set_pen(pen(overlay::target(), 2.0));
        canvas.set_brush(Brush::none());
        canvas.draw_ellipse(ellipse_at(p, RING_RADIUS, RING_RADIUS));
        canvas.set_pen(pen(overlay::ground(), 1.0));
        canvas.set_brush(Brush::solid(overlay::target()));
        canvas.draw_rect(RectF::new(
            p.x - TARGET_SQUARE / 2.0,
            p.y - TARGET_SQUARE / 2.0,
            TARGET_SQUARE,
            TARGET_SQUARE,
        ));
        canvas.set_brush(Brush::none());
    }
    // Removed vertices: a red X on a dark disc. Never over a ring - the "keep" ring at a dense
    // string's straightened end had its neighbour's X across it, a circled X that crossed out a
    // vertex that stays - and thinned as the focus squares are, so a run of them is not one red
    // rope; the removed dashes still show the whole run.
    let backing = overlay::ground().with_alpha(190);
    let mut last_x: Option<PointF> = None;
    for mark in &feedback.marks {
        if mark.role != FeedbackRole::Removed || !is_vertex(mark) || refused_mark(mark) {
            continue;
        }
        let p = vertex_of(mark, frame);
        // Clear of the ring's pen (its outer edge 1 px past its radius) by the whole of the X,
        // its disc or an arm's end, whichever is further.
        let over_a_ring = rings.iter().any(|&ring| {
            hypot(ring, p)
                < RING_RADIUS + 1.0 + katana_math::cpp::max(REMOVED_BACKING, REMOVED_REACH)
        });
        if over_a_ring || last_x.is_some_and(|last| hypot(p, last) < FOCUS_SPACING) {
            continue;
        }
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(backing));
        canvas.draw_ellipse(ellipse_at(p, REMOVED_BACKING, REMOVED_BACKING));
        canvas.set_brush(Brush::none());
        canvas.set_pen(Pen::with(
            overlay::removed(),
            2.5,
            PenStyle::Solid,
            PenCap::Round,
            PenJoin::Bevel,
        ));
        canvas.draw_line(
            p + PointF::new(-REMOVED_HALF, -REMOVED_HALF),
            p + PointF::new(REMOVED_HALF, REMOVED_HALF),
        );
        canvas.draw_line(
            p + PointF::new(-REMOVED_HALF, REMOVED_HALF),
            p + PointF::new(REMOVED_HALF, -REMOVED_HALF),
        );
        last_x = Some(p);
        counts.removed += 1;
    }
    // The refused marks: the struck ring on a vertex, and at a piece's middle.
    for mark in &feedback.marks {
        if !refused_mark(mark) {
            continue;
        }
        if is_vertex(mark) {
            paint_struck(canvas, vertex_of(mark, frame));
        } else if let Some(middle) = middle_of(&mark.geometry, frame) {
            paint_struck(canvas, middle);
        }
    }
    // Added vertices: a filled disc with a "+", on top of everything else - except over a vertex
    // that goes. A fillet or a chamfer a few pixels across puts its new vertices on the corner's
    // X, and three glyphs on one spot say nothing; the X and the caption do, until the view is
    // zoomed in.
    let near_removed = |p: PointF| {
        feedback.marks.iter().any(|other| {
            other.role == FeedbackRole::Removed
                && is_vertex(other)
                && hypot(vertex_of(other, frame), p) < ADDED_RADIUS + REMOVED_HALF
        })
    };
    for mark in &feedback.marks {
        if mark.role != FeedbackRole::Added || !is_vertex(mark) {
            continue;
        }
        let p = vertex_of(mark, frame);
        if near_removed(p) {
            continue;
        }
        canvas.set_pen(pen(overlay::ground(), 1.5));
        canvas.set_brush(Brush::solid(overlay::preview()));
        canvas.draw_ellipse(ellipse_at(p, ADDED_RADIUS, ADDED_RADIUS));
        canvas.set_brush(Brush::none());
        canvas.set_pen(Pen::with(
            overlay::ground(),
            1.5,
            PenStyle::Solid,
            PenCap::Flat,
            PenJoin::Bevel,
        ));
        canvas.draw_line(
            p + PointF::new(-PLUS_ARM, 0.0),
            p + PointF::new(PLUS_ARM, 0.0),
        );
        canvas.draw_line(
            p + PointF::new(0.0, -PLUS_ARM),
            p + PointF::new(0.0, PLUS_ARM),
        );
        counts.added += 1;
    }
    // Added pieces are the ghost's to show, and neither they nor Enter's place are counted here:
    // the Added and Removed vertices, the removed pieces and Enter's were counted as drawn,
    // above, and the record says what is on screen. Every Target is drawn, a refused one struck.
    counts.target = feedback
        .marks
        .iter()
        .filter(|mark| mark.role == FeedbackRole::Target)
        .count();
    // Labels, above the glyphs: vertex 0 of the polyline in play, then each mark's own (a vertex
    // number, "keep", a height).
    if let Some(&first_vertex) = focus_vertices.first() {
        let first = (frame.to_screen)(first_vertex);
        // Not where a mark of the tool labels that vertex itself: two chips on one vertex read as
        // neither.
        let labelled = feedback.marks.iter().any(|mark| {
            !mark.label.is_empty() && is_vertex(mark) && hypot(vertex_of(mark, frame), first) < 1.0
        });
        if reach.contains(first) && !labelled {
            paint_label(
                canvas,
                first + PointF::new(6.0, -6.0),
                "0",
                lighter(overlay::grip_cold(), 130),
            );
        }
    }
    for mark in &feedback.marks {
        if mark.label.is_empty() {
            continue;
        }
        let colour = if refused_mark(mark) || mark.role == FeedbackRole::Removed {
            overlay::removed()
        } else if mark.role == FeedbackRole::Enter {
            overlay::preview()
        } else {
            overlay::target()
        };
        if let Some(at) = label_corner(mark, frame) {
            paint_label(canvas, at, &mark.label, colour);
        }
    }
    if !feedback.caption.is_empty() {
        paint_caption(canvas, feedback, frame);
    }
    canvas.restore();
    counts
}

/// The prompt band's height in pixels at the chosen text size.
pub fn band_height(metrics: &dyn FontMetrics) -> i32 {
    whole_height(metrics, &overlay_font(12)) as i32 + 2 * 4
}

/// The band along the bottom of a view holding a prompt and what has been typed for it - a
/// tool's or a picked-up grip's, one look for both - and answers the line it drew, cut to fit.
/// Cut short at the left by default (Elide::Left), so what is being typed at the end of a prompt
/// stays in sight; a hint is cut at the right, where its start says what it is about; and a
/// tool's prompt with nothing typed yet in the middle, keeping both the tool's name and what
/// Enter does at the end (the view's draw_prompt).
pub fn paint_band(canvas: &mut dyn Canvas, view: IntRect, text: &str, elide: Elide) -> String {
    // The prompt in the view as well as in the window's command line: the eye is on the drawing,
    // and a floating view may be far from the window.
    let font = overlay_font(12);
    let pad = 4;
    let height = whole_height(canvas.metrics(), &font) as i32 + 2 * pad;
    let band = IntRect::new(view.x, view.bottom() + 1 - height, view.width, height);
    let fill = overlay::ground().with_alpha(220);
    let line =
        canvas
            .metrics()
            .elided_text(&font, text, elide, f64::from(band.width - 2 * pad - 2));
    canvas.save();
    canvas.fill_rect(band.to_rect_f(), &Brush::solid(fill));
    canvas.set_font(font);
    canvas.set_pen(Pen::new(overlay::preview()));
    let inner = IntRect::new(
        band.x + pad + 2,
        band.y,
        band.width - (pad + 2) - pad,
        band.height,
    );
    canvas.draw_text_in(
        inner.to_rect_f(),
        TextFlags::aligned(HAlign::Left, VAlign::VCenter),
        &line,
    );
    canvas.restore();
    line
}
