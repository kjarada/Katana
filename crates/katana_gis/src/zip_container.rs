//! A ZIP archive as a container of named byte strings
//! (include/katana/gis/zip_container.hpp, src/katana_io/zip_container.cpp).
//!
//! Here for one reason: an archive file of the zipped kind (.12daz) is a
//! standard ZIP holding a single DEFLATE-compressed archive file (.12da). Its
//! reader (katana_archive12d) deliberately links no third-party library, so
//! the container is opened here, behind the same wall as everything else that
//! needs GDAL, and only bytes cross it.
//!
//! REJECTED: a small inflate of Katana's own inside katana_archive12d: a
//! second implementation of something zlib already does, written by us, on
//! untrusted input.
//!
//! A member's bytes are `Vec<u8>`, where the C++ has a `std::string` of them:
//! text is bytes up to the decoder.

#![allow(
    unsafe_code,
    reason = "Reads and writes archives through GDAL's virtual file system; every call states why it is sound."
)]

use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};

use crate::cpl_error_collector::quiet_errors;
use crate::ffi::cpl;
use crate::formats_detail::remove_path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZipMember {
    /// Path within the archive, '/'-separated.
    pub name: String,
    /// Uncompressed.
    pub size: u64,
}

/// One member of an archive to write: its path within, and its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZipContent<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

/// GDAL's name for a zip that does not end in ".zip": the archive's path in
/// braces. Forward slashes, UTF-8 - which is what GDAL takes on every
/// platform (GDAL_FILENAME_IS_UTF8 defaults to YES).
fn vsi_root(archive: &Path) -> Vec<u8> {
    let mut root = b"/vsizip/{".to_vec();
    root.extend_from_slice(&cpl::generic_path(archive));
    root.push(b'}');
    root
}

/// `root + "/" + member`.
fn member_path(root: &[u8], member: &[u8]) -> Vec<u8> {
    let mut path = root.to_vec();
    path.push(b'/');
    path.extend_from_slice(member);
    path
}

fn last_error() -> String {
    // SAFETY: this thread's last error message, copied at once.
    unsafe { cpl::decoded(cpl::CPLGetLastErrorMsg()) }
}

fn with_reason(context: &str) -> String {
    let reason = last_error();
    if reason.is_empty() {
        String::from(context)
    } else {
        format!("{context}: {reason}")
    }
}

/// FileImportFailure when the file is missing or is not a ZIP.
pub fn list_zip(archive: &Path) -> Result<Vec<ZipMember>> {
    if !archive.is_file() {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the file does not exist",
            cpl::path_text(archive),
        ));
    }
    // Errors are returned, not printed: GDAL's default handler writes to
    // stderr, which in the desktop application nobody is reading.
    let _quiet = quiet_errors();
    let root = vsi_root(archive);
    let root_c = cpl::c_string(&root);
    // SAFETY: a NUL-terminated name; the list GDAL returns is copied and
    // freed by take_string_list.
    let Some(names) = (unsafe { cpl::take_string_list(cpl::VSIReadDirRecursive(root_c.as_ptr())) })
    else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the file is not a ZIP archive",
            with_reason(&cpl::path_text(archive)),
        ));
    };
    let mut members = Vec::new();
    for entry in names {
        if entry.is_empty() || entry.ends_with(b"/") {
            continue; // a directory
        }
        let path = cpl::c_string(member_path(&root, &entry));
        let mut status = cpl::VsiStatBufL::zeroed();
        // SAFETY: a NUL-terminated name and a status buffer of the
        // platform's VSIStatBufL size.
        if unsafe { cpl::VSIStatL(path.as_ptr(), &raw mut status) } != 0 || status.is_dir() {
            continue;
        }
        members.push(ZipMember {
            name: String::from_utf8_lossy(&entry).into_owned(),
            size: status.size() as u64,
        });
    }
    Ok(members)
}

