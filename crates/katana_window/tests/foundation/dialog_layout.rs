//! The dialog frame's look (shell::dialog; the design of 2026-11-16, docs/desktop.md "The rules a
//! dialog or panel follows"): a footer's buttons end at the dialog's right edge less its padding,
//! the primary last and every button one least width; every labelled row of a body starts its
//! field at one edge; a body is no wider than the width it is laid out at, so a note wraps in it.
//!
//! Nothing here is a pixel of ink: rectangles are compared with each other and with the frame's
//! own constants. Within HALF_A_POINT where egui rounds a widget to the pixel grid.

use egui::Ui;
use katana_window::drive::{self, LineOptions};
use katana_window::shell::DialogFrame;
use katana_window::shell::dialog::{
    BODY_WIDTH, FOOTER_BUTTON_WIDTH, FORM_SPACING, LABEL_WIDTH, PADDING, button_row, labelled, note,
};

use crate::window_support::offscreen_harness;

mod dialog_layout {
    use super::*;

    /// egui rounds a widget's rectangle to the pixel grid (at the offscreen screen's one pixel a
    /// point), so two edges laid out at one place are within half a point of each other.
    const HALF_A_POINT: f32 = 0.5;

    #[test]
    fn a_footers_buttons_end_at_the_right_edge_with_the_primary_last() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("buttonsDialog", "Buttons");
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show(
                    &ctx,
                    &mut (),
                    |ui, _| {
                        drive::label(ui, "bodyText", "A body");
                    },
                    |ui, _| {
                        button_row(
                            ui,
                            |ui| {
                                drive::button(ui, "useSelection", "Use Selection");
                            },
                            |ui| {
                                drive::button(ui, "close", "Close");
                                drive::primary(ui, "run", "Run");
                            },
                        );
                    },
                );
            })
            .expect("the dialog is drawn");
        let dialog = harness
            .target_record("buttonsDialog")
            .expect("the dialog is a target");
        let control = |name: &str| harness.control(name).expect("each button is drawn");
        let (left, close, run) = (control("useSelection"), control("close"), control("run"));
        // The frame's border is a point wide; the footer's padding is inside it.
        let right_edge = dialog.rect.right() - 1.0 - PADDING;
        let left_edge = dialog.rect.left() + 1.0 + PADDING;
        assert!(
            (run.rect.right() - right_edge).abs() <= HALF_A_POINT,
            "the primary ends at {} where the footer ends at {right_edge}",
            run.rect.right()
        );
        assert!(close.rect.right() <= run.rect.left(), "the primary is last");
        assert!(
            (left.rect.left() - left_edge).abs() <= HALF_A_POINT,
            "the leading button starts the row"
        );
        assert!(
            left.rect.right() < close.rect.left(),
            "a gap between the two ends"
        );
        for button in [&left, &close, &run] {
            assert!(
                button.rect.width() >= FOOTER_BUTTON_WIDTH - HALF_A_POINT,
                "{} is {} wide",
                button.name,
                button.rect.width()
            );
            assert!(button.visible && button.enabled, "{}", button.name);
        }
        // Close and Run are one width: the least, as neither word needs more.
        assert!((close.rect.width() - run.rect.width()).abs() <= HALF_A_POINT);
    }

    #[test]
    fn a_rows_buttons_are_pressed_by_name_wherever_they_are_laid_out() {
        // The row measures its right end and lays itself out again: a press still reaches each
        // button, once.
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("pressDialog", "Press");
        let mut pressed: Vec<&'static str> = Vec::new();
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            frame.show(
                &ctx,
                &mut pressed,
                |ui, _| {
                    drive::label(ui, "bodyText", "A body");
                },
                |ui, pressed| {
                    let (left, (close, run)) = button_row(
                        ui,
                        |ui| drive::button(ui, "left", "Left").clicked(),
                        |ui| {
                            (
                                drive::button(ui, "close", "Close").clicked(),
                                drive::primary(ui, "run", "Run").clicked(),
                            )
                        },
                    );
                    for (clicked, name) in [(left, "left"), (close, "close"), (run, "run")] {
                        if clicked {
                            pressed.push(name);
                        }
                    }
                },
            );
        };
        harness.show(&mut draw).expect("the dialog is drawn");
        for name in ["run", "left", "close"] {
            harness
                .target("pressDialog")
                .press(name)
                .run(&mut draw)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
        }
        assert_eq!(pressed, ["run", "left", "close"]);
    }

    #[test]
    fn every_labelled_row_of_a_body_starts_its_field_at_one_edge() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("formDialog", "Form");
        let mut values = [String::new(), String::new(), String::new()];
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show_body(&ctx, &mut values, |ui, values| {
                    let [short, long, grouped] = values;
                    labelled(ui, "Ids:", |ui| drive::line(ui, "short", short));
                    // A check box between two rows takes no column of its own.
                    let mut on = false;
                    drive::check(ui, "between", "A check box", &mut on);
                    labelled(ui, "A longer label:", |ui| drive::line(ui, "long", long));
                    // A row inside a group shares the body's column.
                    ui.group(|ui| {
                        labelled(ui, "In a group:", |ui| drive::line(ui, "grouped", grouped));
                    });
                });
            })
            .expect("the dialog is drawn");
        let field = |name: &str| harness.control(name).expect("each field is drawn").rect;
        let (short, long, grouped) = (field("short"), field("long"), field("grouped"));
        assert!(
            (short.left() - long.left()).abs() <= HALF_A_POINT,
            "a short label's field starts at {} and a long one's at {}",
            short.left(),
            long.left()
        );
        assert!(
            (short.left() - grouped.left()).abs() <= HALF_A_POINT,
            "the group's field starts at {}, the body's at {}",
            grouped.left(),
            short.left()
        );
        // The rows' fields fill the body to its right edge.
        assert!((short.right() - long.right()).abs() <= HALF_A_POINT);
        // The column is the widest label's, not the widest a column may be: "A longer label:"
        // is far narrower than LABEL_WIDTH at the Standard text size.
        let dialog = harness
            .target_record("formDialog")
            .expect("the dialog is a target");
        let body_left = dialog.rect.left() + 1.0 + PADDING;
        assert!(
            short.left() - body_left < LABEL_WIDTH,
            "the column is {} wide",
            short.left() - body_left
        );
    }

    #[test]
    fn a_label_wider_than_the_column_wraps_and_the_fields_keep_their_edge() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("wrapDialog", "Wrap");
        let mut values = [String::new(), String::new()];
        let long = "A label far longer than any label column should ever be allowed to grow";
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show_body(&ctx, &mut values, |ui, values| {
                    let [first, second] = values;
                    labelled(ui, long, |ui| drive::line(ui, "first", first));
                    labelled(ui, "Short:", |ui| drive::line(ui, "second", second));
                });
            })
            .expect("the dialog is drawn");
        let dialog = harness
            .target_record("wrapDialog")
            .expect("the dialog is a target");
        let first = harness.control("first").expect("drawn").rect;
        let second = harness.control("second").expect("drawn").rect;
        let body_left = dialog.rect.left() + 1.0 + PADDING;
        assert!((first.left() - second.left()).abs() <= HALF_A_POINT);
        // The column is held to LABEL_WIDTH and the gap after it.
        assert!(first.left() - body_left <= LABEL_WIDTH + FORM_SPACING.x + HALF_A_POINT);
        // The long label wrapped: its row is taller than a one-line row.
        assert!(second.top() - first.top() > first.height() + HALF_A_POINT);
    }

    #[test]
    fn a_body_is_no_wider_than_it_is_laid_out_at_and_a_note_wraps_in_it() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("noteDialog", "Note");
        // Three or four lines of words at BODY_WIDTH: wider than the body, short enough not to scroll.
        let words = "word ".repeat(60);
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show_body(&ctx, &mut (), |ui, _| {
                    note(ui, &words);
                    let mut text = String::new();
                    drive::line_with(
                        ui,
                        "wide",
                        &mut text,
                        LineOptions {
                            width: Some(f32::INFINITY),
                            ..LineOptions::default()
                        },
                    );
                });
            })
            .expect("the dialog is drawn");
        let dialog = harness
            .target_record("noteDialog")
            .expect("the dialog is a target");
        let wide = harness.control("wide").expect("drawn").rect;
        // The body is BODY_WIDTH wide: the note wrapped in it, and a field that fills fills it.
        assert!(
            (wide.width() - BODY_WIDTH).abs() <= HALF_A_POINT,
            "the field is {} wide",
            wide.width()
        );
        assert!(dialog.rect.width() <= BODY_WIDTH + 2.0 * (PADDING + 1.0) + HALF_A_POINT);
    }
}
