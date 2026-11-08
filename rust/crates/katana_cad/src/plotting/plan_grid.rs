//! A plan's coordinate grid, worked out on paper (docs/plotting.md, "The
//! coordinate grid and the live key plan";
//! include/katana/cad/plotting/plan_grid.hpp).
//!
//! A plan viewport shows the ground at a scale, centred on a world point and
//! turned by its rotation (sheet_set.rs). Its grid is the ground's: eastings
//! and northings every so many metres, so on a rotated plan it runs askew
//! across the paper. It is drawn in one of three styles (GridStyle) and
//! labelled where its lines meet the viewport's edges - "E 305 200",
//! "N 6 250 400" - each label running along its edge.
//!
//! Everything here is geometry in paper millimetres (frame.rs: from the
//! paper's bottom-left corner, Y up): which lines, crosses and ticks, and
//! where each label goes. The painter strokes the segments and sets the
//! labels it is given, so the grid is tested without pixels and plots the
//! same from every caller.

use std::cmp::Ordering;

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, format_general};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::document::Document;
use crate::plotting::frame::{HorizontalJustify, VerticalJustify};
use crate::plotting::sheet_commands::edit_viewport;
use crate::plotting::sheet_set::{GridStyle, Viewport, ViewportKind, std_fixed};

const GRID_STYLES: [(&str, GridStyle); 4] = [
    ("none", GridStyle::None),
    ("ticks", GridStyle::Ticks),
    ("crosses", GridStyle::Crosses),
    ("lines", GridStyle::Lines),
];

/// "none", "ticks", "crosses", "lines": the names the JSON and a command line
/// use (the C++ `toString(GridStyle)`).
impl EnumName for GridStyle {
    fn name(self) -> &'static str {
        for (name, value) in GRID_STYLES {
            if value == self {
                return name;
            }
        }
        "none"
    }
}

pub fn grid_style_from(name: &str) -> Option<GridStyle> {
    GRID_STYLES
        .iter()
        .find(|(text, _)| *text == name)
        .map(|(_, value)| *value)
}

// More lines than this on one axis is not a grid anybody reads; it is refused
// with the spacing message long before a real sheet gets near it (an A0 sheet
// at the closest spacing has about 600).
const MAXIMUM_LINES_PER_AXIS: f64 = 5000.0;

// ---- a plan viewport's ground ----------------------------------------------------

/// Where a plan viewport's ground is once "auto" is decided: its scale, and the
/// world point at its rectangle's centre. The painter decides an automatic
/// viewport's (resolvePlanViewport in the window); a fixed one's are its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanPlacement {
    pub scale: f64,
    pub centre: Point2,
}

impl Default for PlanPlacement {
    fn default() -> Self {
        Self {
            scale: 500.0,
            centre: Point2::default(),
        }
    }
}

/// The viewport's own scale and centre, as stored.
pub fn stored_placement(viewport: &Viewport) -> PlanPlacement {
    PlanPlacement {
        scale: viewport.scale,
        centre: viewport.centre,
    }
}

