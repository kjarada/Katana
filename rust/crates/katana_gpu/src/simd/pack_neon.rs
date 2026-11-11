//! NEON packing of a draw list's vertices for the GPU, for 64-bit ARM
//! (pack_neon.cpp).
//!
//! Four vertices a step, as pack_avx2 takes them. The C++'s LD3 splits two
//! vertices' six doubles into x, y and z registers; Rust does not promise
//! Vec3's field order, so the same pairs are loaded from the fields by name.
//! Each lane is the scalar loop's own subtraction of the origin, rounded to
//! float by FCVTN, which rounds to nearest (the FPCR default) as `as f32`
//! does. ST4 then interleaves x, y, z and the colour words - stored as the
//! bits they are, never converted - into four whole GpuVertex records.

use crate::gpu_scene::GpuVertex;
use katana_geometry::Point3;
use katana_render::Rgba;
use std::arch::aarch64::*;

/// The contract is pack_kernels::pack_vertices'.
#[target_feature(enable = "neon")]
pub(super) unsafe fn pack_vertices(
    positions: &[Point3],
    colors: &[Rgba],
    origin: [f64; 3],
    out: &mut [GpuVertex],
) {
    let count = out.len();
    assert!(positions.len() >= count && colors.len() >= count);
    let whole = count / 4 * 4;
    // SAFETY: the caller has proved the processor runs NEON (Kernels). The
    // pair loads read local arrays; each colour load reads colors[i .. i + 4]
    // with i + 4 <= count, and each ST4 writes out[i .. i + 4]: four whole
    // #[repr(C)] GpuVertex records of four 4-byte words, sixteen floats.
    unsafe {
        let words = out.as_mut_ptr().cast::<f32>();
        let ox = vdupq_n_f64(origin[0]);
        let oy = vdupq_n_f64(origin[1]);
        let oz = vdupq_n_f64(origin[2]);
        for (step, p) in positions[..whole].as_chunks::<4>().0.iter().enumerate() {
            let i = 4 * step;
            // Vertices i, i + 1, then i + 2, i + 3: x, y and z apart.
            let ax = vld1q_f64([p[0].x, p[1].x].as_ptr());
            let ay = vld1q_f64([p[0].y, p[1].y].as_ptr());
            let az = vld1q_f64([p[0].z, p[1].z].as_ptr());
            let bx = vld1q_f64([p[2].x, p[3].x].as_ptr());
            let by = vld1q_f64([p[2].y, p[3].y].as_ptr());
            let bz = vld1q_f64([p[2].z, p[3].z].as_ptr());
            let vertex = float32x4x4_t(
                vcombine_f32(
                    vcvt_f32_f64(vsubq_f64(ax, ox)),
                    vcvt_f32_f64(vsubq_f64(bx, ox)),
                ),
                vcombine_f32(
                    vcvt_f32_f64(vsubq_f64(ay, oy)),
                    vcvt_f32_f64(vsubq_f64(by, oy)),
                ),
                vcombine_f32(
                    vcvt_f32_f64(vsubq_f64(az, oz)),
                    vcvt_f32_f64(vsubq_f64(bz, oz)),
                ),
                vreinterpretq_f32_u32(vld1q_u32(colors.as_ptr().add(i))),
            );
            vst4q_f32(words.add(4 * i), vertex);
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
