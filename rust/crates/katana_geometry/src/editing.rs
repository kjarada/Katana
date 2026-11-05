use crate::intersection::Intersect;
pub use crate::intersection::intersect;
use crate::{
    Arc2, Box2, Circle2, IntersectionKind, IntersectionResult, Line2, Point2, Polyline2, Segment2,
    Vec2,
};
use katana_core::error::{Error, ErrorCode, Result};
use katana_core::text::format_exact_real;
use katana_math::numerics::{TWO_PI, normalize_angle, normalize_angle_signed, tolerance};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve2 {
    Segment(Segment2),
    Arc(Arc2),
    Circle(Circle2),
}
impl From<Segment2> for Curve2 {
    fn from(value: Segment2) -> Self {
        Self::Segment(value)
    }
}
impl From<Arc2> for Curve2 {
    fn from(value: Arc2) -> Self {
        Self::Arc(value)
    }
}
impl From<Circle2> for Curve2 {
    fn from(value: Circle2) -> Self {
        Self::Circle(value)
    }
}
impl Intersect for Curve2 {
    fn intersect(&self, other: &Self) -> IntersectionResult {
        match (self, other) {
            (Self::Segment(first), Self::Segment(second)) => intersect(first, second),
            (Self::Segment(first), Self::Arc(second)) => intersect(first, second),
            (Self::Segment(first), Self::Circle(second)) => intersect(first, second),
            (Self::Arc(first), Self::Segment(second)) => intersect(first, second),
            (Self::Arc(first), Self::Arc(second)) => intersect(first, second),
            (Self::Arc(first), Self::Circle(second)) => intersect(first, second),
            (Self::Circle(first), Self::Segment(second)) => intersect(first, second),
            (Self::Circle(first), Self::Arc(second)) => intersect(first, second),
            (Self::Circle(first), Self::Circle(second)) => intersect(first, second),
        }
    }
}
pub fn closest_point(curve: &Curve2, point: Point2) -> Point2 {
    match curve {
        Curve2::Segment(piece) => piece.closest_point(point),
        Curve2::Arc(piece) => piece.closest_point(point),
        Curve2::Circle(piece) => piece.closest_point(point),
    }
}
pub fn distance(curve: &Curve2, point: Point2) -> f64 {
    match curve {
        Curve2::Segment(piece) => piece.distance_to(point),
        Curve2::Arc(piece) => piece.distance_to(point),
        Curve2::Circle(piece) => piece.distance_to(point),
    }
}
pub fn bounding_box(curve: &Curve2) -> Box2 {
    match curve {
        Curve2::Segment(piece) => piece.bounding_box(),
        Curve2::Arc(piece) => piece.bounding_box(),
        Curve2::Circle(piece) => piece.bounding_box(),
    }
}

