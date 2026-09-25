// The Draw > Vertices tools (src/katana_cad/tools/modify_vertex.cpp), each
// driven as a user would - pick the polyline, click or type - against
// results worked out by hand. The arithmetic itself is tested in
// tests/geometry/test_polyline_vertices.cpp; these test that each tool
// collects the right arguments and ends in one undoable command.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/commands/entity_commands.hpp"
#include "tool_driver.hpp"

using katana::cad::readPolyline;
using katana::cad::testing::ToolDriver;
using katana::cad::ToolStep;
using katana::entity::EntityId;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

EntityId addPolyline(ToolDriver& driver, std::vector<Point2> points, bool closed = false,
                     std::vector<std::optional<double>> heights = {})
{
    katana::entity::Entity entity;
    entity.geometry = Polyline2{std::move(points), closed};
    if (!heights.empty()) {
        katana::entity::setHeights(entity.properties, heights);
    }
    return driver.add(katana::commands::createEntities({entity}));
}

CurvePolyline2 shapeOf(ToolDriver& driver, EntityId id)
{
    return *readPolyline(*driver.document().model().entities.find(id));
}

void selectOnly(ToolDriver& driver, std::vector<EntityId> ids)
{
    driver.document().selection().set(std::move(ids));
}

} // namespace

TEST(VertexTools, TheFamilyIsInTheCatalogueUnderDrawVertices)
{
    const auto& catalog = katana::cad::toolCatalog();
    EXPECT_TRUE(katana::cad::toolCatalogProblems().empty());
    int count = 0;
    for (const auto* info : catalog.all()) {
        if (info->group == "Vertices") {
            EXPECT_EQ(info->category, "Draw");
            EXPECT_EQ(info->name.rfind("Vertices, ", 0), 0u) << info->name;
            ++count;
        }
    }
    EXPECT_EQ(count, 18);
    EXPECT_NE(catalog.findByAlias("WEED"), nullptr);
    EXPECT_NE(catalog.findByAlias("vertexz"), nullptr);
}

TEST(VertexTools, InsertVertexAddsAVertexOnTheNearestSegment)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    driver.start("draw.vertex.insert");
    driver.pick(id, 4, 0.1);
    driver.click(4, 0);
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 4u);
    EXPECT_EQ(shapeOf(driver, id).vertices[1].position, Point2(4, 0));
    EXPECT_FALSE(driver.finished()) << "it restarts for the next vertex";
    EXPECT_EQ(driver.document().history().undoName(), "VERTEX_INSERT");
}

TEST(VertexTools, DeleteVertexRemovesTheOneNearestThePick)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 3), Point2(10, 0)});
    driver.start("draw.vertex.delete");
    driver.pick(id, 5.2, 2.9);
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 2u);
    // A second delete would leave one vertex: refused, and said.
    const auto refused = driver.pick(id, 0, 0);
    EXPECT_EQ(refused.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 2u);
}

TEST(VertexTools, MoveVertexTakesARelativePointFromTheVertex)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    driver.start("draw.vertex.move");
    driver.pick(id, 5, 0.2);
    driver.type("@0,3");
    EXPECT_EQ(shapeOf(driver, id).vertices[1].position, Point2(5, 3));
}

TEST(VertexTools, EditVerticesSelectsThePolylineForThePanel)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 0)});
    driver.start("draw.vertex.edit");
    driver.pick(id, 2, 0);
    EXPECT_EQ(driver.executed(), 0) << "no edit, only a selection";
    EXPECT_EQ(driver.document().selection().ids(), std::vector<EntityId>{id});
    EXPECT_TRUE(driver.finished());
}

TEST(VertexTools, StraightenRemovesTheVerticesBetweenTwoPicks)
{
    ToolDriver driver;
    const EntityId id = addPolyline(
        driver, {Point2(0, 0), Point2(1, 1), Point2(2, -1), Point2(3, 1), Point2(4, 0)});
    driver.start("draw.vertex.straighten");
    driver.pick(id, 0, 0);
    driver.click(4, 0);
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 2u);
    EXPECT_EQ(driver.messages().back(), "3 vertices removed");
}

TEST(VertexTools, WeedTakesAToleranceAndCanKeepSurveyPoints)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 0.01), Point2(10, 0)});
    katana::entity::Entity mark;
    mark.geometry = katana::entity::PointGeometry{Point2(5, 0.01)};
    mark.properties["point"] = std::string("7");
    driver.add(katana::commands::createEntities({mark}));

    selectOnly(driver, {id});
    driver.start("draw.vertex.weed");
    driver.type("0.1");
    driver.enter(); // <Yes>: keep the survey point's vertex
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 3u);

    selectOnly(driver, {id});
    driver.start("draw.vertex.weed");
    driver.enter(); // the remembered 0.1
    driver.type("No");
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 2u);
    EXPECT_EQ(driver.document().history().undoName(), "WEED");
}

TEST(VertexTools, DensifySelectsThenTakesTheIntervalAndChordTolerance)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(10, 0)});
    driver.start("draw.vertex.densify");
    EXPECT_EQ(driver.tool().expects(), katana::cad::ToolInput::Selection);
    selectOnly(driver, {id});
    driver.enter();
    driver.type("2.5");
    driver.type("0");
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 5u);
}

