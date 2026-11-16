//! The dialog frame's look (shell::dialog; the design of 2026-11-16, docs/desktop.md "The rules a
//! dialog or panel follows"): a footer's buttons end at the dialog's right edge less its padding,
//! the primary last and every button one least width; every labelled row of a body starts its
//! field at one edge, a field of a width of its own and one beside a button included; a section's
//! heading has a gap above it; a body is no wider than the width it is laid out at, so a note
//! wraps in it. shell::dialog's form is the window's one form layout (dialog_layout's grid was
//! folded into it), so these are the rules every dialog's form keeps.
//!
//! Nothing here is a pixel of ink: rectangles are compared with each other and with the frame's
//! own constants. Within HALF_A_POINT where egui rounds a widget to the pixel grid.

use egui::{Rect, Ui};
use katana_window::drive::{self, LineOptions};
use katana_window::shell::DialogFrame;
use katana_window::shell::dialog::{
    BODY_WIDTH, FOOTER_BUTTON_WIDTH, FORM_SPACING, LABEL_WIDTH, PADDING, SECTION_GAP,
    beside_a_button, button_row, fixed_fields, labelled, note, row_with_text, section,
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
    fn a_named_label_shown_after_its_column_was_measured_widens_it_to_one_line() {
        // The Leaders dialog's "Landing direction" (a label that is a control of its own, drawn
        // with row_with) wrapped to the width of the shorter labels measured before it, a word to
        // a line: measured as drawn, it was measured in the narrow cell it was given.
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("namedDialog", "Named");
        let named_shown = std::cell::Cell::new(false);
        let mut values = [String::new(), String::new()];
        let caption = "A longer named label:";
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            frame.show_body(&ctx, &mut values, |ui, values| {
                let [short, named] = values;
                labelled(ui, "Ids:", |ui| drive::line(ui, "short", short));
                if named_shown.get() {
                    row_with_text(
                        ui,
                        caption,
                        |ui| {
                            drive::label(ui, "caption", caption);
                        },
                        |ui| drive::line(ui, "named", named),
                    );
                }
            });
        };
        // The column measured with "Ids:" alone, then the named row shown in it.
        harness.show(&mut draw).expect("the dialog is drawn");
        named_shown.set(true);
        harness.show(&mut draw).expect("the dialog is drawn again");
        let control = |name: &str| harness.control(name).expect("each control is drawn").rect;
        let (short, named, label) = (control("short"), control("named"), control("caption"));
        assert!((short.left() - named.left()).abs() <= HALF_A_POINT);
        assert!(
            label.right() <= named.left(),
            "the label runs to {} past its field at {}",
            label.right(),
            named.left()
        );
        // One line: no taller than the field beside it (a second line would make it some 34).
        assert!(
            label.height() <= named.height() + HALF_A_POINT,
            "the label is {} high beside a field {} high",
            label.height(),
            named.height()
        );
    }

    #[test]
    fn a_field_given_a_width_of_its_own_keeps_it_and_is_held_to_its_row() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("fixedDialog", "Fixed");
        let mut values = [String::new(), String::new(), String::new()];
        // Far narrower than the row (BODY_WIDTH less a label column), and far wider than it.
        let short_width = 100.0;
        let too_wide = 10.0 * BODY_WIDTH;
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show_body(&ctx, &mut values, |ui, values| {
                    let [short, wide, filled] = values;
                    labelled(ui, "Short:", |ui| {
                        fixed_fields(ui, short_width);
                        drive::line(ui, "short", short)
                    });
                    labelled(ui, "Wide:", |ui| {
                        fixed_fields(ui, too_wide);
                        drive::line(ui, "wide", wide)
                    });
                    labelled(ui, "Filled:", |ui| drive::line(ui, "filled", filled));
                });
            })
            .expect("the dialog is drawn");
        let field = |name: &str| harness.control(name).expect("each field is drawn").rect;
        let (short, wide, filled) = (field("short"), field("wide"), field("filled"));
        assert!(
            (short.width() - short_width).abs() <= HALF_A_POINT,
            "the field is {} wide",
            short.width()
        );
        // A fixed width does not move a field off the label column's edge.
        assert!((short.left() - filled.left()).abs() <= HALF_A_POINT);
        // Wider than its row, it is held to the row: it ends where a field filling the row does.
        assert!(
            (wide.right() - filled.right()).abs() <= HALF_A_POINT,
            "the wide field ends at {}, the row at {}",
            wide.right(),
            filled.right()
        );
    }

    #[test]
    fn a_field_beside_a_button_leaves_the_button_its_room_at_the_rows_end() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("besideDialog", "Beside");
        let mut values = [String::new(), String::new()];
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show_body(&ctx, &mut values, |ui, values| {
                    let [path, other] = values;
                    labelled(ui, "File:", |ui| {
                        let width = beside_a_button(ui, "Browse...");
                        drive::line_with(
                            ui,
                            "path",
                            path,
                            LineOptions {
                                width: Some(width),
                                ..LineOptions::default()
                            },
                        );
                        drive::button(ui, "browse", "Browse...");
                    });
                    labelled(ui, "Other:", |ui| drive::line(ui, "other", other));
                });
            })
            .expect("the dialog is drawn");
        let control = |name: &str| harness.control(name).expect("each control is drawn").rect;
        let (path, browse, other) = (control("path"), control("browse"), control("other"));
        assert!((path.left() - other.left()).abs() <= HALF_A_POINT);
        assert!(
            path.right() < browse.left(),
            "the button is after the field"
        );
        // The button ends the row where a field filling its row ends: inside the body, not past it.
        assert!(
            (browse.right() - other.right()).abs() <= HALF_A_POINT,
            "the button ends at {}, the row at {}",
            browse.right(),
            other.right()
        );
    }

    #[test]
    fn a_section_heading_has_a_gap_above_it_unless_it_starts_its_body() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("sectionDialog", "Sections");
        // The body's top, and the two headings, as the body drew them.
        let mut drawn = [Rect::NOTHING; 3];
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show_body(&ctx, &mut drawn, |ui, drawn| {
                    drawn[0] = ui.max_rect();
                    drawn[1] = section(ui, "First").rect;
                    drive::label(ui, "between", "A line between the two");
                    drawn[2] = section(ui, "Second").rect;
                });
            })
            .expect("the dialog is drawn");
        let [body, first, second] = drawn;
        let between = harness.control("between").expect("drawn").rect;
        assert!(
            (first.top() - body.top()).abs() <= HALF_A_POINT,
            "the first heading starts at {}, its body at {}",
            first.top(),
            body.top()
        );
        assert!(
            (second.top() - between.bottom() - SECTION_GAP).abs() <= HALF_A_POINT,
            "the second heading is {} under what is above it",
            second.top() - between.bottom()
        );
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
