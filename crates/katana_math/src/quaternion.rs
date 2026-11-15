use crate::{Mat3, Vec3, numerics, tolerance};
use std::fmt;
use std::ops::Mul;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Default for Quaternion {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }
}

impl Quaternion {
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    pub fn from_axis_angle(axis: Vec3, radians: f64) -> Self {
        let unit = axis.normalized();
        if unit == Vec3::default() {
            return Self::default();
        }
        let half = 0.5 * radians;
        let sine = half.sin();
        Self::new(unit.x * sine, unit.y * sine, unit.z * sine, half.cos())
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

    pub const fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    pub const fn inverse(self) -> Self {
        let length_squared = self.length_squared();
        if length_squared > 0.0 {
            let inverse = 1.0 / length_squared;
            Self::new(
                -self.x * inverse,
                -self.y * inverse,
                -self.z * inverse,
                self.w * inverse,
            )
        } else {
            Self::new(0.0, 0.0, 0.0, 1.0)
        }
    }

    pub const fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    pub fn rotate(self, vector: Vec3) -> Vec3 {
        let vector_part = Vec3::new(self.x, self.y, self.z);
        let twice_cross = vector_part.cross(vector) * 2.0;
        vector + twice_cross * self.w + vector_part.cross(twice_cross)
    }

    pub const fn to_matrix(self) -> Mat3 {
        let xx = self.x * self.x;
        let yy = self.y * self.y;
        let zz = self.z * self.z;
        let xy = self.x * self.y;
        let xz = self.x * self.z;
        let yz = self.y * self.z;
        let wx = self.w * self.x;
        let wy = self.w * self.y;
        let wz = self.w * self.z;
        Mat3::new(
            1.0 - 2.0 * (yy + zz),
            2.0 * (xy - wz),
            2.0 * (xz + wy),
            2.0 * (xy + wz),
            1.0 - 2.0 * (xx + zz),
            2.0 * (yz - wx),
            2.0 * (xz - wy),
            2.0 * (yz + wx),
            1.0 - 2.0 * (xx + yy),
        )
    }

    pub fn slerp(self, other: Self, parameter: f64) -> Self {
        slerp(self, other, parameter)
    }

    pub fn nearly_equal(self, other: Self, epsilon: f64) -> bool {
        nearly_equal(self, other, epsilon)
    }
}

impl Mul for Quaternion {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        )
    }
}

pub fn slerp(first: Quaternion, mut second: Quaternion, parameter: f64) -> Quaternion {
    let mut cosine = first.dot(second);
    if cosine < 0.0 {
        second = Quaternion::new(-second.x, -second.y, -second.z, -second.w);
        cosine = -cosine;
    }
    if cosine > 1.0 - tolerance::ABSOLUTE {
        return Quaternion::new(
            numerics::lerp(first.x, second.x, parameter),
            numerics::lerp(first.y, second.y, parameter),
            numerics::lerp(first.z, second.z, parameter),
            numerics::lerp(first.w, second.w, parameter),
        )
        .normalized();
    }
    let theta = cosine.acos();
    let sine = theta.sin();
    let first_weight = ((1.0 - parameter) * theta).sin() / sine;
    let second_weight = (parameter * theta).sin() / sine;
    Quaternion::new(
        first_weight * first.x + second_weight * second.x,
        first_weight * first.y + second_weight * second.y,
        first_weight * first.z + second_weight * second.z,
        first_weight * first.w + second_weight * second.w,
    )
}

pub fn nearly_equal(first: Quaternion, second: Quaternion, epsilon: f64) -> bool {
    numerics::nearly_equal(first.x, second.x, epsilon)
        && numerics::nearly_equal(first.y, second.y, epsilon)
        && numerics::nearly_equal(first.z, second.z, epsilon)
        && numerics::nearly_equal(first.w, second.w, epsilon)
}

impl fmt::Display for Quaternion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let precision = formatter.precision().unwrap_or(6).max(1);
        write!(
            formatter,
            "Quaternion({}, {}, {}, {})",
            crate::vector_ops::format_component(self.x, precision),
            crate::vector_ops::format_component(self.y, precision),
            crate::vector_ops::format_component(self.z, precision),
            crate::vector_ops::format_component(self.w, precision)
        )
    }
}
