//! A transfer through GDAL's HTTP client (cpl_http.h) and one on a libcurl
//! handle of the crate's own (curl/curl.h), as safe calls: moved here from
//! web_access.rs so that the fetch, its retries and its reading of headers
//! are safe code.
//!
//! Who frees what:
//!
//! ```text
//! HttpResult  what CPLHTTPFetchEx made; destroyed on drop
//!             (CPLHTTPDestroyResult)
//! CurlEasy    an easy handle of THE libcurl GDAL loads (ffi/curl.rs says
//!             why it must be); cleaned up on drop (curl_easy_cleanup)
//! CurlSlist   a header list libcurl allocated; freed on drop
//!             (curl_slist_free_all)
//! ```
//!
//! A transfer's callbacks reach Rust through a sink: one `&mut dyn` object
//! that GDAL or libcurl hands back to an extern "C" trampoline here, which
//! makes the bytes a slice and calls the sink's method. GDAL and libcurl call
//! a transfer's callbacks one at a time, on the thread that runs it, so one
//! `&mut` at a time reaches the sink. Each trampoline catches what a sink
//! lets escape and answers its failure value, which ends the transfer:
//! nothing unwinds into C. Every pointer a handle is given to this stack
//! frame (the sink, the error buffer, the header list) is taken off it again
//! before `perform` returns.

#![allow(
    unsafe_code,
    reason = "Fetches through GDAL's HTTP client and one libcurl handle; every call states why it is sound."
)]

use std::ffi::{CStr, c_char, c_int, c_long, c_void};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};

use super::cpl::{self, CStringList};
use super::curl::{self, CurlCode, CurlOffT};
use super::http;

/// The `count` bytes at `data`; none when there are none, whatever `data` is.
///
/// # Safety
///
/// `data` points at `count` readable bytes, which outlive the slice, when
/// `count` is not 0.
unsafe fn bytes_at<'a>(data: *const u8, count: usize) -> &'a [u8] {
    if count == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract.
        unsafe { std::slice::from_raw_parts(data, count) }
    }
}

// ---- GDAL's HTTP client ------------------------------------------------------------------

/// What a CPLHTTPFetchEx transfer tells Rust while it runs.
pub(crate) trait FetchSink {
    /// GDAL's progress: TRUE to go on, FALSE to cancel the transfer.
    fn progress(&mut self, fraction: f64) -> c_int;
    /// The body's next bytes: how many were taken; fewer than offered ends
    /// the transfer.
    fn write(&mut self, bytes: &[u8]) -> usize;
}

/// The `&mut dyn FetchSink` a fetch put on its stack, from the pointer GDAL
/// hands back; None for NULL.
///
/// # Safety
///
/// `data` is NULL or the pointer `fetch` handed GDAL, during that call, with
/// no other reference to the sink alive.
unsafe fn fetch_sink<'a>(data: *mut c_void) -> Option<&'a mut dyn FetchSink> {
    // SAFETY: the caller's contract: a pointer to a live `&mut dyn FetchSink`.
    unsafe { data.cast::<&mut dyn FetchSink>().as_mut() }.map(|sink| &mut **sink)
}

// GDAL's progress callback; FALSE cancels the transfer between packets.
extern "C" fn fetch_progress(fraction: f64, _message: *const c_char, data: *mut c_void) -> c_int {
    // SAFETY: `data` is the pointer `fetch` handed GDAL, alive until
    // CPLHTTPFetchEx returns; GDAL calls this and fetch_write one at a time,
    // on the fetching thread.
    let Some(sink) = (unsafe { fetch_sink(data) }) else {
        return cpl::FALSE;
    };
    catch_unwind(AssertUnwindSafe(|| sink.progress(fraction))).unwrap_or(cpl::FALSE)
}

// libcurl's write callback, through GDAL: the body is collected by the sink
// rather than in GDAL's own buffer, whose size is an int.
extern "C" fn fetch_write(buffer: *mut c_void, size: usize, count: usize, data: *mut c_void) -> usize {
    // SAFETY: as fetch_progress's.
    let Some(sink) = (unsafe { fetch_sink(data) }) else {
        return 0;
    };
    // SAFETY: GDAL hands `size * count` readable bytes at `buffer`, alive for
    // the call.
    let bytes = unsafe { bytes_at(buffer.cast::<u8>(), size.wrapping_mul(count)) };
    catch_unwind(AssertUnwindSafe(|| sink.write(bytes))).unwrap_or(0)
}

