//! The twins of tests/qt_widgets/test_view_selection.cpp: the selection in every view (the owner's
//! request of 2026-09-30, "when selecting a feature in 2d it gets highlighted in other views"). A
//! feature selected in the design view shows, faint, in the as-built view that hides its layer, in
//! plan and in 3D, and each view's own switch (VIEWS SET <id> ghosts=on|off, the Layers popup's
//! box) turns that off. The workspace's executor runs each line through the workspace on the
//! drawing's interpreter, as the window's does.

mod view_selection {
    use katana_cad::section::{SectionOptions, extract_section};
    use katana_cad::selection_style::{GHOST_COLOR3D, SELECTION_COLOR};
    use katana_cad::view_set::ViewId;
    use katana_cad::viewport_layout::ViewKind;
    use katana_commands::set_entity_visible;
    use katana_entity::{EntityId, Geometry};
    use katana_geometry::{Point2, Polyline2};
    use katana_window::canvas::PointF;
    use katana_window::view_workspace::Scenery;
    use katana_window::viewport_widget::{KeyModifiers, MouseButton};

    use crate::support::Site;

    /// A design line from (10, 10) to (40, 30) and an as-built line from (10, 30) to (40, 10), the
    /// design one selected; view 1 hides the as-built layer and view 2 the design one.
    struct DesignAndAsBuilt {
        w: Site,
        design: EntityId,
        as_built: EntityId,
    }

    impl DesignAndAsBuilt {
        fn new() -> Self {
            let mut w = Site::sized(900.0, 600.0);
            w.add_layers(&["design"]);
            let design = w.line(Point2::new(10.0, 10.0), Point2::new(40.0, 30.0), Some("design"));
            w.add_layers(&["asbuilt"]);
            let as_built = w.line(
                Point2::new(10.0, 30.0),
                Point2::new(40.0, 10.0),
                Some("asbuilt"),
            );
            w.frame();
            let _ = w.open(ViewKind::Plan);
            let _ = w.run("VIEWS HIDE 1 asbuilt");
            let _ = w.run("VIEWS HIDE 2 design");
            w.document_mut().selection_mut().add(design);
            w.document().notify_selection_changed();
            w.frame();
            Self {
                w,
                design,
                as_built,
            }
        }

        fn ghosts(&self, id: ViewId) -> usize {
            self.w
                .views
                .with_plan_view(id, |plan, _| plan.last_ghost_count())
                .expect("a plan view")
        }

        fn drawn(&self, id: ViewId) -> usize {
            self.w
                .views
                .with_plan_view(id, |plan, _| plan.last_drawn_entity_count())
                .expect("a plan view")
        }

        fn grips(&self, id: ViewId) -> usize {
            self.w
                .views
                .with_plan_view(id, |plan, _| plan.grip_count())
                .expect("a plan view")
        }

        /// A section cut along y = 20 from x = 0 to 50, shown: its view's id.
        fn section(&mut self) -> ViewId {
            let cut = Polyline2::new(vec![Point2::new(0.0, 20.0), Point2::new(50.0, 20.0)], false);
            let section =
                extract_section(&cut, &[], Some(self.w.document().model()), &SectionOptions::default())
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(section.crossings.len(), 2);
            assert!(
                self.w
                    .views
                    .show_section(self.w.host.document_mut(), section, None)
            );
            self.w.frame();
            self.w
                .views
                .view_set()
                .borrow()
                .views()
                .last()
                .map(|state| state.id)
                .expect("a view")
        }

        /// The section painted: drawn, hidden, selected and ghost crossings.
        fn counts(&mut self, id: ViewId) -> [usize; 4] {
            let document = self.w.host.document();
            self.w
                .views
                .with_section_view(id, |view, state| {
                    let _ = view.paint(state, Some(document));
                    [
                        view.last_drawn_crossing_count(),
                        view.last_hidden_crossing_count(),
                        view.last_selected_crossing_count(),
                        view.last_ghost_crossing_count(),
                    ]
                })
                .expect("a section view")
        }
    }

    #[test]
    fn a_feature_selected_in_the_design_view_is_a_ghost_in_the_as_built_plan_view() {
        let mut d = DesignAndAsBuilt::new();
        d.w.paint(1);
        d.w.paint(2);
        // Each view draws its own line; the design line, selected, is drawn in view 1 and ghosted
        // in view 2.
        assert_eq!(d.drawn(1), 1);
        assert_eq!(d.ghosts(1), 0);
        assert_eq!(d.drawn(2), 1);
        assert_eq!(d.ghosts(2), 1);
        // The as-built view's switch off: nothing of the design line there.
        assert!(d.w.run("VIEWS SET 2 ghosts=off").contains(" ghosts=off"));
        d.w.paint(2);
        assert_eq!(d.ghosts(2), 0);
        assert_eq!(d.drawn(2), 1);
        // Deselected, no ghost even with the switch on again.
        let _ = d.w.run("VIEWS SET 2 ghosts=on");
        d.w.document_mut().selection_mut().clear();
        d.w.document().notify_selection_changed();
        d.w.paint(2);
        assert_eq!(d.ghosts(2), 0);
    }

