// The Inquiry tools (src/katana_cad/tools/inquiry.cpp): Distance, Area, ID
// Point, Angle and List, driven as the plan view drives them. They change
// nothing, so each test checks the report - the command log's line - and that
// no command ran. The numbers are worked out by hand in the comments.
//
// The 3-4-5 triangle carries most of them, because its angles are known to
// the second: atan(3/4) = 36.869897645844 degrees; 0.869897645844 * 60 =
// 52.19385875' and 0.19385875 * 60 = 11.63" - so a 3 east, 4 north leg runs
// at 36°52'11.63", and its complement is 90° - 36°52'11.63" = 53°07'48.37".

#include <cmath>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/survey_import.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/math/numerics.hpp"
#include "tool_driver.hpp"

namespace {

namespace cmd = katana::commands;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kPi;

constexpr auto kContinue = ToolStep::Outcome::Continue;
constexpr auto kDone = ToolStep::Outcome::Done;
constexpr auto kRejected = ToolStep::Outcome::Rejected;

// The degree sign as the survey formatter writes it (UTF-8).
const std::string kDeg = "\xC2\xB0";
// Square metres, likewise.
const std::string kSquare = " m\xC2\xB2";

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// A survey point as the survey import writes one: its number and its height.
EntityId addSurveyPoint(ToolDriver& driver, Point2 at, const std::string& name,
                        std::optional<double> height, const std::string& code = {})
{
    Entity point;
    point.geometry = katana::entity::PointGeometry{at};
    const katana::cad::SurveyImportOptions options;
    point.properties[options.pointNumberProperty] = name;
    if (!code.empty()) {
        point.properties[options.codeProperty] = code;
    }
    if (height) {
        katana::entity::setHeights(point.properties, {height});
    }
    return driver.add(cmd::createEntities({point}));
}

// ---- Distance ----------------------------------------------------------------------

TEST(InquiryDistance, TwoPicksReportTheInverseOfA345LegInSurveyTerms)
{
    ToolDriver driver;
    driver.start("inquiry.distance");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_EQ(driver.click(1000, 2000).outcome, kContinue);
    const ToolStep step = driver.click(1003, 2004);
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.command, nullptr);
    const std::string& report = step.message;
    // dE = 3, dN = 4, the distance sqrt(9 + 16) = 5.
    EXPECT_TRUE(contains(report, "dE +3.000   dN +4.000")) << report;
    EXPECT_TRUE(contains(report, "Horizontal distance 5.000")) << report;
    EXPECT_TRUE(contains(report, "Azimuth 36" + kDeg + "52'11.63\"")) << report;
    // Back: 36°52'11.63" + 180°.
    EXPECT_TRUE(contains(report, "back azimuth 216" + kDeg + "52'11.63\"")) << report;
    EXPECT_TRUE(contains(report, "Bearing N 36" + kDeg + "52'11.63\" E")) << report;
    // Two bare picks have no heights, and the report says so rather than
    // reporting a height difference of zero.
    EXPECT_TRUE(contains(report, "No height difference")) << report;
    EXPECT_EQ(driver.executed(), 0);
    // It starts again for the next measurement.
    EXPECT_FALSE(driver.finished());
}

TEST(InquiryDistance, PicksOnSurveyPointsReportTheirNumbersAndHeightDifference)
{
    ToolDriver driver;
    const EntityId a = addSurveyPoint(driver, {1000, 2000}, "CP1", 10.0);
    const EntityId b = addSurveyPoint(driver, {1003, 2004}, "CP2", 12.0);
    driver.start("inquiry.distance");
    (void)driver.click(1000, 2000);
    // A pick 0.004 off the point (within the driver's 0.01 aperture) is the
    // point: its number and height, not the bare coordinates.
    const ToolStep step = driver.click(1003.004, 2004);
    ASSERT_EQ(step.outcome, kDone);
    const std::string& report = step.message;
    EXPECT_TRUE(contains(report, "point " + std::to_string(a) + " (CP1)")) << report;
    EXPECT_TRUE(contains(report, "point " + std::to_string(b) + " (CP2)")) << report;
    // dZ = 12 - 10 = +2; slope sqrt(5^2 + 2^2) = sqrt(29) = 5.38516...;
    // grade 2 / 5 = 40 %.
    EXPECT_TRUE(contains(report, "Height difference +2.000")) << report;
    EXPECT_TRUE(contains(report, "slope distance 5.385")) << report;
    EXPECT_TRUE(contains(report, "grade +40.000 %")) << report;
}

TEST(InquiryDistance, TheSamePointTwiceIsRefusedAndUndoTakesBackTheFirst)
{
    ToolDriver driver;
    driver.start("inquiry.distance");
    (void)driver.click(5, 5);
    EXPECT_EQ(driver.click(5, 5).outcome, kRejected);
    EXPECT_EQ(driver.undo().outcome, kContinue);
    EXPECT_EQ(driver.tool().prompt(), "Specify first point");
    EXPECT_EQ(driver.undo().outcome, kRejected);
}

