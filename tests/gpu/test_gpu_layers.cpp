// A scene of layers on the GPU (GpuRenderer::setLayers): each drawn whole, in
// order, into one depth buffer, with its own depth rule - the GPU twin of
// cad::renderLayers, which the 3D view draws its grid, terrain, edges,
// drawing and selection with (scene.hpp has why the grid and the edges write
// no depth). Every case draws the scene both ways - the layer writing depth
// and not - so it shows the rule is what makes the difference.

#include <gtest/gtest.h>

#include <array>
#include <memory>
#include <span>
#include <string>

#include "gpu/gpu_renderer.hpp"
#include "gpu/offscreen_gpu.hpp"
#include "gpu/scene_origin.hpp"
#include "gpu_test_support.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"

using katana::math::Vec3;
using katana::qt::gpu::FrameSettings;
using katana::qt::gpu::GpuDevice;
using katana::qt::gpu::kOriginErrorPixels;
using katana::qt::gpu::LayerSource;
using katana::qt::gpu::OffscreenGpu;
using katana::qt::gpu::originErrorPixels;
using katana::qt::gpu::testing::brightness;
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

constexpr int kWidth = 64;
constexpr int kHeight = 48;

// Straight down, orthographic, one world unit per pixel: the centre of pixel
// (column, row) is world (column + 0.5 - 32, 23.5 - row), so (32, 23) is
// world (0.5, 0.5).
Camera planCamera()
{
    Camera camera;
    camera.setViewportSize(kWidth, kHeight);
    camera.setProjection(Projection::Orthographic);
    camera.setStandardView(StandardView::Top);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(1000.0);
    camera.setOrthographicHeight(static_cast<double>(kHeight));
    camera.setDepthRange(1.0, 2000.0);
    return camera;
}

// A flat green triangle at z = 0 over the middle of the view: a pad, a pond
// floor - a surface exactly in the plane of whatever else lies at z = 0.
DrawList flatSurface()
{
    DrawList list;
    const Rgba green = rgba(0, 200, 0);
    list.addTriangle(list.addVertex(Vec3(-20.0, -15.0, 0.0), green),
                     list.addVertex(Vec3(20.0, -15.0, 0.0), green),
                     list.addVertex(Vec3(0.0, 20.0, 0.0), green));
    return list;
}

DrawList lineAt(const Vec3& a, const Vec3& b, Rgba color, float width)
{
    DrawList list;
    list.addSegment(a, b, color, width);
    return list;
}

Rgba pixelAt(const Image& image, int column, int row)
{
    return image[static_cast<std::size_t>(row) * kWidth + static_cast<std::size_t>(column)];
}

class GpuLayers : public ::testing::TestWithParam<GpuDevice> {
  protected:
    std::unique_ptr<OffscreenGpu> device()
    {
        return makeGpu(kWidth, kHeight, GetParam(), skipReason);
    }
    Image render(OffscreenGpu& gpu, std::span<const LayerSource> layers)
    {
        gpu.renderer().setLayers(layers);
        FrameSettings settings;
        settings.background = rgba(0, 0, 0);
        Image pixels;
        auto stats = gpu.renderFrame(planCamera(), settings, &pixels);
        EXPECT_TRUE(stats.ok()) << (stats.ok() ? "" : stats.error().describe());
        return pixels;
    }
    std::string label(const std::string& what) const
    {
        return what + "_" + katana::qt::gpu::toString(GetParam());
    }
    std::string skipReason;
};

} // namespace

INSTANTIATE_TEST_SUITE_P(Devices, GpuLayers,
                         ::testing::Values(GpuDevice::Hardware, GpuDevice::Software),
                         [](const ::testing::TestParamInfo<GpuDevice>& param) {
                             return std::string(param.param == GpuDevice::Hardware ? "Hardware"
                                                                                   : "Software");
                         });

