#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <limits>
#include <memory>
#include <string>
#include <vector>

#include "katana/geometry/polygon.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;
using katana::geometry::Polyline2;
using katana::geometry::Triangle2;
using katana::test::Random;

namespace {

// True when some triangle has the undirected edge between the two positions.
bool hasEdge(const TinSurface& surface, const Point2& p, const Point2& q, bool* constrained)
{
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        const TinTriangle& tri = surface.triangles()[t];
        for (std::size_t k = 0; k < 3; ++k) {
            const Point3& a = surface.vertices()[tri[k]];
            const Point3& b = surface.vertices()[tri[(k + 1) % 3]];
            const bool forward = a.x == p.x && a.y == p.y && b.x == q.x && b.y == q.y;
            const bool backward = a.x == q.x && a.y == q.y && b.x == p.x && b.y == p.y;
            if (forward || backward) {
                if (constrained != nullptr) {
                    *constrained = surface.isEdgeConstrained(t, k);
                }
                return true;
            }
        }
    }
    return false;
}

// Structural invariants every surface must satisfy.
void expectConsistent(const TinSurface& surface)
{
    ASSERT_EQ(surface.neighbors().size(), surface.triangleCount());
    ASSERT_EQ(surface.constrainedEdges().size(), surface.triangleCount());
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        const TinTriangle& tri = surface.triangles()[t];
        EXPECT_LT(tri[0], tri[1]) << "triangle must start at its smallest vertex index";
        EXPECT_LT(tri[0], tri[2]);
        EXPECT_GE(surface.planTriangle(t).signedArea(), 0.0) << "counter-clockwise in plan";
        if (t > 0) {
            EXPECT_LT(surface.triangles()[t - 1], tri) << "triangles must be sorted";
        }
        for (std::size_t k = 0; k < 3; ++k) {
            const std::uint32_t n = surface.neighbors()[t][k];
            if (n == kNoTriangle) {
                continue;
            }
            ASSERT_LT(n, surface.triangleCount());
            // The neighbour must run the shared edge the other way round and
            // point back at this triangle, with the same constraint flag.
            const TinTriangle& other = surface.triangles()[n];
            bool found = false;
            for (std::size_t j = 0; j < 3; ++j) {
                if (other[j] == tri[(k + 1) % 3] && other[(j + 1) % 3] == tri[k]) {
                    found = true;
                    EXPECT_EQ(surface.neighbors()[n][j], t);
                    EXPECT_EQ(surface.isEdgeConstrained(n, j), surface.isEdgeConstrained(t, k));
                }
            }
            EXPECT_TRUE(found) << "triangle " << t << " edge " << k;
        }
    }
}

} // namespace

// ---- basic construction ----------------------------------------------------------

TEST(TinBuilder, SingleTriangle)
{
    TinInput input;
    input.points = {Point3(0, 0, 1), Point3(4, 0, 2), Point3(0, 3, 3)};
    const TinBuildResult result = build(input);
    ASSERT_EQ(result.surface.triangleCount(), 1u);
    ASSERT_EQ(result.surface.vertexCount(), 3u);
    EXPECT_EQ(result.surface.triangles()[0], (TinTriangle{0, 1, 2}));
    EXPECT_DOUBLE_EQ(result.surface.planArea(), 6.0); // 4 * 3 / 2
    EXPECT_DOUBLE_EQ(result.surface.minElevation(), 1.0);
    EXPECT_DOUBLE_EQ(result.surface.maxElevation(), 3.0);
    EXPECT_EQ(result.surface.bounds().min, Point2(0, 0));
    EXPECT_EQ(result.surface.bounds().max, Point2(4, 3));
    EXPECT_EQ(result.report.pointVertex, (std::vector<std::uint32_t>{0, 1, 2}));
    expectConsistent(result.surface);
}

