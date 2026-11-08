use crate::{Point2, Point3};
use katana_math::cpp::{max, min};
use std::arch::aarch64::*;

#[target_feature(enable = "neon")]
pub(super) unsafe fn transform_points2(matrix: &[f64; 9], points: &mut [Point2]) {
    unsafe {
        let mut coefficients = [vdupq_n_f64(0.0); 9];
        for (index, &value) in matrix.iter().enumerate() {
            coefficients[index] = vdupq_n_f64(value);
        }
        let whole = points.len() / 4 * 4;
        for chunk in points[..whole].as_chunks_mut::<4>().0 {
            for pair in chunk.as_chunks_mut::<2>().0 {
                let east = vld1q_f64([pair[0].x, pair[1].x].as_ptr());
                let north = vld1q_f64([pair[0].y, pair[1].y].as_ptr());
                let transformed_x = vaddq_f64(
                    vaddq_f64(
                        vmulq_f64(coefficients[0], east),
                        vmulq_f64(coefficients[1], north),
                    ),
                    coefficients[2],
                );
                let transformed_y = vaddq_f64(
                    vaddq_f64(
                        vmulq_f64(coefficients[3], east),
                        vmulq_f64(coefficients[4], north),
                    ),
                    coefficients[5],
                );
                let mut xs = [0.0; 2];
                let mut ys = [0.0; 2];
                vst1q_f64(xs.as_mut_ptr(), transformed_x);
                vst1q_f64(ys.as_mut_ptr(), transformed_y);
                for lane in 0..2 {
                    pair[lane].x = xs[lane];
                    pair[lane].y = ys[lane];
                }
            }
        }
        crate::point_batch::transform_scalar2(matrix, &mut points[whole..]);
    }
}
#[target_feature(enable = "neon")]
pub(super) unsafe fn transform_points3(matrix: &[f64; 16], points: &mut [Point3]) {
    unsafe {
        let mut coefficients = [vdupq_n_f64(0.0); 16];
        for (index, &value) in matrix.iter().enumerate() {
            coefficients[index] = vdupq_n_f64(value);
        }
        let whole = points.len() / 4 * 4;
        for chunk in points[..whole].as_chunks_mut::<4>().0 {
            for pair in chunk.as_chunks_mut::<2>().0 {
                let east = vld1q_f64([pair[0].x, pair[1].x].as_ptr());
                let north = vld1q_f64([pair[0].y, pair[1].y].as_ptr());
                let height = vld1q_f64([pair[0].z, pair[1].z].as_ptr());
                let mut transformed = [[0.0; 2]; 3];
                for (axis, coordinates) in transformed.iter_mut().enumerate() {
                    let row = axis * 4;
                    let result = vaddq_f64(
                        vaddq_f64(
                            vaddq_f64(
                                vmulq_f64(coefficients[row], east),
                                vmulq_f64(coefficients[row + 1], north),
                            ),
                            vmulq_f64(coefficients[row + 2], height),
                        ),
                        coefficients[row + 3],
                    );
                    vst1q_f64(coordinates.as_mut_ptr(), result);
                }
                for lane in 0..2 {
                    pair[lane].x = transformed[0][lane];
                    pair[lane].y = transformed[1][lane];
                    pair[lane].z = transformed[2][lane];
                }
            }
        }
        crate::point_batch::transform_scalar3(matrix, &mut points[whole..]);
    }
}
#[target_feature(enable = "neon")]
unsafe fn bounds<const AXES: usize, Point>(
    points: &[Point],
    coordinate: fn(&Point) -> [f64; AXES],
) -> ([f64; AXES], [f64; AXES]) {
    unsafe {
        let mut lows = [[vdupq_n_f64(f64::INFINITY); AXES]; 2];
        let mut highs = [[vdupq_n_f64(f64::NEG_INFINITY); AXES]; 2];
        let whole = points.len() / 4 * 4;
        for chunk in points[..whole].as_chunks::<4>().0 {
            let values = [
                coordinate(&chunk[0]),
                coordinate(&chunk[1]),
                coordinate(&chunk[2]),
                coordinate(&chunk[3]),
            ];
            for pair in 0..2 {
                for axis in 0..AXES {
                    let lanes =
                        vld1q_f64([values[2 * pair][axis], values[2 * pair + 1][axis]].as_ptr());
                    lows[pair][axis] =
                        vbslq_f64(vcltq_f64(lanes, lows[pair][axis]), lanes, lows[pair][axis]);
                    highs[pair][axis] = vbslq_f64(
                        vcgtq_f64(lanes, highs[pair][axis]),
                        lanes,
                        highs[pair][axis],
                    );
                }
            }
        }
        let mut minimum = [f64::INFINITY; AXES];
        let mut maximum = [f64::NEG_INFINITY; AXES];
        for axis in 0..AXES {
            for pair in 0..2 {
                let mut low = [0.0; 2];
                let mut high = [0.0; 2];
                vst1q_f64(low.as_mut_ptr(), lows[pair][axis]);
                vst1q_f64(high.as_mut_ptr(), highs[pair][axis]);
                for lane in 0..2 {
                    minimum[axis] = min(minimum[axis], low[lane]);
                    maximum[axis] = max(maximum[axis], high[lane]);
                }
            }
        }
        for point in &points[whole..] {
            let values = coordinate(point);
            for axis in 0..AXES {
                minimum[axis] = min(minimum[axis], values[axis]);
                maximum[axis] = max(maximum[axis], values[axis]);
            }
        }
        (minimum, maximum)
    }
}
#[target_feature(enable = "neon")]
pub(super) unsafe fn bounds2(points: &[Point2]) -> [f64; 4] {
    let (low, high) = unsafe { bounds(points, |point| [point.x, point.y]) };
    [low[0], low[1], high[0], high[1]]
}
#[target_feature(enable = "neon")]
pub(super) unsafe fn bounds3(points: &[Point3]) -> [f64; 6] {
    let (low, high) = unsafe { bounds(points, |point| [point.x, point.y, point.z]) };
    [low[0], low[1], low[2], high[0], high[1], high[2]]
}
#[target_feature(enable = "neon")]
pub(super) unsafe fn project_to_pixels(
    projection: &[f64; 5],
    xs: &[f32],
    ys: &[f32],
    px: &mut [i32],
    py: &mut [i32],
) {
    unsafe {
        let half_width = vdupq_n_f64(projection[0]);
        let half_height = vdupq_n_f64(projection[1]);
        let centre_x = vdupq_n_f64(projection[2]);
        let centre_y = vdupq_n_f64(projection[3]);
        let scale = vdupq_n_f64(projection[4]);
        let low = vdupq_n_f64(-1.0e6);
        let high = vdupq_n_f64(1.0e6);
        let rejected = vdup_n_s32(i32::MIN);
        let whole = xs.len() / 4 * 4;
        for chunk in (0..whole).step_by(4) {
            for index in [chunk, chunk + 2] {
                let east = vcvt_f64_f32(vld1_f32(xs.as_ptr().add(index)));
                let north = vcvt_f64_f32(vld1_f32(ys.as_ptr().add(index)));
                let screen_x = vaddq_f64(half_width, vmulq_f64(vsubq_f64(east, centre_x), scale));
                let screen_y = vsubq_f64(half_height, vmulq_f64(vsubq_f64(north, centre_y), scale));
                let inside = vandq_u64(
                    vandq_u64(vcgtq_f64(screen_x, low), vcltq_f64(screen_x, high)),
                    vandq_u64(vcgtq_f64(screen_y, low), vcltq_f64(screen_y, high)),
                );
                let mask = vmovn_u64(inside);
                let converted_x = vmovn_s64(vcvtq_s64_f64(screen_x));
                let converted_y = vmovn_s64(vcvtq_s64_f64(screen_y));
                vst1_s32(
                    px.as_mut_ptr().add(index),
                    vbsl_s32(mask, converted_x, rejected),
                );
                vst1_s32(
                    py.as_mut_ptr().add(index),
                    vbsl_s32(mask, converted_y, rejected),
                );
            }
        }
        super::project_scalar(
            projection,
            &xs[whole..],
            &ys[whole..],
            &mut px[whole..],
            &mut py[whole..],
        );
    }
}
