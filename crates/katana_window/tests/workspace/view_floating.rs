//! A floating view's rectangle (docs/desktop.md, "The dock chrome": "Where a floating view is").
//! Not twins: the C++'s floating docks were the platform's windows.
//!
//! egui_dock keeps a window's rectangle only while its own dock area draws the window, which the
//! workspace does not use, so every floating view was drawn at one place - 40, 40, 400 by 300 -
//! whatever it had been: two floated views lay on each other, Maximise on one filled nothing, and
//! a minimised one came back with no rectangle (docked, when its neighbours had changed). The
//! chrome keeps the rectangle now, in points from the workspace's top left, and each case here
//! fails without that: it asserts a rectangle that is not the one default.
//!
//! Every expected rectangle is worked from the docked place a view left (its far corner stays, its
//! near corner is a title bar's height down and across) or is one the test chose; the workspace is
//! drawn at the screen's top left, so a dock's rectangle on screen is its rectangle in the
//! workspace.

mod view_floating {
    use egui::{Modifiers, PointerButton, Rect, pos2, vec2};
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::ViewKind;
    use katana_window::dock_chrome::BAR_HEIGHT;
    use katana_window::view_workspace::{KIND_MENU, dock_name};

    use crate::support::Site;

    fn docked(w: &Site, id: ViewId) -> Rect {
        w.views
            .dock_rect(id)
            .unwrap_or_else(|| panic!("view {id} is docked"))
    }

    fn floats_at(w: &Site, id: ViewId) -> Rect {
        w.views
            .float_rect(id)
            .unwrap_or_else(|| panic!("view {id} floats"))
    }

    /// Where the last frame drew view `id`'s dock, bar and all.
    fn drawn(w: &Site, id: ViewId) -> Rect {
        w.harness
            .snapshot()
            .target(&dock_name(id))
            .unwrap_or_else(|| panic!("view {id} is drawn"))
            .rect
    }

    /// To within the half point a pixel snap may take.
    fn same(a: Rect, b: Rect) -> bool {
        (a.min - b.min).abs().max_elem() <= 0.5 && (a.max - b.max).abs().max_elem() <= 0.5
    }

    /// The plan the workspace starts with on the left, a 3D view over a section on the right.
    fn three_views() -> (Site, [ViewId; 3]) {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        (w, [plan, model, section])
    }

    #[test]
    fn floating_a_view_lifts_it_off_the_place_it_had_and_dock_puts_it_back_there() {
        let (mut w, [_plan, model, _section]) = three_views();
        let place = docked(&w, model);
        assert!(w.views.float_rect(model).is_none(), "a docked view has no window");

        w.press(model, "DockFloatButton");
        assert!(w.views.is_floating(model));
        // Over the place it left: its far corner where the docked view's was, its near corner a
        // title bar down and across, so the place shows behind it.
        let float = floats_at(&w, model);
        assert_eq!(float.min, place.min + vec2(BAR_HEIGHT, BAR_HEIGHT));
        assert_eq!(float.max, place.max);
        assert!(same(drawn(&w, model), float), "{:?}", drawn(&w, model));
        // Not the one place every floating view was once drawn at.
        assert_ne!(float.min, pos2(40.0, 40.0));

        w.press(model, "DockFloatButton");
        assert!(!w.views.is_floating(model));
        assert!(w.views.float_rect(model).is_none());
        assert_eq!(docked(&w, model), place, "back in the place it came from");
    }

    #[test]
    fn two_floated_views_each_stand_over_their_own_place_not_on_one_spot() {
        let (mut w, [plan, model, _section]) = three_views();
        let model_place = docked(&w, model);
        w.press(model, "DockFloatButton");
        // The plan's place once the 3D view has left the dock area.
        let plan_place = docked(&w, plan);
        w.press(plan, "DockFloatButton");

        let (model_at, plan_at) = (floats_at(&w, model), floats_at(&w, plan));
        assert_eq!(model_at.max, model_place.max);
        assert_eq!(plan_at.max, plan_place.max);
        assert_ne!(model_at, plan_at, "each has a rectangle of its own");
        assert!(same(drawn(&w, model), model_at));
        assert!(same(drawn(&w, plan), plan_at));
    }

