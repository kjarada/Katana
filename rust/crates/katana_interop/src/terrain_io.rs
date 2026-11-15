//! Terrain to and from GDAL and PDAL data (include/katana/interop/terrain_io.hpp,
//! src/katana_interop/terrain_io.cpp): the elevations of a DEM, the ground of
//! a point cloud, and a surface written out as a DEM.
//!
//! Here rather than in katana_terrain because terrain may not see the readers
//! (tools/check_layering.cmake), and rather than in the desktop application
//! because the CLI and the tests need the same conversions - a policy such as
//! "a surface from a cloud is built from its ground returns" must have one
//! definition, not one per front end.
//!
//! The C++'s `std::stop_token` is `Option<&AtomicBool>`, as the bridge's run
//! takes it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use katana_core::error::make_error;
use katana_core::path_text::path_exists;
use katana_core::text::format_exact_real;
use katana_core::{ErrorCode, Result};
use katana_geometry::{Box2, Point3};
use katana_gis::gdal_adapter::GdalDataset;
use katana_gis::processing::{ArgValue, DatasetPath, DatasetValue, OutputTo, RunRequest, run};
use katana_math::cpp;
use katana_terrain::TinSurface;

use crate::geo::raster_products::{GRID_NO_DATA, surface_grid};
use crate::reference_data::PointCloudLayer;

/// `path.string()`, for an error's context.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A path as UTF-8 with '/' between its names, as GDAL is given it.
fn utf8_of(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// Samples a stride keeps along one side: ceil(extent / stride), in 64 bits.
fn samples_along(extent: i64, stride: i64) -> i64 {
    (extent + stride - 1) / stride
}

fn samples_for(width: i32, height: i32, stride: i64) -> u64 {
    (samples_along(i64::from(width), stride) as u64)
        .wrapping_mul(samples_along(i64::from(height), stride) as u64)
}

/// The window `area` of the raster at `path`, cut by GDAL into a file of its
/// own in the temporary folder: the reader then samples the cut as a whole,
/// so the stride is chosen for the site and not for the sheet. Each call has
/// its own file (the bridge names them), which the caller removes.
fn cut_window(path: &Path, area: &Box2) -> Result<PathBuf> {
    let real = format_exact_real;
    let clip = RunRequest {
        path: vec![String::from("raster"), String::from("clip")],
        values: vec![(
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Path(DatasetPath {
                path: utf8_of(path),
                ..DatasetPath::default()
            })),
        )],
        tokens: vec![
            format!(
                "--bbox={},{},{},{}",
                real(area.min.x),
                real(area.min.y),
                real(area.max.x),
                real(area.max.y)
            ),
            // A window reaching past the raster takes what is there; the
            // rest reads as no-data and is left out, counted.
            String::from("--allow-bbox-outside-source"),
        ],
        // Every cell to a file: the cut of a large DEM is as large as the
        // site, and the reader streams a file a row at a time.
        max_memory_cells: 0,
        ..RunRequest::default()
    };
    let cut = run(&clip, None, None)
        .map_err(|error| make_error(error.code, error.message, path_string(path)))?;
    match cut.file {
        Some(file) => Ok(PathBuf::from(file)),
        None => Err(make_error(
            ErrorCode::Internal,
            "GDAL's clip kept no raster",
            path_string(path),
        )),
    }
}

// ---- DEM -> elevations --------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct RasterElevationOptions {
    /// At most this many points are returned. The source is sampled on a
    /// regular stride chosen so that the whole extent is covered at or under
    /// the cap - never truncated to its first rows. 0 is refused.
    pub max_points: usize,
    pub band: i32,
    /// Only the pixels of this window of the ground, so the cap is spent on
    /// the site rather than on the whole sheet a DEM was delivered as. Cut by
    /// GDAL (`raster clip --bbox`) before the stride is chosen; a window
    /// reaching past the raster takes what the raster has there.
    pub area: Option<Box2>,
}

