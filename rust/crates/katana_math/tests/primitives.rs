#[macro_use]
mod common;

mod math_plane {
    use katana_math::{Plane, Vec3, tolerance};

    #[test]
    fn signed_distance_and_projection() {
        let ground = Plane::new(Vec3::new(0.0, 0.0, 1.0), -5.0);
        assert_double_eq!(ground.signed_distance(Vec3::new(1.0, 2.0, 8.0)), 3.0);
        assert_double_eq!(ground.signed_distance(Vec3::new(1.0, 2.0, 1.0)), -4.0);
        assert_eq!(
            ground.project(Vec3::new(1.0, 2.0, 8.0)),
            Vec3::new(1.0, 2.0, 5.0)
        );
    }

    #[test]
    fn from_points_uses_right_hand_rule_and_rejects_collinear() {
        let plane = Plane::from_points(
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, 2.0),
            Vec3::new(0.0, 1.0, 2.0),
        )
        .expect("noncollinear points");
        assert!(
            plane
                .normal
                .nearly_equal(Vec3::new(0.0, 0.0, 1.0), tolerance::RELATIVE)
        );
        assert_near!(plane.signed_distance(Vec3::new(7.0, -3.0, 2.0)), 0.0, 1e-15);
        assert!(
            Plane::from_points(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(2.0, 2.0, 2.0)
            )
            .is_none()
        );
        assert!(
            Plane::from_points(
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(2.0, 2.0, 2.0)
            )
            .is_none()
        );
        assert!(
            Plane::from_point_normal(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)).is_none()
        );
    }
}

mod math_ray_plane {
    use katana_math::{Plane, Ray, Vec3, primitives::intersect};

    #[test]
    fn hit_miss_parallel_and_behind() {
        let ground = Plane::new(Vec3::new(0.0, 0.0, 1.0), 0.0);
        let hit = intersect(
            Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -2.0)),
            ground,
        )
        .expect("ray hits ground");
        assert_double_eq!(hit, 5.0);
        assert_eq!(
            Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -2.0)).at(hit),
            Vec3::new(0.0, 0.0, 0.0)
        );
        assert!(
            intersect(
                Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::new(1.0, 0.0, 0.0)),
                ground
            )
            .is_none()
        );
        assert!(
            intersect(
                Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
                ground
            )
            .is_none()
        );
        assert!(
            intersect(
                Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 1.0)),
                ground
            )
            .is_none()
        );
        let touching = intersect(
            Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
            ground,
        )
        .expect("origin on ground");
        assert_double_eq!(touching, 0.0);
    }
}

mod math_aabb {
    use katana_math::{AABB, Vec3};

    #[test]
    fn default_is_empty_and_grows_by_expansion() {
        let mut bounds = AABB::default();
        assert!(bounds.empty());
        assert_eq!(bounds.size(), Vec3::new(0.0, 0.0, 0.0));
        assert!(!bounds.contains(Vec3::new(0.0, 0.0, 0.0)));
        bounds.expand(Vec3::new(1.0, 2.0, 3.0));
        assert!(!bounds.empty());
        assert_eq!(bounds.size(), Vec3::new(0.0, 0.0, 0.0));
        bounds.expand(Vec3::new(-1.0, 4.0, 0.0));
        assert_eq!(bounds.min, Vec3::new(-1.0, 2.0, 0.0));
        assert_eq!(bounds.max, Vec3::new(1.0, 4.0, 3.0));
        assert_eq!(bounds.center(), Vec3::new(0.0, 3.0, 1.5));
        let mut merged = AABB::default();
        merged.expand(AABB::default());
        assert!(merged.empty());
        merged.expand(bounds);
        assert_eq!(merged.min, bounds.min);
    }

    #[test]
    fn containment_and_overlap_are_boundary_inclusive() {
        let bounds = AABB::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 4.0, 6.0));
        assert!(bounds.contains(Vec3::new(2.0, 4.0, 6.0)));
        assert!(bounds.contains(Vec3::new(1.0, 1.0, 1.0)));
        assert!(!bounds.contains(Vec3::new(2.0000001, 1.0, 1.0)));
        assert!(bounds.intersects(AABB::new(
            Vec3::new(2.0, 4.0, 6.0),
            Vec3::new(3.0, 5.0, 7.0)
        )));
        assert!(!bounds.intersects(AABB::new(
            Vec3::new(2.1, 0.0, 0.0),
            Vec3::new(3.0, 1.0, 1.0)
        )));
        assert!(!bounds.intersects(AABB::default()));
    }
}

mod math_ray_aabb {
    use katana_math::{AABB, Ray, Vec3, primitives::intersect};

    #[test]
    fn slab_intersection_edge_cases() {
        let bounds = AABB::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        let through = intersect(
            Ray::new(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
            bounds,
        )
        .expect("ray through box");
        assert_double_eq!(through.t_enter, 1.0);
        assert_double_eq!(through.t_exit, 2.0);
        let inside = intersect(
            Ray::new(Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.0, 0.0, 1.0)),
            bounds,
        )
        .expect("origin inside box");
        assert_double_eq!(inside.t_enter, 0.0);
        assert_double_eq!(inside.t_exit, 0.5);
        assert!(
            intersect(
                Ray::new(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
                bounds
            )
            .is_some()
        );
        assert!(
            intersect(
                Ray::new(Vec3::new(-1.0, 2.0, 0.5), Vec3::new(1.0, 0.0, 0.0)),
                bounds
            )
            .is_none()
        );
        assert!(
            intersect(
                Ray::new(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(-1.0, 0.0, 0.0)),
                bounds
            )
            .is_none()
        );
        let diagonal = intersect(
            Ray::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0)),
            bounds,
        )
        .expect("ray through opposite corners");
        assert_double_eq!(diagonal.t_enter, 1.0);
        assert_double_eq!(diagonal.t_exit, 2.0);
        assert!(
            intersect(
                Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
                AABB::default()
            )
            .is_none()
        );
    }
}
