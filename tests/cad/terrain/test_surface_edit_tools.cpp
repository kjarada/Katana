// Terrain > Edit Surface's tools (src/katana_cad/tools/terrain_edit.cpp,
// docs/surface_editing.md), driven as the plan view drives them: each click
// or pick becomes the SURFACE EDIT line a person would type, on the active
// surface, as one undo step.
#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "../tools/tool_driver.hpp"
#include "katana/cad/command_interpreter.hpp"
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

    // Delete Triangles Inside's Outside choice is held by its catalogue entry,
    // which these tests share, so each says what it wants rather than toggling.
    bool outside() { return driver.tool().prompt().find("OUTSIDE it") != std::string::npos; }
    void setOutside(bool wanted)
    {
        if (outside() != wanted) {
            driver.type("O");
        }
        ASSERT_EQ(outside(), wanted);
    }
};

} // namespace

TEST_F(SurfaceEditTools, EveryToolIsInTheTerrainCategoryOfTheCatalogue)
{
    for (const char* id : {"terrain.edit.deletetriangle", "terrain.edit.flip",
                           "terrain.edit.addpoint", "terrain.edit.deletepoint",
                           "terrain.edit.movepoint", "terrain.edit.deleteinside",
                           "terrain.edit.breakline", "terrain.edit.boundary",
                           "terrain.edit.hole"}) {
        const ToolInfo* info = toolCatalog().find(id);
        ASSERT_NE(info, nullptr) << id;
        EXPECT_EQ(info->category, "Terrain") << id;
        EXPECT_EQ(info->group, "Surface Edit") << id;
        EXPECT_FALSE(info->aliases.empty()) << id;
    }
    EXPECT_TRUE(toolCatalogProblems().empty());
    EXPECT_NE(toolCatalog().findByAlias("tinflip"), nullptr);
    EXPECT_NE(toolCatalog().findByAlias("tinmovpt"), nullptr);
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
    setOutside(true);
    driver.pick(strip, 1, 5);
    EXPECT_NEAR(surfaceOf(driver.document(), "ground").planArea(), 8.0, 1e-9);
    setOutside(false);
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

TEST_F(SurfaceEditTools, TheOutsideChoiceOfDeleteTrianglesInsideSurvivesTheRestartAfterAnEdit)
{
    // A run of picks is a run of clicks: each edit ends the tool and the host
    // makes it again, and the Outside the person chose went with the old one,
    // so the second polygon's INSIDE was deleted instead.
    putSurface(driver.document(), "ground", flatGridSurface(10, 1.0, 4.0));
    const EntityId first = polyline({Point2(1, 1), Point2(9, 1), Point2(9, 9), Point2(1, 9)}, true);
    const EntityId second = polyline({Point2(2, 2), Point2(8, 2), Point2(8, 8), Point2(2, 8)}, true);
    driver.start("terrain.edit.deleteinside");
    setOutside(true);
    // Outside [1,9]^2 goes: 64 stays, in 128 triangles.
    driver.pick(first, 1, 5);
    EXPECT_NEAR(surfaceOf(driver.document(), "ground").planArea(), 64.0, 1e-9);
    EXPECT_TRUE(outside()) << "the restarted tool still says OUTSIDE: " << driver.tool().prompt();
    // And the second pick deletes outside [2,8]^2 of what is left: 36 stays.
    driver.pick(second, 2, 5);
    EXPECT_NEAR(surfaceOf(driver.document(), "ground").planArea(), 36.0, 1e-9);
    ASSERT_FALSE(driver.messages().empty());
    EXPECT_NE(driver.messages().back().find("DELETE TRIANGLES OUTSIDE #"), std::string::npos)
        << driver.messages().back();
    setOutside(false);
    EXPECT_FALSE(outside());
}

TEST_F(SurfaceEditTools, DeletePointOffersNoVertexOnABreaklineOrBoundaryEdgeThatTheClickRefuses)
{
    // The square with its diagonal 0-2 a breakline (triangle 0 runs it as
    // edge 2, triangle 1 as edge 0). Vertex 0 at (0,0) is on it; vertex 1 at
    // (10,0) is on the rim only, which Delete Point may take.
    putSurface(driver.document(), "ground",
               makeSurface({katana::geometry::Point3(0, 0, 0), katana::geometry::Point3(10, 0, 0),
                            katana::geometry::Point3(10, 10, 0), katana::geometry::Point3(0, 10, 0)},
                           {{0, 1, 2}, {0, 2, 3}}, {std::uint8_t{4}, std::uint8_t{1}}));
    driver.setPickTolerance(0.5);
    driver.start("terrain.edit.deletepoint");
    const ToolFeedback kept = driver.preview(0.1, 0.1);
    EXPECT_TRUE(kept.refused);
    EXPECT_NE(kept.caption.find("kept"), std::string::npos) << kept.caption;
    const ToolFeedback free = driver.preview(9.9, 0.1);
    EXPECT_FALSE(free.refused) << free.caption;
    EXPECT_EQ(free.caption, "delete this vertex");
    // What the preview says is what the click does.
    EXPECT_TRUE(rejected(driver.click(0.1, 0.1)));
    EXPECT_FALSE(rejected(driver.click(9.9, 0.1)));
}

TEST_F(SurfaceEditTools, ACurvePolylineDrawnBackToItsStartIsAPolygonToThePickAsToTheVerb)
{
    // PLINE with an arc, ending where it began, is a curve polyline with the
    // closed flag off - which the verb takes as a polygon (the one reader of
    // the drawing's shapes) and the tool's own copy of the rule refused.
    putSurface(driver.document(), "ground", flatGridSurface(10, 10.0, 4.0));
    CommandInterpreter interpreter(driver.document());
    ASSERT_TRUE(interpreter.run("PLINE 20,20 60,20 ARC 60,60 LINE 20,60 20,20").ok());
    const EntityId curve = driver.document().lastCreatedEntities().front();
    driver.start("terrain.edit.hole");
    const ToolStep step = driver.pick(curve, 20, 40);
    EXPECT_FALSE(rejected(step)) << step.message;
    EXPECT_EQ(driver.executed(), 1);
    // The square 20..60 is 1600 and the arc bulges by a little; the hole is
    // between that and the 3600 of the ground's cells it crosses.
    EXPECT_LT(surfaceOf(driver.document(), "ground").planArea(), 10000.0 - 1500.0);
    // A line is still no polygon.
    const EntityId open = polyline({Point2(1, 1), Point2(3, 1), Point2(3, 3)}, false);
    EXPECT_TRUE(rejected(driver.pick(open, 2, 1)));
}

TEST_F(SurfaceEditTools, TheSurfaceOptionTellsTheToolbarAndOnlyWhileItIsListening)
{
    putSurface(driver.document(), "ground", flatGridSurface(4, 1.0));
    putSurface(driver.document(), "pad", flatGridSurface(4, 1.0, 2.0));
    int told = 0;
    const std::size_t handle = addSurfaceEditTargetObserver([&told] { ++told; });
    driver.start("terrain.edit.deletetriangle");
    driver.type("S");
    EXPECT_EQ(told, 0) << "choosing the option is not choosing the surface";
    driver.type("pad");
    EXPECT_EQ(told, 1);
    EXPECT_EQ(surfaceEditTarget(), "pad");
    removeSurfaceEditTargetObserver(handle);
    setSurfaceEditTarget("ground");
    EXPECT_EQ(told, 1) << "a removed observer is not called";
}

TEST_F(SurfaceEditTools, MovePointTakesAVertexThenATargetAndRunsTheVerbAsOneUndoStep)
{
    // A pick tolerance of 0.5 gives a vertex reach of 0.75: the click at
    // (2.1, 2.1) is 0.14 from the vertex (2,2) of the 4 x 4 grid.
    putSurface(driver.document(), "ground", flatGridSurface(4, 1.0));
    const TinSurface before = surfaceOf(driver.document(), "ground");
    driver.setPickTolerance(0.5);
    driver.start("terrain.edit.movepoint");
    EXPECT_NE(driver.tool().prompt().find("to move"), std::string::npos) << driver.tool().prompt();
    EXPECT_TRUE(rejected(driver.click(10, 10))) << "no vertex within reach of a point off the surface";
    const ToolStep first = driver.click(2.1, 2.1);
    ASSERT_EQ(first.outcome, ToolStep::Outcome::Continue) << first.message;
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_NE(driver.tool().prompt().find("goes"), std::string::npos) << driver.tool().prompt();
    ASSERT_TRUE(driver.tool().lastPoint().has_value());
    EXPECT_EQ(*driver.tool().lastPoint(), Point2(2, 2)) << "the vertex itself, not the click";
    // A target the verb refuses leaves the vertex held for another click.
    EXPECT_TRUE(rejected(driver.click(9, 9)));
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);

    const ToolStep second = driver.click(3.5, 2.2);
    ASSERT_EQ(second.outcome, ToolStep::Outcome::Done) << second.message;
    EXPECT_EQ(driver.executed(), 1);
    ASSERT_FALSE(driver.messages().empty());
    EXPECT_TRUE(driver.messages().back().starts_with(
        "SURFACE EDIT ground MOVE POINT 2,2 TO 3.5,2.2\n"))
        << driver.messages().back();
    const TinSurface moved = surfaceOf(driver.document(), "ground");
    EXPECT_NEAR(moved.planArea(), 16.0, 1e-9);
    EXPECT_EQ(moved.vertexCount(), 25u);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(surfaceOf(driver.document(), "ground"), before);
    EXPECT_FALSE(driver.finished()) << "it starts again for the next vertex";
}