pub trait Offset {
    type Output;
    fn offset(&self, distance: f64) -> Self::Output;
}
pub fn offset<Shape: Offset>(shape: &Shape, distance: f64) -> Shape::Output {
    shape.offset(distance)
}
impl Offset for Segment2 {
    type Output = Segment2;
    fn offset(&self, distance: f64) -> Self::Output {
        let shift = self.delta().normalized().perpendicular() * distance;
        Segment2::new(self.start + shift, self.end + shift)
    }
}
impl Offset for Circle2 {
    type Output = Option<Circle2>;
    fn offset(&self, distance: f64) -> Self::Output {
        let radius = self.radius + distance;
        if !(radius > tolerance::GEOMETRIC) {
            None
        } else {
            Some(Circle2::new(self.center, radius))
        }
    }
}
impl Offset for Arc2 {
    type Output = Option<Arc2>;
    fn offset(&self, distance: f64) -> Self::Output {
        let radius = self.radius + distance;
        if !(radius > tolerance::GEOMETRIC) {
            None
        } else {
            Some(Arc2::new(self.center, radius, self.start_angle, self.sweep))
        }
    }
}
impl Offset for Polyline2 {
    type Output = Result<Polyline2>;
    fn offset(&self, distance: f64) -> Self::Output {
        let clean = self.without_duplicate_vertices();
        let count = clean.vertices.len();
        if count < 2 || (clean.closed && count < 3) {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "polyline has too few distinct vertices to offset",
            )
            .with_context(format!("vertices={count}")));
        }
        let offset_line = |index| {
            let moved = offset(&clean.segment(index), distance);
            Line2 {
                origin: moved.start,
                direction: moved.delta(),
            }
        };
        let mut result = Polyline2::new(Vec::with_capacity(count), clean.closed);
        let segments = clean.segment_count();
        for index in 0..count {
            let has_incoming = clean.closed || index > 0;
            let has_outgoing = clean.closed || index + 1 < count;
            if has_incoming && has_outgoing {
                let incoming = offset_line((index + segments - 1) % segments);
                let outgoing = offset_line(index % segments);
                let corner = intersect(&incoming, &outgoing);
                if corner.kind == IntersectionKind::Points {
                    result.vertices.push(corner.points[0]);
                    continue;
                }
                if corner.kind == IntersectionKind::None {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "polyline reverses direction at a vertex; offset is undefined",
                    )
                    .with_context(format!("vertex={index}")));
                }
                result.vertices.push(outgoing.origin);
            } else if has_outgoing {
                result.vertices.push(offset_line(index).origin);
            } else {
                result
                    .vertices
                    .push(offset(&clean.segment(index - 1), distance).end);
            }
        }
        Ok(result)
    }
}
pub fn offset_towards(curve: &Curve2, distance: f64, side: Point2) -> Result<Curve2> {
    let magnitude = distance.abs();
    if !(magnitude > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "offset distance must be non-zero",
        ));
    }
    match curve {
        Curve2::Segment(segment) => {
            if segment.is_degenerate() {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "cannot offset a zero-length segment",
                ));
            }
            let side_sign = if (Line2 {
                origin: segment.start,
                direction: segment.delta(),
            })
            .signed_distance_to(side)
                >= 0.0
            {
                1.0
            } else {
                -1.0
            };
            Ok(Curve2::Segment(offset(segment, side_sign * magnitude)))
        }
        Curve2::Circle(circle) => {
            let outward = circle.center.distance_to(side) >= circle.radius;
            offset(circle, if outward { magnitude } else { -magnitude })
                .map(Curve2::Circle)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::InvalidGeometry,
                        "offset distance is not smaller than the circle radius",
                    )
                })
        }
        Curve2::Arc(arc) => {
            let outward = arc.center.distance_to(side) >= arc.radius;
            offset(arc, if outward { magnitude } else { -magnitude })
                .map(Curve2::Arc)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::InvalidGeometry,
                        "offset distance is not smaller than the arc radius",
                    )
                })
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrimResult {
    pub remaining: Vec<Curve2>,
}
fn crossing_points(target: &Curve2, others: &[Curve2]) -> Vec<Point2> {
    let mut points = Vec::new();
    for other in others {
        let hit = intersect(target, other);
        if hit.kind == IntersectionKind::Points {
            points.extend_from_slice(&hit.points[..hit.count]);
        }
    }
    points
}
fn sort_unique(values: &mut Vec<f64>, epsilon: f64) {
    values.sort_by(|first, second| {
        first
            .partial_cmp(second)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    values.dedup_by(|current, previous| *current - *previous <= epsilon);
}
fn trim_open(
    mut cuts: Vec<f64>,
    pick_parameter: f64,
    epsilon: f64,
    piece: impl Fn(f64, f64) -> Curve2,
) -> Result<TrimResult> {
    cuts.retain(|&parameter| !(parameter <= epsilon || parameter >= 1.0 - epsilon));
    sort_unique(&mut cuts, epsilon);
    if cuts.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "no cutting edge crosses the interior of the target",
        ));
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for cut in cuts {
        if cut <= pick_parameter {
            low = cut;
        } else {
            high = cut;
            break;
        }
    }
    let mut result = TrimResult::default();
    if low > 0.0 {
        result.remaining.push(piece(0.0, low));
    }
    if high < 1.0 {
        result.remaining.push(piece(high, 1.0));
    }
    Ok(result)
}
pub fn trim(target: &Curve2, cutters: &[Curve2], pick: Point2) -> Result<TrimResult> {
    let crossings = crossing_points(target, cutters);
    match target {
        Curve2::Segment(segment) => {
            if segment.is_degenerate() {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "cannot trim a zero-length segment",
                ));
            }
            let cuts = crossings
                .iter()
                .map(|&point| segment.parameter_of(point))
                .collect();
            trim_open(
                cuts,
                segment.parameter_of(pick),
                tolerance::GEOMETRIC / segment.length(),
                |from, to| {
                    Curve2::Segment(Segment2::new(segment.point_at(from), segment.point_at(to)))
                },
            )
        }
        Curve2::Arc(arc) => {
            if !(arc.length() > tolerance::GEOMETRIC) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "cannot trim a zero-length arc",
                ));
            }
            let cuts = crossings
                .iter()
                .map(|&point| arc.parameter_of_angle((point - arc.center).angle()))
                .collect();
            let pick_parameter = arc.parameter_of_angle((pick - arc.center).angle());
            trim_open(
                cuts,
                pick_parameter,
                tolerance::GEOMETRIC / arc.length(),
                |from, to| {
                    Curve2::Arc(Arc2::new(
                        arc.center,
                        arc.radius,
                        arc.start_angle + arc.sweep * from,
                        arc.sweep * (to - from),
                    ))
                },
            )
        }
        Curve2::Circle(circle) => {
            if !(circle.radius > tolerance::GEOMETRIC) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "cannot trim a zero-radius circle",
                ));
            }
            let mut angles: Vec<f64> = crossings
                .iter()
                .map(|&point| normalize_angle((point - circle.center).angle()))
                .collect();
            let epsilon = tolerance::GEOMETRIC / circle.radius;
            sort_unique(&mut angles, epsilon);
            if angles.len() > 1 && angles[0] + TWO_PI - angles[angles.len() - 1] <= epsilon {
                angles.pop();
            }
            if angles.len() < 2 {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "a circle needs at least two crossings to be trimmed",
                ));
            }
            let pick_angle = normalize_angle((pick - circle.center).angle());
            let mut high_index = 0;
            while high_index < angles.len() && angles[high_index] <= pick_angle {
                high_index += 1;
            }
            let high = angles[high_index % angles.len()];
            let low = angles[(high_index + angles.len() - 1) % angles.len()];
            Ok(TrimResult {
                remaining: vec![Curve2::Arc(Arc2::new(
                    circle.center,
                    circle.radius,
                    high,
                    normalize_angle(low - high),
                ))],
            })
        }
    }
}

