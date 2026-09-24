// The GPU renderer against the software rasteriser, pixel by pixel, within a
// tolerance (docs/gpu.md, "Testing").
//
// Every case runs twice: on the machine's GPU and on WARP, Windows' software
// Direct3D 11 device, which is on every Windows machine and so is what a GPU-
// less CI runner would still test. A case SKIPS when its device is missing.
//
// A GPU frame is never bit-identical to the CPU's: 4x multisampling blends
// edge pixels, lines get an analytic antialiased fringe, and the two fill rules
// differ at exact pixel centres. So the comparisons are:
//
//   COVERAGE  a pixel is covered when it is at least half as bright as the
//             primitive over it would make it; the images may disagree only
//             along edges, and the tolerance is a count of edge pixels worked
//             out from the shape's perimeter, not a percentage;
//   COLOUR    over pixels whose whole 3x3 neighbourhood is covered in both -
//             where no edge reaches - the colours agree within 2 levels, the
//             most two correctly rounded interpolations can differ by.

#include <gtest/gtest.h>

#include <cmath>
#include <numbers>
#include <memory>
#include <string>
#include <vector>

#include "gpu/scene_origin.hpp"
#include "gpu/shader_compiler.hpp"
#include "gpu/shader_library.hpp"
#include "gpu_test_support.hpp"

using katana::math::Vec3;
using katana::qt::gpu::Expansion;
using katana::qt::gpu::FrameSettings;
using katana::qt::gpu::GpuDevice;
using katana::qt::gpu::GpuSceneData;
using katana::qt::gpu::LightingMode;
using katana::qt::gpu::OffscreenGpu;
using katana::qt::gpu::packDrawList;
using katana::qt::gpu::testing::brightness;
using katana::qt::gpu::testing::compare;
using katana::qt::gpu::testing::cpuRender;
using katana::qt::gpu::testing::describe;
using katana::qt::gpu::testing::Image;
using katana::qt::gpu::testing::makeGpu;
using katana::qt::gpu::testing::saveForLooking;
using katana::render::Camera;
using katana::render::DrawList;
using katana::render::Projection;
using katana::render::Rgba;
using katana::render::rgba;
using katana::render::StandardView;

namespace {

constexpr Rgba kBlack = rgba(0, 0, 0);

// Looking straight down, orthographic, one world unit per pixel, the world
// origin at the image centre: pixel (px, py) is world (px - w/2, h/2 - py).
Camera planCamera(int width, int height)
{
    Camera camera;
    camera.setViewportSize(width, height);
    camera.setProjection(Projection::Orthographic);
    camera.setStandardView(StandardView::Top);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(1000.0);
    camera.setOrthographicHeight(static_cast<double>(height));
    camera.setDepthRange(1.0, 2000.0);
    return camera;
}

FrameSettings blackBackground()
{
    FrameSettings settings;
    settings.background = kBlack;
    return settings;
}

class GpuRender : public ::testing::TestWithParam<GpuDevice> {
  protected:
    // Null (and the reason in skipReason) when this device is not here.
    std::unique_ptr<OffscreenGpu> device(int width, int height)
    {
        return makeGpu(width, height, GetParam(), skipReason);
    }
    std::string label(const std::string& what) const
    {
        return what + "_" + katana::qt::gpu::toString(GetParam());
    }
    Image render(OffscreenGpu& gpu, const Camera& camera, const FrameSettings& settings)
    {
        Image pixels;
        auto stats = gpu.renderFrame(camera, settings, &pixels);
        EXPECT_TRUE(stats.ok()) << (stats.ok() ? "" : stats.error().describe());
        return pixels;
    }
    std::string skipReason;
};

// A 40 m square of ground, 8 x 8 cells, rolling a few metres, coloured by
// height: a small TIN, with its outline as lines. `offset` moves it.
DrawList site(const Vec3& offset)
{
    DrawList list;
    constexpr int kCells = 8;
    constexpr double kSize = 40.0;
    const double step = kSize / kCells;
    for (int j = 0; j <= kCells; ++j) {
        for (int i = 0; i <= kCells; ++i) {
            const double x = -kSize * 0.5 + step * i;
            const double y = -kSize * 0.5 + step * j;
            const double z = 3.0 * std::sin(x * 0.15) * std::cos(y * 0.11);
            const auto shade = static_cast<std::uint8_t>(120 + 40.0 * z / 3.0);
            list.addVertex(offset + Vec3(x, y, z), rgba(shade, 200, static_cast<std::uint8_t>(255 - shade)));
        }
    }
    const auto at = [](int i, int j) { return static_cast<std::uint32_t>(j * (kCells + 1) + i); };
    for (int j = 0; j < kCells; ++j) {
        for (int i = 0; i < kCells; ++i) {
            list.addTriangle(at(i, j), at(i + 1, j), at(i + 1, j + 1));
            list.addTriangle(at(i, j), at(i + 1, j + 1), at(i, j + 1));
        }
    }
    const Rgba white = rgba(255, 255, 255);
    list.addSegment(offset + Vec3(-20.0, -20.0, 6.0), offset + Vec3(20.0, -20.0, 6.0), white);
    list.addSegment(offset + Vec3(20.0, -20.0, 6.0), offset + Vec3(20.0, 20.0, 6.0), white, 2.0f);
    return list;
}

Camera siteCamera(const Vec3& offset, int width, int height)
{
    Camera camera;
    camera.setViewportSize(width, height);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::IsoSouthWest);
    camera.setTarget(offset);
    camera.setDistance(75.0);
    camera.setDepthRange(0.5, 1000.0);
    return camera;
}

} // namespace

