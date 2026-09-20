#include <gtest/gtest.h>

#include <cmath>

#include "katana/geometry/primitives2d.hpp"
#include "support/property.hpp"

using namespace katana::geometry;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::math::nearlyEqual;
using katana::test::kPropertyIterations;
using katana::test::Random;

// ---- Box2 ------------------------------------------------------------------

TEST(GeometryBox2, EmptyExpandContainIntersect)
{
    Box2 box;
    EXPECT_TRUE(box.empty());
    EXPECT_DOUBLE_EQ(box.width(), 0.0);
    box.expand(Point2(1, 2));
    box.expand(Point2(-3, 5));
    EXPECT_EQ(box, Box2(Point2(-3, 2), Point2(1, 5)));
    EXPECT_DOUBLE_EQ(box.width(), 4.0);
    EXPECT_DOUBLE_EQ(box.height(), 3.0);
    EXPECT_TRUE(box.contains(Point2(1, 5))); // boundary inclusive
    EXPECT_TRUE(box.intersects(Box2(Point2(1, 5), Point2(9, 9))));
    EXPECT_FALSE(box.intersects(Box2{}));
    EXPECT_TRUE(box.inflated(1.0).contains(Point2(2, 6)));
    EXPECT_TRUE(Box2{}.inflated(1.0).empty());
}

// ---- Line2 -----------------------------------------------------------------

TEST(GeometryLine2, ProjectionAndSignedDistance)
{
    const auto line = Line2::through(Point2(0, 0), Point2(10, 0));
    ASSERT_TRUE(line.has_value());
    EXPECT_EQ(line->closestPoint(Point2(3, 7)), Point2(3, 0));
    EXPECT_DOUBLE_EQ(line->signedDistanceTo(Point2(3, 7)), 7.0);   // left of +x
    EXPECT_DOUBLE_EQ(line->signedDistanceTo(Point2(3, -2)), -2.0); // right of +x
    EXPECT_DOUBLE_EQ(line->distanceTo(Point2(-50, -2)), 2.0);      // infinite extent
}

TEST(GeometryLine2, DuplicatePointsDoNotDefineALine)
{
    EXPECT_FALSE(Line2::through(Point2(1, 1), Point2(1, 1)).has_value());
    EXPECT_FALSE(Line2::through(Point2(1, 1), Point2(1 + 1e-9, 1)).has_value());
    EXPECT_TRUE(Line2::through(Point2(1, 1), Point2(1 + 1e-6, 1)).has_value());
}

// ---- Segment2 --------------------------------------------------------------

TEST(GeometrySegment2, LengthMidpointClosestPoint)
{
    const Segment2 segment{Point2(0, 0), Point2(10, 0)};
    EXPECT_DOUBLE_EQ(segment.length(), 10.0);
    EXPECT_EQ(segment.midpoint(), Point2(5, 0));
    EXPECT_EQ(segment.closestPoint(Point2(4, 3)), Point2(4, 0));
    EXPECT_EQ(segment.closestPoint(Point2(-5, 3)), Point2(0, 0)); // clamped to start
    EXPECT_EQ(segment.closestPoint(Point2(15, 3)), Point2(10, 0)); // clamped to end
    EXPECT_DOUBLE_EQ(segment.distanceTo(Point2(13, 4)), 5.0);
    EXPECT_EQ(segment.boundingBox(), Box2(Point2(0, 0), Point2(10, 0)));
}

TEST(GeometrySegment2, ZeroLengthSegmentBehavesAsAPoint)
{
    const Segment2 point{Point2(2, 3), Point2(2, 3)};
    EXPECT_TRUE(point.isDegenerate());
    EXPECT_DOUBLE_EQ(point.length(), 0.0);
    EXPECT_EQ(point.closestPoint(Point2(9, 9)), Point2(2, 3));
    EXPECT_DOUBLE_EQ(point.distanceTo(Point2(5, 7)), 5.0);
}

// ---- Circle2 ---------------------------------------------------------------

