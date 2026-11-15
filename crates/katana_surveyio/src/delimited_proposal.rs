//! Proposing the layout of a delimited point file from its first lines.
//!
//! The rule this file exists to keep: the order of northing and easting is
//! DECIDED only by a header that names them. Everything else - the delimiter,
//! the header count, whether the first column is an id - is inferred, because a
//! wrong inference there fails loudly in the parser (a column of text read as a
//! number is an error with a line and a column). A wrong coordinate order fails
//! silently, so it is never inferred.

use katana_core::text::{parse_finite_double, parse_integer};
use katana_core::text_encoding::{DecodedText, TextEncoding, decode_text};
use katana_core::{Error, ErrorCode, Result};

use crate::delimited_layout::{all_column_presets, layout_template, preset_columns};
use crate::delimited_points::{
    ColumnRole, DelimitedLayout, Delimiter, LayoutCandidate, LayoutProposal, LayoutProposalOutcome,
    PROPOSAL_SAMPLE_LINES, Quoting,
};
use crate::delimited_records::{
    Field, is_ascii_space_char, quoted_for_message, split_line, split_text_lines, trimmed_text,
    value_of,
};
use crate::detect::PROBE_BYTES;
use crate::format::{Named, to_string};

// ---- LayoutProposal ----------------------------------------------------------------

impl Named for LayoutProposalOutcome {
    fn name(self) -> &'static str {
        match self {
            LayoutProposalOutcome::Decided => "decided",
            LayoutProposalOutcome::Uncertain => "uncertain",
            LayoutProposalOutcome::Empty => "empty",
        }
    }
}

impl LayoutProposal {
    pub fn empty(summary: String) -> Self {
        Self {
            outcome: LayoutProposalOutcome::Empty,
            candidates: Vec::new(),
            summary,
        }
    }

    pub fn uncertain(ranked: Vec<LayoutCandidate>, summary: String) -> Self {
        Self {
            outcome: LayoutProposalOutcome::Uncertain,
            candidates: ranked,
            summary,
        }
    }

    pub fn decided(chosen: LayoutCandidate, summary: String) -> Self {
        Self {
            outcome: LayoutProposalOutcome::Decided,
            candidates: vec![chosen],
            summary,
        }
    }

    /// The layout to parse with when the outcome is Decided; NotFound
    /// otherwise, with the summary as the message.
    pub fn layout(&self) -> Result<DelimitedLayout> {
        if self.outcome == LayoutProposalOutcome::Decided
            && let Some(chosen) = self.candidates.first()
        {
            return Ok(chosen.layout.clone());
        }
        Err(Error::new(
            ErrorCode::NotFound,
            format!(
                "the layout of this file was not decided ({}): {}",
                to_string(self.outcome),
                self.summary
            ),
        ))
    }
}

/// Reads the start of a file and proposes how to read it.
///
/// * The encoding is found as parse_delimited_points finds it (decode_text).
/// * The delimiter is the first of tab, semicolon, comma, whitespace that
///   splits every sampled row into at least two numbers. Tab first because it
///   is almost never content; semicolon before comma because a semicolon file
///   is often written with decimal commas, which a comma split would cut in
///   half; whitespace last because a description with a space in it would
///   otherwise split a CSV row.
/// * Lines above the first row of numbers are header lines, and the last of
///   them is read as the header when there is one. In a whitespace file the
///   blank in "Easting (m)" splits it like the blanks between names, so there
///   a word that is nothing but a bracketed unit or the punctuation below
///   ("(m)", "#"), and every word inside a bracket, belong to the name before
///   them. A name that is still split ("Reduced Level") leaves the header with
///   more names than the rows have fields, which keeps the proposal Uncertain:
///   the names no longer line up with the columns.
/// * A layout taken from a header has the columns the header names, empty
///   names at its end dropped. Rows whose trailing fields are empty do not add
///   Ignore columns, so a value that appears there after the sampled lines is
///   an error on import, naming its line, rather than read past.
/// * '#' at the start of a line is proposed as a comment prefix when a sampled
///   line starts with one. Nothing else is: a prefix that happened to match a
///   point id would drop the point. '#' can match one too, so a '#' line below
///   the header (or anywhere, with no header) that is a row of numbers once the
///   '#' is off - a point commented out, or an id such as "#12" - keeps the
///   proposal Uncertain and is named in the summary.
///
/// Header words, compared without case, blanks, '_', '-', '.', '#' or a
/// bracketed unit such as "(m)":
///
/// ```text
/// point id     p pt pnt point id name no num number pointid pointname pointno
///              pointnumber ptid ptno ptnum
/// northing     n north northing          easting   e east easting
/// elevation    z elev elevation el height ht h rl reducedlevel level
/// code         c code featurecode feature fcode pointcode ptcode
/// description  d desc descr description remark remarks note notes comment comments
/// X, Y         read, but NEVER as a decision. In CAD and GIS, x runs east and y
///              north; in the geodetic convention of Gauss-Kruger grids -
///              Germany, Russia and much of central Europe - x is northing and y
///              easting. A file headed X,Y,Z is therefore Uncertain with both
///              readings listed, x-as-easting first because it is Katana's own
///              drawing convention, and a person says which.
/// latitude, longitude (lat, latitude, lon, long, longitude)  refused: this
///              reader takes grid coordinates, and degrees read as metres put
///              the survey a few hundred metres from the grid origin, where it
///              plots, rather than failing.
/// ```
///
/// Any other header word makes its column Ignore, and the evidence names it;
/// so does a blank header cell, and a value in such a column keeps the
/// proposal Uncertain.
///
/// `bytes` may be the whole file: only the first PROBE_BYTES (detect.rs) are
/// decoded, the budget a detection reads, so a proposal costs the same for a
/// 50 MB file as for a 5 KB one. `truncated` says the bytes themselves stop
/// part way through the file (a detection sample). Either way the last line,
/// which may be cut, is not looked at. ParseFailure when the bytes cannot be
/// decoded at all.
pub fn propose_layout(bytes: &[u8], truncated: bool) -> Result<LayoutProposal> {
    // A caller may hand over a whole file; only its start is ever looked at, so
    // only its start is decoded - the same budget a detection reads.
    let cut = bytes.len() > PROBE_BYTES;
    let text = decode_sample(&bytes[..bytes.len().min(PROBE_BYTES)], truncated || cut)?;
    Ok(analyse(&text).proposal)
}

