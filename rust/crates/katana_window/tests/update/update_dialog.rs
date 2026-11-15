//! The twins of test_update_dialog.cpp's UpdateDialog suite: every state shows exactly its
//! sentences and its buttons, and every button builds exactly its line or calls exactly its
//! callback. The sentences are the C++ test's, written out there by hand from the update system's
//! design (section 7, "The three surfaces", Window); 31 of 71 MB is 0.4366 of the bar's thousand
//! steps, so 437.

use katana_window::theme;
use katana_window::update_dialog::{UpdateButton, UpdateDialogContext, UpdateDialogState};

use super::support::{
    DialogFixture, UpdateBench, available, downloading, refused, view_of,
};

/// The names, in the order of the alphabet.
fn names(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|name| String::from(*name)).collect();
    names.sort();
    names
}

mod update_dialog {
    use super::*;

    // ---- what it is -----------------------------------------------------------------------------

    #[test]
    fn it_is_a_non_modal_dialog_with_a_name_for_every_control() {
        let f = DialogFixture::new();
        let bench = UpdateBench::new(f.context());
        assert_eq!(bench.dialog.object_name(), "updateDialog");
        assert_eq!(bench.dialog.window_title(), "Check for Updates");
        assert!(!bench.dialog.is_modal());

        for name in ["updateStateText", "updateDetailText", "updateProblemText"] {
            assert!(bench.record(name).is_some(), "{name}");
        }
        assert!(bench.record("updateProgress").is_some());
        // The nine buttons and the words on them, as the design names them.
        let buttons = [
            ("updateCheckNow", "Check now"),
            ("updateInstall", "Update"),
            ("updateDownloadOnly", "Download only"),
            ("updateSkip", "Skip this version"),
            ("updateNotes", "Release notes"),
            ("updateLater", "Later"),
            ("updateCancel", "Cancel"),
            ("updateRestart", "Restart and update"),
            ("updateRetry", "Retry"),
        ];
        for (name, words) in buttons {
            let button = bench.control(name);
            assert_eq!(button.text, words, "{name}");
            // The tooltip is the button's own (egui keeps a tooltip on the response it was
            // drawn with, not in the record).
            let tip = UpdateButton::of_name(name).expect("a button of the dialog").tip();
            assert!(!tip.is_empty(), "{name} says what it does");
        }
        assert_eq!(bench.buttons().len(), 9, "and no other button");
    }

