//! Grading: carrying the edge of a designed thing to the ground
//! (include/katana/cad/grading.hpp; feature lines, pads, slopes, daylight,
//! grading surfaces).
//!
//! A FEATURE LINE is a string of 3D points - the edge of a pad, the crest of
//! an embankment, the toe of a batter - at the elevations the design puts
//! them. Grading it means, at every point along it, running a batter of the
//! given slope outward until it meets the ground: the DAYLIGHT line. The pad
//! (or the strip beside an open line) plus the batters between the feature
//! line and the daylight line is the GRADING SURFACE, and the volume between
//! that and the ground is the earthwork.
//!
//! The daylight search is the corridor's: a march outward in fixed steps
//! until the batter and the ground change order, then a bisection to a
//! millimetre. The ground is a TIN, piecewise planar, with no closed form for
//! where a line meets it. What is new here is the DIRECTION of the march:
//! perpendicular to the feature line along an edge, and along the bisector at
//! a vertex - and at a vertex the batter is FLATTER in the direction of march
//! by the cosine of half the corner, because the slope belongs to the edge's
//! batter plane, not to the bisector. Without that a square pad on flat
//! ground would daylight to a chamfered corner 29% short of where the two
//! batter planes actually meet. Mitred corners are what a plane batter gives
//! and what a machine builds; a rounded ("radial") corner is a later option.
//!
//! REJECTED: grading by offsetting the feature line in plan by a fixed width
//! and reading the ground there. That is a bench, not a batter: the width to
//! daylight depends on the ground, and on sloping ground it differs on every
//! side of the same pad.

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::polygon::is_simple;
use katana_geometry::{Point2, Point3, Polyline2, Vec2};
use katana_math::cpp;
use katana_math::numerics::tolerance;
use katana_terrain::volume::compare_surfaces;
use katana_terrain::{
    Breakline, CrossingBreaklinePolicy, DuplicatePointPolicy, TinBuildOptions, TinInput,
    TinSurface, build_tin,
};

// Half a metre resolves any batter a machine can build; the bisection then
// finds the crossing to a millimetre. The same step as the corridor's.
const STEP: f64 = 0.5;

// A corner sharper than this - the bisector at more than about 78 degrees to
// the edge normal - would send the mitre out by more than five times the
// batter width. Real pads do not have corners like that; a feature line that
// does gets its corner graded as if it were this sharp, which keeps the
// daylight line finite. cos(78 deg) = 0.2.
const MINIMUM_CORNER: f64 = 0.2;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureLine {
    pub vertices: Vec<Point3>,
    /// Closed: a pad, graded outward all round. Open: graded on one side.
    pub closed: bool,
}

impl FeatureLine {
    /// A flat pad: the footprint at one elevation.
    pub fn pad(footprint: &Polyline2, elevation: f64) -> Self {
        Self {
            vertices: footprint
                .vertices
                .iter()
                .map(|vertex| Point3::new(vertex.x, vertex.y, elevation))
                .collect(),
            closed: true,
        }
    }
}

/// Of an open feature line, looking along it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GradingSide {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradingSlopes {
    /// Run per unit rise, as Assembly has them: 2.0 is 1 in 2 (1V:2H).
    pub cut_batter: f64,
    pub fill_batter: f64,
    /// How far the batter may run before the search gives up on that sample.
    pub maximum_width: f64,
    /// Spacing of the samples along an edge, in model units. Vertices are
    /// always sampled; edges shorter than this get their ends only.
    pub interval: f64,
    /// Open lines only.
    pub side: GradingSide,
}

impl Default for GradingSlopes {
    fn default() -> Self {
        Self {
            cut_batter: 2.0,
            fill_batter: 3.0,
            maximum_width: 50.0,
            interval: 5.0,
            side: GradingSide::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DaylightPoint {
    /// On the feature line.
    pub origin: Point3,
    /// On the ground.
    pub daylight: Point3,
    /// The ground was above the origin.
    pub cut: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Grading {
    /// In order along the feature line. A sample whose batter never met the
    /// ground is not here; it is counted below.
    pub daylights: Vec<DaylightPoint>,
    pub samples: usize,
    /// No ground there, or none within maximum_width.
    pub missed: usize,
    /// The daylight line as a plan polyline, closed for a closed feature
    /// line.
    pub daylight_line: Polyline2,
    /// Feature line plus batters. Bounded by the daylight line when every
    /// sample reached the ground; otherwise by the convex hull of what did,
    /// and `bounded_by_daylight` says which.
    pub surface: TinSurface,
    pub bounded_by_daylight: bool,
    /// Earthwork between the grading surface and the ground, from the exact
    /// overlay in terrain's compare_surfaces. Cut and fill are both >= 0.
    pub cut: f64,
    pub fill: f64,
    pub plan_area: f64,
}

fn validate(line: &FeatureLine, slopes: &GradingSlopes) -> Status {
    for vertex in &line.vertices {
        if !vertex.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the feature line has a non-finite vertex",
            ));
        }
    }
    let least = if line.closed { 3 } else { 2 };
    if line.vertices.len() < least {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            if line.closed {
                "a closed feature line needs at least three vertices"
            } else {
                "a feature line needs at least two vertices"
            },
        ));
    }
    if !(slopes.cut_batter > 0.0)
        || !(slopes.fill_batter > 0.0)
        || !slopes.cut_batter.is_finite()
        || !slopes.fill_batter.is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "batter slopes must be positive (horizontal run per unit rise)",
        ));
    }
    if !(slopes.maximum_width > 0.0) || !slopes.maximum_width.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the maximum batter width must be positive",
        ));
    }
    if !(slopes.interval > 0.0) || !slopes.interval.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the sample interval must be positive",
        ));
    }
    Ok(())
}

