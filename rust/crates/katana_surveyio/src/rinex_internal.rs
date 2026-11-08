//! Private to the RINEX reader: the column and line helpers the observation
//! reader (rinex.rs) and the navigation summary (rinex_navigation.rs) share,
//! so that "which columns hold the label" has one answer in both. Public, and
//! hidden from the documentation, only because the tests reach the Compact
//! RINEX expansion through it, as the C++ tests include the private header.
//!
//! RINEX is a Fortran-era fixed-column text format. Every header record carries
//! its label in columns 61-80 and its content in columns 1-60; data records have
//! their fields at fixed columns too. The helpers below take 1-based columns,
//! the way the specifications number them, so a field here can be checked
//! against the specification's table by eye.
//!
//! The column and text helpers are the C++ rinex_common.cpp; the expansion and
//! the navigation summary live in rinex_compact.rs and rinex_navigation.rs.

use std::collections::BTreeMap;

use katana_core::text::{equals_ignoring_case, parse_finite_double, parse_integer, trimmed};
use katana_core::text_encoding::{is_valid_utf8, utf8_or_windows1252};

use crate::format::ReadWarning;

pub use crate::rinex_compact::expand_compact_rinex;
pub use crate::rinex_navigation::summarise_navigation;

/// A damaged 200 MB file could otherwise produce a warning per epoch; past this
/// many the rest are counted in one closing warning rather than listed. One
/// limit for the observation reader and the Compact RINEX expansion together:
/// a compact file's damaged stretch can be 40 bytes, so a 1 GiB one could
/// otherwise hold millions of listed warnings.
pub const MAX_LISTED_WARNINGS: usize = 500;

/// Bytes from the file as text for a message, where the C++ puts the bytes
/// themselves: UTF-8 when they are, Windows-1252 when they are not.
pub(crate) fn raw_text(bytes: &[u8]) -> String {
    utf8_or_windows1252(bytes)
}

pub(crate) fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// Lines of a byte buffer, one at a time, without their terminators. LF and
/// CRLF both end a line: RINEX files are written on every operating system and
/// copied between them. (A lone CR is not a RINEX line end; it stays in the
/// line as a stray byte, which the record checks then report.)
#[derive(Clone, Debug)]
pub struct LineCursor<'a> {
    bytes: &'a [u8],
    position: usize,
    number: usize,
}

/// A saved place, to look at a line and step back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Mark {
    pub position: usize,
    pub number: usize,
}

impl<'a> LineCursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            number: 0,
        }
    }

    /// The next line, or None at the end of the bytes.
    #[allow(
        clippy::should_implement_trait,
        reason = "The C++ cursor's next(); it steps back with reset(), which an iterator does not."
    )]
    pub fn next(&mut self) -> Option<&'a [u8]> {
        if self.position >= self.bytes.len() {
            return None;
        }
        let begin = &self.bytes[self.position..];
        let found = begin.iter().position(|&c| c == b'\n');
        let mut length = found.unwrap_or(begin.len());
        self.position += if found.is_some() { length + 1 } else { length };
        if length > 0 && begin[length - 1] == b'\r' {
            length -= 1;
        }
        self.number += 1;
        Some(&begin[..length])
    }

    /// The 1-based number of the line next() returned last.
    pub fn line_number(&self) -> usize {
        self.number
    }

    pub fn at_end(&self) -> bool {
        self.position >= self.bytes.len()
    }

    pub fn mark(&self) -> Mark {
        Mark {
            position: self.position,
            number: self.number,
        }
    }

    pub fn reset(&mut self, mark: Mark) {
        self.position = mark.position;
        self.number = mark.number;
    }
}

/// Columns `first` (1-based) to `first + width - 1` of `line`, clipped to the
/// line: a record whose trailing blanks were removed (the specifications allow
/// it) simply has shorter fields.
pub fn columns(line: &[u8], first: usize, width: usize) -> &[u8] {
    if first == 0 || first > line.len() {
        return &[];
    }
    let start = first - 1;
    &line[start..line.len().min(start + width)]
}

/// The header label: columns 61-80, trailing blanks removed.
pub fn header_label(line: &[u8]) -> &[u8] {
    if line.len() <= 60 {
        return &[];
    }
    let mut label = &line[60..line.len().min(80)];
    while let Some((&last, rest)) = label.split_last() {
        if last != b' ' && last != b'\t' {
            break;
        }
        label = rest;
    }
    label
}

/// The header content: columns 1-60.
pub fn header_content(line: &[u8]) -> &[u8] {
    &line[..line.len().min(60)]
}

