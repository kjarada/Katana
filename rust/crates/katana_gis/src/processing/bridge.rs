//! The GDAL algorithm bridge (include/katana/gis/processing.hpp,
//! src/katana_io/geo/processing.cpp, docs/geoprocessing.md "The bridge"):
//! GDAL 3.13's algorithm framework behind plain types - the catalogue of what
//! GDAL can do, each algorithm's arguments as data, and a run with progress
//! and cancel. The rules each block follows were measured by the probes in
//! docs/geoprocessing.md; a rule here without a probe behind it is a defect.
//!
//! Every rule below was observed, not assumed:
//!
//! ```text
//! the catalogue is walked from the root "gdal" algorithm: the registry's own
//!   top-level list misses the `driver` family;
//! a run uses a fresh algorithm instance (Run() is single-shot), and every
//!   in-memory input becomes its own MEM dataset inside run(): one input
//!   shared between concurrent runs gave 63 wrong results in 120;
//! GDAL's command-line tail is parsed by GDAL itself AFTER the in-memory
//!   datasets are bound, so list, CRS and choice parsing is exactly GDAL's;
//! Run()'s return value is not trusted after a cancel (hillshade returns true
//!   with a partial output): the progress adapter records that it refused,
//!   and such a run is "cancelled" whatever GDAL said;
//! errors are collected per run by a thread-local handler. Errors raised on
//!   GDAL's own worker threads skip it and reach the process-wide quiet
//!   handler; the failure they cause still reaches the Result through Run().
//! ```
//!
//! Threading: every function may be called from any thread. Concurrent runs
//! share nothing but the read-only catalogue.
//!
//! The Rust shape (docs/rust.md, step 5):
//!
//! ```text
//! run's stop       Option<&AtomicBool>, read with Acquire, as katana_cad's
//!                  drape and reproject's warp have it: the C++'s stop_token
//! run's progress   Option<&(dyn Fn(f64) + Sync)>: GDAL's viewshed calls it
//!                  from threads of its own. The adapter is extern "C" and
//!                  catches a panic of the closure, refuses GDAL's progress,
//!                  and raises the panic again once Run() has returned
//! dataset handle   run_detail::DatasetRef, one reference released on drop
//!                  (GDALReleaseDataset), the C++'s DatasetRef
//! the catalogue    a OnceLock, walked once on first use with GDAL's errors
//!                  quieted, the C++'s call_once
//! a sub-algorithm  made by the shim with suggestions off, as the C++'s
//!                  InstantiateSubAlgorithm(name, false): GDAL's C twin asks
//!                  with them on (ffi/algorithm.rs)
//! the overloads    the C++'s toString(ArgType), toString(Policy) and
//!                  toString(const Scalar&) are `to_string`, over the trait
//!                  `Words`
//! ```

#![allow(
    unsafe_code,
    reason = "Builds GDAL datasets in memory, reads them back and drives GDAL's algorithm framework through its C API; every call states why it is sound."
)]

use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, is_ascii_space};
use katana_core::{Error, ErrorCode, Result, Status};

use super::policy::check_tokens;
use super::run_detail::{self, Algorithm, Arg, DatasetItem, DatasetRef};
use super::types::{
    AlgorithmInfo, AlgorithmSpec, ArgSpec, ArgType, ArgValue, Bound, DatasetKind, DatasetValue,
    Diagnostic, Feature, FeatureSet, FeatureTable, FieldDef, FieldType, FieldValue, OutputTo,
    Policy, RasterGrid, RunOutputs, RunRequest, Scalar, Versions,
};
use crate::ffi::algorithm as alg;
use crate::ffi::{cpl, gdal, ogr, osr};
use crate::formats_detail::remove_path;
use crate::gdal_adapter::{GeometryKind, RasterInfo, driver_by_name, metadata_item};
use crate::gdal_registry::ensure_registered;
use crate::ogr_detail::{
    GeometryView, OgrGeometry, SpatialReference, flatten_ogr_geometry, flattened,
    make_ogr_geometry, parse_crs,
};

// ---- words ---------------------------------------------------------------------------------

/// The items with `separator` between them - except that the separator goes
/// in only once the text is not empty, so leading empty items vanish, as the
/// C++'s `joined` has it.
fn joined<S: AsRef<str>>(items: &[S], separator: &str) -> String {
    let mut text = String::new();
    for item in items {
        if !text.is_empty() {
            text.push_str(separator);
        }
        text.push_str(item.as_ref());
    }
    text
}

/// A dataset name that is a file on disk, as opposed to /vsimem/..., a /vsi
/// chain, a URL or a connection string.
fn is_plain_file(name: &str) -> bool {
    !name.is_empty() && !name.starts_with("/vsi") && !name.contains("://") && !name.starts_with(':')
}

// ---- errors: one collector per run, on the run's own thread ------------------------------

/// CPLPushErrorHandlerEx is a per-thread stack, so a collector sees exactly
/// the errors of the run on its thread (8 threads x 50 runs: 200 errors seen,
/// 0 leaked between them). Errors GDAL raises on threads it starts itself -
/// the viewshed's - skip it and reach the process-wide CPLQuietErrorHandler
/// the registry installs; the failure they cause still reaches Run().
///
/// A guard, as cpl_error_collector.rs's is, keeping GDAL's error number too:
/// the handler is pushed with the address of the boxed list, popped on drop;
/// neither Send nor Sync, since a handler is popped on the thread that pushed
/// it.
struct ErrorCollector {
    items: Box<RefCell<Vec<Diagnostic>>>,
    _thread: PhantomData<*const ()>,
}

impl ErrorCollector {
    fn new() -> Self {
        let collector = Self {
            items: Box::new(RefCell::new(Vec::new())),
            _thread: PhantomData,
        };
        let data: *const RefCell<Vec<Diagnostic>> = &*collector.items;
        // SAFETY: the user data is the boxed list, which lives as long as the
        // collector, whose Drop pops this handler on this thread before the
        // box is freed.
        unsafe { cpl::CPLPushErrorHandlerEx(Some(collect), data.cast_mut().cast::<c_void>()) };
        collector
    }

    fn size(&self) -> usize {
        self.items.borrow().len()
    }

    fn items(&self) -> Vec<Diagnostic> {
        self.items.borrow().clone()
    }

    /// The first failure raised since `from`.
    fn first_failure(&self, from: usize) -> Option<Diagnostic> {
        self.items
            .borrow()
            .iter()
            .skip(from)
            .find(|item| item.failure)
            .cloned()
    }
}

impl Drop for ErrorCollector {
    fn drop(&mut self) {
        // SAFETY: pops the handler `new` pushed on this thread; handlers
        // pushed after it were popped by their own guards, dropped first.
        unsafe { cpl::CPLPopErrorHandler() };
    }
}

extern "C" fn collect(kind: cpl::CplErr, number: cpl::CplErrorNum, message: *const c_char) {
    if kind == cpl::CE_DEBUG || kind == cpl::CE_NONE {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: GDAL calls this handler only while it is the top of this
        // thread's stack, whose user data is a collector's boxed list.
        let data = unsafe { cpl::CPLGetErrorHandlerUserData() };
        if data.is_null() {
            return;
        }
        // SAFETY: the user data is a live RefCell<Vec<Diagnostic>> (above).
        let items = unsafe { &*data.cast::<RefCell<Vec<Diagnostic>>>() };
        // SAFETY: GDAL passes NULL or a NUL-terminated message, valid for the
        // call.
        let message = unsafe { cpl::decoded(message) };
        if let Ok(mut items) = items.try_borrow_mut() {
            items.push(Diagnostic {
                failure: kind >= cpl::CE_FAILURE,
                code: number,
                message,
            });
        }
    }));
}

/// The refusal for a failure GDAL raised, in `code`, carrying its own message,
/// which already says which algorithm ("hillshade: ...").
fn gdal_error(errors: &ErrorCollector, code: ErrorCode, fallback: String, from: usize) -> Error {
    match errors.first_failure(from) {
        Some(failure) => Error::new(code, failure.message),
        None => Error::new(code, fallback),
    }
}

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

// ---- thread-local configuration --------------------------------------------------------------

/// Allow-listed: options that tune a run and cannot change what it reads,
/// writes or runs. Thread-local, so concurrent runs cannot see each other's;
/// and for that reason they do not reach threads GDAL starts (measured), so
/// nothing that must be right may depend on them.
const ALLOWED_CONFIG: [&str; 2] = ["GDAL_NUM_THREADS", "GDAL_CACHEMAX"];

/// The options a run sets on its thread, put back as they were, last first,
/// when it ends.
#[derive(Default)]
struct ThreadConfig {
    previous: Vec<(CString, Option<CString>)>,
    // Not Send: the options are this thread's.
    _thread: PhantomData<*const ()>,
}

impl ThreadConfig {
    fn set(&mut self, options: &[(String, String)]) -> Status {
        for (key, value) in options {
            if !ALLOWED_CONFIG.contains(&key.as_str()) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "only GDAL_NUM_THREADS and GDAL_CACHEMAX may be set for a run",
                    key.clone(),
                ));
            }
            let key = cpl::c_string(key);
            let value = cpl::c_string(value);
            // SAFETY: a NUL-terminated key and a NULL default; the value
            // returned is this thread's, copied before it is replaced.
            let before = unsafe { cpl::CPLGetThreadLocalConfigOption(key.as_ptr(), ptr::null()) };
            // SAFETY: NULL or a live NUL-terminated string (above).
            let before =
                (!before.is_null()).then(|| CString::from(unsafe { CStr::from_ptr(before) }));
            // SAFETY: NUL-terminated key and value; GDAL copies both.
            unsafe { cpl::CPLSetThreadLocalConfigOption(key.as_ptr(), value.as_ptr()) };
            self.previous.push((key, before));
        }
        Ok(())
    }
}

impl Drop for ThreadConfig {
    fn drop(&mut self) {
        for (key, value) in self.previous.iter().rev() {
            let value = value.as_ref().map_or(ptr::null(), |value| value.as_ptr());
            // SAFETY: a NUL-terminated key and NULL or a NUL-terminated value,
            // on the thread that set the option.
            unsafe { cpl::CPLSetThreadLocalConfigOption(key.as_ptr(), value) };
        }
    }
}

// ---- sidecars ----------------------------------------------------------------------------

/// No .aux.xml may be left beside a person's file by a run that only reads
/// it: `raster info --stats` writes one when the dataset closes. Neither
/// switch GDAL offers does it (measured, docs/geoprocessing.md "Sidecars"): a
/// thread-local GDAL_PAM_ENABLED=NO stops the writing but also the READING of
/// a sidecar the file already has - its metadata, no-data or georeferencing -
/// and GDAL_PAM_PROXY_DIR is used only where the file's own folder is not
/// writable. So the sidecar beside each input read by name is noted before
/// the run and put back as it was after it: removed when the run made it,
/// restored byte for byte when the run changed it. Inputs opened for update
/// are left alone: what such an algorithm writes is the change asked for.
#[derive(Default)]
struct SidecarGuard {
    kept: Vec<Kept>,
}

