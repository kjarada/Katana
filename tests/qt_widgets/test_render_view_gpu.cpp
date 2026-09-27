// The 3D view drawn on the GPU (render_view_widget.hpp, "TWO RENDERERS";
// docs/gpu.md, "Hosting").
//
// Only where a GPU view can be shown: a desktop platform, with the renderer
// built in. ctest runs these on Linux on xcb under Xvfb, with
// KATANA_RENDERER=gpu so that lavapipe may draw them (qt_widgets_gpu.*,
// tests/qt_widgets/CMakeLists.txt); on Windows, run the binary with
// KATANA_WIDGET_TEST_PLATFORM=windows by hand. Offscreen, where the rest of
// this suite runs, they skip - and there the software view is what every
// other case tests. They skip, too, on a machine with no Vulkan driver at all.
//
// Each case runs in a process of its own under ctest, which matters to the
// last: a GPU view that failed is not tried again for the rest of the
// session.

#include <gtest/gtest.h>

#include <cstddef>
#include <string>

#include <QApplication>
#include <QImage>

#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/render/framebuffer.hpp"
#include "render_view_widget.hpp"
#include "widget_harness.hpp"

#if defined(KATANA_HAS_GPU)
#include "gpu/gpu_scene_view.hpp"
#include "gpu_test_support.hpp"
#endif

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::geometry::Point2;
using katana::qt::RenderViewWidget;
using katana::qt::ViewContext;
using katana::qt::test::processEvents;

namespace {

// renderer_choice.cpp's rule 5, for the backend this build carries. These helpers are
// used only in a build with a GPU backend, hence [[maybe_unused]].
[[maybe_unused]] bool gpuCanBeShown()
{
    const QString platform = QGuiApplication::platformName();
#if defined(KATANA_GPU_VULKAN)
    return platform == QStringLiteral("xcb") || platform == QStringLiteral("wayland");
#elif defined(KATANA_GPU_D3D11)
    return platform == QStringLiteral("windows");
#elif defined(KATANA_GPU_METAL)
    return platform == QStringLiteral("cocoa");
#else
    (void)platform;
    return false;
#endif
}

#if defined(KATANA_HAS_GPU)
// Empty when these cases can run: a platform the GPU view can be shown on,
// and a driver to draw with (the gpu suite's whyNoVulkanDriver).
std::string whyNotShown()
{
    if (!gpuCanBeShown()) {
        return "needs a platform the GPU view can be shown on; this is " +
               QGuiApplication::platformName().toStdString();
    }
    return katana::qt::gpu::testing::whyNoVulkanDriver();
}
#endif

// Pixels of `frame` that are not the view's background (28, 30, 36).
[[maybe_unused]] std::size_t drawnPixels(const QImage& frame)
{
    const QImage rgb = frame.convertToFormat(QImage::Format_ARGB32);
    std::size_t drawn = 0;
    for (int y = 0; y < rgb.height(); ++y) {
        for (int x = 0; x < rgb.width(); ++x) {
            drawn += (rgb.pixel(x, y) & 0xFFFFFFu) != (qRgb(28, 30, 36) & 0xFFFFFFu) ? 1 : 0;
        }
    }
    return drawn;
}

// With KATANA_GPU_TEST_IMAGES set to a directory, saves `frame` there as
// <name>.png, as the GPU suite does (docs/gpu.md, "Testing"): the way to
// look at what the hosted view drew.
[[maybe_unused]] void saveForLooking(const char* name, const QImage& frame)
{
    const QByteArray directory = qgetenv("KATANA_GPU_TEST_IMAGES");
    if (!directory.isEmpty()) {
        (void)frame.save(QString::fromLocal8Bit(directory) + "/" + name + ".png");
    }
}

struct OneLine {
    Document document;
    ViewSet views;

    OneLine()
    {
        EXPECT_TRUE(
            document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 50.0)))
                .ok());
    }
    [[nodiscard]] ViewContext context()
    {
        ViewContext context;
        context.document = &document;
        return context;
    }
};

} // namespace

#if defined(KATANA_HAS_GPU)