TEST(TinBuilder, VerticesKeepInputOrder)
{
    Random random;
    const std::vector<Point3> points = planePoints(Plane{0.1, 0.2, 3.0}, 50.0, 200, random);
    const TinSurface surface = buildFromPoints(points);
    ASSERT_EQ(surface.vertexCount(), points.size());
    for (std::size_t i = 0; i < points.size(); ++i) {
        EXPECT_EQ(surface.vertices()[i], points[i]);
    }
    // Euler: a triangulation of n points with h on the hull has 2n - 2 - h triangles.
    EXPECT_EQ(surface.triangleCount(), 2 * points.size() - 2 - 4);
    expectConsistent(surface);
}

// ---- breaklines -------------------------------------------------------------------

// Thin rhombus: left/right tips far apart, top/bottom close together. The
// circumcircle of (left, bottom, top) has its centre at (-2.4, 0) and radius 2.6,
// so the right tip (distance 7.4) is outside: Delaunay joins top and bottom. The
// breakline along the long diagonal is therefore the NON-Delaunay diagonal.
class BreaklineRhombus : public ::testing::Test {
  protected:
    const Point3 left_{-5, 0, 10};
    const Point3 right_{5, 0, 20};
    const Point3 top_{0, 1, 0};
    const Point3 bottom_{0, -1, 0};
};

TEST_F(BreaklineRhombus, WithoutBreaklineDelaunayUsesShortDiagonal)
{
    const TinSurface surface = buildFromPoints({left_, right_, top_, bottom_});
    ASSERT_EQ(surface.triangleCount(), 2u);
    EXPECT_TRUE(hasEdge(surface, Point2(0, 1), Point2(0, -1), nullptr));
    EXPECT_FALSE(hasEdge(surface, Point2(-5, 0), Point2(5, 0), nullptr));
    // The centre lies on the top-bottom edge, both ends at z = 0.
    ASSERT_TRUE(surface.elevationAt(0, 0).has_value());
    EXPECT_NEAR(*surface.elevationAt(0, 0), 0.0, 1e-12);
}

TEST_F(BreaklineRhombus, BreaklineForcesLongDiagonalAndElevationFollowsIt)
{
    TinInput input;
    input.points = {left_, right_, top_, bottom_};
    input.breaklines.push_back(Breakline{{left_, right_}, false});
    const TinBuildResult result = build(input);
    const TinSurface& surface = result.surface;
    ASSERT_EQ(surface.triangleCount(), 2u);
    ASSERT_EQ(surface.vertexCount(), 4u) << "breakline vertices coincide with the points";
    EXPECT_EQ(result.report.sharedBreaklineVertexCount, 2u);
    EXPECT_EQ(result.report.duplicatePointCount, 0u);

    bool constrained = false;
    EXPECT_TRUE(hasEdge(surface, Point2(-5, 0), Point2(5, 0), &constrained));
    EXPECT_TRUE(constrained);
    EXPECT_FALSE(hasEdge(surface, Point2(0, 1), Point2(0, -1), nullptr));

    // Along the breakline z runs linearly from 10 at x = -5 to 20 at x = 5.
    for (const double x : {-5.0, -2.5, 0.0, 1.25, 5.0}) {
        const auto z = surface.elevationAt(x, 0.0);
        ASSERT_TRUE(z.has_value()) << x;
        EXPECT_NEAR(*z, 15.0 + x, 1e-12) << x;
    }
    expectConsistent(surface);
}

TEST(TinBuilder, SurveyPointOnBreaklineSplitsItAndKeepsItsElevation)
{
    TinInput input;
    input.points = {Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 0), Point3(0, 10, 0),
                    Point3(5, 5, 7)}; // exactly on the diagonal breakline below
    input.breaklines.push_back(Breakline{{Point3(0, 0, 0), Point3(10, 10, 0)}, false});
    const TinSurface surface = build(input).surface;
    ASSERT_EQ(surface.vertexCount(), 5u);
    EXPECT_NEAR(*surface.elevationAt(5, 5), 7.0, 1e-12);
    EXPECT_NEAR(*surface.elevationAt(2.5, 2.5), 3.5, 1e-12); // halfway up to the point
    bool constrained = false;
    EXPECT_TRUE(hasEdge(surface, Point2(0, 0), Point2(5, 5), &constrained));
    EXPECT_TRUE(constrained);
    EXPECT_TRUE(hasEdge(surface, Point2(5, 5), Point2(10, 10), &constrained));
    EXPECT_TRUE(constrained);
}