// ---- Area --------------------------------------------------------------------------

TEST(InquiryArea, FourPickedCornersReportAreaPerimeterAndHectares)
{
    ToolDriver driver;
    driver.start("inquiry.area");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    EXPECT_EQ(driver.enter().outcome, kRejected); // two corners bound nothing
    (void)driver.click(4, 3);
    (void)driver.click(0, 3);
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.command, nullptr);
    // 4 x 3 = 12 m^2 = 0.0012 ha; perimeter 4 + 3 + 4 + 3 = 14.
    EXPECT_TRUE(contains(step.message, "area 12.000" + kSquare + " (0.0012 ha)")) << step.message;
    EXPECT_TRUE(contains(step.message, "perimeter 14.000 m")) << step.message;
    EXPECT_TRUE(contains(step.message, "4 picked corners")) << step.message;
}

TEST(InquiryArea, ObjectMeasuresAClosedCircleAndRefusesAnOpenPolyline)
{
    ToolDriver driver;
    const EntityId circle = driver.add(cmd::createCircle({10, 10}, 2.0));
    const EntityId open = driver.add(cmd::createPolyline(Polyline2{{{0, 0}, {5, 0}, {5, 5}}, false}));
    driver.start("inquiry.area");
    EXPECT_EQ(driver.type("O").outcome, kContinue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.pick(open, 5, 2).outcome, kRejected);
    const ToolStep step = driver.pick(circle, 12, 10);
    ASSERT_EQ(step.outcome, kDone);
    // pi * 2^2 = 12.566370... m^2 = 0.0012566 ha -> 0.0013; circumference
    // 2 * pi * 2 = 12.566370...
    EXPECT_TRUE(contains(step.message, "area 12.566" + kSquare + " (0.0013 ha)")) << step.message;
    EXPECT_TRUE(contains(step.message, "perimeter 12.566 m")) << step.message;
}

TEST(InquiryArea, EnterAtTheFirstCornerMeasuresTheSelectionOnce)
{
    ToolDriver driver;
    const EntityId lot =
        driver.add(cmd::createPolyline(Polyline2{{{0, 0}, {10, 0}, {10, 5}, {0, 5}}, true}));
    driver.document().selection().set({lot});
    driver.start("inquiry.area");
    EXPECT_TRUE(contains(driver.tool().prompt(), "<Selection>"));
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    // 10 x 5 = 50 m^2 = 0.0050 ha; perimeter 30.
    EXPECT_TRUE(contains(step.message, "area 50.000" + kSquare + " (0.0050 ha)")) << step.message;
    EXPECT_TRUE(contains(step.message, "perimeter 30.000 m")) << step.message;
    EXPECT_TRUE(driver.finished()); // not measured again by a restart
}

// ---- ID Point ----------------------------------------------------------------------

TEST(InquiryId, ABarePickReportsEastingAndNorthing)
{
    ToolDriver driver;
    driver.start("inquiry.id");
    const ToolStep step = driver.click(3, 4);
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.message, "E 3.000 N 4.000");
    EXPECT_FALSE(driver.finished());
}

TEST(InquiryId, APickOnASurveyPointReportsItsNumberHeightAndCode)
{
    ToolDriver driver;
    const EntityId id = addSurveyPoint(driver, {100, 200}, "CP1", 10.5, "TB");
    driver.start("inquiry.id");
    const ToolStep step = driver.click(100.005, 200);
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.message,
              "point " + std::to_string(id) + " (CP1)  E 100.000 N 200.000  Z 10.500  code TB");
}

// ---- Angle -------------------------------------------------------------------------

TEST(InquiryAngle, TheAngleFromEastTo345IsReportedBothWaysWithBearings)
{
    ToolDriver driver;
    driver.start("inquiry.angle");
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.click(0, 0).outcome, kRejected); // an arm needs length
    (void)driver.click(10, 0);                        // due east: azimuth 90°
    const ToolStep step = driver.click(3, 4);         // azimuth 36°52'11.63"
    ASSERT_EQ(step.outcome, kDone);
    const std::string& report = step.message;
    // Turned clockwise from 90° to 36°52'11.63": 36°52'11.63" - 90° + 360°
    // = 306°52'11.63"; the other way 53°07'48.37", the included angle.
    EXPECT_TRUE(contains(report, "Included angle 53" + kDeg + "07'48.37\"")) << report;
    EXPECT_TRUE(contains(report, "to the second 306" + kDeg + "52'11.63\"")) << report;
    EXPECT_TRUE(contains(report, "the other way 53" + kDeg + "07'48.37\"")) << report;
    EXPECT_TRUE(contains(report, "First arm  azimuth 90" + kDeg + "00'00.00\"")) << report;
    EXPECT_TRUE(contains(report, "bearing N 36" + kDeg + "52'11.63\" E")) << report;
}

