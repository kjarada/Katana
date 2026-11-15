//! The twins of tests/qt_widgets/test_section_view.cpp: the section view (section_view_widget).
//!
//! Sections are built by hand rather than cut from a surface, so that every expected station and
//! elevation can be worked out from the view's framing rule alone. The framing fits the tighter of
//!
//! ```text
//! horizontal = plot width  / (station span * 1.04)
//! vertical   = plot height / (elevation span * exaggeration * 1.15)
//! ```
//!
//! and centres the box in what is left over, so a framed section always has the middle of its box
//! at the middle of the plot. The plot is the view less 62 px on the left, 12 on the right, 10 at
//! the top and 26 at the bottom.

mod section_view {
    use std::sync::Arc;

    use katana_cad::Document;
    use katana_cad::section::{
        SampleReason, Section, SectionCrossing, SectionOptions, SectionSample, SectionSurface,
        extract_section,
    };
    use katana_cad::view_set::{ViewId, ViewSet, ViewState};
    use katana_cad::viewport_layout::ViewKind;
    use katana_commands::{EntityAttributes, create_layer, create_line, update_layer};
    use katana_entity::{EntityId, Layer};
    use katana_geometry::{Point2, Point3, Polyline2};
    use katana_terrain::{NamedSurface, TinSurface};
    use katana_window::canvas::PointF;
    use katana_window::section_view_widget::{SectionView, SectionViewEvent};

    use crate::support::assert_near;

    const LEFT: f64 = 62.0;
    const RIGHT: f64 = 12.0;

    /// Ground running straight from (0, 10) to (length, 20): station span `length`, elevation
    /// span 10.
    fn ground(length: f64) -> Section {
        let mut section = Section {
            alignment: Polyline2::new(
                vec![Point2::new(0.0, 0.0), Point2::new(length, 0.0)],
                false,
            ),
            length,
            ..Section::default()
        };
        let first = SectionSample {
            station: 0.0,
            elevation: Some(10.0),
            ..SectionSample::default()
        };
        let last = SectionSample {
            station: length,
            plan: Point2::new(length, 0.0),
            elevation: Some(20.0),
        };
        section.surfaces.push(SectionSurface {
            name: String::from("ground"),
            samples: vec![first, last],
            reasons: vec![SampleReason::Start, SampleReason::End],
            min_elevation: Some(10.0),
            max_elevation: Some(20.0),
        });
        section
    }

    /// A section view of its own state, the first of its set.
    fn section_state() -> (ViewSet, ViewId) {
        let mut views = ViewSet::new();
        let id = views.add(ViewKind::Section).id;
        (views, id)
    }

    fn state(views: &mut ViewSet, id: ViewId) -> &mut ViewState {
        views.find_mut(id).expect("the view is open")
    }

    /// The station at the left and the right edge of the plot.
    fn left_station(view: &SectionView, state: &ViewState) -> f64 {
        view.station_elevation_at(state, PointF::new(LEFT, 100.0)).x
    }

    fn right_station(view: &SectionView, state: &ViewState) -> f64 {
        view.station_elevation_at(state, PointF::new(f64::from(view.width()) - RIGHT, 100.0))
            .x
    }

    /// One wheel notch towards the user at `pixel`: a zoom in about that point.
    fn wheel_at(view: &mut SectionView, state: &ViewState, pixel: PointF) {
        let _ = view.wheel(state, pixel, 120.0);
    }

    #[test]
    fn a_section_still_as_framed_is_framed_again_when_its_view_shrinks() {
        // At 800 px the plot is 726 x 264. A 1000 m section: horizontal 726 / 1040 = 0.698 px/m,
        // vertical 264 / (10 * 10 * 1.15) = 2.30, so the stations decide, and the 1040 m shown are
        // centred on 500: the plot runs from station -20 to 1020. At 400 px the plot is 326 wide
        // and the stations still decide (0.313 px/m), so framing again shows -20..1020 again.
        // Keeping the scale instead showed only 326 / 0.698 = 467 m about 500 - 266.5..733.5 -
        // and cut both ends of the section off.
        let (mut views, id) = section_state();
        let mut view = SectionView::new();
        view.resize(state(&mut views, id), 800, 300);
        view.set_section(state(&mut views, id), None, ground(1000.0));
        let _ = view.paint(state(&mut views, id), None);
        let s = state(&mut views, id);
        assert_near(left_station(&view, s), -20.0, 1e-9, "left");
        assert_near(right_station(&view, s), 1020.0, 1e-9, "right");

        view.resize(state(&mut views, id), 400, 300);
        let _ = view.paint(state(&mut views, id), None);
        let s = state(&mut views, id);
        assert_near(left_station(&view, s), -20.0, 1e-9, "left");
        assert_near(right_station(&view, s), 1020.0, 1e-9, "right");
    }

