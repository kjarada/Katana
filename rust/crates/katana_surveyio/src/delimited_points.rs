//! Delimited point files: CSV, TXT, tab-, semicolon- or whitespace-separated
//! coordinate lists, read and written through a column layout a person states.
//!
//! This is the format with no specification at all, which is why everything
//! about how a file is read is a value the caller holds - a DelimitedLayout -
//! and nothing is decided inside the parser. The layout is what the import
//! wizard shows, what a person corrects, and what is saved as a template for
//! the next file off the same instrument.
//!
//! THE ONE THING THIS FILE REFUSES TO GUESS is the order of the coordinates.
//! "1,5000000.0,500000.0" is a point at northing 5 000 000 or at easting
//! 5 000 000 and nothing in the numbers says which: a transposed survey plots,
//! is self-consistent, and is in the wrong place. So the order comes from the
//! layout, the layout comes from a person, and propose_layout() only DECIDES
//! when a header line names the columns; otherwise it says it is uncertain and
//! a caller has to ask.
//!
//! What the reader does not do, by design: it transforms nothing. The
//! coordinate system is recorded as unknown - a delimited file cannot declare
//! one - and the linear unit is whatever the caller says the numbers are in,
//! converted to metres on the way in as the survey data model requires.
//!
//! This module holds the reader, the writer and the format's registration. The
//! layout's names, presets, validation and template are in delimited_layout.rs,
//! the proposal in delimited_proposal.rs, and the record splitter all three
//! share in delimited_records.rs.

use std::collections::BTreeMap;
use std::collections::btree_map;

use katana_core::text::{format_fixed, is_ascii_space, parse_finite_double};
use katana_core::text_encoding::{TextEncoding, decode_text, decode_text_as};
use katana_core::{Error, ErrorCode, Result};
use katana_math::unit_ratio::{from_metres, to_metres};
use katana_survey::data_model::{metres_per, source_file_name, validate_project};
use katana_survey::{LinearUnit, SourceRecord, SurveyPoint};

use crate::delimited_records::{
    RecordReader, delimiter_character, is_padding, quoted_for_message, trimmed_text, value_of,
};
use crate::detect::{IDENTIFICATION_MARGIN, IDENTIFIED_CONFIDENCE};
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, ImportResult,
    Manufacturer, ProbeInput, ruled_out, to_string,
};

pub use crate::delimited_layout::{
    all_column_presets, column_preset_named, header_name, layout_template, parse_layout_template,
    preset_columns, preset_layout, template_letter, validate_layout,
};
pub use crate::delimited_proposal::propose_layout;

/// The id this format registers under (format.rs's contract: a slug, never
/// changed, never translated).
pub const DELIMITED_POINTS_FORMAT_ID: &str = "delimited-points";

// ---- The layout ------------------------------------------------------------------

/// What one column of the file holds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ColumnRole {
    PointId,
    Northing,
    Easting,
    Elevation,
    Code,
    Description,
    /// Read past, never stored.
    Ignore,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Delimiter {
    #[default]
    Comma,
    Tab,
    Semicolon,
    /// A RUN of blanks (space, tab, vertical tab, form feed) separates fields,
    /// and blanks at either end of a line separate nothing - which is what a
    /// column-aligned TXT listing needs. It also means an empty field cannot be
    /// written except as "" (quoted), so export refuses one when quoting is
    /// off.
    Whitespace,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Quoting {
    /// RFC 4180: a field that BEGINS with '"' runs to the matching '"', a
    /// doubled "" inside it is one quote, and it may hold the delimiter and
    /// line breaks. A quote anywhere else in a field is an ordinary character,
    /// so a description such as 6" PVC reads as written; RFC 4180 would call
    /// that field malformed, but it has only one possible reading, and it is
    /// common in survey descriptions.
    #[default]
    DoubleQuote,
    /// '"' is an ordinary character everywhere.
    None,
}

/// How to read, or write, one delimited point file.
///
/// The defaults make a value that is easy to build in code; they are not what
/// a file is assumed to be. A layout that reaches a parser comes from a person,
/// a saved template (parse_layout_template, which requires the delimiter and
/// header count to be stated) or a proposal that says whether it is sure.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DelimitedLayout {
    /// In file order. Northing and Easting are required; every other role at
    /// most once, except Ignore, which may repeat.
    pub columns: Vec<ColumnRole>,
    pub delimiter: Delimiter,
    /// Physical lines skipped at the top of the file, whatever they hold.
    /// Lines, not records: a header is not parsed, so a quote in one cannot run
    /// on.
    pub header_lines: usize,
    /// A line whose first non-blank characters are these is a comment. Empty
    /// for none. May not start with a digit, a sign or '.', since then a line
    /// of data could be a comment and a point would vanish; may not hold a
    /// blank, '"', ';' (the template separator) or the delimiter.
    pub comment_prefix: String,
    pub quoting: Quoting,
}

