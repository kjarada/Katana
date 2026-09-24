// The GPU widget (gpu/gpu_scene_view.hpp) under the offscreen platform, where
// it cannot render: what it tells its host, and that its mouse moves the
// host's camera exactly as RenderViewWidget's does.

#include <gtest/gtest.h>

#include <cmath>

#include <QApplication>
#include <QImage>
#include <QMouseEvent>
#include <QWheelEvent>

#include "gpu/gpu_scene_view.hpp"

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

} // namespace

TEST(GpuSceneView, UnderTheOffscreenPlatformReportsThatItCannotRenderSoTheHostFallsBack)
{
    ASSERT_EQ(QGuiApplication::platformName(), QStringLiteral("offscreen"));
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
    environment.platformName = QGuiApplication::platformName().toStdString(); // offscreen
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
