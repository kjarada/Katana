//! What the two front ends share about IFC, so that a line typed at
//! katana_cli, the same line typed in the window, and the window's IFC dialogs
//! all mean the same file (docs/ifc.md, "Using it"):
//!
//! * the grammar of IMPORT <file.ifc> and EXPORT <file.ifc> ...;
//! * reading what an export names - the utility schedule, its delivery
//!   schema, a project's classification rules;
//! * the project's GlobalId namespace and the header's timestamp, so that
//!   both front ends give an object the same GlobalId.
//!
//! Here and not in either front end because each may see katana_ifc and
//! neither may see the other (katana_cad may not see ifc at all), and because
//! what is here can be tested without a window or a process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::path_text;
use katana_core::text::{
    format_exact_real, lowered, parse_finite_double, parse_integer, read_reply_record,
    reply_quoted, split_lines,
};
use katana_geometry::Box2;
use katana_survey::subsurface::{DeliverySchema, parse_delivery_schema, parse_utility_csv};

use crate::classification::{
    ClassificationRule, default_classification_rules, format_classification_rules,
    parse_classification_rules,
};
use crate::export::{ClassTally, IfcExport, UtilityInput, is_ifc_path};
use crate::import::IfcImport;
use crate::support::{file_name_text, text_of, trimmed, upper};

/// EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]
/// [RULES <rules.csv>] [SPACING <m>] [NODRAWING] [NOENTITIES] [SELECTED]
/// [NOALIGNMENTS] [NOSURFACES] [NOROADMODEL] [PREVIEW]
///
/// Every choice File > Export IFC offers is a word here, so that the dialog
/// only writes the line (format_export_line) and a person, a script and an
/// agent can each ask for the same file.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportArguments {
    /// the file to write, UTF-8, as given
    pub path: String,
    /// UTILITIES: an AS 5488 schedule
    pub schedule: Option<String>,
    /// SCHEMA: the delivery schema it was written to
    pub schema: Option<String>,
    /// RULES: a project's classification rules
    pub rules: Option<String>,
    /// SPACING: longest detected spacing keeping QL-B, m
    pub spacing: Option<f64>,
    // NODRAWING is all four false: the investigation alone.
    /// false: NOENTITIES
    pub entities: bool,
    /// false: NOALIGNMENTS
    pub alignments: bool,
    /// false: NOSURFACES; the drawing's surfaces (Document::surfaces)
    pub surfaces: bool,
    /// false: NOROADMODEL - the road model's 3D objects (corridors, road
    /// objects), made from the drawing's definitions, as IfcRoad and its parts
    pub road_model: bool,
    /// SELECTED: of the entities, the selected ones
    pub selected: bool,
    /// PREVIEW: the reply the file would give, no file written
    pub preview: bool,
}

impl Default for ExportArguments {
    fn default() -> Self {
        Self {
            path: String::new(),
            schedule: None,
            schema: None,
            rules: None,
            spacing: None,
            entities: true,
            alignments: true,
            surfaces: true,
            road_model: true,
            selected: false,
            preview: false,
        }
    }
}

/// IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]
/// [TOLERANCE <m>] [TAKECRS | KEEPCRS]
#[derive(Clone, Debug, PartialEq)]
pub struct ImportArguments {
    pub path: String,
    /// LOCAL: moved to sit at the origin
    pub local: bool,
    /// false: NOALIGNMENTS
    pub alignments: bool,
    /// false: NOELEMENTS - elements and annotations
    pub elements: bool,
    /// false: NOSURFACES
    pub surfaces: bool,
    /// TOLERANCE: ImportOptions::curve_tolerance, m
    pub tolerance: Option<f64>,
    /// TAKECRS: the project takes the file's coordinate system when it has
    /// none; KEEPCRS: it does not. Neither: katana_cli and a typed line say
    /// how (CRS SET), and File > Import asks.
    pub take_coordinate_system: Option<bool>,
}

impl Default for ImportArguments {
    fn default() -> Self {
        Self {
            path: String::new(),
            local: false,
            alignments: true,
            elements: true,
            surfaces: true,
            tolerance: None,
            take_coordinate_system: None,
        }
    }
}

