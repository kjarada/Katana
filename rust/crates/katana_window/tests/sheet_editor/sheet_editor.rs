//! The twins of tests/qt_widgets/test_sheet_editor.cpp: what a click, a drag and the toolbar do to
//! the project's sheets, and that each is ONE undoable step.

mod sheet_editor {
    use katana_cad::Document;
    use katana_cad::plotting::layout::{TilingPreset, tiling_area};
    use katana_cad::plotting::sheet_commands::{add_sheet, edit_viewport};
    use katana_cad::plotting::sheet_set::{Sheet, ViewportKind, drawing_area};
    use katana_geometry::Box2;
    use katana_window::canvas::write_png;

    use crate::support::{ALT, NONE, SHIFT, Shown, bx, double_eq, near, viewport};

    /// A sheet with one viewport of `kind` at `rect`, as one step.
    fn add_sheet_with(document: &mut Document, kind: ViewportKind, rect: Box2) {
        let mut one = viewport("vp1", kind, rect);
        one.scale = 500.0;
        let sheet = Sheet {
            id: String::from("s1"),
            name: String::from("TEST"),
            viewports: vec![one],
            ..Sheet::default()
        };
        add_sheet(document, sheet, None).expect("the sheet is added");
    }

    fn only_rect(shown: &Shown) -> Box2 {
        shown.document().sheet_set().sheets[0].viewports[0].rect
    }

    #[test]
    fn adding_a_view_is_one_step_and_undo_takes_it_away() {
        let mut shown = Shown::new(Document::new(None));
        shown
            .editor
            .add_viewport(&mut shown.host, ViewportKind::Plan)
            .expect("a plan is added");
        // A sheet first, since there was none, then the view on it.
        let set = shown.sheet_set();
        assert_eq!(set.sheets.len(), 1);
        assert_eq!(set.sheets[0].viewports.len(), 1);
        let plan = set.sheets[0].viewports[0].clone();
        assert!(plan.auto_scale);
        assert!(plan.north_arrow);
        assert_eq!(shown.selected(), plan.id);
        // On a sheet with nothing on it, a view fills the tiling area.
        assert_eq!(plan.rect, tiling_area(&set.sheets[0]));

        shown.undo();
        assert!(shown.sheet_set().sheets[0].viewports.is_empty());
        assert!(shown.selected().is_empty());
    }

