//! check() (check.rs): asking the feed whether a newer release exists. The twin of
//! tests/update/test_check.cpp.
//!
//! Where the expectations come from (CLAUDE.md, section 4). The releases are the fixtures of
//! tests/update/fixtures/release: manifests made by the Python producer and signed by
//! `openssl pkeyutl` with the published RFC 8032 test keys, so "this signature is good" is settled
//! outside this program. What each fixture is (v1 is 0.9.0, sequence 1790000000; v2 is 0.10.0,
//! sequence 1792312800) and why each bad/ folder is bad is written in
//! tools/update_make_fixtures.py. A manifest that must differ in one signed field (another
//! product, an old `expires`) is the v2 text with that field changed and signed here. The C++
//! signs it with OpenSSL; this signs it with ed25519-dalek's signing key, which is RFC 8032's
//! deterministic signature and so the very bytes OpenSSL gives for the same seed and message, made
//! by the signer and not by the verifier under test (verify_manifest's strict checks). The feed is
//! a fake Fetch keyed by address that records every request it is given, so what is asked for, in
//! what order, with what bound and with what token is read off the record.

pub mod test_key;

use ed25519_dalek::{Signer, SigningKey};
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, read_file_bytes};
use katana_core::{Error, ErrorCode};
use katana_update::check::{
    CheckResult, CheckStatus, apply_update_state, check, to_string as status_word,
};
use katana_update::feed::{
    FeedConfig, FeedRoute, Fetch, FetchRequest, MAX_MANIFEST_BYTES, MAX_RELEASE_JSON_BYTES,
    MAX_SIGNATURE_BYTES, asset_api_url, fetch_fn, manifest_url, release_lookup_url, signature_url,
};
use katana_update::install::{Install, InstallKind};
use katana_update::manifest::is_expired;
use katana_update::signature::{PublicKey, key_id_of};
use katana_update::state::UpdateState;
use katana_update::trust::{Trust, TrustedKey};
use katana_update::version::parse_version;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use test_key::{TEST_ACTIVE_PUBLIC_HEX, TEST_ACTIVE_SEED_HEX, test_trust};

const SECRET_TOKEN: &str = "ghp_SECRET-TOKEN-0123456789";

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/update/fixtures"
);

// ---- the feed -------------------------------------------------------------------------------------

fn github_feed() -> FeedConfig {
    FeedConfig {
        repo: String::from("kjarada/Katana"),
        target: String::from("win64"),
        user_agent: String::from("Katana/0.9.0 (win64)"),
        ..FeedConfig::default()
    }
}

fn install_of(version: &str, sequence: u64, record: bool) -> Install {
    Install {
        root: PathBuf::from("R"),
        target: String::from("win64"),
        kind: InstallKind::Archive,
        version: parse_version(version).expect("a version"),
        sequence,
        has_installed_record: record,
        ..Install::default()
    }
}

// 0.9.0 as the helper recorded it.
fn installed_v1() -> Install {
    install_of("0.9.0", 1790000000, true)
}

#[derive(Clone)]
struct Release {
    json: Vec<u8>,
    sig: Vec<u8>,
}

fn read_text(relative: &str) -> Vec<u8> {
    read_file_bytes(&path_from_utf8(format!("{FIXTURES}/{relative}")))
        .unwrap_or_else(|error| panic!("{relative}: {}", error.describe()))
}

fn fixture(name: &str) -> Release {
    Release {
        json: read_text(&format!("release/{name}/Katana-update-win64.json")),
        sig: read_text(&format!("release/{name}/Katana-update-win64.json.sig")),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---- signing ----------------------------------------------------------------------------------------

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let at = |i: usize| chunk.get(i).map_or(0, |&byte| u32::from(byte));
        let triple = (at(0) << 16) | (at(1) << 8) | at(2);
        let digit = |shift: u32| char::from(ALPHABET[((triple >> shift) & 63) as usize]);
        out.push(digit(18));
        out.push(digit(12));
        out.push(if chunk.len() > 1 { digit(6) } else { '=' });
        out.push(if chunk.len() > 2 { digit(0) } else { '=' });
    }
    out
}

fn seed_of(hex: &str) -> [u8; 32] {
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hexadecimal");
    }
    seed
}

// The signature line of `message` under the active test key: `ed25519 <keyid> <base64>` and a
// line feed.
fn sign_line(message: &[u8]) -> Vec<u8> {
    let key = SigningKey::from_bytes(&seed_of(TEST_ACTIVE_SEED_HEX));
    let signature = key.sign(message).to_bytes();
    format!(
        "ed25519 {} {}\n",
        &TEST_ACTIVE_PUBLIC_HEX[..16],
        base64(&signature)
    )
    .into_bytes()
}

