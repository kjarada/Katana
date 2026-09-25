// The associative update (cad/annotation/associative.hpp, docs/annotation.md
// "Associativity"): dimensions, leaders and labels follow the geometry they
// refer to through ordinary edits, in the SAME undo step, and one undo puts
// back the geometry and everything that followed it.

#include <gtest/gtest.h>

#include "katana/cad/annotation/associative.hpp"
#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
namespace cmd = katana::commands;
namespace ann = katana::cad::annotation;

namespace {

EntityId add(Document& document, Geometry geometry)
{
    EXPECT_TRUE(document.execute(cmd::createEntities({Entity{.geometry = std::move(geometry)}})).ok());
    return document.lastCreatedEntities().front();
}

const Geometry& geometryOf(const Document& document, EntityId id)
{
    return document.model().entities.find(id)->geometry;
}

} // namespace

TEST(Associative, ADimensionFollowsTheLineItMeasuresInOneUndoStep)
{
    Document document;
    const EntityId line = add(document, Segment2{Point2(0, 0), Point2(10, 0)});
    auto dimension = ann::alignedDimension(
        ann::AnchoredPoint{Point2(0, 0), AnchorRef{line, AnchorPoint::Start, 0}},
        ann::AnchoredPoint{Point2(10, 0), AnchorRef{line, AnchorPoint::End, 0}}, Point2(5, 3));
    ASSERT_TRUE(dimension.ok());
    const EntityId dim = add(document, *dimension);
    const std::size_t steps = document.history().undoCount();

    // Stretch the line by replacing its geometry, as a grip edit does.
    ASSERT_TRUE(document.execute(cmd::setEntityGeometry(line, Segment2{Point2(0, 0), Point2(25, 0)}))
                    .ok());
    EXPECT_EQ(document.history().undoCount(), steps + 1) << "the follow-up is not a step of its own";
    const auto& followed = std::get<DimensionGeometry>(geometryOf(document, dim));
    EXPECT_EQ(followed.end, Point2(25, 0));
    EXPECT_DOUBLE_EQ(followed.measurement(), 25.0);

    // One undo: the line AND the dimension.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(std::get<Segment2>(geometryOf(document, line)).end, Point2(10, 0));
    EXPECT_EQ(std::get<DimensionGeometry>(geometryOf(document, dim)).end, Point2(10, 0));
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(std::get<DimensionGeometry>(geometryOf(document, dim)).end, Point2(25, 0));
}

TEST(Associative, MovingTheMeasuredEntityMovesTheDimension)
{
    Document document;
    const EntityId a = add(document, PointGeometry{Point2(0, 0)});
    const EntityId b = add(document, PointGeometry{Point2(0, 10)});
    auto dimension = ann::linearDimension(ann::AnchoredPoint{Point2(0, 0), AnchorRef{a}},
                                          ann::AnchoredPoint{Point2(0, 10), AnchorRef{b}},
                                          Point2(-3, 5));
    ASSERT_TRUE(dimension.ok());
    EXPECT_NEAR(dimension->angle, 0.5 * katana::math::kPi, 1e-15) << "beside the points: vertical";
    const EntityId dim = add(document, *dimension);
    ASSERT_TRUE(document.execute(cmd::moveEntities({b}, Vec2(0, 5))).ok());
    EXPECT_DOUBLE_EQ(std::get<DimensionGeometry>(geometryOf(document, dim)).measurement(), 15.0);
}

TEST(Associative, ARadiusFollowsItsCircleAndKeepsItsDirection)
{
    Document document;
    const EntityId circle = add(document, Circle2{Point2(0, 0), 5.0});
    auto dimension = ann::radialDimension(document.model(), circle, Point2(10, 10), false);
    ASSERT_TRUE(dimension.ok());
    const EntityId dim = add(document, *dimension);
    ASSERT_TRUE(document.execute(cmd::setEntityGeometry(circle, Circle2{Point2(100, 0), 8.0})).ok());
    const auto& followed = std::get<DimensionGeometry>(geometryOf(document, dim));
    EXPECT_EQ(followed.vertex, Point2(100, 0));
    EXPECT_NEAR(followed.measurement(), 8.0, 1e-12);
    // Still towards the upper right, at 45 degrees.
    EXPECT_NEAR(followed.start.x - 100.0, followed.start.y, 1e-12);
}

TEST(Associative, AnAngleBetweenLinesFollowsTheirIntersection)
{
    Document document;
    const EntityId first = add(document, Segment2{Point2(0, 0), Point2(10, 0)});
    const EntityId second = add(document, Segment2{Point2(0, 0), Point2(0, 10)});
    auto dimension = ann::angularBetweenLines(document.model(), first, second, Point2(3, 3));
    ASSERT_TRUE(dimension.ok());
    EXPECT_NEAR(dimension->measurement(), 0.5 * katana::math::kPi, 1e-12);
    const EntityId dim = add(document, *dimension);
    ASSERT_TRUE(document.execute(cmd::moveEntities({first, second}, Vec2(7, -2))).ok());
    EXPECT_EQ(std::get<DimensionGeometry>(geometryOf(document, dim)).vertex, Point2(7, -2));
}

