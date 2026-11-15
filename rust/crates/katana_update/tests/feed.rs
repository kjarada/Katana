//! The feed (the twin of tests/update/test_feed.cpp): the configure-time defaults, the reader of a
//! release's asset list, and the transport's rules: the one place the access token is held to
//! "exactly https://api.github.com, on no redirect hop", the redirect limits, the portal and range
//! answers, the time a request is given, and the guarantee that no error carries the token or the
//! query of an address, whoever wrote its words.
//!
//! The addresses themselves are tested in the twin of test_contract.cpp (UpdateFeedAddresses);
//! this file does not repeat them.
//!
//! Where the expectations come from (CLAUDE.md, section 4): every address, header and status here
//! is written out by hand from RFC 3986 (the authority ends at the first '/', '?' or '#'; '@'
//! begins a host after user-info), RFC 9110 (Location, Range, Content-Range, the redirect
//! statuses) and GitHub's REST documentation (the release object's assets[]), never read back from
//! the code under test. The transport runs over a fake that records each hop it is given, so no
//! network is touched.

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_update::feed::{FeedRoute, ProgressFn, StopToken};
use katana_update::feed::{
    Fetch, FetchRequest, HOP_TIMEOUT_SECONDS, HopFn, HttpHop, HttpHopReply,
    MAX_HOP_TIMEOUT_SECONDS, MAX_RELEASE_JSON_BYTES, RANGE_HOP_TIMEOUT_SECONDS,
    SLOW_LINE_BYTES_PER_SECOND, default_feed_config, display_url, fetch_failure_reason, hop_fn,
    hop_headers, is_api_github_url, is_fetchable_url, is_loopback_http_url, make_fetch,
    parse_release_assets, range_hop_timeout_seconds, resolve_redirect, without_queries,
};
use katana_update::ranges::{PARTIAL_DOWNLOADS_REFUSED, RANGE_MAX_LENGTH};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;

const SECRET_TOKEN: &str = "ghp_SECRET-TOKEN-0123456789";

// The version the project declares, read here as the C++ test is handed it (KATANA_EXPECTED_VERSION
// from project(Katana VERSION ...)), and not from the library under test.
fn expected_version() -> String {
    let cmake = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../CMakeLists.txt"
    ))
    .expect("the checkout's CMakeLists.txt");
    let start = cmake.find("project(Katana").expect("project(Katana ...)");
    let arguments = &cmake[start..start + cmake[start..].find(')').expect("a closing bracket")];
    let mut words = arguments.split_ascii_whitespace();
    while let Some(word) = words.next() {
        if word == "VERSION" {
            return words.next().expect("a version").to_owned();
        }
    }
    panic!("project(Katana ...) names no VERSION");
}

fn call(fetch: &Fetch<'_>, request: &FetchRequest) -> Result<Vec<u8>> {
    fetch.as_ref().expect("a transport")(request, None, None)
}

fn call_stopped(fetch: &Fetch<'_>, request: &FetchRequest, stop: StopToken<'_>) -> Result<Vec<u8>> {
    fetch.as_ref().expect("a transport")(request, stop, None)
}

fn call_with_progress(
    fetch: &Fetch<'_>,
    request: &FetchRequest,
    progress: ProgressFn<'_>,
) -> Result<Vec<u8>> {
    fetch.as_ref().expect("a transport")(request, None, progress)
}

fn describe(result: &Result<Vec<u8>>) -> String {
    match result {
        Ok(_) => String::from("accepted"),
        Err(error) => error.describe(),
    }
}

// ---- the defaults ------------------------------------------------------------------------------

mod feed_defaults {
    use super::*;

    #[test]
    fn the_default_feed_is_the_owners_repository_on_github_with_the_user_agent_and_no_token() {
        // Decision D0 (docs/update.md): kjarada/Katana, no feed of the owner's own, no token
        // compiled in. A configure that changes KATANA_UPDATE_REPO or KATANA_UPDATE_FEED changes
        // this on purpose, and so changes this test.
        let feed = default_feed_config("win64");
        assert_eq!(feed.repo, "kjarada/Katana");
        assert!(feed.feed_base.is_empty());
        assert_eq!(feed.channel, "stable");
        assert_eq!(feed.target, "win64");
        assert!(feed.token.is_empty(), "a token is never compiled in");
        assert_eq!(feed.route, FeedRoute::Download);
        // "Katana/<ver> (<target>)": the only thing a check says about this copy.
        assert_eq!(
            feed.user_agent,
            format!("Katana/{} (win64)", expected_version())
        );
    }

    #[test]
    fn the_user_agent_follows_the_target() {
        assert_eq!(
            default_feed_config("linux-aarch64").user_agent,
            format!("Katana/{} (linux-aarch64)", expected_version())
        );
        assert_eq!(default_feed_config("linux-aarch64").target, "linux-aarch64");
    }
}

// ---- the asset list of a release ---------------------------------------------------------------

// The shape GitHub documents for a release (REST, "Get the latest release"), trimmed to what
// matters and with the members this reader must ignore left in.
const RELEASE: &str = r#"{
  "url": "https://api.github.com/repos/kjarada/Katana/releases/1001",
  "tag_name": "v0.10.0",
  "draft": false,
  "author": {"login": "someone", "id": 4},
  "assets": [
    {"url": "https://api.github.com/repos/kjarada/Katana/releases/assets/7654321",
     "id": 7654321, "name": "Katana-update-win64.json", "size": 2000, "content_type": "application/json",
     "uploader": {"login": "someone", "id": 4}},
    {"id": 7654322, "name": "Katana-update-win64.json.sig", "size": 114},
    {"id": 18446744073709551615, "name": "Katana-0.10.0-win64.update.zip", "size": 4294967296}
  ]
}"#;

mod release_assets {
    use super::*;

    #[test]
    fn assets_are_read_with_their_names_ids_and_sizes_in_order() {
        let assets =
            parse_release_assets(RELEASE).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(assets.len(), 3);
        assert_eq!(assets[0].name, "Katana-update-win64.json");
        assert_eq!(assets[0].id, 7_654_321);
        assert_eq!(assets[0].size, 2000);
        assert_eq!(assets[1].name, "Katana-update-win64.json.sig");
        assert_eq!(assets[1].id, 7_654_322);
        // Past 2^53 and past 2^32: exact, not a double.
        assert_eq!(assets[2].id, 18_446_744_073_709_551_615);
        assert_eq!(assets[2].size, 4_294_967_296);
    }

    #[test]
    fn a_release_without_assets_or_with_an_empty_list_has_none() {
        for json in [r#"{"tag_name":"v1.0.0"}"#, r#"{"assets":[]}"#] {
            let assets =
                parse_release_assets(json).unwrap_or_else(|error| panic!("{}", error.describe()));
            assert!(assets.is_empty());
        }
    }

