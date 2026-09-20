#pragma once

// Geodesics on the ellipsoid and the grid-to-ground reduction factors.
//
// The inverse and direct problems are solved with the geodesic routines that
// ship with PROJ (C. F. F. Karney, "Algorithms for geodesics", J. Geodesy 87,
// 2013): accurate to about 15 nanometres and convergent for every pair of
// points, including nearly antipodal ones where Vincenty's iteration fails.
//
// Conventions: positions are GeographicCoordinate (degrees); heights are
// ignored - a geodesic lies ON the ellipsoid. Azimuths are DEGREES CLOCKWISE
// FROM NORTH in [0, 360), the surveying convention; this differs deliberately
// from the radians-counter-clockwise convention of katana::geometry, hence the
// unit in every field name.
//
// These functions are stateless and thread-safe.

#include "katana/core/error.hpp"
#include "katana/geodesy/coordinate.hpp"
#include "katana/geodesy/ellipsoid.hpp"

namespace katana::geodesy {

struct GeodesicInverseResult {
    double distanceMetres = 0.0;         // along the ellipsoid surface
    double forwardAzimuthDegrees = 0.0;  // at `from`, towards `to`
    double reverseAzimuthDegrees = 0.0;  // at `to`, back towards `from`
};

struct GeodesicDirectResult {
    GeographicCoordinate position;       // height is 0
    double reverseAzimuthDegrees = 0.0;  // at `position`, back towards the start
};

// Shortest path between two positions. For coincident points the distance is 0
// and the azimuths carry no information. Errors (InvalidArgument): ellipsoid
// with a <= 0 or an inverse flattening in (0, 1]; non-finite positions; latitude
// outside [-90, 90].
[[nodiscard]] core::Result<GeodesicInverseResult> geodesicInverse(const Ellipsoid& ellipsoid,
                                                                  const GeographicCoordinate& from,
                                                                  const GeographicCoordinate& to);

// Position reached from `from` after `distanceMetres` along the geodesic that
// leaves with `azimuthDegrees`. A negative distance travels backwards. The
// resulting longitude is in [-180, 180].
[[nodiscard]] core::Result<GeodesicDirectResult> geodesicDirect(const Ellipsoid& ellipsoid,
                                                                const GeographicCoordinate& from,
                                                                double azimuthDegrees,
                                                                double distanceMetres);

// Gaussian mean radius of curvature sqrt(M N) at a latitude, in metres: the
// radius of the sphere that best fits the ellipsoid there, used for the
// elevation factor.
[[nodiscard]] double gaussianMeanRadius(const Ellipsoid& ellipsoid, double latitudeDegrees);

// Elevation (sea-level) factor R / (R + h): ratio of a distance on the ellipsoid
// to the same distance measured horizontally at ellipsoidal height h. The
// combined grid-to-ground factor is pointScaleFactor * elevationFactor.
[[nodiscard]] double elevationFactor(double radiusMetres, double ellipsoidalHeightMetres);

} // namespace katana::geodesy
