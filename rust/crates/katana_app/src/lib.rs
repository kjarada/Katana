//! The `app` layer: the headless session that katana_cli and katana_mcp share.
//!
//! The port of src/katana_app (docs/rust.md, "Step 6"). One module tree,
//! declared here: a folder of the C++ is an inline block whose files are in
//! `src/<folder>/`, and a module follows the C++ file. Step 6's agents each
//! add only their own lines, in the blocks commented below; the integrator
//! union-merges this file.

pub mod console; // wave 0
pub mod import_records; // wave 0

// pub mod session;            // S1 (its update part: session/update_part.rs)
// pub mod update_verbs;       // S1
// pub mod update_transport;   // S1
// pub mod survey_verbs;       // F1
// pub mod ifc_verbs;          // F1
// pub mod dxf_verbs;          // F1
// pub mod mcp { pub mod tools; pub mod server; mod customisation; }   // C (wave 2)
// pub mod geo {
//     // G1: geo_verbs, bindings, verb_table, replies, schema, staged_files,
//     //     vector_support, terrain_verbs, analysis_support, dem_support,
//     //     gis_records, formats_verbs, gdal_verbs, copc_verb, seams
//     // A:  surface_verbs, references, refs_verb, info_verb, contour_verbs
//     // B:  import_verb, export_verb, drape_verbs, zonal_verbs
//     // V:  buffer_verbs, check_verbs, hull_clip_verbs, overlay_verbs, sql_verbs
//     // R:  shade_verbs, slope_verbs, viewshed_verbs, grid_verbs, dem_verbs
//     // C:  mcp_geo_tools, sql_mcp
// }

pub use console::{Captured, Console, Terminal};