    #[test]
    fn dragging_a_view_moves_it_in_one_step() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, ViewportKind::Notes, bx(100.0, 100.0, 200.0, 180.0));
        let mut shown = Shown::new(document);
        let before = shown.steps();

        let (from, to) = (shown.at(150.0, 140.0), shown.at(180.0, 150.0));
        shown.drag(from, to, NONE);
        assert_eq!(shown.steps(), before + 1);
        let moved = only_rect(&shown);
        assert!(near(moved.min.x, 130.0, 1e-6));
        assert!(near(moved.min.y, 110.0, 1e-6));
        assert!(near(moved.width(), 100.0, 1e-9));

        shown.undo();
        assert_eq!(only_rect(&shown), bx(100.0, 100.0, 200.0, 180.0));
    }

    #[test]
    fn a_dragged_view_snaps_to_the_drawing_area_unless_alt_is_held() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, ViewportKind::Notes, bx(100.0, 100.0, 200.0, 180.0));
        let mut shown = Shown::new(document);
        let drawing_left = drawing_area(&shown.sheet_set().sheets[0]).min.x;
        // Within a snap's reach (8 px) of the drawing area's left edge.
        let close = drawing_left + 3.0 / shown.canvas().zoom();

        let (from, to) = (shown.at(150.0, 140.0), shown.at(150.0 - (100.0 - close), 140.0));
        shown.drag(from, to, NONE);
        assert!(double_eq(only_rect(&shown).min.x, drawing_left));

        shown.undo();
        let (from, to) = (shown.at(150.0, 140.0), shown.at(150.0 - (100.0 - close), 140.0));
        shown.drag(from, to, ALT);
        assert!(near(only_rect(&shown).min.x, close, 1e-6));
    }

    #[test]
    fn a_handle_resizes_one_edge() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, ViewportKind::Notes, bx(100.0, 100.0, 200.0, 180.0));
        let mut shown = Shown::new(document);
        shown.canvas_mut().select("vp1");
        // The right edge's handle, dragged 40 mm right.
        let (from, to) = (shown.at(200.0, 140.0), shown.at(240.0, 140.0));
        shown.drag(from, to, ALT);
        let r = only_rect(&shown);
        assert!(near(r.max.x, 240.0, 1e-6));
        assert!(double_eq(r.min.x, 100.0));
        assert!(double_eq(r.min.y, 100.0));
        assert!(double_eq(r.max.y, 180.0));
    }

    #[test]
    fn shift_drag_pans_the_drawing_inside_a_plan() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, ViewportKind::Plan, bx(100.0, 100.0, 300.0, 250.0));
        let mut shown = Shown::new(document);
        let before = shown.steps();
        // 20 mm right on the paper at 1 : 500 is 10 m; the drawing follows the hand, so the centre
        // moves 10 m west.
        let (from, to) = (shown.at(200.0, 175.0), shown.at(220.0, 175.0));
        shown.drag(from, to, SHIFT);
        assert_eq!(shown.steps(), before + 1);
        let plan = shown.sheet_set().sheets[0].viewports[0].clone();
        assert!(near(plan.centre.x, -10.0, 1e-6));
        assert!(near(plan.centre.y, 0.0, 1e-6));
        assert_eq!(plan.rect, bx(100.0, 100.0, 300.0, 250.0));
    }

    #[test]
    fn tiling_puts_the_main_view_in_the_largest_cell() {
        let mut shown = Shown::new(Document::new(None));
        shown
            .editor
            .add_viewport(&mut shown.host, ViewportKind::Legend)
            .expect("a legend is added");
        shown
            .editor
            .add_viewport(&mut shown.host, ViewportKind::Plan)
            .expect("a plan is added");
        let before = shown.steps();
        shown
            .editor
            .tile(&mut shown.host, TilingPreset::MainRight)
            .expect("tiled");
        assert_eq!(shown.steps(), before + 1);
        let set = shown.sheet_set();
        let views = &set.sheets[0].viewports;
        let legend = if views[0].kind == ViewportKind::Legend {
            &views[0]
        } else {
            &views[1]
        };
        let plan = if views[0].kind == ViewportKind::Plan {
            &views[0]
        } else {
            &views[1]
        };
        assert!(plan.rect.width() > legend.rect.width());
        let area = tiling_area(&set.sheets[0]);
        assert!(area.contains_box(plan.rect));
        assert!(area.contains_box(legend.rect));
    }

    #[test]
    fn delete_removes_the_selected_view() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, ViewportKind::Notes, bx(100.0, 100.0, 200.0, 180.0));
        let mut shown = Shown::new(document);
        shown.canvas_mut().select("vp1");
        shown
            .editor
            .remove_selected_viewport(&mut shown.host)
            .expect("removed");
        assert!(shown.sheet_set().sheets[0].viewports.is_empty());
        assert!(shown.selected().is_empty());
    }

    #[test]
    fn the_canvas_paints_once_and_not_again_for_a_selection() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, ViewportKind::Notes, bx(100.0, 100.0, 200.0, 180.0));
        let mut shown = Shown::new(document);
        shown.paint();
        let renders = shown.canvas().renders();
        // Picking a viewport draws its handles over the last paint.
        shown.canvas_mut().select("vp1");
        shown.paint();
        assert_eq!(shown.canvas().renders(), renders);
        // A change to the sheet paints it afresh.
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
        shown.paint();
        assert_eq!(shown.canvas().renders(), renders + 1);

        if let Ok(folder) = std::env::var("KATANA_SHEET_PNG") {
            let image = shown.grab();
            let _ = write_png(
                &image,
                &std::path::Path::new(&folder).join("SheetEditor.png"),
            );
        }
    }
}
