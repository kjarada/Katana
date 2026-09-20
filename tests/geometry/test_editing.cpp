#include <gtest/gtest.h>

#include <cmath>
#include <vector>

#include "katana/geometry/editing.hpp"

using namespace katana::geometry;
using katana::core::ErrorCode;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::nearlyEqual;

// ---- offset ------------------------------------------------------------------

TEST(EditingOffset, SegmentOffsetsToTheLeftForPositiveDistance)
{
    const Segment2 east{Point2(0, 0), Point2(10, 0)};
    EXPECT_EQ(offset(east, 2.0), (Segment2{Point2(0, 2), Point2(10, 2)}));
    EXPECT_EQ(offset(east, -2.0), (Segment2{Point2(0, -2), Point2(10, -2)}));
    EXPECT_EQ(offset(east.reversed(), 2.0), (Segment2{Point2(10, -2), Point2(0, -2)}));
}

TEST(EditingOffset, CircleAndArcChangeRadiusAndRejectCollapse)
{
    const Circle2 circle{Point2(1, 1), 5.0};
    ASSERT_TRUE(offset(circle, 2.0).has_value());
    EXPECT_DOUBLE_EQ(offset(circle, 2.0)->radius, 7.0);
    EXPECT_DOUBLE_EQ(offset(circle, -2.0)->radius, 3.0);
    EXPECT_FALSE(offset(circle, -5.0).has_value());
    EXPECT_FALSE(offset(circle, -6.0).has_value());

    const Arc2 arc{Point2(0, 0), 5.0, 0.0, kHalfPi};
    const auto grown = offset(arc, 1.0);
    ASSERT_TRUE(grown.has_value());
    EXPECT_DOUBLE_EQ(grown->radius, 6.0);
    EXPECT_DOUBLE_EQ(grown->sweep, arc.sweep);
    EXPECT_FALSE(offset(arc, -5.0).has_value());
}

TEST(EditingOffset, PolylineUsesMitreJoins)
{
    const Polyline2 corner{{Point2(0, 0), Point2(10, 0), Point2(10, 10)}, false};
    const auto left = offset(corner, 1.0);
    ASSERT_TRUE(left.ok());
    ASSERT_EQ(left->vertices.size(), 3u);
    EXPECT_TRUE(nearlyEqual(left->vertices[0], Point2(0, 1)));
    EXPECT_TRUE(nearlyEqual(left->vertices[1], Point2(9, 1))); // inside of the turn
    EXPECT_TRUE(nearlyEqual(left->vertices[2], Point2(9, 10)));

    const auto right = offset(corner, -1.0);
    ASSERT_TRUE(right.ok());
    EXPECT_TRUE(nearlyEqual(right->vertices[1], Point2(11, -1))); // outside of the turn
}

