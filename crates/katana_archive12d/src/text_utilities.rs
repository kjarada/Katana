//! Small text helpers shared by the reader, the writer and the domain mapping.
//! Internal to the crate.
//!
//! The locale-independent helpers every text reader shares are katana_core's
//! (`katana_core::text`); see there for why they must not consult a locale.

use katana_core::text::{format_fixed, is_ascii_space, parse_integer};

/// A token folded to lower case, as the format compares keywords (manual 1.1:
/// case is stored but not compared). The C++ folds into a buffer on the stack
/// to spare an allocation per keyword; the fold itself is the same ASCII fold.
pub(crate) fn lower(text: &str) -> String {
    text.to_ascii_lowercase()
}

pub(crate) fn equals_ignoring_case(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

/// `text` without the ASCII white space at either end (core's `trimmed`, on
/// text that is already UTF-8: what is cut is ASCII, so what is left still is).
pub(crate) fn trimmed(text: &str) -> &str {
    text.trim_matches(|character: char| u8::try_from(character).is_ok_and(is_ascii_space))
}

/// The manual's rule (1.5.8): true is 1, or a word starting with T, t, Y or
/// y; false is 0, or a word starting with F, f, N or n. The writing program
/// also writes 2 for "visible" in some per-vertex flags, and any other
/// non-zero integer is read as true for the same reason.
pub(crate) fn parse_boolean(token: &str) -> Option<bool> {
    let first = *token.as_bytes().first()?;
    if let Some(number) = parse_integer(token) {
        return Some(number != 0);
    }
    match first {
        b'T' | b't' | b'Y' | b'y' => Some(true),
        b'F' | b'f' | b'N' | b'n' => Some(false),
        _ => None,
    }
}

/// A real with at most `decimal_places` digits after the point and trailing
/// zeros removed: 502000, 30.45, -0.001. Never an exponent: the writing
/// program reads them, but a coordinate written as 5.02e+05 is unreadable to a
/// person, and a .12da is a file people open.
pub(crate) fn format_real(value: f64, decimal_places: i32) -> String {
    // std::to_chars fixed at the clamped precision; core's formatFixed is that
    // text, with the sign of a value that rounded to zero dropped - which the
    // C++ does too, below, once the zeros are trimmed.
    let mut text = format_fixed(value, decimal_places.clamp(0, 17));
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    // -0.0004 at three places rounds to "-0", which is a zero with a sign no
    // reader wants.
    if text == "-0" {
        text = String::from("0");
    }
    text
}

/// C99 hexadecimal float, `0x1.6d4014ac08318p+17`: exact for every double.
/// The digits are libstdc++'s std::to_chars(chars_format::hex) of the
/// magnitude - the shortest exact form, the leading digit 1 for a normal
/// number and 0 for a subnormal one (whose exponent is then -1022), trailing
/// zero digits dropped and the point with them, the exponent signed - and the
/// sign is written before the 0x.
pub(crate) fn format_hex_real(value: f64) -> String {
    let negative = value.is_sign_negative();
    let mut text = String::from(if negative { "-0x" } else { "0x" });
    text.push_str(&hex_digits(value.abs()));
    text
}

fn hex_digits(magnitude: f64) -> String {
    if magnitude.is_nan() {
        return String::from("nan");
    }
    if magnitude.is_infinite() {
        return String::from("inf");
    }
    if magnitude == 0.0 {
        return String::from("0p+0");
    }
    let bits = magnitude.to_bits();
    let biased = (bits >> 52) as i32;
    let mantissa = bits & ((1_u64 << 52) - 1);
    let (leading, exponent) = if biased == 0 {
        (0, -1022)
    } else {
        (1, biased - 1023)
    };
    let mut hexits = format!("{mantissa:013x}");
    while hexits.ends_with('0') {
        hexits.pop();
    }
    let mut out = format!("{leading}");
    if !hexits.is_empty() {
        out.push('.');
        out.push_str(&hexits);
    }
    out.push('p');
    if exponent >= 0 {
        out.push('+');
    }
    out.push_str(&format!("{exponent}"));
    out
}

/// The hexadecimal significand and exponent that follow `0x`, as libstdc++'s
/// std::from_chars(chars_format::hex) reads them: an optional minus, hex
/// digits with at most one point, then an optional `p` exponent (a `+`, then a
/// decimal int, which may itself carry a minus). The value is rounded to the
/// nearest double, ties to even. None for anything from_chars refuses, or that
/// leaves text unread, and for a result out of range (which from_chars reports
/// as result_out_of_range rather than a value): an overflow, and an underflow
/// of a nonzero significand to zero.
pub(crate) fn parse_hex_significand(text: &[u8]) -> Option<f64> {
    let mut at = 0;
    let negative = text.first() == Some(&b'-');
    if negative {
        at += 1;
    }
    // "inf" and "nan" are spelt out in the hex format too; neither is a
    // number here, whatever follows.
    if matches!(text.get(at), Some(b'i' | b'I' | b'n' | b'N')) {
        return None;
    }
    let mut seen_hexit = false;
    while text.get(at) == Some(&b'0') {
        seen_hexit = true;
        at += 1;
    }
    // The significant digits, at most sixteen of them (64 bits: eleven more
    // than a double holds, so the rounding below sees its rounding bit), the
    // rest only as whether any is nonzero.
    let mut mantissa: u64 = 0;
    let mut digits = 0;
    let mut sticky = false;
    let mut binary_exponent: i64 = 0;
    let mut seen_point = false;
    while let Some(&byte) = text.get(at) {
        if byte == b'.' && !seen_point {
            seen_point = true;
            at += 1;
            continue;
        }
        let Some(hexit) = char::from(byte).to_digit(16) else {
            break;
        };
        seen_hexit = true;
        at += 1;
        if mantissa == 0 && hexit == 0 {
            // A zero before the first significant digit, after the point.
            binary_exponent -= 4;
            continue;
        }
        if digits < 16 {
            mantissa = mantissa * 16 + u64::from(hexit);
            digits += 1;
            if seen_point {
                binary_exponent -= 4;
            }
        } else {
            sticky |= hexit != 0;
            if !seen_point {
                binary_exponent += 4;
            }
        }
    }
    if !seen_hexit {
        return None;
    }
    let mut written_exponent: i64 = 0;
    if matches!(text.get(at), Some(b'p' | b'P')) {
        let mut cursor = at + 1;
        if text.get(cursor) == Some(&b'+') {
            cursor += 1;
        }
        let start = cursor;
        if text.get(cursor) == Some(&b'-') {
            cursor += 1;
        }
        let digits_start = cursor;
        while text.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        // A `p` with no number after it is not consumed, and then it is text
        // left unread.
        if cursor > digits_start {
            let exponent: i32 = std::str::from_utf8(&text[start..cursor])
                .ok()?
                .parse()
                .ok()?;
            written_exponent = i64::from(exponent);
            at = cursor;
        }
    }
    if at != text.len() {
        return None;
    }
    if mantissa == 0 {
        return Some(if negative { -0.0 } else { 0.0 });
    }
    let magnitude = compose(mantissa, binary_exponent + written_exponent, sticky)?;
    Some(if negative { -magnitude } else { magnitude })
}

/// `mantissa` x 2^`exponent` (with `sticky` set when digits beyond the
/// mantissa were nonzero), rounded to the nearest double, ties to even.
fn compose(mantissa: u64, exponent: i64, sticky: bool) -> Option<f64> {
    let bits = i64::from(64 - mantissa.leading_zeros());
    let top = bits - 1 + exponent;
    if top > 1023 {
        return None;
    }
    if top >= -1022 {
        let shift = bits - 53;
        let mut significand = if shift > 0 {
            shifted_right(mantissa, shift as u32, sticky)
        } else {
            mantissa << ((-shift) as u32)
        };
        let mut top = top;
        if significand == 1_u64 << 53 {
            significand >>= 1;
            top += 1;
            if top > 1023 {
                return None;
            }
        }
        let biased = (top + 1023) as u64;
        return Some(f64::from_bits(
            (biased << 52) | (significand & ((1_u64 << 52) - 1)),
        ));
    }
    // Subnormal: the significand counts units of 2^-1074.
    let shift = -(exponent + 1074);
    let significand = if shift <= 0 {
        mantissa << ((-shift) as u32)
    } else if shift > 64 {
        0
    } else {
        shifted_right(mantissa, shift as u32, sticky)
    };
    if significand == 0 {
        return None;
    }
    // 2^52 units is the smallest normal, whose bits are the same number.
    Some(f64::from_bits(significand))
}

/// `value` >> `shift` (1 to 64), rounded to nearest, ties to even.
fn shifted_right(value: u64, shift: u32, sticky: bool) -> u64 {
    let wide = u128::from(value);
    let quotient = wide >> shift;
    let remainder = wide & ((1_u128 << shift) - 1);
    let half = 1_u128 << (shift - 1);
    let round_up = remainder > half || (remainder == half && (sticky || quotient & 1 == 1));
    (quotient + u128::from(round_up)) as u64
}
