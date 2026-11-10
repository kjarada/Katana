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
// pub mod mcp { pub mod tools; pub mod server; mod customisation; }   // C (wave 2)

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
    mod seams; // A puts the real calls back and deletes it (step 6, wave 2)
    pub mod staged_files;
    pub mod terrain_verbs;
    pub mod vector_support;
    pub mod verb_table;
    // A:  surface_verbs, references, refs_verb, info_verb, contour_verbs
    // B:  import_verb, export_verb, drape_verbs, zonal_verbs
    // V:  buffer_verbs, check_verbs, hull_clip_verbs, overlay_verbs, sql_verbs
    // R:  shade_verbs, slope_verbs, viewshed_verbs, grid_verbs, dem_verbs
    // C:  mcp_geo_tools, sql_mcp

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
