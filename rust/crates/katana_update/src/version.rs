//! Versions as the release workflow writes them (.github/workflows/release.yml reads
//! project(Katana VERSION) with sed on digits and dots only): MAJOR.MINOR, MAJOR.MINOR.PATCH or
//! MAJOR.MINOR.PATCH.BUILD, each component digits.
//!
//! Compared component by component as integers, never as text ("0.10" is newer than "0.9"). A
//! missing trailing component is zero, so "0.3" and "0.3.0" are the same version; how many were
//! written is kept only so that [`to_string`] gives the text back with as many components (a
//! leading zero is not kept).
//!
//! There is no pre-release syntax ("-beta") on purpose: the workflow cannot produce one, and a
//! version the updater cannot order is one it must not offer.
//!
//! The text comes from a signed manifest and from this program's own build, so one parser holds
//! both to the same rule.

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use std::cmp::Ordering;

const MIN_COMPONENTS: usize = 2;
const MAX_COMPONENTS: usize = 4;
// 999,999,999 is the largest nine-digit number and is below 2^32 (4,294,967,296), so a component
// of at most nine digits cannot overflow the u32 it is stored in, and there is no arithmetic to
// wrap: the length is the overflow check.
const MAX_DIGITS: usize = 9;
// A valid version is at most 4 * 9 digits and 3 dots, 39 bytes; an error quotes no more than a
// little over that, so a hostile megabyte is not copied into a message.
const EXCERPT_BYTES: usize = 48;

/// A version: equal and ordered by its four components alone, whatever `written` says.
#[derive(Clone, Copy, Debug)]
pub struct Version {
    pub components: [u32; 4],
    /// 2 to 4: how many components the text had.
    pub written: u8,
}

impl Default for Version {
    fn default() -> Self {
        Self {
            components: [0; 4],
            written: 2,
        }
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.components.cmp(&other.components)
    }
}

// The text for an error context: bounded, and every byte that is not printable ASCII (a NUL, a
// newline, a byte of a UTF-8 sequence) shown as '?', so a log line stays one line.
fn excerpt(text: &str) -> String {
    let bytes = text.as_bytes();
    let shown = bytes.len().min(EXCERPT_BYTES);
    let mut out = String::with_capacity(shown + 3);
    for &byte in &bytes[..shown] {
        out.push(if (0x20..=0x7e).contains(&byte) {
            char::from(byte)
        } else {
            '?'
        });
    }
    if bytes.len() > shown {
        out.push_str("...");
    }
    out
}

fn refuse(why: &str, text: &str) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        format!("not a version (MAJOR.MINOR[.PATCH[.BUILD]], digits): {why}"),
        excerpt(text),
    )
}

/// 2 to 4 components separated by single dots, each 1 to 9 ASCII digits (so it fits 32 bits), no
/// sign, no space, no empty component, leading zeros read as decimal ("1.02" is 1.2).
/// ParseFailure otherwise; the context is the text.
pub fn parse_version(text: &str) -> Result<Version> {
    if text.is_empty() {
        return Err(refuse("empty", text));
    }

    let bytes = text.as_bytes();
    let mut version = Version::default();
    let mut count = 0usize;
    let mut digits = 0usize;
    let mut value = 0u32;

    // One pass over the bytes; a component ends at a dot or at the end of the text. Only ASCII
    // digits and dots are accepted, so a sign, a space, a letter ("1.2-beta", "v1.2"), a
    // non-ASCII digit or a NUL is refused wherever it stands.
    for byte in bytes.iter().copied().map(Some).chain(std::iter::once(None)) {
        let at_end = byte.is_none();
        let c = byte.unwrap_or(b'.');
        if c.is_ascii_digit() {
            digits += 1;
            if digits > MAX_DIGITS {
                return Err(refuse("a component of more than nine digits", text));
            }
            value = value * 10 + u32::from(c - b'0');
        } else if c == b'.' {
            if digits == 0 {
                return Err(refuse("an empty component", text));
            }
            if count == MAX_COMPONENTS {
                return Err(refuse("more than four components", text));
            }
            version.components[count] = value;
            count += 1;
            digits = 0;
            value = 0;
            if at_end {
                break;
            }
        } else {
            return Err(refuse("a character other than a digit or a dot", text));
        }
    }

    if count < MIN_COMPONENTS {
        return Err(refuse("fewer than two components", text));
    }
    version.written = count as u8;
    Ok(version)
}

/// The text as written: `written` components, joined by dots.
pub fn to_string(version: &Version) -> String {
    // `written` is 2 to 4 for anything parse_version made; a hand-built Version with another
    // number still gives text that parses, rather than an out-of-range read.
    let count = usize::from(version.written).clamp(MIN_COMPONENTS, MAX_COMPONENTS);
    let mut out = String::new();
    for (i, component) in version.components[..count].iter().enumerate() {
        if i != 0 {
            out.push('.');
        }
        out.push_str(&format!("{component}"));
    }
    out
}

/// The version this program was built as: project(Katana VERSION ...), read by build.rs from the
/// checkout's CMakeLists.txt, where the C++ build passes it as KATANA_VERSION.
pub fn program_version_text() -> &'static str {
    env!("KATANA_VERSION")
}

/// parse_version of [`program_version_text`]. It cannot fail in a correct build; a failure is
/// Internal.
pub fn program_version() -> Result<Version> {
    detail::program_version_of(program_version_text())
}

#[doc(hidden)]
pub mod detail {
    use super::{Version, excerpt, parse_version};
    use katana_core::error::make_error;
    use katana_core::{ErrorCode, Result};

    /// program_version for any text, so that the refusal can be tested without a broken build
    /// (tests/version.rs; it is in no public interface of the C++).
    pub fn program_version_of(text: &str) -> Result<Version> {
        match parse_version(text) {
            Ok(version) => Ok(version),
            // The text is the project's own, not input: a refusal here is a broken build, not a
            // bad file, so it is Internal.
            Err(error) => Err(make_error(
                ErrorCode::Internal,
                format!(
                    "the program's own version is not MAJOR.MINOR[.PATCH[.BUILD]]: {}",
                    error.message
                ),
                excerpt(text),
            )),
        }
    }
}
