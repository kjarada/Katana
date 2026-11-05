#[cfg(all(
    feature = "simd-kernels",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
use crate::cpu_features::{SimdLevel, active_simd_level};

pub fn valid_utf8_scalar(bytes: &[u8]) -> bool {
    let mut at = 0;
    while at < bytes.len() {
        let lead = bytes[at];
        let mut low_second = 0x80;
        let mut high_second = 0xbf;
        let following = match lead {
            0x00..=0x7f => {
                at += 1;
                continue;
            }
            0xc2..=0xdf => 1,
            0xe0 => {
                low_second = 0xa0;
                2
            }
            0xe1..=0xec => 2,
            0xed => {
                high_second = 0x9f;
                2
            }
            0xee..=0xef => 2,
            0xf0 => {
                low_second = 0x90;
                3
            }
            0xf1..=0xf3 => 3,
            0xf4 => {
                high_second = 0x8f;
                3
            }
            _ => return false,
        };
        if at + following >= bytes.len() {
            return false;
        }
        if bytes[at + 1] < low_second || bytes[at + 1] > high_second {
            return false;
        }
        for byte in &bytes[at + 2..at + following + 1] {
            if !(0x80..=0xbf).contains(byte) {
                return false;
            }
        }
        at += following + 1;
    }
    true
}

#[allow(
    unsafe_code,
    reason = "Only a detected, enabled kernel receives the slice's exact bounds; short inputs stay scalar."
)]
pub fn valid_utf8(bytes: &[u8]) -> bool {
    #[cfg(all(
        feature = "simd-kernels",
        any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
    ))]
    if bytes.len() >= 64 {
        #[cfg(all(
            feature = "simd-kernels",
            any(target_arch = "x86", target_arch = "x86_64")
        ))]
        if active_simd_level() == SimdLevel::Avx2 {
            return unsafe { crate::simd::text_avx2::valid_utf8(bytes.as_ptr(), bytes.len()) == 1 };
        }
        #[cfg(all(feature = "simd-kernels", target_arch = "aarch64"))]
        if active_simd_level() == SimdLevel::Neon {
            return unsafe { crate::simd::text_neon::valid_utf8(bytes.as_ptr(), bytes.len()) == 1 };
        }
    }
    valid_utf8_scalar(bytes)
}

#[allow(
    unsafe_code,
    reason = "Checked slice bounds supply the shared scalar narrowing loop's exact capacity."
)]
pub fn narrow_ascii_utf16_scalar(
    input: &[u8],
    units: usize,
    little_endian: bool,
    out: &mut [u8],
) -> usize {
    assert!(units <= input.len() / 2 && units <= out.len());
    unsafe { narrow_scalar_unchecked(input.as_ptr(), units, little_endian, out.as_mut_ptr()) }
}

#[allow(
    unsafe_code,
    reason = "Slice bounds establish the raw narrowing kernel's input and output capacity."
)]
pub fn narrow_ascii_utf16(
    input: &[u8],
    units: usize,
    little_endian: bool,
    out: &mut [u8],
) -> usize {
    assert!(units <= input.len() / 2 && units <= out.len());
    unsafe { narrow_unchecked(input.as_ptr(), units, little_endian, out.as_mut_ptr()) }
}

#[allow(
    unsafe_code,
    reason = "Both callers reserve or check all units; kernels write only ASCII and the returned prefix length."
)]
unsafe fn narrow_unchecked(
    input: *const u8,
    units: usize,
    little_endian: bool,
    out: *mut u8,
) -> usize {
    #[cfg(all(
        feature = "simd-kernels",
        any(target_arch = "x86", target_arch = "x86_64")
    ))]
    if active_simd_level() == SimdLevel::Avx2 {
        return unsafe {
            crate::simd::text_avx2::narrow_ascii_utf16(input, units, little_endian, out)
        };
    }
    #[cfg(all(feature = "simd-kernels", target_arch = "aarch64"))]
    if active_simd_level() == SimdLevel::Neon {
        return unsafe {
            crate::simd::text_neon::narrow_ascii_utf16(input, units, little_endian, out)
        };
    }
    unsafe { narrow_scalar_unchecked(input, units, little_endian, out) }
}

