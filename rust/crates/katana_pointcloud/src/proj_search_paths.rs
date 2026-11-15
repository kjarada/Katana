//! GDAL's PROJ search path, set once before GDAL or PDAL first looks anything
//! up (katana_core::library_data has why). The port of
//! src/katana_io/proj_search_paths.hpp: the GDAL registry
//! (katana_gis::gdal_registry) calls it before registering the drivers, and
//! the point-cloud engine before every PDAL entry point, since PDAL reads and
//! writes coordinate systems through GDAL's OSR. Its one home is here, in
//! pointcloud: gis may see pointcloud, pointcloud may not see gis (the
//! step-5 plan, section 8, choice 6), so the two callers share one once-flag
//! as the C++'s do.
//!
//! Hidden from the documentation: an internal header of katana_io, public
//! only so that katana_gis can call it.

#![allow(
    unsafe_code,
    reason = "Calls GDAL's C API through the declarations of ffi; every unsafe block states why it is sound."
)]

use std::ffi::CString;
use std::path::Path;
use std::ptr::null;
use std::sync::Once;

use katana_core::library_data::{bundled_proj_data_directory, proj_data_directory};

use crate::ffi::{
    CPLErrorReset, CPLPopErrorHandler, CPLPushErrorHandler, CPLQuietErrorHandler, OGRERR_NONE,
    OSRDestroySpatialReference, OSRImportFromEPSG, OSRNewSpatialReference, OSRSetPROJSearchPaths,
};

/// Whether GDAL's PROJ can open proj.db as things stand: EPSG:4326 is read
/// from the database (GDAL carries no definitions of its own), so the import
/// fails when the database does not open - PROJ_LIB naming a folder without
/// one, or with one of another PROJ's layout, which PROJ rejects.
pub fn gdal_finds_proj_database() -> bool {
    // SAFETY: CPLQuietErrorHandler is GDAL's own handler, which lives as long
    // as the library; the handler stack is this thread's, popped below.
    unsafe { CPLPushErrorHandler(Some(CPLQuietErrorHandler)) };
    // SAFETY: a NULL definition makes an empty reference, which is GDAL's to
    // allocate and is destroyed below.
    let reference = unsafe { OSRNewSpatialReference(null()) };
    // SAFETY: the reference just made (GDAL validates a NULL one itself).
    let found = unsafe { OSRImportFromEPSG(reference, 4326) } == OGRERR_NONE;
    // SAFETY: the reference made above, destroyed once.
    unsafe { OSRDestroySpatialReference(reference) };
    // SAFETY: pops the handler pushed above, on the same thread.
    unsafe { CPLPopErrorHandler() };
    // What the probe said is not a later call's last error.
    // SAFETY: resets this thread's last error; nothing is passed.
    unsafe { CPLErrorReset() };
    found
}

/// The bytes of `path.generic_u8string()`, the text PROJ takes: UTF-8 (the
/// path's own bytes on POSIX; on Windows its characters as UTF-8 with '/'
/// separators, where path::string() would be the ANSI code page, which turns a
/// character outside it into '?').
pub fn proj_path_text(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().replace('\\', "/").into_bytes()
    }
}

// The search path set to the one folder `path`. GDAL copies the list.
fn set_search_path(path: &Path) {
    let text = proj_path_text(path);
    let end = text
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(text.len());
    let text = CString::new(&text[..end]).unwrap_or_default();
    let paths = [text.as_ptr(), null()];
    // SAFETY: a NULL-terminated list of one live NUL-terminated string, alive
    // for the call; GDAL keeps a copy.
    unsafe { OSRSetPROJSearchPaths(paths.as_ptr()) };
}

/// Points GDAL's PROJ at the data Katana carries, once per process.
pub fn point_gdal_at_proj_data() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if let Some(data) = proj_data_directory() {
            set_search_path(data);
            return;
        }
        // PROJ_DATA or PROJ_LIB in the environment replaces the search beside
        // the library, and another GIS package leaves one machine-wide naming
        // data this PROJ cannot open (katana_core::library_data). An explicit
        // choice that works is kept; one that does not falls back on the data
        // Katana carries, as geodesy's contexts do. Asked only when the
        // environment says something, so an ordinary start pays nothing for it.
        if std::env::var_os("PROJ_DATA").is_none() && std::env::var_os("PROJ_LIB").is_none() {
            return;
        }
        if let Some(bundled) = bundled_proj_data_directory()
            && !gdal_finds_proj_database()
        {
            set_search_path(bundled);
        }
    });
}
