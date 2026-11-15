//! The column layout of a delimited point file: roles, presets, validation and
//! the one-line template a layout is saved as. The reader, the writer and the
//! proposal are in delimited_points.rs and delimited_proposal.rs.

use std::collections::BTreeMap;

use katana_core::text::{equals_ignoring_case, is_ascii_space, parse_integer};
use katana_core::{Error, ErrorCode, Result, Status};

use crate::delimited_points::{ColumnPreset, ColumnRole, DelimitedLayout, Delimiter, Quoting};
use crate::delimited_records::{delimiter_character, trimmed_text};
use crate::format::{Named, to_string};

// ---- Names -------------------------------------------------------------------------

/// "point id", "northing", ... - how an error message names a column.
impl Named for ColumnRole {
    fn name(self) -> &'static str {
        match self {
            ColumnRole::PointId => "point id",
            ColumnRole::Northing => "northing",
            ColumnRole::Easting => "easting",
            ColumnRole::Elevation => "elevation",
            ColumnRole::Code => "code",
            ColumnRole::Description => "description",
            ColumnRole::Ignore => "ignored",
        }
    }
}

/// The letter a layout template spells the role with: P N E Z C D, and '-' for
/// Ignore. Not 'X': in a survey file X is a coordinate, and a template in which
/// X meant "skip this column" would be misread by the first person to open it.
pub fn template_letter(role: ColumnRole) -> char {
    match role {
        ColumnRole::PointId => 'P',
        ColumnRole::Northing => 'N',
        ColumnRole::Easting => 'E',
        ColumnRole::Elevation => 'Z',
        ColumnRole::Code => 'C',
        ColumnRole::Description => 'D',
        ColumnRole::Ignore => '-',
    }
}

/// The name export writes in a header line - "Point", "Northing", ... - chosen
/// so that propose_layout() reads an exported file back with the same layout.
/// Empty for Ignore.
pub fn header_name(role: ColumnRole) -> &'static str {
    match role {
        ColumnRole::PointId => "Point",
        ColumnRole::Northing => "Northing",
        ColumnRole::Easting => "Easting",
        ColumnRole::Elevation => "Elevation",
        ColumnRole::Code => "Code",
        ColumnRole::Description => "Description",
        ColumnRole::Ignore => "",
    }
}

/// "comma", "tab", ...
impl Named for Delimiter {
    fn name(self) -> &'static str {
        match self {
            Delimiter::Comma => "comma",
            Delimiter::Tab => "tab",
            Delimiter::Semicolon => "semicolon",
            Delimiter::Whitespace => "whitespace",
        }
    }
}

/// "double", "none"
impl Named for Quoting {
    fn name(self) -> &'static str {
        match self {
            Quoting::DoubleQuote => "double",
            Quoting::None => "none",
        }
    }
}

// ---- Validation --------------------------------------------------------------------

/// InvalidArgument, naming the problem, for a layout no file can be read with:
/// no northing or easting column, a role twice, a comment prefix the rules of
/// DelimitedLayout forbid. Parse and write both call this first.
pub fn validate_layout(layout: &DelimitedLayout) -> Status {
    let mut seen: BTreeMap<ColumnRole, usize> = BTreeMap::new();
    for (i, &role) in layout.columns.iter().enumerate() {
        if role == ColumnRole::Ignore {
            continue;
        }
        if let Some(&at) = seen.get(&role) {
            // Two easting columns is not a layout: which one is the easting?
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "the layout has two {} columns ({at} and {})",
                    to_string(role),
                    i + 1
                ),
            ));
        }
        seen.insert(role, i + 1);
    }
    if !seen.contains_key(&ColumnRole::Northing) || !seen.contains_key(&ColumnRole::Easting) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the layout needs a northing column and an easting column",
        )
        .with_context("a point is not a point without both"));
    }

    let prefix = &layout.comment_prefix;
    if let Some(&first) = prefix.as_bytes().first() {
        if first.is_ascii_digit() || first == b'+' || first == b'-' || first == b'.' {
            // A data line can start with any of these, and would be skipped as
            // a comment: a lost point, not a failed import.
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "comment prefix '{prefix}' could begin a number, so a line of data could be \
                     taken for a comment"
                ),
            ));
        }
        // Whitespace needs no check of its own: every blank is refused below.
        let delimiter = delimiter_character(layout.delimiter);
        for c in prefix.bytes() {
            if is_ascii_space(c) || c == b'"' || c == b';' || c == delimiter {
                // A blank would be trimmed off the line it is looked for in; a
                // quote would open a field; ';' separates template parts; the
                // delimiter would make a line of empty first fields a comment.
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "comment prefix '{prefix}' may not contain a blank, a quote, ';' or the \
                         delimiter"
                    ),
                ));
            }
        }
    }
    Ok(())
}

