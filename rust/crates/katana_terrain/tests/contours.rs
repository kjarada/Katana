//! Twin of tests/terrain/test_contours.cpp.

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
use katana_geometry::polygon::convex_hull;
use katana_geometry::{Point2, Point3, Polyline2};
use katana_math::numerics::tolerance;
use katana_terrain::contours::contours;
use katana_terrain::{Contour, TinInput, TinSurface, contour_at};
use support::*;

// Square pyramid used by several tests: base `base` x `base` at z = 0 centred on
// (base/2, base/2), apex `height` above the centre. The level L therefore cuts it
// in the axis-aligned square of half width s(L) = (base/2) (1 - L/height), whose
// perimeter is 8 s and whose enclosed area is 4 s^2. Both are exact because the
// four ridges are breaklines, so every triangle lies in one planar face.
const PYRAMID_BASE: f64 = 100.0;
const PYRAMID_HEIGHT: f64 = 50.0;

fn half_width_at_level(level: f64) -> f64 {
    0.5 * PYRAMID_BASE * (1.0 - level / PYRAMID_HEIGHT)
}

fn pyramid(height: f64, extra: usize) -> TinSurface {
    let mut random = Random::default();
    build(&pyramid_input(PYRAMID_BASE, height, extra, &mut random)).surface
}

fn code_of(result: katana_core::Result<Vec<Contour>>) -> Option<ErrorCode> {
    result.err().map(|error| error.code)
}

mod contour_at {
    use super::*;

    // ---- a plane: the level set is a straight line ------------------------------------------

    #[test]
    fn plane_gives_one_straight_line_with_higher_ground_on_the_left() {
        // z = x over [0, 100]^2, so the level L is the segment x = L, y in [0, 100]:
        // one open contour of length 100. The uphill direction is +x, and "higher
        // ground on the left" (left of a heading d is d rotated a quarter turn
        // counter-clockwise) forces the heading -y: the line starts at y = 100.
        let mut random = Random::default();
        let surface = build_from_points(plane_points(
            &Plane::new(1.0, 0.0, 0.0),
            100.0,
            400,
            &mut random,
        ));
        assert!(!surface.empty());

        let result =
            contour_at(&surface, 37.5).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(result.len(), 1);
        let contour = &result[0];
        assert_eq!(contour.elevation, 37.5);
        assert!(!contour.major); // contour_at() never flags an index contour
        assert!(!contour.line.closed);

        let v = &contour.line.vertices;
        assert!(v.len() >= 2);
        for p in v {
            // The crossing is x_low + (L - z_low)/(z_high - z_low) * (x_high - x_low)
            // with z == x: four roundings on magnitudes <= 100, well below 1e-12.
            near(p.x, 37.5, 1e-12);
        }
        near(v[0].y, 100.0, 1e-12);
        near(v[v.len() - 1].y, 0.0, 1e-12);
        near(contour.line.length(), 100.0, 1e-9);
    }

    // ---- hill and pit: closed rings with the documented orientation --------------------------

    #[test]
    fn pyramid_level_is_a_square_ring_counter_clockwise() {
        let surface = pyramid(PYRAMID_HEIGHT, 300);
        assert!(!surface.empty());

        for level in [10.0, 20.0, 30.0, 40.0] {
            let s = half_width_at_level(level);
            let result =
                contour_at(&surface, level).unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(result.len(), 1, "level {level}");
            let line = &result[0].line;
            assert!(line.closed);
            near(line.length(), 8.0 * s, 1e-9);
            // Positive = counter-clockwise: the hill is inside, so higher ground is
            // on the left all the way round.
            near(line.signed_area(), 4.0 * s * s, 1e-8);
        }
    }

    #[test]
    fn depression_ring_runs_clockwise() {
        // The same pyramid turned upside down: a square pit 50 m deep, z = -50 + d
        // with d the Chebyshev distance from the centre. The level L in (-50, 0) is
        // the square of half width d = L + 50, and { z >= L } is the ground OUTSIDE
        // it, so the ring is traced with the higher ground on its left = clockwise.
        let surface = pyramid(-PYRAMID_HEIGHT, 300);
        assert!(!surface.empty());
        assert_eq!(surface.min_elevation(), -PYRAMID_HEIGHT);
        assert_eq!(surface.max_elevation(), 0.0);

        for level in [-40.0, -30.0, -20.0, -10.0] {
            let s = level + PYRAMID_HEIGHT;
            let result =
                contour_at(&surface, level).unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(result.len(), 1, "level {level}");
            let line = &result[0].line;
            assert!(line.closed);
            near(line.length(), 8.0 * s, 1e-9);
            near(line.signed_area(), -4.0 * s * s, 1e-8);
        }
    }

