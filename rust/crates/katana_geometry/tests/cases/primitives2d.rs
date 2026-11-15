use crate::support::{PROPERTY_ITERATIONS, Random, double_eq, near, vec2_eq};
use katana_geometry::*;
use katana_math::numerics::{HALF_PI, PI, TWO_PI, nearly_equal};

mod geometry_line2 {
    use super::*;
    #[test]
    fn projection_and_signed_distance() {
        let line = Line2::through(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)).unwrap();
        assert_eq!(
            line.closest_point(Point2::new(3.0, 7.0)),
            Point2::new(3.0, 0.0)
        );
        double_eq(line.signed_distance_to(Point2::new(3.0, 7.0)), 7.0);
        double_eq(line.signed_distance_to(Point2::new(3.0, -2.0)), -2.0);
        double_eq(line.distance_to(Point2::new(-50.0, -2.0)), 2.0);
    }
    #[test]
    fn duplicate_points_do_not_define_a_line() {
        assert!(Line2::through(Point2::new(1.0, 1.0), Point2::new(1.0, 1.0)).is_none());
        assert!(Line2::through(Point2::new(1.0, 1.0), Point2::new(1.0 + 1e-9, 1.0)).is_none());
        assert!(Line2::through(Point2::new(1.0, 1.0), Point2::new(1.0 + 1e-6, 1.0)).is_some());
    }
}

mod geometry_segment2 {
    use super::*;
    #[test]
    fn length_midpoint_closest_point() {
        let segment = Segment2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        double_eq(segment.length(), 10.0);
        assert_eq!(segment.midpoint(), Point2::new(5.0, 0.0));
        assert_eq!(
            segment.closest_point(Point2::new(4.0, 3.0)),
            Point2::new(4.0, 0.0)
        );
        assert_eq!(
            segment.closest_point(Point2::new(-5.0, 3.0)),
            Point2::new(0.0, 0.0)
        );
        assert_eq!(
            segment.closest_point(Point2::new(15.0, 3.0)),
            Point2::new(10.0, 0.0)
        );
        double_eq(segment.distance_to(Point2::new(13.0, 4.0)), 5.0);
        assert_eq!(
            segment.bounding_box(),
            Box2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))
        );
    }
    #[test]
    fn zero_length_segment_behaves_as_a_point() {
        let point = Segment2::new(Point2::new(2.0, 3.0), Point2::new(2.0, 3.0));
        assert!(point.is_degenerate());
        double_eq(point.length(), 0.0);
        assert_eq!(
            point.closest_point(Point2::new(9.0, 9.0)),
            Point2::new(2.0, 3.0)
        );
        double_eq(point.distance_to(Point2::new(5.0, 7.0)), 5.0);
    }
}

mod geometry_circle2 {
    use super::*;
    #[test]
    fn measures_and_containment() {
        let circle = Circle2::new(Point2::new(1.0, 1.0), 2.0);
        double_eq(circle.area(), 4.0 * PI);
        double_eq(circle.perimeter(), 4.0 * PI);
        assert_eq!(circle.classify(Point2::new(1.0, 1.0)), Containment::Inside);
        assert_eq!(
            circle.classify(Point2::new(3.0, 1.0)),
            Containment::OnBoundary
        );
        assert_eq!(
            circle.classify(Point2::new(3.001, 1.0)),
            Containment::Outside
        );
        assert_eq!(
            circle.closest_point(Point2::new(11.0, 1.0)),
            Point2::new(3.0, 1.0)
        );
        assert_eq!(
            circle.closest_point(Point2::new(1.0, 1.0)),
            Point2::new(3.0, 1.0)
        );
        double_eq(circle.distance_to(Point2::new(1.0, 1.0)), 2.0);
        assert_eq!(
            circle.bounding_box(),
            Box2::new(Point2::new(-1.0, -1.0), Point2::new(3.0, 3.0))
        );
    }
    #[test]
    fn area_and_perimeter_are_distinguishable_from_each_other() {
        let circle = Circle2::new(Point2::new(-4.0, 7.0), 3.0);
        double_eq(circle.area(), 9.0 * PI);
        double_eq(circle.perimeter(), 6.0 * PI);
        assert_ne!(circle.area(), circle.perimeter());
        let twice = Circle2::new(circle.center, circle.radius * 2.0);
        double_eq(twice.area(), circle.area() * 4.0);
        double_eq(twice.perimeter(), circle.perimeter() * 2.0);
        let point = Circle2::new(Point2::new(0.0, 0.0), 0.0);
        double_eq(point.area(), 0.0);
        double_eq(point.perimeter(), 0.0);
    }
}

