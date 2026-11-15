#[allow(
    unsafe_code,
    reason = "SSE2 is the x86-64 baseline and NEON is the AArch64 baseline; inputs are full arrays."
)]
pub(super) fn compare_f64(left: &[f64; 2], right: &[f64; 2]) -> [bool; 2] {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        unsafe { native::compare_f64(left, right) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        std::array::from_fn(|index| left[index] < right[index])
    }
}

#[allow(
    unsafe_code,
    reason = "SSE2 is the x86-64 baseline and NEON is the AArch64 baseline; inputs are full arrays."
)]
pub(super) fn compare_f32(left: &[f32; 4], right: &[f32; 4]) -> [bool; 4] {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        unsafe { native::compare_f32(left, right) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        std::array::from_fn(|index| left[index] < right[index])
    }
}

#[allow(
    unsafe_code,
    reason = "Baseline bitwise selection preserves NaN payloads and the sign of zero."
)]
pub(super) fn select_f64(
    mask: &[bool; 2],
    when_true: &[f64; 2],
    when_false: &[f64; 2],
) -> [f64; 2] {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        unsafe { native::select_f64(mask, when_true, when_false) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
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

#[allow(
    unsafe_code,
    reason = "Baseline bitwise selection preserves NaN payloads and the sign of zero."
)]
pub(super) fn select_f32(
    mask: &[bool; 4],
    when_true: &[f32; 4],
    when_false: &[f32; 4],
) -> [f32; 4] {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        unsafe { native::select_f32(mask, when_true, when_false) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
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

#[allow(
    unsafe_code,
    reason = "Sealed primitive elements occupy one full baseline register and have no padding."
)]
pub(super) fn compare_integer<T: super::Element, const COUNT: usize>(
    left: &[T; COUNT],
    right: &[T; COUNT],
    signed: bool,
) -> [bool; COUNT] {
    let width = std::mem::size_of::<T>();
    assert_eq!(width * COUNT, super::REGISTER_BYTES);
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        let mask = unsafe {
            native::compare_integer(left.as_ptr().cast(), right.as_ptr().cast(), width, signed)
        };
        std::array::from_fn(|index| mask[index * width] != 0)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = signed;
        std::array::from_fn(|index| left[index] < right[index])
    }
}

