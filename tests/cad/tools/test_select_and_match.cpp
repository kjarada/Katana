// Match Properties (modify_properties.cpp), Select Similar and Quick Select
// (select_tools.cpp), and what Esc keeps of Copy's work (modify_transform.cpp,
// InteractiveTool::cancel). Each drawing is small enough that which entities
// a selection must hold is counted by hand from the set-up.

#include <algorithm>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <gtest/gtest.h>

#include "katana/commands/entity_commands.hpp"
#include "tool_driver.hpp"

namespace {

namespace cmd = katana::commands;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Segment2;

constexpr auto kContinue = ToolStep::Outcome::Continue;
constexpr auto kDone = ToolStep::Outcome::Done;
constexpr auto kRejected = ToolStep::Outcome::Rejected;

void addLayer(ToolDriver& driver, const std::string& name)
{
    katana::entity::Layer layer;
    layer.name = name;
    ASSERT_TRUE(driver.document().execute(cmd::createLayer(layer)).ok());
}

void addStyle(ToolDriver& driver, const std::string& name)
{
    katana::entity::Style style;
    style.name = name;
    ASSERT_TRUE(driver.document().execute(cmd::createStyle(style)).ok());
}

EntityId addEntity(ToolDriver& driver, katana::entity::Geometry geometry,
                   const std::string& layer = "0", const std::string& style = {},
                   std::optional<Color> color = std::nullopt,
                   katana::entity::PropertyMap properties = {})
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = layer;
    entity.style = style;
    entity.color = color;
    entity.properties = std::move(properties);
    return driver.add(cmd::createEntities({entity}));
}

const Entity& entityOf(ToolDriver& driver, EntityId id)
{
    const Entity* entity = driver.document().model().entities.find(id);
    EXPECT_NE(entity, nullptr);
    static const Entity missing;
    return entity != nullptr ? *entity : missing;
}

std::vector<EntityId> selected(ToolDriver& driver) { return driver.document().selection().ids(); }

Segment2 line(double y) { return Segment2{{0, y}, {10, y}}; }

// ---- Match Properties --------------------------------------------------------------

TEST(MatchProperties, TheTargetsTakeTheSourcesLayerStyleAndColourInOneCommand)
{
    ToolDriver driver;
    addLayer(driver, "ROAD");
    addStyle(driver, "TB");
    const Color red = *Color::fromHex("#FF0000");
    const EntityId source = addEntity(driver, line(0), "ROAD", "TB", red);
    const EntityId a = addEntity(driver, line(5));
    const EntityId b = addEntity(driver, katana::geometry::Circle2{{0, 0}, 1.0});
    driver.start("modify.match_properties");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.pick(source, 5, 0).message,
              "Source: layer ROAD, style TB, colour #FF0000");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection);
    (void)driver.pick(a, 5, 5);
    (void)driver.pick(b, 1, 0);
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.message, "Matched layer, colour and style on 2 objects");
    EXPECT_EQ(driver.executed(), 1);
    for (const EntityId id : {a, b}) {
        EXPECT_EQ(entityOf(driver, id).layer, "ROAD");
        EXPECT_EQ(entityOf(driver, id).style, "TB");
        EXPECT_EQ(entityOf(driver, id).color, red);
    }
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(entityOf(driver, a).layer, "0");
    EXPECT_EQ(entityOf(driver, b).style, "");
}

TEST(MatchProperties, SettingsChooseWhichPropertiesGoAcross)
{
    ToolDriver driver;
    addLayer(driver, "ROAD");
    addStyle(driver, "TB");
    const EntityId source = addEntity(driver, line(0), "ROAD", "TB", *Color::fromHex("#00FF00"));
    const EntityId target = addEntity(driver, line(5));
    driver.start("modify.match_properties");
    (void)driver.pick(source, 5, 0);
    EXPECT_EQ(driver.type("S").outcome, kContinue);
    EXPECT_EQ(driver.type("banana").outcome, kRejected);
    EXPECT_EQ(driver.type("L").message, "Copying layer");
    (void)driver.pick(target, 5, 5);
    ASSERT_EQ(driver.enter().outcome, kDone);
    EXPECT_EQ(entityOf(driver, target).layer, "ROAD");
    EXPECT_EQ(entityOf(driver, target).style, "");
    EXPECT_FALSE(entityOf(driver, target).color.has_value());
    // Remembered for the next use, as AutoCAD remembers them; put back.
    driver.start("modify.match_properties");
    (void)driver.pick(source, 5, 0);
    EXPECT_EQ(driver.tool().prompt(), "Select destination objects or [Settings], then press Enter");
    (void)driver.type("S");
    EXPECT_EQ(driver.type("All").message, "Copying layer, colour and style");
}

