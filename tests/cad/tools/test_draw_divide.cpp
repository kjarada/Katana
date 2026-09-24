// Divide and Measure (src/katana_cad/tools/draw_divide.cpp): points along an
// object, checked against positions worked out by hand. Lengths are chosen so
// the stations are exact in binary (quarters of 10, halves of 16) wherever
// the object is straight; round a circle the positions go through cos and
// sin and are compared to 1e-12.

#include <algorithm>
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
using katana::geometry::Point2;
using katana::geometry::Polyline2;

constexpr auto kContinue = ToolStep::Outcome::Continue;
constexpr auto kDone = ToolStep::Outcome::Done;
constexpr auto kRejected = ToolStep::Outcome::Rejected;

// Every point entity in the drawing, in id order - the order they were made in.
std::vector<Entity> points(ToolDriver& driver)
{
    std::vector<Entity> out;
    driver.document().model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry)) {
            out.push_back(entity);
        }
    });
    std::ranges::sort(out, {}, &Entity::id);
    return out;
}

Point2 at(const Entity& point)
{
    return std::get<katana::entity::PointGeometry>(point.geometry).position;
}

TEST(DrawDivide, ALineInFourPartsGetsThreePointsAtItsQuarters)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("draw.divide");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.pick(line, 5, 0).outcome, kContinue);
    EXPECT_EQ(driver.type("1").outcome, kRejected); // one part divides nothing
    const ToolStep step = driver.type("4");
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.message, "Divided into 4 parts: 3 points");
    // 10 / 4 = 2.5: the ends are marked already, so 2.5, 5 and 7.5.
    const auto made = points(driver);
    ASSERT_EQ(made.size(), 3u);
    EXPECT_EQ(at(made[0]), Point2(2.5, 0));
    EXPECT_EQ(at(made[1]), Point2(5, 0));
    EXPECT_EQ(at(made[2]), Point2(7.5, 0));
    // One command: one undo takes all three.
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(points(driver).empty());
}

TEST(DrawDivide, AClosedSquareInEightPartsGetsAPointAtEveryDivisionIncludingTheStart)
{
    ToolDriver driver;
    const EntityId square =
        driver.add(cmd::createPolyline(Polyline2{{{0, 0}, {4, 0}, {4, 4}, {0, 4}}, true}));
    driver.start("draw.divide");
    (void)driver.pick(square, 2, 0);
    ASSERT_EQ(driver.type("8").outcome, kDone);
    // Perimeter 16, parts of 2, from the first vertex round the way it runs.
    const std::vector<Point2> expected = {{0, 0}, {2, 0}, {4, 0}, {4, 2},
                                          {4, 4}, {2, 4}, {0, 4}, {0, 2}};
    const auto made = points(driver);
    ASSERT_EQ(made.size(), expected.size());
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(at(made[i]), expected[i]) << "point " << i;
    }
}

TEST(DrawDivide, ACircleInFourPartsGetsItsQuadrantsFromEastAnticlockwise)
{
    ToolDriver driver;
    const EntityId circle = driver.add(cmd::createCircle({0, 0}, 1.0));
    driver.start("draw.divide");
    (void)driver.pick(circle, 1, 0);
    ASSERT_EQ(driver.type("4").outcome, kDone);
    const std::vector<Point2> expected = {{1, 0}, {0, 1}, {-1, 0}, {0, -1}};
    const auto made = points(driver);
    ASSERT_EQ(made.size(), 4u);
    for (std::size_t i = 0; i < 4; ++i) {
        EXPECT_NEAR(at(made[i]).x, expected[i].x, 1e-12) << "point " << i;
        EXPECT_NEAR(at(made[i]).y, expected[i].y, 1e-12) << "point " << i;
    }
}

TEST(DrawMeasure, EveryThreeAlongATenLineGoesFromTheEndNearerThePick)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("draw.measure");
    (void)driver.pick(line, 1, 0); // nearer the start
    ASSERT_EQ(driver.type("3").outcome, kDone);
    // 3, 6, 9; 12 is past the end.
    auto made = points(driver);
    ASSERT_EQ(made.size(), 3u);
    EXPECT_EQ(at(made[0]), Point2(3, 0));
    EXPECT_EQ(at(made[2]), Point2(9, 0));

    ASSERT_TRUE(driver.document().undo().ok());
    driver.start("draw.measure");
    (void)driver.pick(line, 9, 0); // nearer the end: 10 - 3, 10 - 6, 10 - 9
    ASSERT_EQ(driver.type("3").outcome, kDone);
    made = points(driver);
    ASSERT_EQ(made.size(), 3u);
    EXPECT_EQ(at(made[0]), Point2(7, 0));
    EXPECT_EQ(at(made[1]), Point2(4, 0));
    EXPECT_EQ(at(made[2]), Point2(1, 0));
}

TEST(DrawMeasure, ALengthThatDividesTheObjectExactlyPutsNoPointOnItsEnd)
{
    ToolDriver driver;
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("draw.measure");
    (void)driver.pick(line, 0, 0);
    ASSERT_EQ(driver.type("2.5").outcome, kDone);
    // 2.5, 5, 7.5 - and not 10, the end.
    const auto made = points(driver);
    ASSERT_EQ(made.size(), 3u);
    EXPECT_EQ(at(made[2]), Point2(7.5, 0));
}

TEST(DrawDivide, PointsAlongA3DStringGetTheHeightInterpolatedThere)
{
    ToolDriver driver;
    Entity string;
    string.geometry = Polyline2{{{0, 0}, {10, 0}}, false};
    katana::entity::setHeights(string.properties, {100.0, 110.0});
    const EntityId id = driver.add(cmd::createEntities({string}));
    driver.start("draw.divide");
    (void)driver.pick(id, 5, 0);
    ASSERT_EQ(driver.type("2").outcome, kDone);
    // The midpoint, half way from 100 to 110.
    const auto made = points(driver);
    ASSERT_EQ(made.size(), 1u);
    EXPECT_EQ(at(made[0]), Point2(5, 0));
    const auto height = katana::entity::heightsOf(made[0].properties, 1).front();
    ASSERT_TRUE(height.has_value());
    EXPECT_DOUBLE_EQ(*height, 105.0);
}

TEST(DrawDivide, TheStyleOptionPutsThePointsInANamedStyleAndRefusesAnUnknownOne)
{
    ToolDriver driver;
    katana::entity::Style post;
    post.name = "POST";
    ASSERT_TRUE(driver.document().execute(cmd::createStyle(post)).ok());
    const EntityId line = driver.add(cmd::createLine({0, 0}, {10, 0}));
    driver.start("draw.divide");
    (void)driver.pick(line, 5, 0);
    EXPECT_EQ(driver.type("S").outcome, kContinue);
    EXPECT_EQ(driver.type("NO SUCH STYLE").outcome, kRejected);
    EXPECT_EQ(driver.type("POST").outcome, kContinue);
    ASSERT_EQ(driver.type("2").outcome, kDone);
    const auto made = points(driver);
    ASSERT_EQ(made.size(), 1u);
    EXPECT_EQ(made[0].style, "POST");
}

TEST(DrawDivide, AnObjectWithNoLengthIsRefusedWithItsKind)
{
    ToolDriver driver;
    const EntityId text =
        driver.add(cmd::createText(katana::entity::TextGeometry{{0, 0}, "LOT 42", 2.5, 0.0}));
    driver.start("draw.divide");
    const ToolStep step = driver.pick(text, 0, 0);
    EXPECT_EQ(step.outcome, kRejected);
    EXPECT_EQ(step.message, "a text has no length to place points along");
}

} // namespace
