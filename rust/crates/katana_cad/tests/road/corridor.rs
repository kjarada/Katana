// Corridor cross sections and earthwork quantities (tests/cad/test_corridor.cpp;
// include/katana/cad/corridor.hpp).
//
// Every area here was worked by hand from the trapezoids and triangles the
// template and the ground make, and confirmed independently by a fine
// numerical integration of ground minus design in Python - nothing shared
// with the trapezoid split in corridor.cpp. The ground is a plane, so the TIN
// reproduces it exactly and the only tolerance is the daylight bisection.

use crate::{must, near};
use katana_cad::corridor::{
    Assembly, CorridorSection, SectionVertex, area_between, corridor_quantities, corridor_section,
    corridor_surface,
};
use katana_core::ErrorCode;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentPi, HorizontalAlignment, Point2, Point3, ProfilePvi, SolvedAlignment, SolvedProfile,
    VerticalAlignment,
};
use katana_terrain::{TinBuildOptions, TinInput, TinSurface, build_tin, compare_surfaces};

// A planar ground z = a x + b y + c over [-100, 300] x [-y_half, y_half]. Four
// coplanar corners make two triangles that reproduce the plane exactly.
fn plane_ground(a: f64, b: f64, c: f64, y_half: f64) -> TinSurface {
    let mut input = TinInput::default();
    for (x, y) in [
        (-100.0, -y_half),
        (300.0, -y_half),
        (300.0, y_half),
        (-100.0, y_half),
    ] {
        input.points.push(Point3::new(x, y, a * x + b * y + c));
    }
    must(build_tin(&input, &TinBuildOptions::default())).surface
}

// 200 m straight along +x from the origin: station s is (s, 0), left is +y.
fn straight200() -> SolvedAlignment {
    let definition = HorizontalAlignment {
        pis: vec![
            AlignmentPi {
                point: Point2::new(0.0, 0.0),
                ..AlignmentPi::default()
            },
            AlignmentPi {
                point: Point2::new(200.0, 0.0),
                ..AlignmentPi::default()
            },
        ],
        ..HorizontalAlignment::default()
    };
    must(solve_alignment(&definition))
}

fn pvi(station: f64, elevation: f64, curve_length: f64) -> ProfilePvi {
    ProfilePvi {
        station,
        elevation,
        curve_length,
    }
}

// The C++ defaults `from` and `to` to 0 and 200.
fn level(elevation: f64, from: f64, to: f64) -> SolvedProfile {
    must(solve_profile(&VerticalAlignment {
        pvis: vec![pvi(from, elevation, 0.0), pvi(to, elevation, 0.0)],
    }))
}

fn assembly(half_width: f64, crossfall: f64, cut_batter: f64, fill_batter: f64) -> Assembly {
    Assembly {
        half_width,
        crossfall,
        cut_batter,
        fill_batter,
        ..Assembly::default()
    }
}

fn vertex(offset: f64, elevation: f64) -> SectionVertex {
    SectionVertex { offset, elevation }
}

mod corridor {
    use super::*;

    // ---- one section, against hand-worked areas -------------------------------------

