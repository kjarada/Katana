//! A FsOps that wraps another and breaks on demand: the deterministic fault-injection seam of the
//! update tests (docs/update.md).
//!
//! - crash_before(k): the k-th MUTATING operation (fsops lists them; counted from 1 across the
//!   wrapper's life) is not performed: a [`SimulatedCrash`] panic is raised in its place, as if
//!   the process had died between operation k-1 and k. What the earlier operations wrote stays on
//!   disk, which is the point: the test then runs the recovery with a clean FsOps on that disk and
//!   requires a tree whose whole hash equals the old or the new version's.
//! - fail_rename_with(k, e): the k-th rename_file (counted among renames only) returns an error
//!   with context `os_error=<e>` (32 for a locked file) instead of performing the rename, as the
//!   real implementation does after its retries ran out; the apply must then roll back.
//!
//! A test sizes its loop by running the apply once with no fault and reading
//! mutating_operations(). The same wrapper goes round the recovery to crash INSIDE a recovery
//! (the second loop). The test build of the helper builds one from `--die-at N` (never the
//! helper itself).
//!
//! SimulatedCrash is deliberately not an Error: no code under test may catch it and carry on. It
//! is the payload of a panic (std::panic::panic_any), as the C++ throws it; a test takes it back
//! with catch_unwind and downcast. It is in the library, not in a test, because the helper's test
//! build and the end-to-end program build one too.
//!
//! The settings and the counters are Cells, so that the wrapper is handed to the apply as a
//! shared `&dyn FsOps` and the test reads and changes them around it.

use crate::fsops::{EntryInfo, FileLock, FsOps, path_text};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use std::cell::Cell;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SimulatedCrash {
    /// The 1-based index of the operation not performed.
    pub operation: u64,
    /// "renameFile R/bin/a.dll -> .update/t1/old/bin/a.dll".
    pub description: String,
}

pub struct FaultFs<'a> {
    inner: &'a dyn FsOps,
    crash_before: Cell<u64>, // 0: never
    fail_rename: Cell<u64>,  // 0: never
    fail_rename_error: Cell<i32>,
    operations: Cell<u64>,
    renames: Cell<u64>,
}

impl<'a> FaultFs<'a> {
    /// `inner` outlives this object.
    pub fn new(inner: &'a dyn FsOps) -> Self {
        Self {
            inner,
            crash_before: Cell::new(0),
            fail_rename: Cell::new(0),
            fail_rename_error: Cell::new(0),
            operations: Cell::new(0),
            renames: Cell::new(0),
        }
    }

    pub fn crash_before(&self, operation: u64) {
        self.crash_before.set(operation);
    }

    pub fn fail_rename_with(&self, rename_index: u64, os_error: i32) {
        self.fail_rename.set(rename_index);
        self.fail_rename_error.set(os_error);
    }

    /// Clears both settings and the counters.
    pub fn reset(&self) {
        self.crash_before.set(0);
        self.fail_rename.set(0);
        self.fail_rename_error.set(0);
        self.operations.set(0);
        self.renames.set(0);
    }

    /// Mutating operations performed so far (not counting one that crashed).
    pub fn mutating_operations(&self) -> u64 {
        self.operations.get()
    }

    // Every changing operation goes through here first: the k-th is the one a crash replaces.
    // The description is only built when the crash happens.
    fn fault_point(&self, description: impl FnOnce() -> String) {
        let next = self.operations.get() + 1;
        let crash_before = self.crash_before.get();
        if crash_before != 0 && next == crash_before {
            std::panic::panic_any(SimulatedCrash {
                operation: next,
                description: description(),
            });
        }
        self.operations.set(next);
    }
}

impl FsOps for FaultFs<'_> {
    // ---- looking passes straight through ------------------------------------------------------

    fn stat(&self, path: &Path) -> Result<EntryInfo> {
        self.inner.stat(path)
    }

    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        self.inner.list_directory(directory)
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.inner.read_file(path)
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        self.inner.resolve(path)
    }

    fn free_space(&self, path: &Path) -> Result<u64> {
        self.inner.free_space(path)
    }

    // ---- changing is counted and may be made to crash or fail ---------------------------------

    fn make_directories(&self, directory: &Path) -> Status {
        self.fault_point(|| format!("makeDirectories {}", path_text(directory)));
        self.inner.make_directories(directory)
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Status {
        self.fault_point(|| format!("copyFile {} -> {}", path_text(from), path_text(to)));
        self.inner.copy_file(from, to)
    }

    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        self.fault_point(|| format!("renameFile {} -> {}", path_text(from), path_text(to)));
        let renames = self.renames.get() + 1;
        self.renames.set(renames);
        let fail_rename = self.fail_rename.get();
        if fail_rename != 0 && renames == fail_rename {
            // What the real implementation reports once its retries have run out: the file was
            // not moved.
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "the file could not be moved (injected)",
                format!(
                    "os_error={} {} -> {}",
                    self.fail_rename_error.get(),
                    path_text(from),
                    path_text(to)
                ),
            ));
        }
        self.inner.rename_file(from, to)
    }

    fn remove_file(&self, path: &Path) -> Status {
        self.fault_point(|| format!("removeFile {}", path_text(path)));
        self.inner.remove_file(path)
    }

    fn remove_empty_directory(&self, path: &Path) -> Status {
        self.fault_point(|| format!("removeEmptyDirectory {}", path_text(path)));
        self.inner.remove_empty_directory(path)
    }

    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        self.fault_point(|| format!("appendDurable {}", path_text(path)));
        self.inner.append_durable(path, bytes)
    }

    fn write_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        self.fault_point(|| format!("writeDurable {}", path_text(path)));
        self.inner.write_durable(path, bytes)
    }

    fn set_executable(&self, path: &Path) -> Status {
        self.fault_point(|| format!("setExecutable {}", path_text(path)));
        self.inner.set_executable(path)
    }

    // ---- not counted ---------------------------------------------------------------------------

    fn sync_directory(&self, directory: &Path) -> Status {
        self.inner.sync_directory(directory)
    }

    fn try_lock(&self, path: &Path) -> Result<Box<dyn FileLock>> {
        self.inner.try_lock(path)
    }
}
