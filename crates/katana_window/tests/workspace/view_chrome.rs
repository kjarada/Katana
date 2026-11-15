//! The twins of tests/qt_widgets/test_view_chrome.cpp: which view is the active one once the dock
//! chrome has minimised, restored, maximised or closed views of the workspace; where the views go
//! when one opens or an arrangement is made; the bar's optional tools; and a view's own layers.
//!
//! The C++ presses a view's own Minimise and Maximise (`Site::press`), the tray button
//! (`restore_view`, the tray being the window's to draw) and calls the chrome's maximise
//! (`toggle_maximised`). A tab clicked is `bring_to_front`. Rectangles are the docks' in points from
//! the workspace's top left, as Qt's geometry is from its parent's.

mod view_chrome {
    use std::collections::BTreeSet;

    use egui::{Color32, Rect};
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::{LayoutKind, ViewKind};
    use katana_window::dock_chrome::{BAR_HEIGHT, BUTTON_SIZE, DockTitleBar, TITLE_GAP, TITLE_ROOM};
    use katana_window::drive::registry::ControlKind;
    use katana_window::view_workspace::dock_name;

    use crate::support::Site;

    const ZOOM_TOOLS: [&str; 3] = [
        "ViewZoomInButton",
        "ViewZoomOutButton",
        "ViewZoomSelectionButton",
    ];

    fn minimise(w: &mut Site, id: ViewId) {
        w.press(id, "DockMinimiseButton");
    }

    fn restore_from_tray(w: &mut Site, id: ViewId) {
        assert!(w.views.restore_view(id), "view {id} was in the tray");
        w.frame();
    }

    fn bring_to_front(w: &mut Site, id: ViewId) {
        w.views.bring_to_front(id);
        w.frame();
    }

    fn rect(w: &Site, id: ViewId) -> Rect {
        // What the view was instead, so a run that fails says which way it left the dock area
        // (docs/rust.md, Not done: this twin's intermittent failure).
        w.views.dock_rect(id).unwrap_or_else(|| {
            panic!(
                "view {id} is docked (maximised={} minimised={} floating={} on_screen={})",
                w.views.is_maximised(id),
                w.views.is_minimised(id),
                w.views.is_floating(id),
                w.views.on_screen(id)
            )
        })
    }

    /// Which of a bar's optional zoom tools it shows, by object name.
    fn shown_zoom_tools(bar: &DockTitleBar) -> Vec<String> {
        let shown = bar.shown_tools();
        ZOOM_TOOLS
            .iter()
            .filter(|name| shown.iter().any(|tool| tool == *name))
            .map(|name| String::from(*name))
            .collect()
    }

    /// The width the bar's title is drawn at: its text in the bar's font.
    fn title_width(w: &Site, id: ViewId) -> f32 {
        let ctx = w.harness.context();
        let font = egui::TextStyle::Body.resolve(&ctx.global_style());
        let title = String::from(w.bar(id).title());
        ctx.fonts_mut(|fonts| fonts.layout_no_wrap(title, font, Color32::WHITE).size().x)
    }

    /// What a plan view's bar should show of the three, worked from the rule DockTitleBar::add_tool
    /// states: Zoom to Selection (priority 2), then In and Out (1, a pair), each only while it fits
    /// beside the bar's least width with the title left the room to be read whole - its text's
    /// width and TITLE_GAP, from the title's least (24) to TITLE_ROOM. Each tool is BUTTON_SIZE (20
    /// since the design of 2026-11-16; the C++'s 22) and the row's 1 of spacing.
    fn zoom_tools_by_the_rule(bar: &DockTitleBar, width: f32, text: f32) -> Vec<String> {
        const TITLE_LEAST: f32 = 24.0;
        let title_room = (text + TITLE_GAP).clamp(TITLE_LEAST, TITLE_ROOM);
        let room = width - bar.minimum_width() - (title_room - TITLE_LEAST);
        const TOOL: f32 = BUTTON_SIZE + 1.0;
        if room < TOOL {
            return Vec::new();
        }
        if room - TOOL < 2.0 * TOOL {
            return vec![String::from("ViewZoomSelectionButton")];
        }
        ZOOM_TOOLS.iter().map(|name| String::from(*name)).collect()
    }

