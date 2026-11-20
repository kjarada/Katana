//! The window's own panels floated and minimised (main_window::docking's `WindowPanels`, drawn by
//! main_window::docking_ui; the owner, 2026-11-21: "float minimise for panels don't work
//! properly"): a title bar's Float and Minimise clicked with the pointer as a person clicks them,
//! a floating panel moved by its title bar and sized by its edges, the tray's button, the layout
//! saved and restored - and the plain model under them.
//!
//! The rectangles expected are the design's of main_window::layout's module comment, at the
//! headless 1360 x 860: Layers at 39,143 240 x 312 over Reference Data at 39,459 240 x 347,
//! Properties at 1081,143 240 x 663, the command line at 283,696 794 x 110 under the views'
//! 283,143 794 x 549; the left column 39,143 240 x 663 whole; the panels 240 wide and the command
//! line 110 high with 4 between two docks.

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use katana_window::MainWindow;
use katana_window::drive::headless::Headless;
use katana_window::main_window::docking::{
    DockingAction, LEAST_PANEL, PANEL_SIZE, WINDOW_PANELS, WindowPanels, is_placeable,
    kept_inside, parse_action,
};
use katana_window::main_window::layout;

use super::tool_bar_docking::{click_bar_button, drag, drawn, settled};

/// A rectangle by its corner and its size, as the design writes one ("39,143 240 x 312").
fn at(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(width, height))
}

/// Whether two rectangles are the same to within the half point a pixel snap may take.
fn same(a: Rect, b: Rect) -> bool {
    (a.min - b.min).abs().max_elem() <= 0.5 && (a.max - b.max).abs().max_elem() <= 0.5
}

/// What a floating panel may cover in the headless window: under the 26 point menu bar and over
/// the 24 point status bar.
fn bounds() -> Rect {
    Rect::from_min_max(pos2(0.0, 26.0), pos2(1360.0, 836.0))
}

fn floats(window: &MainWindow, dock: &str) -> bool {
    window.ui.docking.window_panels.is_floating(dock)
}

fn is_drawn(headless: &Headless, target: &str) -> bool {
    headless.snapshot().target(target).is_some()
}

fn float_button_text(headless: &Headless, dock: &str) -> String {
    headless
        .snapshot()
        .control(dock, "DockFloatButton")
        .unwrap_or_else(|| panic!("{dock} has its Float button"))
        .text
        .clone()
}

mod window_panels {
    use super::*;

    #[test]
    fn a_click_on_layers_float_floats_it_reference_data_takes_the_column_and_dock_puts_it_back() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let layers = drawn(&headless, "LayersDock");
        let reference = drawn(&headless, "ReferenceDataDock");
        assert!(same(layers, at(39.0, 143.0, 240.0, 312.0)), "{layers:?}");
        assert!(same(reference, at(39.0, 459.0, 240.0, 347.0)), "{reference:?}");
        assert_eq!(float_button_text(&headless, "LayersDock"), "Float");

        click_bar_button(&mut window, &mut headless, "LayersDock", "DockFloatButton");
        assert!(floats(&window, "LayersDock"), "Float floats the panel");
        let floating = drawn(&headless, "LayersDock");
        assert!(!same(floating, layers), "it has left its docked place: {floating:?}");
        assert!(
            (floating.size() - layers.size()).abs().max_elem() <= 0.5,
            "at its docked size: {floating:?}"
        );
        assert!(bounds().contains_rect(floating), "inside the window: {floating:?}");
        // The docked layout closes up: Reference Data alone has the left column.
        let column = drawn(&headless, "ReferenceDataDock");
        assert!(same(column, at(39.0, 143.0, 240.0, 663.0)), "{column:?}");
        assert_eq!(
            float_button_text(&headless, "LayersDock"),
            "Dock",
            "a floating panel's button docks it, and says so"
        );