impl Default for RasterElevationOptions {
    fn default() -> Self {
        Self {
            max_points: 400_000,
            band: 1,
            area: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RasterElevations {
    /// One point per kept sample, at the CENTRE of its source pixel, carrying
    /// the band's value unchanged. Georeferencing is the file's own.
    pub points: Vec<Point3>,
    /// Every stride-th pixel of every stride-th row.
    pub stride: i32,
    /// Pixels looked at.
    pub sampled: u64,
    /// Of those, no-data or non-finite and left out.
    pub no_data: u64,
    pub projection_wkt: String,
}

impl Default for RasterElevations {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            stride: 1,
            sampled: 0,
            no_data: 0,
            projection_wkt: String::new(),
        }
    }
}

/// The stride that brings a width x height grid to at most max_points
/// samples: the smallest s with ceil(width/s) * ceil(height/s) <= max_points.
/// Exposed so the cap can be tested at its edges without a file (audit
/// QT-24).
pub fn stride_for_cap(width: i32, height: i32, max_points: usize) -> i32 {
    if width <= 0 || height <= 0 {
        return 1; // nothing to sample; any stride samples all of it
    }
    let longest = i64::from(width.max(height));
    if max_points == 0 {
        // No stride brings a grid to zero samples. The coarsest meaningful
        // one - a single sample - is the closest answer; callers refuse a
        // zero cap before asking (read_raster_elevations does).
        return longest as i32;
    }
    let cap = max_points as u64;
    if samples_for(width, height, 1) <= cap {
        return 1;
    }
    // The sample count ceil(w/s) * ceil(h/s) never increases with s, so the
    // smallest s meeting the cap is found by bisection between a stride known
    // to be too small and one known to be enough. The lower bound is the
    // continuous estimate: ceil(w/s) * ceil(h/s) >= w*h / s^2, so no s below
    // sqrt(w*h / cap) can meet the cap - one less than its floor is safely
    // below, whatever the rounding of the square root. The upper bound is the
    // longer side, where one sample remains. Stepping up from the estimate
    // one stride at a time would also be exact, but not quick: on a one-pixel
    // strip the ceilings put the answer far above the estimate - 1 x 10^9
    // pixels capped at 10 is a stride of 10^8 against an estimate of 10^4.
    //
    // The audit (QT-24) found the GUI taking floor(sqrt(ceil(w*h / cap))),
    // which the floor can leave at 1 with the grid three times over the cap.
    let estimate = (f64::from(width) * f64::from(height) / cap as f64).sqrt();
    let mut too_small = 0_i64.max(estimate as i64 - 1);
    let mut enough = longest;
    // Invariant: samples_for(too_small) > cap (or too_small == 0),
    // samples_for(enough) <= cap.
    while enough - too_small > 1 {
        let middle = too_small + (enough - too_small) / 2;
        if samples_for(width, height, middle) <= cap {
            enough = middle;
        } else {
            too_small = middle;
        }
    }
    enough as i32
}

/// The elevations of a DEM band as points, for the TIN builder: the true band
/// values through GDAL, not the 8-bit display copy a RasterOverlay holds
/// (audit QT-23). NotFound for a missing file; InvalidArgument for a raster
/// with no georeferencing (its pixels have no ground position, and inventing
/// one would build a surface in the wrong place), a band out of range, or a
/// zero cap; FileImportFailure when GDAL cannot read it.
pub fn read_raster_elevations(
    path: &Path,
    options: &RasterElevationOptions,
) -> Result<RasterElevations> {
    if !path_exists(path).unwrap_or(false) {
        return Err(make_error(
            ErrorCode::NotFound,
            "file does not exist",
            path_string(path),
        ));
    }
    if options.max_points == 0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a DEM cannot be read into zero points; the cap must be at least 1",
            path_string(path),
        ));
    }

    let dataset = GdalDataset::open(path)?;
    if !dataset.has_raster() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "file holds no raster bands",
            path_string(path),
        ));
    }
    let info = dataset.raster_info()?;
    if !info.has_geotransform {
        // GDAL's default transform would put pixel (i, j) at (i, j) in world
        // units, and a surface built there is in the wrong place with nothing
        // to say so.
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "this raster is not georeferenced, so its pixels have no ground position",
            path_string(path),
        ));
    }
    if !(1..=info.band_count).contains(&options.band) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "raster band index is out of range",
            format!(
                "requested {} of {} in {}",
                options.band,
                info.band_count,
                path_string(path)
            ),
        ));
    }
    if let Some(area) = &options.area {
        if area.empty() || !(area.width() > 0.0) || !(area.height() > 0.0) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the window of a raster to read needs a width and a height",
                path_string(path),
            ));
        }
        drop(dataset); // the cut opens the file itself
        let cut = cut_window(path, area)?;
        let whole = RasterElevationOptions {
            area: None,
            ..options.clone()
        };
        let read = read_raster_elevations(&cut, &whole);
        let _ = std::fs::remove_file(cut);
        return read;
    }

    let stride = stride_for_cap(info.width, info.height, options.max_points);
    let samples = dataset.read_band_sampled(options.band, stride)?;

    let mut result = RasterElevations {
        stride,
        projection_wkt: info.projection_wkt.clone(),
        sampled: (samples.columns as u64).wrapping_mul(samples.rows as u64),
        ..RasterElevations::default()
    };
    result.points.reserve(result.sampled as usize);

    let gt = &info.geotransform;
    let columns = samples.columns.max(0) as usize;
    for j in 0..samples.rows {
        for i in 0..samples.columns {
            let value = samples.values[j as usize * columns + i as usize];
            if !value.is_finite()
                || samples
                    .no_data_value
                    .is_some_and(|no_data| value == no_data)
            {
                result.no_data += 1;
                continue;
            }
            // The CENTRE of the source pixel: a DEM's value is the ground at
            // the middle of its cell (the "pixel is area" convention GDAL
            // reports every geotransform in), and the corner would shift the
            // whole surface half a pixel north-west.
            let px = f64::from(i) * f64::from(stride) + 0.5;
            let py = f64::from(j) * f64::from(stride) + 0.5;
            result.points.push(Point3::new(
                gt[0] + px * gt[1] + py * gt[2],
                gt[3] + px * gt[4] + py * gt[5],
                value,
            ));
        }
    }
    Ok(result)
}

