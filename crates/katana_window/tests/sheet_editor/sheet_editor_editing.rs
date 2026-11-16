//! The twins of tests/qt_widgets/plotting/test_sheet_editor_editing.cpp: the sheet editor's
//! editing of several viewports at once and its furniture - Ctrl- and Shift-clicks, the rubber
//! band, a group dragged, nudged and deleted as ONE undoable step each, copy and paste onto
//! another sheet with new ids, duplicate, the paper grid, the rulers, the cursor's paper and world
//! position, a dragged view's own paint going with it, Tab, double-click and Zoom to Selection, the
//! sheet list's thumbnails, its dragging and PgUp / PgDn.

mod sheet_editor_editing {
    use egui::{Key, Modifiers};
    use katana_cad::Document;
    use katana_cad::plotting::sheet_commands::{edit_sheet, edit_viewport};
    use katana_cad::plotting::sheet_set::{
        Sheet, SheetSet, Viewport, ViewportKind, WorldMark,
    };
    use katana_cad::plotting::viewport_edits::{PAPER_GRID_MM, plan_world_to_paper};
    use katana_commands::{EntityAttributes, create_line};
    use katana_core::ErrorCode;
    use katana_geometry::{Box2, Point2};
    use katana_window::canvas::{
        Color, Font, FontMetrics, Image, PointF, RectF, TextMetrics, write_png,
    };
    use katana_window::format::qt_fixed;
    use katana_window::host::{Clipboard, MemoryClipboard};
    use katana_window::plotting::sheet_rulers::{SHEET_RULER_PIXELS, ruler_steps};
    use katana_window::plotting::sheet_thumbnails::{SheetThumbnails, shows_the_other_sheets};
    use katana_window::plotting::viewport_clipboard::{
        viewports_from_clipboard_bytes, viewports_on_clipboard, viewports_to_clipboard_bytes,
    };
    use katana_window::sheet_editor::{SheetKey, ShortcutScope};
    use katana_window::sheet_painter::{SheetSource, resolve_plan_viewport};
    use katana_window::theme::palette;
    use katana_window::viewport_widget::MouseButton;

    use crate::support::{
        ALT, CTRL, NONE, SHIFT, Shown, bx, dark_in, double_eq, inside, near, sheet_of,
    };

    type Ids = Vec<String>;

    fn ids(names: &[&str]) -> Ids {
        names.iter().map(|name| String::from(*name)).collect()
    }

    fn panel(id: &str, rect: Box2, kind: ViewportKind) -> Viewport {
        Viewport {
            id: String::from(id),
            kind,
            rect,
            text: format!("PANEL {id}"),
            ..Viewport::default()
        }
    }

    fn notes(id: &str, rect: Box2) -> Viewport {
        panel(id, rect, ViewportKind::Notes)
    }

    /// Two sheets: FIRST with three notes panels in a row, the third locked; SECOND with one small
    /// panel.
    fn two_sheets() -> SheetSet {
        let mut first = sheet_of(
            "s1",
            "FIRST",
            vec![
                notes("vp1", bx(100.0, 100.0, 200.0, 180.0)),
                notes("vp2", bx(220.0, 100.0, 300.0, 180.0)),
                notes("vp3", bx(320.0, 100.0, 380.0, 180.0)),
            ],
        );
        first.viewports[2].locked = true;
        let second = sheet_of("s2", "SECOND", vec![notes("vp4", bx(50.0, 50.0, 90.0, 90.0))]);
        SheetSet {
            sheets: vec![first, second],
            ..SheetSet::default()
        }
    }

    fn shown() -> Shown {
        Shown::with_set(Document::new(None), &two_sheets())
    }

    fn expect_near(actual: PointF, expected: PointF) {
        assert!(near(actual.x, expected.x, 1e-9), "{actual:?} {expected:?}");
        assert!(near(actual.y, expected.y, 1e-9), "{actual:?} {expected:?}");
    }

    fn on_grid(value: f64) -> bool {
        (value / 5.0 - (value / 5.0).round()).abs() < 1e-9
    }

    fn line(document: &mut Document, a: Point2, b: Point2) {
        document
            .execute(create_line(a, b, EntityAttributes::default()))
            .expect("a line");
    }

    // ---- picking -----------------------------------------------------------------------------

