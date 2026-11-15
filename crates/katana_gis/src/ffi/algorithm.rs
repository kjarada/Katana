//! GDAL's algorithm framework in C (gdalalgorithm_c.h), the few raster, CPL
//! and OGR functions the bridge calls that K1's files do not declare, and the
//! crate's own C++ shim over the framework (native/algorithm_shim.cpp):
//! exactly what src/katana_io/geo/processing.cpp and policy.cpp call, written
//! by hand from GDAL 3.13.2's headers (ffi/cpl.rs says how the ffi folder is
//! laid out and checked). Checked by native/algorithm_check.c: a `_Generic`
//! line per function, the shim's against native/algorithm_shim.h, and a
//! `_Static_assert` per constant; the C++-only constants
//! (`HIDDEN_ALIAS_SEPARATOR`, `ROOT_ALG_NAME`, `UNBOUNDED`) by `static_assert`
//! in the shim itself.
//!
//! Why a shim, and why these thirteen functions (docs/rust.md, step 5): the C
//! API has no twin for some C++ calls the bridge makes, and for others its
//! twin is not the same call. Each was measured on libgdal 3.13.2 by
//! disassembling the C function (x86_64, objdump -d):
//!
//! ```text
//! no C twin        IsHidden, GetAliases (with the hidden separator),
//!                  GetSuggestionForArgumentName, ValidateArguments,
//!                  GetAutoCompleteChoices, GetMinValue, GetMaxValue,
//!                  GetMetadata (C reads one item by name, never the keys),
//!                  SetDatasetName, Set(std::vector<GDALArgDatasetValue>)
//!                  of names and open datasets mixed, and
//!                  Get<std::vector<GDALArgDatasetValue>>' names
//! not the same     GDALAlgorithmInstantiateSubAlgorithm passes
//!                  suggestionAllowed = true (`mov $0x1,%ecx` before the
//!                  call) where the C++ passes false; GDALAlgorithmGetArg and
//!                  GDALAlgorithmGetArgNonConst both pass suggestionAllowed =
//!                  true (`mov $0x1,%edx`) where every C++ GetArg passes
//!                  false. With suggestions on, an unknown name raises a
//!                  CE_Failure ("Algorithm 'x' is unknown. Do you mean 'y'?")
//!                  the C++ never raises, which a run's collector would take
//!                  for GDAL's own failure.
//! ```
//!
//! So a sub-algorithm is made by the shim, and an argument is looked up by
//! the shim (`katana_gdal_algorithm_arg_name`, GetArg without suggestions)
//! and only then fetched through the C API by the long name it has, which
//! GDAL finds before any suggestion is tried. Everything else is the C API,
//! whose functions were checked the same way to call exactly the C++ method
//! the bridge calls (`Set(bool)`, `Set(const std::string&)`,
//! `Set(GDALDataset*)`, `Run`, `Finalize`, `GetDatasetIncreaseRefCount`, ...).

#![allow(
    unsafe_code,
    reason = "The declarations of GDAL's algorithm C API and of the crate's shim; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's enumerations are declared whole where the C++ names them; native/algorithm_check.c holds each."
)]

use std::ffi::{CStr, c_char, c_int, c_long, c_uint, c_void};
use std::marker::{PhantomData, PhantomPinned};

use super::cpl::{CplErr, CplErrorNum};
use super::gdal::{GdalColorInterp, GdalDataType, GdalDatasetH, GdalProgressFunc, GdalRasterBandH};
use super::ogr::OgrGeometryH;
use super::osr::OgrSpatialReferenceH;

/// `GDALAlgorithmH`'s object.
#[repr(C)]
pub(crate) struct GdalAlgorithmHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalAlgorithmH = *mut GdalAlgorithmHs;

/// `GDALAlgorithmArgH`'s object.
#[repr(C)]
pub(crate) struct GdalAlgorithmArgHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalAlgorithmArgH = *mut GdalAlgorithmArgHs;

/// `GDALArgDatasetValueH`'s object.
#[repr(C)]
pub(crate) struct GdalArgDatasetValueHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalArgDatasetValueH = *mut GdalArgDatasetValueHs;

