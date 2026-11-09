//! The conversions between Katana's plain geometry and OGR's, and the one way
//! a coordinate system is read (src/katana_io/ogr_detail.hpp, implemented in
//! gdal_adapter.cpp), for the adapter and the algorithm bridge, so that a file
//! read by IMPORT and a dataset handed back by an algorithm become the same
//! geometry by the same code. With them, the Rust owners of OGR's objects.
//!
//! The owners: `OgrGeometry` and `SpatialReference` own their handle and free
//! it on drop (`OGR_G_DestroyGeometry`, `OSRDestroySpatialReference`, as the
//! C++'s `delete` and stack objects do); `GeometryView` is a geometry another
//! object owns (a feature's, a collection's member), valid for its lifetime.
//! Each method is one C call, so the code that converts is safe code.
//!
//! The C API's one difference from the C++ here: `OGR_G_AddGeometryDirectly`
//! deletes a member it refuses, where the C++'s `addGeometryDirectly` leaves it
//! to the caller; so `add_geometry_directly` consumes the member either way.
//! A point is made by `OGR_G_SetPoint`, whose setters mark a point with a NaN
//! coordinate empty, where the C++ built it with `new OGRPoint(x, y[, z])`.

#![allow(
    unsafe_code,
    reason = "Owns OGR's geometries and spatial references and calls their C API; every call states why it is sound."
)]

use std::ffi::{CStr, c_char, c_int};
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use crate::ffi::{cpl, ogr, osr};
use crate::gdal_adapter::{GeoPoint, GeometryKind, VectorGeometry, VectorReadReport};

// ---- spatial references --------------------------------------------------------------

/// An `OGRSpatialReference` Rust owns.
pub(crate) struct SpatialReference(NonNull<osr::OgrSpatialReferenceHs>);

/// A NULL-terminated list of the given C strings, alive as long as they are.
fn option_list(options: &[&CStr]) -> Vec<*const c_char> {
    options
        .iter()
        .map(|option| option.as_ptr())
        .chain([ptr::null()])
        .collect()
}

impl SpatialReference {
    /// An empty one, as `OGRSpatialReference reference;` is.
    pub(crate) fn new() -> Self {
        // SAFETY: NULL asks for an empty reference, owned by the result.
        let handle = unsafe { osr::OSRNewSpatialReference(ptr::null()) };
        Self(
            NonNull::new(handle).expect("GDAL makes a spatial reference unless memory has run out"),
        )
    }

    pub(crate) fn raw(&self) -> osr::OgrSpatialReferenceH {
        self.0.as_ptr()
    }

    pub(crate) fn set_axis_mapping_strategy(&mut self, strategy: osr::OsrAxisMappingStrategy) {
        // SAFETY: a live reference this value owns.
        unsafe { osr::OSRSetAxisMappingStrategy(self.raw(), strategy) };
    }

    /// `importFromWkt(const char *)`.
    pub(crate) fn import_from_wkt(&mut self, text: &str) -> ogr::OgrErr {
        let text = cpl::c_string(text);
        let mut cursor = text.as_ptr().cast_mut();
        // SAFETY: a live reference, and a cursor into a live NUL-terminated
        // string that GDAL advances and never writes through.
        unsafe { osr::OSRImportFromWkt(self.raw(), &raw mut cursor) }
    }

    /// `SetFromUserInput(text, options)`.
    pub(crate) fn set_from_user_input(&mut self, text: &str, options: &[&CStr]) -> ogr::OgrErr {
        let text = cpl::c_string(text);
        let options = option_list(options);
        // SAFETY: a live reference, a NUL-terminated definition and a
        // NULL-terminated option list, alive for the call.
        unsafe { osr::OSRSetFromUserInputEx(self.raw(), text.as_ptr(), options.as_ptr()) }
    }

    pub(crate) fn set_well_known_geog_cs(&mut self, name: &CStr) -> ogr::OgrErr {
        // SAFETY: a live reference and a NUL-terminated name.
        unsafe { osr::OSRSetWellKnownGeogCS(self.raw(), name.as_ptr()) }
    }

