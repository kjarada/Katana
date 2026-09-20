#pragma once

// Chi-square distribution for the global test of an adjustment (PLAN.MD
// Phase 13). Implemented from first principles, without external libraries:
//
//   cdf(x; k) = P(k/2, x/2), the regularised lower incomplete gamma function,
//   evaluated by its power series for x/2 < k/2 + 1 and by the continued
//   fraction of Q = 1 - P otherwise (both summed to machine precision). The
//   prefactor x^a e^-x / Gamma(a) is formed in logarithms, for a >= 16 in the
//   cancellation-free form a*(log1p(mu) - mu) with mu = (x - a)/a.
//
//   quantile(p; k) inverts the cdf by bisection to the resolution of a double.
//
// Accuracy: relative error of the cdf below ~1e-13 for k up to ~1e4 (verified
// in the tests against the closed forms for k = 1, 2, 4 and tabulated critical
// values); the quantile inherits it. Pure functions, safe to call concurrently.

#include <cstddef>

#include "katana/core/error.hpp"

namespace katana::survey {

// P(X <= x) for X ~ chi-square with `degreesOfFreedom` >= 1. x < 0 gives 0.
// InvalidArgument for a non-finite x or zero degrees of freedom; Internal when
// the series fails to converge (not expected below ~1e9 degrees of freedom).
[[nodiscard]] katana::core::Result<double> chiSquareCdf(double x, std::size_t degreesOfFreedom);

// x with P(X <= x) = probability, for probability in the open interval (0, 1).
[[nodiscard]] katana::core::Result<double> chiSquareQuantile(double probability,
                                                             std::size_t degreesOfFreedom);

} // namespace katana::survey
