//! The pointcloud layer's native boundary: the C ABI of native/pdal_shim.cpp
//! (PDAL's primitives; PDAL has only a C++ interface) and the eight GDAL calls
//! of the PROJ search path, declared by hand from the headers of PDAL 2.10.1
//! and GDAL 3.13.2, the toolchain's - then safe wrappers over the shim, so that
//! point_cloud_engine is safe code that decides everything and PDAL is asked
//! only what it says.
//!
//! No binding crate (docs/rust.md, "Third-party libraries"): pdal-c is not in
//! either toolchain, and the GDAL calls are the eight the C++ makes. The
//! layouts below are asserted here and in the shim (`static_assert`), the GDAL
//! sizes and constants here and in native/ffi_check.c, so a header that
//! changes fails the build.
//!
//! Text from PDAL - a driver name, a WKT, an exception's what() - is decoded
//! here, a byte that is not UTF-8 becoming U+FFFD, where the C++ keeps the
//! bytes (docs/rust.md, "Where Rust differs").

#![allow(
    unsafe_code,
    reason = "The declarations of the PDAL shim's and GDAL's C APIs and the wrappers over the shim's; every unsafe block states why it is sound."
)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::mem::offset_of;
use std::ptr::{NonNull, null_mut};

// ---- GDAL (cpl_error.h, ogr_core.h, ogr_srs_api.h) ------------------------------------------

/// `CPLErr`, a C enumeration: a c_int alias, so a value Rust does not list is
/// no undefined behaviour.
pub(crate) type CplErr = c_int;
/// `CPLErrorNum`.
pub(crate) type CplErrorNum = c_int;
/// `OGRErr`.
pub(crate) type OgrErr = c_int;
pub(crate) const OGRERR_NONE: OgrErr = 0;
/// `OGRSpatialReferenceH`.
pub(crate) type OgrSpatialReferenceH = *mut c_void;
/// `CPLErrorHandler`. CPL_STDCALL is empty but under MSVC, which the port
/// does not build with, so the convention is C's.
pub(crate) type CplErrorHandler = Option<unsafe extern "C" fn(CplErr, CplErrorNum, *const c_char)>;

const _: () = assert!(size_of::<CplErr>() == 4);
const _: () = assert!(size_of::<CplErrorNum>() == 4);
const _: () = assert!(size_of::<OgrErr>() == 4);
const _: () = assert!(size_of::<OgrSpatialReferenceH>() == size_of::<*mut c_void>());
const _: () = assert!(size_of::<CplErrorHandler>() == size_of::<*mut c_void>());

#[link(name = "gdal")]
unsafe extern "C" {
    pub(crate) fn CPLPushErrorHandler(handler: CplErrorHandler);
    pub(crate) fn CPLQuietErrorHandler(class: CplErr, number: CplErrorNum, message: *const c_char);
    pub(crate) fn CPLPopErrorHandler();
    pub(crate) fn CPLErrorReset();
    pub(crate) fn OSRNewSpatialReference(wkt: *const c_char) -> OgrSpatialReferenceH;
    pub(crate) fn OSRImportFromEPSG(reference: OgrSpatialReferenceH, code: c_int) -> OgrErr;
    pub(crate) fn OSRDestroySpatialReference(reference: OgrSpatialReferenceH);
    pub(crate) fn OSRSetPROJSearchPaths(paths: *const *const c_char);
}

// ---- the shim (native/pdal_shim.cpp) --------------------------------------------------------

// The codes the shim's fallible functions return.
const KATANA_PDAL_OK: c_int = 0;
/// The factory made no stage of that driver.
const KATANA_PDAL_NO_STAGE: c_int = 1;
/// PDAL threw a std::exception; the message is its what(). The shim's 3 is
/// an exception of any other type, with no message.
const KATANA_PDAL_THREW: c_int = 2;

// A stage's role in a pipeline.
const KATANA_PDAL_READER: c_int = 0;
const KATANA_PDAL_FILTER: c_int = 1;
const KATANA_PDAL_WRITER: c_int = 2;

/// What the C++ engine's guarded() says of an exception that is not a
/// std::exception.
const UNKNOWN_FAILURE: &str = "unknown PDAL failure";

