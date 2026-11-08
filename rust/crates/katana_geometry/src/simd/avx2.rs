use crate::{Point2, Point3};
use katana_math::cpp::{max, min};
use std::arch::x86_64::*;

#[target_feature(enable = "avx2")]
pub(super) unsafe fn transform_points2(matrix: &[f64; 9], points: &mut [Point2]) {
    unsafe {
        let mut coefficients = [_mm256_setzero_pd(); 9];
        for (index, &value) in matrix.iter().enumerate() {
            coefficients[index] = _mm256_set1_pd(value);
        }
        let whole = points.len() / 4 * 4;
        for chunk in points[..whole].as_chunks_mut::<4>().0 {
            let east = _mm256_setr_pd(chunk[0].x, chunk[1].x, chunk[2].x, chunk[3].x);
            let north = _mm256_setr_pd(chunk[0].y, chunk[1].y, chunk[2].y, chunk[3].y);
            let transformed_x = _mm256_add_pd(
                _mm256_add_pd(
                    _mm256_mul_pd(coefficients[0], east),
                    _mm256_mul_pd(coefficients[1], north),
                ),
                coefficients[2],
            );
            let transformed_y = _mm256_add_pd(
                _mm256_add_pd(
                    _mm256_mul_pd(coefficients[3], east),
                    _mm256_mul_pd(coefficients[4], north),
                ),
                coefficients[5],
            );
            let mut xs = [0.0; 4];
            let mut ys = [0.0; 4];
            _mm256_storeu_pd(xs.as_mut_ptr(), transformed_x);
            _mm256_storeu_pd(ys.as_mut_ptr(), transformed_y);
            for lane in 0..4 {
                chunk[lane].x = xs[lane];
                chunk[lane].y = ys[lane];
            }
        }
        crate::point_batch::transform_scalar2(matrix, &mut points[whole..]);
    }
}
#[target_feature(enable = "avx2")]
pub(super) unsafe fn transform_points3(matrix: &[f64; 16], points: &mut [Point3]) {
    unsafe {
        let mut coefficients = [_mm256_setzero_pd(); 16];
        for (index, &value) in matrix.iter().enumerate() {
            coefficients[index] = _mm256_set1_pd(value);
        }
        let whole = points.len() / 4 * 4;
        for chunk in points[..whole].as_chunks_mut::<4>().0 {
            let east = _mm256_setr_pd(chunk[0].x, chunk[1].x, chunk[2].x, chunk[3].x);
            let north = _mm256_setr_pd(chunk[0].y, chunk[1].y, chunk[2].y, chunk[3].y);
            let height = _mm256_setr_pd(chunk[0].z, chunk[1].z, chunk[2].z, chunk[3].z);
            let mut transformed = [[0.0; 4]; 3];
            for (axis, coordinates) in transformed.iter_mut().enumerate() {
                let row = axis * 4;
                let result = _mm256_add_pd(
                    _mm256_add_pd(
                        _mm256_add_pd(
                            _mm256_mul_pd(coefficients[row], east),
                            _mm256_mul_pd(coefficients[row + 1], north),
                        ),
                        _mm256_mul_pd(coefficients[row + 2], height),
                    ),
                    coefficients[row + 3],
                );
                _mm256_storeu_pd(coordinates.as_mut_ptr(), result);
            }
            for lane in 0..4 {
                chunk[lane].x = transformed[0][lane];
                chunk[lane].y = transformed[1][lane];
                chunk[lane].z = transformed[2][lane];
            }
        }
        crate::point_batch::transform_scalar3(matrix, &mut points[whole..]);
    }
}
#[target_feature(enable = "avx2")]
unsafe fn bounds<const AXES: usize, Point>(
    points: &[Point],
    coordinate: fn(&Point) -> [f64; AXES],
) -> ([f64; AXES], [f64; AXES]) {
    unsafe {
        let mut lows = [_mm256_set1_pd(f64::INFINITY); AXES];
        let mut highs = [_mm256_set1_pd(f64::NEG_INFINITY); AXES];
        let whole = points.len() / 4 * 4;
        for chunk in points[..whole].as_chunks::<4>().0 {
            let values = [
                coordinate(&chunk[0]),
                coordinate(&chunk[1]),
                coordinate(&chunk[2]),
                coordinate(&chunk[3]),
            ];
            for axis in 0..AXES {
                let lanes = _mm256_setr_pd(
                    values[0][axis],
                    values[1][axis],
                    values[2][axis],
                    values[3][axis],
                );
                lows[axis] = _mm256_min_pd(lanes, lows[axis]);
                highs[axis] = _mm256_max_pd(lanes, highs[axis]);
            }
        }
        let mut minimum = [f64::INFINITY; AXES];
        let mut maximum = [f64::NEG_INFINITY; AXES];
        for axis in 0..AXES {
            let mut low = [0.0; 4];
            let mut high = [0.0; 4];
            _mm256_storeu_pd(low.as_mut_ptr(), lows[axis]);
            _mm256_storeu_pd(high.as_mut_ptr(), highs[axis]);
            for lane in 0..4 {
                minimum[axis] = min(minimum[axis], low[lane]);
                maximum[axis] = max(maximum[axis], high[lane]);
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
#[target_feature(enable = "avx2")]
pub(super) unsafe fn bounds2(points: &[Point2]) -> [f64; 4] {
    let (low, high) = unsafe { bounds(points, |point| [point.x, point.y]) };
    [low[0], low[1], high[0], high[1]]
}
#[target_feature(enable = "avx2")]
pub(super) unsafe fn bounds3(points: &[Point3]) -> [f64; 6] {
    let (low, high) = unsafe { bounds(points, |point| [point.x, point.y, point.z]) };
    [low[0], low[1], low[2], high[0], high[1], high[2]]
}
#[target_feature(enable = "avx2")]
pub(super) unsafe fn project_to_pixels(
    projection: &[f64; 5],
    xs: &[f32],
    ys: &[f32],
    px: &mut [i32],
    py: &mut [i32],
) {
    unsafe {
        let half_width = _mm256_set1_pd(projection[0]);
        let half_height = _mm256_set1_pd(projection[1]);
        let centre_x = _mm256_set1_pd(projection[2]);
        let centre_y = _mm256_set1_pd(projection[3]);
        let scale = _mm256_set1_pd(projection[4]);
        let low = _mm256_set1_pd(-1.0e6);
        let high = _mm256_set1_pd(1.0e6);
        let rejected = _mm_set1_epi32(i32::MIN);
        let low_halves = _mm256_setr_epi32(0, 2, 4, 6, 0, 2, 4, 6);
        let whole = xs.len() / 4 * 4;
        for index in (0..whole).step_by(4) {
            let east = _mm256_cvtps_pd(_mm_loadu_ps(xs.as_ptr().add(index)));
            let north = _mm256_cvtps_pd(_mm_loadu_ps(ys.as_ptr().add(index)));
            let screen_x = _mm256_add_pd(
                half_width,
                _mm256_mul_pd(_mm256_sub_pd(east, centre_x), scale),
            );
            let screen_y = _mm256_sub_pd(
                half_height,
                _mm256_mul_pd(_mm256_sub_pd(north, centre_y), scale),
            );
            let inside = _mm256_and_pd(
                _mm256_and_pd(
                    _mm256_cmp_pd::<_CMP_GT_OQ>(screen_x, low),
                    _mm256_cmp_pd::<_CMP_LT_OQ>(screen_x, high),
                ),
                _mm256_and_pd(
                    _mm256_cmp_pd::<_CMP_GT_OQ>(screen_y, low),
                    _mm256_cmp_pd::<_CMP_LT_OQ>(screen_y, high),
                ),
            );
            let mask = _mm256_castsi256_si128(_mm256_permutevar8x32_epi32(
                _mm256_castpd_si256(inside),
                low_halves,
            ));
            _mm_storeu_si128(
                px.as_mut_ptr().add(index).cast(),
                _mm_blendv_epi8(rejected, _mm256_cvttpd_epi32(screen_x), mask),
            );
            _mm_storeu_si128(
                py.as_mut_ptr().add(index).cast(),
                _mm_blendv_epi8(rejected, _mm256_cvttpd_epi32(screen_y), mask),
            );
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