/// A Fortran I field: blanks around it allowed, nothing else. None when blank
/// or not an integer.
pub fn integer_field(field: &[u8]) -> Option<i64> {
    let text = trimmed(field);
    if text.is_empty() {
        return None;
    }
    parse_integer(text)
}

/// A Fortran F field (".9030", "-.353", "4375274.0000"). None when blank or
/// not a finite number. A 'D' exponent (Fortran double precision, common in
/// navigation files) is read as 'E'.
pub fn real_field(field: &[u8]) -> Option<f64> {
    let text = trimmed(field);
    if text.is_empty() {
        return None;
    }
    // Fortran writes double precision exponents with a D ("0.123D-04"). A
    // field is at most a few dozen characters; anything longer is not a number.
    if text.iter().any(|&c| c == b'D' || c == b'd') {
        if text.len() >= 64 {
            return None;
        }
        let buffer: Vec<u8> = text
            .iter()
            .map(|&c| if c == b'D' || c == b'd' { b'E' } else { c })
            .collect();
        return parse_finite_double(buffer);
    }
    parse_finite_double(text)
}

/// `text` as something safe to keep and show: surrounding blanks trimmed,
/// control characters replaced by blanks and, when the bytes are not valid
/// UTF-8 (a header typed on an instrument with its own code page), every byte
/// above 127 replaced by '?'. `changed` is set when anything but trimming
/// happened, so the caller can say so once.
pub fn clean_text(text: &[u8], changed: Option<&mut bool>) -> String {
    let mut clean = trimmed(text).to_vec();
    let mut altered = false;
    for c in &mut clean {
        if *c < 0x20 || *c == 0x7F {
            *c = b' ';
            altered = true;
        }
    }
    if !is_valid_utf8(&clean) {
        for c in &mut clean {
            if *c >= 0x80 {
                *c = b'?';
            }
        }
        altered = true;
    }
    if altered && let Some(changed) = changed {
        *changed = true;
    }
    // Valid UTF-8 by now, or nothing but ASCII; the lossy branch is never
    // taken, and is there so that no input can panic.
    String::from_utf8(clean)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

/// Whitespace-separated tokens of `text`.
pub fn tokens(text: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < text.len() {
        while at < text.len() && (text[at] == b' ' || text[at] == b'\t') {
            at += 1;
        }
        let start = at;
        while at < text.len() && text[at] != b' ' && text[at] != b'\t' {
            at += 1;
        }
        if at > start {
            found.push(&text[start..at]);
        }
    }
    found
}

/// The satellite system a RINEX system letter names, in the words
/// GnssSession::satellites_per_system is keyed by; None for a letter that
/// names none.
pub fn system_name(letter: u8) -> Option<&'static str> {
    // RINEX 3.05 section 3.5 (satellite numbers) and Table A2 (RINEX VERSION /
    // TYPE); 'T' is the Transit system of RINEX 2.10, long gone but still a
    // letter an old file may carry.
    match letter {
        b'G' => Some("GPS"),
        b'R' => Some("GLONASS"),
        b'E' => Some("Galileo"),
        b'C' => Some("BeiDou"),
        b'J' => Some("QZSS"),
        b'I' => Some("NavIC"),
        b'S' => Some("SBAS"),
        b'T' => Some("Transit"),
        _ => None,
    }
}

/// The first line of a RINEX file, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VersionRecord {
    /// "3.04", as written (trimmed).
    pub version_text: String,
    pub version: f64,
    pub major: i32,
    /// Column 21: 'O', 'N', 'G', 'M', ...
    pub file_type: u8,
    /// Column 41: 'G', 'R', 'M', ... (blank in old files).
    pub system: u8,
    /// Columns 21-40, trimmed: "OBSERVATION DATA".
    pub type_text: String,
}

/// None when the line is not a RINEX VERSION / TYPE record.
pub fn version_record(first_line: &[u8]) -> Option<VersionRecord> {
    if header_label(first_line) != b"RINEX VERSION / TYPE" {
        return None;
    }
    let version_field = columns(first_line, 1, 9);
    // A number real_field read, so ASCII.
    let version_text = raw_text(trimmed(version_field));
    let version = real_field(version_field)?;
    if !(1.0..100.0).contains(&version) {
        return None;
    }
    Some(VersionRecord {
        version_text,
        version,
        major: version.floor() as i32,
        file_type: first_line.get(20).copied().unwrap_or(b' '),
        system: first_line.get(40).copied().unwrap_or(b' '),
        type_text: clean_text(columns(first_line, 21, 20), None),
    })
}

