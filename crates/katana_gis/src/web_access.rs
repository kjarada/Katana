//! Reaching web services through GDAL, behind a Katana interface
//! (include/katana/gis/web_access.hpp, src/katana_io/web_access.cpp).
//!
//! GDAL already carries an HTTP client - libcurl, with the proxy, certificate
//! and retry handling every one of its web drivers (WMS, WMTS, WFS, OAPIF,
//! /vsicurl/) uses - so Katana brings no second one. A request goes through
//! CPLHTTPFetchEx, which means the proxy a person configured for GDAL
//! (GDAL_HTTP_PROXY, or the ordinary HTTPS_PROXY environment libcurl reads) and
//! the certificate bundle it trusts (CURL_CA_BUNDLE, SSL_CERT_FILE) apply to
//! Katana's own requests and to GDAL's reads alike: one network stack, one set
//! of settings to get right.
//!
//! The one request CPLHTTPFetchEx cannot make - a request that does not follow
//! a redirect, `http_exchange` below - is made on a libcurl handle of this
//! module's own, which GDAL's CPLHTTPSetOptions configures. For that the crate
//! links libcurl by name (ffi/curl.rs), and it must be THE libcurl GDAL itself
//! loads: a handle made by one copy of the library and configured by another
//! is not a handle.
//!
//! Nothing here knows what a web map service IS. Building the URLs, paging,
//! caching and choosing what to ask for belongs to katana_interop (online_*);
//! this module moves bytes, parses XML into plain values and hashes, and names
//! no GDAL type.
//!
//! Threading: every function is safe to call from any thread; a fetch blocks
//! its caller until it ends, is cancelled, or times out, which is why the
//! desktop application calls it from a background job and never from the GUI
//! thread.
//!
//! The Rust shape. The C++'s `std::stop_token` is `Option<&AtomicBool>`, as
//! katana_cad's drape and reproject's warp have it (None: no stop can come),
//! and its `TransferProgress` std::function a borrowed closure used only
//! during the call: GDAL and libcurl call it on the fetching thread. A body is
//! bytes (`Vec<u8>`), not text. What GDAL and libcurl say - a message, a
//! content type, a header - is decoded lossily at the boundary. Every callback
//! GDAL or libcurl calls catches every panic: the transfer is ended there and
//! the panic raised again once the call has returned (a panic of a progress
//! closure, which the C++ would let unwind through C).

#![allow(
    unsafe_code,
    reason = "Fetches through GDAL's HTTP client and one libcurl handle, and reads GDAL's XML tree; every call states why it is sound."
)]

use std::any::Any;
use std::collections::BTreeMap;
use std::ffi::{c_char, c_int, c_long, c_void};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, lowered, trimmed};
use katana_core::{Error, ErrorCode, Result};

use crate::cpl_error_collector::quiet_errors;
use crate::ffi::curl::{self, CurlOffT};
use crate::ffi::{cpl, http, minixml};
use crate::gdal_registry::ensure_registered;

const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;

/// What a bounded copy of an error page is worth: enough to name the problem,
/// never a download.
const ERROR_BODY_BYTES: u64 = 64 * 1024;

// ---- the request and its answers --------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub url: String,
    /// Non-empty makes the request a POST with this body (a STAC search, an
    /// Overpass query too long for a URL).
    pub post_body: String,
    pub post_content_type: String,
    /// Extra request headers, "Name: value" each. A key a service wants in a
    /// header goes here and so never appears in a URL that might be logged.
    pub headers: Vec<String>,
    /// Every request names its program: the OpenStreetMap tile and Overpass
    /// policies require a User-Agent that identifies the application, and a
    /// publisher who sees trouble in its logs should be able to tell whose.
    pub user_agent: String,
    /// Seconds for the whole transfer, and for the connection alone. Both
    /// bounded, always: a service that accepts a connection and then sends
    /// nothing would otherwise hold a job for ever.
    pub timeout_seconds: i32,
    pub connect_timeout_seconds: i32,
    /// Attempts after the first, for the failures worth another try - 429 and
    /// 5xx answers, timeouts, a dropped connection - with the delay doubling
    /// from retry_delay_seconds each time. A 4xx other than 429 is the
    /// request's own fault and is never retried.
    pub retries: i32,
    pub retry_delay_seconds: f64,
    /// The largest body accepted. The transfer is abandoned the moment it is
    /// exceeded, so an unexpected multi-gigabyte answer costs this much memory
    /// and no more.
    pub max_bytes: u64,
}

impl Default for HttpRequest {
    fn default() -> Self {
        Self {
            url: String::new(),
            post_body: String::new(),
            post_content_type: String::from("application/json"),
            headers: Vec::new(),
            user_agent: String::new(),
            timeout_seconds: 120,
            connect_timeout_seconds: 20,
            retries: 3,
            retry_delay_seconds: 1.0,
            max_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpResponse {
    /// 200, or 0 for a file:// URL.
    pub status: i32,
    pub content_type: String,
    /// Bytes, not necessarily text.
    pub body: Vec<u8>,
}

/// Bytes received so far and the total the server announced (0 when it did
/// not). Called from the fetching thread, during the call only.
pub type TransferProgress<'a> = &'a dyn Fn(u64, u64);

/// ONE exchange, as the wire had it (see `http_exchange`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpExchange {
    /// The HTTP status of the last response on the connection.
    pub status: i32,
    /// "" when none.
    pub content_type: String,
    /// The Location header, "" when none.
    pub location: String,
    /// The Content-Range header, "" when none.
    pub content_range: String,
    pub body: Vec<u8>,
    /// The answer was longer than its bound and was ended there: `body` is
    /// empty for a 200 or 206, and the first 64 KiB for any other status.
    pub truncated: bool,
}

/// A transfer that failed after its retries, as `http_fetch` saw it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpFailure {
    /// The HTTP status, 0 when none arrived.
    pub status: i32,
    /// libcurl's CURLcode, 0 for none.
    pub curl_code: i32,
    /// libcurl's (or GDAL's) message.
    pub error: String,
    /// What the server sent, if anything.
    pub body: Vec<u8>,
    pub timeout_seconds: i32,
}

// ---- what the transfers share with their callbacks ------------------------------------

fn stop_requested(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn cancelled(shown: &str) -> Error {
    make_error(ErrorCode::InvalidState, "cancelled", shown)
}

/// std::max(0.1, seconds): gis does not see katana_math, whose `cpp::max` this is.
fn retry_delay(seconds: f64) -> f64 {
    if 0.1 < seconds { seconds } else { 0.1 }
}

/// A state a callback works on, with the place a panic of its work is kept
/// until the transfer has returned.
trait CallbackState {
    fn panicked(&mut self) -> &mut Option<Box<dyn Any + Send>>;
}

/// A callback's `work` on the state GDAL or libcurl hands back, every panic
/// caught: it is kept on the state, and the callback answers `failed`, which
/// ends the transfer. Nothing unwinds into C.
///
/// # Safety
///
/// `state` is a live `S` that nothing else touches during the call.
unsafe fn guarded<S: CallbackState, R>(
    state: *mut c_void,
    failed: R,
    work: impl FnOnce(&mut S) -> R,
) -> R {
    // SAFETY: the caller's contract.
    let state = unsafe { &mut *state.cast::<S>() };
    match catch_unwind(AssertUnwindSafe(|| work(&mut *state))) {
        Ok(answer) => answer,
        Err(payload) => {
            state.panicked().get_or_insert(payload);
            failed
        }
    }
}

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

fn last_error_message() -> String {
    // SAFETY: GDAL's last message on this thread, copied before anything else
    // calls GDAL.
    unsafe { cpl::decoded(cpl::CPLGetLastErrorMsg()) }
}

// What a transfer's callbacks share with the thread that started it.
struct TransferState<'a> {
    stop: Option<&'a AtomicBool>,
    progress: Option<TransferProgress<'a>>,
    max_bytes: u64,
    body: Vec<u8>,
    too_large: bool,
    panic: Option<Box<dyn Any + Send>>,
}

impl CallbackState for TransferState<'_> {
    fn panicked(&mut self) -> &mut Option<Box<dyn Any + Send>> {
        &mut self.panic
    }
}

