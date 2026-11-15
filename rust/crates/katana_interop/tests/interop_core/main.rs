//! The interop core's cases (step 5, wave M, agent M1), one module per C++
//! test file:
//!
//! ```text
//! interop          tests/interop/test_interop.cpp        every case
//! archive12d       tests/interop/test_archive12d.cpp     the archive cases (ZipContainer is
//!                                                        katana_gis's gdal_core target)
//! round_trip       tests/interop/test_round_trip.cpp     every case
//! dataset_info     tests/interop/test_dataset_info.cpp   every case
//! terrain_io       tests/interop/test_terrain_io.cpp     every case
//! online_fetch     tests/interop/test_online_fetch.cpp   OnlineFetch; the Copernicus live case
//! online_verbs     tests/interop/test_online_verbs.cpp   every case
//! formats          tests/geo/test_formats.cpp            the Formats cases of the interop core
//! vector_fidelity  tests/geo/test_vector_fidelity.cpp    every case but WarningsReachTheReply
//! ```
//!
//! The other suites of those files are other targets' (OnlineDiscovery and
//! two live cases: tests/online_requests and tests/online_environment) or the
//! app's (step 6: FormatsVerb, AGpkgHoldingOnlyARasterRoutesToRasterImport,
//! WarningsReachTheReply). Each gtest suite is a module inside its file's
//! module, named as tools/check_rust_parity.py expects.
//!
//! libtest runs these cases on parallel threads of one process, where gtest
//! ran each in its own: every scratch folder is named for the case and the
//! process. No case runs GDAL on a file of tests/geo/data or samples/gis in
//! place - those it reads are read as bytes or copied first - so none takes
//! a lock. No case reaches the network but OnlineLive's, which runs only with
//! KATANA_ONLINE_TESTS=1, as the C++ one does.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::excessive_precision,
    reason = "Expected values are copied digit for digit from the C++ tests."
)]

#[path = "../support/online_support.rs"]
mod online_support;

mod archive12d;
mod dataset_info;
mod formats;
mod interop;
mod online_fetch;
mod online_verbs;
mod round_trip;
mod terrain_io;
mod vector_fidelity;

use std::fs;
use std::path::{Path, PathBuf};

pub use online_support::{double_eq, must, near, refused};

/// A folder of the case's own, named for it and the process, removed
/// afterwards: the C++'s TempDir, whose fixed name would be shared by the
/// threads libtest runs the cases on.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// `prefix` is the C++ suite's ("katana-interop-"), `name` its case's.
    pub fn new(prefix: &str, name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{prefix}{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the scratch folder is made");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A file of the checkout, where the C++ reads it.
pub fn checkout(relative: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")).join(relative)
}

/// tests/archive12d/data (KATANA_ARCHIVE12D_TEST_DATA).
pub fn archive_fixture(name: &str) -> PathBuf {
    checkout("tests/archive12d/data").join(name)
}

/// tests/geo/data (KATANA_GEO_TEST_DATA).
pub fn geo_fixture(name: &str) -> PathBuf {
    checkout("tests/geo/data").join(name)
}

/// A file's bytes, or nothing for one that cannot be read, as the C++'s
/// stream reads.
pub fn bytes_of(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_default()
}

/// The file's text, its bytes as they are.
pub fn file_text(path: &Path) -> String {
    String::from_utf8_lossy(&bytes_of(path)).into_owned()
}

pub fn write_file(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::write(path, bytes).expect("the file is written");
}

/// Any warning holding `fragment`.
pub fn warned(warnings: &[String], fragment: &str) -> bool {
    warnings.iter().any(|warning| warning.contains(fragment))
}