TEST(TinBuilder, CrossingBreaklinesThatAgreeCreateAVertex)
{
    TinInput input;
    input.points = {Point3(0, 0, 0), Point3(10, 0, 5), Point3(10, 10, 10), Point3(0, 10, 5)};
    input.breaklines.push_back(Breakline{{Point3(0, 0, 0), Point3(10, 10, 10)}, false});
    input.breaklines.push_back(Breakline{{Point3(0, 10, 5), Point3(10, 0, 5)}, false});
    const TinBuildResult result = build(input);
    EXPECT_EQ(result.report.breaklineCrossingCount, 1u);
    EXPECT_EQ(result.report.crossingConflictCount, 0u);
    ASSERT_EQ(result.surface.vertexCount(), 5u);
    ASSERT_EQ(result.surface.triangleCount(), 4u);
    // Created vertices come after all input vertices. Both breaklines pass
    // (5, 5) at z = 5.
    EXPECT_NEAR(result.surface.vertices()[4].x, 5.0, 1e-12);
    EXPECT_NEAR(result.surface.vertices()[4].y, 5.0, 1e-12);
    EXPECT_NEAR(result.surface.vertices()[4].z, 5.0, 1e-12);
    expectConsistent(result.surface);
}

TEST(TinBuilder, CrossingBreaklinesThatDisagreeFollowThePolicy)
{
    TinInput input;
    input.points = {Point3(0, 0, 0), Point3(10, 0, 8), Point3(10, 10, 10), Point3(0, 10, 8)};
    input.breaklines.push_back(Breakline{{Point3(0, 0, 0), Point3(10, 10, 10)}, false}); // 5 at centre
    input.breaklines.push_back(Breakline{{Point3(0, 10, 8), Point3(10, 0, 8)}, false});  // 8 at centre

    const auto rejected = buildTin(input);
    ASSERT_FALSE(rejected.ok());
    EXPECT_EQ(rejected.error().code, ErrorCode::InvalidGeometry);

    TinBuildOptions options;
    options.crossingBreaklines = CrossingBreaklinePolicy::Average;
    const TinBuildResult averaged = build(input, options);
    EXPECT_EQ(averaged.report.breaklineCrossingCount, 1u);
    EXPECT_EQ(averaged.report.crossingConflictCount, 1u);
    ASSERT_EQ(averaged.surface.vertexCount(), 5u);
    EXPECT_NEAR(averaged.surface.vertices()[4].z, 6.5, 1e-12); // (5 + 8) / 2
}

// ---- boundary and holes --------------------------------------------------------------

