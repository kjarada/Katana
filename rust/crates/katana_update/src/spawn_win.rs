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

#![allow(
    unsafe_code,
    reason = "Processes, pipes and tokens are reached through their Win32 calls, which are foreign functions."
)]

use crate::command_line::command_line;
use crate::fsops::path_text;
use crate::process::{Launcher, SpawnRequest, SpawnResult, utf8_from_wide};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use std::ffi::OsString;
use std::mem::zeroed;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::Instant;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree, SetHandleInformation,
};
use windows_sys::Win32::Security::{GetTokenInformation, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile};
use windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode;
use windows_sys::Win32::System::Environment::GetCommandLineW;
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken,
    PROCESS_INFORMATION, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

const OUTPUT_LIMIT: usize = 64 * 1024;

// The values of the Windows SDK's headers (winerror.h, winnt.h, winbase.h, processthreadsapi.h).
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const GENERIC_READ: u32 = 0x8000_0000;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const HANDLE_FLAG_INHERIT: u32 = 0x1;
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
const STARTF_USESTDHANDLES: u32 = 0x100;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x400;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
const CREATE_DEFAULT_ERROR_MODE: u32 = 0x0400_0000;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SEM_FAILCRITICALERRORS: u32 = 0x1;
const SEM_NOGPFAULTERRORBOX: u32 = 0x2;
const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
const WAIT_OBJECT_0: u32 = 0;
const TOKEN_QUERY: u32 = 0x8;
const TOKEN_ELEVATION_TYPE_CLASS: i32 = 18; // TokenElevationType of TOKEN_INFORMATION_CLASS
const TOKEN_ELEVATION_TYPE_DEFAULT: i32 = 1;
const TOKEN_ELEVATION_TYPE_FULL: i32 = 2;

fn last_error() -> u32 {
    // SAFETY: GetLastError only reads this thread's last-error value.
    unsafe { GetLastError() }
}

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

fn wide_terminated(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

// The command line as the NUL-terminated wide text CreateProcessW takes (and may write to).
fn command_of(request: &SpawnRequest) -> Result<Vec<u16>> {
    let program = preferred(&request.executable);
    let Some(program) = program.to_str() else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the command line is not valid UTF-8",
            path_text(&request.executable),
        ));
    };
    Ok(command_line(program, &request.arguments)
        .encode_utf16()
        .chain([0])
        .collect())
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

struct Closer(HANDLE);

impl Closer {
    fn get(&self) -> HANDLE {
        self.0
    }

    fn reset(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle is one this Closer owns, closed once.
            unsafe { CloseHandle(self.0) };
        }
        self.0 = null_mut();
    }
}

impl Drop for Closer {
    fn drop(&mut self) {
        self.reset();
    }
}

// Takes what the child has written so far; keeps the first OUTPUT_LIMIT bytes and reads (and
// drops) the rest, so a chatty child never blocks on a full pipe.
fn drain(pipe: HANDLE, output: &mut Vec<u8>) {
    let mut buffer = [0u8; 4096];
    loop {
        let mut available = 0u32;
        // SAFETY: `pipe` is the read end this launcher owns; only the count is asked for.
        let peeked = unsafe {
            PeekNamedPipe(
                pipe,
                null_mut(),
                0,
                null_mut(),
                &raw mut available,
                null_mut(),
            )
        } != 0;
        if !peeked || available == 0 {
            return;
        }
        let want = available.min(buffer.len() as u32);
        let mut got = 0u32;
        // SAFETY: at most `want` bytes, which the buffer holds, are written into it.
        let read = unsafe {
            ReadFile(
                pipe,
                buffer.as_mut_ptr().cast(),
                want,
                &raw mut got,
                null_mut(),
            )
        } != 0;
        if !read || got == 0 {
            return;
        }
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
        // SAFETY: SetErrorMode only sets the process's error mode; it returns the old one, which
        // is kept with what the process already asked for.
        let previous = unsafe { SetErrorMode(0) };
        // SAFETY: as above.
        unsafe {
            SetErrorMode(
                previous | SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX,
            )
        };
        Self { previous }
    }
}

impl Drop for ErrorDialogsOff {
    fn drop(&mut self) {
        // SAFETY: as in new().
        unsafe { SetErrorMode(self.previous) };
    }
}

struct WindowsLauncher;

