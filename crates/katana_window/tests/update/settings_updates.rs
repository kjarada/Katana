//! The twins of test_update_dialog.cpp's SettingsUpdates suite: the Updates page of Settings, of
//! the program and not of the drawing, which runs UPDATE lines and changes nothing itself - but
//! Save, which writes the access token to the credential store with no line, since a line is
//! echoed in the command log. The lines and the replies are the verbs' grammar and the hardening
//! pass's ruling H5 (2026-10-14), copied as the C++ test copied them.

use std::rc::Rc;

use katana_cad::Document;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::CredentialStore;
use katana_window::customisation::settings_dialog::{
    SettingsContext, SettingsUpdates, TOKEN_FIELD,
};
use katana_window::host::VerbOutcome;

use super::support::{SettingsBench, TOKEN, UpdatesFixture, carried_out, refused};

mod settings_updates {
    use super::*;

    #[test]
    fn the_page_is_the_second_of_settings_with_a_name_for_every_control() {
        let f = UpdatesFixture::new();
        let mut bench = SettingsBench::of(&f);
        let pages = bench.control("settingsPages");
        assert_eq!(pages.items.len(), 2);
        assert_eq!(pages.items[1], "Updates");
        let stack = bench.control("settingsStack");
        assert_eq!(stack.items.len(), 2);
        assert_eq!(stack.items[1], "settingsUpdates");
        assert!(bench.record("settingsUpdates").is_some());

        for name in [
            "settingsUpdateNote",
            "settingsUpdateAuto",
            "settingsUpdateSends",
            "settingsUpdateToken",
            "settingsUpdateTokenSave",
            "settingsUpdateTokenClear",
            "settingsUpdateTokenState",
            "settingsUpdateSkipped",
            "settingsUpdateUnskip",
            "settingsUpdateBlocked",
            "settingsUpdateUnblock",
        ] {
            assert!(bench.record(name).is_some(), "{name}");
        }
        // The words the design gives the controls.
        assert_eq!(
            bench.control("settingsUpdateAuto").text,
            "Check for updates automatically"
        );
        assert_eq!(
            bench.control("settingsUpdateUnskip").text,
            // A button's text in Title Case, as a menu item's (the third look audit's K).
            "Offer the Skipped Version Again"
        );
        assert_eq!(bench.control("settingsUpdateTokenSave").text, "Save");
        assert_eq!(bench.control("settingsUpdateTokenClear").text, "Clear");
        // A token is typed as dots.
        const { assert!(TOKEN_FIELD.password) };

        // Turned to as the headless driver's settingsPages=Updates turns to it.
        bench.turn_to("Updates");
        assert_eq!(bench.control("settingsStack").value, "settingsUpdates");
        assert!(bench.ran().is_empty(), "showing the page runs nothing");
    }

    #[test]
    fn the_page_says_what_a_check_sends_and_that_the_server_sees_the_address() {
        let f = UpdatesFixture::new();
        let bench = SettingsBench::of(&f);
        assert_eq!(
            bench.shown("settingsUpdateSends"),
            "A check asks GitHub for the newest version. It sends only \"Katana/0.2.0 (win64)\" \
             as its user agent - nothing about you, your files or your drawings - and, as with \
             every website, GitHub sees this computer's network address."
        );

        // With nobody to say which server or what user agent, the shape of both.
        let mut bare = UpdatesFixture::new();
        bare.updates.user_agent.clear();
        bare.updates.server.clear();
        let shape = SettingsBench::of(&bare);
        assert_eq!(
            shape.shown("settingsUpdateSends"),
            "A check asks the release server for the newest version. It sends only \
             \"Katana/<version> (<target>)\" as its user agent - nothing about you, your files or \
             your drawings - and, as with every website, the release server sees this \
             computer's network address."
        );
    }

