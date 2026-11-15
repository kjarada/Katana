//! CPL, GDAL's portability library (cpl_conv.h, cpl_error.h, cpl_string.h,
//! cpl_vsi.h): exactly the functions, types and constants the C++ of
//! src/katana_io calls, written by hand from the headers of GDAL 3.13.2, the
//! toolchain's. And the one place text crosses the boundary: what GDAL hands
//! back is decoded here, and what Rust hands GDAL is made a C string here.
//!
//! The ffi folder (one file per header family, each function declared once
//! in the crate; a later agent adds to its own file only what is missing):
//!
//! ```text
//! cpl.rs      cpl_conv.h, cpl_error.h, cpl_string.h, cpl_vsi.h   (K1)
//! minixml.rs  cpl_minixml.h                                      (K1)
//! gdal.rs     gdal.h, gdal_utils.h                               (K1)
//! ogr.rs      ogr_api.h, ogr_core.h                              (K1)
//! osr.rs      ogr_srs_api.h                                      (K1)
//! ```
//!
//! No binding crate (docs/rust.md, "Third-party libraries"): each C
//! enumeration is a `c_int` alias with its values as constants, as
//! katana_geodesy's PROJ declarations are, so a value GDAL returns that Rust
//! does not list is no undefined behaviour. `CPL_STDCALL` is empty outside
//! MSVC (cpl_port.h), so every block is `extern "C"`.
//!
//! The check: native/ffi_check.c, compiled by build.rs, holds a
//! `_Static_assert` for every size, offset and constant value these files
//! copy, and the type of every function they declare, written in the header's
//! own typedefs, compared with `_Generic` - so a header that changes, or a
//! declaration transcribed wrongly, fails the build rather than the run.
//! The `#[repr(C)]` layouts are asserted on the Rust side too.
//!
//! Text from GDAL is decoded lossily, a byte that is not UTF-8 becoming
//! U+FFFD, by `decoded` alone, as katana_geodesy decodes PROJ's; the C++
//! copies the bytes through. Paths go to GDAL as UTF-8 (GDAL_FILENAME_IS_UTF8
//! is on by default): a POSIX path's own bytes, a Windows path's text.

#![allow(
    unsafe_code,
    reason = "The declarations of GDAL's C API and the copies of the text it returns; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's values and layouts are declared whole where the C++ names them; the check in native/ffi_check.c holds each."
)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::path::Path;
use std::ptr;

// ---- cpl_error.h ------------------------------------------------------------------

/// `CPLErr`.
pub(crate) type CplErr = c_int;
pub(crate) const CE_NONE: CplErr = 0;
pub(crate) const CE_DEBUG: CplErr = 1;
pub(crate) const CE_WARNING: CplErr = 2;
pub(crate) const CE_FAILURE: CplErr = 3;
pub(crate) const CE_FATAL: CplErr = 4;

/// `CPLErrorNum`: an `int` (cpl_error.h, outside its STRICT_CPLERRORNUM_TYPE).
pub(crate) type CplErrorNum = c_int;

/// `CPLErrorHandler`.
pub(crate) type CplErrorHandler = Option<unsafe extern "C" fn(CplErr, CplErrorNum, *const c_char)>;

// ---- cpl_port.h, cpl_string.h ------------------------------------------------------

/// The C `TRUE` and `FALSE` GDAL's `int` booleans take.
pub(crate) const TRUE: c_int = 1;
pub(crate) const FALSE: c_int = 0;

// ---- cpl_vsi.h ----------------------------------------------------------------------

