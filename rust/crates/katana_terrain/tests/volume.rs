//! Twin of tests/terrain/test_volume.cpp: volume_to_datum on shapes whose
//! integral is known in closed form, and compare_surfaces, its spatial cull and
//! its parallel blocks.

pub mod support;

use katana_core::task_pool::TaskPool;
use katana_core::{ErrorCode, Result};
use katana_geometry::{Point2, Point3};
use katana_terrain::volume::{SurfaceComparison, compare_surfaces, volume_to_datum};
use katana_terrain::{TinInput, TinSurface};
use support::*;

fn must<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// The four corners of the square [0, size]^2, counter-clockwise from the
/// origin.
fn corners_of(size: f64) -> [Point2; 4] {
    [p2(0.0, 0.0), p2(size, 0.0), p2(size, size), p2(0.0, size)]
}

// ---- volume_to_datum on shapes whose integral is known in closed form ---------------------

mod volume_to_datum {
    use super::*;

    #[test]
    fn tilted_plane_splits_symmetrically_about_its_mid_height() {
        // z = x over [0, 100]^2 against the datum 50. The integrand x - 50 is
        // odd about x = 50, so
        //   above = below = integral over x in (50, 100] of (x - 50) dx dy
        //                 = 100 * 50^2/2 = 125000 m^3,
        // and each half of the square contributes 5000 m^2 of plan area.
        let mut random = Random::default();
        let surface = build_from_points(plane_points(
            &Plane::new(1.0, 0.0, 0.0),
            100.0,
            800,
            &mut random,
        ));
        assert!(!surface.empty());

        let volume = must(volume_to_datum(&surface, 50.0));
        // ~1600 triangles, compensated sums, magnitudes <= 1.25e5: the relative
        // rounding of 1e-16 per term leaves absolute errors around 1e-10.
        near(volume.above, 125000.0, 1e-6);
        near(volume.below, 125000.0, 1e-6);
        near(volume.net, 0.0, 1e-6);
        near(volume.plan_area_above, 5000.0, 1e-8);
        near(volume.plan_area_below, 5000.0, 1e-8);
        near(
            volume.plan_area_above + volume.plan_area_below,
            surface.plan_area(),
            1e-8,
        );
    }

    #[test]
    fn pyramid_matches_one_third_base_times_height() {
        // V = (1/3) * base area * height = (1/3) * 100^2 * 30 = 100000 m^3, and
        // the whole base sits on the datum, so nothing is below it.
        let mut random = Random::default();
        let surface = build(&pyramid_input(100.0, 30.0, 400, &mut random)).surface;
        assert!(!surface.empty());

        let volume = must(volume_to_datum(&surface, 0.0));
        near(volume.above, 100000.0, 1e-6);
        assert_eq!(volume.below, 0.0);
        near(volume.net, 100000.0, 1e-6);
        near(volume.plan_area_above, 10000.0, 1e-8);
        assert_eq!(volume.plan_area_below, 0.0);
    }

    #[test]
    fn datum_at_the_mean_height_of_a_pyramid_balances_cut_and_fill() {
        // Same pyramid. Its mean height is V/A = 100000/10000 = 10 m, so the
        // datum 10 gives net == 0 by construction. Above it stands the similar
        // sub-pyramid of height 30 - 10 = 20 whose base has side
        // 100 (1 - 10/30) = 200/3, so
        //   above = below = (1/3) (200/3)^2 * 20 = 800000/27 m^3,
        //   planAreaAbove = (200/3)^2 = 40000/9 m^2, planAreaBelow = 10000 - 40000/9.
        let mut random = Random::default();
        let surface = build(&pyramid_input(100.0, 30.0, 400, &mut random)).surface;
        assert!(!surface.empty());

        let volume = must(volume_to_datum(&surface, 10.0));
        near(volume.above, 800000.0 / 27.0, 1e-6);
        near(volume.below, 800000.0 / 27.0, 1e-6);
        near(volume.net, 0.0, 1e-6);
        near(volume.plan_area_above, 40000.0 / 9.0, 1e-8);
        near(volume.plan_area_below, 10000.0 - 40000.0 / 9.0, 1e-8);
    }

