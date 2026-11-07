//! Where releases are and how the engine asks for them, without linking an HTTP library: the
//! transport is INJECTED. katana_app adapts its HTTP stack (libcurl, retries, timeouts, the CA
//! bundle at <bin>/../etc/ssl/certs, a body cap) to [`Fetch`]; tests pass a fake; the engine never
//! sees curl.
//!
//! THE FEED. No index, no mutable tag, no REST call (60 requests an hour per IP anonymously): the
//! newest release's fixed-name assets,
//!
//! ```text
//! https://github.com/<repo>/releases/latest/download/Katana-update-<target>.json
//! https://github.com/<repo>/releases/latest/download/Katana-update-<target>.json.sig
//! ```
//!
//! and the payload zip of the release the manifest names,
//!
//! ```text
//! https://github.com/<repo>/releases/download/<tag>/<payload.name>
//! ```
//!
//! `latest` is whichever release `gh release create --latest` flagged, so a bad release is undone
//! with `gh release edit vPREV --latest`. A feed base of the owner's own host replaces the first
//! part: <base>/<fixed name>, and <base>/<payload.name> for the payload; it must honour `Range`.
//!
//! THE PRIVATE-REPOSITORY ROUTE ([`FeedRoute::Api`]), for a person who holds a read-only token
//! while the repository is private. A github.com release URL does not take a token and answers an
//! anonymous read of a private repository with 404, so the only door is the REST API, and the
//! token goes there and nowhere else:
//!
//! ```text
//! GET https://api.github.com/repos/<repo>/releases/latest            (release_lookup_url)
//! GET https://api.github.com/repos/<repo>/releases/tags/<tag>        (release_by_tag_url)
//!      Accept: application/vnd.github+json. The body is JSON; its assets[] give each
//!      asset's name, numeric id and size (parse_release_assets).
//! GET https://api.github.com/repos/<repo>/releases/assets/<id>       (asset_api_url)
//!      Accept: application/octet-stream. The API answers 302 to the storage host; the
//!      transport follows it, re-sends `Range` and drops `Authorization` there.
//! ```
//!
//! check() starts on the Download route. Only when the signature answers NotFound, the config has
//! a token and no feed_base, it retries ONCE on the Api route (releases/latest, then the two
//! assets by id) and reports the route that worked; the caller copies it to FeedConfig::route for
//! stage(). stage() on the Api route finds the payload's id through release_by_tag_url(tag): the
//! release that is `latest` may have moved on since the check. The Api route costs REST quota (60
//! an hour anonymously, 5000 with a token), which is why it is the fallback and not the default.
//!
//! Build-time defaults, as the C++ build sets them at configure (src/katana_update/CMakeLists.txt;
//! this crate's build.rs reads the same file): KATANA_UPDATE_REPO "kjarada/Katana", and
//! KATANA_UPDATE_FEED "" (empty: GitHub, from the repo; set: that base URL). An empty repo AND an
//! empty feed is a build with no auto-update: check() answers Unsupported with the reason
//! "disabled_build".
//!
//! THE TRANSPORT'S RULES. What a Fetch does between the request and the bytes is the engine's,
//! and is written here with no HTTP library, so that the rules that keep the token safe are
//! tested to the letter: [`make_fetch`] is a Fetch over a function that moves ONE request on the
//! wire and follows nothing. katana_app supplies that function; a test supplies a fake that
//! records what each hop sent.
//!
//! * Redirects are followed HERE, at most [`MAX_REDIRECTS`], and only to https addresses (an http
//!   address on the loopback only when the request itself began there: a feed of the owner's
//!   own, a test). The headers are REBUILT for every hop ([`hop_headers`]): `Range` and `Accept`
//!   on every hop, and `Authorization: Bearer` on the FIRST hop alone, and there only when the
//!   address the engine asked for is exactly https://api.github.com. No address a server answered
//!   with is sent the token: not the signed address of the storage host, and not api.github.com
//!   itself when a redirect names it. A host that is trusted with a download is not thereby
//!   trusted to aim an authenticated request at a path of its choosing. (What that costs: a
//!   repository that was renamed answers on api.github.com with a 301 to its new address, and the
//!   hop that follows it carries no token, so a private repository then answers 404. Whether that
//!   redirect should carry the token is the owner's to rule, docs/update.md, "To verify on first
//!   release".)
//! * A 404 is NotFound; any other status that is not 200/206 is FileImportFailure with the context
//!   `status=<n>`; a transport failure is `status=0`.
//! * An answer whose content type is HTML is a web page standing in for the file (a captive
//!   portal, a proxy's sign-in): FileImportFailure, context `status=<n>; reason=html_page`,
//!   however long the page is, also when it is longer than the request's max_bytes.
//! * A ranged answer is held to check_range_response (ranges.rs): 206 with the Content-Range asked
//!   for, or a 200 that is the whole file when the whole file was asked for. A server that ignores
//!   `Range` and starts the whole payload is cut off at max_bytes (the hop function does that) and
//!   reported as Unsupported "this network does not allow partial downloads"; a 206 that is
//!   another part of the file, or of a file of another length, or a body of another length (one
//!   that ran past the range included) is FileImportFailure, context `status=206; reason=bad_range`.
//! * No address with a query string, and no header, is ever put in an Error. The Fetch writes the
//!   context of every Error it returns itself, with the address through [`display_url`]; what the
//!   hop function said about a failure is passed on only after [`without_queries`], so an address
//!   the hop quoted has lost its query too; and the request's token is in no word of it: message
//!   and context pass through redact_token (credential.rs).
//!
//! THE CONTEXT OF AN ERROR of a Fetch is written in one order, so that the token of a failure is
//! read from the part this program wrote and never from an address or a server's words:
//!
//! ```text
//! status=<n>; reason=<token>; <address without its query>; <detail>
//! ```
//!
//! `status=` first when an HTTP answer (or none: 0) is what failed, then `reason=` when the
//! failure has a token of its own, then the rest. [`fetch_failure_reason`] reads those two leading
//! fields and nothing after them.

