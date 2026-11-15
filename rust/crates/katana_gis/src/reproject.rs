//! Coordinate transformation and raster warping through GDAL/OGR, behind a
//! Katana interface (include/katana/gis/reproject.hpp,
//! src/katana_io/reproject.cpp).
//!
//! The imports of local files never reproject: a file's declared CRS is read
//! and reported (docs/interop.md, "No reprojection"). Data fetched from a web
//! service is different in kind - the person did not choose the service's
//! CRS, and a web map in EPSG:3857 is useless on an MGA drawing until it is
//! moved - so the online import brings each result into the project's CRS,
//! and this is what it does it with.
//!
//! AXIS ORDER. Every function here takes and returns coordinates in the
//! "traditional GIS order": x is easting or longitude, y northing or latitude,
//! whatever the authority says - OGR's OAMS_TRADITIONAL_GIS_ORDER. EPSG:4326
//! is officially latitude first, and WMS 1.3.0, WFS 2.0 and WMTS honour that;
//! `crs_axis_is_yx` is how a request builder asks whether it must swap, and
//! nothing else in the program ever sees a swapped pair.
//!
//! Threading: each call builds its own OGR objects, so calls on different
//! threads are independent; a warp runs on the calling thread. The C++'s
//! `std::stop_token` is `Option<&AtomicBool>` (katana_cad's drape has the
//! same), and its progress function a `Sync` closure: GDAL may call it from a
//! thread of its own. The callback catches a panic of the closure, cancels
//! the warp, and raises the panic again once GDAL has returned.

#![allow(
    unsafe_code,
    reason = "Reads coordinate systems and warps rasters through GDAL's C API; every call states why it is sound."
)]

use std::any::Any;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use katana_core::error::make_error;
use katana_core::text::{format_fixed, format_general, is_ascii_space};
use katana_core::{Error, ErrorCode, Result, Status};

use crate::cpl_error_collector::QuietHandler;
use crate::ffi::{cpl, gdal, ogr, osr};
use crate::formats_detail::remove_path;
use crate::gdal_adapter::VectorFeature;
use crate::gdal_registry::ensure_registered;
use crate::ogr_detail::{SpatialReference, exported_wkt};

/// No-data for a warped elevation: below any ground on Earth (the Dead Sea
/// shore is -430 m, the deepest trench about -11 000 m) and exactly
/// representable in Float32, so nothing real is ever mistaken for it.
const ELEVATION_NO_DATA: f64 = -32767.0;

/// An axis-aligned box in one CRS, traditional GIS order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CrsBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl CrsBox {
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    pub fn valid(&self) -> bool {
        self.max_x > self.min_x && self.max_y > self.min_y
    }
}

/// What a raster is, read from its header only: for a COG over /vsicurl/
/// that is one or two range requests, not the image. For discovering what a
/// pasted URL offers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RasterProbe {
    pub width: i32,
    pub height: i32,
    pub band_count: i32,
    /// GDAL's name: "Byte", "Float32".
    pub data_type: String,
    pub crs_wkt: String,
    /// When the raster is georeferenced.
    pub lon_lat_bounds: Option<CrsBox>,
}

/// A raster to be warped. Either a dataset that carries its own
/// georeferencing (a GeoTIFF, a COG over /vsicurl/), or an image a service
/// returned for a box it was asked for, in which case `bounds` and `crs` say
/// where it lies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WarpSource {
    /// A file path, or a GDAL virtual path (/vsicurl/https://...).
    pub path: String,
    pub bounds: Option<CrsBox>,
    /// With bounds: the CRS they are in.
    pub crs: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WarpOptions {
    pub target_crs: String,
    /// In target_crs.
    pub target_bounds: CrsBox,
    /// Target units per pixel, both axes.
    pub resolution: f64,
    /// Elevation: one Float32 band, no-data carried through, bilinear.
    /// Imagery: RGBA bytes, cubic, the alpha band marking where no source
    /// covered.
    pub elevation: bool,
    /// Refused before any pixel is read when the output would exceed this
    /// many pixels, with a message saying how to shrink the request.
    pub max_pixels: u64,
    /// Seconds per HTTP request for a /vsicurl/ source, and the User-Agent,
    /// set for this thread only while the warp runs.
    pub timeout_seconds: i32,
    pub user_agent: String,
    /// For /vsicurl/ sources that 404 (a Copernicus tile over the ocean),
    /// skip the source rather than fail; at least one source must open.
    pub skip_missing_sources: bool,
}

