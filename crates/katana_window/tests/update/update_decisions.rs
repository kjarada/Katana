//! The twins of test_update_dialog.cpp's UpdateDecisions suite: what the window's update workbench
//! decides from the engine's records, as functions of their arguments beside the dialog. The
//! records are written by hand, as the C++ test wrote them: the design's 0.2.0 and 0.3.0, its
//! sequence 1792312800, its `expires` of 2027-04-18T08:00:00Z - 1 808 035 200 seconds after 1970
//! (20 926 days: 57 years of 365, 14 leap days, 31 + 28 + 31 + 17; and 8 hours). The recovery
//! log's lines are written in the two forms the engine states: a run of `katana_updater --recover`
//! ("<UTC time> recover found=<tx,...> result=ok|failed left=<tx,...>", and `detail=` after a
//! failure) and a journal record a rollback would not follow ("<tx> journal record refused:
//! <record>"). A transaction id is 1 to 64 characters, each 0-9 or a-f.

use katana_app::update_verbs::{UpdateOffer, UpdateStatus};
use katana_core::{Error, ErrorCode, Result};
use katana_update::check::CheckStatus;
use katana_update::{InstallKind, LastResult, Manifest, UpdateState, Version};
use katana_window::update_dialog::{
    UpdateAutoCheckFacts, UpdateConfirmFacts, UpdateDialogState, UpdateLineEffect,
    UpdateOfferFacts, UpdatePutBackFacts, UpdateQuietAnswer, UpdateView,
    update_automatic_check_is_due, update_detail_text, update_effect_of_line,
    update_failure_is_boxed, update_logged_reply, update_put_back_of_state, update_quiet_answer,
    update_recovered_transactions, update_reply_value, update_skipped_of_state,
    update_staged_after, update_staged_of_reply, update_staged_to_offer,
    update_start_may_confirm, update_status_button_text, update_tells_of_set_aside,
    update_view_of_last_result, update_view_of_offer,
};

use super::support::{available, downloading, said_by, view_of, view_of_version};

/// 2026-10-20T08:00:00Z: 20 746 days after 1970 (56 years of 365, 14 leap days, 273 days to the
/// first of October, 19 more) and 8 hours.
const NOW: i64 = 1_792_483_200;
/// The design's `sequence` for the build of 0.3.0.
const SEQUENCE: u64 = 1_792_312_800;

/// 0.2.0 as the install's record has it.
fn installed_version() -> Version {
    Version {
        components: [0, 2, 0, 0],
        written: 3,
    }
}

/// What UPDATE CHECK answers for the design's example: 0.2.0 installed, 0.3.0 released, 23 of 411
/// files and 71 MB when it is offered.
fn offer_of(status: CheckStatus) -> UpdateOffer {
    let mut offer = UpdateOffer::default();
    offer.check.status = status;
    offer.check.installed_version = String::from("0.2.0");
    offer.check.latest_version = String::from("0.3.0");
    offer.check.notes_url = String::from("https://github.com/kjarada/Katana/releases/tag/v0.3.0");
    if status == CheckStatus::Available {
        offer.changed_files = 23;
        offer.total_files = 411;
        offer.download_bytes = 71_000_000;
    }
    offer
}

fn facts_of() -> UpdateOfferFacts {
    UpdateOfferFacts {
        installed_version: String::from("0.2.0"),
        checked_at: String::from("14:02"),
        now_epoch: NOW,
        ..UpdateOfferFacts::default()
    }
}

/// A state file that did not read.
fn unreadable(code: ErrorCode) -> Result<UpdateState> {
    Err(Error::new(code, "the update settings could not be read"))
}

/// Two transaction ids of the length new_transaction_id gives them, 32 characters of lower-case
/// hex, written by hand.
const CRASHED: &str = "0123456789abcdef0123456789abcdef";
const POWER_CUT: &str = "fedcba9876543210fedcba9876543210";

/// The line a run of `katana_updater --recover` appends, by its form.
fn recovery_line(found: &str, result: &str, left: &str) -> String {
    format!("2026-10-13T08:40:00Z recover found={found} result={result} left={left}\n")
}

fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| String::from(*id)).collect()
}

/// An install as update_recovered_transactions asks about it: whether the folder R/.update/<id>
/// is gone, for the ids in `gone`; and which ids it was asked about, in the order it was asked.
#[derive(Default)]
struct AskedInstall {
    gone: Vec<String>,
    asked: Vec<String>,
}

impl AskedInstall {
    fn with_gone(gone: &[&str]) -> Self {
        Self {
            gone: ids(gone),
            asked: Vec::new(),
        }
    }

    /// The transactions `log` names that are over, asking this install.
    fn over(&mut self, log: &str) -> Vec<String> {
        let gone = &self.gone;
        let asked = &mut self.asked;
        let mut is_over = |id: &str| {
            asked.push(String::from(id));
            gone.iter().any(|folder| folder == id)
        };
        update_recovered_transactions(log.as_bytes(), Some(&mut is_over))
    }
}

mod update_decisions {
    use super::*;

    // THE REGRESSION. 0.3.0 was installed, did not start, and was rolled back: the helper put its
    // sequence on the blocked list and KEPT its stage. UPDATE STATUS then still names 0.3.0 as
    // downloaded; offered as ready, "Restart and update" would install the release known not to
    // start here.
    #[test]
    fn a_release_that_was_rolled_back_here_is_not_offered_as_ready_at_the_next_start() {
        let mut status = UpdateStatus {
            installed_version: String::from("0.2.0"),
            staged_version: String::from("0.3.0"),
            ..UpdateStatus::default()
        };
        status.state.blocked_sequences = vec![SEQUENCE];
        let stage_of_the_release_that_failed = |sequence: u64| sequence == SEQUENCE;
        assert!(
            update_staged_to_offer(
                &status,
                &installed_version(),
                Some(&stage_of_the_release_that_failed)
            )
            .is_empty()
        );

        // The same copy BEFORE that release failed: it is the one to offer.
        status.state.blocked_sequences.clear();
        assert_eq!(
            update_staged_to_offer(
                &status,
                &installed_version(),
                Some(&stage_of_the_release_that_failed)
            ),
            "0.3.0"
        );

        // And once a later build has been downloaded: the stage of the build that failed is gone,
        // so what STATUS names is not the release that does not start here.
        status.state.blocked_sequences = vec![SEQUENCE];
        status.staged_version = String::from("0.3.1");
        let no_stage_of_it = |_: u64| false;
        assert_eq!(
            update_staged_to_offer(&status, &installed_version(), Some(&no_stage_of_it)),
            "0.3.1"
        );
    }

