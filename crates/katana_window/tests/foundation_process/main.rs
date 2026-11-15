//! The foundation's cases that change process-wide state (F0, step 7 wave 0): the twins of
//! tests/qt_widgets/test_project_folder.cpp, one of which moves the working directory. A test
//! binary runs its cases on parallel threads of one process, so every case here takes the one
//! lock first.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

use std::sync::{Mutex, MutexGuard, PoisonError};

#[path = "../support/window_support.rs"]
pub mod window_support;

mod project_folder;

/// The process's state - its working directory - is this target's one lock's.
static PROCESS: Mutex<()> = Mutex::new(());

/// The lock, whatever a case that panicked holding it left.
pub fn process_lock() -> MutexGuard<'static, ()> {
    PROCESS.lock().unwrap_or_else(PoisonError::into_inner)
}
