//! The twins of tests/qt_widgets/plotting/test_sheet_set_menu.cpp: the Sheet Set menu - the set
//! saved and loaded back, another set's sheets appended, the set copied as JSON, the page setup
//! saved without a plot, and a headless session told which verb to type.
//!
//! The C++ answered the system's file dialogs and questions as they came up; the twins answer
//! them where the editor asks (SheetEditor::choose_file and answer_question). The C++ suite
//! FileDialogAnswer tested that answer itself on a real QFileDialog and has no twin: the window
//! has no file dialog of its own to answer.

mod sheet_set_menu {
    use std::cell::RefCell;
    use std::rc::Rc;

    use katana_cad::plot::PlotColourMode;
    use katana_cad::plotting::sheet_json::sheet_set_to_json;
    use katana_cad::plotting::sheet_set::SheetSet;
    use katana_cad::plotting::sheet_verbs::{read_sheet_set_file, write_sheet_set_file};
    use katana_cad::{CommandInterpreter, CustomisationOrigin, Document};
    use katana_entity::{Customisation, CustomisationPlotFrame};
    use katana_window::host::Clipboard;
    use katana_window::sheet_editor::{FileAsk, QuestionAsk};

    use crate::support::{Shown, double_eq, source_with_document};
    use crate::window_support::Scratch;

    /// Lines typed into `document`, as the test's own set-up.
    fn type_lines(document: &mut Document, lines: &[&str]) {
        let mut interpreter = CommandInterpreter::new();
        for line in lines {
            if let Err(error) = interpreter.run(document, line) {
                panic!("{line}: {}", error.describe());
            }
        }
    }

    fn shown(document: Document) -> Shown {
        Shown::bare(document, source_with_document())
    }

    /// The boxes that come up, answered in turn: each file dialog with the next of `files`, each
    /// question with the next of `answers` (No once they run out).
    #[derive(Default)]
    struct Answers {
        files_seen: usize,
        questions_seen: usize,
        question: String,
        question_name: String,
    }

    fn answer(shown: &mut Shown, files: Vec<String>, answers: Vec<bool>) -> Rc<RefCell<Answers>> {
        let seen = Rc::new(RefCell::new(Answers::default()));
        let files_seen = Rc::clone(&seen);
        shown.editor.choose_file = Some(Box::new(move |_: &FileAsk| {
            let mut seen = files_seen.borrow_mut();
            let file = files.get(seen.files_seen).cloned().unwrap_or_default();
            seen.files_seen += 1;
            file
        }));
        let asked = Rc::clone(&seen);
        shown.editor.answer_question = Some(Box::new(move |ask: &QuestionAsk| {
            let mut seen = asked.borrow_mut();
            seen.question.clone_from(&ask.text);
            seen.question_name.clone_from(&ask.name);
            let yes = answers.get(seen.questions_seen).copied().unwrap_or(false);
            seen.questions_seen += 1;
            yes
        }));
        seen
    }

    fn said(shown: &Shown, start: &str) -> bool {
        shown.messages().iter().any(|message| message.starts_with(start))
    }

    #[test]
    fn the_menu_comes_first_with_every_item_by_name() {
        let shown = shown(Document::new(None));
        let menus = shown.editor.menus(&shown.host);
        let first = &menus[0];
        assert_eq!(first.name, "sheetSetMenu");
        assert_eq!(first.title.replace('&', ""), "Sheet Set");
        for name in [
            "sheetSaveSet",
            "sheetLoadSet",
            "sheetAppendSet",
            "sheetCopySetJson",
            "sheetPageSetup",
        ] {
            let action = first
                .actions()
                .into_iter()
                .find(|action| action.name == name)
                .unwrap_or_else(|| panic!("no {name}"));
            assert!(!action.tip.is_empty(), "{name}");
            assert!(shown.editor.action(&shown.host, name).is_some(), "{name}");
        }
    }

