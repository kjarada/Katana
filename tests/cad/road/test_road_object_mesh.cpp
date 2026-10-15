// The solids road objects are made of (include/katana/cad/road/road_object_mesh.hpp).
//
// Every expected volume is worked out by hand from the shape: a box is its
// three sides multiplied, a prism its section's area times its length, a
// hollow sweep the ring between its loops times its length. A mitred corner
// keeps a section symmetric about its line at exactly its area times the
// length of that line: what the outside of the bend gains, the inside loses.

#include <gtest/gtest.h>

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