TEST(TinBuilder, ConcaveBoundaryClipsTriangles)
{
    const Plane plane{0.5, -0.25, 3.0};
    TinInput input;
    input.points = gridPoints(plane, 11); // 0..10 in both directions
    // L shape: 10 x 4 strip along the bottom plus 4 x 6 strip up the left side.
    input.boundary = Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 4), Point2(4, 4),
                                Point2(4, 10), Point2(0, 10)},
                               true};
    const TinBuildResult result = build(input);
    const TinSurface& surface = result.surface;

    EXPECT_NEAR(surface.planArea(), 10.0 * 4.0 + 4.0 * 6.0, 1e-9);
    // Grid points strictly beyond the inner corner are no longer part of the
    // surface: x and y in 5..10 -> 36 points.
    EXPECT_EQ(result.report.droppedVertexCount, 36u);
    EXPECT_EQ(surface.vertexCount(), 121u - 36u);
    EXPECT_GT(result.report.clippedTriangleCount, 0u);
    EXPECT_EQ(result.report.pointVertex[10 * 11 + 10], kNoVertex); // point (10, 10)
    EXPECT_NE(result.report.pointVertex[0], kNoVertex);

    EXPECT_FALSE(surface.elevationAt(7, 7).has_value()) << "inside the hull, outside the L";
    EXPECT_FALSE(surface.elevationAt(4.5, 4.5).has_value());
    ASSERT_TRUE(surface.elevationAt(2, 8).has_value());
    EXPECT_NEAR(*surface.elevationAt(2, 8), plane.at(2, 8), 1e-9);
    ASSERT_TRUE(surface.elevationAt(8.5, 2.5).has_value());
    EXPECT_NEAR(*surface.elevationAt(8.5, 2.5), plane.at(8.5, 2.5), 1e-9);
    // The inner corner edges are surface rim now, and constrained.
    bool constrained = false;
    EXPECT_TRUE(hasEdge(surface, Point2(4, 4), Point2(5, 4), &constrained));
    EXPECT_TRUE(constrained);
    ASSERT_TRUE(surface.elevationAt(7, 4).has_value()) << "on the rim";
    expectConsistent(surface);
}

TEST(TinBuilder, BoundaryVerticesOffTheDataAreInterpolated)
{
    const Plane plane{0.5, -0.25, 3.0};
    TinInput input;
    input.points = gridPoints(plane, 11);
    // The same L moved half a cell inwards: no boundary vertex is a grid point.
    input.boundary = Polyline2{{Point2(0.5, 0.5), Point2(9.5, 0.5), Point2(9.5, 3.5),
                                Point2(3.5, 3.5), Point2(3.5, 9.5), Point2(0.5, 9.5)},
                               true};
    const TinSurface surface = build(input).surface;
    EXPECT_NEAR(surface.planArea(), 9.0 * 3.0 + 3.0 * 6.0, 1e-9);
    EXPECT_EQ(surface.bounds().min, Point2(0.5, 0.5));
    EXPECT_EQ(surface.bounds().max, Point2(9.5, 9.5));
    // The data is a plane, so the interpolated boundary vertices are on it too
    // and the clipped surface still reproduces the plane everywhere.
    Random random;
    for (int i = 0; i < 200; ++i) {
        const Point2 p(random.real(0.5, 9.5), random.real(0.5, 3.5));
        const auto z = surface.elevationAt(p);
        ASSERT_TRUE(z.has_value());
        EXPECT_NEAR(*z, plane.at(p.x, p.y), 1e-9);
    }
    EXPECT_FALSE(surface.elevationAt(0.25, 0.25).has_value());
    EXPECT_FALSE(surface.elevationAt(6, 6).has_value());
}

TEST(TinBuilder, ClockwiseBoundaryAndRepeatedClosingVertexAreAccepted)
{
    TinInput input;
    input.points = gridPoints(Plane{}, 5);
    input.boundary = Polyline2{{Point2(1, 1), Point2(1, 3), Point2(3, 3), Point2(3, 1),
                                Point2(1, 1)}, // clockwise, first vertex repeated
                               false};
    const TinSurface surface = build(input).surface;
    EXPECT_NEAR(surface.planArea(), 4.0, 1e-12);
}

TEST(TinBuilder, HoleRemovesTrianglesInside)
{
    const Plane plane{1.0, 0.0, 0.0};
    TinInput input;
    input.points = gridPoints(plane, 11);
    input.holes.push_back(
        Polyline2{{Point2(3, 3), Point2(6, 3), Point2(6, 6), Point2(3, 6)}, true});
    const TinBuildResult result = build(input);
    const TinSurface& surface = result.surface;
    EXPECT_NEAR(surface.planArea(), 100.0 - 9.0, 1e-9);
    EXPECT_EQ(result.report.droppedVertexCount, 4u); // (4,4) (5,4) (4,5) (5,5)
    EXPECT_FALSE(surface.elevationAt(4.5, 4.5).has_value());
    EXPECT_FALSE(surface.elevationAt(5.0, 5.0).has_value());
    ASSERT_TRUE(surface.elevationAt(3.0, 4.5).has_value()) << "rim of the hole";
    EXPECT_NEAR(*surface.elevationAt(3.0, 4.5), 3.0, 1e-12);
    ASSERT_TRUE(surface.elevationAt(8, 8).has_value());
    expectConsistent(surface);
}