// ---- Presets ---------------------------------------------------------------------

/// The column orders survey software names by their letters. PNEZD is the
/// commonest export from civil design packages and PENZD from GIS and most
/// instrument CSV exports; which one a file is in is the question
/// propose_layout() will not answer without a header.
#[allow(
    clippy::upper_case_acronyms,
    reason = "A preset is named by its column letters, as survey software spells it."
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColumnPreset {
    PNEZD,
    PENZD,
    PNEZ,
    PENZ,
    NEZ,
    ENZ,
    PNE,
    PEN,
}

// ---- Proposing a layout from the file itself ----------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LayoutProposalOutcome {
    /// A header line names the northing and easting columns in words that mean
    /// nothing else, names no column twice, and the sampled rows agree with it:
    /// every value where the header puts a number is one, the header names no
    /// more columns than the widest row has fields, no row has a value in a
    /// column the header does not name (past its last name or under a blank
    /// cell), and no line starting '#' is otherwise a row of numbers. Each of
    /// those would otherwise lose data without an error - read into the wrong
    /// role, read past as Ignore, or skipped as a comment.
    Decided,
    /// Something is not known - usually the coordinate order: no header, a
    /// header that does not name the coordinates, X/Y, or rows that disagree
    /// with the header. The candidates are what fits; a person chooses.
    Uncertain,
    /// There was no text to look at.
    #[default]
    Empty,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LayoutCandidate {
    pub layout: DelimitedLayout,
    /// Why this layout, and what it could not tell - "no header, so the order
    /// of northing and easting is not stated". A person has to be able to
    /// check it.
    pub evidence: String,
}

/// What propose_layout() found. Built like Detection (detect.rs) and for the
/// same reason: the one accessor that hands back a usable layout, layout(),
/// answers only when the outcome is Decided, so a caller cannot take the first
/// candidate of an uncertain proposal by accident. The factories and layout()
/// are in delimited_proposal.rs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LayoutProposal {
    pub(crate) outcome: LayoutProposalOutcome,
    pub(crate) candidates: Vec<LayoutCandidate>,
    pub(crate) summary: String,
}

impl LayoutProposal {
    pub fn outcome(&self) -> LayoutProposalOutcome {
        self.outcome
    }

    /// Best first. For Decided, the one layout the header decided.
    pub fn candidates(&self) -> &[LayoutCandidate] {
        &self.candidates
    }

    /// One sentence for a person: what was decided, or what was not and why.
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

/// Content lines a proposal looks at: rows of data, a header and any title
/// lines above it, not counting blank and comment lines. Policy, not a
/// measurement: a few hundred rows is enough to see that a column is numeric
/// throughout and few enough that proposing is instant on the 64 KiB a
/// detection reads.
pub const PROPOSAL_SAMPLE_LINES: usize = 200;

// ---- Import ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DelimitedImportOptions {
    /// The unit the file's coordinates are in. REQUIRED: Unknown is refused,
    /// because a delimited file never says and there is no default unit
    /// (metres_per says why).
    pub unit: LinearUnit,
    /// The encoding a person has stated, or None to detect it. Detection
    /// (core's text_encoding) reads UTF-8 with or without a byte order mark,
    /// UTF-16 little- or big-endian with a mark or inferred from where the NULs
    /// fall - Excel's "Unicode text" is UTF-16LE - and falls back to
    /// Windows-1252 for bytes that are not UTF-8, with a warning, since that one
    /// is a guess.
    pub encoding: Option<TextEncoding>,
}

// The version of THIS code (format.rs). Bump it when a file would import
// differently, so the SourceRecords of old imports say which reading they had.
const PARSER_VERSION: &str = "1.0";

fn where_(line: usize, column: usize, role: ColumnRole) -> String {
    format!("line {line}, column {column} ({})", to_string(role))
}

