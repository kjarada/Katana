// Editing a surface (tin_edit.hpp, docs/surface_editing.md). Every surface
// here is small and made by hand, so each expected count, area and level is
// worked out on paper in the comment beside it.
#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <numbers>
#include <set>
#include <utility>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/terrain/tin_edit.hpp"

using katana::core::ErrorCode;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::Polyline2;
using katana::terrain::BreaklineVertex;
using katana::terrain::HeightChange;
using katana::terrain::HeightOperation;
using katana::terrain::HeldPointRule;
using katana::terrain::SmoothOptions;
using katana::terrain::TinSurface;
using katana::terrain::TinTriangle;

namespace {

TinSurface make(std::vector<Point3> vertices, std::vector<TinTriangle> triangles,
                std::vector<std::uint8_t> masks = {})
{
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles), std::move(masks));
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

// The square [0,10]^2 as two triangles split along the diagonal 0-2, the
// corners at the levels given.
TinSurface square(double z0 = 0, double z1 = 0, double z2 = 0, double z3 = 0,
                  bool diagonalIsBreakline = false)
{
    std::vector<std::uint8_t> masks;
    if (diagonalIsBreakline) {
        // Triangle {0,1,2} runs the diagonal as its edge 2 (2->0), {0,2,3} as
        // its edge 0 (0->2).
        masks = {std::uint8_t{4}, std::uint8_t{1}};
    }
    return make({Point3(0, 0, z0), Point3(10, 0, z1), Point3(10, 10, z2), Point3(0, 10, z3)},
                {{0, 1, 2}, {0, 2, 3}}, masks);
}

// The square [0,10]^2 with a vertex at its centre at `apex`, the corners at
// 0: four triangles meeting at the centre (a pyramid).
TinSurface pyramid(double apex = 10.0)
{
    return make({Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 0), Point3(0, 10, 0),
                 Point3(5, 5, apex)},
                {{0, 1, 4}, {1, 2, 4}, {2, 3, 4}, {0, 4, 3}});
}

// A grid of `cells` x `cells` squares of side `cell` from the origin, each
// split along its rising diagonal, the levels from `level(x, y)`.
template <typename Level> TinSurface grid(int cells, double cell, Level level)
{
    std::vector<Point3> vertices;
    for (int j = 0; j <= cells; ++j) {
        for (int i = 0; i <= cells; ++i) {
            vertices.emplace_back(i * cell, j * cell, level(i * cell, j * cell));
        }
    }
    const auto at = [cells](int i, int j) { return static_cast<std::uint32_t>(j * (cells + 1) + i); };
    std::vector<TinTriangle> triangles;
    for (int j = 0; j < cells; ++j) {
        for (int i = 0; i < cells; ++i) {
            triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
            triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
        }
    }
    return make(std::move(vertices), std::move(triangles));
}

TinSurface flatGrid(int cells, double cell, double z = 0.0)
{
    return grid(cells, cell, [z](double, double) { return z; });
}

Polyline2 rectangle(double x0, double y0, double x1, double y1)
{
    Polyline2 ring;
    ring.vertices = {Point2(x0, y0), Point2(x1, y0), Point2(x1, y1), Point2(x0, y1)};
    ring.closed = true;
    return ring;
}

// The plan length of the surface's constrained edges, each counted once.
double constrainedLength(const TinSurface& surface)
{
    std::set<std::pair<std::uint32_t, std::uint32_t>> seen;
    double length = 0.0;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        for (std::size_t k = 0; k < 3; ++k) {
            if (!surface.isEdgeConstrained(t, k)) {
                continue;
            }
            const std::uint32_t a = surface.triangles()[t][k];
            const std::uint32_t b = surface.triangles()[t][(k + 1) % 3];
            if (seen.insert({std::min(a, b), std::max(a, b)}).second) {
                const Point3& p = surface.vertices()[a];
                const Point3& q = surface.vertices()[b];
                length += std::hypot(q.x - p.x, q.y - p.y);
            }
        }
    }
    return length;
}

bool hasEdge(const TinSurface& surface, std::uint32_t a, std::uint32_t b)
{
    for (const TinTriangle& tri : surface.triangles()) {
        for (std::size_t k = 0; k < 3; ++k) {
            if ((tri[k] == a && tri[(k + 1) % 3] == b) || (tri[k] == b && tri[(k + 1) % 3] == a)) {
                return true;
            }
        }
    }
    return false;
}

// The vertex of `surface` at (x, y), or -1.
int vertexIndex(const TinSurface& surface, double x, double y)
{
    for (std::size_t v = 0; v < surface.vertexCount(); ++v) {
        if (surface.vertices()[v].x == x && surface.vertices()[v].y == y) {
            return static_cast<int>(v);
        }
    }
    return -1;
}

} // namespace

// ---- flipping ------------------------------------------------------------------------------

