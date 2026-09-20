#include <gtest/gtest.h>

#include <cmath>
#include <limits>

#include "katana/math/numerics.hpp"
#include "katana/survey/statistics.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::math::kPi;

namespace {

// Independent reference for the chi-square cdf, built from closed forms rather
// than from the incomplete gamma expansions of the implementation:
//   k = 1:  F(x) = erf(sqrt(x/2))
//   k = 2:  F(x) = 1 - exp(-x/2)
//   and the recurrence  F_{k+2}(x) = F_k(x) - 2 f_{k+2}(x),
//   f_k(x) = x^{k/2-1} e^{-x/2} / (2^{k/2} Gamma(k/2))  (the density),
// with the density evaluated through std::lgamma.
double referenceCdf(double x, std::size_t dof)
{
    double cdf = dof % 2 == 1 ? std::erf(std::sqrt(0.5 * x)) : 1.0 - std::exp(-0.5 * x);
    for (std::size_t k = dof % 2 == 1 ? 1 : 2; k < dof; k += 2) {
        const double half = 0.5 * static_cast<double>(k + 2);
        const double logDensity =
            (half - 1.0) * std::log(x) - 0.5 * x - half * std::log(2.0) - std::lgamma(half);
        cdf -= 2.0 * std::exp(logDensity);
    }
    return cdf;
}

} // namespace

TEST(SurveyStatistics, CdfMatchesClosedFormsForSmallDegreesOfFreedom)
{
    for (const double x : {0.001, 0.1, 0.5, 1.0, 2.0, 3.841, 5.0, 9.0, 20.0, 50.0}) {
        EXPECT_NEAR(*chiSquareCdf(x, 1), std::erf(std::sqrt(0.5 * x)), 1e-14) << x;
        EXPECT_NEAR(*chiSquareCdf(x, 2), 1.0 - std::exp(-0.5 * x), 1e-14) << x;
        // k = 3: erf(sqrt(x/2)) - sqrt(2x/pi) e^{-x/2}
        EXPECT_NEAR(*chiSquareCdf(x, 3),
                    std::erf(std::sqrt(0.5 * x)) - std::sqrt(2.0 * x / kPi) * std::exp(-0.5 * x),
                    1e-14)
            << x;
        // k = 4: 1 - e^{-x/2} (1 + x/2)
        EXPECT_NEAR(*chiSquareCdf(x, 4), 1.0 - std::exp(-0.5 * x) * (1.0 + 0.5 * x), 1e-14) << x;
    }
}

TEST(SurveyStatistics, CdfMatchesRecurrenceReferenceAcrossBothExpansions)
{
    // Arguments on both sides of x/2 = k/2 + 1 (series vs continued fraction) and
    // degrees of freedom on both sides of 32 (direct vs Stirling prefactor).
    for (const std::size_t dof : {5u, 10u, 25u, 31u, 32u, 33u, 60u, 100u, 250u}) {
        const double mean = static_cast<double>(dof);
        const double spread = std::sqrt(2.0 * mean);
        for (const double z : {-2.5, -1.0, -0.2, 0.0, 0.3, 1.0, 2.5}) {
            const double x = mean + z * spread;
            if (x <= 0.0) {
                continue;
            }
            const auto cdf = chiSquareCdf(x, dof);
            ASSERT_TRUE(cdf.ok());
            // The reference subtracts up to k/2 densities, each good to ~1e-15.
            EXPECT_NEAR(*cdf, referenceCdf(x, dof), 1e-12) << "dof " << dof << " x " << x;
        }
    }
}

TEST(SurveyStatistics, CdfLimitsAndMonotonicity)
{
    EXPECT_DOUBLE_EQ(*chiSquareCdf(0.0, 3), 0.0);
    EXPECT_DOUBLE_EQ(*chiSquareCdf(-5.0, 3), 0.0);
    EXPECT_NEAR(*chiSquareCdf(1000.0, 3), 1.0, 1e-15);
    // The median of chi-square(2) is 2 ln 2.
    EXPECT_NEAR(*chiSquareCdf(2.0 * std::log(2.0), 2), 0.5, 1e-15);

    for (const std::size_t dof : {1u, 7u, 40u, 500u}) {
        double previous = 0.0;
        for (int i = 1; i <= 60; ++i) {
            const double x = static_cast<double>(dof) * 0.05 * i;
            const double cdf = *chiSquareCdf(x, dof);
            EXPECT_GE(cdf, previous) << "dof " << dof << " x " << x;
            EXPECT_LE(cdf, 1.0);
            previous = cdf;
        }
    }
}

