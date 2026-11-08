//! The contract's own behaviour: the pure functions the skeleton finished itself (the C++'s
//! src/katana_update/contract.cpp, feed.cpp and arguments.cpp) and the constants and names the
//! modules fix. The twin of tests/update/test_contract.cpp. Each case is the regression test of a
//! defect an independent review found in the first skeleton, and each expectation is worked out by
//! hand from the rule the module states - a calendar of seconds, RFC 3986's unreserved set,
//! UTF-8's bit layout - never read off the code under test (CLAUDE.md, section 4).

use katana_core::ErrorCode;
use katana_update::apply::{ApplyOutcome, is_valid_transaction_id, to_string as outcome_word};
use katana_update::feed::{
    FeedConfig, FeedRoute, FetchRequest, MAX_MANIFEST_BYTES, MAX_RELEASE_JSON_BYTES,
    MAX_SIGNATURE_BYTES, asset_api_url, manifest_asset_name, manifest_url, payload_url,
    release_by_tag_url, release_lookup_url, signature_asset_name, signature_url,
};
use katana_update::install::{Install, install_root_of_executable};
use katana_update::manifest::{Manifest, Payload};
use katana_update::paths::{INSTALL_ROOTS, MAX_FILES};
use katana_update::stage::{
    STAGED_MANIFEST_NAME, STAGED_SIGNATURE_NAME, staged_manifest_path, staged_signature_path,
};
use katana_update::state::{CHECK_INTERVAL, CHECK_JITTER_MAX, UpdateState, check_is_due};
use katana_update::trust::is_newer_than;
use katana_update::version::Version;
use std::path::{Path, PathBuf};

// A version written out by hand, so these cases do not depend on parse_version.
fn version_of(major: u32, minor: u32, patch: u32) -> Version {
    Version {
        components: [major, minor, patch, 0],
        written: 3,
    }
}

fn manifest_of(version: Version, sequence: u64) -> Manifest {
    Manifest {
        version,
        sequence,
        ..Manifest::default()
    }
}

fn install_of(version: Version, sequence: u64, has_record: bool) -> Install {
    Install {
        version,
        sequence,
        has_installed_record: has_record,
        ..Install::default()
    }
}

// ---- is_newer_than (trust.rs) ---------------------------------------------------------------------

mod update_contract {
    use super::*;

    #[test]
    fn a_fresh_install_is_not_offered_the_release_it_was_made_from() {
        // The zip a person unpacks is 0.3.0; the feed's manifest for 0.3.0 was built at sequence
        // 1792312800. The install has no installed.json, so its sequence is the placeholder 0
        // and, compared as a number, every fresh install would be "behind" its own release by
        // 1792312800 and the status bar would offer "Update 0.3.0" on the first start.
        let release = manifest_of(version_of(0, 3, 0), 1792312800);
        assert!(!is_newer_than(
            &release,
            &install_of(version_of(0, 3, 0), 0, false)
        ));
    }

    #[test]
    fn a_fresh_install_is_still_offered_a_higher_version() {
        let fresh = install_of(version_of(0, 3, 0), 0, false);
        assert!(is_newer_than(&manifest_of(version_of(0, 3, 1), 5), &fresh));
        assert!(is_newer_than(&manifest_of(version_of(0, 4, 0), 5), &fresh));
        assert!(is_newer_than(&manifest_of(version_of(1, 0, 0), 5), &fresh));
    }

    #[test]
    fn an_install_with_a_record_is_offered_the_same_version_only_at_a_higher_sequence() {
        let v030 = version_of(0, 3, 0);
        let recorded = install_of(v030, 100, true);
        assert!(is_newer_than(&manifest_of(v030, 101), &recorded)); // a workflow re-run
        assert!(!is_newer_than(&manifest_of(v030, 100), &recorded)); // the same build
        assert!(!is_newer_than(&manifest_of(v030, 99), &recorded)); // an older build
    }

    #[test]
    fn a_higher_version_is_newer_whatever_the_sequences_are() {
        // The version decides first: a re-run of an old release (a huge sequence) cannot outrank
        // a newer version, and a newer version outranks a bigger recorded sequence.
        let recorded = install_of(version_of(0, 3, 0), 999999, true);
        assert!(is_newer_than(
            &manifest_of(version_of(0, 3, 1), 1),
            &recorded
        ));
    }

