// The preset leader tools (annotate_leader_presets.cpp; docs/annotation.md,
// "Presets"; docs/tools.md): Level, Surface Difference, Cut/Fill, Point
// Name, Code and Station Leader, driven through ToolDriver as the plan view
// drives them, each checked against the LEADER line that makes the same
// leader.
//
// The surface is the plane z = 10 + 0.1 x + 0.2 y over 0..100 x 0..100 (two
// triangles; a TIN interpolates a plane exactly): 21 at (30, 40).

#include <cmath>
#include <memory>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "tool_driver.hpp"

namespace {

using katana::cad::Document;
using katana::cad::testing::ToolDriver;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using katana::geometry::Point3;
using Outcome = katana::cad::ToolStep::Outcome;
namespace ann = katana::cad::annotation;
namespace cmd = katana::commands;
namespace terrain = katana::terrain;

std::shared_ptr<const terrain::TinSurface> plane(double z0)
{
    const auto z = [&](double x, double y) { return z0 + 0.1 * x + 0.2 * y; };
    auto made = terrain::TinSurface::create(
        {Point3(0, 0, z(0, 0)), Point3(100, 0, z(100, 0)), Point3(100, 100, z(100, 100)),
         Point3(0, 100, z(0, 100))},
        {terrain::TinTriangle{0, 1, 2}, terrain::TinTriangle{0, 2, 3}});
    EXPECT_TRUE(made.ok()) << made.error().describe();
    return std::make_shared<const terrain::TinSurface>(std::move(*made));
}

// "ground" (z0 = 10) and "design" (z0 = 12) in `document`.
void addSurfaces(Document& document)
{
    terrain::SurfaceStore store;
    for (const auto& [name, z0] : {std::pair{"ground", 10.0}, std::pair{"design", 12.0}}) {
        terrain::NamedSurface entry;
        entry.name = name;
        entry.surface = plane(z0);
        entry.source = "test plane";
        EXPECT_TRUE(store.add(std::move(entry)).ok());
    }
    EXPECT_TRUE(document.setSurfaces(std::move(store), "TEST_SURFACES").ok());
}

EntityId addPoint(Document& document, Point2 at, std::optional<double> z)
{
    Entity point;
    point.geometry = katana::entity::PointGeometry{at};
    if (z) {
        point.properties["elevation"] = *z;
    }
    EXPECT_TRUE(document.execute(cmd::createEntities({std::move(point)})).ok());
    return document.lastCreatedEntities().front();
}

std::vector<const Entity*> leadersOf(const Document& document)
{
    std::vector<const Entity*> found;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<LeaderGeometry>(entity.geometry)) {
            found.push_back(&entity);
        }
    });
    return found;
}

