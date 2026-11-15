//! AVX2 kernels of the 3D scene build (SceneBuilder; the C++
//! src/katana_cad/simd/scene_avx2.cpp).
//!
//! BIT FOR BIT. Every lane does the operations of scene.rs's scalar code in
//! its order: Vec3::cross, the three-argument hypot as katana_math's Vec3
//! length computes it (libstdc++'s __hypot3: the largest magnitude, three
//! quotients, a square root), Light::intensity, render's shade and the ramp's
//! colour. Rust never fuses a multiply and an add, so each rounds twice as
//! the scalar code does. Conversions to a channel truncate (CVTTPD2DQ), as
//! `as u8` of a double in range does.
//!
//! Where the C++ loads a point's three doubles through a pointer, these read
//! the Point3's fields, whose layout Rust does not fix: a copy of a value is
//! the value, so the bits are the same.

#![allow(
    unsafe_code,
    reason = "Runtime-selected SIMD instructions are confined to the kernels, entered only with proof that the processor runs them."
)]
#![allow(
    unused_unsafe,
    reason = "Whether an intrinsic that takes no pointer is a safe call inside an AVX2 function depends on the toolchain's stdarch; every kernel body is one unsafe block either way."
)]

use super::scene_kernels::{
    KernelParams, PARAM_GROUND, PARAM_LIFT_DATUM, PARAM_LIFT_FACTOR, PARAM_LIGHT_ON,
    PARAM_RAMP_LOW, PARAM_RAMP_SPAN, PARAM_RAMP_TABLE, PARAM_SKY_MINUS_GROUND, PARAM_SUN,
    PARAM_SUN_X, PARAM_SUN_Y, PARAM_SUN_Z, RAMP_SEGMENTS,
};
use katana_geometry::Point3;
use std::arch::x86_64::*;

// _MM_SHUFFLE(3, 0, 2, 1): lanes y, z, x, w.
const YZX: i32 = 0b11_00_10_01;
// _MM_SHUFFLE(3, 1, 0, 2): lanes z, x, y, w.
const ZXY: i32 = 0b11_01_00_10;
// _MM_SHUFFLE(2, 2, 2, 2): lane z everywhere.
const ZZZZ: i32 = 0b10_10_10_10;

/// The hillshade's parameters, a register each.
struct Light {
    on: bool,
    sun_x: __m256d,
    sun_y: __m256d,
    sun_z: __m256d,
    ground: __m256d,
    sky_minus_ground: __m256d,
    sun: __m256d,
}

macro_rules! light_of {
    ($params:expr) => {{
        let params: &KernelParams = $params;
        Light {
            on: params[PARAM_LIGHT_ON] != 0.0,
            sun_x: _mm256_set1_pd(params[PARAM_SUN_X]),
            sun_y: _mm256_set1_pd(params[PARAM_SUN_Y]),
            sun_z: _mm256_set1_pd(params[PARAM_SUN_Z]),
            ground: _mm256_set1_pd(params[PARAM_GROUND]),
            sky_minus_ground: _mm256_set1_pd(params[PARAM_SKY_MINUS_GROUND]),
            sun: _mm256_set1_pd(params[PARAM_SUN]),
        }
    }};
}

// The sign bit cleared: |v|.
macro_rules! magnitude {
    ($v:expr) => {
        _mm256_andnot_pd(_mm256_set1_pd(-0.0), $v)
    };
}

// clamp(v, 0, 1) as the C++'s std::clamp: v < 0 ? 0 : (1 < v ? 1 : v).
// MINPD(1, v) is 1 < v ? 1 : v, a NaN in v included.
macro_rules! clamp01 {
    ($v:expr) => {{
        let v = $v;
        let zero = _mm256_setzero_pd();
        _mm256_blendv_pd(
            _mm256_min_pd(_mm256_set1_pd(1.0), v),
            zero,
            _mm256_cmp_pd::<_CMP_LT_OQ>(v, zero),
        )
    }};
}

