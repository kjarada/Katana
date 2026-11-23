//! The window's floating panels over one another, and over the views (main_window::docking's
//! `WindowPanels`, whose list of floating panels is in their order, the one on top last;
//! main_window::docking_ui's `stack_floating_panels`, which keeps egui's Areas in it).
//!
//! What is on top is asked of egui, by the layer it has uppermost at a point two panels both
//! cover (`on_top_at`): the order a person sees and a press reaches, not only the list the
//! window keeps. The presses are pointer events, as a person's are.
//!
//! The panels are placed by their actions at rectangles chosen so that, in the headless
//! 1360 x 860: Layers is 400..660 across and 200..540 down, Properties 500..760 and 260..600,
//! the command line 450..850 and 300..500. So 580,400 is under all three; 700,400 under
//! Properties and the command line and not Layers; Layers' title at 460,213 and the foot of
//! Properties at 720,560 are under no other.

use egui::{Pos2, pos2};
use katana_window::MainWindow;
use katana_window::main_window::layout;
use katana_window::view_workspace::dock_name;

use super::panel_floating::{
    at, click_and_settle, floats, is_drawn, on_top_at, same, stacking, title_of,
};
use super::tool_bar_docking::{click_bar_button, drawn, settled};

const LAYERS: &str = "viewPanelPlace:LayersDock:float:400,200,260,340";
const PROPERTIES: &str = "viewPanelPlace:PropertiesDock:float:500,260,260,340";
const COMMAND_LINE: &str = "viewPanelPlace:CommandLineDock:float:450,300,400,200";

/// Under all three panels.
const UNDER_ALL: Pos2 = pos2(580.0, 400.0);
/// Under Properties and the command line, to the right of Layers.
const RIGHT_OF_LAYERS: Pos2 = pos2(700.0, 400.0);
/// On Layers' title, to the left of the other two.
const LAYERS_TITLE: Pos2 = pos2(460.0, 213.0);
/// At the foot of Properties, to the right of Layers and under the command line's 500.
const PROPERTIES_FOOT: Pos2 = pos2(720.0, 560.0);

fn placed(actions: &[&str]) -> MainWindow {
    let mut window = MainWindow::new();
    for action in actions {
        window
            .trigger(action)
            .unwrap_or_else(|error| panic!("{action}: {}", error.describe()));
    }
    window
}

mod panel_stacking {
    use super::*;

