//! How a sheet set is plotted: its page setup, which sheets a plot takes, and
//! what the files it writes are called (docs/plotting.md, "Plot styles and
//! output"; include/katana/cad/plotting/page_setup.hpp).
//!
//! The PAGE SETUP is kept with the set (SheetSet::page_setup, stored in its
//! JSON), so a project plots the same way every time and for everyone: the
//! plot style (colour mode and line weight scale), the resolution, and for a
//! file per sheet the pattern the files are named by. The plot dialog starts
//! from it and the command line falls back to it.
//!
//! A SELECTION is text, so a person, a command line and an agent all say it
//! the same way: "1,3-5" is the first, third, fourth and fifth sheets in the
//! set's order; a sheet's id ("s7") names that sheet wherever it has moved to;
//! "" or "all" is every sheet.
//!
//! A FILE NAME is the pattern expanded for one sheet and made safe for the
//! file system: "{set}{n:02} {name}" is "C03 PLAN TILE 3" for the third
//! sheet of set C. Nothing here touches a file; the window writes them.

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, format_general};
use katana_core::{Error, ErrorCode, Result, Status};

use crate::document::Document;
use crate::plot::{PlotColourMode, PlotSettings};
use crate::plotting::sheet_commands::edit_sheet_set;
use crate::plotting::sheet_set::{SheetSet, digits_prefix, format_sheet_number, sheet_index};

/// The sheet number, zero-padded to two, after the set number, then the
/// sheet's name: files that sort in the set's order and say what they hold.
pub const DEFAULT_FILE_NAME_PATTERN: &str = "{set}{n:02} {name}";

/// The resolutions a plot may be made at, and the line weight factors. A
/// resolution outside them is a typing mistake (5 dpi, 30 000 dpi) rather
/// than a plot anyone wants; 1200 dpi is already an A1 raster of 1.1 billion
/// pixels, which plot_output refuses in its own right.
pub const MINIMUM_PLOT_DPI: f64 = 50.0;
pub const MAXIMUM_PLOT_DPI: f64 = 1200.0;
pub const MINIMUM_LINE_WEIGHT_SCALE: f64 = 0.1;
pub const MAXIMUM_LINE_WEIGHT_SCALE: f64 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub struct PageSetup {
    pub colour_mode: PlotColourMode,
    pub line_weight_scale: f64,
    pub dpi: f64,
    /// How a file of one sheet is named: format_sheet_number's {n}, {n:02},
    /// {N} and {set}, and {name} (the sheet's name), {number} (its sheet
    /// number as the set's numbering writes it) and {id}.
    pub file_name_pattern: String,
    /// A PDF a sheet, named by the pattern, rather than one PDF of them all.
    pub file_per_sheet: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            colour_mode: PlotColourMode::Colour,
            line_weight_scale: 1.0,
            dpi: 300.0,
            file_name_pattern: String::from(DEFAULT_FILE_NAME_PATTERN),
            file_per_sheet: false,
        }
    }
}

// The longest file name made, in bytes: well inside every file system's 255,
// with room for a " (12)" and an extension, and short enough that a folder a
// few levels down stays under Windows' 260-character path limit.
const MAXIMUM_FILE_NAME_BYTES: usize = 120;

const PATTERN_TOKENS: [&str; 6] = ["n", "N", "set", "name", "number", "id"];

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

// A position from 1 as written, or nothing when it does not fit a usize.
fn position_of(digits: &str) -> Option<usize> {
    match digits_prefix(digits.as_bytes()) {
        (Some(value), taken) if taken == digits.len() => Some(value),
        _ => None,
    }
}

// A piece of a file-name pattern: the text between tokens, or a {token} with
// its name and the format after its colon. An unclosed brace is literal text.
enum PatternPiece<'a> {
    Literal(&'a str),
    Token {
        whole: &'a str,
        name: &'a str,
        format: &'a str,
    },
}