impl Default for WarpOptions {
    fn default() -> Self {
        Self {
            target_crs: String::new(),
            target_bounds: CrsBox::default(),
            resolution: 1.0,
            elevation: false,
            max_pixels: 64 * 1024 * 1024,
            timeout_seconds: 120,
            user_agent: String::new(),
            skip_missing_sources: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WarpResult {
    pub width: i32,
    pub height: i32,
    pub sources_used: i32,
    pub warnings: Vec<String>,
}

// ---- helpers ----------------------------------------------------------------------------------

/// `text.substr(0, bytes)`: a byte prefix, as the C++ cuts it; a character
/// it splits becomes U+FFFD.
fn prefix(text: &str, bytes: usize) -> String {
    String::from_utf8_lossy(&text.as_bytes()[..text.len().min(bytes)]).into_owned()
}

/// `snprintf("%.17g")`.
fn number(value: f64) -> String {
    format_general(value, 17)
}

fn last_error_message() -> String {
    // SAFETY: this thread's last error message, copied at once.
    unsafe { cpl::decoded(cpl::CPLGetLastErrorMsg()) }
}

/// `std::min` and `std::max` on doubles, as the C++ has them.
fn cpp_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn cpp_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// glibc's `atoi`, `(int) strtol(text, NULL, 10)`: leading space, a sign and
/// the digits after it; 0 for none, the long's range saturated, then cut to
/// an int as the cast does.
fn atoi(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() && is_ascii_space(bytes[at]) {
        at += 1;
    }
    let negative = bytes.get(at) == Some(&b'-');
    if matches!(bytes.get(at), Some(b'+' | b'-')) {
        at += 1;
    }
    let mut value: i64 = 0;
    while let Some(digit) = bytes.get(at).filter(|byte| byte.is_ascii_digit()) {
        let digit = i64::from(digit - b'0');
        value = if negative {
            value.saturating_mul(10).saturating_sub(digit)
        } else {
            value.saturating_mul(10).saturating_add(digit)
        };
        at += 1;
    }
    value as i32
}

/// The text of an I/O error as std::error_code::message() gives it: the
/// system's sentence, without Rust's " (os error N)".
fn error_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => String::from(&text[..at]),
        _ => text,
    }
}

/// Reads a CRS the way the rest of the crate does (ogr_detail::parse_crs):
/// with the network and file access that SetFromUserInput would otherwise
/// allow switched off, and in traditional GIS order - but the user's input
/// first and WKT second, as the C++ has it here.
fn read_crs(text: &str, reference: &mut SpatialReference) -> bool {
    reference.set_axis_mapping_strategy(osr::OAMS_TRADITIONAL_GIS_ORDER);
    if text.is_empty() {
        return false;
    }
    if reference.set_from_user_input(text, &osr::SET_FROM_USER_INPUT_LIMITATIONS)
        == ogr::OGRERR_NONE
    {
        return true;
    }
    reference.import_from_wkt(text) == ogr::OGRERR_NONE
}

fn crs_of(text: &str) -> Result<SpatialReference> {
    ensure_registered();
    let mut reference = SpatialReference::new();
    if !read_crs(text, &mut reference) {
        return Err(make_error(
            ErrorCode::InvalidCrs,
            "GDAL cannot read this coordinate system",
            prefix(text, 200),
        ));
    }
    Ok(reference)
}

/// An OGRCoordinateTransformation Rust owns.
struct Transform(NonNull<osr::OgrCoordinateTransformationHs>);

impl Transform {
    fn raw(&self) -> osr::OgrCoordinateTransformationH {
        self.0.as_ptr()
    }
}

impl Drop for Transform {
    fn drop(&mut self) {
        // SAFETY: the transformation is owned by this value alone, destroyed once.
        unsafe { osr::OCTDestroyCoordinateTransformation(self.raw()) };
    }
}

fn make_transform(from: &str, to: &str) -> Result<Transform> {
    let source = crs_of(from)?;
    let target = crs_of(to)?;
    let quiet = QuietHandler::push();
    // SAFETY: two live references, which the transformation copies; it is
    // owned by the Transform made below.
    let transform = unsafe { osr::OCTNewCoordinateTransformation(source.raw(), target.raw()) };
    drop(quiet);
    NonNull::new(transform).map(Transform).ok_or_else(|| {
        make_error(
            ErrorCode::InvalidCrs,
            "PROJ has no transformation between these systems",
            format!("{} -> {}", prefix(from, 100), prefix(to, 100)),
        )
    })
}

/// Sets GDAL configuration for the calling thread only and puts it back when
/// the scope ends, so a warp's User-Agent and timeouts cannot leak into
/// another thread's reads.
#[derive(Default)]
struct ThreadConfig {
    saved: Vec<(&'static CStr, Option<CString>)>,
    // Not Send: the options are this thread's.
    _thread: PhantomData<*const ()>,
}

impl ThreadConfig {
    fn set(&mut self, key: &'static CStr, value: &str) {
        // SAFETY: a NUL-terminated key and a NULL default.
        let previous = unsafe { cpl::CPLGetThreadLocalConfigOption(key.as_ptr(), ptr::null()) };
        // SAFETY: NULL or this thread's value, copied before it is replaced.
        let previous =
            (!previous.is_null()).then(|| CString::from(unsafe { CStr::from_ptr(previous) }));
        self.saved.push((key, previous));
        let value = cpl::c_string(value);
        // SAFETY: NUL-terminated key and value; GDAL copies both.
        unsafe { cpl::CPLSetThreadLocalConfigOption(key.as_ptr(), value.as_ptr()) };
    }
}

impl Drop for ThreadConfig {
    fn drop(&mut self) {
        for (key, previous) in self.saved.iter().rev() {
            let previous = previous
                .as_ref()
                .map_or(ptr::null(), |value| value.as_ptr());
            // SAFETY: a NUL-terminated key and NULL or a NUL-terminated value,
            // on the thread that set the option.
            unsafe { cpl::CPLSetThreadLocalConfigOption(key.as_ptr(), previous) };
        }
    }
}

/// What the warp's progress callback reads: shared with GDAL, which may call
/// from a thread of its own.
struct ProgressState<'a> {
    stop: Option<&'a AtomicBool>,
    progress: Option<&'a (dyn Fn(f64) + Sync)>,
    // A panic of the progress closure, raised again once GDAL has returned.
    panic: Mutex<Option<Box<dyn Any + Send>>>,
}

extern "C" fn warp_progress(fraction: f64, _message: *const c_char, data: *mut c_void) -> c_int {
    if data.is_null() {
        return cpl::TRUE;
    }
    // SAFETY: the state warp_to_geotiff handed GDAL, alive for the warp.
    let state = unsafe { &*data.cast::<ProgressState<'_>>() };
    let reported = catch_unwind(AssertUnwindSafe(|| {
        if let Some(progress) = state.progress {
            progress(fraction);
        }
    }));
    if let Err(payload) = reported {
        let mut panic = state
            .panic
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        panic.get_or_insert(payload);
        return cpl::FALSE;
    }
    if state.stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
        cpl::FALSE
    } else {
        cpl::TRUE
    }
}

/// A GDAL dataset Rust owns, closed on drop.
struct Dataset(NonNull<gdal::GdalDatasetHs>);

impl Dataset {
    fn from_raw(handle: gdal::GdalDatasetH) -> Option<Self> {
        NonNull::new(handle).map(Self)
    }

