//! IMPORT and EXPORT of a .dgn, natively (katana_dgn, docs/dgn.md): the steps
//! the geoprocessing executor runs for a MicroStation V7 design, as
//! dxf_verbs.rs holds the DXF's. Reading and writing one needs no third-party
//! library; the export's default seed designs are GDAL's data files.
//!
//! Split as the executor runs a line: the file read or written by a pure
//! step, then what it brought applied to the drawing as ONE undo step - the
//! DXF import's own (katana_dxf::import_parts), so the two cannot differ -
//! with the reply records.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use katana_cad::Document;
use katana_cad::annotation::drawing::estimated_measure;
use katana_cad::annotation::export_annotation::{
    DrawnAnnotation, draw_annotation_for_export, plain_dimension_shapes,
};
use katana_cad::dimension_draw::{build_dimension, resolve_dimension_style};
use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{ErrorCode, Result};
use katana_dgn::{
    DgnExport, DgnImport, ExportOptions, ImportOptions, has_heights, read_dgn_file, seed_info,
    write_dgn_file,
};
use katana_entity::{DimensionKind, EntityId, Geometry, LeaderContext, Model};
use katana_geometry::Vec2;

use crate::dxf_verbs::blank_leaders;
use crate::import_records::{
    bounds_text, path_text, record_text, tally_record, warning_text,
};

/// GDAL's seed designs (its data's seed_2d.dgn and seed_3d.dgn), the ones
/// GDAL's own DGN writer starts from.
pub const SEED_2D: &str = "seed_2d.dgn";
pub const SEED_3D: &str = "seed_3d.dgn";

/// Reads `file` with `shift` subtracted from every plan coordinate (cad's
/// import_placement: the file is read again with the shift, so the one
/// reader moves every kind of geometry alike). Pure: no drawing is seen.
pub fn read_dgn_import(file: &Path, shift: Option<Vec2>) -> Result<DgnImport> {
    let options = ImportOptions {
        source_name: file
            .file_name()
            .map(|name| path_text(Path::new(name)))
            .unwrap_or_default(),
        origin_shift: shift,
        ..ImportOptions::default()
    };
    read_dgn_file(file, &options)
}

/// The layers and entities `imported` brings as ONE undo step, and the
/// records: imported, a tally per element kind, the reader's warnings. The
/// placed record is the executor's to put after the first.
pub fn apply_dgn_import(
    document: &mut Document,
    mut imported: DgnImport,
    file: &Path,
) -> Result<String> {
    let count = imported.entities.len();
    let layers = imported.layers.len();
    // None when the file brings nothing the drawing lacks, which is not a
    // failure: the file was read, and its tally says what it held.
    let command = katana_dxf::import_parts(
        &mut imported.entities,
        &imported.layers,
        &[],
        document.model(),
    );
    if let Some(command) = command {
        document.execute(command)?;
    }
    let mut reply = format!(
        "imported file={} kind=dgn format={} entities={count} layers={layers} units={} \
         uor_per_master={} bounds={}",
        record_text(&path_text(file)),
        record_text(imported.format()),
        record_text(&imported.units.master),
        format_exact_real(imported.units.per_master()),
        bounds_text(&imported.bounds)
    );
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

/// The seed a .dgn is written on: `given` (seed= on the line), whose working
/// units and global origin are kept; else GDAL's seed_3d.dgn when the drawing
/// holds heights and seed_2d.dgn when it holds none, with Katana's units. The
/// bytes, and whether the seed's units are kept.
pub fn seed_for(model: &Model, given: Option<&Path>) -> Result<(Vec<u8>, bool)> {
    if let Some(given) = given {
        let bytes = std::fs::read(given).map_err(|_| {
            make_error(
                ErrorCode::NotFound,
                "the seed file cannot be read",
                path_text(given),
            )
        })?;
        seed_info(&bytes).map_err(|mut error| {
            error.context = path_text(given);
            error
        })?;
        return Ok((bytes, true));
    }
    let wanted = if has_heights(model) { SEED_3D } else { SEED_2D };
    let other = if wanted == SEED_3D { SEED_2D } else { SEED_3D };
    // A 2D seed for a drawing with heights writes it in plan, and the writer
    // says so; a 3D seed for one without writes every Z as 0, which reads
    // back as in plan.
    let found: Option<PathBuf> =
        katana_gis::gdal_data_file(wanted).or_else(|| katana_gis::gdal_data_file(other));
    let Some(path) = found else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no seed design to write a DGN on: GDAL's seed_2d.dgn and seed_3d.dgn are not in \
             its data folder; give seed=<a V7 design file>",
            String::from(wanted),
        ));
    };
    let bytes = std::fs::read(&path).map_err(|_| {
        make_error(
            ErrorCode::NotFound,
            "GDAL's seed design cannot be read",
            path_text(&path),
        )
    })?;
    Ok((bytes, false))
}