/// Blank-separated words, a double-quoted word kept whole with its blanks.
/// None for a quote that is never closed.
fn words(text: &str) -> Option<Vec<String>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b' ' || bytes[at] == b'\t' {
            at += 1;
            continue;
        }
        if bytes[at] == b'"' {
            let end = bytes[at + 1..].iter().position(|&c| c == b'"')? + at + 1;
            out.push(String::from(&text[at + 1..end]));
            at = end + 1;
            continue;
        }
        let end = bytes[at..]
            .iter()
            .position(|&c| c == b' ' || c == b'\t')
            .map_or(bytes.len(), |offset| at + offset);
        out.push(String::from(&text[at..end]));
        at = end;
    }
    Some(out)
}

/// The first ".ifc" in `argument`, by any letter case, that ends a word: the
/// byte index just past it.
fn ifc_word_end(argument: &str) -> Option<usize> {
    let lower = lowered(argument);
    let mut from = 0;
    while let Some(found) = crate::support::find_from(&lower, b".ifc", from) {
        let end = found + 4;
        if end == lower.len() || lower[end] == b' ' || lower[end] == b'\t' {
            return Some(end);
        }
        from = found + 1;
    }
    None
}

/// The path, and what follows it: a quoted path, or everything up to the
/// first ".ifc" that ends a word. None when there is no .ifc; an error for a
/// quote opened before a .ifc and never closed, which would otherwise pass to
/// the other formats' grammar and be written under a name starting with the
/// quote.
fn split_path(argument: &str) -> Option<Result<(String, &str)>> {
    let argument = trimmed(argument);
    if let Some(rest) = argument.strip_prefix('"') {
        let Some(end) = rest.find('"') else {
            if ifc_word_end(argument).is_some() {
                return Some(Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a quoted path is never closed",
                )));
            }
            return None;
        };
        let path = String::from(&rest[..end]);
        if !is_ifc_path(&path_from_utf8(&path)) {
            return None;
        }
        return Some(Ok((path, &rest[end + 1..])));
    }
    let end = ifc_word_end(argument)?;
    Some(Ok((String::from(&argument[..end]), &argument[end..])))
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidArgument, message)
}

/// The file's whole text; None when it cannot be read. Bytes that are not
/// UTF-8 become U+FFFD (the C++ keeps them), which is what the writer makes
/// of them in the file anyway.
fn read_file(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(text_of)
}

/// `error`, with the file it is in named, as read_ifc_file names its own.
fn in_file(mut error: Error, path: &str) -> Error {
    error.context = if error.context.is_empty() {
        String::from(path)
    } else {
        format!("{path}: {}", error.context)
    };
    error
}

/// A path given as text: core's path_from_utf8 (path_text), under the name
/// the IFC verbs and both front ends already call it by. UTF-8 is read as
/// UTF-8, and bytes that are not - a name from an ANSI console - as the
/// narrow name they are.
pub fn path_from_utf8(text: impl AsRef<[u8]>) -> PathBuf {
    path_text::path_from_utf8(text)
}

const EXPORT_OPTIONS: &str = "UTILITIES <schedule.csv>, SCHEMA <schema.csv>, RULES <rules.csv>, \
                              SPACING <m>, NODRAWING, NOENTITIES, SELECTED, NOALIGNMENTS, \
                              NOSURFACES, NOROADMODEL, PREVIEW";
const IMPORT_OPTIONS: &str =
    "LOCAL, NOALIGNMENTS, NOELEMENTS, NOSURFACES, TOLERANCE <m>, TAKECRS, KEEPCRS";

/// A positive number of metres, or why not.
fn metres(word: &str, option: &str) -> Result<f64> {
    match parse_finite_double(word) {
        Some(value) if value > 0.0 => Ok(value),
        _ => Err(invalid(format!(
            "{option} takes a positive number of metres"
        ))),
    }
}

/// The words after the path, and the keywords met so far: a keyword given
/// twice is refused, since the second would silently undo or repeat the
/// first.
struct Keywords {
    words: Vec<String>,
    seen: Vec<String>,
}

impl Keywords {
    /// True when `keyword` was met before; otherwise it is remembered.
    fn again(&mut self, keyword: &str) -> bool {
        if self.seen.iter().any(|seen| seen == keyword) {
            return true;
        }
        self.seen.push(String::from(keyword));
        false
    }
}

