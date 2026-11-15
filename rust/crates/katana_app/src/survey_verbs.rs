//! SURVEY READ and SURVEY IMPORT: a survey field file - any format surveyio
//! reads (docs/survey.md) - read, or imported as a survey job, from a line
//! (the port of src/katana_app/survey_verbs.hpp/.cpp). Here, in the app
//! layer, because cad may not see surveyio (tools/check_layering.cmake); the
//! window's command line runs the same function, so katana_cli, katana_mcp
//! and the window give one reply.
//!
//! ```text
//! SURVEY READ <file> [FORMAT <id>]
//! SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>] [SETTINGS <file>]
//!               [SET <key>=<value> ...] [CODES on|off] [LINEWORK on|off]
//! ```
//!
//! READ reads the file and says what the reader made of it, changing nothing.
//! IMPORT does what the import wizard's Import does: the reduction, the points
//! drawn on LAYER (survey/points), and the job kept on the drawing (Survey >
//! Survey Jobs) - ONE undo step (cad's ImportSurveyJobCommand). The format is
//! detected unless FORMAT names it; a detection that is not certain is
//! refused, naming the candidates, so that a file is never read with a reader
//! that only thinks it fits. Each option is given at most once.
//!
//! IMPORT ALSO CODES AND STRINGS WHAT IT DRAWS, in that same undo step (the
//! job's own finish, cad's survey_finish): the points go to the layers and
//! styles their survey codes give them, and are joined into lines - the
//! strings the file numbered itself, then the other points by their codes. It
//! is on unless something says otherwise, because a coded field file imported
//! as bare points on one layer is not what anyone imports it for. CODES on|off
//! and LINEWORK on|off say so for the one line; without the word, the
//! drawing's customisation does (its automation switches, both on until a
//! customisation or a setting turns one off); and with NO survey codes loaded
//! nothing is asked of the job at all: it is imported exactly as it was before
//! this existed, and only the two records below are added, saying
//! reason=no-survey-codes. cad's survey_import_finish decides all three, for
//! this verb and the wizard alike. Strings are ordered by point number. A step
//! with nothing to do is said and is no failure; a step that FAILS (a rule
//! that cannot become a layer) fails the whole import, points included.
//!
//! The reduction runs with the wizard's starting settings - ReductionSettings'
//! defaults, holding the control the file declares - or, with SETTINGS, the
//! settings a file holds in their stable text form (the one a job keeps,
//! survey's reduction_settings), whole: its control lines are all the
//! control. SET's items are lines of that same text, each changing the key it
//! names; a control item (control=<id>;<file|drawing>;then constraint;sigma
//! for northing, easting and elevation) holds its point as the wizard's Hold
//! does, in place of the point of that id or after the others. A point held
//! from the drawing is found among the drawing's survey points, as the
//! wizard's are (cad's reduction_context_for), and the import reduces as the
//! wizard's does (cad's reduce_for_drawing): an id the drawing has at two
//! places is refused. Both are read by survey's parse_reduction_settings and
//! refused with its words; a key SET names that this version does not know is
//! refused, where a SETTINGS file's is skipped and reported, as the text
//! form's versioning has it. A SETTINGS file is decoded as any text file is
//! (core's decode_text), so a byte order mark or UTF-16 reads the same.
//!
//! The reply is records, one per line:
//!
//! ```text
//! survey file=job.fld format=opcode-field-file parser=1.0 read=14 skipped=2 warnings=2
//! content setups=1 observations=12 points=2 unpositioned=2 features=1 control=0
//! declared crs="Australia/GDA2020, Zone 56" epsg=none
//! warning record=20 text="..."           (the first 20; then warnings_more=<n>)
//! not_carried text="no atmospheric settings"
//! settings file=held.txt set=1 differ=2   (IMPORT only; file= empty without SETTINGS)
//! setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0
//!                                        (each line that is not the defaults')
//! settings_warning text="line 3: ..."    (a SETTINGS key this version skipped)
//! imported job=job-1 entities=4 layer=survey/points reduction_warnings=2  (IMPORT only)
//!                                        (entities= is the POINTS drawn, never the
//!                                        lines; reduction_warnings= counts what the
//!                                        finish left over too, cad's finish_warnings)
//! held id=CP1 from=drawing entity=7 northing=5e+06 easting=5e+05 height=100
//!                                        (each point held; from=file has no entity)
//! reduction method=radiation adjustments=0 rejected=0
//! adjustment method="network least squares (horizontal)" observations=10 unknowns=4
//!   redundancy=6 variance_factor=0.1697573300133813 global_test=failed flagged=0 rejected=0
//!                                        (each adjustment run; none where absent)
//! reduction_warning text="..."           (the first 20; then reduction_warnings_more=<n>)
//! coded points=4 matched=3 unmatched_codes=1 layers=2 styles=2
//!   or  coded none reason=off|no-survey-codes|no-codes-in-file|no-rule-matches|no-points
//! unmatched_code text=ZZ                 (each such code, the first 10; then
//!                                        unmatched_codes_more=<n>)
//! linework lines=2 unplaced=3 layers=0 styles=0
//!   or  linework none reason=<the same words>
//! resection setup=S1 ...                 (each resected setup, then its flagged
//! resection_residual setup=S1 ...        residuals: docs/survey.md)
//! ```
//!
//! The coded and linework records follow the reduction's warnings and come
//! before the resections', not straight after `imported`: the lines from
//! `imported` to the first warning, and the resection records at the end, are
//! held where they stand by the tests written before these two existed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use katana_cad::linework_verbs::{SurveyImportFinish, survey_import_finish};
use katana_cad::survey_finish::{SurveyFinishReport, SurveyFinishSkip};
use katana_cad::survey_job::{default_reduction, reduction_context_for};
use katana_cad::survey_points::{DrawingSurveyPoint, drawing_survey_points};
use katana_cad::{
    CommandInterpreter, Document, ImportSurveyJobCommand, SurveyImportOptions, SurveyJob,
    SurveyJobFile, SurveyJobImport,
};
use katana_core::civil_date::now_utc;
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, is_ascii_space, split_lines, trimmed};
use katana_core::text_encoding::{decode_text, to_string as encoding_name};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EnumName;
use katana_survey::data_model::source_file_name;
use katana_survey::reduction_settings::{
    control_from_file, parse_reduction_settings, serialise_reduction_settings,
};
use katana_survey::report_summary::{method_text, rejected_observations};
use katana_survey::{
    AdjustmentReport, ControlOrigin, ControlPoint, ControlSelection, ReductionReport,
    ReductionSettings, SurveyProject,
};
use katana_surveyio::{
    MAX_SURVEY_FILE_BYTES, PROBE_BYTES, ReadOptions, ReadResult, detect_format, format_registry,
    probe_of, read_survey, report_input_for, siblings_in_folder,
};