    #[test]
    fn an_asset_without_a_size_has_size_zero() {
        let assets = parse_release_assets(r#"{"assets":[{"id":5,"name":"a"}]}"#).expect("it reads");
        assert_eq!(assets[0].size, 0);
    }

    #[test]
    fn what_is_not_a_release_with_well_formed_assets_is_a_parse_failure() {
        let broken = [
            "",                                                 // nothing
            "[]",                                               // not an object
            "\"assets\"",                                       // not an object
            "42",                                               // not an object
            "{",                                                // truncated
            r#"{"assets":{"id":1,"name":"a"}}"#,                // assets is not a list
            r#"{"assets":[7]}"#,                                // an asset is not an object
            r#"{"assets":[{"id":1}]}"#,                         // no name
            r#"{"assets":[{"name":"a"}]}"#,                     // no id
            r#"{"assets":[{"id":1,"name":""}]}"#,               // an empty name
            r#"{"assets":[{"id":1,"name":7}]}"#,                // a name that is not text
            r#"{"assets":[{"id":"1","name":"a"}]}"#,            // an id written as text
            r#"{"assets":[{"id":1.5,"name":"a"}]}"#,            // an id that is not whole
            r#"{"assets":[{"id":-1,"name":"a"}]}"#,             // a negative id
            r#"{"assets":[{"id":0,"name":"a"}]}"#,              // no asset has id 0
            r#"{"assets":[{"id":1e3,"name":"a"}]}"#,            // a float, however whole
            r#"{"assets":[{"id":1,"name":"a","size":"big"}]}"#, // a size that is not a number
            r#"{"assets":[{"id":1,"name":"a","size":-4}]}"#,    // a negative size
        ];
        for json in broken {
            let error = parse_release_assets(json).expect_err(&format!("accepted: {json}"));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{json}");
        }
    }

    #[test]
    fn a_nul_byte_nesting_too_deep_and_an_oversize_document_are_refused() {
        // The reader would take the NUL for the end of the text and ignore what follows.
        let with_nul = format!(
            "{}\0{}",
            r#"{"assets":[]}"#, r#"{"assets":[{"id":1,"name":"x"}]}"#
        );
        assert!(parse_release_assets(&with_nul).is_err());

        // 17 levels, the first past the reader's 16.
        let deep = format!("{}1{}", "{\"a\":".repeat(17), "}".repeat(17));
        assert!(parse_release_assets(&deep).is_err());
        // 16 levels, the last that is read: the release object is level 1.
        let allowed = format!("{}1{}", "{\"a\":".repeat(16), "}".repeat(16));
        assert!(parse_release_assets(&allowed).is_ok());

        // One byte past the bound the transport reads a release lookup with.
        let huge = " ".repeat(MAX_RELEASE_JSON_BYTES as usize + 1);
        assert!(parse_release_assets(&huge).is_err());
    }

    #[test]
    fn more_assets_than_a_release_can_have_are_refused() {
        // GitHub allows 1000; the reader allows 2000 and refuses 2001.
        let list = |count: usize| {
            let assets: Vec<String> = (1..=count)
                .map(|i| format!("{{\"id\":{i},\"name\":\"a{i}\"}}"))
                .collect();
            format!("{{\"assets\":[{}]}}", assets.join(","))
        };
        assert!(parse_release_assets(list(2000)).is_ok());
        assert!(parse_release_assets(list(2001)).is_err());
    }
}

// ---- the token's one destination ---------------------------------------------------------------

mod api_host {
    use super::*;

    #[test]
    fn only_https_api_github_com_is_the_api_host() {
        assert!(is_api_github_url(
            "https://api.github.com/repos/o/r/releases/latest"
        ));
        assert!(is_api_github_url("https://api.github.com"));
        assert!(is_api_github_url("https://api.github.com?x=1"));
        assert!(
            is_api_github_url("HTTPS://API.GITHUB.COM/repos"),
            "scheme and host names ignore case"
        );

        // The look-alikes the check exists for: RFC 3986's authority runs to the first '/', '?'
        // or '#', and a '@' puts the real host after it.
        assert!(
            !is_api_github_url("http://api.github.com/repos"),
            "never in the clear"
        );
        for url in [
            "https://api.github.com@evil.example/repos",
            "https://api.github.com:443/repos",
            "https://api.github.com:8443/repos",
            "https://api.github.com./repos",
            "https://api.github.com.evil.example/repos",
            "https://evil-api.github.com/repos",
            "https://github.com/api.github.com",
            "https://evil.example/https://api.github.com/",
            "https://evil.example?x=https://api.github.com/",
            "https://evil.example#@api.github.com/",
            "https://release-assets.githubusercontent.com/x",
            "https://github.com/kjarada/Katana/releases/latest/download/x",
            "https://api.github.com\\@evil.example/",
            "https://api.github.com /repos",
            "https://api.github.com\r\nHost: evil/",
            "ftp://api.github.com/",
            "api.github.com/repos",
            "",
        ] {
            assert!(!is_api_github_url(url), "{url}");
        }
    }
}

mod fetchable_addresses {
    use super::*;

    #[test]
    fn https_always_and_http_only_on_the_loopback_when_allowed() {
        assert!(is_fetchable_url(
            "https://github.com/o/r/releases/latest/download/x",
            false
        ));
        assert!(is_fetchable_url(
            "https://updates.example.org:8443/dir/x",
            false
        ));
        assert!(!is_fetchable_url("http://github.com/x", false));
        assert!(
            !is_fetchable_url("http://github.com/x", true),
            "allowed only on the loopback"
        );

        assert!(is_fetchable_url("http://127.0.0.1:8080/x", true));
        assert!(is_fetchable_url("http://localhost/x", true));
        assert!(is_fetchable_url("http://[::1]:9/x", true));
        assert!(!is_fetchable_url("http://127.0.0.1:8080/x", false));
        assert!(!is_fetchable_url("http://127.0.0.1.evil.example/x", true));
        assert!(!is_fetchable_url("http://localhost@evil.example/x", true));
        assert!(
            !is_fetchable_url("http://127.0.0.1:80a/x", true),
            "a port is digits"
        );

        assert!(!is_fetchable_url("ftp://github.com/x", true));
        assert!(!is_fetchable_url("file:///etc/passwd", true));
        assert!(!is_fetchable_url("https://", true));
        assert!(!is_fetchable_url("https:///path", true), "no host");
        assert!(
            !is_fetchable_url("https://user:pass@github.com/x", true),
            "no user-info"
        );
        assert!(!is_fetchable_url("https://github.com/a b", true));
        assert!(!is_fetchable_url("https://github.com/a\\b", true));
        assert!(!is_fetchable_url("https://github.com/a\nb", true));
        assert!(
            !is_fetchable_url("https://github.com/\u{e9}", true),
            "an address is ASCII"
        );
        assert!(!is_fetchable_url("", true));

        assert!(is_loopback_http_url("http://127.0.0.1:1/"));
        assert!(
            !is_loopback_http_url("https://127.0.0.1:1/"),
            "the exception is for plain http"
        );
    }
}

// ---- redirects ---------------------------------------------------------------------------------

mod redirects {
    use super::*;

    #[test]
    fn a_location_is_resolved_against_the_address_that_answered_with_it() {
        let from = "https://api.github.com/repos/o/r/releases/assets/9?x=1";
        let resolve = |location: &str| resolve_redirect(from, location).expect(location);
        // Absolute: kept as the server wrote it, query and all (a signed address needs its query).
        assert_eq!(
            resolve("https://release-assets.githubusercontent.com/a/b?sig=Zm9v&se=1"),
            "https://release-assets.githubusercontent.com/a/b?sig=Zm9v&se=1"
        );
        assert_eq!(
            resolve("HTTPS://Example.org/P"),
            "https://Example.org/P",
            "the scheme is lower-cased, the rest is left alone"
        );
        // Root-relative and scheme-relative (RFC 3986, 4.2).
        assert_eq!(
            resolve("/repositories/42/releases/assets/9"),
            "https://api.github.com/repositories/42/releases/assets/9"
        );
        assert_eq!(
            resolve("//objects.example.org/blob?x=1"),
            "https://objects.example.org/blob?x=1"
        );
        // A fragment never goes to a server.
        assert_eq!(
            resolve("https://example.org/p#frag"),
            "https://example.org/p"
        );
        assert_eq!(resolve("/p?q=1#frag"), "https://api.github.com/p?q=1");
        // A plain-http address is resolved (is_fetchable_url then judges it).
        assert_eq!(resolve("http://example.org/p"), "http://example.org/p");
        assert_eq!(
            resolve_redirect("http://127.0.0.1:5/a", "/b").expect("resolved"),
            "http://127.0.0.1:5/b"
        );
    }

    #[test]
    fn a_location_that_is_not_an_address_this_program_follows_is_refused_with_a_reason() {
        let from = "https://api.github.com/x";
        for bad in [
            "",                    // none
            "#only-a-fragment",    // nothing to fetch
            "relative/path",       // relative to the current path: not followed
            "../up",               // likewise
            "?q=1",                // likewise
            "ftp://example.org/x", // another scheme
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:pw@example.org/x",        // user-info
            "https://api.github.com@evil.example/", // the look-alike
            "https:///no-host",
            "https://example.org:port/x",
            "https://example.org/a b",       // a space
            "https://example.org/a\\b",      // a backslash
            "https://example.org/a\r\nX: y", // a line break
            "https://example.org/\u{e9}",    // raw non-ASCII
        ] {
            let error = resolve_redirect(from, bad).expect_err(&format!("followed: {bad}"));
            assert!(error.context.contains("reason=bad_redirect"), "{bad}");
        }
    }
}

// ---- what a hop sends --------------------------------------------------------------------------

fn ranged_request() -> FetchRequest {
    FetchRequest {
        url: String::from("https://api.github.com/repos/o/r/releases/assets/9"),
        range_start: 100,
        range_length: 50,
        payload_size: 1000,
        max_bytes: 50,
        accept: String::from("application/octet-stream"),
        bearer_token: String::from(SECRET_TOKEN),
        user_agent: String::from("Katana/9.9.9 (win64)"),
    }
}

fn has_header(headers: &[String], line: &str) -> bool {
    headers.iter().any(|header| header == line)
}

fn has_header_named(headers: &[String], name: &str) -> bool {
    headers.iter().any(|header| {
        header.len() > name.len()
            && header.starts_with(name)
            && header.as_bytes()[name.len()] == b':'
    })
}

mod hop_headers {
    use super::*;

