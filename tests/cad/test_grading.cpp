#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <limits>

#include "katana/cad/grading.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace cad = katana::cad;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::Polyline2;
using katana::terrain::TinSurface;

namespace {

// A plane z = a + b x + c y over the square [-100, 100]^2: two triangles, so
// every elevation query is exact and every batter meets it at a point that
// can be worked by hand.
TinSurface plane(double a, double b, double c)
{
    const auto z = [&](double x, double y) { return a + b * x + c * y; };
    auto surface = TinSurface::create({{-100.0, -100.0, z(-100.0, -100.0)},
                                       {100.0, -100.0, z(100.0, -100.0)},
                                       {100.0, 100.0, z(100.0, 100.0)},
                                       {-100.0, 100.0, z(-100.0, 100.0)}},
                                      {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(surface.ok());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

// The 10 x 10 pad centred on the origin.
Polyline2 square()
{
    return Polyline2{{Point2(-5.0, -5.0), Point2(5.0, -5.0), Point2(5.0, 5.0), Point2(-5.0, 5.0)},
                     true};
}

cad::GradingSlopes slopes(double cut, double fill)
{
    cad::GradingSlopes s;
    s.cutBatter = cut;
    s.fillBatter = fill;
    s.interval = 2.5;
    return s;
}

// Volume of a frustum: h/3 (A1 + A2 + sqrt(A1 A2)). A pad's batters on flat
// ground make one, with the pad on top and the daylight square underneath.
double frustum(double height, double topSide, double baseSide)
{
    const double a1 = topSide * topSide;
    const double a2 = baseSide * baseSide;
    return height / 3.0 * (a1 + a2 + std::sqrt(a1 * a2));
}

} // namespace

TEST(Grading, AFilledPadOnFlatGroundDaylightsAtTheSlopeTimesTheHeightWithMitredCorners)
{
    // Pad at 2 m over ground at 0 with a 1 in 2 fill batter: the batter runs
    // 4 m out from every edge, so the daylight line is the 18 x 18 square,
    // and its corners are the pad's corners pushed out 4 m in BOTH axes -
    // (-9, -9) - where the two batter planes meet, not 4 m along the diagonal.
    const TinSurface ground = plane(0.0, 0.0, 0.0);
    const auto graded = cad::gradeToSurface(cad::FeatureLine::pad(square(), 2.0), slopes(1.0, 2.0),
                                            ground);
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    EXPECT_EQ(graded->missed, 0u);
    // 4 vertices plus 3 interior samples on each 10 m edge at 2.5 m.
    EXPECT_EQ(graded->samples, 16u);
    ASSERT_EQ(graded->daylights.size(), 16u);
    for (const cad::DaylightPoint& point : graded->daylights) {
        EXPECT_FALSE(point.cut);
        EXPECT_NEAR(point.daylight.z, 0.0, 1e-9);
        // Every daylight point is 4 m out in the axis perpendicular to its
        // edge; the corners are 4 m out in both.
        EXPECT_NEAR(std::max(std::abs(point.daylight.x), std::abs(point.daylight.y)), 9.0, 1e-6);
    }
    EXPECT_NEAR(graded->daylights[0].daylight.x, -9.0, 1e-6);
    EXPECT_NEAR(graded->daylights[0].daylight.y, -9.0, 1e-6);
    EXPECT_TRUE(graded->boundedByDaylight);
    EXPECT_TRUE(graded->daylightLine.closed);
    EXPECT_NEAR(graded->daylightLine.area(), 18.0 * 18.0, 1e-6);

    // The earthwork is a frustum 2 m high, 10 m square on top, 18 m below:
    // 2/3 (100 + 324 + 180) = 402.667 m^3, all fill.
    EXPECT_NEAR(graded->fill, frustum(2.0, 10.0, 18.0), 1e-6);
    EXPECT_NEAR(graded->cut, 0.0, 1e-9);
    EXPECT_NEAR(graded->planArea, 18.0 * 18.0, 1e-6);
    EXPECT_NEAR(graded->surface.planArea(), 18.0 * 18.0, 1e-6);
}

TEST(Grading, ACutPadUsesTheCutBatterAndTheCornerIsStillMitred)
{
    // Pad at -2 with a 1 in 1 cut batter: 2 m out, a 14 x 14 daylight square,
    // and the cut is the frustum 2/3 (100 + 196 + 140) = 290.667 m^3. A 1 in
    // 3 FILL batter must play no part.
    const TinSurface ground = plane(0.0, 0.0, 0.0);
    const auto graded = cad::gradeToSurface(cad::FeatureLine::pad(square(), -2.0), slopes(1.0, 3.0),
                                            ground);
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    for (const cad::DaylightPoint& point : graded->daylights) {
        EXPECT_TRUE(point.cut);
        EXPECT_NEAR(std::max(std::abs(point.daylight.x), std::abs(point.daylight.y)), 7.0, 1e-6);
    }
    EXPECT_NEAR(graded->cut, frustum(2.0, 10.0, 14.0), 1e-6);
    EXPECT_NEAR(graded->fill, 0.0, 1e-9);
}

TEST(Grading, OnSlopingGroundEachSideDaylightsWhereItsOwnBatterMeetsTheGround)
{
    // Ground z = 0.1 x. The pad at 0.5 is AT GRADE along its east edge (x = 5,
    // ground 0.5), in fill along the west (ground -0.5). West: batter
    // 0.5 - d/2 meets ground 0.1 (-5 - d) = -0.5 - 0.1 d when 1 = 0.4 d,
    // d = 2.5: daylight at x = -7.5, z = -0.75. North and south edges are
    // in fill by 0.5 - 0.1 x, thinning to nothing at the east end.
    const TinSurface ground = plane(0.0, 0.1, 0.0);
    const auto graded = cad::gradeToSurface(cad::FeatureLine::pad(square(), 0.5), slopes(1.0, 2.0),
                                            ground);
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    EXPECT_EQ(graded->missed, 0u);
    std::size_t west = 0;
    std::size_t east = 0;
    for (const cad::DaylightPoint& point : graded->daylights) {
        const bool onWestEdge = std::abs(point.origin.x + 5.0) < 1e-9 && std::abs(point.origin.y) < 5.0;
        const bool onEastEdge = std::abs(point.origin.x - 5.0) < 1e-9 && std::abs(point.origin.y) < 5.0;
        if (onWestEdge) {
            ++west;
            EXPECT_NEAR(point.daylight.x, -7.5, 1e-6);
            EXPECT_NEAR(point.daylight.z, -0.75, 1e-6);
            EXPECT_FALSE(point.cut);
        }
        if (onEastEdge) {
            ++east;
            EXPECT_NEAR(point.daylight.x, 5.0, 1e-6) << "at grade: the daylight is the edge";
        }
        // Every daylight point lies on the ground, and on the batter from its origin.
        EXPECT_NEAR(point.daylight.z, 0.1 * point.daylight.x, 1e-6);
    }
    EXPECT_EQ(west, 3u);
    EXPECT_EQ(east, 3u);
    EXPECT_GT(graded->fill, 0.0);
    EXPECT_NEAR(graded->cut, 0.0, 1e-6);
}

TEST(Grading, AnOpenFeatureLineIsGradedOnTheChosenSideOnly)
{
    // A line along the x axis at 1 m, graded to its left (+y) at 1 in 2 on
    // flat ground: the daylight is the parallel line y = 2, and the strip
    // between them is a prism 10 long, 2 wide, 1 high, cut diagonally: 10 m^3.
    cad::FeatureLine line;
    line.vertices = {Point3(0.0, 0.0, 1.0), Point3(10.0, 0.0, 1.0)};
    line.closed = false;
    cad::GradingSlopes left = slopes(1.0, 2.0);
    left.side = cad::GradingSide::Left;
    const TinSurface ground = plane(0.0, 0.0, 0.0);
    const auto graded = cad::gradeToSurface(line, left, ground);
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    EXPECT_EQ(graded->samples, 5u);
    for (const cad::DaylightPoint& point : graded->daylights) {
        EXPECT_NEAR(point.daylight.y, 2.0, 1e-6);
        EXPECT_NEAR(point.daylight.x, point.origin.x, 1e-9);
    }
    EXPECT_FALSE(graded->daylightLine.closed);
    EXPECT_TRUE(graded->boundedByDaylight);
    EXPECT_NEAR(graded->fill, 10.0, 1e-6);
    EXPECT_NEAR(graded->planArea, 20.0, 1e-6);

    left.side = cad::GradingSide::Right;
    const auto right = cad::gradeToSurface(line, left, ground);
    ASSERT_TRUE(right.ok());
    for (const cad::DaylightPoint& point : right->daylights) {
        EXPECT_NEAR(point.daylight.y, -2.0, 1e-6);
    }
}

TEST(Grading, AClockwisePadIsGradedOutwardAllTheSame)
{
    Polyline2 clockwise = square();
    std::reverse(clockwise.vertices.begin(), clockwise.vertices.end());
    const TinSurface ground = plane(0.0, 0.0, 0.0);
    const auto graded = cad::gradeToSurface(cad::FeatureLine::pad(clockwise, 2.0), slopes(1.0, 2.0),
                                            ground);
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    EXPECT_NEAR(graded->daylightLine.area(), 18.0 * 18.0, 1e-6);
    EXPECT_NEAR(graded->fill, frustum(2.0, 10.0, 18.0), 1e-6);
}

TEST(Grading, AFeatureLineWithVaryingHeightsGradesEachSampleFromItsOwnElevation)
{
    // An east-west line rising from 1 to 3 m over 10 m, graded left at 1 in
    // 1 on flat ground: the daylight width grows with the height, 1 m at the
    // west end, 3 m at the east, and 2 m at the middle sample.
    cad::FeatureLine line;
    line.vertices = {Point3(0.0, 0.0, 1.0), Point3(10.0, 0.0, 3.0)};
    cad::GradingSlopes s = slopes(1.0, 1.0);
    s.interval = 5.0;
    const auto graded = cad::gradeToSurface(line, s, plane(0.0, 0.0, 0.0));
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    ASSERT_EQ(graded->daylights.size(), 3u);
    EXPECT_NEAR(graded->daylights[0].daylight.y, 1.0, 1e-6);
    EXPECT_NEAR(graded->daylights[1].daylight.y, 2.0, 1e-6);
    EXPECT_NEAR(graded->daylights[2].daylight.y, 3.0, 1e-6);
}

TEST(Grading, SamplesThatFindNoGroundAreCountedAndTheRestStillGrade)
{
    // The pad straddles the east edge of the ground (x = 100): its east side
    // and its corners' mitres run off the surface and are missed; the rest
    // reaches the ground and the surface is bounded by the hull instead.
    Polyline2 pad{{Point2(90.0, -5.0), Point2(105.0, -5.0), Point2(105.0, 5.0), Point2(90.0, 5.0)}, true};
    const auto graded = cad::gradeToSurface(cad::FeatureLine::pad(pad, 2.0), slopes(1.0, 2.0),
                                            plane(0.0, 0.0, 0.0));
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    EXPECT_GT(graded->missed, 0u);
    EXPECT_LT(graded->missed, graded->samples);
    EXPECT_FALSE(graded->boundedByDaylight);
    EXPECT_FALSE(graded->surface.empty());
}

TEST(Grading, ABatterThatNeverMeetsTheGroundWithinTheWidthIsAMiss)
{
    // Ground z = x: falling at 1 in 1 to the west, rising at 1 in 1 to the
    // east. The pad at 0 is in fill on its west edge, where a 1 in 2 batter
    // falls slower than the ground and never catches it; in cut on its east
    // edge, where a 1 in 1 batter rises exactly as fast as the ground and
    // stays 5 m below it for ever. Both edges' 3 interior samples miss, and
    // all 4 corners, whose mitres head west or east: 10 misses. The north
    // and south edges run across the slope and daylight normally.
    const TinSurface ground = plane(0.0, 1.0, 0.0);
    cad::GradingSlopes s = slopes(1.0, 2.0);
    s.maximumWidth = 20.0;
    const auto graded = cad::gradeToSurface(cad::FeatureLine::pad(square(), 0.0), s, ground);
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    for (const cad::DaylightPoint& point : graded->daylights) {
        EXPECT_GT(point.origin.x, -5.0 + 1e-9) << "nothing on the west edge should have daylighted";
        EXPECT_LT(point.origin.x, 5.0 - 1e-9) << "nor on the east";
    }
    EXPECT_EQ(graded->missed, 10u);
    EXPECT_EQ(graded->daylights.size(), 6u);
    EXPECT_FALSE(graded->boundedByDaylight);
}

TEST(Grading, RefusesWhatCannotBeGraded)
{
    const TinSurface ground = plane(0.0, 0.0, 0.0);
    const cad::FeatureLine pad = cad::FeatureLine::pad(square(), 2.0);
    using katana::core::ErrorCode;

    cad::GradingSlopes bad = slopes(0.0, 2.0);
    EXPECT_EQ(cad::gradeToSurface(pad, bad, ground).error().code, ErrorCode::InvalidArgument);
    bad = slopes(1.0, -2.0);
    EXPECT_EQ(cad::gradeToSurface(pad, bad, ground).error().code, ErrorCode::InvalidArgument);
    bad = slopes(1.0, 2.0);
    bad.interval = 0.0;
    EXPECT_EQ(cad::gradeToSurface(pad, bad, ground).error().code, ErrorCode::InvalidArgument);
    bad = slopes(1.0, 2.0);
    bad.maximumWidth = std::nan("");
    EXPECT_EQ(cad::gradeToSurface(pad, bad, ground).error().code, ErrorCode::InvalidArgument);

    cad::FeatureLine two;
    two.vertices = {Point3(0.0, 0.0, 1.0), Point3(1.0, 0.0, 1.0)};
    two.closed = true;
    EXPECT_EQ(cad::gradeToSurface(two, slopes(1.0, 2.0), ground).error().code,
              ErrorCode::InvalidArgument);
    cad::FeatureLine one;
    one.vertices = {Point3(0.0, 0.0, 1.0)};
    EXPECT_EQ(cad::gradeToSurface(one, slopes(1.0, 2.0), ground).error().code,
              ErrorCode::InvalidArgument);
    cad::FeatureLine infinite = pad;
    infinite.vertices[0].z = std::numeric_limits<double>::infinity();
    EXPECT_EQ(cad::gradeToSurface(infinite, slopes(1.0, 2.0), ground).error().code,
              ErrorCode::InvalidArgument);

    // Entirely off the surface: nothing reaches the ground.
    Polyline2 away{{Point2(500.0, 500.0), Point2(510.0, 500.0), Point2(510.0, 510.0)}, true};
    const auto off = cad::gradeToSurface(cad::FeatureLine::pad(away, 2.0), slopes(1.0, 2.0), ground);
    ASSERT_FALSE(off.ok());
    EXPECT_EQ(off.error().code, ErrorCode::InvalidGeometry);
}