// Each {token} of `pattern`, and what is between them, in order. The one
// tokeniser both the validation and the expansion use, so they cannot
// disagree about a pattern.
fn scan_pattern(pattern: &str) -> Vec<PatternPiece<'_>> {
    let mut pieces = Vec::new();
    let mut at = 0;
    while at < pattern.len() {
        let Some(open) = pattern[at..].find('{').map(|found| found + at) else {
            pieces.push(PatternPiece::Literal(&pattern[at..]));
            return pieces;
        };
        pieces.push(PatternPiece::Literal(&pattern[at..open]));
        let Some(close) = pattern[open..].find('}').map(|found| found + open) else {
            pieces.push(PatternPiece::Literal(&pattern[open..]));
            return pieces;
        };
        let inside = &pattern[open + 1..close];
        let (name, format) = match inside.find(':') {
            Some(colon) => (&inside[..colon], &inside[colon + 1..]),
            None => (inside, ""),
        };
        pieces.push(PatternPiece::Token {
            whole: &pattern[open..=close],
            name,
            format,
        });
        at = close + 1;
    }
    pieces
}

fn is_device_name(name: &str) -> bool {
    let stem = match name.find('.') {
        Some(dot) => &name[..dot],
        None => name,
    };
    let base = stem.to_ascii_lowercase();
    if ["con", "prn", "aux", "nul"].contains(&base.as_str()) {
        return true;
    }
    let bytes = base.as_bytes();
    bytes.len() == 4
        && (base.starts_with("com") || base.starts_with("lpt"))
        && (b'1'..=b'9').contains(&bytes[3])
}

fn trim_ends(text: &str) -> String {
    String::from(text.trim_matches([' ', '.']))
}

/// InvalidArgument for an empty pattern, a brace left open, or a {token} that
/// is not one of those PageSetup::file_name_pattern lists. A pattern with no
/// token that changes from sheet to sheet is accepted: sheet_file_names tells
/// the files apart by a counter.
pub fn validate_file_name_pattern(pattern: &str) -> Status {
    if pattern.bytes().all(|byte| byte == b' ') {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the file-name pattern is empty",
        ));
    }
    let mut problem: Option<String> = None;
    for piece in scan_pattern(pattern) {
        match piece {
            PatternPiece::Literal(text) => {
                if problem.is_none() && text.contains('{') {
                    problem = Some(format!(
                        "a brace is left open in the file-name pattern \"{pattern}\""
                    ));
                }
            }
            PatternPiece::Token {
                whole,
                name,
                format,
            } => {
                let known = PATTERN_TOKENS.contains(&name);
                if problem.is_none() && (!known || (!format.is_empty() && !all_digits(format))) {
                    problem = Some(format!(
                        "the file-name pattern has no token {whole}: use {{n}}, {{n:02}}, {{N}}, \
                         {{set}}, {{name}}, {{number}} or {{id}}"
                    ));
                }
            }
        }
    }
    match problem {
        Some(problem) => Err(Error::new(ErrorCode::InvalidArgument, problem)),
        None => Ok(()),
    }
}

/// InvalidArgument, saying which and why, for a resolution or line weight
/// scale out of range or not finite, or a file-name pattern that
/// validate_file_name_pattern refuses.
pub fn validate_page_setup(setup: &PageSetup) -> Status {
    if !setup.dpi.is_finite() || setup.dpi < MINIMUM_PLOT_DPI || setup.dpi > MAXIMUM_PLOT_DPI {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the resolution must be {} to {} dpi",
                format_general(MINIMUM_PLOT_DPI, 6),
                format_general(MAXIMUM_PLOT_DPI, 6)
            ),
            format!("{} dpi", format_exact_real(setup.dpi)),
        ));
    }
    if !setup.line_weight_scale.is_finite()
        || setup.line_weight_scale < MINIMUM_LINE_WEIGHT_SCALE
        || setup.line_weight_scale > MAXIMUM_LINE_WEIGHT_SCALE
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the line weight scale must be {} to {}",
                format_general(MINIMUM_LINE_WEIGHT_SCALE, 6),
                format_general(MAXIMUM_LINE_WEIGHT_SCALE, 6)
            ),
            format_exact_real(setup.line_weight_scale),
        ));
    }
    validate_file_name_pattern(&setup.file_name_pattern)
}

/// The plot settings a page setup asks for: its colour mode, line weight
/// scale and resolution, the rest at their defaults.
pub fn plot_settings_for(setup: &PageSetup) -> PlotSettings {
    PlotSettings {
        colour_mode: setup.colour_mode,
        line_weight_scale: setup.line_weight_scale,
        dpi: setup.dpi,
        ..PlotSettings::default()
    }
}