INSTANTIATE_TEST_SUITE_P(Devices, GpuRender,
                         ::testing::Values(GpuDevice::Hardware, GpuDevice::Warp),
                         [](const ::testing::TestParamInfo<GpuDevice>& param) {
                             return std::string(param.param == GpuDevice::Hardware ? "Hardware"
                                                                                   : "Warp");
                         });

TEST_P(GpuRender, AShadedTriangleCoversTheSoftwarePathsPixelsInItsColours)
{
    constexpr int kWidth = 96;
    constexpr int kHeight = 64;
    auto gpu = device(kWidth, kHeight);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    // Red, green and blue corners: every interpolated colour has r + g + b =
    // 255, so "covered" is simply brightness >= 128.
    DrawList list;
    const auto a = list.addVertex(Vec3(-30.0, -20.0, 0.0), rgba(255, 0, 0));
    const auto b = list.addVertex(Vec3(30.0, -20.0, 0.0), rgba(0, 255, 0));
    const auto c = list.addVertex(Vec3(0.0, 25.0, 0.0), rgba(0, 0, 255));
    list.addTriangle(a, b, c);
    const Camera camera = planCamera(kWidth, kHeight);

    gpu->renderer().setDrawList(list);
    const Image gpuImage = render(*gpu, camera, blackBackground());
    const Image cpuImage = cpuRender(list, camera, kBlack);
    saveForLooking(label("triangle_gpu"), gpuImage, kWidth, kHeight);
    saveForLooking("triangle_cpu", cpuImage, kWidth, kHeight);

    const auto result = compare(cpuImage, gpuImage, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(result));
    // Area: base 60 x height 45 / 2 = 1350 px.
    EXPECT_NEAR(static_cast<double>(result.coveredA), 1350.0, 60.0);
    // Perimeter: 60 + 2 * sqrt(30^2 + 45^2) = 60 + 2 * 54.08 = 168 px. The two
    // can only disagree on a pixel whose centre is within a quarter pixel of
    // an edge (closer to it than two of the four samples), a band half a
    // pixel wide: at most 168 / 2 = 84 pixels.
    EXPECT_LE(result.coverageMismatches, 84u);
    EXPECT_GT(result.interiorPixels, 900u);
    EXPECT_LE(result.maxInteriorDifference, 2);
}

