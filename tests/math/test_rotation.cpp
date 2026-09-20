#include <gtest/gtest.h>

#include "katana/math/quaternion.hpp"
#include "katana/math/transform.hpp"
#include "support/property.hpp"

using namespace katana::math;
using katana::test::kPropertyIterations;
using katana::test::Random;

TEST(MathQuaternion, NormalizationConjugateInverse)
{
    const Quaternion q(0.0, 0.0, 0.0, 2.0);
    EXPECT_EQ(q.normalized(), Quaternion(0.0, 0.0, 0.0, 1.0));
    EXPECT_EQ(q.inverse(), Quaternion(0.0, 0.0, 0.0, 0.5));
    EXPECT_EQ(Quaternion(1.0, 2.0, 3.0, 4.0).conjugate(), Quaternion(-1.0, -2.0, -3.0, 4.0));
    EXPECT_EQ(Quaternion(0.0, 0.0, 0.0, 0.0).normalized(), Quaternion{}); // identity fallback
    EXPECT_EQ(Quaternion(0.0, 0.0, 0.0, 0.0).inverse(), Quaternion{});
}

TEST(MathQuaternion, AxisAngleRotation)
{
    const Quaternion aboutZ = Quaternion::fromAxisAngle(Vec3(0.0, 0.0, 1.0), kHalfPi);
    EXPECT_TRUE(nearlyEqual(aboutZ.rotate(Vec3(1.0, 0.0, 0.0)), Vec3(0.0, 1.0, 0.0)));
    EXPECT_NEAR(aboutZ.length(), 1.0, 1e-15);
    EXPECT_EQ(Quaternion::fromAxisAngle(Vec3(0.0, 0.0, 0.0), 1.0), Quaternion{});
}

TEST(MathQuaternion, CompositionAppliesRightOperandFirst)
{
    const Quaternion aboutZ = Quaternion::fromAxisAngle(Vec3(0.0, 0.0, 1.0), kHalfPi);
    const Quaternion aboutX = Quaternion::fromAxisAngle(Vec3(1.0, 0.0, 0.0), kHalfPi);
    // x --(about Z)--> y --(about X)--> z
    EXPECT_TRUE(nearlyEqual((aboutX * aboutZ).rotate(Vec3(1.0, 0.0, 0.0)), Vec3(0.0, 0.0, 1.0)));
}

TEST(MathQuaternion, MatrixAgreesWithRotate)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Quaternion q = Quaternion::fromAxisAngle(
            Vec3(random.real(-1, 1), random.real(-1, 1), random.real(0.1, 1)),
            random.real(-kPi, kPi));
        const Vec3 v(random.real(-10, 10), random.real(-10, 10), random.real(-10, 10));
        EXPECT_TRUE(nearlyEqual(q.toMatrix() * v, q.rotate(v), 1e-13));
        EXPECT_NEAR(q.rotate(v).length(), v.length(), 1e-13); // rotations preserve length
    }
}

TEST(MathQuaternion, SlerpEndpointsMidpointAndShortestArc)
{
    const Quaternion a;
    const Quaternion b = Quaternion::fromAxisAngle(Vec3(0.0, 0.0, 1.0), kHalfPi);
    EXPECT_TRUE(nearlyEqual(slerp(a, b, 0.0), a));
    EXPECT_TRUE(nearlyEqual(slerp(a, b, 1.0), b));
    EXPECT_TRUE(nearlyEqual(slerp(a, b, 0.5),
                            Quaternion::fromAxisAngle(Vec3(0.0, 0.0, 1.0), kHalfPi / 2.0)));
    // -b is the same rotation as b; slerp must not take the long way round.
    const Quaternion negated(-b.x, -b.y, -b.z, -b.w);
    EXPECT_TRUE(nearlyEqual(slerp(a, negated, 0.5), slerp(a, b, 0.5)));
    // Identical inputs take the lerp branch without dividing by sin(0).
    EXPECT_TRUE(nearlyEqual(slerp(b, b, 0.3), b));
}

TEST(MathTransform, IdentityMatrixAndTrsOrder)
{
    EXPECT_EQ(Transform{}.matrix(), Mat4{});

    Transform t;
    t.translation = Vec3(10.0, 0.0, 0.0);
    t.rotation = Quaternion::fromAxisAngle(Vec3(0.0, 0.0, 1.0), kHalfPi);
    t.scale = Vec3(2.0, 2.0, 2.0);
    // scale (2,0,0) -> rotate (0,2,0) -> translate (10,2,0)
    EXPECT_TRUE(nearlyEqual(t.transformPoint(Vec3(1.0, 0.0, 0.0)), Vec3(10.0, 2.0, 0.0)));
    EXPECT_TRUE(nearlyEqual(t.transformVector(Vec3(1.0, 0.0, 0.0)), Vec3(0.0, 2.0, 0.0)));
    EXPECT_TRUE(nearlyEqual(transformPoint(t.matrix(), Vec3(1.0, 0.0, 0.0)),
                            t.transformPoint(Vec3(1.0, 0.0, 0.0))));
}

TEST(MathTransform, InverseRoundTripProperty)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        Transform t;
        t.translation = Vec3(random.real(-1e4, 1e4), random.real(-1e4, 1e4), random.real(-100, 100));
        t.rotation = Quaternion::fromAxisAngle(
            Vec3(random.real(-1, 1), random.real(-1, 1), random.real(0.1, 1)),
            random.real(-kPi, kPi));
        t.scale = Vec3(random.real(0.1, 10), random.real(0.1, 10), random.real(0.1, 10));

        const Vec3 p(random.real(-1e3, 1e3), random.real(-1e3, 1e3), random.real(-1e3, 1e3));
        const auto back = t.inverseTransformPoint(t.transformPoint(p));
        ASSERT_TRUE(back.has_value());
        EXPECT_TRUE(nearlyEqual(*back, p, 1e-11)) << *back << " vs " << p;
    }
}

TEST(MathTransform, ZeroScaleHasNoInverse)
{
    Transform t;
    t.scale = Vec3(1.0, 0.0, 1.0);
    EXPECT_FALSE(t.inverseTransformPoint(Vec3(1.0, 2.0, 3.0)).has_value());
}
