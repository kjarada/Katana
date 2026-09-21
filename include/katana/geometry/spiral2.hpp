#pragma once

// The clothoid, or Euler spiral (PLAN.MD Phase 21).
//
// A curve whose curvature changes linearly with distance along it. That is the
// shape a vehicle actually follows when its driver turns the wheel at a steady
// rate, which is why every road and railway standard uses it as the transition
// between a straight and a circular curve: on a straight the curvature is 0,
// on the curve it is 1/R, and a spiral takes the traveller from one to the
// other with no jump in lateral acceleration.
//
// The general form is kept - curvature from `startCurvature` to `endCurvature`
// over `length` - rather than only the textbook 0 -> 1/R case, because a real
// alignment also needs the transition between two curves of different radius,
// and the reverse spiral leaving a curve. All three are the same object with
// different end curvatures.
//
// Positive curvature turns left (counter-clockwise), matching the sign of
// `Arc2::sweep` and every other angle in this layer.
//
// WHY QUADRATURE AND NOT THE SERIES. The road-design texts give x and y as
// power series in the deflection angle (x = L(1 - t^2/10 + t^4/216 - ...)).
// Those series are exact only for a spiral starting from zero curvature; the
// general case has no such closed form. Composite Gauss-Legendre over
// sub-intervals of bounded turn handles every case with the same code and is
// accurate to about 1e-15 relative, which on a 100 m spiral is a tenth of a
// nanometre. It is also deterministic (Rule 7): the sub-division depends only
// on the spiral and the distance asked for.

#include <optional>

#include "katana/geometry/primitives2d.hpp"
#include "katana/math/vec2.hpp"

namespace katana::geometry {

struct Spiral2 {
    Point2 start;                // the point at distance 0
    double startDirection = 0.0; // tangent direction at distance 0, radians CCW from +x
    double startCurvature = 0.0; // 1 / radius at distance 0; 0 on a straight
    double endCurvature = 0.0;   // 1 / radius at distance `length`
    double length = 0.0;         // arc length, > 0

    // The textbook transition: from a straight (curvature 0) into a curve of
    // radius `radius`, over `length`. `radius` > 0 turns left. The flatness
    // parameter the standards quote is A = sqrt(radius * length).
    [[nodiscard]] static Spiral2 fromStraightToRadius(const Point2& start, double direction,
                                                      double radius, double length);

    // The spiral's A parameter, sqrt(R * L) for a spiral from a straight; in
    // general 1 / sqrt(|rate of change of curvature|). Infinite for a curve
    // of constant curvature, which is not a spiral - see isDegenerate.
    [[nodiscard]] double flatness() const;

    // Curvature varies linearly, so these are exact.
    [[nodiscard]] double curvatureAt(double distance) const;
    [[nodiscard]] double directionAt(double distance) const;
    // The angle turned through over the whole spiral: (k0 + k1) * L / 2.
    [[nodiscard]] double totalTurn() const { return directionAt(length) - startDirection; }

    [[nodiscard]] Point2 pointAt(double distance) const;
    [[nodiscard]] katana::math::Vec2 tangentAt(double distance) const;
    [[nodiscard]] Point2 endPoint() const { return pointAt(length); }
    [[nodiscard]] double endDirection() const { return directionAt(length); }

    // Radius at a distance, for callers that think in radii; infinite where
    // the curvature is zero.
    [[nodiscard]] double radiusAt(double distance) const;

    // A spiral whose curvature does not change is a circular arc or a straight
    // line, and callers that need one should build that instead. Such a
    // `Spiral2` still evaluates correctly - a constant integrand is the easy
    // case for the quadrature - but `flatness()` has no meaning for it.
    [[nodiscard]] bool isDegenerate() const;

    // The circular arc this spiral becomes when its curvature is constant, or
    // nullopt when it is not (or the curvature is zero, which is a line).
    // Exact: a spiral with k0 == k1 == 1/R IS that arc, and reporting it as
    // one lets the section and stationing code treat it with the closed-form
    // arc arithmetic it already has.
    [[nodiscard]] std::optional<Arc2> asArc() const;

    // Length of the sampled polyline needed to draw this spiral to within
    // `tolerance` of the true curve, using the sagitta rule at the tightest
    // curvature. Never fewer than two points.
    [[nodiscard]] std::size_t chordCountFor(double tolerance) const;

    friend bool operator==(const Spiral2&, const Spiral2&) = default;
};

} // namespace katana::geometry
