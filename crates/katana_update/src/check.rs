//! Asking the feed whether a newer release exists. Window only, only when install.json exists, by
//! default every 24 h plus 0 to 2 h of jitter; typed `UPDATE CHECK` at any time.
//!
//! check() fetches the signature, then the manifest, verifies, and only then parses (trust.rs).
//! ANY failure to learn the answer (offline, 403, 404, 429, a portal's HTML, a bad signature) is
//! the status Unreachable, not an error of the call: silent when the check was automatic, one
//! sentence when a person asked. The Result's own error is for a call that could not even be made
//! (a stop token already requested, an unusable argument).
//!
//! `reason` is a short machine-readable token (the verbs print it as `reason=`): "offline",
//! "http_403", "http_404", "http_429", "http_<n>", "bad_signature", "wrong_product",
//! "wrong_target", "malformed", "untrusted_key", "placeholder_keys", "partial_downloads",
//! "cancelled", "disabled_build", "development_tree", "skipped", "blocked_before". (No check says
//! "root_not_writable" or "headless": whether the install folder can be written is found by the
//! helper, which refuses under the first, and the second is the verbs' answer to a headless
//! window's DOWNLOAD and APPLY; a headless check is a check like any other.) Added by check()
//! itself: "html_page" (a web page where the file should be: a captive portal), "too_large" (a
//! file longer than the engine reads before it has verified it), "wrong_channel", "asset_missing"
//! (the private route's release has no manifest or signature asset), "verify_unavailable" (the
//! signature could not be checked at all, which says nothing about the release).
//! fetch_failure_reason (feed.rs) makes the transport's tokens; `detail` carries the failure's own
//! sentence for a person.
//!
//! Before the answer is known the engine asks `check_is_due` (state.rs) whether an AUTOMATIC
//! check is due; a typed `UPDATE CHECK` asks the feed regardless.
//!
//! The order is the point. The signature is fetched first and the manifest second, both bounded
//! by the engine (feed.rs: a Fetch is never given max_bytes 0), and NOTHING in either is believed
//! until verify_manifest (trust.rs) has checked the signature over the manifest's exact bytes;
//! only then is the manifest read, and only then compared with the install.
//!
//! The private-repository route is the fallback and not the default (feed.rs): it costs REST
//! quota, and it is the only place a token is used. It is tried once, only when the signature on
//! the public route answered 404 (which is what a private repository says to an anonymous read),
//! the config holds a token and the feed is GitHub's own.

use crate::feed::{
    FeedConfig, FeedRoute, Fetch, FetchRequest, MAX_MANIFEST_BYTES, MAX_RELEASE_JSON_BYTES,
    MAX_SIGNATURE_BYTES, ReleaseAsset, StopToken, asset_api_url, fetch_failure_reason,
    manifest_asset_name, manifest_url, parse_release_assets, release_lookup_url,
    signature_asset_name, signature_url, stop_requested,
};
use crate::install::{Install, InstallKind};
use crate::manifest::Manifest;
use crate::state::UpdateState;
use crate::trust::{Trust, is_newer_than, verify_manifest};
use crate::version::{self, parse_version};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CheckStatus {
    /// A release above the installed (version, sequence); `manifest` is set.
    Available,
    /// The feed's release is not above it.
    Current,
    /// The feed could not be read or did not verify; `reason` says why.
    #[default]
    Unreachable,
    /// This copy cannot update: a development tree, or no feed in this build. (An install folder
    /// that cannot be written is not found by a check: the helper finds it.)
    Unsupported,
    /// An apply of this very release was rolled back here: not offered again until the person
    /// clears the list of such releases (`UPDATE BLOCKED CLEAR`).
    Blocked,
}

