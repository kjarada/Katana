//! The geoprocessing bridge's cases (step 5, wave L, agent L1), one module per
//! C++ test file:
//!
//! ```text
//! processing           tests/geo/test_processing.cpp
//! policy               tests/geo/test_policy.cpp
//! processing_contract  tests/geo/test_processing_contract.cpp
//! contour_verb         tests/geo/test_contour_verb.cpp        ContourContract
//! shade_verb           tests/geo/test_shade_verb.cpp          ShadeContract
//! viewshed_and_los     tests/geo/test_viewshed_and_los.cpp    ViewshedContract
//! buffer_and_dissolve  tests/geo/test_buffer_and_dissolve.cpp GisBufferContract
//! check_verbs          tests/geo/test_check_verbs.cpp         GisCheckContract
//! dem_verbs            tests/geo/test_dem_verbs.cpp           DemVerbsContract
//! grid_verb            tests/geo/test_grid_verb.cpp           RasterGridContract
//! hull_and_clip        tests/geo/test_hull_and_clip.cpp       GisHullClipContract
//! overlay_verb         tests/geo/test_overlay_verb.cpp        GisOverlayContract
//! pipeline_verb        tests/geo/test_pipeline_verb.cpp       PipelineContract
//! slope_verbs          tests/geo/test_slope_verbs.cpp         SlopeContract
//! sql_verb             tests/geo/test_sql_verb.cpp            GisSqlContract
//! zonal_verb           tests/geo/test_zonal_verb.cpp          ZonalContract
//! ```
//!
//! The verb files' other suites drive the app layer (step 6), or interop's
//! bindings (katana_interop's drawing_bindings). Each gtest suite is a module
//! inside its file's module, named as tools/check_rust_parity.py expects.
//!
//! libtest runs these cases on parallel threads of one process, where gtest
//! ran each in its own: every scratch folder is named for its case and the
//! process, and the cases that run GDAL on a file of tests/geo/data take one
//! lock (`shared_data`), so GDAL's .aux.xml sidecars - put back by the
//! bridge's own guard, which is per run - never race (the step-5 plan,
//! section 7, item 5). No input or expected value is changed for it.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/contract_support.rs"]
mod contract_support;

mod buffer_and_dissolve;
mod check_verbs;
mod contour_verb;
mod dem_verbs;
mod grid_verb;
mod hull_and_clip;
mod overlay_verb;
mod pipeline_verb;
mod policy;
mod processing;
mod processing_contract;
mod shade_verb;
mod slope_verbs;
mod sql_verb;
mod viewshed_and_los;
mod zonal_verb;

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// The words as the owned strings a path or a tail is.
pub fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four units in the last place, as gtest compares.
#[track_caller]
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// ASSERT_TRUE(result.ok()) << result.error().describe(), and the value.
#[track_caller]
pub fn must<T>(result: katana_core::Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// ASSERT_FALSE(result.ok()), and the error.
#[track_caller]
pub fn refused<T>(result: katana_core::Result<T>, what: &str) -> katana_core::Error {
    match result {
        Ok(_) => panic!("{what} should have been refused"),
        Err(error) => error,
    }
}

/// A file of the checkout, where the C++ reads it.
pub fn checkout(relative: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")).join(relative)
}

/// A file of tests/geo/data (KATANA_GEO_TEST_DATA), as the text a dataset
/// path is.
pub fn geo_data(name: &str) -> String {
    checkout("tests/geo/data")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

static SHARED_DATA: Mutex<()> = Mutex::new(());

/// The lock of the cases that run GDAL on a file of tests/geo/data.
pub fn shared_data() -> MutexGuard<'static, ()> {
    SHARED_DATA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A folder of a case's own under the temporary directory, removed with its
/// contents when dropped: the C++'s TempDir("katana-geo-<name>"), named for
/// the process too, since libtest runs the cases on threads of one process.
pub struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("katana-geo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    /// The path of `name` in the folder, with forward slashes
    /// (generic_string).
    pub fn file(&self, name: &str) -> String {
        self.path.join(name).to_string_lossy().replace('\\', "/")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