// v2's text with `from` replaced by `to` (exactly once), signed by the active test key.
fn changed_v2(from: &str, to: &str) -> Release {
    let mut release = fixture("v2");
    let at = find(&release.json, from.as_bytes());
    assert!(at.is_some(), "the fixture no longer holds: {from}");
    if let Some(at) = at {
        release.json = [
            &release.json[..at],
            to.as_bytes(),
            &release.json[at + from.len()..],
        ]
        .concat();
    }
    release.sig = sign_line(&release.json);
    release
}

// A fake feed: addresses to bodies or errors; anything else is a 404, as on the real host.
#[derive(Default)]
struct FakeFeed {
    bodies: BTreeMap<String, Vec<u8>>,
    errors: BTreeMap<String, Error>,
    requests: RefCell<Vec<FetchRequest>>,
}

impl FakeFeed {
    fn serve(&mut self, url: String, body: impl Into<Vec<u8>>) {
        self.bodies.insert(url, body.into());
    }

    fn fail(&mut self, url: String, error: Error) {
        self.errors.insert(url, error);
    }

    fn serve_release(&mut self, feed: &FeedConfig, release: &Release) {
        self.serve(signature_url(feed), release.sig.clone());
        self.serve(manifest_url(feed), release.json.clone());
    }

    fn fetch(&self) -> Fetch<'_> {
        fetch_fn(move |request, _stop, _progress| {
            self.requests.borrow_mut().push(request.clone());
            if let Some(error) = self.errors.get(&request.url) {
                return Err(error.clone());
            }
            if let Some(body) = self.bodies.get(&request.url) {
                return Ok(body.clone());
            }
            Err(make_error(
                ErrorCode::NotFound,
                "nothing at this address",
                format!("status=404; {}", request.url),
            ))
        })
    }

    fn requests(&self) -> Vec<FetchRequest> {
        self.requests.borrow().clone()
    }
}

fn status(code: ErrorCode, number: i32) -> Error {
    make_error(
        code,
        format!("the server said {number}"),
        format!("status={number}; https://github.com/x"),
    )
}