    #[test]
    fn the_token_is_on_the_api_host_and_on_no_other_address() {
        let request = ranged_request();
        let bearer = format!("Authorization: Bearer {SECRET_TOKEN}");

        let api = hop_headers(
            &request,
            "https://api.github.com/repos/o/r/releases/assets/9",
            false,
        );
        assert!(has_header(&api, &bearer));
        assert!(has_header(&api, "Accept: application/octet-stream"));
        assert!(
            has_header(&api, "Range: bytes=100-149"),
            "inclusive last byte, RFC 9110 14.1.2"
        );
        assert!(has_header(&api, "X-GitHub-Api-Version: 2022-11-28"));

        for url in [
            "https://release-assets.githubusercontent.com/a/b?sig=Zm9v",
            "https://github.com/o/r/releases/download/v1/x.zip",
            "https://api.github.com.evil.example/x",
            "https://api.github.com@evil.example/x",
            "http://api.github.com/x",
            "https://api.github.com:8443/x",
            "http://127.0.0.1:8080/x",
        ] {
            // Whether the engine asked for the address itself or a server answered with it.
            for redirected in [false, true] {
                let other = hop_headers(&request, url, redirected);
                assert!(!has_header_named(&other, "Authorization"), "{url}");
                assert!(!has_header_named(&other, "X-GitHub-Api-Version"), "{url}");
                // What the next hop still needs to get the same bytes.
                assert!(
                    has_header(&other, "Accept: application/octet-stream"),
                    "{url}"
                );
                assert!(has_header(&other, "Range: bytes=100-149"), "{url}");
                for header in &other {
                    assert!(!header.contains(SECRET_TOKEN), "{url}: {header}");
                }
            }
        }
    }

    #[test]
    fn an_address_a_server_answered_with_gets_no_token_even_when_it_is_the_api_host() {
        // The owner's ruling (S2) and the contract of FetchRequest::bearer_token: the token goes
        // to exactly https://api.github.com and NEVER on a redirect hop. So the very address that
        // carries it on the first hop carries none when a redirect is what named it.
        let request = ranged_request();
        for url in [
            "https://api.github.com/repos/o/r/releases/assets/9",
            "https://api.github.com/repositories/42/releases/assets/9",
            "https://api.github.com/user",
            "https://api.github.com",
        ] {
            let redirected = hop_headers(&request, url, true);
            assert!(!has_header_named(&redirected, "Authorization"), "{url}");
            for header in &redirected {
                assert!(!header.contains(SECRET_TOKEN), "{url}: {header}");
            }
            // The rest of what a hop to the API is sent says nothing secret, and stays.
            assert!(
                has_header(&redirected, "X-GitHub-Api-Version: 2022-11-28"),
                "{url}"
            );
            assert!(
                has_header(&redirected, "Accept: application/octet-stream"),
                "{url}"
            );
            assert!(has_header(&redirected, "Range: bytes=100-149"), "{url}");
        }
    }

    #[test]
    fn no_token_means_no_authorization_even_on_the_api_host() {
        let request = FetchRequest {
            bearer_token: String::new(),
            ..ranged_request()
        };
        let api = hop_headers(&request, "https://api.github.com/x", false);
        assert!(!has_header_named(&api, "Authorization"));
    }

    #[test]
    fn a_whole_body_request_has_no_range_and_an_empty_accept_is_omitted() {
        let request = FetchRequest {
            url: String::from("https://github.com/x"),
            max_bytes: 10,
            ..FetchRequest::default()
        };
        let headers = hop_headers(&request, &request.url, false);
        assert!(headers.is_empty());
    }
}

mod display_url {
    use super::*;

    #[test]
    fn drops_the_query_the_fragment_and_the_user_info() {
        assert_eq!(
            display_url("https://release-assets.githubusercontent.com/a/b?sig=SECRET&se=1#f"),
            "https://release-assets.githubusercontent.com/a/b"
        );
        assert_eq!(
            display_url("https://user:pw@example.org/p?x"),
            "https://example.org/p"
        );
        assert_eq!(display_url("HTTPS://Example.org"), "https://Example.org");
        assert_eq!(display_url("not an address"), "(not an address)");
        let long_path = format!("https://example.org/{}", "a".repeat(500));
        assert!(
            display_url(&long_path).len() < 220,
            "bounded: it is for a person to read"
        );
    }
}

mod without_queries {
    use super::*;

    #[test]
    fn every_address_in_a_text_loses_its_query_and_its_fragment() {
        // Written out by hand from the rule in feed.rs: from "://" an address runs to the next
        // blank, control character or byte outside ASCII, and is cut at its first '?' or '#'.
        assert_eq!(without_queries("https://h/p?sp=r&sig=S"), "https://h/p");
        assert_eq!(
            without_queries("no answer from https://h/p?sig=S&se=1: timed out"),
            "no answer from https://h/p timed out"
        );
        assert_eq!(without_queries("https://h/p#frag end"), "https://h/p end");
        assert_eq!(
            without_queries("a https://x/y b http://z/w?k=v c"),
            "a https://x/y b http://z/w c"
        );
        assert_eq!(
            without_queries("https://h/p?a=1\nhttps://g/q?b=2"),
            "https://h/p\nhttps://g/q"
        );
        // What a list of secret parameter names does not know is cut all the same, and so is a
        // query some of whose values were already hidden.
        assert_eq!(
            without_queries("https://s3.example/b/k?X-Amz-Signature=abcdef&X-Amz-Expires=300"),
            "https://s3.example/b/k"
        );
        assert_eq!(
            without_queries("https://h/p?sp=r&sig=***&se=2026 (HTTP 0): Could not resolve host"),
            "https://h/p (HTTP 0): Could not resolve host"
        );
        // An address inside a query goes with the query; a quote or a semicolon inside a query
        // does not end the address (either may stand in one), so nothing after it survives.
        assert_eq!(
            without_queries("https://a/p?next=https://b/q?sig=S tail"),
            "https://a/p tail"
        );
        assert_eq!(
            without_queries("'https://h/p?name='x';sig=S' refused"),
            "'https://h/p refused"
        );
    }

    #[test]
    fn a_text_without_an_address_or_without_a_query_is_left_as_it_is() {
        for text in [
            "",
            "the server did not answer within 60 s",
            "a web page instead of the file (a sign-in page?)", // a '?' that is in no address
            "status=403; https://github.com/o/r/releases/latest/download/x; detail",
            "status=206; reason=bad_range; https://github.com/x.zip; Content-Range 'bytes 1-2/3'",
            "://",
            "a://",
        ] {
            assert_eq!(without_queries(text), text);
        }
    }
}

// ---- the time a request is given ---------------------------------------------------------------

mod timeouts {
    use super::*;

