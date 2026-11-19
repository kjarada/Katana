//! GDAL's drivers, datasets, bands, layers and features as Rust owners and
//! views (gdal.h, gdal_utils.h, ogr_api.h): every call the crate makes on
//! them, each a safe method, so that the adapter, the formats' registry, the
//! sampler, reprojection and the algorithm bridge are safe code.
//!
//! Who frees what:
//!
//! ```text
//! Driver          a registered driver; lives as long as the process (the
//!                 driver manager is never destroyed, gdal_registry.rs), so
//!                 it is Copy and never freed
//! Dataset         a dataset Rust opened or made; closed on drop (GDALClose),
//!                 or by `close`, which says how the close went
//! DatasetRef      one reference to a dataset; dropped on drop
//!                 (GDALReleaseDataset, the C++'s Release()), the dataset
//!                 closing with its last
//! DatasetHandle   what both deref to: the dataset's calls, never freed itself
//! Band, Layer,    borrowed from the dataset for as long as the borrow of it
//! ColorTable,     lasts; GDAL frees them with the dataset
//! FeatureDefn,
//! FieldDefnRef
//! ResultSet       an SQL result layer; released to its dataset on drop
//!                 (GDALDatasetReleaseResultSet)
//! Feature         a feature Rust owns, made for or read from a layer, which
//!                 it may not outlive; destroyed on drop (OGR_F_Destroy)
//! FieldDefn       a field definition Rust made; destroyed on drop
//! *Options        a utility's parsed arguments; freed on drop
//! ```
//!
//! Text crosses the boundary once: what GDAL returns is copied (cpl::decoded)
//! inside the method that read it. A progress callback is a Rust closure,
//! reached through one extern "C" trampoline that catches every panic.

#![allow(
    unsafe_code,
    reason = "Owns GDAL's datasets, features and options and calls their C API; every call states why it is sound."
)]

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::marker::PhantomData;
use std::ops::Deref;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};

use super::cpl::{self, CStringList};
use super::geometry::{GeometryView, OgrGeometry};
use super::srs::{SpatialReference, SpatialReferenceRef};
use super::{algorithm, gdal, ogr};

// ---- progress --------------------------------------------------------------------------

/// A progress callback as Rust hands it to GDAL: given the fraction done, it
/// answers TRUE to go on and FALSE to stop. GDAL may call it from threads of
/// its own (the viewshed's, the warper's), so it is Sync.
pub(crate) type Progress<'a> = &'a (dyn Fn(f64) -> c_int + Sync);