// The grid is a backdrop: drawn first and writing no depth, it is covered by
// a surface lying exactly in its plane. Writing depth, the grid won the tie -
// the flat surface's own polygon offset pushes it back - and showed through
// the pad (docs/render.md, "The grid drawn across a flat pad").
TEST_P(GpuLayers, AGridThatWritesNoDepthStaysUnderASurfaceInItsPlane)
{
    auto gpu = device();
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    const DrawList grid =
        lineAt(Vec3(-30.5, 0.5, 0.0), Vec3(30.5, 0.5, 0.0), rgba(255, 255, 255), 1.0f);
    const DrawList surface = flatSurface();

    const std::array<LayerSource, 2> backdrop{LayerSource{&grid, false},
                                              LayerSource{&surface, true}};
    const Image drawn = render(*gpu, backdrop);
    saveForLooking(label("layers_grid_backdrop"), drawn, kWidth, kHeight);
    // Under the surface: the surface's green, not the grid's white.
    const Rgba under = pixelAt(drawn, 32, 23);
    EXPECT_GT(katana::render::greenOf(under), 150);
    EXPECT_LT(katana::render::redOf(under), 60);
    // Beside it the grid is there: nothing covers it.
    EXPECT_GT(katana::qt::gpu::testing::brightness(pixelAt(drawn, 4, 23)), 600);

    // The control: the same grid writing depth shows through.
    const std::array<LayerSource, 2> written{LayerSource{&grid, true},
                                             LayerSource{&surface, true}};
    const Image control = render(*gpu, written);
    EXPECT_GT(katana::render::redOf(pixelAt(control, 32, 23)), 150)
        << "without the rule the grid is expected to win the tie; if it no longer does, this "
           "case no longer tests the rule";
}

// A surface's edges are tested against it and write no depth, so a line of
// the drawing lying in the same surface draws whole across them. Writing
// depth, the edge drawn first won every tie, and a draped string was dashed
// wherever a TIN edge crossed it (docs/render.md). Both lines are 1 px, the
// drawing's own width (SceneOptions::entityLineWidth): a wider mark is pulled
// towards the eye by its size (gpu_renderer.cpp, kMarkPull) and wins
// whatever the edges write, so it would not test the rule.
TEST_P(GpuLayers, EdgesThatWriteNoDepthDoNotBreakALineDrawnOverThem)
{
    auto gpu = device();
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    const DrawList surface = flatSurface();
    const DrawList edge =
        lineAt(Vec3(0.5, -10.5, 0.0), Vec3(0.5, 10.5, 0.0), rgba(90, 90, 90), 1.0f);
    const DrawList drawing =
        lineAt(Vec3(-10.5, 0.5, 0.0), Vec3(10.5, 0.5, 0.0), rgba(255, 0, 0), 1.0f);

    const std::array<LayerSource, 3> layered{LayerSource{&surface, true},
                                             LayerSource{&edge, false},
                                             LayerSource{&drawing, true}};
    const Image drawn = render(*gpu, layered);
    saveForLooking(label("layers_edges"), drawn, kWidth, kHeight);
    // Where the two cross, the drawing's red.
    const Rgba crossing = pixelAt(drawn, 32, 23);
    EXPECT_GT(katana::render::redOf(crossing), 200);
    EXPECT_LT(katana::render::greenOf(crossing), 60);
    // Along the edge away from the line, the edge is drawn over the surface.
    const Rgba along = pixelAt(drawn, 32, 13);
    EXPECT_LT(katana::render::greenOf(along), 150) << "the edge covers the surface there";

    const std::array<LayerSource, 3> written{LayerSource{&surface, true},
                                             LayerSource{&edge, true},
                                             LayerSource{&drawing, true}};
    const Image control = render(*gpu, written);
    EXPECT_LT(katana::render::redOf(pixelAt(control, 32, 23)), 200)
        << "without the rule the edge is expected to win the tie; if it no longer does, this "
           "case no longer tests the rule";
}

