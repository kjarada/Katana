//! The twins of tests/qt_widgets/customisation/test_drawing_summary_dialog.cpp: what File >
//! Drawing Summary shows of a drawing laid out by hand in each test, that it follows the drawing
//! as commands run, and that its buttons run the line and call the window's hooks they name. The
//! runner and the hooks are the test's own.
//!
//! The customisation pane is what a bare CUSTOMISE replies (docs/customisation.md, "The verbs"):
//! its expected text is written from that chapter's records and from the fixture
//! tests/data/customisation/test_survey.customisation.json - 11 rules over 8 keys, counted by
//! hand - not from a run.

use std::cell::RefCell;
use std::rc::Rc;

use katana_cad::Document;
use katana_entity::style_library::add_or_replace;
use katana_entity::{LineStyle, StyleLibrary};
use katana_window::customisation::{DrawingSummaryContext, DrawingSummaryDialog};
use katana_window::host::{Host, RecordingHost, VerbOutcome};

use crate::support::{Bench, fixed_host, run};
use crate::window_support::customisation_data;

fn summary(host: RecordingHost) -> Bench<DrawingSummaryDialog> {
    Bench::new(host, |document| {
        DrawingSummaryDialog::new(document, DrawingSummaryContext::default())
    })
}

/// A context whose Copy as JSON runs its line through the host.
fn running() -> DrawingSummaryContext {
    DrawingSummaryContext {
        has_runner: true,
        ..DrawingSummaryContext::default()
    }
}

mod drawing_summary_dialog {
    use super::*;

    #[test]
    fn it_says_what_the_drawing_holds_and_what_is_current() {
        let mut host = RecordingHost::new();
        run(&mut host, "RECT 0,0 10,5");
        run(&mut host, "CIRCLE 5,5 2");
        run(&mut host, "LAYER NEW roads");
        run(&mut host, "UNDO");
        run(&mut host, "SELECT 2");
        let bench = summary(host);
        assert!(bench.harness.target_record("drawingSummaryDialog").is_some());
        assert_eq!(
            bench.text("drawingSummaryProject"),
            "none - not saved to a project yet, with unsaved changes"
        );
        assert_eq!(
            bench.text("drawingSummaryDrawing"),
            "2 entities, 1 layer, 0 alignments, 0 sheets"
        );
        assert!(
            bench
                .text("drawingSummaryCurrent")
                .starts_with("layer 0, style ByLayer, annotation scale 1:"),
            "{}",
            bench.text("drawingSummaryCurrent")
        );
        assert_eq!(bench.text("drawingSummarySelection"), "1 entity selected");
        assert_eq!(
            bench.text("drawingSummaryHistory"),
            format!(
                "2 steps to undo (next: {}), 1 step to redo (next: {})",
                bench.document().history().undo_name(),
                bench.document().history().redo_name()
            )
        );
        // Nothing was installed: no name, origin none, not kept, and the settings a session starts
        // with - both automation switches on, the seven linework control codes at their defaults.
        assert_eq!(
            bench.value("drawingSummaryCustomisation"),
            "No customisation is loaded.\n  CUSTOMISE <file> [<file>...]  loads Katana \
             customisation files\ncustomisation name=\"\" origin=none kept=no definitions=0 \
             codes=0 rules=0 colours=0\nautomation auto.codes=on auto.linework=on\nlinework \
             linework.start=ST linework.end=END linework.close=CL linework.arcstart=BC \
             linework.arcend=EC linework.join=JPN linework.rectangle=RECT"
        );
    }