    pub(crate) fn demote_to_2d(&mut self) -> ogr::OgrErr {
        // SAFETY: a live reference; NULL keeps its name.
        unsafe { osr::OSRDemoteTo2D(self.raw(), ptr::null()) }
    }

    pub(crate) fn auto_identify_epsg(&mut self) -> ogr::OgrErr {
        // SAFETY: a live reference.
        unsafe { osr::OSRAutoIdentifyEPSG(self.raw()) }
    }

    /// `exportToWkt(&wkt, options)`: the text when GDAL wrote one.
    pub(crate) fn export_to_wkt_with(&self, options: &[&CStr]) -> Option<String> {
        let options = option_list(options);
        let mut wkt: *mut c_char = ptr::null_mut();
        // SAFETY: a live reference, a NULL-terminated option list and an out
        // pointer GDAL fills with text the caller frees.
        let status = unsafe { osr::OSRExportToWktEx(self.raw(), &raw mut wkt, options.as_ptr()) };
        // SAFETY: NULL or the text GDAL just wrote, read before it is freed.
        let text =
            (status == ogr::OGRERR_NONE && !wkt.is_null()).then(|| unsafe { cpl::decoded(wkt) });
        // SAFETY: GDAL's allocation (or NULL), freed once.
        unsafe { cpl::VSIFree(wkt.cast()) };
        text
    }

    pub(crate) fn is_geographic(&self) -> bool {
        // SAFETY: a live reference.
        unsafe { osr::OSRIsGeographic(self.raw()) != 0 }
    }

    pub(crate) fn is_same(&self, other: &SpatialReference) -> bool {
        // SAFETY: two live references.
        unsafe { osr::OSRIsSame(self.raw(), other.raw()) != 0 }
    }

    pub(crate) fn is_same_with(&self, other: &SpatialReference, options: &[&CStr]) -> bool {
        let options = option_list(options);
        // SAFETY: two live references and a NULL-terminated option list.
        unsafe { osr::OSRIsSameEx(self.raw(), other.raw(), options.as_ptr()) != 0 }
    }

    pub(crate) fn axes_count(&self) -> c_int {
        // SAFETY: a live reference.
        unsafe { osr::OSRGetAxesCount(self.raw()) }
    }

    /// `GetName()`: None for NULL.
    pub(crate) fn name(&self) -> Option<String> {
        // SAFETY: a live reference; the name is copied before the next call.
        let name = unsafe { osr::OSRGetName(self.raw()) };
        // SAFETY: NULL or a string the reference owns, alive for this read.
        (!name.is_null()).then(|| unsafe { cpl::decoded(name) })
    }

    /// `GetAuthorityName(nullptr)`: None for NULL.
    pub(crate) fn authority_name(&self) -> Option<String> {
        // SAFETY: a live reference.
        unsafe { authority_name_of(self.raw()) }
    }

    /// `GetAuthorityCode(nullptr)`: None for NULL.
    pub(crate) fn authority_code(&self) -> Option<String> {
        // SAFETY: a live reference.
        unsafe { authority_code_of(self.raw()) }
    }

    /// `GetDataAxisToSRSAxisMapping()`.
    pub(crate) fn data_axis_to_srs_axis_mapping(&self) -> Vec<c_int> {
        let mut count: c_int = 0;
        // SAFETY: a live reference and an out count; the array GDAL returns
        // is the reference's own, read before the next call.
        let mapping = unsafe { osr::OSRGetDataAxisToSRSAxisMapping(self.raw(), &raw mut count) };
        if mapping.is_null() || count <= 0 {
            return Vec::new();
        }
        // SAFETY: `count` ints at `mapping`, as GDAL says.
        unsafe { std::slice::from_raw_parts(mapping, count as usize) }.to_vec()
    }

    /// `FindMatches(nullptr, &entries, &confidence)`.
    pub(crate) fn find_matches(&self) -> SrsMatches {
        let mut entries: c_int = 0;
        let mut confidence: *mut c_int = ptr::null_mut();
        // SAFETY: a live reference and two out pointers; the arrays GDAL
        // returns are owned by the SrsMatches, which frees them.
        let handles = unsafe {
            osr::OSRFindMatches(
                self.raw(),
                ptr::null(),
                &raw mut entries,
                &raw mut confidence,
            )
        };
        SrsMatches {
            handles,
            entries,
            confidence,
        }
    }
}

