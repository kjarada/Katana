#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <limits>

#include "geodesy_test_support.hpp"
#include "katana/geodesy/units.hpp"
#include "katana/math/numerics.hpp"
#include "support/property.hpp"

using namespace katana::geodesy;
using katana::core::ErrorCode;
using katana::test::kPropertyIterations;
using katana::test::Random;

namespace {

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

constexpr std::array kAllAngleUnits = {
    AngleUnit::Radian,    AngleUnit::Degree,    AngleUnit::Gon,
    AngleUnit::ArcMinute, AngleUnit::ArcSecond,
};

// A conversion is (value * p) / q: two correctly rounded operations, each off by
// at most half a unit in the last place. A round trip is four. This bound is
// derived from IEEE-754, it is not a tolerance.
double roundTripBound(double value)
{
    return 4.0 * std::numeric_limits<double>::epsilon() * std::abs(value);
}

template <typename T, std::size_t N> T pick(Random& random, const std::array<T, N>& values)
{
    return values[static_cast<std::size_t>(random.integer(0, static_cast<int>(N) - 1))];
}

} // namespace

// ---- defining constants (legal definitions, not computed) ------------------------

TEST(GeodesyUnits, DefiningConstantsAreExact)
{
    // International yard and pound agreement, 1959: 1 ft = 0.3048 m exactly.
    EXPECT_EQ(metresPerUnit(LengthUnit::InternationalFoot), 0.3048);
    EXPECT_EQ(metresPerUnit(LengthUnit::InternationalInch), 0.0254);
    EXPECT_EQ(metresPerUnit(LengthUnit::InternationalYard), 0.9144);
    EXPECT_EQ(metresPerUnit(LengthUnit::InternationalMile), 1609.344);
    EXPECT_EQ(metresPerUnit(LengthUnit::InternationalChain), 20.1168);
    EXPECT_EQ(metresPerUnit(LengthUnit::InternationalLink), 0.201168);
    EXPECT_EQ(metresPerUnit(LengthUnit::NauticalMile), 1852.0);
    // Mendenhall order, 1893: 1 m = 39.37 in, hence 1 ftUS = 1200/3937 m exactly.
    EXPECT_EQ(metresPerUnit(LengthUnit::UsSurveyFoot), 1200.0 / 3937.0);
    EXPECT_EQ(metresPerUnit(LengthUnit::UsSurveyChain), 79200.0 / 3937.0);
    EXPECT_EQ(metresPerUnit(LengthUnit::Metre), 1.0);
}

TEST(GeodesyUnits, ExactConversionsOfRoundValues)
{
    // 3937 US survey feet are exactly 1200 m, and 1200 m exactly 3937 ftUS: the
    // Mendenhall Order of 1893 defines 1 m = 39.37 in, i.e. 1 ftUS = 1200/3937 m.
    // Both directions evaluate as (value * p) / q over the integers, so the
    // 3937 cancels and the result is exact, not merely close.
    EXPECT_EQ(convertLength(3937.0, LengthUnit::UsSurveyFoot, LengthUnit::Metre), 1200.0);
    EXPECT_EQ(convertLength(1200.0, LengthUnit::Metre, LengthUnit::UsSurveyFoot), 3937.0);
    EXPECT_EQ(convertLength(2000000.0, LengthUnit::InternationalFoot, LengthUnit::Metre), 609600.0);
    EXPECT_EQ(convertLength(1.0, LengthUnit::InternationalChain, LengthUnit::InternationalFoot),
              66.0);
    EXPECT_EQ(convertLength(1.0, LengthUnit::UsSurveyChain, LengthUnit::UsSurveyFoot), 66.0);
    EXPECT_EQ(convertLength(100.0, LengthUnit::InternationalLink, LengthUnit::InternationalChain),
              1.0);
    EXPECT_EQ(convertLength(1.0, LengthUnit::InternationalMile, LengthUnit::InternationalFoot),
              5280.0);
    EXPECT_EQ(convertLength(1.0, LengthUnit::UsSurveyMile, LengthUnit::UsSurveyFoot), 5280.0);
    EXPECT_EQ(convertLength(2.5, LengthUnit::Kilometre, LengthUnit::Millimetre), 2500000.0);
}

// The classic state-plane blunder. The two feet differ by exactly 2 parts per
// million: ftUS / ft = (1200/3937) / 0.3048 = 1.000002000004. A coordinate of
// 2 000 000 ft read in the wrong foot is wrong by 4 ft (1.2 m).
TEST(GeodesyUnits, UsSurveyFootDiffersFromInternationalFootByTwoPartsPerMillion)
{
    const double ratio =
        metresPerUnit(LengthUnit::UsSurveyFoot) / metresPerUnit(LengthUnit::InternationalFoot);
    // Exact value 1 + 2e-6 + 4e-12 + ...; nearlyEqual's 1e-9 resolves the 2e-6.
    EXPECT_TRUE(katana::math::nearlyEqual(ratio, 1.000002000004));
    EXPECT_GT(ratio, 1.0);

    const double statePlaneFeet = 2000000.0;
    const double asSurveyFeet =
        convertLength(statePlaneFeet, LengthUnit::UsSurveyFoot, LengthUnit::Metre);
    const double asInternationalFeet =
        convertLength(statePlaneFeet, LengthUnit::InternationalFoot, LengthUnit::Metre);
    const double blunderMetres = asSurveyFeet - asInternationalFeet;
    const double blunderFeet =
        convertLength(blunderMetres, LengthUnit::Metre, LengthUnit::InternationalFoot);

    // By hand: 2e6 * (1200/3937 - 0.3048) m = 2e6 * 6.096012192e-7 m
    //        = 1.2192024384 m = 4.000008000 ft.
    EXPECT_NEAR(blunderMetres, 1.2192024384, katana::math::tolerance::kGeometric);
    EXPECT_NEAR(blunderFeet, 4.000008, katana::math::tolerance::kGeometric);
}

