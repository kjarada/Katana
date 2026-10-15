// The solids road objects are made of (include/katana/cad/road/road_object_mesh.hpp).
//
// Every expected volume is worked out by hand from the shape: a box is its
// three sides multiplied, a prism its section's area times its length, a
// hollow sweep the ring between its loops times its length. A mitred corner
// keeps a section symmetric about its line at exactly its area times the
// length of that line: what the outside of the bend gains, the inside loses.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <numbers>
#include <vector>

#include "katana/cad/road/road_object_mesh.hpp"

using katana::cad::road::addBox;
using katana::cad::road::addExtrusion;
using katana::cad::road::addHollowSweep;
using katana::cad::road::addPrism;
using katana::cad::road::addSweep;
using katana::cad::road::counterClockwise;
using katana::cad::road::isClosedMesh;
using katana::geometry::meshVolume;
using katana::cad::road::PathPoint;
using katana::cad::road::regularPolygon;
using katana::cad::road::triangulatePolygon;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::TriangleMesh;
using katana::geometry::Vec2;
using katana::geometry::Vec3;

namespace {

double triangleArea(const std::vector<Point2>& polygon, const std::array<std::uint32_t, 3>& face)
{
    const Point2& a = polygon[face[0]];
    const Point2& b = polygon[face[1]];
    const Point2& c = polygon[face[2]];
    return (b - a).cross(c - a) / 2.0;
}

// Whether the mesh has a vertex at (x, y, z), to a picometre.
bool hasVertex(const TriangleMesh& mesh, double x, double y, double z)
{
    return std::ranges::any_of(mesh.vertices, [&](const Point3& v) {
        return std::abs(v.x - x) < 1e-12 && std::abs(v.y - y) < 1e-12 && std::abs(v.z - z) < 1e-12;
    });
}

// The point `length` along the heading `degrees` counter-clockwise from east,
// from `from`.
Point2 along(Point2 from, double degrees, double length)
{
    const double radians = degrees * std::numbers::pi / 180.0;
    return Point2(from.x + length * std::cos(radians), from.y + length * std::sin(radians));
}

} // namespace

TEST(RoadObjectMesh, ABoxIsClosedAndEnclosesItsThreeSidesMultiplied)
{
    TriangleMesh mesh;
    addBox(mesh, Point3(5.0, 5.0, 1.0), Vec2(1.0, 0.0), 2.0, 3.0, 4.0);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_EQ(mesh.triangleCount(), 12u);
    EXPECT_NEAR(meshVolume(mesh), 2.0 * 3.0 * 4.0, 1e-12);
    const auto box = mesh.bounds();
    EXPECT_NEAR(box.min.x, 4.0, 1e-12);
    EXPECT_NEAR(box.max.x, 6.0, 1e-12);
    EXPECT_NEAR(box.min.y, 3.5, 1e-12);
    EXPECT_NEAR(box.max.y, 6.5, 1e-12);
    EXPECT_NEAR(box.min.z, 1.0, 1e-12);
    EXPECT_NEAR(box.max.z, 5.0, 1e-12);
}

TEST(RoadObjectMesh, AVolumeFarFromTheOriginKeepsItsDigits)
{
    // A box of 24 m3 at map coordinates of seven million metres: summed
    // about the origin, each term would be ~1e20 and the answer noise.
    TriangleMesh mesh;
    addBox(mesh, Point3(500000.0, 7000000.0, 100.0), Vec2(0.6, 0.8), 2.0, 3.0, 4.0);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 24.0, 1e-6);
}

TEST(RoadObjectMesh, AConcavePolygonTriangulatesIntoItsAreaWithTwoFewerTrianglesThanCorners)
{
    // An inverted T - a retaining wall's stem on its footing: the footing
    // 1.2 x 0.4 and the stem 0.25 x 2.0 make 0.48 + 0.5 = 0.98.
    const std::vector<Point2> tee{{-0.6, -0.4},  {0.6, -0.4},  {0.6, 0.0},    {0.125, 0.0},
                                  {0.125, 2.0},  {-0.125, 2.0}, {-0.125, 0.0}, {-0.6, 0.0}};
    const auto faces = triangulatePolygon(tee);
    ASSERT_EQ(faces.size(), tee.size() - 2);
    double area = 0.0;
    for (const auto& face : faces) {
        EXPECT_GE(triangleArea(tee, face), 0.0) << "a triangle wound clockwise";
        area += triangleArea(tee, face);
    }
    EXPECT_NEAR(area, 0.98, 1e-12);
}

