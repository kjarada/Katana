// Annotation drawn out as plain shapes for a file
// (cad/annotation/export_annotation.hpp, docs/annotation.md "Exchange"):
// what the DXF writer is handed for the labels, leaders and dimension kinds
// it cannot draw itself.

#include <gtest/gtest.h>

#include <algorithm>
#include <variant>

#include "katana/cad/annotation/export_annotation.hpp"
#include "katana/entity/annotation.hpp"

using namespace katana::cad::annotation;
using namespace katana::entity;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

template <typename T> std::size_t count(const std::vector<Geometry>& shapes)
{
    return static_cast<std::size_t>(std::count_if(
        shapes.begin(), shapes.end(), [](const Geometry& g) { return std::holds_alternative<T>(g); }));
}

EntityId add(Model& model, Geometry geometry)
{
    Entity entity;
    entity.geometry = std::move(geometry);
    auto id = model.entities.add(std::move(entity));
    EXPECT_TRUE(id.ok());
    return id.ok() ? *id : kInvalidEntityId;
}

} // namespace

TEST(ExportAnnotation, ALabelIsItsTextWhereThePlanViewPutsItAtTheScale)
{
    Model model;
    LabelStyle style;
    style.name = "pt";
    style.kind = LabelKind::Point;
    style.text = "{point}";
    style.marker = LabelMarker::Circle;
    ASSERT_TRUE(model.labelStyles.add(style).ok());
    Entity point;
    point.geometry = PointGeometry{Point2(10.0, 10.0)};
    point.properties["point"] = std::string("P7");
    const auto target = model.entities.add(point);
    ASSERT_TRUE(target.ok());
    const EntityId label =
        add(model, LabelGeometry{.target = *target, .style = "pt", .anchor = Point2(10.0, 10.0)});

    for (const double scale : {200.0, 1000.0}) {
        const DrawnAnnotation drawn = drawAnnotationForExport(model, scale);
        ASSERT_TRUE(drawn.contains(label));
        const auto& shapes = drawn.at(label);
        ASSERT_EQ(count<TextGeometry>(shapes), 1u) << scale;
        const auto text = std::ranges::find_if(
            shapes, [](const Geometry& g) { return std::holds_alternative<TextGeometry>(g); });
        EXPECT_EQ(std::get<TextGeometry>(*text).text, "P7");
        // 2.5 mm on paper at the scale.
        EXPECT_DOUBLE_EQ(std::get<TextGeometry>(*text).height, 2.5 * scale / 1000.0);
        EXPECT_EQ(count<Polyline2>(shapes), 1u) << "the marker, a closed outline";
        EXPECT_FALSE(drawn.contains(*target)) << "the point is the writer's own";
    }
}

TEST(ExportAnnotation, ALabelWithNoRoomIsAnEntryWithNoShapes)
{
    Model model;
    LabelStyle style;
    style.name = "pt";
    style.kind = LabelKind::Point;
    style.text = "{point}";
    style.displace = false;
    ASSERT_TRUE(model.labelStyles.add(style).ok());
    std::vector<EntityId> labels;
    for (int i = 0; i < 2; ++i) {
        Entity point;
        point.geometry = PointGeometry{Point2(0.0, 0.0)};
        point.properties["point"] = std::string("P");
        const auto target = model.entities.add(point);
        ASSERT_TRUE(target.ok());
        labels.push_back(
            add(model, LabelGeometry{.target = *target, .style = "pt", .anchor = Point2(0.0, 0.0)}));
    }
    const DrawnAnnotation drawn = drawAnnotationForExport(model, 1000.0);
    EXPECT_FALSE(drawn.at(labels[0]).empty());
    EXPECT_TRUE(drawn.at(labels[1]).empty()) << "on top of the first, with nowhere else to go";
}

TEST(ExportAnnotation, DimensionKindsAndLeadersAreDrawnOutAndAlignedOnesAreLeftToTheWriter)
{
    Model model;
    const EntityId round = add(model, Circle2{Point2(0.0, 0.0), 5.0});
    DimensionGeometry radius;
    radius.kind = DimensionKind::Radius;
    radius.vertex = Point2(0.0, 0.0);
    radius.start = Point2(5.0, 0.0);
    radius.offset = 3.0;
    const EntityId radial = add(model, radius);
    const EntityId aligned =
        add(model, DimensionGeometry{Point2(0.0, 0.0), Point2(10.0, 0.0), 2.0, ""});
    LeaderGeometry leader;
    leader.vertices = {Point2(0.0, 0.0), Point2(8.0, 8.0)};
    leader.text = "PIT 12\nIL 10.50";
    leader.callout = CalloutShape::Box;
    const EntityId note = add(model, leader);

    const DrawnAnnotation drawn = drawAnnotationForExport(model, 500.0);
    EXPECT_FALSE(drawn.contains(round));
    EXPECT_FALSE(drawn.contains(aligned));
    ASSERT_TRUE(drawn.contains(radial));
    const auto& dimension = drawn.at(radial);
    ASSERT_EQ(count<TextGeometry>(dimension), 1u);
    const auto text = std::ranges::find_if(
        dimension, [](const Geometry& g) { return std::holds_alternative<TextGeometry>(g); });
    EXPECT_EQ(std::get<TextGeometry>(*text).text.front(), 'R');
    EXPECT_GE(count<Segment2>(dimension) + count<Polyline2>(dimension), 2u) << "line and arrow";

    ASSERT_TRUE(drawn.contains(note));
    const auto& callout = drawn.at(note);
    EXPECT_EQ(count<TextGeometry>(callout), 2u) << "a text a line";
    // The line, the arrowhead's outline and the box.
    std::size_t closed = 0;
    for (const Geometry& shape : callout) {
        if (const auto* polyline = std::get_if<Polyline2>(&shape); polyline && polyline->closed) {
            ++closed;
        }
    }
    EXPECT_EQ(closed, 2u) << "the arrowhead and the callout's box";
}

TEST(ExportAnnotation, AStrokeOfTwoPointsIsASegmentAndOneWithNoLengthIsNothing)
{
    Drawing drawing;
    drawing.strokes = {{Point2(0, 0), Point2(1, 0)},
                       {Point2(2, 2), Point2(2, 2)},
                       {Point2(0, 0), Point2(1, 0), Point2(1, 1)}};
    drawing.fills = {{Point2(0, 0), Point2(1, 0), Point2(0, 1)}};
    const auto shapes = plainShapes(drawing);
    EXPECT_EQ(count<Segment2>(shapes), 1u);
    ASSERT_EQ(count<Polyline2>(shapes), 2u);
    EXPECT_FALSE(std::get<Polyline2>(shapes[1]).closed);
    EXPECT_TRUE(std::get<Polyline2>(shapes[2]).closed) << "a fill is its outline";
}
