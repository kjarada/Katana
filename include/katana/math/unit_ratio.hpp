#pragma once

// The exact size of each length unit, as a rational number of metres.
//
// katana/geodesy/units.hpp is the program's catalogue of units - names,
// parsing, conversion - and it is built from THESE definitions. They live here,
// one layer down, because the survey model and the survey file parsers read
// files in feet and links and may not see geodesy (tools/check_layering.cmake:
// an importer that can reach the coordinate transformer will eventually
// transform something). They had started to repeat the ratios, and two
// definitions of a foot are one too many; math is the layer all of them see.
//
// Why rationals rather than doubles: a conversion is then (value * p) / q with p
// and q exactly representable integers - at most two correctly rounded
// operations, and exactly one whenever value * p is exact, which it is for any
// ordinary coordinate. A pre-divided double such as 0.3048 is already one
// rounding in, before the value has been touched.

#include <cstdint>
#include <numeric>

namespace katana::math {

// numerator / denominator metres. Both positive for every definition below.
struct UnitRatio {
    std::int64_t numerator = 1;
    std::int64_t denominator = 1;

    friend constexpr bool operator==(const UnitRatio&, const UnitRatio&) = default;
};

namespace units {

// International units derive from the 1959 international yard (0.9144 m
// exactly); US survey units from the 1893 Mendenhall order (1 m = 39.37 in,
// hence 1 ft = 1200/3937 m exactly). EPSG codes are the unit-of-measure
// records these match.
inline constexpr UnitRatio kMillimetre{1, 1000};
inline constexpr UnitRatio kCentimetre{1, 100};
inline constexpr UnitRatio kMetre{1, 1};                  // EPSG:9001
inline constexpr UnitRatio kKilometre{1000, 1};           // EPSG:9036
inline constexpr UnitRatio kInternationalInch{127, 5000}; // 0.0254 m
inline constexpr UnitRatio kInternationalFoot{381, 1250}; // 0.3048 m, EPSG:9002
inline constexpr UnitRatio kInternationalYard{1143, 1250};
inline constexpr UnitRatio kInternationalChain{12573, 625}; // 66 ft, EPSG:9097
inline constexpr UnitRatio kInternationalLink{12573, 62500}; // 1/100 chain, EPSG:9098
inline constexpr UnitRatio kInternationalMile{201168, 125}; // 5280 ft
inline constexpr UnitRatio kNauticalMile{1852, 1};
inline constexpr UnitRatio kUsSurveyFoot{1200, 3937};   // EPSG:9003
inline constexpr UnitRatio kUsSurveyChain{79200, 3937}; // 66 US ft, EPSG:9033
inline constexpr UnitRatio kUsSurveyLink{792, 3937};    // 1/100 US chain, EPSG:9034
inline constexpr UnitRatio kUsSurveyMile{6336000, 3937}; // 5280 US ft, EPSG:9035

} // namespace units

// value * numerator / denominator with the fraction reduced first. Every
// definition above is small enough that the reduced integers are far below
// 2^53, so both convert to double exactly.
[[nodiscard]] constexpr double scaleByRatio(double value, std::int64_t numerator,
                                            std::int64_t denominator)
{
    const std::int64_t divisor = std::gcd(numerator, denominator);
    const auto p = static_cast<double>(numerator / divisor);
    const auto q = static_cast<double>(denominator / divisor);
    return (value * p) / q;
}

[[nodiscard]] constexpr double toMetres(double value, UnitRatio unit)
{
    return scaleByRatio(value, unit.numerator, unit.denominator);
}

[[nodiscard]] constexpr double fromMetres(double metres, UnitRatio unit)
{
    return scaleByRatio(metres, unit.denominator, unit.numerator);
}

// The correctly rounded double nearest the ratio, for the few callers that want
// a factor rather than a conversion (reporting, unit matching).
[[nodiscard]] constexpr double ratioValue(UnitRatio unit)
{
    return static_cast<double>(unit.numerator) / static_cast<double>(unit.denominator);
}

} // namespace katana::math
