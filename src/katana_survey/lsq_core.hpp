#pragma once

// Private numerical core shared by the linear solver and the network
// adjustments. Eigen types never leave src/katana_survey (PLAN.MD Rule 4).

#include <cstddef>
#include <variant>
#include <vector>

#include <Eigen/Dense>

#include "katana/survey/matrix.hpp"

namespace katana::survey::detail {

struct WhitenedSolution {
    Eigen::VectorXd parameters; // x
    Eigen::MatrixXd cofactor;   // Qxx = (A^T A)^-1 of the whitened system, exactly symmetric
    // Diagonal of the hat matrix A Qxx A^T (squared row norms of the thin Q), each
    // in [0, 1]. The redundancy number of whitened observation i is 1 - leverage_i.
    Eigen::VectorXd leverage;
};

struct RankDeficiency {
    std::size_t rank = 0;
    std::vector<std::size_t> unresolved; // column indices without a usable pivot, ascending
};

// Solves min |A x - l|^2 for an already whitened system (unit weights) by
// column-equilibrated, column-pivoted Householder QR. Precondition: all entries
// finite. Any shape is accepted; n < u is reported as a rank deficiency.
[[nodiscard]] std::variant<WhitenedSolution, RankDeficiency>
solveWhitened(const Eigen::MatrixXd& design, const Eigen::VectorXd& observations);

[[nodiscard]] Matrix toMatrix(const Eigen::MatrixXd& matrix);

} // namespace katana::survey::detail