/// `CPLHTTPFetchEx(url, options, ...)`, the progress and the body handed to
/// `sink`: GDAL's result, or None when it made none (its reason left as this
/// thread's last error).
pub(crate) fn fetch(url: &CStr, options: &CStringList, sink: &mut dyn FetchSink) -> Option<HttpResult> {
    let mut sink: &mut dyn FetchSink = sink;
    let data = ptr::from_mut(&mut sink).cast::<c_void>();
    // SAFETY: a NUL-terminated URL and a NULL or NULL-terminated option list
    // alive for the call; the callbacks are this module's, each given the
    // pointer to the sink on this frame, which outlives the call and is not
    // touched by Rust until it returns. The result is owned by the value made.
    let result = unsafe {
        http::CPLHTTPFetchEx(
            url.as_ptr(),
            options.as_ptr(),
            Some(fetch_progress),
            data,
            Some(fetch_write),
            data,
        )
    };
    NonNull::new(result).map(HttpResult)
}

/// What CPLHTTPFetchEx made: read, then destroyed on drop.
pub(crate) struct HttpResult(NonNull<http::CplHttpResult>);

impl HttpResult {
    fn result(&self) -> &http::CplHttpResult {
        // SAFETY: the result GDAL made, alive until this value destroys it,
        // and not changed by GDAL after the fetch returned.
        unsafe { self.0.as_ref() }
    }

    /// libcurl's CURLcode, which GDAL passes through: 0 for success.
    pub(crate) fn status(&self) -> c_int {
        self.result().n_status
    }

    /// GDAL's error text; empty for none.
    pub(crate) fn error_text(&self) -> String {
        // SAFETY: NULL or a NUL-terminated text of the live result.
        unsafe { cpl::decoded(self.result().psz_err_buf) }
    }

    /// The Content-Type the server sent; empty for none.
    pub(crate) fn content_type(&self) -> String {
        // SAFETY: NULL or a NUL-terminated text of the live result.
        unsafe { cpl::decoded(self.result().psz_content_type) }
    }
}

impl Drop for HttpResult {
    fn drop(&mut self) {
        // SAFETY: the result GDAL made, freed once and not read after.
        unsafe { http::CPLHTTPDestroyResult(self.0.as_ptr()) };
    }
}

// ---- one libcurl handle -------------------------------------------------------------------

/// What one exchange on a CurlEasy tells Rust while it runs.
pub(crate) trait ExchangeSink {
    /// One header line, as libcurl hands it (with its CR LF): how many bytes
    /// were taken; fewer than offered ends the transfer.
    fn header(&mut self, line: &[u8]) -> usize;
    /// The body's next bytes: how many were taken; fewer ends the transfer.
    fn body(&mut self, bytes: &[u8]) -> usize;
    /// libcurl's progress, download total and done: non-zero aborts.
    fn progress(&mut self, download_total: CurlOffT, downloaded: CurlOffT) -> c_int;
}

/// The `&mut dyn ExchangeSink` `perform` put on its stack; None for NULL.
///
/// # Safety
///
/// `data` is NULL or the pointer `perform` handed libcurl, during
/// curl_easy_perform, with no other reference to the sink alive.
unsafe fn exchange_sink<'a>(data: *mut c_void) -> Option<&'a mut dyn ExchangeSink> {
    // SAFETY: the caller's contract: a pointer to a live `&mut dyn ExchangeSink`.
    unsafe { data.cast::<&mut dyn ExchangeSink>().as_mut() }.map(|sink| &mut **sink)
}

extern "C" fn exchange_header(buffer: *mut c_char, size: usize, count: usize, data: *mut c_void) -> usize {
    // SAFETY: `data` is the pointer `perform` handed libcurl, alive until
    // curl_easy_perform returns; libcurl calls the callbacks one at a time,
    // on the performing thread.
    let Some(sink) = (unsafe { exchange_sink(data) }) else {
        return 0;
    };
    // SAFETY: libcurl hands `size * count` readable bytes at `buffer`.
    let line = unsafe { bytes_at(buffer.cast::<u8>(), size.wrapping_mul(count)) };
    catch_unwind(AssertUnwindSafe(|| sink.header(line))).unwrap_or(0)
}