TEST_P(GpuRender, OverlappingSurfacesInPerspectiveHideEachOtherAsTheSoftwarePathDoes)
{
    constexpr int kWidth = 128;
    constexpr int kHeight = 96;
    auto gpu = device(kWidth, kHeight);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    // A red ground square and a green square 2 m above it, half overlapping,
    // and a blue wall standing in front of both.
    DrawList list;
    const auto quad = [&list](Vec3 p0, Vec3 p1, Vec3 p2, Vec3 p3, Rgba color) {
        const auto a = list.addVertex(p0, color);
        const auto b = list.addVertex(p1, color);
        const auto c = list.addVertex(p2, color);
        const auto d = list.addVertex(p3, color);
        list.addTriangle(a, b, c);
        list.addTriangle(a, c, d);
    };
    quad(Vec3(-20, -20, 0), Vec3(20, -20, 0), Vec3(20, 20, 0), Vec3(-20, 20, 0), rgba(255, 0, 0));
    quad(Vec3(0, -10, 2), Vec3(30, -10, 2), Vec3(30, 30, 2), Vec3(0, 30, 2), rgba(0, 255, 0));
    quad(Vec3(-15, -25, 0), Vec3(5, -25, 0), Vec3(5, -25, 12), Vec3(-15, -25, 12),
         rgba(0, 0, 255));
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::IsoSouthWest);
    ASSERT_TRUE(camera.frame(list.bounds()));

    gpu->renderer().setDrawList(list);
    // The software path clips at the camera's far plane; so must the GPU here.
    FrameSettings settings = blackBackground();
    settings.infiniteFar = false;
    const Image gpuImage = render(*gpu, camera, settings);
    const Image cpuImage = cpuRender(list, camera, kBlack);
    saveForLooking(label("overlap_gpu"), gpuImage, kWidth, kHeight);
    saveForLooking("overlap_cpu", cpuImage, kWidth, kHeight);

    const auto result = compare(cpuImage, gpuImage, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(result));
    // Framed by its bounding sphere (radius half of sqrt(50^2 + 55^2 + 12^2)
    // = 37.6 m, 6% margin) the view is 80 m high over 96 px: 1.2 px a metre.
    // Seen 35 degrees down, ground shrinks by sin 35.26 = 0.577: the red
    // square's 1600 m2 is 1330 px, the green's half off it 500 px more, and
    // the wall adds a little: about 1900 px drawn.
    EXPECT_NEAR(static_cast<double>(result.coveredA), 1900.0, 250.0);
    // Disagreement only where an outline or the green square's edge over the
    // red crosses a pixel: the three squares' outlines on screen are each
    // under 4 x 60 px, so under 720 px of edge, and at most half of those
    // pixels (the quarter-pixel band above) can flip either way.
    EXPECT_LE(result.coverageMismatches, 360u);
    EXPECT_LE(result.differingPixels, 360u);
    EXPECT_LE(result.maxInteriorDifference, 2);
}

TEST_P(GpuRender, LinesAndPointsCoverTheSoftwarePathsPixelsWithinTheirAntialiasedEdges)
{
    constexpr int kWidth = 96;
    constexpr int kHeight = 64;
    auto gpu = device(kWidth, kHeight);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    // Pixel centres are at half-integers in the plan camera, so a line along
    // y = 0.5 runs exactly along pixel row 31's centres.
    const Rgba white = rgba(255, 255, 255);
    DrawList list;
    list.addSegment(Vec3(-40.0, 0.5, 0.0), Vec3(40.0, 0.5, 0.0), white);          // 1 px, 81 long
    list.addSegment(Vec3(-20.5, -25.0, 0.0), Vec3(-20.5, 25.0, 0.0), white, 3.0f); // 3 px, 51 long
    list.addSegment(Vec3(0.0, -25.0, 0.0), Vec3(40.0, 15.0, 0.0), white);          // diagonal
    list.addPoint(list.addVertex(Vec3(30.5, -20.5, 0.0), white), 5.0f);
    const Camera camera = planCamera(kWidth, kHeight);

    gpu->renderer().setDrawList(list);
    const Image gpuImage = render(*gpu, camera, blackBackground());
    const Image cpuImage = cpuRender(list, camera, kBlack);
    saveForLooking(label("lines_gpu"), gpuImage, kWidth, kHeight);
    saveForLooking("lines_cpu", cpuImage, kWidth, kHeight);

    // Half of white's 765.
    const auto result = compare(cpuImage, gpuImage, kWidth, kHeight, kBlack, 383);
    SCOPED_TRACE(describe(result));
    // Axis-aligned lines on pixel centres have coverage 1 on their pixels and
    // 0 beside them, so they agree exactly. The diagonal is 40 * sqrt(2) = 57
    // px long, and a 1 px line at 45 degrees leaves each pixel it crosses at
    // up to half coverage: allow all of its 57 edge-hugging pixels on each
    // side to flip, 114, plus the point's 20 px outline.
    EXPECT_LE(result.coverageMismatches, 134u);
    // The lines drawn: the 1 px line's 81 + 2 caps, the 3 px line's 3 x 51
    // plus its caps: something is drawn on both.
    EXPECT_GT(result.coveredB, 250u);
    // The horizontal line's middle pixel is fully covered on both.
    const std::size_t middle = 31u * kWidth + 60u;
    EXPECT_EQ(cpuImage[middle], white);
    EXPECT_GE(brightness(gpuImage[middle]), 760);
}

