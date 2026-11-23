//! Not a twin of a C++ file: the sheet canvas's kept picture (docs/performance.md, "The sheet
//! editor's canvas"). The canvas keeps the last picture it painted, and the editor converts it
//! and hands it to its texture only when a paint left it different - so what the screen shows is
//! only as right as the rule "a changed picture is always handed over, whole".
//!
//! The frames are the editor's own, through egui in the harness. What a frame did is asked of
//! it, never assumed: the Checks dock lays itself out again after its findings change, which
//! resizes the canvas a frame or two after a change to the sheets.

mod kept_canvas {
    use katana_cad::Document;
    use katana_cad::plotting::sheet_commands::edit_viewport;
    use katana_cad::plotting::sheet_set::{SheetSet, ViewportKind};
    use katana_window::canvas::Image;
    use katana_window::sheet_editor::SheetKey;
    use katana_window::viewport_widget::MouseButton;

    use crate::support::{NONE, Shown, bx, sheet_of, viewport};

    /// An editor over one sheet of two notes, its checks run at once: with their delay they
    /// would run in the middle of a test's frames.
    fn two_notes() -> Shown {
        let set = SheetSet {
            sheets: vec![sheet_of(
                "s1",
                "TEST",
                vec![
                    viewport("vp1", ViewportKind::Notes, bx(100.0, 100.0, 200.0, 180.0)),
                    viewport("vp2", ViewportKind::Notes, bx(220.0, 100.0, 320.0, 180.0)),
                ],
            )],
            ..SheetSet::default()
        };
        let mut shown = Shown::with_set(Document::new(None), &set);
        shown.editor.checks_mut().set_delay(0);
        let _ = shown.editor.check_now(&shown.host);
        shown
    }

    /// One frame of the editor: the pictures it handed whole to a texture of the canvas's size
    /// (no other texture of the editor has it), and the canvas as it shows after the frame.
    fn frame(shown: &mut Shown) -> (Vec<egui::ColorImage>, Image) {
        let output = {
            let editor = &mut shown.editor;
            let host = &mut shown.host;
            shown
                .harness
                .frame(|ui: &mut egui::Ui| editor.ui(ui, &mut *host))
        };
        let canvas = shown.grab();
        let size = [canvas.width() as usize, canvas.height() as usize];
        let handed = output
            .textures_delta
            .set
            .values()
            .flatten()
            .filter(|delta| delta.pos.is_none() && delta.image.size() == size)
            .map(|delta| match &delta.image {
                egui::ImageData::Color(image) => (**image).clone(),
            })
            .collect();
        (handed, canvas)
    }

    /// One frame, held to the rule: a canvas that is not the last frame's is handed to the
    /// texture once, whole, and what is handed is the canvas; a canvas that is the last
    /// frame's is not handed over at all. Answers whether it changed, and keeps it in `last`.
    fn frame_changed(shown: &mut Shown, last: &mut Image) -> bool {
        let (handed, now) = frame(shown);
        let changed = now != *last;
        if changed {
            assert_eq!(handed.len(), 1, "a changed canvas is handed over");
            assert!(handed[0] == now.to_color_image());
        } else {
            assert!(handed.is_empty(), "an unchanged canvas is not handed over");
        }
        *last = now;
        changed
    }

    /// Frames until three in a row leave the canvas as it was, each held to the rule: what a
    /// change to the sheets sets going has then run.
    fn settle(shown: &mut Shown, last: &mut Image) {
        let mut still = 0;
        for _ in 0..30 {
            still = if frame_changed(shown, last) {
                0
            } else {
                still + 1
            };
            if still == 3 {
                return;
            }
        }
        panic!("the canvas did not settle in 30 frames");
    }

    #[test]
    fn the_texture_is_given_the_canvas_when_it_changes_and_only_then() {
        let mut shown = two_notes();
        let (_, mut last) = frame(&mut shown);
        settle(&mut shown, &mut last);

        // Frames with nothing changed: painted again, the same, and not handed over.
        for _ in 0..3 {
            assert!(!frame_changed(&mut shown, &mut last));
        }

        // A viewport picked: its outline and handles over the sheet, handed over once.
        shown.editor.canvas_mut().select("vp1");
        assert!(frame_changed(&mut shown, &mut last));
        for _ in 0..3 {
            assert!(!frame_changed(&mut shown, &mut last));
        }

        // The sheet itself changed: painted afresh and handed over.
        edit_viewport(
            shown.document_mut(),
            "vp1",
            |v| {
                v.text = String::from("CHANGED");
                Ok(())
            },
            "EDIT_VIEWPORT",
        )
        .expect("edited");
        assert!(frame_changed(&mut shown, &mut last));
        settle(&mut shown, &mut last);

        // Nothing picked again: handed over, since the texture holds the handles.
        shown.editor.canvas_mut().select("");
        assert!(frame_changed(&mut shown, &mut last));
        assert!(!frame_changed(&mut shown, &mut last));
    }