fn turned(v: Point2, radians: f64) -> Point2 {
    let c = radians.cos();
    let s = radians.sin();
    Point2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Paper to world for a plan viewport placed at `at`: the rectangle's centre
/// is at.centre, a paper millimetre is at.scale / 1000 metres, and the paper's
/// +X runs along the world direction viewport.rotation.
pub fn plan_paper_to_world(viewport: &Viewport, at: &PlanPlacement, paper: Point2) -> Point2 {
    let offset = (paper - viewport.rect.center()) * (at.scale / 1000.0);
    at.centre + turned(offset, viewport.rotation)
}

/// And world to paper.
pub fn plan_world_to_paper(viewport: &Viewport, at: &PlanPlacement, world: Point2) -> Point2 {
    let offset = turned(world - at.centre, -viewport.rotation);
    viewport.rect.center() + offset * (1000.0 / at.scale)
}

/// The ground under the viewport's rectangle: the world points under its four
/// corners, counter-clockwise from the one under its bottom-left. Empty for an
/// unplaced viewport or a scale that is not positive and finite.
pub fn plan_footprint(viewport: &Viewport, at: &PlanPlacement) -> Vec<Point2> {
    if viewport.rect.empty() || !at.scale.is_finite() || !(at.scale > 0.0) {
        return Vec::new();
    }
    let r = &viewport.rect;
    vec![
        plan_paper_to_world(viewport, at, r.min),
        plan_paper_to_world(viewport, at, Point2::new(r.max.x, r.min.y)),
        plan_paper_to_world(viewport, at, r.max),
        plan_paper_to_world(viewport, at, Point2::new(r.min.x, r.max.y)),
    ]
}

// ---- the grid ------------------------------------------------------------------------

/// What an automatic grid (Viewport::grid_interval 0) is spaced at on the
/// paper, roughly: close enough to find a coordinate by eye, far enough apart
/// to leave the drawing readable.
pub const GRID_TARGET_SPACING_MM: f64 = 50.0;
/// The closest a grid may be drawn: a finer one is refused rather than painted
/// as a grey smear (and, at a tiny interval, as millions of lines).
pub const GRID_MINIMUM_SPACING_MM: f64 = 2.0;

/// The spacing of an automatic grid at 1:`scale`: of 1, 2 and 5 times a power
/// of ten metres, the one whose lines fall nearest GRID_TARGET_SPACING_MM apart
/// on the paper (nearest by ratio) - 20 m at 1:500, 50 m at 1:1000, 100 m at
/// 1:2000. 0 for a scale that is not positive and finite.
pub fn automatic_grid_interval(scale: f64) -> f64 {
    if !scale.is_finite() || !(scale > 0.0) {
        return 0.0;
    }
    // The spacing in metres that would fall exactly on the target, and the
    // round values either side of it. A power of ten below 1 divides rather
    // than multiplies, so 0.05 m is the double nearest 0.05, as a label and a
    // stored interval write it.
    let target = GRID_TARGET_SPACING_MM * scale / 1000.0;
    let exponent = target.log10().floor();
    let power = 10.0_f64.powf(exponent.abs());
    let step_of = |multiple: f64| {
        if exponent < 0.0 {
            multiple / power
        } else {
            multiple * power
        }
    };
    let mut best = step_of(1.0);
    let mut best_distance = f64::INFINITY;
    for multiple in [1.0, 2.0, 5.0, 10.0] {
        let candidate = step_of(multiple);
        let distance = (candidate / target).ln().abs();
        if distance < best_distance - 1e-12 {
            best = candidate;
            best_distance = distance;
        }
    }
    best
}

/// How many decimals a coordinate on a grid of `interval` metres needs: none
/// for whole metres, 1 for 0.5 or 2.5, 2 for 0.25 or 0.05, at most 3.
pub fn grid_decimals(interval: f64) -> i32 {
    if !interval.is_finite() || !(interval > 0.0) {
        return 0;
    }
    for decimals in 0..3 {
        // std::pow(double, int) is pow on two doubles.
        let scaled = interval * 10.0_f64.powf(f64::from(decimals));
        if (scaled - scaled.round()).abs() < 1e-6 * cpp::max(1.0, scaled) {
            return decimals;
        }
    }
    3
}

/// A coordinate as a grid label writes it: thousands grouped by a space, with
/// `decimals` places - "305 200", "6 250 400.5", "-1 200". A value that rounds
/// to zero prints "0", never "-0".
pub fn grouped_coordinate(value: f64, decimals: i32) -> String {
    let decimals = decimals.clamp(0, 6);
    let mut text = std_fixed(value, decimals as usize);
    let mut negative = false;
    if text.starts_with('-') {
        negative = text.bytes().any(|byte| !matches!(byte, b'-' | b'0' | b'.'));
        text.remove(0);
    }
    let (whole, fraction) = match text.find('.') {
        Some(point) => text.split_at(point),
        None => (text.as_str(), ""),
    };
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(' ');
        }
        grouped.push(digit);
    }
    format!("{}{grouped}{fraction}", if negative { "-" } else { "" })
}