// The three-argument hypot as katana_math's Vec3 length computes it
// (libstdc++'s __hypot3): a is the largest magnitude, picked by the same
// comparisons, and a zero a gives +0.
macro_rules! hypot3 {
    ($x:expr, $y:expr, $z:expr) => {{
        let x = magnitude!($x);
        let y = magnitude!($y);
        let z = magnitude!($z);
        let yz = _mm256_blendv_pd(y, z, _mm256_cmp_pd::<_CMP_LT_OQ>(y, z));
        let xz = _mm256_blendv_pd(x, z, _mm256_cmp_pd::<_CMP_LT_OQ>(x, z));
        let a = _mm256_blendv_pd(xz, yz, _mm256_cmp_pd::<_CMP_LT_OQ>(x, y));
        let qx = _mm256_div_pd(x, a);
        let qy = _mm256_div_pd(y, a);
        let qz = _mm256_div_pd(z, a);
        let root = _mm256_sqrt_pd(_mm256_add_pd(
            _mm256_add_pd(_mm256_mul_pd(qx, qx), _mm256_mul_pd(qy, qy)),
            _mm256_mul_pd(qz, qz),
        ));
        // if (a): nonzero, a NaN counting as true.
        _mm256_and_pd(
            _mm256_mul_pd(a, root),
            _mm256_cmp_pd::<_CMP_NEQ_UQ>(a, _mm256_setzero_pd()),
        )
    }};
}

// Vec3::cross of the first three lanes: y*o.z - z*o.y, z*o.x - x*o.z,
// x*o.y - y*o.x.
macro_rules! cross {
    ($u:expr, $v:expr) => {{
        let u = $u;
        let v = $v;
        _mm256_sub_pd(
            _mm256_mul_pd(
                _mm256_permute4x64_pd::<YZX>(u),
                _mm256_permute4x64_pd::<ZXY>(v),
            ),
            _mm256_mul_pd(
                _mm256_permute4x64_pd::<ZXY>(u),
                _mm256_permute4x64_pd::<YZX>(v),
            ),
        )
    }};
}

// (x, y, datum + (z - datum) * factor, w) of a point (x, y, z, w): the Lift.
macro_rules! lift_point {
    ($point:expr, $factor:expr, $datum:expr) => {{
        let point = $point;
        let datum = $datum;
        _mm256_blend_pd::<0b0100>(
            point,
            _mm256_add_pd(datum, _mm256_mul_pd(_mm256_sub_pd(point, datum), $factor)),
        )
    }};
}

// Light::intensity of the normal (x, y, z) / length, which is a unit normal.
macro_rules! intensity {
    ($light:expr, $x:expr, $y:expr, $z:expr, $length:expr) => {{
        let light: &Light = $light;
        if !light.on {
            _mm256_set1_pd(1.0)
        } else {
            let half = _mm256_set1_pd(0.5);
            let length = $length;
            let x = _mm256_div_pd($x, length);
            let y = _mm256_div_pd($y, length);
            let z = _mm256_div_pd($z, length);
            let hemisphere = _mm256_add_pd(
                light.ground,
                _mm256_mul_pd(
                    light.sky_minus_ground,
                    _mm256_add_pd(half, _mm256_mul_pd(half, z)),
                ),
            );
            let towards = _mm256_add_pd(
                _mm256_add_pd(_mm256_mul_pd(x, light.sun_x), _mm256_mul_pd(y, light.sun_y)),
                _mm256_mul_pd(z, light.sun_z),
            );
            // max(0.0, d) is 0 < d ? d : 0; MAXPD(d, 0) is d > 0 ? d : 0.
            clamp01!(_mm256_add_pd(
                hemisphere,
                _mm256_mul_pd(light.sun, _mm256_max_pd(towards, _mm256_setzero_pd())),
            ))
        }
    }};
}

// Four colours back from their channels as int32 lanes.
macro_rules! pack {
    ($a:expr, $r:expr, $g:expr, $b:expr) => {
        _mm_or_si128(
            _mm_or_si128(_mm_slli_epi32::<24>($a), _mm_slli_epi32::<16>($r)),
            _mm_or_si128(_mm_slli_epi32::<8>($g), $b),
        )
    };
}

// render's shade(colour, factor) of one channel in the lanes where `apply` is
// set; the others keep their channel (an exact integer, which truncates to
// itself).
macro_rules! shade_channel {
    ($channel:expr, $factor:expr, $apply:expr) => {{
        let value = _mm256_cvtepi32_pd($channel);
        let shaded = _mm256_add_pd(_mm256_mul_pd(value, $factor), _mm256_set1_pd(0.5));
        _mm256_cvttpd_epi32(_mm256_blendv_pd(value, shaded, $apply))
    }};
}

