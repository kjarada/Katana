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

#include <cmath>
#include <cstddef>
#include <string>

#include <QApplication>
#include <QImage>
#include <QWheelEvent>

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

// Whether anything but the view's background is drawn within `radius`
// pixels of `at`, a point in the frame's own (device) pixels.
[[maybe_unused]] bool drawnNear(const QImage& frame, QPointF at, int radius)
{
    const QImage rgb = frame.convertToFormat(QImage::Format_ARGB32);
    const int cx = static_cast<int>(std::floor(at.x()));
    const int cy = static_cast<int>(std::floor(at.y()));
    for (int y = cy - radius; y <= cy + radius; ++y) {
        for (int x = cx - radius; x <= cx + radius; ++x) {
            if (x >= 0 && y >= 0 && x < rgb.width() && y < rgb.height() &&
                (rgb.pixel(x, y) & 0xFFFFFFu) != (qRgb(28, 30, 36) & 0xFFFFFFu)) {
                return true;
            }
        }
    }
    return false;
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

// The wheel over the GPU child zooms through the host, towards what the host
// built under the cursor: the cursor over the line's point (75, 37.5, 0),
// three quarters along it, which lies 21.7 m beyond the plane of the framed
// target (50, 25, 0) - (25, 12.5, 0) . (1, 1, -1) / sqrt 3 - so that the
// child's own zoom, anchored on that plane, magnified it by 1.124 a notch.
// Through the host each notch divides its depth by 1.15 (docs/render.md,
// "Zooming towards the cursor"), and the frame draws the line under the
// cursor. The GPU child's camera counts in its logical pixels, which are the
// wheel's.
TEST(RenderViewGpu, OnTheDesktopTheWheelOverTheGpuViewZoomsTowardsWhatIsUnderTheCursor)
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
    const katana::math::Vec3 point(75.0, 37.5, 0.0);
    const auto depthOf = [&state](const katana::math::Vec3& p) {
        return (p - state.camera.eye()).dot(state.camera.forward());
    };
    ASSERT_NEAR(depthOf(point) - state.camera.distance(), 21.650635094610966, 1e-9)
        << "the point is on the target's plane, where the child's own zoom was right";
    const auto screen = state.camera.project(point);
    ASSERT_TRUE(screen.has_value());
    const QPointF cursor(screen->x - 0.5, screen->y - 0.5);
    double depth = depthOf(point);
    for (int notch = 1; notch <= 30; ++notch) {
        QWheelEvent wheel(cursor, gpuView->mapToGlobal(cursor), QPoint(), QPoint(0, 120),
                          Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
        QApplication::sendEvent(gpuView, &wheel);
        const double now = depthOf(point);
        ASSERT_NEAR(depth / now, 1.15, 1e-9) << "notch " << notch;
        depth = now;
    }
    const QImage frame = gpuView->grabFramebuffer();
    EXPECT_FALSE(gpuView->failed());
    saveForLooking("render_view_gpu_zoomed", frame);
    EXPECT_TRUE(drawnNear(frame, cursor * gpuView->devicePixelRatioF(), 3))
        << "the line is not drawn under the cursor";
}

// A survey at MGA coordinates with one stray point at the origin: the
// centre of the scene's box, what the GPU packs its layers against at first,
// is 3100 km from the survey, where a float steps in 0.25 m. Looked at from
// 50 m and zoomed 30 notches in towards the line under the cursor, to 0.75 m
// where a pixel is 3 mm, the line drawn from that origin lands far off the
// cursor - without the repack, a stub at the top edge. The host packs the
// layers again against the pivot once the error there passes
// gpu::kOriginErrorPixels (scene_origin.hpp, "The origin follows a deep
// zoom"), and the line is drawn under the cursor.
TEST(RenderViewGpu, OnTheDesktopADeepZoomFarFromTheScenesCentrePacksTheLayersAgainstThePivot)
{
    if (const std::string why = whyNotShown(); !why.empty()) {
        GTEST_SKIP() << why;
    }
    Document document;
    ViewSet views;
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(300000.0, 6200000.0),
                                                          Point2(300100.0, 6200050.0)))
                    .ok());
    ASSERT_TRUE(document.execute(katana::commands::createPoint(Point2(0.0, 0.0))).ok());
    ViewContext context;
    context.document = &document;
    ViewState& state = views.add(ViewKind::Model3D);
    RenderViewWidget view(context, state);
    view.resize(320, 200);
    view.show();
    processEvents();
    ASSERT_TRUE(view.drawnOnGpu()) << view.rendererReason().toStdString();
    auto* gpuView = view.gpuView();
    ASSERT_NE(gpuView, nullptr);
    (void)gpuView->grabFramebuffer();
    const katana::math::Vec3 point(300075.0, 6200037.5, 0.0); // on the line
    ASSERT_GT((gpuView->sceneOrigin() - point).length(), 1.0e6)
        << "the GPU packed the scene near the line already, so this proves nothing";

    state.camera.setTarget(point);
    state.camera.setDistance(50.0);
    gpuView->update();
    (void)gpuView->grabFramebuffer();
    const auto screen = state.camera.project(point);
    ASSERT_TRUE(screen.has_value());
    const QPointF cursor(screen->x - 0.5, screen->y - 0.5);
    for (int notch = 1; notch <= 30; ++notch) {
        QWheelEvent wheel(cursor, gpuView->mapToGlobal(cursor), QPoint(), QPoint(0, 120),
                          Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
        QApplication::sendEvent(gpuView, &wheel);
        (void)gpuView->grabFramebuffer();
    }
    EXPECT_NEAR(state.camera.distance(), 50.0 / std::pow(1.15, 30.0), 1e-6);
    const QImage frame = gpuView->grabFramebuffer();
    ASSERT_FALSE(gpuView->failed());
    saveForLooking("render_view_gpu_deep_zoom", frame);
    // The origin followed the zoom to within the view's reach of the pivot.
    EXPECT_LT((gpuView->sceneOrigin() - state.camera.target()).length(), 50.0);
    EXPECT_TRUE(drawnNear(frame, cursor * gpuView->devicePixelRatioF(), 3))
        << "the line is not drawn under the cursor";
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
