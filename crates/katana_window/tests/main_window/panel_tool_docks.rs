//! Vertices and the survey points floated and minimised (main_window::docking's `WindowPanels`,
//! as Layers and Properties are in panel_floating.rs): the two of the window's own panels that
//! are hidden until a tool or a menu item shows them, and whose shown flag is not the layout's
//! but their owner's - the drawing system's for Vertices (View > Panels > Vertices,
//! `actionVerticesPanel`, and the Edit Vertices tool), the survey workbench's for the survey
//! points (Survey > Point Manager, `surveyPointManager`).
//!
//! Each is brought up as a person brings it up, by its menu item's action or its tool, and its
//! title bar is clicked with the pointer.
//!
//! The rectangles are the design's (main_window::layout's module comment) at the headless
//! 1360 x 860: the right column is 1081,143 240 x 663, Properties' alone until one of these is
//! shown; then they share it equally, 4 points between two - two of them 329.5 high each, the
//! second at 143 + 329.5 + 4 = 476.5; three of them 655 / 3 high each.

use egui::{Rect, vec2};
use katana_window::MainWindow;
use katana_window::drive::headless::Headless;

use super::panel_floating::{at, bounds, float_button_text, floats, is_drawn, same};
use super::tool_bar_docking::{click_bar_button, drawn, settled};

/// Each panel's dock, the action that shows it, and its title.
const TOOL_PANELS: [(&str, &str, &str); 2] = [
    ("VerticesDock", "actionVerticesPanel", "Vertices"),
    ("SurveyPointsDock", "surveyPointManager", "Survey Points"),
];

/// The right column whole, and its halves.
fn column() -> Rect {
    at(1081.0, 143.0, 240.0, 663.0)
}

fn upper_half() -> Rect {
    at(1081.0, 143.0, 240.0, 329.5)
}

fn lower_half() -> Rect {
    at(1081.0, 476.5, 240.0, 329.5)
}

/// A window with the panel `action` shows shown, drawn until idle.
fn shown_by(action: &str) -> (MainWindow, Headless) {
    let mut window = MainWindow::new();
    window
        .trigger(action)
        .unwrap_or_else(|error| panic!("{action}: {}", error.describe()));
    let headless = settled(&mut window);
    (window, headless)
}

/// The window drawn until it is idle - after one frame more than that asks: what a tool's start
/// tells the window reaches it at the end of a frame, and the panel it opens is drawn in the
/// next, which nothing else need have asked for.
fn settle(window: &mut MainWindow, headless: &mut Headless) {
    let _ = headless.frame(window);
    headless
        .settle(window)
        .unwrap_or_else(|problem| panic!("{problem}"));
}

/// Whether the tray holds a button for `dock`.
fn in_tray(headless: &Headless, dock: &str) -> bool {
    headless
        .snapshot()
        .control("MinimisedToolBar", &format!("Restore{dock}"))
        .is_some()
}

/// Nothing of `dock` is on the screen: no panel, no sizing edge of a floating one, no button in
/// the tray.
fn no_trace(headless: &Headless, dock: &str) -> bool {
    let frame = headless.snapshot();
    frame.target(dock).is_none()
        && frame.control_anywhere(&format!("{dock}ResizeLeft")).is_none()
        && frame.control_anywhere(&format!("Restore{dock}")).is_none()
}

mod tool_panels {
    use super::*;