// One layer changing alone - a selection, edges recoloured by their fade -
// is uploaded alone, against the origin the scene was packed with, and keeps
// its depth rule.
TEST_P(GpuLayers, ALayerUpdatedAloneIsTheOnlyOneUploaded)
{
    auto gpu = device();
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    const DrawList surface = flatSurface();
    const DrawList grid =
        lineAt(Vec3(-30.5, 0.5, 0.0), Vec3(30.5, 0.5, 0.0), rgba(255, 255, 255), 1.0f);
    const std::array<LayerSource, 2> layers{LayerSource{&grid, false},
                                            LayerSource{&surface, true}};
    gpu->renderer().setLayers(layers);
    FrameSettings settings;
    settings.background = rgba(0, 0, 0);
    auto first = gpu->renderFrame(planCamera(), settings);
    ASSERT_TRUE(first.ok());
    EXPECT_TRUE(first->uploaded);
    const katana::math::Vec3 origin = gpu->renderer().scene().origin;

    // The grid moved down to row 33 (world y = -9.5), still passing under the
    // surface; the surface is not sent again.
    const DrawList moved =
        lineAt(Vec3(-30.5, -9.5, 0.0), Vec3(30.5, -9.5, 0.0), rgba(255, 255, 255), 1.0f);
    gpu->renderer().updateLayer(0, moved);
    Image pixels;
    auto second = gpu->renderFrame(planCamera(), settings, &pixels);
    ASSERT_TRUE(second.ok());
    EXPECT_TRUE(second->uploaded);
    EXPECT_EQ(second->uploadedBytes, 2 * sizeof(katana::qt::gpu::GpuVertex) +
                                         sizeof(katana::qt::gpu::GpuLine));
    EXPECT_EQ(gpu->renderer().layerCount(), 2u);
    EXPECT_EQ(gpu->renderer().scene().origin, origin);
    EXPECT_GT(katana::qt::gpu::testing::brightness(pixelAt(pixels, 4, 33)), 600);
    // Still a backdrop: under the surface the moved grid does not show.
    EXPECT_LT(katana::render::redOf(pixelAt(pixels, 32, 33)), 60);
    EXPECT_GT(katana::render::greenOf(pixelAt(pixels, 32, 33)), 150);

    // A frame that changes nothing uploads nothing.
    auto third = gpu->renderFrame(planCamera(), settings);
    ASSERT_TRUE(third.ok());
    EXPECT_FALSE(third->uploaded);
}

// Zoomed in close to a survey far from its scene's centre, the float offsets
// packed against that centre round to more than the view is wide; packed
// against the pivot, as RenderViewWidget packs them once the bound there
// passes kOriginErrorPixels (scene_origin.hpp, "The origin follows a deep
// zoom"), the survey is drawn where it is. One stray point at the origin
// puts the centre of the scene's box 3100 km from a line at MGA
// coordinates, where a float steps in 0.25 m; the view is 48 mm tall, a
// millimetre a pixel, and the line runs 10.5 mm north of the pivot: along
// the centre of row 23.5 - 10.5 = 13.
TEST_P(GpuLayers, LayersPackedAgainstThePivotDrawASurveyZoomedInCloseWhereItIs)
{
    auto gpu = device();
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    const Vec3 pivot(300000.3, 6200000.3, 30.0);
    const DrawList line = lineAt(pivot + Vec3(-0.04, 0.0105, 0.0), pivot + Vec3(0.04, 0.0105, 0.0),
                                 rgba(255, 255, 255), 1.0f);
    DrawList stray;
    stray.addPoint(stray.addVertex(Vec3(0.0, 0.0, 30.0), rgba(255, 255, 255)), 3.0f);
    const std::array<LayerSource, 2> layers{LayerSource{&stray, true}, LayerSource{&line, true}};
    Camera camera = planCamera();
    camera.setTarget(pivot);
    camera.setOrthographicHeight(0.048);
    FrameSettings settings;
    settings.background = rgba(0, 0, 0);

    gpu->renderer().setLayers(layers);
    ASSERT_GT(originErrorPixels(camera, gpu->renderer().origin()), kOriginErrorPixels)
        << "the scene's centre is near enough to the pivot, so this proves nothing";
    Image far;
    ASSERT_TRUE(gpu->renderFrame(camera, settings, &far).ok());
    saveForLooking(label("far_origin"), far, kWidth, kHeight);
    ASSERT_LT(brightness(pixelAt(far, 32, 13)), 100)
        << "drawn in its place from 3100 km off, so this proves nothing";

    gpu->renderer().setLayers(layers, pivot);
    EXPECT_EQ(gpu->renderer().origin(), pivot);
    EXPECT_LE(originErrorPixels(camera, pivot), kOriginErrorPixels);
    Image near;
    ASSERT_TRUE(gpu->renderFrame(camera, settings, &near).ok());
    saveForLooking(label("pivot_origin"), near, kWidth, kHeight);
    for (const int column : {4, 32, 60}) {
        EXPECT_GT(brightness(pixelAt(near, column, 13)), 600) << column;
        EXPECT_LT(brightness(pixelAt(near, column, 11)), 100) << column;
        EXPECT_LT(brightness(pixelAt(near, column, 15)), 100) << column;
    }
}