/// A line of constant easting runs north-south; one of constant northing runs
/// east-west.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridAxis {
    #[default]
    Easting,
    Northing,
}

/// The side of a viewport a line leaves by, and a label stands on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewportEdge {
    #[default]
    Bottom,
    Right,
    Top,
    Left,
}

/// A grid line across the viewport, on the paper: both ends on its border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLine {
    pub axis: GridAxis,
    /// Its easting or northing, metres.
    pub value: f64,
    pub from: Point2,
    pub to: Point2,
    pub from_edge: ViewportEdge,
    pub to_edge: ViewportEdge,
}

impl Default for GridLine {
    fn default() -> Self {
        Self {
            axis: GridAxis::Easting,
            value: 0.0,
            from: Point2::default(),
            to: Point2::default(),
            from_edge: ViewportEdge::Bottom,
            to_edge: ViewportEdge::Top,
        }
    }
}

/// A straight stroke on the paper.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridSegment {
    pub from: Point2,
    pub to: Point2,
}

/// A label where a grid line meets an edge, as a frame text is set (frame.rs):
/// an anchor, a justification and an angle - 0 along the bottom and top, 90
/// (reading up) along the sides - at PlanGridOptions::label_cap_mm.
#[derive(Clone, Debug, PartialEq)]
pub struct GridLabel {
    pub axis: GridAxis,
    pub value: f64,
    /// "E 305 200", "N 6 250 400".
    pub text: String,
    pub edge: ViewportEdge,
    pub anchor: Point2,
    pub angle_degrees: f64,
    pub horizontal: HorizontalJustify,
    pub vertical: VerticalJustify,
    /// The paper the label covers with its white knock-out: what it keeps off
    /// the other labels, the viewport's furniture and the edges. (The C++'s
    /// `box`, a keyword here.)
    pub box_value: Box2,
}

impl Default for GridLabel {
    fn default() -> Self {
        Self {
            axis: GridAxis::Easting,
            value: 0.0,
            text: String::new(),
            edge: ViewportEdge::Bottom,
            anchor: Point2::default(),
            angle_degrees: 0.0,
            horizontal: HorizontalJustify::Centre,
            vertical: VerticalJustify::Bottom,
            box_value: Box2::default(),
        }
    }
}

/// How wide a label is set, in paper millimetres.
pub type LabelWidth<'a> = &'a dyn Fn(&str) -> f64;

#[derive(Clone)]
pub struct PlanGridOptions<'a> {
    /// The labels' cap height.
    pub label_cap_mm: f64,
    /// Ticks: how far each tick comes in from the border.
    pub tick_mm: f64,
    /// Crosses: the width of each cross.
    pub cross_mm: f64,
    /// From the border, or a tick's end, to a label.
    pub label_gap_mm: f64,
    /// The white around a label.
    pub knock_out_mm: f64,
    /// Paper a label must keep off: the view's title, its scale bar and its
    /// north arrow (the painter passes the boxes they knock out).
    pub keep_out: Vec<Box2>,
    /// How wide a label is set, in paper millimetres. None: estimated at
    /// 0.75 x the cap height a character, which is Arial's figures.
    pub label_width_mm: Option<LabelWidth<'a>>,
    /// The least paper between two labels' knock-outs: two labels closer than
    /// this read as one ("E 305 240 N 6 250 480"), so the later is left out.
    pub label_spacing_mm: f64,
}

impl Default for PlanGridOptions<'_> {
    fn default() -> Self {
        Self {
            label_cap_mm: 1.8,
            tick_mm: 2.5,
            cross_mm: 3.0,
            label_gap_mm: 0.8,
            knock_out_mm: 0.4,
            keep_out: Vec::new(),
            label_width_mm: None,
            label_spacing_mm: 1.5,
        }
    }
}