fn is_required(role: ColumnRole) -> bool {
    role == ColumnRole::PointId || role == ColumnRole::Northing || role == ColumnRole::Easting
}

fn has_role(layout: &DelimitedLayout, role: ColumnRole) -> bool {
    layout.columns.contains(&role)
}

/// Reads a delimited point file with `layout`.
///
/// All or nothing. Any of these is an error naming the line and the column,
/// and no point is imported: a number that does not parse (parse_finite_double:
/// no decimal comma, no "inf", no trailing text); an empty northing or easting;
/// an empty point id where the layout has an id column; a line too short to
/// reach a point id, northing or easting column; text in a field past the
/// layout's last column; a quoted field that is never closed, or text after its
/// closing quote; a point id used twice (AlreadyExists, naming both lines). A
/// silently dropped row is a lost measurement.
///
/// What is NOT an error: an empty elevation, or a line that ends before the
/// elevation, code or description column (the point has no elevation - absent,
/// never 0 - and an empty code or description, counted in the warnings); blank
/// lines, and lines starting with the comment prefix (skipped, and comments
/// counted in the warnings); a layout with NO point id column. Each point is
/// then named by the number of the line it is on ("12"), and a warning says so.
/// Line numbers because they are unique by construction, and because the id
/// then says where in the file the point came from - a sequence number would
/// say neither.
///
/// Unquoted fields are trimmed of blanks; a quoted field is kept exactly. Line
/// numbers are 1-based physical lines of the decoded text, with CR, LF and CRLF
/// each ending one; a record whose quoted field spans lines is numbered by the
/// line it starts on. Only the file NAME of `file_name` is kept. Every point
/// carries a SourceRecord whose format_version is the layout template it was
/// read with, so an import can be repeated exactly; the project's metadata
/// holds "parser version" (delimited_points_format().parser_version) and "text
/// encoding" (the encoding the bytes were decoded as). The project's coordinate
/// system is unknown, its units are options.unit, and it passes
/// validate_project.
pub fn parse_delimited_points(
    bytes: &[u8],
    layout: &DelimitedLayout,
    file_name: &str,
    options: &DelimitedImportOptions,
) -> Result<ImportResult> {
    let name = source_file_name(file_name);
    if let Err(invalid) = validate_layout(layout) {
        return Err(Error::new(invalid.code, invalid.message).with_context(name));
    }
    let Ok(unit) = metres_per(options.unit) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "state the unit the coordinates are in: a delimited file does not say, and none is assumed",
        )
        .with_context(name));
    };
    let decoded = match options.encoding {
        Some(encoding) => decode_text_as(bytes, encoding),
        None => decode_text(bytes),
    };
    let decoded = match decoded {
        Ok(decoded) => decoded,
        Err(error) => {
            return Err(Error::new(ErrorCode::ParseFailure, error.message).with_context(name));
        }
    };

    let descriptor = delimited_points_format();
    let file_source = SourceRecord {
        manufacturer: String::from(to_string(descriptor.manufacturer)),
        format: descriptor.human_name.clone(),
        // The layout, not the parser version: it is what says how THIS file
        // was read, and with it the import can be repeated exactly.
        format_version: layout_template(layout),
        file_name: name.clone(),
        record_number: 0,
    };

    let mut result = ImportResult {
        format_id: descriptor.id.clone(),
        ..Default::default()
    };
    let project = &mut result.project;
    project.name = name.clone();
    project.source = file_source.clone();
    // A delimited file cannot declare a coordinate system: the default,
    // explicitly unknown, is the truth. The unit is the caller's statement.
    project.units.linear = options.unit;
    project.metadata.insert(
        String::from("text encoding"),
        String::from(katana_core::text_encoding::to_string(decoded.encoding)),
    );
    // The format contract wants the parser version on every import, so a file
    // that reads differently after an upgrade can be told apart.
    // SourceRecord::format_version already holds the variant - this file's
    // layout, as the data model defines that field - so the version of the
    // code goes here.
    project.metadata.insert(
        String::from("parser version"),
        descriptor.parser_version.clone(),
    );

    let id_column = has_role(layout, ColumnRole::PointId);
    let mut line_of_id: BTreeMap<String, usize> = BTreeMap::new();
    let mut short_rows = 0usize;
    let mut first_short_row = 0usize;
    let mut without_elevation = 0usize;
    let mut first_without_elevation = 0usize;

    let mut reader = RecordReader::new(&decoded.text, layout);
    loop {
        let record = match reader.next() {
            Ok(Some(record)) => record,
            Ok(None) => break,
            Err(error) => {
                return Err(Error::new(error.code, error.message).with_context(name));
            }
        };

        // Text past the last column is data this layout has nowhere to put -
        // most often a description with an unquoted delimiter in it, which
        // would otherwise be cut short. Empty trailing fields ("a,b,c,") are
        // nothing.
        for (i, field) in record.fields.iter().enumerate().skip(layout.columns.len()) {
            let extra = value_of(field);
            if !extra.is_empty() {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    format!(
                        "line {}: {} fields where the layout has {}; column {} holds {}",
                        record.line,
                        record.fields.len(),
                        layout.columns.len(),
                        i + 1,
                        quoted_for_message(extra, 40)
                    ),
                )
                .with_context(name));
            }
        }

        let mut point = SurveyPoint::default();
        let mut short_row = false;
        for (c, &role) in layout.columns.iter().enumerate() {
            if role == ColumnRole::Ignore {
                continue;
            }
            let Some(field) = record.fields.get(c) else {
                if is_required(role) {
                    return Err(Error::new(
                        ErrorCode::ParseFailure,
                        format!(
                            "{}: missing; the line has only {} fields",
                            where_(record.line, c + 1, role),
                            record.fields.len()
                        ),
                    )
                    .with_context(name));
                }
                short_row = true;
                continue;
            };
            let value = value_of(field);
            match role {
                ColumnRole::PointId => {
                    if value.is_empty() {
                        // No id is invented for a point whose id column is
                        // empty: an invented one would collide with, or be
                        // mistaken for, a real mark's number.
                        return Err(Error::new(
                            ErrorCode::ParseFailure,
                            format!("{}: empty", where_(record.line, c + 1, role)),
                        )
                        .with_context(name));
                    }
                    point.id = String::from(value);
                }
                ColumnRole::Northing | ColumnRole::Easting | ColumnRole::Elevation => {
                    if value.is_empty() {
                        if role == ColumnRole::Elevation {
                            continue; // absent, and absent is not zero
                        }
                        return Err(Error::new(
                            ErrorCode::ParseFailure,
                            format!("{}: empty", where_(record.line, c + 1, role)),
                        )
                        .with_context(name));
                    }
                    let Some(number) = parse_finite_double(value) else {
                        return Err(Error::new(
                            ErrorCode::ParseFailure,
                            format!(
                                "{}: {} is not a number",
                                where_(record.line, c + 1, role),
                                quoted_for_message(value, 40)
                            ),
                        )
                        .with_context(name));
                    };
                    let metres = to_metres(number, unit);
                    if role == ColumnRole::Northing {
                        point.northing = metres;
                    } else if role == ColumnRole::Easting {
                        point.easting = metres;
                    } else {
                        point.elevation = Some(metres);
                    }
                }
                ColumnRole::Code => point.code = String::from(value),
                ColumnRole::Description => point.description = String::from(value),
                ColumnRole::Ignore => {}
            }
        }

        if !id_column {
            point.id = format!("{}", record.line);
        }
        match line_of_id.entry(point.id.clone()) {
            btree_map::Entry::Occupied(previous) => {
                let column = layout
                    .columns
                    .iter()
                    .position(|&role| role == ColumnRole::PointId)
                    .map_or(0, |c| c + 1);
                return Err(Error::new(
                    ErrorCode::AlreadyExists,
                    format!(
                        "{}: {} is already the id of the point on line {}",
                        where_(record.line, column, ColumnRole::PointId),
                        quoted_for_message(&point.id, 40),
                        previous.get()
                    ),
                )
                .with_context(name));
            }
            btree_map::Entry::Vacant(slot) => {
                slot.insert(record.line);
            }
        }
        if short_row {
            if short_rows == 0 {
                first_short_row = record.line;
            }
            short_rows += 1;
        }
        if point.elevation.is_none() {
            if without_elevation == 0 {
                first_without_elevation = record.line;
            }
            without_elevation += 1;
        }
        point.source = SourceRecord {
            record_number: record.line,
            ..file_source.clone()
        };
        project.points.push(point);
    }
    let point_count = project.points.len();
    result.records_read = point_count;

    // ---- What a person should know about how the file was read ----
    let comment_lines = reader.comment_lines();
    let first_id = result
        .project
        .points
        .first()
        .map(|point| point.id.clone())
        .unwrap_or_default();
    let warnings = &mut result.warnings;
    if decoded.guessed {
        warnings.push(format!(
            "the text encoding was not stated and was inferred as {}; if a name or code shows the \
             wrong characters, state the encoding and import again",
            katana_core::text_encoding::to_string(decoded.encoding)
        ));
    }
    if point_count == 0 {
        warnings.push(String::from("the file holds no points"));
    }
    if !id_column && point_count != 0 {
        warnings.push(format!(
            "the layout has no point id column, so each point is named by the number of the line \
             it is on (the first is '{first_id}')"
        ));
    }
    if comment_lines != 0 {
        warnings.push(format!(
            "{comment_lines} comment line(s) starting '{}' were skipped",
            layout.comment_prefix
        ));
    }
    if short_rows != 0 {
        warnings.push(format!(
            "{short_rows} line(s) end before the layout's last column, and what they lack is left \
             empty (the first is line {first_short_row})"
        ));
    }
    if !has_role(layout, ColumnRole::Elevation) {
        if point_count != 0 {
            warnings.push(String::from(
                "the layout has no elevation column, so no point has an elevation",
            ));
        }
    } else if without_elevation != 0 {
        warnings.push(format!(
            "{without_elevation} of {point_count} point(s) have no elevation and are imported \
             without one, not at zero (the first is on line {first_without_elevation})"
        ));
    }

    validate_project(&result.project)?;
    Ok(result)
}

