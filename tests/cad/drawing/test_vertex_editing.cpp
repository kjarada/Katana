// Vertex editing of polyline entities (include/katana/cad/drawing/
// vertex_editing.hpp): which kind an edited polyline is stored as, where
// its heights go, and that each edit is one undo step.

#include <gtest/gtest.h>

#include "katana/cad/document.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/commands/entity_commands.hpp"

using namespace katana::cad;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

EntityId add(Document& document, Entity entity)
{
    EXPECT_TRUE(document.execute(katana::commands::createEntities({std::move(entity)})).ok());
    return document.lastCreatedEntities().front();
}

Entity polylineEntity(std::vector<Point2> points, std::vector<std::optional<double>> heights = {})
{
    Entity entity;
    entity.geometry = Polyline2{std::move(points), false};
    if (!heights.empty()) {
        katana::entity::setHeights(entity.properties, heights);
    }
    return entity;
}

} // namespace

TEST(VertexEditing, AStraightPolylineReadsWithItsPropertyHeights)
{
    const Entity e = polylineEntity({Point2(0, 0), Point2(5, 0)}, {10.0, std::nullopt});
    const auto read = readPolyline(e);
    ASSERT_TRUE(read.has_value());
    EXPECT_EQ(read->vertices[0].height, 10.0);
    EXPECT_FALSE(read->vertices[1].height.has_value());
    EXPECT_FALSE(readPolyline(Entity{}).has_value()) << "a point is no polyline";
}

TEST(VertexEditing, AnEditIsStoredAsTheSimplestKind)
{
    const Entity e = polylineEntity({Point2(0, 0), Point2(5, 0), Point2(5, 5)}, {1.0, 2.0, 3.0});
    auto polyline = *readPolyline(e);
    // An arc makes it a curve polyline, heights in the geometry and the
    // properties cleared.
    polyline.vertices[0].bulge = 0.4;
    const auto curved = writePolyline(e, polyline);
    ASSERT_TRUE(curved.ok());
    ASSERT_TRUE(std::holds_alternative<CurvePolyline2>(curved->geometry));
    EXPECT_EQ(std::get<CurvePolyline2>(curved->geometry).vertices[2].height, 3.0);
    EXPECT_FALSE(curved->properties.contains(std::string(katana::entity::kElevationsProperty)));
    // Straightened again, it goes back to a Polyline2 with its heights in the
    // properties, as every other 3D string is held.
    polyline.vertices[0].bulge = 0.0;
    const auto straight = writePolyline(*curved, polyline);
    ASSERT_TRUE(straight.ok());
    ASSERT_TRUE(std::holds_alternative<Polyline2>(straight->geometry));
    EXPECT_EQ(katana::entity::heightsOf(straight->properties, 3)[1], 2.0);
}

TEST(VertexEditing, EachEditIsOneUndoStep)
{
    Document document;
    const EntityId id =
        add(document, polylineEntity({Point2(0, 0), Point2(5, 0), Point2(10, 0)}, {0.0, 5.0, 10.0}));
    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(document
                    .execute(editPolyline(id, "VERTEX_INSERT",
                                          [](const CurvePolyline2& p) {
                                              return katana::geometry::insertVertex(p, 0,
                                                                                    Point2(2, 0));
                                          }))
                    .ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    const Entity* edited = document.model().entities.find(id);
    ASSERT_EQ(std::get<Polyline2>(edited->geometry).vertices.size(), 4u);
    EXPECT_EQ(katana::entity::heightsOf(edited->properties, 4)[1], 2.0);
    EXPECT_EQ(document.history().undoName(), "VERTEX_INSERT");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(id)->geometry).vertices.size(), 3u);
}

TEST(VertexEditing, AFailedEditLeavesTheDrawingAndHistoryAlone)
{
    Document document;
    const EntityId id = add(document, polylineEntity({Point2(0, 0), Point2(5, 0)}));
    const std::size_t before = document.history().undoCount();
    const auto status = document.execute(editPolyline(id, "VERTEX_DELETE", [](const CurvePolyline2& p) {
        return katana::geometry::deleteVertex(p, 0);
    }));
    EXPECT_FALSE(status.ok());
    EXPECT_EQ(document.history().undoCount(), before);
    Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(1, 1)};
    const EntityId pointId = add(document, point);
    const auto notPolyline = document.execute(editPolyline(pointId, "WEED", [](const CurvePolyline2& p) {
        return katana::geometry::weed(p, 0.1);
    }));
    ASSERT_FALSE(notPolyline.ok());
    EXPECT_NE(notPolyline.error().message.find("not a polyline"), std::string::npos);
}

TEST(VertexEditing, SeveralPolylinesInOneStep)
{
    Document document;
    const EntityId a = add(document, polylineEntity({Point2(0, 0), Point2(1, 0.001), Point2(2, 0)}));
    const EntityId b = add(document, polylineEntity({Point2(0, 5), Point2(1, 5.001), Point2(2, 5)}));
    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(document
                    .execute(editPolylines({a, b}, "WEED",
                                           [](const CurvePolyline2& p) {
                                               return katana::geometry::weed(p, 0.01);
                                           }))
                    .ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(a)->geometry).vertices.size(), 2u);
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(b)->geometry).vertices.size(), 2u);
}

TEST(VertexEditing, SurveyPointsOnVerticesAreFound)
{
    Document document;
    Entity mark;
    mark.geometry = katana::entity::PointGeometry{Point2(5, 0)};
    mark.properties["point"] = std::string("101");
    add(document, mark);
    Entity cad;
    cad.geometry = katana::entity::PointGeometry{Point2(10, 0)}; // no point number
    add(document, cad);
    const auto polyline = CurvePolyline2::fromPoints({Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    EXPECT_EQ(verticesOnSurveyPoints(document, polyline), (std::vector<bool>{false, true, false}));
}
