#include "katana/survey/error_propagation.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "ellipse_core.hpp"
#include "katana/math/numerics.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
namespace tol = katana::math::tolerance;

namespace {

Status checkCovariance(const Covariance2& c, const char* name)
{
    if (!std::isfinite(c.northing) || !std::isfinite(c.easting) ||
        !std::isfinite(c.northingEasting)) {
        return makeError(ErrorCode::InvalidArgument, std::string(name) + " is not finite");
    }
    if (c.northing < 0.0 || c.easting < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(name) + " has a negative variance");
    }
    // Cauchy-Schwarz with the relative tolerance as head-room for a covariance
    // that is singular up to rounding (perfectly correlated components).
    const double bound = c.northing * c.easting;
    if (c.northingEasting * c.northingEasting > bound * (1.0 + tol::kRelative)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(name) + " is not positive semi-definite");
    }
    return {};
}

} // namespace

Result<ErrorEllipse> errorEllipse(const Covariance2& covariance)
{
    if (Status status = checkCovariance(covariance, "covariance"); !status) {
        return status.error();
    }
    return detail::ellipseOf(covariance);
}

Result<double> ellipseConfidenceScale(double probability)
{
    if (!(probability > 0.0 && probability < 1.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "probability must lie strictly between 0 and 1",
                         "probability " + std::to_string(probability));
    }
    // The chi-square distribution with 2 degrees of freedom has the closed-form
    // cdf 1 - exp(-x / 2).
    return std::sqrt(-2.0 * std::log1p(-probability));
}

Result<Covariance2> propagateForward(const Covariance2& start, double azimuth, double distance,
                                     double sigmaAzimuth, double sigmaDistance)
{
    if (Status status = checkCovariance(start, "start covariance"); !status) {
        return status.error();
    }
    if (!std::isfinite(azimuth) || !std::isfinite(distance) || distance < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "azimuth must be finite and distance finite and not negative");
    }
    if (!std::isfinite(sigmaAzimuth) || !std::isfinite(sigmaDistance) || sigmaAzimuth < 0.0 ||
        sigmaDistance < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "standard deviations must be finite and not negative");
    }
    const double sine = std::sin(azimuth);
    const double cosine = std::cos(azimuth);
    const double crossTrack = distance * distance * sigmaAzimuth * sigmaAzimuth; // (d sigma_az)^2
    const double alongTrack = sigmaDistance * sigmaDistance;

    Covariance2 result;
    result.northing = start.northing + sine * sine * crossTrack + cosine * cosine * alongTrack;
    result.easting = start.easting + cosine * cosine * crossTrack + sine * sine * alongTrack;
    result.northingEasting = start.northingEasting + sine * cosine * (alongTrack - crossTrack);
    return result;
}

Result<InverseUncertainty> propagateInverse(const Coordinate2& from, const Coordinate2& to,
                                            const Covariance2& fromCovariance,
                                            const Covariance2& toCovariance)
{
    if (Status status = checkCovariance(fromCovariance, "from covariance"); !status) {
        return status.error();
    }
    if (Status status = checkCovariance(toCovariance, "to covariance"); !status) {
        return status.error();
    }
    const double dn = to.northing - from.northing;
    const double de = to.easting - from.easting;
    const double d = std::hypot(dn, de);
    if (!std::isfinite(d)) {
        return makeError(ErrorCode::InvalidArgument, "coordinates are not finite");
    }
    if (d <= tol::kCoordinate) {
        return makeError(ErrorCode::InvalidArgument,
                         "the points coincide, the azimuth is undefined");
    }
    // The Jacobians with respect to the two ends differ only in sign, so for
    // uncorrelated ends the covariances simply add.
    const double nn = fromCovariance.northing + toCovariance.northing;
    const double ee = fromCovariance.easting + toCovariance.easting;
    const double ne = fromCovariance.northingEasting + toCovariance.northingEasting;

    // Rows of J: azimuth (-dE, dN) / d^2, distance (dN, dE) / d.
    const double azN = -de / (d * d);
    const double azE = dn / (d * d);
    const double dN = dn / d;
    const double dE = de / d;

    InverseUncertainty result;
    result.sigmaAzimuth =
        std::sqrt(std::max(azN * azN * nn + 2.0 * azN * azE * ne + azE * azE * ee, 0.0));
    result.sigmaDistance =
        std::sqrt(std::max(dN * dN * nn + 2.0 * dN * dE * ne + dE * dE * ee, 0.0));
    result.covariance = azN * dN * nn + (azN * dE + azE * dN) * ne + azE * dE * ee;
    return result;
}

Result<Matrix> propagateCovariance(const Matrix& jacobian, const Matrix& covariance)
{
    if (!jacobian.isConsistent() || !covariance.isConsistent()) {
        return makeError(ErrorCode::InvalidArgument,
                         "matrix storage does not match its dimensions");
    }
    if (covariance.rows != covariance.cols || jacobian.cols != covariance.rows) {
        return makeError(ErrorCode::InvalidArgument,
                         "dimension mismatch: J is m x k, Sigma must be k x k",
                         "J " + std::to_string(jacobian.rows) + " x " +
                             std::to_string(jacobian.cols) + ", Sigma " +
                             std::to_string(covariance.rows) + " x " +
                             std::to_string(covariance.cols));
    }
    const auto finite = [](double value) { return std::isfinite(value); };
    if (!std::all_of(jacobian.values.begin(), jacobian.values.end(), finite) ||
        !std::all_of(covariance.values.begin(), covariance.values.end(), finite)) {
        return makeError(ErrorCode::InvalidArgument, "matrices contain non-finite values");
    }
    const std::size_t k = covariance.rows;
    for (std::size_t i = 0; i < k; ++i) {
        for (std::size_t j = i + 1; j < k; ++j) {
            const double scale = std::sqrt(std::abs(covariance(i, i) * covariance(j, j)));
            if (std::abs(covariance(i, j) - covariance(j, i)) > tol::kRelative * scale) {
                return makeError(ErrorCode::InvalidArgument, "covariance is not symmetric",
                                 "entry (" + std::to_string(i) + ", " + std::to_string(j) + ")");
            }
        }
    }

    const std::size_t m = jacobian.rows;
    Matrix product(m, k); // J Sigma
    for (std::size_t r = 0; r < m; ++r) {
        for (std::size_t c = 0; c < k; ++c) {
            double sum = 0.0;
            for (std::size_t i = 0; i < k; ++i) {
                sum += jacobian(r, i) * covariance(i, c);
            }
            product(r, c) = sum;
        }
    }
    Matrix result(m, m);
    for (std::size_t r = 0; r < m; ++r) {
        for (std::size_t c = r; c < m; ++c) {
            double sum = 0.0;
            for (std::size_t i = 0; i < k; ++i) {
                sum += product(r, i) * jacobian(c, i);
            }
            result(r, c) = sum;
            result(c, r) = sum;
        }
    }
    return result;
}

} // namespace katana::survey
