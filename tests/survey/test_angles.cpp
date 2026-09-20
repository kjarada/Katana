#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <string>

#include "katana/survey/angles.hpp"
#include "support/property.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;
using katana::math::kDegToRad;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::test::kPropertyIterations;
using katana::test::Random;

namespace {

// Expected values are formed in decimal degrees, a different route from the
// implementation's integer arc-second arithmetic.
constexpr double kDms453015 = (45.0 + 30.0 / 60.0 + 15.0 / 3600.0) * kDegToRad;

} // namespace

// ---- units -----------------------------------------------------------------

TEST(SurveyAngles, UnitConversions)
{
    EXPECT_DOUBLE_EQ(degreesToRadians(180.0), kPi);
    EXPECT_DOUBLE_EQ(radiansToDegrees(kHalfPi), 90.0);
    EXPECT_DOUBLE_EQ(gonToRadians(200.0), kPi); // 400 gon in a full turn
    EXPECT_DOUBLE_EQ(gonToRadians(100.0), kHalfPi);
    EXPECT_DOUBLE_EQ(radiansToGon(kTwoPi), 400.0);
    // 1 degree = 3600 arc-seconds.
    EXPECT_NEAR(arcSecondsToRadians(3600.0), kDegToRad, 1e-17);
    EXPECT_NEAR(radiansToArcSeconds(kDegToRad), 3600.0, 1e-9);
}

TEST(SurveyAngles, NormalizeAzimuthWrapsIntoFullCircle)
{
    EXPECT_DOUBLE_EQ(normalizeAzimuth(0.0), 0.0);
    EXPECT_DOUBLE_EQ(normalizeAzimuth(kTwoPi), 0.0);
    EXPECT_NEAR(normalizeAzimuth(-kHalfPi), 1.5 * kPi, 1e-15);
    EXPECT_NEAR(normalizeAzimuth(5.0 * kPi), kPi, 1e-14);
    EXPECT_LT(normalizeAzimuth(-1e-20), kTwoPi); // never returns 2*pi itself
}

// ---- DMS parsing -----------------------------------------------------------

TEST(SurveyAngles, ParseDmsAcceptsCommonNotations)
{
    for (const char* text :
         {"45\xC2\xB0"
          "30'15\"",
          "45 30 15", "45-30-15", "45:30:15", "45d30m15s", "45D30M15S", "  45\xC2\xB0 30' 15\"  ",
          "+45 30 15", "45\xC2\xB0"
                       "30'15''",
          "45\xC2\xBA"
          "30\xE2\x80\xB2"
          "15\xE2\x80\xB3"}) {
        const auto parsed = parseDms(text);
        ASSERT_TRUE(parsed.ok()) << text;
        EXPECT_NEAR(*parsed, kDms453015, 1e-15) << text;
    }
}

TEST(SurveyAngles, ParseDmsFractionOnLastFieldOnly)
{
    EXPECT_NEAR(*parseDms("45.5"), 45.5 * kDegToRad, 1e-15);
    EXPECT_NEAR(*parseDms("45.5\xC2\xB0"), 45.5 * kDegToRad, 1e-15);
    // 30.25' = 30'15"
    EXPECT_NEAR(*parseDms("45\xC2\xB0"
                          "30.25'"),
                kDms453015, 1e-15);
    EXPECT_NEAR(*parseDms("45 30 15.5"), (45.0 + 30.0 / 60.0 + 15.5 / 3600.0) * kDegToRad, 1e-15);

    EXPECT_FALSE(parseDms("45.5 30").ok());
    EXPECT_FALSE(parseDms("45 30.5 10").ok());
}

TEST(SurveyAngles, ParseDmsSignAppliesToWholeAngle)
{
    // -0°30' is -0.5°, which a signed degrees field could not express.
    EXPECT_NEAR(*parseDms("-0\xC2\xB0"
                          "30'00\""),
                -0.5 * kDegToRad, 1e-16);
    EXPECT_NEAR(*parseDms("-12 05 07"), -(12.0 + 5.0 / 60.0 + 7.0 / 3600.0) * kDegToRad, 1e-15);
}

TEST(SurveyAngles, ParseDmsRejectsMalformedText)
{
    for (const char* text : {"", "   ", "abc", "45\xC2\xB0"
                                               "60'00\"",
                             "45\xC2\xB0"
                             "30'60\"",
                             "45 30 15 10", "45\xC2\xB0"
                                            "30'15\"x",
                             "1e5", "nan", "inf", "--45", "45'", "12 \" 30"}) {
        const auto parsed = parseDms(text);
        ASSERT_FALSE(parsed.ok()) << "'" << text << "'";
        EXPECT_EQ(parsed.error().code, ErrorCode::ParseFailure) << text;
    }
}

