//! The cases of tests/qt_widgets/tools/test_tool_menus.cpp and test_plan_context_menu.cpp that
//! drive the workspace (tool_bars/tool_menus.rs and host/plan_context_menu.rs twin the rest): one
//! tool at a time in the workspace, in its active plan view; Esc, Enter and typed text as the
//! workspace hands them on; a tool ended, and said so, when its view changes kind or closes; and
//! a plan view's shortcut menu reaching the window's taker, or cancelling without one.
//!
//! The C++ hooks the window sets are the workspace's events (`take_events`), read after each call.

mod tool_menus {
    use katana_cad::viewport_layout::ViewKind;
    use katana_commands::{EntityAttributes, create_line};
    use katana_core::ErrorCode;
    use katana_geometry::Point2;
    use katana_window::view_workspace::{Scenery, WorkspaceEvent};
    use katana_window::viewport_widget::{KeyModifiers, ViewKey};

    use crate::support::Site;

    /// The tools started ("id") and ended ("") since the last look.
    fn tool_changes(w: &Site) -> Vec<String> {
        w.views
            .take_events()
            .into_iter()
            .filter_map(|event| match event {
                WorkspaceEvent::ActiveToolChanged(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_workspace_starts_a_tool_in_its_active_plan_view_only() {
        let mut w = Site::plain(800.0, 600.0);
        let _ = w.views.take_events();
        assert!(w.views.start_tool(w.host.document_mut(), "draw.circle").is_ok());
        assert_eq!(w.views.active_tool_id(), "draw.circle");
        let plan = w.views.active_plan_view().expect("a plan view");
        let running = w
            .views
            .with_plan_view(plan, |view, _| view.active_tool_id())
            .unwrap_or_default();
        assert_eq!(running, "draw.circle");
        let mut changes = tool_changes(&w);

        // The command line's Enter hands its line to the running tool: the centre, then the
        // radius. Circle restarts for the next one.
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "3,4"));
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "2"));
        assert_eq!(w.document().model().entities.size(), 1);

        let refused = w.views.start_tool(w.host.document_mut(), "draw.nothing");
        assert_eq!(refused.expect_err("no such tool").code, ErrorCode::NotFound);
        w.views.stop_tool(w.host.document_mut());
        assert_eq!(w.views.active_tool_id(), "");
        assert!(
            !w.views.type_into_tool(&mut w.host, Scenery::default(), "1,1"),
            "no tool: the line is the command line's"
        );
        changes.extend(tool_changes(&w));
        assert!(!changes.is_empty());
        assert_eq!(changes.first().map(String::as_str), Some("draw.circle"));
        assert_eq!(changes.last().map(String::as_str), Some(""));
    }

