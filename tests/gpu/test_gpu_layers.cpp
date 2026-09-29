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
#include "gpu_test_support.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"

using katana::math::Vec3;
using katana::qt::gpu::FrameSettings;
using katana::qt::gpu::GpuDevice;
using katana::qt::gpu::LayerSource;
using katana::qt::gpu::OffscreenGpu;
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

// The 3D view's selection (cad/selection_style.hpp): a 3 px core of #FF9F1C
// over a 5 px casing of (16, 18, 22), both over the entity's own 1 px line.
// The GPU pulls a mark towards the eye by its size (gpu_renderer.cpp,
// kMarkPull) and reads no draw-list bias, so in ONE layer writing depth the
// wider casing is nearer than the core wherever they overlap and covers it,
// whichever is drawn first: the selection was a dark line. In a layer of its
// own, before the core's and writing no depth, the casing is covered by the
// core drawn after it and shows only either side (scene.hpp, SceneLayers).
TEST_P(GpuLayers, ASelectionCoreDrawnAfterACasingThatWritesNoDepthKeepsItsColour)
{
    auto gpu = device();
    if (!gpu) {
        GTEST_SKIP() << skipReason;
    }
    const Rgba orange = rgba(0xFF, 0x9F, 0x1C);
    const Rgba dark = rgba(16, 18, 22);
    const Vec3 from(-10.5, 0.5, 0.0);
    const Vec3 to(10.5, 0.5, 0.0);
    const DrawList surface = flatSurface();
    const DrawList drawing = lineAt(from, to, rgba(220, 220, 220), 1.0f);
    const DrawList casing = lineAt(from, to, dark, 5.0f);
    const DrawList core = lineAt(from, to, orange, 3.0f);

    const std::array<LayerSource, 4> layered{LayerSource{&surface, true},
                                             LayerSource{&drawing, true},
                                             LayerSource{&casing, false},
                                             LayerSource{&core, true}};
    const Image drawn = render(*gpu, layered);
    saveForLooking(label("layers_selection_casing"), drawn, kWidth, kHeight);
    // The line is on row 23 (world y = 0.5). The core covers rows 22-24, the
    // casing rows 21-25.
    const Rgba centre = pixelAt(drawn, 32, 23);
    EXPECT_GT(katana::render::redOf(centre), 220);
    EXPECT_GT(katana::render::greenOf(centre), 120);
    EXPECT_LT(katana::render::greenOf(centre), 190);
    EXPECT_LT(katana::render::blueOf(centre), 80);
    // Two rows off the centre, inside the casing and outside the core: dark,
    // well under half the core's brightness (255 + 159 + 28 = 442).
    for (const int row : {21, 25}) {
        EXPECT_LT(katana::qt::gpu::testing::brightness(pixelAt(drawn, 32, row)), 221)
            << "row " << row;
    }

    // The control: core then casing in one layer writing depth, as a scene
    // with no casing layer of its own would send them.
    DrawList both;
    both.addSegment(from, to, orange, 3.0f);
    both.addSegment(from, to, dark, 5.0f);
    const std::array<LayerSource, 3> oneLayer{LayerSource{&surface, true},
                                              LayerSource{&drawing, true},
                                              LayerSource{&both, true}};
    const Image control = render(*gpu, oneLayer);
    EXPECT_LT(katana::qt::gpu::testing::brightness(pixelAt(control, 32, 23)), 221)
        << "without the rule the casing is expected to cover the core; if it no longer does, "
           "this case no longer tests the rule";
}
