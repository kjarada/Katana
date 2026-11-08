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
    pub mod dimension_build;
    pub mod dimension_style_verbs;
    mod document_annotation;
    pub mod drawing;
    pub mod text_layout;
}
pub mod bim {
    pub mod bim_object;
}
pub mod colour_lookup;
pub mod corridor;
pub mod customisation_merge;
pub mod customisation_part;
pub mod customisation_record;
pub mod customisation_state;
pub mod dashing;
pub mod definition_edit;
pub mod definition_users;
pub mod dimension_draw;
pub mod document;
pub mod document_status;
pub mod drawing {
    pub mod construction;
    pub mod drafting;
    pub mod draw_shapes;
    pub mod vertex_table;
}
pub mod geo {
    pub mod drape;
    pub mod surface_input;
}
pub mod global_modify;
pub mod grading;
pub mod hatch_pattern_rows;
pub mod hatching;
pub mod import_placement;
pub mod layer_overrides;
pub mod linework;
pub mod plot;
pub mod project_crs;
pub mod plotting {
    pub mod frame;
    pub mod generators;
    pub mod layout;
    pub mod page_setup;
    pub mod plan_grid;
    pub mod section_annotation;
    pub mod section_fit;
    pub mod sheet_commands;
    pub mod sheet_json;
    pub mod sheet_set;
    mod sheet_store;
    pub mod tables;
    pub mod viewport_edits;

    pub use frame::{
        Frame, FrameCell, FrameMargins, FramePolyline, FrameRole, FrameSymbol, FrameText,
        HorizontalJustify, VerticalJustify,
    };
    pub use generators::{
        CrossSectionRequest, DrawnTest, GridRequest, LayoutRequest, SheetTemplate, StripRequest,
    };
    pub use layout::{SizeMm, SnapResult, TilingPreset};
    pub use page_setup::PageSetup;
    pub use plan_grid::{
        GridAxis, GridLabel, GridLine, GridSegment, LabelWidth, PlanGrid, PlanGridOptions,
        PlanPlacement, ViewportEdge,
    };
    pub use section_annotation::{CrossingNote, Earthwork, EarthworkRegion, LabelCandidate};
    pub use section_fit::{SectionFit, SectionFitRequest, SectionLayout, SectionLayoutRequest};
    pub use sheet_set::{
        FieldContext, GridStyle, LegendScope, PlanWindow, Revision, Sheet, SheetDefaults, SheetSet,
        SignOff, Viewport, ViewportKind, ViewportSource, WorldMark, WorldMarkKind,
    };
    pub use tables::{
        RegisterRow, TableColumn, TableLayout, TableRule, TableSpec, TableStyle, TableText,
        TextWidth,
    };
    pub use viewport_edits::{BandMode, EdgeSnap};
}
pub mod purge;
pub mod road {
    pub mod corridor_model;
    mod road_document;
    mod road_object_json;
    pub mod road_object_mesh;
    mod road_object_store;
    pub mod road_objects;
    pub mod road_section;
    pub mod road_template;
}
pub mod scene;
pub mod scene_zoom;
pub mod scope_verbs;
pub mod section;
pub mod selection;
pub mod selection_style;
mod simd {
    #[cfg(target_arch = "x86_64")]
    mod scene_avx2;
    pub(crate) mod scene_kernels;
    #[cfg(target_arch = "aarch64")]
    mod scene_neon;
}
pub mod snapping;
pub mod spatial_query;
pub mod style_catalogue;
pub mod style_drawing;
pub mod style_resolver;
pub mod survey_coding;
pub mod survey_import;
pub mod survey_tools;
pub mod symbol_assign;
pub mod symbols;
pub mod terrain {
    pub mod section_cut;
    pub mod surface_colouring;
    pub(crate) mod surface_document;
    #[doc(hidden)]
    pub mod surface_document_io;
    pub mod surface_plan;
    pub mod surface_style;
}
pub mod tool_feedback;
pub mod view_link;
pub mod view_set;
pub mod view_transform;
pub mod viewport_layout;

