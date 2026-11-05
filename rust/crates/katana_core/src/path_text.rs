use crate::text_encoding::utf8_or_windows1252;
use crate::{Error, ErrorCode, Result};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

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

pub fn environment_variable(name: Option<&str>) -> Vec<u8> {
    let Some(name) = name
        .and_then(|name| name.split('\0').next())
        .filter(|name| !name.is_empty())
    else {
        return Vec::new();
    };
    #[cfg(windows)]
    {
        native::environment_variable(name)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os(name).map_or_else(Vec::new, |value| path_to_utf8(Path::new(&value)))
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

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows code-page and wide environment APIs are confined to this adapter."
)]
mod native {
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

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
        fn GetEnvironmentVariableW(name: *const u16, value: *mut u16, capacity: u32) -> u32;
    }

    pub fn narrow_path(text: &[u8]) -> PathBuf {
        let Ok(bytes) = i32::try_from(text.len()) else {
            return PathBuf::new();
        };
        if bytes == 0 {
            return PathBuf::new();
        }
        let length =
            unsafe { MultiByteToWideChar(0, 0, text.as_ptr(), bytes, std::ptr::null_mut(), 0) };
        if length <= 0 {
            return PathBuf::new();
        }
        let mut wide = vec![0u16; length as usize];
        unsafe { MultiByteToWideChar(0, 0, text.as_ptr(), bytes, wide.as_mut_ptr(), length) };
        PathBuf::from(std::ffi::OsString::from_wide(&wide))
    }

    pub fn environment_variable(name: &str) -> Vec<u8> {
        let wide_name: Vec<u16> = name.bytes().map(u16::from).chain([0]).collect();
        let mut size =
            unsafe { GetEnvironmentVariableW(wide_name.as_ptr(), std::ptr::null_mut(), 0) };
        for _ in 0..4 {
            if size == 0 {
                return Vec::new();
            }
            let mut value = vec![0u16; size as usize];
            let length =
                unsafe { GetEnvironmentVariableW(wide_name.as_ptr(), value.as_mut_ptr(), size) };
            if length < size {
                value.truncate(length as usize);
                return String::from_utf16_lossy(&value).into_bytes();
            }
            size = length;
        }
        Vec::new()
    }
}
