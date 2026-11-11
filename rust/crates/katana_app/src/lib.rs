//! The `app` layer: the headless session that katana_cli and katana_mcp share.
//!
//! The port of src/katana_app (docs/rust.md, "Step 6"). One module tree,
//! declared here: a folder of the C++ is an inline block whose files are in
//! `src/<folder>/`, and a module follows the C++ file. Step 6's agents each
//! add only their own lines, in the blocks commented below; the integrator
//! union-merges this file.

pub mod console; // wave 0
pub mod import_records; // wave 0

pub mod dxf_verbs; // F1
pub mod ifc_verbs; // F1
pub mod session; // S1 (its update part: session/update_part.rs)
pub mod survey_verbs; // F1
pub mod update_transport; // S1
pub mod update_verbs; // S1
/// The MCP server and its tools (src/katana_app/mcp_*): katana_mcp's engine.
/// The server's and the tools' names are re-exported here, as the C++'s
/// namespace `katana::app::mcp` holds them.
pub mod mcp {
    // C
    mod customisation;
    pub mod server;
    pub mod tools;

    // C
    pub use server::{LATEST_PROTOCOL_VERSION, Server};
    pub use tools::{
        LineOutcome, RunOptions, Tool, ToolCall, ToolRefusal, ToolReply, check_update_reply,
    };
}

/// The geoprocessing executor and its verbs (src/katana_app/geo): the
/// executor's frame and shared support (G1), and step 6's wave-2 families,
/// each in the lines named for it below. The executor's names are re-exported
/// here, as the C++'s namespace `katana::app::geo` holds them; not at the
/// crate root, where `Context`, `Prepared` and `Record` would say nothing of
/// whose they are.
pub mod geo {
    // G1: the executor's frame and shared support.
    pub mod analysis_support;
    pub mod bindings;
    mod copc_verb;
    pub mod dem_support;
    pub mod formats_verbs;
    mod gdal_verbs;
    pub mod geo_verbs;
    pub mod gis_records;
    pub mod replies;
    pub mod schema;
    pub mod staged_files;
    pub mod terrain_verbs;
    pub mod vector_support;
    pub mod verb_table;
    // A:  surface_verbs, references, refs_verb, info_verb, contour_verbs
    mod contour_verbs;
    mod info_verb;
    pub mod references;
    mod refs_verb;
    mod surface_verbs;
    // B:  import_verb, export_verb, drape_verbs, zonal_verbs
    mod drape_verbs;
    mod export_verb;
    mod import_verb;
    mod zonal_verbs;
    // V:  the GIS vector verbs.
    mod buffer_verbs;
    mod check_verbs;
    mod hull_clip_verbs;
    mod overlay_verbs;
    mod sql_verbs;
    // R:  shade_verbs, slope_verbs, viewshed_verbs, grid_verbs, dem_verbs
    mod dem_verbs;
    mod grid_verbs;
    mod shade_verbs;
    mod slope_verbs;
    mod viewshed_verbs;
    // C:  the MCP server's geo tools.
    pub mod mcp_geo_tools;
    mod sql_mcp;

    // G1
    pub use geo_verbs::{
        Apply, ChangedHook, Context, FarApartChoice, FarApartHook, FrameHook, FrontEnd,
        ImportShown, ImportedHook, Prepared, Progress, Work, handles, help_text, make_apply,
        make_work, own_scratch, prepare, run_now, stop_requested,
    };
}

pub use console::{Captured, Console, Terminal};
// S1
pub use session::{Session, UpdateContextSource, recover_interrupted_update};
pub use update_verbs::{
    UpdateContext, UpdateOffer, UpdateReply, UpdateStatus, UpdateSurface, abandon_apply,
    make_update_context, run_update_line,
};