    #[test]
    fn a_range_is_given_the_time_its_length_takes_on_a_slow_line() {
        // feed.rs: 60 s to get an answer started, plus the length at 16 KiB a second rounded up;
        // never less than 600 s and never more than four hours. Each figure is worked by hand.
        assert_eq!(HOP_TIMEOUT_SECONDS, 60);
        assert_eq!(RANGE_HOP_TIMEOUT_SECONDS, 600);
        assert_eq!(SLOW_LINE_BYTES_PER_SECOND, 16384);
        assert_eq!(MAX_HOP_TIMEOUT_SECONDS, 14400);

        assert_eq!(range_hop_timeout_seconds(0), 600);
        assert_eq!(range_hop_timeout_seconds(50), 600);
        // The floor ends where the transfer alone takes 540 s: 540 * 16384 = 8847360 bytes.
        assert_eq!(range_hop_timeout_seconds(8_847_360), 600);
        assert_eq!(
            range_hop_timeout_seconds(8_847_361),
            601,
            "a second that was begun counts"
        );
        // The largest merged range, 32 MiB = 33554432 bytes: 2048 s at 16384 a second, and 60.
        assert_eq!(RANGE_MAX_LENGTH, 33_554_432);
        assert_eq!(range_hop_timeout_seconds(RANGE_MAX_LENGTH), 2108);
        assert_eq!(range_hop_timeout_seconds(RANGE_MAX_LENGTH + 1), 2109);
        // The cap is reached at 14340 s of transfer: 14340 * 16384 = 234946560 bytes; one second
        // less of it (14339 * 16384 = 234930176 bytes) is 14399 s.
        assert_eq!(range_hop_timeout_seconds(234_930_176), 14399);
        assert_eq!(range_hop_timeout_seconds(234_946_560), 14400);
        assert_eq!(
            range_hop_timeout_seconds(2 * 1024 * 1024 * 1024),
            14400,
            "the largest file a manifest may list"
        );
        assert_eq!(
            range_hop_timeout_seconds(u64::MAX),
            14400,
            "and no length makes the sum wrap"
        );
    }
}

// ---- the transport, over a fake that records every hop -----------------------------------------

// What the fake wire does with one hop.
#[derive(Clone, Debug)]
struct Reply {
    error: Option<Error>,
    reply: HttpHopReply,
}

fn answer(
    status: i32,
    body: &[u8],
    content_type: &str,
    location: &str,
    content_range: &str,
) -> Reply {
    Reply {
        error: None,
        reply: HttpHopReply {
            status,
            content_type: content_type.to_owned(),
            location: location.to_owned(),
            content_range: content_range.to_owned(),
            body: body.to_vec(),
            overran: false,
        },
    }
}

fn status(code: i32) -> Reply {
    answer(code, b"", "", "", "")
}

fn redirect(code: i32, location: &str) -> Reply {
    answer(code, b"", "", location, "")
}

fn fail(error: Error) -> Reply {
    Reply {
        error: Some(error),
        reply: HttpHopReply::default(),
    }
}

// A scripted wire: the n-th hop gets the n-th reply. Every hop is kept.
#[derive(Default)]
struct Wire {
    script: RefCell<VecDeque<Reply>>,
    hops: RefCell<Vec<HttpHop>>,
}

impl Wire {
    fn with(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            script: RefCell::new(replies.into_iter().collect()),
            hops: RefCell::new(Vec::new()),
        }
    }

    fn hop(&self) -> HopFn<'_> {
        hop_fn(move |one, _stop, _progress| {
            self.hops.borrow_mut().push(one.clone());
            let next = self.script.borrow_mut().pop_front();
            let Some(next) = next else {
                return Err(Error::new(ErrorCode::Internal, "the script ran out"));
            };
            match next.error {
                Some(error) => Err(error),
                None => Ok(next.reply),
            }
        })
    }

    fn hops(&self) -> Vec<HttpHop> {
        self.hops.borrow().clone()
    }
}

fn api_asset() -> FetchRequest {
    FetchRequest {
        url: String::from("https://api.github.com/repos/o/r/releases/assets/9"),
        accept: String::from("application/octet-stream"),
        bearer_token: String::from(SECRET_TOKEN),
        max_bytes: 1024,
        user_agent: String::from("Katana/9.9.9 (win64)"),
        ..FetchRequest::default()
    }
}

fn whole(url: &str, max_bytes: u64) -> FetchRequest {
    FetchRequest {
        url: url.to_owned(),
        max_bytes,
        ..FetchRequest::default()
    }
}

const STORAGE: &str = "https://release-assets.githubusercontent.com/github-production-release-asset/1/uuid?sp=r&sig=SIGNATURE-IN-QUERY&se=2026-10-20T08%3A00%3A00Z";

// The failures of the Fetch contract.
fn one_request(reply: Reply, mut request: FetchRequest) -> Result<Vec<u8>> {
    if request.url.is_empty() {
        request.url = String::from("https://github.com/o/r/releases/latest/download/x");
        request.max_bytes = 1024;
    }
    let wire = Wire::with([reply]);
    call(&make_fetch(wire.hop()), &request)
}

fn one(reply: Reply) -> Result<Vec<u8>> {
    one_request(reply, FetchRequest::default())
}

// What a hop that abandons a transfer at max_bytes hands back when it can say what the answer was
// (katana_app's can): the status and the headers, no body, and `overran`.
fn overrun(status: i32, content_type: &str, content_range: &str) -> Reply {
    let mut reply = answer(status, b"", content_type, "", content_range);
    reply.reply.overran = true;
    reply
}

mod transport {
    use super::*;

    #[test]
    fn the_token_reaches_the_api_host_on_the_first_hop_and_not_the_storage_host_it_redirects_to() {
        let wire = Wire::with([
            redirect(302, STORAGE),
            answer(200, b"the file bytes", "application/octet-stream", "", ""),
        ]);
        let fetch = make_fetch(wire.hop());

        let body =
            call(&fetch, &api_asset()).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(body, b"the file bytes");

        let hops = wire.hops();
        assert_eq!(hops.len(), 2);
        let bearer = format!("Authorization: Bearer {SECRET_TOKEN}");
        assert_eq!(
            hops[0].url,
            "https://api.github.com/repos/o/r/releases/assets/9"
        );
        assert!(has_header(&hops[0].headers, &bearer));
        assert!(has_header(
            &hops[0].headers,
            "Accept: application/octet-stream"
        ));
        // The signed storage address, followed exactly as written, with no token and no X-GitHub
        // header.
        assert_eq!(hops[1].url, STORAGE);
        assert!(!has_header_named(&hops[1].headers, "Authorization"));
        assert!(!has_header_named(&hops[1].headers, "X-GitHub-Api-Version"));
        assert!(has_header(
            &hops[1].headers,
            "Accept: application/octet-stream"
        ));
        for hop in &hops {
            assert_eq!(hop.user_agent, "Katana/9.9.9 (win64)");
            assert_eq!(hop.max_bytes, 1024);
        }
    }

    #[test]
    fn the_range_is_sent_again_on_every_hop() {
        let wire = Wire::with([
            redirect(302, STORAGE),
            answer(
                206,
                &[b'x'; 50],
                "application/octet-stream",
                "",
                "bytes 100-149/1000",
            ),
        ]);
        let fetch = make_fetch(wire.hop());
        let body = call(&fetch, &ranged_request());
        assert!(body.is_ok(), "{}", describe(&body));
        let hops = wire.hops();
        assert_eq!(hops.len(), 2);
        for hop in &hops {
            assert!(
                has_header(&hop.headers, "Range: bytes=100-149"),
                "{}",
                hop.url
            );
            assert_eq!(
                hop.timeout_seconds, RANGE_HOP_TIMEOUT_SECONDS,
                "a range gets the long timeout"
            );
        }
    }

    #[test]
    fn each_hop_is_given_the_time_of_its_request() {
        // A whole body: 60 s. The largest merged range (32 MiB): 60 + 2048 = 2108 s, on every hop.
        {
            let wire = Wire::with([redirect(302, STORAGE), answer(200, b"ok", "", "", "")]);
            assert!(call(&make_fetch(wire.hop()), &api_asset()).is_ok());
            let hops = wire.hops();
            assert_eq!(hops.len(), 2);
            assert_eq!(hops[0].timeout_seconds, 60);
            assert_eq!(hops[1].timeout_seconds, 60);
        }
        {
            let request = FetchRequest {
                range_start: 0,
                range_length: RANGE_MAX_LENGTH,
                max_bytes: RANGE_MAX_LENGTH,
                payload_size: 2 * RANGE_MAX_LENGTH,
                ..ranged_request()
            };
            let wire = Wire::with([redirect(302, STORAGE), answer(503, b"busy", "", "", "")]);
            assert!(call(&make_fetch(wire.hop()), &request).is_err());
            let hops = wire.hops();
            assert_eq!(hops.len(), 2);
            assert_eq!(hops[0].timeout_seconds, 2108);
            assert_eq!(hops[1].timeout_seconds, 2108);
        }
    }