#[allow(
    unsafe_code,
    reason = "Each mask selects all bytes of one sealed primitive lane from an existing valid value."
)]
pub(super) fn select_integer<T: super::Element, const COUNT: usize>(
    mask: &[bool; COUNT],
    when_true: &[T; COUNT],
    when_false: &[T; COUNT],
) -> [T; COUNT] {
    let width = std::mem::size_of::<T>();
    assert_eq!(width * COUNT, super::REGISTER_BYTES);
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        let mask_bytes: [u8; 16] =
            std::array::from_fn(|index| if mask[index / width] { 255 } else { 0 });
        let mut out = *when_false;
        unsafe {
            native::select_integer(
                &mask_bytes,
                when_true.as_ptr().cast(),
                when_false.as_ptr().cast(),
                out.as_mut_ptr().cast(),
            )
        };
        out
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
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

#[cfg(target_arch = "x86_64")]
#[allow(
    unsafe_code,
    reason = "Unaligned SSE2 loads and stores stay within fixed 16-byte arrays."
)]
mod native {
    use std::arch::x86_64::*;

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn compare_integer(
        left: *const u8,
        right: *const u8,
        width: usize,
        signed: bool,
    ) -> [u8; 16] {
        unsafe {
            let left = _mm_loadu_si128(left.cast());
            let right = _mm_loadu_si128(right.cast());
            let mask = match width {
                1 => {
                    let flip = _mm_set1_epi8(if signed { 0 } else { i8::MIN });
                    _mm_cmplt_epi8(_mm_xor_si128(left, flip), _mm_xor_si128(right, flip))
                }
                2 => {
                    let flip = _mm_set1_epi16(if signed { 0 } else { i16::MIN });
                    _mm_cmplt_epi16(_mm_xor_si128(left, flip), _mm_xor_si128(right, flip))
                }
                4 => {
                    let flip = _mm_set1_epi32(if signed { 0 } else { i32::MIN });
                    _mm_cmplt_epi32(_mm_xor_si128(left, flip), _mm_xor_si128(right, flip))
                }
                8 => {
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
                _ => unreachable!("a baseline integer lane is 1, 2, 4 or 8 bytes"),
            };
            let mut out = [0u8; 16];
            _mm_storeu_si128(out.as_mut_ptr().cast(), mask);
            out
        }
    }

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn select_integer(
        mask: &[u8; 16],
        when_true: *const u8,
        when_false: *const u8,
        out: *mut u8,
    ) {
        unsafe {
            let mask = _mm_loadu_si128(mask.as_ptr().cast());
            let left = _mm_loadu_si128(when_true.cast());
            let right = _mm_loadu_si128(when_false.cast());
            _mm_storeu_si128(
                out.cast(),
                _mm_or_si128(_mm_and_si128(mask, left), _mm_andnot_si128(mask, right)),
            );
        }
    }

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn compare_f64(left: &[f64; 2], right: &[f64; 2]) -> [bool; 2] {
        let mask = unsafe {
            _mm_movemask_pd(_mm_cmplt_pd(
                _mm_loadu_pd(left.as_ptr()),
                _mm_loadu_pd(right.as_ptr()),
            ))
        };
        std::array::from_fn(|index| mask & (1 << index) != 0)
    }

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn compare_f32(left: &[f32; 4], right: &[f32; 4]) -> [bool; 4] {
        let mask = unsafe {
            _mm_movemask_ps(_mm_cmplt_ps(
                _mm_loadu_ps(left.as_ptr()),
                _mm_loadu_ps(right.as_ptr()),
            ))
        };
        std::array::from_fn(|index| mask & (1 << index) != 0)
    }

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn select_f64(
        mask: &[bool; 2],
        when_true: &[f64; 2],
        when_false: &[f64; 2],
    ) -> [f64; 2] {
        unsafe {
            let mask = mask.map(|set| if set { u64::MAX } else { 0 });
            let mask = _mm_castsi128_pd(_mm_loadu_si128(mask.as_ptr().cast()));
            let left = _mm_loadu_pd(when_true.as_ptr());
            let right = _mm_loadu_pd(when_false.as_ptr());
            let selected = _mm_or_pd(_mm_and_pd(mask, left), _mm_andnot_pd(mask, right));
            let mut out = [0.0; 2];
            _mm_storeu_pd(out.as_mut_ptr(), selected);
            out
        }
    }

    #[target_feature(enable = "sse2")]
    pub(super) unsafe fn select_f32(
        mask: &[bool; 4],
        when_true: &[f32; 4],
        when_false: &[f32; 4],
    ) -> [f32; 4] {
        unsafe {
            let mask = mask.map(|set| if set { u32::MAX } else { 0 });
            let mask = _mm_castsi128_ps(_mm_loadu_si128(mask.as_ptr().cast()));
            let left = _mm_loadu_ps(when_true.as_ptr());
            let right = _mm_loadu_ps(when_false.as_ptr());
            let selected = _mm_or_ps(_mm_and_ps(mask, left), _mm_andnot_ps(mask, right));
            let mut out = [0.0; 4];
            _mm_storeu_ps(out.as_mut_ptr(), selected);
            out
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(
    unsafe_code,
    reason = "NEON loads and stores stay within fixed 16-byte arrays."
)]
mod native {
    use std::arch::aarch64::*;

    #[target_feature(enable = "neon")]
    pub(super) unsafe fn compare_integer(
        left: *const u8,
        right: *const u8,
        width: usize,
        signed: bool,
    ) -> [u8; 16] {
        unsafe {
            let mask = match (width, signed) {
                (1, true) => vcltq_s8(vld1q_s8(left.cast()), vld1q_s8(right.cast())),
                (1, false) => vcltq_u8(vld1q_u8(left), vld1q_u8(right)),
                (2, true) => {
                    vreinterpretq_u8_u16(vcltq_s16(vld1q_s16(left.cast()), vld1q_s16(right.cast())))
                }
                (2, false) => {
                    vreinterpretq_u8_u16(vcltq_u16(vld1q_u16(left.cast()), vld1q_u16(right.cast())))
                }
                (4, true) => {
                    vreinterpretq_u8_u32(vcltq_s32(vld1q_s32(left.cast()), vld1q_s32(right.cast())))
                }
                (4, false) => {
                    vreinterpretq_u8_u32(vcltq_u32(vld1q_u32(left.cast()), vld1q_u32(right.cast())))
                }
                (8, true) => {
                    vreinterpretq_u8_u64(vcltq_s64(vld1q_s64(left.cast()), vld1q_s64(right.cast())))
                }
                (8, false) => {
                    vreinterpretq_u8_u64(vcltq_u64(vld1q_u64(left.cast()), vld1q_u64(right.cast())))
                }
                _ => unreachable!("a baseline integer lane is 1, 2, 4 or 8 bytes"),
            };
            let mut out = [0u8; 16];
            vst1q_u8(out.as_mut_ptr(), mask);
            out
        }
    }

    #[target_feature(enable = "neon")]
    pub(super) unsafe fn select_integer(
        mask: &[u8; 16],
        when_true: *const u8,
        when_false: *const u8,
        out: *mut u8,
    ) {
        unsafe {
            vst1q_u8(
                out,
                vbslq_u8(
                    vld1q_u8(mask.as_ptr()),
                    vld1q_u8(when_true),
                    vld1q_u8(when_false),
                ),
            )
        };
    }

    #[target_feature(enable = "neon")]
    pub(super) unsafe fn compare_f64(left: &[f64; 2], right: &[f64; 2]) -> [bool; 2] {
        let mask = unsafe { vcltq_f64(vld1q_f64(left.as_ptr()), vld1q_f64(right.as_ptr())) };
        let mut out = [0u64; 2];
        unsafe { vst1q_u64(out.as_mut_ptr(), mask) };
        out.map(|bits| bits != 0)
    }

    #[target_feature(enable = "neon")]
    pub(super) unsafe fn compare_f32(left: &[f32; 4], right: &[f32; 4]) -> [bool; 4] {
        let mask = unsafe { vcltq_f32(vld1q_f32(left.as_ptr()), vld1q_f32(right.as_ptr())) };
        let mut out = [0u32; 4];
        unsafe { vst1q_u32(out.as_mut_ptr(), mask) };
        out.map(|bits| bits != 0)
    }

    #[target_feature(enable = "neon")]
    pub(super) unsafe fn select_f64(
        mask: &[bool; 2],
        when_true: &[f64; 2],
        when_false: &[f64; 2],
    ) -> [f64; 2] {
        let mask = mask.map(|set| if set { u64::MAX } else { 0 });
        let selected = unsafe {
            vbslq_f64(
                vld1q_u64(mask.as_ptr()),
                vld1q_f64(when_true.as_ptr()),
                vld1q_f64(when_false.as_ptr()),
            )
        };
        let mut out = [0.0; 2];
        unsafe { vst1q_f64(out.as_mut_ptr(), selected) };
        out
    }

    #[target_feature(enable = "neon")]
    pub(super) unsafe fn select_f32(
        mask: &[bool; 4],
        when_true: &[f32; 4],
        when_false: &[f32; 4],
    ) -> [f32; 4] {
        let mask = mask.map(|set| if set { u32::MAX } else { 0 });
        let selected = unsafe {
            vbslq_f32(
                vld1q_u32(mask.as_ptr()),
                vld1q_f32(when_true.as_ptr()),
                vld1q_f32(when_false.as_ptr()),
            )
        };
        let mut out = [0.0; 4];
        unsafe { vst1q_f32(out.as_mut_ptr(), selected) };
        out
    }
}