extern "C" fn exchange_body(buffer: *mut c_char, size: usize, count: usize, data: *mut c_void) -> usize {
    // SAFETY: as exchange_header's.
    let Some(sink) = (unsafe { exchange_sink(data) }) else {
        return 0;
    };
    // SAFETY: libcurl hands `size * count` readable bytes at `buffer`.
    let bytes = unsafe { bytes_at(buffer.cast::<u8>(), size.wrapping_mul(count)) };
    catch_unwind(AssertUnwindSafe(|| sink.body(bytes))).unwrap_or(0)
}

extern "C" fn exchange_progress(
    data: *mut c_void,
    download_total: CurlOffT,
    downloaded: CurlOffT,
    _upload_total: CurlOffT,
    _uploaded: CurlOffT,
) -> c_int {
    // SAFETY: as exchange_header's.
    let Some(sink) = (unsafe { exchange_sink(data) }) else {
        return 1;
    };
    catch_unwind(AssertUnwindSafe(|| sink.progress(download_total, downloaded))).unwrap_or(1)
}

/// A libcurl easy handle, cleaned up on drop. Not Send: it is made, used and
/// cleaned up by one exchange on one thread.
pub(crate) struct CurlEasy {
    handle: NonNull<curl::Curl>,
    _thread: PhantomData<*const ()>,
}

impl CurlEasy {
    /// `curl_easy_init()`: None when libcurl cannot start one.
    pub(crate) fn new() -> Option<Self> {
        // SAFETY: no argument; the handle (or NULL) is owned by the value made.
        let handle = unsafe { curl::curl_easy_init() };
        NonNull::new(handle).map(|handle| Self {
            handle,
            _thread: PhantomData,
        })
    }

    fn raw(&self) -> *mut curl::Curl {
        self.handle.as_ptr()
    }

    /// `curl_easy_setopt(curl, option, (long)value)`.
    pub(crate) fn set_long(&mut self, option: curl::LongOption, value: c_long) -> CurlCode {
        // SAFETY: a live handle and the long this option is read as.
        unsafe { curl::set_long(self.raw(), option, value) }
    }

    /// `curl_easy_setopt(curl, option, NULL)`: the option reset to libcurl's
    /// default, which for every pointer option is "none".
    pub(crate) fn clear(&mut self, option: curl::PointerOption) -> CurlCode {
        // SAFETY: a live handle, and NULL, which every pointer option takes.
        unsafe { curl::set_pointer(self.raw(), option, ptr::null()) }
    }

    /// `curl_easy_setopt(curl, CURLOPT_URL, url)`: libcurl copies the text.
    pub(crate) fn set_url(&mut self, url: &CStr) -> CurlCode {
        // SAFETY: a live handle and a NUL-terminated string, which libcurl
        // copies (a string option, curl_easy_setopt(3)).
        unsafe { curl::set_pointer(self.raw(), curl::CURLOPT_URL, url.as_ptr().cast()) }
    }

    /// `CPLHTTPSetOptions(curl, url, options)` through the shim: GDAL's own
    /// option handling applied to this handle. The header list GDAL returns
    /// for the caller to attach is freed unattached. True when
    /// CPLHTTPSetOptions threw (the shim caught it).
    pub(crate) fn apply_gdal_options(&mut self, url: &CStr, options: &CStringList) -> bool {
        let mut gdal_headers: *mut c_void = ptr::null_mut();
        // SAFETY: a live handle of the libcurl GDAL loads, a NUL-terminated
        // URL and option list alive for the call, and the place for GDAL's list.
        let thrown = unsafe {
            http::katana_http_set_options(
                self.raw().cast(),
                url.as_ptr(),
                options.as_ptr(),
                &raw mut gdal_headers,
            )
        };
        // SAFETY: NULL or the curl_slist GDAL allocated for the caller, not
        // attached to the handle, freed once.
        unsafe { curl::curl_slist_free_all(gdal_headers.cast()) };
        thrown != 0
    }

