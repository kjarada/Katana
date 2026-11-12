//! The dock chrome's own cases (dock_chrome, over egui_dock): a minimised dock comes back where it
//! was, a maximised view hides the other docked views until it is restored, a floated dock docks
//! back into the layout it left, what something else ends is ended, the bar's optional tools come
//! and go by their priority as the C++'s fitOptionalTools has them, and the bar's buttons and the
//! tray are pressed by name.

use egui::{Rect, UiBuilder, pos2, vec2};
use egui_dock::{DockState, NodeIndex, TabPath};
use katana_window::dock_chrome::{
    ALWAYS, ChromeIcons, ChromeRequest, DockChrome, DockRole, DockTab, DockTitleBar,
    TitleBarButton, WindowButton,
};
use katana_window::drive::Posted;
use katana_window::drive::headless::Headless;

mod dock_chrome {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Tab(&'static str);

    impl DockTab for Tab {
        fn dock_name(&self) -> &str {
            self.0
        }
    }

    /// The Layers panel on the left, a quarter of the width; the two views tabbed on the right.
    fn layout() -> DockState<Tab> {
        let mut state = DockState::new(vec![Tab("View1"), Tab("View2")]);
        let _ =
            state
                .main_surface_mut()
                .split_left(NodeIndex::root(), 0.25, vec![Tab("LayersDock")]);
        state
    }

    fn chrome() -> DockChrome<Tab> {
        let mut chrome = DockChrome::new(ChromeIcons::none());
        chrome
            .install("LayersDock", "Layers", DockRole::Panel)
            .set_title("Layers");
        chrome
            .install("View1", "PlanView", DockRole::View)
            .set_title("Plan 1");
        chrome
            .install("View2", "PlanView", DockRole::View)
            .set_title("Plan 2");
        chrome
    }

    /// Where every tab is.
    fn placed(state: &DockState<Tab>) -> Vec<(TabPath, &'static str)> {
        state
            .iter_all_tabs()
            .map(|(path, tab)| (path, tab.0))
            .collect()
    }

    fn open(state: &DockState<Tab>, name: &str) -> bool {
        state.iter_all_tabs().any(|(_, tab)| tab.0 == name)
    }

    fn screen() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1360.0, 860.0))
    }

    #[test]
    fn a_minimised_dock_goes_to_the_tray_and_comes_back_where_it_was() {
        let mut state = layout();
        let before = placed(&state);
        let mut chrome = chrome();
        chrome.minimise(&mut state, "LayersDock");
        assert!(!open(&state, "LayersDock"));
        assert!(chrome.is_minimised("LayersDock"));
        assert_eq!(chrome.minimised_names(), ["LayersDock"]);
        assert!(chrome.take_changed());
        let tray = chrome.tray_actions();
        assert_eq!(tray.len(), 1);
        assert_eq!(tray[0].name, "RestoreLayersDock");
        assert_eq!(tray[0].text, "Layers");
        assert!(chrome.restore(&mut state, "LayersDock"));
        assert_eq!(placed(&state), before);
        assert!(!chrome.is_minimised("LayersDock"));
        assert!(
            !chrome.restore(&mut state, "LayersDock"),
            "it is not minimised"
        );
    }

    #[test]
    fn a_minimised_dock_whose_neighbours_changed_comes_back_in_the_first_place_there_is() {
        let mut state = layout();
        let mut chrome = chrome();
        chrome.minimise(&mut state, "LayersDock");
        let view2 = state
            .find_tab_from(|tab| tab.0 == "View2")
            .expect("view 2 is open");
        let _ = state.remove_tab(view2);
        assert!(chrome.restore(&mut state, "LayersDock"));
        assert!(open(&state, "LayersDock"));
        assert!(!open(&state, "View2"));
    }

    #[test]
    fn a_maximised_view_hides_the_other_docked_views_until_it_is_restored() {
        let mut state = layout();
        let before = placed(&state);
        let mut chrome = chrome();
        chrome.maximise(&mut state, "View1", screen());
        assert!(chrome.is_maximised("View1"));
        assert!(open(&state, "View1"));
        assert!(!open(&state, "View2"));
        assert!(open(&state, "LayersDock"), "a panel is no view");
        assert!(chrome.unmaximise(&mut state, "View1"));
        assert_eq!(placed(&state), before);
        // A panel cannot be maximised.
        chrome.maximise(&mut state, "LayersDock", screen());
        assert!(!chrome.is_maximised("LayersDock"));
    }

    #[test]
    fn minimising_a_maximised_view_restores_the_views_it_hid_first() {
        let mut state = layout();
        let mut chrome = chrome();
        chrome.maximise(&mut state, "View1", screen());
        chrome.minimise(&mut state, "View1");
        assert!(!chrome.is_maximised("View1"));
        assert!(open(&state, "View2"));
        assert!(!open(&state, "View1"));
    }

    #[test]
    fn a_view_a_maximise_hid_shown_by_something_else_ends_the_maximise() {
        let mut state = layout();
        let mut chrome = chrome();
        chrome.maximise(&mut state, "View1", screen());
        state.push_to_first_leaf(Tab("View2"));
        chrome.refresh(&mut state);
        assert!(!chrome.is_maximised("View1"));
        // And a minimised dock the owner showed again is not minimised any more.
        chrome.minimise(&mut state, "LayersDock");
        state.push_to_first_leaf(Tab("LayersDock"));
        chrome.refresh(&mut state);
        assert!(!chrome.is_minimised("LayersDock"));
    }

    #[test]
    fn a_floated_dock_docks_back_into_the_layout_it_left() {
        let mut state = layout();
        let before = placed(&state);
        let mut chrome = chrome();
        chrome.toggle_floating(&mut state, "LayersDock");
        let path = state
            .find_tab_from(|tab| tab.0 == "LayersDock")
            .expect("floating, still open");
        assert!(!path.surface.is_main());
        chrome.refresh(&mut state);
        assert!(chrome.facts("LayersDock").floating);
        chrome.toggle_floating(&mut state, "LayersDock");
        assert_eq!(placed(&state), before);
        chrome.refresh(&mut state);
        assert!(!chrome.facts("LayersDock").floating);
        // A dock that may not float does not.
        if let Some(bar) = chrome.title_bar_mut("LayersDock") {
            bar.floatable = false;
        }
        chrome.toggle_floating(&mut state, "LayersDock");
        assert_eq!(placed(&state), before);
    }

    #[test]
    fn names_no_installed_dock_carries_are_handed_back() {
        let mut state = layout();
        let mut chrome = chrome();
        let missing = chrome.minimise_named(
            &mut state,
            &[String::from("View2"), String::from("SurveyDock")],
        );
        assert_eq!(missing, ["SurveyDock"]);
        assert_eq!(chrome.minimised_names(), ["View2"]);
    }

    // By hand: a panel's bar is 6 + 18 (its icon) + 4 + 1 + 24 (the title's least) + 6 + three
    // window buttons of 22 and their spacing, 69, + 3 = 131 points at least; a view's with its
    // 34 point kind switcher, four window buttons (92) and two tools always shown (46) is
    // 6 + 34 + 4 + 1 + 24 + 46 + 6 + 92 + 3 = 216. With the title 30 points wide the optional tools
    // have the width less 131 and 12 more (36, its text and the gap, less the least 24): at 200,
    // 57; at 170, 27; at 150, 7. Each tool takes 22 and its spacing, 23.
    #[test]
    fn optional_tools_are_shown_highest_priority_first_while_they_fit() {
        let mut bar = DockTitleBar::new("Layers", DockRole::Panel);
        assert_eq!(bar.minimum_width(), 131.0);
        bar.add_tool(TitleBarButton::new("first", "", "First", ""), 2);
        bar.add_tool(TitleBarButton::new("second", "", "Second", ""), 1);
        assert_eq!(
            bar.minimum_width(),
            131.0,
            "an optional tool is never in it"
        );
        bar.fit(200.0, 30.0);
        assert_eq!(bar.shown_tools(), ["first", "second"]);
        bar.fit(170.0, 30.0);
        assert_eq!(bar.shown_tools(), ["first"]);
        bar.fit(150.0, 30.0);
        assert!(bar.shown_tools().is_empty());
        // A tool not wanted takes no room and is not shown.
        bar.set_tool_wanted("first", false);
        bar.fit(170.0, 30.0);
        assert_eq!(bar.shown_tools(), ["second"]);

        let mut view = DockTitleBar::new("PlanView", DockRole::View);
        view.set_leading(TitleBarButton::new("ViewKindButton", "PlanView", "Plan", "").width(34.0));
        view.add_tool(
            TitleBarButton::new("ViewLayersButton", "", "Layers", ""),
            ALWAYS,
        );
        view.add_tool(
            TitleBarButton::new("ViewZoomExtentsButton", "", "Zoom", ""),
            ALWAYS,
        );
        assert_eq!(view.minimum_width(), 216.0);
        // A higher priority that does not fit keeps a lower one out though it would.
        view.add_tool(TitleBarButton::new("wide", "", "Wide", "").width(60.0), 2);
        view.add_tool(TitleBarButton::new("narrow", "", "Narrow", ""), 1);
        view.fit(285.0, 30.0);
        assert_eq!(
            view.shown_tools(),
            ["ViewLayersButton", "ViewZoomExtentsButton"]
        );
    }

    // By hand, a panel's bar from 0 to 300 at a point a pixel: Close at 300 - 3 - 22 = 275, Float
    // 23 before it at 252, Minimise at 229; the icon's cell at 6; the title from 6 + 18 + 4 + 1 =
    // 29 to 229 - 1 - 6 = 222.
    #[test]
    fn a_bar_lays_its_parts_out_as_the_cpps_row_does() {
        let bar = DockTitleBar::new("Layers", DockRole::Panel);
        let layout = bar.layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 26.0)), 1.0);
        let lefts: Vec<(WindowButton, f32)> = layout
            .buttons
            .iter()
            .map(|(which, at)| (*which, at.min.x))
            .collect();
        assert_eq!(
            lefts,
            [
                (WindowButton::Minimise, 229.0),
                (WindowButton::Float, 252.0),
                (WindowButton::Close, 275.0),
            ]
        );
        assert_eq!(layout.lead.min, pos2(6.0, 2.0));
        assert_eq!(layout.title.min.x, 29.0);
        assert_eq!(layout.title.max.x, 222.0);
        assert_eq!(layout.buttons[0].1.height(), 22.0);
    }

    /// One frame of the window: every dock open drawn in a rectangle of its own with its bar, and
    /// the tray under them.
    fn draw(ui: &mut egui::Ui, chrome: &mut DockChrome<Tab>, state: &mut DockState<Tab>) {
        let names: Vec<&'static str> = state.iter_all_tabs().map(|(_, tab)| tab.0).collect();
        for (at, name) in names.into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(0.0, 120.0 * at as f32), vec2(400.0, 100.0));
            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                chrome.dock_ui(ui, name, |ui| {
                    ui.label(name);
                });
            });
        }
        ui.scope_builder(
            UiBuilder::new().max_rect(Rect::from_min_size(pos2(0.0, 700.0), vec2(800.0, 40.0))),
            |ui| chrome.tray_ui(ui, state),
        );
    }

    #[test]
    fn the_bars_buttons_and_the_tray_are_pressed_by_their_names() {
        let mut state = layout();
        let mut chrome = chrome();
        let mut headless = Headless::new();
        headless
            .settle_ui(|ui| draw(ui, &mut chrome, &mut state))
            .expect("the docks are drawn");
        let frame = headless.snapshot();
        let close = frame
            .control("LayersDock", "DockCloseButton")
            .expect("the panel's bar has Close");
        assert_eq!(close.text, "Close");
        assert!(frame.control("LayersDock", "DockMaximiseButton").is_none());
        assert!(frame.control("View1", "DockMaximiseButton").is_some());
        headless
            .act_ui(
                |ui| draw(ui, &mut chrome, &mut state),
                Posted::press("LayersDock", "DockMinimiseButton"),
            )
            .expect("the press is taken");
        let requests = chrome.take_requests();
        assert_eq!(
            requests,
            [ChromeRequest::Minimise(String::from("LayersDock"))]
        );
        for request in requests {
            assert_eq!(chrome.apply(&mut state, request, screen()), None);
        }
        assert!(!open(&state, "LayersDock"));
        headless
            .settle_ui(|ui| draw(ui, &mut chrome, &mut state))
            .expect("the tray is drawn");
        assert!(headless.snapshot().target("MinimisedToolBar").is_some());
        headless
            .act_ui(
                |ui| draw(ui, &mut chrome, &mut state),
                Posted::press("MinimisedToolBar", "RestoreLayersDock"),
            )
            .expect("the tray's button is taken");
        assert!(open(&state, "LayersDock"));
        // Close is the owner's to carry out.
        headless
            .act_ui(
                |ui| draw(ui, &mut chrome, &mut state),
                Posted::press("View2", "DockCloseButton"),
            )
            .expect("the press is taken");
        let requests = chrome.take_requests();
        assert_eq!(requests, [ChromeRequest::Close(String::from("View2"))]);
        assert_eq!(
            chrome.apply(&mut state, requests[0].clone(), screen()),
            Some(ChromeRequest::Close(String::from("View2")))
        );
    }
}
