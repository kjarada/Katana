//! Safe moves between arrays and NEON registers, for every crate's kernels:
//! the AArch64 twin of x86_lanes.rs, whose notes hold here too.
//!
//! Every NEON intrinsic that takes no pointer is a safe call inside a
//! function with the `neon` feature (Rust 1.87); the loads and stores
//! (`vld1q_f64`, `vst1q_u32`, `vld2q_u8`, `vst4q_f32`, ...) take one and stay
//! unsafe. In, a register is a splat of lane 0 with the other lanes set
//! (`vsetq_lane`), or two 64-bit halves (`vcreate`, `vcombine`) for bytes,
//! from consecutive elements, which LLVM folds into the one LDR the pointer
//! form was. Out, each lane is read with `vgetq_lane`; copied into memory
//! whole, the lanes become the one STR.
//!
//! Lane k of a register is element k of the array, as LD1 and ST1 place them.
//! Bytes travel as little-endian 64-bit words: ACLE defines `vcreate` by bit
//! position (lane 0 is bits 0-7), `u64::from_le_bytes` puts the first byte
//! there, and every AArch64 target Katana builds is little-endian, where a
//! 64-bit lane's low byte is the register's byte lane 0.

use std::arch::aarch64::*;

/// A register of two doubles, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn f64x2(values: &[f64; 2]) -> float64x2_t {
    vsetq_lane_f64::<1>(values[1], vdupq_n_f64(values[0]))
}

/// The two doubles of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn f64x2_lanes(value: float64x2_t) -> [f64; 2] {
    [vgetq_lane_f64::<0>(value), vgetq_lane_f64::<1>(value)]
}

/// A 64-bit register of two floats, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn f32x2(values: &[f32; 2]) -> float32x2_t {
    vset_lane_f32::<1>(values[1], vdup_n_f32(values[0]))
}

/// A register of four floats, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn f32x4(values: &[f32; 4]) -> float32x4_t {
    let low = vsetq_lane_f32::<1>(values[1], vdupq_n_f32(values[0]));
    vsetq_lane_f32::<3>(values[3], vsetq_lane_f32::<2>(values[2], low))
}

/// The four floats of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn f32x4_lanes(value: float32x4_t) -> [f32; 4] {
    [
        vgetq_lane_f32::<0>(value),
        vgetq_lane_f32::<1>(value),
        vgetq_lane_f32::<2>(value),
        vgetq_lane_f32::<3>(value),
    ]
}

/// The two 32-bit integers of a 64-bit register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn i32x2_lanes(value: int32x2_t) -> [i32; 2] {
    [vget_lane_s32::<0>(value), vget_lane_s32::<1>(value)]
}

/// A register of four 32-bit integers, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn i32x4(values: &[i32; 4]) -> int32x4_t {
    let low = vsetq_lane_s32::<1>(values[1], vdupq_n_s32(values[0]));
    vsetq_lane_s32::<3>(values[3], vsetq_lane_s32::<2>(values[2], low))
}

/// A register of four unsigned 32-bit words (colours), lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn u32x4(values: &[u32; 4]) -> uint32x4_t {
    let low = vsetq_lane_u32::<1>(values[1], vdupq_n_u32(values[0]));
    vsetq_lane_u32::<3>(values[3], vsetq_lane_u32::<2>(values[2], low))
}

/// The four unsigned 32-bit words of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn u32x4_lanes(value: uint32x4_t) -> [u32; 4] {
    [
        vgetq_lane_u32::<0>(value),
        vgetq_lane_u32::<1>(value),
        vgetq_lane_u32::<2>(value),
        vgetq_lane_u32::<3>(value),
    ]
}

/// The two unsigned 64-bit integers of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn u64x2_lanes(value: uint64x2_t) -> [u64; 2] {
    [vgetq_lane_u64::<0>(value), vgetq_lane_u64::<1>(value)]
}

/// The two signed 64-bit integers of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn i64x2_lanes(value: int64x2_t) -> [i64; 2] {
    [vgetq_lane_s64::<0>(value), vgetq_lane_s64::<1>(value)]
}

/// A register of sixteen bytes, byte k in lane k.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn u8x16(bytes: &[u8; 16]) -> uint8x16_t {
    let low: [u8; 8] = std::array::from_fn(|k| bytes[k]);
    let high: [u8; 8] = std::array::from_fn(|k| bytes[8 + k]);
    vcombine_u8(
        vcreate_u8(u64::from_le_bytes(low)),
        vcreate_u8(u64::from_le_bytes(high)),
    )
}

/// The sixteen bytes of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `neon`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "neon")]
pub fn u8x16_lanes(value: uint8x16_t) -> [u8; 16] {
    let [low, high] = u64x2_lanes(vreinterpretq_u64_u8(value));
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&low.to_le_bytes());
    bytes[8..].copy_from_slice(&high.to_le_bytes());
    bytes
}
