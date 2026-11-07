//! Twin of tests/terrain/test_volume_methods.cpp: volumes inside a polygon, to
//! a level, of a stockpile and on a grid. Every expected value is a solid whose
//! volume is known in closed form (a prism, a pyramid, the volume between
//! parallel planes), worked in the comment beside it.

pub mod support;

use katana_core::task_pool::TaskPool;
use katana_core::{ErrorCode, Result};
use katana_geometry::{Point2, Polyline2};
use katana_math::numerics::{PI, TWO_PI};
use katana_terrain::overlay::OverlayRegion;
use katana_terrain::volume::{
    GridVolume, StockpileBase, StockpileOptions, compare_surfaces, compare_surfaces_inside,
    grid_volume, stockpile_volume, volume_to_datum, volume_to_datum_inside,
};
use katana_terrain::{Breakline, TinInput, TinSurface};
use support::*;

fn must<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn ring(vertices: Vec<Point2>) -> Polyline2 {
    Polyline2::new(vertices, true)
}

fn square(x0: f64, y0: f64, size: f64) -> Polyline2 {
    ring(vec![
        p2(x0, y0),
        p2(x0 + size, y0),
        p2(x0 + size, y0 + size),
        p2(x0, y0 + size),
    ])
}

fn region_of(polygon: &Polyline2, holes: &[Polyline2]) -> OverlayRegion {
    must(OverlayRegion::from_polygon(polygon, holes))
}

fn plane_surface(
    plane: Plane,
    size: f64,
    interior: usize,
    seed: u64,
    x0: f64,
    y0: f64,
) -> TinSurface {
    let mut random = Random::new(seed);
    build_from_points(plane_points_at(&plane, size, interior, &mut random, x0, y0))
}

fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

/// A pile of `facets` planar faces: its toe a regular polygon of `facets`
/// corners on a circle of `radius` round (0, 0), standing on the ground plane
/// `ground`, its apex `height` above the ground at the centre, inside a
/// 100 x 100 site of the same ground. The toe is a breakline, so no triangle
/// crosses it, and the regular polygon with its centre triangulates as the
/// fan from the apex: one face per side.
struct Pile {
    surface: TinSurface,
    toe: Polyline2,
}

fn pile(ground: Plane, facets: usize, radius: f64, height: f64) -> Pile {
    let mut input = TinInput::default();
    for corner in [
        p2(-50.0, -50.0),
        p2(50.0, -50.0),
        p2(50.0, 50.0),
        p2(-50.0, 50.0),
    ] {
        input
            .points
            .push(p3(corner.x, corner.y, ground.at(corner.x, corner.y)));
    }
    let mut toe = Breakline {
        vertices: Vec::new(),
        closed: true,
    };
    let mut outline = Polyline2::new(Vec::new(), true);
    for k in 0..facets {
        let angle = TWO_PI * k as f64 / facets as f64;
        let x = radius * angle.cos();
        let y = radius * angle.sin();
        toe.vertices.push(p3(x, y, ground.at(x, y)));
        outline.vertices.push(p2(x, y));
    }
    input.breaklines.push(toe);
    input
        .points
        .push(p3(0.0, 0.0, ground.at(0.0, 0.0) + height));
    Pile {
        surface: build(&input).surface,
        toe: outline,
    }
}

/// The plan area of a regular polygon of n corners on a circle of radius r:
/// n triangles of two radii at 2 pi / n, (n / 2) r^2 sin(2 pi / n).
fn regular_polygon_area(n: usize, r: f64) -> f64 {
    0.5 * n as f64 * r * r * (TWO_PI / n as f64).sin()
}

fn grid(
    existing: &TinSurface,
    design: &TinSurface,
    cell: f64,
    region: Option<&OverlayRegion>,
    claimed: &[OverlayRegion],
) -> Result<GridVolume> {
    grid_volume(existing, design, cell, region, None, claimed)
}

// ---- surface to surface inside a polygon --------------------------------------------------------

mod volume_inside_polygon {
    use super::*;