    #[test]
    fn a_downloaded_release_is_offered_at_a_start_only_when_it_is_one_to_install() {
        let no_blocked_stage = |_: u64| false;
        let mut status = UpdateStatus {
            installed_version: String::from("0.2.0"),
            ..UpdateStatus::default()
        };
        // Nothing is downloaded.
        assert!(
            update_staged_to_offer(&status, &installed_version(), Some(&no_blocked_stage))
                .is_empty()
        );

        // Downloaded, and skipped since ("0.3" and "0.3.0" are one release): a skipped release is
        // not offered.
        status.staged_version = String::from("0.3.0");
        status.state.skipped_version = String::from("0.3");
        assert!(
            update_staged_to_offer(&status, &installed_version(), Some(&no_blocked_stage))
                .is_empty()
        );
        // Another version is the skipped one: this one is still to offer.
        status.state.skipped_version = String::from("0.2.5");
        assert_eq!(
            update_staged_to_offer(&status, &installed_version(), Some(&no_blocked_stage)),
            "0.3.0"
        );
        status.state.skipped_version.clear();

        // The stage a finished update left behind - what is installed, or older - is nothing to
        // install; nor is text that is no version.
        for stale in ["0.2.0", "0.2", "0.1.9", "none", "0.3.0-beta"] {
            status.staged_version = String::from(stale);
            assert!(
                update_staged_to_offer(&status, &installed_version(), Some(&no_blocked_stage))
                    .is_empty(),
                "{stale}"
            );
        }
    }

    #[test]
    fn a_check_that_offers_a_release_is_shown_as_the_design_says_it() {
        let view = update_view_of_offer(&offer_of(CheckStatus::Available), &facts_of());
        assert_eq!(view.state, UpdateDialogState::Available);
        assert_eq!(
            said_by(&view),
            "Update Available Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to \
             download."
        );
        assert_eq!(
            view.notes_url,
            "https://github.com/kjarada/Katana/releases/tag/v0.3.0"
        );
        assert!(!view.notify_only);
        assert!(!view.information_is_old);
        assert_eq!(update_status_button_text(&view), "Update 0.3.0");
    }

    #[test]
    fn the_release_that_is_already_downloaded_is_ready_not_to_be_downloaded_again() {
        let mut facts = facts_of();
        facts.staged_version = String::from("0.3"); // the same release as 0.3.0
        let view = update_view_of_offer(&offer_of(CheckStatus::Available), &facts);
        assert_eq!(
            said_by(&view),
            "Katana 0.3.0 is ready. Katana will close, update and reopen in a few seconds."
        );
        assert_eq!(update_status_button_text(&view), "Update 0.3.0");

        // What is downloaded is an older release than the one offered: that one is offered for
        // download.
        facts.staged_version = String::from("0.2.9");
        assert_eq!(
            update_view_of_offer(&offer_of(CheckStatus::Available), &facts).state,
            UpdateDialogState::Available
        );
    }

    #[test]
    fn every_other_answer_of_a_check_is_its_own_state_and_offers_nothing() {
        // The feed has nothing above this copy.
        let mut current = offer_of(CheckStatus::Current);
        current.check.latest_version = String::from("0.2.0");
        let view = update_view_of_offer(&current, &facts_of());
        assert_eq!(
            said_by(&view),
            "Katana 0.2.0 is up to date. Last checked 14:02."
        );
        assert!(update_status_button_text(&view).is_empty());

        // A release the person skipped: `current`, reason "skipped".
        let mut skipped = offer_of(CheckStatus::Current);
        skipped.check.reason = String::from("skipped");
        let view = update_view_of_offer(&skipped, &facts_of());
        assert_eq!(view.state, UpdateDialogState::NotOffered);
        assert_eq!(
            said_by(&view),
            "Katana 0.3.0 is available, and you chose to skip it. Settings > Updates offers it \
             again."
        );
        assert!(
            update_status_button_text(&view).is_empty(),
            "a skipped release is not offered"
        );

        // A release that was rolled back here: `blocked`. Its sentence names the place that
        // offers it again (ruling H5 of the hardening pass).
        let mut blocked = offer_of(CheckStatus::Blocked);
        blocked.check.reason = String::from("blocked_before");
        let view = update_view_of_offer(&blocked, &facts_of());
        assert_eq!(view.state, UpdateDialogState::NotOffered);
        assert_eq!(
            said_by(&view),
            "Katana 0.3.0 could not be installed here before, and is not offered again. A later \
             release will be offered, and Settings > Updates offers this one again."
        );
        assert!(
            update_status_button_text(&view).is_empty(),
            "a blocked release is not offered"
        );

        // The verbs give that answer a sentence of their own, for a line's reply. It changes
        // nothing here: the dialog says where its own button is.
        blocked.check.detail = String::from(
            "this release was installed here before, did not start and was put back, so it is \
             not offered; UPDATE BLOCKED CLEAR (Settings > Updates in the window) offers it again",
        );
        assert_eq!(
            said_by(&update_view_of_offer(&blocked, &facts_of())),
            said_by(&view)
        );

        // A private feed's 404.
        let mut unreachable = offer_of(CheckStatus::Unreachable);
        unreachable.check.latest_version.clear();
        unreachable.check.notes_url.clear();
        unreachable.check.reason = String::from("http_404");
        let view = update_view_of_offer(&unreachable, &facts_of());
        assert_eq!(
            said_by(&view),
            "No update information found. If the releases are private, add an access token in \
             Settings > Updates."
        );
        assert!(update_status_button_text(&view).is_empty());

        // A copy that cannot update, whose answer names no version: the copy's own.
        let mut unsupported = UpdateOffer::default();
        unsupported.check.status = CheckStatus::Unsupported;
        unsupported.check.reason = String::from("development_tree");
        let view = update_view_of_offer(&unsupported, &facts_of());
        assert_eq!(
            said_by(&view),
            "Updates are not available for this copy of Katana: it is a development build, not \
             an installed release."
        );
        assert_eq!(view.installed_version, "0.2.0");
        assert!(update_status_button_text(&view).is_empty());
    }

    #[test]
    fn a_notify_only_release_and_a_copy_that_cannot_apply_are_told_of_and_not_installed() {
        let mut offer = offer_of(CheckStatus::Available);
        offer.check.manifest = Some(Manifest {
            expires: String::from("2027-04-18T08:00:00Z"),
            notify_only: true,
            ..Manifest::default()
        });
        assert!(update_view_of_offer(&offer, &facts_of()).notify_only);

        if let Some(manifest) = offer.check.manifest.as_mut() {
            manifest.notify_only = false;
        }
        assert!(!update_view_of_offer(&offer, &facts_of()).notify_only);
        // The same release where the copy cannot replace its own files.
        let mut elsewhere = facts_of();
        elsewhere.applies_here = false;
        let view = update_view_of_offer(&offer, &elsewhere);
        assert!(view.notify_only);
        assert_eq!(
            update_detail_text(&view),
            "Katana 0.3.0 (you have 0.2.0). This copy cannot update itself: the release notes \
             say where to download it."
        );
    }

