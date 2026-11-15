//! Twin of tests/survey/test_statistics.cpp.

mod survey_support;

mod survey_statistics {
    use crate::survey_support::{double_eq, error_code, near};
    use katana_core::ErrorCode;
    use katana_math::numerics::PI;
    use katana_survey::statistics::{chi_square_cdf, chi_square_quantile};

    // The C++ test's reference functions, std::erf and std::lgamma, are the C
    // library's; Rust's standard library has neither.
    use katana_math::libm::{erf, lgamma};

    // Independent reference for the chi-square cdf, built from closed forms
    // rather than from the incomplete gamma expansions of the implementation:
    //   k = 1:  F(x) = erf(sqrt(x/2))
    //   k = 2:  F(x) = 1 - exp(-x/2)
    //   and the recurrence  F_{k+2}(x) = F_k(x) - 2 f_{k+2}(x),
    //   f_k(x) = x^{k/2-1} e^{-x/2} / (2^{k/2} Gamma(k/2))  (the density),
    // with the density evaluated through lgamma.
    fn reference_cdf(x: f64, dof: usize) -> f64 {
        let mut cdf = if dof % 2 == 1 {
            erf((0.5 * x).sqrt())
        } else {
            1.0 - (-0.5 * x).exp()
        };
        let mut k = if dof % 2 == 1 { 1 } else { 2 };
        while k < dof {
            let half = 0.5 * (k + 2) as f64;
            let log_density = (half - 1.0) * x.ln() - 0.5 * x - half * 2.0_f64.ln() - lgamma(half);
            cdf -= 2.0 * log_density.exp();
            k += 2;
        }
        cdf
    }

    #[test]
    fn cdf_matches_closed_forms_for_small_degrees_of_freedom() {
        for x in [0.001, 0.1, 0.5, 1.0, 2.0, 3.841, 5.0, 9.0, 20.0, 50.0] {
            near(chi_square_cdf(x, 1).unwrap(), erf((0.5 * x).sqrt()), 1e-14);
            near(chi_square_cdf(x, 2).unwrap(), 1.0 - (-0.5 * x).exp(), 1e-14);
            // k = 3: erf(sqrt(x/2)) - sqrt(2x/pi) e^{-x/2}
            near(
                chi_square_cdf(x, 3).unwrap(),
                erf((0.5 * x).sqrt()) - (2.0 * x / PI).sqrt() * (-0.5 * x).exp(),
                1e-14,
            );
            // k = 4: 1 - e^{-x/2} (1 + x/2)
            near(
                chi_square_cdf(x, 4).unwrap(),
                1.0 - (-0.5 * x).exp() * (1.0 + 0.5 * x),
                1e-14,
            );
        }
    }

    #[test]
    fn cdf_matches_recurrence_reference_across_both_expansions() {
        // Arguments on both sides of x/2 = k/2 + 1 (series vs continued
        // fraction) and degrees of freedom on both sides of 32 (direct vs
        // Stirling prefactor).
        for dof in [5_usize, 10, 25, 31, 32, 33, 60, 100, 250] {
            let mean = dof as f64;
            let spread = (2.0 * mean).sqrt();
            for z in [-2.5, -1.0, -0.2, 0.0, 0.3, 1.0, 2.5] {
                let x = mean + z * spread;
                if x <= 0.0 {
                    continue;
                }
                let cdf = chi_square_cdf(x, dof).unwrap();
                // The reference subtracts up to k/2 densities, each good to
                // ~1e-15.
                near(cdf, reference_cdf(x, dof), 1e-12);
            }
        }
    }

