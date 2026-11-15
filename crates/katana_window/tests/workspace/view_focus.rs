//! The twins of tests/qt_widgets/test_view_focus.cpp: when the keyboard focus arriving in a view
//! makes it the active view. The person moving the focus is a choice of view; Qt moving it is not -
//! when the focused widget is hidden Qt hands the focus to the next widget in the tab chain, and a
//! window coming back gives it to whichever widget last held it.
//!
//! egui keeps no focus reason, so the window says which way the focus came (view_focus), and the
//! workspace hands it to the view (ViewWorkspace::focus_arrived). Each case gives the view the
//! reason and the widget it came from that Qt gave in the C++ case: a hide moves the focus as Tab
//! does, from a widget no longer visible; a closing menu gives it back with the popup's reason; a
//! window brought forward gives it back with the active window's.

mod view_focus_by_kind {
    use katana_cad::viewport_layout::ViewKind;
    use katana_window::view_focus::{FocusPrevious, FocusReason};

    use crate::support::Site;

    const SHOWN: FocusPrevious = FocusPrevious {
        visible: true,
        enabled: true,
    };
    const HIDDEN: FocusPrevious = FocusPrevious {
        visible: false,
        enabled: true,
    };

    /// Two views side by side: the plan that has the focus, then the view of the kind under test
    /// the tab chain reaches next.
    fn two_views(kind: ViewKind) -> (Site, u32, u32) {
        let mut w = Site::sized(640.0, 320.0);
        let holder = w.active();
        let next = w.views.open_view(w.host.document_mut(), kind, false);
        w.frame();
        // The holder has the focus, by a click.
        w.views.focus_arrived(holder, FocusReason::Mouse, None);
        assert_eq!(w.active(), holder);
        (w, holder, next)
    }

    const KINDS: [ViewKind; 3] = [ViewKind::Plan, ViewKind::Model3D, ViewKind::Section];

    #[test]
    fn a_view_given_the_focus_because_the_focused_view_was_hidden_is_not_activated() {
        for kind in KINDS {
            let (w, holder, next) = two_views(kind);
            // The situation under test: Qt itself moved the focus on to the next view, from the
            // view it hid.
            w.views.focus_arrived(next, FocusReason::Tab, Some(HIDDEN));
            assert_eq!(
                w.active(),
                holder,
                "{kind:?}: Qt's move is not the user choosing that view"
            );
        }
    }

    #[test]
    fn tab_pressed_in_a_view_activates_the_view_the_focus_moves_to() {
        for kind in KINDS {
            let (w, _, next) = two_views(kind);
            w.views.focus_arrived(next, FocusReason::Tab, Some(SHOWN));
            assert_eq!(w.active(), next, "{kind:?}");
        }
    }

    #[test]
    fn focus_given_back_by_a_menu_activates_nothing() {
        for kind in KINDS {
            for reason in [FocusReason::Popup, FocusReason::MenuBar] {
                let (w, holder, next) = two_views(kind);
                w.views.focus_arrived(next, reason, Some(SHOWN));
                assert_eq!(w.active(), holder, "{kind:?}, focus reason {reason:?}");
            }
        }
    }
}

mod view_focus {
    use katana_cad::viewport_layout::ViewKind;
    use katana_window::view_focus::{FocusPrevious, FocusReason};

    use crate::support::Site;

    const HIDDEN: FocusPrevious = FocusPrevious {
        visible: false,
        enabled: true,
    };

    /// Three docked views, as the workspace holds them: a plan, a 3D view and a section; the view
    /// at `index` given the focus by a click.
    fn docked(index: usize) -> (Site, [u32; 3]) {
        let mut w = Site::sized(900.0, 500.0);
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        let ids = [plan, model, section];
        w.views.focus_arrived(ids[index], FocusReason::Mouse, None);
        assert_eq!(w.active(), ids[index]);
        (w, ids)
    }

    #[test]
    fn replacing_the_focused_view_in_its_dock_activates_no_other_view() {
        // A change of kind drops the focused widget; the focus falls to another view by the tab
        // chain, from the widget that went.
        let (mut w, ids) = docked(1);
        assert!(
            w.views
                .set_view_kind(w.host.document_mut(), ids[1], ViewKind::Plan)
                .is_ok()
        );
        w.views.focus_arrived(ids[2], FocusReason::Tab, Some(HIDDEN));
        assert_eq!(w.active(), ids[1]);
    }

    #[test]
    fn removing_the_focused_views_dock_activates_no_other_view() {
        // A close or an arrangement takes the dock away: the view that takes over is the
        // ViewSet's choice, not whichever view the tab chain reaches.
        let (mut w, ids) = docked(1);
        w.press(ids[1], "DockMinimiseButton");
        let chosen = w.active();
        w.views.focus_arrived(ids[2], FocusReason::Tab, Some(HIDDEN));
        w.views.focus_arrived(ids[0], FocusReason::Tab, Some(HIDDEN));
        assert_eq!(w.active(), chosen);
    }

    #[test]
    fn a_window_coming_back_gives_the_focus_back_without_activating_the_view_holding_it() {
        let (w, ids) = docked(0);
        // Another view made active in code, then the main window comes back: Qt gives its focus
        // back to the plan, which held it.
        assert!(w.views.activate_view(ids[2]).is_ok());
        w.views
            .focus_arrived(ids[0], FocusReason::ActiveWindow, None);
        assert_eq!(w.active(), ids[2], "the active view stays active");
    }

    #[test]
    fn bringing_a_floating_views_window_forward_activates_that_view() {
        let (mut w, ids) = docked(0);
        w.press(ids[2], "DockFloatButton");
        assert!(w.views.is_floating(ids[2]));
        // Back to the main window first, in case floating the dock activated it: the move under
        // test is the one that follows.
        assert!(w.views.activate_view(ids[0]).is_ok());
        w.views
            .focus_arrived(ids[2], FocusReason::ActiveWindow, None);
        assert_eq!(w.active(), ids[2]);
    }
}
