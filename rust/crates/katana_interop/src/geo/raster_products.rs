//! Rasters in and out of a geoprocessing run
//! (include/katana/interop/geo/raster_products.hpp,
//! src/katana_interop/geo/raster_products.cpp; docs/geoprocessing.md,
//! "Bindings"): a TIN surface as the grid an algorithm reads, where a derived
//! raster file is kept, and such a file as the reference raster the drawing
//! shows.

use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Point2;
use katana_gis::gdal_adapter::RasterInfo;
use katana_gis::processing::RasterGrid;
use katana_math::cpp;
use katana_terrain::TinSurface;

use crate::import::{RasterImportOptions, import_raster};
use crate::reference_data::{RasterOverlay, RasterRole};

/// The no-data value of a grid sampled from a surface - a cell whose centre
/// is off the surface or in a hole - and of a DEM export_surface_raster
/// writes: the conventional Float32 no-data value of Esri and GDAL DEMs,
/// -FLT_MAX. Unlike the other common sentinel, -9999, it cannot be a real
/// elevation on any body a surveyor works on - the Challenger Deep is about
/// -10 935 m - so ground that truly lies at -9999 is not silently punched
/// full of holes. It is exact in Float32 as well as Float64, so it survives
/// a reader or an algorithm that works in Float32 (GDAL's AAIGrid reads so by
/// default).
pub const GRID_NO_DATA: f64 = f32::MIN as f64;

/// The surface sampled at the CENTRE of each `cell`-square cell by the TIN's
/// own linear interpolation, north-up, its top-left corner at the surface's
/// (min x, max y) - the grid export_surface_raster writes, from the same
/// code. InvalidArgument for an empty surface, a cell that is not positive
/// and finite, or more than `max_cells` cells (named in full, with the cell
/// to use instead). The C++'s default `max_cells` is 25 000 000.
pub fn surface_grid(surface: &TinSurface, cell: f64, max_cells: u64) -> Result<RasterGrid> {
    if surface.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the surface has no triangles",
        ));
    }
    if !(cell > 0.0) || !cell.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the cell size must be a positive, finite length",
            format_exact_real(cell),
        ));
    }
    let bounds = *surface.bounds();
    // Counted in doubles first: a cell of a micrometre over a 10 km site is
    // 10^20 cells, which no integer type here would hold.
    let columns_wanted = cpp::max(1.0, (bounds.width() / cell).ceil());
    let rows_wanted = cpp::max(1.0, (bounds.height() / cell).ceil());
    let cells_wanted = columns_wanted * rows_wanted;
    if cells_wanted > max_cells as f64
        || columns_wanted > f64::from(i32::MAX)
        || rows_wanted > f64::from(i32::MAX)
    {
        // Named in full: "too many cells" leaves the user guessing how much
        // larger a cell has to be.
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "a {} cell over this surface is {} cells ({} x {}), more than the {max_cells} \
                 allowed; choose a larger cell",
                format_exact_real(cell),
                format_exact_real(cells_wanted),
                format_exact_real(columns_wanted),
                format_exact_real(rows_wanted)
            ),
        ));
    }

    let width = columns_wanted as i32;
    let height = rows_wanted as i32;
    let mut grid = RasterGrid {
        info: RasterInfo {
            width,
            height,
            band_count: 1,
            // North-up, top-left corner at (min x, max y): row 0 is the
            // northern edge.
            geotransform: [bounds.min.x, cell, 0.0, bounds.max.y, 0.0, -cell],
            has_geotransform: true,
            no_data_value: Some(GRID_NO_DATA),
            ..RasterInfo::default()
        },
        data_type: String::from("Float64"),
        bands: Vec::new(),
        no_data: vec![Some(GRID_NO_DATA)],
    };

    let columns = width as usize;
    let mut values = vec![GRID_NO_DATA; columns * height as usize];
    // One row of positions at a time: the grid of values has to be whole for
    // GDAL, but a grid of Point2 beside it would double the memory for
    // nothing, and a row is still thousands of positions for elevations_at
    // to share across threads.
    let mut positions = vec![Point2::default(); columns];
    for row in 0..height {
        let y = bounds.max.y - (f64::from(row) + 0.5) * cell;
        for (column, position) in positions.iter_mut().enumerate() {
            *position = Point2::new(bounds.min.x + (column as f64 + 0.5) * cell, y);
        }
        let elevations = surface.elevations_at(&positions);
        let out = &mut values[row as usize * columns..(row as usize + 1) * columns];
        for (cell_value, elevation) in out.iter_mut().zip(&elevations) {
            if let Some(z) = elevation {
                *cell_value = *z;
            }
        }
    }
    grid.bands.push(values);
    Ok(grid)
}

/// Where a derived raster called `name` is kept:
/// <project>/cache/gdal/<name>.tif when the drawing has a project directory,
/// else <scratch>/<name>.tif (a reply then says persisted=no). The name is
/// made safe for a file: anything but letters, digits, '.', '-' and '_'
/// becomes '_' - byte by byte, as the C++ walks its chars. Nothing is
/// created.
pub fn derived_raster_path(
    project_directory: Option<&Path>,
    scratch: &Path,
    name: &str,
) -> PathBuf {
    let mut safe = String::new();
    for byte in name.bytes() {
        let plain = byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_');
        safe.push(if plain { char::from(byte) } else { '_' });
    }
    // A name of dots alone would be the folder or its parent.
    if safe.bytes().all(|byte| byte == b'.') {
        safe = String::from("raster");
    }
    let folder = match project_directory {
        Some(project) => project.join("cache").join("gdal"),
        None => scratch.to_path_buf(),
    };
    folder.join(format!("{safe}.tif"))
}

/// `file`, read as a reference raster (import_raster) with role Derived and
/// the line that made it. Its band facts are left for the reference-layer
/// work that shows them.
pub fn derived_overlay(file: &Path, name: String, derivation: String) -> Result<RasterOverlay> {
    let options = RasterImportOptions {
        name,
        ..RasterImportOptions::default()
    };
    let mut overlay = import_raster(file, &options)?;
    overlay.role = RasterRole::Derived;
    overlay.derivation = derivation;
    Ok(overlay)
}
