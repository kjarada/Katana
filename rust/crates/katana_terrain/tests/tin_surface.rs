//! Twin of tests/terrain/test_tin_surface.cpp.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_core::ErrorCode;
use katana_core::task_pool::TaskPool;
use katana_geometry::{Containment, Point2, Point3};
use katana_math::numerics::{HALF_PI, PI, TWO_PI, angles_equal};
use katana_terrain::{
    DuplicatePointPolicy, NO_TRIANGLE, TinBuildOptions, TinInput, TinNeighbors, TinSurface,
    TinTriangle,
};
use support::*;

// ---- a plane must be reproduced exactly ------------------------------------------------

mod plane_surface {
    use super::*;

    const SIZE: f64 = 100.0;
    const PLANE: Plane = Plane::new(0.3, -0.2, 12.5);

    fn surface() -> TinSurface {
        let mut random = Random::default();
        let surface = build_from_points(plane_points(&PLANE, SIZE, 1500, &mut random));
        assert!(!surface.empty());
        surface
    }

    #[test]
    fn elevation_matches_the_plane_everywhere() {
        let surface = surface();
        let mut random = Random::new(7);
        for _ in 0..2000 {
            let p = p2(random.real(0.0, SIZE), random.real(0.0, SIZE));
            let z = surface.elevation_at(p).expect("on the surface");
            // Linear interpolation of a linear function is exact; what remains is the
            // rounding of the barycentric weights (eps * edge / height of thin
            // Delaunay triangles, times a relief of <= 50 m): orders below 1e-9.
            near(z, PLANE.at(p.x, p.y), 1e-9);
        }
    }

    #[test]
    fn slope_and_aspect_of_every_triangle() {
        let surface = surface();
        // |grad z| = sqrt(a^2 + b^2). Steepest descent runs along -grad = (-a, -b) =
        // (-0.3, +0.2): north-west. As an azimuth (clockwise from +y):
        // atan2(east, north) = atan2(-0.3, 0.2), wrapped into [0, 2 pi).
        let expected_slope = (0.3f64 * 0.3 + 0.2 * 0.2).sqrt();
        let expected_aspect = (-0.3f64).atan2(0.2) + TWO_PI;
        let mut checked = 0;
        for t in 0..surface.triangle_count() {
            let Some(slope_aspect) = surface.triangle_slope_aspect(t) else {
                continue;
            };
            checked += 1;
            near(slope_aspect.slope, expected_slope, 1e-9);
            near(slope_aspect.slope_angle, expected_slope.atan(), 1e-9);
            near(
                slope_aspect.aspect.expect("an aspect"),
                expected_aspect,
                1e-8,
            );
        }
        assert_eq!(
            checked,
            surface.triangle_count(),
            "no triangle of this data is a sliver"
        );

        let slope = surface.slope_at(p2(40.0, 60.0)).expect("a slope");
        let aspect = surface.aspect_at(p2(40.0, 60.0)).expect("an aspect");
        near(slope, expected_slope, 1e-9);
        near(aspect, expected_aspect, 1e-8);
        assert!(surface.slope_at(p2(-1.0, 50.0)).is_none());
        assert!(surface.aspect_at(p2(-1.0, 50.0)).is_none());
    }

    #[test]
    fn plan_and_surface_area() {
        let surface = surface();
        // A plane tilts every plan area element by the same factor sqrt(1 + a^2 + b^2).
        near(surface.plan_area(), SIZE * SIZE, 1e-8);
        near(
            surface.surface_area(),
            SIZE * SIZE * (1.0f64 + 0.09 + 0.04).sqrt(),
            1e-8,
        );
        near(surface.min_elevation(), PLANE.at(0.0, SIZE), 1e-12); // a > 0, b < 0
        near(surface.max_elevation(), PLANE.at(SIZE, 0.0), 1e-12);
        assert_eq!(surface.bounds().min, p2(0.0, 0.0));
        assert_eq!(surface.bounds().max, p2(SIZE, SIZE));
    }

