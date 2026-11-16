//! The twins of tests/qt_widgets/annotation/test_label_edit_dialog.cpp: Annotate > Edit Label...
//! and Label Layout Report... - the lines they write, worked out by hand, and the dialogs driven
//! by object name with a runner that runs each line through the command interpreter on the same
//! document - as the window's one executor does - so what a click does is checked on the drawing,
//! one undo step at a time.

use katana_entity::{EntityId, Geometry, LabelGeometry};
use katana_geometry::Point2;
use katana_window::annotation::label_edit_dialog::{
    LabelEditDialog, LabelEditForm, label_set_line,
};
use katana_window::annotation::label_layout_report::{
    LabelLayoutReportDialog, label_layout_line, parse_label_layout_reply, select_line,
};
use katana_window::host::RecordingHost;

use crate::support::{Bench, interpreter_host, run};

/// Label `id` of the drawing.
fn label_in(host: &RecordingHost, id: EntityId) -> LabelGeometry {
    match host
        .document()
        .model()
        .entities
        .find(id)
        .map(|entity| &entity.geometry)
    {
        Some(Geometry::Label(label)) => label.clone(),
        other => panic!("{id} is no label: {other:?}"),
    }
}

/// A lot (1) labelled by its area (2) on layer 0, and a layer "labels".
fn lot_with_label(host: &mut RecordingHost) {
    run(host, "LABELSTYLE DEFAULTS");
    run(host, "LAYER NEW labels");
    run(host, "PLINE 0,0 30,0 30,40 0,40 CLOSE");
    run(host, "LABEL 1 style=\"Lot Area\"");
}

fn unpinned_lot() -> LabelEditForm {
    LabelEditForm {
        id: 2,
        style: String::from("Lot Area"),
        easting: String::from("15"),
        northing: String::from("20"),
        layer: String::from("0"),
        ..LabelEditForm::default()
    }
}

