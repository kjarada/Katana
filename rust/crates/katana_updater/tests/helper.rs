//! The helper program (src/lib.rs): its process scan, its registry, its command line, its
//! self-test, and the whole swap driven through the real katana_updater_test binary. The twin of
//! tests/update/test_helper.cpp.
//!
//! WHERE THE EXPECTATIONS COME FROM (CLAUDE.md, section 4). Nothing here is copied from what the
//! program printed:
//!
//! * the process scan is held to processes this test starts itself, whose image path it knows,
//!   and to the operating system's own answers (the process's id, the path it started, and the
//!   file system's identity for a file where two spellings of its path must be told to be one
//!   file);
//! * the registry is read back with RegGetValueW and written for the reader with
//!   RegSetKeyValueW, not through the code under test, and always under a key of the test's own;
//! * the import table is read from the PE file by the code below, not by the helper;
//! * the tree an update must leave is built from the specification of the release, in another
//!   folder (the swap's harness, apply_harness), or is the T3 fixtures' (the Python producer,
//!   signed by openssl);
//! * exit codes are the table at the top of src/lib.rs, which is a design document and not a log;
//! * the self-test's signature is RFC 8032 TEST 3, and the hostile configuration is shown to be
//!   hostile by a program that does not harden (the openssl command, when it is installed).
//!
//! WHAT A TEST HERE NEVER DOES:
//!
//! * start a file that is not a program. Windows answers a text file named .exe with a message box
//!   that somebody has to click. Every program started is a real one: the helper, and
//!   katana_updater_stub standing in for Katana (in the T3 fixtures, whose katana.exe is text, by
//!   the test helper's --stand-in);
//! * start a program other than through the Launcher, which turns that box off for the start: the
//!   stubs left running are started by it too, and say themselves which process they are
//!   (Sleeper, below);
//! * write the machine's own RunOnce, Uninstall or installer keys. Every registry write goes under
//!   HKCU\Software\KatanaUpdateTest-<unique>, which is deleted with the test; the helper built for
//!   the tests has no way to the machine's keys at all (src/lib.rs).
//!
//! Windows only, as the C++ file is: the process scan and the registry are, and the POSIX seams
//! are T18's. Written, not compiled, until the port is built on Windows (docs/rust.md, step 8). A
//! test that would pass whatever the program did is a defect, so each end-to-end case also asserts
//! the disk: the whole-tree hash of the install against the tree built from the specification.
//!
//! TWO THINGS THE PORT CHANGES. The C++ cases run each in a process of its own (CTest); these share
//! one, on parallel threads, and a case changes what is the whole process's - its environment,
//! which every program it starts inherits, and once HKEY_CURRENT_USER itself - so each case holds
//! one lock from its first line to its last (serial). And cargo builds a test program into
//! target/<profile>/deps and the programs one folder above, where the C++ build puts all of them
//! in one folder: the one case whose premise is that folder copies the stub beside this test
//! program instead of assuming it is there.

#![cfg(windows)]
#![allow(
    unsafe_code,
    reason = "the registry, the processes and the file identities a test reads behind the code's \
              back are Win32 calls, foreign functions"
)]

#[path = "../../katana_update/tests/test_key/mod.rs"]
pub mod test_key;

#[path = "../../katana_update/tests/apply_harness/mod.rs"]
pub mod apply_harness;

use apply_harness::{
    FileSpec, INSTALL_JSON, LogFs, ReleaseSpec, Rig, Scenario, Scratch, StdFs,
    build_fixture_scenario_of, difference, installed_json, is_present, leftovers_in_update,
    manifest_json, read_text, remove_all_with_retry, sign_line, snapshot, tree_hash, write_install,
    write_stage, write_text,
};
use katana_core::sha256::sha256_hex;
use katana_core::{ErrorCode, Result, Status};
use katana_update::apply::{APPLIER_PROTOCOL, ApplyOutcome, ApplyRequest, helper_arguments};
use katana_update::manifest::FileClass;
use katana_update::process::{
    RunningProgram, SpawnRequest, SpawnResult, make_system_launcher, make_system_process_ops,
    running_programs,
};
use katana_update::recover::recover;
use katana_update::registry::{RUN_ONCE_NAME, make_registry_ops_under, make_system_registry_ops};
use katana_update::state::{
    LastResult, health_marker_path, read_last_result, read_state, run_path,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Globalization::CompareStringOrdinal;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle, GetLongPathNameW, GetShortPathNameW,
    SearchPathW,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteKeyW,
    RegDeleteTreeW, RegGetValueW, RegOpenKeyExW, RegOverridePredefKey, RegSetKeyValueW,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{CharLowerBuffW, CharUpperBuffW};

// The values of the Windows SDK's headers (winerror.h, winnt.h, winreg.h, winbase.h,
// stringapiset.h, minwindef.h).
const ERROR_SUCCESS: u32 = 0;
const CSTR_EQUAL: i32 = 2;
const KEY_READ: u32 = 0x0002_0019;
const KEY_ALL_ACCESS: u32 = 0x000F_003F;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_DWORD: u32 = 4;
const RRF_RT_REG_SZ: u32 = 0x0000_0002;
const RRF_RT_REG_EXPAND_SZ: u32 = 0x0000_0004;
const RRF_NOEXPAND: u32 = 0x1000_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const PROCESS_TERMINATE: u32 = 0x0001;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const WAIT_TIMEOUT: u32 = 0x0000_0102;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const MAX_PATH: usize = 260;

// How long a test waits for a program: for a run to end (run_program), and for a program that was
// started and left running to say that it is there (Sleeper, World::wait_for_log). ONE figure for
// all of them, and a generous one, because what it bounds is the machine and not the helper: a
// program is read by the virus scanner the first time it is started, every test process here
// makes programs of its own (a copy of the helper, the stubs of an install), and the suite runs a
// dozen such processes at once. The C++'s integration run of 2026-10-14, 14 tests at a time: the
// cases that only start the two freshly built helpers a few times took 23 s each (0.7 s for two
// starts on the machine at rest), and one swap took 31 s, with every outcome right - while two
// cases that gave such a thing 30 s failed. A wait ends the moment what it waits for happens, so
// the figure costs a passing test nothing; it is how long a test that is going to fail takes.
const PATIENCE: Duration = Duration::from_secs(120);

// ---- the order of the cases -----------------------------------------------------------------------

// Every case runs alone (see the top): this is taken first and held to the end.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- small helpers --------------------------------------------------------------------------------

fn wide_z(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

fn from_wide(units: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(units))
}

// A path as UTF-8, an unpaired surrogate as U+FFFD (the C++'s utf8FromWide).
fn utf8(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn equal_ignoring_case(a: &[u16], b: &[u16]) -> bool {
    // SAFETY: both slices are read for exactly their lengths, which fit an int (paths).
    a.len() == b.len()
        && unsafe {
            CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1)
        } == CSTR_EQUAL
}

fn output_of(ran: &SpawnResult) -> String {
    String::from_utf8_lossy(&ran.output).into_owned()
}

// The value of the first `key=` line of a program's output, or "" when there is none.
fn value_of(output: &str, key: &str) -> String {
    for line in output.split('\n') {
        let text = line.strip_suffix('\r').unwrap_or(line);
        if text.len() > key.len() && text.starts_with(key) && text.as_bytes()[key.len()] == b'=' {
            return String::from(&text[key.len() + 1..]);
        }
    }
    String::new()
}

fn describe(ran: &SpawnResult) -> String {
    format!(
        "exit {}{}, output:\n{}",
        ran.exit_code,
        if ran.timed_out { " (timed out)" } else { "" },
        output_of(ran)
    )
}

fn lines_of(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

fn text_of_file(path: &Path) -> String {
    String::from_utf8_lossy(&read_text(path)).into_owned()
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| String::from(word)).collect()
}

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

fn set_variable(name: &str, value: Option<&OsStr>) {
    // SAFETY: the cases run one at a time (serial), and while one changes the environment no
    // thread of it reads the environment: a program is started after the change, not during it.
    unsafe {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}

// A variable of the test process's environment for the length of a scope: the programs it starts
// inherit it. Restored afterwards, latest first, so the cases cannot affect one another.
#[derive(Default)]
struct ScopedEnvironment {
    saved: RefCell<Vec<(String, Option<OsString>)>>,
}

impl ScopedEnvironment {
    fn new() -> Self {
        Self::default()
    }

    // Set to nothing is not set, as Windows reads it.
    fn remember(&self, name: &str) {
        let value = std::env::var_os(name).filter(|value| !value.is_empty());
        self.saved.borrow_mut().push((String::from(name), value));
    }

    fn set(&self, name: &str, value: impl AsRef<OsStr>) {
        self.remember(name);
        set_variable(name, Some(value.as_ref()));
    }

    fn unset(&self, name: &str) {
        self.remember(name);
        set_variable(name, None);
    }
}

impl Drop for ScopedEnvironment {
    fn drop(&mut self) {
        for (name, value) in self.saved.get_mut().iter().rev() {
            set_variable(name, value.as_deref());
        }
    }
}

// Runs a program to its end with the system Launcher: its exit code and what it printed. A program
// that cannot be started fails the case.
fn run_program_in(
    executable: &Path,
    arguments: Vec<String>,
    working_directory: &Path,
    timeout: Duration,
) -> SpawnResult {
    let request = SpawnRequest {
        executable: executable.to_path_buf(),
        arguments,
        working_directory: working_directory.to_path_buf(),
        timeout,
    };
    match make_system_launcher().run(&request) {
        Ok(ran) => ran,
        Err(error) => panic!(
            "{} could not be run: {}",
            utf8(executable),
            error.describe()
        ),
    }
}

fn run_program(executable: &Path, arguments: Vec<String>) -> SpawnResult {
    run_program_in(executable, arguments, Path::new(""), PATIENCE)
}

// A program left running: the stub, asked to sleep. Ended with the object. It is started by the
// Launcher like every other program here, so that the Launcher's precaution covers this start
// too: should the copy of the stub not be a program after all (a copy cut short), that is an error
// and never a message box of the system's. The Launcher hands back no process id, so the stub
// writes its own into a file this names for it (KATANA_STUB_PID_FILE), and the process is then
// opened by that id - to end it with, and so that the id stays this process's for as long as a
// case asks about it.
struct Sleeper {
    process: HANDLE,
    pid: u32,
    executable: PathBuf,
}

impl Sleeper {
    fn new(executable: &Path, milliseconds: u32) -> Self {
        static NOTES: LazyLock<Scratch> = LazyLock::new(|| Scratch::new("sleepers"));
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let note = NOTES
            .path()
            .join(format!("{}.pid", COUNTER.fetch_add(1, Ordering::Relaxed)));
        {
            // Only the program started now is told where to write: the variable is gone again
            // before anything else is started.
            let environment = ScopedEnvironment::new();
            environment.set("KATANA_STUB_PID_FILE", &note);
            let request = SpawnRequest {
                executable: executable.to_path_buf(),
                arguments: vec![String::from("--sleep"), format!("{milliseconds}")],
                working_directory: executable
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default(),
                ..SpawnRequest::default()
            };
            if let Err(error) = make_system_launcher().start_detached(&request) {
                panic!("could not start {}: {}", utf8(executable), error.describe());
            }
        }
        // The stub writes the digits and a line feed in one write, first thing.
        let mut text = Vec::new();
        let end = Instant::now() + PATIENCE;
        while Instant::now() < end {
            text = read_text(&note);
            if text.last() == Some(&b'\n') {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut pid: u32 = 0;
        for &c in &text {
            if c.is_ascii_digit() {
                pid = pid.wrapping_mul(10).wrapping_add(u32::from(c - b'0'));
            }
        }
        assert_ne!(
            pid,
            0,
            "{} was started and did not say which process it is",
            utf8(executable)
        );
        // SAFETY: OpenProcess reads its three arguments and returns a handle of ours, or null.
        let process = unsafe {
            OpenProcess(
                SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        assert!(
            !process.is_null(),
            "process {pid} could not be opened: {}",
            std::io::Error::last_os_error()
        );
        Self {
            process,
            pid,
            executable: executable.to_path_buf(),
        }
    }

    fn terminate(&self) {
        if self.process.is_null() {
            return;
        }
        // SAFETY: the handle is this object's, opened with SYNCHRONIZE and PROCESS_TERMINATE.
        unsafe {
            if WaitForSingleObject(self.process, 0) == WAIT_TIMEOUT {
                TerminateProcess(self.process, 1);
            }
            WaitForSingleObject(self.process, 10_000);
        }
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        self.terminate();
        if !self.process.is_null() {
            // SAFETY: the handle is this object's, closed once.
            unsafe { CloseHandle(self.process) };
        }
    }
}

fn copy_program(from: &Path, to: &Path) {
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::copy(from, to) {
        panic!("{} -> {}: {error}", utf8(from), utf8(to));
    }
}

// The helper program, copied ONCE per test process into the temp folder, and each world's copy a
// hard link to that (a plain copy where the volume cannot link). The build tree and the temp folder
// are often on different volumes, which rules out linking to the build's own file. A world's helper
// is still a program run from a folder of its own, which is what the helper copy is; what the
// cases that make dozens of worlds save is a copy each, scanned anew by the virus checker every
// time, which was most of their time. (The shared folder is a static, which Rust never drops: it
// stays in the temp folder after the run, where the C++'s goes at exit.)
fn link_program(from: &Path, to: &Path) {
    static SHARED: LazyLock<Scratch> = LazyLock::new(|| Scratch::new("helper-copy"));
    static COPIES: Mutex<BTreeMap<String, PathBuf>> = Mutex::new(BTreeMap::new());
    let source = {
        let mut copies = COPIES.lock().unwrap_or_else(PoisonError::into_inner);
        copies
            .entry(utf8(from))
            .or_insert_with(|| {
                let copy = SHARED.path().join(from.file_name().unwrap_or_default());
                copy_program(from, &copy);
                copy
            })
            .clone()
    };
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::hard_link(&source, to).is_err() {
        copy_program(&source, to);
    }
}

// A folder named with a space and letters outside every ANSI code page at once: the helper must be
// able to work in a profile like that (it reads its command line as UTF-16). Short, like the other
// names below: the RunOnce command names the helper copy AND the install, both under the temp
// folder here, and a RunOnce command is at most 260 characters (registry.rs).
const UNICODE_FOLDER: &str = "K\u{e4} \u{65e5}";

// Cargo gives these with the separators of the target folder; the programs report paths with the
// native separator, and the cases compare them as text.
fn preferred(path: &str) -> PathBuf {
    PathBuf::from(path.replace('/', "\\"))
}

fn stub() -> PathBuf {
    preferred(env!("CARGO_BIN_EXE_katana_updater_stub"))
}

fn helper() -> PathBuf {
    preferred(env!("CARGO_BIN_EXE_katana_updater"))
}

fn test_helper() -> PathBuf {
    preferred(env!("CARGO_BIN_EXE_katana_updater_test"))
}

// The long spelling of an existing folder: Windows may hand out 8.3 names for the temp folder
// (C:\Users\RUNNER~1\... on a build service), and the paths the programs report back are compared
// with the test's as text. GetLongPathNameW, which leaves nothing short.
fn long_spelling(folder: &Path) -> PathBuf {
    let input = wide_z(folder);
    let mut buffer = vec![0u16; input.len() + 64];
    for _ in 0..3 {
        // SAFETY: `input` is NUL-terminated and `buffer` holds as many units as the call is told.
        let length =
            unsafe { GetLongPathNameW(input.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) }
                as usize;
        if length == 0 {
            return folder.to_path_buf();
        }
        if length < buffer.len() {
            buffer.truncate(length);
            return from_wide(&buffer);
        }
        buffer = vec![0u16; length + 1];
    }
    folder.to_path_buf()
}

// The file system's identity of a file: its volume and its number on it. No access is asked for,
// only the attributes: a program that is running can be opened so.
fn identity(path: &Path) -> Option<(u32, u32, u32)> {
    let file = OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    // SAFETY: a plain structure of integers, for the call to fill.
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is the open file's, and the structure is the size the call writes.
    let known = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } != 0;
    known.then_some((
        information.dwVolumeSerialNumber,
        information.nFileIndexHigh,
        information.nFileIndexLow,
    ))
}

// Whether two paths name ONE file, by the file system's own identity for it, and not by how the
// paths are spelled. A short name against a long one, a junction against the folder it points to,
// either case: the spelling a program reports of its own image is the kernel's choice, and the
// test does not guess it.
fn same_file(first: &Path, second: &Path) -> bool {
    let one = identity(first);
    one.is_some() && one == identity(second)
}

fn system_folder() -> PathBuf {
    let mut folder = vec![0u16; MAX_PATH];
    // SAFETY: the buffer holds as many units as the call is told.
    let length = unsafe { GetSystemDirectoryW(folder.as_mut_ptr(), folder.len() as u32) } as usize;
    folder.truncate(if length < MAX_PATH { length } else { 0 });
    from_wide(&folder)
}

// Makes `link` a junction to the folder `target` (cmd's mklink /J, which needs no privilege). What
// cmd said, where the machine's policy refuses it.
fn make_junction(link: &Path, target: &Path) -> std::result::Result<(), String> {
    let made = run_program(
        &system_folder().join("cmd.exe"),
        vec![
            String::from("/c"),
            String::from("mklink"),
            String::from("/J"),
            utf8(link),
            utf8(target),
        ],
    );
    if made.exit_code == 0 {
        Ok(())
    } else {
        Err(output_of(&made))
    }
}

// ---- a registry of the test's own -----------------------------------------------------------------

fn key_exists(subkey: &str) -> bool {
    let key = wide_z(subkey);
    let mut opened: HKEY = null_mut();
    // SAFETY: `key` is NUL-terminated; the handle the call opens is closed below.
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut opened) };
    if status == ERROR_SUCCESS {
        // SAFETY: the key opened above, closed once.
        unsafe { RegCloseKey(opened) };
    }
    status == ERROR_SUCCESS
}

// HKCU\Software\KatanaUpdateTest-<unique>, deleted with the object. Everything the code under test
// writes goes below it.
struct TestHive {
    base: String,
}

impl TestHive {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let seed = format!(
            "{}:{:?}:{}",
            std::process::id(),
            Instant::now(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        Self {
            base: format!("Software\\KatanaUpdateTest-{}", &sha256_hex(seed)[..12]),
        }
    }

    fn key(&self, relative: &str) -> String {
        format!("{}\\{relative}", self.base)
    }

    fn run_once_key(&self) -> String {
        self.key(r"Software\Microsoft\Windows\CurrentVersion\RunOnce")
    }

    fn uninstall_key(&self) -> String {
        self.key(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Katana")
    }

    fn location_key(&self) -> String {
        self.key(r"Software\Jarada\Katana")
    }

    fn exists(&self) -> bool {
        key_exists(&self.base)
    }
}

impl Drop for TestHive {
    fn drop(&mut self) {
        // The tree below the key, then the key: nothing of a test stays in the registry.
        let key = wide_z(&self.base);
        // SAFETY: `key` is NUL-terminated; both calls only delete under the test's own key.
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
            RegDeleteKeyW(HKEY_CURRENT_USER, key.as_ptr());
        }
    }
}

// A string value read back directly, with its registry type. `name` None is the default value.
fn read_string_typed(subkey: &str, name: Option<&str>) -> Option<(String, u32)> {
    let key = wide_z(subkey);
    let name = name.map(wide_z);
    let mut value = vec![0u16; 4096];
    let mut bytes = (value.len() * 2) as u32;
    let mut kind: u32 = 0;
    // SAFETY: the key and the name are NUL-terminated (or the name null); `value` holds `bytes`
    // bytes, which the call writes at most.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ref().map_or(null(), |name| name.as_ptr()),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
            &mut kind,
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let units = (bytes as usize / 2).min(value.len());
    let end = value[..units]
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(units);
    Some((String::from_utf16_lossy(&value[..end]), kind))
}

fn read_string(subkey: &str, name: Option<&str>) -> Option<String> {
    read_string_typed(subkey, name).map(|(text, _)| text)
}

// A string value written directly, under a key of the test's own.
fn write_value(subkey: &str, name: Option<&str>, kind: u32, text: impl AsRef<OsStr>) {
    let key = wide_z(subkey);
    let name = name.map(wide_z);
    let data = wide_z(text);
    // SAFETY: the key and the name are NUL-terminated (or the name null); the data is the text and
    // its terminator, `data.len() * 2` bytes.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ref().map_or(null(), |name| name.as_ptr()),
            kind,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    assert_eq!(status, ERROR_SUCCESS, "RegSetKeyValueW {subkey}");
}

fn delete_value(subkey: &str, name: &str) {
    let key = wide_z(subkey);
    let name = wide_z(name);
    // SAFETY: both NUL-terminated.
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
}

// The value name the helper registers (registry.rs), and the machine's own key for it. The
// machine's key is only ever READ here - but for one line, which takes a value of this file's own
// out of it again should a redirection of HKEY_CURRENT_USER ever fail to hold
// (the_shipped_registry_..., below), and is not reached otherwise.
const RUN_ONCE_VALUE: &str = "KatanaUpdateRecover";
const MACHINE_RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";

// HKEY_CURRENT_USER sent, for this process, to a key of the test's own (RegOverridePredefKey: what
// an installer uses to see what a component would write), and back when the object goes. So the
// production registry - whose key paths are relative to HKEY_CURRENT_USER - can be run as it is and
// still write nothing of the machine's.
struct RedirectedCurrentUser {
    target: HKEY,
    active: bool,
}

impl RedirectedCurrentUser {
    fn new(subkey: &str) -> Self {
        let key = wide_z(subkey);
        let mut target: HKEY = null_mut();
        // SAFETY: `key` is NUL-terminated; the key the call opens is closed with the object.
        let created = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                null(),
                0,
                KEY_ALL_ACCESS,
                null(),
                &mut target,
                null_mut(),
            )
        };
        if created != ERROR_SUCCESS {
            return Self {
                target: null_mut(),
                active: false,
            };
        }
        // SAFETY: both keys are open.
        let active = unsafe { RegOverridePredefKey(HKEY_CURRENT_USER, target) } == ERROR_SUCCESS;
        Self { target, active }
    }
}

impl Drop for RedirectedCurrentUser {
    fn drop(&mut self) {
        // SAFETY: a null new key puts the predefined key back; the target is this object's, closed
        // once.
        unsafe {
            if self.active {
                RegOverridePredefKey(HKEY_CURRENT_USER, null_mut());
            }
            if !self.target.is_null() {
                RegCloseKey(self.target);
            }
        }
    }
}

// ---- the stub, and an install built around it ------------------------------------------------------

fn stub_bytes() -> &'static [u8] {
    static BYTES: LazyLock<Vec<u8>> = LazyLock::new(|| read_text(&stub()));
    BYTES.as_slice()
}

// The stub with a tag appended to the file: the program still runs (the PE loader ignores bytes
// after the last section), and reads its tag from its own file (src/bin/katana_updater_stub.rs).
fn tagged_stub(tag: &str) -> Vec<u8> {
    let mut bytes = stub_bytes().to_vec();
    bytes.extend_from_slice(format!("\nKATANA-STUB-TAG={tag}\n").as_bytes());
    bytes
}

const TRANSACTION: &str = "a1b2c3d4e5f60718";

// The test helper's --timings: wait for the window, wait for the programs under R, poll interval,
// health wait, rename retry - milliseconds.
const TIMINGS: &str = "500,1500,50,20000,30000";

// A health wait that no run can be taken to have sat out: ten minutes, where a shipped helper waits
// one (apply.rs, ApplyTimings) and a test gives a whole run of the helper two (PATIENCE). It is for
// the cases whose point is that the helper does NOT wait - for a new version that has ended, for
// one that could not be started - and for the one that holds the new version in its health check
// until the test is ready. What they must tell apart is "the helper went on when the program
// ended" and "the helper waited the time out", and against a wait of 60 s that took a limit on the
// whole run below 60 s: a bet that everything else in a swap - starting two programs for the first
// time, hashing, copying, the journal - takes less than the limit on a machine doing a dozen other
// things, which the run PATIENCE tells of lost by a second (31.04 s against a limit of 30 s). With
// this wait the two are five limits apart: a helper that sits the wait out is still running when
// the test's limit ends it (SpawnResult::timed_out), and a slow machine has that whole limit, the
// same as in every other case here.
const LONG_HEALTH_WAIT: Duration = Duration::from_secs(600);
const _: () = assert!(
    LONG_HEALTH_WAIT.as_secs() >= 5 * PATIENCE.as_secs(),
    "a run must end, or be ended, long before this wait could"
);

// TIMINGS with that health wait, and with the rename retry of a case that asks for another.
fn timings_with_a_long_health_wait(rename_retry: Duration) -> String {
    format!(
        "500,1500,50,{},{}",
        LONG_HEALTH_WAIT.as_millis(),
        rename_retry.as_millis()
    )
}

// A release of the stub's: v1 and v2 differ in the programs (the tag), one library, one data file,
// and each has a file the other lacks. The seed differs too, and an update must leave v1's.
fn stub_release(tag: &str, version: &str, sequence: u64) -> ReleaseSpec {
    let second = tag == "v2";
    ReleaseSpec {
        version: String::from(version),
        tag: format!("v{version}"),
        sequence,
        files: vec![
            FileSpec::new("bin/katana.exe", tagged_stub(tag), FileClass::Program, true),
            FileSpec::new(
                "bin/katana_cli.exe",
                tagged_stub(tag),
                FileClass::Program,
                true,
            ),
            FileSpec::new(
                "bin/libcore-1.dll",
                format!("core library {tag}\n"),
                FileClass::Lib,
                false,
            ),
            FileSpec::new(
                if second {
                    "bin/libnew-2.dll"
                } else {
                    "bin/libold-1.dll"
                },
                format!("only in {tag}\n"),
                FileClass::Lib,
                false,
            ),
            FileSpec::new(
                "share/katana/readme.txt",
                format!("readme {tag}\n"),
                FileClass::Data,
                false,
            ),
            FileSpec::new(
                if second {
                    "share/katana/help/index.txt"
                } else {
                    "share/katana/legacy/old.txt"
                },
                format!("only in {tag}\n"),
                FileClass::Data,
                false,
            ),
            FileSpec::new(
                "etc/katana/settings.ini",
                format!("seed {tag}\n"),
                FileClass::Seed,
                false,
            ),
        ],
        ..ReleaseSpec::default()
    }
}

fn log_of(log: &Path) -> String {
    if is_present(log) {
        text_of_file(log)
    } else {
        String::new()
    }
}

// The names directly inside a folder, with "/" after a folder's.
fn names_in(folder: &Path) -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .map(|entry| {
            let mut name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() {
                name.push('/');
            }
            name
        })
        .collect()
}