TEST(TinBuilder, BoundaryWithHoleAndBreaklineTogether)
{
    TinInput input;
    input.points = gridPoints(Plane{0.0, 1.0, 0.0}, 11);
    input.boundary =
        Polyline2{{Point2(1, 1), Point2(9, 1), Point2(9, 9), Point2(1, 9)}, true};
    input.holes.push_back(
        Polyline2{{Point2(4, 4), Point2(6, 4), Point2(6, 6), Point2(4, 6)}, true});
    // Crosses the boundary twice and runs below the hole.
    input.breaklines.push_back(Breakline{{Point3(0, 2.5, 2.5), Point3(10, 2.5, 2.5)}, false});
    const TinSurface surface = build(input).surface;
    EXPECT_NEAR(surface.planArea(), 64.0 - 4.0, 1e-9);
    EXPECT_NEAR(*surface.elevationAt(5, 2.5), 2.5, 1e-12);
    EXPECT_FALSE(surface.elevationAt(0.5, 2.5).has_value()) << "breakline part outside";
    EXPECT_FALSE(surface.elevationAt(5, 5).has_value());
    expectConsistent(surface);
}

TEST(TinBuilder, RejectsBadRings)
{
    TinInput input;
    input.points = gridPoints(Plane{}, 11);

    // Asymmetric bow tie (signed area -16, so it is not rejected as degenerate):
    // edges (1,1)-(9,9) and (9,1)-(1,5) cross at x = y = 11/3, off the grid.
    input.boundary = Polyline2{{Point2(1, 1), Point2(9, 9), Point2(9, 1), Point2(1, 5)}, true};
    auto bowTie = buildTin(input);
    ASSERT_FALSE(bowTie.ok());
    EXPECT_EQ(bowTie.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(bowTie.error().message.find("crosses itself"), std::string::npos)
        << bowTie.error().describe();

    input.boundary = Polyline2{{Point2(1, 1), Point2(9, 9)}, true};
    auto twoVertices = buildTin(input);
    ASSERT_FALSE(twoVertices.ok());
    EXPECT_EQ(twoVertices.error().code, ErrorCode::InvalidGeometry);

    input.boundary = Polyline2{{Point2(1, 1), Point2(5, 5), Point2(9, 9)}, true};
    auto collinear = buildTin(input);
    ASSERT_FALSE(collinear.ok());
    EXPECT_EQ(collinear.error().code, ErrorCode::InvalidGeometry);

    // A vertex beyond the data cannot be given an elevation.
    input.boundary =
        Polyline2{{Point2(1, 1), Point2(12, 1), Point2(12, 9), Point2(1, 9)}, true};
    auto outside = buildTin(input);
    ASSERT_FALSE(outside.ok());
    EXPECT_EQ(outside.error().code, ErrorCode::InvalidGeometry);

    // Ring touching itself in a vertex (figure of eight through (5, 5)).
    input.boundary = Polyline2{{Point2(1, 1), Point2(5, 5), Point2(9, 1), Point2(9, 9),
                                Point2(5, 5), Point2(1, 9)},
                               true};
    auto touching = buildTin(input);
    ASSERT_FALSE(touching.ok());
    EXPECT_EQ(touching.error().code, ErrorCode::InvalidGeometry);

    // Everything clipped away.
    input.boundary = Polyline2{};
    input.holes.push_back(
        Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true});
    auto nothingLeft = buildTin(input);
    ASSERT_FALSE(nothingLeft.ok());
    EXPECT_EQ(nothingLeft.error().code, ErrorCode::TriangulationFailure);
}

