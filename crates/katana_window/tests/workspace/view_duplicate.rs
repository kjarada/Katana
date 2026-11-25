//! Duplicating a view (the owner's request of 2026-11-25 for "a button to duplicate the view and
//! tile the views"; docs/desktop.md, "Duplicating a view"). Not twins: the C++ window had no
//! such button.
//!
//! A view's bar has Duplicate and Tile among its tools. Duplicate hands the window's executor
//! one line, VIEWS DUPLICATE <id> TILE: a copy of the view - looking where it looks, hiding what
//! it hides - opened beside it and made active, then the open views tiled. Tile runs VIEWS TILE
//! GRID, as View > Viewport Layout > Tile does. The bar's shortcut menu has Duplicate View too.
//! Every expected place is worked from the workspace's 800 x 600 points and its separator, as
//! the tiling's cases work theirs.

mod view_duplicate {
    use egui::Rect;
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::ViewKind;
    use katana_geometry::Point2;
    use katana_window::view_workspace::{
        BAR_MENU, BAR_MENU_DUPLICATE, DUPLICATE_BUTTON, TILE_BUTTON,
    };

    use crate::support::Site;

    fn rect(w: &Site, id: ViewId) -> Rect {
        w.views
            .dock_rect(id)
            .unwrap_or_else(|| panic!("view {id} is docked"))
    }

    /// The plan the workspace starts with, centred on (10, 20) at 4 px a unit and held there, and
    /// hiding the layer "survey" of its own.
    fn a_plan_somewhere() -> (Site, ViewId) {
        let mut w = Site::new();
        let plan = w.active();
        w.add_layers(&["survey"]);
        w.run("VIEWS HIDE 1 survey");
        w.with_state(plan, |view| {
            view.plan.center = Point2::new(10.0, 20.0);
            view.plan.scale = 4.0;
            view.plan_framed = true;
        });
        w.views.with_plan_view(plan, |view, _| view.hold_view());
        w.frame();
        (w, plan)
    }

    #[test]
    fn the_duplicate_button_opens_a_copy_beside_the_view_and_tiles_the_views_by_one_line() {
        let (mut w, plan) = a_plan_somewhere();
        assert!(w.shown(plan, DUPLICATE_BUTTON));
        let undoable = w.document().history().undo_count();
        w.press(plan, DUPLICATE_BUTTON);

        // One line through the window's executor, logged as a typed one is.
        assert_eq!(
            w.ran().last().map(String::as_str),
            Some("VIEWS DUPLICATE 1 TILE")
        );
        assert_eq!(w.views.view_ids(), [plan, 2]);
        let copy = 2;
        assert_eq!(w.active(), copy, "the copy is the view being worked in");
        // Where the plan looks and what it hides; the copy is a plan view, Plan 2.
        let state = w.state(copy);
        assert_eq!(state.kind, ViewKind::Plan);
        assert_eq!(state.number, 2);
        assert_eq!(state.plan.center, Point2::new(10.0, 20.0));
        assert_eq!(state.plan.scale, 4.0);
        assert!(state.layers.hides_directly("survey"));
        // The plan looks where it did: the copy took half its room, not its zoom.
        assert_eq!(w.state(plan).plan.center, Point2::new(10.0, 20.0));
        assert_eq!(w.state(plan).plan.scale, 4.0);

        // Tiled: two views side by side, each (800 - 4) / 2 = 398 points wide and the whole
        // height, the active copy first.
        let gap = w.views.separator();
        let half = ((800.0 - gap) / 2.0_f32).floor();
        let (left, right) = (rect(&w, copy), rect(&w, plan));
        assert!((left.min.x - 0.0).abs() <= 1.0, "{left:?}");
        assert!((left.width() - half).abs() <= 1.0, "{left:?}");
        assert!((right.min.x - (half + gap)).abs() <= 1.0, "{right:?}");
        assert_eq!(left.height(), 600.0);
        assert_eq!(right.height(), 600.0);
        assert!(w.views.on_screen(copy) && w.views.on_screen(plan));
        // The views are the window's, not the drawing: no undo step.
        assert_eq!(w.document().history().undo_count(), undoable);
    }