use crate::credential::redact_token;
use crate::json::{self, Json};
use crate::manifest::Manifest;
use crate::ranges::{PARTIAL_DOWNLOADS_REFUSED, check_range_response, range_header_value};
use crate::version::program_version_text;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use std::sync::atomic::{AtomicBool, Ordering};

/// Which door the engine is using; see "THE PRIVATE-REPOSITORY ROUTE".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FeedRoute {
    /// github.com/<repo>/releases/... or the owner's feed_base: no token is ever sent.
    #[default]
    Download,
    /// api.github.com: needs a token; asset ids from the release lookup.
    Api,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedConfig {
    /// "owner/name".
    pub repo: String,
    /// "" for GitHub; else a base URL without a trailing '/'.
    pub feed_base: String,
    pub channel: String,
    /// "win64" ... (from install.json).
    pub target: String,
    /// "Katana/<version> (<target>)": the only thing a check sends.
    pub user_agent: String,
    /// Optional read-only token; see credential.rs. Empty: none.
    pub token: String,
    /// check() sets it; stage() reads it.
    pub route: FeedRoute,
}

impl Default for FeedConfig {
    fn default() -> Self {
        Self {
            repo: String::new(),
            feed_base: String::new(),
            channel: String::from("stable"),
            target: String::new(),
            user_agent: String::new(),
            token: String::new(),
            route: FeedRoute::Download,
        }
    }
}

/// One asset of a release, as the API lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub id: u64,
    pub size: u64,
}

/// What may be fetched before anything is verified, and so must be bounded by the engine and not
/// by the server. A Fetch is never given max_bytes 0 (it answers InvalidArgument if it is): check()
/// passes MAX_SIGNATURE_BYTES for the signature, MAX_MANIFEST_BYTES for the manifest and
/// MAX_RELEASE_JSON_BYTES for a release lookup. MAX_MANIFEST_BYTES holds MAX_FILES (paths.rs)
/// entries of about 400 bytes with room to spare; the signature line is
/// `ed25519 <16> <88 base64>` plus a line break (115).
pub const MAX_SIGNATURE_BYTES: u64 = 1024;
pub const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_RELEASE_JSON_BYTES: u64 = 2 * 1024 * 1024;

/// One request. A Fetch returns the body or an error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FetchRequest {
    pub url: String,
    /// 0 length: the whole body. Otherwise bytes [range_start, range_start + range_length) by
    /// `Range: bytes=a-b`, and the answer must be exactly that long (206, or a 200 whose body is
    /// the whole of a file no longer than the range).
    pub range_start: u64,
    pub range_length: u64,
    /// The most bytes to accept; the transfer is abandoned the moment it is exceeded. For a range
    /// this equals range_length, so a server that ignores `Range` and starts the whole payload
    /// cannot flood the disk or memory. NEVER 0: the whole-body requests before verification use
    /// the MAX_..._BYTES constants above. A Fetch given 0 answers InvalidArgument.
    pub max_bytes: u64,
    /// The Accept header; empty: the transport's default. The Api route sets
    /// "application/vnd.github+json" for a release lookup and "application/octet-stream" for an
    /// asset.
    pub accept: String,
    /// Added as `Authorization: Bearer <token>` ONLY when the transport is about to talk https to
    /// exactly api.github.com, and NEVER on a redirect hop: the transport follows redirects itself
    /// and drops the header there. The engine fills it in only for a request to that host. Never
    /// logged.
    pub bearer_token: String,
    pub user_agent: String,
    /// The signed size of the file a range is taken from (the manifest's payload size): what a
    /// `Content-Range` total and a 200 that holds the whole file are held to (check_range_response,
    /// ranges.rs). Required (InvalidArgument without it) when range_length is not 0; ignored
    /// otherwise.
    pub payload_size: u64,
}

/// The C++'s std::stop_token: None is a default token, which no stop reaches; Some is set to true
/// when a stop is requested.
pub type StopToken<'a> = Option<&'a AtomicBool>;

/// The C++'s ProgressFn, `(done, total)`: None is an empty std::function, which is never called.
pub type ProgressFn<'a> = Option<&'a dyn Fn(u64, u64)>;

/// The injected transport. None is an empty std::function: the engine says "no transport". Failure
/// conventions the engine reads:
///
/// * NotFound: a 404 (a private repository answers anonymous reads with it);
/// * Unsupported: the body was longer than max_bytes (a server that ignored Range), reported as
///   "this network does not allow partial downloads";
/// * InvalidState: cancelled through the stop token;
/// * FileImportFailure: anything else; the Error's context begins `status=<n>` when an HTTP
///   status arrived (403, 429 ...), `status=0` otherwise.
///
/// A portal's HTML page that answers 200 is the engine's to notice: it fails the signature, which
/// is checked before the body is believed.
pub type Fetch<'a> = Option<
    Box<
        dyn Fn(&FetchRequest, Option<&AtomicBool>, Option<&dyn Fn(u64, u64)>) -> Result<Vec<u8>>
            + 'a,
    >,
>;

/// A [`Fetch`] of `function`: the one place its signature is spelled, so that a closure handed to
/// it is read with the argument types a Fetch takes.
pub fn fetch_fn<'a, F>(function: F) -> Fetch<'a>
where
    F: Fn(&FetchRequest, Option<&AtomicBool>, Option<&dyn Fn(u64, u64)>) -> Result<Vec<u8>> + 'a,
{
    Some(Box::new(function))
}