/// `VSILFILE` (`struct VSIVirtualHandle`).
#[repr(C)]
pub(crate) struct VsiLFile {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `VSIStatBufL`: the platform's 64-bit `struct stat` - `struct stat64` on
/// Linux (cpl_config.h's VSI_STAT64_T), `struct stat` on macOS, `struct
/// __stat64` on Windows. The C++ reads `st_mode` and `st_size` only, so the
/// struct is its size in bytes with those two read at their offsets; each
/// size and offset below is the platform C library's, checked by
/// native/ffi_check.c.
#[repr(C, align(8))]
pub(crate) struct VsiStatBufL {
    bytes: [u8; VSI_STAT_BUF_L_SIZE],
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod stat_layout {
    // glibc's x86_64 bits/struct_stat.h: st_dev, st_ino, st_nlink (8 each),
    // then st_mode.
    pub(crate) type StMode = u32;
    pub(crate) const SIZE: usize = 144;
    pub(crate) const ST_MODE_OFFSET: usize = 24;
    pub(crate) const ST_SIZE_OFFSET: usize = 48;
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod stat_layout {
    // glibc's generic bits/struct_stat.h: st_dev, st_ino (8 each), then st_mode.
    pub(crate) type StMode = u32;
    pub(crate) const SIZE: usize = 128;
    pub(crate) const ST_MODE_OFFSET: usize = 16;
    pub(crate) const ST_SIZE_OFFSET: usize = 48;
}

#[cfg(target_os = "macos")]
mod stat_layout {
    // Darwin's 64-bit-inode struct stat: st_dev (4), then st_mode (2).
    pub(crate) type StMode = u16;
    pub(crate) const SIZE: usize = 144;
    pub(crate) const ST_MODE_OFFSET: usize = 4;
    pub(crate) const ST_SIZE_OFFSET: usize = 96;
}

#[cfg(windows)]
mod stat_layout {
    // MinGW-w64's struct _stat64: st_dev (4), st_ino (2), then st_mode (2).
    pub(crate) type StMode = u16;
    pub(crate) const SIZE: usize = 56;
    pub(crate) const ST_MODE_OFFSET: usize = 6;
    pub(crate) const ST_SIZE_OFFSET: usize = 24;
}

#[cfg(not(any(
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    target_os = "macos",
    windows
)))]
compile_error!("VSIStatBufL's layout is written for Linux (x86_64, aarch64), macOS and Windows");

const VSI_STAT_BUF_L_SIZE: usize = stat_layout::SIZE;

/// `S_IFMT` and `S_IFDIR` (sys/stat.h): the same octal values on every
/// platform the port builds on (MSVCRT's `_S_IFMT` 0xF000 and `_S_IFDIR` 0x4000).
pub(crate) const S_IFMT: u32 = 0o170000;
pub(crate) const S_IFDIR: u32 = 0o040000;

const _: () = assert!(std::mem::size_of::<VsiStatBufL>() == VSI_STAT_BUF_L_SIZE);
const _: () = assert!(
    stat_layout::ST_MODE_OFFSET + std::mem::size_of::<stat_layout::StMode>() <= VSI_STAT_BUF_L_SIZE
);
const _: () = assert!(stat_layout::ST_SIZE_OFFSET + 8 <= VSI_STAT_BUF_L_SIZE);

impl VsiStatBufL {
    pub(crate) fn zeroed() -> Self {
        Self {
            bytes: [0; VSI_STAT_BUF_L_SIZE],
        }
    }

    /// `st_mode`, widened.
    #[allow(
        clippy::useless_conversion,
        reason = "st_mode is a u32 on Linux and a u16 on macOS and Windows."
    )]
    pub(crate) fn mode(&self) -> u32 {
        const WIDTH: usize = std::mem::size_of::<stat_layout::StMode>();
        let mut field = [0u8; WIDTH];
        field.copy_from_slice(
            &self.bytes[stat_layout::ST_MODE_OFFSET..stat_layout::ST_MODE_OFFSET + WIDTH],
        );
        u32::from(stat_layout::StMode::from_ne_bytes(field))
    }

    /// `st_size`, a 64-bit signed offset on every platform above.
    pub(crate) fn size(&self) -> i64 {
        let mut field = [0u8; 8];
        field.copy_from_slice(
            &self.bytes[stat_layout::ST_SIZE_OFFSET..stat_layout::ST_SIZE_OFFSET + 8],
        );
        i64::from_ne_bytes(field)
    }

    /// `VSI_ISDIR(st_mode)`: `S_ISDIR` on POSIX, a bit test on Windows
    /// (cpl_vsi.h).
    pub(crate) fn is_dir(&self) -> bool {
        #[cfg(windows)]
        {
            self.mode() & S_IFDIR != 0
        }
        #[cfg(not(windows))]
        {
            self.mode() & S_IFMT == S_IFDIR
        }
    }
}

