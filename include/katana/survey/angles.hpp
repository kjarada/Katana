#pragma once

// Angle units, DMS text and bearing conversions (PLAN.MD Phase 12).
//
// Conventions
//   * Every angle is stored and computed in radians (IEEE-754 binary64).
//   * An azimuth is measured clockwise from grid north and lives in [0, 2*pi).
//   * A quadrant bearing is the acute angle from the north or south meridian
//     towards east or west, e.g. "S 12°30'00\" W"; its angle lives in [0, pi/2].
//   * DMS text is parsed and formatted through integer arc-second arithmetic so
//     that rounding never produces a field of 60 minutes or 60 seconds.

#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/math/numerics.hpp"

namespace katana::survey {

inline constexpr double kGonToRad = katana::math::kPi / 200.0;
inline constexpr double kRadToGon = 200.0 / katana::math::kPi;
inline constexpr double kArcSecondToRad = katana::math::kPi / 648000.0;
inline constexpr double kRadToArcSecond = 648000.0 / katana::math::kPi;

[[nodiscard]] constexpr double degreesToRadians(double degrees)
{
    return degrees * katana::math::kDegToRad;
}
[[nodiscard]] constexpr double radiansToDegrees(double radians)
{
    return radians * katana::math::kRadToDeg;
}
[[nodiscard]] constexpr double gonToRadians(double gon)
{
    return gon * kGonToRad;
}
[[nodiscard]] constexpr double radiansToGon(double radians)
{
    return radians * kRadToGon;
}
[[nodiscard]] constexpr double arcSecondsToRadians(double arcSeconds)
{
    return arcSeconds * kArcSecondToRad;
}
[[nodiscard]] constexpr double radiansToArcSeconds(double radians)
{
    return radians * kRadToArcSecond;
}

// Wraps any finite angle into the azimuth range [0, 2*pi).
[[nodiscard]] inline double normalizeAzimuth(double radians)
{
    return katana::math::normalizeAngle(radians);
}

// Sexagesimal angle. The sign is kept apart from the fields so that values in
// (-1°, 0°) such as -0°30' are representable.
struct DmsAngle {
    bool negative = false;
    unsigned degrees = 0;
    unsigned minutes = 0; // 0..59
    double seconds = 0.0; // [0, 60)

    friend bool operator==(const DmsAngle&, const DmsAngle&) = default;
};

// InvalidArgument when minutes >= 60, seconds outside [0, 60) or not finite.
[[nodiscard]] katana::core::Result<double> dmsToRadians(const DmsAngle& dms);

// Splits an angle into fields with the seconds rounded to `secondsDecimals`
// (0..9) decimals; the rounding carries into minutes and degrees. InvalidArgument
// for a non-finite angle, decimals out of range, or an angle so large that the
// requested resolution is not representable in a double.
[[nodiscard]] katana::core::Result<DmsAngle> radiansToDms(double radians, int secondsDecimals = 2);

// Parses "[+-]D[ M[ S]]" into radians. Accepted field marks: degrees `°`, `º`,
// `d`, `:`, `-`; minutes `'`, `′`, `m`, `:`, `-`; seconds `"`, `″`, `''`, `s`;
// blanks may replace or surround any mark. Only the last field may carry a
// fraction ("45.5", "45°30.25'", "45 30 15.5"). ParseFailure otherwise.
[[nodiscard]] katana::core::Result<double> parseDms(std::string_view text);

// Formats as `[-]D°MM'SS.ss"` (UTF-8 degree sign).
[[nodiscard]] katana::core::Result<std::string> formatDms(double radians, int secondsDecimals = 2);

enum class BearingMeridian { North, South };
enum class BearingSide { East, West };

struct QuadrantBearing {
    BearingMeridian meridian = BearingMeridian::North;
    double angle = 0.0; // radians, [0, pi/2]
    BearingSide side = BearingSide::East;

    friend bool operator==(const QuadrantBearing&, const QuadrantBearing&) = default;
};

// Quadrant choice on the axes: azimuth 0 -> N 0 E, pi/2 -> N 90 E, pi -> S 0 E,
// 3*pi/2 -> S 90 W. InvalidArgument for a non-finite azimuth.
[[nodiscard]] katana::core::Result<QuadrantBearing> azimuthToBearing(double azimuth);

// InvalidArgument when the angle is outside [0, pi/2] or not finite.
[[nodiscard]] katana::core::Result<double> bearingToAzimuth(const QuadrantBearing& bearing);

// Parses "N 45°30'15\" E" (any parseDms angle format between the letters, case
// insensitive) into an azimuth. ParseFailure on malformed text or an angle
// outside [0°, 90°].
[[nodiscard]] katana::core::Result<double> parseBearing(std::string_view text);

// Formats an azimuth as `N 45°30'15" E`.
[[nodiscard]] katana::core::Result<std::string> formatBearing(double azimuth,
                                                             int secondsDecimals = 0);

} // namespace katana::survey