    #[test]
    fn ctrl_and_shift_clicks_add_and_remove_views_from_the_selection() {
        let mut shown = shown();
        let before = shown.steps();

        let at = shown.at(150.0, 140.0);
        shown.click(at, NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
        let at = shown.at(260.0, 140.0);
        shown.click(at, CTRL);
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp2"]));
        // The last one picked is the primary: its handles, its properties.
        assert_eq!(shown.selected(), "vp2");
        let at = shown.at(350.0, 140.0);
        shown.click(at, SHIFT);
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp2", "vp3"]));
        // Again: out of the selection, and the primary passes on.
        shown.click(at, CTRL);
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp2"]));
        assert!(!shown.selected().is_empty());
        assert_ne!(shown.selected(), "vp3");
        // A plain click on one of a group selects it alone.
        let at = shown.at(150.0, 140.0);
        shown.click(at, NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
        assert_eq!(shown.selected(), "vp1");
        // Picking changes nothing in the project.
        assert_eq!(shown.steps(), before);
    }

    #[test]
    fn the_selection_lists_only_what_is_on_the_sheet_in_its_order() {
        let mut shown = shown();
        // Ids not on the sheet are dropped; the order is the sheet's, back to front.
        shown
            .canvas_mut()
            .set_selection(&ids(&["vp3", "nothing", "vp1", "vp4"]), "");
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp3"]));
        assert_eq!(shown.selected(), "vp1");
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp3"]), "vp3");
        assert_eq!(shown.selected(), "vp3");
        assert!(shown.canvas().is_selected("vp1"));
        assert!(!shown.canvas().is_selected("vp2"));
        shown.trigger("sheetSelectAll");
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp2", "vp3"]));
        // Another sheet starts with nothing selected.
        shown.editor.set_current_sheet(&shown.host, 1);
        assert!(shown.selected_ids().is_empty());
        assert!(shown.selected().is_empty());
    }

    #[test]
    fn a_rubber_band_on_empty_paper_selects_what_it_encloses_or_crosses() {
        let mut shown = shown();
        let page_at = shown.at(0.0, 0.0);
        let before = shown.steps();

        // Left to right, a window: vp1 wholly inside, vp2 only half.
        let (from, to) = (shown.at(90.0, 190.0), shown.at(260.0, 90.0));
        shown.drag(from, to, NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
        // Right to left, a crossing: both it touches.
        let (from, to) = (shown.at(260.0, 190.0), shown.at(90.0, 90.0));
        shown.drag(from, to, NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp2"]));
        // With Ctrl, the band adds to the selection.
        let (from, to) = (shown.at(310.0, 190.0), shown.at(390.0, 90.0));
        shown.drag(from, to, CTRL);
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp2", "vp3"]));
        // A plain click on empty paper selects nothing.
        let at = shown.at(250.0, 250.0);
        shown.click(at, NONE);
        assert!(shown.selected_ids().is_empty());

        // The band replaced the empty-paper pan: the paper has not moved, and nothing was
        // recorded.
        expect_near(shown.at(0.0, 0.0), page_at);
        assert_eq!(shown.steps(), before);
    }

    #[test]
    fn the_middle_button_and_space_drag_still_pan_the_paper() {
        let mut shown = shown();
        shown.canvas_mut().select("vp1");
        let page_at = shown.at(0.0, 0.0);

        let from = shown.at(250.0, 250.0);
        shown.drag_with(from, from + PointF::new(30.0, 20.0), NONE, MouseButton::Middle, true);
        expect_near(shown.at(0.0, 0.0), page_at + PointF::new(30.0, 20.0));

        // Space held: a left drag pans too, even from a viewport, and moves nothing.
        shown.key(SheetKey::Space, NONE);
        let from = shown.at(150.0, 140.0);
        shown.drag(from, from + PointF::new(-10.0, 5.0), NONE);
        shown.key_release(SheetKey::Space);
        expect_near(shown.at(0.0, 0.0), page_at + PointF::new(20.0, 25.0));
        assert_eq!(shown.by_id(0, "vp1").rect, bx(100.0, 100.0, 200.0, 180.0));
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
    }

    #[test]
    fn tab_steps_the_selection_through_the_views() {
        let mut shown = shown();
        shown.key(SheetKey::Tab, NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
        shown.key(SheetKey::Tab, NONE);
        assert_eq!(shown.selected(), "vp2");
        shown.key(SheetKey::Tab, NONE);
        // Round to the first again.
        shown.key(SheetKey::Tab, NONE);
        assert_eq!(shown.selected(), "vp1");
        shown.key(SheetKey::Backtab, SHIFT);
        assert_eq!(shown.selected_ids(), ids(&["vp3"]));
        shown.trigger("sheetSelectPreviousView");
        assert_eq!(shown.selected(), "vp2");
    }

    // ---- the group's edits: each one step -----------------------------------------------------

    #[test]
    fn dragging_one_of_several_selected_moves_them_all_in_one_step() {
        let mut shown = shown();
        shown
            .canvas_mut()
            .set_selection(&ids(&["vp1", "vp2", "vp3"]), "");
        let before = shown.steps();

        // By vp2, 30 mm right and 20 mm up, without snapping.
        let (from, to) = (shown.at(260.0, 140.0), shown.at(290.0, 160.0));
        shown.drag(from, to, ALT);
        assert_eq!(shown.steps(), before + 1);
        let first = shown.by_id(0, "vp1").rect;
        let second = shown.by_id(0, "vp2").rect;
        assert!(near(first.min.x, 130.0, 1e-6));
        assert!(near(first.min.y, 120.0, 1e-6));
        assert!(near(second.min.x, 250.0, 1e-6));
        assert!(near(second.min.y, 120.0, 1e-6));
        assert!(near(first.width(), 100.0, 1e-9));
        // The locked one stays; the group stays selected.
        assert_eq!(shown.by_id(0, "vp3").rect, bx(320.0, 100.0, 380.0, 180.0));
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp2", "vp3"]));

        // One undo puts the whole group back.
        shown.undo();
        assert!(shown.sheet_set() == two_sheets());
    }

    #[test]
    fn escape_abandons_a_drag_and_leaves_everything_where_it_was() {
        let mut shown = shown();
        let before = shown.steps();
        let (from, to) = (shown.at(150.0, 140.0), shown.at(170.0, 150.0));
        shown.drag_with(from, to, NONE, MouseButton::Left, false);
        shown.key(SheetKey::Escape, NONE);
        shown.release(to, MouseButton::Left);
        assert_eq!(shown.steps(), before);
        assert_eq!(shown.by_id(0, "vp1").rect, bx(100.0, 100.0, 200.0, 180.0));
    }

    #[test]
    fn the_arrows_nudge_every_selected_view_in_one_step_each() {
        let mut shown = shown();
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "");
        let before = shown.steps();
        shown.key(SheetKey::Right, NONE);
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.by_id(0, "vp1").rect, bx(101.0, 100.0, 201.0, 180.0));
        assert_eq!(shown.by_id(0, "vp2").rect, bx(221.0, 100.0, 301.0, 180.0));
        shown.key(SheetKey::Up, SHIFT);
        assert_eq!(shown.steps(), before + 2);
        assert_eq!(shown.by_id(0, "vp1").rect, bx(101.0, 110.0, 201.0, 190.0));
        // The API an agent calls does the same.
        shown
            .editor
            .nudge_selection(&mut shown.host, Point2::new(-1.0, -10.0))
            .expect("nudged");
        assert_eq!(shown.by_id(0, "vp2").rect, bx(220.0, 100.0, 300.0, 180.0));
        shown.canvas_mut().select("");
        assert!(
            shown
                .editor
                .nudge_selection(&mut shown.host, Point2::new(1.0, 0.0))
                .is_err()
        );
    }

    #[test]
    fn delete_removes_every_selected_view_in_one_step() {
        let mut shown = shown();
        // Nothing selected: Delete has nothing to do and leaves its key alone.
        assert!(!shown.action("sheetDeleteViews").enabled);

        shown.canvas_mut().set_selection(&ids(&["vp1", "vp3"]), "");
        assert!(shown.action("sheetDeleteViews").enabled);
        let before = shown.steps();
        shown.trigger("sheetDeleteViews");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.ids_of(0), ids(&["vp2"]));
        assert!(shown.selected_ids().is_empty());

        // The key on the canvas does the same, and undo brings them all back.
        shown.undo();
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "");
        shown.key(SheetKey::Delete, NONE);
        assert_eq!(shown.ids_of(0), ids(&["vp3"]));
        shown.undo();
        assert!(shown.sheet_set() == two_sheets());
    }

    // ---- copy, paste, duplicate ---------------------------------------------------------------

    #[test]
    fn views_copied_on_one_sheet_paste_onto_another_with_new_ids_in_one_step() {
        let mut shown = shown();
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "");
        shown.trigger("sheetCopy");
        assert!(shown.action("sheetPaste").enabled);
        // Copying records nothing.
        let before = shown.steps();

        shown.trigger("sheetNextSheet");
        assert_eq!(shown.editor.current_sheet(), 1);
        shown.trigger("sheetPaste");
        assert_eq!(shown.steps(), before + 1);
        let set = shown.sheet_set();
        let second = &set.sheets[1];
        // New ids after the highest in the set; where they were on the first sheet, since nothing
        // is there on this one; selected.
        assert_eq!(shown.ids_of(1), ids(&["vp4", "vp5", "vp6"]));
        assert_eq!(second.viewports[1].rect, bx(100.0, 100.0, 200.0, 180.0));
        assert_eq!(second.viewports[2].text, "PANEL vp2");
        assert_eq!(shown.selected_ids(), ids(&["vp5", "vp6"]));
        // The first sheet is as it was.
        assert!(set.sheets[0] == two_sheets().sheets[0]);

        // Pasted again on the same sheet: 5 mm right and down from the first paste.
        shown.editor.paste(&mut shown.host).expect("pasted");
        assert_eq!(shown.by_id(1, "vp7").rect, bx(105.0, 95.0, 205.0, 175.0));

        // Undo takes a paste away, and the selection with it.
        shown.undo();
        assert_eq!(shown.ids_of(1), ids(&["vp4", "vp5", "vp6"]));
        assert!(shown.selected_ids().is_empty());
    }