// ---- duplicate points ---------------------------------------------------------------

class DuplicatePoints : public ::testing::Test {
  protected:
    static TinInput input(double duplicateZ, double offset = 0.0)
    {
        TinInput in;
        in.points = {Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 0), Point3(0, 10, 0),
                     Point3(5, 5, 4), Point3(5 + offset, 5, duplicateZ)};
        return in;
    }
};

TEST_F(DuplicatePoints, SameElevationIsMergedAndCounted)
{
    const TinBuildResult result = build(input(4.0));
    EXPECT_EQ(result.report.duplicatePointCount, 1u);
    EXPECT_EQ(result.report.elevationConflictCount, 0u);
    EXPECT_EQ(result.surface.vertexCount(), 5u);
    EXPECT_EQ(result.report.pointVertex[5], result.report.pointVertex[4]);
}

TEST_F(DuplicatePoints, WithinGeometricToleranceIsTheSamePosition)
{
    // 5e-8 m apart: below tolerance::kGeometric (1e-7 m).
    EXPECT_EQ(build(input(4.0, 5e-8)).surface.vertexCount(), 5u);
    // 1e-6 m apart: two vertices.
    const TinBuildResult apart = build(input(4.0, 1e-6));
    EXPECT_EQ(apart.surface.vertexCount(), 6u);
    EXPECT_EQ(apart.report.duplicatePointCount, 0u);
}

TEST_F(DuplicatePoints, ConflictingElevationIsAnErrorByDefault)
{
    const auto result = buildTin(input(4.5));
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(result.error().context.find("point 5"), std::string::npos)
        << result.error().describe();

    // A difference below the elevation tolerance (default 1e-4 m) is no conflict.
    EXPECT_TRUE(buildTin(input(4.00005)).ok());
}

TEST_F(DuplicatePoints, KeepFirstAndAveragePolicies)
{
    TinBuildOptions options;
    options.duplicatePoints = DuplicatePointPolicy::KeepFirst;
    const TinBuildResult first = build(input(6.0), options);
    EXPECT_EQ(first.report.duplicatePointCount, 1u);
    EXPECT_EQ(first.report.elevationConflictCount, 1u);
    EXPECT_DOUBLE_EQ(*first.surface.elevationAt(5, 5), 4.0);

    options.duplicatePoints = DuplicatePointPolicy::Average;
    const TinBuildResult average = build(input(6.0), options);
    EXPECT_EQ(average.report.elevationConflictCount, 1u);
    EXPECT_DOUBLE_EQ(*average.surface.elevationAt(5, 5), 5.0);
}

TEST_F(DuplicatePoints, BreaklineVertexConflictingWithPointIsAnError)
{
    TinInput in = input(4.0);
    in.breaklines.push_back(Breakline{{Point3(0, 0, 0), Point3(5, 5, 9)}, false});
    const auto result = buildTin(in);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidGeometry);
}

// ---- error cases ----------------------------------------------------------------------

TEST(TinBuilderErrors, TooFewOrCollinearPositions)
{
    TinInput input;
    auto empty = buildTin(input);
    ASSERT_FALSE(empty.ok());
    EXPECT_EQ(empty.error().code, ErrorCode::TriangulationFailure);

    input.points = {Point3(0, 0, 0), Point3(1, 1, 1)};
    auto two = buildTin(input);
    ASSERT_FALSE(two.ok());
    EXPECT_EQ(two.error().code, ErrorCode::TriangulationFailure);

    // Three points of which two coincide are two positions.
    input.points = {Point3(0, 0, 0), Point3(1, 1, 1), Point3(1, 1, 1)};
    auto coincident = buildTin(input);
    ASSERT_FALSE(coincident.ok());
    EXPECT_EQ(coincident.error().code, ErrorCode::TriangulationFailure);

    input.points = {Point3(0, 0, 0), Point3(1, 1, 1), Point3(2, 2, 5), Point3(7, 7, 2)};
    auto collinear = buildTin(input);
    ASSERT_FALSE(collinear.ok());
    EXPECT_EQ(collinear.error().code, ErrorCode::TriangulationFailure);
}

