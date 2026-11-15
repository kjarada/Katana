//! The rasteriser's kernels: the vertex transform four vertices a step, and
//! the fill eight pixels a step on AVX2 (four on NEON), chosen at run time.
//!
//! Each kernel gives exactly the bits of its scalar reference in rasterizer.rs
//! (the transform of one vertex and shade_pixel): every lane does the
//! reference's operations in the reference's order, with the same
//! comparisons, and nothing is fused. The rasteriser promises the same frame
//! on every thread count (Rule 7), and the SIMD level must not be a second way
//! to break that.

#![allow(
    unsafe_code,
    reason = "Runtime-selected SIMD instructions are confined to the kernels, entered only with proof that the processor runs them."
)]

use crate::disjoint::TilePixels;
use crate::framebuffer::Rgba;
use crate::rasterizer::{ClipVertex, ScreenTriangle, ScreenVertex};
use katana_geometry::Point3;

#[cfg(target_arch = "x86_64")]
mod avx2;
#[cfg(target_arch = "aarch64")]
mod neon;

#[cfg(target_arch = "x86_64")]
use self::avx2 as arch;
#[cfg(target_arch = "aarch64")]
use self::neon as arch;

/// Proof that the active SIMD level has kernels in this build: made only by
/// kernels(), and only when the processor runs that level (the level can be
/// set only to what the processor was found to support, katana_core's
/// set_simd_level).
#[cfg_attr(
    not(any(target_arch = "x86_64", target_arch = "aarch64")),
    allow(
        dead_code,
        reason = "No kernels exist for this processor, so no proof is ever made."
    )
)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Kernels {
    _proof: (),
}

/// The kernels of the active SIMD level, or None at the scalar level or
/// where this build has none.
pub(crate) fn kernels() -> Option<Kernels> {
    #[cfg(target_arch = "x86_64")]
    if katana_core::cpu_features::active_simd_level() == katana_core::cpu_features::SimdLevel::Avx2
    {
        return Some(Kernels { _proof: () });
    }
    #[cfg(target_arch = "aarch64")]
    if katana_core::cpu_features::active_simd_level() == katana_core::cpu_features::SimdLevel::Neon
    {
        return Some(Kernels { _proof: () });
    }
    None
}

/// The transform of `positions.len() / 4 * 4` vertices: for vertex i, the clip
/// coordinates (the matrix applied in double as Mat4::multiply does, then
/// rounded to float), the clip code, and the projection for a viewport of
/// `width` x `height` pixels - written for every vertex, meaningful where the
/// code is 0 - each with the colour colors[i], white from colors.len() on.
/// The caller transforms the remainder with the reference.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(crate) fn transform_vertices(
    _kernels: Kernels,
    mvp: &[f64; 16],
    positions: &[Point3],
    colors: &[Rgba],
    width: f32,
    height: f32,
    clip: &mut [ClipVertex],
    codes: &mut [u8],
    screen: &mut [ScreenVertex],
) {
    // SAFETY: a Kernels exists only while the processor runs the instructions
    // of the active level, AVX2 on x86-64 and NEON on 64-bit ARM.
    unsafe {
        arch::transform_vertices(mvp, positions, colors, width, height, clip, codes, screen);
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(crate) fn transform_vertices(
    _kernels: Kernels,
    _mvp: &[f64; 16],
    _positions: &[Point3],
    _colors: &[Rgba],
    _width: f32,
    _height: f32,
    _clip: &mut [ClipVertex],
    _codes: &mut [u8],
    _screen: &mut [ScreenVertex],
) {
    unreachable!("no raster kernels exist for this processor");
}

/// The fill of one screen triangle over rows first_row .. first_row +
/// row_count of one tile: row first_row + r covers the pixels spans[2 r] ..=
/// spans[2 r + 1] (none where the first is greater). Each pixel is shaded
/// exactly as shade_pixel shades it; where `covered` every pixel of every span
/// is known to be inside the triangle and the three edge tests are skipped
/// (they would all pass). Returns how many pixels were written.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry."
)]
pub(crate) fn shade_rows(
    _kernels: Kernels,
    triangle: &ScreenTriangle,
    inv_area: f32,
    spans: &[i32],
    first_row: i32,
    row_count: i32,
    covered: bool,
    depth_write: bool,
    pixels: &mut TilePixels<'_>,
) -> usize {
    // SAFETY: a Kernels exists only while the processor runs the instructions
    // of the active level, AVX2 on x86-64 and NEON on 64-bit ARM.
    unsafe {
        arch::shade_rows(
            triangle,
            inv_area,
            spans,
            first_row,
            row_count,
            covered,
            depth_write,
            pixels,
        )
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry."
)]
pub(crate) fn shade_rows(
    _kernels: Kernels,
    _triangle: &ScreenTriangle,
    _inv_area: f32,
    _spans: &[i32],
    _first_row: i32,
    _row_count: i32,
    _covered: bool,
    _depth_write: bool,
    _pixels: &mut TilePixels<'_>,
) -> usize {
    unreachable!("no raster kernels exist for this processor");
}
