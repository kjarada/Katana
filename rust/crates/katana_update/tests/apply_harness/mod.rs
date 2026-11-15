//! What the swap's tests share (the twin of tests/update/apply_harness.hpp, for the twins of
//! test_stage.cpp, test_apply.cpp, test_recover.cpp, test_fault_loops.cpp and test_journal.cpp): a
//! scratch folder, a portable FsOps over std::fs, doubles for the other seams, releases and
//! installs built from specifications, and the whole-tree hash the fault loops compare.
//!
//! A test file that uses it declares, at its root, `pub mod test_key;` and then
//! `pub mod apply_harness;` (both `pub`, so that the parts a file does not use are no dead code):
//! the harness signs with the test keys of `crate::test_key`, and its macros name
//! `$crate::apply_harness`.
//!
//! WHERE THE EXPECTATIONS COME FROM (CLAUDE.md, section 4). The tree an update must leave is BUILT
//! HERE, from the specification of the release, in a separate folder, by writing each file the
//! specification lists - never read back from what the swap produced. The manifests are signed
//! here with the RFC 8032 test key, through ed25519-dalek's signing key - RFC 8032's deterministic
//! signature, so the very bytes OpenSSL gives the C++ harness for the same seed and message, and
//! made by the signer, not by the verifier under test; the T3 fixtures (tests/update/fixtures,
//! made by the Python producer and signed by `openssl`) stand beside them as a second,
//! independent source.
//!
//! [`StdFs`] is portable and does not flush: the swap's algorithm is the same on every platform
//! and the loops run it hundreds of times. The real Windows file system is tested separately and
//! the swap is run once through it.
//!
//! HOW THE C++'S CLASSES BECAME RUST. The seams are traits whose methods take `&self`, so a double
//! keeps what it records in a `Cell`, a `RefCell` or an atomic, and a test sets a hook through a
//! shared reference even while a Rig or the services hold the double ([`HookFs`],
//! [`FakeLauncher`], [`ScriptedProcesses`] have `set_*` methods; their fields are public too). The
//! C++'s ForwardingFs is subclassed by a test to override the one operation it breaks; Rust has no
//! subclasses, so [`ForwardingFs`] takes an override per operation as a closure, which is handed
//! the inner file system to pass the call on to. What crosses threads in the C++ tests - the
//! `release` flag and the `scans` count of [`FakeProcesses`] - is an `Arc` of an atomic, which a
//! test clones before it moves the Rig's work to another thread; a [`StdLock`] (the lock of a
//! [`StdFs`]) can be dropped on any thread ([`StdFs::try_lock_owned`]).

#![allow(
    dead_code,
    reason = "a shared support module: each test file uses part of it"
)]

use crate::test_key::{TEST_ACTIVE_PUBLIC_HEX, TEST_ACTIVE_SEED_HEX, test_trust};
use ed25519_dalek::{Signer, SigningKey};
use katana_core::error::make_error;
use katana_core::path_text::{environment_variable, path_from_utf8, path_to_utf8};
use katana_core::sha256::{Sha256, sha256_file_hex, sha256_hex};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::apply::{ApplyRequest, ApplyResult, ApplyServices, ApplyTimings, apply};
use katana_update::fault_fs::SimulatedCrash;
use katana_update::feed::StopToken;
use katana_update::fsops::{EntryInfo, EntryKind, FileLock, FsOps};
use katana_update::journal::{JournalState, parse_journal};
use katana_update::manifest::FileClass;
use katana_update::process::{Launcher, ProcessOps, RunningProgram, SpawnRequest, SpawnResult};
use katana_update::recover::recover_in_process;
use katana_update::registry::RegistryOps;
use katana_update::state::health_marker_path;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Write};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

// ---- text ------------------------------------------------------------------------------------

/// A path as the C++'s pathToUtf8 gives it, for a message or a key.
pub fn text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

// ---- a folder that goes away -----------------------------------------------------------------

pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        // The process's id too: the test binaries of one crate run side by side.
        let seed = format!(
            "{label}:{:?}:{}:{}",
            Instant::now(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            std::process::id()
        );
        let path = std::env::temp_dir().join(format!("katana-update-{}", &sha256_hex(seed)[..12]));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch folder can be made");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The C++'s `scratch / name`.
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The file, its folders made first; what cannot be written is not reported (as the C++'s
/// ofstream).
pub fn write_text(path: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, bytes.as_ref());
}

/// The whole file, or nothing when it cannot be read.
pub fn read_text(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

pub fn is_present(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn os_code(error: &std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(0)
}

// The system's own sentence for an error, as std::error_code::message() gives it: Rust's text
// without its " (os error N)".
fn os_message(error: &std::io::Error) -> String {
    let shown = format!("{error}");
    if let Some(code) = error.raw_os_error()
        && let Some(sentence) = shown.strip_suffix(&format!(" (os error {code})"))
    {
        return String::from(sentence);
    }
    shown
}

/// Windows says "access denied" or "sharing violation" for a moment about a file a virus scanner
/// is looking at - the real file system retries those (fsops.rs), and so does the one the loops
/// use, so that a scanner cannot make a loop of hundreds of iterations fail once.
pub fn transient(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(5 | 32 | 33))
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        false
    }
}

pub fn retrying(operation: impl Fn() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut last = Ok(());
    for _ in 0..150 {
        match operation() {
            Err(error) if transient(&error) => {
                last = Err(error);
                std::thread::sleep(Duration::from_millis(20));
            }
            done => return done,
        }
    }
    last
}

pub fn remove_all_with_retry(path: &Path) {
    for _ in 0..150 {
        let _ = remove_all(path);
        if !is_present(path) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// std::filesystem::remove_all: a folder with everything in it, a file, or a link (never what it
// points at); nothing there is no error.
fn remove_all(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

// Every entry under `from`, folders before what is in them, each as its path relative to `from`;
// a link is listed and not entered (std::filesystem's recursive_directory_iterator).
fn walk(from: &Path, relative: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from.join(relative))? {
        let entry = entry?;
        let below = relative.join(entry.file_name());
        let entered = entry.file_type()?.is_dir();
        out.push(below.clone());
        if entered {
            walk(from, &below, out)?;
        }
    }
    Ok(())
}

/// Makes `to` a copy of `from`, quickly, for a loop that does it hundreds of times. A file is
/// HARD-LINKED where it can be: the swap never changes a file in place - it moves, creates and
/// deletes - so a second name for the same bytes is as good as a copy and costs no data write (on
/// a machine whose virus scanner looks at every file written, the difference is a factor of
/// several). The exceptions are the files that ARE appended to in place, the journal and the
/// recovery log: those are copied. Where a link cannot be made (another volume, a file system
/// without them) the file is copied.
pub fn replace_tree(from: &Path, to: &Path) {
    remove_all_with_retry(to);
    std::fs::create_dir_all(to).unwrap_or_else(|error| panic!("{}: {error}", text(to)));
    let mut entries: Vec<PathBuf> = Vec::new();
    walk(from, Path::new(""), &mut entries)
        .unwrap_or_else(|error| panic!("{}: {error}", text(from)));
    for relative in &entries {
        let source = from.join(relative);
        let target = to.join(relative);
        let made = if std::fs::metadata(&source).is_ok_and(|metadata| metadata.is_dir()) {
            std::fs::create_dir_all(&target)
        } else {
            let name = relative.file_name().unwrap_or_default();
            let appended_to = name == "journal" || name == "recovery.log";
            let linked = !appended_to && std::fs::hard_link(&source, &target).is_ok();
            if linked {
                Ok(())
            } else {
                std::fs::copy(&source, &target).map(|_| ())
            }
        };
        if let Err(error) = made {
            panic!("{}: {error}", text(&source));
        }
    }
}

// ---- a file system over std::fs --------------------------------------------------------------

pub fn os_error(code: ErrorCode, message: impl Into<String>, path: &Path, os: i32) -> Error {
    make_error(code, message, format!("os_error={os} {}", text(path)))
}

// std::filesystem's lexically_normal, as the lock's key: "." dropped, a ".." taking the name
// before it.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(component),
            },
            _ => parts.push(component),
        }
    }
    parts.iter().collect()
}

// The locks held by any StdFs of this process: a lock is exclusive between holders, as the real
// one is between processes.
static HELD: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

fn held() -> std::sync::MutexGuard<'static, BTreeSet<String>> {
    HELD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The lock a [`StdFs`] hands out (the C++'s StdFs::Lock): let go when it is dropped, on any
/// thread.
#[derive(Debug)]
pub struct StdLock {
    key: String,
}

impl FileLock for StdLock {}

impl Drop for StdLock {
    fn drop(&mut self) {
        held().remove(&self.key);
    }
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
#[allow(
    unsafe_code,
    reason = "statvfs(3) is the free space std::filesystem::space reads; the standard library has \
              no form of it"
)]
mod volume {
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    // struct statvfs of glibc on a 64-bit Linux (bits/statvfs.h): eleven unsigned longs and six
    // ints of padding, 112 bytes.
    #[repr(C)]
    struct StatVfs {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: u64,
        f_flag: u64,
        f_namemax: u64,
        f_spare: [i32; 6],
    }
    const _: () = assert!(std::mem::size_of::<StatVfs>() == 112);

    unsafe extern "C" {
        fn statvfs(path: *const std::ffi::c_char, buffer: *mut StatVfs) -> i32;
    }

