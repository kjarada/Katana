//! The twins of test_settings_dialog.cpp's SettingsDialog cases of the page and its lines: what it
//! is, the lines each button builds, the automation boxes, what is refused while the code manager
//! holds edits, what the host does not have, what a press costs the kept file, the status, what is
//! active, the notice, the linework controls, Browse, no default button, the words, and the
//! lifetime. Every expected line and text is the C++ test's.

use katana_cad::customisation_host::{CustomisationHost, start_customisation};
use katana_cad::CustomisationOrigin;
use katana_entity::linework_codes::{LineworkCodes, linework_code_members};
use katana_window::customisation::customisation_context::CustomisationContext;
use katana_window::customisation::linework_labels::{
    LINEWORK_CONTROL_LABELS, linework_control_label,
};
use katana_window::customisation::settings_dialog::{
    NOTICE_BOX, SettingsContext, run_through_host,
};
use katana_window::customisation::SurveyCodeManagerDialog;
use katana_window::drive::Harness;
use katana_window::host::RecordingHost;
use katana_window::theme;

use crate::fixture_customisation::{
    customisation_fixture_file, customisation_from_text, install_customisation_fixtures,
    install_noticed_session, read_customisation_fixture,
};
use crate::support::{
    Bench, Fixture, KEEP, KEPT_FILE, ONE_CODE, automation, carried_out, install_site, native, ok,
    path_text, refused, site_built_in,
};
use crate::window_support::Scratch;

mod settings_dialog {
    use super::*;

    // ---- what it is -----------------------------------------------------------------------------

    // Two pages since the update system gave Settings its second (2026-10-13). The Updates page's
    // own controls are the SettingsUpdates suite's (tests/update).
    #[test]
    fn it_is_a_non_modal_dialog_with_two_pages_and_a_name_for_every_control() {
        let bench = Fixture::new().open();
        assert_eq!(bench.dialog.object_name(), "settingsDialog");
        assert_eq!(bench.dialog.window_title(), "Settings");
        assert!(!bench.dialog.is_modal());

        let pages = bench.control("settingsPages");
        assert_eq!(pages.items, ["Customisation", "Updates"]);
        assert_eq!(pages.value, "Customisation");
        let stack = bench.control("settingsStack");
        assert_eq!(stack.items.len(), 2);
        assert_eq!(stack.value, "settingsCustomisation");

        for name in [
            "settingsActiveName",
            "settingsActiveOrigin",
            "settingsActiveDefinitions",
            "settingsActiveSymbols",
            "settingsActiveRules",
            "settingsActiveKept",
            "settingsActiveProblems",
            "settingsShowNotice",
            "settingsNotice",
            "settingsImportPath",
            "settingsImportBrowse",
            "settingsImportReplace",
            "settingsImport",
            "settingsExportPath",
            "settingsExportBrowse",
            "settingsExport",
            "settingsReset",
            "settingsKeep",
            "settingsRevert",
            "settingsAutoCodes",
            "settingsAutoLinework",
            "settingsLinework",
            "settingsEditLinework",
            "settingsOpenCodes",
            "settingsOpenSymbols",
            "settingsActiveBuiltIn",
            "settingsBuiltInRow",
            "settingsBuiltIn",
            "settingsBuiltInUse",
        ] {
            assert!(bench.record(name).is_some(), "{name}");
        }
        assert!(bench.record("settingsStatus").is_some());
        assert!(bench.record("settingsClose").is_some());
        // The words the task gives the controls.
        assert_eq!(
            bench.control("settingsImportReplace").text,
            "Replace instead of merging"
        );
        assert_eq!(
            bench.control("settingsAutoCodes").text,
            "Apply survey codes"
        );
        assert_eq!(
            bench.control("settingsAutoLinework").text,
            "Join coded points into lines"
        );
        assert_eq!(bench.control("settingsOpenCodes").text, "Survey Codes...");
        assert_eq!(
            bench.control("settingsOpenSymbols").text,
            "Symbol Library..."
        );
        // There is no Apply button: each control runs its own line.
        for button in bench.buttons() {
            assert!(
                !button.text.to_lowercase().contains("apply"),
                "{}",
                button.name
            );
        }
    }

    // ---- the lines ------------------------------------------------------------------------------

    #[test]
    fn import_builds_the_merge_line_and_with_replace_ticked_the_replace_line() {
        let mut bench = Fixture::new().open();
        // A folder and a file with a blank in each: one word of the line.
        bench.fill(
            "settingsImportPath",
            "C:/survey data/site codes.customisation.json",
        );
        bench.press("settingsImport");
        assert_eq!(
            bench.ran(),
            ["CUSTOMISE \"C:/survey data/site codes.customisation.json\""]
        );

        bench.clear_ran();
        bench.tick("settingsImportReplace", true);
        assert!(bench.ran().is_empty(), "ticking Replace runs nothing");
        bench.press("settingsImport");
        assert_eq!(
            bench.ran(),
            ["CUSTOMISE REPLACE \"C:/survey data/site codes.customisation.json\""]
        );
    }

    // A load reports every name the rules ask for that nothing defines, the built-in's own few
    // included; the reply marks them (`in_built_in=yes`, and the count on its first line), and the
    // status shows them once, as a count.
    #[test]
    fn the_names_the_built_in_itself_leaves_undefined_are_shown_as_a_count_and_the_rest_by_name() {
        let mut bench = Fixture::new().open();
        bench.answer(|_, _| {
            ok("loaded file=\"C:/x.json\" name=X definitions_added=0\n\
                undefined names=3 in_built_in=2\n\
                undefined name=\"TEST Undefined A\" in_built_in=yes\n\
                undefined name=\"MINE Gate\"\n\
                undefined name=TEST Undefined B in_built_in=yes\n\
                customisation name=X origin=loaded kept=no")
        });
        bench.fill("settingsImportPath", "C:/x.json");
        bench.press("settingsImport");
        let status = bench.status();
        // Two of the three are the built-in's: said once, as a count; the third is this load's
        // and is named.
        assert!(
            status.contains("3 names undefined, 2 of them as in the built-in\n"),
            "{status}"
        );
        assert!(status.contains("undefined name=\"MINE Gate\"\n"), "{status}");
        assert!(!status.contains("TEST Undefined A"), "{status}");
        assert!(!status.contains("TEST Undefined B"), "{status}");
        assert!(!status.contains("in_built_in"), "{status}");
        assert!(status.starts_with("> CUSTOMISE \"C:/x.json\"\n"), "{status}");

        // All of them the built-in's: only the count.
        bench.answer(|_, _| {
            ok("undefined names=3 in_built_in=3\n\
                undefined name=A in_built_in=yes\n\
                undefined name=B in_built_in=yes\n\
                undefined name=C in_built_in=yes\n\
                customisation name=X origin=loaded kept=no")
        });
        bench.press("settingsImport");
        assert_eq!(
            bench.status(),
            "> CUSTOMISE \"C:/x.json\"\n3 names undefined (as in the built-in)\ncustomisation \
             name=X origin=loaded kept=no"
        );
        // One name says "name", not "names".
        bench.answer(|_, _| ok("undefined names=1 in_built_in=1\nundefined name=A in_built_in=yes"));
        bench.press("settingsImport");
        assert!(
            bench
                .status()
                .ends_with("1 name undefined (as in the built-in)"),
            "{}",
            bench.status()
        );

        // A reply with no mark - a session whose built-in leaves nothing undefined - is shown as
        // the verb said it.
        bench.answer(|_, _| ok("undefined names=1\nundefined name=\"SITE Gate\""));
        bench.press("settingsImport");
        assert!(
            bench
                .status()
                .ends_with("undefined names=1\nundefined name=\"SITE Gate\""),
            "{}",
            bench.status()
        );
    }

    #[test]
    fn export_builds_the_export_line() {
        let mut bench = Fixture::new().open();
        bench.fill("settingsExportPath", "D:/out put/site set.customisation.json");
        bench.press("settingsExport");
        assert_eq!(
            bench.ran(),
            ["CUSTOMISE EXPORT \"D:/out put/site set.customisation.json\""]
        );
    }

