//! The `cad` layer: the engine both front ends drive: the document, the command interpreter and every verb family.
//!
//! The port of src/katana_cad and include/katana/cad (docs/rust.md). One
//! module tree, declared here: a subfolder of the headers is an inline block
//! whose files are in `src/<subfolder>/`, and a module follows the header's
//! path, not the folder of the .cpp that implements it.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

pub mod annotation {
    pub mod associative;
    pub mod command_words;
    pub mod dimension_style_verbs;
    mod document_annotation;
    pub mod drawing;
    pub mod text_layout;
}
pub mod customisation_part;
pub mod customisation_record;
pub mod customisation_state;
pub mod dashing;
pub mod dimension_draw;
pub mod document;
pub mod drawing {
    pub mod construction;
    pub mod drafting;
}
pub mod geo {
    pub mod drape;
    pub mod surface_input;
}
pub mod grading;
pub mod hatch_pattern_rows;
pub mod hatching;
pub mod import_placement;
pub mod layer_overrides;
pub mod plot;
pub mod purge;
pub mod road {
    pub mod road_object_mesh;
}
pub mod section;
pub mod selection;
pub mod selection_style;
pub mod snapping;
pub mod spatial_query;
pub mod style_drawing;
pub mod survey_import;
pub mod symbols;
pub mod terrain {
    pub(crate) mod surface_document;
    #[doc(hidden)]
    pub mod surface_document_io;
    pub mod surface_style;
}
pub mod tool_feedback;
pub mod view_transform;
pub mod viewport_layout;

pub use customisation_part::DefinitionsOfFile;
pub use customisation_record::{
    CustomisationSource, DistinctFiles, RenamedDefinition, RenamedSource,
};
pub use customisation_state::{CustomisationOrigin, CustomisationState};
pub use dashing::DashOptions;
pub use dimension_draw::DimensionDrawing;
pub use document::{Document, DocumentChange, DocumentParts, ListenerHandle};
pub use drawing::drafting::{
    AngleConvention, ConstrainedPoint, DraftingSettings, PrecisePoint, TrackingPoints,
};
pub use grading::{DaylightPoint, FeatureLine, Grading, GradingSide, GradingSlopes};
pub use hatch_pattern_rows::HatchPatternRow;
pub use hatching::{HatchDrawing, HatchOptions};
pub use import_placement::{ImportPlacement, ImportPlacementMode, ImportShift};
pub use layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
pub use plot::{PaperDimensions, PaperSize, PlotColourMode, PlotSettings, Sheet};
pub use purge::PurgeOptions;
pub use section::{
    SampleReason, Section, SectionCrossing, SectionDesign, SectionOptions, SectionSample,
    SectionSurface, SectionSurfaceInput,
};
pub use selection::{BoxSelectionMode, SelectionFilter, SelectionSet};
pub use snapping::{SnapAccept, SnapMode, SnapModes, SnapRequest, SnapResult};
pub use style_drawing::{
    FlatDefinition, FlatDefinitionRun, FlatDefinitionText, LinestyleLayout, LinestyleLayoutOutcome,
    LinestyleOptions, StyleDrawing, StyleStroke, StyleTextMark,
};
pub use survey_import::{SurveyImportOptions, SurveyImportReport};
pub use terrain::surface_style::{
    ColourStop, SurfaceContours, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyle, SurfaceStyleSet,
};
pub use tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};
pub use view_transform::ViewTransform;
pub use viewport_layout::{CellRect, DockSplit, LayoutKind, ViewKind};
