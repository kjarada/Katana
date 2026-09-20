#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <limits>
#include <vector>

#include "katana/survey/least_squares.hpp"
#include "support/property.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::test::Random;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

Matrix matrixOf(const std::vector<std::vector<double>>& rows)
{
    auto matrix = Matrix::fromRows(rows);
    EXPECT_TRUE(matrix.ok());
    return *matrix;
}

using Mat3 = std::array<std::array<double, 3>, 3>;

double det3(const Mat3& m)
{
    return m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) -
           m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) +
           m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
}

// Inverse by the adjugate (transposed cofactors): the textbook formula, nothing
// in common with the QR factorisation under test.
Mat3 inverse3(const Mat3& m)
{
    const double d = det3(m);
    Mat3 inv{};
    for (int r = 0; r < 3; ++r) {
        for (int c = 0; c < 3; ++c) {
            const int r1 = (r + 1) % 3, r2 = (r + 2) % 3;
            const int c1 = (c + 1) % 3, c2 = (c + 2) % 3;
            // Cyclic indexing yields the signed cofactor directly.
            const double cofactor = m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1];
            inv[c][r] = cofactor / d;
        }
    }
    return inv;
}

// A^T P A and A^T P l for a 3-parameter system.
void normalEquations(const Matrix& a, const std::vector<double>& l, const std::vector<double>& w,
                     Mat3& n, std::array<double, 3>& t)
{
    n = Mat3{};
    t = {};
    for (std::size_t i = 0; i < a.rows; ++i) {
        for (std::size_t r = 0; r < 3; ++r) {
            t[r] += a(i, r) * w[i] * l[i];
            for (std::size_t c = 0; c < 3; ++c) {
                n[r][c] += a(i, r) * w[i] * a(i, c);
            }
        }
    }
}

// Largest |(A^T P v)_k| relative to sum_i |A_ik| w_i |v_i|, the size it would
// have without cancellation.
double orthogonalityDefect(const Matrix& a, const std::vector<double>& w,
                           const std::vector<double>& v)
{
    double worst = 0.0;
    for (std::size_t k = 0; k < a.cols; ++k) {
        double sum = 0.0;
        double scale = 0.0;
        for (std::size_t i = 0; i < a.rows; ++i) {
            sum += a(i, k) * w[i] * v[i];
            scale += std::abs(a(i, k) * w[i] * v[i]);
        }
        if (scale > 0.0) {
            worst = std::max(worst, std::abs(sum) / scale);
        }
    }
    return worst;
}

struct RandomSystem {
    Matrix design;
    std::vector<double> observations;
    std::vector<double> weights;
};

RandomSystem randomSystem(Random& random)
{
    const auto unknowns = static_cast<std::size_t>(random.integer(1, 5));
    const auto rows = unknowns + static_cast<std::size_t>(random.integer(1, 8));
    RandomSystem system;
    system.design = Matrix(rows, unknowns);
    for (double& value : system.design.values) {
        value = random.real(-10.0, 10.0);
    }
    for (std::size_t i = 0; i < rows; ++i) {
        system.observations.push_back(random.real(-100.0, 100.0));
        system.weights.push_back(random.real(0.1, 1000.0));
    }
    return system;
}

} // namespace

// ---- known solutions -------------------------------------------------------

TEST(SurveyLeastSquares, ExactLineIsRecoveredWithZeroResiduals)
{
    // y = 1 + 2t sampled at t = 0..3.
    const Matrix a = matrixOf({{1, 0}, {1, 1}, {1, 2}, {1, 3}});
    const auto solution = solveLeastSquares(a, {1, 3, 5, 7}, {1, 1, 1, 1});
    ASSERT_TRUE(solution.ok()) << solution.error().describe();
    EXPECT_NEAR(solution->parameters[0], 1.0, 1e-13);
    EXPECT_NEAR(solution->parameters[1], 2.0, 1e-13);
    for (const double v : solution->residuals) {
        EXPECT_NEAR(v, 0.0, 1e-13);
    }
    EXPECT_EQ(solution->degreesOfFreedom, 2u);
    ASSERT_TRUE(solution->varianceFactor.has_value());
    EXPECT_NEAR(*solution->varianceFactor, 0.0, 1e-25);
}

