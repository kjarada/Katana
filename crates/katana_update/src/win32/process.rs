//! The process calls of the update layer: starting a program (spawn_win), the process scan
//! (process_scan_win), the token's elevation and the command line. Safe wrappers that answer as
//! their calls do and leave the error for `last_error()`, or return it, read at once.
//!
//! The launch keeps what std::process::Command cannot say (docs/rust.md, "Unsafe confined to FFI
//! modules", has the comparison): exactly two handles inherited, by PROC_THREAD_ATTRIBUTE_HANDLE_
//! LIST, the child's standard output and error one pipe read by peeking, the program named as
//! lpApplicationName with the command line spawn_win built, and a launch that goes on without
//! CREATE_BREAKAWAY_FROM_JOB when the job refuses it.

use std::marker::PhantomData;
use std::os::windows::io::{AsRawHandle, FromRawHandle, HandleOrInvalid, HandleOrNull, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    HANDLE, INVALID_HANDLE_VALUE, LocalFree, SetHandleInformation,
};
use windows_sys::Win32::Globalization::CompareStringOrdinal;
use windows_sys::Win32::Security::{GetTokenInformation, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile};
use windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
};
use windows_sys::Win32::System::Environment::GetCommandLineW;
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess,
    OpenProcessToken, PROCESS_INFORMATION, QueryFullProcessImageNameW, STARTUPINFOEXW,
    STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

use super::{WideName, last_error, length_u32};

// The values of the Windows SDK's headers (winnt.h, winbase.h, processthreadsapi.h).
const GENERIC_READ: u32 = 0x8000_0000;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const HANDLE_FLAG_INHERIT: u32 = 0x1;
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
const STARTF_USESTDHANDLES: u32 = 0x100;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const TOKEN_QUERY: u32 = 0x8;
const TOKEN_ELEVATION_TYPE_CLASS: i32 = 18; // TokenElevationType of TOKEN_INFORMATION_CLASS

// An inheritable SECURITY_ATTRIBUTES with the default descriptor.
fn inheritable() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: length_u32(size_of::<SECURITY_ATTRIBUTES>()),
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    }
}

// ---- handles ---------------------------------------------------------------------------------

/// Owns what a call returned as a handle, or INVALID_HANDLE_VALUE for none.
///
/// # Safety
/// `raw` is a handle the caller now owns, which nothing else closes, or INVALID_HANDLE_VALUE.
unsafe fn owned_or_invalid(raw: HANDLE) -> Option<OwnedHandle> {
    // SAFETY: the caller's contract is HandleOrInvalid's.
    OwnedHandle::try_from(unsafe { HandleOrInvalid::from_raw_handle(raw) }).ok()
}

/// As owned_or_invalid, for a call that returns NULL for none.
///
/// # Safety
/// `raw` is a handle the caller now owns, which nothing else closes, or NULL.
unsafe fn owned_or_null(raw: HANDLE) -> Option<OwnedHandle> {
    // SAFETY: the caller's contract is HandleOrNull's.
    OwnedHandle::try_from(unsafe { HandleOrNull::from_raw_handle(raw) }).ok()
}

/// WaitForSingleObject: WAIT_OBJECT_0, WAIT_TIMEOUT or WAIT_FAILED.
pub(crate) fn wait(handle: &OwnedHandle, milliseconds: u32) -> u32 {
    // SAFETY: the handle is open (borrowed from its owner for the call).
    unsafe { WaitForSingleObject(handle.as_raw_handle(), milliseconds) }
}

/// GetExitCodeProcess; None when it fails.
pub(crate) fn exit_code(process: &OwnedHandle) -> Option<u32> {
    let mut code = 0u32;
    // SAFETY: the handle is open; the code is written into a local.
    (unsafe { GetExitCodeProcess(process.as_raw_handle(), &raw mut code) } != 0).then_some(code)
}

/// TerminateProcess with `code`.
pub(crate) fn terminate(process: &OwnedHandle, code: u32) -> bool {
    // SAFETY: the handle is open.
    unsafe { TerminateProcess(process.as_raw_handle(), code) != 0 }
}

// ---- the pipe of a child's output ------------------------------------------------------------