/// GDAL's progress function for every Rust callback: `data` is a pointer to
/// a `Progress`, alive until the GDAL call that was given it returns. A panic
/// that escapes the callback stops GDAL rather than unwinding into it.
pub(super) extern "C" fn progress_trampoline(
    complete: f64,
    _message: *const c_char,
    data: *mut c_void,
) -> c_int {
    if data.is_null() {
        return cpl::TRUE;
    }
    // SAFETY: `data` points at the Progress the caller of the GDAL function
    // put on its stack, alive until that call returns, and only ever read; a
    // shared reference to a Sync closure may be used from any thread.
    let progress = unsafe { *data.cast::<Progress<'_>>() };
    catch_unwind(AssertUnwindSafe(|| progress(complete))).unwrap_or(cpl::FALSE)
}

// ---- the library -----------------------------------------------------------------------

/// `GDALAllRegister()`: every driver registered. GDAL guards its driver
/// manager, and a second call adds nothing.
pub(crate) fn all_register() {
    // SAFETY: no argument.
    unsafe { gdal::GDALAllRegister() };
}

/// `GDALVersionInfo(request)`: None when GDAL answers NULL.
pub(crate) fn version_info(request: &CStr) -> Option<String> {
    // SAFETY: a NUL-terminated request; the answer is GDAL's static text.
    let text = unsafe { gdal::GDALVersionInfo(request.as_ptr()) };
    // SAFETY: NULL or a live NUL-terminated string, copied at once.
    (!text.is_null()).then(|| unsafe { cpl::decoded(text) })
}

/// `GDALGetDataTypeName(type)`: empty for a type GDAL has no name for.
pub(crate) fn data_type_name(data_type: gdal::GdalDataType) -> String {
    // SAFETY: a static text GDAL keeps (or NULL), copied at once.
    unsafe { cpl::decoded(gdal::GDALGetDataTypeName(data_type)) }
}

/// `GDALGetDataTypeByName(name)`: GDT_Unknown for no such type.
pub(crate) fn data_type_by_name(name: &CStr) -> gdal::GdalDataType {
    // SAFETY: a NUL-terminated name.
    unsafe { algorithm::GDALGetDataTypeByName(name.as_ptr()) }
}

/// `GDALDataTypeIsFloating(type)`.
pub(crate) fn data_type_is_floating(data_type: gdal::GdalDataType) -> bool {
    // SAFETY: a pure function of a data type.
    unsafe { algorithm::GDALDataTypeIsFloating(data_type) != 0 }
}

/// `GDALInvGeoTransform(transform, inverse)`: whether it could be inverted.
pub(crate) fn invert_geo_transform(transform: &[f64; 6], inverse: &mut [f64; 6]) -> bool {
    // SAFETY: six doubles in, six out.
    unsafe { gdal::GDALInvGeoTransform(transform.as_ptr(), inverse.as_mut_ptr()) != 0 }
}

/// `GDALGetOutputDriversForDatasetName(name, ...)`: the drivers GDAL would
/// write the name with, best first; None for none.
pub(crate) fn output_drivers_for_dataset_name(
    name: &CStr,
    raster_or_vector: c_int,
    single_match: bool,
    emit_warning: bool,
) -> Option<Vec<Vec<u8>>> {
    // SAFETY: a NUL-terminated name; the list GDAL returns is the caller's,
    // copied and freed by take_string_list.
    unsafe {
        cpl::take_string_list(gdal::GDALGetOutputDriversForDatasetName(
            name.as_ptr(),
            raster_or_vector,
            single_match,
            emit_warning,
        ))
    }
}

// ---- drivers ---------------------------------------------------------------------------

/// A registered driver. Made only from a handle GDAL's driver manager gave,
/// which lives as long as the process.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Driver(NonNull<gdal::GdalDriverHs>);

impl Driver {
    /// `GDALGetDriverByName(name)`.
    pub(crate) fn by_name(name: &str) -> Option<Driver> {
        let name = cpl::c_string(name);
        // SAFETY: a NUL-terminated name; the driver lives as long as the process.
        NonNull::new(unsafe { gdal::GDALGetDriverByName(name.as_ptr()) }).map(Driver)
    }

    /// `GDALGetDriverCount()`.
    pub(crate) fn count() -> c_int {
        // SAFETY: no argument.
        unsafe { gdal::GDALGetDriverCount() }
    }

    /// `GDALGetDriver(index)`: None past the count.
    pub(crate) fn at(index: c_int) -> Option<Driver> {
        // SAFETY: GDAL checks the index and answers NULL for one it does not
        // have; a driver lives as long as the process.
        NonNull::new(unsafe { gdal::GDALGetDriver(index) }).map(Driver)
    }

    /// `GDALIdentifyDriverEx(name, flags, NULL, NULL)`: the driver that
    /// recognises the dataset, without opening it.
    pub(crate) fn identify(name: &CStr, flags: c_uint) -> Option<Driver> {
        // SAFETY: a NUL-terminated name and no lists; the driver lives as
        // long as the process.
        NonNull::new(unsafe {
            gdal::GDALIdentifyDriverEx(name.as_ptr(), flags, ptr::null(), ptr::null())
        })
        .map(Driver)
    }

    fn raw(self) -> gdal::GdalDriverH {
        self.0.as_ptr()
    }

    /// The driver's description: its short name ("GTiff").
    pub(crate) fn description(self) -> String {
        // SAFETY: a live driver; the text is copied at once.
        unsafe { cpl::decoded(gdal::GDALGetDescription(self.raw().cast())) }
    }

    /// `GetMetadataItem(key)`, the default domain: None for NULL.
    pub(crate) fn metadata_item(self, key: &CStr) -> Option<String> {
        // SAFETY: a live driver and NUL-terminated key and domain.
        let value = unsafe { gdal::GDALGetMetadataItem(self.raw().cast(), key.as_ptr(), c"".as_ptr()) };
        // SAFETY: NULL or a string the driver owns, copied at once.
        (!value.is_null()).then(|| unsafe { cpl::decoded(value) })
    }

    /// Whether a capability is set: `CPLTestBool` of its metadata item.
    pub(crate) fn flag(self, key: &CStr) -> bool {
        // SAFETY: a live driver and NUL-terminated key and domain; the value
        // is the driver's own, tested at once.
        unsafe {
            let value = gdal::GDALGetMetadataItem(self.raw().cast(), key.as_ptr(), c"".as_ptr());
            !value.is_null() && cpl::CPLTestBool(value)
        }
    }

    /// `CPLFetchBool(GetMetadata(""), key, default)`.
    pub(crate) fn metadata_bool(self, key: &CStr, default: bool) -> bool {
        // SAFETY: a live driver; its metadata list is its own, read at once,
        // and a NUL-terminated key.
        unsafe {
            let capabilities = gdal::GDALGetMetadata(self.raw().cast(), c"".as_ptr());
            cpl::CPLFetchBool(capabilities, key.as_ptr(), default)
        }
    }

    /// `GDALCreate` with this driver.
    pub(crate) fn create(
        self,
        name: &CStr,
        width: c_int,
        height: c_int,
        bands: c_int,
        data_type: gdal::GdalDataType,
        options: &CStringList,
    ) -> Option<Dataset> {
        create(Some(self), name, width, height, bands, data_type, options)
    }

    /// `GDALCreateCopy` with this driver.
    pub(crate) fn create_copy(
        self,
        name: &CStr,
        source: &DatasetHandle,
        strict: bool,
        options: &CStringList,
    ) -> Option<Dataset> {
        create_copy(Some(self), name, source, strict, options)
    }

    /// `GDALDeleteDataset(driver, name)`: the dataset and the sidecars the
    /// driver knows of.
    pub(crate) fn delete_dataset(self, name: &CStr) -> cpl::CplErr {
        // SAFETY: a live driver and a NUL-terminated name.
        unsafe { gdal::GDALDeleteDataset(self.raw(), name.as_ptr()) }
    }
}

/// `GDALCreate(driver, ...)`: a new dataset, owned by the result. A missing
/// driver is handed to GDAL as NULL, which refuses it with an error of its
/// own, as the C++'s call through a null driver did.
pub(crate) fn create(
    driver: Option<Driver>,
    name: &CStr,
    width: c_int,
    height: c_int,
    bands: c_int,
    data_type: gdal::GdalDataType,
    options: &CStringList,
) -> Option<Dataset> {
    let driver = driver.map_or(ptr::null_mut(), Driver::raw);
    // SAFETY: NULL (which GDAL refuses) or a live driver, a NUL-terminated
    // name and a NULL or NULL-terminated option list GDAL only reads; the
    // dataset returned is owned by the Dataset made from it.
    let made = unsafe {
        gdal::GDALCreate(driver, name.as_ptr(), width, height, bands, data_type, options.as_ptr())
    };
    NonNull::new(made).map(|handle| Dataset(DatasetHandle(handle)))
}

/// `GDALCreateCopy(driver, name, source, strict, options, NULL, NULL)`: a
/// copy of `source`, owned by the result; a missing driver as `create`'s.
pub(crate) fn create_copy(
    driver: Option<Driver>,
    name: &CStr,
    source: &DatasetHandle,
    strict: bool,
    options: &CStringList,
) -> Option<Dataset> {
    let driver = driver.map_or(ptr::null_mut(), Driver::raw);
    // SAFETY: NULL (which GDAL refuses) or a live driver, a NUL-terminated
    // name, a live source dataset, a NULL or NULL-terminated option list
    // GDAL only reads, and no progress; the copy is owned by the result.
    let copy = unsafe {
        gdal::GDALCreateCopy(
            driver,
            name.as_ptr(),
            source.raw(),
            if strict { cpl::TRUE } else { cpl::FALSE },
            options.as_ptr(),
            None,
            ptr::null_mut(),
        )
    };
    NonNull::new(copy).map(|handle| Dataset(DatasetHandle(handle)))
}

// ---- datasets --------------------------------------------------------------------------

/// A live dataset's calls. Only ever reached through its owner, a `Dataset`
/// or a `DatasetRef`, which keep it alive for the borrow; never Send or Sync
/// itself, so its views stay on the thread that holds the owner.
#[derive(Debug)]
pub(crate) struct DatasetHandle(NonNull<gdal::GdalDatasetHs>);

impl DatasetHandle {
    pub(super) fn raw(&self) -> gdal::GdalDatasetH {
        self.0.as_ptr()
    }

    pub(crate) fn raster_x_size(&self) -> c_int {
        // SAFETY: a live dataset.
        unsafe { gdal::GDALGetRasterXSize(self.raw()) }
    }

    pub(crate) fn raster_y_size(&self) -> c_int {
        // SAFETY: a live dataset.
        unsafe { gdal::GDALGetRasterYSize(self.raw()) }
    }

    pub(crate) fn raster_count(&self) -> c_int {
        // SAFETY: a live dataset.
        unsafe { gdal::GDALGetRasterCount(self.raw()) }
    }

    /// `GetRasterBand(index)`, 1-based: None (and GDAL's error) for a band
    /// the dataset does not have.
    pub(crate) fn band(&self, index: c_int) -> Option<Band<'_>> {
        // SAFETY: a live dataset; GDAL checks the index and answers NULL for
        // one it does not have; a band lives as long as its dataset.
        let band = unsafe { gdal::GDALGetRasterBand(self.raw(), index) };
        NonNull::new(band).map(|handle| Band {
            handle,
            _dataset: PhantomData,
        })
    }

    /// `GetProjectionRef()`: None for NULL.
    pub(crate) fn projection_ref(&self) -> Option<String> {
        // SAFETY: a live dataset; the text is the dataset's own.
        let wkt = unsafe { gdal::GDALGetProjectionRef(self.raw()) };
        // SAFETY: NULL or a live NUL-terminated string, copied at once.
        (!wkt.is_null()).then(|| unsafe { cpl::decoded(wkt) })
    }

    /// `GetSpatialRef()`: the dataset's own reference, borrowed.
    pub(crate) fn spatial_ref(&self) -> Option<SpatialReferenceRef<'_>> {
        // SAFETY: a live dataset; its reference (or NULL) is its own and
        // lives as long as it does, which the borrow of `self` outlasts.
        unsafe { SpatialReferenceRef::from_raw(gdal::GDALGetSpatialRef(self.raw())) }
    }

    /// `SetSpatialRef(reference)`: GDAL copies the reference.
    pub(crate) fn set_spatial_ref(&self, reference: &SpatialReference) -> cpl::CplErr {
        // SAFETY: a live dataset and a live reference, which GDAL copies.
        unsafe { gdal::GDALSetSpatialRef(self.raw(), reference.view().raw()) }
    }

    /// `GetGeoTransform(transform)`: what GDAL wrote into `transform` (its
    /// default affine when it has none) and whether the dataset has one.
    pub(crate) fn geo_transform(&self, transform: &mut [f64; 6]) -> cpl::CplErr {
        // SAFETY: a live dataset and six doubles.
        unsafe { gdal::GDALGetGeoTransform(self.raw(), transform.as_mut_ptr()) }
    }

    /// `SetGeoTransform(transform)`.
    pub(crate) fn set_geo_transform(&self, transform: &[f64; 6]) -> cpl::CplErr {
        // SAFETY: a live dataset and six doubles, which GDAL only reads.
        unsafe { gdal::GDALSetGeoTransform(self.raw(), transform.as_ptr()) }
    }

    /// The driver that opened or made the dataset.
    pub(crate) fn driver(&self) -> Option<Driver> {
        // SAFETY: a live dataset; a driver lives as long as the process.
        NonNull::new(unsafe { gdal::GDALGetDatasetDriver(self.raw()) }).map(Driver)
    }

    /// Whether the dataset's metadata of `domain` has an entry
    /// (`GetMetadata(domain)` neither NULL nor empty).
    pub(crate) fn has_metadata(&self, domain: &CStr) -> bool {
        // SAFETY: a live dataset and a NUL-terminated domain; the list is the
        // dataset's own, NULL or NULL-terminated, and its first entry is read
        // at once.
        unsafe {
            let list = gdal::GDALGetMetadata(self.raw().cast(), domain.as_ptr());
            !list.is_null() && !(*list).is_null()
        }
    }

    pub(crate) fn layer_count(&self) -> c_int {
        // SAFETY: a live dataset.
        unsafe { gdal::GDALDatasetGetLayerCount(self.raw()) }
    }

    /// `GetLayer(index)`, 0-based: None for a layer it does not have.
    pub(crate) fn layer(&self, index: c_int) -> Option<Layer<'_>> {
        // SAFETY: a live dataset; GDAL checks the index and answers NULL for
        // one it does not have; a layer lives as long as its dataset.
        unsafe { Layer::from_raw(gdal::GDALDatasetGetLayer(self.raw(), index)) }
    }

    /// `GetLayerByName(name)`.
    pub(crate) fn layer_by_name(&self, name: &CStr) -> Option<Layer<'_>> {
        // SAFETY: a live dataset and a NUL-terminated name; a layer lives as
        // long as its dataset.
        unsafe { Layer::from_raw(gdal::GDALDatasetGetLayerByName(self.raw(), name.as_ptr())) }
    }

    /// `DeleteLayer(index)`. Taking `&mut self` keeps every borrowed layer
    /// from outliving the one it might be.
    pub(crate) fn delete_layer(&mut self, index: c_int) -> ogr::OgrErr {
        // SAFETY: a live dataset, no layer of which is borrowed (above);
        // GDAL checks the index.
        unsafe { gdal::GDALDatasetDeleteLayer(self.raw(), index) }
    }

    /// `CreateLayer(name, reference, type, options)`: the reference is
    /// copied by the driver.
    pub(crate) fn create_layer(
        &self,
        name: &CStr,
        reference: Option<&SpatialReference>,
        geometry_type: ogr::OgrWkbGeometryType,
        options: &CStringList,
    ) -> Option<Layer<'_>> {
        let reference = reference.map_or(ptr::null_mut(), |reference| reference.view().raw());
        // SAFETY: a live dataset, a NUL-terminated name, NULL or a live
        // reference (copied by the driver), and a NULL or NULL-terminated
        // option list; the layer is the dataset's.
        unsafe {
            Layer::from_raw(gdal::GDALDatasetCreateLayer(
                self.raw(),
                name.as_ptr(),
                reference,
                geometry_type,
                options.as_ptr(),
            ))
        }
    }

    /// `TestCapability(capability)`.
    pub(crate) fn test_capability(&self, capability: &CStr) -> bool {
        // SAFETY: a live dataset and a NUL-terminated capability.
        unsafe { gdal::GDALDatasetTestCapability(self.raw(), capability.as_ptr()) != 0 }
    }

    pub(crate) fn start_transaction(&self, force: bool) -> ogr::OgrErr {
        let force = if force { cpl::TRUE } else { cpl::FALSE };
        // SAFETY: a live dataset.
        unsafe { gdal::GDALDatasetStartTransaction(self.raw(), force) }
    }

    pub(crate) fn commit_transaction(&self) -> ogr::OgrErr {
        // SAFETY: a live dataset.
        unsafe { gdal::GDALDatasetCommitTransaction(self.raw()) }
    }

    pub(crate) fn rollback_transaction(&self) -> ogr::OgrErr {
        // SAFETY: a live dataset.
        unsafe { gdal::GDALDatasetRollbackTransaction(self.raw()) }
    }

    /// `ExecuteSQL(statement, filter, dialect)`: the rows as a layer,
    /// released to the dataset when the result set goes; None when GDAL
    /// gives none. The filter geometry is copied by GDAL.
    pub(crate) fn execute_sql(
        &self,
        statement: &CStr,
        filter: Option<&OgrGeometry>,
        dialect: Option<&CStr>,
    ) -> Option<ResultSet<'_>> {
        // SAFETY: a live dataset, a NUL-terminated statement, NULL or a live
        // filter geometry (copied by GDAL), and NULL or a NUL-terminated
        // dialect; the layer returned is released by the ResultSet.
        let rows = unsafe {
            gdal::GDALDatasetExecuteSQL(
                self.raw(),
                statement.as_ptr(),
                filter.map_or(ptr::null_mut(), OgrGeometry::raw),
                dialect.map_or(ptr::null(), CStr::as_ptr),
            )
        };
        NonNull::new(rows).map(|rows| ResultSet {
            dataset: self,
            rows,
        })
    }
}

