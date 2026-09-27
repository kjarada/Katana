// The GPU widget (gpu/gpu_scene_view.hpp) under the offscreen platform, where
// it cannot render: what it tells its host, and that its mouse moves the
// host's camera exactly as RenderViewWidget's does. The cases named
// "OnTheDesktop..." need a real QRhiWidget frame and run only on a platform
// the build's view can show: windows for the Direct3D 11 build
// (KATANA_GPU_TEST_PLATFORM=windows, by hand - ctest skips them there), xcb
// for the Vulkan one (ctest runs the Linux suite on xcb under Xvfb, so they
// run there, on lavapipe when there is no GPU).

#include <gtest/gtest.h>

#include <cmath>

#include <QApplication>
#include <QImage>
#include <QMouseEvent>
#include <QWheelEvent>

#include "gpu/gpu_scene_view.hpp"
#include "gpu_test_support.hpp"
#include "katana/render/framebuffer.hpp"
#include "katana/render/rasterizer.hpp"

using katana::qt::gpu::GpuSceneView;
using katana::qt::gpu::makeGpuSceneViewIfChosen;
using katana::qt::gpu::RendererDecision;
using katana::qt::gpu::RendererEnvironment;
using katana::render::Camera;

namespace {

void send(QWidget& widget, QEvent::Type type, QPoint at, Qt::MouseButton button,
          Qt::MouseButtons held, Qt::KeyboardModifiers modifiers = Qt::NoModifier)
{
    QMouseEvent event(type, QPointF(at), widget.mapToGlobal(QPointF(at)), button, held,
                      modifiers);
    QApplication::sendEvent(&widget, &event);
}

// A sloping triangle 40 m across and 8 m high about `offset`.
katana::render::DrawList triangleAt(const katana::math::Vec3& offset)
{
    katana::render::DrawList list;
    const auto a = list.addVertex(offset + katana::math::Vec3(-20.0, -20.0, 0.0),
                                  katana::render::rgba(220, 60, 60));
    const auto b = list.addVertex(offset + katana::math::Vec3(20.0, -20.0, 0.0),
                                  katana::render::rgba(60, 220, 60));
    const auto c = list.addVertex(offset + katana::math::Vec3(0.0, 20.0, 8.0),
                                  katana::render::rgba(60, 60, 220));
    list.addTriangle(a, b, c);
    return list;
}

// The platform the build's view can show, as renderer_choice.cpp's rule 5.
bool onDesktop()
{
    const QString platform = QGuiApplication::platformName();
#if defined(KATANA_GPU_VULKAN)
    return platform == QStringLiteral("xcb") || platform == QStringLiteral("wayland");
#elif defined(KATANA_GPU_METAL)
    return platform == QStringLiteral("cocoa");
#else
    return platform == QStringLiteral("windows");
#endif
}

#if defined(KATANA_GPU_VULKAN)
constexpr const char* kNeedsDesktop = "needs the xcb or wayland platform (ctest runs it under Xvfb)";
#elif defined(KATANA_GPU_METAL)
constexpr const char* kNeedsDesktop =
    "needs the cocoa platform (KATANA_GPU_TEST_PLATFORM=cocoa)";
#else
constexpr const char* kNeedsDesktop =
    "needs the windows platform (KATANA_GPU_TEST_PLATFORM=windows)";
#endif

std::size_t drawnPixels(const QImage& grabbed)
{
    const QImage rgb = grabbed.convertToFormat(QImage::Format_ARGB32);
    const QRgb background = qRgb(28, 30, 36);
    std::size_t drawn = 0;
    for (int y = 0; y < rgb.height(); ++y) {
        for (int x = 0; x < rgb.width(); ++x) {
            drawn += (rgb.pixel(x, y) & 0xFFFFFFu) != (background & 0xFFFFFFu) ? 1 : 0;
        }
    }
    return drawn;
}

// saveForLooking for a grabbed widget frame.
void saveGrab(const std::string& name, const QImage& grabbed)
{
    const QImage rgb = grabbed.convertToFormat(QImage::Format_ARGB32);
    katana::qt::gpu::testing::Image pixels(static_cast<std::size_t>(rgb.width()) * rgb.height());
    for (int y = 0; y < rgb.height(); ++y) {
        for (int x = 0; x < rgb.width(); ++x) {
            pixels[static_cast<std::size_t>(y) * rgb.width() + x] = rgb.pixel(x, y);
        }
    }
    katana::qt::gpu::testing::saveForLooking(name, pixels, rgb.width(), rgb.height());
}

} // namespace

