//! INTERNAL to the geodesy crate: ownership of PROJ handles, capture of PROJ
//! diagnostics, and the calls into PROJ the public modules make. The port of
//! src/katana_geodesy/proj_internal.{hpp,cpp}. This module and `ffi` are the
//! only ones that touch PROJ: no PROJ type reaches the public interface, and
//! the other modules are safe code, but for the `unsafe impl Send` of the two
//! types that own a context.
//!
//! Every wrapper takes the owners (`ProjContext`, `PjPtr`, ...) rather than raw
//! pointers, so a handle handed to PROJ is always live, and copies every string
//! PROJ returns before the next call on its object can invalidate it.

#![allow(
    unsafe_code,
    reason = "Owns PROJ's handles and calls its C API; every unsafe block states why it is sound."
)]

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::mem::{MaybeUninit, offset_of};
use std::path::Path;
use std::ptr::{self, NonNull};
use std::sync::{Mutex, MutexGuard};

use katana_core::library_data::{bundled_proj_data_directory, proj_data_directory};
use katana_core::{Error, ErrorCode, Result};

use crate::coordinate::Coordinate;
use crate::coordinate_reference_system::AxisInfo;
use crate::coordinate_transformer::{GridFile, NamedCoordinate};
use crate::ffi;

// Upper bound of the captured log. A batch of millions of out-of-domain points
// makes PROJ log one line per point; only the first lines are diagnostic.
const MAX_LOG_BYTES: usize = 2048;

// Serialises the two calls that touch PROJ's process-wide default context.
// proj_info() fills a static PJ_INFO whose search-path text it frees on its
// next call, and it fills the default context's cached environment lazily;
// proj_context_create() copies that default context without PROJ's own lock.
// The C++ makes both calls with no guard, which is safe there only because no
// two threads happen to make them at once; Rust's test harness runs the cases
// of a suite on parallel threads of one process, so here they are guarded.
static DEFAULT_CONTEXT: Mutex<()> = Mutex::new(());

fn default_context_lock() -> MutexGuard<'static, ()> {
    DEFAULT_CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- owners ----------------------------------------------------------------------

/// Owns one `PJ`, destroyed on drop. A PJ must be destroyed before the context
/// it was made in; every owner of both declares the PJ first (Rust drops
/// fields in declaration order).
pub(crate) struct PjPtr(NonNull<ffi::Pj>);

impl PjPtr {
    fn from_raw(object: *mut ffi::Pj) -> Option<Self> {
        NonNull::new(object).map(Self)
    }

    fn raw(&self) -> *mut ffi::Pj {
        self.0.as_ptr()
    }
}

impl Drop for PjPtr {
    fn drop(&mut self) {
        // SAFETY: the PJ is owned by this value alone, and is destroyed once.
        unsafe { ffi::proj_destroy(self.raw()) };
    }
}

/// Owns one `PJ_OBJ_LIST`.
pub(crate) struct PjListPtr(NonNull<ffi::PjObjList>);

impl Drop for PjListPtr {
    fn drop(&mut self) {
        // SAFETY: the list is owned by this value alone, and is destroyed once.
        unsafe { ffi::proj_list_destroy(self.0.as_ptr()) };
    }
}

/// Owns one `PJ_OPERATION_FACTORY_CONTEXT`.
pub(crate) struct PjFactoryContextPtr(NonNull<ffi::PjOperationFactoryContext>);

impl Drop for PjFactoryContextPtr {
    fn drop(&mut self) {
        // SAFETY: the factory context is owned by this value alone, and is
        // destroyed once.
        unsafe { ffi::proj_operation_factory_context_destroy(self.0.as_ptr()) };
    }
}

/// Owns one `PJ_CONTEXT`. A PROJ context (and every PJ created in it) may be
/// used by one thread at a time, so each public object owns its own
/// `ProjContext` and nothing is shared or global.
///
/// PROJ's log output is routed into the context's diagnostic buffer instead of
/// stderr, and is attached to the `Error` returned to the caller.
pub(crate) struct ProjContext {
    context: *mut ffi::PjContext,
    // The log callback writes here. Boxed, so that the address handed to PROJ
    // stays put when the ProjContext moves; a RefCell, because PROJ calls back
    // while Rust holds the context by shared reference.
    log: Box<RefCell<String>>,
}