/// A dataset Rust opened or made, closed once: by `close`, or on drop.
#[derive(Debug)]
pub(crate) struct Dataset(DatasetHandle);

// SAFETY: GDAL lets a dataset be used from any one thread at a time and
// handed between threads; the value is not Sync, so it is never used by two
// threads at once, and every view of it borrows it, so none crosses with it.
unsafe impl Send for Dataset {}

impl Deref for Dataset {
    type Target = DatasetHandle;

    fn deref(&self) -> &DatasetHandle {
        &self.0
    }
}

impl std::ops::DerefMut for Dataset {
    fn deref_mut(&mut self) -> &mut DatasetHandle {
        &mut self.0
    }
}

impl Dataset {
    /// `GDALOpenEx(name, flags, NULL, options, NULL)`: None when GDAL cannot
    /// open it, GDAL's reason left as this thread's last error.
    pub(crate) fn open(name: &CStr, flags: c_uint, options: &CStringList) -> Option<Self> {
        // SAFETY: a NUL-terminated name and a NULL or NULL-terminated option
        // list GDAL only reads; the dataset returned is owned by the result.
        let opened = unsafe {
            gdal::GDALOpenEx(name.as_ptr(), flags, ptr::null(), options.as_ptr(), ptr::null())
        };
        NonNull::new(opened).map(|handle| Self(DatasetHandle(handle)))
    }

