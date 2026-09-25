// Vertex editing (include/katana/geometry/polyline_vertices.hpp): each
// operation the grips, the Vertices tools and the VERTEX verbs share, against
// results worked out by hand.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/geometry/polyline_vertices.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::geometry;
using katana::math::kHalfPi;
using katana::math::kPi;

namespace {

CurvePolyline2 line3d(std::vector<Point2> points, std::vector<std::optional<double>> heights = {})
{
    CurvePolyline2 p = CurvePolyline2::fromPoints(points);
    for (std::size_t i = 0; i < heights.size() && i < p.vertices.size(); ++i) {
        p.vertices[i].height = heights[i];
    }
    return p;
}

void expectNear(const Point2& a, const Point2& b, double tolerance = 1e-9)
{
    EXPECT_NEAR(a.x, b.x, tolerance);
    EXPECT_NEAR(a.y, b.y, tolerance);
}

} // namespace

TEST(PolylineVertices, InsertOnAStraightSegmentInterpolatesTheHeight)
{
    const auto p = line3d({Point2(0, 0), Point2(10, 0)}, {100.0, 110.0});
    const auto out = insertVertex(p, 0, Point2(4, 0));
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->vertices.size(), 3u);
    expectNear(out->vertices[1].position, Point2(4, 0));
    EXPECT_NEAR(*out->vertices[1].height, 104.0, 1e-12);
    // Off the line it bends the segment, and a given height wins.
    const auto bent = insertVertex(p, 0, Point2(5, 3), 99.0);
    ASSERT_TRUE(bent.ok());
    expectNear(bent->vertices[1].position, Point2(5, 3));
    EXPECT_EQ(bent->vertices[1].height, 99.0);
    EXPECT_FALSE(insertVertex(p, 0, Point2(0, 0)).ok()) << "on a vertex";
    EXPECT_FALSE(insertVertex(p, 1, Point2(5, 0)).ok()) << "no segment 1";
}

TEST(PolylineVertices, InsertOnAnArcKeepsTheCircle)
{
    CurvePolyline2 p = line3d({Point2(0, 0), Point2(2, 0)});
    p.vertices[0].bulge = 1.0; // semicircle below the chord, centre (1,0)
    const auto out = insertVertex(p, 0, Point2(1, -5));
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->vertices.size(), 3u);
    expectNear(out->vertices[1].position, Point2(1, -1));
    // Two quarter circles.
    EXPECT_NEAR(out->vertices[0].bulge, std::tan(kPi / 8), 1e-12);
    EXPECT_NEAR(out->vertices[1].bulge, std::tan(kPi / 8), 1e-12);
    EXPECT_NEAR(out->length(), p.length(), 1e-9);
}

TEST(PolylineVertices, DeleteJoinsTheNeighboursStraightAndKeepsAPolyline)
{
    CurvePolyline2 p = line3d({Point2(0, 0), Point2(5, 5), Point2(10, 0)});
    p.vertices[0].bulge = 0.3;
    const auto out = deleteVertex(p, 1);
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->vertices.size(), 2u);
    EXPECT_EQ(out->vertices[0].bulge, 0.0);
    EXPECT_FALSE(deleteVertex(*out, 0).ok()) << "two is the least";

    CurvePolyline2 ring = line3d({Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)});
    ring.closed = true;
    const auto three = deleteVertices(ring, {3});
    ASSERT_TRUE(three.ok());
    EXPECT_FALSE(deleteVertex(*three, 0).ok()) << "a ring keeps three";
    EXPECT_FALSE(deleteVertices(ring, {0, 1}).ok());
    const auto first = deleteVertex(ring, 0);
    ASSERT_TRUE(first.ok());
    EXPECT_EQ(first->vertices.front().position, Point2(10, 0));
}

TEST(PolylineVertices, MovingAVertexKeepsEachArcsBulge)
{
    CurvePolyline2 p = line3d({Point2(0, 0), Point2(2, 0), Point2(4, 0)}, {1.0, 2.0, 3.0});
    p.vertices[0].bulge = 0.5;
    const auto out = moveVertex(p, 1, Point2(3, 1));
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(out->vertices[1].position, Point2(3, 1));
    EXPECT_EQ(out->vertices[0].bulge, 0.5);
    EXPECT_EQ(out->vertices[1].height, 2.0);
    const auto moved = moveSegment(p, 1, Vec2(0, 2));
    ASSERT_TRUE(moved.ok());
    EXPECT_EQ(moved->vertices[1].position, Point2(2, 2));
    EXPECT_EQ(moved->vertices[2].position, Point2(4, 2));
    EXPECT_EQ(moved->vertices[0].position, Point2(0, 0));
}

TEST(PolylineVertices, SegmentsBecomeArcsAndLines)
{
    const auto p = line3d({Point2(0, 0), Point2(2, 0)});
    const auto arc = segmentToArc(p, 0, Point2(1, 1));
    ASSERT_TRUE(arc.ok());
    EXPECT_NEAR(arc->vertices[0].bulge, -1.0, 1e-12) << "through the top: clockwise";
    EXPECT_FALSE(segmentToArc(p, 0, Point2(1, 0)).ok());
    const auto back = segmentToLine(*arc, 0);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(back->vertices[0].bulge, 0.0);
    EXPECT_FALSE(segmentToLine(*back, 0).ok()) << "already straight";
}

