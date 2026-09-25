#include "gpu_scene_view.hpp"

#include <chrono>
#include <cmath>

#include <QKeyEvent>
#include <QMouseEvent>
#include <QResizeEvent>
#include <QWheelEvent>

#include <rhi/qrhi.h>

namespace katana::qt::gpu {

namespace {

[[nodiscard]] GpuBackend expectedBackend()
{
#if defined(KATANA_GPU_VULKAN)
    return GpuBackend::Vulkan;
#else
    return GpuBackend::Direct3D11;
#endif
}

// The same numbers as RenderViewWidget's (render_view_widget.cpp): the two
// widgets must feel identical under the mouse, so a change to one belongs in
// both. Radians per logical pixel of drag, and the dolly factor per notch.
constexpr double kOrbitPerPixel = 0.008;
constexpr double kZoomPerNotch = 1.15;

} // namespace

GpuSceneView::GpuSceneView(katana::render::Camera& camera, QWidget* parent)
    : QRhiWidget(parent), camera_(camera)
{
#if defined(KATANA_GPU_VULKAN)
    setApi(QRhiWidget::Api::Vulkan);
#else
    setApi(QRhiWidget::Api::Direct3D11);
#endif
    setSampleCount(4);
    // Our own target: the automatic one's depth buffer is not a float one.
    setAutoRenderTarget(false);
    setFocusPolicy(Qt::StrongFocus);
    setMinimumSize(40, 40);
    QObject::connect(this, &QRhiWidget::renderFailed, this, [this] {
        // Qt says only that it has no QRhi for the widget: no device of the
        // API here, or a window that is never shown (a headless run).
        fail(QStringLiteral("the GPU view could not render: Qt made it no %1 device - none "
                            "here, or its window is never shown")
                 .arg(QString::fromLatin1(toString(expectedBackend()))));
    });
}

GpuSceneView::~GpuSceneView() = default;

void GpuSceneView::fail(const QString& reason)
{
    if (failed_) {
        return;
    }
    failed_ = true;
    if (onRenderFailed) {
        onRenderFailed(reason);
    }
}

double GpuSceneView::pixelRatio() const
{
    if (target_ != nullptr && width() > 0) {
        return static_cast<double>(target_->pixelSize().width()) / static_cast<double>(width());
    }
    return devicePixelRatioF();
}

void GpuSceneView::setDrawList(const katana::render::DrawList& list)
{
    renderer_.setDrawList(list);
    // The last framing was of nothing (the host's hook framed an empty
    // scene): this list is the first with something in it, so it is framed
    // at the next frame, as RenderViewWidget frames its first non-empty scene.
    if (framedEmpty_ && !renderer_.sceneBounds().empty()) {
        framed_ = false;
        framedEmpty_ = false;
    }
    update();
}

void GpuSceneView::setLayers(std::span<const LayerSource> layers)
{
    renderer_.setLayers(layers);
    // As setDrawList: the first scene with something in it after an empty
    // framing is framed at the next frame.
    if (framedEmpty_ && !renderer_.sceneBounds().empty()) {
        framed_ = false;
        framedEmpty_ = false;
    }
    // From onPrepareFrame the frame being drawn uploads it: asking for
    // another would draw every change twice.
    if (!inFrame_) {
        update();
    }
}

void GpuSceneView::updateLayer(std::size_t index, const katana::render::DrawList& list)
{
    renderer_.updateLayer(index, list);
    if (!inFrame_) {
        update();
    }
}

void GpuSceneView::setCameraFramed(bool framed)
{
    framed_ = framed;
    framedEmpty_ = false;
}

void GpuSceneView::setFrameSettings(const FrameSettings& settings)
{
    settings_ = settings;
    update();
}

void GpuSceneView::zoomExtents()
{
    frameScene();
    update();
}

void GpuSceneView::frameScene()
{
    keepCameraLogical(); // frame() fits the camera's aspect
    if (onZoomExtents) {
        // The host frames whatever it frames for an empty scene, and asks
        // for a repaint: framed_ is set even then, or every frame would call
        // it again. framedEmpty_ has the next non-empty list framed anyway.
        // Read after the hook: the host may hand over its scene as it frames.
        onZoomExtents();
        framed_ = true;
        framedEmpty_ = renderer_.sceneBounds().empty();
        return;
    }
    katana::math::AABB box = renderer_.sceneBounds();
    if (box.empty()) {
        // Nothing to frame yet: framed_ stays as it was, so a first frame
        // that came before the draw list leaves the framing to the frame
        // after it arrives, rather than to the user pressing E.
        return;
    }
    // What is drawn is exaggerated in the shader; frame what is drawn.
    const double k = settings_.verticalExaggeration;
    const double datum = settings_.exaggerationDatum;
    const double low = datum + (box.min.z - datum) * k;
    const double high = datum + (box.max.z - datum) * k;
    box.min.z = std::min(low, high);
    box.max.z = std::max(low, high);
    if (camera_.frame(box)) {
        framed_ = true;
        framedEmpty_ = false;
    }
}

void GpuSceneView::keepCameraLogical()
{
    camera_.setViewportSize(std::max(width(), 1), std::max(height(), 1));
}

void GpuSceneView::resizeEvent(QResizeEvent* event)
{
    QRhiWidget::resizeEvent(event);
    keepCameraLogical();
}

void GpuSceneView::initialize(QRhiCommandBuffer* /*commands*/)
{
    if (failed_) {
        return;
    }
    QRhi* gpu = rhi();
#if defined(KATANA_GPU_VULKAN)
    constexpr QRhi::Implementation kBackend = QRhi::Vulkan;
#else
    constexpr QRhi::Implementation kBackend = QRhi::D3D11;
#endif
    if (gpu == nullptr || gpu->backend() != kBackend) {
        fail(QStringLiteral("the GPU view needs %1, which its shaders are built for")
                 .arg(QString::fromLatin1(toString(expectedBackend()))));
        return;
    }
    if (!softwareDeviceAllowed_ && gpu->driverInfo().deviceType == QRhiDriverInfo::CpuDevice) {
        fail(QStringLiteral("the only %1 device is a software one (%2); the software "
                            "rasteriser draws faster on the CPU")
                 .arg(QString::fromLatin1(toString(expectedBackend())),
                      QString::fromUtf8(gpu->driverInfo().deviceName)));
        return;
    }
    // Multisampled, the widget draws into msaaColorBuffer() and resolves into
    // resolveTexture(); colorTexture() is null then (QRhiWidget's contract).
    const int samples = std::max(sampleCount(), 1);
    QRhiColorAttachment attachment;
    QSize size;
    if (samples > 1 && msaaColorBuffer() != nullptr && resolveTexture() != nullptr) {
        attachment = QRhiColorAttachment(msaaColorBuffer());
        attachment.setResolveTexture(resolveTexture());
        size = msaaColorBuffer()->pixelSize();
    } else if (colorTexture() != nullptr) {
        attachment = QRhiColorAttachment(colorTexture());
        size = colorTexture()->pixelSize();
    } else {
        fail(QStringLiteral("the GPU view has no colour buffer"));
        return;
    }

    target_.reset();
    depth_.reset(gpu->newTexture(QRhiTexture::D32F, size, samples, QRhiTexture::RenderTarget));
    if (!depth_->create()) {
        fail(QStringLiteral("the GPU view could not create its depth buffer"));
        return;
    }
    QRhiTextureRenderTargetDescription description(attachment);
    description.setDepthTexture(depth_.get());
    target_.reset(gpu->newTextureRenderTarget(description));
    const bool firstTarget = pass_ == nullptr || !renderer_.initialised();
    if (firstTarget) {
        pass_.reset(target_->newCompatibleRenderPassDescriptor());
    }
    target_->setRenderPassDescriptor(pass_.get());
    if (!target_->create()) {
        fail(QStringLiteral("the GPU view could not create its render target"));
        return;
    }
    if (firstTarget) {
        if (auto status = renderer_.initialise(gpu, pass_.get(), samples); !status) {
            fail(QString::fromStdString(status.error().describe()));
        }
    }
}

void GpuSceneView::render(QRhiCommandBuffer* commands)
{
    if (failed_ || target_ == nullptr || !renderer_.initialised()) {
        return;
    }
    keepCameraLogical();
    if (!framed_) {
        frameScene(); // no update(): this IS the frame
    }
    // The frame is drawn in the pixels actually drawn - device pixels - so
    // the image is not upscaled on a HiDPI display. Through a copy: the
    // host's camera stays in logical pixels, where RenderViewWidget and its
    // rasteriser keep it. The same view either way, the aspect being the
    // same to a pixel's rounding; and the mouse moves the host's camera by
    // logical pixels, which move the world as far as the device pixels would.
    const QSize size = target_->pixelSize();
    katana::render::Camera frameCamera = camera_;
    frameCamera.setViewportSize(size.width(), size.height());
    if (onPrepareFrame) {
        inFrame_ = true;
        onPrepareFrame(frameCamera);
        inFrame_ = false;
    }
    FrameSettings settings = settings_;
    settings.pixelRatio = pixelRatio();

    const auto started = std::chrono::steady_clock::now();
    auto result = renderer_.render(commands, target_.get(), frameCamera, settings);
    const auto finished = std::chrono::steady_clock::now();
    if (!result) {
        fail(QString::fromStdString(result.error().describe()));
        return;
    }
    stats_ = *result;
    if (onFrameStats) {
        const double ms = std::chrono::duration<double, std::milli>(finished - started).count();
        onFrameStats(QString("GPU  %1 tri  %2 ms").arg(stats_.triangles).arg(ms, 0, 'f', 1));
    }
}

void GpuSceneView::releaseResources()
{
    renderer_.releaseResources();
    target_.reset();
    depth_.reset();
    pass_.reset();
}

// ---- interaction: RenderViewWidget's, in logical pixels ------------------------------

void GpuSceneView::mousePressEvent(QMouseEvent* event)
{
    if (onActivated) {
        onActivated();
    }
    setFocus(Qt::MouseFocusReason);
    lastMouse_ = event->pos();
    const bool panModifier = (event->modifiers() & Qt::ShiftModifier) != 0;
    if (event->button() == Qt::MiddleButton || (event->button() == Qt::LeftButton && panModifier)) {
        drag_ = Drag::Pan;
    } else if (event->button() == Qt::LeftButton) {
        drag_ = orbitAllowed_ ? Drag::Orbit : Drag::Pan;
    } else {
        drag_ = Drag::None;
    }
}

void GpuSceneView::mouseMoveEvent(QMouseEvent* event)
{
    if (drag_ == Drag::None) {
        return;
    }
    const QPoint delta = event->pos() - lastMouse_;
    lastMouse_ = event->pos();
    if (drag_ == Drag::Orbit) {
        // Radians per LOGICAL pixel: a turn takes the same hand movement at
        // any display scale.
        camera_.orbit(-delta.x() * kOrbitPerPixel, delta.y() * kOrbitPerPixel);
    } else {
        camera_.panPixels(delta.x(), delta.y());
    }
    update();
}

void GpuSceneView::mouseReleaseEvent(QMouseEvent* /*event*/) { drag_ = Drag::None; }

void GpuSceneView::mouseDoubleClickEvent(QMouseEvent* /*event*/) { zoomExtents(); }

void GpuSceneView::wheelEvent(QWheelEvent* event)
{
    const double notches = event->angleDelta().y() / 120.0;
    if (notches == 0.0) {
        return;
    }
    const double factor = std::pow(1.0 / kZoomPerNotch, notches);
    const QPointF position = event->position();
    camera_.dollyAtPixel(factor, position.x(), position.y());
    update();
    event->accept();
}

void GpuSceneView::keyPressEvent(QKeyEvent* event)
{
    using katana::render::StandardView;
    const auto view = [this](StandardView standard) {
        camera_.setStandardView(standard);
        update();
    };
    switch (event->key()) {
    case Qt::Key_1:
        view(StandardView::Top);
        return;
    case Qt::Key_2:
        view(StandardView::Front);
        return;
    case Qt::Key_3:
        view(StandardView::Right);
        return;
    case Qt::Key_4:
        view(StandardView::Back);
        return;
    case Qt::Key_5:
        view(StandardView::Left);
        return;
    case Qt::Key_0:
        view(StandardView::IsoSouthWest);
        return;
    case Qt::Key_P:
        camera_.setProjection(camera_.projection() == katana::render::Projection::Perspective
                                  ? katana::render::Projection::Orthographic
                                  : katana::render::Projection::Perspective);
        update();
        return;
    case Qt::Key_E:
        zoomExtents();
        return;
    default:
        break;
    }
    QRhiWidget::keyPressEvent(event);
}

std::unique_ptr<GpuSceneView> makeGpuSceneViewIfChosen(katana::render::Camera& camera,
                                                       const RendererEnvironment& environment,
                                                       RendererDecision* decision, QWidget* parent)
{
    const RendererDecision chosen = chooseRenderer(environment);
    if (decision != nullptr) {
        *decision = chosen;
    }
    if (chosen.kind != RendererKind::Gpu) {
        return nullptr;
    }
    auto view = std::make_unique<GpuSceneView>(camera, parent);
    view->setSoftwareDeviceAllowed(chosen.softwareDeviceAllowed);
    return view;
}

} // namespace katana::qt::gpu