/// The sheets `text` names, as indices into `set.sheets`, in the order named
/// and each once: comma-separated sheet positions from 1 ("4"), ranges of them
/// ("3-5", low to high) and sheet ids ("s7"); spaces around a comma or a
/// range's dash are ignored, and one inside a part ("1 3") is refused rather
/// than read as sheet 13. Empty text or "all" is every sheet in order.
/// InvalidArgument, naming the part, for a position past the last sheet or
/// below 1, a backward range, or a word that is no sheet's id; and for a set
/// with no sheets.
pub fn parse_sheet_selection(text: &str, set: &SheetSet) -> Result<Vec<usize>> {
    let count = set.sheets.len();
    if count == 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there are no sheets to plot",
        ));
    }
    // Spaces are dropped only around the commas and the dash of a range, not
    // inside a part: "1 3" is a mistake for "1,3" and is refused, rather than
    // read as sheet 13.
    let bytes = text.as_bytes();
    let mut compact = String::new();
    let mut characters = text.char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        if character != ' ' && character != '\t' {
            compact.push(character);
            continue;
        }
        let rest = text[at..].trim_start_matches([' ', '\t']);
        let next = if rest.is_empty() {
            None
        } else {
            Some(text.len() - rest.len())
        };
        let edge = compact.is_empty()
            || compact.ends_with(',')
            || compact.ends_with('-')
            || next.is_none_or(|next| bytes[next] == b',' || bytes[next] == b'-');
        if !edge {
            compact.push(' ');
        }
        let Some(next) = next else {
            break;
        };
        while characters.peek().is_some_and(|&(index, _)| index < next) {
            characters.next();
        }
    }
    if compact.is_empty() || compact.eq_ignore_ascii_case("all") {
        return Ok((0..count).collect());
    }
    let mut chosen = Vec::new();
    let mut seen = BTreeSet::new();
    let mut take = |index: usize, chosen: &mut Vec<usize>| {
        if seen.insert(index) {
            chosen.push(index);
        }
    };
    // A position from 1 checked against the set; the reason when it is not one.
    let position = |digits: &str| -> std::result::Result<usize, String> {
        let value = position_of(digits);
        if value == Some(0) {
            return Err(String::from(
                "there is no sheet 0: sheets are numbered from 1",
            ));
        }
        match value {
            Some(value) if value <= count => Ok(value - 1),
            _ => Err(format!(
                "there is no sheet {digits}: the set has {count} sheet{}",
                if count == 1 { "" } else { "s" }
            )),
        }
    };
    let invalid = |why: String| Error::new(ErrorCode::InvalidArgument, why);
    for part in compact.split(',') {
        if part.is_empty() {
            continue;
        }
        let dash = part.find('-');
        if all_digits(part) {
            let index = position(part).map_err(invalid)?;
            take(index, &mut chosen);
        } else if let Some(dash) = dash
            && dash > 0
            && all_digits(&part[..dash])
            && all_digits(&part[dash + 1..])
        {
            let first = position(&part[..dash]).map_err(invalid)?;
            let last = position(&part[dash + 1..]).map_err(invalid)?;
            if last < first {
                return Err(invalid(format!(
                    "the range {part} runs backwards: write {}-{}",
                    last + 1,
                    first + 1
                )));
            }
            for index in first..=last {
                take(index, &mut chosen);
            }
        } else if let Some(found) = sheet_index(set, part) {
            take(found, &mut chosen);
        } else {
            return Err(invalid(format!(
                "\"{part}\" is neither a sheet number nor a sheet's id"
            )));
        }
    }
    if chosen.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the selection names no sheet",
            text,
        ));
    }
    Ok(chosen)
}

/// The shortest text parse_sheet_selection reads back as `indices`: positions
/// from 1, runs of three or more written as ranges ("1,3-5"). Empty for none.
pub fn format_sheet_selection(indices: &[usize]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < indices.len() {
        let mut run = 1;
        while i + run < indices.len() && indices[i + run] == indices[i] + run {
            run += 1;
        }
        if !out.is_empty() {
            out.push(',');
        }
        if run >= 3 {
            out.push_str(&format!("{}-{}", indices[i] + 1, indices[i] + run));
            i += run;
        } else {
            out.push_str(&format!("{}", indices[i] + 1));
            i += 1;
        }
    }
    out
}

