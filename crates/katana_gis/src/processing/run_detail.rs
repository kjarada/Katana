//! Shared by the bridge's two modules, never public
//! (src/katana_io/geo/gdal_run_detail.hpp): what the policy table needs to
//! see of a live algorithm, and the pipeline's words.
//!
//! The header includes gdalalgorithm.h; here that is the Rust owners of
//! GDAL's algorithm objects, which bridge.rs and policy.rs both hold:
//!
//! ```text
//! Algorithm   an algorithm GDAL made; released (GDALAlgorithmRelease) on drop
//! Arg         one of its arguments, borrowed from it for as long as it lives;
//!             the handle GDAL's C API hands out is released on drop
//! DatasetRef  a reference to a dataset, dropped (GDALReleaseDataset) with it:
//!             the C++'s DatasetRef, whose destructor calls Release()
//! ```
//!
//! Each method is one call: the C API where it calls what the C++ calls, the
//! shim (native/algorithm_shim.cpp) where it does not - ffi/algorithm.rs says
//! which and why. Neither owner is Send: an algorithm is made, run and
//! released on one thread, as the C++ does, and GDAL's per-thread error
//! handlers rely on it. An argument is looked up as the C++ looks it up, with
//! no suggestion raised for a name it lacks.
//!
//! The functions the header declares - classify, pipeline_words,
//! has_external_step, is_pipeline - are implemented in policy.rs, as the
//! C++'s are in policy.cpp, and named here for the bridge.

#![allow(
    unsafe_code,
    reason = "Owns GDAL's algorithm, argument and dataset handles and calls their C API and the crate's shim; every call states why it is sound."
)]

use std::collections::BTreeMap;
use std::ffi::{CString, c_char, c_int};
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use crate::ffi::algorithm as alg;
use crate::ffi::{cpl, gdal};

pub(crate) use super::policy::{classify, has_external_step, is_pipeline, pipeline_words};

/// One word of a pipeline as GDAL's pipeline argument takes it ("read x !
/// slope ! write y", nested steps in [ ]): `step` when it names a step - the
/// first word that is not an option after the text's start, a '!' or a '[' -
/// and `first` while no '!' or '[' has been passed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PipelineWord {
    pub(crate) text: String,
    pub(crate) step: bool,
    pub(crate) first: bool,
}

// ---- text and lists across the boundary ----------------------------------------------------

/// The strings of a NULL-terminated list GDAL allocated, decoded lossily, and
/// the list freed; empty for NULL.
///
/// # Safety
///
/// `list` is NULL or a NULL-terminated list of live strings that GDAL
/// allocated and that nothing else frees.
unsafe fn strings_of(list: *mut *mut c_char) -> Vec<String> {
    // SAFETY: the caller's contract above, which take_string_list asks.
    unsafe { cpl::take_string_list(list) }
        .unwrap_or_default()
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect()
}

/// A copy of a text GDAL allocated for the caller, and the text freed
/// (CPLFree); None for NULL.
///
/// # Safety
///
/// `text` is NULL or a live NUL-terminated string GDAL allocated with
/// CPLMalloc and that nothing else frees.
unsafe fn take_text(text: *mut c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: a live NUL-terminated string (the caller's contract).
    let copy = unsafe { cpl::decoded(text) };
    // SAFETY: GDAL's allocation, freed once, after the copy.
    unsafe { cpl::VSIFree(text.cast()) };
    Some(copy)
}

/// A NULL-terminated list of the items as C strings, never NULL itself (an
/// empty list is its terminator alone), alive for `call`.
fn with_string_list<R>(items: &[String], call: impl FnOnce(*const *const c_char) -> R) -> R {
    let strings: Vec<CString> = items.iter().map(cpl::c_string).collect();
    let pointers: Vec<*const c_char> = strings
        .iter()
        .map(|text| text.as_ptr())
        .chain([ptr::null()])
        .collect();
    call(pointers.as_ptr())
}

// ---- datasets ---------------------------------------------------------------------------------

/// A reference to a GDAL dataset, dropped with this value: the dataset closes
/// with its last reference (GDALReleaseDataset, the C++'s `Release()`). A
/// dataset GDALCreate made starts with one, which this takes over.
#[derive(Debug)]
pub(crate) struct DatasetRef {
    handle: NonNull<gdal::GdalDatasetHs>,
    // Not Send: the dataset is used on the thread of the run that holds it.
    _thread: PhantomData<*const ()>,
}

impl DatasetRef {
    /// Takes over a reference the caller holds; None for NULL.
    pub(crate) fn from_raw(handle: gdal::GdalDatasetH) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self {
            handle,
            _thread: PhantomData,
        })
    }

    pub(crate) fn raw(&self) -> gdal::GdalDatasetH {
        self.handle.as_ptr()
    }
}

