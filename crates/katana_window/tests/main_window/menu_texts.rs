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

    /// The icons of `menu`'s own items (not its submenus'), with their names.
    fn item_icons(menu: &Menu) -> Vec<(String, &'static str)> {
        menu.actions()
            .into_iter()
            .map(|action| (action.name.clone(), action.icon))
            .collect()
    }

    #[test]
    fn the_menus_whose_items_shared_one_icon_give_each_item_its_own() {
        // The third look audit's G: GIS's eleven processing items and Terrain's Analysis and DEM
        // items shared one icon, View > Toolbars' thirteen toolbars one window, Text Size's four
        // one "A", and Edit Surface's scope items the crossed square or another item's icon.
        let window = MainWindow::new();
        for name in [
            "gisMenu",
            "terrainAnalysisMenu",
            "terrainDemMenu",
            "viewToolBars",
            "viewTextSize",
            "terrainEditMenu",
        ] {
            let icons = item_icons(&menu(&window, name));
            assert!(icons.len() >= 2, "the premise: {name} has its items ({icons:?})");
            let mut shared = Vec::new();
            for (at, (item, icon)) in icons.iter().enumerate() {
                assert!(
                    katana_window::icons::is_named_icon(icon),
                    "{name} > {item} has no icon of the set ({icon:?})"
                );
                for (other, theirs) in &icons[at + 1..] {
                    if icon == theirs {
                        shared.push(format!("{item} and {other}: {icon}"));
                    }
                }
            }
            assert!(shared.is_empty(), "{name}'s items sharing an icon: {shared:?}");
        }
    }

    #[test]
    fn a_toolbars_item_shows_its_first_buttons_icon() {
        // Each toggle of View > Toolbars shows the icon of its bar's first button, so a bar is
        // found by its look: File's New, Draw's Select, GIS's Import Vector Data.
        let window = MainWindow::new();
        let toolbars = menu(&window, "viewToolBars");
        let icon_of = |name: &str| {
            toolbars
                .actions()
                .into_iter()
                .find(|action| action.name == name)
                .map(|action| action.icon)
                .unwrap_or_else(|| panic!("View > Toolbars has {name}"))
        };
        assert_eq!(icon_of("viewToolBarFile"), "New");
        assert_eq!(icon_of("viewToolBarEdit"), "Undo");
        assert_eq!(icon_of("viewToolBarProperties"), "Properties");
        assert_eq!(icon_of("viewToolBarDraw"), "Select");
        assert_eq!(icon_of("viewToolBarGIS"), "ImportVector");
        // The icon sizes say their sizes.
        assert_eq!(icon_of("viewToolBarIconsSmall"), "ToolbarIconsSmall");
        assert_eq!(icon_of("viewToolBarIconsLarge"), "ToolbarIconsLarge");
    }

    #[test]
    fn every_menu_section_title_is_in_sentence_case_and_names_no_library() {
        // docs/desktop.md: a menu's section title is in sentence case, small and muted. The third
        // look audit found "Coordinate Geometry", "Tables and Libraries", "Across the Drawing"
        // and GIS's "Vector - GDAL", "Point Cloud - PDAL" - which library does the work is not
        // what a person chooses a command by.
        let window = MainWindow::new();
        fn walk(menu: &Menu, titles: &mut Vec<String>) {
            for item in &menu.items {
                match item {
                    MenuItem::Section(title) => titles.push(title.clone()),
                    MenuItem::Menu(sub) => walk(sub, titles),
                    MenuItem::Action(_) | MenuItem::Separator => {}
                }
            }
        }
        let mut titles = Vec::new();
        for menu in &window.composed().menus {
            walk(menu, &mut titles);
        }
        assert!(titles.len() >= 30, "the premise: the menus' sections ({titles:?})");
        // A word after the first in title case: a capital, then a small letter.
        let title_case = |word: &str| {
            let mut letters = word.chars().filter(|c| c.is_alphabetic());
            matches!(
                (letters.next(), letters.next()),
                (Some(first), Some(second)) if first.is_uppercase() && second.is_lowercase()
            )
        };
        let wrong: Vec<&String> = titles
            .iter()
            .filter(|title| {
                title.split(' ').skip(1).any(title_case)
                    || title.contains("GDAL")
                    || title.contains("PDAL")
            })
            .collect();
        assert!(wrong.is_empty(), "section titles not in sentence case: {wrong:?}");
        for title in ["Coordinate geometry", "Point cloud", "Vector", "Tables and libraries"] {
            assert!(titles.iter().any(|each| each == title), "{title} in {titles:?}");
        }
    }

    #[test]
    fn files_groups_are_each_headed_and_views_items_are_in_title_case() {
        // The fourth look audit: File's last group (Settings, Quit) stood under a bare rule among
        // headed sections, and View's "Thin screen lines (faster)" was the one item in sentence
        // case among Title Case items.
        let window = MainWindow::new();
        let file = menu(&window, "fileMenu");
        assert!(
            !file
                .items
                .iter()
                .any(|item| matches!(item, MenuItem::Separator)),
            "File has no bare rule"
        );
        assert!(
            matches!(file.items.first(), Some(MenuItem::Section(_))),
            "File starts with a section"
        );
        let view = menu(&window, "viewMenu");
        let thin = view
            .actions()
            .into_iter()
            .find(|action| action.name == "ThinScreenLinesAction")
            .cloned()
            .expect("View has the thin lines item");
        assert_eq!(thin.text.replace('&', ""), "Thin Screen Lines (Faster)");
        // Every word of View's own items that is not a short joining word starts with a capital.
        let small = ["a", "an", "and", "as", "at", "by", "for", "in", "of", "on", "or", "the", "to"];
        let lower: Vec<String> = view
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action(action) => Some(action.text.replace('&', "")),
                _ => None,
            })
            .filter(|text| {
                text.split([' ', '(', ')'])
                    .skip(1)
                    .filter(|word| !word.is_empty() && !small.contains(word))
                    .any(|word| word.chars().next().is_some_and(char::is_lowercase))
            })
            .collect();
        assert!(lower.is_empty(), "View's items not in Title Case: {lower:?}");
    }
}
