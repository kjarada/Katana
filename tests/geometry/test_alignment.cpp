// The horizontal alignment (include/katana/geometry/alignment.hpp).
//
// Reference values come from outside the code under test. The tangent-length
// derivation was checked by laying the curve chain out in Python with
// adaptive-Simpson spirals and measuring how far its end landed off the
// forward tangent line: 3e-15 m, 2e-14 m and 2e-14 m for the three cases
// below. The equal-spiral case also matches the published design formula
// T = k + (R + p) tan(D / 2) = 125.652229541472 to twelve decimals, and the
// simple curve reduces to T = R tan(D / 2) = 50 exactly.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/geometry/alignment.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::geometry;
using katana::core::ErrorCode;
using katana::math::kPi;

namespace {

HorizontalAlignment threePIs(Point2 a, Point2 b, Point2 c, double radius, double spiralIn = 0.0,
                             double spiralOut = 0.0)
{
    HorizontalAlignment definition;
    definition.pis = {AlignmentPI{a}, AlignmentPI{b, radius, spiralIn, spiralOut},
                      AlignmentPI{c}};
    return definition;
}

// A 90 degree left turn at (100, 0) rounded with R = 50: T = 50 exactly.
HorizontalAlignment simpleCurve()
{
    return threePIs(Point2(0, 0), Point2(100, 0), Point2(100, 100), 50.0);
}

// A 30 degree left turn at (400, 0), R = 300, 90 m spirals both sides.
HorizontalAlignment equalSpirals()
{
    const double d = kPi / 6.0;
    return threePIs(Point2(0, 0), Point2(400, 0),
                    Point2(400 + 300 * std::cos(d), 300 * std::sin(d)), 300.0, 90.0, 90.0);
}

// A 40 degree RIGHT turn at (300, 0), R = 200, spirals 60 m in and 40 m out.
HorizontalAlignment unequalRightTurn()
{
    const double d = -40.0 * kPi / 180.0;
    return threePIs(Point2(0, 0), Point2(300, 0),
                    Point2(300 + 250 * std::cos(d), 250 * std::sin(d)), 200.0, 60.0, 40.0);
}

std::vector<AlignmentElementKind> kinds(const SolvedAlignment& solved)
{
    std::vector<AlignmentElementKind> out;
    for (const AlignmentElement& element : solved.elements()) {
        out.push_back(kindOf(element));
    }
    return out;
}

} // namespace

// ---- against closed forms and published formulas ---------------------------------

TEST(Alignment, ASimpleCurveReducesToTheTextbookTangentLength)
{
    const auto solved = solveAlignment(simpleCurve());
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    const std::vector<AlignmentElementKind> expected{AlignmentElementKind::Tangent,
                                                     AlignmentElementKind::Arc,
                                                     AlignmentElementKind::Tangent};
    EXPECT_EQ(kinds(*solved), expected);

    // T = R tan(D / 2) = 50 tan(45 deg) = 50, so TS = (50, 0) and ST = (100, 50),
    // and the arc is a quarter circle of length 50 pi / 2.
    const auto& elements = solved->elements();
    ASSERT_EQ(elements.size(), 3u);
    EXPECT_NEAR(elements[0].length, 50.0, 1e-12);
    EXPECT_NEAR(elements[1].length, 50.0 * kPi / 2.0, 1e-12);
    EXPECT_NEAR(elements[2].length, 50.0, 1e-12);
    EXPECT_NEAR(solved->length(), 100.0 + 50.0 * kPi / 2.0, 1e-12);

    const auto ts = solved->pointAtStation(50.0);
    ASSERT_TRUE(ts.has_value());
    EXPECT_NEAR(ts->x, 50.0, 1e-12);
    EXPECT_NEAR(ts->y, 0.0, 1e-12);

    // Mid-arc: the centre is (50, 50), the radius points at -45 degrees.
    const auto mid = solved->pointAtStation(50.0 + 25.0 * kPi / 2.0);
    ASSERT_TRUE(mid.has_value());
    EXPECT_NEAR(mid->x, 50.0 + 50.0 * std::cos(-kPi / 4.0), 1e-9);
    EXPECT_NEAR(mid->y, 50.0 + 50.0 * std::sin(-kPi / 4.0), 1e-9);

    EXPECT_NEAR(*solved->directionAtStation(50.0), 0.0, 1e-12);         // entering east
    EXPECT_NEAR(*solved->directionAtStation(solved->endStation()), kPi / 2.0, 1e-9); // north
    EXPECT_NEAR(*solved->curvatureAtStation(60.0), 1.0 / 50.0, 1e-15);  // left
    EXPECT_EQ(*solved->curvatureAtStation(10.0), 0.0);
}