    #[test]
    fn a_redirect_never_carries_the_token_not_even_back_to_the_api_host() {
        // "Never to a redirect target" (the owner's ruling, S2; FetchRequest::bearer_token). The
        // first hop is the one address the engine wrote; every later one was named by a server.
        {
            // api.github.com answers a renamed repository with a 301 to itself: the hop that
            // follows it is a redirect hop, and has no token.
            let wire = Wire::with([
                redirect(301, "/repositories/42/releases/assets/9"),
                redirect(302, STORAGE),
                answer(200, b"ok", "", "", ""),
            ]);
            assert!(call(&make_fetch(wire.hop()), &api_asset()).is_ok());
            let hops = wire.hops();
            assert_eq!(hops.len(), 3);
            assert_eq!(
                hops[1].url,
                "https://api.github.com/repositories/42/releases/assets/9"
            );
            assert!(has_header_named(&hops[0].headers, "Authorization"));
            assert!(!has_header_named(&hops[1].headers, "Authorization"));
            assert!(!has_header_named(&hops[2].headers, "Authorization"));
        }
        {
            // The storage host (or whoever answers in its place) sends the request back to the
            // API, at a path of its own choosing: that request is not an authenticated one.
            let wire = Wire::with([
                redirect(302, STORAGE),
                redirect(302, "https://api.github.com/user"),
                answer(200, b"ok", "", "", ""),
            ]);
            assert!(call(&make_fetch(wire.hop()), &api_asset()).is_ok());
            let hops = wire.hops();
            assert_eq!(hops.len(), 3);
            assert_eq!(hops[2].url, "https://api.github.com/user");
            assert!(has_header_named(&hops[0].headers, "Authorization"));
            assert!(!has_header_named(&hops[1].headers, "Authorization"));
            assert!(!has_header_named(&hops[2].headers, "Authorization"));
            for hop in &hops[1..] {
                for header in &hop.headers {
                    assert!(!header.contains(SECRET_TOKEN), "{}", hop.url);
                }
            }
        }
        {
            // A redirect to the very address that was asked for is a redirect all the same.
            let wire = Wire::with([
                redirect(307, "https://api.github.com/repos/o/r/releases/assets/9"),
                answer(200, b"ok", "", "", ""),
            ]);
            assert!(call(&make_fetch(wire.hop()), &api_asset()).is_ok());
            let hops = wire.hops();
            assert_eq!(hops.len(), 2);
            assert_eq!(hops[1].url, hops[0].url);
            assert!(has_header_named(&hops[0].headers, "Authorization"));
            assert!(!has_header_named(&hops[1].headers, "Authorization"));
        }
    }

    #[test]
    fn a_download_route_request_never_carries_a_token_anywhere() {
        // The engine fills bearer_token only for the API host; this holds the transport to it even
        // if a caller put one on a request to github.com.
        let wire = Wire::with([redirect(302, STORAGE), answer(200, b"ok", "", "", "")]);
        let request = FetchRequest {
            url: String::from(
                "https://github.com/o/r/releases/latest/download/Katana-update-win64.json",
            ),
            ..api_asset()
        };
        assert!(call(&make_fetch(wire.hop()), &request).is_ok());
        for hop in &wire.hops() {
            assert!(
                !has_header_named(&hop.headers, "Authorization"),
                "{}",
                hop.url
            );
        }
    }

    #[test]
    fn a_redirect_chain_of_five_is_followed_and_one_of_six_is_refused() {
        let chain = |redirects: usize| {
            let mut replies: Vec<Reply> = (0..redirects)
                .map(|i| redirect(302, &format!("https://hop{i}.example.org/x")))
                .collect();
            replies.push(answer(200, b"end", "", "", ""));
            let wire = Wire::with(replies);
            let body = call(&make_fetch(wire.hop()), &whole("https://github.com/x", 100));
            (body, wire.hops().len())
        };
        let (five, made) = chain(5);
        assert!(five.is_ok(), "{}", describe(&five));
        assert_eq!(made, 6);

        let (six, made) = chain(6);
        let error = six.expect_err("six redirects were followed");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert_eq!(fetch_failure_reason(&error), "too_many_redirects");
        assert_eq!(made, 6, "no seventh request is made");
    }

    #[test]
    fn a_redirect_to_http_or_to_anything_but_an_address_is_refused_without_being_followed() {
        for location in [
            "http://objects.example.org/x", // out of https
            "ftp://objects.example.org/x",
            "relative/path",
            "",
            "https://user:pw@objects.example.org/x",
        ] {
            let wire = Wire::with([
                redirect(302, location),
                answer(200, b"must not be asked for", "", "", ""),
            ]);
            let error = call(&make_fetch(wire.hop()), &api_asset())
                .expect_err(&format!("followed: {location}"));
            assert_eq!(fetch_failure_reason(&error), "bad_redirect", "{location}");
            assert_eq!(wire.hops().len(), 1, "{location}");
        }
    }

    #[test]
    fn plain_http_on_the_loopback_is_followed_only_when_the_request_began_there() {
        // A feed of the owner's own on 127.0.0.1 (a test, a local mirror), with a redirect to a
        // second port: allowed. The same redirect from an https address is not.
        {
            let wire = Wire::with([
                redirect(302, "http://127.0.0.1:50002/target"),
                answer(200, b"local", "", "", ""),
            ]);
            let request = FetchRequest {
                bearer_token: String::from(SECRET_TOKEN),
                ..whole("http://127.0.0.1:50001/start", 100)
            };
            let body = call(&make_fetch(wire.hop()), &request);
            assert!(body.is_ok(), "{}", describe(&body));
            let hops = wire.hops();
            assert_eq!(hops.len(), 2);
            assert!(!has_header_named(&hops[0].headers, "Authorization"));
            assert!(!has_header_named(&hops[1].headers, "Authorization"));
        }
        {
            let wire = Wire::with([redirect(302, "http://127.0.0.1:50002/target")]);
            assert!(
                call(
                    &make_fetch(wire.hop()),
                    &whole("https://github.com/start", 100)
                )
                .is_err()
            );
            assert_eq!(wire.hops().len(), 1);
        }
        {
            // A first address that is plain http off the loopback is not fetched at all.
            let wire = Wire::default();
            let error = call(
                &make_fetch(wire.hop()),
                &whole("http://updates.example.org/start", 100),
            )
            .expect_err("fetched");
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(wire.hops().is_empty());
        }
    }

    #[test]
    fn every_redirect_status_is_followed_and_every_other_status_is_a_failure() {
        for code in [301, 302, 303, 307, 308] {
            let wire = Wire::with([
                redirect(code, "https://example.org/next"),
                answer(200, b"ok", "", "", ""),
            ]);
            assert!(
                call(&make_fetch(wire.hop()), &whole("https://github.com/x", 10)).is_ok(),
                "{code}"
            );
        }
        // 300 and 304 are not redirects this program follows; they are unexpected answers.
        for code in [300, 304, 100, 204] {
            let wire = Wire::with([redirect(code, "https://example.org/next")]);
            let error = call(&make_fetch(wire.hop()), &whole("https://github.com/x", 10))
                .expect_err(&format!("{code}"));
            assert_eq!(fetch_failure_reason(&error), format!("http_{code}"));
        }
    }

    #[test]
    fn a_not_found_is_not_found_and_the_other_statuses_carry_their_number() {
        let missing = one(answer(404, b"<html>Not Found</html>", "text/html", "", ""))
            .expect_err("a 404 was a file");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(fetch_failure_reason(&missing), "http_404");
        assert!(
            missing.context.starts_with("status=404"),
            "{}",
            missing.context
        );

        for code in [400, 401, 403, 410, 416, 429, 500, 502, 503] {
            let refused = one(answer(code, b"body", "", "", "")).expect_err(&format!("{code}"));
            assert_eq!(refused.code, ErrorCode::FileImportFailure, "{code}");
            assert!(
                refused.context.starts_with(&format!("status={code}")),
                "{}",
                refused.context
            );
            assert_eq!(fetch_failure_reason(&refused), format!("http_{code}"));
        }
    }

    #[test]
    fn a_transport_failure_is_status_zero_and_is_reported_offline() {
        let offline = one(fail(make_error(
            ErrorCode::FileImportFailure,
            "the service's host name could not be found",
            "status=0; x",
        )))
        .expect_err("offline was an answer");
        assert_eq!(offline.code, ErrorCode::FileImportFailure);
        assert_eq!(fetch_failure_reason(&offline), "offline");
    }

