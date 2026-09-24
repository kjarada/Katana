// Lengthen and Reverse (src/katana_cad/tools/modify_length.cpp), driven as
// the plan view drives them and checked against geometry worked out by hand.
// Straight results are exact in binary and compared exactly; an arc's end
// goes through cos and sin and is compared to 1e-12.

#include <cmath>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <gtest/gtest.h>

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
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kPi;

constexpr auto kContinue = ToolStep::Outcome::Continue;
constexpr auto kDone = ToolStep::Outcome::Done;
constexpr auto kRejected = ToolStep::Outcome::Rejected;

template <typename T>
T geometryOf(ToolDriver& driver, EntityId id)
{
    const Entity* entity = driver.document().model().entities.find(id);
    EXPECT_NE(entity, nullptr);
    if (entity == nullptr || !std::holds_alternative<T>(entity->geometry)) {
        ADD_FAILURE() << "entity " << id << " is not of the kind expected";
        return {};
    }
    return std::get<T>(entity->geometry);
}

// ---- Lengthen ----------------------------------------------------------------------

TEST(ModifyLengthen, PickingAnObjectFirstReportsItsLength)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {3, 4}));
    const EntityId arc = driver.add(cmd::createArc(Arc2{{0, 0}, 10.0, 0.0, kPi / 2.0}));
    driver.start("modify.lengthen");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.pick(line, 3, 4).message, "Current length 5.000");
    // 10 * pi / 2 = 15.70796.
    EXPECT_EQ(driver.pick(arc, 10, 0).message,
              "Current length 15.708, included angle 90\xC2\xB0" "00'00.00\"");
    EXPECT_EQ(driver.executed(), 0);
}

TEST(ModifyLengthen, ADeltaMovesTheEndNearerThePickAndANegativeOneShortens)
{
    ToolDriver driver;
    const EntityId a = driver.add(cmd::createLine({0, 0}, {10, 0}));
    const EntityId b = driver.add(cmd::createLine({0, 5}, {10, 5}));
    driver.start("modify.lengthen");
    EXPECT_EQ(driver.type("DE").outcome, kContinue);
    EXPECT_EQ(driver.type("2").outcome, kContinue);
    // Nearer the end (10, 0): the end goes on 2, to (12, 0).
    EXPECT_EQ(driver.pick(a, 9, 0).message, "Length 12.000");
    EXPECT_EQ(driver.type("DE").outcome, kContinue);
    EXPECT_EQ(driver.type("-2").outcome, kContinue);
    // Nearer the start (0, 5): 10 - 2 = 8 long from the fixed end (10, 5),
    // so the start comes in to (2, 5).
    (void)driver.pick(b, 1, 5);
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.message, "Lengthened 2 objects");
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(geometryOf<Segment2>(driver, a), (Segment2{{0, 0}, {12, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(driver, b), (Segment2{{2, 5}, {10, 5}}));
    // One undo takes both back.
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(geometryOf<Segment2>(driver, a), (Segment2{{0, 0}, {10, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(driver, b), (Segment2{{0, 5}, {10, 5}}));
}

TEST(ModifyLengthen, APercentageScalesTheLengthFromTheFarEnd)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("modify.lengthen");
    (void)driver.type("P");
    (void)driver.type("50");
    (void)driver.pick(line, 1, 0); // nearer the start; the end (10, 0) stays
    ASSERT_EQ(driver.enter().outcome, kDone);
    // 50 % of 10 = 5, from (10, 0) back to (5, 0).
    EXPECT_EQ(geometryOf<Segment2>(driver, line), (Segment2{{5, 0}, {10, 0}}));
}

TEST(ModifyLengthen, ATotalGrowsAPolylineAlongItsEndSegmentAndShrinksItAlongItsPath)
{
    ToolDriver driver;
    // 10 east then 10 north: 20 long.
    const EntityId longer =
        driver.add(cmd::createPolyline(Polyline2{{{0, 0}, {10, 0}, {10, 10}}, false}));
    const EntityId shorter =
        driver.add(cmd::createPolyline(Polyline2{{{0, 20}, {10, 20}, {10, 30}}, false}));
    driver.start("modify.lengthen");
    (void)driver.type("T");
    (void)driver.type("25");
    // 25 - 20 = 5 more along the last leg, north: (10, 10) to (10, 15).
    (void)driver.pick(longer, 10, 9);
    (void)driver.type("T");
    (void)driver.type("15");
    // 15 from the start: 10 east, then 5 of the northward leg - (10, 25).
    (void)driver.pick(shorter, 10, 29);
    ASSERT_EQ(driver.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(driver, longer).vertices,
              (std::vector<Point2>{{0, 0}, {10, 0}, {10, 15}}));
    EXPECT_EQ(geometryOf<Polyline2>(driver, shorter).vertices,
              (std::vector<Point2>{{0, 20}, {10, 20}, {10, 25}}));
}

TEST(ModifyLengthen, AnArcKeepsItsCentreAndRadiusAndChangesItsSweep)
{
    ToolDriver driver;
    // A quarter circle of radius 10 from (10, 0) to (0, 10): 5 * pi long.
    const EntityId arc = driver.add(cmd::createArc(Arc2{{0, 0}, 10.0, 0.0, kPi / 2.0}));
    driver.start("modify.lengthen");
    (void)driver.type("DE");
    // Another quarter: 5 * pi more makes a half circle ending at (-10, 0).
    (void)driver.type("15.707963267948966");
    (void)driver.pick(arc, 0, 10);
    ASSERT_EQ(driver.enter().outcome, kDone);
    const Arc2 half = geometryOf<Arc2>(driver, arc);
    EXPECT_EQ(half.center, Point2(0, 0));
    EXPECT_EQ(half.radius, 10.0);
    EXPECT_EQ(half.startAngle, 0.0);
    EXPECT_NEAR(half.sweep, kPi, 1e-12);
    EXPECT_NEAR(half.endPoint().x, -10.0, 1e-12);
    EXPECT_NEAR(half.endPoint().y, 0.0, 1e-12);
}

TEST(ModifyLengthen, DynamicPutsTheEndWhereTheCursorIsAlongTheLine)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("modify.lengthen");
    (void)driver.type("DY");
    (void)driver.pick(line, 9, 0);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    // (14, 3) projects onto the line's direction at 14.
    EXPECT_EQ(driver.click(14, 3).outcome, kContinue);
    ASSERT_EQ(driver.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(driver, line), (Segment2{{0, 0}, {14, 0}}));
}

TEST(ModifyLengthen, EscKeepsTheChangesAlreadyMade)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("modify.lengthen");
    (void)driver.type("DE");
    (void)driver.type("5");
    (void)driver.pick(line, 9, 0);
    const ToolStep step = driver.cancel();
    EXPECT_EQ(step.outcome, kDone);
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(geometryOf<Segment2>(driver, line), (Segment2{{0, 0}, {15, 0}}));
}

TEST(ModifyLengthen, EscBeforeAnyChangeKeepsNothingAndAppliesNoDefault)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("modify.lengthen");
    (void)driver.type("T"); // at the total's prompt, whose Enter takes <1>
    (void)driver.cancel();
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(geometryOf<Segment2>(driver, line), (Segment2{{0, 0}, {10, 0}}));
}

TEST(ModifyLengthen, AClosedOutlineHasNoEndAndATooShortLengthIsRefused)
{
    ToolDriver driver;
    const EntityId circle = driver.add(cmd::createCircle({0, 0}, 5.0));
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("modify.lengthen");
    (void)driver.type("DE");
    (void)driver.type("-10");
    EXPECT_EQ(driver.pick(circle, 5, 0).message, "a circle has no end to lengthen");
    // 10 - 10 leaves nothing.
    EXPECT_EQ(driver.pick(line, 9, 0).outcome, kRejected);
    EXPECT_EQ(driver.enter().outcome, kDone);
    EXPECT_EQ(driver.executed(), 0);
}

TEST(ModifyLengthen, UndoTakesBackTheLastChangeOnly)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("modify.lengthen");
    (void)driver.type("DE");
    (void)driver.type("1");
    (void)driver.pick(line, 9, 0); // 11
    (void)driver.pick(line, 9, 0); // 12
    EXPECT_EQ(driver.undo().outcome, kContinue);
    ASSERT_EQ(driver.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(driver, line), (Segment2{{0, 0}, {11, 0}}));
}

