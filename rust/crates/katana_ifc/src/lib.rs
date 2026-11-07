//! The `ifc` layer: IFC 4.3 export and import.
//!
//! The port of src/katana_ifc and include/katana/ifc (docs/ifc.md is the area
//! document): the drawing, its alignments, its surfaces, its road model and an
//! AS 5488 subsurface utility investigation written as IFC4X3_ADD2, and
//! IFC2X3, IFC4 and IFC4X3 files read back into the drawing, with no
//! third-party library - IFC is ISO 10303-21 text, and what matters is the
//! mapping. One module per C++ header; the parts of the export that have no
//! header of their own (alignment, utilities, drawing, surfaces, road) are
//! private modules, as their .cpp files are.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, which reject a NaN."
)]

pub mod classification;
pub mod export;
pub mod front_end;
pub mod import;
pub mod step;

mod alignment;
mod builder;
mod drawing;
mod import_alignment;
mod libm;
mod parts;
mod predefined_types;
mod product_classes;
mod product_layout;
mod road;
mod step_file;
mod step_reader;
mod support;
mod surfaces;
mod utilities;

pub use classification::{
    ClassificationRule, EntityClass, IfcClass, UtilityClass, asset_type_code, classification_text,
    classify_entity, classify_utility_point, classify_utility_run, contains_word,
    default_classification_rules, format_classification_rules, parse_classification_rules,
    service_system_for,
};
pub use export::{
    ClassTally, ExportInput, ExportOptions, Georeference, IfcExport, RoadElement, RoadModelInput,
    RoadQuantity, SurfaceInput, UtilityInput, is_epsg_code, is_ifc_path, write_ifc, write_ifc_file,
};
pub use front_end::{
    ExportArguments, ExportFiles, ImportArguments, format_description, format_export_line,
    format_export_reply, format_import_line, format_import_reply, format_info_line,
    format_rules_line, guid_namespace_for, header_timestamp, note_record, parse_export_arguments,
    parse_import_arguments, parse_rules_arguments, path_from_utf8, read_export_files,
    read_export_objects, warning_record, write_default_rules,
};
pub use import::{
    ALIGNMENT_TOLERANCE, IfcImport, ImportOptions, ImportedSurface, import_command, read_ifc,
    read_ifc_file,
};
pub use step::{
    character_count, clip_characters, compress_guid, decode_step_string, guid_for, step_real,
    step_string,
};
