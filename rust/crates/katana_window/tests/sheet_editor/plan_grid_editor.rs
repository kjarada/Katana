//! The twins of tests/qt_widgets/plotting/test_plan_grid_editor.cpp: a plan's coordinate grid
//! chosen in the sheet editor's properties, one step at a time; only a plan or key plan having
//! one; and a key plan added from the toolbar.
//!
//! The spacing box's "Auto" is SH1's grid_properties' (it shows 0 as "0.000"): the C++'s
//! expectation of "Auto" is kept as it was written.

mod sheet_grid_editor {
    use katana_cad::Document;
    use katana_cad::plotting::key_plan::key_plan_outlines;
    use katana_cad::plotting::plan_grid::{PlanPlacement, plan_world_to_paper, set_plan_grid};
    use katana_cad::plotting::sheet_commands::add_sheet;
    use katana_cad::plotting::sheet_set::{GridStyle, Sheet, Viewport, ViewportKind};
    use katana_core::ErrorCode;
    use katana_geometry::{Box2, Point2};
    use katana_window::sheet_painter::resolve_plan_viewport_on_sheet;

    use crate::support::{Shown, bx, double_eq, viewport};

    fn viewport_of(id: &str, kind: ViewportKind, rect: Box2, centre: Point2) -> Viewport {
        let mut made = viewport(id, kind, rect);
        made.scale = 1000.0;
        made.centre = centre;
        made
    }

    /// One sheet with a plan (vp1) and a notes panel (vp2), as one step.
    fn add_plan_sheet(document: &mut Document) {
        let sheet = Sheet {
            id: String::from("s1"),
            name: String::from("PLAN"),
            viewports: vec![
                viewport_of(
                    "vp1",
                    ViewportKind::Plan,
                    bx(30.0, 40.0, 250.0, 280.0),
                    Point2::default(),
                ),
                viewport_of(
                    "vp2",
                    ViewportKind::Notes,
                    bx(300.0, 100.0, 400.0, 200.0),
                    Point2::default(),
                ),
            ],
            ..Sheet::default()
        };
        add_sheet(document, sheet, None).expect("the sheet is added");
    }

    fn plan_of(shown: &Shown) -> Viewport {
        shown.document().sheet_set().sheets[0].viewports[0].clone()
    }

    #[test]
    fn a_plans_grid_is_chosen_in_its_properties_one_step_at_a_time() {
        let mut document = Document::new(None);
        add_plan_sheet(&mut document);
        let mut shown = Shown::new(document);
        shown.canvas_mut().select("vp1");
        shown.draw();
        let style = shown.control("sheetGridStyle").expect("the style");
        let interval = shown.control("sheetGridInterval").expect("the interval");
        // As stored: no grid, so no interval to set yet.
        assert_eq!(style.value, "None");
        assert!(!interval.enabled);
        // "Auto" is what 0 reads, and its tooltip what that comes to at 1:1000.
        assert_eq!(interval.value, "Auto");
        let tip = shown
            .editor
            .tooltip("sheetGridInterval")
            .map(String::from)
            .unwrap_or_default();
        assert!(tip.contains("50 m now"), "{tip}");
        let before = shown.steps();

        shown.fill("sheetGridStyle", "Lines");
        assert_eq!(plan_of(&shown).grid_style, GridStyle::Lines);
        assert_eq!(plan_of(&shown).grid_interval, 0.0);
        assert_eq!(shown.steps(), before + 1);

        // The panel was built again from the document: the new box is enabled.
        assert!(shown.control("sheetGridInterval").expect("the interval").enabled);
        shown.fill("sheetGridInterval", "25");
        assert_eq!(plan_of(&shown).grid_interval, 25.0);
        assert_eq!(plan_of(&shown).grid_style, GridStyle::Lines);
        assert_eq!(shown.steps(), before + 2);
        assert_eq!(shown.control("sheetGridStyle").expect("the style").value, "Lines");
        let shown_interval: f64 = shown
            .control("sheetGridInterval")
            .expect("the interval")
            .value
            .parse()
            .unwrap_or_default();
        assert!(double_eq(shown_interval, 25.0));

        // The canvas draws it, with nothing to report.
        shown.paint();
        assert!(shown.canvas().last_stats().problems.is_empty());

        shown.undo();
        assert_eq!(plan_of(&shown).grid_interval, 0.0);
        shown.undo();
        assert_eq!(plan_of(&shown).grid_style, GridStyle::None);
    }

    #[test]
    fn only_a_plan_or_a_key_plan_has_a_grid() {
        let mut document = Document::new(None);
        add_plan_sheet(&mut document);
        let mut shown = Shown::new(document);
        shown.canvas_mut().select("vp2");
        shown.draw();
        assert!(shown.control("sheetGridStyle").is_none());
        assert!(shown.control("sheetGridInterval").is_none());
        // What the list offers, an agent calls directly - and is told why not.
        let refused = set_plan_grid(shown.document_mut(), "vp2", GridStyle::Ticks, 0.0)
            .expect_err("refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_key_plan_added_from_the_toolbar_stores_no_outlines_and_shows_every_sheet() {
        let mut document = Document::new(None);
        // Two sheets of plans, 220 x 240 m apiece at 1:1000, side by side.
        for i in 0..2 {
            let sheet = Sheet {
                name: String::from("TILE"),
                viewports: vec![viewport_of(
                    "vp",
                    ViewportKind::Plan,
                    bx(30.0, 40.0, 250.0, 280.0),
                    Point2::new(1000.0 + 220.0 * f64::from(i), 2000.0),
                )],
                ..Sheet::default()
            };
            add_sheet(&mut document, sheet, None).expect("the sheet is added");
        }
        let mut shown = Shown::new(document);
        // A new sheet goes after the current one: the last.
        shown.editor.set_current_sheet(&shown.host, 1);
        shown
            .editor
            .add_blank_sheet(&mut shown.host)
            .expect("a blank sheet");
        assert_eq!(shown.editor.current_sheet(), 2);
        shown
            .editor
            .add_viewport(&mut shown.host, ViewportKind::KeyPlan)
            .expect("a key plan");
        let set = shown.sheet_set();
        assert_eq!(set.sheets.len(), 3);
        assert_eq!(set.sheets[2].viewports.len(), 1);
        let key = &set.sheets[2].viewports[0];
        assert_eq!(key.kind, ViewportKind::KeyPlan);
        assert!(key.marks.is_empty());
        assert!(key.auto_scale);
        assert!(key.auto_centre);

        // Fitted, as drawn, to both sheets' plans.
        let at = {
            let source = shown.editor.source(shown.document());
            resolve_plan_viewport_on_sheet(key, &source, &set, 2)
        };
        let outlines = key_plan_outlines(&set, 2, None);
        assert_eq!(outlines.len(), 2);
        assert_eq!(outlines[0].label, "1");
        assert_eq!(outlines[1].label, "2");
        let placement = PlanPlacement {
            scale: at.scale,
            centre: at.centre,
        };
        for outline in &outlines {
            assert!(!outline.current);
            for corner in &outline.corners {
                let paper = plan_world_to_paper(key, &placement, *corner);
                assert!(key.rect.contains(paper));
            }
        }
        shown.paint();
        assert!(shown.canvas().last_stats().problems.is_empty());
    }
}
