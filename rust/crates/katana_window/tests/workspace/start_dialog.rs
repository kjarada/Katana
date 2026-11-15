//! The twins of tests/qt_widgets/test_start_dialog.cpp: the start screen, driven as a person uses
//! it. Start hands the lines it shows to the window's executor, one by one, in order - a recorder
//! here - and a choice that is what the session already has makes no line. The built-ins are two
//! invented ones, so that the test is the same whatever this build compiled in.

mod start_dialog {
    use std::sync::Arc;

    use katana_cad::customisation_host::install_built_in_customisation;
    use katana_cad::{BuiltInCustomisation, BuiltInEntry, Document};
    use katana_core::ErrorCode;
    use katana_entity::customisation::{customisation_digest, customisation_from_json};
    use katana_window::drive::Harness;
    use katana_window::host::{Answer, RecordingHost, VerbOutcome};
    use katana_window::start_dialog::{StartChoices, StartDialog};

    fn entry_named(name: &str) -> BuiltInEntry {
        let text = format!(
            "{{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"{name}\", \
             \"description\": \"An invented one.\"}}"
        );
        let read = customisation_from_json(text.as_bytes())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        BuiltInEntry {
            name: String::from(name),
            description: String::from("An invented one."),
            source: String::new(),
            built_in: BuiltInCustomisation {
                customisation: Some(Arc::new(read)),
                digest: customisation_digest(text.as_bytes()),
                ..BuiltInCustomisation::default()
            },
        }
    }

    /// Two built-ins, the first the default; a name with a blank in it, so the line must quote it.
    fn offered() -> Vec<BuiltInEntry> {
        vec![entry_named("Alpha"), entry_named("Beta Works")]
    }

    /// A session as a start leaves it: the first built-in installed, and kept.
    fn start_on_alpha(document: &mut Document) {
        assert!(install_built_in_customisation(document, &offered()[0].built_in, true).is_ok());
        document.set_built_in_customisation_name(String::from("Alpha"));
    }

    /// The window's executor as a recorder: every line taken, answered "done".
    fn recorder(document: Document) -> RecordingHost {
        RecordingHost::with_document(document)
            .answering(Answer::Fixed(VerbOutcome::new(true, "done", "")))
    }

    #[test]
    fn opens_on_the_session_as_it_is_and_that_makes_no_line() {
        let mut document = Document::new(None);
        start_on_alpha(&mut document);
        let mut host = recorder(document);
        let mut dialog = StartDialog::new(host.document(), StartChoices::default(), &offered());
        let mut harness = Harness::new();
        harness
            .run(|ui| dialog.ui(ui, &mut host))
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(harness.target_record("startDialog").is_some());
        for name in [
            "startLogo",
            "startTitle",
            "startCustomisation",
            "startCustomisationFile",
            "startCustomisationBrowse",
            "startCustomisationReplace",
            "startCustomisationKeep",
            "startCrs",
            "startCrsCheck",
            "startLine",
            "startStatus",
            "startShowAtStart",
            "startOk",
            "startSkip",
        ] {
            assert!(harness.control(name).is_some(), "{name}");
        }
        assert_eq!(
            dialog.customisation_items(),
            [
                "Alpha (built in, the default)",
                "Beta Works (built in)",
                "From a customisation file..."
            ]
        );
        assert_eq!(dialog.customisation_index(), Some(0));
        assert_eq!(dialog.crs_index(), Some(0), "local coordinates");

        let lines = dialog.lines(host.document()).expect("lines");
        assert!(lines.is_empty());
        assert_eq!(
            harness.report("startLine").as_deref(),
            Ok("Nothing to change: the session starts as it is.")
        );
        // No kept file: nothing to keep, whatever is chosen.
        dialog.set_customisation(1);
        assert!(!dialog.keep_enabled());
        assert!(dialog.start(&mut host));
        assert_eq!(host.lines(), ["CUSTOMISE BUILTIN \"Beta Works\""]);
        assert_eq!(dialog.result(), Some(true));
    }

    #[test]
    fn a_built_in_kept_and_a_coordinate_system_are_the_lines_in_order() {
        let mut document = Document::new(None);
        start_on_alpha(&mut document);
        let mut host = recorder(document);
        let last = StartChoices {
            kept_file: String::from("C:/Users/someone/AppData/Katana/customisation.json"),
            ..StartChoices::default()
        };
        let mut dialog = StartDialog::new(host.document(), last.clone(), &offered());
        // The session was the one the next start gives, so a new choice is too unless that is
        // cleared.
        assert!(!dialog.keep_enabled(), "the session as it is makes nothing to keep");
        dialog.set_customisation(1);
        assert!(dialog.keep_enabled());
        assert!(dialog.keep_checked());
        dialog.set_crs_text("epsg:7856");
        assert!(
            dialog.crs_check().starts_with("EPSG:7856  GDA2020 / MGA zone 56"),
            "{}",
            dialog.crs_check()
        );
        let expected = [
            "CUSTOMISE BUILTIN \"Beta Works\"",
            "CUSTOMISE KEEP",
            "CRS SET epsg:7856",
        ];
        assert_eq!(dialog.line_text(host.document()), expected.join("\n"));
        assert!(dialog.start(&mut host));
        assert_eq!(host.lines(), expected);
        // What the window keeps for the next start: the system as the verb takes it.
        assert_eq!(dialog.coordinate_system(), "epsg:7856");

        // Cleared, the choice is this session's alone.
        let mut second = recorder(Document::new(None));
        start_on_alpha(second.document_mut());
        let mut again = StartDialog::new(second.document(), last, &offered());
        again.set_customisation(1);
        again.set_keep(false);
        assert!(again.start(&mut second));
        assert_eq!(second.lines(), ["CUSTOMISE BUILTIN \"Beta Works\""]);
    }