// Where a batter runs from, which way, and how much flatter it is in that
// direction than the edge's own batter.
#[derive(Clone, Copy)]
struct Sample {
    origin: Point3,
    /// Unit, outward.
    direction: Vec2,
    /// The edge normal's projection on `direction`: 1 along an edge, cos of
    /// half the corner at a vertex. The batter's run per rise in the march
    /// direction is the design run divided by this.
    cosine: f64,
}

// The outward side of a closed feature line: to the right of travel when the
// ring is counter-clockwise, to the left when it is clockwise.
fn outward_sign(line: &FeatureLine, side: GradingSide) -> f64 {
    if !line.closed {
        return if side == GradingSide::Left { 1.0 } else { -1.0 };
    }
    let plan = Polyline2::new(
        line.vertices
            .iter()
            .map(|vertex| Point2::new(vertex.x, vertex.y))
            .collect(),
        true,
    );
    if plan.signed_area() > 0.0 { -1.0 } else { 1.0 }
}

// `sign` * perpendicular of the direction of travel: +1 is the left.
fn side_normal(travel: Vec2, sign: f64) -> Vec2 {
    travel.perpendicular() * sign
}

fn sample_feature_line(line: &FeatureLine, slopes: &GradingSlopes) -> Vec<Sample> {
    let mut samples = Vec::new();
    let count = line.vertices.len();
    let edges = if line.closed { count } else { count - 1 };
    let sign = outward_sign(line, slopes.side);

    let travel_of = |edge: usize| -> Option<Vec2> {
        let a = line.vertices[edge % count];
        let b = line.vertices[(edge + 1) % count];
        let d = Vec2::new(b.x - a.x, b.y - a.y);
        let length = d.length();
        if !(length > tolerance::GEOMETRIC) {
            return None; // a doubled vertex has no direction
        }
        Some(d / length)
    };

    for i in 0..count {
        // The vertex: along the bisector of its two edge normals. An open
        // line's ends have one edge, and its normal.
        let has_before = line.closed || i > 0;
        let has_after = line.closed || i + 1 < count;
        let before = if has_before {
            travel_of((i + count - 1) % count)
        } else {
            None
        };
        let after = if has_after { travel_of(i) } else { None };
        let mut vertex = Sample {
            origin: line.vertices[i],
            direction: Vec2::default(),
            cosine: 1.0,
        };
        match (before, after) {
            (Some(before), Some(after)) => {
                let n1 = side_normal(before, sign);
                let n2 = side_normal(after, sign);
                let sum = n1 + n2;
                if sum.length() > tolerance::GEOMETRIC {
                    vertex.direction = sum.normalized();
                    vertex.cosine = cpp::max(MINIMUM_CORNER, vertex.direction.dot(n2));
                } else {
                    // A hairpin: the normals cancel. March straight on.
                    vertex.direction = after;
                    vertex.cosine = MINIMUM_CORNER;
                }
            }
            (Some(one), None) | (None, Some(one)) => {
                vertex.direction = side_normal(one, sign);
            }
            // Both edges degenerate: nothing to grade from here.
            (None, None) => continue,
        }
        samples.push(vertex);

        // Along the edge that follows, at the interval, ends excluded.
        if i < edges
            && let Some(after) = after
        {
            let a = line.vertices[i];
            let b = line.vertices[(i + 1) % count];
            let length = (b.x - a.x).hypot(b.y - a.y);
            // The C++ casts without a guard: the interval is validated finite
            // and positive and the vertices finite, so the count is finite.
            let steps = (length / slopes.interval).floor() as usize;
            let normal = side_normal(after, sign);
            for k in 1..steps {
                let t = k as f64 * slopes.interval / length;
                samples.push(Sample {
                    origin: a + (b - a) * t,
                    direction: normal,
                    cosine: 1.0,
                });
            }
            // The interval does not usually divide the edge; the last
            // stretch is shorter rather than a sample landing on the next
            // vertex.
        }
    }
    samples
}

