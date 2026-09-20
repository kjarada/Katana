#include "katana/survey/least_squares.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "lsq_core.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
namespace tol = katana::math::tolerance;

namespace detail {

std::variant<WhitenedSolution, RankDeficiency> solveWhitened(const Eigen::MatrixXd& design,
                                                             const Eigen::VectorXd& observations)
{
    const Eigen::Index parameterCount = design.cols();

    // Column equilibration. Survey design matrices mix units (metres per metre,
    // radians per metre, pure ones for orientation-like parameters); scaling every
    // column to unit length makes the rank decision independent of those units and
    // minimises the condition number over all column scalings to within sqrt(u).
    Eigen::VectorXd scale(parameterCount);
    for (Eigen::Index j = 0; j < parameterCount; ++j) {
        const double norm = design.col(j).norm();
        scale[j] = norm > 0.0 ? 1.0 / norm : 1.0; // a zero column stays zero and is rejected below
    }
    const Eigen::MatrixXd scaled = design * scale.asDiagonal();

    Eigen::ColPivHouseholderQR<Eigen::MatrixXd> qr(scaled);
    qr.setThreshold(kRankTolerance);
    const Eigen::Index rank = qr.rank();
    if (rank < parameterCount) {
        RankDeficiency deficiency;
        deficiency.rank = static_cast<std::size_t>(rank);
        const auto& permutation = qr.colsPermutation().indices();
        for (Eigen::Index k = rank; k < parameterCount; ++k) {
            deficiency.unresolved.push_back(static_cast<std::size_t>(permutation[k]));
        }
        std::sort(deficiency.unresolved.begin(), deficiency.unresolved.end());
        return deficiency;
    }

    WhitenedSolution solution;
    const Eigen::VectorXd scaledParameters = qr.solve(observations);
    solution.parameters = scale.cwiseProduct(scaledParameters);

    // (As^T As)^-1 = Pi R^-1 R^-T Pi^T. R^-1 comes from a triangular solve
    // against the identity; nothing is inverted through the normal equations.
    const Eigen::MatrixXd inverseR =
        qr.matrixR()
            .topLeftCorner(parameterCount, parameterCount)
            .template triangularView<Eigen::Upper>()
            .solve(Eigen::MatrixXd::Identity(parameterCount, parameterCount));
    const Eigen::MatrixXd permuted = inverseR * inverseR.transpose();
    const Eigen::MatrixXd unpermuted =
        qr.colsPermutation() * permuted * qr.colsPermutation().transpose();

    solution.cofactor.resize(parameterCount, parameterCount);
    for (Eigen::Index i = 0; i < parameterCount; ++i) {
        for (Eigen::Index j = i; j < parameterCount; ++j) {
            // Upper triangle mirrored: exactly symmetric whatever the rounding.
            const double value = scale[i] * scale[j] * unpermuted(i, j);
            solution.cofactor(i, j) = value;
            solution.cofactor(j, i) = value;
        }
    }

    const Eigen::MatrixXd thinQ =
        qr.householderQ() * Eigen::MatrixXd::Identity(design.rows(), parameterCount);
    solution.leverage = thinQ.rowwise().squaredNorm();
    return solution;
}

Matrix toMatrix(const Eigen::MatrixXd& matrix)
{
    Matrix result(static_cast<std::size_t>(matrix.rows()), static_cast<std::size_t>(matrix.cols()));
    for (Eigen::Index r = 0; r < matrix.rows(); ++r) {
        for (Eigen::Index c = 0; c < matrix.cols(); ++c) {
            result(static_cast<std::size_t>(r), static_cast<std::size_t>(c)) = matrix(r, c);
        }
    }
    return result;
}

} // namespace detail

namespace {

using RowMajorMatrix = Eigen::Matrix<double, Eigen::Dynamic, Eigen::Dynamic, Eigen::RowMajor>;

Status checkSystem(const Matrix& design, const std::vector<double>& observations)
{
    if (!design.isConsistent()) {
        return makeError(ErrorCode::InvalidArgument,
                         "design matrix storage does not match its dimensions");
    }
    if (design.cols == 0 || design.rows == 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "design matrix needs at least one row and one column");
    }
    if (observations.size() != design.rows) {
        return makeError(ErrorCode::InvalidArgument,
                         "observation count does not match the design matrix",
                         "rows " + std::to_string(design.rows) + ", observations " +
                             std::to_string(observations.size()));
    }
    const auto finite = [](double value) { return std::isfinite(value); };
    if (!std::all_of(design.values.begin(), design.values.end(), finite) ||
        !std::all_of(observations.begin(), observations.end(), finite)) {
        return makeError(ErrorCode::InvalidArgument,
                         "design matrix or observations contain non-finite values");
    }
    if (design.rows < design.cols) {
        return makeError(ErrorCode::AdjustmentFailure,
                         "fewer observations than parameters",
                         "observations " + std::to_string(design.rows) + ", parameters " +
                             std::to_string(design.cols));
    }
    return {};
}