// ---- point cloud -> surface points --------------------------------------------------------------

/// ASPRS LAS 1.4 R15 table 17: class 2 is Ground.
pub const ASPRS_GROUND: u8 = 2;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CloudSurfacePoints {
    pub points: Vec<Point3>,
    /// True when the points are the cloud's ground returns alone. False when
    /// the cloud carries no class-2 point - unclassified photogrammetry, a
    /// terrestrial scan - and every return was taken, so trees and roofs are
    /// part of the surface. The caller must SAY which it was: a canopy
    /// surface presented as ground is the silent failure audit QT-10 found.
    pub ground_only: bool,
    /// Returns left out because they are classified as something else.
    pub excluded: u64,
}

/// The one policy for a surface from a cloud: its ground returns when it has
/// any, otherwise every return, flagged.
pub fn surface_points(cloud: &PointCloudLayer) -> CloudSurfacePoints {
    let mut result = CloudSurfacePoints::default();
    let ground = cloud
        .points
        .iter()
        .filter(|point| point.classification == ASPRS_GROUND)
        .count();
    if ground == 0 {
        // Nothing is classified as ground: an unclassified cloud, not a
        // cloud with no ground. Every return is used and the caller told so,
        // rather than refusing - which would leave photogrammetry and
        // terrestrial scans with no way to become a surface at all.
        result.points.reserve(cloud.points.len());
        for point in &cloud.points {
            result.points.push(Point3::new(point.x, point.y, point.z));
        }
        return result;
    }
    result.ground_only = true;
    result.points.reserve(ground);
    for point in &cloud.points {
        if point.classification == ASPRS_GROUND {
            result.points.push(Point3::new(point.x, point.y, point.z));
        } else {
            result.excluded += 1;
        }
    }
    result
}

/// The C++'s `surfacePoints(cloud, classes)`: the returns of the ASPRS
/// classes asked for, whatever the cloud holds - a person who knows the
/// classes of their data says so (SURFACE FROM CLOUD classes=2,8).
/// ground_only is true when the classes are exactly {2}; `excluded` counts
/// the returns of every other class. An empty list is the policy above.
pub fn surface_points_of_classes(cloud: &PointCloudLayer, classes: &[u8]) -> CloudSurfacePoints {
    if classes.is_empty() {
        return surface_points(cloud);
    }
    let wanted: BTreeSet<u8> = classes.iter().copied().collect();
    let mut result = CloudSurfacePoints {
        ground_only: wanted.len() == 1 && wanted.contains(&ASPRS_GROUND),
        ..CloudSurfacePoints::default()
    };
    for point in &cloud.points {
        if wanted.contains(&point.classification) {
            result.points.push(Point3::new(point.x, point.y, point.z));
        } else {
            result.excluded += 1;
        }
    }
    result
}