TEST(GeometryCircle2, MeasuresAndContainment)
{
    const Circle2 circle{Point2(1, 1), 2.0};
    EXPECT_DOUBLE_EQ(circle.area(), 4.0 * kPi);
    EXPECT_DOUBLE_EQ(circle.perimeter(), 4.0 * kPi);
    EXPECT_EQ(circle.classify(Point2(1, 1)), Containment::Inside);
    EXPECT_EQ(circle.classify(Point2(3, 1)), Containment::OnBoundary);
    EXPECT_EQ(circle.classify(Point2(3.001, 1)), Containment::Outside);
    EXPECT_EQ(circle.closestPoint(Point2(11, 1)), Point2(3, 1));
    EXPECT_EQ(circle.closestPoint(Point2(1, 1)), Point2(3, 1)); // centre: angle 0 by convention
    EXPECT_DOUBLE_EQ(circle.distanceTo(Point2(1, 1)), 2.0);     // to the curve, not the disc
    EXPECT_EQ(circle.boundingBox(), Box2(Point2(-1, -1), Point2(3, 3)));
}

// ---- Arc2 ------------------------------------------------------------------

TEST(GeometryArc2, EndpointsLengthAndDirection)
{
    const Arc2 ccw{Point2(0, 0), 2.0, 0.0, kHalfPi};
    EXPECT_TRUE(nearlyEqual(ccw.startPoint(), Point2(2, 0)));
    EXPECT_TRUE(nearlyEqual(ccw.endPoint(), Point2(0, 2)));
    EXPECT_DOUBLE_EQ(ccw.length(), kPi);

    const Arc2 cw = ccw.reversed();
    EXPECT_DOUBLE_EQ(cw.sweep, -kHalfPi);
    EXPECT_TRUE(nearlyEqual(cw.startPoint(), ccw.endPoint()));
    EXPECT_TRUE(nearlyEqual(cw.endPoint(), ccw.startPoint()));
    EXPECT_DOUBLE_EQ(cw.length(), ccw.length());
    EXPECT_TRUE(nearlyEqual(cw.midpoint(), ccw.midpoint()));
}

TEST(GeometryArc2, ContainsAngleHandlesWrapAndClockwiseSweeps)
{
    const Arc2 acrossZero{Point2(0, 0), 1.0, 1.75 * kPi, kHalfPi}; // 315 deg -> 45 deg
    EXPECT_TRUE(acrossZero.containsAngle(0.0));
    EXPECT_TRUE(acrossZero.containsAngle(kTwoPi));
    EXPECT_TRUE(acrossZero.containsAngle(-0.1));
    EXPECT_FALSE(acrossZero.containsAngle(kPi));

    const Arc2 clockwise{Point2(0, 0), 1.0, kHalfPi, -kHalfPi}; // 90 deg -> 0 deg
    EXPECT_TRUE(clockwise.containsAngle(0.25 * kPi));
    EXPECT_FALSE(clockwise.containsAngle(0.75 * kPi));
    EXPECT_FALSE(clockwise.containsAngle(kHalfPi + 1e-6));
    EXPECT_TRUE(clockwise.containsAngle(kHalfPi + 1e-6, 1e-5)); // widened by tolerance
}

TEST(GeometryArc2, ClosestPointClampsToTheNearerEnd)
{
    const Arc2 arc{Point2(0, 0), 1.0, 0.0, kHalfPi};
    EXPECT_TRUE(nearlyEqual(arc.closestPoint(Point2(5, 5)), arc.pointAt(0.5)));
    EXPECT_TRUE(nearlyEqual(arc.closestPoint(Point2(3, -1)), arc.startPoint()));
    EXPECT_TRUE(nearlyEqual(arc.closestPoint(Point2(-1, 3)), arc.endPoint()));
    EXPECT_TRUE(nearlyEqual(arc.closestPoint(Point2(0, 0)), arc.startPoint())); // centre
}

TEST(GeometryArc2, BoundingBoxIncludesCardinalExtremes)
{
    const Arc2 upperHalf{Point2(0, 0), 1.0, 0.0, kPi};
    const Box2 box = upperHalf.boundingBox();
    EXPECT_NEAR(box.min.x, -1.0, 1e-15);
    EXPECT_NEAR(box.max.x, 1.0, 1e-15);
    EXPECT_NEAR(box.min.y, 0.0, 1e-15);
    EXPECT_NEAR(box.max.y, 1.0, 1e-15); // the 90 degree extreme, not an endpoint
}

