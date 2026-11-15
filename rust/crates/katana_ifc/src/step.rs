//! The ISO 10303-21 ("STEP physical file") encodings an IFC file is written in,
//! and the IFC globally unique identifier. Public so that the tests can check
//! each against a worked example; the writer (export.rs) is their one user.
//!
//! Everything here is deterministic (Rule 7): the same value gives the same
//! text on every platform, which is what lets two exports of an unchanged
//! drawing be compared byte for byte.

use katana_core::text::format_exact_real;

/// IfcGloballyUniqueId's alphabet, from the IFC documentation of the type.
const GUID_ALPHABET: &[u8; 64] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// FNV-1a, 64 bit: offset basis 14695981039346656037 and prime 1099511628211
/// (Fowler, Noll and Vo's published parameters).
const FNV_PRIME: u64 = 1_099_511_628_211;
const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// SplitMix64's finaliser (Steele, Lea and Flood, "Fast splittable
/// pseudorandom number generators", 2014): FNV-1a's high bits change little
/// for keys that differ in their last characters - "entity/41" and
/// "entity/42" - and the GlobalId's first characters are its high bits.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One code point from `text` at `at`, advancing it; U+FFFD for a byte that
/// does not begin a well-formed sequence, consuming just that byte.
fn next_code_point(text: &[u8], at: &mut usize) -> u32 {
    let lead = text[*at];
    if lead < 0x80 {
        *at += 1;
        return u32::from(lead);
    }
    let (length, mut value) = if (lead & 0xE0) == 0xC0 {
        (2_usize, u32::from(lead & 0x1F))
    } else if (lead & 0xF0) == 0xE0 {
        (3, u32::from(lead & 0x0F))
    } else if (lead & 0xF8) == 0xF0 {
        (4, u32::from(lead & 0x07))
    } else {
        *at += 1;
        return 0xFFFD;
    };
    if *at + length > text.len() {
        *at += 1;
        return 0xFFFD;
    }
    for i in 1..length {
        let byte = text[*at + i];
        if (byte & 0xC0) != 0x80 {
            *at += 1;
            return 0xFFFD;
        }
        value = (value << 6) | u32::from(byte & 0x3F);
    }
    // Overlong forms, surrogates and values past U+10FFFF are not UTF-8.
    const MINIMUM: [u32; 5] = [0, 0, 0x80, 0x800, 0x10000];
    if !(MINIMUM[length]..=0x10FFFF).contains(&value) || (0xD800..=0xDFFF).contains(&value) {
        *at += 1;
        return 0xFFFD;
    }
    *at += length;
    value
}

fn append_hex(out: &mut String, value: u32, digits: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut shift = (digits as i32 - 1) * 4;
    while shift >= 0 {
        out.push(char::from(HEX[((value >> shift) & 0xF) as usize]));
        shift -= 4;
    }
}

/// The 22-character IfcGloballyUniqueId of a 128-bit value, `high` holding its
/// first eight bytes. The encoding is the one the IFC documentation gives for
/// IfcGloballyUniqueId: the first byte as two characters (so the first is
/// always 0 to 3), then each following three bytes as four, from the alphabet
/// 0-9 A-Z a-z _ $.
pub fn compress_guid(high: u64, low: u64) -> String {
    // Byte i of each half is its bits 56 - 8i up: big-endian order.
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&high.to_be_bytes());
    bytes[8..].copy_from_slice(&low.to_be_bytes());
    let mut out = String::with_capacity(22);
    let mut encode = |value: u32, characters: i32| {
        let mut i = characters - 1;
        while i >= 0 {
            out.push(char::from(
                GUID_ALPHABET[((value >> (6 * i)) & 0x3F) as usize],
            ));
            i -= 1;
        }
    };
    encode(u32::from(bytes[0]), 2);
    let mut i = 1;
    while i < 16 {
        encode(
            (u32::from(bytes[i]) << 16) | (u32::from(bytes[i + 1]) << 8) | u32::from(bytes[i + 2]),
            4,
        );
        i += 3;
    }
    out
}

