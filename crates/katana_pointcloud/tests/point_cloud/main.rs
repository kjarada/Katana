//! The point-cloud engine's cases of tests/io/test_gdal_pdal_adapters.cpp, one
//! module per C++ test file:
//!
//! ```text
//! gdal_pdal_adapters   tests/io/test_gdal_pdal_adapters.cpp (PointCloudEngine)
//! ```
//!
//! The file's GdalAdapter suite is katana_gis's (its gdal_core target). These
//! are the seam with PDAL, so the tests drive real files rather than mocks:
//! what needs verifying is how PDAL actually behaves, not how we imagine it
//! does. Failures are reported as Result, never thrown.
//!
//! The C++ named its files under the temporary directory, one name per case,
//! each case its own process; libtest runs the cases on threads of one
//! process, so each case writes in a folder of its own (its name and the
//! process id), removed with it. PDAL itself takes no lock here: each case
//! has files of its own, and PDAL guards its process-wide registries itself
//! (point_cloud_engine's module documentation).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod gdal_pdal_adapters;

use std::path::PathBuf;

use katana_core::Result;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// The value of a result the case needs, failing the case with the error's
/// description otherwise (ASSERT_TRUE(result.ok()) << describe()).
#[track_caller]
pub fn ok<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// A folder of a case's own under the temporary directory, removed with its
/// contents when dropped.
pub struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("katana-io-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    /// The C++ TempFile's name, in this folder.
    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