/// `GDALAlgorithmRegistryH`'s object.
#[repr(C)]
pub(crate) struct GdalAlgorithmRegistryHs {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type GdalAlgorithmRegistryH = *mut GdalAlgorithmRegistryHs;

/// `GDALAlgorithmArgType`.
pub(crate) type GdalAlgorithmArgType = c_int;
pub(crate) const GAAT_BOOLEAN: GdalAlgorithmArgType = 0;
pub(crate) const GAAT_STRING: GdalAlgorithmArgType = 1;
pub(crate) const GAAT_INTEGER: GdalAlgorithmArgType = 2;
pub(crate) const GAAT_REAL: GdalAlgorithmArgType = 3;
pub(crate) const GAAT_DATASET: GdalAlgorithmArgType = 4;
pub(crate) const GAAT_STRING_LIST: GdalAlgorithmArgType = 5;
pub(crate) const GAAT_INTEGER_LIST: GdalAlgorithmArgType = 6;
pub(crate) const GAAT_REAL_LIST: GdalAlgorithmArgType = 7;
pub(crate) const GAAT_DATASET_LIST: GdalAlgorithmArgType = 8;

/// `GDALArgDatasetType`: GDAL_OF_RASTER, GDAL_OF_VECTOR and
/// GDAL_OF_MULTIDIM_RASTER, possibly with GDAL_OF_UPDATE.
pub(crate) type GdalArgDatasetType = c_int;

/// `GADV_NAME` and `GADV_OBJECT`: how a dataset argument may be given.
pub(crate) const GADV_NAME: c_int = 1 << 0;
pub(crate) const GADV_OBJECT: c_int = 1 << 1;

/// gdal.h's open flag the bridge reads beside K1's (ffi/gdal.rs).
pub(crate) const GDAL_OF_MULTIDIM_RASTER: c_uint = 0x10;

/// gdal.h's `GCI_Undefined`.
pub(crate) const GCI_UNDEFINED: GdalColorInterp = 0;

/// cpl_error.h's error numbers the bridge classifies a failure by.
pub(crate) const CPLE_FILE_IO: CplErrorNum = 3;
pub(crate) const CPLE_OPEN_FAILED: CplErrorNum = 4;

/// `GDALAlgorithmRegistry::HIDDEN_ALIAS_SEPARATOR`, a C++ constant:
/// GetAliases lists the public aliases, this, then the hidden ones. Checked
/// by a static_assert in native/algorithm_shim.cpp.
pub(crate) const HIDDEN_ALIAS_SEPARATOR: &str = "==hide==";

/// `GDALGlobalAlgorithmRegistry::ROOT_ALG_NAME`, the root "gdal" algorithm.
/// Checked by a static_assert in native/algorithm_shim.cpp.
pub(crate) const ROOT_ALG_NAME: &CStr = c"gdal";

/// `GDALAlgorithmArgDecl::UNBOUNDED`, `std::numeric_limits<int>::max()`.
/// Checked by a static_assert in native/algorithm_shim.cpp.
pub(crate) const UNBOUNDED: c_int = c_int::MAX;

#[link(name = "gdal")]
unsafe extern "C" {
    // ---- the registry -------------------------------------------------------------
    pub(crate) fn GDALGetGlobalAlgorithmRegistry() -> GdalAlgorithmRegistryH;
    pub(crate) fn GDALAlgorithmRegistryRelease(registry: GdalAlgorithmRegistryH);
    pub(crate) fn GDALAlgorithmRegistryInstantiateAlg(
        registry: GdalAlgorithmRegistryH,
        name: *const c_char,
    ) -> GdalAlgorithmH;

    // ---- algorithms ---------------------------------------------------------------
    pub(crate) fn GDALAlgorithmRelease(algorithm: GdalAlgorithmH);
    pub(crate) fn GDALAlgorithmGetName(algorithm: GdalAlgorithmH) -> *const c_char;
    pub(crate) fn GDALAlgorithmGetDescription(algorithm: GdalAlgorithmH) -> *const c_char;
    pub(crate) fn GDALAlgorithmGetLongDescription(algorithm: GdalAlgorithmH) -> *const c_char;
    pub(crate) fn GDALAlgorithmGetHelpFullURL(algorithm: GdalAlgorithmH) -> *const c_char;
    pub(crate) fn GDALAlgorithmHasSubAlgorithms(algorithm: GdalAlgorithmH) -> bool;
    pub(crate) fn GDALAlgorithmGetSubAlgorithmNames(algorithm: GdalAlgorithmH) -> *mut *mut c_char;
    pub(crate) fn GDALAlgorithmParseCommandLineArguments(
        algorithm: GdalAlgorithmH,
        arguments: *const *const c_char,
    ) -> bool;
    pub(crate) fn GDALAlgorithmRun(
        algorithm: GdalAlgorithmH,
        progress: GdalProgressFunc,
        progress_data: *mut c_void,
    ) -> bool;
    pub(crate) fn GDALAlgorithmFinalize(algorithm: GdalAlgorithmH) -> bool;
    pub(crate) fn GDALAlgorithmGetUsageAsJSON(algorithm: GdalAlgorithmH) -> *mut c_char;
    pub(crate) fn GDALAlgorithmGetArgNames(algorithm: GdalAlgorithmH) -> *mut *mut c_char;
    pub(crate) fn GDALAlgorithmGetArgNonConst(
        algorithm: GdalAlgorithmH,
        name: *const c_char,
    ) -> GdalAlgorithmArgH;

    // ---- arguments ----------------------------------------------------------------
    pub(crate) fn GDALAlgorithmArgRelease(argument: GdalAlgorithmArgH);
    pub(crate) fn GDALAlgorithmArgGetName(argument: GdalAlgorithmArgH) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetType(argument: GdalAlgorithmArgH) -> GdalAlgorithmArgType;
    pub(crate) fn GDALAlgorithmArgGetDescription(argument: GdalAlgorithmArgH) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetShortName(argument: GdalAlgorithmArgH) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetAliases(argument: GdalAlgorithmArgH) -> *mut *mut c_char;
    pub(crate) fn GDALAlgorithmArgGetMetaVar(argument: GdalAlgorithmArgH) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetCategory(argument: GdalAlgorithmArgH) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgIsPositional(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgIsRequired(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgGetMinCount(argument: GdalAlgorithmArgH) -> c_int;
    pub(crate) fn GDALAlgorithmArgGetMaxCount(argument: GdalAlgorithmArgH) -> c_int;
    pub(crate) fn GDALAlgorithmArgGetPackedValuesAllowed(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgGetRepeatedArgAllowed(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgGetChoices(argument: GdalAlgorithmArgH) -> *mut *mut c_char;
    pub(crate) fn GDALAlgorithmArgGetMetadataItem(
        argument: GdalAlgorithmArgH,
        item: *const c_char,
    ) -> *mut *mut c_char;
    pub(crate) fn GDALAlgorithmArgIsExplicitlySet(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgHasDefaultValue(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgGetDefaultAsBoolean(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgGetDefaultAsString(argument: GdalAlgorithmArgH) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetDefaultAsInteger(argument: GdalAlgorithmArgH) -> c_int;
    pub(crate) fn GDALAlgorithmArgGetDefaultAsDouble(argument: GdalAlgorithmArgH) -> f64;
    pub(crate) fn GDALAlgorithmArgGetDefaultAsStringList(
        argument: GdalAlgorithmArgH,
    ) -> *mut *mut c_char;
    pub(crate) fn GDALAlgorithmArgGetDefaultAsIntegerList(
        argument: GdalAlgorithmArgH,
        count: *mut usize,
    ) -> *const c_int;
    pub(crate) fn GDALAlgorithmArgGetDefaultAsDoubleList(
        argument: GdalAlgorithmArgH,
        count: *mut usize,
    ) -> *const f64;
    pub(crate) fn GDALAlgorithmArgIsHidden(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgIsHiddenForAPI(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgIsAvailableInPipelineStep(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgIsInput(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgIsOutput(argument: GdalAlgorithmArgH) -> bool;
    pub(crate) fn GDALAlgorithmArgGetMutualExclusionGroup(
        argument: GdalAlgorithmArgH,
    ) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetMutualDependencyGroup(
        argument: GdalAlgorithmArgH,
    ) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetAsString(argument: GdalAlgorithmArgH) -> *const c_char;
    pub(crate) fn GDALAlgorithmArgGetAsDatasetValue(
        argument: GdalAlgorithmArgH,
    ) -> GdalArgDatasetValueH;
    pub(crate) fn GDALAlgorithmArgGetAsInteger(argument: GdalAlgorithmArgH) -> c_int;
    pub(crate) fn GDALAlgorithmArgGetDirectDependencies(
        argument: GdalAlgorithmArgH,
    ) -> *mut *mut c_char;
    pub(crate) fn GDALAlgorithmArgSetAsBoolean(argument: GdalAlgorithmArgH, value: bool) -> bool;
    pub(crate) fn GDALAlgorithmArgSetAsString(
        argument: GdalAlgorithmArgH,
        value: *const c_char,
    ) -> bool;
    pub(crate) fn GDALAlgorithmArgSetDataset(
        argument: GdalAlgorithmArgH,
        dataset: GdalDatasetH,
    ) -> bool;
    pub(crate) fn GDALAlgorithmArgSetAsInteger(argument: GdalAlgorithmArgH, value: c_int) -> bool;
    pub(crate) fn GDALAlgorithmArgSetAsDouble(argument: GdalAlgorithmArgH, value: f64) -> bool;
    pub(crate) fn GDALAlgorithmArgSetAsStringList(
        argument: GdalAlgorithmArgH,
        values: *const *const c_char,
    ) -> bool;
    pub(crate) fn GDALAlgorithmArgSetAsIntegerList(
        argument: GdalAlgorithmArgH,
        count: usize,
        values: *const c_int,
    ) -> bool;
    pub(crate) fn GDALAlgorithmArgSetAsDoubleList(
        argument: GdalAlgorithmArgH,
        count: usize,
        values: *const f64,
    ) -> bool;
    pub(crate) fn GDALAlgorithmArgGetDatasetType(argument: GdalAlgorithmArgH)
    -> GdalArgDatasetType;
    pub(crate) fn GDALAlgorithmArgGetDatasetInputFlags(argument: GdalAlgorithmArgH) -> c_int;

    // ---- dataset values -----------------------------------------------------------
    pub(crate) fn GDALArgDatasetValueRelease(value: GdalArgDatasetValueH);
    pub(crate) fn GDALArgDatasetValueGetName(value: GdalArgDatasetValueH) -> *const c_char;
    pub(crate) fn GDALArgDatasetValueGetDatasetIncreaseRefCount(
        value: GdalArgDatasetValueH,
    ) -> GdalDatasetH;

    // ---- gdal.h, beside ffi/gdal.rs -----------------------------------------------
    /// What `GDALDataset::Release()` is: a reference dropped, the dataset
    /// closed with its last.
    pub(crate) fn GDALReleaseDataset(dataset: GdalDatasetH) -> c_int;
    pub(crate) fn GDALGetDataTypeByName(name: *const c_char) -> GdalDataType;
    pub(crate) fn GDALDataTypeIsFloating(data_type: GdalDataType) -> c_int;
    pub(crate) fn GDALSetRasterColorInterpretation(
        band: GdalRasterBandH,
        interpretation: GdalColorInterp,
    ) -> CplErr;

    // ---- cpl_multiproc.h, cpl_vsi.h, beside ffi/cpl.rs ---------------------------
    /// The calling thread's id, whatever its name says (cpl_multiproc.h).
    pub(crate) fn CPLGetPID() -> i64;
    pub(crate) fn VSIMkdirRecursive(path: *const c_char, mode: c_long) -> c_int;
    pub(crate) fn VSIRmdirRecursive(path: *const c_char) -> c_int;

    // ---- ogr_api.h, ogr_srs_api.h, beside ffi/ogr.rs and ffi/osr.rs ---------------
    pub(crate) fn OGR_G_AssignSpatialReference(
        geometry: OgrGeometryH,
        reference: OgrSpatialReferenceH,
    );
    pub(crate) fn OGRGetGEOSVersion(
        major: *mut c_int,
        minor: *mut c_int,
        patch: *mut c_int,
    ) -> bool;
    pub(crate) fn OSRGetPROJVersion(major: *mut c_int, minor: *mut c_int, patch: *mut c_int);
}

// The shim (native/algorithm_shim.cpp, its C ABI in native/algorithm_shim.h),
// in the crate's own native library, which build.rs links. Every function
// takes the algorithm and, for an argument, its name, which the shim looks up
// with `GetArg(name)` - no suggestions - as the C++ does. Every one catches
// every exception: it is reported as a CE_Failure through CPLError, which the
// run's collector on this thread takes, and the function answers its failure
// value (NULL, 0). A list or text it returns is GDAL's to free (CSLDestroy,
// VSIFree).
unsafe extern "C" {
    /// `InstantiateSubAlgorithm(name, false)`: NULL for no such algorithm,
    /// with no suggestion raised. Released with GDALAlgorithmRelease.
    pub(crate) fn katana_gdal_algorithm_instantiate_sub_algorithm(
        algorithm: GdalAlgorithmH,
        name: *const c_char,
    ) -> GdalAlgorithmH;
    /// `IsHidden()`.
    pub(crate) fn katana_gdal_algorithm_is_hidden(algorithm: GdalAlgorithmH) -> c_int;
    /// `GetAliases()`, HIDDEN_ALIAS_SEPARATOR included where it stands.
    pub(crate) fn katana_gdal_algorithm_aliases(algorithm: GdalAlgorithmH) -> *mut *mut c_char;
    /// `GetSuggestionForArgumentName(name)`: "" for none.
    pub(crate) fn katana_gdal_algorithm_suggestion_for_argument_name(
        algorithm: GdalAlgorithmH,
        name: *const c_char,
    ) -> *mut c_char;
    /// `ValidateArguments()`.
    pub(crate) fn katana_gdal_algorithm_validate_arguments(algorithm: GdalAlgorithmH) -> c_int;
    /// `GetArg(name)->GetName()`: the long name of the argument a long name,
    /// a short name or an alias names; NULL for none.
    pub(crate) fn katana_gdal_algorithm_arg_name(
        algorithm: GdalAlgorithmH,
        name: *const c_char,
    ) -> *mut c_char;
    /// `GetAutoCompleteChoices(prefix)`.
    pub(crate) fn katana_gdal_arg_auto_complete_choices(
        algorithm: GdalAlgorithmH,
        argument: *const c_char,
        prefix: *const c_char,
    ) -> *mut *mut c_char;
    /// `GetMinValue()`: 0 when the argument does not exist.
    pub(crate) fn katana_gdal_arg_min_value(
        algorithm: GdalAlgorithmH,
        argument: *const c_char,
        value: *mut f64,
        inclusive: *mut c_int,
    ) -> c_int;
    /// `GetMaxValue()`: 0 when the argument does not exist.
    pub(crate) fn katana_gdal_arg_max_value(
        algorithm: GdalAlgorithmH,
        argument: *const c_char,
        value: *mut f64,
        inclusive: *mut c_int,
    ) -> c_int;
    /// The keys of `GetMetadata()`, in the map's order.
    pub(crate) fn katana_gdal_arg_metadata_keys(
        algorithm: GdalAlgorithmH,
        argument: *const c_char,
    ) -> *mut *mut c_char;
    /// `SetDatasetName(name)`.
    pub(crate) fn katana_gdal_arg_set_dataset_name(
        algorithm: GdalAlgorithmH,
        argument: *const c_char,
        name: *const c_char,
    ) -> c_int;
    /// `Set(std::vector<GDALArgDatasetValue>&&)`: value i is `datasets[i]`
    /// (its reference count increased) when that is not NULL, else the name
    /// `names[i]`.
    pub(crate) fn katana_gdal_arg_set_dataset_list(
        algorithm: GdalAlgorithmH,
        argument: *const c_char,
        count: usize,
        names: *const *const c_char,
        datasets: *const GdalDatasetH,
    ) -> c_int;
    /// The names of `Get<std::vector<GDALArgDatasetValue>>()`; NULL for an
    /// argument that is not a dataset list.
    pub(crate) fn katana_gdal_arg_dataset_list_names(
        algorithm: GdalAlgorithmH,
        argument: *const c_char,
    ) -> *mut *mut c_char;
}