    /// `GDALClose`, saying how it went: GDAL (3.7 on) reports here what it
    /// could only find out while flushing - a full disk, a share gone.
    pub(crate) fn close(self) -> cpl::CplErr {
        let handle = self.raw();
        std::mem::forget(self);
        // SAFETY: the dataset this value owned, closed once (Drop will not
        // run); no view of it outlives the value, which was moved here.
        unsafe { gdal::GDALClose(handle) }
    }

    /// The same dataset held as a reference, dropped with GDALReleaseDataset
    /// (the C++'s Release()) rather than closed: what a dataset GDALCreate
    /// made becomes when it is handed to an algorithm, which takes
    /// references of its own.
    pub(crate) fn into_reference(self) -> DatasetRef {
        let handle = self.0.0;
        std::mem::forget(self);
        DatasetRef(DatasetHandle(handle))
    }
}

impl Drop for Dataset {
    fn drop(&mut self) {
        // SAFETY: the dataset is owned by this value alone, closed once; its
        // views borrowed the value and are gone. Deliberately no
        // GDALDestroyDriverManager() (gdal_registry.rs).
        unsafe { gdal::GDALClose(self.raw()) };
    }
}

/// One reference to a GDAL dataset, dropped with this value: the dataset
/// closes with its last reference (GDALReleaseDataset, the C++'s
/// `Release()`). Not Send: it is used on the thread of the run that holds it.
#[derive(Debug)]
pub(crate) struct DatasetRef(DatasetHandle);

impl Deref for DatasetRef {
    type Target = DatasetHandle;

    fn deref(&self) -> &DatasetHandle {
        &self.0
    }
}

impl DatasetRef {
    /// Takes over a reference the caller holds; None for NULL.
    ///
    /// # Safety
    ///
    /// `handle` is NULL or a live dataset of which the caller holds one
    /// reference that nothing else will drop.
    pub(super) unsafe fn from_raw(handle: gdal::GdalDatasetH) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self(DatasetHandle(handle)))
    }
}

impl Drop for DatasetRef {
    fn drop(&mut self) {
        // SAFETY: one reference this value holds, dropped once.
        unsafe { algorithm::GDALReleaseDataset(self.raw()) };
    }
}

// ---- bands -----------------------------------------------------------------------------

/// A raster band, borrowed from its dataset.
#[derive(Clone, Copy)]
pub(crate) struct Band<'a> {
    handle: NonNull<gdal::GdalRasterBandHs>,
    _dataset: PhantomData<&'a DatasetHandle>,
}

/// Whether a buffer of `len` values holds a `width` x `height` window. GDAL
/// does nothing for a window of no cells (it returns at once), so only a
/// positive window is measured.
fn holds(len: usize, width: c_int, height: c_int) -> bool {
    match (usize::try_from(width), usize::try_from(height)) {
        (Ok(width), Ok(height)) if width > 0 && height > 0 => {
            width.checked_mul(height).is_some_and(|needed| needed <= len)
        }
        _ => true,
    }
}

impl<'a> Band<'a> {
    fn raw(self) -> gdal::GdalRasterBandH {
        self.handle.as_ptr()
    }

    /// `RasterIO(GF_Read, ...)` of the window into `buffer` as doubles, at
    /// `buffer_width` x `buffer_height` (GDAL resamples when they differ).
    /// CE_Failure, GDAL not asked, for a buffer too small for them.
    #[allow(clippy::too_many_arguments, reason = "RasterIO's window and buffer.")]
    pub(crate) fn read_f64(
        self,
        x_offset: c_int,
        y_offset: c_int,
        x_size: c_int,
        y_size: c_int,
        buffer: &mut [f64],
        buffer_width: c_int,
        buffer_height: c_int,
    ) -> cpl::CplErr {
        if !holds(buffer.len(), buffer_width, buffer_height) {
            return cpl::CE_FAILURE;
        }
        // SAFETY: a live band and a buffer of at least buffer_width x
        // buffer_height doubles (checked above), packed.
        unsafe {
            gdal::GDALRasterIO(
                self.raw(),
                gdal::GF_READ,
                x_offset,
                y_offset,
                x_size,
                y_size,
                buffer.as_mut_ptr().cast(),
                buffer_width,
                buffer_height,
                gdal::GDT_FLOAT64,
                0,
                0,
            )
        }
    }