TEST_P(GpuRender, ASceneAtMgaCoordinatesDrawsLikeTheSameSceneAtTheOrigin)
{
    constexpr int kWidth = 160;
    constexpr int kHeight = 120;
    auto gpu = device(kWidth, kHeight);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    const Vec3 home(0.0, 0.0, 0.0);
    const Vec3 survey(300000.0, 6250000.0, 50.0);

    gpu->renderer().setDrawList(site(home));
    const Image atHome = render(*gpu, siteCamera(home, kWidth, kHeight), blackBackground());
    gpu->renderer().setDrawList(site(survey));
    const Image atSurvey = render(*gpu, siteCamera(survey, kWidth, kHeight), blackBackground());
    saveForLooking(label("rtc_home"), atHome, kWidth, kHeight);
    saveForLooking(label("rtc_survey"), atSurvey, kWidth, kHeight);

    const auto same = compare(atHome, atSurvey, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(same));
    EXPECT_GT(same.coveredA, 4000u);
    // Relative to their own centres the two lists' positions are equal to
    // within a double's rounding at 6.25e6 (1e-9 m), far below a float's
    // resolution at 20 m (2e-6 m), so the GPU sees the same floats, and the
    // eye-relative matrices agree as closely. At most a pixel or two on an
    // edge may flip where a coordinate lands on a rounding boundary.
    EXPECT_LE(same.differingPixels, 4u);
    EXPECT_LE(same.coverageMismatches, 4u);

    // The control: the survey scene packed against a zero origin, as a GPU
    // path without the rule would upload it, is visibly wrong - its vertices
    // snap to the half-metre float lattice at northing 6.25e6.
    GpuSceneData raw;
    packDrawList(site(survey), Vec3(0.0, 0.0, 0.0), raw);
    gpu->renderer().setScene(std::move(raw));
    const Image withoutOrigin =
        render(*gpu, siteCamera(survey, kWidth, kHeight), blackBackground());
    saveForLooking(label("rtc_without_origin"), withoutOrigin, kWidth, kHeight);
    const auto broken = compare(atHome, withoutOrigin, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(broken));
    EXPECT_GT(broken.differingPixels, same.coveredA / 20) << "the control should differ widely";
}

TEST_P(GpuRender, AFrameThatOnlyMovesTheCameraUploadsNothing)
{
    auto gpu = device(64, 48);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    gpu->renderer().setDrawList(site(Vec3()));
    Camera camera = siteCamera(Vec3(), 64, 48);
    auto first = gpu->renderFrame(camera, blackBackground());
    ASSERT_TRUE(first.ok());
    EXPECT_TRUE(first->uploaded);
    EXPECT_EQ(gpu->renderer().uploadCount(), 1u);

    camera.orbit(0.3, 0.1);
    auto second = gpu->renderFrame(camera, blackBackground());
    ASSERT_TRUE(second.ok());
    EXPECT_FALSE(second->uploaded);
    EXPECT_EQ(second->uploadedBytes, 0u);
    EXPECT_EQ(gpu->renderer().uploadCount(), 1u);

    gpu->renderer().setDrawList(site(Vec3(1.0, 0.0, 0.0)));
    auto third = gpu->renderFrame(camera, blackBackground());
    ASSERT_TRUE(third.ok());
    EXPECT_TRUE(third->uploaded);
    EXPECT_EQ(gpu->renderer().uploadCount(), 2u);
}