struct Kept {
    sidecar: PathBuf,
    bytes: Option<Vec<u8>>,
}

impl SidecarGuard {
    fn watch(&mut self, name: &str) {
        if !is_plain_file(name) {
            return;
        }
        let sidecar = PathBuf::from(format!("{name}.aux.xml"));
        if self.kept.iter().any(|kept| kept.sidecar == sidecar) {
            return;
        }
        let bytes = sidecar
            .exists()
            .then(|| std::fs::read(&sidecar).unwrap_or_default());
        self.kept.push(Kept { sidecar, bytes });
    }
}

impl Drop for SidecarGuard {
    /// Once every dataset is closed: the algorithm is gone.
    fn drop(&mut self) {
        for kept in &self.kept {
            let exists = kept.sidecar.exists();
            let Some(bytes) = &kept.bytes else {
                if exists {
                    remove_path(&kept.sidecar);
                }
                continue;
            };
            let now = if exists {
                std::fs::read(&kept.sidecar).unwrap_or_default()
            } else {
                Vec::new()
            };
            if !exists || now != *bytes {
                let _ = std::fs::write(&kept.sidecar, bytes);
            }
        }
        self.kept.clear();
    }
}

// ---- staging for arguments that take names only ------------------------------------------------

/// raster calc's inputs, the index inputs and a few more accept a dataset by
/// NAME only, so an in-memory grid or feature set is written under
/// /vsimem/katana/<run>/ for them and the folder removed when the run ends.
const STAGING_ROOT: &str = "/vsimem/katana";

/// The runs that staged, for each its own folder.
static STAGED_RUNS: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Staging {
    folder: String,
    count: i32,
}

impl Staging {
    fn stage(&mut self, dataset: gdal::GdalDatasetH, raster: bool) -> Result<String> {
        if self.folder.is_empty() {
            let run = STAGED_RUNS.fetch_add(1, Ordering::Relaxed) + 1;
            self.folder = format!("{STAGING_ROOT}/{run}");
            let folder = cpl::c_string(&self.folder);
            // SAFETY: a NUL-terminated /vsimem path.
            unsafe { alg::VSIMkdirRecursive(folder.as_ptr(), 0o755) };
        }
        self.count += 1;
        let name = format!(
            "{}/{}{}",
            self.folder,
            self.count,
            if raster { ".tif" } else { ".gpkg" }
        );
        let driver_name = if raster { "GTiff" } else { "GPKG" };
        let driver = driver_by_name(driver_name);
        if driver.is_null() {
            return Err(make_error(
                ErrorCode::Internal,
                "GDAL has no driver to stage a dataset with",
                driver_name,
            ));
        }
        let target = cpl::c_string(&name);
        // SAFETY: a registered driver, a NUL-terminated name, a live source
        // dataset, no options and no progress; the copy returned is closed
        // below.
        let copy = unsafe {
            gdal::GDALCreateCopy(
                driver,
                target.as_ptr(),
                dataset,
                cpl::FALSE,
                ptr::null(),
                None,
                ptr::null_mut(),
            )
        };
        if copy.is_null() {
            return Err(make_error(
                ErrorCode::Internal,
                "could not stage an input for GDAL",
                name,
            ));
        }
        // SAFETY: the copy made above, closed once.
        unsafe { gdal::GDALClose(copy) };
        Ok(name)
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.folder.is_empty() {
            let folder = cpl::c_string(&self.folder);
            // SAFETY: a NUL-terminated /vsimem path.
            unsafe { alg::VSIRmdirRecursive(folder.as_ptr()) };
        }
    }
}

// ---- the catalogue -------------------------------------------------------------------------

#[derive(Default)]
struct CatalogueData {
    infos: Vec<AlgorithmInfo>,
    /// Parallel to infos: GDAL's hidden aliases ("warp" for reproject), which
    /// resolve() accepts and nothing lists.
    hidden_aliases: Vec<Vec<String>>,
}

static CATALOGUE: OnceLock<CatalogueData> = OnceLock::new();

fn root_algorithm() -> Option<Algorithm> {
    ensure_registered();
    Algorithm::root()
}

fn walk(algorithm: &Algorithm, path: &[String], out: &mut CatalogueData) {
    for name in algorithm.sub_algorithm_names() {
        let Some(sub) = algorithm.instantiate_sub_algorithm(&name) else {
            continue;
        };
        if sub.is_hidden() {
            continue;
        }
        let mut info_path = path.to_vec();
        info_path.push(name);
        let mut aliases = Vec::new();
        let mut hidden = Vec::new();
        let mut past_separator = false;
        for alias in sub.aliases() {
            if alias == alg::HIDDEN_ALIAS_SEPARATOR {
                past_separator = true;
                continue;
            }
            if past_separator {
                hidden.push(alias);
            } else {
                aliases.push(alias);
            }
        }
        let container = sub.has_sub_algorithms();
        let mut policy_reason = String::new();
        let policy = if container {
            // A group runs nothing; its members carry the policy.
            policy_reason.push_str("container");
            Policy::Safe
        } else {
            run_detail::classify(&info_path, &sub, &mut policy_reason)
        };
        let info = AlgorithmInfo {
            path: info_path,
            aliases,
            description: sub.description(),
            help_url: sub.help_full_url(),
            container,
            policy,
            policy_reason,
        };
        let sub_path = info.path.clone();
        out.infos.push(info);
        out.hidden_aliases.push(hidden);
        if container {
            walk(&sub, &sub_path, out);
        }
    }
}

fn catalogue_data() -> &'static CatalogueData {
    CATALOGUE.get_or_init(|| {
        // Anything the walk says is not the caller's business.
        let _quiet = ErrorCollector::new();
        let mut data = CatalogueData::default();
        if let Some(root) = root_algorithm() {
            walk(&root, &[], &mut data);
        }
        data
    })
}

fn find_info(path: &[String]) -> Option<&'static AlgorithmInfo> {
    catalogue_data().infos.iter().find(|info| info.path == path)
}

/// A fresh instance for every use: Run() is single-shot (a second call fails
/// with a misleading ": No such file or directory"). Walked down from the
/// root, which reaches the driver family the registry's own lookup misses.
fn instantiate(path: &[String]) -> Result<Algorithm> {
    if path.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "name an algorithm; GDAL LIST names them",
        ));
    }
    let mut current = root_algorithm();
    for name in path {
        let Some(parent) = current else {
            break;
        };
        current = parent.instantiate_sub_algorithm(name);
    }
    current.ok_or_else(|| {
        make_error(
            ErrorCode::NotFound,
            "no GDAL algorithm has that path",
            path_text(path),
        )
    })
}

fn children_of(path: &[String]) -> String {
    let children: Vec<&str> = catalogue_data()
        .infos
        .iter()
        .filter(|info| info.path.len() == path.len() + 1 && info.path.starts_with(path))
        .filter_map(|info| info.path.last().map(String::as_str))
        .collect();
    joined(&children, " ")
}

// ---- argument specs ------------------------------------------------------------------------

fn arg_type_of(r#type: alg::GdalAlgorithmArgType) -> ArgType {
    match r#type {
        alg::GAAT_BOOLEAN => ArgType::Boolean,
        alg::GAAT_STRING => ArgType::String,
        alg::GAAT_INTEGER => ArgType::Integer,
        alg::GAAT_REAL => ArgType::Real,
        alg::GAAT_STRING_LIST => ArgType::StringList,
        alg::GAAT_INTEGER_LIST => ArgType::IntegerList,
        alg::GAAT_REAL_LIST => ArgType::RealList,
        alg::GAAT_DATASET => ArgType::Dataset,
        alg::GAAT_DATASET_LIST => ArgType::DatasetList,
        _ => ArgType::String,
    }
}

fn bound_of((value, inclusive): (f64, bool)) -> Option<Bound> {
    // NaN is GDAL's "no bound"; the inclusive flag says nothing about that.
    if value.is_nan() {
        return None;
    }
    Some(Bound { value, inclusive })
}

fn default_of(arg: &Arg<'_>) -> Option<Scalar> {
    if !arg.has_default_value() {
        return None;
    }
    match arg.r#type() {
        alg::GAAT_BOOLEAN => Some(Scalar::Boolean(arg.default_boolean())),
        alg::GAAT_STRING => Some(Scalar::Text(arg.default_string())),
        alg::GAAT_INTEGER => Some(Scalar::Integer(arg.default_integer())),
        alg::GAAT_REAL => Some(Scalar::Real(arg.default_real())),
        alg::GAAT_STRING_LIST => Some(Scalar::TextList(arg.default_string_list())),
        alg::GAAT_INTEGER_LIST => Some(Scalar::IntegerList(arg.default_integer_list())),
        alg::GAAT_REAL_LIST => Some(Scalar::RealList(arg.default_real_list())),
        _ => None,
    }
}

fn spec_of(arg: &Arg<'_>) -> ArgSpec {
    let mut dataset_kinds = 0;
    let mut dataset_update = false;
    let mut accepts_name = false;
    let mut accepts_object = false;
    if arg.is_dataset() {
        let kinds = arg.dataset_type() as u32;
        let bit = |flag: u32, kind: DatasetKind| if kinds & flag != 0 { kind as u32 } else { 0 };
        dataset_kinds = bit(gdal::GDAL_OF_RASTER, DatasetKind::Raster)
            | bit(gdal::GDAL_OF_VECTOR, DatasetKind::Vector)
            | bit(alg::GDAL_OF_MULTIDIM_RASTER, DatasetKind::Multidim);
        dataset_update = kinds & gdal::GDAL_OF_UPDATE != 0;
        let flags = arg.dataset_input_flags();
        accepts_name = flags & alg::GADV_NAME != 0;
        accepts_object = flags & alg::GADV_OBJECT != 0;
    }
    let max_count = arg.max_count();
    ArgSpec {
        name: arg.name(),
        short_name: arg.short_name(),
        description: arg.description(),
        meta_var: arg.meta_var(),
        category: arg.category(),
        aliases: arg.aliases(),
        r#type: arg_type_of(arg.r#type()),
        required: arg.is_required(),
        positional: arg.is_positional(),
        is_input: arg.is_input(),
        is_output: arg.is_output(),
        min_count: arg.min_count(),
        max_count: if max_count == alg::UNBOUNDED {
            -1
        } else {
            max_count
        },
        packed_values: arg.packed_values_allowed(),
        repeatable: arg.repeated_arg_allowed(),
        in_pipeline_step: arg.is_available_in_pipeline_step(),
        default_value: default_of(arg),
        choices: arg.choices(),
        min: bound_of(arg.min_value()),
        max: bound_of(arg.max_value()),
        dataset_kinds,
        dataset_update,
        accepts_name,
        accepts_object,
        exclusion_group: arg.mutual_exclusion_group(),
        dependency_group: arg.mutual_dependency_group(),
        depends_on: arg.direct_dependencies(),
        metadata: arg.metadata(),
    }
}

