#[macro_use]
mod common;
#[path = "common/random.rs"]
mod random;

mod math_mat2 {
    use katana_math::{HALF_PI, Mat2, Vec2, mat2, tolerance};

    #[test]
    fn identity_multiply_transpose_determinant() {
        let matrix = Mat2::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(Mat2::default() * Vec2::new(4.0, 5.0), Vec2::new(4.0, 5.0));
        assert_eq!(matrix * Vec2::new(1.0, 1.0), Vec2::new(3.0, 7.0));
        assert_eq!(matrix * Mat2::default(), matrix);
        assert_eq!(matrix * matrix, Mat2::new(7.0, 10.0, 15.0, 22.0));
        assert_eq!(matrix.transposed(), Mat2::new(1.0, 3.0, 2.0, 4.0));
        assert_double_eq!(matrix.determinant(), -2.0);
    }

    #[test]
    fn inverse_and_singular_detection() {
        let matrix = Mat2::new(4.0, 7.0, 2.0, 6.0);
        let inverse = matrix.inverse().expect("invertible matrix");
        assert!(mat2::nearly_equal(
            matrix * inverse,
            Mat2::default(),
            tolerance::RELATIVE
        ));
        assert!(Mat2::new(1.0, 2.0, 2.0, 4.0).inverse().is_none());
        assert!(Mat2::new(0.0, 0.0, 0.0, 0.0).inverse().is_none());
        assert!(Mat2::new(1e-9, 0.0, 0.0, 1e-9).inverse().is_some());
    }

    #[test]
    fn rotation_preserves_length() {
        let vector = Mat2::rotation(HALF_PI) * Vec2::new(1.0, 0.0);
        assert!(vector.nearly_equal(Vec2::new(0.0, 1.0), tolerance::RELATIVE));
        assert_near!(Mat2::rotation(1.234).determinant(), 1.0, 1e-15);
    }
}

mod math_mat3 {
    use katana_math::{HALF_PI, Mat3, Vec2, mat3, tolerance};

    #[test]
    fn inverse_determinant_and_singular() {
        let matrix = Mat3::new(2.0, 0.0, 1.0, 1.0, 3.0, 2.0, 1.0, 1.0, 1.0);
        assert_double_eq!(
            matrix.determinant(),
            0.0 + 2.0 * (3.0 - 2.0) - 0.0 + 1.0 * (1.0 - 3.0)
        );
        assert!(matrix.inverse().is_none());
        let regular = Mat3::new(2.0, 0.0, 1.0, 1.0, 3.0, 2.0, 1.0, 1.0, 2.0);
        let inverse = regular.inverse().expect("invertible matrix");
        assert!(mat3::nearly_equal(
            regular * inverse,
            Mat3::default(),
            tolerance::RELATIVE
        ));
        assert!(mat3::nearly_equal(
            inverse * regular,
            Mat3::default(),
            tolerance::RELATIVE
        ));
        assert_eq!(regular.transposed().transposed(), regular);
    }

    #[test]
    fn affine_transforms_of_points_and_vectors() {
        let movement = Mat3::translation(Vec2::new(10.0, -5.0));
        assert_eq!(
            mat3::transform_point(movement, Vec2::new(1.0, 1.0)),
            Vec2::new(11.0, -4.0)
        );
        assert_eq!(
            mat3::transform_vector(movement, Vec2::new(1.0, 1.0)),
            Vec2::new(1.0, 1.0)
        );
        let turn = Mat3::rotation_about(Vec2::new(1.0, 1.0), HALF_PI);
        assert!(
            mat3::transform_point(turn, Vec2::new(2.0, 1.0))
                .nearly_equal(Vec2::new(1.0, 2.0), tolerance::RELATIVE)
        );
        assert!(
            mat3::transform_point(turn, Vec2::new(1.0, 1.0))
                .nearly_equal(Vec2::new(1.0, 1.0), tolerance::RELATIVE)
        );
        let grow = Mat3::scaling_about(Vec2::new(1.0, 1.0), 2.0, 3.0);
        assert_eq!(
            mat3::transform_point(grow, Vec2::new(2.0, 2.0)),
            Vec2::new(3.0, 4.0)
        );
    }

    #[test]
    fn reflection_about_line() {
        let mirror = Mat3::reflection(Vec2::new(2.0, 0.0), Vec2::new(0.0, 1.0));
        assert!(
            mat3::transform_point(mirror, Vec2::new(5.0, 7.0))
                .nearly_equal(Vec2::new(-1.0, 7.0), tolerance::RELATIVE)
        );
        assert_near!(mirror.determinant(), -1.0, 1e-15);
        let diagonal = Mat3::reflection(Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0));
        assert!(
            mat3::transform_point(diagonal, Vec2::new(3.0, 8.0))
                .nearly_equal(Vec2::new(8.0, 3.0), tolerance::RELATIVE)
        );
        assert_eq!(
            Mat3::reflection(Vec2::new(1.0, 1.0), Vec2::new(0.0, 0.0)),
            Mat3::default()
        );
    }
}

