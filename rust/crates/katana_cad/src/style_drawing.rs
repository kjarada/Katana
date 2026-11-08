//! A library linestyle or symbol definition -> plain geometry
//! (include/katana/cad/style_drawing.hpp).
//!
//! `entity::LineStyle` says what a definition IS; this says where its strokes
//! land. Everything comes out as polylines and texts in MODEL coordinates, so
//! the viewport, the plotter and a preview all paint the same thing from one
//! definition - which is why this is here and not in the viewport, exactly as
//! `symbols::symbol_strokes` already is. Which definition a NAME means is
//! style_resolver's question, not this module's.
//!
//! Two ways to place a definition, and the definition itself says which:
//!
//! * `at_vertices` (the file's `mode vertex`) makes it a SYMBOL: the strokes
//!   are put at a point, scaled and rotated. `symbol_drawing`.
//! * otherwise it is a LINESTYLE: the strokes repeat along a polyline, with
//!   the definition's +x running along the line and its +y to the left.
//!   `linestyle_drawing`, or `lay_linestyle` for a caller that can see only
//!   part of the line.
//!
//! A definition's coordinates mean different things - see entity::StyleUnits -
//! so both take `paper_scale`, the model units one plot millimetre covers. A
//! `worldstyle` ignores it; a `paperstyle` multiplies by it, which is what
//! keeps a 1.5 mm tick 1.5 mm on the page at any zoom.
//!
//! Every placement starts from a FlatDefinition: the strokes flattened once
//! into runs. A caller drawing thousands of points with one symbol flattens it
//! once (style_resolver's DefinitionCache) instead of once per point per
//! frame. The C++ overloads that take a LineStyle flatten on every call; they
//! are the `*_of_style` functions here, the FlatDefinition overloads keeping
//! the C++ name.