#[link(name = "gdal")]
unsafe extern "C" {
    // ---- cpl_conv.h ---------------------------------------------------------------
    pub(crate) fn CPLGetConfigOption(key: *const c_char, default: *const c_char) -> *const c_char;
    pub(crate) fn CPLSetConfigOption(key: *const c_char, value: *const c_char);
    pub(crate) fn CPLGetThreadLocalConfigOption(
        key: *const c_char,
        default: *const c_char,
    ) -> *const c_char;
    pub(crate) fn CPLSetThreadLocalConfigOption(key: *const c_char, value: *const c_char);

    // ---- cpl_string.h -------------------------------------------------------------
    pub(crate) fn CPLTestBool(value: *const c_char) -> bool;
    pub(crate) fn CPLFetchBool(
        list: *const *const c_char,
        key: *const c_char,
        default: bool,
    ) -> bool;
    pub(crate) fn CSLDestroy(list: *mut *mut c_char);

    // ---- cpl_error.h --------------------------------------------------------------
    pub(crate) fn CPLSetErrorHandler(handler: CplErrorHandler) -> CplErrorHandler;
    pub(crate) fn CPLQuietErrorHandler(kind: CplErr, number: CplErrorNum, message: *const c_char);
    pub(crate) fn CPLPushErrorHandler(handler: CplErrorHandler);
    pub(crate) fn CPLPushErrorHandlerEx(handler: CplErrorHandler, user_data: *mut c_void);
    pub(crate) fn CPLPopErrorHandler();
    pub(crate) fn CPLGetErrorHandlerUserData() -> *mut c_void;
    pub(crate) fn CPLErrorReset();
    pub(crate) fn CPLGetLastErrorMsg() -> *const c_char;

    // ---- cpl_vsi.h ----------------------------------------------------------------
    /// `CPLFree` is a macro for it (cpl_conv.h).
    pub(crate) fn VSIFree(memory: *mut c_void);
    pub(crate) fn VSIReadDirRecursive(path: *const c_char) -> *mut *mut c_char;
    pub(crate) fn VSIStatL(path: *const c_char, status: *mut VsiStatBufL) -> c_int;
    pub(crate) fn VSIFOpenL(path: *const c_char, access: *const c_char) -> *mut VsiLFile;
    pub(crate) fn VSIFReadL(
        buffer: *mut c_void,
        size: usize,
        count: usize,
        file: *mut VsiLFile,
    ) -> usize;
    pub(crate) fn VSIFWriteL(
        buffer: *const c_void,
        size: usize,
        count: usize,
        file: *mut VsiLFile,
    ) -> usize;
    pub(crate) fn VSIFCloseL(file: *mut VsiLFile) -> c_int;
}

// ---- the boundary's text ------------------------------------------------------------

