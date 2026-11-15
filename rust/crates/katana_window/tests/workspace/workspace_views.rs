//! The cases of tests/qt_widgets/test_section_recut.cpp and test_render_view.cpp that drive their
//! views through the workspace (views/section_recut.rs and views/render_view.rs twin the rest): an
//! open section is cut again from its own recipe as the drawing's surfaces change, and a new
//! drawing leaves nothing of the previous one's surfaces in the 3D view or its sections.
//!
//! The surfaces are planes with corners at exact heights, so what a cut reads at the east end is
//! worked out on paper: ground z = 10 + x / 10 over x 0..100 (20 at the east end), raised
//! z = 15 + x / 10 (25).

mod section_recut {
    use std::sync::Arc;

    use katana_cad::Document;
    use katana_cad::section::{Section, SectionDesign};
    use katana_cad::terrain::section_cut::cut_drawing_section;
    use katana_cad::view_set::{ViewId, ViewState};
    use katana_cad::viewport_layout::ViewKind;
    use katana_core::ErrorCode;
    use katana_geometry::profile::solve_profile;
    use katana_geometry::{Point2, Point3, Polyline2, ProfilePvi, VerticalAlignment};
    use katana_terrain::{NamedSurface, TinSurface};
    use katana_window::section_view_widget::SectionView;

    use crate::support::{Site, assert_near};

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

    fn add_surface(document: &mut Document, name: &str, surface: Arc<TinSurface>) {
        let mut store = document.surfaces().clone();
        assert!(
            store
                .add(NamedSurface {
                    name: String::from(name),
                    surface: Some(surface),
                    source: String::from("test"),
                    ..NamedSurface::default()
                })
                .is_ok()
        );
        assert!(document.set_surfaces(store, "TEST").is_ok());
    }

    fn replace_surface(document: &mut Document, name: &str, surface: Arc<TinSurface>) {
        let mut store = document.surfaces().clone();
        assert!(store.replace(name, Some(surface)).is_ok());
        assert!(document.set_surfaces(store, "EDIT").is_ok());
    }

    fn set_visible(document: &mut Document, name: &str, visible: bool) {
        let mut store = document.surfaces().clone();
        assert!(store.set_visible(name, visible).is_ok());
        assert!(document.set_surfaces(store, "VISIBLE").is_ok());
    }

    fn east_along(y: f64) -> Polyline2 {
        Polyline2::new(vec![Point2::new(0.0, y), Point2::new(100.0, y)], false)
    }

    /// The elevation of the section's `series` at its last station.
    fn east_end(section: &Section, series: usize) -> f64 {
        section.surfaces[series]
            .samples
            .last()
            .and_then(|sample| sample.elevation)
            .expect("an elevation at the east end")
    }

    /// The workspace over a drawing, and how many times it asked for a cut.
    struct Recut {
        w: Site,
        cuts: usize,
    }

    impl Recut {
        fn new() -> Self {
            Self {
                w: Site::sized(900.0, 600.0),
                cuts: 0,
            }
        }

        fn document(&mut self) -> &mut Document {
            self.w.document_mut()
        }

        /// What the window's recut does: every section cut again with cut_drawing_section.
        fn recut(&mut self) -> katana_window::view_workspace::SectionRecut {
            let document = self.w.host.document();
            let cuts = &mut self.cuts;
            self.w.views.recut_sections(document, &mut |alignment, design| {
                *cuts += 1;
                cut_drawing_section(document, alignment, design).map(|cut| cut.section)
            })
        }

        /// The state of the section view (the workspace opens a plan view of its own first).
        fn section_state(&self) -> Option<ViewState> {
            self.w
                .views
                .view_set()
                .borrow()
                .views()
                .into_iter()
                .find(|state| state.kind == ViewKind::Section)
                .cloned()
        }

        /// A first cut shown in a section view, as Terrain > Cut Section shows it: its id.
        fn show(&mut self, alignment: &Polyline2, design: Option<SectionDesign>) -> ViewId {
            let cut = cut_drawing_section(self.w.host.document(), alignment, design.as_ref())
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            assert!(
                self.w
                    .views
                    .show_section(self.w.host.document_mut(), cut.section, design)
            );
            self.w.frame();
            self.section_state().expect("a section view").id
        }

        fn section(&self, id: ViewId) -> Section {
            self.w.state(id).section.expect("the view holds a section")
        }

        fn surfaces_changed(&self, id: ViewId) -> bool {
            SectionView::surfaces_changed(&self.w.state(id), Some(self.w.host.document()))
        }
    }

    #[test]
    fn an_open_section_follows_a_surface_edit_an_undo_and_a_hide_after_it_was_cut() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let view = r.show(&east_along(10.0), None);
        assert!(SectionView::has_section(&r.w.state(view)));
        assert_near(east_end(&r.section(view), 0), 20.0, 1e-9);
        assert!(!r.surfaces_changed(view));