    /// The same as bytes (GDT_Byte).
    #[allow(clippy::too_many_arguments, reason = "RasterIO's window and buffer.")]
    pub(crate) fn read_u8(
        self,
        x_offset: c_int,
        y_offset: c_int,
        x_size: c_int,
        y_size: c_int,
        buffer: &mut [u8],
        buffer_width: c_int,
        buffer_height: c_int,
    ) -> cpl::CplErr {
        if !holds(buffer.len(), buffer_width, buffer_height) {
            return cpl::CE_FAILURE;
        }
        // SAFETY: a live band and a buffer of at least buffer_width x
        // buffer_height bytes (checked above), packed.
        unsafe {
            gdal::GDALRasterIO(
                self.raw(),
                gdal::GF_READ,
                x_offset,
                y_offset,
                x_size,
                y_size,
                buffer.as_mut_ptr().cast(),
                buffer_width,
                buffer_height,
                gdal::GDT_BYTE,
                0,
                0,
            )
        }
    }

    /// `RasterIO(GF_Write, ...)` of `values`, doubles, into the window.
    /// CE_Failure, GDAL not asked, for too few values.
    #[allow(clippy::too_many_arguments, reason = "RasterIO's window and buffer.")]
    pub(crate) fn write_f64(
        self,
        x_offset: c_int,
        y_offset: c_int,
        x_size: c_int,
        y_size: c_int,
        values: &[f64],
        buffer_width: c_int,
        buffer_height: c_int,
    ) -> cpl::CplErr {
        if !holds(values.len(), buffer_width, buffer_height) {
            return cpl::CE_FAILURE;
        }
        // RasterIO takes one non-const buffer for reading and writing alike;
        // GF_Write only reads it, and copying a band to satisfy the type
        // would double the memory of the largest thing a write holds.
        // SAFETY: a live band and at least buffer_width x buffer_height
        // doubles (checked above), which GF_Write reads and never writes.
        unsafe {
            gdal::GDALRasterIO(
                self.raw(),
                gdal::GF_WRITE,
                x_offset,
                y_offset,
                x_size,
                y_size,
                values.as_ptr().cast_mut().cast(),
                buffer_width,
                buffer_height,
                gdal::GDT_FLOAT64,
                0,
                0,
            )
        }
    }

    /// `GetNoDataValue(&has)`: None when the band has none.
    pub(crate) fn no_data_value(self) -> Option<f64> {
        let mut has_no_data: c_int = 0;
        // SAFETY: a live band and an out flag.
        let value = unsafe { gdal::GDALGetRasterNoDataValue(self.raw(), &raw mut has_no_data) };
        (has_no_data != 0).then_some(value)
    }

    pub(crate) fn set_no_data_value(self, value: f64) -> cpl::CplErr {
        // SAFETY: a live band.
        unsafe { gdal::GDALSetRasterNoDataValue(self.raw(), value) }
    }

    pub(crate) fn data_type(self) -> gdal::GdalDataType {
        // SAFETY: a live band.
        unsafe { gdal::GDALGetRasterDataType(self.raw()) }
    }

    pub(crate) fn color_interpretation(self) -> gdal::GdalColorInterp {
        // SAFETY: a live band.
        unsafe { gdal::GDALGetRasterColorInterpretation(self.raw()) }
    }

    pub(crate) fn set_color_interpretation(
        self,
        interpretation: gdal::GdalColorInterp,
    ) -> cpl::CplErr {
        // SAFETY: a live band.
        unsafe { algorithm::GDALSetRasterColorInterpretation(self.raw(), interpretation) }
    }

    /// The band's colour table, borrowed with it.
    pub(crate) fn color_table(self) -> Option<ColorTable<'a>> {
        // SAFETY: a live band; its table (or NULL) is its own and lives as
        // long as the dataset does.
        let table = unsafe { gdal::GDALGetRasterColorTable(self.raw()) };
        NonNull::new(table).map(|handle| ColorTable {
            handle,
            _dataset: PhantomData,
        })
    }

    /// `GDALRasterInterpolateAtPoint(pixel, line, method)`: the status and
    /// the real part GDAL wrote (0 when it wrote none).
    pub(crate) fn interpolate_at_point(
        self,
        pixel: f64,
        line: f64,
        method: gdal::GdalRioResampleAlg,
    ) -> (cpl::CplErr, f64) {
        let mut real = 0.0;
        let mut imaginary = 0.0;
        // SAFETY: a live band and two out values.
        let status = unsafe {
            gdal::GDALRasterInterpolateAtPoint(
                self.raw(),
                pixel,
                line,
                method,
                &raw mut real,
                &raw mut imaginary,
            )
        };
        (status, real)
    }
}

/// A band's colour table, borrowed from its dataset.
#[derive(Clone, Copy)]
pub(crate) struct ColorTable<'a> {
    handle: NonNull<gdal::GdalColorTableHs>,
    _dataset: PhantomData<&'a DatasetHandle>,
}

impl ColorTable<'_> {
    pub(crate) fn entry_count(self) -> c_int {
        // SAFETY: a live colour table.
        unsafe { gdal::GDALGetColorEntryCount(self.handle.as_ptr()) }
    }

    /// Entry `index`, copied; None for one the table does not have.
    pub(crate) fn entry(self, index: c_int) -> Option<gdal::GdalColorEntry> {
        // SAFETY: a live colour table; GDAL checks the index and answers
        // NULL for one it does not have; the entry is copied at once.
        unsafe { gdal::GDALGetColorEntry(self.handle.as_ptr(), index).as_ref().copied() }
    }
}

// ---- layers ----------------------------------------------------------------------------

/// A vector layer, borrowed from its dataset.
#[derive(Clone, Copy)]
pub(crate) struct Layer<'a> {
    handle: NonNull<ogr::OgrLayerHs>,
    _dataset: PhantomData<&'a DatasetHandle>,
}