use katana_entity::customisation::CUSTOMISATION_MOST_SWEEP_DEGREES;
use katana_entity::{LineStyle, StrokeOp, StrokeText, StyleUnits};
use katana_geometry::{Box2, Point2, Polyline2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{PI, tolerance};

// The same count symbols::symbol_strokes uses, and for the same reason: a
// symbol is small on any output and the eye cannot tell 24 chords from a
// circle at that size.
const CIRCLE_CHORDS: i32 = 24;
// A pattern shorter than this would repeat without end along any real line.
// It is the geometric tolerance, which is what "two points in the same place"
// already means everywhere else.
const SHORTEST_PERIOD: f64 = tolerance::GEOMETRIC;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleStroke {
    pub path: Polyline2,
    /// The colour name in force, from a `colour` command. EMPTY means the
    /// entity's own colour, which is what "view_colour" asks for.
    pub pen: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StyleTextMark {
    pub at: Point2,
    pub text: String,
    /// Model units.
    pub height: f64,
    /// Radians counter-clockwise, the line's direction included.
    pub angle: f64,
    /// The file's spelling: "middle-centre", "top-left".
    pub justify: String,
    pub font: String,
    pub width_factor: f64,
    pub pen: String,
}

impl Default for StyleTextMark {
    fn default() -> Self {
        Self {
            at: Point2::default(),
            text: String::new(),
            height: 0.0,
            angle: 0.0,
            justify: String::new(),
            font: String::new(),
            width_factor: 1.0,
            pen: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleDrawing {
    pub strokes: Vec<StyleStroke>,
    pub texts: Vec<StyleTextMark>,
}

impl StyleDrawing {
    pub fn empty(&self) -> bool {
        self.strokes.is_empty() && self.texts.is_empty()
    }

    /// Every point of every stroke, plus each text's anchor.
    pub fn bounds(&self) -> Box2 {
        let mut bounds = Box2::default();
        for stroke in &self.strokes {
            for &point in &stroke.path.vertices {
                bounds.expand(point);
            }
        }
        for text in &self.texts {
            bounds.expand(text.at);
        }
        bounds
    }
}

/// A run of connected points of a FlatDefinition (the C++
/// FlatDefinition::Run).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatDefinitionRun {
    pub points: Vec<Point2>,
    pub closed: bool,
    pub pen: String,
}

/// A text of a FlatDefinition (the C++ FlatDefinition::Text).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatDefinitionText {
    pub at: Point2,
    pub text: StrokeText,
    pub pen: String,
}

/// A definition's strokes flattened into runs of connected points, in its own
/// coordinates with `factor` and the origin applied. It OWNS everything it
/// holds - the texts are copied, not pointed at - so it outlives the library
/// it was flattened from: a cache may keep one across a set_style_library
/// without anything in it dangling.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatDefinition {
    pub units: StyleUnits,
    pub at_vertices: bool,
    /// The file's `length`, as LineStyle::length; 0 when it said nothing.
    pub length: f64,
    /// LineStyle::bounds(): what a symbol `size` is a width OF.
    pub bounds: Box2,
    /// TwoPoint only: the anchors in the same frame as the runs.
    pub anchor1: Point2,
    pub anchor2: Point2,
    pub runs: Vec<FlatDefinitionRun>,
    pub texts: Vec<FlatDefinitionText>,

    /// How far the PEN travelled along the definition's x, moves included.
    ///
    /// This is the period of the pattern when the file gives no `length`, and
    /// it is not the same as the span of what was drawn: a linestyle ends its
    /// period with a bare `move`, and that move IS the gap.
    /// "move 0 0 / draw 3 0 / move 5 0" is a three-unit dash and a two-unit
    /// gap. Measuring only the drawn part gave a period of 3, so every dash
    /// butted against the next and the whole linestyle came out as a solid
    /// line - which is exactly what it looked like.
    pub lowest_x: f64,
    pub highest_x: f64,
    pub any_point: bool,
    /// How far along x the MARKS reach: every point of every run - an arc's
    /// chords, a circle's points, a dot - and every text's anchor.
    ///
    /// Not the pen's travel above, and not a period: an arc is written as a
    /// move to its CENTRE, so a scallop's pen stands only at the middle of a
    /// half-circle whose ends are a radius either side of it, and a Draw can
    /// start from where the pen began, which no Move stood on. Laying only the
    /// repeats that can reach a view needs where the marks are; measured by
    /// the pen, the last repeat at the end of a line was left out.
    pub drawn_low_x: f64,
    pub drawn_high_x: f64,
    pub any_drawn: bool,
    /// The furthest any mark sits across the line (|y|), a text's height times
    /// its length included: how far a pattern can reach into a view from a
    /// line that is itself just outside it.
    pub reach_across: f64,
}

impl FlatDefinition {
    pub fn natural_period(&self) -> f64 {
        if self.any_point {
            self.highest_x - self.lowest_x
        } else {
            0.0
        }
    }
}

/// How a linestyle is laid when the caller can see only part of the line and
/// has a frame to paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinestyleOptions {
    pub paper_scale: f64,
    /// Pixels per model unit, for the level of detail. 0 lays the pattern at
    /// any size, which is what a test or a caller with no screen wants.
    pub view_scale: f64,
    /// A pattern whose period is shorter than this on screen is not laid: the
    /// caller draws the plain line. Below about two pixels the repeats merge
    /// into a smudge the colour of the line, so the plain line is both what
    /// the eye sees anyway and thousands of times cheaper.
    pub minimum_period_pixels: f64,
    /// The part of the drawing the caller can see. When given, only the
    /// repeats that can reach it are laid - the box is grown by one period and
    /// by the pattern's reach across the line - and each is laid exactly where
    /// it falls on the WHOLE line, so the pattern keeps its phase from the
    /// first vertex and does not crawl as the view pans.
    pub visible: Option<Box2>,
    /// More repeats than this and NOTHING is laid (OverBudget): the caller
    /// draws the plain line. The all-or-nothing contract is for_each_dash's
    /// (dashing.rs), for the same reason: a truncated pattern is a wrong
    /// drawing with no error anywhere (audit CAD-04).
    pub maximum_instances: usize,
}

impl Default for LinestyleOptions {
    fn default() -> Self {
        Self {
            paper_scale: 1.0,
            view_scale: 0.0,
            minimum_period_pixels: 2.0,
            visible: None,
            maximum_instances: 20_000,
        }
    }
}

/// What came of laying a linestyle (the C++ LinestyleLayout::Outcome).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LinestyleLayoutOutcome {
    /// The drawing IS the line, gaps and all; the caller draws nothing else.
    /// It can be empty when none of the line is in view.
    Laid,
    /// Nothing to lay a pattern along (a degenerate line): plain line.
    #[default]
    NothingToLay,
    /// Under LinestyleOptions::minimum_period_pixels: plain line.
    TooFine,
    /// Over LinestyleOptions::maximum_instances: plain line.
    OverBudget,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinestyleLayout {
    pub drawing: StyleDrawing,
    pub outcome: LinestyleLayoutOutcome,
    /// How many repeats were laid; for Laid only.
    pub instances: usize,
}

// The units a definition's coordinates are in, as a multiplier onto model
// units.
fn unit_scale(units: StyleUnits, paper_scale: f64) -> f64 {
    if units == StyleUnits::Paper {
        paper_scale
    } else {
        1.0
    }
}

fn add_arc(
    into: &mut Vec<Point2>,
    centre: Point2,
    radius: f64,
    from_degrees: f64,
    to_degrees: f64,
) {
    let from = from_degrees * PI / 180.0;
    let to = to_degrees * PI / 180.0;
    // Libraries write an arc both ways round (`arc 1.75 0 180` and `arc 1.75
    // 180 0`), so the sweep's sign is taken from the numbers rather than
    // assumed.
    //
    // A customisation file is read with no arc sweeping more than two turns
    // (entity's CUSTOMISATION_MOST_SWEEP_DEGREES), but a definition made in
    // code is not held to it, and the number of chords follows the sweep: one
    // of a billion degrees would ask for twenty million points, and a sweep
    // past what an int holds of chords is undefined behaviour to cast in the
    // C++. So the sweep drawn is cut to the file's own bound, and one that is
    // not a number draws nothing.
    let raw = to - from;
    if !raw.is_finite() {
        return;
    }
    const MOST_SWEEP: f64 = CUSTOMISATION_MOST_SWEEP_DEGREES * PI / 180.0;
    let sweep = cpp::clamp(raw, -MOST_SWEEP, MOST_SWEEP);
    let steps = std::cmp::max(
        2,
        (sweep.abs() / (2.0 * PI) * f64::from(CIRCLE_CHORDS)).ceil() as i32,
    );
    for i in 0..=steps {
        let angle = from + sweep * f64::from(i) / f64::from(steps);
        into.push(Point2::new(
            centre.x + radius * angle.cos(),
            centre.y + radius * angle.sin(),
        ));
    }
}

// Where a distance along a polyline lands, and which way the line is going
// there. A distance outside the line is CLAMPED to its end, and the caller is
// told, so that a stroke reaching past the end stops at the end and one
// entirely beyond it is dropped. Letting it run on drew a 164 m dash for a
// 30 m fence and scribbled it across the drawing.
#[derive(Clone, Copy)]
struct Frame {
    at: Point2,
    along: Vec2,
}

struct Path {
    points: Vec<Point2>,
    starts: Vec<f64>,
    length: f64,
}

impl Path {
    fn new(line: &Polyline2) -> Self {
        let mut path = Self {
            points: Vec::new(),
            starts: Vec::new(),
            length: 0.0,
        };
        let vertices = &line.vertices;
        let Some(&first) = vertices.first() else {
            return path;
        };
        path.points.push(first);
        path.starts.push(0.0);
        for pair in vertices.windows(2) {
            let step = pair[0].distance_to(pair[1]);
            if step <= 0.0 {
                continue; // a repeated vertex has no direction to offer
            }
            path.length += step;
            path.points.push(pair[1]);
            path.starts.push(path.length);
        }
        if line.closed && path.points.len() > 2 {
            let step = path.points[path.points.len() - 1].distance_to(path.points[0]);
            if step > 0.0 {
                path.length += step;
                path.points.push(path.points[0]);
                path.starts.push(path.length);
            }
        }
        path
    }

    fn usable(&self) -> bool {
        self.points.len() >= 2 && self.length > 0.0
    }

    fn at(&self, distance: f64) -> Frame {
        let distance = cpp::clamp(distance, 0.0, self.length);
        // The segment whose span contains the distance; the first or last
        // where it falls outside, so an overhang keeps going straight. The
        // first start after the distance, as std::upper_bound finds it.
        let found = self.starts.partition_point(|&start| !(distance < start));
        let segment = found.saturating_sub(1).min(self.points.len() - 2);
        let from = self.points[segment];
        let to = self.points[segment + 1];
        let span = self.starts[segment + 1] - self.starts[segment];
        let direction = if span > 0.0 {
            (to - from) / span
        } else {
            Vec2::new(1.0, 0.0)
        };
        let into = distance - self.starts[segment];
        Frame {
            at: Point2::new(from.x + direction.x * into, from.y + direction.y * into),
            along: direction,
        }
    }

    // The stretches of the line inside `bounds`, as distances along it, in
    // order and with touching stretches merged. Each segment is clipped to the
    // box on its own parameter (Liang and Barsky's), which is exact for
    // straight segments and a polyline is nothing else.
    #[allow(
        clippy::needless_range_loop,
        reason = "the index is the segment's number: it reads both its ends and both their starts"
    )]
    fn inside(&self, bounds: &Box2) -> Vec<(f64, f64)> {
        let mut spans: Vec<(f64, f64)> = Vec::new();
        if bounds.empty() {
            return spans;
        }
        for i in 0..self.points.len().saturating_sub(1) {
            let a = self.points[i];
            let b = self.points[i + 1];
            let mut enter: f64 = 0.0;
            let mut leave: f64 = 1.0;
            let mut clip = |p: f64, q: f64| -> bool {
                if p == 0.0 {
                    return q >= 0.0; // parallel to this edge: inside it or not at all
                }
                let t = q / p;
                if p < 0.0 {
                    enter = cpp::max(enter, t);
                } else {
                    leave = cpp::min(leave, t);
                }
                enter <= leave
            };
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            if !clip(-dx, a.x - bounds.min.x)
                || !clip(dx, bounds.max.x - a.x)
                || !clip(-dy, a.y - bounds.min.y)
                || !clip(dy, bounds.max.y - a.y)
            {
                continue;
            }
            let span = self.starts[i + 1] - self.starts[i];
            let from = self.starts[i] + enter * span;
            let to = self.starts[i] + leave * span;
            if let Some(last) = spans.last_mut()
                && from <= last.1 + SHORTEST_PERIOD
            {
                last.1 = cpp::max(last.1, to);
            } else {
                spans.push((from, to));
            }
        }
        spans
    }
}