std::string says(const Document& document, const Entity& leader)
{
    return ann::leaderSays(document.model(), std::get<LeaderGeometry>(leader.geometry),
                           ann::leaderContext(document));
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(LeaderPresetTools, EachPresetHasAToolOfItsOwnOnTheAnnotateMenuWithItsVerb)
{
    const std::pair<const char*, const char*> tools[] = {
        {"annotate.leader.level", "LEADERLEVEL"},     {"annotate.leader.surface", "LEADERSURFACE"},
        {"annotate.leader.cutfill", "LEADERCUTFILL"}, {"annotate.leader.name", "LEADERNAME"},
        {"annotate.leader.code", "LEADERCODE"},       {"annotate.leader.station", "LEADERSTATION"}};
    for (const auto& [id, verb] : tools) {
        const katana::cad::ToolInfo* info = katana::cad::toolCatalog().find(id);
        ASSERT_NE(info, nullptr) << id;
        EXPECT_EQ(info->category, "Annotate") << id;
        EXPECT_EQ(info->group, "Leaders") << id;
        EXPECT_EQ(katana::cad::toolCatalog().findByAlias(verb), info) << verb;
        EXPECT_EQ(info->name.find(','), std::string::npos)
            << id << ": not a family, so each is a button of its own";
    }
    // And every preset with a tool names it.
    for (const ann::LeaderPresetInfo& preset : ann::leaderPresets()) {
        if (!preset.tool.empty()) {
            EXPECT_NE(katana::cad::toolCatalog().find(preset.tool), nullptr) << preset.word;
        }
    }
    EXPECT_TRUE(katana::cad::toolCatalogProblems().empty());
}

TEST(LeaderPresetTools, TheLevelLeaderToolMakesWhatTheLeaderLineMakes)
{
    ToolDriver driver;
    const EntityId pit = addPoint(driver.document(), Point2(30, 40), 25.0);
    driver.start("annotate.leader.level");
    const auto tip = driver.click(30, 40);
    ASSERT_EQ(tip.outcome, Outcome::Continue) << tip.message;
    EXPECT_TRUE(contains(tip.message, "Level leader on Point " + std::to_string(pit) +
                                          ": RL 25.000"))
        << tip.message;
    const auto done = driver.click(40, 50);
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_EQ(driver.executed(), 1);

    // The same answers typed.
    Document typed;
    katana::cad::CommandInterpreter interpreter(typed);
    (void)addPoint(typed, Point2(30, 40), 25.0);
    ASSERT_TRUE(interpreter.run("LEADER #1@30,40 40,50 preset=level").ok());

    const auto made = leadersOf(driver.document());
    const auto line = leadersOf(typed);
    ASSERT_EQ(made.size(), 1U);
    ASSERT_EQ(line.size(), 1U);
    EXPECT_EQ(std::get<LeaderGeometry>(made.front()->geometry),
              std::get<LeaderGeometry>(line.front()->geometry));
    EXPECT_EQ(made.front()->layer, line.front()->layer);
    EXPECT_EQ(says(driver.document(), *made.front()), "RL 25.000");
}

TEST(LeaderPresetTools, TheSurfaceToolReadsTheSurfaceChosenWithSAndSaysTheDifference)
{
    ToolDriver driver;
    addSurfaces(driver.document());
    (void)addPoint(driver.document(), Point2(30, 40), 25.0);
    driver.start("annotate.leader.surface");
    EXPECT_TRUE(contains(driver.tool().prompt(), "[Surface]")) << driver.tool().prompt();
    EXPECT_TRUE(contains(driver.tool().prompt(), "the first visible surface"))
        << driver.tool().prompt();
    ASSERT_EQ(driver.type("S").outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("nope").outcome, Outcome::Rejected) << "no such surface";
    ASSERT_EQ(driver.type("DESIGN").outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "surface: design")) << driver.tool().prompt();
    (void)driver.click(30, 40);
    ASSERT_EQ(driver.click(40, 50).outcome, Outcome::Done);
    // design is 12 + 3 + 8 = 23 at (30, 40): RL 25 is 2 above it.
    const auto made = leadersOf(driver.document());
    ASSERT_EQ(made.size(), 1U);
    EXPECT_EQ(std::get<LeaderGeometry>(made.front()->geometry).text, "{dsurface.design:+.3f}");
    EXPECT_EQ(says(driver.document(), *made.front()), "+2.000");
}

TEST(LeaderPresetTools, ASurfaceToolOffersTheSurfaceTheNewestLeaderOfItsPresetReads)
{
    ToolDriver driver;
    addSurfaces(driver.document());
    (void)addPoint(driver.document(), Point2(30, 40), 25.0);
    katana::cad::CommandInterpreter interpreter(driver.document());
    ASSERT_TRUE(interpreter.run("LEADER #1 40,50 preset=cutfill surface=design").ok());
    driver.start("annotate.leader.cutfill");
    EXPECT_TRUE(contains(driver.tool().prompt(), "surface: design")) << driver.tool().prompt();
    (void)driver.click(30, 40);
    ASSERT_EQ(driver.click(20, 50).outcome, Outcome::Done);
    // 25 over design's 23: a fill of 2.
    const auto made = leadersOf(driver.document());
    ASSERT_EQ(made.size(), 2U);
    EXPECT_EQ(says(driver.document(), *made.back()), "FILL 2.000");
}

