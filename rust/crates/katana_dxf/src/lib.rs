//! The `dxf` layer: DXF read and written natively.
//!
//! The port of src/katana_dxf and include/katana/dxf (docs/dxf.md,
//! docs/rust.md). DXF read and write, natively: entities, layers and
//! linetypes, with no third-party library. It may see core, math, geometry,
//! entity and commands; commands only for import_command, the import as the
//! one undoable step both front ends execute.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

pub mod codes;
pub mod import_command;
pub mod reader;
mod scalar;
mod tokenizer;
pub mod writer;

pub use codes::{
    COLOUR_BY_BLOCK, COLOUR_BY_LAYER, COLOUR_FOREGROUND, RADIANS_PER_DEGREE, encode_text,
    exact_degrees, indexed_colour, nearest_indexed_colour, nearest_lineweight, plain_mtext,
    plain_text, true_colour,
};
pub use import_command::import_command;
pub use reader::{
    APPLICATION_NAME, DxfImport, EXTENDED_COLOUR, EXTENDED_HEIGHTS, EXTENDED_PATH, EntityTally,
    ImportOptions, META_BLOCK, META_LINETYPE, META_LINEWEIGHT, META_SOURCE, is_dxf_path, read_dxf,
    read_dxf_file, release_name,
};
pub use writer::{DxfExport, ExportOptions, layer_name_for, write_dxf, write_dxf_file};