/// An anonymous pipe whose write end is inheritable and whose read end is not: the read end and
/// the write end. Err with the system's error when it cannot be made.
pub(crate) fn output_pipe() -> Result<(OwnedHandle, OwnedHandle), u32> {
    let attributes = inheritable();
    let mut read_end: HANDLE = null_mut();
    let mut write_end: HANDLE = null_mut();
    // SAFETY: the two handles are written by the call; the attributes outlive it.
    if unsafe { CreatePipe(&raw mut read_end, &raw mut write_end, &raw const attributes, 0) } == 0
    {
        return Err(last_error());
    }
    // SAFETY: CreatePipe succeeded, so both are handles this process now owns, closed once by
    // their OwnedHandle; neither is NULL.
    let (reader, writer) = unsafe {
        (
            OwnedHandle::from_raw_handle(read_end),
            OwnedHandle::from_raw_handle(write_end),
        )
    };
    // SAFETY: the read end is this launcher's own handle, open.
    unsafe { SetHandleInformation(reader.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) };
    Ok((reader, writer))
}

/// The null device opened for reading, inheritable; None when it cannot be opened.
pub(crate) fn null_device_for_reading() -> Option<OwnedHandle> {
    let name = WideName::from_text("NUL");
    let attributes = inheritable();
    // SAFETY: a NUL-terminated name and attributes that outlive the call; no template.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &raw const attributes,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    // SAFETY: CreateFileW returns a handle this process now owns, or INVALID_HANDLE_VALUE.
    unsafe { owned_or_invalid(raw) }
}

/// PeekNamedPipe: how many bytes the pipe holds; None when it cannot be asked.
pub(crate) fn bytes_waiting(pipe: &OwnedHandle) -> Option<u32> {
    let mut available = 0u32;
    // SAFETY: the pipe is open; only the count is asked for, into a local.
    let peeked = unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            &raw mut available,
            null_mut(),
        )
    } != 0;
    peeked.then_some(available)
}

/// ReadFile from a pipe into `buffer`; the count read, or None when it fails.
pub(crate) fn read_pipe(pipe: &OwnedHandle, buffer: &mut [u8]) -> Option<u32> {
    let mut got = 0u32;
    // SAFETY: the pipe is open; at most `length_u32(buffer.len())` bytes, which the buffer holds,
    // are written into it; no OVERLAPPED.
    let read = unsafe {
        ReadFile(
            pipe.as_raw_handle(),
            buffer.as_mut_ptr(),
            length_u32(buffer.len()),
            &raw mut got,
            null_mut(),
        )
    } != 0;
    read.then_some(got)
}

// ---- starting a program ----------------------------------------------------------------------

/// SetErrorMode: sets the process's error mode, returns the one before.
pub(crate) fn set_error_mode(mode: u32) -> u32 {
    // SAFETY: SetErrorMode only sets the process's error mode.
    unsafe { SetErrorMode(mode) }
}

/// A process the system started: its process and thread handles, each closed once, when
/// dropped.
#[derive(Debug)]
pub(crate) struct Started {
    pub(crate) process: OwnedHandle,
    #[allow(
        dead_code,
        reason = "Held for its drop: the thread's handle is closed with the process's."
    )]
    thread: OwnedHandle,
}

// The handles of a PROCESS_INFORMATION CreateProcessW filled.
//
// # Safety
// `information` was filled by a CreateProcessW that succeeded: both handles are this process's,
// open, and owned by nothing else.
unsafe fn started(information: PROCESS_INFORMATION) -> Started {
    // SAFETY: the caller's contract.
    unsafe {
        Started {
            process: OwnedHandle::from_raw_handle(information.hProcess),
            thread: OwnedHandle::from_raw_handle(information.hThread),
        }
    }
}

/// The two handles a child inherits, its standard input and its output (standard output and
/// error both), as the attribute list PROC_THREAD_ATTRIBUTE_HANDLE_LIST names them in: only
/// these, not every inheritable handle this process has. It borrows both, so they stay open
/// while it lives, and deletes the list when dropped.
pub(crate) struct InheritedHandles<'h> {
    // The list: its storage, in eight-byte units so that it is aligned as the system's structure
    // is, and the pointer the system is handed, taken once (as_mut_ptr), so that the system's
    // writes go through a pointer no later borrow of the vector invalidates. The vector is not
    // touched again but to be freed; its heap block stays put when the value moves.
    _storage: Vec<u64>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
    // The array the list points at, which must stay put while the list lives.
    handles: Vec<HANDLE>,
    _borrowed: PhantomData<&'h OwnedHandle>,
}