TEST(GeodesyUnits, SameUnitConversionIsBitExact)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double value = random.real(-1e8, 1e8);
        for (const LengthUnit unit : kAllLengthUnits) {
            EXPECT_EQ(convertLength(value, unit, unit), value);
        }
        for (const AngleUnit unit : kAllAngleUnits) {
            EXPECT_EQ(convertAngle(value, unit, unit), value);
        }
    }
}

TEST(GeodesyUnitsProperty, LengthConversionsRoundTrip)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double value = random.real(-1e8, 1e8);
        const LengthUnit from = pick(random, kAllLengthUnits);
        const LengthUnit to = pick(random, kAllLengthUnits);
        const double back = convertLength(convertLength(value, from, to), to, from);
        EXPECT_LE(std::abs(back - value), roundTripBound(value))
            << toString(from) << " -> " << toString(to) << " value=" << value;
    }
}

TEST(GeodesyUnitsProperty, LengthConversionsAreTransitiveThroughMetres)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double value = random.real(-1e7, 1e7);
        const LengthUnit from = pick(random, kAllLengthUnits);
        const LengthUnit to = pick(random, kAllLengthUnits);
        const double direct = convertLength(value, from, to);
        const double viaMetres =
            convertLength(convertLength(value, from, LengthUnit::Metre), LengthUnit::Metre, to);
        EXPECT_LE(std::abs(direct - viaMetres), roundTripBound(direct));
    }
}

// ---- angles ------------------------------------------------------------------------

TEST(GeodesyUnits, AngleConversionsBetweenTurnFractionsAreExact)
{
    EXPECT_EQ(convertAngle(90.0, AngleUnit::Degree, AngleUnit::Gon), 100.0);
    EXPECT_EQ(convertAngle(400.0, AngleUnit::Gon, AngleUnit::Degree), 360.0);
    EXPECT_EQ(convertAngle(1.0, AngleUnit::Degree, AngleUnit::ArcSecond), 3600.0);
    EXPECT_EQ(convertAngle(1.0, AngleUnit::Degree, AngleUnit::ArcMinute), 60.0);
    EXPECT_EQ(convertAngle(1296000.0, AngleUnit::ArcSecond, AngleUnit::Gon), 400.0);
    EXPECT_EQ(convertAngle(52.0, AngleUnit::Gon, AngleUnit::Degree), 46.8); // NTF Lambert origin
}

TEST(GeodesyUnits, RadianConversionsMatchTheCentralConstantsBitForBit)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double value = random.real(-720.0, 720.0);
        EXPECT_EQ(convertAngle(value, AngleUnit::Degree, AngleUnit::Radian),
                  value * katana::math::kDegToRad);
        EXPECT_EQ(convertAngle(value, AngleUnit::Radian, AngleUnit::Degree),
                  value * katana::math::kRadToDeg);
    }
    EXPECT_EQ(radiansPerUnit(AngleUnit::Degree), katana::math::kDegToRad);
    EXPECT_EQ(radiansPerUnit(AngleUnit::Radian), 1.0);
    // 180 * (pi/180) need not round to pi exactly: compare to 4 ulp (gtest).
    EXPECT_DOUBLE_EQ(convertAngle(180.0, AngleUnit::Degree, AngleUnit::Radian), katana::math::kPi);
    EXPECT_DOUBLE_EQ(convertAngle(200.0, AngleUnit::Gon, AngleUnit::Radian), katana::math::kPi);
}

TEST(GeodesyUnitsProperty, AngleConversionsRoundTrip)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double value = random.real(-1e6, 1e6);
        const AngleUnit from = pick(random, kAllAngleUnits);
        const AngleUnit to = pick(random, kAllAngleUnits);
        const double back = convertAngle(convertAngle(value, from, to), to, from);
        EXPECT_LE(std::abs(back - value), roundTripBound(value))
            << toString(from) << " -> " << toString(to) << " value=" << value;
    }
}

// ---- parsing -----------------------------------------------------------------------

