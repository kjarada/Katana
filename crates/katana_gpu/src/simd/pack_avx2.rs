//! AVX2 packing of a draw list's vertices for the GPU (pack_avx2.cpp).
//!
//! Four vertices a step. Their twelve doubles are three registers,
//!   A = x0 y0 z0 x1,  B = y1 z1 x2 y2,  C = z2 x3 y3 z3,
//! so the origin is laid out in the same pattern and subtracted lane by lane -
//! each lane the scalar loop's own subtraction - then each register is rounded
//! to four floats (CVTPD2PS, round to nearest as `as f32`). Each vertex's x, y,
//! z are then brought to lanes 0-2 by one byte rotation and its colour blended
//! into lane 3: the 16-byte GpuVertex, written whole.
//!
//! The C++ loads the twelve doubles with three unaligned loads, as its Point3
//! is three doubles laid end to end. Rust does not promise Vec3's field order,
//! so the registers are built from the fields by name; the lanes hold the same
//! doubles, and so the same bits come out.
//!
//! A safe function, entered from pack_kernels.rs with the proof that the
//! processor runs AVX2. A vertex register becomes a GpuVertex through its four
//! 32-bit lanes (katana_core's x86_lanes) and `f32::from_bits`, which moves the
//! bits unchanged; LLVM merges the four lane stores of one register into the
//! one 16-byte store the C++ made through a pointer.

use crate::gpu_scene::GpuVertex;
use katana_core::simd::x86_lanes::{u32x4, u32x4_lanes};
use katana_geometry::Point3;
use katana_render::Rgba;
use std::arch::x86_64::*;

// The broadcasts below are _MM_SHUFFLE(n, n, n, n) - 0x00, 0x55, 0xAA, 0xFF, lane n
// to every lane - written as values: _MM_SHUFFLE is not yet a stable const fn.

/// The vertex whose x, y, z (as float bits) and colour are lanes 0-3.
#[inline]
#[target_feature(enable = "avx2")]
fn vertex(words: __m128i) -> GpuVertex {
    let [x, y, z, color] = u32x4_lanes(words);
    GpuVertex {
        x: f32::from_bits(x),
        y: f32::from_bits(y),
        z: f32::from_bits(z),
        color,
    }
}

/// The contract is pack_kernels::pack_vertices'.
#[target_feature(enable = "avx2")]
#[allow(
    clippy::needless_range_loop,
    reason = "The tail's index is the vertex's in `out` and in both inputs, as in the C++ kernel."
)]
pub(super) fn pack_vertices(
    positions: &[Point3],
    colors: &[Rgba],
    origin: [f64; 3],
    out: &mut [GpuVertex],
) {
    let count = out.len();
    assert!(positions.len() >= count && colors.len() >= count);
    let whole = count / 4 * 4;
    let origin_a = _mm256_setr_pd(origin[0], origin[1], origin[2], origin[0]);
    let origin_b = _mm256_setr_pd(origin[1], origin[2], origin[0], origin[1]);
    let origin_c = _mm256_setr_pd(origin[2], origin[0], origin[1], origin[2]);
    let steps = positions[..whole]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(colors[..whole].as_chunks::<4>().0)
        .zip(out[..whole].as_chunks_mut::<4>().0);
    for ((p, inks), vertices) in steps {
        // x0 y0 z0 x1
        let a = _mm_castps_si128(_mm256_cvtpd_ps(_mm256_sub_pd(
            _mm256_setr_pd(p[0].x, p[0].y, p[0].z, p[1].x),
            origin_a,
        )));
        // y1 z1 x2 y2
        let b = _mm_castps_si128(_mm256_cvtpd_ps(_mm256_sub_pd(
            _mm256_setr_pd(p[1].y, p[1].z, p[2].x, p[2].y),
            origin_b,
        )));
        // z2 x3 y3 z3
        let c = _mm_castps_si128(_mm256_cvtpd_ps(_mm256_sub_pd(
            _mm256_setr_pd(p[2].z, p[3].x, p[3].y, p[3].z),
            origin_c,
        )));
        let ink = u32x4(inks);

        let v1 = _mm_alignr_epi8::<12>(b, a); // x1 y1 z1 .
        let v2 = _mm_alignr_epi8::<8>(c, b); // x2 y2 z2 .
        let v3 = _mm_srli_si128::<4>(c); // x3 y3 z3 .
        vertices[0] = vertex(_mm_blend_epi32::<0x8>(a, _mm_shuffle_epi32::<0x00>(ink)));
        vertices[1] = vertex(_mm_blend_epi32::<0x8>(v1, _mm_shuffle_epi32::<0x55>(ink)));
        vertices[2] = vertex(_mm_blend_epi32::<0x8>(v2, _mm_shuffle_epi32::<0xAA>(ink)));
        vertices[3] = vertex(_mm_blend_epi32::<0x8>(v3, _mm_shuffle_epi32::<0xFF>(ink)));
    }
    // The last one to three, one at a time: its x and y as a pair, its z
    // alone.
    for i in whole..count {
        let p = positions[i];
        let xy = _mm_sub_pd(_mm_setr_pd(p.x, p.y), _mm_setr_pd(origin[0], origin[1]));
        let z = _mm_sub_sd(_mm_set_sd(p.z), _mm_set_sd(origin[2]));
        let xyz = _mm_movelh_ps(_mm_cvtpd_ps(xy), _mm_cvtpd_ps(z)); // x y z 0
        out[i] = vertex(_mm_insert_epi32::<3>(_mm_castps_si128(xyz), colors[i] as i32));
    }
}