impl<'h> InheritedHandles<'h> {
    /// None for `input` hands the child INVALID_HANDLE_VALUE as its input, as when the null
    /// device would not open. Err with the system's error when the list cannot be made.
    pub(crate) fn new(
        input: Option<&'h OwnedHandle>,
        output: &'h OwnedHandle,
    ) -> Result<InheritedHandles<'h>, u32> {
        let handles = vec![
            input.map_or(INVALID_HANDLE_VALUE, AsRawHandle::as_raw_handle),
            output.as_raw_handle(),
        ];
        let mut bytes = 0usize;
        // SAFETY: the first call only reports the size the list needs.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &raw mut bytes) };
        let mut storage = vec![0u64; bytes.div_ceil(8)];
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = storage.as_mut_ptr().cast();
        // SAFETY: the storage holds at least `bytes` bytes, eight-byte aligned.
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &raw mut bytes) } == 0 {
            return Err(last_error());
        }
        let made = InheritedHandles {
            _storage: storage,
            list,
            handles,
            _borrowed: PhantomData,
        };
        // SAFETY: the list was initialised above (its drop deletes it from here on); the value
        // is the array of the two handles, which lives as long as the list, and both handles are
        // open for 'h.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                made.list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                made.handles.as_ptr().cast(),
                size_of_val(made.handles.as_slice()),
                null_mut(),
                null(),
            )
        } != 0;
        if !updated {
            return Err(last_error());
        }
        Ok(made)
    }
}

impl Drop for InheritedHandles<'_> {
    fn drop(&mut self) {
        // SAFETY: the list was initialised (new() returns a value only then) and is deleted once,
        // here; its storage is freed after.
        unsafe { DeleteProcThreadAttributeList(self.list) };
    }
}

/// CreateProcessW of `program` with `command` as its command line (which the call may write
/// into), every handle in `inherited` inherited and no other, input and output as `inherited`
/// says, in `directory` (None: this process's), with `flags` and EXTENDED_STARTUPINFO_PRESENT.
/// Err with the system's error, read at once.
pub(crate) fn start_inheriting(
    program: &WideName,
    command: &mut WideName,
    directory: Option<&WideName>,
    flags: u32,
    inherited: &InheritedHandles<'_>,
) -> Result<Started, u32> {
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = length_u32(size_of::<STARTUPINFOEXW>());
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = inherited.handles[0];
    startup.StartupInfo.hStdOutput = inherited.handles[1];
    startup.StartupInfo.hStdError = inherited.handles[1];
    startup.lpAttributeList = inherited.list;
    let mut information = PROCESS_INFORMATION::default();
    // SAFETY: every pointer is to a NUL-terminated wide string (WideName) or a structure that
    // outlives the call; the command line is a mutable buffer of the caller's, which the call
    // writes within; the attribute list is initialised and names two handles open for the call.
    let created = unsafe {
        CreateProcessW(
            program.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            flags | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            directory.map_or(null(), WideName::as_ptr),
            &raw const startup.StartupInfo,
            &raw mut information,
        )
    } != 0;
    if !created {
        return Err(last_error());
    }
    // SAFETY: CreateProcessW succeeded and filled `information`.
    Ok(unsafe { started(information) })
}

/// CreateProcessW of `program` with `command`, inheriting nothing, no standard handles given, in
/// `directory` (None: this process's), with `flags`. Err with the system's error, read at once.
pub(crate) fn start_alone(
    program: &WideName,
    command: &mut WideName,
    directory: Option<&WideName>,
    flags: u32,
) -> Result<Started, u32> {
    let startup = STARTUPINFOW {
        cb: length_u32(size_of::<STARTUPINFOW>()),
        ..Default::default()
    };
    let mut information = PROCESS_INFORMATION::default();
    // SAFETY: as in start_inheriting, with no attribute list and no handle inherited.
    let created = unsafe {
        CreateProcessW(
            program.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            flags,
            null(),
            directory.map_or(null(), WideName::as_ptr),
            &raw const startup,
            &raw mut information,
        )
    } != 0;
    if !created {
        return Err(last_error());
    }
    // SAFETY: CreateProcessW succeeded and filled `information`.
    Ok(unsafe { started(information) })
}

// ---- this process ----------------------------------------------------------------------------