impl Drop for SpatialReference {
    fn drop(&mut self) {
        // SAFETY: the reference is owned by this value alone, freed once.
        unsafe { osr::OSRDestroySpatialReference(self.raw()) };
    }
}

/// The WKT of a reference, owned (`SpatialReference::raw`) or not (a
/// layer's, a dataset's): the text when `exportToWkt` wrote one, freed after
/// the copy.
///
/// # Safety
///
/// `reference` is a live spatial reference.
pub(crate) unsafe fn exported_wkt(reference: osr::OgrSpatialReferenceH) -> Option<String> {
    let mut wkt: *mut c_char = ptr::null_mut();
    // SAFETY: a live reference (the caller's contract) and an out pointer
    // GDAL fills with text the caller frees.
    let status = unsafe { osr::OSRExportToWkt(reference, &raw mut wkt) };
    // SAFETY: NULL or the text GDAL just wrote, read before it is freed.
    let text = (status == ogr::OGRERR_NONE && !wkt.is_null()).then(|| unsafe { cpl::decoded(wkt) });
    // SAFETY: GDAL's allocation (or NULL), freed once.
    unsafe { cpl::VSIFree(wkt.cast()) };
    text
}

/// # Safety
///
/// `reference` is a live spatial reference.
unsafe fn authority_name_of(reference: osr::OgrSpatialReferenceH) -> Option<String> {
    // SAFETY: a live reference (the caller's contract); NULL is the root.
    let text = unsafe { osr::OSRGetAuthorityName(reference, ptr::null()) };
    // SAFETY: NULL or a string the reference owns, alive for this read.
    (!text.is_null()).then(|| unsafe { cpl::decoded(text) })
}

/// # Safety
///
/// `reference` is a live spatial reference.
unsafe fn authority_code_of(reference: osr::OgrSpatialReferenceH) -> Option<String> {
    // SAFETY: a live reference (the caller's contract); NULL is the root.
    let text = unsafe { osr::OSRGetAuthorityCode(reference, ptr::null()) };
    // SAFETY: NULL or a string the reference owns, alive for this read.
    (!text.is_null()).then(|| unsafe { cpl::decoded(text) })
}

/// What `FindMatches` returned: the candidates and their confidence, freed
/// together (`OSRFreeSRSArray`, `CPLFree`).
pub(crate) struct SrsMatches {
    handles: *mut osr::OgrSpatialReferenceH,
    entries: c_int,
    confidence: *mut c_int,
}

impl SrsMatches {
    pub(crate) fn entries(&self) -> c_int {
        self.entries
    }

    pub(crate) fn has_candidates(&self) -> bool {
        !self.handles.is_null()
    }

    /// Candidate `index`'s confidence; None without a confidence list or for
    /// an index past the entries.
    pub(crate) fn confidence(&self, index: usize) -> Option<c_int> {
        if self.confidence.is_null() || index >= self.entries.max(0) as usize {
            return None;
        }
        // SAFETY: `entries` ints at `confidence`, as GDAL says, and `index`
        // is below it.
        Some(unsafe { self.confidence.add(index).read() })
    }

    fn candidate(&self, index: usize) -> Option<osr::OgrSpatialReferenceH> {
        if self.handles.is_null() || index >= self.entries.max(0) as usize {
            return None;
        }
        // SAFETY: `entries` handles at `handles`, and `index` is below it.
        let handle = unsafe { self.handles.add(index).read() };
        (!handle.is_null()).then_some(handle)
    }

    pub(crate) fn authority_name(&self, index: usize) -> Option<String> {
        // SAFETY: a live candidate, owned by this list.
        self.candidate(index)
            .and_then(|handle| unsafe { authority_name_of(handle) })
    }

