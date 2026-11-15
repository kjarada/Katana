//! Labels, laid out for one scale with collision avoidance
//! (include/katana/cad/annotation/label_layout.hpp; docs/annotation.md, "The
//! placer").
//!
//! Every label entity says WHAT to label and HOW (its style); where its text
//! goes depends on the scale it is looked at - at 1 : 200 two point numbers
//! are metres apart on the sheet, at 1 : 2000 they would print on top of each
//! other - so placement is worked out per view, here, from the model and the
//! scale, and never stored.
//!
//! The placer is a deterministic greedy candidate placer, the classical
//! cartographic method (Imhof's positions for points, positions slid along
//! lines, the centroid first for areas):
//!
//! ```text
//! 1. Each piece of each label (label_pieces: a point, a segment, an arc, an
//!    area, a chainage mark) gets an ordered list of candidate positions -
//!    its style's preferred place first, then the others the style allows,
//!    then the same further out (DISPLACED, with a leader back to what it
//!    labels when the style asks for one).
//! 2. Pieces are placed in a fixed order: a dragged label first, then by
//!    style priority (higher first), then by label id and piece index. No
//!    container's iteration order and no pointer takes part (Rule 7).
//! 3. Each piece takes its first candidate whose text box overlaps no text
//!    placed before it and crosses no linework it was asked to keep out of.
//!    A piece with no free candidate is SUPPRESSED and counted; a dragged
//!    label is always placed where it was put.
//! ```
//!
//! Overlap is exact for the rotated text boxes (separating axes), found
//! through a uniform grid over their bounding boxes, so a thousand labels
//! cost what their neighbours cost rather than a million pair tests.
//!
//! Headless, and tested headlessly with the estimated measure; the painters
//! pass their font's.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_1_SQRT_2;

use katana_entity::annotation::{
    DEFAULT_ANNOTATION_SCALE, DEFAULT_TEXT_STYLE_NAME, annotation_model_size,
};
use katana_entity::curve_pieces::{curve_pieces, linework};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::label_text::format_label;
use katana_entity::label_values::label_pieces;
use katana_entity::text_block::estimated_width;
use katana_entity::{
    Entity, EntityId, Geometry, LabelGeometry, LabelMarker, LabelOrientation, LabelPiece,
    LabelPieceShape, LabelPlacement, LabelStyle, LeaderContext, Model, TextJustify,
};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::{Arc2, Box2, Curve2, Point2, Segment2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{TWO_PI, tolerance};

use crate::annotation::drawing::{Drawing, TextMeasure, circle_polygon, estimated_measure};
use crate::annotation::leader_draw::{build_leader, code_properties};
use crate::annotation::text_layout::{
    layout_text, layout_text_entity, readable_rotation, resolve_text_appearance,
};
use crate::dimension_draw::{DimensionDrawing, build_dimension, resolve_dimension_style};
use crate::layer_overrides::NO_LAYER_OVERRIDES;
use crate::selection::is_drawn;
use crate::survey_coding::{code_property_candidates, survey_code_of};

// Rings of displaced positions beyond the first: at the style's offset plus
// this many text heights. Two rings, because a third is far enough from its
// feature that a reader no longer connects them even with a leader.
const DISPLACED_RINGS: [f64; 2] = [2.0, 4.0];
// Where along a segment or an arc a label may slide, as a fraction of its
// length, in order of preference: the middle, then outwards.
const SLIDE_POSITIONS: [f64; 5] = [0.5, 0.35, 0.65, 0.2, 0.8];
// The grid cell of the overlap search, in millimetres on paper: about four
// lines of 2.5 mm text, so a label's box meets a handful of cells.
const GRID_CELL_MILLIMETRES: f64 = 10.0;
// How far past the view a label's anchor may be and still have its text
// reach into it, in millimetres on paper.
const REACH_MILLIMETRES: f64 = 30.0;

#[derive(Clone)]
pub struct LabelLayoutOptions {
    /// The annotation scale, 1 : scale.
    pub scale: f64,
    pub measure: TextMeasure,
    /// Only pieces whose anchor lies in this box (grown by the reach of a
    /// label) are laid out; empty lays out everything. A view passes what it
    /// shows, so zoomed in it places what it can see.
    pub visible: Box2,
    /// Linework the text keeps out of: the painter passes the lines of the
    /// entities in view. Empty keeps text out of other text only.
    pub linework: Vec<Segment2>,
    /// The model direction of the sheet's horizontal, radians: minus a
    /// viewport's turn (PlanFrame::rotation), so a point label turned with its
    /// viewport still reads level on the sheet.
    pub horizontal: f64,
    /// No collision avoidance at all: every piece at its first candidate.
    /// What a plot of a drawing whose labels were arranged by hand wants, and
    /// what the tests compare the placer against.
    pub avoid_collisions: bool,
}

impl Default for LabelLayoutOptions {
    fn default() -> Self {
        Self {
            scale: DEFAULT_ANNOTATION_SCALE,
            measure: estimated_measure(),
            visible: Box2::default(),
            linework: Vec::new(),
            horizontal: 0.0,
            avoid_collisions: true,
        }
    }
}

/// One piece of a label, laid out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlacedLabel {
    pub label: EntityId,
    pub piece: usize,
    /// The text and whatever goes with it: a leader when displaced, a point's
    /// marker, a chainage tick.
    pub drawing: Drawing,
    /// Which candidate it took (0: the style's own place) and whether that
    /// was one of the displaced ring.
    pub candidate: usize,
    pub displaced: bool,
}