katana::core::Error rankError(const detail::RankDeficiency& deficiency, std::size_t parameterCount)
{
    std::string indices;
    for (const std::size_t index : deficiency.unresolved) {
        indices += indices.empty() ? "" : ", ";
        indices += std::to_string(index);
    }
    return makeError(ErrorCode::AdjustmentFailure,
                     "design matrix is rank deficient: rank " + std::to_string(deficiency.rank) +
                         " of " + std::to_string(parameterCount) +
                         "; unresolved parameter indices: " + indices);
}

// Statistics shared by both weightings. `whitenedResiduals` has unit covariance
// (W^(1/2) v, or L^-1 v for a full covariance), so its squared norm is v^T P v.
void fillStatistics(LeastSquaresSolution& solution, const detail::WhitenedSolution& core,
                    const Eigen::VectorXd& whitenedResiduals, const LeastSquaresOptions& options)
{
    const std::size_t observationCount = solution.residuals.size();
    const std::size_t parameterCount = solution.parameters.size();
    solution.degreesOfFreedom = observationCount - parameterCount;
    solution.weightedSquaredResiduals = whitenedResiduals.squaredNorm();
    if (solution.degreesOfFreedom > 0) {
        solution.varianceFactor =
            solution.weightedSquaredResiduals / static_cast<double>(solution.degreesOfFreedom);
    }
    solution.covarianceScale =
        options.covarianceScaling == CovarianceScaling::APosteriori && solution.varianceFactor
            ? *solution.varianceFactor
            : 1.0;
    solution.cofactor = detail::toMatrix(core.cofactor);
    solution.covariance = detail::toMatrix(core.cofactor * solution.covarianceScale);
    solution.standardDeviations.resize(parameterCount);
    for (std::size_t i = 0; i < parameterCount; ++i) {
        solution.standardDeviations[i] = std::sqrt(solution.covariance(i, i));
    }
}

} // namespace

Result<LeastSquaresSolution> solveLeastSquares(const Matrix& design,
                                               const std::vector<double>& observations,
                                               const std::vector<double>& weights,
                                               const LeastSquaresOptions& options)
{
    if (Status status = checkSystem(design, observations); !status) {
        return status.error();
    }
    if (weights.size() != design.rows) {
        return makeError(ErrorCode::InvalidArgument,
                         "weight count does not match the design matrix",
                         "rows " + std::to_string(design.rows) + ", weights " +
                             std::to_string(weights.size()));
    }
    for (std::size_t i = 0; i < weights.size(); ++i) {
        if (!std::isfinite(weights[i]) || !(weights[i] > 0.0)) {
            return makeError(ErrorCode::InvalidArgument, "weights must be positive and finite",
                             "weight index " + std::to_string(i));
        }
    }

    const auto rows = static_cast<Eigen::Index>(design.rows);
    const auto cols = static_cast<Eigen::Index>(design.cols);
    const Eigen::Map<const RowMajorMatrix> a(design.values.data(), rows, cols);
    const Eigen::Map<const Eigen::VectorXd> l(observations.data(), rows);
    const Eigen::VectorXd rootWeights =
        Eigen::Map<const Eigen::VectorXd>(weights.data(), rows).cwiseSqrt();

    const Eigen::MatrixXd whitenedDesign = rootWeights.asDiagonal() * a;
    const Eigen::VectorXd whitenedObservations = rootWeights.cwiseProduct(l);
    const auto solved = detail::solveWhitened(whitenedDesign, whitenedObservations);
    if (const auto* deficiency = std::get_if<detail::RankDeficiency>(&solved)) {
        return rankError(*deficiency, design.cols);
    }
    const auto& core = std::get<detail::WhitenedSolution>(solved);

    LeastSquaresSolution solution;
    const Eigen::VectorXd residuals = a * core.parameters - l;
    const Eigen::VectorXd whitenedResiduals = rootWeights.cwiseProduct(residuals);
    solution.parameters.assign(core.parameters.data(), core.parameters.data() + cols);
    solution.residuals.assign(residuals.data(), residuals.data() + rows);
    fillStatistics(solution, core, whitenedResiduals, options);

    solution.redundancyNumbers.resize(design.rows);
    solution.standardizedResiduals.resize(design.rows);
    for (Eigen::Index i = 0; i < rows; ++i) {
        const double redundancy = 1.0 - core.leverage[i];
        const auto index = static_cast<std::size_t>(i);
        solution.redundancyNumbers[index] = redundancy;
        if (redundancy > tol::kRelative) {
            solution.standardizedResiduals[index] = whitenedResiduals[i] / std::sqrt(redundancy);
        }
    }
    return solution;
}