TEST(RoadObjectMesh, AClockwisePolygonIsTriangulatedCounterClockwise)
{
    const std::vector<Point2> clockwise{{0, 0}, {0, 1}, {1, 1}, {1, 0}};
    const auto faces = triangulatePolygon(clockwise);
    ASSERT_EQ(faces.size(), 2u);
    for (const auto& face : faces) {
        EXPECT_GT(triangleArea(clockwise, face), 0.0);
    }
    const auto turned = counterClockwise(clockwise);
    EXPECT_EQ(turned.front(), Point2(1, 0));
}

TEST(RoadObjectMesh, APolygonWithNoAreaGivesNoTriangles)
{
    // A bow-tie: its two halves cancel, and it is no outline to fill.
    EXPECT_TRUE(triangulatePolygon(std::vector<Point2>{{0, 0}, {1, 1}, {1, 0}, {0, 1}}).empty());
    EXPECT_TRUE(triangulatePolygon(std::vector<Point2>{{0, 0}, {1, 1}}).empty());
    EXPECT_TRUE(triangulatePolygon(std::vector<Point2>{}).empty());
}

TEST(RoadObjectMesh, AStraightRunInAPolygonStaysInTheTriangulation)
{
    // (1,0) lies on the bottom edge, a corner of no turn. The edges
    // (0,0)-(1,0) and (1,0)-(2,0) must both be in the triangulation for a
    // cap to meet the sides of a prism: five corners, three triangles, and
    // the prism closed.
    const std::vector<Point2> square{{0, 0}, {1, 0}, {2, 0}, {2, 2}, {0, 2}};
    const auto faces = triangulatePolygon(square);
    ASSERT_EQ(faces.size(), 3u);
    double area = 0.0;
    for (const auto& face : faces) {
        area += triangleArea(square, face);
    }
    EXPECT_NEAR(area, 4.0, 1e-12);
    TriangleMesh mesh;
    addPrism(mesh, square, 0.0, 1.0);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 4.0, 1e-12);
}

TEST(RoadObjectMesh, AnExtrusionFacesOutwardWhicheverWayItsPolygonRuns)
{
    TriangleMesh mesh;
    const std::vector<Point2> clockwise{{0, 0}, {0, 2}, {3, 2}, {3, 0}};
    addExtrusion(mesh, clockwise, Point3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 0, 1), 0.5);
    EXPECT_TRUE(isClosedMesh(mesh));
    // Outward: the volume is positive, 3 x 2 x 0.5.
    EXPECT_NEAR(meshVolume(mesh), 3.0, 1e-12);
    // Extruded along u x v = (1,0,0) x (0,0,1) = (0,-1,0).
    EXPECT_NEAR(mesh.bounds().min.y, -0.5, 1e-12);
    EXPECT_NEAR(mesh.bounds().max.y, 0.0, 1e-12);
}

TEST(RoadObjectMesh, ASweepAlongAStraightPathIsItsSectionTimesItsLength)
{
    TriangleMesh mesh;
    const std::vector<PathPoint> path{{Point2(0, 0), 10.0}, {Point2(4, 0), 10.0},
                                      {Point2(10, 0), 10.0}};
    const std::vector<Point2> section{{-0.25, 0.0}, {0.25, 0.0}, {0.25, 2.0}, {-0.25, 2.0}};
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 0.5 * 2.0 * 10.0, 1e-9);
    EXPECT_NEAR(mesh.bounds().min.z, 10.0, 1e-12);
    EXPECT_NEAR(mesh.bounds().max.z, 12.0, 1e-12);
}

TEST(RoadObjectMesh, ASweepRoundACornerIsMitredAndKeepsItsWidth)
{
    // (0,0) to (10,0) to (10,10): a left turn of 90 degrees. The section
    // 1 wide, 1 tall, centred on the line: volume 1 x 20.
    TriangleMesh mesh;
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(10, 0), 0.0},
                                      {Point2(10, 10), 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 1.0}, {-0.5, 1.0}};
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 20.0, 1e-9);
    // The outside of the corner is at (10.5, -0.5): half a unit from both
    // legs, where the mitre puts it, not at (10, -0.5).
    const auto box = mesh.bounds();
    EXPECT_NEAR(box.max.x, 10.5, 1e-12);
    EXPECT_NEAR(box.min.y, -0.5, 1e-12);
}