    #[test]
    fn a_cancelled_hop_is_invalid_state_and_a_stop_before_the_first_hop_makes_no_request() {
        let cancelled = one(fail(Error::new(ErrorCode::InvalidState, "cancelled")))
            .expect_err("a cancelled hop was an answer");
        assert_eq!(cancelled.code, ErrorCode::InvalidState);
        assert_eq!(fetch_failure_reason(&cancelled), "cancelled");

        let wire = Wire::with([answer(200, b"x", "", "", "")]);
        let stop = AtomicBool::new(true); // the stop source has asked for a stop
        let error = call_stopped(
            &make_fetch(wire.hop()),
            &whole("https://github.com/x", 10),
            Some(&stop),
        )
        .expect_err("fetched after a stop");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(wire.hops().is_empty());
    }

    #[test]
    fn a_web_page_in_place_of_the_file_is_a_portal_whatever_its_status() {
        for content_type in ["text/html", "text/html; charset=utf-8", "TEXT/HTML"] {
            let error = one(answer(
                200,
                b"<html>Sign in to the network</html>",
                content_type,
                "",
                "",
            ))
            .expect_err(content_type);
            assert_eq!(error.code, ErrorCode::FileImportFailure);
            assert_eq!(fetch_failure_reason(&error), "html_page", "{content_type}");
        }
        // A 206 that is a web page is no part of a file.
        let partial = one_request(
            answer(206, &[b' '; 50], "text/html", "", "bytes 100-149/1000"),
            ranged_request(),
        )
        .expect_err("a page was a part");
        assert_eq!(fetch_failure_reason(&partial), "html_page");
        // JSON is not.
        assert!(
            one(answer(
                200,
                b"{}",
                "application/json; charset=utf-8",
                "",
                ""
            ))
            .is_ok()
        );
    }

    #[test]
    fn a_range_is_answered_by_the_range_asked_for_or_it_is_refused() {
        let part = [b'x'; 50];
        let request = FetchRequest {
            url: String::from("https://github.com/o/r/releases/download/v1/x.zip"),
            ..ranged_request()
        };

        assert!(
            one_request(
                answer(206, &part, "", "", "bytes 100-149/1000"),
                request.clone()
            )
            .is_ok()
        );
        assert!(
            one_request(
                answer(206, &part, "", "", "bytes 100-149/*"),
                request.clone()
            )
            .is_ok(),
            "an unknown total is allowed"
        );

        // Another part of the file, another file, no header, a short body: each is a 206 that is
        // not the range asked for, and says so with its own token (`http_206` would read as a
        // status the server refused with).
        for wrong in [
            answer(206, &part, "", "", "bytes 101-150/1000"),
            answer(206, &part, "", "", "bytes 100-149/999"),
            answer(206, &part, "", "", ""),
            answer(206, &part, "", "", "bytes 0-49/1000"),
            answer(206, &part[..49], "", "", "bytes 100-149/1000"),
        ] {
            let content_range = wrong.reply.content_range.clone();
            let error = one_request(wrong, request.clone()).expect_err(&content_range);
            assert_eq!(error.code, ErrorCode::FileImportFailure);
            assert_eq!(fetch_failure_reason(&error), "bad_range", "{content_range}");
            assert!(
                error.context.starts_with("status=206; reason=bad_range; "),
                "{}",
                error.context
            );
        }
    }

    #[test]
    fn a_servers_header_is_shown_bounded_and_printable_and_cannot_name_the_reason() {
        // The Content-Range of a refused answer is quoted for a person, so it is the server's text
        // in this program's Error: cut to a length, with nothing but printable ASCII, and after
        // the token, where it cannot pass for one.
        let request = FetchRequest {
            url: String::from("https://github.com/o/r/releases/download/v1/x.zip"),
            ..ranged_request()
        };
        let hostile = format!("reason=cancelled; \x1b[31m{}", "A".repeat(5000));
        let error = one_request(answer(206, &[b'x'; 50], "", "", &hostile), request)
            .expect_err("a hostile header was a range");
        assert_eq!(fetch_failure_reason(&error), "bad_range");
        assert!(error.context.len() < 400);
        for byte in error.context.bytes() {
            assert!((0x20..0x7f).contains(&byte), "byte {byte}");
        }
    }

