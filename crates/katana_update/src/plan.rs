//! What an update would change, worked out by hashing the installed files FRESH, never from a
//! cache: a stale cache would skip a file that changed, and the swap would then ship a mixed tree.
//! A worker runs it when the dialog shows a newer version and again when the person presses
//! Update.
//!
//! A file is KEPT when the installed file exists and its SHA-256 equals the manifest's. Otherwise
//! it is CHANGED and is fetched. A `seed` file is changed only when it is absent (an edit by the
//! user survives; it is never read). Where a manifest file is not a plain file the plan says so
//! rather than guess: a link in the place of the file is CHANGED (it cannot be hashed without
//! following it, and the update does not follow links); an installed file that exists but cannot
//! be read (open in another program, no permission) is an ERROR naming it, not a change: the
//! update could not tell whether it differs, and replacing a file somebody holds open would fail
//! at the swap anyway; a link in a folder of the path, a file where a folder is needed, or a
//! folder or a special file where the file goes, is an ERROR naming the manifest path.
//!
//! Removals are not planned here: only the helper decides them, from the installed manifest it
//! verified itself, never from anything the engine computed.
//!
//! It hashes with katana_core's SHA-256 (sha256_file_hex streams the file in blocks; nothing is
//! held) and READS only: nothing under the root is created, changed or deleted here.
//!
//! A link is never followed. A folder on the way to a manifest file that is a link (a Windows
//! junction or symbolic link, a POSIX symlink) could lead a read, and in the helper a write, out
//! of the install, so the plan refuses it by name; the helper refuses the same thing again. The
//! FILE itself being a link is not followed either: it cannot be hashed without reading what it
//! points at, so it counts as changed, and the swap replaces the link, never what it points at (a
//! seed that is a link is kept: a seed is never read or written once it exists).
//!
//! What is a link on Windows is decided by the reparse TAG, not by the attribute: a reparse point
//! is also what OneDrive's placeholders, Windows' compressed and de-duplicated files and ProjFS
//! use, and those stay where they are (a folder in OneDrive is `Directory, ReparsePoint` and holds
//! ordinary files). Only a tag with the name-surrogate bit (symbolic link, junction and the like)
//! sends a path somewhere else.
//!
//! Every path the planner hands to the system, the examination of a name and the open of a file to
//! hash it, goes through long_path.rs, as the file system of the apply (fsops_win.rs) does: a file
//! under a deep install root is past the 260 characters the plain calls accept, and a program
//! without the long-path manifest would be told "no such file" for one that is there.

use crate::feed::{StopToken, stop_requested};
use crate::fsops::path_text;
use crate::install::Install;
use crate::long_path::openable;
use crate::manifest::{FileClass, Manifest};
use crate::paths::{MAX_FILES, MAX_INSTALL_BYTES};
use crate::ranges::{MergedRange, RANGE_MAX_LENGTH, RANGE_MERGE_GAP, RangeMember, merge_ranges};
use katana_core::error::make_error;
use katana_core::path_text::path_from_utf8;
use katana_core::sha256::sha256_file_hex;
use katana_core::{Error, ErrorCode, Result};
use std::path::{Path, PathBuf};

/// Free space needed, in changed bytes: the staged copy, the copy under new/, and old/.
pub const FREE_SPACE_FACTOR: f64 = 2.6;

// FREE_SPACE_FACTOR is applied in integer tenths, so the figure is exact: 2.6 is not a binary
// fraction, and ceil(2.6 * n) in doubles is one rounding away from the right answer at some n.
// Changing the factor must change this too.
const _: () = assert!(
    FREE_SPACE_FACTOR == 2.6,
    "required_free_bytes below is written for a factor of 2.6"
);
const FREE_SPACE_TENTHS: u64 = 26;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlannedFile {
    /// Into Manifest::files.
    pub index: usize,
    pub changed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// One per manifest file, in manifest order.
    pub files: Vec<PlannedFile>,
    pub total_files: u64,
    pub changed_files: u64,
    /// Sum of `size` of the changed files.
    pub changed_bytes: u64,
    /// Sum of `compressed_size` of the changed files: what the dialog quotes.
    pub download_bytes: u64,
    /// ceil(FREE_SPACE_FACTOR * changed_bytes).
    pub required_free_bytes: u64,
    /// The requests that fetch the changed files.
    pub ranges: Vec<MergedRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Missing,
    File,
    Directory,
    Link,
    #[cfg_attr(
        windows,
        allow(
            dead_code,
            reason = "Windows reports every entry it finds as a file or a folder"
        )
    )]
    Other,
}

