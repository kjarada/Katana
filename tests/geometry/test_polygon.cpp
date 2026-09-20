#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/geometry/polygon.hpp"
#include "support/property.hpp"

using namespace katana::geometry;
using katana::core::ErrorCode;
using katana::math::kTwoPi;
using katana::math::nearlyEqual;
using katana::test::Random;

namespace {

Polyline2 square(double size)
{
    return Polyline2{{Point2(0, 0), Point2(size, 0), Point2(size, size), Point2(0, size)}, true};
}

Polyline2 lShape()
{
    return Polyline2{{Point2(0, 0), Point2(2, 0), Point2(2, 1), Point2(1, 1), Point2(1, 2),
                      Point2(0, 2)},
                     true};
}

double triangleAreaSum(const Polyline2& polygon, const std::vector<TriangleIndices>& triangles)
{
    double total = 0.0;
    for (const TriangleIndices& t : triangles) {
        const Triangle2 triangle{polygon.vertices[t[0]], polygon.vertices[t[1]],
                                 polygon.vertices[t[2]]};
        EXPECT_GT(triangle.signedArea(), 0.0) << "triangles must be counter-clockwise";
        total += triangle.area();
    }
    return total;
}

} // namespace

TEST(PolygonOrientation, DetectsWindingAndDegeneracy)
{
    EXPECT_EQ(orientation(square(2)), Orientation::CounterClockwise);
    EXPECT_EQ(orientation(square(2).reversed()), Orientation::Clockwise);
    EXPECT_EQ(orientation(Polyline2{{Point2(0, 0), Point2(1, 1), Point2(2, 2)}, true}),
              Orientation::Degenerate);
    EXPECT_EQ(orientation(Polyline2{{Point2(0, 0), Point2(1, 0)}, true}), Orientation::Degenerate);
    Polyline2 open = square(2);
    open.closed = false;
    EXPECT_EQ(orientation(open), Orientation::Degenerate);
}

TEST(PolygonConvexity, ConvexConcaveAndCollinearVertices)
{
    EXPECT_TRUE(isConvex(square(2)));
    EXPECT_TRUE(isConvex(square(2).reversed()));
    EXPECT_FALSE(isConvex(lShape()));
    // A redundant vertex in the middle of an edge does not break convexity.
    EXPECT_TRUE(isConvex(Polyline2{
        {Point2(0, 0), Point2(1, 0), Point2(2, 0), Point2(2, 2), Point2(0, 2)}, true}));
    EXPECT_FALSE(isConvex(Polyline2{{Point2(0, 0), Point2(1, 1), Point2(2, 2)}, true}));
}

TEST(PolygonSimplicity, DetectsSelfIntersection)
{
    EXPECT_TRUE(isSimple(square(2)));
    EXPECT_TRUE(isSimple(lShape()));
    const Polyline2 bowTie{{Point2(0, 0), Point2(2, 2), Point2(2, 0), Point2(0, 2)}, true};
    EXPECT_FALSE(isSimple(bowTie));
    const Polyline2 duplicateVertex{{Point2(0, 0), Point2(0, 0), Point2(2, 0), Point2(0, 2)}, true};
    EXPECT_FALSE(isSimple(duplicateVertex));
    const Polyline2 spike{{Point2(0, 0), Point2(4, 0), Point2(2, 0), Point2(2, 3)}, true};
    EXPECT_FALSE(isSimple(spike)); // edge doubles back over its neighbour
}

TEST(PolygonConvexHull, DropsInteriorCollinearAndDuplicatePoints)
{
    const auto hull = convexHull({Point2(0, 0), Point2(4, 0), Point2(4, 4), Point2(0, 4),
                                  Point2(2, 2), Point2(2, 0), Point2(4, 4), Point2(1, 3)});
    ASSERT_EQ(hull.size(), 4u);
    const Polyline2 polygon{hull, true};
    EXPECT_EQ(orientation(polygon), Orientation::CounterClockwise);
    EXPECT_DOUBLE_EQ(polygon.area(), 16.0);

    EXPECT_TRUE(convexHull({}).empty());
    EXPECT_EQ(convexHull({Point2(1, 1), Point2(1, 1)}).size(), 1u);
    EXPECT_EQ(convexHull({Point2(0, 0), Point2(1, 1), Point2(2, 2)}).size(), 2u); // collinear
}

