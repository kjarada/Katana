#[macro_use]
mod common;

mod math_numerics {
    use katana_math::*;

    #[test]
    fn tolerances_are_ordered_from_tightest_to_loosest() {
        let absolute = tolerance::ABSOLUTE;
        let relative = tolerance::RELATIVE;
        let geometric = tolerance::GEOMETRIC;
        let coordinate = tolerance::COORDINATE;
        assert!(absolute < relative);
        assert!(geometric < coordinate);
        let ulp_at_northing = 1.0e7_f64.next_up() - 1.0e7;
        assert!(geometric > 10.0 * ulp_at_northing);
    }

    #[test]
    fn nearly_equal_is_relative_for_large_magnitudes() {
        assert!(nearly_equal(1.0, 1.0 + 1e-10, tolerance::RELATIVE));
        assert!(!nearly_equal(1.0, 1.0 + 1e-8, tolerance::RELATIVE));
        assert!(nearly_equal(5.0e6, 5.0e6 + 1e-3, tolerance::RELATIVE));
        assert!(!lengths_equal(5.0e6, 5.0e6 + 1e-3, tolerance::GEOMETRIC));
    }

    #[test]
    fn nearly_equal_is_absolute_near_zero() {
        assert!(nearly_equal(0.0, 1e-10, tolerance::RELATIVE));
        assert!(!nearly_equal(0.0, 1e-8, tolerance::RELATIVE));
        assert!(nearly_zero(1e-13, tolerance::ABSOLUTE));
        assert!(!nearly_zero(1e-11, tolerance::ABSOLUTE));
    }

    #[test]
    fn nearly_equal_handles_non_finite_values() {
        let infinity = f64::INFINITY;
        let nan = f64::NAN;
        assert!(nearly_equal(infinity, infinity, tolerance::RELATIVE));
        assert!(!nearly_equal(infinity, -infinity, tolerance::RELATIVE));
        assert!(!nearly_equal(infinity, 1e300, tolerance::RELATIVE));
        assert!(!nearly_equal(nan, nan, tolerance::RELATIVE));
        assert!(!nearly_equal(nan, 0.0, tolerance::RELATIVE));
    }

    #[test]
    fn normalize_angle_wraps_into_half_open_range() {
        assert_double_eq!(normalize_angle(0.0), 0.0);
        assert_double_eq!(normalize_angle(TWO_PI), 0.0);
        assert_near!(normalize_angle(-HALF_PI), 1.5 * PI, 1e-15);
        assert_near!(normalize_angle(5.0 * PI), PI, 1e-14);
        assert!(normalize_angle(-1e-20) < TWO_PI);
        assert!(normalize_angle(-1e-20) >= 0.0);
    }

    #[test]
    fn normalize_angle_signed_wraps_into_plus_minus_pi() {
        assert_near!(normalize_angle_signed(1.5 * PI), -HALF_PI, 1e-15);
        assert_near!(normalize_angle_signed(-1.5 * PI), HALF_PI, 1e-15);
        assert_double_eq!(normalize_angle_signed(PI), PI);
    }

    #[test]
    fn angles_equal_compares_across_the_wrap() {
        assert!(angles_equal(0.0, TWO_PI, tolerance::ANGULAR));
        assert!(angles_equal(-PI, PI, tolerance::ANGULAR));
        assert!(angles_equal(1e-11, TWO_PI - 1e-11, tolerance::ANGULAR));
        assert!(!angles_equal(0.0, 1e-6, tolerance::ANGULAR));
    }

    #[test]
    fn clamp_and_lerp() {
        assert_double_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_double_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
        assert_double_eq!(clamp(0.25, 0.0, 1.0), 0.25);
        assert_double_eq!(lerp(10.0, 20.0, 0.0), 10.0);
        assert_double_eq!(lerp(10.0, 20.0, 1.0), 20.0);
        assert_double_eq!(lerp(10.0, 20.0, 0.5), 15.0);
    }

    #[test]
    fn degree_radian_constants_are_consistent() {
        assert_double_eq!(180.0 * DEG_TO_RAD, PI);
        assert_double_eq!(PI * RAD_TO_DEG, 180.0);
    }
}
