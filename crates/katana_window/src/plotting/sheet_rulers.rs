//! The sheet canvas's rulers and paper grid (docs/plotting.md, "Editing on the canvas"): screen
//! furniture, painted over the sheet and never plotted. The port of plotting/sheet_rulers.
//!
//! ```text
//! the rulers   along the canvas's top and left edges, in paper millimetres from the paper's
//!              bottom-left corner (Y up, as every paper coordinate is), following the zoom and
//!              the pan; the paper's span is white on them, the selection's is shaded, and the
//!              cursor is marked on both
//! the grid     faint lines every so many millimetres through the same origin: the lines a drag
//!              snaps to (cad's snap_to_grid)
//! ```

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]

use katana_geometry::{Box2, Point2};

use crate::canvas::{
    Brush, Canvas, Color, Font, FontSize, Pen, PointF, RectF, RenderHint, SizeF, TextFlags,
};
use crate::format::qt_fixed;

/// How thick the rulers are, in logical pixels.
pub const SHEET_RULER_PIXELS: i32 = 20;

/// A ruler's numbered step and its tick step, in paper millimetres: the smallest of 1, 2, 5, 10,
/// 20 ... mm whose numbers stand at least 50 pixels apart, and ticks between them no closer than
/// 5 pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerSteps {
    pub numbered: f64,
    pub tick: f64,
}

impl Default for RulerSteps {
    fn default() -> Self {
        Self {
            numbered: 10.0,
            tick: 1.0,
        }
    }
}

/// The paper as the canvas shows it, and what the rulers mark on it.
#[derive(Clone, Debug, PartialEq)]
pub struct RulerView {
    pub pixels_per_millimetre: f64,
    /// The widget point of the paper's top-left corner.
    pub paper_origin: PointF,
    pub paper_width_mm: f64,
    pub paper_height_mm: f64,
    /// The cursor's paper position, when it is over the canvas.
    pub cursor: Option<Point2>,
    /// The selection's extent on the paper; empty for none.
    pub highlight: Box2,
}

impl Default for RulerView {
    fn default() -> Self {
        Self {
            pixels_per_millimetre: 1.0,
            paper_origin: PointF::new(0.0, 0.0),
            paper_width_mm: 420.0,
            paper_height_mm: 297.0,
            cursor: None,
            highlight: Box2::default(),
        }
    }
}

const RULER_GROUND: Color = Color::rgb(236, 237, 240);
const RULER_PAPER: Color = Color::rgb(255, 255, 255);
const RULER_INK: Color = Color::rgb(70, 72, 78);
/// The ruler's ink lightened to 160 % (QColor::lighter(160) of 70, 72, 78: its value, 78, times
/// 1.6 at the same hue and saturation).
const RULER_EDGE: Color = Color::rgb(112, 115, 125);
const RULER_HIGHLIGHT: Color = Color::rgba(0, 120, 215, 55);
const RULER_CURSOR: Color = Color::rgb(220, 40, 60);
const GRID_LINE: Color = Color::rgba(0, 120, 215, 26);
const GRID_MAJOR: Color = Color::rgba(0, 120, 215, 52);

const NUMBER_SPACING_PIXELS: f64 = 50.0;
const TICK_SPACING_PIXELS: f64 = 5.0;
const GRID_SPACING_PIXELS: f64 = 4.0;

/// The whole steps `step` apart from `from` to `to` (either way round), by index so that a long
/// ruler does not add up rounding.
fn each_step(from: f64, to: f64, step: f64, mut visit: impl FnMut(f64, i64)) {
    if !(step > 0.0) {
        return;
    }
    let lo = katana_math::cpp::min(from, to);
    let hi = katana_math::cpp::max(from, to);
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    for i in first..=last {
        visit(i as f64 * step, i);
    }
}

fn number(mm: f64) -> String {
    qt_fixed(mm, if mm == mm.floor() { 0 } else { 1 })
}

/// A hairline, a pixel wide whatever the canvas's transform (QPen(colour, 0)).
fn hairline(colour: Color) -> Pen {
    Pen {
        color: colour,
        width: 0.0,
        ..Pen::default()
    }
}

fn one_pixel(colour: Color) -> Pen {
    Pen {
        color: colour,
        width: 1.0,
        ..Pen::default()
    }
}