// libcurl's write callback, through GDAL: the body is collected here rather
// than in GDAL's own buffer, whose size is an int, so the limit is ours and is
// enforced as the bytes arrive. Returning less than was offered aborts the
// transfer.
extern "C" fn collect(buffer: *mut c_void, size: usize, count: usize, state: *mut c_void) -> usize {
    let bytes = size.wrapping_mul(count);
    let work = |transfer: &mut TransferState<'_>| {
        if transfer.body.len() as u64 + bytes as u64 > transfer.max_bytes {
            transfer.too_large = true;
            return 0;
        }
        if stop_requested(transfer.stop) {
            return 0;
        }
        // SAFETY: GDAL hands `bytes` readable bytes at `buffer`.
        transfer
            .body
            .extend_from_slice(unsafe { bytes_at(buffer.cast::<u8>(), bytes) });
        bytes
    };
    // SAFETY: `state` is the TransferState http_fetch handed GDAL for this
    // transfer, alive until CPLHTTPFetchEx returns; GDAL calls this callback
    // and report_transfer one at a time, on the fetching thread.
    unsafe { guarded(state, 0, work) }
}

// GDAL's progress callback; returning FALSE is how a transfer is cancelled
// between packets, so a stopped job does not wait out a slow download.
extern "C" fn report_transfer(fraction: f64, _message: *const c_char, state: *mut c_void) -> c_int {
    let work = |transfer: &mut TransferState<'_>| {
        if let Some(progress) = transfer.progress {
            let received = transfer.body.len() as u64;
            let total = if fraction > 0.0 {
                (received as f64 / fraction) as u64
            } else {
                0
            };
            progress(received, total);
        }
        if stop_requested(transfer.stop) {
            cpl::FALSE
        } else {
            cpl::TRUE
        }
    };
    // SAFETY: as collect's.
    unsafe { guarded(state, cpl::FALSE, work) }
}

// GDAL reports an HTTP failure in its error text ("HTTP error code : 404"),
// not a field: the status is read back out of it.
fn status_from_error(text: &str) -> i32 {
    let Some(at) = text.find("HTTP error code") else {
        return 0;
    };
    let bytes = text.as_bytes();
    let Some(mut digit) = bytes[at..]
        .iter()
        .position(u8::is_ascii_digit)
        .map(|offset| offset + at)
    else {
        return 0;
    };
    let mut code = 0;
    while digit < bytes.len() && bytes[digit].is_ascii_digit() && code < 1000 {
        code = code * 10 + i32::from(bytes[digit] - b'0');
        digit += 1;
    }
    code
}

// Whether another attempt could succeed: rate limiting, a server's own
// trouble, and transport failures (a timeout, a reset). A 4xx is the
// request's fault and the same request would fail the same way.
fn worth_retrying(status: i32, curl_code: i32) -> bool {
    if status == 429 || (500..600).contains(&status) {
        return true;
    }
    if status != 0 {
        return false;
    }
    // curl.h's numbering, which GDAL passes through as nStatus.
    matches!(
        curl_code,
        curl::CURLE_COULDNT_CONNECT
            | curl::CURLE_PARTIAL_FILE
            | curl::CURLE_OPERATION_TIMEDOUT
            | curl::CURLE_GOT_NOTHING
            | curl::CURLE_SEND_ERROR
            | curl::CURLE_RECV_ERROR
    )
}

// Sleeps `seconds`, waking early if a stop is requested. True when it slept
// the whole time.
fn pause(seconds: f64, stop: Option<&AtomicBool>) -> bool {
    // static_cast<long long>(seconds * 1000.0) milliseconds; a time past
    // Instant's range is waited for until a stop.
    let milliseconds = u64::try_from((seconds * 1000.0) as i64).unwrap_or(0);
    let until = Instant::now().checked_add(Duration::from_millis(milliseconds));
    while until.is_none_or(|until| Instant::now() < until) {
        if stop_requested(stop) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !stop_requested(stop)
}

// The path a file:// URL names: "file:///C:/x" and "file:///home/x" both,
// with %XX escapes decoded.
fn path_of_file_url(url: &str) -> PathBuf {
    let mut rest = &url.as_bytes()[7..]; // after "file://"
    if rest.starts_with(b"localhost/") {
        rest = &rest[9..];
    }
    let hex = |byte: u8| char::from(byte).to_digit(16);
    let mut decoded = Vec::with_capacity(rest.len());
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == b'%'
            && i + 2 < rest.len()
            && let (Some(high), Some(low)) = (hex(rest[i + 1]), hex(rest[i + 2]))
        {
            decoded.push((high * 16 + low) as u8);
            i += 3;
            continue;
        }
        decoded.push(rest[i]);
        i += 1;
    }
    // "/C:/data/x.tif" is a Windows drive path with the URL's leading slash.
    if decoded.len() > 2 && decoded[0] == b'/' && decoded[2] == b':' {
        decoded.remove(0);
    }
    path_of_bytes(decoded)
}

/// `std::filesystem::path(std::u8string(bytes))`: a POSIX path's own bytes.
#[cfg(unix)]
fn path_of_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