    #[test]
    fn two_parallel_planes_one_metre_apart_over_a_ten_by_ten_polygon_hold_one_hundred_cubic_metres()
    {
        // A 1 m slab over 10 x 10 m: 100 m^3 of fill, none of cut, whatever the
        // two triangulations and however the polygon crosses them.
        let base = plane_surface(Plane::new(0.05, 0.02, 10.0), 100.0, 200, 11, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.05, 0.02, 11.0), 100.0, 170, 12, 0.0, 0.0);
        let region = region_of(&square(33.3, 41.7, 10.0), &[]);
        let volume = must(compare_surfaces_inside(&base, &top, &region, None));
        // Heights to 17 read off planes: each depth within a few ulps of 1; 100
        // m^2 of pieces.
        near(volume.fill, 100.0, 1e-9);
        near(volume.cut, 0.0, 1e-12);
        near(volume.plan_area, 100.0, 1e-9);
    }

    #[test]
    fn the_edges_of_a_polygon_cut_triangles_where_they_lie_not_by_whole_triangles() {
        // Two triangles each, a 1 m slab, and a polygon whose edges cross both
        // diagonals: the slab under it is its area, 1/2 |(b - a) x (c - a)| =
        // 1/2 (67.6 x 67.6 - 14.7 x 26.7) = 2088.635.
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 100.0, 0, 1, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.0, 0.0, 1.0), 100.0, 0, 1, 0.0, 0.0);
        let region = region_of(
            &ring(vec![p2(13.7, 21.2), p2(81.3, 35.9), p2(40.4, 88.8)]),
            &[],
        );
        let volume = must(compare_surfaces_inside(&base, &top, &region, None));
        near(volume.fill, 2088.635, 1e-9);
    }

    #[test]
    fn a_hole_in_the_polygon_is_left_out() {
        // 100 x 100 less 20 x 20, under a 1 m slab: 9600.
        let base = plane_surface(Plane::new(0.01, 0.0, 3.0), 100.0, 60, 5, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.01, 0.0, 4.0), 100.0, 60, 6, 0.0, 0.0);
        let region = region_of(&square(0.0, 0.0, 100.0), &[square(40.0, 40.0, 20.0)]);
        let volume = must(compare_surfaces_inside(&base, &top, &region, None));
        near(volume.fill, 9600.0, 1e-8);
        near(volume.plan_area, 9600.0, 1e-8);
    }

    #[test]
    fn cut_and_fill_split_where_the_surfaces_cross_inside_the_polygon() {
        // Base z = 0, top z = x - 50, inside [25, 75]^2: the top is below the
        // base west of x = 50 and above it east, each half 50 (in y) x 25^2 / 2
        // = 15 625.
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 100.0, 40, 8, 0.0, 0.0);
        let top = plane_surface(Plane::new(1.0, 0.0, -50.0), 100.0, 40, 9, 0.0, 0.0);
        let volume = must(compare_surfaces_inside(
            &base,
            &top,
            &region_of(&square(25.0, 25.0, 50.0), &[]),
            None,
        ));
        near(volume.cut, 15625.0, 1e-8);
        near(volume.fill, 15625.0, 1e-8);
        near(volume.net, 0.0, 1e-8);
        near(volume.cut_area, 1250.0, 1e-9);
        near(volume.fill_area, 1250.0, 1e-9);
    }

    #[test]
    fn gives_the_same_bits_on_one_thread_as_on_many() {
        let mut random = Random::new(71);
        let mut rough = TinInput::default();
        let mut rougher = TinInput::default();
        for _ in 0..3000 {
            let x = random.real(0.0, 200.0);
            let y = random.real(0.0, 200.0);
            let z = random.real(10.0, 14.0);
            rough.points.push(p3(x, y, z));
            let x = random.real(0.0, 200.0);
            let y = random.real(0.0, 200.0);
            let z = random.real(10.0, 14.0);
            rougher.points.push(p3(x, y, z));
        }
        let base = build(&rough).surface;
        let top = build(&rougher).surface;
        let region = region_of(
            &ring(vec![
                p2(20.0, 20.0),
                p2(180.0, 30.0),
                p2(150.0, 170.0),
                p2(90.0, 120.0),
                p2(30.0, 160.0),
            ]),
            &[],
        );
        let serial = TaskPool::new(0);
        let one = must(compare_surfaces_inside(&base, &top, &region, Some(&serial)));
        for workers in [1, 3, 7] {
            let pool = TaskPool::new(workers);
            let many = must(compare_surfaces_inside(&base, &top, &region, Some(&pool)));
            assert!(same_bits(one.cut, many.cut), "{workers}");
            assert!(same_bits(one.fill, many.fill), "{workers}");
            assert!(same_bits(one.plan_area, many.plan_area), "{workers}");
        }
    }
}

// ---- surface to a level inside a polygon --------------------------------------------------------

mod volume_to_level_inside_polygon {
    use super::*;