fn extend_segment(segment: &Segment2, boundaries: &[Curve2], pick: Point2) -> Result<Segment2> {
    if segment.is_degenerate() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "cannot extend a zero-length segment",
        ));
    }
    let extend_end = pick.distance_to(segment.end) <= pick.distance_to(segment.start);
    let moving = if extend_end {
        segment.end
    } else {
        segment.start
    };
    let fixed = if extend_end {
        segment.start
    } else {
        segment.end
    };
    let ray = Line2 {
        origin: moving,
        direction: (moving - fixed).normalized(),
    };
    let mut nearest: Option<f64> = None;
    for boundary in boundaries {
        let hit = match boundary {
            Curve2::Segment(piece) => intersect(&ray, piece),
            Curve2::Arc(piece) => intersect(&ray, piece),
            Curve2::Circle(piece) => intersect(&ray, piece),
        };
        if hit.kind != IntersectionKind::Points {
            continue;
        }
        for &point in &hit.points[..hit.count] {
            let parameter = ray.parameter_of(point);
            if parameter > tolerance::GEOMETRIC && nearest.is_none_or(|current| parameter < current)
            {
                nearest = Some(parameter);
            }
        }
    }
    let nearest = nearest.ok_or_else(|| {
        Error::new(
            ErrorCode::InvalidGeometry,
            "no boundary lies ahead of the segment end",
        )
    })?;
    let reached = ray.point_at(nearest);
    Ok(if extend_end {
        Segment2::new(segment.start, reached)
    } else {
        Segment2::new(reached, segment.end)
    })
}
fn extend_arc_end(arc: &Arc2, boundaries: &[Curve2]) -> Result<Arc2> {
    if !(arc.radius > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "cannot extend a zero-radius arc",
        ));
    }
    let epsilon = tolerance::GEOMETRIC / arc.radius;
    let available = TWO_PI - arc.sweep.abs();
    let full_circle = Curve2::Circle(arc.circle());
    let mut nearest: Option<f64> = None;
    for point in crossing_points(&full_circle, boundaries) {
        let angle = (point - arc.center).angle();
        let extra = if arc.sweep >= 0.0 {
            normalize_angle(angle - arc.end_angle())
        } else {
            normalize_angle(arc.end_angle() - angle)
        };
        if extra > epsilon && extra <= available && nearest.is_none_or(|current| extra < current) {
            nearest = Some(extra);
        }
    }
    let nearest = nearest.ok_or_else(|| {
        Error::new(
            ErrorCode::InvalidGeometry,
            "no boundary lies ahead of the arc end",
        )
    })?;
    let direction = if arc.sweep >= 0.0 { 1.0 } else { -1.0 };
    Ok(Arc2::new(
        arc.center,
        arc.radius,
        arc.start_angle,
        arc.sweep + direction * nearest,
    ))
}
pub fn extend(target: &Curve2, boundaries: &[Curve2], pick: Point2) -> Result<Curve2> {
    match target {
        Curve2::Segment(segment) => extend_segment(segment, boundaries, pick).map(Curve2::Segment),
        Curve2::Arc(arc) => {
            let extend_end =
                pick.distance_to(arc.end_point()) <= pick.distance_to(arc.start_point());
            let extended =
                extend_arc_end(&if extend_end { *arc } else { arc.reversed() }, boundaries)?;
            Ok(Curve2::Arc(if extend_end {
                extended
            } else {
                extended.reversed()
            }))
        }
        Curve2::Circle(_) => Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a circle has no end to extend",
        )),
    }
}