/// TokenElevationType of this process's token, as the TOKEN_ELEVATION_TYPE number; None when the
/// token cannot be opened or asked, or answers in another size.
pub(crate) fn token_elevation_type() -> Option<i32> {
    let mut opened: HANDLE = null_mut();
    // SAFETY: the pseudo-handle of this process, which needs no closing, and a place for the
    // token handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut opened) } == 0 {
        return None;
    }
    // SAFETY: OpenProcessToken succeeded: the token handle is this process's, open, owned here.
    let token = unsafe { OwnedHandle::from_raw_handle(opened) };
    let mut kind: i32 = 0;
    let mut written = 0u32;
    // SAFETY: the token is open with TOKEN_QUERY; the answer is a TOKEN_ELEVATION_TYPE, four
    // bytes, written into `kind`, which is that size.
    let asked = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TOKEN_ELEVATION_TYPE_CLASS,
            (&raw mut kind).cast(),
            length_u32(size_of::<i32>()),
            &raw mut written,
        )
    } != 0;
    (asked && written as usize == size_of::<i32>()).then_some(kind)
}

/// This process's command line split by CommandLineToArgvW, each argument's UTF-16 units; None
/// when the call fails (it allocates, so out of memory).
pub(crate) fn command_line_arguments() -> Option<Vec<Vec<u16>>> {
    let mut count = 0i32;
    // SAFETY: GetCommandLineW returns this process's command line, which lives as long as the
    // process; CommandLineToArgvW allocates the array it returns, freed below.
    let array = unsafe { CommandLineToArgvW(GetCommandLineW(), &raw mut count) };
    if array.is_null() {
        return None;
    }
    let count = usize::try_from(count).unwrap_or(0);
    let mut arguments = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: the array holds `count` pointers to NUL-terminated wide strings, which live
        // until the LocalFree below; each is copied before then.
        let units = unsafe {
            let argument = *array.add(i);
            let length = (0..).take_while(|&at| *argument.add(at) != 0).count();
            std::slice::from_raw_parts(argument, length).to_vec()
        };
        arguments.push(units);
    }
    // SAFETY: the array was allocated by CommandLineToArgvW, freed once with LocalFree, and not
    // read again.
    unsafe { LocalFree(array.cast()) };
    Some(arguments)
}

// ---- the scan --------------------------------------------------------------------------------

/// OpenProcess of `pid` with `access`, not inheritable; None when it fails (the error is
/// `last_error()`'s).
pub(crate) fn open_process(access: u32, pid: u32) -> Option<OwnedHandle> {
    // SAFETY: integer arguments only.
    let raw = unsafe { OpenProcess(access, 0, pid) };
    // SAFETY: OpenProcess returns a handle this process now owns, or NULL.
    unsafe { owned_or_null(raw) }
}

/// QueryFullProcessImageNameW (Win32 form) into `buffer`: the units written, or None when it
/// fails.
pub(crate) fn image_name(process: &OwnedHandle, buffer: &mut [u16]) -> Option<usize> {
    let mut length = length_u32(buffer.len());
    // SAFETY: the handle is open; `buffer` holds `length` units, the call writes at most that
    // many and says how many it wrote.
    let named = unsafe {
        QueryFullProcessImageNameW(
            process.as_raw_handle(),
            0,
            buffer.as_mut_ptr(),
            &raw mut length,
        )
    } != 0;
    named.then_some((length as usize).min(buffer.len()))
}

/// CreateToolhelp32Snapshot of `flags`, the whole machine's; Err with the system's error.
pub(crate) fn snapshot(flags: u32) -> Result<OwnedHandle, u32> {
    // SAFETY: two integer arguments.
    let raw = unsafe { CreateToolhelp32Snapshot(flags, 0) };
    // SAFETY: CreateToolhelp32Snapshot returns a handle this process now owns, or
    // INVALID_HANDLE_VALUE.
    match unsafe { owned_or_invalid(raw) } {
        Some(snapshot) => Ok(snapshot),
        None => Err(last_error()),
    }
}

/// A PROCESSENTRY32W with its size set, as Process32FirstW asks.
pub(crate) fn process_entry() -> PROCESSENTRY32W {
    PROCESSENTRY32W {
        dwSize: length_u32(size_of::<PROCESSENTRY32W>()),
        ..PROCESSENTRY32W::default()
    }
}