        click_bar_button(&mut window, &mut headless, "LayersDock", "DockFloatButton");
        assert!(!floats(&window, "LayersDock"), "Dock docks it");
        assert!(same(drawn(&headless, "LayersDock"), layers), "where it was");
        assert!(same(drawn(&headless, "ReferenceDataDock"), reference));
        assert_eq!(float_button_text(&headless, "LayersDock"), "Float");
    }

    #[test]
    fn a_click_on_properties_float_gives_its_column_to_the_views_and_dock_takes_it_back() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let docked = drawn(&headless, "PropertiesDock");
        assert!(same(docked, at(1081.0, 143.0, 240.0, 663.0)), "{docked:?}");
        let views = window.ui.docking.drawing;
        assert!(same(views, at(283.0, 143.0, 794.0, 549.0)), "{views:?}");

        click_bar_button(&mut window, &mut headless, "PropertiesDock", "DockFloatButton");
        assert!(floats(&window, "PropertiesDock"));
        let floating = drawn(&headless, "PropertiesDock");
        assert!(!same(floating, docked), "{floating:?}");
        assert!(bounds().contains_rect(floating), "{floating:?}");
        // The views run on to the Modify column at 1321: the panel's 240 and the 4 beside it.
        let wider = window.ui.docking.drawing;
        assert!(same(wider, at(283.0, 143.0, 1038.0, 549.0)), "{wider:?}");
        assert_eq!(float_button_text(&headless, "PropertiesDock"), "Dock");

        click_bar_button(&mut window, &mut headless, "PropertiesDock", "DockFloatButton");
        assert!(!floats(&window, "PropertiesDock"));
        assert!(same(drawn(&headless, "PropertiesDock"), docked));
        assert!(same(window.ui.docking.drawing, views));
    }

    #[test]
    fn a_click_on_the_command_lines_float_gives_its_strip_to_the_views_and_dock_takes_it_back() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let docked = drawn(&headless, "CommandLineDock");
        assert!(same(docked, at(283.0, 696.0, 794.0, 110.0)), "{docked:?}");
        let views = window.ui.docking.drawing;

        click_bar_button(&mut window, &mut headless, "CommandLineDock", "DockFloatButton");
        assert!(floats(&window, "CommandLineDock"));
        let floating = drawn(&headless, "CommandLineDock");
        assert!(!same(floating, docked), "{floating:?}");
        assert!(bounds().contains_rect(floating), "{floating:?}");
        // The views run down to the Drafting bar at 806: the command line's 110 and the 4 over it.
        let taller = window.ui.docking.drawing;
        assert!(same(taller, at(283.0, 143.0, 794.0, 663.0)), "{taller:?}");

        click_bar_button(&mut window, &mut headless, "CommandLineDock", "DockFloatButton");
        assert!(!floats(&window, "CommandLineDock"));
        assert!(same(drawn(&headless, "CommandLineDock"), docked));
        assert!(same(window.ui.docking.drawing, views));
    }

    #[test]
    fn a_floating_panel_dragged_by_its_title_bar_moves_with_the_pointer_and_keeps_its_size() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        click_bar_button(&mut window, &mut headless, "LayersDock", "DockFloatButton");
        let before = drawn(&headless, "LayersDock");
        // On its title, clear of the sizing strip along its top and of its buttons at the right.
        let title = pos2(before.min.x + 60.0, before.min.y + 13.0);
        let by = vec2(120.0, 80.0);
        drag(&mut window, &mut headless, title, title + by);
        let after = drawn(&headless, "LayersDock");
        assert!(same(after, before.translate(by)), "{before:?} -> {after:?}");
        assert!(floats(&window, "LayersDock"), "let go, it stays floating where it is");
    }

    #[test]
    fn a_floating_panel_dragged_past_the_windows_edge_stops_inside_it() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        click_bar_button(&mut window, &mut headless, "LayersDock", "DockFloatButton");
        let before = drawn(&headless, "LayersDock");
        let title = pos2(before.min.x + 60.0, before.min.y + 13.0);
        // 2000 points up and to the left: far past the window's corner.
        drag(&mut window, &mut headless, title, title - vec2(2000.0, 2000.0));
        let after = drawn(&headless, "LayersDock");
        // Against the window's left edge, under the menu bar, its size kept.
        assert!(
            same(after, Rect::from_min_size(pos2(0.0, 26.0), before.size())),
            "{after:?}"
        );
    }

    #[test]
    fn a_floating_panel_dragged_by_an_edge_or_a_corner_is_sized_by_it() {
        // Each edge and one corner, the pointer pressed a point inside the panel's border and
        // moved 40 points out, as the torn-off menus' test does.
        let out = 40.0;
        /// An edge's name, where on the panel it is pressed, and how far it is dragged.
        type Edge = (&'static str, fn(Rect) -> Pos2, Vec2);
        let edges: [Edge; 5] = [
            ("top", |r: Rect| pos2(r.center().x - 40.0, r.min.y + 1.0), vec2(0.0, -out)),
            ("right", |r: Rect| pos2(r.max.x - 1.0, r.center().y), vec2(out, 0.0)),
            ("bottom", |r: Rect| pos2(r.center().x, r.max.y - 1.0), vec2(0.0, out)),
            ("left", |r: Rect| pos2(r.min.x + 1.0, r.center().y), vec2(-out, 0.0)),
            ("corner", |r: Rect| r.max - vec2(1.0, 1.0), vec2(out, out)),
        ];
        for (edge, grab, by) in edges {
            let mut window = MainWindow::new();
            let mut headless = settled(&mut window);
            click_bar_button(&mut window, &mut headless, "LayersDock", "DockFloatButton");
            let before = drawn(&headless, "LayersDock");
            let from = grab(before);
            drag(&mut window, &mut headless, from, from + by);
            let after = drawn(&headless, "LayersDock");
            let grew = after.size() - before.size();
            // The drag's 40 points, less the half point the pixel snap may take at each end.
            assert!(
                (grew.x - by.x.abs()).abs() <= 1.0 && (grew.y - by.y.abs()).abs() <= 1.0,
                "{edge}: {before:?} -> {after:?}"
            );
            if edge == "left" {
                assert!((after.max.x - before.max.x).abs() <= 1.0, "left keeps its right edge");
            }
            if edge == "top" {
                // Sized from its top, it is not moved as a press on its title bar moves it.
                assert!((after.max.y - before.max.y).abs() <= 1.0, "top keeps its bottom edge");
            }
        }
    }

    #[test]
    fn every_edge_and_corner_of_a_floating_panel_is_a_named_control() {
        let mut window = MainWindow::new();
        window.trigger("viewPanelFloatPropertiesDock").expect("floated");
        let headless = settled(&mut window);
        let frame = headless.snapshot();
        for part in [
            "Left", "Top", "Right", "Bottom", "TopLeft", "TopRight", "BottomLeft", "BottomRight",
        ] {
            let name = format!("PropertiesDockResize{part}");
            assert!(frame.control_anywhere(&name).is_some(), "{name}");
        }
        // A docked panel has none: it is sized by the window's geometry.
        assert!(frame.control_anywhere("LayersDockResizeLeft").is_none());
    }

    #[test]
    fn a_click_on_a_docked_panels_minimise_hides_it_to_the_tray_and_its_button_puts_it_back() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let before = drawn(&headless, "PropertiesDock");
        click_bar_button(&mut window, &mut headless, "PropertiesDock", "DockMinimiseButton");
        assert!(!is_drawn(&headless, "PropertiesDock"), "the panel is hidden");
        assert!(window.ui.docking.window_panels.is_minimised("PropertiesDock"));
        let button = headless
            .snapshot()
            .control("MinimisedToolBar", "RestorePropertiesDock")
            .expect("it has its button in the tray")
            .clone();
        assert_eq!(button.text, "Properties", "named as the panel is titled");
        // Hidden, it gives its room to the views as a closed panel does.
        let wider = window.ui.docking.drawing;
        assert!(same(wider, at(283.0, 143.0, 1038.0, 549.0)), "{wider:?}");

        click_bar_button(&mut window, &mut headless, "MinimisedToolBar", "RestorePropertiesDock");
        assert!(same(drawn(&headless, "PropertiesDock"), before), "back where it was");
        assert!(!window.ui.docking.window_panels.is_minimised("PropertiesDock"));
        assert!(
            headless
                .snapshot()
                .control_anywhere("RestorePropertiesDock")
                .is_none(),
            "and its button has left the tray"
        );
    }

    #[test]
    fn a_floating_panel_minimised_comes_back_from_the_tray_floating_where_it_was() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        click_bar_button(&mut window, &mut headless, "CommandLineDock", "DockFloatButton");
        let before = drawn(&headless, "CommandLineDock");
        click_bar_button(&mut window, &mut headless, "CommandLineDock", "DockMinimiseButton");
        assert!(!is_drawn(&headless, "CommandLineDock"));
        assert!(
            headless
                .snapshot()
                .control("MinimisedToolBar", "RestoreCommandLineDock")
                .is_some(),
            "in the tray"
        );
        assert!(floats(&window, "CommandLineDock"), "its place is kept while it is hidden");

        click_bar_button(&mut window, &mut headless, "MinimisedToolBar", "RestoreCommandLineDock");
        assert!(floats(&window, "CommandLineDock"));
        assert!(same(drawn(&headless, "CommandLineDock"), before), "at its rectangle");
    }

    #[test]
    fn ticking_a_minimised_panel_in_view_panels_restores_it_and_empties_its_tray_button() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let before = drawn(&headless, "LayersDock");
        window.trigger("viewPanelMinimiseLayersDock").expect("minimised");
        headless.settle(&mut window).expect("settled");
        assert!(!window.ui.docks.layers, "View > Panels shows it unticked");
        assert!(headless.snapshot().control_anywhere("RestoreLayersDock").is_some());

        window.trigger("viewPanelLayers").expect("View > Panels > Layers");
        headless.settle(&mut window).expect("settled");
        assert!(same(drawn(&headless, "LayersDock"), before), "back where it was");
        assert!(!window.ui.docking.window_panels.is_minimised("LayersDock"));
        assert!(headless.snapshot().control_anywhere("RestoreLayersDock").is_none());
    }

    #[test]
    fn close_hides_a_panel_with_no_tray_button_as_before() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        click_bar_button(&mut window, &mut headless, "LayersDock", "DockCloseButton");
        assert!(!window.ui.docks.layers);
        assert!(!is_drawn(&headless, "LayersDock"));
        assert!(!window.ui.docking.window_panels.is_minimised("LayersDock"));
        assert!(headless.snapshot().control_anywhere("RestoreLayersDock").is_none());
    }

    #[test]
    fn floating_one_of_reference_data_and_surfaces_floats_that_page_and_leaves_the_other_docked() {
        let mut window = MainWindow::new();
        window.trigger("viewSurfacesPanel").expect("Surfaces shown");
        let mut headless = settled(&mut window);
        // Tabbed, Surfaces in front: the one place draws the one page.
        let place = drawn(&headless, "SurfacesDock");
        assert!(same(place, at(39.0, 459.0, 240.0, 347.0)), "{place:?}");
        assert!(!is_drawn(&headless, "ReferenceDataDock"), "behind its tab");

        window.trigger("viewPanelFloatSurfacesDock").expect("floated");
        headless.settle(&mut window).expect("settled");
        assert!(floats(&window, "SurfacesDock"));
        assert!(!floats(&window, "ReferenceDataDock"));
        let floating = drawn(&headless, "SurfacesDock");
        assert!(!same(floating, place), "{floating:?}");
        assert!(
            same(drawn(&headless, "ReferenceDataDock"), place),
            "Reference Data has the place to itself"
        );

        window.trigger("viewPanelDockSurfacesDock").expect("docked");
        headless.settle(&mut window).expect("settled");
        assert!(same(drawn(&headless, "SurfacesDock"), place), "in front of its pair again");
        assert!(!is_drawn(&headless, "ReferenceDataDock"));
    }

    #[test]
    fn the_layout_saved_holds_a_floating_panel_and_brings_a_minimised_one_back_open() {
        let mut window = MainWindow::new();
        for action in [
            "viewPanelPlace:LayersDock:float:400,300,260,340",
            "viewPanelPlace:CommandLineDock:float:500,420,600,150",
            "viewPanelMinimiseCommandLineDock",
            "viewPanelMinimisePropertiesDock",
        ] {
            window.trigger(action).expect(action);
        }
        let text = layout::layout_text(&window);
        let mut restored = MainWindow::new();
        assert!(layout::restore_layout(&mut restored, &text), "{text}");
        let panels = &restored.ui.docking.window_panels;
        assert_eq!(panels.floating_rect("LayersDock"), Some(at(400.0, 300.0, 260.0, 340.0)));
        assert_eq!(
            panels.floating_rect("CommandLineDock"),
            Some(at(500.0, 420.0, 600.0, 150.0)),
            "a panel minimised while floating is saved at its rectangle"
        );
        assert_eq!(panels.floating_rect("PropertiesDock"), None);
        // The tray is a session's: what was minimised comes back open, as a torn-off menu does.
        assert!(panels.minimised.is_empty());
        assert!(restored.ui.docks.properties && restored.ui.docks.command_line);
        // Drawn, the restored window shows them where they were saved.
        let headless = settled(&mut restored);
        assert!(same(drawn(&headless, "LayersDock"), at(400.0, 300.0, 260.0, 340.0)));
        assert!(same(drawn(&headless, "CommandLineDock"), at(500.0, 420.0, 600.0, 150.0)));
        assert!(same(drawn(&headless, "PropertiesDock"), at(1081.0, 143.0, 240.0, 663.0)));
    }

    #[test]
    fn a_layout_saved_before_the_panels_could_float_restores_every_panel_docked() {
        // What a build of 2026-11-19 saved: version 3, with no "windowPanels".
        let older = "{\"version\":3,\"layers\":true,\"properties\":true,\"commandLine\":true,\
                     \"reference\":true,\"surfaces\":false,\"hidden\":[],\"rows\":[],\"bars\":[],\
                     \"menuPanels\":[],\"menuBands\":[]}";
        let mut window = MainWindow::new();
        window.trigger("viewPanelFloatLayersDock").expect("floated");
        window.trigger("viewPanelMinimisePropertiesDock").expect("minimised");
        assert!(layout::restore_layout(&mut window, older));
        assert_eq!(window.ui.docking.window_panels, WindowPanels::default());
        let headless = settled(&mut window);
        assert!(same(drawn(&headless, "LayersDock"), at(39.0, 143.0, 240.0, 312.0)));
        assert!(same(drawn(&headless, "PropertiesDock"), at(1081.0, 143.0, 240.0, 663.0)));
    }

    #[test]
    fn a_saved_layout_passes_over_a_panel_it_cannot_place_and_keeps_the_rest() {
        let text = "{\"version\":3,\"windowPanels\":[\"LayersDock=float:10,20,300,200\",\
                    \"NoSuchDock=float:1,2,300,200\",\"PropertiesDock=left\",\
                    \"CommandLineDock=float:1,2,0,50\",\"SurfacesDock=float:a,b,c,d\"]}";
        let saved = layout::read_layout(text).expect("read");
        assert_eq!(
            saved.window_panels,
            vec![(String::from("LayersDock"), at(10.0, 20.0, 300.0, 200.0))]
        );
    }

    #[test]
    fn a_rectangle_saved_in_a_larger_window_is_drawn_inside_this_one() {
        let mut window = MainWindow::new();
        // Its corner past the headless window's 1360 x 860.
        window
            .trigger("viewPanelPlace:LayersDock:float:3000,2000,260,340")
            .expect("floated");
        let headless = settled(&mut window);
        let shown = drawn(&headless, "LayersDock");
        // Against the right edge and the status bar's top, its size kept.
        assert!(same(shown, at(1100.0, 496.0, 260.0, 340.0)), "{shown:?}");
        // What is kept is what was asked: the window made large again, it returns there.
        assert_eq!(
            window.ui.docking.window_panels.floating_rect("LayersDock"),
            Some(at(3000.0, 2000.0, 260.0, 340.0))
        );
    }

    // Needs the one line asked of main_window::window's "viewResetLayout" (this change's report):
    // `ui.docking.window_panels = super::docking::WindowPanels::default();`.
    #[test]
    fn reset_layout_docks_every_floating_panel_and_empties_the_tray() {
        let mut window = MainWindow::new();
        window.trigger("viewPanelFloatLayersDock").expect("floated");
        window.trigger("viewPanelMinimisePropertiesDock").expect("minimised");
        window.trigger("viewResetLayout").expect("reset");
        assert_eq!(window.ui.docking.window_panels, WindowPanels::default());
        let headless = settled(&mut window);
        assert!(same(drawn(&headless, "LayersDock"), at(39.0, 143.0, 240.0, 312.0)));
        assert!(same(drawn(&headless, "PropertiesDock"), at(1081.0, 143.0, 240.0, 663.0)));
        assert!(headless.snapshot().target("MinimisedToolBar").is_none());
    }

    #[test]
    fn a_headless_run_reports_where_each_panel_is() {
        let mut window = MainWindow::new();
        window
            .trigger("viewPanelPlace:LayersDock:float:400,300,260,340")
            .expect("floated");
        window.trigger("viewPanelMinimisePropertiesDock").expect("minimised");
        let report = window.docking_report("windowPanels").expect("reported");
        let parts: Vec<&str> = report.split(' ').collect();
        assert_eq!(parts.len(), WINDOW_PANELS.len(), "{report}");
        assert!(parts.contains(&"LayersDock=float:400,300,260,340"), "{report}");
        assert!(parts.contains(&"PropertiesDock=docked;minimised"), "{report}");
        assert!(parts.contains(&"CommandLineDock=docked"), "{report}");
        // Surfaces is hidden until asked for.
        assert!(parts.contains(&"SurfacesDock=docked;hidden"), "{report}");
    }

    #[test]
    fn an_action_on_a_panel_that_cannot_take_it_is_refused_with_its_reason() {
        let mut window = MainWindow::new();
        let error = window
            .trigger("viewPanelRestoreLayersDock")
            .expect_err("Layers is not minimised");
        assert!(error.describe().contains("not minimised"), "{}", error.describe());
        // Surfaces is hidden until asked for: there is nothing to minimise.
        let error = window
            .trigger("viewPanelMinimiseSurfacesDock")
            .expect_err("Surfaces is not shown");
        assert!(error.describe().contains("not shown"), "{}", error.describe());
        assert!(window.ui.docking.window_panels.minimised.is_empty());
        // Floated by its action, a hidden panel is shown: placing what nobody sees does nothing.
        window.trigger("viewPanelFloatSurfacesDock").expect("floated");
        assert!(window.ui.docks.surfaces);
    }
}