// ---- the tail: which arguments its words give values to ------------------------------------------

/// What `std::istringstream >> word` reads, one word after another: the runs
/// of characters between white space of the C locale.
fn stream_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_ascii() && is_ascii_space(c as u8))
        .filter(|word| !word.is_empty())
}

/// The name of the last step of a pipeline and whether it has words after
/// it: "... ! write out.tif" names the output, "... ! write" leaves it to the
/// run. An `update` step names the dataset it writes into as its output, as
/// write does: bound a second one by the run, GDAL refused every such
/// pipeline with "update: Positional values starting at 'b.tif' are not
/// expected" (measured).
fn pipeline_names_output(pipeline: &str) -> bool {
    let last = pipeline
        .rfind('!')
        .map_or(pipeline, |bang| &pipeline[bang + 1..]);
    let mut words = stream_words(last);
    let step = words.next().unwrap_or_default().to_ascii_lowercase();
    if step != "write" && step != "update" {
        return false;
    }
    words.any(|argument| !argument.starts_with('-'))
}

/// Whether a pipeline's last step prints rather than writes a dataset: the
/// steps GDAL 3.13 declares with an output-string (its pipeline usage's
/// pipeline_algorithms: compare, export-schema, info). Any other last step -
/// write, or a processing step whose result is the pipeline's output - makes
/// a dataset. A pipeline has an output-string for those three, which is no
/// reason to leave its output unbound when it ends in write (measured: every
/// pipeline to a layer or a reference raster failed with "write: Positional
/// arguments starting at 'OUTPUT' have not been specified").
fn pipeline_prints(pipeline: &str) -> bool {
    let last = pipeline
        .rfind('!')
        .map_or(pipeline, |bang| &pipeline[bang + 1..]);
    let step = stream_words(last)
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    step == "info" || step == "compare" || step == "export-schema"
}

fn pipeline_names_input(pipeline: &str) -> bool {
    let first = pipeline
        .find('!')
        .map_or(pipeline, |bang| &pipeline[..bang]);
    let mut words = stream_words(first);
    if !words
        .next()
        .unwrap_or_default()
        .eq_ignore_ascii_case("read")
    {
        return false;
    }
    words.any(|argument| !argument.starts_with('-'))
}

#[derive(Default)]
struct TailReading {
    /// Long names.
    given: BTreeSet<String>,
    positional: Vec<String>,
}

/// Options by name (--name, --name=value, -n, and aliases, as GDAL's GetArg
/// resolves them); a word after an option that is not a flag is its value;
/// everything else is positional, assigned as GDAL assigns it: in declaration
/// order, skipping what is already set, each list keeping back what the
/// required ones after it need.
fn read_tail(
    algorithm: &Algorithm,
    tokens: &[String],
    bound: &BTreeSet<String>,
    pipeline: bool,
) -> TailReading {
    let mut reading = TailReading::default();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        let bytes = token.as_bytes();
        let long_option = bytes.len() > 2 && token.starts_with("--");
        let short_option = bytes.len() == 2 && bytes[0] == b'-' && bytes[1].is_ascii_alphabetic();
        if !long_option && !short_option {
            reading.positional.push(token.clone());
            i += 1;
            continue;
        }
        let equals = token.find('=');
        // Both cuts are at ASCII characters: "--" or "-" and '='.
        let name = if long_option {
            &token[2..equals.unwrap_or(token.len())]
        } else {
            &token[1..]
        };
        // GDAL refuses an unknown one by name when it parses the tail.
        if let Some(arg) = algorithm.arg(name) {
            reading.given.insert(arg.name());
            if equals.is_none() && arg.r#type() != alg::GAAT_BOOLEAN {
                i += 1;
            }
        }
        i += 1;
    }
    if pipeline {
        // One positional: the pipeline text, which may name its input and
        // output in its read and write steps.
        let mut text = String::new();
        for word in &reading.positional {
            text.push_str(word);
            text.push(' ');
        }
        if !reading.positional.is_empty() {
            reading.given.insert(String::from("pipeline"));
        }
        if pipeline_names_input(&text) {
            reading.given.insert(String::from("input"));
        }
        if pipeline_names_output(&text) {
            reading.given.insert(String::from("output"));
        }
        return reading;
    }
    let open: Vec<Arg<'_>> = algorithm
        .args()
        .into_iter()
        .filter(|arg| {
            let name = arg.name();
            arg.is_positional() && !bound.contains(&name) && !reading.given.contains(&name)
        })
        .collect();
    let mut left = reading.positional.len();
    for (k, arg) in open.iter().enumerate() {
        if left == 0 {
            break;
        }
        let reserve: usize = open[k + 1..]
            .iter()
            .filter(|after| after.is_required())
            .map(|after| after.min_count().max(1) as usize)
            .sum();
        let list = arg.max_count() > 1;
        let most = if list { arg.max_count() as usize } else { 1 };
        let mut take = left.saturating_sub(reserve).min(most);
        if take == 0 && arg.is_required() {
            take = 1;
        }
        if take > 0 {
            reading.given.insert(arg.name());
            left -= take.min(left);
        }
    }
    reading
}

// ---- datasets in memory ----------------------------------------------------------------------

fn mem_driver() -> gdal::GdalDriverH {
    driver_by_name("MEM")
}

fn mem_raster(grid: &RasterGrid) -> Result<DatasetRef> {
    let info = &grid.info;
    let bands = grid.bands.len() as c_int;
    if info.width <= 0 || info.height <= 0 || bands <= 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a grid needs a size and at least one band",
        ));
    }
    let cells = info.width as usize * info.height as usize;
    if grid.bands.iter().any(|band| band.len() != cells) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a band of the grid does not hold width x height values",
        ));
    }
    let type_name = cpl::c_string(&grid.data_type);
    // SAFETY: a NUL-terminated name.
    let data_type = unsafe { alg::GDALGetDataTypeByName(type_name.as_ptr()) };
    if data_type == gdal::GDT_UNKNOWN {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "GDAL has no data type of that name",
            grid.data_type.clone(),
        ));
    }
    // SAFETY: the MEM driver (or NULL, which GDAL refuses), an empty name and
    // no options; the dataset returned is the DatasetRef's.
    let made = unsafe {
        gdal::GDALCreate(
            mem_driver(),
            c"".as_ptr(),
            info.width,
            info.height,
            bands,
            data_type,
            ptr::null(),
        )
    };
    let Some(dataset) = DatasetRef::from_raw(made) else {
        return Err(Error::new(
            ErrorCode::Internal,
            "GDAL could not make a grid in memory",
        ));
    };
    let transform = info.geotransform;
    // SAFETY: a live dataset and six doubles.
    unsafe { gdal::GDALSetGeoTransform(dataset.raw(), transform.as_ptr()) };
    if !info.projection_wkt.is_empty() {
        let mut crs = SpatialReference::new();
        if parse_crs(&info.projection_wkt, &mut crs) {
            // SAFETY: a live dataset and a live reference, which GDAL copies.
            unsafe { gdal::GDALSetSpatialRef(dataset.raw(), crs.raw()) };
        }
    }
    for (b, values) in (0..bands).zip(&grid.bands) {
        // SAFETY: a live dataset and one of its bands.
        let band = unsafe { gdal::GDALGetRasterBand(dataset.raw(), b + 1) };
        // RasterIO takes one non-const buffer for reading and writing alike;
        // GF_Write only reads it, and copying a band to satisfy the type
        // would double the memory of the largest thing a run holds.
        // SAFETY: a live band and width x height doubles, which GF_Write
        // reads and never writes.
        let status = unsafe {
            gdal::GDALRasterIO(
                band,
                gdal::GF_WRITE,
                0,
                0,
                info.width,
                info.height,
                values.as_ptr().cast_mut().cast(),
                info.width,
                info.height,
                gdal::GDT_FLOAT64,
                0,
                0,
            )
        };
        if status != cpl::CE_NONE {
            return Err(Error::new(
                ErrorCode::Internal,
                "GDAL could not fill a grid in memory",
            ));
        }
        let index = b as usize;
        let no_data = if index < grid.no_data.len() {
            grid.no_data[index]
        } else if b == 0 {
            info.no_data_value
        } else {
            None
        };
        if let Some(value) = no_data {
            // SAFETY: a live band.
            unsafe { gdal::GDALSetRasterNoDataValue(band, value) };
        }
    }
    Ok(dataset)
}

fn ogr_field_type(r#type: FieldType) -> (ogr::OgrFieldType, ogr::OgrFieldSubType) {
    match r#type {
        FieldType::Boolean => (ogr::OFT_INTEGER, ogr::OFST_BOOLEAN),
        FieldType::Integer64 => (ogr::OFT_INTEGER64, ogr::OFST_NONE),
        FieldType::Real => (ogr::OFT_REAL, ogr::OFST_NONE),
        FieldType::String => (ogr::OFT_STRING, ogr::OFST_NONE),
        FieldType::Date => (ogr::OFT_DATE, ogr::OFST_NONE),
        FieldType::DateTime => (ogr::OFT_DATE_TIME, ogr::OFST_NONE),
    }
}

fn layer_type(table: &FeatureTable) -> ogr::OgrWkbGeometryType {
    let multi = table.features.iter().any(|feature| feature.parts.len() > 1);
    let geometry_type = match table.kind {
        GeometryKind::Point => {
            if multi {
                ogr::WKB_MULTI_POINT
            } else {
                ogr::WKB_POINT
            }
        }
        GeometryKind::LineString => {
            if multi {
                ogr::WKB_MULTI_LINE_STRING
            } else {
                ogr::WKB_LINE_STRING
            }
        }
        GeometryKind::Polygon => {
            if multi {
                ogr::WKB_MULTI_POLYGON
            } else {
                ogr::WKB_POLYGON
            }
        }
        GeometryKind::Unknown => return ogr::WKB_UNKNOWN,
    };
    if table.has_z {
        // SAFETY: a pure function of an integer (wkbSetZ).
        unsafe { ogr::OGR_GT_SetZ(geometry_type) }
    } else {
        geometry_type
    }
}

