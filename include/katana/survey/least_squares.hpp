#pragma once

// Weighted linear least squares by observation equations (PLAN.MD Phase 13).
//
// Model        l + v = A x,   weight matrix P = Sigma_ll^-1 (sigma0^2 = 1)
// Estimate     x minimises v^T P v
// Residuals    v = A x - l    (the surveying sign convention: observation plus
//                              residual equals the adjusted observation)
//
// Method: the system is whitened (rows scaled by sqrt(w_i), or multiplied by
// L^-1 of the Cholesky factor Sigma_ll = L L^T), its columns are scaled to unit
// length, and it is factorised by Householder QR with column pivoting. The
// normal equations are never formed and no matrix is inverted to obtain x;
// working on A rather than A^T P A keeps the condition number instead of
// squaring it. The cofactor matrix comes from triangular solves with R.
//
// The functions are pure: no global state, identical input gives bitwise
// identical output, and they may be called concurrently from any threads.

#include <cstddef>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/matrix.hpp"

namespace katana::survey {

// Dimensionless. After the columns of the whitened design matrix are scaled to
// unit length, a pivot |R_kk| of the column-pivoted QR below this fraction of
// the largest pivot marks column k as linearly dependent: the parameter cannot
// be resolved (datum defect, disconnected station). It is the project-wide
// relative tolerance: a direction of the parameter space that is "zero to within
// kRelative" of the dominant one would be determined with fewer than ~7
// significant digits in double precision, which is reported instead of returned.
inline constexpr double kRankTolerance = katana::math::tolerance::kRelative;

// Which variance factor scales the cofactor matrix Qxx into the covariance.
enum class CovarianceScaling {
    // sigma0^2 = 1: trust the a-priori standard deviations.
    APriori,
    // The estimated variance factor v^T P v / r. Falls back to 1 when r = 0,
    // where it is undefined; LeastSquaresSolution::covarianceScale tells which
    // value was applied.
    APosteriori,
};

struct LeastSquaresOptions {
    CovarianceScaling covarianceScaling = CovarianceScaling::APosteriori;
};

struct LeastSquaresSolution {
    std::vector<double> parameters; // x
    std::vector<double> residuals;  // v = A x - l, in observation units
    std::size_t degreesOfFreedom = 0;      // r = n - u
    double weightedSquaredResiduals = 0.0; // v^T P v
    // v^T P v / r; nullopt when r = 0 (uniquely determined, nothing to estimate
    // the variance factor from).
    std::optional<double> varianceFactor;
    double covarianceScale = 1.0; // factor applied to `cofactor` to give `covariance`
    Matrix cofactor;              // Qxx = (A^T P A)^-1
    Matrix covariance;            // covarianceScale * Qxx
    std::vector<double> standardDeviations; // sqrt(diag(covariance))
    // r_i = (Qvv P)_ii in [0, 1]: the share of observation i that is checked by
    // the others. They sum to r.
    std::vector<double> redundancyNumbers;
    // v_i / sigma_vi with sigma_vi = sqrt((Qvv)_ii) and sigma0 = 1 (Baarda's
    // w-statistic). nullopt for an observation without redundancy (r_i ~ 0),
    // whose residual is zero by construction and carries no information.
    std::vector<std::optional<double>> standardizedResiduals;
};

// Diagonal weights: `weights[i]` = 1 / sigma_i^2 > 0.
//   InvalidArgument    dimension mismatch, non-finite values, weight <= 0, u = 0
//   AdjustmentFailure  fewer observations than parameters, or rank deficiency;
//                      the message lists the indices of the unresolved parameters.
[[nodiscard]] katana::core::Result<LeastSquaresSolution>
solveLeastSquares(const Matrix& design, const std::vector<double>& observations,
                  const std::vector<double>& weights, const LeastSquaresOptions& options = {});

// Full observation covariance matrix Sigma_ll (symmetric positive definite,
// n x n). InvalidArgument when it is not.
[[nodiscard]] katana::core::Result<LeastSquaresSolution>
solveLeastSquaresWithCovariance(const Matrix& design, const std::vector<double>& observations,
                                const Matrix& observationCovariance,
                                const LeastSquaresOptions& options = {});

} // namespace katana::survey