TEST_F(SurfaceEditTools, MovePointRefusesAHeldVertexAtTheFirstClickAndTheUndoLetsGoOfOne)
{
    // The row y = 4 from x = 1 to 7 is a breakline; the corner (0,0) is on the
    // rim. Neither is picked up. A free vertex is, and Undo lets go of it.
    putSurface(driver.document(), "ground", flatGridSurface(10, 1.0));
    CommandInterpreter interpreter(driver.document());
    ASSERT_TRUE(interpreter.run("SURFACE EDIT ground BREAKLINE 1,4 7,4").ok());
    driver.setPickTolerance(0.5);
    driver.start("terrain.edit.movepoint");
    const ToolStep onLine = driver.click(4.1, 4.1);
    ASSERT_TRUE(rejected(onLine));
    EXPECT_NE(onLine.message.find("kept"), std::string::npos) << onLine.message;
    EXPECT_TRUE(rejected(driver.click(0.1, 0.1)));
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_NE(driver.tool().prompt().find("to move"), std::string::npos) << "still at the first click";

    ASSERT_EQ(driver.click(6.1, 6.1).outcome, ToolStep::Outcome::Continue);
    EXPECT_NE(driver.tool().prompt().find("goes"), std::string::npos);
    EXPECT_EQ(driver.undo().outcome, ToolStep::Outcome::Continue);
    EXPECT_NE(driver.tool().prompt().find("to move"), std::string::npos) << driver.tool().prompt();
    EXPECT_FALSE(driver.tool().lastPoint().has_value());
    EXPECT_EQ(driver.executed(), 0) << "the breakline was added by the test, and no move was made";
}