// ---- Presets -----------------------------------------------------------------------

const PRESETS: [ColumnPreset; 8] = [
    ColumnPreset::PNEZD,
    ColumnPreset::PENZD,
    ColumnPreset::PNEZ,
    ColumnPreset::PENZ,
    ColumnPreset::NEZ,
    ColumnPreset::ENZ,
    ColumnPreset::PNE,
    ColumnPreset::PEN,
];

fn role_for_byte(letter: u8) -> Option<ColumnRole> {
    match letter.to_ascii_lowercase() {
        b'p' => Some(ColumnRole::PointId),
        b'n' => Some(ColumnRole::Northing),
        b'e' => Some(ColumnRole::Easting),
        b'z' => Some(ColumnRole::Elevation),
        b'c' => Some(ColumnRole::Code),
        b'd' => Some(ColumnRole::Description),
        b'-' => Some(ColumnRole::Ignore),
        _ => None,
    }
}

fn role_for_letter(token: &str) -> Result<ColumnRole> {
    if let [letter] = token.as_bytes()
        && let Some(role) = role_for_byte(*letter)
    {
        return Ok(role);
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!("'{token}' is not a column letter (P, N, E, Z, C, D, or - to ignore a column)"),
    ))
}

/// Every preset, in declaration order - the order proposals list them in when
/// nothing in the file separates them, which is NOT a statement that one is
/// more likely.
pub fn all_column_presets() -> &'static [ColumnPreset] {
    &PRESETS
}

/// "PNEZD"
impl Named for ColumnPreset {
    fn name(self) -> &'static str {
        match self {
            ColumnPreset::PNEZD => "PNEZD",
            ColumnPreset::PENZD => "PENZD",
            ColumnPreset::PNEZ => "PNEZ",
            ColumnPreset::PENZ => "PENZ",
            ColumnPreset::NEZ => "NEZ",
            ColumnPreset::ENZ => "ENZ",
            ColumnPreset::PNE => "PNE",
            ColumnPreset::PEN => "PEN",
        }
    }
}

pub fn preset_columns(preset: ColumnPreset) -> Vec<ColumnRole> {
    // The name IS the layout: one letter per column, spelt as templates spell
    // them, so the two cannot drift apart.
    to_string(preset)
        .bytes()
        .map(|letter| role_for_byte(letter).expect("a preset's name is column letters"))
        .collect()
}

/// Case-insensitive ("pnezd" is PNEZD); NotFound for any other name.
pub fn column_preset_named(name: &str) -> Result<ColumnPreset> {
    for preset in PRESETS {
        if equals_ignoring_case(name.as_bytes(), to_string(preset).as_bytes()) {
            return Ok(preset);
        }
    }
    Err(Error::new(
        ErrorCode::NotFound,
        format!("'{name}' is not a column preset"),
    )
    .with_context("PNEZD, PENZD, PNEZ, PENZ, NEZ, ENZ, PNE or PEN"))
}

pub fn preset_layout(
    preset: ColumnPreset,
    delimiter: Delimiter,
    header_lines: usize,
) -> DelimitedLayout {
    DelimitedLayout {
        columns: preset_columns(preset),
        delimiter,
        header_lines,
        ..Default::default()
    }
}

// ---- Templates ---------------------------------------------------------------------
//
// A layout as one line of text, for saving and for showing in an import report:
//
//     P,N,E,Z,D;delimiter=comma;header=1;quote=double;comment=#
//
// The first part is the columns, one letter each (template_letter), separated
// by commas. Then key=value parts separated by ';':
//     delimiter  comma | tab | semicolon | whitespace     REQUIRED
//     header     lines to skip, a decimal integer >= 0   REQUIRED
//     quote      double | none        absent means double (RFC 4180)
//     comment    the comment prefix   absent means no comment lines
// The two required keys are required because nothing in a file can make up for
// a wrong one: a comma file read on whitespace, or a data line taken for a
// header, is a wrong import rather than a failed one. The two optional ones
// have a stated meaning when absent, which is not a default guessed for them.
// Letters, keys and enumerated values are matched without regard to case and
// blanks around any part are ignored; everything else - an unknown letter, key
// or value, a key given twice, an empty part, a layout validate_layout()
// refuses - is InvalidArgument. A template that cannot be read is never read
// as something close to it.