mod window_panels_model {
    use super::*;

    #[test]
    fn a_panel_floated_is_at_its_rectangle_and_docked_again_keeps_it_for_the_next_float() {
        let mut panels = WindowPanels::default();
        assert!(!panels.is_floating("LayersDock"), "a panel not named is docked");
        let first = at(100.0, 200.0, 240.0, 300.0);
        assert!(panels.float("LayersDock", first));
        assert_eq!(panels.floating_rect("LayersDock"), Some(first));
        // Floated again, it is moved, not floated twice.
        let moved = at(300.0, 250.0, 260.0, 320.0);
        assert!(panels.float("LayersDock", moved));
        assert_eq!(panels.floating.len(), 1);
        assert_eq!(panels.floating_rect("LayersDock"), Some(moved));
        assert!(panels.dock("LayersDock"));
        assert!(!panels.is_floating("LayersDock"));
        assert_eq!(panels.rect_before("LayersDock"), Some(moved));
        assert!(!panels.dock("LayersDock"), "docked already");
        // Another panel's place is its own.
        assert_eq!(panels.rect_before("PropertiesDock"), None);
    }

    #[test]
    fn only_a_window_panel_with_a_real_rectangle_floats() {
        let mut panels = WindowPanels::default();
        let good = at(0.0, 0.0, 240.0, 300.0);
        assert!(!panels.float("drawMenuTornOff", good), "a torn-off menu is not one");
        assert!(!panels.float("", good));
        for bad in [
            at(10.0, 10.0, 0.0, 0.0),
            at(10.0, 10.0, 240.0, 0.0),
            at(10.0, 10.0, -5.0, 300.0),
            Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(240.0, 300.0)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(f32::INFINITY, 300.0)),
            Rect::from_min_max(pos2(f32::NEG_INFINITY, 0.0), pos2(240.0, 300.0)),
        ] {
            assert!(!is_placeable(bad), "{bad:?}");
            assert!(!panels.float("LayersDock", bad), "{bad:?}");
        }
        assert_eq!(panels, WindowPanels::default(), "nothing changed");
        for dock in WINDOW_PANELS {
            assert!(panels.float(dock, good), "{dock}");
        }
        assert_eq!(panels.floating.len(), 7);
    }

    #[test]
    fn a_minimised_panel_keeps_its_place_is_in_the_tray_once_and_is_restored_once() {
        let mut panels = WindowPanels::default();
        let rect = at(100.0, 200.0, 240.0, 300.0);
        panels.float("CommandLineDock", rect);
        assert!(panels.minimise("CommandLineDock"));
        assert!(panels.minimise("LayersDock"));
        assert!(!panels.minimise("CommandLineDock"), "in the tray already");
        assert!(!panels.minimise("drawMenuTornOff"), "not a window panel");
        assert_eq!(panels.minimised, ["CommandLineDock", "LayersDock"], "in the order they went");
        assert_eq!(panels.floating_rect("CommandLineDock"), Some(rect), "its place is kept");
        assert!(panels.restore("CommandLineDock"));
        assert!(!panels.restore("CommandLineDock"), "restored already");
        assert!(!panels.is_minimised("CommandLineDock"));
        assert!(panels.is_minimised("LayersDock"));
        assert_eq!(panels.floating_rect("CommandLineDock"), Some(rect));
    }

    #[test]
    fn a_floating_rectangle_is_kept_wholly_inside_the_window() {
        let bounds = bounds();
        // Inside already: as it is.
        let inside = at(300.0, 200.0, 240.0, 312.0);
        assert_eq!(kept_inside(inside, bounds), inside);
        // Past the right and the bottom: moved back by what it overhangs, 1360 - 240 and 836 - 312.
        assert_eq!(
            kept_inside(at(1300.0, 800.0, 240.0, 312.0), bounds),
            at(1120.0, 524.0, 240.0, 312.0)
        );
        // Past the left and over the menu bar: to the bounds' corner.
        assert_eq!(
            kept_inside(at(-50.0, -50.0, 240.0, 312.0), bounds),
            at(0.0, 26.0, 240.0, 312.0)
        );
        // Far off the screen altogether.
        assert_eq!(
            kept_inside(at(90000.0, -90000.0, 240.0, 312.0), bounds),
            at(1120.0, 26.0, 240.0, 312.0)
        );
        // Larger than the window: the bounds' size, 1360 x 810.
        assert_eq!(kept_inside(at(10.0, 40.0, 2000.0, 1000.0), bounds), bounds);
        // Smaller than a panel can be: the least size, where it is.
        assert_eq!(
            kept_inside(at(100.0, 100.0, 50.0, 20.0), bounds),
            Rect::from_min_size(pos2(100.0, 100.0), LEAST_PANEL)
        );
    }

    #[test]
    fn a_rectangle_that_is_not_one_becomes_a_panel_at_the_windows_corner() {
        let bounds = bounds();
        let fallback = Rect::from_min_size(bounds.min, PANEL_SIZE);
        for bad in [
            at(100.0, 100.0, 0.0, 0.0),
            at(100.0, 100.0, -40.0, 300.0),
            Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(240.0, 300.0)),
            Rect::from_min_max(pos2(0.0, f32::NAN), pos2(240.0, f32::NAN)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(f32::INFINITY, 300.0)),
            Rect::NOTHING,
            Rect::EVERYTHING,
        ] {
            let kept = kept_inside(bad, bounds);
            assert_eq!(kept, fallback, "{bad:?}");
            assert!(is_placeable(kept) && bounds.contains_rect(kept));
        }
    }

    #[test]
    fn in_a_window_smaller_than_a_panel_the_panel_is_the_windows_size_and_no_bounds_keep_nothing() {
        // A window 100 x 50 under its bars: less than the least panel's 160 x 96.
        let small = at(0.0, 26.0, 100.0, 50.0);
        assert_eq!(kept_inside(at(30.0, 40.0, 240.0, 312.0), small), small);
        // Bounds that are not a rectangle - a window not yet sized - leave it as it is.
        let rect = at(30.0, 40.0, 240.0, 312.0);
        assert_eq!(kept_inside(rect, at(0.0, 0.0, 0.0, 0.0)), rect);
        assert_eq!(
            kept_inside(rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(f32::NAN, 100.0))),
            rect
        );
    }

    #[test]
    fn the_geometry_leaves_out_a_floating_panel_as_it_does_a_hidden_one() {
        let screen = at(0.0, 0.0, 1360.0, 860.0);
        let lines = layout::BarLines::default();
        let laid = |panels: &WindowPanels| {
            let docked = layout::Docks::default().docked_only(panels);
            layout::geometry(screen, &docked, false, &lines, [None; 4])
        };
        let rect = at(300.0, 200.0, 240.0, 312.0);
        // None floating: the design's rectangles.
        let all = laid(&WindowPanels::default());
        assert_eq!(all.layers, Some(at(39.0, 143.0, 240.0, 312.0)));
        assert_eq!(all.lower_left, Some(at(39.0, 459.0, 240.0, 347.0)));
        assert_eq!(all.views, at(283.0, 143.0, 794.0, 549.0));

        let mut panels = WindowPanels::default();
        panels.float("LayersDock", rect);
        let no_layers = laid(&panels);
        assert_eq!(no_layers.layers, None);
        assert_eq!(no_layers.lower_left, Some(at(39.0, 143.0, 240.0, 663.0)));
        assert_eq!(no_layers.views, all.views, "the column is still there");
        // Reference Data floated too: the left column is gone and the views start at the Draw
        // column's 39.
        panels.float("ReferenceDataDock", rect);
        let no_left = laid(&panels);
        assert_eq!((no_left.layers, no_left.lower_left), (None, None));
        assert_eq!(no_left.views, at(39.0, 143.0, 1038.0, 549.0));
        assert_eq!(no_left.command_line, Some(at(39.0, 696.0, 1038.0, 110.0)));

        let mut panels = WindowPanels::default();
        panels.float("PropertiesDock", rect);
        let no_properties = laid(&panels);
        assert_eq!(no_properties.properties, None);
        assert_eq!(no_properties.views, at(283.0, 143.0, 1038.0, 549.0));

        let mut panels = WindowPanels::default();
        panels.float("CommandLineDock", rect);
        let no_command_line = laid(&panels);
        assert_eq!(no_command_line.command_line, None);
        assert_eq!(no_command_line.views, at(283.0, 143.0, 794.0, 663.0));
        // The same as the panel hidden.
        let hidden = layout::Docks {
            command_line: false,
            ..layout::Docks::default()
        };
        assert_eq!(
            no_command_line,
            layout::geometry(screen, &hidden, false, &lines, [None; 4])
        );
    }

    #[test]
    fn a_hidden_panel_that_floats_is_neither_docked_nor_made_shown() {
        let mut panels = WindowPanels::default();
        panels.float("SurfacesDock", at(300.0, 200.0, 240.0, 312.0));
        let docked = layout::Docks::default().docked_only(&panels);
        assert!(!docked.surfaces, "Surfaces is hidden until asked for");
        assert_eq!(docked.lower_left(), Some("ReferenceDataDock"));
        // Shown and floating, with Surfaces in front of the pair: the docked place is Reference
        // Data's alone.
        let shown = layout::Docks {
            surfaces: true,
            surfaces_in_front: true,
            ..layout::Docks::default()
        };
        let docked = shown.docked_only(&panels);
        assert_eq!(docked.lower_left_tabs(), ["ReferenceDataDock"]);
        assert_eq!(docked.lower_left(), Some("ReferenceDataDock"));
    }

    #[test]
    fn the_window_panels_actions_are_read_from_their_names() {
        let dock = String::from("LayersDock");
        assert_eq!(
            parse_action("viewPanelFloatLayersDock"),
            Some(DockingAction::WindowPanelFloat {
                dock: dock.clone(),
                rect: None
            })
        );
        assert_eq!(
            parse_action("viewPanelDockLayersDock"),
            Some(DockingAction::WindowPanelDock { dock: dock.clone() })
        );
        assert_eq!(
            parse_action("viewPanelMinimiseLayersDock"),
            Some(DockingAction::WindowPanelMinimise { dock: dock.clone() })
        );
        assert_eq!(
            parse_action("viewPanelRestoreLayersDock"),
            Some(DockingAction::WindowPanelRestore { dock: dock.clone() })
        );
        assert_eq!(
            parse_action("viewPanelPlace:LayersDock:float:400,300,260,340"),
            Some(DockingAction::WindowPanelFloat {
                dock,
                rect: Some(at(400.0, 300.0, 260.0, 340.0))
            })
        );
        // Every panel has its four.
        for dock in WINDOW_PANELS {
            for verb in ["Float", "Dock", "Minimise", "Restore"] {
                let name = format!("viewPanel{verb}{dock}");
                assert!(parse_action(&name).is_some(), "{name}");
            }
        }
        // View > Panels' own toggles begin with the same word and are not docking actions; nor is
        // a panel that is none of the window's, a side for a panel, or a rectangle with no size.
        for other in [
            "viewPanelLayers",
            "viewPanelProperties",
            "viewPanelCommandLine",
            "viewPanelReferenceData",
            "viewPanels",
            "viewPanelFloat",
            "viewPanelFloatNoSuchDock",
            "viewPanelDockdrawMenuTornOff",
            "viewPanelPlace:NoSuchDock:float:1,2,300,200",
            "viewPanelPlace:LayersDock:left",
            "viewPanelPlace:LayersDock:float:1,2,0,200",
            "viewPanelPlace:LayersDock",
        ] {
            assert_eq!(parse_action(other), None, "{other}");
        }
    }
}