    #[test]
    fn the_panel_floated_last_is_on_top_and_a_press_on_a_lower_one_brings_it_over_the_other() {
        let mut window = placed(&[LAYERS, PROPERTIES]);
        let mut headless = settled(&mut window);
        assert_eq!(stacking(&window), ["LayersDock", "PropertiesDock"]);
        assert!(on_top_at(&headless, "PropertiesDock", UNDER_ALL), "floated last, it is on top");

        // A press on Layers where Properties does not cover it.
        click_and_settle(&mut window, &mut headless, LAYERS_TITLE);
        assert_eq!(stacking(&window), ["PropertiesDock", "LayersDock"]);
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL), "pressed, Layers is over it");
        assert!(same(drawn(&headless, "LayersDock"), at(400.0, 200.0, 260.0, 340.0)), "unmoved");

        // And the other way: a press on what still shows of Properties.
        click_and_settle(&mut window, &mut headless, PROPERTIES_FOOT);
        assert_eq!(stacking(&window), ["LayersDock", "PropertiesDock"]);
        assert!(on_top_at(&headless, "PropertiesDock", UNDER_ALL));
        // A press on the one on top changes nothing.
        click_and_settle(&mut window, &mut headless, PROPERTIES_FOOT);
        assert_eq!(stacking(&window), ["LayersDock", "PropertiesDock"]);
        assert!(on_top_at(&headless, "PropertiesDock", UNDER_ALL));
    }

    #[test]
    fn a_press_on_a_panel_under_two_others_brings_it_up_and_leaves_those_two_as_they_were() {
        let mut window = placed(&[LAYERS, PROPERTIES, COMMAND_LINE]);
        let mut headless = settled(&mut window);
        assert_eq!(stacking(&window), ["LayersDock", "PropertiesDock", "CommandLineDock"]);
        assert!(on_top_at(&headless, "CommandLineDock", UNDER_ALL));

        click_and_settle(&mut window, &mut headless, LAYERS_TITLE);
        assert_eq!(stacking(&window), ["PropertiesDock", "CommandLineDock", "LayersDock"]);
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL));
        assert!(
            on_top_at(&headless, "CommandLineDock", RIGHT_OF_LAYERS),
            "the command line is still over Properties"
        );
    }

    #[test]
    fn floating_or_placing_a_panel_by_its_action_brings_it_up_as_a_press_does() {
        let mut window = placed(&[LAYERS, PROPERTIES, COMMAND_LINE]);
        let mut headless = settled(&mut window);
        // Float, of a panel that floats already: where it is, and over the others.
        window.trigger("viewPanelFloatLayersDock").expect("brought up");
        assert_eq!(stacking(&window), ["PropertiesDock", "CommandLineDock", "LayersDock"]);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL), "and egui has it so");
        assert!(same(drawn(&headless, "LayersDock"), at(400.0, 200.0, 260.0, 340.0)), "unmoved");

        // Two brought up before a frame is drawn: egui is told of one a frame, and ends in the
        // window's order - Properties, placed where it was, then the command line.
        window.trigger(PROPERTIES).expect("placed");
        window.trigger("viewPanelFloatCommandLineDock").expect("brought up");
        assert_eq!(stacking(&window), ["LayersDock", "PropertiesDock", "CommandLineDock"]);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(on_top_at(&headless, "CommandLineDock", UNDER_ALL));
        // Properties over Layers where the command line covers neither: 580,280 is above its 300.
        assert!(on_top_at(&headless, "PropertiesDock", pos2(580.0, 280.0)));
    }

    #[test]
    fn a_panel_put_back_from_the_tray_or_from_view_panels_comes_back_on_top() {
        let mut window = placed(&[LAYERS, PROPERTIES]);
        let mut headless = settled(&mut window);
        // Layers, the lower of the two, minimised - by its action: a press on its own Minimise
        // would bring it up before it went - and put back by the tray's button.
        window.trigger("viewPanelMinimiseLayersDock").expect("minimised");
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(!is_drawn(&headless, "LayersDock"));
        assert_eq!(stacking(&window), ["LayersDock", "PropertiesDock"], "its place is kept");
        click_bar_button(&mut window, &mut headless, "MinimisedToolBar", "RestoreLayersDock");
        assert!(floats(&window, "LayersDock"));
        assert_eq!(stacking(&window), ["PropertiesDock", "LayersDock"]);
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL));

        // Properties, now the lower, closed and shown again from View > Panels.
        window.trigger("viewPanelProperties").expect("hidden");
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(!is_drawn(&headless, "PropertiesDock"));
        window.trigger("viewPanelProperties").expect("shown");
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert_eq!(stacking(&window), ["LayersDock", "PropertiesDock"]);
        assert!(on_top_at(&headless, "PropertiesDock", UNDER_ALL));
    }

    #[test]
    fn the_order_is_saved_with_the_layout_and_restored_with_it() {
        let mut window = placed(&[LAYERS, PROPERTIES, COMMAND_LINE]);
        // Layers brought up: the order is no longer the one they were floated in.
        window.trigger("viewPanelFloatLayersDock").expect("brought up");
        let order = ["PropertiesDock", "CommandLineDock", "LayersDock"];
        assert_eq!(stacking(&window), order);

        let text = layout::layout_text(&window);
        // Written in the order: it is the order of the layout's list.
        let found: Vec<usize> = order
            .iter()
            .map(|dock| {
                text.find(&format!("\"{dock}=float:"))
                    .unwrap_or_else(|| panic!("{dock} is in {text}"))
            })
            .collect();
        assert!(found[0] < found[1] && found[1] < found[2], "{text}");

        let mut restored = MainWindow::new();
        assert!(layout::restore_layout(&mut restored, &text), "{text}");
        assert_eq!(stacking(&restored), order);
        // Drawn for the first time, the panels are in it on the screen.
        let headless = settled(&mut restored);
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL));
        assert!(on_top_at(&headless, "CommandLineDock", RIGHT_OF_LAYERS));
        assert!(same(drawn(&headless, "LayersDock"), at(400.0, 200.0, 260.0, 340.0)));
    }

    #[test]
    fn a_layout_restored_over_panels_already_drawn_puts_them_in_its_order() {
        let mut window = placed(&[LAYERS, PROPERTIES, COMMAND_LINE]);
        let mut headless = settled(&mut window);
        assert!(on_top_at(&headless, "CommandLineDock", UNDER_ALL));
        // The same three, in the opposite order: every one has to pass another.
        let text = "{\"version\":3,\"windowPanels\":[\
                    \"CommandLineDock=float:450,300,400,200\",\
                    \"PropertiesDock=float:500,260,260,340\",\
                    \"LayersDock=float:400,200,260,340\"]}";
        assert!(layout::restore_layout(&mut window, text));
        assert_eq!(stacking(&window), ["CommandLineDock", "PropertiesDock", "LayersDock"]);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL));
        assert!(
            on_top_at(&headless, "PropertiesDock", RIGHT_OF_LAYERS),
            "and Properties is over the command line now"
        );
    }

    #[test]
    fn a_layout_saved_before_the_order_was_kept_restores_in_the_order_it_lists() {
        // What a build of 2026-11-21 saved: the panels in the order they were floated, which no
        // build read as an order. Read as one, it is the order egui gave panels first drawn
        // together - the one listed last on top - so the window opens as that build opened it.
        let older = "{\"version\":3,\"layers\":true,\"properties\":true,\"commandLine\":true,\
                     \"reference\":true,\"surfaces\":false,\"hidden\":[],\"rows\":[],\"bars\":[],\
                     \"menuPanels\":[],\"menuBands\":[],\"windowPanels\":[\
                     \"PropertiesDock=float:500,260,260,340\",\
                     \"LayersDock=float:400,200,260,340\"]}";
        let mut window = MainWindow::new();
        assert!(layout::restore_layout(&mut window, older));
        assert_eq!(stacking(&window), ["PropertiesDock", "LayersDock"]);
        let headless = settled(&mut window);
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL));
        assert!(same(drawn(&headless, "PropertiesDock"), at(500.0, 260.0, 260.0, 340.0)));
    }

    #[test]
    fn reset_layout_forgets_the_order_and_panels_floated_after_it_are_in_the_order_floated() {
        let mut window = placed(&[LAYERS, PROPERTIES]);
        let mut headless = settled(&mut window);
        assert!(on_top_at(&headless, "PropertiesDock", UNDER_ALL));

        window.trigger("viewResetLayout").expect("reset");
        assert!(stacking(&window).is_empty(), "no panel floats, and none has a place in an order");
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(!floats(&window, "LayersDock") && !floats(&window, "PropertiesDock"));

        // The other way round this time, both before a frame is drawn. egui remembers its old
        // order for two Areas shown again together; the window's is the one that holds.
        window.trigger(PROPERTIES).expect("floated");
        window.trigger(LAYERS).expect("floated");
        assert_eq!(stacking(&window), ["PropertiesDock", "LayersDock"]);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert!(on_top_at(&headless, "LayersDock", UNDER_ALL), "floated last, Layers is on top");
    }

    #[test]
    fn a_headless_run_reads_the_order_from_the_lowest_to_the_one_on_top() {
        let mut window = MainWindow::new();
        assert_eq!(window.docking_report("windowPanelOrder").as_deref(), Some("none"));
        for action in [LAYERS, PROPERTIES] {
            window.trigger(action).expect(action);
        }
        assert_eq!(
            window.docking_report("windowPanelOrder").as_deref(),
            Some("LayersDock PropertiesDock")
        );
        window.trigger("viewPanelFloatLayersDock").expect("brought up");
        assert_eq!(
            window.docking_report("windowPanelOrder").as_deref(),
            Some("PropertiesDock LayersDock")
        );
        window.trigger("viewPanelDockLayersDock").expect("docked");
        assert_eq!(
            window.docking_report("windowPanelOrder").as_deref(),
            Some("PropertiesDock")
        );
    }
}