use crate::import_records::{path_from_text, path_text, record_number, record_text};

const USAGE: &str = "SURVEY READ <file> [FORMAT <id>] | SURVEY IMPORT <file> [FORMAT <id>] \
                     [LAYER <path>] [SETTINGS <file>] [SET <key>=<value> ...] [CODES on|off] \
                     [LINEWORK on|off]";

// The words that begin an option, and so end SET's items: a key of the
// settings text is never one of them.
const OPTIONS: [&str; 6] = ["FORMAT", "LAYER", "SETTINGS", "SET", "CODES", "LINEWORK"];

// Warnings listed in the reply; the rest are counted. The job's report keeps
// every one of them (Survey > Survey Jobs).
const LISTED_WARNINGS: usize = 20;

// Codes no rule answers that the reply names, each a record; the rest are
// counted. Ten is enough to see whether the wrong customisation is loaded or
// one code was mistyped, and CODE CENSUS lists them all.
const LISTED_UNMATCHED_CODES: usize = 10;

// What libstdc++'s std::filesystem::file_size sets for a path that is there
// but is no regular file: is_a_directory for a folder, not_supported for
// anything else, whose messages are the C library's strerror sentences.
const IS_A_DIRECTORY: &str = "Is a directory";
const NOT_SUPPORTED: &str = "Operation not supported";

fn same_word(a: &str, b: &str) -> bool {
    equals_ignoring_case(a.as_bytes(), b.as_bytes())
}

// The option `word` names, as OPTIONS spells it; empty for any other word.
fn option_named(word: &str) -> &'static str {
    OPTIONS
        .iter()
        .copied()
        .find(|option| same_word(word, option))
        .unwrap_or_default()
}

#[derive(Debug, Default)]
struct SurveyLine {
    import: bool,
    path: String,
    format: String,
    layer: Option<String>,
    settings_file: Option<String>,
    /// SET's items, as given.
    set: Vec<String>,
    /// CODES and LINEWORK, when the line says either: without the word the
    /// drawing's customisation decides (cad's survey_import_finish).
    codes: Option<bool>,
    linework: Option<bool>,
}

fn refused(why: String) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("{why}; usage: {USAGE}"))
}