/// The rest of an EXPORT line, after the verb. None when it names no .ifc -
/// a path in quotes, or everything up to the first ".ifc" that ends a word,
/// so that an unquoted path with blanks still reads - and the caller's other
/// exporters take the line. Otherwise the arguments, or InvalidArgument
/// saying what is wrong: an unclosed quote, an unknown word, a word given
/// twice, SCHEMA without UTILITIES, SELECTED without the entities, a SPACING
/// that is not a positive number, a quote opened before a .ifc and never
/// closed. Words are matched in any letter case; a value may be
/// double-quoted.
pub fn parse_export_arguments(argument: &str) -> Option<Result<ExportArguments>> {
    let (path, rest) = match split_path(argument)? {
        Ok(split) => split,
        Err(error) => return Some(Err(error)),
    };
    let mut out = ExportArguments {
        path,
        ..ExportArguments::default()
    };
    let Some(found) = words(rest) else {
        return Some(Err(invalid("a quoted path is never closed")));
    };
    let mut keywords = Keywords {
        words: found,
        seen: Vec::new(),
    };
    let mut no_drawing = false;
    let mut i = 0;
    while i < keywords.words.len() {
        let keyword = upper(&keywords.words[i]);
        let has_value = i + 1 < keywords.words.len();
        let known = matches!(
            keyword.as_str(),
            "UTILITIES"
                | "SCHEMA"
                | "RULES"
                | "SPACING"
                | "NODRAWING"
                | "NOENTITIES"
                | "SELECTED"
                | "NOALIGNMENTS"
                | "NOSURFACES"
                | "NOROADMODEL"
                | "PREVIEW"
        );
        let takes_value = matches!(
            keyword.as_str(),
            "UTILITIES" | "SCHEMA" | "RULES" | "SPACING"
        );
        if !known || (takes_value && !has_value) {
            return Some(Err(invalid(format!(
                "\"{}\" is not an option of EXPORT <file.ifc>: {EXPORT_OPTIONS}",
                keywords.words[i]
            ))));
        }
        if keywords.again(&keyword) {
            return Some(Err(invalid(format!("{keyword} is given twice"))));
        }
        match keyword.as_str() {
            "UTILITIES" => {
                i += 1;
                out.schedule = Some(keywords.words[i].clone());
            }
            "SCHEMA" => {
                i += 1;
                out.schema = Some(keywords.words[i].clone());
            }
            "RULES" => {
                i += 1;
                out.rules = Some(keywords.words[i].clone());
            }
            "SPACING" => {
                i += 1;
                match metres(&keywords.words[i], "SPACING") {
                    Ok(spacing) => out.spacing = Some(spacing),
                    Err(error) => return Some(Err(error)),
                }
            }
            "NODRAWING" => {
                no_drawing = true;
                out.entities = false;
                out.alignments = false;
                out.surfaces = false;
                out.road_model = false;
            }
            "NOENTITIES" => out.entities = false,
            "SELECTED" => out.selected = true,
            "NOALIGNMENTS" => out.alignments = false,
            "NOSURFACES" => out.surfaces = false,
            "NOROADMODEL" => out.road_model = false,
            _ => out.preview = true,
        }
        i += 1;
    }
    if out.schema.is_some() && out.schedule.is_none() {
        return Some(Err(invalid(
            "SCHEMA describes a schedule: give it with UTILITIES",
        )));
    }
    if out.selected && !out.entities {
        return Some(Err(invalid(format!(
            "SELECTED chooses among the entities, and {} leaves them out",
            if no_drawing {
                "NODRAWING"
            } else {
                "NOENTITIES"
            }
        ))));
    }
    Some(Ok(out))
}

