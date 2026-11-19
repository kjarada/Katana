//! View > Meshes Seen from Above (2026-11-19): the window's switch for how its plan views, plots
//! and sheets draw the meshes - their faces, their footprints, or not at all - reached from the
//! View menu, which runs VIEWS MESHES as a typed line would. Its letter is the f of from: the
//! letters of "Meshes in Plan" and the b of Above were all other View items' (the shortcut check).

use katana_cad::scene::MeshPlanDisplay;
use katana_window::MainWindow;
use katana_window::drive::steps::find_menu;
use katana_window::drive::{Menu, MenuItem};
use katana_window::plan_painter::{mesh_plan_display, set_mesh_plan_display};

fn menu(window: &MainWindow, name: &str) -> Menu {
    find_menu(&window.composed().menus, name)
        .unwrap_or_else(|| panic!("the window has a menu {name}"))
        .clone()
}

/// The three items' names, each with whether it is checked.
fn checks(window: &MainWindow) -> Vec<(String, Option<bool>)> {
    menu(window, "viewMeshesMenu")
        .actions()
        .into_iter()
        .map(|action| (action.name.clone(), action.checked))
        .collect()
}

mod meshes_in_plan {
    use super::*;

    #[test]
    fn view_meshes_in_plan_sets_the_windows_switch_through_its_verb_and_shows_which_is_on() {
        // The switch is process-wide (plan_source_of reads it), so this one test owns it and
        // leaves it as it found it.
        set_mesh_plan_display(MeshPlanDisplay::Faces);
        let mut window = MainWindow::new();
        // The submenu is among View's own items, with its picture.
        let view = menu(&window, "viewMenu");
        let sub = view
            .items
            .iter()
            .find_map(|item| match item {
                MenuItem::Menu(sub) if sub.name == "viewMeshesMenu" => Some(sub.clone()),
                _ => None,
            })
            .expect("View has Meshes Seen from Above");
        assert_eq!(sub.title.replace('&', ""), "Meshes Seen from Above");
        assert_eq!(
            checks(&window),
            vec![
                (String::from("viewMeshesFaces"), Some(true)),
                (String::from("viewMeshesFootprint"), Some(false)),
                (String::from("viewMeshesHidden"), Some(false)),
            ],
            "Faces until it is changed"
        );
        for action in sub.actions() {
            assert!(!action.icon.is_empty(), "{}", action.name);
            assert!(action.tip.contains("VIEWS MESHES"), "{}", action.tip);
        }

        window
            .trigger("viewMeshesFootprint")
            .expect("the item is triggered");
        assert_eq!(mesh_plan_display(), MeshPlanDisplay::Footprint);
        assert_eq!(
            checks(&window)[1],
            (String::from("viewMeshesFootprint"), Some(true))
        );
        window
            .trigger("viewMeshesHidden")
            .expect("the item is triggered");
        assert_eq!(mesh_plan_display(), MeshPlanDisplay::Hidden);
        window
            .trigger("viewMeshesFaces")
            .expect("the item is triggered");
        assert_eq!(mesh_plan_display(), MeshPlanDisplay::Faces);
    }
}