    fn raw(&self) -> gdal::GdalDatasetH {
        self.0.as_ptr()
    }
}

impl Drop for Dataset {
    fn drop(&mut self) {
        // SAFETY: the dataset is owned by this value alone, closed once.
        unsafe { gdal::GDALClose(self.raw()) };
    }
}

fn open_raster(path: &str) -> Option<Dataset> {
    let name = cpl::c_string(path);
    let quiet = QuietHandler::push();
    // SAFETY: a NUL-terminated name and no lists; the dataset is owned by
    // the Dataset returned.
    let opened = Dataset::from_raw(unsafe {
        gdal::GDALOpenEx(
            name.as_ptr(),
            gdal::GDAL_OF_RASTER | gdal::GDAL_OF_READONLY,
            ptr::null(),
            ptr::null(),
            ptr::null(),
        )
    });
    drop(quiet);
    opened
}

/// A source as the warp should see it: an image a service returned for a box
/// becomes an in-memory VRT carrying that box and CRS, and an imagery source
/// becomes four bands, red, green, blue and alpha, whatever it was - a
/// paletted PNG from an ArcGIS export, a grey JPEG, an RGB COG - because one
/// warp cannot mix band counts.
///
/// The VRT refers to the opened source rather than reopening it by name, so
/// the source must outlive it: it is handed to `keep_open`, which the caller
/// drops after the VRT (a VRT read after its source was closed crashed
/// GDAL's overview lookup, found by the file-URL test).
fn prepare_source(
    source: &WarpSource,
    elevation: bool,
    keep_open: &mut Vec<Dataset>,
) -> Result<Dataset> {
    let Some(opened) = open_raster(&source.path) else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "GDAL could not open a downloaded raster",
            last_error_message(),
        ));
    };
    let dataset = opened.raw();
    // SAFETY: a live dataset.
    let bands = unsafe { gdal::GDALGetRasterCount(dataset) };
    if bands < 1 {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            "a downloaded raster has no bands",
        ));
    }

    let mut arguments = cpl::CStringList::new();
    arguments.add("-of");
    arguments.add("VRT");
    if let Some(bounds) = &source.bounds {
        arguments.add("-a_ullr");
        arguments.add(number(bounds.min_x));
        arguments.add(number(bounds.max_y));
        arguments.add(number(bounds.max_x));
        arguments.add(number(bounds.min_y));
        let wkt = crs_to_wkt(&source.crs)?;
        arguments.add("-a_srs");
        arguments.add(wkt);
    }
    if elevation {
        arguments.add("-b");
        arguments.add("1");
        // An image a service returned for a box may not declare the no-data
        // it was asked to use (exportImage's noData=-32767): declared here,
        // so the warp does not blend it into real heights at the edges.
        let mut has_no_data: c_int = cpl::FALSE;
        // SAFETY: a live dataset's first band and an out flag.
        unsafe {
            gdal::GDALGetRasterNoDataValue(
                gdal::GDALGetRasterBand(dataset, 1),
                &raw mut has_no_data,
            )
        };
        if has_no_data == cpl::FALSE && source.bounds.is_some() {
            arguments.add("-a_nodata");
            arguments.add(number(ELEVATION_NO_DATA));
        }
    } else {
        // SAFETY: a live dataset and its bands.
        let (paletted, has_alpha) = unsafe {
            let first = gdal::GDALGetRasterBand(dataset, 1);
            (
                !gdal::GDALGetRasterColorTable(first).is_null(),
                bands >= 4
                    || (bands == 2
                        && gdal::GDALGetRasterColorInterpretation(gdal::GDALGetRasterBand(
                            dataset, 2,
                        )) == gdal::GCI_ALPHA_BAND),
            )
        };
        if paletted {
            arguments.add("-expand");
            arguments.add("rgba");
        } else {
            let red = 1;
            let green = if bands >= 3 { 2 } else { 1 };
            let blue = if bands >= 3 { 3 } else { 1 };
            for band in [red, green, blue] {
                arguments.add("-b");
                arguments.add(format!("{band}"));
            }
            arguments.add("-b");
            if has_alpha {
                arguments.add(if bands >= 4 { "4" } else { "2" });
            } else {
                // The dataset's mask: its no-data, or everything valid.
                arguments.add("mask");
            }
            arguments.add("-colorinterp");
            arguments.add("red,green,blue,alpha");
            // A 16-bit or float RGB is not display imagery; it is scaled by
            // build_true_colour_vrt before it gets here. Bytes are passed as
            // is.
            arguments.add("-ot");
            arguments.add("Byte");
        }
    }
    // SAFETY: an argument list GDAL copies; the options are freed below.
    let options = unsafe { gdal::GDALTranslateOptionsNew(arguments.as_argv(), ptr::null_mut()) };
    let quiet = QuietHandler::push();
    // SAFETY: an empty name (an in-memory VRT), a live source and the options
    // (NULL takes GDAL's defaults); the dataset is owned by the Dataset made.
    let prepared = Dataset::from_raw(unsafe {
        gdal::GDALTranslate(c"".as_ptr(), dataset, options, ptr::null_mut())
    });
    drop(quiet);
    // SAFETY: the options made above, freed once.
    unsafe { gdal::GDALTranslateOptionsFree(options) };
    let Some(prepared) = prepared else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "a downloaded raster could not be read",
            last_error_message(),
        ));
    };
    keep_open.push(opened);
    Ok(prepared)
}