    #[test]
    fn a_floating_view_dragged_by_its_title_bar_moves_with_the_pointer_and_keeps_its_size() {
        let (mut w, [_plan, model, _section]) = three_views();
        w.press(model, "DockFloatButton");
        let before = floats_at(&w, model);
        // On its title: past the kind switcher at its left, clear of the buttons at its right.
        let grab = pos2(before.min.x + 60.0, before.min.y + BAR_HEIGHT / 2.0);
        let by = vec2(40.0, 30.0);

        w.harness.pointer_move(grab);
        w.frame();
        w.harness
            .pointer_button(grab, PointerButton::Primary, true, Modifiers::NONE);
        w.frame();
        assert_eq!(floats_at(&w, model), before, "a press alone moves nothing");
        w.harness.pointer_move(grab + by / 2.0);
        w.frame();
        w.harness.pointer_move(grab + by);
        w.frame();
        assert_eq!(floats_at(&w, model), before.translate(by), "it follows the pointer");
        w.harness
            .pointer_button(grab + by, PointerButton::Primary, false, Modifiers::NONE);
        w.frame();

        // Let go, it stays; the pointer moving on moves it no further.
        w.harness.pointer_move(grab + by + vec2(90.0, 90.0));
        w.frame();
        let after = floats_at(&w, model);
        assert_eq!(after, before.translate(by));
        assert_eq!(after.size(), before.size());
        assert!(same(drawn(&w, model), after));
        assert!(w.views.is_floating(model), "a drag docks nothing");
    }

    #[test]
    fn a_floating_view_cannot_be_put_where_its_title_bar_is_out_of_reach() {
        let (mut w, [_plan, model, _section]) = three_views();
        w.press(model, "DockFloatButton");
        let size = floats_at(&w, model).size();
        let room = w.views.rect();

        // Far beyond the bottom right corner: its bar stays inside, 60 points of it across.
        assert!(w.views.set_float_rect(
            model,
            Rect::from_min_size(pos2(5000.0, 5000.0), size)
        ));
        let at = floats_at(&w, model);
        assert_eq!(at.size(), size);
        assert_eq!(at.min.x, room.max.x - 60.0);
        assert_eq!(at.min.y, room.max.y - BAR_HEIGHT);
        // And beyond the top left: its top at the workspace's, 60 points of its bar inside.
        assert!(w.views.set_float_rect(
            model,
            Rect::from_min_size(pos2(-5000.0, -5000.0), size)
        ));
        let at = floats_at(&w, model);
        assert_eq!(at.max.x, 60.0);
        assert_eq!(at.min.y, 0.0);

        // No place at all is refused, and so is a view that does not float.
        assert!(!w.views.set_float_rect(model, Rect::NOTHING));
        assert!(!w.views.set_float_rect(
            model,
            Rect::from_min_size(pos2(f32::NAN, 0.0), size)
        ));
        assert_eq!(floats_at(&w, model), at);
        let plan = w.views.view_ids()[0];
        assert!(!w.views.set_float_rect(plan, Rect::from_min_size(pos2(10.0, 10.0), size)));
    }

    #[test]
    fn a_maximised_floating_view_fills_the_workspace_and_restore_gives_back_its_rectangle() {
        let (mut w, [plan, model, section]) = three_views();
        w.press(model, "DockFloatButton");
        assert!(w.views.set_float_rect(
            model,
            Rect::from_min_size(pos2(120.0, 90.0), vec2(300.0, 220.0))
        ));
        w.frame();
        let before = floats_at(&w, model);
        assert_eq!(before, Rect::from_min_size(pos2(120.0, 90.0), vec2(300.0, 220.0)));
        let (plan_place, section_place) = (docked(&w, plan), docked(&w, section));

        w.press(model, "DockMaximiseButton");
        assert!(w.views.is_maximised(model));
        assert!(w.views.is_floating(model), "it fills the workspace as a window still");
        assert_eq!(floats_at(&w, model), w.views.rect());
        assert!(same(drawn(&w, model), w.views.rect()), "{:?}", drawn(&w, model));
        // A floating maximise hides no docked view: they are under it, where they were.
        assert_eq!(docked(&w, plan), plan_place);
        assert_eq!(docked(&w, section), section_place);
        assert!(w.views.on_screen(plan) && w.views.on_screen(section));
        // The same button, now Restore.
        let button = w
            .harness
            .snapshot()
            .control(&dock_name(model), "DockMaximiseButton")
            .expect("the floating view's Maximise button")
            .text
            .clone();
        assert_eq!(button, "Restore");

        w.press(model, "DockMaximiseButton");
        assert!(!w.views.is_maximised(model));
        assert_eq!(floats_at(&w, model), before, "the rectangle it had");
        assert!(same(drawn(&w, model), before));
    }