// ---- Compression, which is refused rather than guessed at --------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Packing {
    #[default]
    Plain,
    /// 1F 8B: .gz
    Gzip,
    /// 1F 9D: .Z, the classic IGS archive packing.
    UnixCompress,
    /// "BZh"
    Bzip2,
    /// "PK\3\4"
    Zip,
    /// Compact RINEX ("CRINEX VERS   / TYPE"), .crx / .YYd.
    Hatanaka,
}

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

pub fn packing_of(bytes: &[u8]) -> Packing {
    let byte_at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    if byte_at(0) == 0x1F && byte_at(1) == 0x8B {
        return Packing::Gzip;
    }
    if byte_at(0) == 0x1F && byte_at(1) == 0x9D {
        return Packing::UnixCompress;
    }
    if bytes.starts_with(b"BZh") {
        return Packing::Bzip2;
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Packing::Zip;
    }
    // Compact RINEX puts its own version record first ("1.0" or "3.0" in
    // columns 1-20, "COMPACT RINEX FORMAT" in 21-40); the RINEX header it
    // wraps follows from the third line.
    let end = bytes.iter().position(|&c| c == b'\n');
    let mut first = &bytes[..end.unwrap_or(bytes.len())];
    if let Some(rest) = first.strip_suffix(b"\r") {
        first = rest;
    }
    if header_label(first) == b"CRINEX VERS   / TYPE" || contains(first, b"COMPACT RINEX FORMAT") {
        return Packing::Hatanaka;
    }
    Packing::Plain
}

/// The sentence a person is told for a packed file: what it is and what to do.
pub fn packing_refusal(packing: Packing, file_name: &str) -> String {
    match packing {
        Packing::Gzip => format!(
            "{file_name} is gzip-compressed. Decompress it first (7-Zip, or gzip -d) and import \
             the RINEX file inside it."
        ),
        Packing::UnixCompress => format!(
            "{file_name} is Unix-compressed (.Z). Decompress it first (7-Zip, or gzip -d) and \
             import the RINEX file inside it."
        ),
        Packing::Bzip2 => format!(
            "{file_name} is bzip2-compressed. Decompress it first (7-Zip, or bzip2 -d) and \
             import the RINEX file inside it."
        ),
        Packing::Zip => format!(
            "{file_name} is a zip archive. Extract it first and import the RINEX observation \
             file inside it."
        ),
        Packing::Hatanaka => format!(
            "{file_name} is Hatanaka-compressed (Compact RINEX, .crx or .YYd), which only ever \
             holds observation data; import it as the observation file."
        ),
        Packing::Plain => format!("{file_name} is not compressed."),
    }
}

// ---- Compact RINEX (Hatanaka), which is expanded as it is read ---------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Run {
    expanded_first: usize,
    compact_first: usize,
    expanded_per_compact: usize,
}

/// Which line of the compact file each line of its expansion came from, so a
/// warning about the expansion names a line the person can find in the file
/// they have. Stored as runs, not per line: a run is a first expanded line, the
/// compact line it came from, and how many expanded lines each compact line
/// became from there on (1 for a satellite record, 2 or more for a RINEX 2
/// satellite whose observations wrap, 0 for "every line of the run came from
/// the same compact line" - an epoch line and its continuation lines).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineMap {
    runs: Vec<Run>,
}

impl LineMap {
    pub fn add(
        &mut self,
        expanded_first: usize,
        compact_first: usize,
        expanded_per_compact: usize,
    ) {
        if let Some(last) = self.runs.last()
            && last.expanded_per_compact == expanded_per_compact
            && expanded_first >= last.expanded_first
        {
            let offset = expanded_first - last.expanded_first;
            let predicted = if expanded_per_compact == 0 {
                compact_first == last.compact_first
            } else {
                offset.is_multiple_of(expanded_per_compact)
                    && last.compact_first + offset / expanded_per_compact == compact_first
            };
            if predicted {
                return;
            }
        }
        self.runs.push(Run {
            expanded_first,
            compact_first,
            expanded_per_compact,
        });
    }

    /// The compact line for 1-based expanded line `expanded`; 0 stays 0 (the
    /// file as a whole).
    pub fn compact_line(&self, expanded: usize) -> usize {
        if expanded == 0 || self.runs.is_empty() {
            return expanded;
        }
        // The first run that starts after the line: std::upper_bound.
        let after = self
            .runs
            .partition_point(|run| run.expanded_first <= expanded);
        if after == 0 {
            return expanded;
        }
        let run = &self.runs[after - 1];
        if run.expanded_per_compact == 0 {
            return run.compact_first;
        }
        run.compact_first + (expanded - run.expanded_first) / run.expanded_per_compact
    }