fn run_with(feed: &FakeFeed, config: &FeedConfig, install: &Install, trust: &Trust) -> CheckResult {
    let fetch = feed.fetch();
    check(&fetch, config, install, trust, None)
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn run(feed: &FakeFeed, config: &FeedConfig, install: &Install) -> CheckResult {
    run_with(feed, config, install, test_trust())
}

// ---- available, current -----------------------------------------------------------------------------

mod check {
    use super::*;

    #[test]
    fn a_newer_release_is_available_with_its_verified_manifest() {
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve_release(&config, &fixture("v2"));
        let result = run(&feed, &config, &installed_v1());

        assert_eq!(result.status, CheckStatus::Available);
        assert_eq!(result.installed_version, "0.9.0");
        assert_eq!(
            result.latest_version, "0.10.0",
            "0.10 is above 0.9 as a number, not as text"
        );
        assert_eq!(
            result.notes_url,
            "https://github.com/kjarada/Katana/releases/tag/v0.10.0"
        );
        assert!(result.reason.is_empty());
        assert!(result.detail.is_empty());
        assert_eq!(result.route, FeedRoute::Download);
        let manifest = result.manifest.expect("the verified manifest");
        assert_eq!(manifest.sequence, 1792312800);
        assert_eq!(manifest.tag, "v0.10.0");
        // The manifest carries the exact bytes that were signed, for the stage and the helper.
        assert_eq!(manifest.signed_json, fixture("v2").json);
        assert_eq!(manifest.signature_line, fixture("v2").sig);
    }

    #[test]
    fn the_signature_is_fetched_first_then_the_manifest_each_with_the_engines_bound() {
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve_release(&config, &fixture("v2"));
        let _ = run(&feed, &config, &installed_v1());

        let requests = feed.requests();
        assert_eq!(requests.len(), 2);
        // Written out: the addresses of the Download route (feed.rs).
        assert_eq!(
            requests[0].url,
            "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json.sig"
        );
        assert_eq!(
            requests[1].url,
            "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json"
        );
        assert_eq!(requests[0].max_bytes, MAX_SIGNATURE_BYTES);
        assert_eq!(requests[1].max_bytes, MAX_MANIFEST_BYTES);
        for request in &requests {
            assert_ne!(
                request.max_bytes, 0,
                "a Fetch is never given an unbounded request"
            );
            assert_eq!(request.user_agent, "Katana/0.9.0 (win64)");
            assert!(
                request.bearer_token.is_empty(),
                "the Download route never carries a token"
            );
            assert_eq!(request.range_length, 0, "a whole body");
        }
    }

    #[test]
    fn nothing_is_fetched_after_a_signature_that_could_not_be_read() {
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.fail(
            signature_url(&config),
            status(ErrorCode::FileImportFailure, 500),
        );
        feed.serve(manifest_url(&config), fixture("v2").json);
        let result = run(&feed, &config, &installed_v1());
        assert_eq!(result.status, CheckStatus::Unreachable);
        assert_eq!(feed.requests().len(), 1, "the signature gates the manifest");
    }

    #[test]
    fn the_same_or_an_older_release_is_current_and_never_offered() {
        let config = github_feed();
        let cases = [
            (
                "the installed version and sequence",
                "v2",
                install_of("0.10.0", 1792312800, true),
            ),
            (
                "a build with a higher sequence installed",
                "v2",
                install_of("0.10.0", 1792312801, true),
            ),
            (
                "a higher version installed",
                "v2",
                install_of("0.11.0", 1700000000, true),
            ),
            // The fixtures' bad/: a validly signed 0.9.0 with a sequence ABOVE v2's, offered to
            // 0.10.0.
            (
                "a downgrade by version",
                "bad/downgrade-version",
                install_of("0.10.0", 1792312800, true),
            ),
            // ... and a validly signed 0.10.0 with a sequence BELOW v2's, offered to that install.
            (
                "a replay of an older build of the same version",
                "bad/downgrade-sequence",
                install_of("0.10.0", 1792312800, true),
            ),
        ];
        for (what, release, install) in cases {
            let mut feed = FakeFeed::default();
            feed.serve_release(&config, &fixture(release));
            let result = run(&feed, &config, &install);
            assert_eq!(result.status, CheckStatus::Current, "{what}");
            assert!(
                result.manifest.is_some(),
                "{what}: the verified manifest is kept"
            );
            assert!(!result.latest_version.is_empty(), "{what}");
        }
    }

    #[test]
    fn a_fresh_install_is_not_offered_the_release_it_came_from() {
        // No installed.json: a fresh install's sequence is the placeholder 0, whatever sequence
        // its package names (install.rs), and is_newer_than leaves it out while there is no record
        // (trust.rs), so the same version is not newer.
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve_release(&config, &fixture("v2"));
        assert_eq!(
            run(&feed, &config, &install_of("0.10.0", 0, false)).status,
            CheckStatus::Current
        );
        // ... but a higher version is offered to it.
        assert_eq!(
            run(&feed, &config, &install_of("0.9.0", 0, false)).status,
            CheckStatus::Available
        );
    }

    #[test]
    fn a_release_signed_by_the_spare_key_is_as_good_as_one_signed_by_the_active_key() {
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve_release(&config, &fixture("v2-spare-signed"));
        assert_eq!(
            run(&feed, &config, &installed_v1()).status,
            CheckStatus::Available
        );
    }

    #[test]
    fn a_notify_only_release_is_available_and_says_so() {
        let config = FeedConfig {
            target: String::from("macos-arm64"),
            ..github_feed()
        };
        let install = Install {
            target: String::from("macos-arm64"),
            ..installed_v1()
        };
        let mut feed = FakeFeed::default();
        feed.serve(
            signature_url(&config),
            read_text("release/v2-macos-notify/Katana-update-macos-arm64.json.sig"),
        );
        feed.serve(
            manifest_url(&config),
            read_text("release/v2-macos-notify/Katana-update-macos-arm64.json"),
        );
        let result = run(&feed, &config, &install);
        assert_eq!(result.status, CheckStatus::Available);
        let manifest = result.manifest.expect("the verified manifest");
        assert!(manifest.notify_only);
        assert!(manifest.payload.is_none());
    }

    #[test]
    fn an_expired_manifest_is_still_offered_expiry_is_advisory() {
        // `expires` 2021-01-01, long gone: the dialog says the information is old, and the
        // release is offered all the same (manifest.rs). The clock is the case's: is_expired takes
        // it. (The C++ skips without OpenSSL's headers; this signer is always there.)
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve_release(
            &config,
            &changed_v2(
                "\"expires\":\"2027-04-16T08:40:00Z\"",
                "\"expires\":\"2021-01-01T00:00:00Z\"",
            ),
        );
        let result = run(&feed, &config, &installed_v1());
        assert_eq!(result.status, CheckStatus::Available);
        let manifest = result.manifest.expect("the verified manifest");
        assert!(
            is_expired(&manifest, 1_800_000_000),
            "January 2027 is after 2021"
        );
        assert!(
            !is_expired(&manifest, 1_600_000_000),
            "September 2020 is before it"
        );
    }

    // ---- every way of not learning the answer is Unreachable, with a reason ---------------------

    #[test]
    fn a_feed_that_cannot_be_read_is_unreachable_with_the_reason_and_never_an_error_of_the_call() {
        let config = github_feed();
        let failures = [
            (
                "a private repository or a missing release (404)",
                status(ErrorCode::NotFound, 404),
                "http_404",
            ),
            (
                "the rate limit (403)",
                status(ErrorCode::FileImportFailure, 403),
                "http_403",
            ),
            (
                "too many requests (429)",
                status(ErrorCode::FileImportFailure, 429),
                "http_429",
            ),
            (
                "the host in trouble (503)",
                status(ErrorCode::FileImportFailure, 503),
                "http_503",
            ),
            (
                "no network",
                make_error(
                    ErrorCode::FileImportFailure,
                    "the service's host name could not be found",
                    "status=0; https://github.com/x",
                ),
                "offline",
            ),
            // As the transport reports a page (feed.rs: `status=<n>; reason=html_page`).
            (
                "a captive portal's page",
                make_error(
                    ErrorCode::FileImportFailure,
                    "a web page instead of the file",
                    "status=200; reason=html_page; https://github.com/x",
                ),
                "html_page",
            ),
            (
                "a redirect out of https",
                make_error(
                    ErrorCode::FileImportFailure,
                    "redirected to an address that is not https",
                    "status=302; reason=bad_redirect; https://github.com/x",
                ),
                "bad_redirect",
            ),
            (
                "a body past the bound",
                make_error(ErrorCode::Unsupported, "longer than allowed", "status=200"),
                "too_large",
            ),
            (
                "a cancelled transfer",
                Error::new(ErrorCode::InvalidState, "cancelled"),
                "cancelled",
            ),
        ];
        for (what, error, reason) in &failures {
            // The signature failing, and then (the signature good) the manifest failing.
            for on_signature in [true, false] {
                let mut feed = FakeFeed::default();
                feed.serve(signature_url(&config), fixture("v2").sig);
                feed.serve(manifest_url(&config), fixture("v2").json);
                let failing = if on_signature {
                    signature_url(&config)
                } else {
                    manifest_url(&config)
                };
                feed.fail(failing, error.clone());
                let fetch = feed.fetch();
                let result = check(&fetch, &config, &installed_v1(), test_trust(), None)
                    .unwrap_or_else(|failure| panic!("{what}: {}", failure.describe()));
                assert_eq!(result.status, CheckStatus::Unreachable, "{what}");
                assert_eq!(result.reason, *reason, "{what}");
                assert_eq!(result.detail, error.message, "{what}");
                assert!(result.manifest.is_none(), "{what}");
            }
        }
    }

    #[test]
    fn a_web_page_where_the_signature_or_the_manifest_should_be_is_a_portal() {
        let config = github_feed();
        let page: &[u8] = b"<!DOCTYPE html><html><body>Sign in to the guest network</body></html>";
        let bodies: [Vec<u8>; 4] = [
            page.to_vec(),
            [b"  \r\n\t".as_slice(), page].concat(),
            [b"\xEF\xBB\xBF".as_slice(), page].concat(),
            b"<html>".to_vec(),
        ];
        for body in &bodies {
            {
                let mut feed = FakeFeed::default();
                feed.serve_release(&config, &fixture("v2"));
                feed.serve(signature_url(&config), body.clone());
                let result = run(&feed, &config, &installed_v1());
                assert_eq!(result.status, CheckStatus::Unreachable);
                assert_eq!(result.reason, "html_page");
            }
            {
                let mut feed = FakeFeed::default();
                feed.serve_release(&config, &fixture("v2"));
                feed.serve(manifest_url(&config), body.clone());
                let result = run(&feed, &config, &installed_v1());
                assert_eq!(result.status, CheckStatus::Unreachable);
                assert_eq!(result.reason, "html_page");
            }
        }
    }

    #[test]
    fn a_truncated_signature_or_manifest_does_not_verify() {
        let config = github_feed();
        let good = fixture("v2");
        let cases = [
            (
                "a signature cut short",
                good.sig[..60].to_vec(),
                good.json.clone(),
                "malformed",
            ),
            (
                "an empty signature",
                Vec::new(),
                good.json.clone(),
                "malformed",
            ),
            (
                "a manifest cut in half",
                good.sig.clone(),
                good.json[..good.json.len() / 2].to_vec(),
                "bad_signature",
            ),
            (
                "a manifest one byte short",
                good.sig.clone(),
                good.json[..good.json.len() - 1].to_vec(),
                "bad_signature",
            ),
            (
                "an empty manifest",
                good.sig.clone(),
                Vec::new(),
                "malformed",
            ),
        ];
        for (what, sig, json, reason) in cases {
            let mut feed = FakeFeed::default();
            feed.serve(signature_url(&config), sig);
            feed.serve(manifest_url(&config), json);
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.status, CheckStatus::Unreachable, "{what}");
            assert_eq!(result.reason, reason, "{what}");
            assert!(result.manifest.is_none(), "{what}");
        }
    }

    #[test]
    fn anything_that_does_not_verify_is_never_trusted() {
        let config = github_feed();
        let cases = [
            ("bad/flipped-manifest-bit", "bad_signature"), // one bit of a hash digit
            ("bad/swapped-sig", "bad_signature"),          // v1's signature on v2's manifest
            ("bad/wrong-key", "untrusted_key"),            // signed by a key no build pins
            ("bad/wrong-key-claims-active", "bad_signature"), // another key, the active key's id
        ];
        for (folder, reason) in cases {
            let mut feed = FakeFeed::default();
            feed.serve_release(&config, &fixture(folder));
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.status, CheckStatus::Unreachable, "{folder}");
            assert_eq!(result.reason, reason, "{folder}");
            assert!(result.manifest.is_none(), "{folder}");
            assert!(!result.detail.is_empty(), "{folder}");
        }
    }

    #[test]
    fn a_validly_signed_manifest_with_a_breaking_path_is_malformed_not_an_update() {
        // The fixtures' bad/path-traversal: signed correctly, and lists bin/../../evil.dll.
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve_release(&config, &fixture("bad/path-traversal"));
        let result = run(&feed, &config, &installed_v1());
        assert_eq!(result.status, CheckStatus::Unreachable);
        assert_eq!(result.reason, "malformed");
    }

    #[test]
    fn a_build_with_placeholder_keys_can_neither_verify_nor_be_fooled() {
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve_release(&config, &fixture("v2"));
        let placeholders = Trust {
            placeholder: true,
            ..Trust::default()
        };
        let result = run_with(&feed, &config, &installed_v1(), &placeholders);
        assert_eq!(result.status, CheckStatus::Unreachable);
        assert_eq!(result.reason, "placeholder_keys");
    }

    #[test]
    fn a_trust_built_wrongly_is_could_not_check_and_never_forged() {
        // A pinned key that is no public key (here 32 bytes of 0xFF, past the field prime, so not a
        // canonical point) cannot say anything about any signature: Internal, which check()
        // reports as "verify_unavailable" and not as a bad signature.
        let broken: PublicKey = [0xFF; 32];
        let trust = Trust {
            keys: vec![TrustedKey {
                id: key_id_of(&broken),
                key: broken,
            }],
            placeholder: false,
        };
        let line = format!("ed25519 {} {}\n", key_id_of(&broken), base64(&[0u8; 64]));
        let mut feed = FakeFeed::default();
        let config = github_feed();
        feed.serve(signature_url(&config), line);
        feed.serve(manifest_url(&config), fixture("v2").json);
        let result = run_with(&feed, &config, &installed_v1(), &trust);
        assert_eq!(result.status, CheckStatus::Unreachable);
        assert_eq!(result.reason, "verify_unavailable");
    }

    #[test]
    fn a_signed_manifest_for_another_product_target_or_channel_is_not_for_this_install() {
        let config = github_feed();
        let cases = [
            (
                "another product",
                "\"product\":\"katana\"",
                "\"product\":\"other\"",
                "wrong_product",
            ),
            (
                "another target",
                "\"target\":\"win64\"",
                "\"target\":\"win-arm64\"",
                "wrong_target",
            ),
            (
                "another channel",
                "\"channel\":\"stable\"",
                "\"channel\":\"beta\"",
                "wrong_channel",
            ),
        ];
        for (what, from, to, reason) in cases {
            let mut feed = FakeFeed::default();
            feed.serve_release(&config, &changed_v2(from, to));
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.status, CheckStatus::Unreachable, "{what}");
            assert_eq!(result.reason, reason, "{what}");
        }
    }

    // ---- cannot be asked ------------------------------------------------------------------------

    #[test]
    fn a_development_tree_is_never_asked_about() {
        let feed = FakeFeed::default();
        let install = Install {
            kind: InstallKind::Development,
            target: String::new(),
            ..installed_v1()
        };
        let result = run(&feed, &github_feed(), &install);
        assert_eq!(result.status, CheckStatus::Unsupported);
        assert_eq!(result.reason, "development_tree");
        assert!(!result.detail.is_empty());
        assert!(feed.requests().is_empty(), "no network is touched");
    }

    #[test]
    fn a_build_with_no_feed_is_unsupported_and_asks_nothing() {
        let mut feed = FakeFeed::default();
        let mut config = github_feed();
        config.repo.clear();
        let result = run(&feed, &config, &installed_v1());
        assert_eq!(result.status, CheckStatus::Unsupported);
        assert_eq!(result.reason, "disabled_build");
        assert!(feed.requests().is_empty());

        // The owner's own host instead of GitHub is a feed.
        config.feed_base = String::from("https://updates.example.org/katana");
        feed.serve_release(&config, &fixture("v2"));
        assert_eq!(
            run(&feed, &config, &installed_v1()).status,
            CheckStatus::Available
        );
        let requests = feed.requests();
        assert!(!requests.is_empty());
        assert_eq!(
            requests[0].url,
            "https://updates.example.org/katana/Katana-update-win64.json.sig"
        );
    }

    #[test]
    fn a_call_that_cannot_be_made_is_an_error_of_the_call() {
        let feed = FakeFeed::default();
        let fetch = feed.fetch();
        let install = installed_v1();

        let stopped = AtomicBool::new(true);
        let cancelled = check(
            &fetch,
            &github_feed(),
            &install,
            test_trust(),
            Some(&stopped),
        )
        .expect_err("a stop already requested");
        assert_eq!(cancelled.code, ErrorCode::InvalidState);

        let no_target = FeedConfig {
            target: String::new(),
            ..github_feed()
        };
        assert_eq!(
            check(&fetch, &no_target, &install, test_trust(), None)
                .expect_err("no target")
                .code,
            ErrorCode::InvalidArgument
        );

        let beta = FeedConfig {
            channel: String::from("beta"),
            ..github_feed()
        };
        assert_eq!(
            check(&fetch, &beta, &install, test_trust(), None)
                .expect_err("the beta channel")
                .code,
            ErrorCode::InvalidArgument
        );

        let other_target = FeedConfig {
            target: String::from("linux-x86_64"),
            ..github_feed()
        };
        assert_eq!(
            check(&fetch, &other_target, &install, test_trust(), None)
                .expect_err("the feed's target is the install's")
                .code,
            ErrorCode::InvalidArgument
        );

        let no_transport: Fetch<'_> = None;
        assert_eq!(
            check(&no_transport, &github_feed(), &install, test_trust(), None)
                .expect_err("no transport")
                .code,
            ErrorCode::InvalidArgument
        );
        assert!(feed.requests().is_empty());
    }
}