mod math_mat4 {
    use katana_math::{HALF_PI, Mat4, Vec3, Vec4, mat4, tolerance};

    #[test]
    fn transform_composition_order() {
        let translate = Mat4::translation(Vec3::new(10.0, 0.0, 0.0));
        let scale = Mat4::scaling(Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(
            mat4::transform_point(translate * scale, Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(12.0, 2.0, 2.0)
        );
        assert_eq!(
            mat4::transform_point(scale * translate, Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(22.0, 2.0, 2.0)
        );
        assert_eq!(
            mat4::transform_vector(translate, Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            Mat4::default() * Vec4::new(1.0, 2.0, 3.0, 4.0),
            Vec4::new(1.0, 2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn rotation_about_axis() {
        let about_z = Mat4::rotation(Vec3::new(0.0, 0.0, 5.0), HALF_PI);
        assert!(
            mat4::transform_point(about_z, Vec3::new(1.0, 0.0, 0.0))
                .nearly_equal(Vec3::new(0.0, 1.0, 0.0), tolerance::RELATIVE)
        );
        assert_near!(about_z.determinant(), 1.0, 1e-15);
        assert_eq!(
            Mat4::rotation(Vec3::new(0.0, 0.0, 0.0), 1.0),
            Mat4::default()
        );
    }

    #[test]
    fn inverse_of_known_matrix_and_singular_detection() {
        let matrix = Mat4::new(
            1.0, 0.0, 2.0, 1.0, 0.0, 1.0, 0.0, 3.0, 4.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 1.0,
        );
        let inverse = matrix.inverse().expect("invertible matrix");
        assert!(mat4::nearly_equal(
            matrix * inverse,
            Mat4::default(),
            tolerance::RELATIVE
        ));
        assert!(mat4::nearly_equal(
            inverse * matrix,
            Mat4::default(),
            tolerance::RELATIVE
        ));
        let mut singular = matrix;
        for column in 0..4 {
            singular[(3, column)] = 2.0 * singular[(0, column)];
        }
        assert_near!(singular.determinant(), 0.0, 1e-12);
        assert!(singular.inverse().is_none());
    }
}

mod math_matrix_properties {
    use super::random::{PROPERTY_ITERATIONS, Random};
    use katana_math::{Mat3, Mat4, PI, Vec3, mat4, nearly_equal};

    #[test]
    fn inverse_round_trips_points() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let scale_z = random.real(0.5, 2.0);
            let scale_y = random.real(0.5, 2.0);
            let scale = Vec3::new(random.real(0.5, 2.0), scale_y, scale_z);
            let angle = random.real(-PI, PI);
            let axis_y = random.real(-1.0, 1.0);
            let axis = Vec3::new(random.real(-1.0, 1.0), axis_y, 1.0);
            let translation_z = random.real(-1e3, 1e3);
            let translation_y = random.real(-1e3, 1e3);
            let translation = Vec3::new(random.real(-1e3, 1e3), translation_y, translation_z);
            let matrix =
                Mat4::translation(translation) * Mat4::rotation(axis, angle) * Mat4::scaling(scale);
            let inverse = matrix.inverse().expect("invertible affine matrix");
            let point_z = random.real(-1e3, 1e3);
            let point_y = random.real(-1e3, 1e3);
            let point = Vec3::new(random.real(-1e3, 1e3), point_y, point_z);
            let round_trip = mat4::transform_point(inverse, mat4::transform_point(matrix, point));
            assert!(
                round_trip.nearly_equal(point, 1e-11),
                "{round_trip} vs {point}"
            );
        }
    }

    #[test]
    fn determinant_is_multiplicative() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let mut first = Mat3::default();
            let mut second = Mat3::default();
            for index in 0..9 {
                first.data[index] = random.real(-5.0, 5.0);
                second.data[index] = random.real(-5.0, 5.0);
            }
            assert!(nearly_equal(
                (first * second).determinant(),
                first.determinant() * second.determinant(),
                1e-10
            ));
        }
    }

    #[test]
    fn transpose_of_product_is_reversed_product_of_transposes() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let mut first = Mat4::default();
            let mut second = Mat4::default();
            for index in 0..16 {
                first.data[index] = random.real(-5.0, 5.0);
                second.data[index] = random.real(-5.0, 5.0);
            }
            assert!(
                (first * second)
                    .transposed()
                    .nearly_equal(second.transposed() * first.transposed(), 1e-12)
            );
        }
    }
}
