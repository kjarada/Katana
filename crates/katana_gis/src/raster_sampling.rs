//! A raster's value at any point (include/katana/gis/raster_sampling.hpp,
//! src/katana_io/geo/raster_sampling.cpp; docs/terrain.md, "Sampling and
//! drape"): what RASTER SAMPLE reports, DRAPE sets on the drawing's points
//! and vertices, and LOS walks along a sight line. GDAL interpolates
//! (GDALRasterInterpolateAtPoint), so the value between cell centres is
//! GDAL's bilinear - or nearest, cubic, cubic spline - and not a second
//! interpolation of Katana's.
//!
//! Absent is not zero: a point off the raster, or one whose interpolation
//! window touches a no-data cell, has no value, and the caller says so
//! rather than reading 0.
//!
//! A sampler holds its own open dataset and is used on one thread: the job's
//! worker opens it and drops it (never a dataset shared between runs,
//! docs/geoprocessing.md "Risks"). As GdalDataset is, it is Send and not
//! Sync, and `open` returns it by value.

#![allow(
    unsafe_code,
    reason = "Owns a GDAL dataset and samples its band through GDAL's C API; every call states why it is sound."
)]

use std::path::Path;
use std::ptr::{self, NonNull};

use katana_core::error::make_error;
use katana_core::path_text::path_exists;
use katana_core::{ErrorCode, Result};

use crate::ffi::{cpl, gdal};
use crate::formats::is_virtual_path;
use crate::gdal_adapter::RasterInfo;
use crate::gdal_registry::ensure_registered;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resampling {
    Nearest,
    Bilinear,
    Cubic,
    CubicSpline,
}

fn gdal_resampling(resampling: Resampling) -> gdal::GdalRioResampleAlg {
    match resampling {
        Resampling::Nearest => gdal::GRIORA_NEAREST_NEIGHBOUR,
        Resampling::Bilinear => gdal::GRIORA_BILINEAR,
        Resampling::Cubic => gdal::GRIORA_CUBIC,
        Resampling::CubicSpline => gdal::GRIORA_CUBIC_SPLINE,
    }
}

/// "nearest", "bilinear", "cubic", "cubicspline" (any case); InvalidArgument
/// naming the word and the choices otherwise.
pub fn resampling_named(name: &str) -> Result<Resampling> {
    match name.to_ascii_lowercase().as_str() {
        "nearest" => Ok(Resampling::Nearest),
        "bilinear" => Ok(Resampling::Bilinear),
        "cubic" => Ok(Resampling::Cubic),
        "cubicspline" => Ok(Resampling::CubicSpline),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            "method is bilinear, nearest, cubic or cubicspline",
            name,
        )),
    }
}

pub fn to_string(resampling: Resampling) -> &'static str {
    match resampling {
        Resampling::Nearest => "nearest",
        Resampling::Bilinear => "bilinear",
        Resampling::Cubic => "cubic",
        Resampling::CubicSpline => "cubicspline",
    }
}

#[derive(Debug)]
pub struct RasterSampler {
    dataset: NonNull<gdal::GdalDatasetHs>,
    band: gdal::GdalRasterBandH,
    info: RasterInfo,
    /// The inverse of info.geotransform: ground to pixel and line.
    inverse: [f64; 6],
}

// SAFETY: GDAL lets a dataset and its bands be used from any one thread at a
// time; the value is not Sync, so it is never used by two threads at once.
unsafe impl Send for RasterSampler {}

impl Drop for RasterSampler {
    fn drop(&mut self) {
        // SAFETY: the dataset is owned by this value alone, closed once.
        unsafe { gdal::GDALClose(self.dataset.as_ptr()) };
    }
}

/// A dataset opened by `open`, closed on drop until the sampler takes it.
struct Opened(NonNull<gdal::GdalDatasetHs>);

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: the dataset is owned by this value alone, closed once.
        unsafe { gdal::GDALClose(self.0.as_ptr()) };
    }
}

