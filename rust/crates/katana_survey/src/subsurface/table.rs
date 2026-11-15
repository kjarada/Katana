//! The delimited text both subsurface readers share: utility_csv, which
//! resolves the header against the columns it interprets, and delivery_schema,
//! which checks it against a delivery schema's attribute names. Private to the
//! crate, as src/katana_survey/subsurface_table.hpp is.

use katana_core::error::make_error;
use katana_core::text::{is_ascii_space, split_lines, trimmed};
use katana_core::{Error, ErrorCode, Result};

/// True for the blanks the C++'s `core::isAsciiSpace` names, `\v` included,
/// which neither `str::trim` (Unicode) nor `trim_ascii` (no `\v`) agrees with.
pub(super) fn is_blank(character: char) -> bool {
    character.is_ascii() && is_ascii_space(character as u8)
}

/// `text` without the leading and trailing blanks of `is_blank`.
pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(is_blank)
}

/// Lower case with blanks, '-' and '_' removed, so that "Asset Owner",
/// "asset_owner" and "AssetOwner" are one name.
pub(super) fn key(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if !is_blank(character) && character != '-' && character != '_' {
            out.push(character.to_ascii_lowercase());
        }
    }
    out
}

/// "line 12": where an error is.
pub(super) fn at(line_number: usize) -> String {
    format!("line {line_number}")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RawRow {
    /// 1-based line of the text.
    pub line_number: usize,
    /// Trimmed, unquoted.
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RawTable {
    pub header_line: usize,
    pub header: Vec<String>,
    pub rows: Vec<RawRow>,
}

/// Bytes of a `&str` cut only at ASCII bytes are UTF-8 still, so the lossy
/// conversion never replaces anything; it only spares an unreachable error.
fn owned(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The comma-separated fields of one line, trimmed, with double quotes
/// removed. None for an unterminated quote or text after a closing quote.
fn split_fields(line: &[u8]) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut i = 0;
    loop {
        while i < line.len() && is_ascii_space(line[i]) {
            i += 1;
        }
        let field = if i < line.len() && line[i] == b'"' {
            i += 1;
            let mut quoted = Vec::new();
            let mut closed = false;
            while i < line.len() {
                if line[i] == b'"' {
                    if i + 1 < line.len() && line[i + 1] == b'"' {
                        quoted.push(b'"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                quoted.push(line[i]);
                i += 1;
            }
            if !closed {
                return None;
            }
            while i < line.len() && is_ascii_space(line[i]) {
                i += 1;
            }
            if i < line.len() && line[i] != b',' {
                return None;
            }
            owned(&quoted)
        } else {
            let end = line[i..]
                .iter()
                .position(|byte| *byte == b',')
                .map_or(line.len(), |offset| i + offset);
            let unquoted = owned(trimmed(&line[i..end]));
            i = end;
            unquoted
        };
        fields.push(field);
        if i >= line.len() {
            return Some(fields);
        }
        i += 1; // the comma
    }
}

/// The header and rows of `text`: UTF-8, an optional BOM skipped, blank lines
/// and lines starting with '#' skipped, fields separated by commas, a field
/// optionally double-quoted with "" for a quote inside it. ParseFailure, naming
/// the line, for unbalanced quotes, a row whose width differs from the
/// header's, or no header at all.
pub(super) fn read_raw_table(text: &str) -> Result<RawTable> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut table = RawTable::default();
    let mut have_header = false;
    for (index, raw) in split_lines(text.as_bytes()).into_iter().enumerate() {
        let line_number = index + 1;
        let line = trimmed(raw);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let Some(fields) = split_fields(line) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "unbalanced quotes",
                at(line_number),
            ));
        };
        if !have_header {
            have_header = true;
            table.header_line = line_number;
            table.header = fields;
            continue;
        }
        if fields.len() != table.header.len() {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!(
                    "{} fields where the header has {}",
                    fields.len(),
                    table.header.len()
                ),
                at(line_number),
            ));
        }
        table.rows.push(RawRow {
            line_number,
            fields,
        });
    }
    if !have_header {
        return Err(Error::new(ErrorCode::ParseFailure, "no header row"));
    }
    Ok(table)
}