// render's shade(colour, factor) of four colours where `apply` is set.
macro_rules! shade {
    ($colours:expr, $factor:expr, $apply:expr) => {{
        let colours = $colours;
        let apply = $apply;
        let byte = _mm_set1_epi32(0xFF);
        let a = _mm_srli_epi32::<24>(colours);
        let r = _mm_and_si128(_mm_srli_epi32::<16>(colours), byte);
        let g = _mm_and_si128(_mm_srli_epi32::<8>(colours), byte);
        let b = _mm_and_si128(colours, byte);
        let f = clamp01!($factor);
        pack!(
            a,
            shade_channel!(r, f, apply),
            shade_channel!(g, f, apply),
            shade_channel!(b, f, apply)
        )
    }};
}

// The ramp's colour at (z - low) / span of four elevations, as
// surface_ramp_colour computes it.
macro_rules! ramp {
    ($z:expr, $params:expr) => {{
        let params: &KernelParams = $params;
        let t = _mm256_div_pd(
            _mm256_sub_pd($z, _mm256_set1_pd(params[PARAM_RAMP_LOW])),
            _mm256_set1_pd(params[PARAM_RAMP_SPAN]),
        );
        // t.is_finite() ? t : 0.0
        let t = _mm256_and_pd(
            t,
            _mm256_cmp_pd::<_CMP_LT_OQ>(magnitude!(t), _mm256_set1_pd(f64::INFINITY)),
        );
        let clamped = _mm256_mul_pd(clamp01!(t), _mm256_set1_pd(4.0));
        let lower = _mm_min_epi32(
            _mm256_cvttpd_epi32(clamped),
            _mm_set1_epi32(RAMP_SEGMENTS as i32 - 1),
        );
        let f = _mm256_sub_pd(clamped, _mm256_cvtepi32_pd(lower));
        // Each row of the table is four doubles, one register: picking entry
        // `lower` is a permute of its 32-bit halves (2 lower, 2 lower + 1),
        // which costs a fraction of a gather from memory.
        let twice = _mm256_slli_epi64::<1>(_mm256_cvtepu32_epi64(lower));
        let pick = _mm256_or_si256(
            twice,
            _mm256_slli_epi64::<32>(_mm256_add_epi64(twice, _mm256_set1_epi64x(1))),
        );
        let table = &params[PARAM_RAMP_TABLE..PARAM_RAMP_TABLE + 6 * RAMP_SEGMENTS];
        let mut channels = [_mm_setzero_si128(); 3];
        for (channel, value) in channels.iter_mut().enumerate() {
            let stops = _mm256_castpd_si256(_mm256_loadu_pd(
                table[2 * channel * RAMP_SEGMENTS..].as_ptr(),
            ));
            let steps = _mm256_castpd_si256(_mm256_loadu_pd(
                table[(2 * channel + 1) * RAMP_SEGMENTS..].as_ptr(),
            ));
            let a = _mm256_castsi256_pd(_mm256_permutevar8x32_epi32(stops, pick));
            let step = _mm256_castsi256_pd(_mm256_permutevar8x32_epi32(steps, pick));
            *value = _mm256_cvttpd_epi32(_mm256_add_pd(
                _mm256_add_pd(a, _mm256_mul_pd(step, f)),
                _mm256_set1_pd(0.5),
            ));
        }
        pack!(_mm_set1_epi32(0xFF), channels[0], channels[1], channels[2])
    }};
}

/// lifted[4 i ..] = x, y, datum + (z - datum) * factor, z of each point.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn lift(points: &[Point3], params: &KernelParams, lifted: &mut [f64]) {
    assert!(lifted.len() >= 4 * points.len());
    // SAFETY: the caller has proved the processor runs AVX2 (Kernels); each
    // store writes the four doubles of a vertex, inside `lifted` as asserted.
    unsafe {
        let factor = _mm256_set1_pd(params[PARAM_LIFT_FACTOR]);
        let datum = _mm256_set1_pd(params[PARAM_LIFT_DATUM]);
        for (i, p) in points.iter().enumerate() {
            // (x, y, z, z), then the third lifted.
            let point = _mm256_setr_pd(p.x, p.y, p.z, p.z);
            _mm256_storeu_pd(
                lifted[4 * i..4 * i + 4].as_mut_ptr(),
                lift_point!(point, factor, datum),
            );
        }
    }
}

