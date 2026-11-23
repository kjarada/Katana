//! The views between sessions (docs/desktop.md, "The views between sessions"). Not twins: the C++
//! window kept its docks through Qt's saveState, which the Rust window has no part of.
//!
//! `ViewWorkspace::saved_views` writes which views are open - under the ids they have - what each
//! shows, the link, the active view and the arrangement; `restore_views` opens them again. What a
//! view looks at is a drawing's and is not kept. A record that is not what the writer writes is
//! refused whole and changes nothing.
//!
//! The places expected are worked by hand from the 800 x 600 workspace and its 4 point separator:
//! two halves across are 398 wide at 0 and 402, two halves down 298 high at 0 and 302, and a group
//! of tabs loses the 24 points of its strip.

mod view_session {
    use egui::{Rect, pos2, vec2};
    use katana_cad::section::Section;
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::{LayoutKind, ViewKind};
    use katana_window::view_workspace::MOST_SAVED_VIEWS;

    use crate::support::Site;

    /// Two views side by side, the second active: a record written by hand, not by the writer,
    /// so the reader is held to the format the document states.
    const TWO_VIEWS: &str = r#"{"version":1,"views":[{"id":1,"kind":"plan"},{"id":2,"kind":"3d"}],"active":2,"docked":{"split":"columns","share":0.5,"first":{"views":[1],"front":1},"second":{"views":[2],"front":2}},"floating":[]}"#;

