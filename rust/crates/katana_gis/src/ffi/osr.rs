//! OGR's spatial reference C API (ogr_srs_api.h): exactly the functions,
//! types and constants the C++ of src/katana_io calls through
//! `OGRSpatialReference` and `OGRCoordinateTransformation`, each by the C
//! function that method is, written by hand from GDAL 3.13.2's header
//! (ffi/cpl.rs says how the ffi folder is laid out and checked).
//!
//! `OGRSpatialReference::SET_FROM_USER_INPUT_LIMITATIONS_get()` has no C
//! twin: it is a C++ static member. Its list is copied here, as
//! `SET_FROM_USER_INPUT_LIMITATIONS`, from libgdal 3.13.2 itself - the
//! exported array `OGRSpatialReference::SET_FROM_USER_INPUT_LIMITATIONS`
//! holds two pointers and a NULL, relocated to "ALLOW_NETWORK_ACCESS=NO" and
//! "ALLOW_FILE_ACCESS=NO" (read with readelf and objdump on 2026-11-10) - and
//! handed to `OSRSetFromUserInputEx`, which is what the C++ overload calls.

#![allow(
    unsafe_code,
    reason = "The declarations of OGR's spatial reference API; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's values are declared whole where the C++ names them; native/ffi_check.c holds each."
)]

use std::ffi::{CStr, c_char, c_int};
use std::marker::{PhantomData, PhantomPinned};

use super::ogr::OgrErr;

/// `OGRSpatialReferenceH`'s object.
#[repr(C)]
pub(crate) struct OgrSpatialReferenceHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type OgrSpatialReferenceH = *mut OgrSpatialReferenceHs;

/// `OGRCoordinateTransformationH`'s object.
#[repr(C)]
pub(crate) struct OgrCoordinateTransformationHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type OgrCoordinateTransformationH = *mut OgrCoordinateTransformationHs;

/// `OSRAxisMappingStrategy`.
pub(crate) type OsrAxisMappingStrategy = c_int;
pub(crate) const OAMS_TRADITIONAL_GIS_ORDER: OsrAxisMappingStrategy = 0;

/// `OGRSpatialReference::SET_FROM_USER_INPUT_LIMITATIONS`: nothing fetched
/// from the network or a file while a definition is read (the module's
/// documentation says where the list was read from).
pub(crate) const SET_FROM_USER_INPUT_LIMITATIONS: [&CStr; 2] =
    [c"ALLOW_NETWORK_ACCESS=NO", c"ALLOW_FILE_ACCESS=NO"];

#[link(name = "gdal")]
#[allow(clippy::too_many_arguments, reason = "The signatures are OGR's.")]
unsafe extern "C" {
    pub(crate) fn OSRNewSpatialReference(wkt: *const c_char) -> OgrSpatialReferenceH;
    pub(crate) fn OSRDestroySpatialReference(reference: OgrSpatialReferenceH);
    pub(crate) fn OSRSetAxisMappingStrategy(
        reference: OgrSpatialReferenceH,
        strategy: OsrAxisMappingStrategy,
    );
    pub(crate) fn OSRGetDataAxisToSRSAxisMapping(
        reference: OgrSpatialReferenceH,
        count: *mut c_int,
    ) -> *const c_int;
    /// Advances `*input` past what it read; reads, never writes, the text.
    pub(crate) fn OSRImportFromWkt(
        reference: OgrSpatialReferenceH,
        input: *mut *mut c_char,
    ) -> OgrErr;
    pub(crate) fn OSRSetFromUserInputEx(
        reference: OgrSpatialReferenceH,
        definition: *const c_char,
        options: *const *const c_char,
    ) -> OgrErr;
    pub(crate) fn OSRExportToWkt(
        reference: OgrSpatialReferenceH,
        result: *mut *mut c_char,
    ) -> OgrErr;
    pub(crate) fn OSRExportToWktEx(
        reference: OgrSpatialReferenceH,
        result: *mut *mut c_char,
        options: *const *const c_char,
    ) -> OgrErr;
    pub(crate) fn OSRSetWellKnownGeogCS(
        reference: OgrSpatialReferenceH,
        name: *const c_char,
    ) -> OgrErr;
    pub(crate) fn OSRIsGeographic(reference: OgrSpatialReferenceH) -> c_int;
    pub(crate) fn OSRIsSame(first: OgrSpatialReferenceH, second: OgrSpatialReferenceH) -> c_int;
    pub(crate) fn OSRIsSameEx(
        first: OgrSpatialReferenceH,
        second: OgrSpatialReferenceH,
        options: *const *const c_char,
    ) -> c_int;
    pub(crate) fn OSRGetAxesCount(reference: OgrSpatialReferenceH) -> c_int;
    pub(crate) fn OSRDemoteTo2D(reference: OgrSpatialReferenceH, name: *const c_char) -> OgrErr;
    pub(crate) fn OSRGetName(reference: OgrSpatialReferenceH) -> *const c_char;
    pub(crate) fn OSRGetAuthorityName(
        reference: OgrSpatialReferenceH,
        target_key: *const c_char,
    ) -> *const c_char;
    pub(crate) fn OSRGetAuthorityCode(
        reference: OgrSpatialReferenceH,
        target_key: *const c_char,
    ) -> *const c_char;
    pub(crate) fn OSRAutoIdentifyEPSG(reference: OgrSpatialReferenceH) -> OgrErr;
    pub(crate) fn OSRFindMatches(
        reference: OgrSpatialReferenceH,
        options: *const *const c_char,
        entries: *mut c_int,
        confidence: *mut *mut c_int,
    ) -> *mut OgrSpatialReferenceH;
    pub(crate) fn OSRFreeSRSArray(references: *mut OgrSpatialReferenceH);

    pub(crate) fn OCTNewCoordinateTransformation(
        source: OgrSpatialReferenceH,
        target: OgrSpatialReferenceH,
    ) -> OgrCoordinateTransformationH;
    pub(crate) fn OCTDestroyCoordinateTransformation(transform: OgrCoordinateTransformationH);
    pub(crate) fn OCTTransform(
        transform: OgrCoordinateTransformationH,
        count: c_int,
        x: *mut f64,
        y: *mut f64,
        z: *mut f64,
    ) -> c_int;
    pub(crate) fn OCTTransformEx(
        transform: OgrCoordinateTransformationH,
        count: c_int,
        x: *mut f64,
        y: *mut f64,
        z: *mut f64,
        success: *mut c_int,
    ) -> c_int;
    pub(crate) fn OCTTransformBounds(
        transform: OgrCoordinateTransformationH,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        out_min_x: *mut f64,
        out_min_y: *mut f64,
        out_max_x: *mut f64,
        out_max_y: *mut f64,
        densify_points: c_int,
    ) -> c_int;
}