// One local point put on the line: `start` along, then the local x further
// along and the local y to the left of wherever that lands. The flag says
// whether that distance was on the line at all.
fn on_path(path: &Path, start: f64, local: Point2, scale: f64) -> (Point2, bool) {
    let along = start + local.x * scale;
    // Both ends inclusive, and a distance that is not a number is outside, as
    // the C++'s two comparisons have it.
    let inside = (-SHORTEST_PERIOD..=path.length + SHORTEST_PERIOD).contains(&along);
    let frame = path.at(cpp::clamp(along, 0.0, path.length));
    let offset = local.y * scale;
    (
        Point2::new(
            frame.at.x - frame.along.y * offset,
            frame.at.y + frame.along.x * offset,
        ),
        inside,
    )
}

// For the placements that put a definition down whole - a symbol, a two-point
// style - where there is no line to clip against.
fn append_runs(into: &mut StyleDrawing, flat: &FlatDefinition, map: &impl Fn(Point2) -> Point2) {
    for run in &flat.runs {
        let mut stroke = StyleStroke {
            path: Polyline2::new(Vec::with_capacity(run.points.len()), run.closed),
            pen: run.pen.clone(),
        };
        for &point in &run.points {
            stroke.path.vertices.push(map(point));
        }
        into.strokes.push(stroke);
    }
}

