#pragma once

// Coordinate geometry (PLAN.MD Phase 12): inverse / forward, polygon area,
// perimeter and centroid, slope reductions.
//
// Numerical assumptions
//   * Plane (grid) computations; no scale factor, convergence or curvature.
//   * azimuth = atan2(dE, dN), clockwise from grid north, in [0, 2*pi).
//   * Two positions closer than math::tolerance::kCoordinate (0.1 mm) are the
//     same ground mark; the azimuth between them is undefined.
//   * Polygon integrals are evaluated relative to the first vertex. With UTM
//     sized coordinates (E ~ 5e5, N ~ 5e6) the products of a naive shoelace sum
//     are ~1e12 and their rounding (~1e-4 m^2 each) would swamp the area of a
//     parcel; the coordinate differences are small and (nearly) exact.

#include <span>

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"

namespace katana::survey {

struct InverseResult {
    double azimuth = 0.0;  // radians, [0, 2*pi)
    double distance = 0.0; // metres
};

// Azimuth and horizontal distance from `from` to `to`. InvalidArgument for
// non-finite input or when the points coincide (azimuth undefined).
[[nodiscard]] katana::core::Result<InverseResult> inverse(const Coordinate2& from,
                                                          const Coordinate2& to);

// Position reached from `from` along `azimuth` (any finite value) over
// `distance` >= 0. InvalidArgument otherwise.
[[nodiscard]] katana::core::Result<Coordinate2> forward(const Coordinate2& from, double azimuth,
                                                        double distance);

// Polygon given by its vertices in order, implicitly closed; a repeated closing
// vertex is harmless. All polygon functions need at least 3 finite vertices
// (InvalidArgument otherwise). Self-intersecting polygons are not detected; the
// signed areas of their lobes cancel.

// Positive when the vertices run counter-clockwise on the map (east to the
// right, north up), negative when clockwise.
[[nodiscard]] katana::core::Result<double>
polygonSignedArea(std::span<const Coordinate2> vertices);
[[nodiscard]] katana::core::Result<double> polygonArea(std::span<const Coordinate2> vertices);
[[nodiscard]] katana::core::Result<double> polygonPerimeter(std::span<const Coordinate2> vertices);
// Area centroid. InvalidArgument when the area is degenerate (collinear points).
[[nodiscard]] katana::core::Result<Coordinate2>
polygonCentroid(std::span<const Coordinate2> vertices);

// Horizontal distance of a slope distance measured at `zenithAngle`:
// s * sin(z). Requires s >= 0 and z in [0, pi].
[[nodiscard]] katana::core::Result<double> horizontalDistance(double slopeDistance,
                                                              double zenithAngle);

// Trigonometric height difference H(target mark) - H(instrument mark):
// s * cos(z) + instrumentHeight - targetHeight. No curvature or refraction
// correction, so it is meant for sight lengths below a few hundred metres.
[[nodiscard]] katana::core::Result<double> trigonometricHeightDifference(double slopeDistance,
                                                                         double zenithAngle,
                                                                         double instrumentHeight,
                                                                         double targetHeight);

} // namespace katana::survey