TEST(PolygonConvexHull, ContainsEveryInputPoint)
{
    Random random;
    std::vector<Point2> points;
    for (int i = 0; i < 300; ++i) {
        points.emplace_back(random.real(-100, 100), random.real(-100, 100));
    }
    const Polyline2 hull{convexHull(points), true};
    EXPECT_TRUE(isConvex(hull));
    for (const Point2& p : points) {
        EXPECT_NE(hull.classify(p), Containment::Outside);
    }
}

TEST(PolygonSimplify, RemovesPointsWithinToleranceOnly)
{
    const Polyline2 noisy{{Point2(0, 0), Point2(1, 0.01), Point2(2, -0.01), Point2(3, 0),
                           Point2(3, 3)},
                          false};
    const Polyline2 simplified = simplify(noisy, 0.05);
    ASSERT_EQ(simplified.vertices.size(), 3u);
    EXPECT_EQ(simplified.vertices[0], Point2(0, 0)); // endpoints always survive
    EXPECT_EQ(simplified.vertices[1], Point2(3, 0));
    EXPECT_EQ(simplified.vertices[2], Point2(3, 3));

    EXPECT_EQ(simplify(noisy, 0.001).vertices.size(), 5u);
    EXPECT_EQ(simplify(noisy, 0.0), noisy);
    EXPECT_EQ(simplify(noisy, -1.0), noisy);
}

TEST(PolygonSimplify, ClosedRingStaysClosed)
{
    Polyline2 ring = square(10);
    ring.vertices.insert(ring.vertices.begin() + 1, Point2(5, 0.001)); // noise on the bottom edge
    const Polyline2 simplified = simplify(ring, 0.01);
    EXPECT_TRUE(simplified.closed);
    EXPECT_EQ(simplified.vertices.size(), 4u);
    EXPECT_NEAR(simplified.area(), 100.0, 1e-9);
}

TEST(PolygonClipSegment, LiangBarsky)
{
    const Box2 box(Point2(0, 0), Point2(10, 10));

    const auto through = clip(Segment2{Point2(-5, 5), Point2(15, 5)}, box);
    ASSERT_TRUE(through.has_value());
    EXPECT_EQ(*through, (Segment2{Point2(0, 5), Point2(10, 5)}));

    const auto inside = clip(Segment2{Point2(1, 1), Point2(2, 2)}, box);
    ASSERT_TRUE(inside.has_value());
    EXPECT_EQ(*inside, (Segment2{Point2(1, 1), Point2(2, 2)}));

    EXPECT_FALSE(clip(Segment2{Point2(-5, -5), Point2(-1, 20)}, box).has_value());
    EXPECT_FALSE(clip(Segment2{Point2(-5, 11), Point2(15, 11)}, box).has_value()); // parallel, outside
    EXPECT_TRUE(clip(Segment2{Point2(-5, 10), Point2(15, 10)}, box).has_value()); // along an edge
    EXPECT_FALSE(clip(Segment2{Point2(0, 0), Point2(1, 1)}, Box2{}).has_value());

    const auto diagonal = clip(Segment2{Point2(-5, -5), Point2(15, 15)}, box);
    ASSERT_TRUE(diagonal.has_value());
    EXPECT_TRUE(nearlyEqual(diagonal->start, Point2(0, 0)));
    EXPECT_TRUE(nearlyEqual(diagonal->end, Point2(10, 10)));
}