    #[test]
    fn spot_elevations_are_batched_elevation_at() {
        let surface = surface();
        let positions = [
            p2(10.0, 10.0),
            p2(-5.0, 10.0),
            p2(99.5, 0.5),
            p2(50.0, 200.0),
        ];
        let spots = surface.elevations_at(&positions);
        assert_eq!(spots.len(), 4);
        near(spots[0].expect("on"), PLANE.at(10.0, 10.0), 1e-9);
        assert!(spots[1].is_none());
        near(spots[2].expect("on"), PLANE.at(99.5, 0.5), 1e-9);
        assert!(spots[3].is_none());
    }
}

// ---- aspect convention --------------------------------------------------------------------

mod tin_surface_aspect {
    use super::*;

    #[test]
    fn cardinal_directions() {
        let mut random = Random::default();
        // (plane, azimuth of steepest descent, clockwise from north)
        let cases = [
            (Plane::new(0.0, -1.0, 0.0), 0.0), // z falls as y grows: descends to the north
            (Plane::new(-1.0, 0.0, 0.0), HALF_PI), // descends to the east
            (Plane::new(0.0, 1.0, 0.0), PI),   // descends to the south
            (Plane::new(1.0, 0.0, 0.0), 3.0 * HALF_PI), // descends to the west
            (Plane::new(-1.0, -1.0, 0.0), PI / 4.0), // north-east
        ];
        for (plane, azimuth) in cases {
            let surface = build_from_points(plane_points(&plane, 10.0, 20, &mut random));
            let aspect = surface.aspect_at(p2(5.0, 5.0)).expect("an aspect");
            assert!(angles_equal(aspect, azimuth, 1e-9), "{aspect}");
            assert!(aspect >= 0.0);
            assert!(aspect < TWO_PI);
        }
    }

    #[test]
    fn flat_ground_has_no_aspect() {
        let mut random = Random::default();
        let surface = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 42.0),
            10.0,
            20,
            &mut random,
        ));
        let slope_aspect = surface.triangle_slope_aspect(0).expect("not degenerate");
        double_eq(slope_aspect.slope, 0.0);
        double_eq(slope_aspect.slope_angle, 0.0);
        assert!(slope_aspect.aspect.is_none());
        assert!(surface.slope_at(p2(5.0, 5.0)).is_some());
        assert!(surface.aspect_at(p2(5.0, 5.0)).is_none());
    }

    #[test]
    fn slope_angle_of_a45_degree_face() {
        let surface = build_from_points(vec![
            p3(0.0, 0.0, 0.0),
            p3(1.0, 0.0, 1.0),
            p3(1.0, 1.0, 1.0),
            p3(0.0, 1.0, 0.0),
        ]);
        let slope_aspect = surface.triangle_slope_aspect(0).expect("not degenerate");
        near(slope_aspect.slope, 1.0, 1e-15);
        near(slope_aspect.slope_angle, PI / 4.0, 1e-15);
        // Unit square tilted by 45 degrees about the y axis: area sqrt(2).
        near(surface.surface_area(), 2.0f64.sqrt(), 1e-15);
    }
}

// ---- location on edges, vertices and the rim ----------------------------------------------

mod tin_surface_locate {
    use super::*;

    #[test]
    fn edges_vertices_and_rim() {
        // Two triangles sharing the diagonal (0,0)-(10,10).
        let surface = build_from_points(vec![
            p3(0.0, 0.0, 0.0),
            p3(10.0, 0.0, 10.0),
            p3(10.0, 10.0, 20.0),
            p3(0.0, 10.0, 10.0),
        ]);
        assert_eq!(surface.triangle_count(), 2);
        // z = x + y on all four corners, hence on both triangles.
        near(surface.elevation_at_xy(5.0, 5.0).unwrap(), 10.0, 1e-12); // on the shared edge
        near(surface.elevation_at_xy(0.0, 0.0).unwrap(), 0.0, 1e-12); // vertex
        near(surface.elevation_at_xy(10.0, 10.0).unwrap(), 20.0, 1e-12); // vertex
        near(surface.elevation_at_xy(10.0, 4.0).unwrap(), 14.0, 1e-12); // rim
        near(surface.elevation_at_xy(3.0, 0.0).unwrap(), 3.0, 1e-12); // rim

        // Within GEOMETRIC (1e-7) outside the rim still counts as on the surface ...
        let just_outside = surface.locate(p2(10.0 + 5e-8, 4.0)).expect("on the rim");
        assert!(!just_outside.interior);
        near(
            surface.elevation_at_xy(10.0 + 5e-8, 4.0).unwrap(),
            14.0,
            1e-9,
        );
        assert!(surface.elevation_at_xy(-5e-8, -5e-8).is_some()); // beyond a corner
        // ... further out does not.
        assert!(surface.elevation_at_xy(10.0 + 1e-6, 4.0).is_none());
        assert!(surface.elevation_at_xy(-1e-6, 5.0).is_none());
        assert!(surface.elevation_at_xy(5.0, 10.0 + 1e-6).is_none());

        let inside = surface.locate(p2(7.0, 2.0)).expect("inside");
        assert!(inside.interior);
        near(
            inside.weights[0] + inside.weights[1] + inside.weights[2],
            1.0,
            1e-15,
        );

        assert!(surface.elevation_at_xy(f64::NAN, 1.0).is_none());
        assert!(TinSurface::new().elevation_at_xy(1.0, 1.0).is_none());
    }