    #[test]
    fn a_feature_selected_in_plan_is_a_faint_line_in_the_three_d_view_that_hides_its_layer() {
        let mut d = DesignAndAsBuilt::new();
        let model = d.w.open(ViewKind::Model3D);
        let _ = d.w.run(&format!("VIEWS HIDE {model} design"));
        let layers = |d: &mut DesignAndAsBuilt| {
            let document = d.w.host.document();
            d.w.views
                .with_render_view(model, document, Scenery::default(), |view, scene, state| {
                    let _ = view.paint(scene, state);
                    view.scene_layers().clone()
                })
                .expect("a 3D view")
        };
        let painted = layers(&mut d);
        assert_eq!(painted.selection.lines.len(), 1);
        assert_eq!(
            painted.selection.colors[painted.selection.lines[0].a as usize],
            GHOST_COLOR3D
        );
        assert!(painted.selection_casing.lines.is_empty(), "a ghost has no casing");
        let _ = d.w.run(&format!("VIEWS SET {model} ghosts=off"));
        assert!(layers(&mut d).selection.lines.is_empty());
        // Shown in that view again, the line is the selection's core and casing.
        let _ = d.w.run(&format!("VIEWS SHOW {model} ALL"));
        let painted = layers(&mut d);
        assert_eq!(painted.selection.lines.len(), 1);
        assert_eq!(
            painted.selection.colors[painted.selection.lines[0].a as usize],
            SELECTION_COLOR
        );
        assert_eq!(painted.selection_casing.lines.len(), 1);
    }

    #[test]
    fn the_layers_popups_box_runs_views_set_through_the_runner() {
        let mut d = DesignAndAsBuilt::new();
        assert!(d.w.views.show_layers_popup(d.w.host.document(), None, 2));
        let checked = {
            let Site { views, host, .. } = &mut d.w;
            views
                .with_layers_popup(|popup, views| {
                    assert!(popup.ghosts_checked(), "a view ghosts the selection when it opens");
                    popup.click_ghosts(views, host);
                    popup.ghosts_checked()
                })
                .expect("the popup is open")
        };
        assert!(!d.w.ran().is_empty());
        assert_eq!(d.w.ran().last().map(String::as_str), Some("VIEWS SET 2 ghosts=off"));
        assert!(!d.w.state(2).selection_ghosts);
        assert!(d.w.state(1).selection_ghosts, "only that view");
        assert!(!checked);
        d.w.paint(2);
        assert_eq!(d.ghosts(2), 0);

        let checked = {
            let Site { views, host, .. } = &mut d.w;
            views
                .with_layers_popup(|popup, views| {
                    popup.click_ghosts(views, host);
                    popup.ghosts_checked()
                })
                .expect("the popup is open")
        };
        assert_eq!(d.w.ran().last().map(String::as_str), Some("VIEWS SET 2 ghosts=on"));
        assert!(d.w.state(2).selection_ghosts);
        assert!(checked);
    }

    #[test]
    fn the_layers_popups_box_follows_a_views_set_typed_while_it_is_closed() {
        let mut d = DesignAndAsBuilt::new();
        let _ = d.w.run("VIEWS SET 2 ghosts=off");
        assert!(d.w.views.show_layers_popup(d.w.host.document(), None, 2));
        let checked = d
            .w
            .views
            .with_layers_popup(|popup, _| popup.ghosts_checked())
            .expect("the popup is open");
        assert!(!checked);
    }

    #[test]
    fn a_section_marks_the_crossing_of_the_feature_selected_in_plan() {
        // The cut crosses the design line and the as-built line at station 25, each crossing
        // naming its entity.
        let mut d = DesignAndAsBuilt::new();
        let id = d.section();
        let [drawn, _, selected, _] = d.counts(id);
        assert_eq!(selected, 1, "the design line's, selected in plan");
        assert_eq!(drawn, 2);
        // The section hiding the design layer: its crossing is a ghost.
        let _ = d.w.run(&format!("VIEWS HIDE {id} design"));
        let [drawn, _, selected, ghosts] = d.counts(id);
        assert_eq!(selected, 0);
        assert_eq!(ghosts, 1);
        assert_eq!(drawn, 1);
        // A selection change reaches the section without a cut.
        d.w.document_mut().selection_mut().clear();
        d.w.document().notify_selection_changed();
        let [_, _, _, ghosts] = d.counts(id);
        assert_eq!(ghosts, 0);
    }

