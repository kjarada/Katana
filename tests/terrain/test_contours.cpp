#include <gtest/gtest.h>

#include <cmath>
#include <cstddef>
#include <limits>
#include <utility>
#include <vector>

#include "katana/geometry/polygon.hpp"
#include "katana/terrain/contours.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;
using katana::geometry::Polyline2;
using katana::test::Random;

namespace {

// Square pyramid used by several tests: base `base` x `base` at z = 0 centred on
// (base/2, base/2), apex `height` above the centre. The level L therefore cuts it
// in the axis-aligned square of half width s(L) = (base/2) (1 - L/height), whose
// perimeter is 8 s and whose enclosed area is 4 s^2. Both are exact because the
// four ridges are breaklines, so every triangle lies in one planar face.
constexpr double kPyramidBase = 100.0;
constexpr double kPyramidHeight = 50.0;

double halfWidthAtLevel(double level)
{
    return 0.5 * kPyramidBase * (1.0 - level / kPyramidHeight);
}

} // namespace

// ---- a plane: the level set is a straight line ------------------------------------------

TEST(ContourAt, PlaneGivesOneStraightLineWithHigherGroundOnTheLeft)
{
    // z = x over [0, 100]^2, so the level L is the segment x = L, y in [0, 100]:
    // one open contour of length 100. The uphill direction is +x, and "higher
    // ground on the left" (left of a heading d is d rotated a quarter turn
    // counter-clockwise) forces the heading -y: the line starts at y = 100.
    Random random;
    const TinSurface surface =
        buildFromPoints(planePoints(Plane{1.0, 0.0, 0.0}, 100.0, 400, random));
    ASSERT_FALSE(surface.empty());

    const auto result = contourAt(surface, 37.5);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result.value().size(), 1u);
    const Contour& contour = result.value()[0];
    EXPECT_EQ(contour.elevation, 37.5);
    EXPECT_FALSE(contour.major); // contourAt() never flags an index contour
    EXPECT_FALSE(contour.line.closed);

    const std::vector<Point2>& v = contour.line.vertices;
    ASSERT_GE(v.size(), 2u);
    for (const Point2& p : v) {
        // The crossing is x_low + (L - z_low)/(z_high - z_low) * (x_high - x_low)
        // with z == x: four roundings on magnitudes <= 100, well below 1e-12.
        EXPECT_NEAR(p.x, 37.5, 1e-12) << p;
    }
    EXPECT_NEAR(v.front().y, 100.0, 1e-12);
    EXPECT_NEAR(v.back().y, 0.0, 1e-12);
    EXPECT_NEAR(contour.line.length(), 100.0, 1e-9);
}

// ---- hill and pit: closed rings with the documented orientation --------------------------

TEST(ContourAt, PyramidLevelIsASquareRingCounterClockwise)
{
    Random random;
    const TinSurface surface =
        build(pyramidInput(kPyramidBase, kPyramidHeight, 300, random)).surface;
    ASSERT_FALSE(surface.empty());

    for (const double level : {10.0, 20.0, 30.0, 40.0}) {
        const double s = halfWidthAtLevel(level);
        const auto result = contourAt(surface, level);
        ASSERT_TRUE(result.ok()) << result.error().describe();
        ASSERT_EQ(result.value().size(), 1u) << "level " << level;
        const Polyline2& line = result.value()[0].line;
        EXPECT_TRUE(line.closed);
        EXPECT_NEAR(line.length(), 8.0 * s, 1e-9) << "level " << level;
        // Positive = counter-clockwise: the hill is inside, so higher ground is
        // on the left all the way round.
        EXPECT_NEAR(line.signedArea(), 4.0 * s * s, 1e-8) << "level " << level;
    }
}