    #[test]
    fn a_tilted_plane_splits_at_the_level_line_inside_a_square() {
        // z = x against the level 50, inside [25, 75]^2: above it east of
        // x = 50, 50 x 25^2 / 2 = 15 625, and as much below west of it.
        let surface = plane_surface(Plane::new(1.0, 0.0, 0.0), 100.0, 300, 13, 0.0, 0.0);
        let volume = must(volume_to_datum_inside(
            &surface,
            50.0,
            &region_of(&square(25.0, 25.0, 50.0), &[]),
            None,
        ));
        near(volume.above, 15625.0, 1e-8);
        near(volume.below, 15625.0, 1e-8);
        near(volume.plan_area, 2500.0, 1e-9);
    }

    #[test]
    fn a_prism_of_flat_ground_above_a_level_is_its_depth_times_its_area() {
        // Ground at 5 against the level 2 inside an L of 40 x 20 + 20 x 20 =
        // 1200 m^2: a prism 3 m deep, 3600 m^3, though the L is not convex.
        let surface = plane_surface(Plane::new(0.0, 0.0, 5.0), 100.0, 50, 14, 0.0, 0.0);
        let volume = must(volume_to_datum_inside(
            &surface,
            2.0,
            &region_of(
                &ring(vec![
                    p2(10.0, 10.0),
                    p2(50.0, 10.0),
                    p2(50.0, 30.0),
                    p2(30.0, 30.0),
                    p2(30.0, 50.0),
                    p2(10.0, 50.0),
                ]),
                &[],
            ),
            None,
        ));
        near(volume.above, 3600.0, 1e-9);
        near(volume.below, 0.0, 1e-12);
        near(volume.net, 3600.0, 1e-9);
    }

    #[test]
    fn a_polygon_partly_off_the_surface_measures_the_ground_under_it() {
        // [90, 110] x [0, 20] over ground that ends at x = 100: 10 x 20 =
        // 200 m^2 of it on the ground, 3 m above the level, 600 m^3. The plan
        // area says what was measured.
        let surface = plane_surface(Plane::new(0.0, 0.0, 5.0), 100.0, 20, 15, 0.0, 0.0);
        let volume = must(volume_to_datum_inside(
            &surface,
            2.0,
            &region_of(&square(90.0, 0.0, 20.0), &[]),
            None,
        ));
        near(volume.plan_area, 200.0, 1e-9);
        near(volume.above, 600.0, 1e-9);
    }

    #[test]
    fn a_whole_surface_region_agrees_with_the_volume_to_the_datum() {
        // A region that is the surface's own square is the plain
        // volume_to_datum.
        let mut random = Random::new(16);
        let mut input = TinInput::default();
        for corner in [p2(0.0, 0.0), p2(80.0, 0.0), p2(80.0, 80.0), p2(0.0, 80.0)] {
            input.points.push(p3(corner.x, corner.y, 20.0));
        }
        for _ in 0..500 {
            let x = random.real(1.0, 79.0);
            let y = random.real(1.0, 79.0);
            let z = random.real(15.0, 25.0);
            input.points.push(p3(x, y, z));
        }
        let surface = build(&input).surface;
        let whole = must(volume_to_datum(&surface, 19.0));
        let inside = must(volume_to_datum_inside(
            &surface,
            19.0,
            &region_of(&square(0.0, 0.0, 80.0), &[]),
            None,
        ));
        near(inside.above, whole.above, 1e-7);
        near(inside.below, whole.below, 1e-7);
        near(inside.plan_area, 6400.0, 1e-8);
    }

    #[test]
    fn a_flat_surface_against_its_own_level_has_neither_cut_area_nor_fill_area() {
        // z = 10 over [0, 10]^2 against the level 10 inside a skewed
        // quadrilateral: above = below = 0 whatever the polygon. The region is
        // compared with the surface as a flat surface of its own, read by
        // weights that sum to 1 only to a few ulps, so the difference at a
        // corner that is no vertex was +/- 1e-15: 44.6 m2 of the polygon were
        // booked as cut area and 23.3 as fill area, and none is either. The
        // polygon's area by the shoelace sum:
        //   (0.123 x 0.321 - 9.876 x 0.456) + (9.876 x 9.9 - 9.5 x 0.321)
        //   + (9.5 x 9.7 - 0.2 x 9.9) + (0.2 x 0.456 - 0.123 x 9.7)
        //   = -4.463973 + 94.7229 + 90.17 - 1.1019 = 179.327027, halved.
        let flat = build_from_points(vec![
            p3(0.0, 0.0, 10.0),
            p3(10.0, 0.0, 10.0),
            p3(10.0, 10.0, 10.0),
            p3(0.0, 10.0, 10.0),
        ]);
        let skewed = region_of(
            &ring(vec![
                p2(0.123, 0.456),
                p2(9.876, 0.321),
                p2(9.5, 9.9),
                p2(0.2, 9.7),
            ]),
            &[],
        );
        let whole = region_of(&square(0.0, 0.0, 10.0), &[]);
        let sliver = region_of(&ring(vec![p2(1.3, 1.1), p2(8.7, 1.2), p2(8.9, 1.4)]), &[]);
        for region in [&skewed, &whole, &sliver] {
            let volume = must(volume_to_datum_inside(&flat, 10.0, region, None));
            near(volume.plan_area, region.area(), 1e-9);
            near(volume.above, 0.0, 1e-9);
            near(volume.below, 0.0, 1e-9);
            assert_eq!(volume.plan_area_above, 0.0);
            assert_eq!(volume.plan_area_below, 0.0);
        }
        near(skewed.area(), 89.6635135, 1e-9);
        // Without a region the surface is read at its own vertices, exactly.
        let exact = must(volume_to_datum(&flat, 10.0));
        assert_eq!(exact.plan_area_above, 0.0);
        assert_eq!(exact.plan_area_below, 0.0);
    }