/// `std::filesystem::path(std::u8string(bytes))`: UTF-8, as Windows text.
#[cfg(not(unix))]
fn path_of_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_file_url(request: &HttpRequest) -> Result<HttpResponse> {
    let path = path_of_file_url(&request.url);
    if !path.is_file() {
        return Err(make_error(
            ErrorCode::NotFound,
            "no file at this file URL",
            request.url.as_str(),
        ));
    }
    let unreadable = || {
        make_error(
            ErrorCode::FileImportFailure,
            "could not read the file URL",
            request.url.as_str(),
        )
    };
    let size = std::fs::metadata(&path).map_err(|_| unreadable())?.len();
    if size > request.max_bytes {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "the answer is larger than the {} MB this request allows; ask for a smaller area",
                request.max_bytes / (1024 * 1024)
            ),
            request.url.as_str(),
        ));
    }
    // Exactly `size` bytes, as the C++'s read of `size` from the stream.
    let mut body = vec![0u8; size as usize];
    std::fs::File::open(&path)
        .and_then(|mut file| file.read_exact(&mut body))
        .map_err(|_| unreadable())?;
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let content_type = match extension.as_str() {
        "json" | "geojson" => "application/json",
        "xml" => "text/xml",
        "tif" | "tiff" => "image/tiff",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "",
    };
    Ok(HttpResponse {
        status: 0,
        content_type: String::from(content_type),
        body,
    })
}

// The first words of a failed answer's body, for the error's context: an
// ArcGIS or OGC exception report says in plain text what was wrong with the
// request, which is worth more than the status code.
fn snippet_of(body: &[u8]) -> String {
    let text: Vec<u8> = body[..body.len().min(400)]
        .iter()
        .map(|&byte| {
            if matches!(byte, b'\n' | b'\r' | b'\t') {
                b' '
            } else {
                byte
            }
        })
        .collect();
    String::from_utf8_lossy(trimmed(&text)).into_owned()
}

/// `haystack.find(needle) != npos`, of bytes.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

// ---- HTTP -----------------------------------------------------------------------------------

/// What a failure means, in words a person can act on - a key, the network's
/// proxy, a service that answers only from its own country, the certificates
/// https:// is checked against - with `context` (the redacted URL, the status
/// and the server's first words) kept beside it. `http_fetch`'s own
/// classification, public so that every case is tested without a network.
pub fn explain_http_failure(failure: &HttpFailure, context: impl Into<String>) -> Error {
    let context = context.into();
    let status = failure.status;
    let curl_code = failure.curl_code;
    let error = &failure.error;
    if status == 404 {
        return make_error(
            ErrorCode::NotFound,
            "the service has nothing at this address",
            context,
        );
    }
    if status == 401 || status == 403 {
        // CloudFront's own refusal page, not the service's: a distribution
        // that answers only from some countries (some Australian government
        // services answer only from Australia) or blocks this network.
        if status == 403
            && contains_bytes(&failure.body, b"The request could not be satisfied")
            && contains_bytes(&failure.body, b"Request blocked")
        {
            return make_error(
                ErrorCode::FileImportFailure,
                "the service's network blocked the request (403) before it reached the service; \
                 it may answer only from its own country, or not from this network - try from a \
                 connection in that country",
                context,
            );
        }
        return make_error(
            ErrorCode::FileImportFailure,
            format!(
                "the service refused the request ({status}); it may need a key (ONLINE KEY), not \
                 allow this use, or not answer from where this computer is"
            ),
            context,
        );
    }
    if status == 429 {
        return make_error(
            ErrorCode::FileImportFailure,
            "the service is limiting requests (429); wait and try again, or ask for a smaller area",
            context,
        );
    }
    if error.contains("CONNECT tunnel failed") || curl_code == curl::CURLE_COULDNT_RESOLVE_PROXY {
        return make_error(
            ErrorCode::FileImportFailure,
            "the network's proxy refused the connection to this service; the host may be \
             blocked where Katana is running",
            context,
        );
    }
    if curl_code == curl::CURLE_COULDNT_RESOLVE_HOST {
        return make_error(
            ErrorCode::FileImportFailure,
            "the service's host name could not be found; check the address and the network \
             connection",
            context,
        );
    }
    if curl_code == curl::CURLE_OPERATION_TIMEDOUT {
        return make_error(
            ErrorCode::FileImportFailure,
            format!(
                "the service did not answer within {} s; try a smaller area or later",
                failure.timeout_seconds
            ),
            context,
        );
    }
    // libcurl reads the certificates it checks https:// against from
    // etc/ssl/certs/ca-bundle.crt beside its own bin folder (the deploy puts
    // it there; the registry points libcurl at it by its real path on Windows
    // too). When that file cannot be read every https:// request fails here,
    // before reaching any server. The message says WHICH file libcurl was
    // given - its own text names the path, and a path mangled by a code page
    // shows as such - and does not send the person to reinstall as if that
    // were the only cause: for an install under a folder named outside the
    // ANSI code page it once was no cure at all.
    if curl_code == curl::CURLE_SSL_CACERT_BADFILE || error.contains("trust anchors") {
        return make_error(
            ErrorCode::FileImportFailure,
            format!(
                "the certificates https:// is checked against could not be read ({error}); \
                 Katana keeps them in etc/ssl/certs/ca-bundle.crt beside its bin folder - put \
                 that file back, or name another certificate file in CURL_CA_BUNDLE"
            ),
            context,
        );
    }
    if curl_code == curl::CURLE_PEER_FAILED_VERIFICATION {
        return make_error(
            ErrorCode::FileImportFailure,
            "the service's certificate could not be verified against the certificates Katana \
             trusts (its bundle and, on Windows, the Windows certificate store); a proxy or \
             antivirus that inspects https:// may be in the way, and its certificate is not \
             trusted by this computer",
            context,
        );
    }
    make_error(ErrorCode::FileImportFailure, "the request failed", context)
}

/// The first eight bytes of a URL, lower-cased: where its scheme is read.
fn scheme_of(url: &str) -> Vec<u8> {
    lowered(&url.as_bytes()[..url.len().min(8)])
}