    #[test]
    fn a_file_is_loaded_merged_or_in_place_and_no_file_is_refused() {
        let mut document = Document::new(None);
        start_on_alpha(&mut document);
        let mut host = recorder(document);
        let mut dialog = StartDialog::new(host.document(), StartChoices::default(), &offered());
        let last = dialog.customisation_items().len() - 1;
        dialog.set_customisation(last);
        assert!(dialog.file_enabled());
        let none = dialog.lines(host.document()).expect_err("no file");
        assert_eq!(
            none.message,
            "no file is named: type its path, or choose it with Browse"
        );
        assert!(!dialog.start_enabled(host.document()));
        assert!(!dialog.start(&mut host));
        assert!(dialog.status().starts_with("Not started: no file is named"));
        assert!(host.lines().is_empty());

        // As a file manager hands a path over, quotes and native separators and all.
        let native = if cfg!(windows) {
            String::from("C:\\site data\\codes.customisation.json")
        } else {
            String::from("C:/site data/codes.customisation.json")
        };
        dialog.set_file(&format!("\"{native}\""));
        assert_eq!(
            dialog.lines(host.document()).expect("lines"),
            ["CUSTOMISE \"C:/site data/codes.customisation.json\""]
        );
        dialog.set_replace(true);
        assert_eq!(
            dialog.lines(host.document()).expect("lines"),
            ["CUSTOMISE REPLACE \"C:/site data/codes.customisation.json\""]
        );
    }

    #[test]
    fn anything_but_a_coordinate_system_is_refused_and_starts_nothing() {
        let mut document = Document::new(None);
        start_on_alpha(&mut document);
        let mut host = recorder(document);
        let mut dialog = StartDialog::new(host.document(), StartChoices::default(), &offered());
        dialog.set_crs_text("no such system");
        assert!(dialog.crs_check().starts_with("Not a coordinate system"));
        let refused = dialog.lines(host.document()).expect_err("no system");
        assert_eq!(refused.code, ErrorCode::InvalidCrs);
        assert!(!dialog.start_enabled(host.document()));
        assert!(!dialog.start(&mut host));
        assert!(host.lines().is_empty());
        assert_ne!(dialog.result(), Some(true));
    }

    #[test]
    fn the_system_chosen_last_is_offered_and_the_drawings_own_comes_first() {
        let mut document = Document::new(None);
        start_on_alpha(&mut document);
        let mut last = StartChoices {
            coordinate_system: String::from("EPSG:7856"),
            ..StartChoices::default()
        };
        {
            let mut dialog = StartDialog::new(&document, last.clone(), &offered());
            assert!(dialog.crs_text().starts_with("EPSG:7856  "));
            assert_eq!(dialog.coordinate_system(), "EPSG:7856");
            // Without a runner the line goes through the CRS verb on the Document: one undo step.
            let before = document.history().undo_count();
            assert!(dialog.start_on(&mut document));
            assert_eq!(document.metadata().coordinate_system, "EPSG:7856");
            assert_eq!(document.history().undo_count(), before + 1);
        }
        // The drawing has a system now; the one chosen last does not replace it, and leaving it
        // is no line.
        last.coordinate_system = String::from("EPSG:28356");
        let mut dialog = StartDialog::new(&document, last, &offered());
        assert_eq!(dialog.coordinate_system(), "EPSG:7856");
        assert!(dialog.lines(&document).expect("lines").is_empty());
        // Back to local coordinates clears it.
        dialog.set_crs_index(0);
        assert_eq!(dialog.lines(&document).expect("lines"), ["CRS CLEAR"]);
    }

    #[test]
    fn show_at_start_is_what_it_was_given_until_changed() {
        let document = Document::new(None);
        let last = StartChoices {
            show_at_start: false,
            ..StartChoices::default()
        };
        let mut dialog = StartDialog::new(&document, last, &offered());
        assert!(!dialog.show_at_start());
        dialog.set_show_at_start(true);
        assert!(dialog.show_at_start());
    }

    // A run whose seam says none starts with nothing installed: the screen opens on that, and
    // Start then changes nothing; a built-in is one choice away.
    #[test]
    fn a_session_with_nothing_installed_is_offered_as_it_is_and_start_changes_nothing() {
        let mut host = recorder(Document::new(None));
        let mut dialog = StartDialog::new(host.document(), StartChoices::default(), &offered());
        assert_eq!(
            dialog.customisation_items()[0],
            "None - as it is, no customisation installed"
        );
        assert_eq!(dialog.customisation_index(), Some(0));
        assert!(dialog.lines(host.document()).expect("lines").is_empty());
        dialog.set_customisation(1);
        assert_eq!(
            dialog.lines(host.document()).expect("lines"),
            ["CUSTOMISE BUILTIN Alpha"]
        );
        dialog.set_customisation(0);
        assert!(dialog.start(&mut host));
        assert!(host.lines().is_empty());
    }
}