fn text_mark(text: &FlatDefinitionText, at: Point2, scale: f64, angle: f64) -> StyleTextMark {
    StyleTextMark {
        at,
        text: text.text.text.clone(),
        height: text.text.height * scale,
        angle: angle + text.text.angle * PI / 180.0,
        justify: text.text.justify.clone(),
        font: text.text.font.clone(),
        width_factor: text.text.width_factor,
        pen: text.pen.clone(),
    }
}

fn reach(flat: &mut FlatDefinition, point: Point2) {
    flat.lowest_x = if flat.any_point {
        cpp::min(flat.lowest_x, point.x)
    } else {
        point.x
    };
    flat.highest_x = if flat.any_point {
        cpp::max(flat.highest_x, point.x)
    } else {
        point.x
    };
    flat.any_point = true;
}

fn across(flat: &mut FlatDefinition, y: f64) {
    flat.reach_across = cpp::max(flat.reach_across, y);
}

/// The definition's strokes, flattened into runs of connected points. A Move
/// starts a new run; a Draw extends it; a circle, an arc and a dot are runs of
/// their own about the current point.
pub fn flatten_definition(style: &LineStyle) -> FlatDefinition {
    let mut flat = FlatDefinition {
        units: style.units,
        at_vertices: style.at_vertices,
        length: style.length,
        bounds: style.bounds(),
        ..FlatDefinition::default()
    };
    let factor = style.factor;
    let place = |p: Point2| {
        Point2::new(
            (p.x - style.origin.x) * factor,
            (p.y - style.origin.y) * factor,
        )
    };
    flat.anchor1 = place(style.anchor1);
    flat.anchor2 = place(style.anchor2);

    let mut pen = Point2::default(); // the current point, in the definition's coordinates
    let mut ink = String::new(); // the colour in force
    // Whether the last run of `flat.runs` is the one a Draw extends.
    let mut in_run = false;
    for stroke in &style.strokes {
        match stroke.op {
            StrokeOp::Pen => {
                // "view_colour" means "whatever the entity is", which is the
                // empty pen here.
                ink = if stroke.pen == "view_colour" {
                    String::new()
                } else {
                    stroke.pen.clone()
                };
                in_run = false; // a colour change ends the run it interrupts
            }
            StrokeOp::Move => {
                pen = stroke.point;
                reach(&mut flat, place(pen));
                in_run = false;
            }
            StrokeOp::Draw => {
                if !in_run {
                    flat.runs.push(FlatDefinitionRun {
                        points: vec![place(pen)],
                        closed: false,
                        pen: ink.clone(),
                    });
                    in_run = true;
                    across(&mut flat, place(pen).y.abs());
                }
                pen = stroke.point;
                if let Some(run) = flat.runs.last_mut() {
                    run.points.push(place(pen));
                }
                reach(&mut flat, place(pen));
                across(&mut flat, place(pen).y.abs());
            }
            StrokeOp::Circle => {
                let mut ring = FlatDefinitionRun {
                    points: Vec::new(),
                    closed: true,
                    pen: ink.clone(),
                };
                let centre = place(pen);
                // A LENGTH: see the arc below for what the sign is not.
                let radius = stroke.radius.abs() * factor;
                reach(&mut flat, Point2::new(centre.x - radius, centre.y));
                reach(&mut flat, Point2::new(centre.x + radius, centre.y));
                across(&mut flat, centre.y.abs() + radius.abs());
                for i in 0..CIRCLE_CHORDS {
                    let angle = 2.0 * PI * f64::from(i) / f64::from(CIRCLE_CHORDS);
                    ring.points.push(Point2::new(
                        centre.x + radius * angle.cos(),
                        centre.y + radius * angle.sin(),
                    ));
                }
                flat.runs.push(ring);
                in_run = false;
            }
            StrokeOp::Arc => {
                let mut arc = FlatDefinitionRun {
                    points: Vec::new(),
                    closed: false,
                    pen: ink.clone(),
                };
                // The radius is a LENGTH. Libraries write some arcs with a
                // negative one (45 of them, in the U-turn and speed-zone road
                // markings), and the sign is not a side: the angles alone say
                // where the arc runs, and the moves written either side of
                // each such arc land on the ends |r| gives. The signed radius
                // drew every one turned half a turn about its centre.
                let radius = stroke.radius.abs() * factor;
                add_arc(
                    &mut arc.points,
                    place(pen),
                    radius,
                    stroke.start_angle,
                    stroke.end_angle,
                );
                across(&mut flat, place(pen).y.abs() + radius.abs());
                flat.runs.push(arc);
                in_run = false;
            }
            StrokeOp::Dot => {
                // A dot is a mark with no size of its own in the file - every
                // one of the 179 in the two libraries is `dot 0`. It comes out
                // as a one-point run, which is what the renderer already draws
                // a point mark for.
                flat.runs.push(FlatDefinitionRun {
                    points: vec![place(pen)],
                    closed: false,
                    pen: ink.clone(),
                });
                across(&mut flat, place(pen).y.abs());
                in_run = false;
            }
            StrokeOp::Text => {
                if stroke.text < style.texts.len() {
                    let text = &style.texts[stroke.text];
                    flat.texts.push(FlatDefinitionText {
                        at: place(pen),
                        text: text.clone(),
                        pen: ink.clone(),
                    });
                    // A text's extent needs a font and is not known here; its
                    // height times its length is a bound no real font
                    // exceeds.
                    across(
                        &mut flat,
                        place(pen).y.abs()
                            + (text.height * factor).abs() * text.text.len().max(1) as f64,
                    );
                }
                in_run = false;
            }
        }
    }
    // Measured from what was made rather than kept up in each case above, so
    // a mark added to the flattening later is counted without anyone having
    // to remember to.
    let mut drawn_low = 0.0;
    let mut drawn_high = 0.0;
    let mut any_drawn = false;
    let mut drawn = |x: f64| {
        drawn_low = if any_drawn { cpp::min(drawn_low, x) } else { x };
        drawn_high = if any_drawn {
            cpp::max(drawn_high, x)
        } else {
            x
        };
        any_drawn = true;
    };
    for one in &flat.runs {
        for point in &one.points {
            drawn(point.x);
        }
    }
    for text in &flat.texts {
        drawn(text.at.x);
    }
    flat.drawn_low_x = drawn_low;
    flat.drawn_high_x = drawn_high;
    flat.any_drawn = any_drawn;
    flat
}