TEST(RoadObjectMesh, AHollowSweepIsTheRingBetweenItsLoopsTimesItsLength)
{
    TriangleMesh mesh;
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(0, 5), 1.0}};
    const std::vector<Point2> outer{{-1, -1}, {1, -1}, {1, 1}, {-1, 1}};
    const std::vector<Point2> inner{{-0.5, -0.5}, {0.5, -0.5}, {0.5, 0.5}, {-0.5, 0.5}};
    addHollowSweep(mesh, path, outer, inner);
    EXPECT_TRUE(isClosedMesh(mesh));
    // The sections stand upright on a path rising 1 in 5, so the solid is
    // a sheared prism: its volume is the ring's area, 4 - 1, times the plan
    // length 5 (Cavalieri).
    EXPECT_NEAR(meshVolume(mesh), 3.0 * 5.0, 1e-9);
}

TEST(RoadObjectMesh, AHollowSweepRefusesLoopsThatDoNotCorrespond)
{
    TriangleMesh mesh;
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(0, 5), 0.0}};
    addHollowSweep(mesh, path, std::vector<Point2>{{-1, -1}, {1, -1}, {1, 1}, {-1, 1}},
                   std::vector<Point2>{{0, 0}, {0.5, 0}, {0, 0.5}});
    EXPECT_TRUE(mesh.empty());
}

TEST(RoadObjectMesh, ASweepOfOnePointMakesNothing)
{
    TriangleMesh mesh;
    // Two points a tenth of a millimetre apart are one point.
    addSweep(mesh, std::vector<PathPoint>{{Point2(0, 0), 0.0}, {Point2(0.0001, 0), 0.0}},
             std::vector<Point2>{{-1, 0}, {1, 0}, {1, 1}, {-1, 1}});
    EXPECT_TRUE(mesh.empty());
    EXPECT_FALSE(isClosedMesh(mesh));
}

TEST(RoadObjectMesh, AnOpenSheetIsNotClosed)
{
    TriangleMesh mesh;
    mesh.vertices = {Point3(0, 0, 0), Point3(1, 0, 0), Point3(0, 1, 0)};
    mesh.faces = {{0, 1, 2}};
    EXPECT_FALSE(isClosedMesh(mesh));
    // With its back face too, each edge is used once each way: a closed,
    // if flat, surface.
    mesh.faces.push_back({0, 2, 1});
    EXPECT_TRUE(isClosedMesh(mesh));
    // The same face twice uses each edge twice in one direction.
    mesh.faces = {{0, 1, 2}, {0, 1, 2}};
    EXPECT_FALSE(isClosedMesh(mesh));
}

TEST(RoadObjectMesh, ARegularPolygonsCornersLieOnItsCircle)
{
    const auto octagon = regularPolygon(Point2(3, 4), 2.0, 8, std::numbers::pi / 8.0);
    ASSERT_EQ(octagon.size(), 8u);
    for (const Point2& corner : octagon) {
        EXPECT_NEAR(corner.distanceTo(Point2(3, 4)), 2.0, 1e-12);
    }
    // Started at 22.5 degrees, its flats face the axes: the lowest corner
    // is 2 cos 22.5 below the centre.
    double lowest = 1e9;
    for (const Point2& corner : octagon) {
        lowest = std::min(lowest, corner.y);
    }
    EXPECT_NEAR(lowest, 4.0 - 2.0 * std::cos(std::numbers::pi / 8.0), 1e-12);
    EXPECT_TRUE(regularPolygon(Point2(0, 0), 1.0, 2).empty());
}

// ---- the sweep past the mitre limit, and round a closed path ------------------------------------
//
// A mitre longer than four times the half-width (a turn of more than about
// 151 degrees) used to be shortened, which narrowed the section along the
// whole leg: the joint ring was 0.695 of full width at a 160 degree turn and
// the leg tapered to it. Past the limit the joint is now two rings, square to
// the leg that arrives and the leg that leaves.