TEST(Alignment, EqualSpiralsMatchThePublishedDesignFormula)
{
    // T = k + (R + p) tan(D / 2) with p and k from the 90 m spiral on R = 300:
    // 125.652229541472, so TS = (400 - T, 0) = (274.347770458528, 0). The
    // central arc turns D - 2 theta_s = 30 deg - 2 * 0.15 rad = 0.223598775598
    // rad, length 67.079632679490. ST = (508.818022825068, 62.826114770736).
    const auto solved = solveAlignment(equalSpirals());
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    const std::vector<AlignmentElementKind> expected{
        AlignmentElementKind::Tangent, AlignmentElementKind::Spiral, AlignmentElementKind::Arc,
        AlignmentElementKind::Spiral, AlignmentElementKind::Tangent};
    EXPECT_EQ(kinds(*solved), expected);

    const auto& elements = solved->elements();
    ASSERT_EQ(elements.size(), 5u);
    EXPECT_NEAR(elements[0].length, 274.347770458528, 1e-9);
    EXPECT_NEAR(elements[1].length, 90.0, 1e-12);
    EXPECT_NEAR(elements[2].length, 67.079632679490, 1e-9);
    EXPECT_NEAR(elements[3].length, 90.0, 1e-12);

    const double st = elements[4].startStation;
    const auto atST = solved->pointAtStation(st);
    ASSERT_TRUE(atST.has_value());
    EXPECT_NEAR(atST->x, 508.818022825068, 1e-9);
    EXPECT_NEAR(atST->y, 62.826114770736, 1e-9);
    // Leaving the curve exactly along the forward tangent, 30 degrees.
    EXPECT_NEAR(*solved->directionAtStation(st), kPi / 6.0, 1e-9);
    EXPECT_NEAR(*solved->directionAtStation(solved->endStation()), kPi / 6.0, 1e-12);
}

TEST(Alignment, UnequalSpiralsOnARightTurnCloseOnTheForwardTangent)
{
    // No published formula covers unequal spirals; the reference is the
    // Python chain closure. T1 = 102.396853060753, T2 = 93.556128402017,
    // arc 0.448131700798 rad = 89.626340159546 m, ST = (371.668152282091,
    // -60.136720147060). The chain in Python ended 2e-14 m off the forward
    // tangent, so a solver that lands ST further than 1e-9 away is wrong.
    const auto solved = solveAlignment(unequalRightTurn());
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    const auto& elements = solved->elements();
    ASSERT_EQ(elements.size(), 5u);
    EXPECT_NEAR(elements[0].length, 197.603146939247, 1e-9);
    EXPECT_NEAR(elements[1].length, 60.0, 1e-12);
    EXPECT_NEAR(elements[2].length, 89.626340159546, 1e-9);
    EXPECT_NEAR(elements[3].length, 40.0, 1e-12);
    EXPECT_NEAR(elements[4].length, 250.0 - 93.556128402017, 1e-9);

    const double st = elements[4].startStation;
    const auto atST = solved->pointAtStation(st);
    ASSERT_TRUE(atST.has_value());
    EXPECT_NEAR(atST->x, 371.668152282091, 1e-9);
    EXPECT_NEAR(atST->y, -60.136720147060, 1e-9);
    EXPECT_NEAR(*solved->directionAtStation(st), -40.0 * kPi / 180.0, 1e-9);
    // A right turn: curvature negative on the arc, and the spiral-in runs
    // from 0 down to it.
    EXPECT_NEAR(*solved->curvatureAtStation(elements[2].startStation + 1.0), -1.0 / 200.0, 1e-15);
    EXPECT_NEAR(*solved->curvatureAtStation(elements[1].startStation + 30.0), -0.5 / 200.0, 1e-15);
}

// ---- the property that makes it an alignment -------------------------------------

TEST(Alignment, EveryJointIsContinuousInPositionAndDirection)
{
    // The whole reason for defining an alignment by its PIs. Across every
    // element boundary of every curved case, the point and the tangent
    // direction must agree from both sides. A tangent-length error, a wrong
    // arc centre or a spiral started with the wrong curvature would each
    // show up here as a step or a kink.
    for (const HorizontalAlignment& definition : {simpleCurve(), equalSpirals(), unequalRightTurn()}) {
        const auto solved = solveAlignment(definition);
        ASSERT_TRUE(solved.ok()) << solved.error().describe();
        const std::vector<double> joints = solved->keyStations();
        ASSERT_GE(joints.size(), 3u);
        std::size_t checked = 0;
        for (std::size_t i = 1; i + 1 < joints.size(); ++i) {
            const double s = joints[i];
            const double h = 1e-6;
            const auto before = solved->pointAtStation(s - h);
            const auto after = solved->pointAtStation(s + h);
            ASSERT_TRUE(before && after);
            // 2h apart along the curve, so within 2h + rounding of each other.
            EXPECT_NEAR(before->distanceTo(*after), 2.0 * h, 1e-9) << "joint at " << s;
            const double dirBefore = *solved->directionAtStation(s - h);
            const double dirAfter = *solved->directionAtStation(s + h);
            EXPECT_NEAR(katana::math::normalizeAngleSigned(dirAfter - dirBefore), 0.0, 1e-7)
                << "kink at " << s;
            ++checked;
        }
        EXPECT_GE(checked, 2u);
    }
}

