//! Twin of tests/terrain/test_tin_builder.cpp.

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
use katana_geometry::polygon::convex_hull;
use katana_geometry::{Point2, Point3, Polyline2};
use katana_terrain::{
    Breakline, CrossingBreaklinePolicy, DuplicatePointPolicy, NO_TRIANGLE, NO_VERTEX,
    TinBuildOptions, TinInput, TinSurface, build_tin,
};
use support::*;

/// True when some triangle has the undirected edge between the two positions;
/// with it, whether that edge is constrained.
fn has_edge(surface: &TinSurface, p: Point2, q: Point2) -> Option<bool> {
    for t in 0..surface.triangle_count() {
        let tri = surface.triangles()[t];
        for k in 0..3 {
            let a = surface.vertices()[tri[k] as usize];
            let b = surface.vertices()[tri[(k + 1) % 3] as usize];
            let forward = a.x == p.x && a.y == p.y && b.x == q.x && b.y == q.y;
            let backward = a.x == q.x && a.y == q.y && b.x == p.x && b.y == p.y;
            if forward || backward {
                return Some(surface.is_edge_constrained(t, k));
            }
        }
    }
    None
}

/// Structural invariants every surface must satisfy.
fn expect_consistent(surface: &TinSurface) {
    assert_eq!(surface.neighbors().len(), surface.triangle_count());
    assert_eq!(surface.constrained_edges().len(), surface.triangle_count());
    for t in 0..surface.triangle_count() {
        let tri = surface.triangles()[t];
        assert!(
            tri[0] < tri[1],
            "triangle must start at its smallest vertex index"
        );
        assert!(tri[0] < tri[2]);
        assert!(
            surface.plan_triangle(t).signed_area() >= 0.0,
            "counter-clockwise in plan"
        );
        if t > 0 {
            assert!(surface.triangles()[t - 1] < tri, "triangles must be sorted");
        }
        for k in 0..3 {
            let n = surface.neighbors()[t][k];
            if n == NO_TRIANGLE {
                continue;
            }
            assert!((n as usize) < surface.triangle_count());
            // The neighbour must run the shared edge the other way round and
            // point back at this triangle, with the same constraint flag.
            let other = surface.triangles()[n as usize];
            let mut found = false;
            for j in 0..3 {
                if other[j] == tri[(k + 1) % 3] && other[(j + 1) % 3] == tri[k] {
                    found = true;
                    assert_eq!(surface.neighbors()[n as usize][j], t as u32);
                    assert_eq!(
                        surface.is_edge_constrained(n as usize, j),
                        surface.is_edge_constrained(t, k)
                    );
                }
            }
            assert!(found, "triangle {t} edge {k}");
        }
    }
}

fn polygon(points: &[(f64, f64)], closed: bool) -> Polyline2 {
    Polyline2::new(points.iter().map(|&(x, y)| p2(x, y)).collect(), closed)
}

// ---- basic construction ----------------------------------------------------------

mod tin_builder {
    use super::*;

    #[test]
    fn single_triangle() {
        let input = TinInput {
            points: vec![p3(0.0, 0.0, 1.0), p3(4.0, 0.0, 2.0), p3(0.0, 3.0, 3.0)],
            ..TinInput::default()
        };
        let result = build(&input);
        assert_eq!(result.surface.triangle_count(), 1);
        assert_eq!(result.surface.vertex_count(), 3);
        assert_eq!(result.surface.triangles()[0], [0, 1, 2]);
        double_eq(result.surface.plan_area(), 6.0); // 4 * 3 / 2
        double_eq(result.surface.min_elevation(), 1.0);
        double_eq(result.surface.max_elevation(), 3.0);
        assert_eq!(result.surface.bounds().min, p2(0.0, 0.0));
        assert_eq!(result.surface.bounds().max, p2(4.0, 3.0));
        assert_eq!(result.report.point_vertex, vec![0, 1, 2]);
        expect_consistent(&result.surface);
    }