impl Drop for DatasetRef {
    fn drop(&mut self) {
        // SAFETY: one reference this value holds, dropped once.
        unsafe { alg::GDALReleaseDataset(self.raw()) };
    }
}

/// One value of a dataset list: a name GDAL opens, or a dataset already open.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DatasetItem<'a> {
    Name(&'a str),
    Object(gdal::GdalDatasetH),
}

// ---- algorithms ----------------------------------------------------------------------------

/// An algorithm GDAL made for Rust: a fresh instance, run at most once
/// (`Run()` is single-shot), released on drop.
pub(crate) struct Algorithm {
    handle: NonNull<alg::GdalAlgorithmHs>,
}

impl Drop for Algorithm {
    fn drop(&mut self) {
        // SAFETY: the algorithm GDAL handed this value, released once; the
        // arguments borrowed from it are gone (they borrow the value).
        unsafe { alg::GDALAlgorithmRelease(self.raw()) };
    }
}

impl Algorithm {
    fn from_raw(handle: alg::GdalAlgorithmH) -> Option<Self> {
        NonNull::new(handle).map(|handle| Self { handle })
    }

    fn raw(&self) -> alg::GdalAlgorithmH {
        self.handle.as_ptr()
    }

    /// The root "gdal" algorithm, as the global registry instantiates it
    /// (`GDALGlobalAlgorithmRegistry::GetSingleton().Instantiate(ROOT_ALG_NAME)`).
    /// GDAL must be registered first.
    pub(crate) fn root() -> Option<Self> {
        // SAFETY: the registry handle is released after the one call that
        // uses it; the name is NUL-terminated; the algorithm returned is
        // owned by the value made from it.
        let handle = unsafe {
            let registry = alg::GDALGetGlobalAlgorithmRegistry();
            if registry.is_null() {
                return None;
            }
            let handle =
                alg::GDALAlgorithmRegistryInstantiateAlg(registry, alg::ROOT_ALG_NAME.as_ptr());
            alg::GDALAlgorithmRegistryRelease(registry);
            handle
        };
        Self::from_raw(handle)
    }

    /// `GetSubAlgorithmNames()`.
    pub(crate) fn sub_algorithm_names(&self) -> Vec<String> {
        // SAFETY: a live algorithm; the list returned is the caller's.
        unsafe { strings_of(alg::GDALAlgorithmGetSubAlgorithmNames(self.raw())) }
    }

    /// `InstantiateSubAlgorithm(name, false)`: a new algorithm, or None.
    pub(crate) fn instantiate_sub_algorithm(&self, name: &str) -> Option<Self> {
        let name = cpl::c_string(name);
        // SAFETY: a live algorithm and a NUL-terminated name; the algorithm
        // returned is owned by the value made from it.
        Self::from_raw(unsafe {
            alg::katana_gdal_algorithm_instantiate_sub_algorithm(self.raw(), name.as_ptr())
        })
    }

    /// `IsHidden()`.
    pub(crate) fn is_hidden(&self) -> bool {
        // SAFETY: a live algorithm.
        unsafe { alg::katana_gdal_algorithm_is_hidden(self.raw()) != 0 }
    }

    /// `GetAliases()`: the public ones, HIDDEN_ALIAS_SEPARATOR, the hidden
    /// ones.
    pub(crate) fn aliases(&self) -> Vec<String> {
        // SAFETY: a live algorithm; the list returned is the caller's.
        unsafe { strings_of(alg::katana_gdal_algorithm_aliases(self.raw())) }
    }