TEST(TinEdit, FlippingTheDiagonalOfASquareOfTwoTrianglesGivesTheOtherDiagonal)
{
    // Corners 1 and 3 at 1, corners 0 and 2 at 0: along the diagonal 0-2 the
    // centre is at 0, along the other diagonal 1-3 it is at 1.
    const TinSurface before = square(0, 1, 0, 1);
    ASSERT_NEAR(*before.elevationAt(5, 5), 0.0, 1e-12);
    const auto flipped = katana::terrain::flipEdge(before, 0, 2); // triangle {0,1,2}, edge 2->0
    ASSERT_TRUE(flipped.ok()) << flipped.error().describe();
    const TinSurface& after = flipped->surface;
    EXPECT_EQ(after.triangleCount(), 2u);
    EXPECT_EQ(after.vertexCount(), 4u);
    EXPECT_TRUE(hasEdge(after, 1, 3));
    EXPECT_FALSE(hasEdge(after, 0, 2));
    EXPECT_NEAR(*after.elevationAt(5, 5), 1.0, 1e-12);
    EXPECT_NEAR(after.planArea(), 100.0, 1e-9);
}

TEST(TinEdit, FlippingAnEdgeOnTheRimIsRefused)
{
    const auto flipped = katana::terrain::flipEdge(square(), 0, 0); // 0->1, the bottom side
    ASSERT_FALSE(flipped.ok());
    EXPECT_EQ(flipped.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(flipped.error().message.find("rim"), std::string::npos);
}

TEST(TinEdit, FlippingABreaklineIsRefused)
{
    const auto flipped = katana::terrain::flipEdge(square(0, 0, 0, 0, true), 0, 2);
    ASSERT_FALSE(flipped.ok());
    EXPECT_EQ(flipped.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(flipped.error().message.find("breakline"), std::string::npos);
}

TEST(TinEdit, FlippingWhereTheQuadrilateralIsNotConvexIsRefused)
{
    // P(0,0) Q(10,0) R(5,5) above, S(12,-1) below and beyond Q: the quad
    // P S Q R bends inwards at Q, so the diagonal R-S passes outside it.
    // (S - Q) x (R - S) = (2,-1) x (-7,6)... the triangle S, Q, R turns
    // clockwise: (Q - S) x (R - S) = (-2,1) x (-7,6) = -12 + 7 = -5.
    const TinSurface dart = make({Point3(0, 0, 0), Point3(10, 0, 0), Point3(5, 5, 0),
                                  Point3(12, -1, 0)},
                                 {{0, 1, 2}, {1, 0, 3}});
    const auto flipped = katana::terrain::flipEdge(dart, 0, 0);
    ASSERT_FALSE(flipped.ok());
    EXPECT_EQ(flipped.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(flipped.error().message.find("convex"), std::string::npos);
}

TEST(TinEdit, FlippingAnEdgeThatDoesNotExistIsRefused)
{
    EXPECT_EQ(katana::terrain::flipEdge(square(), 2, 0).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::flipEdge(square(), 0, 3).error().code, ErrorCode::InvalidArgument);
}

// ---- adding points ---------------------------------------------------------------------------

TEST(TinEdit, APointAddedInTheMiddleOfASquareGivesFourTrianglesAtTheRightHeights)
{
    // The centre lies on the diagonal, so both triangles are split: four
    // triangles meeting at (5,5,10). (5,2.5) is 0.25 (0,0) + 0.25 (10,0) +
    // 0.5 (5,5), so its level is 0.5 * 10 = 5.
    const auto added = katana::terrain::addPoints(square(), std::vector<Point3>{Point3(5, 5, 10)});
    ASSERT_TRUE(added.ok()) << added.error().describe();
    const TinSurface& after = added->surface;
    EXPECT_EQ(after.triangleCount(), 4u);
    EXPECT_EQ(after.vertexCount(), 5u);
    EXPECT_EQ(added->report.pointsAdded, 1u);
    EXPECT_EQ(added->report.trianglesBefore, 2u);
    EXPECT_EQ(added->report.trianglesAfter, 4u);
    EXPECT_DOUBLE_EQ(*after.elevationAt(5, 5), 10.0);
    EXPECT_NEAR(*after.elevationAt(5, 2.5), 5.0, 1e-12);
    EXPECT_NEAR(*after.elevationAt(2.5, 2.5), 5.0, 1e-12);
    EXPECT_NEAR(after.planArea(), 100.0, 1e-9);
}

TEST(TinEdit, APointInsideATriangleIsInsertedWhereItLies)
{
    // (7,3) is inside {0,1,2}; the triangle across the diagonal has (7,3)
    // inside its circumcircle (centre (5,5), radius 7.07; |(7,3)-(5,5)| =
    // 2.83), so both are rebuilt: five points, four triangles.
    const auto added = katana::terrain::addPoints(square(), std::vector<Point3>{Point3(7, 3, 2)});
    ASSERT_TRUE(added.ok()) << added.error().describe();
    EXPECT_EQ(added->surface.triangleCount(), 4u);
    EXPECT_DOUBLE_EQ(*added->surface.elevationAt(7, 3), 2.0);
}

TEST(TinEdit, APointAtAnExistingVertexGivesItItsLevelAndChangesNoTriangle)
{
    const TinSurface before = square();
    const auto added = katana::terrain::addPoints(before, std::vector<Point3>{Point3(10, 10, 7)});
    ASSERT_TRUE(added.ok()) << added.error().describe();
    EXPECT_EQ(added->surface.triangles(), before.triangles());
    EXPECT_EQ(added->report.pointsRelevelled, 1u);
    EXPECT_EQ(added->report.pointsAdded, 0u);
    EXPECT_DOUBLE_EQ(*added->surface.elevationAt(10, 10), 7.0);
}

TEST(TinEdit, APointOffTheSurfaceExtendsItByTheTrianglesJoiningItToTheRim)
{
    // (15,5) sees only the side x = 10: one new triangle (10,0) (15,5)
    // (10,10) of base 10 and height 5, area 25.
    const auto added = katana::terrain::addPoints(square(), std::vector<Point3>{Point3(15, 5, 3)});
    ASSERT_TRUE(added.ok()) << added.error().describe();
    EXPECT_EQ(added->surface.triangleCount(), 3u);
    EXPECT_NEAR(added->surface.planArea(), 125.0, 1e-9);
    EXPECT_DOUBLE_EQ(*added->surface.elevationAt(15, 5), 3.0);
    // Halfway from the side to the point: half its level.
    EXPECT_NEAR(*added->surface.elevationAt(12.5, 5), 1.5, 1e-12);
}

TEST(TinEdit, APointOnABreaklineSplitsItAndKeepsItsOwnLevel)
{
    // The diagonal is a breakline; (5,5) on it splits it in two, both halves
    // still constrained: the constrained length stays the diagonal's 10 sqrt 2.
    const auto added =
        katana::terrain::addPoints(square(0, 0, 0, 0, true), std::vector<Point3>{Point3(5, 5, 9)});
    ASSERT_TRUE(added.ok()) << added.error().describe();
    EXPECT_EQ(added->surface.triangleCount(), 4u);
    EXPECT_DOUBLE_EQ(*added->surface.elevationAt(5, 5), 9.0);
    EXPECT_NEAR(constrainedLength(added->surface), 10.0 * std::sqrt(2.0), 1e-9);
}

TEST(TinEdit, TwoPointsAtOnePositionWithDifferentLevelsAreRefused)
{
    const auto added = katana::terrain::addPoints(
        square(), std::vector<Point3>{Point3(3, 3, 1), Point3(3, 3, 2)});
    ASSERT_FALSE(added.ok());
    EXPECT_EQ(added.error().code, ErrorCode::InvalidGeometry);
}

TEST(TinEdit, AddingNoPointOrANonFiniteOneIsRefused)
{
    EXPECT_EQ(katana::terrain::addPoints(square(), std::vector<Point3>{}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::addPoints(square(), std::vector<Point3>{Point3(1, NAN, 0)})
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::addPoints(TinSurface{}, std::vector<Point3>{Point3(1, 1, 0)})
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
}

// ---- deleting ----------------------------------------------------------------------------------

TEST(TinEdit, DeletingTheCentreOfAPyramidGivesTheFlatSquareBack)
{
    const TinSurface before = pyramid();
    const auto deleted = katana::terrain::deletePoints(before, std::vector<std::uint32_t>{4});
    ASSERT_TRUE(deleted.ok()) << deleted.error().describe();
    EXPECT_EQ(deleted->surface.triangleCount(), 2u);
    EXPECT_EQ(deleted->surface.vertexCount(), 4u);
    EXPECT_EQ(deleted->report.pointsRemoved, 1u);
    EXPECT_DOUBLE_EQ(deleted->surface.maxElevation(), 0.0);
    EXPECT_NEAR(deleted->surface.planArea(), 100.0, 1e-9);
}

TEST(TinEdit, APointOnABreaklineIsRefusedAloneAndHeldInsideAPolygon)
{
    // The pyramid with its spoke 0-4 a breakline: {0,1,4} runs it as edge 2
    // (4->0), {0,4,3} as edge 0 (0->4).
    const TinSurface spoked =
        make({Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 0), Point3(0, 10, 0),
              Point3(5, 5, 10)},
             {{0, 1, 4}, {1, 2, 4}, {2, 3, 4}, {0, 4, 3}},
             {std::uint8_t{4}, std::uint8_t{0}, std::uint8_t{0}, std::uint8_t{1}});
    const auto refused = katana::terrain::deletePoints(spoked, std::vector<std::uint32_t>{4});
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
    const auto held = katana::terrain::deletePoints(spoked, std::vector<std::uint32_t>{4},
                                                    HeldPointRule::Skip);
    ASSERT_TRUE(held.ok()) << held.error().describe();
    EXPECT_EQ(held->surface, spoked);
    EXPECT_EQ(held->report.pointsHeld, 1u);
}

TEST(TinEdit, DeletingACornerOfTheRimNeverGrowsTheFootprint)
{
    // A 2 x 2 grid of unit cells, 8 triangles, area 4. The corner (0,0)
    // belongs to both triangles of its cell; without it the cell keeps only
    // the triangle (1,0) (1,1) (0,1): 7 triangles, area 3.5.
    const TinSurface before = flatGrid(2, 1.0);
    const auto deleted = katana::terrain::deletePoints(before, std::vector<std::uint32_t>{0});
    ASSERT_TRUE(deleted.ok()) << deleted.error().describe();
    EXPECT_EQ(deleted->surface.triangleCount(), 7u);
    EXPECT_NEAR(deleted->surface.planArea(), 3.5, 1e-12);
    EXPECT_FALSE(deleted->surface.elevationAt(0.2, 0.2).has_value());
}

TEST(TinEdit, DeletingEveryPointIsRefusedForLeavingNothing)
{
    const auto deleted =
        katana::terrain::deletePoints(square(), std::vector<std::uint32_t>{0, 1, 2, 3});
    ASSERT_FALSE(deleted.ok());
    EXPECT_EQ(deleted.error().code, ErrorCode::InvalidState);
}

TEST(TinEdit, DeletingAPointOutOfRangeIsRefused)
{
    EXPECT_EQ(katana::terrain::deletePoints(square(), std::vector<std::uint32_t>{9}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::deletePoints(square(), std::vector<std::uint32_t>{}).error().code,
              ErrorCode::InvalidArgument);
}

TEST(TinEdit, DeletingTrianglesRemovesExactlyThose)
{
    // A 4 x 4 grid of unit cells: 32 triangles, area 16. The triangle under
    // (0.6, 0.2) is the lower half of the first cell, area 0.5.
    const TinSurface before = flatGrid(4, 1.0);
    const auto under = katana::terrain::triangleAt(before, Point2(0.6, 0.2));
    ASSERT_TRUE(under.has_value());
    const auto deleted =
        katana::terrain::deleteTriangles(before, std::vector<std::uint32_t>{*under});
    ASSERT_TRUE(deleted.ok()) << deleted.error().describe();
    EXPECT_EQ(deleted->surface.triangleCount(), 31u);
    EXPECT_NEAR(deleted->surface.planArea(), 15.5, 1e-12);
    EXPECT_FALSE(deleted->surface.elevationAt(0.6, 0.2).has_value());
    EXPECT_TRUE(deleted->surface.elevationAt(0.2, 0.6).has_value());
}

TEST(TinEdit, TrianglesInsideAndOutsideAPolygonAreTakenByTheirCentroids)
{
    // The 4 x 4 grid: cell (i, j)'s triangles have their centroids at
    // (i + 2/3, j + 1/3) and (i + 1/3, j + 2/3). Inside [0.9, 2.1]^2 a
    // coordinate i + 1/3 or i + 2/3 lies only for i = 1 (1.33, 1.67; i = 0
    // gives 0.33, 0.67 and i = 2 gives 2.33, 2.67), so only cell (1, 1)'s two
    // triangles are inside: 2 inside, 30 outside.
    const TinSurface before = flatGrid(4, 1.0);
    const Polyline2 polygon = rectangle(0.9, 0.9, 2.1, 2.1);
    EXPECT_EQ(katana::terrain::trianglesByPolygon(before, polygon, true).size(), 2u);
    EXPECT_EQ(katana::terrain::trianglesByPolygon(before, polygon, false).size(), 30u);
}

TEST(TinEdit, LongAndThinTrianglesAreFoundByTheirSidesAndAngles)
{
    // A unit cell's triangles have sides 1, 1 and sqrt 2 and angles 45, 45
    // and 90 degrees.
    const TinSurface cells = flatGrid(2, 1.0);
    EXPECT_EQ(katana::terrain::trianglesLongerThan(cells, 1.5).size(), 0u);
    EXPECT_EQ(katana::terrain::trianglesLongerThan(cells, 1.4).size(), 8u);
    EXPECT_EQ(katana::terrain::trianglesThinnerThan(cells, 44.0 * std::numbers::pi / 180.0).size(), 0u);
    EXPECT_EQ(katana::terrain::trianglesThinnerThan(cells, 46.0 * std::numbers::pi / 180.0).size(), 8u);
}

TEST(TinEdit, DeletingEveryTriangleIsRefused)
{
    const auto deleted =
        katana::terrain::deleteTriangles(square(), std::vector<std::uint32_t>{0, 1});
    ASSERT_FALSE(deleted.ok());
    EXPECT_EQ(deleted.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(katana::terrain::deleteTriangles(square(), std::vector<std::uint32_t>{5}).error().code,
              ErrorCode::InvalidArgument);
}

// ---- breaklines -----------------------------------------------------------------------------------

TEST(TinEdit, ABreaklineAcrossASquareBecomesConstrainedEdgesAtItsLevels)
{
    // From (0,5) to (10,5) at 2: the lower half is the plane through (0,0,0),
    // (10,0,0) and (10,5,2), z = 0.4 y, so (5,2.5) is at 1.
    const std::vector<BreaklineVertex> line{{Point2(0, 5), 2.0}, {Point2(10, 5), 2.0}};
    const auto added = katana::terrain::addBreakline(square(), line, false);
    ASSERT_TRUE(added.ok()) << added.error().describe();
    const TinSurface& after = added->surface;
    EXPECT_NEAR(constrainedLength(after), 10.0, 1e-9);
    EXPECT_NEAR(*after.elevationAt(5, 5), 2.0, 1e-12);
    EXPECT_NEAR(*after.elevationAt(5, 2.5), 1.0, 1e-12);
    EXPECT_NEAR(*after.elevationAt(5, 0), 0.0, 1e-12);
    EXPECT_NEAR(after.planArea(), 100.0, 1e-9);
}

TEST(TinEdit, ADrapedBreaklineKeepsTheShapeOfTheSurface)
{
    // y = 2.5 across the pyramid crosses the spokes 0-4 and 1-4 at (2.5,2.5)
    // and (7.5,2.5), halfway up them: level 5. The surface must be what it
    // was everywhere; only the edges are new.
    const TinSurface before = pyramid();
    const std::vector<BreaklineVertex> line{{Point2(0, 2.5), std::nullopt},
                                            {Point2(10, 2.5), std::nullopt}};
    const auto added = katana::terrain::addBreakline(before, line, false);
    ASSERT_TRUE(added.ok()) << added.error().describe();
    const TinSurface& after = added->surface;
    EXPECT_EQ(added->report.crossings, 2u);
    EXPECT_NEAR(constrainedLength(after), 10.0, 1e-9);
    EXPECT_NEAR(*after.elevationAt(2.5, 2.5), 5.0, 1e-12);
    for (double x = 0.25; x < 10.0; x += 0.5) {
        for (double y = 0.25; y < 10.0; y += 0.5) {
            EXPECT_NEAR(*after.elevationAt(x, y), *before.elevationAt(x, y), 1e-9)
                << "at " << x << ", " << y;
        }
    }
}

TEST(TinEdit, ABreaklineCrossingABreaklineMeetsItAtItsLevel)
{
    const std::vector<BreaklineVertex> first{{Point2(0, 5), 2.0}, {Point2(10, 5), 2.0}};
    const auto one = katana::terrain::addBreakline(square(), first, false);
    ASSERT_TRUE(one.ok()) << one.error().describe();
    // The second runs at 0 from end to end, but where it crosses the first
    // the first's level, 2, holds.
    const std::vector<BreaklineVertex> second{{Point2(5, 0), 0.0}, {Point2(5, 10), 0.0}};
    const auto two = katana::terrain::addBreakline(one->surface, second, false);
    ASSERT_TRUE(two.ok()) << two.error().describe();
    EXPECT_EQ(two->report.crossings, 1u);
    EXPECT_NEAR(*two->surface.elevationAt(5, 5), 2.0, 1e-12);
    EXPECT_NEAR(constrainedLength(two->surface), 20.0, 1e-9);
}

TEST(TinEdit, ABreaklineOffTheSurfaceOrAcrossAHoleIsRefused)
{
    const std::vector<BreaklineVertex> off{{Point2(5, 5), 1.0}, {Point2(20, 5), 1.0}};
    const auto outside = katana::terrain::addBreakline(square(), off, false);
    ASSERT_FALSE(outside.ok());
    EXPECT_EQ(outside.error().code, ErrorCode::InvalidGeometry);

    katana::terrain::TinInput input;
    input.points = {Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 0), Point3(0, 10, 0)};
    input.holes.push_back(rectangle(4, 4, 6, 6));
    auto holed = katana::terrain::buildTin(input);
    ASSERT_TRUE(holed.ok()) << holed.error().describe();
    const std::vector<BreaklineVertex> across{{Point2(1, 5), std::nullopt},
                                              {Point2(9, 5), std::nullopt}};
    const auto throughHole = katana::terrain::addBreakline(holed->surface, across, false);
    ASSERT_FALSE(throughHole.ok());
    EXPECT_EQ(throughHole.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(throughHole.error().message.find("leaves the surface"), std::string::npos);
}

TEST(TinEdit, ABreaklineWithTooFewVerticesOrNoLengthIsRefused)
{
    const std::vector<BreaklineVertex> one{{Point2(5, 5), 1.0}};
    EXPECT_EQ(katana::terrain::addBreakline(square(), one, false).error().code,
              ErrorCode::InvalidArgument);
    const std::vector<BreaklineVertex> none{{Point2(5, 5), 1.0}, {Point2(5, 5), 1.0}};
    EXPECT_EQ(katana::terrain::addBreakline(square(), none, false).error().code,
              ErrorCode::InvalidGeometry);
}

// ---- boundaries and holes -------------------------------------------------------------------------

TEST(TinEdit, ABoundaryClipsExactlyAndKeepsTheShapeInside)
{
    // [2,8]^2 out of the pyramid: area 36, the rim its 24 m perimeter, all of
    // it constrained, and every level inside what it was.
    const TinSurface before = pyramid();
    const auto clipped = katana::terrain::clipToBoundary(before, rectangle(2, 2, 8, 8));
    ASSERT_TRUE(clipped.ok()) << clipped.error().describe();
    const TinSurface& after = clipped->surface;
    EXPECT_NEAR(after.planArea(), 36.0, 1e-9);
    EXPECT_NEAR(constrainedLength(after), 24.0, 1e-9);
    EXPECT_FALSE(after.elevationAt(1, 1).has_value());
    for (double x = 2.25; x < 8.0; x += 0.5) {
        for (double y = 2.25; y < 8.0; y += 0.5) {
            EXPECT_NEAR(*after.elevationAt(x, y), *before.elevationAt(x, y), 1e-9);
        }
    }
}

TEST(TinEdit, AHoleRemovesExactlyItsArea)
{
    // A 10 x 10 grid of unit cells less [3,6] x [3,7]: 100 - 12 = 88.
    const TinSurface before = flatGrid(10, 1.0, 4.0);
    const auto holed = katana::terrain::cutHole(before, rectangle(3, 3, 6, 7));
    ASSERT_TRUE(holed.ok()) << holed.error().describe();
    EXPECT_NEAR(holed->surface.planArea(), 88.0, 1e-9);
    EXPECT_FALSE(holed->surface.elevationAt(4.5, 5).has_value());
    EXPECT_DOUBLE_EQ(*holed->surface.elevationAt(1, 1), 4.0);
}

TEST(TinEdit, AHoleNotOnTheGridLinesIsCutExactly)
{
    // A tilted plane, z = 0.5 x + 0.25 y, and a hole [2.3, 4.6] x [1.2, 3.9]
    // that cuts through cells: area 100 - 2.3 * 2.7 = 93.79.
    const TinSurface before = grid(10, 1.0, [](double x, double y) { return 0.5 * x + 0.25 * y; });
    const auto holed = katana::terrain::cutHole(before, rectangle(2.3, 1.2, 4.6, 3.9));
    ASSERT_TRUE(holed.ok()) << holed.error().describe();
    EXPECT_NEAR(holed->surface.planArea(), 100.0 - 2.3 * 2.7, 1e-9);
    // The new rim's vertices lie on the plane.
    for (const Point3& v : holed->surface.vertices()) {
        EXPECT_NEAR(v.z, 0.5 * v.x + 0.25 * v.y, 1e-12);
    }
}

TEST(TinEdit, ABoundaryAroundTheWholeSurfaceChangesNothing)
{
    const TinSurface before = pyramid();
    const auto clipped = katana::terrain::clipToBoundary(before, rectangle(-5, -5, 15, 15));
    ASSERT_TRUE(clipped.ok()) << clipped.error().describe();
    EXPECT_EQ(clipped->surface, before);
}

TEST(TinEdit, ABoundaryBesideTheSurfaceIsRefusedForLeavingNothing)
{
    const auto clipped = katana::terrain::clipToBoundary(pyramid(), rectangle(20, 20, 30, 30));
    ASSERT_FALSE(clipped.ok());
    EXPECT_EQ(clipped.error().code, ErrorCode::InvalidState);
}

TEST(TinEdit, APolygonThatCrossesItselfOrHasNoAreaIsRefused)
{
    // (9,2)-(2,9) and (8,8)-(1,1) cross at (5.5, 5.5), on the surface.
    Polyline2 crossing;
    crossing.vertices = {Point2(1, 1), Point2(9, 2), Point2(2, 9), Point2(8, 8)};
    const auto crossed = katana::terrain::cutHole(flatGrid(10, 1.0), crossing);
    ASSERT_FALSE(crossed.ok());
    EXPECT_EQ(crossed.error().code, ErrorCode::InvalidGeometry);

    Polyline2 flat;
    flat.vertices = {Point2(1, 1), Point2(5, 1), Point2(9, 1)};
    EXPECT_EQ(katana::terrain::cutHole(flatGrid(10, 1.0), flat).error().code,
              ErrorCode::InvalidGeometry);
    Polyline2 two;
    two.vertices = {Point2(1, 1), Point2(5, 5)};
    EXPECT_EQ(katana::terrain::clipToBoundary(flatGrid(10, 1.0), two).error().code,
              ErrorCode::InvalidGeometry);
}

// ---- levels ---------------------------------------------------------------------------------------

TEST(TinEdit, RaisingByOneAddsExactlyOneToEveryHeight)
{
    const TinSurface before = grid(4, 2.5, [](double x, double y) { return 0.3 * x + 0.2 * y + 17.1; });
    const auto raised = katana::terrain::changeHeights(before, HeightChange{HeightOperation::Raise, 1.0});
    ASSERT_TRUE(raised.ok()) << raised.error().describe();
    ASSERT_EQ(raised->surface.vertexCount(), before.vertexCount());
    for (std::size_t v = 0; v < before.vertexCount(); ++v) {
        EXPECT_EQ(raised->surface.vertices()[v].z, before.vertices()[v].z + 1.0);
    }
    EXPECT_EQ(raised->surface.triangles(), before.triangles());
    EXPECT_EQ(raised->report.pointsMoved, before.vertexCount());
}

TEST(TinEdit, AHeightChangeInsideAPolygonMovesOnlyTheVerticesInside)
{
    // A 4 x 4 grid of unit cells at 0: inside [0.5, 2.5]^2 lie the vertices
    // (1..2, 1..2), four of them.
    const TinSurface before = flatGrid(4, 1.0);
    const auto set = katana::terrain::changeHeights(
        before, HeightChange{HeightOperation::SetLevel, 3.0}, rectangle(0.5, 0.5, 2.5, 2.5));
    ASSERT_TRUE(set.ok()) << set.error().describe();
    EXPECT_EQ(set->report.pointsMoved, 4u);
    EXPECT_DOUBLE_EQ(*set->surface.elevationAt(1, 1), 3.0);
    EXPECT_DOUBLE_EQ(*set->surface.elevationAt(2, 2), 3.0);
    EXPECT_DOUBLE_EQ(*set->surface.elevationAt(3, 3), 0.0);
    EXPECT_DOUBLE_EQ(*set->surface.elevationAt(0, 0), 0.0);
}

TEST(TinEdit, ScalingStretchesHeightsAboutTheBase)
{
    // z = base + 2 (z - base): about 1, the level 3 goes to 5; with no base,
    // the lowest level (the corners at 0) stays and the apex 10 goes to 20.
    const auto about = katana::terrain::changeHeights(
        pyramid(3.0), HeightChange{HeightOperation::Scale, 2.0, 1.0});
    ASSERT_TRUE(about.ok()) << about.error().describe();
    EXPECT_DOUBLE_EQ(*about->surface.elevationAt(5, 5), 5.0);
    EXPECT_DOUBLE_EQ(*about->surface.elevationAt(0, 0), -1.0);
    const auto lowest =
        katana::terrain::changeHeights(pyramid(10.0), HeightChange{HeightOperation::Scale, 2.0});
    ASSERT_TRUE(lowest.ok()) << lowest.error().describe();
    EXPECT_DOUBLE_EQ(*lowest->surface.elevationAt(5, 5), 20.0);
    EXPECT_DOUBLE_EQ(*lowest->surface.elevationAt(0, 0), 0.0);
}

TEST(TinEdit, AHeightChangeThatCannotBeMadeIsRefused)
{
    EXPECT_EQ(katana::terrain::changeHeights(pyramid(), HeightChange{HeightOperation::Raise, NAN})
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::changeHeights(pyramid(), HeightChange{HeightOperation::Raise, 1.0},
                                             rectangle(20, 20, 30, 30))
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
}

TEST(TinEdit, OneSmoothingPassMovesAVertexTowardsTheMeanOfItsNeighbours)
{
    // The apex at 10 has the four corners, all on the rim and held, as its
    // neighbours: mean 0. Weight 1 puts it at 0, weight 0.5 halfway, at 5.
    const auto all = katana::terrain::smoothSurface(pyramid(), SmoothOptions{1, 1.0});
    ASSERT_TRUE(all.ok()) << all.error().describe();
    EXPECT_DOUBLE_EQ(*all->surface.elevationAt(5, 5), 0.0);
    EXPECT_EQ(all->report.pointsMoved, 1u);
    EXPECT_EQ(all->report.pointsHeld, 4u);
    const auto half = katana::terrain::smoothSurface(pyramid(), SmoothOptions{1, 0.5});
    ASSERT_TRUE(half.ok()) << half.error().describe();
    EXPECT_DOUBLE_EQ(*half->surface.elevationAt(5, 5), 5.0);
}

TEST(TinEdit, SmoothingHoldsBreaklineVertices)
{
    // A 4 x 4 grid with a spike of 8 at (2,2) and a breakline along y = 1 at
    // the grid's level 0. The vertices on the breakline and on the rim must
    // not move; the spike must come down.
    katana::terrain::TinInput input;
    for (int j = 0; j <= 4; ++j) {
        for (int i = 0; i <= 4; ++i) {
            input.points.emplace_back(i, j, (i == 2 && j == 2) ? 8.0 : 0.0);
        }
    }
    input.breaklines.push_back({{Point3(0, 1, 0), Point3(4, 1, 0)}, false});
    auto built = katana::terrain::buildTin(input);
    ASSERT_TRUE(built.ok()) << built.error().describe();
    const auto smoothed =
        katana::terrain::smoothSurface(built->surface, SmoothOptions{3, 0.5});
    ASSERT_TRUE(smoothed.ok()) << smoothed.error().describe();
    for (int i = 0; i <= 4; ++i) {
        EXPECT_DOUBLE_EQ(*smoothed->surface.elevationAt(i, 1), 0.0) << "breakline vertex " << i;
    }
    EXPECT_LT(*smoothed->surface.elevationAt(2, 2), 8.0);
    EXPECT_GT(*smoothed->surface.elevationAt(2, 3), 0.0);
}

TEST(TinEdit, SmoothingOptionsOutOfRangeAreRefused)
{
    EXPECT_EQ(katana::terrain::smoothSurface(pyramid(), SmoothOptions{0, 0.5}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::smoothSurface(pyramid(), SmoothOptions{3, 0.0}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::smoothSurface(pyramid(), SmoothOptions{3, 1.5}).error().code,
              ErrorCode::InvalidArgument);
}

// ---- thinning ----------------------------------------------------------------------------------------

namespace {

// A 4 x 4 grid of unit cells on z = 0.5 x + 0.25 y (exact in binary), the
// vertex (2,2) raised by 1.
TinSurface bumpedPlane()
{
    return grid(4, 1.0, [](double x, double y) {
        return 0.5 * x + 0.25 * y + ((x == 2.0 && y == 2.0) ? 1.0 : 0.0);
    });
}

} // namespace

TEST(TinEdit, ThinningWithToleranceZeroRemovesOnlyThePointsThatChangeNothing)
{
    // The interior vertices are (1..3, 1..3). The bump (2,2) and its six
    // neighbours along the grid's edges - (1,1) (2,1) (1,2) (3,2) (2,3) (3,3)
    // - sit where the surface bends; the other two, (1,3) and (3,1), have
    // every neighbour on the plane and are the only ones that can go.
    const TinSurface before = bumpedPlane();
    const auto thinned = katana::terrain::thinSurface(before, 0.0);
    ASSERT_TRUE(thinned.ok()) << thinned.error().describe();
    EXPECT_EQ(thinned->report.pointsRemoved, 2u);
    EXPECT_EQ(thinned->surface.vertexCount(), 23u);
    EXPECT_EQ(vertexIndex(thinned->surface, 1, 3), -1);
    EXPECT_EQ(vertexIndex(thinned->surface, 3, 1), -1);
    EXPECT_NE(vertexIndex(thinned->surface, 2, 2), -1);
    for (double x = 0.1; x < 4.0; x += 0.2) {
        for (double y = 0.1; y < 4.0; y += 0.2) {
            EXPECT_NEAR(*thinned->surface.elevationAt(x, y), *before.elevationAt(x, y), 1e-9);
        }
    }
}

TEST(TinEdit, ThinningStaysWithinItsToleranceOfTheOriginalEverywhere)
{
    // A rolling surface: the bound is checked at a dense sample, not only at
    // the vertices removed.
    const TinSurface before = grid(12, 1.0, [](double x, double y) {
        return 0.3 * std::sin(0.7 * x) + 0.2 * std::cos(0.5 * y) + 0.01 * x * y;
    });
    const double tolerance = 0.1;
    const auto thinned = katana::terrain::thinSurface(before, tolerance);
    ASSERT_TRUE(thinned.ok()) << thinned.error().describe();
    EXPECT_GT(thinned->report.pointsRemoved, 0u);
    EXPECT_LE(thinned->report.largestChange, tolerance + 1e-7);
    for (double x = 0.05; x < 12.0; x += 0.1) {
        for (double y = 0.05; y < 12.0; y += 0.1) {
            ASSERT_LE(std::abs(*thinned->surface.elevationAt(x, y) - *before.elevationAt(x, y)),
                      tolerance + 1e-7)
                << "at " << x << ", " << y;
        }
    }
    EXPECT_NEAR(thinned->surface.planArea(), before.planArea(), 1e-9);
}

TEST(TinEdit, ThinningGivesTheSameSurfaceAtAnyThreadCount)
{
    const TinSurface before = grid(10, 1.0, [](double x, double y) {
        return 0.2 * std::sin(x) + 0.1 * std::cos(1.3 * y);
    });
    katana::core::TaskPool inline0(0);
    katana::core::TaskPool four(3);
    const auto one = katana::terrain::thinSurface(before, 0.02, &inline0);
    const auto many = katana::terrain::thinSurface(before, 0.02, &four);
    ASSERT_TRUE(one.ok() && many.ok());
    EXPECT_EQ(one->surface, many->surface);
}

TEST(TinEdit, ThinningRefusesANegativeOrNonFiniteToleranceAndAnEmptySurface)
{
    EXPECT_EQ(katana::terrain::thinSurface(pyramid(), -0.1).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::thinSurface(pyramid(), NAN).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::thinSurface(TinSurface{}, 0.1).error().code,
              ErrorCode::InvalidArgument);
}

TEST(TinEdit, AnEditLeavesTheSurfaceItWasGivenAsItWas)
{
    const TinSurface before = pyramid();
    const TinSurface copy = before;
    (void)katana::terrain::addPoints(before, std::vector<Point3>{Point3(2, 2, 1)});
    (void)katana::terrain::cutHole(before, rectangle(4, 4, 6, 6));
    (void)katana::terrain::smoothSurface(before, SmoothOptions{});
    EXPECT_EQ(before, copy);
}
