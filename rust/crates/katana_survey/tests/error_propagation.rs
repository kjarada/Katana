//! Twin of tests/survey/test_error_propagation.cpp.

mod survey_support;

use katana_survey::Covariance2;

fn cov(northing: f64, easting: f64, northing_easting: f64) -> Covariance2 {
    Covariance2 {
        northing,
        easting,
        northing_easting,
    }
}

// ---- error ellipse ---------------------------------------------------------

mod survey_error_ellipse {
    use super::*;
    use crate::survey_support::{Random, double_eq, error_code, near};
    use katana_core::ErrorCode;
    use katana_math::numerics::{DEG_TO_RAD, HALF_PI, PI};
    use katana_survey::error_propagation::{ellipse_confidence_scale, error_ellipse};

    #[test]
    fn axis_aligned_covariance() {
        // Variances 4 (north) and 1 (east): semi-axes 2 and 1, major axis due north.
        let north = error_ellipse(cov(4.0, 1.0, 0.0)).unwrap();
        double_eq(north.semi_major, 2.0);
        double_eq(north.semi_minor, 1.0);
        double_eq(north.orientation, 0.0);

        let east = error_ellipse(cov(1.0, 4.0, 0.0)).unwrap();
        double_eq(east.semi_major, 2.0);
        double_eq(east.semi_minor, 1.0);
        near(east.orientation, HALF_PI, 1e-15);
    }

    #[test]
    fn correlated_covariance() {
        // [[2, 1], [1, 2]]: eigenvalues 3 and 1; positive correlation of N and E
        // stretches the ellipse along north-east (azimuth 45°).
        let positive = error_ellipse(cov(2.0, 2.0, 1.0)).unwrap();
        near(positive.semi_major, 3.0_f64.sqrt(), 1e-15);
        near(positive.semi_minor, 1.0, 1e-15);
        near(positive.orientation, 45.0 * DEG_TO_RAD, 1e-15);

        // Negative correlation: north-west / south-east, reported in [0, pi) as 135°.
        let negative = error_ellipse(cov(2.0, 2.0, -1.0)).unwrap();
        near(negative.semi_major, 3.0_f64.sqrt(), 1e-15);
        near(negative.orientation, 135.0 * DEG_TO_RAD, 1e-15);
    }

    #[test]
    fn degenerate_cases() {
        // Circle: the orientation is arbitrary and reported as 0.
        let circle = error_ellipse(cov(9.0, 9.0, 0.0)).unwrap();
        double_eq(circle.semi_major, 3.0);
        double_eq(circle.semi_minor, 3.0);
        double_eq(circle.orientation, 0.0);

        // A fixed point has no ellipse at all.
        let point = error_ellipse(cov(0.0, 0.0, 0.0)).unwrap();
        double_eq(point.semi_major, 0.0);
        double_eq(point.semi_minor, 0.0);

        // Perfect correlation: a line, the minor axis is exactly zero, never NaN.
        let line = error_ellipse(cov(1.0, 1.0, 1.0)).unwrap();
        near(line.semi_major, 2.0_f64.sqrt(), 1e-15);
        double_eq(line.semi_minor, 0.0);
    }