    /// What std::filesystem::space calls `available`: the blocks free to an unprivileged user
    /// times the fragment size.
    pub fn available(path: &Path) -> std::io::Result<u64> {
        let name = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let mut buffer = StatVfs {
            f_bsize: 0,
            f_frsize: 0,
            f_blocks: 0,
            f_bfree: 0,
            f_bavail: 0,
            f_files: 0,
            f_ffree: 0,
            f_favail: 0,
            f_fsid: 0,
            f_flag: 0,
            f_namemax: 0,
            f_spare: [0; 6],
        };
        // SAFETY: `name` is a NUL-terminated string that outlives the call, and `buffer` is a
        // struct statvfs of the size the system writes (checked above).
        if unsafe { statvfs(name.as_ptr(), &mut buffer) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(buffer.f_bavail.saturating_mul(buffer.f_frsize))
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetDiskFreeSpaceExW is the free space std::filesystem::space reads; the standard \
              library has no form of it"
)]
mod volume {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    pub fn available(path: &Path) -> std::io::Result<u64> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut free_to_caller = 0u64;
        // SAFETY: `wide` is NUL-terminated and outlives the call; the one out-pointer is a u64 of
        // ours, and the two the call may be given null for are null.
        let done = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_to_caller,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if done == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(free_to_caller)
    }
}

#[cfg(not(any(windows, all(target_os = "linux", target_pointer_width = "64"))))]
mod volume {
    use std::path::Path;

    // No system call is declared for this system yet: the free space is not known, which the
    // stage passes over and the swap refuses (io_error), as both do for a figure that cannot be
    // read.
    pub fn available(_path: &Path) -> std::io::Result<u64> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

// std::filesystem::weakly_canonical: the longest leading part that exists, with every link
// resolved, and the rest after it, lexically normal.
fn weakly_canonical(path: &Path) -> std::io::Result<PathBuf> {
    let mut head = path.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(resolved) = std::fs::canonicalize(&head) {
            let mut out = without_verbatim(resolved);
            for name in rest.iter().rev() {
                out.push(name);
            }
            return Ok(lexically_normal(&out));
        }
        match (head.file_name(), head.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                head = parent.to_path_buf();
            }
            _ => return Ok(lexically_normal(path)),
        }
    }
}

// Windows' canonical form carries the \\?\ prefix, which std::filesystem's does not.
fn without_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let shown = text(&path);
        if let Some(rest) = shown.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = shown.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
        path
    }
    #[cfg(not(windows))]
    {
        path
    }
}

/// A file system over std::fs, as the C++'s StdFs is over std::filesystem. A unit struct: what it
/// holds - the locks - is the process's, so `&StdFs` is a 'static reference wherever it is
/// written.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFs;

impl StdFs {
    /// try_lock with the lock's own type, which can be dropped on another thread.
    pub fn try_lock_owned(&self, path: &Path) -> Result<StdLock> {
        let key = text(&lexically_normal(path));
        {
            let mut keys = held();
            if !keys.insert(key.clone()) {
                return Err(make_error(
                    ErrorCode::InvalidState,
                    "held",
                    format!("os_error=32 {key}"),
                ));
            }
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path);
        Ok(StdLock { key })
    }
}

impl FsOps for StdFs {
    fn stat(&self, path: &Path) -> Result<EntryInfo> {
        let link = match std::fs::symlink_metadata(path) {
            Ok(link) => link,
            Err(error)
                if error.kind() == ErrorKind::NotFound
                    || error.kind() == ErrorKind::NotADirectory =>
            {
                return Ok(EntryInfo::default());
            }
            Err(error) => {
                return Err(os_error(
                    ErrorCode::FileImportFailure,
                    os_message(&error),
                    path,
                    os_code(&error),
                ));
            }
        };
        let mut info = EntryInfo {
            reparse_point: link.file_type().is_symlink(),
            ..EntryInfo::default()
        };
        match std::fs::metadata(path) {
            Ok(real) if real.is_dir() => info.kind = EntryKind::Directory,
            Ok(real) if real.is_file() => {
                info.kind = EntryKind::File;
                info.size = real.len();
            }
            _ => {
                info.kind = if info.reparse_point {
                    EntryKind::File
                } else {
                    EntryKind::Other
                };
            }
        }
        Ok(info)
    }

    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        let listed = std::fs::read_dir(directory).and_then(|entries| {
            entries
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<PathBuf>>>()
        });
        listed.map_err(|error| {
            os_error(
                ErrorCode::NotFound,
                os_message(&error),
                directory,
                os_code(&error),
            )
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(os_error(ErrorCode::NotFound, "no such file", path, 2));
        }
        Ok(read_text(path))
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        weakly_canonical(path).map_err(|error| {
            os_error(
                ErrorCode::FileImportFailure,
                os_message(&error),
                path,
                os_code(&error),
            )
        })
    }

    fn free_space(&self, path: &Path) -> Result<u64> {
        let mut probe = path.to_path_buf();
        while !probe.as_os_str().is_empty() && !probe.exists() {
            probe = probe.parent().map(Path::to_path_buf).unwrap_or_default();
        }
        volume::available(&probe).map_err(|error| {
            os_error(
                ErrorCode::FileImportFailure,
                os_message(&error),
                path,
                os_code(&error),
            )
        })
    }

    fn make_directories(&self, directory: &Path) -> Status {
        std::fs::create_dir_all(directory).map_err(|error| {
            os_error(
                ErrorCode::FileExportFailure,
                os_message(&error),
                directory,
                os_code(&error),
            )
        })
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Status {
        if is_present(to) {
            return Err(os_error(
                ErrorCode::AlreadyExists,
                "the target exists",
                to,
                80,
            ));
        }
        retrying(|| std::fs::copy(from, to).map(|_| ())).map_err(|error| {
            os_error(
                ErrorCode::FileExportFailure,
                os_message(&error),
                to,
                os_code(&error),
            )
        })
    }

    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        if is_present(to) {
            return Err(os_error(
                ErrorCode::AlreadyExists,
                "the target exists",
                to,
                183,
            ));
        }
        retrying(|| std::fs::rename(from, to)).map_err(|error| {
            os_error(
                ErrorCode::FileExportFailure,
                os_message(&error),
                from,
                os_code(&error),
            )
        })
    }

    fn remove_file(&self, path: &Path) -> Status {
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(os_error(
                ErrorCode::FileExportFailure,
                "a directory",
                path,
                5,
            ));
        }
        retrying(|| match std::fs::remove_file(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            done => done,
        })
        .map_err(|error| {
            os_error(
                ErrorCode::FileExportFailure,
                os_message(&error),
                path,
                os_code(&error),
            )
        })
    }

    fn remove_empty_directory(&self, path: &Path) -> Status {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return Ok(());
        };
        if !metadata.is_dir() {
            return Err(os_error(
                ErrorCode::InvalidState,
                "not a directory",
                path,
                267,
            ));
        }
        let empty = std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none());
        if !empty {
            return Err(os_error(
                ErrorCode::InvalidState,
                "the folder is not empty",
                path,
                145,
            ));
        }
        std::fs::remove_dir(path).map_err(|error| {
            os_error(
                ErrorCode::FileExportFailure,
                os_message(&error),
                path,
                os_code(&error),
            )
        })
    }

    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        let written = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .and_then(|mut file| file.write_all(bytes));
        written.map_err(|_| os_error(ErrorCode::FileExportFailure, "write failed", path, 0))
    }

    fn write_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        std::fs::write(path, bytes)
            .map_err(|_| os_error(ErrorCode::FileExportFailure, "write failed", path, 0))
    }

    fn set_executable(&self, _path: &Path) -> Status {
        Ok(())
    }

    fn sync_directory(&self, _directory: &Path) -> Status {
        Ok(())
    }

    fn try_lock(&self, path: &Path) -> Result<Box<dyn FileLock>> {
        let lock: Box<dyn FileLock> = Box::new(self.try_lock_owned(path)?);
        Ok(lock)
    }
}

// ---- passing a file system on, and breaking one operation of it ------------------------------

/// An override of a ForwardingFs operation that names one path: handed the inner file system, to
/// pass the call on to.
pub type PathOverride<'a, T> = Box<dyn Fn(&dyn FsOps, &Path) -> T + 'a>;
/// An override of an operation that names two paths (copy, rename).
pub type PairOverride<'a> = Box<dyn Fn(&dyn FsOps, &Path, &Path) -> Status + 'a>;
/// An override of an operation that writes bytes (append, write).
pub type BytesOverride<'a> = Box<dyn Fn(&dyn FsOps, &Path, &[u8]) -> Status + 'a>;
/// An override of try_lock.
pub type LockOverride<'a> = PathOverride<'a, Result<Box<dyn FileLock>>>;