TEST_P(GpuRender, VerticalExaggerationInTheShaderDrawsWhatAnExaggeratedListDraws)
{
    constexpr int kWidth = 160;
    constexpr int kHeight = 120;
    auto gpu = device(kWidth, kHeight);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    constexpr double kFactor = 3.0;
    constexpr double kDatum = -1.0;
    const DrawList flat = site(Vec3());
    DrawList lifted = flat;
    for (auto& p : lifted.positions) {
        p.z = kDatum + (p.z - kDatum) * kFactor;
    }
    const Camera camera = siteCamera(Vec3(), kWidth, kHeight);

    gpu->renderer().setDrawList(lifted);
    const Image expected = render(*gpu, camera, blackBackground());
    gpu->renderer().setDrawList(flat);
    FrameSettings settings = blackBackground();
    settings.verticalExaggeration = kFactor;
    settings.exaggerationDatum = kDatum;
    const Image exaggerated = render(*gpu, camera, settings);
    saveForLooking(label("exaggeration"), exaggerated, kWidth, kHeight);

    const auto result = compare(expected, exaggerated, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(result));
    EXPECT_GT(result.coveredA, 4000u);
    // The two lists are packed against different origins (the lifted one's
    // box is taller), so the floats differ in their last bits: a handful of
    // edge pixels may flip, nothing more.
    EXPECT_LE(result.coverageMismatches, 16u);
    EXPECT_LE(result.maxInteriorDifference, 2);
}

TEST_P(GpuRender, PerPixelLightingDarkensTheSideOfAFaceTurnedFromTheLight)
{
    constexpr int kSize = 48;
    auto gpu = device(kSize, kSize);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    DrawList list;
    const Rgba grey = rgba(200, 200, 200);
    const auto a = list.addVertex(Vec3(-20.0, -20.0, 0.0), grey);
    const auto b = list.addVertex(Vec3(20.0, -20.0, 0.0), grey);
    const auto c = list.addVertex(Vec3(20.0, 20.0, 0.0), grey);
    const auto d = list.addVertex(Vec3(-20.0, 20.0, 0.0), grey);
    list.addTriangle(a, b, c);
    list.addTriangle(a, c, d);
    gpu->renderer().setDrawList(list);

    FrameSettings settings = blackBackground();
    settings.lighting = LightingMode::PerPixel;
    // 60 degrees from the zenith, so n.l = cos 60 = 0.5 for the upper side.
    settings.lightDirection = Vec3(std::sin(std::numbers::pi / 3.0), 0.0, std::cos(std::numbers::pi / 3.0));
    settings.ambient = 0.35;

    Camera above = planCamera(kSize, kSize);
    const Image fromAbove = render(*gpu, above, settings);
    Camera below = above;
    below.setStandardView(StandardView::Bottom);
    const Image fromBelow = render(*gpu, below, settings);

    const std::size_t centre = static_cast<std::size_t>(kSize / 2) * kSize + kSize / 2;
    // Above: 0.35 + 0.65 * 0.5 = 0.675 of 200 = 135.
    EXPECT_NEAR(katana::render::redOf(fromAbove[centre]), 135, 2);
    // Below, the visible side faces away from the light: n.l = -0.5, clamped
    // to 0, ambient alone: 0.35 of 200 = 70. (A baked abs(n.l) would give
    // 135 again - both sides lit alike.)
    EXPECT_NEAR(katana::render::redOf(fromBelow[centre]), 70, 2);
}