TEST(SurveyStatistics, QuantileClosedFormForTwoDegreesOfFreedom)
{
    // F(x) = 1 - exp(-x/2)  =>  x = -2 ln(1 - p)
    for (const double p : {0.001, 0.025, 0.05, 0.5, 0.95, 0.975, 0.999}) {
        EXPECT_NEAR(*chiSquareQuantile(p, 2), -2.0 * std::log1p(-p), 1e-12) << p;
    }
}

TEST(SurveyStatistics, QuantileMatchesPublishedCriticalValues)
{
    // Standard chi-square table (any statistics text), given there to 3 decimals.
    struct Entry {
        double probability;
        std::size_t dof;
        double value;
    };
    const Entry table[] = {
        {0.95, 1, 3.841},   {0.975, 1, 5.024},  {0.95, 2, 5.991},   {0.95, 5, 11.070},
        {0.025, 10, 3.247}, {0.95, 10, 18.307}, {0.975, 10, 20.483}, {0.05, 30, 18.493},
        {0.95, 30, 43.773},
    };
    for (const Entry& entry : table) {
        const auto quantile = chiSquareQuantile(entry.probability, entry.dof);
        ASSERT_TRUE(quantile.ok());
        EXPECT_NEAR(*quantile, entry.value, 5e-4)
            << "p " << entry.probability << " dof " << entry.dof;
    }
}

TEST(SurveyStatistics, QuantileInvertsTheCdf)
{
    for (const std::size_t dof : {1u, 2u, 3u, 9u, 31u, 32u, 120u, 2000u}) {
        for (const double p : {0.005, 0.025, 0.3, 0.5, 0.9, 0.975, 0.995}) {
            const auto x = chiSquareQuantile(p, dof);
            ASSERT_TRUE(x.ok());
            EXPECT_GT(*x, 0.0);
            EXPECT_NEAR(*chiSquareCdf(*x, dof), p, 1e-12) << "dof " << dof << " p " << p;
        }
        EXPECT_LT(*chiSquareQuantile(0.025, dof), *chiSquareQuantile(0.975, dof));
    }
}

TEST(SurveyStatistics, LargeDegreesOfFreedomApproachTheNormalLimit)
{
    // chi-square(k) has mean k and variance 2k; by the Wilson-Hilferty cube-root
    // transform its 97.5 % point is k (1 - 2/(9k) + 1.959964 sqrt(2/(9k)))^3 to a
    // few parts in 1e4 at k = 5000. A coarse check that large k neither overflows
    // nor loses the tail, not a precision test.
    const double k = 5000.0;
    const double term = 2.0 / (9.0 * k);
    const double wilsonHilferty = k * std::pow(1.0 - term + 1.959964 * std::sqrt(term), 3.0);
    const auto quantile = chiSquareQuantile(0.975, 5000);
    ASSERT_TRUE(quantile.ok());
    EXPECT_NEAR(*quantile, wilsonHilferty, 0.05);
    EXPECT_NEAR(*chiSquareCdf(*quantile, 5000), 0.975, 1e-11);
}

TEST(SurveyStatistics, RejectsBadArguments)
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    EXPECT_EQ(chiSquareCdf(1.0, 0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(chiSquareCdf(nan, 3).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(chiSquareCdf(std::numeric_limits<double>::infinity(), 3).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(chiSquareQuantile(0.5, 0).error().code, ErrorCode::InvalidArgument);
    for (const double p : {0.0, 1.0, -0.1, 1.1, nan}) {
        EXPECT_EQ(chiSquareQuantile(p, 4).error().code, ErrorCode::InvalidArgument) << p;
    }
}

TEST(SurveyStatistics, Deterministic)
{
    EXPECT_EQ(*chiSquareQuantile(0.975, 17), *chiSquareQuantile(0.975, 17));
    EXPECT_EQ(*chiSquareCdf(12.5, 17), *chiSquareCdf(12.5, 17));
}
