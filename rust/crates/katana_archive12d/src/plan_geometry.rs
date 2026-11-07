//! The plan shape of a string: vertices joined by straights, arcs and
//! transitions, turned into the straight chords a Polyline2 can hold.
//! Internal to the crate.
//!
//! THE MANUAL IS NOT ENOUGH TO DRAW A TRANSITION, AND IN ONE PLACE IT IS
//! WRONG. Three conventions below were settled by measurement against the
//! writing program's own output (`test Super Alignment.12da`, written by its
//! version 15.0C1t, which holds 32 arcs and 16 transitions whose end points
//! are all recorded):
//!
//! ```text
//! 1. A POSITIVE RADIUS TURNS RIGHT. The manual says "+ve is above the line
//!    connecting the vertices", which does not say which way is up. All 18
//!    arcs that follow a straight agree: 10 positive, all turning clockwise;
//!    8 negative, all counter-clockwise.
//!
//! 2. A TRAILING TRANSITION IS DESCRIBED BACKWARDS. The manual defines l1, r1,
//!    a1 as the values "at the start vertex" and says a full trailing
//!    transition has r2 = 0 and l2 = 0. What the program writes is l1 = 0,
//!    r1 = 0 for trailing transitions too, with a1 the tangent at the
//!    segment's SECOND vertex pointing back along the alignment: the block
//!    always runs from the (l1, r1, a1) end to the (l2, r2, a2) end, and
//!    `leading` says whether that is the direction of the string (1) or
//!    against it (0). Read that way, all 16 transitions land on their
//!    recorded end points; read the manual's way, every trailing one misses
//!    by 40 to 195 m.
//!
//! 3. "CUBIC PARABOLA" IS y = m x^3 IN THE FRAME OF THE TANGENT, with m chosen
//!    so that the TRUE curvature at the end, 6mX / (1 + 9m^2 X^4)^(3/2), is
//!    1/R and the ARC LENGTH to the end is L. Under that definition all 16
//!    close to under 0.01 mm and reproduce a2 to the four decimals written.
//!    The textbook form y = x^3 / (6RL), and a clothoid, both miss by up to
//!    0.3 m on the same data - a railway transition of L = 80, R = 210.
//! ```
//!
//! Angles are degrees counter-clockwise from +x.
//!
//! The other transition types (Westrail cubic, cubic spiral, Bloss,
//! sinusoidal, cosinusoidal) are not defined by the manual and no sample holds
//! one. They are drawn as the clothoid with the same end radii, then sheared
//! so that the far end meets the vertex the file recorded - both ends exact,
//! the shape between them an approximation - and the size of that adjustment
//! is reported, because it IS the error.

use std::collections::BTreeMap;
use std::f64::consts::{PI, SQRT_2};

use katana_geometry::chording::sagitta_chord_count;
use katana_geometry::{Point2, Spiral2, Vec2};

use crate::archive::{Segment, SegmentKind, Vertex};
use crate::text_utilities::lower;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PlanPoint {
    pub point: Point2,
    /// Which segment the point lies on and how far along it, 0 at the
    /// segment's first vertex. A vertex of the string has fraction 0. This is
    /// what lets a height be interpolated for a point that chording invented.
    pub segment: usize,
    pub fraction: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ChordReport {
    pub arcs: usize,
    pub transitions: usize,
    /// Segments drawn as a straight because their parameters made no sense: an
    /// arc whose radius cannot span its chord by more than rounding, a
    /// transition with no length or that missed its end by metres.
    pub straightened: usize,
    /// Transition type -> the largest distance an approximated transition had
    /// to be moved to meet its recorded end.
    pub adjustment_by_type: BTreeMap<String, f64>,
}

impl ChordReport {
    pub fn merge(&mut self, other: &ChordReport) {
        self.arcs += other.arcs;
        self.transitions += other.transitions;
        self.straightened += other.straightened;
        for (kind, &adjustment) in &other.adjustment_by_type {
            raise_to(
                self.adjustment_by_type.entry(kind.clone()).or_insert(0.0),
                adjustment,
            );
        }
    }
}

/// The circle an arc segment lies on, and the part of it the segment is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ArcCircle {
    pub centre: Point2,
    /// Unsigned.
    pub radius: f64,
    pub clockwise: bool,
    pub start_angle: f64,
    /// Signed: negative when clockwise.
    pub sweep: f64,
}