/// A FsOps that passes everything to another, for a test to override the one operation it wants
/// to break (the C++'s ForwardingFs, which a test subclasses): each `on_*` method replaces one
/// operation by a closure, which is handed the inner file system and the arguments. The overrides
/// are kept in RefCells, so that one can be set, or replaced, while a Rig or the services hold
/// this file system.
pub struct ForwardingFs<'a> {
    inner: &'a dyn FsOps,
    pub stat: RefCell<Option<PathOverride<'a, Result<EntryInfo>>>>,
    pub list_directory: RefCell<Option<PathOverride<'a, Result<Vec<PathBuf>>>>>,
    pub read_file: RefCell<Option<PathOverride<'a, Result<Vec<u8>>>>>,
    pub resolve: RefCell<Option<PathOverride<'a, Result<PathBuf>>>>,
    pub free_space: RefCell<Option<PathOverride<'a, Result<u64>>>>,
    pub make_directories: RefCell<Option<PathOverride<'a, Status>>>,
    pub copy_file: RefCell<Option<PairOverride<'a>>>,
    pub rename_file: RefCell<Option<PairOverride<'a>>>,
    pub remove_file: RefCell<Option<PathOverride<'a, Status>>>,
    pub remove_empty_directory: RefCell<Option<PathOverride<'a, Status>>>,
    pub append_durable: RefCell<Option<BytesOverride<'a>>>,
    pub write_durable: RefCell<Option<BytesOverride<'a>>>,
    pub set_executable: RefCell<Option<PathOverride<'a, Status>>>,
    pub sync_directory: RefCell<Option<PathOverride<'a, Status>>>,
    pub try_lock: RefCell<Option<LockOverride<'a>>>,
}

impl<'a> ForwardingFs<'a> {
    pub fn new(inner: &'a dyn FsOps) -> Self {
        Self {
            inner,
            stat: RefCell::new(None),
            list_directory: RefCell::new(None),
            read_file: RefCell::new(None),
            resolve: RefCell::new(None),
            free_space: RefCell::new(None),
            make_directories: RefCell::new(None),
            copy_file: RefCell::new(None),
            rename_file: RefCell::new(None),
            remove_file: RefCell::new(None),
            remove_empty_directory: RefCell::new(None),
            append_durable: RefCell::new(None),
            write_durable: RefCell::new(None),
            set_executable: RefCell::new(None),
            sync_directory: RefCell::new(None),
            try_lock: RefCell::new(None),
        }
    }

    /// The file system everything is passed on to.
    pub fn inner(&self) -> &'a dyn FsOps {
        self.inner
    }

    pub fn on_stat(&self, f: impl Fn(&dyn FsOps, &Path) -> Result<EntryInfo> + 'a) {
        *self.stat.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_list_directory(&self, f: impl Fn(&dyn FsOps, &Path) -> Result<Vec<PathBuf>> + 'a) {
        *self.list_directory.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_read_file(&self, f: impl Fn(&dyn FsOps, &Path) -> Result<Vec<u8>> + 'a) {
        *self.read_file.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_resolve(&self, f: impl Fn(&dyn FsOps, &Path) -> Result<PathBuf> + 'a) {
        *self.resolve.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_free_space(&self, f: impl Fn(&dyn FsOps, &Path) -> Result<u64> + 'a) {
        *self.free_space.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_make_directories(&self, f: impl Fn(&dyn FsOps, &Path) -> Status + 'a) {
        *self.make_directories.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_copy_file(&self, f: impl Fn(&dyn FsOps, &Path, &Path) -> Status + 'a) {
        *self.copy_file.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_rename_file(&self, f: impl Fn(&dyn FsOps, &Path, &Path) -> Status + 'a) {
        *self.rename_file.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_remove_file(&self, f: impl Fn(&dyn FsOps, &Path) -> Status + 'a) {
        *self.remove_file.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_remove_empty_directory(&self, f: impl Fn(&dyn FsOps, &Path) -> Status + 'a) {
        *self.remove_empty_directory.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_append_durable(&self, f: impl Fn(&dyn FsOps, &Path, &[u8]) -> Status + 'a) {
        *self.append_durable.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_write_durable(&self, f: impl Fn(&dyn FsOps, &Path, &[u8]) -> Status + 'a) {
        *self.write_durable.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_set_executable(&self, f: impl Fn(&dyn FsOps, &Path) -> Status + 'a) {
        *self.set_executable.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_sync_directory(&self, f: impl Fn(&dyn FsOps, &Path) -> Status + 'a) {
        *self.sync_directory.borrow_mut() = Some(Box::new(f));
    }
    pub fn on_try_lock(&self, f: impl Fn(&dyn FsOps, &Path) -> Result<Box<dyn FileLock>> + 'a) {
        *self.try_lock.borrow_mut() = Some(Box::new(f));
    }
}

impl FsOps for ForwardingFs<'_> {
    fn stat(&self, path: &Path) -> Result<EntryInfo> {
        match self.stat.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.stat(path),
        }
    }
    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        match self.list_directory.borrow().as_ref() {
            Some(f) => f(self.inner, directory),
            None => self.inner.list_directory(directory),
        }
    }
    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        match self.read_file.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.read_file(path),
        }
    }
    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        match self.resolve.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.resolve(path),
        }
    }
    fn free_space(&self, path: &Path) -> Result<u64> {
        match self.free_space.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.free_space(path),
        }
    }
    fn make_directories(&self, directory: &Path) -> Status {
        match self.make_directories.borrow().as_ref() {
            Some(f) => f(self.inner, directory),
            None => self.inner.make_directories(directory),
        }
    }
    fn copy_file(&self, from: &Path, to: &Path) -> Status {
        match self.copy_file.borrow().as_ref() {
            Some(f) => f(self.inner, from, to),
            None => self.inner.copy_file(from, to),
        }
    }
    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        match self.rename_file.borrow().as_ref() {
            Some(f) => f(self.inner, from, to),
            None => self.inner.rename_file(from, to),
        }
    }
    fn remove_file(&self, path: &Path) -> Status {
        match self.remove_file.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.remove_file(path),
        }
    }
    fn remove_empty_directory(&self, path: &Path) -> Status {
        match self.remove_empty_directory.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.remove_empty_directory(path),
        }
    }
    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        match self.append_durable.borrow().as_ref() {
            Some(f) => f(self.inner, path, bytes),
            None => self.inner.append_durable(path, bytes),
        }
    }
    fn write_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        match self.write_durable.borrow().as_ref() {
            Some(f) => f(self.inner, path, bytes),
            None => self.inner.write_durable(path, bytes),
        }
    }
    fn set_executable(&self, path: &Path) -> Status {
        match self.set_executable.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.set_executable(path),
        }
    }
    fn sync_directory(&self, directory: &Path) -> Status {
        match self.sync_directory.borrow().as_ref() {
            Some(f) => f(self.inner, directory),
            None => self.inner.sync_directory(directory),
        }
    }
    fn try_lock(&self, path: &Path) -> Result<Box<dyn FileLock>> {
        match self.try_lock.borrow().as_ref() {
            Some(f) => f(self.inner, path),
            None => self.inner.try_lock(path),
        }
    }
}

// The operations a wrapper passes on as they are, written once: `$inner` is the field that holds
// the inner file system.
macro_rules! pass_on {
    ($inner:ident; $($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(
            fn $name(&self, $($arg: $ty),*) -> $ret {
                self.$inner.$name($($arg),*)
            }
        )*
    };
}

/// Counts the renames it passes on.
pub struct CountingFs<'a> {
    inner: &'a dyn FsOps,
    pub renames: Cell<u64>,
}

impl<'a> CountingFs<'a> {
    pub fn new(inner: &'a dyn FsOps) -> Self {
        Self {
            inner,
            renames: Cell::new(0),
        }
    }
}

impl FsOps for CountingFs<'_> {
    pass_on! { inner;
        stat(path: &Path) -> Result<EntryInfo>;
        list_directory(directory: &Path) -> Result<Vec<PathBuf>>;
        read_file(path: &Path) -> Result<Vec<u8>>;
        resolve(path: &Path) -> Result<PathBuf>;
        free_space(path: &Path) -> Result<u64>;
        make_directories(directory: &Path) -> Status;
        copy_file(from: &Path, to: &Path) -> Status;
        remove_file(path: &Path) -> Status;
        remove_empty_directory(path: &Path) -> Status;
        append_durable(path: &Path, bytes: &[u8]) -> Status;
        write_durable(path: &Path, bytes: &[u8]) -> Status;
        set_executable(path: &Path) -> Status;
        sync_directory(directory: &Path) -> Status;
        try_lock(path: &Path) -> Result<Box<dyn FileLock>>;
    }

    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        self.renames.set(self.renames.get() + 1);
        self.inner.rename_file(from, to)
    }
}

/// A hook that sees a copy or a rename before it is passed on, and may answer for it.
pub type PairHook<'a> = Box<dyn Fn(&Path, &Path) -> Option<Status> + 'a>;
/// A hook that sees a copy after it was made.
pub type AfterPairHook<'a> = Box<dyn Fn(&Path, &Path) + 'a>;
/// A hook that sees an append before it is passed on, and may answer for it.
pub type AppendHook<'a> = Box<dyn Fn(&Path, &[u8]) -> Option<Status> + 'a>;
/// A hook that changes what stat reports.
pub type StatHook<'a> = Box<dyn Fn(&Path, &mut EntryInfo) + 'a>;
/// A hook that answers resolve for the paths it names.
pub type ResolveHook<'a> = Box<dyn Fn(&Path) -> Option<PathBuf> + 'a>;

