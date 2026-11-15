//! The records beside the programs (install.rs: install.json, installed.json, the signed manifest
//! kept for removals) and in the per-user state folder (state.rs: state.json, last.json, the
//! cleanup list), and the one function that finds the install root of the running program. At
//! the end, the other thing that is asked of the running program: how it was started
//! (process.rs, process_is_elevated). The twin of tests/update/test_state.cpp.
//!
//! Each expectation comes from the contract in the module, worked out by hand: the file shapes
//! are the ones the modules print, the paths are joined by the rules there, and the numbers in the
//! JSON are the ones a careful writer would put (2^64 - 1 is 18446744073709551615). The cleanup
//! list is the one place where input is hostile (the state folder is writable by anyone who is
//! the user), so its cases are the attacks: a line that leaves the folder by "..", by a sibling
//! that shares a prefix, by a link; a name that is a folder, or the list.
//!
//! The cleanup cases run on the file system that ships (make_system_fs_ops) on Windows; DiskFs
//! below (the std::fs operations the interface describes, with links reported as the interface
//! says they are) is the file system elsewhere, where the shipped one refuses every operation
//! until the POSIX apply is written, and it carries the faults the real one cannot be made to have
//! on demand (a locked file, a crash before the k-th change, a failed resolve, an append that
//! lands between two calls).
//!
//! The cases of this file run on threads of one process, where the C++'s ran each in a process of
//! its own: the ones that change the environment or the working folder take one lock first
//! (environment_lock), and every scratch folder is the case's own.

#![allow(
    unsafe_code,
    reason = "Edition 2024 marks changing the environment unsafe, and the Windows cases ask the process token directly."
)]

use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::fault_fs::{FaultFs, SimulatedCrash};
use katana_update::fsops::{EntryInfo, EntryKind, FileLock, FsOps};
use katana_update::install::detail::read_install_as;
use katana_update::install::{
    self, InstallKind, install_record_directory, install_root_of_running_program, read_install,
    update_work_directory,
};
use katana_update::manifest::Manifest;
use katana_update::process::process_is_elevated;
use katana_update::registry::RegistryOps;
use katana_update::state::{
    LastResult, UpdateState, append_cleanup, clear_last_result, health_marker_path,
    process_cleanup_list, read_last_result, read_state, run_path, stage_path, state_directory,
    write_last_result, write_state,
};
use katana_update::trust::is_newer_than;
use katana_update::version::{self, Version, parse_version};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/update/fixtures"
);

// The version the project declares, read here as the C++ test is handed it (KATANA_EXPECTED_VERSION
// from project(Katana VERSION ...)), and not from the library under test.
fn expected_version() -> String {
    let cmake = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../CMakeLists.txt"
    ))
    .expect("the checkout's CMakeLists.txt");
    let start = cmake.find("project(Katana").expect("project(Katana ...)");
    let arguments = &cmake[start..start + cmake[start..].find(')').expect("a closing bracket")];
    let mut words = arguments.split_ascii_whitespace();
    while let Some(word) = words.next() {
        if word == "VERSION" {
            return words.next().expect("a version").to_owned();
        }
    }
    panic!("project(Katana ...) names no VERSION");
}

fn ok<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn describe<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => String::from("ok"),
        Err(error) => error.describe(),
    }
}

fn read_file(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn write_file(path: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, bytes);
}

fn append_to(path: &Path, bytes: &[u8]) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(bytes);
    }
}

fn entries_in(folder: &Path) -> usize {
    std::fs::read_dir(folder).map_or(0, Iterator::count)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn text_of(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "katana_t6_state_{}_{clock}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("a scratch folder");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// A directory link (a junction on Windows: it needs no privilege; a symlink elsewhere), taken away
// again without touching what it points at.
struct DirectoryLink {
    link: PathBuf,
    made: bool,
}

impl DirectoryLink {
    fn new(link: &Path, target: &Path) -> Self {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        #[cfg(not(any(unix, windows)))]
        let made = {
            let _ = (link, target);
            false
        };
        Self {
            link: link.to_path_buf(),
            made,
        }
    }

    fn made(&self) -> bool {
        self.made
    }
}

impl Drop for DirectoryLink {
    fn drop(&mut self) {
        if self.made {
            // The link itself (a symlink is a file to remove, a junction a folder), never what it
            // points at.
            if std::fs::remove_file(&self.link).is_err() {
                let _ = std::fs::remove_dir(&self.link);
            }
        }
    }
}

// The environment and the working folder are the process's: one case at a time changes them.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn environment_lock() -> MutexGuard<'static, ()> {
    ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn set_variable(name: &str, value: Option<&std::ffi::OsStr>) {
    match value {
        // SAFETY: the environment is changed only while ENVIRONMENT is held, and the standard
        // library's own lock orders it with every read through std::env.
        Some(value) => unsafe { std::env::set_var(name, value) },
        // SAFETY: as above.
        None => unsafe { std::env::remove_var(name) },
    }
}

// Sets an environment variable for the length of a case and puts it back. `value` None removes
// it. The caller holds environment_lock().
struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvGuard {
    fn new(name: &'static str, value: Option<&Path>) -> Self {
        let previous = std::env::var_os(name);
        set_variable(name, value.map(Path::as_os_str));
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        set_variable(self.name, self.previous.as_deref());
    }
}

// Makes `to` the working folder for the length of a case and puts the old one back. The caller
// holds environment_lock().
struct CwdGuard {
    previous: PathBuf,
}

impl CwdGuard {
    fn enter(to: &Path) -> Self {
        let previous = std::env::current_dir().expect("the working folder");
        std::env::set_current_dir(to).expect("a working folder to enter");
        Self { previous }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

// ---- the file system, as FsOps describes it ------------------------------------------------------

fn os_error(what: &str, code: i32, path: &Path) -> Error {
    make_error(
        ErrorCode::FileExportFailure,
        what,
        format!("os_error={code}; {}", text_of(path)),
    )
}

fn raw(error: &std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(0)
}

// What a call of the code under test was, as the hooks are told it: whether it is one of the
// operations fsops.rs lists as changing the disk.
struct Call {
    changes: bool,
}

// Raised in place of the k-th changing operation: the process died between k - 1 and k.
struct Crash {
    operation: u64,
}

// The operation a run died before, from what catch_unwind gave back: a Crash of these cases or
// the library's own SimulatedCrash. Any other panic goes on.
fn crash_of(outcome: std::thread::Result<Status>) -> u64 {
    let payload = match outcome {
        Ok(status) => panic!("the run did not die: {}", describe(&status)),
        Err(payload) => payload,
    };
    if let Some(crash) = payload.downcast_ref::<Crash>() {
        return crash.operation;
    }
    if let Some(crash) = payload.downcast_ref::<SimulatedCrash>() {
        return crash.operation;
    }
    std::panic::resume_unwind(payload)
}

// std::filesystem::weakly_canonical: the longest part of the path that exists, with its links
// resolved, and the rest as it is written, normalised.
fn weakly_canonical(path: &Path) -> std::io::Result<PathBuf> {
    let names: Vec<Component<'_>> = path.components().collect();
    let mut existing = names.len();
    let mut head = loop {
        if existing == 0 {
            break PathBuf::new();
        }
        let prefix: PathBuf = names[..existing].iter().collect();
        match std::fs::metadata(&prefix) {
            Ok(_) => break std::fs::canonicalize(&prefix)?,
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
            {
                existing -= 1;
            }
            Err(error) => return Err(error),
        }
    };
    for name in &names[existing..] {
        match name {
            Component::CurDir => {}
            Component::ParentDir => {
                head.pop();
            }
            other => head.push(other),
        }
    }
    Ok(head)
}

// std::filesystem::remove: a file, a link, or a folder that is empty; nothing there is no error.
fn remove_entry(path: &Path) -> std::io::Result<()> {
    let removed = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir(path),
        _ => std::fs::remove_file(path),
    };
    match removed {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(not(windows))]
fn stat_entry(path: &Path) -> Result<EntryInfo> {
    let mut info = EntryInfo::default();
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(info);
        }
        Err(error) => return Err(os_error("stat failed", raw(&error), path)),
    };
    let kind = metadata.file_type();
    info.reparse_point = kind.is_symlink();
    info.kind = if info.reparse_point {
        EntryKind::Other
    } else if kind.is_file() {
        EntryKind::File
    } else if kind.is_dir() {
        EntryKind::Directory
    } else {
        EntryKind::Other
    };
    if info.kind == EntryKind::File {
        info.size = metadata.len();
    }
    Ok(info)
}

#[cfg(windows)]
fn stat_entry(path: &Path) -> Result<EntryInfo> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let mut info = EntryInfo::default();
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(info),
        Err(error) => return Err(os_error("stat failed", raw(&error), path)),
    };
    let attributes = metadata.file_attributes();
    info.reparse_point = (attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
    info.kind = if (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    if info.kind == EntryKind::File {
        info.size = metadata.len();
    }
    Ok(info)
}

// The hooks DiskFs runs around a call, and its test of a path.
type CallHook<'a> = Option<Box<dyn Fn(&Call) + 'a>>;
type PathTest<'a> = Option<Box<dyn Fn(&Path) -> bool + 'a>>;

// The std::fs operations the interface describes, and what the cases ask of them: hooks told of
// every call before it is carried out and after it, so that a case can act BETWEEN two calls of
// the code under test (an append from another program, a crash before the k-th change); names
// whose removal fails with 32 as a locked file's does; a name whose removal is a crash; paths that
// cannot be resolved; and a record of every remove_file. With none of them it is the C++'s
// DiskFs; with the locked names alone its LockedFs; with the hooks its WatchedFs.
#[derive(Default)]
struct DiskFs<'a> {
    before: CallHook<'a>,
    after: CallHook<'a>,
    locked_names: BTreeSet<String>,
    crash_on_name: Option<&'static str>,
    unresolvable: PathTest<'a>,
    removed_files: RefCell<Vec<PathBuf>>,
}

impl DiskFs<'_> {
    fn locked(names: &[&str]) -> Self {
        Self {
            locked_names: names.iter().map(|&name| String::from(name)).collect(),
            ..Self::default()
        }
    }

    fn watched<T>(&self, changes: bool, carry: impl FnOnce() -> T) -> T {
        let call = Call { changes };
        if let Some(before) = &self.before {
            before(&call);
        }
        let result = carry();
        if let Some(after) = &self.after {
            after(&call);
        }
        result
    }

    fn name_of(path: &Path) -> String {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl FsOps for DiskFs<'_> {
    fn stat(&self, path: &Path) -> Result<EntryInfo> {
        self.watched(false, || stat_entry(path))
    }

    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        self.watched(false, || {
            let entries = std::fs::read_dir(directory)
                .map_err(|error| os_error("list failed", raw(&error), directory))?;
            let mut out = Vec::new();
            for entry in entries {
                let entry =
                    entry.map_err(|error| os_error("list failed", raw(&error), directory))?;
                out.push(entry.path());
            }
            Ok(out)
        })
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.watched(false, || {
            std::fs::read(path).map_err(|_| os_error("read failed", 0, path))
        })
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        self.watched(false, || {
            if self
                .unresolvable
                .as_ref()
                .is_some_and(|unresolvable| unresolvable(path))
            {
                return Err(os_error("resolve failed", 5, path));
            }
            weakly_canonical(path).map_err(|error| os_error("resolve failed", raw(&error), path))
        })
    }

    // The standard library has no free-space call, and no case here asks.
    fn free_space(&self, _path: &Path) -> Result<u64> {
        Err(Error::new(ErrorCode::Unsupported, "not used here"))
    }

    fn make_directories(&self, directory: &Path) -> Status {
        self.watched(true, || {
            std::fs::create_dir_all(directory)
                .map_err(|error| os_error("mkdir failed", raw(&error), directory))
        })
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Status {
        // std::filesystem::copy_file with no options: refused when `to` exists.
        if std::fs::symlink_metadata(to).is_ok() {
            return Err(os_error("copy failed", 17, to));
        }
        std::fs::copy(from, to)
            .map(|_| ())
            .map_err(|error| os_error("copy failed", raw(&error), to))
    }

    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        self.watched(true, || {
            std::fs::rename(from, to).map_err(|error| os_error("rename failed", raw(&error), to))
        })
    }

    fn remove_file(&self, path: &Path) -> Status {
        self.watched(true, || {
            self.removed_files.borrow_mut().push(path.to_path_buf());
            let name = Self::name_of(path);
            if self.crash_on_name == Some(name.as_str()) {
                std::panic::panic_any(Crash { operation: 1 });
            }
            if self.locked_names.contains(&name) {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    "the file is in use",
                    format!("os_error=32; {}", text_of(path)),
                ));
            }
            remove_entry(path).map_err(|error| os_error("remove failed", raw(&error), path))
        })
    }

    fn remove_empty_directory(&self, path: &Path) -> Status {
        self.watched(true, || {
            remove_entry(path).map_err(|error| os_error("rmdir failed", raw(&error), path))
        })
    }

    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        self.watched(true, || {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| file.write_all(bytes))
                .map_err(|_| os_error("append failed", 0, path))
        })
    }

    fn write_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        self.watched(true, || {
            std::fs::write(path, bytes).map_err(|_| os_error("write failed", 0, path))
        })
    }

    fn set_executable(&self, _path: &Path) -> Status {
        Ok(())
    }

    fn sync_directory(&self, _directory: &Path) -> Status {
        Ok(())
    }

    fn try_lock(&self, _path: &Path) -> Result<Box<dyn FileLock>> {
        Err(Error::new(ErrorCode::Unsupported, "not used here"))
    }
}

// A registry that says what it is told to.
#[derive(Default)]
struct FakeRegistry {
    location: Option<PathBuf>,
    fails: bool,
}

impl RegistryOps for FakeRegistry {
    fn installer_location(&self) -> Result<Option<PathBuf>> {
        if self.fails {
            return Err(Error::new(
                ErrorCode::Internal,
                "the registry cannot be read",
            ));
        }
        Ok(self.location.clone())
    }

    fn set_run_once(&self, _name: &str, _command: &str) -> Status {
        Ok(())
    }

    fn clear_run_once(&self, _name: &str) -> Status {
        Ok(())
    }

    fn set_installed_version(&self, _display_version: &str, _display_name: &str) -> Status {
        Ok(())
    }
}

fn version_of(a: u32, b: u32, c: u32, d: u32, written: u8) -> Version {
    Version {
        components: [a, b, c, d],
        written,
    }
}

// An install root with install.json (and whatever else the case adds).
struct Root {
    temp: TempDir,
    root: PathBuf,
    records: PathBuf,
}

impl Root {
    fn new() -> Self {
        let temp = TempDir::new();
        let root = temp.path().join("R");
        let records = root.join("share").join("katana");
        std::fs::create_dir_all(&records).expect("the records folder");
        Self {
            temp,
            root,
            records,
        }
    }

