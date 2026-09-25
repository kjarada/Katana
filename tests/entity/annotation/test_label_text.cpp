// The label template language and its number formats (docs/annotation.md,
// "Templates").
//
// Every expected string is worked out by hand from the stated rules: a
// bearing of atan2(3, 4) is 36.8698976...deg = 36 52' 11.63", a 10 m by 12.5 m
// lot is 125 m2 = 0.0125 ha. A label is a number somebody signs.

#include <gtest/gtest.h>

#include <cmath>
#include <string>

#include "katana/entity/label_text.hpp"
#include "katana/math/numerics.hpp"

using katana::entity::checkLabelTemplate;
using katana::entity::formatChainage;
using katana::entity::formatDm;
using katana::entity::formatDms;
using katana::entity::formatFixed;
using katana::entity::formatLabel;
using katana::entity::formatQuadrantBearing;
using katana::entity::LabelKind;
using katana::entity::LabelQuantity;
using katana::entity::LabelValue;
using katana::entity::LabelValues;
using katana::math::kDegToRad;

namespace {

double dms(double degrees, double minutes, double seconds)
{
    return (degrees + minutes / 60.0 + seconds / 3600.0) * kDegToRad;
}

LabelValues segmentValues()
{
    LabelValues values;
    values["bearing"] = LabelValue::of(LabelQuantity::Bearing, std::atan2(3.0, 4.0));
    values["distance"] = LabelValue::of(LabelQuantity::Length, 5.0);
    values["code"] = LabelValue::ofText("bdy");
    return values;
}

} // namespace

TEST(LabelText, BearingAndDistanceInTheSurveyForm)
{
    // Bearing of the 3-4-5 line from north: atan(3/4) = 36.869897646 deg
    // = 36 deg 52' 11.6315" -> whole seconds 12.
    EXPECT_EQ(formatLabel("{bearing:dms} {distance:.3f}", segmentValues()),
              "36°52'12\" 5.000");
    EXPECT_EQ(formatLabel("{bearing:dms.2}", segmentValues()), "36°52'11.63\"");
    EXPECT_EQ(formatLabel("{bearing:dm}", segmentValues()), "36°52'");
    // With no format step a bearing is whole-second DMS and a length 3 decimals.
    EXPECT_EQ(formatLabel("{bearing} {distance}", segmentValues()), "36°52'12\" 5.000");
    EXPECT_EQ(formatLabel("{bearing:qb}", segmentValues()), "N 36°52'12\" E");
    EXPECT_EQ(formatLabel("{bearing:deg:.4f}", segmentValues()), "36.8699");
}

TEST(LabelText, UnitStepsConvertBeforeTheNumberFormat)
{
    LabelValues values;
    values["area"] = LabelValue::of(LabelQuantity::Area, 125.0);
    values["distance"] = LabelValue::of(LabelQuantity::Length, 1.2345);
    EXPECT_EQ(formatLabel("{area:m2:.1f} m2", values), "125.0 m2");
    EXPECT_EQ(formatLabel("{area:ha:.4f} ha", values), "0.0125 ha");
    // 125 / 4046.8564224 = 0.030888...
    EXPECT_EQ(formatLabel("{area:ac:.3f}", values), "0.031");
    EXPECT_EQ(formatLabel("{distance:mm:.0f} mm", values), "1235 mm")
        << "1234.5 rounds half away from zero";
    EXPECT_EQ(formatLabel("{distance:ft:.2f}", values), "4.05"); // 1.2345 / 0.3048 = 4.0502
    EXPECT_EQ(formatLabel("{area}", values), "125.0") << "an area's own format is 1 decimal";
}

TEST(LabelText, ALineWithAnAbsentValueIsDroppedNotPrintedAsZero)
{
    LabelValues values;
    values["point"] = LabelValue::ofText("1001");
    // No "z": the point has no level.
    EXPECT_EQ(formatLabel("{point}\nRL {z:.3f}", values), "1001");
    EXPECT_EQ(formatLabel("RL {z:.3f}", values), "") << "every line dropped: no label at all";
    values["z"] = LabelValue::of(LabelQuantity::Number, 31.2495);
    EXPECT_EQ(formatLabel("{point}\nRL {z:.3f}", values), "1001\nRL 31.250");
}

TEST(LabelText, BracesAreEscapedByDoubling)
{
    EXPECT_EQ(formatLabel("{{{code}}}", segmentValues()), "{bdy}");
    EXPECT_EQ(formatLabel("{code:upper}", segmentValues()), "BDY");
}

