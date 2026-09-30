// The precision rule and the reversed-Z projection (gpu/scene_origin.hpp).
// Plain double arithmetic: none of this needs a GPU.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <string>

#include "gpu/scene_origin.hpp"
#include "katana/core/text.hpp"

using katana::math::AABB;
using katana::math::Mat4;
using katana::math::Vec3;
using katana::math::Vec4;
using katana::qt::gpu::chooseSceneOrigin;
using katana::qt::gpu::kOriginErrorPixels;
using katana::qt::gpu::originErrorPixels;
using katana::qt::gpu::ProjectionOptions;
using katana::qt::gpu::relativePosition;
using katana::qt::gpu::relativeViewProjection;
using katana::qt::gpu::reversedZProjection;
using katana::qt::gpu::toFloatColumnMajor;
using katana::render::Camera;
using katana::render::Projection;

namespace {

// Eye-space depth d in front of the eye (z = -d) through a projection: z/w.
double depthAt(const Mat4& projection, double distance)
{
    const Vec4 clip = projection * Vec4(0.0, 0.0, -distance, 1.0);
    return clip.z / clip.w;
}

Camera perspectiveCamera(double nearPlane, double farPlane)
{
    Camera camera;
    camera.setViewportSize(100, 100);
    camera.setProjection(Projection::Perspective);
    camera.setDepthRange(nearPlane, farPlane);
    return camera;
}

// What a vertex shader computes: a column-major float matrix times a float
// position, in float - then where on a width x height screen it lands.
std::array<double, 2> shade(const std::array<float, 16>& m, const std::array<float, 3>& p,
                            int width, int height)
{
    std::array<float, 4> clip{};
    for (std::size_t row = 0; row < 4; ++row) {
        clip[row] =
            m[0 * 4 + row] * p[0] + m[1 * 4 + row] * p[1] + m[2 * 4 + row] * p[2] + m[3 * 4 + row];
    }
    return std::array<double, 2>{(clip[0] / clip[3] * 0.5f + 0.5f) * static_cast<float>(width),
                                 (0.5f - clip[1] / clip[3] * 0.5f) * static_cast<float>(height)};
}

} // namespace

TEST(SceneOrigin, IsTheCentreOfTheBoxAndZeroForAnEmptyBox)
{
    const AABB box(Vec3(300000.0, 6250000.0, 20.0), Vec3(300100.0, 6250400.0, 60.0));
    const Vec3 origin = chooseSceneOrigin(box);
    // (300000 + 300100) / 2, (6250000 + 6250400) / 2, (20 + 60) / 2.
    EXPECT_EQ(origin.x, 300050.0);
    EXPECT_EQ(origin.y, 6250200.0);
    EXPECT_EQ(origin.z, 40.0);
    const Vec3 none = chooseSceneOrigin(AABB{});
    EXPECT_EQ(none.x, 0.0);
    EXPECT_EQ(none.y, 0.0);
    EXPECT_EQ(none.z, 0.0);
}

TEST(SceneOrigin, AnOffsetFromTheOriginKeepsTheMillimetresAFloatAtSurveyCoordinatesLoses)
{
    // A float has a 24-bit significand. 6 250 000 lies in [2^22, 2^23), so its
    // floats are 2^(22 - 23) = 0.5 apart: 6250000.456 can only be stored as
    // 6250000.5, 44 mm out.
    const Vec3 world(300000.123, 6250000.456, 50.789);
    EXPECT_EQ(static_cast<float>(world.y), 6250000.5f);

    // Relative to an origin a few metres away the offset is below 1, where
    // floats are at most 2^-24 apart: every millimetre survives.
    const auto offset = relativePosition(world, Vec3(300000.0, 6250000.0, 50.0));
    EXPECT_NEAR(offset[0], 0.123, 1.0e-6);
    EXPECT_NEAR(offset[1], 0.456, 1.0e-6);
    EXPECT_NEAR(offset[2], 0.789, 1.0e-6);
}

TEST(ReversedZ, PerspectiveMapsTheNearPlaneToOneAndFallsAsNearOverDistanceTowardsInfinity)
{
    const Camera camera = perspectiveCamera(2.0, 1000.0);
    const Mat4 projection = reversedZProjection(camera); // infinite far by default
    // depth = near / distance with near = 2.
    EXPECT_NEAR(depthAt(projection, 2.0), 1.0, 1e-15);
    EXPECT_NEAR(depthAt(projection, 4.0), 0.5, 1e-15);
    EXPECT_NEAR(depthAt(projection, 2000.0), 0.001, 1e-15);
    // Beyond the camera's far plane (1000) is still in front of the far end
    // of the buffer: zooming out cannot clip the scene away.
    EXPECT_GT(depthAt(projection, 1.0e9), 0.0);
}

