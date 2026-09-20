#include <gtest/gtest.h>

#include <cmath>

#include "katana/geometry/intersection.hpp"
#include "support/property.hpp"

using namespace katana::geometry;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::nearlyEqual;
using katana::test::kPropertyIterations;
using katana::test::Random;

namespace {

void expectSinglePoint(const IntersectionResult& result, const Point2& expected)
{
    ASSERT_EQ(result.kind, IntersectionKind::Points);
    ASSERT_EQ(result.count, 1u);
    EXPECT_TRUE(nearlyEqual(result.points[0], expected))
        << result.points[0] << " vs " << expected;
}

} // namespace

// ---- lines -------------------------------------------------------------------

TEST(IntersectLines, CrossingParallelAndCoincident)
{
    const Line2 horizontal{Point2(0, 1), Vec2(1, 0)};
    expectSinglePoint(intersect(horizontal, Line2{Point2(3, -5), Vec2(0, 2)}), Point2(3, 1));

    EXPECT_EQ(intersect(horizontal, Line2{Point2(0, 2), Vec2(-4, 0)}).kind,
              IntersectionKind::None); // parallel, direction sign and length irrelevant
    EXPECT_EQ(intersect(horizontal, Line2{Point2(50, 1), Vec2(-4, 0)}).kind,
              IntersectionKind::Overlap); // coincident
}

TEST(IntersectLines, NearlyParallelLinesAreTreatedAsParallelBelowAngularTolerance)
{
    const Line2 base{Point2(0, 0), Vec2(1, 0)};
    // 1e-12 rad is below tolerance::kAngular: reported parallel, not a point 1e12 away.
    EXPECT_EQ(intersect(base, Line2{Point2(0, 1), Vec2(1, 1e-12)}).kind, IntersectionKind::None);
    // 1e-6 rad is a genuine (if shallow) crossing a million units away.
    const auto shallow = intersect(base, Line2{Point2(0, 1), Vec2(1, -1e-6)});
    ASSERT_EQ(shallow.kind, IntersectionKind::Points);
    EXPECT_NEAR(shallow.points[0].x, 1e6, 1e-3);
    EXPECT_NEAR(shallow.points[0].y, 0.0, 1e-9);
}

// ---- segments ----------------------------------------------------------------

TEST(IntersectSegments, ProperCrossing)
{
    expectSinglePoint(intersect(Segment2{Point2(0, 0), Point2(10, 10)},
                                Segment2{Point2(0, 10), Point2(10, 0)}),
                      Point2(5, 5));
}

TEST(IntersectSegments, DisjointEvenThoughSupportingLinesCross)
{
    EXPECT_FALSE(intersect(Segment2{Point2(0, 0), Point2(1, 1)},
                           Segment2{Point2(0, 10), Point2(10, 0)})
                     .exists());
}

TEST(IntersectSegments, EndpointTouchAndTJunction)
{
    expectSinglePoint(intersect(Segment2{Point2(0, 0), Point2(5, 0)},
                                Segment2{Point2(5, 0), Point2(5, 5)}),
                      Point2(5, 0)); // shared endpoint
    expectSinglePoint(intersect(Segment2{Point2(0, 0), Point2(10, 0)},
                                Segment2{Point2(4, 0), Point2(4, 7)}),
                      Point2(4, 0)); // T junction
}

TEST(IntersectSegments, ParallelSeparate)
{
    EXPECT_FALSE(intersect(Segment2{Point2(0, 0), Point2(10, 0)},
                           Segment2{Point2(0, 1), Point2(10, 1)})
                     .exists());
}

TEST(IntersectSegments, CollinearOverlapTouchAndGap)
{
    const Segment2 base{Point2(0, 0), Point2(10, 0)};

    const auto shared = intersect(base, Segment2{Point2(15, 0), Point2(6, 0)});
    ASSERT_EQ(shared.kind, IntersectionKind::Overlap);
    ASSERT_EQ(shared.count, 2u);
    EXPECT_TRUE(nearlyEqual(shared.points[0], Point2(6, 0))); // ordered along the first operand
    EXPECT_TRUE(nearlyEqual(shared.points[1], Point2(10, 0)));

    const auto contained = intersect(base, Segment2{Point2(2, 0), Point2(3, 0)});
    ASSERT_EQ(contained.kind, IntersectionKind::Overlap);
    EXPECT_TRUE(nearlyEqual(contained.points[0], Point2(2, 0)));
    EXPECT_TRUE(nearlyEqual(contained.points[1], Point2(3, 0)));

    expectSinglePoint(intersect(base, Segment2{Point2(10, 0), Point2(20, 0)}), Point2(10, 0));
    EXPECT_FALSE(intersect(base, Segment2{Point2(10.5, 0), Point2(20, 0)}).exists());
}

