use crate::text_encoding::utf8_or_windows1252;
use crate::{Error, ErrorCode, Result};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

pub fn path_from_utf8(text: impl AsRef<[u8]>) -> PathBuf {
    let text = text.as_ref();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(text))
    }
    #[cfg(windows)]
    {
        match std::str::from_utf8(text) {
            Ok(text) => PathBuf::from(text),
            Err(_) => native::narrow_path(text),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        PathBuf::from(utf8_or_windows1252(text))
    }
}

pub fn path_to_utf8(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_str()
            .expect("the path is not valid Unicode")
            .as_bytes()
            .to_vec()
    }
}

/// `path.generic_u8string()` as libstdc++ makes it, the form a record or a
/// reply shows a path in on every platform: '/' for the separator on Windows,
/// and a run of separators written as one. libstdc++'s
/// `path::generic_string` does not copy the text: it rebuilds it from the
/// path's elements - the root directory as one '/' however many lead, then
/// each name with one '/' after it - so `/vsizip//root/a.zip` reads back as
/// `/vsizip/root/a.zip` (the cli test of a zipped shapefile types the first
/// and the C++ replies with the second). "." and ".." are elements and stay.
/// On Windows a leading pair of separators begins a root name (`//server`),
/// which keeps both; that arm is unverified until step 8 builds there. One
/// home for the generic form; earlier layers' private copies are listed in
/// docs/rust.md "Not done".
pub fn generic_path_utf8(path: &Path) -> Vec<u8> {
    let mut bytes = path_to_utf8(path);
    if cfg!(windows) {
        for byte in &mut bytes {
            if *byte == b'\\' {
                *byte = b'/';
            }
        }
    }
    let root_name = cfg!(windows) && bytes.starts_with(b"//") && bytes.get(2) != Some(&b'/');
    let kept = if root_name { 2 } else { 0 };
    let mut generic = Vec::with_capacity(bytes.len());
    generic.extend_from_slice(&bytes[..kept]);
    for &byte in &bytes[kept..] {
        if byte == b'/' && generic.len() > kept && generic.last() == Some(&b'/') {
            continue;
        }
        generic.push(byte);
    }
    generic
}

/// A variable of the environment as UTF-8, empty when it is not set. On
/// Windows std reads the variable wide (GetEnvironmentVariableW), so a value
/// outside the ANSI code page is read as the text it holds; an unpaired
/// surrogate becomes U+FFFD. Elsewhere it is the bytes.
/// An [`EnvironmentOverride`] in force for the name is read instead.
pub fn environment_variable(name: Option<&str>) -> Vec<u8> {
    let Some(name) = name
        .and_then(|name| name.split('\0').next())
        .filter(|name| !name.is_empty())
    else {
        return Vec::new();
    };
    let value = overridden(name).unwrap_or_else(|| std::env::var_os(name));
    value.map_or_else(Vec::new, |value| environment_text(&value))
}

fn environment_text(value: &OsStr) -> Vec<u8> {
    #[cfg(windows)]
    {
        value.to_string_lossy().into_owned().into_bytes()
    }
    #[cfg(not(windows))]
    {
        path_to_utf8(Path::new(value))
    }
}

// Each name in force, with its value (None: read as not set).
static OVERRIDES: Mutex<Vec<(String, Option<OsString>)>> = Mutex::new(Vec::new());

fn overrides() -> MutexGuard<'static, Vec<(String, Option<OsString>)>> {
    OVERRIDES.lock().unwrap_or_else(PoisonError::into_inner)
}