    #[test]
    fn update_information_past_its_expiry_is_said_to_be_old_and_the_release_still_offered() {
        let mut offer = offer_of(CheckStatus::Available);
        offer.check.manifest = Some(Manifest {
            expires: String::from("2027-04-18T08:00:00Z"), // 1 808 035 200
            ..Manifest::default()
        });
        let mut facts = facts_of();
        assert!(
            !update_view_of_offer(&offer, &facts).information_is_old,
            "six months before"
        );
        // At the second it expires it has not passed; one later it has.
        facts.now_epoch = 1_808_035_200;
        assert!(!update_view_of_offer(&offer, &facts).information_is_old);
        facts.now_epoch = 1_808_035_201;
        let old = update_view_of_offer(&offer, &facts);
        assert!(old.information_is_old);
        assert_eq!(
            old.state,
            UpdateDialogState::Available,
            "advisory: nothing is refused"
        );
        // An answer that carries no manifest is never called old.
        offer.check.manifest = None;
        assert!(!update_view_of_offer(&offer, &facts).information_is_old);
    }

    #[test]
    fn the_automatic_checks_answer_is_announced_shown_without_a_word_or_dropped() {
        let mut up_to_date = view_of(UpdateDialogState::Current);
        up_to_date.latest_version = String::from("0.2.0");
        let mut blocked_before = view_of(UpdateDialogState::NotOffered);
        blocked_before.reason = String::from("blocked_before");
        let mut skipped = view_of(UpdateDialogState::NotOffered);
        skipped.reason = String::from("skipped");
        let cases: Vec<(UpdateView, UpdateView, UpdateQuietAnswer, &str)> = vec![
            // News.
            (
                view_of(UpdateDialogState::Idle),
                available(),
                UpdateQuietAnswer::Announce,
                "a release, and nothing was known",
            ),
            (
                up_to_date.clone(),
                available(),
                UpdateQuietAnswer::Announce,
                "a release since the last check",
            ),
            (
                available(),
                view_of_version(UpdateDialogState::Available, "0.3.1"),
                UpdateQuietAnswer::Announce,
                "a later release than the one offered",
            ),
            (
                view_of(UpdateDialogState::CheckFailed),
                available(),
                UpdateQuietAnswer::Announce,
                "a release, after a check that failed",
            ),
            // No news: the release is offered already, in whichever way.
            (
                available(),
                available(),
                UpdateQuietAnswer::Nothing,
                "the release already offered",
            ),
            (
                view_of(UpdateDialogState::Ready),
                view_of(UpdateDialogState::Ready),
                UpdateQuietAnswer::Nothing,
                "the release already downloaded",
            ),
            (
                view_of(UpdateDialogState::Blocked),
                view_of(UpdateDialogState::Ready),
                UpdateQuietAnswer::Nothing,
                "the programs to close stay named",
            ),
            // Quiet when it learns nothing.
            (
                view_of(UpdateDialogState::Idle),
                view_of(UpdateDialogState::CheckFailed),
                UpdateQuietAnswer::Nothing,
                "a check that failed says nothing",
            ),
            (
                view_of(UpdateDialogState::Ready),
                view_of(UpdateDialogState::CheckFailed),
                UpdateQuietAnswer::Nothing,
                "and takes nothing back",
            ),
            (
                view_of(UpdateDialogState::Idle),
                view_of(UpdateDialogState::Unsupported),
                UpdateQuietAnswer::Nothing,
                "a copy that cannot update says nothing by itself",
            ),
            // Nothing to tell, something to show.
            (
                view_of(UpdateDialogState::Idle),
                up_to_date.clone(),
                UpdateQuietAnswer::Show,
                "up to date, for when the dialog is opened",
            ),
            (
                view_of(UpdateDialogState::CheckFailed),
                up_to_date.clone(),
                UpdateQuietAnswer::Show,
                "up to date, over an older failure",
            ),
            // A release the window still offers is taken back, without a word.
            (
                view_of(UpdateDialogState::Ready),
                blocked_before,
                UpdateQuietAnswer::Show,
                "ready, and rolled back here: not offered",
            ),
            (
                available(),
                skipped,
                UpdateQuietAnswer::Show,
                "offered, and skipped: not offered",
            ),
            (
                view_of(UpdateDialogState::Ready),
                up_to_date.clone(),
                UpdateQuietAnswer::Show,
                "ready, and no longer released: not offered",
            ),
            // What must stay until it is read, and what a job owns.
            (
                view_of(UpdateDialogState::RolledBack),
                available(),
                UpdateQuietAnswer::Nothing,
                "how the last update ended stays until it is read",
            ),
            (
                view_of(UpdateDialogState::RolledBack),
                up_to_date.clone(),
                UpdateQuietAnswer::Nothing,
                "how the last update ended stays until it is read",
            ),
            (
                view_of(UpdateDialogState::Checking),
                available(),
                UpdateQuietAnswer::Nothing,
                "a check a person asked for answers for itself",
            ),
            (
                downloading(),
                up_to_date,
                UpdateQuietAnswer::Nothing,
                "a download is not interrupted",
            ),
        ];
        for (shown, answer, expected, why) in cases {
            assert_eq!(update_quiet_answer(&shown, &answer), expected, "{why}");
        }
    }

    #[test]
    fn a_downloaded_release_stays_ready_only_while_a_check_still_offers_it() {
        let staged = "0.3.0";
        // The feed has nothing above this copy any more: the release was withdrawn.
        assert!(update_staged_after(staged, &view_of(UpdateDialogState::Current)).is_empty());
        // That very version was skipped, or rolled back here ("0.3" is 0.3.0).
        assert!(update_staged_after(staged, &view_of(UpdateDialogState::NotOffered)).is_empty());
        assert!(
            update_staged_after(
                staged,
                &view_of_version(UpdateDialogState::NotOffered, "0.3")
            )
            .is_empty()
        );
        // Another version is the one not offered; a check that failed un-knows nothing; and an
        // answer that offers a release leaves what is downloaded.
        assert_eq!(
            update_staged_after(
                staged,
                &view_of_version(UpdateDialogState::NotOffered, "0.4.0")
            ),
            staged
        );
        assert_eq!(
            update_staged_after(staged, &view_of(UpdateDialogState::CheckFailed)),
            staged
        );
        assert_eq!(
            update_staged_after(staged, &view_of(UpdateDialogState::Ready)),
            staged
        );
        assert_eq!(
            update_staged_after(
                staged,
                &view_of_version(UpdateDialogState::Available, "0.4.0")
            ),
            staged
        );
    }