fn parse(line: &str) -> Result<SurveyLine> {
    let words = CommandInterpreter::tokenize(line)?;
    if words.len() < 3 || !(same_word(&words[1], "READ") || same_word(&words[1], "IMPORT")) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("usage: {USAGE}"),
        ));
    }
    let mut parsed = SurveyLine {
        import: same_word(&words[1], "IMPORT"),
        path: words[2].clone(),
        ..SurveyLine::default()
    };
    // Each option once: a second FORMAT or SETTINGS has no meaning the first
    // does not contradict, and taking the last one said nothing of the other.
    let mut given: BTreeSet<&'static str> = BTreeSet::new();
    let mut i = 3;
    while i < words.len() {
        let option = option_named(&words[i]);
        if option.is_empty() || (!parsed.import && option != "FORMAT") {
            return Err(refused(format!(
                "'{}' is not an option of SURVEY {}",
                words[i],
                if parsed.import { "IMPORT" } else { "READ" }
            )));
        }
        if !given.insert(option) {
            return Err(refused(format!("{option} is given twice")));
        }
        if option == "SET" {
            // Its items run to the next option word or the end of the line.
            i += 1;
            while i < words.len() && option_named(&words[i]).is_empty() {
                parsed.set.push(words[i].clone());
                i += 1;
            }
            if parsed.set.is_empty() {
                return Err(refused(String::from(
                    "SET needs at least one <key>=<value>",
                )));
            }
            continue;
        }
        if i + 1 >= words.len() {
            return Err(refused(format!("{} needs a value", words[i])));
        }
        let value = &words[i + 1];
        if option == "FORMAT" {
            parsed.format = value.clone();
        } else if option == "LAYER" {
            parsed.layer = Some(value.clone());
        } else if option == "CODES" || option == "LINEWORK" {
            // Two words and no others: "yes", "1" or a mistyped "of" taken
            // for one of them would code, or not code, a whole job unasked.
            let on = same_word(value, "on");
            if !on && !same_word(value, "off") {
                return Err(refused(format!(
                    "{option} takes on or off, and '{value}' is neither"
                )));
            }
            if option == "CODES" {
                parsed.codes = Some(on);
            } else {
                parsed.linework = Some(on);
            }
        } else {
            parsed.settings_file = Some(value.clone());
        }
        i += 2;
    }
    Ok(parsed)
}

// The C++'s std::error_code::message() for a failed file operation: the
// system's own sentence, which Rust's io::Error shows followed by
// " (os error N)".
fn system_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error()
        && let Some(sentence) = text.strip_suffix(&format!(" (os error {code})"))
    {
        return sentence.to_owned();
    }
    text
}

// `what` is the file's part in the line, for the refusal: "the file", "the
// settings file".
fn read_bytes(path: &Path, what: &str) -> Result<Vec<u8>> {
    let cannot_be_read = |message: String| {
        make_error(
            ErrorCode::FileImportFailure,
            format!("{what} cannot be read"),
            format!("{}: {message}", path_text(path)),
        )
    };
    // std::filesystem::file_size: the size of a regular file, following a
    // link; a folder or another kind of file is an error.
    let size = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(metadata) if metadata.is_dir() => {
            return Err(cannot_be_read(String::from(IS_A_DIRECTORY)));
        }
        Ok(_) => return Err(cannot_be_read(String::from(NOT_SUPPORTED))),
        Err(error) => return Err(cannot_be_read(system_message(&error))),
    };
    // A settings file is held to a survey file's limit too: it names at most
    // every point of one, and nothing larger is text a person meant.
    if size > MAX_SURVEY_FILE_BYTES as u64 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "{} is larger than a survey file may be ({MAX_SURVEY_FILE_BYTES} bytes)",
                path_text(path)
            ),
        ));
    }
    std::fs::read(path).map_err(|_| {
        make_error(
            ErrorCode::FileImportFailure,
            format!("{what} cannot be opened"),
            path_text(path),
        )
    })
}

// The format to read with: FORMAT's, or the one detection is certain of.
fn format_for(line: &SurveyLine, bytes: &[u8], name: &str) -> Result<String> {
    if !line.format.is_empty() {
        return Ok(format_registry().find(&line.format)?.id);
    }
    let truncated = bytes.len() > PROBE_BYTES;
    let probe = if truncated {
        &bytes[..PROBE_BYTES]
    } else {
        bytes
    };
    let detection = detect_format(&probe_of(probe, name, truncated));
    match detection.format() {
        Ok(descriptor) => Ok(descriptor.id),
        Err(error) => {
            let mut candidates = String::new();
            for candidate in detection.candidates() {
                if !candidates.is_empty() {
                    candidates.push_str(", ");
                }
                candidates.push_str(&candidate.format_id);
            }
            Err(make_error(
                error.code,
                format!("{} - name the format with FORMAT <id>", detection.summary()),
                if candidates.is_empty() {
                    String::from("no format claims the file")
                } else {
                    format!("candidates: {candidates}")
                },
            ))
        }
    }
}

fn observation_count(project: &SurveyProject) -> usize {
    let mut count = project.observations.len();
    for station in &project.stations {
        count += station.observations.len();
    }
    count
}