TEST(Alignment, OffsetsAreMeasuredToTheLeftLookingAlongIncreasingStation)
{
    // On the simple curve, which turns left, the centre (50, 50) is on the
    // left, so a +5 offset at mid-arc is 5 m closer to it: 45 m away.
    const auto solved = solveAlignment(simpleCurve());
    ASSERT_TRUE(solved.ok());
    const double mid = 50.0 + 25.0 * kPi / 2.0;
    const auto left = solved->pointAtStationOffset(mid, 5.0);
    const auto right = solved->pointAtStationOffset(mid, -5.0);
    ASSERT_TRUE(left && right);
    EXPECT_NEAR(left->distanceTo(Point2(50, 50)), 45.0, 1e-9);
    EXPECT_NEAR(right->distanceTo(Point2(50, 50)), 55.0, 1e-9);
    // And on the first tangent, heading east, left is +y.
    const auto onTangent = solved->pointAtStationOffset(10.0, 3.0);
    ASSERT_TRUE(onTangent.has_value());
    EXPECT_NEAR(onTangent->x, 10.0, 1e-12);
    EXPECT_NEAR(onTangent->y, 3.0, 1e-12);
}

TEST(Alignment, TheChordedPolylineStaysWithinToleranceAndHasNoDoubledJoints)
{
    const auto solved = solveAlignment(simpleCurve());
    ASSERT_TRUE(solved.ok());
    const double tolerance = 0.01;
    const Polyline2 polyline = solved->toPolyline(tolerance);
    ASSERT_GE(polyline.vertices.size(), 4u);
    EXPECT_FALSE(polyline.closed);
    EXPECT_EQ(polyline.vertices.front(), Point2(0, 0));
    EXPECT_NEAR(polyline.vertices.back().distanceTo(Point2(100, 100)), 0.0, 1e-9);
    for (std::size_t i = 1; i < polyline.vertices.size(); ++i) {
        EXPECT_GT(polyline.vertices[i - 1].distanceTo(polyline.vertices[i]), 1e-9)
            << "a joint was emitted twice at vertex " << i;
    }
    // Every vertex on the arc lies on the circle; every chord midpoint is
    // within the sagitta tolerance of it.
    std::size_t onArc = 0;
    for (std::size_t i = 1; i < polyline.vertices.size(); ++i) {
        const Point2& a = polyline.vertices[i - 1];
        const Point2& b = polyline.vertices[i];
        const bool arcChord = std::abs(a.distanceTo(Point2(50, 50)) - 50.0) < 1e-9 &&
                              std::abs(b.distanceTo(Point2(50, 50)) - 50.0) < 1e-9;
        if (arcChord) {
            const Point2 mid = (a + b) * 0.5;
            EXPECT_LE(std::abs(mid.distanceTo(Point2(50, 50)) - 50.0), tolerance * 1.001);
            ++onArc;
        }
    }
    EXPECT_GT(onArc, 5u) << "the arc was not actually chorded";
}

TEST(Alignment, ARadiusOfZeroLeavesAKinkRatherThanFailing)
{
    // An alignment traced from a surveyed polyline has no curves at all. It
    // must solve, station correctly, and simply turn the corner.
    const auto solved = solveAlignment(threePIs(Point2(0, 0), Point2(100, 0), Point2(100, 100), 0.0));
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    EXPECT_EQ(solved->elements().size(), 2u);
    EXPECT_NEAR(solved->length(), 200.0, 1e-12);
    const std::vector<double> expected{0.0, 100.0, 200.0};
    EXPECT_EQ(solved->keyStations(), expected);
    EXPECT_NEAR(*solved->directionAtStation(99.0), 0.0, 1e-12);
    EXPECT_NEAR(*solved->directionAtStation(101.0), kPi / 2.0, 1e-12);
}

