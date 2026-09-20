#include <gtest/gtest.h>

#include <cmath>

#include "katana/math/vec2.hpp"
#include "katana/math/vec3.hpp"
#include "katana/math/vec4.hpp"
#include "support/property.hpp"

using namespace katana::math;
using katana::test::kPropertyIterations;
using katana::test::Random;

TEST(MathVec2, ArithmeticAndLength)
{
    const Vec2 a(3.0, 4.0);
    const Vec2 b(1.0, 2.0);

    EXPECT_EQ(a + b, Vec2(4.0, 6.0));
    EXPECT_EQ(a - b, Vec2(2.0, 2.0));
    EXPECT_EQ(a * 2.0, Vec2(6.0, 8.0));
    EXPECT_EQ(2.0 * a, Vec2(6.0, 8.0));
    EXPECT_EQ(a / 2.0, Vec2(1.5, 2.0));
    EXPECT_EQ(-a, Vec2(-3.0, -4.0));
    EXPECT_DOUBLE_EQ(a.length(), 5.0);
    EXPECT_DOUBLE_EQ(a.lengthSquared(), 25.0);
    EXPECT_DOUBLE_EQ(distance(a, b), std::sqrt(8.0));
}

TEST(MathVec2, DotCrossAndPerpendicular)
{
    const Vec2 x(1.0, 0.0);
    const Vec2 y(0.0, 1.0);

    EXPECT_DOUBLE_EQ(dot(Vec2(3.0, 4.0), Vec2(1.0, 2.0)), 11.0);
    EXPECT_DOUBLE_EQ(cross(x, y), 1.0);  // y is counter-clockwise of x
    EXPECT_DOUBLE_EQ(cross(y, x), -1.0); // x is clockwise of y
    EXPECT_EQ(x.perpendicular(), y);
    EXPECT_DOUBLE_EQ(dot(Vec2(3.0, 4.0), Vec2(3.0, 4.0).perpendicular()), 0.0);
}

TEST(MathVec2, NormalizationIncludingZeroVector)
{
    EXPECT_TRUE(nearlyEqual(normalize(Vec2(3.0, 4.0)), Vec2(0.6, 0.8)));
    EXPECT_EQ(normalize(Vec2(0.0, 0.0)), Vec2(0.0, 0.0));
    // hypot-based length does not overflow or underflow for extreme components.
    EXPECT_DOUBLE_EQ(Vec2(3e200, 4e200).length(), 5e200);
    EXPECT_DOUBLE_EQ(Vec2(3e-200, 4e-200).length(), 5e-200);
    EXPECT_NEAR(normalize(Vec2(3e-200, 4e-200)).length(), 1.0, 1e-15);
}

TEST(MathVec2, RotationAndAngle)
{
    EXPECT_TRUE(nearlyEqual(Vec2(1.0, 0.0).rotated(kHalfPi), Vec2(0.0, 1.0)));
    EXPECT_DOUBLE_EQ(Vec2(0.0, 2.0).angle(), kHalfPi);
    EXPECT_DOUBLE_EQ(Vec2(-1.0, 0.0).angle(), kPi);
}

TEST(MathVec2, LerpProjectReflect)
{
    EXPECT_EQ(lerp(Vec2(0.0, 0.0), Vec2(10.0, 20.0), 0.25), Vec2(2.5, 5.0));
    EXPECT_EQ(project(Vec2(3.0, 4.0), Vec2(10.0, 0.0)), Vec2(3.0, 0.0));
    EXPECT_EQ(project(Vec2(3.0, 4.0), Vec2(0.0, 0.0)), Vec2(0.0, 0.0));
    EXPECT_EQ(reflect(Vec2(1.0, -1.0), Vec2(0.0, 1.0)), Vec2(1.0, 1.0));
}

TEST(MathVec2, EqualityIsExactAndNearlyEqualIsTolerant)
{
    const Vec2 a(1.0, 2.0);
    const Vec2 b(1.0 + 1e-12, 2.0);
    EXPECT_NE(a, b);
    EXPECT_TRUE(nearlyEqual(a, b));
}

TEST(MathVec2, DivisionByZeroFollowsIeeeAndIsDetectable)
{
    const Vec2 v = Vec2(1.0, -1.0) / 0.0;
    EXPECT_FALSE(v.isFinite());
}