// ---- The proposal's working, shared with the detection probe ----------------------

/// `bytes` decoded to UTF-8 as decode_text decodes them. When `truncated` (a
/// detection sample), the bytes may stop inside a character or a line: up to
/// three trailing bytes are dropped until the rest decodes as what it evidently
/// is, rather than letting a character cut in half turn a UTF-8 sample into a
/// Windows-1252 one, and then the last line, which may be cut, is dropped.
pub(crate) fn decode_sample(bytes: &[u8], truncated: bool) -> Result<String> {
    let attempts: usize = if truncated { 4 } else { 1 };
    let mut chosen: Option<DecodedText> = None;
    let mut windows_reading: Option<DecodedText> = None;
    let mut first_error: Option<Error> = None;
    for cut in 0..attempts.min(bytes.len() + 1) {
        let decoded = match decode_text(&bytes[..bytes.len() - cut]) {
            Ok(decoded) => decoded,
            Err(error) => {
                // A UTF-16 sample cut at an odd byte, or inside a surrogate
                // pair.
                if first_error.is_none() {
                    first_error = Some(error);
                }
                continue;
            }
        };
        let fallback = decoded.encoding == TextEncoding::Windows1252 && decoded.guessed;
        if fallback && cut + 1 < attempts {
            // Perhaps a UTF-8 character cut in half at the end; a shorter cut
            // that validates as UTF-8 is the better reading of the same file.
            if windows_reading.is_none() {
                windows_reading = Some(decoded);
            }
            continue;
        }
        chosen = Some(decoded);
        break;
    }
    let Some(chosen) = chosen.or(windows_reading) else {
        return Err(first_error.unwrap_or_else(|| {
            Error::new(ErrorCode::ParseFailure, "the sample could not be decoded")
        }));
    };
    let mut text = chosen.text;
    if truncated {
        let keep = text
            .rfind(['\r', '\n'])
            .map_or(0, |last_break| last_break + 1);
        text.truncate(keep);
    }
    Ok(text)
}

/// What analyse() found.
pub(crate) struct Analysis {
    pub(crate) proposal: LayoutProposal,
    /// The delimiter that split the rows into numbers; None when none did.
    pub(crate) delimiter: Option<Delimiter>,
    /// A header line names the horizontal coordinate columns - as northing and
    /// easting, or as X and Y. What the probe reports as the stronger evidence
    /// that this is a point file, whether or not the order was decided.
    pub(crate) header_names_coordinates: bool,
}

// ---- Header words ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NameKind {
    Role,
    AxisX,
    AxisY,
    Geographic,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Meaning {
    kind: NameKind,
    role: ColumnRole,
}

