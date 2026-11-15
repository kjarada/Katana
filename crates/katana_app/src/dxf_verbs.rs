//! IMPORT and EXPORT of a .dxf, natively (katana_dxf, docs/dxf.md); the port
//! of src/katana_app/dxf_verbs.hpp/.cpp. Reading and writing a DXF needs no
//! third-party library.
//!
//! Split as the geoprocessing executor runs a line: the file read or written
//! by a pure step, then what it brought applied to the drawing as ONE undo
//! step with the reply records (import_records). IMPORT and EXPORT are the
//! executor's, which run these steps for a .dxf - as a job in the window. One
//! DXF import and one DXF export, whichever front end asks.
//!
//! The C++'s runDxfVerb, which runs the steps back to back in a build without
//! GDAL, has no Rust: the Rust app always has GDAL, so the executor's IMPORT
//! and EXPORT are the only callers (step 6 plan, section 8, choice 11).

use std::collections::BTreeSet;
use std::path::Path;

use katana_cad::Document;
use katana_cad::annotation::drawing::estimated_measure;
use katana_cad::annotation::export_annotation::{DrawnAnnotation, draw_annotation_for_export};
use katana_cad::import_placement::{ImportPlacement, ImportShift};
use katana_core::Result;
use katana_dxf::{
    DxfExport, DxfImport, ExportOptions, ImportOptions, read_dxf_file, write_dxf_file,
};
use katana_entity::leader_values::is_smart;
use katana_entity::{EntityId, Geometry, LeaderContext, Model};
use katana_geometry::Vec2;

use crate::import_records::{
    bounds_text, path_text, placed_record, record_text, tally_record, warning_text,
};

// A smart leader that came out with no note at all - the entity its tip is on
// is not in the drawing, or has none of the values its template names - is
// written as a bare arrow. The file is what it is; the export says so rather
// than leave a callout silently blank. `only`: just those entities' leaders.
fn blank_leaders(
    model: &Model,
    drawn: &DrawnAnnotation,
    only: Option<&BTreeSet<EntityId>>,
) -> Vec<String> {
    let mut said = Vec::new();
    model.entities.for_each(|entity| {
        let Geometry::Leader(leader) = &entity.geometry else {
            return;
        };
        if !is_smart(leader) || only.is_some_and(|only| !only.contains(&entity.id)) {
            return;
        }
        let has_note = drawn.get(&entity.id).is_some_and(|shapes| {
            shapes
                .iter()
                .any(|shape| matches!(shape, Geometry::Text(_)))
        });
        if !has_note {
            said.push(format!(
                "leader #{} is written without its note: the entity its tip is on is not in the \
                 drawing, or has none of the values its template names",
                entity.id
            ));
        }
    });
    said
}

/// Reads `file` with `shift` subtracted from every plan coordinate (cad's
/// import_placement: the file is read again with the shift, so the one reader
/// moves every kind of geometry alike). Pure: no drawing is seen. The C++'s
/// default `shift` is None.
pub fn read_dxf_import(file: &Path, shift: Option<Vec2>) -> Result<DxfImport> {
    let options = ImportOptions {
        source_name: file
            .file_name()
            .map(|name| path_text(Path::new(name)))
            .unwrap_or_default(),
        origin_shift: shift,
        ..ImportOptions::default()
    };
    read_dxf_file(file, &options)
}