/// "api.github.com": the one host the token is sent to.
pub const API_HOST: &str = "api.github.com";
pub const MAX_REDIRECTS: i32 = 5;
/// What the hop function is given as the time for one request. A whole-body request is a few
/// kilobytes: HOP_TIMEOUT_SECONDS. A range is given the time its length takes on a slow line
/// (mobile data at the edge of coverage, where a survey crew updates), and never less than
/// RANGE_HOP_TIMEOUT_SECONDS: [`range_hop_timeout_seconds`]. A transfer that has stopped moving
/// is ended long before either by the hop's own low-speed limit; these bound one that only crawls.
pub const HOP_TIMEOUT_SECONDS: i32 = 60;
pub const RANGE_HOP_TIMEOUT_SECONDS: i32 = 600;
pub const SLOW_LINE_BYTES_PER_SECOND: u64 = 16 * 1024;
pub const MAX_HOP_TIMEOUT_SECONDS: i32 = 4 * 60 * 60;

/// One request on the wire, redirects NOT followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpHop {
    pub url: String,
    pub user_agent: String,
    /// From hop_headers.
    pub headers: Vec<String>,
    /// Abandon the transfer past this many body bytes.
    pub max_bytes: u64,
    pub timeout_seconds: i32,
}

impl Default for HttpHop {
    fn default() -> Self {
        Self {
            url: String::new(),
            user_agent: String::new(),
            headers: Vec::new(),
            max_bytes: 0,
            timeout_seconds: HOP_TIMEOUT_SECONDS,
        }
    }
}

/// What came back. Any HTTP status that arrived is a reply, 3xx and 4xx included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpHopReply {
    pub status: i32,
    pub content_type: String,
    /// The Location header, empty when none.
    pub location: String,
    /// The Content-Range header, empty when none.
    pub content_range: String,
    pub body: Vec<u8>,
    /// True when the body of a 200 or 206 passed HttpHop::max_bytes and the transfer was abandoned
    /// there. `body` is then not the answer (the hop of katana_app leaves it empty) and is never
    /// handed on; the status and the content type are still the answer's, and they are what tell
    /// a portal's long page (html_page) from a server that ignored `Range` (a 200:
    /// partial_downloads) from a 206 that ran past its range (bad_range).
    pub overran: bool,
}

/// One request on the wire. None is an empty std::function. Errors of a hop: InvalidState when
/// cancelled; FileImportFailure for any failure to get an HTTP answer (no connection, a
/// certificate refused, a timeout); InvalidArgument for an address it cannot fetch; Unsupported
/// when the body passed max_bytes and the hop cannot say what the answer was (one that can returns
/// the reply with `overran` set, which is the better report).
///
/// The context of a hop's Error is the hop's own words about the failure (`status=<n>` first when
/// it knows one, the library's sentence after it) and NOT the address: the Fetch writes the
/// address itself. No context of a hop is trusted to be free of one, all the same: the Fetch keeps
/// the leading status, and puts what follows it after its own address, through without_queries
/// (and the message likewise).
pub type HopFn<'a> = Option<
    Box<
        dyn Fn(&HttpHop, Option<&AtomicBool>, Option<&dyn Fn(u64, u64)>) -> Result<HttpHopReply>
            + 'a,
    >,
>;

/// A [`HopFn`] of `function`, as [`fetch_fn`] is a Fetch of one.
pub fn hop_fn<'a, F>(function: F) -> HopFn<'a>
where
    F: Fn(&HttpHop, Option<&AtomicBool>, Option<&dyn Fn(u64, u64)>) -> Result<HttpHopReply> + 'a,
{
    Some(Box::new(function))
}

