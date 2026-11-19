//! Safe moves between arrays and x86 SIMD registers, for every crate's
//! kernels.
//!
//! Since Rust 1.87 an intrinsic that takes no pointer is a safe call inside a
//! function that has its target feature, and since 1.86 so is a call from one
//! such function to another with the same features or fewer. What stays
//! unsafe is the pointer loads and stores (`_mm256_loadu_pd`,
//! `_mm256_storeu_pd`, `_mm256_maskstore_ps`, ...), so the kernels read and
//! write their slices through these functions instead:
//!
//! * In, a register is built with a `set` intrinsic from the array's
//!   elements. They are consecutive in memory, and LLVM folds such a build
//!   into the one unaligned load (MOVUPD, MOVDQU) the pointer form was.
//! * Out, the register comes back as an array lane by lane, each lane a
//!   shuffle or extract that LLVM sees as an element of the vector: copied
//!   into memory whole, the lanes become the one unaligned store, and read one
//!   at a time they are the element moves a stored array read back was.
//!
//! No lane is converted, only moved, so the bits are the bits: a NaN keeps its
//! payload and a zero its sign.
//!
//! Rejected: `core::mem::transmute` between an array and a register, the one
//! conversion `core::arch` offers, which is `unsafe`; `std::simd`, not stable;
//! and a crate that wraps these (`safe_arch`, `bytemuck`), a new dependency
//! for a dozen lines.
//!
//! Each function is `#[inline]` so that it is inlined into the kernel that
//! calls it, in any crate: LLVM inlines a target-feature function only into
//! one with the same features or more, so the 128-bit ones ask for no more
//! than SSE2, the x86-64 baseline, and the 256-bit ones for AVX, which every
//! AVX2 kernel has.

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// A register of two doubles, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn f64x2(values: &[f64; 2]) -> __m128d {
    _mm_setr_pd(values[0], values[1])
}

/// The two doubles of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn f64x2_lanes(value: __m128d) -> [f64; 2] {
    [
        _mm_cvtsd_f64(value),
        _mm_cvtsd_f64(_mm_unpackhi_pd(value, value)),
    ]
}

/// A register of four floats, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn f32x4(values: &[f32; 4]) -> __m128 {
    _mm_setr_ps(values[0], values[1], values[2], values[3])
}

/// The four floats of a register, lane 0 first: lane k is moved to lane 0
/// (SHUFPS) and read there.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn f32x4_lanes(value: __m128) -> [f32; 4] {
    [
        _mm_cvtss_f32(value),
        _mm_cvtss_f32(_mm_shuffle_ps::<0x55>(value, value)),
        _mm_cvtss_f32(_mm_shuffle_ps::<0xAA>(value, value)),
        _mm_cvtss_f32(_mm_shuffle_ps::<0xFF>(value, value)),
    ]
}

/// A register of four 32-bit integers, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn i32x4(values: &[i32; 4]) -> __m128i {
    _mm_setr_epi32(values[0], values[1], values[2], values[3])
}

/// The four 32-bit integers of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn i32x4_lanes(value: __m128i) -> [i32; 4] {
    [
        _mm_cvtsi128_si32(value),
        _mm_cvtsi128_si32(_mm_shuffle_epi32::<0x55>(value)),
        _mm_cvtsi128_si32(_mm_shuffle_epi32::<0xAA>(value)),
        _mm_cvtsi128_si32(_mm_shuffle_epi32::<0xFF>(value)),
    ]
}

/// A register of four unsigned 32-bit words (colours), lane 0 first; `as`
/// between u32 and i32 keeps the bits.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn u32x4(values: &[u32; 4]) -> __m128i {
    i32x4(&values.map(|value| value as i32))
}

/// The four unsigned 32-bit words of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn u32x4_lanes(value: __m128i) -> [u32; 4] {
    i32x4_lanes(value).map(|lane| lane as u32)
}

/// A register of sixteen bytes, byte 0 in the lowest lane, as a load from
/// memory places them: x86 is little-endian, so the 32-bit lane k is bytes
/// 4k .. 4k + 4 read little-endian.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn u8x16(bytes: &[u8; 16]) -> __m128i {
    let word = |at: usize| {
        i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    _mm_setr_epi32(word(0), word(4), word(8), word(12))
}

/// The sixteen bytes of a register, in memory order.
///
/// # Safety
///
/// Called from a function that has `sse2`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "sse2")]
pub fn u8x16_lanes(value: __m128i) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(i32x4_lanes(value)) {
        *chunk = word.to_le_bytes();
    }
    bytes
}

/// A register of four doubles, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn f64x4(values: &[f64; 4]) -> __m256d {
    _mm256_setr_pd(values[0], values[1], values[2], values[3])
}

/// The four doubles of a register, lane 0 first: the low half, then the high.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn f64x4_lanes(value: __m256d) -> [f64; 4] {
    let [a, b] = f64x2_lanes(_mm256_castpd256_pd128(value));
    let [c, d] = f64x2_lanes(_mm256_extractf128_pd::<1>(value));
    [a, b, c, d]
}

/// A register of eight floats, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn f32x8(values: &[f32; 8]) -> __m256 {
    _mm256_setr_ps(
        values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
    )
}

/// The eight floats of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn f32x8_lanes(value: __m256) -> [f32; 8] {
    let [a, b, c, d] = f32x4_lanes(_mm256_castps256_ps128(value));
    let [e, f, g, h] = f32x4_lanes(_mm256_extractf128_ps::<1>(value));
    [a, b, c, d, e, f, g, h]
}

/// A register of eight 32-bit integers, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn i32x8(values: &[i32; 8]) -> __m256i {
    _mm256_setr_epi32(
        values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
    )
}

/// The eight 32-bit integers of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn i32x8_lanes(value: __m256i) -> [i32; 8] {
    let [a, b, c, d] = i32x4_lanes(_mm256_castsi256_si128(value));
    let [e, f, g, h] = i32x4_lanes(_mm256_extractf128_si256::<1>(value));
    [a, b, c, d, e, f, g, h]
}

/// A register of eight unsigned 32-bit words (colours), lane 0 first.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn u32x8(values: &[u32; 8]) -> __m256i {
    i32x8(&values.map(|value| value as i32))
}

/// The eight unsigned 32-bit words of a register, lane 0 first.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn u32x8_lanes(value: __m256i) -> [u32; 8] {
    i32x8_lanes(value).map(|lane| lane as u32)
}

/// A register of thirty-two bytes, byte 0 in the lowest lane, as u8x16 lays
/// them.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn u8x32(bytes: &[u8; 32]) -> __m256i {
    let word = |at: usize| {
        i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    _mm256_setr_epi32(
        word(0),
        word(4),
        word(8),
        word(12),
        word(16),
        word(20),
        word(24),
        word(28),
    )
}

/// The thirty-two bytes of a register, in memory order.
///
/// # Safety
///
/// Called from a function that has `avx`, it is safe; from any other, only once
/// the CPU is known to have it.
#[inline]
#[target_feature(enable = "avx")]
pub fn u8x32_lanes(value: __m256i) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(i32x8_lanes(value)) {
        *chunk = word.to_le_bytes();
    }
    bytes
}
