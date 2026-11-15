//! NEON rasteriser kernels, for 64-bit ARM.
//!
//! BIT FOR BIT, as avx2.rs is: each lane repeats its scalar reference in
//! rasterizer.rs operation for operation, in the same order, with the same
//! comparisons. Three things differ from x86 and are handled here rather than
//! left to chance:
//!
//! * FMLA is a baseline instruction, so every product and sum is a separate
//!   FMUL and FADD, which Rust never fuses.
//! * FMIN/FMAX/FMINNM/FMAXNM treat NaN and signed zero otherwise than the
//!   C++'s std::min, std::max and std::clamp (and crate::scalar's), so those
//!   are a compare and a BSL select that return what the scalar functions
//!   return.
//! * There is no masked load or store, so a block of four pixels that runs
//!   past the end of its span is read and written one lane at a time.

use crate::disjoint::TilePixels;
use crate::framebuffer::Rgba;
use crate::rasterizer::{ClipVertex, ScreenTriangle, ScreenVertex};
use katana_geometry::Point3;
use std::arch::aarch64::*;

// Bit `bit` in each lane whose plane distance is not >= 0 - which a NaN is
// not - exactly the reference's !(d >= 0). BIC is `bit and not inside`.
macro_rules! outside {
    ($distance:expr, $bit:expr, $zero:ident) => {
        vbicq_u64(vdupq_n_u64($bit), vcgeq_f64($distance, $zero))
    };
}

// min(v, hi) is (hi < v) ? hi : v; max(v, lo) is (v < lo) ? lo : v. A NaN v
// comes through both, as it does through the scalar functions.
macro_rules! min_of {
    ($v:expr, $hi:expr) => {{
        let v = $v;
        let hi = $hi;
        vbslq_f32(vcltq_f32(hi, v), hi, v)
    }};
}
macro_rules! max_of {
    ($v:expr, $lo:expr) => {{
        let v = $v;
        let lo = $lo;
        vbslq_f32(vcltq_f32(v, lo), lo, v)
    }};
}