    #[test]
    fn vertices_keep_input_order() {
        let mut random = Random::default();
        let points = plane_points(&Plane::new(0.1, 0.2, 3.0), 50.0, 200, &mut random);
        let surface = build_from_points(points.clone());
        assert_eq!(surface.vertex_count(), points.len());
        for (i, point) in points.iter().enumerate() {
            assert_eq!(surface.vertices()[i], *point);
        }
        // Euler: a triangulation of n points with h on the hull has 2n - 2 - h triangles.
        assert_eq!(surface.triangle_count(), 2 * points.len() - 2 - 4);
        expect_consistent(&surface);
    }

    #[test]
    fn survey_point_on_breakline_splits_it_and_keeps_its_elevation() {
        let input = TinInput {
            points: vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(10.0, 10.0, 0.0),
                p3(0.0, 10.0, 0.0),
                p3(5.0, 5.0, 7.0), // exactly on the diagonal breakline below
            ],
            breaklines: vec![Breakline {
                vertices: vec![p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 0.0)],
                closed: false,
            }],
            ..TinInput::default()
        };
        let surface = build(&input).surface;
        assert_eq!(surface.vertex_count(), 5);
        near(surface.elevation_at_xy(5.0, 5.0).unwrap(), 7.0, 1e-12);
        near(surface.elevation_at_xy(2.5, 2.5).unwrap(), 3.5, 1e-12); // halfway up to the point
        assert_eq!(has_edge(&surface, p2(0.0, 0.0), p2(5.0, 5.0)), Some(true));
        assert_eq!(has_edge(&surface, p2(5.0, 5.0), p2(10.0, 10.0)), Some(true));
    }

    #[test]
    fn crossing_breaklines_that_agree_create_a_vertex() {
        let input = TinInput {
            points: vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 5.0),
                p3(10.0, 10.0, 10.0),
                p3(0.0, 10.0, 5.0),
            ],
            breaklines: vec![
                Breakline {
                    vertices: vec![p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0)],
                    closed: false,
                },
                Breakline {
                    vertices: vec![p3(0.0, 10.0, 5.0), p3(10.0, 0.0, 5.0)],
                    closed: false,
                },
            ],
            ..TinInput::default()
        };
        let result = build(&input);
        assert_eq!(result.report.breakline_crossing_count, 1);
        assert_eq!(result.report.crossing_conflict_count, 0);
        assert_eq!(result.surface.vertex_count(), 5);
        assert_eq!(result.surface.triangle_count(), 4);
        // Created vertices come after all input vertices. Both breaklines pass
        // (5, 5) at z = 5.
        near(result.surface.vertices()[4].x, 5.0, 1e-12);
        near(result.surface.vertices()[4].y, 5.0, 1e-12);
        near(result.surface.vertices()[4].z, 5.0, 1e-12);
        expect_consistent(&result.surface);
    }

    #[test]
    fn crossing_breaklines_that_disagree_follow_the_policy() {
        let input = TinInput {
            points: vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 8.0),
                p3(10.0, 10.0, 10.0),
                p3(0.0, 10.0, 8.0),
            ],
            breaklines: vec![
                Breakline {
                    vertices: vec![p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0)],
                    closed: false,
                }, // 5 at centre
                Breakline {
                    vertices: vec![p3(0.0, 10.0, 8.0), p3(10.0, 0.0, 8.0)],
                    closed: false,
                }, // 8 at centre
            ],
            ..TinInput::default()
        };

        let rejected = build_tin(&input, &TinBuildOptions::default());
        assert_eq!(
            rejected.err().map(|error| error.code),
            Some(ErrorCode::InvalidGeometry)
        );

        let options = TinBuildOptions {
            crossing_breaklines: CrossingBreaklinePolicy::Average,
            ..TinBuildOptions::default()
        };
        let averaged = build_with(&input, &options);
        assert_eq!(averaged.report.breakline_crossing_count, 1);
        assert_eq!(averaged.report.crossing_conflict_count, 1);
        assert_eq!(averaged.surface.vertex_count(), 5);
        near(averaged.surface.vertices()[4].z, 6.5, 1e-12); // (5 + 8) / 2
    }

    // ---- boundary and holes --------------------------------------------------------------

    #[test]
    fn concave_boundary_clips_triangles() {
        let plane = Plane::new(0.5, -0.25, 3.0);
        let input = TinInput {
            points: grid_points(&plane, 11), // 0..10 in both directions
            // L shape: 10 x 4 strip along the bottom plus 4 x 6 strip up the left side.
            boundary: polygon(
                &[
                    (0.0, 0.0),
                    (10.0, 0.0),
                    (10.0, 4.0),
                    (4.0, 4.0),
                    (4.0, 10.0),
                    (0.0, 10.0),
                ],
                true,
            ),
            ..TinInput::default()
        };
        let result = build(&input);
        let surface = &result.surface;

        near(surface.plan_area(), 10.0 * 4.0 + 4.0 * 6.0, 1e-9);
        // Grid points strictly beyond the inner corner are no longer part of the
        // surface: x and y in 5..10 -> 36 points.
        assert_eq!(result.report.dropped_vertex_count, 36);
        assert_eq!(surface.vertex_count(), 121 - 36);
        assert!(result.report.clipped_triangle_count > 0);
        assert_eq!(result.report.point_vertex[10 * 11 + 10], NO_VERTEX); // point (10, 10)
        assert_ne!(result.report.point_vertex[0], NO_VERTEX);

        assert!(
            surface.elevation_at_xy(7.0, 7.0).is_none(),
            "inside the hull, outside the L"
        );
        assert!(surface.elevation_at_xy(4.5, 4.5).is_none());
        near(
            surface.elevation_at_xy(2.0, 8.0).expect("inside"),
            plane.at(2.0, 8.0),
            1e-9,
        );
        near(
            surface.elevation_at_xy(8.5, 2.5).expect("inside"),
            plane.at(8.5, 2.5),
            1e-9,
        );
        // The inner corner edges are surface rim now, and constrained.
        assert_eq!(has_edge(surface, p2(4.0, 4.0), p2(5.0, 4.0)), Some(true));
        assert!(surface.elevation_at_xy(7.0, 4.0).is_some(), "on the rim");
        expect_consistent(surface);
    }

    #[test]
    fn boundary_vertices_off_the_data_are_interpolated() {
        let plane = Plane::new(0.5, -0.25, 3.0);
        let input = TinInput {
            points: grid_points(&plane, 11),
            // The same L moved half a cell inwards: no boundary vertex is a grid point.
            boundary: polygon(
                &[
                    (0.5, 0.5),
                    (9.5, 0.5),
                    (9.5, 3.5),
                    (3.5, 3.5),
                    (3.5, 9.5),
                    (0.5, 9.5),
                ],
                true,
            ),
            ..TinInput::default()
        };
        let surface = build(&input).surface;
        near(surface.plan_area(), 9.0 * 3.0 + 3.0 * 6.0, 1e-9);
        assert_eq!(surface.bounds().min, p2(0.5, 0.5));
        assert_eq!(surface.bounds().max, p2(9.5, 9.5));
        // The data is a plane, so the interpolated boundary vertices are on it too
        // and the clipped surface still reproduces the plane everywhere.
        let mut random = Random::default();
        for _ in 0..200 {
            let x = random.real(0.5, 9.5);
            let y = random.real(0.5, 3.5);
            let z = surface.elevation_at(p2(x, y)).expect("inside");
            near(z, plane.at(x, y), 1e-9);
        }
        assert!(surface.elevation_at_xy(0.25, 0.25).is_none());
        assert!(surface.elevation_at_xy(6.0, 6.0).is_none());
    }

    #[test]
    fn clockwise_boundary_and_repeated_closing_vertex_are_accepted() {
        let input = TinInput {
            points: grid_points(&Plane::default(), 5),
            // clockwise, first vertex repeated
            boundary: polygon(
                &[(1.0, 1.0), (1.0, 3.0), (3.0, 3.0), (3.0, 1.0), (1.0, 1.0)],
                false,
            ),
            ..TinInput::default()
        };
        let surface = build(&input).surface;
        near(surface.plan_area(), 4.0, 1e-12);
    }

    #[test]
    fn hole_removes_triangles_inside() {
        let plane = Plane::new(1.0, 0.0, 0.0);
        let input = TinInput {
            points: grid_points(&plane, 11),
            holes: vec![polygon(
                &[(3.0, 3.0), (6.0, 3.0), (6.0, 6.0), (3.0, 6.0)],
                true,
            )],
            ..TinInput::default()
        };
        let result = build(&input);
        let surface = &result.surface;
        near(surface.plan_area(), 100.0 - 9.0, 1e-9);
        assert_eq!(result.report.dropped_vertex_count, 4); // (4,4) (5,4) (4,5) (5,5)
        assert!(surface.elevation_at_xy(4.5, 4.5).is_none());
        assert!(surface.elevation_at_xy(5.0, 5.0).is_none());
        near(
            surface.elevation_at_xy(3.0, 4.5).expect("rim of the hole"),
            3.0,
            1e-12,
        );
        assert!(surface.elevation_at_xy(8.0, 8.0).is_some());
        expect_consistent(surface);
    }

    #[test]
    fn boundary_with_hole_and_breakline_together() {
        let input = TinInput {
            points: grid_points(&Plane::new(0.0, 1.0, 0.0), 11),
            boundary: polygon(&[(1.0, 1.0), (9.0, 1.0), (9.0, 9.0), (1.0, 9.0)], true),
            holes: vec![polygon(
                &[(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0)],
                true,
            )],
            // Crosses the boundary twice and runs below the hole.
            breaklines: vec![Breakline {
                vertices: vec![p3(0.0, 2.5, 2.5), p3(10.0, 2.5, 2.5)],
                closed: false,
            }],
        };
        let surface = build(&input).surface;
        near(surface.plan_area(), 64.0 - 4.0, 1e-9);
        near(surface.elevation_at_xy(5.0, 2.5).unwrap(), 2.5, 1e-12);
        assert!(
            surface.elevation_at_xy(0.5, 2.5).is_none(),
            "breakline part outside"
        );
        assert!(surface.elevation_at_xy(5.0, 5.0).is_none());
        expect_consistent(&surface);
    }

    #[test]
    fn rejects_bad_rings() {
        let mut input = TinInput {
            points: grid_points(&Plane::default(), 11),
            ..TinInput::default()
        };
        let code_of = |input: &TinInput| {
            build_tin(input, &TinBuildOptions::default())
                .err()
                .map(|error| error.code)
        };

        // Asymmetric bow tie (signed area -16, so it is not rejected as degenerate):
        // edges (1,1)-(9,9) and (9,1)-(1,5) cross at x = y = 11/3, off the grid.
        input.boundary = polygon(&[(1.0, 1.0), (9.0, 9.0), (9.0, 1.0), (1.0, 5.0)], true);
        let bow_tie = build_tin(&input, &TinBuildOptions::default());
        let error = bow_tie.expect_err("a bow tie is refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(
            error.message.contains("crosses itself"),
            "{}",
            error.describe()
        );

        input.boundary = polygon(&[(1.0, 1.0), (9.0, 9.0)], true);
        assert_eq!(code_of(&input), Some(ErrorCode::InvalidGeometry));

        input.boundary = polygon(&[(1.0, 1.0), (5.0, 5.0), (9.0, 9.0)], true);
        assert_eq!(code_of(&input), Some(ErrorCode::InvalidGeometry));

        // A vertex beyond the data cannot be given an elevation.
        input.boundary = polygon(&[(1.0, 1.0), (12.0, 1.0), (12.0, 9.0), (1.0, 9.0)], true);
        assert_eq!(code_of(&input), Some(ErrorCode::InvalidGeometry));

        // Ring touching itself in a vertex (figure of eight through (5, 5)).
        input.boundary = polygon(
            &[
                (1.0, 1.0),
                (5.0, 5.0),
                (9.0, 1.0),
                (9.0, 9.0),
                (5.0, 5.0),
                (1.0, 9.0),
            ],
            true,
        );
        assert_eq!(code_of(&input), Some(ErrorCode::InvalidGeometry));

        // Everything clipped away.
        input.boundary = Polyline2::default();
        input.holes.push(polygon(
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            true,
        ));
        assert_eq!(code_of(&input), Some(ErrorCode::TriangulationFailure));
    }
}

