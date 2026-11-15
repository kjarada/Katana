//! The `archive12d` layer: the .12da archive format - its reader and writer,
//! and the mapping between an archive and Katana's domain model.
//!
//! The port of src/katana_archive12d and include/katana/archive12d
//! (docs/rust.md). The readers and writers of the retired customisation
//! formats (src/katana_archive12d/legacy) are not ported: whether they are is
//! the owner's decision.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

mod domain_common;
mod domain_export;
mod domain_import;
mod lexer;
mod plan_geometry;
mod text_utilities;

pub mod archive;
pub mod coverage;
pub mod domain;
pub mod reader;
pub mod writer;

pub use archive::{
    AlignmentPart, AlignmentSource, ArcString, Archive, Attribute, AttributeList, AttributeValue,
    Breakline, CircleString, DraftingLeader, DraftingReference, DrainageRecord, DrainageString,
    Element, Field, FieldList, LasCloud, LasPoint, ModelRecord, PlotFrame, Segment, SegmentKind,
    SolvedGeometry, StringHeader, StringKind, SuperAlignment, SuperTin, TextString, Tin, Trimesh,
    TrimeshInfo, Vertex, VertexString, element_keyword,
};
pub use coverage::{ElementCoverage, Handling, element_coverage};
pub use domain::{
    DomainExport, DomainImport, ElementTally, ExportMesh, ExportOptions, ExportSurface,
    ImportOptions, ImportedCloud, ImportedCloudPoint, ImportedMesh, ImportedSurface, chorded_plan,
    from_domain, layer_path_for_model, split_list, symbol_for_linestyle, to_domain,
};
pub use reader::{ReadOptions, parse_real, read_archive, read_archive_bytes};
pub use writer::{WriteOptions, quoted, write_archive};