    #[test]
    fn the_set_is_saved_and_loaded_back_asking_first() {
        let scratch = Scratch::new("sheet-set-round-trip");
        let mut document = Document::new(None);
        type_lines(
            &mut document,
            &[
                "SHEET NEW PLAN",
                "VIEW ADD 1 notes",
                "SHEET NEW KEY",
                "TITLEBLOCK organisation \"Example Surveys\"",
            ],
        );
        let saved = SheetSet::clone(&document.sheet_set());
        let mut shown = shown(document);
        let steps = shown.steps();
        let file = scratch.path("set one.json").to_string_lossy().into_owned();
        {
            let answers = answer(&mut shown, vec![file.clone()], Vec::new());
            shown.trigger("sheetSaveSet");
            assert_eq!(answers.borrow().files_seen, 1);
        }
        let read = read_sheet_set_file(&scratch.path("set one.json"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read, saved);
        assert!(said(&shown, "> SHEETS SAVE \""));
        assert!(said(&shown, "wrote 2 sheets to "));
        assert_eq!(shown.steps(), steps, "saving is no edit");

        // Changed since, then loaded: asked first, and one step undo takes back.
        type_lines(shown.document_mut(), &["SHEET NEW SCRAP"]);
        shown.observe();
        let changed = shown.sheet_set();
        {
            let answers = answer(&mut shown, vec![file.clone()], vec![true]);
            shown.trigger("sheetLoadSet");
            let answers = answers.borrow();
            assert_eq!(answers.questions_seen, 1);
            assert_eq!(answers.question_name, "sheetLoadSetQuestion");
            assert_eq!(
                answers.question,
                "Replace the 3 sheets there are now? Undo brings them back."
            );
        }
        assert_eq!(shown.sheet_set(), saved);
        assert_eq!(shown.document().history().undo_name(), "LOAD_SHEETS");
        shown.undo();
        assert_eq!(shown.sheet_set(), changed);

        // Cancelled at the question: nothing is loaded.
        let before = shown.steps();
        {
            let answers = answer(&mut shown, vec![file], vec![false]);
            shown.trigger("sheetLoadSet");
            assert_eq!(answers.borrow().questions_seen, 1);
        }
        assert_eq!(shown.sheet_set(), changed);
        assert_eq!(shown.steps(), before);
    }

    #[test]
    fn another_sets_sheets_are_appended_in_one_step() {
        let scratch = Scratch::new("sheet-set-append");
        {
            let mut other = Document::new(None);
            type_lines(
                &mut other,
                &["SHEET NEW NORTH", "VIEW ADD 1 notes", "SHEET NEW SOUTH"],
            );
            write_sheet_set_file(&other.sheet_set(), &scratch.path("other.json"))
                .expect("written");
        }
        let mut document = Document::new(None);
        type_lines(&mut document, &["SHEET NEW COVER", "VIEW ADD 1 notes"]);
        let mut shown = shown(document);
        let steps = shown.steps();
        {
            let answers = answer(
                &mut shown,
                vec![scratch.path("other.json").to_string_lossy().into_owned()],
                Vec::new(),
            );
            shown.trigger("sheetAppendSet");
            let answers = answers.borrow();
            assert_eq!(answers.files_seen, 1);
            assert_eq!(
                answers.questions_seen, 0,
                "appending replaces nothing, so asks nothing"
            );
        }
        let set = shown.sheet_set();
        assert_eq!(set.sheets.len(), 3);
        assert_eq!(set.sheets[0].name, "COVER");
        assert_eq!(set.sheets[1].name, "NORTH");
        assert_eq!(set.sheets[2].name, "SOUTH");
        assert_ne!(set.sheets[1].viewports[0].id, set.sheets[0].viewports[0].id);
        assert_eq!(shown.steps(), steps + 1);
        assert_eq!(shown.document().history().undo_name(), "APPEND_SHEETS");
        assert!(said(&shown, "appended 2 sheets from "));
        // The first appended sheet is shown.
        assert_eq!(shown.editor.current_sheet(), 1);
    }

    #[test]
    fn copy_sheet_set_as_json_puts_the_sets_json_on_the_clipboard() {
        let mut document = Document::new(None);
        type_lines(&mut document, &["SHEET NEW PLAN", "VIEW ADD 1 notes"]);
        let mut shown = shown(document);
        let _ = shown.host.parts.clipboard.set_text("");
        let steps = shown.steps();
        shown.trigger("sheetCopySetJson");
        let json = sheet_set_to_json(&shown.document().sheet_set()).expect("the JSON");
        assert_eq!(shown.host.parts.clipboard.text().unwrap_or_default(), json);
        assert_eq!(shown.steps(), steps);
        assert!(said(&shown, "> SHEETS JSON"));
    }

    #[test]
    fn page_setup_is_saved_without_a_plot() {
        let mut document = Document::new(None);
        type_lines(
            &mut document,
            &["SHEET NEW PLAN", "SHEETS PAGESETUP style=mono dpi=200"],
        );
        let mut shown = shown(document);
        shown.trigger("sheetPageSetup");
        shown.draw();
        assert_eq!(
            shown.editor.plot_dialog().map(|dialog| String::from(dialog.name())),
            Some(String::from("pageSetupDialog"))
        );
        // The page setup's controls, starting from the set's; none of a plot's.
        for name in [
            "plotColourMode",
            "plotLineWeightScale",
            "plotDpi",
            "plotPattern",
            "pageSetupFilePerSheet",
            "pageSetupSave",
        ] {
            let control = shown.control(name).unwrap_or_else(|| panic!("no {name}"));
            assert!(control.visible, "{name}");
        }
        for hidden in [
            "plotSheetsAll",
            "plotFormat",
            "plotFile",
            "plotFolder",
            "plotPrinter",
            "plotOpenAfter",
            "plotKeepSetup",
        ] {
            assert!(
                shown.control(hidden).is_none_or(|control| !control.visible),
                "{hidden}"
            );
        }
        assert!(shown.control("plotRun").is_none());
        let dialog = shown.editor.plot_dialog().expect("the dialog");
        assert_eq!(dialog.colour_mode_name(), "monochrome");
        assert_eq!(dialog.dpi, 200);

        shown.fill_in("pageSetupDialog", "plotColourMode", "Greyscale");
        shown.fill_in("pageSetupDialog", "plotLineWeightScale", "0.7");
        shown.fill_in("pageSetupDialog", "plotDpi", "150");
        shown.fill_in("pageSetupDialog", "plotPattern", "{n:02} {name}");
        shown.fill_in("pageSetupDialog", "pageSetupFilePerSheet", "on");
        let line = shown.editor.plot_dialog().expect("the dialog").page_setup_line();
        assert_eq!(
            line,
            "SHEETS PAGESETUP style=greyscale lineweight=0.7 dpi=150 pattern=\"{n:02} {name}\" \
             filepersheet=on"
        );
        let steps = shown.steps();
        shown.press_in("pageSetupDialog", "pageSetupSave");
        let setup = shown.document().sheet_set().page_setup.clone();
        assert_eq!(setup.colour_mode, PlotColourMode::Greyscale);
        assert!(double_eq(setup.line_weight_scale, 0.7));
        assert_eq!(setup.dpi, 150.0);
        assert_eq!(setup.file_name_pattern, "{n:02} {name}");
        assert!(setup.file_per_sheet);
        assert_eq!(shown.steps(), steps + 1);
        assert_eq!(shown.document().history().undo_name(), "PAGE_SETUP");
        // The line and its reply, and no plot: nothing else was said.
        let messages = shown.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[0],
            "> SHEETS PAGESETUP style=greyscale lineweight=0.7 dpi=150 pattern=\"{n:02} {name}\" \
             filepersheet=on"
        );
        assert!(messages[1].starts_with("pagesetup style=greyscale"));
    }

