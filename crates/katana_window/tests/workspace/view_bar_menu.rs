//! A view's title bar shortcut menu (docs/desktop.md, "A view's title bar menu"). Not twins: the
//! C++ window had none.
//!
//! A right-click on a view's title bar opens a menu of what its window buttons do - Float or
//! Dock, Minimise, Maximise or Restore, Close - and the three tilings. It has no command of its
//! own: a button's item goes the way the button goes, and a tiling's item runs the VIEWS TILE
//! line through the window's executor, as View > Viewport Layout's item does.

mod view_bar_menu {
    use egui::{Modifiers, PointerButton, pos2};
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::ViewKind;
    use katana_window::MainWindow;
    use katana_window::dock_chrome::BAR_HEIGHT;
    use katana_window::drive::registry::ControlKind;
    use katana_window::drive::steps::find_menu;
    use katana_window::drive::{Action, MenuItem};
    use katana_window::view_workspace::{
        BAR_MENU, BAR_MENU_CLOSE, BAR_MENU_FLOAT, BAR_MENU_MAXIMISE, BAR_MENU_MINIMISE, Scenery,
        tiling_actions,
    };

    use crate::support::Site;

    const ITEMS: [&str; 8] = [
        "viewBarFloat",
        "viewBarMinimise",
        "viewBarMaximise",
        "viewBarClose",
        "viewTile",
        "viewTileSideBySide",
        "viewTileStacked",
        "viewBarDuplicate",
    ];

    /// The plan the workspace starts with, a 3D view and a section, the section active.
    fn three_views() -> (Site, [ViewId; 3]) {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        (w, [plan, model, section])
    }

    /// The menu's items as the last frame drew them, by name.
    fn drawn_items(w: &Site) -> Vec<String> {
        w.harness
            .snapshot()
            .in_target(BAR_MENU)
            .filter(|control| control.kind == ControlKind::MenuItem)
            .map(|control| control.name.clone())
            .collect()
    }

    fn menu_is_drawn(w: &Site) -> bool {
        w.harness.snapshot().target(BAR_MENU).is_some()
    }

    /// View `id`'s menu opened as a run with no pointer opens it, and `item` pressed in it.
    fn choose(w: &mut Site, id: ViewId, item: &str) {
        assert!(w.views.show_bar_menu(id, None), "view {id} has a bar menu");
        w.frame();
        w.harness.target(BAR_MENU).press(item);
        w.frame();
    }

    /// The text view `id`'s menu shows for `item` now.
    fn text_of(w: &Site, id: ViewId, item: &str) -> String {
        let menu = w
            .views
            .bar_menu(id)
            .unwrap_or_else(|| panic!("view {id} has a bar menu"));
        menu.actions()
            .into_iter()
            .find(|action| action.name == item)
            .unwrap_or_else(|| panic!("the menu has {item}"))
            .text
            .clone()
    }

    #[test]
    fn a_right_click_on_a_views_title_bar_opens_its_menu_there_and_makes_it_the_active_view() {
        let (mut w, [plan, _model, section]) = three_views();
        assert_eq!(w.active(), section);
        assert!(!menu_is_drawn(&w));
        // On the plan's title: past the kind switcher at its left, clear of its buttons.
        let place = w.views.dock_rect(plan).expect("the plan is docked");
        let at = pos2(place.min.x + 60.0, place.min.y + BAR_HEIGHT / 2.0);
        w.harness.pointer_move(at);
        w.frame();
        w.harness
            .pointer_button(at, PointerButton::Secondary, true, Modifiers::NONE);
        w.frame();
        assert!(w.views.bar_menu_open().is_none(), "it opens when the button is let go");
        w.harness
            .pointer_button(at, PointerButton::Secondary, false, Modifiers::NONE);
        w.frame();

        assert_eq!(w.views.bar_menu_open(), Some(plan));
        assert_eq!(w.active(), plan, "the menu acts on the view it was opened on");
        let menu = w
            .harness
            .snapshot()
            .target(BAR_MENU)
            .expect("the menu is drawn")
            .rect;
        assert!(
            (menu.min - at).abs().max_elem() <= 1.0,
            "the menu is at {:?}, the pointer at {at:?}",
            menu.min
        );
        assert_eq!(drawn_items(&w), ITEMS);

        // A click outside it closes it and does nothing.
        let away = pos2(700.0, 500.0);
        assert!(!menu.contains(away));
        let views = w.views.view_ids();
        w.harness.pointer_move(away);
        w.frame();
        w.harness
            .pointer_button(away, PointerButton::Primary, true, Modifiers::NONE);
        w.frame();
        w.harness
            .pointer_button(away, PointerButton::Primary, false, Modifiers::NONE);
        w.frame();
        assert!(w.views.bar_menu_open().is_none());
        assert!(!menu_is_drawn(&w));
        assert_eq!(w.views.view_ids(), views);
    }