// ---- coordinate systems -----------------------------------------------------------------------

/// The CRS as WKT2, from anything GDAL's SetFromUserInput reads with its
/// network and file access disabled ("EPSG:7856", "EPSG:4326", a WKT,
/// PROJJSON, "urn:ogc:def:crs:EPSG::3857", "CRS84"). InvalidCRS for text it
/// cannot read.
pub fn crs_to_wkt(crs: &str) -> Result<String> {
    let reference = crs_of(crs)?;
    reference
        .export_to_wkt_with(&[c"FORMAT=WKT2_2019"])
        .ok_or_else(|| {
            make_error(
                ErrorCode::InvalidCrs,
                "GDAL could not write this coordinate system",
                prefix(crs, 200),
            )
        })
}

/// True when the CRS's authority defines its first axis as northing or
/// latitude - EPSG:4326, EPSG:4283, EPSG:7844 and most geographic systems,
/// and a few projected ones - so a protocol that follows the authority (WMS
/// 1.3.0, WFS 2.0, WMTS TopLeftCorner) must be given (y, x). False for CRS84
/// and every easting-first projection. InvalidCRS for an unreadable CRS.
pub fn crs_axis_is_yx(crs: &str) -> Result<bool> {
    let reference = crs_of(crs)?;
    // The AUTHORITY's order, which a traditional-GIS-order mapping hides: the
    // data axis mapping says where the CRS's first axis goes, and a swap means
    // the CRS itself is northing/latitude first.
    let mapping = reference.data_axis_to_srs_axis_mapping();
    Ok(mapping.len() >= 2 && mapping[0] == 2 && mapping[1] == 1)
}

pub fn crs_is_geographic(crs: &str) -> Result<bool> {
    let reference = crs_of(crs)?;
    Ok(reference.is_geographic())
}

/// The EPSG code GDAL identifies for the CRS, when it can.
pub fn crs_epsg_code(crs: &str) -> Option<i32> {
    let mut reference = crs_of(crs).ok()?;
    if reference.auto_identify_epsg() != ogr::OGRERR_NONE {
        // AutoIdentifyEPSG knows the common cases only; a WKT that
        // FindMatches ties to one EPSG entry with full confidence counts too.
        let matches = reference.find_matches();
        if matches.entries() >= 1
            && matches.confidence(0) == Some(100)
            && let (Some(authority), Some(value)) =
                (matches.authority_name(0), matches.authority_code(0))
            && authority == "EPSG"
        {
            return Some(atoi(&value));
        }
        return None;
    }
    let authority = reference.authority_name()?;
    let value = reference.authority_code()?;
    if authority != "EPSG" {
        return None;
    }
    Some(atoi(&value))
}

/// `crs_box` from one CRS to another, densified along its edges (21 points
/// per edge, GDAL's own default) so a box whose edges curve in the target is
/// still enclosed - a straight-edged box in MGA is not straight in Web
/// Mercator.
pub fn transform_box(crs_box: &CrsBox, from_crs: &str, to_crs: &str) -> Result<CrsBox> {
    if !crs_box.valid() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the area is empty or inverted",
            format!(
                "{},{} {},{}",
                number(crs_box.min_x),
                number(crs_box.min_y),
                number(crs_box.max_x),
                number(crs_box.max_y)
            ),
        ));
    }
    let transform = make_transform(from_crs, to_crs)?;
    let mut out = CrsBox::default();
    let quiet = QuietHandler::push();
    // SAFETY: a live transformation and four out values.
    let ok = unsafe {
        osr::OCTTransformBounds(
            transform.raw(),
            crs_box.min_x,
            crs_box.min_y,
            crs_box.max_x,
            crs_box.max_y,
            &raw mut out.min_x,
            &raw mut out.min_y,
            &raw mut out.max_x,
            &raw mut out.max_y,
            21,
        )
    };
    drop(quiet);
    if ok == cpl::FALSE || !out.min_x.is_finite() || !out.max_y.is_finite() || !out.valid() {
        return Err(make_error(
            ErrorCode::InvalidCrs,
            "the area cannot be expressed in the service's coordinate system",
            last_error_message(),
        ));
    }
    Ok(out)
}

