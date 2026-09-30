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
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.insert");
    // One click near the segment: the vertex goes ON it, where the click
    // projects (a pick of the polyline came first before 2026-09-30).
    driver.click(4, 0.1);
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
    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.move");
    driver.pick(id, 5, 0.2);
    driver.type("@0,3");
    EXPECT_EQ(shapeOf(driver, id).vertices[1].position, Point2(5, 3));
}

TEST(VertexTools, EditVerticesSelectsThePolylineForThePanel)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 0)});
    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.weed");
    driver.type("0.1");
    driver.enter(); // <Yes>: keep the survey point's vertex
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 3u);

    selectOnly(driver, {id});
    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.start");
    driver.pick(id, 10, 10);
    EXPECT_EQ(shapeOf(driver, id).vertices.front().position, Point2(10, 10));
}

TEST(VertexTools, HeightsAreSetInterpolatedAndGraded)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)},
                                    false, {100.0, std::nullopt, 110.0});
    driver.setPickTolerance(0.5);
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
    second.setPickTolerance(0.5);
    second.start("draw.vertex.interpolate");
    second.enter();
    EXPECT_NEAR(*shapeOf(second, other).vertices[1].height, 105.0, 1e-12);

    ToolDriver third;
    const EntityId graded = addPolyline(third, {Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false,
                                        {100.0, 50.0, 110.0});
    third.setPickTolerance(0.5);
    third.start("draw.vertex.grade");
    third.pick(graded, 0, 0);
    third.click(10, 0);
    EXPECT_NEAR(*shapeOf(third, graded).vertices[1].height, 105.0, 1e-12);
}

TEST(VertexTools, SegmentsBecomeArcsAndLinesAgain)
{
    ToolDriver driver;
    const EntityId id = addPolyline(driver, {Point2(0, 0), Point2(2, 0), Point2(2, 5)});
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.arc");
    driver.pick(id, 1, 0);
    driver.click(1, -1);
    ASSERT_TRUE(std::holds_alternative<CurvePolyline2>(
        driver.document().model().entities.find(id)->geometry))
        << "a polyline with an arc is a curve polyline";
    EXPECT_NEAR(shapeOf(driver, id).vertices[0].bulge, 1.0, 1e-12);

    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.fillet");
    driver.pick(id, 10, 0);
    driver.type("2");
    ASSERT_EQ(shapeOf(driver, id).vertices.size(), 4u);
    EXPECT_EQ(shapeOf(driver, id).vertices[1].position, Point2(8, 0));

    ToolDriver second;
    const EntityId other = addPolyline(second, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    second.setPickTolerance(0.5);
    second.start("draw.vertex.chamfer");
    second.pick(other, 10, 0);
    second.type("2");
    second.type("3");
    EXPECT_EQ(shapeOf(second, other).vertices[2].position, Point2(10, 3));
    // A radius that does not fit is refused and the tool asks again.
    ToolDriver third;
    const EntityId small = addPolyline(third, {Point2(0, 0), Point2(1, 0), Point2(1, 1)});
    third.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.merge");
    driver.enter(); // 0.001
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 3u);
    selectOnly(driver, {id});
    driver.setPickTolerance(0.5);
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
    driver.setPickTolerance(0.5);
    driver.start("draw.vertex.delete");
    driver.pick(id, 5, 0);
    EXPECT_EQ(driver.document().history().undoCount(), before + 1);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(shapeOf(driver, id).vertices.size(), 3u);
}

TEST(VertexTools, ExplodingACurvePolylineGivesLinesAndArcs)
{
    ToolDriver driver;
    katana::entity::Entity entity;
    CurvePolyline2 shape = CurvePolyline2::fromPoints({Point2(0, 0), Point2(2, 0), Point2(2, 5)});
    shape.vertices[0].bulge = 1.0;
    shape.vertices[0].height = 10.0;
    shape.vertices[1].height = 11.0;
    entity.geometry = shape;
    const EntityId id = driver.add(katana::commands::createEntities({entity}));
    selectOnly(driver, {id});
    driver.start("modify.explode");
    driver.enter();
    std::size_t arcs = 0;
    std::size_t lines = 0;
    driver.document().model().entities.forEach([&](const katana::entity::Entity& e) {
        if (const auto* arc = std::get_if<katana::geometry::Arc2>(&e.geometry)) {
            ++arcs;
            EXPECT_NEAR(arc->radius, 1.0, 1e-12);
            const auto heights = katana::entity::heightsOf(e.properties, 2);
            EXPECT_EQ(heights[0], 10.0);
            EXPECT_EQ(heights[1], 11.0);
        } else if (std::holds_alternative<katana::geometry::Segment2>(e.geometry)) {
            ++lines;
        }
    });
    EXPECT_EQ(arcs, 1u);
    EXPECT_EQ(lines, 1u);
}

TEST(VertexTools, OffsettingACurvePolylineMakesConcentricArcs)
{
    // A straight 10 m then a quarter circle of radius 5 turning left:
    // offset 1 to the left (inside the turn) gives radius 4.
    ToolDriver driver;
    katana::entity::Entity entity;
    CurvePolyline2 shape =
        CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0), Point2(15, 5)});
    shape.vertices[1].bulge = std::tan(std::atan(1.0) / 2.0); // a quarter turn, counter-clockwise
    entity.geometry = shape;
    const EntityId id = driver.add(katana::commands::createEntities({entity}));
    driver.start("modify.offset");
    driver.type("1");
    driver.pick(id, 5, 0);
    driver.click(5, 3);
    driver.enter();
    const auto made = driver.document().lastCreatedEntities();
    ASSERT_EQ(made.size(), 1u);
    const auto moved =
        std::get<CurvePolyline2>(driver.document().model().entities.find(made[0])->geometry);
    ASSERT_EQ(moved.vertices.size(), 3u);
    EXPECT_NEAR(moved.vertices[0].position.y, 1.0, 1e-9);
    const auto arc = std::get<katana::geometry::Arc2>(moved.segment(1));
    EXPECT_NEAR(arc.radius, 4.0, 1e-9);
    EXPECT_NEAR(arc.center.distanceTo(Point2(10, 5)), 0.0, 1e-9);
}

