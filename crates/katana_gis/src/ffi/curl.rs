//! libcurl (curl/curl.h, curl/easy.h): exactly the functions, options and
//! codes `gis::http_exchange` uses, written by hand from the headers of libcurl
//! 8.22, the toolchain's (ffi/cpl.rs says how the ffi folder is laid out and
//! checked; this file's check is native/http_check.c).
//!
//! THE library GDAL loads, never a second copy: `http_exchange` hands GDAL's
//! `CPLHTTPSetOptions` a handle this file's `curl_easy_init` made, and a handle
//! made by one copy of libcurl and configured by another is not a handle. So
//! the block links `curl` by name - libcurl.so.4 from the prefix in WSL,
//! libcurl-4.dll on Windows, both GDAL's own - and no curl crate is used: the
//! curl-sys crate builds a static libcurl of its own when pkg-config does not
//! find one (the step-5 plan, section 4).
//!
//! THE VARIADIC CALLS. `curl_easy_setopt` and `curl_easy_getinfo` are C
//! variadics, and are declared as such (`...`): a variadic function called
//! through a non-variadic declaration is undefined, and differs in fact on
//! some platforms (Apple's arm64 passes variadic arguments on the stack). They
//! are called only through the typed wrappers below, whose option constants
//! carry the C type libcurl reads for them - `LongOption` a `long`,
//! `PointerOption` a pointer (an object, a string, a list or a callback's
//! data), `WriteFunctionOption` a `curl_write_callback`, the progress option a
//! `curl_xferinfo_callback` - so a pointer is never passed where libcurl reads
//! a long. Each constant's number is libcurl's (checked by native/http_check.c)
//! and lies in the range of its type class (checked below): libcurl numbers an
//! option by its type, CURLOPTTYPE_LONG + n, CURLOPTTYPE_OBJECTPOINT + n and
//! so on.

#![allow(
    unsafe_code,
    reason = "The declarations of libcurl's C API and the typed calls of its variadic functions; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's codes are declared whole where the C++ names them; native/http_check.c holds each."
)]

use std::ffi::{c_char, c_int, c_long, c_void};
use std::marker::{PhantomData, PhantomPinned};

