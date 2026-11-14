//! The twins of test_update_dialog.cpp's UpdateWords suite: the words and the numbers - a megabyte
//! a million bytes, a person's words for each reason token, the files of a progress line, the skip
//! line, and the echo of a line that has a token typed on it. The tokens are copied from the
//! engine's headers, as the C++ test copied them.

use katana_window::update_dialog::{
    UpdateDialogContext, UpdateDialogState, update_files_of_progress_line, update_logged_line,
    update_megabytes, update_reason_words, update_skip_line,
};

use super::support::{UpdateBench, view_of};

mod update_words {
    use super::*;

    #[test]
    fn megabytes_are_millions_of_bytes_rounded_to_the_nearest_and_never_none_for_a_download() {
        assert_eq!(update_megabytes(0), "0");
        assert_eq!(
            update_megabytes(1),
            "1",
            "a download is never said to be nothing"
        );
        assert_eq!(update_megabytes(499_999), "1");
        assert_eq!(update_megabytes(1_000_000), "1");
        assert_eq!(update_megabytes(1_499_999), "1");
        assert_eq!(update_megabytes(1_500_000), "2");
        assert_eq!(update_megabytes(71_000_000), "71");
        // The largest payload a manifest may name, 4 GiB: 4294.967296 MB.
        assert_eq!(update_megabytes(4_294_967_296), "4295");
    }

    #[test]
    fn a_reason_token_is_said_in_words_and_one_nobody_knows_as_it_came() {
        assert_eq!(
            update_reason_words("partial_downloads"),
            "this network does not allow partial downloads"
        );
        assert_eq!(
            update_reason_words("http_403"),
            "the release server refused the request (error 403)"
        );
        // Any other status the server answers with: the engine writes http_<n>.
        assert_eq!(
            update_reason_words("http_503"),
            "the release server answered with error 503"
        );
        // A token of a later engine is still said, as it came.
        assert_eq!(update_reason_words("something_new"), "something_new");
        assert_eq!(update_reason_words("http_5033"), "http_5033");
        assert!(update_reason_words("").is_empty());
    }

    // The tokens of katana_update's check (a check's `reason`), of its apply (ApplyResult::reason)
    // and the verbs' own, the network engine's, and the two refusals of a copy that may not
    // install: each must read as words, none as the token it is.
    #[test]
    fn every_reason_the_engine_gives_has_words_of_its_own() {
        for token in [
            "offline",
            "http_403",
            "http_404",
            "http_429",
            "bad_signature",
            "wrong_product",
            "wrong_target",
            "malformed",
            "untrusted_key",
            "placeholder_keys",
            "partial_downloads",
            "cancelled",
            "disabled_build",
            "development_tree",
            "root_not_writable",
            "headless",
            "skipped",
            "blocked_before",
            "no_transport",
            "no_store",
            "locked",
            "interrupted_update",
            "invalid_request",
            "install_record",
            "staged_manifest",
            "notify_only",
            "applier_too_new",
            "downgrade",
            "blocked",
            "scan_failed",
            "staged_missing",
            "staged_hash",
            "reparse_point",
            "path_conflict",
            "copy_failed",
            "space",
            "journal",
            "io_error",
            "transaction_exists",
            "rename_failed",
            "health_timeout",
            "health_exit",
            "rollback_failed",
            "internal_error",
            "html_page",
            "too_large",
            "wrong_channel",
            "asset_missing",
            "verify_unavailable",
            "bad_range",
            "bad_redirect",
            "too_many_redirects",
            "error",
            "staged_corrupt",
            "write_failed",
            "stage_busy",
            "low_disk_space",
            "short_range",
            "unverified_manifest",
            "no_token",
            "payload_mismatch",
            "helper_missing",
            "helper_not_started",
            "helper_timeout",
            "helper_failed",
            "platform",
            "elevated",
        ] {
            let words = update_reason_words(token);
            assert_ne!(words, token, "{token}");
            assert!(words.contains(' '), "{token}: {words}");
            assert!(!words.ends_with('.'), "{token} ends the sentence itself");
        }
    }