    /// `GetName()`.
    pub(crate) fn name(&self) -> String {
        // SAFETY: a live algorithm; its name is copied at once.
        unsafe { cpl::decoded(alg::GDALAlgorithmGetName(self.raw())) }
    }

    /// `GetDescription()`.
    pub(crate) fn description(&self) -> String {
        // SAFETY: a live algorithm; the text is copied at once.
        unsafe { cpl::decoded(alg::GDALAlgorithmGetDescription(self.raw())) }
    }

    /// `GetLongDescription()`.
    pub(crate) fn long_description(&self) -> String {
        // SAFETY: a live algorithm; the text is copied at once.
        unsafe { cpl::decoded(alg::GDALAlgorithmGetLongDescription(self.raw())) }
    }

    /// `GetHelpFullURL()`.
    pub(crate) fn help_full_url(&self) -> String {
        // SAFETY: a live algorithm; the text is copied at once.
        unsafe { cpl::decoded(alg::GDALAlgorithmGetHelpFullURL(self.raw())) }
    }

    /// `HasSubAlgorithms()`.
    pub(crate) fn has_sub_algorithms(&self) -> bool {
        // SAFETY: a live algorithm.
        unsafe { alg::GDALAlgorithmHasSubAlgorithms(self.raw()) }
    }

    /// `GetUsageAsJSON()`.
    pub(crate) fn usage_as_json(&self) -> String {
        // SAFETY: a live algorithm; the text returned is the caller's.
        unsafe { take_text(alg::GDALAlgorithmGetUsageAsJSON(self.raw())) }.unwrap_or_default()
    }

    /// `GetSuggestionForArgumentName(name)`: empty for none.
    pub(crate) fn suggestion_for_argument_name(&self, name: &str) -> String {
        let name = cpl::c_string(name);
        // SAFETY: a live algorithm and a NUL-terminated name; the text
        // returned is the caller's.
        unsafe {
            take_text(alg::katana_gdal_algorithm_suggestion_for_argument_name(
                self.raw(),
                name.as_ptr(),
            ))
        }
        .unwrap_or_default()
    }

    /// `ParseCommandLineArguments(tokens)`.
    pub(crate) fn parse_command_line_arguments(&self, tokens: &[String]) -> bool {
        with_string_list(tokens, |list| {
            // SAFETY: a live algorithm and a NULL-terminated list, alive for
            // the call; GDAL copies it.
            unsafe { alg::GDALAlgorithmParseCommandLineArguments(self.raw(), list) }
        })
    }

    /// `ValidateArguments()`.
    pub(crate) fn validate_arguments(&self) -> bool {
        // SAFETY: a live algorithm.
        unsafe { alg::katana_gdal_algorithm_validate_arguments(self.raw()) != 0 }
    }

    /// `Run(progress, data)`.
    ///
    /// # Safety
    ///
    /// `progress` may be called with `data` from this thread and from
    /// threads GDAL starts, until this returns: `data` is what `progress`
    /// expects, alive and safe to share for that long.
    pub(crate) unsafe fn run(
        &self,
        progress: gdal::GdalProgressFunc,
        data: *mut std::ffi::c_void,
    ) -> bool {
        // SAFETY: a live algorithm; the progress function and its data are
        // the caller's contract above.
        unsafe { alg::GDALAlgorithmRun(self.raw(), progress, data) }
    }

    /// `Finalize()`.
    pub(crate) fn finalize(&self) -> bool {
        // SAFETY: a live algorithm.
        unsafe { alg::GDALAlgorithmFinalize(self.raw()) }
    }

