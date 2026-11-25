//! The `dwg` layer: DWG read and written (docs/dwg.md), through acadrust, the one crate that
//! adapts it - no type of acadrust's leaves this crate.
//!
//! It may see core, math, geometry, entity, commands and dxf (tools/check_layering.cmake).
//!
//! A DWG goes through the DXF module both ways, as CADCraft's does (docs/dwg.md, "The choice"):
//! acadrust turns the DWG into DXF text in memory and katana_dxf's reader reads that text, so a
//! DWG comes in as a `katana_dxf::DxfImport` - with the DXF's tally, its warnings and its one
//! undoable step, `katana_dxf::import_command`; and katana_dxf's writer writes the model as DXF,
//! which acadrust turns into a DWG of the release asked for. One mapping between Katana's
//! entities and a CAD drawing, not two that could come to differ.
//!
//! ```text
//!   read:  DWG bytes -> acadrust's DwgReader -> its document -> its DxfWriter -> DXF text
//!                    -> katana_dxf::read_dxf -> DxfImport
//!   write: Model -> katana_dxf::write_dxf -> DXF text -> acadrust's DxfReader -> its document
//!                -> its DwgWriter -> DWG bytes
//! ```

mod bridge;
pub mod reader;
pub mod release;
pub mod writer;

pub use reader::{is_dwg_path, read_dwg, read_dwg_file};
pub use release::DwgRelease;
pub use writer::{DwgExport, write_dwg, write_dwg_file};
