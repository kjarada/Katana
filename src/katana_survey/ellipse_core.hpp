#pragma once

// Closed-form error ellipse shared by the public errorEllipse() (which validates
// its input) and the network adjustment (whose covariance blocks are positive
// semi-definite by construction and must not be rejected over rounding).

#include <algorithm>
#include <cmath>

#include "katana/math/numerics.hpp"
#include "katana/survey/error_propagation.hpp"

namespace katana::survey::detail {

// Precondition: finite entries, non-negative variances.
[[nodiscard]] inline ErrorEllipse ellipseOf(const Covariance2& covariance)
{
    const double mean = 0.5 * (covariance.northing + covariance.easting);
    const double halfDifference = 0.5 * (covariance.northing - covariance.easting);
    const double root = std::hypot(halfDifference, covariance.northingEasting);

    ErrorEllipse ellipse;
    ellipse.semiMajor = std::sqrt(mean + root);
    // mean - root is >= 0 analytically; rounding may push it a hair below.
    ellipse.semiMinor = std::sqrt(std::max(mean - root, 0.0));
    // The major axis direction t (from north towards east) maximises
    //   sN^2 cos^2 t + 2 sNE sin t cos t + sE^2 sin^2 t,
    // which gives tan 2t = 2 sNE / (sN^2 - sE^2).
    double orientation = 0.5 * std::atan2(2.0 * covariance.northingEasting,
                                          covariance.northing - covariance.easting);
    if (orientation < 0.0) {
        orientation += katana::math::kPi;
    }
    ellipse.orientation = orientation >= katana::math::kPi ? 0.0 : orientation;
    return ellipse;
}

} // namespace katana::survey::detail
