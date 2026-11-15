//! Reading a survey file with the reader its format registered (format.rs).
//!
//! ```text
//! let detection = detect_format(...);                               // which format?
//! let read = read_survey(format_registry(), format_id, bytes,
//!                        file_name, &options);                      // read it
//! ```
//!
//! read_survey() is the one door to every reader, so that the rules below hold
//! for all of them however each is written.
//!
//! THE DELIMITED-POINTS FORMAT HAS NO READER, deliberately. A coordinate CSV is
//! read through a column layout (delimited_points.rs) that the import wizard's
//! layout page shows the person and lets them correct; a reader that guessed
//! the layout on its own would import easting as northing without a word the
//! first time a file's header was ambiguous. read_survey() on it is
//! Unsupported, saying so; the wizard keeps driving parse_delimited_points().

use std::any::Any;
use std::fs::File;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_survey::data_model::{source_file_name, validate_project};
use katana_survey::{ReportInput, ReportMessage, SourceRecord};

use crate::delimited_points::DELIMITED_POINTS_FORMAT_ID;
use crate::format::{
    FormatRegistry, ImportResult, ReadOptions, ReadResult, SiblingFile, SiblingLookup, describe,
    format_registry,
};

/// Bytes. The largest file read_survey() hands a reader, and the default cap on
/// one sibling. Policy, stated once: a day's RINEX at 1 Hz from a multi-system
/// receiver is a few hundred megabytes, and anything past this is not a survey
/// file a person meant to import - refusing it is kinder than a program that
/// stops answering while it allocates.
pub const MAX_SURVEY_FILE_BYTES: usize = 1 << 30; // 1 GiB

/// How many sibling files one read may fetch. A Leica DBX job is a handful of
/// files; a reader asking for more is looping on something the file said.
pub const MAX_SIBLING_FILES: usize = 64;

fn equals_ignoring_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

// The sibling rule, in one place: a reader may ask for a NAME and nothing else.
// Anything source_file_name() would change - a directory part, "..", a drive
// letter, an alternate data stream - came from the file, and a file must not
// choose what this program reads (data_model.rs explains).
fn check_sibling_name(name: &str) -> Status {
    if name.is_empty() || source_file_name(name) != name {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "refused to read '{name}': a file named inside a survey file is looked for by its name \
                 alone, beside the file, and this is not a plain file name"
            ),
        ));
    }
    Ok(())
}

/// What a reduction's report says of the file it reduced (ReportInput): its
/// name and siblings, the format by its human name, the parser, the counts,
/// every warning with its record, and what the file did not carry. One place,
/// for the import wizard, the Survey Jobs dialog and SURVEY IMPORT alike.
pub fn report_input_for(read: &ReadResult, file_name: &str) -> ReportInput {
    let format_name = match format_registry().find(&read.format_id) {
        Ok(descriptor) => descriptor.human_name,
        Err(_) => read.format_id.clone(),
    };
    let warnings = read
        .warnings
        .iter()
        .map(|warning| ReportMessage {
            text: warning.message.clone(),
            source: SourceRecord {
                format: format_name.clone(),
                file_name: if warning.file_name.is_empty() {
                    String::from(file_name)
                } else {
                    warning.file_name.clone()
                },
                record_number: warning.record,
                ..Default::default()
            },
        })
        .collect();
    ReportInput {
        file_name: String::from(file_name),
        sibling_files: read
            .siblings_read
            .iter()
            .map(|sibling| sibling.name.clone())
            .collect(),
        format_id: read.format_id.clone(),
        format_name,
        parser_version: read.parser_version.clone(),
        records_read: read.records_read,
        records_skipped: read.records_skipped,
        warnings,
        not_carried: read.not_carried.clone(),
    }
}

// What a panicking reader said, as std::exception::what() says it in the C++.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        String::from(*text)
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        String::from("the reader panicked")
    }
}