// The vocabulary is documented, with the reasons for X, Y and latitude, beside
// propose_layout() above; keep the two in step. Names already normalised: see
// normalised_name.
const KNOWN_NAMES: [(&str, NameKind, ColumnRole); 56] = [
    ("p", NameKind::Role, ColumnRole::PointId),
    ("pt", NameKind::Role, ColumnRole::PointId),
    ("pnt", NameKind::Role, ColumnRole::PointId),
    ("point", NameKind::Role, ColumnRole::PointId),
    ("id", NameKind::Role, ColumnRole::PointId),
    ("name", NameKind::Role, ColumnRole::PointId),
    ("no", NameKind::Role, ColumnRole::PointId),
    ("num", NameKind::Role, ColumnRole::PointId),
    ("number", NameKind::Role, ColumnRole::PointId),
    ("pointid", NameKind::Role, ColumnRole::PointId),
    ("pointname", NameKind::Role, ColumnRole::PointId),
    ("pointno", NameKind::Role, ColumnRole::PointId),
    ("pointnumber", NameKind::Role, ColumnRole::PointId),
    ("ptid", NameKind::Role, ColumnRole::PointId),
    ("ptno", NameKind::Role, ColumnRole::PointId),
    ("ptnum", NameKind::Role, ColumnRole::PointId),
    ("n", NameKind::Role, ColumnRole::Northing),
    ("north", NameKind::Role, ColumnRole::Northing),
    ("northing", NameKind::Role, ColumnRole::Northing),
    ("e", NameKind::Role, ColumnRole::Easting),
    ("east", NameKind::Role, ColumnRole::Easting),
    ("easting", NameKind::Role, ColumnRole::Easting),
    ("z", NameKind::Role, ColumnRole::Elevation),
    ("elev", NameKind::Role, ColumnRole::Elevation),
    ("elevation", NameKind::Role, ColumnRole::Elevation),
    ("el", NameKind::Role, ColumnRole::Elevation),
    ("height", NameKind::Role, ColumnRole::Elevation),
    ("ht", NameKind::Role, ColumnRole::Elevation),
    ("h", NameKind::Role, ColumnRole::Elevation),
    ("rl", NameKind::Role, ColumnRole::Elevation),
    ("reducedlevel", NameKind::Role, ColumnRole::Elevation),
    ("level", NameKind::Role, ColumnRole::Elevation),
    ("c", NameKind::Role, ColumnRole::Code),
    ("code", NameKind::Role, ColumnRole::Code),
    ("featurecode", NameKind::Role, ColumnRole::Code),
    ("feature", NameKind::Role, ColumnRole::Code),
    ("fcode", NameKind::Role, ColumnRole::Code),
    ("pointcode", NameKind::Role, ColumnRole::Code),
    ("ptcode", NameKind::Role, ColumnRole::Code),
    ("d", NameKind::Role, ColumnRole::Description),
    ("desc", NameKind::Role, ColumnRole::Description),
    ("descr", NameKind::Role, ColumnRole::Description),
    ("description", NameKind::Role, ColumnRole::Description),
    ("remark", NameKind::Role, ColumnRole::Description),
    ("remarks", NameKind::Role, ColumnRole::Description),
    ("note", NameKind::Role, ColumnRole::Description),
    ("notes", NameKind::Role, ColumnRole::Description),
    ("comment", NameKind::Role, ColumnRole::Description),
    ("comments", NameKind::Role, ColumnRole::Description),
    ("x", NameKind::AxisX, ColumnRole::Ignore),
    ("y", NameKind::AxisY, ColumnRole::Ignore),
    ("lat", NameKind::Geographic, ColumnRole::Ignore),
    ("latitude", NameKind::Geographic, ColumnRole::Ignore),
    ("lon", NameKind::Geographic, ColumnRole::Ignore),
    ("long", NameKind::Geographic, ColumnRole::Ignore),
    ("longitude", NameKind::Geographic, ColumnRole::Ignore),
];