fn line_of(form: &LabelEditForm, current: &LabelEditForm) -> String {
    label_set_line(form, current).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn edit_label() -> Bench<LabelEditDialog> {
    let mut host = interpreter_host();
    lot_with_label(&mut host);
    Bench::new(host, LabelEditDialog::new)
}

// ---- the LABEL SET line -------------------------------------------------------------------------

mod label_edit_line {
    use super::*;

    #[test]
    fn only_what_differs_from_the_label_is_named() {
        let current = unpinned_lot();
        assert_eq!(line_of(&current, &current), "", "nothing to change");

        let mut form = current.clone();
        form.style = String::from("Bearing Distance");
        assert_eq!(
            line_of(&form, &current),
            "LABEL SET 2 style=\"Bearing Distance\""
        );

        let mut form = current.clone();
        form.pinned = true;
        form.easting = String::from(" 12.5 ");
        form.northing = String::from("-3");
        assert_eq!(line_of(&form, &current), "LABEL SET 2 at=12.5,-3");

        let mut form = current.clone();
        form.r#override = true;
        form.text = String::from("LOT 7\nDP 1234");
        form.layer = String::from("labels");
        assert_eq!(
            line_of(&form, &current),
            "LABEL SET 2 text=\"LOT 7\\nDP 1234\" layer=labels"
        );
    }

    #[test]
    fn unticking_takes_back_the_own_text_and_the_pinned_place() {
        let mut current = unpinned_lot();
        current.r#override = true;
        current.text = String::from("LOT 7");
        current.pinned = true;
        current.easting = String::from("5");
        current.northing = String::from("6");
        let mut form = current.clone();
        form.r#override = false;
        form.pinned = false;
        assert_eq!(line_of(&form, &current), "LABEL SET 2 text=none at=none");
        // The same place written another way is the same place.
        let mut form = current.clone();
        form.easting = String::from("5.000");
        assert_eq!(line_of(&form, &current), "");
    }

    #[test]
    fn fields_that_make_no_line_are_refused_naming_the_field() {
        let current = unpinned_lot();
        let refused = |form: &LabelEditForm, part: &str| {
            let error = label_set_line(form, &current).expect_err(part);
            assert!(error.message.contains(part), "{}", error.message);
        };
        let mut form = current.clone();
        form.id = 0;
        refused(&form, "no label is chosen");
        let mut form = current.clone();
        form.style = String::from(" ");
        refused(&form, "Style:");
        let mut form = current.clone();
        form.layer.clear();
        refused(&form, "Layer:");
        let mut form = current.clone();
        form.r#override = true;
        refused(&form, "Own text:");
        form.text = String::from("None");
        refused(&form, "the word none");
        form.text = String::from("6\" pipe");
        refused(&form, "a double quote");
        // A backslash and an n typed as they are: LABEL SET would read them back as a line break
        // the text does not have.
        form.text = String::from("C:\\new");
        refused(&form, "Own text: a backslash before an n");
        let mut form = current.clone();
        form.layer = String::from("kerb\nline");
        refused(&form, "Layer: a line break");
        let mut form = current.clone();
        form.pinned = true;
        form.easting = String::from("12,5");
        refused(&form, "Easting: '12,5' is not a number");
        form.easting = String::from("1");
        form.northing = String::from("north");
        refused(&form, "Northing:");
    }
}

// ---- the Edit Label dialog ----------------------------------------------------------------------

mod label_edit_dialog {
    use super::*;

    #[test]
    fn its_fields_start_at_one_edge_and_end_at_the_bodys_whichever_box_labels_them() {
        // The look of 2026-11-16: one form, its labels in one column - the two boxes as the
        // labels of the fields they switch - and every field from one edge to the body's. In the
        // window's theme (theme::style::apply); the harness's own style is egui's. Within half a
        // point: egui rounds a widget's rectangle to the pixel grid (the offscreen screen's one
        // pixel a point).
        const HALF_A_POINT: f32 = 0.5;
        let mut bench = edit_label();
        katana_window::theme::style::apply(bench.harness.context());
        bench.document_mut().selection_mut().set(vec![2]);
        assert!(bench.dialog.load_selection(bench.host.document()));
        bench.draw();
        let dialog = bench
            .harness
            .target_record("labelEditDialog")
            .expect("the dialog is drawn")
            .rect;
        // The body's right edge: the frame's pixel of border, its padding and BODY_WIDTH from the
        // dialog's left - not from its right, which a control running past the body moves.
        let body_right = dialog.left()
            + 1.0
            + katana_window::shell::dialog::PADDING
            + katana_window::shell::dialog::BODY_WIDTH;
        let rects: Vec<(&str, egui::Rect)> = [
            "labelEditStyle",
            "labelEditText",
            "labelEditEasting",
            "labelEditNorthing",
            "labelEditLayer",
            "labelEditCommand",
        ]
        .into_iter()
        .map(|name| (name, bench.control(name).rect))
        .collect();
        let said = format!("the dialog is {dialog:?}, its body ends at {body_right}; {rects:?}");
        let start = rects[0].1.left();
        let mut wrong = Vec::new();
        for (name, rect) in &rects {
            if *name != "labelEditNorthing" && (rect.left() - start).abs() > HALF_A_POINT {
                wrong.push(format!("{name} starts at {}", rect.left()));
            }
            // The layer is an editable choice, one control with its list arrow inside its right
            // end, ending at the body's edge (drive::controls::editable_choice_frame): when the
            // arrow was a ComboBox of its own beside the field, reserved a control's height, it
            // ran 14 past the row and pushed the body out for the command under it. The easting
            // ends at half the row, where the northing starts.
            let ends_here = *name != "labelEditEasting";
            if ends_here && (rect.right() - body_right).abs() > HALF_A_POINT {
                wrong.push(format!("{name} ends at {}", rect.right()));
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}: {said}");
    }

    #[test]
    fn with_no_label_selected_it_says_so_and_offers_nothing_to_run() {
        let mut bench = edit_label();
        assert!(!bench.dialog.load_selection(bench.host.document()));
        assert_eq!(
            bench.dialog.status(),
            "Select one label, then choose Edit Label."
        );
        bench.draw();
        assert!(!bench.enabled("labelEditOk"));
        assert!(!bench.enabled("labelEditApply"));
        bench.document_mut().selection_mut().set(vec![1]);
        assert!(!bench.dialog.load_selection(bench.host.document()));
        assert!(
            bench.dialog.status().contains("not a label"),
            "{}",
            bench.dialog.status()
        );
        assert!(bench.lines().is_empty());
    }

    #[test]
    fn apply_runs_the_line_it_shows_as_one_undo_step() {
        let mut bench = edit_label();
        bench.document_mut().selection_mut().set(vec![2]);
        assert!(bench.dialog.load_selection(bench.host.document()));
        bench.draw();
        assert_eq!(
            bench.report("labelEditTarget"),
            "Label 2 of entity 1 (Polyline)"
        );
        assert_eq!(bench.value("labelEditStyle"), "Lot Area");
        assert!(!bench.checked("labelEditPinned"));
        assert!(!bench.enabled("labelEditEasting"));

        bench.fill("labelEditPinned", "on");
        assert!(bench.enabled("labelEditEasting"));
        bench.fill("labelEditEasting", "12");
        bench.fill("labelEditNorthing", "34");
        bench.fill("labelEditOverride", "on");
        bench.fill("labelEditText", "LOT 7");
        bench.fill("labelEditLayer", "labels");
        let line = "LABEL SET 2 text=\"LOT 7\" at=12,34 layer=labels";
        assert_eq!(bench.value("labelEditCommand"), line);

        let steps = bench.steps();
        bench.press("labelEditApply");
        assert_eq!(bench.lines(), [line]);
        assert_eq!(bench.dialog.status(), "updated label id=2");
        assert_eq!(bench.steps(), steps + 1);
        let label = label_in(&bench.host, 2);
        assert_eq!(label.text_override, "LOT 7");
        assert_eq!(label.position, Some(Point2::new(12.0, 34.0)));
        assert_eq!(
            bench
                .document()
                .model()
                .entities
                .find(2)
                .map(|entity| entity.layer.as_str()),
            Some("labels")
        );
        // Shown afresh: the fields now match the label, so nothing is left to run.
        assert_eq!(bench.dialog.line(), "");
        bench.press("labelEditApply");
        assert_eq!(bench.lines().len(), 1, "an unchanged label runs nothing");
        assert_eq!(bench.dialog.status(), "Nothing to change.");

        // Unticked again: back to the placer and the style's words.
        bench.fill("labelEditPinned", "off");
        bench.fill("labelEditOverride", "off");
        assert!(bench.dialog.apply(&mut bench.host));
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("LABEL SET 2 text=none at=none")
        );
        assert!(label_in(&bench.host, 2).position.is_none());
        assert!(label_in(&bench.host, 2).text_override.is_empty());
    }

    #[test]
    fn a_refused_line_leaves_the_label_and_says_why() {
        let mut bench = edit_label();
        assert!(bench.dialog.load(bench.host.document(), 2));
        bench.draw();
        bench.fill("labelEditLayer", "nowhere");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert_eq!(bench.lines(), ["LABEL SET 2 layer=nowhere"]);
        assert!(!bench.dialog.status().is_empty());
        assert_eq!(
            bench
                .document()
                .model()
                .entities
                .find(2)
                .map(|entity| entity.layer.as_str()),
            Some("0")
        );
        // A field that makes no line is said before anything runs.
        bench.fill("labelEditPinned", "on");
        bench.fill("labelEditEasting", "east");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert_eq!(bench.lines().len(), 1);
        assert_eq!(bench.dialog.status(), "Easting: 'east' is not a number");
    }

    // Non-modal, the dialog follows the label: undone behind it, the form shows the label as it
    // now is, so Apply compares with what the model holds. It once kept the pinned place it
    // loaded, and an unchanged form then ran nothing though the label had moved back.
    #[test]
    fn an_undo_while_it_is_open_shows_the_label_as_it_now_is() {
        let mut bench = edit_label();
        assert!(bench.dialog.load(bench.host.document(), 2));
        bench.draw();
        bench.fill("labelEditPinned", "on");
        bench.fill("labelEditEasting", "12");
        bench.fill("labelEditNorthing", "34");
        assert!(bench.dialog.apply(&mut bench.host));
        assert!(label_in(&bench.host, 2).position.is_some());

        bench.document_mut().undo().expect("the undo is done");
        bench.draw();
        assert!(!bench.checked("labelEditPinned"));
        assert_eq!(bench.dialog.line(), "");
        // Pinned again from the form: the line names the place, which the label no longer has.
        bench.fill("labelEditPinned", "on");
        bench.fill("labelEditEasting", "12");
        bench.fill("labelEditNorthing", "34");
        assert_eq!(bench.dialog.line(), "LABEL SET 2 at=12,34");

        // Another entity's change leaves what is typed alone.
        bench.run("POINT 100,100");
        bench.draw();
        assert_eq!(bench.dialog.line(), "LABEL SET 2 at=12,34");
    }

    // A new or opened drawing empties it: the id it holds is the last drawing's, and an opened
    // project may well have a label of its own under it. Left open across an OPEN, it once wrote
    // its edits onto that label.
    #[test]
    fn a_new_drawing_empties_it_rather_than_editing_another_drawings_label() {
        let mut bench = edit_label();
        assert!(bench.dialog.load(bench.host.document(), 2));
        bench.draw();
        bench.fill("labelEditOverride", "on");
        bench.fill("labelEditText", "Lot 7 DP 1234");
        bench.document_mut().new_document();
        // Ids are never reused, so the new drawing's lot and label are numbered on.
        bench.run("LABELSTYLE DEFAULTS");
        let lot = bench.run("PLINE 100,100 130,100 130,140 100,140 CLOSE");
        bench.run(&format!("LABEL {lot} style=\"Lot Area\""));

        bench.draw();
        assert!(!bench.enabled("labelEditApply"));
        assert_eq!(bench.report("labelEditTarget"), "-");
        assert!(
            bench
                .dialog
                .status()
                .starts_with("The drawing was replaced"),
            "{}",
            bench.dialog.status()
        );
        assert_eq!(bench.dialog.line(), "");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(bench.lines().is_empty());
        bench.document().model().entities.for_each(|entity| {
            if let Geometry::Label(label) = &entity.geometry {
                assert!(label.text_override.is_empty(), "label {}", entity.id);
            }
        });
    }

    // Erased behind it, the label leaves the form empty.
    #[test]
    fn an_erased_label_leaves_it_empty() {
        let mut bench = edit_label();
        assert!(bench.dialog.load(bench.host.document(), 2));
        bench.run("SELECT 2");
        bench.run("ERASE");
        bench.draw();
        assert!(!bench.enabled("labelEditApply"));
        assert_eq!(
            bench.dialog.status(),
            "Label 2 is no longer in the drawing; select one label, then choose Edit Label."
        );
    }
}

// ---- the Label Layout Report --------------------------------------------------------------------

mod label_layout_report {
    use super::*;

    #[test]
    fn the_replys_counts_and_suppressed_labels_are_read() {
        let summary = parse_label_layout_reply(
            "scale=500 considered=5 placed=1 displaced=0 suppressed=4 orphaned=1\n\
             label=2 piece=0 x=1 y=2 candidate=0 displaced=no text=\"P 1\"\n\
             label=4 piece=0 suppressed=yes\n\
             label=6 piece=0 suppressed=yes\n\
             label=6 piece=1 suppressed=yes\n",
        );
        assert!(summary.read);
        assert_eq!(summary.scale, 500.0);
        assert_eq!(summary.considered, 5);
        assert_eq!(summary.placed, 1);
        assert_eq!(summary.suppressed, 4);
        assert_eq!(summary.orphaned, 1);
        assert_eq!(summary.suppressed_labels, [4, 6], "each label once");
        assert_eq!(select_line(&summary.suppressed_labels), "SELECT 4 6");
        assert_eq!(select_line(&[]), "");
        assert!(!parse_label_layout_reply("created labels=1 ids=5").read);
        assert_eq!(label_layout_line(false), "LABEL LAYOUT collisions=off");
    }

    #[test]
    fn run_shows_the_counts_and_selects_the_labels_that_found_no_room() {
        // Five numbered points at one place, labelled in a style that may not move its labels:
        // the first is placed, the other four find no room (the placer's own
        // WithNoRoomALabelIsSuppressedAndCounted).
        let mut host = interpreter_host();
        run(
            &mut host,
            "LABELSTYLE NEW pt kind=point text={point} displace=off",
        );
        for i in 1..=5 {
            let id = run(&mut host, "POINT 0,0");
            run(&mut host, &format!("SELECT {id}"));
            run(&mut host, &format!("PROP SET point P{i}"));
        }
        run(&mut host, "LABEL 1 2 3 4 5 style=pt"); // labels 6 to 10
        let mut bench = Bench::new(host, |_| LabelLayoutReportDialog::new());
        assert!(!bench.enabled("labelLayoutSelectSuppressed"));
        bench.press("labelLayoutRun");
        assert_eq!(bench.lines(), ["LABEL LAYOUT"]);
        assert_eq!(bench.report("labelLayoutPlaced"), "1");
        assert_eq!(bench.report("labelLayoutSuppressed"), "4");
        assert_eq!(bench.report("labelLayoutOrphaned"), "0");
        assert_eq!(bench.dialog.summary().suppressed_labels, [7, 8, 9, 10]);
        assert!(bench.enabled("labelLayoutSelectSuppressed"));
        bench.press("labelLayoutSelectSuppressed");
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("SELECT 7 8 9 10")
        );
        assert_eq!(bench.document().selection().ids(), [7, 8, 9, 10]);

        // Unticked, every label is put at its own place: none is suppressed.
        bench.fill("labelLayoutCollisions", "off");
        assert_eq!(
            bench.value("labelLayoutCommand"),
            "LABEL LAYOUT collisions=off"
        );
        assert!(bench.dialog.run(&mut bench.host));
        bench.draw();
        assert_eq!(bench.report("labelLayoutPlaced"), "5");
        assert_eq!(bench.report("labelLayoutSuppressed"), "0");
        assert!(!bench.enabled("labelLayoutSelectSuppressed"));
    }

    #[test]
    fn with_no_runner_it_says_so_rather_than_showing_nothing() {
        let mut bench = Bench::new(RecordingHost::new(), |_| {
            LabelLayoutReportDialog::new().without_command_line()
        });
        assert!(!bench.dialog.run(&mut bench.host));
        assert_eq!(
            bench.dialog.status(),
            "There is no command line to run it on."
        );
        bench.draw();
        assert_eq!(bench.report("labelLayoutPlaced"), "-");
        assert!(bench.lines().is_empty());
    }
}