/// A FsOps whose operations a test can intercept: each hook sees the call before it is passed on,
/// and may answer for it (a Status other than "go on") or change what it reports. The hooks are
/// RefCells and `free_bytes` a Cell, so that a test sets them with the `set_*` methods (or the
/// fields) while a Rig holds this file system, and takes one away by setting its field to None.
pub struct HookFs<'a> {
    inner: &'a dyn FsOps,
    /// Returning a value answers the call instead of passing it on.
    pub before_copy: RefCell<Option<PairHook<'a>>>,
    /// After the copy was made.
    pub after_copy: RefCell<Option<AfterPairHook<'a>>>,
    pub before_rename: RefCell<Option<PairHook<'a>>>,
    pub before_append: RefCell<Option<AppendHook<'a>>>,
    pub adjust_stat: RefCell<Option<StatHook<'a>>>,
    pub override_resolve: RefCell<Option<ResolveHook<'a>>>,
    pub free_bytes: Cell<Option<u64>>,
}

impl<'a> HookFs<'a> {
    pub fn new(inner: &'a dyn FsOps) -> Self {
        Self {
            inner,
            before_copy: RefCell::new(None),
            after_copy: RefCell::new(None),
            before_rename: RefCell::new(None),
            before_append: RefCell::new(None),
            adjust_stat: RefCell::new(None),
            override_resolve: RefCell::new(None),
            free_bytes: Cell::new(None),
        }
    }

    pub fn set_before_copy(&self, hook: impl Fn(&Path, &Path) -> Option<Status> + 'a) {
        *self.before_copy.borrow_mut() = Some(Box::new(hook));
    }
    pub fn set_after_copy(&self, hook: impl Fn(&Path, &Path) + 'a) {
        *self.after_copy.borrow_mut() = Some(Box::new(hook));
    }
    pub fn set_before_rename(&self, hook: impl Fn(&Path, &Path) -> Option<Status> + 'a) {
        *self.before_rename.borrow_mut() = Some(Box::new(hook));
    }
    pub fn set_before_append(&self, hook: impl Fn(&Path, &[u8]) -> Option<Status> + 'a) {
        *self.before_append.borrow_mut() = Some(Box::new(hook));
    }
    pub fn set_adjust_stat(&self, hook: impl Fn(&Path, &mut EntryInfo) + 'a) {
        *self.adjust_stat.borrow_mut() = Some(Box::new(hook));
    }
    pub fn set_override_resolve(&self, hook: impl Fn(&Path) -> Option<PathBuf> + 'a) {
        *self.override_resolve.borrow_mut() = Some(Box::new(hook));
    }
}

impl FsOps for HookFs<'_> {
    pass_on! { inner;
        list_directory(directory: &Path) -> Result<Vec<PathBuf>>;
        read_file(path: &Path) -> Result<Vec<u8>>;
        make_directories(directory: &Path) -> Status;
        remove_file(path: &Path) -> Status;
        remove_empty_directory(path: &Path) -> Status;
        write_durable(path: &Path, bytes: &[u8]) -> Status;
        set_executable(path: &Path) -> Status;
        sync_directory(directory: &Path) -> Status;
        try_lock(path: &Path) -> Result<Box<dyn FileLock>>;
    }

    fn stat(&self, path: &Path) -> Result<EntryInfo> {
        let mut info = self.inner.stat(path)?;
        if let Some(adjust) = self.adjust_stat.borrow().as_ref() {
            adjust(path, &mut info);
        }
        Ok(info)
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        if let Some(other) = self
            .override_resolve
            .borrow()
            .as_ref()
            .and_then(|hook| hook(path))
        {
            return Ok(other);
        }
        self.inner.resolve(path)
    }

    fn free_space(&self, path: &Path) -> Result<u64> {
        match self.free_bytes.get() {
            Some(bytes) => Ok(bytes),
            None => self.inner.free_space(path),
        }
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Status {
        if let Some(answer) = self
            .before_copy
            .borrow()
            .as_ref()
            .and_then(|hook| hook(from, to))
        {
            return answer;
        }
        let done = self.inner.copy_file(from, to);
        if let Some(after) = self.after_copy.borrow().as_ref() {
            after(from, to);
        }
        done
    }

    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        if let Some(answer) = self
            .before_rename
            .borrow()
            .as_ref()
            .and_then(|hook| hook(from, to))
        {
            return answer;
        }
        self.inner.rename_file(from, to)
    }

    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        if let Some(answer) = self
            .before_append
            .borrow()
            .as_ref()
            .and_then(|hook| hook(path, bytes))
        {
            return answer;
        }
        self.inner.append_durable(path, bytes)
    }
}

/// A failure as the file system reports one: FileExportFailure, `os_error=<n> <path>`.
pub fn injected(os_error_number: i32, path: &Path) -> Status {
    Err(make_error(
        ErrorCode::FileExportFailure,
        "injected",
        format!("os_error={os_error_number} {}", text(path)),
    ))
}

/// Writes down every MUTATING operation it passes on, in the order FaultFs counts them (the eight
/// of fsops.rs), so that "the k-th operation" of a fault loop can be named: put it above the
/// FaultFs and the log has an entry for every operation the FaultFs counted, the one it crashed
/// before included.
pub struct LogFs<'a> {
    inner: &'a dyn FsOps,
    pub log: RefCell<Vec<String>>,
}

impl<'a> LogFs<'a> {
    pub fn new(inner: &'a dyn FsOps) -> Self {
        Self {
            inner,
            log: RefCell::new(Vec::new()),
        }
    }

    fn write(&self, entry: String) {
        self.log.borrow_mut().push(entry);
    }

    /// The 1-based number of the first entry at or after `from` (1-based) that begins with
    /// `prefix` and contains `needle`; 0 when there is none. The C++'s defaults are an empty
    /// needle and 1.
    pub fn find(&self, prefix: &str, needle: &str, from: u64) -> u64 {
        let log = self.log.borrow();
        for i in from.max(1)..=log.len() as u64 {
            let entry = &log[(i - 1) as usize];
            if entry.starts_with(prefix) && entry.contains(needle) {
                return i;
            }
        }
        0
    }

    pub fn all(&self, prefix: &str) -> Vec<u64> {
        let log = self.log.borrow();
        (1..=log.len() as u64)
            .filter(|&i| log[(i - 1) as usize].starts_with(prefix))
            .collect()
    }
}

impl FsOps for LogFs<'_> {
    pass_on! { inner;
        stat(path: &Path) -> Result<EntryInfo>;
        list_directory(directory: &Path) -> Result<Vec<PathBuf>>;
        read_file(path: &Path) -> Result<Vec<u8>>;
        resolve(path: &Path) -> Result<PathBuf>;
        free_space(path: &Path) -> Result<u64>;
        sync_directory(directory: &Path) -> Status;
        try_lock(path: &Path) -> Result<Box<dyn FileLock>>;
    }

    fn make_directories(&self, directory: &Path) -> Status {
        self.write(format!("makeDirectories {}", text(directory)));
        self.inner.make_directories(directory)
    }
    fn copy_file(&self, from: &Path, to: &Path) -> Status {
        self.write(format!("copyFile {} -> {}", text(from), text(to)));
        self.inner.copy_file(from, to)
    }
    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        self.write(format!("renameFile {} -> {}", text(from), text(to)));
        self.inner.rename_file(from, to)
    }
    fn remove_file(&self, path: &Path) -> Status {
        self.write(format!("removeFile {}", text(path)));
        self.inner.remove_file(path)
    }
    fn remove_empty_directory(&self, path: &Path) -> Status {
        self.write(format!("removeEmptyDirectory {}", text(path)));
        self.inner.remove_empty_directory(path)
    }
    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        self.write(format!(
            "appendDurable {} {}",
            text(path),
            String::from_utf8_lossy(bytes)
        ));
        self.inner.append_durable(path, bytes)
    }
    fn write_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        self.write(format!(
            "writeDurable {} {}",
            text(path),
            String::from_utf8_lossy(bytes)
        ));
        self.inner.write_durable(path, bytes)
    }
    fn set_executable(&self, path: &Path) -> Status {
        self.write(format!("setExecutable {}", text(path)));
        self.inner.set_executable(path)
    }
}

/// Where the journal of the (single) transaction under R/.update stands; Empty when there is no
/// folder or no complete record.
pub fn journal_state_of(root: &Path) -> JournalState {
    let Ok(entries) = std::fs::read_dir(root.join(".update")) else {
        return JournalState::Empty;
    };
    for entry in entries.flatten() {
        if std::fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir()) {
            let journal = entry.path().join("journal");
            if is_present(&journal) {
                return parse_journal(read_text(&journal)).state;
            }
        }
    }
    JournalState::Empty
}

// ---- the other seams ---------------------------------------------------------------------------

/// The process seam a Rig runs with.
pub struct FakeProcesses {
    pub pid: u64,
    /// The parent (the window): alive for this many is_alive() questions, then gone. -1: forever.
    pub parent_alive_for: Cell<i32>,
    /// Who holds files under the root: for this many holders_under() questions, then nobody. -1:
    /// forever.
    pub holders_for: i32,
    pub holders: Vec<RunningProgram>,
    /// Set to let everyone go at once, whatever holders_for says: a test that needs the helper to
    /// stay waiting until another thread has done its part holds with holders_for = -1 and
    /// releases here (an Arc, so that the other thread holds it too).
    pub release: Arc<AtomicBool>,
    pub scan_fails: bool,
    pub scans: Arc<AtomicI32>,
}

impl Default for FakeProcesses {
    fn default() -> Self {
        Self {
            pid: 1,
            parent_alive_for: Cell::new(0),
            holders_for: 0,
            holders: Vec::new(),
            release: Arc::new(AtomicBool::new(false)),
            scan_fails: false,
            scans: Arc::new(AtomicI32::new(0)),
        }
    }
}

