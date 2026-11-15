//! The twins of tests/qt_widgets/test_view_links.cpp: linked plan views in the workspace (the
//! owner's request of 2026-09-30 to zoom in a design view and have the as-built view zoom in too,
//! with the control on each view's own bar), the ZOOM verb on each kind of view, and the Zoom To
//! dialog's spoken reply. Every expected position is the C++'s, worked by hand from
//! ViewTransform's definitions (a view W by H pixels centred on c at s pixels a unit puts screen
//! (x, y) at world (c.x + (x - W/2)/s, c.y - (y - H/2)/s)).
//!
//! The C++ fixture's interpreter run straight ("the event loop would paint B at once") is
//! `Site::interpret`, with no frame after it; its `run` is `Site::run`, a frame after.

mod view_links {
    use katana_cad::view_set::ViewState;
    use katana_cad::viewport_layout::ViewKind;
    use katana_cad::interactive_tool::ToolInput;
    use katana_entity::Geometry;
    use katana_geometry::{Point2, Point3};
    use katana_window::canvas::PointF;
    use katana_window::view_workspace::Scenery;
    use katana_window::viewport_widget::{KeyModifiers, MouseButton};

    use crate::support::{Site, assert_near, expect_same_view};

    /// A survey line at MGA coordinates, far from the origin every new view starts at:
    /// (300000, 6200000) to (300100, 6200050), its middle (300050, 6200025).
    fn survey_line(w: &mut Site, layer: Option<&str>) {
        if let Some(layer) = layer {
            w.add_layers(&[layer]);
        }
        let _ = w.line(
            Point2::new(300_000.0, 6_200_000.0),
            Point2::new(300_100.0, 6_200_050.0),
            layer,
        );
    }

    /// The scale Zoom Extents frames a w x h box at in `view`: 8 % a side, the tighter axis.
    fn framed_scale(view: &ViewState, w: f64, h: f64) -> f64 {
        (0.84 * view.plan.width_pixels / w).min(0.84 * view.plan.height_pixels / h)
    }

    fn drawn_nothing(w: &mut Site, id: u32) -> bool {
        w.views
            .with_plan_view_lent(w.host.document_mut(), id, |plan, state, cx| {
                plan.drawn_bounds(cx, state).empty()
            })
            .expect("a plan view")
    }

    fn render_paint(w: &mut Site, id: u32) {
        let document = w.host.document();
        w.views
            .with_render_view(id, document, Scenery::default(), |view, scene, state| {
                let _ = view.paint(scene, state);
            })
            .expect("a 3D view");
    }

    #[test]
    fn a_wheel_notch_in_a_linked_plan_view_gives_the_other_the_same_centre_and_scale() {
        let mut w = Site::new();
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        w.place(b, 360, 240, -50.0, -50.0, 1.0);
        assert_eq!(w.run(&format!("VIEWS LINK {a},{b}")), "leader=1 linked=1,2 moved=2");
        w.paint(b);
        let before = w.drawing_paints(b);

        // One notch, x1.2, at A's top-left pixel: by hand the centre is (3.75, 145/6).
        w.wheel(a, PointF::new(0.0, 0.0), 1);
        let leader = w.state(a);
        assert_eq!(leader.plan.center.x, 3.75);
        assert_near(leader.plan.center.y, 145.0 / 6.0, 1e-12);
        assert_near(leader.plan.scale, 4.8, 1e-12);

        expect_same_view(&w.state(b), &leader);
        // B keeps its own size, so it shows more about the same centre.
        assert_eq!(w.state(b).plan.width_pixels, 360.0);
        assert_eq!(w.state(b).plan.height_pixels, 240.0);
        // And it was drawn again, once, for it.
        w.paint(b);
        assert_eq!(w.drawing_paints(b), before + 1);
    }