TEST(PolygonClipPolygon, SutherlandHodgman)
{
    Polyline2 window = square(4);
    Polyline2 subject = square(4);
    for (Point2& vertex : subject.vertices) {
        vertex += Vec2(2, 2);
    }
    const auto overlap = clipPolygon(subject, window);
    ASSERT_TRUE(overlap.ok());
    EXPECT_DOUBLE_EQ(overlap->area(), 4.0);

    // The clip window's orientation does not matter.
    const auto overlapCw = clipPolygon(subject, window.reversed());
    ASSERT_TRUE(overlapCw.ok());
    EXPECT_DOUBLE_EQ(overlapCw->area(), 4.0);

    for (Point2& vertex : subject.vertices) {
        vertex += Vec2(100, 100);
    }
    const auto disjoint = clipPolygon(subject, window);
    ASSERT_TRUE(disjoint.ok());
    EXPECT_TRUE(disjoint->vertices.empty());

    const auto concaveClip = clipPolygon(square(1), lShape());
    ASSERT_FALSE(concaveClip.ok());
    EXPECT_EQ(concaveClip.error().code, ErrorCode::InvalidGeometry);
}

TEST(PolygonTriangulate, ConvexAndConcavePolygonsPreserveArea)
{
    const auto squareTriangles = triangulate(square(2));
    ASSERT_TRUE(squareTriangles.ok());
    EXPECT_EQ(squareTriangles->size(), 2u);
    EXPECT_DOUBLE_EQ(triangleAreaSum(square(2), *squareTriangles), 4.0);

    const auto lTriangles = triangulate(lShape());
    ASSERT_TRUE(lTriangles.ok());
    EXPECT_EQ(lTriangles->size(), 4u);
    EXPECT_DOUBLE_EQ(triangleAreaSum(lShape(), *lTriangles), 3.0);

    // Clockwise input still produces counter-clockwise triangles.
    const Polyline2 clockwise = lShape().reversed();
    const auto cwTriangles = triangulate(clockwise);
    ASSERT_TRUE(cwTriangles.ok());
    EXPECT_DOUBLE_EQ(triangleAreaSum(clockwise, *cwTriangles), 3.0);
}

TEST(PolygonTriangulate, SkipsCollinearVerticesAndRejectsBadInput)
{
    const Polyline2 withMidpoints{{Point2(0, 0), Point2(1, 0), Point2(2, 0), Point2(2, 2),
                                   Point2(0, 2)},
                                  true};
    const auto triangles = triangulate(withMidpoints);
    ASSERT_TRUE(triangles.ok());
    EXPECT_DOUBLE_EQ(triangleAreaSum(withMidpoints, *triangles), 4.0);

    const auto collinear = triangulate(Polyline2{{Point2(0, 0), Point2(1, 1), Point2(2, 2)}, true});
    ASSERT_FALSE(collinear.ok());
    EXPECT_EQ(collinear.error().code, ErrorCode::TriangulationFailure);

    Polyline2 open = square(2);
    open.closed = false;
    EXPECT_FALSE(triangulate(open).ok());
}

TEST(PolygonTriangulate, RandomStarPolygonsPreserveArea)
{
    Random random;
    for (int i = 0; i < 200; ++i) {
        const int count = random.integer(3, 40);
        Polyline2 polygon;
        polygon.closed = true;
        for (int k = 0; k < count; ++k) {
            const double angle = kTwoPi * (k + random.real(0.1, 0.9)) / count;
            polygon.vertices.push_back(Point2(std::cos(angle), std::sin(angle)) *
                                       random.real(5.0, 50.0));
        }
        const auto triangles = triangulate(polygon);
        ASSERT_TRUE(triangles.ok()) << triangles.error().describe();
        EXPECT_NEAR(triangleAreaSum(polygon, *triangles), polygon.area(), 1e-9 * polygon.area());
    }
}

// ---- hatching --------------------------------------------------------------------