// The RunOnce command the helper registers for a helper copy and an install (apply.rs, step 4; the
// quoting is CommandLineToArgvW's, command_line.rs): both paths hold a space here, so both are
// quoted.
fn run_once_command(helper_copy: &Path, root: &Path) -> String {
    format!("\"{}\" --recover \"{}\"", utf8(helper_copy), utf8(root))
}

// Everything one end-to-end case needs: a v1 install, a v2 stage, the tree v2 must leave, the
// state folder, the helper copy in it (as preflight makes it), a registry of its own, and the
// environment the stub reads. Its fields go in the C++'s order of destruction: the environment
// is put back first, then the hive is removed, and the folder goes last.
struct World {
    environment: ScopedEnvironment,
    hive: TestHive,
    top: PathBuf,
    root: PathBuf,
    stage: PathBuf,
    state: PathBuf,
    expected: PathBuf,
    log: PathBuf,
    v1: ReleaseSpec,
    v2: ReleaseSpec,
    v1_hash: String,
    v2_hash: String,
    v1_snapshot: BTreeMap<String, String>,
    scratch: Scratch,
}

impl World {
    fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let top = long_spelling(scratch.path()).join(UNICODE_FOLDER);
        let root = top.join("Katana 1");
        let stage = top.join("stage");
        let state = top.join("state");
        let expected = top.join("expected");
        let log = top.join("stub.log");
        std::fs::create_dir_all(&state).expect("the state folder");

        let v1 = stub_release("v1", "0.9.0", 1000);
        let v2 = stub_release("v2", "0.10.0", 2000);
        write_install(&root, &v1);
        write_stage(&stage, &v2);
        // The tree a good update leaves, from the specification: v2's files, the seed as v1 had it,
        // the files only v1 had gone, and v2's records.
        for file in &v2.files {
            if file.file_class != FileClass::Seed {
                write_text(&expected.join(&file.path), &file.content);
            }
        }
        let seed = v1
            .find("etc/katana/settings.ini")
            .expect("v1's seed")
            .content
            .clone();
        write_text(&expected.join("etc/katana/settings.ini"), seed);
        write_text(&expected.join("share/katana/install.json"), INSTALL_JSON);
        write_text(
            &expected.join("share/katana/installed.json"),
            installed_json(&v2),
        );
        let manifest = manifest_json(&v2);
        write_text(
            &expected.join("share/katana/installed.manifest.json.sig"),
            sign_line(&manifest),
        );
        write_text(
            &expected.join("share/katana/installed.manifest.json"),
            manifest,
        );
        let v1_hash = tree_hash(&root);
        let v1_snapshot = snapshot(&root);
        let v2_hash = tree_hash(&expected);
        let world = Self {
            environment: ScopedEnvironment::new(),
            hive: TestHive::new(),
            top,
            root,
            stage,
            state,
            expected,
            log,
            v1,
            v2,
            v1_hash,
            v2_hash,
            v1_snapshot,
            scratch,
        };
        world.install_helper(TRANSACTION);
        world
    }

    // The helper as preflight leaves it: a copy in U/run-<tx>/, named as recover() looks for it.
    fn install_helper(&self, transaction: &str) {
        link_program(&test_helper(), &self.helper_path(transaction));
    }

    fn helper_path(&self, transaction: &str) -> PathBuf {
        run_path(&self.state, transaction).join("katana_updater.exe")
    }

    // The command line of the swap, as the engine writes it (helper_arguments).
    fn engine_arguments(&self, transaction: &str, wait_pid: u64) -> Vec<String> {
        let request = ApplyRequest {
            root: self.root.clone(),
            stage_directory: self.stage.clone(),
            state_directory: self.state.clone(),
            transaction: String::from(transaction),
            wait_pid,
            relaunch: self.root.join("bin").join("katana.exe"),
            ..ApplyRequest::default()
        };
        helper_arguments(&request)
    }

    // ... plus the test options: the registry of this world, and waits a test can afford.
    fn apply_arguments(&self, transaction: &str, wait_pid: u64, timings: &str) -> Vec<String> {
        let mut arguments = self.engine_arguments(transaction, wait_pid);
        arguments.extend([
            String::from("--registry-base"),
            self.hive.base.clone(),
            String::from("--timings"),
            String::from(timings),
        ]);
        arguments
    }

    fn with_test_options(&self, mut arguments: Vec<String>) -> Vec<String> {
        arguments.push(String::from("--registry-base"));
        arguments.push(self.hive.base.clone());
        arguments
    }

    // `--recover R` with the test options.
    fn recover_arguments(&self) -> Vec<String> {
        self.with_test_options(vec![String::from("--recover"), utf8(&self.root)])
    }

    // The environment of the programs started now: the stub's switches, the state folder it
    // writes the health marker into (and recover() looks for the helper copy in), the registry the
    // stub looks in, and the registry of a test helper whose command line carries no test option.
    // Held until the world ends.
    fn arrange_environment(&self, stub_mode: &str) {
        self.environment.set("KATANA_STUB_LOG", &self.log);
        self.environment.set("KATANA_STUB_MODE", stub_mode);
        self.environment.set("KATANA_UPDATE_STATE_DIR", &self.state);
        self.environment
            .set("KATANA_STUB_REGISTRY_BASE", &self.hive.base);
        self.environment
            .set("KATANA_UPDATER_TEST_REGISTRY_BASE", &self.hive.base);
    }

    fn run_helper(&self, arguments: Vec<String>, transaction: &str) -> SpawnResult {
        let helper = self.helper_path(transaction);
        let folder = helper.parent().map(Path::to_path_buf).unwrap_or_default();
        run_program_in(&helper, arguments, &folder, PATIENCE)
    }

    fn run_once(&self) -> String {
        read_string(&self.hive.run_once_key(), Some(RUN_ONCE_VALUE)).unwrap_or_default()
    }

    fn run_once_is_set(&self) -> bool {
        read_string(&self.hive.run_once_key(), Some(RUN_ONCE_VALUE)).is_some()
    }

    // What Windows does at a logon BEFORE it runs a RunOnce command: the value is taken away. In
    // this world's own key, like every registry write of a test.
    fn consume_run_once(&self) {
        delete_value(&self.hive.run_once_key(), RUN_ONCE_VALUE);
    }

    // Makes the install an installer's: the key the installer writes names this folder, and the
    // Uninstall entry (which only an installer makes) shows version 0.9.0.
    fn register_as_installer(&self) {
        write_value(&self.hive.location_key(), None, REG_SZ, &self.root);
        write_value(
            &self.hive.uninstall_key(),
            Some("DisplayVersion"),
            REG_SZ,
            "0.9.0",
        );
        write_value(
            &self.hive.uninstall_key(),
            Some("DisplayName"),
            REG_SZ,
            "Katana 0.9.0",
        );
    }

    fn display_version(&self) -> Option<String> {
        read_string(&self.hive.uninstall_key(), Some("DisplayVersion"))
    }

    fn display_name(&self) -> Option<String> {
        read_string(&self.hive.uninstall_key(), Some("DisplayName"))
    }

    fn stub_log(&self) -> String {
        log_of(&self.log)
    }

    fn log_has(&self, part: &str) -> bool {
        self.stub_log().contains(part)
    }

    // The stub the helper starts last is detached: wait for its line.
    fn wait_for_log(&self, part: &str) -> bool {
        let end = Instant::now() + PATIENCE;
        while Instant::now() < end {
            if self.log_has(part) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.log_has(part)
    }

    // Lets a stub started in the mode exit1_when_told go: the file it looks for, the log's name
    // with ".go" after it (src/bin/katana_updater_stub.rs).
    fn tell_the_stub_to_exit(&self) {
        let mut go = self.log.as_os_str().to_os_string();
        go.push(".go");
        write_text(Path::new(&go), "go\n");
    }

    fn expect_v1(&self) {
        assert_eq!(
            tree_hash(&self.root),
            self.v1_hash,
            "the tree must be v1\n{}",
            difference(&self.v1_snapshot, &snapshot(&self.root))
        );
    }

    fn expect_v2(&self) {
        assert_eq!(
            tree_hash(&self.root),
            self.v2_hash,
            "the tree must be v2\n{}",
            difference(&snapshot(&self.expected), &snapshot(&self.root))
        );
    }

    fn last(&self) -> LastResult {
        match read_last_result(&self.state) {
            Ok(Some(last)) => last,
            other => panic!("last.json was not written: {other:?}"),
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // The program the helper starts last is detached and may still be on its way out: its file
        // cannot be deleted while it runs, and the folder goes with this object.
        let processes = make_system_process_ops();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            match processes.holders_under(&self.root) {
                Ok(running) if !running.is_empty() => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => break,
            }
        }
    }
}

// A stub left running with its image at `path` (a copy of the stub, put there).
fn start_sleeper_at(path: &Path, milliseconds: u32) -> Box<Sleeper> {
    copy_program(&stub(), path);
    Box::new(Sleeper::new(path, milliseconds))
}

// ---- the T3 fixtures as an install and a stage -----------------------------------------------------

const FIXTURE_TRANSACTION: &str = "c1c2c3c4";
// The fixtures' v2 manifest, tests/update/fixtures/release/v2/Katana-update-win64.json.
const FIXTURE_V2_SEQUENCE: u64 = 1_792_312_800;

// The release pair the Python producer made and openssl signed (apply_harness,
// build_fixture_scenario_of), in a folder with a space and letters outside the code page, with the
// test helper copied where preflight puts it and a registry of its own. The fixtures' katana.exe
// is a text file: it is never started (see the top of this file).
struct FixtureWorld {
    hive: TestHive,
    log: PathBuf,
    top: PathBuf,
    s: Scenario,
}

impl FixtureWorld {
    fn new(label: &str, installed: &str, staged: &str) -> Self {
        let mut s = Scenario::new(label);
        let top = long_spelling(s.scratch.path()).join(UNICODE_FOLDER);
        s.root = top.join("Katana 1");
        s.stage = top.join("stage");
        s.state = top.join("state");
        s.expected = top.join("expected");
        let log = top.join("stub.log");
        std::fs::create_dir_all(&s.state).expect("the state folder");
        build_fixture_scenario_of(&mut s, installed, staged);
        let world = Self {
            hive: TestHive::new(),
            log,
            top,
            s,
        };
        link_program(&test_helper(), &world.helper());
        world
    }

    fn helper(&self) -> PathBuf {
        run_path(&self.s.state, FIXTURE_TRANSACTION).join("katana_updater.exe")
    }

    // The swap's command line with nothing to start at the end, and the test options.
    fn apply_arguments(&self, timings: &str) -> Vec<String> {
        let request = ApplyRequest {
            root: self.s.root.clone(),
            stage_directory: self.s.stage.clone(),
            state_directory: self.s.state.clone(),
            transaction: String::from(FIXTURE_TRANSACTION),
            ..ApplyRequest::default()
        };
        let mut arguments = helper_arguments(&request);
        arguments.extend([
            String::from("--registry-base"),
            self.hive.base.clone(),
            String::from("--timings"),
            String::from(timings),
        ]);
        arguments
    }

    fn run(&self, arguments: Vec<String>) -> SpawnResult {
        let helper = self.helper();
        let folder = helper.parent().map(Path::to_path_buf).unwrap_or_default();
        run_program_in(&helper, arguments, &folder, PATIENCE)
    }

    fn run_once_is_set(&self) -> bool {
        read_string(&self.hive.run_once_key(), Some(RUN_ONCE_VALUE)).is_some()
    }
}

