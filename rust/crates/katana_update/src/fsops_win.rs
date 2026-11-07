//! The real file system of the fsops module, for Windows. Compiled on Windows only.
//!
//! What matters here, and why:
//! - A rename is MoveFileExW with NO flag. MOVEFILE_REPLACE_EXISTING is absent on purpose (the
//!   apply never replaces by accident), MOVEFILE_COPY_ALLOWED is absent (a rename is atomic or it
//!   fails; a cross-volume copy-and-delete is neither), and MOVEFILE_DELAY_UNTIL_REBOOT is NEVER
//!   used: it completes at a reboot, by a different account, with no way to roll it back.
//! - Errors 5, 32 and 33 (access denied, sharing and lock violations) are what an antivirus
//!   scanner holding a file for a moment causes, as much as a program that has it open, so a
//!   rename retries them every RetryPolicy::interval for up to RetryPolicy::total.
//! - A file the update will read back is flushed (FlushFileBuffers) before the call returns.
//! - Directories are opened with FILE_FLAG_BACKUP_SEMANTICS, the only way to get a handle to one.
//! - A path is given the \\?\ prefix when it is long (or ends in a dot or a space, which the
//!   ordinary form would silently drop), so an install under a deep folder works without the
//!   long-path manifest setting.
//! - "Reparse point" here means one that sends the path somewhere else (a symbolic link, a
//!   junction, a mount point: the reparse tags whose "name surrogate" bit is set), which is what a
//!   write through must be refused for. A OneDrive placeholder or a deduplicated file has a
//!   reparse point too, but it is the file itself; refusing it would refuse an install in a
//!   synchronised folder.

#![allow(
    unsafe_code,
    reason = "The file system is reached through its Win32 calls, which are foreign functions."
)]

use crate::fsops::{
    EntryInfo, EntryKind, FileLock, FsOps, RETRYABLE_OS_ERRORS, RetryPolicy, is_link_reparse_tag,
    path_text,
};
use crate::long_path::{absolute_normal, wide_path};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use std::ffi::OsString;
use std::mem::zeroed;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, DeleteFileW, FindClose, FindFirstFileW, FindNextFileW,
    FlushFileBuffers, GetDiskFreeSpaceExW, GetFileAttributesExW, GetFileAttributesW, GetFileSizeEx,
    GetFileTime, GetFinalPathNameByHandleW, MoveFileExW, ReadFile, RemoveDirectoryW,
    SetFileAttributesW, SetFileTime, WIN32_FILE_ATTRIBUTE_DATA, WIN32_FIND_DATAW, WriteFile,
};

// The values of the Windows SDK's headers (winerror.h, winnt.h, fileapi.h, winbase.h).
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NO_MORE_FILES: u32 = 18;
const ERROR_SHARING_VIOLATION: u32 = 32;
const ERROR_LOCK_VIOLATION: u32 = 33;
const ERROR_FILE_EXISTS: u32 = 80;
const ERROR_INVALID_NAME: u32 = 123;
const ERROR_DIR_NOT_EMPTY: u32 = 145;
const ERROR_BAD_PATHNAME: u32 = 161;
const ERROR_ALREADY_EXISTS: u32 = 183;
const ERROR_DIRECTORY: u32 = 267;
const ERROR_NOT_A_REPARSE_POINT: u32 = 4390;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_APPEND_DATA: u32 = 0x4;
const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const CREATE_NEW: u32 = 1;
const CREATE_ALWAYS: u32 = 2;
const OPEN_EXISTING: u32 = 3;
const OPEN_ALWAYS: u32 = 4;
const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;
const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
const FILE_NAME_NORMALIZED: u32 = 0x0;
const VOLUME_NAME_DOS: u32 = 0x0;
const GET_FILE_EX_INFO_STANDARD: i32 = 0; // GetFileExInfoStandard

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
// Past this a file is not a record or a manifest, and read_file would hold it all in memory.
const READ_FILE_LIMIT: u64 = 256 * 1024 * 1024;