    #[test]
    fn the_box_shows_what_the_state_has_and_toggling_it_runs_its_line_once() {
        let f = UpdatesFixture::new();
        let mut bench = SettingsBench::of(&f);
        // The verb, as far as the page can tell: it writes the state.
        let automatic = Rc::clone(&f.automatic);
        bench.answer(move |line| {
            if line == "UPDATE SET AUTO OFF" {
                automatic.set(false);
            } else if line == "UPDATE SET AUTO ON" {
                automatic.set(true);
            }
            carried_out()
        });
        assert!(
            bench.checked("settingsUpdateAuto"),
            "on by default, as the state has it"
        );
        assert!(bench.ran().is_empty(), "showing the state runs nothing");

        bench.press("settingsUpdateAuto");
        assert_eq!(bench.ran(), ["UPDATE SET AUTO OFF"]);
        assert!(!bench.checked("settingsUpdateAuto"));
        assert_eq!(bench.status(), "> UPDATE SET AUTO OFF\nDone.");
        assert!(!bench.says_a_refusal());

        // Set as the headless driver's --fill sets a box: toggled, not clicked.
        bench.fill("settingsUpdateAuto", "on");
        assert_eq!(bench.ran(), ["UPDATE SET AUTO OFF", "UPDATE SET AUTO ON"]);
        assert!(bench.checked("settingsUpdateAuto"));
    }

    #[test]
    fn a_box_whose_line_was_refused_goes_back_to_what_the_state_has() {
        let f = UpdatesFixture::new();
        let mut bench = SettingsBench::of(&f);
        bench.answer(|_| refused("Unsupported: a headless run keeps no update settings"));
        assert!(bench.checked("settingsUpdateAuto"));
        bench.press("settingsUpdateAuto");
        assert_eq!(bench.ran(), ["UPDATE SET AUTO OFF"], "once, not again");
        assert!(
            bench.checked("settingsUpdateAuto"),
            "the state still has it on"
        );
        assert_eq!(
            bench.status(),
            "> UPDATE SET AUTO OFF\nRefused: Unsupported: a headless run keeps no update settings"
        );
        assert!(bench.says_a_refusal());
    }

    #[test]
    fn turning_to_the_page_reads_the_state_again_and_runs_nothing() {
        let f = UpdatesFixture::new();
        let mut bench = SettingsBench::of(&f);
        assert!(bench.checked("settingsUpdateAuto"));
        // Changed elsewhere since: a typed UPDATE SET AUTO OFF, the update dialog's Skip.
        f.automatic.set(false);
        *f.skipped.borrow_mut() = String::from("0.3.0");
        bench.turn_to("Updates");
        assert!(!bench.checked("settingsUpdateAuto"));
        assert_eq!(
            bench.shown("settingsUpdateSkipped"),
            "Katana 0.3.0 is skipped: it is not offered."
        );
        assert!(bench.ran().is_empty(), "{:?}", bench.ran());
    }

    #[test]
    fn save_writes_the_credential_store_and_never_produces_a_line() {
        let f = UpdatesFixture::new();
        let mut bench = SettingsBench::of(&f);
        assert_eq!(
            bench.shown("settingsUpdateTokenState"),
            "No access token is stored."
        );
        // Pasted with the blanks a copy brings: they are no part of the token.
        bench.fill("settingsUpdateToken", &format!("  {TOKEN} \n"));
        bench.press("settingsUpdateTokenSave");

        assert_eq!(*f.store.written.borrow(), [TOKEN]);
        // A token on a line would be echoed in the command log: there is no line.
        assert!(bench.ran().is_empty(), "Save ran a line");
        assert!(bench.logged().is_empty(), "Save logged something");
        assert_eq!(f.token_changes.get(), 1);
        assert!(
            bench.text("settingsUpdateToken").is_empty(),
            "the field is emptied"
        );
        assert_eq!(
            bench.status(),
            "The access token was saved to the credential manager. It is not shown again."
        );
        assert!(!bench.says_a_refusal());
        assert_eq!(
            bench.shown("settingsUpdateTokenState"),
            "An access token is stored in the credential manager."
        );
        // And nothing the dialog says anywhere holds it.
        assert!(!bench.everything_said().contains(TOKEN));
    }

    #[test]
    fn a_token_the_store_refuses_is_said_in_the_stores_words_and_never_shown() {
        let f = UpdatesFixture::new();
        // As a store refuses: the message names the rule and the place, never the value.
        *f.store.refusal.borrow_mut() = Some(Error::new(
            ErrorCode::InvalidArgument,
            "the access token has a space inside it",
        ));
        let mut bench = SettingsBench::of(&f);
        bench.fill("settingsUpdateToken", TOKEN);
        bench.press("settingsUpdateTokenSave");

        assert!(f.store.written.borrow().is_empty());
        assert!(bench.ran().is_empty());
        assert_eq!(f.token_changes.get(), 0);
        assert_eq!(
            bench.status(),
            "Save: the access token has a space inside it"
        );
        assert!(bench.says_a_refusal());
        // Logged, as every refusal the dialog words itself is - without the token.
        let logged = bench.logged();
        assert_eq!(logged.len(), 1);
        assert!(logged[0].1);
        assert!(!bench.everything_said().contains(TOKEN));
        // What was typed stays, to be corrected.
        assert_eq!(bench.text("settingsUpdateToken"), TOKEN);
    }