TEST(Alignment, StationsStartWhereTheJobSaysAndStopAtTheEnds)
{
    HorizontalAlignment definition = simpleCurve();
    definition.startStation = 1000.0;
    const auto solved = solveAlignment(definition);
    ASSERT_TRUE(solved.ok());
    EXPECT_EQ(solved->startStation(), 1000.0);
    EXPECT_NEAR(solved->endStation(), 1100.0 + 50.0 * kPi / 2.0, 1e-12);
    const auto ts = solved->pointAtStation(1050.0);
    ASSERT_TRUE(ts.has_value());
    EXPECT_NEAR(ts->x, 50.0, 1e-12);
    EXPECT_FALSE(solved->pointAtStation(999.0).has_value());
    EXPECT_FALSE(solved->pointAtStation(solved->endStation() + 1.0).has_value());
    EXPECT_EQ(solved->elementAt(500.0), nullptr);
    // The very end is inside, not outside.
    EXPECT_TRUE(solved->pointAtStation(solved->endStation()).has_value());
}

// ---- refusals, each naming the PI ------------------------------------------------

TEST(Alignment, RefusesDefinitionsThatCannotBeBuiltAndNamesThePI)
{
    HorizontalAlignment one;
    one.pis = {AlignmentPI{Point2(0, 0)}};
    EXPECT_EQ(solveAlignment(one).error().code, ErrorCode::InvalidGeometry);

    // Coincident consecutive PIs.
    const auto coincident =
        solveAlignment(threePIs(Point2(0, 0), Point2(0, 0), Point2(100, 100), 50.0));
    EXPECT_EQ(coincident.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(coincident.error().describe().find("PI 0"), std::string::npos);

    // Doubling back through 180 degrees with a radius: nothing can round it.
    const auto reversal =
        solveAlignment(threePIs(Point2(0, 0), Point2(100, 0), Point2(0, 0), 50.0));
    EXPECT_EQ(reversal.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(reversal.error().describe().find("PI 1"), std::string::npos);

    // 200 m spirals on R = 300 each use 0.333 rad; the corner only has 0.524.
    const double d = kPi / 6.0;
    const auto tooMuchSpiral = solveAlignment(
        threePIs(Point2(0, 0), Point2(400, 0), Point2(400 + 300 * std::cos(d), 300 * std::sin(d)),
                 300.0, 200.0, 200.0));
    EXPECT_EQ(tooMuchSpiral.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(tooMuchSpiral.error().describe().find("PI 1"), std::string::npos);

    // Two R = 80 curves at PIs 100 m apart need 80 m of tangent each: overlap.
    HorizontalAlignment overlap;
    overlap.pis = {AlignmentPI{Point2(0, 0)}, AlignmentPI{Point2(100, 0), 80.0},
                   AlignmentPI{Point2(100, 100), 80.0}, AlignmentPI{Point2(200, 100)}};
    const auto overlapping = solveAlignment(overlap);
    EXPECT_EQ(overlapping.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(overlapping.error().describe().find("PI 1"), std::string::npos);
    EXPECT_NE(overlapping.error().describe().find("PI 2"), std::string::npos);

    // Bad numbers are InvalidArgument, not InvalidGeometry.
    EXPECT_EQ(solveAlignment(threePIs(Point2(0, 0), Point2(100, 0), Point2(100, 100), 50.0, -1.0))
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(solveAlignment(threePIs(Point2(0, 0), Point2(100, 0), Point2(100, 100),
                                      std::numeric_limits<double>::quiet_NaN()))
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
}

TEST(Alignment, BackToBackCurvesAreAllowedWhenTheTangentBetweenThemIsExactlyUsedUp)
{
    // Two R = 50 curves at PIs exactly 100 m apart: T = 50 each, so the
    // tangent between them is zero. That is a legal reverse curve and must
    // solve with no zero-length element stored between the arcs.
    HorizontalAlignment definition;
    definition.pis = {AlignmentPI{Point2(0, 0)}, AlignmentPI{Point2(100, 0), 50.0},
                      AlignmentPI{Point2(100, 100), 50.0}, AlignmentPI{Point2(200, 100)}};
    const auto solved = solveAlignment(definition);
    ASSERT_TRUE(solved.ok()) << solved.error().describe();
    const std::vector<AlignmentElementKind> expected{
        AlignmentElementKind::Tangent, AlignmentElementKind::Arc, AlignmentElementKind::Arc,
        AlignmentElementKind::Tangent};
    EXPECT_EQ(kinds(*solved), expected);
    for (const AlignmentElement& element : solved->elements()) {
        EXPECT_GT(element.length, 1.0);
    }
    // The second curve turns right, so curvature flips sign at the joint.
    const double joint = solved->elements()[2].startStation;
    EXPECT_GT(*solved->curvatureAtStation(joint - 1.0), 0.0);
    EXPECT_LT(*solved->curvatureAtStation(joint + 1.0), 0.0);
}