    // ---- the picture kept whole --------------------------------------------------------------
    //
    // A paint with no drag under way, and everything the canvas's picture is painted from as
    // the last paint had it (CanvasKey, sheet_editor.rs), keeps the picture and paints nothing.
    // One case an input of the key: a paint with the input as it was is kept, a change of it
    // paints again, and the picture kept is, pixel for pixel, what a paint of everything gives.
    // `SheetCanvas::paints` says which path a paint took; these cases paint the canvas
    // directly and draw no frame, so nothing lays the canvas out again under them.

    /// A paint of the canvas, and whether it painted the marks over the sheet again; false is
    /// a paint that kept the picture.
    fn painted(shown: &mut Shown) -> bool {
        let before = shown.canvas().paints();
        shown.paint();
        shown.canvas().paints() != before
    }

    /// The canvas as it stands is kept by a paint with nothing changed, and the picture it
    /// keeps is the one a paint of everything then gives: `invalidate` is the editor's word
    /// that the document changed, on which the sheet is painted again and the marks over it.
    fn assert_kept(shown: &mut Shown) {
        // Whatever the events before this changed is painted first.
        shown.paint();
        assert!(
            !painted(shown),
            "a paint with nothing changed keeps the picture"
        );
        let kept = shown.grab();
        shown.canvas_mut().invalidate();
        assert!(painted(shown), "the paint of everything paints the marks");
        assert!(
            shown.grab() == kept,
            "the kept picture is what a paint of everything gives"
        );
        assert!(!painted(shown));
    }

    #[test]
    fn a_paint_with_nothing_changed_keeps_the_canvas_a_paint_of_everything_gives() {
        let mut shown = two_notes();
        assert_kept(&mut shown);
        // Nor is the sheet under the marks painted by a paint that kept the picture.
        let renders = shown.canvas().renders();
        for _ in 0..3 {
            assert!(!painted(&mut shown));
        }
        assert_eq!(shown.canvas().renders(), renders);
    }

    #[test]
    fn an_editor_with_no_sheet_keeps_its_words() {
        let mut shown = Shown::new(Document::new(None));
        assert_kept(&mut shown);
    }

    #[test]
    fn a_change_of_the_selection_paints_the_canvas_again() {
        let mut shown = two_notes();
        assert_kept(&mut shown);
        let none = shown.grab();

        shown.canvas_mut().select("vp1");
        assert!(painted(&mut shown));
        let one = shown.grab();
        assert!(one != none);
        assert_kept(&mut shown);

        // A second with it, the second the primary: the group's outline, and no handles.
        let both = [String::from("vp1"), String::from("vp2")];
        shown.canvas_mut().set_selection(&both, "vp2");
        assert!(painted(&mut shown));
        let two = shown.grab();
        assert!(two != one);
        assert_kept(&mut shown);

        // The same two, the other the primary: the key holds which.
        shown.canvas_mut().set_selection(&both, "vp1");
        assert!(painted(&mut shown));
        assert!(shown.grab() != two);

        shown.canvas_mut().select("");
        assert!(painted(&mut shown));
        assert!(shown.grab() == none);
    }

    #[test]
    fn a_change_to_the_sheet_paints_the_canvas_again() {
        let mut shown = two_notes();
        // Picked, so that its outline and handles move with it whatever the sheet draws of an
        // empty note.
        shown.canvas_mut().select("vp2");
        assert_kept(&mut shown);
        let before = shown.grab();

        edit_viewport(
            shown.document_mut(),
            "vp2",
            |v| {
                v.rect = bx(220.0, 60.0, 320.0, 140.0);
                Ok(())
            },
            "MOVE_VIEWPORT",
        )
        .expect("moved");
        assert!(painted(&mut shown));
        assert!(shown.grab() != before);
        assert_kept(&mut shown);
    }