/// "available" "current" "unreachable" "unsupported" "blocked".
pub fn to_string(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Available => "available",
        CheckStatus::Current => "current",
        CheckStatus::Unreachable => "unreachable",
        CheckStatus::Unsupported => "unsupported",
        CheckStatus::Blocked => "blocked",
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckResult {
    pub status: CheckStatus,
    pub installed_version: String,
    /// When a manifest was read, even if not newer.
    pub latest_version: String,
    pub notes_url: String,
    pub reason: String,
    /// Verified; set for Available and Current.
    pub manifest: Option<Manifest>,
    /// The route the answer came by (feed.rs, "THE PRIVATE-REPOSITORY ROUTE"). The caller copies
    /// it into FeedConfig::route before staging, so the payload is fetched the same way the
    /// manifest was.
    pub route: FeedRoute,
    /// The failure's own sentence (the transport's or the verifier's message, never a URL with a
    /// query and never the token), for "Could not check for updates: <detail>". Empty on success.
    /// (The verbs give a Blocked answer one as well, which is no failure's: how the release is
    /// offered again.)
    pub detail: String,
}

const STABLE: &str = "stable";

// An Error of this module's own, with the reason token fetch_failure_reason reads back.
fn failure(code: ErrorCode, message: impl Into<String>, reason: &str) -> Error {
    make_error(code, message, format!("reason={reason}"))
}

// The first non-blank character is '<': markup where a manifest ('{') or a signature line ('e')
// should be. A captive portal answers 200 with its sign-in page for any address.
fn looks_like_markup(body: &[u8]) -> bool {
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    body.iter()
        .find(|&&c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
        .is_some_and(|&c| c == b'<')
}

fn unreachable_by(mut result: CheckResult, error: &Error) -> CheckResult {
    result.status = CheckStatus::Unreachable;
    result.reason = fetch_failure_reason(error);
    result.detail = error.message.clone();
    result.manifest = None;
    result
}

fn unreachable_with(mut result: CheckResult, reason: &str, detail: &str) -> CheckResult {
    result.status = CheckStatus::Unreachable;
    result.reason = String::from(reason);
    result.detail = String::from(detail);
    result.manifest = None;
    result
}

// What verify_manifest's failures mean to a person, worked out from the code and, where one code
// covers several, the message (trust.rs names the code as the contract; the message tells
// "signed for another product" from "the signature does not match", which share
// InvalidArgument). Internal is "could not check" and says nothing about the release.
fn verify_failure_reason(error: &Error, trust: &Trust) -> &'static str {
    match error.code {
        ErrorCode::Internal => "verify_unavailable",
        ErrorCode::Unsupported => {
            if trust.placeholder {
                "placeholder_keys"
            } else {
                "untrusted_key"
            }
        }
        ErrorCode::ParseFailure => "malformed",
        ErrorCode::InvalidArgument => {
            let says = |words: &str| error.message.contains(words);
            if says("another product") {
                "wrong_product"
            } else if says("another target") {
                "wrong_target"
            } else if says("another channel") {
                "wrong_channel"
            } else if says("the signature") {
                "bad_signature"
            } else {
                // Signed, and still not a manifest this program accepts (a path rule, a size
                // rule), or an empty or oversize file.
                "malformed"
            }
        }
        _ => "bad_signature",
    }
}

struct Files {
    signature: Vec<u8>,
    manifest: Vec<u8>,
}

fn no_transport() -> Error {
    Error::new(ErrorCode::InvalidArgument, "check was given no transport")
}

// One request through the transport; check() has refused a missing one before any is made.
fn fetch_one(fetch: &Fetch<'_>, request: &FetchRequest, stop: StopToken<'_>) -> Result<Vec<u8>> {
    let Some(fetch) = fetch else {
        return Err(no_transport());
    };
    fetch(request, stop, None)
}

fn whole_body(feed: &FeedConfig, url: String, max_bytes: u64) -> FetchRequest {
    FetchRequest {
        url,
        max_bytes,
        user_agent: feed.user_agent.clone(),
        ..FetchRequest::default()
    }
}

// Both files from the Download route: GET .sig, then .json. `signature_not_found` is set when the
// FIRST request answered 404, the one answer that sends a token holder to the other route.
fn fetch_from_download_route(
    fetch: &Fetch<'_>,
    feed: &FeedConfig,
    stop: StopToken<'_>,
    signature_not_found: &mut bool,
) -> Result<Files> {
    let signature = fetch_one(
        fetch,
        &whole_body(feed, signature_url(feed), MAX_SIGNATURE_BYTES),
        stop,
    )
    .inspect_err(|error| *signature_not_found = error.code == ErrorCode::NotFound)?;
    let manifest = fetch_one(
        fetch,
        &whole_body(feed, manifest_url(feed), MAX_MANIFEST_BYTES),
        stop,
    )?;
    Ok(Files {
        signature,
        manifest,
    })
}

fn api_request(feed: &FeedConfig, url: String, accept: &str, max_bytes: u64) -> FetchRequest {
    FetchRequest {
        accept: String::from(accept),
        // Every address built here is api.github.com's (feed.rs). make_fetch sends the token to
        // that host only, whatever this field says, and with the request to this address only: no
        // address a redirect names is sent it.
        bearer_token: feed.token.clone(),
        ..whole_body(feed, url, max_bytes)
    }
}

// The asset `name` of the release's list.
fn find_asset(assets: &[ReleaseAsset], name: &str) -> Result<ReleaseAsset> {
    assets
        .iter()
        .find(|asset| asset.name == name)
        .cloned()
        .ok_or_else(|| {
            failure(
                ErrorCode::NotFound,
                format!("the latest release has no asset named {name}"),
                "asset_missing",
            )
        })
}

// releases/latest, then the two assets by id (feed.rs). The release is the one GitHub calls latest
// at this moment; the stage looks the payload up again by the manifest's tag.
fn fetch_from_api_route(
    fetch: &Fetch<'_>,
    feed: &FeedConfig,
    stop: StopToken<'_>,
) -> Result<Files> {
    let release = fetch_one(
        fetch,
        &api_request(
            feed,
            release_lookup_url(feed),
            "application/vnd.github+json",
            MAX_RELEASE_JSON_BYTES,
        ),
        stop,
    )?;
    let assets = parse_release_assets(&release)
        .map_err(|error| failure(ErrorCode::ParseFailure, error.message, "malformed"))?;
    let signature_asset = find_asset(&assets, &signature_asset_name(&feed.target))?;
    let manifest_asset = find_asset(&assets, &manifest_asset_name(&feed.target))?;

    let signature = fetch_one(
        fetch,
        &api_request(
            feed,
            asset_api_url(feed, signature_asset.id),
            "application/octet-stream",
            MAX_SIGNATURE_BYTES,
        ),
        stop,
    )?;
    let manifest = fetch_one(
        fetch,
        &api_request(
            feed,
            asset_api_url(feed, manifest_asset.id),
            "application/octet-stream",
            MAX_MANIFEST_BYTES,
        ),
        stop,
    )?;
    Ok(Files {
        signature,
        manifest,
    })
}

/// `install` says what is installed and which target to ask for; `trust` is production in the
/// programs and a test key in the tests. The feed's token, when the config has one, goes only to
/// api.github.com (feed.rs).
pub fn check(
    fetch: &Fetch<'_>,
    feed: &FeedConfig,
    install: &Install,
    trust: &Trust,
    stop: StopToken<'_>,
) -> Result<CheckResult> {
    if fetch.is_none() {
        return Err(no_transport());
    }
    if feed.target.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the feed names no target to ask for",
        ));
    }
    // Only the stable channel exists (a beta channel is not built: docs/update.md, "Not done"),
    // and verify_manifest holds the signed channel to it.
    if feed.channel != STABLE {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "only the stable channel exists",
            feed.channel.clone(),
        ));
    }

    let mut result = CheckResult {
        installed_version: version::to_string(&install.version),
        ..CheckResult::default()
    };

    // A development tree is never asked about (install.rs): no network is touched.
    if install.kind == InstallKind::Development {
        result.status = CheckStatus::Unsupported;
        result.reason = String::from("development_tree");
        result.detail = String::from(
            "this copy of Katana is a development build, which is not updated in place",
        );
        return Ok(result);
    }
    if feed.repo.is_empty() && feed.feed_base.is_empty() {
        result.status = CheckStatus::Unsupported;
        result.reason = String::from("disabled_build");
        result.detail = String::from("this build of Katana has no update feed");
        return Ok(result);
    }
    if install.target != feed.target {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the feed asks for another target than the install's",
            format!("feed '{}', install '{}'", feed.target, install.target),
        ));
    }
    if stop_requested(stop) {
        return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
    }

    let mut route = FeedRoute::Download;
    let mut signature_not_found = false;
    let mut files = fetch_from_download_route(fetch, feed, stop, &mut signature_not_found);
    if files.is_err()
        && signature_not_found
        && !feed.token.is_empty()
        && feed.feed_base.is_empty()
        && !stop_requested(stop)
    {
        // What a private repository says to an anonymous read. Once, and only for GitHub.
        route = FeedRoute::Api;
        files = fetch_from_api_route(fetch, feed, stop);
    }
    result.route = route;
    let files = match files {
        Ok(files) => files,
        Err(error) => return Ok(unreachable_by(result, &error)),
    };

    if looks_like_markup(&files.signature) || looks_like_markup(&files.manifest) {
        return Ok(unreachable_with(
            result,
            "html_page",
            "the network answered with a web page instead of the update information (a sign-in \
             page?)",
        ));
    }

    let manifest = match verify_manifest(&files.manifest, &files.signature, trust, install) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Ok(unreachable_with(
                result,
                verify_failure_reason(&error, trust),
                &error.message,
            ));
        }
    };

    result.latest_version = version::to_string(&manifest.version);
    result.notes_url = manifest.notes_url.clone();
    // The version and the sequence together (trust.rs, is_newer_than): a downgrade, a replay of
    // the installed build and the release a fresh install came from are all Current.
    result.status = if is_newer_than(&manifest, install) {
        CheckStatus::Available
    } else {
        CheckStatus::Current
    };
    result.manifest = Some(manifest);
    Ok(result)
}