// ---- Export ----------------------------------------------------------------------

/// The most decimals export writes. Policy: 12 places of a metre is a
/// picometre, far below any survey measurement; a larger request is a caller's
/// mistake.
pub const MAXIMUM_EXPORT_DECIMALS: i32 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DelimitedExportOptions {
    /// Decimal places for every coordinate, 0 to MAXIMUM_EXPORT_DECIMALS.
    /// REQUIRED: -1 is refused, because how many places a survey is published
    /// to is the surveyor's statement, not this writer's.
    pub decimals: i32,
    /// The unit to write coordinates in. REQUIRED, as on import.
    pub unit: LinearUnit,
}

impl Default for DelimitedExportOptions {
    fn default() -> Self {
        Self {
            decimals: -1,
            unit: LinearUnit::Unknown,
        }
    }
}

// `value` as it has to be written for the reader to give it back exactly, or
// an error saying why it cannot be written with this layout. `what` names the
// value for that error. `starts_line` says that nothing but empty fields is
// written before it on its line - see write_delimited_points.
fn encode_field(
    value: &str,
    starts_line: bool,
    layout: &DelimitedLayout,
    what: &str,
) -> Result<String> {
    let delimiter = delimiter_character(layout.delimiter);
    let runs = layout.delimiter == Delimiter::Whitespace;
    let mut reason = "";
    if let (Some(&first), Some(&last)) = (value.as_bytes().first(), value.as_bytes().last()) {
        if is_ascii_space(first) || is_ascii_space(last) {
            reason = "has blanks at an end, which a reader trims";
        }
        for c in value.bytes() {
            let separates = if runs { is_padding(c) } else { c == delimiter };
            if c == b'\n' || c == b'\r' {
                reason = "holds a line break";
            } else if separates {
                reason = "holds the delimiter";
            } else if c == b'"' && layout.quoting == Quoting::DoubleQuote {
                // Only needed at the start, but quoting every field that holds
                // a quote is what RFC 4180 asks and what other readers expect.
                reason = "holds a quote";
            }
        }
        if starts_line
            && !layout.comment_prefix.is_empty()
            && trimmed_text(value).starts_with(layout.comment_prefix.as_str())
        {
            reason = "begins with the comment prefix, so its line would be skipped";
        }
    } else if runs {
        reason = "is empty, and an empty field between blanks vanishes";
    }
    if reason.is_empty() {
        return Ok(String::from(value));
    }
    if layout.quoting == Quoting::None {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{what} {reason}, and the layout does not allow quoting"),
        ));
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for c in value.chars() {
        if c == '"' {
            quoted.push('"');
        }
        quoted.push(c);
    }
    quoted.push('"');
    Ok(quoted)
}