mod geometry_arc2 {
    use super::*;
    #[test]
    fn endpoints_length_and_direction() {
        let ccw = Arc2::new(Point2::new(0.0, 0.0), 2.0, 0.0, HALF_PI);
        assert!(vec2_eq(ccw.start_point(), Point2::new(2.0, 0.0)));
        assert!(vec2_eq(ccw.end_point(), Point2::new(0.0, 2.0)));
        double_eq(ccw.length(), PI);
        let cw = ccw.reversed();
        double_eq(cw.sweep, -HALF_PI);
        assert!(vec2_eq(cw.start_point(), ccw.end_point()));
        assert!(vec2_eq(cw.end_point(), ccw.start_point()));
        double_eq(cw.length(), ccw.length());
        assert!(vec2_eq(cw.midpoint(), ccw.midpoint()));
    }
    #[test]
    fn contains_angle_handles_wrap_and_clockwise_sweeps() {
        let across_zero = Arc2::new(Point2::new(0.0, 0.0), 1.0, 1.75 * PI, HALF_PI);
        assert!(across_zero.contains_angle(0.0));
        assert!(across_zero.contains_angle(TWO_PI));
        assert!(across_zero.contains_angle(-0.1));
        assert!(!across_zero.contains_angle(PI));
        let clockwise = Arc2::new(Point2::new(0.0, 0.0), 1.0, HALF_PI, -HALF_PI);
        assert!(clockwise.contains_angle(0.25 * PI));
        assert!(!clockwise.contains_angle(0.75 * PI));
        assert!(!clockwise.contains_angle(HALF_PI + 1e-6));
        assert!(clockwise.contains_angle_with_tolerance(HALF_PI + 1e-6, 1e-5));
    }
    #[test]
    fn closest_point_clamps_to_the_nearer_end() {
        let arc = Arc2::new(Point2::new(0.0, 0.0), 1.0, 0.0, HALF_PI);
        assert!(vec2_eq(
            arc.closest_point(Point2::new(5.0, 5.0)),
            arc.point_at(0.5)
        ));
        assert!(vec2_eq(
            arc.closest_point(Point2::new(3.0, -1.0)),
            arc.start_point()
        ));
        assert!(vec2_eq(
            arc.closest_point(Point2::new(-1.0, 3.0)),
            arc.end_point()
        ));
        assert!(vec2_eq(
            arc.closest_point(Point2::new(0.0, 0.0)),
            arc.start_point()
        ));
    }
    #[test]
    fn bounding_box_includes_cardinal_extremes() {
        let upper_half = Arc2::new(Point2::new(0.0, 0.0), 1.0, 0.0, PI);
        let bounds = upper_half.bounding_box();
        near(bounds.min.x, -1.0, 1e-15);
        near(bounds.max.x, 1.0, 1e-15);
        near(bounds.min.y, 0.0, 1e-15);
        near(bounds.max.y, 1.0, 1e-15);
    }
    #[test]
    fn through_three_points_respects_travel_direction() {
        let ccw = Arc2::through_points(
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 0.0),
        )
        .unwrap();
        assert!(vec2_eq(ccw.center, Point2::new(0.0, 0.0)));
        near(ccw.radius, 1.0, 1e-15);
        near(ccw.sweep, PI, 1e-15);
        let cw = Arc2::through_points(
            Point2::new(1.0, 0.0),
            Point2::new(0.0, -1.0),
            Point2::new(-1.0, 0.0),
        )
        .unwrap();
        near(cw.sweep, -PI, 1e-15);
        assert!(vec2_eq(cw.midpoint(), Point2::new(0.0, -1.0)));
        assert!(
            Arc2::through_points(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 1.0),
                Point2::new(2.0, 2.0)
            )
            .is_none()
        );
        assert!(
            Arc2::through_points(
                Point2::new(0.0, 0.0),
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 2.0)
            )
            .is_none()
        );
    }
}

fn l_shape() -> Polyline2 {
    Polyline2::new(
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(1.0, 1.0),
            Point2::new(1.0, 2.0),
            Point2::new(0.0, 2.0),
        ],
        true,
    )
}