// ---- the private-repository route -----------------------------------------------------------------

const RELEASE_JSON: &str = r#"{
  "tag_name": "v0.10.0",
  "assets": [
    {"id": 11, "name": "Katana-update-win64.json.sig", "size": 114},
    {"id": 12, "name": "Katana-update-win64.json", "size": 2000},
    {"id": 13, "name": "Katana-0.10.0-win64.update.zip", "size": 3791}
  ]
}"#;

fn private_feed() -> FeedConfig {
    FeedConfig {
        token: String::from(SECRET_TOKEN),
        ..github_feed()
    }
}

// A repository that answers an anonymous read with 404 and a token holder through the API.
fn serve_behind_token(feed: &mut FakeFeed, config: &FeedConfig, release: &Release) {
    feed.serve(release_lookup_url(config), RELEASE_JSON);
    feed.serve(asset_api_url(config, 11), release.sig.clone());
    feed.serve(asset_api_url(config, 12), release.json.clone());
}

mod check_private_route {
    use super::*;

    #[test]
    fn a404_on_the_signature_with_a_token_retries_once_through_the_api_and_says_so() {
        let mut feed = FakeFeed::default();
        let config = private_feed();
        serve_behind_token(&mut feed, &config, &fixture("v2"));
        let result = run(&feed, &config, &installed_v1());

        assert_eq!(result.status, CheckStatus::Available);
        assert_eq!(result.route, FeedRoute::Api);
        let requests = feed.requests();
        assert_eq!(requests.len(), 4);
        // The anonymous read first; then the lookup and the two assets by their ids.
        assert_eq!(
            requests[0].url,
            "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json.sig"
        );
        assert_eq!(
            requests[1].url,
            "https://api.github.com/repos/kjarada/Katana/releases/latest"
        );
        assert_eq!(
            requests[2].url,
            "https://api.github.com/repos/kjarada/Katana/releases/assets/11"
        );
        assert_eq!(
            requests[3].url,
            "https://api.github.com/repos/kjarada/Katana/releases/assets/12"
        );
        assert_eq!(requests[1].accept, "application/vnd.github+json");
        assert_eq!(requests[2].accept, "application/octet-stream");
        assert_eq!(requests[3].accept, "application/octet-stream");
        assert_eq!(requests[1].max_bytes, MAX_RELEASE_JSON_BYTES);
        assert_eq!(requests[2].max_bytes, MAX_SIGNATURE_BYTES);
        assert_eq!(requests[3].max_bytes, MAX_MANIFEST_BYTES);
        // The token: on every Api request, on no Download one.
        assert!(requests[0].bearer_token.is_empty());
        for request in &requests[1..] {
            assert_eq!(request.bearer_token, SECRET_TOKEN, "{}", request.url);
        }
    }