TEST(SurveyLeastSquares, ThreePointRegressionSolvedByHand)
{
    // Points (0,0) (1,1) (2,1), unit weights.
    //   t_mean = 1, y_mean = 2/3, Stt = 2, Sty = (-1)(-2/3) + 0 + (1)(1/3) = 1
    //   slope b = 1/2, intercept a = 2/3 - 1/2 = 1/6
    //   v = A x - l = (1/6, -1/3, 1/6);  v^T v = 1/6;  r = 1;  s0^2 = 1/6
    //   A^T A = [[3,3],[3,5]], det 6, inverse = [[5/6, -1/2], [-1/2, 1/2]]
    //   leverage h_i = 1/n + (t_i - t_mean)^2 / Stt = 5/6, 1/3, 5/6  =>  r_i = 1/6, 2/3, 1/6
    const Matrix a = matrixOf({{1, 0}, {1, 1}, {1, 2}});
    LeastSquaresOptions aPriori;
    aPriori.covarianceScaling = CovarianceScaling::APriori;
    const auto solution = solveLeastSquares(a, {0, 1, 1}, {1, 1, 1}, aPriori);
    ASSERT_TRUE(solution.ok());

    EXPECT_NEAR(solution->parameters[0], 1.0 / 6.0, 1e-14);
    EXPECT_NEAR(solution->parameters[1], 0.5, 1e-14);
    EXPECT_NEAR(solution->residuals[0], 1.0 / 6.0, 1e-14);
    EXPECT_NEAR(solution->residuals[1], -1.0 / 3.0, 1e-14);
    EXPECT_NEAR(solution->residuals[2], 1.0 / 6.0, 1e-14);
    EXPECT_EQ(solution->degreesOfFreedom, 1u);
    EXPECT_NEAR(solution->weightedSquaredResiduals, 1.0 / 6.0, 1e-14);
    EXPECT_NEAR(*solution->varianceFactor, 1.0 / 6.0, 1e-14);

    EXPECT_NEAR(solution->cofactor(0, 0), 5.0 / 6.0, 1e-14);
    EXPECT_NEAR(solution->cofactor(0, 1), -0.5, 1e-14);
    EXPECT_NEAR(solution->cofactor(1, 1), 0.5, 1e-14);
    EXPECT_EQ(solution->cofactor(0, 1), solution->cofactor(1, 0)); // exactly symmetric

    // A-priori scaling: the covariance is the cofactor matrix itself.
    EXPECT_DOUBLE_EQ(solution->covarianceScale, 1.0);
    EXPECT_EQ(solution->covariance, solution->cofactor);
    EXPECT_NEAR(solution->standardDeviations[0], std::sqrt(5.0 / 6.0), 1e-14);
    EXPECT_NEAR(solution->standardDeviations[1], std::sqrt(0.5), 1e-14);

    EXPECT_NEAR(solution->redundancyNumbers[0], 1.0 / 6.0, 1e-14);
    EXPECT_NEAR(solution->redundancyNumbers[1], 2.0 / 3.0, 1e-14);
    EXPECT_NEAR(solution->redundancyNumbers[2], 1.0 / 6.0, 1e-14);
    // With one degree of freedom every |w_i| equals sqrt(v^T P v) = sqrt(1/6).
    ASSERT_TRUE(solution->standardizedResiduals[0].has_value());
    EXPECT_NEAR(*solution->standardizedResiduals[0], std::sqrt(1.0 / 6.0), 1e-14);
    EXPECT_NEAR(*solution->standardizedResiduals[1], -std::sqrt(1.0 / 6.0), 1e-14);
    EXPECT_NEAR(*solution->standardizedResiduals[2], std::sqrt(1.0 / 6.0), 1e-14);
}

