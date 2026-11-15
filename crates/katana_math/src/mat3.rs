use crate::{Vec2, Vec3, numerics, tolerance};
use std::cmp::Ordering;
use std::ops::Mul;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    pub data: [f64; 9],
}

impl Default for Mat3 {
    fn default() -> Self {
        Self::new(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
    }
}

impl Mat3 {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        m00: f64,
        m01: f64,
        m02: f64,
        m10: f64,
        m11: f64,
        m12: f64,
        m20: f64,
        m21: f64,
        m22: f64,
    ) -> Self {
        Self {
            data: [m00, m01, m02, m10, m11, m12, m20, m21, m22],
        }
    }

    pub const fn translation(translation: Vec2) -> Self {
        Self::new(
            1.0,
            0.0,
            translation.x,
            0.0,
            1.0,
            translation.y,
            0.0,
            0.0,
            1.0,
        )
    }

    pub fn rotation(radians: f64) -> Self {
        let cosine = radians.cos();
        let sine = radians.sin();
        Self::new(cosine, -sine, 0.0, sine, cosine, 0.0, 0.0, 0.0, 1.0)
    }

    pub const fn scaling(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 1.0)
    }

    pub fn rotation_about(center: Vec2, radians: f64) -> Self {
        Self::translation(center) * Self::rotation(radians) * Self::translation(-center)
    }

    pub fn scaling_about(center: Vec2, sx: f64, sy: f64) -> Self {
        Self::translation(center) * Self::scaling(sx, sy) * Self::translation(-center)
    }

    pub fn reflection(point_on_line: Vec2, direction: Vec2) -> Self {
        let direction = direction.normalized();
        if direction == Vec2::default() {
            return Self::default();
        }
        let linear = Self::new(
            2.0 * direction.x * direction.x - 1.0,
            2.0 * direction.x * direction.y,
            0.0,
            2.0 * direction.x * direction.y,
            2.0 * direction.y * direction.y - 1.0,
            0.0,
            0.0,
            0.0,
            1.0,
        );
        Self::translation(point_on_line) * linear * Self::translation(-point_on_line)
    }

    pub const fn multiply(self, vector: Vec3) -> Vec3 {
        Vec3::new(
            self.data[0] * vector.x + self.data[1] * vector.y + self.data[2] * vector.z,
            self.data[3] * vector.x + self.data[4] * vector.y + self.data[5] * vector.z,
            self.data[6] * vector.x + self.data[7] * vector.y + self.data[8] * vector.z,
        )
    }

    pub const fn transposed(self) -> Self {
        Self::new(
            self.data[0],
            self.data[3],
            self.data[6],
            self.data[1],
            self.data[4],
            self.data[7],
            self.data[2],
            self.data[5],
            self.data[8],
        )
    }

    pub const fn determinant(self) -> f64 {
        self.data[0] * (self.data[4] * self.data[8] - self.data[5] * self.data[7])
            - self.data[1] * (self.data[3] * self.data[8] - self.data[5] * self.data[6])
            + self.data[2] * (self.data[3] * self.data[7] - self.data[4] * self.data[6])
    }

    pub fn inverse(self) -> Option<Self> {
        let determinant = self.determinant();
        let scale = Vec3::new(self.data[0], self.data[1], self.data[2]).length()
            * Vec3::new(self.data[3], self.data[4], self.data[5]).length()
            * Vec3::new(self.data[6], self.data[7], self.data[8]).length();
        if determinant
            .abs()
            .partial_cmp(&(tolerance::ABSOLUTE * scale))
            != Some(Ordering::Greater)
        {
            return None;
        }
        let inverse = 1.0 / determinant;
        let values = self.data;
        Some(Self::new(
            (values[4] * values[8] - values[5] * values[7]) * inverse,
            (values[2] * values[7] - values[1] * values[8]) * inverse,
            (values[1] * values[5] - values[2] * values[4]) * inverse,
            (values[5] * values[6] - values[3] * values[8]) * inverse,
            (values[0] * values[8] - values[2] * values[6]) * inverse,
            (values[2] * values[3] - values[0] * values[5]) * inverse,
            (values[3] * values[7] - values[4] * values[6]) * inverse,
            (values[1] * values[6] - values[0] * values[7]) * inverse,
            (values[0] * values[4] - values[1] * values[3]) * inverse,
        ))
    }

    pub const fn transform_point(self, point: Vec2) -> Vec2 {
        transform_point(self, point)
    }

    pub const fn transform_vector(self, vector: Vec2) -> Vec2 {
        transform_vector(self, vector)
    }

    pub fn nearly_equal(self, other: Self, epsilon: f64) -> bool {
        nearly_equal(self, other, epsilon)
    }
}

impl_matrix_access!(Mat3, 3);

impl Mul<Vec3> for Mat3 {
    type Output = Vec3;

    fn mul(self, vector: Vec3) -> Vec3 {
        self.multiply(vector)
    }
}

impl Mul for Mat3 {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        let mut result = Self::default();
        for row in 0..3 {
            for column in 0..3 {
                let mut sum = 0.0;
                for inner in 0..3 {
                    sum += self[(row, inner)] * other[(inner, column)];
                }
                result[(row, column)] = sum;
            }
        }
        result
    }
}

pub const fn transform_point(matrix: Mat3, point: Vec2) -> Vec2 {
    Vec2::new(
        matrix.data[0] * point.x + matrix.data[1] * point.y + matrix.data[2],
        matrix.data[3] * point.x + matrix.data[4] * point.y + matrix.data[5],
    )
}

pub const fn transform_vector(matrix: Mat3, vector: Vec2) -> Vec2 {
    Vec2::new(
        matrix.data[0] * vector.x + matrix.data[1] * vector.y,
        matrix.data[3] * vector.x + matrix.data[4] * vector.y,
    )
}

pub fn nearly_equal(first: Mat3, second: Mat3, epsilon: f64) -> bool {
    first
        .data
        .iter()
        .zip(second.data)
        .all(|(&first, second)| numerics::nearly_equal(first, second, epsilon))
}
