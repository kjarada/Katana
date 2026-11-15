//! The twins of tests/qt_widgets/test_section_recut.cpp's cases of the section view itself: an
//! open section is cut again when the drawing's surfaces change, with the pan and zoom a person
//! made kept (section_view_widget's replace_section).
//!
//! Not twinned: the nine cases that drive the cut again through the workspace
//! (ViewWorkspace::recutSections, view_workspace, wave 3) -
//! SectionRecut.AnOpenSectionFollowsASurfaceEditAnUndoAndAHideAfterItWasCut,
//! HidingASurfaceTakesItsLineOutOfTheOpenSectionAndShowingItPutsItBack,
//! ASectionThatCannotBeCutAgainSaysItsSurfacesChangedAndKeepsItsPicture,
//! TheDesignProfileAndItsNameRideIntoTheCutAgain, ASectionCutAlongALineHasNoDesignAndGetsNone,
//! ASectionUpToDateIsNotCutAgainSoAskingTwiceCutsOnce, AWorkspaceWithNoSectionAsksForNoCut,
//! ASectionAViewKeepsAfterItWasTurnedToAnotherKindIsCutAgainToo and
//! ASectionIsCutAgainFromItsOwnAlignmentWhateverTheDrawingHolds.

mod section_recut {
    use std::sync::Arc;

    use katana_cad::Document;
    use katana_cad::section::{
        SampleReason, Section, SectionDesign, SectionSample, SectionSurface,
    };
    use katana_cad::view_set::{ViewId, ViewSet, ViewState};
    use katana_cad::viewport_layout::ViewKind;
    use katana_geometry::profile::solve_profile;
    use katana_geometry::{Point2, Point3, Polyline2, ProfilePvi, VerticalAlignment};
    use katana_terrain::{NamedSurface, TinSurface};
    use katana_window::canvas::PointF;
    use katana_window::section_view_widget::SectionView;

    use crate::support::{assert_near, double_eq};

