//! The file-system calls of the update layer (fsops_win, install, state, process_scan_win): safe
//! wrappers, each the one Win32 call it names. They answer as the call does - a flag, a count, an
//! attribute word - and leave the error for `last_error()`, which the caller reads at once, so
//! the error codes and the `os_error=N` texts built from them are the calls' own.

use std::os::windows::io::{AsRawHandle, FromRawHandle, HandleOrInvalid, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, DeleteFileW, FindClose,
    FindFirstFileW, FindNextFileW, FlushFileBuffers, GetDiskFreeSpaceExW, GetFileAttributesExW,
    GetFileAttributesW, GetFileInformationByHandle, GetFileSizeEx, GetFileTime,
    GetFinalPathNameByHandleW, GetFullPathNameW, GetLongPathNameW, MoveFileExW, ReadFile,
    RemoveDirectoryW, SetFileAttributesW, SetFileTime, WIN32_FILE_ATTRIBUTE_DATA,
    WIN32_FIND_DATAW, WriteFile,
};

use super::{WideName, length_u32};

// GetFileExInfoStandard, the level of GetFileAttributesExW whose answer is a
// WIN32_FILE_ATTRIBUTE_DATA (fileapi.h).
const GET_FILE_EX_INFO_STANDARD: i32 = 0;

/// An open file or directory, closed once, when dropped.
#[derive(Debug)]
pub(crate) struct File(OwnedHandle);

/// CreateFileW with no security attributes and no template; None when it fails (the error is
/// `last_error()`'s).
pub(crate) fn create_file(
    name: &WideName,
    access: u32,
    share: u32,
    disposition: u32,
    flags: u32,
) -> Option<File> {
    // SAFETY: `name` is NUL-terminated (WideName); no security attributes and no template.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            null(),
            disposition,
            flags,
            null_mut(),
        )
    };
    // SAFETY: CreateFileW returns a handle this process now owns, or INVALID_HANDLE_VALUE, which
    // HandleOrInvalid is for; nothing else holds or closes it.
    let handle = unsafe { HandleOrInvalid::from_raw_handle(raw) };
    OwnedHandle::try_from(handle).ok().map(File)
}

impl File {
    fn raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }

    /// GetFileSizeEx; None when it fails.
    pub(crate) fn size(&self) -> Option<i64> {
        let mut size = 0i64;
        // SAFETY: the handle is open (owned by self); the size is written by the call.
        (unsafe { GetFileSizeEx(self.raw(), &raw mut size) } != 0).then_some(size)
    }

    /// ReadFile of at most `buffer.len()` bytes (and at most u32::MAX), synchronously; the count
    /// read, or None when it fails.
    pub(crate) fn read(&self, buffer: &mut [u8]) -> Option<u32> {
        let mut got = 0u32;
        // SAFETY: the handle is open; at most `length_u32(buffer.len())` bytes, which the buffer
        // holds, are written into it; no OVERLAPPED.
        let read = unsafe {
            ReadFile(
                self.raw(),
                buffer.as_mut_ptr(),
                length_u32(buffer.len()),
                &raw mut got,
                null_mut(),
            )
        } != 0;
        read.then_some(got)
    }

    /// WriteFile of at most `bytes.len()` bytes (and at most u32::MAX); the count written, or
    /// None when it fails.
    pub(crate) fn write(&self, bytes: &[u8]) -> Option<u32> {
        let mut written = 0u32;
        // SAFETY: the handle is open; at most `length_u32(bytes.len())` bytes of `bytes` are read
        // from; no OVERLAPPED.
        let wrote = unsafe {
            WriteFile(
                self.raw(),
                bytes.as_ptr(),
                length_u32(bytes.len()),
                &raw mut written,
                null_mut(),
            )
        } != 0;
        wrote.then_some(written)
    }

    /// FlushFileBuffers.
    pub(crate) fn flush(&self) -> bool {
        // SAFETY: the handle is open.
        unsafe { FlushFileBuffers(self.raw()) != 0 }
    }

    /// GetFinalPathNameByHandleW into `buffer`, which the call is told the length of: the length
    /// written, or the length needed when it does not fit, or 0 when it fails.
    pub(crate) fn final_path_name(&self, buffer: &mut [u16], flags: u32) -> u32 {
        // SAFETY: the handle is open and `buffer` holds as many units as the call is told.
        unsafe {
            GetFinalPathNameByHandleW(
                self.raw(),
                buffer.as_mut_ptr(),
                length_u32(buffer.len()),
                flags,
            )
        }
    }

    /// GetFileTime's last-write time alone; None when it fails.
    pub(crate) fn last_write_time(&self) -> Option<FILETIME> {
        let mut written = FILETIME::default();
        // SAFETY: the handle is open; only the last-write time is asked for, into a local.
        (unsafe { GetFileTime(self.raw(), null_mut(), null_mut(), &raw mut written) } != 0)
            .then_some(written)
    }

    /// SetFileTime of the last-write time alone.
    pub(crate) fn set_last_write_time(&self, time: &FILETIME) -> bool {
        // SAFETY: the handle is open; only the last-write time is set, read from `time`.
        unsafe { SetFileTime(self.raw(), null(), null(), time) != 0 }
    }
}