TEST(SurveyLeastSquares, APosterioriScalingMultipliesByTheVarianceFactor)
{
    const Matrix a = matrixOf({{1, 0}, {1, 1}, {1, 2}});
    const auto solution = solveLeastSquares(a, {0, 1, 1}, {1, 1, 1}); // default: a posteriori
    ASSERT_TRUE(solution.ok());
    EXPECT_NEAR(solution->covarianceScale, 1.0 / 6.0, 1e-14);
    // Sigma = s0^2 Qxx = (1/6) [[5/6, -1/2], [-1/2, 1/2]]
    EXPECT_NEAR(solution->covariance(0, 0), 5.0 / 36.0, 1e-14);
    EXPECT_NEAR(solution->covariance(0, 1), -1.0 / 12.0, 1e-14);
    EXPECT_NEAR(solution->covariance(1, 1), 1.0 / 12.0, 1e-14);
    EXPECT_NEAR(solution->standardDeviations[1], std::sqrt(1.0 / 12.0), 1e-14);
    EXPECT_NEAR(solution->cofactor(0, 0), 5.0 / 6.0, 1e-14); // the cofactor is unscaled
}

TEST(SurveyLeastSquares, WeightedMeanOfDirectObservations)
{
    // Two measurements of one quantity: 1.000 (sigma 2 mm) and 1.006 (sigma 4 mm).
    //   w = 250000, 62500;  x = (250000*1.000 + 62500*1.006) / 312500 = 1.0012
    //   v = (+0.0012, -0.0048);  v^T P v = 0.36 + 1.44 = 1.80;  Qxx = 1/312500 = 3.2e-6
    //   s_x (a posteriori) = sqrt(1.8 * 3.2e-6) = 0.0024
    //   r_1 = 1 - 250000/312500 = 0.2, r_2 = 0.8
    const Matrix a = matrixOf({{1}, {1}});
    const auto solution = solveLeastSquares(a, {1.000, 1.006}, {250000.0, 62500.0});
    ASSERT_TRUE(solution.ok());
    EXPECT_NEAR(solution->parameters[0], 1.0012, 1e-14);
    EXPECT_NEAR(solution->residuals[0], 0.0012, 1e-14);
    EXPECT_NEAR(solution->residuals[1], -0.0048, 1e-14);
    EXPECT_NEAR(solution->weightedSquaredResiduals, 1.80, 1e-10);
    EXPECT_NEAR(*solution->varianceFactor, 1.80, 1e-10);
    EXPECT_NEAR(solution->cofactor(0, 0), 3.2e-6, 1e-19);
    EXPECT_NEAR(solution->standardDeviations[0], 0.0024, 1e-12);
    EXPECT_NEAR(solution->redundancyNumbers[0], 0.2, 1e-14);
    EXPECT_NEAR(solution->redundancyNumbers[1], 0.8, 1e-14);
}

// ---- independent formulation -----------------------------------------------

