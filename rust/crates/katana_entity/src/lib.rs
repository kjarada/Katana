//! The `entity` layer: entities, layers, named tables, the geometry blob, the customisation format.
//!
//! The port of src/katana_entity and include/katana/entity (docs/rust.md).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

mod json;
mod scalar;
mod validation;
mod value_parts;

pub mod anchor;
pub mod annotation;
pub mod colour_names;
pub mod curve_pieces;
pub mod customisation;
pub mod dimension_text;
pub mod display;
pub mod entity;
pub mod entity_database;
pub mod entity_geometry;
pub mod geometry_blob;
pub mod label_text;
pub mod label_values;
pub mod layer_path;
pub mod leader_values;
pub mod linework_codes;
pub mod model;
pub mod named_table;
pub mod ring_tags;
pub mod serialization;
pub mod style_library;
pub mod survey_map;
pub mod table_usage;
pub mod tables;
pub mod text_block;

pub use annotation::{
    LabelKind, LabelMarker, LabelOrientation, LabelPlacement, LabelRule, LabelRuleDatabase,
    LabelStyle, LabelStyleDatabase, TextStyle, TextStyleDatabase,
};
pub use colour_names::{ColourTable, ColourTableEntry};
pub use customisation::{
    Customisation, CustomisationAutomation, CustomisationBase, CustomisationSourceNote,
    CustomisationWriteOptions,
};
pub use entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, Color, DimensionGeometry, DimensionKind,
    Entity, EntityId, EntityType, EnumName, Geometry, LabelGeometry, LeaderGeometry, PointGeometry,
    PropertyMap, PropertyValue, TextGeometry, TextJustify,
};
pub use entity_database::{ChangeEvent, ChangeKind, EntityDatabase};
pub use entity_geometry::{Validate, validate};
pub use label_text::{LabelQuantity, LabelValue, LabelValues};
pub use label_values::{LabelPiece, LabelPieceShape};
pub use leader_values::{AlignmentPlace, ContextValueName, LeaderContext};
pub use linework_codes::{LineworkCodeMember, LineworkCodes};
pub use model::Model;
pub use named_table::{Named, NamedMap, NamedTable, NamedTablePolicy};
pub use ring_tags::{ExteriorCandidate, HoleJoiner};
pub use style_library::{
    LineStyle, LineStylePolicy, Stroke, StrokeOp, StrokeText, StyleLibrary, StyleUnits,
};
pub use survey_map::{
    SurveyAttribute, SurveyBreakline, SurveyMap, SurveyMatch, SurveyMatchKind, SurveyPipe,
    SurveyRule, SurveySection, SurveySectionWord, SurveySymbol, SurveyTextStyle,
};
pub use table_usage::{TableUsage, UsageMap, UsageOptions, Users};
pub use tables::{
    Alignment, AlignmentDatabase, DimensionStyle, DimensionStyleDatabase, HatchLineFamily,
    HatchPattern, HatchPatternDatabase, Layer, LayerDatabase, Linetype, LinetypeDatabase,
    LinetypeElement, PropertyDatabase, PropertyDefinition, PropertyType, ResolvedLayer, Style,
    StyleDatabase,
};
pub use text_block::TextBlockExtent;
