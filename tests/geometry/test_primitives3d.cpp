#include <gtest/gtest.h>

#include "katana/geometry/primitives3d.hpp"

using namespace katana::geometry;
using katana::math::Ray;

TEST(GeometrySegment3, ClosestPointAndDistance)
{
    const Segment3 segment{Point3(0, 0, 0), Point3(10, 0, 0)};
    EXPECT_DOUBLE_EQ(segment.length(), 10.0);
    EXPECT_EQ(segment.closestPoint(Point3(4, 3, 4)), Point3(4, 0, 0));
    EXPECT_DOUBLE_EQ(segment.distanceTo(Point3(4, 3, 4)), 5.0);
    EXPECT_EQ(segment.closestPoint(Point3(-4, 0, 0)), Point3(0, 0, 0));
    EXPECT_EQ((Segment3{Point3(1, 1, 1), Point3(1, 1, 1)}).closestPoint(Point3(9, 9, 9)),
              Point3(1, 1, 1));
}

TEST(GeometryTriangle3, AreaNormalPlane)
{
    const Triangle3 triangle{Point3(0, 0, 5), Point3(4, 0, 5), Point3(0, 3, 5)};
    EXPECT_DOUBLE_EQ(triangle.area(), 6.0);
    EXPECT_EQ(triangle.scaledNormal(), Vec3(0, 0, 12));
    const auto plane = triangle.plane();
    ASSERT_TRUE(plane.has_value());
    EXPECT_DOUBLE_EQ(plane->signedDistance(Point3(1, 1, 7)), 2.0);
    EXPECT_FALSE((Triangle3{Point3(0, 0, 0), Point3(1, 1, 1), Point3(2, 2, 2)}).plane().has_value());
}

TEST(GeometryRayTriangle, HitsMissesAndDegenerateCases)
{
    const Triangle3 triangle{Point3(0, 0, 0), Point3(4, 0, 0), Point3(0, 4, 0)};

    const auto hit = intersect(Ray{Point3(1, 1, 10), Vec3(0, 0, -1)}, triangle);
    ASSERT_TRUE(hit.has_value());
    EXPECT_DOUBLE_EQ(*hit, 10.0);

    // Both faces are hit.
    EXPECT_TRUE(intersect(Ray{Point3(1, 1, -10), Vec3(0, 0, 1)}, triangle).has_value());
    // Outside the triangle but inside its plane's bounding square.
    EXPECT_FALSE(intersect(Ray{Point3(3, 3, 10), Vec3(0, 0, -1)}, triangle).has_value());
    // Pointing away.
    EXPECT_FALSE(intersect(Ray{Point3(1, 1, 10), Vec3(0, 0, 1)}, triangle).has_value());
    // Parallel to the triangle.
    EXPECT_FALSE(intersect(Ray{Point3(1, 1, 10), Vec3(1, 0, 0)}, triangle).has_value());
    // Exactly through a vertex and along an edge boundary counts as a hit.
    EXPECT_TRUE(intersect(Ray{Point3(0, 0, 10), Vec3(0, 0, -1)}, triangle).has_value());
    EXPECT_TRUE(intersect(Ray{Point3(2, 0, 10), Vec3(0, 0, -1)}, triangle).has_value());
    // Degenerate triangle.
    EXPECT_FALSE(intersect(Ray{Point3(1, 1, 10), Vec3(0, 0, -1)},
                           Triangle3{Point3(0, 0, 0), Point3(1, 1, 0), Point3(2, 2, 0)})
                     .has_value());
}