impl Launcher for WindowsLauncher {
    fn run(&self, request: &SpawnRequest) -> Result<SpawnResult> {
        let mut command = command_of(request)?;

        let inheritable = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut read_end: HANDLE = null_mut();
        let mut write_end: HANDLE = null_mut();
        // SAFETY: the two handles are written by the call; the attributes outlive it.
        if unsafe {
            CreatePipe(
                &raw mut read_end,
                &raw mut write_end,
                &raw const inheritable,
                0,
            )
        } == 0
        {
            return Err(start_error(last_error(), &request.executable));
        }
        let reader = Closer(read_end);
        let mut writer = Closer(write_end);
        // SAFETY: the read end is this launcher's own handle.
        unsafe { SetHandleInformation(read_end, HANDLE_FLAG_INHERIT, 0) };
        // The child reads nothing: its standard input is the null device.
        let null_device: Vec<u16> = "NUL".encode_utf16().chain([0]).collect();
        // SAFETY: a NUL-terminated name and attributes that outlive the call.
        let mut nothing = Closer(unsafe {
            CreateFileW(
                null_device.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &raw const inheritable,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        });

        // Only these two handles are inherited, not every inheritable one this process has.
        let inherited: [HANDLE; 2] = [nothing.get(), writer.get()];
        let mut attribute_bytes = 0usize;
        // SAFETY: the first call only reports the size the list needs.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &raw mut attribute_bytes) };
        // Eight-byte units, so that the list is aligned as the system's structure is.
        let mut attribute_storage = vec![0u64; attribute_bytes.div_ceil(8)];
        let attributes: LPPROC_THREAD_ATTRIBUTE_LIST = attribute_storage.as_mut_ptr().cast();
        // SAFETY: the storage holds `attribute_bytes`; `inherited` outlives the process creation.
        let ready = unsafe {
            InitializeProcThreadAttributeList(attributes, 1, 0, &raw mut attribute_bytes) != 0
                && UpdateProcThreadAttribute(
                    attributes,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                    inherited.as_ptr().cast(),
                    size_of_val(&inherited),
                    null_mut(),
                    null(),
                ) != 0
        };
        if !ready {
            return Err(start_error(last_error(), &request.executable));
        }

        // SAFETY: STARTUPINFOEXW is plain data, and all zero is a structure with nothing set.
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = nothing.get();
        startup.StartupInfo.hStdOutput = writer.get();
        startup.StartupInfo.hStdError = writer.get();
        startup.lpAttributeList = attributes;

        let program = wide_terminated(&preferred(&request.executable));
        let directory = preferred(&request.working_directory);
        let directory_wide = wide_terminated(&directory);
        let directory_pointer = if directory.as_os_str().is_empty() {
            null()
        } else {
            directory_wide.as_ptr()
        };
        // SAFETY: PROCESS_INFORMATION is plain data, filled by the call.
        let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
        let (started, started_error) = {
            let _no_dialogs = ErrorDialogsOff::new();
            // SAFETY: every pointer is to a NUL-terminated wide string or a structure that
            // outlives the call; the command line is a mutable buffer of this launcher's.
            let started = unsafe {
                CreateProcessW(
                    program.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    1,
                    CREATE_NO_WINDOW
                        | CREATE_UNICODE_ENVIRONMENT
                        | EXTENDED_STARTUPINFO_PRESENT
                        | CREATE_DEFAULT_ERROR_MODE,
                    null(),
                    directory_pointer,
                    &raw const startup.StartupInfo,
                    &raw mut process,
                )
            } != 0;
            (started, last_error())
        };
        // SAFETY: the list was initialised above and is not used again.
        unsafe { DeleteProcThreadAttributeList(attributes) };
        if !started {
            return Err(start_error(started_error, &request.executable));
        }
        let process_handle = Closer(process.hProcess);
        let _thread_handle = Closer(process.hThread);
        // Our copy of the write end must go, or the pipe never reports the end of the output.
        writer.reset();
        nothing.reset();

        let mut result = SpawnResult::default();
        let deadline = Instant::now().checked_add(request.timeout);
        loop {
            drain(reader.get(), &mut result.output);
            // SAFETY: the process handle is this launcher's own.
            if unsafe { WaitForSingleObject(process_handle.get(), 10) } == WAIT_OBJECT_0 {
                break;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                // SAFETY: as above.
                unsafe {
                    TerminateProcess(process_handle.get(), 1);
                    WaitForSingleObject(process_handle.get(), 5000);
                }
                result.timed_out = true;
                break;
            }
        }
        drain(reader.get(), &mut result.output);
        let mut code = 0u32;
        // SAFETY: as above; the code is written by the call.
        if unsafe { GetExitCodeProcess(process_handle.get(), &raw mut code) } != 0 {
            result.exit_code = code as i32;
        }
        Ok(result)
    }

    fn start_detached(&self, request: &SpawnRequest) -> Status {
        let mut command = command_of(request)?;

        let program = wide_terminated(&preferred(&request.executable));
        let directory = preferred(&request.working_directory);
        let directory_wide = wide_terminated(&directory);
        let directory_pointer = if directory.as_os_str().is_empty() {
            null()
        } else {
            directory_wide.as_ptr()
        };
        // SAFETY: plain data, all zero is nothing set.
        let mut startup: STARTUPINFOW = unsafe { zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        // SAFETY: plain data, filled by the call.
        let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
        let common = CREATE_NO_WINDOW
            | CREATE_UNICODE_ENVIRONMENT
            | CREATE_NEW_PROCESS_GROUP
            | CREATE_DEFAULT_ERROR_MODE;
        let (started, started_error) = {
            let _no_dialogs = ErrorDialogsOff::new();
            // Break away from a job that would kill the child with its parent, when the job
            // allows it; when it does not, CreateProcess says access denied and the child just
            // starts.
            // SAFETY: as in run().
            let mut started = unsafe {
                CreateProcessW(
                    program.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    0,
                    common | CREATE_BREAKAWAY_FROM_JOB,
                    null(),
                    directory_pointer,
                    &raw const startup,
                    &raw mut process,
                )
            } != 0;
            let mut started_error = last_error();
            if !started && started_error == ERROR_ACCESS_DENIED {
                // SAFETY: as above.
                started = unsafe {
                    CreateProcessW(
                        program.as_ptr(),
                        command.as_mut_ptr(),
                        null(),
                        null(),
                        0,
                        common,
                        null(),
                        directory_pointer,
                        &raw const startup,
                        &raw mut process,
                    )
                } != 0;
                started_error = last_error();
            }
            (started, started_error)
        };
        if !started {
            return Err(start_error(started_error, &request.executable));
        }
        // SAFETY: the two handles the call returned, closed once.
        unsafe {
            CloseHandle(process.hProcess);
            CloseHandle(process.hThread);
        }
        Ok(())
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
    let mut opened: HANDLE = null_mut();
    // SAFETY: the pseudo-handle of this process, and a place for the token handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut opened) } == 0 {
        return false;
    }
    let token = Closer(opened);
    let mut kind: i32 = TOKEN_ELEVATION_TYPE_DEFAULT;
    let mut written = 0u32;
    // SAFETY: the answer is a TOKEN_ELEVATION_TYPE, four bytes, written into `kind`.
    let asked = unsafe {
        GetTokenInformation(
            token.get(),
            TOKEN_ELEVATION_TYPE_CLASS,
            (&raw mut kind).cast(),
            size_of::<i32>() as u32,
            &raw mut written,
        )
    } != 0;
    if !asked || written as usize != size_of::<i32>() {
        return false;
    }
    is_elevated_half(kind as u32)
}

// The command line as UTF-8 (process.rs, utf8_arguments).
pub(crate) fn utf8_arguments() -> Vec<Vec<u8>> {
    let mut count = 0i32;
    // SAFETY: GetCommandLineW returns this process's command line, which lives as long as the
    // process; CommandLineToArgvW allocates the array it returns.
    let wide = unsafe { CommandLineToArgvW(GetCommandLineW(), &raw mut count) };
    if !wide.is_null() {
        let count = usize::try_from(count).unwrap_or(0);
        let mut arguments = Vec::with_capacity(count);
        for i in 0..count {
            // SAFETY: the array holds `count` pointers to NUL-terminated wide strings.
            let units = unsafe {
                let argument = *wide.add(i);
                let length = (0..).take_while(|&at| *argument.add(at) != 0).count();
                std::slice::from_raw_parts(argument, length)
            };
            arguments.push(utf8_from_wide(units).into_bytes());
        }
        // SAFETY: the array was allocated by CommandLineToArgvW, freed once with LocalFree.
        unsafe { LocalFree(wide.cast()) };
        return arguments;
    }
    // Out of memory in the one call: fall back on what the standard library parsed, which is
    // right for ASCII and the best there is otherwise.
    std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned().into_bytes())
        .collect()
}