    #[test]
    fn a_server_that_ignores_range_is_refused_as_partial_downloads_when_its_body_would_flood_the_range()
     {
        // The fake hop enforces max_bytes as the real one does: past it the transfer is abandoned.
        let hops: RefCell<Vec<HttpHop>> = RefCell::new(Vec::new());
        let hop = hop_fn(|one, _stop, _progress| {
            hops.borrow_mut().push(one.clone());
            const WHOLE_FILE: u64 = 1000; // answered whole, with a 200
            if WHOLE_FILE > one.max_bytes {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "the answer is larger than allowed",
                ));
            }
            Ok(HttpHopReply {
                status: 200,
                body: vec![b'z'; WHOLE_FILE as usize],
                ..HttpHopReply::default()
            })
        });
        let request = FetchRequest {
            url: String::from("https://github.com/o/r/releases/download/v1/x.zip"),
            ..ranged_request()
        };
        let error = call(&make_fetch(hop), &request).expect_err("a flood was a range");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(error.message, PARTIAL_DOWNLOADS_REFUSED);
        assert_eq!(fetch_failure_reason(&error), "partial_downloads");
        let hops = hops.borrow();
        assert_eq!(hops.len(), 1);
        assert_eq!(
            hops[0].max_bytes, 50,
            "the hop was cut off at the range length"
        );
    }

    #[test]
    fn a_two_hundred_that_is_not_the_whole_file_asked_for_is_partial_downloads_too() {
        // A server that ignores Range for a file shorter than the cut-off: the body fits
        // max_bytes, and is still not bytes 100-149.
        let mut request = FetchRequest {
            url: String::from("https://github.com/o/r/releases/download/v1/x.zip"),
            range_start: 0,
            range_length: 40,
            payload_size: 1000,
            max_bytes: 40,
            ..ranged_request()
        };
        let not_the_range = one_request(answer(200, &[b'q'; 40], "", "", ""), request.clone())
            .expect_err("a 200 was the range");
        assert_eq!(fetch_failure_reason(&not_the_range), "partial_downloads");

        // The whole file asked for as a range is a 200 that is all of it.
        request.range_length = 1000;
        request.max_bytes = 1000;
        assert!(one_request(answer(200, &[b'q'; 1000], "", "", ""), request).is_ok());
    }

    #[test]
    fn a_whole_body_request_wants_a200_and_the_engine_s_bound() {
        let request = whole("https://github.com/x", 10);
        assert!(one_request(answer(200, &[b'a'; 10], "", "", ""), request.clone()).is_ok());
        assert!(
            one_request(answer(206, b"abc", "", "", "bytes 0-2/10"), request.clone()).is_err(),
            "a part of a file that was asked for whole"
        );
        // A hop that does not enforce the bound is held to it anyway.
        let over = one_request(answer(200, &[b'a'; 11], "", "", ""), request).expect_err("over");
        assert_eq!(over.code, ErrorCode::Unsupported);
        assert_eq!(fetch_failure_reason(&over), "too_large");
    }

    #[test]
    fn an_answer_that_ran_past_its_bound_is_named_by_what_the_answer_said_it_was() {
        let ranged = FetchRequest {
            url: String::from("https://github.com/o/r/releases/download/v1/x.zip"),
            ..ranged_request() // bytes 100-149 of 1000, at most 50
        };

        // A portal's page is a page however long it is. The signature is asked for with a bound
        // of 1024 bytes, and a sign-in page is longer than that: it is still "html_page", and not
        // a file that is too large or a network without partial downloads.
        for content_type in ["text/html", "text/html; charset=utf-8"] {
            let page = one(overrun(200, content_type, "")).expect_err(content_type);
            assert_eq!(page.code, ErrorCode::FileImportFailure, "{content_type}");
            assert_eq!(fetch_failure_reason(&page), "html_page", "{content_type}");
            let in_place_of_a_range = one_request(overrun(200, content_type, ""), ranged.clone())
                .expect_err(content_type);
            assert_eq!(
                fetch_failure_reason(&in_place_of_a_range),
                "html_page",
                "{content_type}"
            );
        }

        // A whole file longer than the engine reads before it has verified it.
        let large = one(overrun(200, "application/octet-stream", "")).expect_err("large");
        assert_eq!(large.code, ErrorCode::Unsupported);
        assert_eq!(fetch_failure_reason(&large), "too_large");
        assert!(
            large.context.starts_with("status=200; "),
            "{}",
            large.context
        );

        // A range answered with a 200 that began the whole file: this network does not do ranges.
        let ignored =
            one_request(overrun(200, "application/zip", ""), ranged.clone()).expect_err("ignored");
        assert_eq!(ignored.code, ErrorCode::Unsupported);
        assert_eq!(ignored.message, PARTIAL_DOWNLOADS_REFUSED);
        assert_eq!(fetch_failure_reason(&ignored), "partial_downloads");
        assert!(
            ignored.context.starts_with("status=200; "),
            "{}",
            ignored.context
        );

        // A range answered with a 206 that ran past the range: a part of the file, and not the
        // part asked for. Partial downloads ARE allowed here; this one is wrong (feed.rs: a 206
        // whose body is of another length is `status=206; reason=bad_range`).
        let past = one_request(
            overrun(206, "application/zip", "bytes 100-149/1000"),
            ranged.clone(),
        )
        .expect_err("past");
        assert_eq!(past.code, ErrorCode::FileImportFailure);
        assert_eq!(fetch_failure_reason(&past), "bad_range");
        assert!(
            past.context.starts_with("status=206; reason=bad_range; "),
            "{}",
            past.context
        );
        // The same from a hop that does not enforce the bound and hands the long body over: 51
        // bytes where 50 were asked for.
        let handed_over = one_request(
            answer(206, &[b'x'; 51], "", "", "bytes 100-149/1000"),
            ranged,
        )
        .expect_err("handed over");
        assert_eq!(handed_over.code, ErrorCode::FileImportFailure);
        assert_eq!(fetch_failure_reason(&handed_over), "bad_range");
    }

    #[test]
    fn a_request_that_is_not_bounded_or_not_an_address_is_refused_before_any_hop() {
        let wire = Wire::with([answer(200, b"x", "", "", "")]);
        let fetch = make_fetch(wire.hop());

        let unbounded = whole("https://github.com/x", 0);
        assert_eq!(
            call(&fetch, &unbounded).expect_err("unbounded").code,
            ErrorCode::InvalidArgument
        );

        let no_payload_size = FetchRequest {
            payload_size: 0,
            ..ranged_request()
        };
        assert_eq!(
            call(&fetch, &no_payload_size)
                .expect_err("no payload size")
                .code,
            ErrorCode::InvalidArgument
        );

        for url in [
            "",
            "ftp://github.com/x",
            "github.com/x",
            "https://github.com/a b",
        ] {
            assert_eq!(
                call(&fetch, &whole(url, 10)).expect_err(url).code,
                ErrorCode::InvalidArgument,
                "{url}"
            );
        }
        assert!(wire.hops().is_empty());

        // No transport at all is a defect that says so, not a crash.
        let nothing: HopFn<'_> = None;
        assert_eq!(
            call(&make_fetch(nothing), &unbounded)
                .expect_err("no transport")
                .code,
            ErrorCode::Internal
        );
        assert_eq!(
            call(&make_fetch(None), &whole("https://github.com/x", 10))
                .expect_err("no transport")
                .code,
            ErrorCode::Internal
        );
    }

    #[test]
    fn progress_is_passed_to_the_hop() {
        let seen: RefCell<Vec<(u64, u64)>> = RefCell::new(Vec::new());
        let hop = hop_fn(|_one, _stop, progress| {
            let progress = progress.expect("the hop is given the progress function");
            progress(5, 10);
            progress(10, 10);
            Ok(HttpHopReply {
                status: 200,
                body: b"0123456789".to_vec(),
                ..HttpHopReply::default()
            })
        });
        let record = |done: u64, total: u64| seen.borrow_mut().push((done, total));
        let body = call_with_progress(
            &make_fetch(hop),
            &whole("https://github.com/x", 10),
            Some(&record),
        );
        assert!(body.is_ok(), "{}", describe(&body));
        let seen = seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1], (10, 10));
    }

    // ---- nothing secret leaves the transport in an Error ---------------------------------------

    #[test]
    fn no_error_carries_the_token_a_header_or_the_signature_of_a_storage_address() {
        // Every way a request through the signed storage address can fail, with the token on the
        // request: the Error's text must name neither the token nor the query of the address.
        let storage_failures = [
            answer(403, b"denied", "", "", ""),
            answer(404, b"gone", "", "", ""),
            answer(429, b"slow", "", "", ""),
            answer(500, b"oops", "", "", ""),
            answer(200, b"<html>", "text/html", "", ""),
            redirect(
                302,
                "http://downgraded.example.org/x?sig=SIGNATURE-IN-QUERY",
            ),
            redirect(302, ""),
            fail(make_error(
                ErrorCode::FileImportFailure,
                "no route",
                "status=0; x",
            )),
        ];
        for storage in storage_failures {
            let wire = Wire::with([redirect(302, STORAGE), storage]);
            let error = call(&make_fetch(wire.hop()), &api_asset()).expect_err("an answer");
            let text = error.describe();
            assert!(!text.contains(SECRET_TOKEN), "{text}");
            assert!(!text.contains("SIGNATURE-IN-QUERY"), "{text}");
            assert!(!text.contains("sig="), "{text}");
            assert!(!text.contains("Bearer"), "{text}");
        }
        // And the failure of the first hop, to the API host with the token.
        for code in [401, 403, 404, 429, 500] {
            let wire = Wire::with([answer(code, b"body", "", "", "")]);
            let error = call(&make_fetch(wire.hop()), &api_asset()).expect_err("an answer");
            assert!(!error.describe().contains(SECRET_TOKEN), "{code}");
        }
    }

    #[test]
    fn a_hop_that_puts_the_token_in_its_own_error_does_not_get_it_past_the_fetch() {
        // The hop function is the caller's (katana_app's, over libcurl). Suppose it, or the
        // library under it, quotes a request header in a failure: the Fetch still returns no
        // token. Every code a hop may fail with, with the token in the message and in the context.
        let bearer = format!("Authorization: Bearer {SECRET_TOKEN}");
        for code in [
            ErrorCode::FileImportFailure,
            ErrorCode::InvalidState,
            ErrorCode::Unsupported,
            ErrorCode::InvalidArgument,
            ErrorCode::Internal,
        ] {
            let wire = Wire::with([fail(make_error(
                code,
                format!("could not send '{bearer}'"),
                format!("status=0; header {bearer}; {SECRET_TOKEN}"),
            ))]);
            let error = call(&make_fetch(wire.hop()), &api_asset()).expect_err("an answer");
            assert_eq!(error.code, code, "the failure is the hop's, less the token");
            assert!(
                !error.describe().contains(SECRET_TOKEN),
                "{}",
                error.describe()
            );
            // What was beside the token is still there to read.
            assert!(
                error.message.contains("could not send"),
                "{}",
                error.message
            );
            assert!(error.context.starts_with("status=0; "), "{}", error.context);
        }
        // The same on a later hop, where the header was never sent: the request still holds it.
        let wire = Wire::with([
            redirect(302, STORAGE),
            fail(make_error(
                ErrorCode::FileImportFailure,
                format!("echo {SECRET_TOKEN}"),
                "status=0",
            )),
        ]);
        let later = call(&make_fetch(wire.hop()), &api_asset()).expect_err("an answer");
        assert!(!later.describe().contains(SECRET_TOKEN));
    }

    #[test]
    fn a_hop_that_quotes_the_signed_address_does_not_get_its_query_past_the_fetch() {
        // The hop function is the caller's, over a library that puts the address it was given into
        // what it says about a failure: whole, or with the parameters it takes for secrets hidden
        // and the others left (a redaction by parameter name knows `sig`, and has never heard of
        // `sp` or `se`). The address a release redirects to is a signed one: all of its query is
        // the secret. So whatever a hop says, the Error of the Fetch names the address once, where
        // the Fetch writes it, without its query, and nothing else in it has a query either.
        let partly_hidden = "https://release-assets.githubusercontent.com/github-production-release-asset/1/uuid?sp=r&sig=***&se=2026-10-20T08%3A00%3A00Z";
        let said = [
            (
                String::from("the request failed"),
                format!("{STORAGE}: Could not resolve host: release-assets.githubusercontent.com"),
            ),
            (
                String::from("the request failed"),
                format!("{partly_hidden}: SSL certificate problem"),
            ),
            (
                format!("no answer from {STORAGE}"),
                format!("status=0; {STORAGE}"),
            ),
            (
                format!("no answer from '{partly_hidden}'"),
                format!("status=0; reason=cancelled; {partly_hidden}"),
            ),
        ];
        for code in [
            ErrorCode::FileImportFailure,
            ErrorCode::InvalidState,
            ErrorCode::Unsupported,
            ErrorCode::InvalidArgument,
            ErrorCode::Internal,
        ] {
            for (message, context) in &said {
                let wire = Wire::with([
                    redirect(302, STORAGE),
                    fail(make_error(code, message.as_str(), context.as_str())),
                ]);
                let error = call(&make_fetch(wire.hop()), &api_asset()).expect_err("an answer");
                assert_eq!(error.code, code, "the failure is the hop's");
                let text = error.describe();
                for secret in [
                    "SIGNATURE-IN-QUERY",
                    "sig=",
                    "sp=r",
                    "se=2026",
                    "?",
                    SECRET_TOKEN,
                ] {
                    assert!(!text.contains(secret), "{text}");
                }
                // No answer arrived, and then the address as a person may read it.
                assert!(
                    error.context.starts_with(
                        "status=0; https://release-assets.githubusercontent.com/github-production-release-asset/1/uuid"
                    ),
                    "{}",
                    error.context
                );
            }
        }
    }

    #[test]
    fn a_hops_error_keeps_its_status_and_its_words_and_cannot_name_a_reason() {
        // feed.rs, HopFn: the code and the sentence are the hop's; the context is the Fetch's: the
        // status the hop led with (0 when it named none), the address, then what else the hop
        // said, as text. The address here is one_request's.
        let address = "https://github.com/o/r/releases/latest/download/x";

        let refused = one(fail(make_error(
            ErrorCode::FileImportFailure,
            "the service's certificate could not be verified",
            "status=0; SSL certificate problem: unable to get local issuer certificate",
        )))
        .expect_err("an answer");
        assert_eq!(refused.code, ErrorCode::FileImportFailure);
        assert_eq!(
            refused.message,
            "the service's certificate could not be verified"
        );
        assert_eq!(
            refused.context,
            format!(
                "status=0; {address}; SSL certificate problem: unable to get local issuer certificate"
            )
        );
        assert_eq!(fetch_failure_reason(&refused), "offline");

        let bare =
            one(fail(Error::new(ErrorCode::FileImportFailure, "no route"))).expect_err("an answer");
        assert_eq!(bare.context, format!("status=0; {address}"));

        // A reason is this program's to give. One a hop wrote (first in its context, or after its
        // status) comes after the address, where it is words.
        let named = one(fail(make_error(
            ErrorCode::FileImportFailure,
            "m",
            "reason=html_page; x",
        )))
        .expect_err("an answer");
        assert_eq!(
            named.context,
            format!("status=0; {address}; reason=html_page; x")
        );
        assert_eq!(fetch_failure_reason(&named), "offline");
        let after_status = one(fail(make_error(
            ErrorCode::FileImportFailure,
            "m",
            "status=0; reason=cancelled; x",
        )))
        .expect_err("an answer");
        assert_eq!(
            after_status.context,
            format!("status=0; {address}; reason=cancelled; x")
        );
        assert_eq!(fetch_failure_reason(&after_status), "offline");
    }

    #[test]
    fn an_address_cannot_name_the_reason_of_a_failure() {
        // A path may spell anything, `reason=` and `status=` included; it comes after the fields
        // this program writes, and is not read as one of them. Here a server redirects to such an
        // address, which then refuses with 403.
        let wire = Wire::with([
            redirect(
                302,
                "https://evil.example/status=200;reason=cancelled;/portal=html",
            ),
            status(403),
        ]);
        let error = call(
            &make_fetch(wire.hop()),
            &whole("https://github.com/o/r/releases/latest/download/x", 100),
        )
        .expect_err("an answer");
        assert_eq!(
            fetch_failure_reason(&error),
            "http_403",
            "{}",
            error.context
        );
    }
}

