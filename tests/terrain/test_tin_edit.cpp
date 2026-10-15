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
using katana::terrain::BreaklineInput;
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

// ---- junctions within rounding of an existing edge ------------------------------------------
//
// A point decimal arithmetic puts on a line is a few ulps off it. The kernel
// decides only exactly, so such a point does not split the edge it is "on",
// and a new edge that ends there crosses the old one a few ulps from its own
// end - where CGAL moves a crossing it cannot place to the end of the edge it
// crosses, which can be tens of metres away. Coincidence is
// tolerance::kGeometric (1e-7): the old edge is bent through the point.

namespace {

// A flat square [0,100]^2 at 0 with a 3D breakline from (0,0,30) to (100,30,30):
// the line y = 0.3 x, one long edge of the surface.
TinSurface squareWithAnOldBreakline()
{
    const TinSurface square = make({Point3(0, 0, 0), Point3(100, 0, 0), Point3(100, 100, 0),
                                    Point3(0, 100, 0)},
                                   {{0, 1, 2}, {0, 2, 3}});
    const std::vector<BreaklineVertex> line{{Point2(0, 0), 30.0}, {Point2(100, 30), 30.0}};
    const auto added = katana::terrain::addBreakline(square, line, false);
    EXPECT_TRUE(added.ok()) << (added.ok() ? "" : added.error().describe());
    return added.ok() ? added->surface : TinSurface{};
}

// How many distinct constrained edges end at vertex `v`.
int constrainedDegree(const TinSurface& surface, int v)
{
    std::set<std::pair<std::uint32_t, std::uint32_t>> edges;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        for (std::size_t k = 0; k < 3; ++k) {
            const std::uint32_t a = surface.triangles()[t][k];
            const std::uint32_t b = surface.triangles()[t][(k + 1) % 3];
            if (surface.isEdgeConstrained(t, k) && (static_cast<int>(a) == v || static_cast<int>(b) == v)) {
                edges.insert({std::min(a, b), std::max(a, b)});
            }
        }
    }
    return static_cast<int>(edges.size());
}

double smallestTriangleArea(const TinSurface& surface)
{
    double smallest = 1e300;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        smallest = std::min(smallest, surface.planTriangle(t).signedArea());
    }
    return smallest;
}

} // namespace

TEST(TinEdit, ABreaklineEndingWithinRoundingOfAnOldBreaklineSplitsItAndKeepsItsLevelOnEitherSide)
{
    // (14.3, 4.29) is 1.8e-14 below the line y = 0.3 x (the cross product with
    // its direction, worked in exact rationals: -1.78e-14 / 104.4); (30.7, 9.21)
    // is 1.1e-13 / 104.4 above it; (10, 3) is exactly on it. All three are "on"
    // the old breakline to within kGeometric, so each is a T-junction: the old
    // breakline runs (0,0) - end - (100,30) through it, the new one ends there.
    const TinSurface old = squareWithAnOldBreakline();
    for (const Point2 end : {Point2(14.3, 4.29), Point2(30.7, 9.21), Point2(10.0, 3.0)}) {
        SCOPED_TRACE(testing::Message() << "end at " << end.x << ", " << end.y);
        const std::vector<BreaklineVertex> line{{Point2(40, 70), 35.0}, {end, 35.0}};
        const auto added = katana::terrain::addBreakline(old, line, false);
        ASSERT_TRUE(added.ok()) << added.error().describe();
        const TinSurface& after = added->surface;
        const int v = vertexIndex(after, end.x, end.y);
        ASSERT_GE(v, 0) << "the end is a vertex, where it was typed";
        // The two halves of the old breakline and the new line meet there.
        EXPECT_EQ(constrainedDegree(after, v), 3);
        EXPECT_EQ(added->report.crossings, 0u) << "a junction, not a crossing";
        EXPECT_NEAR(after.vertices()[static_cast<std::size_t>(v)].z, 35.0, 1e-12);
        // The new line is level at 35 from end to end (no ramp to the old level).
        for (const double s : {0.25, 0.5, 0.75, 0.95}) {
            EXPECT_NEAR(*after.elevationAt(40 + s * (end.x - 40), 70 + s * (end.y - 70)), 35.0, 1e-6);
        }
        // The old breakline bent to 35 at the junction, as it does for an exact
        // one: halfway along either half it is half way between 30 and 35.
        EXPECT_NEAR(*after.elevationAt(end.x / 2, end.y / 2), 32.5, 1e-6);
        EXPECT_NEAR(*after.elevationAt((end.x + 100) / 2, (end.y + 30) / 2), 32.5, 1e-6);
        // No sliver of no width beside the old line.
        EXPECT_GT(smallestTriangleArea(after), 1e-3);
        // Its length is kept: the old line is 104.4 m, the new one runs from (40,70).
        EXPECT_NEAR(constrainedLength(after),
                    std::hypot(100.0, 30.0) + std::hypot(40.0 - end.x, 70.0 - end.y), 1e-6);
    }
}

