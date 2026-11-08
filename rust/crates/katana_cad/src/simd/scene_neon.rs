//! NEON kernels of the 3D scene build (SceneBuilder), for 64-bit ARM (the
//! C++ src/katana_cad/simd/scene_neon.cpp).
//!
//! BIT FOR BIT, as scene_avx2.rs is: every lane does the operations of
//! scene.rs's scalar code in its order. Three things differ from x86 and are
//! handled here rather than left to chance:
//!
//! * FMLA is a baseline AArch64 instruction, so every product and sum is a
//!   separate FMUL and FADD, which Rust never fuses.
//! * The C++'s std::min, std::max and std::clamp are compares and BSL
//!   selects, because FMIN/FMAX answer otherwise for a NaN and for signed
//!   zeros.
//! * The three-argument hypot is libstdc++'s (__hypot3), which katana_math's
//!   Vec3 length ports; the C++ kernel's libc++ variants are for a C++
//!   build against that library, which the Rust scalar code is not.
//!
//! Conversions to a channel truncate (FCVTZS), as `as u8` of a double in
//! range does. The lanes are two points (or two faces), each coordinate in a
//! register of its own, gathered from the slices.

#![allow(
    unsafe_code,
    reason = "Runtime-selected SIMD instructions are confined to the kernels, entered only with proof that the processor runs them."
)]
#![allow(
    unused_unsafe,
    reason = "Whether an intrinsic that takes no pointer is a safe call inside a NEON function depends on the toolchain's stdarch; every kernel body is one unsafe block either way."
)]

use super::scene_kernels::{
    KernelParams, PARAM_GROUND, PARAM_LIFT_DATUM, PARAM_LIFT_FACTOR, PARAM_LIGHT_ON,
    PARAM_RAMP_LOW, PARAM_RAMP_SPAN, PARAM_RAMP_TABLE, PARAM_SKY_MINUS_GROUND, PARAM_SUN,
    PARAM_SUN_X, PARAM_SUN_Y, PARAM_SUN_Z, RAMP_SEGMENTS,
};
use katana_geometry::Point3;
use std::arch::aarch64::*;

/// The hillshade's parameters, a register each.
struct Light {
    on: bool,
    sun_x: float64x2_t,
    sun_y: float64x2_t,
    sun_z: float64x2_t,
    ground: float64x2_t,
    sky_minus_ground: float64x2_t,
    sun: float64x2_t,
}

macro_rules! light_of {
    ($params:expr) => {{
        let params: &KernelParams = $params;
        Light {
            on: params[PARAM_LIGHT_ON] != 0.0,
            sun_x: vdupq_n_f64(params[PARAM_SUN_X]),
            sun_y: vdupq_n_f64(params[PARAM_SUN_Y]),
            sun_z: vdupq_n_f64(params[PARAM_SUN_Z]),
            ground: vdupq_n_f64(params[PARAM_GROUND]),
            sky_minus_ground: vdupq_n_f64(params[PARAM_SKY_MINUS_GROUND]),
            sun: vdupq_n_f64(params[PARAM_SUN]),
        }
    }};
}

// Two doubles into a register.
macro_rules! pair {
    ($a:expr, $b:expr) => {
        vld1q_f64([$a, $b].as_ptr())
    };
}

// clamp(v, 0, 1) as the C++'s std::clamp: v < 0 ? 0 : (1 < v ? 1 : v), a NaN
// coming through.
macro_rules! clamp01 {
    ($v:expr) => {{
        let v = $v;
        let zero = vdupq_n_f64(0.0);
        let one = vdupq_n_f64(1.0);
        vbslq_f64(
            vcltq_f64(v, zero),
            zero,
            vbslq_f64(vcltq_f64(one, v), one, v),
        )
    }};
}