// ---- surface -> DEM ------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceRasterOptions {
    /// World units per cell, square. Must be positive and finite.
    pub cell_size: f64,
    /// Empty: from the path's extension (GdalDataset::raster_driver_for_path),
    /// or COG when `cog` is set.
    pub driver: String,
    pub projection_wkt: String,
    /// Largest grid written, columns x rows. The whole grid is held as
    /// doubles before GDAL writes it, so this is 200 MB; a finer grid is
    /// refused with InvalidArgument naming the cell count, rather than
    /// exhausting memory.
    pub max_cells: u64,
    /// What each cell is stored as: Float32 (the default) or Float64.
    /// Float32 is the DEM convention and half the size; near 1000 m its step
    /// is 6e-5 m, far finer than any survey that made the surface. The
    /// no-data value is exact in both.
    pub data_type: String,
    /// A Cloud Optimised GeoTIFF, written by GDAL's COG driver: tiled,
    /// compressed and with overviews, readable a window at a time over HTTP.
    pub cog: bool,
    /// KEY=VALUE, GDAL's creation options for the driver, over Katana's
    /// defaults for it: a GeoTIFF is tiled and DEFLATE-compressed with the
    /// floating-point predictor (PREDICTOR=3), a COG DEFLATE with
    /// PREDICTOR=YES.
    pub creation_options: Vec<String>,
    /// A file already at the path is replaced only when this says so; else
    /// AlreadyExists, and the file is left as it was.
    pub overwrite: bool,
}

impl Default for SurfaceRasterOptions {
    fn default() -> Self {
        Self {
            cell_size: 1.0,
            driver: String::new(),
            projection_wkt: String::new(),
            max_cells: 25_000_000,
            data_type: String::from("Float32"),
            cog: false,
            creation_options: Vec::new(),
            overwrite: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceRasterResult {
    pub columns: i32,
    pub rows: i32,
    pub cells_with_data: u64,
    /// Written into every cell whose centre is off the surface or in a hole,
    /// and declared as the band's no-data value.
    pub no_data_value: f64,
    pub driver: String,
    pub data_type: String,
    pub geotransform: [f64; 6],
}

impl Default for SurfaceRasterResult {
    fn default() -> Self {
        Self {
            columns: 0,
            rows: 0,
            cells_with_data: 0,
            no_data_value: 0.0,
            driver: String::new(),
            data_type: String::new(),
            geotransform: [0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
        }
    }
}

/// Each cell's value is the surface's elevation at the cell's CENTRE, by the
/// TIN's own linear interpolation (TinSurface::elevations_at) - the value a
/// surveyor would read off the surface there, not an average over the cell.
/// The grid is north-up with its top-left corner at the surface's (min x,
/// max y). The grid is written by GDAL's own `raster convert` (the bridge),
/// so every driver that can write a raster - those that only copy a finished
/// dataset, as AAIGrid, included - and every creation option it takes are
/// there. InvalidArgument for an empty surface, a bad cell size, too many
/// cells or a data type other than Float32 and Float64; Unsupported for an
/// extension no raster driver claims; AlreadyExists for a file in the way
/// without `overwrite`; InvalidState "cancelled" when `stop` is set (no file
/// is left); what GDAL refused otherwise.
pub fn export_surface_raster(
    surface: &TinSurface,
    path: &Path,
    options: &SurfaceRasterOptions,
    stop: Option<&AtomicBool>,
) -> Result<SurfaceRasterResult> {
    if surface.empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the surface has no triangles to export",
            path_string(path),
        ));
    }
    if !(options.cell_size > 0.0) || !options.cell_size.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the cell size must be a positive, finite length",
            format_exact_real(options.cell_size),
        ));
    }
    if options.data_type != "Float32" && options.data_type != "Float64" {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a DEM is written as Float32 or Float64",
            options.data_type.clone(),
        ));
    }

    let mut driver = if options.cog {
        String::from("COG")
    } else {
        options.driver.clone()
    };
    if driver.is_empty() {
        driver = GdalDataset::raster_driver_for_path(path)?;
    }
    if !options.overwrite && path_exists(path).unwrap_or(false) {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "a file is already there; replacing it has to be asked for",
            path_string(path),
        ));
    }

    // The one sampler (geo::raster_products::surface_grid), so a DEM written
    // here and the grid a geoprocessing run reads from the same surface are
    // the same grid.
    let mut grid = surface_grid(surface, options.cell_size, options.max_cells)
        .map_err(|error| make_error(error.code, error.message, path_string(path)))?;
    let cells_with_data = grid.bands.first().map_or(0, |values| {
        values
            .iter()
            .filter(|&&value| value != GRID_NO_DATA)
            .count()
    });
    let result = SurfaceRasterResult {
        columns: grid.info.width,
        rows: grid.info.height,
        cells_with_data: cells_with_data as u64,
        no_data_value: GRID_NO_DATA,
        driver: driver.clone(),
        data_type: options.data_type.clone(),
        geotransform: grid.info.geotransform,
    };
    // The grid is made in the type asked for, so GDAL rounds each double to
    // the nearest Float32 once, as it fills the grid, and the file holds
    // that.
    grid.data_type.clone_from(&options.data_type);
    grid.info.projection_wkt.clone_from(&options.projection_wkt);

    // Katana's defaults for the driver, then the caller's options over them
    // by key: a DEM is read a window at a time, so tiled, and floating-point
    // heights compress better with the floating-point predictor.
    let mut creation: Vec<(String, String)> = match driver.as_str() {
        "GTiff" => vec![
            (String::from("TILED"), String::from("YES")),
            (String::from("COMPRESS"), String::from("DEFLATE")),
            (String::from("PREDICTOR"), String::from("3")),
        ],
        "COG" => vec![
            (String::from("COMPRESS"), String::from("DEFLATE")),
            (String::from("PREDICTOR"), String::from("YES")),
        ],
        _ => Vec::new(),
    };
    for option in &options.creation_options {
        let Some(equals) = option.find('=').filter(|&equals| equals != 0) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a creation option is KEY=VALUE",
                option.clone(),
            ));
        };
        let key = option[..equals].to_ascii_lowercase();
        creation.retain(|(kept, _)| kept.to_ascii_lowercase() != key);
        creation.push((
            String::from(&option[..equals]),
            String::from(&option[equals + 1..]),
        ));
    }

    let request = RunRequest {
        path: vec![String::from("raster"), String::from("convert")],
        values: vec![(
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(grid)),
        )],
        output_to: OutputTo::File,
        output_path: utf8_of(path),
        output_format: driver,
        creation_options: creation
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect(),
        overwrite: options.overwrite,
        ..RunRequest::default()
    };
    run(&request, stop, None)
        .map_err(|error| make_error(error.code, error.message, path_string(path)))?;
    Ok(result)
}