    #[test]
    fn a_lower_version_is_never_newer_whatever_its_sequence() {
        let huge: u64 = 4_000_000_000_000;
        assert!(!is_newer_than(
            &manifest_of(version_of(0, 2, 9), huge),
            &install_of(version_of(0, 3, 0), 1, true)
        ));
        assert!(!is_newer_than(
            &manifest_of(version_of(0, 2, 9), huge),
            &install_of(version_of(0, 3, 0), 0, false)
        ));
    }

    #[test]
    fn versions_are_compared_as_numbers_and_a_trailing_zero_is_not_a_difference() {
        // 0.10 is above 0.9 (10 > 9; as text "0.10" < "0.9").
        assert!(is_newer_than(
            &manifest_of(version_of(0, 10, 0), 1),
            &install_of(version_of(0, 9, 0), 0, false)
        ));
        // "0.3" and "0.3.0" are one version (version.rs), so the shorter spelling is not newer.
        let two_components = Version {
            components: [0, 3, 0, 0],
            written: 2,
        };
        assert!(!is_newer_than(
            &manifest_of(two_components, 7),
            &install_of(version_of(0, 3, 0), 0, false)
        ));
    }

    #[test]
    fn an_install_defaults_to_no_record() {
        assert!(!Install::default().has_installed_record);
    }
}

// ---- install_root_of_executable (install.rs) ------------------------------------------------------

// A folder of the test's own, by its name and the process's id (the test binary's tests run side
// by side in one process), removed on drop.
struct InstallRoot {
    base: PathBuf,
}

impl InstallRoot {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "katana_update_contract_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("the test's folder");
        Self { base }
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.base.join(relative);
        std::fs::create_dir_all(&path).expect("a folder of the test");
        path
    }
}

impl Drop for InstallRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

mod install_root {
    use super::*;

    #[test]
    fn a_bin_folder_beside_share_is_the_flat_layout_of_windows_and_linux() {
        let fixture = InstallRoot::new("a_bin_folder_beside_share_is_the_flat_layout");
        let root = fixture.dir("Katana");
        fixture.dir("Katana/bin");
        fixture.dir("Katana/share");
        let found = install_root_of_executable(&root.join("bin").join("katana.exe"));
        assert_eq!(found.expect("an install"), root);
    }

    #[test]
    fn the_name_bin_is_matched_ignoring_case() {
        let fixture = InstallRoot::new("the_name_bin_is_matched_ignoring_case");
        let root = fixture.dir("Katana");
        fixture.dir("Katana/BIN");
        fixture.dir("Katana/share");
        let found = install_root_of_executable(&root.join("BIN").join("katana.exe"));
        assert_eq!(found.expect("an install"), root);
    }

    #[test]
    fn a_mac_os_bundle_has_its_root_in_contents_resources_where_share_is() {
        // The layout cmake/KatanaPackaging.cmake gives the bundle: programs in Contents/MacOS,
        // the data - and share/katana/install.json - in Contents/Resources/share. There is no
        // bin/.
        let fixture = InstallRoot::new("a_mac_os_bundle_has_its_root_in_contents_resources");
        let resources = fixture.dir("Katana.app/Contents/Resources");
        fixture.dir("Katana.app/Contents/Resources/share/katana");
        fixture.dir("Katana.app/Contents/MacOS");
        let found = install_root_of_executable(
            &fixture
                .base
                .join("Katana.app")
                .join("Contents")
                .join("MacOS")
                .join("katana"),
        );
        assert_eq!(found.expect("an install"), resources);
    }

    #[test]
    fn an_application_bundle_without_share_in_its_resources_is_not_an_install() {
        let fixture = InstallRoot::new("an_application_bundle_without_share");
        fixture.dir("Katana.app/Contents/Resources");
        fixture.dir("Katana.app/Contents/MacOS");
        let found = install_root_of_executable(
            &fixture
                .base
                .join("Katana.app")
                .join("Contents")
                .join("MacOS")
                .join("katana"),
        );
        assert_eq!(found.expect_err("not an install").code, ErrorCode::NotFound);
    }