TEST(TinEdit, ABreaklineEndingClearOfAnOldBreaklineStillCrossesIt)
{
    // The same line ending 1e-3 below the old one (a thousand times
    // kGeometric): a real crossing, at the old line's level 30. The new line
    // from (40,70) to (14.3, 4.289) meets y = 0.3 x where
    // 70 - 65.711 t = 12 - 7.71 t, t = 58 / 58.001.
    const TinSurface old = squareWithAnOldBreakline();
    const std::vector<BreaklineVertex> line{{Point2(40, 70), 35.0}, {Point2(14.3, 4.289), 35.0}};
    const auto added = katana::terrain::addBreakline(old, line, false);
    ASSERT_TRUE(added.ok()) << added.error().describe();
    EXPECT_EQ(added->report.crossings, 1u);
    const double t = 58.0 / 58.001;
    EXPECT_NEAR(*added->surface.elevationAt(40 + t * (14.3 - 40), 70 + t * (4.289 - 70)), 30.0, 1e-6);
    const int end = vertexIndex(added->surface, 14.3, 4.289);
    ASSERT_GE(end, 0);
    EXPECT_EQ(constrainedDegree(added->surface, end), 1) << "the old line does not pass through it";
}

TEST(TinEdit, ADecimalPolygonWithAnEdgeAlongABreaklineIsCutExactly)
{
    // A parallelogram A B C D with A, B on y = 0.3 x in decimal arithmetic
    // (14.3, 4.29 and 33.9, 10.17) and C, D three to the left and ten up. Its
    // area is |AB x AD| = 19.6 * 10 + 5.88 * 3 = 213.64; its edge AB lies along
    // the old breakline. Whether the cut was refused ("touches or crosses
    // itself", at the breakline's far end) depended on the last bit of A and B.
    const TinSurface old = squareWithAnOldBreakline();
    Polyline2 polygon;
    polygon.vertices = {Point2(14.3, 4.29), Point2(33.9, 10.17), Point2(30.9, 20.17),
                        Point2(11.3, 14.29)};
    const double ab = std::hypot(33.9 - 14.3, 10.17 - 4.29);
    const double bc = std::hypot(3.0, 10.0);
    const double perimeter = 2.0 * (ab + bc);

    const auto clipped = katana::terrain::clipToBoundary(old, polygon);
    ASSERT_TRUE(clipped.ok()) << clipped.error().describe();
    EXPECT_NEAR(clipped->surface.planArea(), 213.64, 1e-6);
    // Its rim, with the breakline along AB being that edge (counted once).
    EXPECT_NEAR(constrainedLength(clipped->surface), perimeter, 1e-6);

    const auto holed = katana::terrain::cutHole(old, polygon);
    ASSERT_TRUE(holed.ok()) << holed.error().describe();
    EXPECT_NEAR(holed->surface.planArea(), 10000.0 - 213.64, 1e-6);
    // The rim of the hole, and the rest of the breakline either side of AB.
    EXPECT_NEAR(constrainedLength(holed->surface), perimeter + std::hypot(100.0, 30.0) - ab, 1e-6);
}

TEST(TinEdit, APadWithAnEdgeAlongABreaklineIsPastedWithACleanSeam)
{
    const TinSurface old = squareWithAnOldBreakline();
    const TinSurface pad = make({Point3(14.3, 4.29, 20), Point3(33.9, 10.17, 20),
                                 Point3(30.9, 20.17, 20), Point3(11.3, 14.29, 20)},
                                {{0, 1, 2}, {0, 2, 3}});
    const auto pasted = katana::terrain::pasteSurface(old, pad);
    ASSERT_TRUE(pasted.ok()) << pasted.error().describe();
    EXPECT_NEAR(pasted->surface.planArea(), 10000.0, 1e-6) << "no gap and no overlap";
    EXPECT_NEAR(*pasted->surface.elevationAt(24, 12), 20.0, 1e-9) << "inside the pad";
}

TEST(TinEdit, APointWithinRoundingOfAnOldBreaklineSplitsItAndKeepsItsOwnLevel)
{
    // ADD POINT at (14.3, 4.29, 35) on the old breakline of the finding above.
    const TinSurface old = squareWithAnOldBreakline();
    const auto added = katana::terrain::addPoints(old, std::vector<Point3>{Point3(14.3, 4.29, 35)});
    ASSERT_TRUE(added.ok()) << added.error().describe();
    const int v = vertexIndex(added->surface, 14.3, 4.29);
    ASSERT_GE(v, 0);
    EXPECT_EQ(constrainedDegree(added->surface, v), 2) << "the breakline passes through it";
    EXPECT_NEAR(added->surface.vertices()[static_cast<std::size_t>(v)].z, 35.0, 1e-12);
    EXPECT_NEAR(constrainedLength(added->surface), std::hypot(100.0, 30.0), 1e-6);
}

// ---- a 3D breakline is straight in 3D through the vertices it passes -------------------------

TEST(TinEdit, ABreaklineThroughExistingVerticesTakesTheirLevelsFromTheLine)
{
    // A flat 10 x 10 grid of unit cells at 0 and the line (0.5,0.5,5) -
    // (9.5,9.5,14) along the rising diagonals y = x: it passes through the
    // vertices (1,1) ... (9,9) exactly. It rises 9 over 9, so (k,k) is at
    // 5 + (k - 0.5) = 4.5 + k, and half way between two (2.5) at 7. These
    // vertices kept 0 while only the two ends moved.
    const TinSurface before = flatGrid(10, 1.0);
    const std::vector<BreaklineVertex> line{{Point2(0.5, 0.5), 5.0}, {Point2(9.5, 9.5), 14.0}};
    const auto added = katana::terrain::addBreakline(before, line, false);
    ASSERT_TRUE(added.ok()) << added.error().describe();
    for (int k = 1; k <= 9; ++k) {
        EXPECT_NEAR(*added->surface.elevationAt(k, k), 4.5 + k, 1e-9) << "vertex " << k;
    }
    EXPECT_NEAR(*added->surface.elevationAt(0.5, 0.5), 5.0, 1e-9);
    EXPECT_NEAR(*added->surface.elevationAt(9.5, 9.5), 14.0, 1e-9);
    EXPECT_NEAR(*added->surface.elevationAt(2.5, 2.5), 7.0, 1e-9);
    // The nine vertices are the surface's, relevelled; the two ends are new.
    EXPECT_EQ(added->report.pointsRelevelled, 9u);
    EXPECT_EQ(added->report.pointsAdded, 2u);
    EXPECT_EQ(added->report.crossings, 0u);
}

