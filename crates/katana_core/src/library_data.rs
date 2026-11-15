use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

pub fn loaded_library_path(file_name_prefix: &str) -> Option<PathBuf> {
    native::loaded_library_path(file_name_prefix)
}

fn normalised(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if result.file_name().is_some_and(|name| name != "..") {
                    result.pop();
                } else if !result.has_root() {
                    result.push("..");
                }
            }
            _ => result.push(component.as_os_str()),
        }
    }
    result
}

fn weakly_canonical(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match std::fs::canonicalize(ancestor) {
            Ok(real) => return Some(normalised(&real.join(path.strip_prefix(ancestor).ok()?))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    Some(normalised(path))
}

pub fn data_beside_library_file(library: &Path, relative: &Path) -> Option<PathBuf> {
    if library.as_os_str().is_empty() {
        return None;
    }
    let real = weakly_canonical(library)?;
    if !real.is_absolute() {
        return None;
    }
    let directory = real.parent()?;
    let prefix = directory.parent()?;
    let data = prefix.join(relative);
    if data.exists() {
        return Some(data);
    }
    if directory
        .file_name()
        .is_some_and(|name| name == "Frameworks")
    {
        let data = prefix.join("Resources").join(relative);
        if data.exists() {
            return Some(data);
        }
    }
    None
}

pub fn data_beside_library(file_name_prefix: &str, relative: &Path) -> Option<PathBuf> {
    data_beside_library_file(&loaded_library_path(file_name_prefix)?, relative)
}

pub fn bundled_proj_data_directory() -> &'static Option<PathBuf> {
    static DIRECTORY: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIRECTORY.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let library = "libproj.";
        #[cfg(windows)]
        let library = "libproj";
        #[cfg(not(any(target_os = "macos", windows)))]
        let library = "libproj.so";
        data_beside_library(library, Path::new("share/proj/proj.db"))
            .and_then(|data| data.parent().map(Path::to_path_buf))
    })
}

pub fn proj_data_directory() -> &'static Option<PathBuf> {
    static DIRECTORY: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIRECTORY.get_or_init(|| {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if std::env::var_os("PROJ_DATA").is_some() || std::env::var_os("PROJ_LIB").is_some() {
                None
            } else {
                bundled_proj_data_directory().clone()
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            None
        }
    })
}

#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "Enumerate loader-owned objects through dl_iterate_phdr; no FFI types escape."
)]
mod native {
    use std::ffi::{CStr, c_char, c_int, c_void};
    use std::path::PathBuf;

    #[repr(C)]
    struct DlPhdrInfo {
        address: usize,
        name: *const c_char,
    }

    unsafe extern "C" {
        fn dl_iterate_phdr(
            callback: unsafe extern "C" fn(*mut DlPhdrInfo, usize, *mut c_void) -> c_int,
            data: *mut c_void,
        ) -> c_int;
    }

    struct Search<'a> {
        prefix: &'a str,
        found: Option<PathBuf>,
    }

    unsafe extern "C" fn visit(info: *mut DlPhdrInfo, _size: usize, data: *mut c_void) -> c_int {
        let name = unsafe { (*info).name };
        if name.is_null() {
            return 0;
        }
        let name = unsafe { CStr::from_ptr(name) }.to_bytes();
        if name.is_empty() {
            return 0;
        }
        let search = unsafe { &mut *data.cast::<Search<'_>>() };
        let path = crate::path_text::path_from_utf8(name);
        let matches = path.file_name().is_some_and(|name| {
            use std::os::unix::ffi::OsStrExt;
            name.as_bytes().starts_with(search.prefix.as_bytes())
        });
        if matches {
            search.found = Some(path);
            1
        } else {
            0
        }
    }

    pub fn loaded_library_path(prefix: &str) -> Option<PathBuf> {
        let mut search = Search {
            prefix,
            found: None,
        };
        unsafe { dl_iterate_phdr(visit, (&mut search as *mut Search<'_>).cast()) };
        search.found
    }
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "dyld owns the loaded image list and its names for these calls."
)]
mod native {
    use std::ffi::{CStr, c_char};
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    unsafe extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(index: u32) -> *const c_char;
    }

    pub fn loaded_library_path(prefix: &str) -> Option<PathBuf> {
        let count = unsafe { _dyld_image_count() };
        for index in 0..count {
            let name = unsafe { _dyld_get_image_name(index) };
            if name.is_null() {
                continue;
            }
            let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
            if bytes.is_empty() {
                continue;
            }
            let path = crate::path_text::path_from_utf8(bytes);
            if path
                .file_name()
                .is_some_and(|name| name.as_bytes().starts_with(prefix.as_bytes()))
            {
                return Some(path);
            }
        }
        None
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows module enumeration and wide file names are confined to this adapter."
)]
mod native {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn K32EnumProcessModules(
            process: *mut c_void,
            modules: *mut *mut c_void,
            capacity: u32,
            needed: *mut u32,
        ) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, capacity: u32) -> u32;
    }

    pub fn loaded_library_path(prefix: &str) -> Option<PathBuf> {
        let mut modules = vec![std::ptr::null_mut::<c_void>(); 1024];
        let mut bytes = 0;
        let status = unsafe {
            K32EnumProcessModules(
                GetCurrentProcess(),
                modules.as_mut_ptr(),
                (modules.len() * std::mem::size_of::<*mut c_void>()) as u32,
                &mut bytes,
            )
        };
        if status == 0 {
            return None;
        }
        modules.truncate(
            modules
                .len()
                .min(bytes as usize / std::mem::size_of::<*mut c_void>()),
        );
        for module in modules {
            let mut candidate = vec![b' ' as u16; 512];
            loop {
                let length = unsafe {
                    GetModuleFileNameW(module, candidate.as_mut_ptr(), candidate.len() as u32)
                };
                if length == 0 {
                    candidate.clear();
                    break;
                }
                if (length as usize) < candidate.len() {
                    candidate.truncate(length as usize);
                    break;
                }
                candidate.resize(candidate.len() * 2, 0);
            }
            if candidate.is_empty() {
                continue;
            }
            let path = PathBuf::from(OsString::from_wide(&candidate));
            let name: Vec<u16> = path.file_name()?.encode_wide().collect();
            if name.len() >= prefix.len()
                && name.iter().zip(prefix.bytes()).all(|(found, wanted)| {
                    let found = if (u16::from(b'A')..=u16::from(b'Z')).contains(found) {
                        *found + u16::from(b'a' - b'A')
                    } else {
                        *found
                    };
                    found == u16::from(wanted.to_ascii_lowercase())
                })
            {
                return Some(path);
            }
        }
        None
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod native {
    pub fn loaded_library_path(_prefix: &str) -> Option<std::path::PathBuf> {
        None
    }
}