TEST(MathVec3, CrossProductFollowsRightHandRule)
{
    const Vec3 x(1.0, 0.0, 0.0);
    const Vec3 y(0.0, 1.0, 0.0);
    const Vec3 z(0.0, 0.0, 1.0);
    EXPECT_EQ(cross(x, y), z);
    EXPECT_EQ(cross(y, z), x);
    EXPECT_EQ(cross(z, x), y);
    EXPECT_EQ(cross(x, x), Vec3(0.0, 0.0, 0.0));
    EXPECT_DOUBLE_EQ(distance(x, y), std::sqrt(2.0));
}

TEST(MathVec3, LerpProjectReflect)
{
    EXPECT_EQ(lerp(Vec3(0, 0, 0), Vec3(2, 4, 6), 0.5), Vec3(1, 2, 3));
    EXPECT_EQ(project(Vec3(1, 2, 3), Vec3(0, 0, 5)), Vec3(0, 0, 3));
    EXPECT_EQ(reflect(Vec3(1, 1, -1), Vec3(0, 0, 1)), Vec3(1, 1, 1));
    EXPECT_EQ(normalize(Vec3(0, 0, 0)), Vec3(0, 0, 0));
}

TEST(MathVec4, DotLengthAndHomogeneousHelpers)
{
    const Vec4 a(1.0, 2.0, 3.0, 4.0);
    const Vec4 b(4.0, 3.0, 2.0, 1.0);
    EXPECT_DOUBLE_EQ(dot(a, b), 20.0);
    EXPECT_DOUBLE_EQ(Vec4(2.0, 0.0, 0.0, 0.0).length(), 2.0);
    EXPECT_EQ(Vec4(Vec3(1, 2, 3), 1.0).xyz(), Vec3(1, 2, 3));
    EXPECT_EQ(lerp(a, b, 0.5), Vec4(2.5, 2.5, 2.5, 2.5));
}

// ---- properties ------------------------------------------------------------

TEST(MathVectorProperties, DistanceToSelfIsZeroAndSymmetric)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Vec3 a(random.real(-1e7, 1e7), random.real(-1e7, 1e7), random.real(-1e3, 1e3));
        const Vec3 b(random.real(-1e7, 1e7), random.real(-1e7, 1e7), random.real(-1e3, 1e3));
        EXPECT_EQ(distance(a, a), 0.0);
        EXPECT_EQ(distance(a, b), distance(b, a));
    }
}

TEST(MathVectorProperties, TriangleInequality)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Vec2 a(random.real(-1e4, 1e4), random.real(-1e4, 1e4));
        const Vec2 b(random.real(-1e4, 1e4), random.real(-1e4, 1e4));
        const Vec2 c(random.real(-1e4, 1e4), random.real(-1e4, 1e4));
        EXPECT_LE(distance(a, c), (distance(a, b) + distance(b, c)) * (1.0 + 1e-15));
    }
}

TEST(MathVectorProperties, NormalizedVectorsHaveUnitLength)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Vec3 v(random.real(-1e6, 1e6), random.real(-1e6, 1e6), random.real(-1e6, 1e6));
        EXPECT_NEAR(normalize(v).length(), 1.0, 1e-15);
    }
}

TEST(MathVectorProperties, CrossProductIsOrthogonalToItsOperands)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Vec3 a(random.real(-100, 100), random.real(-100, 100), random.real(-100, 100));
        const Vec3 b(random.real(-100, 100), random.real(-100, 100), random.real(-100, 100));
        const Vec3 c = cross(a, b);
        const double scale = a.length() * b.length() * c.length() + 1.0;
        EXPECT_NEAR(dot(c, a) / scale, 0.0, 1e-15);
        EXPECT_NEAR(dot(c, b) / scale, 0.0, 1e-15);
    }
}

TEST(MathVectorProperties, ProjectionResidualIsOrthogonalAndReflectionIsAnInvolution)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Vec3 v(random.real(-100, 100), random.real(-100, 100), random.real(-100, 100));
        const Vec3 onto(random.real(1, 100), random.real(-100, 100), random.real(-100, 100));
        const Vec3 residual = v - project(v, onto);
        EXPECT_NEAR(dot(residual, onto) / (onto.lengthSquared() + 1.0), 0.0, 1e-13);

        const Vec3 n = normalize(onto);
        EXPECT_TRUE(nearlyEqual(reflect(reflect(v, n), n), v, 1e-12));
    }
}