    #[test]
    fn how_the_last_update_ended_is_its_own_state_and_a_success_is_none() {
        let installed = "0.2.0";
        let root = "C:\\Programs\\Katana";
        let mut last = LastResult {
            version: String::from("0.3.0"),
            transaction: String::from("0a1b2c3d"),
            ..LastResult::default()
        };

        // A success is one line in the log, not a dialog.
        last.status = String::from("committed");
        assert!(update_view_of_last_result(&last, installed, root).is_none());

        // The design's sentence, and where the release it tells of is offered again.
        last.status = String::from("rolled_back");
        last.reason = String::from("health_exit");
        let view = update_view_of_last_result(&last, installed, root).expect("a view");
        assert_eq!(view.state, UpdateDialogState::RolledBack);
        assert_eq!(
            said_by(&view),
            "Katana 0.3.0 could not be installed, so 0.2.0 was restored. Reason: the new version \
             did not start. A later release will be offered, and Settings > Updates offers this \
             one again."
        );
        assert!(
            update_status_button_text(&view).is_empty(),
            "what was rolled back is not offered"
        );

        // Nothing was touched: the programs to close, and the release still there.
        last.status = String::from("blocked");
        last.reason = String::from("blocked");
        last.holder = String::from("katana_mcp.exe (4412)");
        let view = update_view_of_last_result(&last, installed, root).expect("a view");
        assert_eq!(view.state, UpdateDialogState::Blocked);
        assert_eq!(
            said_by(&view),
            "Close these first: katana_mcp.exe (4412). Then press Retry."
        );
        assert_eq!(update_status_button_text(&view), "Update 0.3.0");

        last.holder.clear();
        last.status = String::from("refused");
        last.reason = String::from("space");
        let view = update_view_of_last_result(&last, installed, root).expect("a view");
        assert_eq!(
            said_by(&view),
            "Katana 0.3.0 could not be installed; nothing was changed. Reason: there is not \
             enough free disk space."
        );

        // The old version is NOT back: the command that puts it right, by hand.
        last.status = String::from("rollback_incomplete");
        last.reason = String::from("rollback_failed");
        let view = update_view_of_last_result(&last, installed, root).expect("a view");
        assert_eq!(
            said_by(&view),
            "Katana 0.3.0 could not be installed, and 0.2.0 could not be put back completely. \
             Reason: the old version could not be put back completely. Close Katana and run: \
             katana_updater --recover \"C:\\Programs\\Katana\""
        );
    }

    // The helper refuses an apply as interrupted_update while an earlier update of the install is
    // neither confirmed nor put back, and names the way out in its own `detail` alone; the result
    // it leaves for the next start has no detail. The view carries the command for this ending,
    // built as it is for rollback_incomplete.
    #[test]
    fn an_update_refused_over_an_earlier_one_that_is_not_settled_names_the_command_that_settles_it()
    {
        let installed = "0.2.0";
        let root = "C:\\Programs\\Katana";
        let mut last = LastResult {
            version: String::from("0.3.0"),
            transaction: String::from("0a1b2c3d"),
            status: String::from("refused"),
            reason: String::from("interrupted_update"),
            ..LastResult::default()
        };
        let view = update_view_of_last_result(&last, installed, root).expect("a view");
        assert_eq!(view.state, UpdateDialogState::RolledBack);
        assert_eq!(
            view.recover_command,
            "katana_updater --recover \"C:\\Programs\\Katana\""
        );
        assert_eq!(
            said_by(&view),
            "Katana 0.3.0 could not be installed; nothing was changed. Reason: an earlier update \
             is not settled yet. Update again; if this is said again, close Katana and run: \
             katana_updater --recover \"C:\\Programs\\Katana\""
        );
        assert!(update_status_button_text(&view).is_empty());

        // Every other refusal changed nothing and left nothing to settle: no command is carried
        // and none is said.
        for reason in ["space", "downgrade", "staged_hash", "platform"] {
            last.reason = String::from(reason);
            let other = update_view_of_last_result(&last, installed, root).expect("a view");
            assert!(other.recover_command.is_empty(), "{reason}");
            assert!(!said_by(&other).contains("katana_updater"), "{reason}");
        }

        // And it is that refusal, not the token under any ending: an install that was changed and
        // put back is told what was restored, and no command.
        last.status = String::from("rolled_back");
        last.reason = String::from("interrupted_update");
        let restored = update_view_of_last_result(&last, installed, root).expect("a view");
        assert!(restored.recover_command.is_empty());
        assert_eq!(
            said_by(&restored),
            "Katana 0.3.0 could not be installed, so 0.2.0 was restored. Reason: an earlier \
             update is not settled yet."
        );
    }

    #[test]
    fn the_window_checks_by_itself_only_when_every_condition_holds() {
        let no_jitter = 0;
        let two_hours = 2 * 3600;
        // An installed copy, a window a person uses, nothing running, nothing switched off - and
        // never checked, with the automatic flag at its default.
        let go = UpdateAutoCheckFacts {
            kind: InstallKind::Installer,
            keeps_state: true,
            ..UpdateAutoCheckFacts::default()
        };
        let never = UpdateState::default();
        assert!(never.automatic, "the state's default is on");
        assert!(update_automatic_check_is_due(&go, &never, NOW, no_jitter));
        let unpacked = UpdateAutoCheckFacts {
            kind: InstallKind::Archive,
            ..go
        };
        assert!(update_automatic_check_is_due(
            &unpacked, &never, NOW, no_jitter
        ));

        // Each condition, alone, stops it.
        let headless = UpdateAutoCheckFacts {
            headless: true,
            ..go
        };
        assert!(
            !update_automatic_check_is_due(&headless, &never, NOW, no_jitter),
            "headless"
        );
        let disabled = UpdateAutoCheckFacts {
            disabled: true,
            ..go
        };
        assert!(
            !update_automatic_check_is_due(&disabled, &never, NOW, no_jitter),
            "KATANA_UPDATE_DISABLE"
        );
        let development = UpdateAutoCheckFacts {
            kind: InstallKind::Development,
            ..go
        };
        assert!(
            !update_automatic_check_is_due(&development, &never, NOW, no_jitter),
            "a development tree stays offline"
        );
        let busy = UpdateAutoCheckFacts {
            job_running: true,
            ..go
        };
        assert!(
            !update_automatic_check_is_due(&busy, &never, NOW, no_jitter),
            "an update job runs"
        );
        let stateless = UpdateAutoCheckFacts {
            keeps_state: false,
            ..go
        };
        assert!(
            !update_automatic_check_is_due(&stateless, &never, NOW, no_jitter),
            "no state folder"
        );
        let switched_off = UpdateState {
            automatic: false,
            ..UpdateState::default()
        };
        assert!(
            !update_automatic_check_is_due(&go, &switched_off, NOW, no_jitter),
            "the person switched it off"
        );

        // The clock: 24 hours and the jitter after the last check.
        let mut checked = UpdateState {
            last_check_epoch: NOW - 3600,
            ..UpdateState::default()
        };
        assert!(
            !update_automatic_check_is_due(&go, &checked, NOW, no_jitter),
            "an hour ago"
        );
        checked.last_check_epoch = NOW - 24 * 3600;
        assert!(
            update_automatic_check_is_due(&go, &checked, NOW, no_jitter),
            "24 hours ago"
        );
        assert!(
            !update_automatic_check_is_due(&go, &checked, NOW, two_hours),
            "24 hours ago, with two hours of jitter still to go"
        );
        checked.last_check_epoch = NOW - 26 * 3600;
        assert!(
            update_automatic_check_is_due(&go, &checked, NOW, two_hours),
            "26 hours ago"
        );
        // A last check in the future - the clock was set back - silences nothing.
        checked.last_check_epoch = NOW + 3600;
        assert!(update_automatic_check_is_due(&go, &checked, NOW, no_jitter));
    }