/// The end of the format's cubic parabola in the frame of its tangent:
/// abscissa X, offset Y (unsigned) and the coefficient m.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CubicParabolaEnd {
    pub abscissa: f64,
    #[allow(
        dead_code,
        reason = "The C++ solves the end whole; the chording reads the abscissa and the coefficient."
    )]
    pub offset: f64,
    pub coefficient: f64,
}

// std::max(worst, value), as the C++ keeps the largest: the first unless the
// second is larger.
fn raise_to(worst: &mut f64, value: f64) {
    if *worst < value {
        *worst = value;
    }
}

// std::min and std::max, which return the first argument unless the second is
// strictly smaller (larger) - not f64::min and f64::max, which differ on NaN.
fn min_of(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn max_of(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

// A computed transition that ends further than this from the vertex the file
// recorded is not an approximation of it, it is a misreading: the segment is
// drawn straight and counted. One metre, or 5% of the length if that is more -
// the worst honest case measured (a clothoid standing in for a cubic parabola
// of L = 80, R = 210) misses by 0.28 m, 0.35% - but never more than the chord
// itself: a curve that misses by more than the distance between its own ends
// is nowhere near, whatever its length claims.
fn closure_limit(length: f64, chord: f64) -> f64 {
    min_of(max_of(1.0, 0.05 * length), chord)
}

// A transition between two vertices is at least as long as the chord between
// them, and not many times longer: a spiral turning through less than a half
// turn is under pi/2 chords long, and a transition is never a half turn. Ten
// chords is far outside anything real and inside nothing else.
fn plausible_transition_length(length: f64, chord: f64, tolerance: f64) -> bool {
    length.is_finite()
        && chord.is_finite()
        && chord > 0.0
        && (chord - tolerance..=10.0 * chord).contains(&length)
}

// A transition moved by less than this to meet its recorded end is not worth
// a warning: a millimetre is what the coordinates of a real export are good
// to, and the two types that are computed exactly - the cubic parabola, and
// the clothoid, of which the writing program's own "clothoid" is a series
// approximation - close far inside it. What is left above it is a genuinely
// approximated type.
const REPORTABLE_ADJUSTMENT: f64 = 0.001;

fn radians(degrees: f64) -> f64 {
    degrees * PI / 180.0
}

// The format's radius to the curvature Spiral2 uses: a radius of 0 means a
// straight, and the hands are opposite - the format's positive turns right,
// Spiral2's positive turns left.
fn curvature_of(radius: f64) -> f64 {
    if radius == 0.0 { 0.0 } else { -1.0 / radius }
}

// Arc length of y = m x^3 over [0, X], composite Simpson. The integrand
// sqrt(1 + 9 m^2 x^4) is smooth and within a few percent of 1 for any real
// transition, so 64 intervals are good to far beyond a double's digits.
#[allow(
    non_snake_case,
    reason = "X is the abscissa of the C++ and of the formula it implements."
)]
fn cubic_arc_length(m: f64, X: f64) -> f64 {
    const INTERVALS: i32 = 64;
    let h = X / f64::from(INTERVALS);
    let f = |x: f64| (1.0 + 9.0 * m * m * x * x * x * x).sqrt();
    let mut sum = f(0.0) + f(X);
    for i in 1..INTERVALS {
        sum += f(f64::from(i) * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

// The m for which the true curvature of y = m x^3 at x = X is 1/R, on the
// rising branch (slope below 1/sqrt 2). Negative when there is none.
#[allow(
    non_snake_case,
    reason = "X is the abscissa of the C++ and of the formula it implements."
)]
fn cubic_coefficient(radius: f64, X: f64) -> f64 {
    // In terms of the end slope t = 3 m X^2 the condition is
    // t / (1 + t^2)^1.5 = X / (2R), and the left side rises on [0, 1/sqrt 2].
    let target = X / (2.0 * radius);
    // Divided as the C++ divides std::numbers::sqrt2: the rounded quotient
    // need not be FRAC_1_SQRT_2, the correctly rounded constant.
    let t_max = 1.0 / SQRT_2;
    if target > t_max / (1.0 + t_max * t_max).powf(1.5) {
        return -1.0;
    }
    let mut low = 0.0;
    let mut high = t_max;
    for _ in 0..100 {
        let t: f64 = 0.5 * (low + high);
        if t / (1.0 + t * t).powf(1.5) < target {
            low = t;
        } else {
            high = t;
        }
    }
    0.5 * (low + high) / (3.0 * X * X)
}

fn push_interior(
    out: &mut Vec<PlanPoint>,
    mut points: Vec<Point2>,
    segment: usize,
    reversed: bool,
) {
    // `points` runs from the described start to the described end, inclusive
    // of both; only what lies strictly between the vertices is new.
    let count = points.len();
    if count < 3 {
        return;
    }
    if reversed {
        points.reverse();
    }
    for (i, &point) in points.iter().enumerate().take(count - 1).skip(1) {
        out.push(PlanPoint {
            point,
            segment,
            fraction: i as f64 / (count - 1) as f64,
        });
    }
}

// Moves a computed curve so that its last point is `target`, the correction
// growing linearly along it. The first point stays put.
fn close_onto(points: &mut [Point2], target: Point2) {
    let Some(&last_point) = points.last() else {
        return;
    };
    let miss: Vec2 = target - last_point;
    let last = (points.len() - 1) as f64;
    for (i, point) in points.iter_mut().enumerate().skip(1) {
        *point += miss * (i as f64 / last);
    }
}

fn chord_arc_segment(
    a: Point2,
    b: Point2,
    segment: &Segment,
    tolerance: f64,
    index: usize,
    out: &mut Vec<PlanPoint>,
    report: &mut ChordReport,
) {
    let Some(arc) = solve_arc(a, b, segment, tolerance) else {
        // Coincident vertices or a radius of nothing are simply no arc; a
        // radius too small for its chord is a segment that was not
        // reproduced.
        if a.distance_to(b) > 0.0 && segment.radius != 0.0 {
            report.straightened += 1;
        }
        return;
    };
    report.arcs += 1;
    let chords = sagitta_chord_count(arc.radius, arc.sweep, tolerance);
    for i in 1..chords {
        let fraction = i as f64 / chords as f64;
        let angle = arc.start_angle + arc.sweep * fraction;
        out.push(PlanPoint {
            point: Point2::new(
                arc.centre.x + arc.radius * angle.cos(),
                arc.centre.y + arc.radius * angle.sin(),
            ),
            segment: index,
            fraction,
        });
    }
}

fn chord_spiral_segment(
    a: Point2,
    b: Point2,
    segment: &Segment,
    tolerance: f64,
    index: usize,
    out: &mut Vec<PlanPoint>,
    report: &mut ChordReport,
) {
    let p = &segment.parameters;
    let a1 = p.real("a1");
    let l1 = p.real("l1").unwrap_or(0.0);
    let l2 = p.real("l2").unwrap_or(0.0);
    let r1 = p.real("r1").unwrap_or(0.0);
    let r2 = p.real("r2").unwrap_or(0.0);
    let length = (l2 - l1).abs();
    let Some(a1) = a1 else {
        report.straightened += 1;
        return;
    };
    if !(length > 0.0)
        || !a1.is_finite()
        || !plausible_transition_length(length, a.distance_to(b), tolerance)
    {
        report.straightened += 1;
        return;
    }
    // See the module comment: the block runs from its (l1, r1, a1) end, and
    // `leading` says whether that end is the segment's first vertex.
    let leading = p.boolean("leading").unwrap_or(true);
    let origin = if leading { a } else { b };
    let target = if leading { b } else { a };
    let kind = lower(&p.text("type", "clothoid"));
    let direction = radians(a1);

    let mut points = Vec::new();
    let cubic = if kind == "cubic parabola" && l1 == 0.0 && r1 == 0.0 && r2 != 0.0 {
        solve_cubic_parabola(r2.abs(), length)
    } else {
        None
    };
    if let Some(cubic) = cubic {
        let turn = (3.0 * cubic.coefficient * cubic.abscissa * cubic.abscissa).atan();
        let chords = 2.max(sagitta_chord_count(r2.abs(), turn, tolerance));
        let hand = if r2 > 0.0 { -1.0 } else { 1.0 }; // positive radius: to the right of the tangent
        let along = Vec2::new(direction.cos(), direction.sin());
        let left = Vec2::new(-along.y, along.x);
        for i in 0..=chords {
            let x = cubic.abscissa * i as f64 / chords as f64;
            points.push(origin + along * x + left * (hand * cubic.coefficient * x * x * x));
        }
    } else {
        let spiral = Spiral2 {
            start: origin,
            start_direction: direction,
            start_curvature: curvature_of(r1),
            end_curvature: curvature_of(r2),
            length,
        };
        let chords = 2.max(spiral.chord_count_for(tolerance));
        for i in 0..=chords {
            points.push(spiral.point_at(length * i as f64 / chords as f64));
        }
    }

    let miss = points[points.len() - 1].distance_to(target);
    if !miss.is_finite() || miss > closure_limit(length, a.distance_to(b)) {
        report.straightened += 1;
        return;
    }
    if miss > REPORTABLE_ADJUSTMENT {
        raise_to(report.adjustment_by_type.entry(kind).or_insert(0.0), miss);
    }
    close_onto(&mut points, target);
    report.transitions += 1;
    push_interior(out, points, index, !leading);
}

// An offset transition (manual 1.5.8.2.2.4). The manual's own comments for
// `start` and `end` contradict its prose, it does not fix the hand of `offset`
// for a trailing transition, and no sample file holds one - so rather than
// pick a reading, every reading is tried and the one that lands on BOTH
// recorded vertices is taken. None landing means the segment is drawn
// straight and counted: a wrong curve drawn confidently is worse than a
// chord.
fn chord_curve_segment(
    a: Point2,
    b: Point2,
    segment: &Segment,
    tolerance: f64,
    index: usize,
    out: &mut Vec<PlanPoint>,
    report: &mut ChordReport,
) {
    let p = &segment.parameters;
    let (Some(x), Some(y), Some(angle), Some(radius), Some(length), Some(from), Some(to)) = (
        p.real("xorigin"),
        p.real("yorigin"),
        p.real("angle"),
        p.real("radius"),
        p.real("length"),
        p.real("start"),
        p.real("end"),
    ) else {
        report.straightened += 1;
        return;
    };
    if !(length > 0.0) || radius == 0.0 || from == to {
        report.straightened += 1;
        return;
    }
    let offset = p.real("offset").unwrap_or(0.0);

    let base = Spiral2 {
        start: Point2::new(x, y),
        start_direction: radians(angle),
        start_curvature: 0.0,
        end_curvature: curvature_of(radius),
        length,
    };

    let low = min_of(from, to);
    let high = max_of(from, to);
    let span = Spiral2 {
        start: base.point_at(low),
        start_direction: base.direction_at(low),
        start_curvature: base.curvature_at(low),
        end_curvature: base.curvature_at(high),
        length: high - low,
    };
    let chords = 2.max(span.chord_count_for(tolerance));

    // Both ends must land within a centimetre, or ten tolerances if that is
    // more: tight enough that a wrong reading (out by the offset, or by the
    // whole curve) cannot pass, loose enough for coordinates written to 3
    // places.
    let limit = max_of(0.01, 10.0 * tolerance);
    for hand in [1.0, -1.0] {
        let mut points = Vec::new();
        for i in 0..=chords {
            let s = low + (high - low) * i as f64 / chords as f64;
            let direction = base.direction_at(s);
            let right = Vec2::new(direction.sin(), -direction.cos());
            points.push(base.point_at(s) + right * (offset * hand));
        }
        for reversed in [false, true] {
            let first = if reversed {
                points[points.len() - 1]
            } else {
                points[0]
            };
            let last = if reversed {
                points[0]
            } else {
                points[points.len() - 1]
            };
            if first.distance_to(a) <= limit && last.distance_to(b) <= limit {
                report.transitions += 1;
                push_interior(out, points, index, reversed);
                return;
            }
        }
        if offset == 0.0 {
            break; // the hand of a zero offset changes nothing
        }
    }
    report.straightened += 1;
}

/// None when the segment is not an arc between these two points: no radius,
/// no chord, or a radius that cannot span the chord by more than `tolerance`.
pub(crate) fn solve_arc(
    a: Point2,
    b: Point2,
    segment: &Segment,
    tolerance: f64,
) -> Option<ArcCircle> {
    let chord = a.distance_to(b);
    let mut radius = segment.radius.abs();
    if !(chord > 0.0) || !(radius > 0.0) || !chord.is_finite() || !radius.is_finite() {
        return None;
    }
    let half = 0.5 * chord;
    if radius < half {
        // A radius written to 3 places can fall a hair short of half the chord
        // of the semicircle it describes. More than that and it is not an arc
        // between these two points at all.
        if half - radius > max_of(tolerance, 1e-6 * half) {
            return None;
        }
        radius = half;
    }
    let height = max_of(0.0, radius * radius - half * half).sqrt();
    let direction = (b - a) / chord;
    let right_normal = Vec2::new(direction.y, -direction.x);
    let clockwise = segment.radius > 0.0;
    // Travelling clockwise the centre is on the right. For the minor arc that
    // is also the right of the CHORD; the major arc bulges so far that its
    // centre falls on the chord's other side.
    let mut side = if clockwise { 1.0 } else { -1.0 };
    if segment.major {
        side = -side;
    }
    let centre = a + (b - a) * 0.5 + right_normal * (height * side);
    let start_angle = (a.y - centre.y).atan2(a.x - centre.x);
    let end_angle = (b.y - centre.y).atan2(b.x - centre.x);
    let sweep = if clockwise {
        -((start_angle - end_angle + 4.0 * PI) % (2.0 * PI))
    } else {
        (end_angle - start_angle + 4.0 * PI) % (2.0 * PI)
    };
    if sweep == 0.0 {
        return None;
    }
    Some(ArcCircle {
        centre,
        radius,
        clockwise,
        start_angle,
        sweep,
    })
}

/// The direction of travel, radians counter-clockwise from +x, at `point` on
/// the arc. EXACT - perpendicular to the radius - which a chord of the arc is
/// not: a PI reconstructed from the chords of an arc drawn to a millionth of a
/// unit still lands 9 mm out at R = 40, because a chord's direction is off by
/// half the angle it subtends and the error is multiplied by the tangent
/// length.
pub(crate) fn arc_tangent_at(arc: &ArcCircle, point: Point2) -> f64 {
    let radial = point - arc.centre;
    // A quarter turn from the radius: to the right of it going clockwise, to
    // the left going counter-clockwise.
    if arc.clockwise {
        (-radial.x).atan2(radial.y)
    } else {
        radial.x.atan2(-radial.y)
    }
}

/// The cubic parabola's end for end radius `radius` (> 0) and arc length
/// `length`. None when no such curve exists: the true curvature of y = m x^3
/// peaks where the slope is 1/sqrt(2), so a transition much longer than 0.77
/// R cannot reach radius R.
#[allow(
    non_snake_case,
    reason = "X is the abscissa of the C++ and of the formula it implements."
)]
pub(crate) fn solve_cubic_parabola(radius: f64, length: f64) -> Option<CubicParabolaEnd> {
    if !(radius > 0.0) || !(length > 0.0) {
        return None;
    }
    // The abscissa is a little less than the arc length; bisect on it.
    let mut low = 0.5 * length;
    let mut high = length;
    if cubic_coefficient(radius, low) < 0.0 {
        return None; // even the shortest candidate cannot reach this radius
    }
    for _ in 0..80 {
        let X = 0.5 * (low + high);
        let m = cubic_coefficient(radius, X);
        // No curve of this abscissa reaches the radius: it is too long.
        if m < 0.0 || cubic_arc_length(m, X) > length {
            high = X;
        } else {
            low = X;
        }
    }
    let X = 0.5 * (low + high);
    let m = cubic_coefficient(radius, X);
    if m < 0.0 || (cubic_arc_length(m, X) - length).abs() > 1e-6 * length {
        return None;
    }
    Some(CubicParabolaEnd {
        abscissa: X,
        coefficient: m,
        offset: m * X * X * X,
    })
}

