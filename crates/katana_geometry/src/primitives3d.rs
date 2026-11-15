use katana_math::numerics::tolerance;
use katana_math::primitives::{AABB as Aabb, Plane, Ray};
pub use katana_math::vec3::Vec3;
pub type Point3 = Vec3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment3 {
    pub start: Point3,
    pub end: Point3,
}
impl Segment3 {
    pub fn new(start: Point3, end: Point3) -> Self {
        Self { start, end }
    }
    pub fn length(&self) -> f64 {
        self.start.distance_to(self.end)
    }
    pub fn point_at(&self, parameter: f64) -> Point3 {
        self.start + (self.end - self.start) * parameter
    }
    pub fn parameter_of(&self, point: Point3) -> f64 {
        let delta = self.end - self.start;
        let squared = delta.length_squared();
        if !(squared > 0.0) {
            return 0.0;
        }
        ((point - self.start).dot(delta) / squared).clamp(0.0, 1.0)
    }
    pub fn closest_point(&self, point: Point3) -> Point3 {
        self.point_at(self.parameter_of(point))
    }
    pub fn distance_to(&self, point: Point3) -> f64 {
        self.closest_point(point).distance_to(point)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Triangle3 {
    pub a: Point3,
    pub b: Point3,
    pub c: Point3,
}
impl Triangle3 {
    pub fn new(first: Point3, second: Point3, third: Point3) -> Self {
        Self {
            a: first,
            b: second,
            c: third,
        }
    }
    pub fn scaled_normal(&self) -> Vec3 {
        (self.b - self.a).cross(self.c - self.a)
    }
    pub fn area(&self) -> f64 {
        0.5 * self.scaled_normal().length()
    }
    pub fn centroid(&self) -> Point3 {
        self.a + ((self.b - self.a) + (self.c - self.a)) / 3.0
    }
    pub fn plane(&self) -> Option<Plane> {
        Plane::from_points(self.a, self.b, self.c)
    }
    pub fn bounding_box(&self) -> Aabb {
        let mut bounds = Aabb::default();
        bounds.expand(self.a);
        bounds.expand(self.b);
        bounds.expand(self.c);
        bounds
    }
}

pub fn intersect(ray: &Ray, triangle: &Triangle3) -> Option<f64> {
    let edge_first = triangle.b - triangle.a;
    let edge_second = triangle.c - triangle.a;
    let cross_first = ray.direction.cross(edge_second);
    let determinant = edge_first.dot(cross_first);
    let scale = ray.direction.length() * edge_first.length() * edge_second.length();
    if !(determinant.abs() > tolerance::ANGULAR * scale) {
        return None;
    }
    let inverse = 1.0 / determinant;
    let to_origin = ray.origin - triangle.a;
    let weight_first = to_origin.dot(cross_first) * inverse;
    if weight_first < 0.0 || weight_first > 1.0 {
        return None;
    }
    let cross_second = to_origin.cross(edge_first);
    let weight_second = ray.direction.dot(cross_second) * inverse;
    if weight_second < 0.0 || weight_first + weight_second > 1.0 {
        return None;
    }
    let parameter = edge_second.dot(cross_second) * inverse;
    if parameter < 0.0 {
        None
    } else {
        Some(parameter)
    }
}