/// The rest of an IMPORT line, after the verb: as parse_export_arguments, and
/// InvalidArgument for a TOLERANCE that is not a positive number, TAKECRS
/// with KEEPCRS or with LOCAL (data moved to the origin is in no system), or
/// an import that leaves out everything.
pub fn parse_import_arguments(argument: &str) -> Option<Result<ImportArguments>> {
    let (path, rest) = match split_path(argument)? {
        Ok(split) => split,
        Err(error) => return Some(Err(error)),
    };
    let mut out = ImportArguments {
        path,
        ..ImportArguments::default()
    };
    let Some(found) = words(rest) else {
        return Some(Err(invalid("a quoted path is never closed")));
    };
    let mut keywords = Keywords {
        words: found,
        seen: Vec::new(),
    };
    let mut i = 0;
    while i < keywords.words.len() {
        let keyword = upper(&keywords.words[i]);
        let known = matches!(
            keyword.as_str(),
            "LOCAL"
                | "NOALIGNMENTS"
                | "NOELEMENTS"
                | "NOSURFACES"
                | "TOLERANCE"
                | "TAKECRS"
                | "KEEPCRS"
        );
        if !known || (keyword == "TOLERANCE" && i + 1 == keywords.words.len()) {
            return Some(Err(invalid(format!(
                "\"{}\" is not an option of IMPORT <file.ifc>: {IMPORT_OPTIONS}",
                keywords.words[i]
            ))));
        }
        if keywords.again(&keyword) {
            return Some(Err(invalid(format!("{keyword} is given twice"))));
        }
        match keyword.as_str() {
            "LOCAL" => out.local = true,
            "NOALIGNMENTS" => out.alignments = false,
            "NOELEMENTS" => out.elements = false,
            "NOSURFACES" => out.surfaces = false,
            "TOLERANCE" => {
                i += 1;
                match metres(&keywords.words[i], "TOLERANCE") {
                    Ok(tolerance) => out.tolerance = Some(tolerance),
                    Err(error) => return Some(Err(error)),
                }
            }
            _ => {
                if out.take_coordinate_system.is_some() {
                    return Some(Err(invalid(
                        "TAKECRS and KEEPCRS say opposite things: give one",
                    )));
                }
                out.take_coordinate_system = Some(keyword == "TAKECRS");
            }
        }
        i += 1;
    }
    if !out.alignments && !out.elements && !out.surfaces {
        return Some(Err(invalid(
            "NOALIGNMENTS, NOELEMENTS and NOSURFACES together leave nothing to import",
        )));
    }
    if out.local && out.take_coordinate_system.unwrap_or(false) {
        return Some(Err(invalid(
            "LOCAL moves the data out of the file's coordinate system, which TAKECRS would give \
             the project",
        )));
    }
    Some(Ok(out))
}

/// A path or a word as the grammar reads it back: always in double quotes,
/// so that blanks, a ".ifc " inside a folder's name and a keyword-like name
/// all read as the path. A double quote cannot be said inside one.
fn quoted_word(text: &str) -> Result<String> {
    if text.contains('"') {
        return Err(invalid(format!(
            "\"{text}\" holds a double quote, which a command line cannot quote"
        )));
    }
    Ok(format!("\"{text}\""))
}

/// The whole line that says `arguments` - "EXPORT \"site.ifc\" NOSURFACES" -
/// every path quoted and every number as it reads back, so that the parse of
/// the line is `arguments` again. What a dialog hands the command line.
/// InvalidArgument for a path holding a double quote, which a line cannot
/// say.
pub fn format_export_line(arguments: &ExportArguments) -> Result<String> {
    let mut line = format!("EXPORT {}", quoted_word(&arguments.path)?);
    for (keyword, value) in [
        ("UTILITIES", &arguments.schedule),
        ("SCHEMA", &arguments.schema),
        ("RULES", &arguments.rules),
    ] {
        if let Some(value) = value {
            line.push_str(&format!(" {keyword} {}", quoted_word(value)?));
        }
    }
    if let Some(spacing) = arguments.spacing {
        line.push_str(&format!(" SPACING {}", format_exact_real(spacing)));
    }
    if !arguments.entities && !arguments.alignments && !arguments.surfaces && !arguments.road_model
    {
        line.push_str(" NODRAWING");
    } else {
        if !arguments.entities {
            line.push_str(" NOENTITIES");
        }
        if !arguments.alignments {
            line.push_str(" NOALIGNMENTS");
        }
        if !arguments.surfaces {
            line.push_str(" NOSURFACES");
        }
        if !arguments.road_model {
            line.push_str(" NOROADMODEL");
        }
    }
    if arguments.selected {
        line.push_str(" SELECTED");
    }
    if arguments.preview {
        line.push_str(" PREVIEW");
    }
    Ok(line)
}

pub fn format_import_line(arguments: &ImportArguments) -> Result<String> {
    let mut line = format!("IMPORT {}", quoted_word(&arguments.path)?);
    if arguments.local {
        line.push_str(" LOCAL");
    }
    if !arguments.alignments {
        line.push_str(" NOALIGNMENTS");
    }
    if !arguments.elements {
        line.push_str(" NOELEMENTS");
    }
    if !arguments.surfaces {
        line.push_str(" NOSURFACES");
    }
    if let Some(tolerance) = arguments.tolerance {
        line.push_str(&format!(" TOLERANCE {}", format_exact_real(tolerance)));
    }
    if let Some(take) = arguments.take_coordinate_system {
        line.push_str(if take { " TAKECRS" } else { " KEEPCRS" });
    }
    Ok(line)
}

