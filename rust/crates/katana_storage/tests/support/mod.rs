//! What the storage tests share: gtest's EXPECT_DOUBLE_EQ, the C++ fixtures'
//! directory of their own per case, and where the committed fixtures are.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use std::path::{Path, PathBuf};

/// EXPECT_DOUBLE_EQ: within four units in the last place.
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

/// The C++ fixtures' SetUp and TearDown: an empty directory of the case's
/// own, `<temp>/katana-storage-tests/<Suite>.<Case>`, removed afterwards.
/// The process id is added to the name because Rust runs a binary's cases on
/// parallel threads of one process, and two runs of the suite at once must not
/// share a folder either.
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(suite: &str, case: &str) -> Self {
        let root = std::env::temp_dir()
            .join("katana-storage-tests")
            .join(format!("{suite}.{case}.{}", std::process::id()));
        let _ = std::fs::remove_dir_all(root.as_path());
        std::fs::create_dir_all(root.as_path()).unwrap();
        Self { root }
    }

    pub fn project_dir(&self) -> PathBuf {
        self.root.join("site.katana")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The checkout's root, where the C++ tests find the samples.
pub fn repository() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."))
}

/// tests/storage/fixtures, the files the C++ suite reads.
pub fn fixtures() -> PathBuf {
    repository().join("tests/storage/fixtures")
}

/// Overwrites `file` with as many 'Z' (0x5A) bytes as it holds.
pub fn overwrite_with_garbage(file: &Path) {
    let size = std::fs::metadata(file).unwrap().len();
    std::fs::write(file, vec![0x5A_u8; size as usize]).unwrap();
}