TEST(PolylineVertices, StraightenRemovesTheVerticesBetween)
{
    const auto p = line3d({Point2(0, 0), Point2(1, 1), Point2(2, -1), Point2(3, 1), Point2(4, 0)});
    const auto out = straighten(p, 3, 0);
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->vertices.size(), 3u);
    EXPECT_EQ(out->vertices[0].position, Point2(0, 0));
    EXPECT_EQ(out->vertices[1].position, Point2(3, 1));
    EXPECT_EQ(verticesBetween(p, 1, 4), (std::vector<std::size_t>{2, 3}));

    CurvePolyline2 ring = line3d({Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(5, 12),
                                  Point2(0, 10)});
    ring.closed = true;
    // Walking forward from 3 round the start to 1 passes 4 and 0.
    EXPECT_EQ(verticesBetween(ring, 3, 1), (std::vector<std::size_t>{4, 0}));
    const auto cut = straighten(ring, 3, 1);
    ASSERT_TRUE(cut.ok());
    EXPECT_EQ(cut->vertices.size(), 3u);
    EXPECT_FALSE(straighten(ring, 2, 2).ok());
}

TEST(PolylineVertices, GradeAndInterpolateSetHeightsByLength)
{
    const auto p = line3d({Point2(0, 0), Point2(3, 0), Point2(3, 4), Point2(10, 4)},
                          {10.0, std::nullopt, 50.0, std::nullopt});
    // 3 + 4 + 7 = 14 m from vertex 0 to vertex 3.
    CurvePolyline2 ends = p;
    ends.vertices[3].height = 24.0;
    const auto graded = gradeBetween(ends, 0, 3);
    ASSERT_TRUE(graded.ok());
    EXPECT_NEAR(*graded->vertices[1].height, 13.0, 1e-12);
    EXPECT_NEAR(*graded->vertices[2].height, 17.0, 1e-12);
    EXPECT_FALSE(gradeBetween(p, 0, 3).ok()) << "vertex 3 has no height";

    const auto filled = interpolateHeights(p);
    ASSERT_TRUE(filled.ok());
    EXPECT_NEAR(*filled->vertices[1].height, 10.0 + 40.0 * 3.0 / 7.0, 1e-12);
    EXPECT_FALSE(filled->vertices[3].height.has_value()) << "no extrapolation";
}

TEST(PolylineVertices, WeedDropsWhatLiesWithinTheTolerance)
{
    // A straight run with a 2 cm wobble, and a real corner.
    const auto p = line3d({Point2(0, 0), Point2(1, 0.02), Point2(2, -0.01), Point2(3, 0),
                           Point2(3, 5)});
    const auto out = weed(p, 0.05);
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->vertices.size(), 3u);
    EXPECT_EQ(out->vertices[1].position, Point2(3, 0));
    // A tighter tolerance keeps the wobble.
    EXPECT_EQ(weed(p, 0.005)->vertices.size(), 5u);
    // A vertex marked to keep stays whatever its deviation.
    const auto kept = weed(p, 0.05, {false, true, false, false, false});
    EXPECT_EQ(kept->vertices.size(), 4u);
}

TEST(PolylineVertices, WeedCountsHeightAndKeepsArcs)
{
    // Straight in plan, but vertex 1 is 1 m above the grade: it stays.
    const auto p = line3d({Point2(0, 0), Point2(5, 0), Point2(10, 0)}, {0.0, 1.0, 0.0});
    EXPECT_EQ(weed(p, 0.1)->vertices.size(), 3u);
    EXPECT_EQ(weed(p, 2.0)->vertices.size(), 2u);
    CurvePolyline2 arc = line3d({Point2(0, 0), Point2(5, 0), Point2(10, 0), Point2(15, 0)});
    arc.vertices[1].bulge = 0.2;
    EXPECT_EQ(weed(arc, 100.0)->vertices.size(), 4u) << "both ends of the arc stay";
    CurvePolyline2 ring = line3d({Point2(0, 0), Point2(5, 0.001), Point2(10, 0), Point2(10, 10),
                                  Point2(0, 10)});
    ring.closed = true;
    const auto square = weed(ring, 0.01);
    ASSERT_TRUE(square.ok());
    EXPECT_EQ(square->vertices.size(), 4u);
    EXPECT_GE(weed(ring, 1000.0)->vertices.size(), 3u);
}