TEST(GeometryArc2, ThroughThreePointsRespectsTravelDirection)
{
    const auto ccw = Arc2::throughPoints(Point2(1, 0), Point2(0, 1), Point2(-1, 0));
    ASSERT_TRUE(ccw.has_value());
    EXPECT_TRUE(nearlyEqual(ccw->center, Point2(0, 0)));
    EXPECT_NEAR(ccw->radius, 1.0, 1e-15);
    EXPECT_NEAR(ccw->sweep, kPi, 1e-15);

    const auto cw = Arc2::throughPoints(Point2(1, 0), Point2(0, -1), Point2(-1, 0));
    ASSERT_TRUE(cw.has_value());
    EXPECT_NEAR(cw->sweep, -kPi, 1e-15);
    EXPECT_TRUE(nearlyEqual(cw->midpoint(), Point2(0, -1)));

    EXPECT_FALSE(Arc2::throughPoints(Point2(0, 0), Point2(1, 1), Point2(2, 2)).has_value());
    EXPECT_FALSE(Arc2::throughPoints(Point2(0, 0), Point2(0, 0), Point2(2, 2)).has_value());
}

// ---- Polyline2 -------------------------------------------------------------

TEST(GeometryPolyline2, OpenAndClosedLengthsAndSegments)
{
    Polyline2 path{{Point2(0, 0), Point2(3, 0), Point2(3, 4)}, false};
    EXPECT_EQ(path.segmentCount(), 2u);
    EXPECT_DOUBLE_EQ(path.length(), 7.0);
    EXPECT_DOUBLE_EQ(path.area(), 0.0); // open polylines bound no area
    EXPECT_FALSE(path.centroid().has_value());

    path.closed = true;
    EXPECT_EQ(path.segmentCount(), 3u);
    EXPECT_DOUBLE_EQ(path.length(), 12.0);
    EXPECT_DOUBLE_EQ(path.area(), 6.0);
    EXPECT_GT(path.signedArea(), 0.0); // counter-clockwise

    EXPECT_EQ((Polyline2{}).segmentCount(), 0u);
    EXPECT_EQ((Polyline2{{Point2(1, 1)}, false}).segmentCount(), 0u);
    EXPECT_FALSE((Polyline2{}).closestPoint(Point2(0, 0)).has_value());
}

TEST(GeometryPolyline2, AreaKeepsPrecisionAtProjectedCoordinates)
{
    // A 10 m square placed at typical UTM coordinates. A naive shoelace sum of
    // x*y products (~2.5e12) would lose the low-order digits of the 100 m^2 area.
    const Point2 origin(500000.0, 5000000.0);
    const Polyline2 parcel{{origin, origin + Vec2(10, 0), origin + Vec2(10, 10),
                            origin + Vec2(0, 10)},
                           true};
    EXPECT_DOUBLE_EQ(parcel.area(), 100.0);
    const auto centroid = parcel.centroid();
    ASSERT_TRUE(centroid.has_value());
    EXPECT_DOUBLE_EQ(centroid->x, 500005.0);
    EXPECT_DOUBLE_EQ(centroid->y, 5000005.0);
}

TEST(GeometryPolyline2, CentroidOfLShapeIsAreaWeighted)
{
    const Polyline2 lShape{{Point2(0, 0), Point2(2, 0), Point2(2, 1), Point2(1, 1), Point2(1, 2),
                            Point2(0, 2)},
                           true};
    EXPECT_DOUBLE_EQ(lShape.area(), 3.0);
    const auto centroid = lShape.centroid();
    ASSERT_TRUE(centroid.has_value());
    EXPECT_NEAR(centroid->x, 5.0 / 6.0, 1e-15);
    EXPECT_NEAR(centroid->y, 5.0 / 6.0, 1e-15);
}

TEST(GeometryPolyline2, DegeneratePolygonsHaveNoAreaOrCentroid)
{
    const Polyline2 collinear{{Point2(0, 0), Point2(1, 1), Point2(2, 2)}, true};
    EXPECT_DOUBLE_EQ(collinear.area(), 0.0);
    EXPECT_FALSE(collinear.centroid().has_value());

    const Polyline2 duplicates{{Point2(5, 5), Point2(5, 5), Point2(5, 5)}, true};
    EXPECT_DOUBLE_EQ(duplicates.area(), 0.0);
    EXPECT_FALSE(duplicates.centroid().has_value());
}

