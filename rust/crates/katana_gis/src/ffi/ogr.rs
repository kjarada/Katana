//! OGR's vector C API (ogr_api.h, ogr_core.h): exactly the functions, types
//! and constants the C++ of src/katana_io calls through `OGRGeometry` and its
//! subclasses, `OGRLayer`, `OGRFeature`, `OGRFeatureDefn` and `OGRFieldDefn`,
//! each by the C function that method is, written by hand from GDAL 3.13.2's
//! headers (ffi/cpl.rs says how the ffi folder is laid out and checked).
//!
//! The C API differs from the C++ in one way the adapter relies on:
//! `OGR_G_AddGeometryDirectly` deletes the geometry it is handed when it
//! refuses it, where the C++ `addGeometryDirectly` leaves it to the caller
//! (ogr_detail.rs, `OgrGeometry::add_geometry_directly`).

#![allow(
    unsafe_code,
    reason = "The declarations of OGR's C API; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's values are declared whole where the C++ names them; native/ffi_check.c holds each."
)]

use std::ffi::{CStr, c_char, c_int};
use std::marker::{PhantomData, PhantomPinned};

use super::osr::OgrSpatialReferenceH;

/// `OGRGeometryH`'s object.
#[repr(C)]
pub(crate) struct OgrGeometryHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type OgrGeometryH = *mut OgrGeometryHs;

/// `OGRLayerH`'s object.
#[repr(C)]
pub(crate) struct OgrLayerHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type OgrLayerH = *mut OgrLayerHs;

/// `OGRFeatureH`'s object.
#[repr(C)]
pub(crate) struct OgrFeatureHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type OgrFeatureH = *mut OgrFeatureHs;

/// `OGRFeatureDefnH`'s object.
#[repr(C)]
pub(crate) struct OgrFeatureDefnHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type OgrFeatureDefnH = *mut OgrFeatureDefnHs;

/// `OGRFieldDefnH`'s object.
#[repr(C)]
pub(crate) struct OgrFieldDefnHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type OgrFieldDefnH = *mut OgrFieldDefnHs;

/// `OGRErr`: an `int` (ogr_core.h, outside its STRICT_OGRERR_TYPE).
pub(crate) type OgrErr = c_int;
pub(crate) const OGRERR_NONE: OgrErr = 0;

/// `OGRwkbGeometryType`: the enumerators the C++ names, at their ISO values.
pub(crate) type OgrWkbGeometryType = c_int;
pub(crate) const WKB_UNKNOWN: OgrWkbGeometryType = 0;
pub(crate) const WKB_POINT: OgrWkbGeometryType = 1;
pub(crate) const WKB_LINE_STRING: OgrWkbGeometryType = 2;
pub(crate) const WKB_POLYGON: OgrWkbGeometryType = 3;
pub(crate) const WKB_MULTI_POINT: OgrWkbGeometryType = 4;
pub(crate) const WKB_MULTI_LINE_STRING: OgrWkbGeometryType = 5;
pub(crate) const WKB_MULTI_POLYGON: OgrWkbGeometryType = 6;
pub(crate) const WKB_GEOMETRY_COLLECTION: OgrWkbGeometryType = 7;
pub(crate) const WKB_CIRCULAR_STRING: OgrWkbGeometryType = 8;
pub(crate) const WKB_COMPOUND_CURVE: OgrWkbGeometryType = 9;
pub(crate) const WKB_CURVE_POLYGON: OgrWkbGeometryType = 10;
pub(crate) const WKB_MULTI_CURVE: OgrWkbGeometryType = 11;
pub(crate) const WKB_MULTI_SURFACE: OgrWkbGeometryType = 12;
pub(crate) const WKB_POLYHEDRAL_SURFACE: OgrWkbGeometryType = 15;
pub(crate) const WKB_TIN: OgrWkbGeometryType = 16;
pub(crate) const WKB_TRIANGLE: OgrWkbGeometryType = 17;
pub(crate) const WKB_LINEAR_RING: OgrWkbGeometryType = 101;

/// `OGRFieldType`.
pub(crate) type OgrFieldType = c_int;
pub(crate) const OFT_INTEGER: OgrFieldType = 0;
pub(crate) const OFT_REAL: OgrFieldType = 2;
pub(crate) const OFT_STRING: OgrFieldType = 4;
pub(crate) const OFT_DATE: OgrFieldType = 9;
pub(crate) const OFT_DATE_TIME: OgrFieldType = 11;
pub(crate) const OFT_INTEGER64: OgrFieldType = 12;