/// What the painter draws for a plan's grid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanGrid {
    pub style: GridStyle,
    /// The spacing drawn, metres.
    pub interval: f64,
    /// In the labels.
    pub decimals: i32,
    /// Every grid line crossing the viewport, whatever the style: eastings west
    /// to east, then northings south to north.
    pub lines: Vec<GridLine>,
    /// What is stroked: Lines the lines themselves; Crosses the two arms of
    /// each cross, along the grid; Ticks one tick in from each end of each line.
    pub strokes: Vec<GridSegment>,
    /// Crosses: the intersections marked, each a whole cross inside the
    /// viewport.
    pub crossings: Vec<Point2>,
    /// The labels that fit: inside the viewport, clear of each other and of
    /// PlanGridOptions::keep_out, taken edge by edge - bottom, left, top,
    /// right - and along each edge in order, so the same grid always keeps the
    /// same ones.
    pub labels: Vec<GridLabel>,
}

// The part of the infinite line through `origin` along `direction` that lies
// inside `rect`, and the edges its ends are on (Liang-Barsky). Nothing when
// the line misses the rectangle, only touches a corner, or lies along an
// edge - that line is the border itself.
struct Clipped {
    from: Point2,
    to: Point2,
    from_edge: ViewportEdge,
    to_edge: ViewportEdge,
}

struct Slabs {
    t0: f64,
    t1: f64,
    e0: ViewportEdge,
    e1: ViewportEdge,
}

const PARALLEL: f64 = 1e-12;
const ON_EDGE_MM: f64 = 1e-9;

fn slab(
    slabs: &mut Slabs,
    p: f64,
    d: f64,
    lo: f64,
    hi: f64,
    lo_edge: ViewportEdge,
    hi_edge: ViewportEdge,
) -> bool {
    if d.abs() < PARALLEL {
        return p > lo + ON_EDGE_MM && p < hi - ON_EDGE_MM;
    }
    let mut ta = (lo - p) / d;
    let mut tb = (hi - p) / d;
    let mut ea = lo_edge;
    let mut eb = hi_edge;
    if ta > tb {
        std::mem::swap(&mut ta, &mut tb);
        std::mem::swap(&mut ea, &mut eb);
    }
    if ta > slabs.t0 {
        slabs.t0 = ta;
        slabs.e0 = ea;
    }
    if tb < slabs.t1 {
        slabs.t1 = tb;
        slabs.e1 = eb;
    }
    true
}

fn clip_line(origin: Point2, direction: Point2, rect: &Box2) -> Option<Clipped> {
    let mut slabs = Slabs {
        t0: f64::NEG_INFINITY,
        t1: f64::INFINITY,
        e0: ViewportEdge::Bottom,
        e1: ViewportEdge::Top,
    };
    if !slab(
        &mut slabs,
        origin.x,
        direction.x,
        rect.min.x,
        rect.max.x,
        ViewportEdge::Left,
        ViewportEdge::Right,
    ) || !slab(
        &mut slabs,
        origin.y,
        direction.y,
        rect.min.y,
        rect.max.y,
        ViewportEdge::Bottom,
        ViewportEdge::Top,
    ) || !(slabs.t1 - slabs.t0 > ON_EDGE_MM)
    {
        return None;
    }
    Some(Clipped {
        from: origin + direction * slabs.t0,
        to: origin + direction * slabs.t1,
        from_edge: slabs.e0,
        to_edge: slabs.e1,
    })
}

// "E 305 200", "N 6 250 400".
fn label_text(line: &GridLine, decimals: i32) -> String {
    format!(
        "{}{}",
        if line.axis == GridAxis::Easting {
            "E "
        } else {
            "N "
        },
        grouped_coordinate(line.value, decimals)
    )
}