TEST(TinEdit, AVertexOnAnOldBreaklineKeepsItsLevelWhenANew3DLinePassesThrough)
{
    // The row y = 5 made a breakline at 3 (its vertices (1,5) ... (9,5) are on
    // it and at 3). A second line along it at 8: the old breakline's levels
    // hold (constrained edges are kept, the new line bends there); only its two
    // ends, typed on the old line, keep their own level, 8.
    const TinSurface before = flatGrid(10, 1.0);
    const std::vector<BreaklineVertex> first{{Point2(0, 5), 3.0}, {Point2(10, 5), 3.0}};
    const auto one = katana::terrain::addBreakline(before, first, false);
    ASSERT_TRUE(one.ok()) << one.error().describe();
    for (int k = 0; k <= 10; ++k) {
        EXPECT_NEAR(*one->surface.elevationAt(k, 5), 3.0, 1e-9) << "the first line, vertex " << k;
    }
    const std::vector<BreaklineVertex> second{{Point2(0.5, 5), 8.0}, {Point2(9.5, 5), 8.0}};
    const auto two = katana::terrain::addBreakline(one->surface, second, false);
    ASSERT_TRUE(two.ok()) << two.error().describe();
    for (int k = 1; k <= 9; ++k) {
        EXPECT_NEAR(*two->surface.elevationAt(k, 5), 3.0, 1e-9) << "vertex " << k;
    }
    EXPECT_NEAR(*two->surface.elevationAt(0.5, 5), 8.0, 1e-9);
    EXPECT_NEAR(*two->surface.elevationAt(9.5, 5), 8.0, 1e-9);
    EXPECT_EQ(two->report.pointsRelevelled, 0u);
}

TEST(TinEdit, TwoNew3DLinesThroughOneVertexMustAgreeThere)
{
    // (5,5) is a vertex of the grid on the diagonal y = x and on the other
    // diagonal. The first line gives it 5 + 4.5 = 9.5; the second, from
    // (0.5,9.5) to (9.5,0.5), 9.5 at its middle if it runs 5 to 14, and 4.5 if
    // it runs 0 to 9.
    const TinSurface before = flatGrid(10, 1.0);
    const BreaklineInput first{{{Point2(0.5, 0.5), 5.0}, {Point2(9.5, 9.5), 14.0}}, false};
    const BreaklineInput agrees{{{Point2(0.5, 9.5), 5.0}, {Point2(9.5, 0.5), 14.0}}, false};
    const BreaklineInput clashes{{{Point2(0.5, 9.5), 0.0}, {Point2(9.5, 0.5), 9.0}}, false};

    const std::vector<BreaklineInput> ok{first, agrees};
    const auto accepted = katana::terrain::addBreaklines(before, ok);
    ASSERT_TRUE(accepted.ok()) << accepted.error().describe();
    EXPECT_NEAR(*accepted->surface.elevationAt(5, 5), 9.5, 1e-9);

    const std::vector<BreaklineInput> bad{first, clashes};
    const auto refused = katana::terrain::addBreaklines(before, bad);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(refused.error().message.find("different levels"), std::string::npos);
    const std::vector<BreaklineInput> reversed{clashes, first};
    EXPECT_EQ(katana::terrain::addBreaklines(before, reversed).error().code,
              ErrorCode::InvalidGeometry);
}

// ---- several breaklines in one edit ----------------------------------------------------------

TEST(TinEdit, TwoNewBreaklinesCrossingAtDifferentLevelsAreRefusedWhicheverIsNamedFirst)
{
    // The square at 10; (1,5,12)-(9,5,12) and (5,1,8)-(5,9,8) cross at (5,5),
    // 12 on the one and 8 on the other. Added one after the other the first
    // would have won and the second bent to it, and which was first was an
    // accident of the order they were named in.
    const TinSurface ground = square(10, 10, 10, 10);
    const BreaklineInput along{{{Point2(1, 5), 12.0}, {Point2(9, 5), 12.0}}, false};
    const BreaklineInput across{{{Point2(5, 1), 8.0}, {Point2(5, 9), 8.0}}, false};
    for (const auto& lines : {std::vector<BreaklineInput>{along, across},
                              std::vector<BreaklineInput>{across, along}}) {
        const auto refused = katana::terrain::addBreaklines(ground, lines);
        ASSERT_FALSE(refused.ok());
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
        EXPECT_NE(refused.error().message.find("cross at two different levels"), std::string::npos)
            << refused.error().message;
    }
}

