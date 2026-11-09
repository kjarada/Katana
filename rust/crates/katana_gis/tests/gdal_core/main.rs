//! The GDAL core's cases (step 5, wave K), one module per C++ test file:
//!
//! ```text
//! gdal_pdal_adapters  tests/io/test_gdal_pdal_adapters.cpp   GdalAdapter
//! archive12d          tests/interop/test_archive12d.cpp      ZipContainer
//! drape_and_sample    tests/geo/test_drape_and_sample.cpp    RasterSampler
//! formats             tests/geo/test_formats.cpp             three Formats cases
//! ```
//!
//! The other suites of those files are other crates' (PointCloudEngine is
//! katana_pointcloud's; the archive and the formats' import cases are
//! katana_interop's) or the app's (step 6). Each gtest suite is a module
//! inside its file's module, named as tools/check_rust_parity.py expects.
//!
//! libtest runs these cases on parallel threads of one process, where gtest
//! ran each in its own: every scratch file is named for the process, and the
//! cases that run GDAL on a shared file under tests/geo/data take one lock,
//! so GDAL's .aux.xml sidecars never race.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod archive12d;
mod drape_and_sample;
mod formats;
mod gdal_pdal_adapters;

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

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

static SHARED_DATA: Mutex<()> = Mutex::new(());

/// The lock of the cases that run GDAL on a file of tests/geo/data.
pub fn shared_data() -> MutexGuard<'static, ()> {
    SHARED_DATA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