// What is at `path`, without following a link at the last component (lstat). A Windows reparse
// point that is a name surrogate (junction, symbolic link, mount point; see is_link_reparse_tag)
// is a Link. Any other reparse point (a OneDrive placeholder) is the file or folder it appears to
// be.
#[cfg(not(windows))]
fn look(path: &Path) -> Result<Kind> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let kind = metadata.file_type();
            Ok(if kind.is_symlink() {
                Kind::Link
            } else if kind.is_file() {
                Kind::File
            } else if kind.is_dir() {
                Kind::Directory
            } else {
                Kind::Other
            })
        }
        // std::filesystem's symlink_status reads both as "not found": a missing name, and a name
        // below a file.
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(Kind::Missing)
        }
        Err(error) => Err(make_error(
            ErrorCode::FileImportFailure,
            "cannot examine the file",
            format!("{error}; {}", path_text(path)),
        )),
    }
}

#[cfg(windows)]
fn look(path: &Path) -> Result<Kind> {
    windows::look(path)
}

// The names a manifest path is made of, or None when it could leave the root. This is the
// containment rule only (relative, '/', no empty, "." or ".." name, no drive, stream or
// backslash) and not the naming rules of paths.rs, which the manifest has passed already
// (verify_manifest): a manifest built any other way must still not make a read outside.
fn split_contained(path: &str) -> Option<Vec<&str>> {
    let mut names = Vec::new();
    for name in path.split('/') {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains(['\\', ':'])
            || name.contains('\0')
        {
            return None;
        }
        names.push(name);
    }
    Some(names)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Absent,
    File,
    Link,
}

fn refused(code: ErrorCode, manifest_path: &str, message: String) -> Error {
    make_error(code, message, manifest_path)
}

// Walks root/<names> one name at a time and says what the last one is, and where it is. A name
// above the last must be a real folder: absent means the file is absent too; a link or a file is
// an error naming the manifest path.
fn locate(root: &Path, names: &[&str], manifest_path: &str) -> Result<(Found, PathBuf)> {
    let mut located = root.to_path_buf();
    for (i, &name) in names.iter().enumerate() {
        located.push(path_from_utf8(name));
        let kind = look(&located)?;
        if kind == Kind::Missing {
            return Ok((Found::Absent, located));
        }
        let last = i + 1 == names.len();
        if !last {
            if kind == Kind::Link {
                return Err(refused(
                    ErrorCode::InvalidState,
                    manifest_path,
                    format!(
                        "the installed file {manifest_path} is reached through a link ({name}), \
                         which the update does not follow"
                    ),
                ));
            }
            if kind != Kind::Directory {
                return Err(refused(
                    ErrorCode::InvalidState,
                    manifest_path,
                    format!(
                        "the installed file {manifest_path} cannot be there: {name} is a file, \
                         not a folder"
                    ),
                ));
            }
            continue;
        }
        return match kind {
            Kind::File => Ok((Found::File, located)),
            Kind::Link => Ok((Found::Link, located)),
            _ => Err(refused(
                ErrorCode::InvalidState,
                manifest_path,
                format!(
                    "the installed file {manifest_path} is a {}, not a file",
                    if kind == Kind::Directory {
                        "folder"
                    } else {
                        "special file"
                    }
                ),
            )),
        };
    }
    Ok((Found::Absent, located)) // unreachable: names is not empty
}

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

