//! GDAL's raster and dataset C API (gdal.h) and its utilities (gdal_utils.h):
//! exactly the functions, types and constants the C++ of src/katana_io calls
//! through `GDALDataset`, `GDALRasterBand`, `GDALDriver`,
//! `GDALDriverManager` and `GDALColorTable`, each by the C function that
//! method is (gdal.h's own wrappers), written by hand from GDAL 3.13.2's
//! headers (ffi/cpl.rs says how the ffi folder is laid out and checked).
//!
//! The handles are `void *` in C; each is an opaque type of its own here, so
//! a band is never handed where a dataset is meant. The "major object"
//! functions (`GDALGetDescription`, `GDALGetMetadata*`) take any of them,
//! cast.

#![allow(
    unsafe_code,
    reason = "The declarations of GDAL's C API; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's values and layouts are declared whole where the C++ names them; native/ffi_check.c holds each."
)]

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::marker::{PhantomData, PhantomPinned};

use super::cpl::CplErr;
use super::ogr::{OgrErr, OgrGeometryH, OgrLayerH, OgrWkbGeometryType};
use super::osr::OgrSpatialReferenceH;

/// `GDALDatasetH`'s object.
#[repr(C)]
pub(crate) struct GdalDatasetHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalDatasetH = *mut GdalDatasetHs;

/// `GDALDriverH`'s object.
#[repr(C)]
pub(crate) struct GdalDriverHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalDriverH = *mut GdalDriverHs;

/// `GDALRasterBandH`'s object.
#[repr(C)]
pub(crate) struct GdalRasterBandHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalRasterBandH = *mut GdalRasterBandHs;

/// `GDALColorTableH`'s object.
#[repr(C)]
pub(crate) struct GdalColorTableHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalColorTableH = *mut GdalColorTableHs;

/// `GDALMajorObjectH`: a dataset, a band or a driver.
pub(crate) type GdalMajorObjectH = *mut c_void;