    #[test]
    fn a_token_is_not_used_when_the_public_route_works() {
        let mut feed = FakeFeed::default();
        let config = private_feed();
        feed.serve_release(&config, &fixture("v2"));
        let result = run(&feed, &config, &installed_v1());
        assert_eq!(result.status, CheckStatus::Available);
        assert_eq!(result.route, FeedRoute::Download);
        let requests = feed.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert!(request.bearer_token.is_empty());
        }
    }

    #[test]
    fn no_retry_without_a_token_or_with_a_feed_base_or_after_any_other_failure() {
        {
            let mut feed = FakeFeed::default();
            let config = github_feed(); // no token
            serve_behind_token(&mut feed, &config, &fixture("v2"));
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.status, CheckStatus::Unreachable);
            assert_eq!(result.reason, "http_404");
            assert_eq!(feed.requests().len(), 1);
            assert_eq!(result.route, FeedRoute::Download);
        }
        {
            let feed = FakeFeed::default();
            // The owner's host: the API is GitHub's.
            let config = FeedConfig {
                feed_base: String::from("https://updates.example.org/katana"),
                ..private_feed()
            };
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.reason, "http_404");
            assert_eq!(feed.requests().len(), 1);
        }
        for number in [403, 429, 500] {
            let mut feed = FakeFeed::default();
            let config = private_feed();
            serve_behind_token(&mut feed, &config, &fixture("v2"));
            feed.fail(
                signature_url(&config),
                status(ErrorCode::FileImportFailure, number),
            );
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.reason, format!("http_{number}"));
            assert_eq!(
                feed.requests().len(),
                1,
                "only a 404 sends a token holder to the API"
            );
        }
        {
            // The signature found, the manifest not (a release being published): not a private
            // repository.
            let mut feed = FakeFeed::default();
            let config = private_feed();
            serve_behind_token(&mut feed, &config, &fixture("v2"));
            feed.serve(signature_url(&config), fixture("v2").sig);
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.reason, "http_404");
            assert_eq!(feed.requests().len(), 2);
        }
    }

    #[test]
    fn the_private_route_fails_with_the_reason_of_what_went_wrong_there() {
        let config = private_feed();
        {
            let feed = FakeFeed::default(); // the token sees nothing either: the lookup is a 404 too
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.status, CheckStatus::Unreachable);
            assert_eq!(result.reason, "http_404");
            assert_eq!(result.route, FeedRoute::Api);
            assert_eq!(feed.requests().len(), 2);
        }
        {
            let mut feed = FakeFeed::default(); // a bad token
            feed.fail(
                release_lookup_url(&config),
                status(ErrorCode::FileImportFailure, 401),
            );
            assert_eq!(run(&feed, &config, &installed_v1()).reason, "http_401");
        }
        {
            let mut feed = FakeFeed::default(); // the release has no manifest asset
            feed.serve(
                release_lookup_url(&config),
                r#"{"assets":[{"id":11,"name":"Katana-update-win64.json.sig"}]}"#,
            );
            let result = run(&feed, &config, &installed_v1());
            assert_eq!(result.reason, "asset_missing");
            assert_eq!(feed.requests().len(), 2);
        }
        {
            let mut feed = FakeFeed::default(); // the release has no signature asset
            feed.serve(
                release_lookup_url(&config),
                r#"{"assets":[{"id":12,"name":"Katana-update-win64.json"}]}"#,
            );
            assert_eq!(run(&feed, &config, &installed_v1()).reason, "asset_missing");
        }
        {
            let mut feed = FakeFeed::default(); // not a release at all
            feed.serve(release_lookup_url(&config), "[1,2,3]");
            assert_eq!(run(&feed, &config, &installed_v1()).reason, "malformed");
        }
        {
            let mut feed = FakeFeed::default(); // a sign-in page in place of the release
            feed.serve(release_lookup_url(&config), "<html>sign in</html>");
            assert_eq!(run(&feed, &config, &installed_v1()).reason, "malformed");
        }
        {
            let mut feed = FakeFeed::default(); // an asset fetched by id that is not what was signed
            serve_behind_token(&mut feed, &config, &fixture("bad/flipped-manifest-bit"));
            assert_eq!(
                run(&feed, &config, &installed_v1()).reason,
                "bad_signature",
                "the bytes fetched by id are verified like any others"
            );
        }
        {
            let mut feed = FakeFeed::default(); // and a page in place of an asset
            serve_behind_token(&mut feed, &config, &fixture("v2"));
            feed.serve(asset_api_url(&config, 12), "<!DOCTYPE html><html></html>");
            assert_eq!(run(&feed, &config, &installed_v1()).reason, "html_page");
        }
    }

    #[test]
    fn the_token_is_in_no_result_whatever_went_wrong() {
        let config = private_feed();
        let mut feeds: Vec<FakeFeed> = (0..4).map(|_| FakeFeed::default()).collect();
        serve_behind_token(&mut feeds[0], &config, &fixture("v2"));
        serve_behind_token(&mut feeds[1], &config, &fixture("bad/wrong-key"));
        feeds[2].fail(
            release_lookup_url(&config),
            status(ErrorCode::FileImportFailure, 401),
        );
        feeds[3].serve(release_lookup_url(&config), "not json");
        for feed in &feeds {
            let result = run(feed, &config, &installed_v1());
            for text in [
                &result.reason,
                &result.detail,
                &result.notes_url,
                &result.latest_version,
                &result.installed_version,
            ] {
                assert!(!text.contains(SECRET_TOKEN), "{text}");
            }
        }
    }
}