TEST(ContourAt, DepressionRingRunsClockwise)
{
    // The same pyramid turned upside down: a square pit 50 m deep, z = -50 + d
    // with d the Chebyshev distance from the centre. The level L in (-50, 0) is
    // the square of half width d = L + 50, and { z >= L } is the ground OUTSIDE
    // it, so the ring is traced with the higher ground on its left = clockwise.
    Random random;
    const TinSurface surface =
        build(pyramidInput(kPyramidBase, -kPyramidHeight, 300, random)).surface;
    ASSERT_FALSE(surface.empty());
    EXPECT_EQ(surface.minElevation(), -kPyramidHeight);
    EXPECT_EQ(surface.maxElevation(), 0.0);

    for (const double level : {-40.0, -30.0, -20.0, -10.0}) {
        const double s = level + kPyramidHeight;
        const auto result = contourAt(surface, level);
        ASSERT_TRUE(result.ok()) << result.error().describe();
        ASSERT_EQ(result.value().size(), 1u) << "level " << level;
        const Polyline2& line = result.value()[0].line;
        EXPECT_TRUE(line.closed);
        EXPECT_NEAR(line.length(), 8.0 * s, 1e-9) << "level " << level;
        EXPECT_NEAR(line.signedArea(), -4.0 * s * s, 1e-8) << "level " << level;
    }
}

// ---- the degenerate case the "z >= level is above" rule exists for ------------------------

TEST(ContourAt, LevelThroughVerticesGivesOneUnbrokenLine)
{
    // Two triangles of the plane z = (x + y)/2 over [0, 2]^2, cut along the
    // diagonal (0,0)-(2,2). The level 1 passes exactly through the vertices
    // (2,0,1) and (0,2,1). The contour must be the single segment x + y = 2
    // through both of them: no duplicated vertex, no zero-length piece, no gap.
    auto created = TinSurface::create(
        {Point3(0, 0, 0), Point3(2, 0, 1), Point3(2, 2, 2), Point3(0, 2, 1)},
        {TinTriangle{0, 1, 2}, TinTriangle{0, 2, 3}});
    ASSERT_TRUE(created.ok()) << created.error().describe();

    const auto result = contourAt(created.value(), 1.0);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result.value().size(), 1u);
    const Polyline2& line = result.value()[0].line;
    EXPECT_FALSE(line.closed);
    // Exact: a vertex hit is returned unchanged, and the diagonal is met at
    // fraction 1/2, which is exact in binary. Heading (2,0) -> (0,2) is ruled out
    // because the uphill direction (1,1) has to lie on the left.
    ASSERT_EQ(line.vertices.size(), 3u);
    EXPECT_EQ(line.vertices[0], Point2(0, 2));
    EXPECT_EQ(line.vertices[1], Point2(1, 1));
    EXPECT_EQ(line.vertices[2], Point2(2, 0));
    EXPECT_DOUBLE_EQ(line.length(), 2.0 * std::sqrt(2.0));
}

// ---- contours(): the ladder of levels -----------------------------------------------------

TEST(Contours, PyramidLadderAscendsAndFlagsIndexContours)
{
    // interval 10 from base 0: the levels in (0, 50] are k = 1..5. Level 50 is the
    // apex and nothing else, which is a single point and therefore dropped, so
    // four rings remain. majorEvery = 2 flags k = 2 and k = 4.
    Random random;
    const TinSurface surface =
        build(pyramidInput(kPyramidBase, kPyramidHeight, 300, random)).surface;
    ASSERT_FALSE(surface.empty());

    const auto result = contours(surface, 10.0, 0.0, 2);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const std::vector<Contour>& lines = result.value();
    ASSERT_EQ(lines.size(), 4u);
    for (std::size_t i = 0; i < lines.size(); ++i) {
        const auto k = static_cast<double>(i + 1);
        const double level = 10.0 * k;
        const double s = halfWidthAtLevel(level);
        EXPECT_EQ(lines[i].elevation, level);
        EXPECT_EQ(lines[i].major, (i + 1) % 2 == 0);
        EXPECT_TRUE(lines[i].line.closed);
        EXPECT_NEAR(lines[i].line.length(), 8.0 * s, 1e-9) << "level " << level;
        EXPECT_NEAR(lines[i].line.signedArea(), 4.0 * s * s, 1e-8) << "level " << level;
    }

    // majorEvery = 0 means no index contours at all.
    const auto plain = contours(surface, 10.0, 0.0, 0);
    ASSERT_TRUE(plain.ok()) << plain.error().describe();
    ASSERT_EQ(plain.value().size(), 4u);
    for (const Contour& contour : plain.value()) {
        EXPECT_FALSE(contour.major);
    }
}