    #[test]
    fn save_with_nothing_typed_writes_nothing_and_says_why() {
        let f = UpdatesFixture::new();
        let mut bench = SettingsBench::of(&f);
        for typed in ["", "   "] {
            bench.fill("settingsUpdateToken", typed);
            bench.press("settingsUpdateTokenSave");
            assert!(f.store.written.borrow().is_empty());
            assert_eq!(
                bench.status(),
                "Save: no access token is typed. Paste one, or press Clear to remove the stored \
                 one."
            );
            assert!(bench.says_a_refusal());
        }
        assert!(bench.ran().is_empty());
        assert_eq!(f.token_changes.get(), 0);
    }

    #[test]
    fn clear_runs_its_line_and_leaves_no_token_typed_either() {
        let f = UpdatesFixture::new();
        *f.store.token.borrow_mut() = Some(String::from(TOKEN));
        let mut bench = SettingsBench::of(&f);
        // The verb, as far as the page can tell: it empties the store.
        let store = Rc::clone(&f.store);
        bench.answer(move |_| {
            *store.token.borrow_mut() = None;
            VerbOutcome::new(true, "token=absent", "")
        });
        assert_eq!(
            bench.shown("settingsUpdateTokenState"),
            "An access token is stored in the credential manager."
        );
        bench.fill("settingsUpdateToken", "half a token");
        bench.press("settingsUpdateTokenClear");

        assert_eq!(bench.ran(), ["UPDATE TOKEN CLEAR"]);
        assert_eq!(
            f.store.cleared.get(),
            0,
            "the verb clears the store, not the dialog"
        );
        assert!(bench.text("settingsUpdateToken").is_empty());
        assert_eq!(bench.status(), "> UPDATE TOKEN CLEAR\ntoken=absent");
        assert_eq!(
            bench.shown("settingsUpdateTokenState"),
            "No access token is stored."
        );
    }

    #[test]
    fn offer_the_skipped_version_again_runs_its_line_and_is_off_when_none_is_skipped() {
        let f = UpdatesFixture::new();
        *f.skipped.borrow_mut() = String::from("0.3.0");
        let mut bench = SettingsBench::of(&f);
        let skipped = Rc::clone(&f.skipped);
        bench.answer(move |_| {
            skipped.borrow_mut().clear();
            carried_out()
        });
        assert_eq!(
            bench.shown("settingsUpdateSkipped"),
            "Katana 0.3.0 is skipped: it is not offered."
        );
        assert!(bench.enabled("settingsUpdateUnskip"));
        bench.press("settingsUpdateUnskip");
        assert_eq!(bench.ran(), ["UPDATE SKIP NONE"]);
        assert_eq!(
            bench.shown("settingsUpdateSkipped"),
            "No version is skipped."
        );
        assert!(
            !bench.enabled("settingsUpdateUnskip"),
            "nothing left to offer again"
        );
    }

    // ---- the version that was put back (the hardening pass's ruling H5, 2026-10-14) ------------

    #[test]
    fn offer_the_version_that_was_put_back_again_is_a_button_under_the_skipped_versions_one() {
        let f = UpdatesFixture::new();
        let mut bench = SettingsBench::of(&f);
        assert!(bench.record("settingsUpdates").is_some());
        let unblock = bench.control("settingsUpdateUnblock");
        // A button's text in Title Case, as a menu item's (the third look audit's K).
        assert_eq!(unblock.text, "Offer the Version That Was Put Back Again");
        // It says what it does, and by which line.
        let tip = bench.dialog.tip("settingsUpdateUnblock");
        assert!(tip.contains("UPDATE BLOCKED CLEAR"), "{tip}");
        assert!(bench.record("settingsUpdateBlocked").is_some());

        // Like every button of Settings it is no default: Enter presses nothing.
        bench
            .harness
            .key(egui::Key::Enter, egui::Modifiers::NONE);
        bench.draw();
        assert!(bench.ran().is_empty());

        // Under the skipped version's: as a person sees the page, turned to and shown, it is the
        // lower one.
        bench.turn_to("Updates");
        let unblock = bench.control("settingsUpdateUnblock");
        let unskip = bench.control("settingsUpdateUnskip");
        assert!(unblock.visible);
        assert!(
            unblock.rect.min.y > unskip.rect.min.y,
            "{:?} over {:?}",
            unblock.rect,
            unskip.rect
        );

        assert!(bench.ran().is_empty(), "showing the page runs nothing");
    }

