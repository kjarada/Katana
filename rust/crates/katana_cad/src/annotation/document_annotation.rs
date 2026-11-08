//! The Document's annotation members (document.rs;
//! src/katana_cad/annotation/document_annotation.cpp), kept here with the rest
//! of the annotation code, as the sheet set's are kept with the sheets.

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{ErrorCode, Status};
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;

use crate::document::Document;

/// The metadata key the scale is kept under. The storage layer keeps a key it
/// does not read and writes it back unchanged (ProjectMetadata::unknown_keys),
/// so the scale travels with the project with no schema change.
const ANNOTATION_SCALE_KEY: &str = "annotation_scale";

impl Document {
    // ---- annotation (docs/annotation.md) ----------------------------------------------
    //
    // The scale the plan view draws paper-sized annotation at: text, labels,
    // leaders and paper-sized dimensions are drawn paperMm x scale / 1000
    // model units tall there (a sheet viewport uses its own scale instead).
    // 1 : DEFAULT_ANNOTATION_SCALE until one is chosen.

    pub fn annotation_scale(&self) -> f64 {
        // A value that is not a positive number - edited by hand, or damaged -
        // is not a scale; the default is used and the key left as it is.
        if let Some(text) = self.state.metadata.unknown_keys.get(ANNOTATION_SCALE_KEY)
            && let Some(scale) = parse_finite_double(text)
            && scale > 0.0
        {
            return scale;
        }
        DEFAULT_ANNOTATION_SCALE
    }

    /// Sets it as ONE undoable step; refuses a scale that is not finite and
    /// positive. Choosing the scale it already has records nothing.
    pub fn set_annotation_scale(&mut self, scale: f64) -> Status {
        if !scale.is_finite() || !(scale > 0.0) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "an annotation scale is a positive number, the N of 1 : N",
                format_exact_real(scale),
            ));
        }
        let before = self
            .state
            .metadata
            .unknown_keys
            .get(ANNOTATION_SCALE_KEY)
            .cloned();
        // The default is stored as no key at all, so choosing it again after
        // a change undoes to a project exactly as it was.
        let after = if scale == DEFAULT_ANNOTATION_SCALE {
            None
        } else {
            Some(format_exact_real(scale))
        };
        if before == after {
            return Ok(()); // nothing changed, and nothing to undo
        }
        let command =
            self.metadata_key_command("SET_ANNOTATION_SCALE", ANNOTATION_SCALE_KEY, before, after);
        self.execute(command)
    }
}