    #[test]
    fn a_selected_feature_the_drawing_hides_is_marked_in_no_view() {
        let mut d = DesignAndAsBuilt::new();
        let id = d.section();
        let [_, _, selected, _] = d.counts(id);
        d.w.paint(2);
        assert_eq!(selected, 1);
        assert_eq!(d.ghosts(2), 1);

        // The design line made invisible, and selected still.
        let design = d.design;
        assert!(
            d.w.document_mut()
                .execute(set_entity_visible(vec![design], false))
                .is_ok()
        );
        assert!(d.w.document().selection().contains(design));
        let [_, _, selected, ghosts] = d.counts(id);
        d.w.paint(2);
        assert_eq!(selected, 0);
        assert_eq!(ghosts, 0);
        assert_eq!(d.ghosts(2), 0);
        // Nor a ghost in a section that hides its layer.
        let _ = d.w.run(&format!("VIEWS HIDE {id} design"));
        let [_, _, _, ghosts] = d.counts(id);
        assert_eq!(ghosts, 0);
        let _ = d.w.run(&format!("VIEWS SHOW {id} ALL"));
        // Visible again; then its layer switched off in the drawing.
        assert!(d.w.document_mut().undo().is_ok());
        let [_, _, selected, _] = d.counts(id);
        assert_eq!(selected, 1);
        let _ = d.w.run("LAYER HIDE design");
        assert!(d.w.document().selection().contains(design));
        let [_, _, selected, ghosts] = d.counts(id);
        d.w.paint(2);
        assert_eq!(selected, 0);
        assert_eq!(ghosts, 0);
        assert_eq!(d.ghosts(2), 0);
    }

    #[test]
    fn a_section_hides_and_marks_a_crossing_by_the_layer_its_entity_is_on_now() {
        let mut d = DesignAndAsBuilt::new();
        let id = d.section();
        let _ = d.w.run(&format!("VIEWS HIDE {id} asbuilt"));
        // drawn, hidden, selected, ghosts
        assert_eq!(d.counts(id), [1, 1, 1, 0], "the design line drawn and selected");
        // The design line, selected, moved onto the layer the section hides: its crossing hidden,
        // and a ghost - once.
        let _ = d.w.run("CHLAYER asbuilt");
        let design = d.design;
        assert_eq!(
            d.w.document().model().entities.find(design).map(|e| e.layer.clone()),
            Some(String::from("asbuilt"))
        );
        assert_eq!(d.counts(id), [0, 2, 0, 1]);
        // Back, and the as-built line selected and moved off that layer: drawn, not hidden, and
        // marked selected.
        assert!(d.w.document_mut().undo().is_ok());
        assert_eq!(d.counts(id), [1, 1, 1, 0]);
        let as_built = d.as_built;
        d.w.document_mut().selection_mut().set(vec![as_built]);
        d.w.document().notify_selection_changed();
        let _ = d.w.run("CHLAYER design");
        assert_eq!(
            d.w.document().model().entities.find(as_built).map(|e| e.layer.clone()),
            Some(String::from("design"))
        );
        assert_eq!(d.counts(id), [2, 0, 1, 0]);
    }

    #[test]
    fn a_ghost_offers_no_grip_so_a_drag_where_it_crosses_the_as_built_line_edits_nothing() {
        let mut d = DesignAndAsBuilt::new();
        let _ = d.w.run("ZOOM WINDOW 0,0,50,40 view=2");
        d.w.paint(1);
        d.w.paint(2);
        assert_eq!(d.grips(1), 3, "the design line's two ends and middle");
        assert_eq!(d.grips(2), 0);
        assert_eq!(d.ghosts(2), 1, "drawn as a ghost there, all the same");
        let _ = d.w.run("VIEWS SHOW 2 design");
        d.w.paint(2);
        assert_eq!(d.grips(2), 3, "shown there again, it is edited there again");
        let _ = d.w.run("VIEWS HIDE 2 design");
        d.w.paint(2);
        assert_eq!(d.grips(2), 0);
        let undoable = d.w.document().history().undo_count();
        let design = d.design;
        let segment = |d: &DesignAndAsBuilt| match d
            .w
            .document()
            .model()
            .entities
            .find(design)
            .map(|entity| entity.geometry.clone())
        {
            Some(Geometry::Line(segment)) => segment,
            other => panic!("a line: {other:?}"),
        };
        let before = segment(&d);
        let crossing = d.w.state(2).plan.world_to_screen(Point2::new(25.0, 20.0));
        let from = PointF::new(crossing.x, crossing.y);
        d.w.views
            .with_plan_view_lent(d.w.host.document_mut(), 2, |plan, state, cx| {
                plan.mouse_press(cx, state, from, MouseButton::Left, KeyModifiers::NONE);
                for step in 1..=4 {
                    let at = PointF::new(from.x + 40.0 * f64::from(step) / 4.0, from.y);
                    plan.mouse_move(cx, state, at, KeyModifiers::NONE);
                }
                let to = PointF::new(from.x + 40.0, from.y);
                plan.mouse_release(cx, state, to, MouseButton::Left, KeyModifiers::NONE);
            });
        d.w.frame();
        let after = segment(&d);
        assert_eq!(after.start, before.start, "moved to {after:?}");
        assert_eq!(after.end, before.end);
        assert_eq!(
            d.w.document().history().undo_count(),
            undoable,
            "no edit was made"
        );
    }

    #[test]
    fn a_ghost_switch_changed_without_a_notification_is_still_drawn() {
        let mut d = DesignAndAsBuilt::new();
        d.w.paint(2);
        assert_eq!(d.ghosts(2), 1);
        d.w.with_state(2, |view| view.selection_ghosts = false);
        d.w.paint(2);
        assert_eq!(d.ghosts(2), 0);
    }
}