pub use corridor::{
    Assembly, CorridorQuantities, CorridorSection, CorridorSurface, SectionVertex, SplitArea,
};
pub use customisation_merge::{CustomisationLoad, CustomisationMerge, LoadMode};
pub use customisation_part::DefinitionsOfFile;
pub use customisation_record::{
    CustomisationSource, DistinctFiles, RenamedDefinition, RenamedSource,
};
pub use customisation_state::{CustomisationOrigin, CustomisationState};
pub use dashing::DashOptions;
pub use definition_edit::StrokeTextRead;
pub use definition_users::{DefinitionRule, DefinitionUsers};
pub use dimension_draw::DimensionDrawing;
pub use document::{Document, DocumentChange, DocumentParts, ListenerHandle};
pub use document_status::DocumentStatus;
pub use drawing::drafting::{
    AngleConvention, ConstrainedPoint, DraftingSettings, PrecisePoint, TrackingPoints,
};
pub use drawing::vertex_table::{VertexColumn, VertexRow};
pub use global_modify::{
    EntityModify, GlobalModify, GlobalModifyPlan, LayerModify, ModifyFilter, ModifyScope,
    ScopeKind, StyleModify, SymbolModify,
};
pub use grading::{DaylightPoint, FeatureLine, Grading, GradingSide, GradingSlopes};
pub use hatch_pattern_rows::HatchPatternRow;
pub use hatching::{HatchDrawing, HatchOptions};
pub use import_placement::{ImportPlacement, ImportPlacementMode, ImportShift};
pub use layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
pub use linework::{
    FieldCode, LineworkControl, LineworkNote, LineworkNoteKind, LineworkOptions, LineworkOrder,
    LineworkReport, LineworkResult, LineworkString, StringName, SurveyFeatureOptions,
    SurveyFeatureResult, UnplacedFeature, UnplacedFeatureReason, UnplacedPoint, UnplacedReason,
};
pub use plot::{PaperDimensions, PaperSize, PlotColourMode, PlotSettings, Sheet};
pub use project_crs::{CrsChoice, CrsDescription};
pub use purge::PurgeOptions;
pub use scene::{
    EdgeRun, LineworkHeights, SceneBuilder, SceneLayers, SceneMesh, SceneOptions, SceneSurface,
    SurfaceColoring,
};
pub use scene_zoom::{PickOptions, ScenePick, ScenePickSource, ZoomResult};
pub use scope_verbs::{
    ResolvedScope, ScopeMatch, ScopeSource, ScopeView, ScopeViewFn, ScopeViewProvider, ScopeWords,
};
pub use section::{
    SampleReason, Section, SectionCrossing, SectionDesign, SectionOptions, SectionSample,
    SectionSurface, SectionSurfaceInput,
};
pub use selection::{BoxSelectionMode, SelectionFilter, SelectionSet};
pub use snapping::{SnapAccept, SnapMode, SnapModes, SnapRequest, SnapResult};
pub use style_catalogue::{
    CatalogueEntry, DefinitionKind, DefinitionSource, MissingName, NameRole, NameStatus,
};
pub use style_drawing::{
    FlatDefinition, FlatDefinitionRun, FlatDefinitionText, LinestyleLayout, LinestyleLayoutOutcome,
    LinestyleOptions, StyleDrawing, StyleStroke, StyleTextMark,
};
pub use style_resolver::{
    DefinitionCache, LinetypeKind, ResolvedLinetype, ResolvedSymbol, StyleSamplePath, SymbolKind,
};
pub use survey_coding::{
    CodingSource, ColourOf, CustomisationCoverage, SurveyCodeRow, SurveyCodingOptions,
    SurveyCodingReport, SurveyStyleOutcome,
};
pub use survey_import::{SurveyImportOptions, SurveyImportReport};
pub use survey_tools::{
    AngleConversion, AngleInputUnit, AreaItem, AreaResult, AreaSkip, CoordinateConversion,
    CoordinateConversionRow, ForwardInput, ForwardResult, InverseResult, LevelBookLine,
    LevelBookResult, LevelBookRow, LevelBookSpec, SurveyPosition, TraverseMethod,
    TraverseOrientation, TraverseSpec, TraverseStationRow, TraverseToolResult,
};
pub use symbol_assign::{
    SymbolAssignment, SymbolCode, SymbolLibraryEntry, SymbolPrintSize, SymbolUsers,
};
pub use terrain::section_cut::DrawingSection;
pub use terrain::surface_colouring::{
    BandPiece, SurfaceColouring, SurfaceLegend, SurfaceLegendCard, SurfaceLegendEntry,
    SurfaceRampStops,
};
pub use terrain::surface_plan::{
    SurfacePlan, SurfacePlanCache, SurfacePlanContour, SurfacePlanImage, SurfacePlanImageGrid,
    SurfacePlanLabel, SurfacePlanStats, SurfacePlanView, SurfaceProblem, SurfaceStyleProblem,
};
pub use terrain::surface_style::{
    ColourStop, SurfaceContours, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyle, SurfaceStyleSet,
};
pub use tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};
pub use view_set::{LinkChange, ViewId, ViewPaperScales, ViewSet, ViewState};
pub use view_transform::ViewTransform;
pub use viewport_layout::{CellRect, DockSplit, LayoutKind, ViewKind};
