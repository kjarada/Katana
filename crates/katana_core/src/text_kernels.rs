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

/// Whether `bytes` is valid UTF-8: the kernel of the active SIMD level from
/// 64 bytes, else the byte loop.
#[allow(
    unsafe_code,
    reason = "Entering a target-feature kernel, only on a processor found to run it."
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
            // SAFETY: the level is AVX2 only on a processor found to run AVX2
            // (cpu_features' set_simd_level refuses a level it cannot run).
            return unsafe { crate::simd::text_avx2::valid_utf8(bytes) == 1 };
        }
        #[cfg(all(feature = "simd-kernels", target_arch = "aarch64"))]
        if active_simd_level() == SimdLevel::Neon {
            // SAFETY: NEON is part of every AArch64 target.
            return unsafe { crate::simd::text_neon::valid_utf8(bytes) == 1 };
        }
    }
    valid_utf8_scalar(bytes)
}

/// A byte a narrowing writes. The kernels are written over this and not over
/// `u8` because a narrowing also wrote, until 2026-11-22, into a String's
/// spare capacity, whose bytes are not yet initialised; a byte of a slice is
/// the one implementer left, and the kernels (`simd/text_avx2.rs`,
/// `simd/text_neon.rs`) compile for it to the stores they always made.
pub(crate) trait AsciiSlot {
    fn set(&mut self, byte: u8);
}

impl AsciiSlot for u8 {
    fn set(&mut self, byte: u8) {
        *self = byte;
    }
}

/// `bytes` into `out` from `at`: a kernel's whole step, which LLVM makes one
/// store.
#[cfg(all(
    feature = "simd-kernels",
    any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) fn put<S: AsciiSlot>(out: &mut [S], at: usize, bytes: &[u8]) {
    for (slot, &byte) in out[at..at + bytes.len()].iter_mut().zip(bytes) {
        slot.set(byte);
    }
}

pub fn narrow_ascii_utf16_scalar(
    input: &[u8],
    units: usize,
    little_endian: bool,
    out: &mut [u8],
) -> usize {
    assert!(units <= input.len() / 2 && units <= out.len());
    narrow_scalar_from(input, 0, units, little_endian, out)
}

pub fn narrow_ascii_utf16(
    input: &[u8],
    units: usize,
    little_endian: bool,
    out: &mut [u8],
) -> usize {
    assert!(units <= input.len() / 2 && units <= out.len());
    narrow(input, units, little_endian, out)
}

/// The ASCII prefix of the first `units` units of `input`, narrowed into
/// `out` from 0: the kernel's whole steps, then the unit loop from where they
/// stop. Every slot before the count returned is written, and no other.
fn narrow<S: AsciiSlot>(input: &[u8], units: usize, little_endian: bool, out: &mut [S]) -> usize {
    let from = narrowed_by_kernel(input, units, little_endian, out);
    narrow_scalar_from(input, from, units, little_endian, out)
}

/// How many units the active level's kernel narrowed, whole steps of 32
/// while every unit is ASCII; 0 at the scalar level.
#[allow(
    unsafe_code,
    reason = "Entering a target-feature kernel, only on a processor found to run it."
)]
fn narrowed_by_kernel<S: AsciiSlot>(
    input: &[u8],
    units: usize,
    little_endian: bool,
    out: &mut [S],
) -> usize {
    #[cfg(all(
        feature = "simd-kernels",
        any(target_arch = "x86", target_arch = "x86_64")
    ))]
    if active_simd_level() == SimdLevel::Avx2 {
        // SAFETY: as valid_utf8's.
        return unsafe {
            crate::simd::text_avx2::narrow_ascii_utf16(input, units, little_endian, out)
        };
    }
    #[cfg(all(feature = "simd-kernels", target_arch = "aarch64"))]
    if active_simd_level() == SimdLevel::Neon {
        // SAFETY: as valid_utf8's.
        return unsafe {
            crate::simd::text_neon::narrow_ascii_utf16(input, units, little_endian, out)
        };
    }
    let _ = (input, units, little_endian, out);
    0
}

/// The unit loop, from unit `from` until the first non-ASCII unit or
/// `units`: where it stopped.
fn narrow_scalar_from<S: AsciiSlot>(
    input: &[u8],
    from: usize,
    units: usize,
    little_endian: bool,
    out: &mut [S],
) -> usize {
    let mut index = from;
    let (pairs, _) = input[2 * from..2 * units].as_chunks::<2>();
    for (&pair, slot) in pairs.iter().zip(&mut out[from..units]) {
        let unit = if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        };
        if unit >= 0x80 {
            break;
        }
        slot.set(unit as u8);
        index += 1;
    }
    index
}

/// How many units one step of an append narrows, and so the size of the
/// scratch a decode keeps for it. A multiple of the kernels' 32, so their
/// steps fall where they fell when a run was narrowed whole; small enough
/// that a step works within 12 KiB (the scratch and the 8192 bytes it is
/// narrowed from), so the check and the copy that follow read what the
/// narrowing has just written, and that clearing the scratch once a decode
/// is nothing; large enough that the two calls a step adds are spread over
/// thousands of bytes. A choice by reasoning, not by measurement: no
/// benchmark covers decoding (docs/rust.md, "The third pass").
pub(crate) const APPEND_BLOCK: usize = 4096;

/// The ASCII prefix of `input`'s UTF-16 units appended to `out`: how many
/// units it took. A block at a time: narrowed into `scratch`, then appended
/// as text. A String takes bytes only as a `str`, so each block is checked to
/// be UTF-8 on its way in - a check ASCII cannot fail - and copied; the
/// String's own spare capacity, which this narrowed straight into until
/// 2026-11-22, is reached only through two unsafe calls. The scratch is the
/// caller's, one a decode: a text dense with other characters calls this
/// once a run, and a scratch made here would be cleared every time.
pub(crate) fn append_ascii_utf16(
    input: &[u8],
    little_endian: bool,
    scratch: &mut [u8; APPEND_BLOCK],
    out: &mut String,
) -> usize {
    let units = input.len() / 2;
    let mut taken = 0;
    while taken < units {
        let step = (units - taken).min(APPEND_BLOCK);
        let written = narrow(
            &input[2 * taken..],
            step,
            little_endian,
            &mut scratch[..step],
        );
        out.push_str(
            std::str::from_utf8(&scratch[..written]).expect("narrowed units are ASCII"),
        );
        taken += written;
        // A block cut short met a unit that is not ASCII: the caller's to decode.
        if written < step {
            break;
        }
    }
    taken
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