    fn rule_for(w: &Site, id: ViewId) -> Vec<String> {
        zoom_tools_by_the_rule(&w.bar(id), w.dock_width(id), title_width(w, id))
    }

    #[test]
    fn restoring_a_view_from_the_tray_ends_a_maximise_made_while_it_was_away() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        // open_view split the plan into halves side by side, which differ by at most the pixel an
        // odd remainder leaves over.
        let plan_width = w.dock_width(plan);
        let model_width = w.dock_width(model);
        assert!((plan_width - model_width).abs() <= 1.0);

        minimise(&mut w, model);
        w.views.toggle_maximised(plan);
        w.frame();
        assert!(w.views.is_maximised(plan));
        restore_from_tray(&mut w, model);

        // The 3D view is back beside the plan, so the plan fills the workspace no longer.
        assert!(!w.views.is_maximised(plan));
        assert!(w.views.on_screen(plan));
        assert!(w.views.on_screen(model));
        // Exactly where they were before the minimise.
        assert_eq!(w.dock_width(plan), plan_width);
        assert_eq!(w.dock_width(model), model_width);
    }

    #[test]
    fn minimising_the_active_view_hands_on_to_the_tab_page_on_screen_not_one_parked_behind_it() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        // Two places for three views: the plan on the left, the 3D view and the section tabbed on
        // the right.
        w.views.arrange(w.host.document_mut(), LayoutKind::SplitVertical);
        w.frame();
        bring_to_front(&mut w, section);
        assert!(w.views.on_screen(section));
        assert!(
            !w.views.on_screen(model),
            "the 3D tab is behind the section, parked off the window"
        );
        assert!(w.views.activate_view(plan).is_ok());

        minimise(&mut w, plan);

        // The 3D view was opened first, but only the section is on screen.
        assert_eq!(w.active(), section);
    }

    #[test]
    fn minimising_the_current_tab_leaves_the_view_made_active_on_screen() {
        let mut w = Site::new();
        let _ = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        // One place for three views: all three tabbed together.
        w.views.arrange(w.host.document_mut(), LayoutKind::Single);
        w.frame();
        bring_to_front(&mut w, section);
        assert!(w.views.activate_view(section).is_ok());
        assert!(w.views.on_screen(section));

        minimise(&mut w, section);

        // Whichever view was made active has to be the page that shows.
        assert_ne!(w.active(), section);
        assert!(w.views.on_screen(w.active()));
    }

    #[test]
    fn closing_the_active_view_never_hands_on_to_a_minimised_view() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        assert!(w.views.activate_view(model).is_ok());
        minimise(&mut w, model);
        // The first view on screen in the order they were opened took over.
        assert_eq!(w.active(), plan);

        assert!(w.views.close_view(w.host.document_mut(), plan).is_ok());
        w.frame();

        // The 3D view was used more recently than the section, but it is in the tray.
        assert_eq!(w.active(), section);
        assert!(w.views.on_screen(section));
    }

    #[test]
    fn closing_the_active_tab_hands_on_to_the_view_used_before_it_and_brings_it_to_the_front() {
        let mut w = Site::new();
        let plan = w.active();
        let _ = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        // All three tabbed, in the order plan, 3D, section.
        w.views.arrange(w.host.document_mut(), LayoutKind::Single);
        w.frame();
        // The plan used, then the section: the plan is the one used before it.
        bring_to_front(&mut w, plan);
        assert!(w.views.activate_view(plan).is_ok());
        bring_to_front(&mut w, section);
        assert!(w.views.activate_view(section).is_ok());

        assert!(w.views.close_view(w.host.document_mut(), section).is_ok());
        w.frame();

        // ViewSet hands on to the plan; the tab bar would show the section's neighbour.
        assert_eq!(w.active(), plan);
        assert!(w.views.on_screen(plan));
    }

    #[test]
    fn a_view_brought_back_from_the_tray_is_active_when_the_active_view_is_still_in_it() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        minimise(&mut w, model);
        assert_eq!(w.active(), plan);
        // No view left to hand on to: the plan stays active, in the tray.
        minimise(&mut w, plan);
        assert_eq!(w.active(), plan);

        restore_from_tray(&mut w, model);

        assert_eq!(w.active(), model);
        assert!(w.views.on_screen(model));
    }

    #[test]
    fn a_view_opened_while_every_view_is_minimised_is_active() {
        let mut w = Site::new();
        let plan = w.active();
        minimise(&mut w, plan);
        assert_eq!(w.active(), plan);

        // What "show the 3D view" does: it opens one without asking for it to be active.
        let model = w.views.ensure_view(w.host.document_mut(), ViewKind::Model3D);
        w.frame();

        assert_eq!(w.active(), model);
        assert!(w.views.on_screen(model));
    }

    #[test]
    fn pressing_maximise_on_a_view_that_is_not_active_makes_it_the_active_view() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        assert_eq!(w.active(), model);

        w.press(plan, "DockMaximiseButton");

        assert!(w.views.is_maximised(plan));
        assert!(!w.views.on_screen(model));
        assert_eq!(w.active(), plan);
    }

    #[test]
    fn pressing_a_views_zoom_extents_makes_it_the_active_view() {
        let mut w = Site::new();
        let plan = w.active();
        let _ = w.open(ViewKind::Model3D);
        assert!(w.bar(plan).tool_mut("ViewZoomExtentsButton").is_some());

        w.press(plan, "ViewZoomExtentsButton");

        assert_eq!(w.active(), plan);
    }

    #[test]
    fn the_title_bar_shows_optional_tools_only_where_they_fit_and_no_two_overlap() {
        let mut w = Site::new();
        let plan = w.active();
        for window_width in [250.0, 300.0, 600.0] {
            w.resize(window_width, 400.0);
            let width = w.dock_width(plan);
            let expected = rule_for(&w, plan);
            assert_eq!(shown_zoom_tools(&w.bar(plan)), expected, "bar {width} px");
            // Each step of the fit is reached: nothing at 250, Selection alone at 300, everything
            // at 600.
            let steps = if window_width == 250.0 {
                0
            } else if window_width == 300.0 {
                1
            } else {
                3
            };
            assert_eq!(expected.len(), steps, "bar {width} px");

            // Every button shown lies in the bar and clear of every other.
            let dock = w.views.dock_rect(plan).expect("docked");
            let bar = Rect::from_min_size(dock.min, egui::vec2(dock.width(), BAR_HEIGHT));
            let snapshot = w.harness.snapshot();
            let shown: Vec<(String, Rect)> = snapshot
                .controls
                .iter()
                .filter(|control| control.target == dock_name(plan))
                .filter(|control| {
                    control.visible
                        && matches!(control.kind, ControlKind::Button | ControlKind::Toggle)
                })
                .map(|control| (control.name.clone(), control.rect))
                .collect();
            assert!(shown.len() >= 7, "{shown:?}");
            for (i, (name, at)) in shown.iter().enumerate() {
                assert!(
                    bar.expand(0.01).contains_rect(*at),
                    "{name} leaves the {width} px bar"
                );
                for (other, other_at) in &shown[i + 1..] {
                    assert!(
                        !at.shrink(0.01).intersects(*other_at),
                        "{name} overlaps {other} at {width} px"
                    );
                }
            }
            // Where an optional tool was fitted, the title between the kind button and the first
            // tool still has the room to be read whole.
            if !expected.is_empty() {
                let kind = shown
                    .iter()
                    .find(|(name, _)| name == "ViewKindButton")
                    .map(|(_, at)| *at)
                    .expect("the kind button");
                let link = shown
                    .iter()
                    .find(|(name, _)| name == "ViewLinkButton")
                    .map(|(_, at)| *at)
                    .expect("the link button");
                let text = title_width(&w, plan);
                assert!(link.min.x - kind.max.x >= text, "bar {width} px");
            }
        }
    }

    #[test]
    fn optional_tools_never_widen_the_docks_minimum() {
        let mut w = Site::new();
        let plan = w.active();
        w.resize(900.0, 400.0);
        assert!(shown_zoom_tools(&w.bar(plan)).contains(&String::from("ViewZoomInButton")));
        let wide = w.bar(plan).minimum_width();
        w.resize(300.0, 400.0);
        assert!(!shown_zoom_tools(&w.bar(plan)).contains(&String::from("ViewZoomInButton")));
        // The least width does not depend on which optional tools are shown, so a view can always
        // be narrowed past them.
        assert_eq!(w.bar(plan).minimum_width(), wide);
    }

    #[test]
    fn a_view_turned_into_a_plan_shows_the_tools_a_plan_opened_at_its_width_shows() {
        let mut seen: BTreeSet<Vec<String>> = BTreeSet::new();
        for window_width in (500..=760).step_by(5) {
            let window_width = window_width as f32;
            // The window sized first: a view opened splits the active one into halves of what it
            // has then, so both windows split alike.
            let mut turned = Site::sized(window_width, 400.0);
            let model = turned.open(ViewKind::Model3D);
            assert!(
                turned
                    .views
                    .set_view_kind(turned.host.document_mut(), model, ViewKind::Plan)
                    .is_ok()
            );
            turned.frame();

            let mut opened = Site::sized(window_width, 400.0);
            let plan = opened.open(ViewKind::Plan);

            assert_eq!(
                turned.dock_width(model),
                opened.dock_width(plan),
                "window {window_width} px"
            );
            assert_eq!(
                shown_zoom_tools(&turned.bar(model)),
                shown_zoom_tools(&opened.bar(plan)),
                "window {window_width} px"
            );
            assert_eq!(
                shown_zoom_tools(&opened.bar(plan)),
                rule_for(&opened, plan),
                "window {window_width} px"
            );
            seen.insert(shown_zoom_tools(&opened.bar(plan)));
        }
        // The widths reach every step of the fit: none of the three, Selection alone, all three.
        assert!(seen.contains(&Vec::new()));
        assert!(seen.contains(&vec![String::from("ViewZoomSelectionButton")]));
        assert!(seen.contains(&ZOOM_TOOLS.iter().map(|name| String::from(*name)).collect::<Vec<_>>()));
    }

    #[test]
    fn an_always_shown_tool_no_longer_wanted_gives_its_room_to_the_optional_ones_at_once() {
        // Link taken off (a view whose kind has none), its 23 px go to the optional tools in the
        // same call, as the rule counts them - and stay so, the bar's size being unchanged.
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for window_width in (250..=360).step_by(2) {
            let mut w = Site::sized(window_width as f32, 400.0);
            let plan = w.active();
            assert!(w.bar(plan).tool_mut("ViewLinkButton").is_some());
            let before = shown_zoom_tools(&w.bar(plan)).len();
            let width = w.dock_width(plan);
            let text = title_width(&w, plan);

            w.views.with_title_bar(plan, |bar| {
                bar.set_tool_wanted("ViewLinkButton", false);
                bar.fit(width, text);
            });

            assert!(!w.bar(plan).shown_tools().contains(&String::from("ViewLinkButton")));
            assert_eq!(
                shown_zoom_tools(&w.bar(plan)),
                rule_for(&w, plan),
                "a {width} px bar"
            );
            w.frame();
            assert_eq!(
                shown_zoom_tools(&w.bar(plan)),
                rule_for(&w, plan),
                "a {width} px bar, after the events"
            );
            if shown_zoom_tools(&w.bar(plan)).len() > before {
                seen.insert(before);
            }
        }
        // Widths at which Link's room let Selection in, and In and Out.
        assert!(seen.contains(&0));
        assert!(seen.contains(&1));
    }

    #[test]
    fn opening_a_view_splits_it_into_equal_halves_whatever_tools_its_bar_shows() {
        let mut w = Site::sized(560.0, 400.0);
        let plan = w.active();
        assert_eq!(
            shown_zoom_tools(&w.bar(plan)).len(),
            3,
            "the lone plan's bar shows all three"
        );
        let whole = rect(&w, plan);

        let model = w.open(ViewKind::Model3D);

        assert!(
            shown_zoom_tools(&w.bar(plan)).len() < 3,
            "a half's bar has no room for all three"
        );
        let left = rect(&w, plan);
        let right = rect(&w, model);
        assert_eq!(left.width() + w.views.separator() + right.width(), whole.width());
        assert!(
            (left.width() - right.width()).abs() <= 1.0,
            "plan {} px, 3D {} px",
            left.width(),
            right.width()
        );
    }

    #[test]
    fn quad_puts_the_views_in_quarters_whatever_tools_their_bars_show() {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for window_width in [560.0, 600.0, 660.0] {
            let mut w = Site::sized(window_width, 500.0);
            let plan = w.active();
            w.views.arrange(w.host.document_mut(), LayoutKind::Quad);
            w.frame();
            assert_eq!(w.views.view_set().borrow().size(), 4);
            let area = w.views.rect();
            let half_width = ((area.width() - w.views.separator()) / 2.0).floor();
            for id in [plan, plan + 1, plan + 2, plan + 3] {
                assert!(
                    (w.dock_width(id) - half_width).abs() <= 1.0,
                    "view {id} is {} px of {}",
                    w.dock_width(id),
                    area.width()
                );
            }
            assert_eq!(
                shown_zoom_tools(&w.bar(plan)),
                rule_for(&w, plan),
                "a {} px bar (window {window_width} px)",
                w.dock_width(plan)
            );
            seen.insert(rule_for(&w, plan).len());
        }
        // A quarter with room for all three, and one without.
        assert!(seen.contains(&3));
        assert!(seen.contains(&0) || seen.contains(&1));
    }

    #[test]
    fn opening_a_view_splits_the_active_view_into_equal_halves_along_its_longer_side() {
        let mut w = Site::new();
        let plan = w.active();
        let whole = rect(&w, plan);
        // The lone plan fills the workspace: wider than it is tall.
        assert_eq!(whole, w.views.rect());
        assert!(whole.width() > whole.height());
        let gap = w.views.separator();

        let model = w.open(ViewKind::Model3D);
        let left = rect(&w, plan);
        let right = rect(&w, model);
        assert_eq!(left.min, whole.min);
        assert_eq!(left.width() + gap + right.width(), whole.width());
        assert!((left.width() - right.width()).abs() <= 1.0);
        assert_eq!(right.min.x, left.width() + gap);
        assert_eq!(left.height(), whole.height());
        assert_eq!(right.height(), whole.height());

        // The 3D view, now active, is taller than it is wide, so the section goes under it.
        assert!(right.height() > right.width());
        let section = w.open(ViewKind::Section);
        let top = rect(&w, model);
        let bottom = rect(&w, section);
        assert_eq!(top.min, right.min);
        assert_eq!(top.height() + gap + bottom.height(), right.height());
        assert!(
            (top.height() - bottom.height()).abs() <= 1.0,
            "3D {} px, section {} px",
            top.height(),
            bottom.height()
        );
        assert_eq!(bottom.min.y, top.height() + gap);
        assert_eq!(top.width(), right.width());
        assert_eq!(bottom.width(), right.width());
        // The plan beside them is untouched.
        assert_eq!(rect(&w, plan), left);
    }

    // Not a twin: the cause of the twin below failing about one run in three. egui_dock's
    // retain_tabs balanced its emptied leaves in a std HashSet's order, random from one set to the
    // next, and in one order it moved the 3D view's split to the root without its children: the
    // view maximised dropped out of the dock. Each Site's maximise empties the plan's leaf and the
    // section's at once; 32 of them meet both orders (dock_chrome::remove_tabs).
    #[test]
    fn maximising_a_view_beside_a_split_keeps_it_in_the_dock_in_any_hash_order() {
        for round in 0..32 {
            let mut w = Site::new();
            let model = w.open(ViewKind::Model3D);
            let _section = w.open(ViewKind::Section);
            w.press(model, "DockMaximiseButton");
            assert_eq!(
                w.views.dock_rect(model),
                Some(w.views.rect()),
                "round {round}: the view maximised fills the workspace"
            );
        }
    }

    #[test]
    fn a_maximised_view_fills_the_workspace_and_restore_gives_every_view_back_its_place() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        let plan_before = rect(&w, plan);
        let model_before = rect(&w, model);
        let section_before = rect(&w, section);

        w.press(model, "DockMaximiseButton");
        assert_eq!(rect(&w, model), w.views.rect());
        assert!(!w.views.on_screen(plan));
        assert!(!w.views.on_screen(section));

        // The same button, now Restore.
        w.press(model, "DockMaximiseButton");
        assert!(!w.views.is_maximised(model));
        assert_eq!(rect(&w, plan), plan_before);
        assert_eq!(rect(&w, model), model_before);
        assert_eq!(rect(&w, section), section_before);
    }

    #[test]
    fn quad_puts_the_four_views_in_the_quarters_of_the_workspace() {
        let mut w = Site::new();
        let plan = w.active();
        w.views.arrange(w.host.document_mut(), LayoutKind::Quad);
        w.frame();
        // Quad opens the kinds it expects in the preset's order - 3D, section, elevation, ids 2, 3
        // and 4 after the plan's - and splits them as dock_splits(Quad) says.
        assert_eq!(w.views.view_set().borrow().size(), 4);
        let (model, section, elevation) = (plan + 1, plan + 2, plan + 3);
        let area = w.views.rect();
        let gap = w.views.separator();
        let half_width = ((area.width() - gap) / 2.0).floor();
        let half_height = ((area.height() - gap) / 2.0).floor();
        let near = |actual: Rect, x: f32, y: f32, width: f32, height: f32| {
            (actual.min.x - x).abs() <= 1.0
                && (actual.min.y - y).abs() <= 1.0
                && (actual.width() - width).abs() <= 1.0
                && (actual.height() - height).abs() <= 1.0
        };
        assert!(near(rect(&w, plan), 0.0, 0.0, half_width, half_height));
        assert!(near(rect(&w, model), half_width + gap, 0.0, half_width, half_height));
        assert!(near(rect(&w, section), 0.0, half_height + gap, half_width, half_height));
        assert!(near(
            rect(&w, elevation),
            half_width + gap,
            half_height + gap,
            half_width,
            half_height
        ));
    }

    #[test]
    fn hiding_a_layer_in_a_views_popup_hides_it_in_that_view_alone_and_is_no_edit() {
        let mut w = Site::new();
        w.add_layers(&["BUILDING", "ROAD"]);
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let modified = w.document().is_modified();
        let undoable = w.document().history().undo_count();

        assert!(w.views.show_layers_popup(w.host.document(), None, plan));
        let Site { views, host, .. } = &mut w;
        views
            .with_layers_popup(|popup, views| {
                assert!(popup.item_for("BUILDING").is_some());
                // What a click on its box does.
                popup.set_layer_checked(views, host, "BUILDING", false);
            })
            .expect("the popup is open");

        assert!(w.state(plan).layers.hides("BUILDING"));
        assert!(!w.state(plan).layers.hides("ROAD"));
        assert!(!w.state(model).layers.hides("BUILDING"));
        assert_eq!(w.views.hidden_count(plan), 1);
        assert_eq!(w.views.hidden_count(model), 0);
        // A view's own filter: no command ran, so nothing to save or undo.
        assert_eq!(w.document().is_modified(), modified);
        assert_eq!(w.document().history().undo_count(), undoable);
    }

    #[test]
    fn isolating_a_node_that_is_not_a_layer_hides_the_siblings_of_it_and_of_each_ancestor() {
        let mut w = Site::new();
        w.add_layers(&["ANNOT", "design/road", "design/surface/tin1", "survey/points"]);
        let plan = w.active();

        assert!(w.views.show_layers_popup(w.host.document(), None, plan));
        let Site { views, host, .. } = &mut w;
        let isolated = views
            .with_layers_popup(|popup, views| {
                // design/surface is a node of the tree only: no layer has that name.
                assert!(popup.item_for("design/surface").is_some());
                popup.set_current(Some("design/surface"));
                popup.isolate_selected(views, host)
            })
            .expect("the popup is open");
        assert!(isolated.is_ok());

        // Kept: design/surface, what is under it (tin1) and its ancestor design. Hidden: design's
        // siblings at the root - the default layer 0, ANNOT and survey - and surface's sibling
        // under design, design/road.
        let expected: BTreeSet<String> = ["0", "ANNOT", "design/road", "survey"]
            .iter()
            .map(|name| String::from(*name))
            .collect();
        assert_eq!(*w.state(plan).layers.hidden(), expected);
    }
}