#[allow(
    unsafe_code,
    reason = "Callers validate or reserve the full input/output span; only the ASCII prefix is read and written."
)]
unsafe fn narrow_scalar_unchecked(
    input: *const u8,
    units: usize,
    little_endian: bool,
    out: *mut u8,
) -> usize {
    let mut index = 0;
    while index < units {
        let pair = unsafe { [*input.add(2 * index), *input.add(2 * index + 1)] };
        let unit = if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        };
        if unit >= 0x80 {
            break;
        }
        unsafe { out.add(index).write(unit as u8) };
        index += 1;
    }
    index
}

#[allow(
    unsafe_code,
    reason = "Narrowing writes only ASCII into reserved spare capacity; no uninitialised bytes enter the String length."
)]
pub(crate) fn append_ascii_utf16(input: &[u8], little_endian: bool, out: &mut String) -> usize {
    let units = input.len() / 2;
    out.reserve(units);
    let length = out.len();
    unsafe {
        let bytes = out.as_mut_vec();
        let written = narrow_unchecked(
            input.as_ptr(),
            units,
            little_endian,
            bytes.as_mut_ptr().add(length),
        );
        bytes.set_len(length + written);
        written
    }
}

#[cfg(all(
    feature = "simd-kernels",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
#[doc = "Keiser and Lemire, Validating UTF-8 in less than one instruction per byte (2021): the C++ oracle's lookup tables."]
pub(crate) mod lookup {
    const TOO_SHORT: u8 = 1 << 0;
    const TOO_LONG: u8 = 1 << 1;
    const OVERLONG3: u8 = 1 << 2;
    const TOO_LARGE: u8 = 1 << 3;
    const SURROGATE: u8 = 1 << 4;
    const OVERLONG2: u8 = 1 << 5;
    const TOO_LARGE1000: u8 = 1 << 6;
    const OVERLONG4: u8 = 1 << 6;
    const TWO_CONTS: u8 = 1 << 7;
    const CARRY: u8 = TOO_SHORT | TOO_LONG | TWO_CONTS;
    const LARGE: u8 = TOO_LARGE | TOO_LARGE1000;

    pub const FIRST_HIGH: [u8; 16] = [
        TOO_LONG,
        TOO_LONG,
        TOO_LONG,
        TOO_LONG,
        TOO_LONG,
        TOO_LONG,
        TOO_LONG,
        TOO_LONG,
        TWO_CONTS,
        TWO_CONTS,
        TWO_CONTS,
        TWO_CONTS,
        TOO_SHORT | OVERLONG2,
        TOO_SHORT,
        TOO_SHORT | OVERLONG3 | SURROGATE,
        TOO_SHORT | LARGE | OVERLONG4,
    ];
    pub const FIRST_LOW: [u8; 16] = [
        CARRY | OVERLONG3 | OVERLONG2 | OVERLONG4,
        CARRY | OVERLONG2,
        CARRY,
        CARRY,
        CARRY | TOO_LARGE,
        CARRY | LARGE,
        CARRY | LARGE,
        CARRY | LARGE,
        CARRY | LARGE,
        CARRY | LARGE,
        CARRY | LARGE,
        CARRY | LARGE,
        CARRY | LARGE,
        CARRY | LARGE | SURROGATE,
        CARRY | LARGE,
        CARRY | LARGE,
    ];
    pub const SECOND_HIGH: [u8; 16] = [
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
        TOO_LONG | OVERLONG2 | TWO_CONTS | OVERLONG3 | TOO_LARGE1000 | OVERLONG4,
        TOO_LONG | OVERLONG2 | TWO_CONTS | OVERLONG3 | TOO_LARGE,
        TOO_LONG | OVERLONG2 | TWO_CONTS | SURROGATE | TOO_LARGE,
        TOO_LONG | OVERLONG2 | TWO_CONTS | SURROGATE | TOO_LARGE,
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
        TOO_SHORT,
    ];
}