impl ProcessOps for FakeProcesses {
    fn current_pid(&self) -> u64 {
        self.pid
    }

    fn is_alive(&self, _pid: u64) -> bool {
        let left = self.parent_alive_for.get();
        if left < 0 {
            return true;
        }
        if left == 0 {
            return false;
        }
        self.parent_alive_for.set(left - 1);
        true
    }

    fn holders_under(&self, _root: &Path) -> Result<Vec<RunningProgram>> {
        if self.scan_fails {
            return Err(Error::new(ErrorCode::Internal, "the snapshot failed"));
        }
        let seen = self.scans.fetch_add(1, Ordering::SeqCst) + 1;
        if !self.release.load(Ordering::SeqCst)
            && (self.holders_for < 0 || seen <= self.holders_for)
        {
            return Ok(self.holders.clone());
        }
        Ok(Vec::new())
    }
}

/// What one question of a [`ScriptedProcesses`] is answered with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// Empty: nobody is running.
    pub holders: Vec<RunningProgram>,
    /// The scan itself fails (no answer at all).
    pub fails: bool,
}

/// A hook a ScriptedProcesses calls at each question, with its number.
pub type ScanHook<'a> = Box<dyn Fn(i32) + 'a>;

/// A process seam that answers each question about the programs under the root from a list.
/// FakeProcesses above can say "these programs, for the first n questions, and then nobody"; the
/// opposite - nobody at first, and somebody at a LATER question - is what a program started while
/// the helper works looks like, and this gives each question its own answer: the swap's two looks
/// and the two a recovery takes before it puts an update back.
pub struct ScriptedProcesses<'a> {
    answers: Vec<Answer>,
    pub pid: u64,
    /// Called at each question with its number, before it is answered: where a test looks at the
    /// disk as it stands at that moment, or notes how far the work had got.
    pub on_scan: RefCell<Option<ScanHook<'a>>>,
    pub scans: Cell<i32>,
}

impl<'a> ScriptedProcesses<'a> {
    /// The k-th question (from 1) gets the k-th answer; past the list nobody is running.
    pub fn new(answers: Vec<Answer>) -> Self {
        Self {
            answers,
            pid: 1,
            on_scan: RefCell::new(None),
            scans: Cell::new(0),
        }
    }

    pub fn set_on_scan(&self, hook: impl Fn(i32) + 'a) {
        *self.on_scan.borrow_mut() = Some(Box::new(hook));
    }
}

impl ProcessOps for ScriptedProcesses<'_> {
    fn current_pid(&self) -> u64 {
        self.pid
    }

    // No window is waited for in the cases that use this seam (wait_pid is 0).
    fn is_alive(&self, _pid: u64) -> bool {
        false
    }

    fn holders_under(&self, _root: &Path) -> Result<Vec<RunningProgram>> {
        let asked = self.scans.get() + 1;
        self.scans.set(asked);
        if let Some(hook) = self.on_scan.borrow().as_ref() {
            hook(asked);
        }
        let Some(answer) = self.answers.get((asked - 1) as usize) else {
            return Ok(Vec::new());
        };
        if answer.fails {
            return Err(Error::new(ErrorCode::Internal, "the snapshot failed"));
        }
        Ok(answer.holders.clone())
    }
}

/// The three answers a question can get: nobody is running.
pub fn nobody() -> Answer {
    Answer::default()
}

/// A program is running.
pub fn somebody(pid: u64, image: &str) -> Answer {
    Answer {
        holders: vec![RunningProgram {
            pid,
            image: path_from_utf8(image),
        }],
        fails: false,
    }
}

/// The scan fails.
pub fn no_answer() -> Answer {
    Answer {
        holders: Vec::new(),
        fails: true,
    }
}

/// What a FakeLauncher answers a run with.
pub type RunHook<'a> = Box<dyn Fn(&SpawnRequest) -> Result<SpawnResult> + 'a>;

/// A launcher that records what it was asked to run and to start, and runs nothing.
#[derive(Default)]
pub struct FakeLauncher<'a> {
    pub ran: RefCell<Vec<SpawnRequest>>,
    pub started: RefCell<Vec<SpawnRequest>>,
    pub on_run: RefCell<Option<RunHook<'a>>>,
}

impl<'a> FakeLauncher<'a> {
    pub fn set_on_run(&self, hook: impl Fn(&SpawnRequest) -> Result<SpawnResult> + 'a) {
        *self.on_run.borrow_mut() = Some(Box::new(hook));
    }

    /// What it was asked to run to its end, in order.
    pub fn ran(&self) -> Vec<SpawnRequest> {
        self.ran.borrow().clone()
    }

    /// What it was asked to start detached, in order.
    pub fn started(&self) -> Vec<SpawnRequest> {
        self.started.borrow().clone()
    }
}

impl Launcher for FakeLauncher<'_> {
    fn run(&self, request: &SpawnRequest) -> Result<SpawnResult> {
        self.ran.borrow_mut().push(request.clone());
        if let Some(hook) = self.on_run.borrow().as_ref() {
            return hook(request);
        }
        Ok(SpawnResult::default())
    }

    fn start_detached(&self, request: &SpawnRequest) -> Status {
        self.started.borrow_mut().push(request.clone());
        Ok(())
    }
}

/// A registry that keeps what it is told.
#[derive(Debug, Default)]
pub struct FakeRegistry {
    pub location: Option<PathBuf>,
    pub run_once: RefCell<BTreeMap<String, String>>,
    /// Every command ever registered.
    pub run_once_history: RefCell<Vec<String>>,
    pub run_once_cleared: Cell<i32>,
    pub versions: RefCell<Vec<(String, String)>>,
}

impl RegistryOps for FakeRegistry {
    fn installer_location(&self) -> Result<Option<PathBuf>> {
        Ok(self.location.clone())
    }

    fn set_run_once(&self, name: &str, command: &str) -> Status {
        self.run_once
            .borrow_mut()
            .insert(String::from(name), String::from(command));
        self.run_once_history
            .borrow_mut()
            .push(String::from(command));
        Ok(())
    }

    fn clear_run_once(&self, name: &str) -> Status {
        self.run_once.borrow_mut().remove(name);
        self.run_once_cleared.set(self.run_once_cleared.get() + 1);
        Ok(())
    }

    fn set_installed_version(&self, display_version: &str, display_name: &str) -> Status {
        self.versions
            .borrow_mut()
            .push((String::from(display_version), String::from(display_name)));
        Ok(())
    }
}

// ---- signing (ed25519-dalek's signer, not the verifier under test) ------------------------------

pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let at = |i: usize| chunk.get(i).map_or(0, |&byte| u32::from(byte));
        let triple = (at(0) << 16) | (at(1) << 8) | at(2);
        let digit = |shift: u32| char::from(ALPHABET[((triple >> shift) & 63) as usize]);
        out.push(digit(18));
        out.push(digit(12));
        out.push(if chunk.len() > 1 { digit(6) } else { '=' });
        out.push(if chunk.len() > 2 { digit(0) } else { '=' });
    }
    out
}

/// Whether the test manifests can be signed. The C++ needs OpenSSL's headers for it; the port's
/// signer is a dependency of the crate, so it always can - kept, with the macro below, for the
/// tests that ask.
pub fn can_sign() -> bool {
    true
}

/// True where a continuous-integration service runs the tests (it sets CI, and GitHub also
/// GITHUB_ACTIONS).
pub fn in_continuous_integration() -> bool {
    !environment_variable(Some("CI")).is_empty()
        || !environment_variable(Some("GITHUB_ACTIONS")).is_empty()
}

/// At the top of a test that signs. Where the test manifests cannot be signed: skipped, and said
/// so, on a developer's machine - but a FAILURE in continuous integration, because the 40-file
/// crash loop, the recovery loop and the rename-failure loop are among the tests that sign, and a
/// skip count cannot be told from three placeholders: the central tests would stop running and
/// nobody would see it.
#[macro_export]
macro_rules! require_can_sign {
    () => {
        if !$crate::apply_harness::can_sign() {
            if $crate::apply_harness::in_continuous_integration() {
                panic!(
                    "the test manifests cannot be signed, and in continuous integration the \
                     signing tests are not optional"
                );
            }
            eprintln!("the test manifests cannot be signed");
            return;
        }
    };
}

/// For a test that needs something of the machine it runs on (PowerShell that can compile the
/// probe, a console to detach from). On a developer's machine, where Constrained Language Mode or
/// a missing tool is likely, it is skipped and says why; in continuous integration it is a
/// FAILURE, because a skip passes - and a test that skips everywhere checks nothing.
#[macro_export]
macro_rules! skip_unless_available_or_fail_in_ci {
    ($message:expr) => {{
        if $crate::apply_harness::in_continuous_integration() {
            panic!(
                "needed in continuous integration, not skipped: {}",
                $message
            );
        }
        eprintln!("{}", $message);
        return;
    }};
}

fn seed_of(hex: &str) -> [u8; 32] {
    let nibble = |c: u8| if c <= b'9' { c - b'0' } else { c - b'a' + 10 };
    let digits = hex.as_bytes();
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = nibble(digits[2 * i]) * 16 + nibble(digits[2 * i + 1]);
    }
    seed
}

