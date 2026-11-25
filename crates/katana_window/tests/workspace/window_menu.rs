//! The Window menu (docs/desktop.md, "The Window menu"; main_window::actions). Not twins: the C++
//! window listed its views nowhere.
//!
//! It stands before Help and holds the three tilings - the very actions View > Viewport Layout
//! shows - and every open view by its title, the active one ticked. An item makes its view the
//! active one by the line a person would type, VIEWS ACTIVATE, which brings a minimised view out
//! of the tray too.

mod window_menu {
    use katana_cad::view_set::ViewId;
    use katana_window::MainWindow;
    use katana_window::drive::headless::Headless;
    use katana_window::drive::steps::find_menu;
    use katana_window::drive::{Action, Menu, MenuItem};
    use katana_window::main_window::layout;

    fn menu_of(window: &MainWindow) -> Menu {
        find_menu(&window.composed().menus, "windowMenu")
            .expect("the menu bar has Window")
            .clone()
    }

    fn rows(menu: &Menu) -> Vec<String> {
        menu.items
            .iter()
            .map(|item| match item {
                MenuItem::Section(title) => format!("[{title}]"),
                MenuItem::Action(action) => action.name.clone(),
                MenuItem::Menu(sub) => sub.name.clone(),
                MenuItem::Separator => String::from("-"),
            })
            .collect()
    }

    /// The menu's items for the open views, in its order.
    fn view_items(menu: &Menu) -> Vec<Action> {
        menu.actions()
            .into_iter()
            .filter(|action| action.name.starts_with("windowView"))
            .cloned()
            .collect()
    }

    /// The window with a 3D view and a section opened beside its plan: views 1, 2 and 3.
    fn window_with_three_views() -> MainWindow {
        let mut window = MainWindow::new();
        assert!(window.exec.run_command("VIEWS OPEN 3d"));
        assert!(window.exec.run_command("VIEWS OPEN section"));
        assert_eq!(window.exec.workspace.view_ids(), [1, 2, 3]);
        window
    }

