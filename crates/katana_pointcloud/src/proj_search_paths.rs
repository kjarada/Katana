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

use std::ffi::CString;
use std::path::Path;
use std::sync::Once;

use katana_core::library_data::{bundled_proj_data_directory, proj_data_directory};

use crate::ffi;

/// Whether GDAL's PROJ can open proj.db as things stand: EPSG:4326 is read
/// from the database (GDAL carries no definitions of its own), so the import
/// fails when the database does not open - PROJ_LIB naming a folder without
/// one, or with one of another PROJ's layout, which PROJ rejects. GDAL's
/// errors are silenced for the probe, and its last error reset after it.
pub fn gdal_finds_proj_database() -> bool {
    ffi::gdal_imports_epsg_quietly(4326)
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
    ffi::set_gdal_proj_search_paths(&[text.as_c_str()]);
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