    #[test]
    fn each_is_hidden_until_its_action_shows_it_under_properties_in_the_right_column() {
        for (dock, action, title) in TOOL_PANELS {
            let mut window = MainWindow::new();
            let mut headless = settled(&mut window);
            assert!(!window.window_panel_shown(dock), "{dock} is hidden until asked for");
            assert!(no_trace(&headless, dock), "{dock}");
            assert!(same(drawn(&headless, "PropertiesDock"), column()));
            let views = window.ui.docking.drawing;

            window.trigger(action).expect(action);
            settle(&mut window, &mut headless);
            assert!(window.window_panel_shown(dock), "{action} shows {dock}");
            let frame = headless.snapshot();
            let panel = frame.target(dock).expect("drawn");
            assert_eq!(panel.title, title);
            assert!(same(panel.rect, lower_half()), "{dock}: {:?}", panel.rect);
            let properties = drawn(&headless, "PropertiesDock");
            assert!(same(properties, upper_half()), "{properties:?}");
            assert_eq!(float_button_text(&headless, dock), "Float");
            // The column was there for Properties: the views are as they were.
            assert!(same(window.ui.docking.drawing, views));
        }
    }

    #[test]
    fn a_click_on_its_float_floats_it_properties_has_the_column_again_and_dock_puts_it_back() {
        for (dock, action, _) in TOOL_PANELS {
            let (mut window, mut headless) = shown_by(action);
            let docked = drawn(&headless, dock);
            assert!(same(docked, lower_half()), "{dock}: {docked:?}");

            click_bar_button(&mut window, &mut headless, dock, "DockFloatButton");
            assert!(floats(&window, dock), "Float floats {dock}");
            // 24 points down and along from where it stood, at its docked size.
            let floating = drawn(&headless, dock);
            assert!(
                same(floating, docked.translate(vec2(24.0, 24.0))),
                "{dock}: {floating:?}"
            );
            assert!(bounds().contains_rect(floating), "{dock}: {floating:?}");
            assert_eq!(float_button_text(&headless, dock), "Dock");
            assert!(window.window_panel_shown(dock), "floating, it is still shown");
            assert!(same(drawn(&headless, "PropertiesDock"), column()), "{dock}");
            // Its eight sizing edges are there, as any floating panel's.
            let edge = format!("{dock}ResizeBottomRight");
            assert!(headless.snapshot().control_anywhere(&edge).is_some(), "{edge}");

            click_bar_button(&mut window, &mut headless, dock, "DockFloatButton");
            assert!(!floats(&window, dock), "Dock docks {dock}");
            assert!(same(drawn(&headless, dock), docked), "{dock} is back in its place");
            assert!(same(drawn(&headless, "PropertiesDock"), upper_half()));
            assert_eq!(float_button_text(&headless, dock), "Float");
        }
    }

    #[test]
    fn a_click_on_its_minimise_hides_it_to_the_tray_and_the_trays_button_puts_it_back() {
        for (dock, action, title) in TOOL_PANELS {
            let (mut window, mut headless) = shown_by(action);
            let docked = drawn(&headless, dock);
            let restore = format!("Restore{dock}");

            click_bar_button(&mut window, &mut headless, dock, "DockMinimiseButton");
            assert!(!is_drawn(&headless, dock), "{dock} is hidden");
            assert!(!window.window_panel_shown(dock), "its menu item is unticked");
            assert!(window.ui.docking.window_panels.is_minimised(dock));
            let button = headless
                .snapshot()
                .control("MinimisedToolBar", &restore)
                .unwrap_or_else(|| panic!("{dock} has its button in the tray"))
                .clone();
            assert_eq!(button.text, title, "named as the panel is titled");
            assert!(same(drawn(&headless, "PropertiesDock"), column()), "{dock}");

            click_bar_button(&mut window, &mut headless, "MinimisedToolBar", &restore);
            assert!(window.window_panel_shown(dock), "{dock} is shown again");
            assert!(same(drawn(&headless, dock), docked), "{dock} is back where it was");
            assert!(!window.ui.docking.window_panels.is_minimised(dock));
            assert!(
                headless.snapshot().target("MinimisedToolBar").is_none(),
                "and the tray is gone with its last button"
            );
        }
    }