    // A customisation's title frames (docs/plotting.md, "A customisation's title frames"): Page
    // Setup's Frame lists the default and the frames the customisation in use offers, and a frame
    // chosen there is the SHEETS PAGESETUP line's frame=, which puts it on every framed sheet in
    // one step. The customisation is invented; the frame it offers is the one compiled in.
    #[test]
    fn page_setup_lists_the_frames_on_offer_and_runs_the_one_chosen_as_its_line() {
        const FRAME: &str = "nsw_a3_utility_cross_section";
        let mut document = Document::new(None);
        document
            .install_customisation(
                Customisation {
                    name: String::from("Probe"),
                    plot_frames: vec![CustomisationPlotFrame {
                        name: String::from(FRAME),
                    }],
                    ..Customisation::default()
                },
                CustomisationOrigin::Loaded,
                false,
            )
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        type_lines(&mut document, &["SHEET NEW PLAN", "SHEET NEW BARE frame=off"]);
        let mut shown = shown(document);
        shown.trigger("sheetPageSetup");
        shown.draw();
        let frame = shown.control("pageSetupFrame").expect("the Frame choice");
        assert!(frame.visible);
        assert_eq!(frame.items, vec![String::from("a3_landscape"), String::from(FRAME)]);
        assert_eq!(frame.value, "a3_landscape");
        // Nothing else chosen: Save names no frame, so no sheet's frame is touched by it.
        let unchanged = shown.editor.plot_dialog().expect("the dialog").page_setup_line();
        assert!(!unchanged.contains("frame="), "{unchanged}");

        shown.fill_in("pageSetupDialog", "pageSetupFrame", FRAME);
        let line = shown.editor.plot_dialog().expect("the dialog").page_setup_line();
        assert_eq!(
            line,
            format!(
                "SHEETS PAGESETUP style=colour lineweight=1 dpi=300 pattern=\"{{set}}{{n:02}} \
                 {{name}}\" filepersheet=off frame={FRAME}"
            )
        );
        let steps = shown.steps();
        shown.press_in("pageSetupDialog", "pageSetupSave");
        assert_eq!(shown.steps(), steps + 1);
        assert_eq!(shown.document().history().undo_name(), "PAGE_SETUP");
        let set = shown.document().sheet_set();
        assert_eq!(set.page_setup.frame, FRAME);
        assert_eq!(set.sheets[0].frame, FRAME);
        // The sheet whose frame was off keeps it off.
        assert!(set.sheets[1].frame.is_empty());
        let messages = shown.messages();
        assert_eq!(messages[0], format!("> {line}"));
        assert!(messages[1].ends_with(&format!(" frame={FRAME}")), "{}", messages[1]);
    }

