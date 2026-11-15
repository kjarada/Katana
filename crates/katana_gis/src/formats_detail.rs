//! The names GDAL is asked to open a path by (src/katana_io/geo/formats_detail.hpp,
//! implemented in formats.cpp), so that GdalDataset::open and the routing of
//! a file (formats::identify_content) look into an archive the same way.
//!
//! With them, the rules of std::filesystem::path the sources apply to a name,
//! kept as the C++ has them where Rust's Path differs: a name that ends in a
//! separator has no file name (Rust's `file_name` takes the last component),
//! and `remove` takes a file or an empty folder.

#![allow(
    unsafe_code,
    reason = "Lists an archive's members through GDAL's virtual file system; every call states why it is sound."
)]

use std::path::Path;

use crate::cpl_error_collector::quiet_errors;
use crate::ffi::cpl;
use crate::formats::{comma_joined, extension_of_name, formats, is_not_offered, is_remote_path};

fn is_separator(byte: u8) -> bool {
    byte == b'/' || (cfg!(windows) && byte == b'\\')
}

/// `path.filename()`: what follows the last separator; empty when the name
/// ends in one.
pub(crate) fn file_name_of(name: &[u8]) -> &[u8] {
    let start = name
        .iter()
        .rposition(|&byte| is_separator(byte))
        .map_or(0, |at| at + 1);
    &name[start..]
}

/// `path.extension()` of a file name, with its dot: from the last dot, empty
/// for "." and "..", for a name that has no dot, and for one whose only dot
/// begins it (".profile").
pub(crate) fn extension_of_file_name(file: &[u8]) -> &[u8] {
    if file == b"." || file == b".." {
        return b"";
    }
    match file.iter().rposition(|&byte| byte == b'.') {
        None | Some(0) => b"",
        Some(at) => &file[at..],
    }
}

/// `std::filesystem::remove(path, ignored)`: a file, or an empty folder;
/// whether there was one to remove is not asked.
pub(crate) fn remove_path(path: &Path) -> bool {
    std::fs::remove_file(path)
        .or_else(|_| std::fs::remove_dir(path))
        .is_ok()
}

/// A name for GDAL: UTF-8, forward slashes (GDAL_FILENAME_IS_UTF8 is on by
/// default, and a slash is a separator everywhere).
fn gdal_name(path: &Path) -> Vec<u8> {
    let text = cpl::native_path(path);
    if text.starts_with(b"/vsi") || text.windows(3).any(|window| window == b"://") {
        return text; // GDAL's own syntax; left exactly as written
    }
    cpl::generic_path(path)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OpenNames {
    /// In the order to try them: the path itself, UTF-8 with forward
    /// slashes; then, for an archive GDAL does not open as it is (.zip, .kmz,
    /// .tar, .tgz), its inside as a folder (/vsizip/{<path>}), and the one
    /// member of it that is a dataset; for one compressed file (.gz), its
    /// inside (/vsigzip/<path>).
    pub(crate) names: Vec<Vec<u8>>,
    /// Set when the archive holds several datasets and none can be chosen:
    /// the refusal, naming them and how to name one.
    pub(crate) ambiguity: String,
}

pub(crate) fn open_names(path: &Path) -> OpenNames {
    let mut names = OpenNames::default();
    let name = gdal_name(path);
    names.names.push(name.clone());
    if name.starts_with(b"/vsi") || is_remote_path(path) {
        return names; // already GDAL's own words for where to look
    }
    let file = file_name_of(&cpl::native_path(path)).to_ascii_lowercase();
    let prefix: &[u8] = if file.ends_with(b".zip") || file.ends_with(b".kmz") {
        b"/vsizip/{"
    } else if file.ends_with(b".tar") || file.ends_with(b".tgz") || file.ends_with(b".tar.gz") {
        b"/vsitar/{"
    } else if file.ends_with(b".gz") {
        // One file compressed: its inside IS the dataset. /vsigzip takes no
        // braces (measured: "/vsigzip/{<path>}" opens nothing); it has no
        // member path after the file to tell apart from the file's own.
        let mut inside = b"/vsigzip/".to_vec();
        inside.extend_from_slice(&name);
        names.names.push(inside);
        return names;
    } else {
        return names;
    };
    let mut root = prefix.to_vec();
    root.extend_from_slice(&name);
    root.push(b'}');
    // The archive as a folder first: a shapefile's four files, or several
    // shapefiles, open so as the layers of one dataset.
    names.names.push(root.clone());
    // Otherwise the one member that is a dataset. Several are not guessed
    // between: the person names one.
    let _quiet = quiet_errors();
    let root_c = cpl::c_string(&root);
    // SAFETY: a NUL-terminated name; the list GDAL returns is copied and
    // freed by take_string_list.
    let members = unsafe { cpl::take_string_list(cpl::VSIReadDirRecursive(root_c.as_ptr())) }
        .unwrap_or_default();
    let mut datasets: Vec<Vec<u8>> = Vec::new();
    for member in members {
        if member.is_empty() || member.ends_with(b"/") {
            continue;
        }
        let extension = extension_of_name(&member);
        let data = !extension.is_empty()
            && !is_not_offered(&extension)
            && formats()
                .iter()
                .any(|format| format.reads() && format.extensions.contains(&extension));
        if data {
            datasets.push(member);
        }
    }
    if datasets.len() == 1 {
        let mut member = root;
        member.push(b'/');
        member.extend_from_slice(&datasets[0]);
        names.names.push(member);
    } else if datasets.len() > 1 {
        let decoded: Vec<String> = datasets
            .iter()
            .map(|member| String::from_utf8_lossy(member).into_owned())
            .collect();
        names.ambiguity = format!(
            "the archive holds {} datasets ({}); name one as {}/<member>",
            datasets.len(),
            comma_joined(decoded.iter().map(String::as_str)),
            String::from_utf8_lossy(&root)
        );
    }
    names
}