TEST(SurveyLeastSquares, WeightedRedundantCaseAgainstNormalEquationsByCofactors)
{
    // Parabola y = c0 + c1 t + c2 t^2 through 6 noisy, unequally weighted points.
    const std::vector<double> t = {-2.0, -1.0, 0.0, 1.0, 2.0, 3.0};
    const std::vector<double> l = {7.3, 2.1, 0.8, 3.2, 8.9, 19.4};
    const std::vector<double> w = {4.0, 1.0, 25.0, 1.0, 0.25, 16.0};
    Matrix a(6, 3);
    for (std::size_t i = 0; i < 6; ++i) {
        a(i, 0) = 1.0;
        a(i, 1) = t[i];
        a(i, 2) = t[i] * t[i];
    }

    // Reference: N x = t by Cramer's rule, Qxx = N^-1 by the adjugate.
    Mat3 n;
    std::array<double, 3> rhs;
    normalEquations(a, l, w, n, rhs);
    const double d = det3(n);
    std::array<double, 3> expected{};
    for (int k = 0; k < 3; ++k) {
        Mat3 replaced = n;
        for (int r = 0; r < 3; ++r) {
            replaced[r][k] = rhs[r];
        }
        expected[static_cast<std::size_t>(k)] = det3(replaced) / d;
    }
    const Mat3 expectedCofactor = inverse3(n);
    double expectedVtpv = 0.0;
    std::vector<double> expectedResiduals;
    for (std::size_t i = 0; i < 6; ++i) {
        const double v = expected[0] + expected[1] * t[i] + expected[2] * t[i] * t[i] - l[i];
        expectedResiduals.push_back(v);
        expectedVtpv += w[i] * v * v;
    }

    const auto solution = solveLeastSquares(a, l, w);
    ASSERT_TRUE(solution.ok()) << solution.error().describe();
    EXPECT_EQ(solution->degreesOfFreedom, 3u);
    for (std::size_t k = 0; k < 3; ++k) {
        EXPECT_NEAR(solution->parameters[k], expected[k], 1e-12) << k;
        for (std::size_t c = 0; c < 3; ++c) {
            EXPECT_NEAR(solution->cofactor(k, c), expectedCofactor[k][c],
                        1e-12 * std::abs(expectedCofactor[k][c]) + 1e-15)
                << k << "," << c;
        }
    }
    for (std::size_t i = 0; i < 6; ++i) {
        EXPECT_NEAR(solution->residuals[i], expectedResiduals[i], 1e-12) << i;
    }
    EXPECT_NEAR(solution->weightedSquaredResiduals, expectedVtpv, 1e-11);
    EXPECT_NEAR(*solution->varianceFactor, expectedVtpv / 3.0, 1e-11);
    for (std::size_t k = 0; k < 3; ++k) {
        EXPECT_NEAR(solution->standardDeviations[k],
                    std::sqrt(expectedVtpv / 3.0 * expectedCofactor[k][k]), 1e-12);
    }
    // trace(redundancy) = degrees of freedom
    double redundancy = 0.0;
    for (const double r : solution->redundancyNumbers) {
        EXPECT_GE(r, 0.0);
        EXPECT_LE(r, 1.0);
        redundancy += r;
    }
    EXPECT_NEAR(redundancy, 3.0, 1e-12);
}

TEST(SurveyLeastSquares, WeightedLineAgainstClosedFormSums)
{
    // Weighted straight-line fit by the classical sums:
    //   D = Sw Swtt - Swt^2,  b = (Sw Swty - Swt Swy)/D,  a = (Swtt Swy - Swt Swty)/D
    //   Qxx = 1/D [[Swtt, -Swt], [-Swt, Sw]]
    const std::vector<double> t = {1000.0, 1001.0, 1002.5, 1004.0, 1007.0};
    const std::vector<double> y = {12.1, 12.9, 14.6, 16.2, 18.8};
    const std::vector<double> w = {1.0, 4.0, 2.0, 0.5, 3.0};
    double sw = 0, swt = 0, swy = 0, swtt = 0, swty = 0;
    Matrix a(5, 2);
    for (std::size_t i = 0; i < 5; ++i) {
        a(i, 0) = 1.0;
        a(i, 1) = t[i];
        sw += w[i];
        swt += w[i] * t[i];
        swy += w[i] * y[i];
        swtt += w[i] * t[i] * t[i];
        swty += w[i] * t[i] * y[i];
    }
    const double d = sw * swtt - swt * swt;
    const double slope = (sw * swty - swt * swy) / d;

    const auto solution = solveLeastSquares(a, y, w);
    ASSERT_TRUE(solution.ok());
    // Abscissae near 1000 make the two columns nearly parallel (the sums lose ~6
    // digits to cancellation; the QR does not), hence the looser bound.
    EXPECT_NEAR(solution->parameters[1], slope, 1e-8);
    EXPECT_NEAR(solution->cofactor(1, 1), sw / d, 1e-8 * sw / d);
    // The fitted line passes through the weighted centroid exactly.
    EXPECT_NEAR(solution->parameters[0] + solution->parameters[1] * (swt / sw), swy / sw, 1e-9);
}