/// The annotation drawn out for a DGN: what the plan view draws of labels,
/// leaders and dimensions (cad's export_annotation), aligned dimensions
/// included - the DXF writer draws those itself, and the DGN writer has no
/// drawing of its own, so they are drawn here by the one dimension drawing.
fn drawn_for_dgn(model: &Model, scale: f64, leaders: &LeaderContext) -> DrawnAnnotation {
    let mut drawn = draw_annotation_for_export(model, scale, &estimated_measure(), leaders);
    for entity in model.entities.iter() {
        if let Geometry::Dimension(dimension) = &entity.geometry
            && dimension.kind == DimensionKind::Aligned
        {
            let drawing = build_dimension(dimension, resolve_dimension_style(model, entity), scale);
            drawn.insert(entity.id, plain_dimension_shapes(&drawing));
        }
    }
    drawn
}

/// Writes `model` to `file` on `seed` (seed_for's), its annotation drawn at
/// `annotation_scale` as the plan view draws it. Pure: reads only `model`
/// and the context, which holds copies.
pub fn write_dgn_export(
    model: &Model,
    annotation_scale: f64,
    file: &Path,
    leaders: &LeaderContext,
    seed: Option<&Path>,
) -> Result<DgnExport> {
    let drawn = drawn_for_dgn(model, annotation_scale, leaders);
    let (seed, keep_seed_units) = seed_for(model, seed)?;
    let options = ExportOptions {
        annotation_scale,
        drawn: Some(&drawn),
        keep_seed_units,
        ..ExportOptions::default()
    };
    let mut written = write_dgn_file(model, &seed, file, &options)?;
    written.warnings.extend(blank_leaders(model, &drawn, None));
    Ok(written)
}

/// The same for the entities `only` of `model` (an EXPORT's scope): the
/// annotation is drawn from the WHOLE of `model` first, as the DXF's is
/// (dxf_verbs.rs, write_dxf_export_scoped), then `model` is cut to `only` IN
/// PLACE and written. `model` is the caller's copy to cut.
pub fn write_dgn_export_scoped(
    model: &mut Model,
    annotation_scale: f64,
    file: &Path,
    leaders: &LeaderContext,
    only: &BTreeSet<EntityId>,
    seed: Option<&Path>,
) -> Result<DgnExport> {
    let mut drawn = drawn_for_dgn(model, annotation_scale, leaders);
    let blank = blank_leaders(model, &drawn, Some(only));
    let left: Vec<EntityId> = model
        .entities
        .ids()
        .into_iter()
        .filter(|id| !only.contains(id))
        .collect();
    for id in left {
        // Each id is the model's own, just listed, so the removal cannot miss.
        let _ = model.entities.remove(id);
        drawn.remove(&id);
    }
    let (seed, keep_seed_units) = seed_for(model, seed)?;
    let options = ExportOptions {
        annotation_scale,
        drawn: Some(&drawn),
        keep_seed_units,
        ..ExportOptions::default()
    };
    let mut written = write_dgn_file(model, &seed, file, &options)?;
    written.warnings.extend(blank);
    Ok(written)
}

/// The records of a DGN written: exported, a level record for each level
/// and the layer it holds (V7 levels are numbers; the file names none), then
/// the writer's warnings. `file` is the name it is known by, which a staged
/// write is not written to.
pub fn dgn_export_records(written: &DgnExport, file: &Path) -> String {
    let mut reply = format!(
        "exported file={} kind=dgn driver=DGN format={} entities={} skipped={} elements={} \
         levels={} units={} uor_per_master={} bytes={}",
        record_text(&path_text(file)),
        record_text(written.format()),
        written.entities_written,
        written.entities_skipped,
        written.elements_written,
        written.levels.len(),
        record_text(&written.units.master),
        format_exact_real(written.units.per_master()),
        written.bytes_written
    );
    for level in &written.levels {
        reply.push_str(&format!(
            "\nlevel number={} layer={}",
            level.level,
            record_text(&level.layer)
        ));
    }
    for warning in &written.warnings {
        reply.push('\n');
        reply.push_str(&warning_text(warning));
    }
    reply
}
