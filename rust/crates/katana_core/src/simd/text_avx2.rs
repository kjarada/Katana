#![allow(
    unsafe_code,
    reason = "AVX2 text kernels use bounded raw loads/stores only after runtime CPU selection."
)]

use crate::text_kernels::lookup::{FIRST_HIGH, FIRST_LOW, SECOND_HIGH};
#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[target_feature(enable = "avx2")]
pub(crate) unsafe fn narrow_ascii_utf16(
    input: *const u8,
    units: usize,
    little_endian: bool,
    out: *mut u8,
) -> usize {
    unsafe {
        let not_ascii = _mm256_set1_epi16(if little_endian { 0xff80u16 } else { 0x80ffu16 } as i16);
        let mut index = 0;
        while units - index >= 32 {
            let mut low = _mm256_loadu_si256(input.add(2 * index).cast());
            let mut high = _mm256_loadu_si256(input.add(2 * index + 32).cast());
            if _mm256_testz_si256(_mm256_or_si256(low, high), not_ascii) == 0 {
                break;
            }
            if !little_endian {
                low = _mm256_srli_epi16::<8>(low);
                high = _mm256_srli_epi16::<8>(high);
            }
            let packed = _mm256_permute4x64_epi64::<0xd8>(_mm256_packus_epi16(low, high));
            _mm256_storeu_si256(out.add(index).cast(), packed);
            index += 32;
        }
        while index < units {
            let first = u16::from(*input.add(2 * index));
            let second = u16::from(*input.add(2 * index + 1));
            let unit = if little_endian {
                first | (second << 8)
            } else {
                (first << 8) | second
            };
            if unit >= 0x80 {
                break;
            }
            out.add(index).write(unit as u8);
            index += 1;
        }
        index
    }
}

#[target_feature(enable = "avx2")]
pub(crate) unsafe fn valid_utf8(bytes: *const u8, size: usize) -> i32 {
    if size < 64 {
        return -1;
    }
    unsafe {
        let first_high = _mm256_broadcastsi128_si256(_mm_loadu_si128(FIRST_HIGH.as_ptr().cast()));
        let first_low = _mm256_broadcastsi128_si256(_mm_loadu_si128(FIRST_LOW.as_ptr().cast()));
        let second_high = _mm256_broadcastsi128_si256(_mm_loadu_si128(SECOND_HIGH.as_ptr().cast()));
        let mut end_limits = [255u8; 32];
        end_limits[29] = 0xf0 - 1;
        end_limits[30] = 0xe0 - 1;
        end_limits[31] = 0xc0 - 1;
        let open_at_end = _mm256_loadu_si256(end_limits.as_ptr().cast());
        let mut errors = _mm256_setzero_si256();
        let mut prior = _mm256_setzero_si256();
        let mut open = _mm256_setzero_si256();
        macro_rules! check {
            ($input:expr) => {{
                let input = $input;
                let nibble = _mm256_set1_epi8(0x0f);
                let joined = _mm256_permute2x128_si256::<0x21>(prior, input);
                let prev1 = _mm256_alignr_epi8::<15>(input, joined);
                let prev2 = _mm256_alignr_epi8::<14>(input, joined);
                let prev3 = _mm256_alignr_epi8::<13>(input, joined);
                let high_prev = _mm256_and_si256(_mm256_srli_epi16::<4>(prev1), nibble);
                let high_input = _mm256_and_si256(_mm256_srli_epi16::<4>(input), nibble);
                let pair_errors = _mm256_and_si256(
                    _mm256_and_si256(
                        _mm256_shuffle_epi8(first_high, high_prev),
                        _mm256_shuffle_epi8(first_low, _mm256_and_si256(prev1, nibble)),
                    ),
                    _mm256_shuffle_epi8(second_high, high_input),
                );
                let third = _mm256_subs_epu8(prev2, _mm256_set1_epi8((0xe0u8 - 0x80) as i8));
                let fourth = _mm256_subs_epu8(prev3, _mm256_set1_epi8((0xf0u8 - 0x80) as i8));
                let wanted = _mm256_and_si256(
                    _mm256_or_si256(third, fourth),
                    _mm256_set1_epi8(0x80u8 as i8),
                );
                errors = _mm256_or_si256(errors, _mm256_xor_si256(wanted, pair_errors));
                prior = input;
                _mm256_subs_epu8(input, open_at_end)
            }};
        }
        let mut index = 0;
        while size - index >= 64 {
            let first = _mm256_loadu_si256(bytes.add(index).cast());
            let second = _mm256_loadu_si256(bytes.add(index + 32).cast());
            if _mm256_movemask_epi8(_mm256_or_si256(first, second)) == 0 {
                errors = _mm256_or_si256(errors, open);
                open = _mm256_setzero_si256();
                prior = second;
            } else {
                let _ = check!(first);
                open = check!(second);
                if _mm256_testz_si256(errors, errors) == 0 {
                    return 0;
                }
            }
            index += 64;
        }
        if size - index >= 32 {
            let block = _mm256_loadu_si256(bytes.add(index).cast());
            if _mm256_movemask_epi8(block) == 0 {
                errors = _mm256_or_si256(errors, open);
                open = _mm256_setzero_si256();
                prior = block;
            } else {
                open = check!(block);
            }
            index += 32;
        }
        if index < size {
            prior = _mm256_loadu_si256(bytes.add(size - 64).cast());
            open = check!(_mm256_loadu_si256(bytes.add(size - 32).cast()));
        }
        let _ = prior;
        let either = _mm256_or_si256(errors, open);
        i32::from(_mm256_testz_si256(either, either) != 0)
    }
}