mod breakline_rhombus {
    use super::*;

    // Thin rhombus: left/right tips far apart, top/bottom close together. The
    // circumcircle of (left, bottom, top) has its centre at (-2.4, 0) and radius 2.6,
    // so the right tip (distance 7.4) is outside: Delaunay joins top and bottom. The
    // breakline along the long diagonal is therefore the NON-Delaunay diagonal.
    const LEFT: Point3 = Point3::new(-5.0, 0.0, 10.0);
    const RIGHT: Point3 = Point3::new(5.0, 0.0, 20.0);
    const TOP: Point3 = Point3::new(0.0, 1.0, 0.0);
    const BOTTOM: Point3 = Point3::new(0.0, -1.0, 0.0);

    #[test]
    fn without_breakline_delaunay_uses_short_diagonal() {
        let surface = build_from_points(vec![LEFT, RIGHT, TOP, BOTTOM]);
        assert_eq!(surface.triangle_count(), 2);
        assert!(has_edge(&surface, p2(0.0, 1.0), p2(0.0, -1.0)).is_some());
        assert!(has_edge(&surface, p2(-5.0, 0.0), p2(5.0, 0.0)).is_none());
        // The centre lies on the top-bottom edge, both ends at z = 0.
        near(surface.elevation_at_xy(0.0, 0.0).expect("on"), 0.0, 1e-12);
    }

