// The drawing curves (include/katana/geometry/curves2d.hpp): the polyline with
// arc segments and heights, the ellipse, and the spline, each against values
// worked out by hand.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/geometry/curves2d.hpp"
#include "katana/math/numerics.hpp"

using katana::geometry::Arc2;
using katana::geometry::arcFromBulge;
using katana::geometry::bulgeFromSweep;
using katana::geometry::bulgeThrough;
using katana::geometry::CurvePolyline2;
using katana::geometry::CurveVertex;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Spline2;
using katana::geometry::Vec2;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;

namespace {

void expectNear(const Point2& a, const Point2& b, double tolerance = 1e-9)
{
    EXPECT_NEAR(a.x, b.x, tolerance) << "x";
    EXPECT_NEAR(a.y, b.y, tolerance) << "y";
}

// A 10 x 10 square whose top edge is a semicircle bulging upward, drawn
// counter-clockwise from the origin.
CurvePolyline2 archedSquare()
{
    CurvePolyline2 p;
    p.closed = true;
    p.vertices = {CurveVertex{Point2(0, 0), 0.0, 1.0}, CurveVertex{Point2(10, 0), 0.0, 2.0},
                  // From (10,10) to (0,10), a positive bulge: a counter-
                  // clockwise arc, which bulges to the RIGHT of its chord -
                  // up, away from the square.
                  CurveVertex{Point2(10, 10), 1.0, 3.0}, CurveVertex{Point2(0, 10), 0.0, 4.0}};
    return p;
}

} // namespace

// ---- bulges --------------------------------------------------------------------------

TEST(CurvePolyline, APositiveUnitBulgeIsACounterClockwiseSemicircle)
{
    const auto arc = arcFromBulge(Point2(0, 0), Point2(2, 0), 1.0);
    ASSERT_TRUE(arc.has_value());
    expectNear(arc->center, Point2(1, 0));
    EXPECT_NEAR(arc->radius, 1.0, 1e-12);
    EXPECT_NEAR(arc->sweep, kPi, 1e-12);
    expectNear(arc->startPoint(), Point2(0, 0));
    expectNear(arc->endPoint(), Point2(2, 0));
    // Counter-clockwise from the west end of the circle passes through its
    // south end.
    expectNear(arc->midpoint(), Point2(1, -1));
}

TEST(CurvePolyline, AQuarterBulgeGivesTheCircleThroughTheEnds)
{
    // A quarter circle of radius 5 about the origin from (5,0) to (0,5):
    // sweep pi/2, bulge tan(pi/8).
    const double bulge = std::tan(kPi / 8.0);
    const auto arc = arcFromBulge(Point2(5, 0), Point2(0, 5), bulge);
    ASSERT_TRUE(arc.has_value());
    expectNear(arc->center, Point2(0, 0));
    EXPECT_NEAR(arc->radius, 5.0, 1e-12);
    EXPECT_NEAR(arc->sweep, kHalfPi, 1e-12);
    EXPECT_NEAR(bulgeFromSweep(kHalfPi), bulge, 1e-15);
    EXPECT_NEAR(bulgeThrough(Point2(5, 0), Point2(5 * std::sqrt(0.5), 5 * std::sqrt(0.5)),
                             Point2(0, 5)),
                bulge, 1e-12);
    // A negative bulge is the same circle's other side: clockwise, centre
    // mirrored across the chord.
    const auto other = arcFromBulge(Point2(5, 0), Point2(0, 5), -bulge);
    ASSERT_TRUE(other.has_value());
    expectNear(other->center, Point2(5, 5));
    EXPECT_NEAR(other->sweep, -kHalfPi, 1e-12);
}

TEST(CurvePolyline, AZeroBulgeOrCoincidentEndsIsStraight)
{
    EXPECT_FALSE(arcFromBulge(Point2(0, 0), Point2(1, 0), 0.0).has_value());
    EXPECT_FALSE(arcFromBulge(Point2(1, 1), Point2(1, 1), 0.5).has_value());
    EXPECT_EQ(bulgeThrough(Point2(0, 0), Point2(1, 0), Point2(2, 0)), 0.0);
}

