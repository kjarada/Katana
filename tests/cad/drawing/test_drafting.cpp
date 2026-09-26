// Precision input (include/katana/cad/drawing/drafting.hpp): the typed
// coordinate grammar with DMS angles and survey bearings, ortho, polar, the
// locks, object snap tracking, and how typed text reaches a tool.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/drawing/drafting.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/math/numerics.hpp"
#include "cad/tools/tool_driver.hpp"

using namespace katana::cad;
using katana::geometry::Point2;
using katana::math::kDegToRad;
using katana::math::kPi;

namespace {

void expectNear(const Point2& a, const Point2& b, double tolerance = 1e-9)
{
    EXPECT_NEAR(a.x, b.x, tolerance);
    EXPECT_NEAR(a.y, b.y, tolerance);
}

} // namespace

TEST(Drafting, DegreesMinutesAndSecondsParse)
{
    EXPECT_NEAR(*parseDegrees("45.5"), 45.5, 1e-15);
    EXPECT_NEAR(*parseDegrees("45d30'15\""), 45.0 + 30.0 / 60.0 + 15.0 / 3600.0, 1e-12);
    EXPECT_NEAR(*parseDegrees("45\xC2\xB0" "30'15.5\""), 45.0 + 30.0 / 60.0 + 15.5 / 3600.0, 1e-12);
    EXPECT_NEAR(*parseDegrees("45d30'"), 45.5, 1e-12);
    EXPECT_NEAR(*parseDegrees("45d"), 45.0, 1e-12);
    EXPECT_NEAR(*parseDegrees("-12d30'"), -12.5, 1e-12);
    EXPECT_NEAR(*parseDegrees("45D30'15"), 45.0 + 30.0 / 60.0 + 15.0 / 3600.0, 1e-12)
        << "a trailing number takes the next unit";
    EXPECT_FALSE(parseDegrees("45d75'").ok()) << "75 minutes";
    EXPECT_FALSE(parseDegrees("north").ok());
    EXPECT_FALSE(parseDegrees("").ok());
}

TEST(Drafting, DirectionsFollowTheConventionAndQuadrantBearingsAreAlwaysBearings)
{
    // 30 degrees counter-clockwise from east.
    EXPECT_NEAR(*parseDirection("30", AngleConvention::Counterclockwise), 30.0 * kDegToRad, 1e-12);
    // As a whole-circle bearing, 30 is 30 clockwise from north: 60 from east.
    EXPECT_NEAR(*parseDirection("30", AngleConvention::Bearing), 60.0 * kDegToRad, 1e-12);
    // N45E is north-east whatever the convention.
    EXPECT_NEAR(*parseDirection("N45d00'00\"E", AngleConvention::Counterclockwise),
                45.0 * kDegToRad, 1e-12);
    EXPECT_NEAR(*parseDirection("S30W", AngleConvention::Counterclockwise),
                (90.0 - 210.0) * kDegToRad, 1e-12);
    EXPECT_NEAR(*parseDirection("s 30d w", AngleConvention::Bearing), (90.0 - 210.0) * kDegToRad,
                1e-12);
    EXPECT_NEAR(*parseDirection("N10W", AngleConvention::Bearing), 100.0 * kDegToRad, 1e-12);
    EXPECT_FALSE(parseDirection("N95E", AngleConvention::Bearing).ok()) << "over 90 in a quadrant";
}

TEST(Drafting, BearingsFormatAsDegreesMinutesAndSeconds)
{
    EXPECT_EQ(formatDms(45.5), "45\xC2\xB0" "30'00\"");
    EXPECT_EQ(formatDms(12.0 + 30.0 / 60.0 + 15.25 / 3600.0, 2), "12\xC2\xB0" "30'15.25\"");
    // 59.9999" rounds to the next minute rather than printing 60".
    EXPECT_EQ(formatDms(1.0 + 59.0 / 60.0 + 59.9999 / 3600.0), "2\xC2\xB0" "00'00\"");
    // East is a bearing of 90; north-west 315.
    EXPECT_EQ(formatBearing(0.0), "90\xC2\xB0" "00'00\"");
    EXPECT_EQ(formatBearing(135.0 * kDegToRad), "315\xC2\xB0" "00'00\"");
    EXPECT_EQ(formatQuadrantBearing(135.0 * kDegToRad), "N45\xC2\xB0" "00'00\"W");
    EXPECT_EQ(formatQuadrantBearing(-60.0 * kDegToRad), "S30\xC2\xB0" "00'00\"E");
}