TEST(LeaderPresetTools, APointWithNothingToSayIsRefusedAtTheFirstClickSayingWhy)
{
    ToolDriver driver;
    (void)addPoint(driver.document(), Point2(30, 40), std::nullopt);
    driver.start("annotate.leader.level");
    const auto refused = driver.click(30, 40);
    EXPECT_EQ(refused.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(refused.message, "would say nothing")) << refused.message;
    EXPECT_TRUE(contains(driver.tool().prompt(), "Pick what the note reads"))
        << "still asking for the point";
    // And a click on nothing.
    const auto nothing = driver.click(70, 70);
    EXPECT_EQ(nothing.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(nothing.message, "nothing is under the cursor")) << nothing.message;
    EXPECT_EQ(driver.executed(), 0);
}

TEST(LeaderPresetTools, EnterPutsTheNoteWhereLeaderForPutsIt)
{
    ToolDriver driver;
    (void)addPoint(driver.document(), Point2(30, 40), 25.0);
    driver.start("annotate.leader.level");
    (void)driver.click(30, 40);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    // LeadersForOptions' 10 mm on paper at 45 degrees, at the drawing's
    // scale: 10 x scale / 1000 model units along (cos 45, sin 45).
    const double reach = 10.0 * driver.document().annotationScale() / 1000.0;
    const double step = reach * std::cos(0.25 * katana::math::kPi);
    const auto made = leadersOf(driver.document());
    ASSERT_EQ(made.size(), 1U);
    const auto& vertices = std::get<LeaderGeometry>(made.front()->geometry).vertices;
    ASSERT_EQ(vertices.size(), 2U);
    EXPECT_NEAR(vertices[1].x, 30.0 + step, 1e-9);
    EXPECT_NEAR(vertices[1].y, 40.0 + step, 1e-9);
}

TEST(LeaderPresetTools, TheToolStartsAgainSoARowOfLevelsIsTwoClicksEach)
{
    ToolDriver driver;
    (void)addPoint(driver.document(), Point2(30, 40), 25.0);
    (void)addPoint(driver.document(), Point2(60, 20), 26.25);
    driver.start("annotate.leader.level");
    (void)driver.click(30, 40);
    (void)driver.click(35, 45);
    (void)driver.click(60, 20);
    (void)driver.click(65, 25);
    EXPECT_EQ(driver.executed(), 2) << "one step each";
    EXPECT_FALSE(driver.finished());
    const auto made = leadersOf(driver.document());
    ASSERT_EQ(made.size(), 2U);
    EXPECT_EQ(says(driver.document(), *made[0]), "RL 25.000");
    EXPECT_EQ(says(driver.document(), *made[1]), "RL 26.250");
    // Enter at the first prompt ends it.
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
}

TEST(LeaderPresetTools, NameCodeAndStationToolsSayWhatTheirPresetsSay)
{
    ToolDriver driver;
    Document& document = driver.document();
    katana::cad::CommandInterpreter interpreter(document);
    // MC01 east along the x axis: (30, -4) is at station 30, 4 to the right.
    ASSERT_TRUE(interpreter.run("ALIGN NEW MC01 0,0 100,0").ok());
    Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(30, -4)};
    point.properties["point"] = std::string("1001");
    point.properties["code"] = std::string("TB");
    point.properties["elevation"] = 25.0;
    ASSERT_TRUE(document.execute(cmd::createEntities({point})).ok());

    const std::pair<const char*, const char*> expected[] = {
        {"annotate.leader.name", "1001"},
        {"annotate.leader.code", "TB"},
        {"annotate.leader.station", "CH 0+030.000\nOS -4.000"}};
    for (const auto& [tool, note] : expected) {
        driver.start(tool);
        ASSERT_EQ(driver.click(30, -4).outcome, Outcome::Continue) << tool;
        ASSERT_EQ(driver.click(40, 5).outcome, Outcome::Done) << tool;
        const auto made = leadersOf(document);
        ASSERT_FALSE(made.empty());
        EXPECT_EQ(says(document, *made.back()), note) << tool;
    }
}
