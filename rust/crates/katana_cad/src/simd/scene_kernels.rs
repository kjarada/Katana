//! The 3D scene build's kernels (src/katana_cad/simd/scene_kernels.hpp): the
//! AVX2 set on x86-64 (scene_avx2.rs), the NEON set on 64-bit ARM
//! (scene_neon.rs), chosen at run time by the active SIMD level, which
//! `KATANA_SIMD` and katana_core's set_simd_level decide.
//!
//! Each entry equals the scalar code of scene.rs bit for bit: the same
//! operations, in the same order, never fused. The layouts:
//!
//! ```text
//! points   the vertices, as Point3.
//! lifted   4 doubles per vertex: x, y, the exaggerated z, the TRUE z (what
//!          the elevation ramp reads).
//! normals  4 doubles per vertex: the x, y, z of the summed face normals, and
//!          a fourth that is never read.
//! params   doubles, at the PARAM_* indices below.
//! colours  0xAARRGGBB, as render's Rgba.
//! ```

#![allow(
    unsafe_code,
    reason = "Runtime-selected SIMD instructions are confined to the kernels, entered only with proof that the processor runs them."
)]

use katana_geometry::Point3;

#[cfg(target_arch = "x86_64")]
use super::scene_avx2 as arch;
#[cfg(target_arch = "aarch64")]
use super::scene_neon as arch;

/// The rows of the elevation ramp a kernel interpolates: for each channel,
/// the four lower stops and the step to the next one, as doubles (the step is
/// an exact small integer, so the kernel's a + step * f is the scalar
/// a + (b - a) * f to the bit).
pub(crate) const RAMP_SEGMENTS: usize = 4;

// Where each parameter sits in `params`.
pub(crate) const PARAM_LIFT_FACTOR: usize = 0;
pub(crate) const PARAM_LIFT_DATUM: usize = 1;
pub(crate) const PARAM_RAMP_LOW: usize = 2;
pub(crate) const PARAM_RAMP_SPAN: usize = 3;
/// 1.0 or 0.0.
pub(crate) const PARAM_LIGHT_ON: usize = 4;
pub(crate) const PARAM_SUN_X: usize = 5;
pub(crate) const PARAM_SUN_Y: usize = 6;
pub(crate) const PARAM_SUN_Z: usize = 7;
pub(crate) const PARAM_GROUND: usize = 8;
pub(crate) const PARAM_SKY_MINUS_GROUND: usize = 9;
pub(crate) const PARAM_SUN: usize = 10;
/// Red stops, red steps, green stops, green steps, blue stops, blue steps.
pub(crate) const PARAM_RAMP_TABLE: usize = 11;
pub(crate) const PARAM_COUNT: usize = PARAM_RAMP_TABLE + 6 * RAMP_SEGMENTS;

pub(crate) type KernelParams = [f64; PARAM_COUNT];

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

/// The kernels of the active SIMD level, or None at the scalar level or where
/// this build has none: the C++'s kernelActive().
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

/// lifted[4 i ..] = x, y, datum + (z - datum) * factor, z for each of the
/// points.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) fn lift(
    _kernels: Kernels,
    points: &[Point3],
    params: &KernelParams,
    lifted: &mut [f64],
) {
    assert!(lifted.len() >= 4 * points.len());
    // SAFETY: a Kernels exists only while the processor runs the instructions
    // of the active level, AVX2 on x86-64 and NEON on 64-bit ARM; the kernel
    // reaches only the slices it is handed, whose lengths are asserted.
    unsafe { arch::lift(points, params, lifted) }
}

/// For each triangle in order, the normal (b - a) x (c - a) of its lifted
/// corners, negated when its z is below zero, added to the normals of its
/// three corners in the order a, b, c. `normals` holds the sums so far.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) fn surface_normals(
    _kernels: Kernels,
    lifted: &[f64],
    triangles: &[[u32; 3]],
    normals: &mut [f64],
) {
    // SAFETY: as lift's; every corner's four doubles are bounds-checked in
    // the kernel before they are loaded or stored.
    unsafe { arch::surface_normals(lifted, triangles, normals) }
}

/// Each of the vertices of `positions`: its lifted position, and its colour -
/// `flat`, or the ramp at its true z when `use_ramp` - shaded by the
/// hillshade of its normal when `normals` is given.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(crate) fn surface_vertices(
    _kernels: Kernels,
    lifted: &[f64],
    normals: Option<&[f64]>,
    params: &KernelParams,
    flat: u32,
    use_ramp: bool,
    positions: &mut [Point3],
    colours: &mut [u32],
) {
    let count = positions.len();
    assert!(lifted.len() >= 4 * count && colours.len() >= count);
    assert!(normals.is_none_or(|normals| normals.len() >= 4 * count));
    // SAFETY: as lift's.
    unsafe { arch::surface_vertices(lifted, normals, params, flat, use_ramp, positions, colours) }
}

/// Faces [first, first + count) of a mesh, every index of which names one of
/// the points: the lifted corners, c then b then a, into positions (three a
/// face) and the face's colour, shaded by its own normal, three times into
/// colours. A face's base colour is face_colours[f] for f below their count,
/// else `flat`.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(crate) fn mesh_faces(
    _kernels: Kernels,
    points: &[Point3],
    faces: &[[u32; 3]],
    first: usize,
    count: usize,
    params: &KernelParams,
    face_colours: &[u32],
    flat: u32,
    positions: &mut [Point3],
    colours: &mut [u32],
) {
    assert!(first + count <= faces.len());
    assert!(positions.len() >= 3 * count && colours.len() >= 3 * count);
    // SAFETY: as lift's; every corner is read through a bounds-checked index.
    unsafe {
        arch::mesh_faces(
            points,
            faces,
            first,
            count,
            params,
            face_colours,
            flat,
            positions,
            colours,
        )
    }
}

/// out[i] = each channel of base[i] moved eighths/8 of the way to ink[i],
/// rounded half up, for eighths in 0..8.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub(crate) fn fade(_kernels: Kernels, base: &[u32], ink: &[u32], eighths: i32, out: &mut [u32]) {
    assert!(base.len() >= out.len() && ink.len() >= out.len());
    // SAFETY: as lift's.
    unsafe { arch::fade(base, ink, eighths, out) }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) fn lift(
    _kernels: Kernels,
    _points: &[Point3],
    _params: &KernelParams,
    _lifted: &mut [f64],
) {
    unreachable!("no scene kernels exist for this processor");
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) fn surface_normals(
    _kernels: Kernels,
    _lifted: &[f64],
    _triangles: &[[u32; 3]],
    _normals: &mut [f64],
) {
    unreachable!("no scene kernels exist for this processor");
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(crate) fn surface_vertices(
    _kernels: Kernels,
    _lifted: &[f64],
    _normals: Option<&[f64]>,
    _params: &KernelParams,
    _flat: u32,
    _use_ramp: bool,
    _positions: &mut [Point3],
    _colours: &mut [u32],
) {
    unreachable!("no scene kernels exist for this processor");
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(crate) fn mesh_faces(
    _kernels: Kernels,
    _points: &[Point3],
    _faces: &[[u32; 3]],
    _first: usize,
    _count: usize,
    _params: &KernelParams,
    _face_colours: &[u32],
    _flat: u32,
    _positions: &mut [Point3],
    _colours: &mut [u32],
) {
    unreachable!("no scene kernels exist for this processor");
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) fn fade(
    _kernels: Kernels,
    _base: &[u32],
    _ink: &[u32],
    _eighths: i32,
    _out: &mut [u32],
) {
    unreachable!("no scene kernels exist for this processor");
}
