// Terrain > Edit Surface's tools (src/katana_cad/tools/terrain_edit.cpp,
// docs/surface_editing.md), driven as the plan view drives them: each click
// or pick becomes the SURFACE EDIT line a person would type, on the active
// surface, as one undo step.
#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "../tools/tool_driver.hpp"
#include "katana/cad/terrain/surface_edit_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "surface_edit_support.hpp"

using namespace katana::cad;
using namespace katana::cad::testing;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
namespace cmd = katana::commands;

namespace {

struct SurfaceEditTools : ::testing::Test {
    ToolDriver driver;

    void SetUp() override { setSurfaceEditTarget(""); }
    void TearDown() override { setSurfaceEditTarget(""); }

    EntityId polyline(std::vector<Point2> vertices, bool closed)
    {
        Polyline2 line;
        line.vertices = std::move(vertices);
        line.closed = closed;
        return driver.add(cmd::createPolyline(line));
    }

    static bool rejected(const ToolStep& step) { return step.outcome == ToolStep::Outcome::Rejected; }
};

} // namespace

TEST_F(SurfaceEditTools, EveryToolIsInTheTerrainCategoryOfTheCatalogue)
{
    for (const char* id : {"terrain.edit.deletetriangle", "terrain.edit.flip",
                           "terrain.edit.addpoint", "terrain.edit.deletepoint",
                           "terrain.edit.deleteinside", "terrain.edit.breakline",
                           "terrain.edit.boundary", "terrain.edit.hole"}) {
        const ToolInfo* info = toolCatalog().find(id);
        ASSERT_NE(info, nullptr) << id;
        EXPECT_EQ(info->category, "Terrain") << id;
        EXPECT_EQ(info->group, "Surface Edit") << id;
        EXPECT_FALSE(info->aliases.empty()) << id;
    }
    EXPECT_TRUE(toolCatalogProblems().empty());
    EXPECT_NE(toolCatalog().findByAlias("tinflip"), nullptr);
}

TEST_F(SurfaceEditTools, DeleteTriangleRunsTheVerbOnTheActiveSurfaceAsOneUndoStep)
{
    putSurface(driver.document(), "ground", flatGridSurface(4, 1.0));
    driver.start("terrain.edit.deletetriangle");
    const ToolStep step = driver.click(0.6, 0.2);
    ASSERT_EQ(step.outcome, ToolStep::Outcome::Done) << step.message;
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(surfaceOf(driver.document(), "ground").triangleCount(), 31u);
    ASSERT_FALSE(driver.messages().empty());
    EXPECT_TRUE(driver.messages().back().starts_with(
        "SURFACE EDIT ground DELETE TRIANGLE 0.6,0.2\n"))
        << driver.messages().back();
    EXPECT_FALSE(driver.finished()); // it starts again for the next triangle
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(surfaceOf(driver.document(), "ground").triangleCount(), 32u);
}

TEST_F(SurfaceEditTools, FlipEdgeFlipsTheEdgeNearestTheClick)
{
    putSurface(driver.document(), "ground", squareSurface(0, 1, 0, 1));
    driver.start("terrain.edit.flip");
    driver.click(5, 5);
    EXPECT_NEAR(*surfaceOf(driver.document(), "ground").elevationAt(5, 5), 1.0, 1e-12);
}