TEST(IntersectSegments, ZeroLengthSegments)
{
    const Segment2 base{Point2(0, 0), Point2(10, 0)};
    expectSinglePoint(intersect(base, Segment2{Point2(3, 0), Point2(3, 0)}), Point2(3, 0));
    expectSinglePoint(intersect(Segment2{Point2(3, 0), Point2(3, 0)}, base), Point2(3, 0));
    EXPECT_FALSE(intersect(base, Segment2{Point2(3, 1), Point2(3, 1)}).exists());
    expectSinglePoint(
        intersect(Segment2{Point2(1, 1), Point2(1, 1)}, Segment2{Point2(1, 1), Point2(1, 1)}),
        Point2(1, 1)); // duplicate points
    EXPECT_FALSE(
        intersect(Segment2{Point2(1, 1), Point2(1, 1)}, Segment2{Point2(2, 2), Point2(2, 2)})
            .exists());
}

TEST(IntersectSegments, WorksAtProjectedCoordinateMagnitudes)
{
    const Vec2 shift(500000.0, 5000000.0);
    expectSinglePoint(intersect(Segment2{Point2(0, 0) + shift, Point2(10, 10) + shift},
                                Segment2{Point2(0, 10) + shift, Point2(10, 0) + shift}),
                      Point2(5, 5) + shift);
}

TEST(IntersectLineSegment, HitMissAndCollinear)
{
    const Line2 vertical{Point2(2, 0), Vec2(0, 1)};
    expectSinglePoint(intersect(vertical, Segment2{Point2(0, 5), Point2(4, 5)}), Point2(2, 5));
    EXPECT_FALSE(intersect(vertical, Segment2{Point2(3, 5), Point2(4, 5)}).exists());
    EXPECT_EQ(intersect(vertical, Segment2{Point2(2, 1), Point2(2, 9)}).kind,
              IntersectionKind::Overlap);
}

// ---- circles -----------------------------------------------------------------

TEST(IntersectLineCircle, SecantTangentAndMiss)
{
    const Circle2 circle{Point2(0, 0), 5.0};

    const auto secant = intersect(Line2{Point2(-10, 3), Vec2(1, 0)}, circle);
    ASSERT_EQ(secant.count, 2u);
    EXPECT_FALSE(secant.tangent);
    EXPECT_TRUE(nearlyEqual(secant.points[0], Point2(-4, 3))); // ordered along the line
    EXPECT_TRUE(nearlyEqual(secant.points[1], Point2(4, 3)));

    const auto tangent = intersect(Line2{Point2(-10, 5), Vec2(1, 0)}, circle);
    ASSERT_EQ(tangent.count, 1u);
    EXPECT_TRUE(tangent.tangent);
    EXPECT_TRUE(nearlyEqual(tangent.points[0], Point2(0, 5)));

    EXPECT_FALSE(intersect(Line2{Point2(-10, 5.001), Vec2(1, 0)}, circle).exists());
}

TEST(IntersectSegmentCircle, OnlyPointsWithinTheSegmentCount)
{
    const Circle2 circle{Point2(0, 0), 5.0};
    expectSinglePoint(intersect(Segment2{Point2(0, 0), Point2(10, 0)}, circle), Point2(5, 0));
    EXPECT_FALSE(intersect(Segment2{Point2(0, 0), Point2(1, 0)}, circle).exists()); // inside
    EXPECT_EQ(intersect(Segment2{Point2(-10, 0), Point2(10, 0)}, circle).count, 2u);
}

TEST(IntersectCircles, TwoPointsTangentSeparateNestedAndCoincident)
{
    const Circle2 a{Point2(0, 0), 5.0};

    const auto crossing = intersect(a, Circle2{Point2(8, 0), 5.0});
    ASSERT_EQ(crossing.count, 2u);
    EXPECT_TRUE(nearlyEqual(crossing.points[0], Point2(4, -3)));
    EXPECT_TRUE(nearlyEqual(crossing.points[1], Point2(4, 3)));

    const auto external = intersect(a, Circle2{Point2(8, 0), 3.0});
    ASSERT_EQ(external.count, 1u);
    EXPECT_TRUE(external.tangent);
    EXPECT_TRUE(nearlyEqual(external.points[0], Point2(5, 0)));

    const auto internal = intersect(a, Circle2{Point2(3, 0), 2.0});
    ASSERT_EQ(internal.count, 1u);
    EXPECT_TRUE(internal.tangent);
    EXPECT_TRUE(nearlyEqual(internal.points[0], Point2(5, 0)));

    // Internal tangency seen from the smaller circle: contact is away from the big centre.
    const auto internalSwapped = intersect(Circle2{Point2(3, 0), 2.0}, a);
    ASSERT_EQ(internalSwapped.count, 1u);
    EXPECT_TRUE(nearlyEqual(internalSwapped.points[0], Point2(5, 0)));

    EXPECT_FALSE(intersect(a, Circle2{Point2(20, 0), 3.0}).exists()); // separate
    EXPECT_FALSE(intersect(a, Circle2{Point2(1, 0), 2.0}).exists());  // nested
    EXPECT_FALSE(intersect(a, Circle2{Point2(0, 0), 2.0}).exists());  // concentric
    EXPECT_EQ(intersect(a, a).kind, IntersectionKind::Overlap);       // coincident
}