/// The signature line of `message` under the test key whose seed is `seed_hex`:
/// `ed25519 <keyid> <base64>` and a line feed.
pub fn sign_line_with(message: &[u8], seed_hex: &str, public_hex: &str) -> Vec<u8> {
    let key = SigningKey::from_bytes(&seed_of(seed_hex));
    let signature = key.sign(message).to_bytes();
    format!("ed25519 {} {}\n", &public_hex[..16], base64(&signature)).into_bytes()
}

/// The same under the active test key (the C++'s defaults).
pub fn sign_line(message: &[u8]) -> Vec<u8> {
    sign_line_with(message, TEST_ACTIVE_SEED_HEX, TEST_ACTIVE_PUBLIC_HEX)
}

// ---- a release, from a specification -------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileSpec {
    pub path: String,
    pub content: Vec<u8>,
    pub file_class: FileClass,
    pub executable: bool,
}

impl FileSpec {
    pub fn new(
        path: &str,
        content: impl Into<Vec<u8>>,
        file_class: FileClass,
        executable: bool,
    ) -> Self {
        Self {
            path: String::from(path),
            content: content.into(),
            file_class,
            executable,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseSpec {
    /// A version no build of Katana has, on purpose, for a release that is given none. The default
    /// was 0.2.0, the project's version of the day; a release that is the version of the build
    /// that reads it, or below it, is an update or not by that build wherever the program stands
    /// for the copy (install.rs, the floor against going back). Every release the tests write out
    /// names its own version, so no case rests on this one.
    pub version: String,
    pub tag: String,
    pub sequence: u64,
    pub target: String,
    pub files: Vec<FileSpec>,
}

impl Default for ReleaseSpec {
    fn default() -> Self {
        Self {
            version: String::from("900.2.0"),
            tag: String::from("v900.2.0"),
            sequence: 1000,
            target: String::from("win64"),
            files: Vec::new(),
        }
    }
}

impl ReleaseSpec {
    pub fn find(&self, path: &str) -> Option<&FileSpec> {
        self.files.iter().find(|file| file.path == path)
    }
}

pub fn class_name(file_class: FileClass) -> &'static str {
    match file_class {
        FileClass::Program => "program",
        FileClass::Lib => "lib",
        FileClass::Data => "data",
        FileClass::Seed => "seed",
    }
}

/// The manifest as the producer writes it (manifest.rs): sorted keys, files sorted by path. Where
/// the bytes would sit in a payload is made up - the swap does not read the payload - but keeps
/// the rules of parse_manifest (csize at least 2, inside the payload).
pub fn manifest_json(release: &ReleaseSpec) -> Vec<u8> {
    let mut sorted: Vec<&FileSpec> = release.files.iter().collect();
    sorted.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    let mut offset: u64 = 44;
    let mut files = String::new();
    for file in sorted {
        let csize = 2u64.max(file.content.len() as u64 + 2);
        if !files.is_empty() {
            files.push_str(",\n");
        }
        files.push_str(&format!(
            "{{\"class\":\"{}\",\"csize\":{csize}",
            class_name(file.file_class)
        ));
        if file.executable {
            files.push_str(",\"exec\":true");
        }
        files.push_str(&format!(
            ",\"offset\":{offset},\"path\":\"{}\",\"sha256\":\"{}\",\"size\":{}}}",
            file.path,
            sha256_hex(&file.content),
            file.content.len()
        ));
        offset += csize;
    }
    let payload = offset + 100;
    format!(
        "{{\"applier\":1,\"channel\":\"stable\",\"expires\":\"2030-01-01T00:00:00Z\",\"files\":[\n\
         {files}\n],\"notes_url\":\"https://github.com/kjarada/Katana/releases/tag/{tag}\",\
         \"payload\":{{\"codec\":\"deflate\",\"name\":\"Katana-{version}-{target}.update.zip\",\
         \"size\":{payload}}},\"product\":\"katana\",\"published\":\"2026-10-01T00:00:00Z\",\
         \"schema\":1,\"sequence\":{sequence},\"tag\":\"{tag}\",\"target\":\"{target}\",\
         \"version\":\"{version}\"}}\n",
        tag = release.tag,
        version = release.version,
        target = release.target,
        sequence = release.sequence,
    )
    .into_bytes()
}

/// installed.json as the swap records it (apply.rs, installed_record), written out by hand here:
/// sorted keys, one LF.
pub fn installed_json(release: &ReleaseSpec) -> Vec<u8> {
    format!(
        "{{\"product\":\"katana\",\"schema\":1,\"sequence\":{},\"tag\":\"{}\",\"target\":\"{}\",\
         \"version\":\"{}\"}}\n",
        release.sequence, release.tag, release.target, release.version
    )
    .into_bytes()
}

/// install.json of every install the harness writes: a win64 package that NAMES THE VERSION IT
/// WAS BUILT AS (install.rs; no sequence, as a package of no release names none).
///
/// Why it names one. A record that says its target and no more leaves the program that asks to
/// stand for the copy, and the installed version is then the higher of installed.json's and THIS
/// BUILD's. The updates of the fixtures, of the forty-file pair and of the helper's own tests are
/// 0.9.0 to 0.10.0: by their versions while the project is below 0.10.0, by their sequences alone
/// at 0.10.0, and every one of them a `downgrade` from the first version above that - the swap's
/// tests failing for a change of the project's version and of nothing else. With the package's
/// own version in the record the asking program is not asked at all (state.rs, the package's
/// build), so what is installed is what the records say, whichever build reads them.
///
/// Why 0.1.0, and one text for every install. An update never writes install.json: the tree a
/// good update leaves still has the record of the package the copy came from, which is neither
/// release's own. So the expected trees are written with this same text (write_install of the new
/// release into `expected`), and the tests that read the record back compare with it. It is the
/// package the copy CAME FROM, then, and 0.1.0 is at or below every release a test installs
/// (0.1.0, 0.9.0, 0.10.0 and the 900s): the copy's own build is never above installed.json's, and
/// the installed version and sequence are the record's. A scenario that installs a release BELOW
/// 0.1.0 would read as 0.1.0 - give that one a record of its own.
pub const INSTALL_JSON: &str = "{\"schema\":1,\"target\":\"win64\",\"version\":\"0.1.0\"}";

/// The files of a release at R.
pub fn write_files(root: &Path, release: &ReleaseSpec) {
    for file in &release.files {
        write_text(&root.join(&file.path), &file.content);
    }
}

/// R as the helper leaves an installed release: its files, install.json, installed.json and the
/// signed manifest beside it.
pub fn write_install(root: &Path, release: &ReleaseSpec) {
    write_files(root, release);
    write_text(&root.join("share/katana/install.json"), INSTALL_JSON);
    write_text(
        &root.join("share/katana/installed.json"),
        installed_json(release),
    );
    let json = manifest_json(release);
    write_text(&root.join("share/katana/installed.manifest.json"), &json);
    write_text(
        &root.join("share/katana/installed.manifest.json.sig"),
        sign_line(&json),
    );
}

/// The stage folder of a release: every file at its path, and the signed manifest.
pub fn write_stage(stage: &Path, release: &ReleaseSpec) {
    write_files(stage, release);
    let json = manifest_json(release);
    write_text(&stage.join("manifest.json"), &json);
    write_text(&stage.join("manifest.json.sig"), sign_line(&json));
}

// ---- whole-tree hash ---------------------------------------------------------------------------

fn snapshot_into(folder: &Path, prefix: &str, entries: &mut BTreeMap<String, String>) {
    let Ok(children) = std::fs::read_dir(folder) else {
        return;
    };
    for child in children.flatten() {
        let name =
            String::from_utf8_lossy(&path_to_utf8(Path::new(&child.file_name()))).into_owned();
        let generic = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if generic == ".update" || generic.starts_with(".update/") {
            continue;
        }
        let path = child.path();
        if std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
            entries.insert(generic.clone(), String::from("dir"));
            if child.file_type().is_ok_and(|kind| kind.is_dir()) {
                snapshot_into(&path, &generic, entries);
            }
        } else {
            // std::filesystem::file_size answers -1 for what it cannot measure.
            let size = std::fs::metadata(&path).map_or(u64::MAX, |metadata| metadata.len());
            let hash = sha256_file_hex(&path).unwrap_or_else(|_| String::from("unreadable"));
            entries.insert(generic, format!("file {size} {hash}"));
        }
    }
}

/// Every entry under `root` except the helper's own working folder .update: a line per folder and
/// per file (path, size, SHA-256), sorted. Two trees are the same when this is. A folder is part
/// of it, so a rollback that leaves an empty folder the update made is a difference.
pub fn snapshot(root: &Path) -> BTreeMap<String, String> {
    let mut entries = BTreeMap::new();
    snapshot_into(root, "", &mut entries);
    entries
}

pub fn tree_hash(root: &Path) -> String {
    let mut hash = Sha256::new();
    for (name, what) in &snapshot(root) {
        hash.update(name);
        hash.update("\n");
        hash.update(what);
        hash.update("\n");
    }
    hash.finish_hex()
}

/// The differences between two snapshots, one per line, for a failure message.
pub fn difference(
    expected: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
) -> String {
    let mut out = String::new();
    for (name, what) in expected {
        match actual.get(name) {
            None => out.push_str(&format!("  missing:    {name}\n")),
            Some(found) if found != what => out.push_str(&format!("  different:  {name}\n")),
            Some(_) => {}
        }
    }
    for name in actual.keys() {
        if !expected.contains_key(name) {
            out.push_str(&format!("  unexpected: {name}\n"));
        }
    }
    out
}

/// The names under R/.update other than the lock: what a finished transaction must not leave.
pub fn leftovers_in_update(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(".update")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| {
            String::from_utf8_lossy(&path_to_utf8(Path::new(&entry.file_name()))).into_owned()
        })
        .filter(|name| name != "lock")
        .collect()
}