fn feature_geometry(feature: &Feature, kind: GeometryKind) -> Option<OgrGeometry> {
    match feature.parts.as_slice() {
        [] => None,
        [only] => make_ogr_geometry(only),
        parts => {
            let collection = OgrGeometry::create(match kind {
                GeometryKind::Point => ogr::WKB_MULTI_POINT,
                GeometryKind::LineString => ogr::WKB_MULTI_LINE_STRING,
                GeometryKind::Polygon => ogr::WKB_MULTI_POLYGON,
                GeometryKind::Unknown => ogr::WKB_GEOMETRY_COLLECTION,
            });
            for part in parts {
                if let Some(member) = make_ogr_geometry(part) {
                    collection.add_geometry_directly(member);
                }
            }
            Some(collection)
        }
    }
}

fn mem_vector(set: &FeatureSet) -> Result<DatasetRef> {
    // SAFETY: the MEM driver (or NULL, which GDAL refuses), an empty name and
    // no options; the dataset returned is the DatasetRef's.
    let made = unsafe {
        gdal::GDALCreate(
            mem_driver(),
            c"".as_ptr(),
            0,
            0,
            0,
            gdal::GDT_UNKNOWN,
            ptr::null(),
        )
    };
    let Some(dataset) = DatasetRef::from_raw(made) else {
        return Err(Error::new(
            ErrorCode::Internal,
            "GDAL could not make a feature set in memory",
        ));
    };
    let mut unnamed = 0;
    for table in &set.tables {
        let mut crs = SpatialReference::new();
        let has_crs = !table.crs_wkt.is_empty() && parse_crs(&table.crs_wkt, &mut crs);
        let name = if table.name.is_empty() {
            unnamed += 1;
            format!("features{unnamed}")
        } else {
            table.name.clone()
        };
        let layer_name = cpl::c_string(&name);
        // SAFETY: a live dataset, a NUL-terminated name, NULL or a live
        // reference (copied by the layer) and no options; the layer is the
        // dataset's.
        let layer = unsafe {
            gdal::GDALDatasetCreateLayer(
                dataset.raw(),
                layer_name.as_ptr(),
                if has_crs { crs.raw() } else { ptr::null_mut() },
                layer_type(table),
                ptr::null(),
            )
        };
        if layer.is_null() {
            return Err(make_error(
                ErrorCode::Internal,
                "GDAL could not make a layer in memory",
                name,
            ));
        }
        for field in &table.fields {
            let (field_type, sub_type) = ogr_field_type(field.r#type);
            let field_name = cpl::c_string(&field.name);
            // SAFETY: a field definition made, handed to the live layer
            // (which copies it) and destroyed here.
            let created = unsafe {
                let definition = ogr::OGR_Fld_Create(field_name.as_ptr(), field_type);
                ogr::OGR_Fld_SetSubType(definition, sub_type);
                let created = ogr::OGR_L_CreateField(layer, definition, cpl::TRUE);
                ogr::OGR_Fld_Destroy(definition);
                created
            };
            if created != ogr::OGRERR_NONE {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "GDAL refused a field",
                    field.name.clone(),
                ));
            }
        }
        for feature in &table.features {
            // SAFETY: the live layer's definition; the feature is destroyed
            // below.
            let ogr_feature = unsafe { ogr::OGR_F_Create(ogr::OGR_L_GetLayerDefn(layer)) };
            for (index, value) in (0..).zip(feature.values.iter().take(table.fields.len())) {
                // SAFETY: a live feature and one of its fields; text is
                // NUL-terminated and copied.
                unsafe {
                    match value {
                        FieldValue::Null => ogr::OGR_F_SetFieldNull(ogr_feature, index),
                        FieldValue::Boolean(held) => {
                            ogr::OGR_F_SetFieldInteger(ogr_feature, index, c_int::from(*held));
                        }
                        FieldValue::Integer64(held) => {
                            ogr::OGR_F_SetFieldInteger64(ogr_feature, index, *held);
                        }
                        FieldValue::Real(held) => {
                            ogr::OGR_F_SetFieldDouble(ogr_feature, index, *held);
                        }
                        FieldValue::Text(held) => {
                            let held = cpl::c_string(held);
                            ogr::OGR_F_SetFieldString(ogr_feature, index, held.as_ptr());
                        }
                    }
                }
            }
            if let Some(geometry) = feature_geometry(feature, table.kind) {
                if has_crs {
                    // SAFETY: a live geometry and the live layer's reference,
                    // which the geometry references.
                    unsafe {
                        alg::OGR_G_AssignSpatialReference(
                            geometry.raw(),
                            ogr::OGR_L_GetSpatialRef(layer),
                        );
                    }
                }
                // SAFETY: a live feature; the geometry is the feature's from
                // here.
                unsafe { ogr::OGR_F_SetGeometryDirectly(ogr_feature, geometry.into_raw()) };
            }
            // SAFETY: the live layer and feature (which the layer copies);
            // the feature is destroyed once.
            let created = unsafe {
                let created = ogr::OGR_L_CreateFeature(layer, ogr_feature);
                ogr::OGR_F_Destroy(ogr_feature);
                created
            };
            if created != ogr::OGRERR_NONE {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "GDAL refused a feature",
                    name,
                ));
            }
        }
    }
    Ok(dataset)
}

// ---- reading an output back -----------------------------------------------------------------

/// A reference's WKT2 (2019), or empty for none and for a reference GDAL
/// cannot write.
fn wkt_of(reference: osr::OgrSpatialReferenceH) -> String {
    if reference.is_null() {
        return String::new();
    }
    let options = [c"FORMAT=WKT2_2019".as_ptr(), ptr::null()];
    let mut text: *mut c_char = ptr::null_mut();
    // SAFETY: a live reference, a NULL-terminated option list and an out
    // pointer GDAL fills with text the caller frees.
    let status = unsafe { osr::OSRExportToWktEx(reference, &raw mut text, options.as_ptr()) };
    // SAFETY: NULL or the text GDAL just wrote, read before it is freed.
    let wkt = if status == ogr::OGRERR_NONE {
        unsafe { cpl::decoded(text) }
    } else {
        String::new()
    };
    // SAFETY: GDAL's allocation (or NULL), freed once.
    unsafe { cpl::VSIFree(text.cast()) };
    wkt
}

fn read_grid(dataset: gdal::GdalDatasetH) -> Result<RasterGrid> {
    // SAFETY: a live dataset; its spatial reference is its own.
    let (width, height, band_count, reference) = unsafe {
        (
            gdal::GDALGetRasterXSize(dataset),
            gdal::GDALGetRasterYSize(dataset),
            gdal::GDALGetRasterCount(dataset),
            gdal::GDALGetSpatialRef(dataset),
        )
    };
    let mut info = RasterInfo {
        width,
        height,
        band_count,
        projection_wkt: wkt_of(reference),
        ..RasterInfo::default()
    };
    let mut transform = [0.0; 6];
    // SAFETY: a live dataset and six doubles.
    if unsafe { gdal::GDALGetGeoTransform(dataset, transform.as_mut_ptr()) } == cpl::CE_NONE {
        info.geotransform = transform;
        info.has_geotransform = true;
    }
    let cells = width as usize * height as usize;
    let mut data_type = String::from("Float64");
    let mut bands = Vec::new();
    let mut no_data = Vec::new();
    for b in 1..=band_count {
        // SAFETY: a live dataset and one of its bands.
        let band = unsafe { gdal::GDALGetRasterBand(dataset, b) };
        if b == 1 {
            // SAFETY: a live band; the name is a static text GDAL keeps.
            data_type = unsafe {
                cpl::decoded(gdal::GDALGetDataTypeName(gdal::GDALGetRasterDataType(band)))
            };
        }
        let mut values = vec![0.0; cells];
        // SAFETY: a live band and a buffer of width x height doubles.
        let status = unsafe {
            gdal::GDALRasterIO(
                band,
                gdal::GF_READ,
                0,
                0,
                width,
                height,
                values.as_mut_ptr().cast(),
                width,
                height,
                gdal::GDT_FLOAT64,
                0,
                0,
            )
        };
        if status != cpl::CE_NONE {
            return Err(Error::new(
                ErrorCode::Internal,
                "GDAL could not read an output band back",
            ));
        }
        bands.push(values);
        let mut has: c_int = 0;
        // SAFETY: a live band and an out flag.
        let value = unsafe { gdal::GDALGetRasterNoDataValue(band, &raw mut has) };
        no_data.push((has != 0).then_some(value));
    }
    info.no_data_value = no_data.first().copied().flatten();
    Ok(RasterGrid {
        info,
        data_type,
        bands,
        no_data,
    })
}

fn field_type_of(field: ogr::OgrFieldDefnH) -> FieldType {
    // SAFETY: a live field definition of a live layer.
    let (field_type, sub_type) =
        unsafe { (ogr::OGR_Fld_GetType(field), ogr::OGR_Fld_GetSubType(field)) };
    match field_type {
        ogr::OFT_INTEGER => {
            if sub_type == ogr::OFST_BOOLEAN {
                FieldType::Boolean
            } else {
                FieldType::Integer64
            }
        }
        ogr::OFT_INTEGER64 => FieldType::Integer64,
        ogr::OFT_REAL => FieldType::Real,
        ogr::OFT_DATE => FieldType::Date,
        ogr::OFT_DATE_TIME => FieldType::DateTime,
        _ => FieldType::String,
    }
}

fn kind_of(geometry_type: ogr::OgrWkbGeometryType) -> GeometryKind {
    match flattened(geometry_type) {
        ogr::WKB_POINT | ogr::WKB_MULTI_POINT => GeometryKind::Point,
        ogr::WKB_LINE_STRING | ogr::WKB_MULTI_LINE_STRING => GeometryKind::LineString,
        ogr::WKB_POLYGON | ogr::WKB_MULTI_POLYGON => GeometryKind::Polygon,
        _ => GeometryKind::Unknown,
    }
}

fn field_as_string(feature: ogr::OgrFeatureH, index: c_int) -> String {
    // SAFETY: a live feature and one of its fields; the text is copied at once.
    unsafe { cpl::decoded(ogr::OGR_F_GetFieldAsString(feature, index)) }
}