/// A maximised view under the window's floating panels (docs/desktop.md, "The dock chrome": a
/// docked view maximised hides every other docked view until it is restored, and a floating one
/// fills the views' area).
mod panels_over_a_maximised_view {
    use super::*;

    #[test]
    fn a_docked_view_maximised_leaves_a_floating_panel_over_it_and_restore_loses_nothing() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        // A second view, by the line a person types, so that Maximise has a view to hide.
        let _ = window.exec.run_verb_line("VIEWS OPEN 3d");
        // A frame for the new view to be laid out in, before the window is drawn until idle.
        let _ = headless.frame(&mut window);
        window.trigger(LAYERS).expect("floated");
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        let ids = window.exec.workspace.view_ids();
        assert_eq!(ids.len(), 2, "the premise: VIEWS OPEN 3d opened a second view");
        let (first, second) = (dock_name(ids[0]), dock_name(ids[1]));
        let views = [drawn(&headless, &first), drawn(&headless, &second)];
        let panel = at(400.0, 200.0, 260.0, 340.0);
        assert!(same(drawn(&headless, "LayersDock"), panel));

        click_bar_button(&mut window, &mut headless, &first, "DockMaximiseButton");
        assert!(window.exec.workspace.is_maximised(ids[0]), "Maximise maximises the view");
        assert!(!is_drawn(&headless, &second), "and hides the other docked view");
        let filled = drawn(&headless, &first);
        assert!(
            filled.contains_rect(views[0]) && filled.contains_rect(views[1]),
            "it has the room of both: {filled:?}"
        );
        // The panel is where it was, floating, and over the view.
        assert!(floats(&window, "LayersDock"));
        assert!(same(drawn(&headless, "LayersDock"), panel));
        assert!(on_top_at(&headless, "LayersDock", panel.center()));
        assert_eq!(stacking(&window), ["LayersDock"]);