    #[test]
    fn esc_in_the_workspace_ends_the_tool_before_it_clears_the_selection() {
        // Two plan views, a Move running in one on a selection. The first Esc ends the Move and
        // nothing else; the second clears the selection.
        let mut w = Site::plain(800.0, 600.0);
        let made = w.document_mut().execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            EntityAttributes::default(),
        ));
        assert!(made.is_ok());
        let line = w.document().last_created_entities()[0];
        w.document_mut().selection_mut().add(line);
        let _ = w.views.open_view(w.host.document_mut(), ViewKind::Plan, true);
        assert_eq!(w.views.plan_views().len(), 2);
        assert!(w.views.start_tool(w.host.document_mut(), "modify.move").is_ok());

        w.views.cancel(w.host.document_mut());
        assert_eq!(w.views.active_tool_id(), "");
        assert!(!w.document().selection().empty());
        w.views.cancel(w.host.document_mut());
        assert!(w.document().selection().empty());
    }

    #[test]
    fn enter_in_an_idle_view_repeats_the_last_tool_with_only_one_tool_running() {
        // Line ran in plan view A and ended; Circle now runs in plan view B. Enter in A repeats
        // the last tool the person ran - Circle - through the workspace, which stops B's first.
        let mut w = Site::plain(800.0, 600.0);
        assert!(w.views.start_tool(w.host.document_mut(), "draw.line").is_ok());
        w.views.cancel(w.host.document_mut()); // Line with no point: nothing kept
        let _ = w.views.open_view(w.host.document_mut(), ViewKind::Plan, true);
        let plans = w.views.plan_views();
        assert_eq!(plans.len(), 2);
        let (a, b) = (plans[0], plans[1]);
        assert_eq!(
            w.views.active_plan_view(),
            Some(b),
            "a view opened is the active one"
        );
        assert!(w.views.start_tool(w.host.document_mut(), "draw.circle").is_ok());
        let active = |w: &Site, id| {
            w.views
                .with_plan_view(id, |view, _| view.tool_active())
                .unwrap_or(false)
        };
        assert!(active(&w, b));

        w.views
            .with_plan_view_lent(w.host.document_mut(), a, |plan, state, cx| {
                plan.key_press(cx, state, ViewKey::Enter, KeyModifiers::NONE, "")
            });

        assert_eq!(plans.iter().filter(|&&id| active(&w, id)).count(), 1);
        let running = w
            .views
            .with_plan_view(a, |view, _| view.active_tool_id())
            .unwrap_or_default();
        assert_eq!(running, "draw.circle");
        assert!(!active(&w, b));
        assert_eq!(
            w.views.active_plan_view(),
            Some(a),
            "the view Enter was pressed in"
        );
        assert_eq!(w.views.active_tool_id(), "draw.circle");
    }

    #[test]
    fn enter_on_an_empty_command_line_finishes_the_running_tool_and_then_repeats_the_last() {
        // By hand: LINE, 0,0 and 10,0 typed, then Enter - one segment, so ONE line is made and
        // the tool says so ("1 line") and starts again; Enter at its first prompt ends it; Enter
        // with nothing running starts Line again.
        let mut w = Site::plain(800.0, 600.0);
        let _ = w.views.take_events();
        assert!(w.views.start_tool(w.host.document_mut(), "draw.line").is_ok());
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "0,0"));
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "10,0"));
        assert!(w.views.press_enter(w.host.document_mut()));
        assert_eq!(w.document().model().entities.size(), 1);
        let messages: Vec<String> = w
            .views
            .take_events()
            .into_iter()
            .filter_map(|event| match event {
                WorkspaceEvent::ToolMessage(message) => Some(message),
                _ => None,
            })
            .collect();
        assert!(!messages.is_empty());
        assert_eq!(messages.last().map(String::as_str), Some("1 line"));
        assert_eq!(
            w.views.active_tool_id(),
            "draw.line",
            "Line starts again for the next chain"
        );

        assert!(w.views.press_enter(w.host.document_mut()));
        assert_eq!(w.views.active_tool_id(), "", "Enter at the first prompt ends it");
        assert_eq!(w.document().model().entities.size(), 1);

        assert!(w.views.press_enter(w.host.document_mut()));
        assert_eq!(
            w.views.active_tool_id(),
            "draw.line",
            "Enter with none running repeats it"
        );
    }

    #[test]
    fn text_typed_over_the_drawing_reaches_the_workspace_only_when_no_tool_runs() {
        let mut w = Site::plain(800.0, 600.0);
        let _ = w.views.take_events();
        let plan = w.views.active_plan_view().expect("a plan view");
        let typed = |w: &Site| -> String {
            w.views
                .take_events()
                .into_iter()
                .filter_map(|event| match event {
                    WorkspaceEvent::TextTyped(text) => Some(text),
                    _ => None,
                })
                .collect()
        };
        let mut seen = String::new();

        w.views
            .with_plan_view_lent(w.host.document_mut(), plan, |view, state, cx| {
                view.key_press(cx, state, ViewKey::Other, KeyModifiers::NONE, "l")
            });
        seen += &typed(&w);
        assert_eq!(seen, "l");

        assert!(w.views.start_tool(w.host.document_mut(), "draw.circle").is_ok());
        w.views
            .with_plan_view_lent(w.host.document_mut(), plan, |view, state, cx| {
                view.key_press(cx, state, ViewKey::Other, KeyModifiers::NONE, "5")
            });
        seen += &typed(&w);
        assert_eq!(seen, "l", "the tool's input never reaches the command line");
        let input = w
            .views
            .with_plan_view(plan, |view, _| String::from(view.typed_input()))
            .unwrap_or_default();
        assert_eq!(input, "5");
    }

    #[test]
    fn a_plan_view_turned_into_another_kind_ends_its_tool_and_says_so() {
        let mut w = Site::plain(800.0, 600.0);
        let _ = w.views.take_events();
        assert!(w.views.start_tool(w.host.document_mut(), "draw.line").is_ok());
        assert_eq!(tool_changes(&w).last().map(String::as_str), Some("draw.line"));

        let active = w.active();
        assert!(
            w.views
                .set_view_kind(w.host.document_mut(), active, ViewKind::Section)
                .is_ok()
        );
        assert_eq!(tool_changes(&w).last().map(String::as_str), Some(""));
        assert_eq!(w.views.active_tool_id(), "");
    }

    #[test]
    fn a_plan_view_closed_while_its_tool_runs_ends_the_tool_and_says_so() {
        let mut w = Site::plain(800.0, 600.0);
        let _ = w.views.open_view(w.host.document_mut(), ViewKind::Plan, true);
        assert_eq!(w.views.plan_views().len(), 2);
        let _ = w.views.take_events();
        assert!(w.views.start_tool(w.host.document_mut(), "draw.line").is_ok());
        assert_eq!(tool_changes(&w).last().map(String::as_str), Some("draw.line"));
        let running = w.active();
        let tool = w
            .views
            .with_plan_view(running, |view, _| view.active_tool_id())
            .unwrap_or_default();
        assert_eq!(tool, "draw.line");

        assert!(w.views.close_view(w.host.document_mut(), running).is_ok());
        assert_eq!(tool_changes(&w).last().map(String::as_str), Some(""));
        assert_eq!(w.views.active_tool_id(), "");
        assert_eq!(w.views.plan_views().len(), 1);
    }
}