/// GetFileInformationByHandle of a file std opened: its volume serial number and file index
/// among the rest; None when it fails.
pub(crate) fn file_information(file: &std::fs::File) -> Option<BY_HANDLE_FILE_INFORMATION> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is `file`'s, open for the length of the call (the borrow), and
    // `information` is a writable struct of the type the call fills.
    let read = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) };
    (read != 0).then_some(information)
}

/// GetFileAttributesW: INVALID_FILE_ATTRIBUTES when it fails.
pub(crate) fn attributes(name: &WideName) -> u32 {
    // SAFETY: `name` is NUL-terminated.
    unsafe { GetFileAttributesW(name.as_ptr()) }
}

/// GetFileAttributesExW at its standard level; None when it fails.
pub(crate) fn attribute_data(name: &WideName) -> Option<WIN32_FILE_ATTRIBUTE_DATA> {
    let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
    // SAFETY: `name` is NUL-terminated and `data` is the structure the level names.
    let examined = unsafe {
        GetFileAttributesExW(
            name.as_ptr(),
            GET_FILE_EX_INFO_STANDARD,
            (&raw mut data).cast(),
        )
    } != 0;
    examined.then_some(data)
}

/// A directory search (FindFirstFileW), closed once, when dropped.
#[derive(Debug)]
pub(crate) struct Search(HANDLE);

/// FindFirstFileW: the search and its first entry, or None when it fails.
pub(crate) fn find_first(name: &WideName) -> Option<(Search, WIN32_FIND_DATAW)> {
    let mut found = WIN32_FIND_DATAW::default();
    // SAFETY: `name` is NUL-terminated and `found` is the structure the call fills.
    let search = unsafe { FindFirstFileW(name.as_ptr(), &raw mut found) };
    if search == INVALID_HANDLE_VALUE {
        return None;
    }
    Some((Search(search), found))
}

impl Search {
    /// FindNextFileW into `found`: false at the end, or when it fails.
    pub(crate) fn next(&mut self, found: &mut WIN32_FIND_DATAW) -> bool {
        // SAFETY: the search handle is open (owned by self) and `found` is its structure.
        unsafe { FindNextFileW(self.0, found) != 0 }
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        // SAFETY: the search handle is this value's, valid (find_first made it only from a
        // successful call), closed once, here.
        unsafe { FindClose(self.0) };
    }
}

/// GetDiskFreeSpaceExW: the bytes free to the caller on the volume of `name`; None when it
/// fails.
pub(crate) fn free_bytes_available(name: &WideName) -> Option<u64> {
    let mut available = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    // SAFETY: a NUL-terminated name and three places for the answers.
    let read = unsafe {
        GetDiskFreeSpaceExW(
            name.as_ptr(),
            &raw mut available,
            &raw mut total,
            &raw mut free,
        )
    } != 0;
    read.then_some(available)
}

/// CreateDirectoryW with no security attributes.
pub(crate) fn create_directory(name: &WideName) -> bool {
    // SAFETY: a NUL-terminated name and no security attributes.
    unsafe { CreateDirectoryW(name.as_ptr(), null()) != 0 }
}