// The three-argument hypot as katana_math's Vec3 length computes it
// (libstdc++'s __hypot3): a is the largest magnitude, picked by the same
// comparisons, and a zero a gives +0.
macro_rules! hypot3 {
    ($x:expr, $y:expr, $z:expr) => {{
        let x = vabsq_f64($x);
        let y = vabsq_f64($y);
        let z = vabsq_f64($z);
        let yz = vbslq_f64(vcltq_f64(y, z), z, y);
        let xz = vbslq_f64(vcltq_f64(x, z), z, x);
        let a = vbslq_f64(vcltq_f64(x, y), yz, xz);
        let qx = vdivq_f64(x, a);
        let qy = vdivq_f64(y, a);
        let qz = vdivq_f64(z, a);
        let root = vsqrtq_f64(vaddq_f64(
            vaddq_f64(vmulq_f64(qx, qx), vmulq_f64(qy, qy)),
            vmulq_f64(qz, qz),
        ));
        // if (a): nonzero, a NaN counting as true. BIC clears the lanes where
        // a is zero, which leaves +0.
        vreinterpretq_f64_u64(vbicq_u64(
            vreinterpretq_u64_f64(vmulq_f64(a, root)),
            vceqq_f64(a, vdupq_n_f64(0.0)),
        ))
    }};
}

// Light::intensity of the normal (x, y, z) / length, which is a unit normal.
macro_rules! intensity {
    ($light:expr, $x:expr, $y:expr, $z:expr, $length:expr) => {{
        let light: &Light = $light;
        if !light.on {
            vdupq_n_f64(1.0)
        } else {
            let half = vdupq_n_f64(0.5);
            let zero = vdupq_n_f64(0.0);
            let length = $length;
            let x = vdivq_f64($x, length);
            let y = vdivq_f64($y, length);
            let z = vdivq_f64($z, length);
            let hemisphere = vaddq_f64(
                light.ground,
                vmulq_f64(light.sky_minus_ground, vaddq_f64(half, vmulq_f64(half, z))),
            );
            let towards = vaddq_f64(
                vaddq_f64(vmulq_f64(x, light.sun_x), vmulq_f64(y, light.sun_y)),
                vmulq_f64(z, light.sun_z),
            );
            // max(0.0, d) is 0 < d ? d : 0, so a NaN d gives 0.
            let lit = vbslq_f64(vcgtq_f64(towards, zero), towards, zero);
            clamp01!(vaddq_f64(hemisphere, vmulq_f64(light.sun, lit)))
        }
    }};
}

/// render's shade(colour, factor) of two colours in the lanes where `apply`
/// is set; the others keep their colour. The alpha byte is never touched.
#[target_feature(enable = "neon")]
unsafe fn shade(colours: &mut [u32; 2], factor: float64x2_t, apply: uint64x2_t) {
    // SAFETY: the caller has proved the processor runs NEON (Kernels); every
    // load and store reaches a local array.
    unsafe {
        let f = clamp01!(factor);
        let mut applied = [0u64; 2];
        vst1q_u64(applied.as_mut_ptr(), apply);
        let mut shaded = [colours[0] & 0xFF00_0000, colours[1] & 0xFF00_0000];
        for shift in [16u32, 8, 0] {
            let channel = pair!(
                f64::from((colours[0] >> shift) & 0xFF),
                f64::from((colours[1] >> shift) & 0xFF)
            );
            let mut value = [0i64; 2];
            vst1q_s64(
                value.as_mut_ptr(),
                vcvtq_s64_f64(vaddq_f64(vmulq_f64(channel, f), vdupq_n_f64(0.5))),
            );
            for (word, &channel) in shaded.iter_mut().zip(&value) {
                *word |= ((channel as u32) & 0xFF) << shift;
            }
        }
        for ((colour, &word), &applied) in colours.iter_mut().zip(&shaded).zip(&applied) {
            if applied != 0 {
                *colour = word;
            }
        }
    }
}