/// A definition placed at a point. `size` is the width the definition is
/// scaled to - a library symbol's `size`, which is a WIDTH the way
/// Style::symbol_size is - or 0 to use the definition at its own scale.
/// `rotation` is radians counter-clockwise. (The C++ defaults: size 0,
/// rotation 0, paper_scale 1.)
pub fn symbol_drawing(
    flat: &FlatDefinition,
    at: Point2,
    size: f64,
    rotation: f64,
    paper_scale: f64,
) -> StyleDrawing {
    let mut drawing = StyleDrawing::default();
    let mut scale = unit_scale(flat.units, paper_scale);
    if size > 0.0 {
        // `size` is a WIDTH, as a library's is and as Style::symbol_size is, so
        // the definition is scaled to span it. A definition with no width - a
        // single vertical stroke - keeps its own scale rather than being
        // divided by zero.
        let width = cpp::max(
            flat.bounds.max.x - flat.bounds.min.x,
            flat.bounds.max.y - flat.bounds.min.y,
        );
        if width > SHORTEST_PERIOD {
            scale = size / width;
        }
    }
    let cosine = rotation.cos();
    let sine = rotation.sin();
    let map = |point: Point2| {
        let x = point.x * scale;
        let y = point.y * scale;
        Point2::new(at.x + x * cosine - y * sine, at.y + x * sine + y * cosine)
    };
    append_runs(&mut drawing, flat, &map);
    for text in &flat.texts {
        drawing
            .texts
            .push(text_mark(text, map(text.at), scale, rotation));
    }
    drawing
}