/// INFO "<file.ifc>": what the import dialog's Describe runs.
pub fn format_info_line(path: &str) -> Result<String> {
    Ok(format!("INFO {}", quoted_word(path)?))
}

/// IFC RULES <file.csv>: the default classification rules written to a file,
/// a project's starting point for its own (RULES). The path after IFC RULES,
/// None when the line is not one; InvalidArgument for a missing path, an
/// unclosed quote or a word after the path.
pub fn parse_rules_arguments(argument: &str) -> Option<Result<String>> {
    let Some(found) = words(argument) else {
        return Some(Err(invalid("a quoted path is never closed")));
    };
    if found.first().is_none_or(|first| upper(first) != "RULES") {
        return None;
    }
    if found.len() != 2 {
        return Some(Err(invalid("usage: IFC RULES <file.csv>")));
    }
    Some(Ok(found[1].clone()))
}

pub fn format_rules_line(path: &str) -> Result<String> {
    Ok(format!("IFC RULES {}", quoted_word(path)?))
}

/// Writes them, and answers "ifc rules file=... rules=N"; FileExportFailure
/// naming the file when it cannot be written.
pub fn write_default_rules(path: &str) -> Result<String> {
    let file = path_from_utf8(path);
    let rules = default_classification_rules();
    if std::fs::write(&file, format_classification_rules(rules)).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the rules could not be written",
            path,
        ));
    }
    Ok(format!(
        "ifc rules file={} rules={}",
        reply_quoted(&file_name_text(&file)),
        rules.len()
    ))
}

/// What an export names, read and checked, ready for write_ifc: the graded
/// investigation, the delivery schema it points into, and the rules.
#[derive(Clone, Debug, Default)]
pub struct ExportFiles {
    pub utilities: Option<UtilityInput>,
    /// Shared with `utilities`, which refers to it however this is moved.
    pub schema: Option<Arc<DeliverySchema>>,
    /// A RULES file's rules, then default_classification_rules(): a project
    /// names the exceptions, and everything else is classified as it would
    /// have been. Empty without a RULES file, which write_ifc reads as the
    /// defaults.
    pub rules: Vec<ClassificationRule>,
}

/// Reads the schedule, schema and rules `arguments` name. NotFound naming a
/// file that cannot be read; the reader's own error, with the file named, for
/// one that does not parse; InvalidArgument for SCHEMA without UTILITIES.
pub fn read_export_files(arguments: &ExportArguments) -> Result<ExportFiles> {
    if arguments.schema.is_some() && arguments.schedule.is_none() {
        return Err(invalid(
            "SCHEMA describes a schedule: give it with UTILITIES",
        ));
    }
    let mut out = ExportFiles::default();
    if let Some(schedule) = &arguments.schedule {
        let path = path_from_utf8(schedule);
        let Some(text) = read_file(&path) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("cannot read {schedule}"),
            ));
        };
        let lines = parse_utility_csv(&text).map_err(|error| in_file(error, schedule))?;
        let mut utilities = UtilityInput {
            lines,
            source_name: file_name_text(&path),
            ..UtilityInput::default()
        };
        if let Some(spacing) = arguments.spacing {
            utilities.grading.maximum_detected_spacing = spacing;
        }
        if let Some(schema) = &arguments.schema {
            let Some(schema_text) = read_file(&path_from_utf8(schema)) else {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    format!("cannot read {schema}"),
                ));
            };
            let parsed =
                parse_delivery_schema(&schema_text).map_err(|error| in_file(error, schema))?;
            let shared = Arc::new(parsed);
            out.schema = Some(Arc::clone(&shared));
            utilities.schema = Some(shared);
        }
        out.utilities = Some(utilities);
    }
    if let Some(rules) = &arguments.rules {
        let Some(text) = read_file(&path_from_utf8(rules)) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("cannot read {rules}"),
            ));
        };
        out.rules = parse_classification_rules(&text).map_err(|error| in_file(error, rules))?;
        out.rules.extend_from_slice(default_classification_rules());
    }
    Ok(out)
}

