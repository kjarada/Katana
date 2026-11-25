//! Entities, layers and linetypes -> a DWG (docs/dwg.md, "Writing"): katana_dxf's writer writes
//! the model as DXF - the one mapping of Katana's entities onto a CAD drawing, with its extended
//! data for what the format cannot hold - and acadrust writes that drawing as a DWG of the
//! release asked for. What the DXF keeps, the DWG keeps; from R2004 a colour no index is
//! exactly is the entity's or layer's own colour too, where R2000's DXF has the nearest index.

use std::path::{Path, PathBuf};

use katana_core::error::{ErrorCode, Result, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_dxf::{ExportOptions, write_dxf};
use katana_entity::Model;

use crate::bridge;
use crate::release::DwgRelease;

/// What a write wrote; the counts are the DXF writer's, since it chose what goes out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DwgExport {
    /// The file (write_dwg); empty from write_dwg_file.
    pub bytes: Vec<u8>,
    pub release: DwgRelease,
    /// Katana entities written.
    pub entities_written: usize,
    pub entities_skipped: usize,
    pub layers_written: usize,
    pub linetypes_written: usize,
    pub bytes_written: usize,
    pub warnings: Vec<String>,
}

/// `model` (or what `options` takes of it, as `katana_dxf::write_dxf` takes it) as a DWG of
/// `release`. Fails as the DXF writer does, with FileExportFailure when acadrust can read
/// nothing of that DXF or cannot write the DWG (the failure is acadrust's, in its words), and
/// with Unsupported for a release before R2007 when a layer's path is longer than its extended
/// data holds. A record of the DXF acadrust could not read is left out of the DWG and said.
pub fn write_dwg(
    model: &Model,
    options: &ExportOptions<'_>,
    release: DwgRelease,
) -> Result<DwgExport> {
    let drawing = write_dxf(model, options)?;
    let encoded = bridge::dxf_to_dwg(&drawing.text, release)?;
    // The DXF writer's warnings, then acadrust's about what of that DXF it could not carry.
    let mut warnings = drawing.warnings;
    warnings.extend(encoded.warnings);
    Ok(DwgExport {
        bytes_written: encoded.bytes.len(),
        bytes: encoded.bytes,
        release,
        entities_written: drawing.entities_written,
        entities_skipped: drawing.entities_skipped,
        layers_written: drawing.layers_written,
        linetypes_written: drawing.linetypes_written,
        warnings,
    })
}

/// std::error_code::message() is the system's text alone; io::Error's Display adds
/// " (os error N)" after it (as katana_dxf's writer says it).
fn os_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error() {
        let suffix = format!(" (os error {code})");
        if let Some(message) = text.strip_suffix(&suffix) {
            return message.to_owned();
        }
    }
    text
}

/// Writes to `path`, replacing it, as `katana_dxf::write_dxf_file` writes a DXF: beside the
/// target first, then over it, so a write that fails part way leaves the previous file and no
/// half of a new one. FileExportFailure when the file cannot be written.
pub fn write_dwg_file(
    model: &Model,
    path: &Path,
    options: &ExportOptions<'_>,
    release: DwgRelease,
) -> Result<DwgExport> {
    let mut written = write_dwg(model, options, release)?;
    let context = || utf8_or_windows1252(path_to_utf8(path));
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".partial");
    let temporary = PathBuf::from(temporary);
    if std::fs::write(temporary.as_path(), &written.bytes).is_err() {
        let _ = std::fs::remove_file(temporary.as_path());
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            context(),
        ));
    }
    if let Err(error) = std::fs::rename(temporary.as_path(), path) {
        let _ = std::fs::remove_file(temporary.as_path());
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!("the file could not be replaced: {}", os_message(&error)),
            context(),
        ));
    }
    written.bytes = Vec::new();
    Ok(written)
}