    #[test]
    fn minimised_while_floating_it_comes_back_from_the_tray_floating_where_it_was() {
        for (dock, action, _) in TOOL_PANELS {
            let (mut window, mut headless) = shown_by(action);
            click_bar_button(&mut window, &mut headless, dock, "DockFloatButton");
            let floating = drawn(&headless, dock);
            click_bar_button(&mut window, &mut headless, dock, "DockMinimiseButton");
            assert!(!is_drawn(&headless, dock));
            assert!(in_tray(&headless, dock), "{dock} is in the tray");
            assert!(floats(&window, dock), "its place is kept while it is hidden");
            assert!(
                headless
                    .snapshot()
                    .control_anywhere(&format!("{dock}ResizeLeft"))
                    .is_none(),
                "a hidden panel has no edge left on the screen"
            );

            click_bar_button(
                &mut window,
                &mut headless,
                "MinimisedToolBar",
                &format!("Restore{dock}"),
            );
            assert!(floats(&window, dock));
            assert!(same(drawn(&headless, dock), floating), "{dock} at its rectangle");
        }
    }

    #[test]
    fn hidden_by_its_own_action_while_floating_it_leaves_no_trace_and_comes_back_floating() {
        for (dock, action, _) in TOOL_PANELS {
            let (mut window, mut headless) = shown_by(action);
            click_bar_button(&mut window, &mut headless, dock, "DockFloatButton");
            let floating = drawn(&headless, dock);

            // The menu item, unticked: hidden, not minimised.
            window.trigger(action).expect(action);
            settle(&mut window, &mut headless);
            assert!(!window.window_panel_shown(dock));
            assert!(no_trace(&headless, dock), "{dock}: no panel, no edge, no tray button");
            assert!(headless.snapshot().target("MinimisedToolBar").is_none());
            let report = window.docking_report("windowPanels").expect("reported");
            assert!(
                report.split(' ').any(|part| {
                    part.starts_with(&format!("{dock}=float:")) && part.ends_with(";hidden")
                }),
                "{report}"
            );

            // Ticked again, it is where it floated.
            window.trigger(action).expect(action);
            settle(&mut window, &mut headless);
            assert!(floats(&window, dock));
            assert!(same(drawn(&headless, dock), floating), "{dock}");
        }
    }

    #[test]
    fn closed_by_its_close_button_it_leaves_no_trace_docked_or_floating() {
        for (dock, action, _) in TOOL_PANELS {
            // Docked.
            let (mut window, mut headless) = shown_by(action);
            click_bar_button(&mut window, &mut headless, dock, "DockCloseButton");
            assert!(!window.window_panel_shown(dock), "Close unticks {action}");
            assert!(no_trace(&headless, dock), "{dock}");
            assert!(same(drawn(&headless, "PropertiesDock"), column()));
            // Floating.
            let (mut window, mut headless) = shown_by(action);
            click_bar_button(&mut window, &mut headless, dock, "DockFloatButton");
            click_bar_button(&mut window, &mut headless, dock, "DockCloseButton");
            assert!(!window.window_panel_shown(dock));
            assert!(no_trace(&headless, dock), "{dock}");
            assert!(!window.ui.docking.window_panels.is_minimised(dock));
        }
    }

    #[test]
    fn shown_again_by_its_own_action_a_minimised_one_leaves_the_tray() {
        for (dock, action, _) in TOOL_PANELS {
            let (mut window, mut headless) = shown_by(action);
            let docked = drawn(&headless, dock);
            click_bar_button(&mut window, &mut headless, dock, "DockMinimiseButton");
            assert!(in_tray(&headless, dock));

            // The menu item, ticked: the panel on the screen has no button to put it back.
            window.trigger(action).expect(action);
            settle(&mut window, &mut headless);
            assert!(window.window_panel_shown(dock));
            assert!(same(drawn(&headless, dock), docked), "{dock}");
            assert!(!in_tray(&headless, dock), "{dock} has left the tray");
            assert!(!window.ui.docking.window_panels.is_minimised(dock));
        }
    }