TEST(VertexTools, CloseOrOpenTogglesEachPolyline)
{
    ToolDriver driver;
    const EntityId open = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(5, 5)});
    const EntityId shut =
        addPolyline(driver, {Point2(10, 0), Point2(15, 0), Point2(15, 5)}, true);
    selectOnly(driver, {open, shut});
    driver.start("draw.vertex.close");
    // The selection answered the only question; Enter applies it.
    EXPECT_EQ(driver.executed(), 0);
    driver.enter();
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_TRUE(shapeOf(driver, open).closed);
    EXPECT_FALSE(shapeOf(driver, shut).closed);
}

TEST(VertexTools, ChangeStartVertexRenumbersAClosedPolyline)
{
    ToolDriver driver;
    const EntityId id =
        addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true);
    driver.start("draw.vertex.start");
    driver.pick(id, 10, 10);
    EXPECT_EQ(shapeOf(driver, id).vertices.front().position, Point2(10, 10));
}

TEST(VertexTools, HeightsAreSetInterpolatedAndGraded)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)},
                                    false, {100.0, std::nullopt, 110.0});
    driver.start("draw.vertex.height");
    driver.pick(id, 0, 0);
    driver.type("101");
    EXPECT_EQ(shapeOf(driver, id).vertices[0].height, 101.0);
    driver.pick(id, 10, 0);
    driver.type("None");
    EXPECT_FALSE(shapeOf(driver, id).vertices[2].height.has_value());

    ToolDriver second;
    const EntityId other = addPolyline(second, {Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false,
                                       {100.0, std::nullopt, 110.0});
    selectOnly(second, {other});
    second.start("draw.vertex.interpolate");
    second.enter();
    EXPECT_NEAR(*shapeOf(second, other).vertices[1].height, 105.0, 1e-12);

    ToolDriver third;
    const EntityId graded = addPolyline(third, {Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false,
                                        {100.0, 50.0, 110.0});
    third.start("draw.vertex.grade");
    third.pick(graded, 0, 0);
    third.click(10, 0);
    EXPECT_NEAR(*shapeOf(third, graded).vertices[1].height, 105.0, 1e-12);
}

TEST(VertexTools, SegmentsBecomeArcsAndLinesAgain)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(2, 0), Point2(2, 5)});
    driver.start("draw.vertex.arc");
    driver.pick(id, 1, 0);
    driver.click(1, -1);
    ASSERT_TRUE(std::holds_alternative<CurvePolyline2>(
        driver.document().model().entities.find(id)->geometry))
        << "a polyline with an arc is a curve polyline";
    EXPECT_NEAR(shapeOf(driver, id).vertices[0].bulge, 1.0, 1e-12);

    driver.start("draw.vertex.line");
    driver.pick(id, 1, -1);
    EXPECT_TRUE(std::holds_alternative<Polyline2>(
        driver.document().model().entities.find(id)->geometry))
        << "straight again, it is a Polyline2 again";
}

TEST(VertexTools, FilletAndChamferOneCorner)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    driver.start("draw.vertex.fillet");
    driver.pick(id, 10, 0);
    driver.type("2");
    ASSERT_EQ(shapeOf(driver, id).vertices.size(), 4u);
    EXPECT_EQ(shapeOf(driver, id).vertices[1].position, Point2(8, 0));

    ToolDriver second;
    const EntityId other = addPolyline(second, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    second.start("draw.vertex.chamfer");
    second.pick(other, 10, 0);
    second.type("2");
    second.type("3");
    EXPECT_EQ(shapeOf(second, other).vertices[2].position, Point2(10, 3));
    // A radius that does not fit is refused and the tool asks again.
    ToolDriver third;
    const EntityId small = addPolyline(third, {Point2(0, 0), Point2(1, 0), Point2(1, 1)});
    third.start("draw.vertex.fillet");
    third.pick(small, 1, 0);
    EXPECT_EQ(third.type("50").outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(third.tool().expects(), katana::cad::ToolInput::Value);
}

TEST(VertexTools, MergeAndSnapToGrid)
{
    ToolDriver driver;
    const EntityId id =
        addPolyline(driver, {Point2(0, 0), Point2(0.0005, 0), Point2(5.2, 0.3), Point2(9.9, 0.1)});
    selectOnly(driver, {id});
    driver.start("draw.vertex.merge");
    driver.enter(); // 0.001
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 3u);
    selectOnly(driver, {id});
    driver.start("draw.vertex.grid");
    driver.type("1");
    EXPECT_EQ(shapeOf(driver, id).vertices[1].position, Point2(5, 0));
    EXPECT_EQ(shapeOf(driver, id).vertices[2].position, Point2(10, 0));
}

TEST(VertexTools, EachEditIsOneUndoStep)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    const std::size_t before = driver.document().history().undoCount();
    driver.start("draw.vertex.delete");
    driver.pick(id, 5, 0);
    EXPECT_EQ(driver.document().history().undoCount(), before + 1);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 3u);
}
