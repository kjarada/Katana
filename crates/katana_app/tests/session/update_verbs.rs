//! tests/app/test_update_verbs.cpp: the UPDATE verbs (update_verbs.rs), each
//! line of the grammar run for real over a fake feed, a scratch install and a
//! scratch state folder (the rig and where its expectations come from:
//! tests/support/update_rig.rs). The cases that set KATANA_UPDATE_TOKEN or
//! KATANA_UPDATE_STATE_DIR, and katana_cli's session (which reads the
//! customisation variables), are in tests/session_process; the three of
//! katana_mcp's checkUpdateReply are the MCP server's (wave 2, C).

mod update_verbs {
    use std::sync::atomic::AtomicBool;

    use katana_app::update_verbs::{
        is_update_apply_line, is_update_line, run_update_line, update_help_text,
    };
    use katana_app::{Captured, Session, UpdateSurface};
    use katana_core::error::make_error;
    use katana_core::{Error, ErrorCode};
    use katana_update::check::CheckStatus;
    use katana_update::feed::fetch_fn;
    use katana_update::{Install, InstallKind, LastResult, UpdateState, parse_version};
    use katana_update::{read_state, write_last_result, write_state};

    use crate::update_rig::{
        AVAILABLE, FRESH_STATUS, MANIFEST_URL, NOTES, NOW, NOW_TEXT, Rig, SECRET, SIGNATURE_URL,
        V1_SEQUENCE, V2_SEQUENCE, bytes_of, first_line_of, fixtures, http_status, lines_of, must,
        put_a_complete_stage_of_v2_in, refused, said, tree_of, write_bytes,
    };

    fn development_install() -> Install {
        Install {
            version: parse_version("0.9.0").expect("0.9.0 is a version"),
            ..Install::default()
        }
    }

    // ---- the line -----------------------------------------------------------------------------

    #[test]
    fn a_line_is_an_update_line_by_its_first_word_in_any_case() {
        for line in [
            "UPDATE CHECK",
            "update status",
            "  Update  DOWNLOAD CONFIRM",
            "\tUPDATE",
            "UPDATE nonsense",
        ] {
            assert!(is_update_line(line), "{line}");
        }
        for line in [
            "",
            "   ",
            "UPDATES CHECK",
            "UPDATECHECK",
            "LINE 0,0 1,1",
            "HELP UPDATE",
            "# UPDATE CHECK",
            "CHECK UPDATE",
        ] {
            assert!(!is_update_line(line), "{line}");
        }
    }

    #[test]
    fn lines_the_grammar_does_not_take_are_invalid_arguments_and_do_nothing() {
        let rig = Rig::new();
        rig.serve_v2();
        let context = rig.cli();
        for line in [
            "UPDATE",
            "UPDATE NOW",
            "UPDATE CHECK NOW",
            "UPDATE STATUS JSON",
            "UPDATE DOWNLOAD CONFIRM NOW",
            "UPDATE DOWNLOAD YES",
            "UPDATE APPLY NOW",
            "UPDATE SKIP",
            "UPDATE SKIP 1",
            "UPDATE SKIP 1.2.3.4.5",
            "UPDATE SKIP v0.10.0",
            "UPDATE SKIP 0.10.0 0.11.0",
            "UPDATE SET",
            "UPDATE SET AUTO",
            "UPDATE SET AUTO MAYBE",
            "UPDATE SET SPEED ON",
            "UPDATE SET AUTO ON NOW",
            "UPDATE TOKEN",
            "UPDATE TOKEN STATUS NOW",
            "LINE 0,0 1,1",
        ] {
            let reply = run_update_line(&context, line);
            let error = match reply {
                Ok(reply) => panic!("{line} -> {}", reply.text),
                Err(error) => error,
            };
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
        }
        assert!(
            rig.feed.requests.borrow().is_empty(),
            "a line that is refused asks nobody anything"
        );
        assert!(!rig.state.exists(), "and keeps nothing");
        assert_eq!(rig.store.clears.get(), 0);
    }

    #[test]
    fn a_line_without_its_confirm_is_refused_saying_what_the_line_with_it_does() {
        let rig = Rig::new();
        rig.serve_v2();
        for surface in [UpdateSurface::Cli, UpdateSurface::Window] {
            let context = rig.context(surface);

            let download = refused(run_update_line(&context, "UPDATE DOWNLOAD"));
            assert_eq!(download.code, ErrorCode::InvalidArgument);
            assert!(download.message.contains("nothing was downloaded"));
            assert!(download.message.contains("UPDATE DOWNLOAD CONFIRM asks"));
            assert!(download.message.contains("downloads the files"));

            let apply = refused(run_update_line(&context, "update apply"));
            assert_eq!(apply.code, ErrorCode::InvalidArgument);
            assert!(apply.message.contains("nothing was changed"));
            assert!(
                apply
                    .message
                    .contains("UPDATE APPLY CONFIRM ends this session")
            );
            assert!(apply.message.contains("replace the installed files"));
        }
        assert!(rig.feed.requests.borrow().is_empty());
        assert!(rig.launcher.runs.borrow().is_empty());
        assert!(!rig.state.exists());
    }

    // ---- UPDATE CHECK -------------------------------------------------------------------------

    #[test]
    fn check_offers_a_newer_release_with_what_it_would_fetch_and_records_when_it_asked() {
        let rig = Rig::new();
        rig.serve_v2();
        let reply = must(run_update_line(&rig.cli(), "UPDATE CHECK"));
        assert_eq!(reply.text, AVAILABLE);

        let offer = reply.offer.as_ref().expect("an offer");
        assert_eq!(offer.check.status, CheckStatus::Available);
        assert_eq!(offer.check.installed_version, "0.9.0");
        assert_eq!(offer.check.latest_version, "0.10.0");
        assert_eq!(offer.changed_files, 5);
        assert_eq!(offer.total_files, 10);
        assert_eq!(offer.download_bytes, 339);
        assert!(reply.status.is_none());
        assert!(reply.apply.is_none());

        // The signature, then the manifest, and nothing of the payload: a check
        // downloads no file of the release.
        let requests = rig.feed.requests.borrow();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url, SIGNATURE_URL);
        assert_eq!(requests[1].url, MANIFEST_URL);

