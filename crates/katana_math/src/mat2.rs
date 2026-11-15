use crate::{Vec2, numerics, tolerance};
use std::cmp::Ordering;
use std::ops::Mul;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat2 {
    pub data: [f64; 4],
}

impl Default for Mat2 {
    fn default() -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0)
    }
}

impl Mat2 {
    pub const fn new(m00: f64, m01: f64, m10: f64, m11: f64) -> Self {
        Self {
            data: [m00, m01, m10, m11],
        }
    }

    pub fn rotation(radians: f64) -> Self {
        let cosine = radians.cos();
        let sine = radians.sin();
        Self::new(cosine, -sine, sine, cosine)
    }

    pub const fn scaling(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, sy)
    }

    pub const fn multiply(self, vector: Vec2) -> Vec2 {
        Vec2::new(
            self.data[0] * vector.x + self.data[1] * vector.y,
            self.data[2] * vector.x + self.data[3] * vector.y,
        )
    }

    pub const fn transposed(self) -> Self {
        Self::new(self.data[0], self.data[2], self.data[1], self.data[3])
    }

    pub const fn determinant(self) -> f64 {
        self.data[0] * self.data[3] - self.data[1] * self.data[2]
    }

    pub fn inverse(self) -> Option<Self> {
        let determinant = self.determinant();
        let scale = self.data[0].hypot(self.data[1]) * self.data[2].hypot(self.data[3]);
        if determinant
            .abs()
            .partial_cmp(&(tolerance::ABSOLUTE * scale))
            != Some(Ordering::Greater)
        {
            return None;
        }
        let inverse = 1.0 / determinant;
        Some(Self::new(
            self.data[3] * inverse,
            -self.data[1] * inverse,
            -self.data[2] * inverse,
            self.data[0] * inverse,
        ))
    }

    pub fn nearly_equal(self, other: Self, epsilon: f64) -> bool {
        nearly_equal(self, other, epsilon)
    }
}

impl_matrix_access!(Mat2, 2);

impl Mul<Vec2> for Mat2 {
    type Output = Vec2;

    fn mul(self, vector: Vec2) -> Vec2 {
        self.multiply(vector)
    }
}

impl Mul for Mat2 {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.data[0] * other.data[0] + self.data[1] * other.data[2],
            self.data[0] * other.data[1] + self.data[1] * other.data[3],
            self.data[2] * other.data[0] + self.data[3] * other.data[2],
            self.data[2] * other.data[1] + self.data[3] * other.data[3],
        )
    }
}

pub fn nearly_equal(first: Mat2, second: Mat2, epsilon: f64) -> bool {
    first
        .data
        .iter()
        .zip(second.data)
        .all(|(&first, second)| numerics::nearly_equal(first, second, epsilon))
}