TEST(VertexTools, BreakingACurvePolylineKeepsItsArcsCircle)
{
    // A straight 10 m then a quarter circle of radius 5 about (10,5): broken
    // from (5,0) to the arc's middle, the piece kept past the gap is the
    // arc's second half, still on its circle, and the first piece a straight
    // polyline (stored as one) with the heights it had.
    ToolDriver driver;
    katana::entity::Entity entity;
    CurvePolyline2 shape = CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0), Point2(15, 5)});
    shape.vertices[1].bulge = std::tan(katana::math::kPi / 8.0);
    shape.vertices[0].height = 100.0;
    shape.vertices[1].height = 110.0;
    shape.vertices[2].height = 120.0;
    entity.geometry = shape;
    const EntityId id = driver.add(katana::commands::createEntities({entity}));
    const Point2 middle = Point2(10, 5) + katana::geometry::Vec2(5, 0).rotated(-katana::math::kPi / 4.0);
    driver.start("modify.break");
    ASSERT_EQ(driver.pick(id, 5, 0).outcome, katana::cad::ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.click(middle.x, middle.y).outcome, katana::cad::ToolStep::Outcome::Done);
    const auto* first = driver.document().model().entities.find(id);
    ASSERT_NE(first, nullptr);
    ASSERT_EQ(first->type(), katana::entity::EntityType::Polyline);
    const auto firstShape = readPolyline(*first);
    ASSERT_TRUE(firstShape.has_value());
    EXPECT_EQ(firstShape->vertices.back().position, Point2(5, 0));
    EXPECT_DOUBLE_EQ(*firstShape->vertices.back().height, 105.0);
    const auto made = driver.document().lastCreatedEntities();
    ASSERT_EQ(made.size(), 1u);
    const auto second =
        std::get<CurvePolyline2>(driver.document().model().entities.find(made[0])->geometry);
    ASSERT_EQ(second.vertices.size(), 2u);
    const auto arc = std::get<katana::geometry::Arc2>(second.segment(0));
    EXPECT_NEAR(arc.radius, 5.0, 1e-9);
    EXPECT_NEAR(arc.center.distanceTo(Point2(10, 5)), 0.0, 1e-9);
    EXPECT_NEAR(std::abs(arc.sweep), katana::math::kPi / 4.0, 1e-9);
    EXPECT_NEAR(*second.vertices[0].height, 115.0, 1e-9) << "halfway along the arc";
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(driver.document().model().entities.size(), 1u);
}

TEST(VertexTools, TrimmingACurvePolylineAtALineCrossingItsArc)
{
    // The same path, cut by the line x = 10 + 5 sin 45 through the arc; the
    // pick on the arc's far end trims that end away.
    ToolDriver driver;
    katana::entity::Entity entity;
    CurvePolyline2 shape = CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0), Point2(15, 5)});
    shape.vertices[1].bulge = std::tan(katana::math::kPi / 8.0);
    entity.geometry = shape;
    const EntityId id = driver.add(katana::commands::createEntities({entity}));
    const double x = 10.0 + 5.0 * std::sin(katana::math::kPi / 4.0);
    const EntityId cutter = driver.add(katana::commands::createLine(
        Point2(x, -10), Point2(x, 10), driver.document().currentAttributes()));
    driver.start("modify.trim");
    driver.pick(cutter, x, 0);
    driver.enter();
    ASSERT_EQ(driver.pick(id, 14.9, 4).outcome, katana::cad::ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.enter().outcome, katana::cad::ToolStep::Outcome::Done);
    const auto kept = std::get<CurvePolyline2>(driver.document().model().entities.find(id)->geometry);
    ASSERT_EQ(kept.vertices.size(), 3u);
    EXPECT_NEAR(kept.vertices.back().position.x, x, 1e-9);
    const auto arc = std::get<katana::geometry::Arc2>(kept.segment(1));
    EXPECT_NEAR(arc.radius, 5.0, 1e-9);
    EXPECT_NEAR(kept.length(), 10.0 + 5.0 * katana::math::kPi / 4.0, 1e-9);
}

TEST(VertexTools, ACurvePolylineIsACuttingEdgeForTrim)
{
    ToolDriver driver;
    katana::entity::Entity entity;
    CurvePolyline2 shape = CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0)});
    shape.vertices[0].bulge = 1.0; // a semicircle below the chord, centre (5,0), radius 5
    entity.geometry = shape;
    const EntityId edge = driver.add(katana::commands::createEntities({entity}));
    const EntityId target = driver.add(katana::commands::createLine(
        Point2(5, 0), Point2(5, -10), driver.document().currentAttributes()));
    driver.start("modify.trim");
    driver.pick(edge, 5, -5);
    driver.enter();
    driver.pick(target, 5, -8);
    ASSERT_EQ(driver.enter().outcome, katana::cad::ToolStep::Outcome::Done);
    const auto line =
        std::get<katana::geometry::Segment2>(driver.document().model().entities.find(target)->geometry);
    EXPECT_NEAR(line.end.y, -5.0, 1e-9) << "cut where it crosses the arc";
}
