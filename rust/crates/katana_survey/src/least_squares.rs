//! Weighted linear least squares by observation equations.
//!
//! Model        l + v = A x,   weight matrix P = Sigma_ll^-1 (sigma0^2 = 1)
//! Estimate     x minimises v^T P v
//! Residuals    v = A x - l    (the surveying sign convention: observation plus
//!                              residual equals the adjusted observation)
//!
//! Method: the system is whitened (rows scaled by sqrt(w_i), or multiplied by
//! L^-1 of the Cholesky factor Sigma_ll = L L^T), its columns are scaled to unit
//! length, and it is factorised by Householder QR with column pivoting. The
//! normal equations are never formed and no matrix is inverted to obtain x;
//! working on A rather than A^T P A keeps the condition number instead of
//! squaring it. The cofactor matrix comes from triangular solves with R.
//!
//! The functions are pure: no global state, identical input gives bitwise
//! identical output, and they may be called concurrently from any threads.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::tolerance;
use nalgebra::DMatrix;
use nalgebra::linalg::Cholesky;

use crate::lsq_core::{
    RankDeficiency, Whitened, WhitenedSolution, scaled_matrix, solve_lower_matrix_in_place,
    solve_lower_vector_in_place, solve_whitened, to_matrix,
};
use crate::matrix::Matrix;

/// Dimensionless. After the columns of the whitened design matrix are scaled to
/// unit length, a pivot |R_kk| of the column-pivoted QR below this fraction of
/// the largest pivot marks column k as linearly dependent: the parameter cannot
/// be resolved (datum defect, disconnected station). It is the project-wide
/// relative tolerance: a direction of the parameter space that is "zero to
/// within RELATIVE" of the dominant one would be determined with fewer than ~7
/// significant digits in double precision, which is reported instead of
/// returned.
pub const RANK_TOLERANCE: f64 = tolerance::RELATIVE;

/// Which variance factor scales the cofactor matrix Qxx into the covariance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CovarianceScaling {
    /// sigma0^2 = 1: trust the a-priori standard deviations.
    #[default]
    APriori,
    /// The estimated variance factor v^T P v / r. Falls back to 1 when r = 0,
    /// where it is undefined; LeastSquaresSolution::covariance_scale tells which
    /// value was applied.
    APosteriori,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeastSquaresOptions {
    pub covariance_scaling: CovarianceScaling,
}

impl Default for LeastSquaresOptions {
    fn default() -> Self {
        Self {
            covariance_scaling: CovarianceScaling::APosteriori,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LeastSquaresSolution {
    /// x
    pub parameters: Vec<f64>,
    /// v = A x - l, in observation units.
    pub residuals: Vec<f64>,
    /// r = n - u
    pub degrees_of_freedom: usize,
    /// v^T P v
    pub weighted_squared_residuals: f64,
    /// v^T P v / r; None when r = 0 (uniquely determined, nothing to estimate
    /// the variance factor from).
    pub variance_factor: Option<f64>,
    /// Factor applied to `cofactor` to give `covariance`.
    pub covariance_scale: f64,
    /// Qxx = (A^T P A)^-1
    pub cofactor: Matrix,
    /// covariance_scale * Qxx
    pub covariance: Matrix,
    /// sqrt(diag(covariance))
    pub standard_deviations: Vec<f64>,
    /// r_i = (Qvv P)_ii in [0, 1]: the share of observation i that is checked by
    /// the others. They sum to r.
    pub redundancy_numbers: Vec<f64>,
    /// v_i / sigma_vi with sigma_vi = sqrt((Qvv)_ii) and sigma0 = 1 (Baarda's
    /// w-statistic). None for an observation without redundancy (r_i ~ 0),
    /// whose residual is zero by construction and carries no information.
    pub standardized_residuals: Vec<Option<f64>>,
}

impl Default for LeastSquaresSolution {
    fn default() -> Self {
        Self {
            parameters: Vec::new(),
            residuals: Vec::new(),
            degrees_of_freedom: 0,
            weighted_squared_residuals: 0.0,
            variance_factor: None,
            covariance_scale: 1.0,
            cofactor: Matrix::default(),
            covariance: Matrix::default(),
            standard_deviations: Vec::new(),
            redundancy_numbers: Vec::new(),
            standardized_residuals: Vec::new(),
        }
    }
}

fn check_system(design: &Matrix, observations: &[f64]) -> Status {
    if !design.is_consistent() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "design matrix storage does not match its dimensions",
        ));
    }
    if design.cols == 0 || design.rows == 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "design matrix needs at least one row and one column",
        ));
    }
    if observations.len() != design.rows {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "observation count does not match the design matrix",
        )
        .with_context(format!(
            "rows {}, observations {}",
            design.rows,
            observations.len()
        )));
    }
    if !design.values.iter().all(|value| value.is_finite())
        || !observations.iter().all(|value| value.is_finite())
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "design matrix or observations contain non-finite values",
        ));
    }
    if design.rows < design.cols {
        return Err(Error::new(
            ErrorCode::AdjustmentFailure,
            "fewer observations than parameters",
        )
        .with_context(format!(
            "observations {}, parameters {}",
            design.rows, design.cols
        )));
    }
    Ok(())
}