const BACKSLASH: u16 = b'\\' as u16;
const DOT: u16 = b'.' as u16;

fn last_error() -> u32 {
    // SAFETY: GetLastError only reads this thread's last-error value.
    unsafe { GetLastError() }
}

fn os_error(code: ErrorCode, message: &str, error: u32, path: &Path) -> Error {
    make_error(
        code,
        message,
        format!("os_error={error} {}", path_text(path)),
    )
}

fn is_missing_error(error: u32) -> bool {
    matches!(
        error,
        ERROR_FILE_NOT_FOUND
            | ERROR_PATH_NOT_FOUND
            | ERROR_INVALID_NAME
            | ERROR_BAD_PATHNAME
            | ERROR_DIRECTORY
            | ERROR_NOT_A_REPARSE_POINT
    )
}

fn is_retryable(error: u32) -> bool {
    i32::try_from(error).is_ok_and(|error| RETRYABLE_OS_ERRORS.contains(&error))
}

// The one definition of how a path is handed to the system (long_path.rs), with the terminator
// the calls need.
fn wide(path: &Path) -> Vec<u16> {
    let mut text = wide_path(path);
    text.push(0);
    text
}

// The parent folder, or None at the top (std::filesystem's parent_path is the path itself there).
fn parent_of(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty() && *parent != path)
        .map(Path::to_path_buf)
}

// When a retry loop gives up: None for a total too long to add to now, which is never.
fn deadline_after(total: Duration) -> Option<Instant> {
    Instant::now().checked_add(total)
}

// How long to wait before the next attempt, or None when the time is up.
fn next_wait(deadline: Option<Instant>, interval: Duration) -> Option<Duration> {
    let now = Instant::now();
    match deadline {
        Some(deadline) if now >= deadline => None,
        Some(deadline) => Some(interval.min(deadline - now)),
        None => Some(interval),
    }
}

// A handle that closes itself.
struct Handle(HANDLE);

impl Handle {
    fn valid(&self) -> bool {
        self.0 != INVALID_HANDLE_VALUE
    }

    fn get(&self) -> HANDLE {
        self.0
    }

    fn reset(&mut self) {
        if self.valid() {
            // SAFETY: the handle is one this Handle owns, closed once.
            unsafe { CloseHandle(self.0) };
            self.0 = INVALID_HANDLE_VALUE;
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.reset();
    }
}

fn create_file(path: &[u16], access: u32, share: u32, disposition: u32, flags: u32) -> Handle {
    // SAFETY: `path` is NUL-terminated; no security attributes and no template.
    Handle(unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            share,
            null(),
            disposition,
            flags,
            null_mut(),
        )
    })
}

fn attributes_of(path: &[u16]) -> u32 {
    // SAFETY: `path` is NUL-terminated.
    unsafe { GetFileAttributesW(path.as_ptr()) }
}

#[allow(
    dead_code,
    reason = "The handle is held for its Drop: while it is open, the lock is held."
)]
struct WindowsLock(Handle);

impl FileLock for WindowsLock {}

// Whether an entry whose attributes say it has a reparse point is a LINK. A failed lookup counts
// as a redirecting one: the safe answer for a caller that refuses to write through it.
fn redirects(path: &[u16]) -> bool {
    // SAFETY: plain data, filled by the call.
    let mut found: WIN32_FIND_DATAW = unsafe { zeroed() };
    // SAFETY: `path` is NUL-terminated; the search handle is closed at once.
    let search = unsafe { FindFirstFileW(path.as_ptr(), &raw mut found) };
    if search == INVALID_HANDLE_VALUE {
        return true;
    }
    // SAFETY: as above.
    unsafe { FindClose(search) };
    is_link_reparse_tag(found.dwReserved0)
}