fn describe_read(read: &ReadResult, name: &str) -> String {
    let project = &read.project;
    let mut reply = format!(
        "survey file={} format={} parser={} read={} skipped={} warnings={}",
        record_text(name),
        read.format_id,
        record_text(&read.parser_version),
        read.records_read,
        read.records_skipped,
        read.warnings.len()
    );
    reply.push_str(&format!(
        "\ncontent setups={} observations={} points={} unpositioned={} features={} control={}",
        project.stations.len(),
        observation_count(project),
        project.points.len(),
        project.unpositioned_points.len(),
        project.features.len(),
        project.control_points.len()
    ));
    let crs = &project.coordinate_system;
    reply.push_str(&format!(
        "\ndeclared crs={} epsg={}",
        if crs.name.is_empty() {
            String::from("none")
        } else {
            record_text(&crs.name)
        },
        if crs.epsg_code > 0 {
            format!("{}", crs.epsg_code)
        } else {
            String::from("none")
        }
    ));
    for warning in read.warnings.iter().take(LISTED_WARNINGS) {
        reply.push_str(&format!(
            "\nwarning record={} text={}",
            warning.record,
            record_text(&warning.message)
        ));
    }
    if read.warnings.len() > LISTED_WARNINGS {
        reply.push_str(&format!(
            "\nwarnings_more={}",
            read.warnings.len() - LISTED_WARNINGS
        ));
    }
    for missing in &read.not_carried {
        reply.push_str(&format!("\nnot_carried text={}", record_text(missing)));
    }
    reply
}

// ---- the reduction settings: SETTINGS and SET --------------------------------------
//
// One grammar, the settings' stable text form (survey's reduction_settings):
// a SETTINGS file is that text, and each SET item is one line of it, read by
// survey's parse_reduction_settings with its own messages. Nothing of the form
// is spelt here - its first line and its keys are taken from what
// serialise_reduction_settings writes, so they cannot fall behind it.

// core's split_lines, on text: every cut is at an ASCII line end, so each
// piece is UTF-8 still.
fn lines_of(text: &str) -> Vec<&str> {
    split_lines(text.as_bytes())
        .into_iter()
        .map(|line| std::str::from_utf8(line).unwrap_or_default())
        .collect()
}

// The C++'s `line.substr(0, line.find('='))`: the key of a settings line, or
// the whole line when it has no '='.
fn key_of(line: &str) -> &str {
    line.split_once('=').map_or(line, |(key, _)| key)
}

// The text form's first line, its name and version, with the line break.
fn settings_header() -> &'static str {
    static HEADER: OnceLock<String> = OnceLock::new();
    HEADER.get_or_init(|| {
        let text = serialise_reduction_settings(&ReductionSettings::default());
        // The C++'s substr(0, find('\n') + 1): nothing when there is no line
        // break, npos + 1 being 0.
        match text.find('\n') {
            Some(end) => text[..end + 1].to_owned(),
            None => String::new(),
        }
    })
}

// Every key of the text form, in its order: the defaults' lines, and one
// control point's for the key a control line has.
fn setting_keys() -> &'static str {
    static KEYS: OnceLock<String> = OnceLock::new();
    KEYS.get_or_init(|| {
        let sample = ReductionSettings {
            control: vec![ControlSelection {
                point: ControlPoint {
                    point_id: String::from("id"),
                    ..ControlPoint::default()
                },
                ..ControlSelection::default()
            }],
            ..ReductionSettings::default()
        };
        let text = serialise_reduction_settings(&sample);
        let mut list = String::new();
        for line in lines_of(&text).into_iter().skip(1) {
            if !list.is_empty() {
                list.push_str(", ");
            }
            list.push_str(key_of(line));
        }
        list
    })
}

// What the line asks of the settings, gathered before the survey file is
// read, so a mistyped key is refused without reading a large file first.
#[derive(Debug, Default)]
struct SettingsRequest {
    /// SETTINGS', when given.
    from_file: Option<ReductionSettings>,
    /// Its name, for the reply.
    file_name: String,
    /// Keys it holds that this version does not.
    file_warnings: Vec<String>,
    /// SET's, as given.
    items: Vec<String>,
    /// The keys SET's items change, control apart.
    keys: BTreeSet<String>,
    /// SET's control items, read.
    control: Vec<ControlSelection>,
}

// One SET item, read as the one line of a settings text after its version
// line, so its value is checked by the same parser and validation a file's
// line is. `request` gathers what it changes.
fn read_set_item(item: &str, request: &mut SettingsRequest) -> Status {
    let place = format!("SET {item}");
    if item.contains(['\r', '\n']) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "SET item {} holds a line break; each item is one line of the settings",
                request.items.len() + 1
            ),
        ));
    }
    // The parser passes over a note and a blank line without a word, which
    // for an item typed on purpose would be a setting silently not made.
    let equals = match item.find('=') {
        Some(equals) if !item.starts_with('#') => equals,
        _ => {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("SET takes <key>=<value> items, and '{item}' is not one"),
                place,
            ));
        }
    };
    let key = &item[..equals];
    let mut unknown: Vec<String> = Vec::new();
    let one = match parse_reduction_settings(
        &format!("{}{item}\n", settings_header()),
        Some(&mut unknown),
    ) {
        Ok(one) => one,
        Err(error) => return Err(make_error(error.code, error.message, place)),
    };
    // In a file, a key this version does not know is skipped, since a later
    // Katana may have written it. Typed here and now, it is a mistake.
    if !unknown.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "'{key}' is not a reduction setting; the settings are {}",
                setting_keys()
            ),
            place,
        ));
    }
    if let Some(held) = one.control.into_iter().next() {
        let id = &held.point.point_id;
        if request
            .control
            .iter()
            .any(|listed| listed.point.point_id == *id)
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("control point '{id}' is listed twice"),
                place,
            ));
        }
        request.control.push(held);
    } else if !request.keys.insert(key.to_owned()) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("'{key}' is given twice"),
            place,
        ));
    }
    request.items.push(item.to_owned());
    Ok(())
}