/// symbol_drawing of a LineStyle, flattened for this call.
pub fn symbol_drawing_of_style(
    style: &LineStyle,
    at: Point2,
    size: f64,
    rotation: f64,
    paper_scale: f64,
) -> StyleDrawing {
    symbol_drawing(&flatten_definition(style), at, size, rotation, paper_scale)
}

/// A definition drawn between two points - what a `twoptstyle` is for. The
/// definition's two anchors are mapped onto `from` and `to`, so the strokes
/// stretch to fill the span.
pub fn two_point_drawing(
    flat: &FlatDefinition,
    from: Point2,
    to: Point2,
    paper_scale: f64,
) -> StyleDrawing {
    let mut drawing = StyleDrawing::default();
    let a = flat.anchor1;
    let b = flat.anchor2;
    let span = b - a;
    let span_length = span.x.hypot(span.y);
    let target = to - from;
    let target_length = target.x.hypot(target.y);
    if span_length <= SHORTEST_PERIOD || target_length <= SHORTEST_PERIOD {
        // Nothing to map onto: fall back to placing it as a symbol, which at
        // least draws the definition rather than nothing.
        return symbol_drawing(flat, from, 0.0, 0.0, paper_scale);
    }
    // The similarity that carries a -> from and b -> to. A similarity rather
    // than an independent scale per axis, so the definition is not sheared: a
    // stretched doorway is still a doorway, a sheared one is a mistake.
    let scale = target_length / span_length;
    let turn = target.y.atan2(target.x) - span.y.atan2(span.x);
    let cosine = turn.cos() * scale;
    let sine = turn.sin() * scale;
    let map = |point: Point2| {
        let x = point.x - a.x;
        let y = point.y - a.y;
        Point2::new(
            from.x + x * cosine - y * sine,
            from.y + x * sine + y * cosine,
        )
    };
    append_runs(&mut drawing, flat, &map);
    for text in &flat.texts {
        drawing
            .texts
            .push(text_mark(text, map(text.at), scale, turn));
    }
    drawing
}

