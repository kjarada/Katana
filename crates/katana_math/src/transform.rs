use crate::{Mat4, Quaternion, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quaternion,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::new(
            Vec3::default(),
            Quaternion::default(),
            Vec3::new(1.0, 1.0, 1.0),
        )
    }
}

impl Transform {
    pub const fn new(translation: Vec3, rotation: Quaternion, scale: Vec3) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    pub const fn matrix(self) -> Mat4 {
        let rotation = self.rotation.to_matrix().data;
        Mat4::new(
            rotation[0] * self.scale.x,
            rotation[1] * self.scale.y,
            rotation[2] * self.scale.z,
            self.translation.x,
            rotation[3] * self.scale.x,
            rotation[4] * self.scale.y,
            rotation[5] * self.scale.z,
            self.translation.y,
            rotation[6] * self.scale.x,
            rotation[7] * self.scale.y,
            rotation[8] * self.scale.z,
            self.translation.z,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    pub fn transform_point(self, point: Vec3) -> Vec3 {
        self.rotation.rotate(Vec3::new(
            point.x * self.scale.x,
            point.y * self.scale.y,
            point.z * self.scale.z,
        )) + self.translation
    }

    pub fn transform_vector(self, vector: Vec3) -> Vec3 {
        self.rotation.rotate(Vec3::new(
            vector.x * self.scale.x,
            vector.y * self.scale.y,
            vector.z * self.scale.z,
        ))
    }

    pub fn inverse_transform_point(self, point: Vec3) -> Option<Vec3> {
        if self.scale.x == 0.0 || self.scale.y == 0.0 || self.scale.z == 0.0 {
            return None;
        }
        let unrotated = self.rotation.conjugate().rotate(point - self.translation);
        Some(Vec3::new(
            unrotated.x / self.scale.x,
            unrotated.y / self.scale.y,
            unrotated.z / self.scale.z,
        ))
    }
}
