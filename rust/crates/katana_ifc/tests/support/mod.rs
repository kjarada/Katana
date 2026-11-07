//! What the ifc test binaries share: gtest's EXPECT_NEAR, where the C++
//! suite's data and the samples are, a scratch folder unique to a test, and
//! the unwrapping a C++ `ASSERT_TRUE(result.ok())` is.

#![allow(
    dead_code,
    reason = "Each test binary uses a part of the shared support."
)]

use std::path::{Path, PathBuf};

use katana_core::Result;

/// `EXPECT_NEAR(actual, expected, epsilon)`.
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// KATANA_IFC_TEST_DATA: tests/ifc/data, where the C++ suite reads it.
pub const TEST_DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/ifc/data");

/// KATANA_SAMPLES: the checkout's samples.
pub const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../samples");

/// A file's whole text, as the C++ tests' readText has it.
pub fn read_text(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The value of an `ASSERT_TRUE(result.ok()) << result.error().describe()`.
pub fn must<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// A folder of the temporary directory for one test, removed when the test
/// is done: unique by the test's name and the process, since the tests of a
/// binary run side by side in one process.
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// `name` in the folder, holding `text`; its path as text.
    pub fn file(&self, name: &str, text: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
