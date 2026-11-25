//! A floating view is a window of its own (the owner, 2026-11-25: "investigate why the view
//! cannot go outside the main menu"; docs/desktop.md, "A floating view is a window of its own").
//! Not twins: the C++'s floating dock was a Qt window, which the system placed.
//!
//! Where egui has windows to give - the desktop, `!Context::embed_viewports` - a floating view
//! is not drawn in the workspace at all: the main window shows it as an egui immediate viewport
//! and draws it there with `window_ui`. No test opens a window, so here the main window's
//! harness is told egui has windows, and a second harness stands in for the view's own: its
//! screen is the window's inside, and its place on the screen is what the desktop's egui reads
//! back from the system (`Harness::set_place`). What the view's window is asked to do - move,
//! size, maximise - is the commands its frame hands back.
//!
//! The main window stands at (100, 50) on the screen, its frame 8 points round it and 30 above,
//! so the workspace's top left - the corner a floating view's rectangle is kept from - is
//! (100, 50) on the screen.

mod view_windows {
    use egui::{Pos2, Rect, ViewportCommand, ViewportId, pos2, vec2};
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::ViewKind;
    use katana_window::dock_chrome::BAR_HEIGHT;
    use katana_window::drive::Harness;
    use katana_window::view_workspace::{BAR_MENU, Scenery, dock_name, view_viewport};

    use crate::support::Site;

    /// The workspace's top left on the screen.
    const ORIGIN: Pos2 = pos2(100.0, 50.0);
    /// A window's system frame: what its inside is in from its outside, left and top.
    const FRAME: egui::Vec2 = vec2(8.0, 30.0);

    /// The workspace in a main window egui has windows to give, at ORIGIN on the screen.
    fn on_the_desktop() -> Site {
        let mut w = Site::new();
        w.harness.context().set_embed_viewports(false);
        let inside = Rect::from_min_size(ORIGIN, vec2(800.0, 600.0));
        w.harness.set_place(Some(inside), Some(outside_of(inside)));
        w.frame();
        w
    }

    fn outside_of(inside: Rect) -> Rect {
        Rect::from_min_max(inside.min - FRAME, inside.max + vec2(FRAME.x, FRAME.x))
    }

    /// The plan, and a 3D view beside it floated: the view and the place it left.
    fn a_floated_view() -> (Site, ViewId, Rect) {
        let mut w = on_the_desktop();
        let model = w.open(ViewKind::Model3D);
        let place = w.views.dock_rect(model).expect("the 3D view is docked");
        w.views.toggle_floating(model);
        w.frame();
        (w, model, place)
    }

    /// The view's own window, sized as its record is and standing where the record says.
    fn its_window(w: &Site, id: ViewId) -> Harness {
        let record = w.views.float_rect(id).expect("the view floats");
        let mut window = Harness::with_screen(record.width(), record.height());
        let inside = record.translate(ORIGIN.to_vec2());
        window.set_place(Some(inside), Some(outside_of(inside)));
        window
    }