/// The ramp's colour at (z - low) / span of two elevations, as
/// surface_ramp_colour computes it.
#[target_feature(enable = "neon")]
unsafe fn ramp(z: float64x2_t, params: &KernelParams) -> [u32; 2] {
    // SAFETY: the caller has proved the processor runs NEON (Kernels); every
    // load and store reaches a local array.
    unsafe {
        let t = vdivq_f64(
            vsubq_f64(z, vdupq_n_f64(params[PARAM_RAMP_LOW])),
            vdupq_n_f64(params[PARAM_RAMP_SPAN]),
        );
        // t.is_finite() ? t : 0.0 - |t| < inf is false for inf and NaN alike.
        let finite = vcltq_f64(vabsq_f64(t), vdupq_n_f64(f64::INFINITY));
        let t = vreinterpretq_f64_u64(vandq_u64(vreinterpretq_u64_f64(t), finite));
        let clamped = vmulq_f64(clamp01!(t), vdupq_n_f64(4.0));
        // min(clamped as i32, stops - 2): clamped is in [0, 4].
        let mut whole = [0i64; 2];
        vst1q_s64(whole.as_mut_ptr(), vcvtq_s64_f64(clamped));
        let last = RAMP_SEGMENTS as i64 - 1;
        let lower = whole.map(|value| (if value < last { value } else { last }) as usize);
        let f = vsubq_f64(clamped, pair!(lower[0] as f64, lower[1] as f64));
        let table = &params[PARAM_RAMP_TABLE..PARAM_RAMP_TABLE + 6 * RAMP_SEGMENTS];
        let mut colours = [0xFF00_0000u32; 2];
        for channel in 0..3 {
            let stops = &table[2 * channel * RAMP_SEGMENTS..];
            let steps = &table[(2 * channel + 1) * RAMP_SEGMENTS..];
            let a = pair!(stops[lower[0]], stops[lower[1]]);
            let step = pair!(steps[lower[0]], steps[lower[1]]);
            let mut value = [0i64; 2];
            vst1q_s64(
                value.as_mut_ptr(),
                vcvtq_s64_f64(vaddq_f64(
                    vaddq_f64(a, vmulq_f64(step, f)),
                    vdupq_n_f64(0.5),
                )),
            );
            let shift = 16 - 8 * channel as u32;
            for (colour, &value) in colours.iter_mut().zip(&value) {
                *colour |= ((value as u32) & 0xFF) << shift;
            }
        }
        colours
    }
}

/// lifted[4 i ..] = x, y, datum + (z - datum) * factor, z of each point.
#[target_feature(enable = "neon")]
pub(super) unsafe fn lift(points: &[Point3], params: &KernelParams, lifted: &mut [f64]) {
    assert!(lifted.len() >= 4 * points.len());
    // SAFETY: the caller has proved the processor runs NEON (Kernels); every
    // store reaches a local array.
    unsafe {
        let factor_value = params[PARAM_LIFT_FACTOR];
        let datum_value = params[PARAM_LIFT_DATUM];
        let factor = vdupq_n_f64(factor_value);
        let datum = vdupq_n_f64(datum_value);
        let count = points.len();
        let mut i = 0;
        while i + 2 <= count {
            let (p, q) = (points[i], points[i + 1]);
            let z = pair!(p.z, q.z);
            let mut lifted_z = [0.0; 2];
            vst1q_f64(
                lifted_z.as_mut_ptr(),
                vaddq_f64(datum, vmulq_f64(vsubq_f64(z, datum), factor)),
            );
            lifted[4 * i..4 * i + 8].copy_from_slice(&[
                p.x,
                p.y,
                lifted_z[0],
                p.z,
                q.x,
                q.y,
                lifted_z[1],
                q.z,
            ]);
            i += 2;
        }
        for (k, p) in points.iter().enumerate().skip(i) {
            lifted[4 * k..4 * k + 4].copy_from_slice(&[
                p.x,
                p.y,
                datum_value + (p.z - datum_value) * factor_value,
                p.z,
            ]);
        }
    }
}