// ---- a scenario: a v1 install, a v2 stage, the tree v2 must leave ------------------------------

pub struct Scenario {
    pub scratch: Scratch,
    pub root: PathBuf,
    pub stage: PathBuf,
    pub state: PathBuf,
    /// The tree after a good update, built from the specification.
    pub expected: PathBuf,
    pub v1: ReleaseSpec,
    pub v2: ReleaseSpec,
    /// The install as it is before the update, entry by entry.
    pub v1_snapshot: BTreeMap<String, String>,
    /// The install as it is before the update.
    pub v1_hash: String,
    /// The tree a good update must leave.
    pub v2_hash: String,
}

impl Scenario {
    pub fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let root = scratch.join("install");
        let stage = scratch.join("stage");
        let state = scratch.join("state");
        let expected = scratch.join("expected");
        let _ = std::fs::create_dir_all(&state);
        Self {
            scratch,
            root,
            stage,
            state,
            expected,
            v1: ReleaseSpec::default(),
            v2: ReleaseSpec::default(),
            v1_snapshot: BTreeMap::new(),
            v1_hash: String::new(),
            v2_hash: String::new(),
        }
    }
}

/// File contents: deterministic, of different sizes, with a NUL and a high byte in some.
pub fn content_of(path: &str, version: &str) -> Vec<u8> {
    let unit = format!("{path}@{version}\n").into_bytes();
    // The first hexadecimal digit of the path's hash, as a character code.
    let first = sha256_hex(path).as_bytes()[0];
    let repeat = usize::from(first) % 7 + 1;
    let mut content = unit.clone();
    for _ in 1..repeat {
        content.extend_from_slice(&unit);
    }
    if path.len().is_multiple_of(5) {
        content.push(0);
        content.push(0xE9);
    }
    content
}

/// The 40-file release pair of the fault loops (see the list in test_fault_loops.cpp). v1 has 38
/// files, v2 has 40; the update changes 12, adds 5, removes 3 (one has been edited by the person
/// and stays), creates 2 folders and empties 1.
pub fn build_forty_file_scenario(s: &mut Scenario) {
    s.v1.version = String::from("0.9.0");
    s.v1.tag = String::from("v0.9.0");
    s.v1.sequence = 1000;
    s.v2.version = String::from("0.10.0");
    s.v2.tag = String::from("v0.10.0");
    s.v2.sequence = 2000;

    fn add(
        release: &mut ReleaseSpec,
        path: &str,
        file_class: FileClass,
        exec: bool,
        version: &str,
    ) {
        release.files.push(FileSpec::new(
            path,
            content_of(path, version),
            file_class,
            exec,
        ));
    }
    fn both(s: &mut Scenario, path: &str, file_class: FileClass, exec: bool, changed: bool) {
        add(&mut s.v1, path, file_class, exec, "1");
        add(
            &mut s.v2,
            path,
            file_class,
            exec,
            if changed { "2" } else { "1" },
        );
    }

    both(s, "bin/katana.exe", FileClass::Program, true, true);
    both(s, "bin/katana_cli.exe", FileClass::Program, true, true);
    both(s, "bin/katana_updater.exe", FileClass::Program, true, true);
    for i in 1..=12 {
        let name = format!("bin/lib{}{i}.dll", if i < 10 { "0" } else { "" });
        both(
            s,
            &name,
            FileClass::Lib,
            false,
            i == 3 || i == 4 || i == 7 || i == 11,
        );
    }
    add(&mut s.v1, "bin/old1.dll", FileClass::Lib, false, "1");
    add(&mut s.v1, "bin/old2.dll", FileClass::Lib, false, "1");
    add(&mut s.v2, "bin/lib13.dll", FileClass::Lib, false, "2");
    add(&mut s.v2, "bin/lib14.dll", FileClass::Lib, false, "2");
    for i in 1..=16 {
        let name = format!(
            "share/katana/data/f{}{i}.dat",
            if i < 10 { "0" } else { "" }
        );
        both(
            s,
            &name,
            FileClass::Data,
            false,
            i == 2 || i == 5 || i == 9 || i == 13,
        );
    }
    add(
        &mut s.v1,
        "share/katana/legacy/old.txt",
        FileClass::Data,
        false,
        "1",
    );
    add(
        &mut s.v2,
        "share/katana/more/g1.dat",
        FileClass::Data,
        false,
        "2",
    );
    add(
        &mut s.v2,
        "share/katana/more/g2.dat",
        FileClass::Data,
        false,
        "2",
    );
    s.v1.files.push(FileSpec::new(
        "share/katana/empty.txt",
        "",
        FileClass::Data,
        false,
    ));
    s.v2.files.push(FileSpec::new(
        "share/katana/empty.txt",
        "",
        FileClass::Data,
        false,
    ));
    both(s, "etc/katana/settings.ini", FileClass::Seed, false, true);
    both(s, "lib/plugins/p1.dat", FileClass::Data, false, false);
    both(s, "lib/plugins/p2.dat", FileClass::Data, false, true);
    add(
        &mut s.v2,
        "lib/plugins/extra/p3.dat",
        FileClass::Data,
        false,
        "2",
    );

    // The install before the update: v1, and what a person did to it.
    write_install(&s.root, &s.v1);
    write_text(&s.root.join("etc/katana/settings.ini"), "user=edited\n"); // a seed, edited
    write_text(
        &s.root.join("share/katana/data/f05.dat"),
        "the person's own f05\n",
    ); // changed by v2, edited
    write_text(&s.root.join("bin/old2.dll"), "the person's own old2\n"); // removed by v2, edited
    write_text(
        &s.root.join("share/katana/user_notes.txt"),
        "notes the update never heard of\n",
    );
    write_text(
        &s.root.join("bin/user_tool.dll"),
        "a plug-in nobody listed\n",
    );
    write_stage(&s.stage, &s.v2);

    // The tree a good update leaves, written out from the specification: v2's files, except the
    // seed and the edited old2 (which stay as the person had them), then the person's unknown
    // files, then the records.
    for file in &s.v2.files {
        if file.file_class != FileClass::Seed {
            write_text(&s.expected.join(&file.path), &file.content);
        }
    }
    write_text(&s.expected.join("etc/katana/settings.ini"), "user=edited\n");
    write_text(&s.expected.join("bin/old2.dll"), "the person's own old2\n");
    write_text(
        &s.expected.join("share/katana/user_notes.txt"),
        "notes the update never heard of\n",
    );
    write_text(
        &s.expected.join("bin/user_tool.dll"),
        "a plug-in nobody listed\n",
    );
    write_text(&s.expected.join("share/katana/install.json"), INSTALL_JSON);
    write_text(
        &s.expected.join("share/katana/installed.json"),
        installed_json(&s.v2),
    );
    write_text(
        &s.expected.join("share/katana/installed.manifest.json"),
        manifest_json(&s.v2),
    );
    write_text(
        &s.expected.join("share/katana/installed.manifest.json.sig"),
        sign_line(&manifest_json(&s.v2)),
    );

    s.v1_hash = tree_hash(&s.root);
    s.v1_snapshot = snapshot(&s.root);
    s.v2_hash = tree_hash(&s.expected);
}

// ---- the T3 fixtures: a release pair made by the Python producer and signed by openssl -----------

pub fn fixture_folder() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/update/fixtures"
    ))
}

/// `from` and everything in it, copied over `to` (std::filesystem::copy, recursive, overwriting).
pub fn copy_tree(from: &Path, to: &Path) {
    let _ = std::fs::create_dir_all(to);
    let mut entries: Vec<PathBuf> = Vec::new();
    walk(from, Path::new(""), &mut entries)
        .unwrap_or_else(|error| panic!("{}: {error}", text(from)));
    for relative in &entries {
        let source = from.join(relative);
        let target = to.join(relative);
        let copied = if std::fs::metadata(&source).is_ok_and(|metadata| metadata.is_dir()) {
            std::fs::create_dir_all(&target)
        } else {
            std::fs::copy(&source, &target).map(|_| ())
        };
        if let Err(error) = copied {
            panic!("{}: {error}", text(&source));
        }
    }
}