mod geometry_polyline2 {
    use super::*;
    #[test]
    fn open_and_closed_lengths_and_segments() {
        let mut path = Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(3.0, 0.0),
                Point2::new(3.0, 4.0),
            ],
            false,
        );
        assert_eq!(path.segment_count(), 2);
        double_eq(path.length(), 7.0);
        double_eq(path.area(), 0.0);
        assert!(path.centroid().is_none());
        path.closed = true;
        assert_eq!(path.segment_count(), 3);
        double_eq(path.length(), 12.0);
        double_eq(path.area(), 6.0);
        assert!(path.signed_area() > 0.0);
        assert_eq!(Polyline2::default().segment_count(), 0);
        assert_eq!(
            Polyline2::new(vec![Point2::new(1.0, 1.0)], false).segment_count(),
            0
        );
        assert!(
            Polyline2::default()
                .closest_point(Point2::new(0.0, 0.0))
                .is_none()
        );
    }
    #[test]
    fn area_keeps_precision_at_projected_coordinates() {
        let origin = Point2::new(500000.0, 5000000.0);
        let parcel = Polyline2::new(
            vec![
                origin,
                origin + Vec2::new(10.0, 0.0),
                origin + Vec2::new(10.0, 10.0),
                origin + Vec2::new(0.0, 10.0),
            ],
            true,
        );
        double_eq(parcel.area(), 100.0);
        let centroid = parcel.centroid().unwrap();
        double_eq(centroid.x, 500005.0);
        double_eq(centroid.y, 5000005.0);
    }
    #[test]
    fn centroid_of_l_shape_is_area_weighted() {
        let polygon = l_shape();
        double_eq(polygon.area(), 3.0);
        let centroid = polygon.centroid().unwrap();
        near(centroid.x, 5.0 / 6.0, 1e-15);
        near(centroid.y, 5.0 / 6.0, 1e-15);
    }
    #[test]
    fn degenerate_polygons_have_no_area_or_centroid() {
        let collinear = Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 1.0),
                Point2::new(2.0, 2.0),
            ],
            true,
        );
        double_eq(collinear.area(), 0.0);
        assert!(collinear.centroid().is_none());
        let duplicates = Polyline2::new(vec![Point2::new(5.0, 5.0); 3], true);
        double_eq(duplicates.area(), 0.0);
        assert!(duplicates.centroid().is_none());
    }
    #[test]
    fn point_in_polygon_including_vertices_and_concavity() {
        let mut polygon = l_shape();
        assert_eq!(polygon.classify(Point2::new(0.5, 0.5)), Containment::Inside);
        assert_eq!(
            polygon.classify(Point2::new(1.5, 1.5)),
            Containment::Outside
        );
        assert_eq!(
            polygon.classify(Point2::new(2.0, 0.0)),
            Containment::OnBoundary
        );
        assert_eq!(
            polygon.classify(Point2::new(1.0, 1.5)),
            Containment::OnBoundary
        );
        assert_eq!(
            polygon.classify(Point2::new(-1.0, 1.0)),
            Containment::Outside
        );
        assert_eq!(polygon.classify(Point2::new(0.5, 1.0)), Containment::Inside);
        assert_eq!(
            polygon.classify(Point2::new(-0.5, 1.0)),
            Containment::Outside
        );
        polygon.closed = false;
        assert_eq!(
            polygon.classify(Point2::new(0.5, 0.5)),
            Containment::Outside
        );
    }
    #[test]
    fn removes_duplicate_vertices() {
        let noisy = Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1e-9),
                Point2::new(1.0, 1.0),
                Point2::new(0.0, 0.0),
            ],
            true,
        );
        let clean = noisy.without_duplicate_vertices();
        assert_eq!(clean.vertices.len(), 3);
        assert_eq!(clean.vertices[0], Point2::new(0.0, 0.0));
        assert_eq!(clean.vertices[1], Point2::new(1.0, 0.0));
        assert_eq!(clean.vertices[2], Point2::new(1.0, 1.0));
    }
}

mod geometry_rectangle2 {
    use super::*;
    #[test]
    fn measures_and_containment() {
        let rect = Rectangle2::from_corners(Point2::new(4.0, 3.0), Point2::new(0.0, 0.0));
        assert_eq!(rect.origin, Point2::new(0.0, 0.0));
        double_eq(rect.area(), 12.0);
        double_eq(rect.perimeter(), 14.0);
        assert_eq!(rect.classify(Point2::new(2.0, 1.0)), Containment::Inside);
        assert_eq!(
            rect.classify(Point2::new(4.0, 1.0)),
            Containment::OnBoundary
        );
        assert_eq!(rect.classify(Point2::new(5.0, 1.0)), Containment::Outside);
        assert_eq!(
            rect.closest_point(Point2::new(2.0, 0.5)),
            Point2::new(2.0, 0.0)
        );
        double_eq(rect.distance_to(Point2::new(7.0, 7.0)), 5.0);
        double_eq(rect.to_polyline().area(), 12.0);
    }
}