    #[test]
    fn reset_keep_and_revert_each_build_their_line() {
        let mut bench = Fixture::new().open();
        bench.press("settingsReset");
        bench.press("settingsKeep");
        bench.press("settingsRevert");
        assert_eq!(bench.ran(), ["CUSTOMISE RESET", KEEP, "CUSTOMISE REVERT"]);
    }

    #[test]
    fn a_path_is_trimmed_and_a_bare_name_is_given_its_directory_so_it_is_never_read_as_a_keyword()
    {
        let mut bench = Fixture::new().open();
        bench.fill("settingsImportPath", "  C:/data/site.json  ");
        bench.press("settingsImport");
        // `CUSTOMISE "keep"` is CUSTOMISE KEEP to the verb, which reads a whole first word as a
        // keyword once the tokenizer has taken the quotes off.
        bench.fill("settingsImportPath", "keep");
        bench.press("settingsImport");
        // EXPORT refuses a file called as one of its own words.
        bench.fill("settingsExportPath", "codes");
        bench.press("settingsExport");
        // A path that names a folder is the person's own, as it is.
        bench.fill("settingsExportPath", "out/codes");
        bench.press("settingsExport");
        assert_eq!(
            bench.ran(),
            [
                "CUSTOMISE \"C:/data/site.json\"",
                "CUSTOMISE \"./keep\"",
                "CUSTOMISE EXPORT \"./codes\"",
                "CUSTOMISE EXPORT \"out/codes\""
            ]
        );
    }