// ---- properties ------------------------------------------------------------

TEST(SurveyLeastSquares, PropertyResidualsAreOrthogonalToTheColumnSpace)
{
    Random random;
    for (int i = 0; i < 200; ++i) {
        const RandomSystem system = randomSystem(random);
        const auto solution =
            solveLeastSquares(system.design, system.observations, system.weights);
        ASSERT_TRUE(solution.ok()) << solution.error().describe();
        // A^T P v = 0 is the defining condition of the minimum.
        EXPECT_LT(orthogonalityDefect(system.design, system.weights, solution->residuals), 1e-10);

        double redundancy = 0.0;
        for (const double r : solution->redundancyNumbers) {
            redundancy += r;
        }
        EXPECT_NEAR(redundancy, static_cast<double>(solution->degreesOfFreedom), 1e-10);
    }
}

TEST(SurveyLeastSquares, PropertyScalingAllWeightsLeavesTheEstimateUnchanged)
{
    Random random;
    for (int i = 0; i < 200; ++i) {
        const RandomSystem system = randomSystem(random);
        const double factor = random.real(0.01, 500.0);
        std::vector<double> scaled = system.weights;
        for (double& weight : scaled) {
            weight *= factor;
        }
        const auto base = solveLeastSquares(system.design, system.observations, system.weights);
        const auto other = solveLeastSquares(system.design, system.observations, scaled);
        ASSERT_TRUE(base.ok());
        ASSERT_TRUE(other.ok());
        for (std::size_t k = 0; k < base->parameters.size(); ++k) {
            const double scale = std::max(1.0, std::abs(base->parameters[k]));
            // x is unchanged ...
            EXPECT_NEAR(other->parameters[k], base->parameters[k], 1e-10 * scale);
            // ... Qxx shrinks by the factor, and the a-posteriori covariance, being
            // s0^2 Qxx, does not move: relative weights are all that matter.
            EXPECT_NEAR(other->cofactor(k, k) * factor, base->cofactor(k, k),
                        1e-9 * base->cofactor(k, k));
            EXPECT_NEAR(other->standardDeviations[k], base->standardDeviations[k],
                        1e-9 * base->standardDeviations[k]);
        }
        // v^T P v and the variance factor scale with the weights.
        EXPECT_NEAR(*other->varianceFactor, *base->varianceFactor * factor,
                    1e-9 * *base->varianceFactor * factor);
    }
}

TEST(SurveyLeastSquares, PropertyDeterministic)
{
    Random random;
    for (int i = 0; i < 50; ++i) {
        const RandomSystem system = randomSystem(random);
        const auto first = solveLeastSquares(system.design, system.observations, system.weights);
        const auto second = solveLeastSquares(system.design, system.observations, system.weights);
        ASSERT_TRUE(first.ok());
        ASSERT_TRUE(second.ok());
        // Bitwise: operator== on double, no tolerance.
        EXPECT_EQ(first->parameters, second->parameters);
        EXPECT_EQ(first->residuals, second->residuals);
        EXPECT_EQ(first->cofactor, second->cofactor);
        EXPECT_EQ(first->covariance, second->covariance);
        EXPECT_EQ(first->redundancyNumbers, second->redundancyNumbers);
        EXPECT_EQ(first->weightedSquaredResiduals, second->weightedSquaredResiduals);
    }
}

// ---- degrees of freedom ----------------------------------------------------

