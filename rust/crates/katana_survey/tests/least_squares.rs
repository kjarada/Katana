//! Twin of tests/survey/test_least_squares.cpp.

#![allow(
    clippy::needless_range_loop,
    reason = "The reference sums index rows and columns as the C++ test's hand formulas do."
)]

mod survey_support;

mod survey_least_squares {
    use crate::survey_support::{Random, double_eq, error_code, near};
    use katana_core::ErrorCode;
    use katana_survey::Matrix;
    use katana_survey::least_squares::{
        CovarianceScaling, LeastSquaresOptions, LeastSquaresSolution, solve_least_squares,
        solve_least_squares_with_covariance,
    };

    type Mat3 = [[f64; 3]; 3];

    fn matrix_of<R: AsRef<[f64]>>(rows: &[R]) -> Matrix {
        let matrix = Matrix::from_rows(rows);
        assert!(matrix.is_ok());
        matrix.unwrap()
    }

    fn solve(
        design: &Matrix,
        observations: &[f64],
        weights: &[f64],
    ) -> katana_core::Result<LeastSquaresSolution> {
        solve_least_squares(
            design,
            observations,
            weights,
            &LeastSquaresOptions::default(),
        )
    }

    fn det3(m: &Mat3) -> f64 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    // Inverse by the adjugate (transposed cofactors): the textbook formula,
    // nothing in common with the QR factorisation under test.
    fn inverse3(m: &Mat3) -> Mat3 {
        let d = det3(m);
        let mut inv = [[0.0; 3]; 3];
        for r in 0..3 {
            for c in 0..3 {
                let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
                let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
                // Cyclic indexing yields the signed cofactor directly.
                let cofactor = m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1];
                inv[c][r] = cofactor / d;
            }
        }
        inv
    }

    // A^T P A and A^T P l for a 3-parameter system.
    fn normal_equations(a: &Matrix, l: &[f64], w: &[f64]) -> (Mat3, [f64; 3]) {
        let mut n = [[0.0; 3]; 3];
        let mut t = [0.0; 3];
        for i in 0..a.rows {
            for r in 0..3 {
                t[r] += a[(i, r)] * w[i] * l[i];
                for c in 0..3 {
                    n[r][c] += a[(i, r)] * w[i] * a[(i, c)];
                }
            }
        }
        (n, t)
    }

    // Largest |(A^T P v)_k| relative to sum_i |A_ik| w_i |v_i|, the size it
    // would have without cancellation.
    fn orthogonality_defect(a: &Matrix, w: &[f64], v: &[f64]) -> f64 {
        let mut worst: f64 = 0.0;
        for k in 0..a.cols {
            let mut sum = 0.0;
            let mut scale = 0.0;
            for i in 0..a.rows {
                sum += a[(i, k)] * w[i] * v[i];
                scale += (a[(i, k)] * w[i] * v[i]).abs();
            }
            if scale > 0.0 {
                // std::max(worst, x) is (worst < x) ? x : worst.
                let defect = sum.abs() / scale;
                if worst < defect {
                    worst = defect;
                }
            }
        }
        worst
    }

    struct RandomSystem {
        design: Matrix,
        observations: Vec<f64>,
        weights: Vec<f64>,
    }

    fn random_system(random: &mut Random) -> RandomSystem {
        let unknowns = random.integer(1, 5) as usize;
        let rows = unknowns + random.integer(1, 8) as usize;
        let mut design = Matrix::new(rows, unknowns);
        for value in &mut design.values {
            *value = random.real(-10.0, 10.0);
        }
        let mut observations = Vec::new();
        let mut weights = Vec::new();
        for _ in 0..rows {
            observations.push(random.real(-100.0, 100.0));
            weights.push(random.real(0.1, 1000.0));
        }
        RandomSystem {
            design,
            observations,
            weights,
        }
    }

    // ---- known solutions -----------------------------------------------------

    #[test]
    fn exact_line_is_recovered_with_zero_residuals() {
        // y = 1 + 2t sampled at t = 0..3.
        let a = matrix_of(&[[1.0, 0.0], [1.0, 1.0], [1.0, 2.0], [1.0, 3.0]]);
        let solution = solve(&a, &[1.0, 3.0, 5.0, 7.0], &[1.0, 1.0, 1.0, 1.0]);
        let solution = match solution {
            Ok(solution) => solution,
            Err(error) => panic!("{}", error.describe()),
        };
        near(solution.parameters[0], 1.0, 1e-13);
        near(solution.parameters[1], 2.0, 1e-13);
        for v in &solution.residuals {
            near(*v, 0.0, 1e-13);
        }
        assert_eq!(solution.degrees_of_freedom, 2);
        assert!(solution.variance_factor.is_some());
        near(solution.variance_factor.unwrap(), 0.0, 1e-25);
    }

    #[test]
    fn three_point_regression_solved_by_hand() {
        // Points (0,0) (1,1) (2,1), unit weights.
        //   t_mean = 1, y_mean = 2/3, Stt = 2, Sty = (-1)(-2/3) + 0 + (1)(1/3) = 1
        //   slope b = 1/2, intercept a = 2/3 - 1/2 = 1/6
        //   v = A x - l = (1/6, -1/3, 1/6);  v^T v = 1/6;  r = 1;  s0^2 = 1/6
        //   A^T A = [[3,3],[3,5]], det 6, inverse = [[5/6, -1/2], [-1/2, 1/2]]
        //   leverage h_i = 1/n + (t_i - t_mean)^2 / Stt = 5/6, 1/3, 5/6
        //     =>  r_i = 1/6, 2/3, 1/6
        let a = matrix_of(&[[1.0, 0.0], [1.0, 1.0], [1.0, 2.0]]);
        let a_priori = LeastSquaresOptions {
            covariance_scaling: CovarianceScaling::APriori,
        };
        let solution =
            solve_least_squares(&a, &[0.0, 1.0, 1.0], &[1.0, 1.0, 1.0], &a_priori).unwrap();

        near(solution.parameters[0], 1.0 / 6.0, 1e-14);
        near(solution.parameters[1], 0.5, 1e-14);
        near(solution.residuals[0], 1.0 / 6.0, 1e-14);
        near(solution.residuals[1], -1.0 / 3.0, 1e-14);
        near(solution.residuals[2], 1.0 / 6.0, 1e-14);
        assert_eq!(solution.degrees_of_freedom, 1);
        near(solution.weighted_squared_residuals, 1.0 / 6.0, 1e-14);
        near(solution.variance_factor.unwrap(), 1.0 / 6.0, 1e-14);

        near(solution.cofactor[(0, 0)], 5.0 / 6.0, 1e-14);
        near(solution.cofactor[(0, 1)], -0.5, 1e-14);
        near(solution.cofactor[(1, 1)], 0.5, 1e-14);
        // exactly symmetric
        assert_eq!(solution.cofactor[(0, 1)], solution.cofactor[(1, 0)]);

        // A-priori scaling: the covariance is the cofactor matrix itself.
        double_eq(solution.covariance_scale, 1.0);
        assert_eq!(solution.covariance, solution.cofactor);
        near(
            solution.standard_deviations[0],
            (5.0_f64 / 6.0).sqrt(),
            1e-14,
        );
        near(solution.standard_deviations[1], 0.5_f64.sqrt(), 1e-14);

        near(solution.redundancy_numbers[0], 1.0 / 6.0, 1e-14);
        near(solution.redundancy_numbers[1], 2.0 / 3.0, 1e-14);
        near(solution.redundancy_numbers[2], 1.0 / 6.0, 1e-14);
        // With one degree of freedom every |w_i| equals sqrt(v^T P v) = sqrt(1/6).
        assert!(solution.standardized_residuals[0].is_some());
        near(
            solution.standardized_residuals[0].unwrap(),
            (1.0_f64 / 6.0).sqrt(),
            1e-14,
        );
        near(
            solution.standardized_residuals[1].unwrap(),
            -(1.0_f64 / 6.0).sqrt(),
            1e-14,
        );
        near(
            solution.standardized_residuals[2].unwrap(),
            (1.0_f64 / 6.0).sqrt(),
            1e-14,
        );
    }

    #[test]
    fn a_posteriori_scaling_multiplies_by_the_variance_factor() {
        let a = matrix_of(&[[1.0, 0.0], [1.0, 1.0], [1.0, 2.0]]);
        // default: a posteriori
        let solution = solve(&a, &[0.0, 1.0, 1.0], &[1.0, 1.0, 1.0]).unwrap();
        near(solution.covariance_scale, 1.0 / 6.0, 1e-14);
        // Sigma = s0^2 Qxx = (1/6) [[5/6, -1/2], [-1/2, 1/2]]
        near(solution.covariance[(0, 0)], 5.0 / 36.0, 1e-14);
        near(solution.covariance[(0, 1)], -1.0 / 12.0, 1e-14);
        near(solution.covariance[(1, 1)], 1.0 / 12.0, 1e-14);
        near(
            solution.standard_deviations[1],
            (1.0_f64 / 12.0).sqrt(),
            1e-14,
        );
        // the cofactor is unscaled
        near(solution.cofactor[(0, 0)], 5.0 / 6.0, 1e-14);
    }

    #[test]
    fn weighted_mean_of_direct_observations() {
        // Two measurements of one quantity: 1.000 (sigma 2 mm) and 1.006 (sigma 4 mm).
        //   w = 250000, 62500;  x = (250000*1.000 + 62500*1.006) / 312500 = 1.0012
        //   v = (+0.0012, -0.0048);  v^T P v = 0.36 + 1.44 = 1.80;  Qxx = 1/312500 = 3.2e-6
        //   s_x (a posteriori) = sqrt(1.8 * 3.2e-6) = 0.0024
        //   r_1 = 1 - 250000/312500 = 0.2, r_2 = 0.8
        let a = matrix_of(&[[1.0], [1.0]]);
        let solution = solve(&a, &[1.000, 1.006], &[250000.0, 62500.0]).unwrap();
        near(solution.parameters[0], 1.0012, 1e-14);
        near(solution.residuals[0], 0.0012, 1e-14);
        near(solution.residuals[1], -0.0048, 1e-14);
        near(solution.weighted_squared_residuals, 1.80, 1e-10);
        near(solution.variance_factor.unwrap(), 1.80, 1e-10);
        near(solution.cofactor[(0, 0)], 3.2e-6, 1e-19);
        near(solution.standard_deviations[0], 0.0024, 1e-12);
        near(solution.redundancy_numbers[0], 0.2, 1e-14);
        near(solution.redundancy_numbers[1], 0.8, 1e-14);
    }

    // ---- independent formulation ---------------------------------------------

    #[test]
    fn weighted_redundant_case_against_normal_equations_by_cofactors() {
        // Parabola y = c0 + c1 t + c2 t^2 through 6 noisy, unequally weighted points.
        let t = [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0];
        let l = [7.3, 2.1, 0.8, 3.2, 8.9, 19.4];
        let w = [4.0, 1.0, 25.0, 1.0, 0.25, 16.0];
        let mut a = Matrix::new(6, 3);
        for (i, ti) in t.iter().enumerate() {
            a[(i, 0)] = 1.0;
            a[(i, 1)] = *ti;
            a[(i, 2)] = ti * ti;
        }

        // Reference: N x = t by Cramer's rule, Qxx = N^-1 by the adjugate.
        let (n, rhs) = normal_equations(&a, &l, &w);
        let d = det3(&n);
        let mut expected = [0.0; 3];
        for (k, value) in expected.iter_mut().enumerate() {
            let mut replaced = n;
            for (r, row) in replaced.iter_mut().enumerate() {
                row[k] = rhs[r];
            }
            *value = det3(&replaced) / d;
        }
        let expected_cofactor = inverse3(&n);
        let mut expected_vtpv = 0.0;
        let mut expected_residuals = Vec::new();
        for i in 0..6 {
            let v = expected[0] + expected[1] * t[i] + expected[2] * t[i] * t[i] - l[i];
            expected_residuals.push(v);
            expected_vtpv += w[i] * v * v;
        }

        let solution = solve(&a, &l, &w).unwrap();
        assert_eq!(solution.degrees_of_freedom, 3);
        for k in 0..3 {
            near(solution.parameters[k], expected[k], 1e-12);
            for c in 0..3 {
                near(
                    solution.cofactor[(k, c)],
                    expected_cofactor[k][c],
                    1e-12 * expected_cofactor[k][c].abs() + 1e-15,
                );
            }
        }
        for (i, expected_residual) in expected_residuals.iter().enumerate() {
            near(solution.residuals[i], *expected_residual, 1e-12);
        }
        near(solution.weighted_squared_residuals, expected_vtpv, 1e-11);
        near(
            solution.variance_factor.unwrap(),
            expected_vtpv / 3.0,
            1e-11,
        );
        for k in 0..3 {
            near(
                solution.standard_deviations[k],
                (expected_vtpv / 3.0 * expected_cofactor[k][k]).sqrt(),
                1e-12,
            );
        }
        // trace(redundancy) = degrees of freedom
        let mut redundancy = 0.0;
        for r in &solution.redundancy_numbers {
            assert!(*r >= 0.0);
            assert!(*r <= 1.0);
            redundancy += r;
        }
        near(redundancy, 3.0, 1e-12);
    }

    #[test]
    fn weighted_line_against_closed_form_sums() {
        // Weighted straight-line fit by the classical sums:
        //   D = Sw Swtt - Swt^2,  b = (Sw Swty - Swt Swy)/D,  a = (Swtt Swy - Swt Swty)/D
        //   Qxx = 1/D [[Swtt, -Swt], [-Swt, Sw]]
        let t = [1000.0, 1001.0, 1002.5, 1004.0, 1007.0];
        let y = [12.1, 12.9, 14.6, 16.2, 18.8];
        let w = [1.0, 4.0, 2.0, 0.5, 3.0];
        let (mut sw, mut swt, mut swy, mut swtt, mut swty) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut a = Matrix::new(5, 2);
        for i in 0..5 {
            a[(i, 0)] = 1.0;
            a[(i, 1)] = t[i];
            sw += w[i];
            swt += w[i] * t[i];
            swy += w[i] * y[i];
            swtt += w[i] * t[i] * t[i];
            swty += w[i] * t[i] * y[i];
        }
        let d = sw * swtt - swt * swt;
        let slope = (sw * swty - swt * swy) / d;

        let solution = solve(&a, &y, &w).unwrap();
        // Abscissae near 1000 make the two columns nearly parallel (the sums lose
        // ~6 digits to cancellation; the QR does not), hence the looser bound.
        near(solution.parameters[1], slope, 1e-8);
        near(solution.cofactor[(1, 1)], sw / d, 1e-8 * sw / d);
        // The fitted line passes through the weighted centroid exactly.
        near(
            solution.parameters[0] + solution.parameters[1] * (swt / sw),
            swy / sw,
            1e-9,
        );
    }

    // ---- properties ----------------------------------------------------------

    #[test]
    fn property_residuals_are_orthogonal_to_the_column_space() {
        let mut random = Random::default();
        for _ in 0..200 {
            let system = random_system(&mut random);
            let solution = solve(&system.design, &system.observations, &system.weights);
            let solution = match solution {
                Ok(solution) => solution,
                Err(error) => panic!("{}", error.describe()),
            };
            // A^T P v = 0 is the defining condition of the minimum.
            assert!(
                orthogonality_defect(&system.design, &system.weights, &solution.residuals) < 1e-10
            );

            let redundancy: f64 = solution.redundancy_numbers.iter().sum();
            near(redundancy, solution.degrees_of_freedom as f64, 1e-10);
        }
    }

    #[test]
    fn property_scaling_all_weights_leaves_the_estimate_unchanged() {
        let mut random = Random::default();
        for _ in 0..200 {
            let system = random_system(&mut random);
            let factor = random.real(0.01, 500.0);
            let scaled: Vec<f64> = system.weights.iter().map(|w| w * factor).collect();
            let base = solve(&system.design, &system.observations, &system.weights).unwrap();
            let other = solve(&system.design, &system.observations, &scaled).unwrap();
            for k in 0..base.parameters.len() {
                // std::max(1.0, |x|) is (1.0 < |x|) ? |x| : 1.0.
                let magnitude = base.parameters[k].abs();
                let scale = if 1.0 < magnitude { magnitude } else { 1.0 };
                // x is unchanged ...
                near(other.parameters[k], base.parameters[k], 1e-10 * scale);
                // ... Qxx shrinks by the factor, and the a-posteriori covariance,
                // being s0^2 Qxx, does not move: relative weights are all that
                // matter.
                near(
                    other.cofactor[(k, k)] * factor,
                    base.cofactor[(k, k)],
                    1e-9 * base.cofactor[(k, k)],
                );
                near(
                    other.standard_deviations[k],
                    base.standard_deviations[k],
                    1e-9 * base.standard_deviations[k],
                );
            }
            // v^T P v and the variance factor scale with the weights.
            near(
                other.variance_factor.unwrap(),
                base.variance_factor.unwrap() * factor,
                1e-9 * base.variance_factor.unwrap() * factor,
            );
        }
    }

    #[test]
    fn property_deterministic() {
        let mut random = Random::default();
        for _ in 0..50 {
            let system = random_system(&mut random);
            let first = solve(&system.design, &system.observations, &system.weights).unwrap();
            let second = solve(&system.design, &system.observations, &system.weights).unwrap();
            // Bitwise: == on f64, no tolerance.
            assert_eq!(first.parameters, second.parameters);
            assert_eq!(first.residuals, second.residuals);
            assert_eq!(first.cofactor, second.cofactor);
            assert_eq!(first.covariance, second.covariance);
            assert_eq!(first.redundancy_numbers, second.redundancy_numbers);
            assert_eq!(
                first.weighted_squared_residuals,
                second.weighted_squared_residuals
            );
        }
    }

    // ---- degrees of freedom --------------------------------------------------

    #[test]
    fn uniquely_determined_system_has_no_variance_factor() {
        // 2 equations, 2 unknowns: x + y = 3, x - y = 1  =>  x = 2, y = 1.
        let a = matrix_of(&[[1.0, 1.0], [1.0, -1.0]]);
        let solution = solve(&a, &[3.0, 1.0], &[1.0, 4.0]).unwrap();
        near(solution.parameters[0], 2.0, 1e-14);
        near(solution.parameters[1], 1.0, 1e-14);
        assert_eq!(solution.degrees_of_freedom, 0);
        // 0/0 is undefined, not zero and not NaN smuggled into a double.
        assert!(solution.variance_factor.is_none());
        // falls back to a priori, and says so
        double_eq(solution.covariance_scale, 1.0);
        assert_eq!(solution.covariance, solution.cofactor);
        for i in 0..2 {
            near(solution.residuals[i], 0.0, 1e-14);
            near(solution.redundancy_numbers[i], 0.0, 1e-14);
            // nothing checks them
            assert!(solution.standardized_residuals[i].is_none());
        }
        // Qxx = (A^T P A)^-1 with A^T P A = [[5, -3], [-3, 5]], det 16.
        near(solution.cofactor[(0, 0)], 5.0 / 16.0, 1e-14);
        near(solution.cofactor[(0, 1)], 3.0 / 16.0, 1e-14);
    }

    #[test]
    fn unchecked_observation_has_no_standardized_residual() {
        // x1 is measured twice, x2 once: the third observation has zero redundancy.
        let a = matrix_of(&[[1.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        let solution = solve(&a, &[10.0, 10.2, 5.0], &[1.0, 1.0, 1.0]).unwrap();
        near(solution.parameters[0], 10.1, 1e-14);
        near(solution.parameters[1], 5.0, 1e-14);
        near(solution.redundancy_numbers[0], 0.5, 1e-14);
        near(solution.redundancy_numbers[2], 0.0, 1e-14);
        assert!(solution.standardized_residuals[0].is_some());
        assert!(solution.standardized_residuals[2].is_none());
    }

    // ---- failures ------------------------------------------------------------

    #[test]
    fn rank_deficiency_is_reported_with_the_unresolved_parameters() {
        // Columns 0 and 2 are identical: only their sum is determined.
        let duplicate = matrix_of(&[
            [1.0, 2.0, 1.0],
            [1.0, 3.0, 1.0],
            [1.0, 5.0, 1.0],
            [1.0, 7.0, 1.0],
        ]);
        let error = solve(&duplicate, &[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0, 1.0, 1.0]).unwrap_err();
        assert_eq!(error.code, ErrorCode::AdjustmentFailure);
        assert!(error.message.contains("rank 2 of 3"), "{}", error.message);

        // A parameter no observation touches.
        let zero_column = matrix_of(&[[1.0, 0.0], [2.0, 0.0], [3.0, 0.0]]);
        let untouched = solve(&zero_column, &[1.0, 2.0, 3.0], &[1.0, 1.0, 1.0]).unwrap_err();
        assert_eq!(untouched.code, ErrorCode::AdjustmentFailure);
        assert!(
            untouched.message.contains("indices: 1"),
            "{}",
            untouched.message
        );

        // Linear dependence that is not visible column by column: c2 = c0 + c1.
        let combination = matrix_of(&[
            [1.0, 2.0, 3.0],
            [4.0, 5.0, 9.0],
            [7.0, 8.0, 15.0],
            [2.0, 2.0, 4.0],
        ]);
        assert_eq!(
            error_code(solve(
                &combination,
                &[1.0, 2.0, 3.0, 4.0],
                &[1.0, 1.0, 1.0, 1.0]
            )),
            ErrorCode::AdjustmentFailure
        );
    }

    #[test]
    fn rank_decision_is_independent_of_column_units() {
        // The same well-posed line fit with the slope column in wildly different
        // units (t in seconds vs in years) must solve, not be mistaken for
        // singular.
        let a = matrix_of(&[[1.0, 1e-9], [1.0, 2e-9], [1.0, 3e-9]]);
        let solution = match solve(&a, &[1.0, 3.0, 5.0], &[1.0, 1.0, 1.0]) {
            Ok(solution) => solution,
            Err(error) => panic!("{}", error.describe()),
        };
        near(solution.parameters[0], -1.0, 1e-9);
        near(solution.parameters[1], 2e9, 1.0);
    }

    #[test]
    fn fewer_observations_than_parameters() {
        let a = matrix_of(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(
            error_code(solve(&a, &[1.0, 2.0], &[1.0, 1.0])),
            ErrorCode::AdjustmentFailure
        );
    }

    #[test]
    fn rejects_invalid_input() {
        let a = matrix_of(&[[1.0, 0.0], [1.0, 1.0], [1.0, 2.0]]);
        let code = |design: &Matrix, l: &[f64], w: &[f64]| error_code(solve(design, l, w));
        // Zero, negative and non-finite weights (sigma = infinity, imaginary, unknown).
        assert_eq!(
            code(&a, &[0.0, 1.0, 1.0], &[1.0, 0.0, 1.0]),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&a, &[0.0, 1.0, 1.0], &[1.0, -1.0, 1.0]),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&a, &[0.0, 1.0, 1.0], &[1.0, f64::NAN, 1.0]),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&a, &[0.0, 1.0, 1.0], &[1.0, f64::INFINITY, 1.0]),
            ErrorCode::InvalidArgument
        );
        // Dimension mismatches.
        assert_eq!(
            code(&a, &[0.0, 1.0], &[1.0, 1.0, 1.0]),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&a, &[0.0, 1.0, 1.0], &[1.0, 1.0]),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&Matrix::default(), &[], &[]),
            ErrorCode::InvalidArgument
        );
        // Non-finite data.
        assert_eq!(
            code(&a, &[0.0, f64::NAN, 1.0], &[1.0, 1.0, 1.0]),
            ErrorCode::InvalidArgument
        );
        let mut poisoned = a.clone();
        poisoned[(1, 1)] = f64::NAN;
        assert_eq!(
            code(&poisoned, &[0.0, 1.0, 1.0], &[1.0, 1.0, 1.0]),
            ErrorCode::InvalidArgument
        );
        // Storage edited behind the dimensions' back.
        let mut inconsistent = a.clone();
        inconsistent.values.pop();
        assert_eq!(
            code(&inconsistent, &[0.0, 1.0, 1.0], &[1.0, 1.0, 1.0]),
            ErrorCode::InvalidArgument
        );

        assert!(Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0]]).is_err());
    }

    // ---- full covariance -----------------------------------------------------

    #[test]
    fn correlated_observations_solved_by_hand() {
        // Two correlated measurements of one quantity, Sigma = [[4, 1], [1, 2]]:
        //   Sigma^-1 = 1/7 [[2, -1], [-1, 4]]
        //   x = (1^T S^-1 l) / (1^T S^-1 1) = ((2-1) 10 + (4-1) 12) / (2 - 1 - 1 + 4) = 46/4 = 11.5
        //   Qxx = 7/4;  v = (1.5, -0.5);  v^T S^-1 v = (2*2.25 + 2*0.75 + 4*0.25)/7 = 1
        let a = matrix_of(&[[1.0], [1.0]]);
        let sigma = matrix_of(&[[4.0, 1.0], [1.0, 2.0]]);
        let solution = match solve_least_squares_with_covariance(
            &a,
            &[10.0, 12.0],
            &sigma,
            &LeastSquaresOptions::default(),
        ) {
            Ok(solution) => solution,
            Err(error) => panic!("{}", error.describe()),
        };
        near(solution.parameters[0], 11.5, 1e-13);
        near(solution.cofactor[(0, 0)], 1.75, 1e-13);
        near(solution.residuals[0], 1.5, 1e-13);
        near(solution.residuals[1], -0.5, 1e-13);
        near(solution.weighted_squared_residuals, 1.0, 1e-13);
        near(solution.variance_factor.unwrap(), 1.0, 1e-13);
        // Qvv = Sigma - A Qxx A^T = [[2.25, -0.75], [-0.75, 0.25]]; w_i = v_i / sqrt(Qvv_ii).
        near(
            solution.standardized_residuals[0].unwrap(),
            // 1.5 / 1.5
            1.0,
            1e-12,
        );
        near(
            solution.standardized_residuals[1].unwrap(),
            -0.5 / 0.5,
            1e-12,
        );
        // r = diag(Qvv Sigma^-1) = (2.25*2 + 0.75, 0.75 + 0.25*4)/7 = (0.75, 0.25); sums to 1.
        near(solution.redundancy_numbers[0], 0.75, 1e-12);
        near(solution.redundancy_numbers[1], 0.25, 1e-12);
    }

    #[test]
    fn diagonal_covariance_agrees_with_weights() {
        let mut random = Random::default();
        for _ in 0..50 {
            let system = random_system(&mut random);
            let n = system.observations.len();
            let mut sigma = Matrix::new(n, n);
            for (k, weight) in system.weights.iter().enumerate() {
                sigma[(k, k)] = 1.0 / weight;
            }
            let by_weights = solve(&system.design, &system.observations, &system.weights).unwrap();
            let by_covariance = solve_least_squares_with_covariance(
                &system.design,
                &system.observations,
                &sigma,
                &LeastSquaresOptions::default(),
            )
            .unwrap();
            for k in 0..by_weights.parameters.len() {
                let magnitude = by_weights.parameters[k].abs();
                let scale = if 1.0 < magnitude { magnitude } else { 1.0 };
                near(
                    by_covariance.parameters[k],
                    by_weights.parameters[k],
                    1e-10 * scale,
                );
            }
            let magnitude = by_weights.weighted_squared_residuals;
            let scale = if 1.0 < magnitude { magnitude } else { 1.0 };
            near(
                by_covariance.weighted_squared_residuals,
                by_weights.weighted_squared_residuals,
                1e-9 * scale,
            );
            for k in 0..n {
                near(
                    by_covariance.redundancy_numbers[k],
                    by_weights.redundancy_numbers[k],
                    1e-10,
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_covariance() {
        let a = matrix_of(&[[1.0], [1.0]]);
        let code = |sigma: &Matrix| {
            error_code(solve_least_squares_with_covariance(
                &a,
                &[10.0, 12.0],
                sigma,
                &LeastSquaresOptions::default(),
            ))
        };
        // asymmetric
        assert_eq!(
            code(&matrix_of(&[[4.0, 1.0], [2.0, 2.0]])),
            ErrorCode::InvalidArgument
        );
        // indefinite
        assert_eq!(
            code(&matrix_of(&[[1.0, 2.0], [2.0, 1.0]])),
            ErrorCode::InvalidArgument
        );
        // singular
        assert_eq!(
            code(&matrix_of(&[[1.0, 0.0], [0.0, 0.0]])),
            ErrorCode::InvalidArgument
        );
        // negative variance
        assert_eq!(
            code(&matrix_of(&[[1.0, 0.0], [0.0, -1.0]])),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&matrix_of(&[
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0]
            ])),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&matrix_of(&[[1.0, f64::NAN], [f64::NAN, 1.0]])),
            ErrorCode::InvalidArgument
        );
    }
}