/// For each triangle in order, the normal of its lifted corners, turned up,
/// added to its corners' sums in the order a, b, c.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn surface_normals(lifted: &[f64], triangles: &[[u32; 3]], normals: &mut [f64]) {
    // SAFETY: the caller has proved the processor runs AVX2 (Kernels); every
    // load and store goes through a four-double slice, bounds-checked.
    unsafe {
        let zero = _mm256_setzero_pd();
        let minus_one = _mm256_set1_pd(-1.0);
        for corner in triangles {
            let at = corner.map(|vertex| 4 * vertex as usize);
            let a = _mm256_loadu_pd(lifted[at[0]..at[0] + 4].as_ptr());
            let b = _mm256_loadu_pd(lifted[at[1]..at[1] + 4].as_ptr());
            let c = _mm256_loadu_pd(lifted[at[2]..at[2] + 4].as_ptr());
            let mut n = cross!(_mm256_sub_pd(b, a), _mm256_sub_pd(c, a));
            // if (n.z < 0.0) n = n * -1.0;
            let down = _mm256_cmp_pd::<_CMP_LT_OQ>(_mm256_permute4x64_pd::<ZZZZ>(n), zero);
            n = _mm256_blendv_pd(n, _mm256_mul_pd(n, minus_one), down);
            // One corner at a time, in order: a repeated corner adds twice.
            for &offset in &at {
                let sum = &mut normals[offset..offset + 4];
                _mm256_storeu_pd(
                    sum.as_mut_ptr(),
                    _mm256_add_pd(_mm256_loadu_pd(sum.as_ptr()), n),
                );
            }
        }
    }
}

/// Four whole vertices of surface_vertices: 16 doubles of `lifted` (and of
/// `normals`), four positions and four colours.
#[target_feature(enable = "avx2")]
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ helper's arguments, one a buffer."
)]
unsafe fn surface_vertices_four(
    lifted: &[f64],
    normals: Option<&[f64]>,
    params: &KernelParams,
    light: &Light,
    flat: u32,
    use_ramp: bool,
    positions: &mut [Point3],
    colours: &mut [u32],
) {
    assert!(lifted.len() >= 16 && positions.len() >= 4 && colours.len() >= 4);
    // SAFETY: the caller has proved the processor runs AVX2 (Kernels); the
    // one store writes four colours, inside `colours` as asserted.
    unsafe {
        for (j, position) in positions.iter_mut().take(4).enumerate() {
            *position = Point3::new(lifted[4 * j], lifted[4 * j + 1], lifted[4 * j + 2]);
        }
        let mut colour = if use_ramp {
            ramp!(
                _mm256_setr_pd(lifted[3], lifted[7], lifted[11], lifted[15]),
                params
            )
        } else {
            _mm_set1_epi32(flat as i32)
        };
        if let Some(n) = normals {
            assert!(n.len() >= 16);
            let x = _mm256_setr_pd(n[0], n[4], n[8], n[12]);
            let y = _mm256_setr_pd(n[1], n[5], n[9], n[13]);
            let z = _mm256_setr_pd(n[2], n[6], n[10], n[14]);
            let length = hypot3!(x, y, z);
            let lit = _mm256_cmp_pd::<_CMP_GT_OQ>(length, _mm256_setzero_pd());
            colour = shade!(colour, intensity!(light, x, y, z, length), lit);
        }
        _mm_storeu_si128(colours.as_mut_ptr().cast::<__m128i>(), colour);
    }
}

