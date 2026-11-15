use katana_math::cpp::{max, min};
use katana_math::numerics::{HALF_PI, PI, TWO_PI, normalize_angle, tolerance};
pub use katana_math::vec2::Vec2;
pub type Point2 = Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Containment {
    Outside,
    OnBoundary,
    Inside,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box2 {
    pub min: Point2,
    pub max: Point2,
}

impl Default for Box2 {
    fn default() -> Self {
        Self::new(
            Point2::new(f64::INFINITY, f64::INFINITY),
            Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        )
    }
}

impl Box2 {
    pub fn new(min: Point2, max: Point2) -> Self {
        Self { min, max }
    }

    pub fn empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn width(&self) -> f64 {
        if self.empty() {
            0.0
        } else {
            self.max.x - self.min.x
        }
    }

    pub fn height(&self) -> f64 {
        if self.empty() {
            0.0
        } else {
            self.max.y - self.min.y
        }
    }

    pub fn center(&self) -> Point2 {
        (self.min + self.max) * 0.5
    }

    pub fn expand(&mut self, point: Point2) {
        if point.x < self.min.x {
            self.min.x = point.x;
        }
        if point.y < self.min.y {
            self.min.y = point.y;
        }
        if self.max.x < point.x {
            self.max.x = point.x;
        }
        if self.max.y < point.y {
            self.max.y = point.y;
        }
    }

    pub fn expand_box(&mut self, other: Self) {
        if !other.empty() {
            self.expand(other.min);
            self.expand(other.max);
        }
    }

    pub fn inflated(&self, margin: f64) -> Self {
        if self.empty() {
            Self::default()
        } else {
            Self::new(
                Point2::new(self.min.x - margin, self.min.y - margin),
                Point2::new(self.max.x + margin, self.max.y + margin),
            )
        }
    }

    pub fn contains(&self, point: Point2) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    pub fn contains_box(&self, other: Self) -> bool {
        !other.empty() && self.contains(other.min) && self.contains(other.max)
    }

    pub fn intersects(&self, other: Self) -> bool {
        !self.empty()
            && !other.empty()
            && self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line2 {
    pub origin: Point2,
    pub direction: Vec2,
}

impl Default for Line2 {
    fn default() -> Self {
        Self {
            origin: Point2::default(),
            direction: Vec2::new(1.0, 0.0),
        }
    }
}
impl Line2 {
    pub fn through(first: Point2, second: Point2) -> Option<Self> {
        if first.distance_to(second) <= tolerance::GEOMETRIC {
            None
        } else {
            Some(Self {
                origin: first,
                direction: second - first,
            })
        }
    }
    pub fn point_at(&self, parameter: f64) -> Point2 {
        self.origin + self.direction * parameter
    }
    pub fn parameter_of(&self, point: Point2) -> f64 {
        let squared = self.direction.length_squared();
        if squared > 0.0 {
            (point - self.origin).dot(self.direction) / squared
        } else {
            0.0
        }
    }
    pub fn closest_point(&self, point: Point2) -> Point2 {
        self.point_at(self.parameter_of(point))
    }
    pub fn signed_distance_to(&self, point: Point2) -> f64 {
        let length = self.direction.length();
        if length > 0.0 {
            self.direction.cross(point - self.origin) / length
        } else {
            0.0
        }
    }
    pub fn distance_to(&self, point: Point2) -> f64 {
        if self.direction.length_squared() > 0.0 {
            self.signed_distance_to(point).abs()
        } else {
            self.origin.distance_to(point)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment2 {
    pub start: Point2,
    pub end: Point2,
}
impl Segment2 {
    pub fn new(start: Point2, end: Point2) -> Self {
        Self { start, end }
    }
    pub fn delta(&self) -> Vec2 {
        self.end - self.start
    }
    pub fn length(&self) -> f64 {
        self.start.distance_to(self.end)
    }
    pub fn midpoint(&self) -> Point2 {
        (self.start + self.end) * 0.5
    }
    pub fn point_at(&self, parameter: f64) -> Point2 {
        self.start + (self.end - self.start) * parameter
    }
    pub fn reversed(&self) -> Self {
        Self::new(self.end, self.start)
    }
    pub fn is_degenerate(&self) -> bool {
        self.length() <= tolerance::GEOMETRIC
    }
    pub fn parameter_of(&self, point: Point2) -> f64 {
        let delta = self.delta();
        let squared = delta.length_squared();
        if !(squared > 0.0) {
            return 0.0;
        }
        ((point - self.start).dot(delta) / squared).clamp(0.0, 1.0)
    }
    pub fn closest_point(&self, point: Point2) -> Point2 {
        self.point_at(self.parameter_of(point))
    }
    pub fn distance_to(&self, point: Point2) -> f64 {
        self.closest_point(point).distance_to(point)
    }
    pub fn bounding_box(&self) -> Box2 {
        let mut bounds = Box2::default();
        bounds.expand(self.start);
        bounds.expand(self.end);
        bounds
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Circle2 {
    pub center: Point2,
    pub radius: f64,
}
impl Circle2 {
    pub fn new(center: Point2, radius: f64) -> Self {
        Self { center, radius }
    }
    pub fn area(&self) -> f64 {
        PI * self.radius * self.radius
    }
    pub fn perimeter(&self) -> f64 {
        TWO_PI * self.radius
    }
    pub fn point_at_angle(&self, radians: f64) -> Point2 {
        self.center + Vec2::new(radians.cos(), radians.sin()) * self.radius
    }
    pub fn classify(&self, point: Point2) -> Containment {
        let offset = self.center.distance_to(point) - self.radius;
        if offset.abs() <= tolerance::GEOMETRIC {
            Containment::OnBoundary
        } else if offset < 0.0 {
            Containment::Inside
        } else {
            Containment::Outside
        }
    }
    pub fn contains(&self, point: Point2) -> bool {
        self.classify(point) != Containment::Outside
    }
    pub fn closest_point(&self, point: Point2) -> Point2 {
        let direction = (point - self.center).normalized();
        if direction == Vec2::default() {
            self.point_at_angle(0.0)
        } else {
            self.center + direction * self.radius
        }
    }
    pub fn distance_to(&self, point: Point2) -> f64 {
        (self.center.distance_to(point) - self.radius).abs()
    }
    pub fn bounding_box(&self) -> Box2 {
        Box2::new(
            Point2::new(self.center.x - self.radius, self.center.y - self.radius),
            Point2::new(self.center.x + self.radius, self.center.y + self.radius),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Arc2 {
    pub center: Point2,
    pub radius: f64,
    pub start_angle: f64,
    pub sweep: f64,
}
impl Arc2 {
    pub fn new(center: Point2, radius: f64, start_angle: f64, sweep: f64) -> Self {
        Self {
            center,
            radius,
            start_angle,
            sweep,
        }
    }
    pub fn through_points(first: Point2, second: Point2, third: Point2) -> Option<Self> {
        let circle = Triangle2::new(first, second, third).circumcircle()?;
        let first_delta = first - circle.center;
        let third_delta = third - circle.center;
        let first_angle = first_delta.y.atan2(first_delta.x);
        let third_angle = third_delta.y.atan2(third_delta.x);
        let sweep = if (second - first).cross(third - first) > 0.0 {
            normalize_angle(third_angle - first_angle)
        } else {
            -normalize_angle(first_angle - third_angle)
        };
        Some(Self::new(circle.center, circle.radius, first_angle, sweep))
    }
    pub fn end_angle(&self) -> f64 {
        self.start_angle + self.sweep
    }
    pub fn length(&self) -> f64 {
        self.sweep.abs() * self.radius
    }
    pub fn circle(&self) -> Circle2 {
        Circle2::new(self.center, self.radius)
    }
    pub fn point_at_angle(&self, radians: f64) -> Point2 {
        self.center + Vec2::new(radians.cos(), radians.sin()) * self.radius
    }
    pub fn point_at(&self, parameter: f64) -> Point2 {
        self.point_at_angle(self.start_angle + self.sweep * parameter)
    }
    pub fn start_point(&self) -> Point2 {
        self.point_at_angle(self.start_angle)
    }
    pub fn end_point(&self) -> Point2 {
        self.point_at_angle(self.end_angle())
    }
    pub fn midpoint(&self) -> Point2 {
        self.point_at(0.5)
    }
    pub fn reversed(&self) -> Self {
        Self::new(self.center, self.radius, self.end_angle(), -self.sweep)
    }
    fn travelled_angle(&self, radians: f64) -> f64 {
        if self.sweep >= 0.0 {
            normalize_angle(radians - self.start_angle)
        } else {
            normalize_angle(self.start_angle - radians)
        }
    }
    pub fn contains_angle(&self, radians: f64) -> bool {
        self.contains_angle_with_tolerance(radians, 0.0)
    }
    pub fn contains_angle_with_tolerance(&self, radians: f64, angular_tolerance: f64) -> bool {
        let travelled = self.travelled_angle(radians);
        travelled <= self.sweep.abs() + angular_tolerance || travelled >= TWO_PI - angular_tolerance
    }
    pub fn parameter_of_angle(&self, radians: f64) -> f64 {
        let span = self.sweep.abs();
        if !(span > 0.0) {
            return 0.0;
        }
        let travelled = self.travelled_angle(radians);
        if travelled <= span {
            return travelled / span;
        }
        if (travelled - span) < (TWO_PI - travelled) {
            1.0
        } else {
            0.0
        }
    }
    pub fn closest_point(&self, point: Point2) -> Point2 {
        let offset = point - self.center;
        if offset == Vec2::default() {
            self.start_point()
        } else {
            self.point_at(self.parameter_of_angle(offset.y.atan2(offset.x)))
        }
    }
    pub fn distance_to(&self, point: Point2) -> f64 {
        self.closest_point(point).distance_to(point)
    }
    pub fn bounding_box(&self) -> Box2 {
        let mut bounds = Box2::default();
        bounds.expand(self.start_point());
        bounds.expand(self.end_point());
        for quadrant in 0..4 {
            let cardinal = HALF_PI * quadrant as f64;
            if self.contains_angle(cardinal) {
                bounds.expand(self.point_at_angle(cardinal));
            }
        }
        bounds
    }
}

fn edge_crosses_ray(first: Point2, second: Point2, point: Point2) -> bool {
    if (first.y > point.y) == (second.y > point.y) {
        return false;
    }
    let at_height = first.x + (point.y - first.y) / (second.y - first.y) * (second.x - first.x);
    point.x < at_height
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polyline2 {
    pub vertices: Vec<Point2>,
    pub closed: bool,
}
impl Polyline2 {
    pub fn new(vertices: Vec<Point2>, closed: bool) -> Self {
        Self { vertices, closed }
    }
    pub fn segment_count(&self) -> usize {
        if self.vertices.len() < 2 {
            0
        } else if self.closed {
            self.vertices.len()
        } else {
            self.vertices.len() - 1
        }
    }
    pub fn segment(&self, index: usize) -> Segment2 {
        Segment2::new(
            self.vertices[index],
            self.vertices[(index + 1) % self.vertices.len()],
        )
    }
    pub fn length(&self) -> f64 {
        let mut total = 0.0;
        for index in 0..self.segment_count() {
            total += self.segment(index).length();
        }
        total
    }
    pub fn perimeter(&self) -> f64 {
        self.length()
    }
    pub fn signed_area(&self) -> f64 {
        if !self.closed || self.vertices.len() < 3 {
            return 0.0;
        }
        let origin = self.vertices[0];
        let mut twice_area = 0.0;
        for index in 0..self.vertices.len() {
            let current = self.vertices[index] - origin;
            let next = self.vertices[(index + 1) % self.vertices.len()] - origin;
            twice_area += current.cross(next);
        }
        0.5 * twice_area
    }
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }
    pub fn centroid(&self) -> Option<Point2> {
        if !self.closed || self.vertices.len() < 3 {
            return None;
        }
        let origin = self.vertices[0];
        let mut twice_area = 0.0;
        let mut moment = Vec2::default();
        let mut scale = 0.0;
        for index in 0..self.vertices.len() {
            let current = self.vertices[index] - origin;
            let next = self.vertices[(index + 1) % self.vertices.len()] - origin;
            let cross = current.cross(next);
            twice_area += cross;
            moment += (current + next) * cross;
            scale = max(scale, current.length_squared());
        }
        if !(twice_area.abs() > tolerance::ABSOLUTE * scale) {
            None
        } else {
            Some(origin + moment / (3.0 * twice_area))
        }
    }
    pub fn classify(&self, point: Point2) -> Containment {
        if !self.closed || self.vertices.len() < 3 {
            return Containment::Outside;
        }
        if self
            .distance_to(point)
            .is_some_and(|distance| distance <= tolerance::GEOMETRIC)
        {
            return Containment::OnBoundary;
        }
        let mut inside = false;
        for index in 0..self.vertices.len() {
            inside ^= edge_crosses_ray(
                self.vertices[index],
                self.vertices[(index + 1) % self.vertices.len()],
                point,
            );
        }
        if inside {
            Containment::Inside
        } else {
            Containment::Outside
        }
    }
    pub fn contains(&self, point: Point2) -> bool {
        self.classify(point) != Containment::Outside
    }
    pub fn closest_point(&self, point: Point2) -> Option<Point2> {
        let mut best = *self.vertices.first()?;
        let mut best_distance = best.distance_to(point);
        for index in 0..self.segment_count() {
            let candidate = self.segment(index).closest_point(point);
            let distance = candidate.distance_to(point);
            if distance < best_distance {
                best_distance = distance;
                best = candidate;
            }
        }
        Some(best)
    }
    pub fn distance_to(&self, point: Point2) -> Option<f64> {
        self.closest_point(point)
            .map(|closest| closest.distance_to(point))
    }
    pub fn bounding_box(&self) -> Box2 {
        crate::point_batch::bounds_of(&self.vertices)
    }
    pub fn reversed(&self) -> Self {
        let mut result = self.clone();
        result.vertices.reverse();
        result
    }
    pub fn without_duplicate_vertices(&self) -> Self {
        let mut result = Self::new(Vec::new(), self.closed);
        for &vertex in &self.vertices {
            if result
                .vertices
                .last()
                .is_none_or(|last| last.distance_to(vertex) > tolerance::GEOMETRIC)
            {
                result.vertices.push(vertex);
            }
        }
        if self.closed
            && result.vertices.len() > 1
            && result.vertices[0].distance_to(*result.vertices.last().unwrap())
                <= tolerance::GEOMETRIC
        {
            result.vertices.pop();
        }
        result
    }
}

#[derive(Clone, Debug)]
pub struct PolygonClassifier {
    vertices: Vec<Point2>,
    bounds: Box2,
    band_height: f64,
    bands: usize,
    band_start: Vec<u32>,
    edges: Vec<u32>,
}
impl PolygonClassifier {
    pub fn new(polygon: &Polyline2) -> Self {
        let mut result = Self {
            vertices: polygon.vertices.clone(),
            bounds: Box2::default(),
            band_height: 1.0,
            bands: 1,
            band_start: Vec::new(),
            edges: Vec::new(),
        };
        let count = result.vertices.len();
        if count < 3 {
            result.vertices.clear();
            return result;
        }
        for &vertex in &result.vertices {
            result.bounds.expand(vertex);
        }
        result.bands = (count / 4).clamp(1, 1024);
        let height = result.bounds.height();
        result.band_height = if height > 0.0 {
            height / result.bands as f64
        } else {
            1.0
        };
        result.band_start = vec![0; result.bands + 1];
        for edge in 0..count {
            let (first, last) = result.edge_range(edge);
            for band in first..=last {
                result.band_start[band + 1] += 1;
            }
        }
        for band in 0..result.bands {
            let previous = result.band_start[band];
            result.band_start[band + 1] += previous;
        }
        result.edges = vec![0; result.band_start[result.bands] as usize];
        let mut cursor = result.band_start[..result.bands].to_vec();
        for edge in 0..count {
            let (first, last) = result.edge_range(edge);
            for position in &mut cursor[first..=last] {
                result.edges[*position as usize] = edge as u32;
                *position += 1;
            }
        }
        result
    }
    fn band_of(&self, height: f64) -> usize {
        let band = ((height - self.bounds.min.y) / self.band_height).floor();
        if !(band > 0.0) {
            0
        } else if band >= self.bands as f64 {
            self.bands - 1
        } else {
            band as usize
        }
    }
    fn edge_range(&self, edge: usize) -> (usize, usize) {
        let first = self.vertices[edge];
        let second = self.vertices[(edge + 1) % self.vertices.len()];
        (
            self.band_of(min(first.y, second.y) - tolerance::GEOMETRIC),
            self.band_of(max(first.y, second.y) + tolerance::GEOMETRIC),
        )
    }
    pub fn classify(&self, point: Point2) -> Containment {
        let count = self.vertices.len();
        if count < 3
            || !(point.y >= self.bounds.min.y - tolerance::GEOMETRIC
                && point.y <= self.bounds.max.y + tolerance::GEOMETRIC)
        {
            return Containment::Outside;
        }
        let band = self.band_of(point.y);
        let edges = &self.edges[self.band_start[band] as usize..self.band_start[band + 1] as usize];
        for &edge in edges {
            if Segment2::new(
                self.vertices[edge as usize],
                self.vertices[(edge as usize + 1) % count],
            )
            .distance_to(point)
                <= tolerance::GEOMETRIC
            {
                return Containment::OnBoundary;
            }
        }
        let mut inside = false;
        for &edge in edges {
            inside ^= edge_crosses_ray(
                self.vertices[edge as usize],
                self.vertices[(edge as usize + 1) % count],
                point,
            );
        }
        if inside {
            Containment::Inside
        } else {
            Containment::Outside
        }
    }
    pub fn contains(&self, point: Point2) -> bool {
        self.classify(point) != Containment::Outside
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rectangle2 {
    pub origin: Point2,
    pub width: f64,
    pub height: f64,
}
impl Rectangle2 {
    pub fn from_corners(first: Point2, second: Point2) -> Self {
        let low = Point2::new(min(first.x, second.x), min(first.y, second.y));
        let high = Point2::new(max(first.x, second.x), max(first.y, second.y));
        Self {
            origin: low,
            width: high.x - low.x,
            height: high.y - low.y,
        }
    }
    pub fn area(&self) -> f64 {
        self.width * self.height
    }
    pub fn perimeter(&self) -> f64 {
        2.0 * (self.width + self.height)
    }
    pub fn corners(&self) -> [Point2; 4] {
        [
            self.origin,
            self.origin + Vec2::new(self.width, 0.0),
            self.origin + Vec2::new(self.width, self.height),
            self.origin + Vec2::new(0.0, self.height),
        ]
    }
    pub fn to_polyline(&self) -> Polyline2 {
        Polyline2::new(self.corners().to_vec(), true)
    }
    pub fn bounding_box(&self) -> Box2 {
        Box2::new(
            self.origin,
            self.origin + Vec2::new(self.width, self.height),
        )
    }
    pub fn classify(&self, point: Point2) -> Containment {
        if self.distance_to(point) <= tolerance::GEOMETRIC {
            Containment::OnBoundary
        } else if self.bounding_box().contains(point) {
            Containment::Inside
        } else {
            Containment::Outside
        }
    }
    pub fn contains(&self, point: Point2) -> bool {
        self.classify(point) != Containment::Outside
    }
    pub fn closest_point(&self, point: Point2) -> Point2 {
        let corners = self.corners();
        let mut best = corners[0];
        let mut best_distance = best.distance_to(point);
        for index in 0..4 {
            let candidate =
                Segment2::new(corners[index], corners[(index + 1) % 4]).closest_point(point);
            let distance = candidate.distance_to(point);
            if distance < best_distance {
                best_distance = distance;
                best = candidate;
            }
        }
        best
    }
    pub fn distance_to(&self, point: Point2) -> f64 {
        self.closest_point(point).distance_to(point)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Triangle2 {
    pub a: Point2,
    pub b: Point2,
    pub c: Point2,
}
impl Triangle2 {
    pub fn new(first: Point2, second: Point2, third: Point2) -> Self {
        Self {
            a: first,
            b: second,
            c: third,
        }
    }
    pub fn twice_signed_area(&self) -> f64 {
        (self.b - self.a).cross(self.c - self.a)
    }
    pub fn signed_area(&self) -> f64 {
        0.5 * self.twice_signed_area()
    }
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }
    pub fn perimeter(&self) -> f64 {
        self.a.distance_to(self.b) + self.b.distance_to(self.c) + self.c.distance_to(self.a)
    }
    pub fn centroid(&self) -> Point2 {
        self.a + ((self.b - self.a) + (self.c - self.a)) / 3.0
    }
    pub fn is_degenerate(&self) -> bool {
        let longest = max(
            max(self.a.distance_to(self.b), self.b.distance_to(self.c)),
            self.c.distance_to(self.a),
        );
        if longest <= tolerance::GEOMETRIC {
            return true;
        }
        2.0 * self.area() / longest <= tolerance::GEOMETRIC
    }
    pub fn bounding_box(&self) -> Box2 {
        let mut bounds = Box2::default();
        bounds.expand(self.a);
        bounds.expand(self.b);
        bounds.expand(self.c);
        bounds
    }
    pub fn classify(&self, point: Point2) -> Containment {
        let edge_distance = min(
            min(
                Segment2::new(self.a, self.b).distance_to(point),
                Segment2::new(self.b, self.c).distance_to(point),
            ),
            Segment2::new(self.c, self.a).distance_to(point),
        );
        if edge_distance <= tolerance::GEOMETRIC {
            return Containment::OnBoundary;
        }
        let first = (self.b - self.a).cross(point - self.a);
        let second = (self.c - self.b).cross(point - self.b);
        let third = (self.a - self.c).cross(point - self.c);
        let negative = first < 0.0 || second < 0.0 || third < 0.0;
        let positive = first > 0.0 || second > 0.0 || third > 0.0;
        if negative && positive {
            Containment::Outside
        } else {
            Containment::Inside
        }
    }
    pub fn contains(&self, point: Point2) -> bool {
        self.classify(point) != Containment::Outside
    }
    pub fn barycentric_with_area(&self, point: Point2, twice_area: f64) -> [f64; 3] {
        let second = (point - self.a).cross(self.c - self.a) / twice_area;
        let third = (self.b - self.a).cross(point - self.a) / twice_area;
        [1.0 - second - third, second, third]
    }
    pub fn barycentric(&self, point: Point2) -> Option<[f64; 3]> {
        if self.is_degenerate() {
            None
        } else {
            Some(self.barycentric_with_area(point, self.twice_signed_area()))
        }
    }
    pub fn circumcircle(&self) -> Option<Circle2> {
        if self.is_degenerate() {
            return None;
        }
        let first = self.b - self.a;
        let second = self.c - self.a;
        let divisor = 2.0 * first.cross(second);
        let first_squared = first.length_squared();
        let second_squared = second.length_squared();
        let offset = Vec2::new(
            (second.y * first_squared - first.y * second_squared) / divisor,
            (first.x * second_squared - second.x * first_squared) / divisor,
        );
        Some(Circle2::new(self.a + offset, offset.length()))
    }
}