// ---- arcs --------------------------------------------------------------------

TEST(IntersectArcs, SegmentAgainstArcKeepsOnlyPointsOnTheSweep)
{
    const Arc2 upperHalf{Point2(0, 0), 5.0, 0.0, kPi};
    expectSinglePoint(intersect(Segment2{Point2(0, -10), Point2(0, 10)}, upperHalf),
                      Point2(0, 5)); // the lower crossing is off the arc
    EXPECT_FALSE(intersect(Segment2{Point2(0, -10), Point2(0, -1)}, upperHalf).exists());
    EXPECT_EQ(intersect(Line2{Point2(-10, 3), Vec2(1, 0)}, upperHalf).count, 2u);
    // An arc endpoint counts as part of the arc.
    expectSinglePoint(intersect(Segment2{Point2(5, -1), Point2(5, 1)}, upperHalf), Point2(5, 0));
}

TEST(IntersectArcs, ArcAgainstArcAndCircle)
{
    const Arc2 upperHalf{Point2(0, 0), 5.0, 0.0, kPi};
    const Arc2 other{Point2(8, 0), 5.0, kHalfPi, kPi}; // left half of the second circle
    expectSinglePoint(intersect(upperHalf, other), Point2(4, 3));
    EXPECT_EQ(intersect(Circle2{Point2(8, 0), 5.0}, upperHalf).count, 1u);
}

TEST(IntersectArcs, ArcsOnTheSameCircle)
{
    const Arc2 first{Point2(0, 0), 5.0, 0.0, kHalfPi};
    EXPECT_EQ(intersect(first, first).kind, IntersectionKind::Overlap);
    EXPECT_EQ(intersect(first, first.reversed()).kind, IntersectionKind::Overlap);
    EXPECT_EQ(intersect(first, Arc2{Point2(0, 0), 5.0, 0.25 * kPi, kHalfPi}).kind,
              IntersectionKind::Overlap);

    // Meeting only at a shared end.
    expectSinglePoint(intersect(first, Arc2{Point2(0, 0), 5.0, kHalfPi, kHalfPi}), Point2(0, 5));

    // Complementary arcs share both ends but no interior.
    const auto complementary = intersect(first, Arc2{Point2(0, 0), 5.0, kHalfPi, 1.5 * kPi});
    EXPECT_EQ(complementary.kind, IntersectionKind::Points);
    EXPECT_EQ(complementary.count, 2u);

    EXPECT_FALSE(intersect(first, Arc2{Point2(0, 0), 5.0, kPi, kHalfPi}).exists());
}

// ---- properties --------------------------------------------------------------

TEST(IntersectProperties, ReportedPointsLieOnBothOperands)
{
    Random random;
    int hits = 0;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Segment2 segment{Point2(random.real(-20, 20), random.real(-20, 20)),
                               Point2(random.real(-20, 20), random.real(-20, 20))};
        const Circle2 circle{Point2(random.real(-10, 10), random.real(-10, 10)),
                             random.real(1, 15)};
        const auto result = intersect(segment, circle);
        for (std::size_t k = 0; k < result.count; ++k) {
            ++hits;
            EXPECT_LE(segment.distanceTo(result.points[k]), 1e-9);
            EXPECT_LE(circle.distanceTo(result.points[k]), 1e-9);
        }
    }
    EXPECT_GT(hits, 100); // the generator actually exercises intersections
}

TEST(IntersectProperties, SegmentIntersectionIsSymmetric)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Segment2 a{Point2(random.real(-20, 20), random.real(-20, 20)),
                         Point2(random.real(-20, 20), random.real(-20, 20))};
        const Segment2 b{Point2(random.real(-20, 20), random.real(-20, 20)),
                         Point2(random.real(-20, 20), random.real(-20, 20))};
        const auto ab = intersect(a, b);
        const auto ba = intersect(b, a);
        ASSERT_EQ(ab.kind, ba.kind);
        if (ab.kind == IntersectionKind::Points) {
            EXPECT_TRUE(nearlyEqual(ab.points[0], ba.points[0], 1e-9));
        }
    }
}
