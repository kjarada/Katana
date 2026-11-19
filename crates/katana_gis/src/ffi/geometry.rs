//! OGR's geometries as Rust owners and views (ogr_api.h), and the pure
//! functions of a geometry type: moved here from ogr_detail.rs so that the
//! conversions there are safe code.
//!
//! Who frees what:
//!
//! ```text
//! OgrGeometry      a geometry Rust owns: one GDAL made for the caller
//!                  (OGR_G_CreateGeometry, OGR_G_GetLinearGeometry); destroyed
//!                  on drop (OGR_G_DestroyGeometry), unless it is handed to
//!                  GDAL, which then owns it (add_geometry_directly,
//!                  Feature::set_geometry_directly)
//! GeometryView     a geometry another object owns - a feature's, a member of
//!                  a collection, a polygon's ring, a compound curve's curve -
//!                  borrowed for as long as its owner is; never freed here
//! ```
//!
//! The C API's one difference from the C++ here: `OGR_G_AddGeometryDirectly`
//! deletes a member it refuses, where the C++'s `addGeometryDirectly` leaves it
//! to the caller; so `add_geometry_directly` consumes the member either way.
//! A point is made by `OGR_G_SetPoint`, whose setters mark a point with a NaN
//! coordinate empty, where the C++ built it with `new OGRPoint(x, y[, z])`.

#![allow(
    unsafe_code,
    reason = "Owns OGR's geometries and calls their C API; every call states why it is sound."
)]

use std::ffi::c_int;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use super::srs::SpatialReferenceRef;
use super::{algorithm, cpl, ogr};

// ---- geometry types -------------------------------------------------------------------

/// `wkbFlatten`.
pub(crate) fn flattened(geometry_type: ogr::OgrWkbGeometryType) -> ogr::OgrWkbGeometryType {
    // SAFETY: a pure function of an integer.
    unsafe { ogr::OGR_GT_Flatten(geometry_type) }
}

/// `wkbHasZ`.
pub(crate) fn has_z(geometry_type: ogr::OgrWkbGeometryType) -> bool {
    // SAFETY: a pure function of an integer.
    unsafe { ogr::OGR_GT_HasZ(geometry_type) != 0 }
}

/// `wkbSetZ`.
pub(crate) fn with_z(geometry_type: ogr::OgrWkbGeometryType) -> ogr::OgrWkbGeometryType {
    // SAFETY: a pure function of an integer.
    unsafe { ogr::OGR_GT_SetZ(geometry_type) }
}

/// GDAL's name of a geometry type (`OGRGeometryTypeToName`).
pub(crate) fn geometry_type_name(geometry_type: ogr::OgrWkbGeometryType) -> String {
    // SAFETY: a static or thread-local string GDAL keeps, copied at once.
    unsafe { cpl::decoded(ogr::OGRGeometryTypeToName(geometry_type)) }
}

/// The version of GEOS GDAL was built with, when it was (`OGRGetGEOSVersion`).
pub(crate) fn geos_version() -> Option<(c_int, c_int, c_int)> {
    let (mut major, mut minor, mut patch): (c_int, c_int, c_int) = (0, 0, 0);
    // SAFETY: three out values.
    let built = unsafe {
        algorithm::OGRGetGEOSVersion(&raw mut major, &raw mut minor, &raw mut patch)
    };
    built.then_some((major, minor, patch))
}

// ---- owned geometries -----------------------------------------------------------------

/// An OGR geometry Rust owns.
pub(crate) struct OgrGeometry(NonNull<ogr::OgrGeometryHs>);

impl OgrGeometry {
    /// An empty geometry of the type (`new OGRPoint()` and so on).
    pub(crate) fn create(geometry_type: ogr::OgrWkbGeometryType) -> Self {
        // SAFETY: a geometry type; the result is owned by this value.
        let handle = unsafe { ogr::OGR_G_CreateGeometry(geometry_type) };
        Self(NonNull::new(handle).expect("GDAL makes every geometry type the crate names"))
    }

    /// Takes ownership of a geometry GDAL made for the caller.
    ///
    /// # Safety
    ///
    /// `handle` is NULL or a live geometry nothing else owns.
    pub(super) unsafe fn from_raw(handle: ogr::OgrGeometryH) -> Option<Self> {
        NonNull::new(handle).map(Self)
    }

    pub(super) fn raw(&self) -> ogr::OgrGeometryH {
        self.0.as_ptr()
    }

    /// Hands the geometry to GDAL, which owns it from here.
    pub(super) fn into_raw(self) -> ogr::OgrGeometryH {
        let handle = self.raw();
        std::mem::forget(self);
        handle
    }