TEST(ReversedZ, FinitePerspectiveMapsTheFarPlaneToZero)
{
    const Camera camera = perspectiveCamera(1.0, 100.0);
    ProjectionOptions options;
    options.infiniteFar = false;
    const Mat4 projection = reversedZProjection(camera, options);
    // A = n/(f-n) = 1/99, B = nf/(f-n) = 100/99; depth = (B - A d) / d.
    EXPECT_NEAR(depthAt(projection, 1.0), 1.0, 1e-15);
    EXPECT_NEAR(depthAt(projection, 100.0), 0.0, 1e-15);
    // d = 10: (100/99 - 10/99) / 10 = 90/990 = 1/11.
    EXPECT_NEAR(depthAt(projection, 10.0), 1.0 / 11.0, 1e-15);
}

TEST(ReversedZ, OrthographicDepthRunsLinearlyFromOneAtTheNearPlaneToZeroAtTheFar)
{
    Camera camera = perspectiveCamera(10.0, 110.0);
    camera.setProjection(Projection::Orthographic);
    const Mat4 projection = reversedZProjection(camera);
    // depth = (f - d) / (f - n) = (110 - d) / 100.
    EXPECT_NEAR(depthAt(projection, 10.0), 1.0, 1e-15);
    EXPECT_NEAR(depthAt(projection, 60.0), 0.5, 1e-15);
    EXPECT_NEAR(depthAt(projection, 110.0), 0.0, 1e-15);
}

TEST(ReversedZ, AVulkanOrOpenGlClipSpaceFlipsYOrWidensDepthAndNothingElse)
{
    const Camera camera = perspectiveCamera(1.0, 100.0);
    const Mat4 d3d = reversedZProjection(camera);
    ProjectionOptions vulkan;
    vulkan.clip.yUpInNdc = false;
    const Mat4 flipped = reversedZProjection(camera, vulkan);
    ProjectionOptions gl;
    gl.clip.depthZeroToOne = false;
    const Mat4 signedDepth = reversedZProjection(camera, gl);
    for (std::size_t col = 0; col < 4; ++col) {
        EXPECT_EQ(flipped(0, col), d3d(0, col));
        EXPECT_EQ(flipped(1, col), -d3d(1, col));
        EXPECT_EQ(flipped(2, col), d3d(2, col));
        // z' = 2z - w.
        EXPECT_EQ(signedDepth(2, col), 2.0 * d3d(2, col) - d3d(3, col));
    }
}

// The rule the whole GPU path rests on, checked without a GPU: the camera
// matrix built against the scene origin, rounded to float and applied to
// float offsets, puts survey-coordinate points where the software path's
// double arithmetic does; the same thing done with float WORLD coordinates
// does not.
TEST(RelativeViewProjection, PutsSurveyCoordinatePointsWhereTheCameraDoesWhenRoundedToFloat)
{
    constexpr int kWidth = 1000;
    constexpr int kHeight = 800;
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    const Vec3 target(300000.0, 6250000.0, 50.0);
    camera.setTarget(target);
    camera.setDistance(100.0);
    camera.setDepthRange(1.0, 10000.0);
    const Vec3 origin(300010.0, 6250005.0, 48.0);

    // Fractions of a quarter metre, which a float at y = 6.25e6 cannot hold.
    const std::array<Vec3, 4> offsets{Vec3(10.25, -7.25, 3.0), Vec3(-12.75, 4.25, -2.0),
                                      Vec3(0.0, 0.0, 0.0), Vec3(24.75, 25.25, 5.0)};

    const Mat4 relative = relativeViewProjection(camera, origin);
    const auto relativeFloat = toFloatColumnMajor(relative);
    const auto worldFloat = toFloatColumnMajor(camera.viewProjection());

    double worstRelative = 0.0;
    double worstWorld = 0.0;
    for (const Vec3& offset : offsets) {
        const Vec3 world = target + offset;
        const auto expected = camera.project(world); // the software path, in double
        ASSERT_TRUE(expected.has_value());

        // In double the relative matrix is the camera's to a millionth of a pixel.
        const Vec4 clip = relative * Vec4(world - origin, 1.0);
        EXPECT_NEAR((clip.x / clip.w * 0.5 + 0.5) * kWidth, expected->x, 1e-6);
        EXPECT_NEAR((0.5 - clip.y / clip.w * 0.5) * kHeight, expected->y, 1e-6);

        const auto gpu = shade(relativeFloat, relativePosition(world, origin), kWidth, kHeight);
        worstRelative = std::max({worstRelative, std::abs(gpu[0] - expected->x),
                                  std::abs(gpu[1] - expected->y)});
        const std::array<float, 3> raw{static_cast<float>(world.x), static_cast<float>(world.y),
                                       static_cast<float>(world.z)};
        const auto naive = shade(worldFloat, raw, kWidth, kHeight);
        worstWorld = std::max({worstWorld, std::abs(naive[0] - expected->x),
                               std::abs(naive[1] - expected->y)});
    }
    // Relative: offsets under 40 m hold floats 4e-6 m apart and the matrix's
    // translation is 100 m, rounded to 8e-6 m; at 100 m with a 45 degree field
    // over 800 px a metre is 800 / (2 * 100 * tan 22.5) = 9.7 px, so the error
    // is around 1e-4 px. A hundredth of a pixel leaves a wide margin.
    EXPECT_LT(worstRelative, 0.01);
    // World floats: the positions alone are up to 0.25 m out, about 2 px.
    EXPECT_GT(worstWorld, 0.5);
}