fn settings_request(line: &SurveyLine) -> Result<SettingsRequest> {
    let mut request = SettingsRequest::default();
    if let Some(file) = &line.settings_file {
        let path = path_from_text(file);
        let bytes = read_bytes(&path, "the settings file")?;
        // Decoded as every text file Katana reads is (core's text_encoding):
        // the byte order mark an editor saves, or its UTF-16, is the same
        // settings, where the parser alone would call the text not settings.
        let text = match decode_text(&bytes) {
            Ok(text) => text,
            Err(error) => {
                return Err(make_error(
                    error.code,
                    error.message,
                    format!("SETTINGS {}", path_text(&path)),
                ));
            }
        };
        if text.guessed {
            request.file_warnings.push(format!(
                "the settings file is not UTF-8; it was read as {}, so an accented point id may \
                 be wrong",
                encoding_name(text.encoding)
            ));
        }
        let settings = match parse_reduction_settings(&text.text, Some(&mut request.file_warnings))
        {
            Ok(settings) => settings,
            Err(error) => {
                return Err(make_error(
                    error.code,
                    error.message,
                    format!("SETTINGS {}", path_text(&path)),
                ));
            }
        };
        request.from_file = Some(settings);
        request.file_name = source_file_name(&path_text(&path));
    }
    for item in &line.set {
        read_set_item(item, &mut request)?;
    }
    Ok(request)
}

// The settings the reduction runs with. The start is the wizard's - the
// defaults, holding the control the file declares - or the SETTINGS file's,
// whole: its control lines are all the control there is. SET's items then
// change what they name, a control item holding its point as the wizard's
// Hold does: in place of a point of that id, or after the others. The result
// is read back through the parser, so it is validated as a whole.
fn settings_for(request: &SettingsRequest, project: &SurveyProject) -> Result<ReductionSettings> {
    let mut settings = match &request.from_file {
        Some(from_file) => from_file.clone(),
        None => ReductionSettings {
            control: control_from_file(project),
            ..ReductionSettings::default()
        },
    };
    if request.items.is_empty() {
        return Ok(settings);
    }
    for held in &request.control {
        match settings
            .control
            .iter_mut()
            .find(|selection| selection.point.point_id == held.point.point_id)
        {
            Some(same) => *same = held.clone(),
            None => settings.control.push(held.clone()),
        }
    }
    let mut text = String::new();
    let start = serialise_reduction_settings(&settings);
    for line in lines_of(&start) {
        if !request.keys.contains(key_of(line)) {
            text.push_str(line);
            text.push('\n');
        }
    }
    for item in &request.items {
        if request.keys.contains(key_of(item)) {
            text.push_str(item);
            text.push('\n');
        }
    }
    parse_reduction_settings(&text, None)
}

// Where the settings came from and every line of their text form that is not
// the defaults' - in its order and its words, so a line reads back as a SET
// item - then what the SETTINGS file held that this version skipped.
fn describe_settings(used: &ReductionSettings, request: &SettingsRequest) -> String {
    let defaults = serialise_reduction_settings(&ReductionSettings::default());
    let unchanged: BTreeSet<&str> = lines_of(&defaults).into_iter().collect();
    let text = serialise_reduction_settings(used);
    let mut lines = String::new();
    let mut differ = 0_usize;
    for line in lines_of(&text) {
        if unchanged.contains(line) {
            continue;
        }
        // The C++'s substr(0, find('=')) and substr(find('=') + 1): the whole
        // line both times when there is no '='.
        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key, value),
            None => (line, line),
        };
        lines.push_str(&format!(
            "\nsetting key={} value={}",
            record_text(key),
            record_text(value)
        ));
        differ += 1;
    }
    let mut reply = format!(
        "settings file={} set={} differ={differ}{lines}",
        record_text(&request.file_name),
        request.items.len()
    );
    for warning in &request.file_warnings {
        reply.push_str(&format!("\nsettings_warning text={}", record_text(warning)));
    }
    reply
}

// " northing=5e+06 easting=5e+05 height=100", each number the shortest text
// that reads back exactly, as every record writes one; height=none for a
// point with no height, which is not a height of zero.
fn place_fields(northing: f64, easting: f64, height: Option<f64>) -> String {
    format!(
        " northing={} easting={} height={}",
        record_number(northing),
        record_number(easting),
        optional_number(height)
    )
}

// "none" for a number that is absent, which is not zero.
fn optional_number(value: Option<f64>) -> String {
    match value {
        Some(value) => record_number(value),
        None => String::from("none"),
    }
}

