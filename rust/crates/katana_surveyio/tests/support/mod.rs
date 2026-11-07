// Support shared by the surveyio layer's test binaries.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use std::path::{Path, PathBuf};

/// A scratch folder unique to one test of one run (the test's name and the
/// process id), removed on drop: the tests of a binary run on parallel threads
/// of one process, where gtest ran each case in its own.
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

    /// Writes `bytes` to `relative` under the folder, making its folders.
    pub fn write(&self, relative: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// haystack.find(needle) != npos.
pub fn contains(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}