/// An ISO 8601 date or date-time, as FieldType::Date and DateTime carry them.
fn iso_date(feature: ogr::OgrFeatureH, index: c_int, with_time: bool) -> String {
    let (mut year, mut month, mut day, mut hour, mut minute, mut zone) = (0, 0, 0, 0, 0, 0);
    let mut second: f32 = 0.0;
    // SAFETY: a live feature, one of its fields, and seven out values.
    let read = unsafe {
        ogr::OGR_F_GetFieldAsDateTimeEx(
            feature,
            index,
            &raw mut year,
            &raw mut month,
            &raw mut day,
            &raw mut hour,
            &raw mut minute,
            &raw mut second,
            &raw mut zone,
        )
    };
    if read == cpl::FALSE {
        return field_as_string(feature, index);
    }
    // Whole numbers only: printf's %f would follow the C locale's decimal point.
    let two = |value: c_int| format!("{}{value}", if value < 10 { "0" } else { "" });
    let mut text = format!("{year}-{}-{}", two(month), two(day));
    if with_time {
        // A float's truncation and rounding, as the C++'s casts and lround
        // of a float (second is 0 to 61).
        let whole = second as c_int;
        let millis = ((second - whole as f32) * 1000.0_f32).round() as c_int;
        text.push_str(&format!("T{}:{}:{}", two(hour), two(minute), two(whole)));
        if millis > 0 {
            let pad = if millis < 100 {
                if millis < 10 { "00" } else { "0" }
            } else {
                ""
            };
            text.push_str(&format!(".{pad}{millis}"));
        }
    }
    text
}

fn read_features(dataset: gdal::GdalDatasetH, diagnostics: &mut Vec<Diagnostic>) -> FeatureSet {
    let mut set = FeatureSet::default();
    let mut warnings = Vec::new();
    // SAFETY: a live dataset.
    let layer_count = unsafe { gdal::GDALDatasetGetLayerCount(dataset) };
    for l in 0..layer_count {
        // SAFETY: a live dataset and one of its layers.
        let layer = unsafe { gdal::GDALDatasetGetLayer(dataset, l) };
        if layer.is_null() {
            continue;
        }
        // SAFETY: a live layer; its name, type, reference and definition are
        // its own.
        let (name, geometry_type, reference, definition) = unsafe {
            (
                cpl::decoded(ogr::OGR_L_GetName(layer)),
                ogr::OGR_L_GetGeomType(layer),
                ogr::OGR_L_GetSpatialRef(layer),
                ogr::OGR_L_GetLayerDefn(layer),
            )
        };
        let mut table = FeatureTable {
            name,
            kind: kind_of(geometry_type),
            // SAFETY: a pure function of an integer (wkbHasZ).
            has_z: unsafe { ogr::OGR_GT_HasZ(geometry_type) } != 0,
            crs_wkt: wkt_of(reference),
            ..FeatureTable::default()
        };
        // SAFETY: a live definition.
        let field_count = unsafe { ogr::OGR_FD_GetFieldCount(definition) };
        for f in 0..field_count {
            // SAFETY: a live definition and one of its fields.
            let field = unsafe { ogr::OGR_FD_GetFieldDefn(definition, f) };
            table.fields.push(FieldDef {
                // SAFETY: a live field definition's own name, copied at once.
                name: unsafe { cpl::decoded(ogr::OGR_Fld_GetNameRef(field)) },
                r#type: field_type_of(field),
            });
        }
        // SAFETY: a live layer.
        unsafe { ogr::OGR_L_ResetReading(layer) };
        loop {
            // SAFETY: a live layer; the feature returned is the caller's to
            // destroy.
            let ogr_feature = unsafe { ogr::OGR_L_GetNextFeature(layer) };
            if ogr_feature.is_null() {
                break;
            }
            let mut feature = Feature::default();
            // SAFETY: the feature's geometry, owned by the live feature, which
            // is destroyed only after the last use of the view.
            let geometry =
                unsafe { GeometryView::from_raw(ogr::OGR_F_GetGeometryRef(ogr_feature)) };
            flatten_ogr_geometry(geometry, &mut feature.parts, &mut warnings);
            if table.kind == GeometryKind::Unknown
                && let Some(first) = feature.parts.first()
            {
                table.kind = first.kind;
            }
            for part in &feature.parts {
                table.has_z = table.has_z || part.has_z;
            }
            for (f, field) in (0..field_count).zip(&table.fields) {
                // SAFETY: a live feature and one of its fields.
                if unsafe { ogr::OGR_F_IsFieldSetAndNotNull(ogr_feature, f) } == 0 {
                    feature.values.push(FieldValue::Null);
                    continue;
                }
                let value = match field.r#type {
                    // SAFETY (the three reads): a live feature and one of its
                    // fields.
                    FieldType::Boolean => FieldValue::Boolean(
                        unsafe { ogr::OGR_F_GetFieldAsInteger(ogr_feature, f) } != 0,
                    ),
                    FieldType::Integer64 => FieldValue::Integer64(unsafe {
                        ogr::OGR_F_GetFieldAsInteger64(ogr_feature, f)
                    }),
                    FieldType::Real => {
                        FieldValue::Real(unsafe { ogr::OGR_F_GetFieldAsDouble(ogr_feature, f) })
                    }
                    FieldType::Date => FieldValue::Text(iso_date(ogr_feature, f, false)),
                    FieldType::DateTime => FieldValue::Text(iso_date(ogr_feature, f, true)),
                    FieldType::String => FieldValue::Text(field_as_string(ogr_feature, f)),
                };
                feature.values.push(value);
            }
            table.features.push(feature);
            // SAFETY: the feature GetNextFeature handed over, destroyed once.
            unsafe { ogr::OGR_F_Destroy(ogr_feature) };
        }
        set.tables.push(table);
    }
    for warning in warnings {
        diagnostics.push(Diagnostic {
            failure: false,
            code: 0,
            message: warning,
        });
    }
    set
}

/// A colour map's picture made in memory says what its bands are. GDAL
/// 3.13's raster color-map leaves every band's colour interpretation
/// Undefined when it writes to MEM (a GTiff output gets red, green, blue and
/// alpha from the driver's own defaults, which is why the command line looks
/// right). Copied on so, the picture reads as three undesignated colour bands
/// and a fourth no reader may take for alpha - it could as well be near
/// infrared - and every cell the map made clear is drawn opaque. Its bands
/// are, by the algorithm's definition, red, green, blue and, with
/// --add-alpha, alpha.
fn stamp_colour_map_bands(path: &[String], dataset: gdal::GdalDatasetH) {
    if path != ["raster", "color-map"] {
        return;
    }
    // SAFETY: a live dataset.
    let count = unsafe { gdal::GDALGetRasterCount(dataset) };
    if count != 3 && count != 4 {
        return;
    }
    const ROLES: [gdal::GdalColorInterp; 4] = [
        gdal::GCI_RED_BAND,
        gdal::GCI_GREEN_BAND,
        gdal::GCI_BLUE_BAND,
        gdal::GCI_ALPHA_BAND,
    ];
    for i in 1..=count {
        // SAFETY: a live dataset and one of its bands.
        let interpretation =
            unsafe { gdal::GDALGetRasterColorInterpretation(gdal::GDALGetRasterBand(dataset, i)) };
        if interpretation != alg::GCI_UNDEFINED {
            return; // said already: a later GDAL that sets them is taken at its word
        }
    }
    for (i, role) in (1..=count).zip(ROLES) {
        // SAFETY: a live dataset and one of its bands.
        unsafe {
            alg::GDALSetRasterColorInterpretation(gdal::GDALGetRasterBand(dataset, i), role);
        }
    }
}

/// The rasters spilled, for each a file of its own.
static SPILLED: AtomicU64 = AtomicU64::new(0);

/// A raster too big for memory, as the tiled GeoTIFF a derived reference
/// raster is kept in: DEFLATE, with the floating-point predictor for
/// floating-point data (docs/geoprocessing.md, "Outputs").
fn spill(dataset: gdal::GdalDatasetH, directory: &str) -> Result<String> {
    // std::filesystem::temp_directory_path() where the C++ has it; Rust's
    // reads TMPDIR alone on POSIX, where the C++ library reads TMPDIR, TMP,
    // TEMP and TEMPDIR (docs/rust.md, "Where Rust differs").
    let folder = if directory.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(directory)
    };
    let _ = std::fs::create_dir_all(&folder);
    let number = SPILLED.fetch_add(1, Ordering::Relaxed) + 1;
    // SAFETY: no argument; the calling thread's id, whatever the name says.
    let process = unsafe { alg::CPLGetPID() };
    let file = folder.join(format!("katana-{process}-{number}.tif"));
    // SAFETY: a live dataset and its first band.
    let data_type = unsafe { gdal::GDALGetRasterDataType(gdal::GDALGetRasterBand(dataset, 1)) };
    // SAFETY: a pure function of a data type.
    let floating = unsafe { alg::GDALDataTypeIsFloating(data_type) } != 0;
    let mut options = cpl::CStringList::new();
    options.set_name_value(b"TILED", b"YES");
    options.set_name_value(b"COMPRESS", b"DEFLATE");
    if floating {
        options.set_name_value(b"PREDICTOR", b"3");
    }
    options.set_name_value(b"BIGTIFF", b"IF_SAFER");
    let gtiff = driver_by_name("GTiff");
    let name = String::from_utf8_lossy(&cpl::generic_path(&file)).into_owned();
    let target = cpl::c_string(&name);
    // SAFETY: the GTiff driver (or NULL, which GDAL refuses), a NUL-terminated
    // name, a live source, a NULL-terminated option list and no progress; the
    // copy returned is closed below.
    let copy = unsafe {
        gdal::GDALCreateCopy(
            gtiff,
            target.as_ptr(),
            dataset,
            cpl::FALSE,
            options.as_ptr(),
            None,
            ptr::null_mut(),
        )
    };
    if copy.is_null() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "could not write the raster to a file",
            name,
        ));
    }
    // SAFETY: the copy made above, closed once.
    unsafe { gdal::GDALClose(copy) };
    Ok(name)
}

// ---- binding a request --------------------------------------------------------------------

/// Declared in the order that makes destruction safe - Rust drops fields
/// first to last, where the C++ destroys members last to first: the
/// algorithm goes first, closing what it opened, then our references to the
/// inputs in memory, and last the staged copies it may have held open.
#[derive(Default)]
struct Binding {
    algorithm: Option<Algorithm>,
    /// The algorithm has a dataset output, its argument "output".
    has_output: bool,
    memory_output: bool,
    /// The file written, when it is one.
    output_file: String,
    output_existed: bool,
    /// Our references to the MEM inputs.
    owned: Vec<DatasetRef>,
    staging: Staging,
}

impl Binding {
    /// The algorithm `bind` made; only called once it has.
    fn algorithm(&self) -> &Algorithm {
        self.algorithm
            .as_ref()
            .expect("a request is bound to its algorithm before it is read")
    }
}

fn suggestion(algorithm: &Algorithm, name: &str) -> String {
    let near = algorithm.suggestion_for_argument_name(name);
    if near.is_empty() {
        String::new()
    } else {
        format!("; did you mean {near}?")
    }
}

/// A dataset as an argument takes it: by name, or an open dataset.
enum Held {
    Name(String),
    Object(gdal::GdalDatasetH),
}

impl Held {
    fn item(&self) -> DatasetItem<'_> {
        match self {
            Held::Name(name) => DatasetItem::Name(name.as_str()),
            Held::Object(dataset) => DatasetItem::Object(*dataset),
        }
    }
}

