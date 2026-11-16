//! The menu bar's catalogue as the look of 2026-11-16 left it (main_window::actions): every
//! submenu carries the icon its item shows, Help's items that open a window say so with an
//! ellipsis, the snap modes read in British English under their old action names, and GIS's
//! Dataset Information sits under a section of its own rather than a bare rule.

use katana_window::MainWindow;
use katana_window::drive::steps::find_menu;
use katana_window::drive::{Menu, MenuItem};

/// The menu named `name` of the window's.
fn menu(window: &MainWindow, name: &str) -> Menu {
    find_menu(&window.composed().menus, name)
        .unwrap_or_else(|| panic!("the window has a menu {name}"))
        .clone()
}

mod menu_texts {
    use super::*;

    #[test]
    fn every_submenu_shows_the_icon_its_item_is_listed_with() {
        // Every item of a menu has an icon (`--check-menus` holds the catalogue to it); a
        // submenu's was in the catalogue's signs alone, and its item drew none.
        let window = MainWindow::new();
        let composed = window.composed();
        fn walk(menu: &Menu, missing: &mut Vec<String>, seen: &mut usize) {
            for item in &menu.items {
                if let MenuItem::Menu(sub) = item {
                    *seen += 1;
                    if !katana_window::icons::is_named_icon(sub.icon) {
                        missing.push(format!("{} > {} ({:?})", menu.name, sub.name, sub.icon));
                    }
                    walk(sub, missing, seen);
                }
            }
        }
        let mut missing = Vec::new();
        let mut seen = 0;
        for menu in &composed.menus {
            walk(menu, &mut missing, &mut seen);
        }
        assert!(seen >= 10, "the premise: the menu bar has its submenus ({seen})");
        assert!(missing.is_empty(), "submenus with no icon: {missing:?}");
    }

    #[test]
    fn helps_items_each_open_a_window_and_each_end_in_an_ellipsis() {
        // An item that opens a dialog ends in "...", one that acts at once does not: Keyboard
        // Shortcuts... and Check for Updates... had it, Command Reference, Sheets and Plotting
        // Commands and About Katana, which open windows too, did not.
        let window = MainWindow::new();
        let help = menu(&window, "helpMenu");
        let texts: Vec<String> = help
            .actions()
            .into_iter()
            .map(|action| action.text.replace('&', ""))
            .collect();
        assert_eq!(texts.len(), 5, "{texts:?}");
        let bare: Vec<&String> = texts.iter().filter(|text| !text.ends_with("...")).collect();
        assert!(bare.is_empty(), "{bare:?}");
    }

    #[test]
    fn the_snap_modes_read_centre_and_keep_their_action_names() {
        let window = MainWindow::new();
        let modes = menu(&window, "viewSnapModes");
        let centre = modes
            .actions()
            .into_iter()
            .find(|action| action.name == "viewSnapModeCenter")
            .cloned()
            .expect("the centre snap keeps its action's name");
        assert_eq!(centre.text, "Centre");
        assert!(
            modes.actions().iter().all(|action| !action.text.contains("Center")),
            "no American spelling shown"
        );
    }

    #[test]
    fn dataset_information_is_under_a_section_of_its_own_not_a_bare_rule() {
        // It reads a vector file, a raster or a point cloud alike; a bare rule left it looking
        // like Point Cloud's last item.
        let window = MainWindow::new();
        let gis = menu(&window, "gisMenu");
        let at = gis
            .items
            .iter()
            .position(|item| {
                matches!(item, MenuItem::Action(action) if action.name == "datasetInformation")
            })
            .expect("GIS has Dataset Information");
        assert!(
            matches!(&gis.items[at - 1], MenuItem::Section(_)),
            "{:?}",
            gis.items[at - 1]
        );
        assert!(
            !gis.items
                .iter()
                .any(|item| matches!(item, MenuItem::Separator)),
            "GIS's groups are all titled sections"
        );
    }
}