    #[test]
    fn a_bin_folder_with_no_share_beside_it_is_not_an_install() {
        let fixture = InstallRoot::new("a_bin_folder_with_no_share_beside_it");
        fixture.dir("Somewhere/bin");
        let found =
            install_root_of_executable(&fixture.base.join("Somewhere").join("bin").join("tool"));
        assert_eq!(found.expect_err("not an install").code, ErrorCode::NotFound);
    }

    #[test]
    fn share_beside_a_folder_that_is_not_bin_is_not_an_install() {
        let fixture = InstallRoot::new("share_beside_a_folder_that_is_not_bin");
        fixture.dir("Katana/tools");
        fixture.dir("Katana/share");
        let found =
            install_root_of_executable(&fixture.base.join("Katana").join("tools").join("tool"));
        assert!(found.is_err());
    }

    #[test]
    fn a_file_named_share_is_not_the_share_folder() {
        let fixture = InstallRoot::new("a_file_named_share_is_not_the_share_folder");
        fixture.dir("Katana/bin");
        std::fs::write(fixture.base.join("Katana").join("share"), "not a directory")
            .expect("the file");
        let found =
            install_root_of_executable(&fixture.base.join("Katana").join("bin").join("katana.exe"));
        assert!(found.is_err());
    }

    #[test]
    fn a_bare_file_name_has_no_install() {
        let found = install_root_of_executable(Path::new("katana.exe"));
        assert_eq!(found.expect_err("not an install").code, ErrorCode::NotFound);
    }
}

// ---- the feed's addresses (feed.rs) ---------------------------------------------------------------

fn github_feed(target: &str) -> FeedConfig {
    FeedConfig {
        repo: String::from("kjarada/Katana"),
        target: String::from(target),
        ..FeedConfig::default()
    }
}

fn release_manifest(tag: &str, payload_name: &str) -> Manifest {
    Manifest {
        tag: String::from(tag),
        payload: Some(Payload {
            codec: String::from("deflate"),
            name: String::from(payload_name),
            size: 0,
        }),
        ..Manifest::default()
    }
}

mod update_feed_addresses {
    use super::*;

    #[test]
    fn the_download_route_names_the_latest_releases_fixed_assets() {
        let feed = github_feed("win64");
        assert_eq!(
            manifest_url(&feed),
            "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json"
        );
        assert_eq!(
            signature_url(&feed),
            "https://github.com/kjarada/Katana/releases/latest/download/\
             Katana-update-win64.json.sig"
        );
        assert_eq!(
            payload_url(
                &feed,
                &release_manifest("v0.3.0", "Katana-0.3.0-win64.update.zip")
            ),
            "https://github.com/kjarada/Katana/releases/download/v0.3.0/\
             Katana-0.3.0-win64.update.zip"
        );
    }

    #[test]
    fn a_feed_base_replaces_github_for_all_three() {
        let feed = FeedConfig {
            feed_base: String::from("https://updates.example.org/katana"),
            ..github_feed("linux-x86_64")
        };
        assert_eq!(
            manifest_url(&feed),
            "https://updates.example.org/katana/Katana-update-linux-x86_64.json"
        );
        assert_eq!(
            signature_url(&feed),
            "https://updates.example.org/katana/Katana-update-linux-x86_64.json.sig"
        );
        assert_eq!(
            payload_url(
                &feed,
                &release_manifest("v0.3.0", "Katana-0.3.0-linux-x86_64.update.zip")
            ),
            "https://updates.example.org/katana/Katana-0.3.0-linux-x86_64.update.zip"
        );
    }

    #[test]
    fn a_notify_only_manifest_has_no_payload_address() {
        let manifest = Manifest {
            notify_only: true,
            tag: String::from("v0.3.0"),
            ..Manifest::default()
        };
        assert!(payload_url(&github_feed("macos-arm64"), &manifest).is_empty());
    }