namespace {

Polyline2 closedPolygon(std::vector<Point2> vertices)
{
    Polyline2 polygon;
    polygon.vertices = std::move(vertices);
    polygon.closed = true;
    return polygon;
}

const Polyline2& tenMetreSquare()
{
    static const Polyline2 square =
        closedPolygon({Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)});
    return square;
}

} // namespace

TEST(Hatching, AHorizontalFamilyAcrossASquareIsWorkedOutExactly)
{
    // Spacing 2 over a square from y = 0 to y = 10. Lines sit at
    // offset + k * spacing, so k runs from ceil(0 / 2) = 0 to floor(10 / 2) = 5.
    //
    // The line at y = 10 produces NOTHING, and that is the half-open rule
    // working, not a defect: the top edge lies exactly on it, and an edge is
    // counted only when it starts on or below the line and ends strictly above
    // (or the reverse). So the bottom edge of a polygon is hatched and the top
    // edge is not, which is what stops two polygons sharing an edge from
    // drawing that line twice.
    //
    // Every coordinate here is exact in binary, so these are equalities.
    const auto lines = hatchLines(tenMetreSquare(), 0.0, 2.0);
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    ASSERT_EQ(lines.value().size(), 5u);
    for (std::size_t i = 0; i < lines.value().size(); ++i) {
        const Segment2& segment = lines.value()[i];
        const double y = 2.0 * static_cast<double>(i);
        EXPECT_EQ(segment.start.x, 0.0);
        EXPECT_EQ(segment.end.x, 10.0);
        EXPECT_EQ(segment.start.y, y);
        EXPECT_EQ(segment.end.y, y);
    }
}

TEST(Hatching, AConcaveBoundaryLeavesItsNotchUnfilled)
{
    // A U opening upward: two arms 3 m wide with a 4 m gap between them. A
    // horizontal line at y = 5 crosses the left arm over [0, 3] and the right
    // arm over [7, 10], and must NOT bridge the gap.
    //
    // Worked through by hand against the even-odd rule: the four edges that
    // cross y = 5 give parameters 0, 3, 7 and 10 along the line, which pair as
    // (0, 3) and (7, 10).
    const Polyline2 u =
        closedPolygon({Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(7, 10), Point2(7, 3),
                       Point2(3, 3), Point2(3, 10), Point2(0, 10)});
    const auto lines = hatchLines(u, 0.0, 5.0); // y = 0 and y = 5
    ASSERT_TRUE(lines.ok()) << lines.error().describe();

    std::vector<Segment2> atFive;
    for (const Segment2& segment : lines.value()) {
        if (segment.start.y == 5.0) {
            atFive.push_back(segment);
        }
    }
    ASSERT_EQ(atFive.size(), 2u) << "the notch was bridged or an arm was missed";
    EXPECT_EQ(atFive[0].start.x, 0.0);
    EXPECT_EQ(atFive[0].end.x, 3.0);
    EXPECT_EQ(atFive[1].start.x, 7.0);
    EXPECT_EQ(atFive[1].end.x, 10.0);
}

TEST(Hatching, TheFamilyIsAnchoredToTheWorldSoAdjacentAreasLineUp)
{
    // Two squares sharing the edge x = 10, hatched at 45 degrees with the same
    // spacing. The lines must continue across the shared edge, which is true
    // exactly when every line of both sits at offset + k * spacing measured
    // from the world origin rather than from each corner.
    const Polyline2 left = tenMetreSquare();
    const Polyline2 right =
        closedPolygon({Point2(10, 0), Point2(20, 0), Point2(20, 10), Point2(10, 10)});
    const double angle = std::atan(1.0); // 45 degrees
    const double spacing = 1.5;

    const auto leftLines = hatchLines(left, angle, spacing);
    const auto rightLines = hatchLines(right, angle, spacing);
    ASSERT_TRUE(leftLines.ok());
    ASSERT_TRUE(rightLines.ok());
    ASSERT_FALSE(leftLines.value().empty());
    ASSERT_FALSE(rightLines.value().empty());

    // Perpendicular offset of each segment from the origin, which must be an
    // exact multiple of the spacing for every segment of both squares.
    const double nx = -std::sin(angle);
    const double ny = std::cos(angle);
    std::size_t checked = 0;
    for (const auto* set : {&leftLines.value(), &rightLines.value()}) {
        for (const Segment2& segment : *set) {
            const double across = segment.start.x * nx + segment.start.y * ny;
            const double k = across / spacing;
            EXPECT_NEAR(k, std::round(k), 1e-9)
                << "a line sits between two of the family, at " << across;
            ++checked;
        }
    }
    EXPECT_GT(checked, 10u); // the property was tested on a real number of lines
}