TEST(SurveyLeastSquares, UniquelyDeterminedSystemHasNoVarianceFactor)
{
    // 2 equations, 2 unknowns: x + y = 3, x - y = 1  =>  x = 2, y = 1.
    const Matrix a = matrixOf({{1, 1}, {1, -1}});
    const auto solution = solveLeastSquares(a, {3, 1}, {1, 4});
    ASSERT_TRUE(solution.ok());
    EXPECT_NEAR(solution->parameters[0], 2.0, 1e-14);
    EXPECT_NEAR(solution->parameters[1], 1.0, 1e-14);
    EXPECT_EQ(solution->degreesOfFreedom, 0u);
    // 0/0 is undefined, not zero and not NaN smuggled into a double.
    EXPECT_FALSE(solution->varianceFactor.has_value());
    EXPECT_DOUBLE_EQ(solution->covarianceScale, 1.0); // falls back to a priori, and says so
    EXPECT_EQ(solution->covariance, solution->cofactor);
    for (std::size_t i = 0; i < 2; ++i) {
        EXPECT_NEAR(solution->residuals[i], 0.0, 1e-14);
        EXPECT_NEAR(solution->redundancyNumbers[i], 0.0, 1e-14);
        EXPECT_FALSE(solution->standardizedResiduals[i].has_value()); // nothing checks them
    }
    // Qxx = (A^T P A)^-1 with A^T P A = [[5, -3], [-3, 5]], det 16.
    EXPECT_NEAR(solution->cofactor(0, 0), 5.0 / 16.0, 1e-14);
    EXPECT_NEAR(solution->cofactor(0, 1), 3.0 / 16.0, 1e-14);
}

TEST(SurveyLeastSquares, UncheckedObservationHasNoStandardizedResidual)
{
    // x1 is measured twice, x2 once: the third observation has zero redundancy.
    const Matrix a = matrixOf({{1, 0}, {1, 0}, {0, 1}});
    const auto solution = solveLeastSquares(a, {10.0, 10.2, 5.0}, {1, 1, 1});
    ASSERT_TRUE(solution.ok());
    EXPECT_NEAR(solution->parameters[0], 10.1, 1e-14);
    EXPECT_NEAR(solution->parameters[1], 5.0, 1e-14);
    EXPECT_NEAR(solution->redundancyNumbers[0], 0.5, 1e-14);
    EXPECT_NEAR(solution->redundancyNumbers[2], 0.0, 1e-14);
    EXPECT_TRUE(solution->standardizedResiduals[0].has_value());
    EXPECT_FALSE(solution->standardizedResiduals[2].has_value());
}

// ---- failures --------------------------------------------------------------

TEST(SurveyLeastSquares, RankDeficiencyIsReportedWithTheUnresolvedParameters)
{
    // Columns 0 and 2 are identical: only their sum is determined.
    const Matrix duplicate = matrixOf({{1, 2, 1}, {1, 3, 1}, {1, 5, 1}, {1, 7, 1}});
    const auto result = solveLeastSquares(duplicate, {1, 2, 3, 4}, {1, 1, 1, 1});
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::AdjustmentFailure);
    EXPECT_NE(result.error().message.find("rank 2 of 3"), std::string::npos)
        << result.error().message;

    // A parameter no observation touches.
    const Matrix zeroColumn = matrixOf({{1, 0}, {2, 0}, {3, 0}});
    const auto untouched = solveLeastSquares(zeroColumn, {1, 2, 3}, {1, 1, 1});
    ASSERT_FALSE(untouched.ok());
    EXPECT_EQ(untouched.error().code, ErrorCode::AdjustmentFailure);
    EXPECT_NE(untouched.error().message.find("indices: 1"), std::string::npos)
        << untouched.error().message;

    // Linear dependence that is not visible column by column: c2 = c0 + c1.
    const Matrix combination = matrixOf({{1, 2, 3}, {4, 5, 9}, {7, 8, 15}, {2, 2, 4}});
    EXPECT_EQ(solveLeastSquares(combination, {1, 2, 3, 4}, {1, 1, 1, 1}).error().code,
              ErrorCode::AdjustmentFailure);
}