/// Fetches `request.url`. `file://` URLs are read from disk without libcurl,
/// which is how the tests exercise every caller of this function with no
/// network. Fails with:
///
/// ```text
/// InvalidArgument    a URL that is not http, https or file;
/// NotFound           a 404, or a file:// path that does not exist;
/// FileImportFailure  any other HTTP or transport failure after the retries,
///                    its context the status and the server's first words;
/// InvalidState       cancelled through `stop` (the message says so);
/// Unsupported        the body exceeded request.max_bytes.
/// ```
///
/// Error messages and contexts carry the URL through `redact_url`, never raw,
/// so a key in a query string does not reach a log.
pub fn http_fetch(
    request: &HttpRequest,
    stop: Option<&AtomicBool>,
    progress: Option<TransferProgress<'_>>,
) -> Result<HttpResponse> {
    let scheme = scheme_of(&request.url);
    if scheme.starts_with(b"file://") {
        return read_file_url(request);
    }
    if !scheme.starts_with(b"https://") && !scheme.starts_with(b"http://") {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "only http, https and file URLs can be fetched",
            redact_url(&request.url),
        ));
    }
    ensure_registered();

    let mut options = cpl::CStringList::new();
    options.set_name_value(
        b"TIMEOUT",
        format!("{}", request.timeout_seconds.max(1)).as_bytes(),
    );
    options.set_name_value(
        b"CONNECTTIMEOUT",
        format!("{}", request.connect_timeout_seconds.max(1)).as_bytes(),
    );
    // A stalled transfer - under 16 bytes a second for 30 seconds - ends
    // before the overall timeout would.
    options.set_name_value(b"LOW_SPEED_LIMIT", b"16");
    options.set_name_value(b"LOW_SPEED_TIME", b"30");
    // Retries are this function's, below: GDAL's own would sleep through a
    // cancellation.
    options.set_name_value(b"MAX_RETRY", b"0");
    if !request.user_agent.is_empty() {
        options.set_name_value(b"USERAGENT", request.user_agent.as_bytes());
    }
    let mut headers = String::new();
    for header in &request.headers {
        headers.push_str(header);
        headers.push_str("\r\n");
    }
    if !request.post_body.is_empty() {
        options.set_name_value(b"POSTFIELDS", request.post_body.as_bytes());
        headers.push_str("Content-Type: ");
        headers.push_str(&request.post_content_type);
        headers.push_str("\r\n");
    }
    if !headers.is_empty() {
        options.set_name_value(b"HEADERS", headers.as_bytes());
    }

    let url = cpl::c_string(&request.url);
    let shown = redact_url(&request.url);
    let mut delay = retry_delay(request.retry_delay_seconds);
    let mut attempt = 0;
    loop {
        if stop_requested(stop) {
            return Err(cancelled(&shown));
        }
        let mut transfer = TransferState {
            stop,
            progress,
            max_bytes: request.max_bytes,
            body: Vec::new(),
            too_large: false,
            panic: None,
        };
        let state: *mut c_void = (&raw mut transfer).cast();

        let quiet = quiet_errors();
        // SAFETY: a NUL-terminated URL and option list, alive for the call;
        // the callbacks are this module's, each given the transfer's state,
        // which outlives the call and is not touched until it returns.
        let result = unsafe {
            http::CPLHTTPFetchEx(
                url.as_ptr(),
                options.as_ptr(),
                Some(report_transfer),
                state,
                Some(collect),
                state,
            )
        };
        drop(quiet);
        if let Some(payload) = transfer.panic.take() {
            if !result.is_null() {
                // SAFETY: the result GDAL made, freed once.
                unsafe { http::CPLHTTPDestroyResult(result) };
            }
            resume_unwind(payload);
        }
        // SAFETY: NULL or the result GDAL made, alive until it is destroyed.
        let Some(answer) = (unsafe { result.as_ref() }) else {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "the request could not be made",
                format!("{shown}: {}", last_error_message()),
            ));
        };
        let curl_code = answer.n_status;
        // SAFETY: NULL or NUL-terminated texts of the live result.
        let (error, content_type) = unsafe {
            (
                cpl::decoded(answer.psz_err_buf),
                cpl::decoded(answer.psz_content_type),
            )
        };
        // SAFETY: the result GDAL made, freed once and not read after.
        unsafe { http::CPLHTTPDestroyResult(result) };

        if stop_requested(stop) {
            return Err(cancelled(&shown));
        }
        if transfer.too_large {
            return Err(make_error(
                ErrorCode::Unsupported,
                format!(
                    "the answer is larger than the {} MB this request allows; ask for a smaller \
                     area or a coarser resolution",
                    request.max_bytes / (1024 * 1024)
                ),
                shown,
            ));
        }
        let status = status_from_error(&error);
        if curl_code == 0 && error.is_empty() {
            return Ok(HttpResponse {
                status: 200,
                content_type,
                body: transfer.body,
            });
        }
        if attempt < request.retries && worth_retrying(status, curl_code) {
            if !pause(delay, stop) {
                return Err(cancelled(&shown));
            }
            delay *= 2.0;
            attempt += 1;
            continue;
        }
        let mut context = shown;
        if status != 0 {
            context.push_str(&format!(" (HTTP {status})"));
        }
        if !error.is_empty() {
            context.push_str(": ");
            context.push_str(&error);
        }
        if !transfer.body.is_empty() {
            context.push_str(" - ");
            context.push_str(&snippet_of(&transfer.body));
        }
        let failure = HttpFailure {
            status,
            curl_code,
            error,
            body: transfer.body[..transfer.body.len().min(4096)].to_vec(),
            timeout_seconds: request.timeout_seconds,
        };
        return Err(explain_http_failure(&failure, context));
    }
}

// ---- one exchange, redirects not followed -----------------------------------------------

/// Whether these request headers ("Name: value" each) hand over credentials:
/// one of them is Authorization or Proxy-Authorization (RFC 9110, 11.6.2 and
/// 11.7.2), the name in any case. `http_exchange`'s rule for when the
/// certificate check cannot be turned off, public so that it is tested
/// without a server.
pub fn carries_credentials(headers: &[String]) -> bool {
    headers.iter().any(|header| {
        // The field name: up to the colon, case-insensitive (RFC 9110, 5.1).
        // A line with no colon is no header; all of it is taken for the name,
        // so that one that names credentials is not let through for being
        // malformed.
        let end = header.find(':').unwrap_or(header.len());
        let name = trimmed(&header.as_bytes()[..end]);
        equals_ignoring_case(name, b"authorization")
            || equals_ignoring_case(name, b"proxy-authorization")
    })
}

struct ExchangeState<'a> {
    stop: Option<&'a AtomicBool>,
    progress: Option<TransferProgress<'a>>,
    max_bytes: u64,
    /// Of the LAST status line seen (a 100 Continue or a proxy's 200 comes
    /// first).
    status: i32,
    content_type: String,
    location: String,
    content_range: String,
    body: Vec<u8>,
    /// The body passed its bound and the transfer was ended here, by
    /// on_exchange_body.
    truncated: bool,
    panic: Option<Box<dyn Any + Send>>,
}

impl CallbackState for ExchangeState<'_> {
    fn panicked(&mut self) -> &mut Option<Box<dyn Any + Send>> {
        &mut self.panic
    }
}

fn read_header_line(state: &mut ExchangeState<'_>, mut line: &[u8]) {
    while let [rest @ .., b'\r' | b'\n'] = line {
        line = rest;
    }
    if line.starts_with(b"HTTP/") {
        // "HTTP/1.1 206 Partial Content", "HTTP/2 200": a new response begins,
        // and what the earlier one said about itself no longer applies.
        let start = line
            .iter()
            .position(|&byte| byte == b' ')
            .map_or(line.len(), |space| space + 1);
        let mut code: i32 = 0;
        for &byte in &line[start..] {
            if !byte.is_ascii_digit() || code >= 1000 {
                break;
            }
            code = code * 10 + i32::from(byte - b'0');
        }
        state.status = code;
        state.content_type.clear();
        state.location.clear();
        state.content_range.clear();
        return;
    }
    let Some(colon) = line.iter().position(|&byte| byte == b':') else {
        return;
    };
    let name = &line[..colon];
    let value = String::from_utf8_lossy(trimmed(&line[colon + 1..])).into_owned();
    if equals_ignoring_case(name, b"content-type") {
        state.content_type = value;
    } else if equals_ignoring_case(name, b"location") {
        state.location = value;
    } else if equals_ignoring_case(name, b"content-range") {
        state.content_range = value;
    }
}