    #[test]
    fn the_edit_vertices_tool_takes_its_minimised_panel_out_of_the_tray_and_ending_it_leaves_no_ghost() {
        let (mut window, mut headless) = shown_by("actionVerticesPanel");
        click_bar_button(&mut window, &mut headless, "VerticesDock", "DockFloatButton");
        let floating = drawn(&headless, "VerticesDock");
        click_bar_button(&mut window, &mut headless, "VerticesDock", "DockMinimiseButton");
        assert!(in_tray(&headless, "VerticesDock"));

        // Draw > Edit Vertices: the tool opens its panel, where it was.
        window.trigger("draw.vertex.edit").expect("Edit Vertices started");
        settle(&mut window, &mut headless);
        assert!(
            window.window_panel_shown("VerticesDock"),
            "the premise: Edit Vertices shows the Vertices panel"
        );
        assert!(!in_tray(&headless, "VerticesDock"), "a tool that opens it empties its button");
        assert!(same(drawn(&headless, "VerticesDock"), floating), "floating where it was");

        // The tool ended (Select): whatever is shown is drawn, and nothing else of it is left.
        window.trigger("toolSelect").expect("Select");
        settle(&mut window, &mut headless);
        assert!(!in_tray(&headless, "VerticesDock"));
        assert_eq!(
            is_drawn(&headless, "VerticesDock"),
            window.window_panel_shown("VerticesDock"),
            "what is drawn is what is shown"
        );
        // Closed after it, the panel is gone whole.
        if window.window_panel_shown("VerticesDock") {
            click_bar_button(&mut window, &mut headless, "VerticesDock", "DockCloseButton");
        }
        assert!(no_trace(&headless, "VerticesDock"));
    }

    #[test]
    fn both_shown_share_the_column_in_thirds_and_one_floated_leaves_the_others_halves() {
        let mut window = MainWindow::new();
        for (_, action, _) in TOOL_PANELS {
            window.trigger(action).expect(action);
        }
        let mut headless = settled(&mut window);
        // 663 less two gaps of 4, in three: 218 and a third each, 222 and a third apart.
        let third = 655.0 / 3.0;
        let step = third + 4.0;
        for (at_index, dock) in ["PropertiesDock", "VerticesDock", "SurveyPointsDock"]
            .into_iter()
            .enumerate()
        {
            let expected = at(1081.0, 143.0 + step * at_index as f32, 240.0, third);
            let shown = drawn(&headless, dock);
            assert!(same(shown, expected), "{dock}: {shown:?}");
        }

        click_bar_button(&mut window, &mut headless, "VerticesDock", "DockFloatButton");
        assert!(floats(&window, "VerticesDock"));
        assert!(same(drawn(&headless, "PropertiesDock"), upper_half()));
        assert!(same(drawn(&headless, "SurveyPointsDock"), lower_half()));
        // Properties closed too: the survey points alone keep the column.
        window.trigger("viewPanelProperties").expect("hidden");
        settle(&mut window, &mut headless);
        assert!(same(drawn(&headless, "SurveyPointsDock"), column()));
        assert!(floats(&window, "VerticesDock"), "and Vertices still floats");
    }

    #[test]
    fn a_minimised_properties_comes_back_beside_a_panel_a_menu_item_opened_meanwhile() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        click_bar_button(&mut window, &mut headless, "PropertiesDock", "DockMinimiseButton");
        // Vertices shown while Properties is in the tray: it has the column to itself.
        window.trigger("actionVerticesPanel").expect("Vertices");
        settle(&mut window, &mut headless);
        assert!(same(drawn(&headless, "VerticesDock"), column()));

        click_bar_button(&mut window, &mut headless, "MinimisedToolBar", "RestorePropertiesDock");
        assert!(same(drawn(&headless, "PropertiesDock"), upper_half()), "over Vertices");
        assert!(same(drawn(&headless, "VerticesDock"), lower_half()));
    }
}
