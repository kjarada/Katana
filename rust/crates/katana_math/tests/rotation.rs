#[macro_use]
mod common;
#[path = "common/random.rs"]
mod random;

mod math_quaternion {
    use super::random::{PROPERTY_ITERATIONS, Random};
    use katana_math::{HALF_PI, PI, Quaternion, Vec3, quaternion::slerp, tolerance};

    #[test]
    fn normalization_conjugate_inverse() {
        let quaternion = Quaternion::new(0.0, 0.0, 0.0, 2.0);
        assert_eq!(quaternion.normalized(), Quaternion::new(0.0, 0.0, 0.0, 1.0));
        assert_eq!(quaternion.inverse(), Quaternion::new(0.0, 0.0, 0.0, 0.5));
        assert_eq!(
            Quaternion::new(1.0, 2.0, 3.0, 4.0).conjugate(),
            Quaternion::new(-1.0, -2.0, -3.0, 4.0)
        );
        assert_eq!(
            Quaternion::new(0.0, 0.0, 0.0, 0.0).normalized(),
            Quaternion::default()
        );
        assert_eq!(
            Quaternion::new(0.0, 0.0, 0.0, 0.0).inverse(),
            Quaternion::default()
        );
    }

    #[test]
    fn axis_angle_rotation() {
        let about_z = Quaternion::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), HALF_PI);
        assert!(
            about_z
                .rotate(Vec3::new(1.0, 0.0, 0.0))
                .nearly_equal(Vec3::new(0.0, 1.0, 0.0), tolerance::RELATIVE)
        );
        assert_near!(about_z.length(), 1.0, 1e-15);
        assert_eq!(
            Quaternion::from_axis_angle(Vec3::new(0.0, 0.0, 0.0), 1.0),
            Quaternion::default()
        );
    }

    #[test]
    fn composition_applies_right_operand_first() {
        let about_z = Quaternion::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), HALF_PI);
        let about_x = Quaternion::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), HALF_PI);
        assert!(
            (about_x * about_z)
                .rotate(Vec3::new(1.0, 0.0, 0.0))
                .nearly_equal(Vec3::new(0.0, 0.0, 1.0), tolerance::RELATIVE)
        );
    }

    #[test]
    fn matrix_agrees_with_rotate() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let angle = random.real(-PI, PI);
            let axis_z = random.real(0.1, 1.0);
            let axis_y = random.real(-1.0, 1.0);
            let quaternion = Quaternion::from_axis_angle(
                Vec3::new(random.real(-1.0, 1.0), axis_y, axis_z),
                angle,
            );
            let vector_z = random.real(-10.0, 10.0);
            let vector_y = random.real(-10.0, 10.0);
            let vector = Vec3::new(random.real(-10.0, 10.0), vector_y, vector_z);
            assert!(
                (quaternion.to_matrix() * vector).nearly_equal(quaternion.rotate(vector), 1e-13)
            );
            assert_near!(quaternion.rotate(vector).length(), vector.length(), 1e-13);
        }
    }

    #[test]
    fn slerp_endpoints_midpoint_and_shortest_arc() {
        let first = Quaternion::default();
        let second = Quaternion::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), HALF_PI);
        assert!(slerp(first, second, 0.0).nearly_equal(first, tolerance::RELATIVE));
        assert!(slerp(first, second, 1.0).nearly_equal(second, tolerance::RELATIVE));
        assert!(slerp(first, second, 0.5).nearly_equal(
            Quaternion::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), HALF_PI / 2.0),
            tolerance::RELATIVE
        ));
        let negated = Quaternion::new(-second.x, -second.y, -second.z, -second.w);
        assert!(
            slerp(first, negated, 0.5).nearly_equal(slerp(first, second, 0.5), tolerance::RELATIVE)
        );
        assert!(slerp(second, second, 0.3).nearly_equal(second, tolerance::RELATIVE));
    }
}

mod math_transform {
    use super::random::{PROPERTY_ITERATIONS, Random};
    use katana_math::{HALF_PI, Mat4, PI, Quaternion, Transform, Vec3, mat4, tolerance};

    #[test]
    fn identity_matrix_and_trs_order() {
        assert_eq!(Transform::default().matrix(), Mat4::default());
        let transform = Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            rotation: Quaternion::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), HALF_PI),
            scale: Vec3::new(2.0, 2.0, 2.0),
        };
        assert!(
            transform
                .transform_point(Vec3::new(1.0, 0.0, 0.0))
                .nearly_equal(Vec3::new(10.0, 2.0, 0.0), tolerance::RELATIVE)
        );
        assert!(
            transform
                .transform_vector(Vec3::new(1.0, 0.0, 0.0))
                .nearly_equal(Vec3::new(0.0, 2.0, 0.0), tolerance::RELATIVE)
        );
        assert!(
            mat4::transform_point(transform.matrix(), Vec3::new(1.0, 0.0, 0.0)).nearly_equal(
                transform.transform_point(Vec3::new(1.0, 0.0, 0.0)),
                tolerance::RELATIVE
            )
        );
    }

    #[test]
    fn inverse_round_trip_property() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let translation_z = random.real(-100.0, 100.0);
            let translation_y = random.real(-1e4, 1e4);
            let translation = Vec3::new(random.real(-1e4, 1e4), translation_y, translation_z);
            let angle = random.real(-PI, PI);
            let axis_z = random.real(0.1, 1.0);
            let axis_y = random.real(-1.0, 1.0);
            let rotation = Quaternion::from_axis_angle(
                Vec3::new(random.real(-1.0, 1.0), axis_y, axis_z),
                angle,
            );
            let scale_z = random.real(0.1, 10.0);
            let scale_y = random.real(0.1, 10.0);
            let scale = Vec3::new(random.real(0.1, 10.0), scale_y, scale_z);
            let transform = Transform {
                translation,
                rotation,
                scale,
            };
            let point_z = random.real(-1e3, 1e3);
            let point_y = random.real(-1e3, 1e3);
            let point = Vec3::new(random.real(-1e3, 1e3), point_y, point_z);
            let back = transform
                .inverse_transform_point(transform.transform_point(point))
                .expect("nonzero scale");
            assert!(back.nearly_equal(point, 1e-11), "{back} vs {point}");
        }
    }

    #[test]
    fn zero_scale_has_no_inverse() {
        let transform = Transform {
            scale: Vec3::new(1.0, 0.0, 1.0),
            ..Transform::default()
        };
        assert!(
            transform
                .inverse_transform_point(Vec3::new(1.0, 2.0, 3.0))
                .is_none()
        );
    }
}