    #[test]
    fn rejects_what_is_not_a_covariance() {
        assert_eq!(
            error_code(error_ellipse(cov(-1.0, 1.0, 0.0))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            error_code(error_ellipse(cov(1.0, -1.0, 0.0))),
            ErrorCode::InvalidArgument
        );
        // |rho| > 1
        assert_eq!(
            error_code(error_ellipse(cov(1.0, 1.0, 1.5))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            error_code(error_ellipse(cov(f64::NAN, 1.0, 0.0))),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn property_axes_are_the_extremes_of_the_directional_variance() {
        let mut random = Random::default();
        for _ in 0..300 {
            let sn = random.real(0.001, 0.05);
            let se = random.real(0.001, 0.05);
            let rho = random.real(-0.95, 0.95);
            let c = cov(sn * sn, se * se, rho * sn * se);
            let ellipse = error_ellipse(c).unwrap();
            assert!(ellipse.semi_major >= ellipse.semi_minor);
            assert!(ellipse.orientation >= 0.0);
            assert!(ellipse.orientation < PI);

            // Variance of the position along azimuth t: u^T C u with
            // u = (cos t, sin t).
            let along = |t: f64| {
                let n = t.cos();
                let e = t.sin();
                c.northing * n * n + 2.0 * c.northing_easting * n * e + c.easting * e * e
            };
            let scale = ellipse.semi_major * ellipse.semi_major;
            near(along(ellipse.orientation), scale, 1e-12 * scale);
            near(
                along(ellipse.orientation + HALF_PI),
                ellipse.semi_minor * ellipse.semi_minor,
                1e-12 * scale,
            );
            // Rotation invariants: trace and determinant.
            let minor2 = ellipse.semi_minor * ellipse.semi_minor;
            near(scale + minor2, c.northing + c.easting, 1e-12 * scale);
            near(
                scale * minor2,
                c.northing * c.easting - c.northing_easting * c.northing_easting,
                1e-10 * scale * scale,
            );
            // No direction is worse than the major axis.
            for k in 0..16 {
                assert!(along(PI * f64::from(k) / 16.0) <= scale * (1.0 + 1e-12));
            }
        }
    }

    #[test]
    fn confidence_scale() {
        // chi-square(2) quantile in closed form; 2.4477 is the familiar 95 % factor.
        near(
            ellipse_confidence_scale(0.95).unwrap(),
            (-2.0 * 0.05_f64.ln()).sqrt(),
            1e-14,
        );
        near(ellipse_confidence_scale(0.95).unwrap(), 2.4477, 5e-5);
        // The standard ellipse itself holds 1 - exp(-1/2) = 39.35 % of the
        // probability.
        near(
            ellipse_confidence_scale(1.0 - (-0.5_f64).exp()).unwrap(),
            1.0,
            1e-14,
        );
        assert!(ellipse_confidence_scale(0.0).is_err());
        assert!(ellipse_confidence_scale(1.0).is_err());
        assert!(ellipse_confidence_scale(f64::NAN).is_err());
    }
}

// ---- forward, inverse and the general law ------------------------------------

mod survey_error_propagation {
    use super::*;
    use crate::survey_support::{Random, near};
    use katana_core::ErrorCode;
    use katana_math::numerics::{DEG_TO_RAD, HALF_PI, TWO_PI};
    use katana_survey::cogo::forward;
    use katana_survey::error_propagation::{
        error_ellipse, propagate_covariance, propagate_forward, propagate_inverse,
    };
    use katana_survey::{Coordinate2, Matrix};

    fn c(northing: f64, easting: f64) -> Coordinate2 {
        Coordinate2 { northing, easting }
    }

    fn matrix<const N: usize>(rows: &[[f64; N]]) -> Matrix {
        Matrix::from_rows(rows).unwrap()
    }

    #[test]
    fn forward_along_the_axes() {
        // 100 m due north, sigma_d = 5 mm, sigma_az = 1e-4 rad (20.6"):
        //   along the line  (north): sigma_d            = 0.005
        //   across the line (east) : d * sigma_az       = 0.010
        let north = propagate_forward(Covariance2::default(), 0.0, 100.0, 1e-4, 0.005).unwrap();
        near(north.northing, 0.005 * 0.005, 1e-18);
        near(north.easting, 0.010 * 0.010, 1e-18);
        near(north.northing_easting, 0.0, 1e-18);

        // Due east the roles swap.
        let east = propagate_forward(Covariance2::default(), HALF_PI, 100.0, 1e-4, 0.005).unwrap();
        near(east.northing, 0.010 * 0.010, 1e-18);
        near(east.easting, 0.005 * 0.005, 1e-18);
        near(east.northing_easting, 0.0, 1e-18);
    }

    #[test]
    fn forward_at_forty_five_degrees() {
        // cross^2 = 1e-4, along^2 = 2.5e-5, sin^2 = cos^2 = 1/2:
        //   var N = var E = (1e-4 + 2.5e-5)/2 = 6.25e-5
        //   cov   = sin cos (along^2 - cross^2) = (2.5e-5 - 1e-4)/2 = -3.75e-5
        let result = propagate_forward(
            Covariance2::default(),
            45.0 * DEG_TO_RAD,
            100.0,
            1e-4,
            0.005,
        )
        .unwrap();
        near(result.northing, 6.25e-5, 1e-18);
        near(result.easting, 6.25e-5, 1e-18);
        near(result.northing_easting, -3.75e-5, 1e-18);

        // Its ellipse: 10 mm across the line (azimuth 135°), 5 mm along it.
        let ellipse = error_ellipse(result).unwrap();
        near(ellipse.semi_major, 0.010, 1e-14);
        near(ellipse.semi_minor, 0.005, 1e-14);
        near(ellipse.orientation, 135.0 * DEG_TO_RAD, 1e-12);
    }

    #[test]
    fn forward_adds_the_start_covariance() {
        let start = cov(4e-6, 9e-6, 1e-6);
        let free = propagate_forward(Covariance2::default(), 1.0, 250.0, 2e-5, 0.003).unwrap();
        let tied = propagate_forward(start, 1.0, 250.0, 2e-5, 0.003).unwrap();
        near(tied.northing, free.northing + 4e-6, 1e-20);
        near(tied.easting, free.easting + 9e-6, 1e-20);
        near(tied.northing_easting, free.northing_easting + 1e-6, 1e-20);

        // Errorless observations move the start covariance unchanged.
        assert_eq!(
            propagate_forward(start, 1.0, 250.0, 0.0, 0.0).unwrap(),
            start
        );
    }

    #[test]
    fn property_forward_matches_numerical_jacobian() {
        // J Sigma J^T with J obtained by central differences of forward() itself:
        // an independent check of the analytic partials.
        let mut random = Random::default();
        for _ in 0..100 {
            let azimuth = random.real(0.0, TWO_PI);
            let distance = random.real(10.0, 1000.0);
            let sigma_azimuth = random.real(1e-6, 1e-4);
            let sigma_distance = random.real(0.001, 0.01);

            let h_az = 1e-6;
            let h_d = 1e-3;
            let at = |az: f64, d: f64| forward(c(0.0, 0.0), az, d).unwrap();
            let az_plus = at(azimuth + h_az, distance);
            let az_minus = at(azimuth - h_az, distance);
            let d_plus = at(azimuth, distance + h_d);
            let d_minus = at(azimuth, distance - h_d);
            let mut jacobian = Matrix::new(2, 2);
            jacobian[(0, 0)] = (az_plus.northing - az_minus.northing) / (2.0 * h_az);
            jacobian[(1, 0)] = (az_plus.easting - az_minus.easting) / (2.0 * h_az);
            jacobian[(0, 1)] = (d_plus.northing - d_minus.northing) / (2.0 * h_d);
            jacobian[(1, 1)] = (d_plus.easting - d_minus.easting) / (2.0 * h_d);
            let mut sigma = Matrix::new(2, 2);
            sigma[(0, 0)] = sigma_azimuth * sigma_azimuth;
            sigma[(1, 1)] = sigma_distance * sigma_distance;

            let general = propagate_covariance(&jacobian, &sigma).unwrap();
            let special = propagate_forward(
                Covariance2::default(),
                azimuth,
                distance,
                sigma_azimuth,
                sigma_distance,
            )
            .unwrap();
            let scale = special.northing + special.easting;
            near(special.northing, general[(0, 0)], 1e-7 * scale);
            near(special.easting, general[(1, 1)], 1e-7 * scale);
            near(special.northing_easting, general[(0, 1)], 1e-7 * scale);
        }
    }

    #[test]
    fn forward_rejects_bad_input() {
        let none = Covariance2::default();
        assert!(propagate_forward(none, 0.0, -1.0, 1e-5, 0.01).is_err());
        assert!(propagate_forward(none, 0.0, 100.0, -1e-5, 0.01).is_err());
        assert!(propagate_forward(none, 0.0, 100.0, 1e-5, f64::NAN).is_err());
        assert!(propagate_forward(none, f64::NAN, 100.0, 1e-5, 0.01).is_err());
        assert!(propagate_forward(cov(-1.0, 1.0, 0.0), 0.0, 100.0, 1e-5, 0.01).is_err());
    }

    #[test]
    fn inverse_from_isotropic_end_point() {
        // Fixed start, end with sigma = 10 mm in every direction, 200 m away:
        //   sigma_distance = 0.010,  sigma_azimuth = 0.010 / 200 = 5e-5 rad,
        //   uncorrelated.
        let isotropic = cov(1e-4, 1e-4, 0.0);
        for azimuth in [0.0, 0.7, 2.0, 4.5] {
            let to = forward(c(1000.0, 1000.0), azimuth, 200.0).unwrap();
            let result =
                propagate_inverse(c(1000.0, 1000.0), to, Covariance2::default(), isotropic)
                    .unwrap();
            near(result.sigma_distance, 0.010, 1e-12);
            near(result.sigma_azimuth, 5e-5, 1e-14);
            near(result.covariance, 0.0, 1e-16);
        }
    }

    #[test]
    fn inverse_separates_along_and_across() {
        // Line due north. The end is uncertain by 3 mm north (along) and 8 mm
        // east (across); both ends contribute equally, so variances double.
        let each = cov(9e-6, 64e-6, 0.0);
        let result = propagate_inverse(c(0.0, 0.0), c(400.0, 0.0), each, each).unwrap();
        near(result.sigma_distance, 0.003 * 2.0_f64.sqrt(), 1e-14);
        near(result.sigma_azimuth, 0.008 * 2.0_f64.sqrt() / 400.0, 1e-16);

        // coincident
        assert!(propagate_inverse(c(1.0, 1.0), c(1.0, 1.0), each, each).is_err());
        assert!(propagate_inverse(c(0.0, 0.0), c(1.0, 1.0), cov(-1.0, 0.0, 0.0), each).is_err());
    }

    #[test]
    fn forward_then_inverse_recovers_the_observation_sigmas() {
        // Propagate sigma_az, sigma_d out to a point from an errorless start,
        // then back: the round trip must return exactly what went in.
        let azimuth = 1.234;
        let distance = 321.0;
        let sigma_azimuth = 3e-5;
        let sigma_distance = 0.004;
        let start = c(500.0, 700.0);
        let end = forward(start, azimuth, distance).unwrap();
        let covariance = propagate_forward(
            Covariance2::default(),
            azimuth,
            distance,
            sigma_azimuth,
            sigma_distance,
        )
        .unwrap();
        let back = propagate_inverse(start, end, Covariance2::default(), covariance).unwrap();
        near(back.sigma_azimuth, sigma_azimuth, 1e-15);
        near(back.sigma_distance, sigma_distance, 1e-13);
        near(back.covariance, 0.0, 1e-18);
    }

    #[test]
    fn general_law_hand_example() {
        // J = [[1,2],[3,4]], S = [[2,1],[1,3]]
        //   J S     = [[4, 7], [10, 15]]
        //   J S J^T = [[4+14, 12+28], [10+30, 30+60]] = [[18, 40], [40, 90]]
        let result = propagate_covariance(
            &matrix(&[[1.0, 2.0], [3.0, 4.0]]),
            &matrix(&[[2.0, 1.0], [1.0, 3.0]]),
        )
        .unwrap();
        assert_eq!(result, matrix(&[[18.0, 40.0], [40.0, 90.0]]));

        // A 1 x k Jacobian gives the variance of one derived quantity: the sum of
        // two independent measurements has variance s1^2 + s2^2.
        let sum =
            propagate_covariance(&matrix(&[[1.0, 1.0]]), &matrix(&[[0.04, 0.0], [0.0, 0.09]]))
                .unwrap();
        assert_eq!(sum.rows, 1);
        near(sum[(0, 0)], 0.13, 1e-16);
    }

    #[test]
    fn general_law_rejects_bad_input() {
        let j = matrix(&[[1.0, 2.0], [3.0, 4.0]]);
        // k mismatch
        assert_eq!(
            propagate_covariance(
                &j,
                &matrix(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
        // asymmetric
        assert!(propagate_covariance(&j, &matrix(&[[2.0, 1.0], [0.0, 3.0]])).is_err());
        assert!(propagate_covariance(&j, &matrix(&[[2.0, f64::NAN], [f64::NAN, 3.0]])).is_err());
        // not square
        assert!(propagate_covariance(&j, &matrix(&[[1.0, 2.0]])).is_err());
    }
}