TEST(GpuSceneView, UnderTheOffscreenPlatformReportsThatItCannotRenderSoTheHostFallsBack)
{
    if (QGuiApplication::platformName() != QStringLiteral("offscreen")) {
        GTEST_SKIP() << "about the offscreen platform; this run is on "
                     << QGuiApplication::platformName().toStdString();
    }
    Camera camera;
    GpuSceneView view(camera);
    view.resize(160, 100);
    int failures = 0;
    QString reason;
    view.onRenderFailed = [&](const QString& why) {
        ++failures;
        reason = why;
    };
    const QImage grabbed = view.grabFramebuffer();
    EXPECT_TRUE(grabbed.isNull() || grabbed.width() == 0);
    EXPECT_TRUE(view.failed());
    EXPECT_EQ(failures, 1);
    EXPECT_FALSE(reason.isEmpty());
}

TEST(GpuSceneView, TheFactoryBuildsNoGpuViewWhereTheRulesChooseSoftware)
{
    Camera camera;
    RendererEnvironment environment;
    environment.platformName = "offscreen";
    RendererDecision decision;
    auto view = makeGpuSceneViewIfChosen(camera, environment, &decision);
    EXPECT_EQ(view, nullptr);
    EXPECT_NE(decision.reason.find("offscreen"), std::string::npos) << decision.reason;

    environment.platformName = "windows";
    view = makeGpuSceneViewIfChosen(camera, environment, &decision);
    EXPECT_NE(view, nullptr);
}