    // The hardening pass's H1 (2026-10-14): an update whose helper died while it waited for the
    // new version to prove that it starts is settled only by an interactive window of the version
    // it installed. Each condition is taken away in turn.
    #[test]
    fn a_start_confirms_an_update_only_in_a_window_of_the_version_that_was_installed() {
        let version = |minor: u32, written: u8| Version {
            components: [0, minor, 0, 0],
            written,
        };
        // 0.3.0 was installed by an update, and this window is 0.3.0.
        let go = UpdateConfirmFacts {
            applies_here: true,
            keeps_state: true,
            kind: InstallKind::Installer,
            has_installed_record: true,
            installed: version(3, 3),
            program: version(3, 3),
            ..UpdateConfirmFacts::default()
        };
        assert!(update_start_may_confirm(&go));
        let unpacked = UpdateConfirmFacts {
            kind: InstallKind::Archive,
            ..go
        };
        assert!(
            update_start_may_confirm(&unpacked),
            "an unpacked archive updates itself too"
        );
        // "0.3" and "0.3.0" are one version.
        let shortened = UpdateConfirmFacts {
            program: version(3, 2),
            ..go
        };
        assert!(update_start_may_confirm(&shortened));

        // THE CASE THE RULE EXISTS FOR: a window of the old files, started while the update was
        // swapping them.
        let old_window = UpdateConfirmFacts {
            program: version(2, 3),
            ..go
        };
        assert!(!update_start_may_confirm(&old_window));
        // The rule is "the program's own", so a version below the program's is refused too.
        let below = UpdateConfirmFacts {
            installed: version(2, 3),
            ..go
        };
        assert!(!update_start_may_confirm(&below));

        // Each other condition, alone, stops it.
        let fresh = UpdateConfirmFacts {
            has_installed_record: false,
            ..go
        };
        assert!(
            !update_start_may_confirm(&fresh),
            "no update ever committed here"
        );
        let headless = UpdateConfirmFacts {
            headless: true,
            ..go
        };
        assert!(
            !update_start_may_confirm(&headless),
            "a headless run writes no per-user state"
        );
        let development = UpdateConfirmFacts {
            kind: InstallKind::Development,
            ..go
        };
        assert!(
            !update_start_may_confirm(&development),
            "a build tree is never updated"
        );
        let stateless = UpdateConfirmFacts {
            keeps_state: false,
            ..go
        };
        assert!(
            !update_start_may_confirm(&stateless),
            "no update folder to write the proof in"
        );
        let elsewhere = UpdateConfirmFacts {
            applies_here: false,
            ..go
        };
        assert!(
            !update_start_may_confirm(&elsewhere),
            "a copy that cannot replace its own files"
        );
        // And with nothing known, nothing is confirmed.
        assert!(!update_start_may_confirm(&UpdateConfirmFacts::default()));
    }

    // What a line the window runs at once does to what the dialog shows. A dialog that says a
    // release "is not offered again" must not go on saying so once the list that set it aside is
    // empty - and each of the two lists is its own.
    #[test]
    fn a_line_that_was_carried_out_takes_back_only_what_its_own_list_set_aside() {
        let clear = "UPDATE BLOCKED CLEAR";
        let cleared = "blocked=none";
        let mut put_back = view_of(UpdateDialogState::NotOffered);
        put_back.reason = String::from("blocked_before");
        let mut skipped = view_of(UpdateDialogState::NotOffered);
        skipped.reason = String::from("skipped");

        // The release that was put back is set aside no longer.
        assert_eq!(
            update_effect_of_line(&put_back, clear, cleared),
            UpdateLineEffect::Forgotten
        );
        // Typed as a person types it: in any case, with any blanks.
        assert_eq!(
            update_effect_of_line(&put_back, "  update   blocked  clear ", cleared),
            UpdateLineEffect::Forgotten
        );
        // No update folder: nothing was kept, so nothing was emptied.
        assert_eq!(
            update_effect_of_line(&put_back, clear, "blocked=none reason=no_state"),
            UpdateLineEffect::Nothing
        );
        // The skipped version is another list: this line does not offer it again.
        assert_eq!(
            update_effect_of_line(&skipped, clear, cleared),
            UpdateLineEffect::Nothing
        );
        // And a view that says nothing is set aside has nothing to take back.
        for state in [
            UpdateDialogState::Idle,
            UpdateDialogState::Checking,
            UpdateDialogState::Current,
            UpdateDialogState::Available,
            UpdateDialogState::Downloading,
            UpdateDialogState::Ready,
            UpdateDialogState::Blocked,
            UpdateDialogState::RolledBack,
            UpdateDialogState::CheckFailed,
            UpdateDialogState::Unsupported,
        ] {
            assert_eq!(
                update_effect_of_line(&view_of(state), clear, cleared),
                UpdateLineEffect::Nothing,
                "{state:?}"
            );
        }
        // UPDATE BLOCKED has the one line; nothing else under those words empties anything.
        for line in [
            "UPDATE BLOCKED",
            "UPDATE BLOCKED LIST",
            "UPDATE BLOCKED CLEAR ALL",
            "BLOCKED CLEAR UPDATE",
        ] {
            assert_eq!(
                update_effect_of_line(&put_back, line, cleared),
                UpdateLineEffect::Nothing,
                "{line}"
            );
        }

        // UPDATE SKIP NONE does the same for its own list, and leaves the other.
        let unskip = "UPDATE SKIP NONE";
        let unskipped = "skipped=none";
        assert_eq!(
            update_effect_of_line(&skipped, unskip, unskipped),
            UpdateLineEffect::Forgotten
        );
        assert_eq!(
            update_effect_of_line(&skipped, "update skip none", unskipped),
            UpdateLineEffect::Forgotten
        );
        assert_eq!(
            update_effect_of_line(&put_back, unskip, unskipped),
            UpdateLineEffect::Nothing
        );
        assert_eq!(
            update_effect_of_line(&skipped, unskip, "skipped=none reason=no_state"),
            UpdateLineEffect::Nothing
        );

        // UPDATE SKIP <version> of the release the dialog offers: offered no more.
        let offered = view_of(UpdateDialogState::Available);
        assert_eq!(offered.latest_version, "0.3.0");
        assert_eq!(
            update_effect_of_line(&offered, "UPDATE SKIP 0.3.0", "skipped=0.3.0"),
            UpdateLineEffect::Skipped
        );
        // "0.3" and "0.3.0" are one release.
        assert_eq!(
            update_effect_of_line(&offered, "UPDATE SKIP 0.3", "skipped=0.3"),
            UpdateLineEffect::Skipped
        );
        // Another version than the one offered leaves the offer as it is.
        assert_eq!(
            update_effect_of_line(&offered, "UPDATE SKIP 0.4.0", "skipped=0.4.0"),
            UpdateLineEffect::Nothing
        );
        // And with no update folder the skip was not kept.
        assert_eq!(
            update_effect_of_line(
                &offered,
                "UPDATE SKIP 0.3.0",
                "skipped=none reason=no_state"
            ),
            UpdateLineEffect::Nothing
        );

        // Every other line changes nothing here, whatever the view.
        let others = [
            (
                "UPDATE STATUS",
                "installed=0.2.0 staged=none skipped=none auto=on",
            ),
            ("UPDATE SET AUTO OFF", "auto=off"),
            ("UPDATE TOKEN CLEAR", "token=absent"),
        ];
        for (line, reply) in others {
            for view in [&put_back, &skipped, &offered] {
                assert_eq!(
                    update_effect_of_line(view, line, reply),
                    UpdateLineEffect::Nothing,
                    "{line}"
                );
            }
        }
    }