TEST(GeometryPolyline2, PointInPolygonIncludingVerticesAndConcavity)
{
    const Polyline2 lShape{{Point2(0, 0), Point2(2, 0), Point2(2, 1), Point2(1, 1), Point2(1, 2),
                            Point2(0, 2)},
                           true};
    EXPECT_EQ(lShape.classify(Point2(0.5, 0.5)), Containment::Inside);
    EXPECT_EQ(lShape.classify(Point2(1.5, 1.5)), Containment::Outside); // the notch
    EXPECT_EQ(lShape.classify(Point2(2, 0)), Containment::OnBoundary);  // vertex
    EXPECT_EQ(lShape.classify(Point2(1, 1.5)), Containment::OnBoundary); // edge
    EXPECT_EQ(lShape.classify(Point2(-1, 1)), Containment::Outside);
    // Horizontal ray from this point passes exactly through the vertex (1, 1).
    EXPECT_EQ(lShape.classify(Point2(0.5, 1.0)), Containment::Inside);
    EXPECT_EQ(lShape.classify(Point2(-0.5, 1.0)), Containment::Outside);

    Polyline2 open = lShape;
    open.closed = false;
    EXPECT_EQ(open.classify(Point2(0.5, 0.5)), Containment::Outside);
}

TEST(GeometryPolyline2, RemovesDuplicateVertices)
{
    const Polyline2 noisy{{Point2(0, 0), Point2(0, 0), Point2(1, 0), Point2(1, 1e-9),
                           Point2(1, 1), Point2(0, 0)},
                          true};
    const Polyline2 clean = noisy.withoutDuplicateVertices();
    ASSERT_EQ(clean.vertices.size(), 3u); // closing duplicate of the first vertex removed too
    EXPECT_EQ(clean.vertices[0], Point2(0, 0));
    EXPECT_EQ(clean.vertices[1], Point2(1, 0));
    EXPECT_EQ(clean.vertices[2], Point2(1, 1));
}

// ---- Rectangle2 / Triangle2 -------------------------------------------------

TEST(GeometryRectangle2, MeasuresAndContainment)
{
    const Rectangle2 rect = Rectangle2::fromCorners(Point2(4, 3), Point2(0, 0));
    EXPECT_EQ(rect.origin, Point2(0, 0));
    EXPECT_DOUBLE_EQ(rect.area(), 12.0);
    EXPECT_DOUBLE_EQ(rect.perimeter(), 14.0);
    EXPECT_EQ(rect.classify(Point2(2, 1)), Containment::Inside);
    EXPECT_EQ(rect.classify(Point2(4, 1)), Containment::OnBoundary);
    EXPECT_EQ(rect.classify(Point2(5, 1)), Containment::Outside);
    EXPECT_EQ(rect.closestPoint(Point2(2, 0.5)), Point2(2, 0)); // nearest boundary point
    EXPECT_DOUBLE_EQ(rect.distanceTo(Point2(7, 7)), 5.0);
    EXPECT_DOUBLE_EQ(rect.toPolyline().area(), 12.0);
}

TEST(GeometryTriangle2, MeasuresBarycentricAndCircumcircle)
{
    const Triangle2 triangle{Point2(0, 0), Point2(4, 0), Point2(0, 3)};
    EXPECT_DOUBLE_EQ(triangle.area(), 6.0);
    EXPECT_DOUBLE_EQ(triangle.perimeter(), 12.0);
    EXPECT_GT(triangle.signedArea(), 0.0);
    EXPECT_LT((Triangle2{triangle.a, triangle.c, triangle.b}).signedArea(), 0.0);
    EXPECT_TRUE(nearlyEqual(triangle.centroid(), Point2(4.0 / 3.0, 1.0)));

    EXPECT_EQ(triangle.classify(Point2(1, 1)), Containment::Inside);
    EXPECT_EQ(triangle.classify(Point2(2, 0)), Containment::OnBoundary);
    EXPECT_EQ(triangle.classify(Point2(4, 3)), Containment::Outside);

    const auto weights = triangle.barycentric(Point2(1, 1));
    ASSERT_TRUE(weights.has_value());
    EXPECT_NEAR((*weights)[0] + (*weights)[1] + (*weights)[2], 1.0, 1e-15);
    EXPECT_NEAR((*weights)[1], 0.25, 1e-15);
    EXPECT_NEAR((*weights)[2], 1.0 / 3.0, 1e-15);

    const auto circle = triangle.circumcircle(); // right triangle: hypotenuse is a diameter
    ASSERT_TRUE(circle.has_value());
    EXPECT_TRUE(nearlyEqual(circle->center, Point2(2.0, 1.5)));
    EXPECT_NEAR(circle->radius, 2.5, 1e-15);
}

