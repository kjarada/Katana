//! Every way the apply, the rollback and the recovery touch the disk, behind one trait. Two
//! reasons it is a trait and not calls to std::fs:
//! 1. The fault-injection loops ([`crate::fault_fs`]) wrap it to crash the apply before the k-th
//!    change and then require the recovery to leave a tree whose whole hash equals the old
//!    version's or the new one's - for every k of a 40-file apply. That is only possible if
//!    every change goes through one seam.
//! 2. A rename of a file in use fails on Windows (errors 5, 32, 33: access denied, sharing and
//!    lock violations, which antivirus scanners cause too). The retry policy belongs to the real
//!    implementation, once.
//!
//! Paths are absolute and already resolved by the caller. An operation that fails returns an
//! Error whose context begins `os_error=<n>` (the GetLastError or errno value, 0 when there was
//! none), so a caller and a test can tell a locked file (32) from a missing one without parsing
//! prose.
//!
//! The MUTATING operations - the ones FaultFs counts and may crash before - are exactly:
//! make_directories, copy_file, rename_file, remove_file, remove_empty_directory, append_durable,
//! write_durable, set_executable. The rest only look.

use katana_core::{Result, Status};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EntryKind {
    #[default]
    Missing,
    File,
    Directory,
    Other,
}

/// Whether a Windows reparse tag is that of a LINK: a name surrogate (winnt.h:
/// IsReparseTagNameSurrogate, bit 29), which makes a path stand for another path. Symbolic links
/// (0xA000000C), junctions and mount points (0xA0000003), WSL symlinks (0xA000001D) are; OneDrive
/// and other cloud placeholders (0x9000001A, 0x9000701A, ...), the de-duplication and compression
/// filters (0x80000013, 0x80000017), app execution aliases (0x8000001B) and ProjFS (0x9000001C)
/// are not. The ONE definition of the rule: the planner and the apply's FsOps::stat both use it,
/// and the helper's own check must. Pure, so it is tested on every platform.
pub const fn is_link_reparse_tag(tag: u32) -> bool {
    (tag & 0x2000_0000) != 0
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EntryInfo {
    pub kind: EntryKind,
    /// True for a Windows reparse point (junction, symlink, mount point) and a POSIX symlink. The
    /// apply refuses to write through one: a parent that is a reparse point could send a write
    /// outside the install root.
    pub reparse_point: bool,
    /// Bytes, for a File.
    pub size: u64,
}

/// What rename_file does when Windows says 5, 32 or 33: try again every `interval` for up to
/// `total`, then fail with the last error. Never MOVEFILE_DELAY_UNTIL_REBOOT: a swap that
/// completes at a reboot is a swap nobody can roll back.
pub const RETRYABLE_OS_ERRORS: [i32; 3] = [5, 32, 33];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    pub interval: Duration,
    pub total: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(250),
            total: Duration::from_millis(30_000),
        }
    }
}

/// Holds the update lock while it lives (FsOps::try_lock): dropping it releases the lock.
pub trait FileLock {}

pub trait FsOps {
    // ---- looking ----------------------------------------------------------------------------

    /// Never follows a link or reparse point: it describes the entry itself. Missing is not an
    /// error.
    fn stat(&self, path: &Path) -> Result<EntryInfo>;

    /// The names directly inside `directory`, full paths, unsorted.
    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>>;

    /// The whole file.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// The path with every link resolved, existing prefix or not (std::filesystem's
    /// weakly_canonical). For "does it still resolve under the root".
    fn resolve(&self, path: &Path) -> Result<PathBuf>;

    /// Bytes free to an unprivileged user on the volume holding `path`.
    fn free_space(&self, path: &Path) -> Result<u64>;

    // ---- changing (counted by FaultFs) --------------------------------------------------------

    /// The directory and any missing parents; success when it exists.
    fn make_directories(&self, directory: &Path) -> Status;

    /// A new file `to` with the bytes of `from`, flushed to disk. Fails (AlreadyExists) when `to`
    /// exists: the apply copies into a fresh folder and must never overwrite by accident.
    fn copy_file(&self, from: &Path, to: &Path) -> Status;

    /// Moves `from` to `to` on one volume, atomically. Fails (AlreadyExists) when `to` exists;
    /// never replaces. Retries as RetryPolicy says.
    fn rename_file(&self, from: &Path, to: &Path) -> Status;

    /// Deletes a file; success when it is already gone.
    fn remove_file(&self, path: &Path) -> Status;

    /// Deletes a directory that is empty; success when it is already gone; fails (InvalidState)
    /// when it holds anything.
    fn remove_empty_directory(&self, path: &Path) -> Status;

    /// Appends and flushes to stable storage before returning (FlushFileBuffers, fsync): the
    /// journal's group commit. Creates the file when absent.
    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status;

    /// Writes the whole file (created or replaced) and flushes it: a record other code reads back
    /// (installed.json, last.json, state.json).
    fn write_durable(&self, path: &Path, bytes: &[u8]) -> Status;

    /// Sets the execute permission bits where the platform has them (POSIX); success doing
    /// nothing on Windows.
    fn set_executable(&self, path: &Path) -> Status;

    // ---- durability and exclusion (not counted) ----------------------------------------------

    /// Flushes a directory's entries to stable storage after renames (POSIX fsync of the
    /// directory); success doing nothing on Windows, where the rename's own flush is the
    /// guarantee.
    fn sync_directory(&self, directory: &Path) -> Status;

    /// The update lock, R/.update/lock: created when absent and held until the returned object is
    /// dropped or the process ends (Windows: a handle opened with share mode 0; POSIX: flock).
    /// InvalidState with the message "held" when another process holds it; any other failure is
    /// the OS's. Counted as nothing: an empty lock file changes no state worth crashing at.
    fn try_lock(&self, path: &Path) -> Result<Box<dyn FileLock>>;
}

/// The real file system: fsops_win.rs on Windows; fsops_posix.rs elsewhere, which refuses every
/// operation until the POSIX apply is written. The C++'s default argument is
/// `RetryPolicy::default()`.
pub fn make_system_fs_ops(retry: &RetryPolicy) -> Box<dyn FsOps> {
    #[cfg(windows)]
    {
        crate::fsops_win::make_system_fs_ops(retry)
    }
    #[cfg(not(windows))]
    {
        crate::fsops_posix::make_system_fs_ops(retry)
    }
}

// A path as an error context or a description shows it: UTF-8, and a byte that is not (a POSIX
// name is bytes) read as Windows-1252, as katana_core's file reader shows one. Never a panic: on
// Windows a name that is not Unicode is shown with replacement characters.
pub(crate) fn path_text(path: &Path) -> String {
    #[cfg(unix)]
    {
        katana_core::text_encoding::utf8_or_windows1252(katana_core::path_text::path_to_utf8(path))
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned()
    }
}
