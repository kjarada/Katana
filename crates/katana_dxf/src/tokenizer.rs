//! DXF text as a stream of (group code, value) pairs.
//!
//! SPEED IS THE DESIGN. A DXF is two lines per pair, and a survey drawing is
//! millions of pairs, so nothing here allocates: a pair's value is a view into
//! the file, found by a byte search, and a number is parsed only when an entity
//! asks for it - the way the archive reader does it (docs/performance.md).
//! GDAL's reader built a feature per entity; this builds nothing.
//!
//! The text is the decoded file (reader.rs decodes before it tokenizes), so it
//! is UTF-8, and every view ends at a line end or an ASCII blank, which are
//! character boundaries.

use katana_core::text::parse_finite_double;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pair<'a> {
    pub code: i32,
    pub value: &'a str,
}

pub(crate) struct Tokenizer<'a> {
    text: &'a str,
    position: usize,
    line_number: usize,
    code_line: usize,
    pending: Option<Pair<'a>>,
    truncated: bool,
    failure: String,
}

impl<'a> Tokenizer<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            position: 0,
            line_number: 0,
            code_line: 0,
            pending: None,
            truncated: false,
            failure: String::new(),
        }
    }

    /// The next pair, skipping 999 comments. None at the end of the text, and
    /// when a group code line is not a number (failed() then says where).
    pub fn next_pair(&mut self) -> Option<Pair<'a>> {
        if let Some(pair) = self.pending.take() {
            return Some(pair);
        }
        if !self.failure.is_empty() {
            return None;
        }
        loop {
            let code_line = self.read_line()?;
            let code_text = trimmed_value(code_line);
            if code_text.is_empty() && self.position >= self.text.len() {
                return None; // a blank last line is only the file's final newline
            }
            let Some(code) = from_chars_integer(code_text)
                .and_then(|code| i32::try_from(code).ok())
                .filter(|code| (0..=1071).contains(code))
            else {
                // The first 40 bytes, as the C++ shows them. A character the
                // cut splits cannot be held by a String and is shown as U+FFFD.
                let shown = &code_line.as_bytes()[..code_line.len().min(40)];
                self.failure = format!(
                    "line {}: a group code was expected and \"{}\" was found",
                    self.line_number,
                    String::from_utf8_lossy(shown)
                );
                return None;
            };
            self.code_line = self.line_number;
            let Some(value) = self.read_line() else {
                self.truncated = true;
                return None;
            };
            if code == 999 {
                continue; // a comment
            }
            return Some(Pair { code, value });
        }
    }

    /// One pair of lookahead: the next call to next_pair() returns it again.
    pub fn push_back(&mut self, pair: Pair<'a>) {
        self.pending = Some(pair);
    }

    pub fn failed(&self) -> bool {
        !self.failure.is_empty()
    }

    pub fn failure(&self) -> &str {
        &self.failure
    }

    /// True when the text ended between a group code and its value.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// One-based line of the last group code read.
    #[allow(
        dead_code,
        reason = "ported with the tokenizer's interface; the reader does not name a line yet"
    )]
    pub fn line(&self) -> usize {
        self.code_line
    }

    /// The next line without its terminator, LF or CRLF. Lone CRs (classic
    /// Mac OS) are not line ends here: such a file reads as one line and fails
    /// as not a DXF, which is said, rather than being half understood.
    fn read_line(&mut self) -> Option<&'a str> {
        let text: &'a str = self.text;
        if self.position >= text.len() {
            return None;
        }
        let begin = self.position;
        let rest = &text.as_bytes()[begin..];
        let found = rest.iter().position(|&byte| byte == b'\n');
        let mut length = found.unwrap_or(rest.len());
        self.position += match found {
            Some(_) => length + 1,
            None => rest.len(),
        };
        if length != 0 && rest[length - 1] == b'\r' {
            length -= 1;
        }
        self.line_number += 1;
        Some(&text[begin..begin + length])
    }
}

/// Blanks off both ends: numbers are written right-aligned in their field.
pub(crate) fn trimmed_value(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\r'])
}

/// std::from_chars of an integer: an optional '-' and decimal digits, every
/// byte of them, and no '+' (which str::parse would take).
fn from_chars_integer(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// std::from_chars of a real takes no '+', which parse_finite_double strips.
fn from_chars_real(text: &str) -> Option<f64> {
    if text.starts_with('+') {
        return None;
    }
    parse_finite_double(text)
}

/// A value as a number; None when it is not one or is not finite. A leading
/// '+' is accepted, since some writers put one there.
pub(crate) fn to_real(value: &str) -> Option<f64> {
    let mut text = trimmed_value(value);
    if let Some(rest) = text.strip_prefix('+') {
        text = rest;
    }
    from_chars_real(text)
}

pub(crate) fn to_integer(value: &str) -> Option<i64> {
    let mut text = trimmed_value(value);
    if let Some(rest) = text.strip_prefix('+') {
        text = rest;
    }
    if let Some(integer) = from_chars_integer(text) {
        return Some(integer);
    }
    // Some writers put a real where an integer belongs ("1.0"); take it when
    // it is a whole number rather than lose the entity over it.
    let real = to_real(text)?;
    if real.trunc() == real && real.abs() < 9.0e15 {
        return Some(real as i64);
    }
    None
}

/// A handle: hexadecimal.
#[allow(
    dead_code,
    reason = "ported with the tokenizer's interface; the reader does not follow handles yet"
)]
pub(crate) fn to_handle(value: &str) -> Option<u64> {
    let text = trimmed_value(value);
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(text, 16).ok()
}