    #[test]
    fn a_path_pasted_in_its_quotes_is_read_as_the_path_it_is() {
        let mut bench = Fixture::new().open();
        // As a file manager copies a path: the whole of it in double quotes.
        bench.fill("settingsImportPath", "\"C:/survey data/site codes.json\"");
        bench.press("settingsImport");
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        // Blanks round the quotes, and inside them, are trimmed as round a path with none.
        bench.fill("settingsExportPath", "  \" D:/out put/site set.json \"  ");
        bench.press("settingsExport");
        assert!(!bench.says_a_refusal(), "{}", bench.status());
        // A bare name is given its directory in quotes or out of them.
        bench.fill("settingsImportPath", "\"keep\"");
        bench.press("settingsImport");
        assert_eq!(
            bench.ran(),
            [
                "CUSTOMISE \"C:/survey data/site codes.json\"",
                "CUSTOMISE EXPORT \"D:/out put/site set.json\"",
                "CUSTOMISE \"./keep\""
            ]
        );
        // The field keeps what was put in it.
        assert_eq!(bench.text("settingsImportPath"), "\"keep\"");

        // ONE pair, round the WHOLE path. Quotes round nothing name no file; a second pair, two
        // paths side by side and a quote alone each still hold a quote no line can carry.
        bench.clear_ran();
        let no_file = "Import: no file is named: type its path, or choose it with Browse";
        let a_quote =
            "Import: the file's path: a double quote cannot be written on a command line";
        let refused_paths = [
            ("\"\"", no_file),
            ("\"   \"", no_file),
            ("\"\"C:/data/site.json\"\"", a_quote),
            ("\"C:/data/a.json\" \"C:/data/b.json\"", a_quote),
            ("\"", a_quote),
        ];
        for (typed, why) in refused_paths {
            bench.fill("settingsImportPath", typed);
            bench.press("settingsImport");
            assert_eq!(bench.status(), why, "{typed}");
            assert!(bench.says_a_refusal(), "{typed}");
        }
        assert!(bench.ran().is_empty(), "{:?}", bench.ran());
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_is_written_with_forward_slashes() {
        let mut bench = Fixture::new().open();
        bench.fill("settingsImportPath", "C:\\survey data\\site.json");
        bench.press("settingsImport");
        // And as Explorer's "Copy as path" gives one: in quotes, with its own separators.
        bench.fill("settingsImportPath", "\"C:\\data\\site codes.json\"");
        bench.press("settingsImport");
        assert_eq!(
            bench.ran(),
            [
                "CUSTOMISE \"C:/survey data/site.json\"",
                "CUSTOMISE \"C:/data/site codes.json\""
            ]
        );
    }

    #[test]
    fn a_file_no_line_can_carry_and_no_file_at_all_run_nothing_and_say_why() {
        let mut bench = Fixture::new().open();
        bench.press("settingsImport");
        assert_eq!(
            bench.status(),
            "Import: no file is named: type its path, or choose it with Browse"
        );
        assert!(bench.says_a_refusal());
        bench.fill("settingsExportPath", "   ");
        bench.press("settingsExport");
        assert_eq!(
            bench.status(),
            "Export: no file is named: type its path, or choose it with Browse"
        );

        // The tokenizer's quoted words have no escape: a quote would end the word.
        bench.fill("settingsImportPath", "C:/data/the \"new\" set.json");
        bench.press("settingsImport");
        assert_eq!(
            bench.status(),
            "Import: the file's path: a double quote cannot be written on a command line"
        );
        assert!(bench.says_a_refusal());
        bench.fill("settingsExportPath", "C:/data/the \"new\" set.json");
        bench.press("settingsExport");
        assert_eq!(
            bench.status(),
            "Export: the file's path: a double quote cannot be written on a command line"
        );

        assert!(bench.ran().is_empty(), "{:?}", bench.ran());
        // Said in the log too, as refusals: a headless run reads them there.
        let logged = bench.logged();
        assert_eq!(logged.len(), 4);
        assert_eq!(logged[3].0, bench.status());
        assert!(logged[3].1);
    }

    // ---- the automation boxes -------------------------------------------------------------------

    #[test]
    fn toggling_a_box_runs_its_line_once_and_the_box_shows_what_the_document_has() {
        let mut bench = Fixture::new().open();
        // Both switches are on in a Document nothing has set them in.
        assert!(bench.checked("settingsAutoCodes"));
        assert!(bench.checked("settingsAutoLinework"));

        bench.press("settingsAutoCodes");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.codes=off"]);
        // The recording runner changed nothing, so the Document still has it on, and putting the
        // box back ran no second line.
        assert!(bench.checked("settingsAutoCodes"));
        bench.draw();
        assert_eq!(bench.ran().len(), 1);

        bench.clear_ran();
        bench.press("settingsAutoLinework");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.linework=off"]);
        assert!(bench.checked("settingsAutoLinework"));

        // With a runner that does what the line says, the box stays where it was put, and the next
        // toggle says the other word.
        bench.clear_ran();
        bench.answer(|parts, line| {
            let mut automation = parts.document.customisation_state().automation;
            if line == "CUSTOMISE SET auto.codes=off" {
                automation.codes_on_survey_import = false;
            } else if line == "CUSTOMISE SET auto.codes=on" {
                automation.codes_on_survey_import = true;
            }
            parts.document.set_automation(automation);
            carried_out()
        });
        bench.press("settingsAutoCodes");
        bench.draw();
        assert!(!bench.checked("settingsAutoCodes"));
        assert!(
            !bench
                .document()
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
        bench.press("settingsAutoCodes");
        bench.draw();
        assert!(bench.checked("settingsAutoCodes"));
        assert_eq!(
            bench.ran(),
            ["CUSTOMISE SET auto.codes=off", "CUSTOMISE SET auto.codes=on"]
        );
    }

    // --fill settingsAutoLinework=off is QCheckBox::setChecked: no click.
    #[test]
    fn a_box_set_as_the_headless_driver_fills_one_runs_its_line_too() {
        let mut bench = Fixture::new().open();
        bench.tick("settingsAutoLinework", false);
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.linework=off"]);
    }

    // The fourth line of the two boxes: each box off in the Document, so that a tick is "on".
    #[test]
    fn a_box_the_document_has_off_runs_its_line_with_on_when_it_is_ticked() {
        let mut f = Fixture::new();
        f.document_mut().set_automation(automation(false, false));
        let mut bench = f.open();
        assert!(!bench.checked("settingsAutoCodes"));
        assert!(!bench.checked("settingsAutoLinework"));
        assert!(
            bench.ran().is_empty(),
            "showing the boxes as the Document has them runs nothing"
        );

        bench.press("settingsAutoLinework");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.linework=on"]);
        // The recording runner set nothing, so the box is off again, as the Document is.
        assert!(!bench.checked("settingsAutoLinework"));

        bench.clear_ran();
        bench.press("settingsAutoCodes");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.codes=on"]);
        assert!(!bench.checked("settingsAutoCodes"));
    }

    #[test]
    fn refreshing_from_a_document_change_runs_nothing() {
        let mut bench = Fixture::new().open();
        bench.document_mut().set_automation(automation(false, false));
        bench.draw();
        assert!(!bench.checked("settingsAutoCodes"));
        assert!(!bench.checked("settingsAutoLinework"));
        bench.document_mut().set_automation(automation(true, false));
        bench.draw();
        assert!(bench.checked("settingsAutoCodes"));
        assert!(!bench.checked("settingsAutoLinework"));
        // A whole customisation installed, and the page read again by hand.
        bench.install(CustomisationOrigin::Loaded, false);
        bench.draw();
        bench.dialog.refresh(&bench.host);
        bench.draw();
        assert!(bench.checked("settingsAutoLinework"));
        assert!(bench.ran().is_empty(), "{:?}", bench.ran());
        assert!(bench.status().is_empty(), "{}", bench.status());
    }

    // ---- refused while the code manager holds edits ---------------------------------------------

    #[test]
    fn import_replace_reset_and_revert_are_refused_while_the_code_manager_holds_unapplied_edits() {
        let f = Fixture::new();
        f.dirty.set(true);
        let mut bench = f.open();
        bench.fill("settingsImportPath", "C:/data/site.json");

        bench.press("settingsImport");
        assert_eq!(
            bench.status(),
            "Import was not run: the Survey Code Manager has rule edits that are not on the \
             drawing, and its Apply would then put the rules it holds back over what Import \
             brought. Apply or Revert them first."
        );
        assert!(bench.says_a_refusal());
        bench.tick("settingsImportReplace", true);
        bench.press("settingsImport");
        assert!(
            bench
                .status()
                .starts_with("Import was not run: the Survey Code Manager has rule edits")
        );

        bench.press("settingsReset");
        assert_eq!(
            bench.status(),
            "Reset to Built-in was not run: the Survey Code Manager has rule edits that are not \
             on the drawing, and its Apply would then put the rules it holds back over what Reset \
             to Built-in brought. Apply or Revert them first."
        );
        // Revert to Kept loads the kept file's rules: the same loss by the same Apply.
        bench.press("settingsRevert");
        assert!(
            bench
                .status()
                .starts_with("Revert to Kept was not run: the Survey Code Manager has rule edits")
        );

        assert!(bench.ran().is_empty(), "{:?}", bench.ran());
        let logged = bench.logged();
        assert_eq!(logged.len(), 4);
        assert!(logged[0].1);
        assert!(logged[0].0.starts_with("Import was not run"));

        // What loads no rules is not held up: an export, a keep, a switch.
        bench.fill("settingsExportPath", "C:/data/out.json");
        bench.press("settingsExport");
        bench.press("settingsKeep");
        bench.press("settingsAutoCodes");
        assert_eq!(
            bench.ran(),
            [
                "CUSTOMISE EXPORT \"C:/data/out.json\"",
                KEEP,
                "CUSTOMISE SET auto.codes=off"
            ]
        );

        // Once the edits are applied or reverted, the same press runs.
        bench.clear_ran();
        bench.dirty.set(false);
        bench.press("settingsReset");
        assert_eq!(bench.ran(), ["CUSTOMISE RESET"]);
    }

    // ---- what the host does not have ------------------------------------------------------------

    #[test]
    fn reset_is_disabled_and_says_why_when_the_host_has_no_built_in() {
        let mut f = Fixture::new();
        f.context.has_built_in = false;
        let mut bench = f.open();
        assert!(!bench.enabled("settingsReset"));
        assert_eq!(
            bench.tip("settingsReset"),
            "This program has no built-in customisation to reset to."
        );
        bench.press("settingsReset");
        assert!(bench.ran().is_empty());
        // The kept file is another matter.
        assert!(bench.enabled("settingsKeep"));
        assert!(bench.enabled("settingsRevert"));
    }

    #[test]
    fn keep_and_revert_are_disabled_and_say_why_when_there_is_no_kept_file() {
        let mut f = Fixture::new();
        f.context.kept_file.clear();
        let mut bench = f.open();
        let why = "This session has no kept file; the environment variable KATANA_CUSTOMISATION \
                   names one.";
        for name in ["settingsKeep", "settingsRevert"] {
            assert!(!bench.enabled(name), "{name}");
            assert_eq!(bench.tip(name), why, "{name}");
            bench.press(name);
        }
        assert!(bench.ran().is_empty());
        assert!(bench.enabled("settingsReset"));
        // Nothing is kept, and the start gives the built-in.
        assert_eq!(
            bench.shown("settingsActiveKept"),
            "Nothing is kept: the next start gives the built-in customisation. The environment \
             variable KATANA_CUSTOMISATION names a file to keep one in."
        );
    }

    #[test]
    fn the_kept_row_of_a_session_with_no_kept_file_and_no_built_in_says_nothing_is_given() {
        let mut f = Fixture::new();
        f.context.kept_file.clear();
        f.context.has_built_in = false;
        let bench = f.open();
        assert_eq!(
            bench.shown("settingsActiveKept"),
            "Nothing is kept: the next start gives no customisation. The environment variable \
             KATANA_CUSTOMISATION names a file to keep one in."
        );
    }

    #[test]
    fn keep_is_disabled_while_the_session_is_kept_and_comes_back_with_the_first_edit() {
        let mut f = Fixture::new();
        f.install(CustomisationOrigin::Kept, true);
        let mut bench = f.open();
        assert!(!bench.enabled("settingsKeep"));
        assert_eq!(
            bench.tip("settingsKeep"),
            "The session is already kept: the next start gives it."
        );
        assert!(bench.enabled("settingsRevert"));
        bench.press("settingsKeep");
        assert!(bench.ran().is_empty());

        // An edit made elsewhere leaves the session not kept, and the Document says so.
        bench.document_mut().set_automation(automation(false, true));
        bench.draw();
        assert!(bench.enabled("settingsKeep"));
        bench.press("settingsKeep");
        assert_eq!(bench.ran(), [KEEP]);
    }

    // ---- what a press costs the kept file, said before it is pressed ----------------------------

    #[test]
    fn reset_says_before_it_is_pressed_that_a_kept_sessions_file_is_set_aside() {
        let mut f = Fixture::new();
        f.install(CustomisationOrigin::Kept, true);
        let mut bench = f.open();
        let plain = "Put the program's built-in customisation in the place of the session's \
                     (CUSTOMISE RESET)";
        // A kept session: its reset is followed by KEEP, and KEEP keeps a session that is the
        // built-in by setting the kept file aside.
        assert_eq!(
            bench.tip("settingsReset"),
            format!(
                "{plain}.\nThis session is the kept one, so its reset is kept too: the kept file, \
                 when there is one, is set aside as C:/kept files/customisation.json.bak, and \
                 Revert to Kept then has none to read.\nImport that file with Replace instead of \
                 merging ticked to have it back."
            )
        );

        // Not kept - an edit made elsewhere: the tip says what the button does and no more.
        bench.document_mut().set_automation(automation(false, true));
        bench.draw();
        assert_eq!(bench.tip("settingsReset"), plain);

        // A host that keeps no file has none to set aside, kept session or not.
        let mut none = Fixture::new();
        none.context.kept_file.clear();
        none.install(CustomisationOrigin::BuiltIn, true);
        let without = none.open();
        assert_eq!(without.tip("settingsReset"), plain);
    }

    #[test]
    fn keep_says_that_a_session_that_is_the_built_in_is_not_written_and_the_kept_file_is_set_aside()
    {
        // The built-in, and not kept: what a reset typed on the command line leaves.
        let mut f = Fixture::new();
        f.install(CustomisationOrigin::BuiltIn, false);
        let mut bench = f.open();
        assert!(bench.enabled("settingsKeep"));
        assert_eq!(
            bench.tip("settingsKeep"),
            "The session is the built-in customisation, which every start gives when nothing is \
             kept: nothing is written, and the kept file, when there is one, is set aside as \
             C:/kept files/customisation.json.bak (CUSTOMISE KEEP)"
        );

        // Edited, it is no longer the built-in, and Keep writes it.
        bench.document_mut().set_automation(automation(false, true));
        bench.draw();
        assert_eq!(
            bench.tip("settingsKeep"),
            "Write the session's customisation to the kept file, which the next start reads \
             (CUSTOMISE KEEP)"
        );
    }

    // ---- what is edited in Settings is kept -----------------------------------------------------

    #[test]
    fn an_action_on_a_kept_session_with_a_kept_file_is_followed_by_keep() {
        let mut f = Fixture::new();
        f.install(CustomisationOrigin::BuiltIn, true);
        let mut bench = f.open();
        // Each line does to the Document what its verb does: a switch set, a file loaded and the
        // built-in put back each leave the session not kept.
        bench.answer(|parts, line| {
            let document = &mut parts.document;
            if line == KEEP {
                document.set_customisation_kept(true);
            } else if line == "CUSTOMISE SET auto.linework=off" {
                document.set_automation(automation(true, false));
            } else if line.starts_with("CUSTOMISE REPLACE ") || line.starts_with("CUSTOMISE \"") {
                install_site(document, CustomisationOrigin::Loaded, false);
            } else if line == "CUSTOMISE RESET" {
                install_site(document, CustomisationOrigin::BuiltIn, false);
            }
            carried_out()
        });

        bench.press("settingsAutoLinework");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.linework=off", KEEP]);
        assert!(bench.document().customisation_state().kept);

        bench.clear_ran();
        bench.fill("settingsImportPath", "C:/data/site.json");
        bench.press("settingsImport");
        assert_eq!(bench.ran(), ["CUSTOMISE \"C:/data/site.json\"", KEEP]);

        bench.clear_ran();
        bench.tick("settingsImportReplace", true);
        bench.press("settingsImport");
        assert_eq!(
            bench.ran(),
            ["CUSTOMISE REPLACE \"C:/data/site.json\"", KEEP]
        );

        bench.clear_ran();
        bench.press("settingsReset");
        assert_eq!(bench.ran(), ["CUSTOMISE RESET", KEEP]);
        // Both lines and both answers are in the status, in the order they ran.
        assert_eq!(
            bench.status(),
            "> CUSTOMISE RESET\nDone.\n> CUSTOMISE KEEP\nDone."
        );
        assert!(!bench.says_a_refusal());
    }

    #[test]
    fn an_action_on_a_session_that_was_not_kept_is_not_followed_by_keep() {
        let mut f = Fixture::new();
        f.install(CustomisationOrigin::Loaded, false);
        let mut bench = f.open();
        bench.answer(|parts, line| {
            if line == "CUSTOMISE SET auto.codes=off" {
                parts.document.set_automation(automation(false, true));
            } else if line != KEEP {
                install_site(&mut parts.document, CustomisationOrigin::Loaded, false);
            }
            carried_out()
        });
        bench.press("settingsAutoCodes");
        bench.fill("settingsImportPath", "C:/data/site.json");
        bench.press("settingsImport");
        bench.press("settingsReset");
        assert_eq!(
            bench.ran(),
            [
                "CUSTOMISE SET auto.codes=off",
                "CUSTOMISE \"C:/data/site.json\"",
                "CUSTOMISE RESET"
            ]
        );
    }

    #[test]
    fn an_action_on_a_kept_session_with_no_kept_file_is_not_followed_by_keep() {
        let mut f = Fixture::new();
        f.context.kept_file.clear();
        f.install(CustomisationOrigin::BuiltIn, true);
        let mut bench = f.open();
        bench.answer(|parts, line| {
            if line == "CUSTOMISE SET auto.codes=off" {
                parts.document.set_automation(automation(false, true));
            } else if line != KEEP {
                install_site(&mut parts.document, CustomisationOrigin::Loaded, false);
            }
            carried_out()
        });
        // Kept before each press, and not after it.
        bench.press("settingsAutoCodes");
        assert!(!bench.document().customisation_state().kept);
        bench.install(CustomisationOrigin::BuiltIn, true);
        bench.fill("settingsImportPath", "C:/data/site.json");
        bench.press("settingsImport");
        assert!(!bench.document().customisation_state().kept);
        bench.install(CustomisationOrigin::BuiltIn, true);
        bench.press("settingsReset");
        assert!(!bench.document().customisation_state().kept);
        assert_eq!(
            bench.ran(),
            [
                "CUSTOMISE SET auto.codes=off",
                "CUSTOMISE \"C:/data/site.json\"",
                "CUSTOMISE RESET"
            ]
        );
    }

    #[test]
    fn keep_follows_only_a_line_that_was_carried_out_and_left_the_session_not_kept() {
        let mut f = Fixture::new();
        f.install(CustomisationOrigin::BuiltIn, true);
        let mut bench = f.open();
        bench.fill("settingsImportPath", "C:/data/site.json");

        // Refused, and nothing is kept behind a refusal - not even one that left the session not
        // kept: the runner sets a switch, and then refuses. That makes the refusal ALONE what
        // stops the KEEP.
        bench.answer(|parts, _| {
            parts.document.set_automation(automation(false, true));
            refused("NotFound: the file cannot be opened")
        });
        bench.press("settingsImport");
        assert!(
            !bench.document().customisation_state().kept,
            "the runner left the session not kept"
        );
        assert_eq!(bench.ran(), ["CUSTOMISE \"C:/data/site.json\""]);
        assert_eq!(
            bench.status(),
            "> CUSTOMISE \"C:/data/site.json\"\nRefused: NotFound: the file cannot be opened"
        );
        assert!(bench.says_a_refusal());

        // Carried out, and the session is still the kept one: a reset to the built-in when
        // nothing is kept in its place.
        bench.clear_ran();
        bench.install(CustomisationOrigin::BuiltIn, true);
        assert!(bench.document().customisation_state().kept);
        bench.answer(|parts, _| {
            install_site(&mut parts.document, CustomisationOrigin::BuiltIn, true);
            carried_out()
        });
        bench.press("settingsReset");
        assert_eq!(bench.ran(), ["CUSTOMISE RESET"]);

        // An export changes nothing, and Revert leaves the session as the kept file has it:
        // neither is ever followed.
        bench.clear_ran();
        bench.answer(|parts, _| {
            install_site(&mut parts.document, CustomisationOrigin::Loaded, false);
            carried_out()
        });
        bench.fill("settingsExportPath", "C:/data/out.json");
        assert!(bench.document().customisation_state().kept);
        bench.press("settingsExport");
        assert!(!bench.document().customisation_state().kept);
        bench.install(CustomisationOrigin::BuiltIn, true);
        bench.press("settingsRevert");
        assert!(!bench.document().customisation_state().kept);
        assert_eq!(
            bench.ran(),
            ["CUSTOMISE EXPORT \"C:/data/out.json\"", "CUSTOMISE REVERT"]
        );
    }

    #[test]
    fn a_keep_that_is_refused_behind_a_line_is_shown_as_the_refusal_it_is() {
        let mut f = Fixture::new();
        f.install(CustomisationOrigin::Kept, true);
        let mut bench = f.open();
        bench.answer(|parts, line| {
            if line == KEEP {
                return refused(
                    "InvalidState: CUSTOMISE KEEP: the kept customisation changed on disk",
                );
            }
            parts.document.set_automation(automation(false, true));
            ok("set auto.codes=off")
        });
        bench.press("settingsAutoCodes");
        assert_eq!(bench.ran(), ["CUSTOMISE SET auto.codes=off", KEEP]);
        assert_eq!(
            bench.status(),
            "> CUSTOMISE SET auto.codes=off\nset auto.codes=off\n> CUSTOMISE KEEP\nRefused: \
             InvalidState: CUSTOMISE KEEP: the kept customisation changed on disk"
        );
        assert!(bench.says_a_refusal());
        // The switch was set all the same, and the page says the session is not kept, with Keep
        // there to try again.
        bench.draw();
        assert!(!bench.checked("settingsAutoCodes"));
        assert!(bench.shown("settingsActiveKept").starts_with("Not kept:"));
        assert!(bench.enabled("settingsKeep"));
    }

    // ---- the status -----------------------------------------------------------------------------

    #[test]
    fn the_status_shows_the_line_run_and_the_verbs_own_words() {
        let mut bench = Fixture::new().open();
        bench.answer(|_, _| ok("reset name=Site definitions=3 codes=2 rules=3 kept=yes"));
        bench.press("settingsReset");
        assert_eq!(
            bench.status(),
            "> CUSTOMISE RESET\nreset name=Site definitions=3 codes=2 rules=3 kept=yes"
        );
        assert!(!bench.says_a_refusal());

        bench.answer(|_, _| {
            refused(
                "InvalidState: CUSTOMISE REVERT needs a kept customisation file, and this \
                 session has none",
            )
        });
        bench.press("settingsRevert");
        assert_eq!(
            bench.status(),
            "> CUSTOMISE REVERT\nRefused: InvalidState: CUSTOMISE REVERT needs a kept \
             customisation file, and this session has none"
        );
        assert!(bench.says_a_refusal());
        // What a line answered is the executor's to log, not the dialog's.
        assert!(bench.logged().is_empty());
    }

    #[test]
    fn with_no_runner_a_press_says_nothing_can_run_its_line_and_a_box_goes_back() {
        let mut f = Fixture::new();
        f.context.run = None;
        let mut bench = f.open();
        bench.press("settingsKeep");
        assert_eq!(
            bench.status(),
            "Nothing here can run CUSTOMISE KEEP: Settings was given no command line."
        );
        assert!(bench.says_a_refusal());
        bench.press("settingsAutoCodes");
        assert_eq!(
            bench.status(),
            "Nothing here can run CUSTOMISE SET auto.codes=off: Settings was given no command \
             line."
        );
        assert!(bench.checked("settingsAutoCodes"));
        assert!(
            bench
                .document()
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
    }

    // ---- what is active -------------------------------------------------------------------------

    #[test]
    fn the_labels_follow_the_document() {
        let mut bench = Fixture::new().open();
        // A Document nothing was installed in.
        assert_eq!(
            bench.shown("settingsActiveName"),
            "No customisation is loaded"
        );
        assert_eq!(bench.shown("settingsActiveOrigin"), "None");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "0");
        assert_eq!(bench.shown("settingsActiveSymbols"), "0");
        assert_eq!(bench.shown("settingsActiveRules"), "0");
        assert_eq!(
            bench.shown("settingsActiveKept"),
            "Not kept: the next start does not give this customisation.\nKept file: C:/kept \
             files/customisation.json"
        );
        assert!(!bench.visible("settingsActiveProblems"));

        // SITE as the program's own, kept: 3 definitions, 2 of them symbols, and 3 rules over FN*
        // and PG*.
        bench.install(CustomisationOrigin::BuiltIn, true);
        assert_eq!(
            bench.shown("settingsActiveName"),
            "No customisation is loaded",
            "the page is read again at the next frame, not inside the change"
        );
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "Site");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Built in");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "3");
        assert_eq!(bench.shown("settingsActiveSymbols"), "2");
        assert_eq!(bench.shown("settingsActiveRules"), "3 over 2 codes");
        assert_eq!(
            bench.shown("settingsActiveKept"),
            "Kept: the next start gives this customisation.\nKept file: C:/kept \
             files/customisation.json"
        );

        // A second, with other counts: the survey fixture alone, read from the user's kept file -
        // 11 rules over 8 codes and no definition.
        bench
            .document_mut()
            .install_customisation(
                read_customisation_fixture("test_survey"),
                CustomisationOrigin::Kept,
                true,
            )
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert_eq!(bench.shown("settingsActiveName"), "test_survey");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Kept by you");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "0");
        assert_eq!(bench.shown("settingsActiveSymbols"), "0");
        assert_eq!(bench.shown("settingsActiveRules"), "11 over 8 codes");

        // The three fixtures loaded into it: the 3 linestyles and the 4 symbols.
        install_customisation_fixtures(bench.document_mut(), &["test_linestyles", "test_symbols"]);
        bench.draw();
        assert_eq!(bench.shown("settingsActiveOrigin"), "Loaded this session");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "7");
        assert_eq!(bench.shown("settingsActiveSymbols"), "4");
        assert_eq!(bench.shown("settingsActiveRules"), "11 over 8 codes");
        assert!(bench.shown("settingsActiveKept").starts_with("Not kept:"));

        // An editor's commit: one definition fewer, by the raw setter, which moves the library's
        // generation and marks the session edited.
        let mut library = bench.document().style_library().clone();
        library
            .remove("TEST Tree")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.document_mut().set_style_library(library);
        bench.draw();
        assert_eq!(bench.shown("settingsActiveOrigin"), "Edited this session");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "6");
        assert_eq!(bench.shown("settingsActiveSymbols"), "3");
        // And a second, of a session already edited: only the library's own generation moves,
        // and the counts follow that too.
        let mut library = bench.document().style_library().clone();
        library
            .remove("TEST Gate")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.document_mut().set_style_library(library);
        bench.draw();
        assert_eq!(bench.shown("settingsActiveDefinitions"), "5");
        assert_eq!(bench.shown("settingsActiveSymbols"), "3");