    /// `curl_easy_perform` with `headers` sent, libcurl's message written
    /// into an error buffer of this frame, and the headers, the body and the
    /// progress handed to `sink`: the code, and libcurl's message for it
    /// (its error buffer's, else curl_easy_strerror's). Every pointer to
    /// this frame is taken off the handle again before this returns.
    pub(crate) fn perform(
        &mut self,
        headers: &CurlSlist,
        sink: &mut dyn ExchangeSink,
    ) -> (CurlCode, String) {
        let handle = self.raw();
        let mut error_buffer: [c_char; curl::CURL_ERROR_SIZE] = [0; curl::CURL_ERROR_SIZE];
        let mut sink: &mut dyn ExchangeSink = sink;
        let data = ptr::from_mut(&mut sink).cast::<c_void>().cast_const();
        // SAFETY: a live handle, and every pointer set is alive until it is
        // taken off below: the header list (borrowed for the call), the
        // error buffer of CURL_ERROR_SIZE bytes, and the sink, which Rust
        // does not touch until curl_easy_perform returns; each callback is
        // this module's, for that sink.
        unsafe {
            curl::set_pointer(handle, curl::CURLOPT_HTTPHEADER, headers.raw().cast_const().cast());
            curl::set_pointer(
                handle,
                curl::CURLOPT_ERRORBUFFER,
                error_buffer.as_mut_ptr().cast_const().cast(),
            );
            curl::set_write_function(handle, curl::CURLOPT_HEADERFUNCTION, exchange_header);
            curl::set_pointer(handle, curl::CURLOPT_HEADERDATA, data);
            curl::set_write_function(handle, curl::CURLOPT_WRITEFUNCTION, exchange_body);
            curl::set_pointer(handle, curl::CURLOPT_WRITEDATA, data);
            curl::set_xferinfo_function(handle, exchange_progress);
            curl::set_pointer(handle, curl::CURLOPT_XFERINFODATA, data);
        }
        // SAFETY: the handle configured above.
        let code = unsafe { curl::curl_easy_perform(handle) };
        // libcurl ends its message with a NUL; the last byte makes sure of it.
        error_buffer[curl::CURL_ERROR_SIZE - 1] = 0;
        let message = if error_buffer[0] != 0 {
            // SAFETY: a NUL-terminated buffer.
            unsafe { cpl::decoded(error_buffer.as_ptr()) }
        } else {
            // SAFETY: libcurl's own text for the code, static.
            unsafe { cpl::decoded(curl::curl_easy_strerror(code)) }
        };
        // SAFETY: a live handle; NULL detaches each pointer to this frame
        // (the callbacks answer a failure for NULL data, and none is called
        // without a perform).
        unsafe {
            curl::set_pointer(handle, curl::CURLOPT_HTTPHEADER, ptr::null());
            curl::set_pointer(handle, curl::CURLOPT_ERRORBUFFER, ptr::null());
            curl::set_pointer(handle, curl::CURLOPT_HEADERDATA, ptr::null());
            curl::set_pointer(handle, curl::CURLOPT_WRITEDATA, ptr::null());
            curl::set_pointer(handle, curl::CURLOPT_XFERINFODATA, ptr::null());
        }
        (code, message)
    }

    /// `curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &value)`: the last
    /// response's status, when libcurl has one to give.
    pub(crate) fn response_code(&mut self) -> Option<c_long> {
        let mut value: c_long = 0;
        // SAFETY: a live handle, and the long the status is written to.
        (unsafe { curl::response_code(self.raw(), &mut value) } == curl::CURLE_OK).then_some(value)
    }
}

impl Drop for CurlEasy {
    fn drop(&mut self) {
        // SAFETY: the handle this value owns, cleaned up once; nothing on it
        // points at a Rust frame any more (perform took each pointer off).
        unsafe { curl::curl_easy_cleanup(self.raw()) };
    }
}

/// A list of request headers libcurl allocated; NULL is the empty list.
/// Freed on drop.
pub(crate) struct CurlSlist {
    list: *mut curl::CurlSlist,
}

impl CurlSlist {
    pub(crate) fn new() -> Self {
        Self {
            list: ptr::null_mut(),
        }
    }

    fn raw(&self) -> *mut curl::CurlSlist {
        self.list
    }

    /// `curl_slist_append(list, text)`: false, the list as it was, when
    /// libcurl could not add it.
    pub(crate) fn append(&mut self, text: &CStr) -> bool {
        // SAFETY: the list so far (or NULL) and a NUL-terminated string,
        // which libcurl copies.
        let longer = unsafe { curl::curl_slist_append(self.list, text.as_ptr()) };
        if longer.is_null() {
            return false;
        }
        self.list = longer;
        true
    }
}

impl Drop for CurlSlist {
    fn drop(&mut self) {
        // SAFETY: NULL or the list libcurl allocated for this value, freed
        // once, and not attached to a handle (perform detaches it).
        unsafe { curl::curl_slist_free_all(self.list) };
    }
}