/// A GlobalId for the object `key` names inside `space`, the same for the same
/// two strings every time. `space` is the project (ExportOptions::
/// guid_namespace), `key` what the object is in Katana - "entity/42",
/// "alignment/MC01", "utility/W-0001/segment/2" - so re-exporting a project
/// gives each object the GlobalId it had before, which is how an IFC
/// consumer recognises a changed object rather than a deleted one and a new
/// one. The 128 bits are two independent 64-bit FNV-1a hashes, each through
/// the SplitMix64 finaliser: not cryptographic, and not meant to be - the
/// chance of two of a million keys colliding is below 1e-26.
pub fn guid_for(space: &str, key: &str) -> String {
    // Two FNV-1a streams from different starting points over the same
    // bytes; the zero byte keeps ("ab", "c") and ("a", "bc") apart.
    let hash = |basis: u64| {
        let mut h = fnv1a(basis, space.as_bytes());
        h = fnv1a(h, b"\0");
        mix(fnv1a(h, key.as_bytes()))
    };
    compress_guid(hash(FNV_OFFSET_BASIS), hash(0x6A09_E667_F3BC_C908))
}

/// A string literal: quoted, a quote doubled, a backslash doubled, and every
/// character outside printable ASCII written as \X2\hhhh\X0\ (UTF-16, for the
/// Basic Multilingual Plane) or \X4\hhhhhhhh\X0\ (beyond it), the encodings
/// ISO 10303-21 gives for them. Invalid UTF-8 is written as U+FFFD, the
/// replacement character, rather than passed through as bytes a reader would
/// have to guess at.
pub fn step_string(utf8: impl AsRef<[u8]>) -> String {
    let utf8 = utf8.as_ref();
    let mut out = String::with_capacity(utf8.len() + 2);
    out.push('\'');
    let mut at = 0;
    let mut in_wide = false; // inside a \X2\ run
    while at < utf8.len() {
        let c = next_code_point(utf8, &mut at);
        if (0x20..0x7F).contains(&c) {
            if in_wide {
                out.push_str("\\X0\\");
                in_wide = false;
            }
            if c == u32::from(b'\'') {
                out.push_str("''");
            } else if c == u32::from(b'\\') {
                out.push_str("\\\\");
            } else {
                out.push(char::from(c as u8));
            }
        } else if c <= 0xFFFF {
            if !in_wide {
                out.push_str("\\X2\\");
                in_wide = true;
            }
            append_hex(&mut out, c, 4);
        } else {
            if in_wide {
                out.push_str("\\X0\\");
                in_wide = false;
            }
            out.push_str("\\X4\\");
            append_hex(&mut out, c, 8);
            out.push_str("\\X0\\");
        }
    }
    if in_wide {
        out.push_str("\\X0\\");
    }
    out.push('\'');
    out
}

/// A REAL: the shortest text that reads back as the same double, with the
/// decimal point the format requires ("1." not "1", "1.E-05" not "1e-05"),
/// and never "-0.". A non-finite value has no STEP form; the writer checks
/// for one before it gets here (see Args::real).
pub fn step_real(value: f64) -> String {
    if !value.is_finite() {
        return String::from("$");
    }
    if value == 0.0 {
        return String::from("0.");
    }
    // std::to_chars' shortest text, which core's exact real is.
    let text = format_exact_real(value);
    let e = text.find('e');
    let mut mantissa = String::from(match e {
        Some(at) => &text[..at],
        None => text.as_str(),
    });
    if !mantissa.contains('.') {
        mantissa.push('.');
    }
    match e {
        None => mantissa,
        Some(at) => format!("{mantissa}E{}", &text[at + 1..]),
    }
}

/// The UTF-8 of `c`; U+FFFD for a surrogate or a value past U+10FFFF.
fn append_utf8(out: &mut String, c: u32) {
    out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
}

fn hex_value(text: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    for &c in text {
        value <<= 4;
        match c {
            b'0'..=b'9' => value |= u32::from(c - b'0'),
            b'A'..=b'F' => value |= u32::from(c - b'A' + 10),
            b'a'..=b'f' => value |= u32::from(c - b'a' + 10),
            _ => return None,
        }
    }
    Some(value)
}