    #[test]
    fn a_signed_name_cannot_add_a_segment_or_a_query_to_the_address() {
        // RFC 3986: only A-Z a-z 0-9 - . _ ~ pass; every other byte is %XX in upper-case hex.
        // '/' is 0x2F, ' ' is 0x20, '?' is 0x3F, '#' is 0x23, '%' itself is 0x25.
        let feed = github_feed("win64");
        assert_eq!(
            payload_url(&feed, &release_manifest("v1/../2", "a b?c#d%e.zip")),
            "https://github.com/kjarada/Katana/releases/download/v1%2F..%2F2/a%20b%3Fc%23d%25e.zip"
        );
        assert_eq!(
            release_by_tag_url(&feed, "v1/../2"),
            "https://api.github.com/repos/kjarada/Katana/releases/tags/v1%2F..%2F2"
        );
    }

    #[test]
    fn the_private_repository_route_is_the_rest_api_on_api_github_com() {
        let feed = github_feed("win64");
        assert_eq!(
            release_lookup_url(&feed),
            "https://api.github.com/repos/kjarada/Katana/releases/latest"
        );
        assert_eq!(
            release_by_tag_url(&feed, "v0.3.0"),
            "https://api.github.com/repos/kjarada/Katana/releases/tags/v0.3.0"
        );
        assert_eq!(
            asset_api_url(&feed, 7654321),
            "https://api.github.com/repos/kjarada/Katana/releases/assets/7654321"
        );
        // The route is GitHub's: a feed_base does not move it. And the token's one destination,
        // api.github.com over https, is where every one of these goes.
        let mirror = FeedConfig {
            feed_base: String::from("https://updates.example.org/katana"),
            ..feed.clone()
        };
        for url in [
            release_lookup_url(&mirror),
            release_by_tag_url(&mirror, "v1"),
            asset_api_url(&mirror, 1),
        ] {
            assert!(url.starts_with("https://api.github.com/"), "{url}");
        }
    }

    #[test]
    fn no_download_route_address_is_on_the_api_host() {
        // The token is sent only to api.github.com, so a request the Download route makes must
        // never be one the transport could attach it to.
        let feed = github_feed("win64");
        let manifest = release_manifest("v0.3.0", "Katana-0.3.0-win64.update.zip");
        for url in [
            manifest_url(&feed),
            signature_url(&feed),
            payload_url(&feed, &manifest),
        ] {
            assert!(url.starts_with("https://github.com/"), "{url}");
        }
    }

    #[test]
    fn the_asset_names_are_the_fixed_ones_of_the_target() {
        assert_eq!(
            manifest_asset_name("win-arm64"),
            "Katana-update-win-arm64.json"
        );
        assert_eq!(
            signature_asset_name("win-arm64"),
            "Katana-update-win-arm64.json.sig"
        );
    }

    #[test]
    fn the_route_defaults_to_download_so_no_token_goes_anywhere_unless_check_says_so() {
        assert_eq!(FeedConfig::default().route, FeedRoute::Download);
        assert!(FetchRequest::default().bearer_token.is_empty());
        assert!(FetchRequest::default().accept.is_empty());
    }

    #[test]
    #[allow(
        clippy::assertions_on_constants,
        reason = "the C++ holds the bounds the feed module fixes to what they must admit"
    )]
    fn what_is_fetched_before_verification_has_a_bound_the_engine_sets() {
        // The signature line is `ed25519 ` (8) + the key id (16) + ' ' (1) + base64 of 64 bytes
        // (4 * ceil(64 / 3) = 88) + CRLF (2) = 115 bytes at most.
        assert!(MAX_SIGNATURE_BYTES >= 115);
        // A manifest of MAX_FILES entries at about 400 bytes each (a 260-character path, a
        // 64-digit hash, the sizes and offsets and the field names) must fit.
        assert!(MAX_MANIFEST_BYTES >= MAX_FILES as u64 * 400);
        assert!(MAX_RELEASE_JSON_BYTES > 0);
        // None is unbounded.
        assert!(MAX_MANIFEST_BYTES < 1u64 << 30);
    }
}

// ---- the staged manifest (stage.rs) ---------------------------------------------------------------

mod update_staged_manifest {
    use super::*;

    #[test]
    fn its_files_are_named_at_the_top_of_the_stage_folder() {
        assert_eq!(
            staged_manifest_path(Path::new("S")),
            Path::new("S").join("manifest.json")
        );
        assert_eq!(
            staged_signature_path(Path::new("S")),
            Path::new("S").join("manifest.json.sig")
        );
        // The signature file is the manifest's name and ".sig", as in the feed.
        assert_eq!(STAGED_SIGNATURE_NAME, format!("{STAGED_MANIFEST_NAME}.sig"));
    }

