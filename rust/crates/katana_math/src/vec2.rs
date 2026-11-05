use crate::numerics;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub const fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn normalized(self) -> Self {
        let length = self.length();
        if length > 0.0 {
            Self::new(self.x / length, self.y / length)
        } else {
            Self::default()
        }
    }

    pub const fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub const fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub const fn perpendicular(self) -> Self {
        Self::new(-self.y, self.x)
    }

    pub fn rotated(self, radians: f64) -> Self {
        let cosine = radians.cos();
        let sine = radians.sin();
        Self::new(
            cosine * self.x - sine * self.y,
            sine * self.x + cosine * self.y,
        )
    }

    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }

    pub fn distance_to(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub fn lerp(self, other: Self, parameter: f64) -> Self {
        lerp(self, other, parameter)
    }

    pub fn project(self, onto: Self) -> Self {
        project(self, onto)
    }

    pub fn reflect(self, unit_normal: Self) -> Self {
        reflect(self, unit_normal)
    }

    pub fn nearly_equal(self, other: Self, epsilon: f64) -> bool {
        nearly_equal(self, other, epsilon)
    }
}

impl_vector_ops!(Vec2, x, y);

pub const fn dot(first: Vec2, second: Vec2) -> f64 {
    first.dot(second)
}

pub const fn cross(first: Vec2, second: Vec2) -> f64 {
    first.cross(second)
}

pub fn length(vector: Vec2) -> f64 {
    vector.length()
}

pub fn normalize(vector: Vec2) -> Vec2 {
    vector.normalized()
}

pub fn distance(first: Vec2, second: Vec2) -> f64 {
    first.distance_to(second)
}

pub fn lerp(first: Vec2, second: Vec2, parameter: f64) -> Vec2 {
    first + (second - first) * parameter
}

pub fn project(vector: Vec2, onto: Vec2) -> Vec2 {
    let denominator = onto.length_squared();
    if denominator > 0.0 {
        onto * (vector.dot(onto) / denominator)
    } else {
        Vec2::default()
    }
}

pub fn reflect(vector: Vec2, unit_normal: Vec2) -> Vec2 {
    vector - unit_normal * (2.0 * vector.dot(unit_normal))
}

pub fn nearly_equal(first: Vec2, second: Vec2, epsilon: f64) -> bool {
    numerics::nearly_equal(first.x, second.x, epsilon)
        && numerics::nearly_equal(first.y, second.y, epsilon)
}