// ---- DMS formatting --------------------------------------------------------

TEST(SurveyAngles, FormatDms)
{
    EXPECT_EQ(*formatDms(kDms453015, 2), "45\xC2\xB0"
                                         "30'15.00\"");
    EXPECT_EQ(*formatDms(kDms453015, 0), "45\xC2\xB0"
                                         "30'15\"");
    EXPECT_EQ(*formatDms(0.0, 1), "0\xC2\xB0"
                                  "00'00.0\"");
    EXPECT_EQ(*formatDms(-0.5 * kDegToRad, 2), "-0\xC2\xB0"
                                               "30'00.00\"");
    EXPECT_EQ(*formatDms(kTwoPi, 0), "360\xC2\xB0"
                                     "00'00\"");
}

TEST(SurveyAngles, FormatDmsCarriesRoundedSecondsIntoMinutesAndDegrees)
{
    // 10°59'59.9996": rounds up to 11° at 2 and 3 decimals, stays at 4.
    const double angle = (10.0 + 59.0 / 60.0 + 59.9996 / 3600.0) * kDegToRad;
    EXPECT_EQ(*formatDms(angle, 2), "11\xC2\xB0"
                                    "00'00.00\"");
    EXPECT_EQ(*formatDms(angle, 3), "11\xC2\xB0"
                                    "00'00.000\"");
    EXPECT_EQ(*formatDms(angle, 4), "10\xC2\xB0"
                                    "59'59.9996\"");

    const auto dms = radiansToDms(angle, 2);
    ASSERT_TRUE(dms.ok());
    EXPECT_EQ(dms->degrees, 11u);
    EXPECT_EQ(dms->minutes, 0u);
    EXPECT_DOUBLE_EQ(dms->seconds, 0.0);
}

TEST(SurveyAngles, FormatDmsNeverPrintsNegativeZero)
{
    EXPECT_EQ(*formatDms(-1e-12, 2), "0\xC2\xB0"
                                     "00'00.00\"");
}