/// DeleteFileW.
pub(crate) fn delete_file(name: &WideName) -> bool {
    // SAFETY: a NUL-terminated name.
    unsafe { DeleteFileW(name.as_ptr()) != 0 }
}

/// SetFileAttributesW.
pub(crate) fn set_attributes(name: &WideName, attributes: u32) -> bool {
    // SAFETY: a NUL-terminated name.
    unsafe { SetFileAttributesW(name.as_ptr(), attributes) != 0 }
}

/// RemoveDirectoryW.
pub(crate) fn remove_directory(name: &WideName) -> bool {
    // SAFETY: a NUL-terminated name.
    unsafe { RemoveDirectoryW(name.as_ptr()) != 0 }
}

/// MoveFileExW with `flags`.
pub(crate) fn move_file(from: &WideName, to: &WideName, flags: u32) -> bool {
    // SAFETY: two NUL-terminated names.
    unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) != 0 }
}

/// GetLongPathNameW into `buffer`: the length written, or the length needed when it does not
/// fit, or 0 when it fails.
pub(crate) fn long_path_name(name: &WideName, buffer: &mut [u16]) -> u32 {
    // SAFETY: `name` is NUL-terminated and `buffer` holds as many units as the call is told.
    unsafe { GetLongPathNameW(name.as_ptr(), buffer.as_mut_ptr(), length_u32(buffer.len())) }
}

/// GetFullPathNameW into `buffer`, no file part asked for: as long_path_name answers.
pub(crate) fn full_path_name(name: &WideName, buffer: &mut [u16]) -> u32 {
    // SAFETY: `name` is NUL-terminated, `buffer` holds as many units as the call is told, and the
    // file-part pointer may be null.
    unsafe {
        GetFullPathNameW(
            name.as_ptr(),
            length_u32(buffer.len()),
            buffer.as_mut_ptr(),
            null_mut(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::last_error;

    // winerror.h.
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    const ERROR_PATH_NOT_FOUND: u32 = 3;
    const OPEN_EXISTING: u32 = 3;
    const GENERIC_READ: u32 = 0x8000_0000;

    fn scratch(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "katana_update_win32_fs_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("the scratch folder is made");
        folder
    }

    fn wide(path: &std::path::Path) -> WideName {
        use std::os::windows::ffi::OsStrExt;
        WideName::from_units(path.as_os_str().encode_wide().collect())
    }

    #[test]
    fn opening_a_missing_file_gives_none_and_leaves_the_systems_error() {
        let folder = scratch("missing");
        let name = wide(&folder.join("not-there.txt"));
        assert!(create_file(&name, GENERIC_READ, 0, OPEN_EXISTING, 0).is_none());
        // The error the caller reads at once is the open's own.
        assert!(matches!(
            last_error(),
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND
        ));
    }

    #[test]
    fn a_file_closed_by_its_drop_can_then_be_deleted_and_a_search_of_its_folder_ends() {
        let folder = scratch("closed");
        let path = folder.join("a.txt");
        std::fs::write(&path, b"abc").expect("written");
        let name = wide(&path);
        {
            // Opened with no sharing: while the handle is open nobody else may delete it.
            let file = create_file(&name, GENERIC_READ, 0, OPEN_EXISTING, 0).expect("opens");
            assert_eq!(file.size(), Some(3));
            let mut buffer = [0u8; 8];
            assert_eq!(file.read(&mut buffer), Some(3));
            assert_eq!(&buffer[..3], b"abc");
            assert!(!delete_file(&name), "the open handle keeps it");
        }
        // Dropped, so closed: the delete goes through.
        assert!(delete_file(&name));

        let mut pattern: Vec<u16> = wide(&folder).0;
        pattern.pop();
        pattern.extend("\\*".encode_utf16());
        let (mut search, mut found) =
            find_first(&WideName::from_units(pattern)).expect("the folder lists '.' at least");
        let mut entries = 1;
        while search.next(&mut found) {
            entries += 1;
        }
        // "." and "..": the folder is empty now.
        assert_eq!(entries, 2);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