/// Each vertex of `positions`: its lifted position, and its colour - `flat`,
/// or the ramp at its true z when `use_ramp` - shaded by the hillshade of its
/// normal when `normals` is given.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn surface_vertices(
    lifted: &[f64],
    normals: Option<&[f64]>,
    params: &KernelParams,
    flat: u32,
    use_ramp: bool,
    positions: &mut [Point3],
    colours: &mut [u32],
) {
    let count = positions.len();
    // SAFETY: the caller has proved the processor runs AVX2 (Kernels); the
    // helper is handed slices of four vertices each.
    unsafe {
        let light = light_of!(params);
        let mut i = 0;
        while i + 4 <= count {
            surface_vertices_four(
                &lifted[4 * i..4 * i + 16],
                normals.map(|normals| &normals[4 * i..4 * i + 16]),
                params,
                &light,
                flat,
                use_ramp,
                &mut positions[i..i + 4],
                &mut colours[i..i + 4],
            );
            i += 4;
        }
        if i == count {
            return;
        }
        // The last one to three through the same code, padded: a zero normal
        // is left unshaded, and the padding's results are thrown away.
        let rest = count - i;
        let mut in_lifted = [0.0; 16];
        let mut in_normals = [0.0; 16];
        in_lifted[..4 * rest].copy_from_slice(&lifted[4 * i..4 * i + 4 * rest]);
        if let Some(normals) = normals {
            in_normals[..4 * rest].copy_from_slice(&normals[4 * i..4 * i + 4 * rest]);
        }
        let mut out_positions = [Point3::default(); 4];
        let mut out_colours = [0u32; 4];
        surface_vertices_four(
            &in_lifted,
            normals.map(|_| &in_normals[..]),
            params,
            &light,
            flat,
            use_ramp,
            &mut out_positions,
            &mut out_colours,
        );
        positions[i..].copy_from_slice(&out_positions[..rest]);
        colours[i..count].copy_from_slice(&out_colours[..rest]);
    }
}

/// Four faces of mesh_faces: face j's corners at corners[j], its number (for
/// its base colour) in numbers[j]; twelve positions and twelve colours.
#[target_feature(enable = "avx2")]
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ helper's arguments, one a buffer."
)]
unsafe fn mesh_faces_four(
    points: &[Point3],
    corners: &[[u32; 3]; 4],
    numbers: &[usize; 4],
    params: &KernelParams,
    light: &Light,
    face_colours: &[u32],
    flat: u32,
    positions: &mut [Point3],
    colours: &mut [u32],
) {
    assert!(positions.len() >= 12 && colours.len() >= 12);
    // SAFETY: the caller has proved the processor runs AVX2 (Kernels); every
    // store writes a local array.
    unsafe {
        let factor = _mm256_set1_pd(params[PARAM_LIFT_FACTOR]);
        let datum = _mm256_set1_pd(params[PARAM_LIFT_DATUM]);
        let mut normal = [[0.0; 4]; 4];
        for (j, face) in corners.iter().enumerate() {
            let [pa, pb, pc] = face.map(|vertex| points[vertex as usize]);
            let a = lift_point!(_mm256_setr_pd(pa.x, pa.y, pa.z, 0.0), factor, datum);
            let b = lift_point!(_mm256_setr_pd(pb.x, pb.y, pb.z, 0.0), factor, datum);
            let d = lift_point!(_mm256_setr_pd(pc.x, pc.y, pc.z, 0.0), factor, datum);
            _mm256_storeu_pd(
                normal[j].as_mut_ptr(),
                cross!(_mm256_sub_pd(b, a), _mm256_sub_pd(d, a)),
            );
            // Corners in the order c, b, a, as scene.rs adds them.
            let mut lanes = [[0.0; 4]; 3];
            _mm256_storeu_pd(lanes[0].as_mut_ptr(), d);
            _mm256_storeu_pd(lanes[1].as_mut_ptr(), b);
            _mm256_storeu_pd(lanes[2].as_mut_ptr(), a);
            for (k, corner) in lanes.iter().enumerate() {
                positions[3 * j + k] = Point3::new(corner[0], corner[1], corner[2]);
            }
        }
        let x = _mm256_setr_pd(normal[0][0], normal[1][0], normal[2][0], normal[3][0]);
        let y = _mm256_setr_pd(normal[0][1], normal[1][1], normal[2][1], normal[3][1]);
        let z = _mm256_setr_pd(normal[0][2], normal[1][2], normal[2][2], normal[3][2]);
        let area = hypot3!(x, y, z);
        let lit = _mm256_cmp_pd::<_CMP_GT_OQ>(area, _mm256_set1_pd(1.0e-12));
        let base: [u32; 4] = numbers.map(|number| {
            if number < face_colours.len() {
                face_colours[number]
            } else {
                flat
            }
        });
        let shaded = shade!(
            _mm_loadu_si128(base.as_ptr().cast::<__m128i>()),
            intensity!(light, x, y, z, area),
            lit
        );
        let mut out = [0u32; 4];
        _mm_storeu_si128(out.as_mut_ptr().cast::<__m128i>(), shaded);
        // Each face's colour on its three vertices.
        for (j, &colour) in out.iter().enumerate() {
            colours[3 * j..3 * j + 3].fill(colour);
        }
    }
}

