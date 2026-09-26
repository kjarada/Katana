// The Vertices panel's table, headless (include/katana/cad/drawing/
// vertex_table.hpp): the rows a polyline gives and what typing into each
// column does.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/drawing/vertex_table.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;

namespace {

CurvePolyline2 traverse()
{
    // East 10, then north 10, heights 100 and 101 and none.
    CurvePolyline2 p = CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    p.vertices[0].height = 100.0;
    p.vertices[1].height = 101.0;
    return p;
}

} // namespace

TEST(VertexTable, RowsGiveCoordinatesHeightsAndEachSegmentsBearingAndDistance)
{
    const auto rows = vertexRows(traverse());
    ASSERT_EQ(rows.size(), 3u);
    EXPECT_EQ(cellText(rows[0], VertexColumn::Easting), "0.000");
    EXPECT_EQ(cellText(rows[0], VertexColumn::Height), "100.000");
    EXPECT_EQ(cellText(rows[2], VertexColumn::Height), "") << "no height is not zero";
    // East is a bearing of 90, north of 0.
    EXPECT_EQ(cellText(rows[0], VertexColumn::Bearing), "90\xC2\xB0" "00'00\"");
    EXPECT_EQ(cellText(rows[1], VertexColumn::Bearing), "0\xC2\xB0" "00'00\"");
    EXPECT_EQ(cellText(rows[0], VertexColumn::Distance), "10.000");
    EXPECT_EQ(cellText(rows[2], VertexColumn::Bearing), "") << "the last vertex starts no segment";
    EXPECT_EQ(vertexColumnFromString("Z"), VertexColumn::Height);
    EXPECT_EQ(vertexColumnFromString("brg"), VertexColumn::Bearing);
    EXPECT_FALSE(vertexColumnFromString("colour").has_value());
}

TEST(VertexTable, TypingIntoACellEditsTheVertex)
{
    const CurvePolyline2 p = traverse();
    EXPECT_EQ(editVertexCell(p, 1, VertexColumn::Easting, "12.5")->vertices[1].position,
              Point2(12.5, 0));
    EXPECT_EQ(editVertexCell(p, 2, VertexColumn::Height, "99.25")->vertices[2].height, 99.25);
    EXPECT_FALSE(editVertexCell(p, 0, VertexColumn::Height, "")->vertices[0].height.has_value());
    EXPECT_EQ(editVertexCell(p, 0, VertexColumn::Bulge, "0.5")->vertices[0].bulge, 0.5);
    EXPECT_FALSE(editVertexCell(p, 0, VertexColumn::Index, "3").ok());
    EXPECT_FALSE(editVertexCell(p, 0, VertexColumn::Easting, "east").ok());
}

TEST(VertexTable, ABearingOrADistanceMovesTheNextVertex)
{
    const CurvePolyline2 p = traverse();
    // The first segment turned to a bearing of 45 degrees, its 10 m kept.
    const auto turned = editVertexCell(p, 0, VertexColumn::Bearing, "45d00'00\"");
    ASSERT_TRUE(turned.ok());
    EXPECT_NEAR(turned->vertices[1].position.x, 10 * std::sqrt(0.5), 1e-9);
    EXPECT_NEAR(turned->vertices[1].position.y, 10 * std::sqrt(0.5), 1e-9);
    // A quadrant bearing reads the same way.
    const auto quadrant = editVertexCell(p, 0, VertexColumn::Bearing, "N45E");
    EXPECT_NEAR(quadrant->vertices[1].position.x, turned->vertices[1].position.x, 1e-9);
    // The second segment stretched to 25 m, north still.
    const auto longer = editVertexCell(p, 1, VertexColumn::Distance, "25");
    ASSERT_TRUE(longer.ok());
    EXPECT_NEAR(longer->vertices[2].position.y, 25.0, 1e-9);
    EXPECT_FALSE(editVertexCell(p, 2, VertexColumn::Distance, "5").ok());
}