TEST(TinEdit, TwoNewBreaklinesThatAgreeWhereTheyCrossAreAcceptedAndADrapedOneMeetsThem)
{
    const TinSurface ground = square(10, 10, 10, 10);
    const BreaklineInput along{{{Point2(1, 5), 12.0}, {Point2(9, 5), 12.0}}, false};
    const BreaklineInput level{{{Point2(5, 1), 12.0}, {Point2(5, 9), 12.0}}, false};
    const std::vector<BreaklineInput> both{along, level};
    const auto agreed = katana::terrain::addBreaklines(ground, both);
    ASSERT_TRUE(agreed.ok()) << agreed.error().describe();
    EXPECT_EQ(agreed->report.crossings, 1u);
    EXPECT_NEAR(*agreed->surface.elevationAt(5, 5), 12.0, 1e-9);

    // A draped line across the 3D one meets it at its level, 12, and is the
    // ground's 10 at its own ends.
    const BreaklineInput draped{{{Point2(5, 1), std::nullopt}, {Point2(5, 9), std::nullopt}}, false};
    const std::vector<BreaklineInput> mixed{along, draped};
    const auto met = katana::terrain::addBreaklines(ground, mixed);
    ASSERT_TRUE(met.ok()) << met.error().describe();
    EXPECT_NEAR(*met->surface.elevationAt(5, 5), 12.0, 1e-9);
    EXPECT_NEAR(*met->surface.elevationAt(5, 1), 10.0, 1e-9);
}

TEST(TinEdit, AddingNoBreaklineOrOneWithTooFewVerticesIsRefused)
{
    const TinSurface ground = square(10, 10, 10, 10);
    EXPECT_EQ(katana::terrain::addBreaklines(ground, std::vector<BreaklineInput>{}).error().code,
              ErrorCode::InvalidArgument);
    const BreaklineInput one{{{Point2(5, 5), 1.0}}, false};
    const BreaklineInput fine{{{Point2(1, 5), 12.0}, {Point2(9, 5), 12.0}}, false};
    const std::vector<BreaklineInput> lines{fine, one};
    EXPECT_EQ(katana::terrain::addBreaklines(ground, lines).error().code,
              ErrorCode::InvalidArgument);
}

TEST(TinEdit, ABreaklineThatReturnsToAPositionTakesTheLevelGivenOnEitherVisit)
{
    // On a square at 10: a closed loop whose last vertex returns to (2,2) and
    // gives it 5, or whose first does. Either way (2,2) is at 5; the segments
    // arriving at and leaving it are straight 3D to the ground's 10 at their
    // other ends, so half way along each (2,5) and (5,2) is 7.5.
    const TinSurface ground = square(10, 10, 10, 10);
    const std::vector<BreaklineVertex> later{{Point2(2, 2), std::nullopt}, {Point2(8, 2), std::nullopt},
                                             {Point2(8, 8), std::nullopt}, {Point2(2, 8), std::nullopt},
                                             {Point2(2, 2), 5.0}};
    std::vector<BreaklineVertex> earlier = later;
    earlier.front().z = 5.0;
    earlier.back().z = std::nullopt;
    for (const std::vector<BreaklineVertex>* line :
         std::initializer_list<const std::vector<BreaklineVertex>*>{&later, &earlier}) {
        const auto added = katana::terrain::addBreakline(ground, *line, false);
        ASSERT_TRUE(added.ok()) << added.error().describe();
        EXPECT_NEAR(*added->surface.elevationAt(2, 2), 5.0, 1e-9);
        EXPECT_NEAR(*added->surface.elevationAt(2, 5), 7.5, 1e-9);
        EXPECT_NEAR(*added->surface.elevationAt(5, 2), 7.5, 1e-9);
    }
    // Two levels that differ are refused, as they always were.
    std::vector<BreaklineVertex> clash = later;
    clash.front().z = 6.0;
    const auto refused = katana::terrain::addBreakline(ground, clash, false);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
}

// ---- on the surface means what TinSurface::locate says ---------------------------------------

TEST(TinEdit, APositionWithinToleranceBeyondTheRimIsOnTheSurfaceNotOffIt)
{
    // The square [0,10]^2 at 10. (-1e-12, 5) is 1e-12 beyond the rim edge x = 0:
    // elevationAt says it is on the surface (within kGeometric of an edge).
    const TinSurface flat = square(10, 10, 10, 10);
    const double beyond = -1e-12;
    ASSERT_TRUE(flat.elevationAt(beyond, 5).has_value());

    // A breakline from it is not "off the surface".
    const std::vector<BreaklineVertex> line{{Point2(beyond, 5), std::nullopt},
                                            {Point2(5, 5), std::nullopt}};
    EXPECT_TRUE(katana::terrain::addBreakline(flat, line, false).ok());

    // An added point there is a vertex of the rim, not a fin: the same three
    // triangles as the point exactly on the rim, whose area is worked out here
    // - (0,5,12) with the corners at 10 gives triangles (P,0,1) and (P,2,3) of
    // |(0,-20,50)| / 2 each and (P,1,2) of |(20,0,100)| / 2: sqrt(2900) +
    // sqrt(10400) / 2 = 104.8418 (a wall of the fin's 10 m2 would read 110).
    const auto on = katana::terrain::addPoints(flat, std::vector<Point3>{Point3(0, 5, 12)});
    const auto near = katana::terrain::addPoints(flat, std::vector<Point3>{Point3(beyond, 5, 12)});
    ASSERT_TRUE(on.ok() && near.ok());
    EXPECT_EQ(on->surface.triangleCount(), 3u);
    EXPECT_EQ(near->surface.triangleCount(), 3u);
    EXPECT_EQ(near->report.pointsAdded, 1u);
    const double expected = std::sqrt(2900.0) + std::sqrt(10400.0) / 2.0;
    EXPECT_NEAR(on->surface.surfaceArea(), expected, 1e-6);
    EXPECT_NEAR(near->surface.surfaceArea(), expected, 1e-6);
    EXPECT_NEAR(near->surface.planArea(), 100.0, 1e-9);
    EXPECT_NEAR(*near->surface.elevationAt(0, 5), 12.0, 1e-9) << "the rim there is the point's level";
}