TEST(GpuSceneView, DraggingOrbitsPansAndTheWheelZoomsTheHostsCameraAsTheSoftwareViewDoes)
{
    Camera camera;
    camera.setViewportSize(200, 100);
    camera.setOrientation(0.0, 0.5);
    camera.setDistance(100.0);
    GpuSceneView view(camera);
    view.resize(200, 100);
    ASSERT_EQ(&view.camera(), &camera);

    // Left drag 50 px right and 25 px down: azimuth -50 * 0.008 = -0.4 rad,
    // elevation +25 * 0.008 = +0.2 rad (RenderViewWidget's kOrbitPerPixel).
    send(view, QEvent::MouseButtonPress, QPoint(100, 50), Qt::LeftButton, Qt::LeftButton);
    send(view, QEvent::MouseMove, QPoint(150, 75), Qt::NoButton, Qt::LeftButton);
    send(view, QEvent::MouseButtonRelease, QPoint(150, 75), Qt::LeftButton, Qt::NoButton);
    EXPECT_NEAR(camera.azimuth(), -0.4, 1e-12);
    EXPECT_NEAR(camera.elevation(), 0.7, 1e-12);

    // An elevation view does not orbit: the same drag pans.
    view.setOrbitAllowed(false);
    const auto before = camera.target();
    send(view, QEvent::MouseButtonPress, QPoint(100, 50), Qt::LeftButton, Qt::LeftButton);
    send(view, QEvent::MouseMove, QPoint(110, 50), Qt::NoButton, Qt::LeftButton);
    send(view, QEvent::MouseButtonRelease, QPoint(110, 50), Qt::LeftButton, Qt::NoButton);
    EXPECT_NEAR(camera.azimuth(), -0.4, 1e-12);
    EXPECT_GT((camera.target() - before).length(), 0.0);

    // One wheel notch in: the distance divides by 1.15.
    const double distance = camera.distance();
    QWheelEvent wheel(QPointF(100, 50), view.mapToGlobal(QPointF(100, 50)), QPoint(),
                      QPoint(0, 120), Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(&view, &wheel);
    EXPECT_NEAR(camera.distance(), distance / 1.15, 1e-9);
}

// Framing an empty draw list frames nothing and leaves the view unframed, so
// the first frame after a list arrives frames that list. It used to count as
// framed before it found the list empty, and a view first drawn before its
// scene was built stayed on the default camera until the user pressed E.
TEST(GpuSceneView, FramingAnEmptyListLeavesTheViewToFrameTheFirstListThatArrives)
{
    Camera camera;
    GpuSceneView view(camera);
    view.resize(200, 100);
    const auto before = camera.target();
    view.zoomExtents();
    EXPECT_FALSE(view.cameraFramed());
    EXPECT_EQ(camera.target(), before);

    // At MGA coordinates: framed means the target moved there.
    view.setDrawList(triangleAt(katana::math::Vec3(300000.0, 6250000.0, 50.0)));
    view.zoomExtents();
    EXPECT_TRUE(view.cameraFramed());
    EXPECT_NEAR(camera.target().x, 300000.0, 20.0);
    EXPECT_NEAR(camera.target().y, 6250000.0, 20.0);
}

// The host's word that its camera is framed stands; and when the host frames
// through its hook while the list is still empty, the first list with
// something in it is framed again, as RenderViewWidget does for its own.
TEST(GpuSceneView, TheHostSaysWhetherItsCameraIsFramedAndAHookFramingNothingIsRedone)
{
    Camera camera;
    GpuSceneView view(camera);
    EXPECT_FALSE(view.cameraFramed());
    view.setCameraFramed(true);
    EXPECT_TRUE(view.cameraFramed());
    view.setCameraFramed(false);
    EXPECT_FALSE(view.cameraFramed());

    int hookCalls = 0;
    view.onZoomExtents = [&hookCalls] { ++hookCalls; };
    view.zoomExtents(); // the list is empty
    EXPECT_EQ(hookCalls, 1);
    EXPECT_FALSE(view.cameraFramed()) << "a frame of nothing is not worth keeping";
    view.setDrawList(triangleAt(katana::math::Vec3()));
    EXPECT_FALSE(view.cameraFramed()) << "the list with something in it is still to be framed";
    view.zoomExtents();
    EXPECT_EQ(hookCalls, 2);
    EXPECT_TRUE(view.cameraFramed());
}

// The host's camera is kept in the widget's logical pixels, where the
// software view keeps it: resizing the widget sizes the camera so.
TEST(GpuSceneView, ResizingKeepsTheHostsCameraInTheWidgetsLogicalPixels)
{
    Camera camera;
    GpuSceneView view(camera);
    view.resize(300, 150);
    QResizeEvent resized(QSize(300, 150), QSize(0, 0));
    QApplication::sendEvent(&view, &resized);
    EXPECT_EQ(camera.viewportWidth(), 300);
    EXPECT_EQ(camera.viewportHeight(), 150);
}

// Only on the desktop platform (KATANA_GPU_TEST_PLATFORM=windows; ctest
// runs offscreen, where it skips): the widget itself draws a scene through
// its host's camera, which the offscreen tests can only prove of the renderer.
TEST(GpuSceneView, OnTheDesktopDrawsTheSceneThroughTheHostsCamera)
{
    if (!onDesktop()) {
        GTEST_SKIP() << kNeedsDesktop;
    }
    katana::render::DrawList list;
    const auto a = list.addVertex(katana::math::Vec3(-20.0, -20.0, 0.0), katana::render::rgba(220, 60, 60));
    const auto b = list.addVertex(katana::math::Vec3(20.0, -20.0, 0.0), katana::render::rgba(60, 220, 60));
    const auto c = list.addVertex(katana::math::Vec3(0.0, 20.0, 8.0), katana::render::rgba(60, 60, 220));
    list.addTriangle(a, b, c);
    list.addSegment(katana::math::Vec3(-20.0, -20.0, 0.0), katana::math::Vec3(0.0, 20.0, 8.0),
                    katana::render::rgba(255, 255, 255), 2.0f);

    Camera camera;
    camera.setStandardView(katana::render::StandardView::IsoSouthWest);
    GpuSceneView view(camera);
    view.setSoftwareDeviceAllowed(true); // the widget is under test, not the device policy
    view.resize(320, 200);
    QString failure;
    view.onRenderFailed = [&failure](const QString& why) { failure = why; };
    view.setDrawList(list);
    const QImage grabbed = view.grabFramebuffer();
    ASSERT_FALSE(view.failed()) << failure.toStdString();
    ASSERT_FALSE(grabbed.isNull());
    // The host's camera stays in the widget's logical pixels; the frame was
    // drawn in its device pixels.
    EXPECT_EQ(camera.viewportWidth(), view.width());
    EXPECT_EQ(camera.viewportHeight(), view.height());
    EXPECT_EQ(grabbed.width(),
              static_cast<int>(std::lround(view.width() * view.devicePixelRatioF())));
    EXPECT_EQ(view.lastStats().triangles, 1u);
    EXPECT_EQ(view.lastStats().lines, 1u);
    // Framed on its first frame, so the triangle is on screen.
    EXPECT_GT(drawnPixels(grabbed),
              static_cast<std::size_t>(grabbed.width() * grabbed.height() / 20));
    saveGrab("scene_view_desktop", grabbed);
}

// A GPU view made over a ViewState whose camera the user has already orbited
// and zoomed - a renderer switch, a re-tile, a re-dock - keeps that view
// through its first frame when the host says the camera is framed. It used to
// frame the scene on every first frame, through the host's own zoomExtents.
TEST(GpuSceneView, OnTheDesktopAHostCameraAlreadyFramedKeepsItsViewThroughTheFirstFrame)
{
    if (!onDesktop()) {
        GTEST_SKIP() << kNeedsDesktop;
    }
    Camera camera;
    camera.setTarget(katana::math::Vec3(-15.0, -15.0, 0.0));
    camera.setDistance(12.0);
    camera.setOrientation(0.7, 0.5);
    const Camera kept = camera;
    // The host's zoomExtents, as the hosting recipe wires it: frames the
    // triangle's box, whose centre is (0, 0, 4).
    const auto hostFrames = [](Camera& framed) {
        framed.frame(katana::math::AABB(katana::math::Vec3(-20.0, -20.0, 0.0),
                                        katana::math::Vec3(20.0, 20.0, 8.0)));
    };

    GpuSceneView view(camera);

    view.setSoftwareDeviceAllowed(true); // the widget is under test, not the device policy
    view.resize(320, 200);
    int hookCalls = 0;
    view.onZoomExtents = [&] {
        ++hookCalls;
        hostFrames(camera);
    };
    view.setCameraFramed(true);
    view.setDrawList(triangleAt(katana::math::Vec3()));
    ASSERT_FALSE(view.grabFramebuffer().isNull());
    ASSERT_FALSE(view.failed());
    EXPECT_EQ(hookCalls, 0);
    EXPECT_EQ(camera.target(), kept.target());
    EXPECT_EQ(camera.distance(), kept.distance());
    EXPECT_EQ(camera.azimuth(), kept.azimuth());
    EXPECT_EQ(camera.elevation(), kept.elevation());
    EXPECT_TRUE(view.cameraFramed());

    // The control: the same camera, not said to be framed, is framed by the
    // host's hook - its target moves from (-15, -15, 0) to (0, 0, 4).
    Camera fresh = kept;
    GpuSceneView other(fresh);
    other.setSoftwareDeviceAllowed(true); // the widget is under test, not the device policy
    other.resize(320, 200);
    other.onZoomExtents = [&] { hostFrames(fresh); };
    other.setDrawList(triangleAt(katana::math::Vec3()));
    ASSERT_FALSE(other.grabFramebuffer().isNull());
    EXPECT_NEAR((fresh.target() - katana::math::Vec3(0.0, 0.0, 4.0)).length(), 0.0, 1e-9);
}

// A view drawn before its scene is built frames the scene when it arrives.
// It used to take its first, empty, frame for its framing, and showed a site
// at MGA coordinates nowhere near the default camera: a blank view.
//
// The view is SHOWN (a window flashes up for a moment), because it is drawn
// twice: grabbing a QRhiWidget that was never shown gives each grab a new
// QRhi without initialising the widget for it, and every grab after the
// first reads back nothing - Qt's behaviour, measured here: all zeros, 3 s
// a grab, where a shown widget's grabs take 2-5 ms and read back the frame.
TEST(GpuSceneView, OnTheDesktopADrawListArrivingAfterTheFirstFrameIsFramed)
{
    if (!onDesktop()) {
        GTEST_SKIP() << kNeedsDesktop;
    }
    Camera camera;
    GpuSceneView view(camera);
    view.setSoftwareDeviceAllowed(true); // the widget is under test, not the device policy
    view.resize(320, 200);
    view.show();
    QApplication::processEvents();
    ASSERT_FALSE(view.grabFramebuffer().isNull()); // a frame with nothing yet
    ASSERT_FALSE(view.failed());
    EXPECT_EQ(view.lastStats().triangles, 0u);
    EXPECT_FALSE(view.cameraFramed());

    const katana::math::Vec3 site(300000.0, 6250000.0, 50.0);
    view.setDrawList(triangleAt(site));
    const QImage grabbed = view.grabFramebuffer();
    ASSERT_FALSE(grabbed.isNull());
    saveGrab("scene_view_list_after_first_frame", grabbed);
    EXPECT_EQ(view.lastStats().triangles, 1u);
    EXPECT_TRUE(view.cameraFramed());
    EXPECT_NEAR(camera.target().x, site.x, 20.0);
    EXPECT_NEAR(camera.target().y, site.y, 20.0);
    // Framed, the triangle is a good part of the frame; unframed, the camera
    // looks at the origin, 6 250 km from it, and nothing is drawn. (An image
    // read back as nothing, all zeros, would count every pixel as drawn: the
    // background's own count rules that out.)
    const std::size_t pixels = static_cast<std::size_t>(grabbed.width() * grabbed.height());
    const std::size_t drawn = drawnPixels(grabbed);
    EXPECT_GT(drawn, pixels / 20);
    EXPECT_GT(pixels - drawn, pixels / 4) << "no background: the frame did not come back";
}

// The software view taking over after a failure paints with the same camera,
// through a rasteriser that refuses a camera whose viewport is not its
// framebuffer's size - the widget's LOGICAL size. The GPU frame, drawn in
// device pixels, must leave the camera there. (On a display at 100% the two
// sizes are the same and this cannot tell them apart; this laptop's is at
// 125%.)
TEST(GpuSceneView, OnTheDesktopAFrameLeavesTheHostsCameraReadyForTheSoftwareView)
{
    if (!onDesktop()) {
        GTEST_SKIP() << kNeedsDesktop;
    }
    const katana::render::DrawList list = triangleAt(katana::math::Vec3());
    Camera camera;
    GpuSceneView view(camera);
    view.setSoftwareDeviceAllowed(true); // the widget is under test, not the device policy
    view.resize(320, 200);
    view.setDrawList(list);
    const QImage grabbed = view.grabFramebuffer();
    ASSERT_FALSE(grabbed.isNull());
    ASSERT_FALSE(view.failed());

    auto framebuffer = katana::render::Framebuffer::create(view.width(), view.height());
    ASSERT_TRUE(framebuffer.ok());
    katana::render::Rasterizer rasterizer;
    const auto painted = rasterizer.render(list, camera, *framebuffer);
    EXPECT_TRUE(painted.ok()) << (painted.ok() ? "" : painted.error().describe())
                              << " (the GPU frame was " << grabbed.width() << "x"
                              << grabbed.height() << ")";
}

// A software device - lavapipe, WARP - runs the GPU's work on the CPU, which
// the rasteriser was written to do better; so the view refuses one as a
// failure, the host falls back, and only KATANA_RENDERER=gpu
// (setSoftwareDeviceAllowed) lets it draw. Testable only where the software
// device is what the view would get - a desktop with no GPU, or Xvfb.
TEST(GpuSceneView, OnTheDesktopASoftwareDeviceIsRefusedUnlessAllowed)
{
    if (!onDesktop()) {
        GTEST_SKIP() << kNeedsDesktop;
    }
    // The view and a hardware OffscreenGpu ask QRhi for the same default
    // device, so this says which kind the view will get.
    katana::qt::gpu::OffscreenOptions hardware;
    hardware.device = katana::qt::gpu::GpuDevice::Hardware;
    if (auto gpu = katana::qt::gpu::OffscreenGpu::create(8, 8, hardware)) {
        GTEST_SKIP() << "this machine has a hardware device (" << (*gpu)->deviceName()
                     << "); a software one is refused only when it is what the view gets";
    }

    Camera camera;
    GpuSceneView refusing(camera);
    refusing.resize(64, 48);
    QString reason;
    refusing.onRenderFailed = [&reason](const QString& why) { reason = why; };
    refusing.setDrawList(triangleAt(katana::math::Vec3()));
    (void)refusing.grabFramebuffer();
    EXPECT_TRUE(refusing.failed());
    EXPECT_NE(reason.indexOf(QStringLiteral("software")), -1) << reason.toStdString();

    Camera other;
    GpuSceneView allowed(other);
    allowed.setSoftwareDeviceAllowed(true);
    allowed.resize(64, 48);
    QString failure;
    allowed.onRenderFailed = [&failure](const QString& why) { failure = why; };
    allowed.setDrawList(triangleAt(katana::math::Vec3()));
    const QImage grabbed = allowed.grabFramebuffer();
    EXPECT_FALSE(allowed.failed()) << failure.toStdString();
    EXPECT_GT(drawnPixels(grabbed), 0u);
}
