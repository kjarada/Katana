//! The Windows Launcher of the process module, the command line read as UTF-8, and the elevation
//! question. Compiled on Windows only.
//!
//! CREATE_NO_WINDOW, in BOTH run() and start_detached(): katana_updater is a console program, and
//! started from the window (a GUI program with no console of its own) Windows would open a black
//! console for the length of the swap, and again for the preflight's --selftest. The flag gives a
//! console program a console that has no window. It is not combined with DETACHED_PROCESS, which
//! the documentation says makes it ignore CREATE_NO_WINDOW; a detached child outlives its parent
//! anyway, and CREATE_BREAKAWAY_FROM_JOB (when the job allows it) is what lets the helper outlive
//! a window that was started inside a job that kills its children when it closes.
//!
//! Nothing goes through a shell: the command line is made from the program and its arguments by
//! the quoting rules CommandLineToArgvW reads back (command_line.rs), and a program that is not
//! found is an error, not a message from cmd.exe.
//!
//! No dialog, ever: CreateProcess on a file that is not a program Windows can run (a text file
//! named .exe, a program for another architecture, a truncated download) does not just fail, it
//! raises a system message box ("This app can't run on your PC", or on some builds "Unsupported
//! 16-Bit Application") and waits for someone to click it - in a helper that runs unattended, or
//! in a test, that is a hang. ErrorDialogsOff turns the box off for the length of the call
//! (SEM_FAILCRITICALERRORS), and CREATE_DEFAULT_ERROR_MODE keeps the child from inheriting the
//! setting, so the child behaves as it would if the user had started it.
//!
//! The calls themselves are win32::process's safe wrappers (docs/rust.md, "Unsafe confined to FFI
//! modules", which also says why std::process::Command does not replace them).

use crate::command_line::command_line;
use crate::fsops::path_text;
use crate::process::{Launcher, SpawnRequest, SpawnResult, utf8_from_wide};
use crate::win32::WideName;
use crate::win32::process::{
    InheritedHandles, bytes_waiting, command_line_arguments, exit_code, null_device_for_reading,
    output_pipe, read_pipe, set_error_mode, start_alone, start_inheriting, terminate,
    token_elevation_type, wait,
};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::OwnedHandle;
use std::path::{Path, PathBuf};
use std::time::Instant;

const OUTPUT_LIMIT: usize = 64 * 1024;

// The values of the Windows SDK's headers (winerror.h, winnt.h, winbase.h, processthreadsapi.h).
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x400;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
const CREATE_DEFAULT_ERROR_MODE: u32 = 0x0400_0000;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SEM_FAILCRITICALERRORS: u32 = 0x1;
const SEM_NOGPFAULTERRORBOX: u32 = 0x2;
const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
const WAIT_OBJECT_0: u32 = 0;
const TOKEN_ELEVATION_TYPE_FULL: i32 = 2;

// A path with the separators of the platform. cmd.exe reads its own command line for "/"
// switches and does not skip the quoted name of the program: a program path with a forward slash
// in it (C:/Windows/System32/cmd.exe) makes it find "/cmd.exe", read it as the switch /c and run
// ".exe /c ..." as a command. Callers join paths with "/" all the time, so the launcher settles it
// once.
fn preferred(path: &Path) -> PathBuf {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                unit
            }
        })
        .collect();
    PathBuf::from(OsString::from_wide(&wide))
}

fn wide_terminated(path: &Path) -> WideName {
    WideName::from_units(path.as_os_str().encode_wide().collect())
}

// The command line as the NUL-terminated wide text CreateProcessW takes (and may write to).
fn command_of(request: &SpawnRequest) -> Result<WideName> {
    let program = preferred(&request.executable);
    let Some(program) = program.to_str() else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the command line is not valid UTF-8",
            path_text(&request.executable),
        ));
    };
    Ok(WideName::from_text(&command_line(program, &request.arguments)))
}

// The working directory as CreateProcessW takes it: None (the caller's) for an empty one.
fn directory_of(request: &SpawnRequest) -> Option<WideName> {
    let directory = preferred(&request.working_directory);
    (!directory.as_os_str().is_empty()).then(|| wide_terminated(&directory))
}

fn start_error(error: u32, program: &Path) -> Error {
    let missing = error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND;
    make_error(
        if missing {
            ErrorCode::NotFound
        } else {
            ErrorCode::InvalidState
        },
        "the program could not be started",
        format!("os_error={error} {}", path_text(program)),
    )
}

// Takes what the child has written so far; keeps the first OUTPUT_LIMIT bytes and reads (and
// drops) the rest, so a chatty child never blocks on a full pipe.
fn drain(pipe: &OwnedHandle, output: &mut Vec<u8>) {
    let mut buffer = [0u8; 4096];
    loop {
        let available = match bytes_waiting(pipe) {
            Some(available) if available != 0 => available,
            _ => return,
        };
        let want = (available as usize).min(buffer.len());
        let got = match read_pipe(pipe, &mut buffer[..want]) {
            Some(got) if got != 0 => got,
            _ => return,
        };
        if output.len() < OUTPUT_LIMIT {
            let take = (got as usize).min(OUTPUT_LIMIT - output.len());
            output.extend_from_slice(&buffer[..take]);
        }
    }
}