TEST_P(GpuRender, ALineOnTheGroundBehindAWallStaysHidden)
{
    constexpr int kWidth = 128;
    constexpr int kHeight = 96;
    auto gpu = device(kWidth, kHeight);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    // A 40 x 25 m red wall facing south, and 5 m north of it a white line on
    // the ground with the scene builder's entity bias, which in the software
    // path's standard-Z buffer is enough to pull it through the wall.
    DrawList list;
    const Rgba red = rgba(255, 0, 0);
    const auto a = list.addVertex(Vec3(-20.0, 0.0, 0.0), red);
    const auto b = list.addVertex(Vec3(20.0, 0.0, 0.0), red);
    const auto c = list.addVertex(Vec3(20.0, 0.0, 25.0), red);
    const auto d = list.addVertex(Vec3(-20.0, 0.0, 25.0), red);
    list.addTriangle(a, b, c);
    list.addTriangle(a, c, d);
    list.addSegment(Vec3(-10.0, 5.0, 0.0), Vec3(10.0, 5.0, 0.0), rgba(255, 255, 255), 1.0f,
                    2.0e-4f);
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Perspective);
    camera.setOrientation(-std::numbers::pi / 2.0, 0.2); // from the south, a little above
    ASSERT_TRUE(camera.frame(list.bounds()));

    // Anything with green in it has the line in it: the wall is pure red and
    // the ground black.
    const auto lineOn = [](const Image& image) {
        std::size_t count = 0;
        for (const Rgba pixel : image) {
            count += katana::render::greenOf(pixel) > 8 ? 1 : 0;
        }
        return count;
    };
    gpu->renderer().setDrawList(list);
    const Image image = render(*gpu, camera, blackBackground());
    saveForLooking(label("xray"), image, kWidth, kHeight);
    EXPECT_EQ(lineOn(image), 0u);

    // The control: without the wall the line is there to be seen. The
    // camera is unchanged, so it is the wall, not the framing, that hid it.
    DrawList lineOnly = list;
    lineOnly.triangles.clear();
    gpu->renderer().setDrawList(lineOnly);
    EXPECT_GT(lineOn(render(*gpu, camera, blackBackground())), 10u);
}

TEST_P(GpuRender, ZoomingFarOutNeverClipsTheSceneAway)
{
    constexpr int kSize = 64;
    auto gpu = device(kSize, kSize);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    const DrawList list = site(Vec3());
    Camera camera;
    camera.setViewportSize(kSize, kSize);
    camera.setStandardView(StandardView::IsoSouthWest);
    ASSERT_TRUE(camera.frame(list.bounds()));
    // Past the far plane frame() chose (at most 8 radii beyond the target):
    // 20 notches of the wheel out is 1.15^20 = 16x the distance.
    camera.dolly(std::pow(1.15, 20.0));
    ASSERT_GT(camera.distance(), camera.farPlane());
    gpu->renderer().setDrawList(list);
    const Image image = render(*gpu, camera, blackBackground());
    std::size_t drawn = 0;
    for (const Rgba pixel : image) {
        drawn += brightness(pixel) > 0 ? 1 : 0;
    }
    EXPECT_GT(drawn, 0u);
}

// The two ways of widening lines and points (shader_library.hpp, Expansion)
// run the same HLSL functions on the same inputs - once in a geometry shader,
// once per corner in a vertex shader - so they must draw the same picture.
TEST_P(GpuRender, TheGeometryShaderAndInstancingDrawTheSameLinesPointsAndCloud)
{
    constexpr int kWidth = 96;
    constexpr int kHeight = 64;
    katana::qt::gpu::OffscreenOptions instancedOptions;
    instancedOptions.expansion = Expansion::Instanced;
    auto geometry = device(kWidth, kHeight);
    if (!geometry) {
        GTEST_SKIP() << skipReason;
    }
    auto instanced = makeGpu(kWidth, kHeight, GetParam(), skipReason, instancedOptions);
    ASSERT_NE(instanced, nullptr) << skipReason;
    // Every Direct3D 11 device has a geometry stage (feature level 10 and up),
    // so the preferred way is the one used.
    EXPECT_EQ(geometry->renderer().expansion(), Expansion::GeometryShader);
    EXPECT_EQ(instanced->renderer().expansion(), Expansion::Instanced);

    DrawList list;
    list.addSegment(Vec3(-40.0, 0.5, 0.0), Vec3(40.0, 0.5, 0.0), rgba(255, 255, 255));
    list.addSegment(Vec3(-20.5, -25.0, 0.0), Vec3(-20.5, 25.0, 0.0), rgba(255, 200, 0), 3.0f);
    list.addSegment(Vec3(0.0, -25.0, 0.0), Vec3(40.0, 15.0, 0.0), rgba(0, 200, 255));
    list.addPoint(list.addVertex(Vec3(30.5, -20.5, 0.0), rgba(255, 0, 255)), 5.0f);
    std::vector<Vec3> cloud;
    for (int i = 0; i < 12; ++i) {
        cloud.emplace_back(-35.0 + 6.0 * i, 20.0, 0.0);
    }
    katana::qt::gpu::PointCloudData packed;
    katana::qt::gpu::packPointCloud(cloud, {}, rgba(120, 255, 120), Vec3(), 0, packed);
    const Camera camera = planCamera(kWidth, kHeight);
    FrameSettings settings = blackBackground();
    settings.cloudPointSize = 4.0;

    geometry->renderer().setDrawList(list);
    geometry->renderer().setPointCloud(packed);
    instanced->renderer().setDrawList(list);
    instanced->renderer().setPointCloud(packed);
    const Image fromGeometry = render(*geometry, camera, settings);
    const Image fromInstances = render(*instanced, camera, settings);
    saveForLooking(label("expansion_geometry"), fromGeometry, kWidth, kHeight);
    saveForLooking(label("expansion_instanced"), fromInstances, kWidth, kHeight);

    const auto result = compare(fromGeometry, fromInstances, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(result));
    EXPECT_GT(result.coveredA, 250u);
    // The compiler may order the same arithmetic differently in the two
    // stages, moving a corner by a float's last bit; that can flip only a
    // pixel whose centre lies exactly on a quad's edge, and in this picture
    // only the diagonal's two sides put centres there - a handful at most.
    EXPECT_LE(result.differingPixels, 4u);
}