/// Stage 1 for `positions.len() / 4 * 4` vertices, four a step as the AVX2
/// kernel takes them; the caller hands the remainder to the reference. The
/// contract is simd::transform_vertices'.
#[target_feature(enable = "neon")]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(super) unsafe fn transform_vertices(
    mvp: &[f64; 16],
    positions: &[Point3],
    colors: &[Rgba],
    width: f32,
    height: f32,
    clip: &mut [ClipVertex],
    codes: &mut [u8],
    screen: &mut [ScreenVertex],
) {
    let whole = positions.len() / 4 * 4;
    assert!(clip.len() >= whole && codes.len() >= whole && screen.len() >= whole);
    // SAFETY: the caller has proved the processor runs NEON (simd::Kernels),
    // and the pointers read and written through are local arrays'.
    unsafe {
        let mut m = [vdupq_n_f64(0.0); 16];
        for (coefficient, &value) in m.iter_mut().zip(mvp) {
            *coefficient = vdupq_n_f64(value);
        }
        let zero = vdupq_n_f64(0.0);
        let one = vdupq_n_f64(1.0);
        let two = vdupq_n_f64(2.0);
        let onef = vdupq_n_f32(1.0);
        let halff = vdupq_n_f32(0.5);
        let widthf = vdupq_n_f32(width);
        let heightf = vdupq_n_f32(height);

        for (step, p) in positions[..whole].chunks_exact(4).enumerate() {
            let i = 4 * step;
            // Two vertices at a time in double: their clip coordinates, x, y,
            // z, w as float pairs, and their clip codes.
            let mut narrow = [[vdup_n_f32(0.0); 4]; 2];
            for (pair, out) in narrow.iter_mut().enumerate() {
                let a = &p[2 * pair];
                let b = &p[2 * pair + 1];
                let x = vld1q_f64([a.x, b.x].as_ptr());
                let y = vld1q_f64([a.y, b.y].as_ptr());
                let z = vld1q_f64([a.z, b.z].as_ptr());
                for (axis, value) in out.iter_mut().enumerate() {
                    let row = 4 * axis;
                    // Mat4::multiply with w = 1: (((m0 x + m1 y) + m2 z) + m3 * 1).
                    let wide = vaddq_f64(
                        vaddq_f64(
                            vaddq_f64(vmulq_f64(m[row], x), vmulq_f64(m[row + 1], y)),
                            vmulq_f64(m[row + 2], z),
                        ),
                        vmulq_f64(m[row + 3], one),
                    );
                    // To float, rounding to nearest (the FPCR default) as
                    // `as f32` does.
                    *value = vcvt_f32_f64(wide);
                }

                // The clip codes, judged in double from the FLOAT coordinates
                // as the reference's plane_distance is: w - z, then 2w +- x and
                // 2w +- y.
                let dx = vcvt_f64_f32(out[0]);
                let dy = vcvt_f64_f32(out[1]);
                let dz = vcvt_f64_f32(out[2]);
                let dw = vcvt_f64_f32(out[3]);
                let band = vmulq_f64(two, dw);
                let code = vorrq_u64(
                    vorrq_u64(
                        vorrq_u64(
                            outside!(vsubq_f64(dw, dz), 1, zero),
                            outside!(vaddq_f64(band, dx), 2, zero),
                        ),
                        vorrq_u64(
                            outside!(vsubq_f64(band, dx), 4, zero),
                            outside!(vaddq_f64(band, dy), 8, zero),
                        ),
                    ),
                    outside!(vsubq_f64(band, dy), 16, zero),
                );
                codes[i + 2 * pair] = vgetq_lane_u64::<0>(code) as u8;
                codes[i + 2 * pair + 1] = vgetq_lane_u64::<1>(code) as u8;
            }

            let fx = vcombine_f32(narrow[0][0], narrow[1][0]);
            let fy = vcombine_f32(narrow[0][1], narrow[1][1]);
            let fz = vcombine_f32(narrow[0][2], narrow[1][2]);
            let fw = vcombine_f32(narrow[0][3], narrow[1][3]);

            // The projection, in float: 1/w, then ((x/w) * 0.5 + 0.5) * width,
            // (0.5 - (y/w) * 0.5) * height and z/w, each product as the
            // reference groups it. FDIV is correctly rounded, as the scalar
            // divide is.
            let inv_w = vdivq_f32(onef, fw);
            let sx = vmulq_f32(
                vaddq_f32(vmulq_f32(vmulq_f32(fx, inv_w), halff), halff),
                widthf,
            );
            let sy = vmulq_f32(
                vsubq_f32(halff, vmulq_f32(vmulq_f32(fy, inv_w), halff)),
                heightf,
            );
            let sz = vmulq_f32(fz, inv_w);

            // Back to one vertex at a time, with the colour word.
            let mut lanes = [[0.0f32; 4]; 8];
            for (out, value) in lanes.iter_mut().zip([fx, fy, fz, fw, sx, sy, sz, inv_w]) {
                vst1q_f32(out.as_mut_ptr(), value);
            }
            for k in 0..4 {
                let vertex = i + k;
                let word = colors.get(vertex).copied().unwrap_or(0xFFFF_FFFF);
                clip[vertex] = ClipVertex {
                    x: lanes[0][k],
                    y: lanes[1][k],
                    z: lanes[2][k],
                    w: lanes[3][k],
                    color: word,
                };
                screen[vertex] = ScreenVertex {
                    x: lanes[4][k],
                    y: lanes[5][k],
                    z: lanes[6][k],
                    inv_w: lanes[7][k],
                    color: word,
                };
            }
        }
    }
}