fn stop_requested(stop: StopToken<'_>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

// ---- the addresses -----------------------------------------------------------------------------

// RFC 3986 unreserved characters pass; every other byte is %XX with upper-case hex.
fn percent_encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for &byte in text.as_bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

// https://github.com/<repo>/releases (the repo is the owner's own setting, not a signed value; it
// is "owner/name", whose slash must stay).
fn github_releases(feed: &FeedConfig) -> String {
    format!("https://github.com/{}/releases", feed.repo)
}

fn api_repo(feed: &FeedConfig) -> String {
    format!("https://api.github.com/repos/{}", feed.repo)
}

fn latest_asset(feed: &FeedConfig, name: &str) -> String {
    if !feed.feed_base.is_empty() {
        return format!("{}/{name}", feed.feed_base);
    }
    format!("{}/latest/download/{name}", github_releases(feed))
}

/// The configure-time defaults, with `target` and the user agent filled in.
pub fn default_feed_config(target: &str) -> FeedConfig {
    // "a base URL without a trailing '/'": the addresses join with one.
    let feed_base = env!("KATANA_UPDATE_FEED").trim_end_matches('/');
    FeedConfig {
        repo: String::from(env!("KATANA_UPDATE_REPO")),
        feed_base: String::from(feed_base),
        channel: String::from("stable"),
        target: target.to_owned(),
        // The only thing a check sends about this copy; no machine name, no user, no identifier.
        user_agent: format!("Katana/{} ({target})", program_version_text()),
        // The token is the caller's to fill in, from the credential store (credential.rs): it is
        // never compiled in.
        token: String::new(),
        route: FeedRoute::Download,
    }
}

/// "Katana-update-<target>.json": what the Api route looks for among a release's assets.
pub fn manifest_asset_name(target: &str) -> String {
    format!("Katana-update-{target}.json")
}

/// The same with ".sig".
pub fn signature_asset_name(target: &str) -> String {
    format!("{}.sig", manifest_asset_name(target))
}

/// The Download-route URL of the manifest, whatever `route` says.
pub fn manifest_url(feed: &FeedConfig) -> String {
    latest_asset(feed, &manifest_asset_name(&feed.target))
}

/// The Download-route URL of the signature, whatever `route` says.
pub fn signature_url(feed: &FeedConfig) -> String {
    latest_asset(feed, &signature_asset_name(&feed.target))
}

/// The Download-route URL of the payload; empty for a manifest without a payload (notify-only).
/// Names taken from the manifest (the tag, the payload name) are percent-encoded, every byte
/// outside RFC 3986's unreserved set (A-Z a-z 0-9 - . _ ~), so a signed value cannot add a path
/// segment or a query.
pub fn payload_url(feed: &FeedConfig, manifest: &Manifest) -> String {
    let Some(payload) = &manifest.payload else {
        return String::new();
    };
    let name = percent_encode(&payload.name);
    if !feed.feed_base.is_empty() {
        return format!("{}/{name}", feed.feed_base);
    }
    format!(
        "{}/download/{}/{name}",
        github_releases(feed),
        percent_encode(&manifest.tag)
    )
}

/// The Api-route URLs (the private-repository route). They name api.github.com whatever
/// `feed_base` says: the route is only taken for GitHub.
pub fn release_lookup_url(feed: &FeedConfig) -> String {
    format!("{}/releases/latest", api_repo(feed))
}

pub fn release_by_tag_url(feed: &FeedConfig, tag: &str) -> String {
    format!("{}/releases/tags/{}", api_repo(feed), percent_encode(tag))
}

pub fn asset_api_url(feed: &FeedConfig, asset_id: u64) -> String {
    format!("{}/releases/assets/{asset_id}", api_repo(feed))
}

// ---- the asset list of a release ---------------------------------------------------------------

// A release's JSON has the release, its author, the assets and each asset's uploader: four levels.
// Sixteen is room for GitHub to add some and stops a hostile document from nesting the parser's
// stack away (the document is capped at MAX_RELEASE_JSON_BYTES, which is room for a million
// levels).
const MAX_RELEASE_JSON_DEPTH: usize = 16;
// GitHub allows 1000 assets to a release (docs/update.md cites about-releases); twice that is
// slack, and a longer list is not a release of this system.
const MAX_RELEASE_ASSETS: usize = 2000;

fn bad_release(what: &str, context: impl Into<String>) -> Error {
    make_error(ErrorCode::ParseFailure, what, context)
}

/// The assets[] of a release's JSON (release_lookup_url / release_by_tag_url). The JSON is NOT
/// signed, so nothing in it is believed beyond locating an asset: the bytes fetched by id are
/// verified like any others. ParseFailure when it is not a release object or an asset lacks a name
/// or an id; an absent assets[] is an empty list.
pub fn parse_release_assets(json: impl AsRef<[u8]>) -> Result<Vec<ReleaseAsset>> {
    let json = json.as_ref();
    if json.len() as u64 > MAX_RELEASE_JSON_BYTES {
        return Err(bad_release(
            "the release description is larger than this program reads",
            format!("{} bytes", json.len()),
        ));
    }
    // The reader takes a NUL byte for the end of its input: what follows would be ignored.
    if json.contains(&0) {
        return Err(bad_release("the release description holds a NUL byte", ""));
    }

    let options = json::Options {
        max_depth: Some(MAX_RELEASE_JSON_DEPTH),
        refuse_duplicates: false,
    };
    let root = json::parse(json, options).map_err(|failure| match failure {
        json::Failure::TooDeep => bad_release("the release description is nested too deeply", ""),
        // Not the reader's own words: they end with the token it was in.
        json::Failure::Syntax(byte) => bad_release(
            "the release description is not well-formed JSON",
            format!("the reader stopped at byte {byte}"),
        ),
        json::Failure::Duplicate(_) | json::Failure::OutOfRange => {
            bad_release("the release description could not be read", "")
        }
    })?;

    let Some(object) = root.as_object() else {
        return Err(bad_release(
            "the release description is not a JSON object",
            "",
        ));
    };
    let Some(listed) = object.get("assets") else {
        return Ok(Vec::new());
    };
    let Some(listed) = listed.as_array() else {
        return Err(bad_release("the release's assets are not a list", "assets"));
    };
    if listed.len() > MAX_RELEASE_ASSETS {
        return Err(bad_release(
            "the release lists more assets than a release of this program has",
            format!("{} assets", listed.len()),
        ));
    }
    let mut assets = Vec::with_capacity(listed.len());
    for (index, item) in listed.iter().enumerate() {
        let place = format!("assets[{index}]");
        let Some(item) = item.as_object() else {
            return Err(bad_release("an asset is not an object", place));
        };
        let Some(name) = item
            .get("name")
            .and_then(Json::as_str)
            .filter(|name| !name.is_empty())
        else {
            return Err(bad_release("an asset has no name", place));
        };
        // An unsigned integer as written: a float, a string or a negative number is not an id.
        let Some(id) = item
            .get("id")
            .and_then(Json::as_unsigned)
            .filter(|&id| id != 0)
        else {
            return Err(bad_release("an asset has no id", place));
        };
        let size = match item.get("size") {
            None => 0,
            Some(size) => match size.as_unsigned() {
                Some(size) => size,
                None => return Err(bad_release("an asset's size is not a whole number", place)),
            },
        };
        assets.push(ReleaseAsset {
            name: name.to_owned(),
            id,
            size,
        });
    }
    Ok(assets)
}

// ---- the transport's rules ---------------------------------------------------------------------

// Control characters, a space, DEL, a backslash (a browser reads it as '/', curl does not: two
// readers of one address) and anything past ASCII: an address that went over the wire is
// percent-encoded.
fn has_forbidden_character(url: &str) -> bool {
    url.bytes()
        .any(|byte| byte <= 0x20 || byte >= 0x7f || byte == b'\\')
}

struct UrlParts<'a> {
    /// Lower case.
    scheme: String,
    /// Between "//" and the first '/', '?' or '#'.
    authority: &'a str,
    /// From that character on; may be empty.
    remainder: &'a str,
}

fn split_url(url: &str) -> Option<UrlParts<'_>> {
    let colon = url.find("://")?;
    if colon == 0 || colon > 8 {
        return None;
    }
    let rest = &url[colon + 3..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(UrlParts {
        scheme: url[..colon].to_ascii_lowercase(),
        authority: &rest[..end],
        remainder: &rest[end..],
    })
}