/// two_point_drawing of a LineStyle, flattened for this call.
pub fn two_point_drawing_of_style(
    style: &LineStyle,
    from: Point2,
    to: Point2,
    paper_scale: f64,
) -> StyleDrawing {
    two_point_drawing(&flatten_definition(style), from, to, paper_scale)
}

/// The definition repeated along the part of `line` the options allow; see
/// LinestyleOptions and LinestyleLayoutOutcome.
#[allow(
    clippy::manual_range_contains,
    reason = "a text at a distance that is not a number is laid, as the C++'s two comparisons let it be; a range's contains would drop it"
)]
pub fn lay_linestyle(
    flat: &FlatDefinition,
    line: &Polyline2,
    options: &LinestyleOptions,
) -> LinestyleLayout {
    let mut layout = LinestyleLayout::default();
    let path = Path::new(line);
    if !path.usable() {
        layout.outcome = LinestyleLayoutOutcome::NothingToLay;
        return layout;
    }
    let scale = unit_scale(flat.units, options.paper_scale);
    let mut period = (if flat.length > 0.0 {
        flat.length
    } else {
        flat.natural_period()
    }) * scale;
    let repeats = period > SHORTEST_PERIOD;
    if !repeats {
        // A definition with no length and no extent along the line - a single
        // vertical tick - is drawn once at the start rather than endlessly.
        period = path.length + 1.0;
    } else if options.view_scale > 0.0
        && period * options.view_scale < options.minimum_period_pixels
    {
        layout.outcome = LinestyleLayoutOutcome::TooFine;
        return layout;
    }
    // Counted in doubles: a millimetre pattern along a 30 km traverse is
    // thirty million repeats, and the count must be compared with the budget
    // before anything is laid rather than overflow on the way.
    let last = (path.length / period).floor();
    let mut repeats_laid: Vec<(f64, f64)> = Vec::new(); // first and last repeat, inclusive
    if let Some(visible) = options.visible {
        // Repeat k covers [k p + low, k p + high] along the line, so it can
        // reach the stretch [a, b] only for k in
        // [ceil((a - high) / p), floor((b - low) / p)].
        //
        // low and high take in the MARKS as well as the pen: an arc reaches a
        // radius past the move to its centre. The margin's extra period does
        // not cover that at the end of the line, where b is the line's end
        // and not the view's, so a pen-only range dropped the last repeat of
        // a scallop that the unclipped laying (the previews) drew.
        let margin = flat.reach_across * scale + if repeats { period } else { 0.0 };
        let mut low = flat.lowest_x;
        let mut high = flat.highest_x;
        if flat.any_drawn {
            low = if flat.any_point {
                cpp::min(low, flat.drawn_low_x)
            } else {
                flat.drawn_low_x
            };
            high = if flat.any_point {
                cpp::max(high, flat.drawn_high_x)
            } else {
                flat.drawn_high_x
            };
        }
        low *= scale;
        high *= scale;
        for (a, b) in path.inside(&visible.inflated(margin)) {
            let mut first = cpp::max(0.0, ((a - high) / period).ceil());
            let last_here = cpp::min(last, ((b - low) / period).floor());
            if let Some(&(_, laid_last)) = repeats_laid.last() {
                first = cpp::max(first, laid_last + 1.0);
            }
            if first <= last_here {
                repeats_laid.push((first, last_here));
            }
        }
    } else {
        repeats_laid.push((0.0, last));
    }
    let mut count = 0.0;
    for &(first, last_here) in &repeats_laid {
        count += last_here - first + 1.0;
    }
    if count > options.maximum_instances as f64 {
        layout.outcome = LinestyleLayoutOutcome::OverBudget;
        return layout;
    }

    let drawing = &mut layout.drawing;
    let mut lay = |start: f64| {
        // Every run clipped to the line: a point past the end is pulled back
        // to it, and a run that lies wholly outside is not drawn at all.
        for run in &flat.runs {
            let mut stroke = StyleStroke {
                path: Polyline2::new(Vec::with_capacity(run.points.len()), run.closed),
                pen: run.pen.clone(),
            };
            let mut any_inside = false;
            for &point in &run.points {
                let (placed, inside) = on_path(&path, start, point, scale);
                stroke.path.vertices.push(placed);
                any_inside = any_inside || inside;
            }
            if any_inside {
                drawing.strokes.push(stroke);
            }
        }
        for text in &flat.texts {
            let along = start + text.at.x * scale;
            if along < 0.0 || along > path.length {
                continue; // the whole word would sit off the end of the line
            }
            // Along the line where it sits, plus whatever the file asks for.
            let frame = path.at(along);
            let (placed, _) = on_path(&path, start, text.at, scale);
            drawing.texts.push(text_mark(
                text,
                placed,
                scale,
                frame.along.y.atan2(frame.along.x),
            ));
        }
    };
    for &(first, last_here) in &repeats_laid {
        let mut k = first;
        while k <= last_here {
            lay(k * period);
            k += 1.0;
        }
    }
    layout.outcome = LinestyleLayoutOutcome::Laid;
    layout.instances = count as usize;
    layout
}