    #[test]
    fn breakline_forces_long_diagonal_and_elevation_follows_it() {
        let input = TinInput {
            points: vec![LEFT, RIGHT, TOP, BOTTOM],
            breaklines: vec![Breakline {
                vertices: vec![LEFT, RIGHT],
                closed: false,
            }],
            ..TinInput::default()
        };
        let result = build(&input);
        let surface = &result.surface;
        assert_eq!(surface.triangle_count(), 2);
        assert_eq!(
            surface.vertex_count(),
            4,
            "breakline vertices coincide with the points"
        );
        assert_eq!(result.report.shared_breakline_vertex_count, 2);
        assert_eq!(result.report.duplicate_point_count, 0);

        assert_eq!(has_edge(surface, p2(-5.0, 0.0), p2(5.0, 0.0)), Some(true));
        assert!(has_edge(surface, p2(0.0, 1.0), p2(0.0, -1.0)).is_none());

        // Along the breakline z runs linearly from 10 at x = -5 to 20 at x = 5.
        for x in [-5.0, -2.5, 0.0, 1.25, 5.0] {
            let z = surface.elevation_at_xy(x, 0.0).expect("on the breakline");
            near(z, 15.0 + x, 1e-12);
        }
        expect_consistent(surface);
    }
}

// ---- duplicate points ---------------------------------------------------------------