/// R is the fixtures' install `installed` ("v1" or "v2") with the records of that release; the
/// stage is the fixtures' v2 files with the manifest and signature of release folder
/// `stage_release` ("v2", or "bad/downgrade-version", ...). With the defaults - v1 installed, v2
/// staged ([`build_fixture_scenario`]) - `expected` is the tree the update must leave: v2's files,
/// the seed as v1 had it, and v2's records. install.json is the harness's (INSTALL_JSON) in R and
/// in `expected`: the fixture trees' own says its target and no more, which would leave the
/// installed version to the build that reads it.
pub fn build_fixture_scenario_of(s: &mut Scenario, installed: &str, stage_release: &str) {
    let fixtures = fixture_folder();
    copy_tree(&fixtures.join("install").join(installed), &s.root);
    write_text(&s.root.join("share/katana/install.json"), INSTALL_JSON);
    let v1 = installed == "v1";
    write_text(
        &s.root.join("share/katana/installed.json"),
        format!(
            "{{\"product\":\"katana\",\"schema\":1,\"sequence\":{},\"target\":\"win64\",\
             \"version\":\"{}\"}}\n",
            if v1 {
                "1790000000,\"tag\":\"v0.9.0\""
            } else {
                "1792312800,\"tag\":\"v0.10.0\""
            },
            if v1 { "0.9.0" } else { "0.10.0" }
        ),
    );
    let installed_release = fixtures.join("release").join(installed);
    write_text(
        &s.root.join("share/katana/installed.manifest.json"),
        read_text(&installed_release.join("Katana-update-win64.json")),
    );
    write_text(
        &s.root.join("share/katana/installed.manifest.json.sig"),
        read_text(&installed_release.join("Katana-update-win64.json.sig")),
    );

    copy_tree(&fixtures.join("install").join("v2"), &s.stage);
    // The installer's mark is never in a stage.
    let _ = std::fs::remove_file(s.stage.join("share/katana/install.json"));
    let staged = fixtures.join("release").join(stage_release);
    write_text(
        &s.stage.join("manifest.json"),
        read_text(&staged.join("Katana-update-win64.json")),
    );
    write_text(
        &s.stage.join("manifest.json.sig"),
        read_text(&staged.join("Katana-update-win64.json.sig")),
    );

    if v1 && stage_release == "v2" {
        copy_tree(&fixtures.join("install").join("v2"), &s.expected);
        // R's: no update writes it.
        write_text(&s.expected.join("share/katana/install.json"), INSTALL_JSON);
        write_text(
            &s.expected.join("etc/katana/settings.ini"),
            read_text(&fixtures.join("install/v1/etc/katana/settings.ini")),
        );
        write_text(
            &s.expected.join("share/katana/installed.json"),
            "{\"product\":\"katana\",\"schema\":1,\"sequence\":1792312800,\"tag\":\"v0.10.0\",\
             \"target\":\"win64\",\"version\":\"0.10.0\"}\n",
        );
        write_text(
            &s.expected.join("share/katana/installed.manifest.json"),
            read_text(&fixtures.join("release/v2/Katana-update-win64.json")),
        );
        write_text(
            &s.expected.join("share/katana/installed.manifest.json.sig"),
            read_text(&fixtures.join("release/v2/Katana-update-win64.json.sig")),
        );
        s.v2_hash = tree_hash(&s.expected);
    }
    s.v1_hash = tree_hash(&s.root);
    s.v1_snapshot = snapshot(&s.root);
}

/// The fixture scenario with the C++'s defaults: v1 installed, v2 staged.
pub fn build_fixture_scenario(s: &mut Scenario) {
    build_fixture_scenario_of(s, "v1", "v2");
}

// ---- running the swap ----------------------------------------------------------------------------

pub fn fast_timings() -> ApplyTimings {
    ApplyTimings {
        wait_for_parent: Duration::from_millis(60),
        wait_for_holders: Duration::from_millis(60),
        poll_interval: Duration::from_millis(1),
        health_wait: Duration::from_millis(2000),
    }
}

/// A new version that starts: --update-health writes the marker and exits 0.
pub fn healthy_start(
    state_dir: &Path,
    transaction: &str,
) -> impl Fn(&SpawnRequest) -> Result<SpawnResult> + 'static {
    let marker = health_marker_path(state_dir, transaction);
    move |_request: &SpawnRequest| {
        write_text(&marker, "ok");
        Ok(SpawnResult::default())
    }
}

/// The doubles and the request of one run of apply().
pub struct Rig<'a> {
    pub fs: &'a dyn FsOps,
    pub tx: String,
    pub request: ApplyRequest,
    pub processes: FakeProcesses,
    pub launcher: FakeLauncher<'a>,
    pub registry: FakeRegistry,
    pub timings: ApplyTimings,
}

impl<'a> Rig<'a> {
    /// The C++'s default transaction, "a1b2c3d4".
    pub fn new(scenario: &Scenario, fs: &'a dyn FsOps) -> Self {
        Self::with_transaction(scenario, fs, "a1b2c3d4")
    }

    pub fn with_transaction(scenario: &Scenario, fs: &'a dyn FsOps, transaction: &str) -> Self {
        let tx = String::from(transaction);
        let request = ApplyRequest {
            root: scenario.root.clone(),
            stage_directory: scenario.stage.clone(),
            state_directory: scenario.state.clone(),
            helper: scenario
                .state
                .join(format!("run-{tx}"))
                .join("katana_updater.exe"),
            transaction: tx.clone(),
            wait_pid: 0,
            relaunch: scenario.root.join("bin").join("katana.exe"),
        };
        let launcher = FakeLauncher::default();
        launcher.set_on_run(healthy_start(&scenario.state, &tx));
        Self {
            fs,
            tx,
            request,
            processes: FakeProcesses::default(),
            launcher,
            registry: FakeRegistry::default(),
            timings: fast_timings(),
        }
    }

    pub fn services(&self) -> ApplyServices<'_> {
        ApplyServices {
            fs: self.fs,
            processes: &self.processes,
            launcher: &self.launcher,
            registry: &self.registry,
            trust: test_trust(),
            timings: self.timings,
        }
    }

    /// apply() with no stop.
    pub fn run(&self) -> ApplyResult {
        self.run_stopped(None)
    }

    pub fn run_stopped(&self, stop: StopToken<'_>) -> ApplyResult {
        apply(&self.request, &self.services(), stop)
    }
}

// ---- crashing and recovering ---------------------------------------------------------------------

pub fn state_name(state: JournalState) -> &'static str {
    match state {
        JournalState::Empty => "Empty",
        JournalState::Planned => "Planned",
        JournalState::Committing => "Committing",
        JournalState::CommittedPendingHealth => "CommittedPendingHealth",
        JournalState::RollingBack => "RollingBack",
        JournalState::RolledBack => "RolledBack",
        JournalState::Healthy => "Healthy",
    }
}

/// A scenario kept pristine on the side, put back in front of every iteration.
pub struct Loop {
    scenario: Scenario,
    pristine: PathBuf,
}

impl Loop {
    pub fn new(label: &str, build: &dyn Fn(&mut Scenario)) -> Self {
        let mut scenario = Scenario::new(label);
        build(&mut scenario);
        let pristine = scenario.scratch.join("pristine");
        copy_tree(&scenario.root, &pristine.join("root"));
        copy_tree(&scenario.stage, &pristine.join("stage"));
        Self { scenario, pristine }
    }

    pub fn reset(&mut self) {
        replace_tree(&self.pristine.join("root"), &self.scenario.root);
        replace_tree(&self.pristine.join("stage"), &self.scenario.stage);
        let _ = remove_all(&self.scenario.state);
        let _ = std::fs::create_dir_all(&self.scenario.state);
    }

    pub fn scenario(&mut self) -> &mut Scenario {
        &mut self.scenario
    }

    pub fn pristine_root(&self) -> &Path {
        &self.pristine
    }
}

/// A SimulatedCrash is a panic, and the default hook would print each of the hundreds a fault
/// loop raises: once in the process, a hook that passes over a SimulatedCrash and hands every
/// other panic to the default one.
pub fn quiet_simulated_crashes() {
    static QUIET: Once = Once::new();
    QUIET.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().is::<SimulatedCrash>() {
                return;
            }
            default(info);
        }));
    });
}

// Runs `work`, and gives back the SimulatedCrash it died of, if it did; any other panic goes on.
fn until_crash<T>(work: impl FnOnce() -> T) -> std::result::Result<T, SimulatedCrash> {
    quiet_simulated_crashes();
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(value) => Ok(value),
        Err(payload) => match payload.downcast::<SimulatedCrash>() {
            Ok(crash) => Err(*crash),
            Err(other) => resume_unwind(other),
        },
    }
}

/// The apply, on `fs`, until it finishes or "dies". Returns the crash, if there was one; the
/// result, when there was none, goes to `result`.
pub fn apply_until_crash(
    s: &Scenario,
    fs: &dyn FsOps,
    result: Option<&mut ApplyResult>,
) -> Option<SimulatedCrash> {
    let rig = Rig::new(s, fs);
    match until_crash(|| rig.run()) {
        Ok(ended) => {
            if let Some(result) = result {
                *result = ended;
            }
            None
        }
        Err(crash) => Some(crash),
    }
}

/// recover_in_process on `fs`, until it finishes or "dies".
pub fn recover_until_crash(
    s: &Scenario,
    fs: &dyn FsOps,
    status: Option<&mut Status>,
) -> Option<SimulatedCrash> {
    let rig = Rig::with_transaction(s, fs, "eeeeeeee");
    match until_crash(|| recover_in_process(&s.root, &rig.services())) {
        Ok(ended) => {
            if let Some(status) = status {
                *status = ended;
            }
            None
        }
        Err(crash) => Some(crash),
    }
}

pub fn recover_clean(s: &Scenario) -> Status {
    let clean = StdFs;
    let mut status: Status = Ok(());
    let crash = recover_until_crash(s, &clean, Some(&mut status));
    assert!(crash.is_none(), "a clean recovery crashed: {crash:?}");
    status
}