        assert!(bench.ran().is_empty());
    }

    // The Document keeps what its start found: the problems, a sentence each, and whether the kept
    // customisation was made from another built-in. The page shows the sentences as they are, one
    // a line, and after them its own sentence for the second.
    #[test]
    fn what_the_start_of_the_session_found_is_shown_and_the_row_is_hidden_when_it_found_nothing() {
        let mut bench = Fixture::new().open();
        assert!(
            !bench.visible("settingsActiveProblems"),
            "a session whose start found nothing"
        );
        let from_another = "The kept customisation was made from another built-in customisation \
                            than this program has; Reset to Built-in gives this program's.";

        bench.document_mut().set_customisation_start(
            vec![
                String::from("the kept file was not read"),
                String::from("the built-in did not parse"),
            ],
            false,
        );
        assert!(
            !bench.visible("settingsActiveProblems"),
            "read again at the next frame, not inside the change"
        );
        bench.draw();
        assert!(bench.visible("settingsActiveProblems"));
        assert_eq!(
            bench.shown("settingsActiveProblems"),
            "the kept file was not read\nthe built-in did not parse"
        );
        assert_eq!(
            bench.dialog.error_colour(),
            theme::error(),
            "in the colour of an error"
        );

        // Nothing went wrong, and the kept customisation is from another built-in.
        bench.document_mut().set_customisation_start(Vec::new(), true);
        bench.draw();
        assert!(bench.visible("settingsActiveProblems"));
        assert_eq!(bench.shown("settingsActiveProblems"), from_another);

        // Both: the problems first.
        bench
            .document_mut()
            .set_customisation_start(vec![String::from("the built-in did not parse")], true);
        bench.draw();
        assert_eq!(
            bench.shown("settingsActiveProblems"),
            format!("the built-in did not parse\n{from_another}")
        );

        // It is of the START: an edit of the session since does not take it away.
        bench.install(CustomisationOrigin::Loaded, false);
        bench.draw();
        assert_eq!(
            bench.shown("settingsActiveProblems"),
            format!("the built-in did not parse\n{from_another}")
        );

        bench.document_mut().set_customisation_start(Vec::new(), false);
        bench.draw();
        assert!(!bench.visible("settingsActiveProblems"));
        assert!(bench.shown("settingsActiveProblems").is_empty());
        assert!(bench.ran().is_empty(), "showing it runs no line");
    }

    // The start itself: SITE is the program's built-in, and the kept file holds JSON of another
    // format, which the reader refuses. The built-in then stands in, and the sentence saying so is
    // the one the page shows.
    #[test]
    fn a_kept_file_that_did_not_read_when_the_session_started_is_said_on_the_page() {
        let scratch = Scratch::new("settings-kept-file-did-not-read");
        let kept = scratch.file(
            "customisation.json",
            "{\"format\": \"another-format\", \"version\": 1, \"name\": \"x\"}\n",
        );
        let host = CustomisationHost {
            built_in: site_built_in(),
            kept_file: kept.clone(),
            built_ins: None,
        };
        let mut f = Fixture::new();
        let started = start_customisation(f.document_mut(), &host);
        assert_eq!(started.installed, CustomisationOrigin::BuiltIn);
        assert_eq!(started.problems.len(), 1);
        let problem = started.problems[0].clone();
        assert!(
            problem.starts_with(
                "the kept customisation is not read, so the built-in customisation is used: "
            ),
            "{problem}"
        );
        assert!(
            problem.contains("not a Katana customisation file"),
            "{problem}"
        );

        f.context.kept_file = path_text(&kept);
        let bench = f.open();
        assert!(bench.visible("settingsActiveProblems"));
        assert_eq!(bench.shown("settingsActiveProblems"), problem);
        assert_eq!(bench.shown("settingsActiveName"), "Site");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Built in");
    }

    #[test]
    fn a_session_edited_before_anything_was_installed_is_shown_as_not_named() {
        let mut bench = Fixture::new().open();
        assert_eq!(
            bench.shown("settingsActiveName"),
            "No customisation is loaded"
        );

        // An editor's commit into a session nothing was ever installed in: the two rules of
        // ONE_CODE, by the raw setter.
        bench
            .document_mut()
            .set_survey_map(customisation_from_text(ONE_CODE).map);
        bench.draw();
        assert!(bench.document().customisation_state().name.is_empty());
        assert_eq!(bench.shown("settingsActiveName"), "Not named");
        assert_eq!(bench.shown("settingsActiveOrigin"), "Edited this session");
        assert_eq!(bench.shown("settingsActiveDefinitions"), "0");
        assert_eq!(bench.shown("settingsActiveRules"), "2 over 1 code");
    }

    #[test]
    fn rules_over_one_code_are_counted_in_the_singular() {
        // ONE_CODE: two rules, both of PG*.
        let mut f = Fixture::new();
        f.document_mut()
            .install_customisation(
                customisation_from_text(ONE_CODE),
                CustomisationOrigin::Loaded,
                false,
            )
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut bench = f.open();
        assert_eq!(bench.shown("settingsActiveName"), "Pegs");
        assert_eq!(bench.shown("settingsActiveRules"), "2 over 1 code");
        // And SITE in its place, over two: the plural.
        bench.install(CustomisationOrigin::Loaded, false);
        bench.draw();
        assert_eq!(bench.shown("settingsActiveRules"), "3 over 2 codes");
    }

    #[test]
    fn the_notice_is_hidden_until_asked_and_its_toggle_is_hidden_when_there_is_none() {
        let mut bench = Fixture::new().open();
        // Nothing is loaded, so there is no notice to ask for.
        assert!(!bench.visible("settingsShowNotice"));
        assert!(!bench.visible("settingsNotice"));

        bench.install(CustomisationOrigin::BuiltIn, true);
        bench.draw();
        assert!(bench.visible("settingsShowNotice"));
        assert!(!bench.checked("settingsShowNotice"));
        assert!(!bench.visible("settingsNotice"), "hidden until asked");
        assert!(NOTICE_BOX.read_only);

        bench.press("settingsShowNotice");
        assert!(bench.visible("settingsNotice"));
        assert_eq!(
            bench.text("settingsNotice"),
            "Site set: for this job only.\nAsk before passing it on."
        );
        bench.press("settingsShowNotice");
        assert!(!bench.visible("settingsNotice"));

        // A session made of four customisations: its own notice, then each source's under its
        // name.
        {
            let mut several = Fixture::new();
            install_noticed_session(several.document_mut());
            let mut merged = several.open();
            merged.press("settingsShowNotice");
            assert_eq!(
                merged.text("settingsNotice"),
                "Base: all rights reserved.\n\nFrom client:\nClient codes, for this job only.\n\n\
                 From marks:\nMarks drawn by hand.\n\nFrom tints:\nTints: free to use."
            );
        }

        // Shown, and then a customisation with no notice takes its place: the notice goes, and the
        // toggle with it.
        bench.press("settingsShowNotice");
        assert!(bench.visible("settingsNotice"));
        bench
            .document_mut()
            .install_customisation(
                read_customisation_fixture("test_symbols"),
                CustomisationOrigin::Loaded,
                false,
            )
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert!(!bench.visible("settingsShowNotice"));
        assert!(!bench.checked("settingsShowNotice"));
        assert!(!bench.visible("settingsNotice"));
        assert!(bench.text("settingsNotice").is_empty());
        assert!(bench.ran().is_empty());
    }

    #[test]
    fn the_linework_codes_are_shown_as_the_document_spells_them_and_edited_in_one_place() {
        let mut bench = Fixture::new().open();
        // The seven defaults, each under the words a person reads its control by - those of the
        // Linework tab the button opens.
        assert_eq!(
            bench.shown("settingsLinework"),
            "Start: ST, End: END, Close: CL, Begin curve: BC, End curve: EC, Join to point: JPN, \
             Rectangle: RECT"
        );

        let codes = LineworkCodes {
            start: String::from("S"),
            rectangle: String::new(), // that control switched off
            ..LineworkCodes::default()
        };
        bench
            .document_mut()
            .set_linework_codes(codes)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert_eq!(
            bench.shown("settingsLinework"),
            "Start: S, End: END, Close: CL, Begin curve: BC, End curve: EC, Join to point: JPN, \
             Rectangle: (off)"
        );

        // Nothing on the page edits them: the button opens their one editor. Of the Customisation
        // page, since the Updates page beside it has a field of its own (the access token's).
        let fields: Vec<String> = bench
            .fields()
            .into_iter()
            .filter(|name| name != "settingsUpdateToken")
            .collect();
        assert_eq!(fields.len(), 2, "the two path fields, and no more: {fields:?}");
        bench.press("settingsEditLinework");
        assert_eq!(bench.opened_linework.get(), 1);
        assert_eq!(bench.opened_codes.get(), 0);
        bench.press("settingsOpenCodes");
        bench.press("settingsOpenSymbols");
        assert_eq!(bench.opened_linework.get(), 1);
        assert_eq!(bench.opened_codes.get(), 1);
        assert_eq!(bench.opened_symbols.get(), 1);
        assert!(bench.ran().is_empty());
    }

    // The list Settings reads against the list of the controls themselves: one label for each,
    // in that order.
    #[test]
    fn the_one_list_of_linework_labels_names_every_control_in_the_order_the_format_lists_them() {
        let members = linework_code_members();
        assert_eq!(members.len(), LINEWORK_CONTROL_LABELS.len());
        for (at, member) in members.iter().enumerate() {
            assert_eq!(member.name, LINEWORK_CONTROL_LABELS[at].member, "{at}");
            assert!(!linework_control_label(member.name).is_empty(), "{at}");
            assert_ne!(
                linework_control_label(member.name),
                member.name,
                "a word for a person, not the format's"
            );
        }
        // By hand, the two a file's name says least about.
        assert_eq!(linework_control_label("arcStart"), "Begin curve");
        assert_eq!(linework_control_label("join"), "Join to point");
        // One the list does not know is shown by the name a file gives it.
        assert_eq!(linework_control_label("spiral"), "spiral");
    }

    // The editor itself: the Survey Code Manager, whose Linework tab labels its seven fields. The
    // words are read from what the tab labels its fields by and its fields hold, not written out
    // again here: this is the case that finds a field labelled otherwise than Settings says it.
    // (The tab's labels are not named controls, so their words are the labeller the tab draws
    // them with - linework_labels' linework_control_label.)
    #[test]
    fn the_linework_controls_are_called_what_their_one_editor_calls_them() {
        let mut host = RecordingHost::new();
        let mut manager = SurveyCodeManagerDialog::new(host.document(), CustomisationContext::with_runner());
        let mut harness = Harness::new();
        harness
            .run(|ui| {
                let ctx = ui.ctx().clone();
                manager.ui(&ctx, &mut host);
            })
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(harness.control("lineworkTab").is_some());

        // Each field of the tab, in the order the page lists the controls, with the spelling a
        // Document nothing has set them in gives it.
        let fields = [
            ("lineworkStart", "start", "ST"),
            ("lineworkEnd", "end", "END"),
            ("lineworkClose", "close", "CL"),
            ("lineworkArcStart", "arcStart", "BC"),
            ("lineworkArcEnd", "arcEnd", "EC"),
            ("lineworkJoin", "join", "JPN"),
            ("lineworkRectangle", "rectangle", "RECT"),
        ];
        let mut words = Vec::new();
        for (name, member, spelling) in fields {
            let field = harness
                .control(name)
                .unwrap_or_else(|| panic!("no {name}"));
            assert_eq!(field.value, spelling, "{name}");
            let label = linework_control_label(member);
            assert!(!label.is_empty(), "{name}");
            words.push(format!("{label}: {spelling}"));
        }

        let bench = Fixture::new().open();
        assert_eq!(
            bench.shown("settingsLinework"),
            words.join(", "),
            "Settings and the Linework tab call a control by different words: make the tab label \
             its fields from LINEWORK_CONTROL_LABELS (linework_labels), the list Settings reads"
        );
    }

    #[test]
    fn a_context_that_fills_nothing_gives_a_dialog_that_runs_nothing() {
        // Every member of the context at its default.
        let mut empty = Fixture::new();
        empty.context = SettingsContext::default();
        let bench = empty.open();
        for name in [
            "settingsImport",
            "settingsExport",
            "settingsReset",
            "settingsKeep",
            "settingsRevert",
            "settingsAutoCodes",
            "settingsAutoLinework",
            "settingsEditLinework",
            "settingsOpenCodes",
            "settingsOpenSymbols",
            "settingsImportBrowse",
            "settingsExportBrowse",
        ] {
            assert!(!bench.enabled(name), "{name}");
        }
        assert!(bench.enabled("settingsClose"));
        assert_eq!(
            bench.status(),
            "Settings was opened on no drawing: nothing here acts on anything."
        );

        // With a Document and nothing else, the buttons that have no editor to open are the ones
        // that are off.
        let mut document_only = Fixture::new();
        document_only.context = SettingsContext {
            reads_drawing: true,
            ..SettingsContext::default()
        };
        let bare = document_only.open();
        for name in [
            "settingsEditLinework",
            "settingsOpenCodes",
            "settingsOpenSymbols",
            "settingsReset",
            "settingsKeep",
            "settingsRevert",
        ] {
            assert!(!bare.enabled(name), "{name}");
        }
        assert!(bare.enabled("settingsImport"));
        assert!(bare.enabled("settingsExport"));
    }

    // ---- Browse ---------------------------------------------------------------------------------

    #[test]
    fn headless_browse_opens_no_file_dialog_and_says_what_to_type() {
        let mut f = Fixture::new();
        f.host.parts.headless = true;
        let mut bench = f.open();

        bench.press("settingsImportBrowse");
        assert_eq!(
            bench.status(),
            "A headless session opens no file dialog: type CUSTOMISE [REPLACE] <file> instead, \
             or fill settingsImportPath."
        );
        assert!(bench.says_a_refusal());
        bench.press("settingsExportBrowse");
        assert_eq!(
            bench.status(),
            "A headless session opens no file dialog: type CUSTOMISE EXPORT <file> instead, or \
             fill settingsExportPath."
        );

        assert_eq!(
            bench.files.borrow().seen,
            0,
            "a file dialog opened: {}",
            bench.files.borrow().last.title
        );
        assert!(bench.ran().is_empty());
        assert!(bench.text("settingsImportPath").is_empty());
        let logged = bench.logged();
        assert_eq!(logged.len(), 2);
        assert!(logged[0].1);
    }

    #[test]
    fn browse_offers_katana_customisation_files_and_only_fills_the_field() {
        let mut bench = Fixture::new().open();
        let filter = "Katana customisation (*.customisation.json *.json)";

        // Import: the committed symbols fixture chosen, as Open would.
        let file = path_text(&customisation_fixture_file("test_symbols"));
        assert!(std::path::Path::new(&file).exists(), "{file}");
        bench.files.borrow_mut().chosen = file.clone();
        bench.press("settingsImportBrowse");
        {
            let files = bench.files.borrow();
            assert_eq!(files.seen, 1, "Browse opens one file dialog");
            assert_eq!(files.last.title, "Import a Katana Customisation");
            assert!(files.filters().contains(&String::from(filter)), "{}", files.last.filter);
        }
        let chosen = bench.text("settingsImportPath");
        assert_eq!(chosen, file);
        assert!(!chosen.contains('\\'), "{chosen}");

        // Export: cancelled, which leaves the field as it was.
        bench.fill("settingsExportPath", "C:/data/out.json");
        bench.files.borrow_mut().chosen.clear();
        bench.press("settingsExportBrowse");
        {
            let files = bench.files.borrow();
            assert_eq!(files.seen, 2, "Browse opens one file dialog");
            assert_eq!(files.last.title, "Export the Customisation");
            assert!(files.filters().contains(&String::from(filter)), "{}", files.last.filter);
        }
        assert_eq!(bench.text("settingsExportPath"), "C:/data/out.json");

        // Choosing a file loads and writes nothing: Import and Export do.
        assert!(bench.ran().is_empty());
        assert!(bench.status().is_empty());
    }

    #[test]
    fn exports_browse_fills_the_field_and_ends_a_name_with_no_json_ending_as_a_customisation_file()
    {
        let mut bench = Fixture::new().open();
        // A folder of this case's own, with a blank in its name and nothing in it.
        let scratch = Scratch::new("settings-exports-browse");
        let folder = path_text(&scratch.folder("site sets"));
        // The name typed in the dialog and accepted, and the file name the field then holds.
        let names = [
            // No ending: given the one Import's own filter shows.
            ("all of it", "all of it.customisation.json"),
            // Ending .json, in any letter case: the person's own, as it is.
            ("plain.json", "plain.json"),
            ("LOUD.JSON", "LOUD.JSON"),
            ("whole.customisation.json", "whole.customisation.json"),
        ];
        for (typed, expected) in names {
            let seen = bench.files.borrow().seen;
            bench.files.borrow_mut().chosen = format!("{folder}/{typed}");
            bench.press("settingsExportBrowse");
            assert_eq!(bench.files.borrow().seen, seen + 1, "{typed}");
            let chosen = bench.text("settingsExportPath");
            let path = std::path::Path::new(&chosen);
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(expected),
                "{typed}"
            );
            assert_eq!(
                path.parent().map(path_text),
                Some(folder.clone()),
                "{chosen}"
            );
            assert!(!chosen.contains('\\'), "{chosen}");
        }
        // Choosing writes nothing and runs nothing: Export does.
        assert!(bench.ran().is_empty(), "{:?}", bench.ran());
        let in_folder = std::fs::read_dir(scratch.path("site sets"))
            .expect("the folder is there")
            .count();
        assert_eq!(in_folder, 0);
        assert!(bench.status().is_empty());
    }

    // The file dialog opens where the field points: QFileDialog given a file's path opens in its
    // folder, and the dialog asks for the path the field holds, its quotes taken off.
    #[test]
    fn browse_opens_where_the_path_in_the_field_points_in_its_quotes_or_out_of_them() {
        let mut bench = Fixture::new().open();
        let browses = [
            ("settingsImportBrowse", "settingsImportPath"),
            ("settingsExportBrowse", "settingsExportPath"),
        ];
        for (at, (button, field)) in browses.into_iter().enumerate() {
            for quoted in [false, true] {
                let scratch = Scratch::new(&format!("settings-browse-opens-{at}-{quoted}"));
                let file = native(&path_text(&scratch.path("here.json")));
                bench.fill(field, &if quoted { format!("\"{file}\"") } else { file });
                bench.files.borrow_mut().chosen.clear(); // cancelled: only where it opened is asked
                let seen = bench.files.borrow().seen;
                bench.press(button);
                let files = bench.files.borrow();
                assert_eq!(files.seen, seen + 1, "{button}");
                let opened_in = std::path::Path::new(&files.last.start)
                    .parent()
                    .map(path_text);
                assert_eq!(
                    opened_in,
                    Some(path_text(&scratch.root)),
                    "{button}{} opened in {}",
                    if quoted { " in quotes" } else { "" },
                    files.last.start
                );
            }
        }
        assert!(bench.ran().is_empty());
    }

    // ---- no default button, no modal box --------------------------------------------------------

    // egui has no default button, and a field takes Enter as text, never as a press.
    #[test]
    fn no_button_is_a_default_and_enter_in_a_field_presses_nothing() {
        let mut bench = Fixture::new().open();
        bench.fill("settingsImportPath", "C:/data/site.json");
        bench.fill("settingsExportPath", "C:/data/out.json");
        assert!(!bench.buttons().is_empty());
        for _ in ["settingsImportPath", "settingsExportPath"] {
            bench.harness.key(egui::Key::Enter, egui::Modifiers::NONE);
            bench.draw();
        }
        assert!(bench.ran().is_empty(), "{:?}", bench.ran());
        assert!(bench.dialog.is_visible());
        assert_eq!(bench.files.borrow().seen, 0, "no box was opened");

        // Close hides it; it is kept by whoever opened it, with what was typed.
        bench.press("settingsClose");
        assert!(!bench.dialog.is_visible());
        bench.dialog.show();
        bench.draw();
        assert_eq!(bench.text("settingsImportPath"), "C:/data/site.json");
    }

    // ---- the words ------------------------------------------------------------------------------

    #[test]
    fn no_text_it_shows_names_another_program_or_a_file_of_an_older_format() {
        let mut seen: Vec<String> = Vec::new();

        // As it opens; with a customisation, its notice shown; and every refusal it words itself.
        let mut bench = Fixture::new().open();
        seen.extend(bench.texts());
        bench.install(CustomisationOrigin::Kept, true);
        bench.draw();
        bench.press("settingsShowNotice");
        seen.extend(bench.texts());
        for name in ["settingsImport", "settingsExport"] {
            bench.press(name); // no file is named
            seen.extend(bench.texts());
        }
        bench.fill("settingsImportPath", "C:/data/a \"b\".json");
        bench.press("settingsImport"); // no line can carry it
        seen.extend(bench.texts());
        bench.fill("settingsImportPath", "C:/data/site.json");
        bench.dirty.set(true);
        for name in ["settingsImport", "settingsReset", "settingsRevert"] {
            bench.press(name); // the code manager holds edits
            seen.extend(bench.texts());
        }
        bench.dirty.set(false);
        bench.host.parts.headless = true;
        for name in ["settingsImportBrowse", "settingsExportBrowse"] {
            bench.press(name); // no file dialog
            seen.extend(bench.texts());
        }
        assert_eq!(bench.files.borrow().seen, 0);
        // The file dialogs a person gets, caught as they open.
        bench.host.parts.headless = false;
        for name in ["settingsImportBrowse", "settingsExportBrowse"] {
            bench.press(name);
            let files = bench.files.borrow();
            seen.push(files.last.title.clone());
            seen.extend(files.filters());
        }
        assert_eq!(bench.files.borrow().seen, 2);
        bench.answer(|_, _| refused(""));
        bench.press("settingsReset"); // refused, with no words of the verb's
        seen.extend(bench.texts());
        // Up to here the session was the kept one, and Reset's tip said what a press costs the
        // kept file. The built-in and not kept: Keep's says it.
        bench.install(CustomisationOrigin::BuiltIn, false);
        bench.draw();
        seen.extend(bench.texts());

        // What the host does not have, and nothing to run a line.
        {
            let mut without = Fixture::new();
            without.context.has_built_in = false;
            without.context.kept_file.clear();
            without.context.run = None;
            let mut bare = without.open();
            bare.press("settingsAutoCodes");
            seen.extend(bare.texts());
        }
        // No drawing at all, and a drawing that has closed.
        {
            let mut nothing = Fixture::new();
            nothing.context = SettingsContext::default();
            seen.extend(nothing.open().texts());
            let mut drawing = Fixture::new();
            drawing.context = SettingsContext {
                reads_drawing: true,
                ..SettingsContext::default()
            };
            let mut closed = drawing.open();
            closed.close_drawing();
            closed.dialog.refresh(&closed.host);
            closed.draw();
            seen.extend(closed.texts());
        }

        for text in &seen {
            // "1" "2d": the word the page must not say, written so that this file does not.
            for word in [concat!("1", "2d"), ".4d", "mapfile"] {
                assert!(
                    !text.to_lowercase().contains(word),
                    "\"{word}\" in: {text}"
                );
            }
        }
        // The walk found the dialog's words, so it did not pass for finding none.
        assert!(seen.len() > 100, "{}", seen.len());
        for expected in [
            "Settings",
            "Customisation",
            "What is active",
            "Replace instead of merging",
            "Reset to Built-in",
            "Revert to Kept",
            "When survey data comes in",
            "Linework control codes",
            "Katana customisation (*.customisation.json *.json)",
            "This program has no built-in customisation to reset to.",
            "The drawing this dialog worked on has closed: nothing here acts on anything any more.",
            "> CUSTOMISE RESET\nRefused: the command was refused.",
        ] {
            assert!(seen.iter().any(|text| text == expected), "{expected}");
        }
        // And the two tips that say what a press costs the kept file, each in the state that
        // shows it.
        for part in [
            "and Revert to Kept then has none to read",
            "is set aside as C:/kept files/customisation.json.bak (CUSTOMISE KEEP)",
        ] {
            assert!(seen.iter().any(|text| text.contains(part)), "{part}");
        }
    }

    // ---- lifetime -------------------------------------------------------------------------------

    // Each control is pressed first after the drawing has gone, in a dialog of its own: the first
    // press is what finds out.
    #[test]
    fn the_dialog_outlives_its_document_and_then_runs_nothing() {
        for name in [
            "settingsImport",
            "settingsExport",
            "settingsReset",
            "settingsKeep",
            "settingsRevert",
            "settingsAutoCodes",
            "settingsAutoLinework",
        ] {
            let mut f = Fixture::new();
            f.context = SettingsContext {
                reads_drawing: true,
                run: Some(run_through_host()),
                has_built_in: true,
                kept_file: String::from(KEPT_FILE),
                ..SettingsContext::default()
            };
            let mut bench = f.open();
            bench.fill("settingsImportPath", "C:/data/site.json");
            bench.fill("settingsExportPath", "C:/data/out.json");
            assert!(bench.enabled(name), "{name}");

            // A change is notified, and the drawing goes before it is delivered.
            bench.document_mut().set_automation(automation(false, false));
            bench.close_drawing();
            bench.draw();

            bench.press(name);
            assert!(bench.ran().is_empty(), "{name} ran {:?}", bench.ran());
            assert_eq!(
                bench.status(),
                "The drawing this dialog worked on has closed: nothing here acts on anything any \
                 more.",
                "{name}"
            );
            assert!(bench.says_a_refusal(), "{name}");
            for each in [
                "settingsImport",
                "settingsExport",
                "settingsReset",
                "settingsKeep",
                "settingsRevert",
                "settingsAutoCodes",
                "settingsAutoLinework",
                "settingsImportBrowse",
                "settingsExportBrowse",
                "settingsImportPath",
                "settingsExportPath",
            ] {
                assert!(!bench.enabled(each), "{name}: {each}");
            }
            assert!(bench.enabled("settingsClose"), "{name}");
            bench.dialog.refresh(&bench.host);
            bench.draw();
        }
    }
}
