#include "gpu_scene_view.hpp"

#include <chrono>
#include <cmath>

#include <QKeyEvent>
#include <QMouseEvent>
#include <QWheelEvent>

#include <rhi/qrhi.h>

namespace katana::qt::gpu {

namespace {

// The same numbers as RenderViewWidget's (render_view_widget.cpp): the two
// widgets must feel identical under the mouse, so a change to one belongs in
// both. Radians per logical pixel of drag, and the dolly factor per notch.
constexpr double kOrbitPerPixel = 0.008;
constexpr double kZoomPerNotch = 1.15;

} // namespace

GpuSceneView::GpuSceneView(katana::render::Camera& camera, QWidget* parent)
    : QRhiWidget(parent), camera_(camera)
{
    setApi(QRhiWidget::Api::Direct3D11);
    setSampleCount(4);
    // Our own target: the automatic one's depth buffer is not a float one.
    setAutoRenderTarget(false);
    setFocusPolicy(Qt::StrongFocus);
    setMinimumSize(40, 40);
    QObject::connect(this, &QRhiWidget::renderFailed, this, [this] {
        fail(QStringLiteral("the GPU view could not render (no Direct3D 11 on this platform)"));
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
    update();
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
    framed_ = true;
    if (onZoomExtents) {
        onZoomExtents();
        return;
    }
    katana::math::AABB box = renderer_.scene().bounds;
    if (box.empty()) {
        return;
    }
    // What is drawn is exaggerated in the shader; frame what is drawn.
    const double k = settings_.verticalExaggeration;
    const double datum = settings_.exaggerationDatum;
    const double low = datum + (box.min.z - datum) * k;
    const double high = datum + (box.max.z - datum) * k;
    box.min.z = std::min(low, high);
    box.max.z = std::max(low, high);
    camera_.frame(box);
}

void GpuSceneView::initialize(QRhiCommandBuffer* /*commands*/)
{
    if (failed_) {
        return;
    }
    QRhi* gpu = rhi();
    if (gpu == nullptr || gpu->backend() != QRhi::D3D11) {
        fail(QStringLiteral("the GPU view needs Direct3D 11 (its shaders are HLSL)"));
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
    const QSize size = target_->pixelSize();
    // The camera works in the pixels actually drawn - device pixels - so the
    // image is not upscaled on a HiDPI display, and pan and zoom below scale
    // the mouse's logical pixels up to match.
    camera_.setViewportSize(size.width(), size.height());
    if (!framed_) {
        frameScene(); // no update(): this IS the frame
    }
    FrameSettings settings = settings_;
    settings.pixelRatio = pixelRatio();

    const auto started = std::chrono::steady_clock::now();
    auto result = renderer_.render(commands, target_.get(), camera_, settings);
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

// ---- interaction: RenderViewWidget's, in device pixels ------------------------------

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
        const double ratio = pixelRatio();
        camera_.panPixels(delta.x() * ratio, delta.y() * ratio);
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
    const double ratio = pixelRatio();
    const QPointF position = event->position();
    camera_.dollyAtPixel(factor, position.x() * ratio, position.y() * ratio);
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
    return std::make_unique<GpuSceneView>(camera, parent);
}

} // namespace katana::qt::gpu