/// The canonical text: every key written, in the order above; `comment` only
/// when there is a prefix. parse_layout_template(layout_template(x)) == x.
pub fn layout_template(layout: &DelimitedLayout) -> String {
    let mut text = String::new();
    for &role in &layout.columns {
        if !text.is_empty() {
            text.push(',');
        }
        text.push(template_letter(role));
    }
    text.push_str(";delimiter=");
    text.push_str(to_string(layout.delimiter));
    text.push_str(&format!(";header={}", layout.header_lines));
    text.push_str(";quote=");
    text.push_str(to_string(layout.quoting));
    if !layout.comment_prefix.is_empty() {
        text.push_str(";comment=");
        text.push_str(&layout.comment_prefix);
    }
    text
}

fn template_error(text: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidArgument, message)
        .with_context(format!("layout template '{text}'"))
}

/// Reads a layout template (see above).
pub fn parse_layout_template(text: &str) -> Result<DelimitedLayout> {
    let parts: Vec<&str> = text.split(';').collect();
    if parts.iter().any(|part| trimmed_text(part).is_empty()) {
        return Err(template_error(text, "the template has an empty part"));
    }

    let mut layout = DelimitedLayout::default();
    for token in parts[0].split(',') {
        let letter = trimmed_text(token);
        if letter.is_empty() {
            return Err(template_error(text, "the column list has an empty entry"));
        }
        match role_for_letter(letter) {
            Ok(role) => layout.columns.push(role),
            Err(error) => return Err(template_error(text, error.message)),
        }
    }

    let mut values: BTreeMap<String, &str> = BTreeMap::new();
    for part in &parts[1..] {
        let part = trimmed_text(part);
        let Some(equals) = part.find('=') else {
            return Err(template_error(text, format!("'{part}' is not key=value")));
        };
        let key = trimmed_text(&part[..equals]).to_ascii_lowercase();
        let value = trimmed_text(&part[equals + 1..]);
        if !["delimiter", "header", "quote", "comment"].contains(&key.as_str()) {
            return Err(template_error(
                text,
                format!("unknown key '{key}' (the keys are delimiter, header, quote, comment)"),
            ));
        }
        if value.is_empty() {
            return Err(template_error(text, format!("'{key}' has no value")));
        }
        if values.contains_key(&key) {
            // Which one was meant? Taking either would be a guess.
            return Err(template_error(text, format!("'{key}' is given twice")));
        }
        values.insert(key, value);
    }

    let Some(&delimiter) = values.get("delimiter") else {
        return Err(template_error(
            text,
            "the template does not state its delimiter",
        ));
    };
    let mut known_delimiter = false;
    for candidate in [
        Delimiter::Comma,
        Delimiter::Tab,
        Delimiter::Semicolon,
        Delimiter::Whitespace,
    ] {
        if equals_ignoring_case(delimiter.as_bytes(), to_string(candidate).as_bytes()) {
            layout.delimiter = candidate;
            known_delimiter = true;
        }
    }
    if !known_delimiter {
        return Err(template_error(
            text,
            format!("delimiter '{delimiter}' is not comma, tab, semicolon or whitespace"),
        ));
    }

    let Some(&header) = values.get("header") else {
        return Err(template_error(
            text,
            "the template does not state how many header lines to skip",
        ));
    };
    // A plain count: parse_integer refuses "1.5", "1e2" and trailing text, and
    // a negative count of lines is not one.
    match parse_integer(header) {
        Some(count) if count >= 0 => layout.header_lines = count as usize,
        _ => {
            return Err(template_error(
                text,
                format!("header '{header}' is not a count of lines"),
            ));
        }
    }

    if let Some(&quote) = values.get("quote") {
        if equals_ignoring_case(quote.as_bytes(), to_string(Quoting::DoubleQuote).as_bytes()) {
            layout.quoting = Quoting::DoubleQuote;
        } else if equals_ignoring_case(quote.as_bytes(), to_string(Quoting::None).as_bytes()) {
            layout.quoting = Quoting::None;
        } else {
            return Err(template_error(
                text,
                format!("quote '{quote}' is not double or none"),
            ));
        }
    }
    if let Some(&comment) = values.get("comment") {
        layout.comment_prefix = String::from(comment);
    }

    if let Err(invalid) = validate_layout(&layout) {
        return Err(template_error(text, invalid.message));
    }
    Ok(layout)
}