    /// One frame of view `id`'s window, as the main window draws it after listing its windows:
    /// the commands it handed the window.
    fn window_frame(w: &mut Site, window: &mut Harness, id: ViewId) -> Vec<ViewportCommand> {
        let listed = w.views.view_windows();
        assert!(
            listed.iter().any(|shown| shown.view == id),
            "view {id} has a window of its own"
        );
        let views = &w.views;
        let host = &mut w.host;
        let output = window.frame(|ui| views.window_ui(ui, id, host, Scenery::default()));
        output
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|viewport| viewport.commands.clone())
            .unwrap_or_default()
    }

    /// Frames of view `id`'s window until `target` is drawn and settled, as the main window's
    /// harness runs until idle: egui lays a popup out first in a sizing pass, where it is not yet
    /// where it goes, and the registry records it then as not settled. Its rectangle.
    fn settled_in_window(w: &mut Site, window: &mut Harness, id: ViewId, target: &str) -> Rect {
        for _ in 0..5 {
            let _ = window_frame(w, window, id);
            if let Some(record) = window.snapshot().target(target)
                && record.settled
            {
                return record.rect;
            }
        }
        panic!("{target} was never drawn settled in view {id}'s window")
    }

    fn moved(commands: &[ViewportCommand]) -> bool {
        commands.iter().any(|command| {
            matches!(
                command,
                ViewportCommand::OuterPosition(_) | ViewportCommand::InnerSize(_)
            )
        })
    }

    #[test]
    fn floated_on_the_desktop_a_view_is_a_window_of_its_own_over_the_place_it_left() {
        let (mut w, model, place) = a_floated_view();
        assert!(w.views.is_floating(model));
        assert!(w.views.is_windowed(model));
        // Not drawn in the main window at all.
        assert!(w.harness.snapshot().target(&dock_name(model)).is_none());
        // Listed for the main window to show: its viewport, its title, and opened over the place
        // it left, lifted a bar's height down and across (the float's rule), on the screen.
        let listed = w.views.view_windows();
        assert_eq!(listed.len(), 1);
        let shown = &listed[0];
        assert_eq!(shown.view, model);
        assert_eq!(shown.viewport, view_viewport(model));
        assert_eq!(shown.builder.title.as_deref(), Some("3D 1 - Katana"));
        let lift = vec2(BAR_HEIGHT, BAR_HEIGHT);
        assert_eq!(shown.builder.position, Some(ORIGIN + place.min.to_vec2() + lift));
        assert_eq!(shown.builder.inner_size, Some(place.size() - lift));
        // Drawn in its window, its bar and all.
        let mut window = its_window(&w, model);
        let commands = window_frame(&mut w, &mut window, model);
        assert!(window.snapshot().target(&dock_name(model)).is_some());
        assert!(window.control("DockFloatButton").is_some());
        // Standing where its record says, it is asked to go nowhere.
        assert!(!moved(&commands), "{commands:?}");
    }

    #[test]
    fn the_view_follows_its_window_wherever_it_is_put_off_the_main_window_too() {
        let (mut w, model, _) = a_floated_view();
        let mut window = its_window(&w, model);
        let _ = window_frame(&mut w, &mut window, model);
        // The person drags it onto a screen to the right of the main window and makes it larger.
        let inside = Rect::from_min_size(pos2(1500.0, 200.0), vec2(500.0, 400.0));
        window.set_place(Some(inside), Some(outside_of(inside)));
        let commands = window_frame(&mut w, &mut window, model);
        assert!(!moved(&commands), "it follows the window, not the other way: {commands:?}");
        // Kept from the workspace's corner, and nowhere near the workspace: a window of its own is
        // not brought back into reach as one drawn inside is.
        let expected = Rect::from_min_size(pos2(1400.0, 150.0), vec2(500.0, 400.0));
        assert_eq!(w.views.float_rect(model), Some(expected));
        w.frame();
        assert_eq!(w.views.float_rect(model), Some(expected));
        // And the next frame of the window, standing there, changes nothing.
        let commands = window_frame(&mut w, &mut window, model);
        assert!(!moved(&commands));
        assert_eq!(w.views.float_rect(model), Some(expected));
    }

    #[test]
    fn a_view_window_put_somewhere_by_a_drive_is_moved_there_frame_and_all() {
        let (mut w, model, _) = a_floated_view();
        let mut window = its_window(&w, model);
        let _ = window_frame(&mut w, &mut window, model);
        // Off the workspace's left edge, which a view drawn inside would be kept from.
        let wanted = Rect::from_min_size(pos2(-150.0, 20.0), vec2(300.0, 200.0));
        assert!(w.views.set_float_rect(model, wanted));
        assert_eq!(w.views.float_rect(model), Some(wanted));
        let commands = window_frame(&mut w, &mut window, model);
        // Its inside the size wanted, and its outside's corner the frame up and left of where the
        // inside's goes: (100 - 150 - 8, 50 + 20 - 30).
        assert!(commands.contains(&ViewportCommand::InnerSize(vec2(300.0, 200.0))), "{commands:?}");
        assert!(
            commands.contains(&ViewportCommand::OuterPosition(pos2(-58.0, 40.0))),
            "{commands:?}"
        );
        // The window lags a frame behind the command, then arrives; the record ends where it was
        // put, and nothing is asked again.
        let _ = window_frame(&mut w, &mut window, model);
        let inside = wanted.translate(ORIGIN.to_vec2());
        window.set_place(Some(inside), Some(outside_of(inside)));
        let _ = window_frame(&mut w, &mut window, model);
        let commands = window_frame(&mut w, &mut window, model);
        assert!(!moved(&commands), "{commands:?}");
        assert_eq!(w.views.float_rect(model), Some(wanted));
    }

    #[test]
    fn maximise_on_a_view_windows_bar_maximises_the_window_and_restore_restores_it() {
        let (mut w, model, _) = a_floated_view();
        let before = w.views.float_rect(model).expect("it floats");
        let mut window = its_window(&w, model);
        let _ = window_frame(&mut w, &mut window, model);

        window.target(&dock_name(model)).press("DockMaximiseButton");
        let mut commands = window_frame(&mut w, &mut window, model);
        commands.extend(window_frame(&mut w, &mut window, model));
        assert!(w.views.is_maximised(model));
        assert!(
            commands.contains(&ViewportCommand::Maximized(true)),
            "{commands:?}"
        );
        assert!(!moved(&commands), "the system maximises it: {commands:?}");

        window.target(&dock_name(model)).press("DockMaximiseButton");
        let mut commands = window_frame(&mut w, &mut window, model);
        commands.extend(window_frame(&mut w, &mut window, model));
        assert!(!w.views.is_maximised(model));
        assert!(
            commands.contains(&ViewportCommand::Maximized(false)),
            "{commands:?}"
        );
        assert!(!moved(&commands), "the system restores it: {commands:?}");
        assert_eq!(w.views.float_rect(model), Some(before));
    }

    #[test]
    fn the_systems_close_button_docks_the_view_back_and_closes_no_view() {
        let (mut w, model, place) = a_floated_view();
        let mut window = its_window(&w, model);
        let _ = window_frame(&mut w, &mut window, model);
        window.press_close();
        let _ = window_frame(&mut w, &mut window, model);
        w.frame();
        assert!(w.views.view_ids().contains(&model), "the view is still open");
        assert!(!w.views.is_floating(model));
        assert!(!w.views.is_windowed(model));
        assert_eq!(w.views.dock_rect(model), Some(place), "back where it came from");
        assert!(w.views.view_windows().is_empty());
        // Drawn in the main window again.
        assert!(w.harness.snapshot().target(&dock_name(model)).is_some());
    }

    #[test]
    fn dock_on_the_view_windows_bar_puts_it_back_where_it_came_from() {
        let (mut w, model, place) = a_floated_view();
        let mut window = its_window(&w, model);
        let _ = window_frame(&mut w, &mut window, model);
        window.target(&dock_name(model)).press("DockFloatButton");
        let _ = window_frame(&mut w, &mut window, model);
        w.frame();
        assert!(!w.views.is_floating(model));
        assert_eq!(w.views.dock_rect(model), Some(place));
        assert!(w.views.view_windows().is_empty());
    }

    #[test]
    fn a_view_windows_menus_and_layers_popup_open_in_its_window_not_the_main_one() {
        let (mut w, model, _) = a_floated_view();
        let mut window = its_window(&w, model);
        let _ = window_frame(&mut w, &mut window, model);

        // The bar's menu: under its bar's left end, in the window's own points.
        assert!(w.views.show_bar_menu(model, None));
        let menu = settled_in_window(&mut w, &mut window, model, BAR_MENU);
        assert!(
            (menu.min - pos2(6.0, BAR_HEIGHT)).abs().max_elem() <= 1.0,
            "{menu:?}"
        );
        w.frame();
        assert!(w.harness.snapshot().target(BAR_MENU).is_none());
        assert_eq!(w.views.bar_menu_open(), Some(model), "drawn elsewhere, still open");

        // The Layers popup: hanging from under the bar's right end, in the window.
        assert!(w.views.show_layers_popup(w.host.document(), None, model));
        w.frame();
        assert!(w.harness.snapshot().target("ViewLayersPopup").is_none());
        let popup = settled_in_window(&mut w, &mut window, model, "ViewLayersPopup");
        let width = w.views.float_rect(model).expect("it floats").width();
        assert!(popup.max.x <= width + 1.0, "{popup:?} in a window {width} wide");
        assert!(popup.min.y >= BAR_HEIGHT - 1.0, "{popup:?}");
    }

    #[test]
    fn headless_a_floating_view_is_drawn_in_the_workspace_and_has_no_window() {
        // egui with no windows to give - the headless runner, every other case - embeds a
        // viewport, and the workspace draws its floating views itself, as it always has.
        let mut w = Site::new();
        let model = w.open(ViewKind::Model3D);
        w.views.toggle_floating(model);
        w.frame();
        assert!(w.views.is_floating(model));
        assert!(!w.views.is_windowed(model));
        assert!(w.views.view_windows().is_empty());
        assert!(w.harness.snapshot().target(&dock_name(model)).is_some());
    }
}
