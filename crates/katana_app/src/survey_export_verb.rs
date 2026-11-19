//! SURVEY EXPORT: the drawing's survey points - point entities with a point
//! number - written to a delimited point file (CSV, TXT) in a column layout,
//! from a line (docs/survey.md, "SURVEY EXPORT"). What Survey > Export Survey
//! Points does, as a verb: katana_cli, katana_mcp and the window's command
//! line run this function, and the dialog's choices are its words.
//!
//! ```text
//! SURVEY EXPORT <file> [LAYOUT <template>] [DECIMALS <n>] [UNIT <unit>]
//!               [<scope> [WHERE <condition> ...]]
//! ```
//!
//! - LAYOUT is a layout template, the text a saved template holds
//!   (surveyio's parse_layout_template): "P,E,N,Z,C,D;delimiter=comma;
//!   header=1;quote=double" - the dialog's own default, which is also this
//!   verb's.
//! - DECIMALS, 0 to 12, the places every coordinate is written to (3 by
//!   default: the millimetre survey coordinates are quoted to).
//! - UNIT is metres, feet, usfeet or links (metres by default).
//! - The scope is the shared grammar (cad's parse_scope_words, resolved by
//!   match_scope), as EXPORT's: no scope word is every survey point of the
//!   drawing; the points among what the scope took otherwise.
//!
//! Here, in the app layer, because surveyio writes the file and cad may not
//! see surveyio (tools/check_layering.cmake).
//!
//! The reply is records, one per line:
//!
//! ```text
//! exported file=pts.csv kind=survey-points points=12 layout="P,E,N,Z,C,D;delimiter=comma;header=1;quote=double" decimals=3 unit=metres without_elevation=1 not_points=0 bytes=402
//! scope scope=layers layers=survey sublayers=yes matched=14   (when a scope was given)
//! export file=pts.csv kind=survey-points ran=no reason="the scope took nothing"
//! ```
//!
//! `without_elevation` counts the points written with an empty height field
//! (absent is not zero); `not_points` what the scope took that is no survey
//! point and was not written.

use std::path::{Path, PathBuf};

use katana_cad::scope_verbs::{ScopeWords, is_scope_word, match_scope, parse_scope_words, scope_record};
use katana_cad::survey_points::{
    DrawingSurveyPoint, drawing_survey_points, survey_points_among, to_survey_point,
};
use katana_cad::{CommandInterpreter, Document, SurveyImportOptions};
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, parse_integer};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_survey::LinearUnit;
use katana_surveyio::{
    DelimitedExportOptions, MAXIMUM_EXPORT_DECIMALS, layout_template, parse_layout_template,
    write_delimited_points,
};

use crate::import_records::{path_from_text, path_text, record_text};

const USAGE: &str = "SURVEY EXPORT <file> [LAYOUT <template>] [DECIMALS <n>] \
                     [UNIT metres|feet|usfeet|links] [<scope> [WHERE ...]]";

/// The layout Survey > Export Survey Points starts with: point, easting,
/// northing, height, code, description, comma-separated under a header of
/// their names, a field quoted where it must be.
pub const DEFAULT_LAYOUT: &str = "P,E,N,Z,C,D;delimiter=comma;header=1;quote=double";

/// The places a coordinate is written to unless DECIMALS says: the
/// millimetre, as the dialog starts.
pub const DEFAULT_DECIMALS: i32 = 3;

// The unit words, each the one word for a unit of survey's LinearUnit that a
// coordinate may be written in (the dialog's Write In choice).
const UNITS: [(&str, LinearUnit); 4] = [
    ("metres", LinearUnit::Metres),
    ("feet", LinearUnit::Feet),
    ("usfeet", LinearUnit::UsSurveyFeet),
    ("links", LinearUnit::Links),
];

fn refused(why: impl Into<String>) -> Error {
    Error::new(
        ErrorCode::InvalidArgument,
        format!("{}; usage: {USAGE}", why.into()),
    )
}

fn same_word(a: &str, b: &str) -> bool {
    equals_ignoring_case(a.as_bytes(), b.as_bytes())
}

/// The word for `unit` on a line: metres, feet, usfeet or links.
pub fn unit_word(unit: LinearUnit) -> &'static str {
    UNITS
        .iter()
        .find(|(_, each)| *each == unit)
        .map_or("metres", |(word, _)| word)
}

/// The unit a line's word names, in any case; None for any other word. The
/// one reading of UNIT, for SURVEY EXPORT and for SURVEY IMPORT's LAYOUT.
pub fn unit_named(word: &str) -> Option<LinearUnit> {
    UNITS
        .iter()
        .find(|(name, _)| same_word(word, name))
        .map(|(_, unit)| *unit)
}

/// A SURVEY EXPORT line, read.
#[derive(Clone, Debug, PartialEq)]
pub struct SurveyExportLine {
    pub file: String,
    /// The template text as given, or DEFAULT_LAYOUT.
    pub layout: String,
    pub decimals: i32,
    pub unit: LinearUnit,
    /// None: every survey point of the drawing.
    pub scope: Option<ScopeWords>,
}

/// True for a SURVEY EXPORT line.
pub fn is_survey_export_line(words: &[String]) -> bool {
    words.len() >= 2 && same_word(&words[0], "SURVEY") && same_word(&words[1], "EXPORT")
}

