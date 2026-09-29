#include "katana/survey/cogo.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::math::kPi;
using katana::math::normalizeAngle;
namespace tol = katana::math::tolerance;

namespace {

bool isFinite(const Coordinate2& c)
{
    return std::isfinite(c.northing) && std::isfinite(c.easting);
}

Status checkPolygon(std::span<const Coordinate2> vertices)
{
    if (vertices.size() < 3) {
        return makeError(ErrorCode::InvalidArgument, "a polygon needs at least 3 vertices",
                         "vertex count " + std::to_string(vertices.size()));
    }
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        if (!isFinite(vertices[i])) {
            return makeError(ErrorCode::InvalidArgument, "polygon vertex is not finite",
                             "vertex index " + std::to_string(i));
        }
    }
    return {};
}

// Sums of the shoelace integrals about the first vertex, in local east (x) /
// north (y) axes.
struct PolygonSums {
    double twiceArea = 0.0;
    double momentEast = 0.0;  // 6 * A * centroid offset east
    double momentNorth = 0.0; // 6 * A * centroid offset north
    double extentSquared = 0.0;
};

PolygonSums polygonSums(std::span<const Coordinate2> vertices)
{
    const Coordinate2 origin = vertices.front();
    PolygonSums sums;
    double x0 = 0.0;
    double y0 = 0.0;
    for (std::size_t i = 1; i <= vertices.size(); ++i) {
        const Coordinate2& next = vertices[i % vertices.size()];
        const double x1 = next.easting - origin.easting;
        const double y1 = next.northing - origin.northing;
        const double cross = x0 * y1 - x1 * y0;
        sums.twiceArea += cross;
        sums.momentEast += (x0 + x1) * cross;
        sums.momentNorth += (y0 + y1) * cross;
        sums.extentSquared = std::max(sums.extentSquared, x1 * x1 + y1 * y1);
        x0 = x1;
        y0 = y1;
    }
    return sums;
}

Status checkSlope(double slopeDistance, double zenithAngle)
{
    if (!std::isfinite(slopeDistance) || slopeDistance < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "slope distance must be finite and not negative",
                         "slope distance " + katana::core::formatExactReal(slopeDistance));
    }
    if (!std::isfinite(zenithAngle) || zenithAngle < 0.0 || zenithAngle > kPi) {
        return makeError(ErrorCode::InvalidArgument, "zenith angle must lie in [0, pi]",
                         "zenith angle " + katana::core::formatExactReal(zenithAngle));
    }
    return {};
}

} // namespace

Result<InverseResult> inverse(const Coordinate2& from, const Coordinate2& to)
{
    if (!isFinite(from) || !isFinite(to)) {
        return makeError(ErrorCode::InvalidArgument, "inverse: coordinates are not finite");
    }
    const double deltaNorthing = to.northing - from.northing;
    const double deltaEasting = to.easting - from.easting;
    const double distance = std::hypot(deltaNorthing, deltaEasting);
    if (distance <= tol::kCoordinate) {
        return makeError(ErrorCode::InvalidArgument,
                         "inverse: the points coincide, the azimuth is undefined",
                         "separation " + katana::core::formatExactReal(distance) + " m");
    }
    return InverseResult{normalizeAngle(std::atan2(deltaEasting, deltaNorthing)), distance};
}

Result<Coordinate2> forward(const Coordinate2& from, double azimuth, double distance)
{
    if (!isFinite(from) || !std::isfinite(azimuth)) {
        return makeError(ErrorCode::InvalidArgument, "forward: start or azimuth is not finite");
    }
    if (!std::isfinite(distance) || distance < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "forward: distance must be finite and not negative",
                         "distance " + katana::core::formatExactReal(distance));
    }
    return Coordinate2{from.northing + distance * std::cos(azimuth),
                       from.easting + distance * std::sin(azimuth)};
}

Result<double> polygonSignedArea(std::span<const Coordinate2> vertices)
{
    if (Status status = checkPolygon(vertices); !status) {
        return status.error();
    }
    return 0.5 * polygonSums(vertices).twiceArea;
}

Result<double> polygonArea(std::span<const Coordinate2> vertices)
{
    const auto signedArea = polygonSignedArea(vertices);
    if (!signedArea) {
        return signedArea.error();
    }
    return std::abs(*signedArea);
}

Result<double> polygonPerimeter(std::span<const Coordinate2> vertices)
{
    if (Status status = checkPolygon(vertices); !status) {
        return status.error();
    }
    double perimeter = 0.0;
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        const Coordinate2& a = vertices[i];
        const Coordinate2& b = vertices[(i + 1) % vertices.size()];
        perimeter += std::hypot(b.northing - a.northing, b.easting - a.easting);
    }
    return perimeter;
}

Result<Coordinate2> polygonCentroid(std::span<const Coordinate2> vertices)
{
    if (Status status = checkPolygon(vertices); !status) {
        return status.error();
    }
    const PolygonSums sums = polygonSums(vertices);
    // Degenerate when the area is negligible against the polygon's own extent
    // (a dimensionless ratio, hence the absolute tolerance).
    if (!(std::abs(sums.twiceArea) > tol::kAbsolute * sums.extentSquared)) {
        return makeError(ErrorCode::InvalidArgument,
                         "polygon centroid: the area is degenerate (collinear vertices)");
    }
    const Coordinate2 origin = vertices.front();
    return Coordinate2{origin.northing + sums.momentNorth / (3.0 * sums.twiceArea),
                       origin.easting + sums.momentEast / (3.0 * sums.twiceArea)};
}

Result<double> horizontalDistance(double slopeDistance, double zenithAngle)
{
    if (Status status = checkSlope(slopeDistance, zenithAngle); !status) {
        return status.error();
    }
    return slopeDistance * std::sin(zenithAngle);
}

Result<double> trigonometricHeightDifference(double slopeDistance, double zenithAngle,
                                             double instrumentHeight, double targetHeight)
{
    if (Status status = checkSlope(slopeDistance, zenithAngle); !status) {
        return status.error();
    }
    if (!std::isfinite(instrumentHeight) || !std::isfinite(targetHeight)) {
        return makeError(ErrorCode::InvalidArgument,
                         "instrument and target heights must be finite");
    }
    return slopeDistance * std::cos(zenithAngle) + instrumentHeight - targetHeight;
}

} // namespace katana::survey