// Windows matches a variable's name without regard to ASCII case; elsewhere
// a name is its bytes.
fn same_name(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

// Some(the override's value) when one is in force for `name`.
fn overridden(name: &str) -> Option<Option<OsString>> {
    overrides()
        .iter()
        .find(|(held, _)| same_name(held, name))
        .map(|(_, value)| value.clone())
}

/// A variable as [`environment_variable`] reads it - set to a value, or not
/// set - for as long as this lives, without a change to the process's
/// environment; when it is dropped the variable reads as it did before.
///
/// The tests that need a variable read otherwise use this. Changing the
/// environment itself (`std::env::set_var`) is unsafe since edition 2024,
/// because any other thread - a test running beside, a C library - may be
/// reading it at that moment; an override is a value behind a lock that only
/// Katana's own reads consult. So it reaches neither a library that reads the
/// environment itself (GDAL, libcurl) nor a program the process starts: a
/// test that needs those sets the variable on the child's `Command` instead.
/// Overrides of one name nest; drop them in the reverse order of making.
#[must_use = "the override ends when it is dropped"]
pub struct EnvironmentOverride {
    name: String,
    // The override of the same name this one hides, put back when it ends.
    hidden: Option<Option<OsString>>,
}

impl EnvironmentOverride {
    /// `name` read as `value`.
    pub fn set(name: &str, value: impl AsRef<OsStr>) -> Self {
        Self::new(name, Some(value.as_ref()))
    }

    /// `name` read as not set.
    pub fn unset(name: &str) -> Self {
        Self::new(name, None)
    }

    /// `value` when there is one, else not set.
    pub fn new(name: &str, value: Option<&OsStr>) -> Self {
        let value = value.map(OsStr::to_os_string);
        let mut overrides = overrides();
        let hidden = match overrides.iter_mut().find(|(held, _)| same_name(held, name)) {
            Some((_, held)) => Some(std::mem::replace(held, value)),
            None => {
                overrides.push((String::from(name), value));
                None
            }
        };
        Self {
            name: String::from(name),
            hidden,
        }
    }
}

impl Drop for EnvironmentOverride {
    fn drop(&mut self) {
        let mut overrides = overrides();
        let Some(at) = overrides
            .iter()
            .position(|(held, _)| same_name(held, &self.name))
        else {
            return;
        };
        match self.hidden.take() {
            Some(hidden) => overrides[at].1 = hidden,
            None => {
                overrides.remove(at);
            }
        }
    }
}

/// `std::filesystem::exists(path, error)` as libstdc++ answers it: a path
/// that leads nowhere, missing (ENOENT) or running through a file (ENOTDIR),
/// is not there, with no error; any other failure to tell is the error.
/// `Path::try_exists` errs for ENOTDIR, where the C++ answers false, so a
/// caller that takes "cannot tell" as "there" would call such a path there.
pub fn path_exists(path: &Path) -> std::io::Result<bool> {
    match path.try_exists() {
        Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => Ok(false),
        answer => answer,
    }
}

pub fn read_file_bytes(path: &Path) -> Result<Vec<u8>> {
    let error = |code, message| {
        Error::new(code, message).with_context(utf8_or_windows1252(path_to_utf8(path)))
    };
    if path.is_dir() {
        return Err(error(
            ErrorCode::NotFound,
            "the file cannot be opened: it is a directory",
        ));
    }
    let mut file =
        File::open(path).map_err(|_| error(ErrorCode::NotFound, "the file cannot be opened"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| error(ErrorCode::FileImportFailure, "the file cannot be read"))?;
    Ok(bytes)
}

// The ANSI code page's reading of bytes that are not UTF-8, which is what the
// C runtime's narrow calls make of them. std has no code-page conversion, so
// this one call stays a foreign one.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "MultiByteToWideChar, the ANSI code page's conversion, has no std equivalent."
)]
mod native {
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    // The system's ANSI code page (CP_ACP, WinNls.h).
    const ANSI_CODE_PAGE: u32 = 0;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MultiByteToWideChar(
            code_page: u32,
            flags: u32,
            bytes: *const u8,
            length: i32,
            wide: *mut u16,
            capacity: i32,
        ) -> i32;
    }

    pub fn narrow_path(text: &[u8]) -> PathBuf {
        let Ok(bytes) = i32::try_from(text.len()) else {
            return PathBuf::new();
        };
        if bytes == 0 {
            return PathBuf::new();
        }
        // SAFETY: `text` is read for exactly `bytes` bytes, its length; a null
        // buffer of no capacity asks only for the length the text needs.
        let length = unsafe {
            MultiByteToWideChar(
                ANSI_CODE_PAGE,
                0,
                text.as_ptr(),
                bytes,
                std::ptr::null_mut(),
                0,
            )
        };
        if length <= 0 {
            return PathBuf::new();
        }
        let mut wide = vec![0u16; length as usize];
        // SAFETY: as above, and `wide` holds the `length` units passed as its
        // capacity, which is all the call writes.
        unsafe {
            MultiByteToWideChar(
                ANSI_CODE_PAGE,
                0,
                text.as_ptr(),
                bytes,
                wide.as_mut_ptr(),
                length,
            )
        };
        PathBuf::from(std::ffi::OsString::from_wide(&wide))
    }
}