impl ProjContext {
    /// Creates a context with the network disabled (results must not depend
    /// on what happens to be downloadable) and verifies that proj.db can be
    /// opened. Fails with `NotFound` otherwise.
    pub(crate) fn create() -> Result<ProjContext> {
        let mut context = ProjContext {
            context: ptr::null_mut(),
            log: Box::new(RefCell::new(String::new())),
        };
        // In a relocated Linux prefix PROJ's own search path is wrong
        // (katana_core::library_data), and Katana points it at the data
        // beside libproj.
        let pointed = proj_data_directory();
        if context.open(pointed.as_deref()) {
            context.clear_diagnostics();
            return Ok(context);
        }
        if context.context.is_null() {
            return Err(Error::new(
                ErrorCode::Internal,
                "PROJ could not allocate a context",
            ));
        }

        // PROJ could not open proj.db where it looked. That is not always a
        // missing installation: PROJ_DATA or PROJ_LIB replaces the search
        // beside the library, and another GIS package (PostGIS, QGIS,
        // OSGeo4W) leaves one machine-wide naming data of its own, usually a
        // database of an older PROJ's layout, which this PROJ rejects. The
        // package carries its own, so fall back on it - a fresh context, so
        // that nothing of the failed attempt lingers in PROJ's error state.
        // (An explicit choice that WORKS is kept: this is reached only when it
        // does not.)
        let mut detail = format!("search paths: {}", library_info().search_path);
        let diagnostics = context.consume_diagnostics();
        if !diagnostics.is_empty() {
            detail = format!("{detail}; {diagnostics}");
        }
        let bundled = bundled_proj_data_directory();
        if let Some(bundled_path) = bundled
            && bundled != pointed
        {
            if context.open(Some(bundled_path.as_path())) {
                context.clear_diagnostics();
                return Ok(context);
            }
            if context.context.is_null() {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "PROJ could not allocate a context",
                ));
            }
            detail = format!(
                "{detail}; the data beside libproj, {}, did not open either",
                bundled_path.to_string_lossy()
            );
            let diagnostics = context.consume_diagnostics();
            if !diagnostics.is_empty() {
                detail = format!("{detail}: {diagnostics}");
            }
        }

        // Say what is wrong in the words that fit: an environment that names
        // data which does not open is not a missing installation, and telling
        // a person whose package carries PROJ's data to install it was the
        // wrong advice.
        let variable = if std::env::var_os("PROJ_DATA").is_some() {
            Some("PROJ_DATA")
        } else if std::env::var_os("PROJ_LIB").is_some() {
            Some("PROJ_LIB")
        } else {
            None
        };
        if let Some(variable) = variable {
            let value = std::env::var_os(variable).unwrap_or_default();
            return Err(Error::new(
                ErrorCode::NotFound,
                format!(
                    "PROJ resource database (proj.db) not usable: {variable} names {}, which \
                     holds no proj.db this PROJ can open; unset it, or point it at the data of \
                     this PROJ",
                    value.to_string_lossy()
                ),
            )
            .with_context(detail));
        }
        Err(Error::new(
            ErrorCode::NotFound,
            "PROJ resource database (proj.db) not found; install PROJ's data files or set the \
             PROJ_DATA environment variable",
        )
        .with_context(detail))
    }

    // Makes a fresh context - the old one, if any, is destroyed - looking for
    // proj.db in `data` when given, else where PROJ looks; true when proj.db
    // opens. What went wrong is left in the diagnostics.
    fn open(&mut self, data: Option<&Path>) -> bool {
        if !self.context.is_null() {
            // SAFETY: the context is this value's, and no PJ made in it is
            // alive: open() runs only inside create(), before any object.
            unsafe { ffi::proj_context_destroy(self.context) };
            self.context = ptr::null_mut();
        }
        self.log.borrow_mut().clear();
        self.context = {
            let _guard = default_context_lock();
            // SAFETY: no preconditions; the copy PROJ makes of its default
            // context is serialised against proj_info() by the guard.
            unsafe { ffi::proj_context_create() }
        };
        if self.context.is_null() {
            return false;
        }
        let app_data = ptr::from_ref::<RefCell<String>>(&self.log)
            .cast_mut()
            .cast::<c_void>();
        // SAFETY: the context was just created and is this value's. app_data
        // is the boxed log, which outlives the context: Drop destroys the
        // context before the box is freed.
        unsafe {
            ffi::proj_log_func(self.context, app_data, Some(log_callback));
            ffi::proj_log_level(self.context, ffi::PJ_LOG_ERROR);
            ffi::proj_context_set_enable_network(self.context, 0);
        }
        if let Some(data) = data {
            // Before proj.db is first opened below. UTF-8, as PROJ takes a
            // path.
            let path = c_string(&generic_utf8(data));
            let paths = [path.as_ptr()];
            // SAFETY: one live NUL-terminated string, which PROJ copies.
            unsafe { ffi::proj_context_set_search_paths(self.context, 1, paths.as_ptr()) };
        }
        // Forces PROJ to locate and open proj.db now, so that a broken
        // installation is reported as such instead of as a confusing "crs not
        // found" later.
        // SAFETY: a live context; the path it returns is only tested for NULL.
        !unsafe { ffi::proj_context_get_database_path(self.context) }.is_null()
    }

    fn raw(&self) -> *mut ffi::PjContext {
        self.context
    }

    /// PROJ's description of the latest failure: the context errno text
    /// followed by the log lines captured since the last call. Clears the log
    /// buffer. (PROJ offers no public way to reset a context errno, so call
    /// this only directly after a PROJ call that reported failure.)
    pub(crate) fn consume_diagnostics(&self) -> String {
        let mut text = String::new();
        // SAFETY: a live context (create() returns only a context that is).
        let code = unsafe { ffi::proj_context_errno(self.context) };
        if code != 0 {
            // SAFETY: a live context; the text is copied before the next call.
            let reason = unsafe { c_text(ffi::proj_context_errno_string(self.context, code)) };
            text = format!("PROJ error {code}: {reason}");
        }
        {
            let log = self.log.borrow();
            if !log.is_empty() {
                if !text.is_empty() {
                    text.push_str("; ");
                }
                text.push_str("PROJ log: ");
                text.push_str(&log);
            }
        }
        self.clear_diagnostics();
        text
    }

    pub(crate) fn clear_diagnostics(&self) {
        self.log.borrow_mut().clear();
    }
}