TEST(EditingOffset, ClosedPolylineShrinksAndGrows)
{
    const Polyline2 square{{Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true};
    const auto inner = offset(square, 1.0); // left of a counter-clockwise ring is inside
    ASSERT_TRUE(inner.ok());
    EXPECT_NEAR(inner->area(), 64.0, 1e-12);
    const auto outer = offset(square, -1.0);
    ASSERT_TRUE(outer.ok());
    EXPECT_NEAR(outer->area(), 144.0, 1e-12);
}

TEST(EditingOffset, PolylineEdgeCases)
{
    // Collinear interior vertex: offset lines coincide, vertex is carried straight across.
    const Polyline2 straight{{Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false};
    const auto moved = offset(straight, 1.0);
    ASSERT_TRUE(moved.ok());
    EXPECT_TRUE(nearlyEqual(moved->vertices[1], Point2(5, 1)));

    // Duplicate vertices are ignored rather than producing NaN normals.
    const Polyline2 duplicated{{Point2(0, 0), Point2(0, 0), Point2(10, 0)}, false};
    const auto deduplicated = offset(duplicated, 1.0);
    ASSERT_TRUE(deduplicated.ok());
    EXPECT_EQ(deduplicated->vertices.size(), 2u);

    const auto tooShort = offset(Polyline2{{Point2(1, 1)}, false}, 1.0);
    ASSERT_FALSE(tooShort.ok());
    EXPECT_EQ(tooShort.error().code, ErrorCode::InvalidGeometry);

    // A path that doubles back has parallel, non-coincident offsets at the turn.
    const Polyline2 reversal{{Point2(0, 0), Point2(10, 0), Point2(5, 0)}, false};
    EXPECT_FALSE(offset(reversal, 1.0).ok());
}

TEST(EditingOffset, OffsetTowardsPicksTheSide)
{
    const Curve2 segment = Segment2{Point2(0, 0), Point2(10, 0)};
    const auto below = offsetTowards(segment, 2.0, Point2(5, -7));
    ASSERT_TRUE(below.ok());
    EXPECT_EQ(std::get<Segment2>(*below), (Segment2{Point2(0, -2), Point2(10, -2)}));

    const Curve2 circle = Circle2{Point2(0, 0), 5.0};
    const auto inward = offsetTowards(circle, 2.0, Point2(1, 0));
    ASSERT_TRUE(inward.ok());
    EXPECT_DOUBLE_EQ(std::get<Circle2>(*inward).radius, 3.0);
    const auto outward = offsetTowards(circle, -2.0, Point2(9, 0)); // sign of distance ignored
    ASSERT_TRUE(outward.ok());
    EXPECT_DOUBLE_EQ(std::get<Circle2>(*outward).radius, 7.0);

    EXPECT_FALSE(offsetTowards(circle, 6.0, Point2(1, 0)).ok()); // would collapse
    EXPECT_FALSE(offsetTowards(segment, 0.0, Point2(1, 1)).ok());
}

// ---- trim --------------------------------------------------------------------

TEST(EditingTrim, SegmentBetweenTwoCuttersLosesItsMiddle)
{
    const Curve2 target = Segment2{Point2(0, 0), Point2(10, 0)};
    const std::vector<Curve2> cutters = {Segment2{Point2(3, -1), Point2(3, 1)},
                                         Segment2{Point2(7, -1), Point2(7, 1)}};
    const auto result = trim(target, cutters, Point2(5, 0.2));
    ASSERT_TRUE(result.ok());
    ASSERT_EQ(result->remaining.size(), 2u);
    EXPECT_EQ(std::get<Segment2>(result->remaining[0]), (Segment2{Point2(0, 0), Point2(3, 0)}));
    EXPECT_EQ(std::get<Segment2>(result->remaining[1]), (Segment2{Point2(7, 0), Point2(10, 0)}));
}

TEST(EditingTrim, SegmentEndIsRemovedUpToTheNearestCutter)
{
    const Curve2 target = Segment2{Point2(0, 0), Point2(10, 0)};
    const std::vector<Curve2> cutters = {Segment2{Point2(3, -1), Point2(3, 1)},
                                         Circle2{Point2(7, 0), 1.0}}; // crosses at x = 6 and 8
    const auto result = trim(target, cutters, Point2(9.5, 0));
    ASSERT_TRUE(result.ok());
    ASSERT_EQ(result->remaining.size(), 1u);
    const auto& kept = std::get<Segment2>(result->remaining[0]);
    EXPECT_TRUE(nearlyEqual(kept.start, Point2(0, 0)));
    EXPECT_TRUE(nearlyEqual(kept.end, Point2(8, 0)));
}

TEST(EditingTrim, FailsWhenNothingCrossesTheTarget)
{
    const Curve2 target = Segment2{Point2(0, 0), Point2(10, 0)};
    const std::vector<Curve2> missing = {Segment2{Point2(3, 5), Point2(3, 6)}};
    const auto result = trim(target, missing, Point2(5, 0));
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidGeometry);

    // A cutter meeting the target exactly at its endpoint bounds nothing.
    const std::vector<Curve2> atEnd = {Segment2{Point2(10, -1), Point2(10, 1)}};
    EXPECT_FALSE(trim(target, atEnd, Point2(5, 0)).ok());
    EXPECT_FALSE(trim(target, {}, Point2(5, 0)).ok());
}

TEST(EditingTrim, ArcIsCutAtItsCrossing)
{
    const Curve2 target = Arc2{Point2(0, 0), 5.0, 0.0, kPi}; // upper half
    const std::vector<Curve2> cutters = {Segment2{Point2(0, 0), Point2(0, 10)}};
    const auto result = trim(target, cutters, Point2(-4, 3)); // pick on the left quarter
    ASSERT_TRUE(result.ok());
    ASSERT_EQ(result->remaining.size(), 1u);
    const auto& kept = std::get<Arc2>(result->remaining[0]);
    EXPECT_NEAR(kept.startAngle, 0.0, 1e-12);
    EXPECT_NEAR(kept.sweep, kHalfPi, 1e-12);
}

TEST(EditingTrim, CircleBecomesAnArc)
{
    const Curve2 target = Circle2{Point2(0, 0), 5.0};
    const std::vector<Curve2> cutters = {Segment2{Point2(-10, 0), Point2(10, 0)}};

    const auto upperRemoved = trim(target, cutters, Point2(0, 5));
    ASSERT_TRUE(upperRemoved.ok());
    const auto& lower = std::get<Arc2>(upperRemoved->remaining.at(0));
    EXPECT_NEAR(lower.startAngle, kPi, 1e-12);
    EXPECT_NEAR(lower.sweep, kPi, 1e-12);
    EXPECT_TRUE(nearlyEqual(lower.midpoint(), Point2(0, -5)));

    const auto lowerRemoved = trim(target, cutters, Point2(0, -5));
    ASSERT_TRUE(lowerRemoved.ok());
    EXPECT_TRUE(nearlyEqual(std::get<Arc2>(lowerRemoved->remaining.at(0)).midpoint(), Point2(0, 5)));

    // A tangent line touches once: not enough to bound a gap.
    const std::vector<Curve2> tangent = {Segment2{Point2(-10, 5), Point2(10, 5)}};
    EXPECT_FALSE(trim(target, tangent, Point2(0, 5)).ok());
}

// ---- extend ------------------------------------------------------------------

TEST(EditingExtend, SegmentReachesTheFirstBoundaryAhead)
{
    const Curve2 target = Segment2{Point2(0, 0), Point2(4, 0)};
    const std::vector<Curve2> boundaries = {Segment2{Point2(10, -5), Point2(10, 5)},
                                            Segment2{Point2(7, -5), Point2(7, 5)},
                                            Segment2{Point2(-3, -5), Point2(-3, 5)}};
    const auto forward = extend(target, boundaries, Point2(3.9, 0));
    ASSERT_TRUE(forward.ok());
    EXPECT_EQ(std::get<Segment2>(*forward), (Segment2{Point2(0, 0), Point2(7, 0)}));

    const auto backward = extend(target, boundaries, Point2(0.1, 0));
    ASSERT_TRUE(backward.ok());
    EXPECT_EQ(std::get<Segment2>(*backward), (Segment2{Point2(-3, 0), Point2(4, 0)}));
}

TEST(EditingExtend, FailsWithoutABoundaryAhead)
{
    const Curve2 target = Segment2{Point2(0, 0), Point2(4, 0)};
    const std::vector<Curve2> behindOnly = {Segment2{Point2(-3, -5), Point2(-3, 5)}};
    EXPECT_FALSE(extend(target, behindOnly, Point2(4, 0)).ok());
    // A boundary the segment already touches does not count as "ahead".
    const std::vector<Curve2> touching = {Segment2{Point2(4, -5), Point2(4, 5)}};
    EXPECT_FALSE(extend(target, touching, Point2(4, 0)).ok());
    EXPECT_FALSE(extend(Curve2{Circle2{Point2(0, 0), 1.0}}, touching, Point2(1, 0)).ok());
}

TEST(EditingExtend, ArcGrowsAlongItsCircle)
{
    const Curve2 target = Arc2{Point2(0, 0), 5.0, 0.0, kHalfPi}; // from (5,0) to (0,5)
    const std::vector<Curve2> boundaries = {Segment2{Point2(-10, 0), Point2(10, 0)}};

    const auto grownEnd = extend(target, boundaries, Point2(0, 5));
    ASSERT_TRUE(grownEnd.ok());
    EXPECT_NEAR(std::get<Arc2>(*grownEnd).sweep, kPi, 1e-12); // reaches (-5, 0)
    EXPECT_NEAR(std::get<Arc2>(*grownEnd).startAngle, 0.0, 1e-12);

    // The start already lies on the boundary; extending it goes on to the next crossing.
    const auto grownStart = extend(target, boundaries, Point2(5, 0));
    ASSERT_TRUE(grownStart.ok());
    EXPECT_TRUE(nearlyEqual(std::get<Arc2>(*grownStart).startPoint(), Point2(-5, 0)));
    EXPECT_TRUE(nearlyEqual(std::get<Arc2>(*grownStart).endPoint(), Point2(0, 5)));
    EXPECT_NEAR(std::get<Arc2>(*grownStart).sweep, 1.5 * kPi, 1e-12);
}

// ---- fillet / chamfer --------------------------------------------------------

TEST(EditingFillet, RightAngleCornerIsTangentToBothSegments)
{
    const Segment2 first{Point2(10, 0), Point2(0, 0)}; // ends at the corner
    const Segment2 second{Point2(0, 0), Point2(0, 10)}; // starts at the corner
    const auto result = fillet(first, second, 2.0);
    ASSERT_TRUE(result.ok());
    // Direction of each input is preserved. The tangent points come from
    // r / tan(pi/4), which is one ulp away from 2, so compare with tolerance.
    EXPECT_EQ(result->first.start, Point2(10, 0));
    EXPECT_TRUE(nearlyEqual(result->first.end, Point2(2, 0)));
    EXPECT_TRUE(nearlyEqual(result->second.start, Point2(0, 2)));
    EXPECT_EQ(result->second.end, Point2(0, 10));
    ASSERT_TRUE(result->arc.has_value());
    EXPECT_TRUE(nearlyEqual(result->arc->center, Point2(2, 2)));
    EXPECT_DOUBLE_EQ(result->arc->radius, 2.0);
    EXPECT_TRUE(nearlyEqual(result->arc->startPoint(), Point2(2, 0)));
    EXPECT_TRUE(nearlyEqual(result->arc->endPoint(), Point2(0, 2)));
    EXPECT_NEAR(std::abs(result->arc->sweep), kHalfPi, 1e-12); // the short way round
    EXPECT_NEAR(result->arc->midpoint().distanceTo(Point2(0, 0)), 2.0 * std::sqrt(2.0) - 2.0,
                1e-12);
}

TEST(EditingFillet, ExtendsSegmentsThatStopShortOfTheCorner)
{
    const Segment2 first{Point2(10, 0), Point2(5, 0)};
    const Segment2 second{Point2(0, 5), Point2(0, 10)};
    const auto sharp = fillet(first, second, 0.0);
    ASSERT_TRUE(sharp.ok());
    EXPECT_FALSE(sharp->arc.has_value());
    EXPECT_EQ(sharp->first, (Segment2{Point2(10, 0), Point2(0, 0)}));
    EXPECT_EQ(sharp->second, (Segment2{Point2(0, 0), Point2(0, 10)}));
}

TEST(EditingFillet, RejectsParallelOversizedAndNegative)
{
    const Segment2 first{Point2(0, 0), Point2(10, 0)};
    EXPECT_FALSE(fillet(first, Segment2{Point2(0, 5), Point2(10, 5)}, 1.0).ok());     // parallel
    EXPECT_FALSE(fillet(first, Segment2{Point2(0, 0), Point2(0, 10)}, 50.0).ok());    // too large
    EXPECT_FALSE(fillet(first, Segment2{Point2(0, 0), Point2(0, 10)}, -1.0).ok());    // negative
    EXPECT_FALSE(fillet(first, Segment2{Point2(3, 3), Point2(3, 3)}, 1.0).ok());      // degenerate
    EXPECT_EQ(fillet(first, Segment2{Point2(0, 0), Point2(0, 10)}, 50.0).error().code,
              ErrorCode::InvalidGeometry);
}

TEST(EditingFillet, AcuteCornerGeometry)
{
    // 60 degree corner at the origin: tangent length = r / tan(30 deg).
    const Segment2 first{Point2(0, 0), Point2(10, 0)};
    const Segment2 second{Point2(0, 0), Point2(5, 5 * std::sqrt(3.0))};
    const auto result = fillet(first, second, 1.0);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->first.start.x, std::sqrt(3.0), 1e-12);
    ASSERT_TRUE(result->arc.has_value());
    EXPECT_NEAR(result->arc->center.distanceTo(Point2(0, 0)), 2.0, 1e-12); // r / sin(30 deg)
    EXPECT_NEAR(std::abs(result->arc->sweep), kPi - kPi / 3.0, 1e-12);
    // Tangency: the centre is exactly one radius from both supporting lines.
    EXPECT_NEAR((Line2{first.start, first.delta()}).distanceTo(result->arc->center), 1.0, 1e-12);
    EXPECT_NEAR((Line2{second.start, second.delta()}).distanceTo(result->arc->center), 1.0, 1e-12);
}

