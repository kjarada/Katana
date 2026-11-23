//! Processes, behind two traits so that the apply, the recovery and the preflight are driven
//! in-process by tests with fakes.
//!
//! - ProcessOps: who is running (and which of them hold files under the install root), and
//!   whether a pid is alive. The apply algorithm waits on both and NEVER kills anything: it says
//!   who holds the files and gives up. Windows: Toolhelp snapshot plus QueryFullProcessImageNameW;
//!   Linux: /proc/<pid>/exe. The real implementations are the helper's process scans
//!   (src/katana_updater/process_scan_win.cpp and process_scan_posix.cpp in the C++), compiled
//!   into the update library (the program asks the same question to say `blocked`): here
//!   process_scan_win.rs and process_scan_posix.rs, behind make_system_process_ops.
//! - Launcher: starts a program: waits for it (the preflight self-test, the recovery run) or
//!   starts it detached (the helper copy that outlives the window, the relaunch of Katana). Real
//!   implementations: spawn_win.rs and spawn_posix.rs. On Windows the child is started with
//!   CREATE_NO_WINDOW, for the helper is a console program and its parent is a GUI program:
//!   without the flag a black console opens for the length of every swap and every self-test.
//!
//! A program's file path is a PathBuf; arguments are UTF-8 and reach the child as given (Windows:
//! quoted by the CreateProcessW rules; never through a shell).
//!
//! THE CHILD'S ENVIRONMENT is this process's unless the request says otherwise, and what it says
//! is data of the request (SpawnRequest::environment): a variable set for the child, or kept from
//! it. The launcher builds the child's environment from that (child_environment) and hands it to
//! the system with the program. THIS PROCESS'S OWN ENVIRONMENT IS NEVER CHANGED FOR A LAUNCH:
//! changing it is unsafe in a process with a second thread (edition 2024 says so of
//! std::env::set_var), and a variable set "for the length of the call" is one every other launch
//! and every reader of that moment sees too. The helper's hardening is such a request
//! (signature.rs, harden_open_ssl_environment).
//!
//! WHAT THE WINDOWS SCAN ANSWERS (the helper's process_scan_win gives the reasons):
//! - holders_under: the processes STARTED FROM a file under the root, and for each
//!   RunningProgram::image is that executable - never a module it loaded: what must not live
//!   through a swap is a program of the old version, the commit only renames (which a loaded DLL
//!   allows), and listing the modules of every process fails on exactly the protected ones. The
//!   root may be spelled in any way that names the folder (either case, forward slashes, a
//!   trailing separator, "." and ".." segments, the extended-length prefix, 8.3 names, a
//!   junction); the paths are put in one form and compared with the file system's own case
//!   folding, and C:/Katana2 is not under C:/Katana. A root that is not there holds nothing; an
//!   empty one is InvalidArgument. A process that has ended is not listed. Nor is one this user
//!   may not open (another account's): it gives no image path and is passed over, NOT taken for
//!   a holder - and because the file of a running program can be renamed, the update goes
//!   through under it. A snapshot that cannot be taken or read is an error, never "no holders":
//!   the apply then refuses with "scan_failed". No elevation is ever asked for.
//! - is_alive: asks about the ID, which Windows may hand to a new process once the old one is
//!   gone; the trait has no creation time to tell them apart, and the worst case is a wait for the
//!   wrong process that ends as "blocked". A process that exists but may not be opened is alive;
//!   0, and a number wider than a process id, are not.

use katana_core::{Result, Status};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunningProgram {
    pub pid: u64,
    /// The executable or loaded module under the root.
    pub image: PathBuf,
}

pub trait ProcessOps {
    /// This process's id.
    fn current_pid(&self) -> u64;

    /// True while `pid` names a live process.
    fn is_alive(&self, pid: u64) -> bool;

    /// Every process whose executable image is under `root` (case-insensitive on Windows),
    /// excluding this process. Katana programs a client holds open - katana_mcp - are among them.
    fn holders_under(&self, root: &Path) -> Result<Vec<RunningProgram>>;
}