/// Stage 3 for one screen triangle over some rows of one tile, four pixels a
/// step. The contract is simd::shade_rows'.
#[target_feature(enable = "neon")]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry."
)]
pub(super) unsafe fn shade_rows(
    t: &ScreenTriangle,
    inv_area: f32,
    spans: &[i32],
    first_row: i32,
    row_count: i32,
    covered: bool,
    depth_write: bool,
    pixels: &mut TilePixels<'_>,
) -> usize {
    let origin = pixels.rect().x0;
    // SAFETY: the caller has proved the processor runs NEON (simd::Kernels);
    // a full block of four is read and written only where the span holds all
    // four pixels, inside the tile's row as asserted below, and the other
    // pointers are local arrays'.
    let total = unsafe {
        let x0 = vdupq_n_f32(t.x[0]);
        let x1 = vdupq_n_f32(t.x[1]);
        let x2 = vdupq_n_f32(t.x[2]);
        let z0 = vdupq_n_f32(t.z[0]);
        let z1 = vdupq_n_f32(t.z[1]);
        let z2 = vdupq_n_f32(t.z[2]);
        let iw0 = vdupq_n_f32(t.inv_w[0]);
        let iw1 = vdupq_n_f32(t.inv_w[1]);
        let iw2 = vdupq_n_f32(t.inv_w[2]);
        let bias = vdupq_n_f32(t.depth_bias);
        let inv = vdupq_n_f32(inv_area);
        let zero = vdupq_n_f32(0.0);
        let one = vdupq_n_f32(1.0);
        let half = vdupq_n_f32(0.5);
        let top = vdupq_n_f32(255.0);
        let lane_offsets: [i32; 4] = [0, 1, 2, 3];
        let lanes = vld1q_s32(lane_offsets.as_ptr());
        let byte_mask = vdupq_n_u32(0xFF);
        let fallback = vdupq_n_u32(t.color[0]);

        // Each vertex's channels as floats, alpha first as the reference packs.
        let mut channel = [[zero; 3]; 4];
        for (k, channels) in channel.iter_mut().enumerate() {
            let shift = 24 - 8 * k as u32;
            for (value, &word) in channels.iter_mut().zip(&t.color) {
                *value = vdupq_n_f32(((word >> shift) & 0xFF) as f32);
            }
        }

        // Written lanes are all ones, -1 each: subtracted, they count up.
        let mut written = vdupq_n_u32(0);
        for r in 0..row_count {
            let first = spans[2 * r as usize];
            let last = spans[2 * r as usize + 1];
            if first > last {
                continue;
            }
            let row = first_row + r;
            let py = row as f32 + 0.5;
            let dy0 = vdupq_n_f32(t.y[0] - py);
            let dy1 = vdupq_n_f32(t.y[1] - py);
            let dy2 = vdupq_n_f32(t.y[2] - py);
            let (color_row, depth_row) = pixels.row(row);
            // Every pixel of the span is in the tile's row; the full blocks
            // below never pass `last`.
            assert!(first >= origin && ((last - origin) as usize) < depth_row.len());
            let end = vdupq_n_s32(last + 1);

            for x in (first..=last).step_by(4) {
                let xs = vaddq_s32(vdupq_n_s32(x), lanes);
                let mut mask = vcgtq_s32(end, xs);
                // SCVTF rounds to nearest, as `as f32` on an integer does.
                let px = vaddq_f32(vcvtq_f32_s32(xs), half);
                let dx0 = vsubq_f32(x0, px);
                let dx1 = vsubq_f32(x1, px);
                let dx2 = vsubq_f32(x2, px);
                let w0 = vmulq_f32(vsubq_f32(vmulq_f32(dx1, dy2), vmulq_f32(dx2, dy1)), inv);
                let w1 = vmulq_f32(vsubq_f32(vmulq_f32(dx2, dy0), vmulq_f32(dx0, dy2)), inv);
                let w2 = vsubq_f32(vsubq_f32(one, w0), w1);
                if !covered {
                    // Outside when any is < 0; NaN is not < 0, as in the reference.
                    let out = vorrq_u32(
                        vorrq_u32(vcltq_f32(w0, zero), vcltq_f32(w1, zero)),
                        vcltq_f32(w2, zero),
                    );
                    mask = vbicq_u32(mask, out);
                    if vmaxvq_u32(mask) == 0 {
                        continue;
                    }
                }
                // The pixels of the block that belong to the span: all four,
                // or the first few of the span's last block, which must not be
                // read or written past `last`.
                let in_span = if last - x + 1 < 4 {
                    (last - x + 1) as usize
                } else {
                    4
                };
                let offset = (x - origin) as usize;
                let before = if in_span == 4 {
                    vld1q_f32(depth_row.as_ptr().add(offset))
                } else {
                    let mut partial = [0.0f32; 4];
                    partial[..in_span].copy_from_slice(&depth_row[offset..offset + in_span]);
                    vld1q_f32(partial.as_ptr())
                };
                let d = min_of!(
                    vaddq_f32(
                        vaddq_f32(
                            vaddq_f32(vmulq_f32(w0, z0), vmulq_f32(w1, z1)),
                            vmulq_f32(w2, z2),
                        ),
                        bias,
                    ),
                    one
                );
                let pass = vandq_u32(mask, vcgtq_f32(d, before));
                if vmaxvq_u32(pass) == 0 {
                    continue;
                }
                let inv_w = vaddq_f32(
                    vaddq_f32(vmulq_f32(w0, iw0), vmulq_f32(w1, iw1)),
                    vmulq_f32(w2, iw2),
                );
                let perspective = vcgtq_f32(inv_w, zero);
                let s = vdivq_f32(one, inv_w);
                let mut packed = vdupq_n_u32(0);
                for (k, channels) in channel.iter().enumerate() {
                    let value = vmulq_f32(
                        vaddq_f32(
                            vaddq_f32(
                                vmulq_f32(vmulq_f32(w0, channels[0]), iw0),
                                vmulq_f32(vmulq_f32(w1, channels[1]), iw1),
                            ),
                            vmulq_f32(vmulq_f32(w2, channels[2]), iw2),
                        ),
                        s,
                    );
                    // clamp(v, 0, 255), then +0.5 truncated. FCVTZS truncates
                    // towards zero and gives 0 for a NaN, which the AVX2
                    // kernel's 0x80000000 & 0xFF and the scalar `as u8` also
                    // give.
                    let clamped = min_of!(max_of!(value, zero), top);
                    let byte = vandq_u32(
                        vreinterpretq_u32_s32(vcvtq_s32_f32(vaddq_f32(clamped, half))),
                        byte_mask,
                    );
                    packed = vorrq_u32(packed, vshlq_u32(byte, vdupq_n_s32(24 - 8 * k as i32)));
                }
                let shaded = vbslq_u32(perspective, packed, fallback);
                if in_span == 4 {
                    // Lanes that did not pass get back what was there: all four
                    // pixels are this span's, which only this call writes.
                    let color_at = color_row.as_mut_ptr().add(offset);
                    vst1q_u32(color_at, vbslq_u32(pass, shaded, vld1q_u32(color_at)));
                    if depth_write {
                        let depth_at = depth_row.as_mut_ptr().add(offset);
                        vst1q_f32(depth_at, vbslq_f32(pass, d, before));
                    }
                } else {
                    let mut pass_lanes = [0u32; 4];
                    let mut shaded_lanes = [0u32; 4];
                    let mut depth_lanes = [0.0f32; 4];
                    vst1q_u32(pass_lanes.as_mut_ptr(), pass);
                    vst1q_u32(shaded_lanes.as_mut_ptr(), shaded);
                    vst1q_f32(depth_lanes.as_mut_ptr(), d);
                    for k in 0..in_span {
                        if pass_lanes[k] != 0 {
                            color_row[offset + k] = shaded_lanes[k];
                            if depth_write {
                                depth_row[offset + k] = depth_lanes[k];
                            }
                        }
                    }
                }
                written = vsubq_u32(written, pass);
            }
        }
        vaddvq_u32(written)
    };
    total as usize
}
