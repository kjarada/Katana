//! What reading the small JSON records beside the programs and in the per-user state folder have
//! in common: the install's own (install.rs: install.json, installed.json) and the user's
//! (state.json, last.json, which the state module reads with these). In the C++ they are the
//! anonymous helpers at the top of src/katana_update/state.cpp; writing a record (the temporary
//! file, the flush, the rename) is the state module's, with it.

use crate::fsops::path_text;
use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::{Error, ErrorCode, Result};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

// install.json, installed.json and last.json are a few lines; state.json lists the releases that
// failed their health check, one number each. Anything longer was not written by this program,
// and the files are user-writable, so they are not read whole into memory.
pub(crate) const MAX_RECORD_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_STATE_BYTES: u64 = 1024 * 1024;

pub(crate) fn parse_failure(message: impl Into<String>, file: &Path) -> Error {
    make_error(ErrorCode::ParseFailure, message, path_text(file))
}

// Where a file could not be opened or read: the system's error number first on Windows (the C++
// reads GetLastError there), the path alone elsewhere (the C++'s stream says no more).
fn os_context(error: &std::io::Error, file: &Path) -> String {
    if cfg!(windows) {
        format!(
            "os_error={}; {}",
            error.raw_os_error().unwrap_or(0),
            path_text(file)
        )
    } else {
        path_text(file)
    }
}

// The whole file, read in blocks and stopped the moment it is longer than `max_bytes`: the files
// are user-writable, and what is read is bounded by this program, not by the file.
//
// Windows: Rust opens a file with every share mode, DELETE included (the C++ asks CreateFileW for
// the same), so that this reader never stops another process from renaming or deleting the
// record.
pub(crate) fn read_capped(file: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let mut opened = File::open(file).map_err(|error| {
        make_error(
            ErrorCode::FileImportFailure,
            "the record cannot be opened",
            os_context(&error, file),
        )
    })?;
    let mut text = Vec::new();
    let mut block = [0u8; 16 * 1024];
    loop {
        let got = match opened.read(&mut block) {
            Ok(got) => got,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(make_error(
                    ErrorCode::FileImportFailure,
                    "the record cannot be read",
                    os_context(&error, file),
                ));
            }
        };
        if got == 0 {
            break;
        }
        text.extend_from_slice(&block[..got]);
        if text.len() as u64 > max_bytes {
            return Err(parse_failure(
                "the record is longer than any this program writes",
                file,
            ));
        }
    }
    Ok(text)
}

/// The file's bytes; None when there is no such file; an error when it is there and cannot be
/// read or is longer than `max_bytes`.
pub(crate) fn read_optional_text(file: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>> {
    // The status of what the path names, links followed, as std::filesystem::status gives it: a
    // missing file or a missing folder on the way to it is "not found", not an error.
    let metadata = match std::fs::metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(None);
        }
        Err(error) => {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "the record cannot be read",
                format!("{error}; {}", path_text(file)),
            ));
        }
    };
    if !metadata.is_file() {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the record cannot be read: it is not a file",
            path_text(file),
        ));
    }
    read_capped(file, max_bytes).map(Some)
}

/// A JSON object with an integer `schema` member: ParseFailure when the text is not that,
/// Unsupported when the schema is not `supported`. Read as nlohmann reads it with no callback and
/// no exceptions (katana_core::json): a text that does not read is no object, a member written
/// twice keeps its last value, and there is no limit of depth.
pub(crate) fn parse_record(text: &[u8], file: &Path, supported: i64) -> Result<Json> {
    let Ok(record) = Json::parse(text) else {
        return Err(parse_failure("the record is not a JSON object", file));
    };
    if !record.is_object() {
        return Err(parse_failure("the record is not a JSON object", file));
    }
    let known = match record.find("schema") {
        Some(Json::NumberUnsigned(schema)) => *schema == supported as u64,
        Some(Json::NumberInteger(schema)) => *schema == supported,
        _ => {
            return Err(parse_failure("the record has no integer \"schema\"", file));
        }
    };
    if !known {
        return Err(make_error(
            ErrorCode::Unsupported,
            "the record is of a schema this version does not know (it was written by a newer or \
             older Katana)",
            path_text(file),
        ));
    }
    Ok(record)
}

/// A string member that must be there (`required`) or may be absent (then `fallback`).
pub(crate) fn string_member(
    record: &Json,
    name: &str,
    required: bool,
    file: &Path,
    fallback: &str,
) -> Result<String> {
    let Some(member) = record.find(name) else {
        if required {
            return Err(parse_failure(format!("the record has no \"{name}\""), file));
        }
        return Ok(fallback.to_owned());
    };
    match member.as_str() {
        Some(text) => Ok(text.to_owned()),
        None => Err(parse_failure(format!("\"{name}\" is not a string"), file)),
    }
}
