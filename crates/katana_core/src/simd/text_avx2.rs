//! AVX2 text kernels, entered from text_kernels.rs only once the processor is
//! known to run AVX2. Safe functions: no intrinsic here takes a pointer, and
//! the bytes come and go through slices and x86_lanes.

use super::x86_lanes::{u8x16, u8x32, u8x32_lanes};
use crate::text_kernels::{AsciiSlot, put};
use crate::text_kernels::lookup::{FIRST_HIGH, FIRST_LOW, SECOND_HIGH};
#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// The 32 bytes of `bytes` from `at` as one register: the copy and the build
/// fold into the one unaligned load the pointer form was.
#[inline]
#[target_feature(enable = "avx2")]
fn block_at(bytes: &[u8], at: usize) -> __m256i {
    let mut block = [0u8; 32];
    block.copy_from_slice(&bytes[at..at + 32]);
    u8x32(&block)
}

/// The first `units` units of `input` narrowed into `out` from 0, 32 a step,
/// while every unit of a step is ASCII: the number written, a multiple of 32.
/// The caller narrows the rest with the scalar loop, which takes over at the
/// step with the first non-ASCII unit.
#[target_feature(enable = "avx2")]
pub(crate) fn narrow_ascii_utf16<S: AsciiSlot>(
    input: &[u8],
    units: usize,
    little_endian: bool,
    out: &mut [S],
) -> usize {
    let not_ascii = _mm256_set1_epi16(if little_endian { 0xff80u16 } else { 0x80ffu16 } as i16);
    let mut index = 0;
    let (halves, _) = input[..2 * units].as_chunks::<32>();
    for step in halves.as_chunks::<2>().0 {
        let mut low = u8x32(&step[0]);
        let mut high = u8x32(&step[1]);
        if _mm256_testz_si256(_mm256_or_si256(low, high), not_ascii) == 0 {
            break;
        }
        if !little_endian {
            low = _mm256_srli_epi16::<8>(low);
            high = _mm256_srli_epi16::<8>(high);
        }
        let packed = _mm256_permute4x64_epi64::<0xd8>(_mm256_packus_epi16(low, high));
        put(out, index, &u8x32_lanes(packed));
        index += 32;
    }
    index
}

/// 1 when the bytes are valid UTF-8, 0 when not; -1 below 64 bytes, which the
/// kernel does not judge (its last block is judged with the 32 bytes before
/// it).
#[target_feature(enable = "avx2")]
pub(crate) fn valid_utf8(bytes: &[u8]) -> i32 {
    let size = bytes.len();
    if size < 64 {
        return -1;
    }
    let first_high = _mm256_broadcastsi128_si256(u8x16(&FIRST_HIGH));
    let first_low = _mm256_broadcastsi128_si256(u8x16(&FIRST_LOW));
    let second_high = _mm256_broadcastsi128_si256(u8x16(&SECOND_HIGH));
    let mut end_limits = [255u8; 32];
    end_limits[29] = 0xf0 - 1;
    end_limits[30] = 0xe0 - 1;
    end_limits[31] = 0xc0 - 1;
    let open_at_end = u8x32(&end_limits);
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
        let first = block_at(bytes, index);
        let second = block_at(bytes, index + 32);
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
        let block = block_at(bytes, index);
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
        prior = block_at(bytes, size - 64);
        open = check!(block_at(bytes, size - 32));
    }
    let _ = prior;
    let either = _mm256_or_si256(errors, open);
    i32::from(_mm256_testz_si256(either, either) != 0)
}
