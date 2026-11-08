//! SURVEY READ <file> [FORMAT <id>] (src/katana_app/survey_verbs.cpp): a
//! survey field file read as the reader makes it, and its reply, the records
//! katana_cli prints. The reading is surveyio's; the reply's words are
//! survey_verbs.cpp's, ported here so the gate can compare the two programs'
//! replies byte for byte. SURVEY IMPORT - the reduction, the coding and the
//! drawing of a job - is the cad layer's and waits for step 4.

use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_survey::data_model::source_file_name;
use katana_surveyio::{
    MAX_SURVEY_FILE_BYTES, PROBE_BYTES, ReadOptions, ReadResult, detect_format, format_registry,
    probe_of, read_survey, siblings_in_folder,
};

use crate::drawing::text_of;

/// Warnings listed in the reply; the rest are counted (survey_verbs.cpp
/// kListedWarnings).
const LISTED_WARNINGS: usize = 20;

/// A value as a reply record writes it (cad's recordValue, through
/// import_records.cpp recordText): empty as nothing, plain when it holds no
/// blank, quote, equals sign, backslash or line break, else in double quotes
/// with a quote and a backslash escaped and a line break made a blank.
fn record_text(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if !value.contains([' ', '\t', '"', '=', '\\', '\n', '\r']) {
        return value.to_owned();
    }
    let mut out = String::from("\"");
    for character in value.chars() {
        if character == '"' || character == '\\' {
            out.push('\\');
        }
        out.push(if character == '\n' || character == '\r' {
            ' '
        } else {
            character
        });
    }
    out.push('"');
    out
}

/// The C++'s `std::error_code::message()` for a failed file operation: the
/// system's own sentence, which Rust's io::Error shows followed by
/// " (os error N)".
fn system_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error()
        && let Some(sentence) = text.strip_suffix(&format!(" (os error {code})"))
    {
        return sentence.to_owned();
    }
    text
}

/// survey_verbs.cpp readBytes: the file whole, refused over a survey file's
/// limit. `what` is the file's part in the line, for the refusal.
fn read_bytes(path: &Path, what: &str) -> Result<Vec<u8>> {
    let size = match std::fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                format!("{what} cannot be read"),
                format!("{}: {}", text_of(path), system_message(&error)),
            ));
        }
    };
    if size > MAX_SURVEY_FILE_BYTES as u64 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "{} is larger than a survey file may be ({MAX_SURVEY_FILE_BYTES} bytes)",
                text_of(path)
            ),
        ));
    }
    std::fs::read(path).map_err(|_| {
        make_error(
            ErrorCode::FileImportFailure,
            format!("{what} cannot be opened"),
            text_of(path),
        )
    })
}

/// survey_verbs.cpp formatFor: FORMAT's, or the one detection is certain of
/// from the file's first PROBE_BYTES and its name.
fn format_for(format: Option<&str>, bytes: &[u8], name: &str) -> Result<String> {
    if let Some(id) = format
        && !id.is_empty()
    {
        return Ok(format_registry().find(id)?.id);
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
            let candidates: Vec<&str> = detection
                .candidates()
                .iter()
                .map(|candidate| candidate.format_id.as_str())
                .collect();
            Err(make_error(
                error.code,
                format!("{} - name the format with FORMAT <id>", detection.summary()),
                if candidates.is_empty() {
                    String::from("no format claims the file")
                } else {
                    format!("candidates: {}", candidates.join(", "))
                },
            ))
        }
    }
}

/// survey_verbs.cpp describeRead: what was read, what it holds, the
/// coordinate system it declares, the first warnings and what it did not
/// carry.
fn describe_read(read: &ReadResult, name: &str) -> String {
    let project = &read.project;
    let observations = project.observations.len()
        + project
            .stations
            .iter()
            .map(|station| station.observations.len())
            .sum::<usize>();
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
        "\ncontent setups={} observations={observations} points={} unpositioned={} features={} control={}",
        project.stations.len(),
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

/// SURVEY READ <file> [FORMAT <id>]: the job's other files - a DBX folder's,
/// a RINEX navigation file - read by name from the folder the file is in, as
/// the C++ reads them.
pub fn read(path: &Path, format: Option<&str>) -> Result<String> {
    let bytes = read_bytes(path, "the file")?;
    let name = source_file_name(&text_of(path));
    let format_id = format_for(format, &bytes, &name)?;
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let options = ReadOptions {
        siblings: Some(siblings_in_folder(folder, MAX_SURVEY_FILE_BYTES)),
        ..ReadOptions::default()
    };
    let read = read_survey(format_registry(), &format_id, &bytes, &name, &options)?;
    Ok(describe_read(&read, &name))
}
