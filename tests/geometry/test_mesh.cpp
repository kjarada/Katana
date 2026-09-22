// A triangle mesh (PLAN.MD 20.2, slice 4): what a 12d trimesh carries, and
// deliberately not a surface.
#include <gtest/gtest.h>

#include <cmath>
#include <limits>

#include "katana/geometry/mesh.hpp"

using katana::core::ErrorCode;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::TriangleMesh;

namespace {

// A unit cube from (0,0,0) to (1,1,1): eight vertices, twelve triangles.
// Its area is 6 square units exactly and its plan hull is the unit square,
// both worked out from the shape and not from the program.
TriangleMesh unitCube()
{
    TriangleMesh mesh;
    mesh.vertices = {Point3(0, 0, 0), Point3(1, 0, 0), Point3(1, 1, 0), Point3(0, 1, 0),
                     Point3(0, 0, 1), Point3(1, 0, 1), Point3(1, 1, 1), Point3(0, 1, 1)};
    mesh.faces = {{0, 2, 1}, {0, 3, 2}, // bottom
                  {4, 5, 6}, {4, 6, 7}, // top
                  {0, 1, 5}, {0, 5, 4}, // front
                  {1, 2, 6}, {1, 6, 5}, // right
                  {2, 3, 7}, {2, 7, 6}, // back
                  {3, 0, 4}, {3, 4, 7}};
    return mesh;
}

} // namespace

TEST(TriangleMesh, ACubeKnowsItsTrianglesItsAreaAndItsBox)
{
    const TriangleMesh cube = unitCube();
    EXPECT_EQ(cube.triangleCount(), 12u);
    EXPECT_FALSE(cube.empty());
    // Six faces of a unit square, each two triangles of area 1/2.
    EXPECT_DOUBLE_EQ(cube.area(), 6.0);
    const auto box = cube.bounds();
    EXPECT_EQ(box.min, Point3(0, 0, 0));
    EXPECT_EQ(box.max, Point3(1, 1, 1));
    const auto first = cube.triangle(0);
    ASSERT_TRUE(first.has_value());
    EXPECT_EQ(first->a, Point3(0, 0, 0));
    EXPECT_DOUBLE_EQ(first->area(), 0.5);
}

TEST(TriangleMesh, ThePlanFootprintIsTheHullOfTheVerticesAndSaysSo)
{
    const auto hull = unitCube().planHull();
    // The cube's eight vertices project onto four corners: the unit square.
    ASSERT_EQ(hull.size(), 4u);
    for (const Point2& corner : hull) {
        EXPECT_TRUE((corner == Point2(0, 0)) || (corner == Point2(1, 0)) ||
                    (corner == Point2(1, 1)) || (corner == Point2(0, 1)))
            << corner.x << "," << corner.y;
    }

    // A vertical wall has no area in plan, and its footprint is the segment
    // it stands on - which is the truth about where it is, and is why a
    // caller must cope with a hull of two points rather than assume a
    // polygon.
    TriangleMesh wall;
    wall.vertices = {Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 0, 3), Point3(0, 0, 3)};
    wall.faces = {{0, 1, 2}, {0, 2, 3}};
    const auto standsOn = wall.planHull();
    ASSERT_EQ(standsOn.size(), 2u);
    EXPECT_EQ(standsOn[0], Point2(0, 0));
    EXPECT_EQ(standsOn[1], Point2(10, 0));
    EXPECT_DOUBLE_EQ(wall.area(), 30.0) << "the wall still has its area in space";
}

TEST(TriangleMesh, AFaceNamingAVertexThatDoesNotExistIsRefusedAndNotRead)
{
    TriangleMesh mesh = unitCube();
    mesh.faces.push_back({0, 1, 99});
    const auto status = validate(mesh);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(status.error().describe().find("99"), std::string::npos)
        << status.error().describe();
    // And the bad face is refused one at a time, so drawing an unvalidated
    // mesh cannot read past the end of the vertices.
    EXPECT_FALSE(mesh.triangle(12).has_value());
    EXPECT_FALSE(mesh.triangle(13).has_value()) << "past the end is nullopt, not a crash";
    EXPECT_DOUBLE_EQ(mesh.area(), 6.0) << "a face that cannot be read adds no area";
}

TEST(TriangleMesh, ANonFiniteVertexIsRefusedNamingIt)
{
    TriangleMesh mesh = unitCube();
    mesh.vertices[3].z = std::numeric_limits<double>::quiet_NaN();
    const auto status = validate(mesh);
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().describe().find("vertex=3"), std::string::npos)
        << status.error().describe();
}

TEST(TriangleMesh, AnEmptyMeshIsValidAndAnswersEverythingWithNothing)
{
    const TriangleMesh mesh;
    EXPECT_TRUE(validate(mesh).ok()) << "nothing to be wrong with";
    EXPECT_TRUE(mesh.empty());
    EXPECT_EQ(mesh.triangleCount(), 0u);
    EXPECT_DOUBLE_EQ(mesh.area(), 0.0);
    EXPECT_TRUE(mesh.planHull().empty());
    EXPECT_FALSE(mesh.triangle(0).has_value());
    EXPECT_TRUE(mesh.bounds().empty()) << "an empty box, not a box about the origin";
}