TEST(GeometryTriangle2, DegenerateTrianglesAreRejected)
{
    const Triangle2 collinear{Point2(0, 0), Point2(1, 1), Point2(2, 2)};
    EXPECT_TRUE(collinear.isDegenerate());
    EXPECT_FALSE(collinear.barycentric(Point2(1, 1)).has_value());
    EXPECT_FALSE(collinear.circumcircle().has_value());
    EXPECT_TRUE((Triangle2{Point2(1, 1), Point2(1, 1), Point2(1, 1)}).isDegenerate());
    // A sliver thinner than the geometric tolerance is degenerate as well.
    EXPECT_TRUE((Triangle2{Point2(0, 0), Point2(100, 0), Point2(50, 1e-8)}).isDegenerate());
    EXPECT_FALSE((Triangle2{Point2(0, 0), Point2(100, 0), Point2(50, 1e-5)}).isDegenerate());
}

// ---- properties (PLAN.MD section 34) -----------------------------------------

namespace {

Polyline2 randomPolygon(Random& random, double originScale)
{
    // Star-shaped polygon: vertices at increasing angles around a centre, so it
    // is always simple.
    const Point2 center(random.real(-originScale, originScale),
                        random.real(-originScale, originScale));
    const int count = random.integer(3, 12);
    Polyline2 polygon;
    polygon.closed = true;
    for (int i = 0; i < count; ++i) {
        const double angle = kTwoPi * (i + random.real(0.1, 0.9)) / count;
        const double radius = random.real(5.0, 50.0);
        polygon.vertices.push_back(center + Vec2(std::cos(angle), std::sin(angle)) * radius);
    }
    return polygon;
}

} // namespace

TEST(GeometryProperties, ReversingAPolygonPreservesAreaAndLength)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Polyline2 polygon = randomPolygon(random, 1e6);
        const Polyline2 reversed = polygon.reversed();
        EXPECT_DOUBLE_EQ(reversed.area(), polygon.area());
        EXPECT_DOUBLE_EQ(reversed.signedArea(), -polygon.signedArea());
        EXPECT_TRUE(nearlyEqual(reversed.length(), polygon.length(), 1e-14));
    }
}

TEST(GeometryProperties, AreaAndCentroidAreTranslationInvariant)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Polyline2 local = randomPolygon(random, 10.0);
        const Vec2 shift(500000.0, 5000000.0);
        Polyline2 projected = local;
        for (Point2& vertex : projected.vertices) {
            vertex += shift;
        }
        EXPECT_NEAR(projected.area(), local.area(), 1e-6 * local.area());
        const auto a = local.centroid();
        const auto b = projected.centroid();
        ASSERT_TRUE(a.has_value() && b.has_value());
        EXPECT_NEAR(b->x - shift.x, a->x, 1e-6);
        EXPECT_NEAR(b->y - shift.y, a->y, 1e-6);
    }
}

TEST(GeometryProperties, CentroidOfAConvexPolygonLiesInside)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Point2 a(random.real(-100, 100), random.real(-100, 100));
        const Point2 b = a + Vec2(random.real(1, 50), random.real(-5, 5));
        const Point2 c = a + Vec2(random.real(-5, 5), random.real(1, 50));
        const Triangle2 triangle{a, b, c};
        if (!triangle.isDegenerate()) {
            EXPECT_EQ(triangle.classify(triangle.centroid()), Containment::Inside);
        }
    }
}

TEST(GeometryProperties, ClosestPointIsNeverFartherThanAnySampledPoint)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Segment2 segment{Point2(random.real(-100, 100), random.real(-100, 100)),
                               Point2(random.real(-100, 100), random.real(-100, 100))};
        const Point2 p(random.real(-200, 200), random.real(-200, 200));
        const double best = segment.distanceTo(p);
        for (int k = 0; k <= 10; ++k) {
            EXPECT_LE(best, segment.pointAt(k / 10.0).distanceTo(p) + 1e-12);
        }
    }
}