extern "C" fn on_exchange_header(
    buffer: *mut c_char,
    size: usize,
    count: usize,
    user: *mut c_void,
) -> usize {
    let bytes = size.wrapping_mul(count);
    let work = |state: &mut ExchangeState<'_>| {
        // SAFETY: libcurl hands `bytes` readable bytes at `buffer`.
        read_header_line(state, unsafe { bytes_at(buffer.cast::<u8>(), bytes) });
        bytes
    };
    // SAFETY: `user` is the ExchangeState perform_exchange handed libcurl,
    // alive until curl_easy_perform returns; libcurl calls the callbacks one
    // at a time, on the performing thread.
    unsafe { guarded(user, 0, work) }
}

extern "C" fn on_exchange_body(
    data: *mut c_char,
    size: usize,
    count: usize,
    user: *mut c_void,
) -> usize {
    let bytes = size.wrapping_mul(count);
    let work = |state: &mut ExchangeState<'_>| {
        if stop_requested(state.stop) {
            return 0;
        }
        // SAFETY: libcurl hands `bytes` readable bytes at `data`.
        let offered = unsafe { bytes_at(data.cast::<u8>(), bytes) };
        // A 200 or a 206 is the file that was asked for, bounded by the
        // request. Any other status carries a page about itself, of which the
        // beginning is all anybody reads.
        let file = state.status == 200 || state.status == 206;
        let bound = if file {
            state.max_bytes
        } else {
            ERROR_BODY_BYTES
        };
        if state.body.len() as u64 + bytes as u64 > bound {
            // Longer than is read, and the rest is not waited for: a server
            // that never stops sending would otherwise hold the request until
            // its timeout, once for every retry. A file that was cut is no
            // file, so none of it is kept; of a page, its beginning is. (What
            // is kept never passes the bound; the C++'s unsigned
            // `bound - size` would wrap if it did.)
            if file {
                state.body.clear();
            } else {
                let room = bound.saturating_sub(state.body.len() as u64) as usize;
                state.body.extend_from_slice(&offered[..room]);
            }
            state.truncated = true;
            // Fewer bytes than were offered (there was at least one): the
            // transfer ends.
            return 0;
        }
        state.body.extend_from_slice(offered);
        bytes
    };
    // SAFETY: as on_exchange_header's.
    unsafe { guarded(user, 0, work) }
}

extern "C" fn on_exchange_progress(
    user: *mut c_void,
    total: CurlOffT,
    received: CurlOffT,
    _upload_total: CurlOffT,
    _uploaded: CurlOffT,
) -> c_int {
    let work = |state: &mut ExchangeState<'_>| {
        if let Some(progress) = state.progress
            && received >= 0
        {
            progress(received as u64, if total > 0 { total as u64 } else { 0 });
        }
        // Non-zero aborts.
        c_int::from(stop_requested(state.stop))
    };
    // SAFETY: as on_exchange_header's.
    unsafe { guarded(user, 1, work) }
}

/// An exchange sends what its request says and nothing else. CPLHTTPSetOptions
/// puts on the handle whatever the person has configured for GDAL's own
/// requests, and some of that is credentials: an entry of ~/.netrc
/// (GDAL_HTTP_NETRC is on unless turned off), GDAL_HTTP_USERPWD, GDAL_HTTP_AUTH
/// with GDAL_HTTP_BEARER, GDAL_HTTP_COOKIE and its files, a client certificate
/// (GDAL_HTTP_SSLCERT), and CPL_CURL_VERBOSE, which prints every request
/// header to stderr, an Authorization among them. Those are a person's
/// settings for a map service. An exchange is used by a caller that decides
/// hop by hop what each host is told, and a hop may be a host that a redirect
/// chose; so they are taken off the handle again, each one. (The headers of
/// GDAL_HTTP_HEADERS and GDAL_HTTP_HEADER_FILE come back from
/// CPLHTTPSetOptions as a list, which perform_exchange drops.) What stays is
/// how this computer reaches the network, which is not something said to a
/// server: the proxy and the proxy's own credentials, the certificate bundle,
/// the timeouts.
///
/// The list is GDAL's (port/cpl_http.cpp, CPLHTTPSetOptions; read against
/// libgdal 3.13's option names). An upgrade of GDAL that teaches it another
/// way to send a credential needs a line here.
///
/// # Safety
///
/// `handle` is a live easy handle.
unsafe fn send_nothing_ambient(handle: *mut curl::Curl) {
    let none: *const c_void = ptr::null();
    // SAFETY: a live handle (the caller's contract), and NULL or a long for
    // each option, which is what each is read as.
    unsafe {
        curl::set_long(handle, curl::CURLOPT_VERBOSE, 0);
        curl::set_long(handle, curl::CURLOPT_NETRC, curl::CURL_NETRC_IGNORED);
        curl::set_pointer(handle, curl::CURLOPT_NETRC_FILE, none);
        curl::set_pointer(handle, curl::CURLOPT_USERPWD, none);
        curl::set_pointer(handle, curl::CURLOPT_XOAUTH2_BEARER, none);
        // No authentication scheme of libcurl's own: an Authorization the
        // request names is a header like any other and is sent all the same.
        curl::set_long(handle, curl::CURLOPT_HTTPAUTH, curl::CURLAUTH_NONE);
        curl::set_pointer(handle, curl::CURLOPT_COOKIE, none);
        // In this order: naming a jar - even none - starts libcurl's cookie
        // engine, and a null list of cookie files is what stops it and forgets
        // what it read.
        curl::set_pointer(handle, curl::CURLOPT_COOKIEJAR, none);
        curl::set_pointer(handle, curl::CURLOPT_COOKIEFILE, none);
        curl::set_pointer(handle, curl::CURLOPT_SSLCERT, none);
        curl::set_pointer(handle, curl::CURLOPT_SSLKEY, none);
        curl::set_pointer(handle, curl::CURLOPT_KEYPASSWD, none);
    }
}