    // egui has no default button: Enter in the dialog presses nothing, whatever is shown.
    #[test]
    fn no_button_is_a_default_so_enter_presses_nothing() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(view_of(UpdateDialogState::Ready));
        assert!(!bench.buttons().is_empty());
        bench
            .harness
            .key(egui::Key::Enter, egui::Modifiers::NONE);
        bench.draw();
        assert!(f.called().is_empty(), "{}", f.called());
        assert!(bench.dialog.is_visible());
    }

    // ---- every state shows exactly its sentences and its buttons -------------------------------

    #[test]
    fn before_anything_is_known_it_says_so_and_offers_a_check() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        // As made, and with the installed version given.
        assert_eq!(bench.said(), "Katana Not checked for updates yet.");
        bench.set_view(view_of(UpdateDialogState::Idle));
        assert_eq!(bench.shown("updateStateText"), "Katana 0.2.0");
        assert_eq!(
            bench.shown("updateDetailText"),
            "Not checked for updates yet."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateCheckNow"]));
        assert!(!bench.visible("updateProgress"));
        assert!(!bench.visible("updateProblemText"));
    }

    #[test]
    fn while_checking_it_says_so_and_offers_only_cancel() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(view_of(UpdateDialogState::Checking));
        assert_eq!(
            bench.said(),
            "Checking for updates... You can keep working."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateCancel"]));
        // Busy: a check cannot say how far it is.
        let (shown, range, _) = bench.progress();
        assert!(shown);
        assert_eq!(range, (0, 0));
    }

    #[test]
    fn current_says_the_version_is_up_to_date_and_when_it_was_checked() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::Current);
        view.last_checked = String::from("14:02");
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Katana 0.2.0 is up to date. Last checked 14:02."
        );
        assert_eq!(
            bench.shown("updateStateText"),
            "Katana 0.2.0 is up to date."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateCheckNow"]));

        // With no time to say, the sentence about it is left out, not left empty.
        view.last_checked.clear();
        bench.set_view(view);
        assert_eq!(bench.said(), "Katana 0.2.0 is up to date.");
    }

    #[test]
    fn available_says_what_changed_and_how_much_there_is_to_download() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(available());
        assert_eq!(bench.shown("updateStateText"), "Update Available");
        assert_eq!(
            bench.shown("updateDetailText"),
            "Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to download."
        );
        assert_eq!(
            bench.shown_buttons(),
            names(&[
                "updateDownloadOnly",
                "updateInstall",
                "updateLater",
                "updateNotes",
                "updateSkip"
            ])
        );
        assert!(!bench.visible("updateProgress"));
        for name in [
            "updateDownloadOnly",
            "updateInstall",
            "updateLater",
            "updateNotes",
            "updateSkip",
        ] {
            assert!(bench.enabled(name), "{name}");
        }
    }

    // The manifest's `expires` is advisory (design section 1): the dialog says the information is
    // old, and refuses nothing.
    #[test]
    fn old_update_information_is_said_and_the_update_is_still_offered() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = available();
        view.information_is_old = true;
        bench.set_view(view);
        assert_eq!(
            bench.shown("updateDetailText"),
            "Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to download. The \
             update information is old: a newer release may exist."
        );
        assert!(
            bench
                .shown_buttons()
                .contains(&String::from("updateInstall"))
        );
        assert!(bench.enabled("updateInstall"));
    }

    // Check and notify (design section 6): nothing here can install it, so neither button that
    // would is offered.
    #[test]
    fn a_copy_that_cannot_update_itself_is_told_of_the_release_and_sent_to_its_notes() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = available();
        view.notify_only = true;
        bench.set_view(view);
        assert_eq!(bench.shown("updateStateText"), "Update Available");
        assert_eq!(
            bench.shown("updateDetailText"),
            "Katana 0.3.0 (you have 0.2.0). This copy cannot update itself: the release notes \
             say where to download it."
        );
        assert_eq!(
            bench.shown_buttons(),
            names(&["updateLater", "updateNotes", "updateSkip"])
        );
    }

    #[test]
    fn downloading_says_how_far_it_is_and_that_work_can_go_on() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(downloading());
        assert_eq!(
            bench.said(),
            "Downloading 31 of 71 MB (12 of 23 files). You can keep working."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateCancel"]));
        let (shown, range, value) = bench.progress();
        assert!(shown);
        assert_eq!(range, (0, 1000));
        // 31 of 71 is 0.4366: 437 of the bar's thousand steps.
        assert_eq!(value, Some(437));
    }

    #[test]
    fn a_download_that_does_not_say_its_files_is_shown_in_bytes_alone() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = downloading();
        view.downloaded_files = None;
        bench.set_view(view.clone());
        assert_eq!(bench.shown("updateStateText"), "Downloading 31 of 71 MB.");

        // Nothing has arrived yet; and all of it has, which is never said as more than the whole,
        // nor the files as more than there are.
        view.downloaded_bytes = 0;
        view.downloaded_files = Some(0);
        bench.set_view(view.clone());
        assert_eq!(
            bench.shown("updateStateText"),
            "Downloading 0 of 71 MB (0 of 23 files)."
        );
        assert_eq!(bench.progress().2, Some(0));
        view.downloaded_bytes = 80_000_000;
        view.downloaded_files = Some(30);
        bench.set_view(view.clone());
        assert_eq!(
            bench.shown("updateStateText"),
            "Downloading 71 of 71 MB (23 of 23 files)."
        );
        assert_eq!(bench.progress().2, Some(1000));

        // A download whose size nobody gave has no measure: the bar is busy.
        view.download_bytes = 0;
        view.downloaded_bytes = 0;
        bench.set_view(view);
        assert_eq!(bench.progress().1.1, 0);
    }

    #[test]
    fn ready_says_katana_will_close_update_and_reopen() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(view_of(UpdateDialogState::Ready));
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 is ready. Katana will close, update and reopen in a few seconds."
        );
        assert_eq!(
            bench.shown_buttons(),
            names(&["updateLater", "updateRestart"])
        );
        assert!(bench.enabled("updateRestart"));
    }

    // "It waits while a job runs" (design section 7): the press is made, and only Later, which
    // takes it back, is left to press.
    #[test]
    fn while_the_window_waits_for_a_job_restart_is_off_and_the_detail_says_what_it_waits_for() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::Ready);
        view.waiting_for_jobs = true;
        bench.set_view(view);
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 is ready. Katana will close when the running job has finished, then \
             update and reopen."
        );
        assert_eq!(
            bench.shown_buttons(),
            names(&["updateLater", "updateRestart"])
        );
        assert!(!bench.enabled("updateRestart"));
        assert!(bench.enabled("updateLater"));
    }

    // The close comes from a timer, not from a press: under a modal dialog the person opened while
    // the release was downloading it would close the program around that dialog, so it waits for
    // it - and says so.
    #[test]
    fn while_the_window_waits_for_an_open_dialog_it_says_that_and_restart_is_off() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::Ready);
        view.waiting_for_dialog = true;
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 is ready. Katana will close when the dialog that is open has been \
             closed, then update and reopen."
        );
        assert_eq!(
            bench.shown_buttons(),
            names(&["updateLater", "updateRestart"])
        );
        assert!(!bench.enabled("updateRestart"));
        assert!(bench.enabled("updateLater"));

        // A job is what it waits for first, when there are both.
        view.waiting_for_jobs = true;
        bench.set_view(view);
        assert_eq!(
            bench.shown("updateDetailText"),
            "Katana will close when the running job has finished, then update and reopen."
        );
    }

    #[test]
    fn blocked_names_what_to_close_and_offers_retry() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::Blocked);
        view.holders = String::from("katana_mcp.exe (4412)");
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Close these first: katana_mcp.exe (4412). Then press Retry."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateRetry"]));

        // A record that names nobody still says what to do.
        view.holders.clear();
        bench.set_view(view);
        assert_eq!(
            bench.said(),
            "Close the other Katana programs that are open. Then press Retry."
        );
    }

    #[test]
    fn rolled_back_says_what_was_restored_and_why() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::RolledBack);
        view.last_status = String::from("rolled_back");
        view.reason = String::from("health_exit");
        bench.set_view(view);
        // The design's sentence, and after it - the hardening pass's ruling H5 of 2026-10-14 -
        // where the release that was put back is offered again.
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 could not be installed, so 0.2.0 was restored. Reason: the new version \
             did not start. A later release will be offered, and Settings > Updates offers this \
             one again."
        );
        assert_eq!(
            bench.shown("updateStateText"),
            "Katana 0.3.0 could not be installed, so 0.2.0 was restored."
        );
        // It is read and closed: nothing here tries the same release again.
        assert!(
            bench.shown_buttons().is_empty(),
            "{:?}",
            bench.shown_buttons()
        );
    }

    // Which endings set a release aside is the helper's rule: a rollback whose reason is
    // health_exit or health_timeout, and no other ending.
    #[test]
    fn only_a_release_that_was_put_back_for_not_starting_is_said_to_be_offered_again_in_settings()
    {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let again = "A later release will be offered, and Settings > Updates offers this one again.";
        let mut view = view_of(UpdateDialogState::RolledBack);
        view.last_status = String::from("rolled_back");
        view.reason = String::from("health_timeout");
        bench.set_view(view.clone());
        assert_eq!(
            bench.shown("updateDetailText"),
            format!("Reason: the new version did not finish starting in time. {again}")
        );

        // Put back because a file could not be replaced: nothing was set aside.
        view.reason = String::from("rename_failed");
        bench.set_view(view.clone());
        assert_eq!(
            bench.shown("updateDetailText"),
            "Reason: a file was in use and could not be replaced."
        );

        // The same reasons under the two endings that put nothing back, or not all of it.
        for status in ["refused", "rollback_incomplete"] {
            view.last_status = String::from(status);
            view.reason = String::from("health_exit");
            bench.set_view(view.clone());
            assert!(
                !bench.shown("updateDetailText").contains("Settings"),
                "{status}"
            );
        }
        // And with no reason given there is no sentence to hang it on.
        view.last_status = String::from("rolled_back");
        view.reason.clear();
        bench.set_view(view);
        assert!(bench.shown("updateDetailText").is_empty());
    }

    // LastResult::status has two more endings than the design's sentence covers: refused, where
    // nothing was touched, and rollback_incomplete, where the old version is NOT back.
    #[test]
    fn an_apply_that_changed_nothing_and_one_that_could_not_be_put_back_are_not_called_restored()
    {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::RolledBack);
        view.last_status = String::from("refused");
        view.reason = String::from("space");
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 could not be installed; nothing was changed. Reason: there is not \
             enough free disk space."
        );

        view.last_status = String::from("rollback_incomplete");
        view.reason = String::from("rollback_failed");
        view.recover_command = String::from("katana_updater --recover \"C:/Programs/Katana\"");
        bench.set_view(view);
        assert_eq!(
            bench.shown("updateStateText"),
            "Katana 0.3.0 could not be installed, and 0.2.0 could not be put back completely."
        );
        assert_eq!(
            bench.shown("updateDetailText"),
            "Reason: the old version could not be put back completely. Close Katana and run: \
             katana_updater --recover \"C:/Programs/Katana\""
        );
        assert!(bench.shown_buttons().is_empty());
    }

    #[test]
    fn a_failed_check_says_why_and_offers_retry() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::CheckFailed);
        view.reason = String::from("offline");
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Could not check for updates: this computer is offline, or the release server did \
             not answer."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateRetry"]));

        // A reason nobody gave leaves the colon out with it.
        view.reason.clear();
        bench.set_view(view);
        assert_eq!(bench.said(), "Could not check for updates.");
    }

    // A private repository answers an anonymous read with 404 (design section 3): the one failure
    // whose remedy the dialog can name.
    #[test]
    fn a_check_that_finds_no_update_information_points_at_the_access_token_in_settings() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::CheckFailed);
        view.reason = String::from("http_404");
        bench.set_view(view);
        assert_eq!(
            bench.said(),
            "No update information found. If the releases are private, add an access token in \
             Settings > Updates."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateRetry"]));
    }

    #[test]
    fn unsupported_says_why_this_copy_cannot_update_and_offers_nothing() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::Unsupported);
        view.reason = String::from("development_tree");
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Updates are not available for this copy of Katana: it is a development build, not \
             an installed release."
        );
        assert!(
            bench.shown_buttons().is_empty(),
            "{:?}",
            bench.shown_buttons()
        );

        view.reason = String::from("root_not_writable");
        bench.set_view(view);
        assert_eq!(
            bench.said(),
            "Updates are not available for this copy of Katana: its folder cannot be written by \
             you."
        );
    }

    // The two answers of UPDATE CHECK that name a newer release without offering it (reason
    // "skipped", "blocked_before").
    #[test]
    fn a_release_that_was_skipped_or_rolled_back_here_before_is_said_and_not_offered() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = view_of(UpdateDialogState::NotOffered);
        view.reason = String::from("skipped");
        bench.set_view(view.clone());
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 is available, and you chose to skip it. Settings > Updates offers it \
             again."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateCheckNow"]));

        // And, since the hardening pass's ruling H5 (2026-10-14), where the way back is.
        view.reason = String::from("blocked_before");
        bench.set_view(view);
        assert_eq!(
            bench.said(),
            "Katana 0.3.0 could not be installed here before, and is not offered again. A later \
             release will be offered, and Settings > Updates offers this one again."
        );
        assert_eq!(bench.shown_buttons(), names(&["updateCheckNow"]));
    }

    // ---- every button calls exactly its callback, or builds exactly its line -------------------

    #[test]
    fn each_button_builds_exactly_its_line_or_calls_exactly_its_callback() {
        let mut blocked = view_of(UpdateDialogState::Blocked);
        blocked.holders = String::from("katana_mcp.exe (4412)");
        let mut failed = view_of(UpdateDialogState::CheckFailed);
        failed.reason = String::from("offline");
        let presses = [
            (view_of(UpdateDialogState::Idle), "updateCheckNow", "UPDATE CHECK"),
            (view_of(UpdateDialogState::Current), "updateCheckNow", "UPDATE CHECK"),
            (view_of(UpdateDialogState::NotOffered), "updateCheckNow", "UPDATE CHECK"),
            (view_of(UpdateDialogState::Checking), "updateCancel", "cancel x1"),
            // One click that is more than one line: the owner's, not a line.
            (available(), "updateInstall", "update x1"),
            (available(), "updateDownloadOnly", "UPDATE DOWNLOAD CONFIRM"),
            (available(), "updateSkip", "UPDATE SKIP 0.3.0"),
            (
                available(),
                "updateNotes",
                "notes https://github.com/kjarada/Katana/releases/tag/v0.3.0",
            ),
            (available(), "updateLater", "later x1"),
            (downloading(), "updateCancel", "cancel x1"),
            (view_of(UpdateDialogState::Ready), "updateRestart", "UPDATE APPLY CONFIRM"),
            (view_of(UpdateDialogState::Ready), "updateLater", "later x1"),
            // Retry tries again what failed: the swap for a blocked copy, the check for a failed
            // check.
            (blocked, "updateRetry", "UPDATE APPLY CONFIRM"),
            (failed, "updateRetry", "UPDATE CHECK"),
        ];
        for (view, button, called) in presses {
            let f = DialogFixture::new();
            let mut bench = UpdateBench::new(f.context());
            let state = view.state;
            bench.set_view(view);
            assert!(
                bench.shown_buttons().contains(&String::from(button)),
                "{button} is not shown in state {state:?}"
            );
            assert!(bench.enabled(button), "{button}");
            assert!(f.called().is_empty(), "showing a state calls nothing");
            bench.press(button);
            assert_eq!(f.called(), called, "{button} in state {state:?}");
        }
    }

    // The dialog shows what its owner says happened, never what a button hoped for: a line that
    // was carried out leaves the state as it was.
    #[test]
    fn a_press_changes_no_sentence_by_itself_the_next_view_does() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(available());
        let before = bench.said();
        bench.press("updateDownloadOnly");
        assert_eq!(bench.said(), before);
        assert_eq!(bench.dialog.view().state, UpdateDialogState::Available);
        assert_eq!(bench.shown_buttons().len(), 5);
    }

    #[test]
    fn later_tells_the_owner_and_hides_the_dialog() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(view_of(UpdateDialogState::Ready));
        bench.dialog.show();
        assert!(bench.dialog.is_visible());
        bench.press("updateLater");
        assert!(!bench.dialog.is_visible());
        assert_eq!(
            f.called(),
            "later x1",
            "and no line: the release stays staged"
        );
        // Kept, with what it showed: opened again it says the same.
        bench.dialog.show();
        bench.draw();
        assert_eq!(bench.shown("updateStateText"), "Katana 0.3.0 is ready.");
    }

    #[test]
    fn a_line_the_executor_refused_is_said_until_the_next_view() {
        let f = DialogFixture::new();
        f.answer(|_| refused("InvalidState: an update job is running"));
        let mut bench = UpdateBench::new(f.context());
        bench.set_view(available());
        bench.press("updateDownloadOnly");
        assert!(bench.visible("updateProblemText"));
        assert_eq!(
            bench.shown("updateProblemText"),
            "InvalidState: an update job is running"
        );
        assert_eq!(bench.dialog.problem_colour(), theme::error());
        assert_eq!(bench.shown("updateStateText"), "Update Available");

        // A refusal with no words of its own still says it was one.
        f.answer(|_| refused(""));
        bench.press("updateSkip");
        assert_eq!(bench.shown("updateProblemText"), "The command was refused.");

        bench.set_view(available());
        assert!(!bench.visible("updateProblemText"));
        assert!(bench.shown("updateProblemText").is_empty());
    }

    #[test]
    fn what_the_owner_could_not_do_is_shown_with_the_state_and_goes_with_the_next_view() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = available();
        view.problem = String::from(
            "The download did not finish: this network does not allow partial downloads.",
        );
        bench.set_view(view.clone());
        assert!(bench.visible("updateProblemText"));
        assert_eq!(bench.shown("updateProblemText"), view.problem);
        assert_eq!(bench.shown("updateStateText"), "Update Available");
        assert!(bench.enabled("updateInstall"), "it can be tried again");

        bench.set_view(available());
        assert!(!bench.visible("updateProblemText"));
    }

    #[test]
    fn a_button_with_nothing_to_call_is_disabled() {
        // Every member of the context at its default: nothing can be run.
        let mut bare = UpdateBench::new(UpdateDialogContext::default());
        for view in [
            view_of(UpdateDialogState::Idle),
            available(),
            downloading(),
            view_of(UpdateDialogState::Ready),
            view_of(UpdateDialogState::CheckFailed),
        ] {
            let state = view.state;
            bare.set_view(view);
            for name in bare.shown_buttons() {
                assert_eq!(
                    bare.enabled(&name),
                    name == "updateLater",
                    "{name} in state {state:?}"
                );
            }
        }
        // Later needs nobody: it only hides the dialog.
        bare.set_view(available());
        bare.dialog.show();
        bare.press("updateLater");
        assert!(!bare.dialog.is_visible());

        // With the executor alone, the lines can be built and the callbacks cannot.
        let f = DialogFixture::new();
        let lines = UpdateDialogContext {
            run: f.context().run,
            ..UpdateDialogContext::default()
        };
        let mut partial = UpdateBench::new(lines);
        partial.set_view(available());
        assert!(partial.enabled("updateDownloadOnly"));
        assert!(partial.enabled("updateSkip"));
        assert!(!partial.enabled("updateInstall"));
        assert!(!partial.enabled("updateNotes"));
    }

    #[test]
    fn release_notes_need_a_url_and_skip_needs_a_version() {
        let f = DialogFixture::new();
        let mut bench = UpdateBench::new(f.context());
        let mut view = available();
        view.notes_url.clear();
        view.latest_version.clear();
        bench.set_view(view);
        assert!(!bench.enabled("updateNotes"));
        assert!(!bench.enabled("updateSkip"));
        bench.press("updateNotes");
        bench.press("updateSkip");
        assert!(f.called().is_empty(), "{}", f.called());
    }

    // ---- lifetime -------------------------------------------------------------------------------

    #[test]
    fn the_dialog_outlives_the_view_it_was_given_and_the_owner_that_gave_it() {
        let owner = DialogFixture::new();
        let mut bench = UpdateBench::new(owner.context());
        {
            // What it shows is a copy: the view it was given goes out of scope.
            let view = Box::new(available());
            bench.set_view((*view).clone());
        }
        assert_eq!(
            bench.shown("updateDetailText"),
            "Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to download."
        );
        assert_eq!(
            bench.dialog.view().notes_url,
            "https://github.com/kjarada/Katana/releases/tag/v0.3.0"
        );

        // The owner goes first, and says so. Its callbacks held its record: none may be called
        // again.
        bench.dialog.detach();
        let record = std::rc::Rc::clone(&owner.record);
        drop(owner);
        for view in [
            view_of(UpdateDialogState::Idle),
            available(),
            downloading(),
            view_of(UpdateDialogState::Ready),
            view_of(UpdateDialogState::Blocked),
            view_of(UpdateDialogState::CheckFailed),
        ] {
            // Shown again for each: Later hides it, and the frames draw a dialog that is shown.
            bench.dialog.show();
            bench.set_view(view);
            for name in bench.shown_buttons() {
                assert_eq!(bench.enabled(&name), name == "updateLater", "{name}");
                // Even a click that reaches a disabled button's slot calls nothing.
                let button = UpdateButton::of_name(&name).expect("a button of the dialog");
                bench.dialog.click(&mut bench.host, button);
            }
        }
        let called = record.borrow();
        assert!(called.ran.is_empty(), "{:?}", called.ran);
        assert_eq!(
            (called.updates, called.cancels, called.laters),
            (0, 0, 0)
        );
        assert!(called.opened.is_empty());
        // It still says what it was last told.
        assert_eq!(bench.dialog.state_text(), "Could not check for updates.");
    }
}