/// For each triangle in order, the normal of its lifted corners, turned up,
/// added to its corners' sums in the order a, b, c.
#[target_feature(enable = "neon")]
pub(super) unsafe fn surface_normals(lifted: &[f64], triangles: &[[u32; 3]], normals: &mut [f64]) {
    // SAFETY: the caller has proved the processor runs NEON (Kernels); every
    // load and store goes through a bounds-checked slice of the arrays.
    unsafe {
        // One triangle at a time, in order: each adds to its corners' running
        // sums, and a later triangle may share them. The normal's x and y are
        // one register - (y, z) * (o.z, o.x) - (z, x) * (o.y, o.z) - and z is
        // x * o.y - y * o.x on its own.
        for corner in triangles {
            let at = corner.map(|vertex| 4 * vertex as usize);
            let a = &lifted[at[0]..at[0] + 4];
            let b = &lifted[at[1]..at[1] + 4];
            let c = &lifted[at[2]..at[2] + 4];
            let uxy = vsubq_f64(vld1q_f64(b.as_ptr()), vld1q_f64(a.as_ptr()));
            let vxy = vsubq_f64(vld1q_f64(c.as_ptr()), vld1q_f64(a.as_ptr()));
            let uz = b[2] - a[2];
            let vz = c[2] - a[2];
            let uyz = vcombine_f64(vget_high_f64(uxy), vdup_n_f64(uz));
            let vyz = vcombine_f64(vget_high_f64(vxy), vdup_n_f64(vz));
            let uzx = vcombine_f64(vdup_n_f64(uz), vget_low_f64(uxy));
            let vzx = vcombine_f64(vdup_n_f64(vz), vget_low_f64(vxy));
            let mut nxy = vsubq_f64(vmulq_f64(uyz, vzx), vmulq_f64(uzx, vyz));
            let mut nz = vgetq_lane_f64::<0>(uxy) * vgetq_lane_f64::<1>(vxy)
                - vgetq_lane_f64::<1>(uxy) * vgetq_lane_f64::<0>(vxy);
            // if (n.z < 0.0) n = n * -1.0; a multiply, not a negation, as the
            // scalar code has it (the two differ in the sign of a NaN).
            if nz < 0.0 {
                nxy = vmulq_f64(nxy, vdupq_n_f64(-1.0));
                nz *= -1.0;
            }
            // One corner at a time, in order: a repeated corner adds twice.
            // The fourth double of each sum is never read, and is left alone.
            for &offset in &at {
                let sum = &mut normals[offset..offset + 4];
                vst1q_f64(sum.as_mut_ptr(), vaddq_f64(vld1q_f64(sum.as_ptr()), nxy));
                sum[2] += nz;
            }
        }
    }
}

/// Two whole vertices of surface_vertices: 8 doubles of `lifted` (and of
/// `normals`), two positions and two colours.
#[target_feature(enable = "neon")]
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ helper's arguments, one a buffer."
)]
unsafe fn surface_vertices_two(
    lifted: &[f64],
    normals: Option<&[f64]>,
    params: &KernelParams,
    light: &Light,
    flat: u32,
    use_ramp: bool,
    positions: &mut [Point3],
    colours: &mut [u32],
) {
    assert!(lifted.len() >= 8 && positions.len() >= 2 && colours.len() >= 2);
    // SAFETY: the caller has proved the processor runs NEON (Kernels); every
    // load reaches a local array.
    unsafe {
        // x, y, the lifted z and the true z of each vertex.
        for (j, position) in positions.iter_mut().take(2).enumerate() {
            *position = Point3::new(lifted[4 * j], lifted[4 * j + 1], lifted[4 * j + 2]);
        }
        let mut colour = [flat, flat];
        if use_ramp {
            colour = ramp(pair!(lifted[3], lifted[7]), params);
        }
        if let Some(n) = normals {
            assert!(n.len() >= 8);
            let x = pair!(n[0], n[4]);
            let y = pair!(n[1], n[5]);
            let z = pair!(n[2], n[6]);
            let length = hypot3!(x, y, z);
            let lit = vcgtq_f64(length, vdupq_n_f64(0.0));
            shade(&mut colour, intensity!(light, x, y, z, length), lit);
        }
        colours[0] = colour[0];
        colours[1] = colour[1];
    }
}