    #[test]
    fn a_maximised_floating_view_is_not_moved_by_its_title_bar() {
        let (mut w, [_plan, model, _section]) = three_views();
        w.press(model, "DockFloatButton");
        w.press(model, "DockMaximiseButton");
        let whole = w.views.rect();
        let grab = pos2(60.0, BAR_HEIGHT / 2.0);
        w.harness.pointer_move(grab);
        w.frame();
        w.harness
            .pointer_button(grab, PointerButton::Primary, true, Modifiers::NONE);
        w.frame();
        w.harness.pointer_move(grab + vec2(50.0, 50.0));
        w.frame();
        w.harness.pointer_button(
            grab + vec2(50.0, 50.0),
            PointerButton::Primary,
            false,
            Modifiers::NONE,
        );
        w.frame();
        assert_eq!(floats_at(&w, model), whole);
        assert!(!w.views.set_float_rect(
            model,
            Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 200.0))
        ));
    }

    #[test]
    fn a_minimised_floating_view_comes_back_from_the_tray_floating_at_the_rectangle_it_had() {
        let (mut w, [_plan, model, _section]) = three_views();
        w.press(model, "DockFloatButton");
        let place = Rect::from_min_size(pos2(150.0, 70.0), vec2(320.0, 240.0));
        assert!(w.views.set_float_rect(model, place));
        w.frame();

        w.press(model, "DockMinimiseButton");
        assert!(w.views.is_minimised(model));
        assert!(w.views.float_rect(model).is_none(), "in the tray it is nowhere");

        assert!(w.views.restore_view(model));
        w.frame();
        assert!(w.views.is_floating(model));
        assert_eq!(floats_at(&w, model), place);
        assert!(same(drawn(&w, model), place));
    }

    #[test]
    fn a_minimised_floating_view_comes_back_floating_after_the_others_were_tiled() {
        let (mut w, [plan, model, section]) = three_views();
        w.press(model, "DockFloatButton");
        let place = Rect::from_min_size(pos2(150.0, 70.0), vec2(320.0, 240.0));
        assert!(w.views.set_float_rect(model, place));
        w.press(model, "DockMinimiseButton");

        // The tiling leaves it in the tray, and arranges the two that show.
        assert_eq!(
            w.run("VIEWS TILE COLUMNS"),
            "tiled=2 kind=columns docked=0 minimised=1"
        );
        assert!(w.views.is_minimised(model));
        let (plan_tile, section_tile) = (docked(&w, plan), docked(&w, section));

        assert!(w.views.restore_view(model));
        w.frame();
        // Floating where it was, over the tiles as the tiling left them.
        assert!(w.views.is_floating(model));
        assert_eq!(floats_at(&w, model), place);
        assert_eq!(docked(&w, plan), plan_tile);
        assert_eq!(docked(&w, section), section_tile);
    }

    #[test]
    fn a_minimised_floating_view_comes_back_floating_when_a_neighbour_was_closed_meanwhile() {
        // The layout it was minimised from no longer fits the views that are open, so it cannot
        // be given back whole: the dock once came back in the first docked place there was.
        let (mut w, [plan, model, section]) = three_views();
        w.press(model, "DockFloatButton");
        let place = Rect::from_min_size(pos2(150.0, 70.0), vec2(320.0, 240.0));
        assert!(w.views.set_float_rect(model, place));
        w.press(model, "DockMinimiseButton");
        assert!(w.views.close_view(w.host.document_mut(), section).is_ok());
        w.frame();

        assert!(w.views.restore_view(model));
        w.frame();
        assert!(w.views.is_floating(model), "not docked beside the plan");
        assert_eq!(floats_at(&w, model), place);
        assert_eq!(docked(&w, plan), w.views.rect(), "the plan keeps the dock area");
    }

    #[test]
    fn a_floating_view_is_docked_into_a_tiling_and_keeps_no_window() {
        let (mut w, [_plan, model, _section]) = three_views();
        w.press(model, "DockFloatButton");
        assert_eq!(
            w.run("VIEWS TILE"),
            "tiled=3 kind=grid docked=1 minimised=0"
        );
        assert!(!w.views.is_floating(model));
        assert!(w.views.float_rect(model).is_none());
        // Floated again, it lifts off the tile it has now, not the rectangle it floated at before.
        let tile = docked(&w, model);
        w.press(model, "DockFloatButton");
        assert_eq!(floats_at(&w, model).max, tile.max);
    }

    #[test]
    fn a_floating_views_kind_switcher_opens_its_menu_under_its_own_bar() {
        // The menu was placed by the docked places alone, so a floating view's never opened.
        let (mut w, [_plan, model, _section]) = three_views();
        w.press(model, "DockFloatButton");
        let float = floats_at(&w, model);
        w.press(model, "ViewKindButton");
        let menu = w
            .harness
            .snapshot()
            .target(KIND_MENU)
            .expect("the kind menu is drawn")
            .rect;
        let expected = float.min + vec2(6.0, BAR_HEIGHT);
        assert!(
            (menu.min - expected).abs().max_elem() <= 1.0,
            "the menu is at {:?}, its bar's corner {expected:?}",
            menu.min
        );
    }
}