    #[test]
    fn cut_takes_the_views_and_paste_brings_them_back_where_they_were() {
        let mut shown = shown();
        shown.canvas_mut().set_selection(&ids(&["vp2"]), "");
        let before = shown.steps();
        shown.trigger("sheetCut");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.ids_of(0), ids(&["vp1", "vp3"]));
        shown.editor.paste(&mut shown.host).expect("pasted");
        assert_eq!(shown.steps(), before + 2);
        // Its place is free again, so it lands there, under a new id.
        assert_eq!(shown.by_id(0, "vp5").rect, bx(220.0, 100.0, 300.0, 180.0));
    }

    /// Pasting into a set with no sheets adds the sheet in the paste's own step: one Undo takes
    /// both away, rather than leaving a blank sheet behind.
    #[test]
    fn a_paste_into_a_set_with_no_sheets_is_one_step() {
        let mut shown = shown();
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "");
        shown
            .editor
            .copy_selection(&mut shown.host)
            .expect("copied");
        shown
            .document_mut()
            .set_sheet_set(&SheetSet::default(), "REMOVE_SHEETS")
            .expect("removed");
        shown.observe();
        let before = shown.steps();
        shown.editor.paste(&mut shown.host).expect("pasted");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.sheet_set().sheets.len(), 1);
        assert_eq!(shown.sheet_set().sheets[0].viewports.len(), 2);
        shown.undo();
        assert!(shown.sheet_set().sheets.is_empty());
    }

    #[test]
    fn duplicate_is_one_step_five_millimetres_right_and_down() {
        let mut shown = shown();
        shown.canvas_mut().set_selection(&ids(&["vp2", "vp1"]), "");
        let before = shown.steps();
        shown.trigger("sheetDuplicateViews");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.ids_of(0), ids(&["vp1", "vp2", "vp3", "vp5", "vp6"]));
        assert_eq!(shown.by_id(0, "vp5").rect, bx(105.0, 95.0, 205.0, 175.0));
        assert_eq!(shown.by_id(0, "vp6").rect, bx(225.0, 95.0, 305.0, 175.0));
        // The copies are selected, for the next drag.
        assert_eq!(shown.selected_ids(), ids(&["vp5", "vp6"]));
        shown.canvas_mut().select("");
        assert!(shown.editor.duplicate_selection(&mut shown.host).is_err());
        assert!(!shown.action("sheetDuplicateViews").enabled);
    }

    #[test]
    fn views_travel_on_the_clipboard_as_the_sheets_own_json() {
        let set = two_sheets();
        let bytes = viewports_to_clipboard_bytes(&set.sheets[0].viewports).expect("bytes");
        assert!(String::from_utf8_lossy(&bytes).contains("katana-sheets"));
        let back = viewports_from_clipboard_bytes(&bytes).expect("read back");
        assert_eq!(back, set.sheets[0].viewports);

        assert!(viewports_to_clipboard_bytes(&[]).is_err());
        assert!(viewports_from_clipboard_bytes(b"not json").is_err());
        // Bytes that are a sheet set but hold no viewports are refused too.
        assert_eq!(
            viewports_from_clipboard_bytes(br#"{"format": "katana-sheets", "version": 1}"#)
                .expect_err("refused")
                .code,
            ErrorCode::ParseFailure
        );

        // Something else on the clipboard is nothing to paste.
        let mut text = MemoryClipboard::new();
        text.set_text("hello").expect("text");
        assert_eq!(
            viewports_on_clipboard(&mut text).expect_err("nothing").code,
            ErrorCode::NotFound
        );
    }

    // ---- snapping to the paper grid -----------------------------------------------------------

    #[test]
    fn with_the_grid_on_a_dragged_view_lands_with_an_edge_on_it() {
        let set = SheetSet {
            sheets: vec![Sheet {
                id: String::from("s1"),
                viewports: vec![notes("vp1", bx(100.0, 100.0, 203.0, 180.0))],
                ..Sheet::default()
            }],
            ..SheetSet::default()
        };
        let mut shown = Shown::with_set(Document::new(None), &set);
        assert!(!shown.canvas().snaps_to_grid());

        shown.trigger("sheetSnapGrid");
        assert!(shown.canvas().snaps_to_grid());
        assert_eq!(shown.action("sheetSnapGrid").checked, Some(true));
        assert!(double_eq(shown.canvas().grid_spacing(), PAPER_GRID_MM));
        let (from, to) = (shown.at(150.0, 140.0), shown.at(157.3, 143.6));
        shown.drag(from, to, NONE);
        let snapped = shown.by_id(0, "vp1").rect;
        assert!(
            on_grid(snapped.min.x) || on_grid(snapped.max.x),
            "{} {}",
            snapped.min.x,
            snapped.max.x
        );
        assert!(
            on_grid(snapped.min.y) || on_grid(snapped.max.y),
            "{} {}",
            snapped.min.y,
            snapped.max.y
        );
        assert!(near(snapped.width(), 103.0, 1e-9));
        assert!(near(snapped.min.x, 107.3, 2.5));
        assert!(near(snapped.min.y, 103.6, 2.5));

        // A handle's edge snaps to the grid too.
        shown.undo();
        shown.canvas_mut().select("vp1");
        let (from, to) = (shown.at(203.0, 140.0), shown.at(231.2, 140.0));
        shown.drag(from, to, NONE);
        assert!(on_grid(shown.by_id(0, "vp1").rect.max.x));
        assert!(double_eq(shown.by_id(0, "vp1").rect.min.x, 100.0));

        // Off again: where the hand put it.
        shown.undo();
        shown.trigger("sheetSnapGrid");
        assert!(!shown.canvas().snaps_to_grid());
        let (from, to) = (shown.at(150.0, 140.0), shown.at(157.3, 143.6));
        shown.drag(from, to, NONE);
        assert!(near(shown.by_id(0, "vp1").rect.min.x, 107.3, 1e-6));
    }

    #[test]
    fn the_grid_is_drawn_faintly_only_while_it_is_on() {
        let mut shown = shown();
        // Empty paper inside the drawing area.
        let empty = inside(shown.canvas(), &bx(240.0, 200.0, 300.0, 260.0), 1.0);
        let off = shown.grab();
        shown.canvas_mut().set_snap_to_grid(true);
        let on = shown.grab();
        let mut changed = 0;
        let mut dark = 0;
        let (left, top) = (empty.left().floor() as u32, empty.top().floor() as u32);
        let (right, bottom) = (
            empty.right().ceil() as u32 - 1,
            empty.bottom().ceil() as u32 - 1,
        );
        for y in top..=bottom {
            for x in left..=right {
                let a = off.pixel(x, y);
                let b = on.pixel(x, y);
                changed += usize::from(a != b);
                // Faint: a tint of the paper, where two of the stronger lines cross no darker than
                // light grey.
                dark += usize::from(b.r < 150);
            }
        }
        assert!(changed > 50);
        assert_eq!(dark, 0);
        // The View menu shows what the canvas holds, however it was set.
        assert!(
            shown
                .editor
                .menus(&shown.host)
                .iter()
                .any(|menu| menu.name == "sheetViewMenu")
        );
        assert_eq!(shown.action("sheetSnapGrid").checked, Some(true));

        // The whole window as a hand sees it: the grid, a group selected, a crossing band
        // mid-drag, the rulers, the pictures in the list.
        if let Some(folder) = std::env::var("KATANA_SHEET_PNG").ok().filter(|folder| !folder.is_empty()) {
            shown.editor.render_thumbnails_now(&shown.host);
            shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "");
            let (from, to) = (shown.at(390.0, 60.0), shown.at(300.0, 150.0));
            shown.drag_with(from, to, CTRL, MouseButton::Left, false);
            let image = shown.grab();
            let _ = write_png(
                &image,
                &std::path::Path::new(&folder).join("SheetEditorEditing.png"),
            );
            shown.release(to, MouseButton::Left);
        }
    }

    // ---- the rulers and the cursor ------------------------------------------------------------

    #[test]
    fn the_rulers_steps_suit_the_zoom() {
        // At 2 px a millimetre, 50 mm is the first step 50 px long; ticks every 5.
        let coarse = ruler_steps(2.0);
        assert!(double_eq(coarse.numbered, 50.0));
        assert!(double_eq(coarse.tick, 5.0));
        // At 10 px, every 5 mm, ticks every half millimetre.
        let fine = ruler_steps(10.0);
        assert!(double_eq(fine.numbered, 5.0));
        assert!(double_eq(fine.tick, 0.5));
        // Far out, a metre of paper at a time.
        assert!(double_eq(ruler_steps(0.05).numbered, 1000.0));
    }

    #[test]
    fn the_rulers_frame_the_canvas_and_the_page_fits_beside_them() {
        let mut shown = shown();
        assert!(shown.canvas().rulers_shown());
        let r = f64::from(SHEET_RULER_PIXELS);
        // The page is fitted to what the rulers leave.
        assert!(shown.at(0.0, 297.0).x > r);
        assert!(shown.at(0.0, 297.0).y > r);

        let with = shown.grab();
        let corner = with.pixel((r / 2.0) as u32, 2);
        // The rulers' ground is the panel token, the chrome's grey: the rulers took the theme's
        // dark tokens in the look of 2026-11-16, where the C++'s were a light grey strip (this
        // asserted a red above 200 then). What is held is the same: the rulers are drawn, and
        // the paper's span on them stands apart from the desk's.
        assert_eq!(corner, palette::panel());
        // The paper's span on the top ruler, above its ticks and numbers, a step lighter.
        let paper_on_ruler = with.pixel(shown.at(235.0, 0.0).x as u32, 1);
        assert_eq!(paper_on_ruler, palette::hover());
        // The desk's is the ruler's own ground.
        let width = shown.canvas().width() as u32;
        assert_eq!(with.pixel(width - 2, 1), palette::panel());
        assert!(grey(paper_on_ruler) > grey(palette::panel()));

        shown.trigger("sheetShowRulers");
        assert!(!shown.canvas().rulers_shown());
        let without = shown.grab();
        // The desk, where the ruler's corner was.
        assert!(without.pixel((r / 2.0) as u32, 2).r < 120);
        assert_ne!(without.pixel((r / 2.0) as u32, 2), corner);

        // The rulers are not paper: a press on one leaves the selection alone.
        shown.canvas_mut().set_rulers_shown(true);
        shown.canvas_mut().select("vp1");
        let x = shown.at(250.0, 0.0).x;
        shown.click(PointF::new(x, r / 2.0), NONE);
        let y = shown.at(0.0, 250.0).y;
        shown.click(PointF::new(r / 2.0, y), NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
    }

    #[test]
    fn the_status_line_reads_the_paper_and_over_a_plan_the_world() {
        let mut document = Document::new(None);
        // A line due north through (1010, 2005).
        line(
            &mut document,
            Point2::new(1010.0, 1960.0),
            Point2::new(1010.0, 2050.0),
        );
        let mut plan = panel("vp1", bx(100.0, 100.0, 300.0, 250.0), ViewportKind::Plan);
        plan.scale = 500.0;
        plan.centre = Point2::new(1000.0, 2000.0);
        plan.rotation = std::f64::consts::PI / 6.0;
        let set = SheetSet {
            sheets: vec![Sheet {
                id: String::from("s1"),
                viewports: vec![plan.clone(), notes("vp2", bx(320.0, 100.0, 380.0, 180.0))],
                ..Sheet::default()
            }],
            ..SheetSet::default()
        };
        let mut shown = Shown::with_set(document, &set);
        shown.canvas_mut().zoom_to(&plan.rect);

        // Where the painter drew (1010, 2005) on the paper.
        let world = Point2::new(1010.0, 2005.0);
        let paper = plan_world_to_paper(&plan, 500.0, plan.centre, world);
        let widget = shown.canvas().paper_to_widget(paper);
        shown.move_to(widget, NONE);
        let readout = shown.canvas().cursor_readout().clone();
        assert!(readout.on_sheet);
        assert!(readout.on_paper);
        assert!(near(readout.paper.x, paper.x, 1e-9));
        assert!(near(readout.paper.y, paper.y, 1e-9));
        assert_eq!(readout.viewport_id, "vp1");
        assert_eq!(readout.kind, ViewportKind::Plan);
        assert_eq!(readout.scale, "1:500");
        let read = readout.world.expect("a world point");
        assert!(near(read.x, 1010.0, 1e-6));
        assert!(near(read.y, 2005.0, 1e-6));

        // And the line is there on the canvas: the readout names what is drawn.
        let shot = shown.grab();
        let around = |at: PointF| RectF::new(at.x - 3.0, at.y - 3.0, 6.0, 6.0);
        assert!(dark_in(&shot, around(widget), 160) > 0);
        // 10 m east of it, nothing.
        let east = shown.canvas().paper_to_widget(plan_world_to_paper(
            &plan,
            500.0,
            plan.centre,
            Point2::new(1020.0, 2005.0),
        ));
        assert_eq!(dark_in(&shot, around(east), 160), 0);

        // The status bar says so.
        shown.draw();
        let status = shown.control("sheetCursorStatus").expect("the cursor status");
        assert!(status.text.contains("vp1 Plan 1:500"), "{}", status.text);
        assert!(status.text.contains("E 1010.000  N 2005.000"), "{}", status.text);

        // Over a notes panel: its kind, but no world and no scale.
        let at = shown.at(350.0, 140.0);
        let over_notes = shown.editor.canvas().readout_at(shown.document(), at);
        assert_eq!(over_notes.viewport_id, "vp2");
        assert_eq!(over_notes.kind, ViewportKind::Notes);
        assert!(over_notes.world.is_none());
        assert!(over_notes.scale.is_empty());
        // Off the paper, it says so.
        let off = shown.at(-20.0, 100.0);
        assert!(!shown.editor.canvas().readout_at(shown.document(), off).on_paper);
        assert!(
            shown
                .editor
                .canvas()
                .readout_at(shown.document(), off)
                .viewport_id
                .is_empty()
        );

        // For a look: the page with the grid, both views selected, a band being drawn and the
        // cursor over the plan.
        if let Some(folder) = std::env::var("KATANA_SHEET_PNG").ok().filter(|folder| !folder.is_empty()) {
            shown.canvas_mut().fit_page();
            shown.canvas_mut().set_snap_to_grid(true);
            shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "");
            let (from, to) = (shown.at(330.0, 270.0), shown.at(250.0, 200.0));
            shown.drag_with(from, to, CTRL, MouseButton::Left, false);
            let at = shown.canvas().paper_to_widget(paper);
            shown.move_to(at, NONE);
            let image = shown.grab();
            let _ = write_png(
                &image,
                &std::path::Path::new(&folder).join("SheetEditorRotatedPlan.png"),
            );
            shown.key(SheetKey::Escape, NONE);
        }
    }

    #[test]
    fn an_automatic_plan_reads_the_world_at_the_scale_it_is_drawn_at() {
        let mut document = Document::new(None);
        line(&mut document, Point2::new(0.0, 0.0), Point2::new(300.0, 200.0));
        let mut plan = panel("vp1", bx(100.0, 100.0, 300.0, 250.0), ViewportKind::Plan);
        plan.auto_scale = true;
        plan.auto_centre = true;
        let set = SheetSet {
            sheets: vec![Sheet {
                id: String::from("s1"),
                viewports: vec![plan.clone()],
                ..Sheet::default()
            }],
            ..SheetSet::default()
        };
        let mut shown = Shown::with_set(document, &set);
        let drawn = resolve_plan_viewport(&plan, &shown.editor.source(shown.document()));
        let at = shown.at(200.0, 175.0);
        let middle = shown.editor.canvas().readout_at(shown.document(), at);
        let world = middle.world.expect("a world point");
        assert!(near(world.x, drawn.centre.x, 1e-6));
        assert!(near(world.y, drawn.centre.y, 1e-6));
        assert_eq!(middle.scale, format!("1:{}", qt_fixed(drawn.scale, 0)));
    }

    // ---- what a drag shows ----------------------------------------------------------------------

    #[test]
    fn a_dragged_view_carries_its_own_paint_with_it() {
        let mut shown = shown();
        shown.canvas_mut().zoom_to(&bx(90.0, 90.0, 210.0, 280.0));
        let from = bx(100.0, 100.0, 200.0, 180.0);
        // 90 mm up: empty paper above it.
        let to = bx(100.0, 190.0, 200.0, 270.0);
        let before = shown.grab();
        let ink_there = dark_in(&before, inside(shown.canvas(), &to, 5.0), 110);
        let ink_here = dark_in(&before, inside(shown.canvas(), &from, 5.0), 110);
        // "NOTES" and its text.
        assert!(ink_here > 20);

        let (a, b) = (shown.at(150.0, 140.0), shown.at(150.0, 230.0));
        shown.drag_with(a, b, ALT, MouseButton::Left, false);
        let during = shown.grab();
        if let Some(folder) = std::env::var("KATANA_SHEET_PNG").ok().filter(|folder| !folder.is_empty()) {
            let _ = write_png(
                &during,
                &std::path::Path::new(&folder).join("SheetEditorDragging.png"),
            );
        }
        // Its ink goes with it, and where it was is faded.
        assert!(dark_in(&during, inside(shown.canvas(), &to, 5.0), 110) > ink_there + ink_here / 2);
        assert!(dark_in(&during, inside(shown.canvas(), &from, 5.0), 110) < ink_here / 4);
        // Nothing is recorded until it is let go.
        let before2 = shown.steps();
        shown.release(b, MouseButton::Left);
        assert_eq!(shown.steps(), before2 + 1);
        assert!(near(shown.by_id(0, "vp1").rect.min.x, 100.0, 1e-6));
        assert!(near(shown.by_id(0, "vp1").rect.min.y, 190.0, 1e-6));
    }

    // ---- zooming to views -----------------------------------------------------------------------

    #[test]
    fn double_clicking_a_view_zooms_to_it_and_so_does_zoom_to_selection() {
        let mut shown = shown();
        let fitted = shown.canvas().zoom();
        let r = f64::from(SHEET_RULER_PIXELS);
        let (width, height) = (shown.canvas().width(), shown.canvas().height());
        let middle = PointF::new((width + r) / 2.0, (height + r) / 2.0);

        let at = shown.at(350.0, 140.0);
        shown.press(at, MouseButton::Left, NONE);
        shown.release(at, MouseButton::Left);
        shown.double_click(at);
        shown.release(at, MouseButton::Left);
        assert!(shown.canvas().zoom() > fitted * 2.0);
        assert_eq!(shown.selected(), "vp3");
        let centre = shown.at(350.0, 140.0);
        assert!(near(centre.x, middle.x, 1.0));
        assert!(near(centre.y, middle.y, 1.0));
        // Nothing was moved by the clicks.
        assert!(shown.sheet_set() == two_sheets());

        // Two views: both in the window, their middle in its middle.
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "");
        shown.trigger("sheetZoomSelection");
        let both = shown.at(200.0, 140.0);
        assert!(near(both.x, middle.x, 1.0));
        assert!(near(both.y, middle.y, 1.0));
        assert!(shown.at(100.0, 180.0).x >= r);
        assert!(shown.at(300.0, 100.0).x <= width);

        // Double-click the desk: the whole page again.
        shown.double_click(PointF::new(width - 3.0, height - 3.0));
        assert!(near(shown.canvas().zoom(), fitted, 1e-9));
    }

    // ---- the sheet list -------------------------------------------------------------------------

    /// qGray: (11 r + 16 g + 5 b) / 32.
    fn grey(c: Color) -> u32 {
        (u32::from(c.r) * 11 + u32::from(c.g) * 16 + u32::from(c.b) * 5) / 32
    }

    #[test]
    fn each_sheet_in_the_list_has_a_picture_of_it_painted_once() {
        let mut set = two_sheets();
        // The second sheet has no frame and one small panel: little ink.
        set.sheets[1].frame.clear();
        let mut shown = Shown::with_set(Document::new(None), &set);
        assert!(shown.has_control("sheetList"));
        shown.editor.render_thumbnails_now(&shown.host);
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 0);
        let painted = shown.editor.thumbnails().renders();

        // A picture of each sheet, the paper's shape, beside its name.
        let framed: Image = shown.editor.thumbnails().cached("s1");
        let bare: Image = shown.editor.thumbnails().cached("s2");
        assert!(!framed.is_null());
        assert!(!bare.is_null());
        if let Some(folder) = std::env::var("KATANA_SHEET_PNG").ok().filter(|folder| !folder.is_empty()) {
            let folder = std::path::Path::new(&folder);
            let _ = write_png(&framed, &folder.join("SheetThumbnailFramed.png"));
            let _ = write_png(&bare, &folder.join("SheetThumbnailBare.png"));
        }
        assert!(
            shown
                .editor
                .list_picture(0)
                .is_some_and(|picture| !picture.is_null())
        );
        let mut framed_ink = 0;
        let mut bare_ink = 0;
        let mut white = 0;
        for y in 0..framed.height() {
            for x in 0..framed.width() {
                // Anything drawn on the paper, however faint at this size.
                let f = framed.pixel(x, y);
                let b = bare.pixel(x, y);
                framed_ink += usize::from(grey(f) < 245 && f.a == 255);
                bare_ink += usize::from(grey(b) < 245 && b.a == 255);
                white += usize::from(f == Color::WHITE);
            }
        }
        // Paper.
        assert!(white > (framed.width() * framed.height() / 3) as usize);
        // Its frame and title block.
        assert!(framed_ink > bare_ink * 2);

        // Nothing changed: nothing to paint.
        shown.editor.render_thumbnails_now(&shown.host);
        assert_eq!(shown.editor.thumbnails().renders(), painted);
        // An edit of the second sheet: only its picture is painted again.
        edit_sheet(
            shown.document_mut(),
            1,
            |sheet| {
                sheet.name = String::from("RENAMED");
                Ok(())
            },
            "EDIT_SHEET",
        )
        .expect("renamed");
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 1);
        shown.editor.render_thumbnails_now(&shown.host);
        assert_eq!(shown.editor.thumbnails().renders(), painted + 1);
        // A change to the drawing leaves sheets of notes alone.
        line(shown.document_mut(), Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 0);
    }

    #[test]
    fn a_plans_picture_is_painted_again_when_the_drawing_changes() {
        let set = SheetSet {
            sheets: vec![Sheet {
                id: String::from("s1"),
                viewports: vec![panel(
                    "vp1",
                    bx(100.0, 100.0, 300.0, 250.0),
                    ViewportKind::Plan,
                )],
                ..Sheet::default()
            }],
            ..SheetSet::default()
        };
        let mut shown = Shown::with_set(Document::new(None), &set);
        shown.editor.render_thumbnails_now(&shown.host);
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 0);
        line(shown.document_mut(), Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 1);
    }

    fn three_sheets() -> SheetSet {
        let mut set = two_sheets();
        set.sheets.push(sheet_of("s3", "THIRD", Vec::new()));
        set
    }

    fn row_text(shown: &Shown, row: usize) -> String {
        shown
            .control("sheetList")
            .and_then(|list| list.items.get(row).cloned())
            .unwrap_or_default()
    }

    #[test]
    fn dropping_a_sheet_in_the_list_moves_it_in_one_step() {
        let mut shown = Shown::with_set(Document::new(None), &three_sheets());
        shown.draw();
        assert_eq!(shown.editor.list().count(), 3);
        let last = shown.editor.list().row_rect(2).expect("the last row");
        // On the lower half of the last row: after it.
        assert_eq!(shown.editor.list().insertion_row_at(last.max.y - 2.0), 3);
        assert_eq!(shown.editor.list().insertion_row_at(last.min.y + 2.0), 2);

        // FIRST dropped on itself, or just below itself: it stays, and nothing is asked for.
        shown.editor.set_current_sheet(&shown.host, 0);
        let before = shown.steps();
        let top = shown.editor.list().row_rect(0).expect("the first row");
        assert!(!shown.editor.list_mut().drop_at(top.min.y + 2.0));
        assert!(!shown.editor.list_mut().drop_at(top.max.y - 2.0));
        // FIRST dropped below THIRD.
        assert!(shown.editor.list_mut().drop_at(last.max.y - 2.0));
        // The move waits for the drop to unwind.
        assert_eq!(shown.steps(), before);
        shown.editor.poll(&mut shown.host);
        assert_eq!(shown.steps(), before + 1);
        let moved = shown.sheet_set();
        assert_eq!(moved.sheets.len(), 3);
        assert_eq!(moved.sheets[0].id, "s2");
        assert_eq!(moved.sheets[1].id, "s3");
        assert_eq!(moved.sheets[2].id, "s1");
        // The list is the set's order, the moved sheet shown.
        shown.draw();
        assert_eq!(shown.editor.list().count(), 3);
        assert!(row_text(&shown, 2).contains("FIRST"));
        assert_eq!(shown.editor.current_sheet(), 2);
        assert_eq!(shown.editor.list().current_row(), Some(2));

        // Undo puts the order back.
        shown.undo();
        assert_eq!(shown.sheet_set().sheets[0].id, "s1");
        shown.draw();
        assert!(row_text(&shown, 0).contains("FIRST"));

        // The API an agent calls; a sheet dropped where it was records nothing.
        shown
            .editor
            .move_sheet_to(&mut shown.host, 2, 0)
            .expect("moved");
        assert_eq!(shown.sheet_set().sheets[0].id, "s3");
        let after = shown.steps();
        shown
            .editor
            .move_sheet_to(&mut shown.host, 1, 1)
            .expect("stays");
        assert_eq!(shown.steps(), after);
        assert!(shown.editor.move_sheet_to(&mut shown.host, 5, 0).is_err());
    }

    #[test]
    fn page_up_and_page_down_step_through_the_sheets() {
        let mut shown = shown();
        assert_eq!(shown.editor.current_sheet(), 0);
        shown.trigger("sheetNextSheet");
        assert_eq!(shown.editor.current_sheet(), 1);
        // Past the last: stays.
        shown.trigger("sheetNextSheet");
        assert_eq!(shown.editor.current_sheet(), 1);
        shown.trigger("sheetPreviousSheet");
        assert_eq!(shown.editor.current_sheet(), 0);
        shown.trigger("sheetPreviousSheet");
        assert_eq!(shown.editor.current_sheet(), 0);
        let next = shown.action("sheetNextSheet").shortcut.expect("a key");
        assert_eq!((next.modifiers, next.logical_key), (Modifiers::NONE, Key::PageDown));
        let previous = shown.action("sheetPreviousSheet").shortcut.expect("a key");
        assert_eq!(
            (previous.modifiers, previous.logical_key),
            (Modifiers::NONE, Key::PageUp)
        );
    }

    #[test]
    fn every_editing_action_has_a_stable_name() {
        let mut shown = shown();
        for name in [
            "sheetCut",
            "sheetCopy",
            "sheetPaste",
            "sheetDuplicateViews",
            "sheetDeleteViews",
            "sheetSelectAll",
            "sheetSelectNextView",
            "sheetSelectPreviousView",
            "sheetZoomSelection",
            "sheetSnapGrid",
            "sheetShowRulers",
            "sheetPreviousSheet",
            "sheetNextSheet",
            "sheetFitPage",
        ] {
            assert!(shown.editor.action(&shown.host, name).is_some(), "{name}");
        }
        // And every action of the window and its toolbar has a name an agent can trigger it by:
        // the separators and the toolbar's buttons aside.
        let bar = shown.editor.tool_bar(&shown.host);
        assert_eq!(bar.name, "sheetToolBar");
        for action in shown.editor.actions(&shown.host) {
            assert!(!action.name.is_empty(), "{}", action.text);
        }
        assert!(
            shown
                .editor
                .menus(&shown.host)
                .iter()
                .any(|menu| menu.name == "sheetEditMenu")
        );
        shown.draw();
        assert!(shown.has_control("sheetCursorStatus"));
    }

    // ---- found in review ------------------------------------------------------------------------

    #[test]
    fn a_click_on_a_locked_member_of_a_group_selects_it_alone_and_a_drag_from_it_moves_the_rest() {
        let mut shown = shown();
        // vp3 is locked.
        shown.canvas_mut().set_selection(&ids(&["vp2", "vp3"]), "vp2");
        let before = shown.steps();
        let at = shown.at(350.0, 140.0);
        shown.click(at, NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp3"]));
        assert_eq!(shown.steps(), before);

        // Dragged from the locked one, the group moves without it, in one step.
        shown.canvas_mut().set_selection(&ids(&["vp2", "vp3"]), "vp2");
        let (from, to) = (shown.at(350.0, 140.0), shown.at(350.0, 120.0));
        shown.drag(from, to, ALT);
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.by_id(0, "vp3").rect, bx(320.0, 100.0, 380.0, 180.0));
        assert!(near(shown.by_id(0, "vp2").rect.min.y, 80.0, 1e-6));
        assert_eq!(shown.selected_ids(), ids(&["vp2", "vp3"]));

        // Only locked ones selected: a drag from one moves nothing and records nothing.
        edit_viewport(
            shown.document_mut(),
            "vp1",
            |v| {
                v.locked = true;
                Ok(())
            },
            "EDIT_VIEWPORT",
        )
        .expect("locked");
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp3"]), "vp1");
        let locked = shown.steps();
        let (from, to) = (shown.at(150.0, 140.0), shown.at(170.0, 120.0));
        shown.drag(from, to, NONE);
        assert_eq!(shown.steps(), locked);
        assert_eq!(shown.by_id(0, "vp1").rect, bx(100.0, 100.0, 200.0, 180.0));
        assert_eq!(shown.selected_ids(), ids(&["vp1", "vp3"]));
        // And a click on one of them selects it alone.
        let at = shown.at(150.0, 140.0);
        shown.click(at, NONE);
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
    }

    #[test]
    fn escape_in_a_rubber_band_leaves_the_selection_as_it_was() {
        let mut shown = shown();
        shown.canvas_mut().select("vp1");
        let (from, to) = (shown.at(260.0, 190.0), shown.at(90.0, 90.0));
        shown.drag_with(from, to, NONE, MouseButton::Left, false);
        shown.key(SheetKey::Escape, NONE);
        shown.release(to, MouseButton::Left);
        assert_eq!(shown.selected_ids(), ids(&["vp1"]));
    }

    #[test]
    fn a_drop_in_the_spacing_between_two_rows_goes_between_them() {
        let mut shown = Shown::with_set(Document::new(None), &three_sheets());
        shown.draw();
        let first = shown.editor.list().row_rect(0).expect("a first row");
        let second = shown.editor.list().row_rect(1).expect("a second row");
        // There is a gap.
        assert!(first.max.y < second.min.y);
        let gap = (first.max.y + second.min.y) / 2.0;
        assert_eq!(shown.editor.list().insertion_row_at(gap), 1);
        // FIRST dropped there stays; THIRD dropped there goes second.
        shown.editor.set_current_sheet(&shown.host, 0);
        assert!(!shown.editor.list_mut().drop_at(gap));
        shown.editor.set_current_sheet(&shown.host, 2);
        assert!(shown.editor.list_mut().drop_at(gap));
        shown.editor.poll(&mut shown.host);
        assert_eq!(shown.sheet_set().sheets[1].id, "s3");
        assert_eq!(shown.sheet_set().sheets[2].id, "s2");
    }

    #[test]
    fn a_picture_goes_stale_when_another_sheets_number_its_match_line_prints_changes() {
        let mut set = two_sheets();
        let mark = WorldMark {
            points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            label: String::from("MATCH LINE"),
            sheet: String::from("s2"),
            ..WorldMark::default()
        };
        set.sheets[0].viewports[0].marks.push(mark);
        let mut thumbnails = SheetThumbnails::default();
        let source = SheetSource::default();
        let _ = thumbnails.thumbnail(&set, 0, &source, 1.0);
        let _ = thumbnails.thumbnail(&set, 1, &source, 1.0);
        assert!(!thumbnails.is_stale(&set, 0, &source));
        // The sheet it leads to gets a number of its own: the label changes.
        set.sheets[1]
            .fields
            .insert(String::from("sheet_number"), String::from("C-101"));
        assert!(thumbnails.is_stale(&set, 0, &source));
    }

    /// A drawing register lists the other sheets, so renaming one of them changes its picture; a
    /// sheet of notes does not list them, so it keeps its picture.
    #[test]
    fn a_registers_picture_goes_stale_when_another_sheet_is_renamed() {
        let mut set = two_sheets();
        let table = Viewport {
            id: String::from("vp90"),
            kind: ViewportKind::SheetIndex,
            rect: bx(30.0, 40.0, 200.0, 150.0),
            ..Viewport::default()
        };
        set.sheets[0].viewports = vec![table.clone()];
        let notes_view = Viewport {
            id: String::from("vp91"),
            kind: ViewportKind::Notes,
            rect: table.rect,
            text: String::from("NOTES"),
            ..Viewport::default()
        };
        set.sheets[1].viewports = vec![notes_view];
        assert!(shows_the_other_sheets(&set.sheets[0]));
        assert!(!shows_the_other_sheets(&set.sheets[1]));
        let mut thumbnails = SheetThumbnails::default();
        let source = SheetSource::default();
        let _ = thumbnails.thumbnail(&set, 0, &source, 1.0);
        let _ = thumbnails.thumbnail(&set, 1, &source, 1.0);
        set.sheets[1].name = String::from("RENAMED");
        assert!(
            thumbnails.is_stale(&set, 0, &source),
            "the register lists the new name"
        );
        let _ = thumbnails.thumbnail(&set, 1, &source, 1.0);
        set.sheets[0].viewports[0].title = String::from("DRAWINGS");
        assert!(
            !thumbnails.is_stale(&set, 1, &source),
            "the notes list no other sheet"
        );
    }

    #[test]
    fn pictures_go_stale_on_undo_and_on_a_title_block_edit() {
        let mut shown = shown();
        shown.editor.render_thumbnails_now(&shown.host);
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 0);
        edit_sheet(
            shown.document_mut(),
            1,
            |sheet| {
                sheet.name = String::from("RENAMED");
                Ok(())
            },
            "EDIT_SHEET",
        )
        .expect("renamed");
        shown.editor.render_thumbnails_now(&shown.host);
        shown.undo();
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 1);
        shown.editor.render_thumbnails_now(&shown.host);
        // The title block every sheet shares: every picture.
        let mut set = shown.sheet_set();
        set.defaults.organisation = String::from("ACME SURVEYS");
        shown
            .document_mut()
            .set_sheet_set(&set, "EDIT_TITLE_BLOCK")
            .expect("edited");
        assert_eq!(shown.editor.pending_thumbnails(&shown.host), 2);
        // And a sheet removed takes its picture with it.
        set.sheets.pop();
        shown
            .document_mut()
            .set_sheet_set(&set, "REMOVE_SHEET")
            .expect("removed");
        shown.editor.poll(&mut shown.host);
        assert!(shown.editor.thumbnails().cached("s2").is_null());
    }

    #[test]
    fn page_up_and_page_down_are_left_to_the_boxes_of_the_properties() {
        let shown = shown();
        for name in ["sheetPreviousSheet", "sheetNextSheet"] {
            // Taken on the canvas and the list, not the window: there a spin box's PgUp would
            // switch sheets.
            match shown.editor.shortcut_scope(name) {
                ShortcutScope::Widgets(on) => {
                    assert!(on.contains(&"sheetCanvas"), "{name}");
                    assert!(on.contains(&"sheetList"), "{name}");
                }
                ShortcutScope::Window => panic!("{name} is the window's"),
            }
        }
    }

    #[test]
    fn the_note_of_a_group_is_short_enough_for_a_narrow_panel() {
        let mut shown = shown();
        shown.canvas_mut().set_selection(&ids(&["vp1", "vp2"]), "vp2");
        shown.draw();
        let note = shown.control("sheetSelectionNote").expect("the note");
        assert!(note.text.starts_with("2 views selected"));
        assert!(note.text.contains("vp2"));
        let metrics = TextMetrics::screen();
        for line in note.text.split('\n') {
            assert!(
                metrics.horizontal_advance(&Font::default(), line) <= 260.0,
                "{line}"
            );
        }
    }
}