    #[test]
    fn offer_the_version_that_was_put_back_again_builds_exactly_its_line_and_nothing_else() {
        let f = UpdatesFixture::new();
        f.put_back.set(true);
        *f.skipped.borrow_mut() = String::from("0.3.0");
        *f.store.token.borrow_mut() = Some(String::from(TOKEN));
        let mut bench = SettingsBench::of(&f);
        // The verb, as far as the page can tell: it empties the list, and answers as the ruling
        // gives its reply.
        let put_back = Rc::clone(&f.put_back);
        bench.answer(move |_| {
            put_back.set(false);
            VerbOutcome::new(true, "blocked=none", "")
        });
        assert_eq!(
            bench.shown("settingsUpdateBlocked"),
            "A version that was installed here did not start and was put back: it is not \
             offered."
        );
        assert!(bench.enabled("settingsUpdateUnblock"));
        assert!(bench.ran().is_empty(), "showing the state runs nothing");

        bench.press("settingsUpdateUnblock");
        // The one line, once, as a person would type it.
        assert_eq!(bench.ran(), ["UPDATE BLOCKED CLEAR"]);
        // And nothing else: the dialog writes no store, tells nobody the token changed, and words
        // nothing of its own into the log.
        assert!(f.store.written.borrow().is_empty());
        assert_eq!(f.store.cleared.get(), 0);
        assert_eq!(f.token_changes.get(), 0);
        assert!(bench.logged().is_empty());
        assert_eq!(bench.status(), "> UPDATE BLOCKED CLEAR\nblocked=none");
        assert!(!bench.says_a_refusal());
        // The page shows what the state has now.
        assert_eq!(
            bench.shown("settingsUpdateBlocked"),
            "No version was put back."
        );
        assert!(
            !bench.enabled("settingsUpdateUnblock"),
            "nothing left to offer again"
        );
        // The skipped version is another list, and this line is not its line.
        assert_eq!(
            bench.shown("settingsUpdateSkipped"),
            "Katana 0.3.0 is skipped: it is not offered."
        );
        assert!(bench.enabled("settingsUpdateUnskip"));

        // Each of the two buttons runs its own line and not the other's.
        bench.press("settingsUpdateUnskip");
        assert_eq!(bench.ran(), ["UPDATE BLOCKED CLEAR", "UPDATE SKIP NONE"]);
    }

    // The state file reads, says a release was put back, and cannot be written: the verb refuses
    // the line, and the list is as it was.
    #[test]
    fn a_line_that_was_refused_leaves_the_version_that_was_put_back_as_the_state_has_it() {
        let f = UpdatesFixture::new();
        f.put_back.set(true);
        let mut bench = SettingsBench::of(&f);
        bench.answer(|_| refused("InvalidState: the update settings could not be written"));
        bench.press("settingsUpdateUnblock");
        assert_eq!(bench.ran(), ["UPDATE BLOCKED CLEAR"], "once, not again");
        assert_eq!(
            bench.status(),
            "> UPDATE BLOCKED CLEAR\nRefused: InvalidState: the update settings could not be \
             written"
        );
        assert!(bench.says_a_refusal());
        assert_eq!(
            bench.shown("settingsUpdateBlocked"),
            "A version that was installed here did not start and was put back: it is not \
             offered."
        );
        assert!(
            bench.enabled("settingsUpdateUnblock"),
            "it can be tried again"
        );
    }

