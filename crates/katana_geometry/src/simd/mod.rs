//! The point kernels' entries: each runs the active SIMD level's kernel, AVX2
//! on x86-64 or NEON on AArch64, and says false (None) at the scalar level,
//! where the caller runs its loop. The kernels are safe functions of slices
//! (avx2.rs, neon.rs); the one unsafe step, entering a target-feature
//! function from code without the feature, is here, one call an entry, each
//! resting on the level: katana_core's set_simd_level makes it AVX2 only on a
//! processor found to run AVX2, and NEON is part of every AArch64 target.

use crate::{Point2, Point3};
use katana_core::cpu_features::{SimdLevel, active_simd_level};
#[cfg(target_arch = "x86_64")]
mod avx2;
#[cfg(target_arch = "aarch64")]
mod neon;

#[allow(
    unsafe_code,
    reason = "Entering the active level's target-feature kernel, which the processor runs."
)]
pub(crate) fn transform_points2(matrix: &[f64; 9], points: &mut [Point2]) -> bool {
    #[cfg(target_arch = "x86_64")]
    if active_simd_level() == SimdLevel::Avx2 {
        // SAFETY: the level is AVX2 (see the module's note).
        unsafe { avx2::transform_points2(matrix, points) };
        return true;
    }
    #[cfg(target_arch = "aarch64")]
    if active_simd_level() == SimdLevel::Neon {
        // SAFETY: the level is NEON (see the module's note).
        unsafe { neon::transform_points2(matrix, points) };
        return true;
    }
    let _ = (matrix, points);
    false
}
#[allow(
    unsafe_code,
    reason = "Entering the active level's target-feature kernel, which the processor runs."
)]
pub(crate) fn transform_points3(matrix: &[f64; 16], points: &mut [Point3]) -> bool {
    #[cfg(target_arch = "x86_64")]
    if active_simd_level() == SimdLevel::Avx2 {
        // SAFETY: the level is AVX2 (see the module's note).
        unsafe { avx2::transform_points3(matrix, points) };
        return true;
    }
    #[cfg(target_arch = "aarch64")]
    if active_simd_level() == SimdLevel::Neon {
        // SAFETY: the level is NEON (see the module's note).
        unsafe { neon::transform_points3(matrix, points) };
        return true;
    }
    let _ = (matrix, points);
    false
}
#[allow(
    unsafe_code,
    reason = "Entering the active level's target-feature kernel, which the processor runs."
)]
pub(crate) fn bounds2(points: &[Point2]) -> Option<[f64; 4]> {
    #[cfg(target_arch = "x86_64")]
    if active_simd_level() == SimdLevel::Avx2 {
        // SAFETY: the level is AVX2 (see the module's note).
        return Some(unsafe { avx2::bounds2(points) });
    }
    #[cfg(target_arch = "aarch64")]
    if active_simd_level() == SimdLevel::Neon {
        // SAFETY: the level is NEON (see the module's note).
        return Some(unsafe { neon::bounds2(points) });
    }
    let _ = points;
    None
}
#[allow(
    unsafe_code,
    reason = "Entering the active level's target-feature kernel, which the processor runs."
)]
pub(crate) fn bounds3(points: &[Point3]) -> Option<[f64; 6]> {
    #[cfg(target_arch = "x86_64")]
    if active_simd_level() == SimdLevel::Avx2 {
        // SAFETY: the level is AVX2 (see the module's note).
        return Some(unsafe { avx2::bounds3(points) });
    }
    #[cfg(target_arch = "aarch64")]
    if active_simd_level() == SimdLevel::Neon {
        // SAFETY: the level is NEON (see the module's note).
        return Some(unsafe { neon::bounds3(points) });
    }
    let _ = points;
    None
}
#[allow(
    unsafe_code,
    reason = "Entering the active level's target-feature kernel, which the processor runs."
)]
pub(crate) fn project_to_pixels(
    projection: &[f64; 5],
    xs: &[f32],
    ys: &[f32],
    px: &mut [i32],
    py: &mut [i32],
) -> bool {
    assert!(ys.len() >= xs.len() && px.len() >= xs.len() && py.len() >= xs.len());
    #[cfg(target_arch = "x86_64")]
    if active_simd_level() == SimdLevel::Avx2 {
        // SAFETY: the level is AVX2 (see the module's note).
        unsafe { avx2::project_to_pixels(projection, xs, ys, px, py) };
        return true;
    }
    #[cfg(target_arch = "aarch64")]
    if active_simd_level() == SimdLevel::Neon {
        // SAFETY: the level is NEON (see the module's note).
        unsafe { neon::project_to_pixels(projection, xs, ys, px, py) };
        return true;
    }
    let _ = (projection, xs, ys, px, py);
    false
}
pub(crate) fn project_scalar(
    projection: &[f64; 5],
    xs: &[f32],
    ys: &[f32],
    px: &mut [i32],
    py: &mut [i32],
) {
    for index in 0..xs.len() {
        let screen_x = projection[0] + (xs[index] as f64 - projection[2]) * projection[4];
        let screen_y = projection[1] - (ys[index] as f64 - projection[3]) * projection[4];
        if screen_x > -1.0e6 && screen_x < 1.0e6 && screen_y > -1.0e6 && screen_y < 1.0e6 {
            px[index] = screen_x as i32;
            py[index] = screen_y as i32;
        } else {
            px[index] = i32::MIN;
            py[index] = i32::MIN;
        }
    }
}