    #[test]
    fn another_sheet_shown_paints_the_canvas_again() {
        let mut first = viewport("vp1", ViewportKind::Notes, bx(100.0, 100.0, 200.0, 180.0));
        first.text = String::from("FIRST");
        let mut second = viewport("vp2", ViewportKind::Notes, bx(60.0, 60.0, 260.0, 220.0));
        second.text = String::from("SECOND");
        let set = SheetSet {
            sheets: vec![
                sheet_of("s1", "FIRST", vec![first]),
                sheet_of("s2", "SECOND", vec![second]),
            ],
            ..SheetSet::default()
        };
        let mut shown = Shown::with_set(Document::new(None), &set);
        assert_kept(&mut shown);
        let on_first = shown.grab();

        shown.editor.set_current_sheet(&shown.host, 1);
        assert!(painted(&mut shown));
        assert!(shown.grab() != on_first);
        assert_kept(&mut shown);

        shown.editor.set_current_sheet(&shown.host, 0);
        assert!(painted(&mut shown));
        assert!(shown.grab() == on_first);
    }

    #[test]
    fn a_zoom_of_the_paper_paints_the_canvas_again() {
        let mut shown = two_notes();
        assert_kept(&mut shown);
        let fitted = shown.grab();

        let at = shown.at(150.0, 140.0);
        shown.canvas_mut().wheel(at, 1.0);
        assert!(painted(&mut shown));
        assert!(shown.grab() != fitted);
        assert_kept(&mut shown);
    }

    #[test]
    fn a_canvas_laid_out_at_another_size_or_ratio_is_painted_again() {
        let mut shown = two_notes();
        assert_kept(&mut shown);
        let whole = shown.grab();

        let (width, height) = {
            let canvas = shown.canvas();
            (canvas.width(), canvas.height())
        };
        shown.canvas_mut().resize(width - 40.0, height - 30.0);
        assert!(painted(&mut shown));
        let smaller = shown.grab();
        assert!(smaller.width() < whole.width() && smaller.height() < whole.height());
        assert_kept(&mut shown);

        // Two device pixels a pixel of the canvas: the picture has more of them.
        shown.canvas_mut().set_device_ratio(2.0);
        assert!(painted(&mut shown));
        assert!(shown.grab().width() > smaller.width());
        assert_kept(&mut shown);
    }

    #[test]
    fn the_paper_grid_and_the_rulers_switched_each_paint_the_canvas_again() {
        let mut shown = two_notes();
        assert!(shown.canvas().rulers_shown());
        assert_kept(&mut shown);
        let plain = shown.grab();

        shown.canvas_mut().set_snap_to_grid(true);
        assert!(painted(&mut shown));
        assert!(shown.grab() != plain);
        assert_kept(&mut shown);
        shown.canvas_mut().set_snap_to_grid(false);
        assert!(painted(&mut shown));
        assert!(shown.grab() == plain);

        shown.canvas_mut().set_rulers_shown(false);
        assert!(painted(&mut shown));
        assert!(shown.grab() != plain);
        assert_kept(&mut shown);
        shown.canvas_mut().set_rulers_shown(true);
        assert!(painted(&mut shown));
        assert!(shown.grab() == plain);
    }

    #[test]
    fn the_cursor_on_the_paper_moves_the_rulers_marks_and_paints_the_canvas_again() {
        let mut shown = two_notes();
        assert_kept(&mut shown);
        let away = shown.grab();

        let at = shown.at(150.0, 140.0);
        shown.move_to(at, NONE);
        assert!(painted(&mut shown));
        let marked = shown.grab();
        assert!(marked != away);

        // The pointer still: kept, marks and all.
        shown.move_to(at, NONE);
        assert!(!painted(&mut shown));
        assert_kept(&mut shown);
        assert!(shown.grab() == marked);

        let along = shown.at(190.0, 120.0);
        shown.move_to(along, NONE);
        assert!(painted(&mut shown));
        assert!(shown.grab() != marked);

        // Off the canvas: the marks go.
        shown.editor.leave();
        assert!(painted(&mut shown));
        assert!(shown.grab() == away);
    }

    // No key: a drag's marks are read from what the drag keeps changing.
    #[test]
    fn a_drag_under_way_is_painted_every_frame() {
        let mut shown = two_notes();
        assert_kept(&mut shown);
        let at = shown.at(150.0, 140.0);

        // The paper under the hand (the middle button), held still: painted all the same.
        shown.press(at, MouseButton::Middle, NONE);
        for _ in 0..3 {
            assert!(painted(&mut shown));
        }
        shown.release(at, MouseButton::Middle);
        // Let go: the first paint has no key to match, and the ones after are kept.
        assert!(painted(&mut shown));
        assert_kept(&mut shown);

        // A viewport under the hand, and Esc abandoning the drag.
        shown.press(at, MouseButton::Left, NONE);
        assert_eq!(shown.selected(), "vp1");
        for _ in 0..3 {
            assert!(painted(&mut shown));
        }
        shown.key(SheetKey::Escape, NONE);
        assert!(painted(&mut shown));
        assert_kept(&mut shown);
    }
}