TEST(TinEdit, TwoPointsOnTheRimAtOnePositionWithDifferentLevelsAreRefused)
{
    // (10,5) and a point 5e-8 beyond the rim are one position by kGeometric:
    // 3 and 7 differ by more than kCoordinate, as inside the square.
    const TinSurface flat = square(10, 10, 10, 10);
    const auto onTheRim = katana::terrain::addPoints(
        flat, std::vector<Point3>{Point3(10, 5, 3), Point3(10.00000005, 5, 7)});
    ASSERT_FALSE(onTheRim.ok());
    EXPECT_EQ(onTheRim.error().code, ErrorCode::InvalidGeometry);
    const auto inside = katana::terrain::addPoints(
        flat, std::vector<Point3>{Point3(5, 3, 3), Point3(5.00000005, 3, 7)});
    ASSERT_FALSE(inside.ok());
    EXPECT_EQ(inside.error().code, ErrorCode::InvalidGeometry);
}

TEST(TinEdit, APositionAThousandTimesToleranceBeyondTheRimIsStillOffTheSurface)
{
    const TinSurface flat = square(10, 10, 10, 10);
    const std::vector<BreaklineVertex> line{{Point2(-1e-4, 5), std::nullopt},
                                            {Point2(5, 5), std::nullopt}};
    const auto refused = katana::terrain::addBreakline(flat, line, false);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(refused.error().message.find("off the surface"), std::string::npos);
    // A point there extends the surface by the triangle joining it to the rim.
    const auto extended = katana::terrain::addPoints(flat, std::vector<Point3>{Point3(-1e-4, 5, 12)});
    ASSERT_TRUE(extended.ok());
    EXPECT_EQ(extended->surface.triangleCount(), 3u);
    EXPECT_NEAR(extended->surface.planArea(), 100.0 + 0.5 * 1e-4 * 10.0, 1e-9);
}

// ---- a polygon that touches itself --------------------------------------------------------------

TEST(TinEdit, APolygonThatVisitsACornerTwiceIsRefusedWhicheverWayItsLobesRun)
{
    const TinSurface ground = flatGrid(10, 1.0);
    // Two triangles joined at (5,5), both counter-clockwise: the total signed
    // area is 8 + 8 and the crossing check, which looks at the interior vertices
    // of an edge, never saw the corner.
    Polyline2 sameWay;
    sameWay.vertices = {Point2(5, 5), Point2(1, 1), Point2(5, 1), Point2(5, 5), Point2(9, 9),
                        Point2(5, 9)};
    // A figure of eight crossing at the vertex (4,4): lobe (2,2) (6,2) (4,4) is
    // counter-clockwise, area 4; lobe (4,4) (3.5,5) (4.5,5) clockwise, area
    // -0.5. The two lobes run opposite ways, so the inside-on-the-left rule
    // that decides which side of a cut edge is kept turned the second lobe
    // inside out: ground outside the polygon was kept and ground inside lost.
    Polyline2 figureOfEight;
    figureOfEight.vertices = {Point2(2, 2), Point2(6, 2), Point2(4, 4), Point2(3.5, 5),
                              Point2(4.5, 5), Point2(4, 4)};
    for (const Polyline2* polygon : {&sameWay, &figureOfEight}) {
        const auto clipped = katana::terrain::clipToBoundary(ground, *polygon);
        ASSERT_FALSE(clipped.ok());
        EXPECT_EQ(clipped.error().code, ErrorCode::InvalidGeometry);
        EXPECT_NE(clipped.error().message.find("touches or crosses itself"), std::string::npos)
            << clipped.error().message;
        const auto holed = katana::terrain::cutHole(ground, *polygon);
        ASSERT_FALSE(holed.ok());
        EXPECT_EQ(holed.error().code, ErrorCode::InvalidGeometry);
    }
}

TEST(TinEdit, APolygonWithACornerOnAnotherEdgeIsRefusedAndASimpleOneIsNot)
{
    const TinSurface ground = flatGrid(10, 1.0);
    // (5,2) lies on the edge (2,2)-(8,2): a touch, not a crossing.
    Polyline2 touching;
    touching.vertices = {Point2(2, 2), Point2(8, 2), Point2(8, 6), Point2(5, 2), Point2(2, 6)};
    EXPECT_EQ(katana::terrain::cutHole(ground, touching).error().code, ErrorCode::InvalidGeometry);
    // The same ring with (5,3) instead of (5,2) is a simple concave polygon.
    // Shoelace, x_i y_{i+1} - x_{i+1} y_i over (2,2) (8,2) (8,6) (5,3) (2,6):
    // 2*2-8*2 = -12; 8*6-8*2 = 32; 8*3-5*6 = -6; 5*6-2*3 = 24; 2*2-2*6 = -8;
    // the sum is 30, the area 15.
    Polyline2 simple;
    simple.vertices = {Point2(2, 2), Point2(8, 2), Point2(8, 6), Point2(5, 3), Point2(2, 6)};
    const auto holed = katana::terrain::cutHole(ground, simple);
    ASSERT_TRUE(holed.ok()) << holed.error().describe();
    EXPECT_NEAR(holed->surface.planArea(), 100.0 - 15.0, 1e-9);
}