// Turns the system's "cannot run this file" message box off while it lives, and puts the error
// mode back. The process error mode is shared by every thread, so two launches at once can
// restore the wrong value; the flags this adds are the ones this library wants always, and the
// helper sets the same flags for the whole process in main(), so the worst case of a race is a
// restore to a value that already had them.
struct ErrorDialogsOff {
    previous: u32,
}

impl ErrorDialogsOff {
    fn new() -> Self {
        // The old mode is kept with what the process already asked for.
        let previous = set_error_mode(0);
        set_error_mode(
            previous | SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX,
        );
        Self { previous }
    }
}

impl Drop for ErrorDialogsOff {
    fn drop(&mut self) {
        set_error_mode(self.previous);
    }
}

struct WindowsLauncher;

impl Launcher for WindowsLauncher {
    fn run(&self, request: &SpawnRequest) -> Result<SpawnResult> {
        let mut command = command_of(request)?;

        let (reader, writer) =
            output_pipe().map_err(|error| start_error(error, &request.executable))?;
        // The child reads nothing: its standard input is the null device. When that will not
        // open, the child is handed no input handle at all (INVALID_HANDLE_VALUE).
        let nothing = null_device_for_reading();
        // Only these two handles are inherited, not every inheritable one this process has.
        let inherited = InheritedHandles::new(nothing.as_ref(), &writer)
            .map_err(|error| start_error(error, &request.executable))?;

        let program = wide_terminated(&preferred(&request.executable));
        let directory = directory_of(request);
        let started = {
            let _no_dialogs = ErrorDialogsOff::new();
            start_inheriting(
                &program,
                &mut command,
                directory.as_ref(),
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | CREATE_DEFAULT_ERROR_MODE,
                &inherited,
            )
        };
        // The list is not used again.
        drop(inherited);
        let child = started.map_err(|error| start_error(error, &request.executable))?;
        // Our copy of the write end must go, or the pipe never reports the end of the output.
        drop(writer);
        drop(nothing);

        let mut result = SpawnResult::default();
        let deadline = Instant::now().checked_add(request.timeout);
        loop {
            drain(&reader, &mut result.output);
            if wait(&child.process, 10) == WAIT_OBJECT_0 {
                break;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                terminate(&child.process, 1);
                wait(&child.process, 5000);
                result.timed_out = true;
                break;
            }
        }
        drain(&reader, &mut result.output);
        if let Some(code) = exit_code(&child.process) {
            result.exit_code = code as i32;
        }
        Ok(result)
    }

    fn start_detached(&self, request: &SpawnRequest) -> Status {
        let mut command = command_of(request)?;

        let program = wide_terminated(&preferred(&request.executable));
        let directory = directory_of(request);
        let common = CREATE_NO_WINDOW
            | CREATE_UNICODE_ENVIRONMENT
            | CREATE_NEW_PROCESS_GROUP
            | CREATE_DEFAULT_ERROR_MODE;
        let started = {
            let _no_dialogs = ErrorDialogsOff::new();
            // Break away from a job that would kill the child with its parent, when the job
            // allows it; when it does not, CreateProcess says access denied and the child just
            // starts.
            match start_alone(
                &program,
                &mut command,
                directory.as_ref(),
                common | CREATE_BREAKAWAY_FROM_JOB,
            ) {
                Err(ERROR_ACCESS_DENIED) => {
                    start_alone(&program, &mut command, directory.as_ref(), common)
                }
                started => started,
            }
        };
        // The two handles the call returned are closed as `started` drops.
        started
            .map(drop)
            .map_err(|error| start_error(error, &request.executable))
    }
}

pub(crate) fn make_system_launcher() -> Box<dyn Launcher> {
    Box::new(WindowsLauncher)
}

/// Which answer of TokenElevationType is the elevated half of a split token:
/// TokenElevationTypeFull is that half; TokenElevationTypeLimited is the other half, and
/// TokenElevationTypeDefault a token that was never split (User Account Control off, or the
/// built-in administrator).
pub fn is_elevated_half(kind: u32) -> bool {
    kind == TOKEN_ELEVATION_TYPE_FULL as u32
}

// The process module says what is asked and why. Every failure to ask is "not elevated".
pub(crate) fn process_is_elevated() -> bool {
    token_elevation_type().is_some_and(|kind| is_elevated_half(kind as u32))
}

// The command line as UTF-8 (process.rs, utf8_arguments).
pub(crate) fn utf8_arguments() -> Vec<Vec<u8>> {
    if let Some(wide) = command_line_arguments() {
        return wide
            .iter()
            .map(|units| utf8_from_wide(units).into_bytes())
            .collect();
    }
    // Out of memory in the one call: fall back on what the standard library parsed, which is
    // right for ASCII and the best there is otherwise.
    std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned().into_bytes())
        .collect()
}