        // Edited: the ground raised by five. The view is of the old one until it is cut again,
        // which says so; cut again, it is the new ground and says nothing.
        replace_surface(r.document(), "ground", slab(15.0, 25.0));
        assert!(r.surfaces_changed(view));
        let recut = r.recut();
        assert_eq!(recut.cut.len(), 1);
        assert!(recut.refused.is_empty());
        assert_near(east_end(&r.section(view), 0), 25.0, 1e-9);
        assert!(!r.surfaces_changed(view));

        // Undone: the first ground again.
        assert!(r.document().undo().is_ok());
        assert!(r.surfaces_changed(view));
        let recut = r.recut();
        assert_eq!(recut.cut.len(), 1);
        assert_near(east_end(&r.section(view), 0), 20.0, 1e-9);
        assert!(!r.surfaces_changed(view));
    }

    #[test]
    fn hiding_a_surface_takes_its_line_out_of_the_open_section_and_showing_it_puts_it_back() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        add_surface(r.document(), "raised", slab(15.0, 25.0));
        let view = r.show(&east_along(10.0), None);
        assert_eq!(r.section(view).surfaces.len(), 2);

        set_visible(r.document(), "raised", false);
        let recut = r.recut();
        assert_eq!(recut.cut.len(), 1);
        assert_eq!(
            r.section(view).surfaces.len(),
            1,
            "a hidden surface is in no picture"
        );
        assert_eq!(r.section(view).surfaces[0].name, "ground");

        set_visible(r.document(), "raised", true);
        let recut = r.recut();
        assert_eq!(recut.cut.len(), 1);
        assert_eq!(r.section(view).surfaces.len(), 2);
        assert_eq!(r.section(view).surfaces[1].name, "raised");
        assert_near(east_end(&r.section(view), 1), 25.0, 1e-9);
    }

    #[test]
    fn a_section_that_cannot_be_cut_again_says_its_surfaces_changed_and_keeps_its_picture() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let view = r.show(&east_along(10.0), None);

        set_visible(r.document(), "ground", false);
        let recut = r.recut();
        assert!(recut.cut.is_empty());
        assert_eq!(recut.refused.len(), 1);
        assert_eq!(recut.refused[0].1.code, ErrorCode::InvalidState);
        assert!(SectionView::has_section(&r.w.state(view)));
        assert_near(east_end(&r.section(view), 0), 20.0, 1e-9);
        assert!(
            r.surfaces_changed(view),
            "and it says it is of surfaces that changed"
        );

        set_visible(r.document(), "ground", true);
        let recut = r.recut();
        assert_eq!(recut.cut.len(), 1);
        assert!(!r.surfaces_changed(view));
    }

    fn design() -> SectionDesign {
        // Two PVIs, (0, 16) and (100, 18): a straight 2% grade, 18 at the east end.
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
        let solved = solve_profile(&definition).unwrap_or_else(|error| panic!("{}", error.describe()));
        SectionDesign {
            profile: solved,
            name: String::from("design Main"),
        }
    }

    #[test]
    fn the_design_profile_and_its_name_ride_into_the_cut_again() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let view = r.show(&east_along(10.0), Some(design()));
        assert_eq!(r.section(view).surfaces.len(), 2);
        assert_near(east_end(&r.section(view), 1), 18.0, 1e-9);

        replace_surface(r.document(), "ground", slab(15.0, 25.0));
        let recut = r.recut();
        assert_eq!(recut.cut.len(), 1);
        assert_eq!(
            r.section(view).surfaces.len(),
            2,
            "the design is cut beside the ground again"
        );
        assert_eq!(r.section(view).surfaces[1].name, "design Main");
        assert_near(east_end(&r.section(view), 0), 25.0, 1e-9);
        assert_near(east_end(&r.section(view), 1), 18.0, 1e-9);
        assert!(
            r.section_state()
                .is_some_and(|state| state.section_design.is_some())
        );
    }

    #[test]
    fn a_section_cut_along_a_line_has_no_design_and_gets_none() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let view = r.show(&east_along(10.0), None);
        replace_surface(r.document(), "ground", slab(15.0, 25.0));
        assert_eq!(r.recut().cut.len(), 1);
        assert_eq!(r.section(view).surfaces.len(), 1);
        assert!(
            r.section_state()
                .is_some_and(|state| state.section_design.is_none())
        );
    }

    #[test]
    fn a_section_up_to_date_is_not_cut_again_so_asking_twice_cuts_once() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let _ = r.show(&east_along(10.0), None);
        assert!(
            r.recut().cut.is_empty(),
            "nothing has changed since the cut"
        );
        assert_eq!(r.cuts, 0);

        replace_surface(r.document(), "ground", slab(15.0, 25.0));
        assert_eq!(r.recut().cut.len(), 1);
        assert!(r.recut().cut.is_empty());
        assert_eq!(r.cuts, 1, "once for the one change");
    }

    #[test]
    fn a_workspace_with_no_section_asks_for_no_cut() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let recut = r.recut();
        assert!(recut.cut.is_empty());
        assert!(recut.refused.is_empty());
        assert_eq!(r.cuts, 0);
    }

    #[test]
    fn a_section_a_view_keeps_after_it_was_turned_to_another_kind_is_cut_again_too() {
        // Its state holds the section and no widget does, so it is cut again in the state.
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let cut = cut_drawing_section(r.w.host.document(), &east_along(10.0), None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let generation = r.w.host.document().surfaces_generation();
        let plan = {
            let views = r.w.views.view_set();
            let mut views = views.borrow_mut();
            let state = views.add(ViewKind::Plan);
            state.section = Some(cut.section);
            state.section_surfaces = generation;
            state.id
        };

        replace_surface(r.document(), "ground", slab(15.0, 25.0));
        let recut = r.recut();
        assert_eq!(recut.cut.len(), 1);
        assert_eq!(recut.cut[0], plan);
        let state = r.w.state(plan);
        assert_near(
            east_end(state.section.as_ref().expect("a section"), 0),
            25.0,
            1e-9,
        );
        assert_eq!(
            state.section_surfaces,
            r.w.host.document().surfaces_generation()
        );
    }

    #[test]
    fn a_section_is_cut_again_from_its_own_alignment_whatever_the_drawing_holds() {
        let mut r = Recut::new();
        add_surface(r.document(), "ground", slab(10.0, 20.0));
        let view = r.show(&east_along(5.0), None);
        replace_surface(r.document(), "ground", slab(15.0, 25.0));
        assert_eq!(r.recut().cut.len(), 1);
        let alignment = r.section(view).alignment;
        assert_eq!(alignment.vertices.len(), 2);
        assert_near(alignment.vertices[0].y, 5.0, 1e-12);
        assert_near(alignment.vertices[1].y, 5.0, 1e-12);
    }
}

