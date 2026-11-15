//! The Windows process scan of the process module: who is running, whether a process id is
//! alive, and which processes have their program under the install root (the C++'s
//! src/katana_updater/process_scan_win.cpp, which the update library compiles on Windows only,
//! because katana, katana_cli and katana_mcp ask the same question to say `blocked`, and
//! running_programs() is part of the engine's API).
//!
//! WHAT "HOLDS" THE INSTALL. A program of the old version that stayed running through a swap would
//! go on as old code among new files, and its own file can be neither replaced nor deleted while
//! it runs; so the question is "which processes were started from a file under R". Toolhelp lists
//! every process of the machine; QueryFullProcessImageNameW names the file each was started from.
//! The modules a process has loaded are not asked: a DLL that is loaded can be renamed (it is
//! opened with FILE_SHARE_DELETE), which is all the commit does, and listing the modules of every
//! process fails on exactly the protected ones. A process that has ended is not a holder, whoever
//! still has a handle to it.
//!
//! WHAT THE SCAN CANNOT SEE, AND WHAT THEN HAPPENS. A process this user may not open - another
//! account's, which is also what a Katana started "as administrator" with another account's
//! password is - gives no image path, and is passed over: it is NOT taken for a holder. Nothing
//! later stops the swap on its account either. By every account the design found (its section 12)
//! Windows lets the file of a running program be renamed and refuses only to delete or overwrite
//! it, and renames are all the commit does; so the update goes through under such a program, which
//! runs on as the old code among the new version's files, its own file now in R/.update/<tx>/old,
//! where the clean-up cannot delete it yet and lists it for the next start. That is the residual,
//! said here rather than hidden: the per-user install (design, D1) is a folder of the person's own
//! profile, which no other account runs a program from, so it takes an archive unpacked where
//! several accounts can reach it, and one of them running Katana from it at that moment; the one
//! thing known of such a process is its file's name (PROCESSENTRY32W::szExeFile), and a name does
//! not say which install it was started from, so to hold the update on "somebody runs a
//! katana.exe" would block it for as long as any other account runs any Katana of its own, on a
//! machine people share; and no elevation is asked for to see more: a per-user install is updated
//! by the user. A snapshot that cannot be taken or read at all is another matter: that is an
//! error, never "no holders" (apply() then refuses with "scan_failed" and touches nothing).
//!
//! PATHS ARE COMPARED AS TEXT, so both sides are first put in one form. The root can be spelled in
//! many ways that name one folder - upper or lower case, "/" for "\", a trailing separator, "."
//! and ".." segments, the \\?\ prefix, 8.3 short names, a junction - and the kernel reports an
//! image's path as it opened the file. normalised() settles the first five lexically; the short
//! names and links are settled by asking the file system for the root's long and final names, and
//! the comparison then tries every spelling of the root. Case is folded by CompareStringOrdinal,
//! which uses the file system's own (NTFS) upper-case table and not just ASCII.
//!
//! A process id can be reused. is_alive() asks about the id, and the helper asks it for the id of
//! the window that started it, a few seconds after that window began to close: a recycled id
//! inside that interval is possible in principle and would only make the helper wait for the wrong
//! process until the 60 s limit and then report "Katana is still running"; the person presses
//! Retry. The trait has no process creation time to tell the two apart.

#![allow(
    unsafe_code,
    reason = "Toolhelp, the process queries and the path calls are Win32 foreign functions."
)]

use crate::process::{ProcessOps, RunningProgram};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Globalization::CompareStringOrdinal;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFinalPathNameByHandleW, GetFullPathNameW, GetLongPathNameW,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW,
    WaitForSingleObject,
};

// The values of the Windows SDK's headers (winerror.h, winnt.h, winbase.h, fileapi.h,
// tlhelp32.h, stringapiset.h).
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_BAD_LENGTH: u32 = 24;
const MAX_PATH: usize = 260;
const TH32CS_SNAPPROCESS: u32 = 0x2;
const SYNCHRONIZE: u32 = 0x0010_0000;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const STILL_ACTIVE: u32 = 259;
const WAIT_TIMEOUT: u32 = 258;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_NAME_NORMALIZED: u32 = 0x0;
const VOLUME_NAME_DOS: u32 = 0x0;
const CSTR_EQUAL: i32 = 2;
const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const COLON: u16 = b':' as u16;
const DOT: u16 = b'.' as u16;
const TILDE: u16 = b'~' as u16;