    #[test]
    fn a_cut_section_on_flat_ground_is_the_textbook_trapezoid() {
        // Level design 1 m below flat ground, 8 m wide, 2:1 cut batters: each
        // batter runs 2 m out to daylight at +-6, and the cut is the trapezoid
        // (12 + 8) / 2 * 1 = 10 m^2.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let section = must(corridor_section(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            100.0,
        ));
        assert!(section.complete);
        near(section.cut_area, 10.0, 1e-9);
        near(section.fill_area, 0.0, 1e-9);
        assert_eq!(section.design.len(), 5);
        near(section.design[0].offset, 6.0, 1e-9); // left daylight
        near(section.design[0].elevation, 0.0, 1e-9);
        near(section.design[section.design.len() - 1].offset, -6.0, 1e-9); // right daylight
        near(section.centre.x, 100.0, 1e-12);
        near(section.design_elevation, -1.0, 1e-12);
    }

    #[test]
    fn a_fill_section_mirrors_the_cut() {
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let section = must(corridor_section(
            &straight200(),
            &level(1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            100.0,
        ));
        near(section.fill_area, 10.0, 1e-9);
        near(section.cut_area, 0.0, 1e-9);
        near(section.design[0].offset, 6.0, 1e-9);
    }

    #[test]
    fn crossfall_drops_the_edges_and_widens_the_cut() {
        // 5% crossfall over 4 m drops each edge 0.2 m to -1.2; the 2:1 batter
        // then needs 2.4 m to daylight at +-6.4. Batters 2 * (2.4 * 1.2 / 2) =
        // 2.88, carriageway 2 * (4 + 0.05 * 8) = 8.8, total 11.68 m^2.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let section = must(corridor_section(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.05, 2.0, 2.0),
            &ground,
            50.0,
        ));
        near(section.cut_area, 11.68, 1e-9);
        near(section.design[0].offset, 6.4, 1e-9);
        near(section.design[1].elevation, -1.2, 1e-12); // the left edge
    }

    #[test]
    fn sloping_ground_daylights_at_a_different_width_on_each_side() {
        // Ground rising 10% to the left (z = 0.1 y). Left edge is 1.4 m under
        // ground: a 2:1 batter rising 0.5/m against ground rising 0.1/m meets it
        // 3.5 m out, at offset 7.5, z = 0.75. Right edge is 0.6 m under: the
        // batter rises 0.5/m against ground FALLING 0.1/m, meeting 1 m out at
        // offset -5, z = -0.5. Area 0.3 + 8.0 + 2.45 = 10.75 m^2, all cut.
        let ground = plane_ground(0.0, 0.1, 0.0, 100.0);
        let section = must(corridor_section(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            100.0,
        ));
        assert!(section.complete);
        near(section.design[0].offset, 7.5, 1e-9);
        near(section.design[0].elevation, 0.75, 1e-9);
        let back = &section.design[section.design.len() - 1];
        near(back.offset, -5.0, 1e-9);
        near(back.elevation, -0.5, 1e-9);
        near(section.cut_area, 10.75, 1e-9);
        near(section.fill_area, 0.0, 1e-9);
    }

    // ---- quantities along the alignment ----------------------------------------------

    #[test]
    fn a_constant_section_along_a_straight_gives_area_times_length_exactly() {
        // The end-area method is exact when the area does not change: 10 m^2
        // over 200 m is 2000 m^3 of cut, whatever the interval.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        for interval in [200.0, 50.0, 7.0] {
            let quantities = must(corridor_quantities(
                &straight200(),
                &level(-1.0, 0.0, 200.0),
                &assembly(4.0, 0.0, 2.0, 2.0),
                &ground,
                interval,
            ));
            near(quantities.cut, 2000.0, 1e-6);
            near(quantities.fill, 0.0, 1e-9);
            // Cut means material to remove.
            near(quantities.net, -2000.0, 1e-6);
            assert_eq!(quantities.incomplete_sections, 0);
            assert_eq!(quantities.start_station, 0.0);
            assert_eq!(quantities.end_station, 200.0);
            assert!(quantities.sections.len() >= 2);
            assert!(
                quantities
                    .sections
                    .windows(2)
                    .all(|pair| pair[0].station <= pair[1].station),
                "interval {interval}: the sections are not in station order"
            );
        }
    }

    #[test]
    fn the_profile_key_stations_are_always_sectioned() {
        // A 40 m sag at 100: PVC 80 and PVT 120 are not multiples of the 50 m
        // interval and must be sectioned anyway, because the quantities change
        // character there and a report that stepped over them could not be
        // checked against the design.
        let profile = must(solve_profile(&VerticalAlignment {
            pvis: vec![
                pvi(0.0, -1.0, 0.0),
                pvi(100.0, -1.5, 40.0),
                pvi(200.0, -1.0, 0.0),
            ],
        }));
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let quantities = must(corridor_quantities(
            &straight200(),
            &profile,
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            50.0,
        ));
        let stations: Vec<f64> = quantities
            .sections
            .iter()
            .map(|section: &CorridorSection| section.station)
            .collect();
        for expected in [0.0, 50.0, 80.0, 100.0, 120.0, 150.0, 200.0] {
            assert!(stations.contains(&expected), "{expected}");
        }
        assert!(stations.windows(2).all(|pair| pair[0] != pair[1]));
        // Deeper in the sag, so more cut than the level case.
        assert!(quantities.cut > 2000.0);
    }

    #[test]
    fn a_section_that_cannot_reach_the_ground_is_incomplete_and_excluded_from_the_total() {
        // Ground only 3 m either side of the centreline: the batters need 6 m,
        // so every section runs off the surface. Nothing is invented for it -
        // the sections are marked, counted, and contribute no volume.
        let ground = plane_ground(0.0, 0.0, 0.0, 3.0);
        let quantities = must(corridor_quantities(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            50.0,
        ));
        assert_eq!(quantities.incomplete_sections, quantities.sections.len());
        assert!(quantities.incomplete_sections > 0);
        assert_eq!(quantities.cut, 0.0);
        assert_eq!(quantities.fill, 0.0);
        for section in &quantities.sections {
            assert!(!section.complete);
            assert_eq!(section.cut_area, 0.0);
        }
    }

    #[test]
    fn refuses_nonsense_and_names_it() {
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let alignment = straight200();
        let profile = level(-1.0, 0.0, 200.0);
        let ok = assembly(4.0, 0.0, 2.0, 2.0);
        assert_eq!(
            corridor_quantities(&alignment, &profile, &ok, &ground, 0.0)
                .expect_err("a zero interval")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            corridor_section(&alignment, &profile, &ok, &ground, 250.0)
                .expect_err("off the alignment")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            corridor_section(&alignment, &level(-1.0, 0.0, 50.0), &ok, &ground, 100.0)
                .expect_err("off the profile")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            corridor_section(
                &alignment,
                &profile,
                &assembly(0.0, 0.0, 2.0, 2.0),
                &ground,
                100.0
            )
            .expect_err("no width")
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            corridor_section(
                &alignment,
                &profile,
                &assembly(4.0, 0.0, 0.0, 2.0),
                &ground,
                100.0
            )
            .expect_err("no cut batter")
            .code,
            ErrorCode::InvalidArgument
        );
        // A profile entirely beyond the alignment: nothing overlaps.
        assert_eq!(
            corridor_quantities(&alignment, &level(-1.0, 300.0, 400.0), &ok, &ground, 10.0)
                .expect_err("no overlap")
                .code,
            ErrorCode::InvalidGeometry
        );
    }

    // ---- the area arithmetic on its own ----------------------------------------------

    #[test]
    fn area_between_splits_at_a_crossing_and_ignores_uncovered_intervals() {
        // Upper falls from +1 to -1 across a 2 m span over a flat lower line at
        // 0: half a square metre above, half below, split exactly at the
        // crossing.
        let upper = [vertex(0.0, 1.0), vertex(2.0, -1.0)];
        let lower = [vertex(0.0, 0.0), vertex(2.0, 0.0)];
        let split = area_between(&upper, &lower);
        near(split.above, 0.5, 1e-12);
        near(split.below, 0.5, 1e-12);

        // A constant 1 m gap over 10 m is 10 m^2, whichever way the vertices
        // run.
        let high = [vertex(10.0, 1.0), vertex(0.0, 1.0)]; // right to left
        let low = [vertex(0.0, 0.0), vertex(10.0, 0.0)];
        near(area_between(&high, &low).above, 10.0, 1e-12);
        near(area_between(&high, &low).below, 0.0, 1e-12);

        // Where only one line reaches, nothing is counted rather than guessed.
        let short_line = [vertex(0.0, 1.0), vertex(5.0, 1.0)];
        near(area_between(&short_line, &low).above, 5.0, 1e-12);
        assert_eq!(area_between(&[vertex(0.0, 1.0)], &low).above, 0.0);
    }

    // ---- the corridor as a surface ---------------------------------------------------

    #[test]
    fn the_corridor_surface_agrees_with_the_end_area_quantities() {
        // Two independent methods for the same volume, both exact on a straight
        // with a level design: end area over the sections, and the exact
        // overlay of the built corridor surface against the ground in
        // compare_surfaces. 2000 m^3 of cut over a 12 m by 200 m footprint, from
        // both.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let corridor = must(corridor_surface(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            10.0,
        ));
        assert_eq!(corridor.incomplete_sections, 0);
        assert_eq!(corridor.sections, 21); // 0, 10, ..., 200
        assert!(corridor.surface.triangle_count() > 0);

        let comparison = must(compare_surfaces(&ground, &corridor.surface, None));
        near(comparison.cut, 2000.0, 1e-6); // ground above design
        near(comparison.fill, 0.0, 1e-9);
        near(comparison.plan_area, 12.0 * 200.0, 1e-6);

        let quantities = must(corridor_quantities(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            10.0,
        ));
        // End area and the exact overlay must agree where both are exact.
        near(quantities.cut, comparison.cut, 1e-6);
    }

    #[test]
    fn the_corridor_surface_stays_within_its_daylight_lines() {
        // The daylight lines are the boundary, so the surface reaches exactly
        // +-6 m off the centreline and no further - nothing is hulled outside.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let corridor = must(corridor_surface(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            10.0,
        ));
        let bounds = corridor.surface.bounds();
        near(bounds.min.x, 0.0, 1e-9);
        near(bounds.max.x, 200.0, 1e-9);
        near(bounds.min.y, -6.0, 1e-9);
        near(bounds.max.y, 6.0, 1e-9);
        near(corridor.surface.min_elevation(), -1.0, 1e-12);
        near(corridor.surface.max_elevation(), 0.0, 1e-9);
    }

    #[test]
    fn no_surface_is_built_when_no_section_reaches_the_ground() {
        let ground = plane_ground(0.0, 0.0, 0.0, 3.0);
        let refused = corridor_surface(
            &straight200(),
            &level(-1.0, 0.0, 200.0),
            &assembly(4.0, 0.0, 2.0, 2.0),
            &ground,
            50.0,
        )
        .expect_err("no section reaches the ground");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
        assert!(refused.describe().contains("incomplete"));
    }
}
