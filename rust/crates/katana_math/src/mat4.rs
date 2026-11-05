use crate::{Vec3, Vec4, numerics, tolerance};
use std::cmp::Ordering;
use std::ops::Mul;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    pub data: [f64; 16],
}

impl Default for Mat4 {
    fn default() -> Self {
        Self::new(
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }
}

impl Mat4 {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        m00: f64,
        m01: f64,
        m02: f64,
        m03: f64,
        m10: f64,
        m11: f64,
        m12: f64,
        m13: f64,
        m20: f64,
        m21: f64,
        m22: f64,
        m23: f64,
        m30: f64,
        m31: f64,
        m32: f64,
        m33: f64,
    ) -> Self {
        Self {
            data: [
                m00, m01, m02, m03, m10, m11, m12, m13, m20, m21, m22, m23, m30, m31, m32, m33,
            ],
        }
    }

    pub const fn translation(translation: Vec3) -> Self {
        Self::new(
            1.0,
            0.0,
            0.0,
            translation.x,
            0.0,
            1.0,
            0.0,
            translation.y,
            0.0,
            0.0,
            1.0,
            translation.z,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    pub const fn scaling(scale: Vec3) -> Self {
        Self::new(
            scale.x, 0.0, 0.0, 0.0, 0.0, scale.y, 0.0, 0.0, 0.0, 0.0, scale.z, 0.0, 0.0, 0.0, 0.0,
            1.0,
        )
    }

    pub fn rotation(axis: Vec3, radians: f64) -> Self {
        let unit = axis.normalized();
        if unit == Vec3::default() {
            return Self::default();
        }
        let cosine = radians.cos();
        let sine = radians.sin();
        let complement = 1.0 - cosine;
        Self::new(
            complement * unit.x * unit.x + cosine,
            complement * unit.x * unit.y - sine * unit.z,
            complement * unit.x * unit.z + sine * unit.y,
            0.0,
            complement * unit.x * unit.y + sine * unit.z,
            complement * unit.y * unit.y + cosine,
            complement * unit.y * unit.z - sine * unit.x,
            0.0,
            complement * unit.x * unit.z - sine * unit.y,
            complement * unit.y * unit.z + sine * unit.x,
            complement * unit.z * unit.z + cosine,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    pub const fn multiply(self, vector: Vec4) -> Vec4 {
        Vec4::new(
            self.data[0] * vector.x
                + self.data[1] * vector.y
                + self.data[2] * vector.z
                + self.data[3] * vector.w,
            self.data[4] * vector.x
                + self.data[5] * vector.y
                + self.data[6] * vector.z
                + self.data[7] * vector.w,
            self.data[8] * vector.x
                + self.data[9] * vector.y
                + self.data[10] * vector.z
                + self.data[11] * vector.w,
            self.data[12] * vector.x
                + self.data[13] * vector.y
                + self.data[14] * vector.z
                + self.data[15] * vector.w,
        )
    }

    pub const fn transposed(self) -> Self {
        Self::new(
            self.data[0],
            self.data[4],
            self.data[8],
            self.data[12],
            self.data[1],
            self.data[5],
            self.data[9],
            self.data[13],
            self.data[2],
            self.data[6],
            self.data[10],
            self.data[14],
            self.data[3],
            self.data[7],
            self.data[11],
            self.data[15],
        )
    }

    pub const fn determinant(self) -> f64 {
        SubDeterminants::new(self).determinant()
    }

    pub fn inverse(self) -> Option<Self> {
        let minors = SubDeterminants::new(self);
        let determinant = minors.determinant();
        let mut scale = 1.0;
        for row in 0..4 {
            scale *= (self[(row, 0)] * self[(row, 0)]
                + self[(row, 1)] * self[(row, 1)]
                + self[(row, 2)] * self[(row, 2)]
                + self[(row, 3)] * self[(row, 3)])
                .sqrt();
        }
        if determinant
            .abs()
            .partial_cmp(&(tolerance::ABSOLUTE * scale))
            != Some(Ordering::Greater)
        {
            return None;
        }
        let values = self.data;
        let inverse = 1.0 / determinant;
        Some(Self::new(
            (values[5] * minors.c5 - values[6] * minors.c4 + values[7] * minors.c3) * inverse,
            (-values[1] * minors.c5 + values[2] * minors.c4 - values[3] * minors.c3) * inverse,
            (values[13] * minors.s5 - values[14] * minors.s4 + values[15] * minors.s3) * inverse,
            (-values[9] * minors.s5 + values[10] * minors.s4 - values[11] * minors.s3) * inverse,
            (-values[4] * minors.c5 + values[6] * minors.c2 - values[7] * minors.c1) * inverse,
            (values[0] * minors.c5 - values[2] * minors.c2 + values[3] * minors.c1) * inverse,
            (-values[12] * minors.s5 + values[14] * minors.s2 - values[15] * minors.s1) * inverse,
            (values[8] * minors.s5 - values[10] * minors.s2 + values[11] * minors.s1) * inverse,
            (values[4] * minors.c4 - values[5] * minors.c2 + values[7] * minors.c0) * inverse,
            (-values[0] * minors.c4 + values[1] * minors.c2 - values[3] * minors.c0) * inverse,
            (values[12] * minors.s4 - values[13] * minors.s2 + values[15] * minors.s0) * inverse,
            (-values[8] * minors.s4 + values[9] * minors.s2 - values[11] * minors.s0) * inverse,
            (-values[4] * minors.c3 + values[5] * minors.c1 - values[6] * minors.c0) * inverse,
            (values[0] * minors.c3 - values[1] * minors.c1 + values[2] * minors.c0) * inverse,
            (-values[12] * minors.s3 + values[13] * minors.s1 - values[14] * minors.s0) * inverse,
            (values[8] * minors.s3 - values[9] * minors.s1 + values[10] * minors.s0) * inverse,
        ))
    }

    pub const fn transform_point(self, point: Vec3) -> Vec3 {
        transform_point(self, point)
    }

    pub const fn transform_vector(self, vector: Vec3) -> Vec3 {
        transform_vector(self, vector)
    }

    pub fn nearly_equal(self, other: Self, epsilon: f64) -> bool {
        nearly_equal(self, other, epsilon)
    }
}

impl_matrix_access!(Mat4, 4);

impl Mul<Vec4> for Mat4 {
    type Output = Vec4;

    fn mul(self, vector: Vec4) -> Vec4 {
        self.multiply(vector)
    }
}

impl Mul for Mat4 {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        let mut result = Self::default();
        for row in 0..4 {
            for column in 0..4 {
                let mut sum = 0.0;
                for inner in 0..4 {
                    sum += self[(row, inner)] * other[(inner, column)];
                }
                result[(row, column)] = sum;
            }
        }
        result
    }
}

pub const fn transform_point(matrix: Mat4, point: Vec3) -> Vec3 {
    Vec3::new(
        matrix.data[0] * point.x
            + matrix.data[1] * point.y
            + matrix.data[2] * point.z
            + matrix.data[3],
        matrix.data[4] * point.x
            + matrix.data[5] * point.y
            + matrix.data[6] * point.z
            + matrix.data[7],
        matrix.data[8] * point.x
            + matrix.data[9] * point.y
            + matrix.data[10] * point.z
            + matrix.data[11],
    )
}

pub const fn transform_vector(matrix: Mat4, vector: Vec3) -> Vec3 {
    Vec3::new(
        matrix.data[0] * vector.x + matrix.data[1] * vector.y + matrix.data[2] * vector.z,
        matrix.data[4] * vector.x + matrix.data[5] * vector.y + matrix.data[6] * vector.z,
        matrix.data[8] * vector.x + matrix.data[9] * vector.y + matrix.data[10] * vector.z,
    )
}

pub fn nearly_equal(first: Mat4, second: Mat4, epsilon: f64) -> bool {
    first
        .data
        .iter()
        .zip(second.data)
        .all(|(&first, second)| numerics::nearly_equal(first, second, epsilon))
}

struct SubDeterminants {
    s0: f64,
    s1: f64,
    s2: f64,
    s3: f64,
    s4: f64,
    s5: f64,
    c0: f64,
    c1: f64,
    c2: f64,
    c3: f64,
    c4: f64,
    c5: f64,
}

impl SubDeterminants {
    const fn new(matrix: Mat4) -> Self {
        Self {
            s0: matrix.data[0] * matrix.data[5] - matrix.data[4] * matrix.data[1],
            s1: matrix.data[0] * matrix.data[6] - matrix.data[4] * matrix.data[2],
            s2: matrix.data[0] * matrix.data[7] - matrix.data[4] * matrix.data[3],
            s3: matrix.data[1] * matrix.data[6] - matrix.data[5] * matrix.data[2],
            s4: matrix.data[1] * matrix.data[7] - matrix.data[5] * matrix.data[3],
            s5: matrix.data[2] * matrix.data[7] - matrix.data[6] * matrix.data[3],
            c0: matrix.data[8] * matrix.data[13] - matrix.data[12] * matrix.data[9],
            c1: matrix.data[8] * matrix.data[14] - matrix.data[12] * matrix.data[10],
            c2: matrix.data[8] * matrix.data[15] - matrix.data[12] * matrix.data[11],
            c3: matrix.data[9] * matrix.data[14] - matrix.data[13] * matrix.data[10],
            c4: matrix.data[9] * matrix.data[15] - matrix.data[13] * matrix.data[11],
            c5: matrix.data[10] * matrix.data[15] - matrix.data[14] * matrix.data[11],
        }
    }

    const fn determinant(&self) -> f64 {
        self.s0 * self.c5 - self.s1 * self.c4 + self.s2 * self.c3 + self.s3 * self.c2
            - self.s4 * self.c1
            + self.s5 * self.c0
    }
}