    /// A rectangle x 0..100, y 0..20, `west` high at x = 0 and `east` at x = 100.
    fn slab(west: f64, east: f64) -> Arc<TinSurface> {
        let surface = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, west),
                Point3::new(100.0, 0.0, east),
                Point3::new(100.0, 20.0, east),
                Point3::new(0.0, 20.0, west),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        Arc::new(surface)
    }

    /// Two samples, `low` at station 0 and `high` at station 100, on a 100 m alignment.
    fn ramp(low: f64, high: f64) -> Section {
        Section {
            alignment: Polyline2::new(
                vec![Point2::new(0.0, 0.0), Point2::new(100.0, 0.0)],
                false,
            ),
            length: 100.0,
            surfaces: vec![SectionSurface {
                name: String::from("ground"),
                samples: vec![
                    SectionSample {
                        station: 0.0,
                        elevation: Some(low),
                        ..SectionSample::default()
                    },
                    SectionSample {
                        station: 100.0,
                        plan: Point2::new(100.0, 0.0),
                        elevation: Some(high),
                    },
                ],
                reasons: vec![SampleReason::Start, SampleReason::End],
                min_elevation: Some(low),
                max_elevation: Some(high),
            }],
            ..Section::default()
        }
    }

    /// A section view over a drawing, 800 x 300, given `ramp(10, 20)` and painted.
    fn framed() -> (Document, ViewSet, ViewId, SectionView) {
        let document = Document::new(None);
        let mut views = ViewSet::new();
        let id = views.add(ViewKind::Section).id;
        let mut view = SectionView::new();
        view.set_document(Some(&document));
        let state = state(&mut views, id);
        view.resize(state, 800, 300);
        view.set_section(state, Some(&document), ramp(10.0, 20.0));
        let _ = view.paint(state, Some(&document));
        (document, views, id, view)
    }

    fn state(views: &mut ViewSet, id: ViewId) -> &mut ViewState {
        views.find_mut(id).expect("the view is open")
    }

    #[test]
    fn a_cut_again_keeps_the_pan_and_zoom_a_person_made_and_the_middle_of_the_plot_stays_where_it_was()
     {
        let (document, mut views, id, mut view) = framed();
        let state = state(&mut views, id);

        // The person zooms in about a point: a station and elevation under the pixel stay under
        // it, and the view is now theirs. The section is then replaced by one 100 m higher, as a
        // re-cut after a surface raised by that much would be.
        view.zoom_at(state, PointF::new(200.0, 100.0), 2.0);
        let _ = view.paint(state, Some(&document));
        let middle = view.plot_centre();
        let before = view.station_elevation_at(state, middle);
        view.replace_section(state, Some(&document), ramp(110.0, 120.0));
        let _ = view.paint(state, Some(&document));
        let after = view.station_elevation_at(state, middle);
        assert_near(after.x, before.x, 1e-9, "the station at the middle of the plot");
        assert_near(after.y, before.y, 1e-9, "and its elevation: not framed afresh");
        assert!(
            after.y < 100.0,
            "so the new, higher section is out of view, as it was cut"
        );
        let stored = state
            .section
            .as_ref()
            .and_then(|section| section.surfaces.first())
            .and_then(|surface| surface.samples.last())
            .and_then(|sample| sample.elevation)
            .expect("the new section's east end");
        assert!(double_eq(stored, 120.0), "{stored}");
    }

    #[test]
    fn a_view_not_moved_by_hand_is_framed_afresh_on_the_new_section() {
        // Still fitted to all of its section, so a section 100 m higher is fitted: the middle of
        // the plot shows the middle of the new section's box, (110 + 120) / 2.
        let (document, mut views, id, mut view) = framed();
        let state = state(&mut views, id);
        assert_near(
            view.station_elevation_at(state, view.plot_centre()).y,
            15.0,
            1e-9,
            "the first section's middle",
        );

        view.replace_section(state, Some(&document), ramp(110.0, 120.0));
        let _ = view.paint(state, Some(&document));
        let middle = view.station_elevation_at(state, view.plot_centre());
        assert_near(middle.y, 115.0, 1e-9, "the new section's middle elevation");
        assert_near(middle.x, 50.0, 1e-9, "the new section's middle station");
    }

    #[test]
    fn a_section_replaced_is_of_the_surfaces_the_drawing_has_now() {
        let mut document = Document::new(None);
        let mut views = ViewSet::new();
        let id = views.add(ViewKind::Section).id;
        let mut view = SectionView::new();
        view.set_document(Some(&document));
        view.resize(state(&mut views, id), 800, 300);
        view.set_section(state(&mut views, id), Some(&document), ramp(10.0, 20.0));
        let mut store = document.surfaces().clone();
        store
            .add(NamedSurface {
                name: String::from("ground"),
                surface: Some(slab(10.0, 20.0)),
                source: String::from("test"),
                ..NamedSurface::default()
            })
            .expect("the surface is added");
        document
            .set_surfaces(store, "TEST")
            .expect("the surfaces are set");
        assert!(SectionView::surfaces_changed(
            state(&mut views, id),
            Some(&document)
        ));
        view.replace_section(state(&mut views, id), Some(&document), ramp(10.0, 20.0));
        assert!(!SectionView::surfaces_changed(
            state(&mut views, id),
            Some(&document)
        ));
    }

    #[test]
    fn a_new_cut_forgets_the_design_the_last_one_had() {
        // set_section is a new cut: the design of the cut before it is not the recipe of this one
        // (the workspace's show_section sets it when there is one).
        let document = Document::new(None);
        let mut views = ViewSet::new();
        let id = views.add(ViewKind::Section).id;
        let mut view = SectionView::new();
        view.set_document(Some(&document));
        let definition = VerticalAlignment {
            pvis: vec![
                ProfilePvi {
                    station: 0.0,
                    elevation: 16.0,
                    ..ProfilePvi::default()
                },
                ProfilePvi {
                    station: 100.0,
                    elevation: 18.0,
                    ..ProfilePvi::default()
                },
            ],
        };
        let design = || SectionDesign {
            profile: solve_profile(&definition).unwrap_or_else(|error| panic!("{}", error.describe())),
            name: String::from("design"),
        };
        state(&mut views, id).section_design = Some(design());
        view.set_section(state(&mut views, id), Some(&document), ramp(10.0, 20.0));
        assert!(state(&mut views, id).section_design.is_none());
        state(&mut views, id).section_design = Some(design());
        view.clear_section(state(&mut views, id));
        assert!(state(&mut views, id).section_design.is_none());
    }
}