// The rule a deep zoom re-packs the GPU's layers by (scene_origin.hpp, "The
// origin follows a deep zoom"), measured: the eye 0.2 m from a point of a
// survey at MGA coordinates, an 800 px tall view, and origins from a metre
// to 3000 km away from the point. A pixel there is 0.2 x 2 tan(22.5 deg) /
// 800 = 2.07e-4 m, so the bound, 10 x 2^-24 of the distance over it, is
// 2.9e-3 px a metre of it. Around the point, what the shader's float
// arithmetic draws stays inside the bound at every distance; past
// kOriginErrorPixels it is out by more than the view can hide; and with the
// origin at the pivot, as the host re-packs it, it is out by nothing to
// speak of.
TEST(RelativeViewProjection, AtTheOriginsErrorBoundThePivotIsDrawnWithinItAndItsOwnOriginDrawsItExactly)
{
    constexpr int kWidth = 1000;
    constexpr int kHeight = 800;
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    const Vec3 target(300050.3, 6200050.7, 31.1);
    camera.setTarget(target);
    camera.setDistance(0.2);
    camera.setDepthRange(1e-4, 1000.0);
    EXPECT_NEAR(camera.worldPerPixel(), 0.2 * 2.0 * 0.41421356237309503 / 800.0, 1e-15);

    // Points round the pivot, as far out as the view reaches at its depth.
    const std::array<Vec3, 5> probes{Vec3(0.0, 0.0, 0.0), Vec3(0.031, -0.012, 0.004),
                                     Vec3(-0.027, 0.019, -0.008), Vec3(0.012, 0.033, 0.0),
                                     Vec3(-0.041, -0.022, 0.011)};
    const auto worstFrom = [&](const Vec3& origin) {
        const auto matrix = toFloatColumnMajor(relativeViewProjection(camera, origin));
        double worst = 0.0;
        for (const Vec3& probe : probes) {
            const Vec3 world = target + probe;
            const auto expected = camera.project(world);
            EXPECT_TRUE(expected.has_value());
            if (!expected) {
                continue;
            }
            const auto gpu = shade(matrix, relativePosition(world, origin), kWidth, kHeight);
            worst = std::max({worst, std::abs(gpu[0] - expected->x),
                              std::abs(gpu[1] - expected->y)});
        }
        return worst;
    };

    // Along a direction no axis favours, so that no coordinate's rounding
    // happens to vanish.
    const Vec3 away = Vec3(0.6, -0.48, 0.64);
    double worstBelowTheRule = 0.0;
    for (const double distance : {1.0, 10.0, 100.0, 1000.0, 1.0e4, 1.0e5, 3.0e6}) {
        const Vec3 origin = target + away * distance;
        const double bound = originErrorPixels(camera, origin);
        EXPECT_NEAR(bound, 10.0 * 0x1.0p-24 * distance / camera.worldPerPixel(),
                    1e-9 * bound);
        const double measured = worstFrom(origin);
        EXPECT_LE(measured, bound) << "origin " << distance << " m from the pivot";
        if (bound <= kOriginErrorPixels) {
            worstBelowTheRule = std::max(worstBelowTheRule, measured);
        }
        RecordProperty("origin_" + std::to_string(static_cast<long long>(distance)) + "_m_px",
                       katana::core::formatExactReal(measured));
    }
    // What the rule leaves alone is drawn right, and 3000 km off - the
    // centre of a survey's box with a stray entity at the origin, where a
    // float steps in 0.125-0.25 m, 600-1200 of these pixels - the points
    // round the pivot are drawn well out.
    EXPECT_LT(worstBelowTheRule, kOriginErrorPixels);
    EXPECT_GT(worstFrom(target + away * 3.0e6), 10.0);
    // Re-packed against the pivot, as the host does past the rule.
    EXPECT_EQ(originErrorPixels(camera, target), 0.0);
    EXPECT_LT(worstFrom(target), 1e-3);
}

TEST(SceneOrigin, TheErrorBoundIsNothingWithoutAViewport)
{
    Camera camera;
    camera.setTarget(Vec3(300000.0, 6200000.0, 0.0));
    EXPECT_EQ(originErrorPixels(camera, Vec3(0.0, 0.0, 0.0)), 0.0);
}