    #[test]
    fn agrees_with_brute_force_on_a_rough_surface() {
        // The grid index must find exactly the triangle a linear scan would find.
        let mut random = Random::default();
        let mut points = Vec::new();
        for _ in 0..1500 {
            let x = random.real(0.0, 300.0);
            let y = random.real(0.0, 120.0);
            let z = random.real(0.0, 50.0);
            points.push(p3(x, y, z));
        }
        let surface = build_from_points(points);
        for _ in 0..1500 {
            let x = random.real(-10.0, 310.0);
            let y = random.real(-10.0, 130.0);
            let p = p2(x, y);
            let mut expected: Option<f64> = None;
            for t in 0..surface.triangle_count() {
                if expected.is_some() {
                    break;
                }
                let plan = surface.plan_triangle(t);
                if plan.classify(p) == Containment::Outside {
                    continue;
                }
                if let Some(w) = plan.barycentric(p) {
                    let tri = surface.triangles()[t];
                    let v = surface.vertices();
                    expected = Some(
                        w[0] * v[tri[0] as usize].z
                            + w[1] * v[tri[1] as usize].z
                            + w[2] * v[tri[2] as usize].z,
                    );
                }
            }
            let actual = surface.elevation_at(p);
            assert_eq!(actual.is_some(), expected.is_some(), "{p:?}");
            if let (Some(actual), Some(expected)) = (actual, expected) {
                near(actual, expected, 1e-9);
            }
        }
    }
}

// ---- TinSurface::create -------------------------------------------------------------------

mod tin_surface_create {
    use super::*;

    fn square() -> Vec<Point3> {
        vec![
            p3(0.0, 0.0, 0.0),
            p3(2.0, 0.0, 0.0),
            p3(2.0, 2.0, 0.0),
            p3(0.0, 2.0, 0.0),
        ]
    }