TEST(TinEdit, ABowTieWhoseLobesCancelIsRefusedAsTouchingItselfNotAsHavingNoArea)
{
    const TinSurface ground = flatGrid(10, 1.0);
    Polyline2 bowTie; // (2,2) (8,8) (8,2) (2,8): two triangles of equal and opposite area
    bowTie.vertices = {Point2(2, 2), Point2(8, 8), Point2(8, 2), Point2(2, 8)};
    const auto refused = katana::terrain::cutHole(ground, bowTie);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(refused.error().message.find("touches or crosses itself"), std::string::npos)
        << refused.error().message;
    Polyline2 flat; // three collinear points: no area, and says so
    flat.vertices = {Point2(1, 1), Point2(5, 1), Point2(9, 1)};
    const auto none = katana::terrain::cutHole(ground, flat);
    ASSERT_FALSE(none.ok());
    EXPECT_NE(none.error().message.find("no area"), std::string::npos) << none.error().message;
}

// ---- a point off the surface joins the pockets it shuts in -----------------------------------

TEST(TinEdit, APointInASquareHoleFillsTheWholeHole)
{
    // A 3 x 3 grid of points 10 m apart (area 400) with [5,15]^2 cut out:
    // 300. A point at its centre is joined to the hole's whole rim: the four
    // fans cover the square, 100, and the area is back to 400. Cocircular rim
    // vertices let the kernel choose a diagonal across a corner of the hole
    // instead, and the triangle of three rim vertices that makes was dropped
    // as not having the point as a corner - leaving two ears of 12.5.
    const TinSurface ground = grid(2, 10.0, [](double x, double y) { return x + y; });
    const auto holed = katana::terrain::cutHole(ground, rectangle(5, 5, 15, 15));
    ASSERT_TRUE(holed.ok()) << holed.error().describe();
    EXPECT_NEAR(holed->surface.planArea(), 300.0, 1e-9);
    const auto filled = katana::terrain::addPoints(holed->surface, std::vector<Point3>{Point3(10, 10, 20)});
    ASSERT_TRUE(filled.ok()) << filled.error().describe();
    EXPECT_NEAR(filled->surface.planArea(), 400.0, 1e-9);
    for (const Point2 spot : {Point2(6, 6), Point2(14, 14), Point2(14, 6), Point2(6, 14), Point2(10, 7)}) {
        EXPECT_TRUE(filled->surface.elevationAt(spot).has_value())
            << "no ground at " << spot.x << ", " << spot.y;
    }
    EXPECT_NEAR(*filled->surface.elevationAt(10, 10), 20.0, 1e-9);
    // Off centre: the hole is just as full.
    const auto aside = katana::terrain::addPoints(holed->surface, std::vector<Point3>{Point3(10.5, 10.5, 20)});
    ASSERT_TRUE(aside.ok());
    EXPECT_NEAR(aside->surface.planArea(), 400.0, 1e-9);
}

TEST(TinEdit, APointBeyondANotchLeavesNoEnclosedPocket)
{
    // The 3 x 3 grid less its corner cell [10,20]^2: an L of area 300. A point
    // at (25,25) is joined to the rim edges it sees: (20,0)-(20,10) and
    // (0,20)-(10,20), 25 each, and over the notch's mouth (20,10)-(25,25)-
    // (10,20), 100 - and the triangle (10,10) (20,10) (10,20) of the notch
    // behind them, 50, is shut in. 300 + 25 + 25 + 100 + 50 = 500.
    const TinSurface ground = grid(2, 10.0, [](double, double) { return 0.0; });
    const auto cells = katana::terrain::trianglesByPolygon(ground, rectangle(10, 10, 20, 20), true);
    ASSERT_EQ(cells.size(), 2u);
    const auto notched = katana::terrain::deleteTriangles(ground, cells);
    ASSERT_TRUE(notched.ok()) << notched.error().describe();
    EXPECT_NEAR(notched->surface.planArea(), 300.0, 1e-9);
    const auto extended =
        katana::terrain::addPoints(notched->surface, std::vector<Point3>{Point3(25, 25, 50)});
    ASSERT_TRUE(extended.ok()) << extended.error().describe();
    EXPECT_NEAR(extended->surface.planArea(), 500.0, 1e-9);
    EXPECT_TRUE(extended->surface.elevationAt(13, 13).has_value()) << "the pocket behind the bridge";
    EXPECT_TRUE(extended->surface.elevationAt(18, 18).has_value());
}

TEST(TinEdit, ConcavitiesThePointDoesNotShutInAreLeftAsTheyWere)
{
    const TinSurface ground = grid(2, 10.0, [](double, double) { return 0.0; });
    // A hole with a point elsewhere stays a hole: the point at (30,10) joins
    // the outer rim only, 0.5 x 20 x 10 = 100 more.
    const auto holed = katana::terrain::cutHole(ground, rectangle(5, 5, 15, 15));
    ASSERT_TRUE(holed.ok());
    const auto aside =
        katana::terrain::addPoints(holed->surface, std::vector<Point3>{Point3(30, 10, 0)});
    ASSERT_TRUE(aside.ok()) << aside.error().describe();
    EXPECT_NEAR(aside->surface.planArea(), 400.0, 1e-9);
    EXPECT_FALSE(aside->surface.elevationAt(10, 10).has_value());
    EXPECT_TRUE(aside->surface.elevationAt(25, 10).has_value());
    // A notch open to the outside, with a point on the far side of the L: the
    // notch is not shut in and stays out.
    const auto cells = katana::terrain::trianglesByPolygon(ground, rectangle(10, 10, 20, 20), true);
    const auto notched = katana::terrain::deleteTriangles(ground, cells);
    ASSERT_TRUE(notched.ok());
    const auto far = katana::terrain::addPoints(notched->surface, std::vector<Point3>{Point3(5, -5, 0)});
    ASSERT_TRUE(far.ok()) << far.error().describe();
    EXPECT_FALSE(far->surface.elevationAt(15, 15).has_value());
    EXPECT_FALSE(far->surface.elevationAt(13, 13).has_value());
}