/// `struct katana_pdal_pipeline`: a pipeline that has run, the shim's.
#[repr(C)]
#[allow(
    dead_code,
    reason = "Opaque: the shim makes it, Rust only holds a pointer to it."
)]
struct KatanaPdalPipeline {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(
    dead_code,
    reason = "Rust writes these members and the shim reads them: the layout is the shim's, member for member."
)]
struct KatanaPdalOption {
    name: *const c_char,
    value: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(
    dead_code,
    reason = "Rust writes these members and the shim reads them: the layout is the shim's, member for member."
)]
struct KatanaPdalStage {
    role: c_int,
    driver: *const c_char,
    filename: *const c_char,
    options: *const KatanaPdalOption,
    option_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KatanaPdalQuickInfo {
    point_count: u64,
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
    bounds_valid: c_int,
    has_red: c_int,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KatanaPdalViewInfo {
    size: u64,
    is_null: c_int,
    has_red: c_int,
    has_green: c_int,
    has_blue: c_int,
    has_intensity: c_int,
    has_classification: c_int,
}

/// `struct katana_pdal_point`: one point as PDAL's getFieldAs reads it and
/// setField writes it, the colour in LAS's 16 bits.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct KatanaPdalPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    pub(crate) intensity: f64,
    pub(crate) red: u16,
    pub(crate) green: u16,
    pub(crate) blue: u16,
    pub(crate) classification: u8,
}

// The layouts above, as the shim asserts them: two pointers; on a 64-bit
// target an int, padding, three pointers and a size_t; a u64, six doubles
// and two ints; a u64 and six ints; four doubles, three u16 and a u8, padded
// to the doubles' alignment.
const _: () = assert!(size_of::<KatanaPdalOption>() == 2 * size_of::<*const c_char>());
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<KatanaPdalStage>() == 40);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(offset_of!(KatanaPdalStage, driver) == 8);
const _: () = assert!(size_of::<KatanaPdalQuickInfo>() == 64);
const _: () = assert!(offset_of!(KatanaPdalQuickInfo, bounds_valid) == 56);
const _: () = assert!(size_of::<KatanaPdalViewInfo>() == 32);
const _: () = assert!(offset_of!(KatanaPdalViewInfo, is_null) == 8);
const _: () = assert!(size_of::<KatanaPdalPoint>() == 40);
const _: () = assert!(offset_of!(KatanaPdalPoint, red) == 32);
const _: () = assert!(offset_of!(KatanaPdalPoint, classification) == 38);
const _: () = assert!(size_of::<c_int>() == 4);

// Linked by build.rs: cc names the shim's archive, then PDAL and GDAL.
#[allow(clippy::too_many_arguments, reason = "The signatures are the shim's.")]
unsafe extern "C" {
    fn katana_pdal_free(memory: *mut c_void);
    fn katana_pdal_version() -> *const c_char;
    fn katana_pdal_infer_driver(
        filename: *const c_char,
        for_writing: c_int,
        driver: *mut *mut c_char,
        message: *mut *mut c_char,
    ) -> c_int;
    fn katana_pdal_preview(
        driver: *const c_char,
        options: *const KatanaPdalOption,
        option_count: usize,
        details: c_int,
        info: *mut KatanaPdalQuickInfo,
        srs_wkt: *mut *mut c_char,
        message: *mut *mut c_char,
    ) -> c_int;
    fn katana_pdal_pipeline_run(
        stages: *const KatanaPdalStage,
        stage_count: usize,
        pipeline: *mut *mut KatanaPdalPipeline,
        view_count: *mut usize,
        message: *mut *mut c_char,
    ) -> c_int;
    fn katana_pdal_pipeline_free(pipeline: *mut KatanaPdalPipeline);
    fn katana_pdal_view_info(
        pipeline: *const KatanaPdalPipeline,
        view_index: usize,
        info: *mut KatanaPdalViewInfo,
        message: *mut *mut c_char,
    ) -> c_int;
    fn katana_pdal_view_points(
        pipeline: *const KatanaPdalPipeline,
        view_index: usize,
        first: u64,
        count: u64,
        read_intensity: c_int,
        read_classification: c_int,
        read_colour: c_int,
        points: *mut KatanaPdalPoint,
        message: *mut *mut c_char,
    ) -> c_int;
    fn katana_pdal_view_srs(
        pipeline: *const KatanaPdalPipeline,
        view_index: usize,
        srs_wkt: *mut *mut c_char,
        message: *mut *mut c_char,
    ) -> c_int;
    fn katana_pdal_write(
        driver: *const c_char,
        options: *const KatanaPdalOption,
        option_count: usize,
        points: *const KatanaPdalPoint,
        point_count: usize,
        with_colour: c_int,
        message: *mut *mut c_char,
    ) -> c_int;
}