/// A piece of a label, by the label's id and the piece's index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LabelPieceRef {
    pub label: EntityId,
    pub piece: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelLayout {
    pub placed: Vec<PlacedLabel>,
    /// Marks drawn whatever the text does - chainage ticks, spot-level
    /// markers - of pieces whose text was suppressed, kept so the mark still
    /// shows where the label would have gone.
    pub marks_only: Vec<PlacedLabel>,
    /// Pieces with text to place.
    pub considered: usize,
    /// Of those, no room.
    pub suppressed: usize,
    /// Of those, placed away from their first place.
    pub displaced: usize,
    /// Labels with no target, style or text.
    pub orphaned: usize,
    /// The suppressed pieces themselves, in the placing order: what LABEL
    /// LAYOUT names, so a person or an agent can select them and give them
    /// room (Annotate > Label Layout Report's Select Suppressed).
    pub suppressed_pieces: Vec<LabelPieceRef>,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    at: Point2,
    rotation: f64,
    justify: TextJustify,
    displaced: bool,
    // Set ON the line with a mask behind it, so the line itself is no
    // obstacle.
    on_line: bool,
}

// A piece waiting to be placed, with what orders it.
struct Pending<'a> {
    entity: &'a Entity,
    label: &'a LabelGeometry,
    style: &'a LabelStyle,
    piece_index: usize,
    piece: LabelPiece,
    text: String,
    dragged: bool,
}

fn unit(angle: f64) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

// The direction a text along `direction` reads in: turned half a turn when it
// would read upside down (text_layout's rule).
fn reading_angle(direction: f64) -> f64 {
    readable_rotation(direction, TextJustify::BottomLeft).rotation
}

// ---- geometry of the overlap test -----------------------------------------------------

fn separated(a: &[Point2], b: &[Point2]) -> bool {
    let n = a.len();
    for i in 0..n {
        let edge = a[(i + 1) % n] - a[i];
        let axis = edge.perpendicular();
        if !(axis.length() > 0.0) {
            continue;
        }
        let mut min_a = f64::INFINITY;
        let mut max_a = -min_a;
        for &p in a {
            let d = axis.dot(p - a[0]);
            min_a = cpp::min(min_a, d);
            max_a = cpp::max(max_a, d);
        }
        let mut min_b = f64::INFINITY;
        let mut max_b = -min_b;
        for &p in b {
            let d = axis.dot(p - a[0]);
            min_b = cpp::min(min_b, d);
            max_b = cpp::max(max_b, d);
        }
        if max_a <= min_b || max_b <= min_a {
            return true;
        }
    }
    false
}

fn point_in_convex(polygon: &[Point2], p: Point2) -> bool {
    let mut positive = false;
    let mut negative = false;
    let n = polygon.len();
    for i in 0..n {
        let c = (polygon[(i + 1) % n] - polygon[i]).cross(p - polygon[i]);
        positive = positive || c > 0.0;
        negative = negative || c < 0.0;
    }
    !(positive && negative)
}