fn rank_error(deficiency: &RankDeficiency, parameter_count: usize) -> Error {
    let indices = deficiency
        .unresolved
        .iter()
        .map(|index| format!("{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    Error::new(
        ErrorCode::AdjustmentFailure,
        format!(
            "design matrix is rank deficient: rank {} of {parameter_count}; unresolved parameter indices: {indices}",
            deficiency.rank
        ),
    )
}

// The design matrix as a dense matrix (row-major storage read into nalgebra's
// column-major one).
fn dense_of(matrix: &Matrix) -> DMatrix<f64> {
    DMatrix::from_fn(matrix.rows, matrix.cols, |r, c| matrix[(r, c)])
}

// A x - l, each row's product summed in column order.
fn residuals_of(design: &Matrix, parameters: &[f64], observations: &[f64]) -> Vec<f64> {
    (0..design.rows)
        .map(|r| {
            let mut sum = 0.0;
            for (c, x) in parameters.iter().enumerate() {
                sum += design[(r, c)] * x;
            }
            sum - observations[r]
        })
        .collect()
}

fn squared_norm(values: &[f64]) -> f64 {
    values.iter().fold(0.0, |sum, value| sum + value * value)
}

// Statistics shared by both weightings. `whitened_residuals` has unit
// covariance (W^(1/2) v, or L^-1 v for a full covariance), so its squared norm
// is v^T P v.
fn fill_statistics(
    solution: &mut LeastSquaresSolution,
    core: &WhitenedSolution,
    whitened_residuals: &[f64],
    options: &LeastSquaresOptions,
) {
    let observation_count = solution.residuals.len();
    let parameter_count = solution.parameters.len();
    solution.degrees_of_freedom = observation_count - parameter_count;
    solution.weighted_squared_residuals = squared_norm(whitened_residuals);
    if solution.degrees_of_freedom > 0 {
        solution.variance_factor =
            Some(solution.weighted_squared_residuals / solution.degrees_of_freedom as f64);
    }
    solution.covariance_scale = match solution.variance_factor {
        Some(factor) if options.covariance_scaling == CovarianceScaling::APosteriori => factor,
        _ => 1.0,
    };
    solution.cofactor = to_matrix(&core.cofactor);
    solution.covariance = scaled_matrix(&core.cofactor, solution.covariance_scale);
    solution.standard_deviations = (0..parameter_count)
        .map(|i| solution.covariance[(i, i)].sqrt())
        .collect();
}

/// Diagonal weights: `weights[i]` = 1 / sigma_i^2 > 0.
///   InvalidArgument    dimension mismatch, non-finite values, weight <= 0, u = 0
///   AdjustmentFailure  fewer observations than parameters, or rank deficiency;
///                      the message lists the indices of the unresolved parameters.
pub fn solve_least_squares(
    design: &Matrix,
    observations: &[f64],
    weights: &[f64],
    options: &LeastSquaresOptions,
) -> Result<LeastSquaresSolution> {
    check_system(design, observations)?;
    if weights.len() != design.rows {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "weight count does not match the design matrix",
        )
        .with_context(format!("rows {}, weights {}", design.rows, weights.len())));
    }
    if let Some(index) = weights
        .iter()
        .position(|weight| !weight.is_finite() || !(*weight > 0.0))
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "weights must be positive and finite",
        )
        .with_context(format!("weight index {index}")));
    }

    let rows = design.rows;
    let cols = design.cols;
    let root_weights: Vec<f64> = weights.iter().map(|weight| weight.sqrt()).collect();

    let whitened_design = DMatrix::from_fn(rows, cols, |r, c| root_weights[r] * design[(r, c)]);
    let whitened_observations: Vec<f64> = root_weights
        .iter()
        .zip(observations)
        .map(|(w, l)| w * l)
        .collect();
    let core = match solve_whitened(&whitened_design, &whitened_observations) {
        Whitened::Solved(core) => core,
        Whitened::Deficient(deficiency) => return Err(rank_error(&deficiency, cols)),
    };

    let residuals = residuals_of(design, &core.parameters, observations);
    let whitened_residuals: Vec<f64> = root_weights
        .iter()
        .zip(&residuals)
        .map(|(w, v)| w * v)
        .collect();
    let mut solution = LeastSquaresSolution {
        parameters: core.parameters.clone(),
        residuals,
        ..Default::default()
    };
    fill_statistics(&mut solution, &core, &whitened_residuals, options);

    solution.redundancy_numbers = Vec::with_capacity(rows);
    solution.standardized_residuals = Vec::with_capacity(rows);
    for (leverage, whitened) in core.leverage.iter().zip(&whitened_residuals) {
        let redundancy = 1.0 - leverage;
        solution.redundancy_numbers.push(redundancy);
        let standardized = if redundancy > tolerance::RELATIVE {
            Some(whitened / redundancy.sqrt())
        } else {
            None
        };
        solution.standardized_residuals.push(standardized);
    }
    Ok(solution)
}

