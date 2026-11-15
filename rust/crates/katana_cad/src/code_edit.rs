//! What the survey-code manager needs to edit a map that is not a dialog's
//! business to decide: the code list a person hands to a field crew, the key a
//! new rule for a drawing's code should start from, and the attribute lists a
//! form edits as text (include/katana/cad/code_edit.hpp,
//! src/katana_cad/customisation/code_edit.cpp).
//!
//! Here, below the window, so that each is tested on its own and the dialog
//! stays a thin form over it (docs/cad.md).

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, split_lines, trimmed};
use katana_core::{ErrorCode, Result};
use katana_entity::{SurveyAttribute, SurveyBreakline, SurveyMap};

use crate::code_table::code_table;

// ---- the code list as CSV -----------------------------------------------------

fn append_record(out: &mut String, fields: &[&str]) {
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&csv_field(field));
    }
    out.push_str("\r\n");
}

/// One field of a CSV record, quoted as RFC 4180 says: a field holding a
/// comma, a double quote, a carriage return or a line feed is enclosed in
/// double quotes, with each double quote inside it doubled; any other field
/// is written as it is. Leading and trailing blanks are data and are kept.
pub fn csv_field(text: &str) -> String {
    if !text.contains([',', '"', '\r', '\n']) {
        return String::from(text);
    }
    let mut quoted = String::from("\"");
    for c in text.chars() {
        if c == '"' {
            quoted.push('"');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// The map's codes as a CSV table for a person or a spreadsheet: a header
/// record, then one record per distinct key in key order (code_table), each
/// giving what a code caught by that key resolves to - the key's own rules and
/// the less specific keys that also catch it, combined as SurveyMap::lookup
/// combines them. Columns:
///
/// ```text
/// code, description, group, layer, colour, line/point, linestyle, symbol,
/// size, surface
/// ```
///
/// description is the rule comment, layer is the rule's model (which becomes
/// the layer), line/point is the breakline ("line" or "point"), size is the
/// symbol's size and surface is the rule's tinable ("yes" or "no": whether
/// the code goes into a surface). A field the rules do not set is EMPTY,
/// never a default: absent is not zero, and a symbol size of 0 is the map's
/// own way of saying "the definition's size", so it is empty too. Records end
/// with CRLF, as RFC 4180 says, including the last.
pub fn code_list_csv(map: &SurveyMap) -> String {
    let mut out = String::new();
    append_record(
        &mut out,
        &[
            "code",
            "description",
            "group",
            "layer",
            "colour",
            "line/point",
            "linestyle",
            "symbol",
            "size",
            "surface",
        ],
    );
    for row in code_table(map) {
        let rule = &row.combined;
        let breakline = match rule.breakline {
            Some(SurveyBreakline::Line) => "line",
            Some(SurveyBreakline::Point) => "point",
            None => "",
        };
        let mut symbol = String::new();
        let mut size = String::new();
        if let Some(given) = &rule.symbol {
            symbol.clone_from(&given.style);
            // 0 is the map's "the definition's own size": no size of its own.
            if given.size != 0.0 {
                size = format_exact_real(given.size);
            }
        }
        let tinable = match rule.tinable {
            Some(true) => "yes",
            Some(false) => "no",
            None => "",
        };
        append_record(
            &mut out,
            &[
                row.key.as_str(),
                rule.comment.as_str(),
                rule.group.as_str(),
                rule.model.as_str(),
                rule.colour.as_str(),
                breakline,
                rule.linestyle.as_str(),
                symbol.as_str(),
                size.as_str(),
                tinable,
            ],
        );
    }
    out
}

// ---- a new rule for a code a drawing carries ------------------------------------

/// The key a new rule for `code` should start from, for a person to accept or
/// change: a code ending in a number - a string number, "WM01", "KB12" - gets
/// a prefix key on what comes before the number ("WM*"), because every other
/// string of that code will want the same rule; any other code gets itself as
/// an exact key. Surrounding blanks are dropped (a key with them matches
/// nothing typed); a code that is all digits, or empty after trimming, is kept
/// exact, because "*" alone would catch every code.
pub fn suggested_key(code: &str) -> String {
    let text = trimmed(code.as_bytes());
    let mut end = text.len();
    while end > 0 && text[end - 1].is_ascii_digit() {
        end -= 1;
    }
    // No trailing number, or nothing but a number: the code itself, exact.
    // (Trimming removes ASCII blanks only, so what is left is whole UTF-8.)
    let whole = || String::from_utf8_lossy(text).into_owned();
    if end == text.len() || end == 0 {
        return whole();
    }
    // "KB 12" -> "KB*", not "KB *": the blank belongs to neither part, and a
    // key ending in a blank before its `*` would read as a typing slip.
    let stem = trimmed(&text[..end]);
    if stem.is_empty() {
        return whole();
    }
    format!("{}*", String::from_utf8_lossy(stem))
}

// ---- attribute lists as text ------------------------------------------------------

/// One attribute per line, as "<type> <name> = <value>": "text Source =
/// Survey". The type is the file's own word ("text" or "integer") and the
/// value is kept verbatim, "$PipeDiameter" included. Lines end with '\n'.
pub fn format_attribute_lines(attributes: &[SurveyAttribute]) -> String {
    let mut out = String::new();
    for attribute in attributes {
        out.push_str(&format!(
            "{} {} = {}\n",
            attribute.r#type, attribute.name, attribute.value
        ));
    }
    out
}

/// The inverse of format_attribute_lines, for a form a person types in. Blank
/// lines are skipped; the name and the value are trimmed, because a person
/// does not mean the blanks around an '='. Fails with InvalidArgument, naming
/// the line, on a line with no '=', no name, or a type other than "text" or
/// "integer" (the only two the mapfile writer can write). A name cannot hold
/// '=': the first '=' ends it.
pub fn parse_attribute_lines(text: &str) -> Result<Vec<SurveyAttribute>> {
    // Every cut below is at an ASCII byte or at the edge of ASCII blanks, so
    // each piece is whole UTF-8.
    let piece = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let mut attributes = Vec::new();
    for (line_number, raw) in (1..).zip(split_lines(text.as_bytes())) {
        let line = trimmed(raw);
        if line.is_empty() {
            continue;
        }
        let place = format!("attribute line {line_number}");
        let Some(equals) = line.iter().position(|&byte| byte == b'=') else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "an attribute is \"<type> <name> = <value>\"; this has no '='",
                place,
            ));
        };
        let head = trimmed(&line[..equals]);
        let blank = head.iter().position(|&byte| byte == b' ' || byte == b'\t');
        let kind = &head[..blank.unwrap_or(head.len())];
        let name: &[u8] = match blank {
            None => &[],
            Some(at) => trimmed(&head[at..]),
        };
        if kind != b"text" && kind != b"integer" {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "an attribute's type is \"text\" or \"integer\", not \"{}\"",
                    piece(kind)
                ),
                place,
            ));
        }
        if name.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "an attribute needs a name",
                place,
            ));
        }
        attributes.push(SurveyAttribute {
            r#type: piece(kind),
            name: piece(name),
            value: piece(trimmed(&line[equals + 1..])),
        });
    }
    Ok(attributes)
}