/// What the person has already decided about a release the feed offers, applied to an answer
/// check() gave (check() reads no per-user file, so it is called with the state the caller read).
/// A release whose sequence is in `state.blocked_sequences` (its apply was rolled back here) is
/// Blocked, reason "blocked_before": not offered again, until `UPDATE BLOCKED CLEAR` empties that
/// list. Otherwise a release whose version is `state.skipped_version` ("UPDATE SKIP") is Current,
/// reason "skipped". The window's automatic check applies this; a typed `UPDATE CHECK` may choose
/// to show the skipped release anyway. Only an Available answer changes; the manifest,
/// latest_version and notes_url stay.
pub fn apply_update_state(result: &mut CheckResult, state: &UpdateState) {
    if result.status != CheckStatus::Available {
        return;
    }
    let Some(manifest) = &result.manifest else {
        return;
    };
    let (sequence, offered) = (manifest.sequence, manifest.version);
    if state.blocked_sequences.contains(&sequence) {
        result.status = CheckStatus::Blocked;
        result.reason = String::from("blocked_before");
        return;
    }
    // As versions and not as text: "0.3" and "0.3.0" are one release.
    if !state.skipped_version.is_empty()
        && let Ok(skipped) = parse_version(&state.skipped_version)
        && skipped == offered
    {
        result.status = CheckStatus::Current;
        result.reason = String::from("skipped");
    }
}
