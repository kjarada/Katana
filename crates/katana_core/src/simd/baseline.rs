//! The facade's compare and select on the baseline register: SSE2 on x86-64
//! and NEON on AArch64, each part of every target of its architecture, so
//! neither needs detecting. Values travel as the register's sixteen bytes
//! (Sealed's byte forms, x86_lanes and neon_lanes), so the kernels are safe
//! functions; the one unsafe step is entering them, in `compare` and `select`
//! below. Elsewhere the scalar comparison and moves stand in.

/// What the sixteen bytes being compared hold.
#[derive(Clone, Copy)]
#[cfg_attr(
    not(any(
        all(target_arch = "x86_64", target_feature = "sse2"),
        all(target_arch = "aarch64", target_feature = "neon")
    )),
    allow(
        dead_code,
        reason = "Without a baseline register the scalar comparison never reads the kind."
    )
)]
enum Lanes {
    /// Integers of `width` bytes, signed or not.
    Integer { width: usize, signed: bool },
    F64,
    F32,
}

pub(super) fn compare_f64(left: &[f64; 2], right: &[f64; 2]) -> [bool; 2] {
    compare(left, right, Lanes::F64)
}

pub(super) fn compare_f32(left: &[f32; 4], right: &[f32; 4]) -> [bool; 4] {
    compare(left, right, Lanes::F32)
}

/// Bitwise selection, which keeps a NaN's payload and a zero's sign.
pub(super) fn select_f64(
    mask: &[bool; 2],
    when_true: &[f64; 2],
    when_false: &[f64; 2],
) -> [f64; 2] {
    select(mask, when_true, when_false)
}

/// Bitwise selection, which keeps a NaN's payload and a zero's sign.
pub(super) fn select_f32(
    mask: &[bool; 4],
    when_true: &[f32; 4],
    when_false: &[f32; 4],
) -> [f32; 4] {
    select(mask, when_true, when_false)
}

pub(super) fn compare_integer<T: super::Element, const COUNT: usize>(
    left: &[T; COUNT],
    right: &[T; COUNT],
    signed: bool,
) -> [bool; COUNT] {
    let width = std::mem::size_of::<T>();
    compare(left, right, Lanes::Integer { width, signed })
}

pub(super) fn select_integer<T: super::Element, const COUNT: usize>(
    mask: &[bool; COUNT],
    when_true: &[T; COUNT],
    when_false: &[T; COUNT],
) -> [T; COUNT] {
    select(mask, when_true, when_false)
}

/// The register's bytes: each lane's value in memory order, lane 0 first.
#[cfg_attr(
    not(any(
        all(target_arch = "x86_64", target_feature = "sse2"),
        all(target_arch = "aarch64", target_feature = "neon")
    )),
    allow(dead_code, reason = "Only the baseline kernels take bytes.")
)]
fn bytes_of<T: super::Element, const COUNT: usize>(values: &[T; COUNT]) -> [u8; 16] {
    let width = std::mem::size_of::<T>();
    assert_eq!(width * COUNT, super::REGISTER_BYTES);
    let mut bytes = [0u8; 16];
    for (lane, &value) in bytes.chunks_exact_mut(width).zip(values) {
        <T as super::sealed::Sealed>::write_bytes(value, lane);
    }
    bytes
}

/// Each lane `left < right`, a NaN never less, as the scalar `<` has it.
#[allow(
    unsafe_code,
    reason = "Entering the baseline kernel, whose instructions every target of the architecture has."
)]
fn compare<T: super::Element, const COUNT: usize>(
    left: &[T; COUNT],
    right: &[T; COUNT],
    lanes: Lanes,
) -> [bool; COUNT] {
    #[cfg(any(
        all(target_arch = "x86_64", target_feature = "sse2"),
        all(target_arch = "aarch64", target_feature = "neon")
    ))]
    {
        let width = std::mem::size_of::<T>();
        // SAFETY: the kernel needs SSE2 on x86-64 and NEON on AArch64, and is
        // compiled only where the target has it (the cfg above).
        let mask = unsafe { native::compare(&bytes_of(left), &bytes_of(right), lanes) };
        std::array::from_fn(|index| mask[index * width] != 0)
    }
    #[cfg(not(any(
        all(target_arch = "x86_64", target_feature = "sse2"),
        all(target_arch = "aarch64", target_feature = "neon")
    )))]
    {
        let _ = lanes;
        std::array::from_fn(|index| left[index] < right[index])
    }
}

