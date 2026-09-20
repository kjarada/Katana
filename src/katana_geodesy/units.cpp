#include "katana/geodesy/units.hpp"

#include <array>
#include <cstdint>
#include <numeric>
#include <stdexcept>
#include <string>

#include "katana/math/numerics.hpp"

namespace katana::geodesy {

namespace {

// Exact size of a length unit: numerator / denominator metres.
struct Ratio {
    std::int64_t numerator;
    std::int64_t denominator;
};

// The defining constants. International units derive from the 1959 yard
// (0.9144 m exactly); US survey units from the 1893 Mendenhall order
// (1 m = 39.37 in, hence 1 ft = 1200/3937 m exactly).
constexpr Ratio lengthRatio(LengthUnit unit)
{
    switch (unit) {
    case LengthUnit::Millimetre:
        return {1, 1000};
    case LengthUnit::Centimetre:
        return {1, 100};
    case LengthUnit::Metre:
        return {1, 1};
    case LengthUnit::Kilometre:
        return {1000, 1};
    case LengthUnit::InternationalInch:
        return {127, 5000};
    case LengthUnit::InternationalFoot:
        return {381, 1250};
    case LengthUnit::InternationalYard:
        return {1143, 1250};
    case LengthUnit::InternationalChain:
        return {12573, 625}; // 66 * 381/1250
    case LengthUnit::InternationalLink:
        return {12573, 62500};
    case LengthUnit::InternationalMile:
        return {201168, 125}; // 5280 * 381/1250
    case LengthUnit::NauticalMile:
        return {1852, 1};
    case LengthUnit::UsSurveyFoot:
        return {1200, 3937};
    case LengthUnit::UsSurveyChain:
        return {79200, 3937};
    case LengthUnit::UsSurveyLink:
        return {792, 3937};
    case LengthUnit::UsSurveyMile:
        return {6336000, 3937};
    }
    throw std::logic_error("katana::geodesy: LengthUnit value outside the enumeration");
}

// Units per full turn; 0 marks the radian, whose size is irrational.
constexpr std::int64_t unitsPerTurn(AngleUnit unit)
{
    switch (unit) {
    case AngleUnit::Radian:
        return 0;
    case AngleUnit::Degree:
        return 360;
    case AngleUnit::Gon:
        return 400;
    case AngleUnit::ArcMinute:
        return 21600;
    case AngleUnit::ArcSecond:
        return 1296000;
    }
    throw std::logic_error("katana::geodesy: AngleUnit value outside the enumeration");
}

// value * numerator / denominator with the fraction reduced first. All unit
// constants are small enough that the products stay far below 2^53, so both
// integers convert to double exactly.
double scaleByRatio(double value, std::int64_t numerator, std::int64_t denominator)
{
    const std::int64_t divisor = std::gcd(numerator, denominator);
    const auto p = static_cast<double>(numerator / divisor);
    const auto q = static_cast<double>(denominator / divisor);
    return (value * p) / q;
}

// Lower-cases ASCII and drops spaces, '-' and '_' so that spelling variants of
// one name compare equal.
std::string normaliseUnitText(std::string_view text)
{
    std::string key;
    key.reserve(text.size());
    for (const char ch : text) {
        if (ch == ' ' || ch == '\t' || ch == '-' || ch == '_') {
            continue;
        }
        key.push_back(ch >= 'A' && ch <= 'Z' ? static_cast<char>(ch - 'A' + 'a') : ch);
    }
    return key;
}

template <typename Unit> struct Alias {
    std::string_view key; // already normalised
    Unit unit;
};

constexpr std::array kLengthAliases = {
    Alias<LengthUnit>{"mm", LengthUnit::Millimetre},
    Alias<LengthUnit>{"millimetre", LengthUnit::Millimetre},
    Alias<LengthUnit>{"millimetres", LengthUnit::Millimetre},
    Alias<LengthUnit>{"millimeter", LengthUnit::Millimetre},
    Alias<LengthUnit>{"millimeters", LengthUnit::Millimetre},
    Alias<LengthUnit>{"cm", LengthUnit::Centimetre},
    Alias<LengthUnit>{"centimetre", LengthUnit::Centimetre},
    Alias<LengthUnit>{"centimetres", LengthUnit::Centimetre},
    Alias<LengthUnit>{"centimeter", LengthUnit::Centimetre},
    Alias<LengthUnit>{"centimeters", LengthUnit::Centimetre},
    Alias<LengthUnit>{"m", LengthUnit::Metre},
    Alias<LengthUnit>{"metre", LengthUnit::Metre},
    Alias<LengthUnit>{"metres", LengthUnit::Metre},
    Alias<LengthUnit>{"meter", LengthUnit::Metre},
    Alias<LengthUnit>{"meters", LengthUnit::Metre},
    Alias<LengthUnit>{"km", LengthUnit::Kilometre},
    Alias<LengthUnit>{"kilometre", LengthUnit::Kilometre},
    Alias<LengthUnit>{"kilometres", LengthUnit::Kilometre},
    Alias<LengthUnit>{"kilometer", LengthUnit::Kilometre},
    Alias<LengthUnit>{"kilometers", LengthUnit::Kilometre},
    Alias<LengthUnit>{"in", LengthUnit::InternationalInch},
    Alias<LengthUnit>{"inch", LengthUnit::InternationalInch},
    Alias<LengthUnit>{"inches", LengthUnit::InternationalInch},
    Alias<LengthUnit>{"ft", LengthUnit::InternationalFoot},
    Alias<LengthUnit>{"ift", LengthUnit::InternationalFoot},
    Alias<LengthUnit>{"foot", LengthUnit::InternationalFoot},
    Alias<LengthUnit>{"feet", LengthUnit::InternationalFoot},
    Alias<LengthUnit>{"internationalfoot", LengthUnit::InternationalFoot},
    Alias<LengthUnit>{"internationalfeet", LengthUnit::InternationalFoot},
    Alias<LengthUnit>{"yd", LengthUnit::InternationalYard},
    Alias<LengthUnit>{"yard", LengthUnit::InternationalYard},
    Alias<LengthUnit>{"yards", LengthUnit::InternationalYard},
    Alias<LengthUnit>{"ch", LengthUnit::InternationalChain},
    Alias<LengthUnit>{"chain", LengthUnit::InternationalChain},
    Alias<LengthUnit>{"chains", LengthUnit::InternationalChain},
    Alias<LengthUnit>{"lk", LengthUnit::InternationalLink},
    Alias<LengthUnit>{"link", LengthUnit::InternationalLink},
    Alias<LengthUnit>{"links", LengthUnit::InternationalLink},
    Alias<LengthUnit>{"mi", LengthUnit::InternationalMile},
    Alias<LengthUnit>{"mile", LengthUnit::InternationalMile},
    Alias<LengthUnit>{"miles", LengthUnit::InternationalMile},
    Alias<LengthUnit>{"statutemile", LengthUnit::InternationalMile},
    Alias<LengthUnit>{"nmi", LengthUnit::NauticalMile},
    Alias<LengthUnit>{"kmi", LengthUnit::NauticalMile}, // PROJ unit id
    Alias<LengthUnit>{"nauticalmile", LengthUnit::NauticalMile},
    Alias<LengthUnit>{"nauticalmiles", LengthUnit::NauticalMile},
    Alias<LengthUnit>{"ftus", LengthUnit::UsSurveyFoot},
    Alias<LengthUnit>{"usft", LengthUnit::UsSurveyFoot}, // PROJ unit id "us-ft"
    Alias<LengthUnit>{"sft", LengthUnit::UsSurveyFoot},
    Alias<LengthUnit>{"footus", LengthUnit::UsSurveyFoot}, // ESRI "Foot_US"
    Alias<LengthUnit>{"ussurveyfoot", LengthUnit::UsSurveyFoot},
    Alias<LengthUnit>{"ussurveyfeet", LengthUnit::UsSurveyFoot},
    Alias<LengthUnit>{"surveyfoot", LengthUnit::UsSurveyFoot},
    Alias<LengthUnit>{"surveyfeet", LengthUnit::UsSurveyFoot},
    Alias<LengthUnit>{"chus", LengthUnit::UsSurveyChain},
    Alias<LengthUnit>{"usch", LengthUnit::UsSurveyChain}, // PROJ unit id "us-ch"
    Alias<LengthUnit>{"ussurveychain", LengthUnit::UsSurveyChain},
    Alias<LengthUnit>{"ussurveychains", LengthUnit::UsSurveyChain},
    Alias<LengthUnit>{"lkus", LengthUnit::UsSurveyLink},
    Alias<LengthUnit>{"ussurveylink", LengthUnit::UsSurveyLink},
    Alias<LengthUnit>{"ussurveylinks", LengthUnit::UsSurveyLink},
    Alias<LengthUnit>{"mius", LengthUnit::UsSurveyMile},
    Alias<LengthUnit>{"usmi", LengthUnit::UsSurveyMile}, // PROJ unit id "us-mi"
    Alias<LengthUnit>{"ussurveymile", LengthUnit::UsSurveyMile},
    Alias<LengthUnit>{"ussurveymiles", LengthUnit::UsSurveyMile},
};

constexpr std::array kAngleAliases = {
    Alias<AngleUnit>{"rad", AngleUnit::Radian},
    Alias<AngleUnit>{"radian", AngleUnit::Radian},
    Alias<AngleUnit>{"radians", AngleUnit::Radian},
    Alias<AngleUnit>{"deg", AngleUnit::Degree},
    Alias<AngleUnit>{"degree", AngleUnit::Degree},
    Alias<AngleUnit>{"degrees", AngleUnit::Degree},
    Alias<AngleUnit>{"\xC2\xB0", AngleUnit::Degree}, // UTF-8 degree sign
    Alias<AngleUnit>{"gon", AngleUnit::Gon},
    Alias<AngleUnit>{"gons", AngleUnit::Gon},
    Alias<AngleUnit>{"grad", AngleUnit::Gon},
    Alias<AngleUnit>{"grads", AngleUnit::Gon},
    Alias<AngleUnit>{"gradian", AngleUnit::Gon},
    Alias<AngleUnit>{"gradians", AngleUnit::Gon},
    Alias<AngleUnit>{"arcmin", AngleUnit::ArcMinute},
    Alias<AngleUnit>{"arcminute", AngleUnit::ArcMinute},
    Alias<AngleUnit>{"arcminutes", AngleUnit::ArcMinute},
    Alias<AngleUnit>{"'", AngleUnit::ArcMinute},
    Alias<AngleUnit>{"arcsec", AngleUnit::ArcSecond},
    Alias<AngleUnit>{"arcsecond", AngleUnit::ArcSecond},
    Alias<AngleUnit>{"arcseconds", AngleUnit::ArcSecond},
    Alias<AngleUnit>{"\"", AngleUnit::ArcSecond},
};

template <typename Unit, std::size_t N>
core::Result<Unit> parseUnit(std::string_view text, const std::array<Alias<Unit>, N>& aliases,
                             std::string_view quantity)
{
    const std::string key = normaliseUnitText(text);
    for (const Alias<Unit>& alias : aliases) {
        if (alias.key == key) {
            return alias.unit;
        }
    }
    return core::makeError(core::ErrorCode::ParseFailure,
                           "unknown " + std::string(quantity) + " unit",
                           "input='" + std::string(text) + "'");
}

} // namespace

double metresPerUnit(LengthUnit unit)
{
    const Ratio ratio = lengthRatio(unit);
    return static_cast<double>(ratio.numerator) / static_cast<double>(ratio.denominator);
}

double radiansPerUnit(AngleUnit unit)
{
    const std::int64_t perTurn = unitsPerTurn(unit);
    return perTurn == 0 ? 1.0 : katana::math::kTwoPi / static_cast<double>(perTurn);
}

std::optional<LengthUnit> lengthUnitFromMetresPerUnit(double metres)
{
    // Every enumerator, so that a new unit only has to be added here and in
    // lengthRatio() (the switch there makes the compiler flag an omission).
    constexpr std::array kAllLengthUnits = {
        LengthUnit::Millimetre,        LengthUnit::Centimetre,
        LengthUnit::Metre,             LengthUnit::Kilometre,
        LengthUnit::InternationalInch, LengthUnit::InternationalFoot,
        LengthUnit::InternationalYard, LengthUnit::InternationalChain,
        LengthUnit::InternationalLink, LengthUnit::InternationalMile,
        LengthUnit::NauticalMile,      LengthUnit::UsSurveyFoot,
        LengthUnit::UsSurveyChain,     LengthUnit::UsSurveyLink,
        LengthUnit::UsSurveyMile,
    };
    for (const LengthUnit unit : kAllLengthUnits) {
        if (katana::math::nearlyEqual(metres, metresPerUnit(unit))) {
            return unit;
        }
    }
    return std::nullopt;
}

double convertLength(double value, LengthUnit from, LengthUnit to)
{
    if (from == to) {
        return value; // guarantees bit-exact identity
    }
    const Ratio source = lengthRatio(from);
    const Ratio target = lengthRatio(to);
    return scaleByRatio(value, source.numerator * target.denominator,
                        source.denominator * target.numerator);
}

double convertAngle(double value, AngleUnit from, AngleUnit to)
{
    if (from == to) {
        return value;
    }
    const std::int64_t fromPerTurn = unitsPerTurn(from);
    const std::int64_t toPerTurn = unitsPerTurn(to);
    if (fromPerTurn == 0) {
        // (n / 2pi) is a single correctly rounded constant; for the degree it is
        // bit-identical to math::kRadToDeg because 2*kPi is exact.
        return value * (static_cast<double>(toPerTurn) / katana::math::kTwoPi);
    }
    if (toPerTurn == 0) {
        return value * (katana::math::kTwoPi / static_cast<double>(fromPerTurn));
    }
    return scaleByRatio(value, toPerTurn, fromPerTurn);
}

std::string_view toString(LengthUnit unit)
{
    switch (unit) {
    case LengthUnit::Millimetre:
        return "millimetre";
    case LengthUnit::Centimetre:
        return "centimetre";
    case LengthUnit::Metre:
        return "metre";
    case LengthUnit::Kilometre:
        return "kilometre";
    case LengthUnit::InternationalInch:
        return "inch";
    case LengthUnit::InternationalFoot:
        return "foot";
    case LengthUnit::InternationalYard:
        return "yard";
    case LengthUnit::InternationalChain:
        return "chain";
    case LengthUnit::InternationalLink:
        return "link";
    case LengthUnit::InternationalMile:
        return "Statute mile";
    case LengthUnit::NauticalMile:
        return "nautical mile";
    case LengthUnit::UsSurveyFoot:
        return "US survey foot";
    case LengthUnit::UsSurveyChain:
        return "US survey chain";
    case LengthUnit::UsSurveyLink:
        return "US survey link";
    case LengthUnit::UsSurveyMile:
        return "US survey mile";
    }
    return "unknown";
}

std::string_view toString(AngleUnit unit)
{
    switch (unit) {
    case AngleUnit::Radian:
        return "radian";
    case AngleUnit::Degree:
        return "degree";
    case AngleUnit::Gon:
        return "grad";
    case AngleUnit::ArcMinute:
        return "arc-minute";
    case AngleUnit::ArcSecond:
        return "arc-second";
    }
    return "unknown";
}

std::string_view abbreviation(LengthUnit unit)
{
    switch (unit) {
    case LengthUnit::Millimetre:
        return "mm";
    case LengthUnit::Centimetre:
        return "cm";
    case LengthUnit::Metre:
        return "m";
    case LengthUnit::Kilometre:
        return "km";
    case LengthUnit::InternationalInch:
        return "in";
    case LengthUnit::InternationalFoot:
        return "ft";
    case LengthUnit::InternationalYard:
        return "yd";
    case LengthUnit::InternationalChain:
        return "ch";
    case LengthUnit::InternationalLink:
        return "lk";
    case LengthUnit::InternationalMile:
        return "mi";
    case LengthUnit::NauticalMile:
        return "nmi";
    case LengthUnit::UsSurveyFoot:
        return "ftUS";
    case LengthUnit::UsSurveyChain:
        return "chUS";
    case LengthUnit::UsSurveyLink:
        return "lkUS";
    case LengthUnit::UsSurveyMile:
        return "miUS";
    }
    return "?";
}

std::string_view abbreviation(AngleUnit unit)
{
    switch (unit) {
    case AngleUnit::Radian:
        return "rad";
    case AngleUnit::Degree:
        return "deg";
    case AngleUnit::Gon:
        return "gon";
    case AngleUnit::ArcMinute:
        return "arcmin";
    case AngleUnit::ArcSecond:
        return "arcsec";
    }
    return "?";
}

core::Result<LengthUnit> parseLengthUnit(std::string_view text)
{
    return parseUnit(text, kLengthAliases, "length");
}

core::Result<AngleUnit> parseAngleUnit(std::string_view text)
{
    return parseUnit(text, kAngleAliases, "angle");
}

} // namespace katana::geodesy