TEST(RoadObjectMesh, ASweepRoundATurnPastTheMitreLimitKeepsItsFullWidthAlongBothLegs)
{
    // East 10 m, then 10 m on a heading of 160 degrees: a left turn of 160.
    // The mitre would be 1 / cos 80 = 5.76 times the half-width, past the
    // limit of 4. A section 1 wide and 1 tall, centred on the line, keeps its
    // area times the length of the line - 1 x 1 x 20 - whatever the turn:
    // the two lobes the joint makes, one each side of the line, are equal
    // and opposite.
    const Point2 end = along(Point2(10, 0), 160.0, 10.0);
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(10, 0), 0.0}, {end, 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 1.0}, {-0.5, 1.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 20.0, 1e-9);
    // The first leg ends in a full-width ring square to it, at (10, +-0.5);
    // the second starts in one square to itself: from (10, 0) along its left
    // normal (-sin 160, cos 160) half a metre each way.
    EXPECT_TRUE(hasVertex(mesh, 10.0, 0.5, 0.0));
    EXPECT_TRUE(hasVertex(mesh, 10.0, -0.5, 0.0));
    const double turn = 160.0 * std::numbers::pi / 180.0;
    EXPECT_TRUE(hasVertex(mesh, 10.0 - 0.5 * std::sin(turn), 0.5 * std::cos(turn), 0.0));
    EXPECT_TRUE(hasVertex(mesh, 10.0 + 0.5 * std::sin(turn), -0.5 * std::cos(turn), 0.0));
    // Two rings at the joint and one at each end: four rings of four corners.
    EXPECT_EQ(mesh.vertices.size(), 16u);
}

TEST(RoadObjectMesh, ASweepRoundATurnJustInsideTheMitreLimitIsStillOneMitredRing)
{
    // 150 degrees: 1 / cos 75 = 3.86, inside the limit. One ring at the
    // joint (three rings of four corners) and the same volume, 1 x 1 x 20.
    const Point2 end = along(Point2(10, 0), 150.0, 10.0);
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(10, 0), 0.0}, {end, 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 1.0}, {-0.5, 1.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_EQ(mesh.vertices.size(), 12u);
    EXPECT_NEAR(meshVolume(mesh), 20.0, 1e-9);
}

TEST(RoadObjectMesh, ASweepThatDoublesBackOnItselfKeepsItsWidthToTheTurn)
{
    // East 10 m and back 5: the normals cancel, there is no mitre at all.
    // 1 x 1 x 15, the joint's ring in two, square to each leg.
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(10, 0), 0.0},
                                      {Point2(5, 0), 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 1.0}, {-0.5, 1.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 15.0, 1e-9);
    EXPECT_TRUE(hasVertex(mesh, 10.0, 0.5, 0.0));
    EXPECT_TRUE(hasVertex(mesh, 10.0, -0.5, 0.0));
}

TEST(RoadObjectMesh, AHollowSweepRoundAHairpinKeepsItsRingAndStaysClosed)
{
    // The ring between a 2 x 2 and a 1 x 1 loop is 4 - 1 = 3, along 20 m.
    const Point2 end = along(Point2(10, 0), 160.0, 10.0);
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(10, 0), 0.0}, {end, 0.0}};
    TriangleMesh mesh;
    addHollowSweep(mesh, path, std::vector<Point2>{{-1, -1}, {1, -1}, {1, 1}, {-1, 1}},
                   std::vector<Point2>{{-0.5, -0.5}, {0.5, -0.5}, {0.5, 0.5}, {-0.5, 0.5}});
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 3.0 * 20.0, 1e-9);
}

TEST(RoadObjectMesh, APathThatReturnsToItsStartIsSweptAsALoopMitredAtTheStartWithNoEndFaces)
{
    // A 10 m square, the first corner repeated at the end as a closed
    // polyline is stored. The section 1 wide and 2 tall, centred: the band
    // between a 11 m square and a 9 m one, (121 - 81) x 2 = 80. Four rings of
    // four corners and no end faces: 16 vertices, 4 x 4 x 2 = 32 triangles
    // (an open sweep of five rings would have 36).
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0},
                                      {Point2(10, 0), 0.0},
                                      {Point2(10, 10), 0.0},
                                      {Point2(0, 10), 0.0},
                                      {Point2(0, 0), 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 2.0}, {-0.5, 2.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 80.0, 1e-9);
    EXPECT_EQ(mesh.vertices.size(), 16u);
    EXPECT_EQ(mesh.triangleCount(), 32u);
    // The start corner is mitred like the other three: the outside of it at
    // (-0.5, -0.5), the inside at (0.5, 0.5). It was two square end faces,
    // one at (0, +-0.5) and one at (+-0.5, 0).
    EXPECT_TRUE(hasVertex(mesh, -0.5, -0.5, 0.0));
    EXPECT_TRUE(hasVertex(mesh, 0.5, 0.5, 0.0));
    EXPECT_TRUE(hasVertex(mesh, -0.5, -0.5, 2.0));
}

