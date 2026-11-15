//! The twins of tests/qt_widgets/terrain/test_surfaces_panel.cpp's two cases that install the
//! Surfaces panel in a window (installSurfacesPanel): the main window's, since the window is where
//! the Rust installs it (main_window::actions and main_window::layout). The panel itself is TR's,
//! twinned in tests/surfaces.

use katana_window::MainWindow;
use katana_window::drive::steps::find_menu;
use katana_window::drive::{Menu, MenuItem};

/// The menu named `name` of the window's.
fn menu(window: &MainWindow, name: &str) -> Menu {
    find_menu(&window.composed().menus, name)
        .unwrap_or_else(|| panic!("the window has a menu {name}"))
        .clone()
}

/// Where an action named `name` is among `menu`'s own items.
fn place_of(menu: &Menu, name: &str) -> Option<usize> {
    menu.items
        .iter()
        .position(|item| matches!(item, MenuItem::Action(action) if action.name == name))
}

mod surfaces_panel_widget {
    use super::*;

    #[test]
    fn its_dock_is_reached_from_view_panels_and_the_terrain_menu() {
        let mut window = MainWindow::new();
        assert!(!window.ui.docks.surfaces, "until asked for");
        // View > Panels > Surfaces.
        let panels = menu(&window, "viewPanels");
        let toggle = panels
            .actions()
            .into_iter()
            .find(|action| action.name == "viewSurfacesPanel")
            .cloned()
            .expect("View > Panels has the Surfaces toggle");
        assert_eq!(toggle.text, "&Surfaces");
        // Terrain > Surfaces Panel, with its picture and its sentence.
        let terrain = menu(&window, "terrainMenu");
        let at = place_of(&terrain, "terrainSurfacesPanel").expect("Terrain has the item");
        let MenuItem::Action(item) = &terrain.items[at] else {
            unreachable!()
        };
        assert!(!item.icon.is_empty());
        assert!(!item.tip.is_empty());
        // Right after Surface From Drawing, before the next section.
        assert!(at > 0);
        assert_eq!(
            place_of(&terrain, "surfaceFromDrawing"),
            Some(at - 1),
            "after Surface From Drawing"
        );
        let sections = terrain
            .items
            .iter()
            .position(|item| matches!(item, MenuItem::Section(title) if title == "Sections"))
            .expect("Terrain has its Sections");
        assert!(at < sections);
        window
            .trigger("terrainSurfacesPanel")
            .expect("the item is triggered");
        assert!(window.ui.docks.surfaces);
    }

    #[test]
    fn view_panels_surfaces_brings_the_panel_to_the_front_of_its_tab_group() {
        // Showing a dock of a tab group is not bringing it to the front: the panel came back as a
        // tab behind Reference Data. View > Panels > Surfaces raises it.
        let mut window = MainWindow::new();
        assert!(!window.ui.docks.surfaces);
        window
            .trigger("viewSurfacesPanel")
            .expect("the toggle is triggered");
        assert!(window.ui.docks.surfaces);
        let tabs = window.ui.docks.lower_left_tabs();
        assert_eq!(
            tabs,
            ["ReferenceDataDock", "SurfacesDock"],
            "the two docks share a tab group"
        );
        assert_eq!(
            window.ui.docks.lower_left(),
            Some("SurfacesDock"),
            "Surfaces is in front, not a tab behind"
        );
    }
}