    #[test]
    fn shifting_the_datum_shifts_the_net_volume_by_area_times_shift() {
        // net(d) = integral of (z - d) = net(0) - d * planArea for every d,
        // exactly, because the plan area does not depend on the datum. A prism
        // identity, so it holds whatever the ground looks like.
        let mut random = Random::default();
        let mut input = TinInput::default();
        for _ in 0..700 {
            let x = random.real(0.0, 300.0);
            let y = random.real(0.0, 300.0);
            let z = random.real(40.0, 90.0);
            input.points.push(p3(x, y, z));
        }
        let surface = build(&input).surface;
        assert!(!surface.empty());

        let base = must(volume_to_datum(&surface, 0.0));
        for datum in [-25.0, 37.5, 65.0, 120.0] {
            let shifted = must(volume_to_datum(&surface, datum));
            let expected = base.net - datum * surface.plan_area();
            // Magnitudes up to 1e7 with ~1400 compensated terms.
            assert!(
                (shifted.net - expected).abs() <= 1e-6,
                "datum {datum}: {} against {expected}",
                shifted.net
            );
            assert!(shifted.above >= 0.0);
            assert!(shifted.below >= 0.0);
            near(shifted.above - shifted.below, shifted.net, 1e-9);
            near(
                shifted.plan_area_above + shifted.plan_area_below,
                surface.plan_area(),
                1e-8,
            );
        }
    }

