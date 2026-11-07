//! What the C++ of this layer takes from its language and library, where
//! Rust's std answers otherwise: the IEEE remainder, `std::clamp`'s order of
//! tests, `printf`'s "%.3f", ASCII-only case and trimming, the text of a path
//! and of an I/O error. Each is a few lines; none is a design of its own.

use std::path::Path;

use katana_core::path_text::path_to_utf8;
use katana_core::text::is_ascii_space;

/// `std::remainder(x, p)`: x - n p, n the integer nearest x / p (an even one
/// on a tie), which is exact. fdlibm's `__ieee754_remainder`, step for step,
/// on top of the exact `fmod` Rust's `%` is.
pub(crate) fn remainder(x: f64, p: f64) -> f64 {
    // The high word of |p|, as fdlibm tests it.
    let high_p = (p.to_bits() >> 32) as u32 & 0x7fff_ffff;
    if p == 0.0 || !x.is_finite() || p.is_nan() {
        #[allow(
            clippy::eq_op,
            reason = "fdlibm's own way to make the NaN of an invalid operation, kept for its bits."
        )]
        return (x * p) / (x * p);
    }
    let negative = x.is_sign_negative();
    let p = p.abs();
    let mut r = x;
    // Below 2^1023 p + p is finite: now |r| < 2p.
    if high_p <= 0x7fdf_ffff {
        r %= p + p;
    }
    if x.abs() == p {
        return 0.0 * x;
    }
    r = r.abs();
    if high_p < 0x0020_0000 {
        // p below 2^-1021: halving it could lose its last bit.
        if r + r > p {
            r -= p;
            if r + r >= p {
                r -= p;
            }
        }
    } else {
        let half = 0.5 * p;
        if r > half {
            r -= p;
            if r >= half {
                r -= p;
            }
        }
    }
    // fdlibm flips the sign bit for a negative x, whatever r's own sign is.
    if negative { -r } else { r }
}

/// `std::clamp(value, low, high)`: `value < low ? low : high < value ? high :
/// value`, which never panics (Rust's `f64::clamp` panics on a NaN bound or
/// on low > high, where the C++ is merely unspecified).
pub(crate) fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value < low {
        low
    } else if high < value {
        high
    } else {
        value
    }
}

/// `std::min(a, b)`: `b < a ? b : a`, which keeps `a` on a NaN where
/// `f64::min` would not.
pub(crate) fn min_of(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`: `a < b ? b : a`.
pub(crate) fn max_of(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `snprintf(buffer, size, "%.<decimals>f", value)`: the exact decimal
/// expansion rounded to `decimals` places, its sign kept ("-0.000" stays, as
/// it does in C; core's format_fixed drops it). glibc's spellings of the
/// values that are not finite.
pub(crate) fn printf_fixed(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        return String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        });
    }
    if value.is_infinite() {
        return String::from(if value < 0.0 { "-inf" } else { "inf" });
    }
    format!("{value:.decimals$}")
}

/// `text` without the ASCII blanks at either end (core::trimmed's set), never
/// Unicode's wider idea of white space that `str::trim` has.
pub(crate) fn trimmed(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_ascii() && is_ascii_space(c as u8))
}

/// ASCII upper case, every other byte as it is: `std::toupper` in the C
/// locale, byte by byte.
pub(crate) fn upper(text: &str) -> String {
    text.to_ascii_uppercase()
}

/// ASCII lower case: core::lowered.
pub(crate) fn lower(text: &str) -> String {
    text.to_ascii_lowercase()
}

/// The first `needle` in `haystack` at or after `from`, by bytes.
pub(crate) fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    if needle.is_empty() {
        return Some(from);
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

/// Bytes that are UTF-8, as text; a byte that is not becomes U+FFFD, which is
/// what the writer's string encoding makes of it anyway (step.rs).
pub(crate) fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

/// `path.string()`: the path's own bytes, as text.
pub(crate) fn path_text(path: &Path) -> String {
    text_of(path_to_utf8(path))
}

/// `path.filename().string()`; empty for a path with no file name.
pub(crate) fn file_name_text(path: &Path) -> String {
    path.file_name()
        .map(|name| path_text(Path::new(name)))
        .unwrap_or_default()
}

/// `std::error_code::message()`: the system's words for the error. Rust's
/// text of an operating system error is those words followed by
/// " (os error N)", which is dropped.
pub(crate) fn error_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error() {
        let suffix = format!(" (os error {code})");
        if let Some(words) = text.strip_suffix(&suffix) {
            return String::from(words);
        }
    }
    text
}