/// F9 and a floating view, in the real window. F9 is View > Toggle Perspective's key, and the
/// action reaches the 3D view the person used last (`ViewSet::most_recent`) wherever the active
/// view is - floating or docked. Triggered by name, as a `*` step and a click on the item do: the
/// window does not yet turn a key press into its action (docs/desktop.md, "The dock chrome", Not
/// done), so the key itself is not what these press.
mod floating_view_keys {
    use katana_cad::view_set::ViewId;
    use katana_render::Projection;
    use katana_window::MainWindow;
    use katana_window::drive::headless::Headless;
    use katana_window::drive::shortcut_text;

    /// The window with a 3D view opened beside its plan, drawn until idle: (plan, 3D).
    fn window_with_a_3d_view() -> (MainWindow, Headless, ViewId, ViewId) {
        let mut window = MainWindow::new();
        assert!(window.exec.run_command("VIEWS OPEN 3d"));
        let mut headless = Headless::new();
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        let ids = window.exec.workspace.view_ids();
        assert_eq!(ids.len(), 2);
        (window, headless, ids[0], ids[1])
    }

    fn projection(window: &MainWindow, id: ViewId) -> Projection {
        window
            .exec
            .workspace
            .state(id)
            .unwrap_or_else(|| panic!("view {id} is open"))
            .camera
            .projection()
    }

    #[test]
    fn toggle_perspective_is_f9_and_turns_a_floating_3d_view_orthographic_and_back() {
        let (mut window, mut headless, _plan, model) = window_with_a_3d_view();
        let action = window
            .composed()
            .action("viewTogglePerspective")
            .expect("View has Toggle Perspective");
        assert_eq!(
            action.shortcut.as_ref().map(shortcut_text).as_deref(),
            Some("F9")
        );
        window.exec.workspace.toggle_floating(model);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(window.exec.workspace.is_floating(model));
        // A 3D view starts in perspective (viewport_layout's configure_camera).
        assert_eq!(projection(&window, model), Projection::Perspective);

        window.trigger("viewTogglePerspective").expect("triggered");
        assert_eq!(projection(&window, model), Projection::Orthographic);
        assert!(window.exec.log.text().contains("Orthographic projection."));
        window.trigger("viewTogglePerspective").expect("triggered");
        assert_eq!(projection(&window, model), Projection::Perspective);
        assert!(window.exec.workspace.is_floating(model), "it floats still");
    }

    #[test]
    fn toggle_perspective_from_a_floating_plan_view_reaches_the_3d_view_used_last() {
        let (mut window, mut headless, plan, model) = window_with_a_3d_view();
        // Float makes its view the active one: the person is working in the floating plan.
        window.exec.workspace.toggle_floating(plan);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(window.exec.workspace.is_floating(plan));
        assert_eq!(window.exec.workspace.active_id(), plan);

        window.trigger("viewTogglePerspective").expect("triggered");
        assert_eq!(projection(&window, model), Projection::Orthographic);
        assert_eq!(
            window.exec.workspace.active_id(),
            plan,
            "the plan stays the active view"
        );
    }
}
