// A section of the drawing (terrain/section_cut.rs;
// tests/cad/terrain/test_section_cut.cpp): the shown surfaces along an
// alignment, with a design profile beside them. What an open section view
// keeps - the section's own alignment and its design - is what cuts it again,
// so the cut is held to values worked by hand.
//
// The surfaces are planes with corners at exact heights, so the elevation of a
// cut at any station is a line worked out on paper, never read from a run:
//
//   ground  z = 10 + x / 10 over x 0..100, y 0..20  (10 at the west edge, 20 at the east)
//   raised  z = 15 + x / 10                         (ground, five metres up)
//   flat    z = 10 over x 0..300, y 0..20           (for the design profile's section)
//
// Each is two triangles split along the diagonal (0,0) - (x1,20), which the
// line y = 10 crosses at x = x1 / 2 and the line y = 10.01 at x = 10.01 / 0.2
// = 50.05 on the 100 m slabs: the triangle edge the cut must find. 50.05 is
// not near a multiple of the 0.1 m interval, which a break at 50 would be (a
// break that lands on an interval station is dropped as a duplicate of it:
// section.rs).

use std::sync::Arc;

use crate::{double_eq, near, ok, tin};
use katana_cad::document::Document;
use katana_cad::section::{SampleReason, SectionDesign};
use katana_cad::terrain::section_cut::{cut_drawing_section, drawing_section_interval};
use katana_commands::{EntityAttributes, create_line};
use katana_core::ErrorCode;
use katana_geometry::profile::solve_profile;
use katana_geometry::{Point2, Point3, Polyline2, ProfilePvi, SolvedProfile, VerticalAlignment};
use katana_terrain::{NamedSurface, TinSurface};