// ---- what the person has already decided -----------------------------------------------------------

fn available_v2() -> CheckResult {
    let mut feed = FakeFeed::default();
    let config = github_feed();
    feed.serve_release(&config, &fixture("v2"));
    let result = run(&feed, &config, &installed_v1());
    assert_eq!(result.status, CheckStatus::Available);
    result
}

mod check_state {
    use super::*;

    #[test]
    fn a_skipped_version_is_current_and_a_blocked_sequence_is_blocked() {
        {
            let mut result = available_v2();
            let state = UpdateState {
                skipped_version: String::from("0.10.0"),
                ..UpdateState::default()
            };
            apply_update_state(&mut result, &state);
            assert_eq!(result.status, CheckStatus::Current);
            assert_eq!(result.reason, "skipped");
            assert!(result.manifest.is_some(), "what was found is kept");
            assert_eq!(result.latest_version, "0.10.0");
        }
        {
            // Compared as versions: "0.10" is "0.10.0".
            let mut result = available_v2();
            let state = UpdateState {
                skipped_version: String::from("0.10"),
                ..UpdateState::default()
            };
            apply_update_state(&mut result, &state);
            assert_eq!(result.status, CheckStatus::Current);
        }
        {
            let mut result = available_v2();
            let state = UpdateState {
                blocked_sequences: vec![1, 1792312800, 5],
                ..UpdateState::default()
            };
            apply_update_state(&mut result, &state);
            assert_eq!(result.status, CheckStatus::Blocked);
            assert_eq!(result.reason, "blocked_before");
        }
        {
            // Blocked is the stronger: it is not offered again whatever else was said.
            let mut result = available_v2();
            let state = UpdateState {
                blocked_sequences: vec![1792312800],
                skipped_version: String::from("0.10.0"),
                ..UpdateState::default()
            };
            apply_update_state(&mut result, &state);
            assert_eq!(result.status, CheckStatus::Blocked);
        }
    }

