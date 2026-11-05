//! The `math` layer: vectors, matrices, quaternions, the tolerance policy, unit ratios.

#[macro_use]
mod vector_ops;
#[macro_use]
mod matrix_ops;

pub mod mat2;
pub mod mat3;
pub mod mat4;
pub mod numerics;
pub mod primitives;
pub mod quaternion;
pub mod summation;
pub mod transform;
pub mod unit_ratio;
pub mod vec2;
pub mod vec3;
pub mod vec4;

pub use mat2::Mat2;
pub use mat3::Mat3;
pub use mat4::Mat4;
pub use numerics::*;
pub use primitives::{AABB, Plane, Ray, RayBoxHit, intersect, intersect_box, intersect_plane};
pub use quaternion::{Quaternion, slerp};
pub use summation::CompensatedSum;
pub use transform::Transform;
pub use unit_ratio::{UnitRatio, from_metres, ratio_value, scale_by_ratio, to_metres, units};
pub use vec2::Vec2;
pub use vec3::Vec3;
pub use vec4::Vec4;