/// One point, for tests and for reporting where something landed.
pub fn transform_point(x: f64, y: f64, from_crs: &str, to_crs: &str) -> Result<[f64; 2]> {
    let transform = make_transform(from_crs, to_crs)?;
    let mut px = x;
    let mut py = y;
    // SAFETY: a live transformation, one x and one y, and no z.
    let ok = unsafe {
        osr::OCTTransform(
            transform.raw(),
            1,
            &raw mut px,
            &raw mut py,
            ptr::null_mut(),
        )
    };
    if ok == cpl::FALSE || !px.is_finite() || !py.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidCrs,
            "the point cannot be transformed",
            format!("{},{}", number(x), number(y)),
        ));
    }
    Ok([px, py])
}

/// Features moved from one CRS to another, vertex by vertex, heights
/// untouched. A vertex PROJ cannot transform (outside the target's domain)
/// fails the whole call with InvalidCRS naming the feature, rather than
/// leaving it where it was.
pub fn reproject_features(
    mut features: Vec<VectorFeature>,
    from_crs: &str,
    to_crs: &str,
) -> Result<Vec<VectorFeature>> {
    let transform = make_transform(from_crs, to_crs)?;
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for (index, feature) in features.iter_mut().enumerate() {
        for part in &mut feature.geometry.parts {
            xs.clear();
            xs.extend(part.iter().map(|point| point.x));
            ys.clear();
            ys.extend(part.iter().map(|point| point.y));
            let mut success = vec![cpl::FALSE; part.len()];
            // A part GDAL's int count cannot hold is cut to the int's range;
            // the vertices beyond it stay unconverted and refuse below.
            let count = c_int::try_from(part.len()).unwrap_or(c_int::MAX);
            // SAFETY: a live transformation and `count` x, y and success
            // values; no z.
            unsafe {
                osr::OCTTransformEx(
                    transform.raw(),
                    count,
                    xs.as_mut_ptr(),
                    ys.as_mut_ptr(),
                    ptr::null_mut(),
                    success.as_mut_ptr(),
                )
            };
            for (i, point) in part.iter_mut().enumerate() {
                if success[i] == cpl::FALSE || !xs[i].is_finite() || !ys[i].is_finite() {
                    return Err(make_error(
                        ErrorCode::InvalidCrs,
                        "a feature lies outside where the project's coordinate system is defined",
                        format!(
                            "feature {} vertex {},{}",
                            index + 1,
                            number(point.x),
                            number(point.y)
                        ),
                    ));
                }
                point.x = xs[i];
                point.y = ys[i];
            }
        }
    }
    Ok(features)
}

pub fn probe_raster(path: &str, user_agent: &str, timeout_seconds: i32) -> Result<RasterProbe> {
    ensure_registered();
    let mut config = ThreadConfig::default();
    if !user_agent.is_empty() {
        config.set(c"GDAL_HTTP_USERAGENT", user_agent);
    }
    config.set(c"GDAL_HTTP_TIMEOUT", &format!("{}", timeout_seconds.max(1)));
    config.set(c"GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR");
    let Some(opened) = open_raster(path) else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "GDAL could not open the raster",
            last_error_message(),
        ));
    };
    let dataset = opened.raw();
    // SAFETY: a live dataset.
    let (width, height, band_count) = unsafe {
        (
            gdal::GDALGetRasterXSize(dataset),
            gdal::GDALGetRasterYSize(dataset),
            gdal::GDALGetRasterCount(dataset),
        )
    };
    let data_type = if band_count > 0 {
        // SAFETY: a live dataset's first band; the name is GDAL's static
        // text, copied at once.
        unsafe {
            cpl::decoded(gdal::GDALGetDataTypeName(gdal::GDALGetRasterDataType(
                gdal::GDALGetRasterBand(dataset, 1),
            )))
        }
    } else {
        String::new()
    };
    // SAFETY: a live dataset; its spatial reference is its own.
    let reference = unsafe { gdal::GDALGetSpatialRef(dataset) };
    let crs_wkt = if reference.is_null() {
        String::new()
    } else {
        // SAFETY: a live spatial reference the dataset owns.
        unsafe { exported_wkt(reference) }.unwrap_or_default()
    };
    let mut geotransform = [0.0_f64; 6];
    let mut lon_lat_bounds = None;
    // SAFETY: a live dataset and six doubles.
    if !crs_wkt.is_empty()
        && unsafe { gdal::GDALGetGeoTransform(dataset, geotransform.as_mut_ptr()) } == cpl::CE_NONE
    {
        let x0 = geotransform[0];
        let x1 = geotransform[0] + geotransform[1] * f64::from(width);
        let y0 = geotransform[3];
        let y1 = geotransform[3] + geotransform[5] * f64::from(height);
        let native = CrsBox {
            min_x: cpp_min(x0, x1),
            max_x: cpp_max(x0, x1),
            min_y: cpp_min(y0, y1),
            max_y: cpp_max(y0, y1),
        };
        if let Ok(bounds) = transform_box(&native, &crs_wkt, "OGC:CRS84") {
            lon_lat_bounds = Some(bounds);
        }
    }
    Ok(RasterProbe {
        width,
        height,
        band_count,
        data_type,
        crs_wkt,
        lon_lat_bounds,
    })
}