    /// `GetArg(name)`: by its long name, a short name or an alias, as the
    /// C++ looks one up - no suggestion raised for a name it lacks. The shim
    /// gives the long name, and GDAL's C API the argument by it, which GDAL
    /// finds before any suggestion is tried.
    pub(crate) fn arg(&self, name: &str) -> Option<Arg<'_>> {
        let name = cpl::c_string(name);
        // SAFETY: a live algorithm and a NUL-terminated name; the text
        // returned is the caller's.
        let long = unsafe {
            take_text(alg::katana_gdal_algorithm_arg_name(
                self.raw(),
                name.as_ptr(),
            ))
        }?;
        self.arg_by_long_name(cpl::c_string(long))
    }

    /// `GetArgs()`, in GDAL's order.
    pub(crate) fn args(&self) -> Vec<Arg<'_>> {
        // SAFETY: a live algorithm; the list returned is the caller's.
        let names = unsafe { cpl::take_string_list(alg::GDALAlgorithmGetArgNames(self.raw())) };
        names
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| self.arg_by_long_name(cpl::c_string(name)))
            .collect()
    }

    fn arg_by_long_name(&self, name: CString) -> Option<Arg<'_>> {
        // SAFETY: a live algorithm and a NUL-terminated long name, which
        // GDAL finds without trying a suggestion; the handle returned is
        // owned by the Arg made from it.
        let handle = unsafe { alg::GDALAlgorithmGetArgNonConst(self.raw(), name.as_ptr()) };
        NonNull::new(handle).map(|handle| Arg {
            handle,
            algorithm: self,
            name,
        })
    }
}

// ---- arguments -----------------------------------------------------------------------------

/// One argument of an algorithm, valid while the algorithm lives. The shim
/// reaches it again by its long name.
pub(crate) struct Arg<'a> {
    handle: NonNull<alg::GdalAlgorithmArgHs>,
    algorithm: &'a Algorithm,
    name: CString,
}

impl Drop for Arg<'_> {
    fn drop(&mut self) {
        // SAFETY: the handle GDAL's C API made for this value, released once;
        // the argument itself is the algorithm's and stays.
        unsafe { alg::GDALAlgorithmArgRelease(self.raw()) };
    }
}

/// A reader of one of the argument's texts.
type TextOf = unsafe extern "C" fn(alg::GdalAlgorithmArgH) -> *const c_char;
/// A reader of one of the argument's flags.
type FlagOf = unsafe extern "C" fn(alg::GdalAlgorithmArgH) -> bool;
/// A reader of one of the argument's lists.
type ListOf = unsafe extern "C" fn(alg::GdalAlgorithmArgH) -> *mut *mut c_char;