fn dataset_value(
    arg: &Arg<'_>,
    value: &DatasetValue,
    owned: &mut Vec<DatasetRef>,
    staging: &mut Staging,
) -> Result<Held> {
    let (dataset, raster) = match value {
        DatasetValue::Path(path) => return Ok(Held::Name(path.path.clone())),
        DatasetValue::Raster(grid) => (mem_raster(grid)?, true),
        DatasetValue::Features(set) => (mem_vector(set)?, false),
    };
    if arg.dataset_input_flags() & alg::GADV_OBJECT != 0 {
        let handle = dataset.raw();
        owned.push(dataset);
        return Ok(Held::Object(handle));
    }
    Ok(Held::Name(staging.stage(dataset.raw(), raster)?))
}

/// The argument that names the layer of the dataset `dataset` gives: GDAL's
/// <name>-layer (vector clip's --like-layer, layer-algebra's --method-layer,
/// zonal-stats' --zones-layer), else --input-layer for the input - or for
/// the one vector dataset, as raster pixel-info's --input-layer is its
/// position-dataset's. Setting --input-layer for every dataset gave vector
/// clip's input the layer FROM like FILE ... LAYER named (review finding;
/// "Cannot find source layer").
fn layer_arg_of<'a>(algorithm: &'a Algorithm, dataset: &Arg<'_>) -> Option<Arg<'a>> {
    let dataset_name = dataset.name();
    if let Some(own) = algorithm.arg(&format!("{dataset_name}-layer")) {
        return Some(own);
    }
    let input = algorithm.arg("input-layer");
    if input.is_none() || dataset_name == "input" {
        return input;
    }
    let vector = |arg: &Arg<'_>| arg.dataset_type() as u32 & gdal::GDAL_OF_VECTOR != 0;
    let vectors = algorithm
        .args()
        .iter()
        .filter(|other| other.is_dataset() && !other.is_output() && vector(other))
        .count();
    if vectors == 1 && vector(dataset) {
        input
    } else {
        None
    }
}

fn bind_value(
    algorithm: &Algorithm,
    name: &str,
    value: &ArgValue,
    owned: &mut Vec<DatasetRef>,
    staging: &mut Staging,
    errors: &ErrorCollector,
) -> Status {
    let Some(arg) = algorithm.arg(name).filter(|arg| !arg.is_hidden_for_api()) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "no argument of {} is called {name}{}",
                algorithm.name(),
                suggestion(algorithm, name)
            ),
            name,
        ));
    };
    let mark = errors.size();
    let refused = || -> Status {
        Err(gdal_error(
            errors,
            ErrorCode::InvalidArgument,
            format!("GDAL refused the value of {}", arg.name()),
            mark,
        ))
    };
    let values: Vec<&DatasetValue> = match value {
        ArgValue::Scalar(scalar) => {
            if arg.is_dataset() {
                // A dataset by name, as the command line gives one.
                let names: Vec<String> = match scalar {
                    Scalar::Text(one) => vec![one.clone()],
                    Scalar::TextList(many) => many.clone(),
                    _ => {
                        return Err(Error::new(
                            ErrorCode::InvalidArgument,
                            format!("{} is a dataset: give a name or a source", arg.name()),
                        ));
                    }
                };
                if arg.r#type() == alg::GAAT_DATASET {
                    return if names.len() == 1 && arg.set_dataset_name(&names[0]) {
                        Ok(())
                    } else {
                        refused()
                    };
                }
                let items: Vec<DatasetItem<'_>> = names
                    .iter()
                    .map(|name| DatasetItem::Name(name.as_str()))
                    .collect();
                return if arg.set_dataset_list(&items) {
                    Ok(())
                } else {
                    refused()
                };
            }
            let set = match scalar {
                Scalar::Boolean(held) => arg.set_boolean(*held),
                Scalar::Text(held) => arg.set_string(held),
                Scalar::Integer(held) => arg.set_integer(*held),
                Scalar::Real(held) => arg.set_real(*held),
                Scalar::TextList(held) => arg.set_string_list(held),
                Scalar::IntegerList(held) => arg.set_integer_list(held),
                Scalar::RealList(held) => arg.set_real_list(held),
            };
            return if set { Ok(()) } else { refused() };
        }
        ArgValue::Dataset(one) => vec![one],
        ArgValue::Datasets(many) => many.iter().collect(),
    };
    if !arg.is_dataset() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{} is not a dataset; it takes a value", arg.name()),
            arg.name(),
        ));
    }
    if values.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{} was given no dataset", arg.name()),
        ));
    }
    for each in &values {
        let DatasetValue::Path(path) = each else {
            continue;
        };
        if !path.open_options.is_empty() {
            let set = algorithm
                .arg("open-option")
                .is_some_and(|options| options.set_string_list(&path.open_options));
            if !set {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{} takes no open options", algorithm.name()),
                ));
            }
        }
        if !path.layer.is_empty() {
            let set = layer_arg_of(algorithm, &arg).is_some_and(|layer| {
                if layer.r#type() == alg::GAAT_STRING {
                    layer.set_string(&path.layer)
                } else {
                    layer.set_string_list(std::slice::from_ref(&path.layer))
                }
            });
            if !set {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{} takes no layer for {}", algorithm.name(), arg.name()),
                    arg.name(),
                ));
            }
        }
    }
    if arg.r#type() == alg::GAAT_DATASET {
        if values.len() != 1 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{} takes one dataset, not {}", arg.name(), values.len()),
            ));
        }
        let set = match dataset_value(&arg, values[0], owned, staging)? {
            Held::Object(dataset) => arg.set_dataset(dataset),
            Held::Name(name) => arg.set_dataset_name(&name),
        };
        return if set { Ok(()) } else { refused() };
    }
    let mut held = Vec::with_capacity(values.len());
    for each in &values {
        held.push(dataset_value(&arg, each, owned, staging)?);
    }
    let items: Vec<DatasetItem<'_>> = held.iter().map(Held::item).collect();
    // Set with a moved list (the shim's): writing into the argument's own
    // list does not mark it set.
    if arg.set_dataset_list(&items) {
        Ok(())
    } else {
        refused()
    }
}

/// A GDAL failure where an input is opened is FileImportFailure; where the
/// output is written, FileExportFailure; anything else as `otherwise`.
fn classified(
    errors: &ErrorCollector,
    output_file: &str,
    otherwise: ErrorCode,
    fallback: &str,
    from: usize,
) -> Error {
    let Some(failure) = errors.first_failure(from) else {
        return Error::new(otherwise, fallback);
    };
    let mentions_output = !output_file.is_empty() && failure.message.contains(output_file);
    if failure.code == alg::CPLE_OPEN_FAILED || failure.code == alg::CPLE_FILE_IO {
        let code = if mentions_output {
            ErrorCode::FileExportFailure
        } else {
            ErrorCode::FileImportFailure
        };
        return Error::new(code, failure.message);
    }
    Error::new(otherwise, failure.message)
}

/// The files a pipeline's steps read, named in its text rather than bound to
/// a dataset argument: every word that is not an option, outside the steps
/// that write (a sidecar beside what a run writes is part of what it wrote).
/// A word that names no plain file is passed over by the guard.
fn watch_pipeline_inputs(pipeline: &str, sidecars: &mut SidecarGuard) {
    let mut step = String::new();
    for word in run_detail::pipeline_words(pipeline) {
        if word.step {
            step = word.text.to_ascii_lowercase();
            continue;
        }
        let writes = step == "write"
            || step == "update"
            || step == "materialize"
            || step == "tile"
            || step == "partition";
        if writes {
            continue;
        }
        let name = if word.text.starts_with('-') {
            word.text
                .find('=')
                .map_or("", |equals| &word.text[equals + 1..])
        } else {
            word.text.as_str()
        };
        if !name.is_empty() {
            sidecars.watch(name);
        }
    }
}

fn watch_inputs(algorithm: &Algorithm, sidecars: &mut SidecarGuard) {
    for arg in algorithm.args() {
        if !arg.is_dataset()
            || arg.is_output()
            || !arg.is_explicitly_set()
            || (arg.dataset_type() as u32 & gdal::GDAL_OF_UPDATE) != 0
        {
            continue;
        }
        if arg.r#type() == alg::GAAT_DATASET {
            sidecars.watch(&arg.dataset_name());
        } else {
            for each in arg.dataset_list_names() {
                sidecars.watch(&each);
            }
        }
    }
}

