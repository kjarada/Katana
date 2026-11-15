//! The twins of test_settings_dialog.cpp's SettingsDialog cases of the built-in library (the
//! chooser settingsBuiltIn and Use, settingsBuiltInUse; SITE the default and PEGS_TWO beside it -
//! SITE 3 definitions and 3 rules over 2 codes, PEGS_TWO none and 2 rules over 1 code), and of the
//! verb itself: the lines read by the interpreter over the same drawing, and what a press does to
//! the kept file on the disk - when it is written, when it is set aside as .bak, that ONE such
//! file is kept (docs/customisation.md, "The host: RESET, KEEP and REVERT"), worked out by hand
//! for each press in the C++ test.

use katana_cad::customisation_host::{
    built_ins, install_built_in_customisation, read_customisation_file, start_customisation,
};
use katana_cad::{CustomisationOrigin, Document};
use katana_entity::Customisation;

use crate::fixture_customisation::{customisation_fixture_file, customisation_from_text};
use crate::support::{
    Fixture, KEEP, KeptSession, LibrarySession, PEGS_TWO, automation, carried_out,
    customisation_in_file, exists, native, path_text, site_and_pegs,
};
use crate::window_support::Scratch;

/// PEGS_TWO installed as a built-in, as CUSTOMISE BUILTIN does to the drawing.
fn install_pegs_two(document: &mut Document, kept: bool) {
    document
        .install_customisation(
            customisation_from_text(PEGS_TWO),
            CustomisationOrigin::BuiltIn,
            kept,
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

mod settings_dialog {
    use super::*;

    // ---- the built-in library -------------------------------------------------------------------

    #[test]
    fn the_chooser_lists_the_built_ins_the_default_marked_and_stands_on_the_one_the_session_is() {
        let mut f = Fixture::new();
        f.start_on_site();
        let bench = f.open();

        assert!(bench.visible("settingsBuiltInRow"));
        assert_eq!(bench.chooser_items(), ["Site (default)", "Pegs Two"]);
        assert_eq!(bench.dialog.built_in_names(), ["Site", "Pegs Two"]);
        // What a person is told of one before choosing it: its own description, what it holds and
        // the file it was compiled in from.
        assert_eq!(
            bench.dialog.built_in_tips()[0],
            "3 definitions, 3 survey code rules over 2 codes\nCompiled in from \
             site.customisation.json"
        );
        assert_eq!(
            bench.dialog.built_in_tips()[1],
            "Two rules for one code\n0 definitions, 2 survey code rules over 1 code\nCompiled in \
             from pegs.customisation.json"
        );
        // It stands on the one the session is, which Use has nothing to do for.
        assert_eq!(bench.dialog.built_in_index(), Some(0));
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Site (default)");
        assert!(!bench.enabled("settingsBuiltInUse"));
        assert_eq!(
            bench.tip("settingsBuiltInUse"),
            "The session is this built-in customisation already."
        );
        assert_eq!(bench.control("settingsBuiltInUse").text, "Use");
    }

    #[test]
    fn the_built_in_row_is_hidden_when_there_is_nothing_to_choose_between() {
        // None, one: nothing to choose between. Two: the row is there.
        for offered in [0_usize, 1, 2] {
            let mut f = Fixture::new();
            let mut entries = site_and_pegs();
            entries.truncate(offered);
            f.context.built_ins = Some(Box::new(move || entries.clone()));
            let bench = f.open();
            assert_eq!(
                !bench.visible("settingsBuiltInRow"),
                offered < 2,
                "{offered} offered"
            );
            assert_eq!(bench.chooser_items().len(), offered);
            assert!(!(bench.enabled("settingsBuiltInUse") && offered == 0));
        }
    }

    #[test]
    fn a_dialog_given_no_list_offers_what_this_program_compiled_in() {
        let mut f = Fixture::new();
        f.context.built_ins = None; // what the window gives
        let bench = f.open();
        let compiled_in = built_ins();
        assert_eq!(bench.chooser_items().len(), compiled_in.len());
        for (at, entry) in compiled_in.iter().enumerate() {
            assert_eq!(bench.dialog.built_in_names()[at], entry.name);
        }
        assert_eq!(
            !bench.visible("settingsBuiltInRow"),
            compiled_in.len() < 2
        );
    }

    #[test]
    fn use_builds_the_line_with_the_name_quoted_and_keep_follows_a_kept_session() {
        let mut f = Fixture::new();
        f.start_on_site();
        let mut bench = f.open();
        // Each line does to the Document what its verb does: the named built-in installed, not
        // kept (it is not the run's own), and a KEEP kept.
        bench.answer(|parts, line| {
            if line == KEEP {
                parts.document.set_customisation_kept(true);
            } else if line == "CUSTOMISE BUILTIN \"Pegs Two\"" {
                install_pegs_two(&mut parts.document, false);
            }
            carried_out()
        });
        bench.choose(1);
        assert!(bench.enabled("settingsBuiltInUse"));
        assert_eq!(
            bench.tip("settingsBuiltInUse"),
            "Put the chosen built-in customisation in the place of the session's (CUSTOMISE \
             BUILTIN).\nThis session is the kept one, so the choice is kept too: the kept file is \
             written with it, and what was there is set aside as C:/kept \
             files/customisation.json.bak, replacing the file of that name.\nImport that file \
             with Replace instead of merging ticked to have it back."
        );
        bench.press("settingsBuiltInUse");
        assert_eq!(bench.ran(), ["CUSTOMISE BUILTIN \"Pegs Two\"", KEEP]);
        assert!(!bench.says_a_refusal());
        assert_eq!(
            bench.status(),
            "> CUSTOMISE BUILTIN \"Pegs Two\"\nDone.\n> CUSTOMISE KEEP\nDone."
        );

        // The page follows the Document: the built-in the session is, now the chosen one, and Use
        // has nothing more to do for it.
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Pegs Two");
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Pegs Two");
        assert_eq!(
            bench.dialog.built_in_index(),
            Some(1),
            "it stays where the person put it"
        );
        assert!(!bench.enabled("settingsBuiltInUse"));

        // A name of one word is one word.
        bench.clear_ran();
        bench.choose(0);
        assert!(bench.enabled("settingsBuiltInUse"));
        bench.press("settingsBuiltInUse");
        assert!(!bench.ran().is_empty());
        assert_eq!(bench.ran()[0], "CUSTOMISE BUILTIN Site");
    }

    #[test]
    fn use_says_before_it_is_pressed_what_keeping_the_choice_does_to_the_kept_file() {
        let plain = "Put the chosen built-in customisation in the place of the session's \
                     (CUSTOMISE BUILTIN)";
        let way = "Import that file with Replace instead of merging ticked to have it back.";
        let aside = "C:/kept files/customisation.json.bak";
        {
            // A kept session, the other built-in chosen: the choice is written to the kept file,
            // and what was there is put away.
            let mut f = Fixture::new();
            f.start_on_site();
            let mut bench = f.open();
            bench.choose(1);
            assert_eq!(
                bench.tip("settingsBuiltInUse"),
                format!(
                    "{plain}.\nThis session is the kept one, so the choice is kept too: the kept \
                     file is written with it, and what was there is set aside as {aside}, \
                     replacing the file of that name.\n{way}"
                )
            );
        }
        {
            // The built-in of the run chosen, over a session that is another and kept: KEEP writes
            // nothing for it and sets the kept file aside. The tip follows the chooser.
            let mut f = Fixture::new();
            f.start_on_site();
            let mut bench = f.open();
            install_pegs_two(bench.document_mut(), true);
            bench.draw();
            bench.choose(0);
            assert!(bench.enabled("settingsBuiltInUse"));
            assert_eq!(
                bench.tip("settingsBuiltInUse"),
                format!(
                    "{plain}.\nThis session is the kept one, so the choice is kept too: that is \
                     the built-in every start gives when nothing is kept, so the kept file is set \
                     aside as {aside}, replacing the file of that name, and Revert to Kept then \
                     has none to read.\n{way}"
                )
            );
        }
        {
            // Not the kept one: nothing follows the press and the kept file stands.
            let mut f = Fixture::new();
            f.start_on_site();
            f.document_mut().set_automation(automation(false, true));
            let mut bench = f.open();
            bench.choose(1);
            assert_eq!(
                bench.tip("settingsBuiltInUse"),
                format!(
                    "{plain}.\nThis session is not the kept one, so the choice is not kept: the \
                     kept file stands."
                )
            );
        }
        {
            // A host that keeps no file has none to put away.
            let mut f = Fixture::new();
            f.context.kept_file.clear();
            f.start_on_site();
            let mut bench = f.open();
            bench.choose(1);
            assert_eq!(bench.tip("settingsBuiltInUse"), plain);
        }
    }

    #[test]
    fn use_on_a_session_that_was_not_kept_is_not_followed_by_keep() {
        let mut f = Fixture::new();
        f.start_on_site();
        f.document_mut().set_automation(automation(false, true));
        assert!(!f.document().customisation_state().kept);
        let mut bench = f.open();
        bench.choose(1);
        bench.press("settingsBuiltInUse");
        assert_eq!(bench.ran(), ["CUSTOMISE BUILTIN \"Pegs Two\""]);
    }

    #[test]
    fn use_is_refused_while_the_code_manager_holds_unapplied_edits_and_runs_nothing() {
        let mut f = Fixture::new();
        f.start_on_site();
        let mut bench = f.open();
        bench.choose(1);
        bench.dirty.set(true);
        bench.press("settingsBuiltInUse");
        assert!(bench.ran().is_empty());
        assert!(bench.says_a_refusal());
        assert!(
            bench.status().starts_with(
                "Use Built-in was not run: the Survey Code Manager has rule edits that are not on \
                 the drawing"
            ),
            "{}",
            bench.status()
        );
        let logged = bench.logged();
        assert_eq!(logged.len(), 1);
        assert!(logged[0].1);

        bench.dirty.set(false);
        bench.press("settingsBuiltInUse");
        assert_eq!(bench.ran().len(), 1);
    }

    #[test]
    fn the_choice_is_kept_when_the_document_changes_and_use_needs_a_runner() {
        let mut f = Fixture::new();
        f.start_on_site();
        let mut bench = f.open();
        bench.choose(1);
        // The session changes under the dialog: the page is read again, and the person's choice
        // stands.
        bench.document_mut().set_automation(automation(false, true));
        bench.draw();
        assert_eq!(bench.dialog.built_in_index(), Some(1));
        assert_eq!(bench.chosen_name(), "Pegs Two");
        // The session is an edited copy now: the built-in it was made from.
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Based on Site");

        // With nothing to run its line, Use is disabled rather than refused.
        let mut without = Fixture::new();
        without.start_on_site();
        without.context.run = None;
        let mut bare = without.open();
        bare.choose(1);
        assert!(!bare.enabled("settingsBuiltInUse"));
    }

    #[test]
    fn the_built_in_row_says_what_the_session_is_or_what_it_was_made_from() {
        let mut f = Fixture::new();
        f.offer_site_and_pegs();
        let mut bench = f.open();
        // Nothing installed.
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "None");

        // A customisation loaded from a file says nothing of a built-in.
        bench.install(CustomisationOrigin::Loaded, false);
        bench.draw();
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "None");

        // A built-in, installed as a start installs it: itself, and the default.
        bench
            .document_mut()
            .set_built_in_customisation_name(String::from("Site"));
        install_built_in_customisation(bench.document_mut(), &site_and_pegs()[0].built_in, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Site (default)");
        // The other one is the built-in the session is, and not the default.
        install_built_in_customisation(bench.document_mut(), &site_and_pegs()[1].built_in, false)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Pegs Two");
        // Edited, it is a copy of that one.
        bench.document_mut().set_automation(automation(false, true));
        bench.draw();
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Based on Pegs Two");
    }

    // ---- the built-in library, with the verb itself ---------------------------------------------

    #[test]
    fn with_the_verb_itself_an_alternate_built_in_chosen_in_settings_is_kept_and_the_default_retires_it()
     {
        let session = LibrarySession::start("settings-library-alternate");
        let host = session.host.clone();
        let kept_file = session.kept_file.clone();
        let set_aside = session.set_aside.clone();
        let mut bench = session.fixture.open();
        assert_eq!(bench.shown("settingsActiveName"), "Site");
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Site (default)");

        // Use the other one. The session was kept (it is the default, which a start gives) and is
        // not after the line, so KEEP follows: written, and it says which built-in it is based on.
        bench.choose(1);
        bench.press("settingsBuiltInUse");
        assert_eq!(bench.ran(), ["CUSTOMISE BUILTIN \"Pegs Two\"", KEEP]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(
            bench.status().contains(
                "used name=\"Pegs Two\" default=no definitions=0 codes=1 rules=2 kept=no"
            ),
            "{}",
            bench.status()
        );
        assert!(
            bench.status().contains("written=yes name=\"Pegs Two\""),
            "{}",
            bench.status()
        );
        assert!(exists(&kept_file));
        let kept = customisation_in_file(&kept_file);
        assert_eq!(kept.name, "Pegs Two");
        assert_eq!(
            kept.based_on.as_ref().map(|base| base.name.as_str()),
            Some("Pegs Two")
        );
        bench.draw();
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Pegs Two");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Built in");
        assert!(bench.shown("settingsActiveKept").starts_with("Kept:"));

        // A window started again comes up as that built-in, and says nothing of another one.
        {
            let mut restarted = Document::new(None);
            let next = start_customisation(&mut restarted, &host);
            assert_eq!(next.installed, CustomisationOrigin::Kept);
            assert_eq!(next.name, "Pegs Two");
            assert!(!next.kept_from_another_built_in);
            assert!(next.problems.is_empty());
        }

        // The default again: its use is the session being the built-in of the run, so the kept
        // file is retired, and the way back is the file set aside.
        bench.clear_ran();
        bench.choose(0);
        bench.press("settingsBuiltInUse");
        assert_eq!(bench.ran(), ["CUSTOMISE BUILTIN Site", KEEP]);
        assert!(
            bench
                .status()
                .contains("written=no reason=the-session-is-the-built-in"),
            "{}",
            bench.status()
        );
        assert!(!exists(&kept_file));
        assert!(exists(&set_aside));
        assert_eq!(customisation_in_file(&set_aside).name, "Pegs Two");
        bench.draw();
        assert_eq!(bench.shown("settingsActiveBuiltIn"), "Site (default)");
        {
            let mut restarted = Document::new(None);
            assert_eq!(
                start_customisation(&mut restarted, &host).installed,
                CustomisationOrigin::BuiltIn
            );
        }
        drop(session.scratch);
    }

    // Reset to Built-in and Use of the default give the same session.
    #[test]
    fn reset_to_built_in_and_use_of_the_default_leave_the_same_session() {
        let mut a = LibrarySession::start("settings-library-reset-a");
        let mut b = LibrarySession::start("settings-library-reset-b");
        for session in [&mut a, &mut b] {
            let parts = &mut session.fixture.host.parts;
            parts
                .interpreter
                .run(&mut parts.document, "CUSTOMISE BUILTIN \"Pegs Two\"")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        }
        let mut first = a.fixture.open();
        let mut second = b.fixture.open();
        first.draw();
        second.draw();
        first.press("settingsReset");
        second.choose(0);
        second.press("settingsBuiltInUse");
        first.draw();
        second.draw();
        assert!(first.document().customisation() == second.document().customisation());
        assert_eq!(
            first.document().customisation_state().origin,
            CustomisationOrigin::BuiltIn
        );
        assert_eq!(
            first.shown("settingsActiveBuiltIn"),
            second.shown("settingsActiveBuiltIn")
        );
        drop((a.scratch, b.scratch));
    }

    // ---- the verb itself ------------------------------------------------------------------------

    // No stand-in: the interpreter over the same drawing, as the window's executor runs a line.
    // Given no host, as a test's Document is.
    #[test]
    fn the_lines_it_builds_are_read_by_the_verb_itself() {
        let mut bench = Fixture::new().open();
        bench.answer_by_the_verb();

        // Import: the committed fixture of 4 symbols.
        bench.fill(
            "settingsImportPath",
            &path_text(&customisation_fixture_file("test_symbols")),
        );
        bench.press("settingsImport");
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "test_symbols");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Loaded this session");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "4");
        assert_eq!(bench.shown("settingsActiveSymbols"), "4");
        assert_eq!(bench.document().style_library().size(), 4);

        // Export, to a folder and a file with a blank in each: one word to the tokenizer, and the
        // file reads back as what the session holds.
        let scratch = Scratch::new("settings-lines-read-by-the-verb");
        let written = path_text(&scratch.path("site sets").join("all of it.json"));
        bench.fill("settingsExportPath", &written);
        bench.press("settingsExport");
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        let read = read_customisation_file(std::path::Path::new(&written))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(read.customisation == bench.document().customisation());
        assert_eq!(read.customisation.library.size(), 4);

        // A switch: set on the Document by the verb, and the box follows it.
        bench.press("settingsAutoLinework");
        assert_eq!(
            bench.ran().last().map(String::as_str),
            Some("CUSTOMISE SET auto.linework=off")
        );
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(
            !bench
                .document()
                .customisation_state()
                .automation
                .linework_on_survey_import
        );
        assert!(!bench.checked("settingsAutoLinework"));
        bench.draw();
        assert_eq!(bench.shown("settingsActiveOrigin"), "Edited this session");
        // And ticked again: the verb reads "on" as it read "off".
        bench.press("settingsAutoLinework");
        assert_eq!(
            bench.ran().last().map(String::as_str),
            Some("CUSTOMISE SET auto.linework=on")
        );
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(
            bench
                .document()
                .customisation_state()
                .automation
                .linework_on_survey_import
        );
        assert!(bench.checked("settingsAutoLinework"));

        // A path in its quotes, as a file manager copies one: the verb is given the path alone and
        // reads the file. The fixture of 3 linestyles, merged into the 4 symbols.
        let linestyles = native(&path_text(&customisation_fixture_file("test_linestyles")));
        bench.fill("settingsImportPath", &format!("\"{linestyles}\""));
        bench.press("settingsImport");
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert_eq!(bench.document().style_library().size(), 7);
        assert!(bench.document().style_library().contains("TEST Gate"));

        // A file called as a word of the family is read as a FILE: the verb looks for it and does
        // not find it, where `CUSTOMISE keep` is KEEP.
        let before: Customisation = bench.document().customisation();
        bench.fill("settingsImportPath", "keep");
        bench.press("settingsImport");
        assert_eq!(
            bench.ran().last().map(String::as_str),
            Some("CUSTOMISE \"./keep\"")
        );
        assert!(bench.says_a_refusal());
        assert!(bench.status().contains("./keep"), "{}", bench.status());
        assert!(
            !bench.status().contains("CUSTOMISE KEEP"),
            "{}",
            bench.status()
        );
        assert!(bench.document().customisation() == before);
    }

    // A host as a front end hands one over: SITE as the built-in, and a kept file in a folder of
    // this case's own that does not exist yet.
    #[test]
    fn with_the_verb_itself_an_edit_of_a_kept_session_is_written_to_the_kept_file() {
        let scratch = Scratch::new("settings-edit-of-a-kept-session");
        let kept_path = scratch.path("kept here").join("customisation.json");
        let kept_file = path_text(&kept_path);
        let host = katana_cad::customisation_host::CustomisationHost {
            built_in: crate::support::site_built_in(),
            kept_file: kept_path.clone(),
            built_ins: None,
        };

        let mut f = Fixture::new();
        f.context.kept_file = kept_file.clone();
        f.host.parts.interpreter.set_customisation_host(host.clone());
        let started = start_customisation(f.document_mut(), &host);
        assert_eq!(started.installed, CustomisationOrigin::BuiltIn);
        assert!(f.document().customisation_state().kept);
        let mut bench = f.open();
        bench.answer_by_the_verb();
        assert_eq!(bench.shown("settingsActiveOrigin"), "Built in");
        assert!(!bench.enabled("settingsKeep"));
        assert!(!exists(&kept_file));

        // A switch turned off in Settings: set, and kept behind it.
        bench.press("settingsAutoCodes");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.codes=off", KEEP]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(bench.document().customisation_state().kept);
        assert!(exists(&kept_file));
        let kept = read_customisation_file(&kept_path)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let switches = kept
            .customisation
            .automation
            .expect("the kept file says its automation");
        assert!(!switches.codes_on_survey_import);
        assert!(switches.linework_on_survey_import);
        bench.draw();
        assert!(bench.shown("settingsActiveKept").starts_with("Kept:"));
        assert!(!bench.enabled("settingsKeep"));

        // Reset to Built-in: with a kept file there the reset session is not the kept one, so it
        // is kept - and a session that IS the built-in is kept by setting the kept file aside.
        bench.clear_ran();
        bench.press("settingsReset");
        assert_eq!(bench.ran(), ["CUSTOMISE RESET", KEEP]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(bench.document().customisation_state().kept);
        assert!(
            bench
                .document()
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
        assert!(!exists(&kept_file));
        assert!(exists(&format!("{kept_file}.bak")));
        bench.draw();
        assert!(bench.checked("settingsAutoCodes"));
        assert_eq!(bench.shown("settingsActiveOrigin"), "Built in");
    }

    // ---- a reset of a kept session, and the way back from it ------------------------------------
    //
    // The reset is kept, and a session that is the built-in is kept by setting the kept file
    // aside, so nothing is kept for Revert to read. These follow the file through each press.

    #[test]
    fn after_a_reset_of_a_kept_session_revert_has_none_to_read_and_the_file_set_aside_is_the_way_back()
     {
        let session = KeptSession::start("settings-reset-way-back");
        let kept_file = session.kept_file.clone();
        let set_aside = session.set_aside.clone();
        let mut bench = session.fixture.open();
        // MINE, read from the kept file at the start: survey codes not applied.
        assert_eq!(bench.shown("settingsActiveName"), "Mine");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Kept by you");
        assert!(!bench.checked("settingsAutoCodes"));
        // What the press will cost is on the button before it is pressed.
        let tip = bench.tip("settingsReset");
        assert!(
            tip.contains(&format!(
                "is set aside as {set_aside}, and Revert to Kept then has none to read"
            )),
            "{tip}"
        );

        // Reset to Built-in. The reset is kept, and CUSTOMISE KEEP writes no copy of the built-in:
        // it sets the kept file aside.
        bench.press("settingsReset");
        assert_eq!(bench.ran(), ["CUSTOMISE RESET", KEEP]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(
            bench
                .status()
                .contains("written=no reason=the-session-is-the-built-in"),
            "{}",
            bench.status()
        );
        assert!(!exists(&kept_file));
        assert!(exists(&set_aside));
        assert_eq!(customisation_in_file(&set_aside).name, "Mine");
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Site");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Built in");
        assert!(bench.shown("settingsActiveKept").starts_with("Kept:"));
        assert!(bench.checked("settingsAutoCodes"));

        // Revert to Kept is NOT the way back: nothing is kept for it to read, and the verb says
        // so. The session stays the built-in.
        bench.clear_ran();
        assert!(bench.enabled("settingsRevert"));
        bench.press("settingsRevert");
        assert_eq!(bench.ran(), ["CUSTOMISE REVERT"]);
        assert!(bench.says_a_refusal());
        assert!(
            bench.status().starts_with(
                "> CUSTOMISE REVERT\nRefused: NotFound: CUSTOMISE REVERT: no customisation is kept"
            ),
            "{}",
            bench.status()
        );
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Site");

        // The way back is the file that was set aside, imported with Replace - and, the session
        // being the kept one, kept again, in a kept file written anew.
        bench.clear_ran();
        bench.fill("settingsImportPath", &set_aside);
        bench.tick("settingsImportReplace", true);
        bench.press("settingsImport");
        assert_eq!(
            bench.ran(),
            [format!("CUSTOMISE REPLACE \"{set_aside}\""), String::from(KEEP)]
        );
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Mine");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "1");
        assert_eq!(bench.shown("settingsActiveSymbols"), "0");
        assert_eq!(bench.shown("settingsActiveRules"), "1 over 1 code");
        assert!(bench.shown("settingsActiveKept").starts_with("Kept:"));
        assert!(!bench.checked("settingsAutoCodes"));
        assert!(bench.document().style_library().contains("MINE Hedge"));
        assert!(!bench.document().style_library().contains("SITE Fence"));
        assert_eq!(bench.document().survey_map().keys(), ["HG*"]);
        assert!(exists(&kept_file));
        let kept = customisation_in_file(&kept_file);
        assert_eq!(kept.name, "Mine");
        assert!(kept.library.contains("MINE Hedge"));
        assert_eq!(kept.library.size(), 1);
        assert_eq!(kept.map.size(), 1);
        let switches = kept.automation.expect("the kept file says its automation");
        assert!(!switches.codes_on_survey_import);
        drop(session.scratch);
    }

    #[test]
    fn the_file_set_aside_given_its_own_name_back_is_read_by_revert_as_it_was() {
        let session = KeptSession::start("settings-set-aside-renamed");
        let kept_file = session.kept_file.clone();
        let set_aside = session.set_aside.clone();
        let mut bench = session.fixture.open();
        let before = bench.document().customisation();
        bench.press("settingsReset");
        assert!(!exists(&kept_file));
        assert!(exists(&set_aside));
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Site");

        // The other way back, taken outside the program, in a file manager: the file's own name
        // given back. Revert to Kept then has a file to read.
        std::fs::rename(&set_aside, &kept_file).expect("the file is renamed");
        bench.clear_ran();
        bench.press("settingsRevert");
        assert_eq!(bench.ran(), ["CUSTOMISE REVERT"]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        // The session the reset put away, every part of it.
        assert!(bench.document().customisation() == before);
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Mine");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Kept by you");
        assert!(bench.shown("settingsActiveKept").starts_with("Kept:"));
        drop(session.scratch);
    }

    #[test]
    fn the_file_a_reset_set_aside_lasts_only_until_the_second_edit_kept_after_it() {
        let session = KeptSession::start("settings-set-aside-lasts");
        let kept_file = session.kept_file.clone();
        let set_aside = session.set_aside.clone();
        let mut bench = session.fixture.open();
        bench.press("settingsReset");
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(!exists(&kept_file));
        assert_eq!(customisation_in_file(&set_aside).name, "Mine");
        bench.draw();

        // The first edit after it is kept in a kept file written ANEW: none was there to be set
        // aside, so the file the reset set aside still is.
        bench.clear_ran();
        bench.press("settingsAutoCodes");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.codes=off", KEEP]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert_eq!(customisation_in_file(&kept_file).name, "Site");
        assert_eq!(customisation_in_file(&set_aside).name, "Mine");

        // The second finds that kept file and sets IT aside, in the place of the one before. ONE
        // earlier file is kept: what is set aside now is the built-in with the first edit.
        bench.clear_ran();
        bench.press("settingsAutoLinework");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.linework=off", KEEP]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert_eq!(customisation_in_file(&kept_file).name, "Site");
        let aside = customisation_in_file(&set_aside);
        assert_eq!(aside.name, "Site");
        let switches = aside.automation.expect("the file set aside says its automation");
        assert!(!switches.codes_on_survey_import);
        assert!(switches.linework_on_survey_import);
        drop(session.scratch);
    }

    #[test]
    fn keep_of_a_session_that_is_the_built_in_writes_nothing_and_sets_the_kept_file_aside() {
        let session = KeptSession::start("settings-keep-of-the-built-in");
        let kept_file = session.kept_file.clone();
        let set_aside = session.set_aside.clone();
        let mut bench = session.fixture.open();
        // A reset TYPED, as on the command line: not Settings', so nothing is kept behind it. The
        // kept file stands and the page says "Not kept".
        {
            let parts = &mut bench.host.parts;
            parts
                .interpreter
                .run(&mut parts.document, "CUSTOMISE RESET")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        }
        bench.draw();
        assert_eq!(bench.shown("settingsActiveOrigin"), "Built in");
        assert!(bench.shown("settingsActiveKept").starts_with("Not kept:"));
        assert!(exists(&kept_file));
        assert!(!exists(&set_aside));
        // Keep says what a press does to a session that is the built-in...
        assert!(bench.enabled("settingsKeep"));
        let tip = bench.tip("settingsKeep");
        assert!(
            tip.contains(&format!(
                "nothing is written, and the kept file, when there is one, is set aside as \
                 {set_aside}"
            )),
            "{tip}"
        );
        // ...and the press does it: the verb's answer says nothing was written, and the kept file
        // is the one set aside.
        bench.press("settingsKeep");
        assert_eq!(bench.ran(), [KEEP]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        assert!(
            bench
                .status()
                .contains("written=no reason=the-session-is-the-built-in"),
            "{}",
            bench.status()
        );
        assert!(!exists(&kept_file));
        assert_eq!(customisation_in_file(&set_aside).name, "Mine");
        bench.draw();
        assert!(bench.shown("settingsActiveKept").starts_with("Kept:"));
        drop(session.scratch);
    }

    // The other half of the rule: where the reset was NOT kept, the kept file stands, and Revert
    // to Kept goes back to it.
    #[test]
    fn a_revert_of_a_session_that_was_reset_and_not_kept_reads_the_kept_file_again() {
        let session = KeptSession::start("settings-revert-after-reset");
        let kept_file = session.kept_file.clone();
        let set_aside = session.set_aside.clone();
        let mut bench = session.fixture.open();
        // An edit typed leaves the session not kept; a reset in Settings is then followed by
        // nothing.
        {
            let parts = &mut bench.host.parts;
            parts
                .interpreter
                .run(&mut parts.document, "CUSTOMISE SET auto.linework=off")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        }
        bench.draw();
        assert!(!bench.document().customisation_state().kept);
        bench.press("settingsReset");
        assert_eq!(bench.ran(), ["CUSTOMISE RESET"]);
        assert!(exists(&kept_file));
        assert!(!exists(&set_aside));
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Site");

        bench.clear_ran();
        bench.press("settingsRevert");
        assert_eq!(bench.ran(), ["CUSTOMISE REVERT"]);
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Mine");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Kept by you");
        assert!(bench.shown("settingsActiveKept").starts_with("Kept:"));
        drop(session.scratch);
    }
}