// ---- the wrappers ---------------------------------------------------------------------------

/// An exception PDAL threw, caught in the shim: its what(), decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Thrown(pub(crate) String);

/// A stage option as PDAL's Options holds it: a name and its value's text.
pub(crate) type StageOption = (&'static str, Vec<u8>);

/// Where a stage stands in a pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StageRole {
    Reader,
    Filter,
    Writer,
}

/// A stage of a pipeline, each taking the one before it as its input.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stage {
    pub(crate) role: StageRole,
    pub(crate) driver: String,
    /// The reader's input, the writer's output; not read for a filter.
    pub(crate) filename: Vec<u8>,
    pub(crate) options: Vec<StageOption>,
}

/// What a preview reported, in the C++ engine's terms.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Preview {
    pub(crate) point_count: u64,
    /// minx, miny, minz, maxx, maxy, maxz, when PDAL's box is valid.
    pub(crate) bounds: Option<[f64; 6]>,
    /// The spatial reference's WKT, empty for none.
    pub(crate) srs_wkt: String,
    /// The dimensions listed hold one named "Red".
    pub(crate) has_red: bool,
}

/// One view of a pipeline that has run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ViewInfo {
    /// A view PDAL gave as a null pointer, which the C++ passes over.
    pub(crate) is_null: bool,
    pub(crate) size: u64,
    pub(crate) has_red: bool,
    pub(crate) has_green: bool,
    pub(crate) has_blue: bool,
    pub(crate) has_intensity: bool,
    pub(crate) has_classification: bool,
}

/// The fields beyond X, Y and Z a read of points takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PointFields {
    pub(crate) intensity: bool,
    pub(crate) classification: bool,
    pub(crate) colour: bool,
}

/// The C string a std::string's c_str() hands PDAL: the text up to its first
/// NUL, where a C reader stops.
fn c_string(text: &[u8]) -> CString {
    let end = text
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(text.len());
    CString::new(&text[..end]).unwrap_or_default()
}

/// The options as C strings, kept alive while the entries point into them.
fn option_texts(options: &[StageOption]) -> Vec<(CString, CString)> {
    options
        .iter()
        .map(|(name, value)| (c_string(name.as_bytes()), c_string(value)))
        .collect()
}

fn option_entries(texts: &[(CString, CString)]) -> Vec<KatanaPdalOption> {
    texts
        .iter()
        .map(|(name, value)| KatanaPdalOption {
            name: name.as_ptr(),
            value: value.as_ptr(),
        })
        .collect()
}

/// A text the shim allocated, decoded and handed back to it.
///
/// SAFETY (for callers): `text` is NULL or an allocation of the shim's that
/// nothing has freed.
unsafe fn take_text(text: *mut c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract: a live NUL-terminated string.
    let decoded = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: the shim's malloc made it, and it is freed once, here.
    unsafe { katana_pdal_free(text.cast()) };
    decoded
}

/// A shim call's code as Rust's: the code for success or for no stage, the
/// exception otherwise. The message is freed either way.
///
/// SAFETY (for callers): `message` is what the call left in its message
/// out-parameter.
unsafe fn outcome(code: c_int, message: *mut c_char) -> Result<c_int, Thrown> {
    // SAFETY: the caller's contract.
    let text = unsafe { take_text(message) };
    match code {
        KATANA_PDAL_OK | KATANA_PDAL_NO_STAGE => Ok(code),
        KATANA_PDAL_THREW => Err(Thrown(text)),
        // 3, an exception that is not a std::exception; and a code the shim
        // never returns, which could only be a failure too.
        _ => Err(Thrown(String::from(UNKNOWN_FAILURE))),
    }
}