    #[test]
    fn refuses_a_non_finite_level_an_empty_surface_and_an_empty_region() {
        let surface = plane_surface(Plane::new(0.0, 0.0, 5.0), 10.0, 5, 17, 0.0, 0.0);
        let region = region_of(&square(0.0, 0.0, 5.0), &[]);
        let code =
            |result: Result<katana_terrain::volume::DatumVolume>| result.expect_err("refused").code;
        assert_eq!(
            code(volume_to_datum_inside(&surface, f64::NAN, &region, None)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(volume_to_datum_inside(
                &TinSurface::new(),
                0.0,
                &region,
                None
            )),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(volume_to_datum_inside(
                &surface,
                0.0,
                &OverlayRegion::default(),
                None
            )),
            ErrorCode::InvalidArgument
        );
    }
}

// ---- stockpiles -------------------------------------------------------------------------------

mod stockpile {
    use super::*;

    #[test]
    fn a_pile_of_facets_on_level_ground_is_a_third_of_its_base_times_its_height() {
        // Twelve faces on a toe of radius 20: a pyramid on a regular dodecagon,
        // base (12 / 2) 20^2 sin 30 deg = 1200 m^2, height 9: V = 1200 x 9 / 3
        // = 3600 m^3 above a base made of the toe's own heights.
        let heap = pile(Plane::new(0.0, 0.0, 0.0), 12, 20.0, 9.0);
        near(regular_polygon_area(12, 20.0), 1200.0, 1e-9);
        let volume = must(stockpile_volume(
            &heap.surface,
            &heap.toe,
            &StockpileOptions::default(),
            None,
        ));
        near(volume.volume.above, 3600.0, 1e-8);
        near(volume.volume.below, 0.0, 1e-9);
        near(volume.boundary_area, 1200.0, 1e-9);
        assert!(!volume.boundary_heights);
        assert!(volume.level.is_none());
    }

    #[test]
    fn many_facets_approach_the_cone_formula() {
        // 360 faces on a toe of radius 20, height 9: the pyramid's
        // (1/3) (360/2) 20^2 sin 1 deg x 9 = 3769.6, which the cone's
        // pi 20^2 x 9 / 3 = 3769.9 exceeds by the circle's area beyond the
        // polygon's, (1 - sin(2 pi / n) / (2 pi / n)) of it: 0.19 m^3.
        let heap = pile(Plane::new(0.0, 0.0, 0.0), 360, 20.0, 9.0);
        let volume = must(stockpile_volume(
            &heap.surface,
            &heap.toe,
            &StockpileOptions::default(),
            None,
        ));
        let facets = regular_polygon_area(360, 20.0) * 9.0 / 3.0;
        let cone = PI * 400.0 * 9.0 / 3.0;
        near(volume.volume.above, facets, 1e-7);
        near(volume.volume.above, cone, 0.2);
        assert!(volume.volume.above < cone);
    }

    #[test]
    fn on_sloping_ground_the_toe_base_follows_the_slope() {
        // The same pile on the ground z = 0.1 x + 5: the toe's heights make a
        // base in the plane of the ground, and a pyramid's volume is a third
        // of its plan area times its height measured plumb from that plane, so
        // still 1200 x 9 / 3 = 3600.
        let heap = pile(Plane::new(0.1, 0.0, 5.0), 12, 20.0, 9.0);
        let volume = must(stockpile_volume(
            &heap.surface,
            &heap.toe,
            &StockpileOptions::default(),
            None,
        ));
        near(volume.volume.above, 3600.0, 1e-8);
        near(volume.volume.below, 0.0, 1e-9);
    }