    #[test]
    fn rejects_empty_surface_and_non_finite_datum() {
        let mut random = Random::default();
        let surface = build_from_points(plane_points(
            &Plane::new(0.1, 0.2, 5.0),
            10.0,
            10,
            &mut random,
        ));
        assert!(!surface.empty());

        let code =
            |result: Result<katana_terrain::volume::DatumVolume>| result.expect_err("refused").code;
        assert_eq!(
            code(volume_to_datum(&surface, f64::NAN)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(volume_to_datum(&surface, f64::INFINITY)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(volume_to_datum(&TinSurface::new(), 0.0)),
            ErrorCode::InvalidArgument
        );
    }
}

// ---- compare_surfaces --------------------------------------------------------------------

/// Rolling ground: two sine terms, so a design laid over it is above in some
/// places and below in others and the mixed-sign path of SignedIntegral runs
/// in many blocks, not just the all-fill one.
fn rolling_points(seed: u64, interior: usize, lift: f64) -> Vec<Point3> {
    let mut random = Random::new(seed);
    let height =
        |x: f64, y: f64| 30.0 + lift + 1.5 * (x * 0.07).sin() * (y * 0.05).cos() + 0.01 * x;
    let mut points: Vec<Point3> = Vec::new();
    for corner in corners_of(200.0) {
        points.push(p3(corner.x, corner.y, height(corner.x, corner.y)));
    }
    for _ in 0..interior {
        let x = random.real(1.0, 199.0);
        let y = random.real(1.0, 199.0);
        points.push(p3(x, y, height(x, y)));
    }
    points
}

/// Bitwise, not ==: 0.0 == -0.0, and "the same result" here means the same
/// bits.
fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

fn expect_same_bits(a: &SurfaceComparison, b: &SurfaceComparison, workers: usize) {
    assert!(same_bits(a.cut, b.cut), "{workers}: {} vs {}", a.cut, b.cut);
    assert!(
        same_bits(a.fill, b.fill),
        "{workers}: {} vs {}",
        a.fill,
        b.fill
    );
    assert!(same_bits(a.net, b.net), "{workers}: {} vs {}", a.net, b.net);
    assert!(
        same_bits(a.plan_area, b.plan_area),
        "{workers}: {} vs {}",
        a.plan_area,
        b.plan_area
    );
    assert!(
        same_bits(a.cut_area, b.cut_area),
        "{workers}: {} vs {}",
        a.cut_area,
        b.cut_area
    );
    assert!(
        same_bits(a.fill_area, b.fill_area),
        "{workers}: {} vs {}",
        a.fill_area,
        b.fill_area
    );
    assert_eq!(
        a.overlay_triangle_count, b.overlay_triangle_count,
        "{workers}"
    );
}

mod compare_surfaces {
    use super::*;

    #[test]
    fn a_surface_against_itself_is_exactly_zero() {
        // Every overlay triangle is a triangle of both inputs, and the
        // interpolation weights at a triangle's own corners are exactly
        // (1,0,0): the difference is the zero double, not a small one.
        let mut random = Random::default();
        let surface = build_from_points(plane_points(
            &Plane::new(0.2, -0.15, 30.0),
            120.0,
            300,
            &mut random,
        ));
        assert!(!surface.empty());

        let result = must(compare_surfaces(&surface, &surface, None));
        assert_eq!(result.cut, 0.0);
        assert_eq!(result.fill, 0.0);
        assert_eq!(result.net, 0.0);
        assert_eq!(result.cut_area, 0.0);
        assert_eq!(result.fill_area, 0.0);
        near(result.plan_area, surface.plan_area(), 1e-8);
        // The square is its own convex hull, so the overlay of the two
        // identical triangulations is that triangulation again.
        assert_eq!(result.overlay_triangle_count, surface.triangle_count());
    }

    #[test]
    fn one_plane_triangulated_two_ways_has_no_cut_area_and_no_fill_area_in_either_order() {
        // The plane z = 10 + 0.1 x + 0.05 y over [0, 100] x [0, 50], 5000 m2.
        // The base is two triangles from its corners (10, 20, 22.5, 12.5); the
        // top is the same plane in ten, with a ring of four more points on it,
        // (30, 20) at 10 + 3 + 1 = 14, (70, 20) at 18, (70, 35) at
        // 10 + 7 + 1.75 = 18.75 and (30, 35) at 14.75. Where the two overlay is
        // not a vertex of either triangle, so the heights come from weights
        // that sum to 1 only to a few ulps, and the difference there is
        // +/- 1e-15, not 0. The surfaces coincide: nothing is cut or filled,
        // and no area is either (it was 750 m2 of cut and 9 of fill one way and
        // 100 of cut the other).
        let corners = vec![
            p3(0.0, 0.0, 10.0),
            p3(100.0, 0.0, 20.0),
            p3(100.0, 50.0, 22.5),
            p3(0.0, 50.0, 12.5),
        ];
        let mut with_ring = corners.clone();
        with_ring.extend([
            p3(30.0, 20.0, 14.0),
            p3(70.0, 20.0, 18.0),
            p3(70.0, 35.0, 18.75),
            p3(30.0, 35.0, 14.75),
        ]);
        let base = build_from_points(corners);
        let top = build_from_points(with_ring);
        assert_eq!(base.triangle_count(), 2);
        assert_eq!(top.triangle_count(), 10);
        for top_first in [false, true] {
            let result = must(if top_first {
                compare_surfaces(&top, &base, None)
            } else {
                compare_surfaces(&base, &top, None)
            });
            assert!((result.plan_area - 5000.0).abs() <= 1e-8, "{top_first}");
            assert!(result.cut.abs() <= 1e-9, "{top_first}");
            assert!(result.fill.abs() <= 1e-9, "{top_first}");
            assert_eq!(result.cut_area, 0.0, "{top_first}");
            assert_eq!(result.fill_area, 0.0, "{top_first}");
        }
    }

    #[test]
    fn two_independent_triangulations_of_one_plane_have_no_cut_area_and_no_fill_area() {
        // Hundreds of overlay pieces, almost none with a corner at a vertex of
        // both: the same plane, so every difference is the rounding of the two
        // readings.
        let ground = Plane::new(0.1, -0.05, 10.0);
        let mut first_random = Random::new(41);
        let mut second_random = Random::new(42);
        let first = build_from_points(plane_points(&ground, 100.0, 300, &mut first_random));
        let second = build_from_points(plane_points(&ground, 100.0, 350, &mut second_random));
        for second_first in [false, true] {
            let result = must(if second_first {
                compare_surfaces(&second, &first, None)
            } else {
                compare_surfaces(&first, &second, None)
            });
            assert!(
                result.overlay_triangle_count > 600,
                "the premise: a real overlay"
            );
            assert!((result.plan_area - 10000.0).abs() <= 1e-7, "{second_first}");
            assert_eq!(result.cut_area, 0.0, "{second_first}");
            assert_eq!(result.fill_area, 0.0, "{second_first}");
            assert!(result.cut.abs() <= 1e-8, "{second_first}");
            assert!(result.fill.abs() <= 1e-8, "{second_first}");
        }
    }

    #[test]
    fn a_difference_above_the_geometric_tolerance_is_area_and_one_below_is_not_but_stays_in_the_volume()
     {
        // Over 100 x 100 = 10000 m2 the top is 1 mm above the base: all of it
        // fill area, 10 m3. At 5e-8 (under GEOMETRIC, 1e-7, a tenth of a
        // micrometre) the surfaces coincide for the areas, while the volume
        // stays what it is, 5e-8 x 10000 = 5e-4 m3: the volumes are summed
        // from the heights as they are, and only the sort of the plan area
        // ignores rounding.
        let mut random = Random::new(43);
        let base = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 10.0),
            100.0,
            50,
            &mut random,
        ));
        for rise in [1e-3, 5e-8] {
            let mut again = Random::new(44);
            let top = build_from_points(plane_points(
                &Plane::new(0.0, 0.0, 10.0 + rise),
                100.0,
                60,
                &mut again,
            ));
            let result = must(compare_surfaces(&base, &top, None));
            assert!((result.fill - rise * 10000.0).abs() <= 1e-9, "{rise}");
            assert_eq!(result.cut, 0.0, "{rise}");
            if rise > 1e-6 {
                assert!((result.fill_area - 10000.0).abs() <= 1e-7, "{rise}");
            } else {
                assert_eq!(result.fill_area, 0.0, "{rise}");
            }
            assert_eq!(result.cut_area, 0.0, "{rise}");
        }
    }

    #[test]
    fn constant_raise_is_all_fill() {
        // Two independently triangulated planes over the same square, the
        // design 2 m above the existing everywhere: fill = 2 * 100^2 =
        // 20000 m^3, no cut.
        let mut existing_random = Random::new(11);
        let mut design_random = Random::new(29);
        let ground = Plane::new(0.1, -0.05, 10.0);
        let existing = build_from_points(plane_points(&ground, 100.0, 300, &mut existing_random));
        let design = build_from_points(plane_points(
            &Plane::new(ground.a, ground.b, ground.c + 2.0),
            100.0,
            350,
            &mut design_random,
        ));
        assert!(!existing.empty());
        assert!(!design.empty());

        let result = must(compare_surfaces(&existing, &design, None));
        near(result.fill, 20000.0, 1e-6);
        near(result.cut, 0.0, 1e-9);
        near(result.net, 20000.0, 1e-6);
        near(result.plan_area, 10000.0, 1e-6);
        near(result.fill_area, 10000.0, 1e-6);
        near(result.cut_area, 0.0, 1e-9);
        assert!(result.overlay_triangle_count > existing.triangle_count());
    }

    #[test]
    fn tilted_design_gives_equal_cut_and_fill() {
        // existing: the level plane z = 0. design: z = x - 50 over [0, 100]^2.
        // zDesign - zExisting = x - 50 is odd about x = 50, so
        //   fill = cut = 100 * 50^2/2 = 125000 m^3 and each covers 5000 m^2.
        let mut existing_random = Random::new(3);
        let mut design_random = Random::new(101);
        let existing = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 0.0),
            100.0,
            300,
            &mut existing_random,
        ));
        let design = build_from_points(plane_points(
            &Plane::new(1.0, 0.0, -50.0),
            100.0,
            300,
            &mut design_random,
        ));

        let result = must(compare_surfaces(&existing, &design, None));
        near(result.cut, 125000.0, 1e-6);
        near(result.fill, 125000.0, 1e-6);
        near(result.net, 0.0, 1e-6);
        near(result.plan_area, 10000.0, 1e-6);
        near(result.cut_area, 5000.0, 1e-6);
        near(result.fill_area, 5000.0, 1e-6);
    }

    #[test]
    fn is_antisymmetric_in_its_two_arguments() {
        // Swapping the surfaces turns every cut into a fill: a definition check
        // that does not depend on the ground being analytic.
        let mut existing_random = Random::new(5);
        let mut design_random = Random::new(7);
        let mut existing_input = TinInput::default();
        let mut design_input = TinInput::default();
        for _ in 0..400 {
            let x = existing_random.real(0.0, 80.0);
            let y = existing_random.real(0.0, 80.0);
            let z = existing_random.real(20.0, 26.0);
            existing_input.points.push(p3(x, y, z));
            let x = design_random.real(0.0, 80.0);
            let y = design_random.real(0.0, 80.0);
            let z = design_random.real(21.0, 27.0);
            design_input.points.push(p3(x, y, z));
        }
        // Shared corners so that both surfaces cover the same square exactly.
        for corner in corners_of(80.0) {
            existing_input.points.push(p3(corner.x, corner.y, 23.0));
            design_input.points.push(p3(corner.x, corner.y, 24.0));
        }
        let existing = build(&existing_input).surface;
        let design = build(&design_input).surface;

        let forward = must(compare_surfaces(&existing, &design, None));
        let reverse = must(compare_surfaces(&design, &existing, None));
        assert!(forward.cut > 0.0); // the ground really does cross over
        assert!(forward.fill > 0.0);
        // Magnitudes of a few thousand m^3 over the same overlay, so only
        // rounding differs.
        near(forward.cut, reverse.fill, 1e-9);
        near(forward.fill, reverse.cut, 1e-9);
        near(forward.net, -reverse.net, 1e-9);
        near(forward.cut_area, reverse.fill_area, 1e-9);
        near(forward.fill_area, reverse.cut_area, 1e-9);
        near(forward.plan_area, reverse.plan_area, 1e-9);
        near(forward.plan_area, 6400.0, 1e-6);
        // cut and fill partition the compared area up to where the surfaces
        // coincide, which here is a set of measure zero.
        near(
            forward.cut_area + forward.fill_area,
            forward.plan_area,
            1e-6,
        );
    }

    #[test]
    fn only_the_common_area_is_compared() {
        // existing covers [0, 100]^2, design covers [50, 150] x [0, 100]: the
        // common ground is the 50 x 100 = 5000 m^2 strip, raised by 1 m, so
        // fill = 5000 m^3.
        let mut existing_random = Random::new(13);
        let mut design_random = Random::new(17);
        let existing = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 0.0),
            100.0,
            300,
            &mut existing_random,
        ));
        let design = build_from_points(plane_points_at(
            &Plane::new(0.0, 0.0, 1.0),
            100.0,
            300,
            &mut design_random,
            50.0,
            0.0,
        ));

        let result = must(compare_surfaces(&existing, &design, None));
        near(result.plan_area, 5000.0, 1e-6);
        near(result.fill, 5000.0, 1e-6);
        near(result.cut, 0.0, 1e-9);
        near(result.fill_area, 5000.0, 1e-6);
    }

    #[test]
    fn disjoint_surfaces_compare_nothing() {
        let mut first = Random::new(19);
        let mut second = Random::new(23);
        let existing = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 0.0),
            10.0,
            20,
            &mut first,
        ));
        let design = build_from_points(plane_points_at(
            &Plane::new(0.0, 0.0, 5.0),
            10.0,
            20,
            &mut second,
            1000.0,
            1000.0,
        ));

        let result = must(compare_surfaces(&existing, &design, None));
        assert_eq!(result.plan_area, 0.0);
        assert_eq!(result.cut, 0.0);
        assert_eq!(result.fill, 0.0);
        assert_eq!(result.overlay_triangle_count, 0);
    }

    #[test]
    fn agrees_with_volume_to_datum_on_a_level_design() {
        // A level design surface at z = d is the datum d, so comparing against
        // it must reproduce volume_to_datum: cut = above, fill = below. Two
        // independent code paths (closed-form per triangle vs. exact overlay)
        // for the same integral.
        let mut random = Random::default();
        let mut input = TinInput::default();
        for _ in 0..500 {
            let x = random.real(0.5, 199.5);
            let y = random.real(0.5, 199.5);
            let z = random.real(10.0, 40.0);
            input.points.push(p3(x, y, z));
        }
        for corner in corners_of(200.0) {
            input.points.push(p3(corner.x, corner.y, 25.0));
        }
        let existing = build(&input).surface;
        assert!(!existing.empty());

        let datum = 25.0;
        let mut level_input = TinInput::default();
        for corner in corners_of(200.0) {
            level_input.points.push(p3(corner.x, corner.y, datum));
        }
        let design = build(&level_input).surface;

        let to_datum = must(volume_to_datum(&existing, datum));
        let compared = must(compare_surfaces(&existing, &design, None));
        // Volumes of order 1e5 reached through different splittings of the
        // same region; the overlay adds one barycentric interpolation per
        // corner.
        near(compared.cut, to_datum.above, 1e-6);
        near(compared.fill, to_datum.below, 1e-6);
        near(compared.cut_area, to_datum.plan_area_above, 1e-6);
        near(compared.fill_area, to_datum.plan_area_below, 1e-6);
        near(compared.plan_area, existing.plan_area(), 1e-6);
    }

    #[test]
    fn rejects_empty_surfaces() {
        let mut random = Random::default();
        let surface = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 1.0),
            10.0,
            8,
            &mut random,
        ));
        assert!(!surface.empty());
        let empty = TinSurface::new();
        let code = |result: Result<SurfaceComparison>| result.expect_err("refused").code;
        assert_eq!(
            code(compare_surfaces(&empty, &surface, None)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(compare_surfaces(&surface, &empty, None)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(compare_surfaces(&empty, &empty, None)),
            ErrorCode::InvalidArgument
        );
    }

    // ---- the spatial cull must lose nothing (CLAUDE.md section 4) --------------------------

    #[test]
    fn every_piece_of_the_common_ground_is_found_by_the_indexed_search() {
        // compare_surfaces pairs triangles through a spatial index rather than
        // testing all pairs. The pieces it keeps must tile the common ground
        // exactly: any pair the index failed to return is missing area, and
        // area is the one quantity whose true value is known here in closed
        // form.
        //
        // Both surfaces carry the four corners of the same 100 m square, so
        // their convex hulls (and therefore the common ground) are exactly
        // that square, 10 000 m^2. The interiors are triangulated from
        // different random points, so no triangle of one matches a triangle of
        // the other and the overlay is a genuine subdivision rather than a
        // one-to-one correspondence.
        let mut existing_random = Random::new(101);
        let mut design_random = Random::new(202);
        let existing = build_from_points(plane_points(
            &Plane::new(0.02, -0.013, 5.0),
            100.0,
            300,
            &mut existing_random,
        ));
        let design = build_from_points(plane_points(
            &Plane::new(-0.011, 0.017, 6.0),
            100.0,
            200,
            &mut design_random,
        ));
        assert!(!existing.empty());
        assert!(!design.empty());

        let result = must(compare_surfaces(&existing, &design, None));

        // 1e-6 m^2 on 10 000 m^2 is 1e-10 relative: far tighter than a single
        // dropped triangle, the smallest of which spans several square metres
        // here.
        near(result.plan_area, 10000.0, 1e-6);
        near(result.cut_area + result.fill_area, 10000.0, 1e-6);

        // The subdivision is real: more pieces than either surface has
        // triangles, so the test exercises many-to-many clipping and not a
        // trivial match. A generator that could not reach that case would read
        // as coverage while proving nothing.
        assert!(
            result.overlay_triangle_count > existing.triangle_count() + design.triangle_count()
        );

        // Shown to fail, not assumed to (in the C++): dropping one candidate
        // pair in every seven from the index result moves planArea by 779 m^2
        // of 10 000 and fails this test (and five of the ones above it). Its
        // sensitivity does have a floor: shrinking the query box by 5 cm loses
        // only sub-millimetre slivers along triangle boundaries and is NOT
        // caught here. What this test protects is the loss of a pair that
        // overlaps in real area, which is the way a broad phase actually goes
        // wrong.
    }

    #[test]
    fn the_answer_does_not_depend_on_how_the_grid_buckets_triangles() {
        // The same two surfaces, moved far from the origin. The hash grid
        // assigns every triangle to a different cell, and triangles that shared
        // a cell before no longer do, so a cull that depended on bucketing
        // would move the answer.
        //
        // `plane_points` lifts points onto a plane defined in ABSOLUTE
        // coordinates, so shifting the site would also change every elevation
        // and the two surfaces would no longer cross. The intercept is
        // corrected for that: a plane with c' = c - a*x0 - b*y0 gives, at
        // (x0 + u, y0 + v), exactly the elevation the original gives at
        // (u, v). The result is a rigid translation (the same shape, in the
        // same relative position), which is what makes this a test of the
        // index rather than of arithmetic.
        let shift = 12345.0;
        let shifted = |plane: &Plane, x0: f64, y0: f64| {
            Plane::new(plane.a, plane.b, plane.c - plane.a * x0 - plane.b * y0)
        };
        let build_at = |seed: u64, plane: &Plane, x0: f64, y0: f64| {
            let mut random = Random::new(seed);
            build_from_points(plane_points_at(
                &shifted(plane, x0, y0),
                100.0,
                250,
                &mut random,
                x0,
                y0,
            ))
        };
        let existing_plane = Plane::new(0.02, -0.013, 5.0);
        let design_plane = Plane::new(-0.011, 0.017, 6.0);

        let here = must(compare_surfaces(
            &build_at(31, &existing_plane, 0.0, 0.0),
            &build_at(37, &design_plane, 0.0, 0.0),
            None,
        ));
        let there = must(compare_surfaces(
            &build_at(31, &existing_plane, shift, shift),
            &build_at(37, &design_plane, shift, shift),
            None,
        ));

        // Exactly the same pieces have to be found, however they were
        // bucketed.
        assert_eq!(here.overlay_triangle_count, there.overlay_triangle_count);

        // The values agree only to floating point. Coordinates near 12 345
        // have an ulp of 1.8e-12 against 1.4e-14 near 100, about 128 times
        // coarser, and a plan area is built from differences of them, so each
        // triangle's area carries roughly 1e-12 relative error. Accumulated
        // over the ~2000 pieces with independent signs that is ~5e-7 absolute
        // on a volume of 1.1e4 m^3. 1e-4 leaves two decades over that
        // estimate, in case the accumulation is less favourable than a random
        // walk.
        near(here.plan_area, there.plan_area, 1e-4);
        near(here.cut, there.cut, 1e-4);
        near(here.fill, there.fill, 1e-4);

        // Not the trivial all-cut or all-fill case: the planes genuinely
        // cross, so the mixed-sign path of SignedIntegral is exercised on both
        // runs.
        assert!(here.cut > 1.0);
        assert!(here.fill > 1.0);
    }

    // ---- parallel comparison: the same bits at any thread count ------------------------------

    #[test]
    fn gives_the_same_bits_on_one_thread_as_on_many() {
        // 4000 interior points make about 8000 triangles: some thirty of the
        // fixed 256-triangle blocks, so the threads really do share the work
        // and a partition that followed the thread count would show here.
        let existing = build_from_points(rolling_points(11, 4000, 0.0));
        let design = build_from_points(rolling_points(13, 2500, 0.3));
        assert!(existing.triangle_count() > 256 * 20);

        let inline_only = TaskPool::new(0);
        let serial = must(compare_surfaces(&existing, &design, Some(&inline_only)));
        assert!(serial.cut > 1.0); // both signs present: the ground crosses
        assert!(serial.fill > 1.0);
        // The two surfaces share the 200 m square's corners, so the common
        // ground is that square: 40 000 m^2, to rounding.
        near(serial.plan_area, 40000.0, 1e-6);

        for workers in [1, 2, 3, 7, 15] {
            let pool = TaskPool::new(workers);
            let parallel = must(compare_surfaces(&existing, &design, Some(&pool)));
            expect_same_bits(&serial, &parallel, workers);
        }
        let shared = must(compare_surfaces(&existing, &design, None));
        expect_same_bits(&serial, &shared, 0);
    }

    #[test]
    fn a_surface_against_itself_is_exactly_zero_on_many_threads() {
        // The exact-zero promise of the clipper survives the split into blocks:
        // a block of zero terms sums to exactly zero, and so do thirty of them.
        let surface = build_from_points(rolling_points(17, 3000, 0.0));
        let pool = TaskPool::new(7);
        let result = must(compare_surfaces(&surface, &surface, Some(&pool)));
        assert_eq!(result.cut, 0.0);
        assert_eq!(result.fill, 0.0);
        assert_eq!(result.net, 0.0);
        near(result.plan_area, 40000.0, 1e-6);
    }
}