impl Drop for ProjContext {
    fn drop(&mut self) {
        if !self.context.is_null() {
            // SAFETY: the context is this value's; every PJ made in it has
            // been destroyed, as its owners drop it first.
            unsafe { ffi::proj_context_destroy(self.context) };
        }
    }
}

// PROJ's log function: appends the message to the context's log, up to
// MAX_LOG_BYTES. It must not unwind into PROJ, so nothing here panics: a log
// that is somehow borrowed already loses the line instead.
unsafe extern "C" fn log_callback(app_data: *mut c_void, _level: c_int, message: *const c_char) {
    if app_data.is_null() || message.is_null() {
        return;
    }
    // SAFETY: app_data is the boxed RefCell of the ProjContext that registered
    // this callback, alive for as long as its context is.
    let log = unsafe { &*app_data.cast::<RefCell<String>>() };
    let Ok(mut log) = log.try_borrow_mut() else {
        return;
    };
    if log.len() >= MAX_LOG_BYTES {
        return;
    }
    if !log.is_empty() {
        log.push_str(" | ");
    }
    // SAFETY: PROJ passes a NUL-terminated message, valid for the call.
    let text = unsafe { CStr::from_ptr(message) };
    log.push_str(&text.to_string_lossy());
}

// Null-safe copy of a C string PROJ returns. PROJ's text is UTF-8; a byte that
// is not becomes U+FFFD, the one difference from the C++'s std::string.
//
// SAFETY (for callers): `text` is NULL or a live NUL-terminated string.
unsafe fn c_text(text: *const c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract above.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

// The C string a std::string's c_str() hands PROJ: the text up to its first
// NUL, where PROJ would stop reading.
fn c_string(text: &[u8]) -> CString {
    let end = text
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(text.len());
    CString::new(&text[..end]).unwrap_or_default()
}

// The bytes of std::filesystem::path::generic_u8string(): the path's own bytes
// on POSIX; UTF-8 with '/' separators on Windows.
fn generic_utf8(path: &Path) -> Vec<u8> {
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

/// Shortens long definitions (WKT) for use inside `Error::context`. The C++
/// cuts at a byte; where that splits a UTF-8 sequence, the partial character
/// becomes U+FFFD.
pub(crate) fn abbreviate(text: &str) -> String {
    const MAX_CONTEXT_BYTES: usize = 240;
    if text.len() <= MAX_CONTEXT_BYTES {
        return text.to_owned();
    }
    format!(
        "{}...",
        String::from_utf8_lossy(&text.as_bytes()[..MAX_CONTEXT_BYTES])
    )
}

/// Instantiates `definition` and checks that it is a CRS. `InvalidCrs` on
/// failure, with PROJ's message in `Error::context`.
pub(crate) fn create_crs(context: &ProjContext, definition: &str) -> Result<PjPtr> {
    context.clear_diagnostics();
    let text = c_string(definition.as_bytes());
    // SAFETY: a live context and a NUL-terminated definition; the PJ returned
    // is owned by the PjPtr.
    let object = PjPtr::from_raw(unsafe { ffi::proj_create(context.raw(), text.as_ptr()) });
    let Some(object) = object else {
        return Err(Error::new(
            ErrorCode::InvalidCrs,
            "PROJ does not recognise the CRS definition",
        )
        .with_context(format!(
            "definition='{}'; {}",
            abbreviate(definition),
            context.consume_diagnostics()
        )));
    };
    if !is_crs(&object) {
        return Err(Error::new(
            ErrorCode::InvalidCrs,
            "the definition describes a PROJ object that is not a CRS",
        )
        .with_context(format!(
            "definition='{}'; object='{}'",
            abbreviate(definition),
            get_name(&object)
        )));
    }
    context.clear_diagnostics();
    Ok(object)
}

/// Text of a PROJ errno for a PJ that failed during proj_trans().
pub(crate) fn describe_errno(context: &ProjContext, proj_errno: c_int) -> String {
    // SAFETY: a live context; the text is copied before the next call.
    let reason = unsafe { c_text(ffi::proj_context_errno_string(context.raw(), proj_errno)) };
    format!("PROJ error {proj_errno}: {reason}")
}

/// What proj_info() reports of the library.
pub(crate) struct LibraryInfo {
    pub(crate) version: String,
    pub(crate) search_path: String,
}

pub(crate) fn library_info() -> LibraryInfo {
    let _guard = default_context_lock();
    // SAFETY: no preconditions. The strings it points at are PROJ's statics,
    // valid until the next proj_info() call, which the guard keeps out until
    // they have been copied.
    unsafe {
        let info = ffi::proj_info();
        LibraryInfo {
            version: c_text(info.version),
            search_path: c_text(info.searchpath),
        }
    }
}

/// Where the context found proj.db; empty when it found none.
pub(crate) fn database_path(context: &ProjContext) -> String {
    // SAFETY: a live context; the text is copied before the next call.
    unsafe { c_text(ffi::proj_context_get_database_path(context.raw())) }
}

// ---- objects ----------------------------------------------------------------------

pub(crate) fn get_type(object: &PjPtr) -> ffi::PjType {
    // SAFETY: a live PJ.
    unsafe { ffi::proj_get_type(object.raw()) }
}

pub(crate) fn get_name(object: &PjPtr) -> String {
    // SAFETY: a live PJ; the name it owns is copied at once.
    unsafe { c_text(ffi::proj_get_name(object.raw())) }
}

pub(crate) fn is_crs(object: &PjPtr) -> bool {
    // SAFETY: a live PJ.
    unsafe { ffi::proj_is_crs(object.raw()) != 0 }
}

pub(crate) fn get_id_auth_name(object: &PjPtr, index: c_int) -> Option<String> {
    // SAFETY: a live PJ; the text it owns is copied at once.
    unsafe {
        let authority = ffi::proj_get_id_auth_name(object.raw(), index);
        if authority.is_null() {
            None
        } else {
            Some(c_text(authority))
        }
    }
}

pub(crate) fn get_id_code(object: &PjPtr, index: c_int) -> String {
    // SAFETY: a live PJ; the text it owns is copied at once.
    unsafe { c_text(ffi::proj_get_id_code(object.raw(), index)) }
}

pub(crate) fn get_source_crs(context: &ProjContext, object: &PjPtr) -> Option<PjPtr> {
    // SAFETY: a live context and PJ; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe { ffi::proj_get_source_crs(context.raw(), object.raw()) })
}

pub(crate) fn crs_get_sub_crs(context: &ProjContext, crs: &PjPtr, index: c_int) -> Option<PjPtr> {
    // SAFETY: a live context and PJ; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe { ffi::proj_crs_get_sub_crs(context.raw(), crs.raw(), index) })
}

pub(crate) fn crs_get_coordinate_system(context: &ProjContext, crs: &PjPtr) -> Option<PjPtr> {
    // SAFETY: a live context and PJ; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe { ffi::proj_crs_get_coordinate_system(context.raw(), crs.raw()) })
}

pub(crate) fn crs_get_geodetic_crs(context: &ProjContext, crs: &PjPtr) -> Option<PjPtr> {
    // SAFETY: a live context and PJ; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe { ffi::proj_crs_get_geodetic_crs(context.raw(), crs.raw()) })
}

pub(crate) fn cs_get_axis_count(context: &ProjContext, system: &PjPtr) -> c_int {
    // SAFETY: a live context and PJ.
    unsafe { ffi::proj_cs_get_axis_count(context.raw(), system.raw()) }
}

/// One axis of a coordinate system: name, abbreviation, direction, unit name
/// and the unit's size in SI units; `None` when PROJ reports failure.
pub(crate) fn cs_get_axis_info(
    context: &ProjContext,
    system: &PjPtr,
    index: c_int,
) -> Option<AxisInfo> {
    let mut name: *const c_char = ptr::null();
    let mut abbreviation: *const c_char = ptr::null();
    let mut direction: *const c_char = ptr::null();
    let mut unit_name: *const c_char = ptr::null();
    let mut unit_to_si = 1.0;
    // SAFETY: a live context and PJ; every out-pointer is a local or NULL, and
    // the strings PROJ points them at, owned by the PJ, are copied at once.
    unsafe {
        let found = ffi::proj_cs_get_axis_info(
            context.raw(),
            system.raw(),
            index,
            &mut name,
            &mut abbreviation,
            &mut direction,
            &mut unit_to_si,
            &mut unit_name,
            ptr::null_mut(),
            ptr::null_mut(),
        ) != 0;
        if !found {
            return None;
        }
        Some(AxisInfo {
            name: c_text(name),
            abbreviation: c_text(abbreviation),
            direction: c_text(direction),
            unit_name: c_text(unit_name),
            unit_to_si,
        })
    }
}

/// The unit size of one axis alone, as the grid factors ask for it: written to
/// `unit_conversion_factor` when PROJ reports success.
pub(crate) fn cs_get_axis_unit_factor(
    context: &ProjContext,
    system: &PjPtr,
    index: c_int,
    unit_conversion_factor: &mut f64,
) -> bool {
    // SAFETY: a live context and PJ; the one out-pointer is a live f64.
    unsafe {
        ffi::proj_cs_get_axis_info(
            context.raw(),
            system.raw(),
            index,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            unit_conversion_factor,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        ) != 0
    }
}

/// The object as single-line WKT of `wkt_type`; `None` when PROJ cannot
/// express it so.
pub(crate) fn as_wkt(
    context: &ProjContext,
    object: &PjPtr,
    wkt_type: ffi::PjWktType,
) -> Option<String> {
    let options = [c"MULTILINE=NO".as_ptr(), ptr::null()];
    // SAFETY: a live context and PJ; a NULL-terminated option list of static
    // strings; the text PROJ returns, owned by the PJ, is copied at once.
    unsafe {
        let wkt = ffi::proj_as_wkt(context.raw(), object.raw(), wkt_type, options.as_ptr());
        if wkt.is_null() {
            None
        } else {
            Some(c_text(wkt))
        }
    }
}

/// The PROJ_5 pipeline text of an operation; empty when PROJ cannot express
/// it.
pub(crate) fn as_proj_string(context: &ProjContext, operation: &PjPtr) -> String {
    // SAFETY: a live context and PJ; the text, owned by the PJ, is copied at
    // once.
    unsafe {
        c_text(ffi::proj_as_proj_string(
            context.raw(),
            operation.raw(),
            ffi::PJ_PROJ_5,
            ptr::null(),
        ))
    }
}

/// The bounds (west, south, east, north, in degrees) of the object's area of
/// use, as PROJ writes them when it reports success.
pub(crate) fn get_area_of_use(context: &ProjContext, object: &PjPtr) -> Option<[f64; 4]> {
    let mut bounds = [0.0; 4];
    let [west, south, east, north] = &mut bounds;
    // SAFETY: a live context and PJ; the out-pointers are locals or NULL.
    let found = unsafe {
        ffi::proj_get_area_of_use(
            context.raw(),
            object.raw(),
            west,
            south,
            east,
            north,
            ptr::null_mut(),
        )
    } != 0;
    found.then_some(bounds)
}

/// The name of the object's area of use, when PROJ reports success.
pub(crate) fn get_area_of_use_name(context: &ProjContext, object: &PjPtr) -> Option<String> {
    let mut name: *const c_char = ptr::null();
    // SAFETY: a live context and PJ; the out-pointers are a local or NULL, and
    // the name PROJ points at, owned by the PJ, is copied at once.
    unsafe {
        let found = ffi::proj_get_area_of_use(
            context.raw(),
            object.raw(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut name,
        ) != 0;
        if found { Some(c_text(name)) } else { None }
    }
}

pub(crate) fn get_ellipsoid(context: &ProjContext, object: &PjPtr) -> Option<PjPtr> {
    // SAFETY: a live context and PJ; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe { ffi::proj_get_ellipsoid(context.raw(), object.raw()) })
}

/// The semi-major axis (metres) and inverse flattening of an ellipsoid.
pub(crate) fn ellipsoid_parameters(context: &ProjContext, ellipsoid: &PjPtr) -> Option<(f64, f64)> {
    let mut semi_major = 0.0;
    let mut inverse_flattening = 0.0;
    // SAFETY: a live context and PJ; the out-pointers are locals or NULL.
    let found = unsafe {
        ffi::proj_ellipsoid_get_parameters(
            context.raw(),
            ellipsoid.raw(),
            &mut semi_major,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut inverse_flattening,
        )
    } != 0;
    found.then_some((semi_major, inverse_flattening))
}

pub(crate) fn is_equivalent_to(
    context: &ProjContext,
    object: &PjPtr,
    other: &PjPtr,
    criterion: ffi::PjComparisonCriterion,
) -> bool {
    // SAFETY: a live context and two live PJs of it.
    unsafe {
        ffi::proj_is_equivalent_to_with_ctx(context.raw(), object.raw(), other.raw(), criterion)
            != 0
    }
}

// ---- coordinate operations --------------------------------------------------------

pub(crate) fn coordoperation_get_accuracy(context: &ProjContext, operation: &PjPtr) -> f64 {
    // SAFETY: a live context and PJ.
    unsafe { ffi::proj_coordoperation_get_accuracy(context.raw(), operation.raw()) }
}

pub(crate) fn coordoperation_has_ballpark_transformation(
    context: &ProjContext,
    operation: &PjPtr,
) -> bool {
    // SAFETY: a live context and PJ.
    unsafe {
        ffi::proj_coordoperation_has_ballpark_transformation(context.raw(), operation.raw()) != 0
    }
}

pub(crate) fn coordoperation_is_instantiable(context: &ProjContext, operation: &PjPtr) -> bool {
    // SAFETY: a live context and PJ.
    unsafe { ffi::proj_coordoperation_is_instantiable(context.raw(), operation.raw()) != 0 }
}

pub(crate) fn coordoperation_requires_per_coordinate_input_time(
    context: &ProjContext,
    operation: &PjPtr,
) -> bool {
    // SAFETY: a live context and PJ.
    unsafe {
        ffi::proj_coordoperation_requires_per_coordinate_input_time(context.raw(), operation.raw())
            != 0
    }
}

pub(crate) fn coordoperation_get_grid_used_count(
    context: &ProjContext,
    operation: &PjPtr,
) -> c_int {
    // SAFETY: a live context and PJ.
    unsafe { ffi::proj_coordoperation_get_grid_used_count(context.raw(), operation.raw()) }
}

/// One grid an operation uses: its short name, its download URL and whether
/// it is installed; `None` when PROJ reports failure.
pub(crate) fn coordoperation_get_grid_used(
    context: &ProjContext,
    operation: &PjPtr,
    index: c_int,
) -> Option<GridFile> {
    let mut short_name: *const c_char = ptr::null();
    let mut url: *const c_char = ptr::null();
    let mut available: c_int = 0;
    // SAFETY: a live context and PJ; the out-pointers are locals or NULL, and
    // the strings PROJ points them at, owned by the PJ, are copied at once.
    unsafe {
        let found = ffi::proj_coordoperation_get_grid_used(
            context.raw(),
            operation.raw(),
            index,
            &mut short_name,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut url,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut available,
        ) != 0;
        if !found {
            return None;
        }
        Some(GridFile {
            name: c_text(short_name),
            url: c_text(url),
            available: available != 0,
        })
    }
}

pub(crate) fn create_operation_factory_context(
    context: &ProjContext,
) -> Option<PjFactoryContextPtr> {
    // SAFETY: a live context; a NULL authority means any; the factory context
    // returned is owned by the PjFactoryContextPtr.
    NonNull::new(unsafe { ffi::proj_create_operation_factory_context(context.raw(), ptr::null()) })
        .map(PjFactoryContextPtr)
}

pub(crate) fn set_spatial_criterion(
    context: &ProjContext,
    factory: &PjFactoryContextPtr,
    criterion: ffi::ProjSpatialCriterion,
) {
    // SAFETY: a live context and factory context of it.
    unsafe {
        ffi::proj_operation_factory_context_set_spatial_criterion(
            context.raw(),
            factory.0.as_ptr(),
            criterion,
        );
    }
}

pub(crate) fn set_grid_availability_use(
    context: &ProjContext,
    factory: &PjFactoryContextPtr,
    grid_use: ffi::ProjGridAvailabilityUse,
) {
    // SAFETY: a live context and factory context of it.
    unsafe {
        ffi::proj_operation_factory_context_set_grid_availability_use(
            context.raw(),
            factory.0.as_ptr(),
            grid_use,
        );
    }
}

pub(crate) fn set_allow_ballpark_transformations(
    context: &ProjContext,
    factory: &PjFactoryContextPtr,
    allow: bool,
) {
    // SAFETY: a live context and factory context of it.
    unsafe {
        ffi::proj_operation_factory_context_set_allow_ballpark_transformations(
            context.raw(),
            factory.0.as_ptr(),
            c_int::from(allow),
        );
    }
}

pub(crate) fn set_area_of_interest(
    context: &ProjContext,
    factory: &PjFactoryContextPtr,
    [west, south, east, north]: [f64; 4],
) {
    // SAFETY: a live context and factory context of it.
    unsafe {
        ffi::proj_operation_factory_context_set_area_of_interest(
            context.raw(),
            factory.0.as_ptr(),
            west,
            south,
            east,
            north,
        );
    }
}

pub(crate) fn set_desired_accuracy(
    context: &ProjContext,
    factory: &PjFactoryContextPtr,
    accuracy: f64,
) {
    // SAFETY: a live context and factory context of it.
    unsafe {
        ffi::proj_operation_factory_context_set_desired_accuracy(
            context.raw(),
            factory.0.as_ptr(),
            accuracy,
        );
    }
}

pub(crate) fn create_operations(
    context: &ProjContext,
    source_crs: &PjPtr,
    target_crs: &PjPtr,
    factory: &PjFactoryContextPtr,
) -> Option<PjListPtr> {
    // SAFETY: a live context, two live PJs and a factory context of it; the
    // list returned is owned by the PjListPtr.
    NonNull::new(unsafe {
        ffi::proj_create_operations(
            context.raw(),
            source_crs.raw(),
            target_crs.raw(),
            factory.0.as_ptr(),
        )
    })
    .map(PjListPtr)
}

pub(crate) fn list_get_count(list: &PjListPtr) -> c_int {
    // SAFETY: a live list.
    unsafe { ffi::proj_list_get_count(list.0.as_ptr()) }
}

pub(crate) fn list_get(context: &ProjContext, list: &PjListPtr, index: c_int) -> Option<PjPtr> {
    // SAFETY: a live context and list; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe { ffi::proj_list_get(context.raw(), list.0.as_ptr(), index) })
}

pub(crate) fn normalize_for_visualization(context: &ProjContext, object: &PjPtr) -> Option<PjPtr> {
    // SAFETY: a live context and PJ; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe { ffi::proj_normalize_for_visualization(context.raw(), object.raw()) })
}

/// The operation from one CRS to another, with no area and no options.
pub(crate) fn create_crs_to_crs_from_pj(
    context: &ProjContext,
    source_crs: &PjPtr,
    target_crs: &PjPtr,
) -> Option<PjPtr> {
    // SAFETY: a live context and two live PJs; NULL area and options are
    // allowed; the PJ returned is owned by the PjPtr.
    PjPtr::from_raw(unsafe {
        ffi::proj_create_crs_to_crs_from_pj(
            context.raw(),
            source_crs.raw(),
            target_crs.raw(),
            ptr::null_mut(),
            ptr::null(),
        )
    })
}

// ---- transformation ----------------------------------------------------------------

pub(crate) fn errno(object: &PjPtr) -> c_int {
    // SAFETY: a live PJ.
    unsafe { ffi::proj_errno(object.raw()) }
}

pub(crate) fn errno_reset(object: &PjPtr) {
    // SAFETY: a live PJ.
    unsafe { ffi::proj_errno_reset(object.raw()) };
}

/// proj_trans() of one coordinate: the result's x, y, z and t.
pub(crate) fn trans(
    operation: &PjPtr,
    direction: ffi::PjDirection,
    [x, y, z, t]: [f64; 4],
) -> [f64; 4] {
    // SAFETY: a live PJ; every member of the PJ_COORD PROJ returns is a
    // double, so reading it as xyzt is always defined.
    unsafe {
        let result = ffi::proj_trans(operation.raw(), direction, ffi::proj_coord(x, y, z, t));
        [result.xyzt.x, result.xyzt.y, result.xyzt.z, result.xyzt.t]
    }
}

/// proj_trans() of one coordinate, read as the C++ reads a 2D result: xy.
pub(crate) fn trans_xy(
    operation: &PjPtr,
    direction: ffi::PjDirection,
    [x, y, z, t]: [f64; 4],
) -> (f64, f64) {
    // SAFETY: as for trans().
    unsafe {
        let result = ffi::proj_trans(operation.raw(), direction, ffi::proj_coord(x, y, z, t));
        (result.xy.x, result.xy.y)
    }
}

/// One proj_trans_generic() call over every coordinate, in place: x, y, z and
/// t are interior pointers with the size of a Coordinate as the stride.
pub(crate) fn trans_generic_coordinates(
    operation: &PjPtr,
    direction: ffi::PjDirection,
    coordinates: &mut [Coordinate],
) {
    if coordinates.is_empty() {
        return;
    }
    let stride = size_of::<Coordinate>();
    let count = coordinates.len();
    // Every pointer is derived from the whole slice, so PROJ may step through
    // all of it.
    let base = coordinates.as_mut_ptr().cast::<u8>();
    let member = |offset: usize| base.wrapping_add(offset).cast::<f64>();
    // SAFETY: a live PJ; each pointer addresses a double member of the first
    // of `count` contiguous Coordinates (repr(C), four doubles), stepped by
    // their size, all within the slice, which is exclusively borrowed.
    unsafe {
        ffi::proj_trans_generic(
            operation.raw(),
            direction,
            member(offset_of!(Coordinate, x)),
            stride,
            count,
            member(offset_of!(Coordinate, y)),
            stride,
            count,
            member(offset_of!(Coordinate, z)),
            stride,
            count,
            member(offset_of!(Coordinate, t)),
            stride,
            count,
        );
    }
}

/// One proj_trans_generic() call over named coordinates, in place, with one
/// epoch broadcast to every point (an array of length 1, which PROJ updates
/// with the last point's).
pub(crate) fn trans_generic_named<T: NamedCoordinate>(
    operation: &PjPtr,
    direction: ffi::PjDirection,
    coordinates: &mut [T],
    epoch: &mut f64,
) {
    if coordinates.is_empty() {
        return;
    }
    let stride = size_of::<T>();
    let count = coordinates.len();
    let base = coordinates.as_mut_ptr().cast::<u8>();
    let member = |offset: usize| base.wrapping_add(offset).cast::<f64>();
    // SAFETY: a live PJ; the named types are repr(C) structs of three doubles,
    // and the offsets are their members' (offset_of!), so each pointer
    // addresses a double of the first of `count` contiguous values, stepped by
    // their size, all within the exclusively borrowed slice; the epoch is one
    // live double.
    unsafe {
        ffi::proj_trans_generic(
            operation.raw(),
            direction,
            member(T::X_OFFSET),
            stride,
            count,
            member(T::Y_OFFSET),
            stride,
            count,
            member(T::HEIGHT_OFFSET),
            stride,
            count,
            epoch,
            0,
            1,
        );
    }
}

/// proj_factors() at a position given in radians (lam = longitude, phi =
/// latitude), set into a PJ_COORD as the C++ does.
pub(crate) fn factors(projected: &PjPtr, longitude: f64, latitude: f64) -> ffi::PjFactors {
    // SAFETY: a live PJ; PJ_COORD is plain doubles, written through lp.
    unsafe {
        let mut position = ffi::proj_coord(0.0, 0.0, 0.0, 0.0);
        position.lp.lam = longitude;
        position.lp.phi = latitude;
        ffi::proj_factors(projected.raw(), position)
    }
}

// ---- geodesics ----------------------------------------------------------------------

/// geod_inverse() on the ellipsoid (a, f): s12, azi1, azi2.
pub(crate) fn geodesic_inverse(
    semi_major_axis: f64,
    flattening: f64,
    [latitude1, longitude1]: [f64; 2],
    [latitude2, longitude2]: [f64; 2],
) -> (f64, f64, f64) {
    let mut geodesic = MaybeUninit::<ffi::GeodGeodesic>::uninit();
    let mut distance = 0.0;
    let mut azimuth_at_start = 0.0;
    let mut azimuth_at_end = 0.0;
    // SAFETY: geod_init fills every member of the struct before geod_inverse
    // reads it; the out-pointers are locals.
    unsafe {
        ffi::geod_init(geodesic.as_mut_ptr(), semi_major_axis, flattening);
        ffi::geod_inverse(
            geodesic.as_ptr(),
            latitude1,
            longitude1,
            latitude2,
            longitude2,
            &mut distance,
            &mut azimuth_at_start,
            &mut azimuth_at_end,
        );
    }
    (distance, azimuth_at_start, azimuth_at_end)
}

/// geod_direct() on the ellipsoid (a, f): lat2, lon2, azi2.
pub(crate) fn geodesic_direct(
    semi_major_axis: f64,
    flattening: f64,
    [latitude, longitude]: [f64; 2],
    azimuth: f64,
    distance: f64,
) -> (f64, f64, f64) {
    let mut geodesic = MaybeUninit::<ffi::GeodGeodesic>::uninit();
    let mut end_latitude = 0.0;
    let mut end_longitude = 0.0;
    let mut azimuth_at_end = 0.0;
    // SAFETY: geod_init fills every member of the struct before geod_direct
    // reads it; the out-pointers are locals.
    unsafe {
        ffi::geod_init(geodesic.as_mut_ptr(), semi_major_axis, flattening);
        ffi::geod_direct(
            geodesic.as_ptr(),
            latitude,
            longitude,
            azimuth,
            distance,
            &mut end_latitude,
            &mut end_longitude,
            &mut azimuth_at_end,
        );
    }
    (end_latitude, end_longitude, azimuth_at_end)
}