Result<LeastSquaresSolution> solveLeastSquaresWithCovariance(
    const Matrix& design, const std::vector<double>& observations,
    const Matrix& observationCovariance, const LeastSquaresOptions& options)
{
    if (Status status = checkSystem(design, observations); !status) {
        return status.error();
    }
    if (!observationCovariance.isConsistent() || observationCovariance.rows != design.rows ||
        observationCovariance.cols != design.rows) {
        return makeError(ErrorCode::InvalidArgument,
                         "observation covariance must be n x n for n observations");
    }
    for (std::size_t i = 0; i < design.rows; ++i) {
        for (std::size_t j = i; j < design.rows; ++j) {
            const double upper = observationCovariance(i, j);
            const double lower = observationCovariance(j, i);
            if (!std::isfinite(upper) || !std::isfinite(lower)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "observation covariance contains non-finite values");
            }
            const double scale =
                std::sqrt(std::abs(observationCovariance(i, i) * observationCovariance(j, j)));
            if (std::abs(upper - lower) > tol::kRelative * scale) {
                return makeError(ErrorCode::InvalidArgument,
                                 "observation covariance is not symmetric",
                                 "entry (" + std::to_string(i) + ", " + std::to_string(j) + ")");
            }
        }
    }

    const auto rows = static_cast<Eigen::Index>(design.rows);
    const auto cols = static_cast<Eigen::Index>(design.cols);
    const Eigen::Map<const RowMajorMatrix> a(design.values.data(), rows, cols);
    const Eigen::Map<const Eigen::VectorXd> l(observations.data(), rows);
    const Eigen::Map<const RowMajorMatrix> sigma(observationCovariance.values.data(), rows, rows);

    // Whitening with the Cholesky factor: Sigma = L L^T, so L^-1 l has unit
    // covariance. LLT reads the lower triangle only.
    const Eigen::MatrixXd sigmaDense = sigma;
    const Eigen::LLT<Eigen::MatrixXd> cholesky(sigmaDense);
    if (cholesky.info() != Eigen::Success) {
        return makeError(ErrorCode::InvalidArgument,
                         "observation covariance is not positive definite");
    }
    const Eigen::MatrixXd dense = a;
    const Eigen::VectorXd denseObservations = l;
    const Eigen::MatrixXd whitenedDesign = cholesky.matrixL().solve(dense);
    const Eigen::VectorXd whitenedObservations = cholesky.matrixL().solve(denseObservations);
    const auto solved = detail::solveWhitened(whitenedDesign, whitenedObservations);
    if (const auto* deficiency = std::get_if<detail::RankDeficiency>(&solved)) {
        return rankError(*deficiency, design.cols);
    }
    const auto& core = std::get<detail::WhitenedSolution>(solved);

    LeastSquaresSolution solution;
    const Eigen::VectorXd residuals = dense * core.parameters - denseObservations;
    const Eigen::VectorXd whitenedResiduals = cholesky.matrixL().solve(residuals);
    solution.parameters.assign(core.parameters.data(), core.parameters.data() + cols);
    solution.residuals.assign(residuals.data(), residuals.data() + rows);
    fillStatistics(solution, core, whitenedResiduals, options);

    // Qvv = Sigma - A Qxx A^T; r_i = (Qvv P)_ii = 1 - (A Qxx A^T Sigma^-1)_ii.
    // With correlated observations r_i is not confined to [0, 1].
    const Eigen::MatrixXd designCofactor = dense * core.cofactor; // A Qxx
    const Eigen::MatrixXd weightedDesign = cholesky.solve(dense); // Sigma^-1 A
    solution.redundancyNumbers.resize(design.rows);
    solution.standardizedResiduals.resize(design.rows);
    for (Eigen::Index i = 0; i < rows; ++i) {
        const auto index = static_cast<std::size_t>(i);
        solution.redundancyNumbers[index] =
            1.0 - designCofactor.row(i).dot(weightedDesign.row(i));
        const double residualCofactor = sigmaDense(i, i) - designCofactor.row(i).dot(dense.row(i));
        if (residualCofactor > tol::kRelative * sigmaDense(i, i)) {
            solution.standardizedResiduals[index] = residuals[i] / std::sqrt(residualCofactor);
        }
    }
    return solution;
}

} // namespace katana::survey