mod duplicate_points {
    use super::*;

    fn input(duplicate_z: f64, offset: f64) -> TinInput {
        TinInput {
            points: vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(10.0, 10.0, 0.0),
                p3(0.0, 10.0, 0.0),
                p3(5.0, 5.0, 4.0),
                p3(5.0 + offset, 5.0, duplicate_z),
            ],
            ..TinInput::default()
        }
    }

    #[test]
    fn same_elevation_is_merged_and_counted() {
        let result = build(&input(4.0, 0.0));
        assert_eq!(result.report.duplicate_point_count, 1);
        assert_eq!(result.report.elevation_conflict_count, 0);
        assert_eq!(result.surface.vertex_count(), 5);
        assert_eq!(result.report.point_vertex[5], result.report.point_vertex[4]);
    }

    #[test]
    fn within_geometric_tolerance_is_the_same_position() {
        // 5e-8 m apart: below tolerance::GEOMETRIC (1e-7 m).
        assert_eq!(build(&input(4.0, 5e-8)).surface.vertex_count(), 5);
        // 1e-6 m apart: two vertices.
        let apart = build(&input(4.0, 1e-6));
        assert_eq!(apart.surface.vertex_count(), 6);
        assert_eq!(apart.report.duplicate_point_count, 0);
    }

    #[test]
    fn conflicting_elevation_is_an_error_by_default() {
        let error =
            build_tin(&input(4.5, 0.0), &TinBuildOptions::default()).expect_err("a conflict");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.context.contains("point 5"), "{}", error.describe());

        // A difference below the elevation tolerance (default 1e-4 m) is no conflict.
        assert!(build_tin(&input(4.00005, 0.0), &TinBuildOptions::default()).is_ok());
    }

    #[test]
    fn keep_first_and_average_policies() {
        let mut options = TinBuildOptions {
            duplicate_points: DuplicatePointPolicy::KeepFirst,
            ..TinBuildOptions::default()
        };
        let first = build_with(&input(6.0, 0.0), &options);
        assert_eq!(first.report.duplicate_point_count, 1);
        assert_eq!(first.report.elevation_conflict_count, 1);
        double_eq(first.surface.elevation_at_xy(5.0, 5.0).unwrap(), 4.0);

        options.duplicate_points = DuplicatePointPolicy::Average;
        let average = build_with(&input(6.0, 0.0), &options);
        assert_eq!(average.report.elevation_conflict_count, 1);
        double_eq(average.surface.elevation_at_xy(5.0, 5.0).unwrap(), 5.0);
    }

    #[test]
    fn breakline_vertex_conflicting_with_point_is_an_error() {
        let mut with_breakline = input(4.0, 0.0);
        with_breakline.breaklines.push(Breakline {
            vertices: vec![p3(0.0, 0.0, 0.0), p3(5.0, 5.0, 9.0)],
            closed: false,
        });
        let result = build_tin(&with_breakline, &TinBuildOptions::default());
        assert_eq!(
            result.err().map(|error| error.code),
            Some(ErrorCode::InvalidGeometry)
        );
    }
}