// The host builds its layers as for the software view and its GPU child
// draws them, framed by the host: one line from (0, 0) to (100, 50) at the
// entity elevation 0, whose box's centre, (50, 25, 0), Camera::frame makes
// the target (as test_render_view.cpp works out for the software view).
TEST(RenderViewGpu, OnTheDesktopTheHostsLayersAreDrawnByItsGpuChild)
{
    if (const std::string why = whyNotShown(); !why.empty()) {
        GTEST_SKIP() << why;
    }
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(320, 200);
    view.show();
    processEvents();
    ASSERT_TRUE(view.drawnOnGpu()) << view.rendererReason().toStdString();
    auto* gpuView = view.gpuView();
    ASSERT_NE(gpuView, nullptr);

    const QImage frame = gpuView->grabFramebuffer();
    ASSERT_FALSE(gpuView->failed());
    ASSERT_FALSE(frame.isNull());
    EXPECT_GT(drawnPixels(frame), 100u) << "the line and the grid under it";
    saveForLooking("render_view_gpu", frame);
    // Five layers, as cad::renderLayers draws them; the line is the drawing's.
    EXPECT_GE(gpuView->lastStats().lines, 1u);
    EXPECT_DOUBLE_EQ(state.camera.target().x, 50.0);
    EXPECT_DOUBLE_EQ(state.camera.target().y, 25.0);
    EXPECT_DOUBLE_EQ(state.camera.target().z, 0.0);
    EXPECT_TRUE(state.cameraFramed);
    // The GPU child keeps the camera in its logical pixels.
    EXPECT_EQ(state.camera.viewportWidth(), gpuView->width());
}

// An edit reaches the GPU view: the host rebuilds the drawing's layers and
// hands them over at the next frame, the terrain's untouched.
TEST(RenderViewGpu, OnTheDesktopAnEditReachesTheGpuView)
{
    if (const std::string why = whyNotShown(); !why.empty()) {
        GTEST_SKIP() << why;
    }
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    view.resize(320, 200);
    view.show();
    processEvents();
    ASSERT_TRUE(view.drawnOnGpu()) << view.rendererReason().toStdString();
    auto* gpuView = view.gpuView();
    ASSERT_NE(gpuView, nullptr);
    (void)gpuView->grabFramebuffer();
    const std::size_t before = gpuView->lastStats().lines;
    const int builds = view.entityBuilds();

    ASSERT_TRUE(scene.document
                    .execute(katana::commands::createLine(Point2(0.0, 50.0), Point2(100.0, 0.0)))
                    .ok());
    processEvents();
    (void)gpuView->grabFramebuffer();
    EXPECT_GT(view.entityBuilds(), builds);
    EXPECT_GT(gpuView->lastStats().lines, before);
}

// A GPU view that fails hands the view to the software rasteriser, and no
// view of the session tries the GPU again (renderer_choice.hpp, rule 4).
TEST(RenderViewGpu, OnTheDesktopAFailedGpuViewLeavesTheSessionToTheSoftwareRasteriser)
{
    if (const std::string why = whyNotShown(); !why.empty()) {
        GTEST_SKIP() << why;
    }
    OneLine scene;
    ViewState& state = scene.views.add(ViewKind::Model3D);
    RenderViewWidget view(scene.context(), state);
    QString status;
    view.onStatus = [&status](const QString& message) { status = message; };
    view.resize(320, 200);
    view.show();
    processEvents();
    ASSERT_TRUE(view.drawnOnGpu()) << view.rendererReason().toStdString();
    auto* gpuView = view.gpuView();
    ASSERT_NE(gpuView, nullptr);

    // What the widget raises when its device is lost or a pipeline fails.
    ASSERT_TRUE(static_cast<bool>(gpuView->onRenderFailed));
    gpuView->onRenderFailed(QStringLiteral("a device lost in a test"));
    processEvents();
    EXPECT_FALSE(view.drawnOnGpu());
    EXPECT_NE(status.indexOf(QStringLiteral("a device lost in a test")), -1)
        << status.toStdString();
    // The software rasteriser draws it now, at the display's device pixels.
    katana::qt::test::paint(view);
    EXPECT_FALSE(view.framebuffer().empty());
    const katana::render::Framebuffer& software = view.framebuffer();
    saveForLooking("render_view_software_after_the_gpu_failed",
                   QImage(reinterpret_cast<const uchar*>(software.color().data()),
                          software.width(), software.height(),
                          software.width() * static_cast<int>(sizeof(katana::render::Rgba)),
                          QImage::Format_ARGB32)
                       .copy());
    EXPECT_GT(view.lastStats().linesSubmitted, 0u);

    ViewState& other = scene.views.add(ViewKind::Model3D);
    RenderViewWidget later(scene.context(), other);
    EXPECT_FALSE(later.drawnOnGpu());
    EXPECT_NE(later.rendererReason().indexOf(QStringLiteral("failed earlier")), -1)
        << later.rendererReason().toStdString();
}

#endif // KATANA_HAS_GPU