/// Each lane `when_true` where the mask is set, else `when_false`: whole
/// values moved, never converted.
#[allow(
    unsafe_code,
    reason = "Entering the baseline kernel, whose instructions every target of the architecture has."
)]
fn select<T: super::Element, const COUNT: usize>(
    mask: &[bool; COUNT],
    when_true: &[T; COUNT],
    when_false: &[T; COUNT],
) -> [T; COUNT] {
    #[cfg(any(
        all(target_arch = "x86_64", target_feature = "sse2"),
        all(target_arch = "aarch64", target_feature = "neon")
    ))]
    {
        let width = std::mem::size_of::<T>();
        let mask_bytes: [u8; 16] =
            std::array::from_fn(|index| if mask[index / width] { 255 } else { 0 });
        // SAFETY: as compare's.
        let bytes =
            unsafe { native::select(&mask_bytes, &bytes_of(when_true), &bytes_of(when_false)) };
        std::array::from_fn(|index| {
            <T as super::sealed::Sealed>::read_bytes(&bytes[index * width..(index + 1) * width])
        })
    }
    #[cfg(not(any(
        all(target_arch = "x86_64", target_feature = "sse2"),
        all(target_arch = "aarch64", target_feature = "neon")
    )))]
    {
        std::array::from_fn(|index| {
            if mask[index] {
                when_true[index]
            } else {
                when_false[index]
            }
        })
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
mod native {
    use super::Lanes;
    use crate::simd::x86_lanes::{u8x16, u8x16_lanes};
    use std::arch::x86_64::*;

    /// Each lane all ones where left < right, else zero. SSE2 compares only
    /// signed integers, so an unsigned lane has its sign bit flipped first,
    /// and a 64-bit lane is its high halves compared, then its low halves
    /// (unsigned) where the high ones are equal.
    #[target_feature(enable = "sse2")]
    pub(super) fn compare(left: &[u8; 16], right: &[u8; 16], lanes: Lanes) -> [u8; 16] {
        let left = u8x16(left);
        let right = u8x16(right);
        let mask = match lanes {
            Lanes::F64 => _mm_castpd_si128(_mm_cmplt_pd(
                _mm_castsi128_pd(left),
                _mm_castsi128_pd(right),
            )),
            Lanes::F32 => _mm_castps_si128(_mm_cmplt_ps(
                _mm_castsi128_ps(left),
                _mm_castsi128_ps(right),
            )),
            Lanes::Integer { width: 1, signed } => {
                let flip = _mm_set1_epi8(if signed { 0 } else { i8::MIN });
                _mm_cmplt_epi8(_mm_xor_si128(left, flip), _mm_xor_si128(right, flip))
            }
            Lanes::Integer { width: 2, signed } => {
                let flip = _mm_set1_epi16(if signed { 0 } else { i16::MIN });
                _mm_cmplt_epi16(_mm_xor_si128(left, flip), _mm_xor_si128(right, flip))
            }
            Lanes::Integer { width: 4, signed } => {
                let flip = _mm_set1_epi32(if signed { 0 } else { i32::MIN });
                _mm_cmplt_epi32(_mm_xor_si128(left, flip), _mm_xor_si128(right, flip))
            }
            Lanes::Integer { width: 8, signed } => {
                let flip_high = _mm_set1_epi32(if signed { 0 } else { i32::MIN });
                let left_high = _mm_xor_si128(_mm_shuffle_epi32::<0xf5>(left), flip_high);
                let right_high = _mm_xor_si128(_mm_shuffle_epi32::<0xf5>(right), flip_high);
                let high_less = _mm_cmplt_epi32(left_high, right_high);
                let high_equal = _mm_cmpeq_epi32(left_high, right_high);
                let flip_low = _mm_set1_epi32(i32::MIN);
                let left_low = _mm_xor_si128(_mm_shuffle_epi32::<0xa0>(left), flip_low);
                let right_low = _mm_xor_si128(_mm_shuffle_epi32::<0xa0>(right), flip_low);
                _mm_or_si128(
                    high_less,
                    _mm_and_si128(high_equal, _mm_cmplt_epi32(left_low, right_low)),
                )
            }
            Lanes::Integer { .. } => unreachable!("a baseline integer lane is 1, 2, 4 or 8 bytes"),
        };
        u8x16_lanes(mask)
    }

    /// Each byte from `when_true` where the mask's is set, else from
    /// `when_false`.
    #[target_feature(enable = "sse2")]
    pub(super) fn select(
        mask: &[u8; 16],
        when_true: &[u8; 16],
        when_false: &[u8; 16],
    ) -> [u8; 16] {
        let mask = u8x16(mask);
        u8x16_lanes(_mm_or_si128(
            _mm_and_si128(mask, u8x16(when_true)),
            _mm_andnot_si128(mask, u8x16(when_false)),
        ))
    }
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
mod native {
    use super::Lanes;
    use crate::simd::neon_lanes::{u8x16, u8x16_lanes};
    use std::arch::aarch64::*;

    /// Each lane all ones where left < right, else zero.
    #[target_feature(enable = "neon")]
    pub(super) fn compare(left: &[u8; 16], right: &[u8; 16], lanes: Lanes) -> [u8; 16] {
        let left = u8x16(left);
        let right = u8x16(right);
        let mask = match lanes {
            Lanes::F64 => vreinterpretq_u8_u64(vcltq_f64(
                vreinterpretq_f64_u8(left),
                vreinterpretq_f64_u8(right),
            )),
            Lanes::F32 => vreinterpretq_u8_u32(vcltq_f32(
                vreinterpretq_f32_u8(left),
                vreinterpretq_f32_u8(right),
            )),
            Lanes::Integer {
                width: 1,
                signed: true,
            } => vcltq_s8(vreinterpretq_s8_u8(left), vreinterpretq_s8_u8(right)),
            Lanes::Integer {
                width: 1,
                signed: false,
            } => vcltq_u8(left, right),
            Lanes::Integer {
                width: 2,
                signed: true,
            } => vreinterpretq_u8_u16(vcltq_s16(
                vreinterpretq_s16_u8(left),
                vreinterpretq_s16_u8(right),
            )),
            Lanes::Integer {
                width: 2,
                signed: false,
            } => vreinterpretq_u8_u16(vcltq_u16(
                vreinterpretq_u16_u8(left),
                vreinterpretq_u16_u8(right),
            )),
            Lanes::Integer {
                width: 4,
                signed: true,
            } => vreinterpretq_u8_u32(vcltq_s32(
                vreinterpretq_s32_u8(left),
                vreinterpretq_s32_u8(right),
            )),
            Lanes::Integer {
                width: 4,
                signed: false,
            } => vreinterpretq_u8_u32(vcltq_u32(
                vreinterpretq_u32_u8(left),
                vreinterpretq_u32_u8(right),
            )),
            Lanes::Integer {
                width: 8,
                signed: true,
            } => vreinterpretq_u8_u64(vcltq_s64(
                vreinterpretq_s64_u8(left),
                vreinterpretq_s64_u8(right),
            )),
            Lanes::Integer {
                width: 8,
                signed: false,
            } => vreinterpretq_u8_u64(vcltq_u64(
                vreinterpretq_u64_u8(left),
                vreinterpretq_u64_u8(right),
            )),
            Lanes::Integer { .. } => unreachable!("a baseline integer lane is 1, 2, 4 or 8 bytes"),
        };
        u8x16_lanes(mask)
    }

    /// Each byte from `when_true` where the mask's is set, else from
    /// `when_false`.
    #[target_feature(enable = "neon")]
    pub(super) fn select(
        mask: &[u8; 16],
        when_true: &[u8; 16],
        when_false: &[u8; 16],
    ) -> [u8; 16] {
        u8x16_lanes(vbslq_u8(u8x16(mask), u8x16(when_true), u8x16(when_false)))
    }
}