    // What Settings > Updates is told of U/state.json: a file that does not read is "nobody can
    // say", never "none" - told as "none", the page would switch the button off and keep a person
    // from the verb's refusal, which names the cause.
    #[test]
    fn a_state_file_that_does_not_read_is_nobody_can_say_and_never_none() {
        // A fresh install, with no record of its own build, for which every build on the list
        // counts.
        let fresh = UpdatePutBackFacts::default();

        for code in [ErrorCode::ParseFailure, ErrorCode::Unsupported] {
            let state = unreadable(code);
            assert!(update_skipped_of_state(&state).is_none());
            assert!(update_put_back_of_state(&state, &fresh).is_none());
        }

        // A file that is not there reads as the defaults, and those ARE known: none skipped, none
        // put back.
        let defaults: Result<UpdateState> = Ok(UpdateState::default());
        assert_eq!(update_skipped_of_state(&defaults), Some(String::new()));
        assert_eq!(update_put_back_of_state(&defaults, &fresh), Some(false));

        // What the file has: the skipped version as it is written, and whether the list of blocked
        // releases holds any - one is enough.
        let kept: Result<UpdateState> = Ok(UpdateState {
            skipped_version: String::from("0.3.0"),
            blocked_sequences: vec![SEQUENCE],
            ..UpdateState::default()
        });
        assert_eq!(update_skipped_of_state(&kept), Some(String::from("0.3.0")));
        assert_eq!(update_put_back_of_state(&kept, &fresh), Some(true));

        // Each is its own list: a skip is no release that was put back, nor the other way about.
        let only_skipped: Result<UpdateState> = Ok(UpdateState {
            skipped_version: String::from("0.3.0"),
            ..UpdateState::default()
        });
        assert_eq!(update_put_back_of_state(&only_skipped, &fresh), Some(false));
        let only_put_back: Result<UpdateState> = Ok(UpdateState {
            blocked_sequences: vec![SEQUENCE, SEQUENCE + 1],
            ..UpdateState::default()
        });
        assert_eq!(update_skipped_of_state(&only_put_back), Some(String::new()));
    }

    // THE REGRESSION. The state's list of blocked releases is only ever added to until UPDATE
    // BLOCKED CLEAR empties it, so after 0.3.0 was put back and 0.4.0 installed and started, the
    // page went on saying a version was put back, of a release no feed offers over 0.4.0. The
    // builds, by hand: 0.2.0 made a week before the design's 0.3.0 and 0.4.0 a week after it, a
    // week being 7 x 86 400 = 604 800 seconds.
    #[test]
    fn a_version_that_was_put_back_is_told_of_only_while_it_can_still_be_offered_here() {
        const WEEK: u64 = 604_800;
        const BUILD_OF_THE_OLD_VERSION: u64 = SEQUENCE - WEEK; // 0.2.0
        const BUILD_OF_THE_NEXT_VERSION: u64 = SEQUENCE + WEEK; // 0.4.0
        // The answer for a file that read; one that did not fails here.
        let told = |state: &UpdateState, facts: &UpdatePutBackFacts| {
            let answer = update_put_back_of_state(&Ok(state.clone()), facts);
            assert!(answer.is_some());
            answer.unwrap_or(false)
        };
        let put_back = UpdateState {
            blocked_sequences: vec![SEQUENCE], // 0.3.0 did not start here
            ..UpdateState::default()
        };

        // 0.2.0 is installed, by an update that recorded its build. 0.3.0 was made after it and
        // is what a feed offers over it: it is told of.
        let on_the_old_version = UpdatePutBackFacts {
            has_installed_record: true,
            installed_sequence: BUILD_OF_THE_OLD_VERSION,
            dialog_tells_of_one: false,
        };
        assert!(told(&put_back, &on_the_old_version));

        // 0.4.0 was installed since, and started: the build that was put back was made before
        // the one installed, and is not told of.
        let on_the_next_version = UpdatePutBackFacts {
            has_installed_record: true,
            installed_sequence: BUILD_OF_THE_NEXT_VERSION,
            dialog_tells_of_one: false,
        };
        assert!(!told(&put_back, &on_the_next_version));

        // "After" is strictly after: a build is not offered over itself.
        let on_that_very_build = UpdatePutBackFacts {
            has_installed_record: true,
            installed_sequence: SEQUENCE,
            dialog_tells_of_one: false,
        };
        assert!(!told(&put_back, &on_that_very_build));

        // One build made before the installed one and one made after it (0.5.0, a week after
        // 0.4.0): the later one is enough, wherever it stands.
        let mut two = UpdateState {
            blocked_sequences: vec![SEQUENCE, BUILD_OF_THE_NEXT_VERSION + WEEK],
            ..UpdateState::default()
        };
        assert!(told(&two, &on_the_next_version));
        two.blocked_sequences = vec![BUILD_OF_THE_NEXT_VERSION + WEEK, SEQUENCE];
        assert!(told(&two, &on_the_next_version));

        // A fresh install has no record of its build: its sequence is a placeholder and is not
        // read, whatever it holds.
        let mut fresh = UpdatePutBackFacts::default();
        assert!(told(&put_back, &fresh));
        fresh.installed_sequence = BUILD_OF_THE_NEXT_VERSION;
        assert!(told(&put_back, &fresh));

        // While the dialog says "Settings > Updates offers this one again" the page is told of the
        // build, so that its button is on.
        let while_the_dialog_tells_of_it = UpdatePutBackFacts {
            dialog_tells_of_one: true,
            ..on_the_next_version
        };
        assert!(told(&put_back, &while_the_dialog_tells_of_it));
        // The list is what the button empties: with nothing on it, none is told of, whatever the
        // dialog still shows.
        assert!(!told(&UpdateState::default(), &while_the_dialog_tells_of_it));
        assert!(!told(&UpdateState::default(), &on_the_old_version));

        // And a file that does not read is nobody can say, under every fact.
        let state = unreadable(ErrorCode::ParseFailure);
        for facts in [
            on_the_old_version,
            on_the_next_version,
            fresh,
            while_the_dialog_tells_of_it,
        ] {
            assert!(update_put_back_of_state(&state, &facts).is_none());
        }
    }

