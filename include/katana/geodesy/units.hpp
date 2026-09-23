#pragma once

// Centralised unit conversions (PLAN.MD Phase 11).
//
// Every length and angle conversion in Katana goes through this header so that
// the definition of a unit exists exactly once. The length DEFINITIONS
// themselves are one layer down, in katana/math/unit_ratio.hpp, so that the
// survey parsers - which may not see geodesy - use the same numbers rather than
// a copy of them; this header is the catalogue built on them.
//
// Numerical policy
//   * Length units are stored as EXACT rational numbers of metres
//     (international foot = 381/1250 m, US survey foot = 1200/3937 m). A
//     conversion is evaluated as (value * p) / q with p and q exactly
//     representable integers, i.e. at most two correctly rounded operations
//     (one when value * p is exact, which is the case for ordinary coordinates).
//   * Angle units other than the radian are exact fractions of a full turn
//     (360 deg = 400 gon = 1 296 000 arc-seconds); conversions between them
//     never touch pi. Conversions involving the radian use the factors
//     (2*pi)/n and n/(2*pi), which for the degree are bit-identical to
//     katana::math::kDegToRad and kRadToDeg.
//   * There is NO default unit: an unknown unit name is an error, never a
//     silent factor of 1.
//
// "foot" / "ft" ALWAYS mean the international foot (0.3048 m exactly), matching
// EPSG ("foot", EPSG:9002) and PROJ ("ft"). The US survey foot must be named
// explicitly ("ftUS", "us-ft", "US survey foot"). The two differ by 2 ppm, which
// is 4 ft at a state-plane coordinate of 2 000 000 ft.

#include <optional>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::geodesy {

enum class LengthUnit {
    Millimetre,
    Centimetre,
    Metre,
    Kilometre,
    InternationalInch,  // 0.0254 m
    InternationalFoot,  // 0.3048 m (EPSG:9002)
    InternationalYard,  // 0.9144 m
    InternationalChain, // 66 international feet = 20.1168 m (EPSG:9097)
    InternationalLink,  // 1/100 chain = 0.201168 m (EPSG:9098)
    InternationalMile,  // 5280 international feet = 1609.344 m
    NauticalMile,       // 1852 m
    UsSurveyFoot,       // 1200/3937 m (EPSG:9003)
    UsSurveyChain,      // 66 US survey feet (EPSG:9033)
    UsSurveyLink,       // 1/100 US survey chain (EPSG:9034)
    UsSurveyMile,       // 5280 US survey feet (EPSG:9035)
};

enum class AngleUnit {
    Radian,
    Degree,
    Gon, // also called grad / gradian: 400 per turn
    ArcMinute,
    ArcSecond,
};

// Metres in one `unit`: the correctly rounded value of the exact rational.
[[nodiscard]] double metresPerUnit(LengthUnit unit);

// Radians in one `unit`.
[[nodiscard]] double radiansPerUnit(AngleUnit unit);

// The unit whose size is `metres`, e.g. a conversion factor read from a WKT
// LENGTHUNIT or a LandXML header. Compared with math::tolerance::kRelative,
// which is 600 times finer than the 2 ppm separating the two feet, so they are
// never confused; nullopt when no known unit matches.
[[nodiscard]] std::optional<LengthUnit> lengthUnitFromMetresPerUnit(double metres);

[[nodiscard]] double convertLength(double value, LengthUnit from, LengthUnit to);
[[nodiscard]] double convertAngle(double value, AngleUnit from, AngleUnit to);

// EPSG-style names ("metre", "foot", "US survey foot", "degree", "grad", ...).
[[nodiscard]] std::string_view toString(LengthUnit unit);
[[nodiscard]] std::string_view toString(AngleUnit unit);

// Short symbols ("m", "ft", "ftUS", "deg", "gon", ...).
[[nodiscard]] std::string_view abbreviation(LengthUnit unit);
[[nodiscard]] std::string_view abbreviation(AngleUnit unit);

// Parses a unit name or symbol. Matching ignores case, spaces, '-' and '_', so
// "US survey foot", "us-ft", "ftUS" and "Foot_US" all name the US survey foot.
// Accepts EPSG names, PROJ unit ids and common survey abbreviations. Unknown or
// empty text fails with ErrorCode::ParseFailure.
[[nodiscard]] core::Result<LengthUnit> parseLengthUnit(std::string_view text);
[[nodiscard]] core::Result<AngleUnit> parseAngleUnit(std::string_view text);

} // namespace katana::geodesy