/// `CURL`, the easy handle (`typedef void CURL` in C).
#[repr(C)]
pub(crate) struct Curl {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `struct curl_slist`: only ever handed back to libcurl.
#[repr(C)]
pub(crate) struct CurlSlist {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `CURLcode`.
pub(crate) type CurlCode = c_int;
pub(crate) const CURLE_OK: CurlCode = 0;
pub(crate) const CURLE_FAILED_INIT: CurlCode = 2;
pub(crate) const CURLE_COULDNT_RESOLVE_PROXY: CurlCode = 5;
pub(crate) const CURLE_COULDNT_RESOLVE_HOST: CurlCode = 6;
pub(crate) const CURLE_COULDNT_CONNECT: CurlCode = 7;
pub(crate) const CURLE_PARTIAL_FILE: CurlCode = 18;
pub(crate) const CURLE_WRITE_ERROR: CurlCode = 23;
pub(crate) const CURLE_OUT_OF_MEMORY: CurlCode = 27;
pub(crate) const CURLE_OPERATION_TIMEDOUT: CurlCode = 28;
pub(crate) const CURLE_GOT_NOTHING: CurlCode = 52;
pub(crate) const CURLE_SEND_ERROR: CurlCode = 55;
pub(crate) const CURLE_RECV_ERROR: CurlCode = 56;
pub(crate) const CURLE_PEER_FAILED_VERIFICATION: CurlCode = 60;
pub(crate) const CURLE_SSL_CACERT_BADFILE: CurlCode = 77;

/// `CURLoption`.
pub(crate) type CurlOption = c_int;
/// `CURLINFO`.
pub(crate) type CurlInfo = c_int;
/// `curl_off_t`: 64 bits on every platform the port builds on (curl/system.h:
/// `long` on LP64, `long long` or `__int64` on Windows).
pub(crate) type CurlOffT = i64;

/// The size `CURLOPT_ERRORBUFFER`'s buffer must have.
pub(crate) const CURL_ERROR_SIZE: usize = 256;
/// `CURL_NETRC_IGNORED`: the .netrc is never read.
pub(crate) const CURL_NETRC_IGNORED: c_long = 0;
/// `CURLAUTH_NONE`: no authentication scheme of libcurl's own.
pub(crate) const CURLAUTH_NONE: c_long = 0;

// The type classes an option's number is made in (curl.h, CURLOPT()): the
// class plus a number under 10000.
const CURLOPTTYPE_LONG: CurlOption = 0;
const CURLOPTTYPE_OBJECTPOINT: CurlOption = 10000;
const CURLOPTTYPE_FUNCTIONPOINT: CurlOption = 20000;

const fn type_class(number: CurlOption) -> CurlOption {
    number - number % 10000
}

/// An option libcurl reads a `long` for (CURLOPTTYPE_LONG, CURLOPTTYPE_VALUES).
#[derive(Clone, Copy)]
pub(crate) struct LongOption(CurlOption);

/// An option libcurl reads a pointer for: an object, a string, a list or a
/// callback's data (CURLOPTTYPE_OBJECTPOINT and its aliases STRINGPOINT,
/// SLISTPOINT and CBPOINT).
#[derive(Clone, Copy)]
pub(crate) struct PointerOption(CurlOption);

/// An option libcurl reads a `curl_write_callback` for: the body's and the
/// headers' (CURLOPTTYPE_FUNCTIONPOINT).
#[derive(Clone, Copy)]
pub(crate) struct WriteFunctionOption(CurlOption);

const fn long_option(number: CurlOption) -> LongOption {
    assert!(type_class(number) == CURLOPTTYPE_LONG);
    LongOption(number)
}

const fn pointer_option(number: CurlOption) -> PointerOption {
    assert!(type_class(number) == CURLOPTTYPE_OBJECTPOINT);
    PointerOption(number)
}

const fn write_function_option(number: CurlOption) -> WriteFunctionOption {
    assert!(type_class(number) == CURLOPTTYPE_FUNCTIONPOINT);
    WriteFunctionOption(number)
}

// Each constant is evaluated where it is defined, so a number outside its
// type class fails the build.
pub(crate) const CURLOPT_WRITEDATA: PointerOption = pointer_option(10001);
pub(crate) const CURLOPT_URL: PointerOption = pointer_option(10002);
pub(crate) const CURLOPT_USERPWD: PointerOption = pointer_option(10005);
pub(crate) const CURLOPT_ERRORBUFFER: PointerOption = pointer_option(10010);
pub(crate) const CURLOPT_WRITEFUNCTION: WriteFunctionOption = write_function_option(20011);
pub(crate) const CURLOPT_COOKIE: PointerOption = pointer_option(10022);
pub(crate) const CURLOPT_HTTPHEADER: PointerOption = pointer_option(10023);
pub(crate) const CURLOPT_SSLCERT: PointerOption = pointer_option(10025);
pub(crate) const CURLOPT_KEYPASSWD: PointerOption = pointer_option(10026);
pub(crate) const CURLOPT_HEADERDATA: PointerOption = pointer_option(10029);
pub(crate) const CURLOPT_COOKIEFILE: PointerOption = pointer_option(10031);
pub(crate) const CURLOPT_VERBOSE: LongOption = long_option(41);
pub(crate) const CURLOPT_NOPROGRESS: LongOption = long_option(43);
pub(crate) const CURLOPT_NETRC: LongOption = long_option(51);
pub(crate) const CURLOPT_FOLLOWLOCATION: LongOption = long_option(52);
pub(crate) const CURLOPT_XFERINFODATA: PointerOption = pointer_option(10057);
pub(crate) const CURLOPT_SSL_VERIFYPEER: LongOption = long_option(64);
pub(crate) const CURLOPT_HEADERFUNCTION: WriteFunctionOption = write_function_option(20079);
pub(crate) const CURLOPT_HTTPGET: LongOption = long_option(80);
pub(crate) const CURLOPT_SSL_VERIFYHOST: LongOption = long_option(81);
pub(crate) const CURLOPT_COOKIEJAR: PointerOption = pointer_option(10082);
pub(crate) const CURLOPT_SSLKEY: PointerOption = pointer_option(10087);
pub(crate) const CURLOPT_NOSIGNAL: LongOption = long_option(99);
pub(crate) const CURLOPT_ACCEPT_ENCODING: PointerOption = pointer_option(10102);
pub(crate) const CURLOPT_HTTPAUTH: LongOption = long_option(107);
pub(crate) const CURLOPT_NETRC_FILE: PointerOption = pointer_option(10118);
/// CURLOPTTYPE_FUNCTIONPOINT + 219; set only through `set_xferinfo_function`.
const CURLOPT_XFERINFOFUNCTION: CurlOption = 20219;
pub(crate) const CURLOPT_XOAUTH2_BEARER: PointerOption = pointer_option(10220);

const _: () = assert!(type_class(CURLOPT_XFERINFOFUNCTION) == CURLOPTTYPE_FUNCTIONPOINT);

/// `CURLINFO_LONG + 2`: the last response's status, read into a `long`.
pub(crate) const CURLINFO_RESPONSE_CODE: CurlInfo = 0x200000 + 2;

/// `curl_write_callback`, the body's and the headers' callback.
pub(crate) type CurlWriteCallback = unsafe extern "C" fn(
    buffer: *mut c_char,
    size: usize,
    count: usize,
    data: *mut c_void,
) -> usize;

/// `curl_xferinfo_callback`: download total and done, upload total and done.
pub(crate) type CurlXferinfoCallback = unsafe extern "C" fn(
    data: *mut c_void,
    download_total: CurlOffT,
    downloaded: CurlOffT,
    upload_total: CurlOffT,
    uploaded: CurlOffT,
) -> c_int;

#[link(name = "curl")]
unsafe extern "C" {
    pub(crate) fn curl_easy_init() -> *mut Curl;
    fn curl_easy_setopt(curl: *mut Curl, option: CurlOption, ...) -> CurlCode;
    pub(crate) fn curl_easy_perform(curl: *mut Curl) -> CurlCode;
    fn curl_easy_getinfo(curl: *mut Curl, info: CurlInfo, ...) -> CurlCode;
    pub(crate) fn curl_easy_cleanup(curl: *mut Curl);
    pub(crate) fn curl_easy_strerror(code: CurlCode) -> *const c_char;
    pub(crate) fn curl_slist_append(list: *mut CurlSlist, text: *const c_char) -> *mut CurlSlist;
    pub(crate) fn curl_slist_free_all(list: *mut CurlSlist);
}

/// `curl_easy_setopt(curl, option, (long)value)`.
///
/// # Safety
///
/// `curl` is a live easy handle.
pub(crate) unsafe fn set_long(curl: *mut Curl, option: LongOption, value: c_long) -> CurlCode {
    // SAFETY: a live handle (the caller's contract) and the long this option
    // is read as.
    unsafe { curl_easy_setopt(curl, option.0, value) }
}

/// `curl_easy_setopt(curl, option, pointer)`; NULL resets the option.
///
/// # Safety
///
/// `curl` is a live easy handle, and `value` is what libcurl reads for this
/// option - NULL, or a string, list, buffer or callback datum that outlives
/// the handle's use of it (a string option libcurl copies).
pub(crate) unsafe fn set_pointer(
    curl: *mut Curl,
    option: PointerOption,
    value: *const c_void,
) -> CurlCode {
    // SAFETY: the caller's contract; a pointer is what this option is read as.
    unsafe { curl_easy_setopt(curl, option.0, value) }
}

/// `curl_easy_setopt(curl, option, callback)` for the body's and the headers'
/// callback.
///
/// # Safety
///
/// `curl` is a live easy handle, and the callback's data option is set to
/// what `callback` expects before the transfer runs.
pub(crate) unsafe fn set_write_function(
    curl: *mut Curl,
    option: WriteFunctionOption,
    callback: CurlWriteCallback,
) -> CurlCode {
    // SAFETY: the caller's contract; a curl_write_callback is what this option
    // is read as.
    unsafe { curl_easy_setopt(curl, option.0, callback) }
}

/// `curl_easy_setopt(curl, CURLOPT_XFERINFOFUNCTION, callback)`.
///
/// # Safety
///
/// `curl` is a live easy handle, and CURLOPT_XFERINFODATA is set to what
/// `callback` expects before the transfer runs.
pub(crate) unsafe fn set_xferinfo_function(
    curl: *mut Curl,
    callback: CurlXferinfoCallback,
) -> CurlCode {
    // SAFETY: the caller's contract; a curl_xferinfo_callback is what this
    // option is read as.
    unsafe { curl_easy_setopt(curl, CURLOPT_XFERINFOFUNCTION, callback) }
}

/// `curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &value)`.
///
/// # Safety
///
/// `curl` is a live easy handle.
pub(crate) unsafe fn response_code(curl: *mut Curl, value: &mut c_long) -> CurlCode {
    let value: *mut c_long = value;
    // SAFETY: a live handle (the caller's contract) and a pointer to the long
    // this information is written as.
    unsafe { curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, value) }
}