/// What seeds every GlobalId of a project's exports (ExportOptions::
/// guid_namespace): its name and the time it was created, so that the same
/// project gives each object the same GlobalId from either front end and in
/// every session, and two projects of one name do not collide.
pub fn guid_namespace_for(project_name: &str, created_utc: &str) -> String {
    format!("{project_name}/{created_utc}")
}

// ---- replies ------------------------------------------------------------------------
//
// What EXPORT, IMPORT and INFO answer, as key=value records one a line
// (core::reply_quoted, docs/ifc.md "Replies"), the same from katana_cli, the
// window's command line and a dialog:
//
//   ifc exported file="site.ifc" schema=IFC4X3_ADD2 instances=1097 bytes=82914
//     (PREVIEW: "ifc previewed", and no bytes)
//   counts alignments=1 services=4 segments=10 segments_3d=6 located_points=14
//          entities_written=5 entities_skipped=0 surfaces=0
//   road elements=12 skipped=0 roads=1 parts=3      when a road model was given
//   class name=IfcKerb count=1                      one a class written
//   object from="layer Survey/Kerb" count=1 class=IfcKerb predefined=NOTDEFINED
//          object_type="" system="" why="rule kerb" one a ClassTally
//   warning text="..."
//
//   ifc imported file="site.ifc" schema=IFC4X3_ADD2 crs=EPSG:7856 entities=29
//          alignments=1 surfaces=0 objects=29 drawn=29 as_points=0
//          alignments_as_polylines=0
//   ifc described ...                               INFO: the same, and
//   alignment name="MC01" pis=4 pvis=4              one a reconstructed alignment
//   surface name="Terrain" triangles=2
//   extent min_x=... min_y=... max_x=... max_y=...  metres, to the millimetre
//
// A front end adds its own records after them: "note text=..." for what it
// did or says to do (the coordinate system, a shift).

fn class_records(classes: &std::collections::BTreeMap<String, usize>) -> String {
    let mut out = String::new();
    for (name, count) in classes {
        out.push_str(&format!("\nclass name={name} count={count}"));
    }
    out
}

fn warning_records(warnings: &[String]) -> String {
    let mut out = String::new();
    for warning in warnings {
        out.push('\n');
        out.push_str(&warning_record(warning));
    }
    out
}

/// Metres to the millimetre, the precision a survey states a position to.
/// (std::format's "{:.3f}" is Rust's "{:.3}" for a finite value, the sign of
/// a negative zero kept by both.)
fn extent_record(bounds: &Box2) -> String {
    if bounds.empty() {
        return String::new();
    }
    format!(
        "\nextent min_x={:.3} min_y={:.3} max_x={:.3} max_y={:.3}",
        bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y
    )
}

/// What a read found, the head of both an import's reply and a description.
fn read_fields(read: &IfcImport, file_name: &str, entities: usize) -> String {
    format!(
        "file={} schema={} crs={} entities={entities} alignments={} surfaces={} objects={} \
         drawn={} as_points={} alignments_as_polylines={}",
        reply_quoted(file_name),
        read.schema,
        reply_quoted(&read.coordinate_system),
        read.alignments.len(),
        read.surfaces.len(),
        read.products,
        read.products_imported,
        read.products_as_points,
        read.alignments_as_polylines
    )
}

/// "note text=...".
pub fn note_record(text: &str) -> String {
    format!("note text={}", reply_quoted(text))
}

/// "warning text=...".
pub fn warning_record(text: &str) -> String {
    format!("warning text={}", reply_quoted(text))
}