TEST(Hatching, RotatingTheFamilyByAHalfTurnHatchesTheSameArea)
{
    // A family at angle t and one at t + pi are the same set of lines through
    // the origin, so the total hatched length must agree. This is the property
    // a sign error in the direction or the normal would break, and it holds
    // whatever the boundary.
    const Polyline2 blob =
        closedPolygon({Point2(1, 1), Point2(9, 2), Point2(11, 7), Point2(4, 9), Point2(0, 5)});
    const double spacing = 0.7;
    const auto total = [&](double angle) {
        const auto lines = hatchLines(blob, angle, spacing);
        EXPECT_TRUE(lines.ok());
        double sum = 0.0;
        for (const Segment2& segment : lines.value()) {
            sum += segment.length();
        }
        return sum;
    };
    const double forward = total(0.6);
    const double backward = total(0.6 + std::acos(-1.0));
    EXPECT_GT(forward, 1.0); // the case is not the empty one
    EXPECT_NEAR(forward, backward, 1e-9);
}

TEST(Hatching, RejectsBoundariesAndSpacingsItCannotHatch)
{
    Polyline2 open = tenMetreSquare();
    open.closed = false;
    EXPECT_EQ(hatchLines(open, 0.0, 1.0).error().code, ErrorCode::InvalidGeometry);

    const Polyline2 twoPoints = closedPolygon({Point2(0, 0), Point2(1, 1)});
    EXPECT_EQ(hatchLines(twoPoints, 0.0, 1.0).error().code, ErrorCode::InvalidGeometry);

    const Polyline2 nonFinite = closedPolygon(
        {Point2(0, 0), Point2(std::numeric_limits<double>::quiet_NaN(), 0), Point2(1, 1)});
    EXPECT_EQ(hatchLines(nonFinite, 0.0, 1.0).error().code, ErrorCode::InvalidGeometry);

    EXPECT_EQ(hatchLines(tenMetreSquare(), 0.0, 0.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(hatchLines(tenMetreSquare(), 0.0, -1.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(
        hatchLines(tenMetreSquare(), std::numeric_limits<double>::infinity(), 1.0).error().code,
        ErrorCode::InvalidArgument);
}

TEST(Hatching, ASpacingTooFineForTheBoundaryIsRefusedWithBothNumbers)
{
    // The mistake this catches is a pattern spacing entered in millimetres on a
    // drawing in metres. Refusing is the useful answer; drawing ten million
    // lines is not (PLAN.MD section 36).
    const auto refused = hatchLines(tenMetreSquare(), 0.0, 1e-6);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    const std::string message = refused.error().describe();
    EXPECT_NE(message.find("10000001"), std::string::npos) << message;
    EXPECT_NE(message.find(std::to_string(kMaxHatchLines)), std::string::npos) << message;
}

TEST(Hatching, ABoundaryThatFallsBetweenTwoLinesHatchesToNothing)
{
    // Not an error: a sliver narrower than the spacing simply has no line
    // through it, and a caller hatching a thousand parcels should not have to
    // treat that as a failure.
    const Polyline2 sliver =
        closedPolygon({Point2(0, 10.1), Point2(10, 10.1), Point2(10, 10.4), Point2(0, 10.4)});
    const auto lines = hatchLines(sliver, 0.0, 5.0);
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    EXPECT_TRUE(lines.value().empty());
}