    #[test]
    fn a_pan_in_a_linked_view_moves_the_other_bit_for_bit_and_the_other_never_reports_it() {
        let mut w = Site::new();
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        w.place(b, 380, 260, -50.0, -50.0, 1.0);
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));
        let reports_a = w.views.reported_moves(a);
        let reports_b = w.views.reported_moves(b);

        // A dragged (+60, -30) px from its middle: by hand the centre is (-5, 12.5).
        w.pan(a, PointF::new(150.0, 100.0), PointF::new(60.0, -30.0));

        assert_eq!(w.state(a).plan.center, Point2::new(-5.0, 12.5));
        assert_eq!(w.state(a).plan.scale, 4.0);
        expect_same_view(&w.state(b), &w.state(a));
        assert!(
            w.views.reported_moves(a) > reports_a,
            "each move of the drag is the user's"
        );
        assert_eq!(
            w.views.reported_moves(b),
            reports_b,
            "moved by the link, B reports nothing: no echo"
        );
    }

    #[test]
    fn a_view_linked_before_it_was_ever_seen_leads_from_its_first_paint() {
        let mut w = Site::new();
        let _ = w.line(
            Point2::new(1000.0, 1000.0),
            Point2::new(1100.0, 1050.0),
            None,
        );
        let a = w.active();
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        let b = w.views.open_view(w.host.document_mut(), ViewKind::Plan, true);
        assert!(!w.state(b).plan_framed);
        // Straight to the interpreter: the event loop would paint B at once.
        let linked = w.interpret(&format!("VIEWS LINK {a},{b} TO {b}"));
        assert_eq!(linked.as_deref(), Ok("leader=2 linked=1,2 moved=none"));
        assert_eq!(
            w.state(a).plan.center,
            Point2::new(10.0, 20.0),
            "nothing framed to follow yet"
        );

        w.paint(b);

        // B framed the line about its middle, and A shows exactly that.
        assert!(w.state(b).plan_framed);
        assert_eq!(w.state(b).plan.center, Point2::new(1050.0, 1025.0));
        expect_same_view(&w.state(a), &w.state(b));
    }

    #[test]
    fn a_link_led_by_a_view_that_frames_nothing_at_its_first_paint_takes_the_others_place() {
        let mut w = Site::new();
        w.add_layers(&["design"]);
        let _ = w.line(
            Point2::new(1000.0, 1000.0),
            Point2::new(1100.0, 1050.0),
            Some("design"),
        );
        let a = w.active();
        w.place(a, 300, 200, 1050.0, 1025.0, 2.0);
        let b = w.views.open_view(w.host.document_mut(), ViewKind::Plan, true);
        // Straight to the interpreter, as a script runs its lines.
        assert!(w.interpret(&format!("VIEWS HIDE {b} design")).is_ok());
        let linked = w.interpret(&format!("VIEWS LINK {a},{b} TO {b}"));
        assert_eq!(linked.as_deref(), Ok("leader=2 linked=1,2 moved=none"));

        w.paint(b);
        w.frame();

        assert!(drawn_nothing(&mut w, b), "B draws nothing");
        assert_eq!(
            w.state(a).plan.center,
            Point2::new(1050.0, 1025.0),
            "A stays where it was put"
        );
        assert_eq!(w.state(a).plan.scale, 2.0);
        expect_same_view(&w.state(b), &w.state(a));
    }

    #[test]
    fn a_wheel_in_an_unlinked_view_moves_no_other_view() {
        let mut w = Site::new();
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        w.place(b, 360, 240, -50.0, -50.0, 1.0);
        w.paint(b);
        let before = w.drawing_paints(b);

        w.wheel(a, PointF::new(0.0, 0.0), 1);
        assert_eq!(w.state(a).plan.center.x, 3.75);
        assert_eq!(w.state(b).plan.center, Point2::new(-50.0, -50.0));
        assert_eq!(w.state(b).plan.scale, 1.0);
        w.paint(b);
        assert_eq!(w.drawing_paints(b), before);
    }

    #[test]
    fn a_linked_view_keeps_the_links_view_when_resized() {
        let mut w = Site::new();
        let _ = w.line(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0), None);
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.views.set_fixed_size(a, Some((300, 200)));
        w.views.set_fixed_size(b, Some((360, 240)));
        w.frame();
        assert!(w.views.zoom_extents(w.host.document_mut(), Scenery::default(), a).is_ok());
        assert!(w.views.zoom_extents(w.host.document_mut(), Scenery::default(), b).is_ok());
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));
        let centre = w.state(a).plan.center;
        let scale = w.state(a).plan.scale;
        expect_same_view(&w.state(b), &w.state(a));

        // Each resized: the one that led and the one that followed.
        w.views.set_fixed_size(a, Some((200, 300)));
        w.views.set_fixed_size(b, Some((500, 150)));
        w.frame();
        assert_eq!(w.state(a).plan.center, centre);
        assert_eq!(w.state(a).plan.scale, scale);
        expect_same_view(&w.state(b), &w.state(a));
    }

    #[test]
    fn a_followers_first_paint_does_not_reframe_it() {
        let mut w = Site::new();
        let _ = w.line(
            Point2::new(1000.0, 1000.0),
            Point2::new(1100.0, 1050.0),
            None,
        );
        let a = w.active();
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        // Opened and linked before it is ever painted.
        let b = w.views.open_view(w.host.document_mut(), ViewKind::Plan, true);
        assert!(!w.state(b).plan_framed);
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));
        w.paint(b);

        assert_eq!(w.state(b).plan.center, Point2::new(10.0, 20.0));
        assert_eq!(w.state(b).plan.scale, 4.0);
    }

    #[test]
    fn pressing_the_link_button_runs_the_line_and_checks_it_from_the_model() {
        let mut w = Site::new();
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        w.place(b, 300, 200, 70.0, 80.0, 2.0);
        let checked = |w: &Site, id| {
            w.bar(id)
                .tool_mut("ViewLinkButton")
                .and_then(|link| link.checked)
                .unwrap_or(false)
        };
        assert!(!checked(&w, a));
        assert!(w.tip(a, "ViewLinkButton").contains("Link this view"));

        // The user zooms the as-built view B, then clicks A's Link first and B's second: B's zoom
        // is what both show.
        w.wheel(b, PointF::new(150.0, 100.0), 1);
        let zoomed_centre = w.state(b).plan.center;
        let zoomed_scale = w.state(b).plan.scale;
        assert_near(zoomed_scale, 2.4, 1e-12);

        w.press(a, "ViewLinkButton");
        assert_eq!(w.ran().len(), 1);
        assert_eq!(w.ran().last().map(String::as_str), Some("VIEWS LINK 1 TO 1"));
        assert!(checked(&w, a));
        assert!(
            w.tip(a, "ViewLinkButton").contains("Linked, waiting"),
            "{}",
            w.tip(a, "ViewLinkButton")
        );
        assert!(!checked(&w, b));

        w.press(b, "ViewLinkButton");
        assert_eq!(
            w.ran().last().map(String::as_str),
            Some("VIEWS LINK 2 TO 2"),
            "B was moved last, so it leads"
        );
        assert!(checked(&w, a));
        assert!(checked(&w, b));
        assert_eq!(w.state(a).plan.center, zoomed_centre);
        assert_eq!(w.state(a).plan.scale, zoomed_scale);
        assert!(
            w.tip(a, "ViewLinkButton").contains("Pans and zooms with Plan 2"),
            "{}",
            w.tip(a, "ViewLinkButton")
        );
        assert!(checked(&w, a), "the button shows the link");

        // A's Link again: out of the link, and B, alone, with it.
        w.press(a, "ViewLinkButton");
        assert_eq!(w.ran().last().map(String::as_str), Some("VIEWS UNLINK 1"));
        assert!(!checked(&w, a));
        assert!(!checked(&w, b));
        assert!(w.views.view_set().borrow().linked_views().is_empty());
    }

    #[test]
    fn a_refused_link_leaves_the_button_as_it_was() {
        let mut w = Site::new();
        let a = w.active();
        let _ = w.open(ViewKind::Plan);
        // An executor that refuses every line, as the window's does a line it cannot run.
        w.refuse.set(true);
        w.press(a, "ViewLinkButton");
        assert_eq!(w.ran().len(), 1);
        let checked = w
            .bar(a)
            .tool_mut("ViewLinkButton")
            .and_then(|link| link.checked);
        assert_eq!(checked, Some(false));
        assert!(!w.state(a).linked);
    }

    #[test]
    fn the_link_button_is_on_plan_views_only() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        for id in [plan, model, section] {
            assert!(w.bar(id).tool_mut("ViewLinkButton").is_some(), "{id}");
        }
        let shown = |w: &Site, id| w.bar(id).shown_tools().contains(&String::from("ViewLinkButton"));
        assert!(shown(&w, plan));
        assert!(!shown(&w, model));
        assert!(!shown(&w, section));

        // It follows the kind: the plan turned into 3D loses it, and the 3D view turned into a
        // plan gains it.
        assert!(
            w.views
                .set_view_kind(w.host.document_mut(), plan, ViewKind::Model3D)
                .is_ok()
        );
        assert!(
            w.views
                .set_view_kind(w.host.document_mut(), model, ViewKind::Plan)
                .is_ok()
        );
        w.frame();
        assert!(!shown(&w, plan));
        assert!(shown(&w, model));
    }

    #[test]
    fn the_plan_docks_minimum_width_grows_by_the_link_button_alone() {
        // Every tool always shown on a bar adds its 22 px and the bar's 1 px between tools to the
        // least a view can be narrowed to. A plan view's bar has the Link button; a 3D view's has
        // not, and is otherwise the same.
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let plan_bar = w.bar(plan).minimum_width();
        let model_bar = w.bar(model).minimum_width();
        assert_eq!(plan_bar - model_bar, 23.0);
    }

    #[test]
    fn zoom_extents_all_frames_the_link_once_on_the_union_of_what_its_views_draw() {
        let mut w = Site::new();
        w.add_layers(&["design", "asbuilt"]);
        let _ = w.line(Point2::new(0.0, 0.0), Point2::new(10.0, 10.0), Some("design"));
        let _ = w.line(
            Point2::new(100.0, 100.0),
            Point2::new(110.0, 110.0),
            Some("asbuilt"),
        );
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.views.set_fixed_size(a, Some((300, 200)));
        w.views.set_fixed_size(b, Some((360, 240)));
        w.frame();
        w.with_state(a, |view| {
            view.layers.hide("asbuilt");
        });
        w.with_state(b, |view| {
            view.layers.hide("design");
        });
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));

        w.views.zoom_extents_all(w.host.document_mut());
        w.frame();

        // Framed in A, the lowest id, nobody having moved either: the union (0, 0)-(110, 110)
        // about (55, 55), fitted into A's 300 x 200 with 8% a side, 0.84 x 200 / 110 px a unit.
        let leader = w.state(a);
        assert_eq!(leader.plan.center, Point2::new(55.0, 55.0));
        assert_near(leader.plan.scale, 0.84 * 200.0 / 110.0, 1e-12);
        expect_same_view(&w.state(b), &leader);
    }

    #[test]
    fn a_zoom_line_on_a_linked_view_moves_it_and_the_views_linked_with_it() {
        let mut w = Site::new();
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        let c = w.open(ViewKind::Plan);
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        w.place(b, 300, 200, 0.0, 0.0, 1.0);
        w.place(c, 300, 200, 7.0, 7.0, 7.0);
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));

        let reply = w.run(&format!("ZOOM CENTRE 50,40 SCALE 8 view={b}"));
        // 300 x 200 at 8 px a unit about (50, 40): x 50 -+ 18.75, y 40 -+ 12.5.
        assert_eq!(
            reply,
            "view=2 kind=plan centre=50,40 scale=8 area=31.25,27.5,68.75,52.5\n\
             view=1 kind=plan followed=2 centre=50,40 scale=8 area=31.25,27.5,68.75,52.5"
        );
        assert_eq!(w.state(a).plan.center, Point2::new(50.0, 40.0));
        assert_eq!(w.state(c).plan.center, Point2::new(7.0, 7.0), "not linked");
    }

    #[test]
    fn zoom_to_selection_in_one_linked_view_frames_it_in_both() {
        // By hand the line (10,10)-(40,30) fits a 300 x 200 view at 0.84 x 300 / 30 = 8.4 px a
        // unit, about (25, 20).
        let mut w = Site::new();
        let _ = w.line(Point2::new(10.0, 10.0), Point2::new(40.0, 30.0), None);
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 360, 240, 0.0, 0.0, 1.0);
        w.place(b, 300, 200, -50.0, -50.0, 2.0);
        let _ = w.run("SELECT ALL");
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));

        w.press(b, "ViewZoomSelectionButton");

        assert_eq!(
            w.ran().last().map(String::as_str),
            Some("ZOOM SELECTION view=2")
        );
        assert_eq!(w.state(b).plan.center, Point2::new(25.0, 20.0));
        assert_near(w.state(b).plan.scale, 8.4, 1e-12);
        expect_same_view(&w.state(a), &w.state(b));
        assert_eq!(w.active(), b, "pressing a view's tool makes it active");
    }

    #[test]
    fn a_transparent_zoom_typed_inside_line_zooms_and_leaves_line_at_its_step() {
        // Something far off to frame, so the zoom is seen: its middle (150, 125).
        let mut w = Site::new();
        let _ = w.line(
            Point2::new(100.0, 100.0),
            Point2::new(200.0, 150.0),
            None,
        );
        let plan = w.active();
        w.place(plan, 300, 200, 0.0, 0.0, 10.0);
        assert!(w.views.start_tool(w.host.document_mut(), "draw.line").is_ok());
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "0,0"));
        let prompt = |w: &Site| {
            let document = w.host.document();
            w.views
                .with_plan_view(plan, |view, _| view.tool_host().prompt(document))
                .unwrap_or_default()
        };
        let before = prompt(&w);

        // Z, as a person types it at the second point: the view's, not a point.
        let in_view = format!(" view={plan}");
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "Z"));
        w.frame();
        assert!(!w.ran().is_empty());
        assert_eq!(w.ran().last().cloned(), Some(format!("ZOOM{in_view}")));
        assert_eq!(w.state(plan).plan.center, Point2::new(150.0, 125.0));
        // The tool is where it was: still Line, still asking for the next point.
        assert_eq!(w.views.active_tool_id(), "draw.line");
        let expects = {
            let document = w.host.document();
            w.views
                .with_plan_view(plan, |view, _| view.tool_expects(document))
                .flatten()
        };
        assert_eq!(expects, Some(ToolInput::Point));
        assert_eq!(prompt(&w), before);

        // 'ZOOM IN with AutoCAD's apostrophe is the same; PAN has no verb and is refused by name,
        // the tool untouched.
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "'ZOOM IN"));
        assert_eq!(w.ran().last().cloned(), Some(format!("ZOOM IN{in_view}")));
        let before_pan = w.ran().len();
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "PAN"));
        assert_eq!(w.ran().len(), before_pan, "PAN ran nothing");
        assert!(w.views.runs_transparently(w.host.document(), "Z"));
        assert!(!w.views.runs_transparently(w.host.document(), "PAN"));

        // And the line goes on from 0,0 as if nothing had been typed between.
        let entities = w.document().model().entities.size();
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "10,0"));
        assert!(w.views.press_enter(w.host.document_mut()));
        w.frame();
        assert_eq!(w.document().model().entities.size(), entities + 1);
        let newest = *w
            .document()
            .model()
            .entities
            .ids()
            .last()
            .expect("a line was drawn");
        let drawn = w
            .document()
            .model()
            .entities
            .find(newest)
            .expect("the newest entity")
            .geometry
            .clone();
        let Geometry::Line(segment) = drawn else {
            panic!("a line: {drawn:?}");
        };
        assert_eq!(segment.start, Point2::new(0.0, 0.0));
        assert_eq!(segment.end, Point2::new(10.0, 0.0));
        w.views.stop_tool(w.host.document_mut());
        assert!(
            !w.views.runs_transparently(w.host.document(), "Z"),
            "no tool runs: Z is a command"
        );
    }

    #[test]
    fn a_zoom_typed_at_a_tools_prompt_zooms_the_view_the_tool_runs_in_not_the_active_one() {
        let mut w = Site::new();
        let _ = w.line(
            Point2::new(100.0, 100.0),
            Point2::new(200.0, 150.0),
            None,
        );
        let drawing = w.active();
        let other = w.open(ViewKind::Plan);
        w.place(drawing, 300, 200, 0.0, 0.0, 10.0);
        w.place(other, 300, 200, -40.0, -40.0, 3.0);
        let _ = w.run(&format!("VIEWS ACTIVATE {drawing}"));
        assert!(w.views.start_tool(w.host.document_mut(), "draw.line").is_ok());
        let running = w
            .views
            .with_plan_view(drawing, |view, _| view.active_tool_id())
            .unwrap_or_default();
        assert_ne!(running, "", "Line runs in view 1");
        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "0,0"));
        let _ = w.run(&format!("VIEWS ACTIVATE {other}"));
        assert_eq!(w.active(), other);
        assert_eq!(w.views.active_tool_id(), "draw.line");

        assert!(w.views.type_into_tool(&mut w.host, Scenery::default(), "'Z"));
        w.frame();
        assert_eq!(w.ran().last().cloned(), Some(format!("ZOOM view={drawing}")));
        assert_eq!(w.state(drawing).plan.center, Point2::new(150.0, 125.0));
        assert_eq!(
            w.state(other).plan.center,
            Point2::new(-40.0, -40.0),
            "the view looked at stays put"
        );
        let expects = {
            let document = w.host.document();
            w.views
                .with_plan_view(drawing, |view, _| view.tool_expects(document))
                .flatten()
        };
        assert_eq!(expects, Some(ToolInput::Point));

        assert!(w.views.type_into_tool(
            &mut w.host,
            Scenery::default(),
            &format!("ZOOM IN view={other}")
        ));
        assert_eq!(w.ran().last().cloned(), Some(format!("ZOOM IN view={other}")));
        assert_eq!(
            w.state(other).plan.scale,
            6.0,
            "twice as close, where it was asked"
        );
        w.views.stop_tool(w.host.document_mut());
    }

    #[test]
    fn a_sections_zoom_in_is_twice_as_close_about_the_middle_of_its_plot() {
        let mut w = Site::new();
        let section = w.open(ViewKind::Section);
        let read = |w: &Site, offset: f64| {
            w.views
                .with_section_view(section, |view, state| {
                    let middle = view.plot_centre();
                    view.station_elevation_at(state, PointF::new(middle.x + offset, middle.y))
                })
                .expect("a section view")
        };
        let before = read(&w, 0.0);
        let edge = read(&w, 100.0);
        let _ = w.run(&format!("ZOOM IN view={section}"));
        let after = read(&w, 0.0);
        assert_near(after.x, before.x, 1e-9);
        assert_near(after.y, before.y, 1e-9);
        // Twice as close: 100 px from the middle now spans half the stations.
        let edge_after = read(&w, 100.0);
        assert_near(edge_after.x - after.x, 0.5 * (edge.x - before.x), 1e-9);
    }

    #[test]
    fn a_zoom_of_a_view_not_yet_painted_starts_from_what_it_draws() {
        let mut w = Site::new();
        survey_line(&mut w, None);
        let opened = w.interpret("VIEWS OPEN plan").unwrap_or_else(|e| panic!("{e}"));
        assert!(opened.contains(" framed=no "), "{opened}");
        let b = w.active();
        assert!(!w.state(b).plan_framed);

        // Straight on, as a script runs its lines.
        let zoomed = w
            .interpret(&format!("ZOOM IN view={b}"))
            .unwrap_or_else(|e| panic!("{e}"));
        let view = w.state(b);
        assert_eq!(view.plan.center, Point2::new(300_050.0, 6_200_025.0));
        let fitted = framed_scale(&view, 100.0, 50.0);
        assert_near(view.plan.scale, 2.0 * fitted, 1e-12 * fitted);
        assert!(
            zoomed.starts_with("view=2 kind=plan centre=300050,6200025 scale="),
            "{zoomed}"
        );

        // Its first paint leaves it there: the zoom is the user's view now.
        w.paint(b);
        assert_eq!(w.state(b).plan.center, Point2::new(300_050.0, 6_200_025.0));
        assert_near(w.state(b).plan.scale, 2.0 * fitted, 1e-12 * fitted);
        let drawn = w
            .views
            .with_plan_view(b, |plan, _| plan.last_drawn_entity_count())
            .unwrap_or_default();
        assert_eq!(drawn, 1);
        assert!(!w.run("VIEWS").contains("framed="), "framed, it says nothing");
    }

    #[test]
    fn a_zoom_straight_after_a_view_opens_is_framed_at_the_width_the_view_is_seen_at() {
        // Over the window widths at which the tools come and go, each view about 250 to 380 px.
        let mut differed = 0;
        for window_width in (500..=760).step_by(5) {
            let mut w = Site::sized(window_width as f32, 600.0);
            survey_line(&mut w, None);
            assert!(w.interpret("VIEWS OPEN plan").is_ok());
            let b = w.active();
            let width = |w: &Site| {
                w.views
                    .with_plan_view(b, |plan, _| plan.size().0)
                    .expect("a plan view")
            };
            let opened = width(&w);
            assert!(w.interpret(&format!("ZOOM IN view={b}")).is_ok());
            let framed = w.state(b).plan.width_pixels;
            w.frame();
            let seen = width(&w);
            assert_eq!(framed, f64::from(seen), "window {window_width} px");
            differed += i32::from(opened != seen);
        }
        // The C++ sweep reaches the case it is about, a width Qt's queued layout still had to
        // change when the view had just opened (`differed > 0`). The workspace here lays a view
        // out as it opens, so that case cannot arise: every view is already at the width it is
        // seen at (docs/rust.md, step 7 wave 3).
        assert_eq!(differed, 0);
    }

    #[test]
    fn a_view_not_yet_painted_that_leads_the_link_takes_it_to_the_drawing_when_zoomed() {
        let mut w = Site::new();
        survey_line(&mut w, None);
        let a = w.active();
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        assert!(w.interpret("VIEWS OPEN plan").is_ok());
        let b = w.active();
        let linked = w.interpret(&format!("VIEWS LINK {a},{b} TO {b}"));
        assert_eq!(linked.as_deref(), Ok("leader=2 linked=1,2 moved=none"));

        let zoomed = w
            .interpret(&format!("ZOOM IN view={b}"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(w.state(b).plan.center, Point2::new(300_050.0, 6_200_025.0));
        expect_same_view(&w.state(a), &w.state(b));
        assert!(
            zoomed.contains("\nview=1 kind=plan followed=2 centre=300050,6200025 "),
            "{zoomed}"
        );
    }

    #[test]
    fn a_three_d_views_zoom_in_is_twice_as_close_about_its_middle() {
        let mut w = Site::new();
        let _ = w.line(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0), None);
        let model = w.open(ViewKind::Model3D);
        render_paint(&mut w, model);
        assert!(w.state(model).camera_framed);
        let before = w.state(model).camera;

        let reply = w.run(&format!("ZOOM IN view={model}"));
        assert!(
            reply.starts_with(&format!("view={model} kind=3d target=")),
            "{reply}"
        );
        let after = w.state(model).camera;
        assert_near(after.distance(), 0.5 * before.distance(), 1e-12 * before.distance());
        let near = 1e-9 * before.distance();
        assert_near(after.target().x, before.target().x, near);
        assert_near(after.target().y, before.target().y, near);
        assert_near(after.target().z, before.target().z, near);
        assert_eq!(after.azimuth(), before.azimuth(), "a zoom turns nothing");

        let _ = w.run(&format!("ZOOM OUT view={model}"));
        assert_near(
            w.state(model).camera.distance(),
            before.distance(),
            1e-12 * before.distance(),
        );
    }

    #[test]
    fn an_enormous_zoom_out_of_a_three_d_view_stops_where_the_scene_is_a_pixel_across() {
        let mut w = Site::new();
        let _ = w.line(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0), None);
        let model = w.open(ViewKind::Model3D);
        render_paint(&mut w, model);
        let before = w.state(model).camera;
        let farthest = 100.0_f64.hypot(50.0) * 0.5 * f64::from(before.viewport_height())
            / (0.5 * before.field_of_view()).tan();
        assert!(before.distance() < farthest);

        let line = format!(" view={model}");
        let _ = w.run(&format!("ZOOM 1e-300{line}"));
        let after = w.state(model).camera;
        assert_near(after.distance(), farthest, 1e-9 * farthest);
        assert_near(after.target().x, before.target().x, 1e-9 * farthest);
        assert_near(after.target().y, before.target().y, 1e-9 * farthest);
        assert_near(after.target().z, before.target().z, 1e-9 * farthest);

        let _ = w.run(&format!("ZOOM OUT{line}"));
        assert_near(w.state(model).camera.distance(), farthest, 1e-9 * farthest);
        let _ = w.run(&format!("ZOOM IN{line}"));
        assert_near(
            w.state(model).camera.distance(),
            0.5 * farthest,
            1e-9 * farthest,
        );
    }

    #[test]
    fn a_three_d_view_not_yet_painted_frames_the_scene_before_it_zooms() {
        let mut w = Site::new();
        let _ = w.line(
            Point2::new(1000.0, 1000.0),
            Point2::new(1100.0, 1050.0),
            None,
        );
        let opened = w.interpret("VIEWS OPEN 3d").unwrap_or_else(|e| panic!("{e}"));
        assert!(opened.contains(" framed=no "), "{opened}");
        let model = w.active();
        assert!(!w.state(model).camera_framed);

        assert!(w.interpret(&format!("ZOOM IN view={model}")).is_ok());
        let zoomed = w.state(model).camera;
        // The frame's target is the middle of the line's box, on the datum 0.
        assert_near(zoomed.target().x, 1050.0, 1e-9);
        assert_near(zoomed.target().y, 1025.0, 1e-9);
        assert_near(zoomed.target().z, 0.0, 1e-9);

        render_paint(&mut w, model);
        assert_eq!(
            w.state(model).camera.distance(),
            zoomed.distance(),
            "the paint kept the zoom"
        );

        // Framed afresh at the same size, the distance is twice the zoomed one.
        let _ = w.run(&format!("ZOOM EXTENTS view={model}"));
        assert_near(
            zoomed.distance(),
            0.5 * w.state(model).camera.distance(),
            1e-9 * zoomed.distance(),
        );
    }

    #[test]
    fn zoom_to_selection_in_a_three_d_view_frames_what_it_draws_of_the_selection() {
        let mut w = Site::new();
        let _ = w.line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), None);
        w.add_layers(&["far"]);
        let far_line = w.line(
            Point2::new(500.0, 300.0),
            Point2::new(520.0, 340.0),
            Some("far"),
        );
        let model = w.open(ViewKind::Model3D);
        render_paint(&mut w, model);
        let line = format!("ZOOM SELECTION view={model}");
        let start = w.state(model).camera;

        assert_eq!(w.run(&line), "scope=selection matched=0");
        assert_eq!(w.state(model).camera.target(), start.target());
        assert_eq!(w.state(model).camera.distance(), start.distance());

        w.document_mut().selection_mut().set(vec![far_line]);
        w.document().notify_selection_changed();
        let framed = w.run(&line);
        assert!(
            framed.starts_with(&format!(
                "scope=selection matched=1\nview={model} kind=3d target=510,320,0 "
            )),
            "{framed}"
        );
        assert_eq!(
            w.state(model).camera.target(),
            Point3::new(510.0, 320.0, 0.0)
        );
        assert!(
            w.state(model).camera.distance() < start.distance(),
            "closer: 45 units, not 500"
        );

        // Hidden in that view, and not ghosted there: nothing of it is drawn, nothing to frame,
        // and the view's record says so.
        let _ = w.run(&format!("VIEWS HIDE {model} far"));
        let _ = w.run(&format!("VIEWS SET {model} ghosts=off"));
        let shown = w.state(model).camera;
        assert_eq!(
            w.run(&line),
            format!("scope=selection matched=1\nview={model} kind=3d shown=0 moved=no")
        );
        assert_eq!(w.state(model).camera.target(), shown.target());
        assert_eq!(w.state(model).camera.distance(), shown.distance());
    }

    #[test]
    fn a_plan_views_zoom_to_selection_moves_nothing_where_it_shows_none_of_the_selection() {
        let mut w = Site::new();
        w.add_layers(&["design"]);
        let _ = w.line(Point2::new(10.0, 10.0), Point2::new(40.0, 30.0), Some("design"));
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 300, 200, -500.0, -500.0, 2.0);
        w.place(b, 300, 200, -500.0, -500.0, 2.0);
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));
        let _ = w.run(&format!("VIEWS HIDE {b} design"));
        let _ = w.run(&format!("VIEWS SET {b} ghosts=off"));
        let _ = w.run("SELECT ALL");
        assert_eq!(w.document().selection().size(), 1);

        w.press(b, "ViewZoomSelectionButton");
        assert_eq!(
            w.ran().last().map(String::as_str),
            Some("ZOOM SELECTION view=2")
        );
        assert_eq!(
            w.state(b).plan.center,
            Point2::new(-500.0, -500.0),
            "nothing of it shown there"
        );
        assert_eq!(w.state(b).plan.scale, 2.0);
        assert_eq!(
            w.state(a).plan.center,
            Point2::new(-500.0, -500.0),
            "and the link stayed put"
        );

        let _ = w.run(&format!("VIEWS SET {b} ghosts=on"));
        w.press(b, "ViewZoomSelectionButton");
        assert_eq!(w.state(b).plan.center, Point2::new(25.0, 20.0));
        assert_near(w.state(b).plan.scale, 8.4, 1e-12);
        expect_same_view(&w.state(a), &w.state(b));
    }

    #[test]
    fn the_bars_zoom_in_and_out_run_their_lines_and_zoom_the_view() {
        let mut w = Site::new();
        w.resize(1200.0, 600.0);
        let _ = w.line(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0), None);
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        w.with_state(plan, |view| {
            view.plan.center = Point2::new(10.0, 20.0);
            view.plan.scale = 4.0;
            view.plan_framed = true;
        });
        w.views.with_plan_view(plan, |view, _| view.hold_view());
        render_paint(&mut w, model);
        w.frame();

        for id in [plan, model] {
            for name in ["ViewZoomInButton", "ViewZoomOutButton"] {
                assert!(w.bar(id).tool_mut(name).is_some(), "{name}");
                assert!(w.shown(id, name), "{name} on view {id}");
            }
        }

        w.press(plan, "ViewZoomInButton");
        assert_eq!(w.ran().last().map(String::as_str), Some("ZOOM IN view=1"));
        assert_eq!(w.state(plan).plan.scale, 8.0);
        assert_eq!(w.state(plan).plan.center, Point2::new(10.0, 20.0));
        w.press(plan, "ViewZoomOutButton");
        assert_eq!(w.ran().last().map(String::as_str), Some("ZOOM OUT view=1"));
        assert_eq!(w.state(plan).plan.scale, 4.0);

        let distance = w.state(model).camera.distance();
        w.press(model, "ViewZoomInButton");
        assert_eq!(w.ran().last().cloned(), Some(format!("ZOOM IN view={model}")));
        assert_near(w.state(model).camera.distance(), 0.5 * distance, 1e-12 * distance);
        w.press(model, "ViewZoomOutButton");
        assert_eq!(w.ran().last().cloned(), Some(format!("ZOOM OUT view={model}")));
        assert_near(w.state(model).camera.distance(), distance, 1e-12 * distance);
        assert_eq!(w.active(), model, "pressing a view's tool makes it active");
    }

    #[test]
    fn zoom_extents_in_a_linked_view_that_draws_nothing_frames_what_the_link_draws() {
        let mut w = Site::new();
        survey_line(&mut w, Some("survey"));
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        w.place(b, 300, 200, 300_050.0, 6_200_025.0, 2.0);
        let _ = w.run(&format!("VIEWS HIDE {a} survey"));
        assert!(drawn_nothing(&mut w, a));
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));

        let reply = w.run(&format!("ZOOM EXTENTS view={a}"));
        assert_eq!(w.state(a).plan.center, Point2::new(300_050.0, 6_200_025.0));
        assert_near(w.state(a).plan.scale, framed_scale(&w.state(a), 100.0, 50.0), 1e-12);
        expect_same_view(&w.state(b), &w.state(a));
        assert!(
            reply.contains("\nview=2 kind=plan followed=1 centre=300050,6200025 "),
            "{reply}"
        );

        // Unlinked, a view that draws nothing still frames nothing, as always.
        let _ = w.run("VIEWS UNLINK ALL");
        let _ = w.run(&format!("ZOOM EXTENTS view={a}"));
        assert_eq!(w.state(a).plan.center, Point2::new(0.0, 0.0));
        assert_eq!(w.state(a).plan.scale, 1.0);
        assert_eq!(
            w.state(b).plan.center,
            Point2::new(300_050.0, 6_200_025.0),
            "and moves nothing else"
        );
    }

    #[test]
    fn a_middle_double_click_in_a_linked_view_that_draws_nothing_frames_what_the_link_draws() {
        let mut w = Site::new();
        survey_line(&mut w, Some("survey"));
        let a = w.active();
        let b = w.open(ViewKind::Plan);
        w.place(a, 300, 200, 10.0, 20.0, 4.0);
        w.place(b, 300, 200, 300_050.0, 6_200_025.0, 2.0);
        let _ = w.run(&format!("VIEWS HIDE {a} survey"));
        assert!(drawn_nothing(&mut w, a));
        let _ = w.run(&format!("VIEWS LINK {a},{b}"));

        w.views
            .with_plan_view_lent(w.host.document_mut(), a, |plan, state, cx| {
                plan.mouse_double_click(
                    cx,
                    state,
                    PointF::new(150.0, 100.0),
                    MouseButton::Middle,
                    KeyModifiers::NONE,
                );
            });
        w.frame();
        assert_eq!(w.state(a).plan.center, Point2::new(300_050.0, 6_200_025.0));
        assert_near(w.state(a).plan.scale, framed_scale(&w.state(a), 100.0, 50.0), 1e-12);
        expect_same_view(&w.state(b), &w.state(a));
    }

    #[test]
    fn a_zoom_tools_tip_says_linked_views_follow_only_where_the_view_can_be_linked() {
        let mut w = Site::new();
        let plan = w.active();
        let model = w.open(ViewKind::Model3D);
        let section = w.open(ViewKind::Section);
        for name in [
            "ViewZoomInButton",
            "ViewZoomOutButton",
            "ViewZoomSelectionButton",
            "ViewZoomExtentsButton",
        ] {
            assert!(w.tip(plan, name).contains("linked views follow"), "{name}");
            assert!(!w.tip(model, name).contains("linked views follow"), "{name}");
            assert!(!w.tip(section, name).contains("linked views follow"), "{name}");
        }
        assert!(w.tip(model, "ViewZoomInButton").contains("as the wheel zooms there"));
        assert!(w.tip(section, "ViewZoomInButton").contains("the section's plot"));

        // The tips follow the kind: the 3D view made a plan says so.
        assert!(
            w.views
                .set_view_kind(w.host.document_mut(), model, ViewKind::Plan)
                .is_ok()
        );
        w.frame();
        assert!(w.tip(model, "ViewZoomInButton").contains("linked views follow"));
    }
}