/// `name` made safe as a file name on every system Katana runs on: the
/// characters Windows forbids (< > : " / \ | ? *) and control characters
/// become '-', runs of spaces become one, spaces and dots are trimmed from the
/// ends (Windows drops them), a device name (CON, PRN, AUX, NUL, COM1-9,
/// LPT1-9, with or without an extension) gets a '_' in front, and the result is
/// cut to 120 bytes at a character boundary. "sheet" when nothing is left.
pub fn sanitise_file_name(name: &str) -> String {
    const FORBIDDEN: &str = "<>:\"/\\|?*";
    let mut out = String::new();
    for character in name.chars() {
        // Every character replaced is ASCII, so this is the C++'s byte loop.
        let kept = if character < '\u{20}' || character == '\u{7f}' || FORBIDDEN.contains(character)
        {
            '-'
        } else {
            character
        };
        if kept == ' ' && (out.is_empty() || out.ends_with(' ')) {
            continue;
        }
        out.push(kept);
    }
    let mut out = trim_ends(&out);
    if out.len() > MAXIMUM_FILE_NAME_BYTES {
        let mut cut = MAXIMUM_FILE_NAME_BYTES;
        // Back to the first byte of a character: never half of one.
        while cut > 0 && !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out = trim_ends(&out);
    }
    if out.is_empty() {
        return String::from("sheet");
    }
    if is_device_name(&out) {
        out.insert(0, '_');
    }
    out
}

/// The file name, without an extension, of sheet `index` under `pattern`:
/// expanded, then sanitised. Empty for an index past the end.
pub fn sheet_file_name(set: &SheetSet, index: usize, pattern: &str) -> String {
    let Some(sheet) = set.sheets.get(index) else {
        return String::new();
    };
    let count = set.sheets.len();
    let mut expanded = String::new();
    for piece in scan_pattern(pattern) {
        match piece {
            PatternPiece::Literal(text) => expanded.push_str(text),
            PatternPiece::Token { whole, name, .. } => match name {
                "name" => expanded.push_str(&sheet.name),
                "number" => expanded.push_str(&format_sheet_number(
                    &set.numbering,
                    index + 1,
                    count,
                    &set.defaults.set_number,
                )),
                "id" => expanded.push_str(&sheet.id),
                // {n}, {n:02}, {N}, {set}; anything else comes back as written.
                _ => expanded.push_str(&format_sheet_number(
                    whole,
                    index + 1,
                    count,
                    &set.defaults.set_number,
                )),
            },
        }
    }
    sanitise_file_name(&expanded)
}

/// sheet_file_name for each of `indices`, made distinct ignoring case (as the
/// file systems Katana runs on compare names): a name met again gets " (2)",
/// " (3)"... in order, so the same set always gives the same names.
pub fn sheet_file_names(set: &SheetSet, indices: &[usize], pattern: &str) -> Vec<String> {
    let mut names = Vec::with_capacity(indices.len());
    let mut used = BTreeSet::new();
    for &index in indices {
        let base = sheet_file_name(set, index, pattern);
        let mut name = base.clone();
        let mut n = 2;
        while !used.insert(name.to_ascii_lowercase()) {
            name = format!("{base} ({n})");
            n += 1;
        }
        names.push(name);
    }
    names
}

/// Makes `setup` the document's page setup, as one undoable step
/// ("PAGE_SETUP"); no step when it is the page setup already. Refused with
/// validate_page_setup's error, or with the document's when its sheets cannot
/// be written (a newer version's).
pub fn set_page_setup(document: &mut Document, setup: &PageSetup) -> Status {
    validate_page_setup(setup)?;
    if document.sheet_set_status().is_ok() && document.sheet_set().page_setup == *setup {
        return Ok(());
    }
    edit_sheet_set(
        document,
        |set| {
            set.page_setup = setup.clone();
            Ok(())
        },
        "PAGE_SETUP",
    )
}
