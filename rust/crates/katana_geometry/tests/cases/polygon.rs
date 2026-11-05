use crate::support::{Random, double_eq, near, vec2_eq};
use katana_core::ErrorCode;
use katana_geometry::intersection::intersect;
use katana_geometry::polygon::*;
use katana_geometry::{
    Box2, Containment, IntersectionKind, Point2, Polyline2, Segment2, Triangle2, Vec2,
};
use katana_math::numerics::TWO_PI;
fn point(east: f64, north: f64) -> Point2 {
    Point2::new(east, north)
}
fn polygon(points: &[(f64, f64)]) -> Polyline2 {
    Polyline2::new(
        points
            .iter()
            .map(|&(east, north)| point(east, north))
            .collect(),
        true,
    )
}
fn square(size: f64) -> Polyline2 {
    polygon(&[(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)])
}
fn l_shape() -> Polyline2 {
    polygon(&[
        (0.0, 0.0),
        (2.0, 0.0),
        (2.0, 1.0),
        (1.0, 1.0),
        (1.0, 2.0),
        (0.0, 2.0),
    ])
}
fn triangle_area_sum(polygon: &Polyline2, triangles: &[TriangleIndices]) -> f64 {
    let mut total = 0.0;
    for indices in triangles {
        let triangle = Triangle2::new(
            polygon.vertices[indices[0]],
            polygon.vertices[indices[1]],
            polygon.vertices[indices[2]],
        );
        assert!(triangle.signed_area() > 0.0);
        total += triangle.area();
    }
    total
}
fn kept_area_sum(polygon: &Polyline2, triangles: &[TriangleIndices]) -> f64 {
    let mut total = 0.0;
    for indices in triangles {
        let triangle = Triangle2::new(
            polygon.vertices[indices[0]],
            polygon.vertices[indices[1]],
            polygon.vertices[indices[2]],
        );
        assert!(triangle.signed_area() >= -1e-9);
        total += triangle.signed_area();
    }
    total
}
fn has_directed_edge(triangles: &[TriangleIndices], first: usize, second: usize) -> bool {
    triangles.iter().any(|triangle| {
        (0..3).any(|index| triangle[index] == first && triangle[(index + 1) % 3] == second)
    })
}
fn uses_vertex(triangles: &[TriangleIndices], vertex: usize) -> bool {
    triangles.iter().any(|triangle| triangle.contains(&vertex))
}
fn run(runs: &PolylineRuns, index: usize) -> &[Point2] {
    &runs.points[runs.begin(index)..runs.ends[index]]
}
fn window() -> Box2 {
    Box2::new(point(0.0, 0.0), point(10.0, 10.0))
}
mod polygon_orientation {
    use super::*;
    #[test]
    fn detects_winding_and_degeneracy() {
        assert_eq!(orientation(&square(2.0)), Orientation::CounterClockwise);
        assert_eq!(orientation(&square(2.0).reversed()), Orientation::Clockwise);
        assert_eq!(
            orientation(&polygon(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)])),
            Orientation::Degenerate
        );
        assert_eq!(
            orientation(&polygon(&[(0.0, 0.0), (1.0, 0.0)])),
            Orientation::Degenerate
        );
        let mut open = square(2.0);
        open.closed = false;
        assert_eq!(orientation(&open), Orientation::Degenerate);
    }
}
mod polygon_convexity {
    use super::*;
    #[test]
    fn convex_concave_and_collinear_vertices() {
        assert!(is_convex(&square(2.0)));
        assert!(is_convex(&square(2.0).reversed()));
        assert!(!is_convex(&l_shape()));
        assert!(is_convex(&polygon(&[
            (0.0, 0.0),
            (1.0, 0.0),
            (2.0, 0.0),
            (2.0, 2.0),
            (0.0, 2.0)
        ])));
        assert!(!is_convex(&polygon(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)])));
    }
}
fn is_simple_exhaustively(polygon: &Polyline2) -> bool {
    let count = polygon.vertices.len();
    if !polygon.closed || count < 3 {
        return false;
    }
    for index in 0..count {
        let first = polygon.segment(index);
        if first.is_degenerate() {
            return false;
        }
        for other in index + 1..count {
            let hit = intersect(&first, &polygon.segment(other));
            if !hit.exists() {
                continue;
            }
            let adjacent = other == index + 1 || (index == 0 && other == count - 1);
            if !adjacent || hit.kind == IntersectionKind::Overlap {
                return false;
            }
        }
    }
    true
}
mod polygon_simplicity {
    use super::*;
    #[test]
    fn detects_self_intersection() {
        assert!(is_simple(&square(2.0)));
        assert!(is_simple(&l_shape()));
        assert!(!is_simple(&polygon(&[
            (0.0, 0.0),
            (2.0, 2.0),
            (2.0, 0.0),
            (0.0, 2.0)
        ])));
        assert!(!is_simple(&polygon(&[
            (0.0, 0.0),
            (0.0, 0.0),
            (2.0, 0.0),
            (0.0, 2.0)
        ])));
        assert!(!is_simple(&polygon(&[
            (0.0, 0.0),
            (4.0, 0.0),
            (2.0, 0.0),
            (2.0, 3.0)
        ])));
    }
    #[test]
    fn the_bounding_box_reject_agrees_with_testing_every_segment_pair() {
        let mut random = Random::new(31337);
        let mut cases = vec![
            square(10.0),
            l_shape(),
            polygon(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0)]),
            polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]),
            polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (5.0, 1e-8)]),
            polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (5.0, 1e-3)]),
        ];
        for _ in 0..60 {
            let vertices = (0..4)
                .map(|_| point(random.real(-10.0, 10.0), random.real(-10.0, 10.0)))
                .collect();
            cases.push(Polyline2::new(vertices, true));
        }
        let mut simple = 0;
        let mut not_simple = 0;
        for case in &cases {
            let culled = is_simple(case);
            assert_eq!(culled, is_simple_exhaustively(case));
            if culled {
                simple += 1;
            } else {
                not_simple += 1;
            }
        }
        assert!(simple > 5);
        assert!(not_simple > 5);
    }
    #[test]
    fn edges_that_pass_within_the_geometric_tolerance_still_count() {
        assert!(!is_simple(&polygon(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (5.0, 1e-8)
        ])));
        assert!(is_simple(&polygon(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (5.0, 1e-3)
        ])));
    }
}
mod polygon_convex_hull {
    use super::*;
    #[test]
    fn drops_interior_collinear_and_duplicate_points() {
        let hull = convex_hull(
            polygon(&[
                (0.0, 0.0),
                (4.0, 0.0),
                (4.0, 4.0),
                (0.0, 4.0),
                (2.0, 2.0),
                (2.0, 0.0),
                (4.0, 4.0),
                (1.0, 3.0),
            ])
            .vertices,
        );
        assert_eq!(hull.len(), 4);
        let hull = Polyline2::new(hull, true);
        assert_eq!(orientation(&hull), Orientation::CounterClockwise);
        double_eq(hull.area(), 16.0);
        assert!(convex_hull(vec![]).is_empty());
        assert_eq!(convex_hull(vec![point(1.0, 1.0), point(1.0, 1.0)]).len(), 1);
        assert_eq!(
            convex_hull(vec![point(0.0, 0.0), point(1.0, 1.0), point(2.0, 2.0)]).len(),
            2
        );
    }
    #[test]
    fn contains_every_input_point() {
        let mut random = Random::default();
        let points: Vec<_> = (0..300)
            .map(|_| point(random.real(-100.0, 100.0), random.real(-100.0, 100.0)))
            .collect();
        let hull = Polyline2::new(convex_hull(points.clone()), true);
        assert!(is_convex(&hull));
        for point in points {
            assert_ne!(hull.classify(point), Containment::Outside);
        }
    }
}
mod polygon_simplify {
    use super::*;
    #[test]
    fn removes_points_within_tolerance_only() {
        let noisy = Polyline2::new(
            vec![
                point(0.0, 0.0),
                point(1.0, 0.01),
                point(2.0, -0.01),
                point(3.0, 0.0),
                point(3.0, 3.0),
            ],
            false,
        );
        let simplified = simplify(&noisy, 0.05);
        assert_eq!(simplified.vertices.len(), 3);
        assert_eq!(simplified.vertices[0], point(0.0, 0.0));
        assert_eq!(simplified.vertices[1], point(3.0, 0.0));
        assert_eq!(simplified.vertices[2], point(3.0, 3.0));
        assert_eq!(simplify(&noisy, 0.001).vertices.len(), 5);
        assert_eq!(simplify(&noisy, 0.0), noisy);
        assert_eq!(simplify(&noisy, -1.0), noisy);
    }
    #[test]
    fn closed_ring_stays_closed() {
        let mut ring = square(10.0);
        ring.vertices.insert(1, point(5.0, 0.001));
        let simplified = simplify(&ring, 0.01);
        assert!(simplified.closed);
        assert_eq!(simplified.vertices.len(), 4);
        near(simplified.area(), 100.0, 1e-9);
    }
}
mod polygon_clip_segment {
    use super::*;
    #[test]
    fn liang_barsky() {
        let bounds = window();
        let through = clip(&Segment2::new(point(-5.0, 5.0), point(15.0, 5.0)), &bounds).unwrap();
        assert_eq!(through, Segment2::new(point(0.0, 5.0), point(10.0, 5.0)));
        let inside = clip(&Segment2::new(point(1.0, 1.0), point(2.0, 2.0)), &bounds).unwrap();
        assert_eq!(inside, Segment2::new(point(1.0, 1.0), point(2.0, 2.0)));
        assert!(
            clip(
                &Segment2::new(point(-5.0, -5.0), point(-1.0, 20.0)),
                &bounds
            )
            .is_none()
        );
        assert!(
            clip(
                &Segment2::new(point(-5.0, 11.0), point(15.0, 11.0)),
                &bounds
            )
            .is_none()
        );
        assert!(
            clip(
                &Segment2::new(point(-5.0, 10.0), point(15.0, 10.0)),
                &bounds
            )
            .is_some()
        );
        assert!(
            clip(
                &Segment2::new(point(0.0, 0.0), point(1.0, 1.0)),
                &Box2::default()
            )
            .is_none()
        );
        let diagonal = clip(
            &Segment2::new(point(-5.0, -5.0), point(15.0, 15.0)),
            &bounds,
        )
        .unwrap();
        assert!(vec2_eq(diagonal.start, point(0.0, 0.0)));
        assert!(vec2_eq(diagonal.end, point(10.0, 10.0)));
    }
}
mod polygon_clip_polyline {
    use super::*;
    #[test]
    fn a_polyline_wholly_inside_comes_back_as_one_run_equal_to_it() {
        let line = vec![point(0.1, 0.2), point(3.3, 0.7), point(9.9, 9.7)];
        let mut runs = PolylineRuns::default();
        clip_polyline(&line, false, &window(), &mut runs);
        assert_eq!(runs.size(), 1);
        assert_eq!(run(&runs, 0), line);
    }
    #[test]
    fn a_line_that_leaves_and_comes_back_is_two_runs_with_crossings_on_the_boundary() {
        let line = vec![
            point(-5.0, 5.0),
            point(5.0, 5.0),
            point(5.0, 15.0),
            point(8.0, 15.0),
            point(8.0, 5.0),
            point(15.0, 5.0),
        ];
        let mut runs = PolylineRuns::default();
        clip_polyline(&line, false, &window(), &mut runs);
        assert_eq!(runs.size(), 2);
        assert_eq!(
            run(&runs, 0),
            [point(0.0, 5.0), point(5.0, 5.0), point(5.0, 10.0)]
        );
        assert_eq!(
            run(&runs, 1),
            [point(8.0, 10.0), point(8.0, 5.0), point(10.0, 5.0)]
        );
    }
    #[test]
    fn kept_whole_a_segment_that_meets_the_box_keeps_both_its_vertices() {
        let line = vec![
            point(-5.0, 5.0),
            point(5.0, 5.0),
            point(5.0, 15.0),
            point(8.0, 15.0),
            point(8.0, 5.0),
            point(15.0, 5.0),
        ];
        let mut runs = PolylineRuns::default();
        clip_polyline_with_mode(
            &line,
            false,
            &window(),
            &mut runs,
            PolylineClip::WholeSegments,
        );
        assert_eq!(runs.size(), 2);
        assert_eq!(
            run(&runs, 0),
            [point(-5.0, 5.0), point(5.0, 5.0), point(5.0, 15.0)]
        );
        assert_eq!(
            run(&runs, 1),
            [point(8.0, 15.0), point(8.0, 5.0), point(15.0, 5.0)]
        );
        clip_polyline_with_mode(
            &[point(-10.0, 5.0), point(20.0, 5.0), point(20.0, 40.0)],
            false,
            &window(),
            &mut runs,
            PolylineClip::WholeSegments,
        );
        assert_eq!(runs.size(), 1);
        assert_eq!(run(&runs, 0), [point(-10.0, 5.0), point(20.0, 5.0)]);
    }
    #[test]
    fn a_segment_crossing_the_box_with_both_ends_outside_is_a_run_of_its_two_crossings() {
        let mut runs = PolylineRuns::default();
        clip_polyline(
            &[point(-10.0, 5.0), point(20.0, 5.0)],
            false,
            &window(),
            &mut runs,
        );
        assert_eq!(runs.size(), 1);
        let only = run(&runs, 0);
        assert_eq!(only.len(), 2);
        assert!(vec2_eq(only[0], point(0.0, 5.0)));
        assert!(vec2_eq(only[1], point(10.0, 5.0)));
    }
    #[test]
    fn a_closed_ring_ends_its_last_run_at_the_first_vertex_without_joining_the_first_run() {
        let ring = vec![
            point(2.0, 2.0),
            point(12.0, 2.0),
            point(12.0, 8.0),
            point(2.0, 8.0),
        ];
        let mut runs = PolylineRuns::default();
        clip_polyline(&ring, true, &window(), &mut runs);
        assert_eq!(runs.size(), 2);
        assert_eq!(run(&runs, 0), [point(2.0, 2.0), point(10.0, 2.0)]);
        assert_eq!(
            run(&runs, 1),
            [point(10.0, 8.0), point(2.0, 8.0), point(2.0, 2.0)]
        );
    }
    #[test]
    fn nothing_outside_the_box_and_nothing_from_too_few_vertices_or_an_empty_box() {
        let mut runs = PolylineRuns {
            points: vec![point(1.0, 1.0)],
            ends: vec![1],
        };
        clip_polyline(
            &[point(-5.0, -5.0), point(-1.0, 20.0)],
            false,
            &window(),
            &mut runs,
        );
        assert!(runs.empty());
        assert!(runs.points.is_empty());
        clip_polyline(&[point(1.0, 1.0)], false, &window(), &mut runs);
        assert!(runs.empty());
        clip_polyline(
            &[point(1.0, 1.0), point(2.0, 2.0)],
            false,
            &Box2::default(),
            &mut runs,
        );
        assert!(runs.empty());
    }
    #[test]
    fn every_run_lies_in_the_box_and_every_vertex_inside_is_kept() {
        let mut random = Random::new(20260924);
        let bounds = Box2::new(point(-3.0, -2.0), point(4.0, 5.0));
        let mut runs = PolylineRuns::default();
        for trial in 0..200 {
            let count = random.integer(2, 31);
            let line: Vec<_> = (0..count)
                .map(|_| point(random.real(-8.0, 9.0), random.real(-7.0, 10.0)))
                .collect();
            clip_polyline(&line, trial % 2 == 1, &bounds, &mut runs);
            for index in 0..runs.size() {
                assert!(runs.ends[index] - runs.begin(index) >= 2);
            }
            for &sample in &runs.points {
                assert!(bounds.inflated(1e-9).contains(sample));
            }
            for vertex in &line {
                if vertex.x > bounds.min.x
                    && vertex.x < bounds.max.x
                    && vertex.y > bounds.min.y
                    && vertex.y < bounds.max.y
                {
                    assert!(runs.points.contains(vertex));
                }
            }
        }
    }
}
mod polygon_clip_polygon {
    use super::*;
    #[test]
    fn sutherland_hodgman() {
        let window = square(4.0);
        let mut subject = square(4.0);
        for vertex in &mut subject.vertices {
            *vertex += Vec2::new(2.0, 2.0);
        }
        let overlap = clip_polygon(&subject, &window).unwrap();
        double_eq(overlap.area(), 4.0);
        let overlap_cw = clip_polygon(&subject, &window.reversed()).unwrap();
        double_eq(overlap_cw.area(), 4.0);
        for vertex in &mut subject.vertices {
            *vertex += Vec2::new(100.0, 100.0);
        }
        assert!(clip_polygon(&subject, &window).unwrap().vertices.is_empty());
        assert_eq!(
            clip_polygon(&square(1.0), &l_shape()).unwrap_err().code,
            ErrorCode::InvalidGeometry
        );
    }
}
mod polygon_triangulate {
    use super::*;
    #[test]
    fn convex_and_concave_polygons_preserve_area() {
        let square_triangles = triangulate(&square(2.0)).unwrap();
        assert_eq!(square_triangles.len(), 2);
        double_eq(triangle_area_sum(&square(2.0), &square_triangles), 4.0);
        let l_triangles = triangulate(&l_shape()).unwrap();
        assert_eq!(l_triangles.len(), 4);
        double_eq(triangle_area_sum(&l_shape(), &l_triangles), 3.0);
        let clockwise = l_shape().reversed();
        let cw_triangles = triangulate(&clockwise).unwrap();
        double_eq(triangle_area_sum(&clockwise, &cw_triangles), 3.0);
    }
    #[test]
    fn skips_collinear_vertices_and_rejects_bad_input() {
        let with_midpoints = polygon(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let triangles = triangulate(&with_midpoints).unwrap();
        double_eq(triangle_area_sum(&with_midpoints, &triangles), 4.0);
        assert_eq!(
            triangulate(&polygon(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]))
                .unwrap_err()
                .code,
            ErrorCode::TriangulationFailure
        );
        let mut open = square(2.0);
        open.closed = false;
        assert!(triangulate(&open).is_err());
    }
    #[test]
    fn keeping_collinear_vertices_uses_every_corner_and_every_edge_of_the_polygon() {
        let square = polygon(&[(1.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 0.0)]);
        let skipped = triangulate(&square).unwrap();
        assert_eq!(skipped.len(), 2);
        assert!(!uses_vertex(&skipped, 0));
        double_eq(triangle_area_sum(&square, &skipped), 4.0);
        let kept = triangulate_with_collinear(&square, CollinearVertices::Keep).unwrap();
        assert_eq!(kept.len(), 3);
        for corner in 0..5 {
            assert!(uses_vertex(&kept, corner));
            assert!(has_directed_edge(&kept, corner, (corner + 1) % 5));
        }
        double_eq(kept_area_sum(&square, &kept), 4.0);
    }
    #[test]
    fn keeping_collinear_vertices_gives_n_take_two_triangles_for_a_run_of_them() {
        let run = polygon(&[
            (0.0, 0.0),
            (0.5, 0.0),
            (1.0, 0.0),
            (1.5, 0.0),
            (2.0, 0.0),
            (2.0, 2.0),
            (0.0, 2.0),
        ]);
        let kept = triangulate_with_collinear(&run, CollinearVertices::Keep).unwrap();
        assert_eq!(kept.len(), 5);
        for corner in 0..7 {
            assert!(uses_vertex(&kept, corner));
            assert!(has_directed_edge(&kept, corner, (corner + 1) % 7));
        }
        near(kept_area_sum(&run, &kept), 4.0, 1e-12);
    }
    #[test]
    fn keeping_collinear_vertices_triangulates_a_clockwise_polygon_counter_clockwise() {
        let run = polygon(&[
            (0.0, 0.0),
            (0.5, 0.0),
            (1.0, 0.0),
            (2.0, 0.0),
            (2.0, 2.0),
            (0.0, 2.0),
        ]);
        let reversed = run.reversed();
        let kept = triangulate_with_collinear(&reversed, CollinearVertices::Keep).unwrap();
        assert_eq!(kept.len(), 4);
        near(kept_area_sum(&reversed, &kept), 4.0, 1e-12);
        for corner in 0..6 {
            assert!(uses_vertex(&kept, corner));
        }
    }
    #[test]
    fn keeping_collinear_vertices_keeps_a_repeated_vertex_as_an_edge_of_no_length() {
        let repeated = polygon(&[(0.0, 0.0), (2.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let kept = triangulate_with_collinear(&repeated, CollinearVertices::Keep).unwrap();
        assert_eq!(kept.len(), 3);
        assert!(has_directed_edge(&kept, 1, 2));
        near(kept_area_sum(&repeated, &kept), 4.0, 1e-12);
        let skipped = triangulate(&repeated).unwrap();
        assert_eq!(skipped.len(), 2);
        assert!(!has_directed_edge(&skipped, 1, 2));
        near(triangle_area_sum(&repeated, &skipped), 4.0, 1e-12);
    }
    #[test]
    fn keeping_collinear_vertices_agrees_with_the_default_where_no_corner_is_straight() {
        let tee = polygon(&[
            (-0.6, -0.4),
            (0.6, -0.4),
            (0.6, 0.0),
            (0.125, 0.0),
            (0.125, 2.0),
            (-0.125, 2.0),
            (-0.125, 0.0),
            (-0.6, 0.0),
        ]);
        let skipped = triangulate(&tee).unwrap();
        let kept = triangulate_with_collinear(&tee, CollinearVertices::Keep).unwrap();
        assert_eq!(skipped, kept);
        assert_eq!(kept.len(), 6);
        near(triangle_area_sum(&tee, &kept), 0.98, 1e-12);
    }
    #[test]
    fn either_mode_refuses_what_there_is_nothing_to_fill() {
        let mut open = square(2.0);
        open.closed = false;
        let nothing = vec![
            open,
            polygon(&[]),
            polygon(&[(0.0, 0.0), (1.0, 1.0)]),
            polygon(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]),
            polygon(&[(1.0, 1.0), (1.0, 1.0), (1.0, 1.0)]),
            polygon(&[(0.0, 0.0), (1.0, 1.0), (1.0, 0.0), (0.0, 1.0)]),
            polygon(&[(0.0, 0.0), (2.0, 0.0), (f64::NAN, 2.0), (0.0, 2.0)]),
            polygon(&[(0.0, 0.0), (2.0, 0.0), (f64::INFINITY, 2.0), (0.0, 2.0)]),
        ];
        for polygon in nothing {
            for mode in [CollinearVertices::Skip, CollinearVertices::Keep] {
                assert_eq!(
                    triangulate_with_collinear(&polygon, mode).unwrap_err().code,
                    ErrorCode::TriangulationFailure
                );
            }
        }
    }
    #[test]
    fn random_polygons_with_corners_of_no_turn_keep_every_corner_and_preserve_area() {
        let mut random = Random::default();
        let mut with_straight_corners = 0;
        for _ in 0..200 {
            let count = random.integer(3, 30);
            let mut star = Polyline2::new(Vec::new(), true);
            for index in 0..count {
                let angle = TWO_PI * (index as f64 + random.real(0.1, 0.9)) / count as f64;
                star.vertices
                    .push(point(angle.cos(), angle.sin()) * random.real(5.0, 50.0));
            }
            let mut polygon = Polyline2::new(Vec::new(), true);
            for index in 0..star.vertices.len() {
                let from = star.vertices[index];
                let to = star.vertices[(index + 1) % star.vertices.len()];
                polygon.vertices.push(from);
                if random.integer(0, 2) == 0 {
                    polygon.vertices.push((from + to) * 0.5);
                }
            }
            if polygon.vertices.len() > star.vertices.len() {
                with_straight_corners += 1;
            }
            let kept = triangulate_with_collinear(&polygon, CollinearVertices::Keep).unwrap();
            let corners = polygon.vertices.len();
            assert_eq!(kept.len(), corners - 2);
            for corner in 0..corners {
                assert!(has_directed_edge(&kept, corner, (corner + 1) % corners));
            }
            near(
                kept_area_sum(&polygon, &kept),
                polygon.area(),
                1e-9 * polygon.area(),
            );
        }
        assert!(with_straight_corners > 150);
    }
    #[test]
    fn random_star_polygons_preserve_area() {
        let mut random = Random::default();
        for _ in 0..200 {
            let count = random.integer(3, 40);
            let mut polygon = Polyline2::new(Vec::new(), true);
            for index in 0..count {
                let angle = TWO_PI * (index as f64 + random.real(0.1, 0.9)) / count as f64;
                polygon
                    .vertices
                    .push(point(angle.cos(), angle.sin()) * random.real(5.0, 50.0));
            }
            let triangles = triangulate(&polygon).unwrap();
            near(
                triangle_area_sum(&polygon, &triangles),
                polygon.area(),
                1e-9 * polygon.area(),
            );
        }
    }
}
mod hatching {
    use super::*;
    #[test]
    fn a_horizontal_family_across_a_square_is_worked_out_exactly() {
        let lines = hatch_lines(&square(10.0), 0.0, 2.0).unwrap();
        assert_eq!(lines.len(), 5);
        for (index, segment) in lines.iter().enumerate() {
            let height = 2.0 * index as f64;
            assert_eq!(segment.start.x, 0.0);
            assert_eq!(segment.end.x, 10.0);
            assert_eq!(segment.start.y, height);
            assert_eq!(segment.end.y, height);
        }
    }
    #[test]
    fn a_concave_boundary_leaves_its_notch_unfilled() {
        let boundary = polygon(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (7.0, 10.0),
            (7.0, 3.0),
            (3.0, 3.0),
            (3.0, 10.0),
            (0.0, 10.0),
        ]);
        let lines = hatch_lines(&boundary, 0.0, 5.0).unwrap();
        let at_five: Vec<_> = lines
            .iter()
            .filter(|segment| segment.start.y == 5.0)
            .collect();
        assert_eq!(at_five.len(), 2);
        assert_eq!(at_five[0].start.x, 0.0);
        assert_eq!(at_five[0].end.x, 3.0);
        assert_eq!(at_five[1].start.x, 7.0);
        assert_eq!(at_five[1].end.x, 10.0);
    }
    #[test]
    fn the_family_is_anchored_to_the_world_so_adjacent_areas_line_up() {
        let left = square(10.0);
        let right = polygon(&[(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0)]);
        let angle = 1.0_f64.atan();
        let spacing = 1.5;
        let left_lines = hatch_lines(&left, angle, spacing).unwrap();
        let right_lines = hatch_lines(&right, angle, spacing).unwrap();
        assert!(!left_lines.is_empty());
        assert!(!right_lines.is_empty());
        let normal_east = -angle.sin();
        let normal_north = angle.cos();
        let mut checked = 0;
        for set in [&left_lines, &right_lines] {
            for segment in set {
                let across = segment.start.x * normal_east + segment.start.y * normal_north;
                let index = across / spacing;
                near(index, index.round(), 1e-9);
                checked += 1;
            }
        }
        assert!(checked > 10);
    }
    #[test]
    fn rotating_the_family_by_a_half_turn_hatches_the_same_area() {
        let blob = polygon(&[(1.0, 1.0), (9.0, 2.0), (11.0, 7.0), (4.0, 9.0), (0.0, 5.0)]);
        let spacing = 0.7;
        let total = |angle| {
            let lines = hatch_lines(&blob, angle, spacing).unwrap();
            let mut sum = 0.0;
            for segment in lines {
                sum += segment.length();
            }
            sum
        };
        let forward = total(0.6);
        let backward = total(0.6 + (-1.0_f64).acos());
        assert!(forward > 1.0);
        near(forward, backward, 1e-9);
    }
    #[test]
    fn rejects_boundaries_and_spacings_it_cannot_hatch() {
        let mut open = square(10.0);
        open.closed = false;
        assert_eq!(
            hatch_lines(&open, 0.0, 1.0).unwrap_err().code,
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            hatch_lines(&polygon(&[(0.0, 0.0), (1.0, 1.0)]), 0.0, 1.0)
                .unwrap_err()
                .code,
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            hatch_lines(
                &polygon(&[(0.0, 0.0), (f64::NAN, 0.0), (1.0, 1.0)]),
                0.0,
                1.0
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            hatch_lines(&square(10.0), 0.0, 0.0).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            hatch_lines(&square(10.0), 0.0, -1.0).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            hatch_lines(&square(10.0), f64::INFINITY, 1.0)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }
    #[test]
    fn a_spacing_too_fine_for_the_boundary_is_refused_with_both_numbers() {
        let error = hatch_lines(&square(10.0), 0.0, 1e-6).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        let message = error.describe();
        assert!(message.contains("10000001"));
        assert!(message.contains(&format!("{MAX_HATCH_LINES}")));
    }
    #[test]
    fn a_boundary_that_falls_between_two_lines_hatches_to_nothing() {
        let sliver = polygon(&[(0.0, 10.1), (10.0, 10.1), (10.0, 10.4), (0.0, 10.4)]);
        assert!(hatch_lines(&sliver, 0.0, 5.0).unwrap().is_empty());
    }
}