    pub(crate) fn authority_code(&self, index: usize) -> Option<String> {
        // SAFETY: a live candidate, owned by this list.
        self.candidate(index)
            .and_then(|handle| unsafe { authority_code_of(handle) })
    }
}

impl Drop for SrsMatches {
    fn drop(&mut self) {
        // SAFETY: GDAL's arrays (or NULL), owned by this value, freed once.
        unsafe {
            osr::OSRFreeSRSArray(self.handles);
            cpl::VSIFree(self.confidence.cast());
        }
    }
}

/// Parses a coordinate system the way GDAL's own SetProjection would - WKT1,
/// WKT2, PROJJSON - but refuses to fetch anything: a string read from a file
/// is not permission to open another file or a URL. With the traditional GIS
/// axis order.
pub(crate) fn parse_crs(text: &str, reference: &mut SpatialReference) -> bool {
    reference.set_axis_mapping_strategy(osr::OAMS_TRADITIONAL_GIS_ORDER);
    if reference.import_from_wkt(text) == ogr::OGRERR_NONE {
        return true;
    }
    reference.set_from_user_input(text, &osr::SET_FROM_USER_INPUT_LIMITATIONS) == ogr::OGRERR_NONE
}

// ---- geometries ---------------------------------------------------------------------------

/// `wkbFlatten`.
pub(crate) fn flattened(geometry_type: ogr::OgrWkbGeometryType) -> ogr::OgrWkbGeometryType {
    // SAFETY: a pure function of an integer.
    unsafe { ogr::OGR_GT_Flatten(geometry_type) }
}

/// GDAL's name of a geometry type (`OGRGeometryTypeToName`).
pub(crate) fn geometry_type_name(geometry_type: ogr::OgrWkbGeometryType) -> String {
    // SAFETY: a static or thread-local string GDAL keeps, copied at once.
    unsafe { cpl::decoded(ogr::OGRGeometryTypeToName(geometry_type)) }
}

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
    pub(crate) unsafe fn from_raw(handle: ogr::OgrGeometryH) -> Option<Self> {
        NonNull::new(handle).map(Self)
    }

    pub(crate) fn raw(&self) -> ogr::OgrGeometryH {
        self.0.as_ptr()
    }

    /// Hands the geometry to GDAL, which owns it from here.
    pub(crate) fn into_raw(self) -> ogr::OgrGeometryH {
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
        // whatever the answer.
        unsafe { ogr::OGR_G_AddGeometryDirectly(self.raw(), member.into_raw()) == ogr::OGRERR_NONE }
    }
}

impl Drop for OgrGeometry {
    fn drop(&mut self) {
        // SAFETY: the geometry is owned by this value alone, destroyed once.
        unsafe { ogr::OGR_G_DestroyGeometry(self.raw()) };
    }
}

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
    pub(crate) unsafe fn from_raw(handle: ogr::OgrGeometryH) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self {
            handle,
            _owner: PhantomData,
        })
    }

    pub(crate) fn raw(self) -> ogr::OgrGeometryH {
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
        // SAFETY: a live geometry (the view's invariant) and three out values.
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
        // SAFETY: a live geometry; a member is owned by it, so alive for 'a.
        unsafe { GeometryView::from_raw(ogr::OGR_G_GetGeometryRef(self.raw(), index)) }
    }

    /// `getLinearGeometry()`: the curves made chords at GDAL's own step, as
    /// a new geometry.
    pub(crate) fn linear_geometry(self) -> Option<OgrGeometry> {
        // SAFETY: a live geometry; the result is a new geometry the caller owns.
        unsafe { OgrGeometry::from_raw(ogr::OGR_G_GetLinearGeometry(self.raw(), 0.0, ptr::null())) }
    }
}

// ---- OGR geometry -> VectorGeometry ----------------------------------------------------

/// The points of a line or of a CircularString (both are simple curves): a
/// CircularString's points are the ends and the middles of its arcs.
fn points_of(line: GeometryView<'_>) -> Vec<GeoPoint> {
    let count = line.point_count();
    let mut points = Vec::with_capacity(count.max(0) as usize);
    let has3d = line.is_3d();
    for i in 0..count {
        let (x, y, z) = line.point(i);
        points.push(GeoPoint {
            x,
            y,
            z: if has3d { z } else { 0.0 },
        });
    }
    points
}