// The label `text`, `width_mm` wide, for the line `line` where it meets `edge`
// at `at`: set along the edge, `inset` in from it, centred on the line.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ labelAt's arguments, one for one."
)]
fn label_at(
    line: &GridLine,
    text: String,
    width_mm: f64,
    edge: ViewportEdge,
    at: Point2,
    rect: &Box2,
    inset: f64,
    options: &PlanGridOptions<'_>,
) -> GridLabel {
    let cap = options.label_cap_mm;
    let k = options.knock_out_mm;
    let half = width_mm / 2.0;
    let (anchor, angle_degrees, vertical, box_value) = match edge {
        ViewportEdge::Bottom => {
            let anchor = Point2::new(at.x, rect.min.y + inset);
            (
                anchor,
                0.0,
                VerticalJustify::Bottom,
                Box2::new(
                    Point2::new(at.x - half - k, anchor.y - k),
                    Point2::new(at.x + half + k, anchor.y + cap + k),
                ),
            )
        }
        ViewportEdge::Top => {
            let anchor = Point2::new(at.x, rect.max.y - inset);
            (
                anchor,
                0.0,
                VerticalJustify::Top,
                Box2::new(
                    Point2::new(at.x - half - k, anchor.y - cap - k),
                    Point2::new(at.x + half + k, anchor.y + k),
                ),
            )
        }
        // Up the sides the text reads from the bottom, its head to the left:
        // on the left edge the head is at the border and the text hangs inward
        // from the anchor, on the right edge the text stands inward on it.
        ViewportEdge::Left => {
            let anchor = Point2::new(rect.min.x + inset, at.y);
            (
                anchor,
                90.0,
                VerticalJustify::Top,
                Box2::new(
                    Point2::new(anchor.x - k, at.y - half - k),
                    Point2::new(anchor.x + cap + k, at.y + half + k),
                ),
            )
        }
        ViewportEdge::Right => {
            let anchor = Point2::new(rect.max.x - inset, at.y);
            (
                anchor,
                90.0,
                VerticalJustify::Bottom,
                Box2::new(
                    Point2::new(anchor.x - cap - k, at.y - half - k),
                    Point2::new(anchor.x + k, at.y + half + k),
                ),
            )
        }
    };
    GridLabel {
        axis: line.axis,
        value: line.value,
        text,
        edge,
        anchor,
        angle_degrees,
        horizontal: HorizontalJustify::Centre,
        vertical,
        box_value,
    }
}

// Where along its edge a label is, for ordering them.
fn along_edge(label: &GridLabel) -> f64 {
    if label.edge == ViewportEdge::Bottom || label.edge == ViewportEdge::Top {
        label.anchor.x
    } else {
        label.anchor.y
    }
}

// The edge order labels are taken in: bottom and left first, where a reader
// looks for coordinates, then top and right.
fn edge_rank(edge: ViewportEdge) -> i32 {
    match edge {
        ViewportEdge::Bottom => 0,
        ViewportEdge::Left => 1,
        ViewportEdge::Top => 2,
        ViewportEdge::Right => 3,
    }
}

fn clear(a: &Box2, b: &Box2) -> bool {
    a.max.x <= b.min.x || b.max.x <= a.min.x || a.max.y <= b.min.y || b.max.y <= a.min.y
}