    fn install_json(&self, text: &str) {
        write_file(&self.records.join("install.json"), text);
    }

    fn installed_json(&self, text: &str) {
        write_file(&self.records.join("installed.json"), text);
    }
}

fn line_of(path: &Path) -> Vec<u8> {
    let mut line = path_to_utf8(path);
    line.push(b'\n');
    line
}

// ---- the installed version is the higher of the record's and the program's own ------------------
//
// The program's own version is the one CMakeLists.txt declares: the version this build was
// configured as, and not taken from the library under test (the version module's tests hold
// program_version() to it). A record is placed below it as 0.1.0, which the project's version left
// behind for good, and above it by raising its first number, so these stay the same cases when the
// project's version moves on.

fn the_programs_own_version() -> Version {
    ok(parse_version(&expected_version()))
}

// The program's own version with its first number raised by one, written with three numbers.
fn a_version_above_the_program() -> Version {
    let mut above = the_programs_own_version();
    above.components = [above.components[0] + 1, 0, 0, 0];
    above.written = 3;
    above
}

fn installed_record_of(version: &str, sequence: u64) -> String {
    format!(
        r#"{{"schema":1,"product":"katana","version":"{version}","sequence":{sequence},"tag":"v{version}","target":"win64"}}"#
    )
}

fn a_release_of(version: Version, sequence: u64) -> Manifest {
    Manifest {
        version,
        sequence,
        ..Manifest::default()
    }
}