// "Point No.", "point_no", "POINT-NO" and "Pt#" are one word to a person, so
// they are one word here: ASCII letters folded, blanks and the punctuation
// people put in column names dropped, and a bracketed unit - "Easting (m)",
// "RL[ft]" - removed. The unit is not read: the caller states the file's unit.
fn normalised_name(raw: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in raw.chars() {
        if c == '(' || c == '[' {
            depth += 1;
        } else if c == ')' || c == ']' {
            depth = if depth > 0 { depth - 1 } else { 0 };
        } else if depth == 0
            && !is_ascii_space_char(c)
            && c != '_'
            && c != '-'
            && c != '.'
            && c != '#'
        {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

fn meaning_of(raw: &str) -> Meaning {
    let name = normalised_name(raw);
    for (known, kind, role) in KNOWN_NAMES {
        if known == name {
            return Meaning { kind, role };
        }
    }
    Meaning {
        kind: NameKind::Unknown,
        role: ColumnRole::Ignore,
    }
}

// Whether `raw` leaves a bracket open, counted as normalised_name counts them.
fn leaves_bracket_open(raw: &str) -> bool {
    let mut depth = 0i32;
    for c in raw.bytes() {
        if c == b'(' || c == b'[' {
            depth += 1;
        } else if c == b')' || c == b']' {
            depth = if depth > 0 { depth - 1 } else { 0 };
        }
    }
    depth > 0
}

#[derive(Clone, Copy, Debug)]
struct Line<'t> {
    text: &'t str,
    /// Physical, 1-based.
    number: usize,
}

// The names a header line gives its columns, in order.
//
// In a whitespace file the blank inside "Easting (m)" splits it exactly as the
// blanks between names do, and every name after it would then head the column
// to the right of its data. So there two kinds of word join the name before
// them: one normalised_name keeps nothing of, such as "(m)" or "#", and one
// inside a bracket that name left open, as in "Easting (US ft)". "Reduced
// Level" cannot be put back together this way, since both halves are words;
// that is left to the check that the header has no more names than the rows
// have fields.
//
// Empty names at the end are dropped: "a,b,c," names three columns, just as a
// row's trailing empty fields are nothing.
fn header_names(header: &Line<'_>, delimiter: Delimiter) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut open = false; // the last name has a bracket not yet closed
    for field in split_line(header.text, delimiter) {
        let word = value_of(&field);
        let part_of_last = delimiter == Delimiter::Whitespace
            && !field.quoted
            && (open || (!word.is_empty() && normalised_name(word).is_empty()));
        if part_of_last && let Some(last) = names.last_mut() {
            last.push(' ');
            last.push_str(word);
        } else {
            names.push(String::from(word));
        }
        open = !field.quoted
            && names
                .last()
                .is_some_and(|last| leaves_bracket_open(last.as_str()));
    }
    while names.last().is_some_and(String::is_empty) {
        names.pop();
    }
    names
}

// ---- Rows --------------------------------------------------------------------------

fn is_number(value: &str) -> bool {
    parse_finite_double(value).is_some()
}

fn numeric_fields(fields: &[Field]) -> usize {
    fields
        .iter()
        .filter(|field| is_number(value_of(field)))
        .count()
}

// A row of data has at least a northing and an easting in it.
const NUMBERS_IN_A_ROW: usize = 2;

struct Split {
    delimiter: Delimiter,
    /// One per content line.
    fields: Vec<Vec<Field>>,
    /// Index of the first row of numbers.
    first_row: Option<usize>,
    /// A later line that is not one.
    first_misfit: Option<usize>,
}

fn split_with(content: &[Line<'_>], delimiter: Delimiter) -> Split {
    let mut split = Split {
        delimiter,
        fields: Vec::with_capacity(content.len()),
        first_row: None,
        first_misfit: None,
    };
    for (i, line) in content.iter().enumerate() {
        let fields = split_line(line.text, delimiter);
        let row = numeric_fields(&fields) >= NUMBERS_IN_A_ROW;
        split.fields.push(fields);
        if row && split.first_row.is_none() {
            split.first_row = Some(i);
        } else if !row && split.first_row.is_some() && split.first_misfit.is_none() {
            split.first_misfit = Some(i);
        }
    }
    split
}

// True when splitting on ';' and reading ',' as a decimal point would have made
// rows of numbers: the file is very likely written with decimal commas, which
// this reader refuses (core's text) and a person should be told about rather
// than left with "no delimiter fits".
fn looks_like_decimal_commas(content: &[Line<'_>]) -> bool {
    content.iter().any(|line| {
        let numbers = split_line(line.text, Delimiter::Semicolon)
            .iter()
            .filter(|field| is_number(&value_of(field).replace(',', ".")))
            .count();
        numbers >= NUMBERS_IN_A_ROW
    })
}

#[derive(Clone, Copy, Debug)]
struct ColumnProfile {
    /// Every value present is a number.
    numeric: bool,
    /// Every value present is an integer.
    integral: bool,
    /// Every row has a value.
    present: bool,
}

fn profile_of(rows: &[Vec<Field>], width: usize) -> Vec<ColumnProfile> {
    let mut profiles = vec![
        ColumnProfile {
            numeric: true,
            integral: true,
            present: true,
        };
        width
    ];
    for row in rows {
        for (c, profile) in profiles.iter_mut().enumerate() {
            let value = row.get(c).map_or("", value_of);
            if value.is_empty() {
                profile.present = false;
                continue;
            }
            profile.numeric = profile.numeric && is_number(value);
            profile.integral = profile.integral && parse_integer(value).is_some();
        }
    }
    profiles
}

fn column_list(columns: &[usize]) -> String {
    let mut text = String::new();
    for (i, column) in columns.iter().enumerate() {
        text.push_str(if i == 0 {
            ""
        } else if i + 1 == columns.len() {
            " and "
        } else {
            ", "
        });
        text.push_str(&format!("{column}"));
    }
    text
}

fn column_of(layout: &DelimitedLayout, role: ColumnRole) -> Option<usize> {
    layout.columns.iter().position(|&column| column == role)
}

// Every layout this is asked about has both: they come from a preset or from a
// header that named both.
fn order_phrase(layout: &DelimitedLayout) -> String {
    let northing = column_of(layout, ColumnRole::Northing).expect("the layout has a northing");
    let easting = column_of(layout, ColumnRole::Easting).expect("the layout has an easting");
    format!(
        "northing in column {}, easting in column {}",
        northing + 1,
        easting + 1
    )
}

// The first sampled row this layout would fail on, as a sentence; None when
// every row fits. The parser's own checks, applied early, so that a header the
// rows contradict is reported as uncertain rather than decided.
fn first_misfit(
    layout: &DelimitedLayout,
    rows: &[Vec<Field>],
    line_numbers: &[usize],
) -> Option<String> {
    for (row, &line) in rows.iter().zip(line_numbers) {
        for (c, &role) in layout.columns.iter().enumerate() {
            let value = row.get(c).map_or("", value_of);
            let at = format!("line {line}, column {} ({})", c + 1, to_string(role));
            let required = role == ColumnRole::PointId
                || role == ColumnRole::Northing
                || role == ColumnRole::Easting;
            if required && value.is_empty() {
                return Some(format!("{at} is empty"));
            }
            let numeric = role == ColumnRole::Northing
                || role == ColumnRole::Easting
                || role == ColumnRole::Elevation;
            if numeric && !value.is_empty() && !is_number(value) {
                return Some(format!(
                    "{at} holds {}, which is not a number",
                    quoted_for_message(value, 40)
                ));
            }
        }
    }
    None
}

// Every preset the rows are consistent with, best fit first. What decides the
// order among them is column count and whether a first column that would be
// the id holds ids; what never does is which coordinate comes first - a
// northing-first and an easting-first preset that both fit are listed in
// all_column_presets() order, and `why` says the order is not known.
fn preset_candidates(
    base: &DelimitedLayout,
    profiles: &[ColumnProfile],
    why: &str,
) -> Vec<LayoutCandidate> {
    struct Scored {
        candidate: LayoutCandidate,
        exact: bool,
        doubtful_id: bool,
    }
    let mut scored: Vec<Scored> = Vec::new();
    let width = profiles.len();
    for &preset in all_column_presets() {
        let columns = preset_columns(preset);
        if columns.len() > width {
            continue;
        }
        let fits = columns
            .iter()
            .zip(profiles)
            .all(|(&role, profile)| match role {
                ColumnRole::Northing | ColumnRole::Easting => profile.present && profile.numeric,
                ColumnRole::Elevation => profile.numeric,
                ColumnRole::PointId => profile.present,
                _ => true,
            });
        if !fits {
            continue;
        }
        let mut layout = base.clone();
        layout.columns = columns;
        let mut ignored = Vec::new();
        for c in layout.columns.len()..width {
            layout.columns.push(ColumnRole::Ignore);
            ignored.push(c + 1);
        }
        let exact = ignored.is_empty();
        // Ids like "12" or "PT7" are ids; a column of values like 5000000.125
        // read as ids would shift every coordinate one column along.
        let doubtful_id = layout.columns[0] == ColumnRole::PointId
            && profiles[0].numeric
            && !profiles[0].integral;
        let mut evidence = format!(
            "{why}; {} reads {}",
            to_string(preset),
            order_phrase(&layout)
        );
        if !ignored.is_empty() {
            evidence.push_str(&format!(
                "; column{}{} ignored",
                if ignored.len() > 1 { "s " } else { " " },
                column_list(&ignored)
            ));
        }
        if doubtful_id {
            evidence.push_str("; column 1 holds decimal numbers, which are unusual ids");
        }
        scored.push(Scored {
            candidate: LayoutCandidate { layout, evidence },
            exact,
            doubtful_id,
        });
    }
    // Stable: exact fits first, then those whose first column could hold ids.
    scored.sort_by_key(|entry| (!entry.exact, entry.doubtful_id));
    scored.into_iter().map(|entry| entry.candidate).collect()
}

fn quoted_header(header: &str) -> String {
    quoted_for_message(trimmed_text(header), 60)
}

// The first sampled row with a value in a column the header does not name - one
// past its last name, or one under a blank header cell - as a clause; None
// when there is none. Empty fields do not count: "a,b,c," is how many writers
// end a row, and the parser reads past them. A value does count, because the
// layout can only read past it as Ignore - an elevation column the header
// forgot, a spreadsheet column nobody headed, the second word of a description
// in a whitespace file - and nothing in the import would say it had gone.
fn unnamed_value(rows: &[Vec<Field>], line_numbers: &[usize], names: &[String]) -> Option<String> {
    for (row, &line) in rows.iter().zip(line_numbers) {
        for (c, field) in row.iter().enumerate() {
            if names.get(c).is_some_and(|name| !name.is_empty()) {
                continue;
            }
            let value = value_of(field);
            if !value.is_empty() {
                return Some(format!(
                    "line {line} has a value in column {}, which the header does not name ({})",
                    c + 1,
                    quoted_for_message(value, 40)
                ));
            }
        }
    }
    None
}

// The first '#' line after `after` that is a row of numbers once the '#' is
// taken off, as a clause; None when there is none. Such a line is a point
// commented out or a point whose id begins with '#', and the text cannot say
// which - but either reading of it as a comment, or as one of the lines above
// the first row, drops a point, so it is never proposed as settled. A '#' line
// of prose is a comment whatever numbers it mentions, since it does not split
// into them.
fn row_shaped_comment(
    hash_lines: &[Line<'_>],
    delimiter: Delimiter,
    after: usize,
) -> Option<String> {
    for line in hash_lines {
        if line.number <= after {
            continue;
        }
        // The line starts with '#' once trimmed, and '#' is one byte.
        let rest = &trimmed_text(line.text)[1..];
        if numeric_fields(&split_line(rest, delimiter)) >= NUMBERS_IN_A_ROW {
            return Some(format!(
                "line {} starts with '#' and is otherwise a row of numbers - a point commented \
                 out, or a point whose id begins with '#' - and would not be imported; check it \
                 before importing",
                line.number
            ));
        }
    }
    None
}

fn joined(notes: &[String]) -> String {
    notes.iter().map(|note| format!("; {note}")).collect()
}

/// Everything propose_layout() does after decoding.
pub(crate) fn analyse(text: &str) -> Analysis {
    let mut analysis = Analysis {
        proposal: LayoutProposal::default(),
        delimiter: None,
        header_names_coordinates: false,
    };

    // Content lines: not blank, not a '#' comment. The '#' lines are kept aside
    // to check, once the delimiter is known, that none of them is a point.
    let mut content: Vec<Line<'_>> = Vec::new();
    let mut hash_lines: Vec<Line<'_>> = Vec::new();
    for (i, &physical) in split_text_lines(text).iter().enumerate() {
        if content.len() >= PROPOSAL_SAMPLE_LINES {
            break;
        }
        let line = trimmed_text(physical);
        if line.is_empty() {
            continue;
        }
        let entry = Line {
            text: physical,
            number: i + 1,
        };
        if line.starts_with('#') {
            hash_lines.push(entry);
            continue;
        }
        content.push(entry);
    }
    let comments = !hash_lines.is_empty();
    if content.is_empty() {
        analysis.proposal = LayoutProposal::empty(String::from(if comments {
            "every line of the file is blank or a comment"
        } else {
            "the file holds no text"
        }));
        return analysis;
    }

    // The delimiter: the first, in the order propose_layout() gives and
    // justifies, that makes every line from the first row of numbers onwards a
    // row of numbers.
    let mut chosen: Option<Split> = None;
    let mut nearest: Option<Split> = None; // for saying why nothing fitted
    for delimiter in [
        Delimiter::Tab,
        Delimiter::Semicolon,
        Delimiter::Comma,
        Delimiter::Whitespace,
    ] {
        let split = split_with(&content, delimiter);
        if split.first_row.is_some() && split.first_misfit.is_none() {
            chosen = Some(split);
            break;
        }
        if split.first_row.is_some()
            && nearest
                .as_ref()
                .is_none_or(|nearest| split.first_misfit > nearest.first_misfit)
        {
            nearest = Some(split);
        }
    }
    let Some(split) = chosen else {
        let mut summary = match &nearest {
            Some(nearest) => {
                let misfit = &content[nearest
                    .first_misfit
                    .expect("a misfit is why it was not chosen")];
                format!(
                    "split on {}, line {} ({}) is not a row of numbers although the lines before \
                     it are, and no other delimiter fits every row",
                    to_string(nearest.delimiter),
                    misfit.number,
                    quoted_header(misfit.text)
                )
            }
            None => String::from(
                "no line splits into two or more numbers on a tab, semicolon, comma or blanks",
            ),
        };
        if looks_like_decimal_commas(&content) {
            summary.push_str(
                "; the numbers look like they are written with decimal commas, which this reader \
                 does not accept - convert them to decimal points",
            );
        }
        analysis.proposal = LayoutProposal::uncertain(Vec::new(), summary);
        return analysis;
    };
    analysis.delimiter = Some(split.delimiter);
    let first_row = split
        .first_row
        .expect("the chosen split has a first row of numbers");

    let base = DelimitedLayout {
        delimiter: split.delimiter,
        quoting: Quoting::DoubleQuote,
        // Every physical line above the first row, header and title lines
        // alike.
        header_lines: content[first_row].number - 1,
        comment_prefix: String::from(if comments { "#" } else { "" }),
        columns: Vec::new(),
    };

    let rows = &split.fields[first_row..];
    let mut line_numbers: Vec<usize> = Vec::with_capacity(rows.len());
    let mut width = 0usize; // the most fields a row has, empty ones included
    let mut value_columns = 0usize; // columns up to the last one any row has a value in
    for (line, fields) in content[first_row..].iter().zip(rows) {
        line_numbers.push(line.number);
        width = width.max(fields.len());
        if let Some(last) = fields.iter().rposition(|field| !value_of(field).is_empty()) {
            value_columns = value_columns.max(last + 1);
        }
    }
    let profiles = profile_of(rows, width);
    let delimiter_word = format!("{}-delimited", to_string(split.delimiter));
    let comment_note = if comments {
        "; lines starting '#' are comments"
    } else {
        ""
    };
    // '#' lines above a header are titles; below it - or anywhere, when there
    // is no header and every line above the first row is skipped - one may be a
    // point.
    let hash_row = row_shaped_comment(
        &hash_lines,
        split.delimiter,
        if first_row > 0 {
            content[first_row - 1].number
        } else {
            0
        },
    );
    let hash_note = hash_row
        .as_ref()
        .map(|row| format!("; {row}"))
        .unwrap_or_default();

    let mut why = String::from(
        "no header line, so the file does not say whether northing or easting comes first",
    );
    if first_row > 0 {
        let header_line = &content[first_row - 1];
        let names = header_names(header_line, split.delimiter);
        let unnamed = unnamed_value(rows, &line_numbers, &names);
        // More names than any row has fields: the header's words do not line
        // up with the rows' columns. A name with a blank or a delimiter in it
        // that header_names could not put back together ("Reduced Level",
        // "Height, m") has split in two, every name after it heads the column
        // to the right of its data, and a value under the wrong name is read
        // into the wrong role or past as Ignore, with nothing to say so.
        let overhang = (names.len() > width).then(|| {
            format!(
                "it names {} columns and no sampled row has more than {width} fields, so its \
                 words may not line up with the rows' columns",
                names.len()
            )
        });

        let meanings: Vec<Meaning> = names.iter().map(|name| meaning_of(name.as_str())).collect();
        let mut northings = 0usize;
        let mut eastings = 0usize;
        let mut xs = 0usize;
        let mut ys = 0usize;
        for meaning in &meanings {
            let named = meaning.kind == NameKind::Role;
            northings += usize::from(named && meaning.role == ColumnRole::Northing);
            eastings += usize::from(named && meaning.role == ColumnRole::Easting);
            xs += usize::from(meaning.kind == NameKind::AxisX);
            ys += usize::from(meaning.kind == NameKind::AxisY);
        }
        let geographic = meanings
            .iter()
            .position(|meaning| meaning.kind == NameKind::Geographic);

        if let Some(column) = geographic {
            analysis.proposal = LayoutProposal::uncertain(
                Vec::new(),
                format!(
                    "column {} is headed {}: this reader takes grid northings and eastings, and \
                     latitude and longitude read as metres would place the survey wrongly without \
                     any error; convert the file to grid coordinates first",
                    column + 1,
                    quoted_for_message(&names[column], 40)
                ),
            );
            return analysis;
        }

        // The header's roles, for the columns it names; X, Y, repeats, blank
        // cells and words it does not know are Ignore here and filled in or
        // reported below.
        //
        // The layout has the header's columns, and more only when a sampled row
        // has a value past them: Ignore out to the last such value, so that the
        // candidate parses the sample while `unnamed`, reported below, keeps it
        // from being decided. It never reaches out to a trailing EMPTY field.
        // An Ignore column there would read past a value that first appears
        // after the sample, where one past the layout's last column the parser
        // refuses, naming the line.
        let layout_from_header = |x_role: Option<ColumnRole>,
                                  y_role: Option<ColumnRole>,
                                  notes: &mut Vec<String>|
         -> DelimitedLayout {
            let mut layout = base.clone();
            let mut used: Vec<ColumnRole> = Vec::new();
            let mut unknown: Vec<String> = Vec::new(); // "2 ('Date')"
            let mut blank: Vec<usize> = Vec::new();
            for c in 0..value_columns.max(names.len()) {
                let mut role = ColumnRole::Ignore;
                if let (Some(meaning), Some(name)) = (meanings.get(c), names.get(c)) {
                    if meaning.kind == NameKind::Role {
                        role = meaning.role;
                    } else if meaning.kind == NameKind::AxisX
                        && let Some(x) = x_role
                    {
                        role = x;
                    } else if meaning.kind == NameKind::AxisY
                        && let Some(y) = y_role
                    {
                        role = y;
                    } else if meaning.kind == NameKind::Unknown && name.is_empty() {
                        blank.push(c + 1);
                    } else if meaning.kind == NameKind::Unknown {
                        unknown.push(format!("{} ({})", c + 1, quoted_for_message(name, 40)));
                    }
                    if role != ColumnRole::Ignore && used.contains(&role) {
                        notes.push(format!(
                            "column {} is a second {} column and is ignored",
                            c + 1,
                            to_string(role)
                        ));
                        role = ColumnRole::Ignore;
                    }
                    used.push(role);
                }
                layout.columns.push(role);
            }
            if !unknown.is_empty() {
                // Quoted, so a person can see a misspelt "Descripton" for what
                // it is rather than wonder where the descriptions went.
                let mut list = String::new();
                for (i, entry) in unknown.iter().enumerate() {
                    list.push_str(if i == 0 {
                        ""
                    } else if i + 1 == unknown.len() {
                        " and "
                    } else {
                        ", "
                    });
                    list.push_str(entry);
                }
                notes.push(if unknown.len() > 1 {
                    format!(
                        "columns {list} are not header words this reader knows, so they are \
                         ignored"
                    )
                } else {
                    format!(
                        "column {list} is not a header word this reader knows, so it is ignored"
                    )
                });
            }
            if !blank.is_empty() {
                // Said even when every sampled row is empty there: a value
                // further down the file would be read past.
                notes.push(if blank.len() > 1 {
                    format!(
                        "columns {} have no header, so they are ignored",
                        column_list(&blank)
                    )
                } else {
                    format!(
                        "column {} has no header, so it is ignored",
                        column_list(&blank)
                    )
                });
            }
            layout
        };

        if northings == 1 && eastings == 1 {
            analysis.header_names_coordinates = true;
            let mut notes: Vec<String> = Vec::new();
            let layout = layout_from_header(None, None, &mut notes);
            if xs + ys != 0 {
                notes.push(String::from(
                    "an X or Y column beside named northing and easting columns is ignored",
                ));
            }
            let header = format!(
                "the header on line {} names {}",
                header_line.number,
                order_phrase(&layout)
            );
            let evidence = format!(
                "{header} ({delimiter_word}, {}){}{comment_note}",
                layout_template(&layout),
                joined(&notes)
            );
            let mut candidate = LayoutCandidate {
                layout: layout.clone(),
                evidence,
            };
            // Named repeats are a question, not a detail: which "Code" was
            // meant?
            let repeated = notes.iter().any(|note| note.contains("is a second"));
            if let Some(misfit) = first_misfit(&layout, rows, &line_numbers) {
                analysis.proposal = LayoutProposal::uncertain(
                    vec![candidate],
                    format!("{header}, but {misfit}; check the header against the rows"),
                );
                return analysis;
            }
            if let Some(overhang) = &overhang {
                candidate.evidence.push_str(&format!("; {overhang}"));
                analysis.proposal = LayoutProposal::uncertain(
                    vec![candidate],
                    format!(
                        "{header}, but {overhang}; check the header against the rows{}{hash_note}",
                        joined(&notes)
                    ),
                );
                return analysis;
            }
            if let Some(unnamed) = &unnamed {
                candidate
                    .evidence
                    .push_str(&format!("; {unnamed}, and it is read past"));
                analysis.proposal = LayoutProposal::uncertain(
                    vec![candidate],
                    format!(
                        "{header}, but {unnamed}; say what that column holds{}{hash_note}",
                        joined(&notes)
                    ),
                );
                return analysis;
            }
            if let Some(hash_row) = &hash_row {
                candidate.evidence.push_str(&hash_note);
                analysis.proposal = LayoutProposal::uncertain(
                    vec![candidate],
                    format!("{header}, but {hash_row}{}", joined(&notes)),
                );
                return analysis;
            }
            if repeated {
                analysis.proposal = LayoutProposal::uncertain(
                    vec![candidate],
                    format!("{header}, but it names a column twice{}", joined(&notes)),
                );
                return analysis;
            }
            analysis.proposal =
                LayoutProposal::decided(candidate, format!("{header}{}", joined(&notes)));
            return analysis;
        }

        if xs == 1 && ys == 1 && northings == 0 && eastings == 0 {
            analysis.header_names_coordinates = true;
            let mut notes: Vec<String> = Vec::new();
            let gis_order = layout_from_header(
                Some(ColumnRole::Easting),
                Some(ColumnRole::Northing),
                &mut notes,
            );
            let rest = format!(
                "{}{}{}{comment_note}{hash_note}",
                joined(&notes),
                overhang
                    .as_ref()
                    .map(|overhang| format!("; {overhang}"))
                    .unwrap_or_default(),
                unnamed
                    .as_ref()
                    .map(|unnamed| format!("; {unnamed}"))
                    .unwrap_or_default()
            );
            notes.clear();
            let geodetic_order = layout_from_header(
                Some(ColumnRole::Northing),
                Some(ColumnRole::Easting),
                &mut notes,
            );
            // The rows cannot tell the two readings apart - that is the point -
            // but they can contradict the header's other columns.
            let check = first_misfit(&gis_order, rows, &line_numbers)
                .map(|misfit| format!("; but {misfit}"))
                .unwrap_or_default();
            let gis_evidence = format!(
                "X read as easting and Y as northing, the CAD and GIS convention and Katana's own \
                 ({}){rest}{check}",
                order_phrase(&gis_order)
            );
            let geodetic_evidence = format!(
                "X read as northing and Y as easting, the geodetic convention of Gauss-Kruger \
                 grids ({}){rest}{check}",
                order_phrase(&geodetic_order)
            );
            analysis.proposal = LayoutProposal::uncertain(
                vec![
                    LayoutCandidate {
                        layout: gis_order,
                        evidence: gis_evidence,
                    },
                    LayoutCandidate {
                        layout: geodetic_order,
                        evidence: geodetic_evidence,
                    },
                ],
                format!(
                    "the header on line {} names X and Y, which are easting and northing in CAD \
                     and GIS but northing and easting in Gauss-Kruger grids; say which this file \
                     uses{hash_note}",
                    header_line.number
                ),
            );
            return analysis;
        }

        why = format!(
            "the header on line {} ({}) does not name one northing and one easting column, so the \
             file does not say which comes first",
            header_line.number,
            quoted_header(header_line.text)
        );
    }

    let mut candidates = preset_candidates(&base, &profiles, &why);
    for candidate in &mut candidates {
        candidate.evidence.push_str(comment_note);
        candidate.evidence.push_str(&hash_note);
    }
    let mut summary = why;
    if candidates.is_empty() {
        summary.push_str(&format!(
            "; no preset layout fits these {delimiter_word} rows, so the columns have to be set \
             by hand"
        ));
    } else {
        summary.push_str("; choose a layout");
    }
    summary.push_str(&hash_note);
    analysis.proposal = LayoutProposal::uncertain(candidates, summary);
    analysis
}