    #[test]
    fn the_button_for_a_version_that_was_put_back_is_off_with_none_and_on_when_nobody_says() {
        // None was put back: nothing to do.
        let none = UpdatesFixture::new();
        let mut without = SettingsBench::of(&none);
        assert_eq!(
            without.shown("settingsUpdateBlocked"),
            "No version was put back."
        );
        assert!(!without.enabled("settingsUpdateUnblock"));
        without.press("settingsUpdateUnblock");
        assert!(without.ran().is_empty(), "a disabled button runs nothing");

        // Changed elsewhere since: the page reads the state again when it is turned to, and runs
        // nothing.
        none.put_back.set(true);
        without.turn_to("Updates");
        assert!(without.enabled("settingsUpdateUnblock"));
        assert!(without.ran().is_empty());

        // Nobody says whether one was (a headless window keeps no state): the page says nothing of
        // it, and the button stays on for the verb to answer.
        let mut unsaid = UpdatesFixture::new();
        unsaid.updates.put_back = None;
        let mut asking = SettingsBench::of(&unsaid);
        assert!(asking.shown("settingsUpdateBlocked").is_empty());
        assert!(asking.enabled("settingsUpdateUnblock"));
        asking.press("settingsUpdateUnblock");
        assert_eq!(asking.ran(), ["UPDATE BLOCKED CLEAR"]);

        // No executor: nothing can run the line, whatever the state has.
        let mut mute = UpdatesFixture::new();
        mute.put_back.set(true);
        mute.runs = false;
        let silent = SettingsBench::of(&mute);
        assert!(!silent.enabled("settingsUpdateUnblock"));
        // And with nothing given at all, as every control of the page.
        let bare = SettingsBench::new(SettingsContext::default(), UpdatesFixture::host());
        assert!(!bare.enabled("settingsUpdateUnblock"));
        assert!(bare.shown("settingsUpdateBlocked").is_empty());
    }

    // U/state.json is there and does not read: nobody can say what is skipped or put back, and
    // the page is told so. It said "No version was put back." and switched the button off - a
    // guess, and one that kept a person from the verb's refusal, which names the cause.
    #[test]
    fn a_state_that_does_not_read_is_not_guessed_at_and_the_buttons_let_the_verb_answer() {
        let mut f = UpdatesFixture::new();
        f.updates.skipped_version = Some(Rc::new(|| None));
        f.updates.put_back = Some(Rc::new(|| None));
        let mut bench = SettingsBench::of(&f);
        bench.answer(|_| refused("InvalidState: the update settings could not be read"));
        assert!(bench.shown("settingsUpdateBlocked").is_empty());
        assert!(bench.shown("settingsUpdateSkipped").is_empty());
        assert!(bench.enabled("settingsUpdateUnblock"));
        assert!(bench.enabled("settingsUpdateUnskip"));
        assert!(bench.ran().is_empty(), "showing the page runs nothing");

        bench.press("settingsUpdateUnblock");
        assert_eq!(bench.ran(), ["UPDATE BLOCKED CLEAR"]);
        assert_eq!(
            bench.status(),
            "> UPDATE BLOCKED CLEAR\nRefused: InvalidState: the update settings could not be read"
        );
        assert!(bench.says_a_refusal());
        // Still nothing known, and still there to press once the file is put right.
        assert!(bench.shown("settingsUpdateBlocked").is_empty());
        assert!(bench.enabled("settingsUpdateUnblock"));

        // The skipped version's button likewise, by its own line.
        bench.press("settingsUpdateUnskip");
        assert_eq!(bench.ran(), ["UPDATE BLOCKED CLEAR", "UPDATE SKIP NONE"]);
        assert_eq!(
            bench.status(),
            "> UPDATE SKIP NONE\nRefused: InvalidState: the update settings could not be read"
        );
        assert!(bench.shown("settingsUpdateSkipped").is_empty());
        assert!(bench.enabled("settingsUpdateUnskip"));
    }