mod plan_view_shortcut_menu {
    use katana_commands::{EntityAttributes, create_line};
    use katana_geometry::Point2;
    use katana_window::canvas::PointF;
    use katana_window::view_workspace::WorkspaceEvent;
    use katana_window::viewport_widget::{KeyModifiers, MouseButton};

    use crate::support::Site;

    fn menus(w: &Site) -> usize {
        w.views
            .take_events()
            .into_iter()
            .filter(|event| matches!(event, WorkspaceEvent::ContextMenu { .. }))
            .count()
    }

    #[test]
    fn the_workspace_gives_the_menu_from_a_view_made_before_its_hook_was_set() {
        // The workspace makes its first plan view as it is made, before the window says it takes
        // the shortcut menu: the view must still reach it.
        let mut w = Site::plain(800.0, 600.0);
        let _ = w.views.take_events();
        w.views.set_context_menu_taker(true);
        let plan = w.views.active_plan_view().expect("a plan view");
        let right_click = |w: &mut Site| {
            w.views
                .with_plan_view_lent(w.host.document_mut(), plan, |view, state, cx| {
                    let (width, height) = view.size();
                    let middle = PointF::new(f64::from(width) / 2.0, f64::from(height) / 2.0);
                    view.mouse_press(cx, state, middle, MouseButton::Right, KeyModifiers::NONE);
                });
        };
        right_click(&mut w);
        let mut count = menus(&w);
        assert_eq!(count, 1);

        // Unset, a right-click cancels as it always did: the selection goes.
        w.views.set_context_menu_taker(false);
        let made = w.document_mut().execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            EntityAttributes::default(),
        ));
        assert!(made.is_ok());
        let line = w.document().last_created_entities()[0];
        w.document_mut().selection_mut().set(vec![line]);
        right_click(&mut w);
        count += menus(&w);
        assert_eq!(count, 1);
        assert!(w.document().selection().empty());
    }
}
