use crate::{Box2, Point2, Point3};
use katana_math::primitives::AABB as Aabb;
use katana_math::{Mat3, Mat4};

pub const BOUNDS_BATCH_MINIMUM: usize = 16;
const TRANSFORM_MINIMUM: usize = 8;

pub trait TransformPoints<Point> {
    fn transform_points(&self, points: &mut [Point]);
}
pub fn transform_points<Point, Matrix: TransformPoints<Point>>(
    matrix: &Matrix,
    points: &mut [Point],
) {
    matrix.transform_points(points);
}
impl TransformPoints<Point2> for Mat3 {
    fn transform_points(&self, points: &mut [Point2]) {
        if points.len() >= TRANSFORM_MINIMUM && crate::simd::transform_points2(&self.data, points) {
            return;
        }
        transform_scalar2(&self.data, points);
    }
}
impl TransformPoints<Point3> for Mat4 {
    fn transform_points(&self, points: &mut [Point3]) {
        if points.len() >= TRANSFORM_MINIMUM && crate::simd::transform_points3(&self.data, points) {
            return;
        }
        transform_scalar3(&self.data, points);
    }
}
pub(crate) fn transform_scalar2(matrix: &[f64; 9], points: &mut [Point2]) {
    for point in points {
        let east = point.x;
        let north = point.y;
        point.x = matrix[0] * east + matrix[1] * north + matrix[2];
        point.y = matrix[3] * east + matrix[4] * north + matrix[5];
    }
}
pub(crate) fn transform_scalar3(matrix: &[f64; 16], points: &mut [Point3]) {
    for point in points {
        let east = point.x;
        let north = point.y;
        let height = point.z;
        point.x = matrix[0] * east + matrix[1] * north + matrix[2] * height + matrix[3];
        point.y = matrix[4] * east + matrix[5] * north + matrix[6] * height + matrix[7];
        point.z = matrix[8] * east + matrix[9] * north + matrix[10] * height + matrix[11];
    }
}
#[inline(always)]
pub const fn bounds_of_2d_inline(points: &[Point2]) -> Box2 {
    let mut bounds = Box2 {
        min: Point2 {
            x: f64::INFINITY,
            y: f64::INFINITY,
        },
        max: Point2 {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
        },
    };
    let mut index = 0;
    while index < points.len() {
        let point = points[index];
        if point.x < bounds.min.x {
            bounds.min.x = point.x;
        }
        if point.y < bounds.min.y {
            bounds.min.y = point.y;
        }
        if bounds.max.x < point.x {
            bounds.max.x = point.x;
        }
        if bounds.max.y < point.y {
            bounds.max.y = point.y;
        }
        index += 1;
    }
    bounds
}
#[inline(always)]
pub const fn bounds_of_3d_inline(points: &[Point3]) -> Aabb {
    let mut bounds = Aabb {
        min: Point3 {
            x: f64::INFINITY,
            y: f64::INFINITY,
            z: f64::INFINITY,
        },
        max: Point3 {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
            z: f64::NEG_INFINITY,
        },
    };
    let mut index = 0;
    while index < points.len() {
        let point = points[index];
        if point.x < bounds.min.x {
            bounds.min.x = point.x;
        }
        if point.y < bounds.min.y {
            bounds.min.y = point.y;
        }
        if point.z < bounds.min.z {
            bounds.min.z = point.z;
        }
        if bounds.max.x < point.x {
            bounds.max.x = point.x;
        }
        if bounds.max.y < point.y {
            bounds.max.y = point.y;
        }
        if bounds.max.z < point.z {
            bounds.max.z = point.z;
        }
        index += 1;
    }
    bounds
}
pub trait BoundsPoint: Sized {
    type Bounds;
    fn bounds_of(points: &[Self]) -> Self::Bounds;
    fn bounds_of_batch(points: &[Self]) -> Self::Bounds;
}
#[inline(always)]
pub fn bounds_of<Point: BoundsPoint>(points: &[Point]) -> Point::Bounds {
    Point::bounds_of(points)
}
fn settle_zero<Point>(extreme: &mut f64, points: &[Point], coordinate: impl Fn(&Point) -> f64) {
    if *extreme == 0.0 {
        *extreme = points
            .iter()
            .map(coordinate)
            .find(|&value| value == 0.0)
            .unwrap_or(0.0);
    }
}
impl BoundsPoint for Point2 {
    type Bounds = Box2;
    #[inline(always)]
    fn bounds_of(points: &[Self]) -> Box2 {
        if points.len() < BOUNDS_BATCH_MINIMUM {
            bounds_of_2d_inline(points)
        } else {
            Self::bounds_of_batch(points)
        }
    }
    fn bounds_of_batch(points: &[Self]) -> Box2 {
        let Some(mut extrema) = crate::simd::bounds2(points) else {
            return bounds_of_2d_inline(points);
        };
        for (index, extreme) in extrema.iter_mut().enumerate() {
            settle_zero(extreme, points, |point| {
                if index % 2 == 0 { point.x } else { point.y }
            });
        }
        Box2::new(
            Point2::new(extrema[0], extrema[1]),
            Point2::new(extrema[2], extrema[3]),
        )
    }
}
impl BoundsPoint for Point3 {
    type Bounds = Aabb;
    #[inline(always)]
    fn bounds_of(points: &[Self]) -> Aabb {
        if points.len() < BOUNDS_BATCH_MINIMUM {
            bounds_of_3d_inline(points)
        } else {
            Self::bounds_of_batch(points)
        }
    }
    fn bounds_of_batch(points: &[Self]) -> Aabb {
        let Some(mut extrema) = crate::simd::bounds3(points) else {
            return bounds_of_3d_inline(points);
        };
        for (index, extreme) in extrema.iter_mut().enumerate() {
            settle_zero(extreme, points, |point| match index % 3 {
                0 => point.x,
                1 => point.y,
                _ => point.z,
            });
        }
        Aabb {
            min: Point3::new(extrema[0], extrema[1], extrema[2]),
            max: Point3::new(extrema[3], extrema[4], extrema[5]),
        }
    }
}
pub mod detail {
    use super::BoundsPoint;
    pub fn bounds_of_batch<Point: BoundsPoint>(points: &[Point]) -> Point::Bounds {
        Point::bounds_of_batch(points)
    }
}