pub fn ruler_steps(pixels_per_millimetre: f64) -> RulerSteps {
    const STEPS: [f64; 12] = [
        1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0,
    ];
    let largest = STEPS[STEPS.len() - 1];
    let mut steps = RulerSteps {
        numbered: largest,
        tick: largest / 10.0,
    };
    let ppmm = if pixels_per_millimetre > 0.0 {
        pixels_per_millimetre
    } else {
        1.0
    };
    for step in STEPS {
        if step * ppmm >= NUMBER_SPACING_PIXELS {
            steps.numbered = step;
            break;
        }
    }
    // A tenth, a fifth or a half of the numbered step: the finest that is not crowded.
    steps.tick = steps.numbered;
    for divisor in [10.0, 5.0, 2.0] {
        if steps.numbered / divisor * ppmm >= TICK_SPACING_PIXELS {
            steps.tick = steps.numbered / divisor;
            break;
        }
    }
    steps
}

/// The two rulers and the corner between them, over a canvas of `size` logical pixels.
pub fn paint_sheet_rulers(canvas: &mut dyn Canvas, size: SizeF, view: &RulerView) {
    let r = f64::from(SHEET_RULER_PIXELS);
    let ppmm = view.pixels_per_millimetre;
    if !(ppmm > 0.0) {
        return;
    }
    canvas.save();
    canvas.set_render_hint(RenderHint::Antialiasing, false);
    let top = RectF::new(r, 0.0, size.width - r, r);
    let left = RectF::new(0.0, r, r, size.height - r);
    canvas.fill_rect(top, &Brush::solid(RULER_GROUND));
    canvas.fill_rect(left, &Brush::solid(RULER_GROUND));

    // Paper millimetres to the widget and back, along each ruler.
    let x_at = |mm: f64| view.paper_origin.x + mm * ppmm;
    let y_at = |mm: f64| view.paper_origin.y + (view.paper_height_mm - mm) * ppmm;
    let mm_at_x = |x: f64| (x - view.paper_origin.x) / ppmm;
    let mm_at_y = |y: f64| view.paper_height_mm - (y - view.paper_origin.y) / ppmm;
    let along_top = |from_mm: f64, to_mm: f64| {
        RectF::from_corners(
            PointF::new(katana_math::cpp::max(x_at(from_mm), r), 0.0),
            PointF::new(katana_math::cpp::max(x_at(to_mm), r), r),
        )
    };
    let down_left = |top_mm: f64, bottom_mm: f64| {
        RectF::from_corners(
            PointF::new(0.0, katana_math::cpp::max(y_at(top_mm), r)),
            PointF::new(r, katana_math::cpp::max(y_at(bottom_mm), r)),
        )
    };

    // The paper's span, white; the selection's, shaded.
    canvas.fill_rect(
        along_top(0.0, view.paper_width_mm),
        &Brush::solid(RULER_PAPER),
    );
    canvas.fill_rect(
        down_left(view.paper_height_mm, 0.0),
        &Brush::solid(RULER_PAPER),
    );
    if !view.highlight.empty() {
        canvas.fill_rect(
            along_top(view.highlight.min.x, view.highlight.max.x),
            &Brush::solid(RULER_HIGHLIGHT),
        );
        canvas.fill_rect(
            down_left(view.highlight.max.y, view.highlight.min.y),
            &Brush::solid(RULER_HIGHLIGHT),
        );
    }

    let steps = ruler_steps(ppmm);
    let per_number = (steps.numbered / steps.tick).round() as i64;
    let font = Font {
        size: FontSize::Pixels(9),
        ..canvas.font()
    };
    canvas.set_font(font.clone());
    canvas.set_pen(hairline(RULER_INK));
    let tick_length = |i: i64| {
        if i % per_number == 0 {
            r * 0.55
        } else if per_number % 2 == 0 && i % (per_number / 2) == 0 {
            r * 0.35
        } else {
            r * 0.2
        }
    };

    // Along the top.
    canvas.save();
    canvas.set_clip_rect(top, false);
    each_step(mm_at_x(r), mm_at_x(size.width), steps.tick, |mm, i| {
        let x = x_at(mm).round() + 0.5;
        canvas.draw_line(PointF::new(x, r), PointF::new(x, r - tick_length(i)));
        if i % per_number == 0 {
            canvas.draw_text(PointF::new(x + 2.0, 9.0), &number(mm));
        }
    });
    canvas.restore();

    // Down the left, its numbers turned to read from below.
    canvas.save();
    canvas.set_clip_rect(left, false);
    each_step(mm_at_y(size.height), mm_at_y(r), steps.tick, |mm, i| {
        let y = y_at(mm).round() + 0.5;
        canvas.draw_line(PointF::new(r, y), PointF::new(r - tick_length(i), y));
        if i % per_number == 0 {
            canvas.save();
            canvas.translate(9.0, y - 2.0);
            canvas.rotate(-90.0);
            canvas.draw_text(PointF::new(0.0, 0.0), &number(mm));
            canvas.restore();
        }
    });
    canvas.restore();

    // The cursor on both.
    if let Some(cursor) = view.cursor {
        canvas.set_pen(one_pixel(RULER_CURSOR));
        let x = x_at(cursor.x);
        let y = y_at(cursor.y);
        if x >= r {
            canvas.draw_line(PointF::new(x, 0.0), PointF::new(x, r));
        }
        if y >= r {
            canvas.draw_line(PointF::new(0.0, y), PointF::new(r, y));
        }
    }

    // Their edges, and the corner.
    canvas.set_pen(one_pixel(RULER_EDGE));
    canvas.draw_line(PointF::new(r, r - 0.5), PointF::new(size.width, r - 0.5));
    canvas.draw_line(PointF::new(r - 0.5, r), PointF::new(r - 0.5, size.height));
    canvas.fill_rect(RectF::new(0.0, 0.0, r, r), &Brush::solid(RULER_GROUND));
    canvas.set_pen(hairline(RULER_INK));
    canvas.set_font(Font {
        size: FontSize::Pixels(8),
        ..font
    });
    canvas.draw_text_in(RectF::new(0.0, 0.0, r, r), TextFlags::CENTER, "mm");
    canvas.restore();
}

