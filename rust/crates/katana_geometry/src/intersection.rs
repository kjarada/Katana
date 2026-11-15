use crate::{Arc2, Circle2, Containment, Line2, Point2, Segment2, Vec2};
use katana_math::cpp::{max, min};
use katana_math::numerics::tolerance;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IntersectionKind {
    #[default]
    None,
    Points,
    Overlap,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IntersectionResult {
    pub kind: IntersectionKind,
    pub points: [Point2; 2],
    pub count: usize,
    pub tangent: bool,
}
impl IntersectionResult {
    pub fn exists(&self) -> bool {
        self.kind != IntersectionKind::None
    }
}
pub trait Intersect<Other = Self> {
    fn intersect(&self, other: &Other) -> IntersectionResult;
}
pub fn intersect<Left: Intersect<Right>, Right>(left: &Left, right: &Right) -> IntersectionResult {
    left.intersect(right)
}

fn one_point(point: Point2, tangent: bool) -> IntersectionResult {
    IntersectionResult {
        kind: IntersectionKind::Points,
        points: [point, Point2::default()],
        count: 1,
        tangent,
    }
}
fn two_points(first: Point2, second: Point2) -> IntersectionResult {
    IntersectionResult {
        kind: IntersectionKind::Points,
        points: [first, second],
        count: 2,
        tangent: false,
    }
}
fn overlap() -> IntersectionResult {
    IntersectionResult {
        kind: IntersectionKind::Overlap,
        ..Default::default()
    }
}
fn bounded_overlap(from: Point2, to: Point2) -> IntersectionResult {
    IntersectionResult {
        kind: IntersectionKind::Overlap,
        points: [from, to],
        count: 2,
        tangent: false,
    }
}
fn parallel(first: Vec2, second: Vec2) -> bool {
    !(first.cross(second).abs() > tolerance::ANGULAR * first.length() * second.length())
}
fn filter_points(
    candidates: IntersectionResult,
    keep: impl Fn(Point2) -> bool,
) -> IntersectionResult {
    if candidates.kind != IntersectionKind::Points {
        return candidates;
    }
    let mut result = IntersectionResult {
        tangent: candidates.tangent,
        ..Default::default()
    };
    for &point in &candidates.points[..candidates.count] {
        if keep(point) {
            result.points[result.count] = point;
            result.count += 1;
        }
    }
    result.kind = if result.count > 0 {
        IntersectionKind::Points
    } else {
        IntersectionKind::None
    };
    result
}
fn on_segment(segment: &Segment2, point: Point2) -> bool {
    segment.distance_to(point) <= tolerance::GEOMETRIC
}
fn on_arc(arc: &Arc2, point: Point2) -> bool {
    let angular_tolerance = if arc.radius > 0.0 {
        tolerance::GEOMETRIC / arc.radius
    } else {
        0.0
    };
    arc.contains_angle_with_tolerance((point - arc.center).angle(), angular_tolerance)
}
impl Intersect for Line2 {
    fn intersect(&self, other: &Self) -> IntersectionResult {
        if parallel(self.direction, other.direction) {
            return if self.distance_to(other.origin) <= tolerance::GEOMETRIC {
                overlap()
            } else {
                IntersectionResult::default()
            };
        }
        let parameter = (other.origin - self.origin).cross(other.direction)
            / self.direction.cross(other.direction);
        one_point(self.point_at(parameter), false)
    }
}
impl Intersect<Segment2> for Line2 {
    fn intersect(&self, segment: &Segment2) -> IntersectionResult {
        if segment.is_degenerate() {
            return if self.distance_to(segment.start) <= tolerance::GEOMETRIC {
                one_point(segment.start, false)
            } else {
                IntersectionResult::default()
            };
        }
        let infinite = intersect(
            self,
            &Line2 {
                origin: segment.start,
                direction: segment.delta(),
            },
        );
        if infinite.kind == IntersectionKind::Overlap {
            return bounded_overlap(segment.start, segment.end);
        }
        filter_points(infinite, |point| on_segment(segment, point))
    }
}
impl Intersect for Segment2 {
    fn intersect(&self, other: &Self) -> IntersectionResult {
        if self.is_degenerate() {
            return if on_segment(other, self.start) {
                one_point(self.start, false)
            } else {
                IntersectionResult::default()
            };
        }
        if other.is_degenerate() {
            return if on_segment(self, other.start) {
                one_point(other.start, false)
            } else {
                IntersectionResult::default()
            };
        }
        let first_delta = self.delta();
        let second_delta = other.delta();
        let offset = other.start - self.start;
        if parallel(first_delta, second_delta) {
            let carrier = Line2 {
                origin: self.start,
                direction: first_delta,
            };
            if carrier.distance_to(other.start) > tolerance::GEOMETRIC
                || carrier.distance_to(other.end) > tolerance::GEOMETRIC
            {
                return IntersectionResult::default();
            }
            let length = first_delta.length();
            let first = carrier.parameter_of(other.start);
            let second = carrier.parameter_of(other.end);
            let low = max(0.0, min(first, second));
            let high = min(1.0, max(first, second));
            let epsilon = tolerance::GEOMETRIC / length;
            if low > high + epsilon {
                return IntersectionResult::default();
            }
            if high - low <= epsilon {
                return one_point(self.point_at((0.5 * (low + high)).clamp(0.0, 1.0)), false);
            }
            return bounded_overlap(self.point_at(low), self.point_at(high));
        }
        let denominator = first_delta.cross(second_delta);
        let parameter = offset.cross(second_delta) / denominator;
        let point = self.start + first_delta * parameter;
        if !on_segment(self, point) || !on_segment(other, point) {
            return IntersectionResult::default();
        }
        one_point(self.point_at(parameter.clamp(0.0, 1.0)), false)
    }
}
impl Intersect<Circle2> for Line2 {
    fn intersect(&self, circle: &Circle2) -> IntersectionResult {
        let unit = self.direction.normalized();
        if unit == Vec2::default() {
            return if circle.classify(self.origin) == Containment::OnBoundary {
                one_point(self.origin, false)
            } else {
                IntersectionResult::default()
            };
        }
        let foot = self.closest_point(circle.center);
        let distance = foot.distance_to(circle.center);
        if distance > circle.radius + tolerance::GEOMETRIC {
            return IntersectionResult::default();
        }
        if (distance - circle.radius).abs() <= tolerance::GEOMETRIC {
            return one_point(foot, true);
        }
        let half_chord = ((circle.radius - distance) * (circle.radius + distance)).sqrt();
        two_points(foot - unit * half_chord, foot + unit * half_chord)
    }
}
impl Intersect<Circle2> for Segment2 {
    fn intersect(&self, circle: &Circle2) -> IntersectionResult {
        if self.is_degenerate() {
            return if circle.classify(self.start) == Containment::OnBoundary {
                one_point(self.start, false)
            } else {
                IntersectionResult::default()
            };
        }
        let infinite = intersect(
            &Line2 {
                origin: self.start,
                direction: self.delta(),
            },
            circle,
        );
        filter_points(infinite, |point| on_segment(self, point))
    }
}
impl Intersect for Circle2 {
    fn intersect(&self, other: &Self) -> IntersectionResult {
        let between = other.center - self.center;
        let distance = between.length();
        let radius_difference = (self.radius - other.radius).abs();
        if distance <= tolerance::GEOMETRIC {
            return if radius_difference <= tolerance::GEOMETRIC {
                overlap()
            } else {
                IntersectionResult::default()
            };
        }
        if distance > self.radius + other.radius + tolerance::GEOMETRIC
            || distance < radius_difference - tolerance::GEOMETRIC
        {
            return IntersectionResult::default();
        }
        let unit = between / distance;
        if (distance - (self.radius + other.radius)).abs() <= tolerance::GEOMETRIC {
            return one_point(self.center + unit * self.radius, true);
        }
        if (distance - radius_difference).abs() <= tolerance::GEOMETRIC {
            let direction = if self.radius >= other.radius {
                1.0
            } else {
                -1.0
            };
            return one_point(self.center + unit * (direction * self.radius), true);
        }
        let along = (distance * distance + self.radius * self.radius - other.radius * other.radius)
            / (2.0 * distance);
        let half_chord = max(0.0, (self.radius - along) * (self.radius + along)).sqrt();
        let foot = self.center + unit * along;
        let normal = unit.perpendicular();
        two_points(foot - normal * half_chord, foot + normal * half_chord)
    }
}
impl Intersect<Arc2> for Line2 {
    fn intersect(&self, arc: &Arc2) -> IntersectionResult {
        filter_points(intersect(self, &arc.circle()), |point| on_arc(arc, point))
    }
}
impl Intersect<Arc2> for Segment2 {
    fn intersect(&self, arc: &Arc2) -> IntersectionResult {
        filter_points(intersect(self, &arc.circle()), |point| on_arc(arc, point))
    }
}
impl Intersect<Arc2> for Circle2 {
    fn intersect(&self, arc: &Arc2) -> IntersectionResult {
        let full = intersect(self, &arc.circle());
        if full.kind == IntersectionKind::Overlap {
            return bounded_overlap(arc.start_point(), arc.end_point());
        }
        filter_points(full, |point| on_arc(arc, point))
    }
}
impl Intersect for Arc2 {
    fn intersect(&self, other: &Self) -> IntersectionResult {
        let full = intersect(&self.circle(), &other.circle());
        if full.kind != IntersectionKind::Overlap {
            return filter_points(full, |point| on_arc(self, point) && on_arc(other, point));
        }
        let strictly_inside = |arc: &Arc2, point: Point2| {
            on_arc(arc, point)
                && point.distance_to(arc.start_point()) > tolerance::GEOMETRIC
                && point.distance_to(arc.end_point()) > tolerance::GEOMETRIC
        };
        let same_ends = (self.start_point().distance_to(other.start_point())
            <= tolerance::GEOMETRIC
            && self.end_point().distance_to(other.end_point()) <= tolerance::GEOMETRIC)
            || (self.start_point().distance_to(other.end_point()) <= tolerance::GEOMETRIC
                && self.end_point().distance_to(other.start_point()) <= tolerance::GEOMETRIC);
        let same_middle = on_arc(other, self.midpoint());
        if strictly_inside(self, other.start_point())
            || strictly_inside(self, other.end_point())
            || strictly_inside(other, self.start_point())
            || strictly_inside(other, self.end_point())
            || (same_ends && same_middle)
        {
            return overlap();
        }
        let mut touching = IntersectionResult::default();
        for end in [other.start_point(), other.end_point()] {
            let already_listed =
                touching.count == 1 && touching.points[0].distance_to(end) <= tolerance::GEOMETRIC;
            if on_arc(self, end) && !already_listed && touching.count < 2 {
                touching.points[touching.count] = end;
                touching.count += 1;
            }
        }
        touching.kind = if touching.count > 0 {
            IntersectionKind::Points
        } else {
            IntersectionKind::None
        };
        touching
    }
}
macro_rules! symmetric_intersection {
    ($first:ty, $second:ty) => {
        impl Intersect<$second> for $first {
            fn intersect(&self, other: &$second) -> IntersectionResult {
                intersect(other, self)
            }
        }
    };
}
symmetric_intersection!(Segment2, Line2);
symmetric_intersection!(Circle2, Line2);
symmetric_intersection!(Circle2, Segment2);
symmetric_intersection!(Arc2, Line2);
symmetric_intersection!(Arc2, Segment2);
symmetric_intersection!(Arc2, Circle2);
