//! The reference layers a project records (docs/interop.md, "Reference
//! layers"), the port of src/katana_app/geo/references.hpp/.cpp: each layer's
//! source and display settings, kept in the drawing's metadata
//! (katana_storage's ProjectMetadata::reference_layers) as the records
//! interop's reference_data writes, and read again when it opens (REFS
//! RESTORE).

use katana_interop::reference_data::reference_records;

use super::geo_verbs::Context;

/// The drawing's metadata made to record the reference layers as they are
/// now: what both front ends do before a SAVE that has somewhere to go, as
/// they record the customisation. Taken at the save, not at each change, so
/// a raster imported or hidden does not by itself make the drawing ask to be
/// saved - reference data was session data, and File > New after an import
/// still just drops it.
pub fn record_references(context: &mut Context<'_>) {
    let mut metadata = context.document.metadata().clone();
    let project = context.document.project_directory();
    metadata.reference_layers = reference_records(context.reference, project.as_deref());
    // set_metadata changes, and marks, nothing when the records are the same.
    context.document.set_metadata(metadata);
}

/// Whether the drawing's metadata records any reference layer: an OPEN then
/// runs REFS RESTORE.
pub fn records_references(context: &Context<'_>) -> bool {
    !context.document.metadata().reference_layers.is_empty()
}