    fn settled(window: &mut MainWindow) -> Headless {
        let mut headless = Headless::new();
        headless
            .settle(window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        headless
    }

    #[test]
    fn window_stands_before_help_at_the_end_of_the_menu_bar_with_a_letter_of_its_own() {
        let window = MainWindow::new();
        let composed = window.composed();
        let names: Vec<&str> = composed
            .menus
            .iter()
            .map(|menu| menu.name.as_str())
            .collect();
        let count = names.len();
        assert_eq!(names[count - 2..], ["windowMenu", "helpMenu"], "{names:?}");
        assert_eq!(menu_of(&window).title, "&Window");
        // Alt+W is no other menu's, and the menu has an icon and a tip on every item.
        let (clashes, _) = layout::shortcut_clashes(&composed);
        assert!(clashes.is_empty(), "{clashes:?}");
        let (gaps, _) = layout::menu_gaps(&composed);
        assert!(gaps.is_empty(), "{gaps:?}");
    }

    #[test]
    fn it_lists_the_tilings_and_then_every_open_view_by_its_title_the_active_one_ticked() {
        let window = MainWindow::new();
        assert_eq!(
            rows(&menu_of(&window)),
            [
                "[Arrange the open views]",
                "viewTile",
                "viewTileSideBySide",
                "viewTileStacked",
                "viewDuplicate",
                "[Open views]",
                "windowView1",
            ]
        );

        let mut window = window_with_three_views();
        let menu = menu_of(&window);
        assert_eq!(
            rows(&menu)[6..],
            ["windowView1", "windowView2", "windowView3"]
        );
        let items = view_items(&menu);
        let texts: Vec<&str> = items.iter().map(|action| action.text.as_str()).collect();
        assert_eq!(texts, ["Plan 1", "3D 1", "Section 1"]);
        let icons: Vec<&str> = items.iter().map(|action| action.icon).collect();
        assert_eq!(icons, ["ViewPlan", "View3D", "ViewSection"]);
        // VIEWS OPEN makes the view it opens the active one: the section, opened last.
        let ticked: Vec<Option<bool>> = items.iter().map(|action| action.checked).collect();
        assert_eq!(ticked, [Some(false), Some(false), Some(true)]);
        for (id, action) in (1..).zip(&items) {
            assert!(
                action.tip.contains(&format!("(VIEWS ACTIVATE {id})")),
                "{}",
                action.tip
            );
            assert!(action.enabled && action.shortcut.is_none());
        }
        // With several views open the menu bar's rules still hold.
        let composed = window.composed();
        assert!(layout::shortcut_clashes(&composed).0.is_empty());
        assert!(layout::menu_gaps(&composed).0.is_empty());

        // A view closed leaves the list.
        let closed = window
            .exec
            .with_parts(|workspace, parts| workspace.close_view(parts, 2));
        assert!(closed.is_ok());
        let texts: Vec<String> = view_items(&menu_of(&window))
            .into_iter()
            .map(|action| action.text)
            .collect();
        assert_eq!(texts, ["Plan 1", "Section 1"]);
    }

    #[test]
    fn an_item_makes_its_view_the_active_one_by_the_views_activate_line() {
        let mut window = window_with_three_views();
        let plan: ViewId = 1;
        assert_eq!(window.exec.workspace.active_id(), 3);

        window.trigger("windowView1").expect("triggered");
        assert_eq!(window.exec.workspace.active_id(), plan);
        // One path: the line is in the command log, as a typed one is.
        let log = window.exec.log.text();
        assert!(log.contains("VIEWS ACTIVATE 1"), "{log}");
        // And the tick has moved.
        let ticked: Vec<Option<bool>> = view_items(&menu_of(&window))
            .iter()
            .map(|action| action.checked)
            .collect();
        assert_eq!(ticked, [Some(true), Some(false), Some(false)]);
    }

    #[test]
    fn an_item_brings_a_minimised_view_out_of_the_tray() {
        let mut window = window_with_three_views();
        let mut headless = settled(&mut window);
        window.exec.workspace.minimise_view(2);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(window.exec.workspace.is_minimised(2));
        // In the tray it is still an open view, and still listed.
        assert_eq!(view_items(&menu_of(&window)).len(), 3);

        window.trigger("windowView2").expect("triggered");
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(!window.exec.workspace.is_minimised(2));
        assert!(window.exec.workspace.on_screen(2));
        assert_eq!(window.exec.workspace.active_id(), 2);
    }

    #[test]
    fn an_item_for_a_view_that_is_not_open_is_no_menu_item() {
        let mut window = window_with_three_views();
        let error = window
            .trigger("windowView9")
            .expect_err("no view 9 is open");
        assert!(
            error.describe().contains("no such menu item"),
            "{}",
            error.describe()
        );
        assert_eq!(window.exec.workspace.active_id(), 3);
    }

    #[test]
    fn a_tiling_chosen_in_window_runs_the_line_view_viewport_layouts_item_runs() {
        // The action is one, shown twice; triggered, it is the one line either way.
        let mut window = window_with_three_views();
        let in_window: Vec<Action> = menu_of(&window)
            .actions()
            .into_iter()
            .filter(|action| action.name.starts_with("viewTile"))
            .cloned()
            .collect();
        let in_view: Vec<Action> = find_menu(&window.composed().menus, "viewLayoutMenu")
            .expect("View has Viewport Layout")
            .actions()
            .into_iter()
            .filter(|action| action.name.starts_with("viewTile"))
            .cloned()
            .collect();
        assert_eq!(in_window.len(), 3);
        assert_eq!(in_window, in_view);

        window.trigger("viewTileStacked").expect("triggered");
        let log = window.exec.log.text();
        assert!(log.contains("VIEWS TILE ROWS"), "{log}");
        assert!(
            log.contains("tiled=3 kind=rows docked=0 minimised=0"),
            "{log}"
        );
    }

    #[test]
    fn duplicate_view_in_window_copies_the_active_view_beside_it_and_tiles_by_one_line() {
        // The one action in both menus, as the tilings are.
        let mut window = window_with_three_views();
        let in_window = menu_of(&window)
            .actions()
            .into_iter()
            .find(|action| action.name == "viewDuplicate")
            .cloned()
            .expect("Window has Duplicate View");
        let in_view = find_menu(&window.composed().menus, "viewLayoutMenu")
            .expect("View has Viewport Layout")
            .actions()
            .into_iter()
            .find(|action| action.name == "viewDuplicate")
            .cloned()
            .expect("Viewport Layout has Duplicate View");
        assert_eq!(in_window, in_view);
        assert_eq!(in_window.text, "Duplicate View");
        assert_eq!(in_window.icon, "Copy");
        assert!(in_window.tip.contains("the active view"), "{}", in_window.tip);
        assert!(in_window.enabled && in_window.shortcut.is_none());

        // The section, opened last, is the active view: it is the one copied, as Section 2, and
        // the four views are tiled - in the one line, logged with its reply.
        window.trigger("viewDuplicate").expect("triggered");
        let log = window.exec.log.text();
        assert!(log.contains("VIEWS DUPLICATE TILE"), "{log}");
        assert!(log.contains("duplicated=3 copy=4"), "{log}");
        assert!(log.contains("tiled=4 kind=grid docked=0 minimised=0"), "{log}");
        let texts: Vec<String> = view_items(&menu_of(&window))
            .into_iter()
            .map(|action| action.text)
            .collect();
        assert_eq!(texts, ["Plan 1", "3D 1", "Section 1", "Section 2"]);
        assert_eq!(window.exec.workspace.active_id(), 4);
        // The window's views, not the drawing.
        assert_eq!(window.exec.document.history().undo_count(), 0);
    }

    #[test]
    fn the_window_menu_tears_off_into_a_panel_as_any_menu_of_the_bar_does() {
        let mut window = MainWindow::new();
        window
            .trigger("viewMenuTearOffWindowMenu")
            .expect("torn off");
        let bar = window
            .ui
            .panels
            .chrome
            .title_bar("windowMenuTornOff")
            .expect("the panel's title bar");
        // Its icon is its first item's, Tile's: the menu opens from no item and has none else.
        assert_eq!(bar.icon(), "LayoutTile");
    }
}