/// A curve's points appended to `points`, and the index each of its arcs
/// starts at appended to `arcs` (VectorGeometry::arcs). A CompoundCurve's
/// members meet end to start, so each after the first gives its points but
/// its first.
fn append_curve(curve: GeometryView<'_>, points: &mut Vec<GeoPoint>, arcs: &mut Vec<usize>) {
    let geometry_type = flattened(curve.geometry_type());
    if geometry_type == ogr::WKB_COMPOUND_CURVE {
        for i in 0..curve.member_count() {
            if let Some(member) = curve.member(i) {
                append_curve(member, points, arcs);
            }
        }
        return;
    }
    let own = points_of(curve);
    let start = if points.is_empty() {
        0
    } else {
        points.len() - 1
    };
    let skip = usize::from(!(points.is_empty() || own.is_empty()));
    points.extend_from_slice(&own[skip..]);
    if geometry_type == ogr::WKB_CIRCULAR_STRING {
        // (0, 1, 2), (2, 3, 4) ...: an arc every two points.
        let mut k = 0;
        while k + 2 < own.len() {
            arcs.push(start + k);
            k += 2;
        }
    }
}

fn is_curve(geometry_type: ogr::OgrWkbGeometryType) -> bool {
    matches!(
        flattened(geometry_type),
        ogr::WKB_CIRCULAR_STRING
            | ogr::WKB_COMPOUND_CURVE
            | ogr::WKB_CURVE_POLYGON
            | ogr::WKB_MULTI_CURVE
            | ogr::WKB_MULTI_SURFACE
    )
}

fn count(report: &mut VectorReadReport, what: &str) {
    *report.skipped.entry(what.to_owned()).or_insert(0) += 1;
}