/// One request on a fresh libcurl handle, configured by GDAL's own option
/// handling (CPLHTTPSetOptions: proxy, certificate bundle, timeouts, user
/// agent), then told to send nothing the request does not name and not to
/// follow a redirect.
fn perform_exchange(
    request: &HttpRequest,
    state: &mut ExchangeState<'_>,
    message: &mut String,
) -> curl::CurlCode {
    // SAFETY: no argument; NULL or a handle this function frees.
    let handle = unsafe { curl::curl_easy_init() };
    if handle.is_null() {
        *message = String::from("libcurl could not be started");
        return curl::CURLE_FAILED_INIT;
    }
    let mut options = cpl::CStringList::new();
    options.set_name_value(
        b"TIMEOUT",
        format!("{}", request.timeout_seconds.max(1)).as_bytes(),
    );
    options.set_name_value(
        b"CONNECTTIMEOUT",
        format!("{}", request.connect_timeout_seconds.max(1)).as_bytes(),
    );
    options.set_name_value(b"LOW_SPEED_LIMIT", b"16");
    options.set_name_value(b"LOW_SPEED_TIME", b"30");
    // Asked of GDAL here, and set on the handle again in send_nothing_ambient.
    options.set_name_value(b"NETRC", b"NO");
    if !request.user_agent.is_empty() {
        options.set_name_value(b"USERAGENT", request.user_agent.as_bytes());
    }
    let url = cpl::c_string(&request.url);
    let quiet = quiet_errors();
    let mut gdal_headers: *mut c_void = ptr::null_mut();
    // SAFETY: a live handle of the libcurl GDAL loads, a NUL-terminated URL
    // and option list alive for the call, and the place for GDAL's list.
    let thrown = unsafe {
        http::katana_http_set_options(
            handle.cast(),
            url.as_ptr(),
            options.as_ptr(),
            &raw mut gdal_headers,
        )
    };
    // GDAL returns the header list it built - from GDAL_HTTP_HEADERS and
    // GDAL_HTTP_HEADER_FILE, since no HEADERS option is passed - for the
    // caller to attach and free. It is not attached: a header a person set for
    // a map service is not this request's to send.
    // SAFETY: NULL or the curl_slist GDAL allocated, freed once.
    unsafe { curl::curl_slist_free_all(gdal_headers.cast()) };
    drop(quiet);
    if thrown != 0 {
        // SAFETY: the handle made above, not used after.
        unsafe { curl::curl_easy_cleanup(handle) };
        // The C++ lets CPLHTTPSetOptions' exception out of httpExchange.
        panic!("CPLHTTPSetOptions threw an exception");
    }
    // SAFETY: a live handle.
    unsafe { send_nothing_ambient(handle) };

    let mut headers: *mut curl::CurlSlist = ptr::null_mut();
    for header in &request.headers {
        let text = cpl::c_string(header);
        // SAFETY: the list so far (or NULL) and a NUL-terminated string,
        // which libcurl copies.
        let longer = unsafe { curl::curl_slist_append(headers, text.as_ptr()) };
        if longer.is_null() {
            // Never a request with some of its headers: one without its Range
            // would be answered with the whole file.
            // SAFETY: the list so far and the handle, neither used after.
            unsafe {
                curl::curl_slist_free_all(headers);
                curl::curl_easy_cleanup(handle);
            }
            *message = String::from("out of memory");
            return curl::CURLE_OUT_OF_MEMORY;
        }
        headers = longer;
    }

    let mut error_buffer: [c_char; curl::CURL_ERROR_SIZE] = [0; curl::CURL_ERROR_SIZE];
    let user: *const c_void = ptr::from_mut(state).cast_const().cast();
    // SAFETY: a live handle, and every pointer set is alive until
    // curl_easy_cleanup below: the URL (which libcurl copies), the header
    // list, the error buffer and the state, which is not touched until
    // curl_easy_perform returns; each callback is this module's, for that
    // state.
    unsafe {
        curl::set_pointer(handle, curl::CURLOPT_URL, url.as_ptr().cast());
        curl::set_long(handle, curl::CURLOPT_HTTPGET, 1);
        curl::set_pointer(
            handle,
            curl::CURLOPT_HTTPHEADER,
            headers.cast_const().cast(),
        );
        // The point of this function. CPLHTTPSetOptions turns following on;
        // it is turned off after.
        curl::set_long(handle, curl::CURLOPT_FOLLOWLOCATION, 0);
        // The body is the bytes the server holds, never a compressed form of
        // them unpacked here: a caller that asks for a range by offset, and
        // counts what arrives, means those bytes. NULL is libcurl's "send no
        // Accept-Encoding and decode nothing".
        curl::set_pointer(handle, curl::CURLOPT_ACCEPT_ENCODING, ptr::null());
        // Credentials go only to a server whose certificate was checked,
        // whatever GDAL_HTTP_UNSAFESSL says (CPLHTTPSetOptions obeys it): that
        // setting is a person's answer to a map server with a bad
        // certificate, not leave to hand a token to whoever answers in its
        // place. A request without credentials keeps the person's setting.
        if carries_credentials(&request.headers) {
            curl::set_long(handle, curl::CURLOPT_SSL_VERIFYPEER, 1);
            curl::set_long(handle, curl::CURLOPT_SSL_VERIFYHOST, 2);
        }
        // Called from worker threads (the update's download job): no
        // signal-based timeouts.
        curl::set_long(handle, curl::CURLOPT_NOSIGNAL, 1);
        curl::set_pointer(
            handle,
            curl::CURLOPT_ERRORBUFFER,
            error_buffer.as_mut_ptr().cast_const().cast(),
        );
        curl::set_write_function(handle, curl::CURLOPT_HEADERFUNCTION, on_exchange_header);
        curl::set_pointer(handle, curl::CURLOPT_HEADERDATA, user);
        curl::set_write_function(handle, curl::CURLOPT_WRITEFUNCTION, on_exchange_body);
        curl::set_pointer(handle, curl::CURLOPT_WRITEDATA, user);
        curl::set_long(handle, curl::CURLOPT_NOPROGRESS, 0);
        curl::set_xferinfo_function(handle, on_exchange_progress);
        curl::set_pointer(handle, curl::CURLOPT_XFERINFODATA, user);
    }

    // SAFETY: the handle configured above.
    let code = unsafe { curl::curl_easy_perform(handle) };
    // libcurl ends its message with a NUL; the last byte makes sure of it.
    error_buffer[curl::CURL_ERROR_SIZE - 1] = 0;
    *message = if error_buffer[0] != 0 {
        // SAFETY: a NUL-terminated buffer.
        unsafe { cpl::decoded(error_buffer.as_ptr()) }
    } else {
        // SAFETY: libcurl's own text for the code, static.
        unsafe { cpl::decoded(curl::curl_easy_strerror(code)) }
    };
    // libcurl's own record of the last response, when it has one: the status
    // lines are read above because the body callback needs the status while
    // the body arrives, and this is the same number without this module's
    // reading of a line.
    let mut answered: c_long = 0;
    // SAFETY: the handle, and the long the status is written to.
    if code == curl::CURLE_OK
        && unsafe { curl::response_code(handle, &mut answered) } == curl::CURLE_OK
        && answered > 0
    {
        #[allow(
            clippy::unnecessary_cast,
            reason = "c_long is i64 on Linux and macOS and i32 on Windows; static_cast<int> in the C++."
        )]
        let answered = answered as i32;
        state.status = answered;
    }
    // SAFETY: the handle, its header list detached first, then the list;
    // neither used after.
    unsafe {
        curl::set_pointer(handle, curl::CURLOPT_HTTPHEADER, ptr::null());
        curl::curl_easy_cleanup(handle);
        curl::curl_slist_free_all(headers);
    }
    code
}

