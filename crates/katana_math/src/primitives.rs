use crate::cpp;
use crate::{Vec3, tolerance};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub offset: f64,
}

impl Default for Plane {
    fn default() -> Self {
        Self::new(Vec3::new(0.0, 0.0, 1.0), 0.0)
    }
}

impl Plane {
    pub const fn new(normal: Vec3, offset: f64) -> Self {
        Self { normal, offset }
    }

    pub fn from_point_normal(point: Vec3, normal: Vec3) -> Option<Self> {
        let normal = normal.normalized();
        if normal == Vec3::default() {
            return None;
        }
        Some(Self::new(normal, -normal.dot(point)))
    }

    pub fn from_points(first: Vec3, second: Vec3, third: Vec3) -> Option<Self> {
        let first_edge = second - first;
        let second_edge = third - first;
        let normal = first_edge.cross(second_edge);
        if normal
            .length()
            .partial_cmp(&(tolerance::ANGULAR * first_edge.length() * second_edge.length()))
            != Some(Ordering::Greater)
        {
            return None;
        }
        Self::from_point_normal(first, normal)
    }

    pub const fn signed_distance(self, point: Vec3) -> f64 {
        self.normal.dot(point) + self.offset
    }

    pub fn project(self, point: Vec3) -> Vec3 {
        point - self.normal * self.signed_distance(point)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Default for Ray {
    fn default() -> Self {
        Self::new(Vec3::default(), Vec3::new(1.0, 0.0, 0.0))
    }
}

impl Ray {
    pub const fn new(origin: Vec3, direction: Vec3) -> Self {
        Self { origin, direction }
    }

    pub fn at(self, parameter: f64) -> Vec3 {
        self.origin + self.direction * parameter
    }

    pub fn intersect<T: RayIntersection>(self, target: T) -> Option<T::Hit> {
        intersect(self, target)
    }

    pub fn intersect_plane(self, plane: Plane) -> Option<f64> {
        intersect_plane(self, plane)
    }

    pub fn intersect_box(self, bounds: AABB) -> Option<RayBoxHit> {
        intersect_box(self, bounds)
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AABB {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for AABB {
    fn default() -> Self {
        Self::new(
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(-f64::INFINITY, -f64::INFINITY, -f64::INFINITY),
        )
    }
}

impl AABB {
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub const fn empty(self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(self) -> Vec3 {
        if self.empty() {
            Vec3::default()
        } else {
            self.max - self.min
        }
    }

    pub fn expand<T: BoundsExpansion>(&mut self, value: T) {
        value.expand_bounds(self);
    }

    pub fn expand_point(&mut self, point: Vec3) {
        self.min = Vec3::new(
            cpp::min(self.min.x, point.x),
            cpp::min(self.min.y, point.y),
            cpp::min(self.min.z, point.z),
        );
        self.max = Vec3::new(
            cpp::max(self.max.x, point.x),
            cpp::max(self.max.y, point.y),
            cpp::max(self.max.z, point.z),
        );
    }

    pub fn expand_box(&mut self, other: Self) {
        if !other.empty() {
            self.expand_point(other.min);
            self.expand_point(other.max);
        }
    }

    pub const fn contains(self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    pub const fn intersects(self, other: Self) -> bool {
        !self.empty()
            && !other.empty()
            && self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }
}

pub trait BoundsExpansion {
    fn expand_bounds(self, bounds: &mut AABB);
}

impl BoundsExpansion for Vec3 {
    fn expand_bounds(self, bounds: &mut AABB) {
        bounds.expand_point(self);
    }
}

impl BoundsExpansion for AABB {
    fn expand_bounds(self, bounds: &mut AABB) {
        bounds.expand_box(self);
    }
}

impl<T: BoundsExpansion + Copy> BoundsExpansion for &T {
    fn expand_bounds(self, bounds: &mut AABB) {
        (*self).expand_bounds(bounds);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RayBoxHit {
    pub t_enter: f64,
    pub t_exit: f64,
}

impl RayBoxHit {
    pub const fn new(t_enter: f64, t_exit: f64) -> Self {
        Self { t_enter, t_exit }
    }
}

pub fn intersect_plane(ray: Ray, plane: Plane) -> Option<f64> {
    let denominator = plane.normal.dot(ray.direction);
    if denominator
        .abs()
        .partial_cmp(&(tolerance::ANGULAR * ray.direction.length()))
        != Some(Ordering::Greater)
    {
        return None;
    }
    let parameter = -plane.signed_distance(ray.origin) / denominator;
    if parameter < 0.0 {
        return None;
    }
    Some(parameter)
}

pub fn intersect_box(ray: Ray, bounds: AABB) -> Option<RayBoxHit> {
    if bounds.empty() {
        return None;
    }
    let mut t_enter = 0.0;
    let mut t_exit = f64::INFINITY;
    let origin = [ray.origin.x, ray.origin.y, ray.origin.z];
    let direction = [ray.direction.x, ray.direction.y, ray.direction.z];
    let lower = [bounds.min.x, bounds.min.y, bounds.min.z];
    let upper = [bounds.max.x, bounds.max.y, bounds.max.z];
    for (((origin, direction), lower), upper) in
        origin.into_iter().zip(direction).zip(lower).zip(upper)
    {
        if direction == 0.0 {
            if origin < lower || origin > upper {
                return None;
            }
            continue;
        }
        let mut first = (lower - origin) / direction;
        let mut second = (upper - origin) / direction;
        if first > second {
            std::mem::swap(&mut first, &mut second);
        }
        t_enter = cpp::max(t_enter, first);
        t_exit = cpp::min(t_exit, second);
        if t_enter > t_exit {
            return None;
        }
    }
    Some(RayBoxHit::new(t_enter, t_exit))
}

pub trait RayIntersection {
    type Hit;

    fn intersect_ray(&self, ray: Ray) -> Option<Self::Hit>;
}

impl RayIntersection for Plane {
    type Hit = f64;

    fn intersect_ray(&self, ray: Ray) -> Option<Self::Hit> {
        intersect_plane(ray, *self)
    }
}

impl RayIntersection for AABB {
    type Hit = RayBoxHit;

    fn intersect_ray(&self, ray: Ray) -> Option<Self::Hit> {
        intersect_box(ray, *self)
    }
}

impl<T: RayIntersection + ?Sized> RayIntersection for &T {
    type Hit = T::Hit;

    fn intersect_ray(&self, ray: Ray) -> Option<Self::Hit> {
        T::intersect_ray(*self, ray)
    }
}

pub fn intersect<T: RayIntersection>(ray: Ray, target: T) -> Option<T::Hit> {
    target.intersect_ray(ray)
}