    #[test]
    fn a_copy_that_may_not_install_is_told_what_it_can_do_about_it() {
        // Started as administrator (the hardening pass's ruling H4): each of the three things is
        // said, in a person's words, and the token is not.
        let elevated = update_reason_words("elevated");
        assert_eq!(
            elevated,
            "Katana was started as administrator; start it the ordinary way, not as \
             administrator, and update again - the download is kept"
        );
        for part in [
            "start it the ordinary way",
            "not as administrator",
            "update again",
            "the download is kept",
        ] {
            assert!(elevated.contains(part), "{part}");
        }
        assert!(!elevated.contains("elevated"));

        // A copy on a platform this version installs nothing on.
        let platform = update_reason_words("platform");
        assert_eq!(
            platform,
            "this copy of Katana cannot install an update on this kind of computer; the release \
             notes say where to download it"
        );
        assert!(platform.contains("the release notes say where to download it"));

        // As the dialog says it: how the last apply ended, when the helper refused it for the
        // platform.
        let mut bench = UpdateBench::new(UpdateDialogContext::default());
        let mut view = view_of(UpdateDialogState::RolledBack);
        view.last_status = String::from("refused");
        view.reason = String::from("platform");
        bench.set_view(view);
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 could not be installed; nothing was changed. Reason: this copy of \
             Katana cannot install an update on this kind of computer; the release notes say \
             where to download it."
        );
    }

    #[test]
    fn an_earlier_update_that_is_still_open_is_said_to_be_unsettled_not_failed() {
        assert_eq!(
            update_reason_words("interrupted_update"),
            "an earlier update is not settled yet"
        );
        let mut bench = UpdateBench::new(UpdateDialogContext::default());
        let mut view = view_of(UpdateDialogState::RolledBack);
        view.last_status = String::from("refused");
        view.reason = String::from("interrupted_update");
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 could not be installed; nothing was changed. Reason: an earlier update \
             is not settled yet."
        );

        // With the command the owner gives it for this ending, the dialog says what to do, in the
        // order the helper's own `detail` gives the two steps. Nothing to press.
        view.recover_command = String::from("katana_updater --recover \"C:/Programs/Katana\"");
        bench.set_view(view.clone());
        assert_eq!(
            bench.shown("updateStateText"),
            "Katana 0.3.0 could not be installed; nothing was changed."
        );
        assert_eq!(
            bench.shown("updateDetailText"),
            "Reason: an earlier update is not settled yet. Update again; if this is said again, \
             close Katana and run: katana_updater --recover \"C:/Programs/Katana\""
        );
        assert!(bench.shown_buttons().is_empty());

        // The command is for that refusal alone: another one names none, whatever the view
        // carries.
        view.reason = String::from("space");
        bench.set_view(view);
        assert_eq!(
            bench.shown("updateDetailText"),
            "Reason: there is not enough free disk space."
        );
    }

    // The two a person meets most of the network engine's reasons: a sign-in page answering in the
    // place of the release server, and a download that does not fit.
    #[test]
    fn a_captive_portal_and_a_full_disk_are_said_in_words_not_as_their_tokens() {
        let mut bench = UpdateBench::new(UpdateDialogContext::default());
        let mut view = view_of(UpdateDialogState::CheckFailed);
        view.reason = String::from("html_page");
        bench.set_view(view);
        assert_eq!(
            bench.shown("updateStateText"),
            "Could not check for updates: a web page answered in the place of the release \
             server; this network may want you to sign in first."
        );
        assert_eq!(
            update_reason_words("low_disk_space"),
            "there is not enough free disk space for the download"
        );
        assert_eq!(
            update_reason_words("stage_busy"),
            "another Katana is downloading this update now"
        );
    }

    #[test]
    fn the_files_of_a_progress_line_are_read_wherever_the_word_stands() {
        for line in [
            "files=12/23",
            "progress bytes=31000000/71000000 files=12/23",
            "progress files=12/23 file=bin/a.dll",
            "files=12/23 staged=none",
        ] {
            assert_eq!(update_files_of_progress_line(line), Some((12, 23)), "{line}");
        }
        // Not a word of its own, not two numbers, or not there: the files are left unsaid.
        for line in [
            "progress profiles=12/23",
            "progress files=12",
            "progress files=a/b",
            "progress files=12/23x",
            "staged=0.3.0",
            "",
        ] {
            assert_eq!(update_files_of_progress_line(line), None, "{line}");
        }
    }

    #[test]
    fn the_skip_line_names_the_version() {
        assert_eq!(update_skip_line("0.3.0"), "UPDATE SKIP 0.3.0");
    }

    // The verbs have UPDATE TOKEN STATUS and UPDATE TOKEN CLEAR and no line that takes a token: a
    // typed line is echoed in the command log before any verb refuses it, so the echo hides every
    // other word after TOKEN.
    #[test]
    fn what_is_typed_where_a_token_was_hoped_to_go_is_never_echoed() {
        let token = "ghp_NotARealTokenOnlyATestsWord0123456789";
        assert_eq!(
            update_logged_line(&format!("UPDATE TOKEN {token}")),
            "UPDATE TOKEN ***"
        );
        assert_eq!(
            update_logged_line(&format!("update token SET {token}")),
            "update token *** ***"
        );
        assert_eq!(
            update_logged_line(&format!("  UPDATE   TOKEN CLEAR {token}")),
            "UPDATE TOKEN CLEAR ***"
        );
        // The two lines there are, and every other line, are echoed as typed.
        for line in [
            "UPDATE TOKEN STATUS",
            "update token clear",
            "UPDATE TOKEN",
            "UPDATE CHECK",
            "UPDATE SKIP 0.3.0",
            "UPDATE  SET AUTO  ON",
            "ONLINE KEY provider secret",
            "TOKEN one two",
            "LINE 0,0 10,0",
            "",
        ] {
            assert_eq!(update_logged_line(line), line, "{line}");
        }
    }
}
