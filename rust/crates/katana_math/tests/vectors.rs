#[macro_use]
mod common;
#[path = "common/random.rs"]
mod random;

mod math_vec2 {
    use katana_math::vec2::*;
    use katana_math::{HALF_PI, PI, tolerance};

    #[test]
    fn arithmetic_and_length() {
        let first = Vec2::new(3.0, 4.0);
        let second = Vec2::new(1.0, 2.0);
        assert_eq!(first + second, Vec2::new(4.0, 6.0));
        assert_eq!(first - second, Vec2::new(2.0, 2.0));
        assert_eq!(first * 2.0, Vec2::new(6.0, 8.0));
        assert_eq!(2.0 * first, Vec2::new(6.0, 8.0));
        assert_eq!(first / 2.0, Vec2::new(1.5, 2.0));
        assert_eq!(-first, Vec2::new(-3.0, -4.0));
        assert_double_eq!(first.length(), 5.0);
        assert_double_eq!(first.length_squared(), 25.0);
        assert_double_eq!(distance(first, second), 8.0_f64.sqrt());
    }

    #[test]
    fn dot_cross_and_perpendicular() {
        let x_axis = Vec2::new(1.0, 0.0);
        let y_axis = Vec2::new(0.0, 1.0);
        assert_double_eq!(dot(Vec2::new(3.0, 4.0), Vec2::new(1.0, 2.0)), 11.0);
        assert_double_eq!(cross(x_axis, y_axis), 1.0);
        assert_double_eq!(cross(y_axis, x_axis), -1.0);
        assert_eq!(x_axis.perpendicular(), y_axis);
        assert_double_eq!(
            dot(Vec2::new(3.0, 4.0), Vec2::new(3.0, 4.0).perpendicular()),
            0.0
        );
    }

    #[test]
    fn normalization_including_zero_vector() {
        assert!(nearly_equal(
            normalize(Vec2::new(3.0, 4.0)),
            Vec2::new(0.6, 0.8),
            tolerance::RELATIVE
        ));
        assert_eq!(normalize(Vec2::new(0.0, 0.0)), Vec2::new(0.0, 0.0));
        assert_double_eq!(Vec2::new(3e200, 4e200).length(), 5e200);
        assert_double_eq!(Vec2::new(3e-200, 4e-200).length(), 5e-200);
        assert_near!(normalize(Vec2::new(3e-200, 4e-200)).length(), 1.0, 1e-15);
    }

    #[test]
    fn rotation_and_angle() {
        assert!(nearly_equal(
            Vec2::new(1.0, 0.0).rotated(HALF_PI),
            Vec2::new(0.0, 1.0),
            tolerance::RELATIVE
        ));
        assert_double_eq!(Vec2::new(0.0, 2.0).angle(), HALF_PI);
        assert_double_eq!(Vec2::new(-1.0, 0.0).angle(), PI);
    }

    #[test]
    fn lerp_project_reflect() {
        assert_eq!(
            lerp(Vec2::new(0.0, 0.0), Vec2::new(10.0, 20.0), 0.25),
            Vec2::new(2.5, 5.0)
        );
        assert_eq!(
            project(Vec2::new(3.0, 4.0), Vec2::new(10.0, 0.0)),
            Vec2::new(3.0, 0.0)
        );
        assert_eq!(
            project(Vec2::new(3.0, 4.0), Vec2::new(0.0, 0.0)),
            Vec2::new(0.0, 0.0)
        );
        assert_eq!(
            reflect(Vec2::new(1.0, -1.0), Vec2::new(0.0, 1.0)),
            Vec2::new(1.0, 1.0)
        );
    }

    #[test]
    fn equality_is_exact_and_nearly_equal_is_tolerant() {
        let first = Vec2::new(1.0, 2.0);
        let second = Vec2::new(1.0 + 1e-12, 2.0);
        assert_ne!(first, second);
        assert!(nearly_equal(first, second, tolerance::RELATIVE));
    }

    #[test]
    fn division_by_zero_follows_ieee_and_is_detectable() {
        let vector = Vec2::new(1.0, -1.0) / 0.0;
        assert!(!vector.is_finite());
    }
}

mod math_vec3 {
    use katana_math::vec3::*;

    #[test]
    fn cross_product_follows_right_hand_rule() {
        let x_axis = Vec3::new(1.0, 0.0, 0.0);
        let y_axis = Vec3::new(0.0, 1.0, 0.0);
        let z_axis = Vec3::new(0.0, 0.0, 1.0);
        assert_eq!(cross(x_axis, y_axis), z_axis);
        assert_eq!(cross(y_axis, z_axis), x_axis);
        assert_eq!(cross(z_axis, x_axis), y_axis);
        assert_eq!(cross(x_axis, x_axis), Vec3::new(0.0, 0.0, 0.0));
        assert_double_eq!(distance(x_axis, y_axis), 2.0_f64.sqrt());
    }