    #[test]
    fn a_zoomed_section_keeps_its_scale_and_its_middle_when_its_view_is_resized() {
        // A 100 m section at 800 x 300 is framed with its box's middle, (50, 15), at the middle of
        // the plot, (62 + 726 / 2, 10 + 264 / 2) = (425, 142). A wheel notch there zooms about
        // that point, so it stays at the middle. Shrinking the view to 400 wide keeps the scale -
        // the plot's 326 px now cover 326 / 726 of the stations they did - and the middle, now at
        // (62 + 326 / 2, 142) = (225, 142), still reads (50, 15).
        let (mut views, id) = section_state();
        let mut view = SectionView::new();
        view.resize(state(&mut views, id), 800, 300);
        view.set_section(state(&mut views, id), None, ground(100.0));
        let _ = view.paint(state(&mut views, id), None);
        assert_eq!(view.plot_centre(), PointF::new(425.0, 142.0));
        let centre = view.plot_centre();
        wheel_at(&mut view, state(&mut views, id), centre);
        let s = state(&mut views, id);
        let span_before = right_station(&view, s) - left_station(&view, s);

        view.resize(state(&mut views, id), 400, 300);
        let _ = view.paint(state(&mut views, id), None);
        assert_eq!(view.plot_centre(), PointF::new(225.0, 142.0));
        let s = state(&mut views, id);
        let middle = view.station_elevation_at(s, view.plot_centre());
        assert_near(middle.x, 50.0, 1e-9, "station");
        assert_near(middle.y, 15.0, 1e-9, "elevation");
        assert_near(
            (right_station(&view, s) - left_station(&view, s)) / span_before,
            326.0 / 726.0,
            1e-12,
            "span",
        );
    }

    #[test]
    fn a_zoomed_section_resized_while_hidden_keeps_its_middle_when_shown_again() {
        // As above, but the resize happens while the view is hidden - a section tabbed behind
        // another view. Qt held the resize back and sent it when the widget was shown, without the
        // old size; here a view not drawn is not resized until it is drawn again, from the size it
        // last had. The middle must still read (50, 15) at the new plot's middle, (225, 142).
        let (mut views, id) = section_state();
        let mut view = SectionView::new();
        view.resize(state(&mut views, id), 800, 300);
        view.set_section(state(&mut views, id), None, ground(100.0));
        let _ = view.paint(state(&mut views, id), None);
        let centre = view.plot_centre();
        wheel_at(&mut view, state(&mut views, id), centre);

        // Hidden: no paint. Shown again at the new size.
        view.resize(state(&mut views, id), 400, 300);
        let _ = view.paint(state(&mut views, id), None);
        assert_eq!(view.plot_centre(), PointF::new(225.0, 142.0));
        let s = state(&mut views, id);
        let middle = view.station_elevation_at(s, view.plot_centre());
        assert_near(middle.x, 50.0, 1e-9, "station");
        assert_near(middle.y, 15.0, 1e-9, "elevation");
    }

    #[test]
    fn a_layer_hidden_in_the_view_takes_its_crossings_out_of_it() {
        // Two crossings on the 1000 m ground, one on "pipes" at station 200 and one on "kerb" at
        // 600, both inside the -20..1020 the plot shows. Hiding "pipes" in this view leaves one
        // drawn and one hidden.
        let (mut views, id) = section_state();
        let mut section = ground(1000.0);
        section.crossings = vec![
            SectionCrossing {
                layer: String::from("pipes"),
                station: 200.0,
                ..SectionCrossing::default()
            },
            SectionCrossing {
                layer: String::from("kerb"),
                station: 600.0,
                ..SectionCrossing::default()
            },
        ];
        let mut view = SectionView::new();
        view.resize(state(&mut views, id), 800, 300);
        view.set_section(state(&mut views, id), None, section);
        let _ = view.paint(state(&mut views, id), None);
        assert_eq!(view.last_drawn_crossing_count(), 2);
        assert_eq!(view.last_hidden_crossing_count(), 0);

        assert!(state(&mut views, id).layers.hide("pipes"));
        let _ = view.paint(state(&mut views, id), None);
        assert_eq!(view.last_drawn_crossing_count(), 1);
        assert_eq!(view.last_hidden_crossing_count(), 1);
    }

    #[test]
    fn moving_the_mouse_reports_the_station_and_elevation_under_it() {
        // The readout goes out as frame stats. At the middle of the plot, (425, 142), a framed
        // 1000 m section reads its box's middle: station 500, elevation (10 + 20) / 2 = 15, at the
        // default exaggeration of 10.
        let (mut views, id) = section_state();
        let mut view = SectionView::new();
        view.resize(state(&mut views, id), 800, 300);
        view.set_section(state(&mut views, id), None, ground(1000.0));
        let _ = view.paint(state(&mut views, id), None);
        let _ = view.take_events();

        view.mouse_move(state(&mut views, id), PointF::new(425.0, 142.0));
        let readout = view
            .take_events()
            .into_iter()
            .filter_map(|event| match event {
                SectionViewEvent::FrameStats(text) => Some(text),
                SectionViewEvent::Activated => None,
            })
            .next_back()
            .expect("a readout");
        assert_eq!(readout, "Station 500.000   Elevation 15.000   (V x10.0)");
    }