/// Process32FirstW into `entry` (made by process_entry): false when it fails.
pub(crate) fn first_process(snapshot: &OwnedHandle, entry: &mut PROCESSENTRY32W) -> bool {
    entry.dwSize = length_u32(size_of::<PROCESSENTRY32W>());
    // SAFETY: the snapshot is open and `entry` is a PROCESSENTRY32W whose size is set.
    unsafe { Process32FirstW(snapshot.as_raw_handle(), entry) != 0 }
}

/// Process32NextW into `entry`: false at the end, or when it fails.
pub(crate) fn next_process(snapshot: &OwnedHandle, entry: &mut PROCESSENTRY32W) -> bool {
    entry.dwSize = length_u32(size_of::<PROCESSENTRY32W>());
    // SAFETY: as for first_process.
    unsafe { Process32NextW(snapshot.as_raw_handle(), entry) != 0 }
}

/// CompareStringOrdinal of the whole of `a` and `b`, case folded as the file system folds it
/// (bIgnoreCase): CSTR_EQUAL (2) when they match; 0 for a slice too long to count.
pub(crate) fn compare_ordinal_ignoring_case(a: &[u16], b: &[u16]) -> i32 {
    let (Ok(a_length), Ok(b_length)) = (i32::try_from(a.len()), i32::try_from(b.len())) else {
        return 0;
    };
    // SAFETY: both pointers name that many units of live slices; the counts are given (never
    // negative, which would ask for a terminator), so no terminator is read.
    unsafe { CompareStringOrdinal(a.as_ptr(), a_length, b.as_ptr(), b_length, 1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // winnls.h.
    const CSTR_EQUAL: i32 = 2;
    // winbase.h.
    const WAIT_OBJECT_0: u32 = 0;

    #[test]
    fn ordinal_comparison_folds_case_and_counts_every_unit() {
        let lower: Vec<u16> = "c:\\katana".encode_utf16().collect();
        let upper: Vec<u16> = "C:\\KATANA".encode_utf16().collect();
        let other: Vec<u16> = "C:\\KATANB".encode_utf16().collect();
        assert_eq!(compare_ordinal_ignoring_case(&lower, &upper), CSTR_EQUAL);
        assert_ne!(compare_ordinal_ignoring_case(&lower, &other), CSTR_EQUAL);
        assert_ne!(compare_ordinal_ignoring_case(&lower, &upper[..5]), CSTR_EQUAL);
    }

    #[test]
    fn a_child_writes_into_the_inherited_pipe_and_its_exit_code_is_read() {
        // cmd.exe from the system folder, as the launcher's own tests use it.
        let system = std::env::var_os("SystemRoot").expect("Windows names its folder");
        let program = std::path::Path::new(&system).join("System32").join("cmd.exe");
        let program = {
            use std::os::windows::ffi::OsStrExt;
            WideName::from_units(program.as_os_str().encode_wide().collect())
        };
        let mut command = WideName::from_text("cmd.exe /c echo here& exit 7");
        let (reader, writer) = output_pipe().expect("a pipe");
        let input = null_device_for_reading();
        let inherited = InheritedHandles::new(input.as_ref(), &writer).expect("the list");
        // CREATE_NO_WINDOW.
        let child = start_inheriting(&program, &mut command, None, 0x0800_0000, &inherited)
            .expect("cmd.exe starts");
        drop(inherited);
        drop(writer);
        assert_eq!(wait(&child.process, 30_000), WAIT_OBJECT_0);
        let mut output = Vec::new();
        let mut buffer = [0u8; 256];
        while let Some(waiting) = bytes_waiting(&reader) {
            if waiting == 0 {
                break;
            }
            match read_pipe(&reader, &mut buffer) {
                Some(got) if got > 0 => output.extend_from_slice(&buffer[..got as usize]),
                _ => break,
            }
        }
        assert!(output.starts_with(b"here"), "{output:?}");
        assert_eq!(exit_code(&child.process), Some(7));
    }

    #[test]
    fn this_process_is_in_a_snapshot_of_the_machine() {
        let snapshot = snapshot(0x2).expect("TH32CS_SNAPPROCESS");
        let mut entry = process_entry();
        let mut seen = first_process(&snapshot, &mut entry);
        let mut found = false;
        while seen {
            found |= entry.th32ProcessID == std::process::id();
            seen = next_process(&snapshot, &mut entry);
        }
        assert!(found);
    }
}
