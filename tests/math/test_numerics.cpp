#include <gtest/gtest.h>

#include <limits>

#include "katana/math/numerics.hpp"

using namespace katana::math;

TEST(MathNumerics, TolerancesAreOrderedFromTightestToLoosest)
{
    EXPECT_LT(tolerance::kAbsolute, tolerance::kRelative);
    EXPECT_LT(tolerance::kGeometric, tolerance::kCoordinate);
    // The geometric tolerance must stay above the spacing of doubles at the
    // largest projected coordinates (a UTM northing of 1e7), or it is meaningless.
    const double ulpAtNorthing = std::nextafter(1.0e7, 2.0e7) - 1.0e7;
    EXPECT_GT(tolerance::kGeometric, 10.0 * ulpAtNorthing);
}

TEST(MathNumerics, NearlyEqualIsRelativeForLargeMagnitudes)
{
    EXPECT_TRUE(nearlyEqual(1.0, 1.0 + 1e-10));
    EXPECT_FALSE(nearlyEqual(1.0, 1.0 + 1e-8));
    // 1 mm apart at a UTM northing: equal relatively, but not geometrically.
    EXPECT_TRUE(nearlyEqual(5.0e6, 5.0e6 + 1e-3));
    EXPECT_FALSE(lengthsEqual(5.0e6, 5.0e6 + 1e-3));
}

TEST(MathNumerics, NearlyEqualIsAbsoluteNearZero)
{
    EXPECT_TRUE(nearlyEqual(0.0, 1e-10));
    EXPECT_FALSE(nearlyEqual(0.0, 1e-8));
    EXPECT_TRUE(nearlyZero(1e-13));
    EXPECT_FALSE(nearlyZero(1e-11));
}

TEST(MathNumerics, NearlyEqualHandlesNonFiniteValues)
{
    const double inf = std::numeric_limits<double>::infinity();
    const double nan = std::numeric_limits<double>::quiet_NaN();
    EXPECT_TRUE(nearlyEqual(inf, inf));
    EXPECT_FALSE(nearlyEqual(inf, -inf));
    EXPECT_FALSE(nearlyEqual(inf, 1e300));
    EXPECT_FALSE(nearlyEqual(nan, nan));
    EXPECT_FALSE(nearlyEqual(nan, 0.0));
}

TEST(MathNumerics, NormalizeAngleWrapsIntoHalfOpenRange)
{
    EXPECT_DOUBLE_EQ(normalizeAngle(0.0), 0.0);
    EXPECT_DOUBLE_EQ(normalizeAngle(kTwoPi), 0.0);
    EXPECT_NEAR(normalizeAngle(-kHalfPi), 1.5 * kPi, 1e-15);
    EXPECT_NEAR(normalizeAngle(5.0 * kPi), kPi, 1e-14);
    // A tiny negative angle must not round up to exactly 2*pi.
    EXPECT_LT(normalizeAngle(-1e-20), kTwoPi);
    EXPECT_GE(normalizeAngle(-1e-20), 0.0);
}

TEST(MathNumerics, NormalizeAngleSignedWrapsIntoPlusMinusPi)
{
    EXPECT_NEAR(normalizeAngleSigned(1.5 * kPi), -kHalfPi, 1e-15);
    EXPECT_NEAR(normalizeAngleSigned(-1.5 * kPi), kHalfPi, 1e-15);
    EXPECT_DOUBLE_EQ(normalizeAngleSigned(kPi), kPi);
}

TEST(MathNumerics, AnglesEqualComparesAcrossTheWrap)
{
    EXPECT_TRUE(anglesEqual(0.0, kTwoPi));
    EXPECT_TRUE(anglesEqual(-kPi, kPi));
    EXPECT_TRUE(anglesEqual(1e-11, kTwoPi - 1e-11));
    EXPECT_FALSE(anglesEqual(0.0, 1e-6));
}

TEST(MathNumerics, ClampAndLerp)
{
    EXPECT_DOUBLE_EQ(clamp(5.0, 0.0, 1.0), 1.0);
    EXPECT_DOUBLE_EQ(clamp(-5.0, 0.0, 1.0), 0.0);
    EXPECT_DOUBLE_EQ(clamp(0.25, 0.0, 1.0), 0.25);
    EXPECT_DOUBLE_EQ(lerp(10.0, 20.0, 0.0), 10.0);
    EXPECT_DOUBLE_EQ(lerp(10.0, 20.0, 1.0), 20.0);
    EXPECT_DOUBLE_EQ(lerp(10.0, 20.0, 0.5), 15.0);
}

TEST(MathNumerics, DegreeRadianConstantsAreConsistent)
{
    EXPECT_DOUBLE_EQ(180.0 * kDegToRad, kPi);
    EXPECT_DOUBLE_EQ(kPi * kRadToDeg, 180.0);
}
