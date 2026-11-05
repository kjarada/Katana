use crate::support::{PROPERTY_ITERATIONS, Random, near, vec2_eq};
use katana_geometry::intersection::*;
use katana_geometry::{Arc2, Circle2, Line2, Point2, Segment2, Vec2};
use katana_math::numerics::{HALF_PI, PI};
fn point(east: f64, north: f64) -> Point2 {
    Point2::new(east, north)
}
fn segment(first: (f64, f64), second: (f64, f64)) -> Segment2 {
    Segment2::new(point(first.0, first.1), point(second.0, second.1))
}
fn line(origin: (f64, f64), direction: (f64, f64)) -> Line2 {
    Line2 {
        origin: point(origin.0, origin.1),
        direction: Vec2::new(direction.0, direction.1),
    }
}
fn single(result: IntersectionResult, expected: Point2) {
    assert_eq!(result.kind, IntersectionKind::Points);
    assert_eq!(result.count, 1);
    assert!(vec2_eq(result.points[0], expected));
}
mod intersect_lines {
    use super::*;
    #[test]
    fn crossing_parallel_and_coincident() {
        let horizontal = line((0.0, 1.0), (1.0, 0.0));
        single(
            intersect(&horizontal, &line((3.0, -5.0), (0.0, 2.0))),
            point(3.0, 1.0),
        );
        assert_eq!(
            intersect(&horizontal, &line((0.0, 2.0), (-4.0, 0.0))).kind,
            IntersectionKind::None
        );
        assert_eq!(
            intersect(&horizontal, &line((50.0, 1.0), (-4.0, 0.0))).kind,
            IntersectionKind::Overlap
        );
    }
    #[test]
    fn nearly_parallel_lines_are_treated_as_parallel_below_angular_tolerance() {
        let base = line((0.0, 0.0), (1.0, 0.0));
        assert_eq!(
            intersect(&base, &line((0.0, 1.0), (1.0, 1e-12))).kind,
            IntersectionKind::None
        );
        let shallow = intersect(&base, &line((0.0, 1.0), (1.0, -1e-6)));
        assert_eq!(shallow.kind, IntersectionKind::Points);
        near(shallow.points[0].x, 1e6, 1e-3);
        near(shallow.points[0].y, 0.0, 1e-9);
    }
}
mod intersect_segments {
    use super::*;
    #[test]
    fn proper_crossing() {
        single(
            intersect(
                &segment((0.0, 0.0), (10.0, 10.0)),
                &segment((0.0, 10.0), (10.0, 0.0)),
            ),
            point(5.0, 5.0),
        );
    }
    #[test]
    fn disjoint_even_though_supporting_lines_cross() {
        assert!(
            !intersect(
                &segment((0.0, 0.0), (1.0, 1.0)),
                &segment((0.0, 10.0), (10.0, 0.0))
            )
            .exists()
        );
    }
    #[test]
    fn endpoint_touch_and_t_junction() {
        single(
            intersect(
                &segment((0.0, 0.0), (5.0, 0.0)),
                &segment((5.0, 0.0), (5.0, 5.0)),
            ),
            point(5.0, 0.0),
        );
        single(
            intersect(
                &segment((0.0, 0.0), (10.0, 0.0)),
                &segment((4.0, 0.0), (4.0, 7.0)),
            ),
            point(4.0, 0.0),
        );
    }
    #[test]
    fn parallel_separate() {
        assert!(
            !intersect(
                &segment((0.0, 0.0), (10.0, 0.0)),
                &segment((0.0, 1.0), (10.0, 1.0))
            )
            .exists()
        );
    }
    #[test]
    fn collinear_overlap_touch_and_gap() {
        let base = segment((0.0, 0.0), (10.0, 0.0));
        let shared = intersect(&base, &segment((15.0, 0.0), (6.0, 0.0)));
        assert_eq!(shared.kind, IntersectionKind::Overlap);
        assert_eq!(shared.count, 2);
        assert!(vec2_eq(shared.points[0], point(6.0, 0.0)));
        assert!(vec2_eq(shared.points[1], point(10.0, 0.0)));
        let contained = intersect(&base, &segment((2.0, 0.0), (3.0, 0.0)));
        assert_eq!(contained.kind, IntersectionKind::Overlap);
        assert!(vec2_eq(contained.points[0], point(2.0, 0.0)));
        assert!(vec2_eq(contained.points[1], point(3.0, 0.0)));
        single(
            intersect(&base, &segment((10.0, 0.0), (20.0, 0.0))),
            point(10.0, 0.0),
        );
        assert!(!intersect(&base, &segment((10.5, 0.0), (20.0, 0.0))).exists());
    }
    #[test]
    fn zero_length_segments() {
        let base = segment((0.0, 0.0), (10.0, 0.0));
        single(
            intersect(&base, &segment((3.0, 0.0), (3.0, 0.0))),
            point(3.0, 0.0),
        );
        single(
            intersect(&segment((3.0, 0.0), (3.0, 0.0)), &base),
            point(3.0, 0.0),
        );
        assert!(!intersect(&base, &segment((3.0, 1.0), (3.0, 1.0))).exists());
        single(
            intersect(
                &segment((1.0, 1.0), (1.0, 1.0)),
                &segment((1.0, 1.0), (1.0, 1.0)),
            ),
            point(1.0, 1.0),
        );
        assert!(
            !intersect(
                &segment((1.0, 1.0), (1.0, 1.0)),
                &segment((2.0, 2.0), (2.0, 2.0))
            )
            .exists()
        );
    }
    #[test]
    fn works_at_projected_coordinate_magnitudes() {
        let shift = Vec2::new(500000.0, 5000000.0);
        single(
            intersect(
                &Segment2::new(point(0.0, 0.0) + shift, point(10.0, 10.0) + shift),
                &Segment2::new(point(0.0, 10.0) + shift, point(10.0, 0.0) + shift),
            ),
            point(5.0, 5.0) + shift,
        );
    }
}
mod intersect_line_segment {
    use super::*;
    #[test]
    fn hit_miss_and_collinear() {
        let vertical = line((2.0, 0.0), (0.0, 1.0));
        single(
            intersect(&vertical, &segment((0.0, 5.0), (4.0, 5.0))),
            point(2.0, 5.0),
        );
        assert!(!intersect(&vertical, &segment((3.0, 5.0), (4.0, 5.0))).exists());
        assert_eq!(
            intersect(&vertical, &segment((2.0, 1.0), (2.0, 9.0))).kind,
            IntersectionKind::Overlap
        );
    }
}
mod intersect_line_circle {
    use super::*;
    #[test]
    fn secant_tangent_and_miss() {
        let circle = Circle2::new(point(0.0, 0.0), 5.0);
        let secant = intersect(&line((-10.0, 3.0), (1.0, 0.0)), &circle);
        assert_eq!(secant.count, 2);
        assert!(!secant.tangent);
        assert!(vec2_eq(secant.points[0], point(-4.0, 3.0)));
        assert!(vec2_eq(secant.points[1], point(4.0, 3.0)));
        let tangent = intersect(&line((-10.0, 5.0), (1.0, 0.0)), &circle);
        assert_eq!(tangent.count, 1);
        assert!(tangent.tangent);
        assert!(vec2_eq(tangent.points[0], point(0.0, 5.0)));
        assert!(!intersect(&line((-10.0, 5.001), (1.0, 0.0)), &circle).exists());
    }
}
mod intersect_segment_circle {
    use super::*;
    #[test]
    fn only_points_within_the_segment_count() {
        let circle = Circle2::new(point(0.0, 0.0), 5.0);
        single(
            intersect(&segment((0.0, 0.0), (10.0, 0.0)), &circle),
            point(5.0, 0.0),
        );
        assert!(!intersect(&segment((0.0, 0.0), (1.0, 0.0)), &circle).exists());
        assert_eq!(
            intersect(&segment((-10.0, 0.0), (10.0, 0.0)), &circle).count,
            2
        );
    }
}
mod intersect_circles {
    use super::*;
    #[test]
    fn two_points_tangent_separate_nested_and_coincident() {
        let first = Circle2::new(point(0.0, 0.0), 5.0);
        let crossing = intersect(&first, &Circle2::new(point(8.0, 0.0), 5.0));
        assert_eq!(crossing.count, 2);
        assert!(vec2_eq(crossing.points[0], point(4.0, -3.0)));
        assert!(vec2_eq(crossing.points[1], point(4.0, 3.0)));
        let external = intersect(&first, &Circle2::new(point(8.0, 0.0), 3.0));
        assert_eq!(external.count, 1);
        assert!(external.tangent);
        assert!(vec2_eq(external.points[0], point(5.0, 0.0)));
        let internal = intersect(&first, &Circle2::new(point(3.0, 0.0), 2.0));
        assert_eq!(internal.count, 1);
        assert!(internal.tangent);
        assert!(vec2_eq(internal.points[0], point(5.0, 0.0)));
        let swapped = intersect(&Circle2::new(point(3.0, 0.0), 2.0), &first);
        assert_eq!(swapped.count, 1);
        assert!(vec2_eq(swapped.points[0], point(5.0, 0.0)));
        assert!(!intersect(&first, &Circle2::new(point(20.0, 0.0), 3.0)).exists());
        assert!(!intersect(&first, &Circle2::new(point(1.0, 0.0), 2.0)).exists());
        assert!(!intersect(&first, &Circle2::new(point(0.0, 0.0), 2.0)).exists());
        assert_eq!(intersect(&first, &first).kind, IntersectionKind::Overlap);
    }
}
mod intersect_arcs {
    use super::*;
    #[test]
    fn segment_against_arc_keeps_only_points_on_the_sweep() {
        let upper = Arc2::new(point(0.0, 0.0), 5.0, 0.0, PI);
        single(
            intersect(&segment((0.0, -10.0), (0.0, 10.0)), &upper),
            point(0.0, 5.0),
        );
        assert!(!intersect(&segment((0.0, -10.0), (0.0, -1.0)), &upper).exists());
        assert_eq!(intersect(&line((-10.0, 3.0), (1.0, 0.0)), &upper).count, 2);
        single(
            intersect(&segment((5.0, -1.0), (5.0, 1.0)), &upper),
            point(5.0, 0.0),
        );
    }
    #[test]
    fn arc_against_arc_and_circle() {
        let upper = Arc2::new(point(0.0, 0.0), 5.0, 0.0, PI);
        let other = Arc2::new(point(8.0, 0.0), 5.0, HALF_PI, PI);
        single(intersect(&upper, &other), point(4.0, 3.0));
        assert_eq!(
            intersect(&Circle2::new(point(8.0, 0.0), 5.0), &upper).count,
            1
        );
    }
    #[test]
    fn arcs_on_the_same_circle() {
        let first = Arc2::new(point(0.0, 0.0), 5.0, 0.0, HALF_PI);
        assert_eq!(intersect(&first, &first).kind, IntersectionKind::Overlap);
        assert_eq!(
            intersect(&first, &first.reversed()).kind,
            IntersectionKind::Overlap
        );
        assert_eq!(
            intersect(&first, &Arc2::new(point(0.0, 0.0), 5.0, 0.25 * PI, HALF_PI)).kind,
            IntersectionKind::Overlap
        );
        single(
            intersect(&first, &Arc2::new(point(0.0, 0.0), 5.0, HALF_PI, HALF_PI)),
            point(0.0, 5.0),
        );
        let complementary = intersect(&first, &Arc2::new(point(0.0, 0.0), 5.0, HALF_PI, 1.5 * PI));
        assert_eq!(complementary.kind, IntersectionKind::Points);
        assert_eq!(complementary.count, 2);
        assert!(!intersect(&first, &Arc2::new(point(0.0, 0.0), 5.0, PI, HALF_PI)).exists());
    }
}
mod intersect_properties {
    use super::*;
    #[test]
    fn reported_points_lie_on_both_operands() {
        let mut random = Random::default();
        let mut hits = 0;
        for _ in 0..PROPERTY_ITERATIONS {
            let segment = Segment2::new(
                point(random.real(-20.0, 20.0), random.real(-20.0, 20.0)),
                point(random.real(-20.0, 20.0), random.real(-20.0, 20.0)),
            );
            let circle = Circle2::new(
                point(random.real(-10.0, 10.0), random.real(-10.0, 10.0)),
                random.real(1.0, 15.0),
            );
            let result = intersect(&segment, &circle);
            for &sample in &result.points[..result.count] {
                hits += 1;
                assert!(segment.distance_to(sample) <= 1e-9);
                assert!(circle.distance_to(sample) <= 1e-9);
            }
        }
        assert!(hits > 100);
    }
    #[test]
    fn segment_intersection_is_symmetric() {
        let mut random = Random::default();
        let mut crossings = 0;
        for _ in 0..PROPERTY_ITERATIONS {
            let first = Segment2::new(
                point(random.real(-20.0, 20.0), random.real(-20.0, 20.0)),
                point(random.real(-20.0, 20.0), random.real(-20.0, 20.0)),
            );
            let second = Segment2::new(
                point(random.real(-20.0, 20.0), random.real(-20.0, 20.0)),
                point(random.real(-20.0, 20.0), random.real(-20.0, 20.0)),
            );
            let forward = intersect(&first, &second);
            let back = intersect(&second, &first);
            assert_eq!(forward.kind, back.kind);
            assert_eq!(forward.count, back.count);
            if forward.kind == IntersectionKind::Points {
                crossings += 1;
                assert!(vec2_eq(forward.points[0], back.points[0]));
            }
        }
        assert!(crossings > 50);
    }
    #[test]
    fn collinear_overlap_is_symmetric_as_a_set_of_endpoints() {
        let mut random = Random::default();
        let mut overlaps = 0;
        let mut touches = 0;
        let mut disjoint = 0;
        for index in 0..PROPERTY_ITERATIONS {
            let origin = point(random.real(-20.0, 20.0), random.real(-20.0, 20.0));
            let angle = random.real(0.0, 6.283185307179586);
            let direction = Vec2::new(angle.cos(), angle.sin());
            let first_start = random.real(-10.0, 10.0);
            let first_end = first_start + random.real(0.5, 8.0);
            let second_start = random.real(-12.0, 12.0);
            let second_end = second_start + random.real(0.5, 8.0);
            let first = Segment2::new(
                origin + direction * first_start,
                origin + direction * first_end,
            );
            let second = if index % 3 == 0 {
                Segment2::new(
                    origin + direction * second_end,
                    origin + direction * second_start,
                )
            } else {
                Segment2::new(
                    origin + direction * second_start,
                    origin + direction * second_end,
                )
            };
            let forward = intersect(&first, &second);
            let back = intersect(&second, &first);
            assert_eq!(forward.kind, back.kind);
            assert_eq!(forward.count, back.count);
            if forward.kind == IntersectionKind::Overlap {
                overlaps += 1;
                assert_eq!(forward.count, 2);
                let same = vec2_eq(forward.points[0], back.points[0])
                    && vec2_eq(forward.points[1], back.points[1]);
                let reversed = vec2_eq(forward.points[0], back.points[1])
                    && vec2_eq(forward.points[1], back.points[0]);
                assert!(same || reversed);
                near(
                    forward.points[0].distance_to(forward.points[1]),
                    back.points[0].distance_to(back.points[1]),
                    1e-9,
                );
            } else if forward.kind == IntersectionKind::Points {
                touches += 1;
                assert!(vec2_eq(forward.points[0], back.points[0]));
            } else {
                disjoint += 1;
            }
        }
        assert!(overlaps > 100);
        assert!(disjoint > 0);
        let _ = touches;
    }
    #[test]
    fn collinear_overlap_reports_the_shared_portion_exactly() {
        let first = segment((0.0, 0.0), (10.0, 0.0));
        let second = segment((4.0, 0.0), (20.0, 0.0));
        let forward = intersect(&first, &second);
        assert_eq!(forward.kind, IntersectionKind::Overlap);
        assert_eq!(forward.count, 2);
        near(forward.points[0].x, 4.0, 1e-12);
        near(forward.points[1].x, 10.0, 1e-12);
        let back = intersect(&second, &first);
        assert_eq!(back.kind, IntersectionKind::Overlap);
        near(back.points[0].x, 4.0, 1e-12);
        near(back.points[1].x, 10.0, 1e-12);
        let reversed = intersect(&segment((20.0, 0.0), (4.0, 0.0)), &first);
        assert_eq!(reversed.kind, IntersectionKind::Overlap);
        near(reversed.points[0].x, 10.0, 1e-12);
        near(reversed.points[1].x, 4.0, 1e-12);
        let touching = intersect(&first, &segment((10.0, 0.0), (30.0, 0.0)));
        assert_eq!(touching.kind, IntersectionKind::Points);
        assert_eq!(touching.count, 1);
        assert_eq!(
            intersect(&first, &segment((12.0, 0.0), (30.0, 0.0))).kind,
            IntersectionKind::None
        );
    }
}
