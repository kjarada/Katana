//! The `dgn` layer: MicroStation V7 design files (DGN), read and written
//! natively (docs/dgn.md).
//!
//! A V7 design file - the Intergraph Standard File Format - is read into
//! drawing entities and their layers with a tally and warnings, and a drawing
//! is written as one on a seed design. Beside dxf in the layering and for its
//! reason: no third-party library, so the parser of untrusted bytes is plain
//! safe Rust. It may see core, math, geometry and entity. The import as ONE
//! undoable step is the DXF import's own (katana_dxf's import_parts), which
//! the front ends call with what this module read, so that the two imports
//! cannot differ about it.
//!
//! A V8 design (an OLE compound document) is refused by name: no open
//! library that reads it is in the build (docs/dgn.md, "V8").

pub mod codes;
pub mod plane;
pub mod reader;
pub mod writer;

pub use reader::{
    Content, DgnImport, DgnTally, ImportOptions, META_CELL, META_COLOUR, META_FONT,
    META_JUSTIFICATION, META_SOURCE, META_STYLE, META_WEIGHT, WorkingUnits, identify,
    is_dgn_path, level_layer_name, level_of_layer_name, read_dgn, read_dgn_file,
};
pub use writer::{
    DgnExport, ExportOptions, LevelUse, SeedInfo, has_heights, seed_info, write_dgn,
    write_dgn_file,
};

/// What a file dialog calls the format.
pub const FILE_TYPE_NAME: &str = "MicroStation DGN (V7)";