fn bind(
    request: &RunRequest,
    bound: &mut Binding,
    errors: &ErrorCollector,
    sidecars: &mut SidecarGuard,
) -> Status {
    let made = instantiate(&request.path)?;
    if made.has_sub_algorithms() {
        let path = path_text(&request.path);
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{path} is a group: {}", children_of(&request.path)),
            path,
        ));
    }
    let algorithm: &Algorithm = bound.algorithm.insert(made);

    let mut bound_names = BTreeSet::new();
    for (name, value) in &request.values {
        if name == "pipeline"
            && let ArgValue::Scalar(Scalar::Text(text)) = value
            && run_detail::has_external_step(text)
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a pipeline step 'external' runs a program and is refused",
            ));
        }
        bind_value(
            algorithm,
            name,
            value,
            &mut bound.owned,
            &mut bound.staging,
            errors,
        )?;
        if let Some(arg) = algorithm.arg(name) {
            bound_names.insert(arg.name());
        }
    }

    let mut tokens = request.tokens.clone();
    let pipeline = run_detail::is_pipeline(&request.path);
    let tail = read_tail(algorithm, &tokens, &bound_names, pipeline);
    if pipeline
        && !bound_names.contains("pipeline")
        && tail.positional.len() == 1
        && tail.positional[0].contains([' ', '!'])
    {
        // The pipeline as one quoted word: set as the argument it is, since
        // GDAL's parser reads a pipeline from separate words.
        let text = tail.positional[0].clone();
        if !algorithm
            .arg("pipeline")
            .is_some_and(|steps| steps.set_string(&text))
        {
            return Err(gdal_error(
                errors,
                ErrorCode::InvalidArgument,
                String::from("GDAL refused the pipeline"),
                0,
            ));
        }
        tokens.retain(|token| *token != text);
    }

    // The output: in memory, or the file asked for, unless the tail names it.
    // raster tile's and vector partition's output is a folder name, no
    // dataset.
    let output = algorithm
        .arg("output")
        .filter(|output| output.r#type() == alg::GAAT_DATASET);
    bound.has_output = output.is_some();
    let tail_names_output = output
        .as_ref()
        .is_some_and(|output| tail.given.contains(&output.name()));
    if let Some(output) = &output
        && !tail_names_output
        && !output.is_explicitly_set()
    {
        // An info-like algorithm whose output is optional prints to
        // output-string by default, and is left to. A pipeline's steps are
        // its argument when it was one quoted word, else the tail's words:
        // "read x ! info" given word by word was bound MEM and refused
        // ("Should be one among 'json', 'text'").
        let steps = if pipeline {
            algorithm.arg("pipeline")
        } else {
            None
        };
        let step_text = match &steps {
            Some(steps) if steps.r#type() == alg::GAAT_STRING && steps.is_explicitly_set() => {
                steps.string_value()
            }
            _ => {
                let mut text = String::new();
                for word in &tail.positional {
                    text.push_str(word);
                    text.push(' ');
                }
                text
            }
        };
        let prints_by_default = !output.is_required()
            && algorithm.arg("output-string").is_some()
            && steps.as_ref().is_none_or(|steps| {
                steps.r#type() != alg::GAAT_STRING || pipeline_prints(&step_text)
            });
        if request.output_to == OutputTo::Memory && !prints_by_default {
            let cannot = || {
                gdal_error(
                    errors,
                    ErrorCode::InvalidArgument,
                    format!("{} cannot write to memory", path_text(&request.path)),
                    0,
                )
            };
            if let Some(format) = algorithm.arg("output-format")
                && !tail.given.contains(&format.name())
                && !format.set_string("MEM")
            {
                return Err(cannot());
            }
            if !output.set_dataset_name("") {
                return Err(cannot());
            }
            bound.memory_output = true;
        } else if request.output_to == OutputTo::File {
            if request.output_path.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a file output needs a path",
                ));
            }
            if !output.set_dataset_name(&request.output_path) {
                return Err(gdal_error(
                    errors,
                    ErrorCode::InvalidArgument,
                    String::from("GDAL refused the output"),
                    0,
                ));
            }
            if !request.output_format.is_empty()
                && !algorithm
                    .arg("output-format")
                    .is_some_and(|format| format.set_string(&request.output_format))
            {
                return Err(gdal_error(
                    errors,
                    ErrorCode::InvalidArgument,
                    format!("GDAL refused the output format {}", request.output_format),
                    0,
                ));
            }
            if !request.creation_options.is_empty()
                && !algorithm
                    .arg("creation-option")
                    .is_some_and(|options| options.set_string_list(&request.creation_options))
            {
                return Err(gdal_error(
                    errors,
                    ErrorCode::InvalidArgument,
                    format!("{} takes no creation options", path_text(&request.path)),
                    0,
                ));
            }
            bound.output_file = request.output_path.clone();
        }
    }
    // Replacing a file the request, or GDAL's own words, name: whoever built
    // the request has asked (OVERWRITE); the words' own --overwrite is theirs.
    if request.overwrite
        && !tail.given.contains("overwrite")
        && let Some(overwrite) = algorithm.arg("overwrite")
        && overwrite.r#type() == alg::GAAT_BOOLEAN
    {
        overwrite.set_boolean(true);
    }

    if !tokens.is_empty() {
        let mark = errors.size();
        if !algorithm.parse_command_line_arguments(&tokens) {
            return Err(classified(
                errors,
                &bound.output_file,
                ErrorCode::InvalidArgument,
                "GDAL could not read the arguments",
                mark,
            ));
        }
    }
    if tail_names_output
        && let Some(output) = &output
        && output.is_explicitly_set()
    {
        bound.output_file = output.dataset_name();
    }
    if !bound.output_file.is_empty() && is_plain_file(&bound.output_file) {
        bound.output_existed = Path::new(&bound.output_file).exists();
    }
    watch_inputs(algorithm, sidecars);
    if pipeline {
        let mut text = String::new();
        for (name, value) in &request.values {
            if name == "pipeline"
                && let ArgValue::Scalar(Scalar::Text(words)) = value
            {
                text.push_str(words);
                text.push(' ');
            }
        }
        for token in &request.tokens {
            text.push_str(token);
            text.push(' ');
        }
        watch_pipeline_inputs(&text, sidecars);
    }
    Ok(())
}

// ---- progress and cancel -------------------------------------------------------------------

/// What the progress adapter reads: shared with GDAL, which may call it from
/// threads of its own (the viewshed's), so every member is safe to share.
struct ProgressState<'a> {
    stop: Option<&'a AtomicBool>,
    progress: Option<&'a (dyn Fn(f64) + Sync)>,
    /// Set when the adapter told GDAL to stop. What makes a run cancelled,
    /// whatever Run() returns: hillshade returns true after a refused
    /// progress call, with a partial output.
    refused: AtomicBool,
    /// A panic of the progress closure, raised again once Run() has
    /// returned: none may unwind into GDAL.
    panic: Mutex<Option<Box<dyn Any + Send>>>,
}

// The adapter's data crosses to GDAL's threads.
const _: fn() = || {
    fn shared<T: Sync>() {}
    shared::<ProgressState<'static>>();
};

extern "C" fn progress_adapter(complete: f64, _message: *const c_char, data: *mut c_void) -> c_int {
    if data.is_null() {
        return cpl::TRUE;
    }
    // SAFETY: the state run() handed GDAL, alive until Run() returns, and
    // safe to share between threads (above).
    let state = unsafe { &*data.cast::<ProgressState<'_>>() };
    let reported = catch_unwind(AssertUnwindSafe(|| {
        if let Some(progress) = state.progress {
            // std::clamp(complete, 0.0, 1.0): neither bound is NaN.
            progress(complete.clamp(0.0, 1.0));
        }
    }));
    if let Err(payload) = reported {
        let mut panic = state.panic.lock().unwrap_or_else(PoisonError::into_inner);
        panic.get_or_insert(payload);
        state.refused.store(true, Ordering::Release);
        return cpl::FALSE;
    }
    if state.stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
        state.refused.store(true, Ordering::Release);
        return cpl::FALSE;
    }
    cpl::TRUE
}

// ---- the public functions ------------------------------------------------------------------------

/// "raster hillshade".
pub fn path_text(path: &[String]) -> String {
    joined(path, " ")
}

/// The C++'s `toString` overloads of processing.hpp, one name whichever value
/// it is given: `to_string(ArgType::Real)`, `to_string(Policy::Safe)`,
/// `to_string(&scalar)`.
pub trait Words {
    fn words(self) -> String;
}

impl Words for ArgType {
    /// "boolean", "string", "integer", "real", "string_list",
    /// "integer_list", "real_list", "dataset", "dataset_list": GDAL's own
    /// words.
    fn words(self) -> String {
        String::from(match self {
            ArgType::Boolean => "boolean",
            ArgType::String => "string",
            ArgType::Integer => "integer",
            ArgType::Real => "real",
            ArgType::StringList => "string_list",
            ArgType::IntegerList => "integer_list",
            ArgType::RealList => "real_list",
            ArgType::Dataset => "dataset",
            ArgType::DatasetList => "dataset_list",
        })
    }
}

impl Words for Policy {
    /// "safe", "confirm".
    fn words(self) -> String {
        String::from(if self == Policy::Safe {
            "safe"
        } else {
            "confirm"
        })
    }
}

impl Words for &Scalar {
    /// A value as GDAL's command line would write it: true, 2, 0.5, a,b,c.
    fn words(self) -> String {
        match self {
            Scalar::Boolean(held) => String::from(if *held { "true" } else { "false" }),
            Scalar::Text(held) => held.clone(),
            Scalar::Integer(held) => format!("{held}"),
            Scalar::Real(held) => format_exact_real(*held),
            Scalar::TextList(held) => joined(held, ","),
            Scalar::IntegerList(held) => {
                let words: Vec<String> = held.iter().map(|each| format!("{each}")).collect();
                joined(&words, ",")
            }
            Scalar::RealList(held) => {
                let words: Vec<String> = held.iter().map(|each| format_exact_real(*each)).collect();
                joined(&words, ",")
            }
        }
    }
}

/// The C++'s `toString` (Words says which).
pub fn to_string<T: Words>(value: T) -> String {
    value.words()
}

/// "raster", "vector", "multidim", joined with ','.
pub fn dataset_kinds_text(kinds: u32) -> String {
    let mut words = Vec::new();
    if kinds & DatasetKind::Raster as u32 != 0 {
        words.push("raster");
    }
    if kinds & DatasetKind::Vector as u32 != 0 {
        words.push("vector");
    }
    if kinds & DatasetKind::Multidim as u32 != 0 {
        words.push("multidim");
    }
    joined(&words, ",")
}

/// Every algorithm and group under the root "gdal", depth first in GDAL's
/// order, walked once and kept.
pub fn catalogue() -> &'static [AlgorithmInfo] {
    &catalogue_data().infos
}

/// The algorithm `words` begins with: the longest run of words that names
/// one, each word matched case-insensitively against a name or an alias,
/// public or hidden ("raster warp" is "raster reproject"). `consumed` is set
/// to how many words it took. NotFound naming the word no algorithm has;
/// InvalidArgument for a group ("raster"), listing what is in it.
pub fn resolve(words: &[String], consumed: &mut usize) -> Result<Vec<String>> {
    *consumed = 0;
    if words.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "name an algorithm; GDAL LIST names them",
        ));
    }
    let data = catalogue_data();
    let mut path: Vec<String> = Vec::new();
    let mut reached = false;
    for (i, raw) in words.iter().enumerate() {
        let word = raw.to_ascii_lowercase();
        let named = |name: &String| name.to_ascii_lowercase() == word;
        let next = data
            .infos
            .iter()
            .zip(&data.hidden_aliases)
            .find(|(info, hidden)| {
                info.path.len() == path.len() + 1
                    && info.path.starts_with(&path)
                    && (info.path.last().is_some_and(named)
                        || info.aliases.iter().any(named)
                        || hidden.iter().any(named))
            })
            .map(|(info, _)| info);
        let Some(next) = next else {
            break;
        };
        reached = true;
        path = next.path.clone();
        *consumed = i + 1;
        if !next.container {
            return Ok(path);
        }
    }
    if !reached {
        return Err(make_error(
            ErrorCode::NotFound,
            format!(
                "no GDAL algorithm is called that; GDAL LIST names them: {}",
                children_of(&[])
            ),
            words[0].clone(),
        ));
    }
    let text = path_text(&path);
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!("{text} is a group: name one of {}", children_of(&path)),
        text,
    ))
}

/// An algorithm's arguments as data, and GDAL's own usage JSON. NotFound for
/// a path that names nothing. A group has no arguments.
pub fn describe(path: &[String]) -> Result<AlgorithmSpec> {
    let Some(info) = find_info(path) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no GDAL algorithm has that path",
            path_text(path),
        ));
    };
    let _quiet = ErrorCollector::new();
    let algorithm = instantiate(path)?;
    let args = if info.container {
        Vec::new()
    } else {
        algorithm
            .args()
            .iter()
            .filter(|arg| !arg.is_hidden() && !arg.is_hidden_for_api())
            .map(spec_of)
            .collect()
    };
    Ok(AlgorithmSpec {
        info: info.clone(),
        long_description: algorithm.long_description(),
        args,
        usage_json: algorithm.usage_as_json(),
    })
}

