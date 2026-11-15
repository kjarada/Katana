use crate::support::double_eq;
use katana_geometry::primitives3d::intersect;
use katana_geometry::{Point3, Segment3, Triangle3, Vec3};
use katana_math::primitives::Ray;

mod geometry_segment3 {
    use super::*;
    #[test]
    fn closest_point_and_distance() {
        let segment = Segment3::new(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0));
        double_eq(segment.length(), 10.0);
        assert_eq!(
            segment.closest_point(Point3::new(4.0, 3.0, 4.0)),
            Point3::new(4.0, 0.0, 0.0)
        );
        double_eq(segment.distance_to(Point3::new(4.0, 3.0, 4.0)), 5.0);
        assert_eq!(
            segment.closest_point(Point3::new(-4.0, 0.0, 0.0)),
            Point3::new(0.0, 0.0, 0.0)
        );
        assert_eq!(
            Segment3::new(Point3::new(1.0, 1.0, 1.0), Point3::new(1.0, 1.0, 1.0))
                .closest_point(Point3::new(9.0, 9.0, 9.0)),
            Point3::new(1.0, 1.0, 1.0)
        );
    }
}
mod geometry_triangle3 {
    use super::*;
    #[test]
    fn area_normal_plane() {
        let triangle = Triangle3::new(
            Point3::new(0.0, 0.0, 5.0),
            Point3::new(4.0, 0.0, 5.0),
            Point3::new(0.0, 3.0, 5.0),
        );
        double_eq(triangle.area(), 6.0);
        assert_eq!(triangle.scaled_normal(), Vec3::new(0.0, 0.0, 12.0));
        let plane = triangle.plane().unwrap();
        double_eq(plane.signed_distance(Point3::new(1.0, 1.0, 7.0)), 2.0);
        assert!(
            Triangle3::new(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 1.0),
                Point3::new(2.0, 2.0, 2.0)
            )
            .plane()
            .is_none()
        );
    }
}
mod geometry_ray_triangle {
    use super::*;
    #[test]
    fn hits_misses_and_degenerate_cases() {
        let triangle = Triangle3::new(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(0.0, 4.0, 0.0),
        );
        let ray = |origin, direction| Ray { origin, direction };
        double_eq(
            intersect(
                &ray(Point3::new(1.0, 1.0, 10.0), Vec3::new(0.0, 0.0, -1.0)),
                &triangle,
            )
            .unwrap(),
            10.0,
        );
        assert!(
            intersect(
                &ray(Point3::new(1.0, 1.0, -10.0), Vec3::new(0.0, 0.0, 1.0)),
                &triangle
            )
            .is_some()
        );
        assert!(
            intersect(
                &ray(Point3::new(3.0, 3.0, 10.0), Vec3::new(0.0, 0.0, -1.0)),
                &triangle
            )
            .is_none()
        );
        assert!(
            intersect(
                &ray(Point3::new(1.0, 1.0, 10.0), Vec3::new(0.0, 0.0, 1.0)),
                &triangle
            )
            .is_none()
        );
        assert!(
            intersect(
                &ray(Point3::new(1.0, 1.0, 10.0), Vec3::new(1.0, 0.0, 0.0)),
                &triangle
            )
            .is_none()
        );
        assert!(
            intersect(
                &ray(Point3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0)),
                &triangle
            )
            .is_some()
        );
        assert!(
            intersect(
                &ray(Point3::new(2.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0)),
                &triangle
            )
            .is_some()
        );
        let degenerate = Triangle3::new(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
        );
        assert!(
            intersect(
                &ray(Point3::new(1.0, 1.0, 10.0), Vec3::new(0.0, 0.0, -1.0)),
                &degenerate
            )
            .is_none()
        );
    }
}