// ---- error cases ----------------------------------------------------------------------

mod tin_builder_errors {
    use super::*;

    fn code_of(input: &TinInput) -> Option<ErrorCode> {
        build_tin(input, &TinBuildOptions::default())
            .err()
            .map(|error| error.code)
    }

    #[test]
    fn too_few_or_collinear_positions() {
        let mut input = TinInput::default();
        assert_eq!(code_of(&input), Some(ErrorCode::TriangulationFailure));

        input.points = vec![p3(0.0, 0.0, 0.0), p3(1.0, 1.0, 1.0)];
        assert_eq!(code_of(&input), Some(ErrorCode::TriangulationFailure));

        // Three points of which two coincide are two positions.
        input.points = vec![p3(0.0, 0.0, 0.0), p3(1.0, 1.0, 1.0), p3(1.0, 1.0, 1.0)];
        assert_eq!(code_of(&input), Some(ErrorCode::TriangulationFailure));

        input.points = vec![
            p3(0.0, 0.0, 0.0),
            p3(1.0, 1.0, 1.0),
            p3(2.0, 2.0, 5.0),
            p3(7.0, 7.0, 2.0),
        ];
        assert_eq!(code_of(&input), Some(ErrorCode::TriangulationFailure));
    }

    #[test]
    fn non_finite_input() {
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let base = TinInput {
            points: vec![p3(0.0, 0.0, 0.0), p3(4.0, 0.0, 0.0), p3(0.0, 4.0, 0.0)],
            ..TinInput::default()
        };

        for bad in [p3(nan, 1.0, 1.0), p3(1.0, inf, 1.0), p3(1.0, 1.0, nan)] {
            let mut input = base.clone();
            input.points.push(bad);
            let error = build_tin(&input, &TinBuildOptions::default()).expect_err("refused");
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(error.context.contains("point=3"));
        }

        let mut breakline = base.clone();
        breakline.breaklines.push(Breakline {
            vertices: vec![p3(0.0, 0.0, 0.0), p3(1.0, nan, 0.0)],
            closed: false,
        });
        assert_eq!(code_of(&breakline), Some(ErrorCode::InvalidArgument));

        let mut boundary = base.clone();
        boundary.boundary = polygon(&[(0.0, 0.0), (1.0, 0.0), (inf, 1.0)], true);
        assert_eq!(code_of(&boundary), Some(ErrorCode::InvalidArgument));
    }

