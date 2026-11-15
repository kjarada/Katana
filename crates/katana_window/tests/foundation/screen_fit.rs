//! The twins of tests/qt_widgets/test_screen_fit.cpp: a window never runs off its screen
//! (shell::dialog): a dialog taller or wider than the room its screen leaves scrolls rather than
//! losing its bottom, a dialog that fits is left as drawn, its footer stays in sight, and a menu
//! taller than the screen is one scrolling column.
//!
//! The screen is Qt's offscreen platform's, 800 x 600, so the room is 784 x 584 less a frame: every
//! dialog here is built far over it, and a menu of a hundred items is over it at any text size.
//! Nothing is measured in pixels of ink: sizes are compared with the screen's own, and a control is
//! "reachable" when scrolling it into view brings it inside the viewport of the scroll area that
//! holds it. Qt's inclusive QRect edges (`bottom()` is the last row) become egui's float rects, so
//! "above" is `bottom <= top` where the C++ wrote `bottom < top`.
//!
//! Eleven of the C++'s twenty cases are exempt (the plan's section 3.3): QtsOwnDialogsKeepTheir-
//! LayoutsToThemselves, FittingWithoutTheFilterDoesTheSameAsTheFilterDoes, ADialogShownAgain-
//! IsNotWrappedAgain, ADialogShownAgainKeepsOneFooter, OnlyTheLastRowOfButtonsIsTheFooterAndAn-
//! EarlierOneScrollsWithTheBody, AReadOnlyLineBesideTheButtonsIsPartOfTheirRow, ADialogWithNoRowOf-
//! ButtonsHasNoFooterAndEverythingScrolls, AButtonRowThatIsNotLastAndIsFollowedByABigListIsNo-
//! Footer, ARowWithAFieldInItIsAFormsOwnAndNotTheFooter, ADialogLaidOutInAGridKeepsItsButtonsIn-
//! TheScrollArea and ADialogThatFitsWithAButtonRowKeepsItsLayoutAndHasNoFooter: their subject is
//! how Qt's retained layouts are read and wrapped, where an egui dialog declares its footer and is
//! fitted at every frame.

use egui::{Pos2, Rect, Ui, Vec2};
use katana_window::drive::{self, Action, Harness, Part, TextOptions, action_item};
use katana_window::shell::{DialogFrame, frame_of, popup_menu, room_on_screen};

use crate::window_support::offscreen_harness;

mod screen_fit {
    use super::*;

    /// The screen's available area (the C++'s screenRoom, availableGeometry).
    fn screen_room(harness: &Harness) -> Rect {
        harness.screen()
    }

    /// `rows` buttons one above another, each at least `height` high and named `<prefix><i>`: the
    /// shape of Terrain > Volumes or a manager with a form of fields.
    fn rows_of(ui: &mut Ui, rows: usize, prefix: &str, height: f32) {
        for at in 0..rows {
            let text = format!("{prefix} {at}");
            drive::button_as(
                ui,
                &format!("{prefix}{at}"),
                &text,
                egui::Button::new(text.as_str()).min_size(Vec2::new(0.0, height)),
            );
        }
    }

    /// The tall dialog the C++ builds: `rows` buttons of `height`, named row0, row1, ...;
    /// whether it scrolls.
    fn tall_dialog(ui: &mut Ui, frame: &mut DialogFrame, rows: usize, height: f32) -> bool {
        let ctx = ui.ctx().clone();
        frame
            .show_body(&ctx, &mut (), |ui, _| rows_of(ui, rows, "row", height))
            .scrolls
    }

    /// Whether control `name` of `dialog`, scrolled to, lies inside the viewport of its scroll
    /// area.
    fn reachable(
        harness: &mut Harness,
        dialog: &str,
        name: &str,
        draw: impl FnMut(&mut Ui),
    ) -> bool {
        harness
            .target(dialog)
            .scroll_to(name)
            .run(draw)
            .expect("the control is scrolled to");
        let control = harness
            .snapshot()
            .control(dialog, name)
            .cloned()
            .expect("the control is drawn");
        control.clip.contains_rect(control.rect)
    }