    #[test]
    fn the_customisation_pane_is_the_customise_lines_reply_and_follows_a_load() {
        // The fixture's 11 rules over 8 keys loaded into an empty session, which takes the file's
        // name and has it as its one source - of rules alone. The pane reads the Document, so it
        // needs no list from the window.
        let fixture = customisation_data().join("test_survey.customisation.json");
        let mut bench = summary(RecordingHost::new());
        let path = fixture.to_string_lossy().replace('\\', "/");
        run(&mut bench.host, &format!("CUSTOMISE \"{path}\""));
        bench.draw();
        let pane = bench.value("drawingSummaryCustomisation");
        assert!(
            pane.contains(
                "11 survey code rules over 8 distinct codes\ncustomisation name=test_survey \
                 origin=loaded kept=no definitions=0 codes=8 rules=11 colours=0\nsource \
                 name=test_survey definitions=no rules=yes\nautomation auto.codes=on \
                 auto.linework=on\n"
            ),
            "{pane}"
        );
        assert!(
            !pane.contains("Loaded files, in load order"),
            "the older prose"
        );
    }

    #[test]
    fn no_record_of_the_customisation_pane_is_cut_at_the_panes_edge() {
        // The linework record is in every reply and is 143 characters long (the word, then its
        // seven key=value pairs: 8 + 18 + 17 + 18 + 21 + 19 + 18 + 24) - wider than the pane in
        // any fixed-width face at the dialog's own width. Nothing of it may lie past the edge: the
        // pane wraps, so there is nothing to scroll sideways. Left unwrapped, the record's last
        // codes were out of sight behind a scroll bar.
        let bench = summary(RecordingHost::new());
        assert!(
            bench
                .value("drawingSummaryCustomisation")
                .ends_with("linework.join=JPN linework.rectangle=RECT")
        );
        // The C++ asks the pane's own horizontal scroll bar; an egui text box has none, so what is
        // asked is the same thing said of the frame: the pane lies inside the dialog, and the
        // dialog's body scrolls no way sideways.
        let pane = bench.control("drawingSummaryCustomisation");
        let dialog = bench
            .harness
            .target_record("drawingSummaryDialog")
            .expect("the dialog is drawn");
        assert!(
            pane.rect.right() <= dialog.rect.right(),
            "a record runs past the pane's edge"
        );
        assert!(
            dialog
                .scroll
                .is_none_or(|scroll| scroll.max_offset().x <= 0.5),
            "a record runs past the pane's edge"
        );
    }

    #[test]
    fn it_follows_the_drawing_as_commands_run() {
        let mut bench = summary(RecordingHost::new());
        assert_eq!(
            bench.text("drawingSummaryDrawing"),
            "0 entities, 1 layer, 0 alignments, 0 sheets"
        );
        run(&mut bench.host, "POINT 1,1");
        run(&mut bench.host, "POINT 2,2");
        // Nothing yet: the dialog refreshes at its next frame, once.
        assert_eq!(
            bench.text("drawingSummaryDrawing"),
            "0 entities, 1 layer, 0 alignments, 0 sheets"
        );
        bench.draw();
        assert_eq!(
            bench.text("drawingSummaryDrawing"),
            "2 entities, 1 layer, 0 alignments, 0 sheets"
        );
    }

    #[test]
    fn a_name_no_loaded_library_defines_is_listed_and_opens_the_style_manager() {
        let mut host = RecordingHost::new();
        let mut library = StyleLibrary::new();
        let fence = LineStyle {
            name: String::from("TEST Fence"),
            group: String::from("FENCES"),
            ..LineStyle::default()
        };
        add_or_replace(&mut library, fence).expect("the fence is added");
        host.document_mut().set_style_library(library);
        run(&mut host, "STYLE NEW fences");
        run(&mut host, "STYLE SET fences linetype \"TEST Fence\"");
        // The library the style was drawn with, replaced by one without it.
        host.document_mut().set_style_library(StyleLibrary::new());

        let shown: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let heard = Rc::clone(&shown);
        let context = DrawingSummaryContext {
            show_missing: Some(Rc::new(move |name: &str| {
                heard.borrow_mut().push(String::from(name));
            })),
            ..DrawingSummaryContext::default()
        };
        let mut bench = Bench::new(host, |document| {
            DrawingSummaryDialog::new(document, context)
        });
        assert_eq!(bench.items("drawingSummaryUnresolved"), ["TEST Fence"]);
        assert!(
            !bench.enabled("drawingSummaryShowMissing"),
            "nothing is chosen yet"
        );
        bench.fill("drawingSummaryUnresolved", "TEST Fence");
        assert!(bench.enabled("drawingSummaryShowMissing"));
        bench.press("drawingSummaryShowMissing");
        assert_eq!(*shown.borrow(), ["TEST Fence"]);
        // A double-click on the name (the C++'s itemActivated).
        bench.dialog.show_missing(0);
        assert_eq!(*shown.borrow(), ["TEST Fence", "TEST Fence"]);
    }