/// Hashes every file under install.root that the manifest lists. Checks `stop` between files;
/// cancelled -> InvalidState "cancelled". A root that is not a directory is NotFound. Cost: about
/// 2 s per 500 MB on the owner's machine.
pub fn plan(manifest: &Manifest, install: &Install, stop: StopToken<'_>) -> Result<Plan> {
    if !openable(&install.root).is_dir() {
        return Err(make_error(
            ErrorCode::NotFound,
            "the install folder does not exist",
            path_text(&install.root),
        ));
    }
    // The limits parse_manifest enforces, again: a plan is only as cheap as the manifest is small,
    // and the totals below must not wrap.
    if manifest.files.len() > MAX_FILES {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the manifest lists more files than an install may have",
            format!("{}", manifest.files.len()),
        ));
    }
    let mut all_bytes: u64 = 0;
    for file in &manifest.files {
        match all_bytes.checked_add(file.size) {
            Some(sum) if sum <= MAX_INSTALL_BYTES => all_bytes = sum,
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "the manifest's files add up to more than an install may hold",
                    format!("{MAX_INSTALL_BYTES} bytes at most"),
                ));
            }
        }
    }

    let mut result = Plan {
        files: Vec::with_capacity(manifest.files.len()),
        total_files: manifest.files.len() as u64,
        ..Plan::default()
    };
    let mut to_fetch: Vec<RangeMember> = Vec::new();

    for (index, file) in manifest.files.iter().enumerate() {
        if stop_requested(stop) {
            return Err(cancelled());
        }
        let Some(names) = split_contained(&file.path) else {
            return Err(refused(
                ErrorCode::InvalidArgument,
                &file.path,
                format!(
                    "the manifest names a file outside the install: {}",
                    file.path
                ),
            ));
        };
        let (found, located) = locate(&install.root, &names, &file.path)?;

        let seed = file.file_class == FileClass::Seed;
        let changed = match found {
            Found::Absent => true, // a seed is written when it is absent
            Found::Link => !seed,  // cannot be hashed without following it
            // A seed that exists is the person's: never read, never replaced.
            Found::File if seed => false,
            Found::File => {
                let hash = sha256_file_hex(&openable(&located)).map_err(|error| {
                    make_error(
                        ErrorCode::FileImportFailure,
                        format!(
                            "cannot read the installed file {} (is it open in another program, \
                             or not readable by you?)",
                            file.path
                        ),
                        format!("{}; {}", file.path, error.describe()),
                    )
                })?;
                hash != file.sha256
            }
        };

        result.files.push(PlannedFile { index, changed });
        if !changed {
            continue;
        }
        result.changed_files += 1;
        result.changed_bytes += file.size; // bounded by all_bytes above
        result.download_bytes = result
            .download_bytes
            .checked_add(file.compressed_size)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    "the manifest's compressed sizes add up to more than 64 bits",
                )
            })?;
        to_fetch.push(RangeMember {
            file_index: index,
            offset: file.offset,
            length: file.compressed_size,
        });
    }

    // ceil(2.6 n) = ceil(26 n / 10); n <= 2 GiB here, so 26 n cannot wrap.
    result.required_free_bytes = (result.changed_bytes * FREE_SPACE_TENTHS).div_ceil(10);
    result.ranges = merge_ranges(to_fetch, RANGE_MERGE_GAP, RANGE_MAX_LENGTH);
    Ok(result)
}

// What is at a path on Windows, by its attributes and, for a reparse point, its tag.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the entry and its reparse tag are read through the Win32 file API, confined to this adapter"
)]
mod windows {
    use super::Kind;
    use crate::fsops::{is_link_reparse_tag, path_text};
    use crate::long_path::wide_path;
    use katana_core::error::make_error;
    use katana_core::{ErrorCode, Result};
    use std::path::Path;
    use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindFirstFileW, GetFileAttributesW, WIN32_FIND_DATAW,
    };

    // The values of the Windows SDK's headers (winerror.h, winnt.h, fileapi.h).
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    const ERROR_PATH_NOT_FOUND: u32 = 3;
    const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    fn last_error() -> u32 {
        // SAFETY: GetLastError only reads this thread's last-error value.
        unsafe { GetLastError() }
    }

    fn cannot_examine(error: u32, path: &Path) -> katana_core::Error {
        make_error(
            ErrorCode::FileImportFailure,
            "cannot examine the file",
            format!("os_error={error}; {}", path_text(path)),
        )
    }

    pub(super) fn look(path: &Path) -> Result<Kind> {
        // Named the way the file system names it (long_path.rs): a deep install root puts a file
        // past the 260 characters the plain calls accept, and a program without the long-path
        // manifest would then be told "not found" - a changed file that is not, and a link at
        // that depth that goes unseen.
        let mut wide = wide_path(path);
        wide.push(0);
        // SAFETY: `wide` is a NUL-terminated wide path that outlives the call.
        let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
        if attributes == INVALID_FILE_ATTRIBUTES {
            let error = last_error();
            if error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND {
                return Ok(Kind::Missing);
            }
            return Err(cannot_examine(error, path));
        }
        if (attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            // The tag is in the directory entry: dwReserved0 of what FindFirstFileW returns. The
            // path exists (it has attributes) and has no wildcard that FindFirstFileW would
            // expand.
            // SAFETY: an all-zero WIN32_FIND_DATAW is a valid value of that plain struct.
            let mut entry: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
            // SAFETY: `wide` is NUL-terminated and `entry` is a writable struct of the type the
            // call fills.
            let find = unsafe { FindFirstFileW(wide.as_ptr(), &raw mut entry) };
            if find == INVALID_HANDLE_VALUE {
                return Err(cannot_examine(last_error(), path));
            }
            // SAFETY: `find` is the search handle opened above, closed once, here.
            unsafe { FindClose(find) };
            if (entry.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0
                && is_link_reparse_tag(entry.dwReserved0)
            {
                return Ok(Kind::Link);
            }
        }
        Ok(if (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0 {
            Kind::Directory
        } else {
            Kind::File
        })
    }
}