// ---- measures -------------------------------------------------------------------------

TEST(CurvePolyline, LengthAndAreaCountTheArcs)
{
    const CurvePolyline2 p = archedSquare();
    ASSERT_EQ(p.segmentCount(), 4u);
    EXPECT_TRUE(p.hasArcs());
    EXPECT_TRUE(p.isArc(2));
    EXPECT_FALSE(p.isArc(0));
    // Three straight sides and a semicircle of radius 5.
    EXPECT_NEAR(p.length(), 30.0 + 5.0 * kPi, 1e-9);
    // The square plus a half disc of radius 5.
    EXPECT_NEAR(p.signedArea(), 100.0 + 0.5 * kPi * 25.0, 1e-9);
    // Reversed, the ring runs clockwise: the same area negated and the same
    // length - and the arc still bulges up.
    const CurvePolyline2 back = p.reversed();
    EXPECT_NEAR(back.signedArea(), -(100.0 + 0.5 * kPi * 25.0), 1e-9);
    EXPECT_NEAR(back.length(), p.length(), 1e-9);
    EXPECT_NEAR(back.boundingBox().max.y, 15.0, 1e-9);
}

TEST(CurvePolyline, TheBoundingBoxHoldsTheArcNotOnlyItsEnds)
{
    const auto box = archedSquare().boundingBox();
    EXPECT_NEAR(box.min.x, 0.0, 1e-12);
    EXPECT_NEAR(box.max.x, 10.0, 1e-12);
    EXPECT_NEAR(box.min.y, 0.0, 1e-12);
    EXPECT_NEAR(box.max.y, 15.0, 1e-9);
}

TEST(CurvePolyline, NearestFindsStationsOnLinesAndArcs)
{
    const CurvePolyline2 p = archedSquare();
    const auto onBase = p.nearest(Point2(4, -3));
    ASSERT_TRUE(onBase.has_value());
    expectNear(onBase->point, Point2(4, 0));
    EXPECT_NEAR(onBase->station, 4.0, 1e-9);
    EXPECT_EQ(onBase->segment, 0u);
    EXPECT_NEAR(onBase->distance, 3.0, 1e-12);

    // Above the top of the arch: the crown, halfway round the semicircle.
    const auto crown = p.nearest(Point2(5, 20));
    ASSERT_TRUE(crown.has_value());
    expectNear(crown->point, Point2(5, 15));
    EXPECT_NEAR(crown->station, 20.0 + 2.5 * kPi, 1e-9);
    expectNear(p.pointAtStation(crown->station), Point2(5, 15));
}

TEST(CurvePolyline, HeightsInterpolateByLengthAlongTheSegment)
{
    const CurvePolyline2 p = archedSquare();
    EXPECT_NEAR(*p.heightAtStation(2.5), 1.25, 1e-12);
    // Halfway round the arc from height 3 to height 4.
    EXPECT_NEAR(*p.heightAtStation(20.0 + 2.5 * kPi), 3.5, 1e-9);
    CurvePolyline2 gap = p;
    gap.vertices[1].height.reset();
    EXPECT_FALSE(gap.heightAtStation(2.5).has_value());
    EXPECT_TRUE(p.hasHeights());
    EXPECT_FALSE(CurvePolyline2::fromPoints({Point2(0, 0), Point2(1, 0)}).hasHeights());
}