TEST(PolylineVertices, DensifyDividesStraightsAndChordsOrSplitsArcs)
{
    const auto p = line3d({Point2(0, 0), Point2(10, 0)}, {0.0, 10.0});
    const auto out = densify(p, 3.0);
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->vertices.size(), 5u); // 4 parts of 2.5
    expectNear(out->vertices[1].position, Point2(2.5, 0));
    EXPECT_NEAR(*out->vertices[2].height, 5.0, 1e-12);

    CurvePolyline2 arc = line3d({Point2(0, 0), Point2(2, 0)});
    arc.vertices[0].bulge = 1.0; // length pi
    const auto split = densify(arc, 1.0);
    ASSERT_TRUE(split.ok());
    EXPECT_EQ(split->vertices.size(), 5u) << "four sub-arcs of pi/4";
    EXPECT_NEAR(split->length(), kPi, 1e-9);
    const auto chorded = densify(arc, 0.0, 0.01);
    ASSERT_TRUE(chorded.ok());
    EXPECT_FALSE(chorded->hasArcs());
    for (const auto& v : chorded->vertices) {
        EXPECT_NEAR(v.position.distanceTo(Point2(1, 0)), 1.0, 1e-9);
    }
    EXPECT_FALSE(densify(p, 0.0, 0.0).ok());
}

TEST(PolylineVertices, MergeSnapCloseOpenAndStart)
{
    const auto p = line3d({Point2(0, 0), Point2(0.001, 0), Point2(5, 0), Point2(5, 5)});
    const auto merged = mergeNearVertices(p, 0.01);
    ASSERT_TRUE(merged.ok());
    EXPECT_EQ(merged->vertices.size(), 3u);

    const auto snapped = snapToGrid(line3d({Point2(0.4, 0.6), Point2(2.2, 2.9)}), 1.0);
    ASSERT_TRUE(snapped.ok());
    EXPECT_EQ(snapped->vertices[0].position, Point2(0, 1));
    EXPECT_EQ(snapped->vertices[1].position, Point2(2, 3));

    const auto closed = closePolyline(p);
    ASSERT_TRUE(closed.ok());
    EXPECT_TRUE(closed->closed);
    EXPECT_FALSE(closePolyline(*closed).ok());
    const auto reopened = openPolyline(*closed);
    ASSERT_TRUE(reopened.ok());
    EXPECT_FALSE(reopened->closed);
    const auto onStart = closePolyline(line3d({Point2(0, 0), Point2(4, 0), Point2(4, 4),
                                               Point2(0, 0)}));
    ASSERT_TRUE(onStart.ok());
    EXPECT_EQ(onStart->vertices.size(), 3u) << "the last vertex on the first is dropped";

    const auto rotated = changeStartVertex(*closed, 2);
    ASSERT_TRUE(rotated.ok());
    EXPECT_EQ(rotated->vertices.front().position, Point2(5, 0));
    EXPECT_NEAR(rotated->area(), closed->area(), 1e-12);
    EXPECT_FALSE(changeStartVertex(p, 1).ok()) << "open";
}

TEST(PolylineVertices, FilletRoundsACornerWithATangentArc)
{
    // A right-angle corner at (10,0), turning left.
    const auto p = line3d({Point2(0, 0), Point2(10, 0), Point2(10, 10)}, {0.0, 10.0, 20.0});
    const auto out = filletVertex(p, 1, 2.0);
    ASSERT_TRUE(out.ok()) << out.error().message;
    ASSERT_EQ(out->vertices.size(), 4u);
    expectNear(out->vertices[1].position, Point2(8, 0));
    expectNear(out->vertices[2].position, Point2(10, 2));
    // A left turn through a quarter: bulge tan(pi/8).
    EXPECT_NEAR(out->vertices[1].bulge, std::tan(kPi / 8), 1e-12);
    const auto arc = std::get<Arc2>(out->segment(1));
    expectNear(arc.center, Point2(8, 2));
    EXPECT_NEAR(arc.radius, 2.0, 1e-12);
    EXPECT_NEAR(*out->vertices[1].height, 8.0, 1e-12);
    EXPECT_NEAR(*out->vertices[2].height, 12.0, 1e-12);
    EXPECT_FALSE(filletVertex(p, 1, 20.0).ok()) << "does not fit";
    EXPECT_FALSE(filletVertex(p, 0, 1.0).ok()) << "an end";
    EXPECT_FALSE(filletVertex(line3d({Point2(0, 0), Point2(1, 0), Point2(2, 0)}), 1, 0.1)
                     .ok())
        << "no corner";
}

TEST(PolylineVertices, ChamferCutsACorner)
{
    const auto p = line3d({Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    const auto out = chamferVertex(p, 1, 2.0, 3.0);
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->vertices.size(), 4u);
    expectNear(out->vertices[1].position, Point2(8, 0));
    expectNear(out->vertices[2].position, Point2(10, 3));
    EXPECT_EQ(out->vertices[1].bulge, 0.0);
    // A chamfer the full length of a segment reuses the neighbour.
    const auto whole = chamferVertex(p, 1, 10.0, 3.0);
    ASSERT_TRUE(whole.ok());
    EXPECT_EQ(whole->vertices.size(), 3u);
}

TEST(PolylineVertices, NearestVertexAndSegment)
{
    const auto p = line3d({Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    EXPECT_EQ(nearestVertex(p, Point2(9, 1)), 1u);
    EXPECT_EQ(nearestSegment(p, Point2(11, 6)), 1u);
    EXPECT_EQ(minimumVertices(p), 2u);
}