impl<'a> Layer<'a> {
    /// # Safety
    ///
    /// `handle` is NULL or a live layer that stays alive for `'a`.
    unsafe fn from_raw(handle: ogr::OgrLayerH) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self {
            handle,
            _dataset: PhantomData,
        })
    }

    fn raw(self) -> ogr::OgrLayerH {
        self.handle.as_ptr()
    }

    pub(crate) fn name(self) -> String {
        // SAFETY: a live layer; its name is copied at once.
        unsafe { cpl::decoded(ogr::OGR_L_GetName(self.raw())) }
    }

    /// The name as GDAL's bytes, for what goes back to GDAL.
    pub(crate) fn name_bytes(self) -> Vec<u8> {
        // SAFETY: a live layer; its name is copied at once.
        unsafe { cpl::bytes_of(ogr::OGR_L_GetName(self.raw())) }
    }

    pub(crate) fn geometry_type(self) -> ogr::OgrWkbGeometryType {
        // SAFETY: a live layer.
        unsafe { ogr::OGR_L_GetGeomType(self.raw()) }
    }

    /// The layer's own reference, borrowed with it.
    pub(crate) fn spatial_ref(self) -> Option<SpatialReferenceRef<'a>> {
        // SAFETY: a live layer; its reference (or NULL) is its own and lives
        // as long as the dataset does.
        unsafe { SpatialReferenceRef::from_raw(ogr::OGR_L_GetSpatialRef(self.raw())) }
    }

    /// The layer's feature definition, borrowed with it.
    pub(crate) fn definition(self) -> Option<FeatureDefn<'a>> {
        // SAFETY: a live layer; its definition is its own and lives as long
        // as the dataset does.
        let definition = unsafe { ogr::OGR_L_GetLayerDefn(self.raw()) };
        NonNull::new(definition).map(|handle| FeatureDefn {
            handle,
            _dataset: PhantomData,
        })
    }

    pub(crate) fn reset_reading(self) {
        // SAFETY: a live layer.
        unsafe { ogr::OGR_L_ResetReading(self.raw()) };
    }

    /// `GetNextFeature()`: the feature, Rust's to destroy; None at the end.
    pub(crate) fn next_feature(self) -> Option<Feature<'a>> {
        // SAFETY: a live layer; the feature returned is the caller's to
        // destroy, which the Feature does.
        let feature = unsafe { ogr::OGR_L_GetNextFeature(self.raw()) };
        NonNull::new(feature).map(|handle| Feature {
            handle,
            _layer: PhantomData,
        })
    }

    pub(crate) fn set_spatial_filter_rect(self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        // SAFETY: a live layer.
        unsafe { ogr::OGR_L_SetSpatialFilterRect(self.raw(), min_x, min_y, max_x, max_y) };
    }

    /// `SetSpatialFilter(nullptr)`.
    pub(crate) fn clear_spatial_filter(self) {
        // SAFETY: a live layer; NULL clears its spatial filter.
        unsafe { ogr::OGR_L_SetSpatialFilter(self.raw(), ptr::null_mut()) };
    }

    /// `SetAttributeFilter(filter)`; None clears it.
    pub(crate) fn set_attribute_filter(self, filter: Option<&CStr>) -> ogr::OgrErr {
        // SAFETY: a live layer and NULL or a NUL-terminated filter.
        unsafe { ogr::OGR_L_SetAttributeFilter(self.raw(), filter.map_or(ptr::null(), CStr::as_ptr)) }
    }

    /// `GetFeatureCount(force)`: -1 when a lazy driver was not forced.
    pub(crate) fn feature_count(self, force: bool) -> i64 {
        let force = if force { cpl::TRUE } else { cpl::FALSE };
        // SAFETY: a live layer.
        unsafe { ogr::OGR_L_GetFeatureCount(self.raw(), force) }
    }

    pub(crate) fn test_capability(self, capability: &CStr) -> bool {
        // SAFETY: a live layer and a NUL-terminated capability.
        unsafe { ogr::OGR_L_TestCapability(self.raw(), capability.as_ptr()) != 0 }
    }

    /// `CreateField(field, approx_ok)`: the layer copies the definition.
    pub(crate) fn create_field(self, field: &FieldDefn, approx_ok: bool) -> ogr::OgrErr {
        let approx_ok = if approx_ok { cpl::TRUE } else { cpl::FALSE };
        // SAFETY: a live layer and a live field definition, which it copies.
        unsafe { ogr::OGR_L_CreateField(self.raw(), field.raw(), approx_ok) }
    }

    /// `CreateFeature(feature)`: the layer copies the feature.
    pub(crate) fn create_feature(self, feature: &Feature<'_>) -> ogr::OgrErr {
        // SAFETY: a live layer and a live feature, which it copies.
        unsafe { ogr::OGR_L_CreateFeature(self.raw(), feature.raw()) }
    }
}

/// An SQL statement's rows: a layer the dataset made, released to it on drop.
pub(crate) struct ResultSet<'a> {
    dataset: &'a DatasetHandle,
    rows: NonNull<ogr::OgrLayerHs>,
}

impl ResultSet<'_> {
    /// The rows as a layer, borrowed from the result set.
    pub(crate) fn layer(&self) -> Layer<'_> {
        Layer {
            handle: self.rows,
            _dataset: PhantomData,
        }
    }
}

impl Drop for ResultSet<'_> {
    fn drop(&mut self) {
        // SAFETY: the result set ExecuteSQL returned, released once to the
        // live dataset that made it; its layer views borrowed this value.
        unsafe { gdal::GDALDatasetReleaseResultSet(self.dataset.raw(), self.rows.as_ptr()) };
    }
}

// ---- definitions ---------------------------------------------------------------------

/// A layer's feature definition, borrowed from its dataset.
#[derive(Clone, Copy)]
pub(crate) struct FeatureDefn<'a> {
    handle: NonNull<ogr::OgrFeatureDefnHs>,
    _dataset: PhantomData<&'a DatasetHandle>,
}

impl<'a> FeatureDefn<'a> {
    fn raw(self) -> ogr::OgrFeatureDefnH {
        self.handle.as_ptr()
    }

    pub(crate) fn field_count(self) -> c_int {
        // SAFETY: a live definition.
        unsafe { ogr::OGR_FD_GetFieldCount(self.raw()) }
    }

    /// Field `index`'s definition: None (and GDAL's error) for one it does
    /// not have.
    pub(crate) fn field(self, index: c_int) -> Option<FieldDefnRef<'a>> {
        // SAFETY: a live definition; GDAL checks the index and answers NULL
        // for one it does not have; a field's definition is the layer's.
        let field = unsafe { ogr::OGR_FD_GetFieldDefn(self.raw(), index) };
        NonNull::new(field).map(|handle| FieldDefnRef {
            handle,
            _dataset: PhantomData,
        })
    }

    /// `GetFieldIndex(name)`: -1 for none.
    pub(crate) fn field_index(self, name: &CStr) -> c_int {
        // SAFETY: a live definition and a NUL-terminated name.
        unsafe { ogr::OGR_FD_GetFieldIndex(self.raw(), name.as_ptr()) }
    }
}

/// A field's definition, borrowed from its layer's dataset.
#[derive(Clone, Copy)]
pub(crate) struct FieldDefnRef<'a> {
    handle: NonNull<ogr::OgrFieldDefnHs>,
    _dataset: PhantomData<&'a DatasetHandle>,
}

impl FieldDefnRef<'_> {
    pub(crate) fn name(self) -> String {
        // SAFETY: a live field definition's own name, copied at once.
        unsafe { cpl::decoded(ogr::OGR_Fld_GetNameRef(self.handle.as_ptr())) }
    }

    pub(crate) fn field_type(self) -> ogr::OgrFieldType {
        // SAFETY: a live field definition.
        unsafe { ogr::OGR_Fld_GetType(self.handle.as_ptr()) }
    }

    pub(crate) fn sub_type(self) -> ogr::OgrFieldSubType {
        // SAFETY: a live field definition.
        unsafe { ogr::OGR_Fld_GetSubType(self.handle.as_ptr()) }
    }
}