fn segments_cross(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let d1 = (b - a).cross(c - a);
    let d2 = (b - a).cross(d - a);
    let d3 = (d - c).cross(a - c);
    let d4 = (d - c).cross(b - c);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn segment_meets_convex(polygon: &[Point2], segment: &Segment2) -> bool {
    if point_in_convex(polygon, segment.start) || point_in_convex(polygon, segment.end) {
        return true;
    }
    let n = polygon.len();
    (0..n).any(|i| segments_cross(polygon[i], polygon[(i + 1) % n], segment.start, segment.end))
}

fn box_of(points: &[Point2]) -> Box2 {
    let mut bounds = Box2::default();
    for &p in points {
        bounds.expand(p);
    }
    bounds
}

// The point of a polygon's boundary nearest `p`: where a leader meets a box.
fn nearest_on_boundary(polygon: &[Point2], p: Point2) -> Point2 {
    let mut best = polygon[0];
    let mut best_distance = f64::INFINITY;
    let n = polygon.len();
    for i in 0..n {
        let edge = Segment2::new(polygon[i], polygon[(i + 1) % n]);
        let q = edge.closest_point(p);
        let d = q.distance_to(p);
        if d < best_distance {
            best_distance = d;
            best = q;
        }
    }
    best
}

// A uniform grid over boxes, for the overlap search. Keyed by cell in an
// ordered map: the lookups ask only "is anything here", so no answer depends on
// the order, and an ordered map keeps it so.
struct Grid {
    cell: f64,
    cells: BTreeMap<(i64, i64), Vec<usize>>,
    oversized: Vec<usize>,
}

impl Grid {
    fn new(cell: f64) -> Self {
        Self {
            cell: if cell > 0.0 { cell } else { 1.0 },
            cells: BTreeMap::new(),
            oversized: Vec::new(),
        }
    }

    fn any_near(&self, bounds: &Box2, mut visit: impl FnMut(usize) -> bool) -> bool {
        let (x0, y0, x1, y1) = self.range(bounds);
        for x in x0..=x1 {
            for y in y0..=y1 {
                let Some(found) = self.cells.get(&(x, y)) else {
                    continue;
                };
                for &index in found {
                    if visit(index) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn insert(&mut self, bounds: &Box2, index: usize) {
        let (x0, y0, x1, y1) = self.range(bounds);
        // A box too big for the grid (a label the size of the drawing) is put
        // on every cell it covers up to a limit, and past it on a list every
        // query sees. Counted wide: the C++'s int64 product overflows only for
        // cells a float-to-integer cast could not make in range anyway.
        let covered = (i128::from(x1) - i128::from(x0) + 1) * (i128::from(y1) - i128::from(y0) + 1);
        if covered > 4096 {
            self.oversized.push(index);
            return;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(index);
            }
        }
    }

    // The cells a box covers. The C++'s cast of a floor out of range is
    // undefined; here it saturates.
    fn range(&self, bounds: &Box2) -> (i64, i64, i64, i64) {
        let at = |v: f64| (v / self.cell).floor() as i64;
        (
            at(bounds.min.x),
            at(bounds.min.y),
            at(bounds.max.x),
            at(bounds.max.y),
        )
    }
}

// ---- candidates ----------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Position {
    place: LabelPlacement,
    dx: f64,
    dy: f64,
    justify: TextJustify,
}

fn point_candidates(
    piece: &LabelPiece,
    style: &LabelStyle,
    gap: f64,
    height: f64,
    horizontal: f64,
    out: &mut Vec<Candidate>,
) {
    // Imhof's order for a point: upper right, lower right, upper left, lower
    // left, then right, above, left, below - the diagonals keep the text
    // clear of lines leaving the point along the axes. The diagonal is
    // 1/sqrt(2) (the C++'s 0.70710678118654752440), so every position is
    // `gap` away.
    let d = FRAC_1_SQRT_2;
    let positions = [
        Position {
            place: LabelPlacement::Auto,
            dx: d,
            dy: d,
            justify: TextJustify::BottomLeft,
        },
        Position {
            place: LabelPlacement::Auto,
            dx: d,
            dy: -d,
            justify: TextJustify::TopLeft,
        },
        Position {
            place: LabelPlacement::Auto,
            dx: -d,
            dy: d,
            justify: TextJustify::BottomRight,
        },
        Position {
            place: LabelPlacement::Auto,
            dx: -d,
            dy: -d,
            justify: TextJustify::TopRight,
        },
        Position {
            place: LabelPlacement::Right,
            dx: 1.0,
            dy: 0.0,
            justify: TextJustify::MiddleLeft,
        },
        Position {
            place: LabelPlacement::Above,
            dx: 0.0,
            dy: 1.0,
            justify: TextJustify::BottomCentre,
        },
        Position {
            place: LabelPlacement::Left,
            dx: -1.0,
            dy: 0.0,
            justify: TextJustify::MiddleRight,
        },
        Position {
            place: LabelPlacement::Below,
            dx: 0.0,
            dy: -1.0,
            justify: TextJustify::TopCentre,
        },
    ];
    let mut ordered: Vec<Position> = positions.to_vec();
    if matches!(
        style.placement,
        LabelPlacement::Right
            | LabelPlacement::Above
            | LabelPlacement::Left
            | LabelPlacement::Below
    ) {
        // std::stable_partition: the style's own place first, the rest in
        // their order.
        let (mut first, rest): (Vec<Position>, Vec<Position>) = ordered
            .iter()
            .partition(|position| position.place == style.placement);
        first.extend(rest);
        ordered = first;
    }
    let x = unit(horizontal);
    let y = x.perpendicular();
    if style.placement == LabelPlacement::Centroid || style.placement == LabelPlacement::Along {
        out.push(Candidate {
            at: piece.anchor,
            rotation: horizontal,
            justify: TextJustify::MiddleCentre,
            displaced: false,
            on_line: true,
        });
    }
    let count = if style.displace { ordered.len() } else { 1 };
    for p in &ordered[..count] {
        out.push(Candidate {
            at: piece.anchor + (x * p.dx + y * p.dy) * gap,
            rotation: horizontal,
            justify: p.justify,
            displaced: false,
            on_line: false,
        });
    }
    if !style.displace {
        return;
    }
    for ring in DISPLACED_RINGS {
        for p in &ordered {
            out.push(Candidate {
                at: piece.anchor + (x * p.dx + y * p.dy) * (gap + ring * height),
                rotation: horizontal,
                justify: p.justify,
                displaced: true,
                on_line: false,
            });
        }
    }
}

// Along a line: the point at `t` and the direction there.
struct Station {
    at: Point2,
    direction: f64,
    // Towards the outside of an arc; the line's left for a segment.
    outside: Vec2,
}

fn station_on(piece: &LabelPiece, t: f64) -> Station {
    if piece.shape == LabelPieceShape::Arc {
        let at = piece.arc.point_at(t);
        let radial = (at - piece.arc.center).normalized();
        let tangent = if piece.arc.sweep >= 0.0 {
            radial.perpendicular()
        } else {
            -radial.perpendicular()
        };
        return Station {
            at,
            direction: tangent.angle(),
            outside: radial,
        };
    }
    let along = piece.to - piece.from;
    Station {
        at: piece.from + along * t,
        direction: along.angle(),
        outside: along.normalized().perpendicular(),
    }
}

fn line_candidates(
    piece: &LabelPiece,
    style: &LabelStyle,
    gap: f64,
    height: f64,
    horizontal: f64,
    out: &mut Vec<Candidate>,
) {
    let aligned = style.orientation == LabelOrientation::Aligned;
    // Above is the outside of an arc and the left of a segment seen along
    // it: for a boundary drawn anticlockwise, outside the lot.
    let below = style.placement == LabelPlacement::Below;
    let along =
        style.placement == LabelPlacement::Along || style.placement == LabelPlacement::Centroid;
    let candidate_at = |t: f64, outside_side: bool, distance: f64, displaced: bool| {
        let s = station_on(piece, t);
        let rotation = if aligned {
            reading_angle(s.direction)
        } else {
            horizontal
        };
        let up = unit(rotation).perpendicular();
        let side = if outside_side { s.outside } else { -s.outside };
        // The text's own up or down, whichever faces away from the line, so
        // the gap is measured from its nearer edge. (The C++ chooses the same
        // two justifications whether the text is aligned or not.)
        let text_above = side.dot(up) >= 0.0;
        let justify = if text_above {
            TextJustify::BottomCentre
        } else {
            TextJustify::TopCentre
        };
        Candidate {
            at: s.at + side * distance,
            rotation,
            justify,
            displaced,
            on_line: false,
        }
    };
    if along {
        for t in SLIDE_POSITIONS {
            let s = station_on(piece, t);
            out.push(Candidate {
                at: s.at,
                rotation: if aligned {
                    reading_angle(s.direction)
                } else {
                    horizontal
                },
                justify: TextJustify::MiddleCentre,
                displaced: false,
                on_line: true,
            });
            if !style.displace {
                return;
            }
        }
    }
    let preferred = !below;
    for t in SLIDE_POSITIONS {
        out.push(candidate_at(t, preferred, gap, false));
        if !style.displace {
            return;
        }
        out.push(candidate_at(t, !preferred, gap, false));
    }
    for ring in DISPLACED_RINGS {
        out.push(candidate_at(0.5, preferred, gap + ring * height, true));
        out.push(candidate_at(0.5, !preferred, gap + ring * height, true));
    }
}

fn area_candidates(
    piece: &LabelPiece,
    style: &LabelStyle,
    height: f64,
    horizontal: f64,
    out: &mut Vec<Candidate>,
) {
    let x = unit(horizontal);
    let y = x.perpendicular();
    if !style.displace {
        // Where the style puts it, over a mask if a line runs through.
        out.push(Candidate {
            at: piece.anchor,
            rotation: horizontal,
            justify: TextJustify::MiddleCentre,
            displaced: false,
            on_line: true,
        });
        return;
    }
    // Clear of every line first: the interior point, then stepped a line up
    // and down and sideways - still inside most lots. Only when none of those
    // is clear does the interior point take the text over a mask, which hides
    // what it covers: a pit or a kerb inside the lot drawn out of sight before
    // its time.
    out.push(Candidate {
        at: piece.anchor,
        rotation: horizontal,
        justify: TextJustify::MiddleCentre,
        displaced: false,
        on_line: false,
    });
    for (dx, dy) in [
        (0.0, 1.5),
        (0.0, -1.5),
        (3.0, 0.0),
        (-3.0, 0.0),
        (0.0, 3.0),
        (0.0, -3.0),
    ] {
        out.push(Candidate {
            at: piece.anchor + (x * dx + y * dy) * height,
            rotation: horizontal,
            justify: TextJustify::MiddleCentre,
            displaced: false,
            on_line: false,
        });
    }
    out.push(Candidate {
        at: piece.anchor,
        rotation: horizontal,
        justify: TextJustify::MiddleCentre,
        displaced: false,
        on_line: true,
    });
}

fn station_candidates(
    piece: &LabelPiece,
    style: &LabelStyle,
    tick: f64,
    gap: f64,
    out: &mut Vec<Candidate>,
) {
    // Across the alignment, beyond the tick, reading outwards from it; on the
    // left looking along increasing chainage unless the style says below.
    let left = unit(piece.direction).perpendicular();
    let left_first = style.placement != LabelPlacement::Below;
    for on_left in [left_first, !left_first] {
        let side = if on_left { left } else { -left };
        let outward = side.angle();
        let readable = readable_rotation(outward, TextJustify::MiddleLeft);
        out.push(Candidate {
            at: piece.anchor + side * (tick + gap),
            rotation: readable.rotation,
            justify: readable.justify,
            displaced: false,
            on_line: false,
        });
        if !style.displace {
            return;
        }
    }
}

// The marks a piece carries whatever its text does: a point's marker, a
// chainage tick.
fn marks_of(piece: &LabelPiece, style: &LabelStyle, scale: f64) -> Drawing {
    let mut marks = Drawing::default();
    if piece.shape == LabelPieceShape::Station {
        let half = annotation_model_size(style.tick_length, scale);
        if half > 0.0 {
            let across = unit(piece.direction).perpendicular() * half;
            marks
                .strokes
                .push(vec![piece.anchor - across, piece.anchor + across]);
        }
    } else if piece.shape == LabelPieceShape::Point && style.marker != LabelMarker::None {
        let half = 0.5 * annotation_model_size(style.marker_size, scale);
        let p = piece.anchor;
        match style.marker {
            LabelMarker::Cross => {
                marks
                    .strokes
                    .push(vec![p - Vec2::new(half, 0.0), p + Vec2::new(half, 0.0)]);
                marks
                    .strokes
                    .push(vec![p - Vec2::new(0.0, half), p + Vec2::new(0.0, half)]);
            }
            LabelMarker::Dot => marks.fills.push(circle_polygon(p, half, 16)),
            LabelMarker::Circle => marks.outlines.push(circle_polygon(p, half, 32)),
            LabelMarker::None => {}
        }
    }
    marks.update_extent();
    marks
}

/// Whether two convex polygons overlap (touching edges count as apart): the
/// separating axis test the placer uses, exposed for its tests.
pub fn convex_overlap(a: &[Point2], b: &[Point2]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    !separated(a, b) && !separated(b, a)
}

fn append_path(path: &[Point2], closed: bool, out: &mut Vec<Segment2>, limit: usize) {
    for pair in path.windows(2) {
        if out.len() >= limit {
            break;
        }
        out.push(Segment2::new(pair[0], pair[1]));
    }
    if closed && path.len() > 2 && out.len() < limit {
        out.push(Segment2::new(path[path.len() - 1], path[0]));
    }
}

fn append_box(corners: &[Point2], out: &mut Vec<Segment2>, limit: usize) {
    append_path(corners, true, out, limit);
    if corners.len() == 4 && out.len() + 2 <= limit {
        out.push(Segment2::new(corners[0], corners[2]));
        out.push(Segment2::new(corners[1], corners[3]));
    }
}

/// What a note or a leader puts in a label's way, as segments appended to
/// `out` (at most up to `limit` in all): its strokes and outlines, and each
/// text box's four edges and two diagonals - the diagonals so that a label
/// wholly inside a note's box still meets it. The painter adds these to
/// LabelLayoutOptions::linework for the annotation in view, so a label keeps
/// clear of a callout's box as well as of the lines. (The C++ `appendKeepOut`
/// overload of a Drawing; append_dimension_keep_out is the other.)
pub fn append_keep_out(drawing: &Drawing, out: &mut Vec<Segment2>, limit: usize) {
    for stroke in &drawing.strokes {
        append_path(stroke, false, out, limit);
    }
    for outline in &drawing.outlines {
        append_path(outline, true, out, limit);
    }
    for corners in &drawing.text_boxes {
        append_box(corners, out, limit);
    }
}

/// The same for a dimension: its extension and dimension lines, its curves,
/// and the figures' box. A dimension's text box is its estimated extent (the
/// placer is a judgement of where there is room, not a typesetter). (The C++
/// `appendKeepOut` overload of a DimensionDrawing.)
pub fn append_dimension_keep_out(
    dimension: &DimensionDrawing,
    out: &mut Vec<Segment2>,
    limit: usize,
) {
    for &line in &dimension.extension_lines {
        if out.len() < limit {
            out.push(line);
        }
    }
    if dimension.has_dimension_line && out.len() < limit {
        out.push(dimension.dimension_line);
    }
    for curve in &dimension.curves {
        append_path(curve, false, out, limit);
    }
    if dimension.text.is_empty() || !(dimension.text_height > 0.0) {
        return;
    }
    // The figures' box from the baseline's left end, as the dimension draws
    // them: its estimated width, and a quarter height of room either side.
    let width = estimated_width(&dimension.text) * dimension.text_height;
    let along = unit(dimension.text_rotation);
    let up = along.perpendicular();
    let margin = 0.25 * dimension.text_height;
    let corner = dimension.text_anchor - along * margin - up * margin;
    let x = along * (width + 2.0 * margin);
    let y = up * (dimension.text_height + 2.0 * margin);
    append_box(
        &[corner, corner + x, corner + x + y, corner + y],
        out,
        limit,
    );
}

// An arc chorded at sixteen a turn, at least four chords.
fn append_chords(arc: &Arc2, out: &mut Vec<Segment2>, limit: usize) {
    let count = std::cmp::max(4, (16.0 * arc.sweep.abs() / TWO_PI).ceil() as i32);
    let mut previous = arc.point_at(0.0);
    let mut i = 1;
    while i <= count && out.len() < limit {
        let next = arc.point_at(f64::from(i) / f64::from(count));
        out.push(Segment2::new(previous, next));
        previous = next;
        i += 1;
    }
}

/// The straight pieces of a line, polyline, arc or circle (arcs chorded at
/// sixteen a turn: the placer asks only whether a text box crosses the
/// curve, and at a label's size that is the curve), appended to `out` up to
/// `limit`. Nothing for any other kind.
pub fn append_linework(geometry: &Geometry, out: &mut Vec<Segment2>, limit: usize) {
    match geometry {
        Geometry::Line(line) if out.len() < limit => out.push(*line),
        Geometry::Polyline(polyline) => {
            append_path(&polyline.vertices, polyline.closed, out, limit);
        }
        Geometry::Arc(arc) => append_chords(arc, out, limit),
        Geometry::Circle(circle) => append_chords(
            &Arc2::new(circle.center, circle.radius, 0.0, TWO_PI),
            out,
            limit,
        ),
        Geometry::CurvePolyline(_) => {
            // Its straight segments as they are, its arcs chorded as an arc is.
            for piece in curve_pieces(geometry, CURVE_CHORD_TOLERANCE) {
                match piece {
                    Curve2::Segment(segment) if out.len() < limit => out.push(segment),
                    Curve2::Arc(part) => append_chords(&part, out, limit),
                    _ => {}
                }
            }
        }
        Geometry::Ellipse(_) | Geometry::Spline(_) => {
            // Chords of a 256th of its extent: as coarse, for its size, as an
            // arc's sixteen a turn, since a keep-out is a place a label should
            // not stand, not a measurement.
            let extent = bounding_box(geometry);
            let chord_tolerance = cpp::max(
                CURVE_CHORD_TOLERANCE,
                extent.width().hypot(extent.height()) / 256.0,
            );
            append_path(&linework(geometry, chord_tolerance), false, out, limit);
        }
        _ => {}
    }
}

/// Everything a label keeps out of in the whole drawing at 1 : `scale`: the
/// linework and the notes', leaders' and dimensions' keep-out of every
/// entity the document draws (is_drawn, no view's layer overrides), labels
/// aside, up to `limit` segments. What a caller with no view (LABEL LAYOUT)
/// passes as LabelLayoutOptions::linework, so the command line places labels
/// where the plan view does; the painter gathers the same for what it has in
/// view as it draws. A leader's keep-out is its note as the drawing's
/// surfaces and alignments make it read (`leaders`, leader_presets.rs; the C++
/// default is `&LeaderContext::default()`).
pub fn label_keep_out(
    model: &Model,
    scale: f64,
    measure: &TextMeasure,
    limit: usize,
    leaders: &LeaderContext,
) -> Vec<Segment2> {
    let mut out = Vec::new();
    model.entities.for_each(|entity| {
        if out.len() >= limit
            || matches!(entity.geometry, Geometry::Label(_))
            || !is_drawn(model, entity, &NO_LAYER_OVERRIDES)
        {
            return;
        }
        append_linework(&entity.geometry, &mut out, limit);
        match &entity.geometry {
            Geometry::Text(text) => append_keep_out(
                &layout_text_entity(model, text, scale, measure),
                &mut out,
                limit,
            ),
            Geometry::Leader(leader) => append_keep_out(
                &build_leader(model, leader, scale, measure, leaders),
                &mut out,
                limit,
            ),
            Geometry::Dimension(dimension) => append_dimension_keep_out(
                &build_dimension(dimension, resolve_dimension_style(model, entity), scale),
                &mut out,
                limit,
            ),
            _ => {}
        }
    });
    out
}

/// Every label entity in the model, in id order: what a caller with no view
/// (the command line's LABEL LAYOUT, a test) lays out.
pub fn label_entities(model: &Model) -> Vec<&Entity> {
    // Through the ids: the database's visitor lends each entity for the call
    // only, and these references are kept.
    model
        .entities
        .ids()
        .into_iter()
        .filter_map(|id| model.entities.find(id))
        .filter(|entity| matches!(entity.geometry, Geometry::Label(_)))
        .collect()
}

/// A label's pieces (entity's label_pieces) with their `code` value found
/// where survey coding finds a code (code_property_candidates): what every
/// caller in this layer asks for, so a label's {code} is the code a mapfile
/// rule reads.
pub fn label_pieces_of(
    model: &Model,
    label: &LabelGeometry,
    style: &LabelStyle,
) -> Vec<LabelPiece> {
    label_pieces(model, label, style, code_properties())
}

/// The survey code `entity` carries, where survey coding looks for one (the
/// first of code_property_candidates it has); empty when none.
pub fn survey_code(entity: &Entity) -> String {
    for &candidate in code_property_candidates() {
        if let Some(code) = survey_code_of(entity, candidate) {
            return String::from(code);
        }
    }
    String::new()
}

/// The text a label's piece shows: its override, or its style's template
/// filled from the piece's values. Empty when the template's every line
/// needed a value the piece does not have.
pub fn label_text(label: &LabelGeometry, style: &LabelStyle, piece: &LabelPiece) -> String {
    if !label.text_override.is_empty() {
        return label.text_override.clone();
    }
    format_label(&style.text, &piece.values)
}

/// The words a whole label shows now: each piece's label_text, the empty
/// ones and chainage ticks left out, joined by " | ". Empty when its style is
/// gone or it says nothing. What LABEL LIST replies as text= and the window's
/// Properties show, so the two cannot disagree.
pub fn shown_label_text(model: &Model, label: &LabelGeometry) -> String {
    let Some(style) = model.label_styles.find(&label.style) else {
        return String::new();
    };
    let mut joined = String::new();
    for piece in label_pieces_of(model, label, style) {
        if piece.tick_only {
            continue;
        }
        let text = label_text(label, style, &piece);
        if text.is_empty() {
            continue;
        }
        if !joined.is_empty() {
            joined.push_str(" | ");
        }
        joined.push_str(&text);
    }
    joined
}

// Whether `text_box` meets a text placed before it or, unless it is set on its
// own line, the linework.
fn collides(
    text_grid: &Grid,
    placed_boxes: &[Vec<Point2>],
    line_grid: &Grid,
    linework: &[Segment2],
    text_box: &[Point2],
    on_line: bool,
    piece: &LabelPiece,
) -> bool {
    let bounds = box_of(text_box);
    let meets_text = |index: usize| convex_overlap(text_box, &placed_boxes[index]);
    if text_grid.any_near(&bounds, meets_text) {
        return true;
    }
    if text_grid.oversized.iter().any(|&index| meets_text(index)) {
        return true;
    }
    if on_line {
        return false; // set on its own line, over a mask
    }
    let meets_line = |index: usize| {
        let line = &linework[index];
        // Its own line does not push a label away: a label beside a segment
        // is placed clear of it by its offset.
        if piece.shape == LabelPieceShape::Segment
            && ((line.start == piece.from && line.end == piece.to)
                || (line.start == piece.to && line.end == piece.from))
        {
            return false;
        }
        segment_meets_convex(text_box, line)
    };
    if line_grid.any_near(&bounds, meets_line) {
        return true;
    }
    line_grid.oversized.iter().any(|&index| meets_line(index))
}

fn keep_marks(layout: &mut LabelLayout, entry: &Pending<'_>, marks: Drawing) {
    if !marks.empty() {
        layout.marks_only.push(PlacedLabel {
            label: entry.entity.id,
            piece: entry.piece_index,
            drawing: marks,
            ..PlacedLabel::default()
        });
    }
}

/// Lays out `labels` (label entities; anything else is ignored) for the
/// model as it is.
pub fn layout_labels(
    model: &Model,
    labels: &[&Entity],
    options: &LabelLayoutOptions,
) -> LabelLayout {
    let mut layout = LabelLayout::default();
    let scale = if options.scale > 0.0 && options.scale.is_finite() {
        options.scale
    } else {
        DEFAULT_ANNOTATION_SCALE
    };
    let reach = if options.visible.empty() {
        Box2::default()
    } else {
        options
            .visible
            .inflated(annotation_model_size(REACH_MILLIMETRES, scale))
    };
    // Horizontal on the sheet: the model direction of the paper's x axis.
    let horizontal = options.horizontal;

    // 1. Every piece with text, in the placing order.
    let mut pending: Vec<Pending<'_>> = Vec::new();
    for &entity in labels {
        let Geometry::Label(label) = &entity.geometry else {
            continue;
        };
        let Some(style) = model.label_styles.find(&label.style) else {
            layout.orphaned += 1;
            continue;
        };
        let pieces = label_pieces_of(model, label, style);
        if pieces.is_empty() {
            layout.orphaned += 1;
            continue;
        }
        let minimum = annotation_model_size(style.minimum_length, scale);
        for (i, piece) in pieces.into_iter().enumerate() {
            if !reach.empty() && !reach.contains(piece.anchor) {
                continue;
            }
            if (piece.shape == LabelPieceShape::Segment || piece.shape == LabelPieceShape::Arc)
                && piece.length < minimum
            {
                continue;
            }
            let text = if piece.tick_only {
                String::new()
            } else {
                label_text(label, style, &piece)
            };
            pending.push(Pending {
                entity,
                label,
                style,
                piece_index: i,
                // The dragged position belongs to the label's first piece.
                dragged: label.position.is_some() && i == 0,
                text,
                piece,
            });
        }
    }
    pending.sort_by(|a, b| {
        b.dragged
            .cmp(&a.dragged)
            .then(b.style.priority.cmp(&a.style.priority))
            .then(a.entity.id.cmp(&b.entity.id))
            .then(a.piece_index.cmp(&b.piece_index))
    });

    // 2. The obstacles.
    let cell = annotation_model_size(GRID_CELL_MILLIMETRES, scale);
    let mut text_grid = Grid::new(cell);
    let mut placed_boxes: Vec<Vec<Point2>> = Vec::new();
    let mut line_grid = Grid::new(cell);
    for (i, line) in options.linework.iter().enumerate() {
        line_grid.insert(&line.bounding_box(), i);
    }

    // 3. Place.
    for entry in &pending {
        let style = entry.style;
        let text_style_name = if style.text_style.is_empty() {
            DEFAULT_TEXT_STYLE_NAME
        } else {
            style.text_style.as_str()
        };
        let mut appearance = resolve_text_appearance(
            model,
            text_style_name,
            style.paper_height,
            annotation_model_size(2.5, scale),
            scale,
        );
        // A label's placement has already made it readable.
        appearance.readable = false;
        if style.color.is_some() {
            appearance.colour = style.color;
        }
        let height = appearance.height;
        let gap = annotation_model_size(style.offset, scale);
        let marks = marks_of(&entry.piece, style, scale);
        if entry.text.is_empty() {
            keep_marks(&mut layout, entry, marks);
            continue;
        }
        layout.considered += 1;

        let mut candidates = Vec::new();
        if entry.dragged {
            let mut rotation = horizontal;
            if style.orientation == LabelOrientation::Aligned
                && (entry.piece.shape == LabelPieceShape::Segment
                    || entry.piece.shape == LabelPieceShape::Arc)
            {
                rotation = reading_angle(entry.piece.direction);
            }
            if let Some(position) = entry.label.position {
                candidates.push(Candidate {
                    at: position,
                    rotation,
                    justify: TextJustify::MiddleCentre,
                    displaced: true,
                    on_line: false,
                });
            }
        } else {
            match entry.piece.shape {
                LabelPieceShape::Point => {
                    let marker = if style.marker == LabelMarker::None {
                        0.0
                    } else {
                        0.5 * annotation_model_size(style.marker_size, scale)
                    };
                    point_candidates(
                        &entry.piece,
                        style,
                        gap + marker,
                        height,
                        horizontal,
                        &mut candidates,
                    );
                }
                LabelPieceShape::Segment | LabelPieceShape::Arc => {
                    line_candidates(
                        &entry.piece,
                        style,
                        gap,
                        height,
                        horizontal,
                        &mut candidates,
                    );
                }
                LabelPieceShape::Area => {
                    area_candidates(&entry.piece, style, height, horizontal, &mut candidates);
                }
                LabelPieceShape::Station => station_candidates(
                    &entry.piece,
                    style,
                    annotation_model_size(style.tick_length, scale),
                    gap,
                    &mut candidates,
                ),
            }
        }

        let mut placed = false;
        for (c, candidate) in candidates.iter().enumerate() {
            let mut chosen = appearance.clone();
            chosen.mask = appearance.mask || candidate.on_line;
            if candidate.on_line && !appearance.mask {
                chosen.mask_margin = 0.25 * height;
            }
            let mut drawing = layout_text(
                &entry.text,
                candidate.at,
                candidate.rotation,
                candidate.justify,
                &chosen,
                &options.measure,
            );
            let Some(text_box) = drawing.text_boxes.first().cloned() else {
                break;
            };
            if options.avoid_collisions
                && !entry.dragged
                && collides(
                    &text_grid,
                    &placed_boxes,
                    &line_grid,
                    &options.linework,
                    &text_box,
                    candidate.on_line,
                    &entry.piece,
                )
            {
                continue;
            }
            if candidate.displaced && style.leader {
                let end = nearest_on_boundary(&text_box, entry.piece.anchor);
                if end.distance_to(entry.piece.anchor) > tolerance::GEOMETRIC {
                    drawing.strokes.push(vec![entry.piece.anchor, end]);
                }
            }
            drawing.append(&marks);
            drawing.update_extent();
            let index = placed_boxes.len();
            text_grid.insert(&box_of(&text_box), index);
            placed_boxes.push(text_box);
            layout.displaced += usize::from(candidate.displaced);
            layout.placed.push(PlacedLabel {
                label: entry.entity.id,
                piece: entry.piece_index,
                drawing,
                candidate: c,
                displaced: candidate.displaced,
            });
            placed = true;
            break;
        }
        if !placed {
            layout.suppressed += 1;
            layout.suppressed_pieces.push(LabelPieceRef {
                label: entry.entity.id,
                piece: entry.piece_index,
            });
            keep_marks(&mut layout, entry, marks);
        }
    }
    layout
}