TEST(RoadObjectMesh, ASectionOffCentreOnALoopHasTheAreaBetweenTheLineAndItsOffset)
{
    // Section u from 0 to 1, left of a counter-clockwise square, which is
    // inside it: the band between the 10 m square and the 8 m one inside it,
    // 100 - 64 = 36, 1 tall. A notch at the start corner and an overlap at the
    // same corner, which the old end faces made, gave 37.
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0},
                                      {Point2(10, 0), 0.0},
                                      {Point2(10, 10), 0.0},
                                      {Point2(0, 10), 0.0},
                                      {Point2(0, 0), 0.0}};
    const std::vector<Point2> section{{0.0, 0.0}, {1.0, 0.0}, {1.0, 1.0}, {0.0, 1.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 36.0, 1e-9);
}

TEST(RoadObjectMesh, APathClosedMidwayAlongASideIsOneLoopWithNoCoincidentEndFaces)
{
    // The same 40 m of square, begun and ended at (5, 0): five rings, the
    // first on a straight, 2 x 4 x 5 = 40 triangles and 1 x 2 x 40 = 80.
    const std::vector<PathPoint> path{{Point2(5, 0), 0.0},
                                      {Point2(10, 0), 0.0},
                                      {Point2(10, 10), 0.0},
                                      {Point2(0, 10), 0.0},
                                      {Point2(0, 0), 0.0},
                                      {Point2(5, 0), 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 2.0}, {-0.5, 2.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 80.0, 1e-9);
    EXPECT_EQ(mesh.vertices.size(), 20u);
    EXPECT_EQ(mesh.triangleCount(), 40u);
}

TEST(RoadObjectMesh, ATriangleOfPathIsTheSmallestLoop)
{
    // 6, 10 and 8 round: 24 m. Every turn is inside the mitre limit (127, 143
    // and 90 degrees), so three rings of four corners: 1 x 1 x 24.
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0},
                                      {Point2(6, 0), 0.0},
                                      {Point2(0, 8), 0.0},
                                      {Point2(0, 0), 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 1.0}, {-0.5, 1.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_EQ(mesh.vertices.size(), 12u);
    EXPECT_EQ(mesh.triangleCount(), 24u);
    EXPECT_NEAR(meshVolume(mesh), 24.0, 1e-9);
}

TEST(RoadObjectMesh, APathThatEndsAtTheStartButAtAnotherLevelIsNotALoop)
{
    // Back above its first point, a metre higher: a spiral's turn, with its two
    // ends. Five rings, and the end faces (36 triangles), as before.
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0},
                                      {Point2(10, 0), 0.0},
                                      {Point2(10, 10), 0.0},
                                      {Point2(0, 10), 0.0},
                                      {Point2(0, 0), 1.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 1.0}, {-0.5, 1.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_EQ(mesh.triangleCount(), 36u);
}

TEST(RoadObjectMesh, ALineDrawnOutAndBackIsNotALoopAndKeepsItsEnds)
{
    // (0,0) to (10,0) and back: three points, so no triangle of path to wrap
    // round. The turn is a full 180 degrees, split at the joint; 1 x 1 x 20.
    const std::vector<PathPoint> path{{Point2(0, 0), 0.0}, {Point2(10, 0), 0.0},
                                      {Point2(0, 0), 0.0}};
    const std::vector<Point2> section{{-0.5, 0.0}, {0.5, 0.0}, {0.5, 1.0}, {-0.5, 1.0}};
    TriangleMesh mesh;
    addSweep(mesh, path, section);
    EXPECT_TRUE(isClosedMesh(mesh));
    EXPECT_NEAR(meshVolume(mesh), 20.0, 1e-9);
}
