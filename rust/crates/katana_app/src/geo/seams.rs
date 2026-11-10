//! Where the geo frame calls what step 5's wave M ports - interop's reference
//! data, the raster products and the terrain readers - each call a marked
//! STAND-IN until step 6's wave 2 (agent A) puts the C++'s real code back at
//! every call site and deletes this file (docs/rust.md, step 6; the plan's
//! section 5, "G1", and section 8, choice 5).
//!
//! The signatures name only what wave 1 has built. Where the C++'s answer for
//! an empty set of reference layers is defined, the stand-in gives it;
//! otherwise it is Unsupported, "<the C++ function> waits for interop's
//! reference data (step 6, wave 2)". The one exception is
//! [`derived_raster_path`], which reads no reference data at all and lies on
//! every GDAL run's path (its spill folder): it is the C++'s own rule, kept
//! here until A calls the real one.
//!
//! The call sites, for A:
//!
//! ```text
//! reference_raster_file  bindings.rs     bind_raster (a RASTER source)
//! reference_name_taken   bindings.rs     keep_derived (the free name)
//!                        dem_support.rs  keep_in_place (the free name)
//! source_raster_name     terrain_verbs.rs source_name (its RASTER arm)
//! keep_derived_raster    bindings.rs     keep_derived
//!                        dem_support.rs  keep_in_place
//! derived_raster_path    bindings.rs     derived_folder, keep_derived
//! suggested_cell_size    bindings.rs     bind_raster (a SURFACE source with no CELL)
//!                        gdal_verbs.rs   run_line (a SURFACE source with no CELL)
//!                        terrain_verbs.rs bind_terrain_source (the same)
//! surface_grid           bindings.rs     bind_raster (a SURFACE source, on the worker)
//! raster_elevations      terrain_verbs.rs surface_from_raster_file
//! ```

use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::{Box2, Point3};
use katana_gis::processing::RasterGrid;
use katana_terrain::TinSurface;

use super::geo_verbs::Context;

/// The Unsupported refusal of a stand-in for a C++ function not yet ported.
fn waits(function: &str) -> Error {
    Error::new(
        ErrorCode::Unsupported,
        format!("{function} waits for interop's reference data (step 6, wave 2)"),
    )
}

/// What a derived raster kept as a reference raster gave back: its record
/// (`output arg=... kind=raster target=reference id=... name=... raster=WxH
/// file=... persisted=...`) and the raster's own coordinate system, which
/// keep_derived compares with the project's.
#[allow(
    dead_code,
    reason = "made by the real call that replaces the stand-in (step 6, wave 2)"
)]
pub(crate) struct KeptRaster {
    pub(crate) record: String,
    pub(crate) crs: String,
}

/// The heights of a DEM band as points, for the TIN builder, and how they
/// were sampled: the C++'s RasterElevations, of which the frame reads these.
#[allow(
    dead_code,
    reason = "made by the real call that replaces the stand-in (step 6, wave 2)"
)]
pub(crate) struct RasterElevations {
    /// One point per kept sample, at the centre of its source pixel.
    pub(crate) points: Vec<Point3>,
    /// Every stride-th pixel of every stride-th row.
    pub(crate) stride: i32,
    /// Pixels looked at.
    pub(crate) sampled: u64,
    /// Of those, no-data or non-finite and left out.
    pub(crate) no_data: u64,
}

/// STAND-IN for the C++'s findRaster with bind_raster's two refusals: the
/// file of the reference raster of that id (digits) or name (any case).
/// NotFound for none - the C++'s answer for an empty set, and so this one's;
/// Unsupported "the reference raster has no file to read at full precision"
/// for one with no file behind it.
pub(crate) fn reference_raster_file(_context: &Context<'_>, id_or_name: &str) -> Result<String> {
    Err(make_error(
        ErrorCode::NotFound,
        "no reference raster has that id or name; REFS lists them",
        id_or_name,
    ))
}

/// STAND-IN for the C++'s referenceNameTaken: whether a reference raster has
/// `name`, in any case. False, as for an empty set.
pub(crate) fn reference_name_taken(_context: &Context<'_>, _name: &str) -> bool {
    false
}

/// STAND-IN for the RASTER arm of the C++'s sourceName: the name of the
/// reference raster a RASTER <id|name> source names, or the word as given
/// when none has it - the C++'s fallback, and so this one's.
pub(crate) fn source_raster_name(_context: &Context<'_>, word: &str) -> String {
    String::from(word)
}

/// STAND-IN for what the C++'s keepDerived and keepInPlace do once the name
/// and the file are settled: the raster read as a derived reference raster
/// (derivedOverlay, its derivation the line), added to the reference data,
/// the front end's `changed` and `frame` told, and the record made.
pub(crate) fn keep_derived_raster(
    _context: &mut Context<'_>,
    _file: &Path,
    _name: &str,
    _derivation: &str,
    _persisted: bool,
    _arg: &str,
) -> Result<KeptRaster> {
    Err(waits("derivedOverlay"))
}

/// The C++'s derivedRasterPath: where a derived raster called `name` is kept,
/// `<project>/cache/gdal/<name>.tif` when the drawing has a project directory,
/// else `<scratch>/<name>.tif` (a reply then says persisted=no). The name is
/// made safe for a file: anything but letters, digits, '.', '-' and '_'
/// becomes '_', byte by byte. Nothing is created.
pub(crate) fn derived_raster_path(context: &Context<'_>, name: &str) -> PathBuf {
    let mut safe = String::new();
    for byte in name.bytes() {
        let plain = byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'_';
        safe.push(if plain { char::from(byte) } else { '_' });
    }
    // A name of dots alone would be the folder or its parent.
    if safe.bytes().all(|byte| byte == b'.') {
        safe = String::from("raster");
    }
    let folder = match context.document.project_directory() {
        Some(project) => project.join("cache").join("gdal"),
        None => context.scratch.to_path_buf(),
    };
    folder.join(format!("{safe}.tif"))
}

/// STAND-IN for the C++'s suggestedCellSize (its default target of 1000
/// cells along the longer side): a round cell for a surface's extent.
pub(crate) fn suggested_cell_size(_bounds: &Box2) -> Result<f64> {
    Err(waits("suggestedCellSize"))
}

/// STAND-IN for the C++'s surfaceGrid (its default cap of 25 000 000 cells):
/// a surface sampled at the centres of `cell`-sized cells over its extent.
pub(crate) fn surface_grid(_surface: &TinSurface, _cell: f64) -> Result<RasterGrid> {
    Err(waits("surfaceGrid"))
}

/// STAND-IN for the C++'s readRasterElevations (band 1): a DEM's true band
/// values through GDAL on the stride that keeps them under `max_points`,
/// `area` cutting the raster first.
pub(crate) fn raster_elevations(
    _path: &Path,
    _max_points: usize,
    _area: Option<Box2>,
) -> Result<RasterElevations> {
    Err(waits("readRasterElevations"))
}