    pub(crate) fn view(&self) -> GeometryView<'_> {
        GeometryView {
            handle: self.0,
            _owner: PhantomData,
        }
    }

    pub(crate) fn is_3d(&self) -> bool {
        self.view().is_3d()
    }

    /// `addPoint(x, y, z)` on a line, ring or circular string.
    pub(crate) fn add_point(&self, x: f64, y: f64, z: f64) {
        // SAFETY: a live geometry this value owns.
        unsafe { ogr::OGR_G_AddPoint(self.raw(), x, y, z) };
    }

    /// `addPoint(x, y)`: the geometry stays 2D.
    pub(crate) fn add_point_2d(&self, x: f64, y: f64) {
        // SAFETY: a live geometry this value owns.
        unsafe { ogr::OGR_G_AddPoint_2D(self.raw(), x, y) };
    }

    /// A point's coordinates, made 3D.
    pub(crate) fn set_point(&self, x: f64, y: f64, z: f64) {
        // SAFETY: a live point this value owns; index 0 is a point's only one.
        unsafe { ogr::OGR_G_SetPoint(self.raw(), 0, x, y, z) };
    }

    /// A point's coordinates, kept 2D.
    pub(crate) fn set_point_2d(&self, x: f64, y: f64) {
        // SAFETY: a live point this value owns; index 0 is a point's only one.
        unsafe { ogr::OGR_G_SetPoint_2D(self.raw(), 0, x, y) };
    }

    /// `addGeometryDirectly`, `addRingDirectly`, `addCurveDirectly`: the
    /// member is GDAL's from here, kept or (when refused) deleted.
    pub(crate) fn add_geometry_directly(&self, member: OgrGeometry) -> bool {
        // SAFETY: two live geometries; the member's ownership passes to GDAL
        // whatever the answer, and into_raw keeps Rust from freeing it again.
        unsafe { ogr::OGR_G_AddGeometryDirectly(self.raw(), member.into_raw()) == ogr::OGRERR_NONE }
    }

    /// `assignSpatialReference(reference)`: the geometry takes a reference
    /// of its own to it (OGR counts them), so it may outlive the owner of
    /// `reference`.
    pub(crate) fn assign_spatial_reference(&self, reference: SpatialReferenceRef<'_>) {
        // SAFETY: a live geometry this value owns and a live reference, whose
        // count the geometry increases.
        unsafe { algorithm::OGR_G_AssignSpatialReference(self.raw(), reference.raw()) };
    }
}

impl Drop for OgrGeometry {
    fn drop(&mut self) {
        // SAFETY: the geometry is owned by this value alone, destroyed once.
        unsafe { ogr::OGR_G_DestroyGeometry(self.raw()) };
    }
}

// ---- borrowed geometries --------------------------------------------------------------

/// A geometry another object owns: a feature's, or a member of a collection,
/// a polygon or a compound curve.
#[derive(Clone, Copy)]
pub(crate) struct GeometryView<'a> {
    handle: NonNull<ogr::OgrGeometryHs>,
    _owner: PhantomData<&'a ()>,
}

impl<'a> GeometryView<'a> {
    /// # Safety
    ///
    /// `handle` is NULL or a live geometry that stays alive and unchanged for
    /// `'a`.
    pub(super) unsafe fn from_raw(handle: ogr::OgrGeometryH) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self {
            handle,
            _owner: PhantomData,
        })
    }

    fn raw(self) -> ogr::OgrGeometryH {
        self.handle.as_ptr()
    }

    pub(crate) fn geometry_type(self) -> ogr::OgrWkbGeometryType {
        // SAFETY: a live geometry (the view's invariant).
        unsafe { ogr::OGR_G_GetGeometryType(self.raw()) }
    }

    pub(crate) fn is_empty(self) -> bool {
        // SAFETY: a live geometry (the view's invariant).
        unsafe { ogr::OGR_G_IsEmpty(self.raw()) != 0 }
    }

    pub(crate) fn is_3d(self) -> bool {
        // SAFETY: a live geometry (the view's invariant).
        unsafe { ogr::OGR_G_Is3D(self.raw()) != 0 }
    }

    /// A point's, line's or circular string's number of points.
    pub(crate) fn point_count(self) -> c_int {
        // SAFETY: a live geometry (the view's invariant).
        unsafe { ogr::OGR_G_GetPointCount(self.raw()) }
    }

    /// Point `index`: x, y, and z (0 for a 2D geometry).
    pub(crate) fn point(self, index: c_int) -> (f64, f64, f64) {
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        // SAFETY: a live geometry (the view's invariant) and three out
        // values; OGR checks the index against the points it has and raises
        // an error, writing nothing, for one it does not.
        unsafe { ogr::OGR_G_GetPoint(self.raw(), index, &raw mut x, &raw mut y, &raw mut z) };
        (x, y, z)
    }

    /// A collection's members, a polygon's rings (exterior first; none when
    /// it has no exterior), or a compound curve's curves.
    pub(crate) fn member_count(self) -> c_int {
        // SAFETY: a live geometry (the view's invariant).
        unsafe { ogr::OGR_G_GetGeometryCount(self.raw()) }
    }

    pub(crate) fn member(self, index: c_int) -> Option<GeometryView<'a>> {
        // SAFETY: a live geometry; OGR checks the index and answers NULL for
        // one it does not have; a member is owned by the geometry, so alive
        // for 'a.
        unsafe { GeometryView::from_raw(ogr::OGR_G_GetGeometryRef(self.raw(), index)) }
    }

    /// `getLinearGeometry()`: the curves made chords at GDAL's own step, as
    /// a new geometry.
    pub(crate) fn linear_geometry(self) -> Option<OgrGeometry> {
        // SAFETY: a live geometry; the result is a new geometry the caller owns.
        unsafe { OgrGeometry::from_raw(ogr::OGR_G_GetLinearGeometry(self.raw(), 0.0, ptr::null())) }
    }
}
