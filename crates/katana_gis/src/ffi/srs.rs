//! OGR's spatial references and coordinate transformations as Rust owners
//! and views (ogr_srs_api.h): moved here from ogr_detail.rs and reproject.rs
//! so that the code that reads and converts coordinate systems is safe code.
//!
//! Who frees what:
//!
//! ```text
//! SpatialReference          one Rust made (OSRNewSpatialReference); freed on
//!                           drop (OSRDestroySpatialReference, as the C++'s
//!                           stack object is)
//! SpatialReferenceRef       one another object owns - a layer's, a dataset's,
//!                           a candidate of SrsMatches - borrowed for as long as
//!                           its owner is; never freed here
//! SrsMatches                what FindMatches returned: the candidates and
//!                           their confidence, freed together (OSRFreeSRSArray,
//!                           CPLFree)
//! CoordinateTransformation  one Rust made (OCTNewCoordinateTransformation);
//!                           destroyed on drop
//! ```
//!
//! A text GDAL writes for the caller (exportToWkt's) is copied and freed
//! (CPLFree) inside the call that asked for it.

#![allow(
    unsafe_code,
    reason = "Owns OGR's spatial references and transformations and calls their C API; every call states why it is sound."
)]

use std::ffi::{CStr, c_char, c_int};
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use super::{algorithm, cpl, ogr, osr};

/// A NULL-terminated list of the given C strings, alive as long as they are.
fn option_list(options: &[&CStr]) -> Vec<*const c_char> {
    options
        .iter()
        .map(|option| option.as_ptr())
        .chain([ptr::null()])
        .collect()
}

/// The version of PROJ GDAL runs with (`OSRGetPROJVersion`).
pub(crate) fn proj_version() -> (c_int, c_int, c_int) {
    let (mut major, mut minor, mut patch): (c_int, c_int, c_int) = (0, 0, 0);
    // SAFETY: three out values.
    unsafe { algorithm::OSRGetPROJVersion(&raw mut major, &raw mut minor, &raw mut patch) };
    (major, minor, patch)
}

// ---- what any live reference answers ---------------------------------------------------

/// `exportToWkt(&wkt, options)` of a live reference: the text when GDAL wrote
/// one, freed after the copy.
///
/// # Safety
///
/// `reference` is a live spatial reference.
unsafe fn export_wkt(
    reference: osr::OgrSpatialReferenceH,
    options: Option<&[&CStr]>,
) -> Option<String> {
    let mut wkt: *mut c_char = ptr::null_mut();
    let status = match options {
        // SAFETY: a live reference (the caller's contract) and an out pointer
        // GDAL fills with text the caller frees.
        None => unsafe { osr::OSRExportToWkt(reference, &raw mut wkt) },
        Some(options) => {
            let options = option_list(options);
            // SAFETY: as above, with a NULL-terminated option list alive for
            // the call.
            unsafe { osr::OSRExportToWktEx(reference, &raw mut wkt, options.as_ptr()) }
        }
    };
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

// ---- borrowed references ---------------------------------------------------------------

/// A spatial reference another object owns, alive for `'a`.
#[derive(Clone, Copy)]
pub(crate) struct SpatialReferenceRef<'a> {
    handle: NonNull<osr::OgrSpatialReferenceHs>,
    _owner: PhantomData<&'a ()>,
}

impl SpatialReferenceRef<'_> {
    /// # Safety
    ///
    /// `handle` is NULL or a live spatial reference that stays alive, and
    /// is not freed, for the lifetime the caller gives the result.
    pub(super) unsafe fn from_raw(handle: osr::OgrSpatialReferenceH) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self {
            handle,
            _owner: PhantomData,
        })
    }

    pub(super) fn raw(self) -> osr::OgrSpatialReferenceH {
        self.handle.as_ptr()
    }

    /// `exportToWkt(&wkt)`: the text when GDAL wrote one.
    pub(crate) fn exported_wkt(self) -> Option<String> {
        // SAFETY: a live reference (the view's invariant).
        unsafe { export_wkt(self.raw(), None) }
    }

    /// `exportToWkt(&wkt, options)`: the text when GDAL wrote one.
    pub(crate) fn export_to_wkt_with(self, options: &[&CStr]) -> Option<String> {
        // SAFETY: a live reference (the view's invariant).
        unsafe { export_wkt(self.raw(), Some(options)) }
    }
}

// ---- owned references ------------------------------------------------------------------

/// An `OGRSpatialReference` Rust owns.
pub(crate) struct SpatialReference(NonNull<osr::OgrSpatialReferenceHs>);