/// A field definition Rust made, to hand to `Layer::create_field`, which
/// copies it; destroyed on drop.
pub(crate) struct FieldDefn(NonNull<ogr::OgrFieldDefnHs>);

impl FieldDefn {
    /// `new OGRFieldDefn(name, type)`.
    pub(crate) fn new(name: &CStr, field_type: ogr::OgrFieldType) -> Self {
        // SAFETY: a NUL-terminated name, which GDAL copies; the definition is
        // owned by this value.
        let handle = unsafe { ogr::OGR_Fld_Create(name.as_ptr(), field_type) };
        Self(NonNull::new(handle).expect("GDAL makes a field definition unless memory has run out"))
    }

    fn raw(&self) -> ogr::OgrFieldDefnH {
        self.0.as_ptr()
    }

    pub(crate) fn set_sub_type(&mut self, sub_type: ogr::OgrFieldSubType) {
        // SAFETY: a live field definition this value owns.
        unsafe { ogr::OGR_Fld_SetSubType(self.raw(), sub_type) };
    }
}

impl Drop for FieldDefn {
    fn drop(&mut self) {
        // SAFETY: the definition this value owns, destroyed once; a layer
        // given it took a copy.
        unsafe { ogr::OGR_Fld_Destroy(self.raw()) };
    }
}

// ---- features ----------------------------------------------------------------------------

/// A date and time as OGR reads a field (`GetFieldAsDateTimeEx`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FieldDateTime {
    pub(crate) year: c_int,
    pub(crate) month: c_int,
    pub(crate) day: c_int,
    pub(crate) hour: c_int,
    pub(crate) minute: c_int,
    pub(crate) second: f32,
}

/// A feature Rust owns: made for a layer, or read from one, and never kept
/// past the dataset it came from; destroyed on drop.
pub(crate) struct Feature<'a> {
    handle: NonNull<ogr::OgrFeatureHs>,
    _layer: PhantomData<&'a DatasetHandle>,
}

impl<'a> Feature<'a> {
    /// `OGR_F_Create(definition)`: an empty feature of the definition.
    pub(crate) fn create(definition: FeatureDefn<'a>) -> Option<Self> {
        // SAFETY: a live definition, which the feature references; the
        // feature is owned by the value made from it.
        let feature = unsafe { ogr::OGR_F_Create(definition.raw()) };
        NonNull::new(feature).map(|handle| Self {
            handle,
            _layer: PhantomData,
        })
    }

    fn raw(&self) -> ogr::OgrFeatureH {
        self.handle.as_ptr()
    }

    /// The feature's geometry, borrowed from it.
    pub(crate) fn geometry(&self) -> Option<GeometryView<'_>> {
        // SAFETY: a live feature; its geometry (or NULL) is its own and lives,
        // unchanged, for the borrow of `self`: replacing it takes `&mut self`.
        unsafe { GeometryView::from_raw(ogr::OGR_F_GetGeometryRef(self.raw())) }
    }

    /// `SetGeometryDirectly(geometry)`: the geometry is the feature's from here.
    pub(crate) fn set_geometry_directly(&mut self, geometry: OgrGeometry) -> ogr::OgrErr {
        // SAFETY: a live feature, no view of whose geometry is held (`&mut`);
        // the geometry's ownership passes to it.
        unsafe { ogr::OGR_F_SetGeometryDirectly(self.raw(), geometry.into_raw()) }
    }

    /// Whether field `index` holds a value (and not a null). False for a
    /// negative index, which OGR would read before its array.
    pub(crate) fn is_field_set_and_not_null(&self, index: c_int) -> bool {
        if index < 0 {
            return false;
        }
        // SAFETY: a live feature and a non-negative index, which OGR checks
        // against its fields.
        unsafe { ogr::OGR_F_IsFieldSetAndNotNull(self.raw(), index) != 0 }
    }

    pub(crate) fn field_as_integer(&self, index: c_int) -> c_int {
        // SAFETY: a live feature; OGR checks the index.
        unsafe { ogr::OGR_F_GetFieldAsInteger(self.raw(), index) }
    }

    pub(crate) fn field_as_integer64(&self, index: c_int) -> i64 {
        // SAFETY: a live feature; OGR checks the index.
        unsafe { ogr::OGR_F_GetFieldAsInteger64(self.raw(), index) }
    }

    pub(crate) fn field_as_double(&self, index: c_int) -> f64 {
        // SAFETY: a live feature; OGR checks the index.
        unsafe { ogr::OGR_F_GetFieldAsDouble(self.raw(), index) }
    }

    pub(crate) fn field_as_string(&self, index: c_int) -> String {
        // SAFETY: a live feature; OGR checks the index; the text is copied
        // at once.
        unsafe { cpl::decoded(ogr::OGR_F_GetFieldAsString(self.raw(), index)) }
    }

    /// `GetFieldAsDateTime(index, ...)`: None when OGR cannot read the field
    /// as one.
    pub(crate) fn field_as_date_time(&self, index: c_int) -> Option<FieldDateTime> {
        let mut out = FieldDateTime::default();
        let mut time_zone: c_int = 0;
        // SAFETY: a live feature, an index OGR checks, and seven out values.
        let read = unsafe {
            ogr::OGR_F_GetFieldAsDateTimeEx(
                self.raw(),
                index,
                &raw mut out.year,
                &raw mut out.month,
                &raw mut out.day,
                &raw mut out.hour,
                &raw mut out.minute,
                &raw mut out.second,
                &raw mut time_zone,
            )
        };
        (read != cpl::FALSE).then_some(out)
    }

    pub(crate) fn set_field_null(&mut self, index: c_int) {
        // SAFETY: a live feature; OGR checks the index.
        unsafe { ogr::OGR_F_SetFieldNull(self.raw(), index) };
    }

    pub(crate) fn set_field_integer(&mut self, index: c_int, value: c_int) {
        // SAFETY: a live feature; OGR checks the index.
        unsafe { ogr::OGR_F_SetFieldInteger(self.raw(), index, value) };
    }

    pub(crate) fn set_field_integer64(&mut self, index: c_int, value: i64) {
        // SAFETY: a live feature; OGR checks the index.
        unsafe { ogr::OGR_F_SetFieldInteger64(self.raw(), index, value) };
    }

    pub(crate) fn set_field_double(&mut self, index: c_int, value: f64) {
        // SAFETY: a live feature; OGR checks the index.
        unsafe { ogr::OGR_F_SetFieldDouble(self.raw(), index, value) };
    }

    pub(crate) fn set_field_string(&mut self, index: c_int, value: &CStr) {
        // SAFETY: a live feature, an index OGR checks, and a NUL-terminated
        // value, which it copies.
        unsafe { ogr::OGR_F_SetFieldString(self.raw(), index, value.as_ptr()) };
    }

    pub(crate) fn set_style_string(&mut self, style: &CStr) {
        // SAFETY: a live feature and a NUL-terminated style, which it copies.
        unsafe { ogr::OGR_F_SetStyleString(self.raw(), style.as_ptr()) };
    }
}