/// A rectangle x 0..width, y 0..20 whose height is `west` at x = 0 and `east`
/// at x = width, as two triangles split on the diagonal from (0,0) to
/// (width,20).
fn slab(width: f64, west: f64, east: f64) -> Arc<TinSurface> {
    Arc::new(tin(
        vec![
            Point3::new(0.0, 0.0, west),
            Point3::new(width, 0.0, east),
            Point3::new(width, 20.0, east),
            Point3::new(0.0, 20.0, west),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    ))
}

fn add_surface(document: &mut Document, name: &str, surface: Arc<TinSurface>) {
    let mut store = document.surfaces().clone();
    ok(store.add(NamedSurface {
        name: String::from(name),
        surface: Some(surface),
        source: String::from("test"),
        ..NamedSurface::default()
    }));
    ok(document.set_surfaces(store, "TEST"));
}

fn set_visible(document: &mut Document, name: &str, visible: bool) {
    let mut store = document.surfaces().clone();
    ok(store.set_visible(name, visible));
    ok(document.set_surfaces(store, "TEST"));
}

/// East along y = `y` from x = 0 to x = `length`: station s is the point
/// (s, y).
fn east_along(length: f64, y: f64) -> Polyline2 {
    Polyline2::new(vec![Point2::new(0.0, y), Point2::new(length, y)], false)
}

/// The textbook sag the section tests use: PVIs (0, 16), (100, 13, L = 100),
/// (300, 17), -3% into +2%: PVC 50 @ 14.5, PVT 150 @ 14.0, the low point at
/// station 110 @ 13.6 (x = 0.03 * 100 / 0.05 = 60 past the PVC).
fn sag() -> SolvedProfile {
    let definition = VerticalAlignment {
        pvis: vec![
            ProfilePvi {
                station: 0.0,
                elevation: 16.0,
                curve_length: 0.0,
            },
            ProfilePvi {
                station: 100.0,
                elevation: 13.0,
                curve_length: 100.0,
            },
            ProfilePvi {
                station: 300.0,
                elevation: 17.0,
                curve_length: 0.0,
            },
        ],
    };
    match solve_profile(&definition) {
        Ok(profile) => profile,
        Err(error) => panic!("{}", error.describe()),
    }
}

mod section_cut {
    use super::*;

    #[test]
    fn the_shown_surfaces_are_cut_along_the_alignment_at_the_drawings_interval() {
        let mut document = Document::default();
        add_surface(&mut document, "ground", slab(100.0, 10.0, 20.0));
        let cut = match cut_drawing_section(&document, &east_along(100.0, 10.01), None) {
            Ok(cut) => cut,
            Err(error) => panic!("{}", error.describe()),
        };

        // 100 m of alignment at 100 / 2000 = 0.05 m is below the 0.1 m floor:
        // 0.1 m, so a thousand intervals and at least 1,001 stations.
        double_eq(cut.interval, 0.1);
        near(cut.section.length, 100.0, 1e-9);
        assert_eq!(cut.section.surfaces.len(), 1);
        let ground = &cut.section.surfaces[0];
        assert_eq!(ground.name, "ground");
        assert!(ground.samples.len() >= 1001);

        // Every station is on the plane z = 10 + s / 10, to the arithmetic of
        // one barycentric interpolation; the first and the last are the
        // plane's ends.
        let first = &ground.samples[0];
        let last = &ground.samples[ground.samples.len() - 1];
        near(first.station, 0.0, 1e-12);
        near(first.elevation.expect("an elevation"), 10.0, 1e-9);
        near(last.station, 100.0, 1e-9);
        near(last.elevation.expect("an elevation"), 20.0, 1e-9);
        for sample in &ground.samples {
            let elevation = sample.elevation.expect("every station has an elevation");
            near(elevation, 10.0 + sample.station / 10.0, 1e-9);
        }
        // The diagonal (0,0) - (100,20) is crossed by y = 10.01 at x = 50.05,
        // height 10 + 5.005 = 15.005: the triangle edge, which the cut places
        // a sample on.
        let at = ground
            .samples
            .iter()
            .zip(&ground.reasons)
            .find(|(sample, reason)| {
                **reason == SampleReason::SurfaceBreak && (sample.station - 50.05).abs() < 1e-5
            })
            .map(|(sample, _)| sample)
            .expect("no sample on the triangle edge at 50.05");
        near(at.elevation.expect("an elevation"), 15.005, 1e-6);
    }

    #[test]
    fn a_hidden_surface_is_left_out_of_the_cut_and_put_back_when_shown() {
        let mut document = Document::default();
        add_surface(&mut document, "ground", slab(100.0, 10.0, 20.0));
        add_surface(&mut document, "raised", slab(100.0, 15.0, 25.0));
        let cut = cut_drawing_section(&document, &east_along(100.0, 10.0), None).expect("a cut");
        assert_eq!(cut.section.surfaces.len(), 2);

        set_visible(&mut document, "raised", false);
        let cut = cut_drawing_section(&document, &east_along(100.0, 10.0), None).expect("a cut");
        assert_eq!(
            cut.section.surfaces.len(),
            1,
            "a hidden surface is in no picture"
        );
        assert_eq!(cut.section.surfaces[0].name, "ground");

        set_visible(&mut document, "raised", true);
        let cut = cut_drawing_section(&document, &east_along(100.0, 10.0), None).expect("a cut");
        assert_eq!(cut.section.surfaces.len(), 2);
        // The raised surface is the ground plus five, at every station:
        // z = 15 + s / 10.
        for sample in &cut.section.surfaces[1].samples {
            let elevation = sample.elevation.expect("an elevation");
            near(elevation, 15.0 + sample.station / 10.0, 1e-9);
        }
    }

    #[test]
    fn a_drawing_with_no_surface_is_not_found_and_one_with_every_one_hidden_is_invalid_state() {
        // Two refusals a caller words differently - build one, or show one -
        // so each has its own code.
        let mut document = Document::default();
        let error =
            cut_drawing_section(&document, &east_along(100.0, 10.0), None).expect_err("no surface");
        assert_eq!(error.code, ErrorCode::NotFound);

        add_surface(&mut document, "ground", slab(100.0, 10.0, 20.0));
        set_visible(&mut document, "ground", false);
        let error = cut_drawing_section(&document, &east_along(100.0, 10.0), None)
            .expect_err("every surface hidden");
        assert_eq!(error.code, ErrorCode::InvalidState);
    }

    #[test]
    fn an_alignment_of_one_point_or_none_is_refused_as_invalid_argument() {
        let mut document = Document::default();
        add_surface(&mut document, "ground", slab(100.0, 10.0, 20.0));
        let one = Polyline2::new(vec![Point2::new(10.0, 10.0)], false);
        let single = cut_drawing_section(&document, &one, None).expect_err("one point");
        assert_eq!(single.code, ErrorCode::InvalidArgument);
        let none =
            cut_drawing_section(&document, &Polyline2::default(), None).expect_err("no points");
        assert_eq!(none.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn the_interval_is_a_two_thousandth_of_the_length_and_no_finer_than_one_hundred_millimetres() {
        // max(length / 2000, 0.1), by hand: 100 -> 0.05 -> 0.1; 200 -> 0.1
        // exactly; 4,000 -> 2; a million metres -> 500; a degenerate length of
        // 0 -> the floor.
        double_eq(drawing_section_interval(100.0), 0.1);
        double_eq(drawing_section_interval(200.0), 0.1);
        double_eq(drawing_section_interval(4000.0), 2.0);
        double_eq(drawing_section_interval(1_000_000.0), 500.0);
        double_eq(drawing_section_interval(0.0), 0.1);
    }

    #[test]
    fn a_design_profile_is_cut_beside_the_ground_under_the_name_it_was_given() {
        let mut document = Document::default();
        add_surface(&mut document, "ground", slab(300.0, 10.0, 10.0));
        let design = SectionDesign {
            profile: sag(),
            name: String::from("design Main"),
        };
        let cut = match cut_drawing_section(&document, &east_along(300.0, 10.0), Some(&design)) {
            Ok(cut) => cut,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(cut.section.surfaces.len(), 2);
        assert_eq!(cut.section.surfaces[0].name, "ground");
        let line = &cut.section.surfaces[1];
        assert_eq!(line.name, "design Main");
        // The profile's own figures, worked by hand in the sag's comment: 16
        // at the start, 17 at the end, the low point 13.6, and
        // 16 - 0.03 * 50 = 14.5 at the PVC (50).
        near(
            line.samples[0].elevation.expect("an elevation"),
            16.0,
            1e-12,
        );
        near(
            line.samples[line.samples.len() - 1]
                .elevation
                .expect("an elevation"),
            17.0,
            1e-12,
        );
        near(line.min_elevation.expect("a lowest point"), 13.6, 1e-12);
        let pvc = line
            .samples
            .iter()
            .find(|sample| sample.station == 50.0)
            .expect("a sample at the PVC");
        near(pvc.elevation.expect("an elevation"), 14.5, 1e-12);
    }

    #[test]
    fn a_design_with_no_name_is_refused_and_nothing_is_cut() {
        let mut document = Document::default();
        add_surface(&mut document, "ground", slab(300.0, 10.0, 10.0));
        let nameless = SectionDesign {
            profile: sag(),
            name: String::new(),
        };
        let error = cut_drawing_section(&document, &east_along(300.0, 10.0), Some(&nameless))
            .expect_err("a design with no name");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn an_entity_crossing_the_alignment_is_a_crossing_at_its_station() {
        // A line x = 30 from y 0 to y 20 crosses the alignment y = 10 at
        // station 30: the model is read, so what is drawn across the section
        // shows on it.
        let mut document = Document::default();
        add_surface(&mut document, "ground", slab(100.0, 10.0, 20.0));
        ok(document.execute(create_line(
            Point2::new(30.0, 0.0),
            Point2::new(30.0, 20.0),
            EntityAttributes::default(),
        )));
        let cut = cut_drawing_section(&document, &east_along(100.0, 10.0), None).expect("a cut");
        assert_eq!(cut.section.crossings.len(), 1);
        near(cut.section.crossings[0].station, 30.0, 1e-9);
    }

    #[test]
    fn the_same_cut_after_a_surface_is_raised_follows_it_and_after_the_undo_goes_back() {
        // What the re-cut of an open section is: the same alignment cut again
        // after the drawing's surfaces changed gives the surfaces as they are
        // now, and undo puts the first picture back. Ground raised by five:
        // 15 + s / 10 at station s; undone: 10 + s / 10.
        let last_elevation = |document: &Document| {
            let cut = cut_drawing_section(document, &east_along(100.0, 10.0), None).expect("a cut");
            let samples = &cut.section.surfaces[0].samples;
            samples[samples.len() - 1].elevation.expect("an elevation")
        };
        let mut document = Document::default();
        add_surface(&mut document, "ground", slab(100.0, 10.0, 20.0));
        near(last_elevation(&document), 20.0, 1e-9);

        let mut store = document.surfaces().clone();
        ok(store.replace("ground", Some(slab(100.0, 15.0, 25.0))));
        ok(document.set_surfaces(store, "RAISE"));
        near(last_elevation(&document), 25.0, 1e-9);

        ok(document.undo());
        near(last_elevation(&document), 20.0, 1e-9);
    }
}
