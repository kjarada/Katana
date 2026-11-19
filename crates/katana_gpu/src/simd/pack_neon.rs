//! NEON packing of a draw list's vertices for the GPU, for 64-bit ARM
//! (pack_neon.cpp).
//!
//! Four vertices a step, as pack_avx2 takes them. The C++'s LD3 splits two
//! vertices' six doubles into x, y and z registers; Rust does not promise
//! Vec3's field order, so the same pairs are built from the fields by name.
//! Each lane is the scalar loop's own subtraction of the origin, rounded to
//! float by FCVTN, which rounds to nearest (the FPCR default) as `as f32`
//! does. The C++'s ST4 then interleaved x, y, z and the colour words into four
//! whole GpuVertex records through a pointer; here each record is built from
//! the registers' lanes (katana_core's neon_lanes) and its colour word, which
//! is copied as the bits it is, never converted - the same sixteen words.
//!
//! A safe function, entered from pack_kernels.rs with the proof that the
//! processor runs NEON.

use crate::gpu_scene::GpuVertex;
use katana_core::simd::neon_lanes::{f32x4_lanes, f64x2};
use katana_geometry::Point3;
use katana_render::Rgba;
use std::arch::aarch64::*;

/// The contract is pack_kernels::pack_vertices'.
#[target_feature(enable = "neon")]
pub(super) fn pack_vertices(
    positions: &[Point3],
    colors: &[Rgba],
    origin: [f64; 3],
    out: &mut [GpuVertex],
) {
    let count = out.len();
    assert!(positions.len() >= count && colors.len() >= count);
    let whole = count / 4 * 4;
    let ox = vdupq_n_f64(origin[0]);
    let oy = vdupq_n_f64(origin[1]);
    let oz = vdupq_n_f64(origin[2]);
    let steps = positions[..whole]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(colors[..whole].as_chunks::<4>().0)
        .zip(out[..whole].as_chunks_mut::<4>().0);
    for ((p, inks), vertices) in steps {
        // Vertices i, i + 1, then i + 2, i + 3: x, y and z apart.
        let ax = f64x2(&[p[0].x, p[1].x]);
        let ay = f64x2(&[p[0].y, p[1].y]);
        let az = f64x2(&[p[0].z, p[1].z]);
        let bx = f64x2(&[p[2].x, p[3].x]);
        let by = f64x2(&[p[2].y, p[3].y]);
        let bz = f64x2(&[p[2].z, p[3].z]);
        let xs = f32x4_lanes(vcombine_f32(
            vcvt_f32_f64(vsubq_f64(ax, ox)),
            vcvt_f32_f64(vsubq_f64(bx, ox)),
        ));
        let ys = f32x4_lanes(vcombine_f32(
            vcvt_f32_f64(vsubq_f64(ay, oy)),
            vcvt_f32_f64(vsubq_f64(by, oy)),
        ));
        let zs = f32x4_lanes(vcombine_f32(
            vcvt_f32_f64(vsubq_f64(az, oz)),
            vcvt_f32_f64(vsubq_f64(bz, oz)),
        ));
        for (k, vertex) in vertices.iter_mut().enumerate() {
            *vertex = GpuVertex {
                x: xs[k],
                y: ys[k],
                z: zs[k],
                color: inks[k],
            };
        }
    }
    // The last one to three, one at a time.
    for ((vertex, position), &color) in out.iter_mut().zip(positions).zip(colors).skip(whole) {
        *vertex = GpuVertex {
            x: (position.x - origin[0]) as f32,
            y: (position.y - origin[1]) as f32,
            z: (position.z - origin[2]) as f32,
            color,
        };
    }
}