// ---- the reason tokens -------------------------------------------------------------------------

fn reason_of(code: ErrorCode, message: &str, context: &str) -> String {
    fetch_failure_reason(&make_error(code, message, context))
}

mod failure_reason {
    use super::*;

    #[test]
    fn each_failure_of_a_fetch_has_a_short_machine_readable_token() {
        use ErrorCode::*;
        assert_eq!(reason_of(InvalidState, "cancelled", ""), "cancelled");
        assert_eq!(
            reason_of(NotFound, "nothing", "status=404; https://x/y"),
            "http_404"
        );
        assert_eq!(
            reason_of(FileImportFailure, "m", "status=403; https://x/y"),
            "http_403"
        );
        assert_eq!(
            reason_of(FileImportFailure, "m", "status=429; https://x/y"),
            "http_429"
        );
        assert_eq!(reason_of(FileImportFailure, "m", "status=503"), "http_503");
        assert_eq!(
            reason_of(FileImportFailure, "m", "status=0; https://x/y; timeout"),
            "offline"
        );
        assert_eq!(
            reason_of(FileImportFailure, "m", "no status here"),
            "offline"
        );
        assert_eq!(
            reason_of(
                FileImportFailure,
                "m",
                "status=200; reason=html_page; https://x/y"
            ),
            "html_page"
        );
        assert_eq!(
            reason_of(Unsupported, PARTIAL_DOWNLOADS_REFUSED, "status=200"),
            "partial_downloads"
        );
        assert_eq!(
            reason_of(Unsupported, "longer than allowed", "status=200"),
            "too_large"
        );
        // A reason the code that made the Error gave it wins, up to the next ';': first in the
        // context, or straight after the status.
        assert_eq!(
            reason_of(InvalidArgument, "m", "reason=staged_hash; path=bin/x"),
            "staged_hash"
        );
        assert_eq!(
            reason_of(FileExportFailure, "m", "reason=low_disk_space"),
            "low_disk_space"
        );
        assert_eq!(
            reason_of(
                FileImportFailure,
                "m",
                "status=302; reason=bad_redirect; https://x/y"
            ),
            "bad_redirect"
        );
        assert_eq!(reason_of(Internal, "m", ""), "error");
        assert_eq!(
            reason_of(ParseFailure, "m", "reason="),
            "error",
            "an empty token is none"
        );
        assert_eq!(
            reason_of(FileImportFailure, "m", "status=403; reason=; https://x/y"),
            "http_403",
            "an empty token is none after a status too"
        );
    }

    #[test]
    fn a_reason_is_read_only_where_this_program_writes_one() {
        // feed.rs: `status=<n>; reason=<token>; <address>; <detail>`. An address and a server's
        // words come after the two fields, and whatever they spell is not a token.
        use ErrorCode::*;
        assert_eq!(
            reason_of(
                FileImportFailure,
                "m",
                "status=403; https://x/reason=cancelled;y"
            ),
            "http_403"
        );
        assert_eq!(
            reason_of(
                FileImportFailure,
                "m",
                "status=429; https://x/y; reason=html_page"
            ),
            "http_429"
        );
        assert_eq!(
            reason_of(NotFound, "m", "status=404; https://x/y?reason=offline"),
            "http_404"
        );
        assert_eq!(
            reason_of(
                FileImportFailure,
                "m",
                "https://x/status=500; reason=cancelled"
            ),
            "offline",
            "a status is read only at the very start"
        );
        assert_eq!(
            reason_of(FileImportFailure, "m", "status=200x; reason=cancelled"),
            "http_200",
            "what follows the number must be the separator"
        );
        assert_eq!(
            reason_of(FileImportFailure, "m", "status=; reason=cancelled"),
            "offline",
            "a status with no number is not the leading field, so nothing follows it"
        );
        assert_eq!(
            reason_of(InvalidState, "m", "path=bin/x; reason=staged_hash"),
            "cancelled",
            "not first: not a token"
        );
    }
}