    // ---- the selection on the crossings (cad's selection_style) -----------------------------

    /// A cut from (0, 40) to (100, 40) and a line from (20, 0) to (20, 100) on "pipes" crossing
    /// it: by hand, at station 20. No surface, so the view frames the stations alone: the 726 px
    /// plot of an 800 px view (800 - 62 - 12) for 100 m, station 20 at x = 62 + 20 x 7.26 =
    /// 207.2.
    struct CutPipe {
        document: Document,
        views: ViewSet,
        id: ViewId,
        pipe: EntityId,
        view: SectionView,
    }

    impl CutPipe {
        fn new() -> Self {
            let mut document = Document::new(None);
            document
                .execute(create_layer(Layer {
                    name: String::from("pipes"),
                    ..Layer::default()
                }))
                .expect("the layer is made");
            let attributes = EntityAttributes {
                layer: String::from("pipes"),
                ..EntityAttributes::default()
            };
            document
                .execute(create_line(
                    Point2::new(20.0, 0.0),
                    Point2::new(20.0, 100.0),
                    attributes,
                ))
                .expect("the line is made");
            let pipe = document.last_created_entities()[0];
            let cut = Polyline2::new(
                vec![Point2::new(0.0, 40.0), Point2::new(100.0, 40.0)],
                false,
            );
            let section = extract_section(
                &cut,
                &[],
                Some(document.model()),
                &SectionOptions::default(),
            );
            let (mut views, id) = section_state();
            let mut view = SectionView::new();
            view.set_document(Some(&document));
            view.resize(state(&mut views, id), 800, 300);
            match section {
                Ok(section) => {
                    assert_eq!(section.crossings.len(), 1);
                    if let Some(crossing) = section.crossings.first() {
                        assert_near(crossing.station, 20.0, 1e-9, "the crossing's station");
                        assert_eq!(crossing.entity, pipe);
                    }
                    view.set_section(state(&mut views, id), Some(&document), section);
                }
                Err(error) => panic!("{}", error.describe()),
            }
            Self {
                document,
                views,
                id,
                pipe,
                view,
            }
        }

        fn state(&mut self) -> &mut ViewState {
            state(&mut self.views, self.id)
        }

        fn select(&mut self) {
            self.document.selection_mut().add(self.pipe);
            self.document.notify_selection_changed();
        }

        /// Pixels of columns 204 to 210, top to bottom of the plot, in the selection's orange:
        /// red above 200, green 120 to 200, blue under 90 - never the crossings' dashed
        /// (210, 90, 90), the grid's greys or the ground.
        fn orange_ink(&mut self) -> usize {
            let state = state(&mut self.views, self.id);
            let image = self.view.paint(state, Some(&self.document)).clone();
            let mut ink = 0;
            for y in 10..(image.height() as i32 - 26) {
                for x in 204..=210 {
                    let c = image.pixel(x as u32, y as u32);
                    if c.r > 200 && (120..=200).contains(&c.g) && c.b < 90 {
                        ink += 1;
                    }
                }
            }
            ink
        }
    }

    #[test]
    fn a_selected_entitys_crossing_is_drawn_solid_in_the_selection_colour() {
        let mut cut = CutPipe::new();
        let unselected = cut.orange_ink();
        assert_eq!(cut.view.last_selected_crossing_count(), 0);
        assert_eq!(cut.view.last_drawn_crossing_count(), 1);

        cut.select();
        let selected = cut.orange_ink();
        assert_eq!(cut.view.last_selected_crossing_count(), 1);
        assert_eq!(
            cut.view.last_drawn_crossing_count(),
            1,
            "drawn once, as selected"
        );
        assert_eq!(cut.view.last_ghost_crossing_count(), 0);
        // Solid down the 264 px plot (300 - 10 - 26), 2 px wide: hundreds of pixels, where the
        // dashed crossing had none of this colour.
        assert_eq!(unselected, 0);
        assert!(selected > 264, "{selected}");
    }