impl Arg<'_> {
    fn raw(&self) -> alg::GdalAlgorithmArgH {
        self.handle.as_ptr()
    }

    fn text(&self, read: TextOf) -> String {
        // SAFETY: a live argument of a live algorithm; the text is the
        // argument's own, copied at once.
        unsafe { cpl::decoded(read(self.raw())) }
    }

    fn flag(&self, read: FlagOf) -> bool {
        // SAFETY: a live argument of a live algorithm.
        unsafe { read(self.raw()) }
    }

    fn list(&self, read: ListOf) -> Vec<String> {
        // SAFETY: a live argument of a live algorithm; the list returned is
        // the caller's.
        unsafe { strings_of(read(self.raw())) }
    }

    pub(crate) fn name(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetName)
    }

    pub(crate) fn short_name(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetShortName)
    }

    pub(crate) fn description(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetDescription)
    }

    pub(crate) fn meta_var(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetMetaVar)
    }

    pub(crate) fn category(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetCategory)
    }

    pub(crate) fn mutual_exclusion_group(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetMutualExclusionGroup)
    }

    pub(crate) fn mutual_dependency_group(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetMutualDependencyGroup)
    }

    pub(crate) fn aliases(&self) -> Vec<String> {
        self.list(alg::GDALAlgorithmArgGetAliases)
    }

    pub(crate) fn choices(&self) -> Vec<String> {
        self.list(alg::GDALAlgorithmArgGetChoices)
    }

    pub(crate) fn direct_dependencies(&self) -> Vec<String> {
        self.list(alg::GDALAlgorithmArgGetDirectDependencies)
    }

    pub(crate) fn r#type(&self) -> alg::GdalAlgorithmArgType {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetType(self.raw()) }
    }

    /// GAAT_DATASET or GAAT_DATASET_LIST.
    pub(crate) fn is_dataset(&self) -> bool {
        let r#type = self.r#type();
        r#type == alg::GAAT_DATASET || r#type == alg::GAAT_DATASET_LIST
    }

    pub(crate) fn is_required(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsRequired)
    }

    pub(crate) fn is_positional(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsPositional)
    }

    pub(crate) fn is_input(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsInput)
    }

    pub(crate) fn is_output(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsOutput)
    }

    pub(crate) fn is_hidden(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsHidden)
    }

    pub(crate) fn is_hidden_for_api(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsHiddenForAPI)
    }

    pub(crate) fn is_explicitly_set(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsExplicitlySet)
    }

    pub(crate) fn is_available_in_pipeline_step(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgIsAvailableInPipelineStep)
    }

    pub(crate) fn packed_values_allowed(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgGetPackedValuesAllowed)
    }

    pub(crate) fn repeated_arg_allowed(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgGetRepeatedArgAllowed)
    }

    pub(crate) fn has_default_value(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgHasDefaultValue)
    }

    pub(crate) fn min_count(&self) -> c_int {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetMinCount(self.raw()) }
    }

    pub(crate) fn max_count(&self) -> c_int {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetMaxCount(self.raw()) }
    }

    /// `GetDatasetType()`: GDAL_OF_RASTER, _VECTOR, _MULTIDIM_RASTER and
    /// _UPDATE bits.
    pub(crate) fn dataset_type(&self) -> c_int {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetDatasetType(self.raw()) }
    }

    /// `GetDatasetInputFlags()`: GADV_NAME and GADV_OBJECT bits.
    pub(crate) fn dataset_input_flags(&self) -> c_int {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetDatasetInputFlags(self.raw()) }
    }

    // ---- defaults ---------------------------------------------------------------------

    pub(crate) fn default_boolean(&self) -> bool {
        self.flag(alg::GDALAlgorithmArgGetDefaultAsBoolean)
    }

    pub(crate) fn default_string(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetDefaultAsString)
    }

    pub(crate) fn default_integer(&self) -> i32 {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetDefaultAsInteger(self.raw()) }
    }

    pub(crate) fn default_real(&self) -> f64 {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetDefaultAsDouble(self.raw()) }
    }

    pub(crate) fn default_string_list(&self) -> Vec<String> {
        self.list(alg::GDALAlgorithmArgGetDefaultAsStringList)
    }

    pub(crate) fn default_integer_list(&self) -> Vec<i32> {
        let mut count: usize = 0;
        // SAFETY: a live argument of a live algorithm and an out count; the
        // values are the argument's own, copied at once.
        unsafe {
            let values = alg::GDALAlgorithmArgGetDefaultAsIntegerList(self.raw(), &raw mut count);
            if values.is_null() || count == 0 {
                return Vec::new();
            }
            std::slice::from_raw_parts(values, count).to_vec()
        }
    }

    pub(crate) fn default_real_list(&self) -> Vec<f64> {
        let mut count: usize = 0;
        // SAFETY: a live argument of a live algorithm and an out count; the
        // values are the argument's own, copied at once.
        unsafe {
            let values = alg::GDALAlgorithmArgGetDefaultAsDoubleList(self.raw(), &raw mut count);
            if values.is_null() || count == 0 {
                return Vec::new();
            }
            std::slice::from_raw_parts(values, count).to_vec()
        }
    }

    // ---- what only the shim reaches ---------------------------------------------------

    /// `GetMinValue()`: the bound, NaN for none, and whether it is included.
    pub(crate) fn min_value(&self) -> (f64, bool) {
        let mut value = f64::NAN;
        let mut inclusive: c_int = 1;
        // SAFETY: a live algorithm, the argument's NUL-terminated long name
        // and two out values.
        unsafe {
            alg::katana_gdal_arg_min_value(
                self.algorithm.raw(),
                self.name.as_ptr(),
                &raw mut value,
                &raw mut inclusive,
            );
        }
        (value, inclusive != 0)
    }

    /// `GetMaxValue()`: the bound, NaN for none, and whether it is included.
    pub(crate) fn max_value(&self) -> (f64, bool) {
        let mut value = f64::NAN;
        let mut inclusive: c_int = 1;
        // SAFETY: a live algorithm, the argument's NUL-terminated long name
        // and two out values.
        unsafe {
            alg::katana_gdal_arg_max_value(
                self.algorithm.raw(),
                self.name.as_ptr(),
                &raw mut value,
                &raw mut inclusive,
            );
        }
        (value, inclusive != 0)
    }

    /// `GetMetadata()`: every key with its values. The keys through the shim,
    /// each key's values through the C API.
    pub(crate) fn metadata(&self) -> BTreeMap<String, Vec<String>> {
        // SAFETY: a live algorithm and the argument's NUL-terminated long
        // name; the list returned is the caller's.
        let keys = unsafe {
            cpl::take_string_list(alg::katana_gdal_arg_metadata_keys(
                self.algorithm.raw(),
                self.name.as_ptr(),
            ))
        };
        let mut metadata = BTreeMap::new();
        for key in keys.unwrap_or_default() {
            let item = cpl::c_string(&key);
            // SAFETY: a live argument and a NUL-terminated key; the list
            // returned is the caller's.
            let values = unsafe {
                strings_of(alg::GDALAlgorithmArgGetMetadataItem(
                    self.raw(),
                    item.as_ptr(),
                ))
            };
            metadata.insert(String::from_utf8_lossy(&key).into_owned(), values);
        }
        metadata
    }

    /// `GetAutoCompleteChoices(prefix)`.
    pub(crate) fn auto_complete_choices(&self, prefix: &str) -> Vec<String> {
        let prefix = cpl::c_string(prefix);
        // SAFETY: a live algorithm, the argument's NUL-terminated long name
        // and a NUL-terminated prefix; the list returned is the caller's.
        unsafe {
            strings_of(alg::katana_gdal_arg_auto_complete_choices(
                self.algorithm.raw(),
                self.name.as_ptr(),
                prefix.as_ptr(),
            ))
        }
    }

    /// `SetDatasetName(name)`.
    pub(crate) fn set_dataset_name(&self, name: &str) -> bool {
        let name = cpl::c_string(name);
        // SAFETY: a live algorithm, the argument's NUL-terminated long name
        // and a NUL-terminated dataset name, which GDAL copies.
        unsafe {
            alg::katana_gdal_arg_set_dataset_name(
                self.algorithm.raw(),
                self.name.as_ptr(),
                name.as_ptr(),
            ) != 0
        }
    }

    /// `Set(std::vector<GDALArgDatasetValue>&&)`: names, open datasets (each
    /// referenced by the argument), or both.
    pub(crate) fn set_dataset_list(&self, items: &[DatasetItem<'_>]) -> bool {
        let names: Vec<CString> = items
            .iter()
            .map(|item| match item {
                DatasetItem::Name(name) => cpl::c_string(name),
                DatasetItem::Object(_) => CString::default(),
            })
            .collect();
        let name_pointers: Vec<*const c_char> = names.iter().map(|name| name.as_ptr()).collect();
        let datasets: Vec<gdal::GdalDatasetH> = items
            .iter()
            .map(|item| match item {
                DatasetItem::Name(_) => ptr::null_mut(),
                DatasetItem::Object(dataset) => *dataset,
            })
            .collect();
        // SAFETY: a live algorithm, the argument's NUL-terminated long name,
        // and two arrays of `items.len()` entries alive for the call: names
        // GDAL copies, and NULL or live datasets it references.
        unsafe {
            alg::katana_gdal_arg_set_dataset_list(
                self.algorithm.raw(),
                self.name.as_ptr(),
                items.len(),
                name_pointers.as_ptr(),
                datasets.as_ptr(),
            ) != 0
        }
    }

    /// The names of `Get<std::vector<GDALArgDatasetValue>>()`.
    pub(crate) fn dataset_list_names(&self) -> Vec<String> {
        // SAFETY: a live algorithm and the argument's NUL-terminated long
        // name; the list returned is the caller's.
        unsafe {
            strings_of(alg::katana_gdal_arg_dataset_list_names(
                self.algorithm.raw(),
                self.name.as_ptr(),
            ))
        }
    }

    // ---- setting and reading values -----------------------------------------------------

    /// `Set(bool)`.
    pub(crate) fn set_boolean(&self, value: bool) -> bool {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgSetAsBoolean(self.raw(), value) }
    }

    /// `Set(const std::string&)`.
    pub(crate) fn set_string(&self, value: &str) -> bool {
        let value = cpl::c_string(value);
        // SAFETY: a live argument and a NUL-terminated value GDAL copies.
        unsafe { alg::GDALAlgorithmArgSetAsString(self.raw(), value.as_ptr()) }
    }

    /// `Set(int)`.
    pub(crate) fn set_integer(&self, value: i32) -> bool {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgSetAsInteger(self.raw(), value) }
    }

    /// `Set(double)`.
    pub(crate) fn set_real(&self, value: f64) -> bool {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgSetAsDouble(self.raw(), value) }
    }

    /// `Set(const std::vector<std::string>&)`.
    pub(crate) fn set_string_list(&self, values: &[String]) -> bool {
        with_string_list(values, |list| {
            // SAFETY: a live argument and a NULL-terminated list alive for
            // the call, which GDAL copies.
            unsafe { alg::GDALAlgorithmArgSetAsStringList(self.raw(), list) }
        })
    }

    /// `Set(const std::vector<int>&)`.
    pub(crate) fn set_integer_list(&self, values: &[i32]) -> bool {
        // SAFETY: a live argument and `values.len()` ints GDAL copies.
        unsafe { alg::GDALAlgorithmArgSetAsIntegerList(self.raw(), values.len(), values.as_ptr()) }
    }

    /// `Set(const std::vector<double>&)`.
    pub(crate) fn set_real_list(&self, values: &[f64]) -> bool {
        // SAFETY: a live argument and `values.len()` doubles GDAL copies.
        unsafe { alg::GDALAlgorithmArgSetAsDoubleList(self.raw(), values.len(), values.as_ptr()) }
    }

    /// `Set(GDALDataset*)`: the argument references the dataset.
    pub(crate) fn set_dataset(&self, dataset: gdal::GdalDatasetH) -> bool {
        // SAFETY: a live argument and a live dataset, which GDAL references.
        unsafe { alg::GDALAlgorithmArgSetDataset(self.raw(), dataset) }
    }

    /// `Get<std::string>()`.
    pub(crate) fn string_value(&self) -> String {
        self.text(alg::GDALAlgorithmArgGetAsString)
    }

    /// `Get<int>()`.
    pub(crate) fn integer_value(&self) -> i32 {
        // SAFETY: a live argument of a live algorithm.
        unsafe { alg::GDALAlgorithmArgGetAsInteger(self.raw()) }
    }

    /// `Get<GDALArgDatasetValue>().GetName()`.
    pub(crate) fn dataset_name(&self) -> String {
        // SAFETY: a live argument; the value handle returned points at the
        // argument's own value, is read before it is released, and is
        // released once.
        unsafe {
            let value = alg::GDALAlgorithmArgGetAsDatasetValue(self.raw());
            if value.is_null() {
                return String::new();
            }
            let name = cpl::decoded(alg::GDALArgDatasetValueGetName(value));
            alg::GDALArgDatasetValueRelease(value);
            name
        }
    }

    /// `Get<GDALArgDatasetValue>().GetDatasetIncreaseRefCount()`: a reference
    /// of the caller's own, or None when the argument holds no dataset.
    pub(crate) fn dataset_increase_ref_count(&self) -> Option<DatasetRef> {
        // SAFETY: a live argument; the value handle returned points at the
        // argument's own value, is read before it is released, and is
        // released once; the dataset reference taken is the DatasetRef's.
        let dataset = unsafe {
            let value = alg::GDALAlgorithmArgGetAsDatasetValue(self.raw());
            if value.is_null() {
                return None;
            }
            let dataset = alg::GDALArgDatasetValueGetDatasetIncreaseRefCount(value);
            alg::GDALArgDatasetValueRelease(value);
            dataset
        };
        DatasetRef::from_raw(dataset)
    }
}
