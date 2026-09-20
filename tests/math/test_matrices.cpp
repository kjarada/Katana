#include <gtest/gtest.h>

#include "katana/math/mat2.hpp"
#include "katana/math/mat3.hpp"
#include "katana/math/mat4.hpp"
#include "support/property.hpp"

using namespace katana::math;
using katana::test::kPropertyIterations;
using katana::test::Random;

TEST(MathMat2, IdentityMultiplyTransposeDeterminant)
{
    const Mat2 m(1.0, 2.0, 3.0, 4.0);
    EXPECT_EQ(Mat2{} * Vec2(4.0, 5.0), Vec2(4.0, 5.0));
    EXPECT_EQ(m * Vec2(1.0, 1.0), Vec2(3.0, 7.0));
    EXPECT_EQ(m * Mat2{}, m);
    EXPECT_EQ(m * m, Mat2(7.0, 10.0, 15.0, 22.0));
    EXPECT_EQ(m.transposed(), Mat2(1.0, 3.0, 2.0, 4.0));
    EXPECT_DOUBLE_EQ(m.determinant(), -2.0);
}

TEST(MathMat2, InverseAndSingularDetection)
{
    const Mat2 m(4.0, 7.0, 2.0, 6.0);
    const auto inverse = m.inverse();
    ASSERT_TRUE(inverse.has_value());
    EXPECT_TRUE(nearlyEqual(m * *inverse, Mat2{}));

    EXPECT_FALSE(Mat2(1.0, 2.0, 2.0, 4.0).inverse().has_value()); // parallel rows
    EXPECT_FALSE(Mat2(0.0, 0.0, 0.0, 0.0).inverse().has_value());
    // Scale invariance: a tiny but well-conditioned matrix is invertible.
    EXPECT_TRUE(Mat2(1e-9, 0.0, 0.0, 1e-9).inverse().has_value());
}

TEST(MathMat2, RotationPreservesLength)
{
    const Vec2 v = Mat2::rotation(kHalfPi) * Vec2(1.0, 0.0);
    EXPECT_TRUE(nearlyEqual(v, Vec2(0.0, 1.0)));
    EXPECT_NEAR(Mat2::rotation(1.234).determinant(), 1.0, 1e-15);
}

TEST(MathMat3, InverseDeterminantAndSingular)
{
    const Mat3 m(2.0, 0.0, 1.0, 1.0, 3.0, 2.0, 1.0, 1.0, 1.0);
    EXPECT_DOUBLE_EQ(m.determinant(), 0.0 + 2.0 * (3.0 - 2.0) - 0.0 + 1.0 * (1.0 - 3.0));
    EXPECT_FALSE(m.inverse().has_value()); // det == 0

    const Mat3 regular(2.0, 0.0, 1.0, 1.0, 3.0, 2.0, 1.0, 1.0, 2.0);
    const auto inverse = regular.inverse();
    ASSERT_TRUE(inverse.has_value());
    EXPECT_TRUE(nearlyEqual(regular * *inverse, Mat3{}));
    EXPECT_TRUE(nearlyEqual(*inverse * regular, Mat3{}));
    EXPECT_EQ(regular.transposed().transposed(), regular);
}

TEST(MathMat3, AffineTransformsOfPointsAndVectors)
{
    const Mat3 move = Mat3::translation(Vec2(10.0, -5.0));
    EXPECT_EQ(transformPoint(move, Vec2(1.0, 1.0)), Vec2(11.0, -4.0));
    EXPECT_EQ(transformVector(move, Vec2(1.0, 1.0)), Vec2(1.0, 1.0)); // directions ignore translation

    const Mat3 turn = Mat3::rotationAbout(Vec2(1.0, 1.0), kHalfPi);
    EXPECT_TRUE(nearlyEqual(transformPoint(turn, Vec2(2.0, 1.0)), Vec2(1.0, 2.0)));
    EXPECT_TRUE(nearlyEqual(transformPoint(turn, Vec2(1.0, 1.0)), Vec2(1.0, 1.0)));

    const Mat3 grow = Mat3::scalingAbout(Vec2(1.0, 1.0), 2.0, 3.0);
    EXPECT_EQ(transformPoint(grow, Vec2(2.0, 2.0)), Vec2(3.0, 4.0));
}

TEST(MathMat3, ReflectionAboutLine)
{
    // Mirror about the vertical line x = 2.
    const Mat3 mirror = Mat3::reflection(Vec2(2.0, 0.0), Vec2(0.0, 1.0));
    EXPECT_TRUE(nearlyEqual(transformPoint(mirror, Vec2(5.0, 7.0)), Vec2(-1.0, 7.0)));
    EXPECT_NEAR(mirror.determinant(), -1.0, 1e-15);
    // Mirror about y = x swaps coordinates.
    const Mat3 diagonal = Mat3::reflection(Vec2(0.0, 0.0), Vec2(1.0, 1.0));
    EXPECT_TRUE(nearlyEqual(transformPoint(diagonal, Vec2(3.0, 8.0)), Vec2(8.0, 3.0)));
    // Degenerate direction yields identity rather than garbage.
    EXPECT_EQ(Mat3::reflection(Vec2(1.0, 1.0), Vec2(0.0, 0.0)), Mat3{});
}