/// One thing a launch changes in the environment its program is handed: the variable named is
/// set to the value, or, with no value, kept from the child. The name is matched as the system
/// matches one (Windows: without regard to case), so a change of OPENSSL_CONF is a change of
/// `openssl_conf` there.
pub type EnvironmentChange = (String, Option<OsString>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnRequest {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    /// Empty: the child inherits ours.
    pub working_directory: PathBuf,
    /// Waiting spawns only: kill the child and report timed_out after this long.
    pub timeout: Duration,
    /// What the child's environment differs from this process's by, made in the order given
    /// (child_environment). Empty: the child inherits ours as it is. A name that is empty or
    /// holds "=" or a NUL, and a value that holds a NUL, are InvalidArgument when the program is
    /// started: no system can hand a program such a variable.
    pub environment: Vec<EnvironmentChange>,
}

impl Default for SpawnRequest {
    fn default() -> Self {
        Self {
            executable: PathBuf::new(),
            arguments: Vec::new(),
            working_directory: PathBuf::new(),
            timeout: Duration::from_millis(60_000),
            environment: Vec::new(),
        }
    }
}

// Whether `held`, the name a variable has in an environment, names the variable `name`, as the
// system that keeps the environment matches the two: Windows without regard to case, by the
// table its file names are folded with; elsewhere a name is its bytes.
pub(crate) fn is_variable(held: &OsStr, name: &str) -> bool {
    #[cfg(windows)]
    {
        crate::spawn_win::same_variable_name(held, name)
    }
    #[cfg(not(windows))]
    {
        held == OsStr::new(name)
    }
}

/// `inherited` with `changes` made in order: a change takes every variable of its name out
/// (however the name is spelled where the system does not tell the spellings apart), and one
/// with a value then adds the variable, under the change's own spelling, at the end. The
/// variables no change names keep their order. It reads no environment: the one a launch starts
/// from is handed in, so that the rule can be put to variables no process needs to hold.
pub fn environment_with(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
    changes: &[EnvironmentChange],
) -> Vec<(OsString, OsString)> {
    let mut variables: Vec<(OsString, OsString)> = inherited.into_iter().collect();
    for (name, value) in changes {
        variables.retain(|(held, _)| !is_variable(held, name));
        if let Some(value) = value {
            variables.push((OsString::from(name), value.clone()));
        }
    }
    variables
}

/// The environment the program of `request` is started with: this process's as it is when this
/// is called, with the request's changes made (environment_with). The system launcher builds
/// the environment of every program whose request changes anything from exactly this, so what
/// this answers is what such a child is handed; it changes nothing of this process.
pub fn child_environment(request: &SpawnRequest) -> Vec<(OsString, OsString)> {
    environment_with(std::env::vars_os(), &request.environment)
}

/// The value the variable `name` has in `environment`, the name matched as the system matches
/// one; None when the environment does not hold it.
pub fn variable_in<'a>(environment: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    environment
        .iter()
        .find(|(held, _)| is_variable(held, name))
        .map(|(_, value)| value.as_os_str())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpawnResult {
    pub exit_code: i32,
    pub timed_out: bool,
    /// The child's standard output and error, merged, capped at 64 KiB: bytes, as the child wrote
    /// them (the cap may cut a character in two).
    pub output: Vec<u8>,
}

pub trait Launcher {
    /// Runs the program to its end (or the timeout) and reports how it ended. An error only when
    /// it could not be started.
    fn run(&self, request: &SpawnRequest) -> Result<SpawnResult>;

    /// Starts the program and returns at once, the child detached from this process's console,
    /// job and lifetime (it must outlive the window that started it). `timeout` is ignored.
    fn start_detached(&self, request: &SpawnRequest) -> Status;
}

/// The program's own command line as UTF-8, argv[0] first. EVERY main() of the update system
/// (katana_updater, katana_update_e2e, katana_updater_test) reads its arguments through this:
/// on Windows the C runtime's argv holds the ANSI code page's version of the command line, so a
/// profile folder with an accented letter in its name reaches the helper as '?' or mojibake, and
/// the helper is exactly the program started with such paths (--root, --stage, --state).
/// Windows: CommandLineToArgvW(GetCommandLineW()) converted to UTF-8; elsewhere argv as given,
/// which is UTF-8 or raw bytes by the file system's own rule - hence bytes. A path is made of one
/// with katana_core's path_from_utf8, never by handing the text to a Path directly. The C++ takes
/// main's argc and argv; a Rust program has them from the standard library.
pub fn utf8_arguments() -> Vec<Vec<u8>> {
    #[cfg(windows)]
    {
        crate::spawn_win::utf8_arguments()
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStringExt;
        std::env::args_os().map(OsStringExt::into_vec).collect()
    }
}

/// UTF-16 to UTF-8 (the conversion of utf8_arguments, exposed so that it can be tested with text
/// no command line is needed to carry). A well-formed surrogate pair becomes one 4-byte sequence;
/// an unpaired surrogate becomes U+FFFD (EF BF BD).
#[cfg(windows)]
pub fn utf8_from_wide(wide: &[u16]) -> String {
    const REPLACEMENT: char = '\u{FFFD}';
    let mut out = String::with_capacity(wide.len());
    let mut i = 0;
    while i < wide.len() {
        let unit = u32::from(wide[i]);
        if (0xd800..=0xdbff).contains(&unit) {
            // A high surrogate: needs a low one next.
            if let Some(&next) = wide.get(i + 1) {
                let next = u32::from(next);
                if (0xdc00..=0xdfff).contains(&next) {
                    let code = 0x10000 + ((unit - 0xd800) << 10) + (next - 0xdc00);
                    out.push(char::from_u32(code).unwrap_or(REPLACEMENT));
                    i += 2;
                    continue;
                }
            }
            out.push(REPLACEMENT);
        } else if (0xdc00..=0xdfff).contains(&unit) {
            // A low surrogate with no high one.
            out.push(REPLACEMENT);
        } else {
            out.push(char::from_u32(unit).unwrap_or(REPLACEMENT));
        }
        i += 1;
    }
    out
}

/// The system process seam: process_scan_win.rs on Windows (the Toolhelp scan); elsewhere
/// process_scan_posix.rs, which answers who it is and who is alive and refuses to guess who runs
/// from an install (its holders_under is Unsupported, `reason=platform`, until the POSIX apply is
/// written). The C++ compiles both into the update library from src/katana_updater.
pub fn make_system_process_ops() -> Box<dyn ProcessOps> {
    #[cfg(windows)]
    {
        crate::process_scan_win::make_system_process_ops()
    }
    #[cfg(not(windows))]
    {
        crate::process_scan_posix::make_system_process_ops()
    }
}

/// The engine's running_programs(root): make_system_process_ops().holders_under(root).
pub fn running_programs(root: &Path) -> Result<Vec<RunningProgram>> {
    make_system_process_ops().holders_under(root)
}

/// The system launcher: spawn_win.rs on Windows; spawn_posix.rs elsewhere, which refuses until
/// the POSIX apply is written.
pub fn make_system_launcher() -> Box<dyn Launcher> {
    #[cfg(windows)]
    {
        crate::spawn_win::make_system_launcher()
    }
    #[cfg(not(windows))]
    {
        crate::spawn_posix::make_system_launcher()
    }
}

/// Whether this process holds the ELEVATED HALF OF A SPLIT TOKEN: a program started "as
/// administrator" on a Windows machine where an administrator's programs ordinarily run with the
/// other, filtered half. The helper runs from a copy in the per-user update folder (the stage's
/// preflight), which those ordinary programs can write: started from such a process it would run
/// with administrator rights from a folder that programs without them control. So `UPDATE APPLY
/// CONFIRM` is refused there, before the copy is made (reason=elevated).
///
/// Windows: GetTokenInformation(TokenElevationType) answers TokenElevationTypeFull. A machine
/// with that protection turned off, and the built-in administrator account, have ONE token and no
/// unelevated half that could interfere (TokenElevationTypeDefault): false, and an update is
/// applied there as anywhere. A process that could not ask - its token cannot be opened or read -
/// is answered false as well, and that is no error: the question guards one case, and what cannot
/// be shown to be that case is not refused an update for it. Elsewhere than Windows: false.
pub fn process_is_elevated() -> bool {
    #[cfg(windows)]
    {
        crate::spawn_win::process_is_elevated()
    }
    #[cfg(not(windows))]
    {
        crate::spawn_posix::process_is_elevated()
    }
}

/// Turns the system's error boxes off for THIS process, for good: no "there is no disk in the
/// drive", no crash box, no "cannot open file" box. For a program that runs with no window on a
/// desktop nobody may be watching (the helper), where a box would hold the work until somebody
/// clicked it. The programs the launcher starts do not inherit it (spawn_win.rs,
/// CREATE_DEFAULT_ERROR_MODE).
///
/// Windows: SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX |
/// SEM_NOOPENFILEERRORBOX), the mode set to exactly those. Elsewhere there are no such boxes and
/// nothing is done.
pub fn turn_error_boxes_off() {
    #[cfg(windows)]
    crate::spawn_win::turn_error_boxes_off();
}

#[doc(hidden)]
pub mod detail {
    /// Which answer of TokenElevationType is the elevated half of a split token: the one decision
    /// process_is_elevated makes, apart from asking. `kind` is the TOKEN_ELEVATION_TYPE as a
    /// number, so that every kind can be put to it by a test whatever token the test's own run
    /// holds.
    #[cfg(windows)]
    pub use crate::spawn_win::is_elevated_half;
}