// Where each point the settings hold is held: at the drawing's survey point
// of its id - the entity too, the first of the id as the reduction takes it
// (cad's reduce_for_drawing refuses an id whose points are not one mark) - or
// at the file's. Taken from what the import is handed, before it runs; an id
// neither has is the reduction's to refuse.
fn held_records(
    settings: &ReductionSettings,
    raw: &SurveyProject,
    drawing: &[DrawingSurveyPoint],
) -> String {
    let mut records = String::new();
    for selection in &settings.control {
        let id = &selection.point.point_id;
        if selection.origin == ControlOrigin::Drawing {
            if let Some(point) = drawing.iter().find(|point| point.id == *id) {
                records.push_str(&format!(
                    "\nheld id={} from=drawing entity={}{}",
                    record_text(id),
                    point.entity,
                    place_fields(point.northing, point.easting, point.elevation)
                ));
            }
        } else if let Some(point) = raw.points.iter().find(|point| point.id == *id) {
            records.push_str(&format!(
                "\nheld id={} from=file{}",
                record_text(id),
                place_fields(point.northing, point.easting, point.elevation)
            ));
        }
    }
    records
}

// The layers or the styles of `all` that `coding` did not make.
fn beyond(all: &[String], coding: &[String]) -> usize {
    all.iter().filter(|name| !coding.contains(name)).count()
}

// What the import did with the survey codes, each after a line break: the
// coding and then the linework, each as what it did or as "none" with the one
// word of why not. `asked` is what the line and the drawing's customisation
// asked for; `report` what the job's finish then did - None when nothing was
// handed on, which is the plain import of a line that said off and of a
// drawing with no survey codes.
fn finish_records(asked: &SurveyImportFinish, report: Option<&SurveyFinishReport>) -> String {
    // Off is said before anything else: it is the line's choice or the
    // customisation's, whatever the file and the codes would have made of it.
    let why_not = |wanted: bool, skipped: SurveyFinishSkip| -> &'static str {
        if !wanted {
            "off"
        } else if !asked.survey_codes_loaded {
            SurveyFinishSkip::NoSurveyCodes.name()
        } else {
            skipped.name()
        }
    };

    let mut records = String::new();
    match report {
        Some(report) if report.codes_ran() => records.push_str(&format!(
            "\ncoded points={} matched={} unmatched_codes={} layers={} styles={}",
            report.coding.coded,
            report.coding.matched,
            report.codes_with_no_rule().len(),
            report.coding.layers_created.len(),
            report.coding.styles_created.len()
        )),
        _ => records.push_str(&format!(
            "\ncoded none reason={}",
            why_not(
                asked.codes_asked,
                report.map_or(SurveyFinishSkip::NotAsked, |report| report.why_not_coded)
            )
        )),
    }
    // The codes themselves, whether the step ran or stopped at them: "no rule
    // matches" is no use to anyone without the codes it is about.
    if let Some(report) = report {
        let unknown = report.codes_with_no_rule();
        for code in unknown.iter().take(LISTED_UNMATCHED_CODES) {
            records.push_str(&format!("\nunmatched_code text={}", record_text(code)));
        }
        if unknown.len() > LISTED_UNMATCHED_CODES {
            records.push_str(&format!(
                "\nunmatched_codes_more={}",
                unknown.len() - LISTED_UNMATCHED_CODES
            ));
        }
    }
    match report {
        // The layers and the styles the LINES made: those the whole finish
        // created less the coding's, which its own record has counted. With
        // CODES off that is every one of them - a line is put on its rule's
        // layer and styled by it whether or not its points were - and unsaid
        // they appeared in the drawing with no record to account for them.
        Some(report) if report.linework_ran() => records.push_str(&format!(
            "\nlinework lines={} unplaced={} layers={} styles={}",
            report.lines.len(),
            report.unplaced(),
            beyond(&report.layers_created(), &report.coding.layers_created),
            beyond(&report.styles_created(), &report.coding.styles_created)
        )),
        _ => records.push_str(&format!(
            "\nlinework none reason={}",
            why_not(
                asked.linework_asked,
                report.map_or(SurveyFinishSkip::NotAsked, |report| report.why_not_strung)
            )
        )),
    }
    records
}

// One adjustment's statistics as its report holds them.
fn adjustment_record(adjustment: &AdjustmentReport) -> String {
    format!(
        "\nadjustment method={} observations={} unknowns={} redundancy={} variance_factor={} \
         global_test={} flagged={} rejected={}",
        record_text(&adjustment.method),
        adjustment.observations,
        adjustment.unknowns,
        adjustment.redundancy,
        optional_number(adjustment.variance_factor),
        match &adjustment.global_test {
            Some(test) if test.passed => "passed",
            Some(_) => "failed",
            None => "none",
        },
        adjustment.flagged_outliers.len(),
        adjustment.rejected_outliers.len()
    )
}