TEST(CurvePolyline, TessellationEndsOnTheVerticesAndHoldsTheTolerance)
{
    const CurvePolyline2 p = archedSquare();
    const auto chain = p.tessellate(0.01);
    ASSERT_GT(chain.size(), 6u);
    EXPECT_EQ(chain.front(), Point2(0, 0));
    EXPECT_EQ(chain.back(), Point2(0, 0)) << "a closed walk returns to its start";
    for (const Point2& q : chain) {
        const double onArc = std::abs(q.distanceTo(Point2(5, 10)) - 5.0);
        const double onSide = std::min({std::abs(q.y), std::abs(q.x), std::abs(q.x - 10.0)});
        EXPECT_LT(std::min(onArc, onSide), 0.011);
    }
    const Polyline2 flat = p.toPolyline(0.01);
    EXPECT_TRUE(flat.closed);
    EXPECT_EQ(flat.vertices.size(), chain.size() - 1);
    // With no arcs the conversion is exact.
    const Polyline2 square{{Point2(0, 0), Point2(1, 0), Point2(1, 1)}, true};
    EXPECT_EQ(CurvePolyline2::fromPolyline(square).toPolyline(0.01), square);
}

// ---- ellipse ----------------------------------------------------------------------------

TEST(Ellipse, PointsFollowTheEccentricAnomaly)
{
    Ellipse2 e;
    e.center = Point2(1, 2);
    e.majorAxis = Vec2(4, 0);
    e.ratio = 0.5;
    expectNear(e.pointAtParameter(0.0), Point2(5, 2));
    expectNear(e.pointAtParameter(kHalfPi), Point2(1, 4));
    expectNear(e.pointAtParameter(kPi), Point2(-3, 2));
    EXPECT_TRUE(e.isFull());
    EXPECT_EQ(e.quadrants().size(), 4u);
    const auto box = e.boundingBox();
    expectNear(box.min, Point2(-3, 0));
    expectNear(box.max, Point2(5, 4));
    EXPECT_NEAR(e.parameterTowards(Point2(1, 10)), kHalfPi, 1e-12);
}

TEST(Ellipse, TheLengthOfACircleAndOfAnEllipseAgreeWithTheirFormulae)
{
    Ellipse2 circle;
    circle.majorAxis = Vec2(3, 4); // radius 5, tilted
    circle.ratio = 1.0;
    EXPECT_NEAR(circle.length(), kTwoPi * 5.0, 1e-9);

    Ellipse2 e;
    e.majorAxis = Vec2(10, 0);
    e.ratio = 0.3;
    // The Gauss-Kummer series, summed until its terms vanish: exact to
    // rounding (Ramanujan's approximation is off by 3e-6 here, which is why
    // it is not the reference).
    const double a = 10.0;
    const double b = 3.0;
    const double h = (a - b) * (a - b) / ((a + b) * (a + b));
    double sum = 0.0;
    double binomial = 1.0;
    for (int n = 0; n < 200; ++n) {
        if (n > 0) {
            binomial *= (0.5 - (n - 1)) / n;
        }
        sum += binomial * binomial * std::pow(h, n);
    }
    const double exact = kPi * (a + b) * sum;
    EXPECT_NEAR(e.length(), exact, 1e-9);
    // A quarter arc is a quarter of the whole, by symmetry.
    e.startParameter = 0.0;
    e.sweep = kHalfPi;
    EXPECT_NEAR(e.length(), exact / 4.0, 1e-9);
}

TEST(Ellipse, AnArcHasOnlyTheQuadrantsAndExtremesItSpans)
{
    Ellipse2 e;
    e.majorAxis = Vec2(4, 0);
    e.ratio = 0.5;
    e.startParameter = 0.0;
    e.sweep = kHalfPi;
    EXPECT_FALSE(e.isFull());
    EXPECT_EQ(e.quadrants().size(), 2u);
    const auto box = e.boundingBox();
    expectNear(box.min, Point2(0, 0));
    expectNear(box.max, Point2(4, 2));
    EXPECT_TRUE(e.containsParameter(0.5));
    EXPECT_FALSE(e.containsParameter(kPi));
}