/// Writes `points` with `layout`: UTF-8 with no byte order mark (instrument
/// upload tools refuse one more often than spreadsheets need one), CRLF line
/// ends as RFC 4180 has them, one header line of header_name()s when
/// layout.header_lines is 1, then one row per point. Coordinates in fixed
/// notation with exactly `decimals` places, written without consulting the
/// locale, and correctly rounded: a value exactly half way, such as 0.125 to
/// two places, goes to the even digit (0.12), as IEEE 754 and printf round it.
/// An absent elevation is an empty field, never 0 - quoted as "" when the
/// delimiter is whitespace, where an unquoted empty field would vanish. A field
/// is quoted when it holds the delimiter, a quote, a line break or blanks at
/// either end, or when it would make the line read as a comment: when it starts
/// with the comment prefix and starts the line - in column 1, or in a tab file
/// after nothing but empty fields, since a reader trims the tabs before them.
///
/// Refused, InvalidArgument naming the point: a layout validate_layout()
/// refuses; header_lines above 1 (this writer produces one header line and
/// nothing to fill others with); decimals or unit unset; a coordinate that is
/// not finite; with an id column, an empty or repeated id; and, with quoting
/// off, a field that would need quotes. Everything refused here is something
/// parse_delimited_points would refuse or misread, so what this writes, that
/// reads back: with the same layout and unit, parse(write(x)) gives x's ids,
/// codes and descriptions exactly, and its coordinates rounded to `decimals`
/// places.
pub fn write_delimited_points(
    points: &[SurveyPoint],
    layout: &DelimitedLayout,
    options: &DelimitedExportOptions,
) -> Result<String> {
    validate_layout(layout)?;
    if layout.header_lines > 1 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the layout skips {} header lines, and this writer writes one; a file it wrote \
                 would lose its first points on reading back",
                layout.header_lines
            ),
        ));
    }
    if options.decimals < 0 || options.decimals > MAXIMUM_EXPORT_DECIMALS {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "choose between 0 and {MAXIMUM_EXPORT_DECIMALS} decimal places ({} was given)",
                options.decimals
            ),
        ));
    }
    let Ok(unit) = metres_per(options.unit) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "state the unit to write coordinates in; none is assumed",
        ));
    };

    // Checked before anything is written, so a refusal names the first bad
    // point rather than leaving half a file.
    if has_role(layout, ColumnRole::PointId) {
        let mut first_index: BTreeMap<&str, usize> = BTreeMap::new();
        for (i, point) in points.iter().enumerate() {
            if point.id.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "point {} in the list has no id, and the layout has an id column",
                        i + 1
                    ),
                ));
            }
            if let Some(&at) = first_index.get(point.id.as_str()) {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "point id '{}' is used twice (points {} and {}), and a file with a \
                         repeated id cannot be read back",
                        point.id,
                        at + 1,
                        i + 1
                    ),
                ));
            }
            first_index.insert(&point.id, i);
        }
    }

    let separator = char::from(delimiter_character(layout.delimiter));
    let mut out = String::new();

    // Whether a field is the one a reader would find the comment prefix at.
    // Not only column 1: the reader looks for the prefix once the line is
    // trimmed of blanks, and a tab is one, so in a tab file the tabs after
    // empty leading fields are trimmed away and "\t#12\t..." reads as a
    // comment. A comma or semicolon is not trimmed and starts the line itself,
    // and with whitespace an empty field is written "", so there column 1 is
    // the one.
    let separator_trimmed = is_ascii_space(delimiter_character(layout.delimiter));
    let mut starts_line = true;

    if layout.header_lines == 1 {
        for (c, &role) in layout.columns.iter().enumerate() {
            let cell = encode_field(
                header_name(role),
                starts_line,
                layout,
                &format!("the header of column {}", c + 1),
            )?;
            if c != 0 {
                out.push(separator);
            }
            out.push_str(&cell);
            starts_line = starts_line && separator_trimmed && cell.is_empty();
        }
        out.push_str("\r\n");
    }

    for (i, point) in points.iter().enumerate() {
        let label = if point.id.is_empty() {
            format!("point {}", i + 1)
        } else {
            format!("point '{}'", point.id)
        };
        starts_line = true;
        for (c, &role) in layout.columns.iter().enumerate() {
            let mut value = String::new();
            let metres = match role {
                ColumnRole::PointId => {
                    value = point.id.clone();
                    None
                }
                ColumnRole::Northing => Some(point.northing),
                ColumnRole::Easting => Some(point.easting),
                // Absent stays absent: an empty field.
                ColumnRole::Elevation => point.elevation,
                ColumnRole::Code => {
                    value = point.code.clone();
                    None
                }
                ColumnRole::Description => {
                    value = point.description.clone();
                    None
                }
                ColumnRole::Ignore => None,
            };
            if let Some(metres) = metres {
                let converted = from_metres(metres, unit);
                if !converted.is_finite() {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        format!("{label}: its {} is not a finite number", to_string(role)),
                    ));
                }
                // Fixed notation, without consulting the locale, so a GUI that
                // set a decimal comma cannot put one in the file; a value that
                // rounds to zero loses its sign ("-0.000"), since there is no
                // such coordinate and a person reading the file would wonder
                // what it meant.
                value = format_fixed(converted, options.decimals);
            }
            let cell = encode_field(
                &value,
                starts_line,
                layout,
                &format!("{label}: its {}", to_string(role)),
            )?;
            if c != 0 {
                out.push(separator);
            }
            out.push_str(&cell);
            starts_line = starts_line && separator_trimmed && cell.is_empty();
        }
        out.push_str("\r\n");
    }
    Ok(out)
}