pub fn format_export_reply(report: &IfcExport, file_name: &str, preview: bool) -> String {
    let mut out = if preview {
        format!(
            "ifc previewed file={} schema=IFC4X3_ADD2 instances={}",
            reply_quoted(file_name),
            report.instances
        )
    } else {
        format!(
            "ifc exported file={} schema=IFC4X3_ADD2 instances={} bytes={}",
            reply_quoted(file_name),
            report.instances,
            report.bytes_written
        )
    };
    out.push_str(&format!(
        "\ncounts alignments={} services={} segments={} segments_3d={} located_points={} \
         entities_written={} entities_skipped={} surfaces={}",
        report.alignments,
        report.services,
        report.service_segments,
        report.segments_in_3d,
        report.located_points,
        report.entities_written,
        report.entities_skipped,
        report.surfaces
    ));
    // Only when a road model was given, so that the reply of an export
    // without one is the record it always was.
    if report.road_elements + report.road_elements_skipped > 0 {
        out.push_str(&format!(
            "\nroad elements={} skipped={} roads={} parts={}",
            report.road_elements, report.road_elements_skipped, report.roads, report.road_parts
        ));
    }
    out.push_str(&class_records(&report.classes));
    for row in &report.tally {
        out.push_str(&format!(
            "\nobject from={} count={} class={} predefined={} object_type={} system={} why={}",
            reply_quoted(&row.source),
            row.count,
            reply_quoted(&row.entity),
            reply_quoted(&row.predefined_type),
            reply_quoted(&row.object_type),
            reply_quoted(&row.system),
            reply_quoted(&row.why)
        ));
    }
    out + &warning_records(&report.warnings)
}

/// An import's reply is written after import_command, whose renames are among
/// the warnings; `entities` is how many it brought, counted before the
/// command moved them out.
pub fn format_import_reply(imported: &IfcImport, file_name: &str, entities: usize) -> String {
    format!(
        "ifc imported {}{}{}{}",
        read_fields(imported, file_name, entities),
        class_records(&imported.classes),
        extent_record(&imported.bounds),
        warning_records(&imported.warnings)
    )
}

pub fn format_description(read: &IfcImport, file_name: &str) -> String {
    let mut out = format!(
        "ifc described {} layers={}{}",
        read_fields(read, file_name, read.entities.len()),
        read.layers.len(),
        class_records(&read.classes)
    );
    for alignment in &read.alignments {
        out.push_str(&format!(
            "\nalignment name={} pis={} pvis={}",
            reply_quoted(&alignment.name),
            alignment.horizontal.pis.len(),
            alignment
                .vertical
                .as_ref()
                .map_or(0, |vertical| vertical.pvis.len())
        ));
    }
    for surface in &read.surfaces {
        out.push_str(&format!(
            "\nsurface name={} triangles={}",
            reply_quoted(&surface.name),
            surface.surface.triangle_count()
        ));
    }
    out + &extent_record(&read.bounds) + &warning_records(&read.warnings)
}

/// The object records of an EXPORT reply as the tally they were written from:
/// the one reader of what format_export_reply writes, which File > Export
/// IFC's table is filled from. Other records are passed over; ParseFailure
/// naming the line for an object record that does not read.
pub fn read_export_objects(reply: &str) -> Result<Vec<ClassTally>> {
    let mut out = Vec::new();
    for (index, line) in split_lines(reply.as_bytes()).into_iter().enumerate() {
        let number = index + 1;
        // The reply is text and its lines end at ASCII bytes, so each line is
        // text too.
        let Ok(line) = std::str::from_utf8(line) else {
            continue;
        };
        let Some(record) = read_reply_record(line).filter(|record| record.words == ["object"])
        else {
            continue;
        };
        let parsed = record.value("count").and_then(parse_integer);
        let (Some(parsed), Some(from), Some(entity)) = (
            parsed.filter(|&count| count >= 0),
            record.value("from"),
            record.value("class"),
        ) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "an object record does not read",
                format!("line {number}"),
            ));
        };
        out.push(ClassTally {
            source: String::from(from),
            count: parsed as usize,
            entity: String::from(entity),
            predefined_type: String::from(record.value("predefined").unwrap_or("")),
            object_type: String::from(record.value("object_type").unwrap_or("")),
            system: String::from(record.value("system").unwrap_or("")),
            why: String::from(record.value("why").unwrap_or("")),
        });
    }
    Ok(out)
}

/// Days from 1970-01-01 to the civil date: Howard Hinnant's
/// civil_from_days ("chrono-Compatible Low-Level Date Algorithms"), the
/// proleptic Gregorian calendar std::chrono uses.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// `when` as the header's time stamp, ISO 8601 in UTC to the second:
/// "2026-09-25T16:01:09". The writer reads no clock (Rule 7); the caller
/// passes its own. Truncated to the second towards the past, as
/// std::chrono::floor is.
pub fn header_timestamp(when: SystemTime) -> String {
    let seconds = match when.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => {
            let before = before.duration();
            -(before.as_secs() as i64) - i64::from(before.subsec_nanos() > 0)
        }
    };
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}