struct Corner {
    apex: Point2,
    far_first: Point2,
    far_second: Point2,
    along_first: Vec2,
    along_second: Vec2,
    reach_first: f64,
    reach_second: f64,
    first_ends_at_apex: bool,
    second_ends_at_apex: bool,
}
fn find_corner(first: &Segment2, second: &Segment2) -> Result<Corner> {
    if first.is_degenerate() || second.is_degenerate() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "segments must have non-zero length",
        ));
    }
    let apex = intersect(
        &Line2 {
            origin: first.start,
            direction: first.delta(),
        },
        &Line2 {
            origin: second.start,
            direction: second.delta(),
        },
    );
    if apex.kind != IntersectionKind::Points {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "segments are parallel; they have no corner to modify",
        ));
    }
    let apex = apex.points[0];
    let first_ends_at_apex = apex.distance_to(first.end) <= apex.distance_to(first.start);
    let second_ends_at_apex = apex.distance_to(second.end) <= apex.distance_to(second.start);
    let far_first = if first_ends_at_apex {
        first.start
    } else {
        first.end
    };
    let far_second = if second_ends_at_apex {
        second.start
    } else {
        second.end
    };
    let reach_first = apex.distance_to(far_first);
    let reach_second = apex.distance_to(far_second);
    if reach_first <= tolerance::GEOMETRIC || reach_second <= tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a segment ends exactly at the corner",
        ));
    }
    Ok(Corner {
        apex,
        far_first,
        far_second,
        reach_first,
        reach_second,
        first_ends_at_apex,
        second_ends_at_apex,
        along_first: (far_first - apex) / reach_first,
        along_second: (far_second - apex) / reach_second,
    })
}
fn rebuilt(far_end: Point2, corner_end: Point2, ends_at_apex: bool) -> Segment2 {
    if ends_at_apex {
        Segment2::new(far_end, corner_end)
    } else {
        Segment2::new(corner_end, far_end)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FilletResult {
    pub first: Segment2,
    pub second: Segment2,
    pub arc: Option<Arc2>,
}
pub fn fillet(first: &Segment2, second: &Segment2, radius: f64) -> Result<FilletResult> {
    if !(radius >= 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "fillet radius must not be negative",
        )
        .with_context(format!("radius={}", format_exact_real(radius))));
    }
    let corner = find_corner(first, second)?;
    if radius <= tolerance::GEOMETRIC {
        return Ok(FilletResult {
            first: rebuilt(corner.far_first, corner.apex, corner.first_ends_at_apex),
            second: rebuilt(corner.far_second, corner.apex, corner.second_ends_at_apex),
            arc: None,
        });
    }
    let cos_angle = corner.along_first.dot(corner.along_second).clamp(-1.0, 1.0);
    let half_angle = 0.5 * cos_angle.acos();
    let tangent_distance = radius / half_angle.tan();
    if !(corner.reach_first - tangent_distance > tolerance::GEOMETRIC)
        || !(corner.reach_second - tangent_distance > tolerance::GEOMETRIC)
    {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "fillet radius is too large for the segments",
        )
        .with_context(format!("radius={}", format_exact_real(radius))));
    }
    let tangent_first = corner.apex + corner.along_first * tangent_distance;
    let tangent_second = corner.apex + corner.along_second * tangent_distance;
    let bisector = (corner.along_first + corner.along_second).normalized();
    let center = corner.apex + bisector * (radius / half_angle.sin());
    let start_angle = (tangent_first - center).angle();
    let sweep = normalize_angle_signed((tangent_second - center).angle() - start_angle);
    Ok(FilletResult {
        first: rebuilt(corner.far_first, tangent_first, corner.first_ends_at_apex),
        second: rebuilt(
            corner.far_second,
            tangent_second,
            corner.second_ends_at_apex,
        ),
        arc: Some(Arc2::new(center, radius, start_angle, sweep)),
    })
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChamferResult {
    pub first: Segment2,
    pub second: Segment2,
    pub bevel: Option<Segment2>,
}
pub fn chamfer(
    first: &Segment2,
    second: &Segment2,
    distance_first: f64,
    distance_second: f64,
) -> Result<ChamferResult> {
    if !(distance_first >= 0.0) || !(distance_second >= 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "chamfer distances must not be negative",
        ));
    }
    let corner = find_corner(first, second)?;
    if !(corner.reach_first - distance_first > tolerance::GEOMETRIC)
        || !(corner.reach_second - distance_second > tolerance::GEOMETRIC)
    {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "chamfer distance leaves no segment behind",
        ));
    }
    let cut_first = corner.apex + corner.along_first * distance_first;
    let cut_second = corner.apex + corner.along_second * distance_second;
    let bevel = if cut_first.distance_to(cut_second) > tolerance::GEOMETRIC {
        Some(Segment2::new(cut_first, cut_second))
    } else {
        None
    };
    Ok(ChamferResult {
        first: rebuilt(corner.far_first, cut_first, corner.first_ends_at_apex),
        second: rebuilt(corner.far_second, cut_second, corner.second_ends_at_apex),
        bevel,
    })
}