TEST(Drafting, TypedPointsAbsoluteRelativeAndPolar)
{
    const Point2 last(10, 10);
    expectNear(parsePrecisePoint("3,4", last)->point, Point2(3, 4));
    const auto withHeight = parsePrecisePoint("3,4,101.25", last);
    ASSERT_TRUE(withHeight.ok());
    EXPECT_EQ(withHeight->z, 101.25);
    expectNear(parsePrecisePoint("@3,4", last)->point, Point2(13, 14));
    expectNear(parsePrecisePoint("@10<90", last)->point, Point2(10, 20));
    // A DMS angle after the <, counter-clockwise from east by default.
    expectNear(parsePrecisePoint("@10<45d", last)->point,
               Point2(10 + 10 * std::sqrt(0.5), 10 + 10 * std::sqrt(0.5)));
    // A quadrant bearing: S45E is south-east.
    expectNear(parsePrecisePoint("@10<S45d00'00\"E", last)->point,
               Point2(10 + 10 * std::sqrt(0.5), 10 - 10 * std::sqrt(0.5)));
    // With bearings as the convention, <0 is north.
    DraftingSettings bearings;
    bearings.angles = AngleConvention::Bearing;
    expectNear(parsePrecisePoint("@25<0", last, bearings)->point, Point2(10, 35));
    expectNear(parsePrecisePoint("@25<90d00'00\"", last, bearings)->point, Point2(35, 10));
    EXPECT_EQ(parsePrecisePoint("@1,1", std::nullopt).error().code,
              katana::core::ErrorCode::InvalidState);
    EXPECT_FALSE(parsePrecisePoint("10<45", last).ok()) << "polar is relative";
    EXPECT_FALSE(parsePrecisePoint("1,2,3,4", last).ok());
}

TEST(Drafting, OrthoPolarAndTheLocks)
{
    DraftingSettings settings;
    const Point2 base(0, 0);
    // Nothing on: the cursor.
    expectNear(constrain(base, Point2(3, 1), settings, 0.5).point, Point2(3, 1));
    settings.ortho = true;
    expectNear(constrain(base, Point2(3, 1), settings, 0.5).point, Point2(3, 0));
    expectNear(constrain(base, Point2(1, -3), settings, 0.5).point, Point2(0, -3));
    settings.ortho = false;
    settings.polar = true;
    settings.polarIncrement = 45.0 * kDegToRad;
    // Near the 45 degree ray: onto it.
    const auto polar = constrain(base, Point2(10, 10.2), settings, 0.5);
    expectNear(polar.point, Point2(10.1, 10.1));
    EXPECT_EQ(polar.label.rfind("Polar 45", 0), 0u);
    // Far from any ray: left alone.
    expectNear(constrain(base, Point2(10, 4), settings, 0.5).point, Point2(10, 4));
    settings.polar = false;
    settings.angleLock = 90.0 * kDegToRad;
    expectNear(constrain(base, Point2(2, 7), settings, 0.5).point, Point2(0, 7));
    settings.lengthLock = 5.0;
    expectNear(constrain(base, Point2(2, 7), settings, 0.5).point, Point2(0, 5));
    settings.angleLock.reset();
    expectNear(constrain(base, Point2(3, 4), settings, 0.5).point, Point2(3, 4));
    expectNear(constrain(base, Point2(6, 8), settings, 0.5).point, Point2(3, 4));
}

TEST(Drafting, ObjectSnapTrackingFindsPathsAndTheirCrossings)
{
    const std::vector<Point2> acquired{Point2(0, 0), Point2(10, 5)};
    // On the horizontal through (0,0), far from it.
    const auto path = trackAcquired(acquired, Point2(20, 0.1), 0.5);
    ASSERT_TRUE(path.has_value());
    expectNear(path->point, Point2(20, 0));
    // Near where the vertical through (0,0) meets the horizontal through
    // (10,5): the crossing wins.
    const auto crossing = trackAcquired(acquired, Point2(0.2, 5.3), 0.5);
    ASSERT_TRUE(crossing.has_value());
    expectNear(crossing->point, Point2(0, 5));
    EXPECT_EQ(crossing->label, "Tracking crossing");
    EXPECT_FALSE(trackAcquired(acquired, Point2(5, 2.5), 0.5).has_value());
}

TEST(Drafting, FromAndMidBetween)
{
    expectNear(*fromBase(Point2(10, 10), "@3,4"), Point2(13, 14));
    expectNear(*fromBase(Point2(10, 10), "5<90"), Point2(10, 15)); // the @ is implied
    expectNear(midBetween(Point2(0, 0), Point2(4, 6)), Point2(2, 3));
    expectNear(directDistance(Point2(0, 0), Point2(3, 4), 10.0), Point2(6, 8));
}