/// The values GDAL offers for `arg` beginning with `prefix`: output formats
/// that can do what the algorithm needs, CRS codes, ... Empty when it offers
/// none or the argument does not exist.
pub fn suggest(path: &[String], arg: &str, prefix: &str) -> Vec<String> {
    let _quiet = ErrorCollector::new();
    let Ok(algorithm) = instantiate(path) else {
        return Vec::new();
    };
    algorithm
        .arg(arg)
        .map(|found| found.auto_complete_choices(prefix))
        .unwrap_or_default()
}

/// Which arguments GDAL's command-line tail gives a value to, by long name,
/// when `bound` (long names) are already set: options by name, and positional
/// words in GDAL's order - an argument already bound takes none. Read from
/// the declarations alone; nothing is opened. What the executor asks to know
/// whether a tail names the output, or a dataset FROM also binds.
pub fn arguments_given(
    path: &[String],
    tokens: &[String],
    bound: &[String],
) -> Result<Vec<String>> {
    let _quiet = ErrorCollector::new();
    let algorithm = instantiate(path)?;
    let names: BTreeSet<String> = bound
        .iter()
        .map(|name| {
            algorithm
                .arg(name)
                .map_or_else(|| name.clone(), |arg| arg.name())
        })
        .collect();
    let tail = read_tail(&algorithm, tokens, &names, run_detail::is_pipeline(path));
    Ok(tail.given.into_iter().collect())
}

/// Binds the request and asks GDAL to validate it, running nothing: required
/// arguments, exclusions, dependencies, values. Datasets given by name are
/// opened to be checked. Fails as run() does before it runs.
pub fn validate(request: &RunRequest) -> Status {
    ensure_registered();
    check_tokens(&request.path, &request.tokens)?;
    let errors = ErrorCollector::new();
    let mut config = ThreadConfig::default();
    config.set(&request.config)?;
    let mut sidecars = SidecarGuard::default();
    let mut bound = Binding::default();
    bind(request, &mut bound, &errors, &mut sidecars)?;
    let mark = errors.size();
    if !bound.algorithm().validate_arguments() {
        return Err(classified(
            &errors,
            &bound.output_file,
            ErrorCode::InvalidArgument,
            "GDAL refused the arguments",
            mark,
        ));
    }
    bound.algorithm = None;
    Ok(())
}

/// Runs one algorithm. Fails with
///
/// ```text
/// InvalidState "cancelled"  the stop was requested, before or during;
/// InvalidArgument           the request (an unknown argument, by name; a
///                           value GDAL refused, with its message; a missing
///                           or conflicting argument);
/// NotFound                  no algorithm has that path;
/// FileImportFailure         GDAL could not open an input;
/// FileExportFailure         GDAL could not write the output;
/// CommandRejected           GDAL ran and failed: its first failure message.
/// ```
///
/// `progress` is called with the fraction done, possibly from a GDAL thread.
/// A panic of `progress` cancels GDAL's run and is raised again here once
/// GDAL has returned.
pub fn run(
    request: &RunRequest,
    stop: Option<&AtomicBool>,
    progress: Option<&(dyn Fn(f64) + Sync)>,
) -> Result<RunOutputs> {
    ensure_registered();
    check_tokens(&request.path, &request.tokens)?;
    let stopped = || stop.is_some_and(|stop| stop.load(Ordering::Acquire));
    if stopped() {
        return Err(cancelled());
    }
    let started = Instant::now();
    let errors = ErrorCollector::new();
    let mut config = ThreadConfig::default();
    config.set(&request.config)?;
    // Declared before `bound`, so the sidecars are put back after every
    // dataset the run opened has closed.
    let mut sidecars = SidecarGuard::default();
    let mut bound = Binding::default();
    bind(request, &mut bound, &errors, &mut sidecars)?;
    // Validated apart from the run, so that what is wrong with the request is
    // InvalidArgument and not a failure of the algorithm.
    let mut mark = errors.size();
    if !bound.algorithm().validate_arguments() {
        return Err(classified(
            &errors,
            &bound.output_file,
            ErrorCode::InvalidArgument,
            "GDAL refused the arguments",
            mark,
        ));
    }
    if stopped() {
        return Err(cancelled());
    }

    let state = ProgressState {
        stop,
        progress,
        refused: AtomicBool::new(false),
        panic: Mutex::new(None),
    };
    mark = errors.size();
    // SAFETY: the adapter catches every panic and reads only the state,
    // which outlives the run and is safe to share with GDAL's threads.
    let ran = unsafe {
        bound.algorithm().run(
            Some(progress_adapter),
            ptr::from_ref(&state).cast_mut().cast(),
        )
    };
    let panicked = state
        .panic
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(payload) = panicked {
        resume_unwind(payload);
    }
    let was_cancelled = state.refused.load(Ordering::Acquire) || stopped();
    // Taken BEFORE Finalize, which drops the algorithm's reference and so,
    // for a dataset in memory, the dataset.
    let mut output: Option<DatasetRef> = None;
    if ran && !was_cancelled && bound.has_output {
        output = bound
            .algorithm()
            .arg("output")
            .and_then(|arg| arg.dataset_increase_ref_count());
    }
    let finalized = bound.algorithm().finalize();
    if was_cancelled {
        drop(output);
        bound.algorithm = None;
        // A partial file the run began is not left looking like a result.
        if !bound.output_file.is_empty()
            && !bound.output_existed
            && is_plain_file(&bound.output_file)
        {
            remove_path(Path::new(&bound.output_file));
        }
        return Err(cancelled());
    }
    if !ran || !finalized {
        return Err(classified(
            &errors,
            &bound.output_file,
            ErrorCode::CommandRejected,
            "GDAL reported a failure and said nothing about it",
            mark,
        ));
    }

    let mut outputs = RunOutputs::default();
    match output.as_ref().filter(|_| bound.memory_output) {
        Some(dataset) => {
            let dataset = dataset.raw();
            stamp_colour_map_bands(&request.path, dataset);
            // SAFETY: a live dataset.
            let (count, width, height) = unsafe {
                (
                    gdal::GDALGetRasterCount(dataset),
                    gdal::GDALGetRasterXSize(dataset),
                    gdal::GDALGetRasterYSize(dataset),
                )
            };
            if count > 0 {
                let cells = width as u64 * height as u64 * count as u64;
                if cells > request.max_memory_cells {
                    outputs.file = Some(spill(dataset, &request.spill_directory)?);
                } else {
                    outputs.raster = Some(read_grid(dataset)?);
                }
            }
            // SAFETY: a live dataset.
            if unsafe { gdal::GDALDatasetGetLayerCount(dataset) } > 0 {
                outputs.features = Some(read_features(dataset, &mut outputs.diagnostics));
            }
        }
        None => {
            if !bound.output_file.is_empty() {
                outputs.file = Some(bound.output_file.clone());
            }
        }
    }
    {
        let algorithm = bound.algorithm();
        if let Some(text) = algorithm.arg("output-string")
            && text.r#type() == alg::GAAT_STRING
        {
            let printed = text.string_value();
            if !printed.is_empty() {
                outputs.text = Some(printed);
            }
        }
        if let Some(code) = algorithm.arg("return-code")
            && code.r#type() == alg::GAAT_INTEGER
        {
            outputs.return_code = Some(code.integer_value());
        }
    }
    outputs
        .diagnostics
        .extend(errors.items().into_iter().filter(|item| !item.failure));
    drop(output);
    bound.algorithm = None;
    outputs.seconds = started.elapsed().as_secs_f64();
    Ok(outputs)
}

/// The versions of GDAL, PROJ and GEOS the process runs, and how many raster
/// and vector drivers and leaf algorithms GDAL has.
pub fn versions() -> Versions {
    ensure_registered();
    // SAFETY: static texts GDAL keeps, copied at once.
    let (gdal_release, full) = unsafe {
        (
            cpl::decoded(gdal::GDALVersionInfo(c"RELEASE_NAME".as_ptr())),
            cpl::decoded(gdal::GDALVersionInfo(c"--version".as_ptr())),
        )
    };
    // "GDAL 3.13.2 "Iowa City", released 2026/..": the name between quotes.
    let mut release = String::new();
    if let Some(open) = full.find('"')
        && let Some(close) = full[open + 1..].find('"')
    {
        release = String::from(&full[open + 1..open + 1 + close]);
    }
    let (mut major, mut minor, mut patch): (c_int, c_int, c_int) = (0, 0, 0);
    // SAFETY: three out values.
    unsafe { alg::OSRGetPROJVersion(&raw mut major, &raw mut minor, &raw mut patch) };
    let proj = format!("{major}.{minor}.{patch}");
    let mut geos = String::new();
    // SAFETY: three out values.
    if unsafe { alg::OGRGetGEOSVersion(&raw mut major, &raw mut minor, &raw mut patch) } {
        geos = format!("{major}.{minor}.{patch}");
    }
    let mut raster_drivers = 0;
    let mut vector_drivers = 0;
    // SAFETY: no argument.
    let driver_count = unsafe { gdal::GDALGetDriverCount() };
    for i in 0..driver_count {
        // SAFETY: one of the registered drivers, which live as long as the
        // process.
        let driver = unsafe { gdal::GDALGetDriver(i) };
        raster_drivers += i32::from(metadata_item(driver.cast(), gdal::GDAL_DCAP_RASTER).is_some());
        vector_drivers += i32::from(metadata_item(driver.cast(), gdal::GDAL_DCAP_VECTOR).is_some());
    }
    let algorithms = catalogue().iter().filter(|info| !info.container).count() as i32;
    Versions {
        gdal: gdal_release,
        release,
        proj,
        geos,
        raster_drivers,
        vector_drivers,
        algorithms,
    }
}

/// How many datasets run() has staged in memory under /vsimem/katana and not
/// yet removed: 0 whenever no run is under way. For the tests, which prove
/// the staging leaves nothing behind.
pub fn staged_dataset_count() -> usize {
    ensure_registered();
    let root = cpl::c_string(STAGING_ROOT);
    // SAFETY: a NUL-terminated path; the list returned is the caller's.
    let entries = unsafe { cpl::take_string_list(cpl::VSIReadDirRecursive(root.as_ptr())) };
    entries
        .unwrap_or_default()
        .iter()
        .filter(|name| name.ends_with(b".tif") || name.ends_with(b".gpkg"))
        .count()
}