    #[test]
    fn on_sloping_ground_with_a_polygon_wider_than_the_toe_only_the_toe_is_cut_area() {
        // The pile of twelve faces on a toe of radius 20 (1200 m2) on the
        // ground z = 0.1 x + 5, measured inside the square [-30, 30]^2 of
        // 3600 m2 with a base made of the boundary's heights read off the
        // ground: a plane. The pile stands above it over the toe, 3600 m3, and
        // everywhere else the ground is the base: cut area 1200, fill area 0,
        // and the other 2400 m2 neither (it was 700 m2 of cut round a toe of
        // 400 and 75 of fill).
        let heap = pile(Plane::new(0.1, 0.0, 5.0), 12, 20.0, 9.0);
        let wider = square(-30.0, -30.0, 60.0);
        let volume = must(stockpile_volume(
            &heap.surface,
            &wider,
            &StockpileOptions::default(),
            None,
        ));
        near(volume.volume.above, 3600.0, 1e-7);
        near(volume.volume.below, 0.0, 1e-9);
        near(volume.volume.plan_area, 3600.0, 1e-8);
        near(volume.volume.plan_area_above, 1200.0, 1e-8);
        assert_eq!(volume.volume.plan_area_below, 0.0);

        // Against a level just under the whole polygon's ground the ground is
        // all above it: the whole 3600 m2 is cut area, the pile's faces and
        // the plane beside them alike.
        let level = StockpileOptions {
            base: StockpileBase::Level,
            level: -5.0,
            ..StockpileOptions::default()
        };
        let beneath = must(stockpile_volume(&heap.surface, &wider, &level, None));
        near(beneath.volume.plan_area_above, 3600.0, 1e-8);
        assert_eq!(beneath.volume.plan_area_below, 0.0);
    }

    #[test]
    fn against_the_lowest_point_of_the_toe_the_wedge_of_ground_below_it_is_added() {
        // On z = 0.1 x + 5 the toe's lowest point is its vertex at x = -20,
        // height 3. Under the pile, above 3, is the ground's wedge: the toe's
        // area times the ground's mean height over it less 3 (the plane at the
        // toe's centroid (0, 0), 5), so 1200 x 2 = 2400 more: 6000.
        let heap = pile(Plane::new(0.1, 0.0, 5.0), 12, 20.0, 9.0);
        let options = StockpileOptions {
            base: StockpileBase::Lowest,
            ..StockpileOptions::default()
        };
        let volume = must(stockpile_volume(&heap.surface, &heap.toe, &options, None));
        let level = volume.level.expect("a lowest point");
        near(level, 3.0, 1e-12);
        near(volume.volume.above, 6000.0, 1e-8);
        near(volume.volume.below, 0.0, 1e-9);
    }

    #[test]
    fn against_a_given_level_the_ground_above_it_is_added() {
        // Level 2, below the whole toe: 3600 + 1200 x (5 - 2) = 7200.
        let heap = pile(Plane::new(0.1, 0.0, 5.0), 12, 20.0, 9.0);
        let options = StockpileOptions {
            base: StockpileBase::Level,
            level: 2.0,
            ..StockpileOptions::default()
        };
        let volume = must(stockpile_volume(&heap.surface, &heap.toe, &options, None));
        near(volume.volume.above, 7200.0, 1e-8);
        assert_eq!(volume.level, Some(2.0));
    }

    #[test]
    fn a_pit_below_its_rim_is_measured_below_the_base() {
        // An inverted pile (a pit 9 deep) against its rim: 3600 below, none
        // above.
        let pit = pile(Plane::new(0.0, 0.0, 0.0), 12, 20.0, -9.0);
        let volume = must(stockpile_volume(
            &pit.surface,
            &pit.toe,
            &StockpileOptions::default(),
            None,
        ));
        near(volume.volume.below, 3600.0, 1e-8);
        near(volume.volume.above, 0.0, 1e-9);
    }