mod render_view {
    use katana_cad::scene::SceneSurface;
    use katana_cad::section::Section;
    use katana_cad::viewport_layout::ViewKind;
    use katana_geometry::Point3;
    use katana_terrain::{TinBuildOptions, TinInput, build_tin};
    use katana_window::view_workspace::Scenery;

    use crate::support::Site;

    #[test]
    fn a_new_drawing_leaves_no_surface_of_the_previous_one_in_the_three_d_view() {
        // Surfaces are lent to the views by the window, not kept in the document, and the new
        // drawing's notification marks only the entities dirty. The window empties its surfaces
        // and tells the workspace, as done here, and a section cut from those surfaces goes too.
        let mut w = Site::new();
        let mut input = TinInput::default();
        for (x, y, z) in [
            (0.0, 0.0, 10.0),
            (100.0, 0.0, 12.0),
            (100.0, 100.0, 14.0),
            (0.0, 100.0, 11.0),
            (50.0, 50.0, 13.0),
        ] {
            input.points.push(Point3::new(x, y, z));
        }
        let built = build_tin(&input, &TinBuildOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut surfaces = vec![SceneSurface {
            name: String::from("Ground"),
            surface: Some(&built.surface),
            ..SceneSurface::default()
        }];
        // setSurfaces: the views rebuild their scenes with the window's list.
        w.views.scenery_changed();

        let model = w.open(ViewKind::Model3D);
        let terrain_empty = |w: &Site, surfaces: &[SceneSurface<'_>]| {
            let scenery = Scenery {
                surfaces,
                meshes: &[],
            };
            w.views
                .with_render_view(model, w.host.document(), scenery, |view, scene, state| {
                    let _ = view.paint(scene, state);
                    view.scene_layers().terrain.empty()
                })
                .expect("a 3D view")
        };
        w.views.scenery_changed();
        assert!(!terrain_empty(&w, &surfaces), "the surface was never drawn");

        let section = Section {
            length: 100.0,
            ..Section::default()
        };
        assert!(w.views.show_section(w.host.document_mut(), section, None));
        let cut = w
            .views
            .view_set()
            .borrow()
            .most_recent(ViewKind::Section)
            .map(|state| state.id)
            .expect("a section view");
        assert!(w.state(cut).section.is_some());

        // What the window does for File > New: the new drawing, then the window's own surfaces
        // emptied and the workspace told.
        w.document_mut().new_document();
        surfaces.clear();
        w.views.drawing_replaced();

        assert!(
            terrain_empty(&w, &surfaces),
            "the previous drawing's surface is still in the 3D view"
        );
        assert!(
            w.state(cut).section.is_none(),
            "the previous drawing's section is still shown"
        );
    }
}
