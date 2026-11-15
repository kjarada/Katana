#![allow(
    unsafe_code,
    reason = "NEON text kernels use bounded raw loads/stores on AArch64 after runtime selection."
)]

use crate::text_kernels::lookup::{FIRST_HIGH, FIRST_LOW, SECOND_HIGH};
use std::arch::aarch64::*;

#[target_feature(enable = "neon")]
pub(crate) unsafe fn narrow_ascii_utf16(
    input: *const u8,
    units: usize,
    little_endian: bool,
    out: *mut u8,
) -> usize {
    unsafe {
        let mut index = 0;
        while units - index >= 32 {
            let first = vld2q_u8(input.add(2 * index));
            let second = vld2q_u8(input.add(2 * index + 32));
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
            vst1q_u8(out.add(index), low_first);
            vst1q_u8(out.add(index + 16), low_second);
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

#[target_feature(enable = "neon")]
pub(crate) unsafe fn valid_utf8(bytes: *const u8, size: usize) -> i32 {
    if size < 64 {
        return -1;
    }
    unsafe {
        let first_high = vld1q_u8(FIRST_HIGH.as_ptr());
        let first_low = vld1q_u8(FIRST_LOW.as_ptr());
        let second_high = vld1q_u8(SECOND_HIGH.as_ptr());
        let mut end_limits = [255u8; 16];
        end_limits[13] = 0xf0 - 1;
        end_limits[14] = 0xe0 - 1;
        end_limits[15] = 0xc0 - 1;
        let open_at_end = vld1q_u8(end_limits.as_ptr());
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
                open = vqsubq_u8(input, open_at_end);
                prior = input;
            }};
        }
        let mut index = 0;
        while size - index >= 64 {
            let first = vld1q_u8(bytes.add(index));
            let second = vld1q_u8(bytes.add(index + 16));
            let third = vld1q_u8(bytes.add(index + 32));
            let fourth = vld1q_u8(bytes.add(index + 48));
            if vmaxvq_u8(vorrq_u8(vorrq_u8(first, second), vorrq_u8(third, fourth))) < 0x80 {
                errors = vorrq_u8(errors, open);
                open = vdupq_n_u8(0);
                prior = fourth;
            } else {
                check!(first);
                check!(second);
                check!(third);
                check!(fourth);
                if vmaxvq_u8(errors) != 0 {
                    return 0;
                }
            }
            index += 64;
        }
        while size - index >= 16 {
            let block = vld1q_u8(bytes.add(index));
            if vmaxvq_u8(block) < 0x80 {
                errors = vorrq_u8(errors, open);
                open = vdupq_n_u8(0);
                prior = block;
            } else {
                check!(block);
            }
            index += 16;
        }
        if index < size {
            prior = vld1q_u8(bytes.add(size - 32));
            check!(vld1q_u8(bytes.add(size - 16)));
        }
        let _ = prior;
        i32::from(vmaxvq_u8(vorrq_u8(errors, open)) == 0)
    }
}