    #[test]
    fn no_manifest_path_can_be_a_member_of_the_stage_folder_root() {
        // A manifest path starts with bin, share, etc or lib (paths.rs). The staged manifest's
        // names are at the root and are neither, so a staged file can never overwrite them.
        for name in [STAGED_MANIFEST_NAME, STAGED_SIGNATURE_NAME] {
            for root in INSTALL_ROOTS {
                assert_ne!(name, root);
            }
            assert!(!name.contains('/'));
        }
    }
}

// ---- how an apply ends (apply.rs, state.rs) -------------------------------------------------------

mod update_outcomes {
    use super::*;

    #[test]
    fn every_outcome_has_its_own_last_result_status() {
        assert_eq!(outcome_word(ApplyOutcome::Committed), "committed");
        assert_eq!(outcome_word(ApplyOutcome::Blocked), "blocked");
        assert_eq!(outcome_word(ApplyOutcome::RolledBack), "rolled_back");
        assert_eq!(outcome_word(ApplyOutcome::Refused), "refused");
        // A rollback that itself failed is not "rolled_back": the tree may be half swapped.
        assert_eq!(
            outcome_word(ApplyOutcome::RollbackIncomplete),
            "rollback_incomplete"
        );
        assert_ne!(
            outcome_word(ApplyOutcome::RollbackIncomplete),
            outcome_word(ApplyOutcome::RolledBack)
        );
    }
}

// ---- transaction ids (apply.rs) -------------------------------------------------------------------

mod update_transaction_ids {
    use super::*;

    #[test]
    fn lower_case_hex_and_digits_of_one_to_sixty_four_characters_are_valid() {
        assert!(is_valid_transaction_id("0"));
        assert!(is_valid_transaction_id("0a1b2c3d"));
        assert!(is_valid_transaction_id("f".repeat(64)));
    }

    #[test]
    fn anything_that_could_name_a_place_outside_the_folder_it_is_joined_to_is_refused() {
        assert!(!is_valid_transaction_id(""));
        assert!(!is_valid_transaction_id("a".repeat(65)));
        assert!(!is_valid_transaction_id("A1")); // upper case
        assert!(!is_valid_transaction_id("g1")); // not hexadecimal
        assert!(!is_valid_transaction_id(".."));
        assert!(!is_valid_transaction_id("a/b"));
        assert!(!is_valid_transaction_id("a\\b"));
        assert!(!is_valid_transaction_id("a b"));
        assert!(!is_valid_transaction_id("c:"));
        assert!(!is_valid_transaction_id("ab\0cd"));
    }
}

// ---- the automatic check's clock (state.rs) -------------------------------------------------------

const DAY: i64 = 86400; // 24 * 60 * 60
const LAST: i64 = 1_000_000;

fn state_checked(at: i64, automatic: bool) -> UpdateState {
    UpdateState {
        last_check_epoch: at,
        automatic,
        ..UpdateState::default()
    }
}

mod update_check_clock {
    use super::*;

    #[test]
    fn the_constants_are_a_day_and_two_hours() {
        assert_eq!(CHECK_INTERVAL, 86400);
        assert_eq!(CHECK_JITTER_MAX, 7200);
    }

    #[test]
    fn a_check_is_due_once_a_day_has_passed() {
        assert!(!check_is_due(&state_checked(LAST, true), LAST + DAY - 1, 0));
        assert!(check_is_due(&state_checked(LAST, true), LAST + DAY, 0));
        assert!(!check_is_due(&state_checked(LAST, true), LAST, 0));
    }

    #[test]
    fn the_jitter_delays_the_check_by_so_much() {
        // 24 h + 1 h = 90000 s.
        assert!(!check_is_due(
            &state_checked(LAST, true),
            LAST + 89999,
            3600
        ));
        assert!(check_is_due(&state_checked(LAST, true), LAST + 90000, 3600));
    }