TEST(MathMat4, TransformCompositionOrder)
{
    const Mat4 translate = Mat4::translation(Vec3(10.0, 0.0, 0.0));
    const Mat4 scale = Mat4::scaling(Vec3(2.0, 2.0, 2.0));
    // (translate * scale) scales first, then translates.
    EXPECT_EQ(transformPoint(translate * scale, Vec3(1.0, 1.0, 1.0)), Vec3(12.0, 2.0, 2.0));
    EXPECT_EQ(transformPoint(scale * translate, Vec3(1.0, 1.0, 1.0)), Vec3(22.0, 2.0, 2.0));
    EXPECT_EQ(transformVector(translate, Vec3(1.0, 2.0, 3.0)), Vec3(1.0, 2.0, 3.0));
    EXPECT_EQ(Mat4{} * Vec4(1.0, 2.0, 3.0, 4.0), Vec4(1.0, 2.0, 3.0, 4.0));
}

TEST(MathMat4, RotationAboutAxis)
{
    const Mat4 aboutZ = Mat4::rotation(Vec3(0.0, 0.0, 5.0), kHalfPi); // axis is normalised
    EXPECT_TRUE(nearlyEqual(transformPoint(aboutZ, Vec3(1.0, 0.0, 0.0)), Vec3(0.0, 1.0, 0.0)));
    EXPECT_NEAR(aboutZ.determinant(), 1.0, 1e-15);
    EXPECT_EQ(Mat4::rotation(Vec3(0.0, 0.0, 0.0), 1.0), Mat4{});
}

TEST(MathMat4, InverseOfKnownMatrixAndSingularDetection)
{
    const Mat4 m(1.0, 0.0, 2.0, 1.0, 0.0, 1.0, 0.0, 3.0, 4.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 1.0);
    const auto inverse = m.inverse();
    ASSERT_TRUE(inverse.has_value());
    EXPECT_TRUE(nearlyEqual(m * *inverse, Mat4{}));
    EXPECT_TRUE(nearlyEqual(*inverse * m, Mat4{}));

    Mat4 singular = m;
    for (std::size_t col = 0; col < 4; ++col) {
        singular(3, col) = 2.0 * singular(0, col); // row 3 = 2 * row 0
    }
    EXPECT_NEAR(singular.determinant(), 0.0, 1e-12);
    EXPECT_FALSE(singular.inverse().has_value());
}

// ---- properties ------------------------------------------------------------

TEST(MathMatrixProperties, InverseRoundTripsPoints)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        const Mat4 m = Mat4::translation(Vec3(random.real(-1e3, 1e3), random.real(-1e3, 1e3),
                                              random.real(-1e3, 1e3))) *
                       Mat4::rotation(Vec3(random.real(-1, 1), random.real(-1, 1), 1.0),
                                      random.real(-kPi, kPi)) *
                       Mat4::scaling(Vec3(random.real(0.5, 2.0), random.real(0.5, 2.0),
                                          random.real(0.5, 2.0)));
        const auto inverse = m.inverse();
        ASSERT_TRUE(inverse.has_value());

        const Vec3 p(random.real(-1e3, 1e3), random.real(-1e3, 1e3), random.real(-1e3, 1e3));
        const Vec3 roundTrip = transformPoint(*inverse, transformPoint(m, p));
        EXPECT_TRUE(nearlyEqual(roundTrip, p, 1e-11)) << roundTrip << " vs " << p;
    }
}

TEST(MathMatrixProperties, DeterminantIsMultiplicative)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        Mat3 a;
        Mat3 b;
        for (std::size_t k = 0; k < 9; ++k) {
            a.data[k] = random.real(-5.0, 5.0);
            b.data[k] = random.real(-5.0, 5.0);
        }
        EXPECT_TRUE(nearlyEqual((a * b).determinant(), a.determinant() * b.determinant(), 1e-10));
    }
}

TEST(MathMatrixProperties, TransposeOfProductIsReversedProductOfTransposes)
{
    Random random;
    for (int i = 0; i < kPropertyIterations; ++i) {
        Mat4 a;
        Mat4 b;
        for (std::size_t k = 0; k < 16; ++k) {
            a.data[k] = random.real(-5.0, 5.0);
            b.data[k] = random.real(-5.0, 5.0);
        }
        EXPECT_TRUE(nearlyEqual((a * b).transposed(), b.transposed() * a.transposed(), 1e-12));
    }
}