TEST(LabelText, TemplatesAreCheckedForTheKindTheyLabel)
{
    EXPECT_TRUE(checkLabelTemplate("{bearing:dms} {distance:.3f}", LabelKind::Segment).ok());
    EXPECT_TRUE(checkLabelTemplate("{area:m2:.1f} m2\n{area:ha:.4f} ha", LabelKind::Area).ok());
    EXPECT_TRUE(checkLabelTemplate("RL {z:.3f}", LabelKind::Point).ok());
    EXPECT_TRUE(checkLabelTemplate("{chainage:ch}", LabelKind::Chainage).ok());
    EXPECT_TRUE(checkLabelTemplate("{prop.owner:upper}", LabelKind::Area).ok());

    EXPECT_FALSE(checkLabelTemplate("{area}", LabelKind::Segment).ok())
        << "a segment has no area";
    EXPECT_FALSE(checkLabelTemplate("{distance:dms}", LabelKind::Segment).ok())
        << "dms applies to an angle, not a length";
    EXPECT_FALSE(checkLabelTemplate("{bearing:.3f}", LabelKind::Segment).ok())
        << "an angle needs deg, rad or gon before a number format";
    EXPECT_FALSE(checkLabelTemplate("{distance:.3f:mm}", LabelKind::Segment).ok())
        << "the number format is the last step";
    EXPECT_FALSE(checkLabelTemplate("{distance", LabelKind::Segment).ok());
    EXPECT_FALSE(checkLabelTemplate("distance}", LabelKind::Segment).ok());
    EXPECT_FALSE(checkLabelTemplate("{}", LabelKind::Point).ok());
}

TEST(LabelText, DmsRoundsOnceAndCarries)
{
    // 59.9996" at whole seconds is a carry into the minutes, never 60".
    EXPECT_EQ(formatDms(dms(10, 59, 59.9996), 0), "11°00'00\"");
    EXPECT_EQ(formatDms(dms(0, 0, 0.5), 0), "0°00'01\"") << "half away from zero";
    EXPECT_EQ(formatDms(dms(359, 59, 59.96), 1), "360°00'00.0\"")
        << "an angle is not wrapped; a bearing is (below)";
    EXPECT_EQ(formatDms(-dms(12, 30, 0), 0), "-12°30'00\"");
    EXPECT_EQ(formatDm(dms(12, 30, 30)), "12°31'");
}

TEST(LabelText, QuadrantBearingsInEachQuadrant)
{
    EXPECT_EQ(formatQuadrantBearing(dms(45, 0, 0), 0), "N 45°00'00\" E");
    EXPECT_EQ(formatQuadrantBearing(dms(135, 0, 0), 0), "S 45°00'00\" E");
    EXPECT_EQ(formatQuadrantBearing(dms(200, 30, 0), 0), "S 20°30'00\" W");
    EXPECT_EQ(formatQuadrantBearing(dms(315, 0, 0), 0), "N 45°00'00\" W");
}

TEST(LabelText, ChainageSplitsKilometresAfterRounding)
{
    EXPECT_EQ(formatChainage(1234.5, 3), "1+234.500");
    EXPECT_EQ(formatChainage(20.0, 3), "0+020.000");
    EXPECT_EQ(formatChainage(999.9996, 3), "1+000.000") << "rounded first, then split";
    EXPECT_EQ(formatChainage(-12.0, 3), "-0+012.000");
    EXPECT_EQ(formatChainage(12345.0, 1), "12+345.0");
}

TEST(LabelText, FixedNeverPrintsMinusZero)
{
    EXPECT_EQ(formatFixed(-0.0004, 3), "0.000");
    EXPECT_EQ(formatFixed(-0.0006, 3), "-0.001");
    EXPECT_EQ(formatFixed(2.5, 0), "3") << "ties away from zero";
}

TEST(LabelText, TheKindsListTheirValues)
{
    const auto segment = katana::entity::labelValueNames(LabelKind::Segment);
    EXPECT_NE(std::find(segment.begin(), segment.end(), "bearing"), segment.end());
    const auto area = katana::entity::labelValueNames(LabelKind::Area);
    EXPECT_NE(std::find(area.begin(), area.end(), "area"), area.end());
    EXPECT_EQ(std::find(area.begin(), area.end(), "bearing"), area.end());
}