/// `GDALTranslateOptions`.
#[repr(C)]
pub(crate) struct GdalTranslateOptions {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `GDALWarpAppOptions`.
#[repr(C)]
pub(crate) struct GdalWarpAppOptions {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `GDALVectorTranslateOptions`.
#[repr(C)]
pub(crate) struct GdalVectorTranslateOptions {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `GDALBuildVRTOptions`.
#[repr(C)]
pub(crate) struct GdalBuildVrtOptions {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// The `...ForBinary` structs of gdal_utils.h; only ever passed as NULL.
#[repr(C)]
pub(crate) struct GdalUtilityForBinary {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `GDALProgressFunc` (cpl_progress.h).
pub(crate) type GdalProgressFunc =
    Option<unsafe extern "C" fn(complete: f64, message: *const c_char, data: *mut c_void) -> c_int>;

/// `GDALColorEntry`, every member in the header's order.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct GdalColorEntry {
    pub(crate) c1: i16,
    pub(crate) c2: i16,
    pub(crate) c3: i16,
    pub(crate) c4: i16,
}

const _: () = assert!(std::mem::size_of::<GdalColorEntry>() == 8);
const _: () = assert!(std::mem::offset_of!(GdalColorEntry, c2) == 2);
const _: () = assert!(std::mem::offset_of!(GdalColorEntry, c3) == 4);
const _: () = assert!(std::mem::offset_of!(GdalColorEntry, c4) == 6);

/// `GDALDataType`. GDT_Byte is GDT_UInt8's name before GDAL 3.13 (gdal.h).
pub(crate) type GdalDataType = c_int;
pub(crate) const GDT_UNKNOWN: GdalDataType = 0;
pub(crate) const GDT_BYTE: GdalDataType = 1;
pub(crate) const GDT_FLOAT64: GdalDataType = 7;

/// `GDALRWFlag`.
pub(crate) type GdalRwFlag = c_int;
pub(crate) const GF_READ: GdalRwFlag = 0;
pub(crate) const GF_WRITE: GdalRwFlag = 1;

/// `GDALColorInterp`.
pub(crate) type GdalColorInterp = c_int;
pub(crate) const GCI_GRAY_INDEX: GdalColorInterp = 1;
pub(crate) const GCI_PALETTE_INDEX: GdalColorInterp = 2;
pub(crate) const GCI_RED_BAND: GdalColorInterp = 3;
pub(crate) const GCI_GREEN_BAND: GdalColorInterp = 4;
pub(crate) const GCI_BLUE_BAND: GdalColorInterp = 5;
pub(crate) const GCI_ALPHA_BAND: GdalColorInterp = 6;

/// `GDALRIOResampleAlg`.
pub(crate) type GdalRioResampleAlg = c_int;
pub(crate) const GRIORA_NEAREST_NEIGHBOUR: GdalRioResampleAlg = 0;
pub(crate) const GRIORA_BILINEAR: GdalRioResampleAlg = 1;
pub(crate) const GRIORA_CUBIC: GdalRioResampleAlg = 2;
pub(crate) const GRIORA_CUBIC_SPLINE: GdalRioResampleAlg = 3;

/// `GDALOpenEx`'s flags.
pub(crate) const GDAL_OF_READONLY: c_uint = 0x00;
pub(crate) const GDAL_OF_UPDATE: c_uint = 0x01;
pub(crate) const GDAL_OF_RASTER: c_uint = 0x02;
pub(crate) const GDAL_OF_VECTOR: c_uint = 0x04;

/// A driver's capabilities and metadata, by name.
pub(crate) const GDAL_DCAP_OPEN: &CStr = c"DCAP_OPEN";
pub(crate) const GDAL_DCAP_CREATE: &CStr = c"DCAP_CREATE";
pub(crate) const GDAL_DCAP_CREATECOPY: &CStr = c"DCAP_CREATECOPY";
pub(crate) const GDAL_DCAP_VIRTUALIO: &CStr = c"DCAP_VIRTUALIO";
pub(crate) const GDAL_DCAP_RASTER: &CStr = c"DCAP_RASTER";
pub(crate) const GDAL_DCAP_VECTOR: &CStr = c"DCAP_VECTOR";
pub(crate) const GDAL_DCAP_CREATE_LAYER: &CStr = c"DCAP_CREATE_LAYER";
pub(crate) const GDAL_DCAP_CREATE_FIELD: &CStr = c"DCAP_CREATE_FIELD";
pub(crate) const GDAL_DMD_LONGNAME: &CStr = c"DMD_LONGNAME";
pub(crate) const GDAL_DMD_HELPTOPIC: &CStr = c"DMD_HELPTOPIC";
pub(crate) const GDAL_DMD_EXTENSION: &CStr = c"DMD_EXTENSION";
pub(crate) const GDAL_DMD_CONNECTION_PREFIX: &CStr = c"DMD_CONNECTION_PREFIX";
pub(crate) const GDAL_DMD_EXTENSIONS: &CStr = c"DMD_EXTENSIONS";
pub(crate) const GDAL_DMD_CREATIONOPTIONLIST: &CStr = c"DMD_CREATIONOPTIONLIST";
pub(crate) const GDAL_DMD_OPENOPTIONLIST: &CStr = c"DMD_OPENOPTIONLIST";
pub(crate) const GDAL_DMD_CREATIONDATATYPES: &CStr = c"DMD_CREATIONDATATYPES";
pub(crate) const GDAL_DMD_CREATIONFIELDDATATYPES: &CStr = c"DMD_CREATIONFIELDDATATYPES";
pub(crate) const GDAL_DMD_CREATIONFIELDDATASUBTYPES: &CStr = c"DMD_CREATIONFIELDDATASUBTYPES";
pub(crate) const GDAL_DS_LAYER_CREATIONOPTIONLIST: &CStr = c"DS_LAYER_CREATIONOPTIONLIST";

#[link(name = "gdal")]
#[allow(clippy::too_many_arguments, reason = "The signatures are GDAL's.")]
unsafe extern "C" {
    // ---- drivers and datasets -----------------------------------------------------
    pub(crate) fn GDALAllRegister();
    pub(crate) fn GDALOpenEx(
        filename: *const c_char,
        open_flags: c_uint,
        allowed_drivers: *const *const c_char,
        open_options: *const *const c_char,
        sibling_files: *const *const c_char,
    ) -> GdalDatasetH;
    pub(crate) fn GDALClose(dataset: GdalDatasetH) -> CplErr;
    pub(crate) fn GDALGetDriverByName(name: *const c_char) -> GdalDriverH;
    pub(crate) fn GDALGetDriverCount() -> c_int;
    pub(crate) fn GDALGetDriver(index: c_int) -> GdalDriverH;
    pub(crate) fn GDALIdentifyDriverEx(
        filename: *const c_char,
        identify_flags: c_uint,
        allowed_drivers: *const *const c_char,
        file_list: *const *const c_char,
    ) -> GdalDriverH;
    pub(crate) fn GDALGetOutputDriversForDatasetName(
        destination: *const c_char,
        raster_or_vector: c_int,
        single_match: bool,
        emit_warning: bool,
    ) -> *mut *mut c_char;
    pub(crate) fn GDALCreate(
        driver: GdalDriverH,
        filename: *const c_char,
        x_size: c_int,
        y_size: c_int,
        bands: c_int,
        band_type: GdalDataType,
        options: *const *const c_char,
    ) -> GdalDatasetH;
    pub(crate) fn GDALCreateCopy(
        driver: GdalDriverH,
        filename: *const c_char,
        source: GdalDatasetH,
        strict: c_int,
        options: *const *const c_char,
        progress: GdalProgressFunc,
        progress_data: *mut c_void,
    ) -> GdalDatasetH;
    pub(crate) fn GDALDeleteDataset(driver: GdalDriverH, filename: *const c_char) -> CplErr;
    pub(crate) fn GDALGetDatasetDriver(dataset: GdalDatasetH) -> GdalDriverH;
    pub(crate) fn GDALVersionInfo(request: *const c_char) -> *const c_char;

    // ---- major objects ----------------------------------------------------------------
    pub(crate) fn GDALGetDescription(object: GdalMajorObjectH) -> *const c_char;
    pub(crate) fn GDALGetMetadata(
        object: GdalMajorObjectH,
        domain: *const c_char,
    ) -> *const *const c_char;
    pub(crate) fn GDALGetMetadataItem(
        object: GdalMajorObjectH,
        name: *const c_char,
        domain: *const c_char,
    ) -> *const c_char;

    // ---- rasters --------------------------------------------------------------------
    pub(crate) fn GDALGetRasterXSize(dataset: GdalDatasetH) -> c_int;
    pub(crate) fn GDALGetRasterYSize(dataset: GdalDatasetH) -> c_int;
    pub(crate) fn GDALGetRasterCount(dataset: GdalDatasetH) -> c_int;
    pub(crate) fn GDALGetRasterBand(dataset: GdalDatasetH, band: c_int) -> GdalRasterBandH;
    pub(crate) fn GDALGetProjectionRef(dataset: GdalDatasetH) -> *const c_char;
    pub(crate) fn GDALGetSpatialRef(dataset: GdalDatasetH) -> OgrSpatialReferenceH;
    pub(crate) fn GDALSetSpatialRef(
        dataset: GdalDatasetH,
        reference: OgrSpatialReferenceH,
    ) -> CplErr;
    pub(crate) fn GDALGetGeoTransform(dataset: GdalDatasetH, transform: *mut f64) -> CplErr;
    pub(crate) fn GDALSetGeoTransform(dataset: GdalDatasetH, transform: *const f64) -> CplErr;
    pub(crate) fn GDALInvGeoTransform(transform: *const f64, inverse: *mut f64) -> c_int;
    pub(crate) fn GDALGetRasterNoDataValue(band: GdalRasterBandH, has_no_data: *mut c_int) -> f64;
    pub(crate) fn GDALSetRasterNoDataValue(band: GdalRasterBandH, no_data: f64) -> CplErr;
    pub(crate) fn GDALGetRasterDataType(band: GdalRasterBandH) -> GdalDataType;
    pub(crate) fn GDALGetDataTypeName(data_type: GdalDataType) -> *const c_char;
    pub(crate) fn GDALGetRasterColorInterpretation(band: GdalRasterBandH) -> GdalColorInterp;
    pub(crate) fn GDALGetRasterColorTable(band: GdalRasterBandH) -> GdalColorTableH;
    pub(crate) fn GDALGetColorEntryCount(table: GdalColorTableH) -> c_int;
    pub(crate) fn GDALGetColorEntry(table: GdalColorTableH, index: c_int) -> *const GdalColorEntry;
    pub(crate) fn GDALRasterIO(
        band: GdalRasterBandH,
        read_or_write: GdalRwFlag,
        x_offset: c_int,
        y_offset: c_int,
        x_size: c_int,
        y_size: c_int,
        buffer: *mut c_void,
        buffer_x_size: c_int,
        buffer_y_size: c_int,
        buffer_type: GdalDataType,
        pixel_space: c_int,
        line_space: c_int,
    ) -> CplErr;
    pub(crate) fn GDALRasterInterpolateAtPoint(
        band: GdalRasterBandH,
        pixel: f64,
        line: f64,
        interpolation: GdalRioResampleAlg,
        real: *mut f64,
        imaginary: *mut f64,
    ) -> CplErr;

    // ---- vector datasets --------------------------------------------------------------
    pub(crate) fn GDALDatasetGetLayerCount(dataset: GdalDatasetH) -> c_int;
    pub(crate) fn GDALDatasetGetLayer(dataset: GdalDatasetH, index: c_int) -> OgrLayerH;
    pub(crate) fn GDALDatasetGetLayerByName(
        dataset: GdalDatasetH,
        name: *const c_char,
    ) -> OgrLayerH;
    pub(crate) fn GDALDatasetDeleteLayer(dataset: GdalDatasetH, index: c_int) -> OgrErr;
    pub(crate) fn GDALDatasetCreateLayer(
        dataset: GdalDatasetH,
        name: *const c_char,
        reference: OgrSpatialReferenceH,
        geometry_type: OgrWkbGeometryType,
        options: *const *const c_char,
    ) -> OgrLayerH;
    pub(crate) fn GDALDatasetTestCapability(
        dataset: GdalDatasetH,
        capability: *const c_char,
    ) -> c_int;
    pub(crate) fn GDALDatasetStartTransaction(dataset: GdalDatasetH, force: c_int) -> OgrErr;
    pub(crate) fn GDALDatasetCommitTransaction(dataset: GdalDatasetH) -> OgrErr;
    pub(crate) fn GDALDatasetRollbackTransaction(dataset: GdalDatasetH) -> OgrErr;
    pub(crate) fn GDALDatasetExecuteSQL(
        dataset: GdalDatasetH,
        statement: *const c_char,
        spatial_filter: OgrGeometryH,
        dialect: *const c_char,
    ) -> OgrLayerH;
    pub(crate) fn GDALDatasetReleaseResultSet(dataset: GdalDatasetH, result: OgrLayerH);

    // ---- gdal_utils.h -----------------------------------------------------------------
    pub(crate) fn GDALTranslateOptionsNew(
        argv: *mut *mut c_char,
        for_binary: *mut GdalUtilityForBinary,
    ) -> *mut GdalTranslateOptions;
    pub(crate) fn GDALTranslateOptionsFree(options: *mut GdalTranslateOptions);
    pub(crate) fn GDALTranslate(
        destination: *const c_char,
        source: GdalDatasetH,
        options: *const GdalTranslateOptions,
        usage_error: *mut c_int,
    ) -> GdalDatasetH;
    pub(crate) fn GDALWarpAppOptionsNew(
        argv: *mut *mut c_char,
        for_binary: *mut GdalUtilityForBinary,
    ) -> *mut GdalWarpAppOptions;
    pub(crate) fn GDALWarpAppOptionsSetProgress(
        options: *mut GdalWarpAppOptions,
        progress: GdalProgressFunc,
        progress_data: *mut c_void,
    );
    pub(crate) fn GDALWarpAppOptionsFree(options: *mut GdalWarpAppOptions);
    pub(crate) fn GDALWarp(
        destination: *const c_char,
        destination_dataset: GdalDatasetH,
        source_count: c_int,
        sources: *mut GdalDatasetH,
        options: *const GdalWarpAppOptions,
        usage_error: *mut c_int,
    ) -> GdalDatasetH;
    pub(crate) fn GDALVectorTranslateOptionsNew(
        argv: *mut *mut c_char,
        for_binary: *mut GdalUtilityForBinary,
    ) -> *mut GdalVectorTranslateOptions;
    pub(crate) fn GDALVectorTranslateOptionsFree(options: *mut GdalVectorTranslateOptions);
    pub(crate) fn GDALVectorTranslate(
        destination: *const c_char,
        destination_dataset: GdalDatasetH,
        source_count: c_int,
        sources: *mut GdalDatasetH,
        options: *const GdalVectorTranslateOptions,
        usage_error: *mut c_int,
    ) -> GdalDatasetH;
    pub(crate) fn GDALBuildVRTOptionsNew(
        argv: *mut *mut c_char,
        for_binary: *mut GdalUtilityForBinary,
    ) -> *mut GdalBuildVrtOptions;
    pub(crate) fn GDALBuildVRTOptionsFree(options: *mut GdalBuildVrtOptions);
    pub(crate) fn GDALBuildVRT(
        destination: *const c_char,
        source_count: c_int,
        sources: *mut GdalDatasetH,
        source_names: *const *const c_char,
        options: *const GdalBuildVrtOptions,
        usage_error: *mut c_int,
    ) -> GdalDatasetH;
}
