//! AVX2 rasteriser kernels.
//!
//! BIT FOR BIT. Each lane repeats its scalar reference in rasterizer.rs
//! operation for operation, in the same order, with the same comparisons -
//! down to which operand a MINPS returns - and Rust never fuses a multiply and
//! an add into one rounding.

use crate::disjoint::TilePixels;
use crate::framebuffer::Rgba;
use crate::rasterizer::{ClipVertex, ScreenTriangle, ScreenVertex};
use katana_geometry::Point3;
use std::arch::x86_64::*;

// Mat4::multiply with w = 1 for one row (m0, m1, m2, m3) of the matrix:
// (((m0 x + m1 y) + m2 z) + m3 * 1).
macro_rules! clip_row {
    ($m0:expr, $m1:expr, $m2:expr, $m3:expr, $x:ident, $y:ident, $z:ident, $one:ident) => {
        _mm256_add_pd(
            _mm256_add_pd(
                _mm256_add_pd(_mm256_mul_pd($m0, $x), _mm256_mul_pd($m1, $y)),
                _mm256_mul_pd($m2, $z),
            ),
            _mm256_mul_pd($m3, $one),
        )
    };
}

// Bit p of the result set for each of the four vertices whose plane-p distance
// is not >= 0 - which a NaN is not - exactly the reference's !(d >= 0).
macro_rules! outside {
    ($distance:expr, $zero:ident) => {
        _mm256_movemask_pd(_mm256_cmp_pd::<_CMP_NGE_UQ>($distance, $zero))
    };
}