    #[test]
    fn a_selected_crossing_on_a_layer_the_view_hides_is_a_ghost() {
        let mut cut = CutPipe::new();
        assert!(cut.state().layers.hide("pipes"));
        cut.select();
        let _ = cut.orange_ink();
        assert_eq!(cut.view.last_hidden_crossing_count(), 1);
        assert_eq!(cut.view.last_drawn_crossing_count(), 0);
        assert_eq!(cut.view.last_selected_crossing_count(), 0);
        assert_eq!(cut.view.last_ghost_crossing_count(), 1);

        // The view's switch off: no ghost.
        cut.state().selection_ghosts = false;
        let _ = cut.orange_ink();
        assert_eq!(cut.view.last_ghost_crossing_count(), 0);

        // On again, but the drawing hides the layer: hidden everywhere.
        cut.state().selection_ghosts = true;
        let mut pipes = cut
            .document
            .model()
            .layers
            .find("pipes")
            .expect("the layer is there")
            .clone();
        pipes.visible = false;
        cut.document
            .execute(update_layer(pipes))
            .expect("the layer is hidden");
        let _ = cut.orange_ink();
        assert_eq!(cut.view.last_ghost_crossing_count(), 0);
    }

    #[test]
    fn an_unselected_crossing_on_a_hidden_layer_stays_out_with_another_selected() {
        // Only a selected entity's crossing is ghosted: another selection leaves the hidden
        // crossing out, as before.
        let mut cut = CutPipe::new();
        assert!(cut.state().layers.hide("pipes"));
        cut.document
            .execute(create_line(
                Point2::new(60.0, 0.0),
                Point2::new(70.0, 0.0),
                EntityAttributes::default(),
            ))
            .expect("the line is made");
        let other = cut.document.last_created_entities()[0];
        cut.document.selection_mut().add(other);
        cut.document.notify_selection_changed();
        let _ = cut.orange_ink();
        assert_eq!(cut.view.last_hidden_crossing_count(), 1);
        assert_eq!(cut.view.last_ghost_crossing_count(), 0);
    }

    /// A store's surface of one triangle.
    fn triangle() -> Arc<TinSurface> {
        let surface = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 1.0),
                Point3::new(10.0, 0.0, 2.0),
                Point3::new(0.0, 10.0, 3.0),
            ],
            vec![[0, 1, 2]],
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        Arc::new(surface)
    }

    fn last_stats(view: &mut SectionView) -> String {
        view.take_events()
            .into_iter()
            .filter_map(|event| match event {
                SectionViewEvent::FrameStats(text) => Some(text),
                SectionViewEvent::Activated => None,
            })
            .next_back()
            .expect("the paint said what it shows")
    }

    #[test]
    fn a_section_says_its_surfaces_changed_since_it_was_cut_and_cutting_again_clears_it() {
        // The section is cut again only when asked, so a surface hidden, edited, removed or put
        // back by an undo leaves the old picture on screen: the view must say that it is of
        // surfaces the drawing no longer has.
        let mut document = Document::new(None);
        let (mut views, id) = section_state();
        let mut view = SectionView::new();
        view.set_document(Some(&document));
        view.resize(state(&mut views, id), 800, 300);

        assert!(
            !SectionView::surfaces_changed(state(&mut views, id), Some(&document)),
            "no section, nothing to be out of date"
        );
        view.set_section(state(&mut views, id), Some(&document), ground(100.0));
        let _ = view.paint(state(&mut views, id), Some(&document));
        assert!(!SectionView::surfaces_changed(state(&mut views, id), Some(&document)));
        let stats = last_stats(&mut view);
        assert!(!stats.contains("surfaces changed"), "{stats}");

        // A change to the drawing's surfaces: a triangle added.
        let mut store = document.surfaces().clone();
        store
            .add(NamedSurface {
                name: String::from("ground"),
                surface: Some(triangle()),
                source: String::from("test"),
                ..NamedSurface::default()
            })
            .expect("the surface is added");
        document
            .set_surfaces(store.clone(), "TEST")
            .expect("the surfaces are set");
        assert!(SectionView::surfaces_changed(state(&mut views, id), Some(&document)));
        let _ = view.paint(state(&mut views, id), Some(&document));
        let stats = last_stats(&mut view);
        assert!(stats.contains("surfaces changed since the cut"), "{stats}");

        // Hiding it is a change too.
        store
            .set_visible("ground", false)
            .expect("the surface is hidden");
        document
            .set_surfaces(store, "TEST")
            .expect("the surfaces are set");
        assert!(SectionView::surfaces_changed(state(&mut views, id), Some(&document)));

        // Cut again: the section is of the surfaces the drawing has now.
        view.set_section(state(&mut views, id), Some(&document), ground(100.0));
        assert!(!SectionView::surfaces_changed(state(&mut views, id), Some(&document)));
        let _ = view.paint(state(&mut views, id), Some(&document));
        let stats = last_stats(&mut view);
        assert!(!stats.contains("surfaces changed"), "{stats}");
        // And an undo of the hide changes them again.
        document.undo().expect("the undo is done");
        assert!(SectionView::surfaces_changed(state(&mut views, id), Some(&document)));
    }
}