// ---- warping ----------------------------------------------------------------------------------

/// Warps and mosaics `sources` into a GeoTIFF at `output` covering
/// options.target_bounds at options.resolution in options.target_crs.
/// Progress in [0, 1] is reported through `progress`, and `stop` cancels
/// between GDAL's chunks (InvalidState, "cancelled", and the partial file
/// removed).
#[allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(resolution > 0), which refuses a NaN resolution too."
)]
pub fn warp_to_geotiff(
    sources: &[WarpSource],
    options: &WarpOptions,
    output: &Path,
    stop: Option<&AtomicBool>,
    progress: Option<&(dyn Fn(f64) + Sync)>,
) -> Result<WarpResult> {
    ensure_registered();
    let stopped = || stop.is_some_and(|stop| stop.load(Ordering::Acquire));
    if sources.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is nothing to warp",
        ));
    }
    if !options.target_bounds.valid() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the area is empty or inverted",
        ));
    }
    if !(options.resolution > 0.0) || !options.resolution.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the ground resolution must be positive",
            number(options.resolution),
        ));
    }
    let columns = (options.target_bounds.width() / options.resolution).ceil();
    let rows = (options.target_bounds.height() / options.resolution).ceil();
    if columns * rows > options.max_pixels as f64 {
        let coarser = options.resolution * (columns * rows / options.max_pixels as f64).sqrt();
        // The C++ writes this into a buffer of 256: at most 255 characters,
        // all of them ASCII.
        let mut advice = format!(
            "the result would be {} x {} pixels, more than the {} allowed; use a resolution of {} \
             or coarser, or a smaller area",
            format_fixed(columns, 0),
            format_fixed(rows, 0),
            options.max_pixels,
            format_general((coarser * 1000.0).ceil() / 1000.0, 3)
        );
        advice.truncate(255);
        return Err(Error::new(ErrorCode::InvalidArgument, advice));
    }
    let target_wkt = crs_to_wkt(&options.target_crs)?;

    let mut config = ThreadConfig::default();
    if !options.user_agent.is_empty() {
        config.set(c"GDAL_HTTP_USERAGENT", &options.user_agent);
    }
    config.set(
        c"GDAL_HTTP_TIMEOUT",
        &format!("{}", options.timeout_seconds.max(1)),
    );
    config.set(c"GDAL_HTTP_CONNECTTIMEOUT", "20");
    config.set(c"GDAL_HTTP_MAX_RETRY", "3");
    config.set(c"GDAL_HTTP_RETRY_DELAY", "1");
    // A COG is one file: listing its directory to look for sidecars costs a
    // request per open and finds nothing.
    config.set(c"GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR");
    config.set(
        c"CPL_VSIL_CURL_ALLOWED_EXTENSIONS",
        ".tif,.tiff,.TIF,.vrt,.jp2",
    );

    let mut result = WarpResult::default();
    // Declared first, dropped last: the prepared VRTs read from these.
    let mut opened: Vec<Dataset> = Vec::new();
    let mut prepared: Vec<Dataset> = Vec::new();
    for source in sources {
        if stopped() {
            return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
        }
        match prepare_source(source, options.elevation, &mut opened) {
            Ok(dataset) => prepared.push(dataset),
            Err(error) => {
                if options.skip_missing_sources {
                    result.warnings.push(format!(
                        "skipped a source that could not be opened: {}",
                        error.context
                    ));
                    continue;
                }
                return Err(error);
            }
        }
    }
    if prepared.is_empty() {
        return Err(Error::new(ErrorCode::NotFound, "no source covers the area"));
    }
    result.sources_used = prepared.len() as i32;

    let mut arguments = cpl::CStringList::new();
    for argument in ["-of", "GTiff", "-t_srs"] {
        arguments.add(argument);
    }
    arguments.add(&target_wkt);
    arguments.add("-te");
    arguments.add(number(options.target_bounds.min_x));
    arguments.add(number(options.target_bounds.min_y));
    arguments.add(number(
        options.target_bounds.min_x + columns * options.resolution,
    ));
    arguments.add(number(
        options.target_bounds.min_y + rows * options.resolution,
    ));
    arguments.add("-ts");
    // The pixel budget above bounds both, so the casts do not saturate.
    arguments.add(format!("{}", columns as i64));
    arguments.add(format!("{}", rows as i64));
    for argument in ["-co", "COMPRESS=DEFLATE", "-co", "TILED=YES", "-overwrite"] {
        arguments.add(argument);
    }
    if options.elevation {
        for argument in ["-ot", "Float32", "-r", "bilinear", "-dstnodata"] {
            arguments.add(argument);
        }
        arguments.add(number(ELEVATION_NO_DATA));
    } else {
        for argument in ["-r", "cubic", "-srcalpha", "-dstalpha"] {
            arguments.add(argument);
        }
    }
    // SAFETY: an argument list GDAL copies; the options are freed below.
    let warp_options = unsafe { gdal::GDALWarpAppOptionsNew(arguments.as_argv(), ptr::null_mut()) };
    let state = ProgressState {
        stop,
        progress,
        panic: Mutex::new(None),
    };
    if !warp_options.is_null() {
        // SAFETY: live options, a callback that catches every panic, and the
        // state, alive until the options are freed after the warp.
        unsafe {
            gdal::GDALWarpAppOptionsSetProgress(
                warp_options,
                Some(warp_progress),
                ptr::from_ref(&state).cast_mut().cast(),
            );
        }
    }

    let mut handles: Vec<gdal::GdalDatasetH> = prepared.iter().map(Dataset::raw).collect();
    let target = cpl::c_string(cpl::native_path(output));
    if let Some(parent) = output.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut usage_error: c_int = cpl::FALSE;
    let quiet = {
        // SAFETY: clears this thread's last error.
        unsafe { cpl::CPLErrorReset() };
        QuietHandler::push()
    };
    // SAFETY: a NUL-terminated name, no destination dataset, the live source
    // handles, the options and an out flag; the dataset returned is closed
    // below.
    let warped = unsafe {
        gdal::GDALWarp(
            target.as_ptr(),
            ptr::null_mut(),
            handles.len() as c_int,
            handles.as_mut_ptr(),
            warp_options,
            &raw mut usage_error,
        )
    };
    drop(quiet);
    // SAFETY: the options made above (or NULL), freed once.
    unsafe { gdal::GDALWarpAppOptionsFree(warp_options) };
    let warp_error = last_error_message();
    let close = |dataset: gdal::GdalDatasetH| {
        if !dataset.is_null() {
            // SAFETY: the warped dataset, closed once.
            unsafe { gdal::GDALClose(dataset) };
        }
    };
    if let Some(payload) = state
        .panic
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
    {
        close(warped);
        remove_path(output);
        resume_unwind(payload);
    }
    if warped.is_null() || stopped() {
        close(warped);
        remove_path(output);
        if stopped() {
            return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
        }
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "GDAL could not warp the downloaded data",
            warp_error,
        ));
    }
    // SAFETY: the warped dataset, read and then closed once.
    unsafe {
        result.width = gdal::GDALGetRasterXSize(warped);
        result.height = gdal::GDALGetRasterYSize(warped);
    }
    close(warped);
    Ok(result)
}