/// The inverse of step_string for the text between a literal's quotes: '' and
/// \\ undone, \X2\ (with surrogate pairs), \X4\, \X\hh and \S\c decoded to
/// UTF-8. Other writers put raw UTF-8 between the quotes, which is not the
/// standard's and is common: bytes that are valid UTF-8 are kept, and bytes
/// that are not are read as ISO 8859-1, the standard's default alphabet. A
/// malformed escape is U+FFFD.
pub fn decode_step_string(quoted: impl AsRef<[u8]>) -> String {
    let quoted = quoted.as_ref();
    let mut out = String::with_capacity(quoted.len());
    let mut at = 0;
    while at < quoted.len() {
        let c = quoted[at];
        if c == b'\'' {
            // '' is one quote; a lone one is the writer's mistake, kept.
            out.push('\'');
            at += if at + 1 < quoted.len() && quoted[at + 1] == b'\'' {
                2
            } else {
                1
            };
            continue;
        }
        if c != b'\\' {
            if c < 0x80 {
                out.push(char::from(c));
                at += 1;
                continue;
            }
            // Raw bytes: UTF-8 when they are, ISO 8859-1 when they are not.
            let mut probe = at;
            let decoded = next_code_point(quoted, &mut probe);
            if decoded == 0xFFFD && !quoted[at..].starts_with(b"\xEF\xBF\xBD") {
                append_utf8(&mut out, u32::from(c));
                at += 1;
            } else {
                // A whole well-formed sequence, which next_code_point checked.
                if let Ok(sequence) = std::str::from_utf8(&quoted[at..probe]) {
                    out.push_str(sequence);
                }
                at = probe;
            }
            continue;
        }
        let rest = &quoted[at..];
        if rest.starts_with(b"\\\\") {
            out.push('\\');
            at += 2;
        } else if rest.starts_with(b"\\X2\\") || rest.starts_with(b"\\X4\\") {
            let digits = if rest[2] == b'2' { 4 } else { 8 };
            let end = crate::support::find_from(rest, b"\\X0\\", 4);
            let Some(end) = end.filter(|end| (*end - 4).is_multiple_of(digits)) else {
                append_utf8(&mut out, 0xFFFD);
                at += 4;
                continue;
            };
            let mut pending_high: u32 = 0;
            let mut i = 4;
            while i < end {
                let unit = hex_value(&rest[i..i + digits]);
                i += digits;
                let Some(unit) = unit else {
                    append_utf8(&mut out, 0xFFFD);
                    continue;
                };
                if digits == 4 && (0xD800..=0xDBFF).contains(&unit) {
                    pending_high = unit;
                    continue;
                }
                if digits == 4 && (0xDC00..=0xDFFF).contains(&unit) && pending_high != 0 {
                    append_utf8(
                        &mut out,
                        0x10000 + ((pending_high - 0xD800) << 10) + (unit - 0xDC00),
                    );
                    pending_high = 0;
                    continue;
                }
                if pending_high != 0 {
                    append_utf8(&mut out, 0xFFFD);
                    pending_high = 0;
                }
                append_utf8(&mut out, unit);
            }
            if pending_high != 0 {
                append_utf8(&mut out, 0xFFFD);
            }
            at += end + 4;
        } else if rest.starts_with(b"\\X\\") && rest.len() >= 5 {
            let byte = hex_value(&rest[3..5]);
            append_utf8(&mut out, byte.unwrap_or(0xFFFD));
            at += 5;
        } else if rest.starts_with(b"\\S\\") && rest.len() >= 4 {
            // The upper half of the current alphabet, ISO 8859-1 unless a
            // \P\ switched it; the switch is not followed.
            append_utf8(&mut out, u32::from(rest[3]) + 128);
            at += 4;
        } else if rest.len() >= 4 && rest[1] == b'P' && rest[3] == b'\\' {
            at += 4; // \PA\ .. \PI\: which ISO 8859 part \S\ means
        } else {
            out.push('\\');
            at += 1;
        }
    }
    out
}

/// How many characters `utf8` has, for the IfcLabel and IfcIdentifier limit
/// of 255 (the schema's STRING(255) counts characters, not bytes).
pub fn character_count(utf8: impl AsRef<[u8]>) -> usize {
    let utf8 = utf8.as_ref();
    let mut count = 0;
    let mut at = 0;
    while at < utf8.len() {
        next_code_point(utf8, &mut at);
        count += 1;
    }
    count
}

/// The longest start of `utf8` that is at most `limit` characters, cut
/// between code points so that what is left is still valid UTF-8.
pub fn clip_characters(utf8: &str, limit: usize) -> String {
    let bytes = utf8.as_bytes();
    let mut count = 0;
    let mut at = 0;
    while at < bytes.len() && count < limit {
        next_code_point(bytes, &mut at);
        count += 1;
    }
    // Valid UTF-8 is read a whole character at a time, so `at` is on a
    // character's boundary.
    String::from(&utf8[..at])
}