    #[test]
    fn a_jitter_beyond_two_hours_is_clamped_and_a_negative_one_to_zero() {
        // Clamped to 7200: due at 86400 + 7200 = 93600, not at 86400 + 10000.
        assert!(!check_is_due(
            &state_checked(LAST, true),
            LAST + 93599,
            10000
        ));
        assert!(check_is_due(
            &state_checked(LAST, true),
            LAST + 93600,
            10000
        ));
        // Clamped to 0: due at 86400.
        assert!(check_is_due(&state_checked(LAST, true), LAST + DAY, -5));
    }

    #[test]
    fn a_check_that_never_happened_is_due_and_one_turned_off_is_not() {
        assert!(check_is_due(&state_checked(0, true), 5, 0));
        assert!(check_is_due(&state_checked(-1, true), 5, 0));
        // The person turned the automatic check off: never due, whatever the clock says.
        assert!(!check_is_due(&state_checked(0, false), LAST, 0));
        assert!(!check_is_due(
            &state_checked(LAST, false),
            LAST + 10 * DAY,
            0
        ));
    }

    #[test]
    fn a_last_check_in_the_future_does_not_silence_the_check() {
        // The clock was set back after the last check: a stored time that cannot be right.
        assert!(check_is_due(&state_checked(LAST + 1, true), LAST, 0));
    }
}

// ---- the command line as UTF-8 (process.rs) -------------------------------------------------------

mod update_arguments {
    use katana_update::process::utf8_arguments;
    #[cfg(windows)]
    use katana_update::process::utf8_from_wide;

    #[cfg(windows)]
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[cfg(windows)]
    #[test]
    fn an_accented_letter_is_its_two_byte_utf8_sequence() {
        // U+00E9 is 110_00011 10_101001 = C3 A9.
        assert_eq!(
            utf8_from_wide(&wide("Jos\u{e9}")).as_bytes(),
            b"Jos\xC3\xA9"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_character_of_the_basic_plane_is_its_three_byte_sequence() {
        // U+20AC (the euro sign) is 1110_0010 10_000010 10_101100 = E2 82 AC.
        assert_eq!(
            utf8_from_wide(&wide("\u{20ac}")).as_bytes(),
            b"\xE2\x82\xAC"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_surrogate_pair_is_one_four_byte_sequence() {
        // U+1F600 = D83D DE00 in UTF-16, and 11110_000 10_011111 10_011000 10_000000 =
        // F0 9F 98 80.
        assert_eq!(
            utf8_from_wide(&[0xD83D, 0xDE00]).as_bytes(),
            b"\xF0\x9F\x98\x80"
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_unpaired_surrogate_becomes_the_replacement_character() {
        // U+FFFD is EF BF BD. A high surrogate alone, a low one alone, and a high one followed by
        // an ordinary character (which must survive).
        assert_eq!(utf8_from_wide(&[0xD800]).as_bytes(), b"\xEF\xBF\xBD");
        assert_eq!(utf8_from_wide(&[0xDC00]).as_bytes(), b"\xEF\xBF\xBD");
        assert_eq!(
            utf8_from_wide(&[0xD800, u16::from(b'A')]).as_bytes(),
            b"\xEF\xBF\xBDA"
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_empty_and_an_ascii_string_convert_as_they_are() {
        assert_eq!(utf8_from_wide(&[]), "");
        assert_eq!(
            utf8_from_wide(&wide("--root C:\\Katana")),
            "--root C:\\Katana"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_running_programs_own_command_line_is_read() {
        // The command line is the operating system's, not argv. The C++'s test program is
        // katana_update_tests; cargo names a test program after its file, with a hash after it.
        let arguments = utf8_arguments();
        assert!(!arguments.is_empty());
        let program = katana_core::path_text::path_from_utf8(&arguments[0]);
        let stem = program
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        assert!(stem.starts_with("contract"), "{stem}");
    }

    #[cfg(not(windows))]
    #[test]
    fn the_arguments_are_returned_as_given() {
        // The C++ hands utf8Arguments an argv of its own; the port reads the program's own
        // arguments, which are what the standard library was given, byte for byte.
        use std::os::unix::ffi::OsStringExt;
        let given: Vec<Vec<u8>> = std::env::args_os().map(OsStringExt::into_vec).collect();
        let arguments = utf8_arguments();
        assert!(!arguments.is_empty());
        assert_eq!(arguments, given);
    }
}
