//! GDAL's HTTP client (cpl_http.h): the result, the fetch and its release,
//! written by hand from GDAL 3.13.2's header (ffi/cpl.rs says how the ffi
//! folder is laid out and checked; this file's check is native/http_check.c),
//! and `CPLHTTPSetOptions` through native/http_shim.cpp.
//!
//! `CPLHTTPSetOptions` is the one GDAL call here that C cannot make: cpl_http.h
//! declares it after CPL_C_END, inside `#if defined(__cplusplus)`, so its
//! symbol is C++'s (`_Z17CPLHTTPSetOptionsPvPKcPKS1_` in libgdal.so.38). The
//! shim gives it a C name, compiled against the header of the GDAL linked, and
//! catches every exception, as the algorithm shim beside it does - rather than
//! a declaration of the mangled name, which no compiler would check and which
//! would let a C++ exception unwind into Rust.

#![allow(
    unsafe_code,
    reason = "The declarations of GDAL's HTTP client; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's result is declared whole; native/http_check.c holds each member."
)]

use std::ffi::{c_char, c_int, c_void};

use super::gdal::GdalProgressFunc;

/// `CPLMimePart`, every member in the header's order.
#[repr(C)]
pub(crate) struct CplMimePart {
    pub(crate) headers: *mut *mut c_char,
    pub(crate) data: *mut u8,
    pub(crate) data_len: c_int,
}

/// `CPLHTTPResult`, every member in the header's order. Made by
/// `CPLHTTPFetchEx`, read by Rust, freed by `CPLHTTPDestroyResult`.
#[repr(C)]
pub(crate) struct CplHttpResult {
    /// libcurl's CURLcode: 0 for success.
    pub(crate) n_status: c_int,
    pub(crate) psz_content_type: *mut c_char,
    pub(crate) psz_err_buf: *mut c_char,
    pub(crate) n_data_len: c_int,
    pub(crate) n_data_alloc: c_int,
    pub(crate) paby_data: *mut u8,
    pub(crate) papsz_headers: *mut *mut c_char,
    pub(crate) n_mime_part_count: c_int,
    pub(crate) pas_mime_part: *mut CplMimePart,
}

// The layouts of the 64-bit platforms the port builds on (an int padded to a
// pointer's alignment before each pointer); native/http_check.c holds the
// header's own numbers.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::size_of::<CplMimePart>() == 24);
    assert!(std::mem::offset_of!(CplMimePart, data) == 8);
    assert!(std::mem::offset_of!(CplMimePart, data_len) == 16);
    assert!(std::mem::size_of::<CplHttpResult>() == 64);
    assert!(std::mem::offset_of!(CplHttpResult, psz_content_type) == 8);
    assert!(std::mem::offset_of!(CplHttpResult, psz_err_buf) == 16);
    assert!(std::mem::offset_of!(CplHttpResult, n_data_len) == 24);
    assert!(std::mem::offset_of!(CplHttpResult, n_data_alloc) == 28);
    assert!(std::mem::offset_of!(CplHttpResult, paby_data) == 32);
    assert!(std::mem::offset_of!(CplHttpResult, papsz_headers) == 40);
    assert!(std::mem::offset_of!(CplHttpResult, n_mime_part_count) == 48);
    assert!(std::mem::offset_of!(CplHttpResult, pas_mime_part) == 56);
};

/// `CPLHTTPFetchWriteFunc`: GDAL hands the body to it instead of keeping it.
pub(crate) type CplHttpFetchWriteFunc = Option<
    unsafe extern "C" fn(
        buffer: *mut c_void,
        size: usize,
        count: usize,
        data: *mut c_void,
    ) -> usize,
>;

#[link(name = "gdal")]
unsafe extern "C" {
    pub(crate) fn CPLHTTPFetchEx(
        url: *const c_char,
        options: *const *const c_char,
        progress: GdalProgressFunc,
        progress_data: *mut c_void,
        write: CplHttpFetchWriteFunc,
        write_data: *mut c_void,
    ) -> *mut CplHttpResult;
    pub(crate) fn CPLHTTPDestroyResult(result: *mut CplHttpResult);
}

// native/http_shim.cpp, compiled into this crate's own native library.
unsafe extern "C" {
    /// `*headers = CPLHTTPSetOptions(curl, url, options)`: 0, or 1 when it
    /// threw (and `*headers` is NULL). The list is GDAL's headers for the
    /// handle, a `curl_slist *` the caller frees.
    pub(crate) fn katana_http_set_options(
        curl: *mut c_void,
        url: *const c_char,
        options: *const *const c_char,
        headers: *mut *mut c_void,
    ) -> c_int;
}
