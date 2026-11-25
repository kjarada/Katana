//! IMPORT and EXPORT of a .dwg (katana_dwg, docs/dwg.md), split as dxf_verbs splits a DXF's:
//! the file read or written by a pure step, then what it brought applied to the drawing as ONE
//! undo step with the reply records. The executor's IMPORT and EXPORT run them for a .dwg.
//!
//! A DWG's entities come in through katana_dxf's reader and go out through its writer
//! (katana_dwg's lib.rs), so the step, the records and what an export draws around the writer
//! are dxf_verbs' own, called with the DWG's words; only the read and the write are
//! katana_dwg's.

use std::collections::BTreeSet;
use std::path::Path;

use katana_cad::Document;
use katana_cad::import_placement::{ImportPlacement, ImportShift};
use katana_core::Result;
use katana_dwg::{DwgExport, DwgRelease, read_dwg_file, write_dwg_file};
use katana_dxf::DxfImport;
use katana_entity::{EntityId, LeaderContext, Model};
use katana_geometry::Vec2;

use crate::dxf_verbs::{
    apply_drawing_import, drawing_import_options, export_drawing, export_drawing_scoped,
};
use crate::import_records::{path_text, record_text, warning_text};

/// Reads the DWG `file` with `shift` subtracted from every plan coordinate, as read_dxf_import
/// reads a DXF. Pure: no drawing is seen.
pub fn read_dwg_import(file: &Path, shift: Option<Vec2>) -> Result<DxfImport> {
    read_dwg_file(file, &drawing_import_options(file, shift))
}

/// The linetypes, layers and entities `imported` brings as ONE undo step (katana_dxf's
/// import_command), and the records: imported (kind=dwg, format="DWG R2018"), then placed,
/// a tally per entity kind by its DXF name, and the warnings - acadrust's first.
pub fn apply_dwg_import(
    document: &mut Document,
    imported: DxfImport,
    file: &Path,
    placement: &ImportPlacement,
    placed: &ImportShift,
) -> Result<String> {
    apply_drawing_import(document, imported, file, placement, placed, "dwg", "DWG")
}

/// Writes `model` to `file` as a DWG of `release`, its annotation drawn as write_dxf_export
/// draws it for a DXF. Pure, as that is.
pub fn write_dwg_export(
    model: &Model,
    annotation_scale: f64,
    file: &Path,
    leaders: &LeaderContext,
    release: DwgRelease,
) -> Result<DwgExport> {
    let (mut written, blank) =
        export_drawing(model, annotation_scale, leaders, |model, options| {
            write_dwg_file(model, file, options, release)
        })?;
    written.warnings.extend(blank);
    Ok(written)
}

/// The same for the entities `only` of `model`, an EXPORT's scope, as write_dxf_export_scoped
/// writes a DXF's: the annotation drawn from the whole of `model`, then `model` - the caller's
/// copy - cut to `only` in place.
pub fn write_dwg_export_scoped(
    model: &mut Model,
    annotation_scale: f64,
    file: &Path,
    leaders: &LeaderContext,
    only: &BTreeSet<EntityId>,
    release: DwgRelease,
) -> Result<DwgExport> {
    let (mut written, blank) =
        export_drawing_scoped(model, annotation_scale, leaders, only, |model, options| {
            write_dwg_file(model, file, options, release)
        })?;
    written.warnings.extend(blank);
    Ok(written)
}

/// The records of a DWG written: exported - its release in format= as a DXF's says "DXF
/// R2000" - then the warnings. `file` is the name it is known by, which a staged write is not
/// written to.
pub fn dwg_export_records(written: &DwgExport, file: &Path) -> String {
    let mut reply = format!(
        "exported file={} kind=dwg driver=DWG format={} entities={} skipped={} layers={} \
         bytes={}",
        record_text(&path_text(file)),
        record_text(&format!("DWG {}", written.release.name())),
        written.entities_written,
        written.entities_skipped,
        written.layers_written,
        written.bytes_written
    );
    for warning in &written.warnings {
        reply.push('\n');
        reply.push_str(&warning_text(warning));
    }
    reply
}