TEST(MatchProperties, ATargetOnALockedLayerIsLeftAndSaidSo)
{
    ToolDriver driver;
    addLayer(driver, "ROAD");
    addLayer(driver, "LOCKED");
    const EntityId source = addEntity(driver, line(0), "ROAD");
    const EntityId open = addEntity(driver, line(5));
    const EntityId locked = addEntity(driver, line(10), "LOCKED");
    katana::entity::Layer lockedLayer;
    lockedLayer.name = "LOCKED";
    lockedLayer.locked = true;
    ASSERT_TRUE(driver.document().execute(cmd::updateLayer(lockedLayer)).ok());
    driver.start("modify.match_properties");
    (void)driver.pick(source, 5, 0);
    (void)driver.pick(open, 5, 5);
    (void)driver.pick(locked, 5, 10);
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.message,
              "Matched layer, colour and style on 1 object; 1 on a locked layer left as it is");
    EXPECT_EQ(entityOf(driver, open).layer, "ROAD");
    EXPECT_EQ(entityOf(driver, locked).layer, "LOCKED");
}

// ---- Select Similar ----------------------------------------------------------------

TEST(SelectSimilar, APickedLineSelectsTheLinesOfItsLayerAndStyleOnly)
{
    ToolDriver driver;
    addLayer(driver, "A");
    addLayer(driver, "B");
    addStyle(driver, "TB");
    const EntityId a1 = addEntity(driver, line(0), "A");
    const EntityId a2 = addEntity(driver, line(1), "A");
    const EntityId a3 = addEntity(driver, line(2), "A");
    (void)addEntity(driver, line(3), "B");                                  // other layer
    const EntityId styled = addEntity(driver, line(4), "A", "TB");          // other style
    const EntityId c1 = addEntity(driver, katana::geometry::Circle2{{0, 0}, 1.0}, "A");
    const EntityId c2 = addEntity(driver, katana::geometry::Circle2{{5, 0}, 1.0}, "A");
    driver.document().selection().set({a1});
    driver.start("select.similar");
    const ToolStep step = driver.enter();
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.command, nullptr);
    // The three plain lines on A: not the one on B, the styled one or the circles.
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{a1, a2, a3}));
    EXPECT_EQ(step.message, "3 entities selected: the same kind, layer and style as the 1 picked");

    // Matched on the layer alone: everything on A.
    driver.start("select.similar");
    (void)driver.type("SE");
    EXPECT_EQ(driver.type("L").message, "Matching on layer");
    ASSERT_EQ(driver.enter().outcome, kDone);
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{a1, a2, a3, styled, c1, c2}));
    // The settings are the program's, kept between uses: put them back.
    driver.start("select.similar");
    (void)driver.type("SE");
    EXPECT_EQ(driver.type("kind,layer,style").message, "Matching on kind, layer and style");
}

// ---- Quick Select ------------------------------------------------------------------

class QuickSelect : public ::testing::Test {
  protected:
    void SetUp() override
    {
        addLayer(driver, "SURVEY");
        addLayer(driver, "ROAD");
        p1 = addEntity(driver, katana::entity::PointGeometry{{0, 0}}, "SURVEY", {}, {},
                       {{"code", std::string("TB")}, {"elevation", 10.0}});
        p2 = addEntity(driver, katana::entity::PointGeometry{{1, 0}}, "SURVEY", {}, {},
                       {{"code", std::string("TB")}, {"elevation", 20.0}});
        p3 = addEntity(driver, katana::entity::PointGeometry{{2, 0}}, "SURVEY", {}, {},
                       {{"code", std::string("FC")}, {"elevation", 30.0}});
        road = addEntity(driver, line(5), "ROAD");
    }

    // Kind, layer and condition typed in turn; "" is Enter.
    ToolStep run(const std::string& kind, const std::string& layer, const std::string& condition)
    {
        driver.start("select.quick");
        EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
        const auto answer = [&](const std::string& text) {
            return text.empty() ? driver.enter() : driver.type(text);
        };
        (void)answer(kind);
        (void)answer(layer);
        return answer(condition);
    }

    ToolDriver driver;
    EntityId p1 = 0;
    EntityId p2 = 0;
    EntityId p3 = 0;
    EntityId road = 0;
};

TEST_F(QuickSelect, ANumericConditionComparesHeightsAsNumbers)
{
    const ToolStep step = run("", "", "elevation > 15");
    ASSERT_EQ(step.outcome, kDone);
    // 20 and 30 exceed 15; 10 does not, and the line has no elevation.
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{p2, p3}));
    EXPECT_EQ(step.message,
              "2 entities selected: any kind on any layer where elevation > 15, in the drawing");
}