// A cloud point is a round sprite of FrameSettings::cloudPointSize logical
// pixels whatever its distance, drawn relative to the cloud's own origin.
TEST_P(GpuRender, CloudPointsAreRoundSpritesOfTheirScreenSizeAtSurveyCoordinates)
{
    constexpr int kWidth = 64;
    constexpr int kHeight = 48;
    auto gpu = device(kWidth, kHeight);
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    // Three points in a row at MGA coordinates, 16 pixels apart in the plan
    // camera moved there, each on a pixel centre (the camera's half-integer
    // grid).
    const Vec3 survey(300000.0, 6250000.0, 50.0);
    std::vector<Vec3> points{survey + Vec3(-15.5, 0.5, 0.0), survey + Vec3(0.5, 0.5, 0.0),
                             survey + Vec3(16.5, 0.5, 0.0)};
    katana::qt::gpu::PointCloudData cloud;
    katana::qt::gpu::packPointCloud(points, {}, rgba(255, 255, 255), survey, 0, cloud);
    gpu->renderer().setPointCloud(cloud);
    Camera camera = planCamera(kWidth, kHeight);
    camera.setTarget(survey);
    FrameSettings settings = blackBackground();
    settings.cloudPointSize = 6.0;
    const Image image = render(*gpu, camera, settings);
    saveForLooking(label("cloud"), image, kWidth, kHeight);

    // A disc of radius 3 around each point's pixel. Its coverage fades over
    // the pixel either side of that radius, so at least half coverage means
    // a centre within 3 px: pi * 3^2 = 28.3 px of area, which a pixel grid
    // fills with between the 21 whole pixels inside radius 2.6 and the 37
    // inside radius 3.4 (counted by hand on the grid).
    const auto discAt = [&](int cx, int cy) {
        std::size_t count = 0;
        for (int y = cy - 5; y <= cy + 5; ++y) {
            for (int x = cx - 5; x <= cx + 5; ++x) {
                count += brightness(image[static_cast<std::size_t>(y) * kWidth + x]) >= 383 ? 1 : 0;
            }
        }
        return count;
    };
    // World (x, 0.5) is pixel column x + 32 - 0.5, row 23.
    for (const int column : {16, 32, 48}) {
        SCOPED_TRACE(column);
        EXPECT_GE(brightness(image[23u * kWidth + static_cast<std::size_t>(column)]), 760);
        const std::size_t disc = discAt(column, 23);
        EXPECT_GE(disc, 21u);
        EXPECT_LE(disc, 37u);
    }
}