/// Reads a SURVEY EXPORT line. InvalidArgument, with the usage, for a line the
/// grammar does not take: no file, an option given twice or without its
/// value, a word that is no option, DECIMALS outside 0 to 12, a unit not
/// named; the scope parser's own refusal for scope words that do not read.
pub fn parse_survey_export(line: &str) -> Result<SurveyExportLine> {
    let words = CommandInterpreter::tokenize(line)?;
    if !is_survey_export_line(&words) {
        return Err(refused("not a SURVEY EXPORT line"));
    }
    let Some(file) = words.get(2).filter(|file| !file.trim().is_empty()) else {
        return Err(refused("SURVEY EXPORT needs the file to write"));
    };
    let mut parsed = SurveyExportLine {
        file: file.clone(),
        layout: String::from(DEFAULT_LAYOUT),
        decimals: DEFAULT_DECIMALS,
        unit: LinearUnit::Metres,
        scope: None,
    };
    let (mut layout, mut decimals, mut unit) = (false, false, false);
    let mut i = 3;
    while i < words.len() {
        let word = &words[i];
        if is_scope_word(word) {
            if parsed.scope.is_some() {
                return Err(refused("one scope per line"));
            }
            parsed.scope = Some(parse_scope_words(&words, &mut i)?);
            continue;
        }
        let option = ["LAYOUT", "DECIMALS", "UNIT"]
            .into_iter()
            .find(|option| same_word(word, option));
        let Some(option) = option else {
            return Err(refused(format!(
                "'{word}' is not an option of SURVEY EXPORT"
            )));
        };
        let given = match option {
            "LAYOUT" => &mut layout,
            "DECIMALS" => &mut decimals,
            _ => &mut unit,
        };
        if *given {
            return Err(refused(format!("{option} is given twice")));
        }
        *given = true;
        let Some(value) = words.get(i + 1) else {
            return Err(refused(format!("{option} needs a value")));
        };
        match option {
            "LAYOUT" => parsed.layout.clone_from(value),
            "DECIMALS" => {
                parsed.decimals = match parse_integer(value) {
                    Some(places) if (0..=i64::from(MAXIMUM_EXPORT_DECIMALS)).contains(&places) => {
                        places as i32
                    }
                    _ => {
                        return Err(refused(format!(
                            "DECIMALS is a whole number from 0 to {MAXIMUM_EXPORT_DECIMALS}, \
                             not '{value}'"
                        )));
                    }
                };
            }
            _ => {
                let Some(named) = unit_named(value) else {
                    return Err(refused(format!(
                        "UNIT is metres, feet, usfeet or links, not '{value}'"
                    )));
                };
                parsed.unit = named;
            }
        }
        i += 2;
    }
    Ok(parsed)
}

// All or nothing: a half-written point file is worse than none, because it
// reads without an error. Written beside the target and renamed over it, as
// the dialog wrote it (the C++'s QSaveFile).
fn write_whole(path: &Path, bytes: &[u8]) -> Status {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".katana-saving");
    let beside = PathBuf::from(beside);
    let written = std::fs::write(&beside, bytes).and_then(|()| std::fs::rename(&beside, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&beside);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            format!("{}: {error}", path_text(path)),
        ));
    }
    Ok(())
}

/// Runs a SURVEY EXPORT line on `document`, writing the file. Nothing in the
/// drawing changes. InvalidArgument for a line or a layout that does not
/// read; InvalidState for a drawing with no survey points, or a scope whose
/// entities hold none; FileExportFailure when the file cannot be written; the
/// writer's own refusal for a point the layout cannot carry.
pub fn run_survey_export(document: &Document, line: &str) -> Result<String> {
    let parsed = parse_survey_export(line)?;
    let layout = parse_layout_template(&parsed.layout)?;
    let options = DelimitedExportOptions {
        decimals: parsed.decimals,
        unit: parsed.unit,
    };
    let path = path_from_text(&parsed.file);
    let file = record_text(&path_text(&path));

    let mut scope_line = String::new();
    let mut not_points = 0;
    let points: Vec<DrawingSurveyPoint> = match &parsed.scope {
        None => drawing_survey_points(document, &SurveyImportOptions::default()),
        Some(words) => {
            // Headless there is no view: VIEW is refused, naming AREA.
            let found = match_scope(document, words, None)?;
            scope_line = format!("\nscope {}", scope_record(&found));
            if found.matched.is_empty() {
                return Ok(format!(
                    "export file={file} kind=survey-points ran=no reason={}{scope_line}",
                    record_text("the scope took nothing")
                ));
            }
            let pick =
                survey_points_among(document, &found.matched, &SurveyImportOptions::default());
            not_points = pick.not_survey_points;
            pick.points
        }
    };
    if points.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            if parsed.scope.is_some() {
                "none of what the scope took is a survey point (a point entity with a point \
                 number)"
            } else {
                "the drawing has no survey points (point entities with a point number)"
            },
        ));
    }
    let without_elevation = points
        .iter()
        .filter(|point| point.elevation.is_none())
        .count();
    let survey_points: Vec<_> = points.iter().map(to_survey_point).collect();
    let text = write_delimited_points(&survey_points, &layout, &options)?;
    write_whole(&path, text.as_bytes())?;
    Ok(format!(
        "exported file={file} kind=survey-points points={} layout={} decimals={} unit={} \
         without_elevation={without_elevation} not_points={not_points} bytes={}{scope_line}",
        points.len(),
        record_text(&layout_template(&layout)),
        parsed.decimals,
        unit_word(parsed.unit),
        text.len()
    ))
}
