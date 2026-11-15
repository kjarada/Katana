//! The update engine's network, from the project's one HTTP stack: the port of
//! src/katana_app/update_transport.hpp/.cpp.
//!
//! The engine (katana_update) links no HTTP library: it takes a [`Fetch`] as
//! a parameter. This is the Fetch the programs pass: katana_update's
//! make_fetch - the engine's own rules for redirects, the access token,
//! portals and ranges, which its tests hold to the letter - over katana_gis's
//! http_exchange, one request on a libcurl handle that does not follow a
//! redirect (GDAL's CPLHTTPFetch cannot be told not to). The proxy, the
//! certificate bundle and the timeouts are GDAL's, as for every other web
//! request Katana makes (docs/gis_online.md); the credentials a person
//! configured for GDAL's requests (a .netrc entry, GDAL_HTTP_USERPWD,
//! GDAL_HTTP_HEADERS, cookies) are NOT: an exchange sends the headers it is
//! given and the User-Agent, no more.
//!
//! The C++ has a second form for a build without GDAL, a Fetch that answers
//! every request Unsupported with the token `no_network`. The Rust app always
//! has its HTTP stack (docs/rust.md, step 6: no `io` feature), so it is not
//! ported.
//!
//! The token is whatever the request carries in FetchRequest::bearer_token;
//! this file never reads, stores or logs one. What http_exchange says about a
//! failure is not passed on as it is: its contexts show an address with its
//! query, and the address a release redirects to is a signed one. Of them this
//! file passes on libcurl's own words alone; make_fetch writes the address of
//! every Error itself, through display_url, and cuts the query from any
//! address a hop quoted all the same.
//!
//! This file is the one part of the update system that needs a socket, and NO
//! TEST DRIVES IT. The rules it must keep - where the token goes, which
//! redirects are followed, what an answer must be - are make_fetch's and are
//! tested there over a recorded wire (katana_update's tests/feed.rs); the one
//! request it makes is http_exchange's, which has tests of its own. What is
//! left to this file (handing a request to the exchange and its answer back)
//! first meets a server in a check against a real release (docs/update.md,
//! "To verify on first release").

use std::sync::atomic::AtomicBool;

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_gis::web_access::{HttpRequest, http_exchange};
use katana_update::feed::hop_fn;
use katana_update::{Fetch, HttpHop, HttpHopReply, make_fetch, program_version_text};

// Seconds to make a connection. Not the request's: a range may take minutes to
// arrive, a connection that is not made in this time will not be made.
const CONNECT_TIMEOUT_SECONDS: i32 = 20;
// Attempts after the first, for a dropped connection or a 502/503/504
// (http_exchange). A download is resumed at file granularity (stage.rs), so a
// few are enough here.
const RETRIES: i32 = 2;

// The Error of a hop that got no answer, written again from what http_exchange
// said, in the form katana_update's HopFn asks for: the hop's own words and NOT
// the address (make_fetch writes the address, without its query).
//
// gis's contexts are "<address>: <libcurl's message>", the address through
// redact_url, which hides the query parameters whose NAMES look like secrets
// and keeps the rest. That is right for a map service's key and wrong here: the
// address a release redirects to is a signed one, all of it query, under names
// redact_url has never heard of. So of gis's context only what follows the
// address is passed on - libcurl's own words ("Could not resolve host: ...",
// "SSL certificate problem: ..."), which are what tell a refused certificate
// from a missing network. (make_fetch does not rely on this: it cuts the query
// from anything a hop quotes.)
fn hop_error(error: &Error) -> Error {
    // Cancelled, or an address the exchange will not fetch: said as it is.
    if matches!(
        error.code,
        ErrorCode::InvalidState | ErrorCode::InvalidArgument
    ) {
        return Error::new(error.code, error.message.clone());
    }
    // No HTTP answer at all: `status=0` first (feed.rs, the Fetch
    // conventions). An address that make_fetch let through holds no blank, so
    // the first ": " is the one gis wrote after it.
    let words = match error.context.find(": ") {
        Some(at) => &error.context[at + 2..],
        None => "",
    };
    make_error(
        ErrorCode::FileImportFailure,
        error.message.clone(),
        if words.is_empty() {
            String::from("status=0")
        } else {
            format!("status=0; {words}")
        },
    )
}

fn hop_over_gis(
    hop: &HttpHop,
    stop: Option<&AtomicBool>,
    progress: Option<&dyn Fn(u64, u64)>,
) -> Result<HttpHopReply> {
    let request = HttpRequest {
        url: hop.url.clone(),
        // Every request names its program (web_access.rs). The engine's
        // requests carry "Katana/<version> (<target>)" (default_feed_config);
        // one that came without says at least which program and version it
        // is, never libcurl's or GDAL's own name.
        user_agent: if hop.user_agent.is_empty() {
            format!("Katana/{}", program_version_text())
        } else {
            hop.user_agent.clone()
        },
        // hop_headers built these: the token is among them on the first hop of
        // a request to api.github.com and on no other. http_exchange adds
        // nothing of its own to them - no credential a person configured for
        // GDAL's requests goes with an exchange.
        headers: hop.headers.clone(),
        timeout_seconds: hop.timeout_seconds,
        connect_timeout_seconds: CONNECT_TIMEOUT_SECONDS,
        retries: RETRIES,
        max_bytes: hop.max_bytes,
        ..HttpRequest::default()
    };

    let forward: &dyn Fn(u64, u64) = &|received: u64, total: u64| {
        if let Some(progress) = progress {
            progress(received, total);
        }
    };
    let exchange =
        http_exchange(&request, stop, Some(forward)).map_err(|error| hop_error(&error))?;
    // gis ended the transfer at hop.max_bytes and kept none of the body
    // (web_access.rs). For a 200 or 206 that is an answer the engine must not
    // take for the file, and the status and the content type say which kind
    // of wrong it is (HttpHopReply::overran). An error's page cut at its
    // first 64 KiB is still just that error's page.
    let overran = exchange.truncated && (exchange.status == 200 || exchange.status == 206);
    Ok(HttpHopReply {
        status: exchange.status,
        content_type: exchange.content_type,
        location: exchange.location,
        content_range: exchange.content_range,
        body: exchange.body,
        overran,
    })
}

/// The transport the programs hand the update verbs (update_verbs.rs,
/// make_update_context).
pub fn make_update_fetch() -> Fetch<'static> {
    make_fetch(hop_fn(hop_over_gis))
}