    #[test]
    fn a_dialog_that_fits_is_left_exactly_as_it_was_written() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("fittingDialog", "A dialog that fits");
        let mut scrolls = true;
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                scrolls = frame
                    .show_body(&ctx, &mut (), |ui, _| {
                        for at in 0..3 {
                            drive::button(ui, &format!("row{at}"), &format!("Row {at}"));
                        }
                    })
                    .scrolls;
            })
            .expect("the dialog is drawn");
        let room = screen_room(&harness);
        let target = harness
            .target_record("fittingDialog")
            .expect("the dialog is a target");
        assert!(
            target.scroll.is_none(),
            "a dialog that fits is given no scroll area"
        );
        // And keeps the layout it was written with: its rows one under another, none cut off.
        let rows: Vec<_> = (0..3)
            .map(|at| {
                harness
                    .control(&format!("row{at}"))
                    .expect("each row is drawn")
            })
            .collect();
        for pair in rows.windows(2) {
            assert!(pair[0].rect.bottom() <= pair[1].rect.top());
        }
        for row in &rows {
            assert!(row.clip.contains_rect(row.rect));
        }
        assert!(!scrolls);
        assert!(target.rect.height() <= room.height());
        assert!(target.rect.width() <= room.width());
    }

    #[test]
    fn a_dialog_taller_than_the_screen_scrolls_and_every_control_is_reachable() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("tallDialog", "Tall");
        // Eighty rows of 40 px: 3,200 px of controls on a 600 px screen.
        let mut draw = |ui: &mut Ui| {
            tall_dialog(ui, &mut frame, 80, 40.0);
        };
        harness.show(&mut draw).expect("the dialog is drawn");
        let room = screen_room(&harness);
        let target = harness
            .target_record("tallDialog")
            .expect("the dialog is a target");
        let scroll = target
            .scroll
            .expect("the controls did not fit, so they scroll");
        assert!(
            scroll.content.y > 4.0 * room.height(),
            "the premise: its controls need far more room than there is"
        );
        // The window is as large as the screen lets it be, and no larger.
        assert!(target.rect.height() <= room.height());
        assert!(target.rect.width() <= room.width());
        assert!(
            target.rect.height() >= room.height() / 2.0,
            "not collapsed to its title bar"
        );
        assert!(
            scroll.max_offset().y > 0.0,
            "there is something to scroll to"
        );
        // Every control: the first, the last, and one in the middle, found as they were - by
        // name in the dialog, whose controls they still are inside its scrolling body.
        for name in ["row0", "row39", "row79"] {
            let row = harness
                .snapshot()
                .control("tallDialog", name)
                .cloned()
                .unwrap_or_else(|| panic!("{name} is drawn"));
            assert_eq!(row.target, "tallDialog", "{name}");
            assert!(
                reachable(&mut harness, "tallDialog", name, &mut draw),
                "{name} cannot be scrolled into view"
            );
        }
        let frame_targets = harness
            .snapshot()
            .targets
            .iter()
            .filter(|target| target.name == "tallDialog")
            .count();
        assert_eq!(frame_targets, 1);
    }

    #[test]
    fn a_dialog_wider_than_the_screen_scrolls_across_as_well() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("wideDialog", "Wide");
        // A field far wider than the screen, and a button at the end of the row: the button is
        // reachable only by scrolling across.
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            frame.show_body(&ctx, &mut (), |ui, _| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        Vec2::new(3000.0, 20.0),
                        egui::Label::new("a field far wider than the screen"),
                    );
                    drive::button(ui, "far", "Far");
                });
            });
        };
        harness.show(&mut draw).expect("the dialog is drawn");
        let room = screen_room(&harness);
        let target = harness
            .target_record("wideDialog")
            .expect("the dialog is a target");
        let scroll = target.scroll.expect("the controls did not fit");
        assert!(target.rect.width() <= room.width());
        assert!(scroll.max_offset().x > 0.0);
        assert!(reachable(&mut harness, "wideDialog", "far", &mut draw));
    }

    #[test]
    fn a_dialog_placed_off_the_screen_by_hand_is_brought_back_onto() {
        let mut harness = offscreen_harness();
        let room = screen_room(&harness);
        let mut frame = DialogFrame::new("placedDialog", "Placed");
        frame.place(Pos2::new(room.right() + 400.0, room.bottom() + 400.0));
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                frame.show_body(&ctx, &mut (), |ui, _| {
                    drive::button(ui, "close", "Close");
                });
            })
            .expect("the dialog is drawn");
        let target = harness
            .target_record("placedDialog")
            .expect("the dialog is a target");
        assert!(
            room.contains_rect(target.rect),
            "placed at {:?}",
            target.rect.min
        );
    }

    #[test]
    fn the_room_is_inside_the_screens_available_area_with_a_margin_round_it() {
        let harness = offscreen_harness();
        let available = screen_room(&harness);
        let room = room_on_screen(available);
        assert!(available.contains_rect(room));
        assert!(room.width() < available.width());
        assert!(room.height() < available.height());
        assert!(
            frame_of(&harness.context().global_style()).y > 0.0,
            "a title bar is allowed for, shown or not"
        );
    }

    // ---- the footer: a dialog's own buttons do not scroll -------------------------------------

    #[test]
    fn a_dialogs_button_box_stays_below_the_scroll_area_and_in_sight_whatever_is_scrolled() {
        let mut harness = offscreen_harness();
        let mut frame = DialogFrame::new("buttonBoxDialog", "Buttons");
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            frame.show(
                &ctx,
                &mut (),
                |ui, _| rows_of(ui, 80, "body", 40.0),
                |ui, _| {
                    drive::button(ui, "footerClose", "Close");
                },
            );
        };
        harness.show(&mut draw).expect("the dialog is drawn");

        let target = harness
            .target_record("buttonBoxDialog")
            .expect("the dialog is a target");
        let scroll = target.scroll.expect("3,200 px of rows do not fit a screen");
        let footer = target
            .footer
            .expect("the button box is the dialog's footer");
        let close = harness
            .snapshot()
            .control("buttonBoxDialog", "footerClose")
            .cloned()
            .expect("found by name in the dialog, as before");
        assert_eq!(close.part, Part::Footer);
        assert!(footer.contains_rect(close.rect));
        assert!(
            scroll.viewport.bottom() <= footer.top(),
            "the footer is beside the scroll area"
        );
        assert_eq!(close.target, "buttonBoxDialog");
        assert!(close.visible);
        assert!(
            target.rect.contains_rect(close.rect),
            "the button is on the dialog without scrolling"
        );

        // The body scrolls from its bottom to its top and the button does not move.
        let resting = close.rect;
        assert!(scroll.max_offset().y > 0.0);
        for name in ["body79", "body0"] {
            harness
                .target("buttonBoxDialog")
                .scroll_to(name)
                .run(&mut draw)
                .expect("the body is scrolled");
            let close = harness.control("footerClose").expect("the button is drawn");
            assert_eq!(close.rect, resting, "scrolled to {name}");
        }
        // And all of the body is still reached by scrolling.
        for name in ["body0", "body40", "body79"] {
            let row = harness
                .snapshot()
                .control("buttonBoxDialog", name)
                .cloned()
                .unwrap_or_else(|| panic!("{name} is drawn"));
            assert_eq!(row.part, Part::Body, "{name}");
            assert!(
                reachable(&mut harness, "buttonBoxDialog", name, &mut draw),
                "{name}"
            );
        }
        let in_footer = harness
            .snapshot()
            .in_target("buttonBoxDialog")
            .filter(|control| control.part == Part::Footer)
            .count();
        assert_eq!(in_footer, 1);
    }

    #[derive(Default)]
    struct Reply {
        reply: String,
        pressed: usize,
    }

    #[test]
    fn a_button_row_and_the_reply_under_it_are_the_footer_together() {
        // The shape of Terrain > Edit Surface's dialogs: a body, a row of Preview, Run and Close,
        // and under it the box that says what Run did. A reply scrolled out of sight after
        // pressing a button that never scrolls would look like the button did nothing.
        let mut harness = offscreen_harness();
        let mut state = Reply::default();
        let mut frame = DialogFrame::new("replyDialog", "Reply");
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            frame.show(
                &ctx,
                &mut state,
                |ui, _| rows_of(ui, 80, "body", 40.0),
                |ui, state| {
                    ui.horizontal(|ui| {
                        drive::button(ui, "preview", "Preview");
                        // The row's stretch between Preview and Run.
                        ui.add_space(24.0);
                        if drive::button(ui, "run", "Run").clicked() {
                            state.pressed += 1;
                        }
                        drive::button(ui, "close", "Close");
                    });
                    drive::text_with(
                        ui,
                        "reply",
                        &mut state.reply,
                        TextOptions {
                            read_only: true,
                            rows: 4,
                            monospace: false,
                        },
                    );
                },
            );
        };
        harness.show(&mut draw).expect("the dialog is drawn");

        let target = harness
            .target_record("replyDialog")
            .expect("the dialog is a target");
        let scroll = target.scroll.expect("the body does not fit");
        let footer = target.footer.expect("the row and its reply are the footer");
        let frame_now = harness.snapshot();
        let control = |name: &str| {
            frame_now
                .control("replyDialog", name)
                .cloned()
                .unwrap_or_else(|| panic!("{name} is drawn"))
        };
        let (preview, run, close, reply) = (
            control("preview"),
            control("run"),
            control("close"),
            control("reply"),
        );
        for widget in [&preview, &run, &close, &reply] {
            assert_eq!(widget.part, Part::Footer, "{}", widget.name);
            assert!(footer.contains_rect(widget.rect), "{}", widget.name);
            assert!(widget.visible, "{}", widget.name);
            assert!(
                target.rect.contains_rect(widget.rect),
                "{} is off the dialog",
                widget.name
            );
        }
        // Their order is the one they were written in: the row above its reply.
        assert!(run.rect.bottom() <= reply.rect.top());
        assert!(preview.rect.right() <= run.rect.left());
        assert!(run.rect.right() <= close.rect.left());
        // The footer is the bottom of the dialog and the body is above it.
        assert!(scroll.viewport.bottom() <= footer.top());
        // A button of the footer still works, by its own click.
        harness
            .target("replyDialog")
            .press("run")
            .run(&mut draw)
            .expect("Run is pressed");
        assert_eq!(state.pressed, 1);
    }

    // ---- menus ----------------------------------------------------------------------------------

    #[test]
    fn a_menu_taller_than_the_screen_is_one_scrolling_column() {
        // Every menu's items are drawn in the one scrolling column (shell::dialog::menu_column):
        // Qt's style hint and stylesheet have no counterpart to check.
        let mut harness = offscreen_harness();
        let room = screen_room(&harness);
        let actions: Vec<Action> = (0..120)
            .map(|at| Action::new(&format!("item{at}"), &format!("Item {at}")))
            .collect();
        let at = room.left_top() + Vec2::new(20.0, 20.0);
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            popup_menu(&ctx, "testMenu", at, |ui| {
                for action in &actions {
                    action_item(ui, action);
                }
            });
        };
        harness.show(&mut draw).expect("the menu is drawn");

        // As large as the screen lets it be, not as large as its items.
        let menu = harness
            .target_record("testMenu")
            .expect("the menu is a target");
        let column = menu.scroll.expect("its column is recorded");
        assert!(menu.rect.height() <= room.height());
        assert!(
            column.content.y > room.height(),
            "the premise: it does not fit"
        );
        // One column: the first and the last item are one above the other, so a reader goes down
        // it and not across a wall of columns.
        let first = harness
            .snapshot()
            .control("testMenu", "item0")
            .cloned()
            .expect("the first item is drawn");
        let last = harness
            .snapshot()
            .control("testMenu", "item119")
            .cloned()
            .expect("the last item is drawn");
        assert_eq!(first.rect.left(), last.rect.left());
        assert!(menu.rect.width() < room.width() / 2.0);
        // And the last is reachable: scrolled to, it is on the menu; and so is the first again.
        harness
            .target("testMenu")
            .scroll_to("item119")
            .run(&mut draw)
            .expect("the menu is scrolled");
        let menu = harness
            .target_record("testMenu")
            .expect("the menu is drawn");
        let last = harness.control("item119").expect("the last item is drawn");
        assert!(
            menu.rect.contains_rect(last.rect),
            "the last item is off the menu after scrolling to it"
        );
        harness
            .scroll_to("item0")
            .run(&mut draw)
            .expect("the menu is scrolled back");
        let menu = harness
            .target_record("testMenu")
            .expect("the menu is drawn");
        let first = harness.control("item0").expect("the first item is drawn");
        assert!(menu.rect.contains_rect(first.rect));
    }

    #[test]
    fn a_menu_that_fits_is_not_scrolled() {
        let mut harness = offscreen_harness();
        let room = screen_room(&harness);
        let actions = [Action::new("one", "One"), Action::new("two", "Two")];
        let at = room.left_top() + Vec2::new(20.0, 20.0);
        harness
            .show(|ui| {
                let ctx = ui.ctx().clone();
                popup_menu(&ctx, "shortMenu", at, |ui| {
                    for action in &actions {
                        action_item(ui, action);
                    }
                });
            })
            .expect("the menu is drawn");
        let menu = harness
            .target_record("shortMenu")
            .expect("the menu is a target");
        let column = menu.scroll.expect("its column is recorded");
        assert!(!column.scrolls(), "its height is its items'");
        for name in ["one", "two"] {
            let item = harness.control(name).expect("each item is drawn");
            assert!(menu.rect.contains_rect(item.rect), "{name}");
        }
    }
}