    #[test]
    fn without_tile_the_copy_splits_the_place_of_the_view_it_copies_not_the_active_ones() {
        let (mut w, plan) = a_plan_somewhere();
        let model = w.open(ViewKind::Model3D);
        assert_eq!(w.active(), model);
        let (plan_was, model_was) = (rect(&w, plan), rect(&w, model));

        let reply = w.run("VIEWS DUPLICATE 1");
        assert!(reply.starts_with("duplicated=1 copy=3\n"), "{reply}");
        let copy = 3;
        assert_eq!(w.active(), copy);
        // The plan's place, 398 wide and 600 high, is cut along its longer side: the plan above,
        // the copy below it, each half its height less half the separator; the 3D view is where
        // it was.
        let (above, below) = (rect(&w, plan), rect(&w, copy));
        assert_eq!(rect(&w, model), model_was);
        assert_eq!(above.min, plan_was.min);
        assert_eq!(above.width(), plan_was.width());
        assert_eq!(below.width(), plan_was.width());
        assert_eq!(below.min.x, plan_was.min.x);
        assert!(below.min.y >= above.max.y, "{above:?} {below:?}");
        assert!(
            (above.height() + w.views.separator() + below.height() - plan_was.height()).abs()
                <= 1.0
        );
    }

    #[test]
    fn the_tile_button_tiles_the_open_views_by_view_viewport_layouts_line() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        assert!(w.shown(plan, TILE_BUTTON));
        assert!(w.tip(plan, TILE_BUTTON).contains("(VIEWS TILE)"));
        w.press(plan, TILE_BUTTON);
        assert_eq!(w.ran().last().map(String::as_str), Some("VIEWS TILE GRID"));
        // Each view in a place of its own, none overlapping, none opened or closed.
        assert_eq!(w.views.view_ids(), [plan, model, section]);
        let places: Vec<Rect> = [plan, model, section]
            .iter()
            .map(|&id| rect(&w, id))
            .collect();
        for (at, one) in places.iter().enumerate() {
            for other in &places[at + 1..] {
                assert!(!one.intersects(*other), "{one:?} and {other:?}");
            }
        }
    }

    #[test]
    fn the_bars_menu_has_duplicate_view_and_it_runs_the_buttons_line() {
        let (mut w, plan) = a_plan_somewhere();
        assert!(w.views.show_bar_menu(plan, None));
        w.frame();
        w.harness.target(BAR_MENU).press(BAR_MENU_DUPLICATE);
        w.frame();
        assert_eq!(
            w.ran().last().map(String::as_str),
            Some("VIEWS DUPLICATE 1 TILE")
        );
        assert_eq!(w.views.view_ids(), [plan, 2]);
        assert!(w.views.bar_menu_open().is_none());
    }

    #[test]
    fn duplicate_and_tile_are_the_first_tools_a_narrowing_bar_lets_go() {
        // Their priority is below every zoom tool's and the paper scale's: whenever Duplicate is
        // shown, every other optional tool is too. Swept across widths that reach both sides of
        // the rule - a bar with all of them, and one with the zoom tools and not these.
        let mut w = Site::new();
        let plan = w.active();
        let tip = w.tip(plan, DUPLICATE_BUTTON);
        assert!(tip.contains("(VIEWS DUPLICATE 1 TILE)"), "{tip}");
        let others = [
            "ViewScaleButton",
            "ViewZoomInButton",
            "ViewZoomOutButton",
            "ViewZoomSelectionButton",
        ];
        let (mut with_both, mut zoom_alone) = (false, false);
        for width in (250..=900).step_by(10) {
            w.resize(width as f32, 400.0);
            let duplicate = w.shown(plan, DUPLICATE_BUTTON);
            // One priority: they come and go together.
            assert_eq!(duplicate, w.shown(plan, TILE_BUTTON), "at {width}");
            let all_others = others.iter().all(|name| w.shown(plan, name));
            if duplicate {
                assert!(all_others, "at {width} Duplicate is shown before a zoom tool");
                with_both = true;
            } else if all_others {
                zoom_alone = true;
            }
        }
        assert!(with_both, "some width shows Duplicate");
        assert!(zoom_alone, "some width shows every zoom tool and not Duplicate");
    }
}