    #[test]
    fn a_store_that_cannot_be_read_says_so_in_its_own_words() {
        // Not NotFound: there is something there and it is no usable token.
        struct Unreadable;
        impl CredentialStore for Unreadable {
            fn read(&self) -> Result<String> {
                Err(Error::new(
                    ErrorCode::InvalidState,
                    "the access token in the credential manager is not a usable token",
                ))
            }
            fn write(&self, _token: &[u8]) -> Status {
                Ok(())
            }
            fn clear(&self) -> Status {
                Ok(())
            }
            fn r#where(&self) -> &str {
                "credential manager"
            }
        }
        let mut f = UpdatesFixture::new();
        f.updates.credentials = Some(Rc::new(Unreadable));
        let bench = SettingsBench::of(&f);
        assert_eq!(
            bench.shown("settingsUpdateTokenState"),
            "the access token in the credential manager is not a usable token"
        );
    }

    #[test]
    fn with_nothing_given_the_pages_controls_are_off_and_it_says_why_no_token_is_kept() {
        // Every member of the context at its default: no executor, no state, no store.
        let bench = SettingsBench::new(SettingsContext::default(), UpdatesFixture::host());
        for name in [
            "settingsUpdateAuto",
            "settingsUpdateToken",
            "settingsUpdateTokenSave",
            "settingsUpdateTokenClear",
            "settingsUpdateUnskip",
        ] {
            assert!(!bench.enabled(name), "{name}");
        }
        assert!(
            !bench.checked("settingsUpdateAuto"),
            "nobody says it is on: it is not shown as on"
        );
        assert_eq!(
            bench.shown("settingsUpdateTokenState"),
            "This session keeps no access token; the environment variable KATANA_UPDATE_TOKEN \
             gives a run one."
        );
        assert!(bench.shown("settingsUpdateSkipped").is_empty());
        // Its own flag: hidden while it has nothing to say.
        assert!(!bench.control("settingsUpdateNote").visible);
    }

    // The Format workbench makes the dialog and knows nothing of updates: the window gives the
    // page its services once the dialog is there.
    #[test]
    fn what_the_window_hands_over_later_fills_the_page_and_the_note_is_shown_when_there_is_one() {
        let f = UpdatesFixture::new();
        let mut without_updates = f.context();
        without_updates.updates = SettingsUpdates::default();
        let mut bench = SettingsBench::new(without_updates, UpdatesFixture::host());
        assert!(!bench.enabled("settingsUpdateAuto"));
        assert!(!bench.enabled("settingsUpdateTokenSave"));

        let mut updates = f.updates.clone();
        updates.note = String::from(
            "The environment variable KATANA_UPDATE_DISABLE is set: no automatic check runs in \
             this session.",
        );
        bench.dialog.set_updates(updates.clone());
        bench.draw();
        assert!(bench.enabled("settingsUpdateAuto"));
        assert!(bench.checked("settingsUpdateAuto"));
        assert!(bench.enabled("settingsUpdateTokenSave"));
        assert!(bench.control("settingsUpdateNote").visible);
        assert_eq!(bench.shown("settingsUpdateNote"), updates.note);
        assert!(bench.ran().is_empty(), "filling the page runs nothing");

        updates.note.clear();
        bench.dialog.set_updates(updates.clone());
        bench.draw();
        assert!(!bench.control("settingsUpdateNote").visible);

        // And on the page as a person sees it: turned to, the note is shown.
        updates.note = String::from("A note.");
        bench.dialog.set_updates(updates);
        bench.draw();
        bench.turn_to("Updates");
        assert!(bench.control("settingsUpdateNote").visible);
    }

    // The first page stops acting when its drawing goes; this one is of the program and goes on.
    #[test]
    fn the_page_works_with_no_drawing_and_after_one_has_closed() {
        let f = UpdatesFixture::new();
        let mut context = f.context();
        context.reads_drawing = true;
        let mut bench = SettingsBench::new(context, UpdatesFixture::host());
        // The drawing goes: another is put in its place, and the dialog's is no more.
        bench.host.parts.document = Document::new(None);
        bench.dialog.refresh(&bench.host);
        bench.draw();
        assert_eq!(
            bench.status(),
            "The drawing this dialog worked on has closed: nothing here acts on anything any more."
        );
        for name in [
            "settingsUpdateAuto",
            "settingsUpdateTokenSave",
            "settingsUpdateTokenClear",
        ] {
            assert!(bench.enabled(name), "{name}");
        }
        bench.press("settingsUpdateTokenClear");
        assert_eq!(bench.ran(), ["UPDATE TOKEN CLEAR"]);
        assert_eq!(bench.status(), "> UPDATE TOKEN CLEAR\nDone.");
    }

    #[test]
    fn with_no_executor_a_token_is_still_saved_since_save_runs_no_line() {
        let mut f = UpdatesFixture::new();
        f.runs = false;
        let mut bench = SettingsBench::of(&f);
        assert!(!bench.enabled("settingsUpdateAuto"));
        assert!(!bench.enabled("settingsUpdateTokenClear"));
        assert!(bench.enabled("settingsUpdateTokenSave"));
        bench.fill("settingsUpdateToken", TOKEN);
        bench.press("settingsUpdateTokenSave");
        assert_eq!(*f.store.written.borrow(), [TOKEN]);
    }
}