// ---- the process scan -----------------------------------------------------------------------------

fn lists_pid(
    scanned: &Result<Vec<RunningProgram>>,
    pid: u32,
    expected_image: Option<&Path>,
) -> bool {
    let Ok(programs) = scanned else {
        return false;
    };
    for program in programs {
        if program.pid == u64::from(pid) {
            // The image the scan names must BE the file that was started, however it is spelled
            // (the temp folder's 8.3 name, a junction): one file by the file system's identity.
            return expected_image.is_none_or(|image| same_file(&program.image, image));
        }
    }
    false
}

fn upper_cased(mut text: Vec<u16>) -> Vec<u16> {
    // SAFETY: the buffer holds as many units as the call is told, and is changed in place.
    unsafe { CharUpperBuffW(text.as_mut_ptr(), text.len() as u32) };
    text
}

fn lower_cased(mut text: Vec<u16>) -> Vec<u16> {
    // SAFETY: the buffer holds as many units as the call is told, and is changed in place.
    unsafe { CharLowerBuffW(text.as_mut_ptr(), text.len() as u32) };
    text
}

mod helper_process_scan {
    use super::*;

    #[test]
    fn programs_running_from_under_the_root_are_found_whatever_way_the_root_is_spelled() {
        let _serial = serial();
        let scratch = Scratch::new("scan-spellings");
        let root = scratch.path().join(UNICODE_FOLDER).join("Install Katana");
        let sleeper = start_sleeper_at(&root.join("bin").join("katana_cli.exe"), 120_000);
        assert_ne!(sleeper.pid, 0);
        let processes = make_system_process_ops();

        let native: Vec<u16> = root.as_os_str().encode_wide().collect();
        let backslash = u16::from(b'\\');
        let slashes: Vec<u16> = native
            .iter()
            .map(|&unit| {
                if unit == backslash {
                    u16::from(b'/')
                } else {
                    unit
                }
            })
            .collect();
        let with = |suffix: &str| {
            let mut spelled = root.as_os_str().to_os_string();
            spelled.push(suffix);
            PathBuf::from(spelled)
        };
        let mut prefixed = OsString::from(r"\\?\");
        prefixed.push(root.as_os_str());
        let mut spellings: Vec<(&str, PathBuf)> = vec![
            ("as given", root.clone()),
            ("upper case", from_wide(&upper_cased(native.clone()))),
            ("lower case", from_wide(&lower_cased(native.clone()))),
            ("forward slashes", from_wide(&slashes)),
            ("a trailing separator", with("\\")),
            ("dot and dot-dot segments", with("\\bin\\..\\share\\..")),
            ("the extended-length prefix", PathBuf::from(prefixed)),
        ];
        // The short name of the folder, where the volume makes 8.3 names (it is the same folder).
        let input = wide_z(&root);
        let mut short_name = vec![0u16; MAX_PATH * 4];
        // SAFETY: `input` is NUL-terminated and `short_name` holds as many units as the call is
        // told.
        let short_length = unsafe {
            GetShortPathNameW(
                input.as_ptr(),
                short_name.as_mut_ptr(),
                short_name.len() as u32,
            )
        } as usize;
        if short_length > 0 && short_length < short_name.len() {
            short_name.truncate(short_length);
            if !equal_ignoring_case(&short_name, &native) {
                spellings.push(("the 8.3 short name", from_wide(&short_name)));
            }
        }

        for (what, spelling) in &spellings {
            let scanned = processes.holders_under(spelling);
            assert!(scanned.is_ok(), "{what}: {scanned:?}");
            assert!(
                lists_pid(&scanned, sleeper.pid, Some(sleeper.executable.as_path())),
                "{what}: the program started from {} was not listed under {}",
                utf8(&sleeper.executable),
                utf8(spelling)
            );
        }
    }

    #[test]
    fn a_root_reached_through_a_junction_finds_the_program_started_from_the_real_folder() {
        let _serial = serial();
        let scratch = Scratch::new("scan-junction");
        let root = scratch.path().join(UNICODE_FOLDER).join("Install Katana");
        let sleeper = start_sleeper_at(&root.join("bin").join("katana.exe"), 120_000);
        let junction = scratch.path().join("link");
        // Where the machine's policy refuses a junction the case cannot be made - on a developer's
        // machine. A build service makes them, and a skip there would pass for a test that ran.
        if let Err(said) = make_junction(&junction, &root) {
            crate::skip_unless_available_or_fail_in_ci!(format!(
                "a junction could not be made here: {said}"
            ));
        }
        let scanned = make_system_process_ops().holders_under(&junction);
        assert!(scanned.is_ok(), "{scanned:?}");
        assert!(
            lists_pid(&scanned, sleeper.pid, Some(sleeper.executable.as_path())),
            "the junction names the same folder"
        );
    }

    #[test]
    fn a_folder_that_only_shares_the_name_prefix_is_not_under_the_root() {
        let _serial = serial();
        let scratch = Scratch::new("scan-prefix");
        let root = scratch.path().join("Katana");
        let sibling = scratch.path().join("Katana2");
        let outside = start_sleeper_at(&sibling.join("bin").join("katana.exe"), 120_000);
        let inside = start_sleeper_at(&root.join("bin").join("katana.exe"), 120_000);
        let processes = make_system_process_ops();

        let scanned = processes.holders_under(&root);
        assert!(scanned.is_ok(), "{scanned:?}");
        assert!(lists_pid(&scanned, inside.pid, None));
        assert!(
            !lists_pid(&scanned, outside.pid, None),
            "C:\\...\\Katana2 is not under C:\\...\\Katana"
        );

        let other = processes.holders_under(&sibling);
        assert!(other.is_ok());
        assert!(lists_pid(&other, outside.pid, None));
        assert!(!lists_pid(&other, inside.pid, None));
    }

    #[test]
    fn a_program_that_has_ended_is_no_longer_listed_and_no_longer_alive() {
        let _serial = serial();
        let scratch = Scratch::new("scan-ended");
        let root = scratch.path().join("Install");
        let sleeper = start_sleeper_at(&root.join("bin").join("katana.exe"), 120_000);
        let processes = make_system_process_ops();
        let pid = sleeper.pid;

        assert!(processes.is_alive(u64::from(pid)));
        assert!(lists_pid(&processes.holders_under(&root), pid, None));

        // Ended, with this test still holding a handle to it: the id and the image name stay
        // readable, and neither question may answer as if it still ran.
        sleeper.terminate();
        assert!(
            !processes.is_alive(u64::from(pid)),
            "the process object outlives the process while a handle is open"
        );
        assert!(!lists_pid(&processes.holders_under(&root), pid, None));
    }

    #[test]
    fn the_process_that_asks_is_never_its_own_holder_and_the_scan_still_sees_its_neighbours() {
        let _serial = serial();
        // The test program and the stub share a folder: the test program's own image is under it,
        // and so is the neighbour's, so a scan that leaves this process out is excluding it, not
        // missing it. Cargo builds the test program into deps/ and the stub one folder above (see
        // the top), so the stub is copied beside this program, under a name of this run's own.
        let own = std::env::current_exe().expect("this test program's path");
        let folder = own.parent().expect("its folder").to_path_buf();
        let neighbour_program =
            folder.join(format!("katana_updater_stub-{}.exe", std::process::id()));
        copy_program(&stub(), &neighbour_program);
        // Removed once the neighbour has ended (declared first, so dropped after it).
        struct Removed(PathBuf);
        impl Drop for Removed {
            fn drop(&mut self) {
                remove_all_with_retry(&self.0);
            }
        }
        let _removed = Removed(neighbour_program.clone());
        let own_folder: Vec<u16> = folder.as_os_str().encode_wide().collect();
        let stub_folder: Vec<u16> = neighbour_program
            .parent()
            .expect("its folder")
            .as_os_str()
            .encode_wide()
            .collect();
        assert!(
            equal_ignoring_case(&own_folder, &stub_folder),
            "the premise: the test program and the stub are in one folder"
        );
        let neighbour = Sleeper::new(&neighbour_program, 120_000);
        let processes = make_system_process_ops();

        let scanned = processes.holders_under(&folder);
        assert!(scanned.is_ok(), "{scanned:?}");
        assert!(
            lists_pid(&scanned, neighbour.pid, None),
            "control: the scan sees a program in this folder"
        );
        assert!(
            !lists_pid(&scanned, std::process::id(), None),
            "this process is never its own holder"
        );
        assert_eq!(processes.current_pid(), u64::from(std::process::id()));
    }

    #[test]
    fn a_missing_root_holds_nothing_and_an_empty_root_is_refused() {
        let _serial = serial();
        let scratch = Scratch::new("scan-missing");
        let processes = make_system_process_ops();
        let none = processes.holders_under(&scratch.path().join("does-not-exist"));
        assert_eq!(none.ok(), Some(Vec::new()));
        let refused = processes
            .holders_under(Path::new(""))
            .expect_err("an empty root is refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn process_ids_are_alive_only_while_the_process_runs() {
        let _serial = serial();
        let processes = make_system_process_ops();
        assert!(processes.is_alive(u64::from(std::process::id())));
        assert!(!processes.is_alive(0), "0 means no process");
        assert!(!processes.is_alive(0xFFFF_FFF0), "a number no process has");
        assert!(
            !processes.is_alive(0x1_0000_0000),
            "wider than a process id"
        );
    }

    #[test]
    fn the_engines_running_programs_is_the_same_scan() {
        let _serial = serial();
        let scratch = Scratch::new("scan-engine");
        let root = scratch.path().join("Install");
        let sleeper = start_sleeper_at(&root.join("bin").join("katana_mcp.exe"), 120_000);
        let running = running_programs(&root);
        assert!(running.is_ok(), "{running:?}");
        assert!(
            lists_pid(&running, sleeper.pid, Some(sleeper.executable.as_path())),
            "a program a client holds open, katana_mcp, is what `blocked` must name"
        );
    }
}

// ---- the registry ---------------------------------------------------------------------------------

// The text of the folder the registry reads, as the system spells it.
fn location_text(location: &Result<Option<PathBuf>>) -> Option<OsString> {
    match location {
        Ok(Some(folder)) => Some(folder.as_os_str().to_os_string()),
        _ => None,
    }
}

mod helper_registry {
    use super::*;

    #[test]
    fn the_installers_folder_is_read_from_the_key_the_installer_writes() {
        let _serial = serial();
        let hive = TestHive::new();
        let registry =
            make_registry_ops_under(&hive.base).expect("a registry under the test's key");

        let location = registry.installer_location();
        assert!(
            matches!(location, Ok(None)),
            "no key: not an installer's install: {location:?}"
        );

        let folder = "C:\\Users\\K\u{e4}se\\AppData\\Local\\Programs\\Katana";
        write_value(&hive.location_key(), None, REG_SZ, folder);
        assert_eq!(
            location_text(&registry.installer_location()),
            Some(OsString::from(folder))
        );

        // An expandable string, as an installer may write %LOCALAPPDATA%: expanded by the reader.
        write_value(
            &hive.location_key(),
            None,
            REG_EXPAND_SZ,
            "%SystemRoot%\\Katana",
        );
        let mut system_root = std::env::var_os("SystemRoot").unwrap_or_default();
        assert!(
            !system_root.is_empty(),
            "the premise: Windows sets SystemRoot"
        );
        system_root.push("\\Katana");
        assert_eq!(
            location_text(&registry.installer_location()),
            Some(system_root)
        );

        // An expansion LONGER than the text stored, which is the usual case and the installer's
        // own (%LOCALAPPDATA%\Programs\Katana): the folder is read to its end. For an expandable
        // string Windows was measured to report the size of the STORED text (registry_win.rs), and
        // a reader that believes that size cuts this folder short after 22 of its 43 characters.
        // The variable is this test's own, set for this process, which is where the expansion is
        // done.
        let environment = ScopedEnvironment::new();
        let profile = "C:\\Users\\K\u{e4}se\\AppData\\Local";
        environment.set("KT9", profile);
        write_value(
            &hive.location_key(),
            None,
            REG_EXPAND_SZ,
            "%KT9%\\Programs\\Katana",
        );
        assert_eq!(
            location_text(&registry.installer_location()),
            Some(OsString::from(format!("{profile}\\Programs\\Katana")))
        );

        // And an expansion longer than the reader's first buffer (260 characters) as well.
        let deep = format!("C:\\{}", "d".repeat(300));
        environment.set("KT9", &deep);
        assert_eq!(
            location_text(&registry.installer_location()),
            Some(OsString::from(format!("{deep}\\Programs\\Katana")))
        );

        // A folder longer than the reader's first buffer (260 characters) is read whole.
        let long_folder = format!("C:\\{}\\Katana", "k".repeat(300));
        write_value(&hive.location_key(), None, REG_SZ, &long_folder);
        assert_eq!(
            location_text(&registry.installer_location()),
            Some(OsString::from(long_folder))
        );

        write_value(&hive.location_key(), None, REG_SZ, "");
        assert!(
            matches!(registry.installer_location(), Ok(None)),
            "an empty folder is no folder"
        );

        let number: u32 = 7;
        let key = wide_z(hive.location_key());
        // SAFETY: `key` is NUL-terminated; the data is the four bytes of `number`.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                null(),
                REG_DWORD,
                (&raw const number).cast(),
                4,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        assert!(
            matches!(registry.installer_location(), Ok(None)),
            "a value that is not a string names no folder"
        );
    }

    #[test]
    fn the_run_once_entry_is_written_replaced_and_removed_under_its_own_name_alone() {
        let _serial = serial();
        let hive = TestHive::new();
        let registry =
            make_registry_ops_under(&hive.base).expect("a registry under the test's key");
        assert_eq!(
            RUN_ONCE_NAME, "KatanaUpdateRecover",
            "src/bin/katana_updater_stub.rs looks for this name"
        );
        assert_eq!(RUN_ONCE_NAME, RUN_ONCE_VALUE);
        write_value(
            &hive.run_once_key(),
            Some("SomebodyElse"),
            REG_SZ,
            "\"C:\\Other\\tool.exe\" --once",
        );

        let first =
            "\"C:\\Users\\x\\run-1\\katana_updater.exe\" --recover \"C:\\Users\\x\\Katana\"";
        assert!(registry.set_run_once(RUN_ONCE_NAME, first).is_ok());
        assert_eq!(
            read_string_typed(&hive.run_once_key(), Some(RUN_ONCE_VALUE)),
            Some((String::from(first), REG_SZ))
        );

        let second =
            "\"C:\\Users\\x\\run-2\\katana_updater.exe\" --recover \"C:\\Users\\x\\Katana\"";
        assert!(registry.set_run_once(RUN_ONCE_NAME, second).is_ok());
        assert_eq!(
            read_string(&hive.run_once_key(), Some(RUN_ONCE_VALUE)).as_deref(),
            Some(second),
            "a second transaction replaces the first"
        );

        assert!(registry.clear_run_once(RUN_ONCE_NAME).is_ok());
        assert!(read_string(&hive.run_once_key(), Some(RUN_ONCE_VALUE)).is_none());
        assert_eq!(
            read_string(&hive.run_once_key(), Some("SomebodyElse")).as_deref(),
            Some("\"C:\\Other\\tool.exe\" --once"),
            "another program's entry is not ours to remove"
        );
        assert!(
            registry.clear_run_once(RUN_ONCE_NAME).is_ok(),
            "removing what is not there succeeds"
        );

        // Not even the key need be there.
        let empty = TestHive::new();
        assert!(
            make_registry_ops_under(&empty.base)
                .expect("a registry under the test's key")
                .clear_run_once(RUN_ONCE_NAME)
                .is_ok()
        );
        assert!(!empty.exists(), "removing creates nothing");
    }

    #[test]
    fn a_command_with_letters_outside_the_code_page_round_trips_as_utf16() {
        let _serial = serial();
        let hive = TestHive::new();
        let registry =
            make_registry_ops_under(&hive.base).expect("a registry under the test's key");
        let command = "\"C:\\Users\\K\u{e4}se \u{65e5}\u{672c}\\katana_updater.exe\" --recover \"C:\\Katana\"";
        assert!(registry.set_run_once(RUN_ONCE_NAME, command).is_ok());
        assert_eq!(
            read_string(&hive.run_once_key(), Some(RUN_ONCE_VALUE)).as_deref(),
            Some(command)
        );
    }

    #[test]
    fn a_command_over_the_documented_limit_is_refused_and_writes_nothing() {
        let _serial = serial();
        let hive = TestHive::new();
        let registry =
            make_registry_ops_under(&hive.base).expect("a registry under the test's key");
        let at_the_limit = "a".repeat(260);
        assert!(
            registry.set_run_once(RUN_ONCE_NAME, &at_the_limit).is_ok(),
            "260 characters is allowed"
        );
        let over = registry
            .set_run_once(RUN_ONCE_NAME, &"b".repeat(261))
            .expect_err("261 characters is refused");
        assert_eq!(over.code, ErrorCode::InvalidArgument);
        assert_eq!(
            read_string(&hive.run_once_key(), Some(RUN_ONCE_VALUE)),
            Some(at_the_limit),
            "the old entry stands"
        );
        assert!(registry.set_run_once(RUN_ONCE_NAME, "").is_err());
        assert!(registry.set_run_once("", "x").is_err());
        // The C++ case also refuses a command that is not UTF-8; a &str cannot be one, so the
        // registry has no such command to refuse (docs/rust.md, step 2: text is validated UTF-8).
    }

    #[test]
    fn the_uninstall_entry_is_rewritten_only_where_the_installer_made_it() {
        let _serial = serial();
        let hive = TestHive::new();
        let registry =
            make_registry_ops_under(&hive.base).expect("a registry under the test's key");

        // An archive install has no Uninstall key: nothing is done, and nothing is created.
        assert!(
            registry
                .set_installed_version("0.3.0", "Katana 0.3.0")
                .is_ok()
        );
        assert!(
            !key_exists(&hive.uninstall_key()),
            "the key belongs to the installer; the updater never makes it"
        );

        write_value(
            &hive.uninstall_key(),
            Some("DisplayVersion"),
            REG_SZ,
            "0.2.0",
        );
        write_value(
            &hive.uninstall_key(),
            Some("DisplayName"),
            REG_SZ,
            "Katana 0.2.0",
        );
        write_value(&hive.uninstall_key(), Some("Publisher"), REG_SZ, "Jarada");
        assert!(
            registry
                .set_installed_version("0.3.0", "Katana 0.3.0")
                .is_ok()
        );
        assert_eq!(
            read_string(&hive.uninstall_key(), Some("DisplayVersion")).as_deref(),
            Some("0.3.0")
        );
        assert_eq!(
            read_string(&hive.uninstall_key(), Some("DisplayName")).as_deref(),
            Some("Katana 0.3.0")
        );
        assert_eq!(
            read_string(&hive.uninstall_key(), Some("Publisher")).as_deref(),
            Some("Jarada"),
            "other values stay"
        );
    }

    #[test]
    fn a_base_that_names_no_folder_of_keys_is_refused() {
        let _serial = serial();
        assert!(make_registry_ops_under("").is_none());
        assert!(make_registry_ops_under("\\Software\\x").is_none());
        assert!(make_registry_ops_under("Software\\x\\").is_none());
        // The C++ case also refuses a base that is not UTF-8; a &str cannot be one.
        assert!(make_registry_ops_under("Software\\x").is_some());
    }

    #[test]
    fn the_shipped_registry_uses_the_keys_windows_and_the_installer_use_shown_with_current_user_redirected()
     {
        let _serial = serial();
        // make_system_registry_ops() is what the shipped programs call, and its key paths are
        // relative to HKEY_CURRENT_USER. To see where they lead without writing the machine's
        // RunOnce key, HKEY_CURRENT_USER is sent to a key of this test's own while the calls are
        // made, and the result is read where Windows and the installer would look - below that
        // key. Even so the value has a name of the test's own and a command that does nothing, and
        // the machine's real key is checked to have been left alone.
        let hive = TestHive::new();
        let name = format!("KatanaUpdateTest-{}", &sha256_hex(&hive.base)[..12]);
        let command = "\"C:\\Windows\\System32\\cmd.exe\" /c exit 0";
        let was_there = read_string(MACHINE_RUN_ONCE_KEY, Some(name.as_str())).is_some();
        assert!(
            !was_there,
            "a value of this test's own name is in the machine's RunOnce key already"
        );

        // What an installer leaves, put where the redirected paths will find it.
        write_value(&hive.location_key(), None, REG_SZ, "C:\\Somewhere\\Katana");
        write_value(
            &hive.uninstall_key(),
            Some("DisplayVersion"),
            REG_SZ,
            "0.2.0",
        );
        write_value(
            &hive.uninstall_key(),
            Some("DisplayName"),
            REG_SZ,
            "Katana 0.2.0",
        );

        let mut written: Status = Ok(());
        let mut versioned: Status = Ok(());
        let mut location: Result<Option<PathBuf>> = Ok(None);
        let redirected = {
            let redirect = RedirectedCurrentUser::new(&hive.base);
            if redirect.active {
                let registry = make_system_registry_ops();
                written = registry.set_run_once(&name, command);
                versioned = registry.set_installed_version("0.3.0", "Katana 0.3.0");
                location = registry.installer_location();
            }
            redirect.active
        };
        // HKEY_CURRENT_USER is itself again: look at where the calls landed.
        let landed = read_string(&hive.run_once_key(), Some(name.as_str()));
        let reached_the_machine = read_string(MACHINE_RUN_ONCE_KEY, Some(name.as_str())).is_some();
        let mut cleared = true;
        if redirected {
            let redirect = RedirectedCurrentUser::new(&hive.base);
            if redirect.active {
                cleared = make_system_registry_ops().clear_run_once(&name).is_ok();
            }
        }
        // Should the redirection not have held, nothing of this test stays in the machine's key. It
        // is the one write this file ever makes there, and only to take its own value out again:
        // the key is not even opened for writing when the value did not reach it.
        if reached_the_machine {
            delete_value(MACHINE_RUN_ONCE_KEY, &name);
        }

        assert!(
            redirected,
            "HKEY_CURRENT_USER could not be redirected, so the shipped registry was not run"
        );
        assert!(
            !reached_the_machine,
            "the redirection did not hold: the machine's RunOnce key was written"
        );
        assert!(cleared);
        assert!(written.is_ok(), "{written:?}");
        assert_eq!(
            landed.as_deref(),
            Some(command),
            "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce of the user's hive"
        );
        assert!(
            read_string(&hive.run_once_key(), Some(name.as_str())).is_none(),
            "and removed from there again"
        );

        assert!(versioned.is_ok(), "{versioned:?}");
        assert_eq!(
            read_string(&hive.uninstall_key(), Some("DisplayVersion")).as_deref(),
            Some("0.3.0"),
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Katana, the key the installer \
             makes"
        );
        assert_eq!(
            read_string(&hive.uninstall_key(), Some("DisplayName")).as_deref(),
            Some("Katana 0.3.0")
        );

        assert!(location.is_ok(), "{location:?}");
        assert_eq!(
            location_text(&location),
            Some(OsString::from("C:\\Somewhere\\Katana")),
            "Software\\Jarada\\Katana, the key the installer writes its folder to"
        );
    }

    #[test]
    fn the_keys_a_test_makes_are_gone_when_it_ends() {
        let _serial = serial();
        let base = {
            let hive = TestHive::new();
            let registry =
                make_registry_ops_under(&hive.base).expect("a registry under the test's key");
            assert!(
                registry
                    .set_run_once(
                        RUN_ONCE_NAME,
                        "\"C:\\x\\katana_updater.exe\" --recover C:\\Katana"
                    )
                    .is_ok()
            );
            write_value(
                &hive.uninstall_key(),
                Some("DisplayVersion"),
                REG_SZ,
                "0.2.0",
            );
            assert!(key_exists(&hive.base), "control: the keys were there");
            hive.base.clone()
        };
        assert!(
            !key_exists(&base),
            "a test leaves nothing behind in the registry"
        );
    }
}

// ---- the program: --version, --help, --selftest, usage -----------------------------------------------

fn builds() -> [(&'static str, PathBuf); 2] {
    [
        ("katana_updater", helper()),
        ("katana_updater_test", test_helper()),
    ]
}

// A configuration that makes an OpenSSL that reads it unable to fetch ANY algorithm, Ed25519
// included: it asks for the FIPS property, which no provider here offers. (Measured with the openssl
// command: `openssl dgst -sha256` under it fails with "unsupported". A configuration that merely
// names a provider module that is missing does not break `openssl version` and is no test.)
fn write_hostile_configuration(folder: &Path) -> PathBuf {
    let path = folder.join("hostile.cnf");
    write_text(
        &path,
        "openssl_conf = hostile_init\n[hostile_init]\nalg_section = hostile_algorithms\n\
         [hostile_algorithms]\ndefault_properties = fips=yes\n",
    );
    path
}

const OPENSSL_VARIABLES: [&str; 3] = ["OPENSSL_CONF", "OPENSSL_MODULES", "OPENSSL_ENGINES"];

mod helper_program {
    use super::*;

    #[test]
    fn version_is_the_project_version_and_the_protocol() {
        let _serial = serial();
        for (name, program) in builds() {
            let ran = run_program(&program, strings(&["--version"]));
            assert_eq!(ran.exit_code, 0, "{name}: {}", describe(&ran));
            assert_eq!(
                lines_of(&output_of(&ran)),
                vec![format!(
                    "katana_updater {} applier={APPLIER_PROTOCOL}",
                    expected_version()
                )],
                "{name}"
            );
        }
    }

    #[test]
    fn help_prints_the_usage_and_every_exit_code() {
        let _serial = serial();
        // The table at the top of src/lib.rs, where the codes are decided: each code, and a word of
        // what it means.
        let documented = [
            ("0", "done"),
            ("10", "blocked"),
            ("11", "rolled back"),
            ("12", "rollback incomplete"),
            ("13", "refused"),
            ("20", "command line"),
            ("21", "not trusted"),
            ("30", "internal error"),
        ];
        for (name, program) in builds() {
            let ran = run_program(&program, strings(&["--help"]));
            assert_eq!(ran.exit_code, 0, "{name}: {}", describe(&ran));
            let said = output_of(&ran);
            for form in [
                "usage: katana_updater --root R --stage S --state U --tx T --wait-pid P \
                 --relaunch <path|none>",
                "katana_updater --recover R",
                "--selftest",
                "--version",
                "--help",
            ] {
                assert!(said.contains(form), "{name}: {form}\n{said}");
            }
            let lines = lines_of(&said);
            for (code, meaning) in documented {
                let listed = lines.iter().any(|line| {
                    let words = line.trim_start_matches(' ');
                    !words.is_empty()
                        && words.split(' ').next() == Some(code)
                        && line.contains(meaning)
                });
                assert!(
                    listed,
                    "{name}: exit code {code} ({meaning}) is not in --help\n{said}"
                );
            }
        }
        assert!(
            !output_of(&run_program(&helper(), strings(&["--help"]))).contains("--die-at"),
            "the shipped helper has no test options to tell of"
        );
        assert!(
            output_of(&run_program(&test_helper(), strings(&["--help"]))).contains("test build"),
            "the test helper says what it is"
        );
    }

    #[test]
    fn the_self_test_verifies_a_known_signature_and_reports_what_it_found() {
        let _serial = serial();
        // None of the three variables is set here, whatever the machine that runs the tests has.
        let environment = ScopedEnvironment::new();
        for variable in OPENSSL_VARIABLES {
            environment.unset(variable);
        }
        for (name, program) in builds() {
            let ran = run_program(&program, strings(&["--selftest"]));
            assert_eq!(ran.exit_code, 0, "{name}: {}", describe(&ran));
            let said = output_of(&ran);
            assert_eq!(value_of(&said, "selftest"), "ok", "{name}");
            assert_eq!(value_of(&said, "version"), expected_version(), "{name}");
            assert_eq!(
                value_of(&said, "applier"),
                format!("{APPLIER_PROTOCOL}"),
                "{name}"
            );
            assert_eq!(value_of(&said, "ed25519"), "ok", "{name}");
            assert_eq!(value_of(&said, "openssl_environment"), "cleared", "{name}");
            assert_eq!(value_of(&said, "openssl_removed"), "none", "{name}");
            assert!(!said.contains("problem="), "{name}: {said}");
        }
        // Which keys each build holds: the test build the test keys, the shipped one the pinned
        // keys (placeholders until the owner runs the key steps, docs/update.md) - never the test
        // keys.
        assert_eq!(
            value_of(
                &output_of(&run_program(&test_helper(), strings(&["--selftest"]))),
                "keys"
            ),
            "test"
        );
        let shipped = value_of(
            &output_of(&run_program(&helper(), strings(&["--selftest"]))),
            "keys",
        );
        assert!(shipped == "pinned" || shipped == "placeholder", "{shipped}");
    }

    #[test]
    fn the_self_test_passes_under_a_hostile_open_ssl_conf_modules_or_engines_and_says_what_it_removed()
     {
        let _serial = serial();
        let scratch = Scratch::new("selftest-hostile");
        let configuration = write_hostile_configuration(scratch.path());
        let missing_folder = scratch.path().join("no-such-folder");
        // (what, conf, modules, engines, the names in the order src/lib.rs lists the variables)
        let cases = [
            (
                "all three",
                true,
                true,
                true,
                "OPENSSL_CONF,OPENSSL_MODULES,OPENSSL_ENGINES",
            ),
            ("OPENSSL_CONF alone", true, false, false, "OPENSSL_CONF"),
            (
                "OPENSSL_MODULES alone",
                false,
                true,
                false,
                "OPENSSL_MODULES",
            ),
            (
                "OPENSSL_ENGINES alone",
                false,
                false,
                true,
                "OPENSSL_ENGINES",
            ),
        ];
        for (what, conf, modules, engines, removed) in cases {
            let environment = ScopedEnvironment::new();
            for variable in OPENSSL_VARIABLES {
                environment.unset(variable);
            }
            if conf {
                environment.set("OPENSSL_CONF", &configuration);
            }
            if modules {
                environment.set("OPENSSL_MODULES", &missing_folder);
            }
            if engines {
                environment.set("OPENSSL_ENGINES", &missing_folder);
            }
            for (name, program) in builds() {
                let ran = run_program(&program, strings(&["--selftest"]));
                assert_eq!(ran.exit_code, 0, "{name}, {what}: {}", describe(&ran));
                let said = output_of(&ran);
                assert_eq!(value_of(&said, "selftest"), "ok", "{name}, {what}: {said}");
                assert_eq!(value_of(&said, "ed25519"), "ok", "{name}, {what}");
                assert_eq!(
                    value_of(&said, "openssl_environment"),
                    "cleared",
                    "{name}, {what}"
                );
                // The variables reached the program and it took them away: "cleared" alone would
                // also be the answer of a program that was never handed them.
                assert_eq!(
                    value_of(&said, "openssl_removed"),
                    removed,
                    "{name}, {what}"
                );
            }
        }
    }

    #[test]
    fn the_hostile_configuration_breaks_a_program_that_reads_it() {
        let _serial = serial();
        // The control of the case above: a program that does not harden its environment is broken
        // by this configuration, so the helper's pass is the helper's doing and not a configuration
        // that does nothing. The program is the openssl command, where one of version 3 or later is
        // installed and works; the release tools' tests need one too (tests/update/CMakeLists.txt).
        // Without one the case is skipped on a developer's machine and FAILS on a build service: a
        // control that skips there would leave the case above with nothing to show it means
        // anything.
        let scratch = Scratch::new("selftest-control");
        let configuration = write_hostile_configuration(scratch.path());
        let name = wide_z("openssl.exe");
        let mut found = vec![0u16; MAX_PATH * 4];
        // SAFETY: the name is NUL-terminated, the buffer holds as many units as the call is told,
        // and the two pointers the call may be given null for are null.
        let found_length = unsafe {
            SearchPathW(
                null(),
                name.as_ptr(),
                null(),
                found.len() as u32,
                found.as_mut_ptr(),
                null_mut(),
            )
        } as usize;
        if found_length == 0 || found_length >= found.len() {
            crate::skip_unless_available_or_fail_in_ci!(
                "no openssl.exe on PATH: the hostile configuration is not shown to be hostile here"
            );
        }
        let openssl = from_wide(&found[..found_length]);
        let digest = vec![
            String::from("dgst"),
            String::from("-sha256"),
            utf8(&configuration),
        ];
        {
            let environment = ScopedEnvironment::new();
            for variable in OPENSSL_VARIABLES {
                environment.unset(variable);
            }
            let version = run_program(&openssl, strings(&["version"]));
            let text = output_of(&version);
            let three_or_later = version.exit_code == 0
                && text.starts_with("OpenSSL ")
                && text.len() > 8
                && text.as_bytes()[8].is_ascii_digit()
                && text.as_bytes()[8] >= b'3';
            if !three_or_later || run_program(&openssl, digest.clone()).exit_code != 0 {
                crate::skip_unless_available_or_fail_in_ci!(format!(
                    "{} is not a working OpenSSL 3 or later ({text}): the hostile configuration \
                     is not shown to be hostile here",
                    utf8(&openssl)
                ));
            }
        }
        let environment = ScopedEnvironment::new();
        environment.set("OPENSSL_CONF", &configuration);
        let control = run_program(&openssl, digest);
        assert_ne!(
            control.exit_code,
            0,
            "the hostile configuration should break an OpenSSL that reads it: {}",
            describe(&control)
        );
    }

    #[test]
    fn every_kind_of_bad_command_line_is_a_usage_error_with_exit_code20() {
        let _serial = serial();
        let scratch = Scratch::new("usage");
        let root = utf8(scratch.path());
        let folder = utf8(&scratch.path().join("x"));
        let tx = "a1b2c3d4";
        // A full, valid swap command line, from which one thing is broken at a time.
        let full = || {
            strings(&[
                "--root",
                folder.as_str(),
                "--stage",
                folder.as_str(),
                "--state",
                folder.as_str(),
                "--tx",
                tx,
                "--wait-pid",
                "0",
                "--relaunch",
                "none",
            ])
        };
        let without = |option: &str| {
            let mut arguments = full();
            if let Some(at) = (0..arguments.len())
                .step_by(2)
                .find(|&at| arguments[at] == option)
            {
                arguments.remove(at);
                arguments.remove(at);
            }
            arguments
        };
        let with = |option: &str, value: String| {
            let mut arguments = full();
            for at in (0..arguments.len()).step_by(2) {
                if arguments[at] == option {
                    arguments[at + 1] = value.clone();
                }
            }
            arguments
        };
        let mut repeated = full();
        repeated.extend(strings(&["--tx", tx]));
        let mut unknown_among_the_rest = full();
        unknown_among_the_rest.extend(strings(&["--x", "y"]));
        let cases: Vec<(&str, Vec<String>)> = vec![
            ("no arguments", Vec::new()),
            ("an unknown option", strings(&["--frobnicate"])),
            (
                "an unknown option with a value",
                strings(&["--frobnicate", "x"]),
            ),
            ("a bare word", strings(&["update"])),
            ("--root without its value", strings(&["--root"])),
            (
                "--version with another option",
                strings(&["--version", "--selftest"]),
            ),
            ("--selftest with an argument", strings(&["--selftest", "x"])),
            ("--help with an argument", strings(&["--help", "x"])),
            ("--recover without a folder", strings(&["--recover"])),
            (
                "--recover with two",
                strings(&["--recover", root.as_str(), root.as_str()]),
            ),
            (
                "--recover with an empty folder",
                strings(&["--recover", ""]),
            ),
            (
                "--recover with a folder that does not exist",
                strings(&["--recover", folder.as_str()]),
            ),
            (
                "--recover with a file",
                vec![
                    String::from("--recover"),
                    utf8(&scratch.path().join("file.txt")),
                ],
            ),
            ("no --root", without("--root")),
            ("no --stage", without("--stage")),
            ("no --state", without("--state")),
            ("no --tx", without("--tx")),
            ("no --wait-pid", without("--wait-pid")),
            ("no --relaunch", without("--relaunch")),
            ("a repeated option", repeated),
            ("an unknown option among the rest", unknown_among_the_rest),
            (
                "a transaction id with a separator",
                with("--tx", String::from("..\\evil")),
            ),
            (
                "a transaction id with a parent folder",
                with("--tx", String::from("../x")),
            ),
            (
                "a transaction id in capitals",
                with("--tx", String::from("A1B2C3")),
            ),
            ("an empty transaction id", with("--tx", String::new())),
            (
                "a transaction id of 65 characters",
                with("--tx", "a".repeat(65)),
            ),
            (
                "a process id that is not a number",
                with("--wait-pid", String::from("abc")),
            ),
            (
                "a negative process id",
                with("--wait-pid", String::from("-1")),
            ),
            ("an empty process id", with("--wait-pid", String::new())),
            (
                "a relative install folder",
                with("--root", String::from("Install")),
            ),
            (
                "a relative stage folder",
                with("--stage", String::from("..\\stage")),
            ),
            (
                "a relative state folder",
                with("--state", String::from("state")),
            ),
            (
                "a relative program to start",
                with("--relaunch", String::from("bin\\katana.exe")),
            ),
            ("an empty install folder", with("--root", String::new())),
        ];
        write_text(&scratch.path().join("file.txt"), "x");
        for (name, program) in builds() {
            for (what, arguments) in &cases {
                let ran = run_program(&program, arguments.clone());
                let said = output_of(&ran);
                assert_eq!(ran.exit_code, 20, "{name}: {what}\n{}", describe(&ran));
                assert!(said.contains("katana_updater: "), "{name}: {what}\n{said}");
                assert!(
                    said.contains("usage: katana_updater"),
                    "{name}: {what}\n{said}"
                );
            }
        }
        // The control: the command line the cases were broken from is not itself a usage error.
        // Its folders do not exist, so the program refuses it for what it finds (13), having read
        // it.
        for (name, program) in builds() {
            let ran = run_program(&program, full());
            let said = output_of(&ran);
            assert_eq!(ran.exit_code, 13, "{name}: {}", describe(&ran));
            assert_eq!(
                value_of(&said, "reason"),
                "development_tree",
                "{name}: {said}"
            );
            assert!(!said.contains("usage: katana_updater"), "{name}: {said}");
        }
    }

    #[test]
    fn the_shipped_helper_has_no_test_only_options_and_the_test_helper_refuses_them_malformed() {
        let _serial = serial();
        let scratch = Scratch::new("usage-test-options");
        let folder = utf8(scratch.path());
        let program = utf8(&stub());
        for arguments in [
            strings(&["--die-at", "3", "--recover", folder.as_str()]),
            strings(&[
                "--registry-base",
                "Software\\x",
                "--recover",
                folder.as_str(),
            ]),
            strings(&["--timings", "1,2,3,4,5", "--recover", folder.as_str()]),
            strings(&["--stand-in", program.as_str(), "--recover", folder.as_str()]),
        ] {
            let ran = run_program(&helper(), arguments);
            assert_eq!(ran.exit_code, 20, "{}", describe(&ran));
            assert!(
                output_of(&ran).contains("unknown option"),
                "the shipped helper has no such option\n{}",
                describe(&ran)
            );
        }
        // Behind a mode the program takes no more arguments at all, so the option is refused there
        // too.
        assert_eq!(
            run_program(
                &helper(),
                strings(&["--recover", folder.as_str(), "--die-at", "3"])
            )
            .exit_code,
            20
        );
        for arguments in [
            strings(&["--die-at", "0", "--recover", folder.as_str()]),
            strings(&["--die-at", "x", "--recover", folder.as_str()]),
            strings(&["--die-at", "-3", "--recover", folder.as_str()]),
            strings(&["--die-at", "--recover", folder.as_str()]),
            strings(&["--registry-base", "", "--recover", folder.as_str()]),
            strings(&[
                "--registry-base",
                "\\Software\\x",
                "--recover",
                folder.as_str(),
            ]),
            strings(&["--timings", "1,2,3,4", "--recover", folder.as_str()]),
            strings(&["--timings", "1,2,3,4,5,6", "--recover", folder.as_str()]),
            strings(&["--timings", "1,2,3,x,5", "--recover", folder.as_str()]),
            strings(&["--stand-in", "stub.exe", "--recover", folder.as_str()]),
            strings(&[
                "--stand-in",
                program.as_str(),
                "--stand-in",
                program.as_str(),
                "--recover",
                folder.as_str(),
            ]),
            strings(&[
                "--die-at",
                "1",
                "--die-at",
                "2",
                "--recover",
                folder.as_str(),
            ]),
        ] {
            let ran = run_program(&test_helper(), arguments);
            assert_eq!(ran.exit_code, 20, "{}", describe(&ran));
        }

        // The registry a test helper is told of by the environment is held to the same rule, and
        // the shipped helper does not read the variable at all.
        let environment = ScopedEnvironment::new();
        environment.set("KATANA_UPDATER_TEST_REGISTRY_BASE", "\\Software\\x");
        assert_eq!(
            run_program(&test_helper(), strings(&["--version"])).exit_code,
            20
        );
        assert_eq!(run_program(&helper(), strings(&["--version"])).exit_code, 0);
    }

    #[test]
    fn the_copy_runs_from_a_folder_of_its_own_with_nothing_of_the_toolchain_on_path() {
        let _serial = serial();
        // S1 of the design: the helper never runs from bin/, so it must need nothing beside it.
        let scratch = Scratch::new("selftest-alone");
        let copy = scratch
            .path()
            .join(UNICODE_FOLDER)
            .join("run-1")
            .join("katana_updater.exe");
        copy_program(&helper(), &copy);
        // Read before it is run. A helper that needed a DLL of the toolchain would not fail quietly
        // here: started with nothing of the toolchain on PATH it would put up the loader's own
        // message box ("... was not found") and sit behind it until somebody clicked - the
        // Launcher gives a program it starts the default error mode. So the regression this case
        // is for is caught from the file, and the program is only started once it can load.
        let imported = imported_libraries(&copy);
        assert!(
            !imported.is_empty(),
            "no import table was read from the copy: it is not started"
        );
        for name in &imported {
            assert!(
                is_operating_system_library(name),
                "katana_updater imports {name}, which is not the operating system's: it is not \
                 started"
            );
        }
        let mut windows = vec![0u16; MAX_PATH];
        // SAFETY: the buffer holds as many units as the call is told.
        let length =
            unsafe { GetWindowsDirectoryW(windows.as_mut_ptr(), windows.len() as u32) } as usize;
        windows.truncate(length.min(MAX_PATH));
        let windows = OsString::from_wide(&windows);
        let mut system_folders = windows.clone();
        system_folders.push("\\System32;");
        system_folders.push(&windows);
        let environment = ScopedEnvironment::new();
        environment.set("PATH", &system_folders);
        // A self-test takes a moment. The limit is the one every run here has (PATIENCE): what it
        // has to cover is a virus scanner's first look at a program just written, on a machine that
        // may be running a dozen other tests.
        let folder = copy.parent().expect("the copy's folder").to_path_buf();
        let ran = run_program_in(&copy, strings(&["--selftest"]), &folder, PATIENCE);
        assert!(!ran.timed_out, "{}", describe(&ran));
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        assert_eq!(value_of(&output_of(&ran), "selftest"), "ok");
    }
}

// ---- the import table -----------------------------------------------------------------------------

// The DLLs a PE file imports, read from the file's import directory (not from objdump, which the
// CTest entries of tests/update/CMakeLists.txt use: this is a second reading, in the test program).
// The offsets are those of the PE format's headers (winnt.h): IMAGE_DOS_HEADER's e_lfanew at 0x3C;
// IMAGE_NT_HEADERS64 of 264 bytes, the signature, the 20 bytes of IMAGE_FILE_HEADER (the number of
// sections at 2, the size of the optional header at 16) and IMAGE_OPTIONAL_HEADER64, whose data
// directories begin at 112, the import table's the second of them; IMAGE_SECTION_HEADER of 40
// bytes (the virtual size at 8, the virtual address at 12, the raw size at 16, the raw data at 20);
// IMAGE_IMPORT_DESCRIPTOR of 20 bytes, the name's address at 12.
fn imported_libraries(executable: &Path) -> Vec<String> {
    let bytes = read_text(executable);
    let mut names = Vec::new();
    let u16_at = |at: usize| {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    // "MZ"
    if bytes.len() < 64 || u16_at(0) != Some(0x5A4D) {
        return names;
    }
    let Some(lfanew) = u32_at(0x3C).map(|value| i64::from(value as i32)) else {
        return names;
    };
    if lfanew < 0 || lfanew as usize + 264 > bytes.len() {
        return names;
    }
    let nt = lfanew as usize;
    // "PE\0\0", and the optional header of PE32+ (x86-64 and ARM64 both).
    if u32_at(nt) != Some(0x0000_4550) || u16_at(nt + 24) != Some(0x020B) {
        return names;
    }
    let sections = usize::from(u16_at(nt + 6).unwrap_or(0));
    let optional_size = usize::from(u16_at(nt + 20).unwrap_or(0));
    let imports = u32_at(nt + 24 + 120).unwrap_or(0);
    let first_section = nt + 24 + optional_size;
    let to_offset = |rva: u32| -> Option<usize> {
        for i in 0..sections {
            let section = first_section + i * 40;
            let virtual_size = u32_at(section + 8)?;
            let virtual_address = u32_at(section + 12)?;
            let raw_size = u32_at(section + 16)?;
            let raw_data = u32_at(section + 20)?;
            let size = virtual_size.max(raw_size);
            if rva >= virtual_address && rva < virtual_address.wrapping_add(size) {
                return Some(rva.wrapping_sub(virtual_address).wrapping_add(raw_data) as usize);
            }
        }
        None
    };
    if imports == 0 {
        return names;
    }
    let mut at = to_offset(imports).unwrap_or(bytes.len());
    while at + 20 <= bytes.len() {
        let name = u32_at(at + 12).unwrap_or(0);
        if name == 0 {
            break;
        }
        if let Some(name_at) = to_offset(name)
            && name_at < bytes.len()
        {
            let end = bytes[name_at..]
                .iter()
                .position(|&c| c == 0)
                .map_or(bytes.len(), |length| name_at + length);
            names.push(String::from_utf8_lossy(&bytes[name_at..end]).into_owned());
        }
        at += 20;
    }
    names
}

// The DLLs of Windows itself: the names of tests/update/check_updater_imports.cmake, one for one.
// Which DLLs are Windows' is decided there; what this file adds is a second READING of the import
// table, by other code than objdump's.
fn is_operating_system_library(name: &str) -> bool {
    const SYSTEM: [&str; 24] = [
        "kernel32",
        "kernelbase",
        "advapi32",
        "user32",
        "shell32",
        "ole32",
        "oleaut32",
        "ws2_32",
        "crypt32",
        "bcrypt",
        "ncrypt",
        "ntdll",
        "rpcrt4",
        "secur32",
        "userenv",
        "version",
        "shlwapi",
        "iphlpapi",
        "wintrust",
        "psapi",
        "cfgmgr32",
        "sechost",
        "msvcrt",
        "ucrtbase",
    ];
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".dll").filter(|stem| !stem.is_empty()) else {
        return false;
    };
    SYSTEM.contains(&stem) || stem.starts_with("api-ms-win-")
}

mod helper_imports {
    use super::*;

    #[test]
    fn every_program_the_helper_runs_imports_only_dlls_of_the_operating_system() {
        let _serial = serial();
        // The shipped helper, the same sources with the test key, and the stub the end-to-end
        // cases start from an install: none may need a DLL of the toolchain, because the first two
        // run from a folder of their own (S1) and the third from an install whose bin/ is being
        // replaced.
        for program in [helper(), test_helper(), stub()] {
            let imported = imported_libraries(&program);
            let file = program
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            assert!(
                !imported.is_empty(),
                "{}: no import table was read",
                utf8(&program)
            );
            let listing = imported.join(", ");
            for name in &imported {
                assert!(
                    is_operating_system_library(name),
                    "{file} imports {name}, which is not the operating system's"
                );
            }
            // Every program for Windows imports its kernel: a reading that did not find it read
            // nothing.
            assert!(
                imported
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case("kernel32.dll")),
                "{file}: {listing}"
            );
            println!("[  IMPORTS ] {file}: {listing}");
        }
    }

    #[test]
    fn the_check_sees_a_dynamically_linked_program_for_what_it_is() {
        let _serial = serial();
        // The control: a program that is NOT static must be reported as importing a DLL of the
        // toolchain, or the case above would pass on a reader that cannot see it. The test program
        // itself is the C++'s control, which links libstdc++ as a DLL.
        let own = std::env::current_exe().expect("this test program's path");
        let imported = imported_libraries(&own);
        assert!(!imported.is_empty());
        let foreign = imported
            .iter()
            .any(|name| !is_operating_system_library(name));
        assert!(
            foreign,
            "the test program imports only system DLLs, so it is no control for the reader: {}",
            imported.join(", ")
        );
        assert!(!is_operating_system_library("libstdc++-6.dll"));
        assert!(!is_operating_system_library("libgcc_s_seh-1.dll"));
        assert!(!is_operating_system_library("libwinpthread-1.dll"));
        assert!(!is_operating_system_library("libcrypto-3-x64.dll"));
        assert!(!is_operating_system_library("zlib1.dll"));
        assert!(
            !is_operating_system_library("kernel32"),
            "a name, not a prefix"
        );
        assert!(!is_operating_system_library("kernel32.dll.evil.dll"));
        assert!(is_operating_system_library("KERNEL32.dll"));
        assert!(is_operating_system_library(
            "api-ms-win-crt-stdio-l1-1-0.dll"
        ));
    }
}

// ---- the swap, through the real program -----------------------------------------------------------

// One run of the real program in a fresh world, killed before its k-th change to the disk
// (--die-at, the test build's FaultFs; a crash is a process that stops).
struct CrashProbe {
    world: Box<World>,
    ran: SpawnResult,
    // It ended by the simulated crash, not by finishing.
    crashed: bool,
    // COMMITTING is in the journal and the tree is neither version's.
    in_the_commit: bool,
}

// `timings` is the test helper's --timings: TIMINGS, or - for a run that must get THROUGH its
// health check before it is killed - one whose health wait no run sits out.
fn probe_crash(label: &str, k: u64, timings: &str) -> CrashProbe {
    let world = Box::new(World::new(&format!("{label}-{k}")));
    world.arrange_environment("healthy");
    let mut arguments = world.apply_arguments(TRANSACTION, 0, timings);
    arguments.push(String::from("--die-at"));
    arguments.push(format!("{k}"));
    let ran = world.run_helper(arguments, TRANSACTION);
    let crashed = ran.exit_code == 99;
    let journal = text_of_file(&world.root.join(".update").join(TRANSACTION).join("journal"));
    let tree = tree_hash(&world.root);
    let in_the_commit = crashed
        && journal.contains("COMMITTING")
        && !journal.contains("COMMITTED_PENDING_HEALTH")
        && tree != world.v1_hash
        && tree != world.v2_hash;
    CrashProbe {
        world,
        ran,
        crashed,
        in_the_commit,
    }
}

// Finds a crash in the MIDDLE of the commit, with `wanted` agreeing, and returns the world it
// happened in with the crash left on disk, what the run said, and its k. The number of the first
// change after the journal says COMMITTING is found by bisection, and the changes after it are
// tried in turn: each is a rename or a journal line.
fn crashed_mid_commit(
    label: &str,
    wanted: &dyn Fn(&World) -> bool,
) -> (Box<World>, SpawnResult, u64) {
    // Whether the run killed before change k had got as far as COMMITTING. A bisection needs an
    // answer that, once yes, stays yes as k grows - so "COMMITTING is in the journal" is not enough
    // by itself:
    //   - not killed at all: k is past the last change, and so past the commit;
    //   - COMMITTING is in the journal;
    //   - there is no journal to read. Before the plan there is none yet. After a healthy update
    //     there is none ANY MORE: the clean-up deletes it with the transaction's folder while
    //     changes are still being counted (the staged files, the records in U), and a run killed
    //     there would read as "before the commit". The install's own record tells the two apart:
    //     installed.json is put in place by the commit, among its last files (apply.rs, step 5),
    //     and by nothing before it, so the new version's record means the commit has begun.
    let reached_the_commit = |k: u64| {
        let probe = probe_crash(label, k, TIMINGS);
        if !probe.crashed {
            return true;
        }
        let killed = &probe.world;
        if text_of_file(
            &killed
                .root
                .join(".update")
                .join(TRANSACTION)
                .join("journal"),
        )
        .contains("COMMITTING")
        {
            return true;
        }
        read_text(
            &killed
                .root
                .join("share")
                .join("katana")
                .join("installed.json"),
        ) == installed_json(&killed.v2)
    };
    // The upper end is found, not assumed: doubled until a run gets as far as the commit, so that a
    // release of more files than today's does not put the answer out of reach.
    let mut low: u64 = 1; // before the first change: nothing on disk
    let mut high: u64 = 32;
    while !reached_the_commit(high) {
        low = high;
        high *= 2;
        assert!(
            high <= 4096,
            "no run got as far as the commit within 4096 changes to the disk"
        );
    }
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if reached_the_commit(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    for k in high..high + 40 {
        let probe = probe_crash(label, k, TIMINGS);
        if !probe.crashed {
            break;
        }
        if probe.in_the_commit && wanted(probe.world.as_ref()) {
            return (probe.world, probe.ran, k);
        }
    }
    panic!(
        "no crash point fell in the middle of the commit (the journal reaches COMMITTING at change \
         {high})"
    );
}

// A tree that is truly half of each: the commit moves data first and programs last (apply.rs, step
// 5), so there is a stretch in which the readme is the new version's and katana.exe still the old
// one's.
fn is_half_swapped(world: &World) -> bool {
    let content = |release: &ReleaseSpec, path: &str| {
        release
            .find(path)
            .map(|file| file.content.clone())
            .unwrap_or_default()
    };
    read_text(&world.root.join("share").join("katana").join("readme.txt"))
        == content(&world.v2, "share/katana/readme.txt")
        && read_text(&world.root.join("bin").join("katana.exe"))
            == content(&world.v1, "bin/katana.exe")
}

// The sequences state.json holds as blocked: the releases whose health check failed on this
// machine, which are not offered again as they were (state.rs). None when there is no file.
fn blocked_sequences(state_directory: &Path) -> Vec<u64> {
    match read_state(state_directory) {
        Ok(recorded) => recorded.blocked_sequences,
        Err(error) => panic!("state.json: {}", error.describe()),
    }
}

// Opened for reading, shared for reading and writing and NOT for deletion: it cannot be moved
// while this is open (the C++'s CreateFileW with FILE_SHARE_READ | FILE_SHARE_WRITE).
fn hold(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)
}

mod helper_swap {
    use super::*;

    #[test]
    fn a_signed_update_is_applied_by_the_program_and_the_tree_is_the_new_versions_with_a_hostile_environment()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let hive_key = {
            let world = World::new("swap-commit");
            let hostile = write_hostile_configuration(&world.top);
            world.arrange_environment("healthy");
            world.register_as_installer();
            world.environment.set("OPENSSL_CONF", &hostile);
            world
                .environment
                .set("OPENSSL_MODULES", world.top.join("no-modules"));
            world
                .environment
                .set("OPENSSL_ENGINES", world.top.join("no-engines"));

            let ran = world.run_helper(world.apply_arguments(TRANSACTION, 0, TIMINGS), TRANSACTION);
            assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
            assert_eq!(value_of(&output_of(&ran), "outcome"), "committed");
            world.expect_v2();
            assert_eq!(
                world.display_version().as_deref(),
                Some("0.10.0"),
                "Settings > Apps names the installed version"
            );
            assert_eq!(world.display_name().as_deref(), Some("Katana 0.10.0"));
            assert!(
                leftovers_in_update(&world.root).is_empty(),
                "a healthy update leaves nothing in R/.update"
            );

            let last = world.last();
            assert_eq!(last.status, "committed");
            assert_eq!(last.version, "0.10.0");
            assert_eq!(last.transaction, TRANSACTION);
            assert!(
                !is_present(&health_marker_path(&world.state, TRANSACTION)),
                "the marker is cleaned up"
            );
            assert!(
                !world.run_once_is_set(),
                "the RunOnce entry is removed when the transaction ends"
            );

            // The new version was started for its health check, from the install, with the
            // person's OPENSSL_CONF handed on (the helper itself ran without it), and with the
            // helper's RunOnce entry in place at that moment: it names the helper copy and the
            // install.
            let log = world.stub_log();
            let lines = lines_of(&log);
            assert!(!lines.is_empty(), "{log}");
            let health = &lines[0];
            let program = utf8(&world.root.join("bin").join("katana.exe"));
            assert!(health.starts_with("health tag=v2 "), "{health}");
            assert!(health.contains(&format!("exe={program} ")), "{health}");
            assert!(
                health.contains(&format!("OPENSSL_CONF={}", utf8(&hostile))),
                "{health}"
            );
            let expected_run_once = run_once_command(&world.helper_path(TRANSACTION), &world.root);
            assert!(
                expected_run_once.encode_utf16().count() <= 260,
                "the premise: a command Windows would run"
            );
            assert!(
                health.contains(&format!("runonce={expected_run_once} ")),
                "{health}\nexpected the RunOnce command\n{expected_run_once}"
            );
            assert!(health.contains(&format!("tx={TRANSACTION}")));
            // And the new version is started at the end, the way the window's own relaunch starts
            // it.
            assert!(world.wait_for_log("start tag=v2 "), "{}", world.stub_log());
            let after = lines_of(&world.stub_log());
            assert_eq!(after.len(), 2, "{}", world.stub_log());
            assert!(
                after[1].contains(&format!("OPENSSL_CONF={}", utf8(&hostile))),
                "{}",
                after[1]
            );
            assert!(
                after[1].contains(&format!("exe={program} ")),
                "{}",
                after[1]
            );
            assert!(
                after[1].contains("runonce=-"),
                "the entry was gone before Katana was started again: {}",
                after[1]
            );
            assert!(
                !after[1].contains(" tx="),
                "the relaunch is a plain start, not another health check"
            );
            world.hive.base.clone()
        };
        assert!(
            !key_exists(&hive_key),
            "the test's registry keys went with the test: nothing is left behind"
        );
    }

    #[test]
    fn a_test_helper_told_of_no_registry_writes_no_run_once_entry_on_the_machine() {
        let _serial = serial();
        crate::require_can_sign!();
        // The machine's own key is only READ, by the stub, at the moment a shipped helper would
        // have its entry there: during the health check. The engine's own command line is used as
        // it is, with no test option, and the environment names no registry either. A machine on
        // which a real Katana was interrupted has such an entry already, and the case cannot judge
        // it; a build service has no Katana installed, so there the entry would be a test's own
        // leak, and that is a failure.
        if read_string(MACHINE_RUN_ONCE_KEY, Some(RUN_ONCE_VALUE)).is_some() {
            crate::skip_unless_available_or_fail_in_ci!(
                "this machine has a Katana recovery entry in its RunOnce key already; it is not \
                 this test's to judge"
            );
        }
        let world = World::new("swap-no-registry");
        world.arrange_environment("healthy");
        world.environment.unset("KATANA_UPDATER_TEST_REGISTRY_BASE");
        world.environment.set("KATANA_STUB_MACHINE_RUNONCE", "1");

        let ran = world.run_helper(world.engine_arguments(TRANSACTION, 0), TRANSACTION);
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        world.expect_v2();
        let lines = lines_of(&world.stub_log());
        assert!(!lines.is_empty(), "{}", world.stub_log());
        assert!(lines[0].starts_with("health tag=v2 "), "{}", lines[0]);
        assert!(
            lines[0].contains(" machine_runonce=- "),
            "the test helper wrote the machine's RunOnce key: {}",
            lines[0]
        );
        assert!(
            lines[0].contains(" runonce=- "),
            "and it had no registry of the test's to write: {}",
            lines[0]
        );
        assert!(
            !world.hive.exists(),
            "nothing was written under the test's key either"
        );
        assert!(world.wait_for_log("start tag=v2 "), "{}", world.stub_log());
    }

    #[test]
    fn the_shipped_helper_will_not_apply_a_release_signed_with_the_test_key_and_touches_nothing() {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-shipped-trust");
        world.arrange_environment("healthy");
        copy_program(&helper(), &world.helper_path("b1b2"));
        // The real program with the engine's own command line, and a stage signed with a key
        // anybody holds: the pinned keys do not include it, so the answer is "not trusted". It
        // reads the machine's registry for the installer's folder and, refusing before the plan,
        // writes none.
        let ran = world.run_helper(world.engine_arguments("b1b2", 0), "b1b2");
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 21, "{}", describe(&ran));
        assert_eq!(value_of(&said, "outcome"), "refused");
        assert_eq!(value_of(&said, "reason"), "staged_manifest");
        world.expect_v1();
        assert!(leftovers_in_update(&world.root).is_empty());
        assert!(!world.run_once_is_set());
        assert_eq!(world.last().status, "refused");
        // The window had closed to let the update in: it is started again, as it was.
        assert!(world.wait_for_log("start tag=v1 "), "{}", world.stub_log());
    }

