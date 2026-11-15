//! The CUSTOMISE case that changes what the process shares: the working folder (wave J, agent
//! J3). One module per C++ test file it comes from:
//!
//! ```text
//! customisation_verbs   tests/cad/customisation/test_customisation_verbs.cpp
//! ```
//!
//! gtest ran each case in a process of its own; Rust runs a binary's tests on parallel threads of
//! one, so a case here holds WORKING_FOLDER for its whole length, and the folder it sets is put
//! back before it lets go. The other cases of that file are in the target
//! tests/customisation_verbs, whose support module this target shares.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/customisation_support.rs"]
mod customisation_support;
#[path = "../customisation_verbs/verbs_support.rs"]
mod verbs_support;

mod customisation_verbs;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The working folder is changed only while this is held. A case that panics while holding it
/// poisons it; the next takes it all the same, since the folder the panicking case set was put
/// back as it unwound.
static WORKING_FOLDER: Mutex<()> = Mutex::new(());

/// WORKING_FOLDER, held until the returned guard is dropped: at the start of every case here.
pub fn working_folder_lock() -> MutexGuard<'static, ()> {
    WORKING_FOLDER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The working folder, made `folder` for one case and put back when it is dropped - whether the
/// case ends or panics. Made only by a case holding WORKING_FOLDER, which it is handed as proof.
pub struct WorkingFolder {
    before: PathBuf,
}

impl WorkingFolder {
    pub fn new(_held: &MutexGuard<'static, ()>, folder: &Path) -> Self {
        let before = std::env::current_dir().expect("the working folder is known");
        std::env::set_current_dir(folder).expect("the working folder is changed");
        Self { before }
    }
}

impl Drop for WorkingFolder {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.before);
    }
}