TEST(EditingChamfer, CutsBothSegmentsAndAddsABevel)
{
    const Segment2 first{Point2(10, 0), Point2(0, 0)};
    const Segment2 second{Point2(0, 0), Point2(0, 10)};
    const auto result = chamfer(first, second, 2.0, 3.0);
    ASSERT_TRUE(result.ok());
    EXPECT_EQ(result->first, (Segment2{Point2(10, 0), Point2(2, 0)}));
    EXPECT_EQ(result->second, (Segment2{Point2(0, 3), Point2(0, 10)}));
    ASSERT_TRUE(result->bevel.has_value());
    EXPECT_EQ(*result->bevel, (Segment2{Point2(2, 0), Point2(0, 3)}));

    const auto sharp = chamfer(first, second, 0.0, 0.0);
    ASSERT_TRUE(sharp.ok());
    EXPECT_FALSE(sharp->bevel.has_value());

    EXPECT_FALSE(chamfer(first, second, 11.0, 1.0).ok());
    EXPECT_FALSE(chamfer(first, second, -1.0, 1.0).ok());
}

// ---- degenerate results --------------------------------------------------------------------

// Regression. The guard tested whether the cut OVERSHOT the segment, so a cut
// that exactly consumed it was allowed through and produced a zero-length
// remainder. The model then rejected that with "line has zero length" - an
// error about the line, when what is wrong is the distance the user typed.
// Chamfering two 10 m lines with a distance of 10 is an ordinary thing to try.
TEST(EditingDegenerate, ACutThatConsumesTheWholeSegmentIsRefused)
{
    const Segment2 first{Point2(0, 0), Point2(10, 0)};
    const Segment2 second{Point2(0, 0), Point2(0, 10)};

    // Exactly the segment length: nothing would be left.
    const auto exact = chamfer(first, second, 10.0, 10.0);
    ASSERT_FALSE(exact.ok());
    EXPECT_EQ(exact.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(exact.error().message.find("segment"), std::string::npos)
        << "the message should be about the distance, not about the line: "
        << exact.error().message;

    // Just inside the tolerance of consuming it: still refused, because the
    // remainder would be shorter than anything the model accepts.
    EXPECT_FALSE(chamfer(first, second, 10.0 - 0.5 * katana::math::tolerance::kGeometric, 1.0).ok());

    // Comfortably shorter: accepted, and the remainders are real segments.
    const auto ok = chamfer(first, second, 3.0, 3.0);
    ASSERT_TRUE(ok.ok()) << ok.error().describe();
    EXPECT_GT(ok->first.length(), 0.0);
    EXPECT_GT(ok->second.length(), 0.0);
}

TEST(EditingDegenerate, AFilletThatConsumesTheWholeSegmentIsRefused)
{
    const Segment2 first{Point2(0, 0), Point2(10, 0)};
    const Segment2 second{Point2(0, 0), Point2(0, 10)};

    // A right angle, so the tangent distance equals the radius: r = 10 consumes
    // both segments exactly.
    const auto exact = fillet(first, second, 10.0);
    ASSERT_FALSE(exact.ok());
    EXPECT_EQ(exact.error().code, ErrorCode::InvalidGeometry);

    const auto ok = fillet(first, second, 2.0);
    ASSERT_TRUE(ok.ok()) << ok.error().describe();
    EXPECT_GT(ok->first.length(), 0.0);
    EXPECT_GT(ok->second.length(), 0.0);
    // The arc centre sits one radius from each supporting line.
    ASSERT_TRUE(ok->arc.has_value());
    EXPECT_NEAR(ok->arc->center.x, 2.0, 1e-12);
    EXPECT_NEAR(ok->arc->center.y, 2.0, 1e-12);
}

// Collinear segments have no corner, and the tangent distance is infinite.
// A plain `tangentDistance > reach` comparison lets an infinity through, so the
// negated form is what keeps this an error rather than a NaN-filled arc.
TEST(EditingDegenerate, CollinearSegmentsCannotBeFilleted)
{
    const Segment2 first{Point2(0, 0), Point2(10, 0)};
    const Segment2 collinear{Point2(10, 0), Point2(20, 0)};
    const auto result = fillet(first, collinear, 1.0);
    EXPECT_FALSE(result.ok()) << "there is no corner to fillet";
}