    #[test]
    fn a_toe_vertex_off_the_ground_with_no_height_of_its_own_is_refused_and_with_one_is_used() {
        // The square [40, 60]^2 reaches past the 100 x 100 site (it ends at
        // 50).
        let heap = pile(Plane::new(0.0, 0.0, 0.0), 12, 20.0, 9.0);
        let toe = square(40.0, 40.0, 20.0);
        let refused = stockpile_volume(&heap.surface, &toe, &StockpileOptions::default(), None)
            .expect_err("a vertex off the ground");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
        assert!(
            refused.message.contains("off the surface"),
            "{}",
            refused.message
        );

        // Given its own heights, at 0, it is the base: the ground inside it is
        // flat at 0, so nothing is above or below it.
        let own = StockpileOptions {
            boundary_heights: vec![Some(0.0), Some(0.0), Some(0.0), Some(0.0)],
            ..StockpileOptions::default()
        };
        let used = must(stockpile_volume(&heap.surface, &toe, &own, None));
        assert!(used.boundary_heights);
        near(used.volume.above, 0.0, 1e-9);
        near(used.volume.plan_area, 100.0, 1e-9); // the quarter on the site
    }

    #[test]
    fn an_off_surface_vertex_with_its_own_height_is_refused_naming_the_vertex_that_has_none() {
        // The boundary's own heights are used only when every vertex has one.
        // The square [40, 60]^2 has its first corner on the 100 x 100 site (it
        // ends at 50) and no height of its own, and its second, (60, 40), off
        // the site with the height 0: the base is built from the ground's
        // heights, so vertex 2 is refused, and not as having no height of its
        // own, which it has.
        let heap = pile(Plane::new(0.0, 0.0, 0.0), 12, 20.0, 9.0);
        let toe = square(40.0, 40.0, 20.0);
        let mut options = StockpileOptions {
            boundary_heights: vec![None, Some(0.0), Some(0.0), Some(0.0)],
            ..StockpileOptions::default()
        };
        let refused =
            stockpile_volume(&heap.surface, &toe, &options, None).expect_err("vertex 2 is refused");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
        assert!(
            refused
                .message
                .contains("boundary vertex 2 is off the surface, and its own height is not used"),
            "{}",
            refused.message
        );
        assert!(
            refused.message.contains("vertex 1 has none"),
            "{}",
            refused.message
        );
        assert!(
            !refused.message.contains("no height of its own"),
            "{}",
            refused.message
        );
        assert_eq!(refused.context, "vertex=2 missing=1");

        // A height that is not finite is none: here it is vertex 4's that is
        // missing.
        options.boundary_heights = vec![Some(0.0), Some(0.0), Some(0.0), Some(f64::NAN)];
        let nan =
            stockpile_volume(&heap.surface, &toe, &options, None).expect_err("vertex 2 is refused");
        assert!(
            nan.message
                .contains("boundary vertex 2 is off the surface, and its own height is not used"),
            "{}",
            nan.message
        );
        assert!(nan.message.contains("vertex 4 has none"), "{}", nan.message);
        options.boundary_heights = Vec::new();
        let none =
            stockpile_volume(&heap.surface, &toe, &options, None).expect_err("vertex 2 is refused");
        assert!(
            none.message.contains("no height of its own"),
            "{}",
            none.message
        );
        assert_eq!(none.context, "vertex=2");
    }

    #[test]
    fn a_toe_that_never_meets_the_ground_has_no_lowest_point() {
        let heap = pile(Plane::new(0.0, 0.0, 0.0), 12, 20.0, 9.0);
        let options = StockpileOptions {
            base: StockpileBase::Lowest,
            ..StockpileOptions::default()
        };
        let refused = stockpile_volume(&heap.surface, &square(200.0, 200.0, 10.0), &options, None)
            .expect_err("no lowest point");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
    }

    #[test]
    fn a_toe_that_is_no_polygon_is_refused() {
        let heap = pile(Plane::new(0.0, 0.0, 0.0), 12, 20.0, 9.0);
        let refused = stockpile_volume(
            &heap.surface,
            &ring(vec![
                p2(0.0, 0.0),
                p2(10.0, 10.0),
                p2(10.0, 0.0),
                p2(0.0, 10.0),
            ]),
            &StockpileOptions::default(),
            None,
        )
        .expect_err("a bow tie is no polygon");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
    }
}

// ---- the grid method --------------------------------------------------------------------------

mod grid_volume {
    use super::*;

