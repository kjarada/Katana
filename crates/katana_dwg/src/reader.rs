//! A DWG file -> the entities, layers and linetypes katana_dxf's reader makes of it
//! (docs/dwg.md, "Reading").
//!
//! The file's first six bytes are looked at first, so that what is not a DWG this reader reads
//! is said as what it is - an empty file, a DXF under the wrong extension, a release too old -
//! before acadrust is given it. acadrust then reads it in its failsafe mode: a record it cannot
//! decode is left out and said, as the DXF reader leaves out an entity it cannot use, because
//! one bad record must not cost the user the rest of the drawing. It fails only when nothing of
//! the file can be read.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_dxf::{DxfImport, ImportOptions, read_dxf, release_name};

use crate::bridge;
use crate::release::READ;

/// True for a path ending in .dwg, in any letter case: the one test the front ends route by, as
/// `katana_dxf::is_dxf_path` is for a DXF.
pub fn is_dwg_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"dwg"))
}

fn not_a_dwg(why: &str) -> Error {
    Error::new(ErrorCode::ParseFailure, format!("not a DWG file: {why}"))
}

/// Whether `bytes` begin as a DXF's text does: a byte order mark and blanks maybe, then the
/// group code 0 and SECTION, or the comment group 999 - how every ASCII DXF starts.
fn looks_like_dxf_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(256)];
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    let mut lines = head
        .split(|&byte| byte == b'\n')
        .map(<[u8]>::trim_ascii)
        .skip_while(|line| line.is_empty());
    match lines.next() {
        Some(first) if first == b"0" => lines.next().is_some_and(|second| second == b"SECTION"),
        Some(first) => first == b"999",
        None => false,
    }
}

/// The version string a DWG begins with ("AC1032") and the release it names ("R2018"), when the
/// file is a DWG of a release this reader reads; otherwise why not, as plainly as the bytes say.
fn release_of(bytes: &[u8]) -> Result<(&'static str, &'static str)> {
    if bytes.is_empty() {
        return Err(not_a_dwg("it is empty (0 bytes)"));
    }
    // The sentinel every binary DXF begins with, as katana_dxf's reader knows it.
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(not_a_dwg(
            "it is a binary DXF; save it as an ASCII DXF and import it under the .dxf extension",
        ));
    }
    if looks_like_dxf_text(bytes) {
        return Err(not_a_dwg(
            "it is a DXF (text); give it the .dxf extension to import it",
        ));
    }
    let Some(head) = bytes.get(..6) else {
        return Err(not_a_dwg(&format!(
            "it is {} bytes long, shorter than the 6 of a DWG's version string",
            bytes.len()
        )));
    };
    // "AC1032" - and "AD1032", which acadrust takes for the same release
    // (io/dwg/dwg_version.rs, `DwgVersion::from_version_string`): two letters, then digits
    // (and a dot, in the version strings of releases before R10, "AC1.50").
    let version_like = (head.starts_with(b"AC") || head.starts_with(b"AD"))
        && head[2..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b'.');
    if !version_like {
        return Err(not_a_dwg(
            "it does not begin with a DWG version string (AC1009 to AC1032)",
        ));
    }
    let mut code = [0_u8; 6];
    code.copy_from_slice(head);
    code[1] = b'C';
    // Every byte is an ASCII letter, digit or dot, checked just above.
    let code = String::from_utf8_lossy(&code).into_owned();
    if let Some(known) = READ.iter().find(|known| **known == code) {
        return Ok((*known, release_name(known)));
    }
    let named = release_name(&code);
    let what = if named.is_empty() {
        format!("DWG version {code}, a release this reader does not know,")
    } else {
        format!("{named} DWG ({code})")
    };
    Err(Error::new(
        ErrorCode::Unsupported,
        format!("{what} is not read: Katana reads R12 (AC1009) and R13 (AC1012) to R2018 (AC1032)"),
    ))
}

/// `bytes` is the whole file. What it holds comes back as katana_dxf's reader makes it of the
/// DXF acadrust writes of it (lib.rs): the release the DWG is ("AC1032", "R2018") as the import's
/// version and release, acadrust's warnings - records it could not decode, a section it could
/// not read, entities left out for a number that is not one - before the reader's own.
///
/// ParseFailure for what is not a DWG (empty, too short, a DXF, no version string) and for a
/// DWG nothing of which can be read (damaged, cut short before its section map); Unsupported,
/// naming it, for a release not read; InvalidArgument for a curve tolerance that is not above 0.
pub fn read_dwg(bytes: impl AsRef<[u8]>, options: &ImportOptions) -> Result<DxfImport> {
    let bytes = bytes.as_ref();
    // As the DXF reader refuses it - but before the conversion, not after a large file's.
    let tolerance = options.curve_tolerance;
    if tolerance.is_nan() || tolerance.is_infinite() || tolerance <= 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
        ));
    }
    let (code, release) = release_of(bytes)?;
    let decoded = bridge::dwg_to_dxf(bytes, release)?;
    let mut imported = read_dxf(decoded.dxf.as_slice(), options).map_err(|error| {
        Error::new(
            error.code,
            format!(
                "the {release} DWG was read, and the DXF made of it was not: {}",
                error.message
            ),
        )
    })?;
    imported.version = String::from(code);
    imported.release = String::from(release);
    let mut warnings = decoded.warnings;
    warnings.append(&mut imported.warnings);
    if imported.entities.is_empty() {
        // Not a failure - a drawing may be empty, or hold only what Katana does not read - but
        // said, so that an import of nothing is not taken for one that failed quietly.
        warnings.push(String::from(
            "nothing was imported: the DWG holds no entity Katana reads (the tally says what it \
             holds)",
        ));
    }
    imported.warnings = warnings;
    Ok(imported)
}

/// `read_dwg` of the file at `path`; FileImportFailure when it cannot be opened or read, and
/// every failure carries the path.
pub fn read_dwg_file(path: &Path, options: &ImportOptions) -> Result<DxfImport> {
    let context = || utf8_or_windows1252(path_to_utf8(path));
    let mut file = File::open(path).map_err(|_| {
        make_error(
            ErrorCode::FileImportFailure,
            "cannot open the file",
            context(),
        )
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| {
        make_error(
            ErrorCode::FileImportFailure,
            "cannot read the file",
            context(),
        )
    })?;
    read_dwg(bytes.as_slice(), options).map_err(|mut error| {
        error.context = context();
        error
    })
}