/// Reads `bytes` as format `format_id`.
///
/// What it adds to the reader's own work:
///
/// * `file_name` goes through source_file_name() before the reader sees it;
/// * files over MAX_SURVEY_FILE_BYTES are refused (InvalidArgument) unread;
/// * every sibling request is checked (a plain name only - see SiblingLookup),
///   counted against MAX_SIBLING_FILES, and RECORDED with its bytes in
///   ReadResult::siblings_read, so a survey job can keep exactly what was read
///   and re-read it later without the folder;
/// * the project the reader returns must pass validate_project(); one that does
///   not is FileImportFailure, naming the format - a reader that returns an
///   inconsistent project has a bug, and the file should not be half-imported
///   because of it;
/// * a reader that panics is caught and reported (Internal), never allowed to
///   take the application down;
/// * the result is stamped with the format's id and parser version.
///
/// NotFound for an unknown format; Unsupported for a format with no reader.
pub fn read_survey(
    registry: &FormatRegistry,
    format_id: &str,
    bytes: &[u8],
    file_name: &str,
    options: &ReadOptions,
) -> Result<ReadResult> {
    let descriptor = registry.find(format_id)?;
    let Some(reader) = registry.reader(format_id) else {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "Katana recognises {} files but cannot import them in one step",
                descriptor.human_name
            ),
        )
        .with_context(if format_id == DELIMITED_POINTS_FORMAT_ID {
            "a coordinate file is read through its column layout"
        } else {
            "no reader is registered for this format"
        }));
    };
    let name = source_file_name(file_name);
    if bytes.len() > MAX_SURVEY_FILE_BYTES {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "{name} is {} bytes, larger than the {MAX_SURVEY_FILE_BYTES} bytes a survey file may be",
                bytes.len()
            ),
        ));
    }

    // Every sibling the reader fetches goes through here: checked, counted and
    // kept, bytes and all, so a survey job can store exactly what was read.
    let fetched: Arc<Mutex<Vec<SiblingFile>>> = Arc::default();
    let mut guarded = options.clone();
    if let Some(lookup) = options.siblings.clone() {
        let fetched = Arc::clone(&fetched);
        guarded.siblings = Some(Arc::new(move |sibling: &str| -> Result<Vec<u8>> {
            check_sibling_name(sibling)?;
            {
                let already = fetched.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(file) = already.iter().find(|file| file.name == sibling) {
                    return Ok(file.bytes.clone());
                }
                if already.len() >= MAX_SIBLING_FILES {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        format!(
                            "refused to read '{sibling}': one import may read at most \
                             {MAX_SIBLING_FILES} other files"
                        ),
                    ));
                }
            }
            let read = lookup(sibling)?;
            if read.len() > MAX_SURVEY_FILE_BYTES {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{sibling} is larger than a survey file may be"),
                ));
            }
            fetched
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(SiblingFile {
                    name: String::from(sibling),
                    bytes: read.clone(),
                });
            Ok(read)
        }));
    }

    // A panic is the C++ reader's exception. Running out of memory, which the
    // C++ catches as std::bad_alloc, aborts a Rust program instead.
    let outcome = catch_unwind(AssertUnwindSafe(|| reader(bytes, &name, &guarded)));
    let mut result = match outcome {
        Ok(result) => result?,
        Err(payload) => {
            return Err(Error::new(
                ErrorCode::Internal,
                format!(
                    "the {} reader failed on {name}: {}",
                    descriptor.human_name,
                    panic_message(&*payload)
                ),
            ));
        }
    };
    if let Err(error) = validate_project(&result.project) {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "the {} reader produced data from {name} that does not hold together: {}",
                descriptor.human_name, error.message
            ),
        )
        .with_context(error.context));
    }
    result.format_id = descriptor.id;
    result.parser_version = descriptor.parser_version;
    result.siblings_read =
        std::mem::take(&mut *fetched.lock().unwrap_or_else(PoisonError::into_inner));
    Ok(result)
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

/// A SiblingLookup over one folder on disk: a plain name only, a regular file
/// only, at most `max_bytes` (MAX_SURVEY_FILE_BYTES is the usual cap). Where the
/// exact name is not there, the folder is searched for a name that matches
/// ignoring ASCII case, because the files of a DBX job name each other in
/// whatever case the instrument felt like and a case sensitive file system
/// would otherwise lose them.
pub fn siblings_in_folder(folder: PathBuf, max_bytes: usize) -> SiblingLookup {
    Arc::new(move |name: &str| -> Result<Vec<u8>> {
        check_sibling_name(name)?;
        // A name inside a field file is UTF-8 (or ASCII, which is UTF-8), and
        // the path is built from it as such.
        let mut candidate = folder.join(path_from_utf8(name));
        if !is_regular_file(&candidate) {
            candidate = PathBuf::new();
            if let Ok(entries) = std::fs::read_dir(folder.as_path()) {
                for entry in entries {
                    let Ok(entry) = entry else {
                        break;
                    };
                    let path = entry.path();
                    if is_regular_file(&path)
                        && equals_ignoring_ascii_case(
                            &path_to_utf8(Path::new(&entry.file_name())),
                            name.as_bytes(),
                        )
                    {
                        candidate = path;
                        break;
                    }
                }
            }
        }
        if candidate.as_os_str().is_empty() {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("there is no file called '{name}' beside the file being read"),
            ));
        }
        let size = match std::fs::metadata(candidate.as_path()) {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                return Err(Error::new(
                    ErrorCode::FileImportFailure,
                    format!("could not read the size of '{name}'"),
                )
                .with_context(format!("{error}")));
            }
        };
        if size > max_bytes as u64 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("'{name}' is {size} bytes, larger than the {max_bytes} bytes allowed"),
            ));
        }
        let unreadable = || {
            Error::new(
                ErrorCode::FileImportFailure,
                format!("could not read '{name}'"),
            )
        };
        // Exactly the size just checked, as the C++ reads it.
        let mut bytes = vec![0_u8; size as usize];
        let mut file = File::open(candidate.as_path()).map_err(|_| unreadable())?;
        file.read_exact(&mut bytes).map_err(|_| unreadable())?;
        Ok(bytes)
    })
}

/// A SiblingLookup over files already in memory - a survey job re-reading what
/// it stored. Same name rules, same case-insensitive fallback.
pub fn siblings_in_memory(files: Vec<SiblingFile>) -> SiblingLookup {
    Arc::new(move |name: &str| -> Result<Vec<u8>> {
        check_sibling_name(name)?;
        if let Some(file) = files.iter().find(|file| file.name == name) {
            return Ok(file.bytes.clone());
        }
        if let Some(file) = files
            .iter()
            .find(|file| equals_ignoring_ascii_case(file.name.as_bytes(), name.as_bytes()))
        {
            return Ok(file.bytes.clone());
        }
        Err(Error::new(
            ErrorCode::NotFound,
            format!("there is no file called '{name}' in the stored job"),
        ))
    })
}

/// The ImportResult shape the import wizard's report page already shows, for a
/// reader's result: warnings through describe(), counts as they are.
pub fn to_import_result(result: ReadResult) -> ImportResult {
    let mut warnings = Vec::with_capacity(result.warnings.len() + result.not_carried.len());
    for warning in &result.warnings {
        warnings.push(describe(warning));
    }
    for missing in &result.not_carried {
        warnings.push(format!("not in the file: {missing}"));
    }
    ImportResult {
        project: result.project,
        format_id: result.format_id,
        warnings,
        records_read: result.records_read,
        records_skipped: result.records_skipped,
    }
}