/// Stage 1 for `positions.len() / 4 * 4` vertices, four a step; the caller
/// hands the remainder to the reference. The contract is simd::transform_vertices'.
#[target_feature(enable = "avx2")]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
#[allow(
    clippy::needless_range_loop,
    reason = "Lane k of each stored register is vertex i + k: the index is the transpose, as in the C++ kernel."
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
    // SAFETY: the caller has proved the processor runs AVX2 (simd::Kernels),
    // and the one pointer written through is a local array's.
    unsafe {
        let mut m = [_mm256_setzero_pd(); 16];
        for (coefficient, &value) in m.iter_mut().zip(mvp) {
            *coefficient = _mm256_set1_pd(value);
        }
        let zero = _mm256_setzero_pd();
        let one = _mm256_set1_pd(1.0);
        let two = _mm256_set1_pd(2.0);
        let onef = _mm_set1_ps(1.0);
        let halff = _mm_set1_ps(0.5);
        let widthf = _mm_set1_ps(width);
        let heightf = _mm_set1_ps(height);

        for (step, p) in positions[..whole].as_chunks::<4>().0.iter().enumerate() {
            let i = 4 * step;
            let x = _mm256_setr_pd(p[0].x, p[1].x, p[2].x, p[3].x);
            let y = _mm256_setr_pd(p[0].y, p[1].y, p[2].y, p[3].y);
            let z = _mm256_setr_pd(p[0].z, p[1].z, p[2].z, p[3].z);

            let cx = clip_row!(m[0], m[1], m[2], m[3], x, y, z, one);
            let cy = clip_row!(m[4], m[5], m[6], m[7], x, y, z, one);
            let cz = clip_row!(m[8], m[9], m[10], m[11], x, y, z, one);
            let cw = clip_row!(m[12], m[13], m[14], m[15], x, y, z, one);

            // To float, rounding to nearest as `as f32` does.
            let fx = _mm256_cvtpd_ps(cx);
            let fy = _mm256_cvtpd_ps(cy);
            let fz = _mm256_cvtpd_ps(cz);
            let fw = _mm256_cvtpd_ps(cw);

            // The clip codes, judged in double from the FLOAT coordinates, as
            // the reference's plane_distance is: w - z, then 2w +- x and 2w +- y.
            let dx = _mm256_cvtps_pd(fx);
            let dy = _mm256_cvtps_pd(fy);
            let dz = _mm256_cvtps_pd(fz);
            let dw = _mm256_cvtps_pd(fw);
            let band = _mm256_mul_pd(two, dw);
            let planes = [
                outside!(_mm256_sub_pd(dw, dz), zero),
                outside!(_mm256_add_pd(band, dx), zero),
                outside!(_mm256_sub_pd(band, dx), zero),
                outside!(_mm256_add_pd(band, dy), zero),
                outside!(_mm256_sub_pd(band, dy), zero),
            ];
            for k in 0..4 {
                let mut code = 0;
                for (plane, &bits) in planes.iter().enumerate() {
                    code |= ((bits >> k) & 1) << plane;
                }
                codes[i + k] = code as u8;
            }

            // The projection, in float: 1/w, then ((x/w) * 0.5 + 0.5) * width,
            // (0.5 - (y/w) * 0.5) * height and z/w, each product as the
            // reference groups it.
            let inv_w = _mm_div_ps(onef, fw);
            let sx = _mm_mul_ps(
                _mm_add_ps(_mm_mul_ps(_mm_mul_ps(fx, inv_w), halff), halff),
                widthf,
            );
            let sy = _mm_mul_ps(
                _mm_sub_ps(halff, _mm_mul_ps(_mm_mul_ps(fy, inv_w), halff)),
                heightf,
            );
            let sz = _mm_mul_ps(fz, inv_w);

            // Back to one vertex at a time, with the colour word.
            let mut lanes = [[0.0f32; 4]; 8];
            for (out, value) in lanes.iter_mut().zip([fx, fy, fz, fw, sx, sy, sz, inv_w]) {
                _mm_storeu_ps(out.as_mut_ptr(), value);
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

/// Stage 3 for one screen triangle over some rows of one tile, eight pixels a
/// step. The contract is simd::shade_rows'.
#[target_feature(enable = "avx2")]
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
    let mut written_lanes = [0i32; 8];
    // SAFETY: the caller has proved the processor runs AVX2 (simd::Kernels);
    // the masked loads and stores reach only enabled lanes, which are the
    // span's pixels, inside the tile's row as asserted below.
    unsafe {
        let x0 = _mm256_set1_ps(t.x[0]);
        let x1 = _mm256_set1_ps(t.x[1]);
        let x2 = _mm256_set1_ps(t.x[2]);
        let z0 = _mm256_set1_ps(t.z[0]);
        let z1 = _mm256_set1_ps(t.z[1]);
        let z2 = _mm256_set1_ps(t.z[2]);
        let iw0 = _mm256_set1_ps(t.inv_w[0]);
        let iw1 = _mm256_set1_ps(t.inv_w[1]);
        let iw2 = _mm256_set1_ps(t.inv_w[2]);
        let bias = _mm256_set1_ps(t.depth_bias);
        let inv = _mm256_set1_ps(inv_area);
        let zero = _mm256_setzero_ps();
        let one = _mm256_set1_ps(1.0);
        let half = _mm256_set1_ps(0.5);
        let top = _mm256_set1_ps(255.0);
        let lanes = _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7);
        let byte_mask = _mm256_set1_epi32(0xFF);
        let fallback = _mm256_set1_epi32(t.color[0] as i32);

        // Each vertex's channels as floats, alpha first as the reference packs.
        let mut channel = [[zero; 3]; 4];
        for (k, channels) in channel.iter_mut().enumerate() {
            let shift = 24 - 8 * k as u32;
            for (value, &word) in channels.iter_mut().zip(&t.color) {
                *value = _mm256_set1_ps(((word >> shift) & 0xFF) as f32);
            }
        }

        // Written lanes are counted as -1 each, summed once at the end.
        let mut written = _mm256_setzero_si256();
        for r in 0..row_count {
            let first = spans[2 * r as usize];
            let last = spans[2 * r as usize + 1];
            if first > last {
                continue;
            }
            let row = first_row + r;
            let py = row as f32 + 0.5;
            let dy0 = _mm256_set1_ps(t.y[0] - py);
            let dy1 = _mm256_set1_ps(t.y[1] - py);
            let dy2 = _mm256_set1_ps(t.y[2] - py);
            let (color_row, depth_row) = pixels.row(row);
            // Every pixel of the span is in the tile's row, so every enabled
            // lane of a masked load or store below is too.
            assert!(first >= origin && ((last - origin) as usize) < depth_row.len());
            let end = _mm256_set1_epi32(last + 1);

            for x in (first..=last).step_by(8) {
                let xs = _mm256_add_epi32(_mm256_set1_epi32(x), lanes);
                let mut mask = _mm256_castsi256_ps(_mm256_cmpgt_epi32(end, xs));
                let px = _mm256_add_ps(_mm256_cvtepi32_ps(xs), half);
                let dx0 = _mm256_sub_ps(x0, px);
                let dx1 = _mm256_sub_ps(x1, px);
                let dx2 = _mm256_sub_ps(x2, px);
                let w0 = _mm256_mul_ps(
                    _mm256_sub_ps(_mm256_mul_ps(dx1, dy2), _mm256_mul_ps(dx2, dy1)),
                    inv,
                );
                let w1 = _mm256_mul_ps(
                    _mm256_sub_ps(_mm256_mul_ps(dx2, dy0), _mm256_mul_ps(dx0, dy2)),
                    inv,
                );
                let w2 = _mm256_sub_ps(_mm256_sub_ps(one, w0), w1);
                if !covered {
                    // Outside when any is < 0; NaN is not < 0, as in the reference.
                    let out = _mm256_or_ps(
                        _mm256_or_ps(
                            _mm256_cmp_ps::<_CMP_LT_OQ>(w0, zero),
                            _mm256_cmp_ps::<_CMP_LT_OQ>(w1, zero),
                        ),
                        _mm256_cmp_ps::<_CMP_LT_OQ>(w2, zero),
                    );
                    mask = _mm256_andnot_ps(out, mask);
                    if _mm256_movemask_ps(mask) == 0 {
                        continue;
                    }
                }
                let offset = (x - origin) as usize;
                let depth_at = depth_row.as_mut_ptr().add(offset);
                let color_at = color_row.as_mut_ptr().add(offset).cast::<i32>();
                let mask_bits = _mm256_castps_si256(mask);
                let before = _mm256_maskload_ps(depth_at, mask_bits);
                // min(d, 1.0) is (1 < d) ? 1 : d, which is MINPS(1, d).
                let d = _mm256_min_ps(
                    one,
                    _mm256_add_ps(
                        _mm256_add_ps(
                            _mm256_add_ps(_mm256_mul_ps(w0, z0), _mm256_mul_ps(w1, z1)),
                            _mm256_mul_ps(w2, z2),
                        ),
                        bias,
                    ),
                );
                let pass = _mm256_and_ps(mask, _mm256_cmp_ps::<_CMP_GT_OQ>(d, before));
                if _mm256_movemask_ps(pass) == 0 {
                    continue;
                }
                let inv_w = _mm256_add_ps(
                    _mm256_add_ps(_mm256_mul_ps(w0, iw0), _mm256_mul_ps(w1, iw1)),
                    _mm256_mul_ps(w2, iw2),
                );
                let perspective = _mm256_cmp_ps::<_CMP_GT_OQ>(inv_w, zero);
                let s = _mm256_div_ps(one, inv_w);
                let mut packed = _mm256_setzero_si256();
                for (k, channels) in channel.iter().enumerate() {
                    let value = _mm256_mul_ps(
                        _mm256_add_ps(
                            _mm256_add_ps(
                                _mm256_mul_ps(_mm256_mul_ps(w0, channels[0]), iw0),
                                _mm256_mul_ps(_mm256_mul_ps(w1, channels[1]), iw1),
                            ),
                            _mm256_mul_ps(_mm256_mul_ps(w2, channels[2]), iw2),
                        ),
                        s,
                    );
                    // clamp(v, 0, 255) is min(max(v, 0), 255): MAXPS(0, v) then
                    // MINPS(255, .), which return what those return for NaN.
                    let clamped = _mm256_min_ps(top, _mm256_max_ps(zero, value));
                    let byte = _mm256_and_si256(
                        _mm256_cvttps_epi32(_mm256_add_ps(clamped, half)),
                        byte_mask,
                    );
                    packed = _mm256_or_si256(
                        packed,
                        _mm256_sllv_epi32(byte, _mm256_set1_epi32(24 - 8 * k as i32)),
                    );
                }
                let shaded = _mm256_castps_si256(_mm256_blendv_ps(
                    _mm256_castsi256_ps(fallback),
                    _mm256_castsi256_ps(packed),
                    perspective,
                ));
                let pass_bits = _mm256_castps_si256(pass);
                _mm256_maskstore_epi32(color_at, pass_bits, shaded);
                if depth_write {
                    _mm256_maskstore_ps(depth_at, pass_bits, d);
                }
                written = _mm256_add_epi32(written, pass_bits);
            }
        }
        _mm256_storeu_si256(written_lanes.as_mut_ptr().cast(), written);
    }
    // -1 per written lane: negate the sum.
    let sum: i64 = written_lanes.iter().map(|&lane| i64::from(lane)).sum();
    (-sum) as usize
}