TEST(SurveyAngles, FormatDmsRejectsBadArguments)
{
    EXPECT_EQ(formatDms(std::numeric_limits<double>::quiet_NaN(), 2).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(formatDms(std::numeric_limits<double>::infinity(), 2).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(formatDms(1.0, -1).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(formatDms(1.0, 10).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(formatDms(1e12, 9).error().code, ErrorCode::InvalidArgument); // not resolvable
}

TEST(SurveyAngles, DmsStructRoundTrip)
{
    const DmsAngle dms{true, 12, 5, 7.25};
    const auto radians = dmsToRadians(dms);
    ASSERT_TRUE(radians.ok());
    EXPECT_NEAR(*radians, -(12.0 + 5.0 / 60.0 + 7.25 / 3600.0) * kDegToRad, 1e-15);
    EXPECT_EQ(*radiansToDms(*radians, 2), dms);

    EXPECT_FALSE(dmsToRadians(DmsAngle{false, 1, 60, 0.0}).ok());
    EXPECT_FALSE(dmsToRadians(DmsAngle{false, 1, 0, 60.0}).ok());
    EXPECT_FALSE(dmsToRadians(DmsAngle{false, 1, 0, -1.0}).ok());
}

TEST(SurveyAngles, PropertyFormatThenParseRoundTrips)
{
    Random random;
    // Formatting with 6 decimals rounds to 1e-6", so the round trip is exact to
    // half of that: 0.5e-6" = 2.4e-12 rad.
    const double tolerance = 0.5e-6 * kArcSecondToRad * 1.0001;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double angle = random.real(-kTwoPi, kTwoPi);
        const auto text = formatDms(angle, 6);
        ASSERT_TRUE(text.ok());
        const auto parsed = parseDms(*text);
        ASSERT_TRUE(parsed.ok()) << *text;
        EXPECT_NEAR(*parsed, angle, tolerance) << *text;
    }
}

// ---- bearings --------------------------------------------------------------

TEST(SurveyAngles, AzimuthToBearingByQuadrant)
{
    // NE: bearing = azimuth; SE: 180 - az; SW: az - 180; NW: 360 - az.
    const auto ne = *azimuthToBearing(30.0 * kDegToRad);
    EXPECT_EQ(ne.meridian, BearingMeridian::North);
    EXPECT_EQ(ne.side, BearingSide::East);
    EXPECT_NEAR(ne.angle, 30.0 * kDegToRad, 1e-15);

    const auto se = *azimuthToBearing(135.0 * kDegToRad);
    EXPECT_EQ(se.meridian, BearingMeridian::South);
    EXPECT_EQ(se.side, BearingSide::East);
    EXPECT_NEAR(se.angle, 45.0 * kDegToRad, 1e-15);

    const auto sw = *azimuthToBearing(192.5 * kDegToRad);
    EXPECT_EQ(sw.meridian, BearingMeridian::South);
    EXPECT_EQ(sw.side, BearingSide::West);
    EXPECT_NEAR(sw.angle, 12.5 * kDegToRad, 1e-15);

    const auto nw = *azimuthToBearing(315.0 * kDegToRad);
    EXPECT_EQ(nw.meridian, BearingMeridian::North);
    EXPECT_EQ(nw.side, BearingSide::West);
    EXPECT_NEAR(nw.angle, 45.0 * kDegToRad, 1e-15);

    EXPECT_FALSE(azimuthToBearing(std::numeric_limits<double>::quiet_NaN()).ok());
}

TEST(SurveyAngles, BearingOnTheAxes)
{
    EXPECT_EQ(*formatBearing(0.0), "N 0\xC2\xB0"
                                   "00'00\" E");
    EXPECT_EQ(*formatBearing(kHalfPi), "N 90\xC2\xB0"
                                       "00'00\" E");
    EXPECT_EQ(*formatBearing(kPi), "S 0\xC2\xB0"
                                   "00'00\" E");
    EXPECT_EQ(*formatBearing(1.5 * kPi), "S 90\xC2\xB0"
                                         "00'00\" W");
    // N 0 W is due north: azimuth 0, not 2*pi.
    EXPECT_DOUBLE_EQ(*bearingToAzimuth({BearingMeridian::North, 0.0, BearingSide::West}), 0.0);
}

TEST(SurveyAngles, FormatAndParseBearing)
{
    EXPECT_EQ(*formatBearing(kDms453015), "N 45\xC2\xB0"
                                          "30'15\" E");
    EXPECT_EQ(*formatBearing(135.0 * kDegToRad), "S 45\xC2\xB0"
                                                 "00'00\" E");
    EXPECT_EQ(*formatBearing(225.0 * kDegToRad), "S 45\xC2\xB0"
                                                 "00'00\" W");
    EXPECT_EQ(*formatBearing(315.0 * kDegToRad, 1), "N 45\xC2\xB0"
                                                    "00'00.0\" W");

    EXPECT_NEAR(*parseBearing("N 45\xC2\xB0"
                              "30'15\" E"),
                kDms453015, 1e-15);
    EXPECT_NEAR(*parseBearing("S 12\xC2\xB0"
                              "30'00\" W"),
                192.5 * kDegToRad, 1e-15);
    EXPECT_NEAR(*parseBearing("s45-00-00e"), 135.0 * kDegToRad, 1e-15);
    EXPECT_NEAR(*parseBearing("N 10.5 W"), 349.5 * kDegToRad, 1e-15);
    EXPECT_NEAR(*parseBearing("N 90 E"), kHalfPi, 1e-15); // exactly 90 is allowed
}

TEST(SurveyAngles, ParseBearingRejectsMalformedText)
{
    for (const char* text : {"", "N", "NE", "N 91 E", "X 10 E", "N 10 Q", "N -10 E", "N E",
                             "45 30 15", "N 45 60 00 E"}) {
        const auto parsed = parseBearing(text);
        ASSERT_FALSE(parsed.ok()) << "'" << text << "'";
        EXPECT_EQ(parsed.error().code, ErrorCode::ParseFailure) << text;
    }
    EXPECT_FALSE(bearingToAzimuth({BearingMeridian::North, 2.0, BearingSide::East}).ok());
    EXPECT_FALSE(bearingToAzimuth({BearingMeridian::North, -0.1, BearingSide::East}).ok());
}

TEST(SurveyAngles, PropertyBearingRoundTrip)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const double azimuth = random.real(0.0, kTwoPi);
        const auto bearing = azimuthToBearing(azimuth);
        ASSERT_TRUE(bearing.ok());
        EXPECT_GE(bearing->angle, 0.0);
        EXPECT_LE(bearing->angle, kHalfPi);
        const auto back = bearingToAzimuth(*bearing);
        ASSERT_TRUE(back.ok());
        // One subtraction from pi or 2*pi each way: a few ulp of 2*pi.
        EXPECT_NEAR(*back, azimuth, 4e-15);
    }
}