    #[test]
    fn lerp_project_reflect() {
        assert_eq!(
            lerp(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 4.0, 6.0), 0.5),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            project(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.0, 5.0)),
            Vec3::new(0.0, 0.0, 3.0)
        );
        assert_eq!(
            reflect(Vec3::new(1.0, 1.0, -1.0), Vec3::new(0.0, 0.0, 1.0)),
            Vec3::new(1.0, 1.0, 1.0)
        );
        assert_eq!(
            normalize(Vec3::new(0.0, 0.0, 0.0)),
            Vec3::new(0.0, 0.0, 0.0)
        );
    }
}

mod math_vec4 {
    use katana_math::Vec3;
    use katana_math::vec4::*;

    #[test]
    fn dot_length_and_homogeneous_helpers() {
        let first = Vec4::new(1.0, 2.0, 3.0, 4.0);
        let second = Vec4::new(4.0, 3.0, 2.0, 1.0);
        assert_double_eq!(dot(first, second), 20.0);
        assert_double_eq!(Vec4::new(2.0, 0.0, 0.0, 0.0).length(), 2.0);
        assert_eq!(
            Vec4::from_xyz(Vec3::new(1.0, 2.0, 3.0), 1.0).xyz(),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(lerp(first, second, 0.5), Vec4::new(2.5, 2.5, 2.5, 2.5));
    }
}

mod math_vector_properties {
    use super::random::{PROPERTY_ITERATIONS, Random};
    use katana_math::{Vec2, Vec3, vec3};

    #[test]
    fn distance_to_self_is_zero_and_symmetric() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let first_z = random.real(-1e3, 1e3);
            let first_y = random.real(-1e7, 1e7);
            let first = Vec3::new(random.real(-1e7, 1e7), first_y, first_z);
            let second_z = random.real(-1e3, 1e3);
            let second_y = random.real(-1e7, 1e7);
            let second = Vec3::new(random.real(-1e7, 1e7), second_y, second_z);
            assert_eq!(first.distance_to(first), 0.0);
            assert_eq!(first.distance_to(second), second.distance_to(first));
        }
    }

    #[test]
    fn triangle_inequality() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let first_y = random.real(-1e4, 1e4);
            let first = Vec2::new(random.real(-1e4, 1e4), first_y);
            let second_y = random.real(-1e4, 1e4);
            let second = Vec2::new(random.real(-1e4, 1e4), second_y);
            let third_y = random.real(-1e4, 1e4);
            let third = Vec2::new(random.real(-1e4, 1e4), third_y);
            assert!(
                first.distance_to(third)
                    <= (first.distance_to(second) + second.distance_to(third)) * (1.0 + 1e-15)
            );
        }
    }

    #[test]
    fn normalized_vectors_have_unit_length() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let z_value = random.real(-1e6, 1e6);
            let y_value = random.real(-1e6, 1e6);
            let vector = Vec3::new(random.real(-1e6, 1e6), y_value, z_value);
            assert_near!(vector.normalized().length(), 1.0, 1e-15);
        }
    }

    #[test]
    fn cross_product_is_orthogonal_to_its_operands() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let first_z = random.real(-100.0, 100.0);
            let first_y = random.real(-100.0, 100.0);
            let first = Vec3::new(random.real(-100.0, 100.0), first_y, first_z);
            let second_z = random.real(-100.0, 100.0);
            let second_y = random.real(-100.0, 100.0);
            let second = Vec3::new(random.real(-100.0, 100.0), second_y, second_z);
            let product = first.cross(second);
            let scale = first.length() * second.length() * product.length() + 1.0;
            assert_near!(product.dot(first) / scale, 0.0, 1e-15);
            assert_near!(product.dot(second) / scale, 0.0, 1e-15);
        }
    }

    #[test]
    fn projection_residual_is_orthogonal_and_reflection_is_an_involution() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let z_value = random.real(-100.0, 100.0);
            let y_value = random.real(-100.0, 100.0);
            let vector = Vec3::new(random.real(-100.0, 100.0), y_value, z_value);
            let onto_z = random.real(-100.0, 100.0);
            let onto_y = random.real(-100.0, 100.0);
            let onto = Vec3::new(random.real(1.0, 100.0), onto_y, onto_z);
            let residual = vector - vec3::project(vector, onto);
            assert_near!(
                residual.dot(onto) / (onto.length_squared() + 1.0),
                0.0,
                1e-13
            );
            let normal = onto.normalized();
            assert!(vec3::nearly_equal(
                vec3::reflect(vec3::reflect(vector, normal), normal),
                vector,
                1e-12
            ));
        }
    }
}
