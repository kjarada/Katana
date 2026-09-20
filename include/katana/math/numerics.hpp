#pragma once

// Central numerical policy (PLAN.MD sections 7 and 35).
//
// Every tolerance used anywhere in Katana is one of the named constants below.
// Code must not invent local epsilons; if a new kind of tolerance is needed it
// is added here, with its unit and rationale.
//
// All quantities are IEEE-754 binary64. Model units are metres unless a
// document says otherwise. For reference, the spacing of doubles (1 ulp) is
// ~1.9e-9 at a UTM northing of 1e7, so no linear tolerance below ~1e-8 can be
// meaningful for projected survey coordinates.

#include <algorithm>
#include <cmath>
#include <limits>

namespace katana::math {

inline constexpr double kPi = 3.14159265358979323846;
inline constexpr double kTwoPi = 2.0 * kPi;
inline constexpr double kHalfPi = 0.5 * kPi;
inline constexpr double kDegToRad = kPi / 180.0;
inline constexpr double kRadToDeg = 180.0 / kPi;

namespace tolerance {

// Dimensionless. Floor below which a normalised quantity (a sine, a unit
// determinant, a barycentric weight) is treated as zero.
inline constexpr double kAbsolute = 1e-12;

// Dimensionless. Two general-purpose values are equal when they differ by less
// than this fraction of their magnitude (magnitudes below 1 are treated as 1).
inline constexpr double kRelative = 1e-9;

// Radians (~2e-5 arc-seconds). Directions closer than this are parallel.
inline constexpr double kAngular = 1e-10;

// Model units (0.1 micrometre for metres). Geometric features closer than this
// coincide: duplicate vertices, a point lying on a curve, tangent contact. It is
// ~50 ulp at the largest projected coordinates, leaving room for the rounding of
// a few chained operations. Matches common CAD kernel practice (1e-7).
inline constexpr double kGeometric = 1e-7;

// Model units (0.1 mm). Survey coordinates closer than this are the same ground
// mark. An order of magnitude below the best achievable field precision.
inline constexpr double kCoordinate = 1e-4;

} // namespace tolerance

// Combined absolute/relative comparison: |a-b| <= eps * max(1, |a|, |b|).
[[nodiscard]] inline bool nearlyEqual(double a, double b, double epsilon = tolerance::kRelative)
{
    if (a == b) {
        return true; // also covers equal infinities
    }
    if (!std::isfinite(a) || !std::isfinite(b)) {
        return false; // otherwise inf <= eps * inf would compare equal to anything
    }
    const double diff = std::abs(a - b);
    const double scale = std::max({1.0, std::abs(a), std::abs(b)});
    return diff <= epsilon * scale;
}

[[nodiscard]] inline bool nearlyZero(double value, double epsilon = tolerance::kAbsolute)
{
    return std::abs(value) <= epsilon;
}

// Linear quantities in model units compared with the geometric tolerance.
[[nodiscard]] inline bool lengthsEqual(double a, double b, double epsilon = tolerance::kGeometric)
{
    return std::abs(a - b) <= epsilon;
}

[[nodiscard]] inline double clamp(double value, double minValue, double maxValue)
{
    return std::min(std::max(value, minValue), maxValue);
}

[[nodiscard]] inline double lerp(double a, double b, double t)
{
    return a + (b - a) * t;
}

// Wraps an angle into [0, 2*pi).
[[nodiscard]] inline double normalizeAngle(double radians)
{
    double wrapped = std::fmod(radians, kTwoPi);
    if (wrapped < 0.0) {
        wrapped += kTwoPi;
    }
    // fmod of a tiny negative value can round back up to exactly 2*pi.
    return wrapped >= kTwoPi ? 0.0 : wrapped;
}

// Wraps an angle into (-pi, pi].
[[nodiscard]] inline double normalizeAngleSigned(double radians)
{
    const double wrapped = normalizeAngle(radians);
    return wrapped > kPi ? wrapped - kTwoPi : wrapped;
}

[[nodiscard]] inline bool anglesEqual(double a, double b, double epsilon = tolerance::kAngular)
{
    return std::abs(normalizeAngleSigned(a - b)) <= epsilon;
}

[[nodiscard]] inline bool isFinite(double value)
{
    return std::isfinite(value);
}

} // namespace katana::math