    // ---- the degenerate case the "z >= level is above" rule exists for ------------------------

    #[test]
    fn level_through_vertices_gives_one_unbroken_line() {
        // Two triangles of the plane z = (x + y)/2 over [0, 2]^2, cut along the
        // diagonal (0,0)-(2,2). The level 1 passes exactly through the vertices
        // (2,0,1) and (0,2,1). The contour must be the single segment x + y = 2
        // through both of them: no duplicated vertex, no zero-length piece, no gap.
        let created = TinSurface::create(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(2.0, 0.0, 1.0),
                p3(2.0, 2.0, 2.0),
                p3(0.0, 2.0, 1.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![],
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));

        let result =
            contour_at(&created, 1.0).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(result.len(), 1);
        let line = &result[0].line;
        assert!(!line.closed);
        // Exact: a vertex hit is returned unchanged, and the diagonal is met at
        // fraction 1/2, which is exact in binary. Heading (2,0) -> (0,2) is ruled out
        // because the uphill direction (1,1) has to lie on the left.
        assert_eq!(line.vertices.len(), 3);
        assert_eq!(line.vertices[0], p2(0.0, 2.0));
        assert_eq!(line.vertices[1], p2(1.0, 1.0));
        assert_eq!(line.vertices[2], p2(2.0, 0.0));
        double_eq(line.length(), 2.0 * 2.0f64.sqrt());
    }
}

// ---- contours(): the ladder of levels -----------------------------------------------------

mod contours {
    use super::*;