/// Each vertex of `positions`: its lifted position, and its colour - `flat`,
/// or the ramp at its true z when `use_ramp` - shaded by the hillshade of its
/// normal when `normals` is given.
#[target_feature(enable = "neon")]
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
    // SAFETY: the caller has proved the processor runs NEON (Kernels); the
    // helper is handed slices of two vertices each.
    unsafe {
        let light = light_of!(params);
        let mut i = 0;
        while i + 2 <= count {
            surface_vertices_two(
                &lifted[4 * i..4 * i + 8],
                normals.map(|normals| &normals[4 * i..4 * i + 8]),
                params,
                &light,
                flat,
                use_ramp,
                &mut positions[i..i + 2],
                &mut colours[i..i + 2],
            );
            i += 2;
        }
        if i == count {
            return;
        }
        // The last one through the same code, padded: a zero normal is left
        // unshaded, and the padding's results are thrown away.
        let mut in_lifted = [0.0; 8];
        let mut in_normals = [0.0; 8];
        in_lifted[..4].copy_from_slice(&lifted[4 * i..4 * i + 4]);
        if let Some(normals) = normals {
            in_normals[..4].copy_from_slice(&normals[4 * i..4 * i + 4]);
        }
        let mut out_positions = [Point3::default(); 2];
        let mut out_colours = [0u32; 2];
        surface_vertices_two(
            &in_lifted,
            normals.map(|_| &in_normals[..]),
            params,
            &light,
            flat,
            use_ramp,
            &mut out_positions,
            &mut out_colours,
        );
        positions[i] = out_positions[0];
        colours[i] = out_colours[0];
    }
}

/// Two faces of mesh_faces: face j's corners at corners[j], its number (for
/// its base colour) in numbers[j]; six positions and six colours.
#[target_feature(enable = "neon")]
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ helper's arguments, one a buffer."
)]
unsafe fn mesh_faces_two(
    points: &[Point3],
    corners: &[[u32; 3]; 2],
    numbers: &[usize; 2],
    params: &KernelParams,
    light: &Light,
    face_colours: &[u32],
    flat: u32,
    positions: &mut [Point3],
    colours: &mut [u32],
) {
    assert!(positions.len() >= 6 && colours.len() >= 6);
    // SAFETY: the caller has proved the processor runs NEON (Kernels); every
    // load and store reaches a local array.
    unsafe {
        let factor = vdupq_n_f64(params[PARAM_LIFT_FACTOR]);
        let datum = vdupq_n_f64(params[PARAM_LIFT_DATUM]);
        // Corner `c` of both faces, coordinate by coordinate, lifted.
        macro_rules! corner {
            ($c:expr) => {{
                let p0 = points[corners[0][$c] as usize];
                let p1 = points[corners[1][$c] as usize];
                (
                    pair!(p0.x, p1.x),
                    pair!(p0.y, p1.y),
                    vaddq_f64(
                        datum,
                        vmulq_f64(vsubq_f64(pair!(p0.z, p1.z), datum), factor),
                    ),
                )
            }};
        }
        let (ax, ay, az) = corner!(0);
        let (bx, by, bz) = corner!(1);
        let (cx, cy, cz) = corner!(2);

        // (b - a).cross(c - a): y*o.z - z*o.y, z*o.x - x*o.z, x*o.y - y*o.x.
        let ux = vsubq_f64(bx, ax);
        let uy = vsubq_f64(by, ay);
        let uz = vsubq_f64(bz, az);
        let vx = vsubq_f64(cx, ax);
        let vy = vsubq_f64(cy, ay);
        let vz = vsubq_f64(cz, az);
        let nx = vsubq_f64(vmulq_f64(uy, vz), vmulq_f64(uz, vy));
        let ny = vsubq_f64(vmulq_f64(uz, vx), vmulq_f64(ux, vz));
        let nz = vsubq_f64(vmulq_f64(ux, vy), vmulq_f64(uy, vx));

        // Corners in the order c, b, a, as scene.rs adds them.
        let mut lanes = [[0.0; 2]; 9];
        for (lane, value) in lanes.iter_mut().zip([cx, cy, cz, bx, by, bz, ax, ay, az]) {
            vst1q_f64(lane.as_mut_ptr(), value);
        }
        for face in 0..2 {
            for k in 0..3 {
                positions[3 * face + k] = Point3::new(
                    lanes[3 * k][face],
                    lanes[3 * k + 1][face],
                    lanes[3 * k + 2][face],
                );
            }
        }

        let area = hypot3!(nx, ny, nz);
        let lit = vcgtq_f64(area, vdupq_n_f64(1.0e-12));
        let mut colour: [u32; 2] = numbers.map(|number| {
            if number < face_colours.len() {
                face_colours[number]
            } else {
                flat
            }
        });
        shade(&mut colour, intensity!(light, nx, ny, nz, area), lit);
        // Each face's colour on its three vertices.
        colours[..3].fill(colour[0]);
        colours[3..6].fill(colour[1]);
    }
}