TEST(InquiryAngle, NorthThenEastIsARightAngleTurnedClockwise)
{
    ToolDriver driver;
    driver.start("inquiry.angle");
    (void)driver.click(5, 5);
    (void)driver.click(5, 15);                // north: azimuth 0
    const ToolStep step = driver.click(15, 5); // east: azimuth 90°
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_TRUE(contains(step.message, "Included angle 90" + kDeg + "00'00.00\"")) << step.message;
    EXPECT_TRUE(contains(step.message, "to the second 90" + kDeg + "00'00.00\"")) << step.message;
    EXPECT_TRUE(contains(step.message, "the other way 270" + kDeg + "00'00.00\"")) << step.message;
}

// ---- List --------------------------------------------------------------------------

TEST(InquiryList, ALineIsListedWithItsEndsLengthAndBearing)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {3, 4}));
    driver.document().selection().set({line});
    driver.start("inquiry.list");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection);
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    const std::string& report = step.message;
    EXPECT_TRUE(contains(report, "1 entity listed")) << report;
    EXPECT_TRUE(contains(report, std::to_string(line) +
                                     "  Line  layer 0  style ByLayer  colour ByLayer"))
        << report;
    EXPECT_TRUE(contains(report, "from E 0.000 N 0.000  to E 3.000 N 4.000")) << report;
    EXPECT_TRUE(contains(report, "length 5.000 m   bearing N 36" + kDeg + "52'11.63\" E"))
        << report;
    EXPECT_TRUE(driver.finished());
}

TEST(InquiryList, APolylineArcAndPickedPointAreListedInSurveyTerms)
{
    ToolDriver driver;
    const EntityId lot =
        driver.add(cmd::createPolyline(Polyline2{{{0, 0}, {4, 0}, {4, 3}, {0, 3}}, true}));
    const EntityId arc = driver.add(cmd::createArc(Arc2{{0, 0}, 10.0, 0.0, kPi / 2.0}));
    const EntityId point = addSurveyPoint(driver, {7, 8}, "P9", 1.25, "FC");
    driver.start("inquiry.list");
    EXPECT_EQ(driver.pick(lot, 0, 0).outcome, kContinue);
    EXPECT_EQ(driver.pick(lot, 0, 0).outcome, kRejected); // picked already
    (void)driver.pick(arc, 10, 0);
    (void)driver.pick(point, 7, 8);
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    const std::string& report = step.message;
    EXPECT_TRUE(contains(report, "3 entities listed")) << report;
    // The rectangle: 4 + 3 + 4 + 3 = 14 round, 4 x 3 = 12 inside.
    EXPECT_TRUE(contains(report, "closed, 4 vertices, length 14.000 m   area 12.000" + kSquare +
                                     " (0.0012 ha)"))
        << report;
    EXPECT_TRUE(contains(report, "3  E 4.000 N 3.000")) << report;
    // The quarter arc of radius 10: 10 * pi / 2 = 15.70796 long, its chord
    // 10 * sqrt(2) = 14.14214, from (10, 0) round to (0, 10).
    EXPECT_TRUE(contains(report, "from E 10.000 N 0.000  to E 0.000 N 10.000  anticlockwise"))
        << report;
    EXPECT_TRUE(contains(report, "length 15.708 m   delta 90" + kDeg + "00'00.00\"   chord 14.142 m"))
        << report;
    // The point, with its height beside its coordinates and its properties.
    EXPECT_TRUE(contains(report, "E 7.000 N 8.000  Z 1.250")) << report;
    EXPECT_TRUE(contains(report, "code = FC")) << report;
    EXPECT_TRUE(contains(report, "point = P9")) << report;
}

TEST(InquiryList, EnterWithNothingSelectedIsRefusedWithWhatToDo)
{
    ToolDriver driver;
    driver.start("inquiry.list");
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, kRejected);
    EXPECT_TRUE(contains(step.message, "nothing is selected")) << step.message;
}

TEST(InquiryTools, NoneOfThemChangesTheDrawing)
{
    ToolDriver driver;
    (void)driver.add(cmd::createLine({0, 0}, {3, 4}));
    const auto revision = driver.document().modelRevision();
    driver.start("inquiry.distance");
    (void)driver.click(0, 0);
    (void)driver.click(3, 4);
    driver.start("inquiry.id");
    (void)driver.click(0, 0);
    driver.start("inquiry.angle");
    (void)driver.click(0, 0);
    (void)driver.click(1, 0);
    (void)driver.click(0, 1);
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(driver.document().modelRevision(), revision);
}

} // namespace