struct Handle(HANDLE);

impl Handle {
    fn valid(&self) -> bool {
        !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.valid() {
            // SAFETY: the handle is one this Handle owns, closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn terminated(text: &[u16]) -> Vec<u16> {
    let mut out = text.to_vec();
    out.push(0);
    out
}

// A path in one form: backslashes only, no \\?\ or \\.\ prefix, no "." or empty segment, ".."
// folded in (never above the drive or the server and share), no trailing separator - except on a
// drive root, which keeps its one. Pure text; nothing is asked of the file system.
fn normalised(text: &[u16]) -> Vec<u16> {
    let mut text: Vec<u16> = text
        .iter()
        .map(|&unit| if unit == SLASH { BACKSLASH } else { unit })
        .collect();
    let extended_unc = wide(r"\\?\UNC\");
    let extended = wide(r"\\?\");
    let device = wide(r"\\.\");
    if text.starts_with(&extended_unc) {
        let mut rest = wide(r"\\");
        rest.extend_from_slice(&text[extended_unc.len()..]);
        text = rest;
    } else if text.starts_with(&extended) || text.starts_with(&device) {
        text.drain(..4);
    }

    let mut anchor: Vec<u16> = Vec::new();
    let mut position = 0usize;
    let mut fixed_segments = 0usize; // the server and the share of a UNC path are not folded away
    let mut rooted = false;
    if text.starts_with(&[BACKSLASH, BACKSLASH]) {
        anchor = vec![BACKSLASH, BACKSLASH];
        position = 2;
        fixed_segments = 2;
        rooted = true;
    } else if text.len() >= 2 && text[1] == COLON {
        anchor = text[..2].to_vec();
        position = 2;
        rooted = position < text.len() && text[position] == BACKSLASH;
    } else if text.first() == Some(&BACKSLASH) {
        rooted = true;
    }

    let mut segments: Vec<Vec<u16>> = Vec::new();
    while position <= text.len() {
        let end = text[position..]
            .iter()
            .position(|&unit| unit == BACKSLASH)
            .map_or(text.len(), |offset| position + offset);
        let segment = &text[position..end];
        if segment == [DOT, DOT] {
            if segments.len() > fixed_segments {
                segments.pop();
            }
        } else if !segment.is_empty() && segment != [DOT] {
            segments.push(segment.to_vec());
        }
        position = end + 1;
    }

    let mut out = anchor.clone();
    if rooted && anchor != [BACKSLASH, BACKSLASH] {
        out.push(BACKSLASH);
    }
    for (i, segment) in segments.iter().enumerate() {
        if i > 0 {
            out.push(BACKSLASH);
        }
        out.extend_from_slice(segment);
    }
    out
}

// GetLongPathNameW of an existing path (8.3 short names written out); the input when it fails (the
// path is not there, or the volume has no short names to expand).
fn long_name(path: &[u16]) -> Vec<u16> {
    let input = terminated(path);
    let mut buffer = vec![0u16; path.len() + 64];
    for _ in 0..3 {
        // SAFETY: `input` is NUL-terminated and `buffer` holds as many units as the call is told.
        let length =
            unsafe { GetLongPathNameW(input.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) }
                as usize;
        if length == 0 {
            return path.to_vec();
        }
        if length < buffer.len() {
            buffer.truncate(length);
            return buffer;
        }
        buffer.resize(length + 1, 0);
    }
    path.to_vec()
}

// The name the file system gives the folder after every link is followed (junctions, symbolic
// links, subst drives); empty when it cannot be opened.
fn final_name(path: &[u16]) -> Vec<u16> {
    let input = terminated(path);
    // SAFETY: `input` is NUL-terminated; no security attributes and no template are given.
    let folder = Handle(unsafe {
        CreateFileW(
            input.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    });
    if !folder.valid() {
        return Vec::new();
    }
    let mut buffer = vec![0u16; 512];
    for _ in 0..3 {
        // SAFETY: the handle is open, and `buffer` holds as many units as the call is told.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                folder.0,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if length == 0 {
            return Vec::new();
        }
        if length < buffer.len() {
            buffer.truncate(length);
            return buffer;
        }
        buffer.resize(length + 1, 0);
    }
    Vec::new()
}

// True for a path that names a place without help from the current directory: a drive and a
// separator, or a UNC or extended-length prefix. std::filesystem does not call all of those
// absolute (the \\?\ form it reads as having no root directory), and then makes a mess of them.
fn names_a_place(text: &[u16]) -> bool {
    if text.len() >= 3 && text[1] == COLON && (text[2] == BACKSLASH || text[2] == SLASH) {
        return true;
    }
    text.starts_with(&[BACKSLASH, BACKSLASH]) || text.starts_with(&[SLASH, SLASH])
}

// The root as absolute text: as it is, or made absolute against the current directory.
fn absolute_text(root: &Path) -> Vec<u16> {
    let text: Vec<u16> = root.as_os_str().encode_wide().collect();
    if names_a_place(&text) {
        return text;
    }
    let input = terminated(&text);
    let mut buffer = vec![0u16; MAX_PATH];
    for _ in 0..3 {
        // SAFETY: `input` is NUL-terminated, `buffer` holds as many units as the call is told, and
        // the file-part pointer may be null.
        let length = unsafe {
            GetFullPathNameW(
                input.as_ptr(),
                buffer.len() as u32,
                buffer.as_mut_ptr(),
                null_mut(),
            )
        } as usize;
        if length == 0 {
            return text;
        }
        if length < buffer.len() {
            buffer.truncate(length);
            return buffer;
        }
        buffer.resize(length + 1, 0);
    }
    text
}

// Every spelling of the root that the comparison tries, without repeats.
fn root_spellings(root: &Path) -> Vec<Vec<u16>> {
    let mut spellings: Vec<Vec<u16>> = Vec::new();
    let mut add = |spelling: Vec<u16>| {
        if !spelling.is_empty() && !spellings.contains(&spelling) {
            spellings.push(spelling);
        }
    };
    let plain = normalised(&absolute_text(root));
    add(plain.clone());
    add(normalised(&long_name(&plain)));
    add(normalised(&final_name(&plain)));
    spellings
}

// True when `path` is the folder `root` names or something inside it. The character after the
// root must be a separator (or the root must end in one), or "C:\Katana2" would be "under"
// "C:\Katana".
fn is_under(root: &[u16], path: &[u16]) -> bool {
    if root.is_empty() || path.len() <= root.len() {
        return false;
    }
    // SAFETY: both pointers name `root.len()` units of live slices; the counts are given, so no
    // terminator is read.
    let compared = unsafe {
        CompareStringOrdinal(
            path.as_ptr(),
            root.len() as i32,
            root.as_ptr(),
            root.len() as i32,
            1,
        )
    };
    if compared != CSTR_EQUAL {
        return false;
    }
    root.last() == Some(&BACKSLASH) || path[root.len()] == BACKSLASH
}

// A Toolhelp snapshot of the processes, or the error when it cannot be taken.
fn take_snapshot() -> std::result::Result<Handle, u32> {
    let mut error = 0u32;
    for _ in 0..5 {
        // SAFETY: two integer arguments; the handle returned is owned by the Handle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot != INVALID_HANDLE_VALUE {
            return Ok(Handle(snapshot));
        }
        // SAFETY: reads this thread's last error.
        error = unsafe { GetLastError() };
        if error != ERROR_BAD_LENGTH {
            // ERROR_BAD_LENGTH is the one transient failure the API documents.
            break;
        }
    }
    Err(error)
}

fn scan_failed(error: u32) -> Error {
    make_error(
        ErrorCode::Internal,
        "the running programs cannot be listed",
        format!("os_error={error}"),
    )
}

struct WindowsProcessOps;

impl ProcessOps for WindowsProcessOps {
    fn current_pid(&self) -> u64 {
        // SAFETY: no arguments.
        u64::from(unsafe { GetCurrentProcessId() })
    }

    fn is_alive(&self, pid: u64) -> bool {
        let Ok(pid) = u32::try_from(pid) else {
            return false;
        };
        if pid == 0 {
            return false;
        }
        // SAFETY: integer arguments; the handle returned is owned by the Handle.
        let process =
            Handle(unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
        if !process.valid() {
            // No such process is "not alive"; a process we may not open is one that exists.
            // SAFETY: reads this thread's last error.
            return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
        }
        // SAFETY: the handle is open, with the right to wait on it.
        let waited = unsafe { WaitForSingleObject(process.0, 0) };
        waited == WAIT_TIMEOUT
    }

    fn holders_under(&self, root: &Path) -> Result<Vec<RunningProgram>> {
        if root.as_os_str().is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "no install folder to look for programs under",
            ));
        }
        let spellings = root_spellings(root);

        let snapshot = take_snapshot().map_err(scan_failed)?;

        // SAFETY: no arguments.
        let this = unsafe { GetCurrentProcessId() };
        let mut holders: Vec<RunningProgram> = Vec::new();
        let mut image = vec![0u16; 32768]; // the longest path Windows has
        // SAFETY: PROCESSENTRY32W is plain data; all zeros is a valid value, and dwSize is set.
        let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: the snapshot is open and `entry` is a PROCESSENTRY32W whose size is set.
        if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
            // A snapshot holds this process at the least, so one with no first entry was not
            // read: an error, and never the answer "nobody is running" (see the top).
            // SAFETY: reads this thread's last error.
            return Err(scan_failed(unsafe { GetLastError() }));
        }
        loop {
            let id = entry.th32ProcessID;
            // This process is never its own holder; 0 is the idle process.
            if id != this && id != 0 {
                // SAFETY: integer arguments; the handle returned is owned by the Handle.
                let process =
                    Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id) });
                // Not valid: exited since the snapshot, or not ours to open (see the top).
                if process.valid() {
                    // A process that has ended holds nothing, though its id and its image name stay
                    // readable for as long as anybody keeps a handle to it (the program that
                    // started it and has not yet collected its exit code, say). An exit code of 259
                    // itself reads as "still running", which only errs towards naming a holder.
                    let mut exit_code = 0u32;
                    // SAFETY: the handle is open with the right to query; the out-pointer is ours.
                    let ended = unsafe { GetExitCodeProcess(process.0, &mut exit_code) } != 0
                        && exit_code != STILL_ACTIVE;
                    if !ended {
                        let mut length = image.len() as u32;
                        // SAFETY: `image` holds `length` units; the call writes at most that many
                        // and says how many it wrote.
                        let named = unsafe {
                            QueryFullProcessImageNameW(
                                process.0,
                                0,
                                image.as_mut_ptr(),
                                &mut length,
                            )
                        } != 0;
                        if named && length != 0 {
                            let mut path = normalised(&image[..length as usize]);
                            let mut under =
                                spellings.iter().any(|spelling| is_under(spelling, &path));
                            if !under && path.contains(&TILDE) {
                                // An 8.3 name in the image's path, against a root spelled long:
                                // expand it.
                                let expanded = normalised(&long_name(&path));
                                under = spellings
                                    .iter()
                                    .any(|spelling| is_under(spelling, &expanded));
                                if under {
                                    path = expanded;
                                }
                            }
                            if under {
                                holders.push(RunningProgram {
                                    pid: u64::from(id),
                                    image: PathBuf::from(OsString::from_wide(&path)),
                                });
                            }
                        }
                    }
                }
            }
            // SAFETY: as for Process32FirstW.
            if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
                break;
            }
        }
        holders.sort_by_key(|holder| holder.pid);
        Ok(holders)
    }
}

pub(crate) fn make_system_process_ops() -> Box<dyn ProcessOps> {
    Box::new(WindowsProcessOps)
}
