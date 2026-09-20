#include <gtest/gtest.h>

#include "katana/math/primitives.hpp"

using namespace katana::math;

TEST(MathPlane, SignedDistanceAndProjection)
{
    const Plane ground(Vec3(0.0, 0.0, 1.0), -5.0); // z = 5
    EXPECT_DOUBLE_EQ(ground.signedDistance(Vec3(1.0, 2.0, 8.0)), 3.0);
    EXPECT_DOUBLE_EQ(ground.signedDistance(Vec3(1.0, 2.0, 1.0)), -4.0);
    EXPECT_EQ(ground.project(Vec3(1.0, 2.0, 8.0)), Vec3(1.0, 2.0, 5.0));
}

TEST(MathPlane, FromPointsUsesRightHandRuleAndRejectsCollinear)
{
    const auto plane = Plane::fromPoints(Vec3(0, 0, 2), Vec3(1, 0, 2), Vec3(0, 1, 2));
    ASSERT_TRUE(plane.has_value());
    EXPECT_TRUE(nearlyEqual(plane->normal, Vec3(0.0, 0.0, 1.0)));
    EXPECT_NEAR(plane->signedDistance(Vec3(7.0, -3.0, 2.0)), 0.0, 1e-15);

    EXPECT_FALSE(Plane::fromPoints(Vec3(0, 0, 0), Vec3(1, 1, 1), Vec3(2, 2, 2)).has_value());
    EXPECT_FALSE(Plane::fromPoints(Vec3(1, 1, 1), Vec3(1, 1, 1), Vec3(2, 2, 2)).has_value());
    EXPECT_FALSE(Plane::fromPointNormal(Vec3(0, 0, 0), Vec3(0, 0, 0)).has_value());
}

TEST(MathRayPlane, HitMissParallelAndBehind)
{
    const Plane ground(Vec3(0.0, 0.0, 1.0), 0.0);

    const auto hit = intersect(Ray{Vec3(0, 0, 10), Vec3(0, 0, -2)}, ground);
    ASSERT_TRUE(hit.has_value());
    EXPECT_DOUBLE_EQ(*hit, 5.0); // parameter is in units of |direction|
    EXPECT_EQ((Ray{Vec3(0, 0, 10), Vec3(0, 0, -2)}.at(*hit)), Vec3(0, 0, 0));

    EXPECT_FALSE(intersect(Ray{Vec3(0, 0, 10), Vec3(1, 0, 0)}, ground).has_value()); // parallel
    EXPECT_FALSE(intersect(Ray{Vec3(0, 0, 0), Vec3(1, 0, 0)}, ground).has_value());  // in plane
    EXPECT_FALSE(intersect(Ray{Vec3(0, 0, 10), Vec3(0, 0, 1)}, ground).has_value()); // behind

    const auto touching = intersect(Ray{Vec3(0, 0, 0), Vec3(0, 0, 1)}, ground); // origin on plane
    ASSERT_TRUE(touching.has_value());
    EXPECT_DOUBLE_EQ(*touching, 0.0);
}

TEST(MathAABB, DefaultIsEmptyAndGrowsByExpansion)
{
    AABB box;
    EXPECT_TRUE(box.empty());
    EXPECT_EQ(box.size(), Vec3(0, 0, 0));
    EXPECT_FALSE(box.contains(Vec3(0, 0, 0)));

    box.expand(Vec3(1, 2, 3));
    EXPECT_FALSE(box.empty());
    EXPECT_EQ(box.size(), Vec3(0, 0, 0)); // a single point has zero extent but is not empty
    box.expand(Vec3(-1, 4, 0));
    EXPECT_EQ(box.min, Vec3(-1, 2, 0));
    EXPECT_EQ(box.max, Vec3(1, 4, 3));
    EXPECT_EQ(box.center(), Vec3(0, 3, 1.5));

    AABB merged;
    merged.expand(AABB{}); // merging an empty box is a no-op
    EXPECT_TRUE(merged.empty());
    merged.expand(box);
    EXPECT_EQ(merged.min, box.min);
}

TEST(MathAABB, ContainmentAndOverlapAreBoundaryInclusive)
{
    const AABB box(Vec3(0, 0, 0), Vec3(2, 4, 6));
    EXPECT_TRUE(box.contains(Vec3(2, 4, 6)));
    EXPECT_TRUE(box.contains(Vec3(1, 1, 1)));
    EXPECT_FALSE(box.contains(Vec3(2.0000001, 1, 1)));

    EXPECT_TRUE(box.intersects(AABB(Vec3(2, 4, 6), Vec3(3, 5, 7)))); // corner touch
    EXPECT_FALSE(box.intersects(AABB(Vec3(2.1, 0, 0), Vec3(3, 1, 1))));
    EXPECT_FALSE(box.intersects(AABB{}));
}

TEST(MathRayAABB, SlabIntersectionEdgeCases)
{
    const AABB box(Vec3(0, 0, 0), Vec3(1, 1, 1));

    const auto through = intersect(Ray{Vec3(-1, 0.5, 0.5), Vec3(1, 0, 0)}, box);
    ASSERT_TRUE(through.has_value());
    EXPECT_DOUBLE_EQ(through->tEnter, 1.0);
    EXPECT_DOUBLE_EQ(through->tExit, 2.0);

    const auto inside = intersect(Ray{Vec3(0.5, 0.5, 0.5), Vec3(0, 0, 1)}, box);
    ASSERT_TRUE(inside.has_value());
    EXPECT_DOUBLE_EQ(inside->tEnter, 0.0);
    EXPECT_DOUBLE_EQ(inside->tExit, 0.5);

    // Travelling exactly along a face: zero direction components on a slab boundary.
    EXPECT_TRUE(intersect(Ray{Vec3(-1, 0, 0), Vec3(1, 0, 0)}, box).has_value());
    // Parallel to a slab but outside it.
    EXPECT_FALSE(intersect(Ray{Vec3(-1, 2, 0.5), Vec3(1, 0, 0)}, box).has_value());
    // Pointing away.
    EXPECT_FALSE(intersect(Ray{Vec3(-1, 0.5, 0.5), Vec3(-1, 0, 0)}, box).has_value());
    // Diagonal through opposite corners.
    const auto diagonal = intersect(Ray{Vec3(-1, -1, -1), Vec3(1, 1, 1)}, box);
    ASSERT_TRUE(diagonal.has_value());
    EXPECT_DOUBLE_EQ(diagonal->tEnter, 1.0);
    EXPECT_DOUBLE_EQ(diagonal->tExit, 2.0);

    EXPECT_FALSE(intersect(Ray{Vec3(0, 0, 0), Vec3(1, 0, 0)}, AABB{}).has_value());
}