TEST(TinBuilderErrors, NonFiniteInput)
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    TinInput base;
    base.points = {Point3(0, 0, 0), Point3(4, 0, 0), Point3(0, 4, 0)};

    for (const Point3& bad : {Point3(nan, 1, 1), Point3(1, inf, 1), Point3(1, 1, nan)}) {
        TinInput input = base;
        input.points.push_back(bad);
        auto result = buildTin(input);
        ASSERT_FALSE(result.ok());
        EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(result.error().context.find("point=3"), std::string::npos);
    }

    TinInput breakline = base;
    breakline.breaklines.push_back(Breakline{{Point3(0, 0, 0), Point3(1, nan, 0)}, false});
    EXPECT_EQ(buildTin(breakline).error().code, ErrorCode::InvalidArgument);

    TinInput boundary = base;
    boundary.boundary = Polyline2{{Point2(0, 0), Point2(1, 0), Point2(inf, 1)}, true};
    EXPECT_EQ(buildTin(boundary).error().code, ErrorCode::InvalidArgument);
}

TEST(TinBuilderErrors, BadBreaklinesAndOptions)
{
    TinInput input;
    input.points = {Point3(0, 0, 0), Point3(4, 0, 0), Point3(0, 4, 0)};
    input.breaklines.push_back(Breakline{{Point3(1, 1, 0)}, false});
    EXPECT_EQ(buildTin(input).error().code, ErrorCode::InvalidGeometry);

    input.breaklines = {Breakline{{Point3(1, 1, 0), Point3(1, 1, 0)}, false}}; // no length
    EXPECT_EQ(buildTin(input).error().code, ErrorCode::InvalidGeometry);

    input.breaklines.clear();
    TinBuildOptions options;
    options.elevationTolerance = -1.0;
    EXPECT_EQ(buildTin(input, options).error().code, ErrorCode::InvalidArgument);
}

// ---- determinism -----------------------------------------------------------------------

TEST(TinBuilderDeterminism, IdenticalInputGivesBitwiseIdenticalSurface)
{
    Random random;
    TinInput input;
    for (int i = 0; i < 3000; ++i) {
        input.points.emplace_back(random.real(0, 500), random.real(0, 500), random.real(90, 110));
    }
    input.breaklines.push_back(
        Breakline{{Point3(50, 60, 100), Point3(450, 80, 101), Point3(430, 440, 99)}, false});
    input.breaklines.push_back(Breakline{{Point3(60, 450, 100), Point3(440, 50, 100)}, false});
    input.holes.push_back(
        Polyline2{{Point2(200, 200), Point2(260, 210), Point2(250, 270), Point2(190, 250)}, true});
    TinBuildOptions options;
    options.crossingBreaklines = CrossingBreaklinePolicy::Average;

    const TinBuildResult first = build(input, options);
    ASSERT_FALSE(first.surface.empty());
    // The first segment of breakline 0 (y = 60 + 0.05 (x - 50)) meets breakline 1
    // (y = 450 - (400/380) (x - 60)) at x = 413.2; no other pair of segments
    // crosses. Breakline 1 also runs through the hole, which creates vertices on
    // the hole's ring but is not a breakline crossing.
    EXPECT_EQ(first.report.breaklineCrossingCount, 1u);

    // Disturb the heap so that the second run sees different addresses: the
    // result must not depend on pointer values (hash or set order of handles).
    std::vector<std::unique_ptr<std::vector<double>>> ballast;
    for (std::size_t i = 1; i <= 257; ++i) {
        ballast.push_back(std::make_unique<std::vector<double>>(i * 37, 1.0));
    }
    const TinBuildResult second = build(input, options);
    ballast.clear();
    const TinBuildResult third = build(input, options);

    EXPECT_TRUE(first.surface == second.surface);
    EXPECT_TRUE(first.surface == third.surface);
    EXPECT_EQ(first.report.pointVertex, second.report.pointVertex);
    EXPECT_EQ(first.surface.planArea(), second.surface.planArea());       // bitwise
    EXPECT_EQ(first.surface.surfaceArea(), second.surface.surfaceArea()); // bitwise
    expectConsistent(first.surface);
}

