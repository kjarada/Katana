//! Entry points of the AVX2 packing of a draw list's vertices for the GPU, and
//! of its NEON twin (pack_kernels.hpp, pack_avx2.cpp, pack_neon.cpp).
//!
//! Each equals the scalar loop in gpu_scene (pack_vertices) bit for bit: each
//! offset is the scalar's double subtraction, rounded to float by CVTPD2PS
//! (FCVTN on ARM), which rounds as `as f32` does - to nearest, under the
//! default MXCSR (FPCR) - and nothing is fused.

#![allow(
    unsafe_code,
    reason = "Runtime-selected SIMD instructions are confined to the kernels, entered only with proof that the processor runs them."
)]

use crate::gpu_scene::GpuVertex;
use katana_geometry::Point3;
use katana_render::Rgba;

/// Proof that the active SIMD level has a packing kernel in this build: made
/// only by kernels(), and only when the processor runs that level (the level
/// can be set only to what the processor was found to support, katana_core's
/// set_simd_level, which is how KATANA_SIMD's rule reaches this kernel too).
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

/// The packing kernel of the active SIMD level, or None at the scalar level
/// or where this build has none.
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

/// For each i < out.len(), with positions[i] the vertex's x, y and z:
/// out[i] is float(x - origin[0]), float(y - origin[1]),
/// float(z - origin[2]), colors[i]. `positions` and `colors` hold at least
/// out.len() entries.
#[cfg(target_arch = "x86_64")]
pub(crate) fn pack_vertices(
    _kernels: Kernels,
    positions: &[Point3],
    colors: &[Rgba],
    origin: [f64; 3],
    out: &mut [GpuVertex],
) {
    // SAFETY: a Kernels exists only while the processor runs AVX2.
    unsafe { super::pack_avx2::pack_vertices(positions, colors, origin, out) }
}

#[cfg(target_arch = "aarch64")]
pub(crate) fn pack_vertices(
    _kernels: Kernels,
    positions: &[Point3],
    colors: &[Rgba],
    origin: [f64; 3],
    out: &mut [GpuVertex],
) {
    // SAFETY: a Kernels exists only while the processor runs NEON.
    unsafe { super::pack_neon::pack_vertices(positions, colors, origin, out) }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) fn pack_vertices(
    _kernels: Kernels,
    _positions: &[Point3],
    _colors: &[Rgba],
    _origin: [f64; 3],
    _out: &mut [GpuVertex],
) {
    unreachable!("no packing kernels exist for this processor");
}
