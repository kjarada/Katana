//! The file system of the fsops module on Linux and macOS, which is NOT WRITTEN YET (task T18:
//! rename(2) plus fsync of the directory, flock, symlink detection). On macOS it will only ever be
//! compiled, never used to apply: a file changed inside Katana.app breaks the bundle's seal
//! (docs/update.md). Compiled everywhere except Windows.
//!
//! Until it is written every operation REFUSES, with one Error, and touches nothing: no file, no
//! folder, no lock. None answers "done" - a file system that said so of a rename it had not made
//! would let a swap go on over a tree it never changed - and none answers "missing" or "empty"
//! for a question it did not put to the disk.
//!
//! The CODE of the refusal, Unsupported, is part of what this file promises. The two places that
//! would otherwise work through this file system and report its refusal as a failure of the disk
//! ask it one question first, stat(), and on Unsupported say `reason=platform` instead: the stage
//! and preflight forms that use the system's file system (they would say write_failed, or
//! helper_missing), and the helper, before a swap or a recovery (the apply would say
//! root_not_writable, and write a last.json that said so). A file system that works never
//! answers stat() with Unsupported, so the day this file is written for LINUX those two questions
//! answer themselves and neither place needs an edit.
//!
//! On macOS neither place asks: both refuse there for good, whatever this file answers, because
//! this file is compiled for macOS too and what is written here for Linux works on a Mac as well.
//! Writing it must not be what turns downloads and swaps on inside a sealed bundle.

use crate::fsops::{EntryInfo, FileLock, FsOps, RetryPolicy};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use std::path::{Path, PathBuf};

// What a person reads where a download or an install was asked for here. It names no task and no
// file: the window shows it as it is.
const NOT_ON_THIS_SYSTEM: &str = "Katana does not install updates itself on this system: it \
                                  checks for a newer version, which is then downloaded from the \
                                  releases page and installed by hand";

// `os_error=0` first, as fsops asks of every failure: no system call stands behind this one.
fn refusal(operation: &str) -> Error {
    make_error(
        ErrorCode::Unsupported,
        NOT_ON_THIS_SYSTEM,
        format!("os_error=0 FsOps::{operation}"),
    )
}

struct NoFsOps;

impl FsOps for NoFsOps {
    fn stat(&self, _path: &Path) -> Result<EntryInfo> {
        Err(refusal("stat"))
    }
    fn list_directory(&self, _directory: &Path) -> Result<Vec<PathBuf>> {
        Err(refusal("listDirectory"))
    }
    fn read_file(&self, _path: &Path) -> Result<Vec<u8>> {
        Err(refusal("readFile"))
    }
    fn resolve(&self, _path: &Path) -> Result<PathBuf> {
        Err(refusal("resolve"))
    }
    fn free_space(&self, _path: &Path) -> Result<u64> {
        Err(refusal("freeSpace"))
    }
    fn make_directories(&self, _directory: &Path) -> Status {
        Err(refusal("makeDirectories"))
    }
    fn copy_file(&self, _from: &Path, _to: &Path) -> Status {
        Err(refusal("copyFile"))
    }
    fn rename_file(&self, _from: &Path, _to: &Path) -> Status {
        Err(refusal("renameFile"))
    }
    fn remove_file(&self, _path: &Path) -> Status {
        Err(refusal("removeFile"))
    }
    fn remove_empty_directory(&self, _path: &Path) -> Status {
        Err(refusal("removeEmptyDirectory"))
    }
    fn append_durable(&self, _path: &Path, _bytes: &[u8]) -> Status {
        Err(refusal("appendDurable"))
    }
    fn write_durable(&self, _path: &Path, _bytes: &[u8]) -> Status {
        Err(refusal("writeDurable"))
    }
    fn set_executable(&self, _path: &Path) -> Status {
        Err(refusal("setExecutable"))
    }
    fn sync_directory(&self, _directory: &Path) -> Status {
        Err(refusal("syncDirectory"))
    }
    fn try_lock(&self, _path: &Path) -> Result<Box<dyn FileLock>> {
        Err(refusal("tryLock"))
    }
}

pub(crate) fn make_system_fs_ops(_retry: &RetryPolicy) -> Box<dyn FsOps> {
    Box::new(NoFsOps)
}