// ---- The format, as the registry knows it -------------------------------------------

/// Points only - a delimited file has no observations, setups or strings -
/// import and export, parser version 1.0.
pub fn delimited_points_format() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(DELIMITED_POINTS_FORMAT_ID),
        human_name: String::from("Delimited text points (CSV, TXT)"),
        manufacturer: Manufacturer::Generic,
        reads: FormatContent {
            points: true,
            ..Default::default()
        },
        can_import: true,
        can_export: true,
        parser_version: String::from(PARSER_VERSION),
        // What such files are usually called. The probe does not consult them
        // - a .txt is anything at all - so they are for file dialogs, not
        // detection.
        extensions: vec![
            String::from("csv"),
            String::from("tsv"),
            String::from("txt"),
        ],
    }
}

/// The probe's answer for a header naming the coordinates. Policy numbers,
/// ordered so that a header is better evidence than rows of numbers with no
/// header (DELIMITED_HEADERLESS_CONFIDENCE).
pub const DELIMITED_HEADER_CONFIDENCE: f64 = 0.5;
/// The probe's answer for rows of numbers with no header.
pub const DELIMITED_HEADERLESS_CONFIDENCE: f64 = 0.3;

// A specific format that clears the identification bar must also beat this one
// by the margin, whatever this probe says; see probe_delimited_points.
const _: () = assert!(
    DELIMITED_HEADER_CONFIDENCE < IDENTIFIED_CONFIDENCE - IDENTIFICATION_MARGIN,
    "the delimited probe must never be close enough to a confident specific format to make the \
     detection ambiguous"
);
const _: () = assert!(DELIMITED_HEADERLESS_CONFIDENCE < DELIMITED_HEADER_CONFIDENCE);