TEST(Associative, ErasingTheMeasuredLineLeavesTheDimensionUnassociated)
{
    Document document;
    const EntityId line = add(document, Segment2{Point2(0, 0), Point2(10, 0)});
    auto dimension = ann::alignedDimension(
        ann::AnchoredPoint{Point2(0, 0), AnchorRef{line, AnchorPoint::Start, 0}},
        ann::AnchoredPoint{Point2(10, 0), AnchorRef{line, AnchorPoint::End, 0}}, Point2(5, 3));
    const EntityId dim = add(document, *dimension);
    ASSERT_TRUE(document.execute(cmd::deleteEntities({line})).ok());
    const auto& left = std::get<DimensionGeometry>(geometryOf(document, dim));
    EXPECT_FALSE(left.startRef.associated());
    EXPECT_FALSE(left.endRef.associated());
    EXPECT_EQ(left.end, Point2(10, 0)) << "where it last was";
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(std::get<DimensionGeometry>(geometryOf(document, dim)).endRef.associated())
        << "the undo gives the reference back with the line";
}

TEST(Associative, ALeaderTipFollowsItsPoint)
{
    Document document;
    const EntityId pit = add(document, PointGeometry{Point2(5, 5)});
    const EntityId leader = add(document, LeaderGeometry{.vertices = {Point2(5, 5), Point2(10, 10)},
                                                         .text = "PIT",
                                                         .tipRef = AnchorRef{pit}});
    ASSERT_TRUE(document.execute(cmd::moveEntities({pit}, Vec2(1, 0))).ok());
    const auto& followed = std::get<LeaderGeometry>(geometryOf(document, leader));
    EXPECT_EQ(followed.vertices.front(), Point2(6, 5));
    EXPECT_EQ(followed.vertices.back(), Point2(10, 10)) << "only the tip follows";
}

TEST(Associative, ALabelFollowsItsTargetAndGoesWithIt)
{
    Document document;
    LabelStyle style;
    style.name = "pt";
    style.kind = LabelKind::Point;
    style.text = "{x:.1f}";
    ASSERT_TRUE(document.execute(cmd::createLabelStyle(style)).ok());
    const EntityId point = add(document, PointGeometry{Point2(1, 1)});
    const EntityId label =
        add(document, LabelGeometry{.target = point, .style = "pt", .anchor = Point2(1, 1),
                                    .position = Point2(3, 3)});
    ASSERT_TRUE(document.execute(cmd::moveEntities({point}, Vec2(10, 0))).ok());
    const auto& moved = std::get<LabelGeometry>(geometryOf(document, label));
    EXPECT_EQ(moved.anchor, Point2(11, 1));
    EXPECT_EQ(*moved.position, Point2(13, 3)) << "a dragged label keeps its place beside it";

    const std::size_t steps = document.history().undoCount();
    ASSERT_TRUE(document.execute(cmd::deleteEntities({point})).ok());
    EXPECT_FALSE(document.model().entities.contains(label)) << "a label goes with its target";
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.model().entities.contains(label)) << "and comes back with it";
    EXPECT_TRUE(document.model().entities.contains(point));
}

TEST(Associative, AnAnnotationOnALockedLayerIsLeftAlone)
{
    Document document;
    Layer locked;
    locked.name = "dims";
    ASSERT_TRUE(document.execute(cmd::createLayer(locked)).ok());
    const EntityId line = add(document, Segment2{Point2(0, 0), Point2(10, 0)});
    auto dimension = ann::alignedDimension(
        ann::AnchoredPoint{Point2(0, 0), AnchorRef{line, AnchorPoint::Start, 0}},
        ann::AnchoredPoint{Point2(10, 0), AnchorRef{line, AnchorPoint::End, 0}}, Point2(5, 3));
    Entity entity{.geometry = *dimension, .layer = "dims"};
    ASSERT_TRUE(document.execute(cmd::createEntities({entity})).ok());
    const EntityId dim = document.lastCreatedEntities().front();
    locked.locked = true;
    ASSERT_TRUE(document.execute(cmd::updateLayer(locked)).ok());
    ASSERT_TRUE(document.execute(cmd::setEntityGeometry(line, Segment2{Point2(0, 0), Point2(20, 0)}))
                    .ok())
        << "the edit is not refused because a dimension that follows it is locked";
    EXPECT_EQ(std::get<DimensionGeometry>(geometryOf(document, dim)).end, Point2(10, 0));
}

TEST(Associative, NothingToFollowIsNoChange)
{
    Model model;
    EXPECT_TRUE(ann::associativeChanges(model).empty());
    Entity plain{.id = 1, .geometry = DimensionGeometry{Point2(0, 0), Point2(1, 0), 1.0, ""}};
    ASSERT_TRUE(model.entities.insert(plain).ok());
    EXPECT_TRUE(ann::associativeChanges(model).empty()) << "an unassociated dimension stays";
}