/// Faces [first, first + count) of a mesh: the lifted corners, c then b then
/// a, into `positions`, and each face's colour, shaded by its own normal,
/// three times into `colours`.
#[target_feature(enable = "avx2")]
#[allow(
    clippy::too_many_arguments,
    reason = "The signature mirrors the C++ kernel entry, one argument a buffer."
)]
pub(super) unsafe fn mesh_faces(
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
    // SAFETY: the caller has proved the processor runs AVX2 (Kernels); the
    // helper is handed slices of four faces each.
    unsafe {
        let light = light_of!(params);
        let mut done = 0;
        while done + 4 <= count {
            let numbers: [usize; 4] = std::array::from_fn(|j| first + done + j);
            let corners = numbers.map(|number| faces[number]);
            mesh_faces_four(
                points,
                &corners,
                &numbers,
                params,
                &light,
                face_colours,
                flat,
                &mut positions[3 * done..3 * done + 12],
                &mut colours[3 * done..3 * done + 12],
            );
            done += 4;
        }
        if done == count {
            return;
        }
        // The last one to three, padded with copies of the last face.
        let rest = count - done;
        let numbers: [usize; 4] =
            std::array::from_fn(|j| first + done + if j < rest { j } else { rest - 1 });
        let corners = numbers.map(|number| faces[number]);
        let mut out_positions = [Point3::default(); 12];
        let mut out_colours = [0u32; 12];
        mesh_faces_four(
            points,
            &corners,
            &numbers,
            params,
            &light,
            face_colours,
            flat,
            &mut out_positions,
            &mut out_colours,
        );
        positions[3 * done..3 * done + 3 * rest].copy_from_slice(&out_positions[..3 * rest]);
        colours[3 * done..3 * done + 3 * rest].copy_from_slice(&out_colours[..3 * rest]);
    }
}

/// mix(lo, hi, e / 8) of every channel: lo + (hi - lo) * e/8 + 0.5 is exact in
/// a double (a multiple of 1/16 below 512), and never below 0.5, so
/// truncating it is flooring 8 lo + (hi - lo) e + 4 over 8 - which fits a
/// 16-bit lane.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn fade(base: &[u32], ink: &[u32], eighths: i32, out: &mut [u32]) {
    let count = out.len();
    assert!(base.len() >= count && ink.len() >= count);
    // SAFETY: the caller has proved the processor runs AVX2 (Kernels); each
    // load and store reaches eight colours, inside the slices as asserted.
    unsafe {
        let zero = _mm256_setzero_si256();
        let e = _mm256_set1_epi16(eighths as i16);
        let four = _mm256_set1_epi16(4);
        macro_rules! channel {
            ($lo:expr, $hi:expr) => {{
                let lo = $lo;
                let moved = _mm256_mullo_epi16(_mm256_sub_epi16($hi, lo), e);
                _mm256_srli_epi16::<3>(_mm256_add_epi16(
                    _mm256_add_epi16(_mm256_slli_epi16::<3>(lo), moved),
                    four,
                ))
            }};
        }
        let mut i = 0;
        while i + 8 <= count {
            let b = _mm256_loadu_si256(base[i..i + 8].as_ptr().cast::<__m256i>());
            let n = _mm256_loadu_si256(ink[i..i + 8].as_ptr().cast::<__m256i>());
            // Unpacking and packing both work within 128-bit halves, so the
            // bytes come back in their places.
            let low = channel!(_mm256_unpacklo_epi8(b, zero), _mm256_unpacklo_epi8(n, zero));
            let high = channel!(_mm256_unpackhi_epi8(b, zero), _mm256_unpackhi_epi8(n, zero));
            _mm256_storeu_si256(
                out[i..i + 8].as_mut_ptr().cast::<__m256i>(),
                _mm256_packus_epi16(low, high),
            );
            i += 8;
        }
        for ((result, &lo_word), &hi_word) in
            out[i..].iter_mut().zip(&base[i..count]).zip(&ink[i..count])
        {
            let mut mixed = 0u32;
            for shift in (0..32).step_by(8) {
                let lo = ((lo_word >> shift) & 0xFF) as i32;
                let hi = ((hi_word >> shift) & 0xFF) as i32;
                mixed |= (((8 * lo + (hi - lo) * eighths + 4) >> 3) as u32) << shift;
            }
            *result = mixed;
        }
    }
}