mod geometry_triangle2 {
    use super::*;
    #[test]
    fn measures_barycentric_and_circumcircle() {
        let triangle = Triangle2::new(
            Point2::new(0.0, 0.0),
            Point2::new(4.0, 0.0),
            Point2::new(0.0, 3.0),
        );
        double_eq(triangle.area(), 6.0);
        double_eq(triangle.perimeter(), 12.0);
        assert!(triangle.signed_area() > 0.0);
        assert!(Triangle2::new(triangle.a, triangle.c, triangle.b).signed_area() < 0.0);
        assert!(vec2_eq(triangle.centroid(), Point2::new(4.0 / 3.0, 1.0)));
        assert_eq!(
            triangle.classify(Point2::new(1.0, 1.0)),
            Containment::Inside
        );
        assert_eq!(
            triangle.classify(Point2::new(2.0, 0.0)),
            Containment::OnBoundary
        );
        assert_eq!(
            triangle.classify(Point2::new(4.0, 3.0)),
            Containment::Outside
        );
        let weights = triangle.barycentric(Point2::new(1.0, 1.0)).unwrap();
        near(weights[0] + weights[1] + weights[2], 1.0, 1e-15);
        near(weights[1], 0.25, 1e-15);
        near(weights[2], 1.0 / 3.0, 1e-15);
        let circle = triangle.circumcircle().unwrap();
        assert!(vec2_eq(circle.center, Point2::new(2.0, 1.5)));
        near(circle.radius, 2.5, 1e-15);
    }
    #[test]
    fn degenerate_triangles_are_rejected() {
        let collinear = Triangle2::new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(2.0, 2.0),
        );
        assert!(collinear.is_degenerate());
        assert!(collinear.barycentric(Point2::new(1.0, 1.0)).is_none());
        assert!(collinear.circumcircle().is_none());
        assert!(
            Triangle2::new(
                Point2::new(1.0, 1.0),
                Point2::new(1.0, 1.0),
                Point2::new(1.0, 1.0)
            )
            .is_degenerate()
        );
        assert!(
            Triangle2::new(
                Point2::new(0.0, 0.0),
                Point2::new(100.0, 0.0),
                Point2::new(50.0, 1e-8)
            )
            .is_degenerate()
        );
        assert!(
            !Triangle2::new(
                Point2::new(0.0, 0.0),
                Point2::new(100.0, 0.0),
                Point2::new(50.0, 1e-5)
            )
            .is_degenerate()
        );
    }
}

fn random_polygon(random: &mut Random, origin_scale: f64) -> Polyline2 {
    let center = Point2::new(
        random.real(-origin_scale, origin_scale),
        random.real(-origin_scale, origin_scale),
    );
    let count = random.integer(3, 12);
    let mut polygon = Polyline2::new(Vec::new(), true);
    for index in 0..count {
        let angle = TWO_PI * (index as f64 + random.real(0.1, 0.9)) / count as f64;
        let radius = random.real(5.0, 50.0);
        polygon
            .vertices
            .push(center + Vec2::new(angle.cos(), angle.sin()) * radius);
    }
    polygon
}

mod geometry_properties {
    use super::*;
    #[test]
    fn reversing_a_polygon_preserves_area_and_length() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let polygon = random_polygon(&mut random, 1e6);
            let reversed = polygon.reversed();
            double_eq(reversed.area(), polygon.area());
            double_eq(reversed.signed_area(), -polygon.signed_area());
            assert!(nearly_equal(reversed.length(), polygon.length(), 1e-14));
        }
    }
    #[test]
    fn area_and_centroid_are_translation_invariant() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let local = random_polygon(&mut random, 10.0);
            let shift = Vec2::new(500000.0, 5000000.0);
            let mut projected = local.clone();
            for vertex in &mut projected.vertices {
                *vertex += shift;
            }
            near(projected.area(), local.area(), 1e-6 * local.area());
            let first = local.centroid().unwrap();
            let second = projected.centroid().unwrap();
            near(second.x - shift.x, first.x, 1e-6);
            near(second.y - shift.y, first.y, 1e-6);
        }
    }
    #[test]
    fn centroid_of_a_convex_polygon_lies_inside() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let first = Point2::new(random.real(-100.0, 100.0), random.real(-100.0, 100.0));
            let second = first + Vec2::new(random.real(1.0, 50.0), random.real(-5.0, 5.0));
            let third = first + Vec2::new(random.real(-5.0, 5.0), random.real(1.0, 50.0));
            let triangle = Triangle2::new(first, second, third);
            if !triangle.is_degenerate() {
                assert_eq!(triangle.classify(triangle.centroid()), Containment::Inside);
            }
        }
    }
    #[test]
    fn closest_point_is_never_farther_than_any_sampled_point() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let segment = Segment2::new(
                Point2::new(random.real(-100.0, 100.0), random.real(-100.0, 100.0)),
                Point2::new(random.real(-100.0, 100.0), random.real(-100.0, 100.0)),
            );
            let point = Point2::new(random.real(-200.0, 200.0), random.real(-200.0, 200.0));
            let best = segment.distance_to(point);
            for sample in 0..=10 {
                assert!(best <= segment.point_at(sample as f64 / 10.0).distance_to(point) + 1e-12);
            }
        }
    }
}