// ---- Reverse -----------------------------------------------------------------------

TEST(ModifyReverse, LinesArcsAndPolylinesTurnRoundWithTheirHeights)
{
    ToolDriver driver;
    Entity string;
    string.geometry = Polyline2{{{0, 0}, {10, 0}, {10, 5}}, false};
    katana::entity::setHeights(string.properties, {1.0, 2.0, 3.0});
    const EntityId polyline = driver.add(cmd::createEntities({string}));
    const EntityId line = driver.add(cmd::createLine({0, 0}, {3, 4}));
    const EntityId arc = driver.add(cmd::createArc(Arc2{{0, 0}, 10.0, 0.0, kPi / 2.0}));
    const EntityId circle = driver.add(cmd::createCircle({0, 0}, 1.0));
    driver.document().selection().set({polyline, line, arc, circle});
    driver.start("modify.reverse");
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.message, "Reversed 3 objects; 1 skipped: circle has no direction");
    EXPECT_EQ(geometryOf<Polyline2>(driver, polyline).vertices,
              (std::vector<Point2>{{10, 5}, {10, 0}, {0, 0}}));
    const Entity* turned = driver.document().model().entities.find(polyline);
    ASSERT_NE(turned, nullptr);
    const auto heights = katana::entity::heightsOf(turned->properties, 3);
    EXPECT_EQ(heights, (std::vector<std::optional<double>>{3.0, 2.0, 1.0}));
    EXPECT_EQ(geometryOf<Segment2>(driver, line), (Segment2{{3, 4}, {0, 0}}));
    // The same quarter circle, drawn from (0, 10) back to (10, 0).
    const Arc2 back = geometryOf<Arc2>(driver, arc);
    EXPECT_EQ(back.startAngle, kPi / 2.0);
    EXPECT_EQ(back.sweep, -kPi / 2.0);
    EXPECT_EQ(driver.executed(), 1);
}

TEST(ModifyReverse, NothingWithADirectionIsRefusedAndSaysWhy)
{
    ToolDriver driver;
    const EntityId circle = driver.add(cmd::createCircle({0, 0}, 1.0));
    driver.document().selection().set({circle});
    driver.start("modify.reverse");
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, kRejected);
    EXPECT_EQ(step.message,
              "nothing selected can be reversed; 1 skipped: circle has no direction");
}

} // namespace