    #[test]
    fn bad_breaklines_and_options() {
        let mut input = TinInput {
            points: vec![p3(0.0, 0.0, 0.0), p3(4.0, 0.0, 0.0), p3(0.0, 4.0, 0.0)],
            ..TinInput::default()
        };
        input.breaklines.push(Breakline {
            vertices: vec![p3(1.0, 1.0, 0.0)],
            closed: false,
        });
        assert_eq!(code_of(&input), Some(ErrorCode::InvalidGeometry));

        input.breaklines = vec![Breakline {
            vertices: vec![p3(1.0, 1.0, 0.0), p3(1.0, 1.0, 0.0)],
            closed: false,
        }]; // no length
        assert_eq!(code_of(&input), Some(ErrorCode::InvalidGeometry));

        input.breaklines.clear();
        let options = TinBuildOptions {
            elevation_tolerance: -1.0,
            ..TinBuildOptions::default()
        };
        assert_eq!(
            build_tin(&input, &options).err().map(|error| error.code),
            Some(ErrorCode::InvalidArgument)
        );
    }
}

// ---- determinism -----------------------------------------------------------------------

mod tin_builder_determinism {
    use super::*;

    #[test]
    fn identical_input_gives_bitwise_identical_surface() {
        let mut random = Random::default();
        let mut input = TinInput::default();
        for _ in 0..3000 {
            let x = random.real(0.0, 500.0);
            let y = random.real(0.0, 500.0);
            let z = random.real(90.0, 110.0);
            input.points.push(p3(x, y, z));
        }
        input.breaklines.push(Breakline {
            vertices: vec![
                p3(50.0, 60.0, 100.0),
                p3(450.0, 80.0, 101.0),
                p3(430.0, 440.0, 99.0),
            ],
            closed: false,
        });
        input.breaklines.push(Breakline {
            vertices: vec![p3(60.0, 450.0, 100.0), p3(440.0, 50.0, 100.0)],
            closed: false,
        });
        input.holes.push(polygon(
            &[
                (200.0, 200.0),
                (260.0, 210.0),
                (250.0, 270.0),
                (190.0, 250.0),
            ],
            true,
        ));
        let options = TinBuildOptions {
            crossing_breaklines: CrossingBreaklinePolicy::Average,
            ..TinBuildOptions::default()
        };

        let first = build_with(&input, &options);
        assert!(!first.surface.empty());
        // The first segment of breakline 0 (y = 60 + 0.05 (x - 50)) meets breakline 1
        // (y = 450 - (400/380) (x - 60)) at x = 413.2; no other pair of segments
        // crosses. Breakline 1 also runs through the hole, which creates vertices on
        // the hole's ring but is not a breakline crossing.
        assert_eq!(first.report.breakline_crossing_count, 1);

        // Disturb the heap so that the second run sees different addresses: the
        // result must not depend on pointer values (hash or set order of handles).
        let mut ballast: Vec<Vec<f64>> = Vec::new();
        for i in 1..=257usize {
            ballast.push(vec![1.0; i * 37]);
        }
        let second = build_with(&input, &options);
        ballast.clear();
        let third = build_with(&input, &options);

        assert!(first.surface == second.surface);
        assert!(first.surface == third.surface);
        assert_eq!(first.report.point_vertex, second.report.point_vertex);
        assert_eq!(first.surface.plan_area(), second.surface.plan_area()); // bitwise
        assert_eq!(first.surface.surface_area(), second.surface.surface_area()); // bitwise
        expect_consistent(&first.surface);
    }
}

