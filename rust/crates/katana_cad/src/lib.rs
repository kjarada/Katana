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

pub mod alignment_report;
pub mod annotation {
    mod annotation_verbs;
    pub mod associative;
    pub mod auto_label;
    pub mod command_words;
    pub mod dimension_build;
    pub mod dimension_style_verbs;
    mod document_annotation;
    pub mod drawing;
    pub mod export_annotation;
    pub mod label_layout;
    pub mod leader_draw;
    pub mod leader_edit;
    pub mod leader_presets;
    pub mod text_layout;
}
pub mod bim {
    pub mod bim_object;
    pub mod bim_view;
}
pub mod code_edit;
pub mod code_table;
pub mod colour_lookup;
pub mod command_interpreter;
pub mod corridor;
pub mod csv_table;
pub mod customisation_host;
pub mod customisation_merge;
pub mod customisation_part;
pub mod customisation_record;
pub mod customisation_report;
pub mod customisation_state;
pub mod customisation_verbs;
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
    mod drawing_verbs;
    pub mod grips;
    pub mod vertex_editing;
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
pub mod interactive_tool;
pub mod layer_overrides;
pub mod linework;
pub mod linework_verbs;
pub mod parcel;
pub mod plot;
pub mod project_crs;
pub mod property_outline;
pub mod plotting {
    pub mod arrange;
    pub mod arrange_commands;
    pub mod frame;
    pub mod generators;
    pub mod key_plan;
    pub mod layout;
    pub mod legend;
    pub mod page_setup;
    pub mod plan_grid;
    pub mod preflight;
    pub mod section_annotation;
    pub mod section_fit;
    pub mod sheet_commands;
    pub mod sheet_json;
    pub mod sheet_set;
    mod sheet_store;
    pub mod sheet_verbs;
    pub mod tables;
    pub mod viewport_edits;