/// The detection probe. A WEAK candidate by design: rows of numbers separated
/// by commas are consistent with a Leica, Trimble or Topcon CSV export and with
/// any number of formats nobody has registered, so this probe never claims a
/// file. Its highest answer is below IDENTIFIED_CONFIDENCE -
/// IDENTIFICATION_MARGIN (detect.rs), which means any specific format confident
/// enough to be identified also beats this one by the margin, and a file only
/// this probe recognises comes back Uncertain for a person to confirm.
pub fn probe_delimited_points(input: &ProbeInput<'_>) -> FormatSignature {
    let Ok(text) = crate::delimited_proposal::decode_sample(input.bytes, input.truncated) else {
        return ruled_out();
    };
    let analysis = crate::delimited_proposal::analyse(&text);
    let Some(delimiter) = analysis.delimiter else {
        return ruled_out();
    };
    if analysis.proposal.candidates().is_empty() {
        return ruled_out();
    }
    let evidence = format!("{}-delimited rows of numbers", to_string(delimiter));
    if analysis.header_names_coordinates {
        return FormatSignature {
            confidence: DELIMITED_HEADER_CONFIDENCE,
            evidence: format!(
                "{evidence} under a header naming the coordinate columns; a generic reading, \
                 which any specific format outranks"
            ),
        };
    }
    FormatSignature {
        confidence: DELIMITED_HEADERLESS_CONFIDENCE,
        evidence: format!(
            "{evidence} with no header naming the columns, so the coordinate order is not stated"
        ),
    }
}

/// This format's line in the program's registry (format.rs).
pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(delimited_points_format(), probe_delimited_points, None)
}