/// Appends one VectorGeometry per SIMPLE geometry. A multi-geometry or
/// collection is recursed into so that every emitted VectorGeometry has
/// exactly one kind, which is what keeps the consumer free of nested-variant
/// handling. A curve is kept as its arcs when `keep_arcs` is set (IMPORT turns
/// them into Katana's arcs and circles), and made chords at GDAL's own step
/// otherwise; what has no Katana counterpart - a TIN, a polyhedral surface -
/// is counted in `report`, never silently dropped.
pub(crate) fn flatten(
    geometry: Option<GeometryView<'_>>,
    out: &mut Vec<VectorGeometry>,
    keep_arcs: bool,
    report: &mut VectorReadReport,
    depth: i32,
) {
    let Some(geometry) = geometry else {
        return;
    };
    if geometry.is_empty() {
        return;
    }
    // Defensive: OGR geometries are trees and a malformed file could in
    // principle nest collections deeply. 32 is far beyond anything meaningful.
    if depth > 32 {
        count(report, "nested too deep");
        return;
    }

    let geometry_type = flattened(geometry.geometry_type());
    if !keep_arcs && is_curve(geometry_type) {
        // The chords GDAL makes at its own step (OGR_ARC_STEPSIZE, 4 degrees):
        // what an algorithm is handed. IMPORT keeps the arcs and makes its
        // chords by the one rule EXPORT uses (docs/interop.md, "Curves").
        if let Some(linear) = geometry.linear_geometry() {
            report.curves_made_chords += 1;
            flatten(Some(linear.view()), out, keep_arcs, report, depth + 1);
        }
        return;
    }

    match geometry_type {
        ogr::WKB_POINT => {
            let is3d = geometry.is_3d();
            let (x, y, z) = geometry.point(0);
            out.push(VectorGeometry {
                kind: GeometryKind::Point,
                parts: vec![vec![GeoPoint {
                    x,
                    y,
                    z: if is3d { z } else { 0.0 },
                }]],
                has_z: is3d,
                ..VectorGeometry::default()
            });
        }
        ogr::WKB_LINE_STRING | ogr::WKB_CIRCULAR_STRING | ogr::WKB_COMPOUND_CURVE => {
            let mut points = Vec::new();
            let mut arcs = Vec::new();
            append_curve(geometry, &mut points, &mut arcs);
            let mut part = VectorGeometry {
                kind: GeometryKind::LineString,
                parts: vec![points],
                has_z: geometry.is_3d(),
                arcs: vec![arcs],
            };
            if !part.has_arcs() {
                part.arcs.clear();
            }
            out.push(part);
        }
        // A Triangle is a polygon of three sides (OGRTriangle is an OGRPolygon).
        ogr::WKB_POLYGON | ogr::WKB_TRIANGLE | ogr::WKB_CURVE_POLYGON => {
            let mut part = VectorGeometry {
                kind: GeometryKind::Polygon,
                ..VectorGeometry::default()
            };
            // The exterior ring and then the interior ones.
            for i in 0..geometry.member_count() {
                let Some(ring) = geometry.member(i) else {
                    continue;
                };
                if ring.is_empty() {
                    continue;
                }
                let mut points = Vec::new();
                let mut arcs = Vec::new();
                append_curve(ring, &mut points, &mut arcs);
                part.parts.push(points);
                part.arcs.push(arcs);
            }
            if part.parts.is_empty() {
                return;
            }
            part.has_z = geometry.is_3d();
            if !part.has_arcs() {
                part.arcs.clear();
            }
            out.push(part);
        }
        ogr::WKB_MULTI_POINT
        | ogr::WKB_MULTI_LINE_STRING
        | ogr::WKB_MULTI_POLYGON
        | ogr::WKB_MULTI_CURVE
        | ogr::WKB_MULTI_SURFACE
        | ogr::WKB_GEOMETRY_COLLECTION => {
            for i in 0..geometry.member_count() {
                flatten(geometry.member(i), out, keep_arcs, report, depth + 1);
            }
        }
        // Faces of a 3D model: Katana has no entity that is one, and a TIN
        // taken apart into triangles would be thousands of closed polylines
        // that are no longer a surface. Counted and said by name instead.
        ogr::WKB_TIN => count(report, "tin"),
        ogr::WKB_POLYHEDRAL_SURFACE => count(report, "polyhedral surface"),
        _ => count(
            report,
            &format!("unsupported {}", geometry_type_name(geometry_type)),
        ),
    }
}

/// Appends one VectorGeometry per simple member of `geometry`: a
/// multi-geometry or collection is recursed into. What cannot be represented
/// is skipped and said in `warnings`.
#[allow(
    dead_code,
    reason = "The algorithm bridge's conversion of a dataset GDAL hands back (processing.cpp), which is ported in wave L."
)]
pub(crate) fn flatten_ogr_geometry(
    geometry: Option<GeometryView<'_>>,
    out: &mut Vec<VectorGeometry>,
    warnings: &mut Vec<String>,
) {
    let mut report = VectorReadReport::default();
    flatten(geometry, out, false, &mut report, 0);
    for (what, count) in &report.skipped {
        warnings.push(format!("{count} {what} geometries were skipped"));
    }
    if report.curves_made_chords != 0 {
        warnings.push(format!(
            "{} curves were made chords at GDAL's own step",
            report.curves_made_chords
        ));
    }
}

// ---- VectorGeometry -> OGR geometry ----------------------------------------------------

/// Adds a vertex in the geometry's own dimension: OGR makes a geometry 3D the
/// moment one vertex is given a z, so a 2D one must never be handed a zero.
fn add_vertex(line: &OgrGeometry, point: &GeoPoint, has_z: bool) {
    if has_z {
        line.add_point(point.x, point.y, point.z);
    } else {
        line.add_point_2d(point.x, point.y);
    }
}

fn make_ring(points: &[GeoPoint], has_z: bool) -> OgrGeometry {
    let ring = OgrGeometry::create(ogr::WKB_LINEAR_RING);
    for point in points {
        add_vertex(&ring, point, has_z);
    }
    // OGR requires a closed ring; close it if the caller's data does not.
    if let (Some(first), Some(last)) = (points.first(), points.last())
        && (first.x != last.x || first.y != last.y)
    {
        add_vertex(&ring, first, has_z);
    }
    ring
}

