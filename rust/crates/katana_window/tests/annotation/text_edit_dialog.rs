//! The twins of tests/qt_widgets/annotation/test_text_edit_dialog.cpp: Annotate > Edit Text...,
//! driven by object name and checked by the TEXTEDIT line it runs - only the fields the form
//! changed, one line, one undo step. The runner is the interpreter itself, so a line is checked by
//! what the verb makes of it.
//!
//! The suite's fifth case, TheAnnotateMenuOpensItByName, installs the annotation workbench and is
//! V3's.

use katana_entity::{EntityId, Geometry, TextGeometry, TextJustify};
use katana_geometry::Point2;
use katana_math::numerics::DEG_TO_RAD;
use katana_window::annotation::TextEditDialog;
use katana_window::host::RecordingHost;

use crate::support::{Bench, interpreter_host, run};

fn select(host: &mut RecordingHost, ids: Vec<EntityId>) {
    host.document_mut().selection_mut().set(ids);
    host.document().notify_selection_changed();
}

fn text_of(host: &RecordingHost, id: EntityId) -> TextGeometry {
    match host
        .document()
        .model()
        .entities
        .find(id)
        .map(|entity| &entity.geometry)
    {
        Some(Geometry::Text(text)) => text.clone(),
        other => panic!("{id} is no text: {other:?}"),
    }
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
fn double_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return false;
    }
    a.to_bits().abs_diff(b.to_bits()) <= 4
}

mod text_edit_dialog {
    use super::*;

    #[test]
    fn with_no_text_selected_it_asks_for_one_and_runs_nothing() {
        let mut host = interpreter_host();
        run(&mut host, "TEXT 0,0 2.5 old");
        run(&mut host, "LINE 0,0 10,0");
        let mut bench = Bench::new(host, TextEditDialog::new);
        assert!(bench.harness.target_record("textEditDialog").is_some());
        assert_eq!(bench.dialog.text_id(), 0);
        assert_eq!(
            bench.dialog.status(),
            "Select one text in the drawing to edit it."
        );
        assert!(!bench.enabled("textEditApply"));
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(bench.lines().is_empty());

        select(&mut bench.host, vec![2]);
        bench.draw();
        assert!(
            bench.dialog.status().contains("not a text"),
            "{}",
            bench.dialog.status()
        );
        select(&mut bench.host, vec![1, 2]);
        bench.draw();
        assert!(
            bench.dialog.status().contains("2 entities"),
            "{}",
            bench.dialog.status()
        );
        select(&mut bench.host, vec![1]);
        bench.draw();
        assert_eq!(bench.dialog.text_id(), 1, "it follows the selection");
        assert_eq!(bench.dialog.status(), "Text 1");
        assert_eq!(bench.value("textEditText"), "old");
    }

    #[test]
    fn every_field_by_name_is_one_line_of_the_changed_keys_and_one_step() {
        let mut host = interpreter_host();
        run(&mut host, "TEXTSTYLE NEW Notes paper=3.5");
        run(&mut host, "TEXT 10,20 2.5 old");
        select(&mut host, vec![1]);
        let mut bench = Bench::new(host, TextEditDialog::new);
        assert_eq!(bench.dialog.text_id(), 1);
        assert_eq!(bench.value("textEditPosition"), "10,20");
        assert_eq!(bench.value("textEditHeight"), "2.5");
        assert_eq!(bench.value("textEditStyle"), "(none)");
        assert_eq!(bench.value("textEditJustify"), "BL");

        bench.fill("textEditText", "PIT 12\\nIL 10.50");
        bench.fill("textEditStyle", "Notes");
        bench.fill("textEditJustify", "MC");
        bench.fill("textEditRotation", "30");
        bench.fill("textEditPosition", "5, 6");

        let before = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.lines().len(), 1);
        assert_eq!(
            bench.lines()[0],
            "TEXTEDIT 1 text=\"PIT 12\\nIL 10.50\" style=Notes justify=MC rotation=30 at=5,6",
            "only the changed keys: the height and paper were left alone"
        );
        assert_eq!(bench.steps(), before + 1);
        let text = text_of(&bench.host, 1);
        assert_eq!(text.text, "PIT 12\nIL 10.50");
        assert_eq!(text.style, "Notes");
        assert_eq!(text.justify, TextJustify::MiddleCentre);
        assert!(
            double_eq(text.rotation, 30.0 * DEG_TO_RAD),
            "{}",
            text.rotation
        );
        assert_eq!(text.position, Point2::new(5.0, 6.0));
        // Paper-sized now: 3.5 mm at the default 1:1000 is 3.5 m.
        assert!(double_eq(text.height, 3.5), "{}", text.height);
        bench.draw();
        assert_eq!(
            bench.value("textEditHeight"),
            "3.5",
            "the form shows what is stored now"
        );

        assert!(bench.dialog.apply(&mut bench.host), "an unchanged form");
        assert_eq!(bench.lines().len(), 1, "runs nothing");

        // The style back to none, and a paper height of its own.
        bench.fill("textEditStyle", "(none)");
        bench.fill("textEditPaper", "5");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("TEXTEDIT 1 style=\"\" paper=5")
        );
        assert_eq!(text_of(&bench.host, 1).style, "");
        assert_eq!(text_of(&bench.host, 1).paper_height, 5.0);

        bench.document_mut().undo().expect("the undo is done");
        bench.document_mut().undo().expect("the undo is done");
        assert_eq!(
            text_of(&bench.host, 1).text,
            "old",
            "each Apply was one step"
        );
        bench.draw();
        assert_eq!(bench.value("textEditText"), "old");
    }

    #[test]
    fn a_value_no_line_can_carry_is_refused_before_anything_runs() {
        let mut host = interpreter_host();
        run(&mut host, "TEXT 0,0 2.5 old");
        select(&mut host, vec![1]);
        let mut bench = Bench::new(host, TextEditDialog::new);
        bench.fill("textEditText", "5\" pipe");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("double quote"),
            "{}",
            bench.dialog.problem()
        );
        bench.dialog.revert();
        bench.draw();
        bench.fill("textEditHeight", "0");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(bench.dialog.problem().contains("greater than zero"));
        bench.fill("textEditHeight", "2.5");
        bench.fill("textEditPosition", "east");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(bench.dialog.problem().contains("E,N"));
        bench.fill("textEditText", "  ");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(bench.lines().is_empty());
        assert_eq!(text_of(&bench.host, 1).text, "old");
    }

    #[test]
    fn edits_are_kept_through_changes_elsewhere_and_replaced_when_the_text_changes() {
        let mut host = interpreter_host();
        run(&mut host, "TEXT 0,0 2.5 old");
        select(&mut host, vec![1]);
        let mut bench = Bench::new(host, TextEditDialog::new);
        bench.fill("textEditText", "typing");
        bench.run("LINE 0,0 10,0");
        bench.draw();
        assert_eq!(
            bench.value("textEditText"),
            "typing",
            "another entity changed"
        );
        bench.run("TEXTEDIT 1 text=typed");
        bench.draw();
        assert_eq!(bench.value("textEditText"), "typed", "this text changed");
        bench.fill("textEditText", "again");
        bench.dialog.revert();
        bench.draw();
        assert_eq!(bench.value("textEditText"), "typed");
    }
}
