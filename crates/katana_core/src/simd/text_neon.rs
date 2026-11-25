//! NEON text kernels, for AArch64, entered from text_kernels.rs. Safe
//! functions: no intrinsic here takes a pointer, and the bytes come and go
//! through slices and neon_lanes.

use super::neon_lanes::{u8x16, u8x16_lanes};
use crate::text_kernels::{AsciiSlot, put};
use crate::text_kernels::lookup::{FIRST_HIGH, FIRST_LOW, SECOND_HIGH};
use std::arch::aarch64::*;

/// The 16 bytes of `bytes` from `at` as one register: the copy and the build
/// fold into the one LDR the pointer form was.
#[inline]
#[target_feature(enable = "neon")]
fn block_at(bytes: &[u8], at: usize) -> uint8x16_t {
    let mut block = [0u8; 16];
    block.copy_from_slice(&bytes[at..at + 16]);
    u8x16(&block)
}

/// Sixteen UTF-16 code units' 32 bytes split into their even bytes and their
/// odd bytes, as LD2 splits them: UZP1 and UZP2 of the two halves.
#[inline]
#[target_feature(enable = "neon")]
fn deinterleave(bytes: &[u8; 32]) -> (uint8x16_t, uint8x16_t) {
    let (halves, _) = bytes.as_chunks::<16>();
    let low = u8x16(&halves[0]);
    let high = u8x16(&halves[1]);
    (vuzp1q_u8(low, high), vuzp2q_u8(low, high))
}

/// The first `units` units of `input` narrowed into `out` from 0, 32 a step,
/// while every unit of a step is ASCII: the number written, a multiple of 32.
/// The caller narrows the rest with the scalar loop.
#[target_feature(enable = "neon")]
pub(crate) fn narrow_ascii_utf16<S: AsciiSlot>(
    input: &[u8],
    units: usize,
    little_endian: bool,
    out: &mut [S],
) -> usize {
    let mut index = 0;
    let (halves, _) = input[..2 * units].as_chunks::<32>();
    for step in halves.as_chunks::<2>().0 {
        let first = deinterleave(&step[0]);
        let second = deinterleave(&step[1]);
        let (low_first, high_first) = if little_endian {
            (first.0, first.1)
        } else {
            (first.1, first.0)
        };
        let (low_second, high_second) = if little_endian {
            (second.0, second.1)
        } else {
            (second.1, second.0)
        };
        let not_ascii = vorrq_u8(
            vorrq_u8(high_first, high_second),
            vandq_u8(vorrq_u8(low_first, low_second), vdupq_n_u8(0x80)),
        );
        if vmaxvq_u8(not_ascii) != 0 {
            break;
        }
        put(out, index, &u8x16_lanes(low_first));
        put(out, index + 16, &u8x16_lanes(low_second));
        index += 32;
    }
    index
}

/// 1 when the bytes are valid UTF-8, 0 when not; -1 below 64 bytes, which the
/// kernel does not judge.
#[target_feature(enable = "neon")]
pub(crate) fn valid_utf8(bytes: &[u8]) -> i32 {
    let size = bytes.len();
    if size < 64 {
        return -1;
    }
    let first_high = u8x16(&FIRST_HIGH);
    let first_low = u8x16(&FIRST_LOW);
    let second_high = u8x16(&SECOND_HIGH);
    let mut end_limits = [255u8; 16];
    end_limits[13] = 0xf0 - 1;
    end_limits[14] = 0xe0 - 1;
    end_limits[15] = 0xc0 - 1;
    let open_at_end = u8x16(&end_limits);
    let mut errors = vdupq_n_u8(0);
    let mut prior = vdupq_n_u8(0);
    let mut open = vdupq_n_u8(0);
    macro_rules! check {
        ($input:expr) => {{
            let input = $input;
            let prev1 = vextq_u8::<15>(prior, input);
            let prev2 = vextq_u8::<14>(prior, input);
            let prev3 = vextq_u8::<13>(prior, input);
            let pair_errors = vandq_u8(
                vandq_u8(
                    vqtbl1q_u8(first_high, vshrq_n_u8::<4>(prev1)),
                    vqtbl1q_u8(first_low, vandq_u8(prev1, vdupq_n_u8(0x0f))),
                ),
                vqtbl1q_u8(second_high, vshrq_n_u8::<4>(input)),
            );
            let third = vqsubq_u8(prev2, vdupq_n_u8(0xe0 - 0x80));
            let fourth = vqsubq_u8(prev3, vdupq_n_u8(0xf0 - 0x80));
            let wanted = vandq_u8(vorrq_u8(third, fourth), vdupq_n_u8(0x80));
            errors = vorrq_u8(errors, veorq_u8(wanted, pair_errors));
            prior = input;
        }};
    }
    // What is still open at the end of a run of blocks is the last block's alone: it is taken
    // once, after the run's last check, where taking it in each check left three of every four
    // values unread (an unused assignment, which -D warnings refuses: found by the first
    // compile for ARM64, 2026-11-25, tools/build_visible.sh targets).
    let mut index = 0;
    while size - index >= 64 {
        let first = block_at(bytes, index);
        let second = block_at(bytes, index + 16);
        let third = block_at(bytes, index + 32);
        let fourth = block_at(bytes, index + 48);
        if vmaxvq_u8(vorrq_u8(vorrq_u8(first, second), vorrq_u8(third, fourth))) < 0x80 {
            errors = vorrq_u8(errors, open);
            open = vdupq_n_u8(0);
            prior = fourth;
        } else {
            check!(first);
            check!(second);
            check!(third);
            check!(fourth);
            open = vqsubq_u8(fourth, open_at_end);
            if vmaxvq_u8(errors) != 0 {
                return 0;
            }
        }
        index += 64;
    }
    while size - index >= 16 {
        let block = block_at(bytes, index);
        if vmaxvq_u8(block) < 0x80 {
            errors = vorrq_u8(errors, open);
            open = vdupq_n_u8(0);
            prior = block;
        } else {
            check!(block);
            open = vqsubq_u8(block, open_at_end);
        }
        index += 16;
    }
    if index < size {
        prior = block_at(bytes, size - 32);
        let last = block_at(bytes, size - 16);
        check!(last);
        open = vqsubq_u8(last, open_at_end);
    }
    let _ = prior;
    i32::from(vmaxvq_u8(vorrq_u8(errors, open)) == 0)
}