    #[test]
    fn page_setup_lists_the_default_frame_alone_when_the_customisation_offers_none() {
        let mut document = Document::new(None);
        type_lines(&mut document, &["SHEET NEW PLAN"]);
        let mut shown = shown(document);
        shown.trigger("sheetPageSetup");
        shown.draw();
        let frame = shown.control("pageSetupFrame").expect("the Frame choice");
        assert_eq!(frame.items, vec![String::from("a3_landscape")]);
        assert_eq!(frame.value, "a3_landscape");
    }

    #[test]
    fn a_page_setup_that_cannot_be_typed_cannot_be_saved() {
        let mut document = Document::new(None);
        type_lines(&mut document, &["SHEET NEW PLAN"]);
        let mut shown = shown(document);
        shown.trigger("sheetPageSetup");
        shown.draw();
        assert!(shown.control("pageSetupSave").expect("Save").enabled);
        shown.fill_in("pageSetupDialog", "plotPattern", "{nope}");
        assert!(!shown.control("pageSetupSave").expect("Save").enabled);
        assert!(!shown.control("plotProblem").expect("the problem").text.is_empty());
        // The command line has no way to type a double quote in a value.
        shown.fill_in("pageSetupDialog", "plotPattern", "say \"{n}\"");
        assert!(!shown.control("pageSetupSave").expect("Save").enabled);
        assert!(
            shown
                .control("plotProblem")
                .expect("the problem")
                .text
                .contains("double quote")
        );
        // A backslash is written so the verb reads it back as itself.
        shown.fill_in("pageSetupDialog", "plotPattern", "plots\\{n}");
        assert!(
            shown
                .editor
                .plot_dialog()
                .expect("the dialog")
                .page_setup_line()
                .contains("pattern=\"plots\\\\{n}\"")
        );
        shown.press_in("pageSetupDialog", "plotCancel");
    }

    #[test]
    fn a_headless_session_names_the_verbs_instead_of_opening_a_file_dialog() {
        let mut document = Document::new(None);
        type_lines(&mut document, &["SHEET NEW PLAN"]);
        let mut shown = shown(document);
        shown.editor.set_headless(true);
        let answers = answer(&mut shown, Vec::new(), Vec::new());
        for (item, verb) in [
            ("sheetSaveSet", "SHEETS SAVE"),
            ("sheetLoadSet", "SHEETS LOAD"),
            ("sheetAppendSet", "SHEETS APPEND"),
        ] {
            let errors = shown.errors();
            shown.trigger(item);
            assert_eq!(shown.errors(), errors + 1, "{item}");
            let last = shown.messages().last().cloned().unwrap_or_default();
            assert!(last.contains(verb), "{item}");
        }
        assert_eq!(answers.borrow().files_seen, 0);
        assert_eq!(answers.borrow().questions_seen, 0);
    }
}