// The seam precompiled shaders will use: the same shaders serialized (as qsb
// would write them) and read back through SerializedShaderLibrary draw the
// same frame as the runtime library.
TEST_P(GpuRender, SerializedShadersDrawWhatTheRuntimeShadersDraw)
{
    constexpr int kWidth = 96;
    constexpr int kHeight = 72;
    auto runtime = device(kWidth, kHeight);
    if (!runtime) {
        GTEST_SKIP() << skipReason;
    }
    // The default library's shaders - bytecode, as a .qsb for Direct3D holds.
    auto table = katana::qt::gpu::SerializedShaderLibrary::serialize(
        katana::qt::gpu::compiledHlslShaders());
    ASSERT_TRUE(table.ok()) << table.error().describe();
    const katana::qt::gpu::SerializedShaderLibrary serialized(std::move(*table));
    katana::qt::gpu::OffscreenOptions options;
    options.shaders = &serialized;
    auto precompiled = makeGpu(kWidth, kHeight, GetParam(), skipReason, options);
    ASSERT_NE(precompiled, nullptr) << skipReason;

    const DrawList list = site(Vec3());
    const Camera camera = siteCamera(Vec3(), kWidth, kHeight);
    runtime->renderer().setDrawList(list);
    precompiled->renderer().setDrawList(list);
    const Image a = render(*runtime, camera, blackBackground());
    const Image b = render(*precompiled, camera, blackBackground());
    const auto result = compare(a, b, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(result));
    EXPECT_GT(result.coveredA, 1000u);
    // The same source compiled the same way on the same device.
    EXPECT_EQ(result.differingPixels, 0u);
}

// A blob that is not a serialized shader is an error when it is asked for,
// never an empty pipeline.
TEST(SerializedShaderLibrary, ReportsABlobThatDoesNotDeserialize)
{
    katana::qt::gpu::SerializedShaderLibrary::BlobTable table;
    table[0][0].vertex = QByteArrayLiteral("not a shader");
    const katana::qt::gpu::SerializedShaderLibrary library(std::move(table));
    auto stages = library.program(katana::qt::gpu::Program::Triangles, Expansion::GeometryShader);
    ASSERT_FALSE(stages.ok());
    EXPECT_NE(stages.error().describe().find("do not deserialize"), std::string::npos);
}

// The default library compiles the HLSL itself (shader_compiler.hpp); QRhi
// compiling the same source must draw the same frame.
TEST_P(GpuRender, BytecodeCompiledHereDrawsWhatQRhiCompilingTheSourceDraws)
{
    constexpr int kWidth = 96;
    constexpr int kHeight = 72;
    auto compiled = device(kWidth, kHeight);
    if (!compiled) {
        GTEST_SKIP() << skipReason;
    }
    katana::qt::gpu::OffscreenOptions options;
    options.shaders = &katana::qt::gpu::runtimeHlslShaders();
    auto fromSource = makeGpu(kWidth, kHeight, GetParam(), skipReason, options);
    ASSERT_NE(fromSource, nullptr) << skipReason;

    const DrawList list = site(Vec3());
    const Camera camera = siteCamera(Vec3(), kWidth, kHeight);
    compiled->renderer().setDrawList(list);
    fromSource->renderer().setDrawList(list);
    const Image a = render(*compiled, camera, blackBackground());
    const Image b = render(*fromSource, camera, blackBackground());
    const auto result = compare(a, b, kWidth, kHeight, kBlack, 128);
    SCOPED_TRACE(describe(result));
    EXPECT_GT(result.coveredA, 1000u);
    // The same source; if the two compilers' flags differ, the same arithmetic
    // may be ordered differently and move a vertex by a float's last bit,
    // which flips only a pixel whose centre lies exactly on an edge.
    EXPECT_LE(result.differingPixels, 4u);
}

TEST(ShaderCompiler, EveryStageOfBothExpansionsCompiles)
{
    for (const Expansion expansion :
         {Expansion::GeometryShader, Expansion::Instanced}) {
        auto status = katana::qt::gpu::precompileHlslShaders(expansion);
        EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    }
}

TEST(ShaderCompiler, ReportsACompileErrorWithTheCompilersOwnMessage)
{
    auto result = katana::qt::gpu::compileHlsl(
        "float4 main() : SV_Target { return undefinedColour; }", QShader::FragmentStage);
    ASSERT_FALSE(result.ok());
    const std::string message = result.error().describe();
    EXPECT_NE(message.find("did not compile"), std::string::npos) << message;
    EXPECT_NE(message.find("undefinedColour"), std::string::npos) << message;
}