    #[test]
    fn derives_adjacency_and_statistics() {
        let surface = TinSurface::create(
            vec![
                p3(0.0, 0.0, 1.0),
                p3(2.0, 0.0, 1.0),
                p3(2.0, 2.0, 3.0),
                p3(0.0, 2.0, 3.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![0b100, 0b001],
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let first: TinNeighbors = [NO_TRIANGLE, NO_TRIANGLE, 1];
        let second: TinNeighbors = [0, NO_TRIANGLE, NO_TRIANGLE];
        assert_eq!(surface.neighbors()[0], first);
        assert_eq!(surface.neighbors()[1], second);
        assert!(surface.is_edge_constrained(0, 2));
        assert!(surface.is_edge_constrained(1, 0));
        assert!(!surface.is_edge_constrained(0, 0));
        double_eq(surface.plan_area(), 4.0);
        near(surface.surface_area(), 4.0 * 2.0f64.sqrt(), 1e-15); // z = 1 + y: 45 degrees
        near(surface.elevation_at_xy(1.0, 0.5).unwrap(), 1.5, 1e-15);
        let bad =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| surface.plan_triangle(2)));
        assert!(bad.is_err(), "a bad index is refused, as std::out_of_range");
    }

    #[test]
    fn rebuilds_the_builders_surface() {
        let mut random = Random::default();
        let input = pyramid_input(10.0, 5.0, 300, &mut random);
        let built = build(&input).surface;
        let recreated = TinSurface::create(
            built.vertices().clone(),
            built.triangles().clone(),
            built.constrained_edges().clone(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        // Adjacency derived from the index triples == adjacency reported by the kernel.
        assert!(recreated == built);
        assert_eq!(recreated.plan_area(), built.plan_area());
    }

    #[test]
    fn rejects_invalid_input() {
        let code = |vertices: Vec<Point3>, triangles: Vec<TinTriangle>, flags: Vec<u8>| {
            TinSurface::create(vertices, triangles, flags)
                .err()
                .map(|error| error.code)
        };
        assert_eq!(
            code(square(), vec![[0, 1, 7]], vec![]),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(square(), vec![[0, 1, 1]], vec![]),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(square(), vec![[0, 2, 1]], vec![]),
            Some(ErrorCode::InvalidGeometry)
        ); // clockwise
        assert_eq!(
            code(square(), vec![[0, 1, 2]], vec![0, 0]),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(square(), vec![[0, 1, 2], [0, 2, 3]], vec![0b100, 0b000]),
            Some(ErrorCode::InvalidArgument)
        ); // flags disagree across the diagonal
        // The same triangle twice: its edges run twice in the same direction.
        assert_eq!(
            code(square(), vec![[0, 1, 2], [0, 1, 2]], vec![]),
            Some(ErrorCode::InvalidGeometry)
        );
        let mut non_finite = square();
        non_finite[1].z = f64::INFINITY;
        assert_eq!(
            code(non_finite, vec![[0, 1, 2]], vec![]),
            Some(ErrorCode::InvalidArgument)
        );

        // No triangles is the valid empty surface.
        let empty = TinSurface::create(square(), vec![], vec![]).expect("the empty surface");
        assert!(empty.empty());
        double_eq(empty.plan_area(), 0.0);
    }
}

// ---- large projected coordinates -------------------------------------------------------------

mod tin_surface_large_coordinates {
    use super::*;

    const EAST: f64 = 500000.0;
    const NORTH: f64 = 5000000.0;

    // The same surface at the origin and shifted to UTM magnitudes (E 500 000,
    // N 5 000 000). Coordinates are multiples of 1/8 m, so the shift is exact in
    // binary64 (5 000 000 + k/8 needs 23 + 3 bits) and both inputs describe
    // precisely the same geometry. Every computation in the module works on
    // coordinate differences, which are then identical, so the results must agree
    // to rounding level - 1e-10 relative is already generous.
    #[test]
    fn exact_shift_gives_the_same_surface() {
        let mut random = Random::default();
        let mut local = Vec::new();
        let mut shifted = Vec::new();
        for _ in 0..2000 {
            let x = f64::from(random.integer(0, 8000)) / 8.0;
            let y = f64::from(random.integer(0, 8000)) / 8.0;
            let z = 100.0 + 10.0 * (0.01 * x).sin() * (0.013 * y).cos();
            local.push(p3(x, y, z));
            shifted.push(p3(x + EAST, y + NORTH, z));
        }
        let options = TinBuildOptions {
            duplicate_points: DuplicatePointPolicy::KeepFirst, // the integer grid may repeat
            ..TinBuildOptions::default()
        };
        let local_input = TinInput {
            points: local,
            ..TinInput::default()
        };
        let shifted_input = TinInput {
            points: shifted,
            ..TinInput::default()
        };
        let a = build_with(&local_input, &options).surface;
        let b = build_with(&shifted_input, &options).surface;

        assert_eq!(a.triangle_count(), b.triangle_count());
        assert_eq!(
            a.triangles(),
            b.triangles(),
            "exact predicates: identical topology"
        );
        near(a.plan_area(), b.plan_area(), 1e-10 * a.plan_area());
        near(a.surface_area(), b.surface_area(), 1e-10 * a.surface_area());
        for _ in 0..1000 {
            let x = f64::from(random.integer(0, 64000)) / 64.0;
            let y = f64::from(random.integer(0, 64000)) / 64.0;
            let za = a.elevation_at_xy(x, y);
            let zb = b.elevation_at_xy(x + EAST, y + NORTH);
            assert_eq!(za.is_some(), zb.is_some());
            if let (Some(za), Some(zb)) = (za, zb) {
                near(za, zb, 1e-10);
            }
        }
    }

    // Arbitrary (not exactly shiftable) coordinates on a plane. Adding 5e6 rounds
    // every coordinate by up to half an ulp (4.7e-10 m); with |grad z| = 0.36 the
    // elevation of a data point or query can move by up to 0.36 * sqrt(2) * 4.7e-10
    // = 2.4e-10 m. Data and query rounding together stay below 1e-9 m.
    #[test]
    fn plane_at_utm_magnitude() {
        let plane = Plane::new(0.3, -0.2, 12.5);
        let mut random = Random::default();
        let mut points = plane_points(&plane, 100.0, 1500, &mut random);
        for p in &mut points {
            p.x += EAST;
            p.y += NORTH;
        }
        let surface = build_from_points(points);
        near(surface.plan_area(), 1e4, 1e-5); // perimeter 400 m * 4.7e-10 m rounding
        near(surface.surface_area(), 1e4 * 1.13f64.sqrt(), 1e-5);
        for _ in 0..1000 {
            let x = random.real(0.0, 100.0);
            let y = random.real(0.0, 100.0);
            let z = surface
                .elevation_at_xy(x + EAST, y + NORTH)
                .expect("on the surface");
            near(z, plane.at(x, y), 1e-9);
        }
        let slope_aspect = surface.triangle_slope_aspect(0).expect("not degenerate");
        // Slope from coordinates carrying 4.7e-10 m of rounding over edges of ~1 m.
        near(slope_aspect.slope, 0.13f64.sqrt(), 1e-7);
    }
}

// ---- elevations_at runs across the cores and must not notice ----------------

mod tin_surface_batch_query {
    use super::*;

    #[test]
    fn elevations_at_gives_exactly_what_elevation_at_gives_one_at_a_time() {
        let mut random = Random::new(4242);
        let plane = Plane::new(0.25, -0.125, 40.0); // exact in binary, so a mismatch is a real one
        let surface = build_from_points(plane_points(&plane, 100.0, 4000, &mut random));
        assert!(!surface.empty());

        // Well past the 256-position chunk, so the work is genuinely shared out,
        // and deliberately half off the surface so both answers - a level and no
        // level at all - are carried back from the worker threads.
        let mut probes: Vec<Point2> = Vec::with_capacity(5000);
        for _ in 0..5000 {
            let x = random.real(-50.0, 150.0);
            let y = random.real(-50.0, 150.0);
            probes.push(p2(x, y));
        }

        let batch = surface.elevations_at(&probes);
        assert_eq!(batch.len(), probes.len());

        let mut on_surface = 0;
        let mut off_surface = 0;
        for (i, &probe) in probes.iter().enumerate() {
            let one = surface.elevation_at(probe);
            assert_eq!(batch[i].is_some(), one.is_some(), "probe {i}");
            if let Some(one) = one {
                assert_eq!(batch[i], Some(one), "probe {i}"); // identical bits, not merely close
                on_surface += 1;
            } else {
                off_surface += 1;
            }
        }
        assert!(
            on_surface > 1000,
            "the probes must actually land on the surface"
        );
        assert!(off_surface > 1000, "and off it");
    }

    #[test]
    fn elevations_at_gives_the_same_answer_on_one_thread_as_on_all_of_them() {
        let mut random = Random::new(99);
        let plane = Plane::new(-0.5, 0.25, 8.0);
        let surface = build_from_points(plane_points(&plane, 64.0, 3000, &mut random));
        assert!(!surface.empty());

        let mut probes: Vec<Point2> = Vec::with_capacity(3000);
        for _ in 0..3000 {
            let x = random.real(-8.0, 72.0);
            let y = random.real(-8.0, 72.0);
            probes.push(p2(x, y));
        }

        let many = surface.elevations_at(&probes);
        // A parallel_ranges() issued while the pool is already busy runs its whole
        // range on the calling thread, so this is the same call made with a
        // concurrency of one. It must be the same bytes.
        let one: std::sync::Mutex<Vec<Option<f64>>> = std::sync::Mutex::new(Vec::new());
        TaskPool::shared().parallel_ranges(0, 1, 1, |_, _| {
            *one.lock().unwrap() = surface.elevations_at(&probes);
        });
        assert_eq!(one.into_inner().unwrap(), many);
    }
}