TEST_F(SurfaceEditTools, AddPointTakesTheTypedLevelTheSurfacesOnEnterOrATypedXyz)
{
    putSurface(driver.document(), "ground", squareSurface());
    driver.start("terrain.edit.addpoint");
    EXPECT_EQ(driver.click(5, 5).outcome, ToolStep::Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    driver.type("10");
    EXPECT_DOUBLE_EQ(*surfaceOf(driver.document(), "ground").elevationAt(5, 5), 10.0);
    EXPECT_EQ(surfaceOf(driver.document(), "ground").vertexCount(), 5u);

    // Enter takes the surface's own level there: a vertex, no change of shape.
    // (2,8) lies on the spoke from (0,10,0) to the new apex (5,5,10), 0.4 of
    // the way up: level 4.
    const double level = *surfaceOf(driver.document(), "ground").elevationAt(2, 8);
    EXPECT_NEAR(level, 4.0, 1e-12);
    driver.click(2, 8);
    driver.enter();
    EXPECT_EQ(surfaceOf(driver.document(), "ground").vertexCount(), 6u);
    EXPECT_NEAR(*surfaceOf(driver.document(), "ground").elevationAt(2, 8), level, 1e-12);

    driver.type("8,2,4");
    EXPECT_DOUBLE_EQ(*surfaceOf(driver.document(), "ground").elevationAt(8, 2), 4.0);
    EXPECT_EQ(driver.executed(), 3);
}

TEST_F(SurfaceEditTools, DeletePointTakesTheVertexWithinTheViewsReach)
{
    // A pick tolerance of 0.5 gives a vertex reach of 0.75 (12 px to 8 px).
    putSurface(driver.document(), "ground", pyramidSurface());
    driver.setPickTolerance(0.5);
    driver.start("terrain.edit.deletepoint");
    EXPECT_TRUE(rejected(driver.click(6, 6))); // 1.41 from the apex
    driver.click(5.3, 5.3);
    EXPECT_EQ(surfaceOf(driver.document(), "ground").triangleCount(), 2u);
}

TEST_F(SurfaceEditTools, HoleBoundaryAndDeleteInsideTakeAPickedClosedPolyline)
{
    putSurface(driver.document(), "ground", flatGridSurface(10, 1.0, 4.0));
    const EntityId hole = polyline({Point2(3, 3), Point2(6, 3), Point2(6, 7), Point2(3, 7)}, true);
    driver.start("terrain.edit.hole");
    driver.pick(hole, 3, 5);
    EXPECT_NEAR(surfaceOf(driver.document(), "ground").planArea(), 88.0, 1e-9);

    const EntityId clip = polyline({Point2(1, 1), Point2(9, 1), Point2(9, 9), Point2(1, 9)}, true);
    driver.start("terrain.edit.boundary");
    driver.pick(clip, 1, 5);
    EXPECT_NEAR(surfaceOf(driver.document(), "ground").planArea(), 64.0 - 12.0, 1e-9);

    // Outside [1,2] x [1,9]: only that strip's 8 x 2 triangles stay, area 8.
    const EntityId strip = polyline({Point2(1, 1), Point2(2, 1), Point2(2, 9), Point2(1, 9)}, true);
    driver.start("terrain.edit.deleteinside");
    driver.type("O");
    driver.pick(strip, 1, 5);
    EXPECT_NEAR(surfaceOf(driver.document(), "ground").planArea(), 8.0, 1e-9);
}

TEST_F(SurfaceEditTools, AddBreaklineTakesAPickedPolylineDraped)
{
    putSurface(driver.document(), "ground", pyramidSurface());
    const EntityId line = polyline({Point2(0, 2.5), Point2(10, 2.5)}, false);
    driver.start("terrain.edit.breakline");
    driver.pick(line, 5, 2.5);
    ASSERT_FALSE(driver.messages().empty());
    EXPECT_NE(driver.messages().back().find("crossings=2"), std::string::npos)
        << driver.messages().back();
    EXPECT_NEAR(*surfaceOf(driver.document(), "ground").elevationAt(2.5, 2.5), 5.0, 1e-12);
}

TEST_F(SurfaceEditTools, APickOfTheWrongKindIsRefused)
{
    putSurface(driver.document(), "ground", flatGridSurface(4, 1.0));
    const EntityId open = polyline({Point2(1, 1), Point2(3, 1), Point2(3, 3)}, false);
    driver.start("terrain.edit.hole");
    EXPECT_TRUE(rejected(driver.pick(open, 2, 1)));
    const EntityId point = driver.add(cmd::createPoint(Point2(2, 2)));
    driver.start("terrain.edit.breakline");
    EXPECT_TRUE(rejected(driver.pick(point, 2, 2)));
    EXPECT_EQ(driver.executed(), 0);
}

TEST_F(SurfaceEditTools, TheSurfaceOptionChoosesTheSurfaceAndTheToolbarKeepsIt)
{
    putSurface(driver.document(), "ground", flatGridSurface(4, 1.0));
    putSurface(driver.document(), "pad", flatGridSurface(4, 1.0, 2.0));
    driver.start("terrain.edit.deletetriangle");
    EXPECT_NE(driver.tool().prompt().find("ground"), std::string::npos);
    driver.type("S");
    EXPECT_TRUE(rejected(driver.type("nowhere")));
    driver.type("pad");
    EXPECT_EQ(surfaceEditTarget(), "pad");
    driver.click(0.6, 0.2);
    EXPECT_EQ(surfaceOf(driver.document(), "pad").triangleCount(), 31u);
    EXPECT_EQ(surfaceOf(driver.document(), "ground").triangleCount(), 32u);
}

TEST_F(SurfaceEditTools, WithNoSurfaceAToolSaysSoAndRefusesEveryClick)
{
    driver.start("terrain.edit.flip");
    EXPECT_NE(driver.tool().prompt().find("no surface"), std::string::npos);
    EXPECT_TRUE(rejected(driver.click(1, 1)));
}

TEST_F(SurfaceEditTools, ThePreviewMarksWhatAClickWouldTakeAndWhatItWouldRefuse)
{
    putSurface(driver.document(), "ground", flatGridSurface(4, 1.0));
    driver.start("terrain.edit.deletetriangle");
    const ToolFeedback over = driver.preview(0.6, 0.2);
    ASSERT_EQ(over.marks.size(), 1u);
    EXPECT_EQ(over.marks.front().role, FeedbackRole::Removed);
    EXPECT_TRUE(driver.preview(9, 9).refused);

    driver.start("terrain.edit.flip");
    EXPECT_TRUE(driver.preview(2.5, 0.01).refused); // nearest the rim along y = 0
    EXPECT_FALSE(driver.preview(2.5, 2.5).refused); // a diagonal inside
}