    #[test]
    fn a_development_tree_is_never_touched_and_the_helper_exits_with_code13() {
        let _serial = serial();
        crate::require_can_sign!();
        // A tree with no share/katana/install.json is a build tree or a plain install, never an
        // install an update may change (install.rs) - whatever is staged for it, however well
        // signed.
        let world = World::new("swap-development-tree");
        world.arrange_environment("healthy");
        std::fs::remove_file(world.root.join("share").join("katana").join("install.json"))
            .expect("install.json goes");
        let before = snapshot(&world.root);

        let ran = world.run_helper(world.apply_arguments(TRANSACTION, 0, TIMINGS), TRANSACTION);
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 13, "{}", describe(&ran));
        assert_eq!(value_of(&said, "outcome"), "refused");
        assert_eq!(value_of(&said, "reason"), "development_tree");
        assert_eq!(
            difference(&before, &snapshot(&world.root)),
            "",
            "the tree was changed"
        );
        assert!(
            !is_present(&world.root.join(".update")),
            "not even the work folder is made in a development tree"
        );
        assert!(!world.run_once_is_set());
        assert_eq!(world.last().reason, "development_tree");
        assert!(world.wait_for_log("start tag=v1 "), "{}", world.stub_log());
    }

    #[test]
    fn the_fixtures_release_signed_by_openssl_is_applied_end_to_end_with_a_real_program_standing_in_for_its_katana()
     {
        let _serial = serial();
        // The T3 fixtures: trees and a manifest made by the Python producer, signed by the openssl
        // command with the published test key - nothing of this program made them. The tree the
        // update must leave is the fixtures' own v2 install with v1's seed and v2's records.
        let world = FixtureWorld::new("swap-fixtures", "v1", "v2");
        let environment = ScopedEnvironment::new();
        environment.set("KATANA_STUB_LOG", &world.log);
        environment.set("KATANA_STUB_MODE", "healthy");
        environment.set("KATANA_UPDATE_STATE_DIR", &world.s.state);
        environment.set("KATANA_STUB_REGISTRY_BASE", &world.hive.base);

        let mut arguments = world.apply_arguments(TIMINGS);
        arguments.push(String::from("--stand-in"));
        arguments.push(utf8(&stub()));
        let ran = world.run(arguments);
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        assert_eq!(value_of(&output_of(&ran), "outcome"), "committed");
        assert_eq!(
            tree_hash(&world.s.root),
            world.s.v2_hash,
            "the tree must be the fixtures' v2\n{}",
            difference(&snapshot(&world.s.expected), &snapshot(&world.s.root))
        );
        assert!(leftovers_in_update(&world.s.root).is_empty());
        assert!(!world.run_once_is_set());

        let last = match read_last_result(&world.s.state) {
            Ok(Some(last)) => last,
            other => panic!("last.json: {other:?}"),
        };
        assert_eq!(last.status, "committed");
        assert_eq!(last.version, "0.10.0");
        assert_eq!(last.transaction, FIXTURE_TRANSACTION);

        // One start, of the stand-in, as the health check of this transaction, from the install's
        // bin folder, with the RunOnce entry standing. Nothing is started at the end (--relaunch
        // none).
        let log = log_of(&world.log);
        let lines = lines_of(&log);
        assert_eq!(lines.len(), 1, "{log}");
        assert!(
            lines[0].starts_with("health tag=none "),
            "the stand-in is the build's own, untagged stub: {}",
            lines[0]
        );
        assert!(
            lines[0].contains(&format!("cwd={} ", utf8(&world.s.root.join("bin")))),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].contains(&format!(
                "runonce={} ",
                run_once_command(&world.helper(), &world.s.root)
            )),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].contains(&format!("tx={FIXTURE_TRANSACTION} ")),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn a_new_version_that_cannot_be_started_at_all_is_rolled_back_at_once_and_not_offered_again() {
        let _serial = serial();
        // The new katana is not there to start - a virus scanner took it between the commit and
        // the health check, say. (The stand-in is a path with no file: starting it is an error of
        // the operating system's, at once and with no message box. A file that is NOT a program is
        // never started by a test.)
        let world = FixtureWorld::new("swap-unstartable", "v1", "v2");
        let missing = world.top.join("not here").join("katana.exe");
        // "At once" is: not after the health wait. The wait of this run is one that a run which
        // sat it out could not have ended within the test's limit for a run (LONG_HEALTH_WAIT).
        let mut arguments =
            world.apply_arguments(&timings_with_a_long_health_wait(Duration::from_secs(30)));
        arguments.push(String::from("--stand-in"));
        arguments.push(utf8(&missing));
        let ran = world.run(arguments);
        let said = output_of(&ran);
        assert!(
            !ran.timed_out,
            "the health wait was sat out for a program that was never started"
        );
        assert_eq!(ran.exit_code, 11, "{}", describe(&ran));
        assert_eq!(value_of(&said, "outcome"), "rolled_back");
        assert_eq!(value_of(&said, "reason"), "health_exit");
        assert_eq!(
            tree_hash(&world.s.root),
            world.s.v1_hash,
            "{}",
            difference(&world.s.v1_snapshot, &snapshot(&world.s.root))
        );
        assert!(leftovers_in_update(&world.s.root).is_empty());
        assert!(!world.run_once_is_set());
        let last = match read_last_result(&world.s.state) {
            Ok(Some(last)) => last,
            other => panic!("last.json: {other:?}"),
        };
        assert_eq!(last.status, "rolled_back");
        assert_eq!(last.reason, "health_exit");
        assert_eq!(
            blocked_sequences(&world.s.state),
            vec![FIXTURE_V2_SEQUENCE],
            "the sequence of the fixtures' v2 manifest"
        );
    }

    #[test]
    fn every_bad_release_of_the_fixtures_is_refused_with_exit_code21_and_the_tree_is_untouched() {
        let _serial = serial();
        let bad = [
            ("flipped-manifest-bit", "v1", "staged_manifest"),
            ("swapped-sig", "v1", "staged_manifest"),
            ("wrong-key", "v1", "staged_manifest"),
            ("wrong-key-claims-active", "v1", "staged_manifest"),
            ("path-traversal", "v1", "staged_manifest"),
            ("downgrade-version", "v2", "downgrade"),
            ("downgrade-sequence", "v2", "downgrade"),
        ];
        for (release, installed, reason) in bad {
            let world = FixtureWorld::new(
                &format!("helper-bad-{release}"),
                installed,
                &format!("bad/{release}"),
            );
            let ran = world.run(world.apply_arguments(TIMINGS));
            let said = output_of(&ran);
            assert_eq!(ran.exit_code, 21, "{release}: {}", describe(&ran));
            assert_eq!(value_of(&said, "outcome"), "refused", "{release}");
            assert_eq!(value_of(&said, "reason"), reason, "{release}");
            assert_eq!(
                tree_hash(&world.s.root),
                world.s.v1_hash,
                "{release}{}",
                difference(&world.s.v1_snapshot, &snapshot(&world.s.root))
            );
            assert!(leftovers_in_update(&world.s.root).is_empty(), "{release}");
            assert!(!world.run_once_is_set(), "{release}");
            let last = match read_last_result(&world.s.state) {
                Ok(Some(last)) => last,
                other => panic!("{release}: last.json: {other:?}"),
            };
            assert_eq!(last.status, "refused", "{release}");
            assert_eq!(last.reason, reason, "{release}");
        }
    }

    #[test]
    fn a_new_version_that_exits_with_an_error_is_rolled_back_with_exit_code11_and_the_old_one_starts_again()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-health-exit");
        world.arrange_environment("exit1");
        world.register_as_installer();
        let ran = world.run_helper(world.apply_arguments(TRANSACTION, 0, TIMINGS), TRANSACTION);
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 11, "{}", describe(&ran));
        assert_eq!(value_of(&said, "outcome"), "rolled_back");
        assert_eq!(value_of(&said, "reason"), "health_exit");
        world.expect_v1();
        assert_eq!(
            world.display_version().as_deref(),
            Some("0.9.0"),
            "a release that did not start is never shown as installed"
        );
        assert_eq!(world.display_name().as_deref(), Some("Katana 0.9.0"));
        assert!(leftovers_in_update(&world.root).is_empty());
        assert!(!world.run_once_is_set());
        assert_eq!(world.last().status, "rolled_back");
        assert_eq!(world.last().reason, "health_exit");

        assert_eq!(
            blocked_sequences(&world.state),
            vec![2000],
            "a release that does not start is not offered again"
        );

        // The new version tried to start, and then the OLD one was started: the person gets Katana
        // back.
        assert!(world.wait_for_log("start tag=v1 "), "{}", world.stub_log());
        assert!(lines_of(&world.stub_log())[0].starts_with("health tag=v2 "));
    }

    #[test]
    fn a_new_version_that_says_it_started_and_then_exits_with_an_error_is_rolled_back_too() {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-health-marker-then-exit");
        world.arrange_environment("marker_then_exit3");
        let ran = world.run_helper(world.apply_arguments(TRANSACTION, 0, TIMINGS), TRANSACTION);
        assert_eq!(ran.exit_code, 11, "{}", describe(&ran));
        assert_eq!(value_of(&output_of(&ran), "reason"), "health_exit");
        world.expect_v1();
        assert!(
            world.log_has("health tag=v2 "),
            "the new version was started"
        );
        assert!(
            !is_present(&health_marker_path(&world.state, TRANSACTION)),
            "the marker of a start that then failed is no evidence for the next one"
        );
        assert_eq!(world.last().status, "rolled_back");
        assert_eq!(world.last().reason, "health_exit");
        assert_eq!(
            blocked_sequences(&world.state),
            vec![2000],
            "v2's sequence: a release that starts and then fails is not offered again"
        );
        assert!(world.wait_for_log("start tag=v1 "), "{}", world.stub_log());
    }

    #[test]
    fn a_new_version_that_ends_well_without_saying_it_started_is_rolled_back_and_not_offered_again()
    {
        let _serial = serial();
        crate::require_can_sign!();
        // Exit code 0 and no marker: a program that was started and ended without a complaint, and
        // never got to its first idle turn - a release whose katana does not know the switch and
        // leaves at once, say. "A start that ... never writes it means rollback" (apply.rs, step
        // 7): a clean exit alone proves nothing. It ended, so the helper has no wait to sit out:
        // the reason is the exit, not the time, and this run's health wait is not spent - a wait so
        // long (LONG_HEALTH_WAIT) that a helper which spent it would still be running when the
        // test's limit for a run ended it.
        let world = World::new("swap-health-exit0");
        world.arrange_environment("exit0");
        world.register_as_installer();
        let ran = world.run_helper(
            world.apply_arguments(
                TRANSACTION,
                0,
                &timings_with_a_long_health_wait(Duration::from_secs(30)),
            ),
            TRANSACTION,
        );
        let said = output_of(&ran);
        assert!(
            !ran.timed_out,
            "the health wait was sat out for a program that had ended"
        );
        assert_eq!(ran.exit_code, 11, "{}", describe(&ran));
        assert_eq!(value_of(&said, "outcome"), "rolled_back");
        assert_eq!(value_of(&said, "reason"), "health_exit");
        world.expect_v1();
        assert_eq!(
            world.display_version().as_deref(),
            Some("0.9.0"),
            "a release that did not prove itself is never shown"
        );
        assert!(leftovers_in_update(&world.root).is_empty());
        assert!(!world.run_once_is_set());
        assert!(
            !is_present(&health_marker_path(&world.state, TRANSACTION)),
            "the premise: no marker"
        );
        assert_eq!(world.last().status, "rolled_back");
        assert_eq!(world.last().reason, "health_exit");
        assert_eq!(blocked_sequences(&world.state), vec![2000], "v2's sequence");
        assert!(world.wait_for_log("start tag=v1 "), "{}", world.stub_log());
        let lines = lines_of(&world.stub_log());
        assert!(!lines.is_empty());
        assert!(lines[0].starts_with("health tag=v2 "), "{}", lines[0]);
        assert!(
            lines[0].contains(" mode=exit0"),
            "the premise: the stub was in the mode asked for"
        );
    }

    #[test]
    fn a_new_version_that_never_says_it_started_is_rolled_back_when_the_health_wait_ends() {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-health-timeout");
        world.arrange_environment("silent");
        // The wait is 60 s in a shipped helper; the test shortens it (the fourth number).
        let ran = world.run_helper(
            world.apply_arguments(TRANSACTION, 0, "500,1500,50,3000,30000"),
            TRANSACTION,
        );
        assert_eq!(ran.exit_code, 11, "{}", describe(&ran));
        assert_eq!(value_of(&output_of(&ran), "reason"), "health_timeout");
        world.expect_v1();
        assert!(
            world.log_has("health tag=v2 "),
            "the new version was started"
        );
        assert_eq!(world.last().status, "rolled_back");
        assert_eq!(world.last().reason, "health_timeout");
        assert_eq!(
            blocked_sequences(&world.state),
            vec![2000],
            "v2's sequence: a release that never says it started is not offered again"
        );
        assert!(world.wait_for_log("start tag=v1 "), "{}", world.stub_log());
    }

    #[test]
    fn a_program_running_from_the_install_blocks_the_update_and_nothing_is_touched_until_retry() {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-blocked");
        world.arrange_environment("healthy");
        // The install's own katana_cli.exe (the stub), running: the real scan must see it.
        let holder = Sleeper::new(&world.root.join("bin").join("katana_cli.exe"), 120_000);
        assert_ne!(holder.pid, 0);

        let blocked = world.run_helper(world.apply_arguments(TRANSACTION, 0, TIMINGS), TRANSACTION);
        let said = output_of(&blocked);
        assert_eq!(blocked.exit_code, 10, "{}", describe(&blocked));
        assert_eq!(value_of(&said, "outcome"), "blocked");
        assert_eq!(
            value_of(&said, "holder"),
            format!("katana_cli.exe ({})", holder.pid)
        );
        world.expect_v1();
        assert!(
            leftovers_in_update(&world.root).is_empty(),
            "a blocked update changed nothing"
        );
        assert!(!world.run_once_is_set());
        let last = world.last();
        assert_eq!(last.status, "blocked");
        assert_eq!(last.holder, format!("katana_cli.exe ({})", holder.pid));
        assert!(
            world.wait_for_log("start tag=v1 "),
            "the old version is started again\n{}",
            world.stub_log()
        );

        // Retry once the program is closed: a new transaction, the same stage.
        holder.terminate();
        drop(holder);
        world.install_helper("e1e2");
        let retried = world.run_helper(world.apply_arguments("e1e2", 0, TIMINGS), "e1e2");
        assert_eq!(retried.exit_code, 0, "{}", describe(&retried));
        world.expect_v2();
        assert!(world.wait_for_log("start tag=v2 "), "{}", world.stub_log());
    }

    #[test]
    fn the_update_waits_for_the_window_to_exit_and_gives_up_if_it_never() {
        let _serial = serial();
        crate::require_can_sign!();
        {
            // The window exits after a while: the helper waits for it and then applies.
            let world = World::new("swap-wait-parent");
            world.arrange_environment("healthy");
            let window = start_sleeper_at(&world.top.join("window").join("katana.exe"), 1500);
            let ran = world.run_helper(
                world.apply_arguments(
                    TRANSACTION,
                    u64::from(window.pid),
                    "20000,1500,50,20000,30000",
                ),
                TRANSACTION,
            );
            assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
            world.expect_v2();
            assert!(world.wait_for_log("start tag=v2 "), "{}", world.stub_log());
        }
        {
            // The window does not exit: blocked, naming it, and no second window is started beside
            // it.
            let world = World::new("swap-parent-stays");
            world.arrange_environment("healthy");
            let window = start_sleeper_at(&world.top.join("window").join("katana.exe"), 120_000);
            let ran = world.run_helper(
                world.apply_arguments(TRANSACTION, u64::from(window.pid), TIMINGS),
                TRANSACTION,
            );
            assert_eq!(ran.exit_code, 10, "{}", describe(&ran));
            assert_eq!(
                value_of(&output_of(&ran), "holder"),
                format!("Katana ({})", window.pid),
                "a pid with no image to name is called Katana"
            );
            world.expect_v1();
            std::thread::sleep(Duration::from_millis(500));
            assert!(
                !world.log_has("start tag="),
                "the window that hung is not started a second time\n{}",
                world.stub_log()
            );
        }
    }

    #[test]
    fn an_update_running_in_another_helper_is_not_disturbed_and_exits_with_code13() {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-locked");
        world.arrange_environment("healthy");
        std::fs::create_dir_all(world.root.join(".update")).expect("the work folder");
        // The lock as the other helper holds it: opened for writing with no sharing at all.
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(world.root.join(".update").join("lock"))
            .expect("the lock, held");
        let ran = world.run_helper(world.apply_arguments(TRANSACTION, 0, TIMINGS), TRANSACTION);
        drop(lock);
        assert_eq!(ran.exit_code, 13, "{}", describe(&ran));
        assert_eq!(value_of(&output_of(&ran), "reason"), "locked");
        world.expect_v1();
        assert!(
            !is_present(&world.state.join("last.json")),
            "the other helper owns the outcome"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !world.log_has("start tag="),
            "and nothing is started: the other helper starts Katana\n{}",
            world.stub_log()
        );
    }

    #[test]
    fn a_rollback_that_cannot_finish_exits_with_code12_and_recover_by_hand_puts_the_tree_back() {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-rollback-incomplete");
        // The new version starts and stays in its health check until this test lets it go, and
        // then exits 1; in that time the test holds a file of the new version open with no sharing
        // for deletion, so the rollback cannot move it back. No step of that is a bet on how fast
        // the machine is: the stub waits for the test (a pause of a fixed length in the stub is one
        // a busy machine outlasts), the test waits for the stub's line for as long as the helper
        // runs (the helper, started at the same moment, has a limit of its own; on a busy machine
        // it was not yet at the health check after the 30 s this wait once had, and went on to
        // it), and the helper's health wait is one no run sits out (LONG_HEALTH_WAIT). The rename
        // retry is 1 s: how long the rollback tries the file this test holds before it gives up.
        world.arrange_environment("exit1_when_told");
        let helper = world.helper_path(TRANSACTION);
        let folder = helper.parent().map(Path::to_path_buf).unwrap_or_default();
        let arguments = world.apply_arguments(
            TRANSACTION,
            0,
            &timings_with_a_long_health_wait(Duration::from_secs(1)),
        );
        let ended = AtomicBool::new(false);
        let mut started = false;
        let mut held: Option<std::io::Result<File>> = None;
        let ran = std::thread::scope(|scope| {
            let running = scope.spawn(|| {
                let ran = run_program_in(&helper, arguments, &folder, PATIENCE);
                ended.store(true, Ordering::SeqCst);
                ran
            });
            loop {
                // Asked before the log is read: a line the helper's run wrote is then seen,
                // whenever it ended.
                let helper_ended = ended.load(Ordering::SeqCst);
                started = world.log_has("health tag=v2 ");
                if helper_ended || started {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if started {
                held = Some(hold(&world.root.join("bin").join("libcore-1.dll")));
            }
            // Whatever came of that, the stub is let go and the helper's thread joined before
            // anything is judged.
            world.tell_the_stub_to_exit();
            running.join().ok()
        });
        assert!(
            started,
            "the new version never started: {}",
            ran.as_ref().map(describe).unwrap_or_default()
        );
        let held = match held {
            Some(Ok(file)) => file,
            Some(Err(error)) => panic!("the new library could not be held: {error}"),
            None => panic!("the new library was not held"),
        };
        let ran = ran.expect("the helper's run");

        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 12, "{}", describe(&ran));
        assert_eq!(value_of(&said, "outcome"), "rollback_incomplete");
        assert_eq!(value_of(&said, "reason"), "rollback_failed");
        assert!(
            value_of(&said, "detail").contains("katana_updater --recover"),
            "it names the command to run"
        );
        assert_eq!(world.last().status, "rollback_incomplete");
        assert!(
            world.run_once_is_set(),
            "the next logon will try again: the entry is kept"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !world.log_has("start tag="),
            "a half-swapped tree is not started\n{}",
            world.stub_log()
        );

        // The lock is let go; the person (or the next start of a program, or the next logon) runs
        // it.
        drop(held);
        let recovered = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(recovered.exit_code, 0, "{}", describe(&recovered));
        assert_eq!(value_of(&output_of(&recovered), "recover"), "ok");
        world.expect_v1();
        assert!(!world.run_once_is_set(), "recovery removes the entry");
        assert_eq!(
            leftovers_in_update(&world.root),
            vec![String::from("recovery.log")],
            "recovery cleared the transaction and kept its one log line"
        );
    }

    #[test]
    fn a_crash_in_the_middle_of_the_commit_is_put_right_by_recover_and_leaves_only_the_recovery_log()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let (world, crashed, at) = crashed_mid_commit("swap-crash", &is_half_swapped);
        let trace = format!("crashed before change {at}");
        assert!(
            output_of(&crashed).contains(&format!("simulated crash before change {at}")),
            "{trace}: {}",
            describe(&crashed)
        );

        // What a crash leaves: the RunOnce entry, the lock released with the process, a half
        // swapped tree.
        assert_eq!(
            world.run_once(),
            run_once_command(&world.helper_path(TRANSACTION), &world.root),
            "{trace}"
        );
        assert!(
            !is_present(&world.state.join("last.json")),
            "{trace}: a process that was killed wrote nothing"
        );
        assert!(!is_present(
            &world.root.join(".update").join("recovery.log")
        ));

        // Recovery as the RunOnce entry or a person runs it, in an environment whose
        // KATANA_UPDATE_STATE_DIR names a new empty folder: that folder must stay empty, and the
        // state folder the swap used must stay as it was, byte for byte.
        let elsewhere = world.top.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("another state folder");
        world.environment.set("KATANA_UPDATE_STATE_DIR", &elsewhere);
        let before = snapshot(&world.state);

        let recovered = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(recovered.exit_code, 0, "{trace}: {}", describe(&recovered));
        assert_eq!(value_of(&output_of(&recovered), "recover"), "ok");
        world.expect_v1();
        assert!(
            !world.run_once_is_set(),
            "recovery removes the RunOnce entry"
        );
        assert!(
            !is_present(&world.root.join(".update").join(TRANSACTION)),
            "the transaction folder is gone"
        );

        assert!(
            snapshot(&elsewhere).is_empty(),
            "recovery wrote into the per-user state folder"
        );
        assert_eq!(
            difference(&before, &snapshot(&world.state)),
            "",
            "recovery changed the state folder the swap used"
        );
        assert!(
            !is_present(&world.state.join("last.json")),
            "no last.json: recover has no state folder to put one in"
        );

        // The one record it does keep: a line in the install's own recovery.log.
        let log = world.root.join(".update").join("recovery.log");
        assert!(is_present(&log));
        let lines = lines_of(&text_of_file(&log));
        assert_eq!(lines.len(), 1, "{}", text_of_file(&log));
        assert!(
            lines[0].contains(&format!(" recover found={TRANSACTION} result=ok left=")),
            "{}",
            lines[0]
        );
        // A UTC time first, as 2026-10-13T08:40:00Z is written.
        let line = lines[0].as_bytes();
        assert!(line.len() >= 21, "{}", lines[0]);
        assert_eq!(line[4], b'-', "{}", lines[0]);
        assert_eq!(line[10], b'T', "{}", lines[0]);
        assert_eq!(&line[19..21], b"Z ", "{}", lines[0]);

        // And a second run has nothing to put right: nothing printed as work, nothing logged.
        let again = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(again.exit_code, 0, "{}", describe(&again));
        assert_eq!(value_of(&output_of(&again), "recover"), "nothing_to_do");
        assert_eq!(
            lines_of(&text_of_file(&log)),
            lines,
            "a run that found nothing writes no line"
        );
    }

    #[test]
    fn recovery_started_the_way_a_headless_program_starts_it_writes_the_recovery_log_and_no_per_user_state()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let (world, _, _) = crashed_mid_commit("swap-headless-recover", &is_half_swapped);
        assert!(
            names_in(&world.state).contains(&format!("run-{TRANSACTION}/")),
            "the premise: the helper copy the swap ran from is in U"
        );
        // The per-user folder, entry by entry and byte by byte, as the crash left it.
        let before = snapshot(&world.state);

        // katana_cli and katana_mcp call recover(R) first thing (recover.rs); this process plays
        // their part. U is the folder the swap used (KATANA_UPDATE_STATE_DIR), so the helper copy
        // is found there and run with `--recover R` - the engine's own command line, no test
        // option: the environment names the registry of this test.
        let recovered = recover(&world.root);
        assert!(recovered.is_ok(), "{recovered:?}");
        world.expect_v1();
        assert!(
            !world.run_once_is_set(),
            "the helper removed the RunOnce entry the crash left"
        );

        // What it wrote: one line in the install's own recovery.log.
        assert_eq!(
            leftovers_in_update(&world.root),
            vec![String::from("recovery.log")]
        );
        let log = world.root.join(".update").join("recovery.log");
        let lines = lines_of(&text_of_file(&log));
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains(&format!(" recover found={TRANSACTION} result=ok left=")),
            "{}",
            lines[0]
        );

        // What it did not write: any per-user state. No last.json - only the helper run for the
        // window writes one - no state.json, no health marker, no cleanup.list; and the rule is
        // held to the letter (recover.rs: "writes only R/.update/recovery.log"): the per-user
        // folder is as it was, nothing in it made, changed or removed - the helper copy it ran
        // included.
        assert!(!is_present(&world.state.join("last.json")));
        assert!(!is_present(&world.state.join("state.json")));
        assert!(!is_present(&health_marker_path(&world.state, TRANSACTION)));
        assert!(!is_present(&world.state.join("cleanup.list")));
        assert_eq!(
            difference(&before, &snapshot(&world.state)),
            "",
            "recover() changed the per-user state folder. A headless program writes no per-user \
             state: recover() may read U to find the helper copy and must leave it as it is \
             (recover.rs)"
        );

        // And a start after that finds nothing to do, and still writes nothing there.
        assert!(recover(&world.root).is_ok());
        assert_eq!(lines_of(&text_of_file(&log)), lines);
        assert_eq!(difference(&before, &snapshot(&world.state)), "");
    }

    #[test]
    fn recover_neither_reads_nor_writes_through_an_update_folder_that_is_a_link() {
        let _serial = serial();
        crate::require_can_sign!();
        // R/.update is a junction to a folder outside the install, and that folder holds what looks
        // like an interrupted transaction. Recovery reads nothing through a link (recover.rs), and
        // the swap makes nothing through one, so no transaction of this install can be in there:
        // the program has nothing to do, says so, and neither lists the folder as this install's
        // nor leaves its recovery.log in it - which would be a write outside R.
        let world = World::new("swap-recover-link");
        world.arrange_environment("healthy");
        let outside = world.top.join("outside");
        write_text(
            &outside.join("c0ffee").join("journal"),
            "1 PLAN c0ffee 1000 2000 00000000\n",
        );
        write_text(&outside.join("note.txt"), "somebody else's folder\n");
        let before = snapshot(&outside);
        let link = world.root.join(".update");
        if let Err(said) = make_junction(&link, &outside) {
            crate::skip_unless_available_or_fail_in_ci!(format!(
                "a junction could not be made here: {said}"
            ));
        }
        assert!(
            is_present(&link.join("c0ffee").join("journal")),
            "the premise: the link leads to the folder"
        );

        let ran = world.run_helper(world.recover_arguments(), TRANSACTION);
        // The link itself goes before anything is judged (only the link: the folder it points to
        // stays), so that the tree and the folder are compared as they are and the world can be
        // cleared away.
        let unlinked = std::fs::remove_dir(&link);
        assert!(
            unlinked.is_ok(),
            "the junction could not be removed: {unlinked:?}"
        );

        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        assert_eq!(value_of(&said, "recover"), "nothing_to_do", "{said}");
        assert!(
            !said.contains("c0ffee"),
            "the folder behind the link was listed as this install's\n{said}"
        );
        assert_eq!(
            difference(&before, &snapshot(&outside)),
            "",
            "the folder behind the link was written to"
        );
        assert!(!is_present(&outside.join("recovery.log")));
        world.expect_v1();
        assert!(!is_present(&world.state.join("last.json")));
    }

    #[test]
    fn a_crash_inside_recovery_leaves_a_state_that_the_next_recover_finishes() {
        let _serial = serial();
        crate::require_can_sign!();
        let (world, _, _) = crashed_mid_commit("swap-recover-crash", &is_half_swapped);

        // Recovery itself is killed before its second change to the disk (the first is the
        // journal's ROLLING_BACK line): the tree is still mixed, the entry still stands, nothing
        // was logged.
        let mut killed = world.recover_arguments();
        killed.push(String::from("--die-at"));
        killed.push(String::from("2"));
        let died = world.run_helper(killed, TRANSACTION);
        assert_eq!(died.exit_code, 99, "{}", describe(&died));
        assert_ne!(tree_hash(&world.root), world.v1_hash);
        assert_ne!(tree_hash(&world.root), world.v2_hash);
        assert!(world.run_once_is_set());
        assert!(
            !is_present(&world.root.join(".update").join("recovery.log")),
            "a process that was killed wrote no line"
        );

        // The next one picks up from the disk and finishes.
        let finished = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(finished.exit_code, 0, "{}", describe(&finished));
        assert_eq!(value_of(&output_of(&finished), "recover"), "ok");
        world.expect_v1();
        assert!(!world.run_once_is_set());
        assert_eq!(
            lines_of(&text_of_file(
                &world.root.join(".update").join("recovery.log")
            ))
            .len(),
            1
        );
    }

    #[test]
    fn recover_on_an_install_that_was_never_updated_has_nothing_to_do_and_writes_nothing() {
        let _serial = serial();
        crate::require_can_sign!();
        let world = World::new("swap-recover-clean");
        world.arrange_environment("healthy");
        let ran = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        assert_eq!(value_of(&output_of(&ran), "recover"), "nothing_to_do");
        world.expect_v1();
        assert!(
            !is_present(&world.root.join(".update")),
            "not even the work folder is made"
        );
        assert!(!is_present(&world.state.join("last.json")));
    }

    #[test]
    fn recover_that_cannot_put_the_tree_back_exits_with_code12_and_says_why() {
        let _serial = serial();
        crate::require_can_sign!();
        // A crashed commit after the new library is in place; the library is then held, so that it
        // cannot be moved out again.
        let new_library_in_place = |world: &World| {
            read_text(&world.root.join("bin").join("libcore-1.dll"))
                == world
                    .v2
                    .find("bin/libcore-1.dll")
                    .map(|file| file.content.clone())
                    .unwrap_or_default()
        };
        let (world, _, _) = crashed_mid_commit("swap-recover-fails", &new_library_in_place);
        // Held open with no sharing for deletion: it cannot be moved back, so the rollback cannot
        // finish.
        let held = hold(&world.root.join("bin").join("libcore-1.dll")).expect("the library, held");
        let mut arguments = world.recover_arguments();
        arguments.extend(strings(&["--timings", "500,500,50,500,500"]));
        let ran = world.run_helper(arguments, TRANSACTION);
        drop(held);
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 12, "{}", describe(&ran));
        assert_eq!(value_of(&said, "recover"), "failed");
        assert!(
            value_of(&said, "detail").contains("katana_updater --recover"),
            "{said}"
        );
        assert!(
            world.run_once_is_set(),
            "the entry stays for the next logon"
        );
        assert!(
            !is_present(&world.state.join("last.json")),
            "a failed recovery writes no per-user state either"
        );
        let lines = lines_of(&text_of_file(
            &world.root.join(".update").join("recovery.log"),
        ));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("result=failed"), "{}", lines[0]);

        // Released, it finishes.
        let finished = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(finished.exit_code, 0, "{}", describe(&finished));
        world.expect_v1();
    }

    // ---- a helper that died before it could say that the new version starts ----------------------
    //
    // The update is in place, the journal says COMMITTED_PENDING_HEALTH, and the helper that would
    // have written HEALTHY - or put the old version back - is dead: power, a shutdown, the process
    // ended. No program's recover() touches such an update; `katana_updater --recover` settles it
    // (recover.rs, settle_unproved; the table at the top of src/lib.rs says what it prints, and the
    // cases read the first word of its `detail=` line from that table). What each case expects is
    // the specification's: the tree built from the release (v2) when the new version's marker is
    // there; the tree as it was before the update (v1) when it is not and nothing of the install
    // is running; and nothing at all changed, with the logon entry back in place, while a program
    // of the install runs.

    // The number of the change to the disk with which a swap of a World's releases writes HEALTHY,
    // counted the way --die-at counts. Found without starting anything: a twin of the world's
    // install and stage is run through apply() in this process, on a file system that writes every
    // change down. Which changes a swap makes is decided by its own logic and by what the tree
    // holds, not by which file system carries them out, so the number is the real program's too -
    // which killed_before_healthy checks, and does not take on trust.
    fn healthy_append_of_a_twin(like: &World) -> u64 {
        let mut twin = Scenario::new("helper-twin");
        twin.v1 = like.v1.clone();
        twin.v2 = like.v2.clone();
        write_install(&twin.root, &twin.v1);
        write_stage(&twin.stage, &twin.v2);
        let disk = StdFs;
        let log = LogFs::new(&disk);
        let rig = Rig::with_transaction(&twin, &log, TRANSACTION);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        log.find("appendDurable", " HEALTHY ", 1)
    }

    // Whether the run a probe made was killed between its COMMITTED_PENDING_HEALTH record and its
    // HEALTHY one. Only one k does that: the record after COMMITTED_PENDING_HEALTH is HEALTHY (the
    // health run in between changes nothing through the helper), so a run killed one change later
    // has HEALTHY in its journal.
    fn is_unsettled(probe: &CrashProbe) -> bool {
        if !probe.crashed {
            return false;
        }
        let journal = text_of_file(
            &probe
                .world
                .root
                .join(".update")
                .join(TRANSACTION)
                .join("journal"),
        );
        journal.contains("COMMITTED_PENDING_HEALTH") && !journal.contains(" HEALTHY ")
    }

    // A world in which the real program applied the update, ran the new version for its health
    // check - which wrote its marker and ended well - and was killed before the change that would
    // have recorded the update as healthy. The k for --die-at is the twin's, checked against what
    // the program left; should the two ever disagree it is found by bisection, as
    // crashed_mid_commit finds its own, and once in a test process either way.
    //
    // THE HEALTH WAIT of these runs is one no run sits out (LONG_HEALTH_WAIT): the helper goes on
    // when the new version's health run ENDS, however long a busy machine takes to start it, and
    // the test's own limit on the run (PATIENCE) is all that bounds it. With the 20 s of TIMINGS a
    // start slower than that is a health run that "timed out": the helper turns to putting the old
    // version back, and its first change to the disk on that road has the very number HEALTHY has
    // on the other - so --die-at stops it there too, with a journal that reads exactly like the one
    // wanted over an update that never said it starts.
    fn killed_before_healthy(label: &str) -> Box<World> {
        static AT: AtomicU64 = AtomicU64::new(0);
        let timings = timings_with_a_long_health_wait(Duration::from_secs(30));
        let unsettled_at = |k: u64| -> Option<Box<World>> {
            let probe = probe_crash(label, k, &timings);
            if is_unsettled(&probe) {
                Some(probe.world)
            } else {
                None
            }
        };
        if AT.load(Ordering::SeqCst) == 0 {
            let twin = {
                let sizing = World::new(&format!("{label}-sizing"));
                healthy_append_of_a_twin(&sizing)
            };
            if twin != 0
                && let Some(world) = unsettled_at(twin)
            {
                AT.store(twin, Ordering::SeqCst);
                return world;
            }
            // Whether the run killed before change k had got as far as COMMITTED_PENDING_HEALTH:
            // once yes, yes for every larger k. Not killed at all is past it; so is a run that left
            // no journal but the new version's install record (the clean-up of a healthy update
            // deletes the journal while changes are still counted: crashed_mid_commit says the same
            // of its own).
            let reached_pending = |k: u64| {
                let probe = probe_crash(&format!("{label}-find"), k, &timings);
                if !probe.crashed {
                    return true;
                }
                let killed = &probe.world;
                let journal = killed
                    .root
                    .join(".update")
                    .join(TRANSACTION)
                    .join("journal");
                if text_of_file(&journal).contains("COMMITTED_PENDING_HEALTH") {
                    return true;
                }
                !is_present(&journal)
                    && read_text(
                        &killed
                            .root
                            .join("share")
                            .join("katana")
                            .join("installed.json"),
                    ) == installed_json(&killed.v2)
            };
            let mut low: u64 = 1; // before the first change: nothing on disk
            let mut high: u64 = 32;
            while !reached_pending(high) {
                low = high;
                high *= 2;
                assert!(
                    high <= 4096,
                    "no run got as far as COMMITTED_PENDING_HEALTH within 4096 changes to the disk"
                );
            }
            while low + 1 < high {
                let middle = low + (high - low) / 2;
                if reached_pending(middle) {
                    high = middle;
                } else {
                    low = middle;
                }
            }
            // The first change after the pending record: the HEALTHY append.
            AT.store(high, Ordering::SeqCst);
        }
        let at = AT.load(Ordering::SeqCst);
        unsettled_at(at).unwrap_or_else(|| {
            panic!(
                "the run killed before change {at} did not stop between COMMITTED_PENDING_HEALTH \
                 and HEALTHY"
            )
        })
    }

    // Every entry under a world's install folder, its .update included (the tree hash leaves that
    // out), and the per-user folder: what "nothing was changed" compares.
    fn install_and_state(world: &World) -> BTreeMap<String, String> {
        let name = |path: &Path| {
            format!(
                "{}/",
                path.file_name().unwrap_or_default().to_string_lossy()
            )
        };
        let install = name(&world.root);
        let state = name(&world.state);
        snapshot(world.root.parent().expect("the install's folder"))
            .into_iter()
            .filter(|(entry, _)| entry.starts_with(&install) || entry.starts_with(&state))
            .collect()
    }

    // The one line `--recover` keeps of a run, in the install's own log.
    fn recovery_log(world: &World) -> Vec<String> {
        lines_of(&log_of(&world.root.join(".update").join("recovery.log")))
    }

    // What every case here starts from, checked: the new version is in place and said that it
    // starts, the journal is pending, and the dead helper left its RunOnce entry and no last.json.
    fn expect_unsettled(world: &World) {
        world.expect_v2();
        assert!(
            is_present(&health_marker_path(&world.state, TRANSACTION)),
            "the health run wrote its marker"
        );
        let lines = lines_of(&world.stub_log());
        assert_eq!(lines.len(), 1, "{}", world.stub_log());
        assert!(lines[0].starts_with("health tag=v2 "), "{}", lines[0]);
        assert_eq!(
            world.run_once(),
            run_once_command(&world.helper_path(TRANSACTION), &world.root)
        );
        assert!(
            !is_present(&world.state.join("last.json")),
            "a process that was killed wrote nothing"
        );
        // The helper waited for the health run to end before it went on, so nothing of the install
        // is running.
        let running = make_system_process_ops().holders_under(&world.root);
        assert!(
            matches!(&running, Ok(programs) if programs.is_empty()),
            "a program of the install is still running: {running:?}"
        );
    }

    #[test]
    fn an_update_whose_helper_died_after_the_new_version_said_it_starts_is_confirmed_by_recover_which_only_reads_the_per_user_folder()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let world = killed_before_healthy("unsettled-healthy");
        expect_unsettled(&world);

        // Every program's recover() leaves such an update exactly as it is: it may be a live
        // helper's, waiting for that very program. This process plays katana_cli's part.
        {
            let before = install_and_state(&world);
            let recovered = recover(&world.root);
            assert!(recovered.is_ok(), "{recovered:?}");
            assert_eq!(
                difference(&before, &install_and_state(&world)),
                "",
                "recover() changed the install or the per-user folder"
            );
            assert!(world.run_once_is_set());
        }

        // The next logon. Windows takes the entry away and runs its command; the install is an
        // installer's here, whose entry in Settings > Apps still names the version before.
        world.register_as_installer();
        world.consume_run_once();
        assert!(!world.run_once_is_set());
        let user_before = snapshot(&world.state);

        let ran = world.run_helper(world.recover_arguments(), TRANSACTION);
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        assert_eq!(value_of(&said, "recover"), "ok");
        assert!(
            value_of(&said, "detail").starts_with("confirmed:"),
            "{said}"
        );
        world.expect_v2();
        assert!(
            !is_present(&world.root.join(".update").join(TRANSACTION)),
            "the transaction folder is gone"
        );
        assert_eq!(
            world.display_version().as_deref(),
            Some("0.10.0"),
            "Settings > Apps names the version that is installed"
        );
        assert_eq!(world.display_name().as_deref(), Some("Katana 0.10.0"));
        assert!(
            !world.run_once_is_set(),
            "nothing is left for a logon to do"
        );
        // U was read, for the marker, and that is all: `--recover` is what headless programs
        // start.
        assert_eq!(
            difference(&user_before, &snapshot(&world.state)),
            "",
            "--recover changed the per-user folder"
        );
        assert!(!is_present(&world.state.join("last.json")));
        assert_eq!(
            lines_of(&world.stub_log()).len(),
            1,
            "no program was started to find out\n{}",
            world.stub_log()
        );

        let lines = recovery_log(&world);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains(&format!(" recover found={TRANSACTION} result=ok left=")),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].ends_with("left="),
            "nothing is left, so the line ends there: {}",
            lines[0]
        );

        // And a second run has nothing to do.
        let again = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(again.exit_code, 0, "{}", describe(&again));
        assert_eq!(value_of(&output_of(&again), "recover"), "nothing_to_do");
        assert_eq!(recovery_log(&world), lines);
        world.expect_v2();
    }

    #[test]
    fn a_recover_that_dies_while_it_settles_had_put_the_logon_entry_back_first_and_the_next_one_finishes()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let world = killed_before_healthy("unsettled-dies");
        expect_unsettled(&world);
        let journal = world.root.join(".update").join(TRANSACTION).join("journal");
        let journal_before = read_text(&journal);

        // The logon: Windows has taken the entry away, and the run it started loses power before
        // its FIRST change to the disk (--die-at 1: the HEALTHY record). Were the entry put back
        // only at the end, no later logon would come for this update.
        world.consume_run_once();
        let mut killed = world.recover_arguments();
        killed.push(String::from("--die-at"));
        killed.push(String::from("1"));
        let died = world.run_helper(killed, TRANSACTION);
        assert_eq!(died.exit_code, 99, "{}", describe(&died));
        assert_eq!(
            world.run_once(),
            run_once_command(&world.helper_path(TRANSACTION), &world.root),
            "the entry was put back before anything else was done"
        );
        world.expect_v2();
        assert_eq!(
            read_text(&journal),
            journal_before,
            "the record was not written: it decides again"
        );
        assert!(
            !is_present(&world.root.join(".update").join("recovery.log")),
            "a process that was killed wrote no line"
        );

        // That logon: decided again from the same marker, and finished.
        world.consume_run_once();
        let finished = world.run_helper(world.recover_arguments(), TRANSACTION);
        let said = output_of(&finished);
        assert_eq!(finished.exit_code, 0, "{}", describe(&finished));
        assert!(
            value_of(&said, "detail").starts_with("confirmed:"),
            "{said}"
        );
        world.expect_v2();
        assert!(!is_present(&world.root.join(".update").join(TRANSACTION)));
        assert!(!world.run_once_is_set());
    }

    #[test]
    fn an_update_whose_new_version_never_said_it_starts_is_put_back_by_recover_when_nothing_of_the_install_is_running()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let world = killed_before_healthy("unsettled-rollback");
        expect_unsettled(&world);
        // As a helper that died while the new version was still starting leaves it: no marker.
        std::fs::remove_file(health_marker_path(&world.state, TRANSACTION))
            .expect("the marker goes");
        world.register_as_installer();
        world.consume_run_once(); // the logon
        let user_before = snapshot(&world.state);

        let ran = world.run_helper(world.recover_arguments(), TRANSACTION);
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        assert_eq!(value_of(&said, "recover"), "ok");
        assert!(value_of(&said, "detail").starts_with("put back:"), "{said}");
        world.expect_v1();
        assert!(
            !is_present(&world.root.join(".update").join(TRANSACTION)),
            "the transaction folder is gone"
        );
        assert_eq!(
            world.display_version().as_deref(),
            Some("0.9.0"),
            "a release that never proved itself is never shown"
        );
        assert!(!world.run_once_is_set());
        // What this run cannot record is not recorded: no last.json, and no state.json - the
        // release is not blocked, and the next check offers it again.
        assert_eq!(
            difference(&user_before, &snapshot(&world.state)),
            "",
            "--recover changed the per-user folder"
        );
        assert!(!is_present(&world.state.join("last.json")));
        assert!(!is_present(&world.state.join("state.json")));
        assert_eq!(
            lines_of(&world.stub_log()).len(),
            1,
            "the old version is not started either\n{}",
            world.stub_log()
        );
        let lines = recovery_log(&world);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains(&format!(" recover found={TRANSACTION} result=ok left=")),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn recover_puts_nothing_back_under_a_running_program_and_puts_the_logon_entry_back_for_the_next_try()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let world = killed_before_healthy("unsettled-holder");
        expect_unsettled(&world);
        let journal = world.root.join(".update").join(TRANSACTION).join("journal");
        std::fs::remove_file(health_marker_path(&world.state, TRANSACTION))
            .expect("the marker goes");
        // The install's own katana_cli.exe - the new version's - is running: somebody is using it.
        let holder = Sleeper::new(&world.root.join("bin").join("katana_cli.exe"), 120_000);
        assert_ne!(holder.pid, 0);
        world.consume_run_once(); // the logon
        let journal_before = read_text(&journal);
        let user_before = snapshot(&world.state);

        let ran = world.run_helper(world.recover_arguments(), TRANSACTION);
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
        assert_eq!(value_of(&said, "recover"), "ok");
        assert!(value_of(&said, "detail").starts_with("left:"), "{said}");
        // Nothing of the update was changed: the tree, its folder and its journal are as they were.
        world.expect_v2();
        assert_eq!(read_text(&journal), journal_before);
        assert_eq!(
            difference(&user_before, &snapshot(&world.state)),
            "",
            "--recover changed the per-user folder"
        );
        // And the entry Windows took away is there again, so that the next logon comes back.
        assert_eq!(
            world.run_once(),
            run_once_command(&world.helper_path(TRANSACTION), &world.root)
        );
        let lines = recovery_log(&world);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains(&format!(
                " recover found={TRANSACTION} result=ok left={TRANSACTION}"
            )),
            "{}",
            lines[0]
        );

        // That logon, with the program closed: now it is put back.
        holder.terminate();
        drop(holder);
        world.consume_run_once();
        let later = world.run_helper(world.recover_arguments(), TRANSACTION);
        let said = output_of(&later);
        assert_eq!(later.exit_code, 0, "{}", describe(&later));
        assert!(value_of(&said, "detail").starts_with("put back:"), "{said}");
        world.expect_v1();
        assert!(!world.run_once_is_set());
    }

    #[test]
    fn recover_that_cannot_put_an_unproved_update_back_exits_with_code12_and_the_next_run_finishes_it()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let world = killed_before_healthy("unsettled-incomplete");
        expect_unsettled(&world);
        std::fs::remove_file(health_marker_path(&world.state, TRANSACTION))
            .expect("the marker goes");
        world.consume_run_once(); // the logon
        // A file of the new version held open with no sharing for deletion: it can be read - the
        // tree is still that release - and it cannot be moved out, so the putting back cannot
        // finish.
        let held = hold(&world.root.join("bin").join("libcore-1.dll")).expect("the library, held");
        // The fifth number is how long a rename that Windows refuses is tried again.
        let mut arguments = world.recover_arguments();
        arguments.extend(strings(&["--timings", "500,500,50,500,500"]));
        let ran = world.run_helper(arguments, TRANSACTION);
        drop(held);
        let said = output_of(&ran);
        assert_eq!(ran.exit_code, 12, "{}", describe(&ran));
        assert_eq!(value_of(&said, "recover"), "failed");
        assert!(
            value_of(&said, "detail").contains("katana_updater --recover"),
            "{said}"
        );
        assert_eq!(
            world.run_once(),
            run_once_command(&world.helper_path(TRANSACTION), &world.root),
            "the entry is back, and stays, for the next logon"
        );
        assert!(!is_present(&world.state.join("last.json")));
        let lines = recovery_log(&world);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains(&format!(
                " recover found={TRANSACTION} result=failed left={TRANSACTION}"
            )),
            "{}",
            lines[0]
        );

        // Released: the journal says ROLLING_BACK, which every run of recovery finishes.
        let finished = world.run_helper(world.recover_arguments(), TRANSACTION);
        assert_eq!(finished.exit_code, 0, "{}", describe(&finished));
        assert_eq!(value_of(&output_of(&finished), "recover"), "ok");
        world.expect_v1();
        assert!(!world.run_once_is_set());
    }

    #[test]
    fn recover_gives_way_to_a_helper_that_holds_the_lock_and_puts_nothing_back_on_a_marker_missing_from_another_per_user_folder()
     {
        let _serial = serial();
        crate::require_can_sign!();
        let world = killed_before_healthy("unsettled-not-mine");
        expect_unsettled(&world);
        let journal = world.root.join(".update").join(TRANSACTION).join("journal");
        let journal_before = read_text(&journal);
        let user_before = snapshot(&world.state);
        world.consume_run_once(); // a logon: Windows takes the entry away, then runs its command

        // 1. A helper is alive: it holds the update lock, as a swap does from its first step to its
        //    last, opened with no sharing at all. The update is its own to settle, and so is the
        //    registry: `busy:`, exit 0, and the entry Windows took away is NOT put back over it.
        {
            let lock = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .share_mode(0)
                .open(world.root.join(".update").join("lock"))
                .expect("the lock, held");
            // The second number is for how long a lock that is held is asked for again. The lock
            // is held for the whole run, so no figure changes the answer; this one keeps the run
            // short.
            let mut arguments = world.recover_arguments();
            arguments.extend(strings(&["--timings", "500,500,50,500,500"]));
            let ran = world.run_helper(arguments, TRANSACTION);
            drop(lock);
            let said = output_of(&ran);
            assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
            assert_eq!(value_of(&said, "recover"), "ok");
            assert!(value_of(&said, "detail").starts_with("busy:"), "{said}");
            world.expect_v2();
            assert_eq!(read_text(&journal), journal_before);
            assert_eq!(
                difference(&user_before, &snapshot(&world.state)),
                "",
                "--recover changed the per-user folder"
            );
            assert!(
                !world.run_once_is_set(),
                "the registry is the live helper's while it holds the lock"
            );
        }

        // 2. The lock is free, and this run's environment names ANOTHER per-user folder than the
        //    one the update was applied from (KATANA_UPDATE_STATE_DIR set where Katana is started
        //    and not at the logon; another account): an empty one. No marker is in it - and none
        //    was ever going to be: the new version wrote its marker in its own folder, where it
        //    still is. A release that proved itself is not put back for that. It is left, and the
        //    logon entry is back so that the next logon decides.
        {
            let elsewhere = world.top.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).expect("another state folder");
            world.environment.set("KATANA_UPDATE_STATE_DIR", &elsewhere);
            let ran = world.run_helper(world.recover_arguments(), TRANSACTION);
            world
                .environment
                .set("KATANA_UPDATE_STATE_DIR", &world.state);
            let said = output_of(&ran);
            assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
            assert_eq!(value_of(&said, "recover"), "ok");
            assert!(value_of(&said, "detail").starts_with("left:"), "{said}");
            world.expect_v2();
            assert_eq!(read_text(&journal), journal_before);
            assert!(
                snapshot(&elsewhere).is_empty(),
                "nothing is made in the folder it looked in"
            );
            assert_eq!(
                difference(&user_before, &snapshot(&world.state)),
                "",
                "--recover changed the per-user folder"
            );
            assert_eq!(
                world.run_once(),
                run_once_command(&world.helper_path(TRANSACTION), &world.root),
                "the entry is back, for the next logon"
            );
        }

        // 3. That logon, in the environment the update was applied in: the marker is found.
        {
            world.consume_run_once();
            let ran = world.run_helper(world.recover_arguments(), TRANSACTION);
            let said = output_of(&ran);
            assert_eq!(ran.exit_code, 0, "{}", describe(&ran));
            assert!(
                value_of(&said, "detail").starts_with("confirmed:"),
                "{said}"
            );
            world.expect_v2();
            assert!(
                !is_present(&world.root.join(".update").join(TRANSACTION)),
                "the transaction folder is gone"
            );
            assert!(!world.run_once_is_set());
        }
    }
}