    #[test]
    fn another_version_another_sequence_and_an_answer_that_is_not_available_are_left_alone() {
        for skipped in ["0.9.0", "0.11.0", "", "banana", "0.10.0.1"] {
            let mut result = available_v2();
            let state = UpdateState {
                skipped_version: String::from(skipped),
                blocked_sequences: vec![1792312799, 1792312801],
                ..UpdateState::default()
            };
            apply_update_state(&mut result, &state);
            assert_eq!(result.status, CheckStatus::Available, "skipped '{skipped}'");
            assert!(result.reason.is_empty());
        }
        let mut unreachable = CheckResult {
            status: CheckStatus::Unreachable,
            reason: String::from("offline"),
            ..CheckResult::default()
        };
        let everything = UpdateState {
            skipped_version: String::from("0.10.0"),
            blocked_sequences: vec![1792312800],
            ..UpdateState::default()
        };
        apply_update_state(&mut unreachable, &everything);
        assert_eq!(unreachable.status, CheckStatus::Unreachable);
        assert_eq!(unreachable.reason, "offline");
    }

    #[test]
    fn each_status_has_its_word() {
        assert_eq!(status_word(CheckStatus::Available), "available");
        assert_eq!(status_word(CheckStatus::Current), "current");
        assert_eq!(status_word(CheckStatus::Unreachable), "unreachable");
        assert_eq!(status_word(CheckStatus::Unsupported), "unsupported");
        assert_eq!(status_word(CheckStatus::Blocked), "blocked");
    }
}