TEST_F(SurfaceEditTools, ThePreviewOfMovePointMarksTheVertexThenWhereItWouldGoAndWhatItRefuses)
{
    putSurface(driver.document(), "ground", flatGridSurface(4, 1.0));
    driver.setPickTolerance(0.5);
    driver.start("terrain.edit.movepoint");
    // Over a free vertex: marked as the one to move. Over a rim vertex: refused, saying it is kept.
    const ToolFeedback free = driver.preview(2.05, 2.05);
    EXPECT_FALSE(free.refused) << free.caption;
    ASSERT_EQ(free.marks.size(), 1u);
    EXPECT_EQ(free.marks.front().role, FeedbackRole::Target);
    EXPECT_EQ(free.caption, "move this vertex");
    const ToolFeedback rim = driver.preview(0.05, 0.05);
    EXPECT_TRUE(rim.refused);
    EXPECT_NE(rim.caption.find("kept"), std::string::npos) << rim.caption;
    EXPECT_TRUE(driver.preview(9, 9).refused);

    // After the first click: the vertex where it is and where it would go; off the surface, refused.
    ASSERT_EQ(driver.click(2.05, 2.05).outcome, ToolStep::Outcome::Continue);
    const ToolFeedback target = driver.preview(3.5, 2.2);
    EXPECT_FALSE(target.refused) << target.caption;
    ASSERT_EQ(target.marks.size(), 2u);
    EXPECT_EQ(target.marks[0].role, FeedbackRole::Removed);
    EXPECT_EQ(target.marks[1].role, FeedbackRole::Added);
    EXPECT_TRUE(driver.preview(9, 9).refused);
}
