//! Phase 0's spike (docs/rust.md, "The GDAL algorithm spike").
//!
//! The C++ drives GDAL's algorithm framework through its C++ API
//! (src/katana_io/geo/processing.cpp), which no Rust binding wraps. GDAL
//! exposes the same framework in C (gdalalgorithm_c.h), and this proves Rust
//! reaches it there on both of the port's toolchains - MSYS2 on Windows, the
//! conda-forge prefix in WSL and the Linux and macOS release jobs - by running
//! `gdal raster info` and reading back what the algorithm wrote. Phase 5
//! replaces these hand-written declarations with the crate's real binding.

// FFI: the declarations below are GDAL's C API, and calling them is unsafe.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr;

type Registry = *mut c_void;
type Algorithm = *mut c_void;
type Argument = *mut c_void;
type Progress = Option<unsafe extern "C" fn(f64, *const c_char, *mut c_void) -> i32>;

#[link(name = "gdal")]
unsafe extern "C" {
    fn GDALAllRegister();
    fn CPLSetConfigOption(key: *const c_char, value: *const c_char);
    fn GDALGetGlobalAlgorithmRegistry() -> Registry;
    fn GDALAlgorithmRegistryRelease(registry: Registry);
    fn GDALAlgorithmRegistryInstantiateAlgFromPath(
        registry: Registry,
        path: *const *const c_char,
    ) -> Algorithm;
    fn GDALAlgorithmParseCommandLineArguments(
        algorithm: Algorithm,
        arguments: *const *const c_char,
    ) -> bool;
    fn GDALAlgorithmRun(algorithm: Algorithm, progress: Progress, data: *mut c_void) -> bool;
    fn GDALAlgorithmFinalize(algorithm: Algorithm) -> bool;
    fn GDALAlgorithmRelease(algorithm: Algorithm);
    fn GDALAlgorithmGetArg(algorithm: Algorithm, name: *const c_char) -> Argument;
    fn GDALAlgorithmArgGetAsString(argument: Argument) -> *const c_char;
    fn GDALAlgorithmArgRelease(argument: Argument);
}

/// A NULL-terminated list of C strings, as GDAL's `CSLConstList` is.
fn string_list(items: &[CString]) -> Vec<*const c_char> {
    items.iter().map(|item| item.as_ptr()).chain([ptr::null()]).collect()
}

/// `gdal raster info --format json <raster>`, through the C API: what the
/// algorithm left in its output-string argument.
fn raster_info_json(raster: &str) -> Result<String, String> {
    let path = [CString::new("raster").unwrap(), CString::new("info").unwrap()];
    let arguments = [
        CString::new("--format=json").unwrap(),
        CString::new(raster).map_err(|error| format!("{error}"))?,
    ];
    let path_list = string_list(&path);
    let argument_list = string_list(&arguments);
    // SAFETY: every pointer handed to GDAL is a live NUL-terminated string or
    // a NULL-terminated list of them, alive for the call; every handle GDAL
    // returns is released exactly once below, after its last use.
    unsafe {
        GDALAllRegister();
        // No .aux.xml beside the fixture: GDAL's persistent auxiliary metadata
        // is a file written next to the raster, in the source tree.
        let pam = CString::new("GDAL_PAM_ENABLED").unwrap();
        let no = CString::new("NO").unwrap();
        CPLSetConfigOption(pam.as_ptr(), no.as_ptr());

        let registry = GDALGetGlobalAlgorithmRegistry();
        if registry.is_null() {
            return Err("GDAL has no algorithm registry".into());
        }
        let algorithm = GDALAlgorithmRegistryInstantiateAlgFromPath(registry, path_list.as_ptr());
        GDALAlgorithmRegistryRelease(registry);
        if algorithm.is_null() {
            return Err("GDAL has no `raster info` algorithm".into());
        }
        let outcome = if !GDALAlgorithmParseCommandLineArguments(algorithm, argument_list.as_ptr()) {
            Err("GDAL refused the arguments".to_owned())
        } else if !GDALAlgorithmRun(algorithm, None, ptr::null_mut()) {
            Err("`raster info` failed".to_owned())
        } else {
            let name = CString::new("output-string").unwrap();
            let argument = GDALAlgorithmGetArg(algorithm, name.as_ptr());
            if argument.is_null() {
                Err("`raster info` has no output-string argument".to_owned())
            } else {
                let text = GDALAlgorithmArgGetAsString(argument);
                let output = if text.is_null() {
                    Err("the output-string argument is empty".to_owned())
                } else {
                    Ok(CStr::from_ptr(text).to_string_lossy().into_owned())
                };
                GDALAlgorithmArgRelease(argument);
                output
            }
        };
        GDALAlgorithmFinalize(algorithm);
        GDALAlgorithmRelease(algorithm);
        outcome
    }
}

#[test]
fn the_raster_info_algorithm_runs_through_the_c_api_and_reports_the_rasters_size() {
    // The geo suite's plane, an ESRI ASCII grid whose header says `ncols 40`
    // and `nrows 30`: 40 columns wide and 30 rows high, which GDAL reports as
    // the size [width, height].
    let raster: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "..", "..", "tests", "geo", "data", "plane.asc"]
        .iter()
        .collect();
    let json = raster_info_json(raster.to_str().expect("the checkout's path is UTF-8"))
        .unwrap_or_else(|why| panic!("{why}"));
    let compact: String = json.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(compact.contains(r#""size":[40,30]"#), "raster info said: {json}");
    assert!(compact.contains(r#""driverShortName":"AAIGrid""#), "raster info said: {json}");
}
