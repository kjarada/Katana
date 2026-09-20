#pragma once

// First-order propagation of variances (PLAN.MD Phase 13): Sigma_y = J Sigma_x J^T
// for y = f(x) linearised at the given values, plus the error ellipse of a 2D
// position. First order means the standard deviations must be small against the
// distances involved (sigma_azimuth << 1 rad, sigma << d), which holds for any
// realistic survey.

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"
#include "katana/survey/matrix.hpp"

namespace katana::survey {

// Symmetric 2 x 2 covariance of a horizontal position, m^2.
struct Covariance2 {
    double northing = 0.0;        // variance of the northing
    double easting = 0.0;         // variance of the easting
    double northingEasting = 0.0; // covariance

    friend constexpr bool operator==(const Covariance2&, const Covariance2&) = default;
};

// Standard (1 sigma, 39.3 % confidence) error ellipse.
struct ErrorEllipse {
    double semiMajor = 0.0; // metres
    double semiMinor = 0.0; // metres
    // Azimuth of the semi-major axis, clockwise from north, in [0, pi). 0 for a
    // circular ellipse, whose orientation is arbitrary.
    double orientation = 0.0;
};

// Eigen-decomposition of the covariance in closed form:
//   lambda = (sN^2 + sE^2)/2 +- sqrt(((sN^2 - sE^2)/2)^2 + sNE^2)
//   orientation = atan2(2 sNE, sN^2 - sE^2) / 2
// InvalidArgument when the matrix is not a covariance (negative variance,
// |sNE| > sN sE, non-finite).
[[nodiscard]] katana::core::Result<ErrorEllipse> errorEllipse(const Covariance2& covariance);

// Factor that scales the standard ellipse to confidence level `probability` in
// (0, 1) when the variance factor is known: sqrt(chi2(2, p)) = sqrt(-2 ln(1 - p));
// 2.4477 for 95 %. (With an estimated variance factor the rigorous factor is
// sqrt(2 F(2, r, p)), which tends to this value as r grows.)
[[nodiscard]] katana::core::Result<double> ellipseConfidenceScale(double probability);

// Covariance of the point computed by forward(start, azimuth, distance) when the
// start has covariance `start` and azimuth and distance are uncorrelated with
// standard deviations `sigmaAzimuth` (radians) and `sigmaDistance` (metres):
//   J = [ -d sin(az)  cos(az) ]   (rows N, E; columns azimuth, distance)
//       [  d cos(az)  sin(az) ]
//   Sigma = Sigma_start + J diag(sigmaAzimuth^2, sigmaDistance^2) J^T
[[nodiscard]] katana::core::Result<Covariance2> propagateForward(const Covariance2& start,
                                                                 double azimuth, double distance,
                                                                 double sigmaAzimuth,
                                                                 double sigmaDistance);

struct InverseUncertainty {
    double sigmaAzimuth = 0.0;  // radians
    double sigmaDistance = 0.0; // metres
    double covariance = 0.0;    // radians * metres
};

// Standard deviations of the azimuth and distance computed by inverse(from, to)
// from two mutually uncorrelated positions. InvalidArgument when the points
// coincide.
[[nodiscard]] katana::core::Result<InverseUncertainty>
propagateInverse(const Coordinate2& from, const Coordinate2& to, const Covariance2& fromCovariance,
                 const Covariance2& toCovariance);

// General law: returns J Sigma J^T (m x m, exactly symmetric) for a Jacobian
// J (m x k) and a symmetric covariance Sigma (k x k). InvalidArgument on
// dimension mismatch, non-finite values or an asymmetric Sigma.
[[nodiscard]] katana::core::Result<Matrix> propagateCovariance(const Matrix& jacobian,
                                                               const Matrix& covariance);

} // namespace katana::survey