    fn at(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(width, height))
    }

    /// Within the point an odd remainder leaves over.
    fn near(actual: Option<Rect>, expected: Rect) -> bool {
        actual.is_some_and(|actual| {
            (actual.min - expected.min).abs().max_elem() <= 1.0
                && (actual.max - expected.max).abs().max_elem() <= 1.0
        })
    }

    /// Every view's kind, docked place and floating rectangle, in the order opened.
    fn arrangement(w: &Site) -> Vec<(ViewId, ViewKind, Option<Rect>, Option<Rect>)> {
        w.views
            .view_ids()
            .into_iter()
            .map(|id| {
                (
                    id,
                    w.state(id).kind,
                    w.views.dock_rect(id),
                    w.views.float_rect(id),
                )
            })
            .collect()
    }

    /// A new session's workspace with `text` restored into it and drawn.
    fn restored(text: &str) -> Site {
        let mut w = Site::new();
        let count = w
            .views
            .restore_views(w.host.document_mut(), text)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(count, w.views.view_ids().len());
        w.frame();
        w
    }

    /// The plan the workspace starts with on the left, a 3D view over a section on the right.
    fn three_views() -> (Site, [ViewId; 3]) {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        (w, [plan, model, section])
    }

    #[test]
    fn a_record_written_by_hand_opens_its_views_side_by_side_under_their_ids() {
        let w = restored(TWO_VIEWS);
        assert_eq!(w.views.view_ids(), [1, 2]);
        assert_eq!(w.state(1).kind, ViewKind::Plan);
        assert_eq!(w.state(2).kind, ViewKind::Model3D);
        assert_eq!(w.bar(1).title(), "Plan 1");
        assert_eq!(w.bar(2).title(), "3D 1");
        assert_eq!(w.active(), 2);
        assert!(w.bar(2).is_active() && !w.bar(1).is_active());
        assert!(near(w.views.dock_rect(1), at(0.0, 0.0, 398.0, 600.0)));
        assert!(near(w.views.dock_rect(2), at(402.0, 0.0, 398.0, 600.0)));
        assert!(w.views.on_screen(1) && w.views.on_screen(2));
    }

    #[test]
    fn the_saved_views_come_back_under_their_ids_in_the_places_they_had() {
        let (mut a, [plan, model, section]) = three_views();
        assert!(a.views.activate_view(model).is_ok());
        a.frame();
        let text = a.views.saved_views();

        let b = restored(&text);
        assert_eq!(b.views.view_ids(), [plan, model, section]);
        assert_eq!(arrangement(&b), arrangement(&a));
        assert_eq!(b.active(), model);
        // Opening left a half and two quarters: the splits and their shares are back.
        assert!(near(b.views.dock_rect(plan), at(0.0, 0.0, 398.0, 600.0)));
        assert!(near(b.views.dock_rect(model), at(402.0, 0.0, 398.0, 298.0)));
        assert!(near(b.views.dock_rect(section), at(402.0, 302.0, 398.0, 298.0)));
        // What was restored saves as what was saved.
        assert_eq!(b.views.saved_views(), text);
        // An id is never reused: the next view opened is the fourth.
        let mut b = b;
        assert_eq!(b.open(ViewKind::Elevation), 4);
    }

    #[test]
    fn a_group_of_tabs_its_page_in_front_and_a_floating_view_come_back_as_they_were() {
        let mut a = Site::new();
        let plan = a.active();
        let model = a.open(ViewKind::Model3D);
        let section = a.open(ViewKind::Section);
        let elevation = a.open(ViewKind::Elevation);
        // Two places for four views: the plan on the left, the other three tabbed on the right.
        a.views
            .arrange(a.host.document_mut(), LayoutKind::SplitVertical);
        a.frame();
        // The elevation out of the group into a window of its own, at a place of the test's.
        a.views.toggle_floating(elevation);
        let window = at(150.0, 70.0, 320.0, 240.0);
        assert!(a.views.set_float_rect(elevation, window));
        a.views.bring_to_front(section);
        a.frame();
        assert!(a.views.on_screen(section) && !a.views.on_screen(model));
        let text = a.views.saved_views();

        let b = restored(&text);
        assert_eq!(b.views.view_ids(), [plan, model, section, elevation]);
        assert!(b.views.is_floating(elevation));
        assert_eq!(b.views.float_rect(elevation), Some(window));
        assert_eq!(b.active(), elevation, "floating it made it the active view");
        assert!(near(b.views.dock_rect(plan), at(0.0, 0.0, 398.0, 600.0)));
        // The group, under its strip of tabs, the section the page shown.
        assert!(near(b.views.dock_rect(section), at(402.0, 24.0, 398.0, 576.0)));
        assert!(near(b.views.dock_rect(model), at(402.0, 24.0, 398.0, 576.0)));
        assert!(b.views.on_screen(plan) && b.views.on_screen(section));
        assert!(!b.views.on_screen(model), "the 3D view is the page behind");
        assert_eq!(arrangement(&b), arrangement(&a));
        assert_eq!(b.views.saved_views(), text);
    }

    #[test]
    fn a_tiling_the_link_and_a_views_ghost_switch_are_kept() {
        let mut a = Site::new();
        let plan = a.active();
        let model = a.open(ViewKind::Model3D);
        let second = a.open(ViewKind::Plan);
        assert_eq!([plan, model, second], [1, 2, 3]);
        let _ = a.run("VIEWS LINK 1,3");
        let _ = a.run("VIEWS SET 2 ghosts=off");
        assert_eq!(
            a.run("VIEWS TILE COLUMNS"),
            "tiled=3 kind=columns docked=0 minimised=0"
        );
        let text = a.views.saved_views();

        let b = restored(&text);
        assert!(b.state(plan).linked && b.state(second).linked);
        assert!(!b.state(model).linked);
        assert!(!b.state(model).selection_ghosts);
        assert!(b.state(plan).selection_ghosts && b.state(second).selection_ghosts);
        // Three across 800 points with two separators: 264 each, the view that was active - the
        // second plan, opened last - first, then the others as opened.
        assert!(near(b.views.dock_rect(second), at(0.0, 0.0, 264.0, 600.0)));
        assert!(near(b.views.dock_rect(plan), at(268.0, 0.0, 264.0, 600.0)));
        assert!(near(b.views.dock_rect(model), at(536.0, 0.0, 264.0, 600.0)));
        assert_eq!(arrangement(&b), arrangement(&a));
    }

    #[test]
    fn a_link_of_one_view_is_not_kept() {
        // A link of one waits for a second click, which a new session has not been given.
        let mut a = Site::new();
        let plan = a.active();
        let _ = a.open(ViewKind::Model3D);
        let _ = a.run("VIEWS LINK 1");
        assert!(a.state(plan).linked);
        let b = restored(&a.views.saved_views());
        assert!(!b.state(plan).linked);
    }

    #[test]
    fn a_minimise_and_a_maximise_are_not_kept_every_view_comes_back_open_in_its_place() {
        let (mut a, [plan, model, section]) = three_views();
        let open = arrangement(&a);
        a.press(section, "DockMinimiseButton");
        a.views.toggle_maximised(plan);
        a.frame();
        assert!(a.views.is_minimised(section) && a.views.is_maximised(plan));

        let text = a.views.saved_views();
        // Saving ends both in the session saved, which is closing.
        assert!(a.views.tray_actions().is_empty());
        assert!(!a.views.is_maximised(plan));

        let b = restored(&text);
        for id in [plan, model, section] {
            assert!(!b.views.is_minimised(id) && !b.views.is_maximised(id));
            assert!(b.views.on_screen(id), "view {id} is seen");
        }
        assert!(b.views.tray_actions().is_empty());
        assert_eq!(arrangement(&b), open);
    }

    #[test]
    fn where_a_view_looks_the_layers_it_hides_and_its_section_are_a_drawings_and_are_not_kept() {
        let (mut a, [plan, _model, section]) = three_views();
        a.add_layers(&["ROAD"]);
        a.place(plan, 400, 300, 123456.0, 654321.0, 7.25);
        a.with_state(plan, |view| {
            let _ = view.layers.hide("ROAD");
        });
        a.with_state(section, |view| view.section = Some(Section::default()));
        assert!(a.state(plan).plan_framed && a.state(plan).layers.hides("ROAD"));
        let text = a.views.saved_views();
        assert!(!text.contains("ROAD"), "{text}");

        let mut b = Site::new();
        b.add_layers(&["ROAD"]);
        assert!(
            b.views
                .restore_views(b.host.document_mut(), &text)
                .is_ok()
        );
        // The plan is this session's own view 1, looking where it looked and hiding nothing:
        // nothing of where the other session's looked has come with the record.
        let looks = b.state(plan).plan;
        assert_ne!(looks.center.x, 123456.0);
        assert_ne!(looks.center.y, 654321.0);
        assert_ne!(looks.scale, 7.25);
        assert!(b.state(plan).layers.hidden().is_empty());
        // The section view is a section view, with nothing cut.
        assert_eq!(b.state(section).kind, ViewKind::Section);
        assert!(b.state(section).section.is_none());
    }

    #[test]
    fn a_view_open_now_that_the_record_names_is_kept_as_it_is_and_every_other_is_closed() {
        let (mut b, [plan, model, section]) = three_views();
        b.add_layers(&["ROAD"]);
        b.with_state(plan, |view| {
            let _ = view.layers.hide("ROAD");
        });
        // Views 1 and 2 of the record: the plan stays the view it is, the 3D view too; the
        // section, which the record does not name, goes.
        let count = b
            .views
            .restore_views(b.host.document_mut(), TWO_VIEWS)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        b.frame();
        assert_eq!(count, 2);
        assert_eq!(b.views.view_ids(), [plan, model]);
        assert!(b.views.state(section).is_none());
        assert!(b.state(plan).layers.hides("ROAD"), "its own layers are still its own");
        assert!(near(b.views.dock_rect(plan), at(0.0, 0.0, 398.0, 600.0)));
        assert!(near(b.views.dock_rect(model), at(402.0, 0.0, 398.0, 600.0)));
    }

    #[test]
    fn a_view_the_record_names_under_another_kind_changes_what_it_shows() {
        let (mut b, [plan, _model, _section]) = three_views();
        let text = r#"{"version":1,"views":[{"id":1,"kind":"3d"},{"id":4,"kind":"plan"}],"active":4,"docked":{"views":[1,4],"front":4},"floating":[]}"#;
        assert!(b.views.restore_views(b.host.document_mut(), text).is_ok());
        b.frame();
        assert_eq!(b.views.view_ids(), [1, 4]);
        assert_eq!(b.state(plan).kind, ViewKind::Model3D);
        assert_eq!(b.bar(plan).title(), "3D 1");
        assert_eq!(b.state(4).kind, ViewKind::Plan);
        assert_eq!(b.active(), 4);
        // One place, two pages: the plan in front.
        assert!(b.views.on_screen(4) && !b.views.on_screen(plan));
    }

    #[test]
    fn an_id_this_session_has_already_given_out_is_not_given_to_a_restored_view() {
        // View 2 was a 3D view of this session, closed since: an event still queued for it must
        // not find the record's view 2, which is opened under the next id instead.
        let mut b = Site::new();
        let model = b.open(ViewKind::Model3D);
        assert_eq!(model, 2);
        assert!(b.views.close_view(b.host.document_mut(), model).is_ok());
        b.frame();

        assert!(
            b.views
                .restore_views(b.host.document_mut(), TWO_VIEWS)
                .is_ok()
        );
        b.frame();
        assert_eq!(b.views.view_ids(), [1, 3]);
        assert_eq!(b.state(3).kind, ViewKind::Model3D);
        // In the place the record gave its view 2, and active as it was.
        assert!(near(b.views.dock_rect(3), at(402.0, 0.0, 398.0, 600.0)));
        assert_eq!(b.active(), 3);
    }

    #[test]
    fn a_damaged_record_is_refused_saying_what_is_wrong_and_changes_nothing() {
        let deep = {
            // Forty splits, one inside the other: deeper than thirty-two views can make.
            let mut node = String::from(r#"{"views":[1],"front":1}"#);
            for _ in 0..40 {
                node = format!(
                    r#"{{"split":"rows","share":0.5,"first":{node},"second":{{"views":[2],"front":2}}}}"#
                );
            }
            format!(
                r#"{{"version":1,"views":[{{"id":1,"kind":"plan"}},{{"id":2,"kind":"3d"}}],"docked":{node}}}"#
            )
        };
        let cases: Vec<(String, &str)> = vec![
            (String::from("the views of yesterday"), "not JSON"),
            (String::from("[]"), "there is no version"),
            (
                TWO_VIEWS.replace(r#""version":1"#, r#""version":2"#),
                "a version this build does not read",
            ),
            (
                TWO_VIEWS.replace(r#""version":1"#, r#""version":"one""#),
                "there is no version",
            ),
            (
                String::from(r#"{"version":1,"views":[],"docked":null}"#),
                "the list of views is empty",
            ),
            (
                String::from(r#"{"version":1,"docked":null}"#),
                "there is no list of views",
            ),
            (
                TWO_VIEWS.replace(r#""kind":"3d""#, r#""kind":"isometric""#),
                "a view shows plan, 3d, section or elevation",
            ),
            (
                TWO_VIEWS.replace(r#"{"id":2,"#, r#"{"id":0,"#),
                "a view id is a whole number from 1",
            ),
            (
                TWO_VIEWS.replace(r#"{"id":2,"#, r#"{"id":-2,"#),
                "a view id is a whole number from 1",
            ),
            (
                TWO_VIEWS.replace(r#"{"id":2,"#, r#"{"id":1,"#),
                "a view id is named twice",
            ),
            (
                TWO_VIEWS.replace(r#""share":0.5"#, r#""share":1.5"#),
                "a split's share is a number between 0 and 1",
            ),
            (
                TWO_VIEWS.replace(r#""share":0.5"#, r#""share":0"#),
                "a split's share is a number between 0 and 1",
            ),
            (
                TWO_VIEWS.replace(r#""share":0.5,"#, ""),
                "a split's share is a number between 0 and 1",
            ),
            (
                TWO_VIEWS.replace(r#""split":"columns""#, r#""split":"diagonal""#),
                "a split is columns or rows",
            ),
            (
                TWO_VIEWS.replace(r#","second":{"views":[2],"front":2}"#, ""),
                "a split has a first and a second part",
            ),
            (
                TWO_VIEWS.replace(r#""views":[2],"front":2"#, r#""views":[9],"front":9"#),
                "the arrangement places a view the list does not name",
            ),
            (
                TWO_VIEWS.replace(r#""views":[2],"front":2"#, r#""views":[1],"front":1"#),
                "the arrangement places a view twice",
            ),
            (
                TWO_VIEWS.replace(r#""views":[2],"front":2"#, r#""views":[],"front":2"#),
                "a place holds at least one view",
            ),
            (
                String::from(
                    r#"{"version":1,"views":[{"id":1,"kind":"plan"},{"id":2,"kind":"3d"}],"docked":{"views":[1],"front":1}}"#,
                ),
                "a view of the list has no place in the arrangement",
            ),
            (
                String::from(
                    r#"{"version":1,"views":[{"id":1,"kind":"plan"},{"id":2,"kind":"3d"}],"docked":{"views":[1],"front":1},"floating":[{"view":2,"x":10,"y":10,"width":5,"height":5}]}"#,
                ),
                "a floating view is smaller than a view can be",
            ),
            (
                String::from(
                    r#"{"version":1,"views":[{"id":1,"kind":"plan"},{"id":2,"kind":"3d"}],"docked":{"views":[1],"front":1},"floating":[{"view":2,"x":"left","y":10,"width":300,"height":200}]}"#,
                ),
                "a floating view has x, y, width and height in points",
            ),
            (deep, "nested deeper than its views allow"),
        ];

        let (mut w, [plan, _model, section]) = three_views();
        w.press(section, "DockFloatButton");
        assert!(w.views.activate_view(plan).is_ok());
        w.frame();
        let before = arrangement(&w);
        for (text, why) in &cases {
            let refused = w
                .views
                .restore_views(w.host.document_mut(), text)
                .expect_err(why);
            assert!(
                refused.describe().contains(why),
                "{why}: {}",
                refused.describe()
            );
            w.frame();
            assert_eq!(arrangement(&w), before, "{why}");
            assert_eq!(w.active(), plan, "{why}");
        }
    }

    #[test]
    fn a_front_page_or_an_active_view_the_record_does_not_have_is_the_first_ones() {
        // Neither is a reason to refuse an arrangement that is otherwise whole.
        let text = r#"{"version":1,"views":[{"id":1,"kind":"plan"},{"id":2,"kind":"3d"}],"active":7,"docked":{"views":[1,2],"front":7}}"#;
        let w = restored(text);
        assert_eq!(w.active(), 1);
        assert!(w.views.on_screen(1) && !w.views.on_screen(2));
    }

    /// A record of `count` plan views under the ids 1 to `count`, tabbed in one place.
    fn record_of(count: usize) -> String {
        let views: Vec<String> = (1..=count)
            .map(|id| format!(r#"{{"id":{id},"kind":"plan"}}"#))
            .collect();
        let ids: Vec<String> = (1..=count).map(|id| format!("{id}")).collect();
        format!(
            r#"{{"version":1,"views":[{}],"active":1,"docked":{{"views":[{}],"front":1}}}}"#,
            views.join(","),
            ids.join(",")
        )
    }

    #[test]
    fn a_record_naming_more_views_than_a_session_restores_is_refused_whole() {
        assert_eq!(MOST_SAVED_VIEWS, 32);
        let mut w = Site::new();
        let refused = w
            .views
            .restore_views(w.host.document_mut(), &record_of(33))
            .expect_err("thirty-three views are one too many");
        let why = refused.describe();
        assert!(why.contains("33") && why.contains("32"), "{why}");
        w.frame();
        assert_eq!(w.views.view_ids(), [1], "the one plan view it started with");

        // And the most it restores, it does.
        let count = w
            .views
            .restore_views(w.host.document_mut(), &record_of(32))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        w.frame();
        assert_eq!(count, 32);
        let ids = w.views.view_ids();
        assert_eq!(ids.len(), 32);
        assert_eq!(ids.first().copied().zip(ids.last().copied()), Some((1, 32)));
        assert!(ids.iter().all(|&id| w.views.is_plan_view(id)));
        assert!(w.views.on_screen(1));
    }

    #[test]
    fn a_workspace_with_no_view_open_saves_a_record_that_restores_nothing() {
        // A person who closed every view gets the one plan view of a new window next time.
        let mut a = Site::new();
        let plan = a.active();
        assert!(a.views.close_view(a.host.document_mut(), plan).is_ok());
        let text = a.views.saved_views();

        let mut b = Site::new();
        let refused = b
            .views
            .restore_views(b.host.document_mut(), &text)
            .expect_err("no view is nothing to restore");
        assert!(
            refused.describe().contains("the list of views is empty"),
            "{}",
            refused.describe()
        );
        assert_eq!(b.views.view_ids(), [1]);
    }
}

/// The window's own part (main_window::layout): the views saved beside the layout as the window
/// closes, and restored before it is first shown.
mod window_view_session {
    use egui::Rect;
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::ViewKind;
    use katana_window::MainWindow;
    use katana_window::drive::DrivenWindow;
    use katana_window::drive::headless::Headless;
    use katana_window::main_window::layout;

    /// The window drawn until it is idle, and the runner that drew it: one runner a window, so
    /// every frame of it is drawn in the one context.
    fn settled(window: &mut MainWindow) -> Headless {
        let mut headless = Headless::new();
        settle(&mut headless, window);
        headless
    }

    fn settle(headless: &mut Headless, window: &mut MainWindow) {
        headless
            .settle(window)
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// Every view's kind and docked place, in the order opened.
    fn arrangement(window: &MainWindow) -> Vec<(ViewId, Option<ViewKind>, Option<Rect>)> {
        let workspace = &window.exec.workspace;
        workspace
            .view_ids()
            .into_iter()
            .map(|id| {
                (
                    id,
                    workspace.state(id).map(|state| state.kind),
                    workspace.dock_rect(id),
                )
            })
            .collect()
    }

    #[test]
    fn the_window_saves_its_views_beside_its_layout_and_a_new_window_restores_them() {
        let mut first = MainWindow::new();
        assert!(first.exec.run_command("VIEWS OPEN 3d"));
        assert!(first.exec.run_command("VIEWS OPEN section"));
        let mut headless = settled(&mut first);
        assert!(first.exec.run_command("VIEWS TILE ROWS"));
        settle(&mut headless, &mut first);
        let saved = arrangement(&first);
        assert_eq!(saved.len(), 3);

        first.save_session();
        let views = first
            .exec
            .settings
            .text(layout::VIEWS_KEY)
            .map(String::from)
            .expect("the views are in the settings");
        // Beside the layout, under a key of their own: the layout's text is as it was.
        let panels = first
            .exec
            .settings
            .text(layout::LAYOUT_KEY)
            .map(String::from)
            .expect("the layout is in the settings");
        assert_eq!(panels, layout::layout_text(&first));

        let mut second = MainWindow::new();
        second.exec.settings.set_text(layout::VIEWS_KEY, &views);
        layout::restore_saved_views(&mut second);
        let _ = settled(&mut second);
        assert_eq!(arrangement(&second), saved);
        assert_eq!(
            second.exec.workspace.active_id(),
            first.exec.workspace.active_id()
        );
        // Stacked: three places one above the other, each the drawing area's width.
        let places: Vec<Rect> = saved.iter().filter_map(|(_, _, place)| *place).collect();
        assert_eq!(places.len(), 3);
        assert!(places.iter().all(|place| place.width() == places[0].width()));
        assert!(!second.exec.log.text().contains("were not restored"));
    }

    #[test]
    fn settings_saved_before_the_views_were_kept_start_with_the_one_plan_view() {
        let mut window = MainWindow::new();
        assert!(window.exec.settings.text(layout::VIEWS_KEY).is_none());
        layout::restore_saved_views(&mut window);
        assert_eq!(window.exec.workspace.view_ids(), [1]);
        assert_eq!(
            window.exec.workspace.state(1).map(|state| state.kind),
            Some(ViewKind::Plan)
        );
        assert!(!window.exec.log.text().contains("were not restored"));
    }

    #[test]
    fn restoring_a_layout_leaves_the_views_as_they_are() {
        // The layout's own checks restore layouts into a running window (--check-toolbar-fit):
        // the views are no part of it.
        let mut window = MainWindow::new();
        assert!(window.exec.run_command("VIEWS OPEN 3d"));
        let text = layout::layout_text(&window);
        assert!(!text.contains("\"views\""), "{text}");
        assert!(layout::restore_layout(&mut window, &text));
        assert_eq!(window.exec.workspace.view_ids(), [1, 2]);
    }

    #[test]
    fn a_damaged_views_record_leaves_the_one_plan_view_and_says_so_in_the_log() {
        let mut window = MainWindow::new();
        window
            .exec
            .settings
            .set_text(layout::VIEWS_KEY, r#"{"version":1,"views":[]}"#);
        layout::restore_saved_views(&mut window);
        let _ = settled(&mut window);
        assert_eq!(window.exec.workspace.view_ids(), [1]);
        let log = window.exec.log.text();
        assert!(
            log.contains("The views of the last session were not restored"),
            "{log}"
        );
        assert!(log.contains("the list of views is empty"), "{log}");
    }

    #[test]
    fn a_headless_window_closing_writes_no_views() {
        let mut window = MainWindow::new();
        window.set_headless(true);
        assert!(window.exec.run_command("VIEWS OPEN 3d"));
        window.close();
        assert!(window.closed());
        assert!(window.exec.settings.text(layout::VIEWS_KEY).is_none());
        assert!(window.exec.settings.text(layout::LAYOUT_KEY).is_none());
    }
}