// The host of an authority without user-info: "127.0.0.1", "localhost", "[::1]" with a port after
// it or not, lower case, brackets kept. None when there is user-info or the port is not digits.
fn split_authority(authority: &str) -> Option<String> {
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = if authority.starts_with('[') {
        let close = authority.find(']')?;
        let after = &authority[close + 1..];
        let port = if after.is_empty() {
            None
        } else {
            Some(after.strip_prefix(':')?)
        };
        (authority[..=close].to_ascii_lowercase(), port)
    } else {
        match authority.find(':') {
            Some(colon) => (
                authority[..colon].to_ascii_lowercase(),
                Some(&authority[colon + 1..]),
            ),
            None => (authority.to_ascii_lowercase(), None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let bad_port =
        |port: &str| port.is_empty() || port.len() > 5 || !port.bytes().all(|c| c.is_ascii_digit());
    if port.is_some_and(bad_port) {
        return None;
    }
    Some(host)
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

fn bad_redirect(why: &str) -> Error {
    make_error(
        ErrorCode::FileImportFailure,
        "the server redirected somewhere this program does not follow",
        format!("reason=bad_redirect; {why}"),
    )
}

// The Error of a request that got an HTTP answer or none, its context in the one order the module
// gives: `status=<n>`, then the failure's own token when it has one, then the address without its
// query, then whatever else is worth saying. The token comes before the address because the
// address is not this program's text: a path may spell anything.
fn fetch_error(
    code: ErrorCode,
    message: impl Into<String>,
    status: i32,
    url: &str,
    reason: &str,
    detail: &str,
) -> Error {
    let mut context = format!("status={status}; ");
    if !reason.is_empty() {
        context.push_str("reason=");
        context.push_str(reason);
        context.push_str("; ");
    }
    context.push_str(&display_url(url));
    if !detail.is_empty() {
        context.push_str("; ");
        context.push_str(detail);
    }
    make_error(code, message, context)
}

// A server's words (a header it sent) as they may stand in an Error: printable ASCII, and no more
// of them than a person reads.
fn shown_server_text(text: &str) -> String {
    const SHOWN: usize = 160;
    let bytes = text.as_bytes();
    let mut out: String = bytes[..bytes.len().min(SHOWN)]
        .iter()
        .map(|&byte| {
            if (0x20..0x7f).contains(&byte) {
                char::from(byte)
            } else {
                '?'
            }
        })
        .collect();
    if bytes.len() > SHOWN {
        out.push_str("...");
    }
    out
}

// What check_range_response refused, as an Error of a Fetch. A server that ignored `Range` keeps
// its code and its sentence, which the dialog shows. A 206 that is not the range asked for is
// given the token bad_range: its `status=206` alone would be read as "the server said 206".
fn range_refused(why: Error, request: &FetchRequest, url: &str, content_range: &str) -> Error {
    if why.code != ErrorCode::FileImportFailure {
        return why;
    }
    // Written again here, not copied from `why`: its context quotes the header as it came.
    fetch_error(
        ErrorCode::FileImportFailure,
        why.message,
        206,
        url,
        "bad_range",
        &format!(
            "asked for {} bytes from {} of {}; Content-Range '{}'",
            request.range_length,
            request.range_start,
            request.payload_size,
            shown_server_text(content_range)
        ),
    )
}

// Neither the request's token nor the query of an address is in any word of an Error the Fetch
// returns, whoever wrote the words: the hop function is the caller's, what it says about a failure
// is passed on, and a server's own words are quoted in some of them.
fn without_secrets(mut error: Error, token: &str) -> Error {
    error.message = without_queries(&error.message);
    error.context = without_queries(&error.context);
    if !token.is_empty() {
        error.message = redact_token(&error.message, token);
        error.context = redact_token(&error.context, token);
    }
    error
}

fn cancelled(url: &str) -> Error {
    make_error(ErrorCode::InvalidState, "cancelled", display_url(url))
}

// The leading `status=<n>` of a context written in the one order.
struct LeadingStatus<'a> {
    /// 0 as well when the context names none: either way no answer arrived.
    status: i32,
    /// What follows the field and its separator. All of the context when it does not begin with
    /// the field; nothing when the field is not `status=<digits>; ` to the letter (`status=200x`,
    /// a `status=` with no number), so that nothing after a malformed field is taken for one.
    rest: &'a str,
}

fn leading_status(context: &str) -> LeadingStatus<'_> {
    const STATUS: &str = "status=";
    const NEXT: &str = "; ";
    let Some(rest) = context.strip_prefix(STATUS) else {
        return LeadingStatus {
            status: 0,
            rest: context,
        };
    };
    let bytes = rest.as_bytes();
    let mut status = 0i32;
    let mut digits = 0usize;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() && status < 10_000 {
        status = status * 10 + i32::from(bytes[digits] - b'0');
        digits += 1;
    }
    let rest = &rest[digits..];
    let rest = match rest.strip_prefix(NEXT) {
        Some(after) if digits > 0 => after,
        _ => "",
    };
    LeadingStatus { status, rest }
}

// The Error of a hop that failed, as the Fetch hands it on. The code and the sentence are the
// hop's. The context is written again here, in the one order: the hop's own `status=<n>` (0 when
// it named none: no answer arrived), the address of the hop without its query, and then whatever
// else the hop said, which is text and no longer a field. So a hop that passes on a library's
// context, address and all, still cannot put a signed address into an Error (make_fetch cuts the
// query from what it quoted) or name a reason.
fn hop_failure(error: &Error, url: &str) -> Error {
    let led = leading_status(&error.context);
    fetch_error(
        error.code,
        error.message.as_str(),
        led.status,
        url,
        "",
        led.rest,
    )
}

fn is_redirect_status(status: i32) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

// A person's sentence for a status that is neither a success nor a redirect.
fn message_for_status(status: i32) -> String {
    match status {
        401 => String::from("the server did not accept the access token (401)"),
        403 => String::from(
            "the server refused the request (403); it may be limiting requests, or the network \
             may block it",
        ),
        429 => String::from("the server is limiting requests (429); try again later"),
        416 => String::from(
            "the server does not have the part of the file that was asked for (416); the release \
             may have been replaced",
        ),
        500..=599 => format!("the server had a problem answering ({status})"),
        _ => format!("the server answered with an unexpected status ({status})"),
    }
}

fn content_type_is_html(content_type: &str) -> bool {
    content_type.to_ascii_lowercase().contains("text/html")
}

/// True for exactly `https://api.github.com` and a path or query after it, in any case: the
/// authority is the whole text up to the first '/', '?' or '#', so a user-info prefix
/// (https://api.github.com@host/), a port, a longer name (api.github.com.example.org) and a
/// trailing dot are all NOT api.github.com. The one test the token passes.
pub fn is_api_github_url(url: &str) -> bool {
    if has_forbidden_character(url) {
        return false;
    }
    // The authority must BE the host name: nothing before it (user-info), nothing after it (a
    // port, a trailing dot), nothing longer. HTTPS only, so the token never crosses in clear.
    split_url(url).is_some_and(|parts| {
        parts.scheme == "https" && parts.authority.to_ascii_lowercase() == API_HOST
    })
}

/// http://127.0.0.1, http://localhost or http://[::1], any port, with no control character,
/// space or backslash and no user-info.
pub fn is_loopback_http_url(url: &str) -> bool {
    if has_forbidden_character(url) {
        return false;
    }
    let Some(parts) = split_url(url) else {
        return false;
    };
    if parts.scheme != "http" {
        return false;
    }
    split_authority(parts.authority).is_some_and(|host| is_loopback_host(&host))
}

/// https://..., or a loopback http address (is_loopback_http_url) when `allow_loopback_http`. No
/// control character, space or backslash, no user-info, a non-empty host.
pub fn is_fetchable_url(url: &str, allow_loopback_http: bool) -> bool {
    if has_forbidden_character(url) {
        return false;
    }
    let Some(parts) = split_url(url) else {
        return false;
    };
    if parts.scheme == "https" {
        return split_authority(parts.authority).is_some();
    }
    parts.scheme == "http" && allow_loopback_http && is_loopback_http_url(url)
}

/// The address a Location header names, from the address that answered with it: an absolute https
/// (or http, which is_fetchable_url then judges) address, "//host/path" with the scheme of `from`,
/// or "/path" on the host of `from`. The fragment is dropped. Anything else (a path relative to
/// the current one, another scheme, a control character, a space, a backslash, user-info) is
/// FileImportFailure "bad_redirect".
pub fn resolve_redirect(from: &str, location: &str) -> Result<String> {
    if location.is_empty() {
        return Err(bad_redirect("the answer names no Location"));
    }
    if has_forbidden_character(location) {
        return Err(bad_redirect(
            "the Location holds a character an address cannot hold",
        ));
    }
    // A fragment is never sent to a server.
    let location = &location[..location.find('#').unwrap_or(location.len())];
    if location.is_empty() {
        return Err(bad_redirect("the Location is only a fragment"));
    }
    let Some(base) = split_url(from) else {
        return Err(bad_redirect(
            "the address that was redirected is not an address",
        ));
    };

    let target = if location.starts_with("//") {
        format!("{}:{location}", base.scheme)
    } else if location.starts_with('/') {
        format!("{}://{}{location}", base.scheme, base.authority)
    } else {
        location.to_owned()
    };

    let Some(parts) =
        split_url(&target).filter(|parts| matches!(parts.scheme.as_str(), "https" | "http"))
    else {
        return Err(bad_redirect("the Location is not an http or https address"));
    };
    if split_authority(parts.authority).is_none() {
        return Err(bad_redirect(
            "the Location names no host, or names user-info or a bad port",
        ));
    }
    // Normalise the scheme and keep everything else as the server wrote it.
    Ok(format!(
        "{}://{}{}",
        parts.scheme, parts.authority, parts.remainder
    ))
}

/// Seconds for one ranged request of `range_length` bytes: HOP_TIMEOUT_SECONDS to get an answer
/// started plus the length at SLOW_LINE_BYTES_PER_SECOND, rounded up (60 + 2048 = 2108 s for the
/// largest merged range, 32 MiB, ranges.rs), and at least RANGE_HOP_TIMEOUT_SECONDS, at most
/// MAX_HOP_TIMEOUT_SECONDS (a member larger than a merged range is a range of its own, unsplit).
pub fn range_hop_timeout_seconds(range_length: u64) -> i32 {
    // In 64 bits, and rounded up: the length is a manifest's and may be anything. The quotient is
    // at most 2^50, so the sum cannot wrap.
    let transfer = range_length / SLOW_LINE_BYTES_PER_SECOND
        + u64::from(!range_length.is_multiple_of(SLOW_LINE_BYTES_PER_SECOND));
    let seconds = HOP_TIMEOUT_SECONDS as u64 + transfer;
    if seconds >= MAX_HOP_TIMEOUT_SECONDS as u64 {
        return MAX_HOP_TIMEOUT_SECONDS;
    }
    // Below MAX_HOP_TIMEOUT_SECONDS, so it fits.
    RANGE_HOP_TIMEOUT_SECONDS.max(seconds as i32)
}

/// The request headers for one hop to `hop_url`, "Name: value" each. `redirected` says how the
/// hop came about: false for the address the engine asked for (the first hop), true for one a
/// server answered with.
///
/// ```text
/// Accept: <request.accept>                     when it is not empty
/// Range: bytes=a-b                             for a ranged request
/// Authorization: Bearer <request.bearer_token> only when the token is not empty AND the hop is
///                                              not `redirected` AND is_api_github_url(hop_url)
/// X-GitHub-Api-Version: 2022-11-28             for a hop to api.github.com
/// ```
///
/// The token is a value in the result and nowhere else; log nothing built from it.
pub fn hop_headers(request: &FetchRequest, hop_url: &str, redirected: bool) -> Vec<String> {
    let mut headers = Vec::new();
    if !request.accept.is_empty() {
        headers.push(format!("Accept: {}", request.accept));
    }
    if request.range_length > 0
        && let Ok(range) = range_header_value(request.range_start, request.range_length)
    {
        headers.push(format!("Range: {range}"));
    }
    if is_api_github_url(hop_url) {
        headers.push(String::from("X-GitHub-Api-Version: 2022-11-28"));
        // The token goes with the request the engine wrote, to the address the engine wrote, and
        // with no other. An address a server answered with gets none, api.github.com's own
        // included: whoever can redirect a download must not thereby be able to aim an
        // authenticated request.
        if !redirected && !request.bearer_token.is_empty() {
            headers.push(format!("Authorization: Bearer {}", request.bearer_token));
        }
    }
    headers
}

/// `url` without user-info, query and fragment: scheme://host/path, for an error context or a log
/// line. A signed storage address carries its signature in the query.
pub fn display_url(url: &str) -> String {
    let Some(parts) = split_url(url) else {
        return String::from("(not an address)");
    };
    let authority = match parts.authority.rfind('@') {
        Some(at) => &parts.authority[at + 1..],
        None => parts.authority,
    };
    let path = &parts.remainder[..parts
        .remainder
        .find(['?', '#'])
        .unwrap_or(parts.remainder.len())];
    // Bounded: an address in a message is for a person to read.
    let mut shown = format!("{}://{authority}{path}", parts.scheme);
    const SHOWN: usize = 200;
    if shown.len() > SHOWN {
        // An address that went over the wire is ASCII, where this is the C++'s cut to the byte;
        // the cut never splits a character of one that is not.
        let mut cut = SHOWN;
        while !shown.is_char_boundary(cut) {
            cut -= 1;
        }
        shown.truncate(cut);
        shown.push_str("...");
    }
    shown
}

/// `text` with the query and the fragment cut from every address written in it. Wherever "://"
/// stands, what follows up to the next blank, control character or byte outside ASCII is taken
/// for an address (none of those can be in one that went over the wire), and everything from its
/// first '?' or '#' to its end is dropped:
///
/// ```text
/// "no answer from https://h/p?sig=S&se=1: timed out"  ->  "no answer from https://h/p timed out"
/// ```
///
/// It is what the words of a hop function, or of the library under it, pass through before they
/// are put in an Error: they may quote the address they were given, whole or with some of its
/// query hidden, and the address a release redirects to is a signed one (all of its query is the
/// secret, under names no list of secret parameter names knows).
pub fn without_queries(text: &str) -> String {
    const MARK: &str = "://";
    // Where an address in running text ends: at a byte no address that went over the wire can
    // hold. Nothing else ends it: a quote, a bracket and a semicolon may all stand in a query, and
    // stopping at one of them would keep what follows it.
    let ends_address = |byte: u8| byte <= 0x20 || byte >= 0x7f;
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    while at < text.len() {
        let Some(found) = text[at..].find(MARK) else {
            out.push_str(&text[at..]);
            break;
        };
        let mark = at + found;
        let mut end = mark + MARK.len();
        while end < bytes.len() && !ends_address(bytes[end]) {
            end += 1;
        }
        // [mark, end) is the address from its "://" on. It is kept up to its first '?' or '#',
        // with whatever stood before it; the rest of it is dropped. Every index here is at a
        // character boundary: the address is ASCII, and it ends at an ASCII byte or at the first
        // byte of a character.
        let kept = text[mark..end]
            .find(['?', '#'])
            .map_or(end, |cut| mark + cut);
        out.push_str(&text[at..kept]);
        at = end;
    }
    out
}

// One request, its redirects followed here (the module's "transport's rules").
fn follow_request(
    hop: &HopFn<'_>,
    request: &FetchRequest,
    stop: StopToken<'_>,
    progress: ProgressFn<'_>,
) -> Result<Vec<u8>> {
    let Some(hop) = hop else {
        return Err(Error::new(ErrorCode::Internal, "no transport was given"));
    };
    // The caller's bound is what keeps an unverified answer from filling memory or disk.
    if request.max_bytes == 0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a request must name the most bytes it accepts",
            display_url(&request.url),
        ));
    }
    let ranged = request.range_length > 0;
    if ranged && request.payload_size == 0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a ranged request must name the signed size of the file",
            display_url(&request.url),
        ));
    }
    // A feed of the owner's own may be a plain-http loopback server (a test, a local mirror); a
    // request that began on the open internet is https the whole way.
    let allow_loopback_http = is_loopback_http_url(&request.url);
    if !is_fetchable_url(&request.url, allow_loopback_http) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "only https addresses are fetched",
            display_url(&request.url),
        ));
    }

    let mut url = request.url.clone();
    let mut redirects = 0i32;
    loop {
        if stop_requested(stop) {
            return Err(cancelled(&url));
        }
        let one = HttpHop {
            url: url.clone(),
            user_agent: request.user_agent.clone(),
            headers: hop_headers(request, &url, redirects > 0),
            max_bytes: request.max_bytes,
            timeout_seconds: if ranged {
                range_hop_timeout_seconds(request.range_length)
            } else {
                HOP_TIMEOUT_SECONDS
            },
        };

        let reply = match hop(&one, stop, progress) {
            Ok(reply) => reply,
            Err(error) => {
                if error.code == ErrorCode::Unsupported && ranged {
                    // The body outgrew the range, and the hop could not say what the answer was
                    // (one that can sets HttpHopReply::overran, below). A server that ignored
                    // `Range` and began the whole payload is what does that; no status is claimed
                    // for it.
                    return Err(make_error(
                        ErrorCode::Unsupported,
                        PARTIAL_DOWNLOADS_REFUSED,
                        format!(
                            "reason=partial_downloads; {}; exceeded={} bytes",
                            display_url(&url),
                            request.max_bytes
                        ),
                    ));
                }
                return Err(hop_failure(&error, &url));
            }
        };

        if is_redirect_status(reply.status) {
            if redirects >= MAX_REDIRECTS {
                return Err(fetch_error(
                    ErrorCode::FileImportFailure,
                    "the server redirected too many times",
                    reply.status,
                    &url,
                    "too_many_redirects",
                    "",
                ));
            }
            let next = match resolve_redirect(&url, &reply.location) {
                Ok(next) => next,
                Err(error) => {
                    return Err(fetch_error(
                        ErrorCode::FileImportFailure,
                        error.message,
                        reply.status,
                        &url,
                        "bad_redirect",
                        "",
                    ));
                }
            };
            if !is_fetchable_url(&next, allow_loopback_http) {
                return Err(fetch_error(
                    ErrorCode::FileImportFailure,
                    "the server redirected to an address that is not https",
                    reply.status,
                    &url,
                    "bad_redirect",
                    "",
                ));
            }
            url = next;
            redirects += 1;
            continue;
        }

        if reply.status == 404 {
            return Err(fetch_error(
                ErrorCode::NotFound,
                "the server has nothing at this address (404)",
                404,
                &url,
                "",
                "",
            ));
        }
        if reply.status != 200 && reply.status != 206 {
            return Err(fetch_error(
                ErrorCode::FileImportFailure,
                message_for_status(reply.status),
                reply.status,
                &url,
                "",
                "",
            ));
        }
        if content_type_is_html(&reply.content_type) {
            return Err(fetch_error(
                ErrorCode::FileImportFailure,
                "the network answered with a web page instead of the file (a sign-in page?)",
                reply.status,
                &url,
                "html_page",
                "",
            ));
        }
        // Longer than was asked for: the hop cut it off (overran), or did not and handed it all
        // over. What it is depends on what the answer said it was.
        if reply.overran || reply.body.len() as u64 > request.max_bytes {
            let exceeded = format!("exceeded={} bytes", request.max_bytes);
            if !ranged {
                return Err(fetch_error(
                    ErrorCode::Unsupported,
                    "the answer is longer than this request allows",
                    reply.status,
                    &url,
                    "",
                    &exceeded,
                ));
            }
            if reply.status == 206 {
                // A part of the file, and more of it than the part asked for: not a server that
                // ignores `Range`, but one whose answer is not the range.
                return Err(fetch_error(
                    ErrorCode::FileImportFailure,
                    "the server sent more than the part of the file that was asked for",
                    206,
                    &url,
                    "bad_range",
                    &format!(
                        "asked for {} bytes from {} of {}; {exceeded}",
                        request.range_length, request.range_start, request.payload_size
                    ),
                ));
            }
            // A 200 that began the whole file.
            return Err(fetch_error(
                ErrorCode::Unsupported,
                PARTIAL_DOWNLOADS_REFUSED,
                reply.status,
                &url,
                "",
                &exceeded,
            ));
        }
        if ranged {
            if let Err(why) = check_range_response(
                reply.status,
                &reply.content_range,
                reply.body.len() as u64,
                request.range_start,
                request.range_length,
                request.payload_size,
            ) {
                return Err(range_refused(why, request, &url, &reply.content_range));
            }
        } else if reply.status != 200 {
            return Err(fetch_error(
                ErrorCode::FileImportFailure,
                "the server sent part of a file that was asked for whole",
                reply.status,
                &url,
                "",
                "",
            ));
        }
        return Ok(reply.body);
    }
}

