//! The online catalogue and the colour ramps (step 5, agent K2), one module
//! per C++ test file:
//!
//! ```text
//! online_catalogue   tests/interop/test_online_catalogue.cpp
//! shade_verb         tests/geo/test_shade_verb.cpp (the ColourRamps suite)
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. The rest of test_shade_verb.cpp drives
//! the app layer's verbs and is step 6's; its ShadeContract case is the
//! bridge's (wave L).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

mod online_catalogue;
mod shade_verb;

use std::path::PathBuf;

/// A folder of a case's own under the temporary directory, removed with its
/// contents when dropped: the C++'s fixed names became the case's name and
/// the process id, since libtest runs the cases on threads of one process.
pub struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "katana-interop-catalogue-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