/// A number no other run of this process has made: the partial file's
/// suffix, as the C++'s `std::random_device{}()`. std has no random device,
/// and each RandomState is seeded apart from every other in the process.
fn random_suffix() -> u32 {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.finish() as u32
}

/// Writes the features of every layer of `input` clipped to `crs_box` (in
/// the input's own CRS) to a GeoPackage at `output` - GDAL's VectorTranslate
/// with -clipsrc, so a line or polygon crossing the box is cut at its edge
/// rather than kept whole. For a file that covers the world (Natural Earth's
/// coastline is one feature per landmass): without the clip an area around
/// Sydney imported the whole east coast of Australia.
pub fn clip_vector_file(input: &Path, crs_box: &CrsBox, output: &Path) -> Status {
    ensure_registered();
    if !crs_box.valid() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the clip box is empty or inverted",
        ));
    }
    let name = cpl::c_string(cpl::native_path(input));
    let quiet = QuietHandler::push();
    // SAFETY: a NUL-terminated name and no lists; the dataset is owned by the
    // Dataset made.
    let source = Dataset::from_raw(unsafe {
        gdal::GDALOpenEx(
            name.as_ptr(),
            gdal::GDAL_OF_VECTOR | gdal::GDAL_OF_READONLY,
            ptr::null(),
            ptr::null(),
            ptr::null(),
        )
    });
    drop(quiet);
    let Some(source) = source else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "GDAL could not open the downloaded file",
            last_error_message(),
        ));
    };
    let mut arguments = cpl::CStringList::new();
    arguments.add("-f");
    arguments.add("GPKG");
    arguments.add("-clipsrc");
    arguments.add(number(crs_box.min_x));
    arguments.add(number(crs_box.min_y));
    arguments.add(number(crs_box.max_x));
    arguments.add(number(crs_box.max_y));
    // A polygon cut by the box can become several; keep each part's type
    // general rather than refuse the layer.
    arguments.add("-nlt");
    arguments.add("PROMOTE_TO_MULTI");
    arguments.add("-skipfailures");
    // SAFETY: an argument list GDAL copies; the options are freed below.
    let options =
        unsafe { gdal::GDALVectorTranslateOptionsNew(arguments.as_argv(), ptr::null_mut()) };
    if let Some(parent) = output.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Written under a name of its own and renamed into place, as the cache's
    // other files are (online_fetch, writeAtomically): the output's name is a
    // hash of the file and the area, so two imports of the same area - two
    // windows, or two tests run side by side - name the same file, and on
    // Windows the second writer found it held open by the first and failed.
    let mut partial = output.as_os_str().to_owned();
    partial.push(format!(".part-{}.gpkg", random_suffix()));
    let partial = PathBuf::from(partial);
    let partial_name = cpl::c_string(cpl::native_path(&partial));
    let mut sources = [source.raw()];
    let mut usage_error: c_int = cpl::FALSE;
    let quiet = {
        // SAFETY: clears this thread's last error.
        unsafe { cpl::CPLErrorReset() };
        QuietHandler::push()
    };
    // SAFETY: a NUL-terminated name, no destination dataset, one live source,
    // the options and an out flag; the dataset is owned by the Dataset made.
    let clipped = Dataset::from_raw(unsafe {
        gdal::GDALVectorTranslate(
            partial_name.as_ptr(),
            ptr::null_mut(),
            1,
            sources.as_mut_ptr(),
            options,
            &raw mut usage_error,
        )
    });
    drop(quiet);
    // SAFETY: the options made above (or NULL), freed once.
    unsafe { gdal::GDALVectorTranslateOptionsFree(options) };
    let Some(clipped) = clipped else {
        remove_path(&partial);
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "GDAL could not clip the downloaded file",
            last_error_message(),
        ));
    };
    drop(clipped); // closed, so the rename can move it
    if let Err(renamed) = std::fs::rename(&partial, output) {
        remove_path(&partial);
        // Another import of the same area got there first: its file is the
        // same clip, and it is the one to read.
        if !output.is_file() {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "could not keep the clipped file",
                error_message(&renamed),
            ));
        }
    }
    Ok(())
}