// The corridor's daylight search, from an arbitrary origin in an arbitrary
// direction: it marches rather than solving, since the ground has no closed
// form.
fn daylight(ground: &TinSurface, sample: &Sample, slopes: &GradingSlopes) -> Option<DaylightPoint> {
    let ground_at = |d: f64| {
        ground.elevation_at(Point2::new(
            sample.origin.x + sample.direction.x * d,
            sample.origin.y + sample.direction.y * d,
        ))
    };
    let ground_at_origin = ground_at(0.0)?;
    let difference = ground_at_origin - sample.origin.z;
    let mut point = DaylightPoint {
        origin: sample.origin,
        daylight: Point3::default(),
        cut: difference > 0.0,
    };
    if difference.abs() <= tolerance::COORDINATE {
        point.daylight = sample.origin; // already at grade
        return Some(point);
    }
    let cut = point.cut;
    let run = (if cut {
        slopes.cut_batter
    } else {
        slopes.fill_batter
    }) / sample.cosine;
    let rise = (if cut { 1.0 } else { -1.0 }) / run;
    let batter_above = |d: f64| -> Option<bool> {
        let z = ground_at(d)?;
        let batter = sample.origin.z + rise * d;
        Some(if cut { batter >= z } else { batter <= z })
    };

    let mut previous = 0.0;
    // The width limit is a plan distance perpendicular to the edge; along a
    // mitre the same batter runs further.
    let limit = slopes.maximum_width / sample.cosine;
    let mut d = STEP;
    while d <= limit + 1e-9 {
        let crossed = batter_above(d)?;
        if crossed {
            let mut low = previous;
            let mut high = d;
            for _ in 0..40 {
                let mid = 0.5 * (low + high);
                if batter_above(mid)? {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            let d0 = 0.5 * (low + high);
            point.daylight = Point3::new(
                sample.origin.x + sample.direction.x * d0,
                sample.origin.y + sample.direction.y * d0,
                sample.origin.z + rise * d0,
            );
            return Some(point);
        }
        previous = d;
        d += STEP;
    }
    None
}

/// Fails with InvalidArgument for a slope, width or interval that is not
/// positive, a non-finite vertex, or a feature line with fewer than two
/// vertices (three when closed); with InvalidGeometry when no sample reaches
/// the ground, or the surface cannot be built. A self-intersecting daylight
/// line (a sharp concave corner whose batters cross their neighbours') is not
/// an error: the surface is built without it as a boundary and
/// `bounded_by_daylight` is false.
pub fn grade_to_surface(
    line: &FeatureLine,
    slopes: &GradingSlopes,
    ground: &TinSurface,
) -> Result<Grading> {
    validate(line, slopes)?;
    let mut result = Grading::default();
    let samples = sample_feature_line(line, slopes);
    result.samples = samples.len();
    for sample in &samples {
        if let Some(point) = daylight(ground, sample, slopes) {
            result.daylights.push(point);
        } else {
            result.missed += 1;
        }
    }
    if result.daylights.len() < 2 {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "the batters reach the ground at fewer than two points: is the feature line over the surface?",
            format!(
                "{} of {} samples found no ground",
                result.missed, result.samples
            ),
        ));
    }

    // The surface: the feature line and the daylight line as breaklines, and
    // every batter as one - each is the edge of a plane and the triangles
    // must not cut across it. The daylight line is the boundary when it is
    // whole and simple; a concave corner whose batters overrun its
    // neighbours' can make it cross itself, and then the hull bounds the
    // surface instead.
    let mut input = TinInput::default();
    input.breaklines.push(Breakline {
        vertices: line.vertices.clone(),
        closed: line.closed,
    });
    let mut daylight_line = Breakline::default();
    for point in &result.daylights {
        input.points.push(point.origin);
        input.points.push(point.daylight);
        daylight_line.vertices.push(point.daylight);
        // At grade there is no batter, and a breakline of no length is
        // refused by the builder rather than quietly accepted.
        if point.daylight.distance_to(point.origin) > tolerance::GEOMETRIC {
            input.breaklines.push(Breakline {
                vertices: vec![point.origin, point.daylight],
                closed: false,
            });
        }
        result
            .daylight_line
            .vertices
            .push(Point2::new(point.daylight.x, point.daylight.y));
    }
    for &vertex in &line.vertices {
        input.points.push(vertex);
    }
    daylight_line.closed = line.closed;
    result.daylight_line.closed = line.closed;
    input.breaklines.push(daylight_line);

    if result.missed == 0 {
        let mut boundary = result.daylight_line.clone();
        if !line.closed {
            // An open line's grading is the strip between it and its
            // daylight: close the boundary back along the feature line.
            for vertex in line.vertices.iter().rev() {
                boundary.vertices.push(Point2::new(vertex.x, vertex.y));
            }
            boundary.closed = true;
        }
        let cleaned = boundary.without_duplicate_vertices();
        if cleaned.vertices.len() >= 3 && is_simple(&cleaned) {
            input.boundary = cleaned;
            result.bounded_by_daylight = true;
        }
    }

    let options = TinBuildOptions {
        // Batter lines meet the feature and daylight lines at their own ends;
        // where two mitred batters cross at a concave corner the mean is
        // right.
        duplicate_points: DuplicatePointPolicy::Average,
        crossing_breaklines: CrossingBreaklinePolicy::Average,
        ..TinBuildOptions::default()
    };
    let built = build_tin(&input, &options)?;
    result.surface = built.surface;

    let volumes = compare_surfaces(ground, &result.surface, None)?;
    result.cut = volumes.cut;
    result.fill = volumes.fill;
    result.plan_area = volumes.plan_area;
    Ok(result)
}