impl Drop for Feature<'_> {
    fn drop(&mut self) {
        // SAFETY: the feature this value owns, destroyed once; a layer given
        // it took a copy.
        unsafe { ogr::OGR_F_Destroy(self.raw()) };
    }
}

// ---- the utilities (gdal_utils.h) ----------------------------------------------------------

// A utility's options, parsed from its arguments by GDAL; NULL when GDAL
// refused them (its reason left as the last error), which the utility then
// takes for its defaults. Freed on drop.
macro_rules! utility_options {
    ($name:ident, $raw:ty, $new:path, $free:path) => {
        pub(crate) struct $name(Option<NonNull<$raw>>);

        impl $name {
            /// The options of `arguments`, GDAL's command-line words.
            pub(crate) fn new(arguments: &CStringList) -> Self {
                // SAFETY: a NULL or NULL-terminated argument list, which GDAL
                // copies; the options are owned by the value made.
                Self(NonNull::new(unsafe { $new(arguments.as_argv(), ptr::null_mut()) }))
            }

            fn raw(&self) -> *mut $raw {
                self.0.map_or(ptr::null_mut(), NonNull::as_ptr)
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                if let Some(options) = self.0 {
                    // SAFETY: the options GDAL parsed for this value, freed once.
                    unsafe { $free(options.as_ptr()) };
                }
            }
        }
    };
}

utility_options!(
    TranslateOptions,
    gdal::GdalTranslateOptions,
    gdal::GDALTranslateOptionsNew,
    gdal::GDALTranslateOptionsFree
);
utility_options!(
    WarpAppOptions,
    gdal::GdalWarpAppOptions,
    gdal::GDALWarpAppOptionsNew,
    gdal::GDALWarpAppOptionsFree
);
utility_options!(
    VectorTranslateOptions,
    gdal::GdalVectorTranslateOptions,
    gdal::GDALVectorTranslateOptionsNew,
    gdal::GDALVectorTranslateOptionsFree
);
utility_options!(
    BuildVrtOptions,
    gdal::GdalBuildVrtOptions,
    gdal::GDALBuildVRTOptionsNew,
    gdal::GDALBuildVRTOptionsFree
);

/// `GDALTranslate(destination, source, options, NULL)`: the result, owned.
pub(crate) fn translate(
    destination: &CStr,
    source: &DatasetHandle,
    options: &TranslateOptions,
) -> Option<Dataset> {
    // SAFETY: a NUL-terminated name, a live source, and the options (NULL
    // takes GDAL's defaults); the dataset is owned by the result.
    let made = unsafe {
        gdal::GDALTranslate(destination.as_ptr(), source.raw(), options.raw(), ptr::null_mut())
    };
    NonNull::new(made).map(|handle| Dataset(DatasetHandle(handle)))
}

/// `GDALWarp(destination, NULL, sources, options, &usage_error)`, with
/// `progress` called while the warp runs (from GDAL's threads too): the
/// result, owned. The callback is taken off the options again before this
/// returns, so the options never hold a pointer to it after.
pub(crate) fn warp(
    destination: &CStr,
    sources: &[&DatasetHandle],
    options: &WarpAppOptions,
    progress: Progress<'_>,
) -> Option<Dataset> {
    let mut handles: Vec<gdal::GdalDatasetH> = sources.iter().map(|source| source.raw()).collect();
    let count = c_int::try_from(handles.len()).unwrap_or(c_int::MAX);
    let data = ptr::from_ref(&progress).cast_mut().cast::<c_void>();
    let hooked = !options.raw().is_null();
    if hooked {
        // SAFETY: live options, the trampoline, and the Progress on this
        // stack frame, alive until the warp returns and unhooked after.
        unsafe {
            gdal::GDALWarpAppOptionsSetProgress(options.raw(), Some(progress_trampoline), data);
        }
    }
    let mut usage_error: c_int = cpl::FALSE;
    // SAFETY: a NUL-terminated name, no destination dataset, `count` live
    // source handles (at most the array's length), the options (NULL takes
    // GDAL's defaults) and an out flag; the dataset returned is the result's.
    let warped = unsafe {
        gdal::GDALWarp(
            destination.as_ptr(),
            ptr::null_mut(),
            count,
            handles.as_mut_ptr(),
            options.raw(),
            &raw mut usage_error,
        )
    };
    if hooked {
        // SAFETY: live options; no callback is left pointing at this frame.
        unsafe { gdal::GDALWarpAppOptionsSetProgress(options.raw(), None, ptr::null_mut()) };
    }
    NonNull::new(warped).map(|handle| Dataset(DatasetHandle(handle)))
}

/// `GDALVectorTranslate(destination, NULL, sources, options, &usage_error)`:
/// the result, owned.
pub(crate) fn vector_translate(
    destination: &CStr,
    sources: &[&DatasetHandle],
    options: &VectorTranslateOptions,
) -> Option<Dataset> {
    let mut handles: Vec<gdal::GdalDatasetH> = sources.iter().map(|source| source.raw()).collect();
    let count = c_int::try_from(handles.len()).unwrap_or(c_int::MAX);
    let mut usage_error: c_int = cpl::FALSE;
    // SAFETY: a NUL-terminated name, no destination dataset, `count` live
    // sources, the options (NULL takes GDAL's defaults) and an out flag; the
    // dataset returned is the result's.
    let made = unsafe {
        gdal::GDALVectorTranslate(
            destination.as_ptr(),
            ptr::null_mut(),
            count,
            handles.as_mut_ptr(),
            options.raw(),
            &raw mut usage_error,
        )
    };
    NonNull::new(made).map(|handle| Dataset(DatasetHandle(handle)))
}

/// `GDALBuildVRT(destination, names, NULL, options, NULL)` over every name
/// of the list: the result, owned.
pub(crate) fn build_vrt(
    destination: &CStr,
    names: &CStringList,
    options: &BuildVrtOptions,
) -> Option<Dataset> {
    let count = c_int::try_from(names.len()).unwrap_or(c_int::MAX);
    // SAFETY: a NUL-terminated name, `count` NUL-terminated source names (at
    // most the list's), no open sources, and the options (NULL takes GDAL's
    // defaults); the dataset returned is the result's.
    let made = unsafe {
        gdal::GDALBuildVRT(
            destination.as_ptr(),
            count,
            ptr::null_mut(),
            names.as_ptr(),
            options.raw(),
            ptr::null_mut(),
        )
    };
    NonNull::new(made).map(|handle| Dataset(DatasetHandle(handle)))
}