/// ONE exchange, as the wire had it. `http_fetch` follows redirects inside
/// libcurl, re-sending the request's headers to wherever it is sent, and
/// GDAL's CPLHTTPFetch has no switch to stop it (checked in libgdal 3.13: no
/// such option exists). A caller that must decide, hop by hop, which headers
/// go where - the update engine, whose access token must reach api.github.com
/// and never the storage host that address redirects to - uses this instead:
///
/// * the request is sent once to request.url (http or https; GET only, so
///   post_body must be empty); a redirect is NOT followed, its Location is
///   returned;
/// * every HTTP status that arrives is a result, 3xx, 4xx and 5xx included:
///   the caller reads it. Only a failure to get an HTTP answer is an error;
/// * a body is read up to a bound and no further: request.max_bytes for a 200
///   or 206, 64 KiB for any other status (an error's page, of which the
///   beginning is all anybody reads). An answer that is longer is ended the
///   moment it passes the bound - the rest is not waited for - and is still a
///   result, with `truncated` set: its status and headers arrived, and they are
///   what tell a caller a sign-in page from a file that is too long. Of a 200
///   or 206 that was cut NOTHING is handed over (`body` is empty): a beginning
///   must never pass for the file. Of any other status the first 64 KiB are;
/// * the body is the server's bytes as it holds them: no Accept-Encoding is
///   sent and nothing is decompressed, so a `Range` header asks for, and a
///   count measures, those bytes;
/// * retries (request.retries, doubling from retry_delay_seconds) are for what
///   is worth another try: a dropped or timed-out connection, and a 502, 503 or
///   504 answer. A 429 is returned at once - a rate limit that says wait an
///   hour is not helped by a second.
///
/// Proxy, certificates, timeouts and the User-Agent are set exactly as
/// `http_fetch` sets them (GDAL's own option handling is applied to the same
/// libcurl handle), with two exceptions.
///
/// * An exchange sends what its request names and NOTHING a person configured
///   for GDAL's own requests: no ~/.netrc entry (GDAL_HTTP_NETRC), no
///   GDAL_HTTP_USERPWD, no GDAL_HTTP_AUTH or GDAL_HTTP_BEARER, no
///   GDAL_HTTP_HEADERS or GDAL_HTTP_HEADER_FILE header, no cookie, no client
///   certificate; and CPL_CURL_VERBOSE does not print its headers. Those are
///   settings for a map service, and a hop of a caller that follows redirects
///   by hand may be any host. The proxy and the proxy's own credentials stay:
///   they are how the network is reached.
/// * A request with an Authorization (or Proxy-Authorization) header always
///   has the server's certificate checked, even where GDAL_HTTP_UNSAFESSL
///   turns the check off for everything else, so that a credential is never
///   handed to an unverified server.
///
/// Errors: InvalidArgument (not http or https, or a body), InvalidState
/// (cancelled), FileImportFailure (no HTTP answer: explain_http_failure's
/// words - except a timeout, which says how long was waited and gives no
/// advice about map areas - with the address through redact_url and then
/// libcurl's own message in the context, "<address>: <message>"). The
/// request's headers are never put in an error or a log. redact_url keeps a
/// query it does not know for a secret, so a caller whose addresses are secret
/// as a whole (a signed download address) takes the message from the context
/// and leaves the address.
pub fn http_exchange(
    request: &HttpRequest,
    stop: Option<&AtomicBool>,
    progress: Option<TransferProgress<'_>>,
) -> Result<HttpExchange> {
    let scheme = scheme_of(&request.url);
    if !scheme.starts_with(b"https://") && !scheme.starts_with(b"http://") {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "only http and https URLs can be exchanged",
            redact_url(&request.url),
        ));
    }
    if !request.post_body.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "an exchange is a GET and has no body",
            redact_url(&request.url),
        ));
    }
    ensure_registered();

    let shown = redact_url(&request.url);
    let mut delay = retry_delay(request.retry_delay_seconds);
    let mut attempt = 0;
    loop {
        if stop_requested(stop) {
            return Err(cancelled(&shown));
        }
        let mut state = ExchangeState {
            stop,
            progress,
            max_bytes: request.max_bytes,
            status: 0,
            content_type: String::new(),
            location: String::new(),
            content_range: String::new(),
            body: Vec::new(),
            truncated: false,
            panic: None,
        };
        let mut message = String::new();
        let code = perform_exchange(request, &mut state, &mut message);
        if let Some(payload) = state.panic.take() {
            resume_unwind(payload);
        }

        if stop_requested(stop) {
            return Err(cancelled(&shown));
        }
        // A transfer this function ended itself, at the body's bound, is an
        // answer like one that ran to its end: the status and the headers
        // arrived. libcurl calls it a write error.
        if code == curl::CURLE_OK || state.truncated {
            let server_trouble = matches!(state.status, 502..=504);
            if !server_trouble || attempt >= request.retries {
                return Ok(HttpExchange {
                    status: state.status,
                    content_type: state.content_type,
                    location: state.location,
                    content_range: state.content_range,
                    body: state.body,
                    truncated: state.truncated,
                });
            }
        } else if attempt >= request.retries || !worth_retrying(0, code) {
            if code == curl::CURLE_OPERATION_TIMEDOUT {
                // Worded here: explain_http_failure's sentence for a timeout
                // advises "a smaller area", which is advice for a map request
                // and not for whoever calls this.
                return Err(make_error(
                    ErrorCode::FileImportFailure,
                    format!(
                        "the server did not answer within {} s",
                        request.timeout_seconds
                    ),
                    format!("{shown}: {message}"),
                ));
            }
            let context = format!("{shown}: {message}");
            let failure = HttpFailure {
                status: 0,
                curl_code: code,
                error: message,
                body: Vec::new(),
                timeout_seconds: request.timeout_seconds,
            };
            return Err(explain_http_failure(&failure, context));
        }
        if !pause(delay, stop) {
            return Err(cancelled(&shown));
        }
        delay *= 2.0;
        attempt += 1;
    }
}

/// Whether a query parameter of this name holds a secret, by redact_url's rule
/// (also "api", "subscription-key" and "auth", which services use for keys).
pub fn is_secret_parameter(name: &str) -> bool {
    let folded = lowered(name);
    matches!(
        folded.as_slice(),
        b"key"
            | b"apikey"
            | b"api_key"
            | b"api"
            | b"token"
            | b"access_token"
            | b"password"
            | b"sig"
            | b"signature"
            | b"subscription-key"
            | b"auth"
    ) || folded.ends_with(b"key")
        || folded.ends_with(b"token")
}