    #[test]
    fn pyramid_ladder_ascends_and_flags_index_contours() {
        // interval 10 from base 0: the levels in (0, 50] are k = 1..5. Level 50 is the
        // apex and nothing else, which is a single point and therefore dropped, so
        // four rings remain. major_every = 2 flags k = 2 and k = 4.
        let surface = pyramid(PYRAMID_HEIGHT, 300);
        assert!(!surface.empty());

        let lines = contours(&surface, 10.0, 0.0, 2, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(lines.len(), 4);
        for (i, contour) in lines.iter().enumerate() {
            let k = (i + 1) as f64;
            let level = 10.0 * k;
            let s = half_width_at_level(level);
            assert_eq!(contour.elevation, level);
            assert_eq!(contour.major, (i + 1) % 2 == 0);
            assert!(contour.line.closed);
            near(contour.line.length(), 8.0 * s, 1e-9);
            near(contour.line.signed_area(), 4.0 * s * s, 1e-8);
        }

        // major_every = 0 means no index contours at all.
        let plain = contours(&surface, 10.0, 0.0, 0, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(plain.len(), 4);
        for contour in &plain {
            assert!(!contour.major);
        }
    }

    #[test]
    fn shifted_base_moves_the_ladder() {
        // base 5, interval 10: the levels in (0, 50] are 5, 15, 25, 35, 45.
        let surface = pyramid(PYRAMID_HEIGHT, 200);
        let result = contours(&surface, 10.0, 5.0, 5, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(result.len(), 5);
        for (i, contour) in result.iter().enumerate() {
            let level = 5.0 + 10.0 * i as f64;
            assert_eq!(contour.elevation, level);
            near(
                contour.line.length(),
                8.0 * half_width_at_level(level),
                1e-9,
            );
        }
    }

    // ---- errors and empty ranges ---------------------------------------------------------------

    #[test]
    fn rejects_bad_arguments_and_empty_ranges() {
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let surface = pyramid(PYRAMID_HEIGHT, 40);
        assert!(!surface.empty());

        assert_eq!(
            code_of(contour_at(&surface, nan)),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code_of(contour_at(&surface, inf)),
            Some(ErrorCode::InvalidArgument)
        );

        // The contour is the boundary of { z >= L }. At or below the lowest ground
        // that region is the whole surface and above the summit it is empty; either
        // way it has no boundary inside the surface.
        assert!(contour_at(&surface, 0.0).unwrap().is_empty());
        assert!(contour_at(&surface, -1.0).unwrap().is_empty());
        assert!(contour_at(&surface, 50.0 + 1e-6).unwrap().is_empty());
        // Exactly the summit: the region is the apex alone, a point, which is dropped.
        assert!(contour_at(&surface, PYRAMID_HEIGHT).unwrap().is_empty());

        let ladder =
            |interval: f64, base: f64| code_of(contours(&surface, interval, base, 5, None));
        assert_eq!(ladder(0.0, 0.0), Some(ErrorCode::InvalidArgument));
        assert_eq!(ladder(-5.0, 0.0), Some(ErrorCode::InvalidArgument));
        assert_eq!(ladder(nan, 0.0), Some(ErrorCode::InvalidArgument));
        assert_eq!(ladder(inf, 0.0), Some(ErrorCode::InvalidArgument));
        assert_eq!(
            ladder(tolerance::GEOMETRIC, 0.0),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(ladder(1.0, nan), Some(ErrorCode::InvalidArgument));
        // 50 m of relief at a 0.1 mm interval is 500000 lines, past MAX_CONTOUR_LEVELS.
        assert_eq!(ladder(1.0e-4, 0.0), Some(ErrorCode::InvalidArgument));

        let empty = TinSurface::new();
        assert!(contour_at(&empty, 1.0).unwrap().is_empty());
        assert!(contours(&empty, 1.0, 0.0, 5, None).unwrap().is_empty());
    }

    // ---- parallel tracing: identical at any thread count -------------------------------------

    #[test]
    fn are_identical_on_one_thread_and_on_many() {
        // Rolling ground over 200 m with 3000 random points and a 0.1 m interval:
        // some forty levels with open contours, rings and several pieces per
        // level, so levels really are traced on different threads, and the
        // chunks of levels differ with the thread count while the answer must
        // not.
        let mut random = Random::new(23);
        let height = |x: f64, y: f64| 12.0 + 1.5 * (x * 0.07).sin() * (y * 0.05).cos() + 0.004 * x;
        let mut points: Vec<Point3> = Vec::new();
        for _ in 0..3000 {
            let x = random.real(0.0, 200.0);
            let y = random.real(0.0, 200.0);
            points.push(p3(x, y, height(x, y)));
        }
        let surface = build_from_points(points);
        assert!(!surface.empty());

        let inline_only = TaskPool::new(0);
        let serial = contours(&surface, 0.1, 0.0, 5, Some(&inline_only))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(serial.len() > 40);

        let same_bits = |a: f64, b: f64| a.to_bits() == b.to_bits();
        for workers in [1usize, 3, 15] {
            let pool = TaskPool::new(workers);
            let parallel = contours(&surface, 0.1, 0.0, 5, Some(&pool))
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(parallel.len(), serial.len(), "workers {workers}");
            for (i, (a, b)) in serial.iter().zip(parallel.iter()).enumerate() {
                assert!(same_bits(a.elevation, b.elevation), "contour {i}");
                assert_eq!(a.major, b.major, "contour {i}");
                assert_eq!(a.line.closed, b.line.closed, "contour {i}");
                assert_eq!(a.line.vertices.len(), b.line.vertices.len(), "contour {i}");
                for (v, (p, q)) in a
                    .line
                    .vertices
                    .iter()
                    .zip(b.line.vertices.iter())
                    .enumerate()
                {
                    assert!(
                        same_bits(p.x, q.x) && same_bits(p.y, q.y),
                        "contour {i} vertex {v} workers {workers}"
                    );
                }
            }
        }
    }

    #[test]
    fn stay_in_ascending_level_order_when_traced_in_parallel() {
        // The documented order - levels ascending - is what joining the per-level
        // lists in level order has to keep, whichever thread finished first.
        let mut random = Random::new(29);
        let mut points: Vec<Point3> = Vec::new();
        for _ in 0..2000 {
            let x = random.real(0.0, 100.0);
            let y = random.real(0.0, 100.0);
            points.push(p3(x, y, 0.3 * x + 0.1 * y)); // 0 to 40 m
        }
        let surface = build_from_points(points);
        let pool = TaskPool::new(7);
        let lines = contours(&surface, 0.5, 0.0, 5, Some(&pool))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(lines.len() > 50);
        for i in 1..lines.len() {
            assert!(lines[i - 1].elevation <= lines[i].elevation, "{i}");
        }
    }
}

// ---- properties on rough ground --------------------------------------------------------

mod rough_ground {
    use super::*;

    /// Smooth but genuinely curved ground, so the level sets are neither straight
    /// nor all of one shape: several open contours and several rings per level.
    fn elevation(x: f64, y: f64) -> f64 {
        100.0 + 10.0 * (0.05 * x).sin() * (0.04 * y).cos()
    }

    fn surface() -> TinSurface {
        let mut random = Random::default();
        let mut input = TinInput::default();
        for _ in 0..1200 {
            let x = random.real(0.0, 200.0);
            let y = random.real(0.0, 200.0);
            input.points.push(p3(x, y, elevation(x, y)));
        }
        let surface = build(&input).surface;
        assert!(!surface.empty());
        surface
    }

    #[test]
    fn every_contour_vertex_interpolates_to_its_own_level() {
        let surface = surface();
        let result = contours(&surface, 2.0, 100.0, 5, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(result.len() >= 4);

        let mut vertices = 0;
        for contour in &result {
            let v = &contour.line.vertices;
            assert!(v.len() >= if contour.line.closed { 3 } else { 2 });
            for i in 0..v.len() {
                vertices += 1;
                // A vertex lies on a triangle edge, where locate() may pick either
                // side; both interpolate the same elevation because the surface is
                // continuous. Barycentric rounding on a 20 m relief stays far below
                // 1e-9 even on the thinnest Delaunay triangles here.
                let z = surface.elevation_at(v[i]).expect("on the surface");
                near(z, contour.elevation, 1e-9);
                assert_ne!(v[i], v[(i + 1) % v.len()]); // no zero-length piece
            }
        }
        assert!(vertices > 100);
    }

    #[test]
    fn levels_are_ascending_and_each_is_exactly_what_contour_at_gives() {
        let surface = surface();
        let interval = 2.5;
        let base = 99.0;
        let ladder = contours(&surface, interval, base, 0, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(!ladder.is_empty());

        let mut previous = f64::NEG_INFINITY;
        let mut index = 0;
        while index < ladder.len() {
            let level = ladder[index].elevation;
            assert!(level > previous);
            previous = level;
            // The level must be one of base + k * interval, and inside the relief.
            let k = ((level - base) / interval).round();
            assert_eq!(level, base + k * interval);
            assert!(level > surface.min_elevation());
            assert!(level <= surface.max_elevation());

            // Same level through the single-level entry point: identical geometry.
            let single =
                contour_at(&surface, level).unwrap_or_else(|error| panic!("{}", error.describe()));
            let mut count = 0;
            while index + count < ladder.len() && ladder[index + count].elevation == level {
                count += 1;
            }
            assert_eq!(single.len(), count, "level {level}");
            for i in 0..count {
                assert_eq!(single[i].line, ladder[index + i].line, "level {level}");
            }
            index += count;
        }
    }

    #[test]
    fn contours_stay_on_the_surface_and_open_ones_end_on_its_rim() {
        let surface = surface();
        let result = contours(&surface, 3.0, 100.0, 5, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let bounds = surface.bounds().inflated(tolerance::GEOMETRIC);

        // This surface has no boundary and no holes, so its rim is exactly the convex
        // hull of the input positions.
        let plan: Vec<Point2> = surface.vertices().iter().map(|p| p2(p.x, p.y)).collect();
        let rim = Polyline2::new(convex_hull(plan), true);

        let mut open = 0;
        let mut rings = 0;
        for contour in &result {
            for &p in &contour.line.vertices {
                assert!(bounds.contains(p), "{p:?}");
            }
            if contour.line.closed {
                rings += 1;
                continue;
            }
            open += 1;
            // An open contour is open because tracing ran out of triangles, which can
            // only happen on the rim. A crossing on a rim edge is that edge's own
            // interpolation, so it is on the hull to within its rounding.
            let vertices = &contour.line.vertices;
            for end in [vertices[0], vertices[vertices.len() - 1]] {
                let distance = rim.distance_to(end).expect("a rim");
                assert!(distance < 1e-9, "{end:?}");
            }
        }
        assert!(open > 0);
        assert!(rings > 0);
    }
}
