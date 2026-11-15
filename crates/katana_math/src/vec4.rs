use crate::{Vec3, numerics};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Vec4 {
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    pub const fn from_xyz(xyz: Vec3, w: f64) -> Self {
        Self::new(xyz.x, xyz.y, xyz.z, w)
    }

    pub const fn xyz(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    pub const fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    pub fn normalized(self) -> Self {
        let length = self.length();
        if length > 0.0 {
            Self::new(
                self.x / length,
                self.y / length,
                self.z / length,
                self.w / length,
            )
        } else {
            Self::default()
        }
    }

    pub const fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    pub fn distance_to(self, other: Self) -> f64 {
        Self::new(
            self.x - other.x,
            self.y - other.y,
            self.z - other.z,
            self.w - other.w,
        )
        .length()
    }

    pub fn lerp(self, other: Self, parameter: f64) -> Self {
        lerp(self, other, parameter)
    }

    pub fn nearly_equal(self, other: Self, epsilon: f64) -> bool {
        nearly_equal(self, other, epsilon)
    }
}

impl_vector_ops!(Vec4, x, y, z, w);

pub const fn dot(first: Vec4, second: Vec4) -> f64 {
    first.dot(second)
}

pub fn length(vector: Vec4) -> f64 {
    vector.length()
}

pub fn normalize(vector: Vec4) -> Vec4 {
    vector.normalized()
}

pub fn distance(first: Vec4, second: Vec4) -> f64 {
    first.distance_to(second)
}

pub fn lerp(first: Vec4, second: Vec4, parameter: f64) -> Vec4 {
    first + (second - first) * parameter
}

pub fn nearly_equal(first: Vec4, second: Vec4, epsilon: f64) -> bool {
    numerics::nearly_equal(first.x, second.x, epsilon)
        && numerics::nearly_equal(first.y, second.y, epsilon)
        && numerics::nearly_equal(first.z, second.z, epsilon)
        && numerics::nearly_equal(first.w, second.w, epsilon)
}
