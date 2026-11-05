use crate::numerics;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub const fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn length(self) -> f64 {
        scaled_hypot(self.x, self.y, self.z)
    }

    pub fn normalized(self) -> Self {
        let length = self.length();
        if length > 0.0 {
            Self::new(self.x / length, self.y / length, self.z / length)
        } else {
            Self::default()
        }
    }

    pub const fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub const fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn distance_to(self, other: Self) -> f64 {
        scaled_hypot(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
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

impl_vector_ops!(Vec3, x, y, z);

pub const fn dot(first: Vec3, second: Vec3) -> f64 {
    first.dot(second)
}

pub const fn cross(first: Vec3, second: Vec3) -> Vec3 {
    first.cross(second)
}

pub fn length(vector: Vec3) -> f64 {
    vector.length()
}

pub fn normalize(vector: Vec3) -> Vec3 {
    vector.normalized()
}

pub fn distance(first: Vec3, second: Vec3) -> f64 {
    first.distance_to(second)
}

pub fn lerp(first: Vec3, second: Vec3, parameter: f64) -> Vec3 {
    first + (second - first) * parameter
}

pub fn project(vector: Vec3, onto: Vec3) -> Vec3 {
    let denominator = onto.length_squared();
    if denominator > 0.0 {
        onto * (vector.dot(onto) / denominator)
    } else {
        Vec3::default()
    }
}

pub fn reflect(vector: Vec3, unit_normal: Vec3) -> Vec3 {
    vector - unit_normal * (2.0 * vector.dot(unit_normal))
}

pub fn nearly_equal(first: Vec3, second: Vec3, epsilon: f64) -> bool {
    numerics::nearly_equal(first.x, second.x, epsilon)
        && numerics::nearly_equal(first.y, second.y, epsilon)
        && numerics::nearly_equal(first.z, second.z, epsilon)
}

fn scaled_hypot(x: f64, y: f64, z: f64) -> f64 {
    let x = x.abs();
    let y = y.abs();
    let z = z.abs();
    let maximum = if x < y {
        if y < z { z } else { y }
    } else if x < z {
        z
    } else {
        x
    };
    if maximum != 0.0 {
        maximum
            * ((x / maximum) * (x / maximum)
                + (y / maximum) * (y / maximum)
                + (z / maximum) * (z / maximum))
                .sqrt()
    } else {
        0.0
    }
}