TEST(GeodesyUnits, ParsesNamesSymbolsAndProjIds)
{
    const auto expectLength = [](std::string_view text, LengthUnit expected) {
        const auto parsed = parseLengthUnit(text);
        ASSERT_TRUE(parsed.ok()) << "'" << text << "': " << parsed.error().describe();
        EXPECT_EQ(*parsed, expected) << "'" << text << "'";
    };
    expectLength("m", LengthUnit::Metre);
    expectLength("metre", LengthUnit::Metre);
    expectLength("Meters", LengthUnit::Metre);
    expectLength("  km ", LengthUnit::Kilometre);
    expectLength("ft", LengthUnit::InternationalFoot);
    expectLength("foot", LengthUnit::InternationalFoot);
    expectLength("International Foot", LengthUnit::InternationalFoot);
    expectLength("ftUS", LengthUnit::UsSurveyFoot);
    expectLength("us-ft", LengthUnit::UsSurveyFoot); // PROJ unit id
    expectLength("US survey foot", LengthUnit::UsSurveyFoot);
    expectLength("Foot_US", LengthUnit::UsSurveyFoot); // ESRI WKT
    expectLength("ch", LengthUnit::InternationalChain);
    expectLength("us-ch", LengthUnit::UsSurveyChain);
    expectLength("link", LengthUnit::InternationalLink);
    expectLength("kmi", LengthUnit::NauticalMile);

    const auto expectAngle = [](std::string_view text, AngleUnit expected) {
        const auto parsed = parseAngleUnit(text);
        ASSERT_TRUE(parsed.ok()) << "'" << text << "': " << parsed.error().describe();
        EXPECT_EQ(*parsed, expected) << "'" << text << "'";
    };
    expectAngle("rad", AngleUnit::Radian);
    expectAngle("Degree", AngleUnit::Degree);
    expectAngle("gon", AngleUnit::Gon);
    expectAngle("grad", AngleUnit::Gon);
    expectAngle("arc-second", AngleUnit::ArcSecond);
    expectAngle("arcmin", AngleUnit::ArcMinute);
}

TEST(GeodesyUnits, FootNeverMeansUsSurveyFoot)
{
    // "ft" is the international foot, as in EPSG and PROJ. Silently reading it
    // as the survey foot (or vice versa) is the 2 ppm blunder tested above.
    EXPECT_EQ(*parseLengthUnit("ft"), LengthUnit::InternationalFoot);
    EXPECT_EQ(*parseLengthUnit("feet"), LengthUnit::InternationalFoot);
    EXPECT_NE(*parseLengthUnit("ftUS"), *parseLengthUnit("ft"));
}

TEST(GeodesyUnits, UnknownUnitIsAnErrorNeverASilentFactorOfOne)
{
    for (const std::string_view text : {"", "   ", "furlong", "metres per second", "f t x", "1"}) {
        const auto length = parseLengthUnit(text);
        ASSERT_FALSE(length.ok()) << "'" << text << "' must not parse";
        EXPECT_EQ(length.error().code, ErrorCode::ParseFailure);
        EXPECT_NE(length.error().context.find(std::string(text)), std::string::npos);

        const auto angle = parseAngleUnit(text);
        ASSERT_FALSE(angle.ok()) << "'" << text << "' must not parse";
        EXPECT_EQ(angle.error().code, ErrorCode::ParseFailure);
    }
    // A length name is not an angle and vice versa.
    EXPECT_FALSE(parseAngleUnit("m").ok());
    EXPECT_FALSE(parseLengthUnit("deg").ok());
}

TEST(GeodesyUnits, NamesAndAbbreviationsParseBackToTheSameUnit)
{
    for (const LengthUnit unit : kAllLengthUnits) {
        const auto fromName = parseLengthUnit(toString(unit));
        ASSERT_TRUE(fromName.ok()) << toString(unit);
        EXPECT_EQ(*fromName, unit);
        const auto fromSymbol = parseLengthUnit(abbreviation(unit));
        ASSERT_TRUE(fromSymbol.ok()) << abbreviation(unit);
        EXPECT_EQ(*fromSymbol, unit);
    }
    for (const AngleUnit unit : kAllAngleUnits) {
        const auto fromName = parseAngleUnit(toString(unit));
        ASSERT_TRUE(fromName.ok()) << toString(unit);
        EXPECT_EQ(*fromName, unit);
        const auto fromSymbol = parseAngleUnit(abbreviation(unit));
        ASSERT_TRUE(fromSymbol.ok()) << abbreviation(unit);
        EXPECT_EQ(*fromSymbol, unit);
    }
}

TEST(GeodesyUnits, FactorLookupSeparatesTheTwoFeet)
{
    EXPECT_EQ(lengthUnitFromMetresPerUnit(0.3048), LengthUnit::InternationalFoot);
    // The 15-digit value stored in the EPSG registry for the US survey foot.
    EXPECT_EQ(lengthUnitFromMetresPerUnit(0.304800609601219), LengthUnit::UsSurveyFoot);
    EXPECT_EQ(lengthUnitFromMetresPerUnit(1.0), LengthUnit::Metre);
    EXPECT_FALSE(lengthUnitFromMetresPerUnit(0.3047972654).has_value()); // Clarke's foot
    EXPECT_FALSE(lengthUnitFromMetresPerUnit(std::numeric_limits<double>::quiet_NaN()).has_value());
}