    #[test]
    fn cdf_limits_and_monotonicity() {
        double_eq(chi_square_cdf(0.0, 3).unwrap(), 0.0);
        double_eq(chi_square_cdf(-5.0, 3).unwrap(), 0.0);
        near(chi_square_cdf(1000.0, 3).unwrap(), 1.0, 1e-15);
        // The median of chi-square(2) is 2 ln 2.
        near(chi_square_cdf(2.0 * 2.0_f64.ln(), 2).unwrap(), 0.5, 1e-15);

        for dof in [1_usize, 7, 40, 500] {
            let mut previous = 0.0;
            for i in 1..=60 {
                let x = dof as f64 * 0.05 * f64::from(i);
                let cdf = chi_square_cdf(x, dof).unwrap();
                assert!(cdf >= previous, "dof {dof} x {x}");
                assert!(cdf <= 1.0);
                previous = cdf;
            }
        }
    }

    #[test]
    fn quantile_closed_form_for_two_degrees_of_freedom() {
        // F(x) = 1 - exp(-x/2)  =>  x = -2 ln(1 - p)
        for p in [0.001, 0.025, 0.05, 0.5, 0.95, 0.975, 0.999] {
            near(
                chi_square_quantile(p, 2).unwrap(),
                -2.0 * (-p).ln_1p(),
                1e-12,
            );
        }
    }

    #[test]
    fn quantile_matches_published_critical_values() {
        // Standard chi-square table (any statistics text), given there to 3
        // decimals: (probability, degrees of freedom, value).
        let table = [
            (0.95, 1, 3.841),
            (0.975, 1, 5.024),
            (0.95, 2, 5.991),
            (0.95, 5, 11.070),
            (0.025, 10, 3.247),
            (0.95, 10, 18.307),
            (0.975, 10, 20.483),
            (0.05, 30, 18.493),
            (0.95, 30, 43.773),
        ];
        for (probability, dof, value) in table {
            let quantile = chi_square_quantile(probability, dof).unwrap();
            near(quantile, value, 5e-4);
        }
    }

    #[test]
    fn quantile_inverts_the_cdf() {
        for dof in [1_usize, 2, 3, 9, 31, 32, 120, 2000] {
            for p in [0.005, 0.025, 0.3, 0.5, 0.9, 0.975, 0.995] {
                let x = chi_square_quantile(p, dof).unwrap();
                assert!(x > 0.0);
                near(chi_square_cdf(x, dof).unwrap(), p, 1e-12);
            }
            assert!(
                chi_square_quantile(0.025, dof).unwrap() < chi_square_quantile(0.975, dof).unwrap()
            );
        }
    }

    #[test]
    fn large_degrees_of_freedom_approach_the_normal_limit() {
        // chi-square(k) has mean k and variance 2k; by the Wilson-Hilferty
        // cube-root transform its 97.5 % point is
        // k (1 - 2/(9k) + 1.959964 sqrt(2/(9k)))^3 to a few parts in 1e4 at
        // k = 5000. A coarse check that large k neither overflows nor loses the
        // tail, not a precision test.
        let k: f64 = 5000.0;
        let term = 2.0 / (9.0 * k);
        let wilson_hilferty = k * (1.0 - term + 1.959964 * term.sqrt()).powf(3.0);
        let quantile = chi_square_quantile(0.975, 5000).unwrap();
        near(quantile, wilson_hilferty, 0.05);
        near(chi_square_cdf(quantile, 5000).unwrap(), 0.975, 1e-11);
    }

    #[test]
    fn rejects_bad_arguments() {
        assert_eq!(
            error_code(chi_square_cdf(1.0, 0)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            error_code(chi_square_cdf(f64::NAN, 3)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            error_code(chi_square_cdf(f64::INFINITY, 3)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            error_code(chi_square_quantile(0.5, 0)),
            ErrorCode::InvalidArgument
        );
        for p in [0.0, 1.0, -0.1, 1.1, f64::NAN] {
            assert_eq!(
                error_code(chi_square_quantile(p, 4)),
                ErrorCode::InvalidArgument,
                "{p}"
            );
        }
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            chi_square_quantile(0.975, 17).unwrap(),
            chi_square_quantile(0.975, 17).unwrap()
        );
        assert_eq!(
            chi_square_cdf(12.5, 17).unwrap(),
            chi_square_cdf(12.5, 17).unwrap()
        );
    }
}