TEST(DraftingTypedInput, ADistanceTypedAtALinePromptGoesAlongTheCursor)
{
    katana::cad::testing::ToolDriver driver;
    driver.start("draw.line");
    driver.click(0, 0);
    DraftingSettings settings;
    // Direct distance entry: 5 towards a cursor at (30, 40).
    const auto step =
        routeTypedInput(driver.tool(), "5", settings, std::optional<Point2>(Point2(30, 40)));
    EXPECT_EQ(step.outcome, ToolStep::Outcome::Continue);
    expectNear(*driver.tool().lastPoint(), Point2(3, 4));
}

TEST(DraftingTypedInput, AnglesAndLengthsLockAndClear)
{
    katana::cad::testing::ToolDriver driver;
    driver.start("draw.line");
    driver.click(0, 0);
    DraftingSettings settings;
    EXPECT_EQ(routeTypedInput(driver.tool(), "<N45E", settings, std::nullopt).outcome,
              ToolStep::Outcome::Continue);
    ASSERT_TRUE(settings.angleLock.has_value());
    EXPECT_NEAR(*settings.angleLock, 45.0 * kDegToRad, 1e-12);
    // A distance now goes along the lock whatever the cursor does.
    (void)routeTypedInput(driver.tool(), "10", settings, std::optional<Point2>(Point2(-5, 0)));
    expectNear(*driver.tool().lastPoint(), Point2(10 * std::sqrt(0.5), 10 * std::sqrt(0.5)));
    (void)routeTypedInput(driver.tool(), "=7", settings, std::nullopt);
    EXPECT_EQ(settings.lengthLock, 7.0);
    (void)routeTypedInput(driver.tool(), "<", settings, std::nullopt);
    (void)routeTypedInput(driver.tool(), "=", settings, std::nullopt);
    EXPECT_FALSE(settings.angleLock.has_value());
    EXPECT_FALSE(settings.lengthLock.has_value());
    EXPECT_EQ(routeTypedInput(driver.tool(), "=abc", settings, std::nullopt).outcome,
              ToolStep::Outcome::Rejected);
}

TEST(DraftingTypedInput, ARadiusIsStillTheToolsValue)
{
    // A number the tool takes is its value, not a distance along the cursor.
    katana::cad::testing::ToolDriver driver;
    driver.start("draw.circle");
    driver.click(5, 5);
    DraftingSettings settings;
    auto step =
        routeTypedInput(driver.tool(), "2.5", settings, std::optional<Point2>(Point2(100, 5)));
    ASSERT_EQ(step.outcome, ToolStep::Outcome::Done);
    ASSERT_TRUE(driver.document().execute(std::move(step.command)).ok());
    const auto& entities = driver.document().model().entities;
    ASSERT_EQ(entities.size(), 1u);
    bool found = false;
    entities.forEach([&](const katana::entity::Entity& e) {
        if (const auto* c = std::get_if<katana::geometry::Circle2>(&e.geometry)) {
            EXPECT_NEAR(c->radius, 2.5, 1e-12);
            found = true;
        }
    });
    EXPECT_TRUE(found);
}

TEST(Drafting, TrackingPointsKeepTheSevenMostRecentAndMoveARepeatToTheFront)
{
    TrackingPoints tracking;
    for (int i = 0; i < 9; ++i) {
        tracking.acquire(Point2(i, 0), 1e-9);
    }
    ASSERT_EQ(tracking.points().size(), TrackingPoints::kMost);
    EXPECT_EQ(tracking.points().front(), Point2(8, 0));
    EXPECT_EQ(tracking.points().back(), Point2(2, 0));
    tracking.acquire(Point2(5, 0), 1e-9);
    EXPECT_EQ(tracking.points().front(), Point2(5, 0));
    EXPECT_EQ(tracking.points().size(), TrackingPoints::kMost) << "moved, not added again";
    EXPECT_TRUE(TrackingPoints::tracks(SnapMode::Endpoint));
    EXPECT_TRUE(TrackingPoints::tracks(SnapMode::Quadrant));
    EXPECT_FALSE(TrackingPoints::tracks(SnapMode::Nearest));
    EXPECT_FALSE(TrackingPoints::tracks(SnapMode::Grid));
    tracking.clear();
    EXPECT_TRUE(tracking.points().empty());
}