    // The two sentences that tell of a release that is set aside end "Settings > Updates offers
    // this one again.": update_tells_of_set_aside holds for exactly the views that say it.
    #[test]
    fn the_dialog_tells_of_a_release_that_is_set_aside_exactly_where_it_points_at_settings() {
        let again = "Settings > Updates offers this one again.";
        let ended = |status: &str, reason: &str| {
            let mut view = view_of(UpdateDialogState::RolledBack);
            view.last_status = String::from(status);
            view.reason = String::from(reason);
            view
        };
        let with_reason = |state: UpdateDialogState, reason: &str| {
            let mut view = view_of(state);
            view.reason = String::from(reason);
            view
        };

        for view in [
            with_reason(UpdateDialogState::NotOffered, "blocked_before"),
            ended("rolled_back", "health_exit"),
            ended("rolled_back", "health_timeout"),
        ] {
            assert!(update_tells_of_set_aside(&view), "{}", said_by(&view));
            assert!(
                update_detail_text(&view).ends_with(again),
                "{}",
                said_by(&view)
            );
        }

        // A skipped release is another list's; an install that was put back for another reason,
        // or not put back, set nothing aside; and the token under a state that does not read it
        // tells of nothing.
        for view in [
            with_reason(UpdateDialogState::NotOffered, "skipped"),
            ended("rolled_back", "rename_failed"),
            ended("refused", "health_exit"),
            ended("rollback_incomplete", "health_timeout"),
            ended("rolled_back", ""),
            with_reason(UpdateDialogState::Available, "blocked_before"),
            with_reason(UpdateDialogState::CheckFailed, "blocked_before"),
            view_of(UpdateDialogState::Idle),
            view_of(UpdateDialogState::Checking),
            view_of(UpdateDialogState::Current),
            view_of(UpdateDialogState::Downloading),
            view_of(UpdateDialogState::Ready),
            view_of(UpdateDialogState::Blocked),
            view_of(UpdateDialogState::Unsupported),
        ] {
            assert!(!update_tells_of_set_aside(&view), "{}", said_by(&view));
            assert!(
                !update_detail_text(&view).ends_with(again),
                "{}",
                said_by(&view)
            );
        }
    }

    // The hardening pass's point for the program (2026-10-14): a helper that could not be started
    // is said on stderr always, and in a box only when the run was given no --command.
    #[test]
    fn an_update_that_could_not_be_started_is_boxed_only_in_a_window_nobody_drove() {
        // A window a person started: nobody reads a GUI program's stderr.
        assert!(update_failure_is_boxed::<&str>(&[]));
        // A run that was given a line to run may have nobody at it to close a box.
        assert!(!update_failure_is_boxed(&["--command"]));
        assert!(!update_failure_is_boxed(&["--run-line"]));
        // Wherever among its steps the line stands, and however many there are.
        assert!(!update_failure_is_boxed(&[
            "--dialog",
            "--press",
            "--command"
        ]));
        assert!(!update_failure_is_boxed(&["--run-line", "--report"]));
        assert!(!update_failure_is_boxed(&[
            "--command",
            "--command",
            "--run-line"
        ]));
        // Steps that run no line give the run no --command: it is still boxed.
        assert!(update_failure_is_boxed(&["--dialog", "--press"]));
    }

    #[test]
    fn the_status_bar_button_names_the_release_and_is_hidden_until_one_is_known() {
        // "Update <version>" while there is a release to act on.
        for state in [
            UpdateDialogState::Available,
            UpdateDialogState::Downloading,
            UpdateDialogState::Ready,
            UpdateDialogState::Blocked,
        ] {
            assert_eq!(
                update_status_button_text(&view_of(state)),
                "Update 0.3.0",
                "{state:?}"
            );
        }
        // Hidden - no text - in every other state, though a version is named...
        for state in [
            UpdateDialogState::Idle,
            UpdateDialogState::Checking,
            UpdateDialogState::Current,
            UpdateDialogState::NotOffered,
            UpdateDialogState::RolledBack,
            UpdateDialogState::CheckFailed,
            UpdateDialogState::Unsupported,
        ] {
            assert!(
                update_status_button_text(&view_of(state)).is_empty(),
                "{state:?}"
            );
        }
        // ...as made, before anything is known, and for a download nobody named.
        assert!(update_status_button_text(&UpdateView::default()).is_empty());
        let unnamed = view_of_version(UpdateDialogState::Downloading, "");
        assert!(update_status_button_text(&unnamed).is_empty());
    }

    #[test]
    fn a_record_of_a_reply_is_read_by_its_key_and_the_last_one_counts() {
        // The verbs' grammar: records `key=value`, blanks between.
        let check = "update=unreachable installed=0.2.0 reason=offline";
        assert_eq!(update_reply_value(check, "reason"), "offline");
        assert_eq!(update_reply_value(check, "update"), "unreachable");
        assert!(update_reply_value(check, "staged").is_empty());
        // A key is a whole word's start, not the end of another key.
        assert!(update_reply_value("unstaged=0.3.0", "staged").is_empty());
        assert_eq!(
            update_reply_value("notes_url=\"https://example.org/v0.3.0\"", "notes_url"),
            "https://example.org/v0.3.0"
        );

        // DOWNLOAD: "progress lines, then staged=<v>" - the last record is the answer.
        let download = "progress bytes=31000000/71000000 files=12/23\nprogress \
                        bytes=71000000/71000000 files=23/23\r\nstaged=0.3.0\n";
        assert_eq!(update_staged_of_reply(download), "0.3.0");
        assert_eq!(
            update_staged_of_reply("staged=none\nstaged=0.3.0"),
            "0.3.0"
        );
        // Nothing staged: said as none, or not said at all.
        assert!(update_staged_of_reply("staged=none reason=headless").is_empty());
        assert!(update_staged_of_reply("reason=offline").is_empty());
        assert!(update_staged_of_reply("").is_empty());
    }

    // The echo of such a line is hidden by update_logged_line; a refusal may quote the line all the
    // same, and may have folded its case to find the verb. Whatever is logged of it holds the word
    // in no case.
    #[test]
    fn what_the_verbs_say_back_to_a_line_with_a_token_typed_on_it_is_hidden_in_any_case() {
        let token = "ghp_NotARealTokenOnlyATestsWord0123456789";
        let line = format!("update token {token}");
        assert_eq!(
            update_logged_reply(
                &format!(
                    "InvalidArgument: UPDATE TOKEN takes STATUS or CLEAR [UPDATE TOKEN {}]",
                    token.to_uppercase()
                ),
                &line
            ),
            "InvalidArgument: UPDATE TOKEN takes STATUS or CLEAR [UPDATE TOKEN ***]"
        );
        assert_eq!(
            update_logged_reply(&format!("no such word: {token}"), &line),
            "no such word: ***"
        );
        assert_eq!(
            update_logged_reply(&format!("no such word: {}", token.to_lowercase()), &line),
            "no such word: ***"
        );

        // The two lines the grammar has, and every other line, hide nothing.
        assert_eq!(
            update_logged_reply("token=present", "UPDATE TOKEN STATUS"),
            "token=present"
        );
        assert_eq!(
            update_logged_reply("skipped=0.3.0", "UPDATE SKIP 0.3.0"),
            "skipped=0.3.0"
        );
        assert_eq!(
            update_logged_reply("update=current", ""),
            "update=current"
        );
    }