// ---- properties ---------------------------------------------------------------------------

mod tin_builder_properties {
    use super::*;

    #[test]
    fn plan_area_equals_convex_hull_area() {
        let mut random = Random::default();
        for iteration in 0..25 {
            let count = random.integer(3, 400);
            let mut points: Vec<Point3> = Vec::new();
            let mut plan: Vec<Point2> = Vec::new();
            for _ in 0..count {
                let x = random.real(-100.0, 100.0);
                let y = random.real(-50.0, 50.0);
                let z = random.real(0.0, 10.0);
                points.push(p3(x, y, z));
                plan.push(p2(x, y));
            }
            let surface = build_from_points(points);
            let hull_area = Polyline2::new(convex_hull(plan), true).area();
            // Sum of up to ~800 triangle areas of magnitude <= 2e4: rounding far below 1e-7.
            assert!(
                (surface.plan_area() - hull_area).abs() <= 1e-7 * hull_area.max(1.0),
                "{iteration}"
            );
        }
    }

    #[test]
    fn every_input_point_interpolates_to_its_own_elevation() {
        let mut random = Random::default();
        let mut points: Vec<Point3> = Vec::new();
        for _ in 0..2000 {
            let x = random.real(0.0, 1000.0);
            let y = random.real(0.0, 1000.0);
            let z = random.real(-20.0, 80.0);
            points.push(p3(x, y, z));
        }
        let surface = build_from_points(points.clone());
        for p in &points {
            let z = surface.elevation_at_xy(p.x, p.y).expect("on the surface");
            // At a vertex the barycentric weights are exactly (1, 0, 0).
            double_eq(z, p.z);
        }
    }

    #[test]
    fn unconstrained_triangles_have_empty_circumcircles() {
        let mut random = Random::default();
        for _ in 0..5 {
            let mut points: Vec<Point3> = Vec::new();
            for _ in 0..250 {
                let x = random.real(0.0, 100.0);
                let y = random.real(0.0, 100.0);
                points.push(p3(x, y, 0.0));
            }
            let surface = build_from_points(points.clone());
            for t in 0..surface.triangle_count() {
                let Some(circle) = surface.plan_triangle(t).circumcircle() else {
                    continue; // sliver: its circle is numerically meaningless
                };
                // Cocircular points are allowed; the slack covers the rounding of
                // the circle construction (relative to its radius).
                let slack = 1e-9 * circle.radius.max(1.0);
                for p in &points {
                    assert!(
                        circle.center.distance_to(p2(p.x, p.y)) >= circle.radius - slack,
                        "triangle {t}"
                    );
                }
            }
        }
    }

    #[test]
    fn nearly_collinear_hull_points_do_not_break_the_surface() {
        // (50, 1e-13) is a hair off the bottom edge: exact predicates make a sliver
        // triangle out of it that evaluates to (almost) zero area in doubles.
        let input = TinInput {
            points: vec![
                p3(0.0, 0.0, 0.0),
                p3(50.0, 1e-13, 10.0),
                p3(100.0, 0.0, 0.0),
                p3(50.0, 50.0, 0.0),
            ],
            ..TinInput::default()
        };
        let surface = build(&input).surface;
        assert_eq!(surface.vertex_count(), 4);
        near(surface.plan_area(), 2500.0, 1e-9);
        let z = surface.elevation_at_xy(25.0, 0.0).expect("on the rim");
        assert!(z >= 0.0);
        assert!(z <= 10.0);
        near(surface.elevation_at_xy(50.0, 25.0).unwrap(), 5.0, 1e-9); // halfway between (50,0+) and (50,50)
        expect_consistent(&surface);
    }
}