/// The straight run of a curve from the last vertex taken to `end`.
fn straight_to(
    compound: &OgrGeometry,
    points: &[GeoPoint],
    has_z: bool,
    at: &mut usize,
    end: usize,
) {
    if end <= *at {
        return;
    }
    let line = OgrGeometry::create(ogr::WKB_LINE_STRING);
    for point in &points[*at..=end] {
        add_vertex(&line, point, has_z);
    }
    compound.add_geometry_directly(line);
    *at = end;
}

/// A part with arcs as the curve it was read from: straight runs as line
/// strings, each arc a CircularString of its three points, joined in a
/// CompoundCurve. A driver without curves makes its own chords of it.
fn make_curve(points: &[GeoPoint], arcs: &[usize], has_z: bool, closed: bool) -> OgrGeometry {
    let compound = OgrGeometry::create(ogr::WKB_COMPOUND_CURVE);
    let mut at = 0;
    for &arc in arcs {
        // An index that names no arc of this part (the C++'s size_t sum
        // cannot overflow there for any index a part can hold).
        if arc.checked_add(2).is_none_or(|end| end >= points.len()) || arc < at {
            continue;
        }
        straight_to(&compound, points, has_z, &mut at, arc);
        let circular = OgrGeometry::create(ogr::WKB_CIRCULAR_STRING);
        for point in &points[arc..=arc + 2] {
            add_vertex(&circular, point, has_z);
        }
        compound.add_geometry_directly(circular);
        at = arc + 2;
    }
    straight_to(&compound, points, has_z, &mut at, points.len() - 1);
    let first = &points[0];
    let last = &points[points.len() - 1];
    if closed && (first.x != last.x || first.y != last.y) {
        let closing = OgrGeometry::create(ogr::WKB_LINE_STRING);
        add_vertex(&closing, last, has_z);
        add_vertex(&closing, first, has_z);
        compound.add_geometry_directly(closing);
    }
    compound
}

/// A new OGR geometry the caller owns, in the geometry's own dimension (2D
/// stays 2D); None for a geometry with too few points for its kind.
pub(crate) fn make_ogr_geometry(geometry: &VectorGeometry) -> Option<OgrGeometry> {
    let arcs_of = |part: usize| geometry.arcs.get(part).filter(|arcs| !arcs.is_empty());
    match geometry.kind {
        GeometryKind::Point => {
            let p = geometry.parts.first()?.first()?;
            let point = OgrGeometry::create(ogr::WKB_POINT);
            if geometry.has_z {
                point.set_point(p.x, p.y, p.z);
            } else {
                point.set_point_2d(p.x, p.y);
            }
            Some(point)
        }
        GeometryKind::LineString => {
            let first = geometry.parts.first().filter(|part| part.len() >= 2)?;
            if let Some(arcs) = arcs_of(0) {
                return Some(make_curve(first, arcs, geometry.has_z, false));
            }
            let line = OgrGeometry::create(ogr::WKB_LINE_STRING);
            for p in first {
                add_vertex(&line, p, geometry.has_z);
            }
            Some(line)
        }
        GeometryKind::Polygon => {
            if geometry.parts.first().is_none_or(|part| part.len() < 3) {
                return None;
            }
            if geometry.has_arcs() {
                let polygon = OgrGeometry::create(ogr::WKB_CURVE_POLYGON);
                for (r, part) in geometry.parts.iter().enumerate() {
                    if part.len() < 3 {
                        continue;
                    }
                    let arcs = arcs_of(r).map_or(&[][..], Vec::as_slice);
                    polygon.add_geometry_directly(make_curve(part, arcs, geometry.has_z, true));
                }
                return Some(polygon);
            }
            let polygon = OgrGeometry::create(ogr::WKB_POLYGON);
            for part in &geometry.parts {
                if part.len() >= 3 {
                    polygon.add_geometry_directly(make_ring(part, geometry.has_z));
                }
            }
            Some(polygon)
        }
        GeometryKind::Unknown => None,
    }
}