// ---- properties ---------------------------------------------------------------------------

TEST(TinBuilderProperties, PlanAreaEqualsConvexHullArea)
{
    Random random;
    for (int iteration = 0; iteration < 25; ++iteration) {
        const int count = random.integer(3, 400);
        std::vector<Point3> points;
        std::vector<Point2> plan;
        for (int i = 0; i < count; ++i) {
            points.emplace_back(random.real(-100, 100), random.real(-50, 50), random.real(0, 10));
            plan.emplace_back(points.back().x, points.back().y);
        }
        const TinSurface surface = buildFromPoints(points);
        const double hullArea = Polyline2{katana::geometry::convexHull(plan), true}.area();
        // Sum of up to ~800 triangle areas of magnitude <= 2e4: rounding far below 1e-7.
        EXPECT_NEAR(surface.planArea(), hullArea, 1e-7 * std::max(1.0, hullArea)) << iteration;
    }
}

TEST(TinBuilderProperties, EveryInputPointInterpolatesToItsOwnElevation)
{
    Random random;
    std::vector<Point3> points;
    for (int i = 0; i < 2000; ++i) {
        points.emplace_back(random.real(0, 1000), random.real(0, 1000), random.real(-20, 80));
    }
    const TinSurface surface = buildFromPoints(points);
    for (const Point3& p : points) {
        const auto z = surface.elevationAt(p.x, p.y);
        ASSERT_TRUE(z.has_value());
        // At a vertex the barycentric weights are exactly (1, 0, 0).
        EXPECT_DOUBLE_EQ(*z, p.z);
    }
}

TEST(TinBuilderProperties, UnconstrainedTrianglesHaveEmptyCircumcircles)
{
    Random random;
    for (int iteration = 0; iteration < 5; ++iteration) {
        std::vector<Point3> points;
        for (int i = 0; i < 250; ++i) {
            points.emplace_back(random.real(0, 100), random.real(0, 100), 0.0);
        }
        const TinSurface surface = buildFromPoints(points);
        for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
            const auto circle = surface.planTriangle(t).circumcircle();
            if (!circle) {
                continue; // sliver: its circle is numerically meaningless
            }
            // Cocircular points are allowed; the slack covers the rounding of
            // the circle construction (relative to its radius).
            const double slack = 1e-9 * std::max(1.0, circle->radius);
            for (const Point3& p : points) {
                EXPECT_GE(circle->center.distanceTo(Point2(p.x, p.y)), circle->radius - slack)
                    << "triangle " << t;
            }
        }
    }
}

TEST(TinBuilderProperties, NearlyCollinearHullPointsDoNotBreakTheSurface)
{
    // (50, 1e-13) is a hair off the bottom edge: exact predicates make a sliver
    // triangle out of it that evaluates to (almost) zero area in doubles.
    TinInput input;
    input.points = {Point3(0, 0, 0), Point3(50, 1e-13, 10), Point3(100, 0, 0),
                    Point3(50, 50, 0)};
    const TinSurface surface = build(input).surface;
    EXPECT_EQ(surface.vertexCount(), 4u);
    EXPECT_NEAR(surface.planArea(), 2500.0, 1e-9);
    const auto z = surface.elevationAt(25, 0);
    ASSERT_TRUE(z.has_value());
    EXPECT_GE(*z, 0.0);
    EXPECT_LE(*z, 10.0);
    EXPECT_NEAR(*surface.elevationAt(50, 25), 5.0, 1e-9); // halfway between (50,0+) and (50,50)
    expectConsistent(surface);
}