TEST(Ellipse, TheClosestPointIsWhereTheNormalPassesThroughThePoint)
{
    Ellipse2 e;
    e.majorAxis = Vec2(4, 0);
    e.ratio = 0.5;
    for (const Point2 p : {Point2(6, 1), Point2(-1, 5), Point2(0.5, 0.2), Point2(-7, -3)}) {
        const double t = e.closestParameter(p);
        const Vec2 r = e.pointAtParameter(t) - p;
        EXPECT_NEAR(r.dot(e.derivativeAtParameter(t)) / e.derivativeAtParameter(t).length(), 0.0,
                    1e-9);
        // And no sampled point is nearer.
        for (int k = 0; k < 360; ++k) {
            EXPECT_GE(e.pointAtParameter(k * kTwoPi / 360.0).distanceTo(p) + 1e-9,
                      e.distanceTo(p));
        }
    }
}

TEST(Ellipse, FromAxesMakesTheLongerAxisTheMajorOne)
{
    const auto flat = Ellipse2::fromAxes(Point2(0, 0), Point2(10, 0), 4.0);
    ASSERT_TRUE(flat.has_value());
    EXPECT_NEAR(flat->ratio, 0.4, 1e-15);
    expectNear(Point2(flat->majorAxis.x, flat->majorAxis.y), Point2(10, 0));
    const auto tall = Ellipse2::fromAxes(Point2(0, 0), Point2(2, 0), 5.0);
    ASSERT_TRUE(tall.has_value());
    EXPECT_NEAR(tall->ratio, 0.4, 1e-15);
    EXPECT_NEAR(tall->majorRadius(), 5.0, 1e-12);
    expectNear(tall->pointAtParameter(kHalfPi), Point2(-2, 0));
    EXPECT_FALSE(Ellipse2::fromAxes(Point2(0, 0), Point2(0, 0), 1.0).has_value());
    EXPECT_FALSE(Ellipse2::fromAxes(Point2(0, 0), Point2(1, 0), 0.0).has_value());
}

TEST(Ellipse, TessellationStaysOnTheEllipse)
{
    Ellipse2 e;
    e.majorAxis = Vec2(0, 20);
    e.ratio = 0.25;
    const auto chain = e.tessellate(0.001);
    ASSERT_GT(chain.size(), 20u);
    expectNear(chain.front(), e.startPoint());
    expectNear(chain.back(), e.startPoint());
    for (std::size_t i = 1; i < chain.size(); ++i) {
        const Point2 mid = (chain[i - 1] + chain[i]) * 0.5;
        EXPECT_LT(e.distanceTo(mid), 0.0011);
    }
}

// ---- spline -----------------------------------------------------------------------------

TEST(Spline, AFitSplinePassesThroughItsPoints)
{
    const std::vector<Point2> fit = {Point2(0, 0), Point2(3, 4), Point2(7, 3), Point2(10, 8),
                                     Point2(14, 5)};
    const auto spline = Spline2::throughPoints(fit, 3);
    ASSERT_TRUE(spline.ok()) << spline.error().message;
    EXPECT_TRUE(spline->checkStructure());
    EXPECT_EQ(spline->degree, 3);
    EXPECT_EQ(spline->fitPoints, fit);
    EXPECT_EQ(spline->controlPoints.size(), fit.size());
    expectNear(spline->startPoint(), fit.front());
    expectNear(spline->endPoint(), fit.back());
    // Every fit point lies on the curve.
    for (const Point2& p : fit) {
        EXPECT_LT(spline->distanceTo(p), 1e-4);
    }
}

TEST(Spline, TwoPointsGiveALineAndThreeAParabola)
{
    const auto line = Spline2::throughPoints({Point2(0, 0), Point2(4, 2)}, 3);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(line->degree, 1);
    expectNear(line->pointAt(0.5), Point2(2, 1));

    const auto parabola = Spline2::throughPoints({Point2(-1, 1), Point2(0, 0), Point2(1, 1)}, 3);
    ASSERT_TRUE(parabola.ok());
    EXPECT_EQ(parabola->degree, 2);
    // Symmetric points give the symmetric parabola, whose middle is (0, 0).
    expectNear(parabola->pointAt(0.5), Point2(0, 0), 1e-12);
    EXPECT_FALSE(Spline2::throughPoints({Point2(1, 1), Point2(1, 1)}).ok());
}