impl RasterSampler {
    /// Band `band` (1-based) of the raster at `path` - a file, a /vsi path, a
    /// URL - read at full precision. NotFound for a local file that is not
    /// there; FileImportFailure with GDAL's message when GDAL cannot open it
    /// as a raster; InvalidArgument for a band it does not have, or a raster
    /// with no georeferencing (its cells have no place on the ground).
    pub fn open(path: &Path, band: i32) -> Result<RasterSampler> {
        ensure_registered();
        let utf8 = String::from_utf8_lossy(&cpl::generic_path(path)).into_owned();
        if !is_virtual_path(path) && !path_exists(path).unwrap_or(false) {
            return Err(make_error(ErrorCode::NotFound, "file does not exist", utf8));
        }
        let name = cpl::c_string(cpl::generic_path(path));
        // SAFETY: clears this thread's last error; then a NUL-terminated name
        // and no lists; the dataset is owned by the Opened made below.
        let dataset = unsafe {
            cpl::CPLErrorReset();
            gdal::GDALOpenEx(
                name.as_ptr(),
                gdal::GDAL_OF_READONLY | gdal::GDAL_OF_RASTER,
                ptr::null(),
                ptr::null(),
                ptr::null(),
            )
        };
        let Some(dataset) = NonNull::new(dataset).map(Opened) else {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                format!("GDAL could not open '{utf8}' as a raster"),
                // SAFETY: this thread's last error message, copied at once.
                unsafe { cpl::decoded(cpl::CPLGetLastErrorMsg()) },
            ));
        };
        let handle = dataset.0.as_ptr();
        // SAFETY: a live dataset.
        let count = unsafe { gdal::GDALGetRasterCount(handle) };
        if band < 1 || band > count {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the raster has no such band",
                format!("{band}"),
            ));
        }
        let mut info = RasterInfo::default();
        let mut has_no_data: std::ffi::c_int = 0;
        // SAFETY: a live dataset and one of its bands; the projection is the
        // dataset's own text, copied at once; the geotransform is six doubles.
        let raster_band = unsafe {
            let raster_band = gdal::GDALGetRasterBand(handle, band);
            info.width = gdal::GDALGetRasterXSize(handle);
            info.height = gdal::GDALGetRasterYSize(handle);
            info.band_count = count;
            let wkt = gdal::GDALGetProjectionRef(handle);
            if !wkt.is_null() {
                info.projection_wkt = cpl::decoded(wkt);
            }
            info.has_geotransform =
                gdal::GDALGetGeoTransform(handle, info.geotransform.as_mut_ptr()) == cpl::CE_NONE;
            let no_data = gdal::GDALGetRasterNoDataValue(raster_band, &raw mut has_no_data);
            if has_no_data != 0 {
                info.no_data_value = Some(no_data);
            }
            raster_band
        };
        // A raster without a geotransform is placed by GDAL's default at the
        // origin, a unit per pixel: sampled so, a height would come from a
        // place on the ground the raster never described.
        if !info.has_geotransform {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the raster has no georeferencing, so no cell has a place on the ground",
                utf8,
            ));
        }
        let mut inverse = [0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // SAFETY: six doubles in, six out.
        if unsafe { gdal::GDALInvGeoTransform(info.geotransform.as_ptr(), inverse.as_mut_ptr()) }
            == 0
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the raster's geotransform cannot be inverted",
                utf8,
            ));
        }
        let dataset = std::mem::ManuallyDrop::new(dataset).0;
        Ok(RasterSampler {
            dataset,
            band: raster_band,
            info,
            inverse,
        })
    }

    /// The value at the ground point (x, y) by `resampling`; none off the
    /// raster, on or beside a no-data cell (GDAL refuses an interpolation
    /// window that touches one), or where GDAL gives no finite number.
    #[allow(
        clippy::nonminimal_bool,
        reason = "The C++'s !(pixel >= 0 && ...), which takes a NaN as off the raster."
    )]
    pub fn at(&self, x: f64, y: f64, resampling: Resampling) -> Option<f64> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let inverse = &self.inverse;
        let pixel = inverse[0] + x * inverse[1] + y * inverse[2];
        let line = inverse[3] + x * inverse[4] + y * inverse[5];
        // Off the raster, asked here rather than of GDAL: its refusal would
        // raise an error per point of a line that runs off the data.
        if !(pixel >= 0.0
            && line >= 0.0
            && pixel <= f64::from(self.info.width)
            && line <= f64::from(self.info.height))
        {
            return None;
        }
        let mut real = 0.0;
        let mut imaginary = 0.0;
        // SAFETY: the sampler's live band and two out values.
        let status = unsafe {
            gdal::GDALRasterInterpolateAtPoint(
                self.band,
                pixel,
                line,
                gdal_resampling(resampling),
                &raw mut real,
                &raw mut imaginary,
            )
        };
        if status != cpl::CE_NONE {
            return None;
        }
        // GDAL refuses a bilinear or cubic window that touches no-data;
        // nearest hands the cell back, which may be the no-data value itself.
        if !real.is_finite()
            || self
                .info
                .no_data_value
                .is_some_and(|no_data| real == no_data)
        {
            return None;
        }
        Some(real)
    }

    pub fn info(&self) -> &RasterInfo {
        &self.info
    }

    /// The longer side of a cell, in ground units: what a sight line steps by.
    pub fn cell_size(&self) -> f64 {
        let gt = &self.info.geotransform;
        let across = gt[1].hypot(gt[4]);
        let down = gt[2].hypot(gt[5]);
        // std::max, as the C++ has it.
        if across < down { down } else { across }
    }
}
