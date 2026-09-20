#include "katana/geodesy/geodesic.hpp"

#include <cmath>

// Karney's geodesic routines, distributed as part of PROJ (third-party: stays
// in this translation unit, PLAN.MD Rule 4).
#include <geodesic.h>

#include "katana/math/numerics.hpp"

namespace katana::geodesy {

namespace {

core::Status validateEllipsoid(const Ellipsoid& ellipsoid)
{
    const bool axisValid = std::isfinite(ellipsoid.semiMajorAxis) && ellipsoid.semiMajorAxis > 0.0;
    // 1/f == 0 encodes a sphere; otherwise f must be a proper flattening < 1.
    const bool flatteningValid =
        ellipsoid.inverseFlattening == 0.0 ||
        (std::isfinite(ellipsoid.inverseFlattening) && ellipsoid.inverseFlattening > 1.0);
    if (axisValid && flatteningValid) {
        return {};
    }
    return core::makeError(core::ErrorCode::InvalidArgument,
                           "ellipsoid needs a positive semi-major axis and an inverse "
                           "flattening that is 0 (sphere) or greater than 1");
}

core::Status validatePosition(const GeographicCoordinate& position)
{
    if (std::isfinite(position.latitude) && std::isfinite(position.longitude) &&
        std::abs(position.latitude) <= 90.0) {
        return {};
    }
    return core::makeError(core::ErrorCode::InvalidArgument,
                           "geographic position must be finite with latitude in [-90, 90]");
}

// Wraps an azimuth from the library's (-180, 180] into the survey range [0, 360).
double toCompassDegrees(double azimuthDegrees)
{
    double wrapped = std::fmod(azimuthDegrees, 360.0);
    if (wrapped < 0.0) {
        wrapped += 360.0;
    }
    return wrapped >= 360.0 ? 0.0 : wrapped; // a tiny negative value can round up to 360
}

} // namespace

core::Result<GeodesicInverseResult> geodesicInverse(const Ellipsoid& ellipsoid,
                                                    const GeographicCoordinate& from,
                                                    const GeographicCoordinate& to)
{
    if (auto status = validateEllipsoid(ellipsoid); !status) {
        return status.error();
    }
    for (const GeographicCoordinate* position : {&from, &to}) {
        if (auto status = validatePosition(*position); !status) {
            return status.error();
        }
    }

    geod_geodesic geodesic;
    geod_init(&geodesic, ellipsoid.semiMajorAxis, ellipsoid.flattening());
    double distance = 0.0;
    double azimuthAtStart = 0.0;
    double azimuthAtEnd = 0.0; // direction of travel on arrival, NOT the back azimuth
    geod_inverse(&geodesic, from.latitude, from.longitude, to.latitude, to.longitude, &distance,
                 &azimuthAtStart, &azimuthAtEnd);
    if (!std::isfinite(distance) || !std::isfinite(azimuthAtStart) ||
        !std::isfinite(azimuthAtEnd)) {
        return core::makeError(core::ErrorCode::Internal,
                               "geodesic inverse computation did not produce a finite result");
    }
    return GeodesicInverseResult{distance, toCompassDegrees(azimuthAtStart),
                                 toCompassDegrees(azimuthAtEnd + 180.0)};
}

core::Result<GeodesicDirectResult> geodesicDirect(const Ellipsoid& ellipsoid,
                                                  const GeographicCoordinate& from,
                                                  double azimuthDegrees, double distanceMetres)
{
    if (auto status = validateEllipsoid(ellipsoid); !status) {
        return status.error();
    }
    if (auto status = validatePosition(from); !status) {
        return status.error();
    }
    if (!std::isfinite(azimuthDegrees) || !std::isfinite(distanceMetres)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "azimuth and distance must be finite");
    }

    geod_geodesic geodesic;
    geod_init(&geodesic, ellipsoid.semiMajorAxis, ellipsoid.flattening());
    double latitude = 0.0;
    double longitude = 0.0;
    double azimuthAtEnd = 0.0;
    geod_direct(&geodesic, from.latitude, from.longitude, azimuthDegrees, distanceMetres,
                &latitude, &longitude, &azimuthAtEnd);
    if (!std::isfinite(latitude) || !std::isfinite(longitude) || !std::isfinite(azimuthAtEnd)) {
        return core::makeError(core::ErrorCode::Internal,
                               "geodesic direct computation did not produce a finite result");
    }
    return GeodesicDirectResult{GeographicCoordinate{latitude, longitude, 0.0},
                                toCompassDegrees(azimuthAtEnd + 180.0)};
}

double gaussianMeanRadius(const Ellipsoid& ellipsoid, double latitudeDegrees)
{
    // M = a (1 - e^2) / w^3, N = a / w, w^2 = 1 - e^2 sin^2(phi)  =>
    // sqrt(M N) = a sqrt(1 - e^2) / w^2.
    const double eSquared = ellipsoid.eccentricitySquared();
    const double sinLatitude = std::sin(latitudeDegrees * katana::math::kDegToRad);
    return ellipsoid.semiMajorAxis * std::sqrt(1.0 - eSquared) /
           (1.0 - eSquared * sinLatitude * sinLatitude);
}

double elevationFactor(double radiusMetres, double ellipsoidalHeightMetres)
{
    return radiusMetres / (radiusMetres + ellipsoidalHeightMetres);
}

} // namespace katana::geodesy