TEST(Spline, AControlPointSplineStartsAndEndsOnItsPolygon)
{
    const auto spline = Spline2::fromControlPoints(
        {Point2(0, 0), Point2(0, 10), Point2(10, 10), Point2(10, 0)}, 3);
    ASSERT_TRUE(spline.ok());
    EXPECT_FALSE(spline->hasFitPoints());
    expectNear(spline->startPoint(), Point2(0, 0));
    expectNear(spline->endPoint(), Point2(10, 0));
    // A single cubic Bezier: its middle is (P0 + 3P1 + 3P2 + P3) / 8.
    expectNear(spline->pointAt(0.5), Point2(5, 7.5));
    const auto box = spline->boundingBox();
    EXPECT_TRUE(box.contains(spline->pointAt(0.3)));
}

TEST(Spline, ARationalQuadraticDrawsAnExactQuarterCircle)
{
    Spline2 arc;
    arc.degree = 2;
    arc.controlPoints = {Point2(1, 0), Point2(1, 1), Point2(0, 1)};
    arc.knots = {0, 0, 0, 1, 1, 1};
    arc.weights = {1.0, std::sqrt(0.5), 1.0};
    ASSERT_TRUE(arc.checkStructure());
    for (int k = 0; k <= 10; ++k) {
        EXPECT_NEAR(arc.pointAt(k / 10.0).length(), 1.0, 1e-12);
    }
    EXPECT_NEAR(arc.length(), kHalfPi, 1e-6);
}

TEST(Spline, ReversingWalksTheSameCurveBackwards)
{
    const auto spline =
        Spline2::throughPoints({Point2(0, 0), Point2(2, 3), Point2(5, 1), Point2(8, 4)}, 3);
    ASSERT_TRUE(spline.ok());
    const Spline2 back = spline->reversed();
    ASSERT_TRUE(back.checkStructure());
    for (int k = 0; k <= 8; ++k) {
        const double u = k / 8.0;
        expectNear(back.pointAt(back.domainStart() + (back.domainEnd() - back.domainStart()) * u),
                   spline->pointAt(spline->domainEnd() -
                                   (spline->domainEnd() - spline->domainStart()) * u),
                   1e-9);
    }
}

TEST(Spline, BadStructureIsRefusedNotEvaluated)
{
    Spline2 bad;
    bad.degree = 3;
    bad.controlPoints = {Point2(0, 0), Point2(1, 1)};
    bad.knots = {0, 0, 1, 1};
    EXPECT_FALSE(bad.checkStructure());
    EXPECT_TRUE(bad.tessellate(0.01).empty());
    bad.degree = 1;
    bad.knots = {0, 0, 1, 1};
    bad.weights = {1.0, -1.0};
    EXPECT_FALSE(bad.checkStructure());
    bad.weights.clear();
    bad.knots = {0, 1, 0.5, 1};
    EXPECT_FALSE(bad.checkStructure());
}

TEST(Spline, TessellationHoldsTheTolerance)
{
    const auto spline = Spline2::throughPoints(
        {Point2(0, 0), Point2(10, 20), Point2(20, -20), Point2(30, 20), Point2(40, 0)}, 3);
    ASSERT_TRUE(spline.ok());
    const auto coarse = spline->tessellate(0.1);
    const auto fine = spline->tessellate(0.001);
    EXPECT_GT(fine.size(), coarse.size());
    for (std::size_t i = 1; i < coarse.size(); ++i) {
        EXPECT_LT(spline->distanceTo((coarse[i - 1] + coarse[i]) * 0.5), 0.11);
    }
}