/// A copy of text GDAL returns, decoded lossily; empty for NULL.
///
/// # Safety
///
/// `text` is NULL or a live NUL-terminated string.
pub(crate) unsafe fn decoded(text: *const c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract above.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

/// A copy of the bytes GDAL returns, undecoded, for what goes back to GDAL
/// (a member's name inside an archive); empty for NULL.
///
/// # Safety
///
/// `text` is NULL or a live NUL-terminated string.
pub(crate) unsafe fn bytes_of(text: *const c_char) -> Vec<u8> {
    if text.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller's contract above.
    unsafe { CStr::from_ptr(text) }.to_bytes().to_vec()
}

/// The C string a std::string's c_str() hands GDAL: the text up to its first
/// NUL, where GDAL would stop reading.
pub(crate) fn c_string(text: impl AsRef<[u8]>) -> CString {
    let text = text.as_ref();
    let end = text
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(text.len());
    CString::new(&text[..end]).unwrap_or_default()
}

/// The strings of a NULL-terminated list GDAL allocated, copied, and the
/// list freed (`CSLDestroy`); None for a NULL list.
///
/// # Safety
///
/// `list` is NULL or a NULL-terminated list of live strings that GDAL
/// allocated and that nothing else frees.
pub(crate) unsafe fn take_string_list(list: *mut *mut c_char) -> Option<Vec<Vec<u8>>> {
    if list.is_null() {
        return None;
    }
    let mut out = Vec::new();
    let mut at = list;
    loop {
        // SAFETY: the list is NULL-terminated (the caller's contract) and the
        // terminator has not been passed, so this entry is inside it.
        let item = unsafe { at.read() };
        if item.is_null() {
            break;
        }
        // SAFETY: each entry before the terminator is a live string.
        out.push(unsafe { bytes_of(item) });
        // SAFETY: the terminator has not been reached, so the next entry is
        // still inside the list.
        at = unsafe { at.add(1) };
    }
    // SAFETY: the list is GDAL's to free and freed once, after its last read.
    unsafe { CSLDestroy(list) };
    Some(out)
}

/// `CPLStringList`: a NULL-terminated list of C strings that Rust owns, for
/// GDAL's `CSLConstList` and `char **` arguments, which GDAL only reads. Empty
/// is NULL, as an empty CPLStringList's `List()` is.
#[derive(Default)]
pub(crate) struct CStringList {
    strings: Vec<CString>,
    // The pointers of `strings`, then NULL. A CString's bytes are on the heap,
    // so a pointer stays good when the CString moves into the Vec.
    pointers: Vec<*const c_char>,
}

impl CStringList {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// `AddString`.
    pub(crate) fn add(&mut self, text: impl AsRef<[u8]>) {
        let text = c_string(text);
        self.pointers.pop();
        self.pointers.push(text.as_ptr());
        self.pointers.push(ptr::null());
        self.strings.push(text);
    }

    /// `SetNameValue`: replaces the entry whose name is `key`, ignoring case,
    /// followed by '=' or ':' (CSLFindName), with "key=value"; otherwise adds
    /// "key=value".
    pub(crate) fn set_name_value(&mut self, key: &[u8], value: &[u8]) {
        let key = &key[..key.iter().position(|&b| b == 0).unwrap_or(key.len())];
        let mut line = key.to_vec();
        line.push(b'=');
        line.extend_from_slice(value);
        let found = self.strings.iter().position(|entry| {
            let entry = entry.as_bytes();
            entry.len() > key.len()
                && entry[..key.len()].eq_ignore_ascii_case(key)
                && matches!(entry[key.len()], b'=' | b':')
        });
        match found {
            Some(index) => {
                let text = c_string(&line);
                self.pointers[index] = text.as_ptr();
                self.strings[index] = text;
            }
            None => self.add(&line),
        }
    }

    /// The list as `CSLConstList`: NULL when empty.
    pub(crate) fn as_ptr(&self) -> *const *const c_char {
        if self.strings.is_empty() {
            ptr::null()
        } else {
            self.pointers.as_ptr()
        }
    }

    /// The list as the `char **` of an argument GDAL only reads (the
    /// utilities' options: GDALTranslateOptionsNew copies its argv).
    pub(crate) fn as_argv(&self) -> *mut *mut c_char {
        self.as_ptr().cast_mut().cast()
    }
}

/// `path.string()`: a POSIX path's own bytes; a Windows path's text, as UTF-8.
pub(crate) fn native_path(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned().into_bytes()
    }
}

/// `path.generic_u8string()`: the same with forward slashes on Windows.
pub(crate) fn generic_path(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        native_path(path)
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().replace('\\', "/").into_bytes()
    }
}

/// A path as the text of a message (`path.string()` in an Error).
pub(crate) fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&native_path(path)).into_owned()
}