fn write_all(file: HANDLE, bytes: &[u8], path: &Path) -> Status {
    let mut done = 0usize;
    while done < bytes.len() {
        let chunk = (bytes.len() - done).min(1 << 20) as u32;
        let mut written = 0u32;
        // SAFETY: at most `chunk` bytes of `bytes` are read from.
        let wrote = unsafe {
            WriteFile(
                file,
                bytes[done..].as_ptr().cast(),
                chunk,
                &raw mut written,
                null_mut(),
            )
        } != 0;
        if !wrote {
            return Err(os_error(
                ErrorCode::FileExportFailure,
                "the file could not be written",
                last_error(),
                path,
            ));
        }
        done += written as usize;
    }
    Ok(())
}

fn flush(file: &Handle) -> bool {
    // SAFETY: the handle is open.
    unsafe { FlushFileBuffers(file.get()) != 0 }
}

struct WindowsFsOps {
    retry: RetryPolicy,
}

impl FsOps for WindowsFsOps {
    // ---- looking -------------------------------------------------------------------------------

    fn stat(&self, path: &Path) -> Result<EntryInfo> {
        let wide = wide(path);
        // SAFETY: plain data, filled by the call.
        let mut data: WIN32_FILE_ATTRIBUTE_DATA = unsafe { zeroed() };
        // SAFETY: `wide` is NUL-terminated and `data` is the structure the level names.
        let examined = unsafe {
            GetFileAttributesExW(
                wide.as_ptr(),
                GET_FILE_EX_INFO_STANDARD,
                (&raw mut data).cast(),
            )
        } != 0;
        if !examined {
            let error = last_error();
            if is_missing_error(error) {
                return Ok(EntryInfo::default());
            }
            return Err(os_error(
                ErrorCode::FileImportFailure,
                "the entry cannot be examined",
                error,
                path,
            ));
        }
        Ok(EntryInfo {
            kind: if data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            reparse_point: data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                && redirects(&wide),
            size: (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow),
        })
    }

    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        let mut pattern = wide_path(directory);
        if pattern.last() != Some(&BACKSLASH) {
            pattern.push(BACKSLASH);
        }
        pattern.push(u16::from(b'*'));
        pattern.push(0);
        // SAFETY: plain data, filled by the call.
        let mut found: WIN32_FIND_DATAW = unsafe { zeroed() };
        // SAFETY: `pattern` is NUL-terminated.
        let search = unsafe { FindFirstFileW(pattern.as_ptr(), &raw mut found) };
        if search == INVALID_HANDLE_VALUE {
            let error = last_error();
            return Err(os_error(
                if is_missing_error(error) {
                    ErrorCode::NotFound
                } else {
                    ErrorCode::FileImportFailure
                },
                "the directory cannot be listed",
                error,
                directory,
            ));
        }
        let mut names = Vec::new();
        loop {
            let length = found
                .cFileName
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(found.cFileName.len());
            match &found.cFileName[..length] {
                [DOT] | [DOT, DOT] => {}
                name => names.push(directory.join(OsString::from_wide(name))),
            }
            // SAFETY: the search handle is open and `found` is its structure.
            if unsafe { FindNextFileW(search, &raw mut found) } == 0 {
                break;
            }
        }
        let last = last_error();
        // SAFETY: the search handle is open, closed once.
        unsafe { FindClose(search) };
        if last != ERROR_NO_MORE_FILES {
            return Err(os_error(
                ErrorCode::FileImportFailure,
                "the directory cannot be listed",
                last,
                directory,
            ));
        }
        Ok(names)
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let file = create_file(
            &wide(path),
            GENERIC_READ,
            SHARE_ALL,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
        );
        if !file.valid() {
            let error = last_error();
            return Err(os_error(
                if is_missing_error(error) {
                    ErrorCode::NotFound
                } else {
                    ErrorCode::FileImportFailure
                },
                "the file cannot be opened",
                error,
                path,
            ));
        }
        let mut size = 0i64;
        // SAFETY: the handle is open; the size is written by the call.
        if unsafe { GetFileSizeEx(file.get(), &raw mut size) } == 0 || size < 0 {
            return Err(os_error(
                ErrorCode::FileImportFailure,
                "the file cannot be sized",
                last_error(),
                path,
            ));
        }
        if size as u64 > READ_FILE_LIMIT {
            return Err(os_error(
                ErrorCode::FileImportFailure,
                "the file is too large to read",
                0,
                path,
            ));
        }
        let mut bytes = vec![0u8; size as usize];
        let mut done = 0usize;
        while done < bytes.len() {
            let want = (bytes.len() - done).min(1 << 20) as u32;
            let mut got = 0u32;
            // SAFETY: at most `want` bytes are written, which fit after `done`.
            let read = unsafe {
                ReadFile(
                    file.get(),
                    bytes[done..].as_mut_ptr().cast(),
                    want,
                    &raw mut got,
                    null_mut(),
                )
            } != 0;
            if !read {
                return Err(os_error(
                    ErrorCode::FileImportFailure,
                    "the file cannot be read",
                    last_error(),
                    path,
                ));
            }
            if got == 0 {
                break; // it shrank while it was read
            }
            done += got as usize;
        }
        bytes.truncate(done);
        Ok(bytes)
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        let absolute = absolute_normal(path);
        // The longest prefix that exists is the part a link can be in; the rest is names that do
        // not exist yet and so cannot be links.
        let mut existing = absolute.clone();
        let mut tail: Vec<OsString> = Vec::new();
        loop {
            if attributes_of(&wide(&existing)) != INVALID_FILE_ATTRIBUTES {
                break;
            }
            let error = last_error();
            if !is_missing_error(error) {
                return Err(os_error(
                    ErrorCode::FileImportFailure,
                    "the path cannot be resolved",
                    error,
                    &existing,
                ));
            }
            let Some(parent) = parent_of(&existing) else {
                return Ok(absolute); // not even the drive exists
            };
            tail.push(existing.file_name().map(OsString::from).unwrap_or_default());
            existing = parent;
        }
        let handle = create_file(
            &wide(&existing),
            0,
            SHARE_ALL,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
        );
        if !handle.valid() {
            return Err(os_error(
                ErrorCode::FileImportFailure,
                "the path cannot be resolved",
                last_error(),
                &existing,
            ));
        }
        let final_name = |buffer: &mut Vec<u16>| -> u32 {
            // SAFETY: the handle is open and the buffer holds the length given.
            unsafe {
                GetFinalPathNameByHandleW(
                    handle.get(),
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
                )
            }
        };
        let mut final_path = vec![0u16; 512];
        let mut length = final_name(&mut final_path);
        if length as usize >= final_path.len() {
            final_path = vec![0u16; length as usize + 1];
            length = final_name(&mut final_path);
        }
        if length == 0 || length as usize >= final_path.len() {
            return Err(os_error(
                ErrorCode::FileImportFailure,
                "the path cannot be resolved",
                last_error(),
                &existing,
            ));
        }
        final_path.truncate(length as usize);
        let unc: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
        let verbatim: Vec<u16> = "\\\\?\\".encode_utf16().collect();
        let final_path = if final_path.starts_with(&unc) {
            let mut shortened = vec![BACKSLASH, BACKSLASH];
            shortened.extend_from_slice(&final_path[unc.len()..]);
            shortened
        } else if final_path.starts_with(&verbatim) {
            final_path[verbatim.len()..].to_vec()
        } else {
            final_path
        };
        let mut resolved = PathBuf::from(OsString::from_wide(&final_path));
        for name in tail.iter().rev() {
            resolved.push(name);
        }
        Ok(resolved)
    }

    fn free_space(&self, path: &Path) -> Result<u64> {
        // The volume of the nearest folder that exists.
        let mut probe = absolute_normal(path);
        loop {
            let attributes = attributes_of(&wide(&probe));
            if attributes != INVALID_FILE_ATTRIBUTES {
                if attributes & FILE_ATTRIBUTE_DIRECTORY == 0
                    && let Some(parent) = probe.parent().map(Path::to_path_buf)
                {
                    probe = parent;
                }
                break;
            }
            match parent_of(&probe) {
                Some(parent) => probe = parent,
                None => break,
            }
        }
        let mut available = 0u64;
        let mut total = 0u64;
        let mut free = 0u64;
        // SAFETY: a NUL-terminated name and three places for the answers.
        let read = unsafe {
            GetDiskFreeSpaceExW(
                wide(&probe).as_ptr(),
                &raw mut available,
                &raw mut total,
                &raw mut free,
            )
        } != 0;
        if !read {
            return Err(os_error(
                ErrorCode::FileImportFailure,
                "the free space cannot be read",
                last_error(),
                &probe,
            ));
        }
        Ok(available)
    }

    // ---- changing ------------------------------------------------------------------------------

    fn make_directories(&self, directory: &Path) -> Status {
        let mut missing: Vec<PathBuf> = Vec::new();
        let mut probe = absolute_normal(directory);
        loop {
            let attributes = attributes_of(&wide(&probe));
            if attributes != INVALID_FILE_ATTRIBUTES {
                if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
                    return Err(os_error(
                        ErrorCode::InvalidState,
                        "a file is where a folder is needed",
                        ERROR_DIRECTORY,
                        &probe,
                    ));
                }
                break;
            }
            let error = last_error();
            if !is_missing_error(error) {
                return Err(os_error(
                    ErrorCode::FileExportFailure,
                    "the folder cannot be examined",
                    error,
                    &probe,
                ));
            }
            missing.push(probe.clone());
            match parent_of(&probe) {
                Some(parent) => probe = parent,
                None => break,
            }
        }
        for folder in missing.iter().rev() {
            let name = wide(folder);
            // SAFETY: a NUL-terminated name and no security attributes.
            if unsafe { CreateDirectoryW(name.as_ptr(), null()) } == 0 {
                let error = last_error();
                if error == ERROR_ALREADY_EXISTS
                    && (attributes_of(&name) & FILE_ATTRIBUTE_DIRECTORY) != 0
                {
                    continue; // made by someone else a moment ago
                }
                return Err(os_error(
                    ErrorCode::FileExportFailure,
                    "the folder cannot be created",
                    error,
                    folder,
                ));
            }
        }
        Ok(())
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Status {
        let source = create_file(
            &wide(from),
            GENERIC_READ,
            SHARE_ALL,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
        );
        if !source.valid() {
            let error = last_error();
            return Err(os_error(
                if is_missing_error(error) {
                    ErrorCode::NotFound
                } else {
                    ErrorCode::FileImportFailure
                },
                "the file to copy cannot be opened",
                error,
                from,
            ));
        }
        // A new file with the folder's own access rules (not the source's: the stage folder is
        // writable by anyone running as the user, and a file in the install must take the
        // install's), written by hand rather than by CopyFileW, which would carry the source's
        // attributes and alternate streams along and gives no handle to flush.
        let wide_to = wide(to);
        let mut destination = create_file(
            &wide_to,
            GENERIC_WRITE,
            0,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
        );
        if !destination.valid() {
            let error = last_error();
            let code = if error == ERROR_FILE_EXISTS || error == ERROR_ALREADY_EXISTS {
                ErrorCode::AlreadyExists
            } else if is_missing_error(error) {
                ErrorCode::NotFound
            } else {
                ErrorCode::FileExportFailure
            };
            return Err(os_error(code, "the copy cannot be created", error, to));
        }
        let mut buffer = vec![0u8; 1 << 20];
        let mut status: Status = Ok(());
        while status.is_ok() {
            let mut got = 0u32;
            // SAFETY: at most the buffer's length is written into it.
            let read = unsafe {
                ReadFile(
                    source.get(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &raw mut got,
                    null_mut(),
                )
            } != 0;
            if !read {
                status = Err(os_error(
                    ErrorCode::FileImportFailure,
                    "the file to copy cannot be read",
                    last_error(),
                    from,
                ));
                break;
            }
            if got == 0 {
                break;
            }
            status = write_all(destination.get(), &buffer[..got as usize], to);
        }
        if status.is_ok() && !flush(&destination) {
            status = Err(os_error(
                ErrorCode::FileExportFailure,
                "the copy cannot be flushed to disk",
                last_error(),
                to,
            ));
        }
        if status.is_ok() {
            // SAFETY: plain data, filled by the call.
            let mut written: FILETIME = unsafe { zeroed() };
            // SAFETY: both handles are open; only the last-write time is asked for and set. Best
            // effort: a copy whose time could not be carried is still the copy.
            unsafe {
                if GetFileTime(source.get(), null_mut(), null_mut(), &raw mut written) != 0 {
                    SetFileTime(destination.get(), null(), null(), &raw const written);
                }
            }
        }
        destination.reset();
        if status.is_err() {
            // Never leave half a file that looks like a copy.
            // SAFETY: a NUL-terminated name.
            unsafe { DeleteFileW(wide_to.as_ptr()) };
        }
        status
    }

    fn rename_file(&self, from: &Path, to: &Path) -> Status {
        let wide_from = wide(from);
        let wide_to = wide(to);
        let deadline = deadline_after(self.retry.total);
        loop {
            // SAFETY: two NUL-terminated names; no flag.
            if unsafe { MoveFileExW(wide_from.as_ptr(), wide_to.as_ptr(), 0) } != 0 {
                return Ok(());
            }
            let error = last_error();
            if error == ERROR_ALREADY_EXISTS || error == ERROR_FILE_EXISTS {
                return Err(os_error(
                    ErrorCode::AlreadyExists,
                    "the target exists",
                    error,
                    to,
                ));
            }
            if is_missing_error(error) {
                return Err(os_error(
                    ErrorCode::NotFound,
                    "the file to move is not there",
                    error,
                    from,
                ));
            }
            let wait = next_wait(deadline, self.retry.interval);
            match wait {
                Some(wait) if is_retryable(error) => std::thread::sleep(wait),
                _ => {
                    return Err(os_error(
                        ErrorCode::FileExportFailure,
                        "the file could not be moved",
                        error,
                        from,
                    ));
                }
            }
        }
    }

    fn remove_file(&self, path: &Path) -> Status {
        let wide = wide(path);
        // SAFETY: a NUL-terminated name.
        if unsafe { DeleteFileW(wide.as_ptr()) } != 0 {
            return Ok(());
        }
        let mut error = last_error();
        if is_missing_error(error) {
            return Ok(());
        }
        if error == ERROR_ACCESS_DENIED {
            let attributes = attributes_of(&wide);
            if attributes != INVALID_FILE_ATTRIBUTES
                && (attributes & FILE_ATTRIBUTE_READONLY) != 0
                && (attributes & FILE_ATTRIBUTE_DIRECTORY) == 0
            {
                // SAFETY: a NUL-terminated name.
                unsafe {
                    SetFileAttributesW(wide.as_ptr(), attributes & !FILE_ATTRIBUTE_READONLY);
                }
                // SAFETY: as above.
                if unsafe { DeleteFileW(wide.as_ptr()) } != 0 {
                    return Ok(());
                }
                error = last_error();
            }
        }
        Err(os_error(
            ErrorCode::FileExportFailure,
            "the file could not be deleted",
            error,
            path,
        ))
    }

    fn remove_empty_directory(&self, path: &Path) -> Status {
        // SAFETY: a NUL-terminated name.
        if unsafe { RemoveDirectoryW(wide(path).as_ptr()) } != 0 {
            return Ok(());
        }
        let error = last_error();
        if is_missing_error(error) {
            return Ok(());
        }
        if error == ERROR_DIR_NOT_EMPTY {
            return Err(os_error(
                ErrorCode::InvalidState,
                "the folder is not empty",
                error,
                path,
            ));
        }
        Err(os_error(
            ErrorCode::FileExportFailure,
            "the folder could not be deleted",
            error,
            path,
        ))
    }

    fn append_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        // FILE_APPEND_DATA without FILE_WRITE_DATA: every write goes to the end, whatever else is
        // writing, and FlushFileBuffers is still allowed.
        let file = create_file(
            &wide(path),
            FILE_APPEND_DATA | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
            SHARE_ALL,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
        );
        if !file.valid() {
            return Err(os_error(
                ErrorCode::FileExportFailure,
                "the file cannot be opened",
                last_error(),
                path,
            ));
        }
        let mut status = write_all(file.get(), bytes, path);
        if status.is_ok() && !flush(&file) {
            status = Err(os_error(
                ErrorCode::FileExportFailure,
                "the file cannot be flushed to disk",
                last_error(),
                path,
            ));
        }
        status
    }

    fn write_durable(&self, path: &Path, bytes: &[u8]) -> Status {
        // Through a temporary and a rename, so a crash leaves the old file or the new one and
        // never half of one.
        let mut temporary = path.as_os_str().to_os_string();
        temporary.push(".katana-tmp");
        let temporary = PathBuf::from(temporary);
        let wide_temporary = wide(&temporary);
        {
            let mut file = create_file(
                &wide_temporary,
                GENERIC_WRITE,
                0,
                CREATE_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
            );
            if !file.valid() {
                return Err(os_error(
                    ErrorCode::FileExportFailure,
                    "the file cannot be created",
                    last_error(),
                    &temporary,
                ));
            }
            let mut status = write_all(file.get(), bytes, &temporary);
            if status.is_ok() && !flush(&file) {
                status = Err(os_error(
                    ErrorCode::FileExportFailure,
                    "the file cannot be flushed to disk",
                    last_error(),
                    &temporary,
                ));
            }
            if let Err(error) = status {
                file.reset();
                // SAFETY: a NUL-terminated name.
                unsafe { DeleteFileW(wide_temporary.as_ptr()) };
                return Err(error);
            }
        }
        let wide = wide(path);
        let deadline = deadline_after(self.retry.total);
        loop {
            // SAFETY: two NUL-terminated names.
            let moved = unsafe {
                MoveFileExW(
                    wide_temporary.as_ptr(),
                    wide.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } != 0;
            if moved {
                return Ok(());
            }
            let error = last_error();
            match next_wait(deadline, self.retry.interval) {
                Some(wait) if is_retryable(error) => std::thread::sleep(wait),
                _ => {
                    // SAFETY: a NUL-terminated name.
                    unsafe { DeleteFileW(wide_temporary.as_ptr()) };
                    return Err(os_error(
                        ErrorCode::FileExportFailure,
                        "the file could not be replaced",
                        error,
                        path,
                    ));
                }
            }
        }
    }

    fn set_executable(&self, _path: &Path) -> Status {
        Ok(())
    }

    // ---- durability and exclusion ----------------------------------------------------------------

    fn sync_directory(&self, directory: &Path) -> Status {
        // Best effort: a directory handle can be flushed on NTFS, and some file systems refuse.
        // NTFS logs a rename's metadata change in order, so a refusal costs nothing the recovery
        // does not already tolerate (any prefix of the renames).
        let handle = create_file(
            &wide(directory),
            GENERIC_WRITE,
            SHARE_ALL,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
        );
        if handle.valid() {
            flush(&handle);
        }
        Ok(())
    }

    fn try_lock(&self, path: &Path) -> Result<Box<dyn FileLock>> {
        let handle = create_file(
            &wide(path),
            GENERIC_READ | GENERIC_WRITE,
            0,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
        );
        if !handle.valid() {
            let error = last_error();
            if error == ERROR_SHARING_VIOLATION || error == ERROR_LOCK_VIOLATION {
                return Err(os_error(ErrorCode::InvalidState, "held", error, path));
            }
            return Err(os_error(
                if is_missing_error(error) {
                    ErrorCode::NotFound
                } else {
                    ErrorCode::FileExportFailure
                },
                "the lock file cannot be opened",
                error,
                path,
            ));
        }
        Ok(Box::new(WindowsLock(handle)))
    }
}

pub(crate) fn make_system_fs_ops(retry: &RetryPolicy) -> Box<dyn FsOps> {
    Box::new(WindowsFsOps { retry: *retry })
}