    #[test]
    fn copy_as_json_runs_status_json_and_copies_its_reply() {
        let host = fixed_host(VerbOutcome::new(true, "{\n  \"entities\": 0\n}", ""));
        let mut bench = Bench::new(host, |document| {
            DrawingSummaryDialog::new(document, running())
        });
        bench.press("drawingSummaryCopyJson");
        assert_eq!(bench.lines(), ["STATUS JSON"]);
        assert_eq!(
            bench.host.clipboard().text().as_deref(),
            Some("{\n  \"entities\": 0\n}")
        );
        assert_eq!(
            bench.text("drawingSummaryStatus"),
            "Copied the drawing's state as JSON (STATUS JSON)."
        );
    }

    #[test]
    fn a_refused_status_copies_nothing_and_says_why() {
        let host = fixed_host(VerbOutcome::new(false, "", "no"));
        let mut bench = Bench::new(host, |document| {
            DrawingSummaryDialog::new(document, running())
        });
        bench
            .host
            .clipboard()
            .set_text("before")
            .expect("the clipboard takes text");
        assert!(bench.dialog.copy_json(&mut bench.host).is_empty());
        assert_eq!(bench.host.clipboard().text().as_deref(), Some("before"));
        bench.draw();
        assert_eq!(
            bench.text("drawingSummaryStatus"),
            "STATUS JSON was refused: no"
        );
    }

    #[test]
    fn it_offers_no_button_for_the_removed_load_customisation_item() {
        // The summary had a Load Customisation button that triggered the Format menu's item of
        // that name. The item is gone - a customisation file is loaded in File > Settings or by
        // the CUSTOMISE line - and a button left behind would trigger a name that no longer
        // exists, and fail without a word. What the dialog does is say where to go: the heading
        // over the customisation report names the menu path, which the window has (File >
        // Settings, fileSettings).
        let bench = summary(RecordingHost::new());
        assert!(!bench.has("drawingSummaryLoad"));
        let heading = bench.text("drawingSummaryCustomisationLabel");
        assert!(heading.contains("File > Settings"), "{heading}");
        assert!(!heading.contains("Load Customisation"));
        let frame = bench.harness.snapshot();
        for control in frame.in_target("drawingSummaryDialog") {
            assert!(
                !control.text.contains("Load Customisation"),
                "{}",
                control.text
            );
        }
    }

    #[test]
    fn a_document_that_goes_first_leaves_the_dialog_safe() {
        let mut document = Document::new(None);
        let dialog = DrawingSummaryDialog::new(&document, DrawingSummaryContext::default());
        katana_cad::CommandInterpreter::new()
            .run(&mut document, "POINT 1,1")
            .expect("the point is drawn");
        drop(document); // a refresh is queued, and must deliver nothing
        // A stranger with a point of its own stands where the Document was: read, it would show.
        let mut host = RecordingHost::new();
        run(&mut host, "POINT 5,5");
        let mut bench = Bench::showing(host, dialog);
        bench.dialog.refresh(bench.host.document());
        bench.draw();
        assert_eq!(
            bench.text("drawingSummaryDrawing"),
            "0 entities, 1 layer, 0 alignments, 0 sheets"
        );
    }
}
