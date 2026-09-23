#include <gtest/gtest.h>

#include "katana/math/unit_ratio.hpp"

namespace units = katana::math::units;
using katana::math::fromMetres;
using katana::math::ratioValue;
using katana::math::toMetres;

TEST(UnitRatio, TheDefinitionsAreTheStatutoryOnes)
{
    // 1959 international yard agreement: 1 yd = 0.9144 m, 1 ft = yd / 3,
    // 1 in = ft / 12. Mendenhall order 1893: 1 m = 39.37 US inches, so
    // 1 US ft = 12 / 39.37 = 1200/3937 m. A chain is 66 ft and a link a
    // hundredth of one; a mile is 5280 ft.
    EXPECT_EQ(units::kInternationalFoot.numerator * 3 * 1250,
              units::kInternationalYard.numerator * 1250);
    EXPECT_DOUBLE_EQ(ratioValue(units::kInternationalFoot), 0.3048);
    EXPECT_DOUBLE_EQ(ratioValue(units::kInternationalInch), 0.0254);
    EXPECT_DOUBLE_EQ(ratioValue(units::kInternationalYard), 0.9144);
    EXPECT_DOUBLE_EQ(ratioValue(units::kInternationalChain), 66 * 0.3048);
    EXPECT_DOUBLE_EQ(ratioValue(units::kInternationalLink), 0.201168);
    EXPECT_DOUBLE_EQ(ratioValue(units::kInternationalMile), 1609.344);
    EXPECT_DOUBLE_EQ(ratioValue(units::kUsSurveyFoot), 12.0 / 39.37);
    EXPECT_DOUBLE_EQ(ratioValue(units::kUsSurveyChain), 66 * 12.0 / 39.37);
    EXPECT_DOUBLE_EQ(ratioValue(units::kUsSurveyMile), 5280 * 12.0 / 39.37);
    EXPECT_EQ(ratioValue(units::kMetre), 1.0);
}

TEST(UnitRatio, AConversionIsExactWhereTheArithmeticAllowsIt)
{
    // 1000 ft = 304.8 m: 1000 * 381 = 381000 is exact and /1250 is one
    // correctly rounded division, so the result is the double nearest 304.8.
    EXPECT_EQ(toMetres(1000.0, units::kInternationalFoot), 304.8);
    // 3937 US ft is exactly 1200 m, and the rational form gets it exactly where
    // multiplying by a rounded 0.3048006096 would not.
    EXPECT_EQ(toMetres(3937.0, units::kUsSurveyFoot), 1200.0);
    EXPECT_EQ(fromMetres(1200.0, units::kUsSurveyFoot), 3937.0);
    // The two feet are 2 ppm apart: at 2 000 000 ft that is 4 ft (about 1.2 m).
    const double international = toMetres(2.0e6, units::kInternationalFoot);
    const double survey = toMetres(2.0e6, units::kUsSurveyFoot);
    EXPECT_NEAR(survey - international, 4.0 * 0.3048, 1e-3);
}