// ---- the look (the second look audit of 2026-11-16) ---------------------------------------------

mod label_dialogs_look {
    use super::*;

    /// egui rounds a widget's rectangle to the pixel grid (the offscreen screen's one pixel a
    /// point): two places laid out as one are within half a point of each other.
    const HALF_A_POINT: f32 = 0.5;

    #[test]
    fn the_layout_reports_counts_stand_on_their_labels_lines() {
        // T7: each count was centred twice - by its row's wrapping layout and by text_value's gap
        // above - and stood 5 points under its label. The rows are a form's: a control high and
        // FORM_SPACING apart, a label centred in its row; so the last count's middle is the
        // Command field's top less the spacing and half a control.
        let mut bench = Bench::new(interpreter_host(), |_| LabelLayoutReportDialog::new());
        katana_window::theme::style::apply(bench.harness.context());
        bench.draw();
        let control = bench
            .harness
            .context()
            .global_style()
            .spacing
            .interact_size
            .y;
        let spacing = katana_window::shell::dialog::FORM_SPACING.y;
        let command = bench.control("labelLayoutCommand").rect;
        let orphaned = bench.control("labelLayoutOrphaned").rect;
        let row_middle = command.top() - spacing - control / 2.0;
        assert!(
            (orphaned.center().y - row_middle).abs() <= HALF_A_POINT,
            "the count's middle is at {} and its row's at {row_middle}",
            orphaned.center().y
        );
    }

    #[test]
    fn edit_labels_footer_is_apply_then_close_then_ok_the_primary() {
        // T5: the dismissing button read Cancel where every other footer's reads Close; its name
        // and what it does are kept.
        let bench = edit_label();
        assert_eq!(bench.report("labelEditCancel"), "Close");
        let rect = |name: &str| bench.control(name).rect;
        let (apply, close, ok) = (
            rect("labelEditApply"),
            rect("labelEditCancel"),
            rect("labelEditOk"),
        );
        assert!(apply.right() <= close.left(), "Apply is before Close");
        assert!(close.right() <= ok.left(), "OK is last");
    }
}