/// PDAL's version, pdal::pdalVersion.
pub(crate) fn version() -> String {
    // SAFETY: the shim returns a pointer to PDAL's static version string,
    // never freed.
    let text = unsafe { katana_pdal_version() };
    if text.is_null() {
        return String::new();
    }
    // SAFETY: a live NUL-terminated string of static storage.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

/// The reader (or, `for_writing`, the writer) PDAL infers from a file name;
/// empty for none.
pub(crate) fn infer_driver(filename: &[u8], for_writing: bool) -> Result<String, Thrown> {
    let filename = c_string(filename);
    let mut driver: *mut c_char = null_mut();
    let mut message: *mut c_char = null_mut();
    // SAFETY: a live NUL-terminated name; the out-parameters are this frame's,
    // which the shim fills with allocations of its own or NULL.
    let code = unsafe {
        katana_pdal_infer_driver(
            filename.as_ptr(),
            c_int::from(for_writing),
            &mut driver,
            &mut message,
        )
    };
    // SAFETY: what the call left there.
    let driver = unsafe { take_text(driver) };
    // SAFETY: the message the call left.
    unsafe { outcome(code, message) }?;
    Ok(driver)
}

/// A stage of `driver`, given `options`, asked for its preview(); with
/// `details`, what it reported. None when the factory made no such stage.
pub(crate) fn preview(
    driver: &str,
    options: &[StageOption],
    details: bool,
) -> Result<Option<Preview>, Thrown> {
    let driver = c_string(driver.as_bytes());
    let texts = option_texts(options);
    let entries = option_entries(&texts);
    let mut info = KatanaPdalQuickInfo::default();
    let mut wkt: *mut c_char = null_mut();
    let mut message: *mut c_char = null_mut();
    // SAFETY: the driver and every option are live NUL-terminated strings,
    // the entries point into `texts`, which outlives the call, and the
    // out-parameters are this frame's.
    let code = unsafe {
        katana_pdal_preview(
            driver.as_ptr(),
            entries.as_ptr(),
            entries.len(),
            c_int::from(details),
            &mut info,
            &mut wkt,
            &mut message,
        )
    };
    // SAFETY: what the call left there.
    let srs_wkt = unsafe { take_text(wkt) };
    // SAFETY: the message the call left.
    if unsafe { outcome(code, message) }? == KATANA_PDAL_NO_STAGE {
        return Ok(None);
    }
    Ok(Some(Preview {
        point_count: info.point_count,
        bounds: (info.bounds_valid != 0).then_some([
            info.min_x, info.min_y, info.min_z, info.max_x, info.max_y, info.max_z,
        ]),
        srs_wkt,
        has_red: info.has_red != 0,
    }))
}

/// A pipeline that has run, holding its views until it is dropped.
pub(crate) struct Pipeline {
    handle: NonNull<KatanaPdalPipeline>,
    views: usize,
}

/// The stages made in a pipeline, in order, and the pipeline executed.
pub(crate) fn run_pipeline(stages: &[Stage]) -> Result<Pipeline, Thrown> {
    let drivers: Vec<CString> = stages
        .iter()
        .map(|stage| c_string(stage.driver.as_bytes()))
        .collect();
    let filenames: Vec<CString> = stages
        .iter()
        .map(|stage| c_string(&stage.filename))
        .collect();
    let texts: Vec<Vec<(CString, CString)>> = stages
        .iter()
        .map(|stage| option_texts(&stage.options))
        .collect();
    let entries: Vec<Vec<KatanaPdalOption>> = texts
        .iter()
        .map(|texts| option_entries(texts.as_slice()))
        .collect();
    let native: Vec<KatanaPdalStage> = stages
        .iter()
        .zip(&drivers)
        .zip(&filenames)
        .zip(&entries)
        .map(|(((stage, driver), filename), options)| KatanaPdalStage {
            role: match stage.role {
                StageRole::Reader => KATANA_PDAL_READER,
                StageRole::Filter => KATANA_PDAL_FILTER,
                StageRole::Writer => KATANA_PDAL_WRITER,
            },
            driver: driver.as_ptr(),
            filename: filename.as_ptr(),
            options: options.as_ptr(),
            option_count: options.len(),
        })
        .collect();
    let mut handle: *mut KatanaPdalPipeline = null_mut();
    let mut views: usize = 0;
    let mut message: *mut c_char = null_mut();
    // SAFETY: every stage points into `drivers`, `filenames` and `entries`,
    // and every entry into `texts`, all of which outlive the call; the
    // out-parameters are this frame's.
    let code = unsafe {
        katana_pdal_pipeline_run(
            native.as_ptr(),
            native.len(),
            &mut handle,
            &mut views,
            &mut message,
        )
    };
    // SAFETY: the message the call left.
    unsafe { outcome(code, message) }?;
    match NonNull::new(handle) {
        Some(handle) => Ok(Pipeline { handle, views }),
        // The shim hands back a pipeline with every success.
        None => Err(Thrown(String::from(UNKNOWN_FAILURE))),
    }
}

impl Pipeline {
    pub(crate) fn view_count(&self) -> usize {
        self.views
    }

    pub(crate) fn view_info(&self, view: usize) -> Result<ViewInfo, Thrown> {
        let mut info = KatanaPdalViewInfo::default();
        let mut message: *mut c_char = null_mut();
        // SAFETY: a live pipeline of the shim's, owned by self; the
        // out-parameters are this frame's. An index out of range is the
        // shim's std::out_of_range, caught there.
        let code =
            unsafe { katana_pdal_view_info(self.handle.as_ptr(), view, &mut info, &mut message) };
        // SAFETY: the message the call left.
        unsafe { outcome(code, message) }?;
        Ok(ViewInfo {
            is_null: info.is_null != 0,
            size: info.size,
            has_red: info.has_red != 0,
            has_green: info.has_green != 0,
            has_blue: info.has_blue != 0,
            has_intensity: info.has_intensity != 0,
            has_classification: info.has_classification != 0,
        })
    }

    /// Points `first`, `first + 1` ... of a view into `points`, as many as it
    /// holds.
    pub(crate) fn view_points(
        &self,
        view: usize,
        first: u64,
        fields: PointFields,
        points: &mut [KatanaPdalPoint],
    ) -> Result<(), Thrown> {
        let mut message: *mut c_char = null_mut();
        // SAFETY: a live pipeline of the shim's; `points` has room for the
        // count the shim is told to write; the out-parameter is this frame's.
        let code = unsafe {
            katana_pdal_view_points(
                self.handle.as_ptr(),
                view,
                first,
                points.len() as u64,
                c_int::from(fields.intensity),
                c_int::from(fields.classification),
                c_int::from(fields.colour),
                points.as_mut_ptr(),
                &mut message,
            )
        };
        // SAFETY: the message the call left.
        unsafe { outcome(code, message) }?;
        Ok(())
    }

    /// The view's spatial reference as WKT, empty when it has none.
    pub(crate) fn view_srs(&self, view: usize) -> Result<String, Thrown> {
        let mut wkt: *mut c_char = null_mut();
        let mut message: *mut c_char = null_mut();
        // SAFETY: a live pipeline of the shim's; the out-parameters are this
        // frame's.
        let code =
            unsafe { katana_pdal_view_srs(self.handle.as_ptr(), view, &mut wkt, &mut message) };
        // SAFETY: what the call left there.
        let wkt = unsafe { take_text(wkt) };
        // SAFETY: the message the call left.
        unsafe { outcome(code, message) }?;
        Ok(wkt)
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        // SAFETY: the shim made the pipeline and nothing else frees it.
        unsafe { katana_pdal_pipeline_free(self.handle.as_ptr()) };
    }
}

/// The points written by a writer of `driver` given `options`; false when the
/// factory made no such writer.
pub(crate) fn write_points(
    driver: &str,
    options: &[StageOption],
    points: &[KatanaPdalPoint],
    with_colour: bool,
) -> Result<bool, Thrown> {
    let driver = c_string(driver.as_bytes());
    let texts = option_texts(options);
    let entries = option_entries(&texts);
    let mut message: *mut c_char = null_mut();
    // SAFETY: the driver and every option are live NUL-terminated strings,
    // the entries point into `texts`, the points are `points.len()` live
    // structs, all outliving the call; the out-parameter is this frame's.
    let code = unsafe {
        katana_pdal_write(
            driver.as_ptr(),
            entries.as_ptr(),
            entries.len(),
            points.as_ptr(),
            points.len(),
            c_int::from(with_colour),
            &mut message,
        )
    };
    // SAFETY: the message the call left.
    Ok(unsafe { outcome(code, message) }? == KATANA_PDAL_OK)
}