/// The grid of plan viewport `viewport` placed at `at`, in its own style and at
/// its own interval (automatic_grid_interval when that is 0). GridStyle::None
/// gives an empty grid. InvalidArgument for an unplaced viewport, a scale that
/// is not positive and finite, an interval that is negative or not finite, and
/// an interval whose lines would be closer than GRID_MINIMUM_SPACING_MM on the
/// paper.
pub fn plan_grid(
    viewport: &Viewport,
    at: &PlanPlacement,
    options: &PlanGridOptions<'_>,
) -> Result<PlanGrid> {
    let mut grid = PlanGrid {
        style: viewport.grid_style,
        ..PlanGrid::default()
    };
    if viewport.grid_style == GridStyle::None {
        return Ok(grid);
    }
    let rect = &viewport.rect;
    if rect.empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the viewport is not placed on the paper",
            viewport.id.as_str(),
        ));
    }
    if !at.scale.is_finite()
        || !(at.scale > 0.0)
        || !at.centre.x.is_finite()
        || !at.centre.y.is_finite()
        || !viewport.rotation.is_finite()
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the viewport's scale, centre or rotation is not a finite number",
            viewport.id.as_str(),
        ));
    }
    if !viewport.grid_interval.is_finite() || viewport.grid_interval < 0.0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a grid interval is a distance in metres, 0 for automatic",
            format!(
                "{}: {}",
                viewport.id,
                format_exact_real(viewport.grid_interval)
            ),
        ));
    }
    let interval = if viewport.grid_interval > 0.0 {
        viewport.grid_interval
    } else {
        automatic_grid_interval(at.scale)
    };
    let spacing_mm = interval * 1000.0 / at.scale;
    if !(spacing_mm >= GRID_MINIMUM_SPACING_MM) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "a {} m grid at 1:{} is {} mm apart on the paper; the closest a grid may be is \
                 {} mm - choose a larger interval, or 0 for automatic",
                grouped_coordinate(interval, grid_decimals(interval)),
                format_exact_real(at.scale),
                format_general(spacing_mm, 2),
                format_exact_real(GRID_MINIMUM_SPACING_MM)
            ),
            viewport.id.as_str(),
        ));
    }
    grid.interval = interval;
    grid.decimals = grid_decimals(interval);

    // The round coordinates the ground under the viewport spans, on each axis.
    let mut ground = Box2::default();
    for corner in plan_footprint(viewport, at) {
        ground.expand(corner);
    }
    let multiples = |from: f64, to: f64| ((from / interval).ceil(), (to / interval).floor());
    let (e0, e1) = multiples(ground.min.x, ground.max.x);
    let (n0, n1) = multiples(ground.min.y, ground.max.y);
    if e1 - e0 > MAXIMUM_LINES_PER_AXIS || n1 - n0 > MAXIMUM_LINES_PER_AXIS {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the grid would have too many lines to draw",
            viewport.id.as_str(),
        ));
    }
    // World east and north on the paper.
    let east = turned(Point2::new(1.0, 0.0), -viewport.rotation);
    let north = turned(Point2::new(0.0, 1.0), -viewport.rotation);
    let mut add_line = |axis: GridAxis, value: f64| {
        let world = if axis == GridAxis::Easting {
            Point2::new(value, at.centre.y)
        } else {
            Point2::new(at.centre.x, value)
        };
        let along = if axis == GridAxis::Easting {
            north
        } else {
            east
        };
        if let Some(clipped) = clip_line(plan_world_to_paper(viewport, at, world), along, rect) {
            grid.lines.push(GridLine {
                axis,
                value,
                from: clipped.from,
                to: clipped.to,
                from_edge: clipped.from_edge,
                to_edge: clipped.to_edge,
            });
        }
    };
    let mut k = e0;
    while k <= e1 {
        add_line(GridAxis::Easting, k * interval);
        k += 1.0;
    }
    let mut k = n0;
    while k <= n1 {
        add_line(GridAxis::Northing, k * interval);
        k += 1.0;
    }

    match grid.style {
        GridStyle::None => {}
        GridStyle::Lines => {
            for line in &grid.lines {
                grid.strokes.push(GridSegment {
                    from: line.from,
                    to: line.to,
                });
            }
        }
        GridStyle::Ticks => {
            for line in &grid.lines {
                let d = line.to - line.from;
                let run = d.length();
                let unit = d * (1.0 / run);
                let tick = cpp::min(options.tick_mm, run / 2.0);
                grid.strokes.push(GridSegment {
                    from: line.from,
                    to: line.from + unit * tick,
                });
                grid.strokes.push(GridSegment {
                    from: line.to,
                    to: line.to - unit * tick,
                });
            }
        }
        GridStyle::Crosses => {
            // Each cross whole inside the viewport: one cut by the border reads
            // as a tick that is not there.
            let arm = options.cross_mm / 2.0;
            let inside = Box2::new(
                Point2::new(rect.min.x + arm, rect.min.y + arm),
                Point2::new(rect.max.x - arm, rect.max.y - arm),
            );
            let mut i = e0;
            while i <= e1 {
                let mut j = n0;
                while j <= n1 {
                    let crossing =
                        plan_world_to_paper(viewport, at, Point2::new(i * interval, j * interval));
                    if inside.contains(crossing) {
                        grid.crossings.push(crossing);
                        grid.strokes.push(GridSegment {
                            from: crossing - east * arm,
                            to: crossing + east * arm,
                        });
                        grid.strokes.push(GridSegment {
                            from: crossing - north * arm,
                            to: crossing + north * arm,
                        });
                    }
                    j += 1.0;
                }
                i += 1.0;
            }
        }
    }

    // The labels: one where each line meets each edge, set along the edge and
    // centred on the line - past the tick, for ticks - and kept only where it
    // fits inside the viewport, clear of the furniture and of the labels
    // already placed.
    let inset = if grid.style == GridStyle::Ticks {
        options.tick_mm + options.label_gap_mm
    } else {
        options.label_gap_mm
    };
    let width_of = |text: &str| match options.label_width_mm {
        Some(measure) => measure(text),
        None => 0.75 * options.label_cap_mm * text.len() as f64,
    };
    let mut candidates = Vec::with_capacity(2 * grid.lines.len());
    for line in &grid.lines {
        let text = label_text(line, grid.decimals);
        let width = width_of(&text);
        candidates.push(label_at(
            line,
            text.clone(),
            width,
            line.from_edge,
            line.from,
            rect,
            inset,
            options,
        ));
        candidates.push(label_at(
            line,
            text,
            width,
            line.to_edge,
            line.to,
            rect,
            inset,
            options,
        ));
    }
    // A stable sort, as the C++'s std::stable_sort; two positions neither
    // before the other (a NaN) keep their order.
    candidates.sort_by(|a, b| {
        edge_rank(a.edge).cmp(&edge_rank(b.edge)).then_with(|| {
            along_edge(a)
                .partial_cmp(&along_edge(b))
                .unwrap_or(Ordering::Equal)
        })
    });
    let spacing = cpp::max(options.label_spacing_mm, 0.0);
    for label in candidates {
        if !rect.contains_box(label.box_value) {
            continue;
        }
        let covers = |other: &Box2| !other.empty() && !clear(other, &label.box_value);
        let blocked = options.keep_out.iter().any(covers)
            || grid
                .labels
                .iter()
                .any(|placed| !clear(&placed.box_value.inflated(spacing), &label.box_value));
        if !blocked {
            grid.labels.push(label);
        }
    }
    Ok(grid)
}

/// Sets the grid of the viewport with id `viewport_id` - its style and its
/// interval in metres, 0 for automatic (the C++'s default) - as ONE undoable
/// step. NotFound for no such viewport; InvalidArgument for a viewport that is
/// not a plan or a key plan, and for an interval that is negative or not
/// finite. Setting what it already has records no step.
pub fn set_plan_grid(
    document: &mut Document,
    viewport_id: &str,
    style: GridStyle,
    interval_m: f64,
) -> Status {
    if !interval_m.is_finite() || interval_m < 0.0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a grid interval is a distance in metres, 0 for automatic",
            format_exact_real(interval_m),
        ));
    }
    edit_viewport(
        document,
        viewport_id,
        |viewport| {
            if viewport.kind != ViewportKind::Plan && viewport.kind != ViewportKind::KeyPlan {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "only a plan or a key plan has a coordinate grid",
                    viewport.kind.name(),
                ));
            }
            viewport.grid_style = style;
            viewport.grid_interval = interval_m;
            Ok(())
        },
        "SET_PLAN_GRID",
    )
}
