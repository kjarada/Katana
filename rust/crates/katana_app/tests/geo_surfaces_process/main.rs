//! The cases of step 6's agent A that change the working folder - a state the
//! whole process shares - one module per C++ test file they come from:
//!
//! ```text
//! refs_verbs  tests/geo/test_refs_verbs.cpp  RefsSession.ALayerImportedByARelativePathReopensFromAnotherFolder
//! session     tests/app/test_session.cpp     Session.InfoOfAFileNamedLikeAnIdStillReadsTheFile
//! ```
//!
//! gtest ran each case in a process of its own; Rust runs a binary's tests on
//! parallel threads of one, so each case here holds the one lock for its whole
//! length, and the folder it changes to is put back before it lets go. The
//! other cases of those files are in tests/geo_surfaces (and S1's
//! tests/session).

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

#[path = "../support/app_support.rs"]
pub mod app_support;
#[path = "../support/geo_fixture.rs"]
pub mod geo_fixture;
#[path = "../support/surface_support.rs"]
pub mod surface_support;

mod refs_verbs;
mod session;

static WORKING_FOLDER: Mutex<()> = Mutex::new(());

/// The lock of every case that changes the working folder.
pub fn working_folder() -> MutexGuard<'static, ()> {
    WORKING_FOLDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The working folder, changed while it lives and put back after (the C++'s
/// InFolder). Made only while the lock is held.
pub struct InFolder {
    was: PathBuf,
}

impl InFolder {
    pub fn new(folder: &Path) -> Self {
        let was = std::env::current_dir().expect("the working folder");
        std::env::set_current_dir(folder).expect("the working folder changed");
        Self { was }
    }
}

impl Drop for InFolder {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.was);
    }
}