/// A definition repeated along the whole of a line. The pattern's period is
/// the definition's `length`, or the span of its own strokes when it says
/// nothing.
///
/// Each point is placed by ITS OWN distance along the line rather than by
/// rigidly transforming a whole instance, so the pattern bends with the line
/// instead of leaving it at a corner. On a straight run the two are identical.
///
/// Empty - never truncated - when the line would need more repeats than
/// LinestyleOptions' default budget; lay_linestyle says why.
pub fn linestyle_drawing(style: &LineStyle, line: &Polyline2, paper_scale: f64) -> StyleDrawing {
    let options = LinestyleOptions {
        paper_scale,
        ..LinestyleOptions::default()
    };
    lay_linestyle(&flatten_definition(style), line, &options).drawing
}

/// Whichever of the three the definition asks for: a symbol at EVERY vertex, a
/// two-point style across the whole line, or a linestyle along it. This is
/// what a caller drawing an entity wants, so that the definition decides
/// rather than the caller guessing. The outcome is lay_linestyle's for a
/// linestyle and Laid (or NothingToLay for an empty line) for the other two.
pub fn style_drawing(
    flat: &FlatDefinition,
    line: &Polyline2,
    options: &LinestyleOptions,
) -> LinestyleLayout {
    let mut layout = LinestyleLayout::default();
    if line.vertices.is_empty() {
        layout.outcome = LinestyleLayoutOutcome::NothingToLay;
        return layout;
    }
    if flat.at_vertices {
        for &vertex in &line.vertices {
            let one = symbol_drawing(flat, vertex, 0.0, 0.0, options.paper_scale);
            layout.drawing.strokes.extend(one.strokes);
            layout.drawing.texts.extend(one.texts);
        }
        layout.outcome = LinestyleLayoutOutcome::Laid;
        layout.instances = line.vertices.len();
        return layout;
    }
    if flat.units == StyleUnits::TwoPoint {
        layout.drawing = two_point_drawing(
            flat,
            line.vertices[0],
            line.vertices[line.vertices.len() - 1],
            options.paper_scale,
        );
        layout.outcome = LinestyleLayoutOutcome::Laid;
        layout.instances = 1;
        return layout;
    }
    lay_linestyle(flat, line, options)
}

/// style_drawing of a LineStyle, flattened for this call, along the whole of
/// the line at the default budget.
pub fn style_drawing_of_style(
    style: &LineStyle,
    line: &Polyline2,
    paper_scale: f64,
) -> StyleDrawing {
    let options = LinestyleOptions {
        paper_scale,
        ..LinestyleOptions::default()
    };
    style_drawing(&flatten_definition(style), line, &options).drawing
}