    #[test]
    fn the_helper_copy_of_a_recovered_update_is_listed_only_once_its_transaction_is_over() {
        // An update that a crash interrupted, put right at the next start: the recovery found its
        // folder and left none.
        let log = recovery_line(CRASHED, "ok", "");

        // The folder R/.update/<tx> is gone: the transaction is over, and its helper copy
        // U/run-<tx> is one to remove.
        let mut recovered = AskedInstall::with_gone(&[CRASHED]);
        assert_eq!(recovered.over(&log), ids(&[CRASHED]));
        assert_eq!(recovered.asked, ids(&[CRASHED]));

        // The folder is still there - the rollback failed, and the next try runs from that very
        // copy: it is not one to remove.
        let mut half_swapped = AskedInstall::default();
        assert!(
            half_swapped
                .over(&recovery_line(CRASHED, "failed", CRASHED))
                .is_empty()
        );
        assert_eq!(half_swapped.asked, ids(&[CRASHED]));
    }

    #[test]
    fn a_transaction_is_asked_about_once_however_many_recoveries_named_it() {
        // A recovery that failed, the one that then put the install right and found a second
        // interrupted update beside it, and a third run that names the second again: two
        // transactions, the first one named first.
        let log = format!(
            "2026-10-13T08:40:00Z recover found={CRASHED} result=failed left={CRASHED} \
             detail=InvalidState: an interrupted update could not be put back\n{}{}",
            recovery_line(&format!("{CRASHED},{POWER_CUT}"), "ok", ""),
            recovery_line(POWER_CUT, "ok", "")
        );

        let mut both_over = AskedInstall::with_gone(&[POWER_CUT, CRASHED]);
        assert_eq!(both_over.over(&log), ids(&[CRASHED, POWER_CUT]));
        assert_eq!(both_over.asked, ids(&[CRASHED, POWER_CUT]));

        // Only the second is over: only its copy is one to remove.
        let mut one_over = AskedInstall::with_gone(&[POWER_CUT]);
        assert_eq!(one_over.over(&log), ids(&[POWER_CUT]));
        assert_eq!(one_over.asked, ids(&[CRASHED, POWER_CUT]));

        // A long log is no more questions: 2000 lines naming one transaction ask about it once and
        // name it once.
        let long_log: String = (0..2000).map(|_| recovery_line(CRASHED, "ok", "")).collect();
        let mut often = AskedInstall::with_gone(&[CRASHED]);
        assert_eq!(often.over(&long_log), ids(&[CRASHED]));
        assert_eq!(often.asked, ids(&[CRASHED]));
    }

    #[test]
    fn only_a_whole_line_that_a_recovery_wrote_names_a_transaction() {
        let stamp = "2026-10-13T08:40:00Z ";
        let cut = &CRASHED[..12];
        let lines = [
            // The log quotes a journal record that a rollback refused, and a journal is anybody's
            // text: its second word is `journal`, whatever the record goes on to say.
            (
                "a refused journal record",
                format!(
                    "{CRASHED} journal record refused: REPLACE x recover found={POWER_CUT} \
                     result=ok left=: outside the install\n"
                ),
            ),
            // `recover` is the SECOND word and `found=` the third.
            (
                "no time",
                format!("recover found={CRASHED} result=ok left=\n"),
            ),
            (
                "found= is not the third word",
                format!("{stamp}recover result=ok found={CRASHED} left=\n"),
            ),
            (
                "another mode's line",
                format!("{stamp}apply found={CRASHED} result=ok left=\n"),
            ),
            // `result=` is the fourth: it is what says the list before it is whole.
            (
                "the list ends the line",
                format!("{stamp}recover found={CRASHED}\n"),
            ),
            (
                "no result",
                format!("{stamp}recover found={CRASHED} left=\n"),
            ),
            // A write that was cut short has no line feed, and is not read though every word of it
            // is there...
            (
                "no line feed",
                format!("{stamp}recover found={CRASHED} result=ok left="),
            ),
            // ...and one cut inside its list ends in what is left of an id, which is still hex and
            // would name a folder of its own.
            (
                "cut inside the id",
                format!("{stamp}recover found={cut}"),
            ),
        ];
        for (what, line) in lines {
            let mut install = AskedInstall::with_gone(&[CRASHED, POWER_CUT, cut]);
            assert!(install.over(&line).is_empty(), "{what}");
            assert!(install.asked.is_empty(), "{what}");
        }

        // A whole line before a torn one still counts, and the torn one does not.
        let mut install = AskedInstall::with_gone(&[CRASHED, POWER_CUT]);
        let log = format!(
            "{}{stamp}recover found={POWER_CUT} result=ok left=",
            recovery_line(CRASHED, "ok", "")
        );
        assert_eq!(install.over(&log), ids(&[CRASHED]));
        assert_eq!(install.asked, ids(&[CRASHED]));
    }

    // Beside one real id: a way out of the folder, a drive, upper-case hex, nothing at all between
    // two commas, 65 characters, and a folder's own name. None is 1 to 64 characters of 0-9 and
    // a-f, so none is asked about.
    #[test]
    fn what_is_no_transaction_id_in_a_recovery_line_names_no_folder() {
        let long = "f".repeat(65);
        let run = format!("run-{CRASHED}");
        let list =
            format!("../../outside,C:/Windows,0123456789ABCDEF,,{long},{run},{POWER_CUT}");
        let mut install = AskedInstall::with_gone(&[
            "../../outside",
            "C:/Windows",
            "0123456789ABCDEF",
            "",
            long.as_str(),
            run.as_str(),
            POWER_CUT,
        ]);
        assert_eq!(
            install.over(&recovery_line(&list, "ok", "")),
            ids(&[POWER_CUT])
        );
        assert_eq!(install.asked, ids(&[POWER_CUT]));
    }

    #[test]
    fn a_recovery_log_that_names_nothing_lists_nothing() {
        let mut install = AskedInstall::with_gone(&[CRASHED]);
        // No log, empty lines, and the line of a run that failed before it found anything (an
        // empty list).
        assert!(install.over("").is_empty());
        assert!(install.over("\n\n\n").is_empty());
        assert!(
            install
                .over(
                    "2026-10-13T08:40:00Z recover found= result=failed left= detail=Internal: \
                     looking for an interrupted update stopped on an unexpected error\n"
                )
                .is_empty()
        );
        assert!(install.asked.is_empty());

        // With nobody to say whether a transaction is over, none is known to be.
        assert!(
            update_recovered_transactions(recovery_line(CRASHED, "ok", "").as_bytes(), None)
                .is_empty()
        );
    }
}