/// The reply's records of a reduction's outcome, each after a line break: the
/// reduction (the settings' method in words, how many adjustments ran, the
/// observations rejected - survey's rejected_observations, the count the
/// wizard shows), then each adjustment's statistics as its report holds them.
pub fn reduction_records(report: &ReductionReport) -> String {
    let mut records = format!(
        "\nreduction method={} adjustments={} rejected={}",
        record_text(&method_text(&report.settings)),
        report.adjustments.len(),
        rejected_observations(report)
    );
    for adjustment in &report.adjustments {
        records.push_str(&adjustment_record(adjustment));
    }
    records
}

// Each setup the reduction positioned by resection, as the report's
// Resections table has it: where, from which points, how well (one sigma,
// metres), its redundancy (plan + heights), how many of its residuals the
// outlier test flagged, the geometry's dilution and whether that is weak, its
// checks, how far the file's own coordinates for the station were, which setup
// had radiated the station and how far from the resection, and a scale factor
// that was not applied. Then each residual flagged or rejected, one record
// each, so an agent sees which without the report and past the warnings' cap.
fn resection_records(report: &ReductionReport) -> String {
    let mut reply = String::new();
    for resection in &report.resections {
        let from = resection.targets.join(",");
        let flagged = resection.horizontal.flagged_outliers.len()
            + resection
                .height
                .as_ref()
                .map_or(0, |height| height.flagged_outliers.len());
        let file_offset = match (
            resection.file_northing_difference,
            resection.file_easting_difference,
        ) {
            (Some(northing), Some(easting)) => Some(northing.hypot(easting)),
            _ => None,
        };
        let radiation_offset = match (
            resection.radiated_northing_difference,
            resection.radiated_easting_difference,
        ) {
            (Some(northing), Some(easting)) => Some(northing.hypot(easting)),
            _ => None,
        };
        reply.push_str(&format!(
            "\nresection setup={} point={} from={} northing={} easting={} height={} sigma_n={} \
             sigma_e={} sigma_h={} redundancy={}+{} flagged={flagged} dilution={} weak={} \
             checks={} file_offset={} radiated_from={} radiation_offset={} \
             unapplied_scale_factor={}",
            record_text(&resection.station_id),
            record_text(&resection.point_id),
            record_text(&from),
            record_number(resection.northing),
            record_number(resection.easting),
            optional_number(resection.elevation),
            record_number(resection.sigma_northing),
            record_number(resection.sigma_easting),
            optional_number(resection.sigma_elevation),
            resection.horizontal.redundancy,
            resection
                .height
                .as_ref()
                .map_or(0, |height| height.redundancy),
            record_number(resection.dilution),
            if resection.weak_geometry { "1" } else { "0" },
            resection.checks,
            optional_number(file_offset),
            if resection.radiated_from.is_empty() {
                String::from("none")
            } else {
                record_text(&resection.radiated_from)
            },
            optional_number(radiation_offset),
            optional_number(resection.unapplied_scale_factor)
        ));
        let adjustments = std::iter::once(&resection.horizontal).chain(resection.height.as_ref());
        for adjustment in adjustments {
            for residual in &adjustment.residuals {
                if !residual.flagged && !residual.rejected {
                    continue;
                }
                reply.push_str(&format!(
                    "\nresection_residual setup={} observation={} residual={} unit={} sigma={} \
                     standardised={} state={}",
                    record_text(&resection.station_id),
                    record_text(&residual.observation),
                    record_number(residual.residual),
                    if residual.angular { "rad" } else { "m" },
                    record_number(residual.sigma),
                    optional_number(residual.standardised),
                    if residual.rejected {
                        "rejected"
                    } else {
                        "flagged"
                    }
                ));
            }
        }
    }
    reply
}

/// True for a line whose verb is SURVEY.
pub fn is_survey_line(line: &str) -> bool {
    let text = trimmed(line.as_bytes());
    let end = text
        .iter()
        .position(|&byte| is_ascii_space(byte))
        .unwrap_or(text.len());
    equals_ignoring_case(&text[..end], b"SURVEY")
}