pub(crate) fn chord_plan(
    vertices: &[Vertex],
    segments: &[Segment],
    closed: bool,
    tolerance: f64,
    report: &mut ChordReport,
) -> Vec<PlanPoint> {
    let mut out = Vec::new();
    let count = vertices.len();
    if count == 0 {
        return out;
    }
    let segment_count = if count < 2 {
        0
    } else if closed {
        count
    } else {
        count - 1
    };
    out.reserve(count);
    for (i, vertex) in vertices.iter().enumerate() {
        let a = Point2::new(vertex.x, vertex.y);
        out.push(PlanPoint {
            point: a,
            segment: i,
            fraction: 0.0,
        });
        if i >= segment_count || i >= segments.len() {
            continue;
        }
        let next = &vertices[(i + 1) % count];
        let b = Point2::new(next.x, next.y);
        let segment = &segments[i];
        match segment.kind {
            SegmentKind::Arc => chord_arc_segment(a, b, segment, tolerance, i, &mut out, report),
            SegmentKind::Spiral => {
                chord_spiral_segment(a, b, segment, tolerance, i, &mut out, report);
            }
            SegmentKind::Curve => {
                chord_curve_segment(a, b, segment, tolerance, i, &mut out, report)
            }
            // A parabola is vertical geometry only; in plan it is a line.
            SegmentKind::Straight | SegmentKind::Parabola => {}
        }
    }
    out
}