        // Restore, by the same button: both views where they were, the panel untouched.
        click_bar_button(&mut window, &mut headless, &first, "DockMaximiseButton");
        assert!(!window.exec.workspace.is_maximised(ids[0]));
        assert!(same(drawn(&headless, &first), views[0]), "the first view is back");
        assert!(same(drawn(&headless, &second), views[1]), "and the second");
        assert!(floats(&window, "LayersDock"));
        assert!(same(drawn(&headless, "LayersDock"), panel));
        assert!(on_top_at(&headless, "LayersDock", panel.center()));
    }

    #[test]
    fn a_floating_view_maximised_stays_under_the_windows_floating_panels() {
        let mut window = placed(&[LAYERS]);
        let mut headless = settled(&mut window);
        let id = window.exec.workspace.view_ids()[0];
        let view = dock_name(id);
        let panel = at(400.0, 200.0, 260.0, 340.0);

        // The view floated by its button: shown last, it is over every floating thing it
        // covers, the panel among them, as anything floated or pressed last is.
        click_bar_button(&mut window, &mut headless, &view, "DockFloatButton");
        assert!(window.exec.workspace.is_floating(id), "the premise: the view floats");
        let floating = drawn(&headless, &view);

        // Maximised - by a press on it, which leaves it on top - it is the views' area, and the
        // panel is over it as over a docked view.
        click_bar_button(&mut window, &mut headless, &view, "DockMaximiseButton");
        assert!(window.exec.workspace.is_maximised(id), "the premise: it is maximised");
        let filled = drawn(&headless, &view);
        assert!(filled.contains_rect(panel), "the view fills what the panel floats over");
        assert!(on_top_at(&headless, "LayersDock", panel.center()), "the panel is over it");
        assert!(same(drawn(&headless, "LayersDock"), panel));

        // Pressed, as a person working in it presses it - at its foot, clear of the panel -
        // it still does not come over the panel.
        let foot = pos2(filled.max.x - 60.0, filled.max.y - 40.0);
        assert!(!panel.contains(foot), "the premise: the press is on the view alone");
        click_and_settle(&mut window, &mut headless, foot);
        assert!(on_top_at(&headless, "LayersDock", panel.center()), "nor when it is pressed");

        // Restored, the view has its floating rectangle again; the panel is as it was.
        click_bar_button(&mut window, &mut headless, &view, "DockMaximiseButton");
        assert!(!window.exec.workspace.is_maximised(id));
        assert!(window.exec.workspace.is_floating(id), "still floating");
        assert!(same(drawn(&headless, &view), floating), "at the rectangle it floated at");
        assert!(floats(&window, "LayersDock"));
        assert!(same(drawn(&headless, "LayersDock"), panel));
    }

    // "F9 from a floating panel" (docs/desktop.md, "The dock chrome"): F9 is View > Toggle
    // Perspective, which acts on the 3D view whichever view or panel the person clicked last.
    #[test]
    fn a_click_in_a_floating_panel_is_not_a_click_in_the_view_under_it_and_f9s_action_still_reaches_the_3d_view() {
        let mut window = MainWindow::new();
        let mut headless = settled(&mut window);
        let _ = window.exec.run_verb_line("VIEWS OPEN 3d");
        // A frame for the new view to be laid out in, before the window is drawn until idle.
        let _ = headless.frame(&mut window);
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        let workspace = window.exec.workspace.clone();
        let ids = workspace.view_ids();
        assert_eq!(ids.len(), 2, "the premise: VIEWS OPEN 3d opened a second view");
        let render = workspace
            .active_render_view()
            .expect("the premise: one of the two is a 3D view");
        // Layers floated over the plan view, the one that is not the 3D view. Floating Layers
        // leaves the views where they are: Reference Data keeps the left column.
        let plan = ids
            .iter()
            .copied()
            .find(|id| *id != render)
            .expect("the other view");
        let under = drawn(&headless, &dock_name(plan));
        assert!(
            under.width() >= 260.0 && under.height() >= 240.0,
            "the premise: the plan view has room for a small panel: {under:?}"
        );
        let panel = at(under.min.x.round() + 30.0, under.min.y.round() + 40.0, 200.0, 160.0);
        window
            .trigger(&format!(
                "viewPanelPlace:LayersDock:float:{},{},200,160",
                panel.min.x as i32, panel.min.y as i32
            ))
            .expect("floated");
        // The 3D view the active one, whichever the line left active.
        workspace.activate_view(render).expect("activated");
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        assert_eq!(workspace.active_id(), render, "the premise: the 3D view is active");
        assert!(same(drawn(&headless, "LayersDock"), panel), "the premise: the panel is drawn");
        assert!(same(drawn(&headless, &dock_name(plan)), under), "and the plan view under it");

        // A click on the panel's title, over the plan view: the plan view is not pressed.
        click_and_settle(&mut window, &mut headless, title_of(panel));
        assert_eq!(workspace.active_id(), render, "the click was the panel's");

        // What F9 runs still acts on the 3D view.
        let projection = |window: &MainWindow| {
            window
                .exec
                .workspace
                .state(render)
                .expect("the 3D view has its state")
                .camera
                .projection()
        };
        let before = projection(&window);
        window.trigger("viewTogglePerspective").expect("Toggle Perspective");
        assert!(projection(&window) != before, "the 3D view's projection is toggled");

        // That the case is reached: the same click on the plan view where the panel does not
        // cover it does make the plan view the active one.
        let bare = pos2(under.max.x - 20.0, under.max.y - 20.0);
        assert!(!panel.contains(bare));
        click_and_settle(&mut window, &mut headless, bare);
        assert_eq!(workspace.active_id(), plan, "a click on the view itself chooses it");
        // And F9's action reaches the 3D view from there too (`ViewSet::most_recent`).
        let before = projection(&window);
        window.trigger("viewTogglePerspective").expect("Toggle Perspective");
        assert!(projection(&window) != before);
    }
}