/// Runs a SURVEY line on `document`. InvalidArgument for a line the grammar
/// above does not take; the settings parser's, the detection's, the reader's
/// or the import's own error otherwise, with the drawing unchanged.
pub fn run_survey_line(document: &mut Document, line: &str) -> Result<String> {
    let parsed = parse(line)?;
    let wanted = if parsed.import {
        Some(settings_request(&parsed)?)
    } else {
        None
    };
    let path = path_from_text(&parsed.path);
    let bytes = read_bytes(&path, "the file")?;
    let name = source_file_name(&path_text(&path));
    let format = format_for(&parsed, &bytes, &name)?;
    // The job's other files - a DBX folder, a RINEX navigation file - by name,
    // from the folder the file is in, as the wizard reads them.
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let options = ReadOptions {
        siblings: Some(siblings_in_folder(folder, MAX_SURVEY_FILE_BYTES)),
        ..ReadOptions::default()
    };
    let read = read_survey(format_registry(), &format, &bytes, &name, &options)?;
    let mut reply = describe_read(&read, &name);
    let Some(wanted) = wanted else {
        return Ok(reply);
    };

    let settings = settings_for(&wanted, &read.project)?;
    // The drawing's points, for the control held from the drawing: the
    // wizard's context, from the one function both take it from.
    let mut context = reduction_context_for(document)?;
    context.input = report_input_for(&read, &name);
    context.created_utc = now_utc();
    let mut import_options = SurveyImportOptions::default();
    if let Some(layer) = &parsed.layer {
        import_options.layer = layer.clone();
    }
    let job = SurveyJob {
        name: name.clone(),
        format_id: read.format_id.clone(),
        parser_version: read.parser_version.clone(),
        source_file_name: name.clone(),
        source_bytes: bytes,
        sibling_files: read
            .siblings_read
            .iter()
            .map(|sibling| SurveyJobFile {
                name: sibling.name.clone(),
                bytes: sibling.bytes.clone(),
            })
            .collect(),
        settings: settings.clone(),
        layer: import_options.layer.clone(),
        imported_utc: context.created_utc.clone(),
        ..SurveyJob::default()
    };
    // Before the import draws anything, and before the project is handed on.
    let held = held_records(
        &settings,
        &read.project,
        &drawing_survey_points(document, &SurveyImportOptions::default()),
    );
    // Coded and strung inside the job's own command, so one UNDO takes back
    // the points, their codes and their lines: as CODES and LINEWORK say,
    // else as the drawing's customisation does, and not at all where no
    // survey codes are loaded (cad's survey_import_finish, which the wizard
    // asks too).
    let finish = survey_import_finish(document, parsed.codes, parsed.linework);
    let request = SurveyJobImport {
        job,
        raw: read.project,
        context,
        import_options: import_options.clone(),
        finish: finish.options.clone(),
        ..SurveyJobImport::default()
    };
    // The command's own reduction, cad's reduce_for_drawing, as the wizard's.
    // It moves into the drawing's history when it runs, so what the C++ reads
    // back through its pointer is read through the results it fills.
    let command = ImportSurveyJobCommand::new(document, request, Some(default_reduction()));
    let results = command.results();
    document.execute(Box::new(command))?;
    reply.push('\n');
    reply.push_str(&describe_settings(&settings, &wanted));
    let report = results.report();
    let finished = results.finish_report();
    // The POINTS drawn, as it always was: a finished import's created
    // entities are its points and then its lines, and the lines have a record
    // of their own below.
    let points = match &finished {
        Some(finished) => finished.points,
        None => document.last_created_entities().len(),
    };
    reply.push_str(&format!(
        "\nimported job={} entities={points} layer={} reduction_warnings={}",
        record_text(&results.job_id()),
        record_text(&import_options.layer),
        report.as_ref().map_or(0, |report| report.warnings.len())
    ));
    reply.push_str(&held);
    // What the adjustment made of the data, and then the reduction's own
    // warnings, as the read's are listed: an agent has no Survey Jobs dialog
    // to open the report in.
    if let Some(report) = &report {
        reply.push_str(&reduction_records(report));
        for warning in report.warnings.iter().take(LISTED_WARNINGS) {
            reply.push_str(&format!(
                "\nreduction_warning text={}",
                record_text(&warning.text)
            ));
        }
        if report.warnings.len() > LISTED_WARNINGS {
            reply.push_str(&format!(
                "\nreduction_warnings_more={}",
                report.warnings.len() - LISTED_WARNINGS
            ));
        }
    }
    // What the import made of the survey codes: after the reduction's
    // warnings, the last of which are the finish's own, and before the
    // resections, whose records end the reply as they always have.
    reply.push_str(&finish_records(&finish, finished.as_ref()));
    if let Some(report) = &report {
        reply.push_str(&resection_records(report));
    }
    Ok(reply)
}

/// The lines --help prints for it.
pub fn survey_help_text() -> &'static str {
    concat!(
        "Survey    SURVEY READ <file> [FORMAT <id>]  what a survey field file holds, as\n",
        "          the reader made it; nothing is changed\n",
        "          SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>] [SETTINGS <file>]\n",
        "          [SET <key>=<value> ...] [CODES on|off] [LINEWORK on|off]  reduced and\n",
        "          drawn as a survey job, one undo step: with the defaults and the\n",
        "          control the file declares, or a SETTINGS file's (the settings text a\n",
        "          job keeps), each SET item one line of that text; SET\n",
        "          control=CP1;drawing;fixed;0;fixed;0;fixed;0 holds the drawing's point\n",
        "          CP1 (refused where the drawing has two CP1s apart); the reply says\n",
        "          where each point was held and what the adjustment made of the data\n",
        "          (key=value records, docs/survey.md).\n",
        "          The points are coded by the loaded survey codes and strung into\n",
        "          lines in that same step - the file's numbered strings, then the\n",
        "          rest by code - unless CODES off or LINEWORK off says not to, or the\n",
        "          customisation does; the reply's coded and linework records say what\n",
        "          was done, or \"none\" and why (reason=off, no-survey-codes,\n",
        "          no-codes-in-file, no-rule-matches, no-points)\n",
    )
}