// ---- smaller things ------------------------------------------------------------------------------

TEST(TinEdit, ThinningThatRemovesNothingReturnsTheSurfaceAsItWas)
{
    // The square's triangles written rotated: (1,2,0) and (3,0,2) are the
    // canonical (0,1,2) and (0,2,3) started at another corner. Every vertex is
    // on the rim and held, so nothing can go - and the surface must come back
    // as it was, not sorted into the canonical order.
    const TinSurface rotated = make({Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 0),
                                     Point3(0, 10, 0)},
                                    {{1, 2, 0}, {3, 0, 2}});
    const auto thinned = katana::terrain::thinSurface(rotated, 0.0);
    ASSERT_TRUE(thinned.ok()) << thinned.error().describe();
    EXPECT_EQ(thinned->report.pointsRemoved, 0u);
    EXPECT_EQ(thinned->surface, rotated);
}

TEST(TinEdit, SmoothingInsideAPolygonWithNoVertexSaysNoVertexLiesThereRatherThanAllAreHeld)
{
    const TinSurface surface = pyramid();
    // Inside one triangle, holding no vertex at all.
    const auto empty = katana::terrain::smoothSurface(surface, SmoothOptions{}, rectangle(1, 4, 2, 5));
    ASSERT_FALSE(empty.ok());
    EXPECT_EQ(empty.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(empty.error().message.find("no vertex of the surface lies inside the polygon"),
              std::string::npos) << empty.error().message;
    // Round the corner (0,0) only, which is on the rim and held.
    const auto held = katana::terrain::smoothSurface(surface, SmoothOptions{}, rectangle(-1, -1, 1, 1));
    ASSERT_FALSE(held.ok());
    EXPECT_NE(held.error().message.find("free to move"), std::string::npos) << held.error().message;
}

TEST(TinEdit, AVertexWithinReachIsFoundWhereverItIsRelativeToTheTriangleUnderThePoint)
{
    // Off the surface and in a thin triangle whose own corners are far.
    const TinSurface flat = flatGrid(2, 1.0);
    // The corner (0,0) is 0.07 from (-0.05,-0.05): found with a reach of 0.2,
    // not found without one (the point is off the surface), nor with 0.05.
    EXPECT_EQ(katana::terrain::vertexAt(flat, Point2(-0.05, -0.05), 0.2), 0u);
    EXPECT_FALSE(katana::terrain::vertexAt(flat, Point2(-0.05, -0.05)).has_value());
    EXPECT_FALSE(katana::terrain::vertexAt(flat, Point2(-0.05, -0.05), 0.05).has_value());
    EXPECT_FALSE(katana::terrain::vertexAt(flat, Point2(-5, -5), 0.2).has_value());

    // A thin triangle A(0,0) B(10,0) C(5,0.1) over D(9,-0.2): the point (9,0.01)
    // is inside ABC, whose corners are 1.0 (B), 9 and 4 away; D is 0.21 away.
    const TinSurface thin = make({Point3(0, 0, 0), Point3(10, 0, 0), Point3(5, 0.1, 0), Point3(9, -0.2, 0)},
                                 {{0, 1, 2}, {0, 3, 1}});
    EXPECT_EQ(katana::terrain::vertexAt(thin, Point2(9, 0.01), 0.5), 3u);
    EXPECT_EQ(katana::terrain::vertexAt(thin, Point2(9, 0.01)), 1u) << "no reach: the nearest corner";
    EXPECT_EQ(katana::terrain::vertexAt(thin, Point2(9, 0.01), 1.5), 1u) << "a corner within reach";
}

TEST(TinEdit, AVertexOnABreaklineOrBoundaryEdgeIsTheOneDeletePointRefuses)
{
    const TinSurface withLine = squareWithAnOldBreakline();
    // The old breakline's end (100,30) is on a constrained edge; the square's
    // corner (100,100) is on the rim only, which deletePoints does delete.
    const int onLine = vertexIndex(withLine, 100, 30);
    const int corner = vertexIndex(withLine, 100, 100);
    ASSERT_GE(onLine, 0);
    ASSERT_GE(corner, 0);
    EXPECT_TRUE(katana::terrain::onConstrainedEdge(withLine, static_cast<std::uint32_t>(onLine)));
    EXPECT_FALSE(katana::terrain::onConstrainedEdge(withLine, static_cast<std::uint32_t>(corner)));
    EXPECT_FALSE(katana::terrain::deletePoints(withLine, std::vector<std::uint32_t>{
                                                             static_cast<std::uint32_t>(onLine)})
                     .ok());
    EXPECT_TRUE(katana::terrain::deletePoints(withLine, std::vector<std::uint32_t>{
                                                            static_cast<std::uint32_t>(corner)})
                    .ok());
}

TEST(TinEdit, ANumberBeyondTheMostAnEditAcceptsIsRefusedWhereItIsMade)
{
    // 1e308 passed the finite-only check, and then the plan area read nan.
    const TinSurface flat = square(10, 10, 10, 10);
    const double most = katana::terrain::kMostMagnitude;
    EXPECT_EQ(katana::terrain::addPoints(flat, std::vector<Point3>{Point3(1e308, 1, 1)}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::addPoints(flat, std::vector<Point3>{Point3(5, 5, 1e308)}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::changeHeights(flat, HeightChange{HeightOperation::Raise, 1e308})
                  .error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::changeHeights(flat, HeightChange{HeightOperation::SetLevel, 1e308})
                  .error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::changeHeights(flat, HeightChange{HeightOperation::Scale, 1e308, 0.0})
                  .error().code,
              ErrorCode::InvalidArgument);
    const std::vector<BreaklineVertex> high{{Point2(1, 1), 1e308}, {Point2(9, 9), 1.0}};
    EXPECT_EQ(katana::terrain::addBreakline(flat, high, false).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::cutHole(flat, rectangle(1, 1, 1e308, 9)).error().code,
              ErrorCode::InvalidArgument);
    // The limit itself is accepted, and areas stay numbers.
    const auto at = katana::terrain::addPoints(flat, std::vector<Point3>{Point3(5, 5, most)});
    ASSERT_TRUE(at.ok()) << at.error().describe();
    EXPECT_TRUE(std::isfinite(at->surface.surfaceArea()));
    EXPECT_EQ(katana::terrain::addPoints(flat, std::vector<Point3>{Point3(5, 5, most * 1.0001)})
                  .error().code,
              ErrorCode::InvalidArgument);
}

// ---- the indexed polygon test answers what Polyline2::classify answers -----------------------

TEST(TinEdit, TheBandedPolygonTestGivesEveryAnswerThePlainOneGives)
{
    // A twelve-pointed star about (50,50) with radii 20 and 8, concave and
    // with edges at every slope, asked about a lattice of points 0.7 apart, its
    // corners, the middles of its edges and points 1e-8 and 1e-6 off them.
    Polyline2 star;
    star.closed = true;
    for (int i = 0; i < 24; ++i) {
        const double radius = i % 2 == 0 ? 20.0 : 8.0;
        const double angle = i * std::numbers::pi / 12.0;
        star.vertices.emplace_back(50 + radius * std::cos(angle), 50 + radius * std::sin(angle));
    }
    const katana::geometry::PolygonClassifier banded(star);
    std::vector<Point2> queries;
    for (double x = 20; x < 80; x += 0.7) {
        for (double y = 20; y < 80; y += 0.7) {
            queries.emplace_back(x, y);
        }
    }
    for (std::size_t i = 0; i < star.vertices.size(); ++i) {
        const Point2& a = star.vertices[i];
        const Point2& b = star.vertices[(i + 1) % star.vertices.size()];
        for (const double t : {0.0, 0.5}) {
            const Point2 on(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
            for (const double off : {0.0, 1e-8, -1e-8, 1e-6, -1e-6}) {
                queries.emplace_back(on.x + off, on.y);
                queries.emplace_back(on.x, on.y + off);
            }
        }
    }
    std::size_t inside = 0;
    std::size_t outside = 0;
    std::size_t boundary = 0;
    for (const Point2& q : queries) {
        const auto plain = star.classify(q);
        EXPECT_EQ(banded.classify(q), plain) << "at " << q.x << ", " << q.y;
        inside += plain == katana::geometry::Containment::Inside;
        outside += plain == katana::geometry::Containment::Outside;
        boundary += plain == katana::geometry::Containment::OnBoundary;
    }
    // The sweep reaches all three answers.
    EXPECT_GT(inside, 100u);
    EXPECT_GT(outside, 100u);
    EXPECT_GT(boundary, 20u);
    // Fewer than three vertices bound nothing, as an open or degenerate polyline does.
    Polyline2 two;
    two.vertices = {Point2(0, 0), Point2(1, 1)};
    EXPECT_EQ(katana::geometry::PolygonClassifier(two).classify(Point2(0.5, 0.5)),
              katana::geometry::Containment::Outside);
}

TEST(TinEdit, ABoundaryOrAHoleRoundACircleCostsExactlyTheCirclesArea)
{
    // The triangles outside the region are asked about a piece at a time. A
    // 64-gon of radius 7 about (10.3, 9.7) in a 20 x 20 grid: its area
    // (64 / 2) r^2 sin(2 pi / 64) is the boundary's whole result and the
    // hole's 400 less it.
    const TinSurface ground = flatGrid(20, 1.0);
    Polyline2 circle;
    circle.closed = true;
    for (int i = 0; i < 64; ++i) {
        const double angle = i * 2.0 * std::numbers::pi / 64.0;
        circle.vertices.emplace_back(10.3 + 7.0 * std::cos(angle), 9.7 + 7.0 * std::sin(angle));
    }
    const double area = 32.0 * 49.0 * std::sin(2.0 * std::numbers::pi / 64.0);
    const auto clipped = katana::terrain::clipToBoundary(ground, circle);
    ASSERT_TRUE(clipped.ok()) << clipped.error().describe();
    EXPECT_NEAR(clipped->surface.planArea(), area, 1e-9);
    const auto holed = katana::terrain::cutHole(ground, circle);
    ASSERT_TRUE(holed.ok()) << holed.error().describe();
    EXPECT_NEAR(holed->surface.planArea(), 400.0 - area, 1e-9);
}
