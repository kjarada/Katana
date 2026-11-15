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
//! folder it sets are put back before it lets go. The other cases of those files are in the target
//! tests/customisation.

#![allow(
    unsafe_code,
    reason = "Edition 2024 makes std::env::set_var and remove_var unsafe: they race a thread that \
              reads the environment, which every case here rules out by holding ENVIRONMENT."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/customisation_support.rs"]
mod customisation_support;

mod builtin_library;
mod customisation_host;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use katana_cad::customisation_host::BUILT_IN_CUSTOMISATION_VARIABLE;

/// What the process shares - its environment and its working folder - is changed only while this
/// is held. A case that panics while holding it poisons it; the next takes it all the same, since
/// what the panicking case set was put back as it unwound.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// ENVIRONMENT, held until the returned guard is dropped: at the start of every case here.
pub fn environment() -> MutexGuard<'static, ()> {
    ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
}

// Sets the seam to `value`, or removes it for None. The caller holds ENVIRONMENT.
fn set_seam(value: Option<&OsStr>) {
    match value {
        // SAFETY: every case of this binary holds ENVIRONMENT while it reads or
        // changes the environment, and none of them starts a thread, so no
        // other thread reads it while it is changed.
        Some(value) => unsafe { std::env::set_var(BUILT_IN_CUSTOMISATION_VARIABLE, value) },
        // SAFETY: as above.
        None => unsafe { std::env::remove_var(BUILT_IN_CUSTOMISATION_VARIABLE) },
    }
}

/// The seam, set for one case and put back as it was - set or not - when it is dropped, so that no
/// case depends on, or leaves a mark on, the environment it runs in. Made only by a case holding
/// ENVIRONMENT, which it is handed as proof.
pub struct ScopedSeam {
    before: Option<OsString>,
}

impl ScopedSeam {
    /// None: the variable is not set at all.
    pub fn new(_held: &MutexGuard<'static, ()>, value: Option<&OsStr>) -> Self {
        let before = std::env::var_os(BUILT_IN_CUSTOMISATION_VARIABLE);
        set_seam(value);
        Self { before }
    }

    /// The seam's value as text, as a case gives it.
    pub fn text(held: &MutexGuard<'static, ()>, value: &str) -> Self {
        Self::new(held, Some(OsStr::new(value)))
    }

    pub fn before(&self) -> Option<&OsStr> {
        self.before.as_deref()
    }
}

impl Drop for ScopedSeam {
    fn drop(&mut self) {
        set_seam(self.before.as_deref());
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