/// Faces [first, first + count) of a mesh: the lifted corners, c then b then
/// a, into `positions`, and each face's colour, shaded by its own normal,
/// three times into `colours`.
#[target_feature(enable = "neon")]
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
    // SAFETY: the caller has proved the processor runs NEON (Kernels); the
    // helper is handed slices of two faces each.
    unsafe {
        let light = light_of!(params);
        let mut done = 0;
        while done + 2 <= count {
            let numbers = [first + done, first + done + 1];
            let corners = numbers.map(|number| faces[number]);
            mesh_faces_two(
                points,
                &corners,
                &numbers,
                params,
                &light,
                face_colours,
                flat,
                &mut positions[3 * done..3 * done + 6],
                &mut colours[3 * done..3 * done + 6],
            );
            done += 2;
        }
        if done == count {
            return;
        }
        // The last one, padded with a copy of itself.
        let numbers = [first + done, first + done];
        let corners = numbers.map(|number| faces[number]);
        let mut out_positions = [Point3::default(); 6];
        let mut out_colours = [0u32; 6];
        mesh_faces_two(
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
        positions[3 * done..3 * done + 3].copy_from_slice(&out_positions[..3]);
        colours[3 * done..3 * done + 3].copy_from_slice(&out_colours[..3]);
    }
}

/// mix(lo, hi, e / 8) of every channel, as scene_avx2.rs has it: flooring
/// 8 lo + (hi - lo) e + 4 over 8, which fits a 16-bit lane (and wraps
/// harmlessly through a negative (hi - lo) e on the way).
#[target_feature(enable = "neon")]
pub(super) unsafe fn fade(base: &[u32], ink: &[u32], eighths: i32, out: &mut [u32]) {
    let count = out.len();
    assert!(base.len() >= count && ink.len() >= count);
    // SAFETY: the caller has proved the processor runs NEON (Kernels); each
    // load and store reaches four colours, inside the slices as asserted.
    unsafe {
        let e = vdupq_n_u16(eighths as u16);
        let four = vdupq_n_u16(4);
        macro_rules! channel {
            ($lo8:expr, $hi8:expr) => {{
                let lo = vmovl_u8($lo8);
                let moved = vmulq_u16(vsubq_u16(vmovl_u8($hi8), lo), e);
                vmovn_u16(vshrq_n_u16::<3>(vaddq_u16(
                    vaddq_u16(vshlq_n_u16::<3>(lo), moved),
                    four,
                )))
            }};
        }
        let mut i = 0;
        // Four colours, sixteen channel bytes, a step: each byte is one
        // channel, and every channel is mixed alike.
        while i + 4 <= count {
            let b = vreinterpretq_u8_u32(vld1q_u32(base[i..i + 4].as_ptr()));
            let n = vreinterpretq_u8_u32(vld1q_u32(ink[i..i + 4].as_ptr()));
            let mixed = vcombine_u8(
                channel!(vget_low_u8(b), vget_low_u8(n)),
                channel!(vget_high_u8(b), vget_high_u8(n)),
            );
            vst1q_u32(out[i..i + 4].as_mut_ptr(), vreinterpretq_u32_u8(mixed));
            i += 4;
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
