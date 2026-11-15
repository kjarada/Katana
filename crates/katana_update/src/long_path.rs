//! A path as the system's file calls want it. Internal to the update library (the C++'s private
//! header long_path.hpp).
//!
//! On Windows a path of about 260 characters or more is refused by the ordinary calls unless it
//! carries the \\?\ prefix, and one that ends in a dot or a space is silently shortened. The real
//! file system (fsops_win.rs) prefixes every path it hands to the system, so an install under a
//! deep folder works. Code that reads a file by a way other than FsOps - the SHA-256 of a file,
//! which reads it through the standard library - has to give the same path or it fails exactly
//! where the file system did not: a copy under R/.update/<tx>/new/ is about 46 characters deeper
//! than the file it will replace.
//!
//! Nothing here touches the disk (std::path::absolute asks the system to lexically complete a
//! relative path, as the C++'s std::filesystem::absolute does).

use std::path::{Path, PathBuf};

#[cfg(windows)]
pub use windows::{LONG_PATH_LENGTH, absolute_normal, ends_in_dot_or_space, wide_path};

#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Path, PathBuf};

    /// Below MAX_PATH (260) with room for a name.
    pub const LONG_PATH_LENGTH: usize = 240;

    // std::filesystem::path::lexically_normal: "." dropped, a ".." taking the name before it (or
    // dropped at the root), the separators the platform's; a trailing separator kept after a name.
    fn lexically_normal(path: &Path) -> PathBuf {
        let mut prefix_and_root = PathBuf::new();
        let mut names: Vec<OsString> = Vec::new();
        let mut rooted = false;
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => prefix_and_root.push(prefix.as_os_str()),
                Component::RootDir => {
                    prefix_and_root.push(component.as_os_str());
                    rooted = true;
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if names.last().is_some_and(|name| name != "..") {
                        names.pop();
                    } else if !rooted {
                        names.push(OsString::from(".."));
                    }
                }
                Component::Normal(name) => names.push(name.to_os_string()),
            }
        }
        let mut normal = prefix_and_root;
        for name in &names {
            normal.push(name);
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        let trailing = matches!(wide.last(), Some(&unit) if unit == u16::from(b'\\') || unit == u16::from(b'/'));
        if trailing && names.last().is_some_and(|name| name != "..") {
            let mut text = normal.into_os_string();
            text.push("\\");
            normal = PathBuf::from(text);
        }
        if normal.as_os_str().is_empty() {
            normal = PathBuf::from(".");
        }
        normal
    }

    /// The path made absolute and lexically normal, with backslashes.
    pub fn absolute_normal(path: &Path) -> PathBuf {
        // An empty path names nothing and stays empty, so that every check on it fails. libc++
        // makes the CURRENT folder of it (absolute("") is current_path() there, with no error;
        // measured with clang 22 on Windows, the library of the Windows ARM64 build, 2026-10-14),
        // and an empty install root would then be planned, and changed, wherever the process
        // happened to start.
        if path.as_os_str().is_empty() {
            return PathBuf::new();
        }
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        lexically_normal(&absolute)
    }

    /// Whether the last name of `text` ends in a dot or a space. A trailing separator is not part
    /// of the name.
    pub fn ends_in_dot_or_space(text: &[u16]) -> bool {
        let mut end = text.len();
        while end > 0 && text[end - 1] == u16::from(b'\\') {
            end -= 1;
        }
        end > 0 && (text[end - 1] == u16::from(b'.') || text[end - 1] == u16::from(b' '))
    }

    fn starts_with(text: &[u16], prefix: &str) -> bool {
        let prefix: Vec<u16> = prefix.encode_utf16().collect();
        text.starts_with(&prefix)
    }

    /// The path as the wide string the system calls take, without a terminator: absolute, with
    /// backslashes, and the long-path prefix (backslash backslash question-mark backslash) when
    /// it is long or ends in a dot or a space.
    pub fn wide_path(path: &Path) -> Vec<u16> {
        let absolute = absolute_normal(path);
        let text: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        if starts_with(&text, "\\\\?\\") || starts_with(&text, "\\\\.\\") {
            return text;
        }
        if text.len() < LONG_PATH_LENGTH && !ends_in_dot_or_space(&text) {
            return text;
        }
        if starts_with(&text, "\\\\") {
            // \\server\share\... -> \\?\UNC\server\share\...
            let mut prefixed: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
            prefixed.extend_from_slice(&text[2..]);
            return prefixed;
        }
        let mut prefixed: Vec<u16> = "\\\\?\\".encode_utf16().collect();
        prefixed.extend_from_slice(&text);
        prefixed
    }

    pub(crate) fn openable(path: &Path) -> PathBuf {
        PathBuf::from(OsString::from_wide(&wide_path(path)))
    }
}

/// The path to give code that opens a file through a library (katana_core's sha256_file_hex): the
/// same file, named the way fsops_win.rs would name it. Unchanged where paths have no such limit.
pub fn openable(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        windows::openable(path)
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}
