//! What the C++ of this layer takes from its language and library, where
//! Rust's std answers otherwise: `printf`'s "%.3f", ASCII-only case and
//! trimming, the text of a path and of an I/O error. Each is a few lines; none
//! is a design of its own. std::min, std::max, std::clamp and std::remainder
//! are katana_math::cpp's, the C library's asinh katana_math::libm's.

use std::path::Path;

use katana_core::path_text::path_to_utf8;
use katana_core::text::is_ascii_space;

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