/// `OGRFieldSubType`.
pub(crate) type OgrFieldSubType = c_int;
pub(crate) const OFST_NONE: OgrFieldSubType = 0;
pub(crate) const OFST_BOOLEAN: OgrFieldSubType = 1;

/// Capabilities, by name (ogr_core.h).
pub(crate) const OLC_CREATE_FIELD: &CStr = c"CreateField";
pub(crate) const ODS_C_CREATE_LAYER: &CStr = c"CreateLayer";
pub(crate) const ODS_C_TRANSACTIONS: &CStr = c"Transactions";

#[link(name = "gdal")]
#[allow(clippy::too_many_arguments, reason = "The signatures are OGR's.")]
unsafe extern "C" {
    // ---- geometry types (ogr_core.h) ----------------------------------------------
    /// What `wkbFlatten` expands to.
    pub(crate) fn OGR_GT_Flatten(geometry_type: OgrWkbGeometryType) -> OgrWkbGeometryType;
    /// What `wkbHasZ` calls.
    pub(crate) fn OGR_GT_HasZ(geometry_type: OgrWkbGeometryType) -> c_int;
    /// What `wkbSetZ` expands to.
    pub(crate) fn OGR_GT_SetZ(geometry_type: OgrWkbGeometryType) -> OgrWkbGeometryType;
    pub(crate) fn OGRGeometryTypeToName(geometry_type: OgrWkbGeometryType) -> *const c_char;

    // ---- geometries ---------------------------------------------------------------
    pub(crate) fn OGR_G_CreateGeometry(geometry_type: OgrWkbGeometryType) -> OgrGeometryH;
    pub(crate) fn OGR_G_DestroyGeometry(geometry: OgrGeometryH);
    pub(crate) fn OGR_G_GetGeometryType(geometry: OgrGeometryH) -> OgrWkbGeometryType;
    pub(crate) fn OGR_G_IsEmpty(geometry: OgrGeometryH) -> c_int;
    pub(crate) fn OGR_G_Is3D(geometry: OgrGeometryH) -> c_int;
    pub(crate) fn OGR_G_GetLinearGeometry(
        geometry: OgrGeometryH,
        max_angle_step_degrees: f64,
        options: *const *const c_char,
    ) -> OgrGeometryH;
    pub(crate) fn OGR_G_GetPointCount(geometry: OgrGeometryH) -> c_int;
    pub(crate) fn OGR_G_GetPoint(
        geometry: OgrGeometryH,
        index: c_int,
        x: *mut f64,
        y: *mut f64,
        z: *mut f64,
    );
    pub(crate) fn OGR_G_SetPoint(
        geometry: OgrGeometryH,
        index: c_int,
        x: f64,
        y: f64,
        z: f64,
    ) -> OgrErr;
    pub(crate) fn OGR_G_SetPoint_2D(geometry: OgrGeometryH, index: c_int, x: f64, y: f64)
    -> OgrErr;
    pub(crate) fn OGR_G_AddPoint(geometry: OgrGeometryH, x: f64, y: f64, z: f64) -> OgrErr;
    pub(crate) fn OGR_G_AddPoint_2D(geometry: OgrGeometryH, x: f64, y: f64) -> OgrErr;
    pub(crate) fn OGR_G_GetGeometryCount(geometry: OgrGeometryH) -> c_int;
    pub(crate) fn OGR_G_GetGeometryRef(geometry: OgrGeometryH, index: c_int) -> OgrGeometryH;
    pub(crate) fn OGR_G_AddGeometryDirectly(geometry: OgrGeometryH, member: OgrGeometryH)
    -> OgrErr;

    // ---- layers ---------------------------------------------------------------------
    pub(crate) fn OGR_L_GetName(layer: OgrLayerH) -> *const c_char;
    pub(crate) fn OGR_L_GetGeomType(layer: OgrLayerH) -> OgrWkbGeometryType;
    pub(crate) fn OGR_L_GetSpatialRef(layer: OgrLayerH) -> OgrSpatialReferenceH;
    pub(crate) fn OGR_L_GetLayerDefn(layer: OgrLayerH) -> OgrFeatureDefnH;
    pub(crate) fn OGR_L_ResetReading(layer: OgrLayerH);
    pub(crate) fn OGR_L_GetNextFeature(layer: OgrLayerH) -> OgrFeatureH;
    pub(crate) fn OGR_L_SetSpatialFilterRect(
        layer: OgrLayerH,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    );
    pub(crate) fn OGR_L_SetSpatialFilter(layer: OgrLayerH, filter: OgrGeometryH);
    pub(crate) fn OGR_L_SetAttributeFilter(layer: OgrLayerH, filter: *const c_char) -> OgrErr;
    pub(crate) fn OGR_L_GetFeatureCount(layer: OgrLayerH, force: c_int) -> i64;
    pub(crate) fn OGR_L_TestCapability(layer: OgrLayerH, capability: *const c_char) -> c_int;
    pub(crate) fn OGR_L_CreateField(
        layer: OgrLayerH,
        field: OgrFieldDefnH,
        approx_ok: c_int,
    ) -> OgrErr;
    pub(crate) fn OGR_L_CreateFeature(layer: OgrLayerH, feature: OgrFeatureH) -> OgrErr;

    // ---- features -------------------------------------------------------------------
    pub(crate) fn OGR_F_Create(definition: OgrFeatureDefnH) -> OgrFeatureH;
    pub(crate) fn OGR_F_Destroy(feature: OgrFeatureH);
    pub(crate) fn OGR_F_GetGeometryRef(feature: OgrFeatureH) -> OgrGeometryH;
    pub(crate) fn OGR_F_SetGeometryDirectly(feature: OgrFeatureH, geometry: OgrGeometryH)
    -> OgrErr;
    pub(crate) fn OGR_F_IsFieldSetAndNotNull(feature: OgrFeatureH, field: c_int) -> c_int;
    pub(crate) fn OGR_F_GetFieldAsInteger(feature: OgrFeatureH, field: c_int) -> c_int;
    pub(crate) fn OGR_F_GetFieldAsInteger64(feature: OgrFeatureH, field: c_int) -> i64;
    pub(crate) fn OGR_F_GetFieldAsDouble(feature: OgrFeatureH, field: c_int) -> f64;
    pub(crate) fn OGR_F_GetFieldAsString(feature: OgrFeatureH, field: c_int) -> *const c_char;
    pub(crate) fn OGR_F_GetFieldAsDateTimeEx(
        feature: OgrFeatureH,
        field: c_int,
        year: *mut c_int,
        month: *mut c_int,
        day: *mut c_int,
        hour: *mut c_int,
        minute: *mut c_int,
        second: *mut f32,
        time_zone: *mut c_int,
    ) -> c_int;
    pub(crate) fn OGR_F_SetFieldNull(feature: OgrFeatureH, field: c_int);
    pub(crate) fn OGR_F_SetFieldInteger(feature: OgrFeatureH, field: c_int, value: c_int);
    pub(crate) fn OGR_F_SetFieldInteger64(feature: OgrFeatureH, field: c_int, value: i64);
    pub(crate) fn OGR_F_SetFieldDouble(feature: OgrFeatureH, field: c_int, value: f64);
    pub(crate) fn OGR_F_SetFieldString(feature: OgrFeatureH, field: c_int, value: *const c_char);
    pub(crate) fn OGR_F_SetStyleString(feature: OgrFeatureH, style: *const c_char);

    // ---- feature and field definitions ----------------------------------------------
    pub(crate) fn OGR_FD_GetFieldCount(definition: OgrFeatureDefnH) -> c_int;
    pub(crate) fn OGR_FD_GetFieldDefn(definition: OgrFeatureDefnH, field: c_int) -> OgrFieldDefnH;
    pub(crate) fn OGR_FD_GetFieldIndex(definition: OgrFeatureDefnH, name: *const c_char) -> c_int;
    pub(crate) fn OGR_Fld_Create(name: *const c_char, field_type: OgrFieldType) -> OgrFieldDefnH;
    pub(crate) fn OGR_Fld_Destroy(field: OgrFieldDefnH);
    pub(crate) fn OGR_Fld_SetSubType(field: OgrFieldDefnH, sub_type: OgrFieldSubType);
    pub(crate) fn OGR_Fld_GetNameRef(field: OgrFieldDefnH) -> *const c_char;
    pub(crate) fn OGR_Fld_GetType(field: OgrFieldDefnH) -> OgrFieldType;
    pub(crate) fn OGR_Fld_GetSubType(field: OgrFieldDefnH) -> OgrFieldSubType;
}