/// A cell size for `bounds` of about `target_cells` along its longer side,
/// rounded to 1, 2 or 5 x 10^n so the grid lines fall on round coordinates.
/// 1.0 for an empty or zero-sized box. The C++'s default `target_cells` is
/// 1000.
pub fn suggested_cell_size(bounds: &Box2, target_cells: i32) -> f64 {
    let longer = cpp::max(bounds.width(), bounds.height());
    if bounds.empty() || !(longer > 0.0) || !longer.is_finite() || target_cells < 1 {
        return 1.0;
    }
    let raw = longer / f64::from(target_cells);

    // value x 10^n. A power of ten is exact in a double up to 10^22, and
    // multiplying or dividing by one rounds once, so 5 x 10^-1 comes out as
    // the double nearest 0.5 rather than as 5 times an inexact 0.1. (powf is
    // the C library's pow, as std::pow is.)
    let scaled = |value: f64, n: i32| {
        if n >= 0 {
            value * 10.0_f64.powf(f64::from(n))
        } else {
            value / 10.0_f64.powf(f64::from(-n))
        }
    };

    // raw = mantissa x 10^exponent with the mantissa in [1, 10). log10 can
    // land a hair below an exact power of ten, so the split is corrected by
    // comparison rather than trusted.
    let mut exponent = raw.log10().floor() as i32;
    let mut mantissa = scaled(raw, -exponent);
    if mantissa >= 10.0 {
        exponent += 1;
        mantissa = scaled(raw, -exponent);
    } else if mantissa < 1.0 {
        exponent -= 1;
        mantissa = scaled(raw, -exponent);
    }

    // Nearest of 1, 2, 5 and 10 on a LOGARITHMIC scale, the scale on which a
    // cell size is judged (twice as fine is as far off as twice as coarse).
    // Its boundaries are the geometric means of neighbours: sqrt(2), sqrt(10)
    // and sqrt(50). On a linear scale 1.45 would round to 1, 3.3 to 2 and 7.3
    // to 5, although each is nearer by ratio to the step above it.
    let step = if mantissa < 2.0_f64.sqrt() {
        1.0
    } else if mantissa < 10.0_f64.sqrt() {
        2.0
    } else if mantissa < 50.0_f64.sqrt() {
        5.0
    } else {
        10.0
    };
    scaled(step, exponent)
}