TEST(SurveyLeastSquares, RankDecisionIsIndependentOfColumnUnits)
{
    // The same well-posed line fit with the slope column in wildly different
    // units (t in seconds vs in years) must solve, not be mistaken for singular.
    const Matrix a = matrixOf({{1, 1e-9}, {1, 2e-9}, {1, 3e-9}});
    const auto solution = solveLeastSquares(a, {1, 3, 5}, {1, 1, 1});
    ASSERT_TRUE(solution.ok()) << solution.error().describe();
    EXPECT_NEAR(solution->parameters[0], -1.0, 1e-9);
    EXPECT_NEAR(solution->parameters[1], 2e9, 1.0);
}

TEST(SurveyLeastSquares, FewerObservationsThanParameters)
{
    const Matrix a = matrixOf({{1, 2, 3}, {4, 5, 6}});
    const auto result = solveLeastSquares(a, {1, 2}, {1, 1});
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::AdjustmentFailure);
}

TEST(SurveyLeastSquares, RejectsInvalidInput)
{
    const Matrix a = matrixOf({{1, 0}, {1, 1}, {1, 2}});
    const auto code = [&](const Matrix& design, const std::vector<double>& l,
                          const std::vector<double>& w) {
        return solveLeastSquares(design, l, w).error().code;
    };
    // Zero, negative and non-finite weights (sigma = infinity, imaginary, unknown).
    EXPECT_EQ(code(a, {0, 1, 1}, {1, 0, 1}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(a, {0, 1, 1}, {1, -1, 1}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(a, {0, 1, 1}, {1, kNaN, 1}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(a, {0, 1, 1}, {1, std::numeric_limits<double>::infinity(), 1}),
              ErrorCode::InvalidArgument);
    // Dimension mismatches.
    EXPECT_EQ(code(a, {0, 1}, {1, 1, 1}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(a, {0, 1, 1}, {1, 1}), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(Matrix{}, {}, {}), ErrorCode::InvalidArgument);
    // Non-finite data.
    EXPECT_EQ(code(a, {0, kNaN, 1}, {1, 1, 1}), ErrorCode::InvalidArgument);
    Matrix poisoned = a;
    poisoned(1, 1) = kNaN;
    EXPECT_EQ(code(poisoned, {0, 1, 1}, {1, 1, 1}), ErrorCode::InvalidArgument);
    // Storage edited behind the dimensions' back.
    Matrix inconsistent = a;
    inconsistent.values.pop_back();
    EXPECT_EQ(code(inconsistent, {0, 1, 1}, {1, 1, 1}), ErrorCode::InvalidArgument);

    EXPECT_FALSE(Matrix::fromRows({{1, 2}, {3}}).ok());
}

// ---- full covariance -------------------------------------------------------

TEST(SurveyLeastSquares, CorrelatedObservationsSolvedByHand)
{
    // Two correlated measurements of one quantity, Sigma = [[4, 1], [1, 2]]:
    //   Sigma^-1 = 1/7 [[2, -1], [-1, 4]]
    //   x = (1^T S^-1 l) / (1^T S^-1 1) = ((2-1) 10 + (4-1) 12) / (2 - 1 - 1 + 4) = 46/4 = 11.5
    //   Qxx = 7/4;  v = (1.5, -0.5);  v^T S^-1 v = (2*2.25 + 2*0.75 + 4*0.25)/7 = 1
    const Matrix a = matrixOf({{1}, {1}});
    const Matrix sigma = matrixOf({{4, 1}, {1, 2}});
    const auto solution = solveLeastSquaresWithCovariance(a, {10, 12}, sigma);
    ASSERT_TRUE(solution.ok()) << solution.error().describe();
    EXPECT_NEAR(solution->parameters[0], 11.5, 1e-13);
    EXPECT_NEAR(solution->cofactor(0, 0), 1.75, 1e-13);
    EXPECT_NEAR(solution->residuals[0], 1.5, 1e-13);
    EXPECT_NEAR(solution->residuals[1], -0.5, 1e-13);
    EXPECT_NEAR(solution->weightedSquaredResiduals, 1.0, 1e-13);
    EXPECT_NEAR(*solution->varianceFactor, 1.0, 1e-13);
    // Qvv = Sigma - A Qxx A^T = [[2.25, -0.75], [-0.75, 0.25]]; w_i = v_i / sqrt(Qvv_ii).
    EXPECT_NEAR(*solution->standardizedResiduals[0], 1.5 / 1.5, 1e-12);
    EXPECT_NEAR(*solution->standardizedResiduals[1], -0.5 / 0.5, 1e-12);
    // r = diag(Qvv Sigma^-1) = (2.25*2 + 0.75, 0.75 + 0.25*4)/7 = (0.75, 0.25); sums to 1.
    EXPECT_NEAR(solution->redundancyNumbers[0], 0.75, 1e-12);
    EXPECT_NEAR(solution->redundancyNumbers[1], 0.25, 1e-12);
}

TEST(SurveyLeastSquares, DiagonalCovarianceAgreesWithWeights)
{
    Random random;
    for (int i = 0; i < 50; ++i) {
        const RandomSystem system = randomSystem(random);
        const std::size_t n = system.observations.size();
        Matrix sigma(n, n);
        for (std::size_t k = 0; k < n; ++k) {
            sigma(k, k) = 1.0 / system.weights[k];
        }
        const auto byWeights =
            solveLeastSquares(system.design, system.observations, system.weights);
        const auto byCovariance =
            solveLeastSquaresWithCovariance(system.design, system.observations, sigma);
        ASSERT_TRUE(byWeights.ok());
        ASSERT_TRUE(byCovariance.ok());
        for (std::size_t k = 0; k < byWeights->parameters.size(); ++k) {
            EXPECT_NEAR(byCovariance->parameters[k], byWeights->parameters[k],
                        1e-10 * std::max(1.0, std::abs(byWeights->parameters[k])));
        }
        EXPECT_NEAR(byCovariance->weightedSquaredResiduals, byWeights->weightedSquaredResiduals,
                    1e-9 * std::max(1.0, byWeights->weightedSquaredResiduals));
        for (std::size_t k = 0; k < n; ++k) {
            EXPECT_NEAR(byCovariance->redundancyNumbers[k], byWeights->redundancyNumbers[k], 1e-10);
        }
    }
}

TEST(SurveyLeastSquares, RejectsInvalidCovariance)
{
    const Matrix a = matrixOf({{1}, {1}});
    const auto code = [&](const Matrix& sigma) {
        return solveLeastSquaresWithCovariance(a, {10, 12}, sigma).error().code;
    };
    EXPECT_EQ(code(matrixOf({{4, 1}, {2, 2}})), ErrorCode::InvalidArgument);  // asymmetric
    EXPECT_EQ(code(matrixOf({{1, 2}, {2, 1}})), ErrorCode::InvalidArgument);  // indefinite
    EXPECT_EQ(code(matrixOf({{1, 0}, {0, 0}})), ErrorCode::InvalidArgument);  // singular
    EXPECT_EQ(code(matrixOf({{1, 0}, {0, -1}})), ErrorCode::InvalidArgument); // negative variance
    EXPECT_EQ(code(matrixOf({{1, 0, 0}, {0, 1, 0}, {0, 0, 1}})), ErrorCode::InvalidArgument);
    EXPECT_EQ(code(matrixOf({{1, kNaN}, {kNaN, 1}})), ErrorCode::InvalidArgument);
}