/// Full observation covariance matrix Sigma_ll (symmetric positive definite,
/// n x n). InvalidArgument when it is not.
pub fn solve_least_squares_with_covariance(
    design: &Matrix,
    observations: &[f64],
    observation_covariance: &Matrix,
    options: &LeastSquaresOptions,
) -> Result<LeastSquaresSolution> {
    check_system(design, observations)?;
    if !observation_covariance.is_consistent()
        || observation_covariance.rows != design.rows
        || observation_covariance.cols != design.rows
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "observation covariance must be n x n for n observations",
        ));
    }
    for i in 0..design.rows {
        for j in i..design.rows {
            let upper = observation_covariance[(i, j)];
            let lower = observation_covariance[(j, i)];
            if !upper.is_finite() || !lower.is_finite() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "observation covariance contains non-finite values",
                ));
            }
            let scale = (observation_covariance[(i, i)] * observation_covariance[(j, j)])
                .abs()
                .sqrt();
            if (upper - lower).abs() > tolerance::RELATIVE * scale {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "observation covariance is not symmetric",
                )
                .with_context(format!("entry ({i}, {j})")));
            }
        }
    }

    let rows = design.rows;
    let cols = design.cols;
    let dense = dense_of(design);
    let sigma_dense = dense_of(observation_covariance);

    // Whitening with the Cholesky factor: Sigma = L L^T, so L^-1 l has unit
    // covariance. The factorisation reads the lower triangle only, as Eigen's
    // LLT does, and refuses a matrix whose pivot is not positive, as that does.
    let Some(cholesky) = Cholesky::new(sigma_dense.clone()) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "observation covariance is not positive definite",
        ));
    };
    let lower = cholesky.l_dirty();
    let mut whitened_design = dense.clone();
    solve_lower_matrix_in_place(lower, &mut whitened_design);
    let mut whitened_observations = observations.to_vec();
    solve_lower_vector_in_place(lower, &mut whitened_observations);
    let core = match solve_whitened(&whitened_design, &whitened_observations) {
        Whitened::Solved(core) => core,
        Whitened::Deficient(deficiency) => return Err(rank_error(&deficiency, cols)),
    };

    let residuals = residuals_of(design, &core.parameters, observations);
    let mut whitened_residuals = residuals.clone();
    solve_lower_vector_in_place(lower, &mut whitened_residuals);
    let mut solution = LeastSquaresSolution {
        parameters: core.parameters.clone(),
        residuals,
        ..Default::default()
    };
    fill_statistics(&mut solution, &core, &whitened_residuals, options);

    // Qvv = Sigma - A Qxx A^T; r_i = (Qvv P)_ii = 1 - (A Qxx A^T Sigma^-1)_ii.
    // With correlated observations r_i is not confined to [0, 1].
    // A Qxx
    let mut design_cofactor = DMatrix::<f64>::zeros(rows, cols);
    for r in 0..rows {
        for c in 0..cols {
            let mut sum = 0.0;
            for k in 0..cols {
                sum += dense[(r, k)] * core.cofactor[(k, c)];
            }
            design_cofactor[(r, c)] = sum;
        }
    }
    // Sigma^-1 A
    let weighted_design = cholesky.solve(&dense);
    let row_dot = |left: &DMatrix<f64>, right: &DMatrix<f64>, row: usize| {
        let mut sum = 0.0;
        for c in 0..cols {
            sum += left[(row, c)] * right[(row, c)];
        }
        sum
    };
    solution.redundancy_numbers = Vec::with_capacity(rows);
    solution.standardized_residuals = Vec::with_capacity(rows);
    for i in 0..rows {
        solution
            .redundancy_numbers
            .push(1.0 - row_dot(&design_cofactor, &weighted_design, i));
        let residual_cofactor = sigma_dense[(i, i)] - row_dot(&design_cofactor, &dense, i);
        let standardized = if residual_cofactor > tolerance::RELATIVE * sigma_dense[(i, i)] {
            Some(solution.residuals[i] / residual_cofactor.sqrt())
        } else {
            None
        };
        solution.standardized_residuals.push(standardized);
    }
    Ok(solution)
}