/// The linetypes, layers and entities `imported` brings as ONE undo step, and
/// the records: imported, then placed (when `placement` is not Keep), a tally
/// per entity kind, the reader's warnings.
pub fn apply_dxf_import(
    document: &mut Document,
    mut imported: DxfImport,
    file: &Path,
    placement: &ImportPlacement,
    placed: &ImportShift,
) -> Result<String> {
    let count = imported.entities.len();
    let layers = imported.layers.len();
    let linetypes = imported.linetypes.len();
    // Linetypes, layers and entities: ONE undo step (katana_dxf's
    // import_command). None when the file brings nothing the drawing lacks,
    // which is not a failure: the file was read, and its tally says what it
    // held.
    let command = katana_dxf::import_command(&mut imported, document.model());
    if let Some(command) = command {
        document.execute(command)?;
    }
    let format = if imported.release.is_empty() {
        String::from("DXF")
    } else {
        format!("DXF {}", imported.release)
    };
    let mut reply = format!(
        "imported file={} kind=dxf format={} entities={count} layers={layers} \
         linetypes={linetypes} bounds={}",
        record_text(&path_text(file)),
        record_text(&format),
        bounds_text(&imported.bounds)
    );
    let record = placed_record(placement, placed);
    if !record.is_empty() {
        reply.push('\n');
        reply.push_str(&record);
    }
    for tally in &imported.tally {
        reply.push('\n');
        reply.push_str(&tally_record(&tally.kind, tally.read, tally.imported));
    }
    for warning in &imported.warnings {
        reply.push('\n');
        reply.push_str(&warning_text(warning));
    }
    Ok(reply)
}

/// Writes `model` to `file`, its annotation drawn at `annotation_scale` as the
/// plan view draws it (cad's export_annotation), a leader's note reading the
/// drawing's surfaces and alignments through `leaders` (cad's leader_presets,
/// leader_context; the C++'s default is an empty context). Pure: reads only
/// `model` and the context, which holds copies, so a front end may hand it a
/// copy of its drawing to write on another thread.
pub fn write_dxf_export(
    model: &Model,
    annotation_scale: f64,
    file: &Path,
    leaders: &LeaderContext,
) -> Result<DxfExport> {
    // Labels and every dimension kind drawn out as the plan view draws them,
    // at the drawing's annotation scale.
    let drawn = draw_annotation_for_export(model, annotation_scale, &estimated_measure(), leaders);
    let options = ExportOptions {
        annotation_scale,
        drawn: Some(&drawn),
        ..ExportOptions::default()
    };
    let mut written = write_dxf_file(model, file, &options)?;
    written.warnings.extend(blank_leaders(model, &drawn, None));
    Ok(written)
}

/// The same for the entities `only` of `model` (an EXPORT's scope; the C++'s
/// second writeDxfExport): the annotation is drawn from the WHOLE of `model`
/// first - a smart leader reads the entity its tip is on, a label the entity
/// it labels, and drawn after the scope had taken that entity away each said
/// nothing, a bare arrow in the file - and then `model` is cut to `only` IN
/// PLACE and written with the annotation of the entities that stay. `model`
/// is the caller's copy to cut: it is not the Document's.
pub fn write_dxf_export_scoped(
    model: &mut Model,
    annotation_scale: f64,
    file: &Path,
    leaders: &LeaderContext,
    only: &BTreeSet<EntityId>,
) -> Result<DxfExport> {
    let mut drawn =
        draw_annotation_for_export(model, annotation_scale, &estimated_measure(), leaders);
    // Only then the scope: what it left out is not written, and neither is
    // the drawing of an annotation it left out.
    let blank = blank_leaders(model, &drawn, Some(only));
    let left: Vec<EntityId> = model
        .entities
        .ids()
        .into_iter()
        .filter(|id| !only.contains(id))
        .collect();
    for id in left {
        // Each id is the model's own, just listed, so the removal cannot
        // miss; the C++ discards its status likewise.
        let _ = model.entities.remove(id);
        drawn.remove(&id);
    }
    let options = ExportOptions {
        annotation_scale,
        drawn: Some(&drawn),
        ..ExportOptions::default()
    };
    let mut written = write_dxf_file(model, file, &options)?;
    written.warnings.extend(blank);
    Ok(written)
}

/// The records of a DXF written: exported, then the writer's warnings.
/// `file` is the name it is known by, which a staged write is not written to.
pub fn dxf_export_records(written: &DxfExport, file: &Path) -> String {
    // The writer writes R2000, the release every reader of DXF reads.
    let mut reply = format!(
        "exported file={} kind=dxf driver=DXF format={} entities={} skipped={} layers={} \
         bytes={}",
        record_text(&path_text(file)),
        record_text("DXF R2000"),
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