        let state = read_state(&rig.state).expect("the state reads");
        assert_eq!(state.last_check_epoch, NOW);
        assert!(!rig.stage_directory().exists());
    }

    #[test]
    fn check_is_asked_in_any_case_and_with_blanks_around() {
        let rig = Rig::new();
        rig.serve_v2();
        let reply = must(run_update_line(&rig.cli(), "  update   check "));
        assert_eq!(reply.text, AVAILABLE);
    }

    #[test]
    fn check_says_current_when_the_feed_holds_what_is_installed() {
        let mut rig = Rig::new();
        rig.serve_v2();
        rig.install_v2();
        let reply = must(run_update_line(&rig.cli(), "UPDATE CHECK"));
        // Nothing would be fetched, so nothing is said of files or bytes.
        assert_eq!(
            reply.text,
            format!("update=current installed=0.10.0 latest=0.10.0 notes_url={NOTES}")
        );
        let offer = reply.offer.as_ref().expect("an offer");
        assert_eq!(offer.check.status, CheckStatus::Current);
        assert_eq!(offer.changed_files, 0);

        // The feed answered, so this was a check all the same.
        let state = read_state(&rig.state).expect("the state reads");
        assert_eq!(state.last_check_epoch, NOW);
    }

    #[test]
    fn check_says_unreachable_with_the_transports_reason_and_records_no_check() {
        // Each as katana_update's feed says a transport reports it, with the
        // token it is read as (fetch_failure_reason) and its own sentence as
        // the detail. (what, error, the reply after "update=unreachable
        // installed=0.9.0 ").
        let failures: [(&str, Error, &str); 9] = [
            (
                "a private repository or no release (404)",
                http_status(ErrorCode::NotFound, 404),
                "reason=http_404 detail=\"the server said 404\"",
            ),
            (
                "the rate limit (403)",
                http_status(ErrorCode::FileImportFailure, 403),
                "reason=http_403 detail=\"the server said 403\"",
            ),
            (
                "too many requests (429)",
                http_status(ErrorCode::FileImportFailure, 429),
                "reason=http_429 detail=\"the server said 429\"",
            ),
            (
                "the host in trouble (503)",
                http_status(ErrorCode::FileImportFailure, 503),
                "reason=http_503 detail=\"the server said 503\"",
            ),
            (
                "no network",
                make_error(
                    ErrorCode::FileImportFailure,
                    "the host name could not be found",
                    "status=0; https://github.com/x",
                ),
                "reason=offline detail=\"the host name could not be found\"",
            ),
            (
                "a captive portal's page",
                make_error(
                    ErrorCode::FileImportFailure,
                    "a web page instead of the file",
                    "status=200; reason=html_page; https://github.com/x",
                ),
                "reason=html_page detail=\"a web page instead of the file\"",
            ),
            (
                "a redirect out of https",
                make_error(
                    ErrorCode::FileImportFailure,
                    "redirected to no https address",
                    "status=302; reason=bad_redirect; https://github.com/x",
                ),
                "reason=bad_redirect detail=\"redirected to no https address\"",
            ),
            (
                "a body past the engine's bound",
                make_error(ErrorCode::Unsupported, "longer than allowed", "status=200"),
                "reason=too_large detail=\"longer than allowed\"",
            ),
            (
                "a transfer that was cancelled",
                Error::new(ErrorCode::InvalidState, "cancelled"),
                "reason=cancelled detail=cancelled",
            ),
        ];
        for (what, error, expected) in failures {
            let rig = Rig::new();
            rig.serve_v2();
            rig.feed.fail(SIGNATURE_URL, error);
            let reply = run_update_line(&rig.cli(), "UPDATE CHECK");
            let reply = match reply {
                Ok(reply) => reply,
                Err(error) => panic!(
                    "{what}: a feed that cannot be read is a reply, {}",
                    error.describe()
                ),
            };
            assert_eq!(
                reply.text,
                format!("update=unreachable installed=0.9.0 {expected}"),
                "{what}"
            );
            let offer = reply.offer.as_ref().expect("an offer");
            assert_eq!(offer.check.status, CheckStatus::Unreachable, "{what}");
            assert_eq!(
                rig.feed.request_count(),
                1,
                "{what}: the signature gates the rest"
            );
            assert!(
                !rig.state.join("state.json").exists(),
                "{what}: nothing answered, so no check is recorded"
            );
        }
    }

    #[test]
    fn check_says_unreachable_when_the_signature_is_not_the_release_keys() {
        // tests/update/fixtures/release/bad/wrong-key: v2, signed by a key the
        // trust does not hold.
        let rig = Rig::new();
        let release = fixtures().join("release").join("bad").join("wrong-key");
        rig.feed.serve(
            SIGNATURE_URL,
            bytes_of(&release.join("Katana-update-win64.json.sig")),
        );
        rig.feed.serve(
            MANIFEST_URL,
            bytes_of(&release.join("Katana-update-win64.json")),
        );
        let reply = run_update_line(&rig.cli(), "UPDATE CHECK");
        assert!(reply.is_ok(), "{}", said(&reply));
        assert!(
            first_line_of(&reply)
                .starts_with("update=unreachable installed=0.9.0 reason=untrusted_key"),
            "{}",
            said(&reply)
        );
        assert!(
            !said(&reply).contains("latest="),
            "nothing of an unverified manifest is said"
        );
        assert!(!rig.state.join("state.json").exists());
    }

    #[test]
    fn check_with_no_transport_says_so_and_asks_nobody() {
        let rig = Rig::new();
        rig.serve_v2();
        let mut context = rig.cli();
        context.fetch = None;
        let reply = must(run_update_line(&context, "UPDATE CHECK"));
        assert!(
            reply
                .text
                .starts_with("update=unreachable installed=0.9.0 reason=no_transport detail="),
            "{}",
            reply.text
        );
        assert!(rig.feed.requests.borrow().is_empty());
        assert!(!rig.state.exists());
    }

    #[test]
    fn a_build_with_no_network_is_a_copy_that_cannot_update() {
        // What the transport of a build without an HTTP stack answers every
        // request with (the C++'s update_transport.hpp): Unsupported, and the
        // token no_network. No server can cure that, so the copy is
        // unsupported, not the feed unreachable.
        let rig = Rig::new();
        let mut context = rig.cli();
        context.fetch = fetch_fn(|_request, _stop, _progress| {
            Err(make_error(
                ErrorCode::Unsupported,
                "this build of Katana has no network access",
                "reason=no_network",
            ))
        });
        let reply = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(
            reply.text,
            "update=unsupported installed=0.9.0 reason=no_network detail=\"this build of Katana \
             has no network access\""
        );
        let offer = reply.offer.as_ref().expect("an offer");
        assert_eq!(offer.check.status, CheckStatus::Unsupported);
    }

    #[test]
    fn check_does_not_offer_a_release_that_was_rolled_back_here() {
        let rig = Rig::new();
        rig.serve_v2();
        let state = UpdateState {
            blocked_sequences: vec![V2_SEQUENCE],
            ..UpdateState::default()
        };
        write_state(&rig.state, &state).expect("the state is written");

        let reply = must(run_update_line(&rig.cli(), "UPDATE CHECK"));
        // One record: what the feed holds, why it is not offered, and - the
        // detail, which closes the record - the way back (update_verbs.rs).
        assert!(
            reply.text.starts_with(&format!(
                "update=blocked installed=0.9.0 latest=0.10.0 notes_url={NOTES} \
                 reason=blocked_before detail=\""
            )),
            "{}",
            reply.text
        );
        assert_eq!(lines_of(&reply.text).len(), 1, "{}", reply.text);
        let offer = reply.offer.as_ref().expect("an offer");
        assert_eq!(offer.check.status, CheckStatus::Blocked);
        assert_eq!(offer.check.reason, "blocked_before");
        assert!(
            offer.check.detail.contains("was put back"),
            "{}",
            offer.check.detail
        );
        assert!(
            offer.check.detail.contains(
                "UPDATE BLOCKED CLEAR (Settings > Updates in the window) offers it again"
            ),
            "{}",
            offer.check.detail
        );

        // Recording the check kept what the state already held.
        let after = read_state(&rig.state).expect("the state reads");
        assert_eq!(after.blocked_sequences, vec![V2_SEQUENCE]);
        assert_eq!(after.last_check_epoch, NOW);
    }

    #[test]
    fn check_does_not_offer_a_skipped_version_until_the_skip_is_taken_back() {
        let rig = Rig::new();
        rig.serve_v2();
        let context = rig.cli();

        let skipped = must(run_update_line(&context, "UPDATE SKIP 0.10.0"));
        assert_eq!(skipped.text, "skipped=0.10.0");

        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(
            checked.text,
            format!(
                "update=current installed=0.9.0 latest=0.10.0 notes_url={NOTES} reason=skipped"
            )
        );
        assert_eq!(
            checked.offer.as_ref().expect("an offer").check.status,
            CheckStatus::Current
        );

        let taken = must(run_update_line(&context, "update skip none"));
        assert_eq!(taken.text, "skipped=none");
        let again = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(again.text, AVAILABLE);
    }

    #[test]
    fn a_version_is_skipped_as_a_version_not_as_text() {
        // 0.10 and 0.10.0 are one release (katana_update's version).
        let rig = Rig::new();
        rig.serve_v2();
        let context = rig.cli();
        let skipped = must(run_update_line(&context, "UPDATE SKIP 0.10"));
        assert_eq!(skipped.text, "skipped=0.10");
        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert!(checked.text.contains("update=current"), "{}", checked.text);
        assert!(checked.text.contains("reason=skipped"), "{}", checked.text);

        // Another version skipped hides nothing of this one.
        assert!(run_update_line(&context, "UPDATE SKIP 0.11.0").is_ok());
        let other = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(other.text, AVAILABLE);
    }

    #[test]
    fn check_says_when_the_releases_information_is_old_and_offers_it_all_the_same() {
        // v2's `expires` is 2027-04-16T08:40:00Z. 1900000000 is in 2030.
        let rig = Rig::new();
        rig.serve_v2();
        let mut context = rig.cli();
        context.now_epoch = 1900000000;
        let reply = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(reply.text, format!("{AVAILABLE} information=old"));
    }

    #[test]
    fn a_notify_only_release_is_available_and_has_nothing_to_fetch() {
        // tests/update/fixtures/release/v2-macos-notify: 0.10.0 announced for a
        // target whose copies are replaced by hand. It lists no file.
        let mut rig = Rig::new();
        rig.install.target = String::from("macos-arm64");
        rig.feed_config.target = String::from("macos-arm64");
        let release = fixtures().join("release").join("v2-macos-notify");
        let base = "https://github.com/kjarada/Katana/releases/latest/download/";
        rig.feed.serve(
            &format!("{base}Katana-update-macos-arm64.json.sig"),
            bytes_of(&release.join("Katana-update-macos-arm64.json.sig")),
        );
        rig.feed.serve(
            &format!("{base}Katana-update-macos-arm64.json"),
            bytes_of(&release.join("Katana-update-macos-arm64.json")),
        );
        let reply = must(run_update_line(&rig.cli(), "UPDATE CHECK"));
        assert_eq!(
            reply.text,
            format!(
                "update=available installed=0.9.0 latest=0.10.0 notes_url={NOTES} \
                 reason=notify_only"
            )
        );
        let offer = reply.offer.as_ref().expect("an offer");
        assert_eq!(offer.check.status, CheckStatus::Available);
        assert_eq!(offer.total_files, 0);
    }

    #[test]
    fn a_development_copy_is_never_asked_about_and_keeps_nothing() {
        // A copy packaging did not write: no install.json, so no target and no
        // kind but development. The feed would offer v2; it is never asked.
        let mut rig = Rig::new();
        rig.serve_v2();
        rig.install = development_install();
        let context = rig.cli();

        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert!(
            checked
                .text
                .starts_with("update=unsupported installed=0.9.0 reason=development_tree detail="),
            "{}",
            checked.text
        );
        assert_eq!(
            checked.offer.as_ref().expect("an offer").check.status,
            CheckStatus::Unsupported
        );
        assert!(
            rig.feed.requests.borrow().is_empty(),
            "a build tree stays offline"
        );
        assert!(
            !rig.state.exists(),
            "and a check that asked nobody is not recorded"
        );

        let status = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(
            status.text,
            "installed=0.9.0 target=none kind=development staged=none skipped=none auto=on \
             last_check=never token=absent last=none"
        );
    }

    #[test]
    fn an_install_record_that_does_not_read_is_said_and_the_copy_is_not_updated() {
        let rig = Rig::new();
        rig.serve_v2();
        let mut context = rig.cli();
        context.install = development_install();
        context.install_problem = String::from("ParseFailure: install.json is not a record");
        let reply = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(
            reply.text,
            "update=unsupported installed=0.9.0 reason=install_record \
             detail=\"ParseFailure: install.json is not a record\""
        );
        assert!(rig.feed.requests.borrow().is_empty());

        // STATUS has no word in kind= for such a copy: its record says what the
        // context holds, and a warning after it says that this is not what the
        // copy is. Without it the copy was called a development build and no
        // more.
        let status = must(run_update_line(&context, "UPDATE STATUS"));
        let lines = lines_of(&status.text);
        assert_eq!(lines.len(), 2, "{}", status.text);
        assert_eq!(
            lines[0],
            "installed=0.9.0 target=none kind=development staged=none skipped=none auto=on \
             last_check=never token=absent last=none"
        );
        assert!(
            lines[1].starts_with("warning text=\"this copy's install record could not be read"),
            "{}",
            lines[1]
        );
        assert!(
            lines[1].contains("ParseFailure: install.json is not a record"),
            "{}",
            lines[1]
        );
        assert!(!rig.state.exists());
    }

    #[test]
    fn check_offers_a_release_whose_files_it_could_not_compare_and_says_that_it_could_not() {
        // katana_update's plan: a folder where a file of the release goes is an
        // error that names the file, not a change. The release is known all the
        // same; what it would fetch is not, so the record has no files= and no
        // download_bytes= (update_verbs.rs) and a warning says why.
        let rig = Rig::new();
        rig.serve_v2();
        let readme = rig.root.join("share").join("katana").join("readme.txt");
        std::fs::remove_file(&readme).expect("the readme is removed");
        std::fs::create_dir(&readme).expect("a folder takes its place");

        let reply = must(run_update_line(&rig.cli(), "UPDATE CHECK"));
        let lines = lines_of(&reply.text);
        assert_eq!(lines.len(), 2, "{}", reply.text);
        assert_eq!(
            lines[0],
            format!("update=available installed=0.9.0 latest=0.10.0 notes_url={NOTES}")
        );
        assert!(
            lines[1]
                .starts_with("warning text=\"what the update would change could not be worked out"),
            "{}",
            lines[1]
        );
        assert!(lines[1].contains("share/katana/readme.txt"), "{}", lines[1]);
        let offer = reply.offer.as_ref().expect("an offer");
        assert_eq!(offer.check.status, CheckStatus::Available);
        assert_eq!(offer.total_files, 0);
        assert_eq!(offer.download_bytes, 0);

        // The feed answered, so it was a check.
        let state = read_state(&rig.state).expect("the state reads");
        assert_eq!(state.last_check_epoch, NOW);
    }

    #[test]
    fn a_check_that_was_cancelled_before_it_began_is_an_error_and_asks_nobody() {
        // katana_update's check: the call's own error is for a check that could
        // not be made at all, and a stop already asked for is one; `cancelled`
        // is what the engine calls it everywhere (plan.rs, stage.rs).
        let rig = Rig::new();
        rig.serve_v2();
        let cancelled = AtomicBool::new(true);
        let mut context = rig.cli();
        context.stop = Some(&cancelled);

        let error = refused(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(error.message, "cancelled");
        assert!(rig.feed.requests.borrow().is_empty());
        assert!(
            !rig.state.exists(),
            "a check that asked nobody is not recorded"
        );

        // A status asks nobody, so there is nothing of it to cancel.
        let status = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(status.text, FRESH_STATUS);
    }

    #[test]
    fn a_state_file_that_does_not_read_is_said_and_is_not_written_over() {
        let rig = Rig::new();
        rig.serve_v2();
        let damaged = "this is not a state file";
        write_bytes(&rig.state.join("state.json"), damaged);
        let context = rig.cli();

        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        let lines = lines_of(&checked.text);
        assert_eq!(lines.len(), 2, "{}", checked.text);
        assert_eq!(
            lines[0], AVAILABLE,
            "the defaults are used: nothing is skipped or blocked"
        );
        assert!(
            lines[1].starts_with("warning text=\"the update settings could not be read"),
            "{}",
            lines[1]
        );
        assert_eq!(bytes_of(&rig.state.join("state.json")), damaged.as_bytes());

        let status = must(run_update_line(&context, "UPDATE STATUS"));
        let status_lines = lines_of(&status.text);
        assert_eq!(status_lines.len(), 2, "{}", status.text);
        assert_eq!(status_lines[0], FRESH_STATUS);
        assert!(status_lines[1].starts_with("warning text="));

        for line in ["UPDATE SKIP 0.10.0", "UPDATE SET AUTO OFF"] {
            let error = refused(run_update_line(&context, line));
            assert_eq!(error.code, ErrorCode::InvalidState, "{line}");
        }
        assert_eq!(bytes_of(&rig.state.join("state.json")), damaged.as_bytes());
    }

    // ---- UPDATE STATUS ------------------------------------------------------------------------

    #[test]
    fn status_says_what_this_copy_is_and_what_is_kept() {
        let rig = Rig::new();
        rig.serve_v2();
        let context = rig.cli();

        let fresh = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(fresh.text, FRESH_STATUS);
        let said_fresh = fresh.status.as_ref().expect("a status");
        assert_eq!(said_fresh.installed_version, "0.9.0");
        assert_eq!(said_fresh.target, "win64");
        assert_eq!(said_fresh.kind, InstallKind::Archive);
        assert!(said_fresh.staged_version.is_empty());
        assert!(said_fresh.state.automatic);
        assert!(!said_fresh.token_present);
        assert!(said_fresh.last.is_none());
        assert!(fresh.offer.is_none());
        assert!(
            rig.feed.requests.borrow().is_empty(),
            "a status asks nobody"
        );
        assert!(!rig.state.exists(), "and writes nothing");

        // Each thing kept, changed by its own line or by what writes it, and
        // read back.
        assert!(run_update_line(&context, "UPDATE CHECK").is_ok());
        assert!(run_update_line(&context, "UPDATE SKIP 0.10.0").is_ok());
        assert!(run_update_line(&context, "UPDATE SET AUTO OFF").is_ok());
        rig.store.set_token(SECRET);
        let last = LastResult {
            status: String::from("rolled_back"),
            reason: String::from("health_exit"),
            version: String::from("0.10.0"),
            ..LastResult::default()
        };
        write_last_result(&rig.state, &last).expect("the outcome is written");

        let kept = must(run_update_line(&context, "update status"));
        assert_eq!(
            kept.text,
            format!(
                "installed=0.9.0 target=win64 kind=archive staged=none skipped=0.10.0 auto=off \
                 last_check={NOW_TEXT} token=present last=rolled_back:health_exit"
            )
        );
        let said_kept = kept.status.as_ref().expect("a status");
        assert_eq!(said_kept.state.skipped_version, "0.10.0");
        assert!(!said_kept.state.automatic);
        assert_eq!(said_kept.state.last_check_epoch, NOW);
        assert!(said_kept.token_present);
        assert_eq!(
            said_kept.last.as_ref().expect("an outcome").status,
            "rolled_back"
        );

        // Reading how the last update ended does not clear it: the window shows
        // it once and clears it itself.
        assert!(rig.state.join("last.json").exists());
    }

    #[test]
    fn status_says_an_installer_kind_and_a_last_update_that_went_through() {
        let mut rig = Rig::new();
        rig.install.kind = InstallKind::Installer;
        let last = LastResult {
            status: String::from("committed"),
            version: String::from("0.9.0"),
            ..LastResult::default()
        };
        write_last_result(&rig.state, &last).expect("the outcome is written");
        let reply = must(run_update_line(&rig.cli(), "UPDATE STATUS"));
        assert_eq!(
            reply.text,
            "installed=0.9.0 target=win64 kind=installer staged=none skipped=none auto=on \
             last_check=never token=absent last=committed"
        );
    }

    #[test]
    fn what_is_downloaded_and_how_the_last_update_ended_are_an_installs_and_no_development_copys() {
        // One update folder, as one user has: the v2 release complete in it,
        // and the record of an update that was put back.
        let mut rig = Rig::new();
        put_a_complete_stage_of_v2_in(&rig.state);
        let last = LastResult {
            status: String::from("rolled_back"),
            reason: String::from("health_exit"),
            version: String::from("0.10.0"),
            ..LastResult::default()
        };
        write_last_result(&rig.state, &last).expect("the outcome is written");

        // The installed copy says both: the release is above what it has.
        let installed = must(run_update_line(&rig.cli(), "UPDATE STATUS"));
        assert_eq!(
            installed.text,
            "installed=0.9.0 target=win64 kind=archive staged=0.10.0 skipped=none auto=on \
             last_check=never token=absent last=rolled_back:health_exit"
        );

        // A development copy on the same machine shares the folder, and neither
        // is its own (update_verbs.rs): nothing is ever applied to it.
        rig.install = development_install();
        let development = must(run_update_line(&rig.cli(), "UPDATE STATUS"));
        assert_eq!(
            development.text,
            "installed=0.9.0 target=none kind=development staged=none skipped=none auto=on \
             last_check=never token=absent last=none"
        );
        let status = development.status.as_ref().expect("a status");
        assert!(status.staged_version.is_empty());
        assert!(status.last.is_none());
    }

    #[test]
    fn a_stage_is_ready_by_its_two_signed_records_and_by_nothing_else() {
        // What STATUS calls staged (update_verbs.rs): the folder
        // stage-<sequence> whose two records verify and name that sequence.
        // Each folder here lacks one of those things, and is passed over.
        let release = fixtures().join("release").join("v2");
        let manifest = bytes_of(&release.join("Katana-update-win64.json"));
        let signature = bytes_of(&release.join("Katana-update-win64.json.sig"));
        let rig = Rig::new();
        let manifest_in = |stage: &str| katana_update::staged_manifest_path(&rig.state.join(stage));
        let signature_in =
            |stage: &str| katana_update::staged_signature_path(&rig.state.join(stage));
        // No signature beside the manifest: a download that did not finish.
        write_bytes(
            &katana_update::staged_manifest_path(&rig.stage_directory()),
            &manifest,
        );
        // The records of v2 in a folder named for another release.
        write_bytes(&manifest_in("stage-1792312801"), &manifest);
        write_bytes(&signature_in("stage-1792312801"), &signature);
        // Names that are no stage-<sequence>, and a file where a folder would be.
        write_bytes(&manifest_in("stage-"), &manifest);
        write_bytes(&signature_in("stage-"), &signature);
        write_bytes(&manifest_in("stage-1792312800x"), manifest);
        write_bytes(&signature_in("stage-1792312800x"), &signature);
        write_bytes(&rig.state.join("stage-17"), "a file");

        let context = rig.cli();
        let none = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(none.text, FRESH_STATUS);

        // With its signature the first folder is the release, ready.
        write_bytes(
            &katana_update::staged_signature_path(&rig.stage_directory()),
            signature,
        );
        let ready = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(
            ready.status.as_ref().expect("a status").staged_version,
            "0.10.0"
        );
    }

    // ---- UPDATE SKIP, UPDATE SET AUTO ---------------------------------------------------------

    #[test]
    fn set_auto_is_kept_and_changing_one_setting_keeps_the_others() {
        let rig = Rig::new();
        let context = rig.cli();
        let off = must(run_update_line(&context, "update set auto off"));
        assert_eq!(off.text, "auto=off");
        assert!(run_update_line(&context, "UPDATE SKIP 0.10.0").is_ok());

        let state = read_state(&rig.state).expect("the state reads");
        assert!(!state.automatic, "the skip did not put the check back on");
        assert_eq!(state.skipped_version, "0.10.0");

        let on = must(run_update_line(&context, "UPDATE SET AUTO ON"));
        assert_eq!(on.text, "auto=on");
        let state = read_state(&rig.state).expect("the state reads");
        assert!(state.automatic);
        assert_eq!(
            state.skipped_version, "0.10.0",
            "and the switch kept the skip"
        );
    }

    #[test]
    fn with_no_state_folder_nothing_per_user_is_read_or_written() {
        // What a caller passes to keep nothing: an empty state folder. A state
        // file lies where the program's own would be, and is neither read nor
        // touched.
        let rig = Rig::new();
        rig.serve_v2();
        let state = UpdateState {
            skipped_version: String::from("0.10.0"),
            automatic: false,
            ..UpdateState::default()
        };
        write_state(&rig.state, &state).expect("the state is written");
        let before = tree_of(&rig.state);

        let mut context = rig.cli();
        context.state_directory.clear();

        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(
            checked.text, AVAILABLE,
            "the skip that is kept elsewhere was not read"
        );

        let status = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(
            status.text, FRESH_STATUS,
            "nor the switch, nor when the last check was"
        );

        // Nothing was carried out, and each reply gives the reason.
        let skipped = must(run_update_line(&context, "UPDATE SKIP 0.10.0"));
        assert_eq!(skipped.text, "skipped=none reason=no_state");
        let automatic = must(run_update_line(&context, "UPDATE SET AUTO OFF"));
        assert_eq!(automatic.text, "auto=on reason=no_state");

        assert_eq!(tree_of(&rig.state), before);
    }

    // ---- UPDATE BLOCKED CLEAR -----------------------------------------------------------------
    //
    // A release whose apply was put back here is in the state's `blocked` list
    // by its sequence (katana_update's state), which is what the helper
    // writes; the tests write it the same way. The reply and each refusal are
    // the ones the grammar gives (update_verbs.rs): blocked=none;
    // reason=no_state with no update folder; InvalidState for a state file
    // that does not read; InvalidArgument for any other word after BLOCKED.

    #[test]
    fn blocked_clear_empties_the_list_and_the_next_check_offers_the_release_again() {
        for surface in [UpdateSurface::Cli, UpdateSurface::Window] {
            let rig = Rig::new();
            rig.serve_v2();
            // Two releases put back here, v2 among them, beside what the person
            // keeps: another version skipped, the automatic check off, a check
            // made long ago.
            let state = UpdateState {
                blocked_sequences: vec![V1_SEQUENCE, V2_SEQUENCE],
                skipped_version: String::from("0.11.0"),
                automatic: false,
                last_check_epoch: 1700000000,
            };
            write_state(&rig.state, &state).expect("the state is written");
            let context = rig.context(surface);

            let refused_check = must(run_update_line(&context, "UPDATE CHECK"));
            assert!(
                refused_check.text.starts_with("update=blocked "),
                "{}",
                refused_check.text
            );

            let cleared = must(run_update_line(&context, "UPDATE BLOCKED CLEAR"));
            assert_eq!(cleared.text, "blocked=none");
            assert!(cleared.offer.is_none());
            assert!(cleared.status.is_none());
            assert!(cleared.apply.is_none());

            // The whole list is gone and nothing else of the state was touched.
            // (The check above was answered by the feed, so it recorded its
            // time.)
            let after = read_state(&rig.state).expect("the state reads");
            assert!(after.blocked_sequences.is_empty());
            assert_eq!(after.skipped_version, "0.11.0");
            assert!(!after.automatic);
            assert_eq!(after.last_check_epoch, NOW);

            let offered = must(run_update_line(&context, "UPDATE CHECK"));
            assert_eq!(offered.text, AVAILABLE);

            // With nothing blocked, and typed in any case: the same answer.
            let again = must(run_update_line(&context, "  update  blocked\tclear "));
            assert_eq!(again.text, "blocked=none");

            // The feed was asked by the two checks, a signature and a manifest
            // each, and by nothing else: the line itself asks nobody.
            assert_eq!(rig.feed.request_count(), 4);
            assert!(rig.launcher.runs.borrow().is_empty());
        }
    }

    #[test]
    fn update_blocked_takes_clear_and_no_other_word_and_says_the_one_line_there_is() {
        let rig = Rig::new();
        rig.serve_v2();
        let state = UpdateState {
            blocked_sequences: vec![V2_SEQUENCE],
            ..UpdateState::default()
        };
        write_state(&rig.state, &state).expect("the state is written");
        let before = tree_of(&rig.state);
        let context = rig.cli();

        for line in [
            "UPDATE BLOCKED",
            "UPDATE BLOCKED NONE",
            "UPDATE BLOCKED LIST",
            "UPDATE BLOCKED CLEAR NOW",
            "UPDATE BLOCKED 1792312800",
            "UPDATE BLOCKED CLEAR 1792312800",
            "update blocked clear all",
            "UPDATE BLOCKED CONFIRM",
        ] {
            let error = refused(run_update_line(&context, line));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(
                error
                    .message
                    .contains("UPDATE BLOCKED CLEAR is the one line"),
                "{line}: {}",
                error.message
            );
        }
        assert_eq!(
            tree_of(&rig.state),
            before,
            "a line that is refused clears nothing"
        );
        assert!(rig.feed.requests.borrow().is_empty());

        // The release is blocked as it was.
        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert!(
            checked.text.starts_with("update=blocked "),
            "{}",
            checked.text
        );
    }

    #[test]
    fn blocked_clear_with_no_update_folder_says_so_and_writes_no_file() {
        {
            // A caller that keeps nothing. A state file lies where the program's
            // own would be, with a release blocked in it: neither read nor
            // touched.
            let rig = Rig::new();
            let state = UpdateState {
                blocked_sequences: vec![V2_SEQUENCE],
                ..UpdateState::default()
            };
            write_state(&rig.state, &state).expect("the state is written");
            let before = tree_of(&rig.state);
            let mut context = rig.cli();
            context.state_directory.clear();

            let reply = must(run_update_line(&context, "UPDATE BLOCKED CLEAR"));
            assert_eq!(reply.text, "blocked=none reason=no_state");
            assert_eq!(tree_of(&rig.state), before);
        }
        {
            // And with no folder on the disk at all, none is made.
            let rig = Rig::new();
            let mut context = rig.cli();
            context.state_directory.clear();
            let reply = must(run_update_line(&context, "update blocked clear"));
            assert_eq!(reply.text, "blocked=none reason=no_state");
            assert!(!rig.state.exists());
        }
    }

    #[test]
    fn blocked_clear_is_refused_when_the_state_file_does_not_read_and_leaves_the_file_as_it_is() {
        // As SKIP and SET AUTO: a file that is damaged, or was written by a
        // newer Katana, holds what this line cannot keep, and is not written
        // over.
        let rig = Rig::new();
        rig.serve_v2();
        let damaged = "this is not a state file";
        write_bytes(&rig.state.join("state.json"), damaged);

        let error = refused(run_update_line(&rig.cli(), "UPDATE BLOCKED CLEAR"));
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(
            error.message.contains("could not be read"),
            "{}",
            error.message
        );
        assert_eq!(bytes_of(&rig.state.join("state.json")), damaged.as_bytes());
        assert!(rig.feed.requests.borrow().is_empty());
    }

    // ---- UPDATE TOKEN -------------------------------------------------------------------------

    #[test]
    fn token_status_says_present_or_absent_and_never_the_token() {
        let rig = Rig::new();
        let context = rig.cli();
        let absent = must(run_update_line(&context, "UPDATE TOKEN STATUS"));
        assert_eq!(absent.text, "token=absent");

        rig.store.set_token(SECRET);
        let present = must(run_update_line(&context, "update token status"));
        assert_eq!(
            present.text, "token=present",
            "the whole reply: no word of it is the token"
        );
    }

    #[test]
    fn token_status_says_when_the_store_cannot_say() {
        let rig = Rig::new();
        let context = rig.cli();
        // credential.rs: InvalidState is something stored that is no usable
        // token; Unsupported and Internal are a store that cannot be asked.
        rig.store.set_read_failure(make_error(
            ErrorCode::InvalidState,
            "the stored value holds a space",
            "test store",
        ));
        let unusable = must(run_update_line(&context, "UPDATE TOKEN STATUS"));
        assert_eq!(
            unusable.text,
            "token=absent reason=token_unusable detail=\"the stored value holds a space\""
        );

        rig.store.set_read_failure(Error::new(
            ErrorCode::Unsupported,
            "the store cannot be used in this session",
        ));
        let unavailable = must(run_update_line(&context, "UPDATE TOKEN STATUS"));
        assert_eq!(
            unavailable.text,
            "token=absent reason=store_unavailable detail=\"the store cannot be used in this \
             session\""
        );
    }

    #[test]
    fn with_no_store_the_token_lines_say_so() {
        let rig = Rig::new();
        let mut context = rig.cli();
        context.credentials = None;
        for line in ["UPDATE TOKEN STATUS", "UPDATE TOKEN CLEAR"] {
            let reply = run_update_line(&context, line);
            assert!(reply.is_ok(), "{line} -> {}", said(&reply));
            assert_eq!(said(&reply), "token=absent reason=no_store", "{line}");
        }
        // A token the context was handed is one the feed would send.
        context.feed.token = String::from(SECRET);
        let handed = must(run_update_line(&context, "UPDATE TOKEN STATUS"));
        assert_eq!(handed.text, "token=present reason=no_store");
    }

    #[test]
    fn token_clear_removes_the_stored_token() {
        let rig = Rig::new();
        rig.store.set_token(SECRET);
        let reply = must(run_update_line(&rig.cli(), "UPDATE TOKEN CLEAR"));
        assert_eq!(reply.text, "token=absent");
        assert_eq!(rig.store.clears.get(), 1);
        assert!(rig.store.token.borrow().is_empty());

        // Clearing when none is stored is no failure.
        let again = must(run_update_line(&rig.cli(), "update token clear"));
        assert_eq!(again.text, "token=absent");
    }

    #[test]
    fn token_clear_says_when_the_environment_still_gives_one() {
        // A store that is the environment variable: read-only, as credential.rs
        // says of it (clear is Unsupported), and still giving its token.
        let rig = Rig::new();
        rig.store.set_token(SECRET);
        rig.store.place.set("environment");
        rig.store.set_clear_failure(Error::new(
            ErrorCode::Unsupported,
            "the variable is not Katana's to change",
        ));
        let reply = must(run_update_line(&rig.cli(), "UPDATE TOKEN CLEAR"));
        assert!(
            reply
                .text
                .starts_with("token=present reason=environment detail="),
            "{}",
            reply.text
        );
        assert!(reply.text.contains("KATANA_UPDATE_TOKEN"), "{}", reply.text);
        assert!(!reply.text.contains(SECRET));

        // Any other failure of the store is the line's failure.
        rig.store
            .set_clear_failure(Error::new(ErrorCode::Internal, "the store failed (5)"));
        let failed = refused(run_update_line(&rig.cli(), "UPDATE TOKEN CLEAR"));
        assert_eq!(failed.code, ErrorCode::Internal);
        assert!(!failed.describe().contains(SECRET));
    }

    #[test]
    fn a_token_typed_on_a_line_is_refused_and_never_repeated() {
        let rig = Rig::new();
        for line in [
            "UPDATE TOKEN SET ghp_TYPED-ON-A-LINE",
            "UPDATE TOKEN ghp_TYPED-ON-A-LINE",
            "UPDATE TOKEN STATUS ghp_TYPED-ON-A-LINE",
        ] {
            let error = refused(run_update_line(&rig.cli(), line));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(!error.describe().contains("ghp_TYPED-ON-A-LINE"), "{line}");
            assert!(error.message.contains("KATANA_UPDATE_TOKEN"), "{line}");
        }
        assert!(
            rig.store.token.borrow().is_empty(),
            "no line stores a token"
        );
    }

    #[test]
    fn the_token_is_in_nothing_that_is_said_whatever_a_transport_or_a_store_says() {
        // A transport, and a store, that put the token in their own words -
        // which the engine's never do. What reaches a reply, an error or a
        // window's offer has it masked all the same.
        let mut rig = Rig::new();
        rig.serve_v2();
        rig.store.set_token(SECRET);
        rig.feed_config.token = String::from(SECRET);
        rig.feed.fail(
            SIGNATURE_URL,
            make_error(
                ErrorCode::FileImportFailure,
                format!("no answer from https://api.github.com/x?access_token={SECRET}"),
                format!("status=0; Bearer {SECRET}"),
            ),
        );
        let context = rig.cli();

        let checked = run_update_line(&context, "UPDATE CHECK");
        let checked = checked.unwrap_or_else(|_| panic!("(not shown: it may hold the token)"));
        assert!(!checked.text.contains(SECRET));
        assert!(
            checked.text.contains("access_token=***"),
            "masked, not dropped"
        );
        let offer = checked.offer.as_ref().expect("an offer");
        assert!(!offer.check.detail.contains(SECRET));
        assert!(!offer.check.reason.contains(SECRET));

        // A word of a line that is the token is not said back either.
        let typed = refused(run_update_line(&context, &format!("UPDATE {SECRET}")));
        assert!(!typed.describe().contains(SECRET));
        assert!(typed.message.contains("'***' is not an UPDATE action"));

        for line in [
            "UPDATE STATUS",
            "UPDATE TOKEN STATUS",
            "UPDATE SKIP 0.10.0",
            "UPDATE SET AUTO OFF",
            "UPDATE TOKEN CLEAR",
        ] {
            let reply = run_update_line(&context, line);
            assert!(!said(&reply).contains(SECRET), "{line}");
        }
    }

    // ---- what a surface may not run -----------------------------------------------------------

    #[test]
    fn a_headless_window_checks_and_reports_when_asked_and_keeps_nothing() {
        let rig = Rig::new();
        rig.serve_v2();
        // What the person keeps: a skip of this very release, and the check off.
        let state = UpdateState {
            skipped_version: String::from("0.10.0"),
            automatic: false,
            ..UpdateState::default()
        };
        write_state(&rig.state, &state).expect("the state is written");
        let before = tree_of(&rig.state);

        let context = rig.context(UpdateSurface::HeadlessWindow);
        assert!(context.state_directory.as_os_str().is_empty());

        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(checked.text, AVAILABLE, "the same answer on every machine");
        let status = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(status.text, FRESH_STATUS);
        assert_eq!(tree_of(&rig.state), before, "no per-user file was written");
    }

    #[test]
    fn a_headless_windows_download_and_apply_are_answered_with_the_reason_and_do_nothing() {
        let rig = Rig::new();
        rig.serve_v2();
        let context = rig.context(UpdateSurface::HeadlessWindow);

        let download = run_update_line(&context, "UPDATE DOWNLOAD CONFIRM");
        let download = download.unwrap_or_else(|error| {
            panic!("a reply, not a refusal of the line: {}", error.describe())
        });
        assert!(
            download
                .text
                .starts_with("staged=none reason=headless text="),
            "{}",
            download.text
        );
        assert!(
            download.text.contains("the Katana window or katana_cli"),
            "{}",
            download.text
        );

        let apply = must(run_update_line(&context, "UPDATE APPLY CONFIRM"));
        assert!(
            apply.text.starts_with("ready=none reason=headless text="),
            "{}",
            apply.text
        );
        assert!(
            apply.text.contains("the Katana window or katana_cli"),
            "{}",
            apply.text
        );
        assert!(apply.apply.is_none());

        assert!(
            rig.feed.requests.borrow().is_empty(),
            "nothing was asked of the feed"
        );
        assert!(rig.launcher.runs.borrow().is_empty());
        assert!(!rig.state.exists());
    }

    #[test]
    fn a_headless_window_is_refused_every_line_that_keeps_or_reads_a_per_user_thing() {
        let rig = Rig::new();
        rig.serve_v2();
        rig.store.set_token(SECRET);
        let context = rig.context(UpdateSurface::HeadlessWindow);
        for line in [
            "UPDATE SKIP 0.10.0",
            "UPDATE SKIP NONE",
            "UPDATE BLOCKED CLEAR",
            "UPDATE SET AUTO OFF",
            "UPDATE SET AUTO ON",
            "UPDATE TOKEN STATUS",
            "UPDATE TOKEN CLEAR",
        ] {
            let error = refused(run_update_line(&context, line));
            assert_eq!(error.code, ErrorCode::Unsupported, "{line}");
            assert_eq!(error.context, "reason=headless", "{line}");
            assert!(
                error
                    .message
                    .contains("the Katana window or katana_cli runs it"),
                "{line}: {}",
                error.message
            );
        }
        assert_eq!(rig.store.clears.get(), 0);
        assert_eq!(*rig.store.token.borrow(), SECRET);
        assert!(!rig.state.exists());
        assert!(rig.feed.requests.borrow().is_empty());
    }

    #[test]
    fn katana_mcps_surface_checks_and_reports_and_is_refused_everything_else() {
        let rig = Rig::new();
        rig.serve_v2();
        rig.store.set_token(SECRET);
        let context = rig.context(UpdateSurface::Mcp);
        assert!(context.state_directory.as_os_str().is_empty());

        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(checked.text, AVAILABLE);
        let status = must(run_update_line(&context, "UPDATE STATUS"));
        // The store this context was given holds a token, and that much is said.
        assert_eq!(
            status.text,
            "installed=0.9.0 target=win64 kind=archive staged=none skipped=none auto=on \
             last_check=never token=present last=none"
        );

        let asked = rig.feed.request_count();
        for line in [
            "UPDATE DOWNLOAD CONFIRM",
            "UPDATE APPLY CONFIRM",
            "UPDATE SKIP 0.10.0",
            "UPDATE SKIP NONE",
            "UPDATE BLOCKED CLEAR",
            "UPDATE SET AUTO OFF",
            "UPDATE TOKEN STATUS",
            "UPDATE TOKEN CLEAR",
        ] {
            let error = refused(run_update_line(&context, line));
            assert_eq!(error.code, ErrorCode::Unsupported, "{line}");
            assert_eq!(error.context, "reason=mcp", "{line}");
            assert!(error.message.contains("katana_check_update"), "{line}");
            assert!(
                error
                    .message
                    .contains("the Katana window or katana_cli runs it"),
                "{line}: {}",
                error.message
            );
        }
        assert_eq!(
            rig.feed.request_count(),
            asked,
            "a refused line asks the feed for nothing"
        );
        assert!(rig.launcher.runs.borrow().is_empty());
        assert_eq!(rig.store.clears.get(), 0);
        assert!(!rig.state.exists(), "an agent's check keeps nothing");
    }

    // ---- the help, and the session ------------------------------------------------------------

    #[test]
    fn the_help_names_every_line_of_the_grammar_and_is_in_the_sessions_help() {
        let help = update_help_text();
        for words in [
            "Update    UPDATE CHECK",
            "UPDATE STATUS",
            "UPDATE DOWNLOAD CONFIRM",
            "UPDATE APPLY CONFIRM",
            "UPDATE SKIP <version> | UPDATE SKIP NONE",
            "UPDATE BLOCKED CLEAR  offer again a release",
            "blocked=none",
            "started as administrator installs no update (reason=elevated)",
            "UPDATE SET AUTO ON|OFF",
            "UPDATE TOKEN STATUS | UPDATE TOKEN CLEAR",
            "update=available|current|unreachable|unsupported|blocked",
            "kind=installer|archive|development",
            "KATANA_UPDATE_TOKEN",
            "katana_check_update",
            "the last line of a katana_cli run",
        ] {
            assert!(help.contains(words), "{words}");
        }
        // The word a script fails a run on is in no help text.
        assert!(!help.contains("error"));
        assert!(Session::help_text().contains(help));
    }

    #[test]
    fn help_update_typed_in_a_session_is_the_update_verbs_own_block() {
        let mut session = Session::new(None, &mut Captured::default());
        for line in ["HELP UPDATE", "help  update", "? UPDATE"] {
            let mut captured = Captured::default();
            assert!(session.run(line, &mut captured), "{line}");
            assert_eq!(captured.out, update_help_text(), "{line}");
            assert!(captured.err.is_empty(), "{line}");
        }
    }

    #[test]
    fn a_session_of_no_program_refuses_update_lines_and_leaves_nothing_to_apply() {
        // The session a test makes is no program's (session.rs): there is no
        // install for UPDATE to act on, and no user whose settings to read.
        let mut session = Session::new(None, &mut Captured::default());
        for line in ["UPDATE STATUS", "UPDATE CHECK", "update apply confirm"] {
            let mut captured = Captured::default();
            assert!(!session.run(line, &mut captured), "{line}");
            assert!(
                captured
                    .err
                    .contains("error: Unsupported: UPDATE acts on the program"),
                "{}",
                captured.err
            );
            assert!(captured.out.is_empty(), "{}", captured.out);
        }
        assert!(session.take_pending_apply().is_none());
    }

    #[test]
    fn a_session_told_it_is_katana_mcps_refuses_every_update_line_naming_the_tool() {
        let mut session = Session::new(None, &mut Captured::default());
        session.set_update_surface(UpdateSurface::Mcp);
        for line in [
            "UPDATE CHECK",
            "UPDATE STATUS",
            "update download confirm",
            "UPDATE APPLY CONFIRM",
            "UPDATE BLOCKED CLEAR",
            "UPDATE",
        ] {
            let mut captured = Captured::default();
            assert!(!session.run(line, &mut captured), "{line}");
            assert!(
                captured
                    .err
                    .contains("error: Unsupported: UPDATE lines are not run by katana_mcp"),
                "{}",
                captured.err
            );
            assert!(
                captured.err.contains("katana_check_update"),
                "{}",
                captured.err
            );
        }
        assert!(session.take_pending_apply().is_none());
    }

    #[test]
    fn the_line_a_front_end_must_run_last_is_update_apply() {
        for line in [
            "UPDATE APPLY CONFIRM",
            "update apply confirm",
            "  UPDATE\tAPPLY",
            "UPDATE APPLY NOW",
        ] {
            assert!(Session::is_update_apply(line), "{line}");
            assert!(is_update_apply_line(line), "{line}");
        }
        for line in [
            "UPDATE CHECK",
            "UPDATE DOWNLOAD CONFIRM",
            "UPDATE APPLYING",
            "APPLY",
            "HELP UPDATE",
            "",
            "# UPDATE APPLY CONFIRM",
        ] {
            assert!(!Session::is_update_apply(line), "{line}");
            assert!(!is_update_apply_line(line), "{line}");
        }
    }

    #[test]
    fn the_last_line_rule_reads_a_line_as_the_verbs_do() {
        // A carriage return or a line feed parts two words for the verbs, as a
        // blank does, and a -c argument can hold one. Such a line IS an apply
        // to the verbs, so it is one to the rule that holds an apply to the
        // last line: read otherwise, the lines after it would be run by a
        // session that had already prepared the swap.
        let rig = Rig::new();
        let headless = rig.context(UpdateSurface::HeadlessWindow);
        for line in [
            "UPDATE\rAPPLY CONFIRM",
            "UPDATE\nAPPLY CONFIRM",
            "update \r\n apply\rconfirm",
            "UPDATE\tAPPLY\rCONFIRM",
        ] {
            // What the verbs make of it: UPDATE APPLY CONFIRM, answered here as
            // a headless window answers that line on every system.
            let reply = must(run_update_line(&headless, line));
            assert!(
                reply.text.starts_with("ready=none reason=headless"),
                "{}",
                reply.text
            );
            assert!(Session::is_update_apply(line));
            assert!(is_update_apply_line(line));
        }
    }

    // ---- UPDATE DOWNLOAD CONFIRM and UPDATE APPLY CONFIRM -------------------------------------

    #[cfg(not(windows))]
    #[test]
    fn download_and_apply_are_refused_where_this_version_installs_no_update() {
        // The swap is written for Windows only so far: elsewhere a release is
        // told of (UPDATE CHECK) and no more, and the two lines say so at once.
        let rig = Rig::new();
        rig.serve_v2();
        for line in ["UPDATE DOWNLOAD CONFIRM", "UPDATE APPLY CONFIRM"] {
            let error = refused(run_update_line(&rig.cli(), line));
            assert_eq!(error.code, ErrorCode::Unsupported, "{line}");
            assert_eq!(error.context, "reason=platform", "{line}");
            assert!(error.message.contains("UPDATE CHECK"), "{}", error.message);
        }
        assert!(rig.feed.requests.borrow().is_empty());
        assert!(rig.launcher.runs.borrow().is_empty());
        assert!(!rig.state.exists());
    }

    // The cases of the C++'s Windows branch: the download and the apply, which
    // the system's file system is written for there alone. In a module of the
    // suite's own name, so that the parity checker reads them as the suite's.
    // Written, not compiled until step 8.
    #[cfg(windows)]
    mod update_verbs {
        use std::cell::RefCell;
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::sync::atomic::AtomicBool;

        use katana_app::abandon_apply;
        use katana_app::update_verbs::run_update_line;
        use katana_core::ErrorCode;
        use katana_core::error::make_error;
        use katana_update::check::CheckStatus;
        use katana_update::{
            ApplyRequest, RetryPolicy, UpdateState, helper_arguments, is_valid_transaction_id,
            launch_apply_with, make_system_fs_ops, process_cleanup_list, read_state,
            staged_manifest_path, staged_signature_path, write_state,
        };

        use super::development_install;
        use crate::update_rig::{
            AVAILABLE, CHANGED, KEPT, NOTES, NOW_TEXT, PAYLOAD_SIZE, PAYLOAD_URL, Rig, SECRET,
            SIGNATURE_URL, V2_SEQUENCE, bytes_of, fixtures, http_status, lines_of,
            listed_for_the_sweep, must, put_a_complete_stage_of_v2_in, put_a_helper_in, refused,
            run_folders_in, tree_of, write_bytes,
        };

        #[test]
        fn download_stages_the_changed_files_in_one_request_and_says_so() {
            let rig = Rig::new();
            rig.serve_v2();
            let installed = tree_of(&rig.root);
            let spoken: RefCell<Vec<String>> = RefCell::new(Vec::new());
            let speak: &dyn Fn(&str) = &|line: &str| spoken.borrow_mut().push(String::from(line));
            let counted: RefCell<Vec<(u64, u64)>> = RefCell::new(Vec::new());
            let count: &dyn Fn(u64, u64) =
                &|done: u64, total: u64| counted.borrow_mut().push((done, total));
            let mut context = rig.cli();
            context.progress_line = Some(speak);
            context.progress = Some(count);

            let reply = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));

            // Progress lines, then what is staged.
            let lines = lines_of(&reply.text);
            assert!(lines.len() >= 2, "{}", reply.text);
            assert_eq!(lines[lines.len() - 1], "staged=0.10.0");
            let progress = &lines[..lines.len() - 1];
            for line in progress {
                assert!(line.starts_with("progress bytes="), "{line}");
                assert!(line.contains("/2949 files="), "{line}");
            }
            // One request for [44, 2993), and all five files there at the end.
            assert_eq!(
                progress[progress.len() - 1],
                "progress bytes=2949/2949 files=5/5"
            );
            assert_eq!(
                *spoken.borrow(),
                progress,
                "each line was said as it happened, and is in the reply"
            );
            let counted = counted.borrow();
            assert!(!counted.is_empty());
            assert_eq!(counted[counted.len() - 1], (2949, 2949));

            // It asked the feed first, as CHECK does, so what a window shows of
            // the release is in this reply too (update_verbs.rs).
            let offer = reply.offer.as_ref().expect("an offer");
            assert_eq!(offer.check.status, CheckStatus::Available);
            assert_eq!(offer.check.latest_version, "0.10.0");
            assert_eq!(offer.changed_files, 5);
            assert_eq!(offer.total_files, 10);
            assert_eq!(offer.download_bytes, 339);
            assert!(reply.apply.is_none());

            let payload = rig.feed.requests_for(PAYLOAD_URL);
            assert_eq!(payload.len(), 1);
            assert_eq!(payload[0].range_start, 44);
            assert_eq!(payload[0].range_length, 2949);
            assert_eq!(
                payload[0].max_bytes, 2949,
                "a server that ignores Range cannot send more"
            );
            assert_eq!(payload[0].payload_size, PAYLOAD_SIZE);

            // The staged files are v2's, byte for byte; what did not change is
            // not there.
            let stage = rig.stage_directory();
            for path in CHANGED {
                assert_eq!(
                    bytes_of(&stage.join(path)),
                    bytes_of(&fixtures().join("install").join("v2").join(path)),
                    "{path}"
                );
            }
            for path in KEPT {
                assert!(!stage.join(path).exists(), "{path}");
            }
            // And the two records that say the stage is complete: the signed
            // bytes.
            let release = fixtures().join("release").join("v2");
            assert_eq!(
                bytes_of(&staged_manifest_path(&stage)),
                bytes_of(&release.join("Katana-update-win64.json"))
            );
            assert_eq!(
                bytes_of(&staged_signature_path(&stage)),
                bytes_of(&release.join("Katana-update-win64.json.sig"))
            );

            assert_eq!(
                tree_of(&rig.root),
                installed,
                "a download changes nothing in the install"
            );

            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert_eq!(
                status.text,
                format!(
                    "installed=0.9.0 target=win64 kind=archive staged=0.10.0 skipped=none \
                     auto=on last_check={NOW_TEXT} token=absent last=none"
                )
            );
            assert_eq!(
                status.status.as_ref().expect("a status").staged_version,
                "0.10.0"
            );
        }

        #[test]
        fn a_second_download_fetches_only_what_is_missing() {
            let rig = Rig::new();
            rig.serve_v2();
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());

            // One staged file lost (a download cut short leaves the same): the
            // next run asks for that member alone - news.txt, 54 bytes at 725 -
            // and counts the other four, 85 + 70 + 75 + 55 = 285 bytes, as
            // already fetched.
            let stage = rig.stage_directory();
            std::fs::remove_file(stage.join("share").join("katana").join("news.txt"))
                .expect("the staged news.txt is removed");
            rig.feed.clear_requests();
            let resumed = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            let payload = rig.feed.requests_for(PAYLOAD_URL);
            assert_eq!(payload.len(), 1);
            assert_eq!(payload[0].range_start, 725);
            assert_eq!(payload[0].range_length, 54);
            let lines = lines_of(&resumed.text);
            assert!(lines.len() >= 2);
            assert_eq!(lines[0], "progress bytes=285/339 files=4/5");
            assert_eq!(lines[lines.len() - 2], "progress bytes=339/339 files=5/5");
            assert_eq!(lines[lines.len() - 1], "staged=0.10.0");
            assert_eq!(
                bytes_of(&stage.join("share").join("katana").join("news.txt")),
                bytes_of(
                    &fixtures()
                        .join("install")
                        .join("v2")
                        .join("share")
                        .join("katana")
                        .join("news.txt")
                )
            );

            // With nothing missing, nothing of the payload is asked for at all.
            rig.feed.clear_requests();
            let again = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            assert!(rig.feed.requests_for(PAYLOAD_URL).is_empty());
            assert_eq!(
                again.text,
                "progress bytes=339/339 files=5/5\nstaged=0.10.0"
            );
        }

        #[test]
        fn a_download_that_fails_is_an_error_with_its_reason_and_leaves_nothing_ready() {
            let rig = Rig::new();
            rig.serve_v2();
            rig.feed
                .fail(PAYLOAD_URL, http_status(ErrorCode::FileImportFailure, 503));
            let context = rig.cli();
            let error = refused(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            assert_eq!(error.message, "the server said 503");
            assert!(
                error.context.starts_with("reason=http_503; "),
                "{}",
                error.context
            );

            assert!(!staged_manifest_path(&rig.stage_directory()).exists());
            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("staged=none"), "{}", status.text);

            // Nothing is ready, so there is nothing to apply.
            let apply = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(apply.context, "reason=not_staged");
        }

        #[test]
        fn a_download_with_a_token_in_its_failure_says_none_of_it() {
            let mut rig = Rig::new();
            rig.serve_v2();
            rig.feed_config.token = String::from(SECRET);
            rig.feed.fail(
                PAYLOAD_URL,
                make_error(
                    ErrorCode::FileImportFailure,
                    format!("refused: Authorization: Bearer {SECRET}"),
                    format!("status=403; https://h/p?token={SECRET}"),
                ),
            );
            let spoken: RefCell<Vec<String>> = RefCell::new(Vec::new());
            let speak: &dyn Fn(&str) = &|line: &str| spoken.borrow_mut().push(String::from(line));
            let mut context = rig.cli();
            context.progress_line = Some(speak);
            let error = refused(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            assert!(!error.describe().contains(SECRET));
            assert!(error.message.contains("Bearer ***"));
            assert!(error.context.starts_with("reason=http_403; "));
            for line in spoken.borrow().iter() {
                assert!(!line.contains(SECRET), "a progress line");
            }
        }

        #[test]
        fn download_with_nothing_newer_says_why_and_fetches_nothing() {
            {
                let mut rig = Rig::new();
                rig.serve_v2();
                rig.install_v2();
                let reply = must(run_update_line(&rig.cli(), "UPDATE DOWNLOAD CONFIRM"));
                assert_eq!(
                    reply.text,
                    format!(
                        "update=current installed=0.10.0 latest=0.10.0 notes_url={NOTES}\n\
                         staged=none"
                    )
                );
                assert!(rig.feed.requests_for(PAYLOAD_URL).is_empty());
                assert!(!rig.stage_directory().exists());
            }
            {
                let rig = Rig::new();
                rig.serve_v2();
                rig.feed.fail(
                    SIGNATURE_URL,
                    http_status(ErrorCode::FileImportFailure, 429),
                );
                let reply = must(run_update_line(&rig.cli(), "UPDATE DOWNLOAD CONFIRM"));
                assert_eq!(
                    reply.text,
                    "update=unreachable installed=0.9.0 reason=http_429 \
                     detail=\"the server said 429\"\nstaged=none"
                );
                assert!(!rig.stage_directory().exists());
            }
            {
                // Skipped: what CHECK does not offer, DOWNLOAD does not fetch.
                let rig = Rig::new();
                rig.serve_v2();
                let context = rig.cli();
                assert!(run_update_line(&context, "UPDATE SKIP 0.10.0").is_ok());
                let reply = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
                assert_eq!(
                    reply.text,
                    format!(
                        "update=current installed=0.9.0 latest=0.10.0 notes_url={NOTES} \
                         reason=skipped\nstaged=none"
                    )
                );
                assert!(rig.feed.requests_for(PAYLOAD_URL).is_empty());
            }
        }

        #[test]
        fn nothing_is_downloaded_to_or_applied_to_a_development_copy() {
            let mut rig = Rig::new();
            rig.serve_v2();
            rig.install = development_install();
            let context = rig.cli();

            let download = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            let lines = lines_of(&download.text);
            assert_eq!(lines.len(), 2, "{}", download.text);
            assert!(
                lines[0].starts_with("update=unsupported installed=0.9.0 reason=development_tree")
            );
            assert_eq!(lines[1], "staged=none");

            let apply = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(apply.code, ErrorCode::Unsupported);
            assert_eq!(apply.context, "reason=development_tree");

            assert!(rig.feed.requests.borrow().is_empty());
            assert!(rig.launcher.runs.borrow().is_empty());
            assert!(!rig.state.exists());
        }

        #[test]
        fn with_no_state_folder_there_is_nowhere_to_download_to_and_nothing_to_apply() {
            let rig = Rig::new();
            rig.serve_v2();
            let mut context = rig.cli();
            context.state_directory.clear();

            let download = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            assert_eq!(download.text, "staged=none reason=no_state");
            assert!(
                rig.feed.requests.borrow().is_empty(),
                "nothing is asked for that could not be kept"
            );

            let apply = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(apply.context, "reason=no_state");
            assert!(!rig.state.exists());
        }

        #[test]
        fn apply_without_a_downloaded_release_is_refused() {
            let rig = Rig::new();
            rig.serve_v2();
            let error = refused(run_update_line(&rig.cli(), "UPDATE APPLY CONFIRM"));
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert_eq!(error.context, "reason=not_staged");
            assert!(error.message.contains("UPDATE DOWNLOAD CONFIRM"));
            assert!(
                rig.launcher.runs.borrow().is_empty(),
                "no helper was copied or started"
            );
            assert!(
                rig.feed.requests.borrow().is_empty(),
                "and an apply asks the feed for nothing"
            );
        }

        #[test]
        fn apply_prepares_the_helper_and_hands_back_the_request_changing_nothing_in_the_install() {
            let rig = Rig::new();
            rig.serve_v2();
            let installed_helper = put_a_helper_in(&rig.root);
            let mut context = rig.cli();
            context.pid = 4242;
            context.relaunch = rig.root.join("bin").join("katana.exe");
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            let installed = tree_of(&rig.root);
            rig.feed.clear_requests();

            let reply = must(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            let request = reply.apply.as_ref().expect("a request");
            assert_eq!(request.root, rig.root);
            assert_eq!(request.stage_directory, rig.stage_directory());
            assert_eq!(request.state_directory, rig.state);
            assert!(
                is_valid_transaction_id(&request.transaction),
                "{}",
                request.transaction
            );
            assert_eq!(request.wait_pid, 4242);
            assert_eq!(request.relaunch, rig.root.join("bin").join("katana.exe"));
            assert_eq!(
                reply.text,
                format!("ready=0.10.0 transaction={}", request.transaction)
            );

            // The helper that will run the swap is a copy outside the install,
            // and it was asked to prove it starts - by the launcher the context
            // gave.
            assert_eq!(
                request.helper,
                rig.state
                    .join(format!("run-{}", request.transaction))
                    .join(installed_helper.file_name().expect("the helper has a name"))
            );
            assert!(
                bytes_of(&request.helper) == bytes_of(&installed_helper),
                "the copy is the install's helper, byte for byte"
            );
            {
                let runs = rig.launcher.runs.borrow();
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].executable, request.helper);
                assert_eq!(runs[0].arguments, vec![String::from("--selftest")]);
            }

            // The line itself starts nothing and changes nothing: the caller
            // ends its session first, and launches then.
            assert!(rig.launcher.detached.borrow().is_empty());
            assert_eq!(tree_of(&rig.root), installed);
            assert!(!rig.root.join(".update").exists());
            assert!(
                rig.feed.requests.borrow().is_empty(),
                "an apply asks the feed for nothing"
            );

            // What the caller then does with the request: the helper copy,
            // started detached with the request as its command line
            // (katana_update's apply).
            assert!(launch_apply_with(request, &rig.launcher).is_ok());
            let detached = rig.launcher.detached.borrow();
            assert_eq!(detached.len(), 1);
            assert_eq!(detached[0].executable, request.helper);
            assert_eq!(detached[0].arguments, helper_arguments(request));
        }

        #[test]
        fn a_request_says_who_is_waited_for_and_what_is_started_again_as_its_context_did() {
            // A context that names nobody to wait for and nothing to start
            // again makes a request that says so to the helper (katana_update's
            // apply).
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            let reply = must(run_update_line(&context, "update apply confirm"));
            let request = reply.apply.as_ref().expect("a request");
            assert!(request.relaunch.as_os_str().is_empty());
            assert_eq!(request.wait_pid, 0);
        }

        #[test]
        fn an_apply_nobody_launches_has_its_helper_copy_listed_for_the_sweep() {
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            let reply = must(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            let request = reply.apply.as_ref().expect("a request");
            let run_folder = rig.state.join(format!("run-{}", request.transaction));
            assert_eq!(request.helper.parent(), Some(run_folder.as_path()));
            assert!(request.helper.exists());
            // A request that may yet be launched is listed nowhere.
            assert!(listed_for_the_sweep(&rig.state).is_empty());

            // Its caller will not launch it: the copy is listed
            // (katana_update's state), not removed here, and the release stays
            // downloaded.
            assert!(abandon_apply(request).is_ok());
            assert_eq!(listed_for_the_sweep(&rig.state), vec![run_folder.clone()]);
            assert!(request.helper.exists());
            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("staged=0.10.0"), "{}", status.text);

            // And the line is one the sweep of the next start takes for its own
            // (katana_update's state): the folder goes - or, should a scanner
            // be holding the new copy open at this moment, it stays listed for
            // the start after. A line the sweep did not take for its own would
            // be dropped, with the folder still there.
            let files = make_system_fs_ops(&RetryPolicy::default());
            assert!(process_cleanup_list(&rig.state, &rig.root, files.as_ref()).is_ok());
            let left = listed_for_the_sweep(&rig.state);
            assert!(
                !run_folder.exists() || left == vec![run_folder.clone()],
                "the folder is still there and is no longer listed"
            );
            assert!(staged_manifest_path(&rig.stage_directory()).exists());

            // A request that is none - nothing was prepared for it - lists
            // nothing.
            assert!(abandon_apply(&ApplyRequest::default()).is_ok());
            assert_eq!(listed_for_the_sweep(&rig.state), left);
        }

        #[test]
        fn apply_is_refused_when_the_state_file_does_not_read_for_it_is_where_what_was_put_back_is_kept()
         {
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            // Damaged, or written by a newer Katana: either way the list of the
            // releases that were put back here cannot be read out of it.
            let damaged = "this is not a state file";
            write_bytes(&rig.state.join("state.json"), damaged);

            let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert_eq!(error.context, "reason=state_unreadable");
            assert!(error.message.contains("Katana 0.10.0"), "{}", error.message);
            assert!(
                rig.launcher.runs.borrow().is_empty(),
                "no helper was copied or started"
            );
            assert_eq!(
                bytes_of(&rig.state.join("state.json")),
                damaged.as_bytes(),
                "and the file is left as it is"
            );

            // With the file gone the defaults are the truth again - nothing was
            // put back here - and the release is applied.
            std::fs::remove_file(rig.state.join("state.json")).expect("the state file goes");
            let again = must(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert!(again.apply.is_some());
        }

        #[test]
        fn a_folder_whose_name_is_no_unicode_text_in_the_update_folder_is_passed_over() {
            // What a Windows file system lets a name be and UTF-8 cannot say: a
            // surrogate with no partner, U+D800 alone. Turning such a name into
            // text fails, and the update folder is the user's to write: with
            // one such folder in it, STATUS and APPLY ended in an exception
            // that nothing caught.
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            let mut name: Vec<u16> = "stage-".encode_utf16().collect();
            name.push(0xD800);
            if let Err(error) = std::fs::create_dir(rig.state.join(OsString::from_wide(&name))) {
                eprintln!("this file system holds no such name: {error}");
                return;
            }

            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("staged=0.10.0"), "{}", status.text);
            let apply = must(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert!(
                apply.text.starts_with("ready=0.10.0 transaction="),
                "{}",
                apply.text
            );
        }

        #[test]
        fn nothing_is_downloaded_to_or_applied_to_a_copy_whose_install_record_does_not_read() {
            let rig = Rig::new();
            rig.serve_v2();
            let mut context = rig.cli();
            context.install_problem = String::from("ParseFailure: install.json is not a record");

            let download = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            assert_eq!(
                download.text,
                "update=unsupported installed=0.9.0 reason=install_record \
                 detail=\"ParseFailure: install.json is not a record\"\nstaged=none"
            );

            let apply = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(apply.code, ErrorCode::Unsupported);
            assert_eq!(apply.context, "reason=install_record");
            assert!(
                apply
                    .message
                    .contains("ParseFailure: install.json is not a record"),
                "{}",
                apply.message
            );

            assert!(rig.feed.requests.borrow().is_empty());
            assert!(rig.launcher.runs.borrow().is_empty());
            assert!(!rig.state.exists());
        }

        #[test]
        fn a_download_that_cannot_compare_the_installed_files_fetches_nothing_and_says_why() {
            // As for CHECK above: a folder where a file of the release goes.
            // What to fetch cannot be worked out, so nothing is.
            let rig = Rig::new();
            rig.serve_v2();
            let readme = rig.root.join("share").join("katana").join("readme.txt");
            std::fs::remove_file(&readme).expect("the readme is removed");
            std::fs::create_dir(&readme).expect("a folder takes its place");

            let error = refused(run_update_line(&rig.cli(), "UPDATE DOWNLOAD CONFIRM"));
            assert!(
                error.context.starts_with("reason=plan_failed"),
                "{}",
                error.context
            );
            assert!(
                error.message.starts_with(
                    "what the update would change could not be worked out, so nothing was \
                     downloaded: "
                ),
                "{}",
                error.message
            );
            assert!(
                error.context.contains("share/katana/readme.txt"),
                "{}",
                error.context
            );
            assert!(rig.feed.requests_for(PAYLOAD_URL).is_empty());
            assert!(!rig.stage_directory().exists());
        }

        #[test]
        fn a_download_that_was_cancelled_before_it_began_is_an_error_and_fetches_nothing() {
            let rig = Rig::new();
            rig.serve_v2();
            let cancelled = AtomicBool::new(true);
            let mut context = rig.cli();
            context.stop = Some(&cancelled);
            let error = refused(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert_eq!(error.message, "cancelled");
            assert!(rig.feed.requests.borrow().is_empty());
            assert!(!rig.state.exists());
        }

        #[test]
        fn apply_says_why_when_the_helper_does_not_prove_it_starts() {
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            rig.launcher.exit_code.set(3);
            let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            // katana_update's stage, the preflight's tokens.
            assert!(
                error.context.starts_with("reason=helper_failed"),
                "{}",
                error.context
            );
            assert!(rig.launcher.detached.borrow().is_empty());

            // And with no helper in the install at all.
            let bare = Rig::new();
            bare.serve_v2();
            let bare_context = bare.cli();
            assert!(run_update_line(&bare_context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            let missing = refused(run_update_line(&bare_context, "UPDATE APPLY CONFIRM"));
            assert!(
                missing.context.starts_with("reason=helper_missing"),
                "{}",
                missing.context
            );
            assert!(bare.launcher.runs.borrow().is_empty());
        }

        #[test]
        fn apply_refuses_a_release_that_was_rolled_back_here() {
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            // What the helper records when the new version did not start and
            // the old one was put back (katana_update's state, `blocked`).
            let mut state = read_state(&rig.state).expect("the state reads");
            state.blocked_sequences.push(V2_SEQUENCE);
            write_state(&rig.state, &state).expect("the state is written");

            let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(error.context, "reason=blocked_before");
            assert!(error.message.contains("Katana 0.10.0"));
            assert!(rig.launcher.runs.borrow().is_empty());
        }

        #[test]
        fn after_blocked_clear_apply_confirm_returns_the_request_for_the_release_that_was_put_back()
        {
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            // What the helper records when the new version did not start and
            // the old one was put back (katana_update's state, `blocked`).
            let mut state = read_state(&rig.state).expect("the state reads");
            state.blocked_sequences.push(V2_SEQUENCE);
            write_state(&rig.state, &state).expect("the state is written");

            // Refused, and the refusal names the way back - the line, and where
            // the window has it.
            let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert_eq!(error.context, "reason=blocked_before");
            assert!(
                error.message.contains(
                    "UPDATE BLOCKED CLEAR (Settings > Updates in the window) offers this one \
                     again"
                ),
                "{}",
                error.message
            );
            assert!(rig.launcher.runs.borrow().is_empty());

            // A download of it is what CHECK says and nothing staged anew: the
            // release is not offered, so it is not fetched.
            rig.feed.clear_requests();
            let not_fetched = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            let record = lines_of(&not_fetched.text);
            assert_eq!(record.len(), 2, "{}", not_fetched.text);
            assert!(record[0].starts_with("update=blocked "), "{}", record[0]);
            assert_eq!(record[1], "staged=none");
            assert!(rig.feed.requests_for(PAYLOAD_URL).is_empty());

            let cleared = must(run_update_line(&context, "UPDATE BLOCKED CLEAR"));
            assert_eq!(cleared.text, "blocked=none");

            // The release that was downloaded before is still there, and is
            // applied: the line hands back the request, and asks the feed for
            // nothing.
            rig.feed.clear_requests();
            let applied = must(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            let request = applied.apply.as_ref().expect("a request");
            assert_eq!(
                applied.text,
                format!("ready=0.10.0 transaction={}", request.transaction)
            );
            assert_eq!(request.root, rig.root);
            assert_eq!(request.stage_directory, rig.stage_directory());
            assert_eq!(request.state_directory, rig.state);
            assert_eq!(rig.launcher.runs.borrow().len(), 1);
            assert!(rig.feed.requests.borrow().is_empty());
        }

        // ---- a Katana that was started as administrator ---------------------------------------
        //
        // UpdateContext::elevated is what make_update_context sets for the
        // elevated half of a split token (katana_update's process). These
        // tests set the member, so they are the same tests however the suite
        // itself was started. That make_update_context fills it from the
        // system is seen only by a suite that itself holds that half
        // (the_context_of_a_program_in_no_install_is_a_development_copys says
        // why).

        #[test]
        fn apply_is_refused_in_a_katana_started_as_administrator_before_any_helper_is_copied() {
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let mut context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            let kept = tree_of(&rig.state);
            let installed = tree_of(&rig.root);
            rig.feed.clear_requests();

            context.elevated = true;
            let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(error.code, ErrorCode::Unsupported);
            assert_eq!(error.context, "reason=elevated");
            // What to do about it, and that the download is not lost.
            assert!(
                error.message.contains("started as administrator"),
                "{}",
                error.message
            );
            assert!(
                error
                    .message
                    .contains("Start Katana the ordinary way and update again"),
                "{}",
                error.message
            );
            assert!(
                error.message.contains("Katana 0.10.0 stays downloaded"),
                "{}",
                error.message
            );

            // Refused BEFORE the preflight: the launcher was asked for nothing -
            // no self-test run, nothing started - and no helper copy was made,
            // so there is no run folder and nothing to list for a sweep. The
            // update folder is file for file what the download left.
            assert_eq!(rig.launcher.runs.borrow().len(), 0);
            assert_eq!(rig.launcher.detached.borrow().len(), 0);
            assert!(run_folders_in(&rig.state).is_empty());
            assert_eq!(tree_of(&rig.state), kept);
            assert_eq!(tree_of(&rig.root), installed);
            assert!(rig.feed.requests.borrow().is_empty());

            // The release is still staged ...
            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("staged=0.10.0"), "{}", status.text);
            assert!(staged_manifest_path(&rig.stage_directory()).exists());

            // ... and Katana started the ordinary way applies it, without a
            // download.
            context.elevated = false;
            let applied = must(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            let request = applied.apply.as_ref().expect("a request");
            assert_eq!(rig.launcher.runs.borrow().len(), 1);
            assert_eq!(
                run_folders_in(&rig.state),
                vec![rig.state.join(format!("run-{}", request.transaction))]
            );
            assert!(rig.feed.requests.borrow().is_empty());
        }

        #[test]
        fn a_katana_started_as_administrator_still_checks_and_downloads() {
            // Only the apply starts the helper, so only the apply is refused:
            // what is downloaded here is applied by Katana started the ordinary
            // way.
            let rig = Rig::new();
            rig.serve_v2();
            let mut context = rig.cli();
            context.elevated = true;

            let checked = must(run_update_line(&context, "UPDATE CHECK"));
            assert_eq!(checked.text, AVAILABLE);

            let downloaded = must(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM"));
            let lines = lines_of(&downloaded.text);
            assert!(!lines.is_empty());
            assert_eq!(lines[lines.len() - 1], "staged=0.10.0");
            assert!(staged_manifest_path(&rig.stage_directory()).exists());
        }

        #[test]
        fn the_other_refusals_of_apply_are_said_before_the_one_for_an_administrators_session() {
            // A line that would not be carried out in any session says why not,
            // and not how this one was started: a build tree's own reason is
            // the same from an administrator's prompt, and there is nothing to
            // start Katana again for when no release is downloaded or the one
            // that is was put back.
            {
                // A development copy.
                let mut rig = Rig::new();
                rig.install = development_install();
                let mut context = rig.cli();
                context.elevated = true;
                let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
                assert_eq!(error.context, "reason=development_tree");
            }
            {
                // A copy whose install record does not read.
                let rig = Rig::new();
                let mut context = rig.cli();
                context.install_problem =
                    String::from("ParseFailure: install.json is not a record");
                context.elevated = true;
                let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
                assert_eq!(error.context, "reason=install_record");
            }
            {
                // No update folder.
                let rig = Rig::new();
                let mut context = rig.cli();
                context.state_directory.clear();
                context.elevated = true;
                let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
                assert_eq!(error.context, "reason=no_state");
            }
            {
                // Nothing downloaded.
                let rig = Rig::new();
                let mut context = rig.cli();
                context.elevated = true;
                let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
                assert_eq!(error.context, "reason=not_staged");
            }
            {
                // Downloaded, and put back here before.
                let rig = Rig::new();
                put_a_complete_stage_of_v2_in(&rig.state);
                let state = UpdateState {
                    blocked_sequences: vec![V2_SEQUENCE],
                    ..UpdateState::default()
                };
                write_state(&rig.state, &state).expect("the state is written");
                let mut context = rig.cli();
                context.elevated = true;
                let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
                assert_eq!(error.context, "reason=blocked_before");
            }
            {
                // Downloaded, and the state file does not read.
                let rig = Rig::new();
                put_a_complete_stage_of_v2_in(&rig.state);
                write_bytes(&rig.state.join("state.json"), "this is not a state file");
                let mut context = rig.cli();
                context.elevated = true;
                let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
                assert_eq!(error.context, "reason=state_unreadable");
            }
            {
                // And with a release ready and nothing else in the way, it is
                // this one.
                let rig = Rig::new();
                put_a_complete_stage_of_v2_in(&rig.state);
                let mut context = rig.cli();
                context.elevated = true;
                let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
                assert_eq!(error.context, "reason=elevated");
                assert_eq!(rig.launcher.runs.borrow().len(), 0);
                assert!(run_folders_in(&rig.state).is_empty());
            }
        }

        #[test]
        fn a_stage_that_is_not_above_what_is_installed_is_not_ready_to_apply() {
            // What a finished update can leave behind: the stage of the release
            // that is now installed. It is not offered as staged, and is not
            // applied.
            let mut rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            assert!(run_update_line(&rig.cli(), "UPDATE DOWNLOAD CONFIRM").is_ok());
            rig.install_v2();
            let context = rig.cli();
            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("installed=0.10.0"), "{}", status.text);
            assert!(status.text.contains("staged=none"), "{}", status.text);
            let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(error.context, "reason=not_staged");
        }

        #[test]
        fn a_stage_whose_manifest_was_changed_is_not_ready_to_apply() {
            // The stage folder is the user's to write. One byte of the staged
            // manifest changed, and the signature no longer covers it.
            let rig = Rig::new();
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let context = rig.cli();
            assert!(run_update_line(&context, "UPDATE DOWNLOAD CONFIRM").is_ok());
            let manifest = staged_manifest_path(&rig.stage_directory());
            let mut bytes = bytes_of(&manifest);
            let at = bytes
                .windows(6)
                .position(|window| window == b"0.10.0")
                .expect("the manifest names 0.10.0");
            bytes[at + 2] = b'9'; // 0.90.0
            write_bytes(&manifest, bytes);

            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("staged=none"), "{}", status.text);
            let error = refused(run_update_line(&context, "UPDATE APPLY CONFIRM"));
            assert_eq!(error.context, "reason=not_staged");
            assert!(rig.launcher.runs.borrow().is_empty());
        }
    }
}