    pub fn empty(&self) -> bool {
        self.runs.is_empty()
    }
}

/// A Compact RINEX file expanded to the RINEX observation file it stands for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompactExpansion {
    /// The RINEX observation file, as bytes: the compact file's bytes, expanded.
    pub text: Vec<u8>,
    /// "Compact RINEX 3.0, written by RNX2CRX ver.4.1.0 on ..."
    pub description: String,
    /// Expanded line -> compact line.
    pub lines: LineMap,
    /// What could not be expanded, by compact line; the rest of the file is
    /// still expanded from the next epoch that starts afresh.
    pub warnings: Vec<ReadWarning>,
    /// Past MAX_LISTED_WARNINGS: counted, not listed.
    pub unlisted_warnings: usize,
    /// Lines in the compact file.
    pub compact_lines: usize,
    /// Compact lines that could not be expanded.
    pub lines_skipped: usize,
    /// Compact RINEX 3 optional ('&') records, skipped.
    pub optional_lines: usize,
}

// ---- Navigation files -----------------------------------------------------------

/// The navigation files that conventionally sit beside an observation file
/// called `observation_file_name`, most likely first: the short-name siblings
/// (site2560.24o -> site2560.24n, .24g, ...), the long-name ones
/// (..._01D_30S_MO.rnx -> ..._01D_MN.rnx, _GN, _RN, ...), or "stem.nav".
pub fn navigation_file_candidates(observation_file_name: &str) -> Vec<String> {
    let mut names = Vec::new();
    let dot = match observation_file_name.rfind('.') {
        Some(dot) if dot != 0 => dot,
        _ => return names,
    };
    let stem = &observation_file_name[..dot];
    let extension = &observation_file_name[dot + 1..];
    let ext = extension.as_bytes();

    // Short names (RINEX 2, and RINEX 3 files named the old way): ssssdddf.yyo
    // (or .yyd, Hatanaka-compressed) beside ssssdddf.yyn (GPS), .yyg
    // (GLONASS), .yyl (Galileo), .yyh (SBAS) and .yyp (mixed, the RINEX 3
    // short-name convention).
    if ext.len() == 3
        && is_digit(ext[0])
        && is_digit(ext[1])
        && matches!(ext[2], b'o' | b'O' | b'd' | b'D')
    {
        let upper = ext[2] == b'O' || ext[2] == b'D';
        for &kind in b"nglhp" {
            let mut name = String::from(stem);
            name.push('.');
            // Two ASCII digits.
            name.push_str(&extension[..2]);
            name.push(char::from(if upper {
                kind.to_ascii_uppercase()
            } else {
                kind
            }));
            names.push(name);
        }
        return names;
    }

    // Long names (RINEX 3 and 4, Table A1): SSSSMRCCC_K_YYYYDDDHHMM_PPU_FRU_xO.rnx
    // (or .crx, Hatanaka-compressed) beside SSSSMRCCC_K_YYYYDDDHHMM_PPU_xN.rnx -
    // a navigation file has no observation frequency part, and is never
    // Hatanaka-compressed (the compression is defined for observations only).
    let compact = equals_ignoring_case(ext, b"crx");
    if compact || equals_ignoring_case(ext, b"rnx") {
        let parts: Vec<&str> = stem.split('_').collect();
        if parts.len() == 6
            && parts[5].len() == 2
            && (parts[5].as_bytes()[1] == b'O' || parts[5].as_bytes()[1] == b'o')
        {
            let mut base = String::new();
            for part in &parts[..4] {
                base.push_str(part);
                base.push('_');
            }
            let navigation_extension = if compact {
                if ext[0] == b'C' { "RNX" } else { "rnx" }
            } else {
                extension
            };
            for &system in b"MGRECJIS" {
                names.push(format!(
                    "{base}{}N.{navigation_extension}",
                    char::from(system)
                ));
            }
            return names;
        }
    }

    // Anything else: the name converters such as RTKLIB's give a mixed
    // navigation file written beside an observation file.
    names.push(format!("{stem}.nav"));
    names
}

/// What a broadcast navigation file holds, counted rather than stored: without a
/// positioning step the orbits themselves have no use in the survey model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavigationSummary {
    pub file_name: String,
    pub version_text: String,
    /// Broadcast ephemeris records per system name ("GPS", "GLONASS", ...).
    pub ephemerides: BTreeMap<String, usize>,
    /// RINEX 4 STO / EOP / ION records.
    pub other_messages: usize,
    pub ionosphere_parameters: bool,
    pub leap_seconds: String,
    pub lines: usize,
    pub warnings: Vec<ReadWarning>,
}