mod zoom_to {
    use katana_window::host::{Answer, RecordingHost, VerbOutcome};
    use katana_window::zoom_to_dialog::{ZoomToContext, ZoomToDialog};

    #[test]
    fn the_status_says_what_zoom_answered_as_a_sentence_and_a_refusal_in_the_error_colour() {
        assert_eq!(
            ZoomToDialog::spoken_reply("scope=selection matched=0"),
            "Nothing matched, so no view moved."
        );
        assert_eq!(
            ZoomToDialog::spoken_reply(
                "scope=drawing where=\"LAYER=design\" matched=3\n\
                 view=2 kind=plan centre=33.333333333333336,-3.135162601626014 scale=4 \
                 area=1,2,3,4"
            ),
            "3 matched, framed in view 2."
        );
        assert_eq!(
            ZoomToDialog::spoken_reply(
                "scope=selection matched=1\n\
                 view=2 kind=plan centre=1,2 scale=3 area=0,0,1,1\n\
                 view=1 kind=plan followed=2 centre=1,2 scale=3 area=0,0,1,1"
            ),
            "1 matched, framed in view 2; view 1 followed."
        );
        assert_eq!(
            ZoomToDialog::spoken_reply(
                "scope=selection matched=5\n\
                 view=2 kind=plan centre=1,2 scale=3 area=0,0,1,1\n\
                 view=1 kind=plan followed=2 centre=1,2 scale=3\n\
                 view=3 kind=plan followed=2 centre=1,2 scale=3"
            ),
            "5 matched, framed in view 2; views 1 and 3 followed."
        );
        // A view that shows none of what the scope took.
        assert_eq!(
            ZoomToDialog::spoken_reply(
                "scope=selection matched=2\nview=2 kind=plan shown=0 moved=no"
            ),
            "2 matched, but view 2 shows none of them: nothing moved."
        );
        // One that shows some of them, which it frames.
        assert_eq!(
            ZoomToDialog::spoken_reply(
                "scope=layers layers=a,b sublayers=yes matched=5\n\
                 view=3 kind=3d target=1,2,3 distance=4 shown=2"
            ),
            "5 matched, 2 of them shown and framed in view 3."
        );

        let mut host = RecordingHost::new().answering(Answer::Fixed(VerbOutcome::new(
            false,
            "",
            "refused, as asked",
        )));
        let mut dialog = ZoomToDialog::new(ZoomToContext::default(), &host);
        assert!(!dialog.zoom(&mut host));
        assert_eq!(dialog.status(), "refused, as asked");
        assert!(dialog.status_is_error());
    }
}