/// The URL with the value of every query parameter whose name suggests a
/// secret (key, apikey, api_key, token, access_token, password, sig,
/// signature, and anything ending in "key" or "token") replaced by "***", and
/// any user:password@ removed. For messages and logs; the request itself is
/// sent unredacted.
pub fn redact_url(url: &str) -> String {
    let mut text = String::from(url);
    // user:password@host
    if let Some(scheme) = text.find("://") {
        let host_start = scheme + 3;
        let path_start = text[host_start..]
            .find(['/', '?', '#'])
            .map(|at| at + host_start);
        if let Some(at) = text[host_start..].find('@').map(|at| at + host_start)
            && path_start.is_none_or(|path_start| at < path_start)
        {
            text.replace_range(host_start..=at, "");
        }
    }
    let Some(query) = text.find('?') else {
        return text;
    };
    let mut out = String::from(&text[..=query]);
    let mut at = query + 1;
    let mut first = true;
    while at <= text.len() {
        let end = text[at..].find('&').map_or(text.len(), |found| found + at);
        let pair = &text[at..end];
        let equals = pair.find('=');
        let secret = is_secret_parameter(&pair[..equals.unwrap_or(pair.len())]);
        if !first {
            out.push('&');
        }
        first = false;
        match equals {
            Some(equals) if secret => {
                out.push_str(&pair[..equals]);
                out.push_str("=***");
            }
            _ => out.push_str(pair),
        }
        if end == text.len() {
            break;
        }
        at = end + 1;
    }
    out
}

/// Percent-encodes everything outside RFC 3986's unreserved set, for a query
/// parameter's value. A space is %20, never '+', which some servers read
/// literally.
pub fn percent_encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for &byte in text.as_bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
    out
}

/// The lower-case hexadecimal SHA-256 of `bytes` (FIPS 180-4). It forwards to
/// katana_core::sha256, which holds the one implementation that the update
/// engine shares; Katana carries its own because GDAL's CPL_SHA256 is not in
/// its installed headers. The disk cache names its files with it, so two
/// different requests cannot share a file and a key in a URL never becomes
/// part of a file name.
pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    katana_core::sha256::sha256_hex(bytes)
}

// ---- XML ------------------------------------------------------------------------------------

/// One element of a parsed document, with namespace prefixes kept as written
/// ("wms:Layer"); `local_name` drops the prefix, which is what a capabilities
/// reader compares, because services disagree about prefixes and never about
/// local names. GDAL's text is decoded lossily into it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XmlElement {
    pub name: String,
    pub attributes: BTreeMap<String, String>,
    /// The element's own text, the concatenation of its text children,
    /// trimmed. Empty for an element with only child elements.
    pub text: String,
    pub children: Vec<XmlElement>,
}

fn local_of(name: &str) -> &str {
    match name.rfind(':') {
        Some(colon) => &name[colon + 1..],
        None => name,
    }
}

impl XmlElement {
    pub fn local_name(&self) -> &str {
        local_of(&self.name)
    }

    /// The first child with this local name, or None.
    pub fn child(&self, local: &str) -> Option<&XmlElement> {
        self.children
            .iter()
            .find(|nested| nested.local_name() == local)
    }

    /// Every child with this local name, in document order.
    pub fn children_named(&self, local: &str) -> Vec<&XmlElement> {
        self.children
            .iter()
            .filter(|nested| nested.local_name() == local)
            .collect()
    }

    /// The text of the first child with this local name; empty when absent.
    pub fn child_text(&self, local: &str) -> String {
        self.child(local)
            .map(|found| found.text.clone())
            .unwrap_or_default()
    }

    /// An attribute by its local name (xlink:href is found as "href").
    pub fn attribute(&self, local: &str) -> String {
        self.attributes
            .iter()
            .find(|(key, _)| local_of(key) == local)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }
}

/// The tree CPLParseXMLString made, destroyed when it goes.
struct ParsedXml(*mut minixml::CplXmlNode);

impl Drop for ParsedXml {
    fn drop(&mut self) {
        // SAFETY: the tree GDAL made, destroyed once, after its last read.
        unsafe { minixml::CPLDestroyXMLNode(self.0) };
    }
}

fn convert(node: &minixml::CplXmlNode) -> XmlElement {
    // SAFETY: NULL or a NUL-terminated name in the live tree.
    let mut element = XmlElement {
        name: unsafe { cpl::decoded(node.psz_value) },
        ..XmlElement::default()
    };
    let mut text = Vec::new();
    let mut child = node.ps_child;
    // SAFETY: each child is NULL or a node of the live tree.
    while let Some(nested) = unsafe { child.as_ref() } {
        match nested.e_type {
            minixml::CXT_ATTRIBUTE => {
                // SAFETY: the attribute's name, and NULL or its value node,
                // NUL-terminated strings of the live tree.
                let (name, value) = unsafe {
                    let value = match nested.ps_child.as_ref() {
                        Some(value) if !value.psz_value.is_null() => cpl::decoded(value.psz_value),
                        _ => String::new(),
                    };
                    (cpl::decoded(nested.psz_value), value)
                };
                element.attributes.insert(name, value);
            }
            minixml::CXT_TEXT => {
                // SAFETY: NULL or a NUL-terminated text of the live tree.
                text.extend_from_slice(&unsafe { cpl::bytes_of(nested.psz_value) });
            }
            minixml::CXT_ELEMENT => element.children.push(convert(nested)),
            _ => {}
        }
        child = nested.ps_next;
    }
    element.text = String::from_utf8_lossy(trimmed(&text)).into_owned();
    element
}

/// Parses an XML document with GDAL's own parser (CPLParseXMLString) into the
/// value tree above - not katana_core::xml, a different reader with different
/// rules; the root returned is the document element. ParseFailure, with the
/// parser's message, for text that is not XML. A document larger than 64 MB
/// is refused before parsing: no capabilities document is that size, and the
/// tree costs several times the text. The text is read up to its first NUL,
/// where GDAL stops.
pub fn parse_xml(text: impl AsRef<[u8]>) -> Result<XmlElement> {
    let text = text.as_ref();
    if text.len() as u64 > MAX_XML_BYTES {
        return Err(make_error(
            ErrorCode::Unsupported,
            "the XML document is larger than 64 MB",
            format!("{} bytes", text.len()),
        ));
    }
    let owned = cpl::c_string(text);
    let quiet = quiet_errors();
    // SAFETY: a NUL-terminated document; GDAL returns a tree it allocated, or
    // NULL.
    let root = unsafe { minixml::CPLParseXMLString(owned.as_ptr()) };
    drop(quiet);
    if root.is_null() {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "the document is not XML",
            last_error_message(),
        ));
    }
    let tree = ParsedXml(root);
    // The document element: the first element sibling, after any <?xml?>
    // declaration and comments.
    let mut element = tree.0;
    // SAFETY: each sibling is NULL or a node of the live tree, and its name
    // NULL or NUL-terminated.
    while let Some(node) = unsafe { element.as_ref() }
        && (node.e_type != minixml::CXT_ELEMENT
            || (!node.psz_value.is_null() && unsafe { *node.psz_value.cast::<u8>() } == b'?'))
    {
        element = node.ps_next;
    }
    // SAFETY: NULL or a node of the live tree.
    let Some(node) = (unsafe { element.as_ref() }) else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "the XML document has no element",
        ));
    };
    let out = convert(node);
    drop(tree);
    Ok(out)
}