// install.json for target win64 with `build` after it: "version":"0.3.0","sequence":300.
fn package_record_of(build: &str) -> String {
    format!(r#"{{"schema":1,"target":"win64",{build}}}"#)
}

mod install_records {
    use super::*;

    // ---- the names of things --------------------------------------------------------------------

    #[test]
    fn kinds_are_named_in_the_words_of_update_status() {
        assert_eq!(install::to_string(InstallKind::Installer), "installer");
        assert_eq!(install::to_string(InstallKind::Archive), "archive");
        assert_eq!(install::to_string(InstallKind::Development), "development");
    }

    #[test]
    fn the_records_live_in_share_katana_and_the_work_folder_is_dot_update() {
        let root = Path::new("R");
        assert_eq!(
            install_record_directory(root),
            root.join("share").join("katana")
        );
        assert_eq!(update_work_directory(root), root.join(".update"));
    }

    // ---- read_install ---------------------------------------------------------------------------

    #[test]
    fn a_tree_without_install_json_is_a_development_tree_whatever_else_is_in_it() {
        let world = Root::new();
        // Even a registry that claims it does not make it an install.
        let registry = FakeRegistry {
            location: Some(world.root.clone()),
            ..FakeRegistry::default()
        };
        // installed.json and the manifest copy, left behind in a copied folder: still not an
        // install.
        world.installed_json(r#"{"schema":1,"version":"9.9.9","sequence":5}"#);
        write_file(&world.records.join("installed.manifest.json"), "{}");
        write_file(&world.records.join("installed.manifest.json.sig"), "x");
        let install = ok(read_install(&world.root, &registry));
        assert_eq!(install.kind, InstallKind::Development);
        assert_eq!(install.root, world.root);
        assert_eq!(install.target, "");
        assert_eq!(install.product, "katana");
        assert_eq!(install.version, the_programs_own_version());
        assert_eq!(version::to_string(&install.version), expected_version());
        assert_eq!(install.sequence, 0);
        assert!(!install.has_installed_record);
        assert!(!install.has_installed_manifest);
    }

    #[test]
    fn a_folder_that_does_not_exist_is_a_development_tree_too() {
        let temp = TempDir::new();
        let registry = FakeRegistry::default();
        let install = ok(read_install(&temp.path().join("nothing_here"), &registry));
        assert_eq!(install.kind, InstallKind::Development);
    }

    #[test]
    fn an_install_json_alone_is_a_fresh_install_of_the_named_target() {
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win-arm64"}"#);
        let registry = FakeRegistry::default();
        let install = ok(read_install(&world.root, &registry));
        assert_eq!(install.kind, InstallKind::Archive);
        assert_eq!(install.target, "win-arm64");
        // No installed.json: the version this program was built as, at sequence 0, and no record.
        assert_eq!(install.version, the_programs_own_version());
        assert_eq!(version::to_string(&install.version), expected_version());
        assert_eq!(install.sequence, 0);
        assert!(!install.has_installed_record);
        assert!(!install.has_installed_manifest);
    }

    #[test]
    fn the_fixture_install_trees_are_installs_of_target_win64() {
        for tree in ["v1", "v2"] {
            let registry = FakeRegistry::default();
            let install = read_install(
                &path_from_utf8(format!("{FIXTURES}/install/{tree}")),
                &registry,
            )
            .unwrap_or_else(|error| panic!("{tree}: {}", error.describe()));
            assert_eq!(install.kind, InstallKind::Archive, "{tree}");
            assert_eq!(install.target, "win64", "{tree}");
        }
    }

    #[test]
    fn installed_json_says_which_build_the_helper_put_there() {
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        world.installed_json(
            r#"{"schema":1,"product":"katana","version":"0.9.0","sequence":1790000000,"tag":"v0.9.0","target":"win64"}"#,
        );
        let registry = FakeRegistry::default();
        // Read as a program below the record (0.1.0: 1 < 9), so that the version comes from the
        // record and from nowhere else, whatever version this project has reached.
        let install = ok(read_install_as(
            &world.root,
            &registry,
            &version_of(0, 1, 0, 0, 3),
        ));
        assert_eq!(install.version, version_of(0, 9, 0, 0, 3));
        assert_eq!(
            version::to_string(&install.version),
            "0.9.0",
            "== ignores how many were written"
        );
        assert_eq!(install.sequence, 1790000000);
        assert!(
            install.has_installed_record,
            "exactly when installed.json is there"
        );
    }

    #[test]
    fn versions_of_four_components_and_sequences_beyond_two_to_the_fifty_third_are_read() {
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        world.installed_json(r#"{"schema":1,"version":"1.2.3.4","sequence":18446744073709551615}"#);
        let registry = FakeRegistry::default();
        // As above: a program below the record (0.1.0 against 1.2.3.4: 0 < 1 in the first
        // number).
        let install = ok(read_install_as(
            &world.root,
            &registry,
            &version_of(0, 1, 0, 0, 3),
        ));
        assert_eq!(install.version, version_of(1, 2, 3, 4, 4));
        assert_eq!(
            version::to_string(&install.version),
            "1.2.3.4",
            "== ignores how many were written"
        );
        assert_eq!(install.sequence, u64::MAX);
    }

    #[test]
    fn a_record_below_the_programs_own_version_does_not_lower_the_installed_version() {
        // What a zip unpacked by hand over a copy that had updated itself leaves: the programs of
        // the newer version, and the helper's record of the older one.
        assert!(version_of(0, 1, 0, 0, 3) < the_programs_own_version());
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        world.installed_json(&installed_record_of("0.1.0", 1790000000));
        let registry = FakeRegistry::default();
        let install = ok(read_install(&world.root, &registry));
        assert_eq!(install.version, the_programs_own_version());
        assert_eq!(version::to_string(&install.version), expected_version());
        // The record is still a record: its sequence is the one it names.
        assert_eq!(install.sequence, 1790000000);
        assert!(install.has_installed_record);
    }

    #[test]
    fn a_record_at_or_above_the_programs_own_version_is_the_installed_version() {
        let registry = FakeRegistry::default();
        {
            // The other order: the record names a version above this program's.
            let above = a_version_above_the_program();
            assert!(above > the_programs_own_version());
            let world = Root::new();
            world.install_json(r#"{"schema":1,"target":"win64"}"#);
            world.installed_json(&installed_record_of(
                &version::to_string(&above),
                1790000000,
            ));
            let install = ok(read_install(&world.root, &registry));
            assert_eq!(install.version, above);
            assert_eq!(
                version::to_string(&install.version),
                version::to_string(&above)
            );
            assert_eq!(install.sequence, 1790000000);
            assert!(install.has_installed_record);
        }
        {
            // And the two the same, which is every copy the helper updated and nobody overwrote.
            let world = Root::new();
            world.install_json(r#"{"schema":1,"target":"win64"}"#);
            world.installed_json(&installed_record_of(&expected_version(), 7));
            let install = ok(read_install(&world.root, &registry));
            assert_eq!(install.version, the_programs_own_version());
            assert_eq!(version::to_string(&install.version), expected_version());
            assert_eq!(install.sequence, 7);
            assert!(install.has_installed_record);
        }
    }

    #[test]
    fn the_floor_against_going_back_is_the_higher_version_and_not_a_stale_records() {
        // is_newer_than (trust.rs) is the one place the order is written: a higher version, or the
        // same version with a higher sequence than the record's. Each answer below is that rule
        // applied by hand to an install whose version is this program's and whose record is build
        // 100 of 0.1.0.
        let program = the_programs_own_version();
        assert!(version_of(0, 1, 0, 0, 3) < program);
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        world.installed_json(&installed_record_of("0.1.0", 100));
        let registry = FakeRegistry::default();
        let install = ok(read_install(&world.root, &registry));

        // A later build of the version the record names. Against the record alone it is newer
        // (the same version, a higher sequence) and it is an EARLIER release than the programs
        // that are installed: the way back that the record must not open.
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 1, 0, 0, 3), 200),
            &install
        ));
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 1, 0, 0, 3), 100),
            &install
        ));
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 0, 9, 0, 3), 300),
            &install
        ));

        // What is above the programs is above the install, whatever its sequence.
        assert!(is_newer_than(
            &a_release_of(a_version_above_the_program(), 1),
            &install
        ));

        // The programs' own version: the sequences decide, and the record's is the one there is.
        // (The helper of a release that brings its own helper is that release's version, and must
        // still find the release above the build the record names: install.rs, read_install.)
        assert!(is_newer_than(&a_release_of(program, 200), &install));
        assert!(!is_newer_than(&a_release_of(program, 100), &install));
        assert!(!is_newer_than(&a_release_of(program, 99), &install));
    }

    // ---- the same rule, for a program of any version ------------------------------------------
    //
    // The three cases above read with this build's own version, which is the one thing they cannot
    // choose. read_install_as is the same reading with the program's version handed in
    // (read_install is that, given program_version()), so here both versions are written down and
    // every answer is the rule worked by hand: the installed version is the higher of the two,
    // compared number by number; the sequence, and that there is a record, are the record's alone.

    #[test]
    fn of_a_record_and_a_program_the_higher_version_is_the_installed_one_in_both_orders() {
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        world.installed_json(&installed_record_of("0.4.0", 400));
        let registry = FakeRegistry::default();
        let cases = [
            (version_of(0, 3, 9, 0, 3), "0.4.0"), // the record is the higher: 4 > 3
            (version_of(0, 4, 0, 0, 3), "0.4.0"), // the two the same
            (version_of(0, 4, 1, 0, 3), "0.4.1"), // the program is the higher, by its last number
            (version_of(0, 10, 0, 0, 3), "0.10.0"), // by number and not by text: 10 > 4
            (version_of(1, 0, 0, 0, 2), "1.0"),   // by its first number, and written with two
        ];
        for (program, installed) in cases {
            let install = read_install_as(&world.root, &registry, &program)
                .unwrap_or_else(|error| panic!("{installed}: {}", error.describe()));
            assert_eq!(
                version::to_string(&install.version),
                installed,
                "for a program of {}",
                version::to_string(&program)
            );
            // Whichever version it is, the record is still the record.
            assert_eq!(install.sequence, 400, "{installed}");
            assert!(install.has_installed_record, "{installed}");
            assert_eq!(install.target, "win64", "{installed}");
            assert_eq!(install.kind, InstallKind::Archive, "{installed}");
        }
    }

    #[test]
    fn with_no_record_of_the_helpers_the_installed_version_is_the_programs_whatever_that_is() {
        let registry = FakeRegistry::default();
        let program = version_of(7, 8, 9, 0, 3);
        {
            // A fresh install: install.json and nothing the helper wrote.
            let world = Root::new();
            world.install_json(r#"{"schema":1,"target":"win64"}"#);
            let install = ok(read_install_as(&world.root, &registry, &program));
            assert_eq!(install.kind, InstallKind::Archive);
            assert_eq!(version::to_string(&install.version), "7.8.9");
            assert_eq!(install.sequence, 0);
            assert!(!install.has_installed_record);
        }
        {
            // A development tree, with a record somebody copied into it: the record is not read at
            // all there, so it does not raise the version either (9.9.9 is above 7.8.9).
            let world = Root::new();
            world.installed_json(&installed_record_of("9.9.9", 5));
            let install = ok(read_install_as(&world.root, &registry, &program));
            assert_eq!(install.kind, InstallKind::Development);
            assert_eq!(version::to_string(&install.version), "7.8.9");
            assert_eq!(install.sequence, 0);
            assert!(!install.has_installed_record);
        }
    }

    #[test]
    fn a_stale_record_under_newer_programs_lets_no_release_between_the_two_back_in() {
        // The case the rule is for, with its versions written down. A copy updated itself to 0.3.0
        // (build 300), and 0.5.0 was then unpacked over it by hand: the programs are 0.5.0 and the
        // helper's record still says 0.3.0. is_newer_than (trust.rs) is the one place the order is
        // written (a higher version, or the same version with a higher sequence than the
        // record's), and each answer below is that rule applied by hand to an install of version
        // 0.5.0 whose record is build 300.
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        world.installed_json(&installed_record_of("0.3.0", 300));
        let registry = FakeRegistry::default();
        let install = ok(read_install_as(
            &world.root,
            &registry,
            &version_of(0, 5, 0, 0, 3),
        ));
        assert_eq!(version::to_string(&install.version), "0.5.0");

        // 0.4.0 is a release between the two: above the record (4 > 3), below the programs
        // (4 < 5). It is signed and it is the way back.
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 4, 0, 0, 3), 400),
            &install
        ));
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 4, 9, 0, 3), 490),
            &install
        ));
        // The release the record names, and a later build of it.
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 3, 0, 0, 3), 300),
            &install
        ));
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 3, 0, 0, 3), 310),
            &install
        ));
        // What is above the programs is an update, whatever its sequence.
        assert!(is_newer_than(
            &a_release_of(version_of(0, 5, 1, 0, 3), 510),
            &install
        ));
        assert!(is_newer_than(
            &a_release_of(version_of(0, 6, 0, 0, 3), 1),
            &install
        ));
        // The programs' own version: the sequences decide, and the record's is 300.
        assert!(is_newer_than(
            &a_release_of(version_of(0, 5, 0, 0, 3), 500),
            &install
        ));
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 5, 0, 0, 3), 300),
            &install
        ));

        // And the same record under the programs it names: 0.4.0 is then the update it is.
        let before = ok(read_install_as(
            &world.root,
            &registry,
            &version_of(0, 3, 0, 0, 3),
        ));
        assert_eq!(version::to_string(&before.version), "0.3.0");
        assert!(is_newer_than(
            &a_release_of(version_of(0, 4, 0, 0, 3), 400),
            &before
        ));
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 3, 0, 0, 3), 300),
            &before
        ));
    }

    // ---- a package that names its own build ---------------------------------------------------
    //
    // Every install.json above says its target and no more, and the program that asks then stands
    // for the copy. install.json may also say what the package IS (install.rs, package_build): the
    // version it was built as and, for a package of a release, the sequence of that release. The
    // copy's own build is then the package's word, and the asking program is not asked. Each
    // answer below is that rule worked by hand, with the order is_newer_than (trust.rs) writes:
    // with no record of the helper's, the installed version is the package's, the sequence is the
    // placeholder 0 and there is no record; with one, the installed build is the higher of the
    // record's and the package's (by version first, number by number, and with the versions equal
    // by sequence). A package that names no sequence raises the version alone, and the record's
    // sequence stays.

    #[test]
    fn a_fresh_install_that_names_its_version_is_that_version_whichever_program_reads_it() {
        let world = Root::new();
        world.install_json(&package_record_of(r#""version":"0.3.0","sequence":300"#));
        let registry = FakeRegistry::default();
        // A program below the package, one of it, and one above it, which is what the helper of a
        // later release is, when that release brings its own.
        for program in [
            version_of(0, 2, 0, 0, 3),
            version_of(0, 3, 0, 0, 3),
            version_of(0, 5, 0, 0, 3),
        ] {
            let install = ok(read_install_as(&world.root, &registry, &program));
            assert_eq!(install.kind, InstallKind::Archive);
            assert_eq!(install.target, "win64");
            assert_eq!(
                version::to_string(&install.version),
                "0.3.0",
                "for a program of {}",
                version::to_string(&program)
            );
            assert_eq!(
                install.sequence, 0,
                "nothing the helper wrote: the placeholder"
            );
            assert!(!install.has_installed_record);
        }
    }

    #[test]
    fn a_releases_own_helper_finds_its_release_newer_where_the_package_names_its_version() {
        // The helper that installs a release is that release's OWN when the release needs a newer
        // helper than the installed one (the stage's preflight). So the program that reads the
        // install is then of the version it is about to install: here a helper of 0.5.0, asked to
        // install 0.5.0 (build 500).
        let helper = version_of(0, 5, 0, 0, 3);
        let release = a_release_of(version_of(0, 5, 0, 0, 3), 500);
        let registry = FakeRegistry::default();
        {
            // A copy of 0.3.0 that never updated, and says so: 0.5.0 is above it (5 > 3).
            let world = Root::new();
            world.install_json(&package_record_of(r#""version":"0.3.0","sequence":300"#));
            let install = ok(read_install_as(&world.root, &registry, &helper));
            assert_eq!(version::to_string(&install.version), "0.3.0");
            assert!(is_newer_than(&release, &install));
        }
        {
            // The version alone is enough for that: a package built outside the release workflow.
            let world = Root::new();
            world.install_json(&package_record_of(r#""version":"0.3.0""#));
            let install = ok(read_install_as(&world.root, &registry, &helper));
            assert_eq!(version::to_string(&install.version), "0.3.0");
            assert!(is_newer_than(&release, &install));
        }
        {
            // A copy that updated itself to 0.3.1, a later build of an older line: its sequence,
            // 600, is ABOVE the release's 500. The install is 0.3.1 and 0.5.0 is above it by its
            // version (5 > 3), where the sequences are not asked.
            let world = Root::new();
            world.install_json(&package_record_of(r#""version":"0.3.0","sequence":300"#));
            world.installed_json(&installed_record_of("0.3.1", 600));
            let install = ok(read_install_as(&world.root, &registry, &helper));
            assert_eq!(version::to_string(&install.version), "0.3.1");
            assert_eq!(install.sequence, 600);
            assert!(is_newer_than(&release, &install));
        }
    }

    #[test]
    fn a_releases_own_helper_refuses_a_copy_whose_package_names_no_version() {
        // THE LIMIT of a package that says its target and no more, written down so that the first
        // release to raise the helper's protocol does not meet it in the field. Such a copy is
        // stood for by the program that asks, and to a release's own helper that is the version
        // it is about to install: 0.5.0, build 500, as in the case above.
        let helper = version_of(0, 5, 0, 0, 3);
        let release = a_release_of(version_of(0, 5, 0, 0, 3), 500);
        let registry = FakeRegistry::default();
        {
            // A copy that never updated: the same version, and no record whose sequence could tell
            // the two builds apart, is not newer (is_newer_than): the helper's `downgrade`.
            let world = Root::new();
            world.install_json(r#"{"schema":1,"target":"win64"}"#);
            let install = ok(read_install_as(&world.root, &registry, &helper));
            assert_eq!(version::to_string(&install.version), "0.5.0");
            assert!(!install.has_installed_record);
            assert!(!is_newer_than(&release, &install));
        }
        {
            // A copy that updated itself to 0.3.1 at sequence 600: the version is raised to the
            // helper's 0.5.0 and the record's sequence stays, and 500 is not above 600.
            let world = Root::new();
            world.install_json(r#"{"schema":1,"target":"win64"}"#);
            world.installed_json(&installed_record_of("0.3.1", 600));
            let install = ok(read_install_as(&world.root, &registry, &helper));
            assert_eq!(version::to_string(&install.version), "0.5.0");
            assert_eq!(install.sequence, 600);
            assert!(!is_newer_than(&release, &install));
        }
        {
            // What does pass with such a package: a record of an earlier sequence than the
            // release's (300 < 500), which is every copy whose releases were built in the order of
            // their versions.
            let world = Root::new();
            world.install_json(r#"{"schema":1,"target":"win64"}"#);
            world.installed_json(&installed_record_of("0.3.0", 300));
            let install = ok(read_install_as(&world.root, &registry, &helper));
            assert!(is_newer_than(&release, &install));
        }
    }

    #[test]
    fn a_second_build_unpacked_over_an_updated_copy_brings_its_own_floor_within_its_version() {
        // A copy updated itself to 0.3.0 (build 300). 0.5.0 was built twice: build 500 was
        // withdrawn and build 550 took its place. The zip of build 550 is unpacked over the copy by
        // hand, so the helper's record still says 0.3.0 at 300 and the zip's install.json says
        // 0.5.0 at 550. The installed build is the higher of the two: 0.5.0 (5 > 3), at 550.
        let world = Root::new();
        world.install_json(&package_record_of(r#""version":"0.5.0","sequence":550"#));
        world.installed_json(&installed_record_of("0.3.0", 300));
        let registry = FakeRegistry::default();
        let program = version_of(0, 5, 0, 0, 3);
        let install = ok(read_install_as(&world.root, &registry, &program));
        assert_eq!(version::to_string(&install.version), "0.5.0");
        assert_eq!(install.sequence, 550);
        assert!(
            install.has_installed_record,
            "the helper's record is there, stale as it is"
        );

        // The withdrawn build is signed, and it is the way back: the same version, and 500 < 550.
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 5, 0, 0, 3), 500),
            &install
        ));
        // The build that is installed is no update of itself; a third build (551 > 550) is one.
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 5, 0, 0, 3), 550),
            &install
        ));
        assert!(is_newer_than(
            &a_release_of(version_of(0, 5, 0, 0, 3), 551),
            &install
        ));
        // Below the programs, whatever the sequence: the release between the two, and a later
        // build of the one the record names.
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 4, 0, 0, 3), 9999),
            &install
        ));
        assert!(!is_newer_than(
            &a_release_of(version_of(0, 3, 0, 0, 3), 9999),
            &install
        ));
        // Above them, whatever the sequence.
        assert!(is_newer_than(
            &a_release_of(version_of(0, 5, 1, 0, 3), 1),
            &install
        ));

        // The same copy where the zip says its target and no more. The program stands for it and
        // a program has no sequence: the version rises to 0.5.0, the sequence stays the record's
        // 300, and the withdrawn build (500 > 300) is let back in. That is what the package's own
        // sequence closes.
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        let unnamed = ok(read_install_as(&world.root, &registry, &program));
        assert_eq!(version::to_string(&unnamed.version), "0.5.0");
        assert_eq!(unnamed.sequence, 300);
        assert!(is_newer_than(
            &a_release_of(version_of(0, 5, 0, 0, 3), 500),
            &unnamed
        ));
    }

    #[test]
    fn of_the_helpers_record_and_the_packages_own_build_the_higher_is_the_installed_one() {
        let registry = FakeRegistry::default();
        // The helper's record is build 400 of 0.4.0 throughout. (What install.json says after its
        // target, the installed version, the installed sequence.)
        let cases: [(&str, &str, u64); 9] = [
            // The record is the higher, by version (4 > 3): a sequence under a lower version
            // counts for nothing.
            (r#""version":"0.3.9","sequence":999"#, "0.4.0", 400),
            // The versions equal: the later build of the two.
            (r#""version":"0.4.0","sequence":399"#, "0.4.0", 400),
            (r#""version":"0.4.0","sequence":400"#, "0.4.0", 400),
            (r#""version":"0.4.0","sequence":401"#, "0.4.0", 401),
            // The versions equal and the package names no sequence: the record's is all there is.
            (r#""version":"0.4.0""#, "0.4.0", 400),
            // The package is the higher, by version: its own sequence with it, lower as it is.
            (r#""version":"0.4.1","sequence":100"#, "0.4.1", 100),
            // By number and not by text: 10 > 4.
            (r#""version":"0.10.0","sequence":1000"#, "0.10.0", 1000),
            // By its first number, and written with two.
            (r#""version":"1.0","sequence":7"#, "1.0", 7),
            // The higher version with no sequence of its own: the record's stays.
            (r#""version":"0.5.0""#, "0.5.0", 400),
        ];
        for (package, installed_version, installed_sequence) in cases {
            let world = Root::new();
            world.install_json(&package_record_of(package));
            world.installed_json(&installed_record_of("0.4.0", 400));
            // A program below every version here and one above every one: neither is asked.
            for program in [version_of(0, 1, 0, 0, 3), version_of(9, 0, 0, 0, 3)] {
                let install = read_install_as(&world.root, &registry, &program)
                    .unwrap_or_else(|error| panic!("{package}: {}", error.describe()));
                assert_eq!(
                    version::to_string(&install.version),
                    installed_version,
                    "{package} for a program of {}",
                    version::to_string(&program)
                );
                assert_eq!(install.sequence, installed_sequence, "{package}");
                assert!(install.has_installed_record, "{package}");
                assert_eq!(install.target, "win64", "{package}");
            }
        }
    }

    #[test]
    fn a_packages_own_build_of_four_components_and_a_sequence_beyond_two_to_the_fifty_third_is_read()
     {
        let world = Root::new();
        world.install_json(&package_record_of(
            r#""version":"1.2.3.4","sequence":18446744073709551615"#,
        ));
        world.installed_json(&installed_record_of("1.2.3.4", 5));
        let registry = FakeRegistry::default();
        let install = ok(read_install_as(
            &world.root,
            &registry,
            &version_of(0, 1, 0, 0, 3),
        ));
        assert_eq!(install.version, version_of(1, 2, 3, 4, 4));
        assert_eq!(version::to_string(&install.version), "1.2.3.4");
        assert_eq!(
            install.sequence,
            u64::MAX,
            "the same version: the later build, 2^64 - 1"
        );
    }

    #[test]
    fn an_install_json_whose_own_build_does_not_read_is_refused() {
        let cases = [
            r#""version":5"#, // not text
            r#""version":null"#,
            r#""version":"""#,
            r#""version":"nine""#,
            r#""version":"1""#, // one component
            r#""version":"0.3.0","sequence":-1"#,
            r#""version":"0.3.0","sequence":"5""#,
            r#""version":"0.3.0","sequence":1.5"#,
            r#""version":"0.3.0","sequence":null"#,
            r#""version":"0.3.0","sequence":18446744073709551616"#, // 2^64
            r#""sequence":5"#, // a sequence, and no version it could belong to
        ];
        let registry = FakeRegistry::default();
        for build in cases {
            // With and without a record of the helper's beside it: the copy is not read either way.
            for updated in [false, true] {
                let world = Root::new();
                world.install_json(&package_record_of(build));
                if updated {
                    world.installed_json(&installed_record_of("0.9.0", 5));
                }
                let error = read_install_as(&world.root, &registry, &version_of(0, 3, 0, 0, 3))
                    .expect_err(build);
                assert_eq!(error.code, ErrorCode::ParseFailure, "{build}");
                assert!(
                    error.context.contains("install.json"),
                    "the file is named: {}",
                    error.context
                );
            }
        }
    }

    #[test]
    fn the_installed_manifest_counts_only_with_its_signature_beside_it() {
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        let registry = FakeRegistry::default();
        let read = || ok(read_install(&world.root, &registry));
        assert!(!read().has_installed_manifest);
        write_file(&world.records.join("installed.manifest.json"), "{}");
        assert!(
            !read().has_installed_manifest,
            "a manifest with no signature is not one to delete by"
        );
        write_file(
            &world.records.join("installed.manifest.json.sig"),
            "ed25519 k s",
        );
        assert!(read().has_installed_manifest);
        let _ = std::fs::remove_file(world.records.join("installed.manifest.json"));
        assert!(!read().has_installed_manifest);
        // A folder is not a manifest.
        let _ = std::fs::create_dir(world.records.join("installed.manifest.json"));
        assert!(!read().has_installed_manifest);
    }

    fn kind_of(world: &Root, registry: &FakeRegistry) -> InstallKind {
        ok(read_install(&world.root, registry)).kind
    }

    #[test]
    fn the_registry_decides_between_installer_and_archive() {
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        let mut registry = FakeRegistry::default();

        assert_eq!(
            kind_of(&world, &registry),
            InstallKind::Archive,
            "no installer entry"
        );
        registry.location = Some(world.root.clone());
        assert_eq!(kind_of(&world, &registry), InstallKind::Installer);
        registry.location = Some(world.root.join("bin").join(".."));
        assert_eq!(
            kind_of(&world, &registry),
            InstallKind::Installer,
            "the same folder spelled another way"
        );
        let mut trailing = world.root.clone().into_os_string();
        trailing.push("/"); // a trailing separator
        registry.location = Some(PathBuf::from(trailing));
        assert_eq!(kind_of(&world, &registry), InstallKind::Installer);
        registry.location = Some(world.temp.path().join("somewhere_else"));
        assert_eq!(
            kind_of(&world, &registry),
            InstallKind::Archive,
            "the installer put another copy elsewhere"
        );
        registry.location = Some(world.temp.path().to_path_buf());
        assert_eq!(
            kind_of(&world, &registry),
            InstallKind::Archive,
            "a parent folder is not the folder"
        );
        registry.location = Some(world.root.clone());
        registry.fails = true;
        assert_eq!(
            kind_of(&world, &registry),
            InstallKind::Archive,
            "a registry that cannot be read is no reason to refuse"
        );
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_installer_folder_may_differ_in_case_from_the_root() {
        let world = Root::new();
        world.install_json(r#"{"schema":1,"target":"win64"}"#);
        let upper = world
            .root
            .to_str()
            .expect("a scratch folder of Unicode")
            .to_ascii_uppercase();
        let registry = FakeRegistry {
            location: Some(PathBuf::from(upper)),
            ..FakeRegistry::default()
        };
        assert_eq!(kind_of(&world, &registry), InstallKind::Installer);
    }

    #[test]
    fn an_install_json_that_is_not_what_the_cpack_install_writes_is_refused() {
        let cases = [
            ("", ErrorCode::ParseFailure),
            ("{", ErrorCode::ParseFailure),
            ("[]", ErrorCode::ParseFailure),
            ("null", ErrorCode::ParseFailure),
            (r#"{"target":"win64"}"#, ErrorCode::ParseFailure), // no schema
            (
                r#"{"schema":"1","target":"win64"}"#,
                ErrorCode::ParseFailure,
            ), // schema is text
            (
                r#"{"schema":1.0,"target":"win64"}"#,
                ErrorCode::ParseFailure,
            ), // not an integer
            (r#"{"schema":1}"#, ErrorCode::ParseFailure),       // no target
            (r#"{"schema":1,"target":5}"#, ErrorCode::ParseFailure),
            (r#"{"schema":1,"target":""}"#, ErrorCode::ParseFailure),
            (r#"{"schema":1,"target":"Win64"}"#, ErrorCode::ParseFailure), // upper case
            (r#"{"schema":1,"target":"win 64"}"#, ErrorCode::ParseFailure),
            (
                r#"{"schema":1,"target":"win64\n"}"#,
                ErrorCode::ParseFailure,
            ),
            // It becomes part of a URL.
            (
                r#"{"schema":1,"target":"../win64"}"#,
                ErrorCode::ParseFailure,
            ),
            (r#"{"schema":1,"target":"a/b"}"#, ErrorCode::ParseFailure),
            (
                r#"{"schema":1,"target":"win64?x=1"}"#,
                ErrorCode::ParseFailure,
            ),
            // 33 characters.
            (
                r#"{"schema":1,"target":"abcdefghijklmnopqrstuvwxyz0123456"}"#,
                ErrorCode::ParseFailure,
            ),
            (r#"{"schema":2,"target":"win64"}"#, ErrorCode::Unsupported),
            (r#"{"schema":0,"target":"win64"}"#, ErrorCode::Unsupported),
            (r#"{"schema":-1,"target":"win64"}"#, ErrorCode::Unsupported),
        ];
        for (text, code) in cases {
            let world = Root::new();
            world.install_json(text);
            let registry = FakeRegistry::default();
            let error = read_install(&world.root, &registry).expect_err(text);
            assert_eq!(error.code, code, "{text}");
            assert!(
                error.context.contains("install.json"),
                "the file is named: {}",
                error.context
            );
        }
    }

    #[test]
    fn a_target_of_thirty_two_characters_and_the_five_real_ones_are_accepted() {
        for target in [
            "win64",
            "win-arm64",
            "linux-x86_64",
            "linux-aarch64",
            "macos-arm64",
            "abcdefghijklmnopqrstuvwxyz012345",
        ] {
            let world = Root::new();
            world.install_json(&format!(r#"{{"schema":1,"target":"{target}"}}"#));
            let registry = FakeRegistry::default();
            let install = read_install(&world.root, &registry)
                .unwrap_or_else(|error| panic!("{target}: {}", error.describe()));
            assert_eq!(install.target, target);
        }
    }

    #[test]
    fn members_an_install_json_has_beyond_the_ones_read_are_ignored() {
        let world = Root::new();
        world.install_json(
            r#"{"schema":1,"target":"win64","written_by":"a newer packaging","extra":[1,2]}"#,
        );
        let registry = FakeRegistry::default();
        assert!(read_install(&world.root, &registry).is_ok());
    }

    #[test]
    fn an_installed_json_that_is_not_what_the_helper_writes_is_refused() {
        let cases = [
            ("", ErrorCode::ParseFailure),
            ("{", ErrorCode::ParseFailure),
            (r#"{"schema":1,"sequence":5}"#, ErrorCode::ParseFailure), // no version
            (r#"{"schema":1,"version":"0.9.0"}"#, ErrorCode::ParseFailure), // no sequence
            (
                r#"{"schema":1,"version":5,"sequence":5}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":1,"version":"nine","sequence":5}"#,
                ErrorCode::ParseFailure,
            ),
            // One component.
            (
                r#"{"schema":1,"version":"1","sequence":5}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":1,"version":"0.9.0","sequence":-1}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":1,"version":"0.9.0","sequence":"5"}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":1,"version":"0.9.0","sequence":1.5}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":1,"version":"0.9.0","sequence":18446744073709551616}"#,
                ErrorCode::ParseFailure,
            ),
            // No schema.
            (
                r#"{"version":"0.9.0","sequence":5}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":2,"version":"0.9.0","sequence":5}"#,
                ErrorCode::Unsupported,
            ),
        ];
        for (text, code) in cases {
            let world = Root::new();
            world.install_json(r#"{"schema":1,"target":"win64"}"#);
            world.installed_json(text);
            let registry = FakeRegistry::default();
            let error = read_install(&world.root, &registry).expect_err(text);
            assert_eq!(error.code, code, "{text}");
            assert!(
                error.context.contains("installed.json"),
                "{}",
                error.context
            );
        }
    }

    #[test]
    fn a_record_longer_than_any_this_program_writes_is_not_read_whole() {
        let world = Root::new();
        world.install_json(&format!(
            r#"{{"schema":1,"target":"win64","pad":"{}"}}"#,
            "x".repeat(70 * 1024)
        ));
        let registry = FakeRegistry::default();
        let error = read_install(&world.root, &registry).expect_err("a record of 70 KiB");
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn a_record_that_is_a_folder_is_an_error_not_a_development_tree() {
        let world = Root::new();
        std::fs::create_dir(world.records.join("install.json")).expect("a folder");
        let registry = FakeRegistry::default();
        let error = read_install(&world.root, &registry).expect_err(
            "an install.json that cannot be read must not turn a real install into a dev tree",
        );
        assert_eq!(error.code, ErrorCode::FileImportFailure);
    }

    #[test]
    fn no_root_is_an_invalid_argument() {
        let registry = FakeRegistry::default();
        let error = read_install(Path::new(""), &registry).expect_err("no root");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    // ---- the root of the running program ------------------------------------------------------

    #[test]
    fn the_running_program_is_not_inside_an_install_when_it_is_a_test_in_the_build_tree() {
        // The test program is cargo's, target/<profile>/deps/state-<hash>: its folder is "deps",
        // not "bin", so it is in no install (the layout rules are tested with the contract's
        // cases). What this holds is that the function found THIS program's path: the error names
        // it. (The C++'s program is katana_update_tests; this one is named after its file and
        // cargo's hash of it.)
        let error = install_root_of_running_program().expect_err("unexpectedly inside an install");
        assert_eq!(error.code, ErrorCode::NotFound);
        let named = path_from_utf8(&error.context);
        assert!(named.is_absolute(), "{}", error.context);
        assert!(named.exists(), "{}", error.context);
        let stem = named
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert!(
            stem == "state" || stem.starts_with("state-"),
            "{}",
            error.context
        );
    }
}

// ---- the state folder ----------------------------------------------------------------------------

mod state_folder {
    use super::*;

    #[test]
    fn the_environment_variable_says_where_it_is_and_nothing_else_moves_it() {
        let _lock = environment_lock();
        let temp = TempDir::new();
        let chosen = temp.path().join("chosen").join("folder");
        let _variable = EnvGuard::new("KATANA_UPDATE_STATE_DIR", Some(chosen.as_path()));
        assert_eq!(state_directory(), chosen);
        // Not created by asking.
        assert!(!chosen.exists());
    }

    #[test]
    fn a_relative_value_is_made_absolute_at_once() {
        let _lock = environment_lock();
        let relative = Path::new("relative").join("dir");
        let _variable = EnvGuard::new("KATANA_UPDATE_STATE_DIR", Some(relative.as_path()));
        let state = state_directory();
        assert!(state.is_absolute(), "{}", text_of(&state));
        assert_eq!(
            state,
            std::path::absolute(&relative).expect("an absolute path")
        );
    }

    #[test]
    fn an_empty_variable_is_as_good_as_none() {
        let _lock = environment_lock();
        let _variable = EnvGuard::new("KATANA_UPDATE_STATE_DIR", Some(Path::new("")));
        let state = state_directory();
        assert!(
            !state.as_os_str().is_empty(),
            "falls back to the platform's place on any machine with a home"
        );
        assert_eq!(
            state.file_name().and_then(|name| name.to_str()),
            Some("update")
        );
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_it_is_local_app_data_katana_update() {
        let _lock = environment_lock();
        let temp = TempDir::new();
        let _none = EnvGuard::new("KATANA_UPDATE_STATE_DIR", None);
        let _local = EnvGuard::new("LOCALAPPDATA", Some(temp.path().join("Local").as_path()));
        assert_eq!(
            state_directory(),
            temp.path().join("Local").join("Katana").join("update")
        );
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_without_local_app_data_it_is_under_the_profile() {
        let _lock = environment_lock();
        let temp = TempDir::new();
        let _none = EnvGuard::new("KATANA_UPDATE_STATE_DIR", None);
        let _local = EnvGuard::new("LOCALAPPDATA", None);
        let _profile = EnvGuard::new("USERPROFILE", Some(temp.path().join("profile").as_path()));
        assert_eq!(
            state_directory(),
            temp.path()
                .join("profile")
                .join("AppData")
                .join("Local")
                .join("Katana")
                .join("update")
        );
        let _no_profile = EnvGuard::new("USERPROFILE", None);
        assert!(
            state_directory().as_os_str().is_empty(),
            "no place to put it: an empty path, not the working folder"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn on_linux_it_is_xdg_state_home_katana_update() {
        let _lock = environment_lock();
        let temp = TempDir::new();
        let _none = EnvGuard::new("KATANA_UPDATE_STATE_DIR", None);
        let _xdg = EnvGuard::new("XDG_STATE_HOME", Some(temp.path().join("state").as_path()));
        assert_eq!(
            state_directory(),
            temp.path().join("state").join("katana").join("update")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn on_linux_a_relative_xdg_value_is_ignored_as_the_specification_says() {
        let _lock = environment_lock();
        let temp = TempDir::new();
        let _none = EnvGuard::new("KATANA_UPDATE_STATE_DIR", None);
        let _xdg = EnvGuard::new("XDG_STATE_HOME", Some(Path::new("relative")));
        let _home = EnvGuard::new("HOME", Some(temp.path().join("home").as_path()));
        assert_eq!(
            state_directory(),
            temp.path()
                .join("home")
                .join(".local")
                .join("state")
                .join("katana")
                .join("update")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn on_linux_without_xdg_it_is_under_home_local_state() {
        let _lock = environment_lock();
        let temp = TempDir::new();
        let _none = EnvGuard::new("KATANA_UPDATE_STATE_DIR", None);
        let _xdg = EnvGuard::new("XDG_STATE_HOME", None);
        let _home = EnvGuard::new("HOME", Some(temp.path().join("home").as_path()));
        assert_eq!(
            state_directory(),
            temp.path()
                .join("home")
                .join(".local")
                .join("state")
                .join("katana")
                .join("update")
        );
        let _no_home = EnvGuard::new("HOME", None);
        assert!(state_directory().as_os_str().is_empty());
    }

    #[test]
    fn the_names_inside_it_are_the_ones_the_header_lists_and_nothing_is_created() {
        let u = Path::new("U");
        assert_eq!(stage_path(u, 1792312800), u.join("stage-1792312800"));
        assert_eq!(stage_path(u, 0), u.join("stage-0"));
        assert_eq!(
            stage_path(u, u64::MAX),
            u.join("stage-18446744073709551615")
        );
        assert_eq!(run_path(u, "3fa9"), u.join("run-3fa9"));
        assert_eq!(health_marker_path(u, "3fa9"), u.join("healthy-3fa9"));
        assert!(!u.exists());
    }

    #[test]
    fn no_state_folder_is_refused_by_every_function_and_is_never_the_working_folder() {
        // state_directory() is an empty path on a machine with no home. Joined with a name that is
        // a file in the working folder; the folder here holds the very files an unguarded call
        // would read, replace, remove or act on.
        let _lock = environment_lock();
        let temp = TempDir::new();
        let disk = DiskFs::default();
        let victim = temp.path().join("victim.txt");
        write_file(&victim, "x");
        write_file(
            &temp.path().join("state.json"),
            r#"{"schema":1,"auto":false}"#,
        );
        write_file(
            &temp.path().join("last.json"),
            r#"{"schema":1,"status":"rolled_back"}"#,
        );
        write_file(&temp.path().join("cleanup.list"), line_of(&victim));
        let state_before = read_file(&temp.path().join("state.json"));
        let last_before = read_file(&temp.path().join("last.json"));
        let list_before = read_file(&temp.path().join("cleanup.list"));

        let _here = CwdGuard::enter(temp.path());
        let none = Path::new("");
        let expect_refused = |status: Status, what: &str| {
            let error = status.expect_err(what);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{what}");
        };
        {
            let error = read_state(none).expect_err("read ./state.json as if it were the state");
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
        {
            let error =
                read_last_result(none).expect_err("read ./last.json as if it were the last result");
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
        expect_refused(write_state(none, &UpdateState::default()), "write_state");
        expect_refused(
            write_last_result(
                none,
                &LastResult {
                    status: String::from("committed"),
                    ..LastResult::default()
                },
            ),
            "write_last_result",
        );
        expect_refused(clear_last_result(none), "clear_last_result");
        expect_refused(
            append_cleanup(none, &temp.path().join("leftover.bin"), &disk),
            "append_cleanup",
        );
        expect_refused(
            process_cleanup_list(none, &temp.path().join("R"), &disk),
            "process_cleanup_list",
        );
        expect_refused(
            process_cleanup_list(none, Path::new(""), &disk),
            "process_cleanup_list with no root",
        );

        assert!(victim.exists(), "a list in the working folder was believed");
        assert_eq!(read_file(&temp.path().join("state.json")), state_before);
        assert_eq!(read_file(&temp.path().join("last.json")), last_before);
        assert_eq!(read_file(&temp.path().join("cleanup.list")), list_before);
        assert_eq!(entries_in(temp.path()), 4, "and nothing was created");
    }
}

// ---- state.json ----------------------------------------------------------------------------------

// Whether `text` holds `parts` in order with only white space between them (the C++'s regular
// expressions' `\s*`) and, when `then_separator`, a ',' or white space straight after the last
// (their `[,\s]`).
fn holds(text: &str, parts: &[&str], then_separator: bool) -> bool {
    let bytes = text.as_bytes();
    'start: for (start, _) in text.match_indices(parts[0]) {
        let mut at = start + parts[0].len();
        for part in &parts[1..] {
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if !text[at..].starts_with(part) {
                continue 'start;
            }
            at += part.len();
        }
        if !then_separator
            || bytes
                .get(at)
                .is_some_and(|&c| c == b',' || c.is_ascii_whitespace())
        {
            return true;
        }
    }
    false
}

fn read_text(path: &Path) -> String {
    String::from_utf8_lossy(&read_file(path)).into_owned()
}

mod state_file {
    use super::*;

    #[test]
    fn no_file_is_the_defaults_and_not_an_error() {
        let temp = TempDir::new();
        for folder in [temp.path().join("never_made"), temp.path().to_path_buf()] {
            let state = ok(read_state(&folder));
            assert_eq!(state.last_check_epoch, 0);
            assert!(state.automatic, "the automatic check is on by default");
            assert_eq!(state.skipped_version, "");
            assert!(state.blocked_sequences.is_empty());
        }
        assert!(
            !temp.path().join("never_made").exists(),
            "reading creates nothing"
        );
    }

    #[test]
    fn every_field_comes_back_as_it_went_in() {
        let temp = TempDir::new();
        let folder = temp.path().join("nested").join("deeper").join("U");
        let state = UpdateState {
            last_check_epoch: 1790000000,
            automatic: false,
            skipped_version: String::from("0.3.0"),
            blocked_sequences: vec![1792312800, 7, u64::MAX],
        };
        assert!(write_state(&folder, &state).is_ok());

        let back = ok(read_state(&folder));
        assert_eq!(back.last_check_epoch, 1790000000);
        assert!(!back.automatic);
        assert_eq!(back.skipped_version, "0.3.0");
        assert_eq!(back.blocked_sequences, vec![1792312800, 7, u64::MAX]);
    }

    #[test]
    fn the_defaults_and_the_extremes_round_trip_too() {
        let temp = TempDir::new();
        let defaults = UpdateState::default();
        assert!(write_state(temp.path(), &defaults).is_ok());
        let back = ok(read_state(temp.path()));
        assert_eq!(back.last_check_epoch, 0);
        assert!(back.automatic);
        assert!(back.blocked_sequences.is_empty());

        let mut extremes = UpdateState {
            last_check_epoch: i64::MAX,
            // UTF-8 (e acute, C3 A9) and JSON's escapes.
            skipped_version: String::from("caf\u{e9} \"quoted\" \\ back\nslash"),
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &extremes).is_ok());
        let back = ok(read_state(temp.path()));
        assert_eq!(back.last_check_epoch, i64::MAX);
        assert_eq!(back.skipped_version, extremes.skipped_version);
        extremes.last_check_epoch = i64::MIN;
        assert!(write_state(temp.path(), &extremes).is_ok());
        assert_eq!(ok(read_state(temp.path())).last_check_epoch, i64::MIN);
    }

    #[test]
    fn the_file_is_the_documented_json() {
        let temp = TempDir::new();
        let state = UpdateState {
            last_check_epoch: 1790000000,
            automatic: false,
            skipped_version: String::from("0.3.0"),
            blocked_sequences: vec![5, 6],
        };
        assert!(write_state(temp.path(), &state).is_ok());
        let text = read_text(&temp.path().join("state.json"));
        let patterns: [(&[&str], bool); 5] = [
            (&["\"schema\"", ":", "1"], true),
            (&["\"last_check\"", ":", "1790000000"], true),
            (&["\"auto\"", ":", "false"], true),
            (&["\"skipped\"", ":", "\"0.3.0\""], true),
            (&["\"blocked\"", ":", "[", "5", ",", "6", "]"], false),
        ];
        for (parts, then_separator) in patterns {
            assert!(holds(&text, parts, then_separator), "{parts:?}\n{text}");
        }
    }

    // Not a twin: the exact text, byte for byte, that nlohmann::json's dump(2) writes for these
    // records in the C++ (serializer.hpp of the toolchain's nlohmann 3.12: the members in key
    // order, two spaces of indent, `": "`, an empty list as `[]`; \b \t \n \f \r \" \\ escaped,
    // every other byte below 0x20 as \u00xx in lower case, and '/', DEL and every byte beyond
    // ASCII as they are), worked out by hand from those rules. The window and the helper of the
    // C++ read what the Rust writes, so the two texts must not differ.
    #[test]
    fn the_records_are_the_text_nlohmann_writes() {
        let temp = TempDir::new();
        let state = UpdateState {
            last_check_epoch: 1790000000,
            automatic: false,
            skipped_version: String::from("0.3.0"),
            blocked_sequences: vec![5, 6],
        };
        assert!(write_state(temp.path(), &state).is_ok());
        assert_eq!(
            read_text(&temp.path().join("state.json")),
            "{\n  \"auto\": false,\n  \"blocked\": [\n    5,\n    6\n  ],\n  \
             \"last_check\": 1790000000,\n  \"schema\": 1,\n  \"skipped\": \"0.3.0\"\n}\n"
        );

        let extremes = UpdateState {
            last_check_epoch: i64::MIN,
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &extremes).is_ok());
        assert_eq!(
            read_text(&temp.path().join("state.json")),
            "{\n  \"auto\": true,\n  \"blocked\": [],\n  \"last_check\": -9223372036854775808,\n  \
             \"schema\": 1,\n  \"skipped\": \"\"\n}\n"
        );

        let written = LastResult {
            status: String::from("blocked"),
            reason: String::from("a/b \u{1}\u{1f}\u{7f}\t\u{8}\u{c}\r\n\"\\"),
            holder: String::from("caf\u{e9} \u{2713}"),
            version: String::from("0.3.0"),
            transaction: String::from("3fa9"),
        };
        assert!(write_last_result(temp.path(), &written).is_ok());
        assert_eq!(
            read_file(&temp.path().join("last.json")),
            "{\n  \"holder\": \"caf\u{e9} \u{2713}\",\n  \
             \"reason\": \"a/b \\u0001\\u001f\u{7f}\\t\\b\\f\\r\\n\\\"\\\\\",\n  \"schema\": 1,\n  \
             \"status\": \"blocked\",\n  \"tx\": \"3fa9\",\n  \"version\": \"0.3.0\"\n}\n"
                .as_bytes()
        );
    }

    #[test]
    fn members_missing_take_their_defaults_and_members_unknown_are_ignored() {
        let temp = TempDir::new();
        write_file(&temp.path().join("state.json"), r#"{"schema":1}"#);
        let state = ok(read_state(temp.path()));
        assert_eq!(state.last_check_epoch, 0);
        assert!(state.automatic);

        write_file(
            &temp.path().join("state.json"),
            r#"{"schema":1,"auto":false,"added_later":{"a":[1]},"last_check":-5}"#,
        );
        let state = ok(read_state(temp.path()));
        assert!(!state.automatic);
        assert_eq!(
            state.last_check_epoch, -5,
            "a time before 1970 is kept as it is; check_is_due treats it as never"
        );
    }

    #[test]
    fn a_corrupt_file_is_a_parse_failure_that_the_caller_answers_with_defaults() {
        let cases: [&[u8]; 25] = [
            b"",
            b"{",
            b"null",
            b"[]",
            b"42",
            b"\"state\"",
            b"{\"schema\":1} trailing",
            b"\x01\x02\xff\xfe binary",
            br#"{"auto":true}"#,  // no schema
            br#"{"schema":"1"}"#, // schema as text
            br#"{"schema":1.0}"#,
            br#"{"schema":1,"auto":"yes"}"#,
            br#"{"schema":1,"auto":1}"#,
            br#"{"schema":1,"last_check":"yesterday"}"#,
            br#"{"schema":1,"last_check":1.5}"#,
            br#"{"schema":1,"last_check":true}"#,
            br#"{"schema":1,"last_check":9223372036854775808}"#, // INT64_MAX + 1
            br#"{"schema":1,"skipped":5}"#,
            br#"{"schema":1,"skipped":null}"#,
            br#"{"schema":1,"blocked":5}"#,
            br#"{"schema":1,"blocked":{"a":1}}"#,
            br#"{"schema":1,"blocked":[-1]}"#,
            br#"{"schema":1,"blocked":["a"]}"#,
            br#"{"schema":1,"blocked":[1.5]}"#,
            br#"{"schema":1,"blocked":[1,2,null]}"#,
        ];
        for text in cases {
            let shown = String::from_utf8_lossy(text);
            let temp = TempDir::new();
            write_file(&temp.path().join("state.json"), text);
            let error = read_state(temp.path()).expect_err(&shown);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{shown}");
            assert!(error.context.contains("state.json"), "{}", error.context);
        }
    }

    #[test]
    fn a_schema_from_another_version_is_unsupported_not_corrupt() {
        for text in [
            r#"{"schema":2}"#,
            r#"{"schema":0}"#,
            r#"{"schema":99,"auto":false}"#,
        ] {
            let temp = TempDir::new();
            write_file(&temp.path().join("state.json"), text);
            let error = read_state(temp.path()).expect_err(text);
            assert_eq!(error.code, ErrorCode::Unsupported, "{text}");
        }
    }

    #[test]
    fn a_state_file_that_is_a_folder_or_far_too_long_is_an_error_not_the_defaults() {
        {
            let temp = TempDir::new();
            std::fs::create_dir(temp.path().join("state.json")).expect("a folder");
            let error = read_state(temp.path()).expect_err("a folder");
            assert_eq!(error.code, ErrorCode::FileImportFailure);
        }
        {
            let temp = TempDir::new();
            write_file(
                &temp.path().join("state.json"),
                format!(r#"{{"schema":1,"skipped":"{}"}}"#, "x".repeat(1024 * 1024)),
            );
            let error = read_state(temp.path())
                .expect_err("a megabyte of a version string was not written by this program");
            assert_eq!(error.code, ErrorCode::ParseFailure);
        }
    }

    #[test]
    fn writing_fails_on_a_folder_that_cannot_be_made() {
        let temp = TempDir::new();
        write_file(&temp.path().join("a_file"), "x");
        let error = write_state(
            &temp.path().join("a_file").join("U"),
            &UpdateState::default(),
        )
        .expect_err("a folder under a file");
        assert_eq!(error.code, ErrorCode::FileExportFailure);
    }

    // ---- state.json is replaced whole or not at all ---------------------------------------------

    #[test]
    fn a_rewrite_leaves_one_file_and_nothing_of_the_old_content() {
        let temp = TempDir::new();
        let big = UpdateState {
            skipped_version: String::from("9.9.9"),
            blocked_sequences: (0..2000u64).map(|i| 1790000000 + i).collect(),
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &big).is_ok());
        let small = UpdateState {
            automatic: false,
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &small).is_ok());

        let back = ok(read_state(temp.path()));
        assert!(!back.automatic);
        assert!(
            back.blocked_sequences.is_empty(),
            "no tail of the longer old file is left"
        );
        assert_eq!(back.skipped_version, "");

        let names: Vec<String> = std::fs::read_dir(temp.path())
            .expect("the folder")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![String::from("state.json")],
            "the temporary file is gone"
        );
    }

    #[test]
    fn a_write_that_cannot_complete_leaves_the_old_file_whole_and_no_litter() {
        let temp = TempDir::new();
        let first = UpdateState {
            skipped_version: String::from("0.3.0"),
            blocked_sequences: vec![1, 2, 3],
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &first).is_ok());
        let record = temp.path().join("state.json");
        let before = read_file(&record);

        #[cfg(windows)]
        {
            // The file is read-only: Windows will not replace it.
            let mut permissions = std::fs::metadata(&record)
                .expect("the record")
                .permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&record, permissions).expect("a read-only record");
        }
        #[cfg(unix)]
        {
            // The folder takes no new files.
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(temp.path())
                .expect("the folder")
                .permissions()
                .mode();
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode & !0o200))
                .expect("a folder without write permission");
            if std::fs::File::create(temp.path().join("probe")).is_ok() {
                let _ = std::fs::remove_file(temp.path().join("probe"));
                let _ =
                    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode));
                eprintln!("this user can create files in a folder without permission (root?)");
                return;
            }
        }
        let second = UpdateState {
            skipped_version: String::from("9.9.9"),
            ..UpdateState::default()
        };
        let status = write_state(temp.path(), &second);
        #[cfg(windows)]
        {
            let mut permissions = std::fs::metadata(&record)
                .expect("the record")
                .permissions();
            #[allow(
                clippy::permissions_set_readonly_false,
                reason = "on Windows this clears the read-only attribute, which is all it does"
            )]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(&record, permissions);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(temp.path())
                .expect("the folder")
                .permissions()
                .mode();
            let _ = std::fs::set_permissions(
                temp.path(),
                std::fs::Permissions::from_mode(mode | 0o200),
            );
        }
        let error = status.expect_err("the replacement was refused, and said so");
        assert_eq!(error.code, ErrorCode::FileExportFailure);
        assert_eq!(read_file(&record), before, "the old record is as it was");
        assert_eq!(
            entries_in(temp.path()),
            1,
            "no temporary file was left behind"
        );
    }

    #[test]
    fn a_reader_during_many_rewrites_never_sees_half_a_file() {
        // The control first: a file cut in half IS refused, so the check below could see a torn
        // one.
        let control = TempDir::new();
        let wide = UpdateState {
            blocked_sequences: (0..3000u64).map(|i| 1790000000 + i).collect(),
            ..UpdateState::default()
        };
        assert!(write_state(control.path(), &wide).is_ok());
        let whole = read_file(&control.path().join("state.json"));
        write_file(
            &control.path().join("state.json"),
            &whole[..whole.len() / 2],
        );
        assert_eq!(
            read_state(control.path()).expect_err("half a record").code,
            ErrorCode::ParseFailure
        );

        // Now a writer that alternates a long record and a short one, and a reader that reads as
        // fast as it can.
        let temp = TempDir::new();
        let small = UpdateState {
            skipped_version: String::from("short"),
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &wide).is_ok());
        let done = AtomicBool::new(false);
        let write_failures = AtomicI32::new(0);
        const WRITES: i32 = 40;
        let (reads, saw_wide, saw_small, torn, first_failure) = std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                let mut first_failure = String::new();
                for i in 0..WRITES {
                    let written = write_state(temp.path(), if i % 2 == 0 { &small } else { &wide });
                    if let Err(error) = written
                        && write_failures.fetch_add(1, Ordering::SeqCst) == 0
                    {
                        first_failure = error.describe();
                    }
                }
                done.store(true, Ordering::SeqCst);
                first_failure
            });
            let (mut reads, mut saw_wide, mut saw_small, mut torn) =
                (0usize, 0usize, 0usize, 0usize);
            while !done.load(Ordering::SeqCst) {
                match read_state(temp.path()) {
                    Ok(state) => {
                        reads += 1;
                        if state.blocked_sequences.len() == 3000 && state.skipped_version.is_empty()
                        {
                            saw_wide += 1;
                        } else if state.blocked_sequences.is_empty()
                            && state.skipped_version == "short"
                        {
                            saw_small += 1;
                        } else {
                            torn += 1;
                        }
                    }
                    // An unreadable half is exactly what this must never see.
                    Err(error) if error.code == ErrorCode::ParseFailure => torn += 1,
                    // FileImportFailure: the replacement was under way at that instant; try again.
                    Err(_) => {}
                }
                std::thread::sleep(Duration::from_micros(300));
            }
            let first_failure = writer.join().expect("the writer");
            (reads, saw_wide, saw_small, torn, first_failure)
        });
        assert_eq!(torn, 0);
        assert!(reads > 0);
        assert!(saw_wide > 0);
        assert!(
            saw_small > 0,
            "both records were seen: the check covered a change of content"
        );
        // Windows will not rename over a record that a reader has open at that instant, even one
        // opened with every share mode: the write waits for the reader to let go (two seconds at
        // most, the cases below). This reader reopens the file every 300 microseconds without
        // end, which no real one does, so some writes wait for a long time and a few may give up;
        // most must not.
        assert!(
            write_failures.load(Ordering::SeqCst) < WRITES / 2,
            "{first_failure}"
        );
    }

    // A file held open with no sharing, as a virus scanner holds a file it is looking at.
    #[cfg(windows)]
    fn hold_without_sharing(file: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(file)
            .expect("the record, held")
    }

    #[cfg(windows)]
    #[test]
    fn a_record_another_program_has_open_for_a_while_is_replaced_when_it_lets_go() {
        // A virus scanner looking at a file that was just closed holds it with no sharing for a
        // moment: 400 ms here, eight times what the first version of the retry waited. (Windows
        // answers the rename with error 5, access denied, or 32, sharing violation.)
        let temp = TempDir::new();
        let first = UpdateState {
            skipped_version: String::from("0.3.0"),
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &first).is_ok());
        let held = hold_without_sharing(&temp.path().join("state.json"));
        let second = UpdateState {
            skipped_version: String::from("9.9.9"),
            ..UpdateState::default()
        };
        let status = std::thread::scope(|scope| {
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(400));
                drop(held);
            });
            write_state(temp.path(), &second)
        });
        assert!(status.is_ok(), "{}", describe(&status));
        let back = ok(read_state(temp.path()));
        assert_eq!(back.skipped_version, "9.9.9");
        assert_eq!(
            entries_in(temp.path()),
            1,
            "no temporary file was left behind"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_record_held_open_for_good_is_reported_after_the_wait_and_the_old_one_is_whole() {
        let temp = TempDir::new();
        let first = UpdateState {
            skipped_version: String::from("0.3.0"),
            ..UpdateState::default()
        };
        assert!(write_state(temp.path(), &first).is_ok());
        let file = temp.path().join("state.json");
        let before = read_file(&file);
        let held = hold_without_sharing(&file);
        let second = UpdateState {
            skipped_version: String::from("9.9.9"),
            ..UpdateState::default()
        };
        let begin = std::time::Instant::now();
        let status = write_state(temp.path(), &second);
        let waited = begin.elapsed();
        drop(held);
        let error = status.expect_err("a record held for good");
        assert_eq!(error.code, ErrorCode::FileExportFailure);
        let context = &error.context;
        assert!(
            context.starts_with("os_error=5;")
                || context.starts_with("os_error=32;")
                || context.starts_with("os_error=33;"),
            "the system's own number, one of the three that mean in use: {context}"
        );
        assert!(
            waited >= Duration::from_millis(1500),
            "it waited for the program to let go"
        );
        assert_eq!(read_file(&file), before);
        assert_eq!(
            entries_in(temp.path()),
            1,
            "no temporary file was left behind"
        );
    }
}

// ---- last.json -----------------------------------------------------------------------------------

mod last_result {
    use super::*;

    #[test]
    fn no_file_is_no_result() {
        let temp = TempDir::new();
        assert!(ok(read_last_result(temp.path())).is_none());
        assert!(ok(read_last_result(&temp.path().join("never_made"))).is_none());
    }

    #[test]
    fn every_field_comes_back_including_the_half_swapped_trees_status() {
        let temp = TempDir::new();
        let written = LastResult {
            status: String::from("rollback_incomplete"),
            reason: String::from("rollback_failed"),
            holder: String::from("katana_mcp.exe (4412)"),
            version: String::from("0.3.0"),
            transaction: String::from("3fa9c0de"),
        };
        assert!(write_last_result(&temp.path().join("U"), &written).is_ok());
        let result = ok(read_last_result(&temp.path().join("U"))).expect("a result");
        assert_eq!(result.status, "rollback_incomplete");
        assert_eq!(result.reason, "rollback_failed");
        assert_eq!(result.holder, "katana_mcp.exe (4412)");
        assert_eq!(result.version, "0.3.0");
        assert_eq!(result.transaction, "3fa9c0de");
        let text = read_text(&temp.path().join("U").join("last.json"));
        assert!(
            holds(&text, &["\"tx\"", ":", "\"3fa9c0de\""], false),
            "{text}"
        );
    }

    #[test]
    fn quotes_backslashes_line_breaks_and_non_ascii_survive() {
        let temp = TempDir::new();
        let written = LastResult {
            status: String::from("blocked"),
            // e acute (C3 A9) and a check mark (E2 9C 93).
            holder: String::from(
                "\"katana mcp\".exe (4412), C:\\Program Files\\x\nsecond line, caf\u{e9} \u{2713}",
            ),
            ..LastResult::default()
        };
        assert!(write_last_result(temp.path(), &written).is_ok());
        let result = ok(read_last_result(temp.path())).expect("a result");
        assert_eq!(result.holder, written.holder);
        assert_eq!(result.reason, "");
    }

    #[test]
    fn another_result_replaces_the_first_and_clearing_removes_it() {
        let temp = TempDir::new();
        let first = LastResult {
            status: String::from("rolled_back"),
            reason: String::from("health_timeout"),
            ..LastResult::default()
        };
        let second = LastResult {
            status: String::from("committed"),
            ..LastResult::default()
        };
        assert!(write_last_result(temp.path(), &first).is_ok());
        assert!(write_last_result(temp.path(), &second).is_ok());
        let back = ok(read_last_result(temp.path())).expect("a result");
        assert_eq!(back.status, "committed");
        assert_eq!(back.reason, "", "nothing of the first is left");

        assert!(clear_last_result(temp.path()).is_ok());
        assert!(!temp.path().join("last.json").exists());
        assert!(
            ok(read_last_result(temp.path())).is_none(),
            "shown once, then cleared"
        );
        assert!(
            clear_last_result(temp.path()).is_ok(),
            "clearing what is not there is fine"
        );
        assert!(clear_last_result(&temp.path().join("never_made")).is_ok());
    }

    #[test]
    fn a_file_that_is_not_a_result_is_refused() {
        let cases = [
            ("", ErrorCode::ParseFailure),
            ("{", ErrorCode::ParseFailure),
            ("[]", ErrorCode::ParseFailure),
            (r#"{"status":"committed"}"#, ErrorCode::ParseFailure), // no schema
            (r#"{"schema":1}"#, ErrorCode::ParseFailure),           // no status
            (r#"{"schema":1,"status":""}"#, ErrorCode::ParseFailure),
            (r#"{"schema":1,"status":5}"#, ErrorCode::ParseFailure),
            (
                r#"{"schema":1,"status":"blocked","holder":5}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":1,"status":"blocked","reason":null}"#,
                ErrorCode::ParseFailure,
            ),
            (
                r#"{"schema":1,"status":"blocked","tx":["a"]}"#,
                ErrorCode::ParseFailure,
            ),
            (r#"{"schema":2,"status":"blocked"}"#, ErrorCode::Unsupported),
        ];
        for (text, code) in cases {
            let temp = TempDir::new();
            write_file(&temp.path().join("last.json"), text);
            let error = read_last_result(temp.path()).expect_err(text);
            assert_eq!(error.code, code, "{text}");
        }
        // Members beyond the ones read are ignored.
        let temp = TempDir::new();
        write_file(
            &temp.path().join("last.json"),
            r#"{"schema":1,"status":"refused","new_member":[1,2]}"#,
        );
        let result = ok(read_last_result(temp.path())).expect("a result");
        assert_eq!(result.status, "refused");
    }
}

// ---- U/cleanup.list ------------------------------------------------------------------------------

// The file system of the shipped kind: Windows' own (fsops_win.rs), so that these cases judge the
// cleanup against it and not against the test-local DiskFs, whose stat calls any reparse point a
// link where the real one goes by the tag (is_link_reparse_tag). Elsewhere the shipped one refuses
// every operation until the POSIX apply is written, and DiskFs stands in for it. The crash and lock
// cases inject their faults into a DiskFs instead, which is all an injected fault needs.
fn shipped_fs() -> Box<dyn FsOps> {
    #[cfg(windows)]
    {
        katana_update::fsops::make_system_fs_ops(&katana_update::fsops::RetryPolicy {
            interval: Duration::from_millis(5),
            total: Duration::from_millis(100),
        })
    }
    #[cfg(not(windows))]
    {
        Box::new(DiskFs::default())
    }
}

// U and R/.update under a temporary folder, a victim outside both, and a place to make files in.
struct CleanupWorld {
    temp: TempDir,
    state_dir: PathBuf,
    root: PathBuf,
    work: PathBuf,
    list: PathBuf,
    system: Box<dyn FsOps>,
}

impl CleanupWorld {
    fn new() -> Self {
        let temp = TempDir::new();
        let state_dir = temp.path().join("U");
        let root = temp.path().join("R");
        let work = root.join(".update");
        let list = state_dir.join("cleanup.list");
        std::fs::create_dir_all(&state_dir).expect("U");
        std::fs::create_dir_all(&work).expect("R/.update");
        Self {
            temp,
            state_dir,
            root,
            work,
            list,
            system: shipped_fs(),
        }
    }

    fn disk(&self) -> &dyn FsOps {
        self.system.as_ref()
    }

    fn file(&self, path: PathBuf) -> PathBuf {
        write_file(&path, "x");
        path
    }

    fn list_lines(&self, paths: &[PathBuf]) {
        let mut text = Vec::new();
        for path in paths {
            text.extend_from_slice(&line_of(path));
        }
        write_file(&self.list, text);
    }

    fn process(&self, fs: &dyn FsOps) -> Status {
        process_cleanup_list(&self.state_dir, &self.root, fs)
    }
}

// The two leftovers of the crash cases: a helper copy's folder with a locked file in it, and a
// loose file.
fn set_up_locked_and_loose(world: &CleanupWorld) {
    world.file(world.state_dir.join("run-x").join("locked.bin"));
    world.file(world.state_dir.join("loose.tmp"));
    world.list_lines(&[
        world.state_dir.join("run-x"),
        world.state_dir.join("loose.tmp"),
    ]);
}

mod cleanup_list {
    use super::*;

    #[test]
    fn appending_creates_the_folder_and_the_list_and_adds_a_line_each_time() {
        let temp = TempDir::new();
        let shipped = shipped_fs();
        let disk = shipped.as_ref();
        let state_dir = temp.path().join("not").join("made").join("yet");
        let first = temp.path().join("a").join("first.bin");
        // A space, and non-ASCII.
        let second = temp
            .path()
            .join("b c")
            .join(path_from_utf8("caf\u{e9}.bin"));
        assert!(append_cleanup(&state_dir, &first, disk).is_ok());
        assert_eq!(read_file(&state_dir.join("cleanup.list")), line_of(&first));
        assert!(append_cleanup(&state_dir, &second, disk).is_ok());
        assert_eq!(
            read_file(&state_dir.join("cleanup.list")),
            [line_of(&first), line_of(&second)].concat()
        );
    }

    #[test]
    fn only_an_absolute_path_on_one_line_is_listed() {
        let temp = TempDir::new();
        let shipped = shipped_fs();
        let disk = shipped.as_ref();
        let state_dir = temp.path().join("U");
        for bad in [
            PathBuf::from("relative/file.bin"),
            PathBuf::new(),
            PathBuf::from("file.bin"),
            temp.path().join("two\nlines"),
            temp.path().join("carriage\rreturn"),
        ] {
            let error = append_cleanup(&state_dir, &bad, disk).expect_err(&text_of(&bad));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{}", text_of(&bad));
        }
        assert!(
            !state_dir.join("cleanup.list").exists(),
            "nothing was written"
        );
    }

    #[test]
    fn appending_goes_through_the_seam_so_a_crash_at_any_step_leaves_a_whole_list() {
        // Run once to count the changing operations, then die before each: afterwards the list is
        // absent or holds exactly the leftover, whole. (The library's FaultFs counts and crashes
        // as the C++ case's own CrashingFs does.)
        let operations = {
            let temp = TempDir::new();
            let disk = DiskFs::default();
            let crashing = FaultFs::new(&disk);
            let appended = append_cleanup(
                &temp.path().join("U"),
                &temp.path().join("leftover.bin"),
                &crashing,
            );
            assert!(appended.is_ok(), "{}", describe(&appended));
            let operations = crashing.mutating_operations();
            assert!(
                operations >= 1,
                "the append is counted: it went through the seam"
            );
            operations
        };
        for k in 1..=operations {
            let temp = TempDir::new();
            let disk = DiskFs::default();
            let crashing = FaultFs::new(&disk);
            crashing.crash_before(k);
            let leftover = temp.path().join("leftover.bin");
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                append_cleanup(&temp.path().join("U"), &leftover, &crashing)
            }));
            assert_eq!(crash_of(outcome), k, "operation {k}");
            let list = temp.path().join("U").join("cleanup.list");
            if list.exists() {
                assert_eq!(read_file(&list), line_of(&leftover), "operation {k}");
            }
        }
    }

    #[test]
    fn a_list_taken_for_processing_survives_a_crash_and_the_next_start_finishes_it() {
        // The list is taken by a rename (cleanup.list to cleanup.list.work) and given back by an
        // append of what stays, so that a line another program appends meanwhile is never written
        // over. A crash in the middle leaves the lines in the work file under their other name:
        // nothing is lost, and the next start finishes.
        let world = CleanupWorld::new();
        let locked = DiskFs::locked(&["locked.bin"]);
        let stuck = world.file(world.state_dir.join("run-x").join("locked.bin"));
        let loose = world.file(world.state_dir.join("loose.tmp"));
        world.list_lines(&[world.state_dir.join("run-x"), loose.clone()]);
        let before = read_file(&world.list);
        let taken = world.state_dir.join("cleanup.list.work");

        // A crash on the removal of the second line's file.
        let crashing = DiskFs {
            crash_on_name: Some("loose.tmp"),
            ..DiskFs::locked(&["locked.bin"])
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| world.process(&crashing)));
        assert_eq!(crash_of(outcome), 1);
        assert_eq!(
            read_file(&taken),
            before,
            "the same text, under the name of a list being worked on"
        );
        assert!(!world.list.exists());
        assert!(stuck.exists());
        assert!(loose.exists());
        // And the next start gets on with it.
        assert!(world.process(&locked).is_ok());
        assert!(!loose.exists());
        assert_eq!(
            read_file(&world.list),
            line_of(&world.state_dir.join("run-x"))
        );
        assert!(!taken.exists());
    }

    #[test]
    fn a_crash_before_any_change_of_a_processing_run_loses_no_leftover_and_the_next_starts_finish_it()
     {
        // The run takes the list (a rename), removes what it can (two removals, one refused),
        // appends what stays and removes the work file: five changes. Die before each and then
        // restart, as the next three starts would: the free leftover is gone, the locked one is
        // still listed (once, after the duplicate a death between the append and the last removal
        // leaves has been merged) and no work file is left.
        let changes = Cell::new(0u64);
        {
            let world = CleanupWorld::new();
            set_up_locked_and_loose(&world);
            let watched = DiskFs {
                before: Some(Box::new(|call: &Call| {
                    if call.changes {
                        changes.set(changes.get() + 1);
                    }
                })),
                ..DiskFs::locked(&["locked.bin"])
            };
            assert!(world.process(&watched).is_ok());
            assert_eq!(
                changes.get(),
                5,
                "rename, two removals, the append, the work file's removal"
            );
        }
        for k in 1..=changes.get() {
            let world = CleanupWorld::new();
            set_up_locked_and_loose(&world);
            {
                let seen = Cell::new(0u64);
                let watched = DiskFs {
                    before: Some(Box::new(|call: &Call| {
                        if call.changes {
                            seen.set(seen.get() + 1);
                            if seen.get() == k {
                                std::panic::panic_any(Crash { operation: k });
                            }
                        }
                    })),
                    ..DiskFs::locked(&["locked.bin"])
                };
                let outcome = catch_unwind(AssertUnwindSafe(|| world.process(&watched)));
                assert_eq!(crash_of(outcome), k, "change {k}");
            }
            for start in 1..=3 {
                let again = DiskFs::locked(&["locked.bin"]);
                let status = world.process(&again);
                assert!(
                    status.is_ok(),
                    "change {k}, start {start}: {}",
                    describe(&status)
                );
            }
            assert!(!world.state_dir.join("loose.tmp").exists(), "change {k}");
            assert!(
                world.state_dir.join("run-x").join("locked.bin").exists(),
                "change {k}"
            );
            assert_eq!(
                read_file(&world.list),
                line_of(&world.state_dir.join("run-x")),
                "change {k}"
            );
            assert!(
                !world.state_dir.join("cleanup.list.work").exists(),
                "change {k}"
            );
        }
    }

    #[test]
    fn a_leftover_appended_by_another_program_while_the_list_is_processed_is_never_lost() {
        // The helper appends (append_cleanup) at any moment, and the window may be working through
        // the list just then. Whichever call of the run the append falls after (between the read
        // and what used to be the rewrite most of all) the leftover is either acted on in this run
        // or still listed when it ends. Once with a list that ends up empty and once with a locked
        // file kept.
        for with_locked in [false, true] {
            let set_up = |world: &CleanupWorld| {
                world.file(world.state_dir.join("old.tmp"));
                let mut lines = vec![world.state_dir.join("old.tmp")];
                if with_locked {
                    world.file(world.state_dir.join("run-x").join("locked.bin"));
                    lines.push(world.state_dir.join("run-x"));
                }
                world.list_lines(&lines);
            };
            let calls = Cell::new(0u64);
            {
                let world = CleanupWorld::new();
                set_up(&world);
                let watched = DiskFs {
                    after: Some(Box::new(|_: &Call| calls.set(calls.get() + 1))),
                    ..DiskFs::locked(&["locked.bin"])
                };
                assert!(world.process(&watched).is_ok());
            }
            assert!(
                calls.get() >= 8,
                "the run makes the calls the loop is to fall between"
            );
            for k in 1..=calls.get() {
                let world = CleanupWorld::new();
                set_up(&world);
                let late = world.file(world.state_dir.join("late.tmp"));
                let seen = Cell::new(0u64);
                let watched = DiskFs {
                    after: Some(Box::new(|_: &Call| {
                        seen.set(seen.get() + 1);
                        if seen.get() == k {
                            append_to(&world.list, &line_of(&late));
                        }
                    })),
                    ..DiskFs::locked(&["locked.bin"])
                };
                let status = world.process(&watched);
                assert!(status.is_ok(), "append after call {k}");
                let listed = if world.list.exists() {
                    read_file(&world.list)
                } else {
                    Vec::new()
                };
                assert!(
                    !late.exists() || find(&listed, &line_of(&late)).is_some(),
                    "append after call {k}: late.tmp is neither removed nor listed"
                );
                assert!(!world.state_dir.join("old.tmp").exists(), "call {k}");
                assert!(
                    !world.state_dir.join("cleanup.list.work").exists(),
                    "call {k}"
                );
                if with_locked {
                    let line = line_of(&world.state_dir.join("run-x"));
                    assert!(
                        world.state_dir.join("run-x").join("locked.bin").exists(),
                        "call {k}"
                    );
                    assert!(find(&listed, &line).is_some(), "call {k}");
                    assert_eq!(
                        find(&listed, &line),
                        rfind(&listed, &line),
                        "listed once, call {k}"
                    );
                }

                // The next start finishes whatever was left, the lock gone.
                let next = DiskFs::default();
                assert!(world.process(&next).is_ok());
                assert!(!late.exists(), "call {k}");
                assert!(!world.state_dir.join("run-x").exists(), "call {k}");
                assert!(!world.list.exists(), "call {k}");
            }
        }
    }

    #[test]
    fn a_work_file_left_by_a_run_that_died_is_finished_first_and_the_list_waits_for_the_next_start()
    {
        let world = CleanupWorld::new();
        let taken = world.state_dir.join("cleanup.list.work");
        let from_the_dead_run = world.file(world.state_dir.join("a.tmp"));
        let from_the_list = world.file(world.state_dir.join("b.tmp"));
        write_file(&taken, line_of(&from_the_dead_run));
        world.list_lines(std::slice::from_ref(&from_the_list));
        assert!(world.process(world.disk()).is_ok());
        assert!(!from_the_dead_run.exists());
        assert!(!taken.exists());
        assert!(from_the_list.exists(), "the list is the next start's");
        assert_eq!(read_file(&world.list), line_of(&from_the_list));
        assert!(world.process(world.disk()).is_ok());
        assert!(!from_the_list.exists());
        assert!(!world.list.exists());
    }

    #[test]
    fn a_work_file_that_is_a_folder_is_an_error() {
        let world = CleanupWorld::new();
        world.list_lines(&[world.state_dir.join("x.tmp")]);
        std::fs::create_dir(world.state_dir.join("cleanup.list.work")).expect("a folder");
        let error = world
            .process(world.disk())
            .expect_err("a work file that is a folder");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(world.list.exists(), "and the list is not touched");
    }

    #[test]
    fn a_line_naming_the_file_the_list_is_being_worked_from_is_not_acted_on() {
        // The list does not delete itself, and that holds for the name it has while it is
        // processed. Removed from under the run it would take the lines with it should the run die
        // before the end. The only removal of it is the run's own, once, at the end.
        let world = CleanupWorld::new();
        let taken = world.state_dir.join("cleanup.list.work");
        let loose = world.file(world.state_dir.join("loose.tmp"));
        world.list_lines(&[taken.clone(), loose.clone(), world.list.clone()]);
        let watched = DiskFs::default();
        assert!(world.process(&watched).is_ok());
        assert!(!loose.exists());
        assert!(!taken.exists());
        assert!(!world.list.exists());
        let removed = watched.removed_files.borrow();
        assert_eq!(removed.iter().filter(|&path| *path == taken).count(), 1);
        assert_eq!(removed.iter().filter(|&path| *path == loose).count(), 1);
    }

    #[test]
    fn a_line_under_a_place_that_cannot_be_resolved_just_now_is_kept_for_the_next_start() {
        // A place that is missing resolves (the existing part, then the rest as it is); one that
        // cannot be resolved gave the system's error (access denied, a sharing violation), which
        // may pass. The lines under it are by their text ours: kept, not dropped, and the lines
        // under the other place are not held up.
        let world = CleanupWorld::new();
        let in_work = world.file(world.work.join("tx1").join("old.dll"));
        let in_state = world.file(world.state_dir.join("loose.tmp"));
        world.list_lines(&[world.work.join("tx1"), in_state.clone()]);
        {
            let watched = DiskFs {
                unresolvable: Some(Box::new(|path: &Path| {
                    path.file_name()
                        .is_some_and(|name| name == std::ffi::OsStr::new(".update"))
                })),
                ..DiskFs::default()
            };
            assert!(world.process(&watched).is_ok());
        }
        assert!(
            in_work.exists(),
            "not acted on while its place cannot be examined"
        );
        assert!(!in_state.exists(), "the other place was examined");
        assert_eq!(read_file(&world.list), line_of(&world.work.join("tx1")));

        // The next start, when the system answers.
        assert!(world.process(world.disk()).is_ok());
        assert!(!world.work.join("tx1").exists());
        assert!(world.work.is_dir());
        assert!(!world.list.exists());
    }

    #[test]
    fn no_list_is_ok_and_creates_nothing() {
        let world = CleanupWorld::new();
        assert!(world.process(world.disk()).is_ok());
        assert!(!world.list.exists());
        let never_made = world.temp.path().join("never_made");
        assert!(process_cleanup_list(&never_made, &world.root, world.disk()).is_ok());
        assert!(!never_made.exists());
    }

    #[test]
    fn leftovers_under_the_state_folder_and_the_transaction_folder_are_removed_whole() {
        let world = CleanupWorld::new();
        // A helper copy's folder, a staged tree, a loose file, and a transaction's old/ and new/.
        world.file(world.state_dir.join("run-3fa9").join("katana_updater.exe"));
        world.file(world.state_dir.join("run-3fa9").join("selftest.txt"));
        world.file(
            world
                .state_dir
                .join("stage-1792312800")
                .join("bin")
                .join("katana.exe"),
        );
        world.file(
            world
                .state_dir
                .join("stage-1792312800")
                .join("share")
                .join("katana")
                .join("deep")
                .join("er")
                .join("x.txt"),
        );
        world.file(world.state_dir.join("loose.tmp"));
        world.file(world.work.join("tx1").join("old").join("bin").join("a.dll"));
        world.file(world.work.join("tx1").join("new").join("bin").join("a.dll"));
        std::fs::create_dir_all(world.work.join("tx1").join("empty_dir")).expect("a folder");
        let leftovers = [
            world.state_dir.join("run-3fa9"),
            world.state_dir.join("stage-1792312800"),
            world.state_dir.join("loose.tmp"),
            world.work.join("tx1"),
        ];
        for leftover in &leftovers {
            assert!(append_cleanup(&world.state_dir, leftover, world.disk()).is_ok());
        }
        assert!(world.process(world.disk()).is_ok());
        for leftover in &leftovers {
            assert!(!leftover.exists(), "{}", text_of(leftover));
        }
        assert!(!world.list.exists(), "nothing is left to list");
        assert!(world.state_dir.is_dir());
        assert!(world.work.is_dir(), "the places themselves stay");
    }

    #[test]
    fn a_line_that_is_not_in_the_state_folder_or_the_transaction_folder_is_dropped_and_untouched() {
        let world = CleanupWorld::new();
        let outside = world.file(world.temp.path().join("victim.txt"));
        // R, but not R/.update.
        let in_install = world.file(world.root.join("bin").join("katana.exe"));
        // Shares U's name as a prefix.
        let in_sibling = world.file(world.temp.path().join("U-evil").join("x"));
        let in_work_sibling = world.file(world.root.join(".update-evil").join("x"));
        // Lexically outside.
        let dot_dot = world
            .state_dir
            .join("run-a")
            .join("..")
            .join("..")
            .join("victim.txt");
        world.file(world.state_dir.join("run-a").join("keep.txt"));
        world.list_lines(&[
            outside.clone(),
            in_install.clone(),
            in_sibling.clone(),
            in_work_sibling.clone(),
            dot_dot,
        ]);
        // Lines that are not absolute paths at all, and blank lines, written by hand.
        let mut text = read_file(&world.list);
        text.extend_from_slice(b"relative/victim.txt\n\n   \nvictim.txt\n");
        write_file(&world.list, text);

        assert!(world.process(world.disk()).is_ok());
        assert!(outside.exists());
        assert!(
            in_install.exists(),
            "R/bin is the install, not the work folder"
        );
        assert!(in_sibling.exists());
        assert!(in_work_sibling.exists());
        assert!(world.state_dir.join("run-a").join("keep.txt").exists());
        assert!(!world.list.exists(), "dropped lines are not kept");
    }

    #[test]
    fn the_state_folder_and_the_work_folder_themselves_are_never_removed() {
        let world = CleanupWorld::new();
        write_file(&world.state_dir.join("state.json"), "{}");
        world.file(world.work.join("lock"));
        world.list_lines(&[
            world.state_dir.clone(),
            world.work.clone(),
            world.state_dir.join("."),
            world.work.join("tx1").join("..").join("."),
            world.list.clone(),
        ]);
        assert!(world.process(world.disk()).is_ok());
        assert!(world.state_dir.join("state.json").exists());
        assert!(world.work.join("lock").exists());
        assert!(world.work.is_dir());
    }

    #[test]
    fn with_no_root_only_the_state_folder_is_a_place_the_list_may_name() {
        let world = CleanupWorld::new();
        let in_work = world.file(world.work.join("tx1").join("old.dll"));
        let in_state = world.file(world.state_dir.join("run-1").join("x.exe"));
        world.list_lines(&[in_work.clone(), in_state.clone()]);
        assert!(process_cleanup_list(&world.state_dir, Path::new(""), world.disk()).is_ok());
        assert!(
            in_work.exists(),
            "R/.update is not known, so nothing there is the list's to remove"
        );
        assert!(!in_state.exists());
    }

    #[test]
    fn a_line_naming_a_path_that_is_already_gone_is_dropped_without_fuss() {
        let world = CleanupWorld::new();
        world.list_lines(&[
            world.state_dir.join("run-gone"),
            world.work.join("tx-gone").join("x"),
        ]);
        assert!(world.process(world.disk()).is_ok());
        assert!(!world.list.exists());
    }

    #[test]
    fn a_line_with_a_windows_line_ending_and_duplicates_is_read_as_one_entry() {
        let world = CleanupWorld::new();
        let leftover = world.file(world.state_dir.join("once.tmp"));
        let name = path_to_utf8(&leftover);
        write_file(
            &world.list,
            [
                name.as_slice(),
                b"\r\n".as_slice(),
                name.as_slice(),
                b"\r\n\r\n".as_slice(),
            ]
            .concat(),
        );
        assert!(world.process(world.disk()).is_ok());
        assert!(!leftover.exists());
        assert!(!world.list.exists());
    }

    #[test]
    fn a_line_of_bytes_that_are_not_utf8_is_read_without_throwing_and_a_non_ascii_leftover_is_removed()
     {
        // The list is user-writable text read first thing at start-up, where nothing catches a
        // panic. The names are written as bytes: e acute (C3 A9) in a folder of the state folder,
        // two CJK characters (E5 9C B0 E5 9B B3) in one of the work folder. A line is taken as
        // UTF-8 (path_from_utf8); read by the code page, "caf" + C3 A9 names another folder on
        // Windows, and the leftover stays.
        let world = CleanupWorld::new();
        let accented = world.state_dir.join(path_from_utf8(b"caf\xC3\xA9"));
        let japanese = world
            .work
            .join("tx1")
            .join(path_from_utf8(b"\xE5\x9C\xB0\xE5\x9B\xB3"));
        world.file(accented.join("x.bin"));
        world.file(japanese.join("y.bin"));
        let victim = world.file(world.temp.path().join("victim.txt"));

        let mut under = path_to_utf8(&world.state_dir.join("run-1"));
        under.push(b'/');
        let mut text = Vec::new();
        text.extend_from_slice(&under);
        text.extend_from_slice(b"\xFF\xFE\n"); // not UTF-8 at all
        text.extend_from_slice(&under);
        text.extend_from_slice(b"caf\xC3\n"); // a sequence cut short
        text.extend_from_slice(&path_to_utf8(world.temp.path()));
        text.extend_from_slice(b"/\xC0\xAF\n"); // an overlong '/'
        text.extend_from_slice(b"\xFF\xFE\n"); // and not absolute either
        text.extend_from_slice(&line_of(&accented));
        text.extend_from_slice(&line_of(&japanese));
        write_file(&world.list, text);

        let outcome = catch_unwind(AssertUnwindSafe(|| world.process(world.disk())));
        let status = outcome.expect("the list is read without a panic");
        assert!(status.is_ok(), "{}", describe(&status));
        assert!(
            !accented.exists(),
            "the folder named in UTF-8 is the one removed"
        );
        assert!(!japanese.exists());
        assert!(world.work.join("tx1").is_dir(), "only what the line named");
        assert!(victim.exists());
        assert!(
            !world.list.exists(),
            "no line is left that could be retried"
        );
    }

    #[test]
    fn a_locked_file_stays_listed_and_everything_else_is_removed() {
        let world = CleanupWorld::new();
        let locked = DiskFs::locked(&["locked.bin"]);
        let folder = world.state_dir.join("run-x");
        let stuck = world.file(folder.join("locked.bin"));
        let free = world.file(folder.join("other.bin"));
        let plain = world.file(world.state_dir.join("plain.tmp"));
        world.list_lines(&[folder.clone(), plain.clone(), folder.clone(), plain.clone()]);
        assert!(
            world.process(&locked).is_ok(),
            "a file still locked is the list's business, not an error"
        );
        assert!(stuck.exists());
        assert!(!free.exists(), "what could be removed was");
        assert!(!plain.exists());
        assert_eq!(
            read_file(&world.list),
            line_of(&folder),
            "only the folder that could not be removed, once"
        );

        // The next start, with the lock gone: removed, and the list with it.
        assert!(world.process(world.disk()).is_ok());
        assert!(!folder.exists());
        assert!(!world.list.exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_file_really_held_open_by_another_program_stays_listed_until_it_is_released() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        let world = CleanupWorld::new();
        let held = world.file(world.state_dir.join("run-1").join("katana_updater.exe"));
        assert!(
            append_cleanup(
                &world.state_dir,
                &world.state_dir.join("run-1"),
                world.disk()
            )
            .is_ok()
        );
        // No FILE_SHARE_DELETE: like a running image.
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&held)
            .expect("the file, held");
        let while_held = world.process(world.disk());
        assert!(while_held.is_ok(), "{}", describe(&while_held));
        assert!(held.exists());
        assert_eq!(
            read_file(&world.list),
            line_of(&world.state_dir.join("run-1"))
        );
        drop(handle);
        assert!(world.process(world.disk()).is_ok());
        assert!(!world.state_dir.join("run-1").exists());
        assert!(!world.list.exists());
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_name_that_the_system_reads_as_another_is_not_a_line_this_program_wrote() {
        // Windows drops the dots and spaces at the end of a name: "U\..." is U itself, "U\. " too,
        // and ".. " may be the folder above. Each passes the test that the line lies under U (or
        // R/.update) when read as text. The state and the lock (the journals) are what would go.
        let world = CleanupWorld::new();
        write_file(&world.state_dir.join("state.json"), "{}");
        world.file(world.state_dir.join("run-a").join("keep.txt"));
        world.file(world.work.join("lock"));
        world.file(world.work.join("tx1").join("journal"));
        let mut text = Vec::new();
        for base in [&world.state_dir, &world.work] {
            let base = path_to_utf8(base);
            for name in ["...", "....", ". ", ".. ", "run-a.", "run-a ", "tx1."] {
                text.extend_from_slice(&base);
                text.extend_from_slice(format!("\\{name}\n").as_bytes());
            }
            text.extend_from_slice(&base);
            text.extend_from_slice(b"\\tx1\\...\n"); // tx1\... is tx1
            text.extend_from_slice(&base);
            text.extend_from_slice(b"\\run-a\\keep.txt.\n");
        }
        write_file(&world.list, text);
        assert!(world.process(world.disk()).is_ok());
        assert!(world.state_dir.join("state.json").exists());
        assert!(world.state_dir.join("run-a").join("keep.txt").exists());
        assert!(world.work.join("lock").exists());
        assert!(world.work.join("tx1").join("journal").exists());
        assert!(world.root.is_dir());
        assert!(!world.list.exists(), "and the lines are dropped, not kept");
    }

    #[test]
    fn a_too_long_list_is_discarded_whole_and_nothing_in_it_is_acted_on() {
        let world = CleanupWorld::new();
        let victim = world.file(world.state_dir.join("first_line_victim.tmp"));
        let mut text = line_of(&victim);
        while text.len() < 2 * 1024 * 1024 {
            let pad = world.state_dir.join(format!("pad-{}", text.len()));
            text.extend_from_slice(&line_of(&pad));
        }
        write_file(&world.list, text);
        let error = world
            .process(world.disk())
            .expect_err("a list of two megabytes");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            victim.exists(),
            "a list this size is not one the program wrote: none of it is believed"
        );
        assert!(!world.list.exists());
    }

    #[test]
    fn a_list_that_is_a_folder_is_an_error() {
        let world = CleanupWorld::new();
        std::fs::create_dir(&world.list).expect("a folder");
        let error = world
            .process(world.disk())
            .expect_err("a list that is a folder");
        assert_eq!(error.code, ErrorCode::InvalidState);
    }

    #[test]
    fn a_line_that_would_reach_out_through_a_link_is_not_followed() {
        let world = CleanupWorld::new();
        let outside = world.temp.path().join("outside");
        let victim = world.file(outside.join("f.txt"));
        world.file(outside.join("sub").join("g.txt"));
        std::fs::create_dir_all(world.state_dir.join("run-a")).expect("a folder");
        let link = DirectoryLink::new(&world.state_dir.join("run-a").join("link"), &outside);
        if !link.made() {
            eprintln!("this machine does not let the test make a directory link");
            return;
        }
        // Written by hand: the file behind the link, and the folder that holds the link.
        world.list_lines(&[
            world.state_dir.join("run-a").join("link").join("f.txt"),
            world
                .state_dir
                .join("run-a")
                .join("link")
                .join("sub")
                .join("g.txt"),
        ]);
        assert!(world.process(world.disk()).is_ok());
        assert!(
            victim.exists(),
            "a file reached through a link out of the folder is not removed"
        );
        assert!(outside.join("sub").join("g.txt").exists());
        assert!(!world.list.exists(), "and the lines are dropped");
    }

    #[test]
    fn a_link_is_removed_itself_and_what_it_points_at_is_not_walked() {
        let world = CleanupWorld::new();
        let outside = world.temp.path().join("outside");
        let victim = world.file(outside.join("f.txt"));
        world.file(outside.join("sub").join("g.txt"));
        std::fs::create_dir_all(world.state_dir.join("run-a")).expect("a folder");
        std::fs::create_dir_all(world.state_dir.join("run-b").join("inner")).expect("a folder");
        let leaf = DirectoryLink::new(&world.state_dir.join("run-a").join("link"), &outside);
        let inside = DirectoryLink::new(
            &world.state_dir.join("run-b").join("inner").join("link"),
            &outside,
        );
        if !leaf.made() || !inside.made() {
            eprintln!("this machine does not let the test make a directory link");
            return;
        }
        world.file(world.state_dir.join("run-b").join("own.txt"));
        // run-a/link is named itself; run-b holds a link somewhere in it.
        world.list_lines(&[
            world.state_dir.join("run-a").join("link"),
            world.state_dir.join("run-b"),
        ]);
        assert!(world.process(world.disk()).is_ok());
        assert!(
            !world.state_dir.join("run-a").join("link").exists(),
            "the link itself is a leftover"
        );
        assert!(
            !world.state_dir.join("run-b").exists(),
            "run-b, with the link in it, is gone"
        );
        assert!(
            victim.exists(),
            "the folder the links pointed at is untouched"
        );
        assert!(outside.join("sub").join("g.txt").exists());
        assert!(!world.list.exists());
    }

    #[test]
    fn a_state_folder_reached_through_a_link_of_its_own_is_still_the_state_folder() {
        // %LOCALAPPDATA% redirected to another disk is a link, and every path under it is spelled
        // through it: the base being a link is not what the list guards against.
        let world = CleanupWorld::new();
        let alias = world.temp.path().join("U-alias");
        let link = DirectoryLink::new(&alias, &world.state_dir);
        if !link.made() {
            eprintln!("this machine does not let the test make a directory link");
            return;
        }
        let leftover = world.file(world.state_dir.join("run-1").join("x.exe"));
        assert!(append_cleanup(&alias, &alias.join("run-1"), world.disk()).is_ok());
        assert!(process_cleanup_list(&alias, &world.root, world.disk()).is_ok());
        assert!(!leftover.exists());
        assert!(!world.list.exists());
    }

    #[test]
    fn a_folder_nested_deeper_than_the_updater_ever_makes_is_left_alone() {
        let world = CleanupWorld::new();
        let mut deep = world.state_dir.join("run-deep");
        for _ in 0..70 {
            deep.push("d");
        }
        if let Err(error) = std::fs::create_dir_all(&deep) {
            eprintln!("this machine cannot make a path that long: {error}");
            return;
        }
        world.list_lines(&[world.state_dir.join("run-deep")]);
        assert!(world.process(world.disk()).is_ok());
        assert!(
            deep.exists(),
            "the walk gives up past 64 levels instead of recursing without end"
        );
        assert_eq!(
            read_file(&world.list),
            line_of(&world.state_dir.join("run-deep")),
            "and it stays listed"
        );
    }
}

// ---- how this process was started (process.rs) -------------------------------------------------
//
// The verbs refuse an apply in a process that holds the elevated half of a split token, and take
// the answer from process_is_elevated. Whether this run of the suite is such a process is not for
// a test to assume, so what is held here is true of every run: the answer does not change while
// the process lives, and it is never "yes" for a token Windows itself calls not elevated. That
// second question is put to Windows another way than the function puts its own (TokenElevation,
// where the function reads TokenElevationType): the elevated half of a split token is an elevated
// token, so a yes from the one with a no from the other cannot be.
//
// WHAT THAT CANNOT SHOW: a "yes". A suite started the ordinary way holds no elevated half, and no
// test can give itself one. So the function's one decision (which kind of token is a yes) is a
// function of its own (spawn_win.rs, detail::is_elevated_half) and every kind is put to it below.
// What stays unshown in an ordinary run is only that process_is_elevated hands it the kind Windows
// gave: that is seen by a suite that itself holds the elevated half.

mod process_is_elevated {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn only_the_elevated_half_of_a_split_token_is_a_yes() {
        use katana_update::process::detail::is_elevated_half;
        // The three kinds Windows names (winnt.h, TOKEN_ELEVATION_TYPE), and what the ruling makes
        // of each: the elevated half of a split token is refused an apply; the other half is an
        // ordinary program; and a token that was never split (User Account Control off, or the
        // built-in administrator) has no unelevated half that could interfere, so it is not.
        const TOKEN_ELEVATION_TYPE_DEFAULT: u32 = 1;
        const TOKEN_ELEVATION_TYPE_FULL: u32 = 2;
        const TOKEN_ELEVATION_TYPE_LIMITED: u32 = 3;
        assert!(is_elevated_half(TOKEN_ELEVATION_TYPE_FULL));
        assert!(!is_elevated_half(TOKEN_ELEVATION_TYPE_LIMITED));
        assert!(!is_elevated_half(TOKEN_ELEVATION_TYPE_DEFAULT));
        // And a number that is none of the three, which is what an answer nobody could read is.
        assert!(!is_elevated_half(0));
        assert!(!is_elevated_half(4));
    }

    // Whether Windows calls this process's token elevated (TokenElevation).
    #[cfg(windows)]
    fn token_is_elevated() -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::GetTokenInformation;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        // winnt.h: TOKEN_QUERY, and TokenElevation of TOKEN_INFORMATION_CLASS, whose answer is a
        // TOKEN_ELEVATION, one DWORD.
        const TOKEN_QUERY: u32 = 0x8;
        const TOKEN_ELEVATION_CLASS: i32 = 20;
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo-handle of this process, and a place for the token handle.
        let opened =
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } != 0;
        assert!(opened, "the process token");
        let mut held: u32 = 0;
        let mut written: u32 = 0;
        // SAFETY: the answer is a TOKEN_ELEVATION, four bytes, written into `held`.
        let asked = unsafe {
            GetTokenInformation(
                token,
                TOKEN_ELEVATION_CLASS,
                (&raw mut held).cast(),
                4,
                &raw mut written,
            )
        } != 0;
        // SAFETY: `token` is the handle opened above, closed once, here.
        unsafe { CloseHandle(token) };
        assert!(asked, "the token's elevation");
        held != 0
    }

    #[test]
    fn the_answer_is_steady_and_is_never_yes_for_a_token_windows_calls_not_elevated() {
        let elevated = process_is_elevated();
        assert_eq!(process_is_elevated(), elevated);
        #[cfg(windows)]
        {
            if !token_is_elevated() {
                assert!(!elevated);
            }
        }
        #[cfg(not(windows))]
        assert!(!elevated, "there is no split token here");
    }
}