    #[test]
    fn a_right_click_on_a_bars_button_opens_no_menu() {
        let (mut w, [plan, ..]) = three_views();
        let at = w
            .harness
            .snapshot()
            .control(&katana_window::view_workspace::dock_name(plan), "DockMinimiseButton")
            .expect("the plan's Minimise button")
            .rect
            .center();
        w.harness.pointer_move(at);
        w.frame();
        w.harness
            .pointer_button(at, PointerButton::Secondary, true, Modifiers::NONE);
        w.frame();
        w.harness
            .pointer_button(at, PointerButton::Secondary, false, Modifiers::NONE);
        w.frame();
        assert!(w.views.bar_menu_open().is_none());
        assert!(!w.views.is_minimised(plan), "and a right-click presses no button");
    }

    #[test]
    fn the_menu_words_its_items_as_the_bars_buttons_are_worded_for_the_views_state() {
        let (mut w, [plan, ..]) = three_views();
        let menu = w.views.bar_menu(plan).expect("the plan has a bar menu");
        assert_eq!(menu.name, BAR_MENU);
        assert_eq!(menu.title, "Plan 1");
        let rows: Vec<String> = menu
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Section(title) => format!("[{title}]"),
                MenuItem::Action(action) => format!("{}={}", action.name, action.text),
                MenuItem::Menu(sub) => sub.name.clone(),
                MenuItem::Separator => String::from("-"),
            })
            .collect();
        assert_eq!(
            rows,
            [
                "[This view]",
                "viewBarFloat=Float",
                "viewBarMinimise=Minimise",
                "viewBarMaximise=Maximise",
                "viewBarClose=Close",
                "[Arrange the open views]",
                "viewTile=Tile",
                "viewTileSideBySide=Tile Side by Side",
                "viewTileStacked=Tile Stacked",
                "viewBarDuplicate=Duplicate View",
            ]
        );
        // Every item has an icon and says what it does; no two of the view's own share an icon.
        let actions = menu.actions();
        for action in &actions {
            assert!(!action.icon.is_empty(), "{} has an icon", action.name);
            assert!(!action.tip.is_empty(), "{} has a tip", action.name);
            assert!(action.enabled, "{}", action.name);
        }
        let icons: Vec<&str> = actions.iter().take(4).map(|action| action.icon).collect();
        assert_eq!(icons, ["Float", "Minimise", "Maximise", "Close"]);
        // The words are the bar's own buttons': one sentence for the button and its item.
        let bar = w.bar(plan);
        let float = bar.window_button(
            katana_window::dock_chrome::WindowButton::Float,
            katana_window::dock_chrome::BarFacts::default(),
        );
        assert_eq!(actions[0].text, float.label);
        assert_eq!(actions[0].tip, float.tip);

        // Floating, its first item is Dock; maximised, its third is Restore.
        w.press(plan, "DockFloatButton");
        assert_eq!(text_of(&w, plan, BAR_MENU_FLOAT), "Dock");
        w.press(plan, "DockMaximiseButton");
        assert_eq!(text_of(&w, plan, BAR_MENU_MAXIMISE), "Restore");
        let floating = w.views.bar_menu(plan).expect("the plan has a bar menu");
        let icons: Vec<&str> = floating
            .actions()
            .iter()
            .take(4)
            .map(|action| action.icon)
            .collect();
        assert_eq!(icons, ["Dock", "Minimise", "Restore", "Close"]);
    }

    #[test]
    fn the_menus_tilings_are_the_view_menus_and_the_window_menus_own_actions() {
        // One action, three places: the same name, text, icon and tip wherever it is shown.
        let w = Site::new();
        let plan = w.active();
        let menu = w.views.bar_menu(plan).expect("the plan has a bar menu");
        // The tilings by their names, as the two menus below are read: the bar's section holds
        // Duplicate View after them (the owner's request of 2026-11-25), which is no tiling.
        let in_bar: Vec<Action> = menu
            .actions()
            .into_iter()
            .filter(|action| action.name.starts_with("viewTile"))
            .cloned()
            .collect();
        assert_eq!(in_bar, tiling_actions());

        let window = MainWindow::new();
        let composed = window.composed();
        for name in ["viewLayoutMenu", "windowMenu"] {
            let listed: Vec<Action> = find_menu(&composed.menus, name)
                .unwrap_or_else(|| panic!("the window has {name}"))
                .actions()
                .into_iter()
                .filter(|action| action.name.starts_with("viewTile"))
                .cloned()
                .collect();
            assert_eq!(listed, tiling_actions(), "{name}");
        }
    }

    #[test]
    fn float_minimise_maximise_and_close_in_the_menu_do_what_the_bars_buttons_do() {
        let (mut w, [plan, model, section]) = three_views();
        let place = w.views.dock_rect(model).expect("the 3D view is docked");

        // Float, then the same item, now Dock.
        choose(&mut w, model, BAR_MENU_FLOAT);
        assert!(w.views.is_floating(model));
        assert_eq!(w.active(), model, "as a press of its Float makes it active");
        assert!(w.views.bar_menu_open().is_none(), "an item chosen closes the menu");
        assert!(!menu_is_drawn(&w));
        choose(&mut w, model, BAR_MENU_FLOAT);
        assert!(!w.views.is_floating(model));
        assert_eq!(w.views.dock_rect(model), Some(place));

        // Maximise, then the same item, now Restore.
        choose(&mut w, model, BAR_MENU_MAXIMISE);
        assert!(w.views.is_maximised(model));
        assert_eq!(w.views.dock_rect(model), Some(w.views.rect()));
        assert!(!w.views.on_screen(plan));
        choose(&mut w, model, BAR_MENU_MAXIMISE);
        assert!(!w.views.is_maximised(model));
        assert_eq!(w.views.dock_rect(model), Some(place));

        // Minimise: to the tray, the active view handed on to one that shows.
        choose(&mut w, model, BAR_MENU_MINIMISE);
        assert!(w.views.is_minimised(model));
        assert_eq!(w.views.tray_actions().len(), 1);
        assert_ne!(w.active(), model);
        assert!(w.views.restore_view(model));
        w.frame();

        // Close: the view is gone, and no command ran for any of them.
        choose(&mut w, section, BAR_MENU_CLOSE);
        assert_eq!(w.views.view_ids(), [plan, model]);
        assert!(w.ran().is_empty(), "{:?}", w.ran());
    }

    #[test]
    fn a_tiling_in_the_menu_runs_its_line_through_the_executor_and_arranges_the_views() {
        let (mut w, [plan, model, section]) = three_views();
        let untiled: Vec<_> = [plan, model, section]
            .iter()
            .map(|&id| w.views.dock_rect(id))
            .collect();

        choose(&mut w, plan, "viewTileSideBySide");
        // One path: the line a person would type, handed to the window's executor.
        assert_eq!(w.ran(), ["VIEWS TILE COLUMNS"]);
        let tiled: Vec<_> = [plan, model, section]
            .iter()
            .map(|&id| w.views.dock_rect(id).expect("docked"))
            .collect();
        assert_ne!(
            tiled.iter().map(|rect| Some(*rect)).collect::<Vec<_>>(),
            untiled
        );
        // Side by side: three places of one height across the workspace, the view the menu was
        // opened on - made active by it - first.
        let area = w.views.rect();
        for place in &tiled {
            assert_eq!(place.height(), area.height());
            assert!((place.width() - tiled[0].width()).abs() <= 1.0);
        }
        assert_eq!(tiled[0].min.x, 0.0);
        assert!(tiled[0].min.x < tiled[1].min.x && tiled[1].min.x < tiled[2].min.x);

        choose(&mut w, plan, "viewTileStacked");
        choose(&mut w, plan, "viewTile");
        assert_eq!(
            w.ran(),
            ["VIEWS TILE COLUMNS", "VIEWS TILE ROWS", "VIEWS TILE GRID"]
        );
    }

    #[test]
    fn a_tiling_the_executor_refuses_moves_no_view_and_an_item_the_menu_lacks_is_not_found() {
        let (mut w, [plan, model, section]) = three_views();
        let before: Vec<_> = [plan, model, section]
            .iter()
            .map(|&id| w.views.dock_rect(id))
            .collect();
        w.refuse.set(true);
        let Site { views, host, .. } = &mut w;
        assert!(
            views
                .trigger_bar_menu(host, Scenery::default(), plan, "viewTile")
                .is_ok(),
            "the item was chosen; its line is the executor's to refuse"
        );
        w.frame();
        assert_eq!(w.ran(), ["VIEWS TILE GRID"]);
        let after: Vec<_> = [plan, model, section]
            .iter()
            .map(|&id| w.views.dock_rect(id))
            .collect();
        assert_eq!(after, before);

        let Site { views, host, .. } = &mut w;
        let unknown = views
            .trigger_bar_menu(host, Scenery::default(), plan, "viewBarRollUp")
            .expect_err("the menu has no such item");
        assert!(unknown.describe().contains("no such item"), "{}", unknown.describe());
        let closed = views
            .trigger_bar_menu(host, Scenery::default(), 99, BAR_MENU_CLOSE)
            .expect_err("no view 99 is open");
        assert!(closed.describe().contains("no such view"), "{}", closed.describe());
        assert!(!views.show_bar_menu(99, None));
        assert!(views.bar_menu(99).is_none());
    }

    #[test]
    fn a_workspace_without_the_windows_chrome_has_no_bar_menu() {
        let w = Site::plain(800.0, 600.0);
        let plan = w.active();
        assert!(w.views.bar_menu(plan).is_none());
        assert!(!w.views.show_bar_menu(plan, None));
        assert!(w.views.bar_menu_open().is_none());
    }

    #[test]
    fn the_menu_of_a_view_closed_while_it_is_open_goes_with_it() {
        let (mut w, [_plan, model, _section]) = three_views();
        assert!(w.views.show_bar_menu(model, None));
        w.frame();
        assert!(menu_is_drawn(&w));
        assert!(w.views.close_view(w.host.document_mut(), model).is_ok());
        w.frame();
        assert!(w.views.bar_menu_open().is_none());
        assert!(!menu_is_drawn(&w));
    }
}