/// Faint lines every `spacing_mm` across the paper, every tenth a little stronger. Where the lines
/// would crowd closer than 4 pixels only every second, fifth or tenth is drawn, so the paper never
/// turns grey.
pub fn paint_paper_grid(canvas: &mut dyn Canvas, view: &RulerView, spacing_mm: f64) {
    let ppmm = view.pixels_per_millimetre;
    if !(spacing_mm > 0.0) || !(ppmm > 0.0) {
        return;
    }
    // Every line, or every 2nd, 5th or 10th of them where they would crowd.
    let mut every: i64 = 1;
    for skip in [1_i64, 2, 5, 10, 20, 50] {
        every = skip;
        if spacing_mm * skip as f64 * ppmm >= GRID_SPACING_PIXELS {
            break;
        }
    }
    let step = spacing_mm * every as f64;
    let paper = RectF::new(
        view.paper_origin.x,
        view.paper_origin.y,
        view.paper_width_mm * ppmm,
        view.paper_height_mm * ppmm,
    );
    canvas.save();
    canvas.set_render_hint(RenderHint::Antialiasing, false);
    canvas.set_clip_rect(paper, true);
    // Every tenth line of the grid (not of what is drawn) is a major one.
    let major_every = (10 / every).max(1);
    let pen_of = |i: i64| {
        hairline(if i % major_every == 0 {
            GRID_MAJOR
        } else {
            GRID_LINE
        })
    };
    each_step(0.0, view.paper_width_mm, step, |mm, i| {
        canvas.set_pen(pen_of(i));
        let x = (view.paper_origin.x + mm * ppmm).round() + 0.5;
        canvas.draw_line(PointF::new(x, paper.top()), PointF::new(x, paper.bottom()));
    });
    each_step(0.0, view.paper_height_mm, step, |mm, i| {
        canvas.set_pen(pen_of(i));
        let y = (view.paper_origin.y + (view.paper_height_mm - mm) * ppmm).round() + 0.5;
        canvas.draw_line(PointF::new(paper.left(), y), PointF::new(paper.right(), y));
    });
    canvas.restore();
}