impl SpatialReference {
    /// An empty one, as `OGRSpatialReference reference;` is.
    pub(crate) fn new() -> Self {
        // SAFETY: NULL asks for an empty reference, owned by the result.
        let handle = unsafe { osr::OSRNewSpatialReference(ptr::null()) };
        Self(
            NonNull::new(handle).expect("GDAL makes a spatial reference unless memory has run out"),
        )
    }

    fn raw(&self) -> osr::OgrSpatialReferenceH {
        self.0.as_ptr()
    }

    /// The reference as a view, borrowed from this owner.
    pub(crate) fn view(&self) -> SpatialReferenceRef<'_> {
        SpatialReferenceRef {
            handle: self.0,
            _owner: PhantomData,
        }
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
        self.view().export_to_wkt_with(options)
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

    /// Candidate `index`, borrowed from this list; None past the entries.
    fn candidate(&self, index: usize) -> Option<SpatialReferenceRef<'_>> {
        if self.handles.is_null() || index >= self.entries.max(0) as usize {
            return None;
        }
        // SAFETY: `entries` handles at `handles`, and `index` is below it.
        let handle = unsafe { self.handles.add(index).read() };
        // SAFETY: NULL or a candidate this list owns, alive until it is freed
        // with the list, which the borrow of `self` outlasts.
        unsafe { SpatialReferenceRef::from_raw(handle) }
    }

    pub(crate) fn authority_name(&self, index: usize) -> Option<String> {
        self.candidate(index)
            // SAFETY: a live candidate, owned by this list.
            .and_then(|candidate| unsafe { authority_name_of(candidate.raw()) })
    }

    pub(crate) fn authority_code(&self, index: usize) -> Option<String> {
        self.candidate(index)
            // SAFETY: a live candidate, owned by this list.
            .and_then(|candidate| unsafe { authority_code_of(candidate.raw()) })
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

// ---- transformations -------------------------------------------------------------------

/// An `OGRCoordinateTransformation` Rust owns.
pub(crate) struct CoordinateTransformation(NonNull<osr::OgrCoordinateTransformationHs>);

impl CoordinateTransformation {
    /// `OCTNewCoordinateTransformation(source, target)`: None when PROJ has
    /// no transformation between them. Each reference is copied.
    pub(crate) fn new(source: &SpatialReference, target: &SpatialReference) -> Option<Self> {
        // SAFETY: two live references, which the transformation copies; it
        // is owned by the value made below.
        let handle = unsafe { osr::OCTNewCoordinateTransformation(source.raw(), target.raw()) };
        NonNull::new(handle).map(Self)
    }

    fn raw(&self) -> osr::OgrCoordinateTransformationH {
        self.0.as_ptr()
    }

    /// `Transform(1, &x, &y)`, no z: whether PROJ transformed the point.
    pub(crate) fn transform_point(&self, x: &mut f64, y: &mut f64) -> bool {
        // SAFETY: a live transformation, one x and one y, and no z.
        unsafe { osr::OCTTransform(self.raw(), 1, x, y, ptr::null_mut()) != cpl::FALSE }
    }

    /// `Transform(count, xs, ys, nullptr, success)`, no z, over as many
    /// points as the shortest of the three slices holds (cut to GDAL's int):
    /// each point's own success in `success`.
    pub(crate) fn transform_points(&self, xs: &mut [f64], ys: &mut [f64], success: &mut [c_int]) {
        let count = xs.len().min(ys.len()).min(success.len());
        let count = c_int::try_from(count).unwrap_or(c_int::MAX);
        // SAFETY: a live transformation and at least `count` x, y and success
        // values in each slice; no z.
        unsafe {
            osr::OCTTransformEx(
                self.raw(),
                count,
                xs.as_mut_ptr(),
                ys.as_mut_ptr(),
                ptr::null_mut(),
                success.as_mut_ptr(),
            )
        };
    }

    /// `TransformBounds(...)` with `densify` points per edge: the box GDAL
    /// wrote, as min x, min y, max x, max y, and whether it succeeded.
    pub(crate) fn transform_bounds(
        &self,
        bounds: [f64; 4],
        out: &mut [f64; 4],
        densify: c_int,
    ) -> bool {
        let [min_x, min_y, max_x, max_y] = out;
        // SAFETY: a live transformation and four out values.
        unsafe {
            osr::OCTTransformBounds(
                self.raw(),
                bounds[0],
                bounds[1],
                bounds[2],
                bounds[3],
                min_x,
                min_y,
                max_x,
                max_y,
                densify,
            ) != cpl::FALSE
        }
    }
}

impl Drop for CoordinateTransformation {
    fn drop(&mut self) {
        // SAFETY: the transformation is owned by this value alone, destroyed once.
        unsafe { osr::OCTDestroyCoordinateTransformation(self.raw()) };
    }
}