TEST(Contours, ShiftedBaseMovesTheLadder)
{
    // base 5, interval 10: the levels in (0, 50] are 5, 15, 25, 35, 45.
    Random random;
    const TinSurface surface =
        build(pyramidInput(kPyramidBase, kPyramidHeight, 200, random)).surface;
    const auto result = contours(surface, 10.0, 5.0, 5);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result.value().size(), 5u);
    for (std::size_t i = 0; i < 5; ++i) {
        const double level = 5.0 + 10.0 * static_cast<double>(i);
        EXPECT_EQ(result.value()[i].elevation, level);
        EXPECT_NEAR(result.value()[i].line.length(), 8.0 * halfWidthAtLevel(level), 1e-9);
    }
}

// ---- errors and empty ranges ---------------------------------------------------------------

TEST(Contours, RejectsBadArgumentsAndEmptyRanges)
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    Random random;
    const TinSurface surface =
        build(pyramidInput(kPyramidBase, kPyramidHeight, 40, random)).surface;
    ASSERT_FALSE(surface.empty());

    EXPECT_EQ(contourAt(surface, nan).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(contourAt(surface, inf).error().code, ErrorCode::InvalidArgument);

    // The contour is the boundary of { z >= L }. At or below the lowest ground
    // that region is the whole surface and above the summit it is empty; either
    // way it has no boundary inside the surface.
    EXPECT_TRUE(contourAt(surface, 0.0).value().empty());
    EXPECT_TRUE(contourAt(surface, -1.0).value().empty());
    EXPECT_TRUE(contourAt(surface, 50.0 + 1e-6).value().empty());
    // Exactly the summit: the region is the apex alone, a point, which is dropped.
    EXPECT_TRUE(contourAt(surface, kPyramidHeight).value().empty());

    EXPECT_EQ(contours(surface, 0.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(contours(surface, -5.0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(contours(surface, nan).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(contours(surface, inf).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(contours(surface, katana::math::tolerance::kGeometric).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(contours(surface, 1.0, nan).error().code, ErrorCode::InvalidArgument);
    // 50 m of relief at a 0.1 mm interval is 500000 lines, past kMaxContourLevels.
    EXPECT_EQ(contours(surface, 1.0e-4).error().code, ErrorCode::InvalidArgument);

    const TinSurface empty;
    EXPECT_TRUE(contourAt(empty, 1.0).value().empty());
    EXPECT_TRUE(contours(empty, 1.0).value().empty());
}

// ---- properties on rough ground --------------------------------------------------------

class RoughGround : public ::testing::Test {
  protected:
    void SetUp() override
    {
        Random random;
        TinInput input;
        for (int i = 0; i < 1200; ++i) {
            const double x = random.real(0.0, 200.0);
            const double y = random.real(0.0, 200.0);
            input.points.emplace_back(x, y, elevation(x, y));
        }
        surface_ = build(input).surface;
        ASSERT_FALSE(surface_.empty());
    }

    // Smooth but genuinely curved ground, so the level sets are neither straight
    // nor all of one shape: several open contours and several rings per level.
    [[nodiscard]] static double elevation(double x, double y)
    {
        return 100.0 + 10.0 * std::sin(0.05 * x) * std::cos(0.04 * y);
    }

    TinSurface surface_;
};

TEST_F(RoughGround, EveryContourVertexInterpolatesToItsOwnLevel)
{
    const auto result = contours(surface_, 2.0, 100.0, 5);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_GE(result.value().size(), 4u);

    std::size_t vertices = 0;
    for (const Contour& contour : result.value()) {
        const std::vector<Point2>& v = contour.line.vertices;
        ASSERT_GE(v.size(), contour.line.closed ? 3u : 2u);
        for (std::size_t i = 0; i < v.size(); ++i) {
            ++vertices;
            // A vertex lies on a triangle edge, where locate() may pick either
            // side; both interpolate the same elevation because the surface is
            // continuous. Barycentric rounding on a 20 m relief stays far below
            // 1e-9 even on the thinnest Delaunay triangles here.
            const auto z = surface_.elevationAt(v[i]);
            ASSERT_TRUE(z.has_value()) << v[i];
            EXPECT_NEAR(*z, contour.elevation, 1e-9) << v[i];
            EXPECT_NE(v[i], v[(i + 1) % v.size()]); // no zero-length piece
        }
    }
    EXPECT_GT(vertices, 100u);
}

TEST_F(RoughGround, LevelsAreAscendingAndEachIsExactlyWhatContourAtGives)
{
    const double interval = 2.5;
    const double base = 99.0;
    const auto ladder = contours(surface_, interval, base, 0);
    ASSERT_TRUE(ladder.ok()) << ladder.error().describe();
    ASSERT_FALSE(ladder.value().empty());

    double previous = -std::numeric_limits<double>::infinity();
    std::size_t index = 0;
    while (index < ladder.value().size()) {
        const double level = ladder.value()[index].elevation;
        EXPECT_GT(level, previous);
        previous = level;
        // The level must be one of base + k * interval, and inside the relief.
        const double k = std::round((level - base) / interval);
        EXPECT_EQ(level, base + k * interval);
        EXPECT_GT(level, surface_.minElevation());
        EXPECT_LE(level, surface_.maxElevation());

        // Same level through the single-level entry point: identical geometry.
        const auto single = contourAt(surface_, level);
        ASSERT_TRUE(single.ok()) << single.error().describe();
        std::size_t count = 0;
        while (index + count < ladder.value().size() &&
               ladder.value()[index + count].elevation == level) {
            ++count;
        }
        ASSERT_EQ(single.value().size(), count) << "level " << level;
        for (std::size_t i = 0; i < count; ++i) {
            EXPECT_EQ(single.value()[i].line, ladder.value()[index + i].line) << "level " << level;
        }
        index += count;
    }
}

TEST_F(RoughGround, ContoursStayOnTheSurfaceAndOpenOnesEndOnItsRim)
{
    const auto result = contours(surface_, 3.0, 100.0, 5);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const Box2 bounds = surface_.bounds().inflated(katana::math::tolerance::kGeometric);

    // This surface has no boundary and no holes, so its rim is exactly the convex
    // hull of the input positions.
    std::vector<Point2> plan;
    plan.reserve(surface_.vertexCount());
    for (const Point3& p : surface_.vertices()) {
        plan.emplace_back(p.x, p.y);
    }
    const Polyline2 rim{katana::geometry::convexHull(std::move(plan)), true};

    std::size_t open = 0;
    std::size_t rings = 0;
    for (const Contour& contour : result.value()) {
        for (const Point2& p : contour.line.vertices) {
            EXPECT_TRUE(bounds.contains(p)) << p;
        }
        if (contour.line.closed) {
            ++rings;
            continue;
        }
        ++open;
        // An open contour is open because tracing ran out of triangles, which can
        // only happen on the rim. A crossing on a rim edge is that edge's own
        // interpolation, so it is on the hull to within its rounding.
        for (const Point2& end : {contour.line.vertices.front(), contour.line.vertices.back()}) {
            const auto distance = rim.distanceTo(end);
            ASSERT_TRUE(distance.has_value());
            EXPECT_LT(*distance, 1e-9) << end;
        }
    }
    EXPECT_GT(open, 0u);
    EXPECT_GT(rings, 0u);
}
