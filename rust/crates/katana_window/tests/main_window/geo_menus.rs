//! The twin of tests/qt_widgets/geo/test_geo_menus.cpp: the GIS and Terrain items the
//! geoprocessing lanes add (geo::menu_table) - each one, and each submenu, reached from the
//! keyboard by a letter of its own within its menu. Terrain > Analysis and its six items had none,
//! so a person could reach them only with the mouse.
//! qt_every_shortcut_and_menu_letter_reaches_one_thing_headless checks that no letter is shared
//! across the whole window; this checks that none is missing.

use std::collections::BTreeSet;

use katana_window::drive::{Menu, MenuItem};
use katana_window::geo::geo_workbench::GeoMenus;
use katana_window::geo::menu_table::{GeoMenuDialogs, build_geo_menus};

/// The letter after a single '&', upper case; empty for a text with none (the rule the window's
/// shortcut_clashes reads).
fn letter_of(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at + 1 < characters.len() {
        if characters[at] == '&' {
            if characters[at + 1] != '&' {
                return characters[at + 1].to_uppercase().collect();
            }
            at += 2;
            continue;
        }
        at += 1;
    }
    String::new()
}

/// Every item of `menu` and of its submenus has a letter, none shared in one menu; each failure
/// names the menu path and the item.
fn expect_letters(menu: &Menu, path: &str) {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for item in &menu.items {
        let (text, sub) = match item {
            MenuItem::Action(action) => (action.text.clone(), None),
            MenuItem::Menu(sub) => (sub.title.clone(), Some(sub)),
            // A section is a separator to Qt: no letter of its own.
            MenuItem::Section(_) | MenuItem::Separator => continue,
        };
        let letter = letter_of(&text);
        assert!(!letter.is_empty(), "{path} > {text} has no letter");
        assert!(taken.insert(letter.clone()), "{path}: {letter} is shared");
        if let Some(sub) = sub {
            expect_letters(sub, &format!("{path} > {}", text.replace('&', "")));
        }
    }
}

mod geo_menus {
    use super::*;

    #[test]
    fn every_item_the_lanes_add_has_a_letter_of_its_own() {
        let mut menus = GeoMenus::new(Menu::new("gisMenu", "&GIS"), Menu::new("terrainMenu", "&Terrain"));
        let mut dialogs = GeoMenuDialogs::new();
        build_geo_menus(&mut menus, &mut dialogs);

        let analysis = menus.terrain().items.iter().find_map(|item| match item {
            MenuItem::Menu(menu) if menu.name == "terrainAnalysisMenu" => Some(menu),
            _ => None,
        });
        let analysis = analysis.expect("Terrain > Analysis is made");
        assert_eq!(analysis.actions().len(), 6);
        expect_letters(menus.terrain(), "Terrain");
        expect_letters(menus.gis(), "GIS");
    }
}