    pub use arrange::{
        AlignEdge, ArrangeResult, DistributeAxis, PaperAdvice, PaperChange, PaperRequest,
        RotationFit,
    };
    pub use frame::{
        Frame, FrameCell, FrameMargins, FramePolyline, FrameRole, FrameSymbol, FrameText,
        HorizontalJustify, VerticalJustify,
    };
    pub use generators::{
        CrossSectionRequest, DrawnTest, GridRequest, LayoutRequest, SheetTemplate, StripRequest,
    };
    pub use key_plan::{KeyPlanOutline, PlanPlacer};
    pub use layout::{SizeMm, SnapResult, TilingPreset};
    pub use legend::{
        Legend, LegendCell, LegendEntry, LegendKind, LegendLayout, LegendMetrics, LegendOptions,
        PlanResolver,
    };
    pub use page_setup::PageSetup;
    pub use plan_grid::{
        GridAxis, GridLabel, GridLine, GridSegment, LabelWidth, PlanGrid, PlanGridOptions,
        PlanPlacement, ViewportEdge,
    };
    pub use preflight::{CheckDescription, Finding, PreflightOptions, PreflightSummary, Severity};
    pub use section_annotation::{CrossingNote, Earthwork, EarthworkRegion, LabelCandidate};
    pub use section_fit::{SectionFit, SectionFitRequest, SectionLayout, SectionLayoutRequest};
    pub use sheet_set::{
        FieldContext, GridStyle, LegendScope, PlanWindow, Revision, Sheet, SheetDefaults, SheetSet,
        SignOff, Viewport, ViewportKind, ViewportSource, WorldMark, WorldMarkKind,
    };
    pub use sheet_verbs::{
        PlotSheetsRequest, SheetVerbContext, SheetVerbContextFn, SheetVerbContextProvider,
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
    pub mod road_object_scope;
    mod road_object_store;
    pub mod road_object_verbs;
    pub mod road_objects;
    pub mod road_section;
    pub mod road_template;
    pub mod road_verbs;
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
pub mod style_manager_rows;
pub mod style_resolver;
pub mod survey_code_verbs;
pub mod survey_coding;
pub mod survey_finish;
pub mod survey_import;
pub mod survey_job;
pub mod survey_points;
pub mod survey_tools;
pub mod symbol_assign;
pub mod symbols;
pub mod terrain {
    pub mod drawing_shapes;
    pub mod section_cut;
    pub mod surface_colouring;
    pub mod surface_display_verbs;
    pub(crate) mod surface_document;
    #[doc(hidden)]
    pub mod surface_document_io;
    pub mod surface_edit_verbs;
    pub mod surface_plan;
    pub mod surface_style;
    pub mod surface_verbs;
    pub mod volume_verbs;
}
pub mod tool_feedback;
pub mod tools {
    pub mod annotate;
    mod annotate_balloon;
    #[doc(hidden)]
    pub mod annotate_common;
    mod annotate_dimension;
    mod annotate_dimension_chain;
    mod annotate_dimension_kinds;
    mod annotate_label;
    mod annotate_leader;
    mod annotate_leader_presets;
    mod annotate_text;
    #[doc(hidden)]
    pub mod draw_curves;
    mod draw_divide;
    #[doc(hidden)]
    pub mod draw_lines;
    #[doc(hidden)]
    pub mod draw_professional;
    #[doc(hidden)]
    pub mod everyday_support;
    #[doc(hidden)]
    pub mod families;
    mod inquiry;
    mod modify_edit;
    mod modify_edit_break;
    mod modify_edit_corner;
    mod modify_edit_join;
    mod modify_edit_offset;
    #[doc(hidden)]
    pub mod modify_edit_support;
    mod modify_edit_trim;
    mod modify_length;
    mod modify_properties;
    #[doc(hidden)]
    pub mod modify_transform;
    pub mod modify_vertex;
    mod select_tools;
    mod terrain_edit;
}
pub mod utilities {
    pub mod utility_data;
    pub mod utility_drawing;
    pub mod utility_verbs;

    pub use utility_data::{
        DrawnService, GeometryHeights, GeometryServiceOptions, GeometryServices, UtilityData,
        UtilityRegrade, UtilityRegradeOptions,
    };
    pub use utility_drawing::{DrawnUtilityLine, UtilityDrawOptions, UtilityDrawing, keys};
}
pub mod view_link;
pub mod view_set;
pub mod view_transform;
pub mod view_verbs;
pub mod viewport_layout;

pub use alignment_report::SettingOutStation;
pub use code_table::{
    BuiltInSymbolTest, CodeAttribute, CodeCensus, CodeCensusRow, CodeColour, CodeDefinition,
    CodeExplanation, CodeFieldSource, CodeFieldSourceOverruled, CodeNearMiss, CodeTableRow,
    ColourLookup, DefinitionLookup, LintIssue, LintKind, LintSeverity, NearMissKind,
};
pub use command_interpreter::{CommandInterpreter, ImportArgument};
pub use corridor::{
    Assembly, CorridorQuantities, CorridorSection, CorridorSurface, SectionVertex, SplitArea,
};
pub use csv_table::CsvTable;
pub use customisation_host::{
    BuiltInChoice, BuiltInCustomisation, BuiltInEntry, BuiltInFile, CustomisationFile,
    CustomisationHost, CustomisationStart,
};
pub use customisation_merge::{CustomisationLoad, CustomisationMerge, LoadMode};
pub use customisation_part::DefinitionsOfFile;
pub use customisation_record::{
    CustomisationSource, DistinctFiles, RenamedDefinition, RenamedSource,
};
pub use customisation_report::CustomisationSummary;
pub use customisation_state::{CustomisationOrigin, CustomisationState};
pub use customisation_verbs::CustomisationVerbContext;
pub use dashing::DashOptions;
pub use definition_edit::StrokeTextRead;
pub use definition_users::{DefinitionRule, DefinitionUsers};
pub use dimension_draw::DimensionDrawing;
pub use document::{Document, DocumentChange, DocumentParts, ListenerHandle};
pub use document_status::DocumentStatus;
pub use drawing::drafting::{
    AngleConvention, ConstrainedPoint, DraftingSettings, PrecisePoint, TrackingPoints,
};
pub use drawing::grips::{Grip, GripDrag, GripKind};
pub use drawing::vertex_table::{VertexColumn, VertexRow};
pub use global_modify::{
    EntityModify, GlobalModify, GlobalModifyPlan, LayerModify, ModifyFilter, ModifyScope,
    ScopeKind, StyleModify, SymbolModify,
};
pub use grading::{DaylightPoint, FeatureLine, Grading, GradingSide, GradingSlopes};
pub use hatch_pattern_rows::HatchPatternRow;
pub use hatching::{HatchDrawing, HatchOptions};
pub use import_placement::{ImportPlacement, ImportPlacementMode, ImportShift};
pub use interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolInfo, ToolInput, ToolStart, ToolStep,
    ToolStepOutcome,
};
pub use layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
pub use linework::{
    FieldCode, LineworkControl, LineworkNote, LineworkNoteKind, LineworkOptions, LineworkOrder,
    LineworkReport, LineworkResult, LineworkString, StringName, SurveyFeatureOptions,
    SurveyFeatureResult, UnplacedFeature, UnplacedFeatureReason, UnplacedPoint, UnplacedReason,
};
pub use linework_verbs::{LineworkPlan, LineworkWords, SurveyImportFinish};
pub use parcel::{ParcelCourse, ParcelReport};
pub use plot::{PaperDimensions, PaperSize, PlotColourMode, PlotSettings, Sheet};
pub use project_crs::{CrsChoice, CrsDescription};
pub use property_outline::PropertyOutlineEntry;
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
pub use style_manager_rows::{
    LinetypeOrigin, LinetypeRow, LinetypeRowKind, StyleDiagnostic, StyleDiagnosticKind,
    StyleFields, StyleFilter, StyleRow, UsageTable,
};
pub use style_resolver::{
    DefinitionCache, LinetypeKind, ResolvedLinetype, ResolvedSymbol, StyleSamplePath, SymbolKind,
};
pub use survey_coding::{
    CodingSource, ColourOf, CustomisationCoverage, SurveyCodeRow, SurveyCodingOptions,
    SurveyCodingReport, SurveyStyleOutcome,
};
pub use survey_finish::{
    SharedFinishReport, SurveyFinishEarlier, SurveyFinishOptions, SurveyFinishReport,
    SurveyFinishSkip,
};
pub use survey_import::{SurveyImportOptions, SurveyImportReport};
pub use survey_job::{
    HandEditPolicy, ImportSurveyJobCommand, ImportSurveyJobResults, ReadjustSurveyJobCommand,
    ReadjustSurveyJobResults, ReductionFunction, RemoveSurveyJobCommand, SurveyJob,
    SurveyJobAccess, SurveyJobChanges, SurveyJobContext, SurveyJobFile, SurveyJobImport,
    SurveyJobOptions, SurveyJobPoint, SurveyJobReader, SurveyJobReadjustment,
};
pub use survey_points::{
    DrawingSurveyPoint, ExistingPointPolicy, SurveyImportSummary, SurveyPointImportReport,
    SurveyPointPick,
};
pub use survey_tools::{
    AngleConversion, AngleInputUnit, AreaItem, AreaResult, AreaSkip, CoordinateConversion,
    CoordinateConversionRow, ForwardInput, ForwardResult, InverseResult, LevelBookLine,
    LevelBookResult, LevelBookRow, LevelBookSpec, SurveyPosition, TraverseMethod,
    TraverseOrientation, TraverseSpec, TraverseStationRow, TraverseToolResult,
};
pub use symbol_assign::{
    SymbolAssignment, SymbolCode, SymbolLibraryEntry, SymbolPrintSize, SymbolUsers,
};
pub use terrain::drawing_shapes::{
    DrawingShape, DrawingShapes, NamedEntities, NotAShape, ShapeKind, ShapeOf, ShapeOptions,
    ShapeRefusal, ShapeWords, SkippedShapes, TypedCorner,
};
pub use terrain::section_cut::DrawingSection;
pub use terrain::surface_colouring::{
    BandPiece, SurfaceColouring, SurfaceLegend, SurfaceLegendCard, SurfaceLegendEntry,
    SurfaceRampStops,
};
pub use terrain::surface_display_verbs::SurfaceLegendReply;
pub use terrain::surface_edit_verbs::SurfaceEditPlan;
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
pub use view_verbs::{ViewHostProvider, ViewVerbHost, ZoomRequest, ZoomRequestKind};
pub use viewport_layout::{CellRect, DockSplit, LayoutKind, ViewKind};