TEST_F(QuickSelect, KindLayerWildcardAndATextWildcardCombine)
{
    ASSERT_EQ(run("point", "surv*", "code = t?").outcome, kDone);
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{p1, p2}));
    // With nothing selected, so it does not ask where to look.
    driver.document().selection().clear();
    ASSERT_EQ(run("Line", "", "").outcome, kDone);
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{road}));
}

TEST_F(QuickSelect, NotEqualSelectsOnlyEntitiesThatHaveTheProperty)
{
    ASSERT_EQ(run("", "", "code <> TB").outcome, kDone);
    // FC only: the line has no code at all, which is not "a code other than TB".
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{p3}));
}

TEST_F(QuickSelect, OrderingTextAndAMalformedConditionAreRefused)
{
    EXPECT_EQ(run("", "", "code > TB").outcome, kRejected);
    EXPECT_EQ(driver.type("no operator here").outcome, kRejected);
    EXPECT_EQ(driver.type("Point").outcome, kRejected); // still at the condition
    driver.start("select.quick");
    EXPECT_EQ(driver.type("Banana").outcome, kRejected);
}

TEST_F(QuickSelect, WithASelectionGivenItAsksWhereToLook)
{
    driver.document().selection().set({p1, p3, road});
    driver.start("select.quick");
    (void)driver.type("Point");
    (void)driver.enter();
    (void)driver.enter();
    EXPECT_EQ(driver.tool().prompt(), "Look in [Drawing/Selection] <Selection>");
    ASSERT_EQ(driver.enter().outcome, kDone);
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{p1, p3}));

    driver.document().selection().set({p1});
    driver.start("select.quick");
    (void)driver.type("Point");
    (void)driver.enter();
    (void)driver.enter();
    ASSERT_EQ(driver.type("D").outcome, kDone);
    EXPECT_EQ(selected(driver), (std::vector<EntityId>{p1, p2, p3}));
}

// ---- Esc keeps what Copy placed -------------------------------------------------------

std::size_t lineCount(ToolDriver& driver)
{
    std::size_t count = 0;
    driver.document().model().entities.forEach([&](const Entity& entity) {
        count += std::holds_alternative<Segment2>(entity.geometry) ? 1 : 0;
    });
    return count;
}

TEST(CopyEscape, EscKeepsTheCopiesAlreadyPlacedAsOneCommand)
{
    ToolDriver driver;
    const EntityId original = driver.add(cmd::createLine({0, 0}, {1, 0}));
    driver.document().selection().set({original});
    driver.start("modify.copy");
    (void)driver.click(0, 0);  // base point
    (void)driver.click(10, 0); // a copy at +10
    (void)driver.click(20, 0); // and at +20
    const ToolStep step = driver.cancel();
    ASSERT_EQ(step.outcome, kDone);
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(lineCount(driver), 3u);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(lineCount(driver), 1u);
}

TEST(CopyEscape, EscWithNoCopyPlacedAppliesNoDefault)
{
    ToolDriver driver;
    const EntityId original = driver.add(cmd::createLine({0, 0}, {1, 0}));
    driver.document().selection().set({original});
    driver.start("modify.copy");
    // At the second point with none placed, Enter would copy by the base
    // point as a displacement; Esc must not.
    (void)driver.click(5, 5);
    (void)driver.cancel();
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(lineCount(driver), 1u);
}

TEST(LineEscape, EscKeepsTheChainButMoveKeepsNothing)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(10, 0);
    (void)driver.click(10, 10);
    const ToolStep step = driver.cancel();
    EXPECT_EQ(step.message, "2 lines");
    EXPECT_EQ(lineCount(driver), 2u);

    const EntityId first = driver.document().lastCreatedEntities().front();
    const auto before = driver.document().model().entities.find(first)->geometry;
    driver.document().selection().set({first});
    driver.start("modify.move");
    (void)driver.click(3, 3); // base point; Enter now would move by (3, 3)
    (void)driver.cancel();
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(driver.document().model().entities.find(first)->geometry, before);
}

TEST(FilletEscape, EscAtTheRadiusPromptStoresNoRadiusAndKeepsTheCorners)
{
    // The work-keeping tools now say it themselves (keepWorkOnEscape), with
    // the host's old rule: out of a value prompt first, then Enter.
    ToolDriver driver;
    driver.start("modify.fillet");
    EXPECT_EQ(driver.type("R").outcome, kContinue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    const ToolStep step = driver.cancel();
    EXPECT_EQ(step.outcome, kDone);
    EXPECT_EQ(step.command, nullptr);
    EXPECT_EQ(driver.executed(), 0);
}

} // namespace