    #[test]
    fn the_midpoint_rule_overstates_a_pyramid_by_a_tenth_of_a_cell_cubed_in_each_cell_on_a_ridge() {
        // The pyramid on [0, 100]^2 with its apex 30 high: exactly 100 000 m^3
        // between it and the plane z = 0. Its height is 30 - 0.6 max(u, v), u
        // and v the distances from the centre lines, so on any cell of a grid
        // whose lines include x = y = 50 it is linear (and the midpoint rule
        // exact) except on the cells the four ridges u = v run through, 50 / c
        // of them each. There max(u, v) averages c / 6 more than at the centre
        // (the mean of the larger of two uniform offsets in [-c/2, c/2]), so
        // the cell's prism stands 0.6 c / 6 = 0.1 c too high: 0.1 c^3 a cell,
        // and over 200 / c cells, 20 c^2 in all.
        let mut random = Random::default();
        let ground = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 0.0),
            100.0,
            0,
            &mut random,
        ));
        let pyramid = build(&pyramid_input(100.0, 30.0, 0, &mut random)).surface;
        let exact = must(compare_surfaces(&ground, &pyramid, None));
        near(exact.fill, 100000.0, 1e-7);
        for cell in [5.0_f64, 2.0, 1.0, 0.5] {
            let result = must(grid(&ground, &pyramid, cell, None, &[]));
            assert!(
                (result.fill - exact.fill - 20.0 * cell * cell).abs() <= 1e-6,
                "{cell}: {} against {}",
                result.fill - exact.fill,
                20.0 * cell * cell
            );
            assert_eq!(
                result.cells,
                (10000.0 / (cell * cell)).round() as usize,
                "{cell}"
            );
            assert!((result.plan_area - 10000.0).abs() <= 1e-9, "{cell}");
            assert_eq!(result.cut, 0.0, "{cell}");
        }
    }

    #[test]
    fn inside_a_polygon_only_the_cells_whose_centres_are_inside_count() {
        // A 1 m slab inside [10, 20]^2 on 1 m cells: the hundred cells whose
        // centres lie inside, 100 m^3; a polygon offset half a cell takes the
        // centres of 10 x 10 cells all the same.
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 100.0, 30, 21, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.0, 0.0, 1.0), 100.0, 30, 22, 0.0, 0.0);
        let region = region_of(&square(10.0, 10.0, 10.0), &[]);
        let result = must(grid(&base, &top, 1.0, Some(&region), &[]));
        assert_eq!(result.cells, 100);
        near(result.fill, 100.0, 1e-12);
        let shifted = region_of(&square(10.25, 10.25, 10.0), &[]);
        let offset = must(grid(&base, &top, 1.0, Some(&shifted), &[]));
        assert_eq!(offset.cells, 100);
    }

    #[test]
    fn lots_sharing_an_edge_on_a_line_of_centres_count_its_cells_once_not_twice() {
        // A 1 m slab over [0, 50]^2 on 1 m cells, whose centres are at k + 0.5.
        // Lot A is [5, 25.5] x [5, 45] and lot B [25.5, 45] x [5, 45]: they
        // share the edge x = 25.5, a line of centres. A holds the centres
        // x = 5.5 ... 25.5 (21 of them) by y = 5.5 ... 44.5 (40): 840 cells; B
        // holds x = 25.5 ... 44.5 (20) by 40: 800. Measured on their own and
        // summed that is 1640, the 40 centres on x = 25.5 twice. Their union
        // [5, 45]^2 holds x = 5.5 ... 44.5 (40) by 40 = 1600 cells, its area
        // exactly, and the exact overlay agrees.
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 50.0, 30, 31, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.0, 0.0, 1.0), 50.0, 30, 32, 0.0, 0.0);
        let lot_a = region_of(
            &ring(vec![
                p2(5.0, 5.0),
                p2(25.5, 5.0),
                p2(25.5, 45.0),
                p2(5.0, 45.0),
            ]),
            &[],
        );
        let lot_b = region_of(
            &ring(vec![
                p2(25.5, 5.0),
                p2(45.0, 5.0),
                p2(45.0, 45.0),
                p2(25.5, 45.0),
            ]),
            &[],
        );
        let both = region_of(&square(5.0, 5.0, 40.0), &[]);
        let alone = |region: &OverlayRegion| must(grid(&base, &top, 1.0, Some(region), &[])).cells;
        assert_eq!(alone(&lot_a), 840);
        assert_eq!(alone(&lot_b), 800);
        assert_eq!(alone(&both), 1600);

        // One after the other, the second leaving the cells the first holds:
        // the cells add up to the union's whichever lot goes first.
        let ab = must(grid(
            &base,
            &top,
            1.0,
            Some(&lot_b),
            std::slice::from_ref(&lot_a),
        ));
        let ba = must(grid(
            &base,
            &top,
            1.0,
            Some(&lot_a),
            std::slice::from_ref(&lot_b),
        ));
        assert_eq!(ab.cells, 800 - 40); // B without the 40 centres A holds on x = 25.5
        assert_eq!(ba.cells, 840 - 40); // A without the same 40
        assert_eq!(840 + ab.cells, 1600);
        assert_eq!(800 + ba.cells, 1600);
        near(ab.fill, 760.0, 1e-9);
        near(ab.plan_area, 760.0, 1e-9);
        assert_eq!(ab.cut, 0.0);
        let exact = must(compare_surfaces_inside(&base, &top, &both, None));
        near(exact.fill, 1600.0, 1e-9);
    }

    #[test]
    fn overlapping_polygons_count_each_cell_once_and_the_neighbours_of_the_case_are_unchanged() {
        // A = [5, 30] x [5, 45] holds x = 5.5 ... 29.5 (25) by 40 = 1000 cells,
        // B = [20, 45] x [5, 45] holds x = 20.5 ... 44.5 (25) by 40 = 1000; they
        // overlap on x = 20.5 ... 29.5 (10 columns, 400 cells). The union holds
        // x = 5.5 ... 44.5 = 1600, so B after A counts 1000 - 400 = 600.
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 50.0, 30, 33, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.0, 0.0, 1.0), 50.0, 30, 34, 0.0, 0.0);
        let a = region_of(
            &ring(vec![
                p2(5.0, 5.0),
                p2(30.0, 5.0),
                p2(30.0, 45.0),
                p2(5.0, 45.0),
            ]),
            &[],
        );
        let b = region_of(
            &ring(vec![
                p2(20.0, 5.0),
                p2(45.0, 5.0),
                p2(45.0, 45.0),
                p2(20.0, 45.0),
            ]),
            &[],
        );
        let second = must(grid(&base, &top, 1.0, Some(&b), std::slice::from_ref(&a)));
        assert_eq!(second.cells, 600);
        assert_eq!(
            must(grid(&base, &top, 1.0, Some(&a), &[])).cells + second.cells,
            1600
        );

        // The neighbours: nothing claimed is the grid as it was; a region
        // claimed by itself has nothing left (cells 0, not an error); a claim
        // that is elsewhere takes nothing from it.
        let none = must(grid(&base, &top, 1.0, Some(&a), &[]));
        assert_eq!(none.cells, 1000);
        let itself = must(grid(&base, &top, 1.0, Some(&a), std::slice::from_ref(&a)));
        assert_eq!(itself.cells, 0);
        assert_eq!(itself.fill, 0.0);
        let elsewhere = region_of(&square(100.0, 100.0, 5.0), &[]);
        let apart = must(grid(
            &base,
            &top,
            1.0,
            Some(&a),
            std::slice::from_ref(&elsewhere),
        ));
        assert_eq!(apart.cells, 1000);
    }

    #[test]
    fn refuses_a_cell_that_is_no_length_and_a_grid_too_large_naming_a_cell_that_fits() {
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 100.0, 5, 23, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.0, 0.0, 1.0), 100.0, 5, 24, 0.0, 0.0);
        let code = |result: Result<GridVolume>| result.expect_err("refused").code;
        assert_eq!(
            code(grid(&base, &top, 0.0, None, &[])),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(grid(&base, &top, -1.0, None, &[])),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(grid(&base, &top, f64::NAN, None, &[])),
            ErrorCode::InvalidArgument
        );
        // 100 m at 1 mm is 1e10 cells; 25 million fit at sqrt(1e4 / 2.5e7) =
        // 0.02 m.
        let huge = grid(&base, &top, 0.001, None, &[]).expect_err("too many cells");
        assert_eq!(huge.code, ErrorCode::InvalidArgument);
        assert!(huge.message.contains("0.02"), "{}", huge.message);
        assert_eq!(
            code(grid(&base, &TinSurface::new(), 1.0, None, &[])),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn gives_the_same_bits_on_one_thread_as_on_many() {
        let mut random = Random::new(81);
        let mut rough = TinInput::default();
        for _ in 0..2000 {
            let x = random.real(0.0, 100.0);
            let y = random.real(0.0, 100.0);
            let z = random.real(0.0, 3.0);
            rough.points.push(p3(x, y, z));
        }
        let base = build(&rough).surface;
        let top = plane_surface(Plane::new(0.0, 0.0, 1.5), 100.0, 10, 82, 0.0, 0.0);
        let serial = TaskPool::new(0);
        let one = must(grid_volume(&base, &top, 0.5, None, Some(&serial), &[]));
        for workers in [2, 5] {
            let pool = TaskPool::new(workers);
            let many = must(grid_volume(&base, &top, 0.5, None, Some(&pool), &[]));
            assert!(same_bits(one.cut, many.cut), "{workers}");
            assert!(same_bits(one.fill, many.fill), "{workers}");
            assert_eq!(one.cells, many.cells, "{workers}");
        }
    }
}