/// A Fetch over `hop`, which moves one request on the wire: the rules of the module's "transport's
/// rules", and no word of the request's token or of an address's query in any Error it returns.
pub fn make_fetch<'a>(hop: HopFn<'a>) -> Fetch<'a> {
    fetch_fn(move |request, stop, progress| {
        follow_request(&hop, request, stop, progress)
            .map_err(|error| without_secrets(error, &request.bearer_token))
    })
}

/// The short machine-readable token for a failure of a Fetch (or of stage(), which adds its own
/// `reason=<token>` to its contexts): the `reason=` token the Error carries (the transport's are
/// "html_page", "bad_range", "bad_redirect" and "too_many_redirects"), else "cancelled",
/// "http_<status>" (http_404, http_403, http_429), "offline" (status=0, or no status at all),
/// "partial_downloads", "too_large"; "error" for anything else. What check()'s reason holds.
///
/// A `reason=` counts only where this program writes one: at the very start of the context, or
/// straight after a leading `status=<n>; `. One further on (inside an address, in a server's
/// header) is text, not a token.
pub fn fetch_failure_reason(error: &Error) -> String {
    // The two leading fields this program writes, `status=<n>; ` and then `reason=<token>`, and
    // nothing after them: what follows may be an address or a server's words, and must not be
    // able to name a reason. Only a number and then the separator lead to a second field:
    // `status=200x` and a `status=` with no number have none (leading_status).
    let led = leading_status(&error.context);
    if let Some(rest) = led.rest.strip_prefix("reason=") {
        let token = &rest[..rest.find(';').unwrap_or(rest.len())];
        if !token.is_empty() {
            return token.to_owned();
        }
    }

    match error.code {
        ErrorCode::InvalidState => String::from("cancelled"),
        ErrorCode::NotFound => String::from("http_404"),
        ErrorCode::Unsupported => {
            if error.message == PARTIAL_DOWNLOADS_REFUSED {
                String::from("partial_downloads")
            } else {
                String::from("too_large")
            }
        }
        // No status at all is a failure to get an answer, like status 0.
        ErrorCode::FileImportFailure => {
            if led.status == 0 {
                String::from("offline")
            } else {
                format!("http_{}", led.status)
            }
        }
        _ => String::from("error"),
    }
}