/// The uncompressed bytes of one member. A member larger than `max_bytes`
/// is refused BEFORE it is inflated: the size comes from the archive's
/// directory, and a few kilobytes of zip can claim to hold gigabytes.
pub fn read_zip_member(archive: &Path, member: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let members = list_zip(archive)?;
    let Some(found) = members.iter().find(|entry| entry.name == member) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "the archive has no such member",
            format!("{}: {member}", cpl::path_text(archive)),
        ));
    };
    if found.size > max_bytes {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            format!("the member is larger than the limit of {max_bytes} bytes"),
            format!("{member} is {}", found.size),
        ));
    }

    let _quiet = quiet_errors();
    let path = cpl::c_string(member_path(&vsi_root(archive), member.as_bytes()));
    // SAFETY: NUL-terminated name and mode; the file is closed below.
    let file = unsafe { cpl::VSIFOpenL(path.as_ptr(), c"rb".as_ptr()) };
    if file.is_null() {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the member could not be opened",
            with_reason(member),
        ));
    }
    let mut bytes = vec![0u8; found.size as usize];
    let read = if bytes.is_empty() {
        0
    } else {
        // SAFETY: an open file and a buffer of bytes.len() bytes.
        unsafe { cpl::VSIFReadL(bytes.as_mut_ptr().cast(), 1, bytes.len(), file) }
    };
    // SAFETY: the file opened above, closed once.
    unsafe { cpl::VSIFCloseL(file) };
    if read != bytes.len() {
        // The directory promised more than the stream held: a truncated or
        // corrupt archive, which must not be parsed as if it were whole.
        return Err(make_error(
            ErrorCode::FileImportFailure,
            format!(
                "the member is truncated or corrupt: {read} of {} bytes could be read",
                bytes.len()
            ),
            member,
        ));
    }
    Ok(bytes)
}

/// Writes a NEW archive holding the one member, DEFLATE-compressed, replacing
/// any file already at `archive`. It is written beside the target and renamed
/// over it, so a failure half-way leaves the old file, not half a new one.
pub fn write_zip(archive: &Path, member_name: &str, bytes: &[u8]) -> Status {
    write_zip_members(
        archive,
        &[ZipContent {
            name: member_name,
            bytes,
        }],
    )
}

/// The same with several members, in their order. InvalidArgument for none,
/// or for a name that is no plain relative path.
pub fn write_zip_members(archive: &Path, members: &[ZipContent<'_>]) -> Status {
    if members.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "an archive needs a member to hold",
            cpl::path_text(archive),
        ));
    }
    for member in members {
        let name = member.name;
        if name.is_empty()
            || name.contains(['\\', ':'])
            || name.starts_with('/')
            || name.contains("..")
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a member name must be a plain relative path",
                name,
            ));
        }
    }
    let mut temporary = archive.as_os_str().to_owned();
    temporary.push(".writing");
    let temporary = PathBuf::from(temporary);
    // vsizip adds to an archive that exists, which a leftover would be.
    remove_path(&temporary);

    // One member at a time, each closed before the next is opened: what
    // vsizip writes, adding each to the archive the first one created.
    for member in members {
        let _quiet = quiet_errors();
        let path = cpl::c_string(member_path(&vsi_root(&temporary), member.name.as_bytes()));
        // SAFETY: NUL-terminated name and mode; the file is closed below.
        let file = unsafe { cpl::VSIFOpenL(path.as_ptr(), c"wb".as_ptr()) };
        if file.is_null() {
            let reason = with_reason(&cpl::path_text(archive));
            remove_path(&temporary);
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "the archive could not be created",
                reason,
            ));
        }
        let bytes = member.bytes;
        let written = if bytes.is_empty() {
            0
        } else {
            // SAFETY: an open file and bytes.len() bytes to write.
            unsafe { cpl::VSIFWriteL(bytes.as_ptr().cast(), 1, bytes.len(), file) }
        };
        // The central directory is written on close, so a failed close is a
        // failed archive however many bytes went in.
        // SAFETY: the file opened above, closed once.
        let closed = unsafe { cpl::VSIFCloseL(file) };
        if written != bytes.len() || closed != 0 {
            let reason = with_reason(&cpl::path_text(archive));
            remove_path(&temporary);
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "the archive could not be written",
                reason,
            ));
        }
    }
    if std::fs::rename(&temporary, archive).is_err() {
        remove_path(&temporary);
        return Err(Error::new(
            ErrorCode::FileExportFailure,
            "the archive could not be moved into place",
        )
        .with_context(cpl::path_text(archive)));
    }
    Ok(())
}