/// A virtual raster (GDAL VRT, written to `output`) stacking three
/// single-band rasters - Sentinel-2's red, green and blue reflectance, 0 to
/// about 10000 - as one 8-bit RGB image, each band's `low`..`high` scaled
/// onto 1..255 (a value outside it clamped to the end, never to 0) and 0 kept
/// for no-data: the true-colour composite a person expects to see. No pixel
/// is read here; the VRT is a warp source like any other, so only the part
/// of each band the warp needs is ever fetched.
#[allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(high > low), which refuses a NaN bound too."
)]
pub fn build_true_colour_vrt(
    band_paths: &[String],
    low: f64,
    high: f64,
    output: &Path,
    user_agent: &str,
    timeout_seconds: i32,
) -> Status {
    ensure_registered();
    // The bands are opened over HTTP here, so the warp's limits apply: a
    // stalled connection must end, and each open is one request, not a
    // directory listing first.
    let mut config = ThreadConfig::default();
    if !user_agent.is_empty() {
        config.set(c"GDAL_HTTP_USERAGENT", user_agent);
    }
    config.set(c"GDAL_HTTP_TIMEOUT", &format!("{}", timeout_seconds.max(1)));
    config.set(c"GDAL_HTTP_CONNECTTIMEOUT", "20");
    config.set(c"GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR");
    if band_paths.len() != 3 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a true-colour composite needs three bands",
            format!("{}", band_paths.len()),
        ));
    }
    if !(high > low) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the stretch's high must exceed its low",
        ));
    }
    let mut names = cpl::CStringList::new();
    for path in band_paths {
        names.add(path);
    }
    let mut stack_arguments = cpl::CStringList::new();
    stack_arguments.add("-separate");
    // SAFETY: an argument list GDAL copies; the options are freed below.
    let stack_options =
        unsafe { gdal::GDALBuildVRTOptionsNew(stack_arguments.as_argv(), ptr::null_mut()) };
    let quiet = QuietHandler::push();
    // SAFETY: an empty name (an in-memory VRT), three NUL-terminated source
    // names and the options; the dataset is owned by the Dataset made.
    let stacked = Dataset::from_raw(unsafe {
        gdal::GDALBuildVRT(
            c"".as_ptr(),
            3,
            ptr::null_mut(),
            names.as_ptr(),
            stack_options,
            ptr::null_mut(),
        )
    });
    drop(quiet);
    // SAFETY: the options made above (or NULL), freed once.
    unsafe { gdal::GDALBuildVRTOptionsFree(stack_options) };
    let Some(stacked) = stacked else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the three bands could not be stacked",
            last_error_message(),
        ));
    };
    let mut arguments = cpl::CStringList::new();
    for argument in ["-of", "VRT", "-ot", "Byte", "-scale"] {
        arguments.add(argument);
    }
    arguments.add(number(low));
    arguments.add(number(high));
    arguments.add("1");
    arguments.add("255");
    // With an exponent (of 1: still linear) gdal_translate CLAMPS a value
    // outside low..high to the ends of the output range. Without it a value
    // below `low` - dark water - came out 0, the no-data value, and the
    // harbour was transparent.
    for argument in [
        "-exponent",
        "1",
        "-a_nodata",
        "0",
        "-colorinterp",
        "red,green,blue",
    ] {
        arguments.add(argument);
    }
    // SAFETY: an argument list GDAL copies; the options are freed below.
    let options = unsafe { gdal::GDALTranslateOptionsNew(arguments.as_argv(), ptr::null_mut()) };
    if let Some(parent) = output.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let output_name = cpl::c_string(cpl::native_path(output));
    let quiet = QuietHandler::push();
    // SAFETY: a NUL-terminated name, the live stack and the options; the
    // dataset is owned by the Dataset made.
    let composite = Dataset::from_raw(unsafe {
        gdal::GDALTranslate(
            output_name.as_ptr(),
            stacked.raw(),
            options,
            ptr::null_mut(),
        )
    });
    drop(quiet);
    // SAFETY: the options made above (or NULL), freed once.
    unsafe { gdal::GDALTranslateOptionsFree(options) };
    if composite.is_none() {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the composite could not be written",
            last_error_message(),
        ));
    }
    Ok(())
}
