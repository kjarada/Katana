//! The customisation cases that change what the process shares: the seam
//! KATANA_BUILTIN_CUSTOMISATION in the environment, and the working folder (wave H2). One module
//! per C++ test file they come from:
//!
//! ```text
//! customisation_host   tests/cad/customisation/test_customisation_host.cpp
//! builtin_library      tests/cad/customisation/test_builtin_library.cpp
//! ```
//!
//! gtest ran each case in a process of its own; Rust runs a binary's tests on parallel threads of
//! one, so each case here holds ENVIRONMENT for its whole length, and the seam and the working
//! folder it sets are put back before it lets go. The seam is not set in the process's environment
//! (std::env::set_var is unsafe, since a thread beside may be reading it) but overridden where the
//! host reads it, through katana_core's environment_variable (EnvironmentOverride). The other cases
//! of those files are in the target tests/customisation.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/customisation_support.rs"]
mod customisation_support;

mod builtin_library;
mod customisation_host;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use katana_cad::customisation_host::BUILT_IN_CUSTOMISATION_VARIABLE;
use katana_core::path_text::{EnvironmentOverride, environment_variable};

/// What the process shares - its environment and its working folder - is changed only while this
/// is held. A case that panics while holding it poisons it; the next takes it all the same, since
/// what the panicking case set was put back as it unwound.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// ENVIRONMENT, held until the returned guard is dropped: at the start of every case here.
pub fn environment() -> MutexGuard<'static, ()> {
    ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The seam as the host reads it, set for one case and read as it was - set or not - when it is
/// dropped, so that no case depends on, or leaves a mark on, the environment it runs in. Made only
/// by a case holding ENVIRONMENT, which it is handed as proof.
pub struct ScopedSeam {
    // What the host read before, as it reads it (environment_variable: empty when not set).
    before: Vec<u8>,
    _override: EnvironmentOverride,
}

impl ScopedSeam {
    /// None: the variable is not set at all.
    pub fn new(_held: &MutexGuard<'static, ()>, value: Option<&OsStr>) -> Self {
        let before = environment_variable(Some(BUILT_IN_CUSTOMISATION_VARIABLE));
        Self {
            before,
            _override: EnvironmentOverride::new(BUILT_IN_CUSTOMISATION_VARIABLE, value),
        }
    }

    /// The seam's value as text, as a case gives it.
    pub fn text(held: &MutexGuard<'static, ()>, value: &str) -> Self {
        Self::new(held, Some(OsStr::new(value)))
    }

    pub fn before(&self) -> &[u8] {
        &self.before
    }
}

/// The working folder, made `folder` for one case and put back when it is dropped - whether the
/// case ends or panics. Made only by a case holding ENVIRONMENT.
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
